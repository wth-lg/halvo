use halvo_coord::mem::Config;
use halvo_coord::schedule::{
    active_state, needs_model, needs_person, start_step, Around, Clear, Go, Queue, StartStep,
    Verdict, Waiting,
};
use halvo_coord::types::{
    AgentId, AgentState, Category, Cause, Doing, Event, Facts, FullWorkflow, HoldSetCause, Limits,
    Live, Mode, Name, RunEnd, RunId, Trigger, What, WorkflowId, WorkflowState, WorkflowTrigger,
};
use std::num::NonZeroU32;

const AGENT: &str = "000000000000000000000000000000a1";
const FLOW: &str = "000000000000000000000000000000f1";
const TODAY: u32 = 107;

struct Spec {
    external: bool,
    tokens: u64,
    runtime: &'static str,
    playbook: bool,
}

const AUTO: Spec = Spec {
    external: false,
    tokens: 0,
    runtime: "graph_calendar",
    playbook: true,
};

fn run(n: u32) -> RunId {
    RunId::parse(&format!("{n:032x}")).unwrap()
}

fn config(s: &Spec) -> Config {
    let playbook = if s.playbook { "1" } else { "null" };
    Config::parse(&format!(
        "{{\"v\":1,\"workflow\":\"{FLOW}\",\"agent\":\"{AGENT}\",\"rev\":1,\"category\":\"calendar_cleanup\",\"tier\":\"on_device\",\"reasoning\":\"medium\",\"autonomy\":\"draft_and_confirm\",\"allow\":[],\"caps\":{{\"untrusted\":\"yes\",\"private\":\"yes\",\"external\":\"{}\"}},\"limits\":{{\"steps\":20,\"wall_s\":120,\"model_tokens\":{}}},\"budget\":null,\"permissions\":[],\"runtime\":\"{}\",\"playbook\":{playbook}}}",
        if s.external { "yes" } else { "no" },
        s.tokens,
        s.runtime
    ))
    .unwrap()
}

fn facts(spec: &Spec, whats: Vec<(u32, What)>) -> Facts<Config> {
    let events = whats
        .into_iter()
        .enumerate()
        .map(|(i, (day, what))| Event {
            workflow: WorkflowId::parse(FLOW).unwrap(),
            agent: AgentId::parse(AGENT).unwrap(),
            seq: NonZeroU32::new(i as u32 + 1).unwrap(),
            day,
            at: 0,
            what,
        })
        .collect();
    Facts {
        agent: AgentId::parse(AGENT).unwrap(),
        workflow: FullWorkflow {
            id: WorkflowId::parse(FLOW).unwrap(),
            state: WorkflowState::Active,
            category: Category::CalendarCleanup,
            site: None,
            trigger: WorkflowTrigger::Schedule,
            period: NonZeroU32::new(7),
            last: 100,
            agent: Some(AgentId::parse(AGENT).unwrap()),
        },
        config: config(spec),
        slots: Vec::<Name>::new(),
        runs: vec![],
        events,
        store_ended: true,
        today: TODAY,
        doing: Doing::Idle(None),
    }
}

fn started(n: u32, trigger: Trigger, slot: Option<u32>) -> What {
    What::RunStarted {
        run: run(n),
        trigger,
        slot,
    }
}

fn ended(n: u32, cause: RunEnd) -> What {
    What::RunEnded { run: run(n), cause }
}

fn done_before() -> Vec<(u32, What)> {
    vec![
        (90, started(1, Trigger::Person, None)),
        (90, ended(1, RunEnd::Completed)),
    ]
}

fn free() -> Around {
    Around {
        listed: true,
        live: 0,
        model_held: false,
        queued: 0,
        session: true,
        from_queue: false,
        owed: false,
    }
}

fn cause(step: &StartStep) -> Option<Cause> {
    match step.verdict {
        Verdict::Refused(c) => Some(c),
        Verdict::Queued | Verdict::Go(_) => None,
    }
}

fn go(step: &StartStep) -> Go {
    match &step.verdict {
        Verdict::Go(g) => g.clone(),
        other => panic!("no go: {other:?}"),
    }
}

const L: Limits = Limits::PRODUCT;

#[test]
fn step_0_a_workflow_that_is_not_listed_is_state() {
    let f = facts(&AUTO, done_before());
    let s = start_step(
        &f,
        Trigger::Person,
        &L,
        &Around {
            listed: false,
            ..free()
        },
    );
    assert_eq!(cause(&s), Some(Cause::State));
    assert_eq!(s.clear, None);
}

#[test]
fn step_1_each_hold_is_hold() {
    let mut off = done_before();
    off.push((
        100,
        What::HoldSet {
            cause: HoldSetCause::Person,
        },
    ));
    assert_eq!(
        cause(&start_step(
            &facts(&AUTO, off),
            Trigger::Person,
            &L,
            &free()
        )),
        Some(Cause::Hold)
    );
    let none = facts(
        &Spec {
            runtime: "none",
            ..AUTO
        },
        done_before(),
    );
    assert_eq!(
        cause(&start_step(&none, Trigger::Person, &L, &free())),
        Some(Cause::Hold)
    );
    let mut setup = facts(&AUTO, done_before());
    setup.workflow.category = Category::Email;
    assert_eq!(
        cause(&start_step(&setup, Trigger::Person, &L, &free())),
        Some(Cause::Hold)
    );
}

#[test]
fn step_2_starts_per_day_run_started_events_of_today_is_budget() {
    let mut whats = done_before();
    for n in 0..6 {
        whats.push((TODAY, started(10 + n, Trigger::Person, None)));
        whats.push((TODAY, ended(10 + n, RunEnd::Failed)));
    }
    let f = facts(&AUTO, whats.clone());
    assert_eq!(
        cause(&start_step(&f, Trigger::Person, &L, &free())),
        Some(Cause::Budget)
    );
    whats.truncate(whats.len() - 2);
    let g = facts(&AUTO, whats);
    assert!(matches!(
        start_step(&g, Trigger::Person, &L, &free()).verdict,
        Verdict::Go(_)
    ));
    let mut yesterday = done_before();
    for n in 0..6 {
        yesterday.push((TODAY - 1, started(10 + n, Trigger::Person, None)));
    }
    let y = facts(&AUTO, yesterday);
    assert!(matches!(
        start_step(&y, Trigger::Person, &L, &free()).verdict,
        Verdict::Go(_)
    ));
}

#[test]
fn step_3_a_live_or_queued_run_of_the_agent_is_state() {
    let mut live = facts(&AUTO, done_before());
    live.doing = Doing::Live(Live::Working);
    assert_eq!(
        cause(&start_step(&live, Trigger::Person, &L, &free())),
        Some(Cause::State)
    );
    live.doing = Doing::Live(Live::Stopping);
    assert_eq!(
        cause(&start_step(&live, Trigger::Person, &L, &free())),
        Some(Cause::State)
    );
    let mut queued = facts(&AUTO, done_before());
    queued.doing = Doing::Queued(None);
    assert_eq!(
        cause(&start_step(&queued, Trigger::Person, &L, &free())),
        Some(Cause::State)
    );
    let leaving = start_step(
        &queued,
        Trigger::Person,
        &L,
        &Around {
            from_queue: true,
            ..free()
        },
    );
    assert!(
        matches!(leaving.verdict, Verdict::Go(_)),
        "its own start leaves the queue"
    );
}

#[test]
fn step_4_no_room_or_the_model_held_queues_and_a_full_queue_is_queue_full() {
    let model = Spec {
        tokens: 1000,
        ..AUTO
    };
    let f = facts(&model, done_before());
    let two = Limits {
        max_live_runs: 2,
        ..L
    };
    let held = Around {
        live: 1,
        model_held: true,
        ..free()
    };
    assert_eq!(
        start_step(&f, Trigger::Person, &two, &held).verdict,
        Verdict::Queued
    );
    let other = facts(&AUTO, done_before());
    assert!(
        matches!(
            start_step(&other, Trigger::Person, &two, &held).verdict,
            Verdict::Go(_)
        ),
        "an agent that needs no model starts beside the holder"
    );
    let full_room = Around { live: 1, ..free() };
    assert_eq!(
        start_step(&other, Trigger::Person, &L, &full_room).verdict,
        Verdict::Queued
    );
    let full_queue = Around {
        live: 1,
        queued: 4,
        ..free()
    };
    assert_eq!(
        cause(&start_step(&other, Trigger::Person, &L, &full_queue)),
        Some(Cause::QueueFull)
    );
    let three = Around {
        live: 1,
        queued: 3,
        ..free()
    };
    assert_eq!(
        start_step(&other, Trigger::Person, &L, &three).verdict,
        Verdict::Queued
    );
    let leaving = Around {
        live: 1,
        queued: 4,
        from_queue: true,
        ..free()
    };
    assert_eq!(
        start_step(&other, Trigger::Person, &L, &leaving).verdict,
        Verdict::Queued,
        "a start that leaves the queue stays at its head and is never queue_full"
    );
}

fn failing(steps: &[u8]) -> (Facts<Config>, Around) {
    let mut whats = done_before();
    let mut around = free();
    for step in steps {
        match step {
            0 => around.listed = false,
            1 => whats.push((
                100,
                What::HoldSet {
                    cause: HoldSetCause::Failures,
                },
            )),
            2 => {
                for n in 0..6 {
                    whats.push((TODAY, started(10 + n, Trigger::Schedule, None)));
                    whats.push((TODAY, ended(10 + n, RunEnd::Cancelled)));
                }
            }
            3 => {}
            4 => {
                around.live = 1;
                around.queued = 4;
            }
            _ => unreachable!(),
        }
    }
    let mut f = facts(&AUTO, whats);
    if steps.contains(&3) {
        f.doing = Doing::Live(Live::Working);
    }
    (f, around)
}

#[test]
fn an_earlier_step_that_fails_wins_over_every_later_one() {
    let causes = [
        Cause::State,
        Cause::Hold,
        Cause::Budget,
        Cause::State,
        Cause::QueueFull,
    ];
    for first in 0..5u8 {
        let (f, a) = failing(&[first]);
        let alone = start_step(&f, Trigger::Schedule, &L, &a);
        assert_eq!(
            cause(&alone),
            Some(causes[first as usize]),
            "step {first} alone"
        );
        for later in first + 1..5 {
            let (f, a) = failing(&[first, later]);
            let s = start_step(&f, Trigger::Schedule, &L, &a);
            assert_eq!(
                cause(&s),
                Some(causes[first as usize]),
                "steps {first} and {later}"
            );
        }
    }
}

#[test]
fn step_0_comes_before_the_end_of_failures_and_step_3_after_it() {
    let (gone, a) = failing(&[0, 1, 3]);
    let s = start_step(&gone, Trigger::Person, &L, &a);
    assert_eq!((cause(&s), s.clear), (Some(Cause::State), None));
    let (live, a) = failing(&[1, 3]);
    let s = start_step(&live, Trigger::Person, &L, &a);
    assert_eq!(cause(&s), Some(Cause::State));
    assert_eq!(
        s.clear,
        Some(Clear {
            set_state: Some(AgentState::Active)
        }),
        "a refused start of the person ends failures all the same"
    );
}

#[test]
fn a_start_of_the_person_ends_failures_and_one_of_the_scheduler_does_not() {
    let (f, a) = failing(&[1]);
    let person = start_step(&f, Trigger::Person, &L, &a);
    assert_eq!(
        person.clear,
        Some(Clear {
            set_state: Some(AgentState::Active)
        })
    );
    assert!(matches!(person.verdict, Verdict::Go(_)));
    let schedule = start_step(&f, Trigger::Schedule, &L, &a);
    assert_eq!(
        (cause(&schedule), schedule.clear),
        (Some(Cause::Hold), None)
    );
    let leaving = start_step(
        &f,
        Trigger::Person,
        &L,
        &Around {
            from_queue: true,
            ..a
        },
    );
    assert_eq!((cause(&leaving), leaving.clear), (Some(Cause::Hold), None));
}

#[test]
fn under_off_a_start_of_the_person_ends_failures_sends_no_state_and_is_hold() {
    let (mut f, a) = failing(&[1]);
    let seq = NonZeroU32::new(f.events.len() as u32 + 1).unwrap();
    f.events.push(Event {
        seq,
        what: What::HoldSet {
            cause: HoldSetCause::Person,
        },
        ..f.events[0].clone()
    });
    let s = start_step(&f, Trigger::Person, &L, &a);
    assert_eq!(s.clear, Some(Clear { set_state: None }));
    assert_eq!(cause(&s), Some(Cause::Hold));
}

#[test]
fn the_state_after_failures_is_approved_for_web_with_no_playbook() {
    let web = Spec {
        runtime: "web",
        playbook: false,
        ..AUTO
    };
    assert_eq!(active_state(&config(&web)), AgentState::Approved);
    assert_eq!(
        active_state(&config(&Spec {
            runtime: "web",
            ..AUTO
        })),
        AgentState::Active
    );
    assert_eq!(active_state(&config(&AUTO)), AgentState::Active);
    let mut whats = done_before();
    whats.push((
        100,
        What::HoldSet {
            cause: HoldSetCause::Failures,
        },
    ));
    let s = start_step(&facts(&web, whats), Trigger::Person, &L, &free());
    assert_eq!(
        s.clear,
        Some(Clear {
            set_state: Some(AgentState::Approved)
        })
    );
}

#[test]
fn a_start_that_goes_carries_its_mode_head_model_slot_and_defer() {
    let f = facts(&AUTO, done_before());
    let g = go(&start_step(&f, Trigger::Person, &L, &free()));
    assert_eq!(
        g,
        Go {
            slot: Some(107),
            mode: Mode::Replay,
            headed: false,
            holds_model: false,
            defer_ms: (120 + 900) * 1000,
        }
    );
    let web = facts(
        &Spec {
            runtime: "web",
            playbook: false,
            tokens: 5,
            ..AUTO
        },
        done_before(),
    );
    let g = go(&start_step(&web, Trigger::Person, &L, &free()));
    assert_eq!(
        (g.mode, g.headed, g.holds_model),
        (Mode::Compile, true, true)
    );
    let nosession = go(&start_step(
        &f,
        Trigger::Person,
        &L,
        &Around {
            session: false,
            ..free()
        },
    ));
    assert_eq!((nosession.mode, nosession.headed), (Mode::Replay, true));
}

#[test]
fn needs_person_and_needs_model_read_caps_external_tier_and_model_tokens() {
    assert!(!needs_person(&config(&AUTO)));
    assert!(needs_person(&config(&Spec {
        external: true,
        ..AUTO
    })));
    assert!(!needs_model(&config(&AUTO)));
    assert!(needs_model(&config(&Spec { tokens: 1, ..AUTO })));
}

#[test]
fn the_slot_of_a_start_of_the_person_is_the_open_current_slot_of_an_agent_with_a_finished_run() {
    let f = facts(&AUTO, done_before());
    assert_eq!(
        go(&start_step(&f, Trigger::Person, &L, &free())).slot,
        Some(107)
    );
    let first = facts(&AUTO, vec![]);
    assert_eq!(
        go(&start_step(&first, Trigger::Person, &L, &free())).slot,
        None
    );
    let mut closed = done_before();
    closed.push((106, started(2, Trigger::Person, None)));
    closed.push((106, ended(2, RunEnd::NothingToDo)));
    let c = facts(&AUTO, closed);
    assert_eq!(go(&start_step(&c, Trigger::Person, &L, &free())).slot, None);
    let mut manual = facts(&AUTO, done_before());
    manual.workflow.trigger = WorkflowTrigger::Manual;
    assert_eq!(
        go(&start_step(&manual, Trigger::Person, &L, &free())).slot,
        None
    );
    let mut old = facts(&AUTO, done_before());
    old.today = 113;
    assert_eq!(
        go(&start_step(&old, Trigger::Person, &L, &free())).slot,
        Some(107),
        "a person is not asked for the age of the slot"
    );
}

#[test]
fn a_start_of_the_scheduler_needs_a_due_slot_an_agent_with_no_person_and_a_finished_run() {
    let f = facts(&AUTO, done_before());
    assert_eq!(
        go(&start_step(&f, Trigger::Schedule, &L, &free())).slot,
        Some(107)
    );
    let mut named = done_before();
    named.push((107, started(2, Trigger::Schedule, Some(107))));
    named.push((107, ended(2, RunEnd::Failed)));
    let n = facts(&AUTO, named);
    assert_eq!(
        cause(&start_step(&n, Trigger::Schedule, &L, &free())),
        Some(Cause::State)
    );
    assert!(
        matches!(
            start_step(&n, Trigger::Person, &L, &free()).verdict,
            Verdict::Go(_)
        ),
        "a person may start it again"
    );
    let mut stale = facts(&AUTO, done_before());
    stale.today = 107 + 8;
    stale.workflow.period = NonZeroU32::new(30);
    stale.workflow.last = 77;
    assert_eq!(
        cause(&start_step(&stale, Trigger::Schedule, &L, &free())),
        Some(Cause::State)
    );
    let person = facts(
        &Spec {
            external: true,
            ..AUTO
        },
        done_before(),
    );
    assert_eq!(
        cause(&start_step(&person, Trigger::Schedule, &L, &free())),
        Some(Cause::State)
    );
    let first = facts(&AUTO, vec![]);
    assert_eq!(
        cause(&start_step(&first, Trigger::Schedule, &L, &free())),
        Some(Cause::State)
    );
    let no_room = Around { live: 1, ..free() };
    let mut dead = facts(&AUTO, done_before());
    dead.events.push(Event {
        seq: NonZeroU32::new(3).unwrap(),
        day: 107,
        what: What::SlotSkipped { slot: Some(107) },
        ..dead.events[0].clone()
    });
    assert_eq!(
        cause(&start_step(&dead, Trigger::Schedule, &L, &no_room)),
        Some(Cause::State),
        "the slot rule comes before step 4"
    );
}

fn waiting(n: u8, trigger: Trigger) -> Waiting {
    Waiting {
        agent: AgentId::parse(&format!("{n:032x}")).unwrap(),
        trigger,
    }
}

#[test]
fn a_start_of_the_person_stands_before_every_start_of_the_scheduler() {
    let mut q = Queue::default();
    q.push(waiting(1, Trigger::Schedule));
    q.push(waiting(2, Trigger::Person));
    q.push(waiting(3, Trigger::Schedule));
    q.push(waiting(4, Trigger::Person));
    let order: Vec<_> = q.entries().iter().map(|w| w.agent.clone()).collect();
    assert_eq!(
        order,
        [2u8, 4, 1, 3]
            .iter()
            .map(|n| AgentId::parse(&format!("{n:032x}")).unwrap())
            .collect::<Vec<_>>()
    );
    assert_eq!(q.len(), 4);
    assert_eq!(q.head(), Some(&waiting(2, Trigger::Person)));
    assert!(q.holds(&waiting(3, Trigger::Schedule).agent));
    assert!(q.remove(&waiting(4, Trigger::Person).agent));
    assert!(!q.remove(&waiting(4, Trigger::Person).agent));
    assert_eq!(q.pop(), Some(waiting(2, Trigger::Person)));
    assert_eq!(q.pop(), Some(waiting(1, Trigger::Schedule)));
    assert_eq!(q.pop(), Some(waiting(3, Trigger::Schedule)));
    assert_eq!(q.pop(), None);
    assert!(q.is_empty());
}

#[test]
fn step_3_a_newest_run_that_the_agent_store_has_not_ended_is_state() {
    let mut f = facts(&AUTO, done_before());
    f.store_ended = false;
    assert_eq!(
        cause(&start_step(&f, Trigger::Person, &L, &free())),
        Some(Cause::State)
    );
    assert_eq!(
        cause(&start_step(&f, Trigger::Schedule, &L, &free())),
        Some(Cause::State)
    );
    let leaving = Around {
        from_queue: true,
        ..free()
    };
    assert_eq!(
        cause(&start_step(&f, Trigger::Person, &L, &leaving)),
        Some(Cause::State),
        "a start that leaves the queue is dropped"
    );
    let full = Around {
        live: 1,
        queued: 4,
        ..free()
    };
    assert_eq!(
        cause(&start_step(&f, Trigger::Person, &L, &full)),
        Some(Cause::State),
        "step 3 comes before step 4"
    );
    let (mut spent, a) = failing(&[2]);
    spent.store_ended = false;
    assert_eq!(
        cause(&start_step(&spent, Trigger::Person, &L, &a)),
        Some(Cause::Budget),
        "step 2 comes before step 3"
    );
    let (mut held, a) = failing(&[1]);
    held.store_ended = false;
    let s = start_step(&held, Trigger::Person, &L, &a);
    assert_eq!(cause(&s), Some(Cause::State));
    assert_eq!(
        s.clear,
        Some(Clear {
            set_state: Some(AgentState::Active)
        }),
        "the end of failures comes before step 3"
    );
}

#[test]
fn step_3_an_owed_run_ended_of_the_agent_is_state() {
    let f = facts(&AUTO, done_before());
    let owed = Around {
        owed: true,
        ..free()
    };
    assert_eq!(
        cause(&start_step(&f, Trigger::Person, &L, &owed)),
        Some(Cause::State)
    );
    let full = Around {
        owed: true,
        live: 1,
        queued: 4,
        ..free()
    };
    assert_eq!(
        cause(&start_step(&f, Trigger::Person, &L, &full)),
        Some(Cause::State)
    );
}
