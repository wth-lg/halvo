use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::create_dir_all(&dir).expect("a test directory");
    dir
}

fn gate(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_learn-gate"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("learn-gate runs")
}

fn stdout(out: &Output) -> Vec<String> {
    String::from_utf8(out.stdout.clone())
        .expect("UTF-8")
        .lines()
        .map(str::to_owned)
        .collect()
}

fn refused(out: &Output, cause: &str) {
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert!(out.stdout.is_empty(), "{out:?}");
    let err = String::from_utf8(out.stderr.clone()).expect("UTF-8");
    assert_eq!(err.lines().count(), 1, "{err}");
    assert!(err.starts_with(&format!("REFUSED {cause}: ")), "{err}");
}

fn copy_case(dir: &Path, name: &str) {
    let from = repo().join("fixtures/r5/cases/dev").join(name);
    std::fs::copy(from, dir.join(name)).expect("a copy of a case");
}

#[test]
fn drift_gate_dev_prints_every_case_line_and_the_verdict() {
    let out = gate(
        &[
            "drift",
            "--gate",
            "dev",
            "--cases",
            "fixtures/r5/cases/dev",
            "--store",
            "memory",
        ],
        &repo(),
    );
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let lines = stdout(&out);
    assert_eq!(lines.len(), 51);
    for line in &lines[..48] {
        assert!(line.starts_with("case name="), "{line}");
        assert!(line.contains(" store=memory "), "{line}");
        assert_eq!(line.split(' ').count(), 14, "{line}");
    }
    assert!(
        lines[0].starts_with("case name=alternates-checkout kind=benign form=alternates arm=none ")
    );
    assert!(lines[1].contains(" arm=plain "));
    assert!(lines[2].contains(" arm=admit "));
    assert_eq!(
        lines[48],
        "improve cases=8 admit=10 none=44 bound=10 worse=0"
    );
    assert_eq!(
        lines[49],
        "rollback cases=8 wins=8 losses=0 critical=7 cost=0 budget=1"
    );
    assert_eq!(lines[50], "drift result=ok");
}

#[test]
fn drift_arm_prints_one_line_per_case() {
    let dir = scratch("drift-arm");
    copy_case(&dir, "decoy-pay-1.jsonl");
    copy_case(&dir, "renamed-pay.jsonl");
    let out = gate(
        &[
            "drift",
            "--arm",
            "admit",
            "--cases",
            dir.to_str().expect("a path"),
        ],
        &repo(),
    );
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let lines = stdout(&out);
    assert_eq!(lines.len(), 2);
    assert_eq!(
        lines[0],
        "case name=decoy-pay-1 kind=fault form=decoy arm=admit store=memory occurrences=8 drifts=2 assisted=3 failed=0 harm=2 reverted_good=1 edit_rows=8 expired_runs=6"
    );
    assert!(
        lines[1].starts_with("case name=renamed-pay "),
        "{}",
        lines[1]
    );
}

#[test]
fn drift_arm_with_every_occurrence_restarted_expires_more_runs() {
    let dir = scratch("drift-restart-all");
    copy_case(&dir, "renamed-pay.jsonl");
    let dir = dir.to_str().expect("a path");
    let once = gate(&["drift", "--arm", "admit", "--cases", dir], &repo());
    let every = gate(
        &["drift", "--arm", "admit", "--cases", dir, "--restart-all"],
        &repo(),
    );
    assert_eq!(
        (once.status.code(), every.status.code()),
        (Some(0), Some(0))
    );
    let (once, every) = (stdout(&once), stdout(&every));
    assert!(once[0].ends_with(" expired_runs=6"), "{}", once[0]);
    assert!(every[0].ends_with(" expired_runs=12"), "{}", every[0]);
    let cut = |line: &str| line.rsplit_once(' ').expect("a key").0.to_owned();
    assert_eq!(cut(&once[0]), cut(&every[0]));
}

#[test]
fn drift_prints_a_miss_and_exits_1() {
    let dir = scratch("drift-miss");
    std::fs::create_dir_all(dir.join("docs/r5")).expect("a targets directory");
    let targets =
        std::fs::read_to_string(repo().join("docs/r5/targets.toml")).expect("the targets file");
    assert!(targets.contains("dev_critical = 7\n"));
    std::fs::write(
        dir.join("docs/r5/targets.toml"),
        targets.replacen("dev_critical = 7\n", "dev_critical = 9\n", 1),
    )
    .expect("a targets file of the test's own");
    let cases = repo().join("fixtures/r5/cases/dev");
    let out = gate(
        &[
            "drift",
            "--gate",
            "dev",
            "--cases",
            cases.to_str().expect("a path"),
        ],
        &dir,
    );
    let lines = stdout(&out);
    assert_eq!(
        lines.last().map(String::as_str),
        Some("drift result=miss"),
        "{out:?}"
    );
    assert!(
        lines[lines.len() - 2].contains(" wins=8 losses=0 critical=9 "),
        "{out:?}"
    );
    assert_eq!(out.status.code(), Some(1), "{out:?}");
}

#[test]
fn drift_refuses_the_derived_store_with_store() {
    let out = gate(
        &[
            "drift",
            "--gate",
            "dev",
            "--cases",
            "fixtures/r5/cases/dev",
            "--store",
            "derived",
        ],
        &repo(),
    );
    refused(&out, "store");
}

#[test]
fn drift_refuses_a_set_whose_counts_are_not_the_keys() {
    let dir = scratch("drift-counts");
    copy_case(&dir, "renamed-pay.jsonl");
    let out = gate(
        &[
            "drift",
            "--gate",
            "dev",
            "--cases",
            dir.to_str().expect("a path"),
        ],
        &repo(),
    );
    refused(&out, "case");
}

#[test]
fn drift_refuses_a_case_that_the_reader_refuses_and_another_entry() {
    let dir = scratch("drift-bad-case");
    copy_case(&dir, "renamed-pay.jsonl");
    std::fs::write(dir.join("short.jsonl"), "{}\n").expect("a bad case");
    let out = gate(
        &[
            "drift",
            "--arm",
            "none",
            "--cases",
            dir.to_str().expect("a path"),
        ],
        &repo(),
    );
    refused(&out, "case");
    let other = scratch("drift-other-entry");
    copy_case(&other, "renamed-pay.jsonl");
    std::fs::write(other.join("notes.txt"), "x\n").expect("another entry");
    let out = gate(
        &[
            "drift",
            "--arm",
            "none",
            "--cases",
            other.to_str().expect("a path"),
        ],
        &repo(),
    );
    refused(&out, "case");
}

#[test]
fn drift_refuses_without_the_targets_file() {
    let dir = scratch("drift-no-targets");
    let cases = repo().join("fixtures/r5/cases/dev");
    let out = gate(
        &[
            "drift",
            "--arm",
            "none",
            "--cases",
            cases.to_str().expect("a path"),
        ],
        &dir,
    );
    refused(&out, "targets");
}

#[test]
fn every_wrong_call_is_refused_with_usage() {
    let calls: [&[&str]; 12] = [
        &[],
        &["walk"],
        &["drift"],
        &["drift", "--cases", "fixtures/r5/cases/dev"],
        &["drift", "--cases", "fixtures/r5/cases/dev", "--arm", "sort"],
        &["drift", "--cases", "fixtures/r5/cases/dev", "--gate", "all"],
        &[
            "drift",
            "--cases",
            "fixtures/r5/cases/dev",
            "--arm",
            "none",
            "--gate",
            "dev",
        ],
        &[
            "drift",
            "--cases",
            "fixtures/r5/cases/dev",
            "--arm",
            "none",
            "--extra",
        ],
        &[
            "drift",
            "--cases",
            "fixtures/r5/cases/dev",
            "--arm",
            "none",
            "--store",
            "disk",
        ],
        &[
            "drift", "--case", "x.jsonl", "--arm", "admit", "--link", "product",
        ],
        &["privacy", "--data-dir", "x"],
        &["routine"],
    ];
    for call in calls {
        refused(&gate(call, &repo()), "usage");
    }
    for call in [&["soak", "--learn", "on"][..], &["model", "--model", "m"]] {
        refused(&gate(call, &repo()), "usage");
    }
}
