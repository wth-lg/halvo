mod common;

use common::*;
use halvo_agent_store::{Host, MemoryStore};
use halvo_policy::{preset, validate};
use halvo_schema::executable::CoordinatorReply;
use halvo_schema::{
    AgentId, Category, ErrorName, SeedCategory, TypedRecord, Workflow, WorkflowId, WorkflowState,
};
use serde_json::json;

fn state_of(store: &MemoryStore, w: u32) -> (WorkflowState, Option<String>) {
    match stored_workflow(store, w) {
        Workflow::Full(f) => (f.state, f.agent.map(|a| a.as_str().to_owned())),
        Workflow::Dismissed(_) => panic!("a full workflow"),
    }
}

fn set_state(store: &mut MemoryStore, a: u32, state: &str) -> Result<CoordinatorReply, ErrorName> {
    coordinator(
        store,
        json!({"op": "set_state", "workflow": wf(1), "agent": agent(a), "state": state}),
    )
}

fn start(store: &mut MemoryStore, a: u32, mode: &str) -> Result<CoordinatorReply, ErrorName> {
    coordinator(
        store,
        json!({"op": "start_run", "workflow": wf(1), "agent": agent(a), "run": run(1), "mode": mode}),
    )
}

fn state_reply(state: &str) -> CoordinatorReply {
    CoordinatorReply::parse(format!(r#"{{"state":"{state}"}}"#).as_bytes()).expect("a state reply")
}

#[test]
fn create_agent_stores_the_preset_and_makes_the_state_call() {
    for category in SeedCategory::ALL {
        let mut store = MemoryStore::new();
        store.put_workflow(workflow(&wf(1), category.as_str(), "approved", None));
        assert_eq!(create(&mut store, 1, 1), Ok(rev(1)));
        let stored = configs(&store);
        let expected = preset(
            Category::from(*category),
            WorkflowId::parse(&wf(1)).expect("an id"),
            AgentId::parse(&agent(1)).expect("an id"),
        )
        .expect("a preset");
        assert_eq!(validate(&expected), Ok(()));
        assert_eq!(stored, [expected]);
        assert_eq!(
            state_of(&store, 1),
            (WorkflowState::Approved, Some(agent(1)))
        );
    }
}

#[test]
fn create_agent_repeated_changes_nothing() {
    let mut store = store_with("email");
    assert_eq!(create(&mut store, 1, 1), Ok(rev(1)));
    assert_eq!(configs(&store).len(), 1);
    assert_eq!(
        set_state(&mut store, 1, "active"),
        Ok(state_reply("active"))
    );
    assert_eq!(create(&mut store, 1, 1), Ok(rev(1)));
    assert_eq!(configs(&store).len(), 1);
    assert_eq!(state_of(&store, 1), (WorkflowState::Active, Some(agent(1))));
}

#[test]
fn create_agent_after_a_lost_state_call_makes_it_and_stores_no_second_config() {
    let mut store = MemoryStore::new();
    store.put_workflow(workflow(&wf(1), "email", "approved", None));
    let config = preset(
        Category::Email,
        WorkflowId::parse(&wf(1)).expect("an id"),
        AgentId::parse(&agent(1)).expect("an id"),
    )
    .expect("a preset");
    let json = config.to_json().expect("the config's JSON");
    assert_eq!(
        store.commit(vec![TypedRecord {
            kind: "agent_config",
            json
        }]),
        Ok(())
    );
    assert_eq!(create(&mut store, 1, 1), Ok(rev(1)));
    assert_eq!(configs(&store).len(), 1);
    assert_eq!(
        state_of(&store, 1),
        (WorkflowState::Approved, Some(agent(1)))
    );
}

#[test]
fn create_agent_refusals() {
    let mut store = store_with("email");
    assert_eq!(create(&mut store, 1, 2), Err(declared("binding")));
    assert_eq!(configs(&store).len(), 1);
    assert_eq!(create(&mut store, 9, 1), Err(ErrorName::NoWorkflow));

    store.put_workflow(workflow(&wf(2), "email", "approved", None));
    assert_eq!(create(&mut store, 2, 1), Err(declared("binding")));
    assert_eq!(configs(&store).len(), 1);
    assert_eq!(state_of(&store, 2), (WorkflowState::Approved, None));

    store.put_workflow(workflow(&wf(3), "email", "proposed", None));
    assert_eq!(create(&mut store, 3, 3), Err(ErrorName::State));
    store.put_workflow(dismissed(&wf(4)));
    assert_eq!(create(&mut store, 4, 4), Err(ErrorName::State));
    assert_eq!(configs(&store).len(), 1);

    store.put_workflow(workflow(&wf(5), "email", "approved", None));
    assert_eq!(create(&mut store, 5, 5), Ok(rev(1)));
    assert_eq!(configs(&store).len(), 2);
}

#[test]
fn a_body_that_is_not_the_form_is_malformed() {
    let mut store = store_with("email");
    for body in [
        json!({"op": "create_agent", "workflow": wf(1)}),
        json!({"op": "list_agents", "workflow": wf(1), "agent": agent(1)}),
        json!({"op": "create_agent", "workflow": wf(1), "agent": agent(1), "extra": 1}),
        json!(["create_agent"]),
    ] {
        assert_eq!(coordinator(&mut store, body), Err(ErrorName::Malformed));
    }
    assert_eq!(
        halvo_agent_store::coordinator(&mut store, NOW, b"{"),
        Err(ErrorName::Malformed)
    );
    assert_eq!(configs(&store).len(), 1);
}

#[test]
fn set_state_moves_the_workflow_of_its_agent() {
    let mut store = store_with("email");
    assert_eq!(
        set_state(&mut store, 1, "active"),
        Ok(state_reply("active"))
    );
    assert_eq!(state_of(&store, 1), (WorkflowState::Active, Some(agent(1))));
    assert_eq!(
        set_state(&mut store, 1, "active"),
        Ok(state_reply("active"))
    );
    assert_eq!(
        set_state(&mut store, 1, "paused"),
        Ok(state_reply("paused"))
    );
    assert_eq!(state_of(&store, 1), (WorkflowState::Paused, Some(agent(1))));
    assert_eq!(
        set_state(&mut store, 1, "approved"),
        Ok(state_reply("approved"))
    );
    assert_eq!(
        state_of(&store, 1),
        (WorkflowState::Approved, Some(agent(1)))
    );
    assert_eq!(set_state(&mut store, 2, "active"), Err(declared("binding")));
    assert_eq!(
        state_of(&store, 1),
        (WorkflowState::Approved, Some(agent(1)))
    );
    assert_eq!(
        coordinator(
            &mut store,
            json!({"op": "set_state", "workflow": wf(9), "agent": agent(1), "state": "active"})
        ),
        Err(ErrorName::NoWorkflow)
    );
}

#[test]
fn set_state_active_needs_a_playbook_for_a_web_agent() {
    let mut store = store_with("utility_bill_payment");
    assert_eq!(
        set_state(&mut store, 1, "active"),
        Err(declared("playbook"))
    );
    assert_eq!(
        state_of(&store, 1),
        (WorkflowState::Approved, Some(agent(1)))
    );
    assert_eq!(
        set_state(&mut store, 1, "paused"),
        Ok(state_reply("paused"))
    );
    let row = json!({
        "v": 1,
        "workflow": wf(1),
        "agent": agent(1),
        "number": 1,
        "n": 1,
        "step": step(1, "navigate", "read"),
    });
    assert_eq!(
        store.commit(vec![TypedRecord {
            kind: "agent_playbook",
            json: row.to_string()
        }]),
        Ok(())
    );
    assert_eq!(
        set_state(&mut store, 1, "active"),
        Ok(state_reply("active"))
    );

    let mut other = MemoryStore::new();
    other.put_workflow(workflow(&wf(1), "utility_bill_payment", "approved", None));
    create(&mut other, 1, 1).expect("create_agent");
    other.put_workflow(workflow(&wf(2), "email", "approved", None));
    create(&mut other, 2, 2).expect("create_agent");
    let row = json!({
        "v": 1,
        "workflow": wf(2),
        "agent": agent(2),
        "number": 1,
        "n": 1,
        "step": step(1, "navigate", "read"),
    });
    assert_eq!(
        other.commit(vec![TypedRecord {
            kind: "agent_playbook",
            json: row.to_string()
        }]),
        Ok(())
    );
    assert_eq!(
        set_state(&mut other, 1, "active"),
        Err(declared("playbook"))
    );
}

#[test]
fn set_state_passes_the_state_call_refusal_on() {
    let mut store = MemoryStore::new();
    store.put_workflow(workflow(&wf(1), "email", "proposed", Some(&agent(1))));
    assert_eq!(set_state(&mut store, 1, "paused"), Err(ErrorName::State));
}

#[test]
fn start_run_checks_the_state_and_the_agent() {
    let mut store = store_with("email");
    assert_eq!(start(&mut store, 1, "compile"), Ok(accepted()));
    assert_eq!(start(&mut store, 1, "replay"), Err(ErrorName::State));
    assert_eq!(start(&mut store, 2, "compile"), Err(declared("binding")));
    set_state(&mut store, 1, "active").expect("active");
    assert_eq!(start(&mut store, 1, "compile"), Ok(accepted()));
    assert_eq!(start(&mut store, 1, "replay"), Ok(accepted()));
    set_state(&mut store, 1, "paused").expect("paused");
    assert_eq!(start(&mut store, 1, "compile"), Err(ErrorName::State));
    assert_eq!(start(&mut store, 1, "replay"), Err(ErrorName::State));
    assert_eq!(
        coordinator(
            &mut store,
            json!({"op": "start_run", "workflow": wf(9), "agent": agent(1), "run": run(1), "mode": "compile"})
        ),
        Err(ErrorName::NoWorkflow)
    );
    assert!(rows(&store, "agent_run").is_empty());
}

#[test]
fn a_dismissed_workflow_has_no_agent() {
    let mut store = MemoryStore::new();
    store.put_workflow(dismissed(&wf(1)));
    assert_eq!(set_state(&mut store, 1, "active"), Err(declared("binding")));
    assert_eq!(start(&mut store, 1, "compile"), Err(declared("binding")));
}
