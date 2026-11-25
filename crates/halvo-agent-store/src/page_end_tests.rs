//! r3 U5.10-P (the coordinator's rulings of log lines 20487 and 20577): the store's two reads hand the core a
//! payment's end. A `stopped` row of the read of a page after a reply names its effect; `agent_history` hands out
//! `page_cause` and `page_amount` for the effect that such a row names, and `list_agents` says whether a payment or a
//! purchase of the agent's newest run left (`left`). Unit tests of the library, which the merge gate runs in both
//! jobs, over the store in memory, with made-up values.

use halvo_schema::executable::ActionClass;
use halvo_schema::{Instant, RowRead, TypedRecord, Workflow};
use serde_json::{json, Value};

use crate::{Host, MemoryStore, Now};

/// The instant of every request of these tests, 2026-11-12T16:13:20Z, as in the store's test targets.
const NOW: Now = Now {
    at: Instant(1_794_500_000_000_000_000),
};

/// The instant and the day in UTC of the rows that the runner of these tests sends, 2026-11-11T12:26:40Z.
const ROW_AT: i64 = 1_794_400_000_000_000_000;
const ROW_DAY: u32 = 20_768;

fn id(prefix: char, n: u32) -> String {
    format!("{prefix}{n:031x}")
}

fn wf() -> String {
    id('3', 1)
}

fn agent() -> String {
    id('4', 1)
}

fn run(n: u32) -> String {
    id('5', n)
}

fn effect(n: u32) -> String {
    id('6', n)
}

fn workflow() -> Workflow {
    serde_json::from_value(json!({
        "v": 1,
        "id": wf(),
        "state": "approved",
        "category": "utility_bill_payment",
        "site": null,
        "app": "com.example.browser",
        "trigger": "schedule",
        "recurrence": {"count": 3, "period": 31, "first": 20705, "last": 20767},
        "confidence": 87,
        "agent": null,
    }))
    .expect("a workflow record")
}

/// The reply of a coordinator body as the store writes it, read as JSON; a refusal fails the test.
fn coordinator(store: &mut MemoryStore, body: Value) -> Value {
    let reply = crate::coordinator(store, NOW, body.to_string().as_bytes())
        .unwrap_or_else(|e| panic!("{body}: {e:?}"));
    serde_json::from_str(&reply.to_json().expect("the reply's JSON")).expect("the reply's text")
}

/// A runner body of the run `r`; a refusal fails the test.
fn runner(store: &mut MemoryStore, body: Value) {
    crate::runner(store, NOW, body.to_string().as_bytes())
        .unwrap_or_else(|e| panic!("{body}: {e:?}"));
}

/// The payment agent of an approved workflow, made by `create_agent`, with its run `r` started.
fn started(r: u32) -> MemoryStore {
    let mut store = MemoryStore::new();
    store.put_workflow(workflow());
    coordinator(
        &mut store,
        json!({"op": "create_agent", "workflow": wf(), "agent": agent()}),
    );
    coordinator(
        &mut store,
        json!({"op": "start_run", "workflow": wf(), "agent": agent(), "run": run(r), "mode": "compile"}),
    );
    store
}

/// The four rows of the effect `e` of the run `r` of the class `payment`, `held` to `committed`.
fn committed(store: &mut MemoryStore, r: u32, e: u32) {
    for (seq, state) in [
        (1, "held"),
        (2, "approved"),
        (3, "releasing"),
        (4, "committed"),
    ] {
        effect_row(store, r, e, seq, state);
    }
}

/// One row of the effect `e` of the run `r` of the class `payment`, in `state`.
fn effect_row(store: &mut MemoryStore, r: u32, e: u32, seq: u32, state: &str) {
    let kept: Vec<Value> = ActionClass::Payment
        .kept_fields()
        .into_iter()
        .map(|(name, _)| json!({"name": name.as_str(), "value": ["v"]}))
        .collect();
    let row = json!({
        "v": 1, "workflow": wf(), "agent": agent(), "run": run(r), "effect": effect(e), "seq": seq,
        "day": ROW_DAY, "at": ROW_AT, "state": state, "class": "payment", "digest": format!("d{e:063x}"),
        "kept": kept, "fields": ActionClass::Payment.bound_fields().len(), "approval": (state != "held").then(|| id('7', e)), "cause": null,
    });
    runner(
        store,
        json!({"op": "effect", "agent": agent(), "run": run(r), "record": row}),
    );
}

/// One log line of the run `r`: `event` at `step` with `cause`, `page_amount` and, where given, the key `effect`.
fn line(
    r: u32,
    seq: u32,
    (event, step, cause): (&str, Option<u32>, Option<&str>),
    page_amount: Option<u32>,
    named: Option<u32>,
) -> Value {
    let mut line = json!({
        "v": 1, "workflow": wf(), "agent": agent(), "run": run(r), "seq": seq, "day": ROW_DAY, "at": ROW_AT,
        "event": event, "step": step, "cause": cause, "model": null, "ms": null, "tokens_in": null,
        "tokens_out": null, "page_amount": page_amount,
    });
    if let Some(e) = named {
        line["effect"] = json!(effect(e));
    }
    line
}

fn log(store: &mut MemoryStore, r: u32, lines: Vec<Value>) {
    runner(
        store,
        json!({"op": "log", "agent": agent(), "run": run(r), "records": lines}),
    );
}

fn end(store: &mut MemoryStore, r: u32) {
    coordinator(
        store,
        json!({"op": "end_run", "workflow": wf(), "agent": agent(), "run": run(r)}),
    );
}

/// `page_cause` and `page_amount` of each effect of the newest run in the reply of `agent_history`, by effect id.
fn page_ends(store: &mut MemoryStore) -> Vec<(Value, Value, Value)> {
    let history = coordinator(
        store,
        json!({"op": "agent_history", "workflow": wf(), "agent": agent(), "runs": 1}),
    );
    history["runs"][0]["effects"]
        .as_array()
        .expect("the run's effects")
        .iter()
        .map(|e| {
            (
                e["effect"].clone(),
                e["page_cause"].clone(),
                e["page_amount"].clone(),
            )
        })
        .collect()
}

/// The one entry of `list_agents`.
fn entry(store: &mut MemoryStore) -> Value {
    let agents = coordinator(
        store,
        json!({"op": "list_agents", "workflow": null, "agent": null, "after": null}),
    );
    assert_eq!(
        agents["agents"].as_array().map(Vec::len),
        Some(1),
        "{agents}"
    );
    agents["agents"][0].clone()
}

/// U5.10-P (2), with the ruled row of a matched page: `agent_history` hands out, for each committed payment that a
/// `stopped` row of a page read names, that row's cause as `page_cause` (`amount_matched`, `amount_unread`,
/// `amount_differs` beside its `page_amount`, `page_unread`), and null for every other effect: a committed payment that
/// no row names, and a payment that a row names but that never stood committed.
#[test]
fn u510p_the_history_hands_out_the_page_cause_of_the_effect_that_its_row_names() {
    let mut store = started(1);
    for e in 1..=5 {
        committed(&mut store, 1, e);
    }
    effect_row(&mut store, 1, 6, 1, "held");
    log(
        &mut store,
        1,
        vec![
            line(
                1,
                2,
                ("stopped", Some(3), Some("amount_matched")),
                None,
                Some(1),
            ),
            line(
                1,
                3,
                ("stopped", Some(4), Some("amount_unread")),
                None,
                Some(2),
            ),
            line(
                1,
                4,
                ("stopped", Some(5), Some("amount_differs")),
                Some(1334),
                Some(3),
            ),
            line(
                1,
                5,
                ("stopped", Some(6), Some("page_unread")),
                None,
                Some(4),
            ),
            line(
                1,
                6,
                ("stopped", Some(7), Some("amount_matched")),
                None,
                Some(6),
            ),
        ],
    );
    end(&mut store, 1);
    assert_eq!(
        page_ends(&mut store),
        [
            (json!(effect(1)), json!("amount_matched"), Value::Null),
            (json!(effect(2)), json!("amount_unread"), Value::Null),
            (json!(effect(3)), json!("amount_differs"), json!(1334)),
            (json!(effect(4)), json!("page_unread"), Value::Null),
            (json!(effect(5)), Value::Null, Value::Null),
            (json!(effect(6)), Value::Null, Value::Null),
        ],
        "(effect, page_cause, page_amount)"
    );
}

/// U5.10-P (3): `list_agents` says `left` `yes` for an agent whose newest run committed a payment, and `no` for one
/// whose newest run held its payment back before any card (a `step_failed` row with `amount_differs` and no effect
/// row), although `last_cause` reads `amount_differs` there as it would behind a payment that left.
#[test]
fn u510p_list_agents_says_left_yes_behind_a_committed_payment_and_no_behind_one_held_back() {
    let mut paid = started(1);
    committed(&mut paid, 1, 1);
    log(
        &mut paid,
        1,
        vec![
            line(
                1,
                2,
                ("stopped", Some(3), Some("amount_differs")),
                Some(1334),
                Some(1),
            ),
            line(1, 3, ("stopped", None, Some("amount_differs")), None, None),
        ],
    );
    end(&mut paid, 1);
    let paid = entry(&mut paid);

    let mut held_back = started(1);
    log(
        &mut held_back,
        1,
        vec![
            line(
                1,
                2,
                ("step_failed", Some(3), Some("amount_differs")),
                None,
                None,
            ),
            line(1, 3, ("stopped", None, Some("amount_differs")), None, None),
        ],
    );
    end(&mut held_back, 1);
    let held_back = entry(&mut held_back);

    assert_eq!(
        [
            (paid["left"].clone(), paid["last_cause"].clone()),
            (held_back["left"].clone(), held_back["last_cause"].clone()),
        ],
        [
            (json!("yes"), json!("amount_differs")),
            (json!("no"), json!("amount_differs")),
        ],
        "(left, last_cause): the payment that left, then the one held back"
    );
}

/// U5.10-P (4), the store's own test of an old row beside a new one: a `stopped` row of a page read stored before the
/// key `effect` stood, with no such key, stays as it was stored, is read by both reads, ties to no effect and refuses
/// no read; the new row beside it ties to the effect that it names.
#[test]
fn u510p_an_old_stopped_row_with_no_effect_is_read_and_ties_to_no_effect() {
    let mut store = started(1);
    committed(&mut store, 1, 1);
    committed(&mut store, 1, 2);
    let old = line(
        1,
        2,
        ("stopped", Some(3), Some("amount_unread")),
        None,
        None,
    )
    .to_string();
    store
        .commit(vec![TypedRecord {
            kind: "agent_log",
            json: old.clone(),
        }])
        .expect("the old row is stored as an older build stored it");
    log(
        &mut store,
        1,
        vec![line(
            1,
            3,
            ("stopped", Some(4), Some("amount_unread")),
            None,
            Some(2),
        )],
    );
    end(&mut store, 1);
    let mut stored = Vec::new();
    store.for_each("agent_log", &mut |row| stored.push(row.to_owned()));
    assert!(
        stored.contains(&old),
        "the old row's text is kept: {stored:?}"
    );
    assert_eq!(
        page_ends(&mut store),
        [
            (json!(effect(1)), Value::Null, Value::Null),
            (json!(effect(2)), json!("amount_unread"), Value::Null),
        ],
        "the old row ties to no effect, the new one to its own"
    );
    let entry = entry(&mut store);
    assert_eq!(
        (entry["left"].clone(), entry["last_cause"].clone()),
        (json!("yes"), Value::Null),
        "the roster reads beside the old row"
    );
}
