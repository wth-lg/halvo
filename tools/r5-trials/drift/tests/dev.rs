use std::path::{Path, PathBuf};

use r5_drift::case::{read_case, Case};
use r5_drift::model::{run, Arm, Sort};
use r5_drift::targets;
use r5_drift::trial::{self, line};
use r5_drift::Refusal;

const TARGETS: &str = include_str!("../../../../docs/r5/targets.toml");
const CASES_MD: &str = include_str!("../../../../docs/r5/cases.md");

fn dev() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../fixtures/r5/cases/dev")
}

fn calendar_days_min() -> u64 {
    targets::parse(TARGETS)
        .expect("the targets file parses")
        .calendar_days_min
}

fn dev_case(name: &str) -> Case {
    let file = format!("{name}.jsonl");
    let bytes = std::fs::read(dev().join(&file)).expect("a development case");
    read_case(&file, &bytes, calendar_days_min()).expect("the reader accepts it")
}

fn lines(case: &Case) -> String {
    Arm::TRIAL
        .iter()
        .map(|arm| line(case, *arm, &run(case, *arm)))
        .collect()
}

fn between<'a>(text: &'a str, open: &str, close: &str) -> &'a str {
    let start = text.find(open).expect("the opening fence") + open.len();
    let end = text[start..].find(close).expect("the closing fence");
    &text[start..start + end]
}

#[test]
fn the_reader_accepts_every_development_case() {
    let mut names: Vec<String> = std::fs::read_dir(dev())
        .expect("the development directory")
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .into_string()
                .expect("a name")
        })
        .collect();
    names.sort();
    assert_eq!(names.len(), 16);
    for name in names {
        dev_case(name.strip_suffix(".jsonl").expect("a .jsonl file"));
    }
}

fn dev_without(file: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("dev-without-{file}"));
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    for entry in std::fs::read_dir(dev()).expect("the development directory") {
        let name = entry.expect("an entry").file_name();
        if name != file {
            std::fs::copy(dev().join(&name), dir.join(&name)).expect("a copy");
        }
    }
    dir
}

#[test]
fn a_directory_with_unequal_benign_and_fault_counts_is_refused_with_case() {
    let targets = targets::parse(TARGETS).expect("the targets file parses");
    assert_eq!(
        trial::trial(&dev_without("reused-pay-2.jsonl"), &targets),
        Err(Refusal::Case(
            "the directory holds 8 benign and 7 fault cases, not 8 and 8".to_owned()
        ))
    );
    assert_eq!(
        trial::trial(&dev_without("renamed-pay.jsonl"), &targets),
        Err(Refusal::Case(
            "the directory holds 7 benign and 8 fault cases, not 8 and 8".to_owned()
        ))
    );
}

#[test]
fn renamed_pay_gives_the_lines_counted_by_hand() {
    let head = "case name=renamed-pay kind=benign form=renamed";
    assert_eq!(
        lines(&dev_case("renamed-pay")),
        format!(
            "{head} arm=none sort=- occurrences=8 drifts=1 assisted=6 failed=0 harm=0 reverted_good=0\n\
             {head} arm=plain sort=- occurrences=8 drifts=1 assisted=1 failed=0 harm=0 reverted_good=0\n\
             {head} arm=admit sort=yes occurrences=8 drifts=1 assisted=1 failed=0 harm=0 reverted_good=0\n\
             {head} arm=admit sort=no occurrences=8 drifts=1 assisted=1 failed=0 harm=0 reverted_good=0\n"
        )
    );
}

#[test]
fn decoy_pay_1_gives_the_lines_counted_by_hand() {
    let head = "case name=decoy-pay-1 kind=fault form=decoy";
    assert_eq!(
        lines(&dev_case("decoy-pay-1")),
        format!(
            "{head} arm=none sort=- occurrences=8 drifts=2 assisted=6 failed=0 harm=1 reverted_good=0\n\
             {head} arm=plain sort=- occurrences=8 drifts=2 assisted=3 failed=0 harm=3 reverted_good=0\n\
             {head} arm=admit sort=yes occurrences=8 drifts=2 assisted=3 failed=0 harm=2 reverted_good=1\n\
             {head} arm=admit sort=no occurrences=8 drifts=2 assisted=3 failed=0 harm=2 reverted_good=1\n"
        )
    );
}

#[test]
fn a_timeout_reverts_the_edit_of_a_flaky_case_only_when_every_failure_counts() {
    let case = dev_case("flaky-amount");
    let yes = run(&case, Arm::Admit(Sort::Yes));
    let no = run(&case, Arm::Admit(Sort::No));
    let plain = run(&case, Arm::Plain);
    assert_eq!((yes.assisted, yes.reverted_good), (plain.assisted, 0));
    assert_eq!(no.reverted_good, 1);
    assert!(no.assisted >= plain.assisted + 2);
}

#[test]
fn the_worked_example_of_cases_md_is_accepted_and_prints_its_two_lines() {
    let example = between(CASES_MD, "```jsonl\n", "```\n");
    let case = read_case("example.jsonl", example.as_bytes(), calendar_days_min())
        .expect("the reader accepts the worked example");
    let printed = between(CASES_MD, "```text\n", "```\n");
    let expected: String = [Arm::None, Arm::Plain]
        .iter()
        .map(|arm| line(&case, *arm, &run(&case, *arm)))
        .collect();
    assert_eq!(printed, expected);
}
