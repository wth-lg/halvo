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

const L: halvo_coord::types::Limits = halvo_coord::types::Limits::PRODUCT;

fn go(
    p: &Ports<Config>,
    board: &mut halvo_coord::schedule::Board<Config>,
    mv: halvo_coord::schedule::Move,
) -> halvo_coord::schedule::Report {
    halvo_coord::schedule::drive(p, board, &L, mv)
}

fn switch_on(n: usize) -> halvo_coord::schedule::Move {
    halvo_coord::schedule::Move::SwitchOn { agent: agent(n) }
}

fn start(n: usize) -> halvo_coord::schedule::Move {
    halvo_coord::schedule::Move::Start { agent: agent(n) }
}

/// The calls that wrote or asked the owner, as tried, in order: `set_state <state>` and `log`
/// with its events' names, each with `refused` where the owner refused it.
fn writes(p: &Ports<Config>) -> Vec<String> {
    p.tried
        .borrow()
        .iter()
        .filter_map(|t| {
            let text = match &t.call {
                Call::Send(Body::SetState {
                    agent: a, state, ..
                }) => {
                    format!("set_state {} {}", &a.as_str()[30..], state.name())
                }
                Call::Send(Body::StartRun { .. }) => "start_run".to_owned(),
                Call::Log(events) => format!(
                    "log {}",
                    events
                        .iter()
                        .map(halvo_coord::items::drive::event_name)
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                _ => return None,
            };
            Some(match &t.refused {
                Some(_) => format!("{text} refused"),
                None => text,
            })
        })
        .collect()
}

fn stored(p: &Ports<Config>, n: usize) -> Vec<&'static str> {
    p.events
        .borrow()
        .iter()
        .filter(|e| e.agent == agent(n))
        .map(halvo_coord::items::drive::event_name)
        .collect()
}

fn holds_of(
    board: &halvo_coord::schedule::Board<Config>,
    n: usize,
) -> Vec<halvo_coord::schedule::Hold> {
    halvo_coord::schedule::holds(&board.members[n].facts)
}

const OFF: halvo_coord::schedule::Hold = halvo_coord::schedule::Hold::Off;
const FAILURES: halvo_coord::schedule::Hold = halvo_coord::schedule::Hold::Failures;

fn fresh(p: &Ports<Config>) {
    p.tried.borrow_mut().clear();
    p.calls.borrow_mut().clear();
}

#[test]
fn b_a_switch_on_sends_the_state_first_and_stores_hold_cleared_only_after_its_acknowledgement() {
    let p = ports(&[(
        PAUSED,
        ACCOUNT,
        &["ann@people.test"],
        &[HoldSetCause::Person],
    )]);
    let mut board = life(&p);
    fresh(&p);
    assert_eq!(
        go(&p, &mut board, switch_on(0)).answer,
        Ok(halvo_coord::schedule::Answer::Ok)
    );
    assert_eq!(writes(&p), ["set_state a0 active", "log hold_cleared"]);
    assert_eq!(
        board.members[0].facts.workflow.state, ACTIVE,
        "the acknowledged state"
    );
    assert_eq!(holds_of(&board, 0), vec![]);
}

#[test]
fn b_a_switch_on_whose_state_gets_no_acknowledgement_clears_nothing_stores_nothing_and_answers_the_cause(
) {
    use halvo_coord::types::Cause;
    for (error, cause) in [
        (PortError::Busy, Cause::Busy),
        (PortError::NoService, Cause::NoService),
        (refused("store_unavailable"), Cause::NoService),
        (refused("overlap"), Cause::NoService),
    ] {
        let p = ports(&[(
            PAUSED,
            ACCOUNT,
            &["ann@people.test"],
            &[HoldSetCause::Person, HoldSetCause::Failures],
        )]);
        let mut board = life(&p);
        let before = stored(&p, 0);
        fresh(&p);
        p.refuse(Method::Send, error.clone());
        assert_eq!(
            go(&p, &mut board, switch_on(0)).answer,
            Err(cause),
            "{error:?}"
        );
        assert_eq!(writes(&p), ["set_state a0 active refused"], "{error:?}");
        assert_eq!(stored(&p, 0), before, "{error:?}: nothing stored");
        assert_eq!(holds_of(&board, 0), vec![OFF, FAILURES], "{error:?}");
        assert_eq!(board.members[0].facts.workflow.state, PAUSED, "{error:?}");
    }
}

#[test]
fn b_a_switch_on_refused_with_overlap_by_the_store_beside_a_live_agent_stores_nothing() {
    let p = ports(&[
        (
            PAUSED,
            ACCOUNT,
            &["ann@people.test"],
            &[HoldSetCause::Person],
        ),
        (ACTIVE, ACCOUNT, &["ann@people.test"], &[]),
    ]);
    let mut board = life(&p);
    let before = stored(&p, 0);
    fresh(&p);
    assert_eq!(
        go(&p, &mut board, switch_on(0)).answer,
        Err(halvo_coord::types::Cause::NoService)
    );
    assert_eq!(writes(&p), ["set_state a0 active refused"]);
    assert_eq!(stored(&p, 0), before);
    assert_eq!(holds_of(&board, 0), vec![OFF]);
    assert_eq!(listed(&p, 0), PAUSED);
}

#[test]
fn b_a_start_that_ends_failures_sends_the_state_first_and_on_no_acknowledgement_stores_no_run() {
    let p = ports(&[(
        PAUSED,
        ACCOUNT,
        &["ann@people.test"],
        &[HoldSetCause::Failures],
    )]);
    let mut board = life(&p);
    let before = stored(&p, 0);
    fresh(&p);
    p.refuse(Method::Send, PortError::Busy);
    assert_eq!(
        go(&p, &mut board, start(0)).answer,
        Err(halvo_coord::types::Cause::Busy)
    );
    assert_eq!(writes(&p), ["set_state a0 active refused"]);
    assert_eq!(
        stored(&p, 0),
        before,
        "no hold_cleared, no run_started, no run_ended"
    );
    assert_eq!(holds_of(&board, 0), vec![FAILURES]);
    p.clear_refusals();
    fresh(&p);
    let r = go(&p, &mut board, start(0));
    assert_eq!(r.answer, Ok(halvo_coord::schedule::Answer::Ok), "{r:?}");
    assert_eq!(
        writes(&p),
        [
            "set_state a0 active",
            "log hold_cleared",
            "log run_started",
            "start_run"
        ]
    );
}

#[test]
fn b_under_a_standing_hold_off_a_start_ends_failures_and_sends_no_state() {
    let p = ports(&[(
        PAUSED,
        ACCOUNT,
        &["ann@people.test"],
        &[HoldSetCause::Person, HoldSetCause::Failures],
    )]);
    let mut board = life(&p);
    fresh(&p);
    assert_eq!(
        go(&p, &mut board, start(0)).answer,
        Err(halvo_coord::types::Cause::Hold)
    );
    assert_eq!(writes(&p), ["log hold_cleared"]);
    assert_eq!(holds_of(&board, 0), vec![OFF]);
}

#[test]
fn b_the_derived_off_takes_switch_on_with_its_state_and_no_event() {
    let p = ports(&[(PAUSED, ACCOUNT, &["ann@people.test"], &[])]);
    let mut board = life(&p);
    assert_eq!(holds_of(&board, 0), vec![OFF]);
    assert_eq!(
        go(&p, &mut board, start(0)).answer,
        Err(halvo_coord::types::Cause::Hold),
        "a start is refused with hold"
    );
    fresh(&p);
    assert_eq!(
        go(&p, &mut board, switch_on(0)).answer,
        Ok(halvo_coord::schedule::Answer::Ok)
    );
    assert_eq!(writes(&p), ["set_state a0 active"]);
    assert_eq!(listed(&p, 0), ACTIVE);
    assert_eq!(holds_of(&board, 0), vec![]);
    assert_eq!(stored(&p, 0), Vec::<&str>::new());
}

fn tick() -> halvo_coord::schedule::Move {
    halvo_coord::schedule::Move::Tick
}

fn switch_off(n: usize) -> halvo_coord::schedule::Move {
    halvo_coord::schedule::Move::SwitchOff { agent: agent(n) }
}

/// The `set_state` calls as tried, in order, each with `refused` where the owner refused it.
fn states(p: &Ports<Config>) -> Vec<String> {
    writes(p)
        .into_iter()
        .filter(|w| w.starts_with("set_state"))
        .collect()
}

#[test]
fn d_a_switch_off_whose_paused_is_lost_gets_paused_from_the_next_tick_and_none_once_listed_paused()
{
    let p = ports(&[(ACTIVE, ACCOUNT, &["ann@people.test"], &[])]);
    let mut board = life(&p);
    p.refuse(Method::Send, PortError::Busy);
    assert_eq!(
        go(&p, &mut board, switch_off(0)).answer,
        Ok(halvo_coord::schedule::Answer::Ok)
    );
    assert_eq!(
        listed(&p, 0),
        ACTIVE,
        "the paused of the switch-off is lost"
    );
    p.clear_refusals();
    fresh(&p);
    go(&p, &mut board, tick());
    assert_eq!(states(&p), ["set_state a0 paused"]);
    assert_eq!(listed(&p, 0), PAUSED);
    fresh(&p);
    go(&p, &mut board, tick());
    assert_eq!(
        states(&p),
        Vec::<String>::new(),
        "none once the list shows paused"
    );
}

#[test]
fn d_a_switch_off_whose_paused_is_lost_gets_paused_from_the_first_tick_of_a_new_life() {
    let p = ports(&[(ACTIVE, ACCOUNT, &["ann@people.test"], &[])]);
    let mut board = life(&p);
    p.refuse(Method::Send, PortError::NoService);
    go(&p, &mut board, switch_off(0));
    p.clear_refusals();
    assert_eq!(listed(&p, 0), ACTIVE);
    let mut again = life(&p);
    fresh(&p);
    go(&p, &mut again, tick());
    assert_eq!(states(&p), ["set_state a0 paused"]);
    assert_eq!(listed(&p, 0), PAUSED);
    fresh(&p);
    go(&p, &mut again, tick());
    assert_eq!(states(&p), Vec::<String>::new());
}

#[test]
fn d_a_hold_failures_beside_an_active_list_gets_paused_at_each_tick_until_it_is_acknowledged() {
    let p = ports(&[(
        ACTIVE,
        ACCOUNT,
        &["ann@people.test"],
        &[HoldSetCause::Failures],
    )]);
    let mut board = life(&p);
    p.refuse(Method::Send, PortError::Busy);
    fresh(&p);
    go(&p, &mut board, tick());
    assert_eq!(states(&p), ["set_state a0 paused refused"]);
    fresh(&p);
    go(&p, &mut board, tick());
    assert_eq!(
        states(&p),
        ["set_state a0 paused refused"],
        "again at the next tick"
    );
    p.clear_refusals();
    fresh(&p);
    go(&p, &mut board, tick());
    assert_eq!(states(&p), ["set_state a0 paused"]);
    assert_eq!(
        board.members[0].facts.workflow.state, PAUSED,
        "the acknowledged state"
    );
    fresh(&p);
    go(&p, &mut board, tick());
    assert_eq!(states(&p), Vec::<String>::new());
}

#[test]
fn d_a_switch_on_whose_state_arrived_and_whose_hold_cleared_was_refused_gets_paused_at_the_next_tick(
) {
    let p = ports(&[(
        PAUSED,
        ACCOUNT,
        &["ann@people.test"],
        &[HoldSetCause::Person],
    )]);
    let mut board = life(&p);
    p.refuse(Method::Log, refused("internal"));
    fresh(&p);
    assert_eq!(
        go(&p, &mut board, switch_on(0)).answer,
        Err(halvo_coord::types::Cause::NoService)
    );
    assert_eq!(listed(&p, 0), ACTIVE, "the state arrived");
    assert_eq!(
        holds_of(&board, 0),
        vec![OFF],
        "the hold stands by the events"
    );
    p.clear_refusals();
    fresh(&p);
    go(&p, &mut board, tick());
    assert_eq!(states(&p), ["set_state a0 paused"]);
    assert_eq!(listed(&p, 0), PAUSED);
}

#[test]
fn d_no_paused_goes_for_the_stand_in_of_a_dismissed_workflow_or_a_workflow_off_the_list() {
    let p = ports(&[
        (
            ACTIVE,
            ACCOUNT,
            &["ann@people.test"],
            &[HoldSetCause::Person],
        ),
        (
            ACTIVE,
            "two@home.test",
            &["bob@people.test"],
            &[HoldSetCause::Person],
        ),
    ]);
    let mut board = life(&p);
    p.workflows.borrow_mut()[0] = Workflow::Dismissed(halvo_coord::types::DismissedWorkflow {
        id: flow(0),
        category: Category::Email,
        site: None,
    });
    p.workflows.borrow_mut().remove(1);
    fresh(&p);
    go(&p, &mut board, tick());
    assert_eq!(states(&p), Vec::<String>::new());
}

fn line_of(
    board: &halvo_coord::schedule::Board<Config>,
    n: usize,
) -> halvo_coord::types::StatusLine {
    halvo_coord::roster::status(&board.members[n].facts, &L, None)
}

/// The test of ruling RD-67 4.7, from a store state made by hand: two live agents with a shared
/// sender, the tick's `active` refused once, `paused` follows, the row shows the derived hold's
/// line, no `active` goes out at the ticks that follow, and a Start now stores no failed run.
#[test]
fn c_rd67_4_7_an_active_refused_with_overlap_is_followed_by_paused_and_no_active_again() {
    let p = ports(&[
        (APPROVED, ACCOUNT, &["ann@people.test"], &[]),
        (ACTIVE, ACCOUNT, &["ann@people.test"], &[]),
    ]);
    let mut board = life(&p);
    fresh(&p);
    go(&p, &mut board, tick());
    assert_eq!(
        states(&p),
        ["set_state a0 active refused", "set_state a0 paused"]
    );
    assert_eq!(listed(&p, 0), PAUSED);
    assert_eq!(line_of(&board, 0), halvo_coord::types::StatusLine::Kept);
    for _ in 0..3 {
        fresh(&p);
        go(&p, &mut board, tick());
        assert_eq!(
            states(&p),
            Vec::<String>::new(),
            "no active at the ticks that follow"
        );
    }
    let before = stored(&p, 0);
    fresh(&p);
    assert_eq!(
        go(&p, &mut board, start(0)).answer,
        Err(halvo_coord::types::Cause::Hold)
    );
    assert_eq!(stored(&p, 0), before, "a Start now stores no failed run");
    assert_eq!(states(&p), Vec::<String>::new());
}

#[test]
fn c_a_paused_that_does_not_reach_the_list_goes_again_at_each_tick_and_no_active() {
    let p = ports(&[
        (APPROVED, ACCOUNT, &["ann@people.test"], &[]),
        (ACTIVE, ACCOUNT, &["ann@people.test"], &[]),
    ]);
    let mut board = life(&p);
    go(&p, &mut board, tick());
    for _ in 0..2 {
        set_listed(&p, 0, APPROVED);
        fresh(&p);
        go(&p, &mut board, tick());
        assert_eq!(
            states(&p),
            ["set_state a0 paused"],
            "paused again, no active"
        );
    }
}

#[test]
fn c_a_move_of_the_person_takes_the_mark_off_and_a_new_life_sends_active_once_more() {
    let p = ports(&[
        (APPROVED, ACCOUNT, &["ann@people.test"], &[]),
        (ACTIVE, ACCOUNT, &["ann@people.test"], &[]),
    ]);
    let mut board = life(&p);
    go(&p, &mut board, tick());
    assert!(board.active_refused(&agent(0)));
    let mut again = life(&p);
    assert!(!again.active_refused(&agent(0)), "a new life holds no mark");
    set_listed(&p, 0, APPROVED);
    fresh(&p);
    go(&p, &mut again, tick());
    assert_eq!(
        states(&p),
        ["set_state a0 active refused", "set_state a0 paused"],
        "a new life sends active once more and meets the same answer"
    );
    assert_eq!(
        go(&p, &mut board, switch_on(0)).answer,
        Err(halvo_coord::types::Cause::NoService),
        "the store refuses the switch-on with overlap"
    );
    assert!(
        !board.active_refused(&agent(0)),
        "the person's move takes the mark off"
    );
    set_listed(&p, 1, PAUSED);
    set_listed(&p, 0, APPROVED);
    fresh(&p);
    go(&p, &mut board, tick());
    assert_eq!(states(&p), ["set_state a0 active"]);
}

#[test]
fn c_an_active_refused_by_another_name_leaves_no_mark_and_goes_again_at_the_next_tick() {
    let p = ports(&[(APPROVED, ACCOUNT, &["ann@people.test"], &[])]);
    let mut board = life(&p);
    p.refuse(Method::Send, refused("busy"));
    fresh(&p);
    go(&p, &mut board, tick());
    assert_eq!(states(&p), ["set_state a0 active refused"]);
    assert!(!board.active_refused(&agent(0)));
    p.clear_refusals();
    fresh(&p);
    go(&p, &mut board, tick());
    assert_eq!(states(&p), ["set_state a0 active"]);
}

#[test]
fn d_a_raise_whose_read_brings_a_lost_switch_off_sends_paused_in_that_tick() {
    let p = ports(&[(ACTIVE, ACCOUNT, &["ann@people.test"], &[])]);
    let mut board = life(&p);
    p.log(&[Event {
        workflow: flow(0),
        agent: agent(0),
        seq: NonZeroU32::new(1).unwrap(),
        day: DAY,
        at: 0,
        what: What::HoldSet {
            cause: HoldSetCause::Person,
        },
    }])
    .unwrap();
    fresh(&p);
    let r = go(&p, &mut board, tick());
    assert_eq!(r.raised, Vec::<AgentId>::new(), "the raise meets the hold");
    assert_eq!(states(&p), ["set_state a0 paused"]);
    assert_eq!(holds_of(&board, 0), vec![OFF]);
    assert_eq!(listed(&p, 0), PAUSED);
}

/// The memory ports, whose owner answers `set_state` with the body `{"state":"approved"}`
/// whatever state it was asked for, after it set the asked state, as an owner whose reply does
/// not hold what it set.
struct Misreply<'a>(&'a Ports<Config>);

impl Owner for Misreply<'_> {
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
        let reply = self.0.send(body)?;
        Ok(match body {
            Body::SetState { .. } => serde_json::json!({ "state": "approved" }),
            _ => reply,
        })
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

impl halvo_coord::ports::Runs for Misreply<'_> {
    fn start(&self, spec: &halvo_coord::types::RunSpec) -> Result<(), PortError> {
        self.0.start(spec)
    }
    fn answer_confirm(
        &self,
        run: &halvo_coord::types::RunId,
        answers: &[halvo_coord::types::Answer],
        record: &mut dyn FnMut(&halvo_coord::types::Decision) -> Result<(), PortError>,
    ) -> Result<(), PortError> {
        self.0.answer_confirm(run, answers, record)
    }
    fn answer_handback(
        &self,
        run: &halvo_coord::types::RunId,
        result: halvo_coord::types::HandbackResult,
    ) -> Result<(), PortError> {
        self.0.answer_handback(run, result)
    }
    fn answer_bind(
        &self,
        run: &halvo_coord::types::RunId,
        result: halvo_coord::types::BindResult,
    ) -> Result<(), PortError> {
        self.0.answer_bind(run, result)
    }
    fn cancel(&self, run: &halvo_coord::types::RunId) -> Result<(), PortError> {
        self.0.cancel(run)
    }
    fn kill(&self, run: &halvo_coord::types::RunId) -> Result<(), PortError> {
        self.0.kill(run)
    }
    fn reap(
        &self,
    ) -> Result<Vec<(halvo_coord::types::RunId, halvo_coord::ports::Reaped)>, PortError> {
        self.0.reap()
    }
}

impl halvo_coord::ports::Analysis for Misreply<'_> {
    fn defer(&self, until: u64) -> Result<(), PortError> {
        self.0.defer(until)
    }
}

impl halvo_coord::ports::Clock for Misreply<'_> {
    fn now(&self) -> Result<u64, PortError> {
        self.0.now()
    }
    fn day(&self, at: u64) -> Result<u32, PortError> {
        self.0.day(at)
    }
}

impl halvo_coord::ports::Rng for Misreply<'_> {
    fn bytes16(&self) -> Result<[u8; 16], PortError> {
        self.0.bytes16()
    }
}

#[test]
fn b_a_set_state_reply_that_does_not_hold_the_state_sent_is_no_acknowledgement() {
    let p = ports(&[(
        PAUSED,
        ACCOUNT,
        &["ann@people.test"],
        &[HoldSetCause::Person],
    )]);
    let mut board = life(&p);
    let before = stored(&p, 0);
    let m = Misreply(&p);
    let r = halvo_coord::schedule::drive(&m, &mut board, &L, switch_on(0));
    assert_eq!(r.answer, Err(halvo_coord::types::Cause::NoService));
    assert_eq!(stored(&p, 0), before, "no hold_cleared");
    assert_eq!(
        board.members[0].facts.workflow.state, PAUSED,
        "no state written"
    );
    assert_eq!(holds_of(&board, 0), vec![OFF]);
}

#[test]
fn c_no_active_goes_under_the_mark_also_for_a_workflow_that_left_the_list() {
    let p = ports(&[
        (APPROVED, ACCOUNT, &["ann@people.test"], &[]),
        (ACTIVE, ACCOUNT, &["ann@people.test"], &[]),
    ]);
    let mut board = life(&p);
    go(&p, &mut board, tick());
    assert!(board.active_refused(&agent(0)));
    board.members[0].facts.workflow.state = APPROVED;
    p.workflows.borrow_mut().remove(0);
    fresh(&p);
    go(&p, &mut board, tick());
    assert_eq!(states(&p), Vec::<String>::new());
}

/// The memory ports behind an owner whose answer to `set_state` `paused` the test chooses: the
/// call does not reach the owner and answers `busy`, or it reaches the owner, which sets
/// `paused`, and its reply is lost (`no_service`). Every other call goes to the memory ports.
struct PausedLost<'a> {
    p: &'a Ports<Config>,
    form: &'static str,
}

impl Owner for PausedLost<'_> {
    type Config = Config;
    fn log(&self, events: &[Event]) -> Result<(), PortError> {
        self.p.log(events)
    }
    fn history(&self, agent: &AgentId, after: u32) -> Result<(Vec<Event>, u32), PortError> {
        self.p.history(agent, after)
    }
    fn workflows(&self) -> Result<Vec<Workflow>, PortError> {
        self.p.workflows()
    }
    fn agents(&self) -> Result<Vec<Agent<Config>>, PortError> {
        self.p.agents()
    }
    fn send(&self, body: &Body) -> Result<halvo_coord::types::Reply, PortError> {
        let paused = matches!(
            body,
            Body::SetState {
                state: AgentState::Paused,
                ..
            }
        );
        match (paused, self.form) {
            (true, "busy") => Err(PortError::Busy),
            (true, "reply lost") => self.p.send(body).and(Err(PortError::NoService)),
            _ => self.p.send(body),
        }
    }
    fn replies(
        &self,
        workflow: &WorkflowId,
        agent: &AgentId,
        run: &halvo_coord::types::RunId,
    ) -> Result<Vec<halvo_coord::types::ReplyRow>, PortError> {
        self.p.replies(workflow, agent, run)
    }
    fn overlaps(
        &self,
        workflow: &WorkflowId,
        agent: &AgentId,
        slots: Option<&[Slot]>,
    ) -> Result<halvo_coord::ports::Overlaps, PortError> {
        self.p.overlaps(workflow, agent, slots)
    }
}

impl halvo_coord::ports::Runs for PausedLost<'_> {
    fn start(&self, spec: &halvo_coord::types::RunSpec) -> Result<(), PortError> {
        self.p.start(spec)
    }
    fn answer_confirm(
        &self,
        run: &halvo_coord::types::RunId,
        answers: &[halvo_coord::types::Answer],
        record: &mut dyn FnMut(&halvo_coord::types::Decision) -> Result<(), PortError>,
    ) -> Result<(), PortError> {
        self.p.answer_confirm(run, answers, record)
    }
    fn answer_handback(
        &self,
        run: &halvo_coord::types::RunId,
        result: halvo_coord::types::HandbackResult,
    ) -> Result<(), PortError> {
        self.p.answer_handback(run, result)
    }
    fn answer_bind(
        &self,
        run: &halvo_coord::types::RunId,
        result: halvo_coord::types::BindResult,
    ) -> Result<(), PortError> {
        self.p.answer_bind(run, result)
    }
    fn cancel(&self, run: &halvo_coord::types::RunId) -> Result<(), PortError> {
        self.p.cancel(run)
    }
    fn kill(&self, run: &halvo_coord::types::RunId) -> Result<(), PortError> {
        self.p.kill(run)
    }
    fn reap(
        &self,
    ) -> Result<Vec<(halvo_coord::types::RunId, halvo_coord::ports::Reaped)>, PortError> {
        self.p.reap()
    }
}

impl halvo_coord::ports::Analysis for PausedLost<'_> {
    fn defer(&self, until: u64) -> Result<(), PortError> {
        self.p.defer(until)
    }
}

impl halvo_coord::ports::Clock for PausedLost<'_> {
    fn now(&self) -> Result<u64, PortError> {
        self.p.now()
    }
    fn day(&self, at: u64) -> Result<u32, PortError> {
        self.p.day(at)
    }
}

impl halvo_coord::ports::Rng for PausedLost<'_> {
    fn bytes16(&self) -> Result<[u8; 16], PortError> {
        self.p.bytes16()
    }
}

/// The store state of ruling RD-67 4.7, with agent 0 on a schedule whose slot is due today and
/// one completed run, so that a tick would start it and a Start now would reach the owner.
fn due_beside_a_sharing_agent() -> Ports<Config> {
    let p = ports(&[
        (APPROVED, ACCOUNT, &["ann@people.test"], &[]),
        (ACTIVE, ACCOUNT, &["ann@people.test"], &[]),
    ]);
    if let Workflow::Full(f) = &mut p.workflows.borrow_mut()[0] {
        f.trigger = WorkflowTrigger::Schedule;
        f.period = NonZeroU32::new(2);
    }
    let run = halvo_coord::types::RunId::parse(&format!("{:032x}", 0x900)).unwrap();
    p.agents.borrow_mut()[0].last = Some(halvo_coord::types::PastRun {
        run: run.clone(),
        day: DAY - 2,
        outcome: halvo_coord::types::Outcome::Completed,
        unknown: 0,
    });
    let e = |seq: u32, what: What| Event {
        workflow: flow(0),
        agent: agent(0),
        seq: NonZeroU32::new(seq).unwrap(),
        day: DAY - 2,
        at: 0,
        what,
    };
    p.log(&[
        e(
            1,
            What::RunStarted {
                run: run.clone(),
                trigger: halvo_coord::types::Trigger::Person,
                slot: None,
            },
        ),
        e(
            2,
            What::RunEnded {
                run,
                cause: halvo_coord::types::RunEnd::Completed,
                defers: None,
            },
        ),
    ])
    .unwrap();
    p.calls.borrow_mut().clear();
    p.tried.borrow_mut().clear();
    p
}

/// Ruling RD-160 (1), from the verify step's row V1: after the tick's `active` is refused with
/// `overlap` and its `paused` gets no acknowledgement, in both forms (`busy`, and a reply lost
/// after the owner set `paused`), the agent stands as off while that `paused` is owed: the tick
/// starts nothing and raises nothing, the row reads `line.off.kept` with the controls of the
/// hold `off`, and three Starts now each answer `hold` and store nothing, so that no run is
/// stored as failed and no hold `failures` is set.
#[test]
fn c_a_paused_with_no_acknowledgement_after_a_refused_active_holds_every_start() {
    use halvo_coord::types::{Cause, Control, StatusLine};
    for form in ["busy", "reply lost"] {
        let p = due_beside_a_sharing_agent();
        let mut board = life(&p);
        let before = stored(&p, 0);
        let t = PausedLost { p: &p, form };
        for k in 0..3 {
            let r = halvo_coord::schedule::drive(&t, &mut board, &L, tick());
            assert!(r.started.is_empty(), "{form} tick {k}: {:?}", r.started);
            assert!(r.raised.is_empty(), "{form} tick {k}: {:?}", r.raised);
            assert_eq!(
                stored(&p, 0),
                before,
                "{form} tick {k}: no run_started, no run_ended"
            );
            assert_eq!(line_of(&board, 0), StatusLine::Kept, "{form} tick {k}");
            assert_eq!(
                halvo_coord::roster::controls(&board.members[0].facts),
                vec![Control::SwitchOn, Control::Delete],
                "{form} tick {k}"
            );
        }
        for k in 0..3 {
            let r = halvo_coord::schedule::drive(&t, &mut board, &L, start(0));
            assert_eq!(r.answer, Err(Cause::Hold), "{form} start {k}");
            assert_eq!(stored(&p, 0), before, "{form} start {k}: nothing stored");
            assert!(!holds_of(&board, 0).contains(&FAILURES), "{form} start {k}");
        }
        assert!(!stored(&p, 0).contains(&"hold_set"), "{form}");
    }
}

#[test]
fn c_with_the_paused_of_a_refused_active_acknowledged_the_tick_starts_nothing() {
    let p = due_beside_a_sharing_agent();
    let mut board = life(&p);
    let before = stored(&p, 0);
    let r = go(&p, &mut board, tick());
    assert!(r.started.is_empty(), "{:?}", r.started);
    assert_eq!(stored(&p, 0), before);
    assert_eq!(listed(&p, 0), PAUSED);
}
