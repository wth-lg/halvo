use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

const SEEDS: &str = include_str!("../seeds.txt");
const TARGETS: &str = include_str!("../../../../docs/r5/targets.toml");
const BLOCK: &str = include_str!("../../../../docs/r5/trials/routine.md");
const BIN: &str = env!("CARGO_BIN_EXE_r5-routine");

fn root(case: &str, seeds: Option<&str>, targets: Option<&str>) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("r5-routine-binary-{}-{case}", std::process::id()));
    let routine = dir.join("tools/r5-trials/routine");
    let docs = dir.join("docs/r5");
    std::fs::create_dir_all(&routine).expect("the routine directory");
    std::fs::create_dir_all(&docs).expect("the docs directory");
    if let Some(seeds) = seeds {
        std::fs::write(routine.join("seeds.txt"), seeds).expect("the seeds file");
    }
    if let Some(targets) = targets {
        std::fs::write(docs.join("targets.toml"), targets).expect("the targets file");
    }
    dir
}

fn run(case: &str, seeds: Option<&str>, targets: Option<&str>, args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .current_dir(root(case, seeds, targets))
        .output()
        .expect("the binary runs")
}

fn refused(out: &Output, cause: &str) {
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{stderr}");
    assert!(out.stdout.is_empty());
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    assert!(
        stderr.starts_with(&format!("REFUSED {cause}: ")),
        "{stderr}"
    );
}

fn committed_block() -> &'static str {
    let open = BLOCK.find("```\n").expect("the opening fence") + 4;
    let close = BLOCK.rfind("```\n").expect("the closing fence");
    &BLOCK[open..close]
}

#[test]
fn the_binary_prints_the_committed_block_and_exits_0() {
    let out = run("block", Some(SEEDS), Some(TARGETS), &[]);
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stderr.is_empty());
    assert_eq!(String::from_utf8_lossy(&out.stdout), committed_block());
}

#[test]
fn the_binary_refuses_an_argument_that_it_does_not_take() {
    refused(&run("usage", Some(SEEDS), Some(TARGETS), &["x"]), "usage");
    refused(
        &run("usage", Some(SEEDS), Some(TARGETS), &["--sample"]),
        "usage",
    );
    refused(
        &run("usage", Some(SEEDS), Some(TARGETS), &["--sample", ""]),
        "usage",
    );
}

#[test]
fn the_binary_refuses_a_missing_seeds_file() {
    refused(&run("seed-missing", None, Some(TARGETS), &[]), "seed");
}

#[test]
fn the_binary_refuses_another_grid() {
    let other = SEEDS.replacen("7 0 0 1\n", "7 0 0 2\n", 1);
    refused(&run("seed-grid", Some(&other), Some(TARGETS), &[]), "seed");
}

#[test]
fn the_binary_refuses_a_missing_targets_file() {
    refused(&run("input-missing", Some(SEEDS), None, &[]), "input");
}

#[test]
fn the_binary_refuses_a_targets_file_without_a_key() {
    let without = TARGETS.replace("detected_min = 45\n", "");
    refused(&run("input-key", Some(SEEDS), Some(&without), &[]), "input");
}

#[test]
fn the_binary_refuses_when_a_write_to_standard_output_fails() {
    let mut child = Command::new(BIN)
        .current_dir(root("output", Some(SEEDS), Some(TARGETS)))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary starts");
    drop(child.stdout.take());
    let out = child.wait_with_output().expect("the binary ends");
    refused(&out, "output");
    assert!(String::from_utf8_lossy(&out.stderr).contains("standard output cannot be written"));
}
