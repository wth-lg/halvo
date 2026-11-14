mod common;

use std::time::{SystemTime, UNIX_EPOCH};

use common::fixture::Fixture;
use common::seed::Seed;
use halvo_connect::{Calendar, CallError, Effect, Mail, RunDay, Token};
use serde_json::{json, Value};

const BODY: &str = "Thank you, noted.";

fn today() -> RunDay {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("a time after 1970");
    RunDay::from_unix_seconds(now.as_secs())
}

fn cleanup_ids(seed: &Seed) -> Vec<String> {
    seed.end("cleanup")
        .as_array()
        .expect("end.cleanup")
        .iter()
        .map(|id| id.as_str().expect("an id").to_owned())
        .collect()
}

fn requests(fixture: &Fixture) -> Value {
    fixture.state()["requests"].clone()
}

fn counts(total: u64, state_changing: u64) -> Value {
    json!({"total": total, "state_changing": state_changing, "evil": 0, "unauthorized": 0})
}

fn reply_rows(seed: &Seed) -> Vec<(String, String, String)> {
    seed.end("reply")
        .as_array()
        .expect("end.reply")
        .iter()
        .map(|row| {
            (
                row["to"].as_str().expect("to").to_owned(),
                row["conversation"]
                    .as_str()
                    .expect("conversation")
                    .to_owned(),
                row["in_reply_to"].as_str().expect("in_reply_to").to_owned(),
            )
        })
        .collect()
}

#[test]
fn mail_list() {
    let fixture = Fixture::start();
    let graph = fixture.graph("mail");
    for n in 1..=3 {
        fixture.reset(n, &[]);
        let seed = Seed::load(n);
        let messages = graph
            .mail_list(&seed.token(), &seed.rule())
            .expect("mail_list answers");
        let got: Vec<_> = messages
            .iter()
            .map(|m| {
                (
                    m.sender().to_owned(),
                    m.conversation().to_owned(),
                    m.id().to_owned(),
                )
            })
            .collect();
        assert!(
            got == reply_rows(&seed),
            "seed {n}: the messages that the reply rule selects"
        );
        assert!(
            messages
                .iter()
                .all(|m| seed.subject(m.id()) == Some(m.subject())),
            "seed {n}: each subject"
        );
        assert!(
            messages
                .iter()
                .next()
                .is_some_and(|m| m.body().contains(seed.page_marker())),
            "seed {n}: the body of the first message"
        );
        assert!(
            requests(&fixture) == counts(1, 0),
            "seed {n}: one GET and no state change"
        );
    }
}

#[test]
fn mail_draft_reply() {
    let fixture = Fixture::start();
    let graph = fixture.graph("mail");
    for n in 1..=3 {
        fixture.reset(n, &[]);
        let seed = Seed::load(n);
        let token = seed.token();
        let messages = graph
            .mail_list(&token, &seed.rule())
            .expect("mail_list answers");
        let want: Vec<_> = messages
            .iter()
            .map(|m| {
                (
                    m.id().to_owned(),
                    vec![m.sender().to_owned()],
                    format!("RE: {}", m.subject()),
                )
            })
            .collect();
        let drafts = graph
            .mail_draft_reply(&token, messages, BODY)
            .expect("mail_draft_reply answers");
        let got: Vec<_> = drafts
            .iter()
            .map(|d| {
                (
                    d.message().to_owned(),
                    d.to().to_vec(),
                    d.subject().to_owned(),
                )
            })
            .collect();
        assert!(
            got == want,
            "seed {n}: one draft per message, to its sender, with its subject"
        );
        assert!(
            drafts.iter().all(|d| d.id().starts_with("draft-")),
            "seed {n}: the ids of the drafts"
        );
        let state = fixture.state();
        assert!(
            state["mail"]["drafts"] == 2,
            "seed {n}: two drafts at the provider"
        );
        assert!(state["mail"]["sent"] == json!([]), "seed {n}: nothing sent");
        assert!(
            state["requests"] == counts(5, 4),
            "seed {n}: createReply and PATCH once per message"
        );
    }
}

#[test]
fn mail_send() {
    let fixture = Fixture::start();
    let graph = fixture.graph("mail");
    for n in 1..=3 {
        fixture.reset(n, &[]);
        let seed = Seed::load(n);
        let token = seed.token();
        let messages = graph
            .mail_list(&token, &seed.rule())
            .expect("mail_list answers");
        let drafts = graph
            .mail_draft_reply(&token, messages, BODY)
            .expect("mail_draft_reply answers");
        for draft in drafts.into_sends() {
            graph.mail_send(&token, draft).expect("mail_send answers");
        }
        let state = fixture.state();
        assert!(
            state["mail"]["sent"] == *seed.end("reply"),
            "seed {n}: mail.sent equals end.reply"
        );
        assert!(state["mail"]["drafts"] == 0, "seed {n}: no draft left");
        assert!(
            state["requests"] == counts(7, 6),
            "seed {n}: one send per draft"
        );
        let again = graph
            .mail_list(&token, &seed.rule())
            .expect("mail_list answers");
        assert!(again.is_empty(), "seed {n}: the answered messages are read");
        fixture.advance();
        let later = graph
            .mail_list(&token, &seed.rule())
            .expect("mail_list answers");
        let ids: Vec<_> = later.iter().map(|m| m.id()).collect();
        assert!(
            ids == [seed.later("message/id")],
            "seed {n}: the later message alone"
        );
    }
}

#[test]
fn calendar_list() {
    let fixture = Fixture::start();
    let graph = fixture.graph("calendar");
    for n in 1..=3 {
        fixture.reset(n, &[]);
        let seed = Seed::load(n);
        let events = graph
            .calendar_list(&seed.token(), today())
            .expect("calendar_list answers");
        let ids: Vec<_> = events.iter().map(|e| e.id().to_owned()).collect();
        assert!(
            ids == cleanup_ids(&seed),
            "seed {n}: the events that the clean-up rule selects"
        );
        assert!(
            events
                .iter()
                .all(|e| seed.event_subject(e.id()) == Some(e.subject())),
            "seed {n}: each subject"
        );
        assert!(
            requests(&fixture) == counts(1, 0),
            "seed {n}: one GET and no state change"
        );
    }
}

#[test]
fn calendar_delete() {
    let fixture = Fixture::start();
    let graph = fixture.graph("calendar");
    for n in 1..=3 {
        fixture.reset(n, &[]);
        let seed = Seed::load(n);
        let token = seed.token();
        let events = graph
            .calendar_list(&token, today())
            .expect("calendar_list answers");
        for event in events.into_deletes() {
            graph
                .calendar_delete(&token, event)
                .expect("calendar_delete answers");
        }
        let state = fixture.state();
        assert!(
            state["calendar"]["deleted"] == *seed.end("cleanup"),
            "seed {n}: calendar.deleted equals end.cleanup"
        );
        assert!(
            state["requests"] == counts(3, 2),
            "seed {n}: one DELETE per event"
        );
        let again = graph
            .calendar_list(&token, today())
            .expect("calendar_list answers");
        assert!(again.is_empty(), "seed {n}: the deleted events are gone");
        fixture.advance();
        let later = graph
            .calendar_list(&token, today())
            .expect("calendar_list answers");
        let ids: Vec<_> = later.iter().map(|e| e.id()).collect();
        assert!(
            ids == [seed.later("event/id")],
            "seed {n}: the later event alone"
        );
    }
}

#[test]
fn unauthorized() {
    let fixture = Fixture::start();
    let mail = fixture.graph("mail");
    let calendar = fixture.graph("calendar");
    let seed = Seed::load(1);
    let marker = seed.token_text().to_owned();
    let right = seed.token();
    let wrong = Token::new(format!("{marker}x")).expect("a token");
    let mut errors = Vec::new();

    let error = mail.mail_list(&wrong, &seed.rule()).expect_err("a 401");
    errors.push(error);
    let messages = mail
        .mail_list(&right, &seed.rule())
        .expect("mail_list answers");
    let error = mail
        .mail_draft_reply(&wrong, messages, BODY)
        .expect_err("a 401");
    errors.push(error);
    assert!(fixture.state()["mail"]["drafts"] == 0, "no draft is made");

    let messages = mail
        .mail_list(&right, &seed.rule())
        .expect("mail_list answers");
    let drafts = mail
        .mail_draft_reply(&right, messages, BODY)
        .expect("mail_draft_reply answers");
    let mut sends = drafts.into_sends();
    let error = mail
        .mail_send(&wrong, sends.next().expect("a draft"))
        .expect_err("a 401");
    errors.push(error);
    assert!(
        fixture.state()["mail"]["sent"] == json!([]),
        "nothing is sent"
    );

    let error = calendar.calendar_list(&wrong, today()).expect_err("a 401");
    errors.push(error);
    let events = calendar
        .calendar_list(&right, today())
        .expect("calendar_list answers");
    let error = calendar
        .calendar_delete(&wrong, events.into_deletes().next().expect("an event"))
        .expect_err("a 401");
    errors.push(error);
    assert!(
        fixture.state()["calendar"]["deleted"] == json!([]),
        "nothing is deleted"
    );

    assert!(
        errors.iter().all(|e| *e == CallError::Unauthorized),
        "each of the five operations answers Unauthorized"
    );
    assert!(
        errors.iter().all(|e| e.effect() == Effect::NotMade),
        "no effect is made"
    );
    assert!(
        fixture.state()["requests"]["unauthorized"] == 5,
        "one request per refused call"
    );
    let texts: Vec<String> = errors
        .iter()
        .flat_map(|e| [e.to_string(), format!("{e:?}")])
        .chain([format!("{wrong:?}"), format!("{right:?}")])
        .collect();
    assert!(
        texts.iter().all(|text| !text.contains(&marker)),
        "no text of an error or a token holds the token"
    );
}

#[test]
fn no_text_of_a_value_holds_the_token() {
    let fixture = Fixture::start();
    let seed = Seed::load(1);
    let marker = seed.token_text().to_owned();
    let token = seed.token();
    let mail = fixture.graph("mail");
    let calendar = fixture.graph("calendar");
    let rule = seed.rule();
    let messages = mail.mail_list(&token, &rule).expect("mail_list answers");
    let mut texts = vec![
        format!("{token:?}"),
        format!("{mail:?}"),
        format!("{calendar:?}"),
        format!("{rule:?}"),
        format!("{messages:?}"),
    ];
    texts.extend(messages.iter().map(|m| format!("{m:?}")));
    let drafts = mail
        .mail_draft_reply(&token, messages, BODY)
        .expect("mail_draft_reply answers");
    texts.push(format!("{drafts:?}"));
    texts.extend(drafts.iter().map(|d| format!("{d:?}")));
    for draft in drafts.into_sends() {
        texts.push(format!("{draft:?}"));
        mail.mail_send(&token, draft).expect("mail_send answers");
    }
    let events = calendar
        .calendar_list(&token, today())
        .expect("calendar_list answers");
    texts.push(format!("{events:?}"));
    texts.extend(events.iter().map(|e| format!("{e:?}")));
    for event in events.into_deletes() {
        texts.push(format!("{event:?}"));
        calendar
            .calendar_delete(&token, event)
            .expect("calendar_delete answers");
    }
    assert!(
        texts.len() == 17,
        "every value of the two tasks is formatted"
    );
    assert!(
        texts.iter().all(|text| !text.contains(&marker)),
        "no text holds the token"
    );
}

fn alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }
    #[cfg(windows)]
    {
        std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output()
            .is_ok_and(|out| {
                String::from_utf8_lossy(&out.stdout)
                    .split_whitespace()
                    .any(|word| word == pid.to_string())
            })
    }
}

#[test]
fn a_panic_after_the_start_leaves_no_fixture_server() {
    let (tx, rx) = std::sync::mpsc::channel();
    let joined = std::thread::spawn(move || {
        Fixture::start_then(|pid| {
            tx.send(pid).expect("the pid is sent");
            panic!("a panic on purpose, right after the server started");
        })
    })
    .join();
    assert!(joined.is_err(), "the helper panicked");
    let pid = rx.recv().expect("the pid of the server");
    assert!(!alive(pid), "the fixture server ended with the panic");
}

const HOLDER: &str = "HALVO_CONNECT_TEST_FIXTURE_HOLDER";
const STAND_IN: &str = "HALVO_CONNECT_TEST_STAND_IN_SECONDS";
const GONE_WITHIN: std::time::Duration = std::time::Duration::from_secs(10);

#[test]
#[ignore = "a process of its own, which a_hard_kill_of_the_test_process_ends_its_fixture_server starts"]
fn a_fixture_holder() {
    if std::env::var_os(HOLDER).is_none() {
        return;
    }
    let pid = std::cell::Cell::new(0);
    let _fixture = Fixture::start_then(|id| pid.set(id));
    println!("\nfixture server pid {}", pid.get());
    let _ = std::io::Read::read_to_end(&mut std::io::stdin(), &mut Vec::new());
}

#[test]
#[ignore = "a process of its own, which the tests of the guard and of the wait start"]
fn a_stand_in() {
    let Some(seconds) = std::env::var(STAND_IN)
        .ok()
        .and_then(|seconds| seconds.parse::<u64>().ok())
    else {
        return;
    };
    std::thread::sleep(std::time::Duration::from_secs(seconds));
}

fn test_process(test: &str, variable: &str, value: &str) -> std::process::Command {
    let mut command = std::process::Command::new(std::env::current_exe().expect("the test binary"));
    command
        .args([
            test,
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads",
            "1",
        ])
        .env(variable, value);
    command
}

fn stand_in(seconds: u64) -> std::process::Child {
    test_process("a_stand_in", STAND_IN, &seconds.to_string())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .spawn()
        .expect("the stand-in starts")
}

struct Own(std::process::Child);

impl Drop for Own {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Known {
    pid: u32,
    start: String,
}

impl Known {
    fn read(pid: u32) -> Option<Known> {
        start_fact(pid).map(|start| Known { pid, start })
    }

    fn runs(&self) -> bool {
        start_fact(self.pid).as_deref() == Some(self.start.as_str())
    }
}

fn nameable(pid: u32) -> bool {
    pid != 0 && pid <= i32::MAX as u32
}

fn start_fact_command(pid: u32) -> Option<std::process::Command> {
    if !nameable(pid) {
        return None;
    }
    #[cfg(unix)]
    let command = {
        let mut command = std::process::Command::new("ps");
        command.args(["-o", "lstart=", "-p", &pid.to_string()]);
        command
    };
    #[cfg(windows)]
    let command = {
        let mut command = std::process::Command::new("powershell");
        command.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &format!("(Get-Process -Id {pid} -ErrorAction Stop).StartTime.Ticks"),
        ]);
        command
    };
    Some(command)
}

fn start_fact(pid: u32) -> Option<String> {
    let out = start_fact_command(pid)?
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8(out.stdout).ok()?;
    let text = text.trim();
    (out.status.success() && !text.is_empty()).then(|| text.to_owned())
}

fn end_command(pid: u32) -> Option<std::process::Command> {
    if !nameable(pid) {
        return None;
    }
    #[cfg(unix)]
    let command = {
        let mut command = std::process::Command::new("kill");
        command.args(["-KILL", &pid.to_string()]);
        command
    };
    #[cfg(windows)]
    let command = {
        let mut command = std::process::Command::new("taskkill");
        command.args(["/F", "/PID", &pid.to_string()]);
        command
    };
    Some(command)
}

struct Leftover(Option<Known>);

impl Leftover {
    fn disarm(&mut self) {
        self.0 = None;
    }
}

impl Drop for Leftover {
    fn drop(&mut self) {
        let Some(known) = self.0.take() else { return };
        if !known.runs() {
            return;
        }
        if let Some(mut command) = end_command(known.pid) {
            let _ = command
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
    }
}

fn gone_within(known: &Known, bound: std::time::Duration) -> bool {
    let start = std::time::Instant::now();
    loop {
        if !known.runs() {
            return true;
        }
        if start.elapsed() >= bound {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

fn ended_within(
    child: &mut std::process::Child,
    bound: std::time::Duration,
) -> Option<std::process::ExitStatus> {
    let start = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait().expect("a status read") {
            return Some(status);
        }
        if start.elapsed() >= bound {
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

#[test]
fn a_guard_leaves_a_process_whose_start_fact_differs() {
    let mut stand_in = Own(stand_in(60));
    let known = Known::read(stand_in.0.id()).expect("the stand-in runs");
    let other = Known {
        pid: known.pid,
        start: format!("{} and another", known.start),
    };
    drop(Leftover(Some(other)));
    assert_eq!(
        ended_within(&mut stand_in.0, std::time::Duration::from_secs(2)),
        None,
        "the guard left the stand-in running"
    );
}

#[test]
fn a_disarmed_guard_leaves_the_process_of_its_pair() {
    let mut stand_in = Own(stand_in(60));
    let known = Known::read(stand_in.0.id()).expect("the stand-in runs");
    let mut leftover = Leftover(Some(known));
    leftover.disarm();
    drop(leftover);
    assert_eq!(
        ended_within(&mut stand_in.0, std::time::Duration::from_secs(2)),
        None,
        "the disarmed guard left the stand-in running"
    );
}

#[test]
fn a_guard_ends_the_process_whose_pair_matches() {
    let mut stand_in = Own(stand_in(60));
    let known = Known::read(stand_in.0.id()).expect("the stand-in runs");
    drop(Leftover(Some(known)));
    let status = ended_within(&mut stand_in.0, GONE_WITHIN);
    assert!(
        status.is_some_and(|status| !status.success()),
        "the guard ended the stand-in: {status:?}"
    );
}

#[test]
fn the_wait_reads_an_ended_process_as_gone_and_a_running_one_as_running() {
    let short = stand_in(6);
    let known = Known::read(short.id()).expect("the short stand-in runs");
    let reaper = std::thread::spawn(move || {
        let mut short = short;
        short.wait()
    });
    assert!(
        gone_within(&known, GONE_WITHIN),
        "a stand-in of 6 s is gone within the bound"
    );
    let ended = reaper
        .join()
        .expect("the reaper")
        .expect("the short stand-in's status");
    assert!(ended.success(), "the short stand-in ended by itself");
    let long = Own(stand_in(60));
    let known = Known::read(long.0.id()).expect("the long stand-in runs");
    assert!(
        !gone_within(&known, std::time::Duration::ZERO),
        "a running stand-in is read as running at a bound of zero"
    );
}

#[test]
fn a_helper_refuses_a_number_that_names_no_single_process() {
    for pid in [0, i32::MAX as u32 + 1, u32::MAX] {
        assert!(start_fact_command(pid).is_none(), "{pid}");
        assert!(end_command(pid).is_none(), "{pid}");
        assert_eq!(Known::read(pid), None, "{pid}");
    }
    assert!(start_fact_command(i32::MAX as u32).is_some());
    assert!(end_command(1).is_some());
}

#[test]
fn a_hard_kill_of_the_test_process_ends_its_fixture_server() {
    use std::io::BufRead;

    let mut holder = Own(test_process("a_fixture_holder", HOLDER, "1")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("the test binary starts"));
    let stdout = holder.0.stdout.take().expect("its standard output");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if let Some(pid) = line.strip_prefix("fixture server pid ") {
                let _ = tx.send(pid.parse::<u32>().ok());
                break;
            }
        }
    });
    let pid = rx
        .recv_timeout(std::time::Duration::from_secs(60))
        .ok()
        .flatten()
        .expect("the holder prints the pid of its fixture server");
    let known = Known::read(pid).expect("the fixture server runs");
    let mut leftover = Leftover(Some(known.clone()));
    holder.0.kill().expect("the hard kill of the holder");
    holder.0.wait().expect("the holder ends");
    let gone = gone_within(&known, GONE_WITHIN);
    if gone {
        leftover.disarm();
    }
    assert!(
        gone,
        "the fixture server ended within 10 s of the hard kill of the process that started it"
    );
}
