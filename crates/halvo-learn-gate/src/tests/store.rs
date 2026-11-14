use halvo_agent_store::Host;
use halvo_learn_store::LearnHost;
use halvo_schema::kinds::registry;
use halvo_schema::{RowRead, TypedRecord, Workflow, WorkflowId};
use serde_json::{json, Value};

use crate::host::GateHost;
use crate::store::{Family, MemoryStore};

pub fn wf(n: u32) -> String {
    format!("3{n:031x}")
}

pub fn workflow(id: &str) -> Workflow {
    serde_json::from_value(json!({
        "v": 1, "id": id, "state": "approved", "category": "utility_bill_payment",
        "site": {"key": "northwind-power.test", "source": "address_field"},
        "app": "com.example.browser", "trigger": "schedule",
        "recurrence": {"count": 3, "period": 31, "first": 20705, "last": 20767},
        "confidence": 87, "agent": null,
    }))
    .expect("a workflow record")
}

fn record(kind: &'static str, workflow: &str, day: Option<u32>) -> TypedRecord {
    let entry = registry()
        .find(|e| e.name == kind)
        .expect("a registered kind");
    let mut value: Value = serde_json::from_str(entry.example).expect("the entry's example");
    value["workflow"] = Value::String(workflow.to_owned());
    if let Some(day) = day {
        value["day"] = json!(day);
    }
    TypedRecord {
        kind,
        json: value.to_string(),
    }
}

fn visited(rows: &dyn RowRead, kind: &str) -> Vec<String> {
    let mut out = Vec::new();
    rows.for_each(kind, &mut |row| out.push(row.to_owned()));
    out
}

fn store_with(workflows: &[u32]) -> MemoryStore {
    let mut store = MemoryStore::new();
    for n in workflows {
        store.put_workflow(workflow(&wf(*n)));
    }
    store
}

#[test]
fn store_keeps_each_family_behind_its_own_calls() {
    let mut host = GateHost::new(store_with(&[1]), 20_771);
    assert_eq!(
        LearnHost::commit(&mut host, vec![record("learn_run", &wf(1), None)]),
        Ok(())
    );
    assert!(LearnHost::commit(&mut host, vec![record("agent_run", &wf(1), None)]).is_err());
    assert!(Host::commit(&mut host.store, vec![record("learn_run", &wf(1), None)]).is_err());
    assert_eq!(
        Host::commit(&mut host.store, vec![record("agent_run", &wf(1), None)]),
        Ok(())
    );
    assert_eq!(visited(LearnHost::rows(&host), "learn_run").len(), 1);
    assert!(visited(LearnHost::rows(&host), "agent_run").is_empty());
    assert_eq!(visited(Host::rows(&host.store), "agent_run").len(), 1);
    assert!(visited(Host::rows(&host.store), "learn_run").is_empty());
}

#[test]
fn store_commit_is_all_or_none() {
    let mut store = store_with(&[1]);
    let both = vec![
        record("learn_run", &wf(1), None),
        record("learn_run", &wf(2), None),
    ];
    assert!(store.commit(Family::Learn, both).is_err());
    assert_eq!(store.rows(Family::Learn).count("learn_run"), 0);
    let mut bad = record("learn_run", &wf(1), None);
    bad.json = bad.json.replacen('{', "{\"extra\":1,", 1);
    assert!(store.commit(Family::Learn, vec![bad]).is_err());
    assert_eq!(store.rows(Family::Learn).count("learn_run"), 0);
}

#[test]
fn store_rows_are_visited_in_byte_order() {
    let mut store = store_with(&[1, 2]);
    let rows = vec![
        record("learn_run", &wf(2), Some(20_771)),
        record("learn_run", &wf(1), Some(20_771)),
    ];
    store.commit(Family::Learn, rows).expect("a commit");
    let seen = visited(store.rows(Family::Learn), "learn_run");
    let mut sorted = seen.clone();
    sorted.sort();
    assert_eq!(seen, sorted);
    assert_eq!(seen.len(), 2);
}

#[test]
fn store_open_expires_bounded_rows_by_day() {
    let day = 20_000;
    let mut store = store_with(&[1]);
    let rows = [
        record("learn_run", &wf(1), Some(day)),
        record("learn_edit", &wf(1), None),
        record("agent_log", &wf(1), Some(day)),
    ];
    store
        .commit(Family::Learn, rows[..2].to_vec())
        .expect("a commit");
    store
        .commit(Family::Agent, rows[2..].to_vec())
        .expect("a commit");
    assert_eq!(store.clone().open(day - 2).of("learn_run"), 0);
    assert_eq!(store.clone().open(day - 3).of("learn_run"), 1);
    assert_eq!(store.open(day + 92).of("learn_run"), 0);
    let expired = store.open(day + 93);
    assert_eq!((expired.of("learn_run"), expired.of("agent_log")), (1, 1));
    assert_eq!(store.rows(Family::Learn).count("learn_run"), 0);
    assert_eq!(store.rows(Family::Learn).count("learn_edit"), 1);
    assert_eq!(store.open(day + 10_000).of("learn_edit"), 0);
    assert_eq!(store.rows(Family::Learn).count("learn_edit"), 1);
}

#[test]
fn store_delete_takes_every_row_of_one_workflow() {
    let mut store = store_with(&[1, 2]);
    for n in [1, 2] {
        let learn = vec![
            record("learn_run", &wf(n), None),
            record("learn_edit", &wf(n), None),
        ];
        store.commit(Family::Learn, learn).expect("a commit");
        store
            .commit(Family::Agent, vec![record("agent_run", &wf(n), None)])
            .expect("a commit");
    }
    assert!(store.delete_workflow(&WorkflowId::parse(&wf(1)).expect("an id")));
    let names = |family| {
        store
            .rows(family)
            .all()
            .map(|(_, json)| {
                serde_json::from_str::<Value>(json).expect("a row")["workflow"].clone()
            })
            .collect::<Vec<Value>>()
    };
    let (learn, agent) = (names(Family::Learn), names(Family::Agent));
    assert_eq!(learn, vec![json!(wf(2)), json!(wf(2))]);
    assert_eq!(agent, vec![json!(wf(2))]);
    assert!(store
        .workflow_item(&WorkflowId::parse(&wf(1)).expect("an id"))
        .is_none());
    assert!(store
        .workflow_item(&WorkflowId::parse(&wf(2)).expect("an id"))
        .is_some());
}
