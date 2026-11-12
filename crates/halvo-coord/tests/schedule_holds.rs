use halvo_coord::mem::Config;
use halvo_coord::schedule::{current_slot, finished, holds, next_slot, slot_due, slot_open, Hold};
use halvo_coord::types::{
    AgentId, Category, CheckAnswer, Cleared, Doing, Event, Facts, FullWorkflow, HoldSetCause,
    Limits, Name, Outcome, PastRun, RunEnd, RunId, Trigger, What, WorkflowId, WorkflowState,
    WorkflowTrigger,
};
use std::num::NonZeroU32;

const AGENT: &str = "000000000000000000000000000000a1";
const FLOW: &str = "000000000000000000000000000000f1";

fn run(n: u32) -> RunId {
    RunId::parse(&format!("{n:032x}")).unwrap()
}

fn config(category: Category, runtime: &str) -> Config {
    Config::parse(&format!(
        "{{\"v\":1,\"workflow\":\"{FLOW}\",\"agent\":\"{AGENT}\",\"rev\":1,\"category\":\"{}\",\"tier\":\"on_device\",\"reasoning\":\"medium\",\"autonomy\":\"draft_and_confirm\",\"allow\":[],\"caps\":{{\"untrusted\":\"yes\",\"private\":\"yes\",\"external\":\"no\"}},\"limits\":{{\"steps\":20,\"wall_s\":120,\"model_tokens\":0}},\"budget\":null,\"permissions\":[],\"runtime\":\"{runtime}\",\"playbook\":1}}",
        category.name()
    ))
    .unwrap()
}

fn workflow(trigger: WorkflowTrigger, period: u32, last: u32) -> FullWorkflow {
    FullWorkflow {
        id: WorkflowId::parse(FLOW).unwrap(),
        state: WorkflowState::Active,
        category: Category::CalendarCleanup,
        site: None,
        trigger,
        period: NonZeroU32::new(period),
        last,
        agent: Some(AgentId::parse(AGENT).unwrap()),
    }
}

fn facts(category: Category, runtime: &str, whats: Vec<(u32, What)>) -> Facts<Config> {
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
            category,
            ..workflow(WorkflowTrigger::Schedule, 7, 100)
        },
        config: config(category, runtime),
        slots: vec![],
        runs: vec![],
        events,
        store_ended: true,
        today: 107,
        doing: Doing::Idle(None),
    }
}

fn calendar(whats: Vec<(u32, What)>) -> Facts<Config> {
    facts(Category::CalendarCleanup, "graph_calendar", whats)
}

fn set(cause: HoldSetCause) -> What {
    What::HoldSet { cause }
}

fn cleared(cause: Cleared) -> What {
    What::HoldCleared { cause }
}

fn started(n: u32, slot: Option<u32>) -> What {
    What::RunStarted {
        run: run(n),
        trigger: Trigger::Schedule,
        slot,
    }
}

fn ended(n: u32, cause: RunEnd) -> What {
    What::RunEnded { run: run(n), cause }
}

fn names(slots: &[&str]) -> Vec<Name> {
    slots.iter().map(|s| Name::parse(s).unwrap()).collect()
}

#[test]
fn no_hold_stands_for_an_agent_with_no_hold_event() {
    assert_eq!(holds(&calendar(vec![])), Vec::<Hold>::new());
}

#[test]
fn runtime_none_is_the_hold_not_yet() {
    let f = facts(Category::RideOrder, "none", vec![]);
    assert_eq!(holds(&f), vec![Hold::NotYet]);
}

#[test]
fn off_stands_from_its_hold_set_until_a_hold_cleared_of_person() {
    let mut f = calendar(vec![(100, set(HoldSetCause::Person))]);
    assert_eq!(holds(&f), vec![Hold::Off]);
    f = calendar(vec![
        (100, set(HoldSetCause::Person)),
        (101, cleared(Cleared::Failures)),
    ]);
    assert_eq!(
        holds(&f),
        vec![Hold::Off],
        "a hold_cleared of failures ends no off"
    );
    f = calendar(vec![
        (100, set(HoldSetCause::Person)),
        (101, cleared(Cleared::Person)),
    ]);
    assert_eq!(holds(&f), Vec::<Hold>::new());
    f = calendar(vec![
        (100, set(HoldSetCause::Person)),
        (101, cleared(Cleared::Person)),
        (102, set(HoldSetCause::Person)),
    ]);
    assert_eq!(holds(&f), vec![Hold::Off], "a later hold_set stands again");
    f = calendar(vec![(101, cleared(Cleared::Person))]);
    assert_eq!(
        holds(&f),
        Vec::<Hold>::new(),
        "a hold_cleared alone sets nothing"
    );
}

#[test]
fn failures_stands_from_its_hold_set_until_a_hold_cleared_of_failures() {
    let mut f = calendar(vec![(100, set(HoldSetCause::Failures))]);
    assert_eq!(holds(&f), vec![Hold::Failures]);
    f = calendar(vec![
        (100, set(HoldSetCause::Failures)),
        (101, cleared(Cleared::Person)),
    ]);
    assert_eq!(
        holds(&f),
        vec![Hold::Failures],
        "a hold_cleared of person ends no failures"
    );
    f = calendar(vec![
        (100, set(HoldSetCause::Failures)),
        (101, cleared(Cleared::Failures)),
    ]);
    assert_eq!(holds(&f), Vec::<Hold>::new());
}

#[test]
fn off_and_failures_both_stand_in_that_order() {
    let f = calendar(vec![
        (100, set(HoldSetCause::Failures)),
        (101, set(HoldSetCause::Person)),
    ]);
    assert_eq!(holds(&f), vec![Hold::Off, Hold::Failures]);
}

#[test]
fn the_state_of_the_workflow_is_no_part_of_off_or_failures() {
    let mut f = calendar(vec![(100, set(HoldSetCause::Person))]);
    f.workflow.state = WorkflowState::Active;
    assert_eq!(holds(&f), vec![Hold::Off]);
    let mut g = calendar(vec![]);
    g.workflow.state = WorkflowState::Paused;
    assert_eq!(holds(&g), Vec::<Hold>::new());
}

fn past(n: u32, day: u32, outcome: Outcome, unknown: u32) -> PastRun {
    PastRun {
        run: run(n),
        day,
        outcome,
        unknown,
    }
}

#[test]
fn check_stands_while_the_newest_run_has_an_unknown_effect_no_answer_names() {
    let mut f = calendar(vec![]);
    f.runs = vec![
        past(2, 103, Outcome::EffectUnknown, 1),
        past(1, 100, Outcome::Completed, 0),
    ];
    assert_eq!(holds(&f), vec![Hold::Check]);
    f.events.push(Event {
        workflow: WorkflowId::parse(FLOW).unwrap(),
        agent: AgentId::parse(AGENT).unwrap(),
        seq: NonZeroU32::new(1).unwrap(),
        day: 104,
        at: 0,
        what: cleared(Cleared::Check {
            run: run(1),
            answer: CheckAnswer::TryAgain,
        }),
    });
    assert_eq!(
        holds(&f),
        vec![Hold::Check],
        "a cleared check of another run ends nothing"
    );
    f.events[0].what = cleared(Cleared::Check {
        run: run(2),
        answer: CheckAnswer::WentThrough,
    });
    assert_eq!(holds(&f), Vec::<Hold>::new());
    let mut g = calendar(vec![]);
    g.runs = vec![
        past(2, 103, Outcome::Completed, 0),
        past(1, 100, Outcome::EffectUnknown, 1),
    ];
    assert_eq!(holds(&g), Vec::<Hold>::new(), "only the newest run counts");
}

#[test]
fn setup_stands_while_a_parameter_of_the_category_has_no_slot() {
    let mut f = facts(Category::UtilityBillPayment, "web", vec![]);
    f.slots = names(&["start", "instrument"]);
    assert_eq!(holds(&f), vec![Hold::Setup]);
    f.slots = names(&["start", "instrument", "payee"]);
    assert_eq!(holds(&f), Vec::<Hold>::new());
    let mut s = facts(Category::SpreadsheetFill, "file", vec![]);
    s.slots = names(&["workbook", "sheet", "range"]);
    assert_eq!(holds(&s), vec![Hold::Setup]);
    s.slots = names(&["workbook", "sheet", "range", "source"]);
    assert_eq!(holds(&s), Vec::<Hold>::new());
    let mut e = facts(Category::Email, "graph_mail", vec![]);
    assert_eq!(holds(&e), vec![Hold::Setup]);
    e.slots = names(&["reply_senders"]);
    assert_eq!(holds(&e), Vec::<Hold>::new());
    let mut o = facts(Category::ShopOrder, "web", vec![]);
    o.slots = names(&["start", "seller"]);
    assert_eq!(holds(&o), vec![Hold::Setup]);
    o.slots = names(&["start", "seller", "instrument"]);
    assert_eq!(holds(&o), Vec::<Hold>::new());
    assert_eq!(
        holds(&calendar(vec![])),
        Vec::<Hold>::new(),
        "calendar_cleanup has none"
    );
}

#[test]
fn holds_come_in_the_order_of_i5_4() {
    let mut f = facts(
        Category::UtilityBillPayment,
        "none",
        vec![
            (100, set(HoldSetCause::Failures)),
            (101, set(HoldSetCause::Person)),
        ],
    );
    f.runs = vec![past(1, 100, Outcome::EffectUnknown, 1)];
    assert_eq!(
        holds(&f),
        vec![
            Hold::NotYet,
            Hold::Off,
            Hold::Failures,
            Hold::Check,
            Hold::Setup
        ]
    );
    assert_eq!(
        Hold::ALL.iter().map(|h| h.name()).collect::<Vec<_>>(),
        ["not_yet", "off", "failures", "check", "setup"]
    );
}

#[test]
fn slots_are_last_plus_k_periods() {
    let w = workflow(WorkflowTrigger::Schedule, 7, 100);
    assert_eq!(current_slot(&w, 99), None);
    assert_eq!(current_slot(&w, 100), None, "last itself is no slot");
    assert_eq!(current_slot(&w, 106), None);
    assert_eq!(next_slot(&w, 106), Some(107));
    assert_eq!(current_slot(&w, 107), Some(107));
    assert_eq!(next_slot(&w, 107), Some(114));
    assert_eq!(current_slot(&w, 113), Some(107));
    assert_eq!(next_slot(&w, 113), Some(114));
    assert_eq!(current_slot(&w, 114), Some(114));
    assert_eq!(current_slot(&w, 170), Some(170));
    assert_eq!(current_slot(&w, 176), Some(170));
    assert_eq!(next_slot(&w, 99), Some(107));
    let one = workflow(WorkflowTrigger::Schedule, 1, 100);
    assert_eq!(current_slot(&one, 101), Some(101));
    assert_eq!(next_slot(&one, 101), Some(102));
}

#[test]
fn a_workflow_with_no_schedule_has_no_slot() {
    for w in [
        workflow(WorkflowTrigger::Manual, 7, 100),
        workflow(WorkflowTrigger::AppContext, 7, 100),
        workflow(WorkflowTrigger::Schedule, 0, 100),
    ] {
        assert_eq!(current_slot(&w, 120), None);
        assert_eq!(next_slot(&w, 120), None);
    }
    let mut f = calendar(vec![]);
    f.workflow = workflow(WorkflowTrigger::Manual, 7, 100);
    assert!(!slot_open(&f, 107));
    assert_eq!(slot_due(&f, &Limits::PRODUCT), None);
}

#[test]
fn a_served_run_on_or_after_the_slot_less_half_a_period_closes_it() {
    let half = 107 - 7 / 2;
    for (day, cause, open) in [
        (half, RunEnd::Completed, false),
        (half, RunEnd::NothingToDo, false),
        (half - 1, RunEnd::Completed, true),
        (half - 1, RunEnd::NothingToDo, true),
        (107, RunEnd::Failed, true),
        (107, RunEnd::Cancelled, true),
        (107, RunEnd::Declined, true),
        (108, RunEnd::Completed, false),
    ] {
        let f = calendar(vec![(day, started(1, None)), (day, ended(1, cause))]);
        assert_eq!(
            slot_open(&f, 107),
            open,
            "a run on {day} that ended {cause}"
        );
    }
    let unended = calendar(vec![(107, started(1, Some(107)))]);
    assert!(
        slot_open(&unended, 107),
        "a run that has not ended closes nothing"
    );
}

#[test]
fn the_day_of_a_run_is_the_day_of_its_run_started_event() {
    let f = calendar(vec![
        (103, started(1, None)),
        (105, ended(1, RunEnd::Completed)),
    ]);
    assert!(
        slot_open(&f, 107),
        "started on 103, before 104, though it ended on 105"
    );
}

#[test]
fn a_slot_skipped_event_closes_the_slot_it_names() {
    let f = calendar(vec![(107, What::SlotSkipped { slot: Some(107) })]);
    assert!(!slot_open(&f, 107));
    let g = calendar(vec![(107, What::SlotSkipped { slot: Some(100) })]);
    assert!(slot_open(&g, 107));
    let h = calendar(vec![(107, What::SlotSkipped { slot: None })]);
    assert!(slot_open(&h, 107));
}

#[test]
fn a_slot_is_due_while_open_at_most_catch_up_days_old_and_named_by_no_start() {
    let limits = Limits::PRODUCT;
    let mut f = calendar(vec![]);
    f.today = 107;
    assert_eq!(slot_due(&f, &limits), Some(107), "the day the slot opens");
    f.today = 113;
    assert_eq!(slot_due(&f, &limits), Some(107));
    f.workflow = workflow(WorkflowTrigger::Schedule, 30, 100);
    f.today = 130 + 7;
    assert_eq!(slot_due(&f, &limits), Some(130), "catch_up_days old");
    f.today = 130 + 8;
    assert_eq!(slot_due(&f, &limits), None, "the day after catch_up_days");
    f.today = 130;
    f.events = calendar(vec![(129, started(1, Some(100)))]).events;
    assert_eq!(
        slot_due(&f, &limits),
        Some(130),
        "a start that names another slot"
    );
    f.events = calendar(vec![
        (130, started(1, Some(130))),
        (130, ended(1, RunEnd::Failed)),
    ])
    .events;
    assert_eq!(
        slot_due(&f, &limits),
        None,
        "a failed run is not started again for its slot"
    );
    let zero = Limits {
        catch_up_days: 0,
        ..Limits::PRODUCT
    };
    f.events = vec![];
    assert_eq!(slot_due(&f, &zero), Some(130));
    f.today = 131;
    assert_eq!(slot_due(&f, &zero), None);
}

#[test]
fn a_finished_run_is_a_run_that_ended_completed() {
    assert!(!finished(&calendar(vec![])));
    for cause in RunEnd::ALL {
        let f = calendar(vec![(100, started(1, None)), (100, ended(1, *cause))]);
        assert_eq!(finished(&f), *cause == RunEnd::Completed, "{cause}");
    }
}

#[test]
fn check_stands_only_for_a_newest_run_that_the_agent_store_has_ended() {
    let mut f = calendar(vec![]);
    f.runs = vec![past(2, 103, Outcome::EffectUnknown, 1)];
    f.store_ended = false;
    assert_eq!(holds(&f), Vec::<Hold>::new());
    f.store_ended = true;
    assert_eq!(holds(&f), vec![Hold::Check]);
}
