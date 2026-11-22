//! The tests of the worker slot in the capture service's run (halvo-capture, daemon::r2::service): each opens the
//! slot over a new data directory that holds a capture store, with the directory of the built worker and owner as
//! the place of the programs and a stand-in of the system's readings that allows a worker, and drives it with the
//! calls of the service's run and the supervisor's tick (`when::tick`). No thread of these tests passes the cursor,
//! so a test writes the figures of a read into the cell as the cursor does, and a started worker waits in its first
//! read. The process has one form, one cell and one driver, so one lock keeps the tests apart, and the loop's time of
//! the ticks rises across them. A test finds the owner by derived/owner.pid and, on macOS, the worker among the
//! children of this process; on a system without that list it reads the pid file and the slot's lines alone.

use std::io::Write;
#[cfg(target_os = "macos")]
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(target_os = "macos")]
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};
#[cfg(target_os = "macos")]
use std::time::{SystemTime, UNIX_EPOCH};

use halvo_capture::daemon::custody_policy::Custody;
use halvo_capture::daemon::port::{DataDirStore, Opened, StoreHost};
use halvo_capture::daemon::r2::daemon_caller::cell;
use halvo_capture::daemon::r2::service;
use halvo_capture::daemon::r2::when::{self, Reading, Readings};
#[cfg(target_os = "macos")]
use halvo_capture::daemon::server::{
    serve_connection, Admitting, READ_DEADLINE, REPLY_WRITE_DEADLINE,
};
#[cfg(target_os = "macos")]
use halvo_control::client::{Client, ClientError, Connector};
use halvo_control::proto::Request;
use halvo_schema_core::ids::{Position, StoreId};

/// The longest wait for something that the driver, a slot or a child owes at once.
const WAIT: Duration = Duration::from_secs(30);

/// How long a test watches that nothing starts.
const WATCH: Duration = Duration::from_secs(1);

/// The line of the end of a worker that the slot ended.
const ENDED: &str = "halvo-capture: analysis end=ended code=- peak_bytes=";

/// The lock that keeps the tests of this file apart.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// The loop's time, in seconds, of the next tick. The driver lives for the process and reads at most once in 1 s of
/// that time, so the time rises across the tests.
static LOOP_SECONDS: AtomicU64 = AtomicU64::new(1_000);

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

/// A writer of lines: the test reads what was written.
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

/// The readings of a computer on mains, not in low-power mode, idle for 10 min: a worker may run.
struct Allowed;

impl Readings for Allowed {
    fn read(&mut self) -> Reading {
        Reading {
            idle: Some(Duration::from_secs(600)),
            mains: Some(true),
            low_power: Some(false),
        }
    }
}

/// The owner's binary beside the worker's. A test binary of this package gets the worker's binary from cargo and
/// no binary of another package, so the first call builds the owner's with the cargo that built this test, in the
/// same target directory and profile, as `owner_binary` of tests/daemon_caller.rs does.
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

/// The store id of the first capture store whose read a test writes into the cell.
fn first_store() -> StoreId {
    StoreId::parse("0123456789abcdef0123456789abcdef").unwrap()
}

/// The store id of the capture store that a delete leaves in place of the first.
fn fresh_store() -> StoreId {
    StoreId::parse("fedcba9876543210fedcba9876543210").unwrap()
}

/// Writes the figures of one read from ordinal 0 of `store` into the cell, as the cursor does.
fn write_read(store: &StoreId) {
    cell().keep(store.clone(), Position { seg: 1, idx: 0 }, Some(1));
}

/// The app's liveness signal, and two ticks of the supervisor.
fn seen_and_two_ticks() {
    service::seen(&Request::Status { indicator: true });
    tick();
    tick();
}

/// One tick of the supervisor, 1 s of the loop's time after the last.
fn tick() {
    when::tick(Duration::from_secs(
        LOOP_SECONDS.fetch_add(1, Ordering::SeqCst),
    ));
}

/// The workers that run as children of this process: on macOS, the children named halvo-analysis.
#[cfg(target_os = "macos")]
fn workers() -> Vec<u32> {
    let mut pids: Vec<libc::pid_t> = vec![0; 1_024];
    let bytes = libc::c_int::try_from(std::mem::size_of_val(pids.as_slice())).unwrap();
    // SAFETY: the call writes at most `bytes` bytes of pids into the buffer, which holds that many.
    let listed =
        unsafe { libc::proc_listchildpids(libc::getpid(), pids.as_mut_ptr().cast(), bytes) };
    assert!(listed >= 0, "the children of this process were not listed");
    pids.truncate(usize::try_from(listed).unwrap().min(pids.len()));
    pids.into_iter()
        .filter(|pid| *pid > 0 && name_of(*pid).as_deref() == Some("halvo-analysis"))
        .map(|pid| u32::try_from(pid).unwrap())
        .collect()
}

/// The name of the process of the pid, none where it has ended.
#[cfg(target_os = "macos")]
fn name_of(pid: libc::pid_t) -> Option<String> {
    let mut name = [0u8; 256];
    let size = u32::try_from(name.len()).unwrap();
    // SAFETY: the call writes at most `size` bytes of the name into the buffer, which holds that many.
    let written = unsafe { libc::proc_name(pid, name.as_mut_ptr().cast(), size) };
    let written = usize::try_from(written)
        .ok()
        .filter(|written| *written > 0)?;
    Some(String::from_utf8_lossy(&name[..written.min(name.len())]).into_owned())
}

/// A system without the list of a process's children names no worker.
#[cfg(not(target_os = "macos"))]
fn workers() -> Vec<u32> {
    Vec::new()
}

/// Whether a process has the pid: one that the slot under test started, and nothing is sent to it.
#[cfg(unix)]
fn runs(pid: u32) -> bool {
    let pid = libc::pid_t::try_from(pid).unwrap();
    // SAFETY: kill with the signal 0 sends nothing; it checks the pid and touches no memory.
    unsafe { libc::kill(pid, 0) == 0 }
}

/// That the process of the pid runs. A system without the signal 0 reads the pid file and the slot's lines alone.
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

/// The two children of the slot: the owner by its pid file, and the worker where this system lists the children of
/// this process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Running {
    owner: u32,
    worker: Option<u32>,
}

/// A data directory that holds a capture store, with the writer of the slot's lines. The slot is stopped when the
/// rig is dropped, before its directory is removed, and the lock of the tests is let go last.
struct Rig {
    lines: Lines,
    scratch: Scratch,
    _one: MutexGuard<'static, ()>,
}

impl Drop for Rig {
    /// Stops the slot, so that the next test finds no child; a test that fails prints what the slot wrote.
    fn drop(&mut self) {
        service::stop();
        if std::thread::panicking() {
            eprintln!("the slot wrote: {:?}", self.lines.text());
        }
    }
}

impl Rig {
    /// Makes a capture store in a new data directory under the root file's custody and drops it, as `Rig::new` of
    /// tests/daemon_caller.rs does.
    fn new(name: &str) -> Rig {
        let one = ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner);
        let scratch = Scratch::new(name);
        let host = DataDirStore::at(&scratch.0);
        let Opened::ReadWrite { store, .. } = host.create(Custody::RootFile) else {
            panic!("no capture store was made in {}", scratch.0.display());
        };
        drop(store);
        Rig {
            lines: Lines::default(),
            scratch,
            _one: one,
        }
    }

    fn dir(&self) -> &Path {
        &self.scratch.0
    }

    fn pid_file(&self) -> PathBuf {
        self.dir().join("derived").join("owner.pid")
    }

    /// The open of the store in the service's run, with the directory of the built programs.
    fn open(&self) {
        service::open(
            self.dir(),
            Ok(owner_binary().parent().unwrap().to_path_buf()),
            Allowed,
            Box::new(self.lines.clone()),
        );
    }

    /// The pid that derived/owner.pid names, none where no such file stands.
    fn owner(&self) -> Option<u32> {
        let line = std::fs::read_to_string(self.pid_file()).ok()?;
        let pid = line.strip_prefix("pid=")?.split(' ').next()?;
        Some(pid.parse().unwrap())
    }

    /// Waits until the owner's pid file stands and, where this system lists the children, one worker runs.
    fn wait_running(&self) -> Running {
        let until = Instant::now() + WAIT;
        loop {
            let workers = workers();
            if let Some(owner) = self.owner() {
                if cfg!(not(target_os = "macos")) || workers.len() == 1 {
                    return Running {
                        owner,
                        worker: workers.first().copied(),
                    };
                }
            }
            assert!(
                Instant::now() < until,
                "no worker and owner run (workers {workers:?}): {}",
                self.lines.text()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// That for 1 s no owner's pid file stands and no worker runs.
    fn assert_nothing_starts(&self) {
        let until = Instant::now() + WATCH;
        while Instant::now() < until {
            assert!(
                !self.pid_file().exists(),
                "an owner started: {}",
                self.lines.text()
            );
            assert_eq!(workers(), Vec::<u32>::new(), "a worker started");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// That neither child runs, no pid file of the owner stands, and the slot wrote the line of the worker that it
    /// ended.
    fn assert_both_ended(&self, running: Running) {
        if let Some(worker) = running.worker {
            assert_gone(worker);
        }
        assert_gone(running.owner);
        assert!(!self.pid_file().exists(), "derived/owner.pid stands");
        let until = Instant::now() + WAIT;
        while !self.lines.text().contains(ENDED) {
            assert!(
                Instant::now() < until,
                "no line of an ended worker: {}",
                self.lines.text()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// Rule 1: after open, a read in the cell, the app's signal and two ticks, a worker and an owner run, and
/// derived/owner.pid stands; the slot wrote no line.
#[test]
fn an_open_store_starts_a_worker() {
    let rig = Rig::new("children-open");
    rig.open();
    write_read(&first_store());
    seen_and_two_ticks();
    let running = rig.wait_running();
    assert_runs(running.owner);
    if let Some(worker) = running.worker {
        assert_runs(worker);
    }
    assert!(rig.pid_file().is_file());
    assert_eq!(rig.lines.text(), "");
}

/// Rule 2: an open whose directory of the programs is an error returns and writes one line, and the ticks after it
/// start no child.
#[test]
fn a_making_that_fails_leaves_the_service_running() {
    let rig = Rig::new("children-not-made");
    let fault = std::io::Error::new(std::io::ErrorKind::NotFound, "no directory");
    service::open(rig.dir(), Err(fault), Allowed, Box::new(rig.lines.clone()));
    assert_eq!(
        rig.lines.text(),
        "halvo-capture: analysis is not made: the directory of the daemon's programs cannot be read: entity not found\n"
    );
    write_read(&first_store());
    seen_and_two_ticks();
    rig.assert_nothing_starts();
}

/// Rule 4: with both children running, the stop returns within 8 s, neither child runs, no pid file of the owner
/// stands, and the ticks after it start nothing.
#[test]
fn the_stop_ends_both_children() {
    let rig = Rig::new("children-stop");
    rig.open();
    write_read(&first_store());
    seen_and_two_ticks();
    let running = rig.wait_running();
    let began = Instant::now();
    service::stop();
    let took = began.elapsed();
    eprintln!("the stop took {} ms", took.as_millis());
    assert!(took < Duration::from_secs(8), "the stop took {took:?}");
    rig.assert_both_ended(running);
    tick();
    tick();
    rig.assert_nothing_starts();
}

/// Rule 5: after the delete neither child runs, the cell holds no figures, and a tick starts nothing. A read of the
/// store that goes, kept after the delete as a late pass of its store owner would keep it, starts nothing after the
/// next open either: no owner starts before the cell holds a read of the fresh store, and then a worker and an
/// owner start anew.
#[test]
fn a_delete_ends_both_and_a_new_open_starts_anew() {
    let rig = Rig::new("children-delete");
    rig.open();
    write_read(&first_store());
    seen_and_two_ticks();
    let first = rig.wait_running();
    service::delete();
    rig.assert_both_ended(first);
    assert_eq!(
        cell().figures(),
        None,
        "the delete left figures in the cell"
    );
    tick();
    rig.assert_nothing_starts();

    write_read(&first_store());
    rig.open();
    assert_eq!(cell().figures(), None, "the open left figures in the cell");
    tick();
    let until = Instant::now() + WAIT;
    while !cell().asked() {
        assert!(
            Instant::now() < until,
            "the driver did not turn after the open"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    rig.assert_nothing_starts();

    write_read(&fresh_store());
    seen_and_two_ticks();
    let again = rig.wait_running();
    assert_ne!(again.owner, first.owner);
    assert_runs(again.owner);
    if let Some(worker) = again.worker {
        assert_runs(worker);
    }
}

/// The user id of this process, the daemon's own in these tests.
#[cfg(target_os = "macos")]
fn own_user() -> u32 {
    // SAFETY: the call reads the effective user id of this process and touches no memory.
    unsafe { libc::geteuid() }
}

/// A connector that gives the client call the client's end of the test's control connection, once, as `Once` of
/// tests/daemon_stream.rs does.
#[cfg(target_os = "macos")]
struct Once(Mutex<Option<UnixStream>>);

#[cfg(target_os = "macos")]
impl Connector for Once {
    type Transport = UnixStream;

    fn endpoint(&self) -> String {
        "the test's control connection".to_owned()
    }

    fn connect(&self, _: Duration) -> Result<UnixStream, ClientError> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .take()
            .expect("the client call connects once"))
    }
}

/// One connection of the control server, as `connection` of tests/daemon_stream.rs: a thread serves one end of a
/// new socket pair with `serve_connection` for a peer of this process's user, while `call` asks on the other end.
#[cfg(target_os = "macos")]
fn connection<T>(call: impl FnOnce(UnixStream) -> T) -> T {
    let (client, mut server) = UnixStream::pair().unwrap();
    server.set_read_timeout(Some(READ_DEADLINE)).unwrap();
    server
        .set_write_timeout(Some(REPLY_WRITE_DEADLINE))
        .unwrap();
    let admitting = Arc::new(Admitting::default());
    let (hand, handed) = mpsc::channel();
    let answer = std::thread::scope(|scope| {
        let serving = scope.spawn(|| {
            serve_connection(&mut server, Some(own_user()), own_user(), &admitting, &hand)
        });
        let answer = call(client);
        assert!(
            serving.join().unwrap().is_none(),
            "the server waits for the loop's answer"
        );
        answer
    });
    assert!(
        handed.try_recv().is_err(),
        "a request reached the service's loop"
    );
    answer
}

/// The client's call `defer` with the deadline `until`, in nanoseconds since 1970.
#[cfg(target_os = "macos")]
fn defer(until: i64) -> Result<Option<i64>, ClientError> {
    connection(|client| Client::new(Once(Mutex::new(Some(client)))).defer(until))
}

/// The wall clock in nanoseconds since 1970.
#[cfg(target_os = "macos")]
fn wall_nanos() -> i64 {
    let since = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    i64::try_from(since.as_nanos()).unwrap()
}

/// Ruling 7: with a worker and an owner running, after a `defer` 60 s ahead and one tick the worker is gone and the
/// slot wrote the line of a worker that it ended; after two more ticks no worker runs for 1 s, and derived/owner.pid
/// names the same owner. A `defer` in the past clears the mark of the process.
#[cfg(target_os = "macos")]
#[test]
fn a_defer_ends_the_worker_and_none_starts() {
    let rig = Rig::new("children-defer");
    rig.open();
    write_read(&first_store());
    seen_and_two_ticks();
    let running = rig.wait_running();
    let worker = running.worker.expect("macOS lists the worker");
    assert!(
        defer(wall_nanos() + 60_000_000_000).unwrap().is_some(),
        "no deadline is in force"
    );
    tick();
    let until = Instant::now() + WAIT;
    while !rig.lines.text().contains(ENDED) {
        assert!(
            Instant::now() < until,
            "no line of an ended worker: {}",
            rig.lines.text()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_gone(worker);

    tick();
    tick();
    let until = Instant::now() + WATCH;
    while Instant::now() < until {
        assert_eq!(workers(), Vec::<u32>::new(), "a worker started");
        assert_eq!(rig.owner(), Some(running.owner), "the owner changed");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_runs(running.owner);

    assert_eq!(defer(wall_nanos() - 1_000_000_000).unwrap(), None);
}
