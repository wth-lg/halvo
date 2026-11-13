mod common;

use common::{
    bodies, count, episode, open_host, position, raw_request, store_id, Grouping, Scratch, Seq,
    TestClock,
};
use halvo_derived::{Error, Handler, HandlerCx, Host, MoveTo, Store, Takes};
use halvo_schema_core::ids::{AgentId, RequestId, WorkflowId};
use halvo_schema_core::record::{Workflow, WorkflowState};
use halvo_schema_core::request::{ErrorName, InterfaceDecision, InterfaceRole, RawRequest};
use serde_json::value::RawValue;

const DAY: u32 = 20_767;

/// A handler that makes the state call that its body names:
/// `{"op":"set_state","workflow":<id>,"state":"approved"|"active"|"paused","agent":<id>|null}`.
struct SetState;

impl Handler for SetState {
    fn handle(
        &self,
        request: &RawRequest,
        cx: &mut HandlerCx<'_>,
    ) -> Result<Box<RawValue>, ErrorName> {
        let body: serde_json::Value = serde_json::from_str(request.body().get()).unwrap();
        let workflow = WorkflowId::parse(body["workflow"].as_str().unwrap()).unwrap();
        let to = match body["state"].as_str().unwrap() {
            "approved" => MoveTo::Approved,
            "active" => MoveTo::Active,
            _ => MoveTo::Paused,
        };
        let agent = body["agent"].as_str().map(|a| AgentId::parse(a).unwrap());
        match cx.state(&workflow, to, agent) {
            Ok(()) => Ok(RawValue::from_string("null".to_owned()).unwrap()),
            Err(Error::Refused(name)) => Err(name),
            Err(Error::Fault(_)) => Err(ErrorName::Internal),
        }
    }
}

fn host(clock: &TestClock, takes: Takes) -> Host {
    Host::new(Box::new(clock.clone()), Box::new(Grouping { min: 1 })).handler(
        "test",
        takes,
        Box::new(SetState),
    )
}

fn call(
    store: &mut Store,
    name: &str,
    workflow: &str,
    state: &str,
    agent: Option<&str>,
) -> Result<(), Error> {
    let agent = agent.map_or("null".to_owned(), |a| format!("\"{a}\""));
    let body = format!(
        r#"{{"op":"set_state","workflow":"{workflow}","state":"{state}","agent":{agent}}}"#
    );
    store.handle(&raw_request(name, &body)).map(|_| ())
}

fn stored(scratch: &Scratch) -> Workflow {
    serde_json::from_str(&bodies(scratch, "workflow")[0]).unwrap()
}

fn revision(scratch: &Scratch) -> i64 {
    count(scratch, "SELECT n FROM revisions WHERE family = 'workflow'")
}

/// A store with one proposed workflow, and its id.
fn proposed(scratch: &Scratch, clock: &TestClock, takes: Takes) -> (Store, String) {
    let mut store = open_host(scratch, host(clock, takes)).unwrap();
    store
        .commit_batch(
            &store_id(1),
            &[episode(1, DAY, None)],
            position(1, 0),
            &mut Seq(100),
        )
        .unwrap();
    drop(store);
    let id = stored(scratch).id().as_str().to_owned();
    (open_host(scratch, host(clock, takes)).unwrap(), id)
}

fn approve(store: &mut Store, id: &str) {
    store
        .decide(&InterfaceDecision::approve(
            &InterfaceRole::claim(),
            RequestId::from_random([2; 16]),
            WorkflowId::parse(id).unwrap(),
        ))
        .unwrap();
}

const SET: Takes = Takes::Coordinator(&["set_state"]);

#[test]
fn the_state_call_moves_an_approved_workflow_among_approved_active_and_paused() {
    let scratch = Scratch::new("state-call");
    let clock = TestClock::at_day(DAY);
    let (mut store, w) = proposed(&scratch, &clock, SET);
    approve(&mut store, &w);
    let agent = "4".repeat(32);
    drop(store);
    let mut last = revision(&scratch);
    let steps: [(&str, Option<&str>, bool); 5] = [
        ("active", Some(&agent), true),
        ("active", Some(&agent), false),
        ("paused", Some(&agent), true),
        ("approved", None, true),
        ("approved", None, false),
    ];
    for (state, with, changes) in steps {
        let mut store = open_host(&scratch, host(&clock, SET)).unwrap();
        assert_eq!(
            call(&mut store, "coordinator_write", &w, state, with),
            Ok(())
        );
        drop(store);
        let Workflow::Full(full) = stored(&scratch) else {
            panic!("dismissed");
        };
        assert_eq!(full.state.as_str(), state);
        assert_eq!(full.agent.as_ref().map(|a| a.as_str()), with);
        let now = revision(&scratch);
        assert_eq!(now - last, i64::from(changes), "{state}");
        last = now;
    }
}

#[test]
fn the_state_call_refuses_a_proposed_a_dismissed_and_a_missing_workflow_and_an_active_one_with_no_agent(
) {
    let scratch = Scratch::new("state-refusals");
    let clock = TestClock::at_day(DAY);
    let (mut store, w) = proposed(&scratch, &clock, SET);
    let agent = "4".repeat(32);
    assert_eq!(
        call(&mut store, "coordinator_write", &w, "active", Some(&agent)),
        Err(Error::Refused(ErrorName::State))
    );
    assert_eq!(
        call(
            &mut store,
            "coordinator_write",
            &"7".repeat(32),
            "approved",
            None
        ),
        Err(Error::Refused(ErrorName::NoWorkflow))
    );
    approve(&mut store, &w);
    for state in ["active", "paused"] {
        assert_eq!(
            call(&mut store, "coordinator_write", &w, state, None),
            Err(Error::Refused(ErrorName::State))
        );
    }
    drop(store);
    let Workflow::Full(full) = stored(&scratch) else {
        panic!("dismissed");
    };
    assert_eq!(full.state, WorkflowState::Approved);
    let scratch = Scratch::new("state-dismissed");
    let (mut store, w) = proposed(&scratch, &clock, SET);
    store
        .decide(&InterfaceDecision::dismiss(
            &InterfaceRole::claim(),
            RequestId::from_random([2; 16]),
            WorkflowId::parse(&w).unwrap(),
        ))
        .unwrap();
    assert_eq!(
        call(&mut store, "coordinator_write", &w, "approved", None),
        Err(Error::Refused(ErrorName::State))
    );
}

#[test]
fn the_state_call_is_refused_while_a_handler_answers_a_request_of_another_role() {
    let scratch = Scratch::new("state-runner");
    let clock = TestClock::at_day(DAY);
    let (mut store, w) = proposed(&scratch, &clock, Takes::Runner);
    approve(&mut store, &w);
    let agent = "4".repeat(32);
    assert_eq!(
        call(&mut store, "runner", &w, "active", Some(&agent)),
        Err(Error::Refused(ErrorName::Role))
    );
    drop(store);
    let Workflow::Full(full) = stored(&scratch) else {
        panic!("dismissed");
    };
    assert_eq!(full.state, WorkflowState::Approved);
}

#[test]
fn the_state_call_answers_no_workflow_for_a_workflow_that_is_not_stored_before_the_rules_of_the_move(
) {
    let scratch = Scratch::new("state-order");
    let clock = TestClock::at_day(DAY);
    let (store, w) = proposed(&scratch, &clock, SET);
    drop(store);
    let agent = "4".repeat(32);
    let every = |store: &mut Store, id: &str| -> Vec<Result<(), Error>> {
        let mut answers = Vec::new();
        for state in ["approved", "active", "paused"] {
            for with in [Some(agent.as_str()), None] {
                answers.push(call(store, "coordinator_write", id, state, with));
            }
        }
        answers
    };
    let before = revision(&scratch);
    let mut store = open_host(&scratch, host(&clock, SET)).unwrap();
    let missing = "7".repeat(32);
    assert_eq!(
        every(&mut store, &missing),
        vec![Err(Error::Refused(ErrorName::NoWorkflow)); 6]
    );
    assert_eq!(
        every(&mut store, &w),
        vec![Err(Error::Refused(ErrorName::State)); 6],
        "proposed"
    );
    drop(store);
    assert_eq!(revision(&scratch), before);
    let mut store = open_host(&scratch, host(&clock, SET)).unwrap();
    store
        .decide(&InterfaceDecision::dismiss(
            &InterfaceRole::claim(),
            RequestId::from_random([2; 16]),
            WorkflowId::parse(&w).unwrap(),
        ))
        .unwrap();
    assert_eq!(
        every(&mut store, &w),
        vec![Err(Error::Refused(ErrorName::State)); 6],
        "dismissed"
    );
    store
        .decide(&InterfaceDecision::delete(
            &InterfaceRole::claim(),
            RequestId::from_random([3; 16]),
            WorkflowId::parse(&w).unwrap(),
        ))
        .unwrap();
    assert_eq!(
        every(&mut store, &w),
        vec![Err(Error::Refused(ErrorName::NoWorkflow)); 6],
        "deleted"
    );
}

#[test]
fn a_state_call_in_the_same_state_sets_the_agent_given_or_clears_it() {
    let scratch = Scratch::new("state-same-agent");
    let clock = TestClock::at_day(DAY);
    let (mut store, w) = proposed(&scratch, &clock, SET);
    approve(&mut store, &w);
    drop(store);
    let a = "4".repeat(32);
    let b = "5".repeat(32);
    let steps: [(&str, Option<&str>); 5] = [
        ("approved", Some(&a)),
        ("approved", Some(&b)),
        ("approved", None),
        ("active", Some(&a)),
        ("active", Some(&b)),
    ];
    let mut seen = Vec::new();
    for (state, with) in steps {
        let before = revision(&scratch);
        let mut store = open_host(&scratch, host(&clock, SET)).unwrap();
        let answer = call(&mut store, "coordinator_write", &w, state, with);
        drop(store);
        let Workflow::Full(full) = stored(&scratch) else {
            panic!("dismissed");
        };
        seen.push((
            answer,
            full.state.as_str().to_owned(),
            full.agent.as_ref().map(|x| x.as_str().to_owned()),
            revision(&scratch) - before,
        ));
    }
    let want: Vec<_> = steps
        .iter()
        .map(|(state, with)| (Ok(()), (*state).to_owned(), with.map(|x| x.to_owned()), 1))
        .collect();
    assert_eq!(
        seen, want,
        "(answer, state, agent, rise of the revision) per call: each call sets the agent given, once"
    );
}
