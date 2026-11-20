#![cfg(all(target_os = "macos", debug_assertions))]

use std::process::Command;

#[test]
fn the_main_thread_turns_the_main_run_loop() {
    let output = Command::new(env!("CARGO_BIN_EXE_halvo-capture"))
        .args(["--self-check", "run-loop"])
        .output()
        .expect("the self-check did not start");
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text
        .lines()
        .find(|line| line.starts_with("run_loop_block_ms="))
        .unwrap_or_else(|| panic!("no run_loop_block_ms line in {text:?}"));
    let ms: u64 = line["run_loop_block_ms=".len()..]
        .parse()
        .unwrap_or_else(|_| panic!("the line {line:?} holds no count"));
    assert_eq!(output.status.code(), Some(0), "{text}");
    assert!(ms <= 250, "the block ran after {ms} ms");
}
