mod support;

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use halvo_schema::executable::frame::{Cancel, SecretReply, StoreReply, StoreReplyFrame};
use halvo_schema::executable::record::RUN_LIST_KEYS;
use halvo_schema::executable::{FromRunner, Outcome, RunnerCall, ToRunner};
use halvo_schema::{Category, RequestId};

use support::{is_op, Act, Runner, Supervisor, LONG};

fn deferred() -> Supervisor {
    Supervisor::new(11, Category::TenantRent, Vec::new())
}

#[test]
fn hello_comes_first_then_load_for_the_runs_agent() {
    let sup = deferred();
    let seen = sup.serve(|_, _| Act::Answer, LONG);
    let FromRunner::Hello(hello) = &seen.frames[0] else {
        panic!("the first frame is hello: {:?}", seen.tags())
    };
    assert_eq!(
        (&hello.run, &hello.agent),
        (&sup.run, &sup.agent),
        "hello names the run and the agent of the arguments"
    );
    let calls = seen.calls();
    let RunnerCall::Load(load) = &calls[0] else {
        panic!("the first request is load: {:?}", seen.ops())
    };
    assert_eq!((&load.agent, &load.run), (&sup.agent, &sup.run));
}

#[test]
fn a_category_with_no_task_ends_nothing_to_do_with_its_finish_before_done() {
    let sup = deferred();
    let seen = sup.serve(|_, _| Act::Answer, LONG);
    assert_eq!(
        seen.tags(),
        ["hello", "store", "store", "done"],
        "hello, load, finish, done"
    );
    assert_eq!(seen.ops(), ["load", "finish"]);
    let finish = seen.finish().expect("a finish");
    assert_eq!(finish.outcome, Outcome::NothingToDo);
    assert_eq!(
        (&finish.workflow, &finish.agent, &finish.run),
        (&sup.workflow, &sup.agent, &sup.run)
    );
    assert_eq!(seen.done(), Some(Outcome::NothingToDo));
    assert_eq!(seen.code(), Some(0), "stderr: {}", seen.stderr);
}

#[test]
fn the_finish_record_of_a_run_holds_each_key_of_the_run_s_list_as_null() {
    let sup = deferred();
    let seen = sup.serve(|_, _| Act::Answer, LONG);
    let finish = seen.finish().expect("a finish");
    let stored: serde_json::Value =
        serde_json::from_str(&finish.to_json().expect("the record's text")).expect("json");
    for key in RUN_LIST_KEYS {
        assert_eq!(stored.get(*key), Some(&serde_json::Value::Null), "{key}");
    }
    assert_eq!(seen.code(), Some(0), "stderr: {}", seen.stderr);
}

fn refused_usage(args: &[&str]) -> (Option<i32>, String, bool) {
    let mut runner = Runner::start(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    let frame = runner.recv();
    let status = runner.wait_within(LONG);
    (
        status.and_then(|s| s.code()),
        runner.stderr(),
        frame.is_none(),
    )
}

#[test]
fn arguments_outside_the_form_are_refused_with_usage_and_no_frame() {
    let sup = deferred();
    let good = sup.args();
    let without = |key: &str| -> Vec<String> {
        let at = good.iter().position(|a| a == key).expect("key");
        let mut args = good.clone();
        args.drain(at..at + 2);
        args
    };
    let with = |extra: &[&str]| -> Vec<String> {
        let mut args = good.clone();
        args.extend(extra.iter().map(|s| s.to_string()));
        args
    };
    let mut bad_mode = good.clone();
    let at = bad_mode.iter().position(|a| a == "--mode").expect("mode");
    bad_mode[at + 1] = "teach".into();
    let mut bad_run = good.clone();
    bad_run[1] = "A".repeat(32);
    let cases: Vec<(&str, Vec<String>)> = vec![
        ("no run", without("--run")),
        ("no agent", without("--agent")),
        ("no workflow", without("--workflow")),
        ("no mode", without("--mode")),
        ("a mode outside the list", bad_mode),
        ("a run id in upper case", bad_run),
        ("a key twice", with(&["--mode", "replay"])),
        ("an unknown key", with(&["--standin-seed", "seed.json"])),
        ("a key with no value", with(&["--model"])),
        ("a headed flag twice", with(&["--headed", "--headed"])),
    ];
    for (what, args) in cases {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let (code, stderr, no_frame) = refused_usage(&args);
        assert_eq!(code, Some(2), "{what}: status");
        assert!(no_frame, "{what}: no frame");
        let lines: Vec<&str> = stderr.lines().collect();
        assert_eq!(lines.len(), 1, "{what}: one line: {stderr}");
        assert!(lines[0].starts_with("REFUSED usage: "), "{what}: {stderr}");
    }
}

/// The flag `--version` as the only argument prints the package's version and a line feed on standard output,
/// nothing on standard error, and exits 0 while its standard input stays open with no frame on it: the runner
/// starts no link, so it writes no `hello` and waits for no frame. Beside another argument the flag stays a refusal
/// `usage` with no line on standard output.
#[test]
fn the_version_flag_prints_one_line_and_reads_no_frame() {
    let start = |args: &[&str]| -> (Option<i32>, Vec<u8>, String) {
        let mut child = Command::new(support::RUNNER)
            .args(args)
            .env("TMPDIR", env!("CARGO_TARGET_TMPDIR"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start the runner");
        let open_input = child.stdin.take();
        let started = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().expect("try_wait") {
                break status;
            }
            if started.elapsed() > LONG {
                let _ = child.kill();
                let _ = child.wait();
                panic!("{args:?}: the runner did not end within {LONG:?} with its input open");
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        drop(open_input);
        let mut stdout = Vec::new();
        let mut stderr = String::new();
        child
            .stdout
            .take()
            .expect("the runner's output")
            .read_to_end(&mut stdout)
            .expect("read the output");
        child
            .stderr
            .take()
            .expect("the runner's standard error")
            .read_to_string(&mut stderr)
            .expect("read standard error");
        (status.code(), stdout, stderr)
    };
    let (code, stdout, stderr) = start(&["--version"]);
    assert_eq!(code, Some(0), "status: {stderr}");
    assert_eq!(
        String::from_utf8_lossy(&stdout),
        format!("{}\n", env!("CARGO_PKG_VERSION")),
        "one line, the version"
    );
    assert_eq!(stderr, "", "nothing on standard error");
    let (code, stdout, stderr) = start(&["--version", "--headed"]);
    assert_eq!(code, Some(2), "beside another argument: status: {stderr}");
    assert!(stdout.is_empty(), "beside another argument: no output");
    assert!(
        stderr.starts_with("REFUSED usage: ") && stderr.lines().count() == 1,
        "beside another argument: {stderr}"
    );
}

/// The arguments of `sup` without `--utc-offset` and its value.
fn without_offset(sup: &Supervisor) -> Vec<String> {
    let mut args = sup.args();
    if let Some(at) = args.iter().position(|a| a == "--utc-offset") {
        args.drain(at..at + 2);
    }
    args
}

/// CP-204 D4: a start without `--utc-offset`, or with a value that is no whole number from -50,400 to 50,400 in
/// the form that the library writes, exits with the status 2 and one line before any frame; it never defaults to
/// UTC.
#[test]
fn a_start_without_a_utc_offset_or_with_one_out_of_range_is_refused_before_any_frame() {
    let sup = deferred();
    let mut cases: Vec<(String, Vec<String>, &str)> = vec![(
        "none".into(),
        without_offset(&sup),
        "REFUSED usage: no --utc-offset",
    )];
    for value in [
        "50401",
        "-50401",
        "2147483648",
        "1.5",
        "",
        "x",
        "+3600",
        "0x10",
        "007",
        "-0",
        " 1",
    ] {
        let mut args = without_offset(&sup);
        args.extend(["--utc-offset".to_string(), value.to_string()]);
        cases.push((
            format!("{value:?}"),
            args,
            "REFUSED usage: --utc-offset is out of range",
        ));
    }
    let mut twice = without_offset(&sup);
    twice.extend(["--utc-offset", "0", "--utc-offset", "0"].map(String::from));
    cases.push(("twice".into(), twice, "REFUSED usage: --utc-offset twice"));
    for (what, args, line) in cases {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let (code, stderr, no_frame) = refused_usage(&args);
        assert_eq!(code, Some(2), "{what}: status: {stderr}");
        assert!(no_frame, "{what}: no frame");
        assert_eq!(stderr, format!("{line}\n"), "{what}");
    }
}

/// CP-204 D1: the offsets at the edges of the range and 0 are taken, and the runner says `hello`.
#[test]
fn a_utc_offset_from_minus_50400_to_50400_is_taken() {
    let sup = deferred();
    for value in ["-50400", "-1", "0", "19800", "50400"] {
        let mut args = without_offset(&sup);
        args.extend(["--utc-offset".to_string(), value.to_string()]);
        let mut runner = Runner::start(&args);
        let first = runner.recv();
        runner.close_input();
        runner.close_output();
        assert!(runner.wait_within(LONG).is_some(), "{value}: ended");
        assert!(
            matches!(first, Some(FromRunner::Hello(_))),
            "{value}: hello: {}",
            runner.stderr()
        );
    }
}

#[test]
fn the_optional_arguments_are_taken() {
    let sup = deferred();
    let mut args = sup.args();
    args.extend([
        "--model".to_string(),
        "weights.gguf".into(),
        "--headed".into(),
    ]);
    let mut runner = Runner::start(&args);
    let first = runner.recv();
    assert!(matches!(first, Some(FromRunner::Hello(_))), "hello");
    runner.close_input();
    runner.close_output();
    assert!(runner.wait_within(LONG).is_some());
}

fn after_load(bytes: &[u8]) -> (Option<i32>, String, Vec<&'static str>) {
    let sup = deferred();
    let mut runner = Runner::start(&sup.args());
    let mut tags = Vec::new();
    while let Some(frame) = runner.recv() {
        tags.push(support::tag(&frame));
        if is_op(&frame, "load") {
            runner.write_raw(bytes);
        }
    }
    let status = runner.wait_within(LONG);
    (status.and_then(|s| s.code()), runner.stderr(), tags)
}

fn framed(json: &str) -> Vec<u8> {
    let mut bytes = u32::try_from(json.len())
        .expect("len")
        .to_be_bytes()
        .to_vec();
    bytes.extend_from_slice(json.as_bytes());
    bytes
}

#[test]
fn a_length_over_65536_is_refused_with_frame() {
    let (code, stderr, tags) = after_load(&65_537u32.to_be_bytes());
    assert_eq!(code, Some(2));
    assert!(stderr.starts_with("REFUSED frame: "), "{stderr}");
    assert_eq!(tags, ["hello", "store"], "nothing after the refusal");
}

#[test]
fn a_frame_that_does_not_parse_is_refused_with_frame() {
    for json in [
        r#"{"t":"store_reply"}"#,
        r#"{"t":"store_reply","reply":{"v":1,"id":null,"ok":{"stored":1}},"x":1}"#,
        r#"{"t":"teach"}"#,
        "not json",
    ] {
        let (code, stderr, tags) = after_load(&framed(json));
        assert_eq!(code, Some(2), "{json}");
        assert!(stderr.starts_with("REFUSED frame: "), "{json}: {stderr}");
        assert_eq!(tags, ["hello", "store"], "{json}");
    }
}

#[test]
fn a_reply_that_is_not_the_answer_to_the_request_is_refused_with_frame() {
    let other = ToRunner::StoreReply(StoreReplyFrame {
        reply: StoreReply {
            id: Some(RequestId::from_random([7; 16])),
            outcome: Ok(halvo_schema::executable::RunnerReply::Stored(
                halvo_schema::executable::body::Stored { stored: 1 },
            )),
        },
    });
    let wrong_kind = ToRunner::SecretReply(SecretReply::None);
    for reply in [other, wrong_kind] {
        let json = reply.to_json().expect("json");
        let (code, stderr, _) = after_load(&framed(&json));
        assert_eq!(code, Some(2), "{json}");
        assert!(stderr.starts_with("REFUSED frame: "), "{json}: {stderr}");
    }
}

/// The schema reads the op `checkpoint` since U1.5 brought it (778f8c9a): the same envelope as before, with a
/// well-formed `planned` `AgentCheckpoint` record, parses as `RunnerCall::Checkpoint`. This test takes the place of
/// `a_store_frame_with_the_op_checkpoint_is_refused_by_the_schema_parser`, which held that the schema had no such op
/// (CP-127). This runner sends no checkpoint row yet: `fill::the_fill_playbook_replays_on_seeds_1_to_3` holds the
/// store ops of a fill run, and no `checkpoint` is among them.
#[test]
fn a_store_frame_with_the_op_checkpoint_is_read_by_the_schema_parser_since_u15() {
    let record = format!(
        r#"{{"v":1,"workflow":"{w}","agent":"{a}","run":"{r}","seq":1,"day":20768,"at":1794339000000000001,"folder":["/data/books"],"workbook":["/data/books/bills.xlsx"],"copy":["bills.halvo-checkpoint-{r}.xlsx"],"sha256":null,"bytes":null,"state":"planned","left":null}}"#,
        w = "30".repeat(16),
        a = "40".repeat(16),
        r = "50".repeat(16)
    );
    let body = format!(
        r#"{{"op":"checkpoint","agent":"{a}","run":"{r}","record":{record}}}"#,
        a = "40".repeat(16),
        r = "50".repeat(16)
    );
    let frame = format!(
        r#"{{"t":"store","request":{{"v":1,"id":"{id}","request":"runner","body":{body}}}}}"#,
        id = "3".repeat(32)
    );
    match FromRunner::parse(frame.as_bytes()) {
        Ok(FromRunner::Store(store)) => assert!(
            matches!(store.request.body(), RunnerCall::Checkpoint(_)),
            "the call is checkpoint"
        ),
        Ok(_) => panic!("the frame is a store frame"),
        Err(refusal) => panic!("the schema reads the op checkpoint: {refusal:?}"),
    }
}

#[test]
fn a_cancel_before_the_first_step_ends_the_run_declined_with_no_step() {
    let files = support::FillFiles::new("cancel-first", 1);
    let sup = Supervisor::new(12, Category::SpreadsheetFill, files.slots());
    let before = files.workbook_bytes();
    let seen = sup.serve(
        |frame, _| {
            if is_op(frame, "load") {
                Act::Then(ToRunner::Cancel(Cancel {}), Box::new(Act::Answer))
            } else {
                Act::Answer
            }
        },
        LONG,
    );
    assert_eq!(seen.done(), Some(Outcome::Declined), "{:?}", seen.tags());
    assert_eq!(seen.finish().map(|f| f.outcome), Some(Outcome::Declined));
    assert!(
        !seen.tags().contains(&"progress"),
        "no step began: {:?}",
        seen.tags()
    );
    assert_eq!(files.workbook_bytes(), before);
    let _ = Duration::ZERO;
}

#[test]
fn a_reply_of_another_form_with_the_requests_id_is_refused_with_frame() {
    let files = support::FillFiles::new("other-form", 1);
    let sup = Supervisor::new(13, Category::SpreadsheetFill, files.slots());
    let seen = sup.serve(
        |frame, _| {
            let FromRunner::Store(s) = frame else {
                return Act::Answer;
            };
            if s.request.body().op() != "log" {
                return Act::Answer;
            }
            Act::With(ToRunner::StoreReply(StoreReplyFrame {
                reply: StoreReply {
                    id: Some(s.request.id().clone()),
                    outcome: Ok(halvo_schema::executable::RunnerReply::Steps(
                        halvo_schema::executable::body::StepsReply {
                            agent: s.request.body().agent().clone(),
                            steps: Vec::new(),
                        },
                    )),
                },
            }))
        },
        LONG,
    );
    assert_eq!(seen.code(), Some(2));
    assert!(
        seen.stderr.starts_with("REFUSED frame: "),
        "{}",
        seen.stderr
    );
    assert!(seen.effects().is_empty(), "no effect after the refusal");
}

#[test]
fn a_frame_out_of_turn_is_refused_with_frame_before_any_effect() {
    let files = support::FillFiles::new("out-of-turn", 2);
    let before = files.workbook_bytes();
    let sup = Supervisor::new(14, Category::SpreadsheetFill, files.slots());
    let seen = sup.serve(
        |frame, _| {
            if matches!(frame, FromRunner::Progress(p) if p.step.get() == 2) {
                Act::With(ToRunner::SecretReply(SecretReply::None))
            } else {
                Act::Answer
            }
        },
        LONG,
    );
    assert_eq!(seen.code(), Some(2));
    assert!(
        seen.stderr.starts_with("REFUSED frame: "),
        "{}",
        seen.stderr
    );
    assert!(seen.effects().is_empty());
    assert_eq!(files.workbook_bytes(), before);
}

#[test]
fn a_closed_standard_error_does_not_change_the_end_of_a_run() {
    let sup = deferred();
    let mut runner = Runner::start(&sup.args());
    runner.close_stderr();
    let mut tags = Vec::new();
    while let Some(frame) = runner.recv() {
        tags.push(support::tag(&frame));
        if let Some(reply) = sup.answer(&frame) {
            runner.send(&reply);
        }
    }
    let status = runner.wait_within(LONG);
    assert_eq!(tags, ["hello", "store", "store", "done"]);
    assert_eq!(
        status.and_then(|s| s.code()),
        Some(0),
        "the status of a stored finish"
    );
}

#[test]
fn a_reply_of_the_right_form_for_another_request_id_is_refused_with_frame() {
    let files = support::FillFiles::new("other-id", 1);
    let sup = Supervisor::new(17, Category::SpreadsheetFill, files.slots());
    let seen = sup.serve(
        |frame, _| {
            let FromRunner::Store(s) = frame else {
                return Act::Answer;
            };
            if s.request.body().op() != "log" {
                return Act::Answer;
            }
            Act::With(ToRunner::StoreReply(StoreReplyFrame {
                reply: StoreReply {
                    id: Some(RequestId::from_random([9; 16])),
                    outcome: Ok(sup.store_answer(s.request.body())),
                },
            }))
        },
        LONG,
    );
    assert_eq!(seen.code(), Some(2));
    assert!(
        seen.stderr.starts_with("REFUSED frame: not_the_answer"),
        "{}",
        seen.stderr
    );
    assert!(seen.effects().is_empty(), "no effect after the refusal");
}

#[test]
fn a_load_reply_for_another_agent_or_another_workflow_is_refused_with_frame() {
    let files = support::FillFiles::new("other-agent", 1);
    let sup = Supervisor::new(15, Category::SpreadsheetFill, files.slots());
    for other in ["agent", "workflow"] {
        let seen = sup.serve(
            |frame, _| {
                let FromRunner::Store(s) = frame else {
                    return Act::Answer;
                };
                let mut answer = sup.store_answer(s.request.body());
                if let halvo_schema::executable::RunnerReply::Load(l) = &mut answer {
                    if other == "agent" {
                        l.config.agent = halvo_schema::AgentId::from_random([42; 16]);
                    } else {
                        l.config.workflow = halvo_schema::WorkflowId::from_random([43; 16]);
                    }
                }
                Act::With(ToRunner::StoreReply(StoreReplyFrame {
                    reply: StoreReply {
                        id: Some(s.request.id().clone()),
                        outcome: Ok(answer),
                    },
                }))
            },
            LONG,
        );
        assert_eq!(seen.code(), Some(2), "the {other} half");
        let refusal = if other == "agent" {
            "REFUSED frame: not_the_answer"
        } else {
            "REFUSED frame: load_of_another_agent"
        };
        assert!(
            seen.stderr.starts_with(refusal),
            "the {other} half: {}",
            seen.stderr
        );
        assert_eq!(seen.tags(), ["hello", "store"], "the {other} half");
    }
}
