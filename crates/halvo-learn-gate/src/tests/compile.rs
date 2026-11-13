use serde_json::Value;

use crate::case::read_dir;
use crate::store::{Family, MemoryStore};
use crate::stub::{id, Arm, Play};
use crate::tests::reader::{calendar_days_min, repo};

#[test]
fn the_compile_run_of_the_stub_stands_at_rev_1_and_at_rev_2_verified_with_no_base() {
    let cases =
        read_dir(&repo().join("fixtures/r5/cases/dev"), calendar_days_min()).expect("the cases");
    let case = cases
        .iter()
        .find(|c| c.name == "renamed-pay")
        .expect("a case");
    let mut play = Play::new(case, Arm::Admit, 1, MemoryStore::new());
    play.host.day = case.occurrences[0].day;
    play.compile().expect("the compile");
    let run = id('5', 1);
    let rows: Vec<Value> = play
        .host
        .store
        .rows(Family::Learn)
        .all()
        .filter(|(kind, _)| *kind == "learn_run")
        .map(|(_, json)| serde_json::from_str(json).expect("a row"))
        .filter(|row: &Value| row["run"] == run.as_str())
        .collect();
    let mut revs: Vec<u64> = rows.iter().filter_map(|r| r["rev"].as_u64()).collect();
    revs.sort_unstable();
    assert_eq!(revs, [1, 2], "{rows:?}");
    let second = rows.iter().find(|r| r["rev"] == 2).expect("rev 2");
    assert_eq!(second["mode"], "compile");
    assert_eq!(second["state"], "verified");
    assert_eq!(second["base"], Value::Null);
}
