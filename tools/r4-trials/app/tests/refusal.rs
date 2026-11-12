use std::io::Read;
use std::net::{Ipv4Addr, TcpListener};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const BOUND: Duration = Duration::from_secs(20);

struct Ended {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

fn refused(args: &[&str]) -> Ended {
    let mut child = Command::new(env!("CARGO_BIN_EXE_r4-trial-app"))
        .args(args)
        .env_remove("CI")
        .env_remove("R4_GUEST")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + BOUND;
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the app did not end within {BOUND:?}");
        }
        thread::sleep(Duration::from_millis(20));
    };
    let mut stdout = String::new();
    let mut stderr = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut stdout)
        .unwrap();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    Ended {
        code: status.code(),
        stdout,
        stderr,
    }
}

fn assert_refused(ended: &Ended, cause: &str) {
    assert_eq!(ended.code, Some(2), "{}", ended.stderr);
    assert_eq!(ended.stdout, "");
    assert_eq!(ended.stderr.lines().count(), 1, "{:?}", ended.stderr);
    assert!(ended.stderr.ends_with('\n'), "{:?}", ended.stderr);
    let prefix = format!("REFUSED {cause}: ");
    assert!(ended.stderr.starts_with(&prefix), "{:?}", ended.stderr);
    assert!(ended.stderr.trim_end().len() > prefix.len(), "no detail");
}

#[test]
fn a_bad_flag_is_refused_with_usage() {
    for args in [
        &["--arm", "c"][..],
        &["--window"],
        &["--control-port", "0"],
        &["--arm", "a", "--arm", "b"],
    ] {
        assert_refused(&refused(args), "usage");
    }
}

#[test]
fn a_start_where_neither_ci_nor_r4_guest_is_set_is_refused_with_not_hosted() {
    let ended = refused(&[]);
    assert_refused(&ended, "not_hosted");
    assert_eq!(
        ended.stderr,
        "REFUSED not_hosted: neither CI nor R4_GUEST is set\n"
    );
}

#[test]
fn a_control_port_in_use_is_refused_with_port_before_the_hosted_check() {
    let taken = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = taken.local_addr().unwrap().port().to_string();
    let ended = refused(&["--control-port", &port]);
    assert_refused(&ended, "port");
    assert!(ended.stderr.contains(&port), "{:?}", ended.stderr);
    drop(taken);
}
