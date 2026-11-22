#![allow(dead_code)]

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(60);

/// Builds the binary `bin` of the package `package` beside `agent-gate`, with the features `features` of that package,
/// and returns its path. A test binary of this package gets the gate's binary from cargo and no binary of another
/// package, so the cargo that built this test builds the other binary in the same target directory and profile (the
/// form of halvo-learn-gate's test of its product link). The gate looks for the runner's binary there.
pub fn build_beside(package: &str, bin: &str, features: &[&str]) -> PathBuf {
    let gate = Path::new(env!("CARGO_BIN_EXE_agent-gate"));
    let profile_dir = gate.parent().expect("the gate's directory");
    let mut cmd = Command::new(env!("CARGO"));
    cmd.args(["build", "--locked", "-p", package, "--bin", bin]);
    if !features.is_empty() {
        cmd.arg("--features").arg(features.join(","));
    }
    cmd.arg("--target-dir")
        .arg(profile_dir.parent().expect("the target directory"))
        .current_dir(env!("CARGO_MANIFEST_DIR"));
    match profile_dir.file_name().and_then(|name| name.to_str()) {
        Some("debug") => {}
        Some("release") => {
            cmd.arg("--release");
        }
        other => panic!("the gate's binary lies in the directory {other:?}, of no known profile"),
    }
    let built = cmd.output().expect("cargo runs");
    assert!(
        built.status.success(),
        "cargo did not build the binary {bin}: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    let path = profile_dir.join(format!("{bin}{}", std::env::consts::EXE_SUFFIX));
    assert!(path.is_file(), "{}", path.display());
    path
}

/// Builds the runner's binary beside `agent-gate` with the runner's test feature `loopback`, named in the bare form
/// with `-p halvo-runner` (the form `halvo-runner/loopback` names the runner's dev-dependency on itself and leaves the
/// feature off). The binary that it leaves at that path is the one that the runner's own tests start in the same
/// cargo call, and a runner without the feature refuses the `http` origin of their fixture server before any request.
pub fn build_runner() -> PathBuf {
    build_beside("halvo-runner", "halvo-runner", &["loopback"])
}

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
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_agent-gate"));
        cmd.args(args);
        Guarded::run_command(cmd, WAIT)
    }

    /// Runs `agent-gate` as `run` does, with `tmp` as the system's temporary directory, under which the command
    /// makes its own, and ends it when it has not ended within `wait`.
    pub fn run_in(args: &[&str], tmp: &Path, wait: Duration) -> Output {
        Guarded::run_in_with(args, tmp, wait, &[])
    }

    /// Runs `agent-gate` as `run_in` does, with the variables `env` set beside the temporary directory.
    pub fn run_in_with(args: &[&str], tmp: &Path, wait: Duration, env: &[(&str, &str)]) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_agent-gate"));
        cmd.args(args)
            .env("TMPDIR", tmp)
            .env("TMP", tmp)
            .env("TEMP", tmp)
            .envs(env.iter().copied());
        Guarded::run_command(cmd, wait)
    }

    fn run_command(mut cmd: Command, wait: Duration) -> Output {
        let mut guard = Guarded { child: None };
        guard.child = Some(
            cmd.stdin(Stdio::null())
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
        let deadline = Instant::now() + wait;
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
