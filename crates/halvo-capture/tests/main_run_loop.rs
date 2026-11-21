#![cfg(all(target_os = "macos", debug_assertions))]

use std::process::Command;

fn block_ms(stdout: &[u8]) -> u64 {
    let text = String::from_utf8_lossy(stdout);
    let line = text
        .lines()
        .find(|line| line.starts_with("run_loop_block_ms="))
        .unwrap_or_else(|| panic!("no run_loop_block_ms line in {text:?}"));
    line["run_loop_block_ms=".len()..]
        .parse()
        .unwrap_or_else(|_| panic!("the line {line:?} holds no count"))
}

#[test]
fn the_main_thread_turns_the_main_run_loop() {
    let output = Command::new(env!("CARGO_BIN_EXE_halvo-capture"))
        .args(["--self-check", "run-loop"])
        .output()
        .expect("the self-check did not start");
    let ms = block_ms(&output.stdout);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(ms <= 250, "the block ran after {ms} ms");
}

#[test]
fn the_service_run_waits_on_the_main_run_loop() {
    let dir = tempfile::Builder::new()
        .prefix("service-run-loop-")
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .unwrap()
        .keep();
    let output = Command::new(env!("CARGO_BIN_EXE_halvo-capture"))
        .args(["--self-check", "service-run", "--data-dir"])
        .arg(&dir)
        .output()
        .expect("the self-check did not start");
    let ms = block_ms(&output.stdout);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(ms <= 250, "the block ran after {ms} ms");
    assert!(
        dir.join("daemon-state.json").is_file(),
        "the self-check did not enter the service run"
    );
}
