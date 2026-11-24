//! r3 U11.7: the read `sheet_answers` hands the core the stored values of the spreadsheet agent's four slots,
//! `workbook`, `sheet`, `range` and `source`, those four alone: never a slot of an agent of another category, never a
//! fifth slot and never a payment's. The values of these tests are made-up texts; they are unit tests of the library,
//! which the merge gate runs in both jobs, over the store in memory.

use halvo_schema::executable::text::OwnValue;
use halvo_schema::{ErrorName, Instant, TypedRecord, Workflow, WorkflowId};
use serde_json::{json, Value};

use crate::{MemoryStore, Now};

/// The instant of every request of these tests, 2026-11-12T16:13:20Z, as in the store's test targets.
const NOW: Now = Now {
    at: Instant(1_794_500_000_000_000_000),
};

fn id(prefix: char, n: u32) -> String {
    format!("{prefix}{n:031x}")
}

fn wf(n: u32) -> String {
    id('3', n)
}

fn agent(n: u32) -> String {
    id('4', n)
}

fn declared(name: &str) -> ErrorName {
    ErrorName::read(name).expect("a declared error name")
}

fn workflow(n: u32, category: &str, agent: Option<&str>) -> Workflow {
    serde_json::from_value(json!({
        "v": 1,
        "id": wf(n),
        "state": "approved",
        "category": category,
        "site": null,
        "app": "com.example.browser",
        "trigger": "schedule",
        "recurrence": {"count": 3, "period": 31, "first": 20705, "last": 20767},
        "confidence": 87,
        "agent": agent,
    }))
    .expect("a workflow record")
}

/// The reply of the body, written as the store writes it, or the store's refusal.
fn coordinator(store: &mut MemoryStore, body: Value) -> Result<String, ErrorName> {
    crate::coordinator(store, NOW, body.to_string().as_bytes())
        .map(|r| r.to_json().expect("the reply's JSON"))
}

/// The read of the agent `a` of the workflow `w`.
fn answers(store: &mut MemoryStore, w: u32, a: u32) -> Result<String, ErrorName> {
    coordinator(
        store,
        json!({"op": "sheet_answers", "workflow": wf(w), "agent": agent(a)}),
    )
}

fn slots(pairs: &[(&str, &str)]) -> Value {
    pairs
        .iter()
        .map(|(name, value)| json!({"name": name, "value": OwnValue::new(value)}))
        .collect()
}

/// `set_bindings` of the agent `a` of the workflow `w` with the slots.
fn bind(store: &mut MemoryStore, w: u32, a: u32, pairs: &[(&str, &str)]) {
    coordinator(
        store,
        json!({"op": "set_bindings", "workflow": wf(w), "agent": agent(a), "slots": slots(pairs)}),
    )
    .expect("set_bindings");
}

/// A spreadsheet agent `1` of the approved workflow `1`, made by `create_agent`, with no bindings.
fn spreadsheet() -> MemoryStore {
    let mut store = MemoryStore::new();
    store.put_workflow(workflow(1, "spreadsheet_fill", None));
    coordinator(
        &mut store,
        json!({"op": "create_agent", "workflow": wf(1), "agent": agent(1)}),
    )
    .expect("create_agent");
    store
}

/// An absolute path of the form of this system, which `set_bindings` takes in the slot `workbook`.
fn native(steps: &str) -> String {
    if cfg!(windows) {
        format!("C:\\{}", steps.replace('/', "\\"))
    } else {
        format!("/{steps}")
    }
}

/// The reply that the read writes for the four values, none where a value is `None`.
fn reply(four: [Option<&str>; 4]) -> String {
    let text = |v: Option<&str>| serde_json::to_string(&v).expect("JSON");
    format!(
        r#"{{"workbook":{},"sheet":{},"range":{},"source":{}}}"#,
        text(four[0]),
        text(four[1]),
        text(four[2]),
        text(four[3])
    )
}

const SHEET: &str = "März – \"Übersicht\"";
const RANGE: &str = "B2:D14";

/// U11.7 test 1: the read answers the four values of a spreadsheet agent as `set_bindings` stored them, each byte for
/// byte, a quote and letters of more than one byte among them, and the value of the newest revision where a later
/// `set_bindings` replaced one.
#[test]
fn u117_the_read_answers_the_four_values_of_a_spreadsheet_agent_as_they_were_stored() {
    let mut store = spreadsheet();
    let book = native("srv/Tabellen für 2026/Monat.xlsx");
    let source = native("srv/Daten/März.csv");
    bind(
        &mut store,
        1,
        1,
        &[
            ("workbook", book.as_str()),
            ("sheet", "Februar"),
            ("range", RANGE),
            ("source", source.as_str()),
        ],
    );
    bind(&mut store, 1, 1, &[("sheet", SHEET)]);
    assert_eq!(
        answers(&mut store, 1, 1),
        Ok(reply([
            Some(&book),
            Some(SHEET),
            Some(RANGE),
            Some(&source)
        ]))
    );
}

/// Every category but `spreadsheet_fill`.
const OTHERS: [&str; 9] = [
    "utility_bill_payment",
    "tenant_rent",
    "lease_renewal_or_change",
    "tenant_application_review",
    "email",
    "shop_order",
    "calendar_cleanup",
    "ride_order",
    "daily_chart_check",
];

/// Values that must never cross, each a text that no other value of these tests holds.
const NEVER: [(&str, &str); 8] = [
    ("instrument", "never-instrument-4111"),
    ("payee", "never-payee-Example-Power"),
    ("start", "https://never-start.test:443"),
    ("seller", "never-seller-Example-Shop"),
    ("account_address", "never-account@example.test"),
    ("reply_senders", "never-sender@example.test"),
    ("amount", "never-amount-12.34"),
    ("note", "never-note"),
];

/// The agent `1` of the workflow `1` of the category, as a stored workflow that names it and one stored revision
/// of its bindings with the slots, written as rows: so an agent of each category holds stored values, whichever
/// category `create_agent` has a preset for.
fn stored_agent(category: &str, pairs: &[(&str, &str)]) -> MemoryStore {
    let mut store = MemoryStore::new();
    store.put_workflow(workflow(1, category, Some(&agent(1))));
    let row = json!({
        "v": 1, "workflow": wf(1), "agent": agent(1), "rev": 1, "origins": [], "slots": slots(pairs),
    });
    store
        .typed_commit(vec![TypedRecord {
            kind: "agent_bindings",
            json: row.to_string(),
        }])
        .expect("the bindings row");
    store
}

/// U11.7 test 2, OF WEIGHT: no other value crosses. An agent of every other category whose stored bindings hold
/// the four names and the slots of a payment is refused with `config`, and the refusal holds none of its values; a
/// spreadsheet agent whose bindings hold slots beside the four answers the four alone, and no byte of another
/// slot's value stands in its reply.
#[test]
fn u117_no_value_crosses_for_another_category_a_slot_beside_the_four_or_a_payments_slot() {
    let book = native("srv/never/Book.xlsx");
    let mut all: Vec<(&str, &str)> = vec![
        ("workbook", book.as_str()),
        ("sheet", "never-sheet"),
        ("range", "never-range"),
        ("source", "never-source"),
    ];
    all.extend(NEVER);
    for category in OTHERS {
        let mut store = stored_agent(category, &all);
        assert_eq!(
            answers(&mut store, 1, 1),
            Err(declared("config")),
            "{category}"
        );
    }
    let mut store = stored_agent(
        "spreadsheet_fill",
        &[
            ("workbook", book.as_str()),
            ("sheet", SHEET),
            ("range", RANGE),
            ("source", "Daten.csv"),
        ]
        .into_iter()
        .chain(NEVER)
        .collect::<Vec<_>>(),
    );
    let read = answers(&mut store, 1, 1).expect("the read of the spreadsheet agent");
    assert_eq!(
        read,
        reply([Some(&book), Some(SHEET), Some(RANGE), Some("Daten.csv")])
    );
    for (name, value) in NEVER {
        assert!(!read.contains(name), "{name} in {read}");
        assert!(!read.contains(value), "{value} in {read}");
    }
}

/// U11.7 test 3: a value that is gone reads null, never an empty text, and a pair that the store no longer holds is
/// refused: bindings never set give four nulls; a revision that holds no `range` and an empty `source` gives null for
/// both; a stored value of more than 1,024 bytes is refused with `length`, never cut; an agent that a new
/// `create_agent` replaced is refused with `binding` and the new agent reads four nulls; a deleted workflow is
/// refused with `no_workflow`.
#[test]
fn u117_a_value_that_is_gone_reads_null_and_a_pair_that_is_gone_is_refused() {
    let mut store = spreadsheet();
    assert_eq!(answers(&mut store, 1, 1), Ok(reply([None; 4])));
    let book = native("srv/Book.xlsx");
    bind(
        &mut store,
        1,
        1,
        &[
            ("workbook", book.as_str()),
            ("sheet", SHEET),
            ("source", ""),
        ],
    );
    assert_eq!(
        answers(&mut store, 1, 1),
        Ok(reply([Some(&book), Some(SHEET), None, None]))
    );
    bind(&mut store, 1, 1, &[("range", &"A".repeat(1025))]);
    assert_eq!(answers(&mut store, 1, 1), Err(declared("length")));
    bind(&mut store, 1, 1, &[("range", &"A".repeat(1024))]);
    assert_eq!(
        answers(&mut store, 1, 1),
        Ok(reply([
            Some(&book),
            Some(SHEET),
            Some(&"A".repeat(1024)),
            None
        ]))
    );
    store.put_workflow(workflow(1, "spreadsheet_fill", None));
    coordinator(
        &mut store,
        json!({"op": "create_agent", "workflow": wf(1), "agent": agent(2)}),
    )
    .expect("create_agent of a new agent");
    assert_eq!(answers(&mut store, 1, 1), Err(declared("binding")));
    assert_eq!(answers(&mut store, 1, 2), Ok(reply([None; 4])));
    store.remove_workflow(&WorkflowId::parse(&wf(1)).expect("an id"));
    assert_eq!(answers(&mut store, 1, 2), Err(ErrorName::NoWorkflow));
}
