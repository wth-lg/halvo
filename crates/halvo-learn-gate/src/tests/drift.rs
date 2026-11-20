use std::path::PathBuf;

use crate::case::{read_dir, Case};
use crate::cli::{Cause, Refusal};
use crate::derived::{process_key, DerivedHost, Scratch};
use crate::drift::{played, read_back, verdict, Gate, Plays};
use crate::host::PlayHost;
use crate::stub::{play_case, Arm, Tally};
use crate::tests::admission::{admission, Line};
use crate::tests::reader::{calendar_days_min, repo};

const TARGETS: &str = include_str!("../../../../docs/r5/targets.toml");

fn dev_cases() -> Vec<Case> {
    read_dir(&repo().join("fixtures/r5/cases/dev"), calendar_days_min())
        .expect("the development cases")
}

fn scratch(name: &str) -> PathBuf {
    let exe = std::env::current_exe().expect("the test binary");
    let root = exe
        .ancestors()
        .nth(3)
        .expect("the target directory")
        .join("tmp")
        .join("r5-derived-drift");
    halvo_test_dir::make(&root, name).expect("a test directory")
}

fn line(case: &Case, arm: Arm, tally: &Tally) -> Line {
    let c = &tally.counts;
    Line {
        case: case.name.clone(),
        arm: arm.as_str().to_owned(),
        sort: if arm == Arm::Admit { "yes" } else { "-" }.to_owned(),
        occurrences: c.occurrences,
        drifts: c.drifts,
        assisted: c.assisted,
        failed: c.failed,
        harm: c.harm,
        reverted_good: tally.reverted_good,
    }
}

#[test]
fn drift_gives_the_integers_of_the_admission_trial() {
    let lines = admission();
    let mut differ = Vec::new();
    let mut compared = 0;
    for case in dev_cases() {
        for arm in Arm::ALL {
            let tally = play_case(&case, arm, false).expect("every occurrence completes");
            let got = line(&case, arm, &tally);
            let want = lines
                .iter()
                .find(|l| l.case == got.case && l.arm == got.arm && l.sort == got.sort)
                .expect("a line of admission.md");
            if &got != want {
                differ.push(format!("want {want:?}\n got {got:?}"));
            }
            compared += 1;
        }
    }
    assert_eq!(compared, 48);
    assert!(
        differ.is_empty(),
        "{} of 48 lines differ:\n{}",
        differ.len(),
        differ.join("\n")
    );
}

#[test]
fn drift_restart_all_changes_expired_runs_alone() {
    for case in dev_cases() {
        for arm in Arm::ALL {
            let once = play_case(&case, arm, false).expect("a play");
            let every =
                play_case(&case, arm, true).expect("a play with every occurrence restarted");
            let without = |t: Tally| Tally {
                expired_runs: 0,
                ..t
            };
            assert_eq!(
                without(once),
                without(every),
                "{} {}",
                case.name,
                arm.as_str()
            );
            match arm {
                Arm::Admit => {
                    assert!(once.expired_runs > 0, "{}: no run expired", case.name);
                    assert!(once.edit_rows > 0, "{}: no edit stored", case.name);
                    assert!(every.expired_runs > once.expired_runs, "{}", case.name);
                }
                Arm::None | Arm::Plain => assert_eq!((once.expired_runs, once.edit_rows), (0, 0)),
            }
        }
    }
}

#[test]
fn drift_gate_of_the_development_cases_is_ok() {
    let targets = crate::targets::parse(TARGETS.as_bytes()).expect("the targets file");
    let played: Vec<(Case, [Tally; 3])> = dev_cases()
        .into_iter()
        .map(|case| {
            let tallies = Arm::ALL.map(|arm| play_case(&case, arm, false).expect("a play"));
            (case, tallies)
        })
        .collect();
    let (lines, ok) = verdict(&played, &targets, Gate::Dev);
    assert!(ok, "{lines:?}");
    assert_eq!(lines[2], "drift result=ok");
    assert!(
        lines[1].starts_with("rollback cases=8 wins=8 losses=0 critical=7 cost=0 budget=1"),
        "{}",
        lines[1]
    );
}

#[test]
fn drift_derived_plays_a_case_on_a_store_file_in_the_play_s_own_directory() {
    let case = dev_cases().remove(0);
    let scratch =
        Scratch::make_in(&scratch("one-play")).expect("the command's temporary directory");
    let dir = scratch.path().join("play-1");
    let store = dir.join("derived").join("derived.db");
    let key = process_key();
    let plays = Plays::Derived {
        scratch,
        key: key.clone(),
    };
    assert!(!store.exists(), "no play has run yet");
    let tally = plays
        .play(&case, Arm::Admit, false)
        .expect("every occurrence completes");
    assert!(
        store.is_file(),
        "the play of {} in the arm admit left no file derived/derived.db in its directory play-1",
        case.name
    );
    let last = case.occurrences[case.occurrences.len() - 1].day;
    let mut again = DerivedHost::new(dir, key, last);
    let mut learn_rows = 0;
    let mut edit_rows = 0;
    for entry in halvo_schema::learning::KINDS {
        let rows = again
            .learn_rows(entry.name)
            .expect("a read of the play's own store")
            .len();
        learn_rows += rows;
        if entry.name == "learn_edit" {
            edit_rows = rows;
        }
    }
    assert!(
        tally.edit_rows > 0,
        "the play of {} in the arm admit stored no edit",
        case.name
    );
    assert_eq!(
        edit_rows, tally.edit_rows as usize,
        "the learn_edit rows of the play's own store, opened again with its key, against the tally's"
    );
    assert!(
        learn_rows > 0,
        "the play's own store holds no row of the family learn"
    );
}

#[test]
fn drift_read_back_refuses_a_play_s_store_that_does_not_hold_the_tally_s_edits() {
    let case = dev_cases().remove(0);
    let last = case.occurrences[case.occurrences.len() - 1].day;
    let key = process_key();
    let cause = |read: Result<(), Refusal>| read.err().map(|refusal| refusal.cause);
    let none = scratch("read-back-none");
    assert_eq!(
        cause(read_back(&none, &key, last, 0)),
        Some(Cause::Store),
        "no store file under the directory"
    );
    let empty = scratch("read-back-empty");
    DerivedHost::new(empty.clone(), key.clone(), last)
        .learn_rows("learn_edit")
        .expect("a new store with no rows");
    assert!(empty.join("derived").join("derived.db").is_file());
    assert_eq!(
        cause(read_back(&empty, &key, last, 3)),
        Some(Cause::Store),
        "a store with no rows against a tally with rows"
    );
    let scratch =
        Scratch::make_in(&scratch("read-back-play")).expect("the command's temporary directory");
    let dir = scratch.path().join("play-1");
    let plays = Plays::Derived {
        scratch,
        key: key.clone(),
    };
    let mut n = 0;
    let tally = played(&plays, &case, Arm::Admit, false, &mut n)
        .expect("a play whose own store holds its edits");
    assert_eq!(n, 1);
    assert!(tally.edit_rows > 0);
    assert_eq!(read_back(&dir, &key, last, tally.edit_rows), Ok(()));
    assert_eq!(
        cause(read_back(&dir, &key, last, tally.edit_rows + 1)),
        Some(Cause::Store),
        "a store whose rows are not the tally's"
    );
    assert_eq!(
        cause(read_back(&dir, &process_key(), last, tally.edit_rows)),
        Some(Cause::Store),
        "the play's store opened with another key"
    );
    let mut later = 5;
    assert_eq!(
        played(&plays, &case, Arm::Admit, false, &mut later)
            .err()
            .map(|refusal| refusal.cause),
        Some(Cause::Store),
        "a count that names no play of the command"
    );
}
