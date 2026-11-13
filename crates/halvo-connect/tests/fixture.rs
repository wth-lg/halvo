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

#[test]
fn a_fixture_holder() {
    if std::env::var_os(HOLDER).is_none() {
        return;
    }
    let pid = std::cell::Cell::new(0);
    let _fixture = Fixture::start_then(|id| pid.set(id));
    println!("\nfixture server pid {}", pid.get());
    let _ = std::io::Read::read_to_end(&mut std::io::stdin(), &mut Vec::new());
}

struct Holder(std::process::Child);

impl Drop for Holder {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Leftover(u32);

impl Drop for Leftover {
    fn drop(&mut self) {
        if alive(self.0) {
            end(self.0);
        }
    }
}

fn end(pid: u32) {
    #[cfg(unix)]
    let ended = std::process::Command::new("kill")
        .args(["-KILL", &pid.to_string()])
        .status();
    #[cfg(windows)]
    let ended = std::process::Command::new("taskkill")
        .args(["/F", "/PID", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .status();
    let _ = ended;
}

#[test]
fn a_hard_kill_of_the_test_process_ends_its_fixture_server() {
    use std::io::BufRead;
    use std::time::{Duration, Instant};

    let mut holder = Holder(
        std::process::Command::new(std::env::current_exe().expect("the test binary"))
            .args([
                "a_fixture_holder",
                "--exact",
                "--nocapture",
                "--test-threads",
                "1",
            ])
            .env(HOLDER, "1")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("the test binary starts"),
    );
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
        .recv_timeout(Duration::from_secs(60))
        .ok()
        .flatten()
        .expect("the holder prints the pid of its fixture server");
    let _leftover = Leftover(pid);
    assert!(alive(pid), "the fixture server runs");
    holder.0.kill().expect("the hard kill of the holder");
    holder.0.wait().expect("the holder ends");
    let start = Instant::now();
    while alive(pid) && start.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(
        !alive(pid),
        "the fixture server ended within 10 s of the hard kill of the process that started it"
    );
}
