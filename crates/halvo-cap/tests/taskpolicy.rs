#![cfg(target_os = "macos")]

use std::os::unix::process::ExitStatusExt;
use std::process::{Command, Stdio};

const LIMIT: u64 = 268_435_456;

#[test]
#[ignore = "the one trial of taskpolicy -m (contracts.md C23.6), run once on each Mac"]
fn taskpolicy_m_trial() {
    let argument = LIMIT / 1_048_576;
    let place = std::env::var("HALVO_CAP_TRIAL_PLACE").unwrap_or_else(|_| "unnamed".to_string());
    let output = Command::new("/usr/sbin/taskpolicy")
        .arg("-m")
        .arg(argument.to_string())
        .arg(env!("CARGO_BIN_EXE_measure-peak"))
        .args(["--control-child", "--limit", &LIMIT.to_string()])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let has = |word: &str| stdout.lines().any(|l| l.starts_with(word));
    let completed = has("completed ");
    let signal = output.status.signal();
    println!(
        "taskpolicy code={:?} signal={signal:?} crossed={} refused={} completed={completed} stderr_lines={}",
        output.status.code(),
        has("crossed "),
        has("refused "),
        String::from_utf8_lossy(&output.stderr).lines().count()
    );
    let enforced = if !completed && signal.is_some() {
        "yes"
    } else {
        "no"
    };
    println!("cell taskpolicy.{place} runs=1 enforced={enforced} argument={argument}");
}
