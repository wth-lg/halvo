#![cfg(all(target_os = "macos", debug_assertions))]

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_halvo-capture");
const FIRST_LINE: &str = "halvo-capture: sources=scripted: this build has the feature test-seam";
const EVENTS_A_START: u64 = 3;
const TICK_MS: u64 = 100;
const LAPSE_MARGIN_MS: u64 = 1_000;

struct Daemon {
    child: Child,
    stderr: Arc<Mutex<Vec<String>>>,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn scratch_data_dir() -> PathBuf {
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("cli-{}-{since}", std::process::id()))
}

fn start(dir: &Path) -> Daemon {
    let mut child = Command::new(BIN)
        .args(["--service-run", "--data-dir"])
        .arg(dir)
        .arg("--scripted-sources")
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
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn total_events(dir: &Path) -> u64 {
    let output = ctl(dir, &["inventory"]);
    assert_eq!(output.status.code(), Some(0), "{}", text(&output));
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    stdout
        .lines()
        .find_map(|line| line.strip_prefix("total events="))
        .and_then(|rest| rest.split(' ').next())
        .and_then(|events| events.parse().ok())
        .unwrap_or_else(|| panic!("no total line: {stdout}"))
}

fn grows_to(dir: &Path, want: u64) {
    let reached = until(Duration::from_secs(20), || {
        let total = total_events(dir);
        (total >= want).then_some(total)
    });
    assert_eq!(
        reached,
        Some(want),
        "the inventory total did not reach {want}"
    );
}

fn status_line(dir: &Path, key: &str) -> Option<String> {
    let output = ctl(dir, &["status"]);
    let prefix = format!("{key}=");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.strip_prefix(prefix.as_str()).map(str::to_owned))
}

fn files_under(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.map_while(Result::ok) {
        let path = entry.path();
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => found.extend(files_under(&path)),
            Ok(kind) if kind.is_file() => {
                if let Ok(bytes) = std::fs::read(&path) {
                    found.push((path, bytes));
                }
            }
            _ => {}
        }
    }
    found
}

fn names_under(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.map_while(Result::ok) {
        let path = entry.path();
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            found.extend(names_under(&path));
        }
        found.push(path);
    }
    found
}

fn holds(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

fn equal_twice(dir: &Path) {
    let first = total_events(dir);
    std::thread::sleep(Duration::from_millis(1_500));
    assert_eq!(
        total_events(dir),
        first,
        "the inventory grew while nothing may record"
    );
}

#[test]
fn ctl_starts_pauses_resumes_and_stops_the_service_run() {
    let dir = scratch_data_dir();
    let mut daemon = start(&dir);

    let first = until(Duration::from_secs(10), || {
        daemon.stderr.lock().unwrap().first().cloned()
    });
    assert_eq!(
        first.as_deref(),
        Some(FIRST_LINE),
        "case 1: the sources are not scripted"
    );
    let status = until(Duration::from_secs(20), || {
        let output = ctl(&dir, &["status"]);
        (output.status.code() == Some(0)).then_some(output)
    })
    .expect("case 1: ctl status answers");
    let status = String::from_utf8_lossy(&status.stdout).into_owned();
    assert!(status.contains("\nplatform=macos\n"), "case 1: {status}");
    assert!(
        status.contains("\ntyping=off\n"),
        "case 1: no typing line: {status}"
    );
    until(Duration::from_secs(20), || {
        let output = ctl(&dir, &["status"]);
        String::from_utf8_lossy(&output.stdout)
            .contains("\nstore=open\n")
            .then_some(())
    })
    .expect("case 1: the store opens");

    let before = total_events(&dir);
    let resumed = ctl(&dir, &["resume"]);
    assert_eq!(resumed.status.code(), Some(0), "case 2: {}", text(&resumed));
    let claimed = ctl(&dir, &["status", "--indicator"]);
    assert_eq!(claimed.status.code(), Some(0), "case 2: {}", text(&claimed));
    grows_to(&dir, before + EVENTS_A_START);

    let paused = ctl(&dir, &["pause"]);
    assert_eq!(paused.status.code(), Some(0), "case 3: {}", text(&paused));
    assert!(
        String::from_utf8_lossy(&paused.stdout).contains("capture=paused"),
        "case 3: {}",
        text(&paused)
    );
    equal_twice(&dir);

    let old_id = status_line(&dir, "store_id").expect("case D: the status shows a store id");
    let kept: Vec<Vec<u8>> = files_under(&dir.join("roots"))
        .into_iter()
        .map(|(_, bytes)| bytes)
        .chain(std::fs::read(dir.join("store").join("store.key")).ok())
        .collect();
    assert_eq!(kept.len(), 2, "case D: one root file and store.key");
    let fixed = ["store.key", "store.db", "store.db-wal", "segments"]
        .map(|name| dir.join("store").join(name));
    let names_before: Vec<PathBuf> = names_under(&dir.join("store"))
        .into_iter()
        .chain(names_under(&dir.join("roots")))
        .filter(|name| !fixed.contains(name))
        .collect();
    let deleted = ctl(&dir, &["delete-all", &old_id]);
    assert_eq!(deleted.status.code(), Some(0), "case D: {}", text(&deleted));
    let deleted_line = String::from_utf8_lossy(&deleted.stdout).into_owned();
    assert!(
        deleted_line.starts_with("deleted "),
        "case D: {}",
        text(&deleted)
    );
    assert_eq!(
        total_events(&dir),
        0,
        "case D: the inventory after the delete"
    );
    let new_id = status_line(&dir, "store_id").expect("case D: the status shows the fresh store");
    assert_ne!(new_id, old_id, "case D: the store id did not change");
    assert!(
        deleted_line.ends_with(&format!(" store_id={new_id}\n")),
        "case D: {deleted_line}"
    );
    for name in &names_before {
        assert!(
            std::fs::symlink_metadata(name).is_err(),
            "case D: {} stands after the delete",
            name.display()
        );
    }
    for (path, bytes) in files_under(&dir) {
        assert!(
            !kept.iter().any(|kept| holds(&bytes, kept)),
            "case D: {} holds bytes that the store held before the delete",
            path.display()
        );
    }
    let again = ctl(&dir, &["delete-all", &old_id]);
    assert_eq!(again.status.code(), Some(0), "case D: {}", text(&again));
    assert_eq!(
        String::from_utf8_lossy(&again.stdout),
        format!("already_deleted store_id={new_id}\n"),
        "case D: {}",
        text(&again)
    );

    let before = total_events(&dir);
    let resumed = ctl(&dir, &["resume"]);
    assert_eq!(resumed.status.code(), Some(0), "case 5: {}", text(&resumed));
    let claimed = ctl(&dir, &["status", "--indicator"]);
    let last_claim = Instant::now();
    assert_eq!(claimed.status.code(), Some(0), "case 5: {}", text(&claimed));
    grows_to(&dir, before + EVENTS_A_START);
    let lapse =
        Duration::from_millis(halvo_control::INDICATOR_LEASE_MS + TICK_MS + LAPSE_MARGIN_MS);
    std::thread::sleep(lapse.saturating_sub(last_claim.elapsed()));
    let lapsed = ctl(&dir, &["status"]);
    let lapsed = String::from_utf8_lossy(&lapsed.stdout).into_owned();
    assert!(
        lapsed.contains("\ncapture=no_indicator\n"),
        "case 5: {lapsed}"
    );
    assert!(lapsed.contains("\nlease=lapsed\n"), "case 5: {lapsed}");
    equal_twice(&dir);

    let stopped = ctl(&dir, &["stop"]);
    assert_eq!(stopped.status.code(), Some(0), "case 6: {}", text(&stopped));
    assert!(
        String::from_utf8_lossy(&stopped.stdout).starts_with("stopped flushed=yes "),
        "case 6: {}",
        text(&stopped)
    );
    let ended = until(Duration::from_secs(20), || {
        daemon.child.try_wait().ok().flatten()
    });
    assert_eq!(
        ended.and_then(|status| status.code()),
        Some(0),
        "case 6: {:?}",
        daemon.stderr.lock().unwrap()
    );
}
