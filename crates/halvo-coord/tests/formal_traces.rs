use halvo_coord::guard::{guard_step, EffectRecord, EffectState, Gives, Table};
use halvo_coord::items::{
    confirm_step, raise_step, ConfirmAnswer, ConfirmMove, ConfirmState, Confirmation, Raise, Source,
};
use halvo_coord::mem::{Call, Config, Method, Ports};
use halvo_coord::ports::{PortError, Reaped};
use halvo_coord::run::{delete_step, drive, recover, Act, StopBy};
use halvo_coord::schedule::{self, holds, Answer, Board, Ended, Served};
use halvo_coord::types::{
    Agent, AgentId, AgentState, Answered, ApprovalId, AskedEffect, Body, Category, Cause, Class,
    Cleared, ConfirmAsk, Decision, DecisionEntry, Digest, Doing, EffectAnswer, EffectId, Event,
    Facts, FullWorkflow, ItemId, LapseCause, Lapsed, Limits, Live, Name, Outcome, PastRun,
    Presenter, Raised, RunEnd, RunId, RunItem, RunItemKind, What, Workflow, WorkflowId,
    WorkflowState, WorkflowTrigger,
};
use serde_json::{json, Map, Value};
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
        What::RunEnded { run, cause } => json!({
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
    fn line(&mut self, m: &Value, at: &str, out: &mut Mismatch) {
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
                        Act::Stop {
                            run,
                            by: if a["cause"] == "lapsed" {
                                StopBy::WaitLimit
                            } else {
                                StopBy::Person
                            },
                        }
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
                    let unknown = u32::from(self.left.get(&run.to_string()) == Some(&true));
                    self.store_ends(n, &run, Outcome::Failed, unknown);
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
                let done = self.act(Act::SwitchOff {
                    agent: self.names.agents[n].clone(),
                });
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

/// One run of one agent with the confirmations and the guard's table, replayed through the
/// pure functions: `confirm_step` for the moves of an item, `raise_step` for whether a request
/// is admitted, `run_step` for the person's stop, `guard_step` for a record and the table's
/// own moves. The port mints approval ids from a counter, `p1` first, in the order of the
/// effects (I4.6).
struct Conf {
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
        Conf {
            items: Vec::new(),
            pending: BTreeMap::new(),
            table: Table::new(),
            run: conf_id(RunId::parse, "r1"),
            agent: conf_id(AgentId::parse, "a1"),
            state: "live",
            minted: 0,
            raised: Vec::new(),
            limits: limits(),
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

    fn open_item(&self) -> Option<RunItem> {
        self.items
            .iter()
            .find(|(_, c)| c.state().is_open())
            .map(|(name, _)| RunItem {
                item: conf_id(ItemId::parse, name),
                kind: RunItemKind::Confirm,
            })
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
                                if lives && conf.state().waits_for_person() {
                                    let d = self.mint(&conf, EffectAnswer::RejectOnce);
                                    sent = entries(&d);
                                    p.push(json!({"call": "answer_confirm", "item": name, "answer": "reject"}));
                                }
                                if lives && matches!(conf.state(), ConfirmState::Answered(_)) {
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
                                if matches!(cause, LapseCause::WaitLimit | LapseCause::Cancelled) {
                                    self.table = self.table.stop(&self.run);
                                    self.state = "stopped";
                                    p.push(json!({"call": "cancel"}));
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
fn formal_traces_refuses_a_pair_whose_stamps_differ() {
    let (stamp, _) = read_file(CONFIRM).unwrap();
    let other = CONFIRM.replacen(&stamp, &"0".repeat(64), 1);
    let refused = read_pair(SCHEDULE, &other);
    assert!(refused.is_err_and(|e| e.contains("two stamps")));
    assert!(read_pair(SCHEDULE, CONFIRM).is_ok());
}
