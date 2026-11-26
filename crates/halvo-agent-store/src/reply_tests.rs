//! r3 U7.9: the text of a reply that a writer model wrote is kept beside its reply row, as a row of
//! `agent_reply_body`, and `load_reply_body` hands it out only while its draft waits. A unit test of the library,
//! which the merge gate runs in both jobs, over the store in memory, with made-up values.

use halvo_schema::executable::{OwnValue, KINDS};
use halvo_schema::{ErrorName, Instant, RetentionClass, RowRead, Workflow};
use serde_json::{json, Value};

use crate::{MemoryStore, Now};

/// The instant of every request of this test, 2026-11-12T16:13:20Z, as in the store's other tests.
const NOW: Now = Now {
    at: Instant(1_794_500_000_000_000_000),
};

/// The instant and the day in UTC of the reply rows, 2026-11-11T12:26:40Z.
const ROW_AT: i64 = 1_794_400_000_000_000_000;
const ROW_DAY: u32 = 20_768;

/// The made-up text of a writer model.
const TEXT: &str = "Friday at ten suits me. Thank you for asking.";

fn id(prefix: char, n: u32) -> String {
    format!("{prefix}{n:031x}")
}

fn workflow(n: u32) -> Workflow {
    serde_json::from_value(json!({
        "v": 1,
        "id": id('3', n),
        "state": "approved",
        "category": "email",
        "site": null,
        "app": "com.example.browser",
        "trigger": "schedule",
        "recurrence": {"count": 3, "period": 31, "first": 20705, "last": 20767},
        "confidence": 87,
        "agent": null,
    }))
    .expect("a workflow record")
}

/// The store with the agents 1 and 2, each of its own workflow, each with its run started (a compile, which needs no
/// stored playbook; the rows of a reply do not read the run's mode).
fn two_agents() -> MemoryStore {
    let mut store = MemoryStore::new();
    for n in [1, 2] {
        store.put_workflow(workflow(n));
        for body in [
            json!({"op": "create_agent", "workflow": id('3', n), "agent": id('4', n)}),
            json!({"op": "start_run", "workflow": id('3', n), "agent": id('4', n), "run": id('5', n), "mode": "compile"}),
        ] {
            crate::coordinator(&mut store, NOW, body.to_string().as_bytes())
                .unwrap_or_else(|e| panic!("{body}: {e:?}"));
        }
    }
    store
}

/// The runner body `body` of the agent 1's run, answered as JSON or refused.
fn runner(store: &mut MemoryStore, body: Value) -> Result<Value, ErrorName> {
    let reply = crate::runner(store, NOW, body.to_string().as_bytes())?;
    Ok(serde_json::from_str(&reply.to_json().expect("the reply's JSON")).expect("JSON"))
}

/// A `reply_row` body of the agent 1 for the message `message`, with `body` where given.
fn row(seq: u32, message: &str, state: &str, body: Option<&str>) -> Value {
    let (draft, effect) = match state {
        "lost" => (None, None),
        "begun" | "committed" | "unknown" => (Some("d1"), Some(id('6', 1))),
        _ => (Some("d1"), None),
    };
    let record = json!({
        "v": 1, "workflow": id('3', 1), "agent": id('4', 1), "run": id('5', 1), "seq": seq,
        "day": ROW_DAY, "at": ROW_AT, "message": message, "draft": draft, "effect": effect,
        "state": state,
    });
    let mut put =
        json!({"op": "reply_row", "agent": id('4', 1), "run": id('5', 1), "record": record});
    if let Some(body) = body {
        put["body"] = json!(OwnValue::new(body).pieces());
    }
    put
}

/// What `load_reply_body` of the agent `a`'s run answers for the draft `d1` of `message`.
fn kept(store: &mut MemoryStore, a: u32, message: &str) -> Value {
    let body = json!({"op": "load_reply_body", "agent": id('4', a), "run": id('5', a), "message": message, "draft": "d1"});
    runner(store, body).expect("load_reply_body answers")["kept"].clone()
}

fn kept_rows(store: &MemoryStore) -> usize {
    let mut count = 0;
    store.for_each("agent_reply_body", &mut |_| count += 1);
    count
}

#[test]
fn u79_a_kept_reply_text_reads_while_its_draft_waits_and_no_read_hands_it_out_after_its_reply_moves_on(
) {
    let entry = KINDS
        .iter()
        .find(|k| k.name == "agent_reply_body")
        .expect("the kind's entry");
    assert_eq!(entry.class, RetentionClass::Bounded { days: 35 });
    assert_eq!(entry.workflow_key, Some("workflow"));

    let mut store = two_agents();
    runner(&mut store, row(1, "m1", "drafted", Some(TEXT))).expect("a first drafted row");
    assert_eq!(
        kept_rows(&store),
        1,
        "the text is stored in the commit of the row"
    );
    assert_eq!(kept(&mut store, 1, "m1"), json!([TEXT]), "the draft waits");
    assert_eq!(
        kept(&mut store, 2, "m1"),
        Value::Null,
        "another agent's read"
    );
    runner(&mut store, row(2, "m1", "begun", None)).expect("begun");
    assert_eq!(kept(&mut store, 1, "m1"), Value::Null, "after begun");
    runner(&mut store, row(3, "m1", "committed", None)).expect("committed");
    assert_eq!(kept(&mut store, 1, "m1"), Value::Null, "after committed");

    runner(&mut store, row(1, "m2", "drafted", Some(TEXT))).expect("a first drafted row");
    assert_eq!(kept(&mut store, 1, "m2"), json!([TEXT]), "the draft waits");
    runner(&mut store, row(2, "m2", "lost", None)).expect("lost");
    assert_eq!(kept(&mut store, 1, "m2"), Value::Null, "after lost");
    assert_eq!(kept_rows(&store), 2, "the rows stay until their days pass");

    let beside_begun = row(1, "m3", "begun", Some(TEXT));
    assert!(
        runner(&mut store, beside_begun).is_err(),
        "a text beside begun"
    );
    runner(&mut store, row(1, "m4", "drafted", None)).expect("a first drafted row");
    assert!(
        runner(&mut store, row(2, "m4", "begun", Some(TEXT))).is_err(),
        "a text beside begun after drafted"
    );
    let over = "x".repeat(3_100);
    assert!(
        runner(&mut store, row(1, "m3", "drafted", Some(&over))).is_err(),
        "a text over 3,072 bytes of JSON"
    );
    assert_eq!(kept_rows(&store), 2, "a refused body stores nothing");
    assert_eq!(kept(&mut store, 1, "m3"), Value::Null);
    assert_eq!(kept(&mut store, 1, "m4"), Value::Null);
}
