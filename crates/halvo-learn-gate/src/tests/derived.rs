use std::path::PathBuf;

use halvo_schema::executable::names::LocatorBy;
use halvo_schema::executable::step::Target;
use halvo_schema::executable::ExecutableStep;

use crate::delete::{seed_case, MARKERS, SEED_DAY};
use crate::derived::{process_key, DerivedHost};
use crate::host::PlayHost;
use crate::stub::{play_all, Arm, Play};

fn scratch(name: &str) -> PathBuf {
    let exe = std::env::current_exe().expect("the test binary");
    let root = exe
        .ancestors()
        .nth(3)
        .expect("the target directory")
        .join("tmp")
        .join("r5-derived-gate");
    halvo_test_dir::make(&root, name).expect("a test directory")
}

fn first_role_name(steps: &[ExecutableStep]) -> Option<String> {
    match &steps.first()?.target {
        Some(Target::Web(target)) => target
            .locators
            .first()
            .filter(|locator| locator.by == LocatorBy::Role)
            .and_then(|locator| locator.name.as_ref())
            .map(|name| name.as_str().to_owned()),
        _ => None,
    }
}

fn last_day() -> u32 {
    let case = seed_case(MARKERS[0], SEED_DAY);
    case.occurrences[case.occurrences.len() - 1].day
}

#[test]
fn derived_a_learned_edit_is_read_back_and_served_after_the_host_and_the_store_restart() {
    let case = seed_case(MARKERS[0], SEED_DAY);
    let host = DerivedHost::new(scratch("restart"), process_key(), SEED_DAY);
    let mut play = Play::with_host(&case, Arm::Admit, 1, host);
    play_all(&mut play, false).expect("the seeded case plays on the derived store");
    let edits = play.host.learn_rows("learn_edit").expect("a read");
    assert!(
        edits.iter().any(|row| row.contains("\"active\"")),
        "{edits:?}"
    );
    play.restart().expect("a restart of the host and the store");
    assert_eq!(
        play.host
            .learn_rows("learn_edit")
            .expect("a read after the restart"),
        edits
    );
    let served = play.served_now().expect("a run after the restart");
    assert_eq!(first_role_name(&served).as_deref(), Some(MARKERS[0]));
}

#[test]
fn derived_the_store_s_files_hold_neither_the_sqlite_header_nor_a_learned_label() {
    let root = scratch("cipher");
    let case = seed_case(MARKERS[0], SEED_DAY);
    {
        let host = DerivedHost::new(root.clone(), process_key(), SEED_DAY);
        let mut play = Play::with_host(&case, Arm::Admit, 1, host);
        play_all(&mut play, false).expect("the seeded case plays on the derived store");
        let edits = play.host.learn_rows("learn_edit").expect("a read");
        assert!(
            edits.iter().any(|row| row.contains(MARKERS[0])),
            "{edits:?}"
        );
    }
    let database =
        std::fs::read(root.join("derived").join("derived.db")).expect("the database file");
    assert!(database.len() >= 4_096, "{}", database.len());
    for entry in std::fs::read_dir(root.join("derived")).expect("the store's directory") {
        let bytes = std::fs::read(entry.expect("an entry").path()).expect("a file of the store");
        assert!(!bytes.starts_with(b"SQLite format 3\0"));
        assert!(!bytes
            .windows(MARKERS[0].len())
            .any(|window| window == MARKERS[0].as_bytes()));
    }
}

#[test]
fn derived_the_store_opens_again_with_its_key_and_refuses_another_key() {
    let root = scratch("key");
    let key = process_key();
    let case = seed_case(MARKERS[0], SEED_DAY);
    let edits = {
        let mut play = Play::with_host(
            &case,
            Arm::Admit,
            1,
            DerivedHost::new(root.clone(), key.clone(), SEED_DAY),
        );
        play_all(&mut play, false).expect("the seeded case plays on the derived store");
        play.host.learn_rows("learn_edit").expect("a read")
    };
    let mut again = DerivedHost::new(root.clone(), key, last_day());
    assert_eq!(
        again
            .learn_rows("learn_edit")
            .expect("an open with the store's key"),
        edits
    );
    drop(again);
    let mut other = DerivedHost::new(root, process_key(), last_day());
    let refused = other
        .learn_rows("learn_edit")
        .expect_err("an open with another key");
    assert_eq!(
        refused,
        "the derived store does not open: the key does not open the derived database"
    );
}
