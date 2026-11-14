use halvo_schema::learning::record::{LearnRoutine, LearnRun};
use halvo_schema::{AgentId, TypedRecord};

use super::fixtures::*;
use crate::rows::refused_workflow;

fn workflows(records: &[TypedRecord]) -> Vec<String> {
    records
        .iter()
        .map(|r| {
            assert_eq!(r.kind, "learn_routine");
            LearnRoutine::parse(r.json.as_bytes())
                .expect("a learn_routine row")
                .workflow
                .as_str()
                .to_owned()
        })
        .collect()
}

fn three_workflows() -> Store {
    let mut store = Store::default();
    store.put_routine(&fresh(1, DAY));
    store.put_routine(&LearnRoutine {
        agent: AgentId::parse(&agent(9)).expect("an agent id"),
        ..fresh(1, DAY)
    });
    store.put_routine(&fresh_at(2, DAY, domain(OTHER, "address_field"), BROWSER));
    store.put_routine(&fresh_at(3, DAY, domain(HOME, "sign_in"), BROWSER));
    store
}

fn refused_routine_of(w: u32) -> String {
    fresh(w, DAY)
        .to_json()
        .expect("a writable row")
        .replace("\"mark\":\"none\"", "\"mark\":\"gone\"")
}

fn refused_run_of(w: u32) -> String {
    run(w, 7, DAY, "replay", "verified")
        .to_json()
        .expect("a writable row")
        .replace("\"mode\":\"replay\"", "\"mode\":\"rerun\"")
}

#[test]
fn a_refused_row_of_either_kind_whose_workflow_key_can_be_read_stops_that_workflow_alone() {
    let today = DAY + 30;
    assert_eq!(
        workflows(&three_workflows().step(today)),
        vec![wf(1), wf(1), wf(2), wf(3)]
    );
    for (kind, json) in [
        ("learn_routine", refused_routine_of(1)),
        ("learn_run", refused_run_of(1)),
        ("learn_routine", format!("{{\"workflow\":\"{}\"}}", wf(1))),
        (
            "learn_run",
            format!("{{\"v\":2,\"workflow\":\"{}\"}}", wf(1)),
        ),
    ] {
        let mut store = three_workflows();
        store.rows.push((kind, json.clone()));
        let refused = match kind {
            "learn_run" => LearnRun::parse(json.as_bytes()).is_err(),
            _ => LearnRoutine::parse(json.as_bytes()).is_err(),
        };
        assert!(refused, "{kind} {json}");
        assert_eq!(
            workflows(&store.step(today)),
            vec![wf(2), wf(3)],
            "{kind} {json}"
        );
    }
}

#[test]
fn refused_rows_of_two_workflows_stop_both_and_a_refused_row_of_a_workflow_with_no_routine_row_stops_none(
) {
    let today = DAY + 30;
    let mut store = three_workflows();
    store.rows.push(("learn_routine", refused_routine_of(1)));
    store.rows.push(("learn_run", refused_run_of(3)));
    assert_eq!(workflows(&store.step(today)), vec![wf(2)]);

    let mut store = three_workflows();
    store.rows.push(("learn_run", refused_run_of(4)));
    store.rows.push(("learn_routine", refused_routine_of(5)));
    assert_eq!(
        workflows(&store.step(today)),
        vec![wf(1), wf(1), wf(2), wf(3)]
    );
}

#[test]
fn a_refused_row_whose_workflow_key_cannot_be_read_stops_every_workflow() {
    let today = DAY + 30;
    let name = wf(1);
    for json in [
        String::from("{}"),
        String::from("[]"),
        String::from("not json"),
        format!("{{\"workflow\":null,\"v\":1,\"x\":\"{name}\"}}"),
        String::from("{\"workflow\":7}"),
        format!("{{\"workflow\":[\"{name}\"]}}"),
        String::from("{\"workflow\":\"3000000000000000000000000000000G\"}"),
        format!("{{\"workflow\":\"{}\"}}", wf(10).to_uppercase()),
        format!("{{\"workflow\":\"{name}\",\"workflow\":\"{name}\"}}"),
        format!("{{\"workflow\":\"{name}\"}} {{}}"),
        format!("{{\"Workflow\":\"{name}\"}}"),
        format!("{{\"inner\":{{\"workflow\":\"{name}\"}}}}"),
    ] {
        for kind in ["learn_routine", "learn_run"] {
            let mut store = three_workflows();
            store.rows.push((kind, json.clone()));
            assert!(store.step(today).is_empty(), "{kind} {json}");
        }
    }
}

#[test]
fn the_key_reader_gives_the_workflow_under_the_registrys_key_of_each_kind_that_the_step_reads() {
    let name = wf(1);
    for kind in ["learn_routine", "learn_run", "learn_edit"] {
        for json in [
            format!("{{\"workflow\":\"{name}\"}}"),
            format!(
                "{{\"v\":9,\"mark\":[1,{{\"workflow\":2}}],\"workflow\":\"{name}\",\"x\":null}}"
            ),
            format!(" {{ \"workflow\" : \"{name}\" }}\n"),
            refused_routine_of(1),
            refused_run_of(1),
        ] {
            assert_eq!(
                refused_workflow(kind, &json).map(|w| w.as_str().to_owned()),
                Some(name.clone()),
                "{kind} {json}"
            );
        }
    }
}

#[test]
fn the_key_reader_gives_nothing_where_the_key_cannot_be_read_or_the_kind_has_none() {
    let name = wf(1);
    for json in [
        String::from(""),
        String::from("{}"),
        String::from("[]"),
        String::from("null"),
        format!("\"{name}\""),
        String::from("{\"workflow\":null}"),
        String::from("{\"workflow\":7}"),
        format!("{{\"workflow\":[\"{name}\"]}}"),
        format!("{{\"workflow\":{{\"id\":\"{name}\"}}}}"),
        String::from("{\"workflow\":\"\"}"),
        format!("{{\"workflow\":\"{}\"}}", wf(10).to_uppercase()),
        format!("{{\"workflow\":\"{name}0\"}}"),
        format!("{{\"workflow\":\"{name}\",\"workflow\":\"{name}\"}}"),
        format!("{{\"workflow\":\"{name}\",\"workflow\":\"{}\"}}", wf(2)),
        format!("{{\"workflow\":\"{name}\"}} {{}}"),
        format!("{{\"workflow\":\"{name}\""),
        format!("{{\"Workflow\":\"{name}\"}}"),
        format!("{{\"inner\":{{\"workflow\":\"{name}\"}}}}"),
    ] {
        assert_eq!(refused_workflow("learn_routine", &json), None, "{json}");
    }
    let row = format!("{{\"workflow\":\"{name}\",\"id\":\"{name}\"}}");
    assert_eq!(refused_workflow("no_such_kind", &row), None);
    assert_eq!(refused_workflow("summary", &row), None);
    assert_eq!(
        refused_workflow("workflow", &row).map(|w| w.as_str().to_owned()),
        Some(name.clone())
    );
    assert_eq!(
        refused_workflow("workflow", &format!("{{\"workflow\":\"{name}\"}}")),
        None
    );
}
