//! The tests of r3 U7.4, each a run of a reply agent's runner in this process over two pipes, with three stand-ins
//! on threads and in children of its own: the supervisor and store (the form of crate::model::tests), the mail host of
//! the agent's bindings on 127.0.0.1 (one listener that answers the list, createReply, the PATCH and the send, and
//! records every request whole), and the door, which is this test binary started as the entry `write_door` alone and
//! answers each request line by the word in square brackets that the message's text holds. No test here starts
//! halvo-model, and nothing leaves 127.0.0.1.

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use halvo_schema::executable::body::{LoadRepliesReply, LoadReply, LoadedBindings, Stored};
use halvo_schema::executable::day::day_of;
use halvo_schema::executable::frame::{
    Approval, Confirm, ConfirmDecision, ConfirmEffect, ConfirmReply, ConfirmRequest, ReplyError,
    SecretReply, StoreReply, StoreReplyFrame, FRAME_JSON_MAX,
};
use halvo_schema::executable::names::{
    ActionClass, AllowedClass, Answer as Choice, Autonomy, FieldName, LogEvent, ModelTier, Source,
    YesNo,
};
use halvo_schema::executable::record::Slot;
use halvo_schema::executable::text::{Name, OwnValue, SecretText, V1};
use halvo_schema::executable::{
    AgentReply, AgentStart, ApprovalId, Bound, BoundField, Cause, EffectId, FromRunner, Outcome,
    ProviderId, ReplyState, RunId, RunnerCall, RunnerReply, ToRunner, WebOrigin,
};
use halvo_schema::record::Instant;
use halvo_schema::{AgentId, Category, ErrorName, WorkflowId};
use serde_json::{json, Value};

use super::{cut, json_bytes, BODY_JSON_MAX, LINE_MAX};
use crate::link::{encode, Link};
use crate::model::ask::Place;
use crate::run::Runner;

/// The made-up key of these tests: visible ASCII, held by no file but this one.
const KEY: &str = "made-up-key-u74-Hq3Vn7Ty-8e2b";

/// The made-up token of the mail adapter of these tests.
const TOKEN: &str = "made-up-mail-token-u74";

/// The account of the agent's bindings and the one sender of its list.
const ACCOUNT: &str = "me@home.agent.test";
const SENDER: &str = "ada@people.agent.test";

/// The entry of the stand-in door, as the test harness names it.
const DOOR: &str = "model::write::tests::write_door";

/// The cell of the test's policy that opens the kind `write` of the category `email` at the tier `frontier`.
const OPEN: &str = "email\twrite\tfrontier\tmedium\tline_met\t4\t4";

/// The texts of the stand-in door's answers.
const PLAIN: &str = "Friday at ten suits me. Thank you for asking.";
const SECRET_A: &str = "SECRET-A-the-sender-wants-a-time";
const SECRET_B: &str = "SECRET-B-friday-could-work";
const BODY_A: &str = "Friday at ten suits me, see you then.";

/// A body of several lines and above 1,024 bytes, so that its value has two pieces.
fn long_body() -> String {
    (1..=20)
        .map(|n| format!("Line {n} of the answer: Friday at ten suits me, and the room is booked."))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A body of letters whose value's JSON text is `BODY_JSON_MAX + extra` bytes.
fn at_bound(extra: usize) -> String {
    let mut body = String::new();
    while json_bytes(&body) < BODY_JSON_MAX + extra {
        body.push('x');
    }
    assert_eq!(
        json_bytes(&body),
        BODY_JSON_MAX + extra,
        "a body at the bound"
    );
    body
}

/// The stand-in door's output for the word of a message: a reply's text, or a refusal's word.
fn output(word: &str) -> Result<String, &'static str> {
    match word {
        "closed" => Ok(format!(
            "<think>\n{SECRET_A}: the sender asks for a time.\n</think>\n\n  {BODY_A}  \n"
        )),
        "open" => Ok(format!("<think>\n{SECRET_B}: I should answer that")),
        "long" => Ok(long_body()),
        "bound" => Ok(at_bound(0)),
        "over" => Ok(at_bound(1)),
        "cut" => Err("cut"),
        _ => Ok(PLAIN.to_owned()),
    }
}

/// The word in square brackets of `text`, `plain` where it holds none.
fn word_of(text: &str) -> &str {
    text.split_once('[')
        .and_then(|(_, rest)| rest.split_once(']'))
        .map_or("plain", |(word, _)| word)
}

/// THE STAND-IN DOOR: in a run of the tests it returns at once; started by a test's runner, with the record directory
/// after `--` and the door's own arguments behind it, it records each start and its standard input, and answers each
/// request line by the word of its snapshot.
#[test]
fn write_door() {
    let args: Vec<String> = std::env::args().collect();
    let (Some(dash), Some(_)) = (
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
        let value: Value = serde_json::from_slice(&line).unwrap_or_default();
        let id = value["id"].as_u64().unwrap_or(0);
        let reply = match output(word_of(value["snapshot"].as_str().unwrap_or(""))) {
            Ok(text) => format!(
                "{{\"id\":{id},\"text\":{},\"tokens_in\":11,\"tokens_out\":7,\"first_token_ms\":1,\"ms\":2}}",
                Value::String(text)
            ),
            Err(word) => format!("{{\"id\":{id},\"refused\":\"{word}\"}}"),
        };
        let _ = writeln!(out, "{reply}").and_then(|()| out.flush());
    }
    write("stdin.bin", &got);
}

/// One message of the inbox, from `SENDER` to `ACCOUNT`, unread, whose body holds `words`.
fn message(id: &str, words: &str) -> Value {
    json!({
        "id": id,
        "isRead": false,
        "from": {"emailAddress": {"address": SENDER}},
        "toRecipients": [{"emailAddress": {"address": ACCOUNT}}],
        "replyTo": [],
        "receivedDateTime": "2026-11-25T10:00:00Z",
        "conversationId": format!("conv-{id}"),
        "subject": format!("Question {id}"),
        "body": {"contentType": "text", "content": words},
        "isReadReceiptRequested": false
    })
}

/// The text of `message` as a request must hold it.
fn text_of(message: &Value) -> String {
    format!(
        "From: {SENDER}\nSubject: {}\n{}",
        message["subject"].as_str().unwrap_or(""),
        message["body"]["content"].as_str().unwrap_or("")
    )
}

/// The stand-in mail host: its port and every request that it got, whole, in order.
struct Mailbox {
    port: u16,
    seen: Arc<Mutex<Vec<String>>>,
}

impl Mailbox {
    fn start(messages: Vec<Value>) -> Mailbox {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a listener on the loopback");
        let port = listener.local_addr().expect("its address").port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let kept = Arc::clone(&seen);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { break };
                serve(stream, &messages, &kept);
            }
        });
        Mailbox { port, seen }
    }

    fn requests(&self) -> Vec<String> {
        self.seen.lock().expect("lock").clone()
    }

    /// The draft and the body's content of each PATCH, in order.
    fn patches(&self) -> Vec<(String, String)> {
        self.requests()
            .iter()
            .filter(|r| r.starts_with("PATCH "))
            .map(|r| {
                let path = r.split(' ').nth(1).unwrap_or("");
                let draft = path.rsplit('/').next().unwrap_or("").to_owned();
                let body = r.split_once("\r\n\r\n").map_or("", |(_, b)| b);
                let value: Value = serde_json::from_str(body).unwrap_or_default();
                let content = value["body"]["content"].as_str().unwrap_or("").to_owned();
                (draft, content)
            })
            .collect()
    }

    fn count(&self, method: &str, end: &str) -> usize {
        self.requests()
            .iter()
            .filter(|r| {
                r.starts_with(&format!("{method} "))
                    && r.split(' ').nth(1).is_some_and(|p| p.ends_with(end))
            })
            .count()
    }

    fn sends(&self) -> usize {
        self.count("POST", "/send")
    }
}

fn serve(mut stream: TcpStream, messages: &[Value], seen: &Mutex<Vec<String>>) {
    let Ok(clone) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(clone);
    let mut head = String::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }
        head.push_str(&line);
        if line == "\r\n" {
            break;
        }
    }
    let length = head
        .lines()
        .find_map(|l| {
            let (name, value) = l.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    let mut body = vec![0u8; length];
    let _ = reader.read_exact(&mut body);
    let body = String::from_utf8_lossy(&body).into_owned();
    seen.lock().expect("lock").push(format!("{head}{body}"));
    let first = head.lines().next().unwrap_or_default().to_owned();
    let mut words = first.split(' ');
    let (method, path) = (words.next().unwrap_or(""), words.next().unwrap_or(""));
    let tail = path.strip_prefix("/v1.0/me/").unwrap_or(path);
    let (status, answer) = if method == "GET" && tail.starts_with("mailFolders/inbox/messages") {
        (200, json!({ "value": messages }).to_string())
    } else if method == "POST" && tail.ends_with("/createReply") {
        let id = tail
            .strip_prefix("messages/")
            .and_then(|t| t.strip_suffix("/createReply"))
            .unwrap_or("");
        (
            201,
            json!({"id": format!("draft-{id}"), "isDraft": true}).to_string(),
        )
    } else if method == "POST" && tail.ends_with("/send") {
        (202, String::new())
    } else if method == "PATCH" {
        let draft = tail.strip_prefix("messages/").unwrap_or("");
        let answer = json!({
            "id": draft,
            "isDraft": true,
            "toRecipients": [{"emailAddress": {"address": SENDER}}],
            "ccRecipients": [],
            "bccRecipients": [],
            "subject": format!("RE: {draft}")
        });
        (200, answer.to_string())
    } else {
        (404, String::new())
    };
    let reply = format!(
        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
        answer.len()
    );
    let _ = stream.write_all(reply.as_bytes());
    let _ = stream.flush();
}

/// What a run of a test is given: the agent's tier and whether the start names it (`--hosted`); whether the test's
/// policy opens the cell `email write` at `frontier` (else the release table, which hands it back); the stored key;
/// the store's refusal of a `model_hold`; the agent's autonomy, whether `send` is allowed and whether its caps split;
/// the person's answer on every card; and the messages whose own newest row is `drafted` with the draft
/// `draft-<id>`.
#[derive(Clone)]
struct Setup {
    tier: ModelTier,
    hosted: bool,
    open: bool,
    key: Option<&'static str>,
    hold: Option<&'static str>,
    autonomy: Autonomy,
    allow_send: bool,
    split: bool,
    answer: Choice,
    waiting: Vec<&'static str>,
}

impl Setup {
    /// A frontier agent that the person allowed, with a key, the cell open, every card answered `allow_once`.
    fn allowed() -> Setup {
        Setup {
            tier: ModelTier::Frontier,
            hosted: true,
            open: true,
            key: Some(KEY),
            hold: None,
            autonomy: Autonomy::DraftAndConfirm,
            allow_send: false,
            split: false,
            answer: Choice::AllowOnce,
            waiting: Vec::new(),
        }
    }
}

/// What one run wrote and got: every frame of the runner, parsed and as its bytes; the count of sends that the mail
/// host had got when each confirmation came; the lines on standard error; the exit status.
struct Seen {
    frames: Vec<FromRunner>,
    from_runner: Vec<Vec<u8>>,
    sends_at_cards: Vec<usize>,
    lines: Vec<String>,
    status: i32,
}

impl Seen {
    /// The model's frames in order: `hold`, `secret` (for the key alone) and `settle`.
    fn model_frames(&self) -> Vec<&'static str> {
        self.frames
            .iter()
            .filter_map(|frame| match frame {
                FromRunner::Secret(s) if s.secret.as_str() == "model_key" => Some("secret"),
                FromRunner::Store(s) => match s.request.body() {
                    RunnerCall::ModelHold(_) => Some("hold"),
                    RunnerCall::ModelSettle(_) => Some("settle"),
                    _ => None,
                },
                _ => None,
            })
            .collect()
    }

    /// The `body_bytes` of each `model_hold`.
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

    /// The field `body` of each effect of each confirmation, with its source.
    fn cards(&self) -> Vec<Vec<(String, Source)>> {
        self.frames
            .iter()
            .filter_map(|frame| match frame {
                FromRunner::Confirm(c) => Some(
                    c.request
                        .effects
                        .iter()
                        .map(|e| {
                            let f = e
                                .bound
                                .fields()
                                .iter()
                                .find(|f| f.name == FieldName::Body)
                                .expect("a body field");
                            (f.value.joined(), f.from)
                        })
                        .collect(),
                ),
                _ => None,
            })
            .collect()
    }

    /// The outcome of the run's `finish` and the cause of its `stopped` row.
    fn end(&self) -> (Option<Outcome>, Option<Cause>) {
        let (mut finish, mut stopped) = (None, None);
        for frame in &self.frames {
            if let FromRunner::Store(s) = frame {
                match s.request.body() {
                    RunnerCall::Finish(f) => finish = Some(f.record.outcome),
                    RunnerCall::Log(b) => {
                        for row in &b.records {
                            if row.event == LogEvent::Stopped {
                                stopped = row.cause;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        (finish, stopped)
    }
}

/// The ids of a run of these tests.
struct Ids {
    run: RunId,
    agent: AgentId,
    workflow: WorkflowId,
}

fn ids() -> Ids {
    Ids {
        run: RunId::from_random([0x71; 16]),
        agent: AgentId::from_random([0x72; 16]),
        workflow: WorkflowId::from_random([0x73; 16]),
    }
}

fn slot(name: &str, value: &str) -> Slot {
    Slot {
        name: Name::parse(name).expect("a slot name"),
        value: OwnValue::new(value),
    }
}

/// The store's load of the agent of `setup` with the mail host at `port`.
fn load(setup: &Setup, ids: &Ids, port: u16) -> LoadReply {
    let mut config = halvo_policy::preset(Category::Email, ids.workflow.clone(), ids.agent.clone())
        .expect("a preset");
    config.tier = setup.tier;
    config.autonomy = setup.autonomy;
    config.allow = if setup.allow_send {
        vec![AllowedClass::Send]
    } else {
        Vec::new()
    };
    let (untrusted, private) = if setup.split {
        (YesNo::No, YesNo::Yes)
    } else {
        (YesNo::Yes, YesNo::Yes)
    };
    config.caps.untrusted = untrusted;
    config.caps.private = private;
    let at = Instant(1);
    LoadReply {
        config,
        playbook: None,
        next_playbook: NonZeroU32::MIN,
        bindings: Some(LoadedBindings {
            rev: NonZeroU32::MIN,
            origins: vec![
                WebOrigin::parse(&format!("http://mail.agent.test:{port}")).expect("an origin")
            ],
            slots: vec![
                slot("account_address", ACCOUNT),
                slot("reply_senders", SENDER),
            ],
        }),
        sessions: Vec::new(),
        start: Some(AgentStart {
            v: V1,
            workflow: ids.workflow.clone(),
            agent: ids.agent.clone(),
            day: day_of(at, 0),
            at,
        }),
    }
}

/// The agent's own row `drafted` of the message `id`, with the draft `draft-<id>`, of an earlier run.
fn drafted_row(ids: &Ids, id: &str) -> AgentReply {
    let at = Instant(2);
    AgentReply {
        v: V1,
        workflow: ids.workflow.clone(),
        agent: ids.agent.clone(),
        run: RunId::from_random([0x7e; 16]),
        seq: NonZeroU32::MIN,
        day: day_of(at, 0),
        at,
        message: ProviderId::parse(id).expect("an id"),
        draft: Some(ProviderId::parse(&format!("draft-{id}")).expect("an id")),
        effect: None,
        state: ReplyState::Drafted,
    }
}

/// The stand-in supervisor and store: it answers every frame that waits for a reply by `setup`, and records every
/// frame of the runner until the end of its output.
fn supervise(
    mut from_runner: impl Read,
    mut to_runner: impl Write,
    setup: Setup,
    port: u16,
    sends: Arc<Mutex<Vec<String>>>,
) -> (Vec<FromRunner>, Vec<Vec<u8>>, Vec<usize>) {
    let ids = ids();
    let (mut frames, mut bytes, mut at_cards) = (Vec::new(), Vec::new(), Vec::new());
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
        bytes.push(body);
        let reply = match &frame {
            FromRunner::Secret(s) => ToRunner::SecretReply(if s.secret.as_str() == "model_key" {
                match setup.key {
                    Some(key) => SecretReply::Stored(SecretText::parse(key).expect("a key")),
                    None => SecretReply::None,
                }
            } else {
                SecretReply::Stored(SecretText::parse(TOKEN).expect("a token"))
            }),
            FromRunner::Store(s) => {
                let outcome = match s.request.body() {
                    RunnerCall::Load(_) => Ok(RunnerReply::Load(load(&setup, &ids, port))),
                    RunnerCall::LoadReplies(b) => Ok(RunnerReply::Replies(LoadRepliesReply {
                        replies: b
                            .message_ids
                            .iter()
                            .filter(|id| setup.waiting.contains(&id.as_str()))
                            .map(|id| drafted_row(&ids, id.as_str()))
                            .collect(),
                        blocked: Vec::new(),
                        blocked_withheld: Vec::new(),
                    })),
                    RunnerCall::ModelHold(_) => match setup.hold {
                        Some(name) => Err(ReplyError::Workflow(
                            ErrorName::read(name).expect("an error name"),
                        )),
                        None => Ok(RunnerReply::Stored(Stored { stored: 1 })),
                    },
                    RunnerCall::Log(b) => Ok(RunnerReply::Stored(Stored {
                        stored: b.records.len() as u32,
                    })),
                    _ => Ok(RunnerReply::Stored(Stored { stored: 1 })),
                };
                ToRunner::StoreReply(StoreReplyFrame {
                    reply: StoreReply {
                        id: Some(s.request.id().clone()),
                        outcome,
                    },
                })
            }
            FromRunner::Confirm(c) => {
                at_cards.push(
                    sends
                        .lock()
                        .expect("lock")
                        .iter()
                        .filter(|r| r.starts_with("POST ") && r.contains("/send "))
                        .count(),
                );
                let approvals = c
                    .request
                    .effects
                    .iter()
                    .enumerate()
                    .map(|(i, e)| Approval {
                        approval: ApprovalId::from_random([0xa0 + i as u8; 16]),
                        effect: e.effect.clone(),
                        digest: e.digest.clone(),
                        answer: setup.answer,
                    })
                    .collect();
                ToRunner::ConfirmReply(ConfirmReply {
                    decision: ConfirmDecision { v: V1, approvals },
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
        if to_runner.write_all(&encode(&json)).is_err() {
            break;
        }
    }
    (frames, bytes, at_cards)
}

/// One replay run of the reply agent of `setup` over the mail host `mailbox`, with the stand-in door recording into
/// `record`.
fn run_once(setup: &Setup, mailbox: &Mailbox, record: &Path) -> Seen {
    let ids = ids();
    let mut words = vec![
        "--run".to_owned(),
        ids.run.as_str().to_owned(),
        "--agent".to_owned(),
        ids.agent.as_str().to_owned(),
        "--workflow".to_owned(),
        ids.workflow.as_str().to_owned(),
        "--mode".to_owned(),
        "replay".to_owned(),
        "--utc-offset".to_owned(),
        "0".to_owned(),
    ];
    if setup.hosted {
        words.extend(["--hosted".to_owned(), setup.tier.as_str().to_owned()]);
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
    let (given, port, sends) = (setup.clone(), mailbox.port, Arc::clone(&mailbox.seen));
    let supervisor =
        std::thread::spawn(move || supervise(from_runner, to_runner, given, port, sends));
    let mut runner = Runner::new(args, link);
    let mut before: Vec<OsString> = ["--exact", "--test-threads=1", "--quiet", DOOR, "--"]
        .into_iter()
        .map(OsString::from)
        .collect();
    before.push(record.as_os_str().to_owned());
    runner.stand_in_door = Some(Place {
        program: std::env::current_exe().expect("the test binary"),
        before,
    });
    let table = setup
        .open
        .then(|| crate::route::tests::policy_with(&[OPEN]));
    crate::route::TEST_POLICY.with(|cell| cell.set(table));
    let _ = crate::tests_lines();
    let status = runner.run();
    let lines = crate::tests_lines();
    crate::route::TEST_POLICY.with(|cell| cell.set(None));
    let (frames, from_runner, sends_at_cards) = supervisor.join().expect("the supervisor");
    Seen {
        frames,
        from_runner,
        sends_at_cards,
        lines,
        status,
    }
}

/// A new, empty directory of a test under the target directory's `tmp`.
fn scratch(name: &str) -> PathBuf {
    let exe = std::env::current_exe().expect("the test binary");
    let target = exe
        .ancestors()
        .nth(3)
        .expect("the target directory above deps");
    halvo_test_dir::make(&target.join("tmp").join("halvo-runner-unit-u74"), name)
        .expect("make the scratch directory")
}

/// The starts of the stand-in door that `record` holds.
fn starts(record: &Path) -> usize {
    std::fs::read_dir(record)
        .expect("the record directory")
        .filter(|e| {
            e.as_ref()
                .is_ok_and(|e| e.file_name().to_string_lossy().starts_with("started-"))
        })
        .count()
}

/// The request lines that the stand-in door got behind the key's head.
fn request_lines(record: &Path) -> Vec<(String, Value)> {
    let Ok(input) = std::fs::read(record.join("stdin.bin")) else {
        return Vec::new();
    };
    let head = format!("{}\n{KEY}\n", KEY.len());
    assert!(input.starts_with(head.as_bytes()), "the key's line first");
    input[head.len()..]
        .split(|b| *b == b'\n')
        .filter(|l| !l.is_empty())
        .map(|l| {
            let text = String::from_utf8(l.to_vec()).expect("UTF-8");
            let value = serde_json::from_str(&text).expect("a request line");
            (text, value)
        })
        .collect()
}

/// How many times `needle` stands in `hay`.
fn count(hay: &[u8], needle: &str) -> usize {
    let needle = needle.as_bytes();
    hay.windows(needle.len()).filter(|w| *w == needle).count()
}

/// The const text of A11.4.
fn constant() -> String {
    crate::connect::const_body().to_owned()
}

/// A file of the driver trial, by its path from the workspace's root.
fn trial(path: &str) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::fs::read_to_string(root.join(path)).expect("a text of the trial")
}

/// Sentence 1: the body of a reply is the writer model's text, asked through `ask` alone: one request line for each
/// message, with the trial's boundary text whole as its `system`, the trial's goal text of a reply with its end
/// trimmed as its `prompt`, the message's sender, subject and body as its `snapshot` and nothing else of the mail,
/// the grammar `text` and the pick's level `medium`, a hold before it and a settle behind it; the draft's PATCH and
/// the card hold the model's text with the source `model`. A message whose text would make the line pass the door's
/// bound is cut at its last line end that fits, so that the hold names what the door would send.
#[test]
fn a_reply_body_is_the_writers_text_asked_through_ask_alone_with_the_trials_texts() {
    let dir = scratch("sentence-1");
    let record = halvo_test_dir::make(&dir, "record").expect("the record directory");
    let long_text: String = std::iter::once("Can we meet on Friday? [plain]".to_owned())
        .chain((0..2_000).map(|n| {
            format!("Quoted line {n:04} of an earlier message, kept as the mail holds it.")
        }))
        .collect::<Vec<_>>()
        .join("\n");
    let messages = vec![
        message("m1", "Can we meet on Friday? [plain]"),
        message("m2", &long_text),
    ];
    let mailbox = Mailbox::start(messages.clone());
    let seen = run_once(&Setup::allowed(), &mailbox, &record);
    let lines = request_lines(&record);
    assert_eq!(lines.len(), 2, "one request line a message");
    let boundary = trial("tools/r3-trials/drive/prompts/boundary.txt");
    let goal = trial("tools/r3-trials/drive/prompts/goal-reply.txt");
    for (n, (text, line)) in lines.iter().enumerate() {
        let keys: Vec<&String> = line.as_object().expect("an object").keys().collect();
        assert_eq!(keys.len(), 6, "the six keys of A12.2: {keys:?}");
        assert_eq!(
            line["system"].as_str(),
            Some(boundary.as_str()),
            "line {n}: the trial's boundary text whole"
        );
        assert_eq!(
            line["prompt"].as_str(),
            Some(goal.trim_end()),
            "line {n}: the trial's goal text, its end trimmed"
        );
        assert_eq!(line["grammar"], "text");
        assert_eq!(line["level"], "medium");
        assert!(text.len() <= LINE_MAX, "line {n} fits the door's bound");
    }
    assert_eq!(
        lines[0].1["snapshot"].as_str(),
        Some(text_of(&messages[0]).as_str()),
        "the message's text alone"
    );
    let whole = text_of(&messages[1]);
    let kept = lines[1].1["snapshot"].as_str().expect("a snapshot");
    println!(
        "long message: text {} bytes, kept {} bytes, line {} bytes, holds {:?}",
        whole.len(),
        kept.len(),
        lines[1].0.len(),
        seen.holds()
    );
    assert!(
        kept.len() < whole.len() && whole.starts_with(kept),
        "the long text cut"
    );
    assert!(whole[kept.len()..].starts_with('\n'), "cut at a line end");
    assert!(
        seen.holds().iter().all(|b| *b as usize <= LINE_MAX),
        "each hold names at most the door's bound"
    );
    assert_eq!(
        seen.model_frames(),
        ["hold", "secret", "settle", "hold", "settle"]
    );
    assert_eq!(
        mailbox.patches(),
        [
            ("draft-m1".to_owned(), PLAIN.to_owned()),
            ("draft-m2".to_owned(), PLAIN.to_owned())
        ],
        "each draft holds the model's text"
    );
    assert_eq!(
        seen.cards(),
        [vec![
            (PLAIN.to_owned(), Source::Model),
            (PLAIN.to_owned(), Source::Model)
        ]],
        "the card shows the model's text with the source model"
    );
    assert_eq!(mailbox.sends(), 2, "sent after the yes");
    assert_eq!(seen.end(), (Some(Outcome::Completed), None));
    assert_eq!(seen.status, 0);
}

/// Sentence 2, its refusal and its way through: in one run, a model output with a closed thinking block before its
/// body, and one with an OPEN thinking block. No request to the mail host, no frame of the runner (log rows, reply
/// rows, effect rows, the confirmation) and no line on standard error holds a byte of either block; the kept side:
/// the first message's body stands in its PATCH and on the card with `model`, the second gets the const text with
/// `const`.
#[test]
fn a_thinking_block_stands_in_no_request_row_frame_or_card_and_an_open_block_gives_the_const_text()
{
    let dir = scratch("sentence-2");
    let record = halvo_test_dir::make(&dir, "record").expect("the record directory");
    let messages = vec![
        message("m1", "When can you come? [closed]"),
        message("m2", "Is Friday good? [open]"),
    ];
    let mailbox = Mailbox::start(messages);
    let seen = run_once(&Setup::allowed(), &mailbox, &record);
    let requests = mailbox.requests().join("\n");
    let frames: Vec<u8> = seen.from_runner.concat();
    let stderr = seen.lines.join("\n");
    let mut found = Vec::new();
    for needle in ["<think", "</think", SECRET_A, SECRET_B, "I should answer"] {
        let counts = (
            count(requests.as_bytes(), needle),
            count(&frames, needle),
            count(stderr.as_bytes(), needle),
        );
        println!(
            "block {needle:?}: mail requests {} frames {} stderr lines {}",
            counts.0, counts.1, counts.2
        );
        found.push(counts);
    }
    let kept = (
        count(requests.as_bytes(), BODY_A),
        count(&frames, BODY_A),
        count(requests.as_bytes(), &constant()),
        count(&frames, &constant()),
    );
    println!(
        "kept: the body in mail requests {} frames {}; the const text in mail requests {} frames {}; searched {} requests, {} frames, {} stderr lines",
        kept.0,
        kept.1,
        kept.2,
        kept.3,
        mailbox.requests().len(),
        seen.from_runner.len(),
        seen.lines.len()
    );
    assert!(
        found.iter().all(|c| *c == (0, 0, 0)),
        "a byte of a block: {found:?}"
    );
    assert_eq!(request_lines(&record).len(), 2, "both asked");
    assert_eq!(
        mailbox.patches(),
        [
            ("draft-m1".to_owned(), BODY_A.to_owned()),
            ("draft-m2".to_owned(), constant())
        ]
    );
    assert_eq!(
        seen.cards(),
        [vec![
            (BODY_A.to_owned(), Source::Model),
            (constant(), Source::Const)
        ]]
    );
    assert!(
        kept.0 >= 1 && kept.1 >= 1 && kept.2 >= 1 && kept.3 >= 1,
        "the kept side: {kept:?}"
    );
}

/// Sentence 2, the rule of the cut on its own: the text after the last closing mark, trimmed; the whole output where it
/// holds no mark; no body for an open block, for a mark left after the cut and for white space alone.
#[test]
fn the_cut_keeps_the_text_after_the_last_closing_mark_and_none_for_an_open_block_or_white_space() {
    let rows: [(&str, Option<&str>); 9] = [
        (
            "<think>\nThe sender asks.\n</think>\n\n  Friday suits me.  ",
            Some("Friday suits me."),
        ),
        (
            "<think>a</think> First. <think>b</think> Second.",
            Some("Second."),
        ),
        (
            "The sender asks.</think>Friday suits me.",
            Some("Friday suits me."),
        ),
        ("  Friday suits me.\n", Some("Friday suits me.")),
        ("<think>\nI should answer that", None),
        ("<think>a</think> Friday <think>b", None),
        ("<think>a</think>  \n\t ", None),
        ("", None),
        (
            "<THINK>upper</THINK> Body.",
            Some("<THINK>upper</THINK> Body."),
        ),
    ];
    for (output, want) in rows {
        assert_eq!(cut(output), want, "{output:?}");
    }
}

/// Sentence 3: a reply that a model wrote is never sent without the person's yes on a card that shows its whole
/// text, in every configuration that the code allows for a send: no send before a card's answer; each card's body
/// is the model's text whole, byte for byte, in two pieces, with `model`; `allow_once` sends it, `reject_once` does
/// not; the settings that would clear a send end the run `refused` with `approval`, and `observe` with `autonomy`,
/// each with no card and no send.
#[test]
fn a_model_written_reply_is_sent_only_after_a_yes_on_a_card_that_shows_its_whole_text_in_every_configuration(
) {
    let long = long_body();
    assert_eq!(
        OwnValue::new(&long).pieces().len(),
        2,
        "the body has two pieces"
    );
    let rows = [
        (
            "draft_and_confirm, yes",
            Autonomy::DraftAndConfirm,
            false,
            false,
            Choice::AllowOnce,
            1,
            1,
            (Some(Outcome::Completed), None),
        ),
        (
            "draft_and_confirm, no",
            Autonomy::DraftAndConfirm,
            false,
            false,
            Choice::RejectOnce,
            1,
            0,
            (Some(Outcome::Declined), Some(Cause::Declined)),
        ),
        (
            "unattended, send not allowed, yes",
            Autonomy::Unattended,
            false,
            true,
            Choice::AllowOnce,
            1,
            1,
            (Some(Outcome::Completed), None),
        ),
        (
            "unattended, send allowed, caps not split, no",
            Autonomy::Unattended,
            true,
            false,
            Choice::RejectOnce,
            1,
            0,
            (Some(Outcome::Declined), Some(Cause::Declined)),
        ),
        (
            "unattended, send allowed, caps split: clears",
            Autonomy::Unattended,
            true,
            true,
            Choice::AllowOnce,
            0,
            0,
            (Some(Outcome::Refused), Some(Cause::Approval)),
        ),
        (
            "observe",
            Autonomy::Observe,
            false,
            false,
            Choice::AllowOnce,
            0,
            0,
            (Some(Outcome::Refused), Some(Cause::Autonomy)),
        ),
    ];
    for (name, autonomy, allow_send, split, answer, cards, sends, end) in rows {
        let dir = scratch(&format!(
            "sentence-3-{}",
            name.replace([',', ' ', ':'], "-")
        ));
        let record = halvo_test_dir::make(&dir, "record").expect("the record directory");
        let mailbox = Mailbox::start(vec![message("m1", "Can you confirm the booking? [long]")]);
        let setup = Setup {
            autonomy,
            allow_send,
            split,
            answer,
            ..Setup::allowed()
        };
        let seen = run_once(&setup, &mailbox, &record);
        println!(
            "{name}: cards {} sends {} sends before each card {:?} end {:?}",
            seen.cards().len(),
            mailbox.sends(),
            seen.sends_at_cards,
            seen.end()
        );
        assert_eq!(
            mailbox.patches(),
            [("draft-m1".to_owned(), long.clone())],
            "{name}: the draft"
        );
        assert_eq!(seen.cards().len(), cards, "{name}: cards");
        for card in seen.cards() {
            assert_eq!(
                card,
                [(long.clone(), Source::Model)],
                "{name}: the whole text on the card"
            );
        }
        assert!(
            seen.sends_at_cards.iter().all(|n| *n == 0),
            "{name}: no send before the card"
        );
        assert_eq!(mailbox.sends(), sends, "{name}: sends");
        assert_eq!(seen.end(), end, "{name}: the end");
    }
}

/// Sentence 4, and the two ends beside it that the coordinator ruled: where the pick hands the call back, where it
/// names no writer for the agent's tier, where the agreement is missing and where the cap is reached, every body is
/// the const text with `const`, the run goes on to the card, and no request line goes to the door (no door at all
/// but where the hold was refused); a missing key ends the run `handed_back` with `no_model_key` before any draft;
/// a message whose draft of an earlier run waits gets no second body: no request line for it, the const text.
#[test]
fn with_no_writer_no_agreement_or_the_cap_reached_the_body_is_the_const_text_and_nothing_reaches_the_host(
) {
    let constant_cards = vec![vec![(constant(), Source::Const)]];
    let rows: [(&str, Setup, Vec<&str>); 4] = [
        (
            "the pick hands back",
            Setup {
                open: false,
                ..Setup::allowed()
            },
            vec![],
        ),
        (
            "no writer for the tier",
            Setup {
                tier: ModelTier::LowCostHosted,
                ..Setup::allowed()
            },
            vec![],
        ),
        (
            "the agreement missing",
            Setup {
                hosted: false,
                ..Setup::allowed()
            },
            vec![],
        ),
        (
            "the cap reached",
            Setup {
                hold: Some("over_cap"),
                ..Setup::allowed()
            },
            vec!["hold"],
        ),
    ];
    for (name, setup, model) in rows {
        let dir = scratch(&format!("sentence-4-{}", name.replace(' ', "-")));
        let record = halvo_test_dir::make(&dir, "record").expect("the record directory");
        let mailbox = Mailbox::start(vec![message("m1", "Can we meet on Friday? [plain]")]);
        let seen = run_once(&setup, &mailbox, &record);
        println!(
            "{name}: model frames {:?} door starts {} end {:?}",
            seen.model_frames(),
            starts(&record),
            seen.end()
        );
        assert_eq!(seen.model_frames(), model, "{name}: the model's frames");
        assert_eq!(starts(&record), 0, "{name}: no door");
        assert!(request_lines(&record).is_empty(), "{name}: no request line");
        assert_eq!(
            mailbox.patches(),
            [("draft-m1".to_owned(), constant())],
            "{name}: the draft"
        );
        assert_eq!(seen.cards(), constant_cards, "{name}: the card");
        assert_eq!(
            seen.end(),
            (Some(Outcome::Completed), None),
            "{name}: the run goes on"
        );
    }

    let dir = scratch("sentence-4-the-key-missing");
    let record = halvo_test_dir::make(&dir, "record").expect("the record directory");
    let mailbox = Mailbox::start(vec![message("m1", "Can we meet on Friday? [plain]")]);
    let seen = run_once(
        &Setup {
            key: None,
            ..Setup::allowed()
        },
        &mailbox,
        &record,
    );
    println!(
        "the key missing: model frames {:?} end {:?} createReply {}",
        seen.model_frames(),
        seen.end(),
        mailbox.count("POST", "/createReply")
    );
    assert_eq!(
        seen.end(),
        (Some(Outcome::HandedBack), Some(Cause::NoModelKey))
    );
    assert_eq!(seen.model_frames(), ["hold", "secret", "settle"]);
    assert_eq!(starts(&record), 0, "no door");
    assert_eq!(
        mailbox.count("POST", "/createReply") + mailbox.patches().len(),
        0,
        "no draft"
    );

    let dir = scratch("sentence-4-a-draft-waits");
    let record = halvo_test_dir::make(&dir, "record").expect("the record directory");
    let mailbox = Mailbox::start(vec![
        message("m1", "Can we meet on Friday? [plain]"),
        message("m2", "And on Monday? [plain]"),
    ]);
    let seen = run_once(
        &Setup {
            waiting: vec!["m1"],
            ..Setup::allowed()
        },
        &mailbox,
        &record,
    );
    let lines = request_lines(&record);
    println!(
        "a draft waits: request lines {} patches {:?}",
        lines.len(),
        mailbox.patches()
    );
    assert_eq!(lines.len(), 1, "no second body for the waiting draft");
    assert_eq!(
        lines[0].1["snapshot"]
            .as_str()
            .map(|s| s.contains("Monday")),
        Some(true)
    );
    assert_eq!(
        mailbox.patches(),
        [
            ("draft-m1".to_owned(), constant()),
            ("draft-m2".to_owned(), PLAIN.to_owned())
        ]
    );
    assert_eq!(
        seen.cards(),
        [vec![
            (constant(), Source::Const),
            (PLAIN.to_owned(), Source::Model)
        ]]
    );
}

/// The bound on a body: ten bodies at `BODY_JSON_MAX`, each beside a `to` of 320 bytes and a subject of 1,000 bytes at
/// the worst of their JSON text, fit one confirmation frame; in a run of ten messages, nine bodies at the bound stand
/// on the one card with `model`, and the tenth, one byte of JSON over it, is the const text.
#[test]
fn ten_bodies_at_the_bound_fit_one_confirm_frame_and_a_body_over_it_is_the_const_text() {
    let worst = "\u{1}".repeat(BODY_JSON_MAX / 6 - 1);
    let mut body = worst.clone();
    while json_bytes(&body) < BODY_JSON_MAX {
        body.push('x');
    }
    assert_eq!(json_bytes(&body), BODY_JSON_MAX);
    let field = |name, from, value: &str| BoundField {
        name,
        from,
        origin: None,
        value: OwnValue::new(value),
    };
    let effects = (0..10u8)
        .map(|n| {
            let bound = Bound::new(
                ActionClass::Send,
                vec![
                    field(FieldName::To, Source::Extract, &"\u{1}".repeat(320)),
                    field(FieldName::Subject, Source::Extract, &"s".repeat(1_000)),
                    field(FieldName::Body, Source::Model, &body),
                ],
            )
            .expect("a bound object");
            ConfirmEffect {
                effect: EffectId::from_random([n + 1; 16]),
                class: ActionClass::Send,
                digest: halvo_policy::digest(&bound).expect("a digest"),
                bound,
                new_origin: None,
            }
        })
        .collect();
    let ids = ids();
    let frame = FromRunner::Confirm(Confirm {
        request: ConfirmRequest {
            v: V1,
            run: ids.run,
            agent: ids.agent,
            cut: None,
            effects,
        },
    });
    let json = frame.to_json().expect("the frame's JSON");
    println!(
        "ten bodies at the bound at the worst: frame {} bytes of {FRAME_JSON_MAX}",
        json.len()
    );
    assert!(json.len() <= FRAME_JSON_MAX);

    let dir = scratch("bound");
    let record = halvo_test_dir::make(&dir, "record").expect("the record directory");
    let messages: Vec<Value> = (1..=10)
        .map(|n| {
            message(
                &format!("m{n:02}"),
                if n == 10 {
                    "Long? [over]"
                } else {
                    "Long? [bound]"
                },
            )
        })
        .collect();
    let mailbox = Mailbox::start(messages);
    let seen = run_once(&Setup::allowed(), &mailbox, &record);
    let at = at_bound(0);
    let mut want: Vec<(String, Source)> = (0..9).map(|_| (at.clone(), Source::Model)).collect();
    want.push((constant(), Source::Const));
    let card = seen
        .frames
        .iter()
        .zip(&seen.from_runner)
        .find(|(f, _)| matches!(f, FromRunner::Confirm(_)))
        .map(|(_, bytes)| bytes.len());
    println!(
        "a run of ten: the card's frame {card:?} bytes; request lines {}",
        request_lines(&record).len()
    );
    assert_eq!(
        seen.cards(),
        [want],
        "nine at the bound, the tenth the const text"
    );
    assert!(card.is_some_and(|n| n <= FRAME_JSON_MAX));
    assert_eq!(mailbox.sends(), 10);
}
