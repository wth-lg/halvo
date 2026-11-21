//! The tests of the daemon's worker slot (halvo-capture, daemon::r2::worker_slot): each makes a slot over a new data
//! directory, with the directory of the built worker and owner as the place of the programs, and reads what the slot
//! did. The process has one cursor, which no thread of these tests passes, so a started worker waits in its first
//! read. One lock keeps the tests apart, because the cursor serves one connection at a time.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use halvo_capture::custody::StoreKey;
use halvo_capture::daemon::r2::owner_slot::{Caller, Place};
use halvo_capture::daemon::r2::worker_slot::{Answer, Committed, Origin, WorkerSlot};
use halvo_schema_core::ids::{Position, RequestId, StoreId};
use halvo_schema_core::request::{InterfaceRead, InterfaceRole};

const KEY: [u8; 32] = [7; 32];

/// The longest wait for something that the slot or a child owes at once.
const WAIT: Duration = Duration::from_secs(30);

/// The lock that keeps the tests of this file apart.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// A new directory under the target's tmp directory, removed when the value is dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let parent = Path::new(env!("CARGO_TARGET_TMPDIR"));
        Scratch(halvo_test_dir::make(parent, name).unwrap())
    }

    fn pid_file(&self) -> PathBuf {
        self.0.join("derived").join("owner.pid")
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

/// The test's daemon: the key of the derived store and the flag of the delete.
struct Daemon {
    deleting: AtomicBool,
}

impl Caller for Daemon {
    fn key(&self) -> Option<StoreKey> {
        Some(StoreKey::from_bytes(KEY))
    }

    fn first_live(&self) -> u64 {
        1
    }

    fn deleting(&self) -> bool {
        self.deleting.load(Ordering::SeqCst)
    }

    fn report(&self, _: &[u8]) {}
}

/// The slot's writer: the test reads what the slot wrote.
#[derive(Clone, Default)]
struct Lines(Arc<Mutex<Vec<u8>>>);

impl Lines {
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
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

/// The owner's binary beside the worker's. A test binary of this package gets the worker's binary from cargo and
/// no binary of another package, so the first call builds the owner's with the cargo that built this test, in the
/// same target directory and profile, as `owner_binary` of tests/worker.rs does.
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

/// One slot over a new data directory, with what the test reads of it. The slot is dropped first and the lock of
/// the tests last.
struct Rig {
    slot: WorkerSlot,
    lines: Lines,
    daemon: Arc<Daemon>,
    scratch: Scratch,
    _one: MutexGuard<'static, ()>,
}

impl Rig {
    fn new(name: &str) -> Rig {
        let one = ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner);
        let scratch = Scratch::new(name);
        let daemon = Arc::new(Daemon {
            deleting: AtomicBool::new(false),
        });
        let lines = Lines::default();
        let place = Place {
            program_dir: owner_binary().parent().unwrap().to_path_buf(),
            data_dir: scratch.0.clone(),
            idle_ms: None,
        };
        let slot = WorkerSlot::new(
            place,
            Arc::clone(&daemon) as Arc<dyn Caller>,
            Box::new(lines.clone()),
        );
        Rig {
            slot,
            lines,
            daemon,
            scratch,
            _one: one,
        }
    }
}

/// The id of the capture store of these tests.
fn capture() -> StoreId {
    StoreId::parse("0123456789abcdef0123456789abcdef").unwrap()
}

/// A start from the zero position of the capture store.
fn from_zero() -> Origin {
    Origin {
        capture: capture(),
        committed: None,
    }
}

/// A start from a committed position of the store `store`.
fn from_committed(store: StoreId) -> Origin {
    Origin {
        capture: capture(),
        committed: Some(Committed {
            store,
            position: Position { seg: 1, idx: 0 },
        }),
    }
}

fn read_workflows() -> Vec<u8> {
    static NEXT: AtomicU8 = AtomicU8::new(1);
    let id = RequestId::from_random([NEXT.fetch_add(1, Ordering::Relaxed); 16]);
    InterfaceRead::workflows(&InterfaceRole::claim(), id, None)
        .to_bytes()
        .unwrap()
}

/// Whether a process has the pid: one that the slot under test started, and nothing is sent to it.
#[cfg(unix)]
fn runs(pid: u32) -> bool {
    let pid = libc::pid_t::try_from(pid).unwrap();
    // SAFETY: kill with the signal 0 sends nothing; it checks the pid and touches no memory.
    unsafe { libc::kill(pid, 0) == 0 }
}

/// That the process of the pid, which the slot under test started, runs. A system without the signal 0 reads the
/// slot's own answers alone.
#[cfg(unix)]
fn assert_runs(pid: u32) {
    assert!(runs(pid), "no process {pid} runs");
}

#[cfg(not(unix))]
fn assert_runs(_: u32) {}

/// That no process has the pid within 2 s.
#[cfg(unix)]
fn assert_gone(pid: u32) {
    let until = Instant::now() + Duration::from_secs(2);
    while runs(pid) {
        assert!(Instant::now() < until, "the process {pid} is still there");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(not(unix))]
fn assert_gone(_: u32) {}

/// Kills the worker of the pid, which the slot under test started for this test.
#[cfg(unix)]
fn kill(pid: u32) {
    let pid = libc::pid_t::try_from(pid).unwrap();
    // SAFETY: kill takes a pid and a signal number; the pid is the worker that this test's slot started and has
    // not reaped.
    assert_eq!(unsafe { libc::kill(pid, libc::SIGKILL) }, 0);
}

#[cfg(windows)]
fn kill(pid: u32) {
    let killed = Command::new("taskkill")
        .args(["/F", "/PID", &pid.to_string()])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        killed.status.success(),
        "taskkill: {}",
        String::from_utf8_lossy(&killed.stderr)
    );
}

/// Passes the slot with yes at the reading `now` until it has seen the end of its worker, and returns the answer
/// of that pass.
fn pass_until_seen(rig: &mut Rig, now: Instant, origin: &Origin) -> Answer {
    let until = Instant::now() + WAIT;
    loop {
        let answer = rig.slot.pass(now, true, origin);
        if answer != Answer::Lives {
            return answer;
        }
        assert!(Instant::now() < until, "the slot did not see the end");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Rule 1: one start runs the worker with its four ends, and with the committed position that the caller names; a
/// worker whose arguments or ends are bad exits at once, and this one still runs 1 s later.
#[test]
fn the_worker_starts() {
    let mut rig = Rig::new("slot-starts");
    let origin = from_committed(capture());
    assert_eq!(rig.slot.start(Instant::now(), &origin), Answer::Started);
    let pid = rig.slot.pid().expect("the slot names no worker");
    std::thread::sleep(Duration::from_secs(1));
    assert_eq!(
        rig.slot.start(Instant::now(), &origin),
        Answer::Lives,
        "{}",
        rig.lines.text()
    );
    assert_runs(pid);
    assert_eq!(rig.lines.text(), "");
}

/// Rule 2: an owner that a relay started holds no worker pair, so the first start stops it and starts one with the
/// pair; the second start answers that a worker lives and starts nothing; a relay after it reaches the worker's
/// owner.
#[test]
fn a_second_start_starts_nothing() {
    let mut rig = Rig::new("slot-second-start");
    rig.slot.owner().relay(&read_workflows()).unwrap();
    let relayed = rig.slot.owner().pid().expect("the relay started no owner");
    let began = Instant::now();
    assert_eq!(
        rig.slot.start(Instant::now(), &from_zero()),
        Answer::Started
    );
    eprintln!(
        "the start of a worker, with the stop of the owner and the start of the next, took {} ms",
        began.elapsed().as_millis()
    );
    let worker = rig.slot.pid().expect("the slot names no worker");
    let paired = rig.slot.owner().pid().expect("no owner runs");
    assert_ne!(relayed, paired, "the owner of the relay was not stopped");
    assert_gone(relayed);
    assert_eq!(rig.slot.start(Instant::now(), &from_zero()), Answer::Lives);
    assert_eq!(rig.slot.pid(), Some(worker));
    assert_eq!(rig.slot.owner().pid(), Some(paired));
    rig.slot.owner().relay(&read_workflows()).unwrap();
    assert_eq!(rig.slot.owner().pid(), Some(paired));
    assert_runs(worker);
}

/// Rule 3: after an end no process has the worker's pid, and a new start succeeds, so the cursor serves again.
#[test]
fn end_leaves_no_worker() {
    let mut rig = Rig::new("slot-end");
    assert_eq!(
        rig.slot.start(Instant::now(), &from_zero()),
        Answer::Started
    );
    let pid = rig.slot.pid().expect("the slot names no worker");
    rig.slot.end();
    assert_eq!(rig.slot.pid(), None);
    assert_gone(pid);
    assert_eq!(
        rig.slot.start(Instant::now(), &from_zero()),
        Answer::Started,
        "{}",
        rig.lines.text()
    );
    assert_runs(rig.slot.pid().expect("the slot names no worker"));
}

/// Rule 4: over three passes at one reading the caller says yes, no and yes: a worker runs, none runs, and a new
/// one runs, because an end that the slot made counts for nothing.
#[test]
fn a_no_ends_the_worker() {
    let mut rig = Rig::new("slot-no");
    let (now, origin) = (Instant::now(), from_zero());
    assert_eq!(rig.slot.pass(now, true, &origin), Answer::Started);
    let first = rig.slot.pid().expect("the slot names no worker");
    assert_runs(first);
    assert_eq!(rig.slot.pass(now, false, &origin), Answer::Idle);
    assert_eq!(rig.slot.pid(), None);
    assert_gone(first);
    assert_eq!(
        rig.slot.pass(now, true, &origin),
        Answer::Started,
        "{}",
        rig.lines.text()
    );
    assert_runs(rig.slot.pid().expect("the slot names no worker"));
}

/// Rule 5: after the delete call neither the worker nor the owner runs, no pid file of the owner stands, and a
/// pass starts nothing while the delete runs.
#[test]
fn a_delete_leaves_no_child() {
    let mut rig = Rig::new("slot-delete");
    let (now, origin) = (Instant::now(), from_zero());
    assert_eq!(rig.slot.start(now, &origin), Answer::Started);
    let worker = rig.slot.pid().expect("the slot names no worker");
    let owner = rig.slot.owner().pid().expect("no owner runs");
    assert!(rig.scratch.pid_file().exists());
    rig.daemon.deleting.store(true, Ordering::SeqCst);
    rig.slot.delete();
    assert_gone(worker);
    assert_gone(owner);
    assert!(!rig.scratch.pid_file().exists(), "derived/owner.pid stands");
    assert_eq!(rig.slot.pass(now, true, &origin), Answer::Deleting);
    assert_eq!(rig.slot.pid(), None);
    assert_eq!(rig.slot.owner().pid(), None);
}

/// Rule 6: after a worker that the test killed, a pass at the same reading starts none, and a pass at a reading
/// 60 s later starts one.
#[test]
fn a_failed_worker_waits() {
    let mut rig = Rig::new("slot-waits");
    let (now, origin) = (Instant::now(), from_zero());
    assert_eq!(rig.slot.pass(now, true, &origin), Answer::Started);
    kill(rig.slot.pid().expect("the slot names no worker"));
    assert_eq!(pass_until_seen(&mut rig, now, &origin), Answer::Waits);
    assert_eq!(rig.slot.pid(), None);
    assert_eq!(rig.slot.pass(now, true, &origin), Answer::Waits);
    assert_eq!(
        rig.slot.pass(now + Duration::from_secs(60), true, &origin),
        Answer::Started,
        "{}",
        rig.lines.text()
    );
}

/// Rule 7: after a worker that the test killed, the slot's writer holds the one line of that end, whole: the end
/// by a signal on macOS and by a code on Windows, and the cap's reading of the peak.
#[test]
fn the_line_of_an_end() {
    let mut rig = Rig::new("slot-line");
    let (now, origin) = (Instant::now(), from_zero());
    assert_eq!(rig.slot.pass(now, true, &origin), Answer::Started);
    kill(rig.slot.pid().expect("the slot names no worker"));
    assert_eq!(pass_until_seen(&mut rig, now, &origin), Answer::Waits);
    let text = rig.lines.text();
    let line = text.strip_suffix('\n').expect(&text);
    let values = line
        .strip_prefix("halvo-capture: analysis end=")
        .expect(&text);
    let (end, values) = values.split_once(" code=").expect(&text);
    let (code, peak) = values.split_once(" peak_bytes=").expect(&text);
    let number = |value: &str| !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit());
    #[cfg(unix)]
    assert_eq!((end, code), ("signal", "-"), "{text}");
    #[cfg(windows)]
    assert!(end == "code" && number(code), "{text}");
    assert!(number(peak), "{text}");
}

/// Rule 1, the position: a committed position of another store than the capture store starts no worker and no
/// owner, and the slot writes its one line for it, once over two starts. On Windows the test then starts a worker
/// while one end of a pipe of this process is inheritable, as an end of the owner's channel is while an owner
/// starts: the owner, which inherits every such handle, is stopped, and the pipe then ends while the worker lives,
/// so the worker holds no copy of that end.
#[test]
fn a_position_of_another_store_starts_nothing() {
    let mut rig = Rig::new("slot-other-store");
    let other = StoreId::parse("fedcba9876543210fedcba9876543210").unwrap();
    let origin = from_committed(other);
    for _ in 0..2 {
        assert_eq!(rig.slot.start(Instant::now(), &origin), Answer::OtherStore);
    }
    assert_eq!(rig.slot.pid(), None);
    assert_eq!(rig.slot.owner().pid(), None);
    assert_eq!(
        rig.lines.text(),
        "halvo-capture: analysis is not started: the committed position names another store than the capture store\n"
    );
    #[cfg(windows)]
    {
        use std::io::Read;
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Foundation::{SetHandleInformation, HANDLE_FLAG_INHERIT};
        let (mut reads, writes) = std::io::pipe().unwrap();
        // SAFETY: the handle is an open end that this test owns; the call sets its inherit flag alone.
        let set = unsafe {
            SetHandleInformation(
                writes.as_raw_handle(),
                HANDLE_FLAG_INHERIT,
                HANDLE_FLAG_INHERIT,
            )
        };
        assert_ne!(set, 0, "{}", std::io::Error::last_os_error());
        assert_eq!(
            rig.slot.start(Instant::now(), &from_zero()),
            Answer::Started,
            "{}",
            rig.lines.text()
        );
        drop(writes);
        rig.slot.owner().stop();
        let (tx, read) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut byte = [0u8; 1];
            let _ = tx.send(reads.read(&mut byte).ok());
        });
        let ended = read
            .recv_timeout(WAIT)
            .expect("the pipe did not end: a child holds the inheritable end");
        assert_eq!(ended, Some(0));
        assert_eq!(rig.slot.start(Instant::now(), &from_zero()), Answer::Lives);
    }
}
