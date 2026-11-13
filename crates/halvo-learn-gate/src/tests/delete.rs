use serde_json::Value;

use crate::delete::{seed, seed_case, MARKERS};
use crate::store::Family;
use crate::stub::id;

#[test]
fn delete_seeds_each_marker_in_an_active_and_a_reverted_row_of_its_workflow() {
    let cases = MARKERS.map(seed_case);
    let store = seed(&cases).expect("the seeded store").host.store;
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
