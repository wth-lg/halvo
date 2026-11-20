use halvo_coord::guard::{guard_step, EffectRecord, EffectState, Gives, Table};
use halvo_coord::items::drive::{answer_bind, lapse_bind, raise_bind, BindState, HeldBind, Ledger};
use halvo_coord::items::{
    confirm_step, raise_step, ConfirmAnswer, ConfirmMove, ConfirmState, Confirmation, Raise, Source,
};
use halvo_coord::mem::{Call, Config, Method, Ports};
use halvo_coord::ports::{Analysis, Clock, Owner, PortError, Reaped, Rng, Runs};
use halvo_coord::run::{delete_step, drive, recover, recover_step, Act, Earlier, StopBy};
use halvo_coord::schedule::{self, holds, Answer, Board, Ended, Served};
use halvo_coord::types::{
    Agent, AgentId, AgentState, Answered, ApprovalId, AskedEffect, Body, Category, Cause, Class,
    Cleared, ConfirmAsk, Decision, DecisionEntry, Digest, Doing, EffectAnswer, EffectId, Event,
    Facts, FullWorkflow, ItemId, LapseCause, Lapsed, Limits, Live, Name, Outcome, PastRun,
    Presenter, Raised, RunEnd, RunId, RunItem, RunItemKind, Unknown, What, Workflow, WorkflowId,
    WorkflowState, WorkflowTrigger,
};
use halvo_coord::types::{
    Answer as PortAnswer, BindAsk, BindResult, Choice, Decision as PortDecision, HandbackResult,
    Origin, OtherLapse, Reply, RunEvent, RunSpec, SignInAnswer,
};
use serde_json::{json, Map, Value};
use std::cell::Cell;
use std::collections::BTreeMap;
use std::num::NonZeroU32;

const DAY: u32 = 20_000;
const T0: u64 = 1_700_000_000_000;
const SCHEDULE: &str = include_str!("../../../formal/r4-coordinator/traces-schedule.jsonl");
const CONFIRM: &str = include_str!("../../../formal/r4-coordinator/traces-confirm.jsonl");
const LIMITS: &str = include_str!("../../../formal/r4-coordinator/limits.json");

fn limits() -> Limits {
    Limits::from_json(LIMITS.replace('\r', "").trim_end()).unwrap()
}

/// A trace of a committed file: its number, its case or none, and its lines.
struct Trace {
    n: u64,
    case: Option<String>,
    moves: Vec<Value>,
}

impl Trace {
    fn name(&self) -> String {
        match &self.case {
            Some(c) => format!("trace {} ({c})", self.n),
            None => format!("trace {}", self.n),
        }
    }
}

fn read_file(text: &str) -> Result<(String, Vec<Trace>), String> {
    let mut lines = text.split('\n').map(|l| l.trim_end_matches('\r'));
    let head: Value = serde_json::from_str(lines.next().ok_or("empty file")?)
        .map_err(|e| format!("first line: {e}"))?;
    let stamp = head["stamp"]
        .as_str()
        .ok_or("the first line holds no stamp")?
        .to_string();
    let mut traces = Vec::new();
    for line in lines.filter(|l| !l.is_empty()) {
        let t: Value = serde_json::from_str(line).map_err(|e| format!("a trace line: {e}"))?;
        traces.push(Trace {
            n: t["n"].as_u64().ok_or("a trace with no n")?,
            case: t["case"].as_str().map(str::to_string),
            moves: t["moves"]
                .as_array()
                .ok_or("a trace with no moves")?
                .clone(),
        });
    }
    Ok((stamp, traces))
}

/// Both committed files of one export, refused when their two stamps differ (I5.13).
fn read_pair(schedule: &str, confirm: &str) -> Result<(Vec<Trace>, Vec<Trace>), String> {
    let (a, s) = read_file(schedule)?;
    let (b, c) = read_file(confirm)?;
    if a != b {
        return Err(format!("the two trace files hold two stamps: {a} and {b}"));
    }
    Ok((s, c))
}

fn hex_id(short: &str, width: usize) -> String {
    let h: String = short.bytes().map(|b| format!("{b:02x}")).collect();
    format!("{h:0>width$}")
}

fn agent_of(short: &str) -> AgentId {
    AgentId::parse(&hex_id(short, 32)).unwrap()
}

fn flow_of(n: usize) -> WorkflowId {
    WorkflowId::parse(&hex_id(&format!("w{n}"), 32)).unwrap()
}

fn cause_name(answer: &Result<Answer, Cause>) -> String {
    match answer {
        Ok(Answer::Ok) => "ok".into(),
        Ok(Answer::Queued) => "queued".into(),
        Err(c) => c.name().into(),
    }
}

struct Mismatch(Vec<String>);

impl Mismatch {
    fn check(&mut self, at: &str, key: &str, want: &Value, got: &Value) {
        if want != got && self.0.len() < 400 {
            self.0
                .push(format!("{at}: {key}: the model has {want}, the code {got}"));
        }
    }
}

/// The schedule's world of the README: five agents and their workflows, a run of each but
/// `a3` completed on the day before the first, every agent `active` from the first line
/// (ruling IG-8).
const AGENTS: [(bool, bool, bool, bool); 5] = [
    (true, true, true, true),
    (false, true, true, true),
    (false, false, true, false),
    (true, false, false, true),
    (false, false, false, true),
];

fn sched_config(n: usize) -> Config {
    let (person, model, _, _) = AGENTS[n];
    Config::parse(&format!(
        "{{\"v\":1,\"workflow\":\"{}\",\"agent\":\"{}\",\"rev\":1,\"category\":\"calendar_cleanup\",\"tier\":\"on_device\",\"reasoning\":\"medium\",\"autonomy\":\"draft_and_confirm\",\"allow\":[],\"caps\":{{\"untrusted\":\"yes\",\"private\":\"yes\",\"external\":\"{}\"}},\"limits\":{{\"steps\":20,\"wall_s\":120,\"model_tokens\":{}}},\"budget\":null,\"permissions\":[],\"runtime\":\"graph_calendar\",\"playbook\":1}}",
        flow_of(n + 1),
        agent_of(&format!("a{}", n + 1)),
        if person { "yes" } else { "no" },
        if model { 1000 } else { 0 },
    ))
    .unwrap()
}

fn sched_workflow(n: usize) -> FullWorkflow {
    FullWorkflow {
        id: flow_of(n + 1),
        state: WorkflowState::Active,
        category: Category::CalendarCleanup,
        site: None,
        trigger: if AGENTS[n].2 {
            WorkflowTrigger::Schedule
        } else {
            WorkflowTrigger::Manual
        },
        period: NonZeroU32::new(2),
        last: DAY - 2,
        agent: Some(agent_of(&format!("a{}", n + 1))),
    }
}

struct Names {
    runs: Vec<RunId>,
    agents: Vec<AgentId>,
}

impl Names {
    fn run(&self, run: &RunId) -> Value {
        match self.runs.iter().position(|r| r == run) {
            Some(k) => json!(format!("r{}", k + 1)),
            None => json!(format!("unnamed:{run}")),
        }
    }

    fn run_id(&self, name: &Value) -> RunId {
        let k: usize = name.as_str().unwrap()[1..].parse().unwrap();
        self.runs[k - 1].clone()
    }

    fn agent(&self, agent: &AgentId) -> Value {
        match self.agents.iter().position(|a| a == agent) {
            Some(k) => json!(format!("a{}", k + 1)),
            None => json!(format!("unknown:{agent}")),
        }
    }

    fn rank(&self, name: &Value) -> u64 {
        name.as_str()
            .and_then(|s| s.get(1..))
            .and_then(|s| s.parse().ok())
            .unwrap_or(u64::MAX)
    }
}

fn rel(day: u32) -> Value {
    json!(i64::from(day) - i64::from(DAY) + 1)
}

/// The events of I7.1 as a schedule line holds them, less every event of an item of the
/// coordinator's own kinds, which the schedule model does not hold (ruling XB-12 for `ready`,
/// and by the same reason the item `check` of ruling XB-18 and the lapses of recovery).
fn sched_event(e: &Event, names: &Names) -> Option<Value> {
    let agent = names.agent(&e.agent);
    Some(match &e.what {
        What::RunStarted { run, trigger, slot } => json!({
            "agent": agent, "event": "run_started", "run": names.run(run),
            "slot": slot.map_or(Value::Null, rel), "trigger": trigger.name()
        }),
        What::RunEnded {
            run,
            cause,
            defers: _,
        } => json!({
            "agent": agent, "event": "run_ended", "run": names.run(run), "cause": cause.name()
        }),
        What::HoldSet { cause } => {
            json!({"agent": agent, "event": "hold_set", "cause": cause.name()})
        }
        What::HoldCleared { cause } => match cause {
            Cleared::Person => json!({"agent": agent, "event": "hold_cleared", "cause": "person"}),
            Cleared::Failures => {
                json!({"agent": agent, "event": "hold_cleared", "cause": "failures"})
            }
            Cleared::Check { run, .. } => json!({
                "agent": agent, "event": "hold_cleared", "cause": "check", "run": names.run(run)
            }),
        },
        What::SlotSkipped { slot } => json!({
            "agent": agent, "event": "slot_skipped", "slot": slot.map_or(Value::Null, rel)
        }),
        What::ItemRaised {
            raised: Raised::Ready | Raised::Check(None) | Raised::Setup,
            ..
        }
        | What::ItemAnswered {
            answer: Answered::Ready(_) | Answered::Check(_) | Answered::Setup(_),
            ..
        }
        | What::ItemLapsed {
            lapsed: Lapsed::Ready(_) | Lapsed::Check(_) | Lapsed::Setup(_),
            ..
        } => return None,
        other => json!({"agent": agent, "event": format!("{other:?}")}),
    })
}

fn sched_call(call: &Call, names: &Names) -> Option<Value> {
    Some(match call {
        Call::Defer(_) => json!({"call": "defer"}),
        Call::Send(Body::StartRun { run, .. }) => {
            json!({"call": "start_run", "run": names.run(run)})
        }
        Call::Send(Body::SetState { agent, state, .. }) => json!({
            "call": "set_state", "agent": names.agent(agent), "state": state.name()
        }),
        Call::Send(Body::EndRun { run }) => json!({"call": "end_run", "run": names.run(run)}),
        Call::Send(other) => json!({"call": format!("{other:?}")}),
        Call::Start(spec) => json!({"call": "start", "run": names.run(&spec.run)}),
        Call::Cancel(run) => json!({"call": "cancel", "run": names.run(run)}),
        Call::Kill(run) => json!({"call": "kill", "run": names.run(run)}),
        Call::Reap => json!({"call": "reap"}),
        Call::AnswerConfirm(..) | Call::AnswerHandback(..) | Call::AnswerBind(..) => {
            json!({"call": "answer"})
        }
        Call::Log(_)
        | Call::History(..)
        | Call::Workflows
        | Call::Agents
        | Call::Attention(_)
        | Call::Changed => return None,
    })
}

fn served(s: &Served, names: &Names) -> Value {
    json!({
        "agent": names.agent(&s.agent),
        "r": cause_name(&s.answer),
        "run": s.run.as_ref().map_or(Value::Null, |r| names.run(r)),
    })
}

fn ended(e: &Ended, names: &Names) -> Value {
    json!({
        "run": names.run(&e.run),
        "cause": e.cause.name(),
        "next": e.next.iter().map(|s| served(s, names)).collect::<Vec<_>>(),
    })
}

/// The schedule's environment and the coordinator of one trace on the memory ports.
struct Sched {
    ports: Ports<Config>,
    board: Board<Config>,
    table: Table,
    limits: Limits,
    names: Names,
    events_seen: usize,
    tried_seen: usize,
    open: Vec<RunId>,
    left: BTreeMap<String, bool>,
    unknown: BTreeMap<usize, RunId>,
    life: u64,
    items: u32,
}

impl Sched {
    fn new() -> Sched {
        let limits = limits();
        let ports: Ports<Config> = Ports::with_limits(T0, DAY, limits);
        let agents: Vec<AgentId> = (1..=5).map(|n| agent_of(&format!("a{n}"))).collect();
        for n in 0..5 {
            ports
                .workflows
                .borrow_mut()
                .push(Workflow::Full(sched_workflow(n)));
            let mut last = None;
            if AGENTS[n].3 {
                let run = RunId::parse(&hex_id(&format!("b{}", n + 1), 32)).unwrap();
                let event = |seq: u32, what: What| Event {
                    workflow: flow_of(n + 1),
                    agent: agents[n].clone(),
                    seq: NonZeroU32::new(seq).unwrap(),
                    day: DAY - 2,
                    at: 0,
                    what,
                };
                ports.events.borrow_mut().extend([
                    event(
                        1,
                        What::RunStarted {
                            run: run.clone(),
                            trigger: halvo_coord::types::Trigger::Person,
                            slot: None,
                        },
                    ),
                    event(
                        2,
                        What::RunEnded {
                            run: run.clone(),
                            cause: RunEnd::Completed,
                            defers: None,
                        },
                    ),
                ]);
                last = Some(PastRun {
                    run,
                    day: DAY - 2,
                    outcome: Outcome::Completed,
                    unknown: 0,
                });
            }
            ports.agents.borrow_mut().push(Agent {
                workflow: flow_of(n + 1),
                agent: agents[n].clone(),
                config: sched_config(n),
                state: AgentState::Active,
                slots: Vec::new(),
                session: true,
                last,
                unknown: Vec::new(),
            });
        }
        let life = recover(&ports, &limits).unwrap();
        let events_seen = ports.events.borrow().len();
        let tried_seen = ports.tried.borrow().len();
        Sched {
            ports,
            board: life.board,
            table: Table::new(),
            limits,
            names: Names {
                runs: Vec::new(),
                agents,
            },
            events_seen,
            tried_seen,
            open: Vec::new(),
            left: BTreeMap::new(),
            unknown: BTreeMap::new(),
            life: 1,
            items: 0,
        }
    }

    fn index(&self, agent: &Value) -> usize {
        agent.as_str().unwrap()[1..].parse::<usize>().unwrap() - 1
    }

    fn ports_for(&self, port: Option<&str>) {
        self.ports.clear_refusals();
        self.ports.refuse(Method::EndRun, PortError::NoService);
        match port {
            Some("no_ack") => self
                .ports
                .refuse(Method::LogRunStarted, PortError::NoService),
            Some("no_run") => self.ports.refuse(
                Method::Start,
                PortError::Refused(Name::parse("not_started").unwrap()),
            ),
            Some("no_store") => self.ports.refuse(Method::StartRun, PortError::NoService),
            _ => {}
        }
    }

    fn agent_run(&self, n: usize) -> Option<RunId> {
        let agent = &self.names.agents[n];
        self.open
            .iter()
            .rev()
            .find(|r| {
                self.ports.events.borrow().iter().any(|e| {
                    &e.agent == agent
                        && matches!(&e.what, What::RunStarted { run, .. } if run == *r)
                })
            })
            .cloned()
    }

    fn start_day(&self, run: &RunId) -> u32 {
        self.ports
            .events
            .borrow()
            .iter()
            .find_map(|e| match &e.what {
                What::RunStarted { run: r, .. } if r == run => Some(e.day),
                _ => None,
            })
            .unwrap_or(DAY)
    }

    fn store_ends(&mut self, n: usize, run: &RunId, outcome: Outcome, unknown: u32) {
        self.ports.store_end(
            &self.names.agents[n],
            PastRun {
                run: run.clone(),
                day: self.start_day(run),
                outcome,
                unknown,
            },
        );
        self.open.retain(|r| r != run);
        if unknown > 0 {
            self.unknown.insert(n, run.clone());
        }
    }

    /// The agent store's `end_run` of a run that left an effect `releasing`: its record counts 0
    /// and the roster entry's key `unknown` names the effect (ruling RD-81 8.1, 13.5), which the
    /// model's fact of a run whose outcome is not known maps.
    fn key_names(&mut self, n: usize, run: &RunId) {
        let agent = &self.names.agents[n];
        if let Some(a) = self
            .ports
            .agents
            .borrow_mut()
            .iter_mut()
            .find(|a| &a.agent == agent)
        {
            a.unknown.push(Unknown {
                run: run.clone(),
                effect: EffectId::parse(&format!("{:032x}", 0xe400 + n)).unwrap(),
                day: DAY,
                class: Class::Send,
                kept: vec![],
            });
        }
        self.unknown.insert(n, run.clone());
    }

    fn member_of(&self, run: &RunId) -> Option<usize> {
        let r = self.board.runs.iter().find(|r| &r.run == run)?;
        self.names.agents.iter().position(|a| a == &r.agent)
    }

    fn act(&mut self, act: Act) -> halvo_coord::run::Done {
        drive(
            &self.ports,
            &mut self.board,
            &mut self.table,
            &self.limits,
            act,
        )
    }

    /// One line: the move on the code, then the answer, the events, the calls and the state
    /// compared with the model's.
    fn line(&mut self, m: &Value, at: &str, out: &mut Mismatch) -> Value {
        let mv = m["m"].as_str().unwrap();
        let a = &m["a"];
        let mut keys = Map::new();
        let now = self.ports.now_ms() + 1;
        self.ports.set_clock(now, self.board.day);
        let answer: Value;
        match mv {
            "start" => {
                self.ports_for(a["port"].as_str());
                let n = self.index(&a["agent"]);
                let done = self.act(Act::Start {
                    agent: self.names.agents[n].clone(),
                });
                answer = json!(cause_name(&done.report.answer));
                self.name_new_events();
                keys.insert(
                    "run".into(),
                    done.report
                        .run
                        .as_ref()
                        .map_or(Value::Null, |r| self.names.run(r)),
                );
            }
            "hello" | "wait" | "resume" | "stop" => {
                self.ports_for(None);
                let run = self.names.run_id(&a["run"]);
                let act = match mv {
                    "hello" => Act::Hello { run },
                    "wait" => {
                        self.items += 1;
                        Act::Wait {
                            run,
                            item: RunItem {
                                item: ItemId::parse(&format!("{:032x}", 0x1700 + self.items))
                                    .unwrap(),
                                kind: RunItemKind::Confirm,
                            },
                        }
                    }
                    "resume" => Act::Resume { run },
                    _ => {
                        let waits = self.board.runs.iter().any(|r| {
                            r.run == run
                                && self.board.members.iter().any(|m| {
                                    m.facts.agent == r.agent
                                        && matches!(m.facts.doing, Doing::Live(Live::Waiting(_)))
                                })
                        });
                        if waits {
                            self.table = self.table.stop(&run);
                            let _ = halvo_coord::ports::Runs::cancel(&self.ports, &run);
                        }
                        let by = match a["cause"].as_str() {
                            Some("cancelled") => StopBy::Person,
                            Some("lapsed") => StopBy::WaitLimit,
                            Some("answer_lost") => StopBy::AnswerLost,
                            other => panic!("{at}: no stop cause {other:?}"),
                        };
                        Act::Stop { run, by }
                    }
                };
                let done = self.act(act);
                answer = json!(cause_name(&done.report.answer));
            }
            "end" => {
                self.ports_for(a["port"].as_str());
                let run = self.names.run_id(&a["run"]);
                let frame = a["frame"].as_str().map(|f| Outcome::parse(f).unwrap());
                let live = self.member_of(&run);
                if let (Some(n), Some(o)) = (live, frame) {
                    let unknown = u32::from(o == Outcome::EffectUnknown);
                    self.store_ends(n, &run, o, unknown);
                }
                if live.is_some() {
                    self.ports.end_runner(&run);
                    if frame.is_none() {
                        self.left.insert(run.to_string(), a["left"] == "yes");
                    }
                }
                let waiting = live.is_some_and(|n| {
                    matches!(
                        self.board
                            .members
                            .iter()
                            .find(|m| m.facts.agent == self.names.agents[n])
                            .map(|m| &m.facts.doing),
                        Some(Doing::Live(Live::Waiting(_)))
                    )
                });
                if waiting {
                    self.act(Act::Resume { run: run.clone() });
                }
                let done = self.act(Act::End {
                    run,
                    outcome: frame,
                });
                answer = json!(cause_name(&done.report.answer));
                self.name_new_events();
                if let Some(e) = done.report.ended.first() {
                    keys.insert("cause".into(), json!(e.cause.name()));
                    keys.insert(
                        "next".into(),
                        json!(e
                            .next
                            .iter()
                            .map(|s| served(s, &self.names))
                            .collect::<Vec<_>>()),
                    );
                }
            }
            "tick" => {
                self.ports_for(a["port"].as_str());
                let day = DAY + a["day"].as_u64().unwrap() as u32 - 1;
                let grace = if a["grace"] == "yes" {
                    self.limits.cancel_grace_s * 1000
                } else {
                    0
                };
                self.ports.set_clock(now + grace, day);
                let listed: Vec<WorkflowId> = self
                    .ports
                    .workflows
                    .borrow()
                    .iter()
                    .map(|w| w.id().clone())
                    .collect();
                let gone = delete_step(&self.board, &listed);
                let done = self.act(Act::Tick);
                answer = json!(cause_name(&done.report.answer));
                self.name_new_events();
                if gone.stop != done.report.stopped {
                    out.check(
                        at,
                        "delete_step",
                        &json!("the tick's stops"),
                        &json!("others"),
                    );
                }
                for e in &done.report.ended {
                    self.left.insert(e.run.to_string(), a["left"] == "yes");
                }
                let r = &done.report;
                let names = &self.names;
                keys.insert(
                    "stopped".into(),
                    json!(r.stopped.iter().map(|x| names.run(x)).collect::<Vec<_>>()),
                );
                keys.insert(
                    "ended".into(),
                    json!(r.ended.iter().map(|x| ended(x, names)).collect::<Vec<_>>()),
                );
                keys.insert(
                    "next".into(),
                    json!(r.next.iter().map(|x| served(x, names)).collect::<Vec<_>>()),
                );
                keys.insert(
                    "raised".into(),
                    json!(r.raised.iter().map(|x| names.agent(x)).collect::<Vec<_>>()),
                );
                keys.insert(
                    "started".into(),
                    json!(r
                        .started
                        .iter()
                        .map(|x| served(x, names))
                        .collect::<Vec<_>>()),
                );
            }
            "restart" => {
                self.ports_for(None);
                let day = DAY + a["day"].as_u64().unwrap() as u32 - 1;
                self.ports.set_clock(now, day);
                let alive: Vec<RunId> = a["alive"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|r| self.names.run_id(r))
                    .collect();
                for r in &self.board.runs {
                    if !alive.contains(&r.run) {
                        self.left.insert(r.run.to_string(), a["left"] == "yes");
                    }
                }
                let marks: Vec<(RunId, Reaped)> = self
                    .open
                    .iter()
                    .map(|r| {
                        let answer = if alive.contains(r) {
                            Reaped::Alive
                        } else {
                            Reaped::Gone
                        };
                        (r.clone(), answer)
                    })
                    .collect();
                self.ports.new_life();
                self.ports.leave_marks(&marks);
                let life = recover(&self.ports, &self.limits).unwrap();
                self.board = life.board;
                self.table = Table::new();
                self.life += 1;
                answer = json!("ok");
                self.name_new_events();
                keys.insert(
                    "ended".into(),
                    json!(life
                        .ended
                        .iter()
                        .map(|x| ended(x, &self.names))
                        .collect::<Vec<_>>()),
                );
            }
            "ack" => {
                let n = self.index(&a["agent"]);
                if let Some(run) = self.agent_run(n) {
                    let left = self.left.get(&run.to_string()) == Some(&true);
                    self.store_ends(n, &run, Outcome::Failed, 0);
                    if left {
                        self.key_names(n, &run);
                    }
                }
                answer = json!("ok");
            }
            "check" => {
                self.ports_for(None);
                let n = self.index(&a["agent"]);
                let report = schedule::drive(
                    &self.ports,
                    &mut self.board,
                    &self.limits,
                    schedule::Move::Check {
                        agent: self.names.agents[n].clone(),
                        answer: halvo_coord::types::CheckAnswer::TryAgain,
                    },
                );
                answer = json!(cause_name(&report.answer));
            }
            "delete" => {
                let n = self.index(&a["agent"]);
                self.ports
                    .workflows
                    .borrow_mut()
                    .retain(|w| w.id() != &flow_of(n + 1));
                answer = json!("ok");
            }
            "off" => {
                self.ports_for(None);
                let n = self.index(&a["agent"]);
                let lossy = Lossy {
                    inner: &self.ports,
                    lose: Cell::new(a["port"] == "lost" || a["port"] == "lost_read"),
                    unread: a["port"] == "lost",
                    read_fails: Cell::new(false),
                };
                let done = drive(
                    &lossy,
                    &mut self.board,
                    &mut self.table,
                    &self.limits,
                    Act::SwitchOff {
                        agent: self.names.agents[n].clone(),
                    },
                );
                answer = json!(cause_name(&done.report.answer));
                keys.insert(
                    "stopped".into(),
                    json!(done
                        .report
                        .stopped
                        .iter()
                        .map(|x| self.names.run(x))
                        .collect::<Vec<_>>()),
                );
            }
            other => panic!("{at}: no move {other}"),
        }
        self.name_new_events();
        out.check(at, "r", &m["r"], &answer);
        let events: Vec<Value> = self.ports.events.borrow()[self.events_seen..]
            .iter()
            .filter_map(|e| sched_event(e, &self.names))
            .collect();
        self.events_seen = self.ports.events.borrow().len();
        out.check(at, "e", &m["e"], &json!(events));
        let calls: Vec<Value> = self.ports.tried.borrow()[self.tried_seen..]
            .iter()
            .filter_map(|t| sched_call(&t.call, &self.names))
            .collect();
        self.tried_seen = self.ports.tried.borrow().len();
        out.check(at, "p", &m["p"], &json!(calls));
        self.state(&mut keys);
        let want = m["s"].as_object().unwrap();
        let got_keys: Vec<&String> = keys.keys().collect();
        let want_keys: Vec<&String> = want.keys().collect();
        let mut gk = got_keys.clone();
        let mut wk = want_keys.clone();
        gk.sort();
        wk.sort();
        out.check(at, "keys of s", &json!(wk), &json!(gk));
        for (k, v) in want {
            out.check(
                at,
                &format!("s.{k}"),
                v,
                keys.get(k).unwrap_or(&Value::Null),
            );
        }
        json!({"r": answer, "e": events, "p": calls, "s": keys})
    }

    fn name_new_events(&mut self) {
        let events = self.ports.events.borrow();
        for e in events[self.events_seen..].iter() {
            if let What::RunStarted { run, .. } = &e.what {
                if !self.names.runs.contains(run) {
                    self.names.runs.push(run.clone());
                    self.open.push(run.clone());
                    if let Some(n) = self.names.agents.iter().position(|a| a == &e.agent) {
                        self.unknown.remove(&n);
                    }
                }
            }
        }
    }

    fn facts_now(&self, facts: &Facts<Config>) -> Facts<Config> {
        let mut f = facts.clone();
        let agents = self.ports.agents.borrow();
        let entry = agents.iter().find(|a| a.agent == f.agent);
        let last = entry.and_then(|a| a.last.clone());
        let newest = f.events.iter().rev().find_map(|e| match &e.what {
            What::RunStarted { run, .. } => Some(run.clone()),
            _ => None,
        });
        f.store_ended = match newest {
            None => true,
            Some(run) => last.as_ref().is_some_and(|l| l.run == run),
        };
        if let Some(l) = last {
            f.runs.retain(|r| r.run != l.run);
            f.runs.insert(0, l);
        }
        if let Some(a) = entry {
            f.take_unknown(&a.unknown);
        }
        f
    }

    fn state(&self, keys: &mut Map<String, Value>) {
        let names = &self.names;
        let events = self.ports.events.borrow();
        let member = |n: usize| {
            self.board
                .members
                .iter()
                .find(|m| m.facts.agent == names.agents[n])
        };
        keys.insert("day".into(), rel(self.board.day));
        let mut runs = Map::new();
        for r in &self.board.runs {
            let phase = self
                .board
                .members
                .iter()
                .find(|m| m.facts.agent == r.agent)
                .and_then(|m| m.facts.doing.phase())
                .map_or("none", |p| p.name());
            if let Value::String(name) = names.run(&r.run) {
                runs.insert(name, json!(phase));
            }
        }
        keys.insert("runs".into(), Value::Object(runs));
        keys.insert(
            "queue".into(),
            json!(self
                .board
                .queue
                .entries()
                .iter()
                .map(|w| names.agent(&w.agent))
                .collect::<Vec<_>>()),
        );
        let agents_where = |f: &dyn Fn(usize) -> bool| -> Value {
            json!((0..5)
                .filter(|n| f(*n))
                .map(|n| format!("a{}", n + 1))
                .collect::<Vec<_>>())
        };
        keys.insert(
            "ready".into(),
            agents_where(&|n| {
                member(n).is_some_and(|m| {
                    matches!(&m.facts.doing, Doing::Idle(Some(o)) | Doing::Queued(Some(o))
                        if o.kind == halvo_coord::types::OwnItemKind::Ready)
                })
            }),
        );
        keys.insert(
            "held".into(),
            agents_where(&|n| {
                member(n).is_some_and(|m| !holds(&self.facts_now(&m.facts)).is_empty())
            }),
        );
        let of = |n: usize| events.iter().filter(move |e| e.agent == names.agents[n]);
        keys.insert(
            "starts".into(),
            json!((0..5)
                .map(|n| {
                    of(n)
                        .filter(|e| {
                            e.day == self.board.day && matches!(e.what, What::RunStarted { .. })
                        })
                        .count()
                })
                .collect::<Vec<_>>()),
        );
        keys.insert(
            "slot".into(),
            json!((0..5)
                .map(|n| of(n)
                    .filter_map(|e| match e.what {
                        What::RunStarted { slot, .. } => slot,
                        _ => None,
                    })
                    .next_back()
                    .map_or(Value::Null, rel))
                .collect::<Vec<_>>()),
        );
        let cap = self.limits.fails_to_pause as usize;
        keys.insert(
            "fails".into(),
            json!((0..5)
                .map(|n| {
                    let ends: Vec<RunEnd> = of(n)
                        .filter_map(|e| match e.what {
                            What::RunEnded { cause, .. } => Some(cause),
                            _ => None,
                        })
                        .collect();
                    ends.iter()
                        .rev()
                        .take_while(|c| c.failure().is_some())
                        .count()
                        .min(cap)
                })
                .collect::<Vec<_>>()),
        );
        let listed: Vec<WorkflowId> = self
            .ports
            .workflows
            .borrow()
            .iter()
            .map(|w| w.id().clone())
            .collect();
        keys.insert(
            "gone".into(),
            agents_where(&|n| !listed.contains(&flow_of(n + 1))),
        );
        keys.insert(
            "lost".into(),
            agents_where(&|n| !listed.contains(&flow_of(n + 1)) && member(n).is_none()),
        );
        let sorted = |mut v: Vec<Value>| {
            v.sort_by_key(|x| names.rank(x));
            json!(v)
        };
        keys.insert(
            "open".into(),
            sorted(self.open.iter().map(|r| names.run(r)).collect()),
        );
        keys.insert(
            "unknown".into(),
            sorted(self.unknown.values().map(|r| names.run(r)).collect()),
        );
        keys.insert(
            "cleared".into(),
            sorted(
                events
                    .iter()
                    .filter_map(|e| match &e.what {
                        What::HoldCleared {
                            cause: Cleared::Check { run, .. },
                        } => Some(names.run(run)),
                        _ => None,
                    })
                    .collect(),
            ),
        );
        keys.insert(
            "owing".into(),
            sorted(
                self.board
                    .owed
                    .iter()
                    .filter_map(|o| match &o.what {
                        schedule::Owing::EndRun { run } => Some(names.run(run)),
                        _ => None,
                    })
                    .collect(),
            ),
        );
        keys.insert("life".into(), json!(self.life));
    }
}

trait NowMs {
    fn now_ms(&self) -> u64;
}

impl NowMs for Ports<Config> {
    fn now_ms(&self) -> u64 {
        halvo_coord::ports::Clock::now(self).unwrap()
    }
}

fn replay_schedule(traces: &[Trace]) -> Vec<String> {
    let mut out = Mismatch(Vec::new());
    for t in traces {
        let mut s = Sched::new();
        let before = out.0.len();
        for (k, m) in t.moves.iter().enumerate() {
            let at = format!("schedule {} line {}", t.name(), k + 1);
            s.line(m, &at, &mut out);
            if out.0.len() > before {
                break;
            }
        }
    }
    out.0
}

/// The memory ports with an owner that can store a body and lose its reply: with `lose` set, the
/// next body that the memory owner stores is answered `no_service`, as a lost acknowledgement;
/// with `unread` set as well, the one read of the owner's events after that body fails too.
struct Lossy<'a> {
    inner: &'a Ports<Config>,
    lose: Cell<bool>,
    unread: bool,
    read_fails: Cell<bool>,
}

impl Owner for Lossy<'_> {
    type Config = Config;
    fn log(&self, events: &[Event]) -> Result<(), PortError> {
        let stored = self.inner.log(events);
        if stored.is_ok() && self.lose.replace(false) {
            self.read_fails.set(self.unread);
            return Err(PortError::NoService);
        }
        stored
    }
    fn history(&self, agent: &AgentId, after: u32) -> Result<(Vec<Event>, u32), PortError> {
        if self.read_fails.replace(false) {
            return Err(PortError::NoService);
        }
        self.inner.history(agent, after)
    }
    fn workflows(&self) -> Result<Vec<Workflow>, PortError> {
        self.inner.workflows()
    }
    fn agents(&self) -> Result<Vec<Agent<Config>>, PortError> {
        self.inner.agents()
    }
    fn send(&self, body: &Body) -> Result<Reply, PortError> {
        self.inner.send(body)
    }
    fn replies(
        &self,
        workflow: &halvo_coord::types::WorkflowId,
        agent: &halvo_coord::types::AgentId,
        run: &halvo_coord::types::RunId,
    ) -> Result<Vec<halvo_coord::types::ReplyRow>, PortError> {
        self.inner.replies(workflow, agent, run)
    }
}

impl Runs for Lossy<'_> {
    fn start(&self, spec: &RunSpec) -> Result<(), PortError> {
        self.inner.start(spec)
    }
    fn answer_confirm(
        &self,
        run: &RunId,
        answers: &[PortAnswer],
        record: &mut dyn FnMut(&PortDecision) -> Result<(), PortError>,
    ) -> Result<(), PortError> {
        self.inner.answer_confirm(run, answers, record)
    }
    fn answer_handback(&self, run: &RunId, result: HandbackResult) -> Result<(), PortError> {
        self.inner.answer_handback(run, result)
    }
    fn answer_bind(&self, run: &RunId, result: BindResult) -> Result<(), PortError> {
        self.inner.answer_bind(run, result)
    }
    fn cancel(&self, run: &RunId) -> Result<(), PortError> {
        self.inner.cancel(run)
    }
    fn kill(&self, run: &RunId) -> Result<(), PortError> {
        self.inner.kill(run)
    }
    fn reap(&self) -> Result<Vec<(RunId, Reaped)>, PortError> {
        self.inner.reap()
    }
}

impl Clock for Lossy<'_> {
    fn now(&self) -> Result<u64, PortError> {
        self.inner.now()
    }
    fn day(&self, at: u64) -> Result<u32, PortError> {
        self.inner.day(at)
    }
}

impl Rng for Lossy<'_> {
    fn bytes16(&self) -> Result<[u8; 16], PortError> {
        self.inner.bytes16()
    }
}

impl Analysis for Lossy<'_> {
    fn defer(&self, until: u64) -> Result<(), PortError> {
        self.inner.defer(until)
    }
}

/// One run of one agent with the confirmations and the guard's table, replayed through the
/// pure functions: `confirm_step` for the moves of an item, `raise_step` for whether a request
/// is admitted, `run_step` for the person's stop, `guard_step` for a record and the table's
/// own moves. The port mints approval ids from a counter, `p1` first, in the order of the
/// effects (I4.6). The second step of a `sign_in` item goes through the items drive on the
/// memory ports (`raise_bind`, `answer_bind`, `lapse_bind`), and a kill through `recover_step`
/// over the owner's events.
struct Conf {
    ports: Ports<Config>,
    ledger: Ledger,
    binds: Vec<(String, HeldBind)>,
    killed: Vec<String>,
    answering: Option<String>,
    events_seen: usize,
    tried_seen: usize,
    items: Vec<(String, Confirmation)>,
    pending: BTreeMap<String, Decision>,
    table: Table,
    run: RunId,
    agent: AgentId,
    state: &'static str,
    minted: u32,
    raised: Vec<Event>,
    limits: Limits,
}

fn conf_id<T>(parse: fn(&str) -> Result<T, Cause>, short: &str) -> T {
    parse(&hex_id(short, 32)).unwrap()
}

fn digest_of(short: &str) -> Digest {
    Digest::parse(&hex_id(short, 64)).unwrap()
}

fn short(hexed: &str) -> String {
    let bytes: Vec<u8> = (0..hexed.len() / 2)
        .map(|i| u8::from_str_radix(&hexed[2 * i..2 * i + 2], 16).unwrap())
        .filter(|b| *b != 0)
        .collect();
    String::from_utf8(bytes).unwrap()
}

fn state_name(s: &ConfirmState) -> &'static str {
    match s {
        ConfirmState::Shown => "shown",
        ConfirmState::Armed => "armed",
        ConfirmState::Answered(_) => "answered",
        ConfirmState::Recorded(_) => "recorded",
        ConfirmState::Released(_) => "released",
        ConfirmState::Lapsed(_) => "lapsed",
    }
}

fn entries(d: &Decision) -> Vec<Value> {
    d.approvals()
        .iter()
        .map(|e| {
            json!({
                "effect": short(e.effect.as_str()), "digest": short(e.digest.as_str()),
                "approval": short(e.approval.as_str()), "answer": e.answer.name()
            })
        })
        .collect()
}

fn rows(event: &str, item: &str, d: &Decision) -> Vec<Value> {
    d.approvals()
        .iter()
        .map(|e| {
            json!({
                "event": event, "item": item, "effect": short(e.effect.as_str()),
                "approval": short(e.approval.as_str()), "digest": short(e.digest.as_str()),
                "answer": e.answer.name()
            })
        })
        .collect()
}

impl Conf {
    fn new() -> Conf {
        let limits = limits();
        let ports: Ports<Config> = Ports::with_limits(T0, DAY, limits);
        ports
            .workflows
            .borrow_mut()
            .push(Workflow::Full(Conf::workflow()));
        let run = conf_id(RunId::parse, "r1");
        ports.runner_of(&run);
        Conf {
            ports,
            ledger: Ledger::new(
                conf_id(WorkflowId::parse, "w1"),
                conf_id(AgentId::parse, "a1"),
                0,
            ),
            binds: Vec::new(),
            killed: Vec::new(),
            answering: None,
            events_seen: 0,
            tried_seen: 0,
            items: Vec::new(),
            pending: BTreeMap::new(),
            table: Table::new(),
            run: conf_id(RunId::parse, "r1"),
            agent: conf_id(AgentId::parse, "a1"),
            state: "live",
            minted: 0,
            raised: Vec::new(),
            limits,
        }
    }

    fn mint(&mut self, conf: &Confirmation, answer: EffectAnswer) -> Decision {
        let entries = conf
            .asked()
            .iter()
            .map(|(effect, digest)| {
                self.minted += 1;
                DecisionEntry {
                    approval: conf_id(ApprovalId::parse, &format!("p{}", self.minted)),
                    effect: effect.clone(),
                    digest: digest.clone(),
                    answer,
                }
            })
            .collect();
        Decision::new(entries).unwrap()
    }

    fn workflow() -> FullWorkflow {
        FullWorkflow {
            id: conf_id(WorkflowId::parse, "w1"),
            state: WorkflowState::Active,
            category: Category::Email,
            site: None,
            trigger: WorkflowTrigger::Manual,
            period: None,
            last: DAY,
            agent: Some(conf_id(AgentId::parse, "a1")),
        }
    }

    fn open_item(&self) -> Option<RunItem> {
        let confirm = self
            .items
            .iter()
            .find(|(_, c)| c.state().is_open())
            .map(|(name, _)| RunItem {
                item: conf_id(ItemId::parse, name),
                kind: RunItemKind::Confirm,
            });
        let bind = self
            .binds
            .iter()
            .find(|(name, b)| *b.state() != BindState::Closed && !self.killed.contains(name))
            .map(|(_, b)| RunItem {
                item: b.item().clone(),
                kind: RunItemKind::SignIn,
            });
        confirm.or(bind)
    }

    fn bind_name(&self, item: &ItemId) -> String {
        self.binds
            .iter()
            .find(|(_, b)| b.item() == item)
            .map(|(n, _)| n.clone())
            .unwrap_or_default()
    }

    fn bind_state(&self, name: &str, b: &HeldBind) -> &'static str {
        if self.killed.iter().any(|k| k == name) {
            return "closed";
        }
        match b.state() {
            BindState::Open => "open",
            BindState::Recorded(BindResult::Allow) => "allow",
            BindState::Recorded(BindResult::Decline) => "decline",
            BindState::Closed => "closed",
        }
    }

    /// The events that the memory owner stored and the calls that act since the last line, as
    /// a trace line writes them.
    fn port_lines(&mut self, e: &mut Vec<Value>, p: &mut Vec<Value>) {
        let stored: Vec<Event> = self.ports.events.borrow()[self.events_seen..].to_vec();
        self.events_seen = self.ports.events.borrow().len();
        for ev in stored {
            match &ev.what {
                What::ItemRaised { item, .. } => {
                    e.push(json!({"event": "item_raised", "item": self.bind_name(item)}))
                }
                What::ItemAnswered {
                    item,
                    answer: Answered::SignIn(SignInAnswer::Second(b)),
                } => e.push(
                    json!({"event": "item_answered", "item": self.bind_name(item),
                    "answer": b.answer.name()}),
                ),
                What::ItemLapsed {
                    item,
                    lapsed: Lapsed::SignIn(cause),
                } => e.push(json!({"event": "item_lapsed", "item": self.bind_name(item),
                    "cause": cause.cause().name()})),
                other => panic!("an event that no bind line stores: {other:?}"),
            }
        }
        let tried: Vec<Call> = self.ports.tried.borrow()[self.tried_seen..]
            .iter()
            .map(|t| t.call.clone())
            .collect();
        self.tried_seen = self.ports.tried.borrow().len();
        let open = self
            .binds
            .iter()
            .rev()
            .find(|(_, b)| *b.state() != BindState::Closed)
            .map(|(n, _)| n.clone());
        for call in tried {
            match call {
                Call::AnswerBind(_, result) => {
                    let name = self.answering.clone().or(open.clone()).unwrap_or_default();
                    p.push(json!({"call": "answer_bind", "item": name, "answer": result.name()}))
                }
                Call::Cancel(_) => p.push(json!({"call": "cancel"})),
                _ => {}
            }
        }
    }

    fn facts(&self) -> Facts<Config> {
        let doing = match (self.state, self.open_item()) {
            ("live", Some(item)) => Doing::Live(Live::Waiting(item)),
            ("live", None) => Doing::Live(Live::Working),
            ("stopped", _) => Doing::Live(Live::Stopping),
            _ => Doing::Idle(None),
        };
        Facts {
            agent: self.agent.clone(),
            workflow: FullWorkflow {
                id: conf_id(WorkflowId::parse, "w1"),
                state: WorkflowState::Active,
                category: Category::Email,
                site: None,
                trigger: WorkflowTrigger::Manual,
                period: None,
                last: DAY,
                agent: Some(self.agent.clone()),
            },
            config: sched_config(0),
            slots: vec![Name::parse("reply_senders").unwrap()],
            runs: Vec::new(),
            events: self.raised.clone(),
            store_ended: true,
            replies: halvo_coord::types::Replies::NotAsked,
            unknown: Vec::new(),
            today: DAY,
            doing,
        }
    }

    fn item_index(&self, name: &Value) -> Option<usize> {
        self.items
            .iter()
            .position(|(n, _)| n == name.as_str().unwrap())
    }

    fn line(&mut self, m: &Value, at: &str, out: &mut Mismatch) {
        let a = &m["a"];
        let mut keys = Map::new();
        let mut e: Vec<Value> = Vec::new();
        let mut p: Vec<Value> = Vec::new();
        let r: String;
        match m["m"].as_str().unwrap() {
            "issue" => {
                let effects: Vec<AskedEffect> = a["effects"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|x| AskedEffect {
                        effect: conf_id(EffectId::parse, x["effect"].as_str().unwrap()),
                        class: Class::parse(x["class"].as_str().unwrap()).unwrap(),
                        digest: digest_of(x["digest"].as_str().unwrap()),
                        digest_ok: a["digest_ok"] == "yes",
                        fields: Vec::new(),
                        new_origin: None,
                    })
                    .collect();
                let ask = ConfirmAsk::new(self.run.clone(), self.agent.clone(), effects).unwrap();
                let mut sent = Vec::new();
                let mut item = Value::Null;
                match raise_step(&self.facts(), Source::Confirm(&ask), &self.limits) {
                    Raise::Run(RunItemKind::Confirm) => {
                        let conf = confirm_step(None, ConfirmMove::Issue(&ask)).unwrap();
                        let name = format!("i{}", self.items.len() + 1);
                        if let ConfirmState::Lapsed(LapseCause::Digest) = conf.state() {
                            let d = self.mint(&conf, EffectAnswer::RejectOnce);
                            sent = entries(&d);
                            p.push(
                                json!({"call": "answer_confirm", "item": name, "answer": "reject"}),
                            );
                            e.push(
                                json!({"event": "item_lapsed", "item": name, "cause": "digest"}),
                            );
                        } else {
                            e.push(json!({"event": "item_raised", "item": name}));
                            self.raised.push(Event {
                                workflow: conf_id(WorkflowId::parse, "w1"),
                                agent: self.agent.clone(),
                                seq: NonZeroU32::new(self.raised.len() as u32 + 1).unwrap(),
                                day: DAY,
                                at: 0,
                                what: What::ItemRaised {
                                    item: conf_id(ItemId::parse, &name),
                                    raised: Raised::Confirm(self.run.clone()),
                                },
                            });
                        }
                        item = json!(name);
                        self.items.push((name, conf));
                        r = "ok".into();
                    }
                    Raise::End(_) => {
                        self.state = "ended";
                        r = "state".into();
                    }
                    other => {
                        r = match other {
                            Raise::Refused(c) => c.name().into(),
                            _ => format!("{other:?}"),
                        }
                    }
                }
                keys.insert("item".into(), item);
                keys.insert("sent".into(), json!(sent));
            }
            "arm" | "answer" | "record" | "release" | "lapse" => {
                let mv = m["m"].as_str().unwrap();
                let Some(i) = self.item_index(&a["item"]) else {
                    panic!("{at}: no item {}", a["item"]);
                };
                let name = self.items[i].0.clone();
                let conf = self.items[i].1.clone();
                let mut sent = Vec::new();
                let step = match mv {
                    "arm" => confirm_step(Some(&conf), ConfirmMove::Arm),
                    "answer" => confirm_step(
                        Some(&conf),
                        ConfirmMove::Answer {
                            answer: if a["answer"] == "allow" {
                                ConfirmAnswer::Allow
                            } else {
                                ConfirmAnswer::Reject
                            },
                            from: Presenter::parse(a["from"].as_str().unwrap()).unwrap(),
                        },
                    ),
                    "record" => match self.pending.get(&name) {
                        Some(d) => confirm_step(Some(&conf), ConfirmMove::Record(d)),
                        None => Err(Cause::State),
                    },
                    "release" => confirm_step(Some(&conf), ConfirmMove::Release),
                    _ => confirm_step(
                        Some(&conf),
                        ConfirmMove::Lapse(
                            LapseCause::parse(a["cause"].as_str().unwrap()).unwrap(),
                        ),
                    ),
                };
                match step {
                    Ok(next) => {
                        r = "ok".into();
                        match mv {
                            "answer" => {
                                let answer = match next.state() {
                                    ConfirmState::Answered(x) => *x,
                                    _ => ConfirmAnswer::Reject,
                                };
                                let d = self.mint(&next, answer.once());
                                keys.insert(
                                    "approvals".into(),
                                    json!(d
                                        .approvals()
                                        .iter()
                                        .map(|x| short(x.approval.as_str()))
                                        .collect::<Vec<_>>()),
                                );
                                p.push(json!({"call": "answer_confirm", "item": name,
                                    "answer": if answer == ConfirmAnswer::Allow { "allow" } else { "reject" }}));
                                self.pending.insert(name.clone(), d);
                            }
                            "record" => {
                                let d = self.pending[&name].clone();
                                e.extend(rows("item_answered", &name, &d));
                                self.table = self.table.record(
                                    &conf_id(ItemId::parse, &name),
                                    &self.run,
                                    &d,
                                );
                            }
                            "release" => {
                                if let ConfirmState::Released(d) = next.state() {
                                    e.extend(rows("item_released", &name, d));
                                    sent = entries(d);
                                }
                                self.table = self.table.release(&conf_id(ItemId::parse, &name));
                            }
                            "lapse" => {
                                let cause =
                                    LapseCause::parse(a["cause"].as_str().unwrap()).unwrap();
                                let lives =
                                    !matches!(cause, LapseCause::RunEnded | LapseCause::AppEnded);
                                if matches!(
                                    cause,
                                    LapseCause::WaitLimit
                                        | LapseCause::Cancelled
                                        | LapseCause::AnswerLost
                                ) {
                                    self.table = self.table.stop(&self.run);
                                    self.state = "stopped";
                                    p.push(json!({"call": "cancel"}));
                                }
                                if lives && conf.state().waits_for_person() {
                                    let d = self.mint(&conf, EffectAnswer::RejectOnce);
                                    sent = entries(&d);
                                    p.push(json!({"call": "answer_confirm", "item": name, "answer": "reject"}));
                                }
                                if lives
                                    && cause != LapseCause::AnswerLost
                                    && matches!(conf.state(), ConfirmState::Answered(_))
                                {
                                    let d = self.mint(&conf, EffectAnswer::RejectOnce);
                                    sent = entries(&d);
                                }
                                if cause == LapseCause::NoRecord && a["stored"] == "yes" {
                                    e.extend(rows("item_answered", &name, &self.pending[&name]));
                                }
                                e.push(json!({"event": "item_lapsed", "item": name, "cause": cause.name()}));
                                self.table = self.table.lapse(&conf_id(ItemId::parse, &name));
                                if matches!(cause, LapseCause::RunEnded | LapseCause::AppEnded) {
                                    self.state = "ended";
                                }
                            }
                            _ => {}
                        }
                        self.items[i].1 = next;
                    }
                    Err(c) => {
                        r = c.name().into();
                        if mv == "answer" {
                            keys.insert("approvals".into(), json!([]));
                        }
                    }
                }
                if matches!(mv, "release" | "lapse") {
                    keys.insert("sent".into(), json!(sent));
                }
            }
            "stop" => {
                let phase = match (self.state, self.open_item()) {
                    ("live", None) => Some(halvo_coord::run::Phase::Working),
                    ("live", Some(item)) => Some(halvo_coord::run::Phase::Waiting(item)),
                    ("stopped", _) => Some(halvo_coord::run::Phase::Stopping(
                        halvo_coord::run::StopCause::Cancelled,
                    )),
                    _ => None,
                };
                let step = halvo_coord::run::run_step(
                    phase.as_ref(),
                    halvo_coord::run::RunMove::Stop(halvo_coord::run::StopCause::Cancelled),
                );
                let open = matches!(phase, Some(halvo_coord::run::Phase::Waiting(_)));
                if step.is_ok() && !open {
                    r = "ok".into();
                    self.table = self.table.stop(&self.run);
                    self.state = "stopped";
                    p.push(json!({"call": "cancel"}));
                } else {
                    r = "state".into();
                }
            }
            "guard" => {
                let record = EffectRecord {
                    run: self.run.clone(),
                    agent: self.agent.clone(),
                    effect: conf_id(EffectId::parse, a["effect"].as_str().unwrap()),
                    state: EffectState::parse(a["state"].as_str().unwrap()).unwrap(),
                    class: Class::parse(a["class"].as_str().unwrap()).unwrap(),
                    digest: digest_of(a["digest"].as_str().unwrap()),
                    approval: a["approval"]
                        .as_str()
                        .map(|x| conf_id(ApprovalId::parse, x)),
                };
                let gives = if a["policy"] == "run" {
                    Gives::Run
                } else {
                    Gives::Confirm
                };
                match guard_step(&self.table, &record, gives) {
                    Ok(t) => {
                        r = if record.state == EffectState::Approved && t == self.table {
                            "repeat".into()
                        } else {
                            "ok".into()
                        };
                        self.table = t;
                    }
                    Err(refusal) => r = refusal.name().into(),
                }
            }
            "bind" => {
                let ask = BindAsk {
                    run: self.run.clone(),
                    agent: self.agent.clone(),
                    origin: Origin::parse("https://pay.site1.example:8443").unwrap(),
                };
                let event = RunEvent::Bind(ask.clone());
                let mut item = Value::Null;
                match raise_step(&self.facts(), Source::of(&event).unwrap(), &self.limits) {
                    Raise::Run(RunItemKind::SignIn) => {
                        let lossy = Lossy {
                            inner: &self.ports,
                            lose: Cell::new(false),
                            unread: false,
                            read_fails: Cell::new(false),
                        };
                        match raise_bind(&lossy, &mut self.ledger, &self.table, &self.limits, &ask)
                        {
                            Ok(held) => {
                                let name = format!("b{}", self.binds.len() + 1);
                                self.raised.push(Event {
                                    workflow: conf_id(WorkflowId::parse, "w1"),
                                    agent: self.agent.clone(),
                                    seq: NonZeroU32::new(self.raised.len() as u32 + 1).unwrap(),
                                    day: DAY,
                                    at: 0,
                                    what: What::ItemRaised {
                                        item: held.item().clone(),
                                        raised: Raised::SignIn(self.run.clone()),
                                    },
                                });
                                item = json!(name);
                                self.binds.push((name, held));
                                r = "ok".into();
                            }
                            Err(c) => r = c.name().into(),
                        }
                    }
                    Raise::End(_) => {
                        self.state = "ended";
                        r = "state".into();
                    }
                    Raise::Refused(c) => r = c.name().into(),
                    other => r = format!("{other:?}"),
                }
                self.port_lines(&mut e, &mut p);
                keys.insert("item".into(), item);
            }
            "press" | "bind_lapse" => {
                let name = a["item"].as_str().unwrap().to_string();
                let Some(j) = self.binds.iter().position(|(n, _)| *n == name) else {
                    panic!("{at}: no item {name}");
                };
                self.ports.clear_refusals();
                self.answering = Some(name.clone());
                let lossy = Lossy {
                    inner: &self.ports,
                    lose: Cell::new(a["store"] == "lost"),
                    unread: false,
                    read_fails: Cell::new(false),
                };
                let answer = if m["m"] == "press" {
                    match a["runs"].as_str().unwrap() {
                        "busy" => self.ports.refuse(Method::AnswerBind, PortError::Busy),
                        "ended" => self.ports.refuse(
                            Method::AnswerBind,
                            PortError::Refused(Name::parse("ended").unwrap()),
                        ),
                        _ => {}
                    }
                    let choice = if a["choice"] == "allow" {
                        Choice::Allow
                    } else {
                        Choice::Decline
                    };
                    answer_bind(
                        &lossy,
                        &mut self.ledger,
                        &self.table,
                        &self.limits,
                        &mut self.binds[j].1,
                        choice,
                    )
                } else {
                    let cause = OtherLapse::parse(a["cause"].as_str().unwrap()).unwrap();
                    let lapsed = lapse_bind(
                        &lossy,
                        &mut self.ledger,
                        &mut self.table,
                        &mut self.binds[j].1,
                        cause,
                        &self.limits,
                    );
                    if lapsed.is_ok() {
                        self.state = match cause {
                            OtherLapse::WaitLimit | OtherLapse::Cancelled => "stopped",
                            _ => "ended",
                        };
                    }
                    lapsed
                };
                let ended = self.ports.tried.borrow()[self.tried_seen..]
                    .iter()
                    .any(|t| {
                        matches!(t.call, Call::AnswerBind(..))
                            && t.refused == Some(PortError::Refused(Name::parse("ended").unwrap()))
                    });
                if ended {
                    self.state = "ended";
                    self.ports.end_runner(&self.run);
                }
                self.ports.clear_refusals();
                r = match answer {
                    Ok(()) => "ok".into(),
                    Err(c) => c.name().into(),
                };
                self.port_lines(&mut e, &mut p);
                self.answering = None;
            }
            "kill" => {
                let events: Vec<Event> = self.ports.events.borrow().clone();
                let recovery = recover_step(
                    &Earlier {
                        agent: &self.agent,
                        events: &events,
                        last: None,
                    },
                    &[],
                );
                for (item, lapsed) in &recovery.lapsed {
                    let name = self.bind_name(item);
                    let Lapsed::SignIn(cause) = lapsed else {
                        panic!("{at}: a kill lapses no confirmation");
                    };
                    e.push(json!({"event": "item_lapsed", "item": name, "cause": cause.cause().name()}));
                    self.killed.push(name);
                }
                self.state = "ended";
                r = "ok".into();
            }
            other => panic!("{at}: no move {other}"),
        }
        out.check(at, "r", &m["r"], &json!(r));
        out.check(at, "e", &m["e"], &json!(e));
        out.check(at, "p", &m["p"], &json!(p));
        let mut items = Map::new();
        for (name, c) in &self.items {
            items.insert(name.clone(), json!(state_name(c.state())));
        }
        keys.insert("items".into(), Value::Object(items));
        let approvals = self.table.approvals();
        keys.insert(
            "table".into(),
            json!(approvals
                .iter()
                .map(|x| short(x.0.as_str()))
                .collect::<Vec<_>>()),
        );
        keys.insert(
            "used".into(),
            json!(approvals
                .iter()
                .filter(|x| x.2)
                .map(|x| short(x.0.as_str()))
                .collect::<Vec<_>>()),
        );
        keys.insert("run".into(), json!(self.state));
        let mut binds = Map::new();
        for (name, b) in &self.binds {
            binds.insert(name.clone(), json!(self.bind_state(name, b)));
        }
        keys.insert("binds".into(), Value::Object(binds));
        let want = m["s"].as_object().unwrap();
        let mut gk: Vec<&String> = keys.keys().collect();
        let mut wk: Vec<&String> = want.keys().collect();
        gk.sort();
        wk.sort();
        out.check(at, "keys of s", &json!(wk), &json!(gk));
        for (k, v) in want {
            out.check(
                at,
                &format!("s.{k}"),
                v,
                keys.get(k).unwrap_or(&Value::Null),
            );
        }
    }
}

fn replay_confirm(traces: &[Trace]) -> Vec<String> {
    let mut out = Mismatch(Vec::new());
    for t in traces {
        let mut c = Conf::new();
        let before = out.0.len();
        for (k, m) in t.moves.iter().enumerate() {
            let at = format!("confirm {} line {}", t.name(), k + 1);
            c.line(m, &at, &mut out);
            if out.0.len() > before {
                break;
            }
        }
    }
    out.0
}

#[test]
fn formal_traces() {
    let (schedule, confirm) = read_pair(SCHEDULE, CONFIRM).unwrap();
    assert!(schedule.len() >= 200 && confirm.len() >= 200);
    let mut faults = replay_schedule(&schedule);
    faults.extend(replay_confirm(&confirm));
    assert!(
        faults.is_empty(),
        "{} lines differ from the model:\n{}",
        faults.len(),
        faults.join("\n")
    );
}

#[test]
fn formal_traces_a_lost_switch_off_whose_read_answers_moves_as_the_switch_off_in_order() {
    let (schedule, _) = read_pair(SCHEDULE, CONFIRM).unwrap();
    let mut cases = 0;
    for t in &schedule {
        for (k, m) in t.moves.iter().enumerate() {
            if !(m["m"] == "off" && m["a"]["port"] == "lost") {
                continue;
            }
            let at = format!("schedule {} line {}", t.name(), k + 1);
            let lines: Vec<Value> = ["lost_read", "ok"]
                .iter()
                .map(|port| {
                    let mut s = Sched::new();
                    let mut prefix = Mismatch(Vec::new());
                    for before in &t.moves[..k] {
                        s.line(before, &at, &mut prefix);
                    }
                    assert!(prefix.0.is_empty(), "{at}: {:?}", prefix.0);
                    let mut mv = m.clone();
                    mv["a"]["port"] = json!(port);
                    s.line(&mv, &at, &mut Mismatch(Vec::new()))
                })
                .collect();
            assert_eq!(lines[0], lines[1], "{at}");
            assert_eq!(lines[0]["r"], json!("ok"), "{at}");
            cases += 1;
        }
    }
    assert!(cases > 0, "no trace holds a lost switch-off");
}

#[test]
fn formal_traces_refuses_a_pair_whose_stamps_differ() {
    let (stamp, _) = read_file(CONFIRM).unwrap();
    let other = CONFIRM.replacen(&stamp, &"0".repeat(64), 1);
    let refused = read_pair(SCHEDULE, &other);
    assert!(refused.is_err_and(|e| e.contains("two stamps")));
    assert!(read_pair(SCHEDULE, CONFIRM).is_ok());
}

const CONFIRM_MODEL: &str = include_str!("../../../formal/r4-coordinator/Confirm4.tla");
const SCHEDULE_MODEL: &str = include_str!("../../../formal/r4-coordinator/Schedule.tla");

/// The set that a line `<head>{"a", "b"}` of a model names, sorted.
fn model_set<'a>(model: &'a str, head: &str) -> Vec<&'a str> {
    let line = model
        .lines()
        .map(str::trim_start)
        .find(|l| l.starts_with(head))
        .unwrap_or_else(|| panic!("no line {head}"));
    let set = line[head.len()..].trim_end().trim_end_matches('}');
    let mut names: Vec<&str> = set.split(", ").map(|n| n.trim_matches('"')).collect();
    names.sort_unstable();
    names
}

#[test]
fn formal_traces_the_models_take_the_codes_cause_answer_lost() {
    let mut code: Vec<&str> = LapseCause::ALL.iter().map(|c| c.name()).collect();
    code.sort_unstable();
    assert_eq!(
        model_set(CONFIRM_MODEL, "Causes == {"),
        code,
        "Confirm4's Causes are the causes of a lapse of a confirmation (ruling RD-84 item 5)"
    );
    assert_eq!(
        model_set(SCHEDULE_MODEL, "/\\ StopCauses \\subseteq {"),
        ["answer_lost", "cancelled", "lapsed"],
        "Schedule's bound on the stop causes holds answer_lost (ruling RD-84 item 5)"
    );
}

/// The directed traces of the settle of a lost answer (I5.13, RD-84 items 2 and 4; the start
/// object's "a directed trace for the settle"): exported from this commit's models with the case
/// `answer_lost_settle` added to copies of ScheduleTrace.tla and Confirm4Trace.tla alone. The
/// schedule stops a waiting run with `answer_lost` and the run ends `failed`; the confirmation is
/// answered, lapses with `answer_lost` with one cancel and nothing sent, and the guard refuses
/// the allow that the lost move may have handed on.
const SETTLE_SCHEDULE: &str = r##"{"n":215,"case":"answer_lost_settle","moves":[{"m":"start","a":{"agent":"a4","trigger":"person","port":"ok"},"r":"ok","e":[{"agent":"a4","event":"run_started","run":"r1","slot":null,"trigger":"person"}],"p":[{"call":"defer"},{"call":"start_run","run":"r1"},{"call":"start","run":"r1"}],"s":{"run":"r1","day":1,"runs":{"r1":"starting"},"queue":[],"ready":[],"held":[],"starts":[0,0,0,1,0],"slot":[null,null,null,null,null],"fails":[0,0,0,0,0],"gone":[],"lost":[],"open":["r1"],"unknown":[],"cleared":[],"owing":[],"life":1}},{"m":"hello","a":{"run":"r1"},"r":"ok","e":[],"p":[],"s":{"day":1,"runs":{"r1":"working"},"queue":[],"ready":[],"held":[],"starts":[0,0,0,1,0],"slot":[null,null,null,null,null],"fails":[0,0,0,0,0],"gone":[],"lost":[],"open":["r1"],"unknown":[],"cleared":[],"owing":[],"life":1}},{"m":"wait","a":{"run":"r1"},"r":"ok","e":[],"p":[],"s":{"day":1,"runs":{"r1":"waiting"},"queue":[],"ready":[],"held":[],"starts":[0,0,0,1,0],"slot":[null,null,null,null,null],"fails":[0,0,0,0,0],"gone":[],"lost":[],"open":["r1"],"unknown":[],"cleared":[],"owing":[],"life":1}},{"m":"stop","a":{"run":"r1","cause":"answer_lost"},"r":"ok","e":[],"p":[{"call":"cancel","run":"r1"}],"s":{"day":1,"runs":{"r1":"stopping"},"queue":[],"ready":[],"held":[],"starts":[0,0,0,1,0],"slot":[null,null,null,null,null],"fails":[0,0,0,0,0],"gone":[],"lost":[],"open":["r1"],"unknown":[],"cleared":[],"owing":[],"life":1}},{"m":"end","a":{"run":"r1","frame":null,"port":"ok","left":"no"},"r":"ok","e":[{"agent":"a4","event":"run_ended","run":"r1","cause":"failed"}],"p":[],"s":{"cause":"failed","next":[],"day":1,"runs":{},"queue":[],"ready":[],"held":[],"starts":[0,0,0,1,0],"slot":[null,null,null,null,null],"fails":[0,0,0,1,0],"gone":[],"lost":[],"open":["r1"],"unknown":[],"cleared":[],"owing":[],"life":1}},{"m":"tick","a":{"day":1,"grace":"yes","port":"ok","left":"no"},"r":"ok","e":[{"agent":"a2","event":"run_started","run":"r2","slot":1,"trigger":"schedule"}],"p":[{"call":"kill","run":"r1"},{"call":"defer"},{"call":"start_run","run":"r2"},{"call":"start","run":"r2"}],"s":{"stopped":[],"ended":[],"next":[],"raised":["a1","a3"],"started":[{"agent":"a2","r":"ok","run":"r2"}],"day":1,"runs":{"r2":"starting"},"queue":[],"ready":["a1","a3"],"held":[],"starts":[0,1,0,1,0],"slot":[null,1,null,null,null],"fails":[0,0,0,1,0],"gone":[],"lost":[],"open":["r1","r2"],"unknown":[],"cleared":[],"owing":[],"life":1}},{"m":"start","a":{"agent":"a4","trigger":"person","port":"ok"},"r":"state","e":[],"p":[],"s":{"run":null,"day":1,"runs":{"r2":"starting"},"queue":[],"ready":["a1","a3"],"held":[],"starts":[0,1,0,1,0],"slot":[null,1,null,null,null],"fails":[0,0,0,1,0],"gone":[],"lost":[],"open":["r1","r2"],"unknown":[],"cleared":[],"owing":[],"life":1}}]}"##;
const SETTLE_CONFIRM: &str = r##"{"n":216,"case":"answer_lost_settle","moves":[{"m":"issue","a":{"effects":[{"effect":"e1","class":"send","digest":"d1"},{"effect":"e2","class":"payment","digest":"d2"}],"digest_ok":"yes"},"r":"ok","e":[{"event":"item_raised","item":"i1"}],"p":[],"s":{"item":"i1","sent":[],"items":{"i1":"shown"},"table":[],"used":[],"run":"live","binds":{}}},{"m":"arm","a":{"item":"i1"},"r":"ok","e":[],"p":[],"s":{"items":{"i1":"armed"},"table":[],"used":[],"run":"live","binds":{}}},{"m":"answer","a":{"item":"i1","answer":"allow","from":"native"},"r":"ok","e":[],"p":[{"call":"answer_confirm","item":"i1","answer":"allow"}],"s":{"approvals":["p1","p2"],"items":{"i1":"answered"},"table":[],"used":[],"run":"live","binds":{}}},{"m":"lapse","a":{"item":"i1","cause":"answer_lost","stored":null},"r":"ok","e":[{"event":"item_lapsed","item":"i1","cause":"answer_lost"}],"p":[{"call":"cancel"}],"s":{"sent":[],"items":{"i1":"lapsed"},"table":[],"used":[],"run":"stopped","binds":{}}},{"m":"guard","a":{"effect":"e1","class":"send","state":"approved","digest":"d1","approval":"p1","policy":null},"r":"approval","e":[],"p":[],"s":{"items":{"i1":"lapsed"},"table":[],"used":[],"run":"stopped","binds":{}}},{"m":"lapse","a":{"item":"i1","cause":"run_ended","stored":null},"r":"state","e":[],"p":[],"s":{"sent":[],"items":{"i1":"lapsed"},"table":[],"used":[],"run":"stopped","binds":{}}}]}"##;

fn directed(line: &str) -> Vec<Trace> {
    read_file(&format!("{{\"stamp\":\"directed\"}}\n{line}"))
        .unwrap()
        .1
}

#[test]
fn formal_traces_the_directed_settle_of_a_lost_answer_replays() {
    let schedule = directed(SETTLE_SCHEDULE);
    let confirm = directed(SETTLE_CONFIRM);
    let stop = schedule[0]
        .moves
        .iter()
        .position(|m| m["m"] == "stop" && m["a"]["cause"] == "answer_lost")
        .expect("the schedule trace stops with answer_lost");
    assert_eq!(schedule[0].moves[stop + 1]["m"], json!("end"));
    assert_eq!(
        schedule[0].moves[stop + 1]["e"][0]["cause"],
        json!("failed"),
        "the run's end is failed (RD-84 item 4)"
    );
    let lapse = confirm[0]
        .moves
        .iter()
        .find(|m| m["m"] == "lapse" && m["a"]["cause"] == "answer_lost")
        .expect("the confirm trace lapses with answer_lost");
    assert_eq!(
        lapse["p"],
        json!([{"call": "cancel"}]),
        "one cancel and no decision"
    );
    assert_eq!(lapse["s"]["sent"], json!([]), "nothing sent");
    let mut faults = replay_schedule(&schedule);
    faults.extend(replay_confirm(&confirm));
    assert!(
        faults.is_empty(),
        "{} lines differ from the model:\n{}",
        faults.len(),
        faults.join("\n")
    );
}
