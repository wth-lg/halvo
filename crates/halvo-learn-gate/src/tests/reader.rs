use std::path::{Path, PathBuf};

use crate::case::{read_case, read_dir, Kind};
use crate::forms::{play, ReaderArm};
use crate::tests::admission::{admission, Line};

const TARGETS: &str = include_str!("../../../../docs/r5/targets.toml");

pub fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

pub fn calendar_days_min() -> u64 {
    crate::targets::parse(TARGETS.as_bytes())
        .expect("the targets file parses")
        .get("gate", "calendar_days_min")
}

fn trial_fixtures() -> PathBuf {
    repo().join("tools/r5-trials/drift/tests")
}

#[test]
fn reader_accepts_every_development_case() {
    let cases = read_dir(&repo().join("fixtures/r5/cases/dev"), calendar_days_min())
        .expect("the development cases are read");
    assert_eq!(cases.len(), 16);
    let benign = cases.iter().filter(|c| c.kind == Kind::Benign).count();
    assert_eq!((benign, cases.len() - benign), (8, 8));
    let mut names: Vec<&str> = cases.iter().map(|c| c.name.as_str()).collect();
    let read_order = names.clone();
    names.sort_unstable();
    assert_eq!(
        read_order, names,
        "the cases come in byte order of their names"
    );
}

#[test]
fn reader_refuses_every_file_that_the_trial_refuses() {
    let dir = trial_fixtures().join("refusals");
    let expected = std::fs::read_to_string(dir.join("expected.txt")).expect("expected.txt");
    let mut accepted = Vec::new();
    let mut files = 0;
    for line in expected.lines() {
        let rest = line.strip_prefix("REFUSED case: ").expect("a refusal line");
        let (file, _) = rest.split_once(": ").expect("a file name and a rule");
        let bytes = std::fs::read(dir.join(file)).expect("a refusal file");
        files += 1;
        if read_case(file, &bytes, calendar_days_min()).is_ok() {
            accepted.push(file.to_owned());
        }
    }
    assert_eq!(files, 132);
    assert!(accepted.is_empty(), "accepted: {accepted:?}");
}

#[test]
fn reader_accepts_the_case_at_each_upper_limit() {
    let file = "upper-limits-name-of-32-bytes-ok.jsonl";
    let bytes = std::fs::read(trial_fixtures().join("limits").join(file)).expect("the limits file");
    let case = read_case(file, &bytes, calendar_days_min());
    assert!(case.is_ok(), "{case:?}");
}

#[test]
fn reader_arms_give_the_integers_of_the_trial() {
    let cases = read_dir(&repo().join("fixtures/r5/cases/dev"), calendar_days_min())
        .expect("the development cases are read");
    let lines = admission();
    let mut compared = 0;
    for case in &cases {
        for (arm, name) in [(ReaderArm::None, "none"), (ReaderArm::Plain, "plain")] {
            let want = lines
                .iter()
                .find(|l| l.case == case.name && l.arm == name)
                .expect("a line of admission.md");
            let got = play(case, arm);
            let got = Line {
                case: case.name.clone(),
                arm: name.to_owned(),
                sort: "-".to_owned(),
                occurrences: got.occurrences,
                drifts: got.drifts,
                assisted: got.assisted,
                failed: got.failed,
                harm: got.harm,
                reverted_good: 0,
            };
            assert_eq!(&got, want);
            compared += 1;
        }
    }
    assert_eq!(compared, 32);
}

#[test]
fn reader_answers_the_unit_s_own_files_as_the_trial_does() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("cases");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .expect("the unit's case files")
        .map(|e| {
            e.expect("an entry")
                .file_name()
                .into_string()
                .expect("a name")
        })
        .collect();
    names.sort();
    assert_eq!(names.len(), 6);
    for name in names {
        let bytes = std::fs::read(dir.join(&name)).expect("a case file");
        let answer = read_case(&name, &bytes, calendar_days_min());
        match name.split('-').next() {
            Some("accepted") => assert!(answer.is_ok(), "{name}: {answer:?}"),
            Some("refused") => assert!(answer.is_err(), "{name} was accepted"),
            _ => panic!("{name} names no answer"),
        }
    }
}
