mod common;

use common::*;
use halvo_agent_store::AGENT_KINDS;
use halvo_schema::ErrorName;
use serde_json::json;

fn all_rows(store: &halvo_agent_store::MemoryStore) -> Vec<Vec<String>> {
    AGENT_KINDS
        .iter()
        .map(|(kind, _)| rows(store, kind))
        .collect()
}

/// The store answers the runner body `checkpoint` and the coordinator bodies `list_checkpoints` and
/// `mark_checkpoint` with `malformed` and stores nothing, as main's store at 873892f3 answers them, until the
/// handlers of U3.1's second pull request take these bodies.
#[test]
fn the_three_checkpoint_bodies_are_answered_malformed_and_store_nothing() {
    let mut store = store_with("spreadsheet_fill");
    let before = all_rows(&store);
    let record = json!({
        "v": 1, "workflow": wf(1), "agent": agent(1), "run": run(1), "seq": 1, "day": 20771,
        "at": 1794500000000000000i64, "folder": ["/data/books"], "workbook": ["/data/books/bills.xlsx"],
        "copy": [format!("bills.halvo-checkpoint-{}.xlsx", run(1))], "sha256": null, "bytes": null,
        "state": "planned", "left": null
    });
    let checkpoint = runner(
        &mut store,
        json!({"op": "checkpoint", "agent": agent(1), "run": run(1), "record": record}),
    );
    let list = coordinator(
        &mut store,
        json!({"op": "list_checkpoints", "workflow": null, "agent": null, "after": null}),
    );
    let mark = coordinator(
        &mut store,
        json!({
            "op": "mark_checkpoint", "workflow": wf(1), "agent": agent(1), "run": run(1),
            "workbook": ["/data/books/bills.xlsx"], "state": "gone", "left": null
        }),
    );
    println!("checkpoint={checkpoint:?} list={list:?} mark={mark:?}");
    assert_eq!(checkpoint, Err(ErrorName::Malformed));
    assert_eq!(list, Err(ErrorName::Malformed));
    assert_eq!(mark, Err(ErrorName::Malformed));
    assert_eq!(all_rows(&store), before);
}
