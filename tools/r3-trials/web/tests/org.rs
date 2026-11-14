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

/// The program's lines for one `org_debugging` run of a stand-in browser that holds the pipe and
/// answers nothing (the effect of the policy), on hosted-win, with the job's facts given.
fn org_lines(tag: &str, facts: &[(&str, &str)]) -> String {
    use start::Start;
    let dir = std::env::temp_dir().join(format!("r3-web-org-{tag}-{}", std::process::id()));
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
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_web-trial"));
    cmd.args(["--candidate", "cdp", "--browser"])
        .arg(&launcher)
        .args([
            "--port",
            &port.to_string(),
            "--machine",
            "hosted-win",
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
        .stdin(std::process::Stdio::null());
    for k in [
        "R3_ORG_CONTROL_BEFORE",
        "R3_ORG_VALUE_READ",
        "R3_ORG_VALUE_WANT",
        "R3_ORG_PAGE_NAMED",
        "R3_ORG_PAGE_CAUSE",
    ] {
        cmd.env_remove(k);
    }
    for (k, v) in facts {
        cmd.env(k, v);
    }
    let out = cmd.start_output().unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// CP-137 (1): a page that the job read and that does not name the setting shows no policy in
/// force; the cell is notrun with the reason, no miss and no ok.
#[test]
fn a_page_that_does_not_name_the_setting_holds_the_cell_open() {
    let stdout = org_lines(
        "unnamed",
        &[
            ("R3_ORG_CONTROL_BEFORE", "reply"),
            ("R3_ORG_VALUE_READ", "0"),
            ("R3_ORG_VALUE_WANT", "0"),
            ("R3_ORG_PAGE_NAMED", "no"),
        ],
    );
    assert!(
        stdout.lines().any(|l| l.starts_with("notrun ")
            && l.contains("step=org_debugging")
            && l.contains("cause=policy_not_seen reason=page_not_named")),
        "a page that does not name the setting did not hold the cell open: {stdout}"
    );
    assert!(
        !stdout
            .lines()
            .any(|l| (l.starts_with("run ") || l.starts_with("cell "))
                && l.contains("step=org_debugging")),
        "the cell counted a run with a page that does not name the setting: {stdout}"
    );
}

/// CP-137 (1) and (3): a page that the job could not read leaves the run on the control and the
/// value (E7), and the cell line names why the page was not read.
#[test]
fn an_unread_page_rests_on_the_control_and_the_value_and_names_its_cause() {
    let stdout = org_lines(
        "unread",
        &[
            ("R3_ORG_CONTROL_BEFORE", "reply"),
            ("R3_ORG_VALUE_READ", "0"),
            ("R3_ORG_VALUE_WANT", "0"),
            ("R3_ORG_PAGE_NAMED", "unread"),
            ("R3_ORG_PAGE_CAUSE", "empty_text"),
        ],
    );
    let cell = stdout
        .lines()
        .find(|l| l.starts_with("cell ") && l.contains("step=org_debugging"))
        .unwrap_or("");
    assert!(
        cell.contains(" ok=1 of=1 ") && cell.contains(" page_named=unread page_cause=empty_text"),
        "an unread page did not rest on the control and the value with its cause: {stdout}"
    );
}

/// CP-137 (1): a control that does not reply, or a value that does not read back, stays a miss as
/// built: the effect cannot count as the policy's.
#[test]
fn a_control_without_reply_or_a_value_not_read_back_stays_a_miss() {
    for (tag, facts, cause) in [
        (
            "noreply",
            [
                ("R3_ORG_CONTROL_BEFORE", "none"),
                ("R3_ORG_VALUE_READ", "0"),
            ],
            "cause=no_control_reply",
        ),
        (
            "novalue",
            [
                ("R3_ORG_CONTROL_BEFORE", "reply"),
                ("R3_ORG_VALUE_READ", "1"),
            ],
            "cause=no_value_read",
        ),
    ] {
        let mut all = facts.to_vec();
        all.extend([("R3_ORG_VALUE_WANT", "0"), ("R3_ORG_PAGE_NAMED", "yes")]);
        let stdout = org_lines(tag, &all);
        let run = stdout
            .lines()
            .find(|l| l.starts_with("run ") && l.contains("step=org_debugging"))
            .unwrap_or("");
        assert!(
            run.contains("result=miss") && run.contains(cause),
            "{tag}: not a miss with {cause}: {stdout}"
        );
    }
}
