use r3_web::fixture::{fixture_program, Control, Server};
use r3_web::procs::{self, Proc};
use r3_web::start::Start;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const GONE: Duration = Duration::from_millis(5000);
const VAR: &str = "R3_FIXTURE_GUARD_CHILD";
const CHILD: &str = "a_trial_process_that_holds_a_server";

#[test]
fn the_bound_is_the_fixtures_own_gone_ms() {
    let selftest =
        std::fs::read_to_string(repo().join("tools/fixture-server/variants/selftest.mjs"))
            .expect("the fixture's self-test");
    let gone_ms: Vec<u128> = selftest
        .lines()
        .filter_map(|l| l.strip_prefix("const GONE_MS = "))
        .filter_map(|v| v.trim_end_matches(';').parse().ok())
        .collect();
    assert_eq!(gone_ms, [GONE.as_millis()]);
}

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn start() -> Server {
    Server::start(&fixture_program(&repo()), &["--port".into(), "0".into()]).expect("server")
}

fn row(pid: u32) -> Proc {
    procs::table()
        .expect("process table")
        .into_iter()
        .find(|p| p.pid == pid)
        .expect("row of the server")
}

fn gone_within(p: &Proc, bound: Duration) -> bool {
    let until = Instant::now() + bound;
    loop {
        if !procs::running(p) {
            return true;
        }
        if Instant::now() >= until {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// What a test started, ended on every way out of the test: its child through the handle, and the
/// fixture server by its row through `procs::kill` behind the fence (CP-161 (3)), which admits that
/// row alone, by number and start time.
struct Ends {
    child: Option<Child>,
    server: Option<Proc>,
}

impl Ends {
    fn new() -> Ends {
        procs::fence::on();
        Ends {
            child: None,
            server: None,
        }
    }
}

impl Drop for Ends {
    fn drop(&mut self) {
        if let Some(c) = self.child.as_mut() {
            let _ = c.kill();
            let _ = c.wait();
        }
        if let Some(p) = &self.server {
            let _admitted = procs::fence::admit_process(p);
            if procs::running(p) {
                procs::kill(p);
            }
        }
    }
}

/// Runs `drop_it` on a thread of its own and fails the test when it has not returned within
/// `GONE` (CP-191 F5: a drop that never returns fails the test at its bound).
fn returns_within_gone(drop_it: impl FnOnce() + Send + 'static) {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        drop_it();
        let _ = tx.send(());
    });
    assert!(
        rx.recv_timeout(GONE).is_ok(),
        "the drop did not return within {} ms",
        GONE.as_millis()
    );
}

#[test]
fn a_trial_process_that_holds_a_server() {
    if std::env::var_os(VAR).is_none() {
        return;
    }
    let server = start();
    println!("\nserver {} {}", server.pid(), server.port);
    std::thread::sleep(Duration::from_secs(60));
    drop(server);
}

#[test]
fn the_server_is_gone_within_its_bound_when_its_trial_process_is_killed() {
    let mut ends = Ends::new();
    let mut child = Command::new(std::env::current_exe().expect("test binary"))
        .args([CHILD, "--exact", "--nocapture", "--test-threads=1"])
        .env(VAR, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .start()
        .expect("trial process");
    let out = child.stdout.take().expect("stdout");
    ends.child = Some(child);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(out).lines() {
            let Ok(line) = line else { break };
            if let Some(rest) = line.strip_prefix("server ") {
                let _ = tx.send(rest.split(' ').next().and_then(|p| p.parse::<u32>().ok()));
                return;
            }
        }
        let _ = tx.send(None);
    });
    let pid = rx
        .recv_timeout(Duration::from_secs(30))
        .expect("the trial process printed no line within 30 s");
    let server = row(pid.expect("the trial process printed its server"));
    ends.server = Some(server.clone());
    let trial = ends.child.as_mut().expect("child");
    trial.kill().expect("kill the trial process");
    trial.wait().expect("wait for the trial process");
    assert!(
        gone_within(&server, GONE),
        "the server runs {} ms after its trial process was killed",
        GONE.as_millis()
    );
}

#[test]
fn a_server_that_its_trial_drops_without_a_stop_is_gone_when_the_drop_returns() {
    let server = start();
    let p = row(server.pid());
    let mut ends = Ends::new();
    ends.server = Some(p.clone());
    assert!(
        Control { port: server.port }.state().is_ok(),
        "the server serves while its trial holds it"
    );
    returns_within_gone(move || drop(server));
    assert!(!procs::running(&p), "the server runs after its drop");
}

#[test]
fn a_server_that_ignores_its_input_is_gone_when_the_drop_returns() {
    let program = Path::new(env!("CARGO_TARGET_TMPDIR")).join("r3-ignores-its-input.mjs");
    std::fs::write(
        &program,
        "process.stdout.write('listening 127.0.0.1:1\\n');\nsetInterval(() => {}, 1000);\n",
    )
    .expect("program");
    let server = Server::start(&program, &[]).expect("server");
    let p = row(server.pid());
    let mut ends = Ends::new();
    ends.server = Some(p.clone());
    returns_within_gone(move || drop(server));
    assert!(!procs::running(&p), "the server runs after its drop");
}

#[test]
fn a_server_of_a_trial_that_panics_is_gone_when_the_panic_has_unwound() {
    let mut ends = Ends::new();
    let (tx, rx) = std::sync::mpsc::channel();
    let trial = std::thread::spawn(move || {
        let server = start();
        tx.send(row(server.pid())).expect("send");
        panic!("the trial fails while it holds its server");
    });
    let p = rx
        .recv_timeout(Duration::from_secs(30))
        .expect("row of the server");
    ends.server = Some(p.clone());
    let until = Instant::now() + GONE;
    while !trial.is_finished() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        trial.is_finished(),
        "the panic had not unwound within {} ms",
        GONE.as_millis()
    );
    assert!(trial.join().is_err(), "the trial thread panicked");
    assert!(!procs::running(&p), "the server runs after the panic");
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
