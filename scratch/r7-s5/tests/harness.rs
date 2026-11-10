//! Checks the S6 harness itself. These tests read no key-store item and change no trust
//! store, so they are safe on a developer's machine.
#![cfg(feature = "net")]

use r7_s5_spike::stub::{self, Entry, Stub};
use r7_s5_spike::{client, procstat};
use std::process::Command;
use std::time::{Duration, Instant};

fn entries_when_present(server: &Stub) -> Vec<Entry> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while server.entries().is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    server.entries()
}

#[test]
fn a_request_under_a_trusted_ca_lands_in_the_stub_record() {
    let pki = stub::generate_pki().unwrap();
    let server = Stub::start(&pki, None).unwrap();

    let ping = client::ping(
        &server.url("/v1/ping"),
        "run-01",
        "synthetic-item",
        Some(&pki.ca_pem),
    )
    .unwrap();

    assert_eq!(ping.status, 200);
    assert_eq!(
        server.bearer_of("run-01").as_deref(),
        Some("synthetic-item")
    );
}

#[test]
fn a_request_fails_at_the_handshake_while_the_ca_is_untrusted() {
    let pki = stub::generate_pki().unwrap();
    let server = Stub::start(&pki, None).unwrap();

    let error = client::ping(&server.url("/v1/ping"), "control", "synthetic-item", None)
        .expect_err("the test CA is not in the trust store");

    let chain = client::error_chain(&error);
    assert!(chain.to_lowercase().contains("certificate"), "{chain}");
    let entries = entries_when_present(&server);
    assert!(
        matches!(entries.as_slice(), [Entry::NoRequest(_)]),
        "{entries:?}"
    );
}

#[test]
fn a_child_process_is_measured() {
    let mut command = if cfg!(windows) {
        let mut command = Command::new("cmd");
        command.args(["/C", "echo measured& exit 3"]);
        command
    } else {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "echo measured; exit 3"]);
        command
    };

    let measured = procstat::run(&mut command).unwrap();

    assert_eq!(measured.exit_code, Some(3));
    assert_eq!(measured.stdout.trim(), "measured");
    assert!(
        measured.peak_rss_bytes > 100 * 1024,
        "{}",
        measured.peak_rss_bytes
    );
    assert!(measured.wall > Duration::ZERO);
}

/// Runs the driver against the stand-in helper. Each scenario gets its own state
/// directory under the temp directory, which is left in place.
#[cfg(unix)]
fn drive(
    scenario: &str,
    trust: &[&str],
    limit_ms: &str,
    helper_mode: Option<&str>,
) -> (Option<i32>, String) {
    let state =
        std::env::temp_dir().join(format!("r7-s5-harness-{}-{scenario}", std::process::id()));
    std::fs::create_dir_all(&state).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_bench"));
    command
        .arg("--helper")
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fake-helper.sh"))
        .arg("--workdir")
        .arg(&state)
        .args(["--runner", "test-runner", "--runs", "5"])
        .args(["--limit-ms", limit_ms, "--"])
        .args(trust)
        .env("FAKE_HELPER_STATE", &state);
    if let Some(mode) = helper_mode {
        command.env("FAKE_HELPER_MODE", mode);
    }
    let output = command.output().unwrap();
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
    )
}

/// Copies the test CA to where the stand-in helper looks for it and marks it trusted.
#[cfg(unix)]
const TRUST: [&str; 5] = [
    "/bin/sh",
    "-c",
    r#"cp "$1" "$FAKE_HELPER_STATE/ca.pem" && touch "$FAKE_HELPER_STATE/trusted""#,
    "sh",
    "{ca}",
];

#[cfg(unix)]
fn assert_does_not_count(code: Option<i32>, stdout: &str, reason: &str) {
    assert_eq!(code, Some(1), "{stdout}");
    assert!(
        stdout.contains("S6 RULE test-runner: DOES NOT COUNT"),
        "{stdout}"
    );
    assert!(stdout.contains(reason), "{stdout}");
    assert!(!stdout.contains(": MET"), "{stdout}");
    assert!(!stdout.contains(": MISSED"), "{stdout}");
}

#[cfg(unix)]
#[test]
fn the_driver_reports_met_after_a_failed_control_and_a_real_trust_step() {
    let (code, stdout) = drive("met", &TRUST, "5000", None);

    assert_eq!(code, Some(0), "{stdout}");
    assert!(
        stdout.contains("S6 control: failed on the certificate at the TLS handshake, as required"),
        "{stdout}"
    );
    assert!(
        stdout.contains("S6 summary runner=test-runner runs=5 ok=5/5"),
        "{stdout}"
    );
    assert!(
        stdout.contains("S6 RULE test-runner: MET (ok=5/5"),
        "{stdout}"
    );
}

#[cfg(unix)]
#[test]
fn the_driver_reports_missed_when_a_run_is_over_the_limit() {
    let (code, stdout) = drive("over-limit", &TRUST, "0", None);

    assert_eq!(code, Some(0), "{stdout}");
    assert!(
        stdout.contains("S6 RULE test-runner: MISSED (ok=5/5"),
        "{stdout}"
    );
}

/// A MISSED must speak about the helper. When no timed run gets past the certificate,
/// the runner never trusted the CA, and that is the harness's failure.
#[cfg(unix)]
#[test]
fn the_line_does_not_count_when_the_trust_step_trusts_nothing() {
    let (code, stdout) = drive("trusts-nothing", &["/usr/bin/true"], "5000", None);

    assert_does_not_count(
        code,
        &stdout,
        "every timed run failed on a certificate error",
    );
}

#[cfg(unix)]
#[test]
fn the_line_does_not_count_when_the_trust_command_fails() {
    let (code, stdout) = drive("trust-fails", &["/usr/bin/false"], "5000", None);

    assert_does_not_count(code, &stdout, "the trust command ended with");
}

#[cfg(unix)]
#[test]
fn the_line_does_not_count_when_the_control_passes() {
    let (code, stdout) = drive("no-control", &["/usr/bin/true"], "5000", Some("insecure"));

    assert_does_not_count(code, &stdout, "the control run did not fail");
}

/// A control that fails at the stub for some other reason shows no trust decision.
#[cfg(unix)]
#[test]
fn the_line_does_not_count_when_the_control_fails_for_another_reason() {
    let (code, stdout) = drive("other-failure", &TRUST, "5000", Some("plain-http"));

    assert_does_not_count(code, &stdout, "not on a certificate error");
}
