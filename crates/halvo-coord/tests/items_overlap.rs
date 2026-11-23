use halvo_coord::items::drive::{answer_setup, Ledger, SetupDone};
use halvo_coord::items::{setup_step, Setup, SetupInput, SetupMove, SetupParam, SetupState};
use halvo_coord::mem::{Config, Ports};
use halvo_coord::ports::{Chosen, Clock, Files, Owner, PortError, Reading, Senders, Shared};
use halvo_coord::types::{
    Agent, AgentId, AgentState, Body, Category, Cause, Doing, Event, Facts, FullWorkflow, ItemId,
    Limits, Name, OwnItem, OwnItemKind, Param, Reply, ReplyRow, RunId, Workflow, WorkflowId,
    WorkflowState, WorkflowTrigger,
};
use std::cell::Cell;

const DAY: u32 = 20_000;
const T0: u64 = 1_700_000_000_000;

fn agent() -> AgentId {
    AgentId::parse(&format!("{:032x}", 0xa1)).unwrap()
}

fn flow() -> WorkflowId {
    WorkflowId::parse(&format!("{:032x}", 0xf1)).unwrap()
}

fn item() -> ItemId {
    ItemId::parse(&format!("{:032x}", 0x51)).unwrap()
}

fn config() -> Config {
    Config::parse(&format!(
        "{{\"v\":1,\"workflow\":\"{}\",\"agent\":\"{}\",\"rev\":1,\"category\":\"email\",\"tier\":\"on_device\",\"reasoning\":\"medium\",\"autonomy\":\"draft_and_confirm\",\"allow\":[],\"caps\":{{\"untrusted\":\"yes\",\"private\":\"yes\",\"external\":\"no\"}},\"limits\":{{\"steps\":20,\"wall_s\":120,\"model_tokens\":0}},\"budget\":null,\"permissions\":[],\"runtime\":\"graph_mail\",\"playbook\":1}}",
        flow(),
        agent()
    ))
    .unwrap()
}

fn workflow() -> FullWorkflow {
    FullWorkflow {
        id: flow(),
        state: WorkflowState::Approved,
        category: Category::Email,
        site: None,
        trigger: WorkflowTrigger::Manual,
        period: None,
        last: DAY,
        agent: Some(agent()),
    }
}

fn facts() -> Facts<Config> {
    Facts {
        agent: agent(),
        workflow: workflow(),
        config: config(),
        slots: vec![],
        runs: vec![],
        events: vec![],
        store_ended: true,
        replies: halvo_coord::types::Replies::NotAsked,
        unknown: Vec::new(),
        refused: None,
        last_cause: None,
        today: DAY,
        doing: Doing::Idle(Some(OwnItem {
            item: item(),
            kind: OwnItemKind::Setup,
        })),
        stand_in: false,
        active_refused: false,
    }
}

fn ports() -> Ports<Config> {
    let p = Ports::new(T0, DAY);
    p.workflows.borrow_mut().push(Workflow::Full(workflow()));
    p.agents.borrow_mut().push(Agent {
        workflow: flow(),
        agent: agent(),
        config: config(),
        state: AgentState::Approved,
        slots: vec![],
        session: true,
        window: false,
        last: None,
        unknown: vec![],
        refused_run: None,
        last_cause: None,
    });
    p
}

fn setup() -> Setup {
    Setup::new(
        item(),
        flow(),
        agent(),
        Category::Email,
        vec![SetupParam {
            param: Param::ReplySenders,
            candidates: vec![],
        }],
    )
    .unwrap()
}

fn typed(text: &str) -> SetupInput<'_> {
    SetupInput::Typed {
        param: Param::ReplySenders,
        text,
    }
}

/// The memory owner, whose `set_state` `active` is refused with `overlap` while `overlap` is set,
/// as when another agent became live between the body's acknowledgement and the state call.
struct Raced<'a> {
    inner: &'a Ports<Config>,
    overlap: Cell<bool>,
}

impl Owner for Raced<'_> {
    type Config = Config;
    fn log(&self, events: &[Event]) -> Result<(), PortError> {
        self.inner.log(events)
    }
    fn history(&self, agent: &AgentId, after: u32) -> Result<(Vec<Event>, u32), PortError> {
        self.inner.history(agent, after)
    }
    fn workflows(&self) -> Result<Vec<Workflow>, PortError> {
        self.inner.workflows()
    }
    fn agents(&self) -> Result<Vec<Agent<Config>>, PortError> {
        self.inner.agents()
    }
    fn send(&self, body: &Body) -> Result<Reply, PortError> {
        if self.overlap.get()
            && matches!(
                body,
                Body::SetState {
                    state: AgentState::Active,
                    ..
                }
            )
        {
            return Err(PortError::Refused(Name::parse("overlap").unwrap()));
        }
        self.inner.send(body)
    }
    fn replies(
        &self,
        workflow: &WorkflowId,
        agent: &AgentId,
        run: &RunId,
    ) -> Result<Vec<ReplyRow>, PortError> {
        self.inner.replies(workflow, agent, run)
    }
}

impl Clock for Raced<'_> {
    fn now(&self) -> Result<u64, PortError> {
        self.inner.now()
    }
    fn day(&self, at: u64) -> Result<u32, PortError> {
        self.inner.day(at)
    }
}

impl Senders for Raced<'_> {
    fn read(&self, text: &str) -> Result<Reading, PortError> {
        self.inner.read(text)
    }
}

impl Files for Raced<'_> {
    fn choose(&self, folder: Option<&str>) -> Result<Chosen, PortError> {
        self.inner.choose(folder)
    }

    fn sheets(&self, workbook: &str) -> Result<Vec<String>, PortError> {
        self.inner.sheets(workbook)
    }
}

#[test]
fn an_active_of_activate_refused_with_overlap_after_the_acknowledged_body_answers_active_overlap() {
    let inner = ports();
    let p = Raced {
        inner: &inner,
        overlap: Cell::new(true),
    };
    let mut s = setup();
    let mut l = Ledger::new(flow(), agent(), 0);
    let done = answer_setup(
        &p,
        &mut l,
        &Limits::PRODUCT,
        &mut s,
        &facts(),
        typed("ann@x.test"),
    );
    assert_eq!(done, Ok(SetupDone::ActiveOverlap));
    assert_eq!(
        s.state(),
        &SetupState::Closed,
        "the item closed at the acknowledgement"
    );
    p.overlap.set(false);
    let mut s = setup();
    let mut l = Ledger::new(flow(), agent(), 0);
    let done = answer_setup(
        &p,
        &mut l,
        &Limits::PRODUCT,
        &mut s,
        &facts(),
        typed("ann@x.test"),
    );
    assert_eq!(done, Ok(SetupDone::Closed { active: true }));
}

#[test]
fn a_refusal_with_overlap_at_the_first_send_leaves_the_body_owed_for_the_caller() {
    let inner = ports();
    inner.refuse(
        halvo_coord::mem::Method::Send,
        PortError::Refused(Name::parse("overlap").unwrap()),
    );
    let mut s = setup();
    let mut l = Ledger::new(flow(), agent(), 0);
    let done = answer_setup(
        &inner,
        &mut l,
        &Limits::PRODUCT,
        &mut s,
        &facts(),
        typed("a@x.test"),
    );
    assert_eq!(done, Ok(SetupDone::Overlap));
    assert!(matches!(s.state(), SetupState::Owed(_)), "{:?}", s.state());
    assert_eq!(s.step(), 0);
}

#[test]
fn the_move_overlap_keeps_the_body_asks_again_and_again_owes_the_kept_body() {
    let s = setup();
    let owed = setup_step(&s, SetupMove::Input(typed("ann@x.test"))).unwrap();
    let SetupState::Owed(body) = owed.state().clone() else {
        panic!("owed")
    };
    let asked = setup_step(&owed, SetupMove::Overlap).unwrap();
    assert_eq!(asked.state(), &SetupState::Open);
    assert_eq!(asked.kept(), Some(&body));
    assert!(asked.not_taken());
    assert_eq!(asked.step(), 1);
    let again = setup_step(&asked, SetupMove::Again).unwrap();
    assert_eq!(again.state(), &SetupState::Owed(body.clone()));
    assert_eq!(
        setup_step(&s, SetupMove::Again),
        Err(Cause::State),
        "no kept body"
    );
    assert_eq!(setup_step(&s, SetupMove::Overlap), Err(Cause::State));
    let none = setup_step(&asked, SetupMove::Input(SetupInput::None)).unwrap();
    assert_eq!(none.state(), &SetupState::Open, "none sends nothing");
    assert_eq!(none.kept(), Some(&body));
    let new = setup_step(&asked, SetupMove::Input(typed("carol@x.test"))).unwrap();
    assert!(matches!(new.state(), SetupState::Owed(b) if b != &body));
    assert_eq!(new.kept(), None, "a new answer ends the kept body");
    let other = setup_step(&again, SetupMove::NotTaken).unwrap();
    assert_eq!(other.kept(), None, "a refusal by another name ends it");
    assert_eq!(other.shared(), None::<&Shared>);
}
