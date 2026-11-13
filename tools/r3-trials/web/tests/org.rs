//! CP-126 E7: an `org_` run whose browser stalls in its start with no policy in force gives the
//! effect that `org_debugging` waits for (`no_channel` after the first-reply deadline, the plain
//! message, no request at the fixture). With no control reply, no value read back and no page, the
//! run is no ok run. The program runs one `org_debugging` run with a stand-in browser that holds the
//! pipe and answers nothing.
#![cfg(all(target_os = "macos", feature = "driver"))]

use r3_web::start;
use std::path::Path;

#[allow(dead_code)]
mod standin {
    include!("../src/standin.rs");
}

#[test]
fn a_hang_with_no_policy_is_no_ok_org_run() {
    use start::Start;
    let dir = std::env::temp_dir().join(format!("r3-web-org-hang-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let stand_in = standin::bundle(&dir);
    let launcher = standin::launcher_as(stand_in.binary.parent().unwrap(), "hang", "Google Chrome");
    let scratch = dir.join("scratch");
    std::fs::create_dir_all(&scratch).unwrap();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_web-trial"))
        .args(["--candidate", "cdp", "--browser"])
        .arg(&launcher)
        .args([
            "--port",
            &port.to_string(),
            "--machine",
            "hosted-mac",
            "--repo",
        ])
        .arg(&repo)
        .arg("--scratch")
        .arg(&scratch)
        .args([
            "--runs",
            "1",
            "--rows",
            "org",
            "--round",
            "3",
            "--relay",
            "--step",
            "org_debugging",
        ])
        .env_remove("R3_ORG_CONTROL_BEFORE")
        .env_remove("R3_ORG_VALUE_READ")
        .env_remove("R3_ORG_VALUE_WANT")
        .env_remove("R3_ORG_PAGE_NAMED")
        .stdin(std::process::Stdio::null())
        .start_output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let run = stdout
        .lines()
        .find(|l| l.starts_with("run ") && l.contains("step=org_debugging"))
        .unwrap_or("")
        .to_string();
    assert!(!run.is_empty(), "no run line: {stdout}");
    assert!(
        run.contains("result=miss"),
        "a hang with no policy in force counted as the policy's effect: {run}"
    );
}
