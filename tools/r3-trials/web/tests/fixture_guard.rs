//! The fixture server's lifetime under a trial (CP-133 (c)): it is gone within the fixture's own
//! bound, `GONE_MS` of tools/fixture-server/variants/selftest.mjs, when its trial process is
//! killed, and at once when the trial drops its `Server`, also by a panic.

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

/// Ends, when a test leaves by any way, the processes that the test started: its child by its
/// handle, and the server by its number and creation time.
struct Ends {
    child: Option<Child>,
    server: Option<Proc>,
}

impl Drop for Ends {
    fn drop(&mut self) {
        if let Some(c) = self.child.as_mut() {
            let _ = c.kill();
            let _ = c.wait();
        }
        if let Some(p) = &self.server {
            if procs::running(p) {
                procs::kill(p);
            }
        }
    }
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
    let mut ends = Ends {
        child: None,
        server: None,
    };
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
    let mut pid = None;
    for line in BufReader::new(out).lines() {
        let Ok(line) = line else { break };
        if let Some(rest) = line.strip_prefix("server ") {
            pid = rest.split(' ').next().and_then(|p| p.parse::<u32>().ok());
            break;
        }
    }
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
    let _ends = Ends {
        child: None,
        server: Some(p.clone()),
    };
    assert!(
        Control { port: server.port }.state().is_ok(),
        "the server serves while its trial holds it"
    );
    drop(server);
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
    let _ends = Ends {
        child: None,
        server: Some(p.clone()),
    };
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        drop(server);
        let _ = tx.send(());
    });
    assert!(
        rx.recv_timeout(GONE).is_ok(),
        "the drop did not return within {} ms",
        GONE.as_millis()
    );
    assert!(!procs::running(&p), "the server runs after its drop");
}

#[test]
fn a_server_of_a_trial_that_panics_is_gone_when_the_panic_has_unwound() {
    let (tx, rx) = std::sync::mpsc::channel();
    let joined = std::thread::spawn(move || {
        let server = start();
        tx.send(row(server.pid())).expect("send");
        panic!("the trial fails while it holds its server");
    })
    .join();
    assert!(joined.is_err(), "the trial thread panicked");
    let p = rx.recv().expect("row of the server");
    let _ends = Ends {
        child: None,
        server: Some(p.clone()),
    };
    assert!(!procs::running(&p), "the server runs after the panic");
}
