//! The product link of `learn-gate drift` (contracts.md L10.4, L11.5): on a Mac it plays one development case on the
//! memory store and through the product's owner; on another system it is refused as `usage`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The development case that the product line plays: the first of the form `renamed`.
const CASE: &str = "fixtures/r5/cases/dev/renamed-pay.jsonl";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn gate() -> Output {
    Command::new(env!("CARGO_BIN_EXE_learn-gate"))
        .args([
            "drift", "--case", CASE, "--arm", "admit", "--link", "product",
        ])
        .current_dir(repo())
        .output()
        .expect("learn-gate runs")
}

/// Builds the owner's binary beside `learn-gate`, where the gate looks for it: a test binary of this package gets
/// the gate's binary from cargo and no binary of another package, so the cargo that built this test builds the
/// owner's in the same target directory and profile.
#[cfg(target_os = "macos")]
fn build_owner() {
    let gate = Path::new(env!("CARGO_BIN_EXE_learn-gate"));
    let profile_dir = gate.parent().expect("the gate's directory");
    let mut cmd = Command::new(env!("CARGO"));
    cmd.args([
        "build",
        "--locked",
        "-p",
        "halvo-owner",
        "--bin",
        "halvo-owner",
    ])
    .arg("--target-dir")
    .arg(profile_dir.parent().expect("the target directory"))
    .current_dir(env!("CARGO_MANIFEST_DIR"));
    match profile_dir.file_name().and_then(|name| name.to_str()) {
        Some("debug") => {}
        Some("release") => {
            cmd.arg("--release");
        }
        other => panic!("the gate's binary lies in the directory {other:?}, of no known profile"),
    }
    let built = cmd.output().expect("cargo runs");
    assert!(
        built.status.success(),
        "cargo did not build the owner's binary: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    assert!(profile_dir.join("halvo-owner").is_file());
}

/// U8.1 sentence 6 (L10.4): the case's line on the memory store, its line through the product's owner, and the
/// product line with the learned locator served, the edit listed and the result `ok`.
#[cfg(target_os = "macos")]
#[test]
fn the_product_link_prints_served_learned_yes_listed_yes_result_ok() {
    build_owner();
    let out = gate();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let text = String::from_utf8(out.stdout).expect("UTF-8");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3, "{text}");
    for (line, store) in lines[..2].iter().zip(["memory", "owner"]) {
        assert!(
            line.starts_with("case name=renamed-pay kind=benign form=renamed arm=admit "),
            "{line}"
        );
        assert!(line.contains(&format!(" store={store} ")), "{line}");
    }
    assert!(
        lines[1].contains(" reverted_good=0 edit_rows=0 "),
        "{}",
        lines[1]
    );
    assert_eq!(lines[2], "product served_learned=yes listed=yes result=ok");
}

/// The ruling of 2026-11-21 on the owner's starter: off a Mac the product link is refused as `usage`.
#[cfg(not(target_os = "macos"))]
#[test]
fn the_product_link_is_refused_as_usage_off_a_mac() {
    let out = gate();
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert!(out.stdout.is_empty(), "{out:?}");
    let err = String::from_utf8(out.stderr).expect("UTF-8");
    assert_eq!(err.lines().count(), 1, "{err}");
    assert!(err.starts_with("REFUSED usage: "), "{err}");
}
