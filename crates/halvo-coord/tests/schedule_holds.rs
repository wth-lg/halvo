use halvo_coord::mem::Config;
use halvo_coord::schedule::{current_slot, finished, holds, next_slot, slot_due, slot_open, Hold};
use halvo_coord::types::{
    AgentId, Category, CheckAnswer, Class, Cleared, Doing, EffectId, Event, Facts, FullWorkflow,
    HoldSetCause, ItemId, Limits, Name, Outcome, PastRun, Raised, Replies, ReplyRow, ReplyState,
    RunEnd, RunId, Trigger, Unknown, What, WorkflowId, WorkflowState, WorkflowTrigger,
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
        replies: halvo_coord::types::Replies::NotAsked,
        unknown: Vec::new(),
        refused: None,
        last_cause: None,
        today: 107,
        doing: Doing::Idle(None),
        stand_in: false,
        active_refused: false,
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
    What::RunEnded {
        run: run(n),
        cause,
        defers: None,
    }
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
    assert_eq!(
        holds(&g),
        vec![Hold::Off],
        "turned by ruling RD-67 1.2: the derived off is the one test that reads the state"
    );
}

fn listed_paused(whats: Vec<(u32, What)>) -> Facts<Config> {
    let mut f = calendar(whats);
    f.workflow.state = WorkflowState::Paused;
    f
}

fn start_answer(f: &Facts<Config>) -> halvo_coord::schedule::StartStep {
    let around = halvo_coord::schedule::Around {
        listed: true,
        live: 0,
        model_held: false,
        queued: 0,
        window: false,
        from_queue: false,
        owed: false,
    };
    halvo_coord::schedule::start_step(f, Trigger::Person, &Limits::PRODUCT, &around)
}

#[test]
fn an_agent_listed_paused_with_no_hold_in_its_events_holds_the_derived_off() {
    let f = listed_paused(vec![]);
    assert_eq!(holds(&f), vec![Hold::Off]);
    assert!(halvo_coord::schedule::kept_off(&f));
    for state in [WorkflowState::Approved, WorkflowState::Active] {
        let mut g = f.clone();
        g.workflow.state = state;
        assert_eq!(holds(&g), Vec::<Hold>::new(), "{state:?}");
    }
}

#[test]
fn the_derived_off_shows_the_line_off_kept_takes_switch_on_and_refuses_a_start_with_hold() {
    let f = listed_paused(vec![]);
    let line = halvo_coord::roster::status(&f, &Limits::PRODUCT, None);
    assert_eq!(line, halvo_coord::types::StatusLine::Kept);
    assert_eq!(line.key(), "line.off.kept");
    assert_eq!(line.status(), halvo_coord::types::Status::Off);
    assert!(halvo_coord::words::text(&line.key(), &[]).is_ok());
    assert_eq!(
        halvo_coord::roster::controls(&f, None, &halvo_coord::types::Limits::PRODUCT),
        vec![
            halvo_coord::types::Control::SwitchOn,
            halvo_coord::types::Control::Delete
        ]
    );
    let step = start_answer(&f);
    assert_eq!(
        step.verdict,
        halvo_coord::schedule::Verdict::Refused(halvo_coord::types::Cause::Hold)
    );
    assert_eq!(step.clear, None, "a start ends no derived off");
}

#[test]
fn a_switch_off_by_the_events_keeps_the_line_off_person_beside_a_paused_list() {
    let f = listed_paused(vec![(100, set(HoldSetCause::Person))]);
    assert_eq!(holds(&f), vec![Hold::Off]);
    assert!(!halvo_coord::schedule::kept_off(&f));
    assert_eq!(
        halvo_coord::roster::status(&f, &Limits::PRODUCT, None),
        halvo_coord::types::StatusLine::Off
    );
}

#[test]
fn the_derived_off_does_not_stand_beside_a_hold_failures_by_the_events() {
    let f = listed_paused(vec![(100, set(HoldSetCause::Failures))]);
    assert_eq!(holds(&f), vec![Hold::Failures]);
    let step = start_answer(&f);
    assert!(step.clear.is_some(), "a start by the person ends failures");
}

#[test]
fn a_hold_failures_whose_rows_have_left_stands_as_the_derived_off_and_no_start_ends_it() {
    let before = listed_paused(vec![(10, set(HoldSetCause::Failures))]);
    assert_eq!(holds(&before), vec![Hold::Failures]);
    let after = listed_paused(vec![]);
    assert_eq!(
        holds(&after),
        vec![Hold::Off],
        "the rows of day 10 have left"
    );
    let step = start_answer(&after);
    assert_eq!(
        step.verdict,
        halvo_coord::schedule::Verdict::Refused(halvo_coord::types::Cause::Hold)
    );
    assert_eq!(step.clear, None);
}

#[test]
fn the_stand_in_of_a_dismissed_workflow_gives_no_hold() {
    let mut f = listed_paused(vec![]);
    f.stand_in = true;
    assert_eq!(holds(&f), Vec::<Hold>::new());
    assert!(!halvo_coord::schedule::kept_off(&f));
}

#[test]
fn the_derived_off_comes_after_not_yet_in_the_line() {
    let mut f = facts(Category::RideOrder, "none", vec![]);
    f.workflow.state = WorkflowState::Paused;
    assert_eq!(holds(&f), vec![Hold::NotYet, Hold::Off]);
    assert_eq!(
        halvo_coord::roster::status(&f, &Limits::PRODUCT, None),
        halvo_coord::types::StatusLine::NotYet
    );
}

fn past(n: u32, day: u32, outcome: Outcome, unknown: u32) -> PastRun {
    PastRun {
        run: run(n),
        day,
        outcome,
        unknown,
        notes: halvo_coord::types::Notes::default(),
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
        [
            "not_yet",
            "off",
            "sign_in_refused",
            "site_declined",
            "failures",
            "check",
            "amounts",
            "setup"
        ]
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

fn effect_of(n: u32) -> Unknown {
    Unknown {
        run: run(n),
        effect: EffectId::parse(&format!("{:032x}", 0xe000 + n)).unwrap(),
        day: 103,
        class: Class::Send,
        kept: vec![],
    }
}

#[test]
fn check_stands_for_a_record_of_0_whose_run_the_roster_key_names() {
    let mut f = calendar(vec![]);
    f.runs = vec![
        past(2, 103, Outcome::Failed, 0),
        past(1, 100, Outcome::Completed, 0),
    ];
    assert_eq!(holds(&f), Vec::<Hold>::new());
    f.unknown = vec![effect_of(2)];
    assert_eq!(holds(&f), vec![Hold::Check], "the key names the newest run");
    f.store_ended = false;
    assert_eq!(
        holds(&f),
        Vec::<Hold>::new(),
        "only once the store has ended the newest run"
    );
    f.store_ended = true;
    f.events.push(Event {
        workflow: WorkflowId::parse(FLOW).unwrap(),
        agent: AgentId::parse(AGENT).unwrap(),
        seq: NonZeroU32::new(1).unwrap(),
        day: 104,
        at: 0,
        what: cleared(Cleared::Check {
            run: run(2),
            answer: CheckAnswer::TryAgain,
        }),
    });
    assert_eq!(holds(&f), Vec::<Hold>::new(), "the answer that names it");
}

#[test]
fn an_entry_of_an_older_run_alone_gives_no_check() {
    let mut f = calendar(vec![]);
    f.runs = vec![
        past(2, 103, Outcome::Completed, 0),
        past(1, 100, Outcome::Failed, 0),
    ];
    f.unknown = vec![effect_of(1)];
    assert_eq!(holds(&f), Vec::<Hold>::new());
    f.unknown.push(effect_of(2));
    assert_eq!(holds(&f), vec![Hold::Check]);
}

#[test]
fn take_unknown_keeps_the_entries_of_the_newest_run_alone() {
    let mut f = calendar(vec![]);
    f.runs = vec![
        past(2, 103, Outcome::Failed, 0),
        past(1, 100, Outcome::Failed, 0),
    ];
    f.take_unknown(&[effect_of(1), effect_of(2), effect_of(3)]);
    assert_eq!(f.unknown, vec![effect_of(2)]);
    f.take_unknown(&[]);
    assert_eq!(
        f.unknown,
        Vec::<Unknown>::new(),
        "in place of the ones held"
    );
    f.runs.clear();
    f.take_unknown(&[effect_of(2)]);
    assert_eq!(f.unknown, Vec::<Unknown>::new(), "no run, no entry");
}

#[test]
fn check_stands_for_a_reply_row_that_is_not_known_and_after_a_raised_card_until_the_answer() {
    let raised = |seq: u32, what: What| Event {
        workflow: WorkflowId::parse(FLOW).unwrap(),
        agent: AgentId::parse(AGENT).unwrap(),
        seq: NonZeroU32::new(seq).unwrap(),
        day: 103,
        at: 0,
        what,
    };
    let mut f = facts(Category::Email, "graph_mail", vec![]);
    f.slots = names(&["reply_senders"]);
    f.runs = vec![past(2, 103, Outcome::Failed, 0)];
    f.events = vec![raised(
        1,
        What::RunStarted {
            run: run(2),
            trigger: Trigger::Person,
            slot: None,
        },
    )];
    f.replies = Replies::Rows(vec![ReplyRow::Read(ReplyState::Committed)]);
    assert_eq!(holds(&f), Vec::<Hold>::new());
    f.replies = Replies::Rows(vec![
        ReplyRow::Read(ReplyState::Committed),
        ReplyRow::Read(ReplyState::Unknown),
    ]);
    assert_eq!(holds(&f), vec![Hold::Check]);
    f.replies = Replies::Rows(vec![ReplyRow::Unreadable]);
    assert_eq!(holds(&f), vec![Hold::Check]);
    f.replies = Replies::NoAnswer;
    assert_eq!(holds(&f), Vec::<Hold>::new(), "nothing known holds nothing");
    f.events.push(raised(
        2,
        What::ItemRaised {
            item: ItemId::parse(&format!("{:032x}", 0x77)).unwrap(),
            raised: Raised::Check(Some(run(2))),
        },
    ));
    assert_eq!(holds(&f), Vec::<Hold>::new(), "a run's own check item");
    f.events.push(raised(
        3,
        What::ItemRaised {
            item: ItemId::parse(&format!("{:032x}", 0x78)).unwrap(),
            raised: Raised::Check(None),
        },
    ));
    assert_eq!(
        holds(&f),
        vec![Hold::Check],
        "the raised card's hold stands"
    );
    f.replies = Replies::Rows(vec![ReplyRow::Read(ReplyState::Committed)]);
    assert_eq!(
        holds(&f),
        vec![Hold::Check],
        "a later read that answers too"
    );
    f.replies = Replies::NotAsked;
    assert_eq!(
        holds(&f),
        vec![Hold::Check],
        "and a roster read that did not answer, so no read of the rows"
    );
    f.replies = Replies::NoAnswer;
    f.events.push(raised(
        4,
        cleared(Cleared::Check {
            run: run(2),
            answer: CheckAnswer::TryAgain,
        }),
    ));
    assert_eq!(holds(&f), Vec::<Hold>::new(), "until the answer");
    let started = f.events[0].clone();
    let card = f.events[2].clone();
    f.events = vec![card, started];
    assert_eq!(
        holds(&f),
        Vec::<Hold>::new(),
        "a card raised before the newest run started is another run's"
    );
    f.events.reverse();
    assert_eq!(holds(&f), vec![Hold::Check]);
    f.store_ended = false;
    assert_eq!(holds(&f), Vec::<Hold>::new());
}

#[test]
fn a_reply_row_unknown_that_is_not_the_last_row_holds() {
    let mut f = facts(Category::Email, "graph_mail", vec![]);
    f.slots = names(&["reply_senders"]);
    f.runs = vec![past(2, 103, Outcome::Failed, 0)];
    f.replies = Replies::Rows(vec![
        ReplyRow::Read(ReplyState::Unknown),
        ReplyRow::Read(ReplyState::Committed),
    ]);
    assert_eq!(holds(&f), vec![Hold::Check]);
    f.replies = Replies::Rows(vec![
        ReplyRow::Unreadable,
        ReplyRow::Read(ReplyState::Withheld),
    ]);
    assert_eq!(holds(&f), vec![Hold::Check]);
}
