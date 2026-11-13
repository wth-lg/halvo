use crate::case::{read_dir, Case};
use crate::drift::{verdict, Gate};
use crate::stub::{play_case, Arm, Tally};
use crate::tests::admission::{admission, Line};
use crate::tests::reader::{calendar_days_min, repo};

const TARGETS: &str = include_str!("../../../../docs/r5/targets.toml");

fn dev_cases() -> Vec<Case> {
    read_dir(&repo().join("fixtures/r5/cases/dev"), calendar_days_min())
        .expect("the development cases")
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
