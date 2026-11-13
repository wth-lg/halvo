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

fn port_args() -> Vec<String> {
    if r4_driver_trial::options::CANDIDATE.is_some() {
        vec!["--webdriver-port".into(), "47002".into()]
    } else {
        vec![]
    }
}

fn refused(args: &[&str]) -> Ended {
    let mut all = port_args();
    all.extend(args.iter().map(|a| a.to_string()));
    let mut child = Command::new(env!("CARGO_BIN_EXE_r4-driver-trial"))
        .args(&all)
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
            panic!("the trial did not end within {BOUND:?}");
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
    assert_eq!(ended.code, Some(2), "stderr: {}", ended.stderr);
    assert_eq!(ended.stdout, "");
    let lines: Vec<&str> = ended.stderr.lines().collect();
    assert_eq!(lines.len(), 1, "{:?}", ended.stderr);
    assert!(
        lines[0].starts_with(&format!("REFUSED {cause}: ")),
        "{:?}",
        lines[0]
    );
}

#[test]
fn without_ci_or_a_guest_the_trial_refuses_not_hosted() {
    assert_refused(&refused(&[]), "not_hosted");
    assert_refused(
        &refused(&["--dialog", "plugin", "--shows", "3"]),
        "not_hosted",
    );
    assert_refused(&refused(&["--dialog", "system"]), "not_hosted");
}

#[test]
fn a_bad_flag_is_refused_with_usage_before_anything_else() {
    assert_refused(&refused(&["--dialog", "alert"]), "usage");
    assert_refused(&refused(&["--shows", "2"]), "usage");
    assert_refused(&refused(&["--arm", "a"]), "usage");
}

#[test]
fn a_control_port_in_use_is_refused_with_port() {
    let taken = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = taken.local_addr().unwrap().port().to_string();
    assert_refused(&refused(&["--control-port", &port]), "port");
}
