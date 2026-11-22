#![cfg(all(windows, debug_assertions))]

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_halvo-capture");
const START_WAIT: Duration = Duration::from_secs(60);
const STOP_WAIT: Duration = Duration::from_secs(20);

struct Daemon {
    child: Child,
    stderr: Arc<Mutex<Vec<String>>>,
}

impl Daemon {
    fn stderr(&self) -> String {
        self.stderr.lock().unwrap().join("\n")
    }

    fn ended_within(&mut self, within: Duration) -> Option<ExitStatus> {
        until(within, || self.child.try_wait().unwrap())
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn scratch_data_dir(name: &str) -> PathBuf {
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("cli-windows-{name}-{}-{since}", std::process::id()))
}

fn start(dir: &Path) -> Daemon {
    let mut child = Command::new(BIN)
        .args(["--service-run", "--data-dir"])
        .arg(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the service run starts");
    let pipe = child.stderr.take().unwrap();
    let stderr = Arc::new(Mutex::new(Vec::new()));
    let lines = stderr.clone();
    std::thread::spawn(move || {
        for line in BufReader::new(pipe).lines().map_while(Result::ok) {
            lines.lock().unwrap().push(line);
        }
    });
    Daemon { child, stderr }
}

fn ctl(dir: &Path, verb: &[&str]) -> Output {
    Command::new(BIN)
        .arg("ctl")
        .arg("--data-dir")
        .arg(dir)
        .args(verb)
        .output()
        .expect("ctl runs")
}

fn text(output: &Output) -> String {
    format!(
        "exit={:?}\nstdout:\n{}stderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn until<T>(within: Duration, mut read: impl FnMut() -> Option<T>) -> Option<T> {
    let end = Instant::now() + within;
    loop {
        if let Some(found) = read() {
            return Some(found);
        }
        if Instant::now() >= end {
            return None;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn first_status(dir: &Path, daemon: &Daemon, holds: impl Fn(&str) -> bool) -> String {
    until(START_WAIT, || {
        let output = ctl(dir, &["status"]);
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        (output.status.code() == Some(0) && holds(&stdout)).then_some(stdout)
    })
    .unwrap_or_else(|| {
        panic!(
            "no such status within {START_WAIT:?}; the last ctl status:\n{}\nthe run's stderr:\n{}",
            text(&ctl(dir, &["status"])),
            daemon.stderr()
        )
    })
}

fn stop(dir: &Path, daemon: &mut Daemon) {
    let stopped = ctl(dir, &["stop"]);
    assert_eq!(stopped.status.code(), Some(0), "{}", text(&stopped));
    assert!(
        String::from_utf8_lossy(&stopped.stdout).starts_with("stopped flushed=yes"),
        "{}",
        text(&stopped)
    );
    let ended = daemon.ended_within(STOP_WAIT);
    assert_eq!(
        ended.and_then(|status| status.code()),
        Some(0),
        "the run's end after ctl stop; its stderr:\n{}",
        daemon.stderr()
    );
}

#[test]
fn a_service_run_on_windows_answers_status_over_its_pipe_and_stops_on_request() {
    let dir = scratch_data_dir("status");
    let mut daemon = start(&dir);
    let status = first_status(&dir, &daemon, |_| true);
    assert!(
        status.lines().any(|line| line == "platform=windows"),
        "{status}"
    );
    let peak: u64 = status
        .lines()
        .find_map(|line| line.strip_prefix("rss_peak_bytes="))
        .and_then(|bytes| bytes.parse().ok())
        .unwrap_or_else(|| panic!("no rss_peak_bytes line: {status}"));
    assert!(peak > 0, "{status}");

    let lifeline = Command::new(BIN)
        .args(["--service-run", "--lifeline", "--data-dir"])
        .arg(&dir)
        .stdin(Stdio::null())
        .output()
        .expect("the lifeline form runs");
    assert_eq!(lifeline.status.code(), Some(1), "{}", text(&lifeline));
    assert!(
        String::from_utf8_lossy(&lifeline.stderr)
            .contains("the lifeline mode is not built on this system"),
        "{}",
        text(&lifeline)
    );
    let again = ctl(&dir, &["status"]);
    assert_eq!(again.status.code(), Some(0), "{}", text(&again));

    stop(&dir, &mut daemon);
}

#[test]
fn a_fresh_windows_store_keeps_its_root_under_the_data_directory_and_its_key_under_dpapi() {
    let dir = scratch_data_dir("store");
    let mut daemon = start(&dir);
    first_status(&dir, &daemon, |status| {
        status.lines().any(|line| line == "store=open")
    });
    stop(&dir, &mut daemon);

    let key_file = dir.join("store").join("store.key");
    let key: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&key_file).unwrap_or_else(|e| panic!("{}: {e}", key_file.display())),
    )
    .unwrap();
    assert_eq!(key["custody_kind"], "dpapi", "{key}");
    let roots: Vec<PathBuf> = std::fs::read_dir(dir.join("roots"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(roots.len(), 1, "{roots:?}");
    let root_id = key["root_id"].as_str().unwrap_or_else(|| panic!("{key}"));
    assert!(
        roots[0]
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(root_id)),
        "{roots:?} for the root id {root_id}"
    );
}
