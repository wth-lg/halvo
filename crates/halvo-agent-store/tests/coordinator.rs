mod common;

use common::*;
use halvo_agent_store::{Host, MemoryStore, AGENT_KINDS, GOES_ON_AFTER};
use halvo_policy::{preset, validate};
use halvo_schema::executable::body::{AgentsReply, CHECKPOINTS_REPLY_MAX};
use halvo_schema::executable::names::{Cause, LogEvent};
use halvo_schema::executable::{AgentLog, CoordinatorReply, RunnerReply};
use halvo_schema::{
    AgentId, Category, ErrorName, SeedCategory, TypedRecord, Workflow, WorkflowId, WorkflowState,
};
use serde_json::{json, Value};

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
        json!({"op": "list_agents", "workflow": null, "agent": null}),
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
        Err(declared("playbook"))
    );
    assert_eq!(store.commit(vec![finished_row(1, 1, 1, 9)]), Ok(()));
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
fn a_web_agent_with_a_half_recorded_playbook_is_not_switched_on() {
    let mut store = store_with("utility_bill_payment");
    let approved = (WorkflowState::Approved, Some(agent(1)));
    let one = vec![step(1, "navigate", "read")];
    assert_eq!(put_steps(&mut store, 1, one.clone()), Ok(stored(1)));
    assert_eq!(
        set_state(&mut store, 1, "active"),
        Err(declared("playbook"))
    );
    assert_eq!(state_of(&store, 1), approved);
    let mut failed = run_record(1, Some("compile"), "failed");
    failed["playbook"] = json!(1);
    assert_eq!(finish(&mut store, failed), Ok(stored(1)));
    assert_eq!(
        set_state(&mut store, 1, "active"),
        Err(declared("playbook"))
    );
    assert_eq!(state_of(&store, 1), approved);
    start_at(&mut store, NOW, 1, 1, 2).expect("start_run of the second compile");
    let body =
        json!({"op": "put_playbook", "agent": agent(1), "run": run(2), "number": 2, "steps": one});
    assert_eq!(runner(&mut store, body), Ok(stored(1)));
    assert_eq!(end_run(&mut store, 2), Ok(accepted()));
    assert_eq!(
        set_state(&mut store, 1, "active"),
        Err(declared("playbook"))
    );
    assert_eq!(state_of(&store, 1), approved);
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

const ME: &str = "me@mail.test";

fn slot(name: &str, value: &str) -> Value {
    json!({"name": name, "value": [value]})
}

fn reply_slots(account: Option<&str>, senders: Option<&str>) -> Value {
    let mut slots = Vec::new();
    if let Some(account) = account {
        slots.push(slot("account_address", account));
    }
    if let Some(senders) = senders {
        slots.push(slot("reply_senders", senders));
    }
    Value::Array(slots)
}

fn put_bindings(store: &mut MemoryStore, w: u32, a: u32, rev: u32, slots: Value) {
    let row = json!({
        "v": 1, "workflow": wf(w), "agent": agent(a), "rev": rev, "origins": [], "slots": slots,
    });
    store
        .commit(vec![TypedRecord {
            kind: "agent_bindings",
            json: row.to_string(),
        }])
        .expect("the bindings row is stored");
}

fn other_agent(store: &mut MemoryStore, n: u32, state: &str, slots: Value) {
    store.put_workflow(workflow(&wf(n), "email", state, Some(&agent(n))));
    put_bindings(store, n, n, 1, slots);
}

fn gone(store: &mut MemoryStore, n: u32) {
    store.remove_workflow(&WorkflowId::parse(&wf(n)).expect("an id"));
}

fn move_to(store: &mut MemoryStore, n: u32, state: &str) -> Result<CoordinatorReply, ErrorName> {
    coordinator(
        store,
        json!({"op": "set_state", "workflow": wf(n), "agent": agent(n), "state": state}),
    )
}

fn set_bindings(
    store: &mut MemoryStore,
    n: u32,
    slots: Value,
) -> Result<CoordinatorReply, ErrorName> {
    coordinator(
        store,
        json!({"op": "set_bindings", "workflow": wf(n), "agent": agent(n), "slots": slots}),
    )
}

fn list_overlaps(
    store: &mut MemoryStore,
    n: u32,
    slots: Value,
) -> Result<CoordinatorReply, ErrorName> {
    coordinator(
        store,
        json!({"op": "list_overlaps", "workflow": wf(n), "agent": agent(n), "slots": slots}),
    )
}

fn overlaps(entries: &[(u32, u32, &[&str])], more: &str) -> CoordinatorReply {
    let entries: Vec<Value> = entries
        .iter()
        .map(|(w, a, senders)| json!({"workflow": wf(*w), "agent": agent(*a), "senders": senders}))
        .collect();
    let reply = json!({"overlaps": entries, "more": more});
    CoordinatorReply::parse(reply.to_string().as_bytes()).expect("an overlaps reply")
}

fn every_row(store: &MemoryStore) -> Vec<Vec<String>> {
    AGENT_KINDS
        .iter()
        .map(|(kind, _)| rows(store, kind))
        .collect()
}

#[test]
fn list_overlaps_stores_nothing_and_changes_no_state() {
    let mut store = MemoryStore::new();
    for n in [1, 2] {
        store.put_workflow(workflow(&wf(n), "email", "approved", None));
        assert_eq!(create(&mut store, n, n), Ok(rev(1)), "{n}");
    }
    let slots = reply_slots(Some(ME), Some("ann@people.test"));
    assert_eq!(set_bindings(&mut store, 1, slots.clone()), Ok(rev(1)));
    assert_eq!(move_to(&mut store, 1, "paused"), Ok(state_reply("paused")));
    assert_eq!(set_bindings(&mut store, 2, slots), Ok(rev(1)));
    let held = |store: &MemoryStore| (every_row(store), state_of(store, 1), state_of(store, 2));
    let before = held(&store);
    for (slots, sender) in [
        (json!(null), "ann@people.test"),
        (json!([]), "ann@people.test"),
        (
            json!([{"name": "reply_senders", "value": ["ANN@people.test"]}]),
            "ANN@people.test",
        ),
    ] {
        assert_eq!(
            list_overlaps(&mut store, 1, slots),
            Ok(overlaps(&[(2, 2, &[sender])], "no"))
        );
    }
    for slots in [json!(null), reply_slots(Some(ME), Some("ann@people.test"))] {
        for (w, a, refusal) in [(9, 1, ErrorName::NoWorkflow), (1, 2, declared("binding"))] {
            let body = json!({"op": "list_overlaps", "workflow": wf(w), "agent": agent(a), "slots": slots});
            assert_eq!(
                coordinator(&mut store, body),
                Err(refusal),
                "workflow {w}, agent {a}, slots {slots}"
            );
        }
    }
    assert_eq!(held(&store), before);
}

#[test]
fn list_overlaps_names_each_other_live_agent_with_the_senders_that_it_shares_in_the_spelling_of_the_asking_list(
) {
    let mut store = MemoryStore::new();
    let asking = "Ann@People.test, bob@people.test, dora@people.test";
    other_agent(&mut store, 1, "paused", reply_slots(Some(ME), Some(asking)));
    let zed = Some("zed@other.test, ann@people.test");
    other_agent(
        &mut store,
        2,
        "active",
        reply_slots(Some("ME@mail.test"), zed),
    );
    let both = Some("BOB@PEOPLE.TEST, ANN@people.test");
    other_agent(&mut store, 3, "approved", reply_slots(Some(ME), both));
    other_agent(
        &mut store,
        4,
        "paused",
        reply_slots(Some(ME), Some("dora@people.test")),
    );
    other_agent(
        &mut store,
        5,
        "active",
        reply_slots(Some(ME), Some("dora@people.test")),
    );
    gone(&mut store, 5);
    store.put_workflow(workflow(&wf(6), "email", "active", Some(&agent(6))));
    put_bindings(
        &mut store,
        6,
        6,
        1,
        reply_slots(Some(ME), Some("dora@people.test")),
    );
    store.put_workflow(workflow(&wf(6), "email", "active", Some(&agent(7))));
    put_bindings(
        &mut store,
        6,
        7,
        1,
        reply_slots(Some(ME), Some("Dora@people.test")),
    );
    let other = Some("other@mail.test");
    other_agent(
        &mut store,
        8,
        "active",
        reply_slots(other, Some("ann@people.test")),
    );
    let refused = Some("Ann Berg, ann@people.test");
    other_agent(&mut store, 9, "active", reply_slots(Some(ME), refused));
    other_agent(
        &mut store,
        10,
        "active",
        reply_slots(None, Some("ann@people.test")),
    );
    let older = reply_slots(Some(ME), Some("dora@people.test"));
    other_agent(&mut store, 11, "active", older);
    let newer = reply_slots(Some(ME), Some("erik@people.test"));
    put_bindings(&mut store, 11, 11, 2, newer);
    store.put_workflow(workflow(&wf(12), "email", "active", Some(&agent(12))));
    let newer = reply_slots(Some(ME), Some("bob@people.test"));
    put_bindings(&mut store, 12, 12, 10, newer);
    let older = reply_slots(Some(ME), Some("erik@people.test"));
    put_bindings(&mut store, 12, 12, 9, older);
    other_agent(
        &mut store,
        13,
        "active",
        reply_slots(None, Some("ANN@people.test")),
    );
    let held = |store: &MemoryStore| (every_row(store), state_of(store, 1), state_of(store, 6));
    let before = held(&store);

    let named = overlaps(
        &[
            (2, 2, &["Ann@People.test"]),
            (3, 3, &["Ann@People.test", "bob@people.test"]),
            (6, 7, &["dora@people.test"]),
            (12, 12, &["bob@people.test"]),
        ],
        "no",
    );
    assert_eq!(list_overlaps(&mut store, 1, json!(null)), Ok(named.clone()));
    assert_eq!(list_overlaps(&mut store, 1, json!([])), Ok(named));
    let shouted = json!([slot("reply_senders", "ANN@PEOPLE.TEST, carl@people.test")]);
    assert_eq!(
        list_overlaps(&mut store, 1, shouted),
        Ok(overlaps(
            &[(2, 2, &["ANN@PEOPLE.TEST"]), (3, 3, &["ANN@PEOPLE.TEST"])],
            "no"
        ))
    );
    let unread = json!([slot("reply_senders", "Ann Berg, ann@people.test")]);
    assert_eq!(
        list_overlaps(&mut store, 1, unread),
        Ok(overlaps(&[], "no"))
    );
    let account = json!([slot("account_address", "Other@Mail.test")]);
    assert_eq!(
        list_overlaps(&mut store, 1, account),
        Ok(overlaps(&[(8, 8, &["Ann@People.test"])], "no"))
    );
    for n in [9, 10] {
        assert_eq!(
            list_overlaps(&mut store, n, json!(null)),
            Ok(overlaps(&[], "no")),
            "{n}"
        );
    }
    assert_eq!(
        list_overlaps(&mut store, 8, json!(null)),
        Ok(overlaps(&[], "no"))
    );
    assert_eq!(
        list_overlaps(&mut store, 3, json!(null)),
        Ok(overlaps(
            &[(2, 2, &["ANN@people.test"]), (12, 12, &["BOB@PEOPLE.TEST"])],
            "no"
        ))
    );
    assert_eq!(held(&store), before);

    let mut store = ten_revisions();
    let ann = overlaps(&[(2, 2, &["ann@people.test"])], "no");
    assert_eq!(list_overlaps(&mut store, 1, json!(null)), Ok(ann.clone()));
    let unstored = reply_slots(Some(ME), Some("ann@people.test"));
    assert_eq!(list_overlaps(&mut store, 3, unstored), Ok(ann));

    let mut store = older_rows_beside_live_agents();
    assert_eq!(
        list_overlaps(&mut store, 1, json!(null)),
        Ok(overlaps(
            &[(2, 2, &["ann@people.test"]), (3, 7, &["dora@people.test"])],
            "no"
        ))
    );

    let mut store = MemoryStore::new();
    let zed = reply_slots(Some(ME), Some("zed@other.test"));
    other_agent(&mut store, 1, "paused", zed);
    let three = Some("ann@people.test, bob@people.test, carl@people.test");
    other_agent(&mut store, 2, "active", reply_slots(Some(ME), three));
    let asking = "carl@people.test, Bob@People.test, ann@people.test";
    assert_eq!(
        list_overlaps(&mut store, 1, json!([slot("reply_senders", asking)])),
        Ok(overlaps(
            &[(
                2,
                2,
                &["carl@people.test", "Bob@People.test", "ann@people.test"]
            )],
            "no"
        ))
    );
}

#[test]
fn list_overlaps_names_the_first_16_by_workflow_id_and_more_yes_where_17_overlap() {
    let mut store = MemoryStore::new();
    store.put_workflow(workflow(&wf(1), "email", "approved", None));
    assert_eq!(create(&mut store, 1, 1), Ok(rev(1)));
    put_bindings(
        &mut store,
        1,
        1,
        1,
        reply_slots(Some(ME), Some("ann@people.test")),
    );
    for n in (2..=18).rev() {
        other_agent(
            &mut store,
            n,
            "active",
            reply_slots(Some(ME), Some("ANN@people.test")),
        );
    }
    const ANN: &[&str] = &["ann@people.test"];
    let first = |from: u32, to: u32| (from..=to).map(|n| (n, n, ANN)).collect::<Vec<_>>();
    assert_eq!(
        list_overlaps(&mut store, 1, json!(null)),
        Ok(overlaps(&first(2, 17), "yes"))
    );
    assert_eq!(move_to(&mut store, 2, "paused"), Ok(state_reply("paused")));
    assert_eq!(
        list_overlaps(&mut store, 1, json!(null)),
        Ok(overlaps(&first(3, 18), "no"))
    );

    let crossed = |pairs: &[(u32, u32)]| {
        let mut store = MemoryStore::new();
        store.put_workflow(workflow(&wf(1), "email", "approved", None));
        assert_eq!(create(&mut store, 1, 1), Ok(rev(1)));
        put_bindings(
            &mut store,
            1,
            1,
            1,
            reply_slots(Some(ME), Some("ann@people.test")),
        );
        for &(w, a) in pairs {
            store.put_workflow(workflow(&wf(w), "email", "approved", None));
            assert_eq!(create(&mut store, w, a), Ok(rev(1)), "{w} {a}");
            let ann = reply_slots(Some(ME), Some("ANN@people.test"));
            put_bindings(&mut store, w, a, 1, ann);
        }
        store
    };
    let mut store = crossed(&[(2, 9), (3, 4)]);
    assert_eq!(
        list_overlaps(&mut store, 1, json!(null)),
        Ok(overlaps(&[(2, 9, ANN), (3, 4, ANN)], "no"))
    );
    let pairs: Vec<(u32, u32)> = (2..=18).map(|w| (w, 100 - w)).collect();
    let mut store = crossed(&pairs);
    let kept: Vec<(u32, u32, &[&str])> = (2..=17).map(|w| (w, 100 - w, ANN)).collect();
    assert_eq!(
        list_overlaps(&mut store, 1, json!(null)),
        Ok(overlaps(&kept, "yes"))
    );
}

fn created(agents: &[u32]) -> MemoryStore {
    let mut store = MemoryStore::new();
    for &n in agents {
        store.put_workflow(workflow(&wf(n), "email", "approved", None));
        assert_eq!(create(&mut store, n, n), Ok(rev(1)), "{n}");
    }
    store
}

fn ten_revisions() -> MemoryStore {
    let mut store = created(&[1, 2, 3]);
    let ann = reply_slots(Some(ME), Some("ann@people.test"));
    assert_eq!(set_bindings(&mut store, 2, ann), Ok(rev(1)));
    assert_eq!(move_to(&mut store, 2, "paused"), Ok(state_reply("paused")));
    assert_eq!(move_to(&mut store, 1, "paused"), Ok(state_reply("paused")));
    for n in 1..=10 {
        let slots = match n {
            1 => reply_slots(Some(ME), None),
            10 => reply_slots(None, Some("ann@people.test")),
            _ => reply_slots(None, Some("carl@people.test")),
        };
        assert_eq!(set_bindings(&mut store, 1, slots), Ok(rev(n)), "{n}");
    }
    assert_eq!(move_to(&mut store, 2, "active"), Ok(state_reply("active")));
    store.put_workflow(workflow(&wf(8), "email", "approved", None));
    let elsewhere = reply_slots(Some(ME), Some("zed@other.test"));
    put_bindings(&mut store, 8, 1, 20, elsewhere);
    store
}

fn older_rows_beside_live_agents() -> MemoryStore {
    let mut store = MemoryStore::new();
    store.put_workflow(workflow(&wf(1), "email", "approved", Some(&agent(1))));
    let both = reply_slots(Some(ME), Some("ann@people.test, dora@people.test"));
    put_bindings(&mut store, 1, 1, 1, both);
    store.put_workflow(workflow(&wf(3), "email", "active", Some(&agent(7))));
    put_bindings(
        &mut store,
        3,
        3,
        3,
        reply_slots(Some(ME), Some("dora@people.test")),
    );
    put_bindings(
        &mut store,
        3,
        7,
        1,
        reply_slots(Some(ME), Some("Dora@people.test")),
    );
    store.put_workflow(workflow(&wf(9), "email", "active", Some(&agent(2))));
    for rev in 1..=5 {
        put_bindings(
            &mut store,
            9,
            2,
            rev,
            reply_slots(Some(ME), Some("erik@people.test")),
        );
    }
    gone(&mut store, 9);
    other_agent(
        &mut store,
        2,
        "active",
        reply_slots(Some(ME), Some("ANN@people.test")),
    );
    store
}

#[test]
fn set_bindings_that_would_overlap_a_live_agent_is_refused_with_overlap_and_stores_nothing() {
    let mut store = created(&[1, 2, 3]);
    let first = reply_slots(Some(ME), Some("ann@people.test, bob@people.test"));
    assert_eq!(set_bindings(&mut store, 1, first), Ok(rev(1)));
    let refused = |store: &mut MemoryStore, n: u32, slots: Value| {
        let before = every_row(store);
        assert_eq!(
            set_bindings(store, n, slots.clone()),
            Err(declared("overlap")),
            "{n}: {slots}"
        );
        assert_eq!(every_row(store), before, "{n}: {slots}");
    };
    refused(
        &mut store,
        2,
        reply_slots(Some(ME), Some("ann@people.test")),
    );
    let other_case = Some("carl@people.test, bob@people.test");
    refused(&mut store, 2, reply_slots(Some("ME@Mail.TEST"), other_case));
    refused(
        &mut store,
        2,
        reply_slots(Some(ME), Some("ANN@People.Test")),
    );

    let list_alone = reply_slots(None, Some("ann@people.test"));
    assert_eq!(set_bindings(&mut store, 2, list_alone), Ok(rev(1)));
    refused(&mut store, 2, reply_slots(Some(ME), None));
    let address_alone = reply_slots(Some(ME), None);
    assert_eq!(set_bindings(&mut store, 3, address_alone), Ok(rev(1)));
    let completes = Some("carl@people.test, Bob@People.test");
    refused(&mut store, 3, reply_slots(None, completes));

    assert_eq!(move_to(&mut store, 3, "paused"), Ok(state_reply("paused")));
    refused(&mut store, 3, reply_slots(None, Some("bob@people.test")));
    assert_eq!(move_to(&mut store, 1, "active"), Ok(state_reply("active")));
    refused(&mut store, 2, reply_slots(Some(ME), None));
    assert_eq!(bindings(&store).len(), 3);
    let with_book = json!([
        slot("account_address", ME),
        slot("reply_senders", "ann@people.test"),
        slot("workbook", "Book.xlsx"),
    ]);
    let before = every_row(&store);
    assert_eq!(
        set_bindings(&mut store, 2, with_book),
        Err(declared("binding"))
    );
    assert_eq!(every_row(&store), before);

    let mut store = ten_revisions();
    refused(&mut store, 1, json!([slot("signature", "Ann")]));
    let carl = reply_slots(None, Some("carl@people.test"));
    assert_eq!(set_bindings(&mut store, 1, carl), Ok(rev(11)));

    let mut store = older_rows_beside_live_agents();
    for sender in ["ann@people.test", "dora@people.test"] {
        refused(&mut store, 1, reply_slots(None, Some(sender)));
    }
}

#[test]
fn set_bindings_passes_where_no_live_agent_overlaps() {
    let first = reply_slots(Some(ME), Some("ann@people.test, bob@people.test"));
    let passes = |store: &mut MemoryStore, slots: Value| {
        let stored = bindings(store).len();
        assert_eq!(set_bindings(store, 2, slots.clone()), Ok(rev(1)), "{slots}");
        assert_eq!(bindings(store).len(), stored + 1, "{slots}");
    };
    for slots in [
        reply_slots(Some(ME), Some("carl@people.test")),
        reply_slots(Some("you@mail.test"), Some("ann@people.test")),
        reply_slots(None, Some("ann@people.test")),
        reply_slots(Some(ME), None),
        reply_slots(Some(ME), Some("ann@people.test, Ann Berg")),
        reply_slots(Some(ME), Some("ann@people.test,")),
        reply_slots(Some(ME), Some("ann@people.test, ANN@people.test")),
    ] {
        let mut store = created(&[1, 2]);
        assert_eq!(set_bindings(&mut store, 1, first.clone()), Ok(rev(1)));
        passes(&mut store, slots);
    }
    let shared = reply_slots(Some(ME), Some("ann@people.test"));
    for unread in [
        "Ann Berg, ann@people.test",
        "ann@people.test, ANN@people.test",
    ] {
        let mut store = created(&[1, 2]);
        let slots = reply_slots(Some(ME), Some(unread));
        assert_eq!(set_bindings(&mut store, 1, slots), Ok(rev(1)));
        passes(&mut store, shared.clone());
    }
    let mut store = created(&[1, 2]);
    let no_account = reply_slots(None, Some("ann@people.test"));
    assert_eq!(set_bindings(&mut store, 1, no_account), Ok(rev(1)));
    passes(&mut store, reply_slots(None, Some("ANN@people.test")));
    for why in [
        "paused",
        "not stored",
        "of another agent",
        "proposed",
        "dismissed",
    ] {
        let mut store = created(&[1, 2]);
        assert_eq!(
            set_bindings(&mut store, 1, first.clone()),
            Ok(rev(1)),
            "{why}"
        );
        match why {
            "paused" => {
                assert_eq!(move_to(&mut store, 1, "paused"), Ok(state_reply("paused")));
            }
            "not stored" => gone(&mut store, 1),
            "of another agent" => {
                store.put_workflow(workflow(&wf(1), "email", "active", Some(&agent(9))));
            }
            "proposed" => {
                store.put_workflow(workflow(&wf(1), "email", "proposed", Some(&agent(1))));
            }
            _ => store.put_workflow(dismissed(&wf(1))),
        }
        assert_eq!(rows(&store, "agent_bindings").len(), 1, "{why}");
        passes(&mut store, shared.clone());
    }

    let mut store = created(&[1]);
    other_agent(&mut store, 5, "active", shared.clone());
    gone(&mut store, 5);
    store.put_workflow(workflow(&wf(5), "email", "approved", None));
    assert_eq!(rows(&store, "agent_bindings").len(), 1);
    assert_eq!(set_bindings(&mut store, 1, shared.clone()), Ok(rev(1)));
    assert_eq!(
        list_overlaps(&mut store, 1, json!(null)),
        Ok(overlaps(&[], "no"))
    );
    assert_eq!(move_to(&mut store, 1, "active"), Ok(state_reply("active")));

    let mut store = created(&[1, 2]);
    assert_eq!(set_bindings(&mut store, 1, shared.clone()), Ok(rev(1)));
    assert_eq!(move_to(&mut store, 1, "paused"), Ok(state_reply("paused")));
    assert_eq!(set_bindings(&mut store, 2, shared.clone()), Ok(rev(1)));
    let cleared = json!([{"name": "reply_senders", "value": []}]);
    assert_eq!(set_bindings(&mut store, 1, cleared), Ok(rev(2)));
    assert_eq!(
        list_overlaps(&mut store, 1, json!(null)),
        Ok(overlaps(&[], "no"))
    );
    assert_eq!(move_to(&mut store, 1, "active"), Ok(state_reply("active")));
}

#[test]
fn set_state_to_approved_or_active_of_an_agent_that_overlaps_a_live_agent_is_refused_and_changes_no_state(
) {
    let mut store = created(&[1, 2]);
    let shared = reply_slots(Some(ME), Some("ann@people.test"));
    assert_eq!(set_bindings(&mut store, 1, shared), Ok(rev(1)));
    assert_eq!(move_to(&mut store, 1, "paused"), Ok(state_reply("paused")));
    let other = reply_slots(Some(ME), Some("carl@people.test, ANN@people.test"));
    assert_eq!(set_bindings(&mut store, 2, other), Ok(rev(1)));
    for target in ["active", "approved"] {
        assert_eq!(
            move_to(&mut store, 1, target),
            Err(declared("overlap")),
            "{target}"
        );
        assert_eq!(
            state_of(&store, 1),
            (WorkflowState::Paused, Some(agent(1))),
            "{target}"
        );
    }
    store.put_workflow(workflow(&wf(1), "email", "active", Some(&agent(1))));
    for target in ["active", "approved"] {
        assert_eq!(
            move_to(&mut store, 2, target),
            Err(declared("overlap")),
            "{target}"
        );
        assert_eq!(
            state_of(&store, 2),
            (WorkflowState::Approved, Some(agent(2))),
            "{target}"
        );
    }
    assert_eq!(move_to(&mut store, 2, "paused"), Ok(state_reply("paused")));
    assert_eq!(state_of(&store, 2), (WorkflowState::Paused, Some(agent(2))));
    assert_eq!(move_to(&mut store, 1, "paused"), Ok(state_reply("paused")));
    for target in ["approved", "active"] {
        assert_eq!(
            move_to(&mut store, 1, target),
            Ok(state_reply(target)),
            "{target}"
        );
    }
    assert_eq!(move_to(&mut store, 2, "active"), Err(declared("overlap")));
    assert_eq!(state_of(&store, 2), (WorkflowState::Paused, Some(agent(2))));

    let mut store = ten_revisions();
    for target in ["active", "approved"] {
        assert_eq!(
            move_to(&mut store, 1, target),
            Err(declared("overlap")),
            "{target}"
        );
        assert_eq!(
            state_of(&store, 1),
            (WorkflowState::Paused, Some(agent(1))),
            "{target}"
        );
    }

    let mut store = MemoryStore::new();
    store.put_workflow(workflow(&wf(1), "utility_bill_payment", "approved", None));
    assert_eq!(create(&mut store, 1, 1), Ok(rev(1)));
    store.put_workflow(workflow(&wf(2), "email", "approved", None));
    assert_eq!(create(&mut store, 2, 2), Ok(rev(1)));
    let shared = reply_slots(Some(ME), Some("ann@people.test"));
    assert_eq!(set_bindings(&mut store, 1, shared.clone()), Ok(rev(1)));
    assert_eq!(move_to(&mut store, 1, "paused"), Ok(state_reply("paused")));
    assert_eq!(set_bindings(&mut store, 2, shared), Ok(rev(1)));
    for (target, refusal) in [("active", "playbook"), ("approved", "overlap")] {
        assert_eq!(
            move_to(&mut store, 1, target),
            Err(declared(refusal)),
            "{target}"
        );
        assert_eq!(
            state_of(&store, 1),
            (WorkflowState::Paused, Some(agent(1))),
            "{target}"
        );
    }
}

#[test]
fn the_state_call_of_create_agent_goes_through_the_function_of_the_overlap() {
    let mut store = MemoryStore::new();
    other_agent(
        &mut store,
        1,
        "active",
        reply_slots(Some(ME), Some("ann@people.test")),
    );
    store.put_workflow(workflow(&wf(2), "email", "approved", None));
    put_bindings(
        &mut store,
        2,
        2,
        1,
        reply_slots(Some(ME), Some("Ann@People.test")),
    );
    assert_eq!(create(&mut store, 2, 2), Err(declared("overlap")));
    assert_eq!(state_of(&store, 2), (WorkflowState::Approved, None));
    assert_eq!(configs(&store).len(), 1);
    assert_eq!(move_to(&mut store, 1, "paused"), Ok(state_reply("paused")));
    assert_eq!(create(&mut store, 2, 2), Ok(rev(1)));
    assert_eq!(
        state_of(&store, 2),
        (WorkflowState::Approved, Some(agent(2)))
    );
    assert_eq!(configs(&store).len(), 1);
}

fn named(store: &mut MemoryStore, slots: Value, why: &str) -> usize {
    match list_overlaps(store, 1, slots) {
        Ok(CoordinatorReply::Overlaps(reply)) => reply.overlaps.len(),
        other => panic!("{why}: {other:?}"),
    }
}

#[test]
fn the_read_and_the_keeper_agree_on_drawn_pairs_of_slot_sets() {
    let accounts = [
        Some(ME),
        Some(ME),
        Some("ME@Mail.Test"),
        Some("you@mail.test"),
        None,
    ];
    let lists = [
        Some("ann@people.test"),
        Some("ann@people.test"),
        Some("ANN@people.test, bob@people.test"),
        Some("dora@people.test, Bob@People.Test"),
        Some("carl@people.test"),
        Some("Ann Berg, ann@people.test"),
        Some("bob@people.test, BOB@people.test"),
        None,
    ];
    let states = [
        "approved",
        "active",
        "approved",
        "active",
        "paused",
        "not stored",
    ];
    let mut seed: u64 = 0x0035_b220_2611_2001;
    let mut draw = move |n: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % n as u64) as usize
    };
    let (mut kept_out, mut let_in, mut held_off, mut switched_on) = (0, 0, 0, 0);
    for case in 0..1000 {
        let mut slots = || reply_slots(accounts[draw(5)], lists[draw(8)]);
        let (other, stored, body) = (slots(), slots(), slots());
        let state = states[draw(6)];
        let why = format!("case {case}: stored {stored}, body {body}, other {other} {state}");
        let mut store = created(&[1]);
        put_bindings(&mut store, 1, 1, 1, stored);
        let live = if state == "not stored" {
            "active"
        } else {
            state
        };
        other_agent(&mut store, 2, live, other);
        if state == "not stored" {
            gone(&mut store, 2);
        }

        let read = named(&mut store, json!(null), &why);
        let switched = move_to(&mut store, 1, "active");
        if read == 0 {
            assert_eq!(switched, Ok(state_reply("active")), "{why}");
            switched_on += 1;
        } else {
            assert_eq!(switched, Err(declared("overlap")), "{why}");
            assert_eq!(state_of(&store, 1).0, WorkflowState::Approved, "{why}");
            held_off += 1;
        }

        let read = named(&mut store, body.clone(), &why);
        let before = every_row(&store);
        let kept = set_bindings(&mut store, 1, body);
        if read == 0 {
            assert_eq!(kept, Ok(rev(2)), "{why}");
            let_in += 1;
        } else {
            assert_eq!(kept, Err(declared("overlap")), "{why}");
            assert_eq!(every_row(&store), before, "{why}");
            kept_out += 1;
        }
    }
    assert!(
        [kept_out, let_in, held_off, switched_on]
            .iter()
            .all(|&n| n >= 30),
        "{kept_out} kept out, {let_in} let in, {held_off} held off, {switched_on} switched on"
    );
}

/// The reply of `list_agents` after the agent `after`, or from the first, as its JSON.
fn roster_after(store: &mut MemoryStore, after: Option<u32>) -> Value {
    let body =
        json!({"op": "list_agents", "workflow": null, "agent": null, "after": after.map(agent)});
    let reply = coordinator(store, body).expect("the reply of list_agents");
    serde_json::from_str(&reply.to_json().expect("the reply's JSON")).expect("the reply's text")
}

fn roster(store: &mut MemoryStore) -> Value {
    roster_after(store, None)
}

/// `put_playbook` of one step of playbook `number` in the run `r` of the first agent.
fn put_step_of(store: &mut MemoryStore, r: u32, number: u32) -> Result<RunnerReply, ErrorName> {
    let steps = vec![step(1, "navigate", "read")];
    runner(
        store,
        json!({"op": "put_playbook", "agent": agent(1), "run": run(r), "number": number, "steps": steps}),
    )
}

#[test]
fn the_roster_shows_a_playbook_only_after_its_compile_finished() {
    let mut store = store_with("utility_bill_payment");
    let playbook =
        |store: &mut MemoryStore| roster(store)["agents"][0]["config"]["playbook"].clone();
    let stored_configs = configs(&store);
    assert_eq!(playbook(&mut store), Value::Null);
    assert_eq!(put_step_of(&mut store, 1, 1), Ok(stored(1)));
    assert_eq!(
        playbook(&mut store),
        Value::Null,
        "a stored step is no finished playbook"
    );
    assert_eq!(finish(&mut store, compiled(1, 1)), Ok(stored(1)));
    assert_eq!(
        playbook(&mut store),
        json!(1),
        "the compile finished with completed"
    );

    start_at(&mut store, NOW, 1, 1, 2).expect("start_run of the second compile");
    assert_eq!(put_step_of(&mut store, 2, 2), Ok(stored(1)));
    assert_eq!(playbook(&mut store), json!(1));
    let mut failed = run_record(2, Some("compile"), "failed");
    failed["playbook"] = json!(2);
    assert_eq!(finish(&mut store, failed), Ok(stored(1)));
    assert_eq!(
        playbook(&mut store),
        json!(1),
        "another outcome leaves the number before"
    );

    start_at(&mut store, NOW, 1, 1, 3).expect("start_run of the third compile");
    assert_eq!(put_step_of(&mut store, 3, 3), Ok(stored(1)));
    assert_eq!(end_run(&mut store, 3), Ok(accepted()));
    assert_eq!(
        playbook(&mut store),
        json!(1),
        "an end_run leaves the number before"
    );
    assert_eq!(
        configs(&store),
        stored_configs,
        "the read stores no configuration"
    );
}

/// The typed reply of `list_agents` after the agent id `after`, or from the first.
fn roster_page(store: &mut MemoryStore, after: Option<&str>) -> AgentsReply {
    let body = json!({"op": "list_agents", "workflow": null, "agent": null, "after": after});
    match coordinator(store, body) {
        Ok(CoordinatorReply::Agents(reply)) => reply,
        other => panic!("the reply of list_agents: {other:?}"),
    }
}

/// The configuration of the agent `a` for the workflow `w`, as `create_agent` stores it for an email workflow.
fn config_row(w: u32, a: u32) -> TypedRecord {
    let config = preset(
        Category::Email,
        WorkflowId::parse(&wf(w)).expect("an id"),
        AgentId::parse(&agent(a)).expect("an id"),
    )
    .expect("a preset");
    TypedRecord {
        kind: "agent_config",
        json: config.to_json().expect("the config's JSON"),
    }
}

#[test]
fn the_roster_holds_only_named_agents_in_order_and_pages_by_after() {
    let mut store = MemoryStore::new();
    let mut named = Vec::new();
    let mut records = Vec::new();
    for n in (1..=1200).rev() {
        records.push(config_row(n, n));
        match n % 100 {
            7 => store.put_workflow(workflow(&wf(n), "email", "active", Some(&agent(n + 5000)))),
            8 => store.put_workflow(dismissed(&wf(n))),
            _ => {
                let state = ["approved", "active", "paused"][(n % 3) as usize];
                store.put_workflow(workflow(&wf(n), "email", state, Some(&agent(n))));
                if n % 100 != 9 {
                    named.push(agent(n));
                }
            }
        }
    }
    store
        .commit(records)
        .expect("the configurations are stored");
    for n in (9..=1200).step_by(100) {
        gone(&mut store, n);
    }
    named.sort();

    let mut listed: Vec<String> = Vec::new();
    let mut pages: Vec<AgentsReply> = Vec::new();
    let mut after: Option<String> = None;
    loop {
        let page = roster_page(&mut store, after.as_deref());
        let text = page.to_json().expect("the page's text");
        assert!(text.len() <= CHECKPOINTS_REPLY_MAX, "{} bytes", text.len());
        assert!(!page.agents.is_empty(), "page {}", pages.len());
        listed.extend(
            page.agents
                .iter()
                .map(|a| a.config.agent.as_str().to_owned()),
        );
        after = page.next.as_ref().map(|next| next.as_str().to_owned());
        if let Some(next) = &after {
            assert_eq!(
                Some(next),
                listed.last(),
                "next names the page's last agent"
            );
        }
        pages.push(page);
        if after.is_none() {
            break;
        }
    }
    assert!(pages.len() >= 2, "{} pages", pages.len());
    for pair in pages.windows(2) {
        let text = pair[0].to_json().expect("the page's text");
        let entry = pair[1].agents[0].to_json().expect("the entry's text");
        assert!(
            text.len() + 1 + entry.len() > CHECKPOINTS_REPLY_MAX,
            "a page holds every whole entry that fits"
        );
    }
    assert_eq!(
        listed, named,
        "every named agent once, in ascending order, and no other"
    );

    let last = roster_page(&mut store, named.last().map(String::as_str));
    assert!(last.agents.is_empty() && last.next.is_none());
    let unlisted = agent(1109);
    let from_unlisted = roster_page(&mut store, Some(&unlisted));
    let above: Vec<&String> = named.iter().filter(|a| **a > unlisted).collect();
    let got: Vec<String> = from_unlisted
        .agents
        .iter()
        .map(|a| a.config.agent.as_str().to_owned())
        .collect();
    assert_eq!(got.iter().collect::<Vec<_>>(), above);
}

#[test]
fn an_entry_names_state_slots_session_and_the_last_run() {
    let mut store = store_with("utility_bill_payment");
    let entry = |store: &mut MemoryStore| roster(store)["agents"][0].clone();
    let first = entry(&mut store);
    assert_eq!(
        [
            &first["state"],
            &first["slots"],
            &first["session"],
            &first["last"]
        ],
        [&json!("approved"), &json!([]), &json!("no"), &Value::Null]
    );

    let paying = json!([
        slot("payee", "Northwind Power"),
        slot("instrument", "card 4242")
    ]);
    assert_eq!(set_bindings(&mut store, 1, paying), Ok(rev(1)));
    let due = json!([slot("payee", "Contoso Power"), slot("due_day", "12")]);
    assert_eq!(set_bindings(&mut store, 1, due), Ok(rev(2)));
    assert_eq!(
        entry(&mut store)["slots"],
        json!(["payee", "instrument", "due_day"]),
        "the slot names of the newest bindings"
    );

    let part = |part: u32, data: Vec<String>| json!({"op": "put_session", "agent": agent(1), "run": run(1), "origin": "http://utility.test:18090", "rev": 1, "part": part, "of": 2, "data": data});
    assert_eq!(
        runner(&mut store, part(1, vec!["A".repeat(1024); 48])),
        Ok(stored(1))
    );
    assert_eq!(entry(&mut store)["session"], json!("no"), "one part of two");
    assert_eq!(
        runner(&mut store, part(2, vec!["QUJD".to_owned()])),
        Ok(stored(1))
    );
    assert_eq!(entry(&mut store)["session"], json!("yes"), "stored whole");
    let other = json!({"op": "put_session", "agent": agent(1), "run": run(1), "origin": "http://other.test:18091", "rev": 1, "part": 1, "of": 1, "data": ["QUJD"]});
    assert_eq!(runner(&mut store, other), Ok(stored(1)));
    let dropped = json!({"op": "drop_session", "agent": agent(1), "run": run(1), "origin": "http://utility.test:18090"});
    assert_eq!(runner(&mut store, dropped), Ok(stored(1)));
    assert_eq!(
        entry(&mut store)["session"],
        json!("no"),
        "one origin dropped, another kept"
    );
    let again = json!({"op": "put_session", "agent": agent(1), "run": run(1), "origin": "http://utility.test:18090", "rev": 3, "part": 1, "of": 1, "data": ["QUJD"]});
    assert_eq!(runner(&mut store, again), Ok(stored(1)));
    assert_eq!(entry(&mut store)["session"], json!("yes"), "kept again");

    assert_eq!(
        set_state(&mut store, 1, "paused"),
        Ok(state_reply("paused"))
    );
    assert_eq!(entry(&mut store)["state"], json!("paused"));
    assert_eq!(
        set_state(&mut store, 1, "approved"),
        Ok(state_reply("approved"))
    );
    assert_eq!(entry(&mut store)["state"], json!("approved"));

    let record = run_record(1, Some("compile"), "failed");
    assert_eq!(finish(&mut store, record.clone()), Ok(stored(1)));
    assert_eq!(entry(&mut store)["last"], record);
    start_at(&mut store, NOW, 1, 1, 2).expect("start_run of the second run");
    assert_eq!(end_run(&mut store, 2), Ok(accepted()));
    let ended: Value = rows(&store, "agent_run")
        .iter()
        .map(|r| serde_json::from_str::<Value>(r).expect("a run row"))
        .find(|r| r["run"] == json!(run(2)))
        .expect("the record of end_run");
    assert_eq!(ended["day"], json!(TODAY));
    assert_eq!(entry(&mut store)["last"], ended, "the newest record");
}

#[test]
fn the_roster_names_each_effect_whose_newest_row_is_unknown() {
    let mut store = store_with("utility_bill_payment");
    let unknown = |store: &mut MemoryStore| roster(store)["agents"][0]["unknown"].clone();
    let begun = |store: &mut MemoryStore, r: u32, e: u32| {
        for m in [
            row(r, e, 1, "held", "payment", 1, None, None),
            row(r, e, 2, "approved", "payment", 1, Some(e), None),
            row(r, e, 3, "releasing", "payment", 1, Some(e), None),
        ] {
            assert_eq!(put(store, m), Ok(stored(1)));
        }
    };
    begun(&mut store, 1, 1);
    begun(&mut store, 1, 2);
    let committed = row(1, 2, 4, "committed", "payment", 1, Some(2), None);
    assert_eq!(put(&mut store, committed), Ok(stored(1)));
    assert_eq!(unknown(&mut store), json!([]), "releasing is not unknown");

    assert_eq!(end_run(&mut store, 1), Ok(accepted()));
    let records = rows(&store, "agent_run");
    assert_eq!(records.len(), 1);
    assert!(records[0].contains(r#""unknown":0"#), "{}", records[0]);
    let entry = |r: u32, e: u32| json!({"run": run(r), "effect": effect(e), "day": TODAY, "class": "payment", "kept": kept("payment")});
    assert_eq!(
        unknown(&mut store),
        json!([entry(1, 1)]),
        "after an end_run whose record counts 0"
    );

    start_at(&mut store, NOW, 1, 1, 2).expect("start_run of the second run");
    begun(&mut store, 2, 3);
    let mut record = run_record(2, Some("replay"), "effect_unknown");
    record["unknown"] = json!(1);
    assert_eq!(finish(&mut store, record), Ok(stored(1)));
    assert_eq!(
        unknown(&mut store),
        json!([entry(1, 1), entry(2, 3)]),
        "every effect of the agent, whatever the newest record counts"
    );
}

/// The runner's `log` of one `step_failed` line with the cause, step 1 of the run `r` of the agent `a` of the
/// workflow `w`.
fn step_failed(store: &mut MemoryStore, w: u32, a: u32, r: u32, cause: &str) {
    let line = json!({
        "v": 1, "workflow": wf(w), "agent": agent(a), "run": run(r), "seq": 2, "day": TODAY,
        "at": NOW.at.0, "event": "step_failed", "step": 1, "cause": cause, "model": null, "ms": null,
        "tokens_in": null, "tokens_out": null,
    });
    let body = json!({"op": "log", "agent": agent(a), "run": run(r), "records": [line]});
    assert_eq!(runner(store, body), Ok(stored(1)), "{cause}");
}

#[test]
fn the_roster_names_the_newest_started_run_that_a_site_refused() {
    let mut store = store_with("email");
    store.put_workflow(workflow(&wf(2), "email", "approved", None));
    assert_eq!(create(&mut store, 2, 2), Ok(rev(1)));
    start_at(&mut store, NOW, 2, 2, 2).expect("start_run of the second agent");
    step_failed(&mut store, 1, 1, 1, "timeout");
    step_failed(&mut store, 2, 2, 2, "unauthorized");
    let refused = |store: &mut MemoryStore| -> Vec<Value> {
        let agents = roster(store)["agents"].clone();
        let agents = agents.as_array().expect("the entries").clone();
        agents.iter().map(|a| a["refused_run"].clone()).collect()
    };
    assert!(rows(&store, "agent_run").is_empty(), "no run ended");
    assert_eq!(
        refused(&mut store),
        [Value::Null, json!(run(2))],
        "the second agent's newest started run, with no record; the first agent's line of another cause"
    );

    let later = at(NOW.at.0 + 1_000_000_000);
    start_at(&mut store, later, 2, 2, 3).expect("a newer start of the second agent");
    assert_eq!(
        refused(&mut store),
        [Value::Null, Value::Null],
        "a newer start ends the sign"
    );
}

/// The `stopped` line of the run `r` of the agent `a` of the workflow `w`, with the step, if any, and the cause.
fn stopped_line(w: u32, a: u32, r: u32, step: Option<u32>, cause: &str) -> Value {
    json!({
        "v": 1, "workflow": wf(w), "agent": agent(a), "run": run(r), "seq": 2, "day": TODAY,
        "at": NOW.at.0, "event": "stopped", "step": step, "cause": cause, "model": null, "ms": null,
        "tokens_in": null, "tokens_out": null,
    })
}

/// The runner's `log` of the one line in the run `r` of the agent `a`.
fn log_line(store: &mut MemoryStore, a: u32, r: u32, line: Value) {
    let body = json!({"op": "log", "agent": agent(a), "run": run(r), "records": [line]});
    assert_eq!(runner(store, body), Ok(stored(1)));
}

/// The runner's `finish` of the run `r` of the agent `a` of the workflow `w` with the outcome.
fn finish_of(store: &mut MemoryStore, w: u32, a: u32, r: u32, outcome: &str) {
    let mut record = run_record(r, Some("replay"), outcome);
    record["workflow"] = json!(wf(w));
    record["agent"] = json!(agent(a));
    assert_eq!(finish(store, record), Ok(stored(1)), "{outcome}");
}

/// The `last_cause` of each entry of the roster, none where an entry lacks the key.
fn causes(store: &mut MemoryStore) -> Vec<Option<Value>> {
    let agents = roster(store)["agents"].clone();
    let agents = agents.as_array().expect("the entries").clone();
    agents
        .iter()
        .map(|a| a.get("last_cause").cloned())
        .collect()
}

#[test]
fn the_roster_names_the_cause_of_the_newest_started_run() {
    let mut store = store_with("email");
    store.put_workflow(workflow(&wf(2), "email", "approved", None));
    assert_eq!(create(&mut store, 2, 2), Ok(rev(1)));
    start_at(&mut store, NOW, 2, 2, 2).expect("start_run of the second agent");
    log_line(&mut store, 2, 2, stopped_line(2, 2, 2, None, "no_browser"));
    assert_eq!(
        causes(&mut store),
        [Some(Value::Null), Some(Value::Null)],
        "the row of the end stands and the record does not: the run has not ended"
    );
    finish_of(&mut store, 2, 2, 2, "failed");
    assert_eq!(
        causes(&mut store),
        [Some(Value::Null), Some(json!("no_browser"))],
        "the first agent's run has not ended; the second agent's run ended with no_browser"
    );

    start_at(&mut store, at(NOW.at.0 + 1_000_000_000), 2, 2, 3).expect("a later start");
    finish_of(&mut store, 2, 2, 3, "completed");
    assert_eq!(
        causes(&mut store),
        [Some(Value::Null), Some(Value::Null)],
        "a later run that ended well"
    );

    start_at(&mut store, at(NOW.at.0 + 2_000_000_000), 2, 2, 4).expect("a later start");
    log_line(&mut store, 2, 4, stopped_line(2, 2, 4, Some(1), "no_start"));
    finish_of(&mut store, 2, 2, 4, "failed");
    assert_eq!(
        causes(&mut store),
        [Some(Value::Null), Some(json!("no_start"))],
        "the row of a list step that ended its run"
    );

    let outside = "no_such_cause";
    assert!(
        Cause::parse(outside).is_err(),
        "{outside} is a name of the list"
    );
    start_at(&mut store, at(NOW.at.0 + 3_000_000_000), 2, 2, 5).expect("the latest start");
    finish_of(&mut store, 2, 2, 5, "failed");
    store
        .commit(vec![TypedRecord {
            kind: "agent_log",
            json: stopped_line(2, 2, 5, None, outside).to_string(),
        }])
        .expect("the row is stored as the store holds it");
    assert!(rows(&store, "agent_log")
        .iter()
        .any(|r| r.contains(outside)));
    assert_eq!(
        causes(&mut store),
        [Some(Value::Null), Some(Value::Null)],
        "a stored cause that is no name of the list"
    );
}

#[test]
fn the_roster_names_no_cause_of_a_step_after_which_the_run_went_on() {
    let mut store = store_with("email");
    assert!(GOES_ON_AFTER.contains(&Cause::NotSelected));
    for cause in GOES_ON_AFTER {
        log_line(
            &mut store,
            1,
            1,
            stopped_line(1, 1, 1, Some(3), cause.as_str()),
        );
    }
    assert_eq!(
        causes(&mut store),
        [Some(Value::Null)],
        "the run goes on after the send step's row"
    );
    finish_of(&mut store, 1, 1, 1, "completed");
    assert_eq!(
        causes(&mut store),
        [Some(Value::Null)],
        "the run ended completed"
    );
}

/// verifier-100's case of r3 U5.2: the runner's `stopped` row of a state over the limit (no step, the cause
/// `session_size`, written by the run's end before its own end row) is no row of the run's end: a run that then ends
/// `completed` reads `last_cause` null, and a run that ends with a cause after that row reads its own cause.
#[test]
fn a_completed_run_whose_state_was_over_the_limit_reads_no_cause() {
    let mut store = store_with("email");
    log_line(
        &mut store,
        1,
        1,
        stopped_line(1, 1, 1, None, "session_size"),
    );
    finish_of(&mut store, 1, 1, 1, "completed");
    assert_eq!(
        causes(&mut store),
        [Some(Value::Null)],
        "the run ended completed"
    );

    let mut store = store_with("email");
    log_line(
        &mut store,
        1,
        1,
        stopped_line(1, 1, 1, None, "session_size"),
    );
    let mut end = stopped_line(1, 1, 1, None, "step_failed");
    end["seq"] = json!(3);
    log_line(&mut store, 1, 1, end);
    finish_of(&mut store, 1, 1, 1, "failed");
    assert_eq!(
        causes(&mut store),
        [Some(json!("step_failed"))],
        "the run's own end row"
    );
}

/// The runner's `reply_row` of the agent's first message, `drafted` and then `begun`, in the run `r`.
fn begun_reply(store: &mut MemoryStore, r: u32) {
    for (seq, effect_n, state) in [(1, None, "drafted"), (2, Some(1), "begun")] {
        let row = json!({
            "v": 1, "workflow": wf(1), "agent": agent(1), "run": run(r), "seq": seq, "day": ROW_DAY,
            "at": ROW_AT, "message": "m1", "draft": "d1", "effect": effect_n.map(effect), "state": state,
        });
        let body = json!({"op": "reply_row", "agent": agent(1), "run": run(r), "record": row});
        assert_eq!(runner(store, body), Ok(stored(1)), "{state}");
    }
}

#[test]
fn the_roster_says_reply_unknown_for_the_run_of_last_alone() {
    let mut store = store_with("email");
    let entry = |store: &mut MemoryStore| roster(store)["agents"][0].clone();
    let said = |entry: &Value| [entry["last"]["run"].clone(), entry["reply_unknown"].clone()];
    assert_eq!(
        said(&entry(&mut store)),
        [Value::Null, json!("no")],
        "last null"
    );

    begun_reply(&mut store, 1);
    assert_eq!(end_run(&mut store, 1), Ok(accepted()));
    assert!(
        rows(&store, "agent_reply")
            .iter()
            .any(|r| r.contains(r#""state":"unknown""#)),
        "the run's end left the begun row unknown"
    );
    assert_eq!(said(&entry(&mut store)), [json!(run(1)), json!("yes")]);

    let later = at(NOW.at.0 + 1_000_000_000);
    start_at(&mut store, later, 1, 1, 2).expect("start_run of the second run");
    assert_eq!(end_run(&mut store, 2), Ok(accepted()));
    assert_eq!(
        said(&entry(&mut store)),
        [json!(run(2)), json!("no")],
        "an older run holds the row, the run of last none"
    );
}

#[test]
fn the_roster_names_the_stored_slots_that_their_reader_refuses() {
    let mut store = store_with("email");
    let named = |store: &mut MemoryStore| {
        let entry = roster(store)["agents"][0].clone();
        [entry["slots"].clone(), entry["refused_slots"].clone()]
    };
    let refused_senders = json!([
        slot("account_address", ME),
        slot("reply_senders", "bob@people.test."),
        slot("payee", "Northwind Power"),
    ]);
    assert_eq!(set_bindings(&mut store, 1, refused_senders), Ok(rev(1)));
    assert_eq!(
        named(&mut store),
        [
            json!(["account_address", "reply_senders", "payee"]),
            json!(["reply_senders"])
        ],
        "a list that its reader refuses; slots with no reader are not named"
    );

    let taken = json!([slot("reply_senders", "ann@people.test, bob@people.test")]);
    assert_eq!(set_bindings(&mut store, 1, taken), Ok(rev(2)));
    assert_eq!(
        named(&mut store),
        [
            json!(["account_address", "reply_senders", "payee"]),
            json!([])
        ],
        "a list that its reader takes"
    );

    let relative = slot("workbook", "books/bills.xlsx");
    assert_eq!(
        set_bindings(&mut store, 1, json!([relative.clone()])),
        Err(declared("binding")),
        "set_bindings refuses the workbook, so its row is put as stored"
    );
    put_bindings(
        &mut store,
        1,
        1,
        3,
        json!([
            relative,
            slot("payee", "Northwind Power"),
            slot("reply_senders", "bob@people.test.")
        ]),
    );
    assert_eq!(
        named(&mut store),
        [
            json!(["workbook", "payee", "reply_senders"]),
            json!(["workbook", "reply_senders"])
        ],
        "of the newest bindings, in stored order"
    );
}

/// The reply of `agent_history` of the first agent for `runs` runs, as its JSON.
fn history(store: &mut MemoryStore, runs: u32) -> Value {
    let body = json!({"op": "agent_history", "workflow": wf(1), "agent": agent(1), "runs": runs});
    let reply = coordinator(store, body).expect("the reply of agent_history");
    serde_json::from_str(&reply.to_json().expect("the reply's JSON")).expect("the reply's text")
}

/// The stored run record of the run `r`.
fn record_of(store: &MemoryStore, r: u32) -> Value {
    rows(store, "agent_run")
        .iter()
        .map(|text| serde_json::from_str::<Value>(text).expect("a run row"))
        .find(|record| record["run"] == json!(run(r)))
        .expect("the run's record")
}

/// One effect of the history as the last row of the effect `e` holds it.
fn as_last_row(store: &MemoryStore, e: u32) -> Value {
    let text = last_of(store, &effect(e))
        .to_json()
        .expect("the row's JSON");
    let last: Value = serde_json::from_str(&text).expect("the row's text");
    json!({
        "effect": last["effect"], "digest": last["digest"], "class": last["class"],
        "state": last["state"], "cause": last["cause"], "kept": last["kept"],
    })
}

#[test]
fn the_history_gives_the_newest_runs_first_and_no_more_than_asked() {
    let mut store = unstarted("utility_bill_payment");
    for (k, r) in [3, 1, 2, 4].into_iter().enumerate() {
        let started = at(NOW.at.0 + 1_000_000_000 * i64::try_from(k).expect("a count"));
        start_at(&mut store, started, 1, 1, r).expect("start_run");
        assert_eq!(
            put(&mut store, row(r, r, 1, "held", "payment", r, None, None)),
            Ok(stored(1))
        );
    }
    for (seq, state) in [(2, "approved"), (3, "releasing"), (4, "committed")] {
        let next = row(2, 2, seq, state, "payment", 2, Some(2), None);
        assert_eq!(put(&mut store, next), Ok(stored(1)));
    }
    for r in [1, 2, 3] {
        assert_eq!(end_run(&mut store, r), Ok(accepted()));
    }
    let entry = |store: &MemoryStore, r: u32| json!({"run": record_of(store, r), "effects": [as_last_row(store, r)]});
    assert_eq!(
        history(&mut store, 2),
        json!({"runs": [entry(&store, 2), entry(&store, 1)]}),
        "the two runs started last, newest first, each effect as its last row holds it"
    );
    assert_eq!(
        history(&mut store, 5),
        json!({"runs": [entry(&store, 2), entry(&store, 1), entry(&store, 3)]}),
        "every run with a record, and the run 4, with none, not at all"
    );
    assert_eq!(history(&mut store, 0), json!({"runs": []}));
}

#[test]
fn an_effect_of_the_history_names_its_id_and_the_digest_of_its_last_row() {
    let mut store = store_with("utility_bill_payment");
    assert_eq!(
        put(&mut store, row(1, 1, 1, "held", "payment", 1, None, None)),
        Ok(stored(1))
    );
    assert_eq!(
        put(&mut store, row(1, 1, 2, "held", "payment", 2, None, None)),
        Ok(stored(1)),
        "a rebind to a second digest"
    );
    assert_eq!(
        put(&mut store, row(1, 2, 1, "held", "payment", 3, None, None)),
        Ok(stored(1))
    );
    assert_eq!(end_run(&mut store, 1), Ok(accepted()));
    let effects = history(&mut store, 1)["runs"][0]["effects"].clone();
    assert_eq!(effects.as_array().map(Vec::len), Some(2), "{effects}");
    for (n, (e, d)) in [(1, 2), (2, 3)].into_iter().enumerate() {
        let last = last_of(&store, &effect(e));
        assert_eq!(
            [&effects[n]["effect"], &effects[n]["digest"]],
            [&json!(effect(e)), &json!(digest(d))]
        );
        assert_eq!(effects[n]["digest"], json!(last.digest.as_str()));
    }
}

#[test]
fn a_rejected_effect_says_what_ended_it() {
    let mut store = unattended_sender();
    for (m, why) in [
        (row(1, 1, 1, "cleared", "send", 1, None, None), "cleared"),
        (
            row(1, 1, 2, "rejected", "send", 1, None, Some("reproposed")),
            "the runner proposes it again",
        ),
        (row(1, 2, 1, "held", "delete", 1, None, None), "held"),
        (
            row(1, 2, 2, "rejected", "delete", 1, None, None),
            "a reject_once",
        ),
        (row(1, 3, 1, "held", "delete", 1, None, None), "held"),
    ] {
        assert_eq!(put(&mut store, m), Ok(stored(1)), "{why}");
    }
    assert_eq!(end_run(&mut store, 1), Ok(accepted()));
    let effects = history(&mut store, 1)["runs"][0]["effects"].clone();
    let said: Vec<(Value, Value)> = effects
        .as_array()
        .expect("the run's effects")
        .iter()
        .map(|e| (e["state"].clone(), e["cause"].clone()))
        .collect();
    assert_eq!(
        said,
        [
            (json!("rejected"), json!("reproposed")),
            (json!("rejected"), Value::Null),
            (json!("rejected"), json!("ended")),
        ]
    );
}

#[test]
fn a_local_edit_names_its_checkpoint_made_and_then_gone() {
    const FOLDER: &str = "/data/books";
    let book = |n: u32| format!("{FOLDER}/book-{n}.xlsx");
    let copy_of = |r: u32, n: u32| format!("book-{n}.halvo-checkpoint-{}.xlsx", run(r));
    let checkpoint = |r: u32, n: u32, made: bool| {
        let mut row = json!({
            "v": 1, "workflow": wf(1), "agent": agent(1), "run": run(r), "seq": 1, "day": ROW_DAY,
            "at": ROW_AT, "folder": [FOLDER], "workbook": [book(n)], "copy": [copy_of(r, n)],
            "sha256": null, "bytes": null, "state": "planned", "left": null,
        });
        if made {
            row["seq"] = json!(2);
            row["state"] = json!("made");
            row["sha256"] = json!(digest(n));
            row["bytes"] = json!(4096 + n);
        }
        json!({"op": "checkpoint", "agent": agent(1), "run": run(r), "record": row})
    };
    let edit = |r: u32, e: u32, seq: u32, state: &str, n: u32| {
        let mut row = row(r, e, seq, state, "local_edit", 1, None, None);
        for field in row["kept"].as_array_mut().expect("the kept fields") {
            if field["name"] == "file" {
                field["value"] = json!([book(n)]);
            }
        }
        row
    };

    let mut store = store_with("spreadsheet_fill");
    assert_eq!(put(&mut store, edit(1, 1, 1, "cleared", 1)), Ok(stored(1)));
    assert_eq!(
        put(&mut store, row(1, 2, 1, "held", "payment", 1, None, None)),
        Ok(stored(1))
    );
    assert_eq!(runner(&mut store, checkpoint(1, 1, false)), Ok(stored(1)));
    assert_eq!(end_run(&mut store, 1), Ok(accepted()));

    start_at(&mut store, at(NOW.at.0 + 1_000_000_000), 1, 1, 2).expect("start_run of run 2");
    for (seq, state) in [(1, "cleared"), (2, "releasing"), (3, "committed")] {
        assert_eq!(put(&mut store, edit(2, 3, seq, state, 1)), Ok(stored(1)));
    }
    for (n, made) in [(1, false), (1, true), (2, false), (2, true)] {
        assert_eq!(runner(&mut store, checkpoint(2, n, made)), Ok(stored(1)));
    }
    assert_eq!(end_run(&mut store, 2), Ok(accepted()));

    let runs = history(&mut store, 2)["runs"].clone();
    assert_eq!(
        runs[0]["effects"][0]["checkpoint"],
        json!({"state": "made", "at": ROW_AT, "name": copy_of(2, 1)}),
        "the newest row of its run's checkpoint of its file"
    );
    let first = &runs[1]["effects"];
    assert_eq!(
        first[0].get("checkpoint"),
        Some(&Value::Null),
        "a newest row planned: no copy was made"
    );
    assert_eq!(first[1]["class"], json!("payment"));
    assert_eq!(
        first[1].get("checkpoint"),
        None,
        "another class carries none"
    );

    let later = at(NOW.at.0 + 5_000_000_000);
    let mark = json!({
        "op": "mark_checkpoint", "workflow": wf(1), "agent": agent(1), "run": run(2),
        "workbook": [book(1)], "state": "gone", "left": null,
    });
    let marked = coordinator_at(&mut store, later, mark).expect("mark_checkpoint");
    assert_eq!(
        marked.to_json().expect("the reply's JSON"),
        r#"{"stored":1}"#
    );
    assert_eq!(
        history(&mut store, 1)["runs"][0]["effects"][0]["checkpoint"],
        json!({"state": "gone", "at": later.at.0, "name": copy_of(2, 1)}),
        "gone, at the instant of mark_checkpoint"
    );
}
