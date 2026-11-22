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
    start(&mut store, 2);
    payment_through_begin(&mut store, 2, 3);
    let mut record = run_record(1, Some("replay"), "effect_unknown");
    record["unknown"] = json!(1);
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
    assert_eq!((lost.day, lost.at), (TODAY, NOW.at));
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
    let mut record = run_record(1, Some("replay"), "effect_unknown");
    record["unknown"] = json!(1);
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
        "seq": 2,
        "day": ROW_DAY,
        "at": ROW_AT,
        "event": "step_ok",
        "step": 1,
        "cause": null,
        "model": null,
        "ms": null,
        "tokens_in": null,
        "tokens_out": null, "page_amount": null,
    });
    assert_eq!(
        runner(
            &mut store,
            json!({"op": "log", "agent": agent(2), "run": run(1), "records": [line]})
        ),
        Err(declared("binding"))
    );
    assert_eq!(effects(&store).len(), 3);
    assert_eq!(rows(&store, "agent_log").len(), 1);
    let mut record = run_record(1, Some("replay"), "effect_unknown");
    record["unknown"] = json!(1);
    assert_eq!(finish(&mut store, record), Ok(stored(1)));
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
        "seq": 2,
        "day": ROW_DAY,
        "at": ROW_AT,
        "event": "step_ok",
        "step": 1,
        "cause": null,
        "model": null,
        "ms": null,
        "tokens_in": null,
        "tokens_out": null, "page_amount": null,
    });
    assert_eq!(start_at(&mut store, NOW, 2, 2, 8), Ok(accepted()));
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

#[test]
fn end_run_of_a_run_with_no_start_row_stores_its_record_as_before() {
    let mut store = unstarted("utility_bill_payment");
    assert!(rows(&store, "agent_log").is_empty());
    assert_eq!(end_run(&mut store, 3), Ok(accepted()));
    let stored_runs = runs(&store);
    assert_eq!(stored_runs.len(), 1);
    assert_eq!(stored_runs[0].run.as_str(), run(3));
    assert_eq!(stored_runs[0].mode, None);
    assert_eq!(end_run(&mut store, 3), Ok(accepted()));
    assert_eq!(runs(&store).len(), 1);
}

#[test]
fn a_finish_dated_after_the_clock_or_outside_its_bound_is_refused_with_nothing_of_the_end_stored() {
    let mut store = store_with("utility_bill_payment");
    payment_through_begin(&mut store, 1, 1);
    let before = effects(&store).len();
    for day in [TODAY + 1, TODAY - 93] {
        let mut record = run_record(1, Some("replay"), "effect_unknown");
        record["unknown"] = json!(1);
        record["day"] = json!(day);
        assert_eq!(finish(&mut store, record), Err(declared("day")), "{day}");
        assert!(runs(&store).is_empty());
        assert_eq!(effects(&store).len(), before);
    }
    let mut record = run_record(1, Some("replay"), "effect_unknown");
    record["unknown"] = json!(1);
    record["day"] = json!(TODAY - 92);
    assert_eq!(finish(&mut store, record), Ok(stored(1)));
}

#[test]
fn end_run_and_the_moves_of_the_end_take_the_local_day_of_the_request() {
    let mut store = store_with("utility_bill_payment").with_utc_offset(WEST);
    payment_through_begin(&mut store, 1, 1);
    let night = at(local(TODAY, 22 * 3600, WEST));
    assert_eq!(night.at.0.div_euclid(NANOS_PER_DAY), i64::from(TODAY) + 1);
    let body = json!({"op": "end_run", "workflow": wf(1), "agent": agent(1), "run": run(1)});
    assert_eq!(coordinator_at(&mut store, night, body), Ok(accepted()));
    assert_eq!(runs(&store)[0].day, TODAY);
    let lost = last_of(&store, &effect(1));
    assert_eq!((lost.day, lost.at), (TODAY, night.at));
}

fn open_effect(store: &mut MemoryStore, r: u32, state: &str) {
    start(store, r);
    let path: Vec<serde_json::Value> = match state {
        "held" => vec![row(r, r, 1, "held", "payment", 1, None, None)],
        "cleared" => vec![row(r, r, 1, "cleared", "send", 1, None, None)],
        "approved" => vec![
            row(r, r, 1, "held", "payment", 1, None, None),
            row(r, r, 2, "approved", "payment", 1, Some(r), None),
        ],
        "releasing" => vec![
            row(r, r, 1, "cleared", "send", 1, None, None),
            row(r, r, 2, "releasing", "send", 1, None, None),
        ],
        other => panic!("no path to {other}"),
    };
    for m in path {
        assert_eq!(put(store, m.clone()), Ok(stored(1)), "{m}");
    }
}

#[test]
fn a_completed_finish_while_an_effect_of_the_run_is_open_is_refused_with_effect() {
    let mut store = unattended_sender();
    for (r, state) in [
        (2, "held"),
        (3, "cleared"),
        (4, "approved"),
        (5, "releasing"),
    ] {
        open_effect(&mut store, r, state);
        let before = effects(&store).len();
        let completed = run_record(r, Some("replay"), "completed");
        assert_eq!(
            finish(&mut store, completed),
            Err(declared("effect")),
            "{state}"
        );
        assert!(runs(&store).is_empty(), "{state}");
        assert_eq!(effects(&store).len(), before, "{state}");
    }
    let mut lost = run_record(5, Some("replay"), "effect_unknown");
    lost["unknown"] = json!(1);
    assert_eq!(finish(&mut store, lost), Ok(stored(1)));
    let failed = run_record(2, Some("replay"), "failed");
    assert_eq!(finish(&mut store, failed), Ok(stored(1)));
    assert_eq!(last_of(&store, &effect(2)).state, EffectState::Rejected);
}

#[test]
fn a_completed_finish_after_every_effect_of_the_run_ended_is_stored() {
    let mut store = unattended_sender();
    for m in [
        row(1, 1, 1, "cleared", "send", 1, None, None),
        row(1, 1, 2, "releasing", "send", 1, None, None),
        row(1, 1, 3, "committed", "send", 1, None, None),
        row(1, 2, 1, "held", "payment", 1, None, None),
        row(1, 2, 2, "rejected", "payment", 1, None, None),
    ] {
        assert_eq!(put(&mut store, m.clone()), Ok(stored(1)), "{m}");
    }
    let mut completed = run_record(1, Some("replay"), "completed");
    completed["effects"] = json!(2);
    assert_eq!(finish(&mut store, completed), Ok(stored(1)));
}

#[test]
fn a_finish_whose_unknown_count_is_below_the_run_s_unknown_effects_after_the_moves_is_refused() {
    let mut store = unattended_sender();
    for e in [1, 2] {
        for m in [
            row(1, e, 1, "cleared", "send", 1, None, None),
            row(1, e, 2, "releasing", "send", 1, None, None),
        ] {
            assert_eq!(put(&mut store, m.clone()), Ok(stored(1)), "{m}");
        }
    }
    let before = effects(&store).len();
    for unknown in [0, 1] {
        let mut record = run_record(1, Some("replay"), "effect_unknown");
        record["effects"] = json!(2);
        record["unknown"] = json!(unknown);
        assert_eq!(
            finish(&mut store, record),
            Err(declared("effect")),
            "{unknown}"
        );
        assert!(runs(&store).is_empty());
        assert_eq!(effects(&store).len(), before);
    }
    let mut record = run_record(1, Some("replay"), "effect_unknown");
    record["effects"] = json!(2);
    record["unknown"] = json!(2);
    assert_eq!(finish(&mut store, record), Ok(stored(1)));
    assert_eq!(last_of(&store, &effect(2)).state, EffectState::Unknown);
}

#[test]
fn finish_stores_each_key_of_the_run_s_list_as_given() {
    let mut store = store_with("email");
    let mut record = run_record(1, Some("replay"), "failed");
    record["effects"] = json!(0);
    for (key, value) in [
        ("before_start", json!(5)),
        ("cut", json!("yes")),
        ("no_key", json!(0)),
        ("not_to_account", json!(70)),
        ("incomplete", json!(1)),
        ("reply_to_other", json!(4_294_967_295_u32)),
        ("more_pages", json!("no")),
        ("held_recipient", json!(3)),
        ("organized", json!(null)),
        ("other_task", json!(250)),
        ("held_earlier", json!(9)),
        ("earlier_page", json!(6)),
    ] {
        record[key] = value;
    }
    assert_eq!(finish(&mut store, record.clone()), Ok(stored(1)));
    let stored_runs = runs(&store);
    assert_eq!(stored_runs.len(), 1);
    assert_eq!(
        stored_runs[0],
        AgentRun::parse(record.to_string().as_bytes()).expect("the body's record")
    );
    let row: serde_json::Value =
        serde_json::from_str(&rows(&store, "agent_run")[0]).expect("a stored row");
    for key in [
        "before_start",
        "cut",
        "no_key",
        "not_to_account",
        "incomplete",
        "reply_to_other",
        "more_pages",
        "held_recipient",
        "organized",
        "other_task",
        "held_earlier",
        "earlier_page",
    ] {
        assert_eq!(row[key], record[key], "{key}");
    }
}

#[test]
fn a_finished_compile_switches_its_agent_on_and_the_next_compile_stores_above_the_rows_a_killed_one_left(
) {
    let mut store = store_with("utility_bill_payment");
    let two = vec![step(1, "navigate", "read"), step(2, "navigate", "read")];
    assert_eq!(put_steps(&mut store, 1, two.clone()), Ok(stored(2)));
    let mut record = run_record(1, Some("compile"), "completed");
    record["playbook"] = json!(1);
    assert_eq!(finish(&mut store, record), Ok(stored(1)));
    let active =
        json!({"op": "set_state", "workflow": wf(1), "agent": agent(1), "state": "active"});
    assert_eq!(
        coordinator(&mut store, active),
        Ok(
            halvo_schema::executable::CoordinatorReply::parse(br#"{"state":"active"}"#)
                .expect("a state reply")
        )
    );
    start_at(&mut store, NOW, 1, 1, 2).expect("start_run of the compile that is killed");
    let killed = json!({"op": "put_playbook", "agent": agent(1), "run": run(2), "number": 2, "steps": [step(1, "navigate", "read")]});
    assert_eq!(runner(&mut store, killed), Ok(stored(1)));
    start_at(&mut store, NOW, 1, 1, 3).expect("start_run of the next compile");
    let loaded = runner(
        &mut store,
        json!({"op": "load", "agent": agent(1), "run": run(3)}),
    )
    .expect("load")
    .to_json()
    .expect("the reply's text");
    let loaded: serde_json::Value = serde_json::from_str(&loaded).expect("a JSON reply");
    assert_eq!(loaded["playbook"], json!({"number": 1, "steps": 2}));
    assert_eq!(loaded["next_playbook"], json!(3));
    let next =
        json!({"op": "put_playbook", "agent": agent(1), "run": run(3), "number": 3, "steps": two});
    assert_eq!(runner(&mut store, next), Ok(stored(2)));
    let mut record = run_record(3, Some("compile"), "completed");
    record["playbook"] = json!(3);
    assert_eq!(finish(&mut store, record), Ok(stored(1)));
    let agent_1 = halvo_schema::AgentId::parse(&agent(1)).expect("an agent id");
    let finished = halvo_agent_store::newest_playbook(&store, &agent_1)
        .expect("the rows are read")
        .map(|p| (p.number.get(), p.steps.get()));
    assert_eq!(finished, Some((3, 2)));
}
