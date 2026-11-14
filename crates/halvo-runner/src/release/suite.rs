//! The suite of the port [`Paused`] (the coordinator's ruling CP-142): the cases `commit_without_reply` and
//! `changed_on_page` of A5.4, written for any request maker. Each case starts the fixture server of A10 with its
//! variant, takes one paused request from the maker, runs the runner's confirm path on it in this process against
//! an in-memory supervisor and store, and checks what the fixture recorded and which rows and frames the runner
//! wrote. A maker of this crate calls [`commit_without_reply`] and [`changed_on_page`] from its own tests with its
//! own [`Maker`].

use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::AtomicU64;
use std::sync::{mpsc, Arc};
use std::time::Duration;

use halvo_policy::{effect_step, preset, Effect, EffectMove};
use halvo_schema::executable::body::{LoadReply, LoadedBindings, Stored};
use halvo_schema::executable::frame::{
    Approval, Cancel, ConfirmDecision, ConfirmReply, ConfirmRequest, HandbackReply, StoreReply,
    StoreReplyFrame,
};
use halvo_schema::executable::names::{
    AllowedClass, Answer, Autonomy, FieldName, HandbackOutcome, HandbackReason, LogEvent, YesNo,
};
use halvo_schema::executable::record::Slot;
use halvo_schema::executable::text::{Name, OwnValue, V1};
use halvo_schema::executable::{
    AgentConfig, AgentEffect, AgentLog, AgentRun, ApprovalId, Cause, Digest, EffectId, EffectState,
    FromRunner, HandbackResult, Outcome, RunId, RunnerCall, RunnerReply, ToRunner, WebOrigin,
};
use halvo_schema::{AgentId, Category, WorkflowId};

use super::Paused;
use crate::link::{encode, Link};
use crate::run::Runner;

/// The case of A5.4 for which a maker pauses a request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Case {
    /// `commit_without_reply`: the paused request is the first state-changing request at the fixture after its
    /// reset with the variant `no_reply`, which holds it with no reply.
    NoReply,
    /// `changed_on_page`: a `payment` of the bill whose amount the payment frame shows, under the variant
    /// `amount_changes`.
    AmountChanges,
    /// A `delete` whose request carries a token that the service refuses, so that it is answered 401.
    AnotherToken,
    /// The `payment` of [`Case::AmountChanges`] with an instrument that the service does not offer, both in the
    /// bindings and in the request, so that it is answered 400.
    NotOffered,
}

/// A request maker as the suite drives it.
pub(crate) trait Maker {
    /// The category whose preset is the run's configuration.
    fn category(&self, case: Case) -> Category;
    /// The origins and the slots of the bindings of a run that makes the request of `case`.
    fn bindings(&self, fixture: &Fixture, case: Case) -> (Vec<WebOrigin>, Vec<Slot>);
    /// A state-changing request of `case` against `fixture`, paused.
    fn paused(&mut self, fixture: &Fixture, case: Case) -> Box<dyn Paused>;
}

/// An answer of the fixture server: the status and the body.
#[derive(Debug)]
pub(crate) struct Answered {
    pub(crate) status: u16,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: String,
}

/// The bytes of one HTTP/1.1 request to `host` of the fixture on `port`, which closes its connection.
pub(crate) fn request_bytes(
    port: u16,
    method: &str,
    host: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> Vec<u8> {
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    let mut bytes = head.into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

/// Opens a connection to the fixture on `port` and writes `bytes`.
pub(crate) fn send_bytes(port: u16, bytes: &[u8]) -> TcpStream {
    let mut stream = TcpStream::connect(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
        .expect("connect to the fixture");
    stream.write_all(bytes).expect("write the request");
    stream
}

/// The answer on `stream` within `wait`, or none when no answer came.
pub(crate) fn answer_of(mut stream: TcpStream, wait: Duration) -> Option<Answered> {
    stream.set_read_timeout(Some(wait)).ok()?;
    let mut bytes = Vec::new();
    let _ = stream.read_to_end(&mut bytes);
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let (head, body) = text.split_once("\r\n\r\n")?;
    let mut lines = head.lines();
    let status = lines.next()?.split(' ').nth(1)?.parse().ok()?;
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(n, v)| (n.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    let chunked = headers
        .iter()
        .any(|(n, v)| n == "transfer-encoding" && v.eq_ignore_ascii_case("chunked"));
    let body = if chunked {
        dechunk(body)?
    } else {
        body.to_string()
    };
    Some(Answered {
        status,
        headers,
        body,
    })
}

/// The body of a chunked answer.
fn dechunk(mut rest: &str) -> Option<String> {
    let mut body = String::new();
    loop {
        let (size, after) = rest.split_once("\r\n")?;
        let size = usize::from_str_radix(size.split(';').next()?.trim(), 16).ok()?;
        if size == 0 {
            return Some(body);
        }
        body.push_str(after.get(..size)?);
        rest = after.get(size + 2..)?;
    }
}

/// One request to the fixture and its answer within ten seconds.
pub(crate) fn http(
    port: u16,
    method: &str,
    host: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> Answered {
    let stream = send_bytes(
        port,
        &request_bytes(port, method, host, path, headers, body),
    );
    answer_of(stream, Duration::from_secs(10)).expect("the fixture answers")
}

/// The fixture server of A10, started with a pipe on its standard input, so that it ends with this process, and
/// ended when dropped.
pub(crate) struct Fixture {
    child: Child,
    port: u16,
    seed: serde_json::Value,
}

fn variants_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/fixture-server/variants")
}

impl Fixture {
    /// Starts the server on a free port with the seed 1.
    pub(crate) fn start() -> Fixture {
        let dir = variants_dir();
        let mut child = Command::new("node")
            .arg(dir.join("server.mjs"))
            .args(["--port", "0", "--seed", "1"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("node starts the fixture server");
        let stdout = child.stdout.take().expect("its standard output");
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            let _ = BufReader::new(stdout).read_line(&mut line);
            let _ = tx.send(line);
        });
        let line = rx.recv_timeout(Duration::from_secs(60)).unwrap_or_default();
        let port = line
            .trim_end()
            .strip_prefix("listening 127.0.0.1:")
            .and_then(|p| p.parse::<u16>().ok());
        let seed = std::fs::read(dir.join("data").join("seed-1.json")).expect("read the seed");
        let mut fixture = Fixture {
            child,
            port: 0,
            seed: serde_json::from_slice(&seed).expect("the seed's json"),
        };
        fixture.port = port.expect("the fixture prints listening 127.0.0.1:<port>");
        fixture
    }

    pub(crate) fn port(&self) -> u16 {
        self.port
    }

    /// The seed's data.
    pub(crate) fn seed(&self) -> &serde_json::Value {
        &self.seed
    }

    /// The origin of the fixture's host `name`.
    pub(crate) fn origin(&self, name: &str) -> WebOrigin {
        WebOrigin::parse(&format!("http://{name}.agent.test:{}", self.port)).expect("an origin")
    }

    /// One request to the control host.
    pub(crate) fn control(&self, method: &str, path: &str) -> Answered {
        http(self.port, method, "control.agent.test", path, &[], b"")
    }

    /// Sets the seed 1 again with exactly `variants`.
    pub(crate) fn reset(&self, variants: &[&str]) {
        let mut path = "/reset?seed=1".to_string();
        for v in variants {
            path.push_str(&format!("&variant={v}"));
        }
        assert_eq!(self.control("POST", &path).status, 200, "the reset");
    }

    /// The state object of A10.6.
    pub(crate) fn state(&self) -> serde_json::Value {
        serde_json::from_str(&self.control("GET", "/state").body).expect("the state object")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// What the in-memory supervisor saw and its store holds at the run's end.
pub(crate) struct Record {
    pub(crate) frames: Vec<FromRunner>,
    /// The state of each effect after the store's handler of the run's end, which makes a `releasing` effect
    /// `unknown` (A7.5).
    pub(crate) after_end: HashMap<EffectId, EffectState>,
    pub(crate) status: i32,
}

impl Record {
    pub(crate) fn effects(&self) -> Vec<AgentEffect> {
        self.frames
            .iter()
            .filter_map(|f| match f {
                FromRunner::Store(s) => match s.request.body() {
                    RunnerCall::Effect(b) => Some(b.record.clone()),
                    _ => None,
                },
                _ => None,
            })
            .collect()
    }

    pub(crate) fn states(&self) -> Vec<EffectState> {
        self.effects().iter().map(|e| e.state).collect()
    }

    pub(crate) fn confirms(&self) -> Vec<ConfirmRequest> {
        self.frames
            .iter()
            .filter_map(|f| match f {
                FromRunner::Confirm(c) => Some(c.request.clone()),
                _ => None,
            })
            .collect()
    }

    pub(crate) fn handbacks(&self) -> Vec<HandbackReason> {
        self.frames
            .iter()
            .filter_map(|f| match f {
                FromRunner::Handback(h) => Some(h.request.reason),
                _ => None,
            })
            .collect()
    }

    pub(crate) fn done(&self) -> Option<Outcome> {
        self.frames.iter().find_map(|f| match f {
            FromRunner::Done(d) => Some(d.outcome),
            _ => None,
        })
    }

    pub(crate) fn logs(&self) -> Vec<AgentLog> {
        self.frames
            .iter()
            .filter_map(|f| match f {
                FromRunner::Store(s) => match s.request.body() {
                    RunnerCall::Log(b) => Some(b.records.clone()),
                    _ => None,
                },
                _ => None,
            })
            .flatten()
            .collect()
    }

    pub(crate) fn finish(&self) -> Option<AgentRun> {
        self.frames.iter().find_map(|f| match f {
            FromRunner::Store(s) => match s.request.body() {
                RunnerCall::Finish(b) => Some(b.record.clone()),
                _ => None,
            },
            _ => None,
        })
    }
}

/// The data directory that the suite's runner is given: a path that it never makes, since the confirm path does
/// no file step.
fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("port-suite-data-dir-never-made")
}

/// A change of the slots that the load of a count gives.
pub(crate) type SlotsAt = Box<dyn FnMut(usize, &mut Vec<Slot>) + Send>;

/// The frames before whose reply the supervisor writes one `cancel`.
pub(crate) type CancelBefore = Box<dyn FnMut(&FromRunner) -> bool + Send>;

/// A change of the origins of the bindings that the load of a count gives.
pub(crate) type OriginsAt = Box<dyn FnMut(usize, &mut Vec<WebOrigin>) + Send>;

/// What the in-memory supervisor does beside answering: `on_confirm` runs with the count of confirmation requests so
/// far, before the decision is written; `answer` is the answer for every effect of a decision, unless `answer_for`
/// gives one for the card of a count and the effect of an index in it (CP-166 E6); with
/// `stop_after_begin` it closes both ends of the link once it has answered a `begin` row; with
/// `cancel_before_begin_answer` it writes a `cancel` before the answer to a `begin` row; and from the load of the
/// count `observe_from_load` on, the configuration's autonomy is `observe`, and from the load of the count
/// `wall_zero_from_load` on its `wall_s` is 0; with `one_approval_id` every approval has the same id; with
/// `other_digest` every approval names a digest that is not its effect's; and the loads for whose count `runs_at`
/// holds give a configuration under which `decide` gives `run` for `delete` and `submit` (`unattended`, `allow`
/// `delete` and `submit`, `untrusted` no), and `allow_at` gives the classes that a load allows, by its count;
/// `slots_at` and `origins_at` change the slots and the origins of the
/// bindings that the load of a count gives; the supervisor writes one `cancel` before its reply to the first frame
/// for which `cancel_before` holds; `wall_s` is the `wall_s` of every load; and `clock` is the run's clock of the test's own, in milliseconds added to its time.
pub(crate) struct Plan {
    pub(crate) on_confirm: Box<dyn FnMut(usize) + Send>,
    pub(crate) answer: Answer,
    pub(crate) answer_for: Option<fn(usize, usize) -> Answer>,
    pub(crate) stop_after_begin: bool,
    pub(crate) cancel_before_begin_answer: bool,
    pub(crate) observe_from_load: Option<usize>,
    pub(crate) wall_zero_from_load: Option<usize>,
    pub(crate) one_approval_id: bool,
    pub(crate) other_digest: bool,
    pub(crate) runs_at: Option<fn(usize) -> bool>,
    pub(crate) allow_at: Option<fn(usize) -> Vec<AllowedClass>>,
    pub(crate) slots_at: Option<SlotsAt>,
    pub(crate) origins_at: Option<OriginsAt>,
    pub(crate) cancel_before: Option<CancelBefore>,
    pub(crate) wall_s: Option<u32>,
    pub(crate) clock: Arc<AtomicU64>,
}

impl Plan {
    /// A plan that answers `allow_once` and does nothing else.
    pub(crate) fn plain() -> Plan {
        Plan {
            on_confirm: Box::new(|_| {}),
            answer: Answer::AllowOnce,
            answer_for: None,
            stop_after_begin: false,
            cancel_before_begin_answer: false,
            observe_from_load: None,
            wall_zero_from_load: None,
            one_approval_id: false,
            other_digest: false,
            runs_at: None,
            allow_at: None,
            slots_at: None,
            origins_at: None,
            cancel_before: None,
            wall_s: None,
            clock: Arc::default(),
        }
    }
}

/// The in-memory supervisor and store: it reads the runner's frames until the end of the runner's output and
/// answers each as `plan` says. The store's handler of the run's end makes a `releasing` effect `unknown` at the
/// run's `finish`, and at the end of the link when no `finish` came, as after the library's `end_run` (A7.5).
fn supervise(
    mut from_runner: impl Read,
    mut to_runner: impl Write,
    config: AgentConfig,
    bindings: (Vec<WebOrigin>, Vec<Slot>),
    mut plan: Plan,
) -> (Vec<FromRunner>, HashMap<EffectId, EffectState>) {
    let mut frames = Vec::new();
    let mut last: HashMap<EffectId, AgentEffect> = HashMap::new();
    let mut after_end = HashMap::new();
    let mut confirms = 0usize;
    let mut loads = 0usize;
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
        let reply = match &frame {
            FromRunner::Store(s) => {
                let answer = match s.request.body() {
                    RunnerCall::Load(_) => RunnerReply::Load(LoadReply {
                        config: {
                            loads += 1;
                            let mut config = config.clone();
                            if plan.observe_from_load.is_some_and(|from| loads >= from) {
                                config.autonomy = Autonomy::Observe;
                            }
                            if let Some(wall_s) = plan.wall_s {
                                config.limits.wall_s = wall_s;
                            }
                            if plan.wall_zero_from_load.is_some_and(|from| loads >= from) {
                                config.limits.wall_s = 0;
                            }
                            if plan.runs_at.is_some_and(|runs| runs(loads)) {
                                config.autonomy = Autonomy::Unattended;
                                config.allow = vec![AllowedClass::Delete, AllowedClass::Submit];
                                config.caps.untrusted = YesNo::No;
                            }
                            if let Some(allow) = plan.allow_at {
                                config.autonomy = Autonomy::Unattended;
                                config.allow = allow(loads);
                                config.caps.untrusted = YesNo::No;
                            }
                            config
                        },
                        playbook: None,
                        bindings: Some(LoadedBindings {
                            rev: NonZeroU32::MIN,
                            origins: {
                                let mut origins = bindings.0.clone();
                                if let Some(change) = plan.origins_at.as_mut() {
                                    change(loads, &mut origins);
                                }
                                origins
                            },
                            slots: {
                                let mut slots = bindings.1.clone();
                                if let Some(change) = plan.slots_at.as_mut() {
                                    change(loads, &mut slots);
                                }
                                slots
                            },
                        }),
                        sessions: Vec::new(),
                        start: None,
                    }),
                    RunnerCall::Effect(b) => {
                        last.insert(b.record.effect.clone(), b.record.clone());
                        RunnerReply::Stored(Stored { stored: 1 })
                    }
                    RunnerCall::Log(b) => RunnerReply::Stored(Stored {
                        stored: b.records.len() as u32,
                    }),
                    RunnerCall::Finish(_) => {
                        for (id, row) in &last {
                            let now = Effect {
                                id: id.clone(),
                                state: row.state,
                                class: row.class,
                                digest: row.digest.clone(),
                                rebinds: 0,
                            };
                            let state =
                                effect_step(Some(&now), EffectMove::Lose).unwrap_or(row.state);
                            after_end.insert(id.clone(), state);
                        }
                        RunnerReply::Stored(Stored { stored: 1 })
                    }
                    _ => RunnerReply::Stored(Stored { stored: 1 }),
                };
                Some(ToRunner::StoreReply(StoreReplyFrame {
                    reply: StoreReply {
                        id: Some(s.request.id().clone()),
                        outcome: Ok(answer),
                    },
                }))
            }
            FromRunner::Confirm(c) => {
                confirms += 1;
                let approvals = c
                    .request
                    .effects
                    .iter()
                    .enumerate()
                    .map(|(i, e)| Approval {
                        approval: if plan.one_approval_id {
                            ApprovalId::from_random([0x77; 16])
                        } else {
                            ApprovalId::from_random([(confirms * 16 + i) as u8; 16])
                        },
                        effect: e.effect.clone(),
                        digest: if plan.other_digest {
                            Digest::parse(&"0".repeat(64)).expect("a digest")
                        } else {
                            e.digest.clone()
                        },
                        answer: plan.answer_for.map_or(plan.answer, |of| of(confirms, i)),
                    })
                    .collect();
                (plan.on_confirm)(confirms);
                Some(ToRunner::ConfirmReply(ConfirmReply {
                    decision: ConfirmDecision { v: V1, approvals },
                }))
            }
            FromRunner::Handback(_) => Some(ToRunner::HandbackReply(HandbackReply {
                result: HandbackResult {
                    v: V1,
                    outcome: HandbackOutcome::Declined,
                    secret: None,
                },
            })),
            _ => None,
        };
        let begun = matches!(&frame, FromRunner::Store(s)
            if matches!(s.request.body(), RunnerCall::Effect(b) if b.record.state == EffectState::Releasing));
        let cancel_now = plan
            .cancel_before
            .as_mut()
            .is_some_and(|before| before(&frame));
        if cancel_now {
            plan.cancel_before = None;
        }
        frames.push(frame);
        if (plan.cancel_before_begin_answer && begun) || cancel_now {
            let json = ToRunner::Cancel(Cancel {}).to_json().expect("a frame");
            if to_runner.write_all(&encode(&json)).is_err() {
                break;
            }
        }
        if let Some(reply) = reply {
            let json = reply.to_json().expect("a frame");
            if to_runner.write_all(&encode(&json)).is_err() {
                break;
            }
        }
        if plan.stop_after_begin && begun {
            break;
        }
    }
    for (id, row) in last {
        let now = Effect {
            id: id.clone(),
            state: row.state,
            class: row.class,
            digest: row.digest.clone(),
            rebinds: 0,
        };
        let state = effect_step(Some(&now), EffectMove::Lose).unwrap_or(row.state);
        after_end.entry(id).or_insert(state);
    }
    (frames, after_end)
}

/// Runs the runner's confirm path on `request` in this process, with the preset of `category` and `bindings`,
/// against the in-memory supervisor, and ends the run as the runner ends one.
pub(crate) fn run_path(
    category: Category,
    bindings: (Vec<WebOrigin>, Vec<Slot>),
    request: Box<dyn Paused>,
    plan: Plan,
) -> Record {
    run_many_path(category, bindings, vec![request], plan)
}

/// Runs the runner's confirm path on `requests`, the paused requests of one step, in one call of `release`, as
/// [`run_path`] runs one.
pub(crate) fn run_many_path(
    category: Category,
    bindings: (Vec<WebOrigin>, Vec<Slot>),
    requests: Vec<Box<dyn Paused>>,
    plan: Plan,
) -> Record {
    let run = RunId::from_random([0x51; 16]);
    let agent = AgentId::from_random([0x52; 16]);
    let workflow = WorkflowId::from_random([0x53; 16]);
    let config = preset(category, workflow.clone(), agent.clone()).expect("a preset");
    let data = data_dir().to_string_lossy().into_owned();
    let words = [
        "--run",
        run.as_str(),
        "--agent",
        agent.as_str(),
        "--workflow",
        workflow.as_str(),
        "--mode",
        "replay",
        "--browser",
        "none",
        "--data-dir",
        &data,
    ];
    let words: Vec<OsString> = words.iter().map(OsString::from).collect();
    let args = crate::args::parse(&words).expect("the arguments");
    let (to_runner_read, to_runner) = std::io::pipe().expect("a pipe");
    let (from_runner, from_runner_write) = std::io::pipe().expect("a pipe");
    let link = Link::start(
        Box::new(to_runner_read),
        Box::new(from_runner_write),
        Box::new(|| {}),
    );
    let clock = Arc::clone(&plan.clock);
    let supervisor =
        std::thread::spawn(move || supervise(from_runner, to_runner, config, bindings, plan));
    let mut runner = Runner::new(args, link);
    runner.test_clock = Arc::clone(&clock);
    let end = runner.release(requests).map(|()| Outcome::Completed);
    let status = runner.close(end);
    let (frames, after_end) = supervisor.join().expect("the supervisor");
    assert!(
        !data_dir().exists(),
        "the suite's runner writes nothing under its data directory"
    );
    Record {
        frames,
        after_end,
        status,
    }
}

/// The correction of CP-157 E1, case 4: every payment that the fixture recorded is to the payee of the card that
/// was allowed.
fn paid_to_the_allowed_payee(state: &serde_json::Value, allowed: &ConfirmRequest) {
    let payee = field(allowed, FieldName::Payee);
    for p in state["utility"]["payments"]
        .as_array()
        .expect("the payments")
    {
        assert_eq!(
            p["payee"].as_str(),
            Some(payee.as_str()),
            "paid to the payee of the allowed card"
        );
    }
}

/// The value of the kept field `name` of an effect row.
fn kept(row: &AgentEffect, name: FieldName) -> String {
    row.kept
        .iter()
        .find(|k| k.name == name)
        .map(|k| k.value.joined())
        .unwrap_or_default()
}

/// The value of the field `name` in the bound object of an effect of a confirmation request.
fn field(request: &ConfirmRequest, name: FieldName) -> String {
    request.effects[0]
        .bound
        .fields()
        .iter()
        .find(|f| f.name == name)
        .map(|f| f.value.joined())
        .expect("the field")
}

fn count(state: &serde_json::Value, key: &str) -> u64 {
    state["requests"][key].as_u64().expect("a count")
}

/// A5.4 `commit_without_reply`: on the fixture variant `no_reply` the state-changing request of an effect leaves
/// once and gets no reply; no `committed` row exists for the effect, the runner hands back `effect_unknown` before
/// its `finish` and ends `effect_unknown`, and after the run's end the effect is `unknown`.
pub(crate) fn commit_without_reply(maker: &mut dyn Maker) {
    let fixture = Fixture::start();
    fixture.reset(&["no_reply"]);
    let request = maker.paused(&fixture, Case::NoReply);
    let before = fixture.state();
    let record = run_path(
        maker.category(Case::NoReply),
        maker.bindings(&fixture, Case::NoReply),
        request,
        Plan::plain(),
    );
    let after = fixture.state();
    assert_eq!(
        count(&after, "state_changing") - count(&before, "state_changing"),
        1,
        "the effect's request left once"
    );
    let states = record.states();
    assert_eq!(
        states,
        [
            EffectState::Held,
            EffectState::Approved,
            EffectState::Releasing
        ],
        "a row before the request and none after it"
    );
    assert!(
        !states.contains(&EffectState::Committed),
        "no committed row"
    );
    assert_eq!(record.confirms().len(), 1, "one confirmation");
    assert_eq!(record.handbacks(), [HandbackReason::EffectUnknown]);
    assert_eq!(record.done(), Some(Outcome::EffectUnknown));
    assert_eq!(record.finish().map(|f| f.unknown), Some(1));
    assert_eq!(record.status, 0, "the finish is stored");
    let effect = record.effects()[0].effect.clone();
    assert_eq!(
        record.after_end.get(&effect),
        Some(&EffectState::Unknown),
        "the effect is unknown once the run has ended"
    );
}

/// A5.4 `changed_on_page`: on the fixture variant `amount_changes` the amount changes after the approval; the
/// request is failed before it leaves, the effect takes a `rebind`, and a new confirmation is asked with the new
/// amount; after its approval one request leaves, with the new amount, and its `begin` carries the digest that the
/// person was shown.
pub(crate) fn changed_on_page(maker: &mut dyn Maker) {
    let fixture = Fixture::start();
    fixture.reset(&["amount_changes"]);
    let request = maker.paused(&fixture, Case::AmountChanges);
    let before = fixture.state();
    let port = fixture.port();
    let record = run_path(
        maker.category(Case::AmountChanges),
        maker.bindings(&fixture, Case::AmountChanges),
        request,
        Plan {
            on_confirm: Box::new(move |n| {
                if n == 1 {
                    let changed = http(port, "POST", "control.agent.test", "/change", &[], b"");
                    assert_eq!(changed.status, 200, "the amount changes after the approval");
                }
            }),
            ..Plan::plain()
        },
    );
    let after = fixture.state();
    let due = fixture.seed()["utility"]["amount_due"]
        .as_u64()
        .expect("the amount due");
    let confirms = record.confirms();
    assert_eq!(confirms.len(), 2, "a new confirmation is asked");
    let money = |minor: u64| format!("{}.{:02}", minor / 100, minor % 100);
    assert_eq!(field(&confirms[0], FieldName::Amount), money(due));
    assert_eq!(
        field(&confirms[1], FieldName::Amount),
        money(due + 100),
        "the new confirmation shows the new amount"
    );
    assert_eq!(confirms[0].effects[0].effect, confirms[1].effects[0].effect);
    assert_eq!(
        record.states(),
        [
            EffectState::Held,
            EffectState::Approved,
            EffectState::Held,
            EffectState::Approved,
            EffectState::Releasing,
            EffectState::Committed
        ]
    );
    let effects = record.effects();
    assert_eq!(effects[2].approval, None, "the rebind voids the approval");
    assert_ne!(effects[1].approval, effects[3].approval, "a new approval");
    assert_eq!(effects[4].digest, confirms[1].effects[0].digest);
    assert_eq!(effects[4].approval, effects[3].approval);
    let kept_amounts: Vec<String> = effects.iter().map(|e| kept(e, FieldName::Amount)).collect();
    assert_eq!(
        kept_amounts,
        [
            money(due),
            money(due),
            money(due + 100),
            money(due + 100),
            money(due + 100),
            money(due + 100)
        ],
        "each row keeps the fields of the object whose digest it carries (A7.4, CP-157 E2)"
    );
    let paid: Vec<u64> = after["utility"]["payments"]
        .as_array()
        .expect("the payments")
        .iter()
        .map(|p| p["amount"].as_u64().expect("an amount"))
        .collect();
    assert_eq!(paid, [due + 100], "one payment, with the confirmed amount");
    paid_to_the_allowed_payee(&after, &confirms[1]);
    assert_eq!(
        count(&after, "state_changing") - count(&before, "state_changing"),
        1,
        "the request with the old amount never left"
    );
    assert_eq!(record.done(), Some(Outcome::Completed));
    assert_eq!(record.finish().map(|f| f.confirmations), Some(2));
}

/// A slot of the bindings.
pub(crate) fn slot(name: &str, value: &str) -> Slot {
    Slot {
        name: Name::parse(name).expect("a name"),
        value: OwnValue::new(value),
    }
}

/// The runner-level part of P4's gate line: a runner whose link closes after its `begin` was answered and before
/// its `commit` (the request of [`Case::NoReply`], with no variant) writes no `commit`, no handback and no
/// `finish`, ends with the status 1, lets the request leave at most once, and its effect is `unknown` after the
/// run's end that the library's `end_run` makes.
pub(crate) fn stopped_between_begin_and_commit(maker: &mut dyn Maker) {
    let fixture = Fixture::start();
    fixture.reset(&[]);
    let request = maker.paused(&fixture, Case::NoReply);
    let before = fixture.state();
    let record = run_path(
        maker.category(Case::NoReply),
        maker.bindings(&fixture, Case::NoReply),
        request,
        Plan {
            stop_after_begin: true,
            ..Plan::plain()
        },
    );
    let after = fixture.state();
    assert!(
        count(&after, "state_changing") - count(&before, "state_changing") <= 1,
        "the request left at most once"
    );
    let states = record.states();
    assert_eq!(states.last(), Some(&EffectState::Releasing));
    assert!(!states.contains(&EffectState::Committed), "no commit");
    assert!(
        record.handbacks().is_empty(),
        "a runner that is gone hands nothing back"
    );
    assert!(record.finish().is_none() && record.done().is_none());
    assert_eq!(record.status, 1, "the status of a closed link");
    let effect = record.effects()[0].effect.clone();
    assert_eq!(
        record.after_end.get(&effect),
        Some(&EffectState::Unknown),
        "the effect is unknown after the run's end"
    );
}

/// The ruling CP-157 E1 (A5.1, A5.2): the slot `instrument` of a `payment` names another instrument at the load
/// before the `begin` than at the load before the `propose`. The object built again from that load has another
/// digest, so the effect takes a `rebind` and the person is asked again with the new instrument; the request that
/// leaves is the renewed one, and the fixture holds one payment, with the new instrument and none with the old.
pub(crate) fn slot_changed_at_the_load_before_the_begin(maker: &mut dyn Maker) {
    let fixture = Fixture::start();
    fixture.reset(&["amount_changes"]);
    let request = maker.paused(&fixture, Case::AmountChanges);
    let instruments = &fixture.seed()["utility"]["instruments"];
    let first = instruments[0].as_str().expect("an instrument").to_string();
    let second = instruments[1].as_str().expect("an instrument").to_string();
    let newer = second.clone();
    let record = run_path(
        maker.category(Case::AmountChanges),
        maker.bindings(&fixture, Case::AmountChanges),
        request,
        Plan {
            slots_at: Some(Box::new(move |load, slots| {
                if load >= 2 {
                    for s in slots.iter_mut() {
                        if s.name.as_str() == "instrument" {
                            *s = slot("instrument", &newer);
                        }
                    }
                }
            })),
            ..Plan::plain()
        },
    );
    let after = fixture.state();
    let confirms = record.confirms();
    assert_eq!(confirms.len(), 2, "the person is asked again");
    assert_eq!(field(&confirms[0], FieldName::Instrument), first);
    assert_eq!(
        field(&confirms[1], FieldName::Instrument),
        second,
        "the second confirmation shows the new instrument"
    );
    assert_eq!(
        record.states(),
        [
            EffectState::Held,
            EffectState::Approved,
            EffectState::Held,
            EffectState::Approved,
            EffectState::Releasing,
            EffectState::Committed
        ]
    );
    let paid: Vec<String> = after["utility"]["payments"]
        .as_array()
        .expect("the payments")
        .iter()
        .map(|p| p["instrument"].as_str().expect("an instrument").to_string())
        .collect();
    assert_eq!(paid, [second], "one payment, with the new instrument");
    paid_to_the_allowed_payee(&after, &confirms[1]);
    assert_eq!(record.done(), Some(Outcome::Completed));
}

/// The correction of CP-157 E1, case 2: the slot `payee` names another payee at the load before the `begin`. The
/// object built again has another digest, and the payment's request does not carry the payee, so the maker's
/// `renew` answers `changed`: no `rebind` row and no second card; the effect is ended, nothing leaves, and the run
/// ends `failed` with `changed`.
pub(crate) fn payee_changed_at_the_load_before_the_begin(maker: &mut dyn Maker) {
    let fixture = Fixture::start();
    fixture.reset(&["amount_changes"]);
    let request = maker.paused(&fixture, Case::AmountChanges);
    let before = fixture.state();
    let record = run_path(
        maker.category(Case::AmountChanges),
        maker.bindings(&fixture, Case::AmountChanges),
        request,
        Plan {
            slots_at: Some(Box::new(|load, slots| {
                if load >= 2 {
                    for s in slots.iter_mut() {
                        if s.name.as_str() == "payee" {
                            *s = slot("payee", "Another Fixture Payee");
                        }
                    }
                }
            })),
            ..Plan::plain()
        },
    );
    let after = fixture.state();
    assert_eq!(record.confirms().len(), 1, "one card in all");
    assert_eq!(
        record.states(),
        [
            EffectState::Held,
            EffectState::Approved,
            EffectState::Rejected
        ],
        "no rebind row; the effect is ended"
    );
    assert_eq!(record.effects()[2].cause, Some(Cause::Ended));
    assert!(after["utility"]["payments"]
        .as_array()
        .expect("payments")
        .is_empty());
    assert_eq!(
        count(&after, "state_changing"),
        count(&before, "state_changing"),
        "no state-changing request"
    );
    assert_eq!(record.done(), Some(Outcome::Failed));
    assert_eq!(
        record.logs().last().map(|l| (l.event, l.cause)),
        Some((LogEvent::Stopped, Some(Cause::Changed)))
    );
}

/// Runs the payment of [`Case::AmountChanges`] with the origins of the bindings changed by `change` from the load
/// before the `begin` on; the record and the fixture's state before and after it.
fn pay_with_origins(
    maker: &mut dyn Maker,
    mut change: impl FnMut(&OriginsOf, &mut Vec<WebOrigin>) + Send + 'static,
) -> (Record, serde_json::Value, serde_json::Value) {
    let fixture = Fixture::start();
    fixture.reset(&["amount_changes"]);
    let request = maker.paused(&fixture, Case::AmountChanges);
    let before = fixture.state();
    let of = OriginsOf {
        port: fixture.port(),
    };
    let record = run_path(
        maker.category(Case::AmountChanges),
        maker.bindings(&fixture, Case::AmountChanges),
        request,
        Plan {
            origins_at: Some(Box::new(move |load, origins| {
                if load >= 2 {
                    change(&of, origins);
                }
            })),
            ..Plan::plain()
        },
    );
    let after = fixture.state();
    (record, before, after)
}

/// The origin of a host of the fixture on its port.
pub(crate) struct OriginsOf {
    port: u16,
}

impl OriginsOf {
    pub(crate) fn origin(&self, name: &str) -> WebOrigin {
        WebOrigin::parse(&format!("http://{name}.agent.test:{}", self.port)).expect("an origin")
    }
}

/// The ruling CP-159 G1 (A9.2, A9.3), adversary 2's origin case: the origin of the paused request, the payment
/// frame's, is no longer in the bindings of the load before the `begin`, and the digest is the same. No `begin` row
/// and no payment; the effect is ended and the run ends `refused` with the cause `origin`.
pub(crate) fn pay_origin_gone_at_the_load_before_the_begin(maker: &mut dyn Maker) {
    let (record, before, after) = pay_with_origins(maker, |f, origins| {
        let pay = f.origin("pay");
        origins.retain(|o| *o != pay);
    });
    assert_eq!(
        record.states(),
        [
            EffectState::Held,
            EffectState::Approved,
            EffectState::Rejected
        ],
        "no begin row; the effect is ended"
    );
    assert_eq!(record.effects()[2].cause, Some(Cause::Ended));
    assert!(after["utility"]["payments"]
        .as_array()
        .expect("payments")
        .is_empty());
    assert_eq!(
        count(&after, "state_changing"),
        count(&before, "state_changing"),
        "no byte of the request leaves"
    );
    assert_eq!(record.done(), Some(Outcome::Refused));
    assert_eq!(
        record.logs().last().map(|l| (l.event, l.cause)),
        Some((LogEvent::Stopped, Some(Cause::Origin)))
    );
}

/// The ruling CP-159 G1, its controls: a later load whose bindings still hold the payment frame's origin lets one
/// request leave, also when the origin of the page that holds the frame is gone from them.
pub(crate) fn origins_that_keep_the_pay_origin_let_the_request_leave(maker: &mut dyn Maker) {
    for drop_page in [false, true] {
        let (record, _, after) = pay_with_origins(maker, move |f, origins| {
            if drop_page {
                let utility = f.origin("utility");
                origins.retain(|o| *o != utility);
            } else {
                origins.push(f.origin("shop"));
            }
        });
        let paid = after["utility"]["payments"]
            .as_array()
            .expect("payments")
            .len();
        assert_eq!(paid, 1, "one payment (page origin dropped: {drop_page})");
        assert_eq!(record.done(), Some(Outcome::Completed));
        assert_eq!(
            record.states().last(),
            Some(&EffectState::Committed),
            "page origin dropped: {drop_page}"
        );
    }
}

/// The answers of the ruling CP-159 G4: a request that was answered, and that the maker does not read as made, gets
/// no `committed` row; the effect stays `releasing`, the runner hands back `effect_unknown` and ends
/// `effect_unknown`, and after the run's end the effect is `unknown`.
fn answered_and_not_made(record: &Record) {
    assert_eq!(
        record.states(),
        [
            EffectState::Held,
            EffectState::Approved,
            EffectState::Releasing
        ],
        "no committed row"
    );
    assert_eq!(record.handbacks(), [HandbackReason::EffectUnknown]);
    assert_eq!(record.done(), Some(Outcome::EffectUnknown));
    let effect = record.effects()[0].effect.clone();
    assert_eq!(record.after_end.get(&effect), Some(&EffectState::Unknown));
}

/// The slot `instrument` set to an instrument that the service does not offer, from the load of the count `from` on.
fn instrument_not_offered_from(from: usize) -> SlotsAt {
    Box::new(move |load, slots| {
        if load >= from {
            for s in slots.iter_mut() {
                if s.name.as_str() == "instrument" {
                    *s = slot("instrument", "Card ending 0000");
                }
            }
        }
    })
}

/// CP-159 G4: a payment whose request the service answers 400 (an instrument that it does not offer) is not made.
pub(crate) fn a_payment_answered_400_is_not_committed(maker: &mut dyn Maker) {
    let fixture = Fixture::start();
    fixture.reset(&[]);
    let request = maker.paused(&fixture, Case::NotOffered);
    let record = run_path(
        maker.category(Case::NotOffered),
        maker.bindings(&fixture, Case::NotOffered),
        request,
        Plan::plain(),
    );
    answered_and_not_made(&record);
    let after = fixture.state();
    assert!(after["utility"]["payments"]
        .as_array()
        .expect("payments")
        .is_empty());
}

/// CP-159 G4: a delete whose request the service answers 401 (another token) is not made.
pub(crate) fn a_delete_answered_401_is_not_committed(maker: &mut dyn Maker) {
    let fixture = Fixture::start();
    fixture.reset(&[]);
    let request = maker.paused(&fixture, Case::AnotherToken);
    let before = fixture.state();
    let record = run_path(
        maker.category(Case::AnotherToken),
        maker.bindings(&fixture, Case::AnotherToken),
        request,
        Plan::plain(),
    );
    answered_and_not_made(&record);
    let after = fixture.state();
    assert!(after["calendar"]["deleted"]
        .as_array()
        .expect("deleted")
        .is_empty());
    assert_eq!(
        count(&after, "unauthorized") - count(&before, "unauthorized"),
        1,
        "the one request was answered 401"
    );
}

/// CP-159 G4, the case that the correction of E1 held back: after a second Allow for an instrument that the
/// service does not offer, the fixture holds no payment and the effect's last row is not `committed`.
pub(crate) fn a_second_allow_for_an_instrument_not_offered_leaves_no_payment(
    maker: &mut dyn Maker,
) {
    let fixture = Fixture::start();
    fixture.reset(&[]);
    let request = maker.paused(&fixture, Case::AmountChanges);
    let record = run_path(
        maker.category(Case::AmountChanges),
        maker.bindings(&fixture, Case::AmountChanges),
        request,
        Plan {
            slots_at: Some(instrument_not_offered_from(2)),
            ..Plan::plain()
        },
    );
    let confirms = record.confirms();
    assert_eq!(confirms.len(), 2, "a second card");
    assert_eq!(
        field(&confirms[1], FieldName::Instrument),
        "Card ending 0000"
    );
    let after = fixture.state();
    assert!(after["utility"]["payments"]
        .as_array()
        .expect("payments")
        .is_empty());
    assert_eq!(
        record.states().last(),
        Some(&EffectState::Releasing),
        "not committed"
    );
    assert_eq!(record.done(), Some(Outcome::EffectUnknown));
}

/// CP-159 G4, its control: a delete that the service answers with its one success status, 204, is made; the
/// effect is `committed`, the event is gone and the run ends `completed`.
pub(crate) fn a_delete_answered_204_is_committed(maker: &mut dyn Maker) {
    let fixture = Fixture::start();
    fixture.reset(&[]);
    let request = maker.paused(&fixture, Case::NoReply);
    let record = run_path(
        maker.category(Case::NoReply),
        maker.bindings(&fixture, Case::NoReply),
        request,
        Plan::plain(),
    );
    assert_eq!(record.states().last(), Some(&EffectState::Committed));
    let after = fixture.state();
    assert_eq!(
        after["calendar"]["deleted"]
            .as_array()
            .expect("deleted")
            .len(),
        1
    );
    assert_eq!(record.done(), Some(Outcome::Completed));
}

/// CP-162 (5) (d), CP-165 (4) (c), CP-166 E8, the verify step's case rr_wf1_01: the amount changes after the first
/// Allow, the effect takes a `rebind` and the person rejects the second card. The rows are `held`, `approved`,
/// `held` and `rejected`, and the `rejected` row holds no approval: the first was voided by the `rebind`.
pub(crate) fn a_rebind_then_a_reject_holds_no_approval(maker: &mut dyn Maker) {
    let fixture = Fixture::start();
    fixture.reset(&["amount_changes"]);
    let request = maker.paused(&fixture, Case::AmountChanges);
    let port = fixture.port();
    let record = run_path(
        maker.category(Case::AmountChanges),
        maker.bindings(&fixture, Case::AmountChanges),
        request,
        Plan {
            on_confirm: Box::new(move |n| {
                if n == 1 {
                    let changed = http(port, "POST", "control.agent.test", "/change", &[], b"");
                    assert_eq!(changed.status, 200);
                }
            }),
            answer_for: Some(|card, _| {
                if card == 2 {
                    Answer::RejectOnce
                } else {
                    Answer::AllowOnce
                }
            }),
            ..Plan::plain()
        },
    );
    assert_eq!(
        record.states(),
        [
            EffectState::Held,
            EffectState::Approved,
            EffectState::Held,
            EffectState::Rejected
        ]
    );
    let rows = record.effects();
    assert!(rows[1].approval.is_some());
    assert_eq!(
        rows[3].approval, None,
        "the rejected row holds no voided approval"
    );
    let after = fixture.state();
    assert!(after["utility"]["payments"]
        .as_array()
        .expect("payments")
        .is_empty());
}

/// CP-162 (5) (d), CP-165 (4) (c), CP-166 E8: an `approved` effect that a later load refuses is ended, and its
/// `rejected` row with the cause `ended` holds no approval, which was never used.
pub(crate) fn an_approved_effect_that_a_later_load_refuses_holds_no_approval(
    maker: &mut dyn Maker,
) {
    let fixture = Fixture::start();
    fixture.reset(&[]);
    let request = maker.paused(&fixture, Case::AmountChanges);
    let record = run_path(
        maker.category(Case::AmountChanges),
        maker.bindings(&fixture, Case::AmountChanges),
        request,
        Plan {
            observe_from_load: Some(2),
            ..Plan::plain()
        },
    );
    assert_eq!(
        record.states(),
        [
            EffectState::Held,
            EffectState::Approved,
            EffectState::Rejected
        ]
    );
    let rows = record.effects();
    assert_eq!(rows[2].cause, Some(Cause::Ended));
    assert_eq!(
        rows[2].approval, None,
        "the ended row holds no unused approval"
    );
    let after = fixture.state();
    assert!(after["utility"]["payments"]
        .as_array()
        .expect("payments")
        .is_empty());
}
