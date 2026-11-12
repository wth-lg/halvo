mod common;

use common::*;
use halvo_agent_store::{Host, MemoryStore};
use halvo_schema::executable::names::{Cause, EffectState};
use halvo_schema::executable::AgentRun;
use halvo_schema::kinds::{check_record, registry};
use halvo_schema::{ErrorName, TypedRecord};
use serde_json::json;

fn runs(store: &MemoryStore) -> Vec<AgentRun> {
    rows(store, "agent_run")
        .iter()
        .map(|r| AgentRun::parse(r.as_bytes()).expect("a run row"))
        .collect()
}

fn payment_through_begin(store: &mut MemoryStore, r: u32, e: u32) {
    for m in [
        row(r, e, 1, "held", "payment", 1, None, None),
        row(r, e, 2, "approved", "payment", 1, Some(e), None),
        row(r, e, 3, "releasing", "payment", 1, Some(e), None),
    ] {
        assert_eq!(put(store, m), Ok(stored(1)));
    }
}

#[test]
fn finish_stores_the_record_and_makes_the_moves_of_the_end() {
    let mut store = store_with("utility_bill_payment");
    payment_through_begin(&mut store, 1, 1);
    assert_eq!(
        put(&mut store, row(1, 2, 1, "held", "payment", 1, None, None)),
        Ok(stored(1))
    );
    payment_through_begin(&mut store, 2, 3);
    let record = run_record(1, Some("replay"), "effect_unknown");
    assert_eq!(finish(&mut store, record.clone()), Ok(stored(1)));
    let stored_runs = runs(&store);
    assert_eq!(stored_runs.len(), 1);
    assert_eq!(
        stored_runs[0],
        AgentRun::parse(record.to_string().as_bytes()).expect("the body's record")
    );
    let lost = last_of(&store, &effect(1));
    assert_eq!(lost.state, EffectState::Unknown);
    assert_eq!(lost.seq.get(), 4);
    assert_eq!(lost.cause, None);
    assert_eq!(
        lost.approval.map(|a| a.as_str().to_owned()),
        Some(approval(1))
    );
    assert_eq!((lost.day, lost.at), (NOW.day, NOW.at));
    let ended = last_of(&store, &effect(2));
    assert_eq!(ended.state, EffectState::Rejected);
    assert_eq!(ended.cause, Some(Cause::Ended));
    assert_eq!(ended.seq.get(), 2);
    assert_eq!(last_of(&store, &effect(3)).state, EffectState::Releasing);
}

#[test]
fn finish_repeated_is_answered_as_the_first_and_end_run_after_it_changes_nothing() {
    let mut store = store_with("utility_bill_payment");
    payment_through_begin(&mut store, 1, 1);
    let record = run_record(1, Some("replay"), "effect_unknown");
    assert_eq!(finish(&mut store, record.clone()), Ok(stored(1)));
    let count = (effects(&store).len(), runs(&store).len());
    assert_eq!(count, (4, 1));
    assert_eq!(finish(&mut store, record), Ok(stored(1)));
    assert_eq!(end_run(&mut store, 1), Ok(accepted()));
    assert_eq!((effects(&store).len(), runs(&store).len()), count);
    assert_eq!(
        finish(&mut store, run_record(1, Some("replay"), "failed")),
        Err(declared("effect"))
    );
    assert_eq!((effects(&store).len(), runs(&store).len()), count);
}

#[test]
fn finish_after_end_run_is_refused() {
    let mut store = store_with("utility_bill_payment");
    payment_through_begin(&mut store, 1, 1);
    assert_eq!(end_run(&mut store, 1), Ok(accepted()));
    assert_eq!(
        finish(&mut store, run_record(1, Some("replay"), "effect_unknown")),
        Err(declared("effect"))
    );
    assert_eq!(runs(&store)[0].mode, None);
}

#[test]
fn end_run_of_a_run_with_no_effect_stores_its_record_alone() {
    let mut store = store_with("email");
    assert_eq!(end_run(&mut store, 1), Ok(accepted()));
    assert_eq!(runs(&store).len(), 1);
    assert!(effects(&store).is_empty());
}

fn as_agent_2(mut value: serde_json::Value) -> serde_json::Value {
    value["workflow"] = json!(wf(2));
    value["agent"] = json!(agent(2));
    value
}

fn second_agent(store: &mut MemoryStore) {
    store.put_workflow(workflow(&wf(2), "utility_bill_payment", "approved", None));
    create(store, 2, 2).expect("create_agent of the second agent");
}

#[test]
fn a_runner_body_naming_a_run_of_another_agent_is_refused() {
    let mut store = store_with("utility_bill_payment");
    second_agent(&mut store);
    payment_through_begin(&mut store, 1, 1);
    assert_eq!(
        put(
            &mut store,
            as_agent_2(row(1, 2, 1, "held", "payment", 1, None, None))
        ),
        Err(declared("binding"))
    );
    assert_eq!(
        finish(
            &mut store,
            as_agent_2(run_record(1, Some("replay"), "failed"))
        ),
        Err(declared("binding"))
    );
    let line = json!({
        "v": 1,
        "workflow": wf(2),
        "agent": agent(2),
        "run": run(1),
        "seq": 1,
        "day": 20_770,
        "at": 1_794_400_000_000_000_000_i64,
        "event": "step_ok",
        "step": 1,
        "cause": null,
        "model": null,
        "ms": null,
        "tokens_in": null,
        "tokens_out": null,
    });
    assert_eq!(
        runner(
            &mut store,
            json!({"op": "log", "agent": agent(2), "run": run(1), "records": [line]})
        ),
        Err(declared("binding"))
    );
    assert_eq!(effects(&store).len(), 3);
    assert!(rows(&store, "agent_log").is_empty());
    assert_eq!(
        finish(&mut store, run_record(1, Some("replay"), "effect_unknown")),
        Ok(stored(1))
    );
    assert_eq!(last_of(&store, &effect(1)).state, EffectState::Unknown);
}

#[test]
fn a_run_that_another_agent_ended_or_logged_is_not_this_agent_s() {
    let mut store = store_with("utility_bill_payment");
    second_agent(&mut store);
    assert_eq!(
        coordinator(
            &mut store,
            json!({"op": "end_run", "workflow": wf(2), "agent": agent(2), "run": run(7)})
        ),
        Ok(accepted())
    );
    assert_eq!(
        put(&mut store, row(7, 1, 1, "held", "payment", 1, None, None)),
        Err(declared("binding"))
    );
    assert_eq!(end_run(&mut store, 7), Err(declared("binding")));
    let mut line = json!({
        "v": 1,
        "workflow": wf(2),
        "agent": agent(2),
        "run": run(8),
        "seq": 1,
        "day": 20_770,
        "at": 1_794_400_000_000_000_000_i64,
        "event": "step_ok",
        "step": 1,
        "cause": null,
        "model": null,
        "ms": null,
        "tokens_in": null,
        "tokens_out": null,
    });
    assert_eq!(
        runner(
            &mut store,
            json!({"op": "log", "agent": agent(2), "run": run(8), "records": [line.clone()]})
        ),
        Ok(stored(1))
    );
    assert_eq!(
        put(&mut store, row(8, 1, 1, "held", "payment", 1, None, None)),
        Err(declared("binding"))
    );
    line["workflow"] = json!(wf(1));
    line["agent"] = json!(agent(1));
    assert_eq!(
        runner(
            &mut store,
            json!({"op": "log", "agent": agent(1), "run": run(8), "records": [line]})
        ),
        Err(declared("binding"))
    );
    assert!(effects(&store).is_empty());
    assert_eq!(runs(&store).len(), 1);
}

#[test]
fn the_end_of_a_run_is_for_its_own_agent_and_workflow() {
    let mut store = store_with("utility_bill_payment");
    payment_through_begin(&mut store, 1, 1);
    assert_eq!(
        coordinator(
            &mut store,
            json!({"op": "end_run", "workflow": wf(1), "agent": agent(2), "run": run(1)})
        ),
        Err(declared("binding"))
    );
    assert_eq!(
        coordinator(
            &mut store,
            json!({"op": "end_run", "workflow": wf(9), "agent": agent(1), "run": run(1)})
        ),
        Err(ErrorName::NoWorkflow)
    );
    let mut other = run_record(1, Some("replay"), "failed");
    other["workflow"] = json!(wf(2));
    assert_eq!(finish(&mut store, other), Err(declared("binding")));

    store.put_workflow(workflow(&wf(2), "utility_bill_payment", "approved", None));
    create(&mut store, 2, 2).expect("create_agent");
    assert_eq!(
        coordinator(
            &mut store,
            json!({"op": "end_run", "workflow": wf(2), "agent": agent(2), "run": run(1)})
        ),
        Err(declared("binding"))
    );
    assert!(runs(&store).is_empty());
    assert_eq!(last_of(&store, &effect(1)).state, EffectState::Releasing);
}

#[test]
fn the_typed_reader_refuses_what_check_record_takes() {
    let entry = registry()
        .find(|e| e.name == "agent_run")
        .expect("the entry of agent_run");
    let record = run_record(1, None, "completed");
    assert_eq!(check_record(entry, &record.to_string()), Ok(()));
    let mut store = store_with("utility_bill_payment");
    assert_eq!(finish(&mut store, record), Err(ErrorName::Malformed));
    assert!(runs(&store).is_empty());

    let config = registry()
        .find(|e| e.name == "agent_config")
        .expect("the entry of agent_config");
    let mut store = MemoryStore::new();
    store.put_workflow(workflow(&wf(1), "email", "approved", Some(&agent(1))));
    let paying = configs_json_with_allow("payment");
    assert_eq!(check_record(config, &paying), Ok(()));
    assert_eq!(
        store.commit(vec![TypedRecord {
            kind: "agent_config",
            json: paying
        }]),
        Ok(())
    );
    assert_eq!(
        runner(
            &mut store,
            json!({"op": "load", "agent": agent(1), "run": run(1)})
        )
        .map(|_| ()),
        Err(ErrorName::Store)
    );
}

fn configs_json_with_allow(class: &str) -> String {
    format!(
        r#"{{"v":1,"workflow":"{}","agent":"{}","rev":1,"category":"email","tier":"on_device","reasoning":"medium","autonomy":"unattended","allow":["{class}"],"caps":{{"untrusted":"no","private":"yes","external":"yes"}},"limits":{{"steps":20,"wall_s":120,"model_tokens":0}},"budget":null,"permissions":[],"runtime":"graph_mail","playbook":null}}"#,
        wf(1),
        agent(1)
    )
}
