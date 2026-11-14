use halvo_schema::learning::body::Refinements;
use halvo_schema::learning::record::RunStatus;
use halvo_schema::{ErrorName, TypedRecord};
use serde_json::{json, Value};

use super::host::{active_world, agent, compiled, run, wf, workflow, world, World, DAY};
use crate::{CoordinatorAnswer, LearnHost, Refused};

pub fn edit_id(n: u32) -> String {
    super::host::id('e', n)
}

#[allow(clippy::too_many_arguments)]
pub fn edit_row(
    e: u32,
    rev: u32,
    base: u32,
    n: u32,
    state: &str,
    on: Option<u32>,
    runs: &[u32],
    days: (u32, u32),
    clean: u32,
    locators: Vec<Value>,
) -> TypedRecord {
    let cause = (state == "reverted").then_some("step_failed");
    let row = json!({
        "v": 1, "workflow": wf(1), "agent": agent(1), "edit": edit_id(e), "rev": rev, "base": base,
        "n": n, "state": state, "cause": cause, "on": on.map(edit_id),
        "runs": runs.iter().map(|r| run(*r)).collect::<Vec<_>>(), "first": days.0, "last": days.1,
        "clean": clean, "target": {"desc": "button Pay now", "locators": locators},
    });
    TypedRecord {
        kind: "learn_edit",
        json: row.to_string(),
    }
}

fn read(w: &mut World, body: Value) -> Result<Value, Refused> {
    match w.coordinator(body)? {
        CoordinatorAnswer::Refinements(reply) => {
            Ok(serde_json::from_str(&reply.to_json().expect("the reply's JSON")).expect("JSON"))
        }
        other => panic!("not the read's reply: {other:?}"),
    }
}

fn list(w: &mut World) -> Value {
    read(
        w,
        json!({"op": "list_refinements", "workflow": wf(1), "agent": agent(1)}),
    )
    .expect("the read is answered")
}

fn loc(name: &str) -> Value {
    json!({"by": "role", "value": "button", "name": name})
}

#[test]
fn start_run_stores_the_started_row_once_and_only_when_the_agent_store_answers_ok() {
    let mut w = active_world();
    let before = w.host.runs().len();
    let refused = w.coordinator(json!({
        "op": "start_run", "workflow": wf(1), "agent": agent(9), "run": run(2), "mode": "replay",
    }));
    assert!(refused.is_err());
    assert_eq!(w.host.runs().len(), before);
    w.host.day = DAY + 3;
    w.start(2, "replay");
    w.start(2, "replay");
    let rows: Vec<_> = w
        .host
        .runs()
        .into_iter()
        .filter(|r| r.run.as_str() == run(2))
        .collect();
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.status, RunStatus::Started);
    assert_eq!(row.day.get(), DAY + 3);
    assert_eq!(row.mode.as_str(), "replay");
    assert_eq!(row.workflow.as_str(), wf(1));
    assert_eq!(row.agent.as_str(), agent(1));
}

#[test]
fn create_agent_stores_the_routine_row_once_with_the_workflow_key() {
    let mut w = world();
    let routines = w.host.routines();
    assert_eq!(routines.len(), 1);
    let r = &routines[0];
    assert_eq!(r.rev.get(), 1);
    assert_eq!(r.day.get(), DAY);
    assert_eq!(r.category.as_str(), "utility_bill_payment");
    assert_eq!(
        serde_json::to_value(&r.site).expect("JSON"),
        json!({"key": "northwind-power.test", "source": "address_field"})
    );
    assert_eq!(r.app.as_str(), "com.example.browser");
    assert_eq!(
        (r.period, r.first, r.last, r.next, r.since),
        (None, None, None, None, None)
    );
    assert_eq!((r.seen, r.missed, r.mark.as_str()), (0, 0, "none"));
    let again = w.coordinator(json!({"op": "create_agent", "workflow": wf(1), "agent": agent(1)}));
    assert!(again.is_ok());
    assert_eq!(w.host.routines().len(), 1);
    let refused =
        w.coordinator(json!({"op": "create_agent", "workflow": wf(2), "agent": agent(2)}));
    assert_eq!(refused, Err(Refused::Error(ErrorName::NoWorkflow)));
    assert_eq!(w.host.routines().len(), 1);
}

#[test]
fn list_refinements_names_each_edit_of_the_newest_compile_number_by_its_newest_row() {
    let mut w = active_world();
    assert_eq!(list(&mut w), json!({"refinements": []}));
    let rows = vec![
        edit_row(
            1,
            1,
            1,
            2,
            "provisional",
            None,
            &[11],
            (DAY - 9, DAY - 9),
            0,
            vec![loc("A")],
        ),
        edit_row(
            1,
            2,
            1,
            2,
            "provisional",
            None,
            &[11, 12],
            (DAY - 9, DAY - 5),
            1,
            vec![loc("A")],
        ),
        edit_row(
            1,
            3,
            1,
            2,
            "active",
            None,
            &[11, 12, 13],
            (DAY - 9, DAY - 2),
            2,
            vec![loc("A")],
        ),
        edit_row(
            2,
            1,
            1,
            2,
            "provisional",
            Some(1),
            &[14],
            (DAY - 1, DAY - 1),
            0,
            vec![loc("B"), loc("A")],
        ),
        edit_row(
            3,
            1,
            1,
            3,
            "provisional",
            None,
            &[15],
            (DAY - 7, DAY - 7),
            0,
            vec![loc("C")],
        ),
        edit_row(
            3,
            2,
            1,
            3,
            "reverted",
            None,
            &[15],
            (DAY - 7, DAY - 7),
            0,
            vec![loc("C")],
        ),
        edit_row(
            4,
            1,
            1,
            3,
            "provisional",
            None,
            &[16],
            (DAY - 2, DAY - 2),
            0,
            vec![loc("D")],
        ),
        edit_row(
            4,
            2,
            1,
            3,
            "withdrawn",
            None,
            &[16],
            (DAY - 2, DAY - 2),
            0,
            vec![loc("D")],
        ),
    ];
    w.host.commit(rows).expect("the rows are stored");
    assert_eq!(
        list(&mut w),
        json!({"refinements": [
            {"day": DAY - 7, "what": "target_revoked"},
            {"day": DAY - 2, "what": "target_kept"},
            {"day": DAY - 1, "what": "target_learned"},
        ]})
    );
    let routine = json!({
        "v": 1, "workflow": wf(1), "agent": agent(1), "rev": 2, "day": DAY, "category": "utility_bill_payment",
        "site": null, "app": "com.example.browser", "period": 30, "seen": 3, "first": DAY - 200,
        "last": DAY - 90, "next": DAY - 60, "missed": 3, "mark": "stale", "since": DAY - 7,
    });
    w.host
        .commit(vec![TypedRecord {
            kind: "learn_routine",
            json: routine.to_string(),
        }])
        .expect("the routine row is stored");
    assert_eq!(
        list(&mut w),
        json!({"refinements": [
            {"day": DAY - 7, "what": "routine_stale"},
            {"day": DAY - 7, "what": "target_revoked"},
            {"day": DAY - 2, "what": "target_kept"},
            {"day": DAY - 1, "what": "target_learned"},
        ]})
    );
    w.compile(20, 2, compiled());
    let lower = list(&mut w);
    assert_eq!(
        lower,
        json!({"refinements": [{"day": DAY - 7, "what": "routine_stale"}]})
    );
    w.host
        .commit(vec![edit_row(
            5,
            1,
            2,
            2,
            "provisional",
            None,
            &[17],
            (DAY, DAY),
            0,
            vec![loc("E")],
        )])
        .expect("the row is stored");
    assert_eq!(
        list(&mut w),
        json!({"refinements": [
            {"day": DAY - 7, "what": "routine_stale"},
            {"day": DAY, "what": "target_learned"},
        ]})
    );
    let other = read(
        &mut w,
        json!({"op": "list_refinements", "workflow": wf(1), "agent": agent(2)}),
    );
    assert_eq!(other, Ok(json!({"refinements": []})));
    let other_workflow = read(
        &mut w,
        json!({"op": "list_refinements", "workflow": wf(2), "agent": agent(1)}),
    );
    assert_eq!(other_workflow, Ok(json!({"refinements": []})));
}

#[test]
fn list_refinements_refuses_another_shape_and_is_not_passed_on() {
    let mut w = active_world();
    for body in [
        json!({"op": "list_refinements", "workflow": wf(1)}),
        json!({"op": "list_refinements", "workflow": wf(1), "agent": agent(1), "run": run(1)}),
        json!({"op": "list_refinements", "workflow": "x", "agent": agent(1)}),
    ] {
        assert_eq!(
            w.coordinator(body),
            Err(Refused::Error(ErrorName::Malformed))
        );
    }
    let mut plain = halvo_agent_store::MemoryStore::new();
    plain.put_workflow(workflow(
        &wf(1),
        "utility_bill_payment",
        "active",
        Some(&agent(1)),
    ));
    let body = json!({"op": "list_refinements", "workflow": wf(1), "agent": agent(1)}).to_string();
    let now = w.host.agent_now();
    assert_eq!(
        halvo_agent_store::coordinator(&mut plain, now, body.as_bytes()),
        Err(ErrorName::Malformed)
    );
    let count = w.host.count();
    let reply = w.coordinator(serde_json::from_str(&body).expect("JSON"));
    assert!(matches!(
        reply,
        Ok(CoordinatorAnswer::Refinements(Refinements { .. }))
    ));
    assert_eq!(w.host.count(), count);
}

fn stale_routine(w: &mut World, a: u32) {
    let routine = json!({
        "v": 1, "workflow": wf(1), "agent": agent(a), "rev": 2, "day": DAY, "category": "utility_bill_payment",
        "site": null, "app": "com.example.browser", "period": 30, "seen": 3, "first": DAY - 200,
        "last": DAY - 90, "next": DAY - 60, "missed": 3, "mark": "stale", "since": DAY - 7,
    });
    w.host
        .commit(vec![TypedRecord {
            kind: "learn_routine",
            json: routine.to_string(),
        }])
        .expect("the routine row is stored");
}

#[test]
fn a_load_that_the_agent_store_refuses_gives_the_empty_list_also_beside_a_stale_routine_row() {
    let mut w = world();
    stale_routine(&mut w, 2);
    let reply = read(
        &mut w,
        json!({"op": "list_refinements", "workflow": wf(1), "agent": agent(2)}),
    );
    assert_eq!(reply, Ok(json!({"refinements": []})));
}

#[test]
fn an_agent_that_the_agent_store_holds_with_no_compile_number_still_gets_its_stale_routine() {
    let mut w = world();
    stale_routine(&mut w, 1);
    assert_eq!(
        list(&mut w),
        json!({"refinements": [{"day": DAY - 7, "what": "routine_stale"}]})
    );
}

#[test]
fn list_refinements_draws_a_new_run_id_for_each_call_and_no_row_log_line_or_reply_holds_one() {
    let mut w = active_world();
    w.learn(2, 2, "button Pay now", vec![loc("A")]);
    let kinds: Vec<&str> = halvo_schema::kinds::registry().map(|e| e.name).collect();
    let rows = |w: &World| -> Vec<String> {
        kinds
            .iter()
            .flat_map(|k| {
                if k.starts_with("learn_") {
                    w.host.learn_rows(k)
                } else {
                    w.host.agent_rows(k)
                }
            })
            .collect()
    };
    let before = rows(&w);
    assert!(
        before.iter().any(|r| r.contains("\"event\":")),
        "the store holds log lines"
    );
    let first = list(&mut w);
    let second = list(&mut w);
    let after = rows(&w);
    assert_eq!(after, before);
    assert_eq!(w.handlers.probes.len(), 2);
    assert_ne!(w.handlers.probes[0], w.handlers.probes[1]);
    for probe in &w.handlers.probes {
        let id = probe.as_str();
        assert!(!after.iter().any(|r| r.contains(id)));
        assert!(!first.to_string().contains(id) && !second.to_string().contains(id));
    }
    let refused = read(
        &mut w,
        json!({"op": "list_refinements", "workflow": wf(1), "agent": agent(9)}),
    );
    assert_eq!(refused, Ok(json!({"refinements": []})));
}
