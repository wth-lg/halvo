//! The variants fixture server of tools/fixture-server/variants/ for the runner's tests of the connect adapters
//! (A11.2), started with `node` on a port of its own on the loopback, and a test store of the rows of answered
//! messages (A7.4 `agent_reply`) that answers `load_replies` and keeps each `reply_row`.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use halvo_schema::executable::body::{KeptReply, LoadRepliesReply, RunnerCall, RunnerReply};
use halvo_schema::executable::day::day_of;
use halvo_schema::executable::frame::{
    Approval, ConfirmDecision, ConfirmEffect, ConfirmReply, SecretReply, StoreReply,
    StoreReplyFrame,
};
use halvo_schema::executable::names::{Answer, FieldName};
use halvo_schema::executable::text::{SecretText, V1};
use halvo_schema::executable::{
    AgentReply, AgentRun, AgentStart, ApprovalId, Bound, Cause, EffectId, EffectState, FromRunner,
    ProviderId, ReplyState, ToRunner, WebOrigin,
};
use halvo_schema::record::Instant;
use halvo_schema::Category;
use serde_json::Value;

use super::{is_op, load_with, slot, Act, Runner, Seen, Supervisor, LONG};

/// The token of the seeds' credentials, the one that the fixture's Graph hosts take.
pub const TOKEN: &str = "Zq7-halvo-marker-token";
/// The account's address of seed 1.
pub const ACCOUNT: &str = "resident.one@agent.test";
/// The two senders of seed 1 as one slot value.
pub const SENDERS: &str = "ada.vell@people.agent.test,bo.tarrin@people.agent.test";

/// The fixture server, ended when the value is dropped.
pub struct Fixture {
    child: Child,
    port: u16,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Fixture {
    /// Starts the server on a port that the system gives and waits for its line `listening 127.0.0.1:<port>`.
    pub fn start() -> Fixture {
        let server = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/fixture-server/variants/server.mjs");
        let mut child = Command::new("node")
            .arg(server)
            .args(["--port", "0"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("node starts the fixture server");
        let stdout = child.stdout.take().expect("its standard output");
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            let _ = BufReader::new(stdout).read_line(&mut line);
            let _ = tx.send(line);
        });
        let line = rx.recv_timeout(LONG).unwrap_or_default();
        let port = line
            .trim_end()
            .strip_prefix("listening 127.0.0.1:")
            .and_then(|port| port.parse::<u16>().ok());
        match port {
            Some(port) => Fixture { child, port },
            None => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("the fixture server printed no line listening 127.0.0.1:<port>")
            }
        }
    }

    /// The origin of the fixture's host `host` (`mail`, `calendar`), which the loopback route maps to the server.
    pub fn origin(&self, host: &str) -> WebOrigin {
        WebOrigin::parse(&format!("http://{host}.agent.test:{}", self.port)).expect("an origin")
    }

    /// Sets the state of seed `seed` again with the variants `variants`.
    pub fn reset(&self, seed: u8, variants: &[&str]) {
        let mut path = format!("/reset?seed={seed}");
        for variant in variants {
            path.push_str("&variant=");
            path.push_str(variant);
        }
        self.control("POST", &path);
    }

    /// The state object of the control host.
    pub fn state(&self) -> Value {
        self.control("GET", "/state")
    }

    /// The counts of requests that the fixture took since its reset: all, the state-changing ones (createReply,
    /// PATCH, send, DELETE) and those answered 401.
    pub fn requests(&self) -> (u64, u64, u64) {
        let state = self.state();
        let count = |key: &str| state["requests"][key].as_u64().expect("a count");
        (
            count("total"),
            count("state_changing"),
            count("unauthorized"),
        )
    }

    /// The number of drafts that the mail host holds.
    pub fn drafts(&self) -> u64 {
        self.state()["mail"]["drafts"].as_u64().expect("a count")
    }

    /// The mails that the mail host sent, in order, each as its `to` and the id of the message that it answered.
    pub fn sent(&self) -> Vec<(String, String)> {
        let text = |value: &Value| value.as_str().expect("a text").to_owned();
        self.state()["mail"]["sent"]
            .as_array()
            .expect("a list")
            .iter()
            .map(|sent| (text(&sent["to"]), text(&sent["in_reply_to"])))
            .collect()
    }

    /// The ids of the events that the calendar host deleted, in order.
    pub fn deleted(&self) -> Vec<String> {
        self.state()["calendar"]["deleted"]
            .as_array()
            .expect("a list")
            .iter()
            .map(|id| id.as_str().expect("a text").to_owned())
            .collect()
    }

    /// Has the next request to a Graph host answered 429 with `Retry-After: <seconds>`.
    pub fn throttle(&self, seconds: u32) {
        self.control("POST", &format!("/throttle?seconds={seconds}"));
    }

    /// Ends the server now, so that its port takes no connection; no call of the fixture can follow.
    pub fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    fn control(&self, method: &str, path: &str) -> Value {
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, self.port));
        let mut stream =
            TcpStream::connect(address).expect("the fixture server takes a connection");
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .expect("a read timeout");
        write!(
            stream,
            "{method} {path} HTTP/1.0\r\nHost: control.agent.test:{}\r\nContent-Length: 0\r\n\r\n",
            self.port
        )
        .expect("the request is written");
        let mut answer = Vec::new();
        stream.read_to_end(&mut answer).expect("the answer is read");
        let answer = String::from_utf8(answer).expect("a text answer");
        let (head, body) = answer.split_once("\r\n\r\n").expect("a head and a body");
        assert!(
            head.lines()
                .next()
                .is_some_and(|line| line.contains(" 200 ")),
            "the control host takes the call"
        );
        serde_json::from_str(body).expect("a JSON answer")
    }
}

/// A reply agent of seed 1 whose bindings name the fixture's mail host, `account` in the slot `account_address` and
/// `senders` in the slot `reply_senders`.
pub fn reply_agent(n: u8, fixture: &Fixture, account: &str, senders: &str) -> Supervisor {
    let mut sup = Supervisor::new(
        n,
        Category::Email,
        vec![
            slot("account_address", account),
            slot("reply_senders", senders),
        ],
    );
    sup.origins = vec![fixture.origin("mail")];
    sup
}

/// A clean-up agent whose bindings name the fixture's calendar host.
pub fn cleanup_agent(n: u8, fixture: &Fixture) -> Supervisor {
    let mut sup = Supervisor::new(n, Category::CalendarCleanup, Vec::new());
    sup.origins = vec![fixture.origin("calendar")];
    sup
}

/// The start row of `sup`'s agent at `at`.
pub fn start_row(sup: &Supervisor, at: Instant) -> AgentStart {
    AgentStart {
        v: V1,
        workflow: sup.workflow.clone(),
        agent: sup.agent.clone(),
        day: day_of(at, 0),
        at,
    }
}

/// A row of `sup`'s agent for the message `message`.
pub fn reply_row(
    sup: &Supervisor,
    message: &str,
    seq: u32,
    state: ReplyState,
    draft: Option<&str>,
) -> AgentReply {
    let effect = matches!(
        state,
        ReplyState::Begun | ReplyState::Committed | ReplyState::Unknown
    )
    .then(|| EffectId::from_random([0x5e; 16]));
    AgentReply {
        v: V1,
        workflow: sup.workflow.clone(),
        agent: sup.agent.clone(),
        run: sup.run.clone(),
        seq: std::num::NonZeroU32::new(seq).expect("a seq from 1"),
        day: 1,
        at: Instant(1),
        message: ProviderId::parse(message).expect("an id"),
        draft: draft.map(|d| ProviderId::parse(d).expect("an id")),
        effect,
        state,
    }
}

/// The test's store of the rows of answered messages: the rows of every agent, and the ids that another agent's
/// rows block for the asking agent, as bare ids of the two lists of the reply of `load_replies`.
#[derive(Default)]
pub struct ReplyStore {
    pub rows: Vec<AgentReply>,
    pub blocked: Vec<String>,
    pub blocked_withheld: Vec<String>,
}

impl ReplyStore {
    /// The reply to the `load_replies` frame `frame`: for each id asked that has a row of the asking agent its row
    /// of the highest `seq`, in the order asked, and the ids asked that stand in the two lists.
    pub fn answer(&self, frame: &FromRunner) -> ToRunner {
        let FromRunner::Store(s) = frame else {
            panic!("not a store frame")
        };
        let RunnerCall::LoadReplies(body) = s.request.body() else {
            panic!("not a load_replies body")
        };
        let replies = body
            .message_ids
            .iter()
            .filter_map(|id| {
                self.rows
                    .iter()
                    .filter(|row| row.agent == body.agent && row.message == *id)
                    .max_by_key(|row| row.seq)
                    .cloned()
            })
            .collect();
        let asked = |list: &[String]| -> Vec<ProviderId> {
            body.message_ids
                .iter()
                .filter(|id| list.iter().any(|listed| listed == id.as_str()))
                .cloned()
                .collect()
        };
        ToRunner::StoreReply(StoreReplyFrame {
            reply: StoreReply {
                id: Some(s.request.id().clone()),
                outcome: Ok(RunnerReply::Replies(LoadRepliesReply {
                    replies,
                    blocked: asked(&self.blocked),
                    blocked_withheld: asked(&self.blocked_withheld),
                })),
            },
        })
    }

    /// The reply to the `load_reply_body` frame `frame` (r3 U7.9): no kept text, since this store keeps the row of
    /// each `reply_row` alone and no text beside it.
    pub fn kept(&self, frame: &FromRunner) -> ToRunner {
        let FromRunner::Store(s) = frame else {
            panic!("not a store frame")
        };
        ToRunner::StoreReply(StoreReplyFrame {
            reply: StoreReply {
                id: Some(s.request.id().clone()),
                outcome: Ok(RunnerReply::Kept(KeptReply { kept: None })),
            },
        })
    }

    /// The rows of the agent of `sup` for `message`, in the order stored.
    pub fn of(&self, sup: &Supervisor, message: &str) -> Vec<AgentReply> {
        self.rows
            .iter()
            .filter(|row| row.agent == sup.agent && row.message.as_str() == message)
            .cloned()
            .collect()
    }
}

/// Serves a run of `sup` that reads the stored secret `token` at every `secret`, gets a load whose start row stands
/// at `start` (none for `None`), the rows of `store` at each `load_replies`, whose each `reply_row` `store` keeps,
/// and whose each confirmation the person answers `reject_once` for every effect.
pub fn serve_run(
    sup: &Supervisor,
    token: &str,
    start: Option<Instant>,
    store: &mut ReplyStore,
) -> Seen {
    serve_run_then(sup, token, start, store, |_| None)
}

/// `serve_run`, in which `first` may take the act for a frame before the rules of `serve_run`.
pub fn serve_run_then(
    sup: &Supervisor,
    token: &str,
    start: Option<Instant>,
    store: &mut ReplyStore,
    mut first: impl FnMut(&FromRunner) -> Option<Act>,
) -> Seen {
    sup.serve(
        |frame, _| first(frame).unwrap_or_else(|| rule(sup, token, start, store, frame)),
        LONG,
    )
}

/// The act of `serve_run` for `frame`.
fn rule(
    sup: &Supervisor,
    token: &str,
    start: Option<Instant>,
    store: &mut ReplyStore,
    frame: &FromRunner,
) -> Act {
    if matches!(frame, FromRunner::Secret(_)) {
        return secret(token);
    }
    if is_op(frame, "load") {
        return load_with(sup, frame, |l| l.start = start.map(|at| start_row(sup, at)));
    }
    if is_op(frame, "load_replies") {
        return Act::With(store.answer(frame));
    }
    if is_op(frame, "load_reply_body") {
        return Act::With(store.kept(frame));
    }
    if matches!(frame, FromRunner::Confirm(_)) {
        return decision(frame, |_| Answer::RejectOnce);
    }
    if let FromRunner::Store(s) = frame {
        if let RunnerCall::ReplyRow(body) = s.request.body() {
            store.rows.push(body.record.clone());
        }
    }
    Act::Answer
}

/// The reply to a `secret` frame that gives the stored secret `token`.
pub fn secret(token: &str) -> Act {
    let value = SecretText::parse(token).expect("a stored secret");
    Act::With(ToRunner::SecretReply(SecretReply::Stored(value)))
}

/// The person's decision on the confirmation `frame`: `answer` for each of its effects, each approval with an id of
/// its own and the digest that the card shows.
pub fn decision(frame: &FromRunner, answer: impl Fn(&ConfirmEffect) -> Answer) -> Act {
    let FromRunner::Confirm(c) = frame else {
        panic!("not a confirm frame")
    };
    let approvals = c
        .request
        .effects
        .iter()
        .enumerate()
        .map(|(i, e)| Approval {
            approval: ApprovalId::from_random([0xa0 + u8::try_from(i).expect("an index"); 16]),
            effect: e.effect.clone(),
            digest: e.digest.clone(),
            answer: answer(e),
        })
        .collect();
    Act::With(ToRunner::ConfirmReply(ConfirmReply {
        decision: ConfirmDecision { v: V1, approvals },
    }))
}

/// The value of the bound field `to` of `bound`.
pub fn bound_to(bound: &Bound) -> String {
    bound
        .fields()
        .iter()
        .find(|f| f.name == FieldName::To)
        .map(|f| f.value.joined())
        .expect("a field to")
}

/// The value of the bound field `items` of `bound`.
pub fn bound_items(bound: &Bound) -> String {
    bound
        .fields()
        .iter()
        .find(|f| f.name == FieldName::Items)
        .map(|f| f.value.joined())
        .expect("a field items")
}

/// How long `serve_run_holding` keeps the link open after the frame that it does not answer.
pub const HOLD: Duration = Duration::from_millis(1500);

/// `serve_run_then` until the first frame for which `held` holds: that frame gets no answer, the runner's standard
/// input is closed `HOLD` after it, and every frame that the runner writes until its end is read and kept.
pub fn serve_run_holding(
    sup: &Supervisor,
    token: &str,
    start: Option<Instant>,
    store: &mut ReplyStore,
    mut first: impl FnMut(&FromRunner) -> Option<Act>,
    held: impl Fn(&FromRunner) -> bool,
) -> Seen {
    let mut runner = Runner::start(&sup.args());
    let mut frames = Vec::new();
    let mut closed_at = None;
    let mut closer = None;
    while let Some(frame) = runner.recv() {
        frames.push(frame.clone());
        if closer.is_some() {
            continue;
        }
        if held(&frame) {
            let input = runner.input.take();
            closer = Some(std::thread::spawn(move || {
                std::thread::sleep(HOLD);
                drop(input);
            }));
            continue;
        }
        let act = first(&frame).unwrap_or_else(|| rule(sup, token, start, store, &frame));
        if !sup.act(&mut runner, &frame, act, &mut closed_at) {
            break;
        }
    }
    if let Some(closer) = closer {
        closer.join().expect("the closer");
    }
    let status = runner.wait_within(LONG);
    if status.is_none() {
        runner.kill();
    }
    let stderr = runner.stderr();
    Seen {
        data_dir: runner.data_dir.clone(),
        line: runner.line.clone(),
        frames,
        status,
        stderr,
        close_to_end: None,
        group_gone: None,
    }
}

/// Whether `frame` is a `reply_row` of the state `state`.
pub fn is_reply_row(frame: &FromRunner, state: ReplyState) -> bool {
    matches!(frame, FromRunner::Store(s)
        if matches!(s.request.body(), RunnerCall::ReplyRow(b) if b.record.state == state))
}

/// The rows of the effects of a run in the order sent, each as the index of its effect among the run's effects by
/// their first rows, its state and its cause.
pub fn effect_rows(seen: &Seen) -> Vec<(usize, EffectState, Option<Cause>)> {
    let mut ids: Vec<EffectId> = Vec::new();
    seen.effects()
        .into_iter()
        .map(|e| {
            let at = ids
                .iter()
                .position(|id| *id == e.effect)
                .unwrap_or_else(|| {
                    ids.push(e.effect.clone());
                    ids.len() - 1
                });
            (at, e.state, e.cause)
        })
        .collect()
}

/// The rows of each `reply_row` that the runner sent for `message`, in order, each as its `seq`, its state, its
/// draft and the index of its effect among the run's effects by their first rows.
pub fn reply_rows_of(
    seen: &Seen,
    message: &str,
) -> Vec<(u32, ReplyState, Option<String>, Option<usize>)> {
    let mut ids: Vec<EffectId> = Vec::new();
    for e in seen.effects() {
        if !ids.contains(&e.effect) {
            ids.push(e.effect);
        }
    }
    seen.calls()
        .into_iter()
        .filter_map(|call| match call {
            RunnerCall::ReplyRow(body) if body.record.message.as_str() == message => Some((
                body.record.seq.get(),
                body.record.state,
                body.record.draft.as_ref().map(|d| d.as_str().to_owned()),
                body.record.effect.as_ref().map(|e| {
                    ids.iter()
                        .position(|id| id == e)
                        .expect("an effect of the run")
                }),
            )),
            _ => None,
        })
        .collect()
}

/// The ids of each `load_replies` that the runner sent, in order.
pub fn asked(seen: &Seen) -> Vec<Vec<String>> {
    seen.calls()
        .into_iter()
        .filter_map(|call| match call {
            RunnerCall::LoadReplies(body) => Some(
                body.message_ids
                    .iter()
                    .map(|id| id.as_str().to_owned())
                    .collect(),
            ),
            _ => None,
        })
        .collect()
}

/// The rows of each `reply_row` that the runner sent, in order, each as its message, `seq`, state and draft.
pub fn replies(seen: &Seen) -> Vec<(String, u32, ReplyState, Option<String>)> {
    seen.calls()
        .into_iter()
        .filter_map(|call| match call {
            RunnerCall::ReplyRow(body) => Some((
                body.record.message.as_str().to_owned(),
                body.record.seq.get(),
                body.record.state,
                body.record.draft.as_ref().map(|d| d.as_str().to_owned()),
            )),
            _ => None,
        })
        .collect()
}

/// The keys of the run's list of the record `run` (`RUN_LIST_KEYS`), each with its value as the record's JSON
/// writes it.
pub fn list_keys(run: &AgentRun) -> Vec<(String, Value)> {
    let text = run.to_json().expect("the record writes");
    let value: Value = serde_json::from_str(&text).expect("JSON");
    halvo_schema::executable::record::RUN_LIST_KEYS
        .iter()
        .map(|key| ((*key).to_owned(), value[*key].clone()))
        .collect()
}

/// Whether every key of the run's list of `run` is null.
pub fn keys_null(run: &AgentRun) -> bool {
    list_keys(run).iter().all(|(_, value)| value.is_null())
}
