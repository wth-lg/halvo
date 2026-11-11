use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_measure-peak");
const LIMIT: &str = "268435456";
const ROLE: &str = "HALVO_CAP_MEASURE_PEAK_CHILD";

#[test]
#[ignore = "a command that the other tests start under measure-peak"]
fn child() {
    let Ok(role) = std::env::var(ROLE) else {
        return;
    };
    if role == "read-stdin" {
        let mut text = Vec::new();
        std::io::stdin().read_to_end(&mut text).unwrap();
        println!("stdin bytes={}", text.len());
        std::io::stdout().write_all(&text).unwrap();
    }
}

fn this_test_as_a_command() -> Vec<String> {
    vec![
        std::env::current_exe().unwrap().display().to_string(),
        "--ignored".into(),
        "--exact".into(),
        "child".into(),
        "--nocapture".into(),
        "--quiet".into(),
    ]
}

fn expected_mode() -> &'static str {
    if cfg!(windows) {
        return "job";
    }
    let trials = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/r2/trials/cap.md");
    let text = std::fs::read_to_string(trials).unwrap();
    match text.lines().find(|l| l.starts_with("verdict taskpolicy: ")) {
        Some("verdict taskpolicy: enforced") => "taskpolicy",
        Some("verdict taskpolicy: not-enforced") => "poll",
        other => panic!("cap.md has no verdict line of taskpolicy: {other:?}"),
    }
}

struct Ran {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Ran {
    fn lines(&self) -> Vec<&str> {
        self.stdout.lines().collect()
    }
}

fn measure(args: &[&str]) -> Ran {
    let output = Command::new(BIN)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    Ran {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8(output.stdout).unwrap(),
        stderr: String::from_utf8(output.stderr).unwrap(),
    }
}

#[cfg(windows)]
fn shell(script: &str) -> Vec<String> {
    vec!["cmd".into(), "/c".into(), script.into()]
}

#[cfg(not(windows))]
fn shell(script: &str) -> Vec<String> {
    vec!["/bin/sh".into(), "-c".into(), script.into()]
}

fn command_form(limit: &str, runs: &str, command: &[String]) -> Ran {
    let mut args = vec!["--limit", limit, "--runs", runs, "--"];
    args.extend(command.iter().map(String::as_str));
    measure(&args)
}

fn platform() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else {
        "macos"
    }
}

fn field<'a>(line: &'a str, key: &str) -> &'a str {
    line.split(' ')
        .find_map(|word| word.strip_prefix(key).and_then(|w| w.strip_prefix('=')))
        .unwrap_or_else(|| panic!("no {key} in: {line}"))
}

fn number(line: &str, key: &str) -> i64 {
    field(line, key).parse().unwrap()
}

#[test]
fn each_bad_usage_is_refused_with_one_line() {
    let cases: &[&[&str]] = &[
        &[],
        &["--limit"],
        &["--control"],
        &["--control-child"],
        &["--limit", "0", "--runs", "1", "--", "x"],
        &["--limit", "12x", "--runs", "1", "--", "x"],
        &["--limit", "-5", "--runs", "1", "--", "x"],
        &["--limit", "+5", "--runs", "1", "--", "x"],
        &["--limit", "99999999999999999999", "--runs", "1", "--", "x"],
        &["--runs", "1", "--", "x"],
        &["--limit", "5", "--", "x"],
        &["--limit", "5", "--runs", "0", "--", "x"],
        &["--limit", "5", "--runs", "1"],
        &["--limit", "5", "--runs", "1", "--"],
        &["--control", "--runs", "1", "--limit", "5"],
        &["--control", "--limit", "5", "--", "x"],
        &["--control-child", "--control", "--limit", "5"],
        &["--control-child", "--runs", "2", "--limit", "5"],
        &["--control-child", "--limit", "5", "--", "x"],
        &["--bogus", "--limit", "5", "--runs", "1", "--", "x"],
        &["--limit", "5", "--limit", "6", "--runs", "1", "--", "x"],
        &["--limit", "5", "--runs", "1", "x", "--", "x"],
    ];
    for args in cases {
        let ran = measure(args);
        assert_eq!(ran.code, 2, "{args:?}: {}", ran.stderr);
        assert_eq!(ran.stdout, "", "{args:?}");
        let lines: Vec<&str> = ran.stderr.lines().collect();
        assert_eq!(lines.len(), 1, "{args:?}: {}", ran.stderr);
        assert!(
            lines[0].starts_with("REFUSED usage: "),
            "{args:?}: {}",
            ran.stderr
        );
    }
}

#[test]
fn three_runs_print_their_lines_the_median_and_the_checks() {
    let ran = command_form(LIMIT, "3", &shell("exit 0"));
    assert_eq!(ran.code, 0, "{}{}", ran.stdout, ran.stderr);
    let lines = ran.lines();
    assert_eq!(lines.len(), 6, "{}", ran.stdout);
    assert_eq!(
        lines[0],
        format!(
            "measure-peak platform={} mode={} limit_bytes={LIMIT} runs=3",
            platform(),
            expected_mode()
        )
    );
    let mut peaks = Vec::new();
    for (i, line) in lines[1..4].iter().enumerate() {
        assert!(
            line.starts_with(&format!("run n={} exit=0 peak_bytes=", i + 1)),
            "{line}"
        );
        assert_eq!(field(line, "capped"), "no", "{line}");
        assert!(number(line, "start") <= number(line, "end"), "{line}");
        assert!(number(line, "peak_bytes") > 0, "{line}");
        peaks.push(number(line, "peak_bytes"));
    }
    peaks.sort_unstable();
    assert_eq!(lines[4], format!("median peak_bytes={}", peaks[1]));
    assert_eq!(lines[5], "checks exit=ok cap=ok median=ok");
}

#[test]
fn the_options_are_taken_in_any_order() {
    let mut args = vec!["--runs", "1", "--limit", LIMIT, "--"];
    let command = shell("exit 0");
    args.extend(command.iter().map(String::as_str));
    let ran = measure(&args);
    assert_eq!(ran.code, 0, "{}{}", ran.stdout, ran.stderr);
}

#[test]
fn a_command_that_fails_misses_the_exit_check() {
    let ran = command_form(LIMIT, "1", &shell("exit 3"));
    assert_eq!(ran.code, 1, "{}", ran.stdout);
    let lines = ran.lines();
    assert_eq!(field(lines[1], "exit"), "3");
    assert_eq!(lines[3], "checks exit=miss cap=ok median=ok");
}

#[test]
fn a_command_that_cannot_start_prints_not_started() {
    let missing = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("no-such-program")
        .display()
        .to_string();
    let ran = command_form(LIMIT, "2", &[missing]);
    assert_eq!(ran.code, 1, "{}", ran.stdout);
    let lines = ran.lines();
    for line in &lines[1..3] {
        assert_eq!(field(line, "exit"), "not-started", "{line}");
        assert_eq!(field(line, "peak_bytes"), "0", "{line}");
        assert_eq!(field(line, "capped"), "no", "{line}");
        assert_eq!(number(line, "start"), number(line, "end"), "{line}");
    }
    assert_eq!(lines[3], "median peak_bytes=0");
    assert_eq!(lines[4], "checks exit=miss cap=ok median=ok");
}

#[test]
fn a_run_above_the_limit_misses_the_cap_check() {
    let command = vec![
        BIN.to_string(),
        "--control-child".into(),
        "--limit".into(),
        "67108864".into(),
    ];
    let ran = command_form("33554432", "1", &command);
    assert_eq!(ran.code, 1, "{}", ran.stdout);
    let lines = ran.lines();
    assert_eq!(field(lines[1], "capped"), "yes", "{}", ran.stdout);
    if cfg!(windows) {
        assert_eq!(field(lines[1], "exit"), "0", "{}", ran.stdout);
        assert!(
            number(lines[1], "peak_bytes") <= 33_554_432,
            "{}",
            ran.stdout
        );
    } else {
        assert_eq!(field(lines[1], "exit"), "signal-9", "{}", ran.stdout);
    }
    assert!(lines[3].starts_with("checks exit="), "{}", ran.stdout);
    assert_eq!(field(lines[3], "cap"), "miss", "{}", ran.stdout);
}

#[test]
fn the_output_of_a_command_goes_to_standard_error() {
    let ran = command_form(LIMIT, "1", &shell("echo marker-of-the-child"));
    assert_eq!(ran.code, 0, "{}{}", ran.stdout, ran.stderr);
    assert!(
        !ran.stdout.contains("marker-of-the-child"),
        "{}",
        ran.stdout
    );
    assert!(ran.stderr.contains("marker-of-the-child"), "{}", ran.stderr);
}

#[test]
fn the_command_reads_the_null_device_and_not_the_standard_input_of_measure_peak() {
    let mut args = vec![
        "--limit".to_string(),
        LIMIT.into(),
        "--runs".into(),
        "1".into(),
        "--".into(),
    ];
    args.extend(this_test_as_a_command());
    let mut started = Command::new(BIN)
        .args(&args)
        .env(ROLE, "read-stdin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = started.stdin.take().unwrap();
    input.write_all(b"marker-of-the-standard-input\n").unwrap();
    drop(input);
    let output = started.wait_with_output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(output.status.code(), Some(0), "{stdout}{stderr}");
    assert!(stderr.lines().any(|l| l == "stdin bytes=0"), "{stderr}");
    assert!(!stderr.contains("marker-of-the-standard-input"), "{stderr}");
}

#[cfg(target_os = "macos")]
#[test]
fn a_control_that_the_cap_does_not_catch_prints_miss_and_exits_1() {
    let ran = measure(&["--limit", "18446744073692774399", "--control"]);
    assert_eq!(ran.code, 1, "{}{}", ran.stdout, ran.stderr);
    let lines = ran.lines();
    assert_eq!(lines.len(), 3, "{}", ran.stdout);
    assert_eq!(field(lines[1], "outcome"), "failed", "{}", ran.stdout);
    assert_eq!(lines[2], "checks control=miss", "{}", ran.stdout);
}

#[test]
fn the_control_is_caught_at_the_limit() {
    let ran = measure(&["--limit", LIMIT, "--control"]);
    assert_eq!(ran.code, 0, "{}{}", ran.stdout, ran.stderr);
    let lines = ran.lines();
    assert_eq!(lines.len(), 3, "{}", ran.stdout);
    assert_eq!(
        lines[0],
        format!(
            "measure-peak platform={} mode={} limit_bytes={LIMIT} control=yes",
            platform(),
            expected_mode()
        )
    );
    assert!(
        lines[1].starts_with("control allocate_bytes=285212672 outcome="),
        "{}",
        lines[1]
    );
    if cfg!(windows) {
        assert_eq!(field(lines[1], "outcome"), "refused");
        assert_eq!(field(lines[1], "stop_ms"), "-");
        assert!(number(lines[1], "peak_bytes") <= 268_435_456);
    } else {
        assert_eq!(field(lines[1], "outcome"), "stopped");
        assert!(number(lines[1], "stop_ms") <= 2000);
        assert!(number(lines[1], "peak_bytes") > 268_435_456);
    }
    assert_eq!(lines[2], "checks control=ok");
}

#[test]
fn the_control_child_allocates_past_its_limit_and_prints_its_lines() {
    let ran = measure(&["--control-child", "--limit", "1048576"]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    assert_eq!(
        ran.lines(),
        [
            "crossed allocated_bytes=2097152",
            "completed allocated_bytes=17825792"
        ]
    );
}

#[test]
fn a_limit_off_the_mebibyte_is_reached_exactly() {
    let ran = measure(&["--control-child", "--limit", "1500000"]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    assert_eq!(
        ran.lines(),
        [
            "crossed allocated_bytes=2097152",
            "completed allocated_bytes=18277216"
        ]
    );
}
