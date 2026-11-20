use halvo_coord::mem::{Call, Config, Method, Ports};
use halvo_coord::ports::{Owner, PortError};
use halvo_coord::types::{
    Agent, AgentId, AgentState, Body, Category, Name, Slot, Workflow, WorkflowId, WorkflowState,
    WorkflowTrigger,
};
use halvo_coord::types::{Event, FullWorkflow, HoldSetCause, What};
use std::num::NonZeroU32;

const DAY: u32 = 20_000;
const T0: u64 = 1_700_000_000_000;
const ACCOUNT: &str = "me@home.test";

fn agent(n: usize) -> AgentId {
    AgentId::parse(&format!("{:032x}", 0xa0 + n)).unwrap()
}

fn flow(n: usize) -> WorkflowId {
    WorkflowId::parse(&format!("{:032x}", 0xf0 + n)).unwrap()
}

fn name(text: &str) -> Name {
    Name::parse(text).unwrap()
}

fn refused(text: &str) -> PortError {
    PortError::Refused(name(text))
}

fn config(n: usize) -> Config {
    Config::parse(&format!(
        "{{\"v\":1,\"workflow\":\"{}\",\"agent\":\"{}\",\"rev\":1,\"category\":\"email\",\"tier\":\"on_device\",\"reasoning\":\"medium\",\"autonomy\":\"draft_and_confirm\",\"allow\":[],\"caps\":{{\"untrusted\":\"yes\",\"private\":\"yes\",\"external\":\"no\"}},\"limits\":{{\"steps\":20,\"wall_s\":120,\"model_tokens\":0}},\"budget\":null,\"permissions\":[],\"runtime\":\"graph_mail\",\"playbook\":1}}",
        flow(n),
        agent(n),
    ))
    .unwrap()
}

fn workflow(n: usize, state: WorkflowState) -> FullWorkflow {
    FullWorkflow {
        id: flow(n),
        state,
        category: Category::Email,
        site: None,
        trigger: WorkflowTrigger::Manual,
        period: None,
        last: DAY - 2,
        agent: Some(agent(n)),
    }
}

fn slots(account: &str, senders: &[&str]) -> Vec<Slot> {
    vec![
        Slot {
            name: name("account_address"),
            value: vec![account.to_owned()],
        },
        Slot {
            name: name("reply_senders"),
            value: senders.iter().map(|s| (*s).to_owned()).collect(),
        },
    ]
}

/// Memory ports that hold an email agent `n` for each entry, with its workflow in the state and
/// its bindings with the account and the senders, stored through `set_bindings` before any other
/// agent is live, and with each `hold_set` of its events.
fn ports(agents: &[(WorkflowState, &str, &[&str], &[HoldSetCause])]) -> Ports<Config> {
    let p = Ports::new(T0, DAY);
    for (n, (_, account, senders, _)) in agents.iter().enumerate() {
        p.workflows
            .borrow_mut()
            .push(Workflow::Full(workflow(n, WorkflowState::Paused)));
        p.agents.borrow_mut().push(Agent {
            workflow: flow(n),
            agent: agent(n),
            config: config(n),
            state: AgentState::Paused,
            slots: vec![name("account_address"), name("reply_senders")],
            session: true,
            last: None,
            unknown: vec![],
        });
        p.send(&Body::SetBindings {
            workflow: flow(n),
            agent: agent(n),
            slots: slots(account, senders),
        })
        .unwrap();
    }
    for (n, (state, _, _, held)) in agents.iter().enumerate() {
        set_listed(&p, n, *state);
        let events: Vec<Event> = held
            .iter()
            .enumerate()
            .map(|(k, cause)| Event {
                workflow: flow(n),
                agent: agent(n),
                seq: NonZeroU32::new(k as u32 + 1).unwrap(),
                day: DAY - 1,
                at: 0,
                what: What::HoldSet { cause: *cause },
            })
            .collect();
        if !events.is_empty() {
            p.log(&events).unwrap();
        }
    }
    p.calls.borrow_mut().clear();
    p.tried.borrow_mut().clear();
    p
}

fn set_listed(p: &Ports<Config>, n: usize, state: WorkflowState) {
    for w in p.workflows.borrow_mut().iter_mut() {
        if let Workflow::Full(f) = w {
            if f.id == flow(n) {
                f.state = state;
            }
        }
    }
}

fn listed(p: &Ports<Config>, n: usize) -> WorkflowState {
    p.workflows
        .borrow()
        .iter()
        .find_map(|w| match w {
            Workflow::Full(f) if f.id == flow(n) => Some(f.state),
            _ => None,
        })
        .unwrap()
}

fn set_state(p: &Ports<Config>, n: usize, state: AgentState) -> Result<(), PortError> {
    p.send(&Body::SetState {
        workflow: flow(n),
        agent: agent(n),
        state,
    })
    .map(|_| ())
}

const ACTIVE: WorkflowState = WorkflowState::Active;
const APPROVED: WorkflowState = WorkflowState::Approved;
const PAUSED: WorkflowState = WorkflowState::Paused;

#[test]
fn an_acknowledged_set_state_keeps_its_state_in_the_owners_list() {
    let p = ports(&[(APPROVED, ACCOUNT, &["ann@people.test"], &[])]);
    set_state(&p, 0, AgentState::Active).unwrap();
    assert_eq!(listed(&p, 0), ACTIVE);
    set_state(&p, 0, AgentState::Paused).unwrap();
    assert_eq!(listed(&p, 0), PAUSED);
    set_state(&p, 0, AgentState::Approved).unwrap();
    assert_eq!(listed(&p, 0), APPROVED);
}

#[test]
fn a_refused_set_state_leaves_the_owners_list_as_it_was() {
    let p = ports(&[(APPROVED, ACCOUNT, &["ann@people.test"], &[])]);
    p.refuse(Method::Send, refused("busy"));
    assert_eq!(set_state(&p, 0, AgentState::Active), Err(refused("busy")));
    assert_eq!(listed(&p, 0), APPROVED);
}

#[test]
fn set_state_active_and_approved_are_refused_with_overlap_beside_a_live_agent_and_paused_never() {
    let p = ports(&[
        (
            PAUSED,
            ACCOUNT,
            &["Ann@People.test", "bob@people.test"],
            &[],
        ),
        (ACTIVE, ACCOUNT, &["ann@people.test"], &[]),
    ]);
    for state in [AgentState::Active, AgentState::Approved] {
        assert_eq!(
            set_state(&p, 0, state),
            Err(refused("overlap")),
            "{state:?}"
        );
        assert_eq!(listed(&p, 0), PAUSED, "{state:?}: no state changes");
    }
    assert_eq!(set_state(&p, 0, AgentState::Paused), Ok(()));
    set_listed(&p, 1, PAUSED);
    assert_eq!(
        set_state(&p, 0, AgentState::Active),
        Ok(()),
        "a paused agent is not live"
    );
}

#[test]
fn set_state_is_not_refused_for_another_account_or_no_shared_sender() {
    let p = ports(&[
        (PAUSED, ACCOUNT, &["ann@people.test"], &[]),
        (ACTIVE, "other@home.test", &["ann@people.test"], &[]),
        (ACTIVE, ACCOUNT, &["carol@people.test"], &[]),
    ]);
    assert_eq!(set_state(&p, 0, AgentState::Active), Ok(()));
}

#[test]
fn set_bindings_that_would_share_a_sender_is_refused_with_overlap_and_stores_nothing() {
    let p = ports(&[
        (APPROVED, ACCOUNT, &["carol@people.test"], &[]),
        (ACTIVE, ACCOUNT, &["ann@people.test"], &[]),
    ]);
    let before = p.slots(&flow(0), &agent(0));
    let body = Body::SetBindings {
        workflow: flow(0),
        agent: agent(0),
        slots: slots(ACCOUNT, &["ANN@people.test"]),
    };
    assert_eq!(p.send(&body), Err(refused("overlap")));
    assert_eq!(p.slots(&flow(0), &agent(0)), before);
    assert!(p.calls.borrow().is_empty(), "a refusal records no call");
}

#[test]
fn the_read_names_each_other_live_agent_with_the_shared_senders_in_the_asking_spelling() {
    let p = ports(&[
        (
            APPROVED,
            ACCOUNT,
            &["Ann@People.test", "bob@people.test"],
            &[],
        ),
        (
            ACTIVE,
            ACCOUNT,
            &["ann@people.test", "dan@people.test"],
            &[],
        ),
        (ACTIVE, ACCOUNT, &["carol@people.test"], &[]),
        (ACTIVE, "other@home.test", &["bob@people.test"], &[]),
        (PAUSED, ACCOUNT, &["bob@people.test"], &[]),
        (
            APPROVED,
            ACCOUNT,
            &["BOB@people.test", "ann@people.test"],
            &[],
        ),
    ]);
    let read = p.overlaps(&flow(0), &agent(0), None).unwrap();
    let named: Vec<(AgentId, Vec<String>)> = read
        .entries
        .iter()
        .map(|e| (e.agent.clone(), e.senders.clone()))
        .collect();
    assert_eq!(
        named,
        vec![
            (agent(1), vec!["Ann@People.test".to_owned()]),
            (
                agent(5),
                vec!["Ann@People.test".to_owned(), "bob@people.test".to_owned()]
            ),
        ]
    );
    assert_eq!(read.entries[0].workflow, flow(1));
    assert!(!read.more);
    assert_eq!(p.overlap_reads.borrow().clone(), vec![(agent(0), None)]);
}

#[test]
fn the_read_takes_the_bodys_slots_merged_by_name_and_stores_nothing() {
    let p = ports(&[
        (APPROVED, ACCOUNT, &["ann@people.test"], &[]),
        (ACTIVE, ACCOUNT, &["ann@people.test"], &[]),
    ]);
    let fewer = vec![Slot {
        name: name("reply_senders"),
        value: vec!["carol@people.test".to_owned()],
    }];
    let read = p.overlaps(&flow(0), &agent(0), Some(&fewer)).unwrap();
    assert!(read.entries.is_empty(), "{read:?}");
    assert_eq!(
        p.overlaps(&flow(0), &agent(0), None).unwrap().entries.len(),
        1,
        "the stored slots stand"
    );
    assert!(p
        .calls
        .borrow()
        .iter()
        .all(|t| !matches!(t.call, Call::Send(_))));
}

#[test]
fn the_read_names_sixteen_and_says_more() {
    let mut agents: Vec<(WorkflowState, &str, &[&str], &[HoldSetCause])> =
        vec![(APPROVED, ACCOUNT, &["ann@people.test"], &[])];
    for _ in 0..17 {
        agents.push((ACTIVE, ACCOUNT, &["ann@people.test"], &[]));
    }
    let p = ports(&agents);
    let read = p.overlaps(&flow(0), &agent(0), None).unwrap();
    assert_eq!(read.entries.len(), 16);
    assert!(read.more);
    let sixteen: Vec<AgentId> = (1..=16).map(agent).collect();
    let named: Vec<AgentId> = read.entries.iter().map(|e| e.agent.clone()).collect();
    assert_eq!(named, sixteen, "the first sixteen by workflow id");
}

#[test]
fn the_read_refuses_as_the_owner_does() {
    let p = ports(&[(APPROVED, ACCOUNT, &["ann@people.test"], &[])]);
    assert_eq!(
        p.overlaps(&flow(9), &agent(0), None),
        Err(refused("no_workflow"))
    );
    assert_eq!(
        p.overlaps(&flow(0), &agent(9), None),
        Err(refused("binding"))
    );
    p.refuse(Method::Overlaps, PortError::Busy);
    assert_eq!(p.overlaps(&flow(0), &agent(0), None), Err(PortError::Busy));
    assert_eq!(p.overlap_reads.borrow().len(), 3);
}

#[test]
fn an_owner_that_does_not_read_overlaps_answers_no_service() {
    struct Plain<'a>(&'a Ports<Config>);
    impl Owner for Plain<'_> {
        type Config = Config;
        fn log(&self, events: &[Event]) -> Result<(), PortError> {
            self.0.log(events)
        }
        fn history(&self, agent: &AgentId, after: u32) -> Result<(Vec<Event>, u32), PortError> {
            self.0.history(agent, after)
        }
        fn workflows(&self) -> Result<Vec<Workflow>, PortError> {
            self.0.workflows()
        }
        fn agents(&self) -> Result<Vec<Agent<Config>>, PortError> {
            self.0.agents()
        }
        fn send(&self, body: &Body) -> Result<halvo_coord::types::Reply, PortError> {
            self.0.send(body)
        }
        fn replies(
            &self,
            workflow: &WorkflowId,
            agent: &AgentId,
            run: &halvo_coord::types::RunId,
        ) -> Result<Vec<halvo_coord::types::ReplyRow>, PortError> {
            self.0.replies(workflow, agent, run)
        }
    }
    let p = ports(&[(APPROVED, ACCOUNT, &["ann@people.test"], &[])]);
    assert_eq!(
        Plain(&p).overlaps(&flow(0), &agent(0), None),
        Err(PortError::NoService)
    );
}

fn life(p: &Ports<Config>) -> halvo_coord::schedule::Board<Config> {
    halvo_coord::run::recover(p, &halvo_coord::types::Limits::PRODUCT)
        .unwrap()
        .board
}

#[test]
fn a_new_life_gives_the_stand_in_of_a_dismissed_workflow_no_hold() {
    let p = ports(&[(PAUSED, ACCOUNT, &["ann@people.test"], &[])]);
    p.workflows.borrow_mut()[0] = Workflow::Dismissed(halvo_coord::types::DismissedWorkflow {
        id: flow(0),
        category: Category::Email,
        site: None,
    });
    let board = life(&p);
    let facts = &board.members[0].facts;
    assert!(facts.stand_in);
    assert_eq!(facts.workflow.state, PAUSED);
    assert_eq!(
        halvo_coord::schedule::holds(facts),
        Vec::<halvo_coord::schedule::Hold>::new()
    );
    let listed_paused = ports(&[(PAUSED, ACCOUNT, &["ann@people.test"], &[])]);
    let board = life(&listed_paused);
    assert!(!board.members[0].facts.stand_in);
    assert_eq!(
        halvo_coord::schedule::holds(&board.members[0].facts),
        vec![halvo_coord::schedule::Hold::Off]
    );
}
