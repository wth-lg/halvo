mod common;

use common::*;
use halvo_agent_store::{Host, MemoryStore, AGENT_KINDS};
use halvo_policy::{preset, validate};
use halvo_schema::executable::names::LogEvent;
use halvo_schema::executable::{AgentLog, CoordinatorReply};
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
    let started = start_rows(&store);
    assert_eq!(started.len(), 1);
    assert_eq!(started[0].run.as_str(), run(1));
}

fn start_rows(store: &MemoryStore) -> Vec<AgentLog> {
    rows(store, "agent_log")
        .iter()
        .map(|r| AgentLog::parse(r.as_bytes()).expect("a log row"))
        .filter(|l| l.event == LogEvent::StartAccepted)
        .collect()
}

#[test]
fn start_run_stores_one_start_row_from_the_request_s_clock() {
    let mut store = unstarted("email").with_utc_offset(WEST);
    let night = at(local(TODAY, 22 * 3600, WEST));
    assert_eq!(night.at.0.div_euclid(NANOS_PER_DAY), i64::from(TODAY) + 1);
    assert_eq!(start_at(&mut store, night, 1, 1, 1), Ok(accepted()));
    assert_eq!(rows(&store, "agent_log").len(), 1);
    let row = start_rows(&store).pop().expect("the start row");
    assert_eq!(
        (row.workflow.as_str(), row.agent.as_str(), row.run.as_str()),
        (wf(1).as_str(), agent(1).as_str(), run(1).as_str())
    );
    assert_eq!((row.seq.get(), row.day, row.at), (1, TODAY, night.at));
    assert_eq!(
        (
            row.step,
            row.cause,
            row.model,
            row.ms,
            row.tokens_in,
            row.tokens_out
        ),
        (None, None, None, None, None, None)
    );
}

#[test]
fn a_start_run_repeated_for_the_same_agent_and_workflow_is_answered_as_the_first_and_stores_nothing(
) {
    let mut store = unstarted("email");
    assert_eq!(start_at(&mut store, NOW, 1, 1, 1), Ok(accepted()));
    let before = rows(&store, "agent_log");
    let later = at(NOW.at.0 + NANOS_PER_DAY);
    assert_eq!(start_at(&mut store, later, 1, 1, 1), Ok(accepted()));
    assert_eq!(rows(&store, "agent_log"), before);
}

#[test]
fn a_start_run_of_a_run_that_another_agent_s_rows_name_is_refused_with_binding() {
    let mut store = unstarted("email");
    store.put_workflow(workflow(&wf(2), "email", "approved", None));
    create(&mut store, 2, 2).expect("create_agent of the second agent");
    assert_eq!(start_at(&mut store, NOW, 2, 2, 1), Ok(accepted()));
    let before = rows(&store, "agent_log");
    assert_eq!(start_at(&mut store, NOW, 1, 1, 1), Err(declared("binding")));
    assert_eq!(rows(&store, "agent_log"), before);
}

#[test]
fn a_start_run_after_the_run_s_end_is_refused_with_effect() {
    let mut store = unstarted("email");
    assert_eq!(start_at(&mut store, NOW, 1, 1, 1), Ok(accepted()));
    assert_eq!(end_run(&mut store, 1), Ok(accepted()));
    let before = rows(&store, "agent_log");
    assert_eq!(start_at(&mut store, NOW, 1, 1, 1), Err(declared("effect")));
    assert_eq!(end_run(&mut store, 2), Ok(accepted()));
    assert_eq!(start_at(&mut store, NOW, 1, 1, 2), Err(declared("effect")));
    assert_eq!(rows(&store, "agent_log"), before);
}

#[test]
fn a_start_run_of_another_agent_for_a_run_that_end_run_ended_is_refused_with_binding() {
    let mut store = unstarted("email");
    assert_eq!(end_run(&mut store, 7), Ok(accepted()));
    store.put_workflow(workflow(&wf(2), "email", "approved", None));
    create(&mut store, 2, 2).expect("create_agent of the second agent");
    let before = rows(&store, "agent_log");
    assert_eq!(start_at(&mut store, NOW, 2, 2, 7), Err(declared("binding")));
    assert_eq!(rows(&store, "agent_log"), before);
}

#[test]
fn a_dismissed_workflow_has_no_agent() {
    let mut store = MemoryStore::new();
    store.put_workflow(dismissed(&wf(1)));
    assert_eq!(set_state(&mut store, 1, "active"), Err(declared("binding")));
    assert_eq!(start(&mut store, 1, "compile"), Err(declared("binding")));
}

fn start_row_of(a: u32, at_nanos: i64) -> serde_json::Value {
    json!({"v": 1, "workflow": wf(1), "agent": agent(a), "day": TODAY, "at": at_nanos})
}

fn start_rows_of_kind(store: &MemoryStore) -> Vec<serde_json::Value> {
    rows(store, "agent_start")
        .iter()
        .map(|r| serde_json::from_str(r).expect("a stored start row"))
        .collect()
}

fn load_start(store: &mut MemoryStore, a: u32, r: u32) -> Option<serde_json::Value> {
    let body = json!({"op": "load", "agent": agent(a), "run": run(r)});
    match runner(store, body) {
        Ok(halvo_schema::executable::RunnerReply::Load(reply)) => reply
            .start
            .map(|s| serde_json::from_str(&s.to_json().expect("text")).expect("json")),
        other => panic!("a reply of load, not {other:?}"),
    }
}

#[test]
fn create_agent_stores_the_start_row_in_the_commit_of_the_preset_from_the_request_s_clock() {
    let mut store = MemoryStore::new();
    store.put_workflow(workflow(&wf(1), "email", "approved", None));
    let mut host = Stopper::new(&mut store, None);
    let body = json!({"op": "create_agent", "workflow": wf(1), "agent": agent(1)});
    assert_eq!(
        halvo_agent_store::coordinator(&mut host, NOW, body.to_string().as_bytes()),
        Ok(rev(1))
    );
    assert_eq!(host.commits, 1);
    assert_eq!(configs(&store).len(), 1);
    assert_eq!(start_rows_of_kind(&store), [start_row_of(1, NOW.at.0)]);
}

#[test]
fn a_repeated_create_agent_stores_the_start_row_only_when_none_stands() {
    let mut store = MemoryStore::new();
    store.put_workflow(workflow(&wf(1), "email", "approved", None));
    create(&mut store, 1, 1).expect("create_agent");
    let later = at(NOW.at.0 + 60_000_000_000);
    let body = json!({"op": "create_agent", "workflow": wf(1), "agent": agent(1)});
    assert_eq!(coordinator_at(&mut store, later, body.clone()), Ok(rev(1)));
    assert_eq!(start_rows_of_kind(&store), [start_row_of(1, NOW.at.0)]);

    let mut bare = unattended_sender();
    assert!(start_rows_of_kind(&bare).is_empty());
    assert_eq!(coordinator_at(&mut bare, later, body.clone()), Ok(rev(1)));
    assert_eq!(start_rows_of_kind(&bare), [start_row_of(1, later.at.0)]);
    assert_eq!(coordinator(&mut bare, body), Ok(rev(1)));
    assert_eq!(start_rows_of_kind(&bare), [start_row_of(1, later.at.0)]);
}

#[test]
fn a_configuration_stored_later_does_not_move_the_start_row_and_load_answers_it() {
    let mut store = store_with("email");
    assert_eq!(
        load_start(&mut store, 1, 1),
        Some(start_row_of(1, NOW.at.0))
    );
    let mut config = configs(&store).pop().expect("a config");
    config.rev = std::num::NonZeroU32::new(2).expect("2");
    store
        .commit(vec![TypedRecord {
            kind: "agent_config",
            json: config.to_json().expect("the config's JSON"),
        }])
        .expect("a later config");
    let later = at(NOW.at.0 + 60_000_000_000);
    let body = json!({"op": "create_agent", "workflow": wf(1), "agent": agent(1)});
    assert_eq!(coordinator_at(&mut store, later, body), Ok(rev(1)));
    assert_eq!(start_rows_of_kind(&store), [start_row_of(1, NOW.at.0)]);
    assert_eq!(
        load_start(&mut store, 1, 1),
        Some(start_row_of(1, NOW.at.0))
    );
}

#[test]
fn load_answers_start_null_where_no_start_row_of_the_agent_stands() {
    let mut store = unattended_sender();
    assert_eq!(load_start(&mut store, 1, 1), None);
}

#[test]
fn create_agent_after_a_stop_behind_its_commit_and_sent_again_with_a_new_agent_id() {
    let mut store = MemoryStore::new();
    store.put_workflow(workflow(&wf(1), "email", "approved", None));
    let mut host = Stopper::new(&mut store, Some(1));
    let body = json!({"op": "create_agent", "workflow": wf(1), "agent": agent(1)});
    assert_eq!(
        halvo_agent_store::coordinator(&mut host, NOW, body.to_string().as_bytes()),
        Err(ErrorName::Internal)
    );
    assert_eq!(configs(&store).len(), 1);
    assert_eq!(start_rows_of_kind(&store), [start_row_of(1, NOW.at.0)]);
    assert_eq!(state_of(&store, 1), (WorkflowState::Approved, None));

    assert_eq!(create(&mut store, 1, 2), Ok(rev(1)));
    assert_eq!(
        state_of(&store, 1),
        (WorkflowState::Approved, Some(agent(2)))
    );
    assert_eq!(configs(&store).len(), 2);
    assert_eq!(start_rows_of_kind(&store).len(), 2);
    assert_eq!(start_at(&mut store, NOW, 1, 1, 1), Err(declared("binding")));
    let load_first = json!({"op": "load", "agent": agent(1), "run": run(1)});
    assert_eq!(runner(&mut store, load_first), Err(declared("binding")));
    assert_eq!(start_at(&mut store, NOW, 1, 2, 2), Ok(accepted()));
    for kind in halvo_agent_store::AGENT_KINDS.iter().map(|(k, _)| *k) {
        for row in rows(&store, kind) {
            let value: serde_json::Value = serde_json::from_str(&row).expect("a row");
            assert_eq!(value["workflow"], json!(wf(1)), "{kind}: {row}");
        }
    }
}

fn log_at(store: &mut MemoryStore, now: halvo_agent_store::Now) -> Result<(), ErrorName> {
    let day = store.day(now.at);
    let line = json!({
        "v": 1, "workflow": wf(1), "agent": agent(1), "run": run(1), "seq": 2, "day": day,
        "at": now.at.0, "event": "step_ok", "step": 1, "cause": null, "model": null, "ms": null,
        "tokens_in": null, "tokens_out": null,
    });
    runner_at(
        store,
        now,
        json!({"op": "log", "agent": agent(1), "run": run(1), "records": [line]}),
    )
    .map(|_| ())
}

#[test]
fn a_start_run_whose_start_row_lies_past_the_log_s_bound_does_not_open_the_run_again() {
    for days in [-3, 93] {
        let mut store = unstarted("email");
        assert_eq!(start_at(&mut store, NOW, 1, 1, 1), Ok(accepted()));
        let then = at(NOW.at.0 + days * NANOS_PER_DAY);
        assert_eq!(log_at(&mut store, then), Err(declared("binding")), "{days}");
        assert_eq!(
            start_at(&mut store, then, 1, 1, 1),
            Ok(accepted()),
            "{days}"
        );
        assert_eq!(start_rows(&store).len(), 1, "{days}");
        assert_eq!(log_at(&mut store, then), Err(declared("binding")), "{days}");
        assert_eq!(log_at(&mut store, NOW), Ok(()), "{days}");
    }
}

#[test]
fn list_overlaps_is_answered_malformed_and_stores_nothing_and_changes_no_state() {
    let mut store = MemoryStore::new();
    for n in [1, 2] {
        store.put_workflow(workflow(&wf(n), "email", "approved", None));
        assert_eq!(create(&mut store, n, n), Ok(rev(1)), "{n}");
        let slots = json!([
            {"name": "account_address", "value": ["me@mail.test"]},
            {"name": "reply_senders", "value": ["ann@people.test"]}
        ]);
        assert_eq!(
            coordinator(
                &mut store,
                json!({"op": "set_bindings", "workflow": wf(n), "agent": agent(n), "slots": slots}),
            ),
            Ok(rev(1)),
            "{n}"
        );
    }
    let held = |store: &MemoryStore| {
        let rows: Vec<Vec<String>> = AGENT_KINDS
            .iter()
            .map(|(kind, _)| rows(store, kind))
            .collect();
        (rows, state_of(store, 1), state_of(store, 2))
    };
    let before = held(&store);
    for slots in [
        json!(null),
        json!([]),
        json!([{"name": "reply_senders", "value": ["ANN@people.test"]}]),
    ] {
        let body =
            json!({"op": "list_overlaps", "workflow": wf(1), "agent": agent(1), "slots": slots});
        assert_eq!(coordinator(&mut store, body), Err(ErrorName::Malformed));
    }
    for (w, a) in [(9, 1), (1, 2)] {
        let body =
            json!({"op": "list_overlaps", "workflow": wf(w), "agent": agent(a), "slots": null});
        assert_eq!(
            coordinator(&mut store, body),
            Err(ErrorName::Malformed),
            "workflow {w}, agent {a}"
        );
    }
    assert_eq!(held(&store), before);
}
