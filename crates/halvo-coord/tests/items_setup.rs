#[path = "items_support.rs"]
mod support;

use halvo_coord::items::drive::{answer_setup, tick_setup, SetupDone};
use halvo_coord::items::{
    params, raise_step, setup_step, Raise, Setup, SetupInput, SetupMove, SetupParam, SetupState,
    Source,
};
use halvo_coord::mem::{Call, Config, Ports};
use halvo_coord::ports::{Owner, PortError};
use halvo_coord::schedule::{holds, Hold};
use halvo_coord::types::fixtures::{self, State};
use halvo_coord::types::{
    Agent, AgentId, AgentState, Body, CandidateFrom, Category, Cause, Doing, Event, Facts,
    FullWorkflow, HoldSetCause, Limits, Name, Outcome, OwnItem, OwnItemKind, Param, PastRun, Reply,
    RunId, What, Workflow, WorkflowId, WorkflowState, WorkflowTrigger,
};
use std::cell::{Cell, RefCell};
use std::num::NonZeroU32;
use support::{item_id, name};

const LIMITS: Limits = Limits::PRODUCT;

/// An idle agent with no item and no run of the category, whose newest bindings hold the slots
/// `bound`.
fn idle(category: Category, bound: &[Param]) -> Facts<Config> {
    let mut f = fixtures::facts(State::Ten)
        .into_iter()
        .next()
        .expect("an agent");
    f.workflow.category = category;
    f.slots = bound.iter().map(|p| name(p.name())).collect();
    f.doing = Doing::Idle(None);
    f
}

/// Every set of the nine parameters of I3.5, as a list.
fn every_set() -> Vec<Vec<Param>> {
    (0u32..1 << Param::ALL.len())
        .map(|bits| {
            Param::ALL
                .iter()
                .enumerate()
                .filter(|(i, _)| bits & (1 << i) != 0)
                .map(|(_, p)| *p)
                .collect()
        })
        .collect()
}

#[test]
fn the_hold_setup_and_the_item_setup_read_one_table_of_parameters() {
    let sets = every_set();
    assert_eq!(sets.len(), 512);
    for c in Category::ALL {
        for bound in &sets {
            let f = idle(*c, bound);
            let hold = holds(&f).contains(&Hold::Setup);
            let item = raise_step(&f, Source::Setup, &LIMITS) == Raise::Own(OwnItemKind::Setup);
            assert_eq!(hold, item, "{c} with the slots {bound:?}");
            let unbound = params(*c).iter().any(|p| !bound.contains(p));
            assert_eq!(hold, unbound, "{c} with the slots {bound:?}");
        }
    }
}

const AGENT: &str = "000000000000000000000000000000a1";
const FLOW: &str = "000000000000000000000000000000f1";
const NOW: u64 = 1_793_793_600_000;
const TODAY: u32 = 20_760;
const ADDRESS: &str = "https://pay.site1.example:443";

fn config(category: Category, runtime: &str) -> Config {
    Config::parse(&format!(
        "{{\"v\":1,\"workflow\":\"{FLOW}\",\"agent\":\"{AGENT}\",\"rev\":1,\"category\":\"{}\",\"tier\":\"on_device\",\"reasoning\":\"medium\",\"autonomy\":\"draft_and_confirm\",\"allow\":[],\"caps\":{{\"untrusted\":\"yes\",\"private\":\"yes\",\"external\":\"no\"}},\"limits\":{{\"steps\":20,\"wall_s\":120,\"model_tokens\":0}},\"budget\":null,\"permissions\":[],\"runtime\":\"{runtime}\",\"playbook\":1}}",
        category.name()
    ))
    .unwrap()
}

fn flow() -> WorkflowId {
    WorkflowId::parse(FLOW).unwrap()
}

fn agent() -> AgentId {
    AgentId::parse(AGENT).unwrap()
}

/// The facts of the agent of a workflow in `state` of `category`, whose setup item is open and
/// whose newest bindings hold no slot.
fn facts(category: Category, runtime: &str, state: WorkflowState) -> Facts<Config> {
    Facts {
        agent: agent(),
        workflow: FullWorkflow {
            id: flow(),
            state,
            category,
            site: None,
            trigger: WorkflowTrigger::Manual,
            period: None,
            last: TODAY,
            agent: Some(agent()),
        },
        config: config(category, runtime),
        slots: vec![],
        runs: vec![],
        events: vec![],
        store_ended: true,
        today: TODAY,
        doing: Doing::Idle(Some(OwnItem {
            item: item_id(1),
            kind: OwnItemKind::Setup,
        })),
    }
}

/// Memory ports whose owner lists the workflow of `f` and its agent.
fn ports(f: &Facts<Config>) -> Ports<Config> {
    let p = Ports::new(NOW, TODAY);
    p.workflows
        .borrow_mut()
        .push(Workflow::Full(f.workflow.clone()));
    p.agents.borrow_mut().push(Agent {
        workflow: flow(),
        agent: agent(),
        config: f.config.clone(),
        state: AgentState::Approved,
        slots: vec![],
        session: false,
        last: None,
        unknown: vec![],
    });
    p
}

/// The owner, whose acknowledgements of `send` are lost while `lose` is set, and of
/// `set_state` alone while `lose_state` is set: the body reaches the memory owner, which
/// stores it and records the call, and the coordinator gets `no_service`. Every attempt of
/// `send` is kept in `sends`.
struct Lossy<'a> {
    inner: &'a Ports<Config>,
    lose: Cell<bool>,
    lose_state: Cell<bool>,
    sends: RefCell<Vec<Body>>,
}

impl<'a> Lossy<'a> {
    fn new(inner: &'a Ports<Config>) -> Lossy<'a> {
        Lossy {
            inner,
            lose: Cell::new(false),
            lose_state: Cell::new(false),
            sends: RefCell::new(Vec::new()),
        }
    }

    fn bindings(&self) -> Vec<Body> {
        self.sends
            .borrow()
            .iter()
            .filter(|b| matches!(b, Body::SetBindings { .. }))
            .cloned()
            .collect()
    }

    fn states(&self) -> Vec<Body> {
        self.sends
            .borrow()
            .iter()
            .filter(|b| matches!(b, Body::SetState { .. }))
            .cloned()
            .collect()
    }
}

impl Owner for Lossy<'_> {
    type Config = Config;

    fn log(&self, events: &[Event]) -> Result<(), PortError> {
        self.inner.log(events)
    }

    fn history(&self, agent: &AgentId, after: u32) -> Result<Vec<Event>, PortError> {
        self.inner.history(agent, after)
    }

    fn workflows(&self) -> Result<Vec<Workflow>, PortError> {
        self.inner.workflows()
    }

    fn agents(&self) -> Result<Vec<Agent<Config>>, PortError> {
        self.inner.agents()
    }

    fn send(&self, body: &Body) -> Result<Reply, PortError> {
        self.sends.borrow_mut().push(body.clone());
        let reply = self.inner.send(body)?;
        let state = matches!(body, Body::SetState { .. });
        if self.lose.get() || (state && self.lose_state.get()) {
            Err(PortError::NoService)
        } else {
            Ok(reply)
        }
    }
}

fn values(text: &[&str]) -> Vec<String> {
    text.iter().map(|s| (*s).to_owned()).collect()
}

/// The setup item of a `spreadsheet_fill` agent: `workbook` with one candidate from the file
/// dialog, `sheet` with two from the records, `range` with none, so that it is typed, and
/// `source` with one candidate read on a site, whose value reads as an address.
fn spreadsheet() -> Setup {
    let params = vec![
        SetupParam {
            param: Param::Workbook,
            candidates: vec![(values(&["Rent 2026.xlsx"]), CandidateFrom::Files)],
        },
        SetupParam {
            param: Param::Sheet,
            candidates: vec![
                (values(&["March"]), CandidateFrom::Records),
                (values(&["April", " and May"]), CandidateFrom::Records),
            ],
        },
        SetupParam {
            param: Param::Range,
            candidates: vec![],
        },
        SetupParam {
            param: Param::Source,
            candidates: vec![(values(&[ADDRESS]), CandidateFrom::Site)],
        },
    ];
    Setup::new(
        item_id(1),
        flow(),
        agent(),
        Category::SpreadsheetFill,
        params,
    )
    .unwrap()
}

fn slot(name_: &str, value: &[&str]) -> (Name, Vec<String>) {
    (name(name_), values(value))
}

fn slots_of(body: &Body) -> Vec<(Name, Vec<String>)> {
    match body {
        Body::SetBindings { slots, .. } => slots
            .iter()
            .map(|s| (s.name.clone(), s.value.clone()))
            .collect(),
        other => panic!("not set_bindings: {other:?}"),
    }
}

const CHOSEN: [(&str, &[&str]); 4] = [
    ("workbook", &["Rent 2026.xlsx"]),
    ("sheet", &["April", " and May"]),
    ("range", &["A1 to C20"]),
    ("source", &[ADDRESS]),
];

fn chosen() -> Vec<(Name, Vec<String>)> {
    CHOSEN.iter().map(|(n, v)| slot(n, v)).collect()
}

/// The three answers before the last of the spreadsheet's setup.
fn three_answers() -> Vec<SetupInput<'static>> {
    vec![
        SetupInput::Choose {
            param: Param::Workbook,
            n: 1,
        },
        SetupInput::Choose {
            param: Param::Sheet,
            n: 2,
        },
        SetupInput::Typed {
            param: Param::Range,
            text: "A1 to C20",
        },
    ]
}

const LAST: SetupInput<'static> = SetupInput::Choose {
    param: Param::Source,
    n: 1,
};

#[test]
fn refusal_setup_owed() {
    let f = facts(Category::SpreadsheetFill, "file", WorkflowState::Approved);
    let inner = ports(&f);
    let p = Lossy::new(&inner);
    let mut s = spreadsheet();
    for mv in three_answers() {
        assert_eq!(answer_setup(&p, &mut s, &f, mv), Ok(SetupDone::Open));
    }
    assert!(
        p.sends.borrow().is_empty(),
        "no body while a parameter has no answer"
    );
    p.lose.set(true);
    assert_eq!(answer_setup(&p, &mut s, &f, LAST), Ok(SetupDone::Owed));
    assert_eq!(p.bindings().len(), 1, "one set_bindings in the answer move");
    assert_eq!(slots_of(&p.bindings()[0]), chosen());
    assert!(p.states().is_empty(), "no set_state in the answer move");
    assert!(matches!(s.state(), SetupState::Owed(_)));
    assert_eq!(tick_setup(&p, &mut s, &f), Ok(SetupDone::Owed));
    assert_eq!(p.bindings().len(), 2, "sent again at the tick");
    assert_eq!(p.bindings()[1], p.bindings()[0]);
    let sent = p.sends.borrow().len();
    assert_eq!(
        answer_setup(
            &p,
            &mut s,
            &f,
            SetupInput::Choose {
                param: Param::Sheet,
                n: 1
            }
        ),
        Err(Cause::State),
        "a second answer while the body is owed"
    );
    assert_eq!(
        answer_setup(
            &p,
            &mut s,
            &f,
            SetupInput::Typed {
                param: Param::Range,
                text: "B2"
            }
        ),
        Err(Cause::State)
    );
    assert_eq!(
        answer_setup(&p, &mut s, &f, SetupInput::None),
        Err(Cause::State)
    );
    assert_eq!(
        p.sends.borrow().len(),
        sent,
        "a refused answer sends nothing"
    );
    assert!(
        matches!(s.state(), SetupState::Owed(_)),
        "the item stays open"
    );
    p.lose.set(false);
    assert_eq!(
        tick_setup(&p, &mut s, &f),
        Ok(SetupDone::Closed { active: true })
    );
    assert_eq!(s.state(), &SetupState::Closed);
    assert_eq!(p.bindings().len(), 3);
    assert!(p.bindings().iter().all(|b| slots_of(b) == chosen()));
    let stored: Vec<(Name, Vec<String>)> = inner
        .slots(&flow(), &agent())
        .into_iter()
        .map(|s| (s.name, s.value))
        .collect();
    assert_eq!(stored, chosen(), "the owner holds the chosen values");
    let names = inner.agents().unwrap()[0].slots.clone();
    assert_eq!(
        names,
        chosen().into_iter().map(|(n, _)| n).collect::<Vec<_>>()
    );
    let active = Body::SetState {
        workflow: flow(),
        agent: agent(),
        state: AgentState::Active,
    };
    assert_eq!(p.states(), vec![active], "the rule of active ran once");
    assert_eq!(
        *p.sends.borrow().last().unwrap(),
        p.states()[0],
        "after the acknowledgement"
    );
    let sent = p.sends.borrow().len();
    assert_eq!(tick_setup(&p, &mut s, &f), Err(Cause::Stale));
    assert_eq!(answer_setup(&p, &mut s, &f, LAST), Err(Cause::Stale));
    assert_eq!(p.sends.borrow().len(), sent, "a closed item sends nothing");
    let stored: Vec<Body> = inner
        .calls
        .borrow()
        .iter()
        .filter_map(|c| match &c.call {
            Call::Send(b) => Some(b.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        stored.len(),
        4,
        "the owner took three bodies and one set_state"
    );
}

#[test]
fn complete_answers_in_any_order_give_the_one_body_and_the_last_choice_counts() {
    let mut moves = three_answers();
    moves.push(LAST);
    let orders = [[0, 1, 2, 3], [3, 2, 1, 0], [1, 3, 0, 2], [2, 0, 3, 1]];
    for order in orders {
        let mut s = spreadsheet();
        for (k, i) in order.iter().enumerate() {
            assert_eq!(s.state(), &SetupState::Open, "{order:?} before answer {k}");
            s = setup_step(&s, SetupMove::Input(moves[*i])).unwrap();
        }
        let SetupState::Owed(body) = s.state() else {
            panic!("{order:?}: no body");
        };
        assert_eq!(slots_of(&body.body()), chosen(), "{order:?}");
    }
    let mut s = spreadsheet();
    s = setup_step(
        &s,
        SetupMove::Input(SetupInput::Choose {
            param: Param::Sheet,
            n: 1,
        }),
    )
    .unwrap();
    s = setup_step(&s, SetupMove::Input(SetupInput::None)).unwrap();
    for mv in three_answers() {
        s = setup_step(&s, SetupMove::Input(mv)).unwrap();
    }
    s = setup_step(&s, SetupMove::Input(LAST)).unwrap();
    let SetupState::Owed(body) = s.state() else {
        panic!("no body");
    };
    assert_eq!(
        slots_of(&body.body())[1],
        slot("sheet", &["April", " and May"])
    );
    match body.body() {
        Body::SetBindings {
            workflow, agent: a, ..
        } => {
            assert_eq!(workflow, flow());
            assert_eq!(a, agent());
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn none_leaves_the_item_open_and_sends_nothing() {
    let f = facts(Category::SpreadsheetFill, "file", WorkflowState::Approved);
    let inner = ports(&f);
    let p = Lossy::new(&inner);
    let mut s = spreadsheet();
    assert_eq!(
        answer_setup(&p, &mut s, &f, SetupInput::None),
        Ok(SetupDone::Open)
    );
    assert_eq!(s.state(), &SetupState::Open);
    assert!(p.sends.borrow().is_empty());
    assert_eq!(tick_setup(&p, &mut s, &f), Ok(SetupDone::Open));
    assert!(
        p.sends.borrow().is_empty(),
        "a tick sends nothing for an open item"
    );
}

#[test]
fn an_answer_that_does_not_fit_the_item_is_malformed_and_changes_nothing() {
    let s = spreadsheet();
    let long = "x".repeat(1025);
    let bad = [
        SetupInput::Choose {
            param: Param::Payee,
            n: 1,
        },
        SetupInput::Choose {
            param: Param::Sheet,
            n: 0,
        },
        SetupInput::Choose {
            param: Param::Sheet,
            n: 3,
        },
        SetupInput::Choose {
            param: Param::Range,
            n: 1,
        },
        SetupInput::Typed {
            param: Param::Sheet,
            text: "March",
        },
        SetupInput::Typed {
            param: Param::Payee,
            text: "City Water",
        },
        SetupInput::Typed {
            param: Param::Range,
            text: &long,
        },
        SetupInput::Typed {
            param: Param::Range,
            text: "A1\nC20",
        },
        SetupInput::Typed {
            param: Param::Range,
            text: "A1\rC20",
        },
        SetupInput::Typed {
            param: Param::Range,
            text: "",
        },
        SetupInput::Typed {
            param: Param::Range,
            text: " \t ",
        },
    ];
    for mv in bad {
        assert_eq!(
            setup_step(&s, SetupMove::Input(mv)),
            Err(Cause::Malformed),
            "{mv:?}"
        );
    }
    let fits = "y".repeat(1024);
    let s = setup_step(
        &s,
        SetupMove::Input(SetupInput::Typed {
            param: Param::Range,
            text: &fits,
        }),
    )
    .unwrap();
    assert_eq!(s.state(), &SetupState::Open);
    assert_eq!(
        setup_step(&s, SetupMove::Acknowledged),
        Err(Cause::State),
        "no body is owed"
    );
}

#[test]
fn a_setup_asks_parameters_of_its_category_each_once() {
    let p = |param: Param| SetupParam {
        param,
        candidates: vec![],
    };
    let made = |c: Category, ps: Vec<SetupParam>| Setup::new(item_id(1), flow(), agent(), c, ps);
    assert_eq!(
        made(Category::SpreadsheetFill, vec![]).err(),
        Some(Cause::Malformed)
    );
    assert_eq!(
        made(Category::SpreadsheetFill, vec![p(Param::Payee)]).err(),
        Some(Cause::Malformed)
    );
    assert_eq!(
        made(
            Category::SpreadsheetFill,
            vec![p(Param::Sheet), p(Param::Sheet)]
        )
        .err(),
        Some(Cause::Malformed)
    );
    assert_eq!(
        made(Category::CalendarCleanup, vec![p(Param::Start)]).err(),
        Some(Cause::Malformed)
    );
    let one = made(Category::Email, vec![p(Param::ReplySenders)]).unwrap();
    assert_eq!(one.item(), &item_id(1));
    let done = setup_step(
        &one,
        SetupMove::Input(SetupInput::Typed {
            param: Param::ReplySenders,
            text: "Anna Berg",
        }),
    )
    .unwrap();
    let SetupState::Owed(body) = done.state() else {
        panic!("one answer completes a setup of one parameter");
    };
    assert_eq!(
        slots_of(&body.body()),
        vec![slot("reply_senders", &["Anna Berg"])]
    );
}

fn past(n: u32, outcome: Outcome) -> PastRun {
    PastRun {
        run: RunId::parse(&format!("{n:032x}")).unwrap(),
        day: TODAY - 1,
        outcome,
        unknown: 0,
    }
}

/// Answers the spreadsheet's setup to the end with every acknowledgement given, and returns
/// what the last answer did and the set_state bodies that it sent.
fn closed_with(f: &Facts<Config>) -> (SetupDone, Vec<Body>) {
    let inner = ports(f);
    let p = Lossy::new(&inner);
    let mut s = spreadsheet();
    for mv in three_answers() {
        answer_setup(&p, &mut s, f, mv).unwrap();
    }
    let done = answer_setup(&p, &mut s, f, LAST).unwrap();
    (done, p.states())
}

#[test]
fn the_rule_of_active_runs_once_right_after_the_acknowledgement() {
    let active = |done: SetupDone, states: &[Body], want: bool| {
        assert_eq!(done, SetupDone::Closed { active: want });
        assert_eq!(states.len(), usize::from(want), "{states:?}");
    };
    let f = facts(Category::SpreadsheetFill, "file", WorkflowState::Approved);
    let (done, states) = closed_with(&f);
    active(done, &states, true);
    let mut web = facts(Category::SpreadsheetFill, "web", WorkflowState::Approved);
    let (done, states) = closed_with(&web);
    active(done, &states, false);
    web.runs = vec![past(1, Outcome::Completed)];
    let (done, states) = closed_with(&web);
    active(done, &states, true);
    let mut off = facts(Category::SpreadsheetFill, "file", WorkflowState::Approved);
    off.events.push(Event {
        workflow: flow(),
        agent: agent(),
        seq: NonZeroU32::MIN,
        day: TODAY,
        at: 0,
        what: What::HoldSet {
            cause: HoldSetCause::Person,
        },
    });
    let (done, states) = closed_with(&off);
    active(done, &states, false);
    let already = facts(Category::SpreadsheetFill, "file", WorkflowState::Active);
    let (done, states) = closed_with(&already);
    active(done, &states, false);
    let none = facts(Category::SpreadsheetFill, "none", WorkflowState::Approved);
    let (done, states) = closed_with(&none);
    active(done, &states, false);
}

#[test]
fn a_refused_body_is_owed_and_a_lost_acknowledgement_of_active_is_not_sent_again() {
    let f = facts(Category::SpreadsheetFill, "file", WorkflowState::Approved);
    let inner = ports(&f);
    let p = Lossy::new(&inner);
    let mut s = spreadsheet();
    for mv in three_answers() {
        answer_setup(&p, &mut s, &f, mv).unwrap();
    }
    inner.refuse(halvo_coord::mem::Method::Send, PortError::Busy);
    assert_eq!(answer_setup(&p, &mut s, &f, LAST), Ok(SetupDone::Owed));
    assert!(
        inner.slots(&flow(), &agent()).is_empty(),
        "the owner stored nothing"
    );
    inner.clear_refusals();
    p.lose_state.set(true);
    assert_eq!(
        tick_setup(&p, &mut s, &f),
        Ok(SetupDone::Closed { active: true })
    );
    assert_eq!(p.states().len(), 1, "sent once, its acknowledgement lost");
    assert_eq!(tick_setup(&p, &mut s, &f), Err(Cause::Stale));
    assert_eq!(
        p.states().len(),
        1,
        "the tick's own rule sends active again, not the setup"
    );
}

#[test]
fn a_body_refused_with_no_workflow_is_dropped_and_not_sent_again() {
    let f = facts(Category::SpreadsheetFill, "file", WorkflowState::Approved);
    let inner = ports(&f);
    let p = Lossy::new(&inner);
    let mut s = spreadsheet();
    for mv in three_answers() {
        answer_setup(&p, &mut s, &f, mv).unwrap();
    }
    inner.workflows.borrow_mut().clear();
    inner.refuse(
        halvo_coord::mem::Method::Send,
        PortError::Refused(name("no_workflow")),
    );
    assert_eq!(answer_setup(&p, &mut s, &f, LAST), Ok(SetupDone::Gone));
    assert_eq!(s.state(), &SetupState::Gone);
    let sent = p.sends.borrow().len();
    assert_eq!(tick_setup(&p, &mut s, &f), Err(Cause::Stale));
    assert_eq!(answer_setup(&p, &mut s, &f, LAST), Err(Cause::Stale));
    assert_eq!(p.sends.borrow().len(), sent);
    assert!(p.states().is_empty());
}

#[test]
fn a_drive_for_another_agent_is_refused_and_sends_nothing() {
    let mut f = facts(Category::SpreadsheetFill, "file", WorkflowState::Approved);
    let inner = ports(&f);
    let p = Lossy::new(&inner);
    let mut s = spreadsheet();
    f.agent = AgentId::parse("000000000000000000000000000000a2").unwrap();
    assert_eq!(answer_setup(&p, &mut s, &f, LAST), Err(Cause::State));
    assert_eq!(tick_setup(&p, &mut s, &f), Err(Cause::State));
    let mut other_flow = facts(Category::SpreadsheetFill, "file", WorkflowState::Approved);
    other_flow.workflow.id = WorkflowId::parse("000000000000000000000000000000f2").unwrap();
    assert_eq!(
        answer_setup(&p, &mut s, &other_flow, LAST),
        Err(Cause::State),
        "the same agent in another workflow"
    );
    assert_eq!(tick_setup(&p, &mut s, &other_flow), Err(Cause::State));
    assert_eq!(s.state(), &SetupState::Open);
    assert!(p.sends.borrow().is_empty());
}

/// A step of a drawn sequence: an answer of the item, a tick, or a change of whether the owner's
/// acknowledgement of `send` is lost.
#[derive(Clone, Copy, Debug)]
enum Step {
    Answer(SetupInput<'static>),
    Tick,
    Lose(bool),
}

#[test]
fn every_body_of_any_sequence_of_answers_ticks_and_lost_acknowledgements_holds_the_given_values() {
    let alphabet = [
        Step::Answer(SetupInput::Choose {
            param: Param::Workbook,
            n: 1,
        }),
        Step::Answer(SetupInput::Choose {
            param: Param::Sheet,
            n: 2,
        }),
        Step::Answer(SetupInput::Typed {
            param: Param::Range,
            text: ADDRESS,
        }),
        Step::Answer(SetupInput::Choose {
            param: Param::Source,
            n: 1,
        }),
        Step::Answer(SetupInput::None),
        Step::Tick,
        Step::Lose(true),
        Step::Lose(false),
    ];
    let f = facts(Category::SpreadsheetFill, "file", WorkflowState::Approved);
    let length = 6;
    let mut sequences = 0u64;
    let mut bodies = 0u64;
    let mut closed = 0u64;
    let total = (alphabet.len() as u64).pow(length);
    for code in 0..total {
        let inner = ports(&f);
        let p = Lossy::new(&inner);
        let mut s = spreadsheet();
        let mut rest = code;
        for _ in 0..length {
            let step = alphabet[(rest % alphabet.len() as u64) as usize];
            rest /= alphabet.len() as u64;
            let _ = match step {
                Step::Answer(mv) => answer_setup(&p, &mut s, &f, mv).map(|_| ()),
                Step::Tick => tick_setup(&p, &mut s, &f).map(|_| ()),
                Step::Lose(on) => {
                    p.lose.set(on);
                    Ok(())
                }
            };
        }
        sequences += 1;
        for b in p.bindings() {
            bodies += 1;
            for (n, v) in slots_of(&b) {
                let allowed: Vec<Vec<String>> = match n.as_str() {
                    "workbook" => vec![values(&["Rent 2026.xlsx"])],
                    "sheet" => vec![values(&["April", " and May"])],
                    "range" => vec![values(&[ADDRESS])],
                    "source" => vec![values(&[ADDRESS])],
                    other => panic!("sequence {code}: slot {other}"),
                };
                assert!(allowed.contains(&v), "sequence {code}: {n:?} {v:?}");
            }
        }
        if s.state() == &SetupState::Closed {
            closed += 1;
            assert_eq!(p.states().len(), 1, "sequence {code}");
        } else {
            assert!(p.states().is_empty(), "sequence {code}");
        }
    }
    assert_eq!(sequences, 262_144);
    assert!(bodies > 0 && closed > 0, "bodies={bodies} closed={closed}");
}
