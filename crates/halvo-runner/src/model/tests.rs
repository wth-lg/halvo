//! The three sentences of r3 U7.2a, each read by a run of a runner in this process over two pipes with a stand-in
//! supervisor on a thread (the form of `a_run_that_finds_no_browser_ends_with_no_browser` of crate::web). The door in
//! them is a STAND-IN: this test binary started as the entry `stand_in_door` alone, which records what it got (its
//! standard input, its arguments, its environment, its working directory and, on macOS, its open descriptors and
//! whether its standard error is the null device) into a directory of the test, answers one line a request line, and
//! opens no socket. No test of this file starts halvo-model: the one test of the tree that starts it from the runner's
//! own start stands in the child module real_door (r6 U5.4); a plain run ignores it, and
//! scripts/r6/ci.d/61-r6-senders-egress.sh runs it under the egress check.

use std::ffi::OsString;
use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};

use halvo_schema::executable::body::{LoadReply, ModelUsage, Stored};
use halvo_schema::executable::frame::{ReplyError, SecretReply, StoreReply, StoreReplyFrame};
use halvo_schema::executable::names::{LogEvent, ModelTier};
use halvo_schema::executable::text::SecretText;
use halvo_schema::executable::{
    Cause, FromRunner, Outcome, RunId, RunnerCall, RunnerReply, ToRunner,
};
use halvo_schema::{AgentId, Category, ErrorName, WorkflowId};

use super::ask::{Grammar, Level, ModelRequest, Place, OUTPUT_LIMIT};
use crate::link::{encode, Link};
use crate::route::{Call, Picked};
use crate::run::Runner;

#[cfg(unix)]
mod real_door;

/// The made-up key of these tests: visible ASCII, held by no file but this one.
const KEY: &str = "made-up-key-u72a-Wq4Zp8Rk-3c1d";

/// The entry of the stand-in door, as the test harness names it.
const STAND_IN: &str = "model::tests::stand_in_door";

/// What the stand-in supervisor answers: the key that the frame `secret` gets (none stored where none), and the
/// error that the store gives a `model_hold` (stored where none).
#[derive(Clone, Copy)]
struct Answers {
    key: Option<&'static str>,
    hold: Option<&'static str>,
}

/// What one run wrote and got: every frame of the runner, parsed and as its bytes; every frame to the runner as its
/// bytes; whether the stand-in had ended when the store got the run's `finish`; the runner's exit status; and the
/// lines that it wrote on standard error.
struct Seen {
    frames: Vec<FromRunner>,
    from_runner: Vec<Vec<u8>>,
    to_runner: Vec<Vec<u8>>,
    ended_at_finish: Option<bool>,
    status: i32,
    lines: Vec<String>,
}

impl Seen {
    /// The store bodies of the model and the `secret` frames, in their order, by a short name.
    fn model_frames(&self) -> Vec<&'static str> {
        self.frames
            .iter()
            .filter_map(|frame| match frame {
                FromRunner::Secret(_) => Some("secret"),
                FromRunner::Store(s) => match s.request.body() {
                    RunnerCall::ModelHold(_) => Some("hold"),
                    RunnerCall::ModelSettle(_) => Some("settle"),
                    _ => None,
                },
                _ => None,
            })
            .collect()
    }

    /// The usage of each `model_settle`, in order.
    fn settles(&self) -> Vec<Option<ModelUsage>> {
        self.frames
            .iter()
            .filter_map(|frame| match frame {
                FromRunner::Store(s) => match s.request.body() {
                    RunnerCall::ModelSettle(b) => Some(b.usage.clone()),
                    _ => None,
                },
                _ => None,
            })
            .collect()
    }

    /// The `body_bytes` of each `model_hold`, in order.
    fn holds(&self) -> Vec<u64> {
        self.frames
            .iter()
            .filter_map(|frame| match frame {
                FromRunner::Store(s) => match s.request.body() {
                    RunnerCall::ModelHold(b) => Some(b.body_bytes),
                    _ => None,
                },
                _ => None,
            })
            .collect()
    }

    /// The outcome of the run's `finish` and of its frame `done`, and the cause of its `stopped` row.
    fn end(&self) -> (Option<Outcome>, Option<Outcome>, Option<Cause>) {
        let mut finish = None;
        let mut stopped = None;
        let mut done = None;
        for frame in &self.frames {
            match frame {
                FromRunner::Store(s) => match s.request.body() {
                    RunnerCall::Finish(f) => finish = Some(f.record.outcome),
                    RunnerCall::Log(b) => {
                        for row in &b.records {
                            if row.event == LogEvent::Stopped {
                                stopped = row.cause;
                            }
                        }
                    }
                    _ => {}
                },
                FromRunner::Done(d) => done = Some(d.outcome),
                _ => {}
            }
        }
        (finish, done, stopped)
    }
}

/// A new, empty directory of a test under the target directory's `tmp`.
fn scratch(name: &str) -> PathBuf {
    let exe = std::env::current_exe().expect("the test binary");
    let target = exe
        .ancestors()
        .nth(3)
        .expect("the target directory above deps");
    halvo_test_dir::make(&target.join("tmp").join("halvo-runner-unit"), name)
        .expect("make the scratch directory")
}

fn request(prompt: &str) -> (Picked, ModelRequest) {
    asked(prompt, Grammar::Action, Level::Medium)
}

/// A request of `grammar` and the pick of a test that it is asked with: the kind of call of that grammar, the tier
/// `frontier` and `level` (crate::route::picked_for_a_test).
fn asked(prompt: &str, grammar: Grammar, level: Level) -> (Picked, ModelRequest) {
    let call = match grammar {
        Grammar::Action => Call::Compile,
        Grammar::Text => Call::Write,
    };
    (
        crate::route::picked_for_a_test(call, ModelTier::Frontier, level),
        ModelRequest {
            system: "Be brief.".to_owned(),
            prompt: prompt.to_owned(),
            snapshot: None,
            grammar,
        },
    )
}

/// What `supervise` gives back: every frame of the runner, parsed and as its bytes; every frame to the runner as its
/// bytes; whether the stand-in door had ended at the `finish`.
type Supervised = (Vec<FromRunner>, Vec<Vec<u8>>, Vec<Vec<u8>>, Option<bool>);

/// The stand-in supervisor and store of a web agent at the tier `frontier` with no stored playbook: it answers every
/// frame that waits for a reply by `answers`, records every frame both ways until the end of the runner's output,
/// notes whether the stand-in door's directory `record` holds its mark `ended` when the store gets the `finish`, and
/// closes the link at a frame that it does not answer.
fn supervise(
    mut from_runner: impl Read,
    mut to_runner: impl Write,
    agent: AgentId,
    workflow: WorkflowId,
    answers: Answers,
    record: PathBuf,
) -> Supervised {
    let mut config =
        halvo_policy::preset(Category::UtilityBillPayment, workflow, agent).expect("a preset");
    config.tier = ModelTier::Frontier;
    let (mut frames, mut from, mut to, mut ended) = (Vec::new(), Vec::new(), Vec::new(), None);
    loop {
        let mut head = [0u8; 4];
        if from_runner.read_exact(&mut head).is_err() {
            break;
        }
        let mut body = vec![0u8; u32::from_be_bytes(head) as usize];
        if from_runner.read_exact(&mut body).is_err() {
            break;
        }
        let frame = FromRunner::parse(&body).expect("the runner writes frames that parse");
        from.push(body);
        let reply = match &frame {
            FromRunner::Secret(_) => ToRunner::SecretReply(match answers.key {
                Some(key) => SecretReply::Stored(SecretText::parse(key).expect("a key")),
                None => SecretReply::None,
            }),
            FromRunner::Store(s) => {
                let outcome = match s.request.body() {
                    RunnerCall::Load(_) => Ok(RunnerReply::Load(LoadReply {
                        config: config.clone(),
                        playbook: None,
                        next_playbook: std::num::NonZeroU32::MIN,
                        bindings: None,
                        sessions: Vec::new(),
                        start: None,
                    })),
                    RunnerCall::Log(b) => Ok(RunnerReply::Stored(Stored {
                        stored: b.records.len() as u32,
                    })),
                    RunnerCall::ModelHold(_) => match answers.hold {
                        Some(name) => Err(ReplyError::Workflow(
                            ErrorName::read(name).expect("an error name"),
                        )),
                        None => Ok(RunnerReply::Stored(Stored { stored: 1 })),
                    },
                    RunnerCall::Finish(_) => {
                        ended = Some(record.join("ended").exists());
                        Ok(RunnerReply::Stored(Stored { stored: 1 }))
                    }
                    _ => Ok(RunnerReply::Stored(Stored { stored: 1 })),
                };
                ToRunner::StoreReply(StoreReplyFrame {
                    reply: StoreReply {
                        id: Some(s.request.id().clone()),
                        outcome,
                    },
                })
            }
            FromRunner::Hello(_) | FromRunner::Progress(_) | FromRunner::Done(_) => {
                frames.push(frame);
                continue;
            }
            _ => {
                frames.push(frame);
                break;
            }
        };
        frames.push(frame);
        let json = reply.to_json().expect("a frame");
        to.push(json.as_bytes().to_vec());
        if to_runner.write_all(&encode(&json)).is_err() {
            break;
        }
    }
    (frames, from, to, ended)
}

/// One compile of the agent of `supervise`, with `--hosted frontier` where `hosted`, whose stand-in drive asks
/// `drive`, with the stand-in door recording into `record` and the run's data directory `data`.
fn run_once(
    hosted: bool,
    answers: Answers,
    drive: Vec<(Picked, ModelRequest)>,
    record: &Path,
    data: &Path,
) -> Seen {
    let mut before: Vec<OsString> = ["--exact", "--test-threads=1", "--quiet", STAND_IN, "--"]
        .into_iter()
        .map(OsString::from)
        .collect();
    before.push(record.as_os_str().to_owned());
    let place = Place {
        program: std::env::current_exe().expect("the test binary"),
        before,
    };
    run_at(place, hosted, answers, drive, record, data)
}

/// One compile of the agent of `supervise` whose door stands at `place`, with `--hosted frontier` where `hosted`,
/// whose stand-in drive asks `drive`; the supervisor reads the door's mark `ended` in `record`, and `data` is the
/// run's data directory.
fn run_at(
    place: Place,
    hosted: bool,
    answers: Answers,
    drive: Vec<(Picked, ModelRequest)>,
    record: &Path,
    data: &Path,
) -> Seen {
    let run = RunId::from_random([0x81; 16]);
    let agent = AgentId::from_random([0x82; 16]);
    let workflow = WorkflowId::from_random([0x83; 16]);
    let data_text = data.to_string_lossy().into_owned();
    let mut words = vec![
        "--run",
        run.as_str(),
        "--agent",
        agent.as_str(),
        "--workflow",
        workflow.as_str(),
        "--mode",
        "compile",
        "--data-dir",
        data_text.as_str(),
        "--utc-offset",
        "0",
    ];
    if hosted {
        words.extend(["--hosted", "frontier"]);
    }
    let words: Vec<OsString> = words.into_iter().map(OsString::from).collect();
    let args = crate::args::parse(&words).expect("the arguments");
    let (to_runner_read, to_runner) = std::io::pipe().expect("a pipe");
    let (from_runner, from_runner_write) = std::io::pipe().expect("a pipe");
    let link = Link::start(
        Box::new(to_runner_read),
        Box::new(from_runner_write),
        Box::new(|| {}),
    );
    let kept = record.to_path_buf();
    let supervisor = std::thread::spawn(move || {
        supervise(from_runner, to_runner, agent, workflow, answers, kept)
    });
    let mut runner = Runner::new(args, link);
    runner.stand_in_door = Some(place);
    runner.drive = drive;
    let _ = crate::tests_lines();
    let status = runner.run();
    let lines = crate::tests_lines();
    let (frames, from_runner, to_runner, ended_at_finish) =
        supervisor.join().expect("the supervisor");
    Seen {
        frames,
        from_runner,
        to_runner,
        ended_at_finish,
        status,
        lines,
    }
}

/// The names of the files of `dir` whose names begin with `started-`: one a start of the stand-in.
fn starts(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .expect("the record directory")
        .filter(|entry| {
            entry
                .as_ref()
                .is_ok_and(|e| e.file_name().to_string_lossy().starts_with("started-"))
        })
        .count()
}

fn recorded(dir: &Path, name: &str) -> String {
    std::fs::read_to_string(dir.join(name)).unwrap_or_else(|_| format!("<no {name}>"))
}

/// How many times `needle` stands in `hay`.
fn count(hay: &[u8], needle: &[u8]) -> usize {
    hay.windows(needle.len()).filter(|w| *w == needle).count()
}

/// Every file under `dir`, with its bytes.
fn files_under(dir: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
    for entry in std::fs::read_dir(dir).expect("a directory of the run") {
        let path = entry.expect("an entry").path();
        if path.is_dir() {
            files_under(&path, out);
        } else {
            let bytes = std::fs::read(&path).expect("a file of the run");
            out.push((path, bytes));
        }
    }
}

/// THE STAND-IN DOOR: in a run of the tests it returns at once; started by a test's runner, with the record
/// directory after `--` and the door's own arguments behind it, it records what it got and answers each request
/// line: the prompt `cut` with the refusal `cut`, the prompt `status` with the refusal `status`, any other with a
/// reply of 11 tokens in and 7 out.
#[test]
fn stand_in_door() {
    let args: Vec<String> = std::env::args().collect();
    let (Some(dash), Some(tier)) = (
        args.iter().position(|a| a == "--"),
        args.iter().position(|a| a == "--tier"),
    ) else {
        return;
    };
    let record = PathBuf::from(&args[dash + 1]);
    let write = |name: &str, bytes: &[u8]| {
        std::fs::write(record.join(name), bytes).expect("a record of the stand-in")
    };
    write(&format!("started-{}", std::process::id()), b"");
    write("argv.txt", args.join("\n").as_bytes());
    write("args.txt", args[tier..].join("\n").as_bytes());
    // On macOS the system's CoreFoundation sets `__CF_USER_TEXT_ENCODING` in a process as it loads, so it stands in
    // this process's environment whatever the start passed; the stand-in leaves that one name out (a named limit:
    // a start that passed it is not seen here; halvo-web's `child::initial_env` reads the start's own block, by
    // unsafe code that this crate forbids).
    let mut env: Vec<String> = std::env::vars_os()
        .filter(|(k, _)| !(cfg!(target_os = "macos") && k == "__CF_USER_TEXT_ENCODING"))
        .map(|(k, v)| format!("{}={}", k.to_string_lossy(), v.to_string_lossy()))
        .collect();
    env.sort();
    write("env.txt", env.join("\n").as_bytes());
    let cwd =
        std::env::current_dir().map_or_else(|_| "<none>".to_owned(), |d| d.display().to_string());
    write("cwd.txt", cwd.as_bytes());
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::fs::MetadataExt;
        let mut fds: Vec<u32> = std::fs::read_dir("/dev/fd")
            .expect("the descriptors")
            .filter_map(|e| e.ok()?.file_name().to_str()?.parse().ok())
            .collect();
        fds.sort_unstable();
        let fds: Vec<String> = fds.iter().map(u32::to_string).collect();
        write("fds.txt", fds.join("\n").as_bytes());
        let same = match (
            std::fs::metadata("/dev/fd/2"),
            std::fs::metadata("/dev/null"),
        ) {
            (Ok(err), Ok(null)) => err.dev() == null.dev() && err.ino() == null.ino(),
            _ => false,
        };
        write("stderr.txt", if same { b"null" } else { b"other" });
    }
    let mut input = std::io::stdin().lock();
    let mut out = std::io::stdout().lock();
    let mut got = Vec::new();
    let mut count_line = Vec::new();
    let _ = input.read_until(b'\n', &mut count_line);
    got.extend_from_slice(&count_line);
    let n: usize = String::from_utf8_lossy(&count_line)
        .trim_end()
        .parse()
        .unwrap_or(0);
    let mut key = vec![0u8; n + 1];
    if input.read_exact(&mut key).is_ok() {
        got.extend_from_slice(&key);
    }
    loop {
        let mut line = Vec::new();
        match input.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => got.extend_from_slice(&line),
        }
        let value: serde_json::Value = serde_json::from_slice(&line).unwrap_or_default();
        let id = value["id"].as_u64().unwrap_or(0);
        let reply = match value["prompt"].as_str() {
            Some(word @ ("cut" | "status")) => format!("{{\"id\":{id},\"refused\":\"{word}\"}}"),
            _ => format!(
                "{{\"id\":{id},\"text\":\"done\",\"tokens_in\":11,\"tokens_out\":7,\"first_token_ms\":1,\"ms\":2}}"
            ),
        };
        let _ = writeln!(out, "{reply}").and_then(|()| out.flush());
    }
    write("stdin.bin", &got);
    write("ended", b"");
}

/// Sentence 1 and the money's way through: a compile that the person allowed to use a hosted model and whose drive
/// asks three requests starts the door ONCE, behind the first hold, as A12.3 orders a child (its arguments; the
/// environment of A6.1's list and no variable more; the working directory of the system; on macOS the descriptors
/// 0, 1 and 2 alone and standard error on the null device), hands it the key as the first line of its standard
/// input in the form of A12.6, asks for the key at that start and nowhere else, and ends the door before the run's
/// `finish`. Each request has a hold before it and a settle behind it: at the reported tokens for a reply, at the
/// worst case of its hold for the refusal `cut` (a request that left), at nothing for the refusal `status`.
#[test]
fn a_run_allowed_a_hosted_model_starts_the_door_once_with_the_key_first_and_ends_it_before_the_run_ends(
) {
    let dir = scratch("door-once");
    let record = halvo_test_dir::make(&dir, "record").expect("the record directory");
    let data = halvo_test_dir::make(&dir, "data").expect("the data directory");
    let drive = vec![
        request("Pay the bill."),
        asked("cut", Grammar::Text, Level::Low),
        asked("status", Grammar::Action, Level::High),
    ];
    let seen = run_once(
        true,
        Answers {
            key: Some(KEY),
            hold: None,
        },
        drive,
        &record,
        &data,
    );
    assert_eq!(starts(&record), 1, "one start of the door");
    assert_eq!(
        recorded(&record, "args.txt"),
        "--tier\nfrontier\n--ceiling\nfrontier\n--consent\nrecorded"
    );
    let mut want: Vec<(OsString, OsString)> =
        halvo_web::start::guard_env().expect("the system's values");
    let name = if cfg!(windows) {
        "LOCALAPPDATA"
    } else {
        "HOME"
    };
    if let Some(value) = std::env::var_os(name) {
        want.push((name.into(), value));
    }
    let mut want: Vec<String> = want
        .iter()
        .map(|(k, v)| format!("{}={}", k.to_string_lossy(), v.to_string_lossy()))
        .collect();
    want.sort();
    assert_eq!(
        recorded(&record, "env.txt"),
        want.join("\n"),
        "the door's environment"
    );
    let dir_want = halvo_web::start::system_dir().expect("the system directory");
    assert_eq!(
        recorded(&record, "cwd.txt").to_lowercase(),
        dir_want.display().to_string().to_lowercase(),
        "the door's working directory"
    );
    #[cfg(target_os = "macos")]
    {
        let fds = recorded(&record, "fds.txt");
        let numbers: Vec<&str> = fds.lines().collect();
        assert_eq!(
            numbers.len(),
            4,
            "0, 1, 2 and the listing's own descriptor: {fds}"
        );
        assert_eq!(&numbers[..3], ["0", "1", "2"], "{fds}");
        assert_eq!(recorded(&record, "stderr.txt"), "null");
    }
    let input = std::fs::read(record.join("stdin.bin")).expect("the door's input");
    let head = format!("{}\n{KEY}\n", KEY.len());
    assert!(input.starts_with(head.as_bytes()), "the key's line first");
    let lines: Vec<serde_json::Value> = input[head.len()..]
        .split(|b| *b == b'\n')
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_slice(l).expect("a request line"))
        .collect();
    let ids: Vec<u64> = lines.iter().filter_map(|l| l["id"].as_u64()).collect();
    assert_eq!(ids, [1, 2, 3], "three request lines behind the key");
    assert_eq!(
        seen.model_frames(),
        ["hold", "secret", "settle", "hold", "settle", "hold", "settle"],
        "a hold before each request, the key asked at the door's start alone, a settle behind each"
    );
    let holds = seen.holds();
    assert_eq!(
        seen.settles(),
        [
            Some(ModelUsage {
                tokens_in: 11,
                tokens_out: 7
            }),
            Some(ModelUsage {
                tokens_in: holds[1],
                tokens_out: OUTPUT_LIMIT
            }),
            None,
        ],
        "reported, worst case for cut, nothing for status"
    );
    assert_eq!(
        seen.ended_at_finish,
        Some(true),
        "the door ended before the run's finish"
    );
    assert!(
        seen.lines
            .iter()
            .any(|l| l == "run door=ended by=input_end"),
        "{:?}",
        seen.lines
    );
    assert_eq!(seen.status, 0, "the finish was stored and done written");
}

/// Sentence 2: in such a run the made-up key stands in the door's standard input and in nothing else that the run
/// wrote: not in the door's arguments or environment, not in a frame that the runner wrote, not in a line of its
/// standard error, not in a file under the run's data directory. The kept side: the same search finds it once in
/// the door's input, in the form of A12.6, and once in the supervisor's `secret_reply`.
#[test]
fn the_key_stands_in_the_doors_standard_input_and_in_nothing_else_that_the_run_wrote() {
    let dir = scratch("key-search");
    let record = halvo_test_dir::make(&dir, "record").expect("the record directory");
    let data = halvo_test_dir::make(&dir, "data").expect("the data directory");
    let seen = run_once(
        true,
        Answers {
            key: Some(KEY),
            hold: None,
        },
        vec![request("Pay the bill.")],
        &record,
        &data,
    );
    let key = KEY.as_bytes();
    let input = std::fs::read(record.join("stdin.bin")).expect("the door's input");
    let head = format!("{}\n{KEY}\n", KEY.len());
    let door_input = count(&input, key);
    let door_input_head = usize::from(input.starts_with(head.as_bytes()));
    let replies = seen.to_runner.iter().map(|f| count(f, key)).sum::<usize>();
    let door_argv = count(recorded(&record, "argv.txt").as_bytes(), key);
    let door_env = count(recorded(&record, "env.txt").as_bytes(), key);
    let frames = seen
        .from_runner
        .iter()
        .map(|f| count(f, key))
        .sum::<usize>();
    let stderr = seen
        .lines
        .iter()
        .map(|l| count(l.as_bytes(), key))
        .sum::<usize>();
    let mut files = Vec::new();
    files_under(&data, &mut files);
    let in_files = files.iter().map(|(_, b)| count(b, key)).sum::<usize>();
    println!(
        "key counts: door_input={door_input} door_input_head={door_input_head} secret_reply={replies} \
         door_arguments={door_argv} door_environment={door_env} frames_from_runner={} with_the_key={frames} \
         stderr_lines={} with_the_key={stderr} data_files={} with_the_key={in_files}",
        seen.from_runner.len(),
        seen.lines.len(),
        files.len()
    );
    assert_eq!(
        (door_input, door_input_head, replies),
        (1, 1, 1),
        "the kept side"
    );
    assert_eq!(
        (door_argv, door_env, frames, stderr, in_files),
        (0, 0, 0, 0, 0),
        "the key outside the door's input"
    );
    assert!(seen.from_runner.len() > 5, "the run wrote its frames");
}

/// Sentence 3: a compile hands its work back, starts no door and writes no request line, BEFORE anything is sent:
/// with no `--hosted` (the agreement is missing) it asks the supervisor nothing about the model; with no key stored
/// it settles its hold at nothing and starts no door; at the person's cap (`over_cap`) it starts no door, asks for
/// no key and settles nothing that it did not hold. Each ends `handed_back` with its own cause.
#[test]
fn a_run_hands_back_before_anything_is_sent_where_the_agreement_the_key_or_the_room_is_missing() {
    struct Case {
        name: &'static str,
        hosted: bool,
        answers: Answers,
        cause: Cause,
        model: &'static [&'static str],
        settles: Vec<Option<ModelUsage>>,
    }
    let cases = [
        Case {
            name: "agreement",
            hosted: false,
            answers: Answers {
                key: Some(KEY),
                hold: None,
            },
            cause: Cause::NoAgreement,
            model: &[],
            settles: vec![],
        },
        Case {
            name: "key",
            hosted: true,
            answers: Answers {
                key: None,
                hold: None,
            },
            cause: Cause::NoModelKey,
            model: &["hold", "secret", "settle"],
            settles: vec![None],
        },
        Case {
            name: "cap",
            hosted: true,
            answers: Answers {
                key: Some(KEY),
                hold: Some("over_cap"),
            },
            cause: Cause::OverCap,
            model: &["hold"],
            settles: vec![],
        },
    ];
    for Case {
        name,
        hosted,
        answers,
        cause,
        model,
        settles,
    } in cases
    {
        let dir = scratch(&format!("hand-back-{name}"));
        let record = halvo_test_dir::make(&dir, "record").expect("the record directory");
        let data = halvo_test_dir::make(&dir, "data").expect("the data directory");
        let seen = run_once(
            hosted,
            answers,
            vec![request("Pay the bill.")],
            &record,
            &data,
        );
        assert_eq!(
            seen.end(),
            (
                Some(Outcome::HandedBack),
                Some(Outcome::HandedBack),
                Some(cause)
            ),
            "{name}"
        );
        assert_eq!(seen.model_frames(), model, "{name}");
        assert_eq!(seen.settles(), settles, "{name}");
        assert_eq!(starts(&record), 0, "{name}: no door started");
        assert!(
            !record.join("stdin.bin").exists(),
            "{name}: no request line"
        );
    }
}
