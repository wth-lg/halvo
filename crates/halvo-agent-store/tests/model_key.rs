//! r3 U7.6: the person's key for a model's host, one row of the kind `model_key` (the coordinator's log line 16422,
//! point 4). The keys of these tests are made-up texts that name themselves as test values.

mod common;

use common::*;
use halvo_agent_store::MemoryStore;
use halvo_schema::ErrorName;
use serde_json::{json, Value};

const KEY_A: &str = "test-value-not-a-model-key-a";
const KEY_B: &str = "test-value-not-a-model-key-b-of-another-length-0123456789";

/// The text of the reply to the coordinator body, or its error name.
fn reply(store: &mut MemoryStore, body: Value) -> Result<String, ErrorName> {
    halvo_agent_store::coordinator(store, NOW, body.to_string().as_bytes())
        .map(|r| r.to_json().expect("the reply's JSON"))
}

fn state(store: &mut MemoryStore) -> Result<String, ErrorName> {
    reply(
        store,
        json!({"op": "model_key_state", "workflow": null, "agent": null}),
    )
}

fn put(store: &mut MemoryStore, key: &str) -> Result<String, ErrorName> {
    reply(
        store,
        json!({"op": "put_model_key", "workflow": null, "agent": null, "key": key}),
    )
}

fn drop_key(store: &mut MemoryStore) -> Result<String, ErrorName> {
    reply(
        store,
        json!({"op": "drop_model_key", "workflow": null, "agent": null}),
    )
}

/// Sentence (1): the window is told that a key stands, or that none stands, and never the key. On a store with no
/// workflow, the state reads `no` before a key, `yes` after the write of a key and after the write of a second key
/// over it, and `no` after the drop. The replies of the state and of the write are the same bytes for two keys of
/// other texts and lengths, so no reply holds a part, a length or a sign of the key.
#[test]
fn the_window_reads_that_a_key_stands_and_never_the_key() {
    let mut store = MemoryStore::new();
    let no = Ok(r#"{"stands":"no"}"#.to_owned());
    let yes = Ok(r#"{"stands":"yes"}"#.to_owned());
    let stored = |n: u32| Ok(format!(r#"{{"stored":{n}}}"#));
    assert_eq!(state(&mut store), no, "the state before any key");
    let told: Vec<_> = [KEY_A, KEY_B]
        .into_iter()
        .map(|key| (put(&mut store, key), state(&mut store)))
        .collect();
    assert_eq!(
        told,
        vec![(stored(1), yes.clone()), (stored(1), yes)],
        "the replies of the write and of the state, for the key and for the key written over it"
    );
    assert_eq!(drop_key(&mut store), stored(0), "the reply of the drop");
    assert_eq!(state(&mut store), no, "the state after the drop");
}
