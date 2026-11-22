#![cfg(all(windows, debug_assertions))]

use std::ffi::OsStr;
use std::io::{BufRead, BufReader};
use std::os::windows::io::AsHandle;
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
    let stderr = lines_of(child.stderr.take().unwrap());
    Daemon { child, stderr }
}

fn lines_of(pipe: impl std::io::Read + Send + 'static) -> Arc<Mutex<Vec<String>>> {
    let stderr = Arc::new(Mutex::new(Vec::new()));
    let lines = stderr.clone();
    std::thread::spawn(move || {
        for line in BufReader::new(pipe).lines().map_while(Result::ok) {
            lines.lock().unwrap().push(line);
        }
    });
    stderr
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
    status_of(dir, &daemon.stderr, holds)
}

fn status_of(dir: &Path, stderr: &Mutex<Vec<String>>, holds: impl Fn(&str) -> bool) -> String {
    until(START_WAIT, || {
        let output = ctl(dir, &["status"]);
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        (output.status.code() == Some(0) && holds(&stdout)).then_some(stdout)
    })
    .unwrap_or_else(|| {
        panic!(
            "no such status within {START_WAIT:?}; the last ctl status:\n{}\nthe run's stderr:\n{}",
            text(&ctl(dir, &["status"])),
            stderr.lock().unwrap().join("\n")
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

    stop(&dir, &mut daemon);
}

#[test]
fn a_lifeline_run_on_windows_ends_when_its_starter_closes_the_pipe() {
    let dir = scratch_data_dir("lifeline");
    let refused = Command::new(BIN)
        .args(["--service-run", "--lifeline", "--data-dir"])
        .arg(&dir)
        .stdin(Stdio::null())
        .output()
        .expect("a start with no pipe runs");
    assert_eq!(refused.status.code(), Some(64), "{}", text(&refused));
    assert!(
        String::from_utf8_lossy(&refused.stderr)
            .contains("--lifeline needs a pipe on standard input"),
        "{}",
        text(&refused)
    );
    assert!(!dir.exists(), "a start with no pipe made {}", dir.display());

    let (input, lifeline) = std::io::pipe().unwrap();
    let (errors, error_end) = std::io::pipe().unwrap();
    let null = std::fs::OpenOptions::new().write(true).open("NUL").unwrap();
    let args = [
        OsStr::new("--service-run"),
        OsStr::new("--lifeline"),
        OsStr::new("--data-dir"),
        dir.as_os_str(),
    ];
    let child = halvo_control::child_windows::start_with_handles(
        Path::new(BIN),
        &args,
        halvo_control::child_windows::StdHandles {
            input: input.as_handle(),
            output: null.as_handle(),
            error: error_end.as_handle(),
        },
        &[],
    )
    .expect("the lifeline run starts");
    drop((input, null, error_end));
    let stderr = lines_of(errors);
    let status = status_of(&dir, &stderr, |status| {
        status.lines().any(|line| line == "store=open")
    });
    assert!(
        status.lines().any(|line| line == "parent=watched"),
        "{status}"
    );

    drop(lifeline);
    let ended = until(STOP_WAIT, || child.wait(Duration::ZERO).unwrap());
    assert_eq!(
        ended,
        Some(0),
        "the run's end after its pipe closed; its stderr:\n{}",
        stderr.lock().unwrap().join("\n")
    );
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

#[test]
fn a_windows_run_ends_inside_the_budget_when_the_session_ends() {
    use halvo_capture::windows::session::CLASS_NAME;
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowExW, GetWindowThreadProcessId, PostMessageW, WM_ENDSESSION, WM_QUERYENDSESSION,
    };
    let dir = scratch_data_dir("session");
    let mut daemon = start(&dir);
    first_status(&dir, &daemon, |status| {
        status.lines().any(|line| line == "store=open")
    });
    let class = HSTRING::from(CLASS_NAME);
    let pid = daemon.child.id();
    let window = until(START_WAIT, || {
        let mut after: Option<HWND> = None;
        loop {
            // SAFETY: the class name is a live wide string; the call reads the
            // top-level windows after `after` and takes no other pointer.
            let found = unsafe { FindWindowExW(None, after, &class, PCWSTR::null()) }.ok()?;
            let mut owner = 0_u32;
            // SAFETY: `owner` is a live u32 that the call fills.
            unsafe { GetWindowThreadProcessId(found, Some(&mut owner)) };
            if owner == pid {
                return Some(found);
            }
            after = Some(found);
        }
    })
    .unwrap_or_else(|| {
        panic!(
            "no window of the class {CLASS_NAME} for the run {pid}; its stderr:\n{}",
            daemon.stderr()
        )
    });
    // SAFETY: the call takes the window and two plain values.
    unsafe { PostMessageW(Some(window), WM_QUERYENDSESSION, WPARAM(0), LPARAM(0)) }.unwrap();
    let posted = Instant::now();
    // SAFETY: as above; a WPARAM of 1 says that the session ends.
    unsafe { PostMessageW(Some(window), WM_ENDSESSION, WPARAM(1), LPARAM(0)) }.unwrap();
    let budget = Duration::from_millis(halvo_control::SESSION_END_BUDGET_MS);
    let ended = loop {
        if let Some(status) = daemon.child.try_wait().unwrap() {
            break Some((status, posted.elapsed()));
        }
        if posted.elapsed() > budget {
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let (status, took) = ended.unwrap_or_else(|| {
        panic!(
            "the run still ran {budget:?} after the session's messages; its stderr:\n{}",
            daemon.stderr()
        )
    });
    assert_eq!(
        status.code(),
        Some(0),
        "the run's end after {took:?}; its stderr:\n{}",
        daemon.stderr()
    );
    assert!(took <= budget, "the run took {took:?}");
}
