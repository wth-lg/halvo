use std::path::{Path, PathBuf};

use r5_drift::case::read_case;
use r5_drift::targets;

const TARGETS: &str = include_str!("../../../../docs/r5/targets.toml");

fn refusals() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("refusals")
}

fn calendar_days_min() -> u64 {
    targets::parse(TARGETS)
        .expect("the targets file of the repository parses")
        .calendar_days_min
}

fn expected() -> Vec<(String, String)> {
    let text = std::fs::read_to_string(refusals().join("expected.txt")).expect("expected.txt");
    text.lines()
        .map(|line| {
            let rest = line
                .strip_prefix("REFUSED case: ")
                .expect("an expected line starts with REFUSED case: ");
            let (file, _) = rest.split_once(": ").expect("a file name and a rule");
            (file.to_owned(), line.to_owned())
        })
        .collect()
}

#[test]
fn every_refusal_file_is_refused_with_its_rule() {
    let mut wrong = Vec::new();
    for (file, want) in expected() {
        let bytes = std::fs::read(refusals().join(&file)).expect("a refusal file");
        let got = match read_case(&file, &bytes, calendar_days_min()) {
            Ok(_) => format!("accepted {file}"),
            Err(rule) => format!("REFUSED case: {file}: {rule}"),
        };
        if got != want {
            wrong.push(format!("want {want}\n got {got}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "{} of {} refusal files differ:\n{}",
        wrong.len(),
        expected().len(),
        wrong.join("\n")
    );
}

#[test]
fn every_file_of_the_refusal_directory_has_its_expected_line() {
    let mut files: Vec<String> = std::fs::read_dir(refusals())
        .expect("the refusal directory")
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .into_string()
                .expect("a name")
        })
        .filter(|name| name != "expected.txt")
        .collect();
    files.sort();
    let mut listed: Vec<String> = expected().into_iter().map(|(file, _)| file).collect();
    listed.sort();
    assert_eq!(files, listed);
}
