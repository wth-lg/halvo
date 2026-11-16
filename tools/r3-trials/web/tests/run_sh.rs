//! CP-126 E8 (f): run.sh's refusal word for a port number out of range. `--port 0` and
//! `--port 70000` are refused as `REFUSED usage` with exit 2 today; the verifier's model said
//! `port`, the word that run.sh gives a port that another process holds. A13.1 does not fix the
//! word for a value out of range: this pins today's answer until the planner words it.
#![cfg(unix)]

use r3_web::start::Start;
use std::path::Path;

fn refused(port: &str) -> (i32, String) {
    let run_sh = Path::new(env!("CARGO_MANIFEST_DIR")).join("run.sh");
    let out = std::process::Command::new("sh")
        .arg(&run_sh)
        .args([
            "--candidate",
            "cdp",
            "--browser",
            "/nonexistent/browser",
            "--port",
            port,
        ])
        .stdin(std::process::Stdio::null())
        .start_output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn a_port_out_of_range_is_refused_as_usage() {
    for port in ["0", "70000"] {
        let (code, stderr) = refused(port);
        assert_eq!(code, 2, "--port {port}: {stderr}");
        assert!(
            stderr.lines().any(|l| l.starts_with("REFUSED usage:")),
            "--port {port}: {stderr}"
        );
    }
}

/// CP-215 G1: the fence of `r3_web::procs` is on in this test target from its start, so that no
/// test of the target reaches the real end of a process that it did not admit.
#[test]
fn the_fence_is_on_in_this_test_target() {
    assert!(
        r3_web::procs::fence::is_on(),
        "the fence is off in this test target"
    );
}
