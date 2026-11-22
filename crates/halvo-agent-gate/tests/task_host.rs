//! The command `task` of `agent-gate` over the host link (contracts.md A14.5, plan.md U4.11, its first pull
//! request): `task fill --mode replay --seed N` plays the task `fill` with the real runner against the gate host's
//! memory store, and every form that this pull request does not build is refused as `usage` before a runner starts.

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// The keys of the line of A14.5, in its order.
const KEYS: [&str; 15] = [
    "task",
    "name",
    "mode",
    "seed",
    "store",
    "steps",
    "prompts",
    "handbacks",
    "confirmations",
    "effects",
    "unknown",
    "model_calls",
    "marks",
    "fixture",
    "result",
];

fn scratch(name: &str) -> PathBuf {
    halvo_test_dir::make(Path::new(env!("CARGO_TARGET_TMPDIR")), name).expect("a scratch directory")
}

/// Runs `agent-gate` with `tmp` as the system's temporary directory, under which the command makes its own.
fn gate(args: &[&str], tmp: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_agent-gate"))
        .args(args)
        .env("TMPDIR", tmp)
        .env("TMP", tmp)
        .env("TEMP", tmp)
        .stdin(Stdio::null())
        .output()
        .expect("agent-gate runs")
}

/// Builds the runner's binary beside `agent-gate`, where the gate looks for it, with the runner's test feature
/// `loopback` (`common::build_runner` says why).
fn build_runner() {
    common::build_runner();
}

/// U4.11 (first) sentence 1: the fill of seed 1 over the host link prints the one line of A14.5, its keys in its
/// order, with `store=memory`, `fixture=ok`, `result=completed`, `unknown=0` and `marks=start,end`.
#[test]
fn a_fill_over_the_host_link_completes_with_store_memory() {
    build_runner();
    let tmp = scratch("task-fill");
    let out = gate(&["task", "fill", "--mode", "replay", "--seed", "1"], &tmp);
    let text = String::from_utf8(out.stdout).expect("UTF-8");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "{text}{stderr}");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 1, "{text}");
    let words: Vec<&str> = lines[0].split(' ').collect();
    let keys: Vec<&str> = words
        .iter()
        .map(|word| word.split('=').next().unwrap_or(word))
        .collect();
    assert_eq!(keys, KEYS, "{}", lines[0]);
    assert!(
        lines[0].starts_with("task name=fill mode=replay seed=1 "),
        "{}",
        lines[0]
    );
    for pair in [
        "store=memory",
        "fixture=ok",
        "result=completed",
        "unknown=0",
        "marks=start,end",
    ] {
        assert!(words.contains(&pair), "{pair} in {}", lines[0]);
    }
}

/// U4.11 (first) sentence 2: each form that this pull request does not build is refused with `REFUSED usage`, prints
/// no line and starts no runner: the command makes no directory under the temporary directory that it is given, and
/// a runner starts only with the data directory that the command makes there.
#[test]
fn a_form_that_the_command_does_not_build_yet_is_refused_as_usage() {
    const FILL: [&str; 6] = ["task", "fill", "--mode", "replay", "--seed", "1"];
    let mut forms: Vec<Vec<&str>> = ["reply", "cleanup", "bill", "order"]
        .into_iter()
        .map(|task| vec!["task", task, "--mode", "replay", "--seed", "1"])
        .collect();
    for mode in ["compile", "recover"] {
        forms.push(vec!["task", "fill", "--mode", mode, "--seed", "1"]);
    }
    for more in [
        &["--model", "/model.gguf"][..],
        &["--variant", "renamed"],
        &["--second"],
    ] {
        forms.push(FILL.iter().chain(more).copied().collect());
    }
    for form in &forms {
        let tmp = scratch("task-usage");
        let out = gate(form, &tmp);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "{form:?}: {stderr}");
        assert!(out.stdout.is_empty(), "{form:?}");
        assert!(
            stderr.starts_with("REFUSED usage: ") && stderr.lines().count() == 1,
            "{form:?}: {stderr}"
        );
        let made: Vec<_> = std::fs::read_dir(&tmp)
            .expect("the temporary directory")
            .collect();
        assert!(made.is_empty(), "{form:?}: the command made {made:?}");
    }
}
