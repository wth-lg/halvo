//! The test of the driver of the worker slot (halvo-capture, daemon::r2::when) over the real slot: the slot starts the
//! built worker and owner, as the tests of tests/worker_slot.rs start them, and the readings of the system are a
//! stand-in. The process has one cursor, which no thread of this test passes, so a started worker waits in its first
//! read.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use halvo_capture::custody::StoreKey;
use halvo_capture::daemon::r2::owner_slot::{Caller, Place};
use halvo_capture::daemon::r2::when::{self, Driver, Reading, Readings, Source};
use halvo_capture::daemon::r2::worker_slot::{Answer, Origin, WorkerSlot};
use halvo_schema_core::ids::{Position, StoreId};

/// A new directory under the target's tmp directory, removed when the value is dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let parent = Path::new(env!("CARGO_TARGET_TMPDIR"));
        Scratch(halvo_test_dir::make(parent, name).unwrap())
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Err(e) = std::fs::remove_dir_all(&self.0) {
            eprintln!(
                "the scratch directory {} was not removed: {e}",
                self.0.display()
            );
        }
    }
}

/// The test's daemon: the key of the derived store, and no delete.
struct Daemon;

impl Caller for Daemon {
    fn key(&self) -> Option<StoreKey> {
        Some(StoreKey::from_bytes([7; 32]))
    }

    fn first_live(&self) -> u64 {
        1
    }

    fn deleting(&self) -> bool {
        false
    }

    fn report(&self, _: &[u8]) {}
}

/// The slot's writer: the test prints what the slot wrote when it fails.
#[derive(Clone, Default)]
struct Lines(Arc<Mutex<Vec<u8>>>);

impl Lines {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
}

impl Write for Lines {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The stand-in of the system's readings: the test sets the reading.
#[derive(Clone)]
struct Set(Arc<Mutex<Reading>>);

impl Readings for Set {
    fn read(&mut self) -> Reading {
        *self.0.lock().unwrap()
    }
}

/// The stand-in source: the zero position of the capture store, and no watermark.
struct FromZero;

impl Source for FromZero {
    fn origin(&mut self) -> Option<(Origin, Option<Position>)> {
        let capture = StoreId::parse("0123456789abcdef0123456789abcdef").unwrap();
        Some((
            Origin {
                capture,
                committed: None,
            },
            None,
        ))
    }
}

/// The owner's binary beside the worker's, built with the cargo that built this test, in the same target directory
/// and profile, as `owner_binary` of tests/worker_slot.rs builds it.
fn owner_binary() -> &'static Path {
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    BUILT.get_or_init(|| {
        let worker = Path::new(env!("CARGO_BIN_EXE_halvo-analysis"));
        let profile_dir = worker.parent().unwrap();
        let mut cmd = Command::new(env!("CARGO"));
        cmd.args(["build", "--locked", "-p", "halvo-owner"])
            .args(["--bin", "halvo-owner", "--target-dir"])
            .arg(profile_dir.parent().unwrap())
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .stdin(Stdio::null());
        match profile_dir.file_name().and_then(|name| name.to_str()) {
            Some("debug") => {}
            Some("release") => {
                cmd.arg("--release");
            }
            other => {
                panic!("the worker's binary lies in the directory {other:?}, of no known profile")
            }
        }
        let built = cmd.output().unwrap();
        assert!(
            built.status.success(),
            "cargo did not build the owner's binary: {}",
            String::from_utf8_lossy(&built.stderr)
        );
        let owner = profile_dir.join(format!("halvo-owner{}", std::env::consts::EXE_SUFFIX));
        assert!(owner.is_file(), "no file {}", owner.display());
        owner
    })
}

/// Whether a process has the pid: the worker that the slot under test started, and nothing is sent to it.
#[cfg(unix)]
fn runs(pid: u32) -> bool {
    let pid = libc::pid_t::try_from(pid).unwrap();
    // SAFETY: kill with the signal 0 sends nothing; it checks the pid and touches no memory.
    unsafe { libc::kill(pid, 0) == 0 }
}

/// That the process of the pid runs. A system without the signal 0 reads the slot's own answers alone.
#[cfg(unix)]
fn assert_runs(pid: u32) {
    assert!(runs(pid), "no process {pid} runs");
}

#[cfg(not(unix))]
fn assert_runs(_: u32) {}

/// That no process has the pid within 2 s.
#[cfg(unix)]
fn assert_gone(pid: u32) {
    let until = std::time::Instant::now() + Duration::from_secs(2);
    while runs(pid) {
        assert!(
            std::time::Instant::now() < until,
            "the process {pid} is still there"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(not(unix))]
fn assert_gone(_: u32) {}

/// Rule 6: a turn with an idle time of 600 s, on mains and with the app seen, starts a worker; a touch of the keyboard
/// (an idle time of 0) at the next reading, 1 s of the loop's time later, gives the slot a no, and no worker runs
/// 2 s after that turn.
#[test]
fn a_touch_ends_the_worker() {
    let scratch = Scratch::new("when-touch");
    let lines = Lines::default();
    let place = Place {
        program_dir: owner_binary().parent().unwrap().to_path_buf(),
        data_dir: scratch.0.clone(),
        idle_ms: None,
    };
    let slot = WorkerSlot::new(place, Arc::new(Daemon), Box::new(lines.clone()));
    let reading = Arc::new(Mutex::new(Reading {
        idle: Some(Duration::from_secs(600)),
        mains: Some(true),
        low_power: Some(false),
    }));
    let mut driver = Driver::new(Set(Arc::clone(&reading)), FromZero, slot);
    when::app_seen();
    assert_eq!(
        driver.turn(Duration::from_secs(600)),
        Some(Answer::Started),
        "{}",
        lines.text()
    );
    let pid = driver.slot().pid().expect("the slot names no worker");
    assert_runs(pid);
    reading.lock().unwrap().idle = Some(Duration::ZERO);
    assert_eq!(driver.turn(Duration::from_secs(601)), Some(Answer::Idle));
    assert_eq!(driver.slot().pid(), None);
    assert_gone(pid);
}
