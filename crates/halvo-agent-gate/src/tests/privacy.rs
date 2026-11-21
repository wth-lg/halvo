//! The two deletion tests of r3 U8.3 (contracts.md A14.4, A7.3), on the derived store opened with a key under the
//! owner's step list: the lines `agent_deleted` and `agent_scrub` of `agent-gate privacy`. Each makes its own store
//! under `<target>/tmp/r3-privacy/` and leaves it there. A failure names kinds and marker numbers, never a row's text.

use std::path::PathBuf;

use crate::privacy::{deleted, holds, naming, play, process_key, scrub, Delete, Row, MARKERS};

fn played(name: &str) -> Delete {
    let exe = std::env::current_exe().expect("the test binary");
    let root = exe
        .ancestors()
        .nth(3)
        .expect("the target directory")
        .join("tmp")
        .join("r3-privacy");
    let dir: PathBuf = halvo_test_dir::make(&root, name).expect("a test directory");
    play(&dir, &process_key().expect("a key")).expect("the play")
}

fn kinds(rows: &[&Row]) -> Vec<String> {
    rows.iter().map(|row| row.0.clone()).collect()
}

/// S1 (A14.4 `agent_deleted`; A7.3): after the decision `delete` of the first workflow no row of the family `agent`
/// names it, and every row of that family of the second workflow stored before the delete still stands, both read
/// through the keyed store.
#[test]
fn agent_deleted_leaves_the_other_workflow() {
    let delete = played("agent-deleted");
    assert!(delete.stored, "the first workflow was not stored");
    assert!(!naming(&delete.before, &delete.first).is_empty());
    assert_eq!(
        kinds(&naming(&delete.after, &delete.first)),
        Vec::<String>::new(),
        "rows of the deleted workflow stand"
    );
    let kept = naming(&delete.before, &delete.second);
    assert!(!kept.is_empty());
    for row in &kept {
        assert!(
            delete.after.contains(row),
            "a row of the other workflow is gone: {}",
            row.0
        );
    }
    assert!(deleted(&delete));
}

/// S2 (A14.4 `agent_scrub`): the three markers, stored in rows of the first workflow, are in no row that the keyed
/// store hands out after the delete, and they went by the one call of `Store::decide`: its delete guard read them
/// under the workflow inside the delete's own transaction, and the call committed one transaction, which raised the
/// family's revision. A removal by another path makes it fail.
#[test]
fn agent_scrub_goes_through_the_store_s_delete() {
    let delete = played("agent-scrub");
    let first = naming(&delete.before, &delete.first);
    for (n, marker) in MARKERS.iter().enumerate() {
        assert!(
            first.iter().any(|row| holds(row, marker)),
            "marker {n} was not stored"
        );
    }
    assert_eq!(delete.guarded.len(), 1, "the delete guard's calls");
    assert_eq!(delete.guarded[0].0, delete.first);
    for (n, marker) in MARKERS.iter().enumerate() {
        assert!(
            delete.guarded[0].1.iter().any(|row| holds(row, marker)),
            "marker {n} was gone before the delete's transaction"
        );
    }
    assert_eq!(delete.reports, 1, "transactions of the delete call");
    assert!(delete.family_rose);
    for (n, marker) in MARKERS.iter().enumerate() {
        assert!(
            !delete.after.iter().any(|row| holds(row, marker)),
            "marker {n} stands"
        );
    }
    assert!(scrub(&delete));
}
