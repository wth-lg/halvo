//! The tests of the handback (A6.4, A8.4), each on a stand-in of the executor (`StandIn`), with no browser and no
//! window: the stand-in logs every call that the runner makes of the browser, and the test's supervisor, which reads
//! the runner's frames in the same process, logs each frame and each answer that it gives in the same log, so that
//! the log holds the order of the runner's commands and frames.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::io::{Read, Write};
use std::num::NonZeroU32;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use halvo_policy::preset;
use halvo_schema::executable::body::{LoadReply, LoadedBindings};
use halvo_schema::executable::names::Verb;
use halvo_schema::executable::step::Cond;
use halvo_schema::executable::{
    Cause, ExecutableStep, FromRunner, Outcome, RunnerCall, ToRunner, WebOrigin,
};
use halvo_schema::{AgentId, Category, WorkflowId};
use halvo_web::cdp::{asks_to_sign_in, PausedRequest, SessionState, Sign};

use crate::link::{encode, Link};
use crate::run::{Failure, Runner, Stop};
use crate::web::{Act, Page, Web};

type Log = Arc<Mutex<Vec<String>>>;

fn note(log: &Log, line: impl Into<String>) {
    log.lock().expect("the log").push(line.into());
}

/// The bound origin of the agent in every test.
const BOUND: &str = "http://127.0.0.1:8080";

/// The stand-in of the executor: each reading gives the next sign that the test queued (none once the queue is
/// empty), the address is the one that the test gave, the first `act_fails` acts fail, as the executor refuses an
/// act on a field that turns into a credential field at the act's own focus, and every call is one line of the log.
struct StandIn {
    log: Log,
    signs: VecDeque<Option<Sign>>,
    /// The addresses that the reads give first, in order, before `address`.
    addresses: VecDeque<String>,
    address: String,
    /// How many of the acts that come fail, the first ones.
    act_fails: usize,
    /// The links and buttons that the page shows, each by its role and its accessible name, which the second reading
    /// reads by halvo-web's own rule (`asks_to_sign_in`), as the executor reads the lines of its snapshot.
    controls: Vec<(&'static str, &'static str)>,
}

impl Page for StandIn {
    fn frame(&self) -> Option<FromRunner> {
        None
    }

    fn arm(&mut self, origins: &[WebOrigin]) -> Result<(), ()> {
        let origins: Vec<&str> = origins.iter().map(WebOrigin::as_str).collect();
        note(&self.log, format!("arm fail [{}]", origins.join(" ")));
        Ok(())
    }

    fn carry_out(
        &mut self,
        _: &[(&Cond, Option<String>)],
        act: &Act<'_>,
        _: &[(&Cond, Option<String>)],
    ) -> Result<Option<String>, ()> {
        note(&self.log, format!("act {}", act.verb()));
        if self.act_fails > 0 {
            self.act_fails -= 1;
            return Err(());
        }
        Ok(None)
    }

    fn record(&mut self) -> Vec<PausedRequest> {
        Vec::new()
    }

    fn record_after_a_fault(&mut self) -> Vec<PausedRequest> {
        Vec::new()
    }

    fn stop(self: Box<Self>) -> bool {
        note(&self.log, "stop");
        true
    }

    fn sign(&mut self) -> Result<Option<Sign>, ()> {
        note(&self.log, "read");
        Ok(self.signs.pop_front().flatten())
    }

    fn asks_for_a_sign_in(&mut self) -> Result<bool, ()> {
        note(&self.log, "read sign-in");
        Ok(self
            .controls
            .iter()
            .any(|(role, name)| asks_to_sign_in(role, name)))
    }

    fn fail_held(&mut self) -> Result<(), ()> {
        note(&self.log, "fail held");
        Ok(())
    }

    fn close_others(&mut self) -> Result<(), ()> {
        note(&self.log, "close others");
        Ok(())
    }

    fn clear_stores(&mut self) -> Result<(), ()> {
        note(&self.log, "clear stores");
        Ok(())
    }

    fn load_again(&mut self) -> Result<(), ()> {
        note(&self.log, "load again");
        Ok(())
    }

    fn pass_all(&mut self) -> Result<(), ()> {
        note(&self.log, "pass all");
        Ok(())
    }

    fn address(&mut self) -> Result<String, ()> {
        note(&self.log, "address");
        Ok(self
            .addresses
            .pop_front()
            .unwrap_or_else(|| self.address.clone()))
    }

    fn export(&mut self, _: &WebOrigin) -> Result<SessionState, ()> {
        Err(())
    }

    fn import(&mut self, _: &SessionState) -> Result<(), ()> {
        Err(())
    }
}

/// The runner's standard input: the bytes that the supervisor sends, an end of file once every sender is gone.
struct Input {
    from: mpsc::Receiver<Vec<u8>>,
    bytes: Vec<u8>,
    at: usize,
}

impl Read for Input {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        while self.at == self.bytes.len() {
            match self.from.recv() {
                Ok(bytes) => (self.bytes, self.at) = (bytes, 0),
                Err(_) => return Ok(0),
            }
        }
        let n = out.len().min(self.bytes.len() - self.at);
        out[..n].copy_from_slice(&self.bytes[self.at..self.at + n]);
        self.at += n;
        Ok(n)
    }
}

/// The supervisor, on the runner's standard output: it answers each `store` frame as the store does at once, and
/// each handback request and each `bind` by the next answer that the test queued, 100 ms after the frame, the
/// answer's line in the log written just before it is sent.
struct Supervisor {
    log: Log,
    /// The JSON of each handback request, as the runner wrote it.
    requests: Log,
    to_runner: mpsc::Sender<Vec<u8>>,
    answers: VecDeque<&'static str>,
    bytes: Vec<u8>,
}

/// A frame's line in the log.
fn line_of(frame: &FromRunner) -> String {
    match frame {
        FromRunner::Store(s) => format!("frame store {}", s.request.body().op()),
        FromRunner::Handback(h) => format!("frame handback {}", h.request.reason.as_str()),
        FromRunner::Bind(b) => format!("frame bind {}", b.origin.as_str()),
        FromRunner::Done(d) => format!("frame done {}", d.outcome.as_str()),
        other => format!("frame {}", other.to_json().unwrap_or_default()),
    }
}

impl Supervisor {
    fn answer(&mut self, frame: &FromRunner) {
        let reply = match frame {
            FromRunner::Store(s) => {
                let stored = match s.request.body() {
                    RunnerCall::Log(b) => b.records.len(),
                    _ => 1,
                };
                let text = format!(
                    r#"{{"t":"store_reply","reply":{{"v":1,"id":"{}","ok":{{"stored":{stored}}}}}}}"#,
                    s.request.id().as_str()
                );
                let _ = self.to_runner.send(encode(&text));
                return;
            }
            FromRunner::Handback(_) | FromRunner::Bind(_) => self
                .answers
                .pop_front()
                .expect("an answer that the test queued"),
            _ => return,
        };
        let (log, to_runner) = (Arc::clone(&self.log), self.to_runner.clone());
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            let frame = ToRunner::parse(reply.as_bytes()).expect("an answer of the supervisor");
            let line = match &frame {
                ToRunner::HandbackReply(h) => format!("result {}", h.result.outcome.as_str()),
                ToRunner::BindReply(b) => format!("bind_reply {}", b.bound.as_str()),
                _ => "answer".to_string(),
            };
            note(&log, line);
            let _ = to_runner.send(encode(reply));
        });
    }
}

impl Write for Supervisor {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.bytes.extend_from_slice(bytes);
        while self.bytes.len() >= 4 {
            let length =
                u32::from_be_bytes([self.bytes[0], self.bytes[1], self.bytes[2], self.bytes[3]]);
            let end = 4 + length as usize;
            if self.bytes.len() < end {
                break;
            }
            let json: Vec<u8> = self.bytes.drain(..end).skip(4).collect();
            let frame = FromRunner::parse(&json).expect("a frame of the runner");
            note(&self.log, line_of(&frame));
            if matches!(frame, FromRunner::Handback(_)) {
                note(&self.requests, String::from_utf8_lossy(&json));
            }
            self.answer(&frame);
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

const DONE: &str = r#"{"t":"handback_reply","result":{"v":1,"outcome":"done","secret":null}}"#;
const DECLINED: &str =
    r#"{"t":"handback_reply","result":{"v":1,"outcome":"declined","secret":null}}"#;
const TIMEOUT: &str =
    r#"{"t":"handback_reply","result":{"v":1,"outcome":"timeout","secret":null}}"#;
const BOUND_YES: &str = r#"{"t":"bind_reply","bound":"yes"}"#;
const BOUND_NO: &str = r#"{"t":"bind_reply","bound":"no"}"#;

/// A runner whose browser is a stand-in, its supervisor, the shared log and the load of its run.
struct Bench {
    runner: Runner,
    log: Log,
    requests: Log,
    loaded: LoadReply,
}

impl Bench {
    /// A runner of an agent bound to `BOUND`, whose stand-in's readings give `signs` and whose page holds `address`,
    /// and whose supervisor answers each handback request and `bind` by `answers` in order.
    fn new(signs: &[Option<Sign>], address: &str, answers: &[&'static str]) -> Bench {
        Bench::with(signs, address, answers, false, &[])
    }

    /// `new`, with a stand-in whose acts fail where `act_fails` holds and whose reads of the address give
    /// `addresses` first.
    fn with(
        signs: &[Option<Sign>],
        address: &str,
        answers: &[&'static str],
        act_fails: bool,
        addresses: &[&str],
    ) -> Bench {
        let act_fails = if act_fails { usize::MAX } else { 0 };
        Bench::on_page(signs, address, answers, act_fails, addresses, &[])
    }

    /// `with`, with a stand-in whose first `act_fails` acts fail, on a page that shows the links and buttons
    /// `controls`.
    fn on_page(
        signs: &[Option<Sign>],
        address: &str,
        answers: &[&'static str],
        act_fails: usize,
        addresses: &[&str],
        controls: &[(&'static str, &'static str)],
    ) -> Bench {
        let log: Log = Arc::default();
        let requests: Log = Arc::default();
        let (to_runner, from) = mpsc::channel();
        let supervisor = Supervisor {
            log: Arc::clone(&log),
            requests: Arc::clone(&requests),
            to_runner,
            answers: answers.iter().copied().collect(),
            bytes: Vec::new(),
        };
        let input = Input {
            from,
            bytes: Vec::new(),
            at: 0,
        };
        let link = Link::start(Box::new(input), Box::new(supervisor), Box::new(|| {}));
        let (workflow, agent) = (
            WorkflowId::from_random([7; 16]),
            AgentId::from_random([6; 16]),
        );
        let run = "0d".repeat(16);
        let words = [
            "--run",
            &run,
            "--agent",
            agent.as_str(),
            "--workflow",
            workflow.as_str(),
            "--mode",
            "replay",
            "--browser",
            "none",
            "--utc-offset",
            "0",
        ];
        let words: Vec<OsString> = words.iter().map(OsString::from).collect();
        let args = crate::args::parse(&words).expect("the arguments");
        let mut runner = Runner::new(args, link);
        runner.web = Some(Web(Box::new(StandIn {
            log: Arc::clone(&log),
            signs: signs.iter().copied().collect(),
            addresses: addresses.iter().map(|a| a.to_string()).collect(),
            address: address.to_string(),
            act_fails,
            controls: controls.to_vec(),
        })));
        let config = preset(Category::UtilityBillPayment, workflow, agent).expect("a preset");
        let loaded = LoadReply {
            config,
            playbook: None,
            next_playbook: NonZeroU32::MIN,
            bindings: Some(LoadedBindings {
                rev: NonZeroU32::MIN,
                origins: vec![WebOrigin::parse(BOUND).expect("an origin")],
                slots: Vec::new(),
            }),
            sessions: Vec::new(),
            start: None,
        };
        Bench {
            runner,
            log,
            requests,
            loaded,
        }
    }

    fn step(&mut self, step: &ExecutableStep) -> Result<(), Failure> {
        self.runner.web_step(step, &self.loaded)
    }

    /// Ends the run as `run` ends it after `end`: the browser stopped, then the run's end written. The log then.
    fn end(self, end: Result<Outcome, Stop>) -> Vec<String> {
        let Bench {
            mut runner, log, ..
        } = self;
        runner.stop_browser();
        let _ = runner.close(end);
        let log = log.lock().expect("the log").clone();
        log
    }

    fn log(&self) -> Vec<String> {
        self.log.lock().expect("the log").clone()
    }

    /// The JSON of each handback request that the runner wrote.
    fn requests(&self) -> Vec<String> {
        self.requests.lock().expect("the requests").clone()
    }
}

fn web_step(json: &str) -> ExecutableStep {
    ExecutableStep::parse(json.as_bytes()).expect("a web step")
}

fn handback() -> ExecutableStep {
    web_step(
        r#"{"v":1,"n":1,"kind":"web","verb":"handback","target":null,"args":[{"name":"reason","from":"const","value":"sign_in","key":null}],"pre":[],"post":[],"effect":"none","class":"credential"}"#,
    )
}

fn click() -> ExecutableStep {
    web_step(
        r#"{"v":1,"n":2,"kind":"web","verb":"click","target":{"desc":"the form's button","frame":[],"credential":"no","locators":[{"by":"test_id","value":"send","name":null}]},"args":[],"pre":[],"post":[],"effect":"none","class":"prepare"}"#,
    )
}

fn fill() -> ExecutableStep {
    web_step(
        r#"{"v":1,"n":2,"kind":"web","verb":"fill","target":{"desc":"the name field","frame":[],"credential":"no","locators":[{"by":"test_id","value":"name","name":null}]},"args":[{"name":"text","from":"const","value":"Ada","key":null}],"pre":[],"post":[],"effect":"none","class":"prepare"}"#,
    )
}

/// A fill of a target marked `credential`: a step that acts on nothing, and fails where the page shows no credential
/// field.
fn credential_fill() -> ExecutableStep {
    web_step(
        r#"{"v":1,"n":2,"kind":"web","verb":"fill","target":{"desc":"the password field","frame":[],"credential":"yes","locators":[{"by":"test_id","value":"password","name":null}]},"args":[{"name":"text","from":"const","value":"x","key":null}],"pre":[],"post":[],"effect":"none","class":"credential"}"#,
    )
}

/// A case of `each_end_without_a_bind_ends_the_run`: the readings, the address, the answers, the end, and how many
/// handback requests come before it.
type Case = (
    &'static [Option<Sign>],
    &'static str,
    &'static [&'static str],
    (Outcome, Option<Cause>),
    usize,
);

/// The end that a step's failure gives the run, where it stops the run.
fn stop_of(got: Result<(), Failure>) -> Option<(Outcome, Option<Cause>)> {
    match got {
        Err(Failure::Stop(Stop::End(outcome, cause))) => Some((outcome, cause)),
        _ => None,
    }
}

fn at(log: &[String], line: &str) -> usize {
    log.iter()
        .position(|l| l == line)
        .unwrap_or_else(|| panic!("no line {line:?} in {log:?}"))
}

fn count(log: &[String], line: &str) -> usize {
    log.iter().filter(|l| *l == line).count()
}

/// A8.4 `acts_on_credential_page` (CP-177): on a page that shows a credential field, a click on the form's button
/// and a fill of the name field each make the runner read the page first and open a handback request; the stand-in
/// gets no command to the control, and no act lets a request leave. A fill of a field that turns into a credential
/// field at the act's own focus, which the executor refuses, makes the runner read the page again before the step
/// ends and open a handback, with no second act (the coordinator's word of log line 13906, the round of part 1).
#[test]
fn refusal_acts_on_credential_page() {
    for step in [click(), fill()] {
        let mut bench = Bench::new(&[Some(Sign::Secret)], BOUND, &[DECLINED]);
        let got = stop_of(bench.step(&step));
        let log = bench.log();
        assert_eq!(
            got,
            Some((Outcome::Declined, Some(Cause::Declined))),
            "{log:?}"
        );
        assert_eq!(log.first().map(String::as_str), Some("read"), "{log:?}");
        assert_eq!(count(&log, "frame handback sign_in"), 1, "{log:?}");
        assert!(
            !log.iter().any(|l| l.starts_with("act ")),
            "a command to the control: {log:?}"
        );
    }

    let mut bench = Bench::with(&[None, Some(Sign::Secret)], BOUND, &[DECLINED], true, &[]);
    let got = stop_of(bench.step(&fill()));
    let log = bench.log();
    assert_eq!(
        got,
        Some((Outcome::Declined, Some(Cause::Declined))),
        "{log:?}"
    );
    assert_eq!(&log[..3], ["read", "act fill", "read"], "{log:?}");
    assert_eq!(count(&log, "frame handback sign_in"), 1, "{log:?}");
    assert_eq!(count(&log, "act fill"), 1, "{log:?}");
}

/// The coordinator's ruling of log line 13906 (1): the request of a sign-in handback holds the origin of the top
/// document, which the runner reads once before the opening, its scheme, host and port alone, and no byte of the
/// page's path or query; a top document with no origin of http or https gives null.
#[test]
fn a_sign_in_request_holds_the_page_s_origin_and_no_path() {
    let address = "http://127.0.0.1:8080/sign-in/halvo-path-marker?halvo-query-marker=1#halvo-fragment-marker";
    let mut bench = Bench::new(&[Some(Sign::Secret)], address, &[DECLINED]);
    let _ = bench.step(&handback());
    let requests = bench.requests();
    assert_eq!(requests.len(), 1, "{:?}", bench.log());
    assert!(
        requests[0].contains(r#""origin":"http://127.0.0.1:8080""#),
        "{}",
        requests[0]
    );
    for part in [
        "sign-in",
        "halvo-path-marker",
        "halvo-query-marker",
        "halvo-fragment-marker",
    ] {
        assert!(!requests[0].contains(part), "{part} in {}", requests[0]);
    }
    let log = bench.log();
    let opening = at(&log, "fail held");
    assert_eq!(count(&log[..opening], "address"), 1, "{log:?}");

    let mut bench = Bench::new(&[Some(Sign::Secret)], "about:blank", &[DECLINED]);
    let _ = bench.step(&handback());
    let requests = bench.requests();
    assert_eq!(requests.len(), 1, "{:?}", bench.log());
    assert!(requests[0].contains(r#""origin":null"#), "{}", requests[0]);
}

/// A6.4: the reason follows the field that the reading found, `captcha` for a challenge, `second_factor` for a
/// one-time code, else `sign_in`, also for the handback that a one-time code asks for after the `done` of a sign-in;
/// a `handback` step on a page with no credential field, or before any browser runs, opens none and is done.
#[test]
fn the_reason_follows_the_field() {
    for (sign, reason) in [
        (Sign::Challenge, "captcha"),
        (Sign::Code, "second_factor"),
        (Sign::Secret, "sign_in"),
    ] {
        let mut bench = Bench::new(&[Some(sign)], BOUND, &[DECLINED]);
        let got = stop_of(bench.step(&handback()));
        let log = bench.log();
        assert_eq!(
            got,
            Some((Outcome::Declined, Some(Cause::Declined))),
            "{log:?}"
        );
        let handbacks: Vec<&String> = log
            .iter()
            .filter(|l| l.starts_with("frame handback"))
            .collect();
        assert_eq!(handbacks, [&format!("frame handback {reason}")], "{log:?}");
    }

    let signs = [Some(Sign::Secret), Some(Sign::Code), None];
    let mut bench = Bench::new(&signs, BOUND, &[DONE, DONE, BOUND_YES]);
    assert!(bench.step(&handback()).is_ok(), "{:?}", bench.log());
    let log = bench.log();
    let handbacks: Vec<&String> = log
        .iter()
        .filter(|l| l.starts_with("frame handback"))
        .collect();
    assert_eq!(
        handbacks,
        ["frame handback sign_in", "frame handback second_factor"],
        "{log:?}"
    );

    let mut bench = Bench::new(&[None], BOUND, &[]);
    assert!(bench.step(&handback()).is_ok());
    assert_eq!(bench.log(), ["read"]);

    let mut bench = Bench::new(&[], BOUND, &[]);
    bench.runner.web = None;
    assert!(bench.step(&handback()).is_ok());
    assert!(bench.log().is_empty(), "{:?}", bench.log());
}

/// A8.4 `acts_during_handback`, A6.4: before the handback request the runner reads the top document's address once
/// for the request's origin, fails every paused request, closes every other page, clears what each loaded origin
/// keeps but its cookies and loads the address again by GET, in that order, then arms the handback's mode; between the request and its result it sends the browser no command and
/// reads nothing, and it arms no other rule until the `bind_reply`.
#[test]
fn refusal_acts_during_handback() {
    let mut bench = Bench::new(&[Some(Sign::Secret), None], BOUND, &[DONE, BOUND_YES]);
    assert!(bench.step(&handback()).is_ok(), "{:?}", bench.log());
    let log = bench.log();
    let request = at(&log, "frame handback sign_in");
    let before: Vec<&str> = log[..request]
        .iter()
        .filter(|l| !l.starts_with("frame store"))
        .map(String::as_str)
        .collect();
    assert_eq!(
        before,
        [
            "read",
            "address",
            "fail held",
            "close others",
            "clear stores",
            "load again",
            "pass all"
        ],
        "{log:?}"
    );
    let result = at(&log, "result done");
    assert_eq!(
        result,
        request + 1,
        "a command or a frame between the request and the result: {log:?}"
    );
    let passed = at(&log, "pass all");
    let replied = at(&log, "bind_reply yes");
    assert!(
        !log[passed..replied].iter().any(|l| l.starts_with("arm")),
        "another rule while the handback is open: {log:?}"
    );
}

/// A6.4: after a `done` that leaves no credential field the runner reads once the address of the page on which the
/// handback opened and sends `bind` with its origin; after `bound=yes` it first arms `Mode::Fail` with the origins
/// of the bindings and the bound one, then closes the pages opened meanwhile (the coordinator's ruling of log line
/// 14127 (4)), and goes on to the next step, which acts, and the run completes.
#[test]
fn a_sign_in_ends_in_a_bind_and_the_run_goes_on() {
    let address = "https://accounts.example.com/done?x=1";
    let mut bench = Bench::new(
        &[Some(Sign::Secret), None, None],
        address,
        &[DONE, BOUND_YES],
    );
    assert!(bench.step(&handback()).is_ok(), "{:?}", bench.log());
    assert!(bench.step(&click()).is_ok(), "{:?}", bench.log());
    let log = bench.end(Ok(Outcome::Completed));
    let result = at(&log, "result done");
    assert_eq!(
        &log[result + 1..],
        [
            "read",
            "address",
            "frame bind https://accounts.example.com:443",
            "bind_reply yes",
            "arm fail [http://127.0.0.1:8080 https://accounts.example.com:443]",
            "close others",
            "read",
            "act click",
            "stop",
            "frame store finish",
            "frame done completed",
        ],
        "{log:?}"
    );
    assert_eq!(count(&log[result + 1..], "address"), 1);
}

/// The coordinator's ruling of log line 14244: a field that the page shows only later opens no handback while it is
/// not shown and opens its handback at the web step that meets it shown: the first click reads no field and acts, the
/// second reads the field, hands back and acts on nothing.
#[test]
fn a_field_shown_later_opens_its_handback_at_the_step_that_meets_it() {
    let mut bench = Bench::new(&[None, Some(Sign::Secret)], BOUND, &[DECLINED]);
    assert!(bench.step(&click()).is_ok(), "{:?}", bench.log());
    let got = stop_of(bench.step(&click()));
    let log = bench.log();
    assert_eq!(
        got,
        Some((Outcome::Declined, Some(Cause::Declined))),
        "{log:?}"
    );
    assert_eq!(&log[..3], ["read", "act click", "read"], "{log:?}");
    assert_eq!(count(&log, "act click"), 1, "{log:?}");
    assert_eq!(count(&log, "frame handback sign_in"), 1, "{log:?}");
}

/// The coordinator's ruling of log line 14127 (3): every origin that was bound in this run stays in the rule after a
/// second bind, beside the origins of the bindings.
#[test]
fn every_origin_bound_in_the_run_stays_after_a_second_bind() {
    let (first, second) = (
        "https://accounts.example.com/done",
        "https://pay.example.com/done",
    );
    let mut bench = Bench::with(
        &[Some(Sign::Secret), None, Some(Sign::Code), None],
        BOUND,
        &[DONE, BOUND_YES, DONE, BOUND_YES],
        false,
        &[first, first, second, second],
    );
    assert!(bench.step(&handback()).is_ok(), "{:?}", bench.log());
    assert!(bench.step(&handback()).is_ok(), "{:?}", bench.log());
    let log = bench.log();
    let arms: Vec<&String> = log.iter().filter(|l| l.starts_with("arm fail")).collect();
    assert_eq!(
        arms,
        [
            "arm fail [http://127.0.0.1:8080 https://accounts.example.com:443]",
            "arm fail [http://127.0.0.1:8080 https://accounts.example.com:443 https://pay.example.com:443]",
        ],
        "{log:?}"
    );
}

/// A6.4: no bind and no further step at each other end of a handback: `declined` ends the run `declined`, `timeout`
/// ends it `handed_back` with the cause `timeout`, a credential field after the third `done` in a row and an
/// address that gives no origin end it `handed_back`; each time the runner lets no request through, fails what it
/// holds paused and closes the pages opened meanwhile, and sends no `bind`.
#[test]
fn each_end_without_a_bind_ends_the_run() {
    const FIELD: Option<Sign> = Some(Sign::Secret);
    let cases: [Case; 4] = [
        (
            &[FIELD],
            BOUND,
            &[DECLINED],
            (Outcome::Declined, Some(Cause::Declined)),
            1,
        ),
        (
            &[FIELD],
            BOUND,
            &[TIMEOUT],
            (Outcome::HandedBack, Some(Cause::Timeout)),
            1,
        ),
        (
            &[FIELD; 4],
            BOUND,
            &[DONE, DONE, DONE],
            (Outcome::HandedBack, None),
            3,
        ),
        (
            &[FIELD, None],
            "about:blank",
            &[DONE],
            (Outcome::HandedBack, None),
            1,
        ),
    ];
    for (signs, address, answers, want, handbacks) in cases {
        let mut bench = Bench::new(signs, address, answers);
        let got = stop_of(bench.step(&handback()));
        assert_eq!(got, Some(want), "{:?}", bench.log());
        let (outcome, cause) = want;
        let log = bench.end(Err(Stop::End(outcome, cause)));
        assert_eq!(
            log.iter()
                .filter(|l| l.starts_with("frame handback"))
                .count(),
            handbacks,
            "{log:?}"
        );
        assert!(!log.iter().any(|l| l.starts_with("frame bind")), "{log:?}");
        let last = log
            .iter()
            .rposition(|l| l.starts_with("result"))
            .expect("a result");
        let shut = log[last + 1..]
            .iter()
            .position(|l| l == "arm fail []")
            .expect("the rule armed with no origin");
        let after: Vec<&str> = log[last + 1 + shut..]
            .iter()
            .filter(|l| !l.starts_with("frame store"))
            .map(String::as_str)
            .collect();
        let done = format!("frame done {}", outcome.as_str());
        assert_eq!(
            after,
            [
                "arm fail []",
                "fail held",
                "close others",
                "stop",
                done.as_str()
            ],
            "{log:?}"
        );
    }
}

/// The gate line of plan line 203 (A6.4, the interface run's RD-99 item 1): after `bind_reply` with `bound=no` the
/// runner binds nothing, makes no further step and no effect, lets no request through, fails every request that it
/// holds paused, closes the pages opened while the handback was open, sends no frame but those of its end, and ends
/// the run `declined`.
#[test]
fn after_bound_no_nothing_leaves() {
    let mut bench = Bench::new(&[Some(Sign::Secret), None], BOUND, &[DONE, BOUND_NO]);
    let got = stop_of(bench.step(&handback()));
    assert_eq!(
        got,
        Some((Outcome::Declined, Some(Cause::Declined))),
        "{:?}",
        bench.log()
    );
    let log = bench.end(Err(Stop::End(Outcome::Declined, Some(Cause::Declined))));
    let replied = at(&log, "bind_reply no");
    assert_eq!(
        &log[replied + 1..],
        [
            "arm fail []",
            "fail held",
            "close others",
            "stop",
            "frame store log",
            "frame store finish",
            "frame done declined",
        ],
        "{log:?}"
    );
}

/// r3 U5.14 (the coordinator's ruling of log line 14127 (1)): on a page that shows no credential field and asks for a
/// sign-in by a link alone, a web step that fails hands back with the reason `sign_in` in place of its failure, as at a
/// credential field. A click whose act fails reads the page for a field, then reads it a second time, then opens the
/// handback in the order of a credential field's, and the request names the page's origin; so does a fill of a target
/// marked `credential`, which fails where the page shows no credential field. Declined, the run ends `declined`. A
/// click that does not fail on such a page acts, and makes no second reading. Behind the person's `done` and the bind
/// the click's act is asked once more (the coordinator's ruling of log line 16106 (3)): end (i), it acts and the step
/// is done; end (ii), it fails again and the step fails as before, with one handback at that step.
#[test]
fn a_failed_step_on_a_page_that_asks_by_a_link_alone_hands_back() {
    let page = |link: &'static str| vec![("button", "Pay bill"), ("link", link)];
    let mut bench = Bench::on_page(&[], BOUND, &[], 0, &[], &page("Sign in"));
    assert!(bench.step(&click()).is_ok(), "{:?}", bench.log());
    assert_eq!(bench.log(), ["read", "act click"]);

    let opening = [
        "address",
        "fail held",
        "close others",
        "clear stores",
        "load again",
        "pass all",
    ];
    let cases = [
        (click(), "Sign in"),
        (click(), "Log in"),
        (click(), "Sign-in with Google"),
        (click(), "Customer LOGIN"),
        (credential_fill(), "Sign in"),
    ];
    for (step, link) in cases {
        let mut bench = Bench::on_page(&[], BOUND, &[DECLINED], usize::MAX, &[], &page(link));
        let got = stop_of(bench.step(&step));
        let log = bench.log();
        assert_eq!(
            got,
            Some((Outcome::Declined, Some(Cause::Declined))),
            "{link}: {log:?}"
        );
        let mut want = if step.verb == Verb::Click {
            vec!["read", "act click", "read", "read sign-in"]
        } else {
            vec!["read", "read sign-in"]
        };
        want.extend(opening);
        let request = at(&log, "frame handback sign_in");
        let before: Vec<&str> = log[..request]
            .iter()
            .filter(|l| !l.starts_with("frame store"))
            .map(String::as_str)
            .collect();
        assert_eq!(before, want, "{link}: {log:?}");
        assert_eq!(count(&log, "frame handback sign_in"), 1, "{link}: {log:?}");
        let requests = bench.requests();
        assert!(
            requests[0].contains(r#""origin":"http://127.0.0.1:8080""#),
            "{link}: {}",
            requests[0]
        );
    }

    // The coordinator's ruling of log line 16106 (3): behind the person's `done` and the bind, the act that failed is
    // asked once more, after the reading of the fields before an act. End (i): the act acts, and the step goes on with
    // what it gives. End (ii): the act fails again, and the step fails as before this unit: the reading of the fields
    // after a failed act, no second reading, and no second handback at that step.
    for again in [false, true] {
        let fails = if again { 2 } else { 1 };
        let answers = [DONE, BOUND_YES];
        let mut bench = Bench::on_page(&[], BOUND, &answers, fails, &[], &page("Sign in"));
        let got = bench.step(&click());
        let log = bench.log();
        if again {
            assert!(
                matches!(got, Err(Failure::Step(Cause::StepFailed))),
                "{log:?}"
            );
        } else {
            assert!(got.is_ok(), "{log:?}");
        }
        let replied = at(&log, "bind_reply yes");
        let after: Vec<&str> = log[replied + 1..]
            .iter()
            .filter(|l| !l.starts_with("frame store"))
            .map(String::as_str)
            .collect();
        let mut want = vec![
            "arm fail [http://127.0.0.1:8080]",
            "close others",
            "read",
            "act click",
        ];
        if again {
            want.push("read");
        }
        assert_eq!(after, want, "{log:?}");
        assert_eq!(count(&log, "act click"), 2, "{log:?}");
        assert_eq!(count(&log, "read sign-in"), 1, "{log:?}");
        assert_eq!(count(&log, "frame handback sign_in"), 1, "{log:?}");
    }
}

/// r3 U5.14: on a page that shows no credential field and no link or button that asks for a sign-in by its name, a web
/// step that fails fails as before, with `step_failed` and no handback, after the second reading. The pages: a link
/// that names a password alone; a link that names a login and a password; buttons and a link that ask for a new
/// account; a link to sign out; the words of a sign-in on a heading and in the name of a field; and a page with none.
/// Each with a click whose act fails and with a fill of a target marked `credential`.
#[test]
fn a_failed_step_on_a_page_with_no_such_sign_fails_as_before() {
    let pages: [&[(&'static str, &'static str)]; 6] = [
        &[("link", "Change password")],
        &[("link", "Forgot your login password?")],
        &[
            ("button", "Sign up"),
            ("link", "Register"),
            ("button", "Create account"),
        ],
        &[("link", "Sign out")],
        &[("heading", "Log in"), ("textbox", "Login")],
        &[],
    ];
    for page in pages {
        for step in [click(), credential_fill()] {
            let mut bench = Bench::on_page(&[], BOUND, &[DECLINED], usize::MAX, &[], page);
            let got = bench.step(&step);
            let log = bench.log();
            assert!(
                matches!(got, Err(Failure::Step(Cause::StepFailed))),
                "{page:?}: {log:?}"
            );
            let want = if step.verb == Verb::Click {
                &["read", "act click", "read", "read sign-in"][..]
            } else {
                &["read", "read sign-in"][..]
            };
            assert_eq!(log, want, "{page:?}");
        }
    }
}
