use halvo_coord::mem::{Call, Config, Method, Ports};
use halvo_coord::ports::{Owner, PortError, Runs};
use halvo_coord::schedule::{drive, Answer, Board, Member, Move, Report};
use halvo_coord::types::{
    Agent, AgentId, AgentState, Body, Category, CheckAnswer, Doing, Event, Facts, FullWorkflow,
    Limits, Name, Outcome, PastRun, RunEnd, RunId, Trigger, What, Workflow, WorkflowId,
    WorkflowState, WorkflowTrigger,
};
use std::collections::BTreeMap;
use std::num::NonZeroU32;

const DAY: u32 = 20_000;
const T0: u64 = 1_700_000_000_000;
const N: usize = 5;
const PERIOD: u32 = 2;

struct Draw(u64);

impl Draw {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

#[derive(Clone, Copy)]
struct A {
    person: bool,
    model: bool,
    slotted: bool,
    finished: bool,
}

const SPEC: [A; N] = [
    A {
        person: true,
        model: true,
        slotted: true,
        finished: true,
    },
    A {
        person: false,
        model: true,
        slotted: true,
        finished: true,
    },
    A {
        person: false,
        model: false,
        slotted: true,
        finished: false,
    },
    A {
        person: true,
        model: false,
        slotted: false,
        finished: true,
    },
    A {
        person: false,
        model: false,
        slotted: false,
        finished: true,
    },
];

fn agent(n: usize) -> AgentId {
    AgentId::parse(&format!("{:032x}", 0xa0 + n)).unwrap()
}

fn flow(n: usize) -> WorkflowId {
    WorkflowId::parse(&format!("{:032x}", 0xf0 + n)).unwrap()
}

fn index(id: &AgentId) -> usize {
    (0..N).find(|n| agent(*n) == *id).unwrap()
}

fn config(n: usize) -> Config {
    let a = SPEC[n];
    Config::parse(&format!(
        "{{\"v\":1,\"workflow\":\"{}\",\"agent\":\"{}\",\"rev\":1,\"category\":\"calendar_cleanup\",\"tier\":\"on_device\",\"reasoning\":\"medium\",\"autonomy\":\"draft_and_confirm\",\"allow\":[],\"caps\":{{\"untrusted\":\"yes\",\"private\":\"yes\",\"external\":\"{}\"}},\"limits\":{{\"steps\":20,\"wall_s\":120,\"model_tokens\":{}}},\"budget\":null,\"permissions\":[],\"runtime\":\"graph_calendar\",\"playbook\":1}}",
        flow(n),
        agent(n),
        if a.person { "yes" } else { "no" },
        if a.model { 1000 } else { 0 },
    ))
    .unwrap()
}

fn workflow(n: usize) -> FullWorkflow {
    FullWorkflow {
        id: flow(n),
        state: WorkflowState::Active,
        category: Category::CalendarCleanup,
        site: None,
        trigger: if SPEC[n].slotted {
            WorkflowTrigger::Schedule
        } else {
            WorkflowTrigger::Manual
        },
        period: NonZeroU32::new(PERIOD),
        last: DAY - 2,
        agent: Some(agent(n)),
    }
}

fn current(n: usize, day: u32) -> Option<u32> {
    let last = DAY - 2;
    (SPEC[n].slotted && day >= last + PERIOD).then(|| last + (day - last) / PERIOD * PERIOD)
}

#[derive(Clone)]
struct HRun {
    run: RunId,
    agent: usize,
    stop: Option<(u64, RunEnd)>,
    stopped_before_tick: bool,
}

struct Hist {
    live: Vec<HRun>,
    waiting: [Option<Trigger>; N],
    off: [bool; N],
    failures: [bool; N],
    streak: [usize; N],
    finished: [bool; N],
    served: [Vec<u32>; N],
    given: [Vec<u32>; N],
    skipped: [Vec<u32>; N],
    checked: Vec<RunId>,
    gone: [bool; N],
    acked: Vec<RunId>,
    started: Vec<RunId>,
    run_slot: BTreeMap<String, Option<u32>>,
    run_agent: BTreeMap<String, usize>,
    pending: BTreeMap<String, RunEnd>,
    count: BTreeMap<(usize, u32), u32>,
    day: u32,
    unended: [Option<RunId>; N],
    store_due: [Option<PastRun>; N],
    owed_ends: BTreeMap<String, RunEnd>,
    no_runner: Vec<RunId>,
}

struct Sim {
    ports: Ports<Config>,
    board: Board<Config>,
    limits: Limits,
    store_runs: [Vec<PastRun>; N],
    h: Hist,
    calls_seen: usize,
    events_seen: usize,
    now: u64,
    deletes: usize,
    faults: Vec<String>,
    seen: BTreeMap<String, u32>,
    refused_log_now: bool,
    refused_send_now: bool,
}

fn member(sim: &Sim, n: usize) -> Member<Config> {
    let events: Vec<Event> = sim
        .ports
        .events
        .borrow()
        .iter()
        .filter(|e| e.agent == agent(n))
        .cloned()
        .collect();
    let newest = events.iter().rev().find_map(|e| match &e.what {
        What::RunStarted { run, .. } => Some(run.clone()),
        _ => None,
    });
    let last = sim
        .ports
        .agents
        .borrow()
        .iter()
        .find(|a| a.agent == agent(n))
        .and_then(|a| a.last.clone());
    Member {
        facts: Facts {
            agent: agent(n),
            workflow: workflow(n),
            config: config(n),
            slots: Vec::<Name>::new(),
            runs: sim.store_runs[n].clone(),
            store_ended: newest.is_none_or(|r| last.is_some_and(|l| l.run == r)),
            events,
            today: sim.h.day,
            doing: Doing::Idle(None),
        },
        session: true,
    }
}

impl Sim {
    fn new(limits: Limits) -> Sim {
        let ports = Ports::with_limits(T0, DAY, limits);
        let mut h = Hist {
            live: vec![],
            waiting: [None; N],
            off: [false; N],
            failures: [false; N],
            streak: [0; N],
            finished: [false; N],
            served: Default::default(),
            given: Default::default(),
            skipped: Default::default(),
            checked: vec![],
            gone: [false; N],
            acked: vec![],
            started: vec![],
            run_slot: BTreeMap::new(),
            run_agent: BTreeMap::new(),
            pending: BTreeMap::new(),
            count: BTreeMap::new(),
            day: DAY,
            unended: Default::default(),
            store_due: Default::default(),
            owed_ends: BTreeMap::new(),
            no_runner: Vec::new(),
        };
        for (n, spec) in SPEC.iter().enumerate() {
            ports
                .workflows
                .borrow_mut()
                .push(Workflow::Full(workflow(n)));
            ports.agents.borrow_mut().push(Agent {
                workflow: flow(n),
                agent: agent(n),
                config: config(n),
                state: AgentState::Active,
                slots: vec![],
                session: true,
                last: spec.finished.then(|| PastRun {
                    run: RunId::parse(&format!("{:032x}", 0x900 + n)).unwrap(),
                    day: DAY - 2,
                    outcome: Outcome::Completed,
                    unknown: 0,
                }),
                unknown: vec![],
            });
            if spec.finished {
                let run = RunId::parse(&format!("{:032x}", 0x900 + n)).unwrap();
                let e = |seq: u32, what: What| Event {
                    workflow: flow(n),
                    agent: agent(n),
                    seq: NonZeroU32::new(seq).unwrap(),
                    day: DAY - 2,
                    at: 0,
                    what,
                };
                ports
                    .log(&[
                        e(
                            1,
                            What::RunStarted {
                                run: run.clone(),
                                trigger: Trigger::Person,
                                slot: None,
                            },
                        ),
                        e(
                            2,
                            What::RunEnded {
                                run,
                                cause: RunEnd::Completed,
                            },
                        ),
                    ])
                    .unwrap();
                h.finished[n] = true;
                h.served[n].push(DAY - 2);
                h.unended[n] = None;
            }
        }
        let calls_seen = ports.calls.borrow().len();
        let events_seen = ports.events.borrow().len();
        let mut sim = Sim {
            ports,
            board: Board::new(DAY, vec![]),
            limits,
            store_runs: Default::default(),
            h,
            calls_seen,
            events_seen,
            now: T0,
            deletes: 0,
            faults: vec![],
            seen: BTreeMap::new(),
            refused_log_now: false,
            refused_send_now: false,
        };
        let members = (0..N).map(|n| member(&sim, n)).collect();
        sim.board = Board::new(DAY, members);
        sim
    }

    fn fault(&mut self, what: String) {
        self.faults.push(what);
    }

    fn tally(&mut self, key: String) {
        *self.seen.entry(key).or_insert(0) += 1;
    }

    fn open(&self, n: usize, slot: u32) -> bool {
        let from = slot.saturating_sub(PERIOD / 2);
        !self.h.served[n].iter().any(|d| *d >= from) && !self.h.skipped[n].contains(&slot)
    }

    fn holds_by_history(&self, n: usize) -> bool {
        let check = self.h.unended[n].is_none()
            && self.store_runs[n]
                .first()
                .is_some_and(|r| r.unknown > 0 && !self.h.checked.contains(&r.run));
        self.h.off[n] || self.h.failures[n] || check
    }

    fn listed(&self, n: usize) -> bool {
        let id = flow(n);
        self.ports.workflows.borrow().iter().any(|w| w.id() == &id)
    }

    fn step(&mut self, mv: Move, label: &str) {
        let direct = match &mv {
            Move::Start { agent } => Some(index(agent)),
            _ => None,
        };
        let tick = matches!(mv, Move::Tick);
        match &mv {
            Move::Start { agent } => {
                let n = index(agent);
                if self.listed(n) {
                    self.h.failures[n] = false;
                }
            }
            Move::SwitchOn { agent } => {
                let n = index(agent);
                self.h.off[n] = false;
                self.h.failures[n] = false;
            }
            Move::Tick => {
                self.h.day = halvo_coord::ports::Clock::today(&self.ports).unwrap();
                let listed: Vec<bool> = (0..N).map(|n| self.listed(n)).collect();
                for r in &mut self.h.live {
                    r.stopped_before_tick = r.stop.is_some();
                    if !listed[r.agent] {
                        if let Some(stop) = &mut r.stop {
                            stop.1 = RunEnd::Deleted;
                        }
                    }
                }
            }
            Move::End { run, outcome } => {
                let Some(k) = self.h.live.iter().position(|r| &r.run == run) else {
                    self.fault(format!("{label}: end of a run that is not live"));
                    return;
                };
                let r = self.h.live.remove(k);
                let cause = match (r.stop, outcome) {
                    (Some((_, c)), _) => c,
                    (None, Some(o)) => RunEnd::from(*o),
                    (None, None) => RunEnd::Interrupted,
                };
                self.h.pending.insert(run.to_string(), cause);
                self.h.store_due[r.agent] = Some(PastRun {
                    run: run.clone(),
                    day: self.h.day,
                    outcome: outcome.unwrap_or(Outcome::Failed),
                    unknown: u32::from(*outcome == Some(Outcome::EffectUnknown)),
                });
            }
            _ => {}
        }
        let checked_run = match &mv {
            Move::Check { agent, .. } => {
                self.store_runs[index(agent)].first().map(|r| r.run.clone())
            }
            _ => None,
        };
        let skip_agent = match &mv {
            Move::Skip { agent } => Some(index(agent)),
            _ => None,
        };
        let went_through = matches!(
            &mv,
            Move::Check {
                answer: CheckAnswer::WentThrough,
                ..
            }
        );
        let report = drive(&self.ports, &mut self.board, &self.limits, mv.clone());
        self.tally(format!("answer {:?}", report.answer));
        for e in &report.ended {
            self.tally(format!("ended {}", e.cause));
        }
        if let Move::End { .. } = mv {
            if report.answer != Ok(Answer::Ok) {
                self.fault(format!(
                    "{label}: the coordinator refused the end of a live run"
                ));
            }
        }
        if report.answer == Ok(Answer::Ok) {
            if let Move::SwitchOff { agent } = &mv {
                let n = index(agent);
                self.h.off[n] = true;
                if self.h.waiting[n].take().is_some() {
                    self.tally("dropped by the switch-off".into());
                }
            }
            if let (Some(run), true) = (checked_run, matches!(mv, Move::Check { .. })) {
                if went_through {
                    let n = self.h.run_agent.get(run.as_str()).copied();
                    if let Some(Some(slot)) = self.h.run_slot.get(run.as_str()) {
                        if let Some(n) = n {
                            self.h.skipped[n].push(*slot);
                        }
                    }
                }
                self.h.checked.push(run);
            }
            if let Some(n) = skip_agent {
                if let Some(slot) = current(n, self.h.day) {
                    if self.h.finished[n] && self.open(n, slot) {
                        self.h.skipped[n].push(slot);
                    }
                }
            }
        }
        let refused_log = self.read_calls(direct, tick, label);
        self.queue_answers(&report, direct);
        if tick {
            self.after_tick(label);
        }
        for (run, cause) in std::mem::take(&mut self.h.pending) {
            let n = self.h.run_agent.get(&run).copied();
            if let Some(n) = n {
                if self.listed(n) {
                    if refused_log {
                        self.h.owed_ends.insert(run, cause);
                    } else {
                        self.fault(format!("{label}: no run_ended stored for {run}"));
                    }
                }
            }
        }
        if tick && !self.refused_send_now {
            let owed: Vec<RunId> = self
                .h
                .no_runner
                .iter()
                .filter(|r| {
                    self.h
                        .run_agent
                        .get(r.as_str())
                        .is_some_and(|n| self.listed(*n))
                })
                .cloned()
                .collect();
            if !owed.is_empty() {
                self.fault(format!(
                    "{label}: XB-14 an owed end_run was not sent: {owed:?}"
                ));
            }
        }
        if tick && !refused_log {
            let owed: Vec<String> = self
                .h
                .owed_ends
                .keys()
                .filter(|r| {
                    self.h
                        .run_agent
                        .get(r.as_str())
                        .is_some_and(|n| self.listed(*n))
                })
                .cloned()
                .collect();
            if !owed.is_empty() {
                self.fault(format!(
                    "{label}: XB-17 an owed run_ended was not logged: {owed:?}"
                ));
            }
        }
        self.properties(label);
    }

    fn store_end(&mut self, n: usize, left: bool) -> bool {
        let Some(mut run) = self.h.store_due[n].take() else {
            return false;
        };
        if left && run.outcome != Outcome::Completed {
            run.unknown = 1;
        }
        self.ports.store_end(&agent(n), run.clone());
        if self.h.unended[n].as_ref() == Some(&run.run) {
            self.h.unended[n] = None;
        }
        self.store_runs[n].insert(0, run);
        self.tally("store_end".into());
        true
    }

    fn queue_answers(&mut self, report: &Report, direct: Option<usize>) {
        if let (Some(n), Ok(Answer::Queued)) = (direct, report.answer) {
            self.h.waiting[n] = Some(Trigger::Person);
            self.tally("queued person".into());
        }
        for s in &report.started {
            if s.answer == Ok(Answer::Queued) {
                self.h.waiting[index(&s.agent)] = Some(Trigger::Schedule);
                self.tally("queued schedule".into());
            }
        }
        let nexts = report
            .next
            .iter()
            .chain(report.ended.iter().flat_map(|e| e.next.iter()));
        for s in nexts {
            if s.answer != Ok(Answer::Queued) {
                self.h.waiting[index(&s.agent)] = None;
                self.tally(format!("left {:?}", s.answer));
            }
        }
        for a in &report.dropped {
            self.h.waiting[index(a)] = None;
            self.tally("dropped".into());
        }
    }

    fn read_calls(&mut self, direct: Option<usize>, tick: bool, label: &str) -> bool {
        let calls: Vec<_> = self.ports.calls.borrow()[self.calls_seen..].to_vec();
        self.calls_seen += calls.len();
        let stored = self.ports.events.borrow().len();
        let mut person_starts = 0;
        let refused_log = self.refused_log_now;
        for t in calls {
            match t.call {
                Call::Log(events) => {
                    for e in events {
                        self.read_event(&e, direct, &mut person_starts, label);
                    }
                }
                Call::Start(spec) => {
                    if !self.h.acked.contains(&spec.run) {
                        self.fault(format!(
                            "{label}: C4 a run began with no acknowledged run_started"
                        ));
                    }
                    let n = index(&spec.agent);
                    self.h.started.push(spec.run.clone());
                    self.h.live.push(HRun {
                        run: spec.run,
                        agent: n,
                        stop: None,
                        stopped_before_tick: false,
                    });
                    self.tally("start".into());
                }
                Call::Cancel(run) => {
                    let cause = if tick {
                        RunEnd::Deleted
                    } else {
                        RunEnd::Cancelled
                    };
                    if let Some(r) = self.h.live.iter_mut().find(|r| r.run == run) {
                        if r.stop.is_none() {
                            r.stop = Some((t.at, cause));
                        }
                    }
                }
                Call::Send(Body::EndRun { run }) => {
                    self.tally("end_run".into());
                    match self.h.no_runner.iter().position(|r| *r == run) {
                        Some(k) => {
                            self.h.no_runner.remove(k);
                        }
                        None => self.faults.push(format!(
                            "{label}: C12 an end_run for a run that is not owed: {run}"
                        )),
                    }
                    if let Some(n) = self.h.run_agent.get(run.as_str()).copied() {
                        if self.h.unended[n].as_ref() == Some(&run) {
                            self.h.unended[n] = None;
                        }
                        self.store_runs[n].insert(
                            0,
                            PastRun {
                                run: run.clone(),
                                day: self.h.day,
                                outcome: Outcome::Failed,
                                unknown: 0,
                            },
                        );
                    }
                }
                Call::Kill(run) => {
                    self.tally("kill".into());
                    if let Some(k) = self.h.live.iter().position(|r| r.run == run) {
                        let r = self.h.live.remove(k);
                        let grace = self.limits.cancel_grace_s * 1000;
                        if r.stop.is_none_or(|(at, _)| t.at.saturating_sub(at) < grace) {
                            self.faults
                                .push(format!("{label}: C9 a kill before the grace of its stop"));
                        }
                        self.h.store_due[r.agent] = Some(PastRun {
                            run: run.clone(),
                            day: self.h.day,
                            outcome: Outcome::Failed,
                            unknown: 0,
                        });
                        if let Some((_, cause)) = r.stop {
                            self.h.pending.insert(run.to_string(), cause);
                        }
                    }
                }
                _ => {}
            }
        }
        self.events_seen = stored;
        let pending_failed: Vec<RunId> = self
            .h
            .acked
            .iter()
            .filter(|r| !self.h.started.contains(r))
            .cloned()
            .collect();
        for r in pending_failed {
            if !refused_log {
                self.fault(format!(
                    "{label}: C10 a start that failed after step 6 stored no end"
                ));
            }
            self.h.ended_mark(&r);
        }
        refused_log
    }

    fn read_event(
        &mut self,
        e: &Event,
        direct: Option<usize>,
        person_starts: &mut u32,
        label: &str,
    ) {
        let n = index(&e.agent);
        match &e.what {
            What::RunStarted { run, .. } => {
                self.tally("run_started".into());
                if !self.listed(n) {
                    self.fault(format!("{label}: C8 a start for a workflow that is gone"));
                }
                if self.holds_by_history(n) {
                    self.fault(format!("{label}: C4 a start while a hold stands"));
                }
                if self
                    .h
                    .owed_ends
                    .keys()
                    .any(|r| self.h.run_agent.get(r.as_str()) == Some(&n))
                {
                    self.fault(format!(
                        "{label}: XB-17 a start while a run_ended of the agent is owed"
                    ));
                }
                if self.h.unended[n].is_some() {
                    self.fault(format!(
                        "{label}: C4 a start while the agent store has not ended the newest run"
                    ));
                }
                self.h.unended[n] = Some(run.clone());
                if self.h.live.iter().any(|r| r.agent == n) {
                    self.fault(format!("{label}: C3 a second live run"));
                }
                let by_person = if direct == Some(n) {
                    *person_starts += 1;
                    true
                } else {
                    self.h.waiting[n].take() == Some(Trigger::Person)
                };
                if *person_starts > 1 {
                    self.fault(format!(
                        "{label}: C5 one move of the person led to two starts"
                    ));
                }
                let slot_now = current(n, self.h.day);
                let given = if by_person {
                    slot_now.filter(|s| self.h.finished[n] && self.open(n, *s))
                } else {
                    let due = slot_now.filter(|s| {
                        self.open(n, *s)
                            && self.h.day - *s <= self.limits.catch_up_days
                            && !self.h.given[n].contains(s)
                    });
                    if SPEC[n].person || !self.h.finished[n] {
                        self.fault(format!("{label}: C5 a start with no move of the person"));
                    }
                    if due.is_none() {
                        self.fault(format!(
                            "{label}: C6/C11 a start of the scheduler for no due slot"
                        ));
                    }
                    slot_now
                };
                if let Some(s) = given {
                    self.h.given[n].push(s);
                }
                let c = {
                    let c = self.h.count.entry((n, e.day)).or_insert(0);
                    *c += 1;
                    *c
                };
                if c > self.limits.starts_per_day {
                    self.fault(format!("{label}: C7 more than starts_per_day starts"));
                }
                if e.day != self.h.day {
                    self.fault(format!("{label}: a start stored with another day"));
                }
                self.h.acked.push(run.clone());
                self.h.run_slot.insert(run.to_string(), given);
                self.h.run_agent.insert(run.to_string(), n);
            }
            What::RunEnded { run, cause } => {
                self.tally(format!("run_ended {cause}"));
                if !self.listed(n) {
                    self.fault(format!(
                        "{label}: C10 a run_ended for a workflow that is gone"
                    ));
                }
                let failed_start = self.h.acked.contains(run) && !self.h.started.contains(run);
                let expected = if let Some(c) = self.h.pending.remove(run.as_str()) {
                    Some(c)
                } else if let Some(c) = self.h.owed_ends.remove(run.as_str()) {
                    self.tally("owed run_ended logged".into());
                    Some(c)
                } else if failed_start {
                    self.h.no_runner.push(run.clone());
                    Some(RunEnd::Failed)
                } else {
                    None
                };
                match expected {
                    Some(c) if c == *cause => {}
                    Some(c) => self.fault(format!("{label}: C10 stored {cause} for an end of {c}")),
                    None => self.fault(format!(
                        "{label}: C10 a run_ended for a run that did not end"
                    )),
                }
                self.h.ended_mark(run);
                if cause.failure().is_some() {
                    self.h.streak[n] += 1;
                    if self.h.streak[n] >= self.limits.fails_to_pause as usize {
                        self.h.failures[n] = true;
                    }
                } else {
                    self.h.streak[n] = 0;
                }
                if *cause == RunEnd::Completed {
                    self.h.finished[n] = true;
                }
                if matches!(cause, RunEnd::Completed | RunEnd::NothingToDo) {
                    let day = self.start_day(run);
                    self.h.served[n].push(day);
                }
            }
            _ => {}
        }
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
            .unwrap()
    }

    fn after_tick(&mut self, label: &str) {
        let grace = self.limits.cancel_grace_s * 1000;
        for r in &self.h.live {
            if !self.listed(r.agent) && r.stop.is_none() {
                self.faults.push(format!(
                    "{label}: C8 a run of a gone workflow is not stopping after a tick"
                ));
            }
            if let (Some((at, _)), true) = (r.stop, r.stopped_before_tick) {
                if self.now.saturating_sub(at) >= grace {
                    self.faults
                        .push(format!("{label}: C9 a stopped run is live after its grace"));
                }
            }
        }
    }

    fn properties(&mut self, label: &str) {
        for n in 0..N {
            let queued = self.board.queue.holds(&agent(n));
            if queued != self.h.waiting[n].is_some() {
                self.fault(format!(
                    "{label}: C3 the queue holds agent {n}: {queued}, the answers say {:?}",
                    self.h.waiting[n]
                ));
            }
        }
        let live = self.h.live.len() as u32;
        let models = self.h.live.iter().filter(|r| SPEC[r.agent].model).count();
        if live > self.limits.max_live_runs || models > 1 {
            self.fault(format!("{label}: C1 live={live} model={models}"));
        }
        for n in 0..N {
            let mine = self.h.live.iter().filter(|r| r.agent == n).count()
                + usize::from(self.h.waiting[n].is_some());
            if mine > 1 {
                self.fault(format!(
                    "{label}: C3 agent {n} has {mine} live or queued runs"
                ));
            }
        }
    }

    fn restart(&mut self, day: u32) {
        self.ports.set_clock(self.now, day);
        self.h.day = day;
        let _ = self.ports.reap();
        for run in std::mem::take(&mut self.h.no_runner) {
            let Some(n) = self.h.run_agent.get(run.as_str()).copied() else {
                continue;
            };
            if self.listed(n) && self.ports.send(&Body::EndRun { run: run.clone() }).is_ok() {
                if self.h.unended[n].as_ref() == Some(&run) {
                    self.h.unended[n] = None;
                }
                self.store_runs[n].insert(
                    0,
                    PastRun {
                        run,
                        day,
                        outcome: Outcome::Failed,
                        unknown: 0,
                    },
                );
            }
        }
        self.h.owed_ends.clear();
        let events = self.ports.events.borrow().clone();
        for n in 0..N {
            if !self.listed(n) {
                continue;
            }
            let open: Vec<RunId> = events
                .iter()
                .filter(|e| e.agent == agent(n))
                .filter_map(|e| match &e.what {
                    What::RunStarted { run, .. } => Some(run.clone()),
                    _ => None,
                })
                .filter(|r| {
                    !events
                        .iter()
                        .any(|e| matches!(&e.what, What::RunEnded { run, .. } if run == r))
                })
                .collect();
            for run in open {
                let last = self
                    .ports
                    .events
                    .borrow()
                    .iter()
                    .filter(|e| e.agent == agent(n))
                    .map(|e| e.seq.get())
                    .max()
                    .unwrap();
                self.ports
                    .log(&[Event {
                        workflow: flow(n),
                        agent: agent(n),
                        seq: NonZeroU32::new(last + 1).unwrap(),
                        day,
                        at: 0,
                        what: What::RunEnded {
                            run: run.clone(),
                            cause: RunEnd::Interrupted,
                        },
                    }])
                    .unwrap();
                self.h.streak[n] = 0;
                self.h.store_due[n] = Some(PastRun {
                    run: run.clone(),
                    day,
                    outcome: Outcome::Failed,
                    unknown: 0,
                });
                self.h.ended_mark(&run);
            }
        }
        self.h.live.clear();
        self.h.waiting = [None; N];
        self.calls_seen = self.ports.calls.borrow().len();
        self.events_seen = self.ports.events.borrow().len();
        let members = (0..N).map(|n| member(self, n)).collect();
        self.board = Board::new(day, members);
    }
}

impl Hist {
    fn ended_mark(&mut self, run: &RunId) {
        self.acked.retain(|r| r != run);
        self.started.retain(|r| r != run);
    }
}

fn sequence(seed: u64, limits: Limits, moves: usize, seen: &mut BTreeMap<String, u32>) {
    let mut d = Draw(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    let mut sim = Sim::new(limits);
    let mut log: Vec<String> = Vec::new();
    for k in 0..moves {
        let refusal = match d.below(14) {
            0 => Some((Method::Log, PortError::NoService)),
            1 => Some((
                Method::Send,
                PortError::Refused(Name::parse("state").unwrap()),
            )),
            2 => Some((Method::Start, PortError::NoService)),
            _ => None,
        };
        let pick = d.below(100);
        let a = d.below(N);
        let mv = if pick < 30 {
            let day_step = u32::from(d.below(3) == 0);
            let ms = [0u64, 1_000, 3_000, 5_000, 6_000][d.below(5)];
            sim.now += ms;
            let day = halvo_coord::ports::Clock::today(&sim.ports).unwrap() + day_step;
            sim.ports.set_clock(sim.now, day);
            Move::Tick
        } else if pick < 52 {
            Move::Start { agent: agent(a) }
        } else if pick < 70 && !sim.h.live.is_empty() {
            let r = sim.h.live[d.below(sim.h.live.len())].run.clone();
            let outcome = match d.below(11) {
                0 | 1 => None,
                k => Some(Outcome::ALL[(k - 2) % Outcome::ALL.len()]),
            };
            Move::End { run: r, outcome }
        } else if pick < 76 {
            Move::SwitchOff { agent: agent(a) }
        } else if pick < 82 {
            Move::SwitchOn { agent: agent(a) }
        } else if pick < 87 {
            Move::Skip { agent: agent(a) }
        } else if pick < 92 {
            let answer = if d.below(2) == 0 {
                CheckAnswer::TryAgain
            } else {
                CheckAnswer::WentThrough
            };
            Move::Check {
                agent: agent(a),
                answer,
            }
        } else if pick < 94 && sim.deletes < 2 && sim.listed(a) {
            sim.deletes += 1;
            sim.ports
                .workflows
                .borrow_mut()
                .retain(|w| w.id() != &flow(a));
            sim.h.gone[a] = true;
            log.push(format!("{k}: delete a{a}"));
            continue;
        } else if pick < 97 {
            let left = d.below(4) == 0;
            if sim.store_end(a, left) {
                log.push(format!("{k}: store ends the run of a{a} left={left}"));
            }
            continue;
        } else if pick < 99 {
            let day =
                halvo_coord::ports::Clock::today(&sim.ports).unwrap() + u32::from(d.below(2) == 0);
            log.push(format!("{k}: restart day+{}", day - DAY));
            sim.restart(day);
            continue;
        } else {
            Move::Tick
        };
        let label = format!("seed {seed} move {k} {mv:?} refusal {refusal:?}");
        log.push(label.clone());
        let refused_log = matches!(refusal, Some((Method::Log, _)));
        sim.refused_send_now = matches!(refusal, Some((Method::Send, _)));
        if let Some((m, e)) = refusal {
            sim.ports.refuse(m, e);
        }
        sim.step_with(mv, &label, refused_log);
        sim.ports.clear_refusals();
        if !sim.faults.is_empty() {
            panic!("{}\nmoves:\n{}", sim.faults.join("\n"), log.join("\n"));
        }
    }
    for (k, v) in sim.seen {
        *seen.entry(k).or_insert(0) += v;
    }
}

impl Sim {
    fn step_with(&mut self, mv: Move, label: &str, refused_log: bool) {
        self.refused_log_now = refused_log;
        self.step(mv, label);
    }
}

fn run_all(limits: Limits, seeds: u64) -> BTreeMap<String, u32> {
    let mut seen = BTreeMap::new();
    for seed in 1..=seeds {
        sequence(seed, limits, 40, &mut seen);
    }
    seen
}

const COVERED: [&str; 15] = [
    "answer Ok(Ok)",
    "answer Ok(Queued)",
    "answer Err(Hold)",
    "answer Err(State)",
    "answer Err(NoService)",
    "kill",
    "dropped",
    "queued person",
    "queued schedule",
    "run_ended cancelled",
    "ended deleted",
    "run_ended failed",
    "dropped by the switch-off",
    "end_run",
    "owed run_ended logged",
];

fn covered(seen: &BTreeMap<String, u32>, keys: &[&str]) {
    let missing: Vec<_> = keys.iter().filter(|k| !seen.contains_key(**k)).collect();
    assert!(
        missing.is_empty(),
        "never drawn: {missing:?}; drawn: {seen:?}"
    );
    eprintln!("drawn: {seen:?}");
}

#[test]
fn drawn_sequences_keep_the_schedule_properties_with_the_limits_of_the_models() {
    let limits =
        Limits::from_json(include_str!("../../../formal/r4-coordinator/limits.json").trim_end())
            .unwrap();
    let seen = run_all(limits, 400);
    covered(&seen, &COVERED);
    covered(
        &seen,
        &[
            "answer Err(Budget)",
            "answer Err(QueueFull)",
            "left Err(State)",
        ],
    );
}

#[test]
fn drawn_sequences_keep_the_schedule_properties_with_the_products_limits() {
    let seen = run_all(Limits::PRODUCT, 400);
    covered(&seen, &COVERED);
}
