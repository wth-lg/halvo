#![allow(dead_code)]

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(60);

/// A child process of a test that is killed and reaped when the guard is dropped, on every path.
pub struct Guarded {
    child: Option<Child>,
}

impl Drop for Guarded {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

pub struct Output {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Guarded {
    /// Runs `agent-gate` with the arguments and no standard input, and returns its status and output.
    pub fn run(args: &[&str]) -> Output {
        let mut guard = Guarded { child: None };
        guard.child = Some(
            Command::new(env!("CARGO_BIN_EXE_agent-gate"))
                .args(args)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("agent-gate starts"),
        );
        let child = guard.child.as_mut().expect("the child");
        let mut stdout = child.stdout.take().expect("stdout");
        let mut stderr = child.stderr.take().expect("stderr");
        let out = thread::spawn(move || {
            let mut text = String::new();
            stdout.read_to_string(&mut text).map(|_| text)
        });
        let err = thread::spawn(move || {
            let mut text = String::new();
            stderr.read_to_string(&mut text).map(|_| text)
        });
        let deadline = Instant::now() + WAIT;
        let status = loop {
            if let Some(status) = child.try_wait().expect("try_wait") {
                break status;
            }
            assert!(Instant::now() < deadline, "agent-gate did not end");
            thread::sleep(Duration::from_millis(10));
        };
        guard.child = None;
        Output {
            code: status.code(),
            stdout: out.join().expect("reader").expect("stdout text"),
            stderr: err.join().expect("reader").expect("stderr text"),
        }
    }
}
