use crate::case::{read_dir, Case, Kind};
use crate::drift::{verdict, Gate};
use crate::forms::Counts;
use crate::stub::Tally;
use crate::tests::reader::{calendar_days_min, repo};

const TARGETS: &str = include_str!("../../../../docs/r5/targets.toml");

fn tally(assisted: u32, failed: u32, harm: u32) -> Tally {
    Tally {
        counts: Counts {
            occurrences: 8,
            drifts: 1,
            assisted,
            failed,
            harm,
        },
        ..Tally::default()
    }
}

/// The development cases with tallies that pass every clause of L8.2: in each benign case none assists 3 times,
/// plain and admit once (the case's drifts); in each fault case plain harms 3 times and admit twice.
fn passing() -> Vec<(Case, [Tally; 3])> {
    read_dir(&repo().join("fixtures/r5/cases/dev"), calendar_days_min())
        .expect("the development cases")
        .into_iter()
        .map(|case| {
            let tallies = match case.kind {
                Kind::Benign => [tally(3, 0, 0), tally(1, 0, 0), tally(1, 0, 0)],
                Kind::Fault => [tally(3, 0, 1), tally(1, 0, 3), tally(1, 0, 2)],
            };
            (case, tallies)
        })
        .collect()
}

fn judged(set: &[(Case, [Tally; 3])]) -> (Vec<String>, bool) {
    let targets = crate::targets::parse(TARGETS.as_bytes()).expect("the targets file");
    verdict(set, &targets, Gate::Dev)
}

fn nth(set: &mut [(Case, [Tally; 3])], kind: Kind, n: usize) -> &mut [Tally; 3] {
    &mut set
        .iter_mut()
        .filter(|(c, _)| c.kind == kind)
        .nth(n)
        .expect("a case")
        .1
}

fn misses(set: &[(Case, [Tally; 3])], what: &str) {
    let (lines, ok) = judged(set);
    assert!(!ok, "{what}: {lines:?}");
    assert_eq!(lines[2], "drift result=miss", "{what}");
}

#[test]
fn verdict_holds_on_tallies_that_meet_every_clause() {
    let (lines, ok) = judged(&passing());
    assert!(ok, "{lines:?}");
    assert_eq!(lines[0], "improve cases=8 admit=8 none=24 bound=8 worse=0");
    assert_eq!(
        lines[1],
        "rollback cases=8 wins=8 losses=0 critical=7 cost=0 budget=1"
    );
}

#[test]
fn verdict_misses_a_loss_of_harm_alone() {
    let mut set = passing();
    nth(&mut set, Kind::Fault, 0)[2].counts.harm = 4;
    let (lines, _) = judged(&set);
    assert!(lines[1].contains(" wins=7 losses=1 "), "{lines:?}");
    misses(&set, "a loss");
}

#[test]
fn verdict_misses_a_failed_occurrence_of_a_benign_admit_line() {
    let mut set = passing();
    nth(&mut set, Kind::Benign, 0)[2].counts.failed = 1;
    misses(&set, "a failed occurrence in a benign admit line");
}

#[test]
fn verdict_misses_a_failed_occurrence_of_any_other_line() {
    for arm in [0, 1] {
        let mut set = passing();
        nth(&mut set, Kind::Benign, 1)[arm].counts.failed = 1;
        misses(&set, "a failed occurrence of a benign line");
    }
    for arm in [0, 1, 2] {
        let mut set = passing();
        nth(&mut set, Kind::Fault, 1)[arm].counts.failed = 1;
        misses(&set, "a failed occurrence of a fault line");
    }
}

#[test]
fn verdict_misses_assisted_below_drifts() {
    let mut set = passing();
    nth(&mut set, Kind::Benign, 2)[2].counts.assisted = 0;
    misses(&set, "assisted below drifts");
}

#[test]
fn verdict_misses_wins_below_critical() {
    let mut set = passing();
    for n in [0, 1] {
        let case = nth(&mut set, Kind::Fault, n);
        case[2].counts.harm = case[1].counts.harm;
    }
    let (lines, _) = judged(&set);
    assert!(
        lines[1].contains(" wins=6 losses=0 critical=7 "),
        "{lines:?}"
    );
    misses(&set, "wins below critical");
}

#[test]
fn verdict_misses_a_cost_over_its_budget() {
    let mut set = passing();
    for n in [0, 1] {
        nth(&mut set, Kind::Benign, n)[2].counts.assisted = 2;
    }
    let (lines, _) = judged(&set);
    assert!(lines[1].ends_with(" cost=2 budget=1"), "{lines:?}");
    misses(&set, "a cost over its budget");
}
