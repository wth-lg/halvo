use halvo_coord::mem::{Call, Config, Method, Ports};
use halvo_coord::ports::{Owner, PortError};
use halvo_coord::schedule::{
    drive, holds, start_step, tick_step, Answer, Around, Ask, Board, Hold, Member, Move, Report,
    Served, Verdict,
};
use halvo_coord::types::{
    Agent, AgentId, AgentState, Answered, Body, Category, Cause, CheckAnswer, Cleared, Doing,
    Event, Facts, FullWorkflow, HoldSetCause, Limits, Mode, Name, Outcome, OwnItemKind, PastRun,
    Raised, ReadyAnswer, RunEnd, RunId, Trigger, What, Workflow, WorkflowId, WorkflowState,
    WorkflowTrigger,
};
use std::num::NonZeroU32;

const DAY: u32 = 20_000;
const T0: u64 = 1_700_000_000_000;

#[derive(Clone, Copy)]
struct A {
    person: bool,
    model: bool,
    slotted: bool,
    finished: bool,
    runtime: &'static str,
    playbook: bool,
    category: Category,
    slots: &'static [&'static str],
}

const AUTO: A = A {
    person: false,
    model: false,
    slotted: true,
    finished: true,
    runtime: "graph_calendar",
    playbook: true,
    category: Category::CalendarCleanup,
    slots: &[],
};

fn agent(n: usize) -> AgentId {
    AgentId::parse(&format!("{:032x}", 0xa0 + n)).unwrap()
}

fn flow(n: usize) -> WorkflowId {
    WorkflowId::parse(&format!("{:032x}", 0xf0 + n)).unwrap()
}

fn config(n: usize, a: &A) -> Config {
    Config::parse(&format!(
        "{{\"v\":1,\"workflow\":\"{}\",\"agent\":\"{}\",\"rev\":1,\"category\":\"{}\",\"tier\":\"on_device\",\"reasoning\":\"medium\",\"autonomy\":\"draft_and_confirm\",\"allow\":[],\"caps\":{{\"untrusted\":\"yes\",\"private\":\"yes\",\"external\":\"{}\"}},\"limits\":{{\"steps\":20,\"wall_s\":120,\"model_tokens\":{}}},\"budget\":null,\"permissions\":[],\"runtime\":\"{}\",\"playbook\":{}}}",
        flow(n),
        agent(n),
        a.category.name(),
        if a.person { "yes" } else { "no" },
        if a.model { 1000 } else { 0 },
        a.runtime,
        if a.playbook { "1" } else { "null" }
    ))
    .unwrap()
}

fn workflow(n: usize, a: &A) -> FullWorkflow {
    FullWorkflow {
        id: flow(n),
        state: WorkflowState::Active,
        category: a.category,
        site: None,
        trigger: if a.slotted {
            WorkflowTrigger::Schedule
        } else {
            WorkflowTrigger::Manual
        },
        period: NonZeroU32::new(2),
        last: DAY - 2,
        agent: Some(agent(n)),
    }
}

struct World {
    ports: Ports<Config>,
    board: Board<Config>,
    limits: Limits,
    spec: Vec<A>,
}

fn names(slots: &[&str]) -> Vec<Name> {
    slots.iter().map(|s| Name::parse(s).unwrap()).collect()
}

fn member(world: &World, n: usize) -> Member<Config> {
    let a = world.spec[n];
    let events: Vec<Event> = world
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
    let last = world
        .ports
        .agents
        .borrow()
        .iter()
        .find(|e| e.agent == agent(n))
        .and_then(|e| e.last.clone());
    Member {
        facts: Facts {
            agent: agent(n),
            workflow: workflow(n, &a),
            config: config(n, &a),
            slots: names(a.slots),
            store_ended: newest.is_none_or(|r| last.as_ref().is_some_and(|l| l.run == r)),
            runs: last.into_iter().collect(),
            events,
            today: world.board.day,
            doing: Doing::Idle(None),
        },
        session: true,
    }
}

fn world_with(spec: &[A], limits: Limits) -> World {
    let ports = Ports::with_limits(T0, DAY, limits);
    for (n, a) in spec.iter().enumerate() {
        ports
            .workflows
            .borrow_mut()
            .push(Workflow::Full(workflow(n, a)));
        ports.agents.borrow_mut().push(Agent {
            workflow: flow(n),
            agent: agent(n),
            config: config(n, a),
            state: AgentState::Active,
            slots: names(a.slots),
            session: true,
            last: a.finished.then(|| PastRun {
                run: RunId::parse(&format!("{:032x}", 0x900 + n)).unwrap(),
                day: DAY - 2,
                outcome: Outcome::Completed,
                unknown: 0,
            }),
            unknown: vec![],
        });
        if a.finished {
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
        }
    }
    ports.calls.borrow_mut().clear();
    let mut w = World {
        ports,
        board: Board::new(DAY, vec![]),
        limits,
        spec: spec.to_vec(),
    };
    let members = (0..spec.len()).map(|n| member(&w, n)).collect();
    w.board = Board::new(DAY, members);
    w
}

fn world(spec: &[A]) -> World {
    world_with(spec, Limits::PRODUCT)
}

impl World {
    fn go(&mut self, mv: Move) -> Report {
        drive(&self.ports, &mut self.board, &self.limits, mv)
    }

    fn start(&mut self, n: usize) -> Report {
        self.go(Move::Start { agent: agent(n) })
    }

    fn end(&mut self, run: &RunId, outcome: Option<Outcome>) -> Report {
        let r = self.end_only(run, outcome);
        self.store_end(run, outcome);
        r
    }

    fn end_only(&mut self, run: &RunId, outcome: Option<Outcome>) -> Report {
        self.go(Move::End {
            run: run.clone(),
            outcome,
        })
    }

    fn store_end(&self, run: &RunId, outcome: Option<Outcome>) {
        let n = self
            .stored()
            .iter()
            .find_map(|e| match &e.what {
                What::RunStarted { run: r, .. } if r == run => Some(e.agent.clone()),
                _ => None,
            })
            .unwrap();
        let o = outcome.unwrap_or(Outcome::Failed);
        self.ports.store_end(
            &n,
            PastRun {
                run: run.clone(),
                day: self.board.day,
                outcome: o,
                unknown: u32::from(o == Outcome::EffectUnknown),
            },
        );
    }

    fn fail_start(&mut self, n: usize) -> Report {
        let r = self.start(n);
        assert_eq!(r.answer, Err(Cause::NoService));
        r
    }

    fn tick(&mut self) -> Report {
        self.go(Move::Tick)
    }

    fn clock(&self, now: u64, day: u32) {
        self.ports.set_clock(now, day);
    }

    fn calls(&self) -> Vec<String> {
        self.ports
            .calls
            .borrow()
            .iter()
            .filter_map(|t| act(&t.call))
            .collect()
    }

    fn forget(&self) {
        self.ports.calls.borrow_mut().clear();
    }

    fn stored(&self) -> Vec<Event> {
        self.ports.events.borrow().clone()
    }

    fn starts(&self) -> usize {
        self.calls()
            .iter()
            .filter(|c| c.starts_with("start "))
            .count()
    }

    fn facts(&self, n: usize) -> &Facts<Config> {
        &self
            .board
            .members
            .iter()
            .find(|m| m.facts.agent == agent(n))
            .unwrap()
            .facts
    }

    fn relife(&mut self) {
        let day = self.ports.today_for_test();
        self.board = Board::new(day, vec![]);
        let members = (0..self.spec.len()).map(|n| member(self, n)).collect();
        self.board = Board::new(day, members);
    }

    fn owner_workflow(&self, n: usize, change: impl FnOnce(&mut FullWorkflow)) {
        for w in self.ports.workflows.borrow_mut().iter_mut() {
            if let Workflow::Full(f) = w {
                if f.id == flow(n) {
                    change(f);
                    return;
                }
            }
        }
    }

    fn owner_agent(&self, n: usize, change: impl FnOnce(&mut Agent<Config>)) {
        if let Some(a) = self
            .ports
            .agents
            .borrow_mut()
            .iter_mut()
            .find(|a| a.agent == agent(n))
        {
            change(a);
        }
    }

    fn delete(&self, n: usize) {
        self.ports
            .workflows
            .borrow_mut()
            .retain(|w| w.id() != &flow(n));
    }

    fn run_of(&self, n: usize) -> RunId {
        self.board
            .runs
            .iter()
            .find(|r| r.agent == agent(n))
            .unwrap()
            .run
            .clone()
    }
}

trait TodayForTest {
    fn today_for_test(&self) -> u32;
}

impl TodayForTest for Ports<Config> {
    fn today_for_test(&self) -> u32 {
        halvo_coord::ports::Clock::today(self).unwrap()
    }
}

fn what_name(w: &What) -> String {
    match w {
        What::RunStarted { .. } => "run_started".into(),
        What::RunEnded { cause, .. } => format!("run_ended {cause}"),
        What::ItemRaised { raised, .. } => format!("item_raised {}", raised.kind()),
        What::ItemAnswered { answer, .. } => format!(
            "item_answered {} {}",
            answer.kind(),
            answer.choice().map(|c| c.name()).unwrap_or("-")
        ),
        What::ItemLapsed { lapsed, .. } => format!("item_lapsed {}", lapsed.kind()),
        What::ItemReleased { .. } => "item_released".into(),
        What::HoldSet { cause } => format!("hold_set {cause}"),
        What::HoldCleared { cause } => match cause {
            Cleared::Person => "hold_cleared person".into(),
            Cleared::Failures => "hold_cleared failures".into(),
            Cleared::Check { answer, .. } => format!("hold_cleared check {answer}"),
        },
        What::SlotSkipped { slot } => format!(
            "slot_skipped {}",
            slot.map(|s| (s as i64 - DAY as i64).to_string())
                .unwrap_or("-".into())
        ),
    }
}

fn act(call: &Call) -> Option<String> {
    Some(match call {
        Call::Log(events) => format!(
            "log {}",
            events
                .iter()
                .map(|e| what_name(&e.what))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Call::Send(Body::StartRun { .. }) => "start_run".into(),
        Call::Send(Body::SetState { state, .. }) => format!("set_state {state}"),
        Call::Send(Body::CreateAgent { .. }) => "create_agent".into(),
        Call::Send(Body::SetBindings { .. }) => "set_bindings".into(),
        Call::Send(Body::EndRun { run }) => format!("end_run {run}"),
        Call::Start(spec) => format!("start {}", spec.agent),
        Call::Cancel(run) => format!("cancel {run}"),
        Call::Kill(run) => format!("kill {run}"),
        Call::Defer(_) => "defer".into(),
        Call::Reap => "reap".into(),
        Call::AnswerConfirm(..) | Call::AnswerHandback(..) | Call::AnswerBind(..) => {
            "answer".into()
        }
        Call::Workflows | Call::Agents | Call::History(..) | Call::Attention(_) | Call::Changed => {
            return None
        }
    })
}

fn ok_run(r: &Report) -> RunId {
    assert_eq!(r.answer, Ok(Answer::Ok), "{r:?}");
    r.run.clone().unwrap()
}

fn count(events: &[Event], name: &str) -> usize {
    events
        .iter()
        .filter(|e| what_name(&e.what).starts_with(name))
        .count()
}

fn person() -> A {
    A {
        person: true,
        ..AUTO
    }
}

fn model() -> A {
    A {
        model: true,
        ..AUTO
    }
}

fn manual() -> A {
    A {
        slotted: false,
        ..AUTO
    }
}

#[test]
fn refusal_two_models() {
    let mut w = world_with(
        &[model(), model(), manual()],
        Limits {
            max_live_runs: 2,
            ..Limits::PRODUCT
        },
    );
    let first = ok_run(&w.start(0));
    let second = w.start(1);
    assert_eq!(second.answer, Ok(Answer::Queued));
    assert_eq!(second.run, None);
    assert_eq!(
        w.starts(),
        1,
        "Runs::start is not called for the second holder"
    );
    assert_eq!(w.board.queue.len(), 1);
    let beside = ok_run(&w.start(2));
    assert_eq!(
        w.starts(),
        2,
        "an agent that needs no model starts beside the holder"
    );
    w.forget();
    w.tick();
    assert_eq!(
        w.starts(),
        0,
        "a tick with the model held starts no second holder"
    );
    w.forget();
    let end = w.end(&beside, Some(Outcome::Completed));
    assert_eq!(
        end.ended[0].next[0].answer,
        Ok(Answer::Queued),
        "the holder is still live"
    );
    assert_eq!(w.starts(), 0);
    w.forget();
    let end = w.end(&first, Some(Outcome::Completed));
    assert_eq!(end.ended[0].next[0].agent, agent(1));
    assert_eq!(end.ended[0].next[0].answer, Ok(Answer::Ok));
    let calls = w.calls();
    let ended_at = calls
        .iter()
        .position(|c| c == "log run_ended completed")
        .unwrap();
    let started_at = calls
        .iter()
        .position(|c| *c == format!("start {}", agent(1)))
        .unwrap();
    assert!(ended_at < started_at, "{calls:?}");
}

#[test]
fn refusal_two_runs_agent() {
    let mut w = world(&[AUTO, AUTO]);
    ok_run(&w.start(0));
    w.forget();
    let again = w.start(0);
    assert_eq!(again.answer, Err(Cause::State));
    assert_eq!(
        w.calls(),
        Vec::<String>::new(),
        "nothing is stored and nothing starts"
    );
    assert_eq!(w.start(1).answer, Ok(Answer::Queued));
    w.forget();
    let queued_again = w.start(1);
    assert_eq!(queued_again.answer, Err(Cause::State));
    assert_eq!(w.board.queue.len(), 1);
    assert_eq!(w.calls(), Vec::<String>::new());
}

#[test]
fn refusal_hold_start() {
    let none = A {
        runtime: "none",
        category: Category::RideOrder,
        ..AUTO
    };
    let setup = A {
        runtime: "graph_mail",
        category: Category::Email,
        ..AUTO
    };
    let mut w = world(&[none, AUTO, AUTO, AUTO, setup]);
    assert_eq!(
        w.go(Move::SwitchOff { agent: agent(1) }).answer,
        Ok(Answer::Ok)
    );
    let failures = w.facts(2).events.len() as u32 + 1;
    w.ports
        .log(&[Event {
            workflow: flow(2),
            agent: agent(2),
            seq: NonZeroU32::new(failures).unwrap(),
            day: DAY,
            at: 0,
            what: What::HoldSet {
                cause: HoldSetCause::Failures,
            },
        }])
        .unwrap();
    w.relife();
    with_check(&mut w, 3);
    w.forget();
    let tick = w.tick();
    assert_eq!(
        tick.started,
        Vec::<Served>::new(),
        "the scheduler starts none under a hold"
    );
    for (n, hold) in [
        (0, Hold::NotYet),
        (1, Hold::Off),
        (3, Hold::Check),
        (4, Hold::Setup),
    ] {
        assert_eq!(halvo_coord::schedule::holds(w.facts(n)), vec![hold]);
        let r = w.start(n);
        assert_eq!(r.answer, Err(Cause::Hold), "{hold:?}");
        assert_eq!(r.run, None);
    }
    let by_scheduler = start_step(
        w.facts(2),
        Trigger::Schedule,
        &w.limits,
        &Around {
            listed: true,
            live: 0,
            model_held: false,
            queued: 0,
            session: true,
            from_queue: false,
            owed: false,
        },
    );
    assert_eq!(by_scheduler.verdict, Verdict::Refused(Cause::Hold));
    assert_eq!(by_scheduler.clear, None);
    assert_eq!(w.starts(), 0);
    assert_eq!(
        count(&w.stored(), "run_started"),
        5,
        "the runs before the test"
    );
    w.forget();
    let person = w.start(2);
    ok_run(&person);
    assert_eq!(
        w.calls(),
        vec![
            "log hold_cleared failures".to_string(),
            "set_state active".into(),
            "defer".into(),
            "log run_started".into(),
            "start_run".into(),
            format!("start {}", agent(2)),
        ],
        "a start by the person ends failures"
    );
}

#[test]
fn refusal_unrecorded_start() {
    let mut w = world(&[AUTO]);
    w.ports.refuse(Method::Log, PortError::NoService);
    let r = w.start(0);
    assert_eq!(r.answer, Err(Cause::NoService));
    assert_eq!(r.run, None);
    assert_eq!(
        w.calls(),
        vec!["defer".to_string()],
        "no start_run and no Runs::start"
    );
    assert_eq!(w.board.runs.len(), 0);
    assert!(matches!(w.facts(0).doing, Doing::Idle(None)));
    w.ports.refuse(Method::Log, PortError::Busy);
    assert_eq!(w.start(0).answer, Err(Cause::Busy));
    assert_eq!(w.starts(), 0);
    w.ports.clear_refusals();
    ok_run(&w.start(0));
}

#[test]
fn refusal_second_slot_run() {
    let mut w = world(&[AUTO]);
    let first = w.tick();
    assert_eq!(first.started.len(), 1);
    assert_eq!(first.started[0].answer, Ok(Answer::Ok));
    w.relife();
    let second = w.tick();
    assert_eq!(second.started, Vec::<Served>::new());
    w.clock(T0 + 1, DAY + 1);
    let third = w.tick();
    assert_eq!(
        third.started,
        Vec::<Served>::new(),
        "the next day of the same slot"
    );
    let stored = w.stored();
    let slot_starts: Vec<_> = stored
        .iter()
        .filter(|e| matches!(e.what, What::RunStarted { slot: Some(s), .. } if s == DAY))
        .collect();
    assert_eq!(slot_starts.len(), 1);
    assert_eq!(w.starts(), 1);
}

#[test]
fn refusal_auto_needs_person() {
    let mut w = world(&[person()]);
    let tick = w.tick();
    assert_eq!(tick.raised, vec![agent(0)]);
    assert_eq!(tick.started, Vec::<Served>::new());
    assert_eq!(w.starts(), 0);
    assert_eq!(count(&w.stored(), "item_raised ready"), 1);
    assert!(matches!(
        w.facts(0).doing,
        Doing::Idle(Some(ref own)) if own.kind == OwnItemKind::Ready
    ));
    for day in 1..9 {
        w.clock(T0 + day as u64, DAY + day);
        assert_eq!(w.tick().raised, Vec::<AgentId>::new());
    }
    assert_eq!(
        count(&w.stored(), "item_raised ready"),
        1,
        "once however many slots passed"
    );
    assert_eq!(
        count(&w.stored(), "run_started"),
        1,
        "the run before the test alone"
    );
}

#[test]
fn refusal_over_budget() {
    let mut w = world(&[manual()]);
    for _ in 0..6 {
        let run = ok_run(&w.start(0));
        w.end(&run, Some(Outcome::Completed));
    }
    w.forget();
    let seventh = w.start(0);
    assert_eq!(seventh.answer, Err(Cause::Budget));
    assert_eq!(w.calls(), Vec::<String>::new());
    let today = w
        .stored()
        .iter()
        .filter(|e| e.day == DAY && matches!(e.what, What::RunStarted { .. }))
        .count();
    assert_eq!(today, 6);
    w.clock(T0 + 1, DAY + 1);
    w.tick();
    ok_run(&w.start(0));
}

#[test]
fn refusal_queue_full() {
    let spec = [manual(); 6];
    let mut w = world(&spec);
    ok_run(&w.start(0));
    for n in 1..5 {
        assert_eq!(w.start(n).answer, Ok(Answer::Queued), "start {n}");
    }
    let fifth = w.start(5);
    assert_eq!(fifth.answer, Err(Cause::QueueFull));
    assert_eq!(w.board.queue.len(), 4);
    assert!(!w.board.queue.holds(&agent(5)));
    assert!(matches!(w.facts(5).doing, Doing::Idle(None)));
}

#[test]
fn one_start_goes_through_steps_5_to_8_in_order() {
    let mut w = world(&[AUTO]);
    let r = w.start(0);
    let run = ok_run(&r);
    assert_eq!(
        w.calls(),
        vec![
            "defer".to_string(),
            "log run_started".into(),
            "start_run".into(),
            format!("start {}", agent(0)),
        ]
    );
    let defer = w
        .ports
        .calls
        .borrow()
        .iter()
        .find_map(|t| match t.call {
            Call::Defer(until) => Some(until),
            _ => None,
        })
        .unwrap();
    assert_eq!(defer, T0 + (120 + 900) * 1000);
    let e = w.stored().last().cloned().unwrap();
    assert_eq!(
        e.what,
        What::RunStarted {
            run: run.clone(),
            trigger: Trigger::Person,
            slot: Some(DAY)
        }
    );
    assert_eq!((e.day, e.at), (DAY, T0 as i64 * 1_000_000));
    let spec = w
        .ports
        .calls
        .borrow()
        .iter()
        .find_map(|t| match &t.call {
            Call::Start(s) => Some(s.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        (spec.run, spec.agent, spec.workflow, spec.headed),
        (run, agent(0), flow(0), false)
    );
    assert!(matches!(
        w.facts(0).doing,
        Doing::Live(halvo_coord::types::Live::Starting)
    ));
}

#[test]
fn a_failed_step_7_stores_the_start_and_its_failed_end_and_starts_no_runner() {
    let mut w = world(&[AUTO]);
    w.ports.refuse(
        Method::Send,
        PortError::Refused(Name::parse("state").unwrap()),
    );
    let r = w.start(0);
    assert_eq!(r.answer, Err(Cause::NoService));
    assert!(r.run.is_some());
    assert_eq!(
        w.calls(),
        vec![
            "defer".to_string(),
            "log run_started".into(),
            "log run_ended failed".into()
        ]
    );
    assert_eq!(w.board.runs.len(), 0);
    assert!(matches!(w.facts(0).doing, Doing::Idle(None)));
}

#[test]
fn a_failed_step_8_stores_failed_and_the_streak_sets_failures_with_paused() {
    let mut w = world(&[manual()]);
    w.ports.refuse(Method::Start, PortError::NoService);
    for _ in 0..2 {
        w.fail_start(0);
    }
    w.forget();
    let third = w.fail_start(0).run.unwrap();
    assert_eq!(
        w.calls(),
        vec![
            "defer".to_string(),
            "log run_started".into(),
            "start_run".into(),
            "log run_ended failed, hold_set failures".into(),
            "set_state paused".into(),
            format!("end_run {third}"),
        ]
    );
    assert_eq!(
        halvo_coord::schedule::holds(w.facts(0)),
        vec![Hold::Failures]
    );
    w.ports.clear_refusals();
    w.forget();
    ok_run(&w.start(0));
    let run = w.run_of(0);
    w.forget();
    w.end(&run, Some(Outcome::Failed));
    assert_eq!(
        w.calls(),
        vec![
            "log run_ended failed, hold_set failures".to_string(),
            "set_state paused".into()
        ],
        "a start by the person leaves the streak as it is"
    );
}

#[test]
fn an_end_of_another_cause_ends_the_streak() {
    let mut w = world(&[manual()]);
    w.ports.refuse(Method::Start, PortError::NoService);
    w.fail_start(0);
    w.fail_start(0);
    w.ports.clear_refusals();
    let run = ok_run(&w.start(0));
    w.end(&run, Some(Outcome::Declined));
    w.ports.refuse(Method::Start, PortError::NoService);
    w.forget();
    w.fail_start(0);
    w.fail_start(0);
    assert_eq!(
        w.calls().iter().filter(|c| c.contains("hold_set")).count(),
        0
    );
    w.fail_start(0);
    assert_eq!(
        w.calls().iter().filter(|c| c.contains("hold_set")).count(),
        1
    );
}

#[test]
fn a_stopped_run_ends_with_the_cause_of_its_stop_whatever_its_last_frame() {
    let mut w = world(&[manual()]);
    let run = ok_run(&w.start(0));
    w.go(Move::SwitchOff { agent: agent(0) });
    let end = w.end(&run, Some(Outcome::Completed));
    assert_eq!(end.ended[0].cause, RunEnd::Cancelled);
    assert_eq!(count(&w.stored(), "run_ended cancelled"), 1);
    let mut g = world(&[manual()]);
    let run = ok_run(&g.start(0));
    let end = g.end(&run, None);
    assert_eq!(end.ended[0].cause, RunEnd::Interrupted, "no last frame");
}

fn queue_two_behind(w: &mut World) -> RunId {
    let run = ok_run(&w.start(0));
    assert_eq!(w.start(1).answer, Ok(Answer::Queued));
    run
}

/// A hold `off` that the owner stored between the queue and the start, which the board learns
/// from the start's read after its refusal `seq`: the start leaves the queue answered `hold`
/// (ruling RD-52 (a)). The hold is stored at the owner because a switch-off in this life takes
/// the start out of the queue (ruling RD-52 (d)).
#[test]
fn a_hold_stored_between_the_queue_and_the_start_drops_the_start() {
    let mut w = world(&[manual(), manual(), manual()]);
    let run = queue_two_behind(&mut w);
    assert_eq!(w.start(2).answer, Ok(Answer::Queued));
    stored_at_owner(&mut w, 1, OFF, false);
    w.forget();
    let end = w.end(&run, Some(Outcome::Completed));
    let next = &end.ended[0].next;
    assert_eq!(next.len(), 1, "one end takes one start from the queue");
    assert_eq!(
        (next[0].agent.clone(), next[0].answer),
        (agent(1), Err(Cause::Hold))
    );
    assert_eq!(
        w.starts(),
        0,
        "no start behind the dropped one in that step"
    );
    assert_eq!(w.board.queue.len(), 1);
    assert!(matches!(w.facts(1).doing, Doing::Idle(None)));
    let tick = w.tick();
    assert_eq!(
        tick.next[0].agent,
        agent(2),
        "the next tick with no live run serves it"
    );
    assert_eq!(tick.next[0].answer, Ok(Answer::Ok));
}

#[test]
fn a_start_of_the_scheduler_that_leaves_the_queue_after_its_slot_closed_is_dropped() {
    let limits = Limits {
        catch_up_days: 0,
        ..Limits::PRODUCT
    };
    let mut w = world_with(&[manual(), AUTO], limits);
    let run = ok_run(&w.start(0));
    let tick = w.tick();
    assert_eq!(tick.started[0].answer, Ok(Answer::Queued));
    w.clock(T0 + 1, DAY + 1);
    w.tick();
    w.forget();
    let end = w.end(&run, Some(Outcome::Completed));
    assert_eq!(end.ended[0].next[0].answer, Err(Cause::State));
    assert_eq!(w.starts(), 0);
    assert!(w.board.queue.is_empty());
}

#[test]
fn a_tick_starts_on_the_day_the_slot_opens_and_not_after_catch_up_days() {
    let mut w = world(&[AUTO]);
    w.clock(T0, DAY - 1);
    assert_eq!(
        w.tick().started,
        Vec::<Served>::new(),
        "the day before the slot"
    );
    w.clock(T0, DAY);
    assert_eq!(w.tick().started.len(), 1, "the day the slot opens");
    let mut late = world(&[A { ..AUTO }]);
    late.owner_workflow(0, |f| f.period = NonZeroU32::new(30));
    late.clock(T0, DAY - 2 + 30 + 8);
    assert_eq!(
        late.tick().started,
        Vec::<Served>::new(),
        "the day after catch_up_days"
    );
    late.clock(T0, DAY - 2 + 60);
    assert_eq!(late.tick().started.len(), 1, "the next slot");
}

#[test]
fn a_workflow_that_is_gone_while_its_start_waits_loses_the_start_at_the_tick() {
    let mut w = world(&[manual(), manual()]);
    queue_two_behind(&mut w);
    w.delete(1);
    let gone = w.start(1);
    assert_eq!(
        gone.answer,
        Err(Cause::State),
        "step 0 refuses a start of it"
    );
    w.forget();
    let tick = w.tick();
    assert!(w.board.queue.is_empty());
    assert_eq!(tick.stopped, Vec::<RunId>::new());
    assert_eq!(tick.dropped, vec![agent(1)]);
    assert!(
        !w.calls().iter().any(|c| c.starts_with("log")),
        "nothing is stored for the dropped start: {:?}",
        w.calls()
    );
}

#[test]
fn a_tick_drops_the_ready_item_of_a_workflow_that_is_gone_and_stores_nothing() {
    let mut w = world(&[person()]);
    assert_eq!(w.tick().raised, vec![agent(0)]);
    w.delete(0);
    w.forget();
    let tick = w.tick();
    assert_eq!(tick.dropped, vec![agent(0)]);
    assert!(matches!(w.facts(0).doing, Doing::Idle(None)));
    assert_eq!(w.calls(), Vec::<String>::new());
}

#[test]
fn a_start_of_the_person_for_a_workflow_that_is_gone_ends_no_hold() {
    let mut w = world(&[manual()]);
    w.ports.refuse(Method::Start, PortError::NoService);
    w.fail_start(0);
    w.fail_start(0);
    w.fail_start(0);
    w.ports.clear_refusals();
    w.delete(0);
    w.forget();
    assert_eq!(w.start(0).answer, Err(Cause::State));
    assert_eq!(w.calls(), Vec::<String>::new());
}

#[test]
fn a_restart_with_a_start_in_the_queue_leads_to_no_start_of_it() {
    let mut w = world(&[
        manual(),
        A {
            person: true,
            finished: false,
            ..manual()
        },
    ]);
    ok_run(&w.start(0));
    assert_eq!(w.start(1).answer, Ok(Answer::Queued));
    w.relife();
    assert!(w.board.queue.is_empty());
    w.forget();
    let tick = w.tick();
    assert_eq!(tick.next, Vec::<Served>::new());
    assert_eq!(
        tick.raised,
        vec![agent(1)],
        "the first tick raises ready again"
    );
    assert_eq!(w.starts(), 0);
}

#[test]
fn one_tick_works_in_the_order_of_i5_9() {
    let mut w = world_with(
        &[manual(), manual(), manual(), person(), AUTO],
        Limits {
            max_live_runs: 3,
            ..Limits::PRODUCT
        },
    );
    let stopped = ok_run(&w.start(0));
    let gone = ok_run(&w.start(1));
    let kept = ok_run(&w.start(2));
    w.go(Move::SwitchOff { agent: agent(0) });
    w.delete(1);
    w.clock(T0 + 5_000, DAY);
    w.forget();
    let tick = w.tick();
    assert_eq!(
        w.calls(),
        vec![
            "defer".to_string(),
            format!("cancel {gone}"),
            format!("kill {stopped}"),
            "log run_ended cancelled".into(),
            "log item_raised ready".into(),
            "defer".into(),
            "log run_started".into(),
            "start_run".into(),
            format!("start {}", agent(4)),
        ]
    );
    assert_eq!(tick.stopped, vec![gone.clone()]);
    assert_eq!(tick.ended.len(), 1);
    assert_eq!(
        (tick.ended[0].run.clone(), tick.ended[0].cause),
        (stopped, RunEnd::Cancelled)
    );
    assert_eq!(tick.raised, vec![agent(3)]);
    assert_eq!(tick.started[0].agent, agent(4));
    assert_eq!(
        tick.next,
        Vec::<Served>::new(),
        "a kill ended a run, so the tick serves nothing"
    );
    w.clock(T0 + 10_000, DAY);
    w.forget();
    let later = w.tick();
    assert_eq!(later.ended[0].cause, RunEnd::Deleted);
    assert_eq!(w.calls()[0], "defer");
    assert_eq!(w.calls()[1], format!("kill {gone}"));
    assert!(
        !w.calls().iter().any(|c| c.starts_with("log run_ended")),
        "no run_ended for a run whose workflow is gone"
    );
    let _ = kept;
}

#[test]
fn a_tick_kills_a_stopped_run_whose_runner_ended_in_between_and_stores_nothing_for_it() {
    let mut w = world(&[manual()]);
    let run = ok_run(&w.start(0));
    w.go(Move::SwitchOff { agent: agent(0) });
    w.end(&run, None);
    w.clock(T0 + 4_999, DAY);
    w.forget();
    w.tick();
    assert_eq!(w.calls(), Vec::<String>::new(), "the grace has not passed");
    w.clock(T0 + 5_000, DAY);
    w.tick();
    assert_eq!(w.calls(), vec![format!("kill {run}")]);
    w.forget();
    w.clock(T0 + 9_000, DAY);
    w.tick();
    assert_eq!(w.calls(), Vec::<String>::new(), "killed once");
}

#[test]
fn a_tick_with_no_live_run_takes_one_start_from_the_queue_before_the_agents() {
    let mut w = world(&[manual(), manual(), manual(), person()]);
    let run = queue_two_behind(&mut w);
    assert_eq!(w.start(2).answer, Ok(Answer::Queued));
    stored_at_owner(&mut w, 1, OFF, false);
    w.end(&run, Some(Outcome::Completed));
    w.forget();
    let tick = w.tick();
    assert_eq!(tick.next[0].agent, agent(2));
    let calls = w.calls();
    let start = calls
        .iter()
        .position(|c| *c == format!("start {}", agent(2)))
        .unwrap();
    let raise = calls
        .iter()
        .position(|c| c == "log item_raised ready")
        .unwrap();
    assert!(start < raise, "{calls:?}");
    assert_eq!(
        calls.iter().filter(|c| *c == "defer").count(),
        1,
        "no run was live before"
    );
}

#[test]
fn tick_step_raises_ready_for_a_first_run_and_an_open_slot_of_the_person_and_starts_a_due_slot() {
    let w = world(&[
        person(),
        AUTO,
        A {
            finished: false,
            ..AUTO
        },
        manual(),
        A {
            person: true,
            slotted: false,
            ..AUTO
        },
    ]);
    let l = Limits::PRODUCT;
    assert_eq!(tick_step(w.facts(0), true, &l), Ask::Ready);
    assert_eq!(tick_step(w.facts(1), true, &l), Ask::Start);
    assert_eq!(
        tick_step(w.facts(2), true, &l),
        Ask::Ready,
        "a first run is the person's"
    );
    assert_eq!(tick_step(w.facts(3), true, &l), Ask::Nothing, "no slot");
    assert_eq!(tick_step(w.facts(4), true, &l), Ask::Nothing, "no slot");
    assert_eq!(tick_step(w.facts(1), false, &l), Ask::Nothing, "gone");
    let mut queued = w.facts(1).clone();
    queued.doing = Doing::Queued(None);
    assert_eq!(tick_step(&queued, true, &l), Ask::Nothing);
    let mut live = w.facts(0).clone();
    live.doing = Doing::Live(halvo_coord::types::Live::Working);
    assert_eq!(tick_step(&live, true, &l), Ask::Nothing);
    let mut w = world(&[AUTO, manual()]);
    nothing_first(&mut w, 0);
    nothing_first(&mut w, 1);
    assert_eq!(
        tick_step(w.facts(0), true, &l),
        Ask::Ready,
        "a clean run and no finished one: ready at an open slot (ruling RD-67 5.2)"
    );
    assert_eq!(
        tick_step(w.facts(1), true, &l),
        Ask::Nothing,
        "a clean run and no slot: no ready at once"
    );
}

/// Makes the one stored run of agent `n` one that ended `nothing_to_do`, at the owner and on the
/// board: a clean run, and no finished one (ruling RD-67 5.2).
fn nothing_first(w: &mut World, n: usize) {
    let to_nothing = |e: &mut Event| {
        if let What::RunEnded { cause, .. } = &mut e.what {
            if e.agent == agent(n) {
                *cause = RunEnd::NothingToDo;
            }
        }
    };
    w.ports.events.borrow_mut().iter_mut().for_each(to_nothing);
    let i = w
        .board
        .members
        .iter()
        .position(|m| m.facts.agent == agent(n))
        .unwrap();
    w.board.members[i]
        .facts
        .events
        .iter_mut()
        .for_each(to_nothing);
    for r in &mut w.board.members[i].facts.runs {
        r.outcome = Outcome::NothingToDo;
    }
}

/// RD-67 5.2: the slot of a skip and the slot of a start by the person read the clean run, so
/// that a skip at a card that an open slot raised for an agent with a clean run and no finished
/// one stores that slot's day, and a start by the person stores it as its slot.
#[test]
fn rd67_the_slot_of_a_skip_and_of_a_persons_start_read_the_clean_run() {
    let mut w = world(&[AUTO]);
    nothing_first(&mut w, 0);
    assert_eq!(w.tick().raised, vec![agent(0)]);
    w.forget();
    assert_eq!(w.go(Move::Skip { agent: agent(0) }).answer, Ok(Answer::Ok));
    assert_eq!(
        w.calls(),
        vec!["log item_answered ready skip, slot_skipped 0".to_string()]
    );
    let mut s = world(&[AUTO]);
    nothing_first(&mut s, 0);
    let run = ok_run(&s.start(0));
    let slot = s.stored().iter().find_map(|e| match &e.what {
        What::RunStarted { run: r, slot, .. } if *r == run => Some(*slot),
        _ => None,
    });
    assert_eq!(
        slot,
        Some(Some(DAY)),
        "the start by the person takes the slot"
    );
}

#[test]
fn skip_stores_slot_skipped_and_no_ready_comes_before_the_next_slot() {
    let mut w = world(&[person()]);
    w.tick();
    w.forget();
    assert_eq!(w.go(Move::Skip { agent: agent(0) }).answer, Ok(Answer::Ok));
    assert_eq!(
        w.calls(),
        vec!["log item_answered ready skip, slot_skipped 0".to_string()]
    );
    assert!(matches!(w.facts(0).doing, Doing::Idle(None)));
    w.clock(T0, DAY + 1);
    assert_eq!(w.tick().raised, Vec::<AgentId>::new());
    w.clock(T0, DAY + 2);
    assert_eq!(w.tick().raised, vec![agent(0)], "the next slot");
    assert_eq!(
        w.go(Move::Skip { agent: agent(1) }).answer,
        Err(Cause::UnknownAgent)
    );
}

#[test]
fn skip_of_a_first_run_stores_a_null_slot_and_waits_for_the_next_day() {
    let mut w = world(&[A {
        finished: false,
        ..AUTO
    }]);
    w.tick();
    w.forget();
    w.go(Move::Skip { agent: agent(0) });
    assert_eq!(
        w.calls(),
        vec!["log item_answered ready skip, slot_skipped -".to_string()]
    );
    assert_eq!(w.tick().raised, Vec::<AgentId>::new());
    w.clock(T0, DAY + 1);
    assert_eq!(
        w.tick().raised,
        Vec::<AgentId>::new(),
        "turned by ruling RD-73 1.2: no ready until the next slot opens"
    );
    w.clock(T0, DAY + 2);
    assert_eq!(w.tick().raised, vec![agent(0)], "the next slot");
    let mut manual = world(&[A {
        finished: false,
        ..manual()
    }]);
    manual.tick();
    manual.go(Move::Skip { agent: agent(0) });
    for day in [DAY + 1, DAY + 40] {
        manual.clock(T0, day);
        assert_eq!(
            manual.tick().raised,
            Vec::<AgentId>::new(),
            "no slot: no ready while the skip's row is handed out"
        );
    }
    let mut none = world(&[AUTO]);
    assert_eq!(
        none.go(Move::Skip { agent: agent(0) }).answer,
        Err(Cause::State),
        "no ready item"
    );
}

#[test]
fn a_start_closes_the_ready_item_and_a_refused_one_leaves_it_open() {
    let mut w = world(&[person(), manual()]);
    w.tick();
    w.ports.refuse(Method::Log, PortError::NoService);
    assert_eq!(w.start(0).answer, Err(Cause::NoService));
    assert!(
        matches!(w.facts(0).doing, Doing::Idle(Some(_))),
        "stays open"
    );
    w.ports.clear_refusals();
    w.forget();
    ok_run(&w.start(0));
    assert_eq!(w.calls()[1], "log item_answered ready start, run_started");
    let mut q = world(&[manual(), person()]);
    ok_run(&q.start(0));
    q.tick();
    q.forget();
    assert_eq!(q.start(1).answer, Ok(Answer::Queued));
    assert_eq!(q.calls(), vec!["log item_answered ready start".to_string()]);
    assert!(matches!(q.facts(1).doing, Doing::Queued(None)));
}

fn with_check(w: &mut World, n: usize) -> RunId {
    let run = RunId::parse(&format!("{:032x}", 0x900 + n)).unwrap();
    w.ports.store_end(
        &agent(n),
        PastRun {
            run: run.clone(),
            day: DAY - 2,
            outcome: Outcome::EffectUnknown,
            unknown: 1,
        },
    );
    run
}

#[test]
fn went_through_clears_the_check_and_skips_the_slot_of_its_run() {
    let mut w = world(&[AUTO]);
    let run = with_check(&mut w, 0);
    assert_eq!(w.tick().started, Vec::<Served>::new());
    w.forget();
    let r = w.go(Move::Check {
        agent: agent(0),
        answer: CheckAnswer::WentThrough,
    });
    assert_eq!(r.answer, Ok(Answer::Ok));
    assert_eq!(
        w.calls(),
        vec!["log hold_cleared check went_through, slot_skipped -".to_string()]
    );
    let last = w.stored().iter().rev().nth(1).cloned().unwrap();
    assert_eq!(
        last.what,
        What::HoldCleared {
            cause: Cleared::Check {
                run,
                answer: CheckAnswer::WentThrough
            }
        }
    );
    assert_eq!(halvo_coord::schedule::holds(w.facts(0)), Vec::<Hold>::new());
    assert_eq!(
        w.go(Move::Check {
            agent: agent(0),
            answer: CheckAnswer::TryAgain
        })
        .answer,
        Err(Cause::State),
        "a check that is answered is not answered again"
    );
}

#[test]
fn try_again_clears_the_check_and_the_agent_runs_at_its_next_run() {
    let mut w = world(&[AUTO]);
    with_check(&mut w, 0);
    w.forget();
    w.go(Move::Check {
        agent: agent(0),
        answer: CheckAnswer::TryAgain,
    });
    assert_eq!(
        w.calls(),
        vec!["log hold_cleared check try_again".to_string()]
    );
    assert_eq!(w.tick().started.len(), 1, "the slot is still due");
}

#[test]
fn went_through_skips_the_slot_that_the_checked_run_started_for() {
    let mut w = world(&[AUTO]);
    let run = ok_run(&w.tick().started[0].clone().into_report());
    w.end(&run, Some(Outcome::EffectUnknown));
    w.forget();
    w.go(Move::Check {
        agent: agent(0),
        answer: CheckAnswer::WentThrough,
    });
    assert_eq!(
        w.calls(),
        vec!["log hold_cleared check went_through, slot_skipped 0".to_string()]
    );
}

trait IntoReport {
    fn into_report(self) -> Report;
}

impl IntoReport for Served {
    fn into_report(self) -> Report {
        Report {
            answer: self.answer,
            run: self.run,
            ..Report::default()
        }
    }
}

#[test]
fn switch_on_ends_off_and_failures_with_one_event_each_and_then_sends_the_state() {
    let mut w = world(&[manual()]);
    w.ports.refuse(Method::Start, PortError::NoService);
    for _ in 0..3 {
        w.fail_start(0);
    }
    w.ports.clear_refusals();
    w.go(Move::SwitchOff { agent: agent(0) });
    assert_eq!(
        halvo_coord::schedule::holds(w.facts(0)),
        vec![Hold::Off, Hold::Failures]
    );
    w.forget();
    assert_eq!(
        w.go(Move::SwitchOn { agent: agent(0) }).answer,
        Ok(Answer::Ok)
    );
    assert_eq!(
        w.calls(),
        vec![
            "log hold_cleared person, hold_cleared failures".to_string(),
            "set_state active".into()
        ]
    );
    assert_eq!(halvo_coord::schedule::holds(w.facts(0)), Vec::<Hold>::new());
    assert_eq!(
        w.go(Move::SwitchOn { agent: agent(0) }).answer,
        Err(Cause::State)
    );
    let mut web = world(&[A {
        runtime: "web",
        finished: false,
        playbook: false,
        category: Category::CalendarCleanup,
        ..manual()
    }]);
    web.go(Move::SwitchOff { agent: agent(0) });
    web.forget();
    web.go(Move::SwitchOn { agent: agent(0) });
    assert_eq!(
        web.calls(),
        vec![
            "log hold_cleared person".to_string(),
            "set_state approved".into()
        ]
    );
}

#[test]
fn switch_off_stores_its_hold_first_stops_the_run_and_then_sends_paused() {
    let mut w = world(&[manual(), person()]);
    let run = ok_run(&w.start(0));
    w.forget();
    let r = w.go(Move::SwitchOff { agent: agent(0) });
    assert_eq!(r.stopped, vec![run.clone()]);
    assert_eq!(
        w.calls(),
        vec![
            "log hold_set person".to_string(),
            format!("cancel {run}"),
            "set_state paused".into()
        ]
    );
    assert_eq!(
        w.go(Move::SwitchOff { agent: agent(0) }).answer,
        Err(Cause::State)
    );
    w.tick();
    w.go(Move::SwitchOff { agent: agent(1) });
    assert!(
        matches!(w.facts(1).doing, Doing::Idle(Some(_))),
        "an open ready item stays open"
    );
    assert_eq!(w.start(1).answer, Err(Cause::Hold));
    w.ports.refuse(Method::Log, PortError::NoService);
    let mut g = world(&[manual()]);
    g.ports.refuse(Method::Log, PortError::NoService);
    assert_eq!(
        g.go(Move::SwitchOff { agent: agent(0) }).answer,
        Err(Cause::NoService)
    );
    assert_eq!(
        g.calls(),
        Vec::<String>::new(),
        "nothing before the hold is stored"
    );
}

#[test]
fn a_tick_sends_the_request_to_defer_again_while_a_run_is_live() {
    let mut w = world(&[manual()]);
    w.tick();
    assert!(!w.calls().contains(&"defer".to_string()));
    let run = ok_run(&w.start(0));
    w.forget();
    w.clock(T0 + 3_000, DAY);
    w.tick();
    assert_eq!(w.calls(), vec!["defer".to_string()]);
    let until = w
        .ports
        .calls
        .borrow()
        .iter()
        .find_map(|t| match t.call {
            Call::Defer(u) => Some(u),
            _ => None,
        })
        .unwrap();
    assert_eq!(until, T0 + 3_000 + (120 + 900) * 1000);
    w.ports.refuse(Method::Defer, PortError::NoService);
    w.end(&run, Some(Outcome::Completed));
    ok_run(&w.start(0));
}

fn approved(n: usize, category: Category) -> FullWorkflow {
    FullWorkflow {
        id: flow(n),
        state: WorkflowState::Approved,
        category,
        site: None,
        trigger: WorkflowTrigger::Manual,
        period: None,
        last: DAY,
        agent: None,
    }
}

fn listed_agent(w: &World, n: usize, made: &AgentId, a: &A) {
    let cfg = Config::parse(
        &serde_json::to_string(&serde_json::json!({
            "v": 1, "workflow": flow(n).as_str(), "agent": made.as_str(), "rev": 1,
            "category": a.category.name(), "tier": "on_device", "reasoning": "medium",
            "autonomy": "draft_and_confirm", "allow": [],
            "caps": {"untrusted": "yes", "private": "yes", "external": "yes"},
            "limits": {"steps": 20, "wall_s": 120, "model_tokens": 0},
            "budget": null, "permissions": [], "runtime": a.runtime,
            "playbook": if a.playbook { serde_json::json!(1) } else { serde_json::Value::Null }
        }))
        .unwrap(),
    )
    .unwrap();
    w.ports.agents.borrow_mut().push(Agent {
        workflow: flow(n),
        agent: made.clone(),
        config: cfg,
        state: AgentState::Approved,
        slots: vec![],
        session: false,
        last: None,
        unknown: vec![],
    });
}

fn minted(k: u128) -> AgentId {
    AgentId::parse(&format!("{k:032x}")).unwrap()
}

#[test]
fn an_approved_workflow_with_no_agent_gets_one_and_the_state_follows_its_runtime() {
    let mut w = world(&[]);
    for (n, c) in [
        (0, Category::CalendarCleanup),
        (1, Category::Email),
        (2, Category::CalendarCleanup),
    ] {
        w.ports
            .workflows
            .borrow_mut()
            .push(Workflow::Full(approved(n, c)));
    }
    listed_agent(
        &w,
        0,
        &minted(1),
        &A {
            runtime: "graph_calendar",
            ..AUTO
        },
    );
    listed_agent(
        &w,
        1,
        &minted(2),
        &A {
            runtime: "graph_mail",
            category: Category::Email,
            ..AUTO
        },
    );
    listed_agent(
        &w,
        2,
        &minted(3),
        &A {
            runtime: "web",
            playbook: false,
            ..AUTO
        },
    );
    let r = w.go(Move::Create);
    assert_eq!(r.created, vec![minted(1), minted(2), minted(3)]);
    assert_eq!(
        w.calls(),
        vec![
            "create_agent".to_string(),
            "create_agent".into(),
            "create_agent".into(),
            "log item_raised setup".into(),
        ],
        "the creation sends no set_state"
    );
    assert_eq!(w.board.members.len(), 3);
    for n in 0..3 {
        w.owner_workflow(n, |f| f.agent = Some(minted(n as u128 + 1)));
    }
    w.forget();
    assert_eq!(w.go(Move::Create).created, Vec::<AgentId>::new());
    assert_eq!(
        w.calls(),
        Vec::<String>::new(),
        "each workflow gets one agent"
    );
    assert!(!w.board.roster_full);
}

#[test]
fn an_approved_workflow_beyond_roster_max_gets_no_agent() {
    let mut w = world_with(
        &[manual()],
        Limits {
            roster_max: 1,
            ..Limits::PRODUCT
        },
    );
    w.ports
        .workflows
        .borrow_mut()
        .push(Workflow::Full(approved(1, Category::CalendarCleanup)));
    let r = w.go(Move::Create);
    assert_eq!(r.created, Vec::<AgentId>::new());
    assert!(!w.calls().contains(&"create_agent".to_string()));
    assert!(w.board.roster_full);
}

#[test]
fn a_move_for_an_agent_that_the_board_does_not_hold_is_unknown_agent() {
    let mut w = world(&[manual()]);
    for mv in [
        Move::Start { agent: agent(7) },
        Move::SwitchOn { agent: agent(7) },
        Move::SwitchOff { agent: agent(7) },
        Move::Check {
            agent: agent(7),
            answer: CheckAnswer::TryAgain,
        },
    ] {
        assert_eq!(w.go(mv).answer, Err(Cause::UnknownAgent));
    }
    let unknown = RunId::parse(&format!("{:032x}", 0x77)).unwrap();
    assert_eq!(w.end_only(&unknown, None).answer, Err(Cause::State));
    assert_eq!(w.calls(), Vec::<String>::new());
}

#[test]
fn the_stored_events_of_one_agent_rise_by_one_from_its_last_seq() {
    let mut w = world(&[manual()]);
    let run = ok_run(&w.start(0));
    w.end(&run, Some(Outcome::Completed));
    let seqs: Vec<u32> = w.stored().iter().map(|e| e.seq.get()).collect();
    assert_eq!(seqs, vec![1, 2, 3, 4]);
    let kinds: Vec<_> = w.stored()[2..]
        .iter()
        .map(|e| match &e.what {
            What::RunStarted { .. } => "started",
            What::RunEnded { .. } => "ended",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds, ["started", "ended"]);
    let _ = (Answered::Ready(ReadyAnswer::Skip), Raised::Ready);
}

#[test]
fn refusal_store_not_ended() {
    let mut w = world(&[manual(), manual()]);
    let run = ok_run(&w.start(0));
    w.end_only(&run, Some(Outcome::Completed));
    w.forget();
    let after_end = w.start(0);
    assert_eq!(
        after_end.answer,
        Err(Cause::State),
        "after the runner's end"
    );
    assert_eq!(w.calls(), Vec::<String>::new(), "Runs::start is not called");
    w.store_end(&run, Some(Outcome::Completed));
    let run = ok_run(&w.start(0));
    w.go(Move::SwitchOff { agent: agent(0) });
    w.clock(T0 + 5_000, DAY);
    assert_eq!(w.tick().ended.len(), 1, "the kill ends the run");
    w.go(Move::SwitchOn { agent: agent(0) });
    assert_eq!(w.start(0).answer, Err(Cause::State), "after a kill");
    w.store_end(&run, None);
    w.ports.refuse(Method::Start, not_started());
    w.ports.refuse(Method::EndRun, PortError::Busy);
    let failed = w.start(0);
    assert_eq!(failed.answer, Err(Cause::NoService));
    w.ports.clear_refusals();
    assert_eq!(
        w.start(0).answer,
        Err(Cause::State),
        "after a failed step 8, before its end_run is acknowledged"
    );
    w.tick();
    let crashed = ok_run(&w.start(0));
    w.relife();
    w.forget();
    assert_eq!(w.start(0).answer, Err(Cause::State), "after a crash");
    assert_eq!(w.starts(), 0);
    w.store_end(&crashed, None);
    ok_run(&w.start(0));
    let queued = w.start(1);
    assert_eq!(queued.answer, Ok(Answer::Queued));
}

#[test]
fn a_start_reads_the_agent_store_from_the_owner_and_not_from_the_facts_it_kept() {
    let mut w = world(&[manual()]);
    let run = ok_run(&w.start(0));
    w.end_only(&run, Some(Outcome::Completed));
    w.board.members[0].facts.store_ended = true;
    assert_eq!(w.start(0).answer, Err(Cause::State));
    w.ports.refuse(Method::Agents, PortError::Busy);
    assert_eq!(
        w.start(0).answer,
        Err(Cause::Busy),
        "a failed read starts nothing"
    );
    w.ports.clear_refusals();
    w.store_end(&run, Some(Outcome::Completed));
    w.board.members[0].facts.store_ended = false;
    ok_run(&w.start(0));
}

#[test]
fn a_queued_start_of_an_agent_whose_run_the_store_has_not_ended_is_dropped_with_state() {
    let mut w = world(&[manual(), manual()]);
    let first = ok_run(&w.start(0));
    assert_eq!(w.start(1).answer, Ok(Answer::Queued));
    w.end_only(&first, Some(Outcome::Completed));
    let second = w.run_of(1);
    w.end_only(&second, Some(Outcome::Completed));
    w.forget();
    w.store_end(&first, Some(Outcome::Completed));
    let tick = w.tick();
    assert_eq!(tick.next, Vec::<Served>::new(), "the queue is empty");
    assert_eq!(w.start(1).answer, Err(Cause::State));
}

#[test]
fn a_tick_takes_no_start_while_the_store_has_not_ended_the_newest_run() {
    let mut w = world(&[AUTO]);
    let run = ok_run(&w.tick().started[0].clone().into_report());
    w.end_only(&run, Some(Outcome::Completed));
    w.clock(T0, DAY + 2);
    let tick = w.tick();
    assert_eq!(tick.started.len(), 1);
    assert_eq!(tick.started[0].answer, Err(Cause::State));
    assert_eq!(w.starts(), 1);
    w.store_end(&run, Some(Outcome::Completed));
    w.ports.refuse(Method::Agents, PortError::NoService);
    assert_eq!(
        w.tick().started,
        Vec::<Served>::new(),
        "no start on a failed read"
    );
    w.ports.clear_refusals();
    assert_eq!(w.tick().started[0].answer, Ok(Answer::Ok));
}

#[test]
fn check_waits_for_the_store_to_end_the_run_and_then_holds_the_agent() {
    let mut w = world(&[manual()]);
    let run = ok_run(&w.start(0));
    w.end_only(&run, Some(Outcome::EffectUnknown));
    assert_eq!(
        w.go(Move::Check {
            agent: agent(0),
            answer: CheckAnswer::TryAgain
        })
        .answer,
        Err(Cause::State),
        "no check while the store has not ended the run"
    );
    w.store_end(&run, Some(Outcome::EffectUnknown));
    assert_eq!(w.start(0).answer, Err(Cause::Hold));
    assert_eq!(
        w.go(Move::Check {
            agent: agent(0),
            answer: CheckAnswer::TryAgain
        })
        .answer,
        Ok(Answer::Ok)
    );
    ok_run(&w.start(0));
}

#[test]
fn a_stopped_run_is_killed_at_the_first_tick_after_its_grace_and_not_before() {
    let mut w = world(&[manual()]);
    let run = ok_run(&w.start(0));
    w.go(Move::SwitchOff { agent: agent(0) });
    w.forget();
    w.tick();
    assert_eq!(
        w.calls(),
        vec!["defer".to_string()],
        "a tick at the stop kills nothing"
    );
    w.clock(T0 + 4_999, DAY);
    w.forget();
    w.tick();
    assert_eq!(w.calls(), vec!["defer".to_string()]);
    assert_eq!(w.board.runs.len(), 1);
    w.clock(T0 + 5_000, DAY);
    w.forget();
    let tick = w.tick();
    assert_eq!(
        w.calls(),
        vec![
            "defer".to_string(),
            format!("kill {run}"),
            "log run_ended cancelled".into()
        ]
    );
    assert_eq!(tick.ended[0].cause, RunEnd::Cancelled);
}

#[test]
fn a_run_that_a_tick_stopped_for_a_gone_workflow_ends_deleted_whatever_its_last_frame() {
    let mut w = world(&[manual()]);
    let run = ok_run(&w.start(0));
    w.delete(0);
    assert_eq!(w.tick().stopped, vec![run.clone()]);
    let end = w.end_only(&run, Some(Outcome::Completed));
    assert_eq!(end.ended[0].cause, RunEnd::Deleted);
    assert_eq!(
        count(&w.stored(), "run_ended"),
        1,
        "the run before the test alone"
    );
}

#[test]
fn a_stopping_run_whose_workflow_goes_takes_deleted_from_the_next_tick() {
    let mut w = world(&[manual()]);
    let run = ok_run(&w.start(0));
    w.go(Move::SwitchOff { agent: agent(0) });
    w.delete(0);
    w.forget();
    let tick = w.tick();
    assert_eq!(
        tick.stopped,
        Vec::<RunId>::new(),
        "a stopping run gets no second cancel"
    );
    assert!(!w.calls().iter().any(|c| c.starts_with("cancel")));
    let end = w.end_only(&run, None);
    assert_eq!(end.ended[0].cause, RunEnd::Deleted);
}

#[test]
fn a_refused_kill_leaves_the_run_stopping_and_a_later_tick_kills_it() {
    let mut w = world(&[manual()]);
    let run = ok_run(&w.start(0));
    w.go(Move::SwitchOff { agent: agent(0) });
    w.clock(T0 + 5_000, DAY);
    w.ports.refuse(Method::Kill, PortError::Busy);
    let tick = w.tick();
    assert!(tick.ended.is_empty());
    assert_eq!(w.board.runs.len(), 1);
    assert_eq!(count(&w.stored(), "run_ended cancelled"), 0);
    w.ports.clear_refusals();
    w.forget();
    let tick = w.tick();
    assert_eq!(tick.ended[0].cause, RunEnd::Cancelled);
    assert_eq!(w.calls()[1], format!("kill {run}"));
}

#[test]
fn creation_sends_no_state_for_a_web_agent_even_with_a_playbook() {
    let mut w = world(&[]);
    w.ports
        .workflows
        .borrow_mut()
        .push(Workflow::Full(approved(0, Category::CalendarCleanup)));
    listed_agent(
        &w,
        0,
        &minted(1),
        &A {
            runtime: "web",
            playbook: true,
            ..AUTO
        },
    );
    assert_eq!(w.go(Move::Create).created, vec![minted(1)]);
    assert_eq!(w.calls(), vec!["create_agent".to_string()]);
}

#[test]
fn an_end_whose_read_of_the_agents_fails_serves_no_start_and_the_next_tick_does() {
    let mut w = world(&[manual(), manual()]);
    let run = queue_two_behind(&mut w);
    w.ports.refuse(Method::Agents, PortError::NoService);
    w.forget();
    let end = w.end(&run, Some(Outcome::Completed));
    assert_eq!(end.ended[0].next, Vec::<Served>::new());
    assert_eq!(w.starts(), 0);
    assert_eq!(w.board.queue.len(), 1, "the start stays at the head");
    w.ports.clear_refusals();
    let tick = w.tick();
    assert_eq!(tick.next[0].agent, agent(1));
    assert_eq!(tick.next[0].answer, Ok(Answer::Ok));
}

#[test]
fn a_failed_read_of_the_agents_lets_an_earlier_step_of_a_start_keep_its_cause() {
    let mut w = world(&[manual(), manual(), manual()]);
    w.delete(0);
    w.go(Move::SwitchOff { agent: agent(1) });
    w.ports.refuse(Method::Agents, PortError::Busy);
    assert_eq!(w.start(0).answer, Err(Cause::State), "step 0");
    assert_eq!(w.start(1).answer, Err(Cause::Hold), "step 1");
    assert_eq!(w.start(2).answer, Err(Cause::Busy), "step 3 needs the read");
    assert_eq!(w.starts(), 0);
    assert!(w.board.queue.is_empty());
}

#[test]
fn a_tick_kills_in_the_order_in_which_the_runs_started() {
    let mut w = world_with(
        &[manual(), manual()],
        Limits {
            max_live_runs: 2,
            ..Limits::PRODUCT
        },
    );
    let first = ok_run(&w.start(0));
    let second = ok_run(&w.start(1));
    w.go(Move::SwitchOff { agent: agent(0) });
    w.go(Move::SwitchOff { agent: agent(1) });
    w.end_only(&first, None);
    w.clock(T0 + 5_000, DAY);
    w.forget();
    w.tick();
    let calls = w.calls();
    let kills: Vec<_> = calls.iter().filter(|c| c.starts_with("kill")).collect();
    assert_eq!(kills, [&format!("kill {first}"), &format!("kill {second}")]);
}

#[test]
fn the_end_that_a_kill_makes_takes_its_one_start_from_the_queue() {
    let mut w = world(&[manual(), manual()]);
    let run = queue_two_behind(&mut w);
    w.go(Move::SwitchOff { agent: agent(0) });
    w.clock(T0 + 5_000, DAY);
    let tick = w.tick();
    assert_eq!(tick.ended[0].run, run);
    assert_eq!(tick.ended[0].next[0].agent, agent(1));
    assert_eq!(tick.ended[0].next[0].answer, Ok(Answer::Ok));
    assert_eq!(tick.next, Vec::<Served>::new());
    assert!(w.board.queue.is_empty());
}

#[test]
fn a_tick_whose_kill_ended_a_run_serves_no_second_start_after_a_drop() {
    let mut w = world(&[manual(), manual(), manual()]);
    queue_two_behind(&mut w);
    assert_eq!(w.start(2).answer, Ok(Answer::Queued));
    stored_at_owner(&mut w, 1, OFF, false);
    w.go(Move::SwitchOff { agent: agent(0) });
    w.clock(T0 + 5_000, DAY);
    let tick = w.tick();
    assert_eq!(tick.ended[0].next[0].answer, Err(Cause::Hold));
    assert_eq!(tick.next, Vec::<Served>::new());
    assert!(
        w.board.queue.holds(&agent(2)),
        "the start behind the dropped one waits"
    );
    assert_eq!(w.starts(), 1, "the first run alone started");
}

#[test]
fn a_start_that_leaves_the_queue_and_goes_is_no_longer_queued() {
    let mut w = world(&[manual(), manual()]);
    let run = queue_two_behind(&mut w);
    let end = w.end(&run, Some(Outcome::Completed));
    assert_eq!(end.ended[0].next[0].answer, Ok(Answer::Ok));
    assert!(!w.board.queue.holds(&agent(1)));
    assert!(matches!(
        w.facts(1).doing,
        Doing::Live(halvo_coord::types::Live::Starting)
    ));
}

fn not_started() -> PortError {
    PortError::Refused(Name::parse("not_started").unwrap())
}

fn last_record(w: &World, n: usize) -> Option<(RunId, Outcome, u32)> {
    w.ports
        .agents
        .borrow()
        .iter()
        .find(|a| a.agent == agent(n))
        .and_then(|a| {
            a.last
                .as_ref()
                .map(|l| (l.run.clone(), l.outcome, l.unknown))
        })
}

#[test]
fn refusal_after_failed_step7() {
    let mut w = world(&[manual()]);
    w.ports.refuse(Method::Send, PortError::Busy);
    let failed = w.start(0);
    assert_eq!(failed.answer, Err(Cause::NoService));
    let run = failed.run.clone().unwrap();
    assert_eq!(
        w.calls(),
        ["defer", "log run_started", "log run_ended failed"],
        "no start_run and no Runs::start were taken; the refused end_run is owed"
    );
    w.ports.clear_refusals();
    w.forget();
    assert_eq!(
        w.start(0).answer,
        Err(Cause::State),
        "before the acknowledgement"
    );
    assert_eq!(w.starts(), 0);
    w.tick();
    assert_eq!(w.calls(), [format!("end_run {run}")]);
    assert_eq!(last_record(&w, 0), Some((run, Outcome::Failed, 0)));
    w.forget();
    w.tick();
    assert_eq!(
        w.calls(),
        Vec::<String>::new(),
        "sent until acknowledged, and no more"
    );
    ok_run(&w.start(0));
}

#[test]
fn refusal_after_failed_step8() {
    let mut w = world(&[manual()]);
    w.ports.refuse(Method::Start, not_started());
    w.ports.refuse(Method::EndRun, PortError::Busy);
    let failed = w.start(0);
    assert_eq!(failed.answer, Err(Cause::NoService));
    let run = failed.run.clone().unwrap();
    assert_eq!(
        w.calls(),
        [
            "defer",
            "log run_started",
            "start_run",
            "log run_ended failed"
        ]
    );
    w.ports.clear_refusals();
    w.forget();
    assert_eq!(
        w.start(0).answer,
        Err(Cause::State),
        "before the acknowledgement"
    );
    w.tick();
    assert_eq!(w.calls(), [format!("end_run {run}")]);
    ok_run(&w.start(0));
}

#[test]
fn a_failed_step_8_sends_end_run_in_its_move_and_the_next_start_goes_on() {
    let mut w = world(&[manual()]);
    w.ports.refuse(Method::Start, not_started());
    let failed = w.start(0);
    let run = failed.run.clone().unwrap();
    assert_eq!(
        w.calls(),
        vec![
            "defer".to_string(),
            "log run_started".into(),
            "start_run".into(),
            "log run_ended failed".into(),
            format!("end_run {run}"),
        ]
    );
    assert_eq!(last_record(&w, 0), Some((run, Outcome::Failed, 0)));
    w.ports.clear_refusals();
    ok_run(&w.start(0));
}

#[test]
fn a_tick_sends_what_is_owed_after_its_kills_and_before_its_agents() {
    let mut w = world_with(
        &[manual(), person(), manual()],
        Limits {
            max_live_runs: 2,
            ..Limits::PRODUCT
        },
    );
    let live = ok_run(&w.start(0));
    w.ports.refuse(Method::Start, not_started());
    w.ports.refuse(Method::EndRun, PortError::Busy);
    let failed = w.start(2).run.unwrap();
    w.ports.clear_refusals();
    w.go(Move::SwitchOff { agent: agent(0) });
    w.clock(T0 + 5_000, DAY);
    w.forget();
    w.tick();
    assert_eq!(
        w.calls(),
        vec![
            "defer".to_string(),
            format!("kill {live}"),
            "log run_ended cancelled".into(),
            format!("end_run {failed}"),
            "log item_raised ready".into(),
        ]
    );
}

#[test]
fn an_owed_end_run_of_a_workflow_that_is_gone_is_dropped() {
    let mut w = world(&[manual()]);
    w.ports.refuse(Method::Send, PortError::Busy);
    w.start(0);
    w.ports.clear_refusals();
    w.delete(0);
    w.ports.refuse(
        Method::EndRun,
        PortError::Refused(Name::parse("no_workflow").unwrap()),
    );
    let before = w.ports.tried.borrow().len();
    w.forget();
    w.tick();
    w.tick();
    assert_eq!(w.calls(), Vec::<String>::new());
    let sent = w.ports.tried.borrow()[before..]
        .iter()
        .filter(|t| matches!(t.call, Call::Send(Body::EndRun { .. })))
        .count();
    assert_eq!(
        sent, 1,
        "sent once more, and the owner's refusal no_workflow ends the debt (ruling XB-26)"
    );
    assert!(w.board.owed.is_empty());
}

#[test]
fn a_run_ended_that_the_owner_did_not_acknowledge_is_owed_and_holds_the_agent() {
    let mut w = world(&[manual()]);
    let run = ok_run(&w.start(0));
    w.ports.refuse(Method::Log, PortError::NoService);
    let end = w.end(&run, Some(Outcome::Completed));
    assert_eq!(end.ended[0].cause, RunEnd::Completed);
    w.ports.clear_refusals();
    assert_eq!(
        count(&w.stored(), "run_ended"),
        1,
        "the run before the test alone"
    );
    w.forget();
    assert_eq!(
        w.start(0).answer,
        Err(Cause::State),
        "no start while its run_ended is owed"
    );
    assert_eq!(w.calls(), Vec::<String>::new());
    w.clock(T0 + 1_000, DAY + 1);
    w.tick();
    assert_eq!(w.calls(), ["log run_ended completed"]);
    let e = w.stored().last().cloned().unwrap();
    assert_eq!(
        e.what,
        What::RunEnded {
            run,
            cause: RunEnd::Completed
        }
    );
    assert_eq!(e.day, DAY, "the day of the end");
    w.forget();
    w.tick();
    assert_eq!(w.calls(), Vec::<String>::new(), "logged once");
    ok_run(&w.start(0));
}

#[test]
fn an_owed_run_ended_is_logged_before_the_starts_of_the_tick() {
    let mut w = world(&[manual(), AUTO]);
    let run = ok_run(&w.start(0));
    w.ports.refuse(Method::Log, PortError::NoService);
    w.end(&run, Some(Outcome::Failed));
    w.ports.clear_refusals();
    w.forget();
    w.tick();
    let calls = w.calls();
    assert_eq!(calls[0], "log run_ended failed");
    assert_eq!(
        calls[1], "defer",
        "then the scheduler's start of the other agent"
    );
}

#[test]
fn an_owed_run_ended_of_a_workflow_that_is_gone_is_dropped() {
    let mut w = world(&[manual()]);
    let run = ok_run(&w.start(0));
    w.ports.refuse(Method::Log, PortError::NoService);
    w.end(&run, Some(Outcome::Completed));
    w.ports.clear_refusals();
    w.delete(0);
    w.forget();
    w.tick();
    w.tick();
    assert_eq!(w.calls(), Vec::<String>::new());
}

#[test]
fn refusal_slot_gone() {
    let mut w = world(&[AUTO]);
    w.owner_workflow(0, |f| f.last = DAY - 1);
    let tick = w.tick();
    assert_eq!(
        tick.started,
        Vec::<Served>::new(),
        "the owner's list has no slot today"
    );
    assert_eq!(
        count(&w.stored(), "run_started"),
        1,
        "the run before the test alone"
    );
    assert_eq!(w.starts(), 0);
    let mut q = world(&[manual(), AUTO]);
    let run = ok_run(&q.start(0));
    assert_eq!(q.tick().started[0].answer, Ok(Answer::Queued));
    q.owner_workflow(1, |f| f.last = DAY - 1);
    q.forget();
    let end = q.end(&run, Some(Outcome::Completed));
    assert_eq!(end.ended[0].next[0].agent, agent(1));
    assert_eq!(end.ended[0].next[0].answer, Err(Cause::State));
    assert_eq!(q.starts(), 0);
    assert!(!q.calls().iter().any(|c| c.contains("run_started")));
}

#[test]
fn a_tick_reads_the_trigger_from_the_owners_list() {
    let mut w = world(&[AUTO]);
    w.owner_workflow(0, |f| {
        f.trigger = WorkflowTrigger::Manual;
        f.period = None;
    });
    let tick = w.tick();
    assert_eq!(tick.started, Vec::<Served>::new());
    assert_eq!(w.calls(), Vec::<String>::new());
}

#[test]
fn a_tick_reads_the_configuration_from_the_owners_entry() {
    let mut w = world(&[AUTO]);
    w.owner_agent(0, |a| a.config = config(0, &person()));
    let tick = w.tick();
    assert_eq!(
        tick.started,
        Vec::<Served>::new(),
        "an agent that needs the person"
    );
    assert_eq!(tick.raised, vec![agent(0)]);
    assert_eq!(w.starts(), 0);
}

#[test]
fn a_start_reads_whether_the_agent_needs_the_model_from_the_owners_entry() {
    let mut w = world_with(
        &[model(), manual()],
        Limits {
            max_live_runs: 2,
            ..Limits::PRODUCT
        },
    );
    ok_run(&w.start(0));
    w.owner_agent(1, |a| a.config = config(1, &model()));
    assert_eq!(w.start(1).answer, Ok(Answer::Queued), "the model is held");
    assert_eq!(w.starts(), 1);
}

#[test]
fn a_start_reads_the_bindings_and_the_session_from_the_owners_entry() {
    let email = A {
        category: Category::Email,
        runtime: "graph_mail",
        slots: &[],
        ..manual()
    };
    let mut w = world(&[email]);
    assert_eq!(w.start(0).answer, Err(Cause::Hold), "setup stands");
    w.owner_agent(0, |a| {
        a.slots = names(&["reply_senders"]);
        a.session = false;
    });
    ok_run(&w.start(0));
    let headed = w.ports.calls.borrow().iter().find_map(|t| match &t.call {
        Call::Start(spec) => Some(spec.headed),
        _ => None,
    });
    assert_eq!(headed, Some(true), "no stored session");
    assert_eq!(halvo_coord::schedule::holds(w.facts(0)), Vec::<Hold>::new());
}

#[test]
fn refusal_agents_unread() {
    let mut w = world(&[person()]);
    w.ports.refuse(Method::Agents, PortError::NoService);
    let tick = w.tick();
    assert_eq!(tick.raised, Vec::<AgentId>::new());
    assert_eq!(w.calls(), Vec::<String>::new());
    w.ports.clear_refusals();
    assert_eq!(w.tick().raised, vec![agent(0)]);
}

fn started_on(w: &World, run: &RunId) -> (u32, Option<u32>) {
    w.stored()
        .iter()
        .find_map(|e| match &e.what {
            What::RunStarted { run: r, slot, .. } if r == run => Some((e.day, *slot)),
            _ => None,
        })
        .unwrap()
}

#[test]
fn a_start_of_the_person_reads_the_day_first() {
    let mut w = world(&[AUTO]);
    let first = ok_run(&w.tick().started[0].clone().into_report());
    w.end(&first, Some(Outcome::Completed));
    w.clock(T0 + 1_000, DAY + 2);
    let run = ok_run(&w.start(0));
    assert_eq!(started_on(&w, &run), (DAY + 2, Some(DAY + 2)));
    w.end(&run, Some(Outcome::Completed));
    assert_eq!(
        w.tick().started,
        Vec::<Served>::new(),
        "no second run in the slot of the clock's day"
    );
}

#[test]
fn an_end_reads_the_day_first() {
    let mut w = world(&[manual(), manual()]);
    let run = queue_two_behind(&mut w);
    w.clock(T0 + 1_000, DAY + 1);
    let end = w.end(&run, Some(Outcome::Completed));
    let ended_day = w
        .stored()
        .iter()
        .find_map(|e| match &e.what {
            What::RunEnded { run: r, .. } if *r == run => Some(e.day),
            _ => None,
        })
        .unwrap();
    assert_eq!(ended_day, DAY + 1);
    let next = end.ended[0].next[0].run.clone().unwrap();
    assert_eq!(started_on(&w, &next).0, DAY + 1);
}

#[test]
fn a_failed_read_of_the_agents_answers_its_cause_where_the_entry_or_the_store_decides() {
    let mut w = world(&[manual(); 6]);
    let r5 = ok_run(&w.start(5));
    w.end_only(&r5, Some(Outcome::Completed));
    ok_run(&w.start(0));
    for n in 1..=4 {
        assert_eq!(w.start(n).answer, Ok(Answer::Queued));
    }
    assert_eq!(
        w.start(5).answer,
        Err(Cause::State),
        "the store has not ended its run"
    );
    w.ports.refuse(Method::Agents, PortError::Busy);
    assert_eq!(
        w.start(5).answer,
        Err(Cause::Busy),
        "not step 4's queue_full"
    );
    let email = A {
        category: Category::Email,
        runtime: "graph_mail",
        slots: &[],
        ..manual()
    };
    let mut e = world(&[email]);
    e.ports.refuse(Method::Agents, PortError::NoService);
    assert_eq!(
        e.start(0).answer,
        Err(Cause::NoService),
        "the hold setup comes from the entry, which was not read"
    );
    e.go(Move::SwitchOff { agent: agent(0) });
    assert_eq!(
        e.start(0).answer,
        Err(Cause::Hold),
        "the hold off comes from the stored events"
    );
}

#[test]
fn a_web_agent_with_a_playbook_starts_in_replay_and_with_a_session_not_headed() {
    let mut w = world(&[A {
        runtime: "web",
        playbook: true,
        ..manual()
    }]);
    ok_run(&w.start(0));
    let calls = w.ports.calls.borrow();
    let modes: Vec<(Mode, Option<bool>)> = calls
        .iter()
        .filter_map(|t| match &t.call {
            Call::Send(Body::StartRun { mode, .. }) => Some((*mode, None)),
            Call::Start(spec) => Some((spec.mode, Some(spec.headed))),
            _ => None,
        })
        .collect();
    assert_eq!(modes, [(Mode::Replay, None), (Mode::Replay, Some(false))]);
}

#[test]
fn with_two_live_runs_a_tick_defers_until_the_end_of_the_longer() {
    let mut w = world_with(
        &[manual(), manual()],
        Limits {
            max_live_runs: 2,
            ..Limits::PRODUCT
        },
    );
    ok_run(&w.start(0));
    ok_run(&w.start(1));
    let longer = w.board.runs[1].defer_ms + 60_000;
    w.board.runs[1].defer_ms = longer;
    w.forget();
    w.tick();
    let defers: Vec<u64> = w
        .ports
        .calls
        .borrow()
        .iter()
        .filter_map(|t| match t.call {
            Call::Defer(until) => Some(until),
            _ => None,
        })
        .collect();
    assert_eq!(defers, [T0 + longer]);
}

#[test]
fn a_tick_whose_read_of_the_agents_fails_takes_no_start_from_the_queue() {
    let mut w = world(&[manual(), manual(), manual()]);
    let run = queue_two_behind(&mut w);
    assert_eq!(w.start(2).answer, Ok(Answer::Queued));
    stored_at_owner(&mut w, 1, OFF, false);
    let end = w.end(&run, Some(Outcome::Completed));
    assert_eq!(end.ended[0].next[0].answer, Err(Cause::Hold));
    assert!(w.board.queue.holds(&agent(2)) && w.board.runs.is_empty());
    w.ports.refuse(Method::Agents, PortError::NoService);
    w.forget();
    let tick = w.tick();
    assert_eq!(tick.next, Vec::<Served>::new());
    assert_eq!(w.calls(), Vec::<String>::new());
    assert!(w.board.queue.holds(&agent(2)));
    w.ports.clear_refusals();
    assert_eq!(w.tick().next[0].answer, Ok(Answer::Ok));
}

#[test]
fn a_second_switch_off_does_not_stop_a_stopping_run_again() {
    let mut w = world(&[manual()]);
    let run = ok_run(&w.start(0));
    w.go(Move::SwitchOff { agent: agent(0) });
    w.go(Move::SwitchOn { agent: agent(0) });
    w.clock(T0 + 3_000, DAY);
    w.forget();
    let again = w.go(Move::SwitchOff { agent: agent(0) });
    assert_eq!(again.stopped, Vec::<RunId>::new());
    assert!(!w.calls().iter().any(|c| c.starts_with("cancel")));
    w.clock(T0 + 5_000, DAY);
    w.forget();
    w.tick();
    assert!(
        w.calls().contains(&format!("kill {run}")),
        "the kill comes cancel_grace_s after the first stop"
    );
}

fn approved_listed(w: &World, n: usize) {
    w.owner_workflow(n, |f| f.state = WorkflowState::Approved);
}

fn set_states(w: &World) -> Vec<String> {
    w.calls()
        .into_iter()
        .filter(|c| c.starts_with("set_state"))
        .collect()
}

#[test]
fn an_agent_of_a_category_with_no_parameter_becomes_active_at_the_tick_after_its_creation() {
    let mut w = world(&[]);
    w.ports
        .workflows
        .borrow_mut()
        .push(Workflow::Full(approved(0, Category::CalendarCleanup)));
    listed_agent(
        &w,
        0,
        &minted(1),
        &A {
            runtime: "graph_calendar",
            ..AUTO
        },
    );
    w.go(Move::Create);
    assert_eq!(set_states(&w), Vec::<String>::new());
    w.owner_workflow(0, |f| f.agent = Some(minted(1)));
    w.forget();
    w.tick();
    assert_eq!(set_states(&w), ["set_state active"]);
    w.owner_workflow(0, |f| f.state = WorkflowState::Active);
    w.forget();
    w.tick();
    assert_eq!(
        set_states(&w),
        Vec::<String>::new(),
        "the list shows the agent active"
    );
}

#[test]
fn a_lost_acknowledgement_of_active_is_followed_by_a_second_send_at_the_next_tick() {
    let mut w = world(&[manual()]);
    approved_listed(&w, 0);
    w.ports.refuse(Method::Send, PortError::Busy);
    w.tick();
    assert_eq!(set_states(&w), Vec::<String>::new());
    w.ports.clear_refusals();
    w.ports.refuse(Method::Agents, PortError::NoService);
    w.forget();
    w.tick();
    assert_eq!(
        set_states(&w),
        Vec::<String>::new(),
        "no send on a failed read of the agents"
    );
    w.ports.clear_refusals();
    w.forget();
    w.tick();
    assert_eq!(set_states(&w), ["set_state active"]);
    w.forget();
    w.tick();
    assert_eq!(
        set_states(&w),
        ["set_state active"],
        "until the list shows it active"
    );
}

#[test]
fn a_web_agent_becomes_active_only_after_a_completed_run() {
    let web = A {
        runtime: "web",
        playbook: false,
        finished: false,
        slotted: false,
        ..AUTO
    };
    let mut w = world(&[web]);
    approved_listed(&w, 0);
    w.tick();
    assert_eq!(set_states(&w), Vec::<String>::new(), "no completed run yet");
    let run = ok_run(&w.start(0));
    w.end(&run, Some(Outcome::Declined));
    w.forget();
    w.tick();
    assert_eq!(
        set_states(&w),
        Vec::<String>::new(),
        "its last run did not complete"
    );
    w.clock(T0, DAY + 1);
    let run = ok_run(&w.start(0));
    w.end(&run, Some(Outcome::Completed));
    w.owner_agent(0, |a| {
        a.config = config(
            0,
            &A {
                playbook: true,
                ..web
            },
        )
    });
    w.forget();
    w.tick();
    assert_eq!(
        set_states(&w),
        ["set_state active"],
        "the store holds the playbook of the completed compile run"
    );
}

#[test]
fn an_agent_on_which_a_hold_stands_gets_no_active() {
    let email = A {
        category: Category::Email,
        runtime: "graph_mail",
        slots: &[],
        ..manual()
    };
    let none = A {
        runtime: "none",
        category: Category::RideOrder,
        ..manual()
    };
    let mut w = world(&[email, none, manual()]);
    for n in 0..3 {
        approved_listed(&w, n);
    }
    w.go(Move::SwitchOff { agent: agent(2) });
    w.forget();
    w.tick();
    assert_eq!(
        set_states(&w),
        Vec::<String>::new(),
        "setup, not_yet and off"
    );
}

fn next_minted(w: &mut World) -> RunId {
    w.start(w.spec.len() - 1).run.unwrap()
}

#[test]
fn refusal_create_again() {
    let mut w = world(&[manual()]);
    w.ports
        .workflows
        .borrow_mut()
        .push(Workflow::Full(approved(1, Category::CalendarCleanup)));
    listed_agent(
        &w,
        1,
        &minted(2),
        &A {
            runtime: "graph_calendar",
            ..AUTO
        },
    );
    w.ports.refuse(Method::Send, PortError::Busy);
    assert_eq!(w.go(Move::Create).created, Vec::<AgentId>::new());
    w.ports.clear_refusals();
    w.forget();
    assert_eq!(
        w.go(Move::Create).created,
        vec![minted(2)],
        "the body goes again"
    );
    assert_eq!(w.calls(), ["create_agent"]);
    assert_eq!(w.board.members.len(), 2, "the agent joined the board");
    w.forget();
    assert_eq!(
        w.go(Move::Create).created,
        Vec::<AgentId>::new(),
        "the owner refuses a second agent for the workflow with binding"
    );
    assert_eq!(
        next_minted(&mut w),
        RunId::parse(&format!("{:032x}", 4)).unwrap(),
        "while the list shows no agent the creation made an id and sent the body again"
    );
    let run = w.run_of(0);
    w.end(&run, Some(Outcome::Completed));
    w.owner_workflow(1, |f| f.agent = Some(minted(2)));
    w.forget();
    assert_eq!(w.go(Move::Create).created, Vec::<AgentId>::new());
    assert_eq!(
        next_minted(&mut w),
        RunId::parse(&format!("{:032x}", 5)).unwrap(),
        "the list shows the agent: no id was made and nothing was sent"
    );
    assert!(
        !w.calls().iter().any(|c| c.starts_with("set_state")),
        "no set_state in a creation"
    );
    let stored = |n: u128| Body::CreateAgent {
        workflow: flow(1),
        agent: minted(n),
    };
    assert!(
        w.ports.send(&stored(2)).is_ok(),
        "a repeat of the stored pair"
    );
    assert_eq!(
        w.ports.send(&stored(3)),
        Err(PortError::Refused(Name::parse("binding").unwrap())),
        "the refused body stored nothing: another agent stays refused"
    );
    assert_eq!(
        Category::ALL.len(),
        10,
        "the type of a category holds the ten seeds alone"
    );
    assert_eq!(
        Category::parse("tax_filing"),
        Err(Cause::Malformed),
        "so the read of the owner's list refuses a workflow of another category"
    );
}

#[test]
fn a_tick_sends_active_after_what_is_owed_and_before_any_start() {
    let mut w = world(&[manual(), AUTO]);
    approved_listed(&w, 1);
    w.ports.refuse(Method::Start, not_started());
    w.ports.refuse(Method::EndRun, PortError::Busy);
    let failed = w.start(0).run.unwrap();
    w.ports.clear_refusals();
    w.owner_workflow(1, |f| f.state = WorkflowState::Approved);
    w.forget();
    w.tick();
    let calls = w.calls();
    let owed = calls
        .iter()
        .position(|c| *c == format!("end_run {failed}"))
        .unwrap();
    let active = calls.iter().position(|c| c == "set_state active").unwrap();
    let start = calls.iter().position(|c| c == "log run_started").unwrap();
    assert!(owed < active && active < start, "{calls:?}");
}

#[test]
fn switch_on_sends_active_where_the_rule_gives_it_and_approved_otherwise() {
    let none = A {
        runtime: "none",
        category: Category::RideOrder,
        ..manual()
    };
    let email = A {
        category: Category::Email,
        runtime: "graph_mail",
        slots: &[],
        ..manual()
    };
    let web_done = A {
        runtime: "web",
        playbook: true,
        ..manual()
    };
    let mut w = world(&[none, email, web_done, manual()]);
    for n in 0..4 {
        w.go(Move::SwitchOff { agent: agent(n) });
    }
    let mut got = Vec::new();
    for n in 0..4 {
        w.forget();
        w.go(Move::SwitchOn { agent: agent(n) });
        got.push(set_states(&w));
    }
    assert_eq!(
        got,
        [
            vec!["set_state approved".to_string()],
            vec!["set_state approved".to_string()],
            vec!["set_state active".to_string()],
            vec!["set_state active".to_string()],
        ],
        "runtime none and a hold get approved; a web agent whose last run completed gets active"
    );
}

#[test]
fn a_tick_sends_active_before_the_start_that_it_serves_from_the_queue() {
    let mut w = world(&[manual(), manual(), manual(), manual()]);
    let run = queue_two_behind(&mut w);
    assert_eq!(w.start(2).answer, Ok(Answer::Queued));
    stored_at_owner(&mut w, 1, OFF, false);
    w.end(&run, Some(Outcome::Completed));
    approved_listed(&w, 3);
    w.forget();
    let tick = w.tick();
    assert_eq!(tick.next[0].agent, agent(2));
    let calls = w.calls();
    let active = calls.iter().position(|c| c == "set_state active").unwrap();
    let start = calls.iter().position(|c| c == "log run_started").unwrap();
    assert!(active < start, "{calls:?}");
}

#[test]
fn refusal_active_tick() {
    let none = A {
        runtime: "none",
        category: Category::RideOrder,
        ..manual()
    };
    let email = A {
        category: Category::Email,
        runtime: "graph_mail",
        slots: &[],
        ..manual()
    };
    let web = A {
        runtime: "web",
        playbook: false,
        finished: false,
        ..manual()
    };
    let mut w = world(&[none, email, web, manual()]);
    for n in 0..4 {
        approved_listed(&w, n);
    }
    w.ports.refuse(Method::Send, PortError::Busy);
    w.forget();
    w.tick();
    assert_eq!(set_states(&w), Vec::<String>::new(), "no acknowledgement");
    w.ports.clear_refusals();
    let web_run = ok_run(&w.start(2));
    w.forget();
    w.end(&web_run, Some(Outcome::Declined));
    assert_eq!(
        set_states(&w),
        Vec::<String>::new(),
        "none at the end of a run"
    );
    for _ in 0..2 {
        w.forget();
        w.tick();
        let sent: Vec<_> = w
            .ports
            .calls
            .borrow()
            .iter()
            .filter_map(|c| match &c.call {
                Call::Send(Body::SetState {
                    agent: a, state, ..
                }) => Some((a.clone(), *state)),
                _ => None,
            })
            .collect();
        assert_eq!(
            sent,
            [(agent(3), AgentState::Active)],
            "the last agent alone"
        );
    }
    approved_listed(&w, 3);
    w.owner_workflow(3, |f| f.state = WorkflowState::Active);
    w.forget();
    w.tick();
    assert_eq!(
        set_states(&w),
        Vec::<String>::new(),
        "the list shows it active"
    );
    let run = ok_run(&w.start(3));
    w.forget();
    w.end(&run, Some(Outcome::Completed));
    assert_eq!(
        set_states(&w),
        Vec::<String>::new(),
        "none at the end of a run"
    );
}

/// The end of the local day `DAY` in the zone of `world`.
const MIDNIGHT: u64 = T0 + 6_400_000;

/// The rows that the moves of a test stored, the two rows of an earlier run that `world_with`
/// put in first left out: each carries the local day of its `at` (I7.1). Gives their days.
fn tied_days(w: &World) -> Vec<u32> {
    let mut days = Vec::new();
    for e in w.stored().iter().filter(|e| e.at != 0) {
        let at = u64::try_from(e.at / 1_000_000).unwrap();
        assert_eq!(
            halvo_coord::ports::Clock::day(&w.ports, at),
            Ok(e.day),
            "{}",
            what_name(&e.what)
        );
        days.push(e.day);
    }
    days
}

#[test]
fn no_row_of_a_move_after_midnight_carries_the_day_of_the_last_tick() {
    let mut w = world(&[manual()]);
    w.tick();
    assert_eq!(w.board.day, DAY);
    w.ports.set_now(MIDNIGHT + 1_000);
    w.go(Move::SwitchOff { agent: agent(0) });
    w.go(Move::SwitchOn { agent: agent(0) });
    let run = ok_run(&w.start(0));
    w.end(&run, Some(Outcome::Completed));
    w.tick();
    let days = tied_days(&w);
    assert_eq!(
        days,
        [DAY + 1; 4],
        "hold_set, hold_cleared, run_started, run_ended"
    );
}

#[test]
fn the_rows_of_a_tick_a_start_and_an_end_carry_the_day_of_their_at_across_midnight() {
    let mut days = Vec::new();
    for start in MIDNIGHT - 8..MIDNIGHT + 4 {
        let mut w = world(&[AUTO]);
        w.ports.set_now(start);
        w.ports.step_reads(1);
        let r = w.tick();
        assert_eq!(r.started.len(), 1, "the tick from {start} starts its slot");
        let run = w.run_of(0);
        w.end(&run, Some(Outcome::Completed));
        let mut v = world(&[manual()]);
        v.ports.set_now(start);
        v.ports.step_reads(1);
        let run = ok_run(&v.start(0));
        v.end(&run, None);
        for x in [&w, &v] {
            let tied = tied_days(x);
            assert_eq!(tied.len(), 2, "run_started and run_ended from {start}");
            days.extend(tied);
        }
    }
    assert!(days.contains(&DAY) && days.contains(&(DAY + 1)));
}

/// The end of a run takes the coordinator's day first (I5.9): a run that started on `DAY` ends
/// after local midnight with a start waiting in the queue, and the start that the end serves
/// finds the board's day and its facts' day at the new day, and stores its row on it (ruling
/// RD-69 E4).
#[test]
fn an_end_after_midnight_takes_the_day_first_for_the_start_that_it_serves() {
    let mut w = world(&[manual(), manual()]);
    let run = queue_two_behind(&mut w);
    assert_eq!(w.board.day, DAY);
    w.ports.set_now(MIDNIGHT + 1_000);
    let end = w.end(&run, Some(Outcome::Completed));
    let next = &end.ended[0].next;
    assert_eq!(next.len(), 1);
    assert_eq!(
        (next[0].agent.clone(), next[0].answer),
        (agent(1), Ok(Answer::Ok))
    );
    assert_eq!(w.board.day, DAY + 1, "the board's day");
    assert_eq!(
        w.facts(1).today,
        DAY + 1,
        "the facts' day of the served start"
    );
    let started: Vec<u32> = w
        .stored()
        .iter()
        .filter(|e| e.agent == agent(1) && e.at != 0 && matches!(e.what, What::RunStarted { .. }))
        .map(|e| e.day)
        .collect();
    assert_eq!(started, [DAY + 1]);
}

/// Stores `what` for agent `n` at the owner after its last event, and, where `seen`, in the
/// board's events too.
fn stored_at_owner(w: &mut World, n: usize, what: What, seen: bool) {
    let last = w
        .stored()
        .iter()
        .filter(|e| e.agent == agent(n))
        .map(|e| e.seq.get())
        .max()
        .unwrap_or(0);
    let event = Event {
        workflow: flow(n),
        agent: agent(n),
        seq: NonZeroU32::new(last + 1).unwrap(),
        day: DAY,
        at: 1,
        what,
    };
    w.ports.events.borrow_mut().push(event.clone());
    if seen {
        let i = w
            .board
            .members
            .iter()
            .position(|m| m.facts.agent == agent(n))
            .unwrap();
        w.board.members[i].facts.events.push(event);
    }
}

const OFF: What = What::HoldSet {
    cause: HoldSetCause::Person,
};

/// RD-52 (b) rule 4, at the drive: for an agent whose hold `off` stands in the board's events,
/// `Move::SwitchOff` answers `state` where no live run stands that a stop can reach, with no run
/// and with a stopping one; where a live run stands it reads the owner's events first: with the
/// hold standing it stores no second `hold_set` and stops the run; with the hold ended by a
/// switch-on whose acknowledgement was lost it stores `hold_set` as a new press and stops the
/// run; with the read failing it answers the read's cause and does nothing.
#[test]
fn rd52b_rule_4_the_switch_offs_guard_answers_by_a_read_where_a_live_run_stands() {
    let mut w = world(&[manual()]);
    stored_at_owner(&mut w, 0, OFF, true);
    w.forget();
    assert_eq!(
        w.go(Move::SwitchOff { agent: agent(0) }).answer,
        Err(Cause::State),
        "off with no run"
    );
    assert!(w.calls().is_empty());
    for case in ["standing", "ended", "read_fails"] {
        let mut w = world(&[manual()]);
        let run = ok_run(&w.start(0));
        stored_at_owner(&mut w, 0, OFF, true);
        if case == "ended" {
            stored_at_owner(
                &mut w,
                0,
                What::HoldCleared {
                    cause: Cleared::Person,
                },
                false,
            );
        }
        if case == "read_fails" {
            w.ports.refuse(Method::History, PortError::NoService);
        }
        w.forget();
        let r = w.go(Move::SwitchOff { agent: agent(0) });
        w.ports.clear_refusals();
        let expected: Vec<String> = match case {
            "standing" => vec![format!("cancel {run}"), "set_state paused".into()],
            "ended" => vec![
                "log hold_set person".into(),
                format!("cancel {run}"),
                "set_state paused".into(),
            ],
            _ => vec![],
        };
        assert_eq!(w.calls(), expected, "{case}");
        if case == "read_fails" {
            assert_eq!(r.answer, Err(Cause::NoService), "{case}");
            assert!(r.stopped.is_empty(), "{case}");
            continue;
        }
        assert_eq!(r.answer, Ok(Answer::Ok), "{case}");
        assert_eq!(r.stopped, vec![run.clone()], "{case}");
        assert_eq!(
            w.go(Move::SwitchOff { agent: agent(0) }).answer,
            Err(Cause::State),
            "{case}: off with a stopping run"
        );
    }
}

/// RD-52 (a), left to U2.4b: the tick's raise of a `ready` item whose log meets `seq` and whose
/// read shows a hold raises no item: no `item_raised` row, and the agent holds no item.
#[test]
fn rd52a_the_ticks_raise_of_a_ready_item_whose_read_shows_a_hold_raises_nothing() {
    let mut w = world(&[person()]);
    stored_at_owner(&mut w, 0, OFF, false);
    let r = w.tick();
    assert!(r.raised.is_empty(), "{r:?}");
    assert!(
        !w.stored()
            .iter()
            .any(|e| matches!(e.what, What::ItemRaised { .. })),
        "no item_raised"
    );
    assert_eq!(w.facts(0).doing, Doing::Idle(None));
    assert!(
        holds(w.facts(0)).contains(&Hold::Off),
        "the read brought the hold"
    );
    let mut control = world(&[person()]);
    assert_eq!(
        control.tick().raised,
        vec![agent(0)],
        "the control raises it"
    );
}
