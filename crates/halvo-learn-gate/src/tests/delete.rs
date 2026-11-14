use std::collections::BTreeSet;

use halvo_schema::{TypedRecord, WorkflowId};
use serde_json::Value;

use crate::delete::{seed, seed_cases, workflow_line, MARKERS};
use crate::store::{Family, MemoryStore};
use crate::stub::id;

fn names(json: &str, workflow: &str) -> bool {
    serde_json::from_str::<Value>(json).is_ok_and(|v| v["workflow"] == workflow)
}

#[test]
fn delete_seeds_each_marker_in_an_active_and_a_reverted_row_of_its_workflow() {
    let cases = seed_cases();
    let store = seed(&cases).expect("the seeded store").0.host.store;
    let edits: Vec<Value> = store
        .rows(Family::Learn)
        .all()
        .filter(|(kind, _)| *kind == "learn_edit")
        .map(|(_, json)| serde_json::from_str(json).expect("a row"))
        .collect();
    for (n, marker) in (1u32..).zip(MARKERS) {
        let of: Vec<&Value> = edits
            .iter()
            .filter(|e| e["workflow"] == id('3', n))
            .collect();
        for state in ["active", "reverted"] {
            let marked = of.iter().any(|e| {
                e["state"] == state
                    && e["target"]["locators"][0]["name"]
                        .as_str()
                        .is_some_and(|name| name.contains(marker))
                    && e["target"]["desc"]
                        .as_str()
                        .is_some_and(|d| d.contains(marker))
            });
            assert!(marked, "workflow {n}: no {state} row with its marker");
        }
        let other = MARKERS[2 - n as usize];
        assert!(
            of.iter().all(|e| !e.to_string().contains(other)),
            "workflow {n} holds the other marker"
        );
    }
}

#[test]
fn delete_seeds_both_workflows_with_the_rows_of_the_same_number_of_runs() {
    let cases = seed_cases();
    let store = seed(&cases).expect("the seeded store").0.host.store;
    let runs = |workflow: &str, family: Family, kind: &str| -> usize {
        store
            .rows(family)
            .all()
            .filter(|(k, json)| *k == kind && names(json, workflow))
            .map(|(_, json)| {
                serde_json::from_str::<Value>(json).expect("a row")["run"]
                    .clone()
                    .to_string()
            })
            .collect::<BTreeSet<String>>()
            .len()
    };
    for (family, kind) in [(Family::Learn, "learn_run"), (Family::Agent, "agent_run")] {
        let first = runs(&id('3', 1), family, kind);
        assert!(first > 0, "{kind}");
        assert_eq!(first, runs(&id('3', 2), family, kind), "{kind}");
    }
}

fn line(cascade: &mut dyn FnMut(&mut MemoryStore, &WorkflowId) -> bool) -> (String, bool) {
    workflow_line(cascade).expect("the delete line")
}

fn miss(cascade: &mut dyn FnMut(&mut MemoryStore, &WorkflowId) -> bool, what: &str) {
    let (text, ok) = line(cascade);
    assert!(!ok && text.ends_with(" result=miss"), "{what}: {text}");
}

fn remove_first(store: &mut MemoryStore, kind: &str, keep_out: impl Fn(&str) -> bool) {
    let rows = store
        .learn_rows_mut()
        .get_mut(kind)
        .expect("rows of the kind");
    let at = rows
        .iter()
        .position(|r| keep_out(r))
        .expect("a row to take");
    rows.remove(at);
}

#[test]
fn delete_line_reads_ok_with_the_store_s_cascade() {
    let (text, ok) = line(&mut |store, id| store.delete_workflow(id));
    assert!(ok, "{text}");
    assert!(
        text.starts_with("delete what=workflow store=memory rows_left=0 other_rows="),
        "{text}"
    );
    assert!(text.ends_with(" other_served=yes result=ok"), "{text}");
}

#[test]
fn delete_line_misses_a_cascade_that_keeps_a_learn_row_of_the_deleted_workflow() {
    for kind in ["learn_run", "learn_edit", "learn_routine"] {
        miss(
            &mut |store, id| {
                let workflow = id.as_str().to_owned();
                let row = store.learn_rows_mut()[kind]
                    .iter()
                    .find(|r| names(r, &workflow))
                    .expect("a row of the first workflow")
                    .clone();
                let stored = store.delete_workflow(id);
                store
                    .learn_rows_mut()
                    .entry(kind_name(kind))
                    .or_default()
                    .push(row);
                stored
            },
            kind,
        );
    }
}

fn kind_name(kind: &str) -> &'static str {
    ["learn_run", "learn_edit", "learn_routine"]
        .into_iter()
        .find(|k| *k == kind)
        .expect("a kind of the family learn")
}

#[test]
fn delete_line_misses_a_cascade_that_takes_a_row_of_the_other_workflow() {
    let second = id('3', 2);
    let cases: [(&str, &str, &str); 4] = [
        ("learn_run", "", "a learn_run row"),
        ("learn_edit", "\"state\":\"reverted\"", "the reverted row"),
        ("learn_edit", "\"state\":\"active\"", "the active row"),
        ("learn_routine", "", "the routine row"),
    ];
    for (kind, holds, what) in cases {
        let second = second.clone();
        miss(
            &mut |store, id| {
                let stored = store.delete_workflow(id);
                remove_first(store, kind, |r| names(r, &second) && r.contains(holds));
                stored
            },
            what,
        );
    }
}

#[test]
fn delete_line_reads_other_served_no_when_the_other_workflow_s_reverted_row_is_taken() {
    let second = id('3', 2);
    let (text, ok) = line(&mut |store, id| {
        let stored = store.delete_workflow(id);
        remove_first(store, "learn_edit", |r| {
            names(r, &second) && r.contains("\"state\":\"reverted\"")
        });
        stored
    });
    assert!(!ok, "{text}");
    assert!(text.contains(" other_served=no "), "{text}");
    assert!(text.ends_with(" result=miss"), "{text}");
}

#[test]
fn delete_line_misses_a_cascade_that_rewrites_a_row_of_the_other_workflow() {
    let second = id('3', 2);
    miss(
        &mut |store, id| {
            let stored = store.delete_workflow(id);
            let rows = store
                .learn_rows_mut()
                .get_mut("learn_edit")
                .expect("edit rows");
            let row = rows
                .iter_mut()
                .find(|r| names(r, &second) && r.contains("\"state\":\"active\""))
                .expect("the active row of the second workflow");
            *row = row.replacen("Zq7", "Zq8", 1);
            stored
        },
        "a rewritten row",
    );
}

#[test]
fn delete_line_misses_a_cascade_that_takes_nothing_or_names_no_stored_workflow() {
    miss(&mut |_, _| true, "nothing taken");
    miss(
        &mut |store, id| {
            store.delete_workflow(id);
            false
        },
        "the cascade answered that the workflow was not stored",
    );
    let elsewhere = WorkflowId::parse(&id('3', 9)).expect("an id");
    miss(
        &mut |store, _| store.delete_workflow(&elsewhere),
        "no stored workflow",
    );
}

#[test]
fn delete_line_misses_when_the_other_agent_is_no_longer_served_its_learned_step() {
    let second = id('3', 2);
    let (text, ok) = line(&mut |store, id| {
        let stored = store.delete_workflow(id);
        let playbook: Vec<TypedRecord> = store
            .rows(Family::Agent)
            .all()
            .filter(|(kind, json)| *kind == "agent_playbook" && names(json, &second))
            .map(|(kind, json)| TypedRecord {
                kind,
                json: json.replacen("\"number\":1,", "\"number\":2,", 1),
            })
            .collect();
        assert!(!playbook.is_empty());
        store
            .commit(Family::Agent, playbook)
            .expect("a newer playbook of the second agent");
        stored
    });
    assert!(
        !ok && text.contains(" other_served=no ") && text.ends_with(" result=miss"),
        "{text}"
    );
}
