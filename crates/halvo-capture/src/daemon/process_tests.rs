#![cfg(all(target_os = "macos", debug_assertions))]

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use halvo_control::instance_lock::{self, LockRead};
use halvo_control::model::{StorageCap, UtcDay};
use halvo_control::onboarding::{GrantState, Permission};
use halvo_control::paths::DataDir;

use super::custody_policy::{select, Custody, System};
use super::instance::tests::{backup_mark, kept_dir};
use super::instance::InstanceLock;
use super::main_loop::{self, MainLoop};
use super::maintenance::{Clocks, Upkeep, MAINTENANCE_INTERVAL, SHRED_CHUNK};
use super::model_store::{foreground, noon, open_floor, ManualClocks, APP, AT_NOON};
use super::owner::open::{try_open, CreateRule, Held};
use super::owner::{Answer, Owner, OwnerHandle, OwnerParts};
use super::permissions::{ProbeRunner, Probed, RequestStarter};
use super::port::{
    Access, ByteBound, CaptureStore, DataDirStore, EntryKind, KindEntry, MaintFault, MaintainStore,
    Opened, Pass, SegmentId, SegmentWrite, StoreHost, Usage, WriteFault, Written,
};
use super::state::tests::TestWrite;
use super::state::{Cap, DaemonState, StateDir, StateFile, StateWrite};
use super::{serve, serve_held, signal};
use crate::event::Modality;
use crate::sink::{Emitted, SinkHandle};

const SERVE: &str = "HALVO_DAEMON_SERVE_DIR";
const SERVER: &str = "daemon::process_tests::serve_for_parent";
const READY_WITHIN: Duration = Duration::from_secs(60);
const SECOND_START_WITHIN: Duration = Duration::from_secs(2);

struct Service(Child);

impl Drop for Service {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_some() {
            return;
        }
        if let Ok(pid) = libc::pid_t::try_from(self.0.id()) {
            unsafe { libc::kill(pid, libc::SIGTERM) };
        }
        let started = Instant::now();
        while self.0.try_wait().ok().flatten().is_none() {
            if started.elapsed() > Duration::from_secs(10) {
                let _ = self.0.kill();
                let _ = self.0.wait();
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

struct SleepLoop(Instant);

impl MainLoop for SleepLoop {
    fn turn(&mut self, wait: Duration) {
        std::thread::sleep(wait);
    }

    fn now(&self) -> Duration {
        self.0.elapsed()
    }
}

#[test]
fn serve_for_parent() {
    let Some(dir) = std::env::var_os(SERVE) else {
        return;
    };
    signal::prepare().unwrap();
    let dir = DataDir::from_flag(PathBuf::from(dir)).unwrap();
    let status = serve(
        &dir,
        Box::new(TestWrite::default()),
        SleepLoop(Instant::now()),
        signal::flag(),
        Arc::new(main_loop::ExitNow),
        &mut std::io::stderr(),
    );
    std::process::exit(status);
}

fn service(dir: &Path) -> Service {
    Service(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", SERVER, "--test-threads=1"])
            .env(SERVE, dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    )
}

fn data_dir(path: &Path) -> DataDir {
    DataDir::from_flag(path.to_str().unwrap()).unwrap()
}

fn ready(dir: &Path, service: &mut Service) {
    let child = &mut service.0;
    let named = data_dir(dir);
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            panic!("the service run ended before it was ready: {status}");
        }
        let held = instance_lock::read(&named).is_ok_and(|read| read == LockRead::Held);
        if held && dir.join("daemon-state.json").exists() && backup_mark(dir, false).is_some() {
            return;
        }
        assert!(
            started.elapsed() < READY_WITHIN,
            "the service run never became ready"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn terminate(mut service: Service) -> Option<i32> {
    let child = &mut service.0;
    let pid = libc::pid_t::try_from(child.id()).unwrap();
    assert_eq!(unsafe { libc::kill(pid, libc::SIGTERM) }, 0);
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status.code();
        }
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "the service run did not end on SIGTERM"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

type Listing = Vec<(PathBuf, bool, u64, SystemTime, u32)>;

fn listing(dir: &Path) -> Listing {
    use std::os::unix::fs::MetadataExt;
    let mut found = Vec::new();
    let mut walk = vec![dir.to_path_buf()];
    while let Some(at) = walk.pop() {
        let meta = std::fs::symlink_metadata(&at).unwrap();
        found.push((
            at.strip_prefix(dir).unwrap().to_path_buf(),
            meta.is_dir(),
            meta.len(),
            meta.modified().unwrap(),
            meta.mode(),
        ));
        if meta.is_dir() {
            for entry in std::fs::read_dir(&at).unwrap() {
                walk.push(entry.unwrap().path());
            }
        }
    }
    found.sort();
    found
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn store_open(dir: &Path, service: &mut Service) {
    let store = dir.join("store");
    let started = Instant::now();
    loop {
        if let Some(status) = service.0.try_wait().unwrap() {
            panic!("the service run ended before its store opened: {status}");
        }
        let marked = std::fs::read_to_string(dir.join("daemon-state.json"))
            .is_ok_and(|text| text.contains("\"store_made\":true"));
        if marked
            && store.join("store.key").exists()
            && !store.join("create.pending").exists()
            && instance_lock::read(&data_dir(&store)).is_ok_and(|read| read == LockRead::Held)
        {
            return;
        }
        assert!(
            started.elapsed() < READY_WITHIN,
            "the service run never opened a store under its data directory"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

type StoreBytes = Vec<(PathBuf, Option<Vec<u8>>)>;

fn store_bytes(dir: &Path) -> StoreBytes {
    let store = dir.join("store");
    let mut found: StoreBytes = ["store.key", "store.db", "store.db-wal"]
        .into_iter()
        .map(|name| (PathBuf::from(name), std::fs::read(store.join(name)).ok()))
        .collect();
    let mut walk = vec![store.join("segments")];
    while let Some(at) = walk.pop() {
        let Ok(entries) = std::fs::read_dir(&at) else {
            continue;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            let name = path.strip_prefix(&store).unwrap().to_path_buf();
            if path.is_dir() {
                found.push((name, None));
                walk.push(path);
            } else {
                found.push((name, Some(std::fs::read(&path).unwrap())));
            }
        }
    }
    found.sort();
    found
}

const MADE: [&str; 4] = ["daemon-state.json", "roots", "run", "store"];

fn second_start(dir: &Path) -> (Option<i32>, Duration) {
    let started = Instant::now();
    let status = service(dir).0.wait().unwrap();
    (status.code(), started.elapsed())
}

#[test]
fn a_second_service_run_exits_0_within_2_s_and_changes_nothing_in_the_directory() {
    let dir = kept_dir("service-tests-");
    let mut first = service(&dir);
    ready(&dir, &mut first);
    store_open(&dir, &mut first);
    let before = listing(&dir);
    let (status, took) = second_start(&dir);
    assert_eq!(status, Some(0));
    assert!(took < SECOND_START_WITHIN, "the second start took {took:?}");
    assert_eq!(listing(&dir), before);
    assert!(first.0.try_wait().unwrap().is_none(), "the first run ended");
    assert_eq!(terminate(first), Some(0));
    assert_eq!(names(&dir), MADE);
    assert_eq!(
        instance_lock::read(&data_dir(&dir)).unwrap(),
        LockRead::Free
    );
}

const TIMED: &str = "HALVO_DAEMON_TIMED_STORE_DIR";
const TIMED_RUNNER: &str = "daemon::process_tests::timed_run_for_parent";

struct WaitsForTheSignal(super::scripted::ScriptedRun);

impl super::RunSources for WaitsForTheSignal {
    fn start(
        &mut self,
        ctx: &crate::source::SourceContext,
        wanted: &[super::SourceName],
        hold_after: Option<Duration>,
    ) -> Vec<super::FailedSource> {
        self.0.start(ctx, wanted, hold_after)
    }

    fn wait(&mut self, window: Duration, stop: &AtomicBool) -> bool {
        let started = Instant::now();
        let _ = self.0.wait(Duration::ZERO, stop);
        while !stop.load(Ordering::SeqCst) && started.elapsed() < window {
            std::thread::sleep(Duration::from_millis(10));
        }
        stop.load(Ordering::SeqCst)
    }

    fn stop(&mut self) -> Vec<super::FailedSource> {
        self.0.stop()
    }

    fn figures(&self) -> super::SourceCounters {
        self.0.figures()
    }

    fn hold_refusal(&self) -> Option<super::DaemonError> {
        self.0.hold_refusal()
    }
}

#[test]
fn timed_run_for_parent() {
    use super::model_store::foreground_now;
    use super::scripted::{ScriptedRun, ScriptedSource};
    let Some(dir) = std::env::var_os(TIMED) else {
        return;
    };
    signal::prepare().unwrap();
    let run = super::TimedRun {
        window: Duration::from_secs(60),
        secure_input: super::SecureInput::DoNotHold,
        switches: super::SwitchFlags(Vec::new()),
        store_dir: PathBuf::from(dir),
    };
    let mut sources = WaitsForTheSignal(ScriptedRun::new(vec![(
        super::SourceName::Foreground,
        ScriptedSource::emitting(vec![foreground_now(), foreground_now()]),
    )]));
    let scratch = super::scratch_store(&run).unwrap();
    let answer = super::run_timed(
        &run,
        &mut sources,
        scratch.store,
        super::maintenance::SystemClocks::new(),
        signal::flag(),
        super::SETTLE_BOUND,
    );
    let outcome = super::end_of_a_scratch_run(answer, scratch.root).unwrap();
    let status = super::outcome_answer(&outcome, &mut std::io::stdout(), &mut std::io::stderr());
    std::process::exit(status);
}

#[test]
fn a_signalled_timed_run_prints_its_report_and_leaves_no_root_of_its_scratch_store() {
    use std::io::Read;
    let dir = kept_dir("timed-tests-");
    let mut child = Service(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", TIMED_RUNNER, "--test-threads=1"])
            .env(TIMED, &dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let started = Instant::now();
    while !(dir.join("store.key").exists() && names(&dir.join("roots")).len() == 1) {
        if let Some(status) = child.0.try_wait().unwrap() {
            panic!("the timed run ended before its store was made: {status}");
        }
        assert!(
            started.elapsed() < READY_WITHIN,
            "the timed run made no scratch store"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    std::thread::sleep(Duration::from_millis(500));
    let mut out = child.0.stdout.take().unwrap();
    assert_eq!(
        terminate(child),
        Some(halvo_control::exit::TIMED_RUN_INTERRUPTED)
    );
    let mut report = String::new();
    out.read_to_string(&mut report).unwrap();
    assert!(
        report.lines().any(|line| line == "segments_written=1"),
        "{report}"
    );
    assert!(
        report.lines().any(|line| line == "events_persisted=2"),
        "{report}"
    );
    assert!(
        dir.join("store.key").exists(),
        "the run removed its store's files"
    );
    assert_eq!(
        names(&dir.join("roots")),
        Vec::<String>::new(),
        "a root of the scratch store stays"
    );
}

const KEYLESS_LINE: &str = "halvo-capture: store=unavailable store_error=obstructed: store.key is missing while the store's database or segments stand; no store was made over them";

fn tree_bytes(root: &Path) -> StoreBytes {
    let mut found = Vec::new();
    let mut walk = vec![root.to_path_buf()];
    while let Some(at) = walk.pop() {
        for entry in std::fs::read_dir(&at).unwrap() {
            let path = entry.unwrap().path();
            let name = path.strip_prefix(root).unwrap().to_path_buf();
            if path.is_dir() {
                found.push((name, None));
                walk.push(path);
            } else {
                found.push((name, Some(std::fs::read(&path).unwrap())));
            }
        }
    }
    found.sort();
    found
}

const MADE_AGAIN_LINE: &str = "halvo-capture: an unfinished create of the store stood, with no key file and no segment; the store was made again";

fn run_until(dir: &Path, done: impl Fn(&[String]) -> bool) -> Vec<String> {
    use std::io::BufRead;
    let mut run = Service(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", SERVER, "--test-threads=1"])
            .env(SERVE, dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let (lines, read) = std::sync::mpsc::channel();
    let err = run.0.stderr.take().unwrap();
    let reader = std::thread::spawn(move || {
        for line in std::io::BufReader::new(err).lines() {
            let Ok(line) = line else { return };
            let _ = lines.send(line);
        }
    });
    let started = Instant::now();
    let mut seen = Vec::new();
    while !done(&seen) {
        assert!(
            run.0.try_wait().unwrap().is_none(),
            "the run ended: {seen:?}"
        );
        assert!(
            started.elapsed() < READY_WITHIN,
            "the run never got there: {seen:?}"
        );
        seen.extend(read.recv_timeout(Duration::from_millis(10)));
    }
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(terminate(run), Some(0));
    reader.join().unwrap();
    seen.extend(read.try_iter());
    seen
}

#[test]
fn a_start_with_no_key_file_beside_a_standing_store_makes_nothing_and_says_why() {
    use super::custody_policy::Custody;
    use super::port::{
        CaptureStore, DataDirStore, EntryKind, KindEntry, Opened, SegmentId, SegmentWrite,
        StoreHost,
    };
    let dir = kept_dir("service-tests-");
    let Opened::ReadWrite { mut store, .. } = DataDirStore::at(&dir).create(Custody::RootFile)
    else {
        panic!("the first life made no store");
    };
    let id = SegmentId::mint().unwrap();
    let entries = [KindEntry {
        kind: EntryKind::of(crate::event::Modality::ForegroundApp),
        events: 1,
        bytes: 3,
    }];
    let day = halvo_control::model::UtcDay::from_days_since_epoch(20_000).unwrap();
    let at = 20_000 * 86_400 * 1_000_000_000_i64;
    store
        .write_batch(SegmentWrite::new(&id, day, at, at, &entries, b"{}\n"))
        .unwrap();
    drop(store);
    let store_dir = dir.join("store");
    std::fs::remove_file(store_dir.join("store.key")).unwrap();
    assert!(
        !dir.join("daemon-state.json").exists(),
        "the first life marked its store"
    );
    let database = std::fs::read(store_dir.join("store.db")).unwrap();
    let segments = tree_bytes(&store_dir.join("segments"));
    assert!(
        segments.iter().any(|(_, bytes)| bytes.is_some()),
        "no segment stands"
    );
    let roots = names(&dir.join("roots"));
    let seen = run_until(&dir, |seen| {
        seen.iter().any(|line| line == KEYLESS_LINE) || store_dir.join("store.key").exists()
    });
    assert!(
        !store_dir.join("store.key").exists(),
        "a new store was made over the standing one"
    );
    assert!(
        std::fs::read(store_dir.join("store.db")).unwrap() == database,
        "the standing database changed"
    );
    assert!(
        tree_bytes(&store_dir.join("segments")) == segments,
        "the standing segments changed"
    );
    assert_eq!(names(&dir.join("roots")), roots, "a new root was made");
    assert_eq!(
        seen.iter().filter(|line| *line == KEYLESS_LINE).count(),
        1,
        "{seen:?}"
    );

    let cut = kept_dir("service-tests-");
    let Opened::ReadWrite { store, .. } = DataDirStore::at(&cut).create(Custody::RootFile) else {
        panic!("the cut life made no store");
    };
    drop(store);
    let cut_store = cut.join("store");
    let root = names(&cut.join("roots")).remove(0);
    let root_id = root.strip_suffix(".root").unwrap();
    std::fs::write(
        cut_store.join("create.pending"),
        format!("{{\"custody_kind\":\"dev_fallback\",\"root_id\":\"{root_id}\"}}"),
    )
    .unwrap();
    std::fs::remove_file(cut_store.join("store.key")).unwrap();
    assert!(cut_store.join("store.db").exists() && !cut_store.join("segments").exists());
    let seen = run_until(&cut, |seen| {
        seen.iter().any(|line| line == MADE_AGAIN_LINE) && cut_store.join("store.key").exists()
    });
    assert!(cut_store.join("store.key").exists(), "no store was made");
    assert!(
        !cut_store.join("create.pending").exists(),
        "the marker stays"
    );
    assert_eq!(
        seen.iter().filter(|line| *line == MADE_AGAIN_LINE).count(),
        1,
        "{seen:?}"
    );
    assert!(!seen.iter().any(|line| line == KEYLESS_LINE), "{seen:?}");
}

const DIR_SHUT_LINE: &str = "halvo-capture: store=unavailable store_error=obstructed: the data directory failed its check; no store was made or opened in it";

#[test]
fn a_data_directory_that_fails_its_check_gets_no_store_and_no_root() {
    use std::os::unix::fs::PermissionsExt;
    let root = kept_dir("service-tests-");
    let folder = root.join("open-folder");
    std::fs::create_dir(&folder).unwrap();
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();
    let linked = root.join("data");
    std::os::unix::fs::symlink(&folder, &linked).unwrap();
    let started = Instant::now();
    let seen = run_until(&linked, |_| {
        started.elapsed() > Duration::from_millis(2_500)
            || folder.join("store").exists()
            || folder.join("roots").exists()
    });
    assert!(
        seen.iter().any(
            |line| line == "halvo-capture: data_dir=obstructed cause=linked_and_open_to_others"
        ),
        "{seen:?}"
    );
    assert!(!folder.join("store").exists(), "a store was made: {seen:?}");
    assert!(!folder.join("roots").exists(), "a root was made: {seen:?}");
    assert_eq!(
        seen.iter().filter(|line| *line == DIR_SHUT_LINE).count(),
        1,
        "{seen:?}"
    );
}

#[test]
fn a_second_service_run_changes_no_byte_of_the_store_that_the_first_holds() {
    let dir = kept_dir("service-tests-");
    let mut first = service(&dir);
    store_open(&dir, &mut first);
    let before = store_bytes(&dir);
    assert!(
        before[..2].iter().all(|(_, bytes)| bytes.is_some()),
        "no store.key or no store.db stands"
    );
    let (status, took) = second_start(&dir);
    assert_eq!(status, Some(0));
    assert!(took < SECOND_START_WITHIN, "the second start took {took:?}");
    assert!(
        store_bytes(&dir) == before,
        "the second start changed a byte of the store"
    );
    assert!(first.0.try_wait().unwrap().is_none(), "the first run ended");
    assert_eq!(terminate(first), Some(0));
}

#[test]
fn a_sigterm_to_a_service_run_with_an_open_store_exits_0_in_the_budget_and_the_store_opens_again() {
    let dir = kept_dir("service-tests-");
    let mut first = service(&dir);
    store_open(&dir, &mut first);
    let key = std::fs::read(dir.join("store").join("store.key")).unwrap();
    let roots = names(&dir.join("roots"));
    assert_eq!(roots.len(), 1, "{roots:?}");
    let signalled = Instant::now();
    let status = terminate(first);
    let took = signalled.elapsed();
    assert_eq!(status, Some(0));
    assert!(
        took < Duration::from_millis(super::supervisor::SHUTDOWN_BUDGET_MS),
        "the run ended {took:?} after SIGTERM"
    );
    assert_eq!(
        instance_lock::read(&data_dir(&dir.join("store"))).unwrap(),
        LockRead::Free,
        "the store stays held after the run ended"
    );
    let mut again = service(&dir);
    store_open(&dir, &mut again);
    assert!(
        std::fs::read(dir.join("store").join("store.key")).unwrap() == key,
        "the next start made a second key file"
    );
    assert_eq!(
        names(&dir.join("roots")),
        roots,
        "the next start made a second root"
    );
    assert_eq!(terminate(again), Some(0));
}

#[test]
fn a_service_run_without_a_state_file_writes_the_defaults_after_the_lock() {
    let dir = kept_dir("service-tests-");
    let held = InstanceLock::take(&data_dir(&dir)).unwrap().unwrap();
    let (status, took) = second_start(&dir);
    assert_eq!(status, Some(0));
    assert!(took < SECOND_START_WITHIN, "the held start took {took:?}");
    assert!(names(&dir).is_empty(), "{:?}", names(&dir));
    drop(held);
    let mut run = service(&dir);
    ready(&dir, &mut run);
    store_open(&dir, &mut run);
    assert_eq!(terminate(run), Some(0));
    let loaded = StateFile::open(
        StateDir::open(&dir).unwrap(),
        Box::new(TestWrite::default()),
    );
    assert_eq!(
        loaded.state(),
        &DaemonState {
            store_made: true,
            ..DaemonState::default()
        }
    );
    assert_eq!(names(&dir), MADE);
}

#[test]
fn a_second_service_run_while_a_loop_unlinks_daemon_lock_exits_0_and_the_first_still_holds() {
    let dir = kept_dir("service-tests-");
    let mut first = service(&dir);
    ready(&dir, &mut first);
    let done = Arc::new(AtomicBool::new(false));
    let unlinks = {
        let done = done.clone();
        let lock_file = dir.join("daemon.lock");
        std::thread::spawn(move || {
            let mut count = 0u64;
            while !done.load(Ordering::SeqCst) {
                std::fs::write(&lock_file, b"").unwrap();
                std::fs::remove_file(&lock_file).unwrap();
                count += 1;
            }
            count
        })
    };
    let (status, took) = second_start(&dir);
    done.store(true, Ordering::SeqCst);
    assert!(unlinks.join().unwrap() > 0);
    assert_eq!(status, Some(0));
    assert!(took < SECOND_START_WITHIN, "the second start took {took:?}");
    assert!(first.0.try_wait().unwrap().is_none(), "the first run ended");
    assert!(
        halvo_control::instance_lock::InstanceLock::try_take(&data_dir(&dir))
            .unwrap()
            .is_none()
    );
    assert_eq!(terminate(first), Some(0));
}

#[test]
fn the_backup_mark_is_on_the_directory_within_2_s_of_the_start() {
    let dir = kept_dir("service-tests-");
    let started = Instant::now();
    let mut run = service(&dir);
    while backup_mark(&dir, false).is_none() && started.elapsed() < SECOND_START_WITHIN {
        assert!(run.0.try_wait().unwrap().is_none(), "the run ended");
        std::thread::sleep(Duration::from_millis(5));
    }
    let took = started.elapsed();
    let mark = backup_mark(&dir, false);
    ready(&dir, &mut run);
    assert_eq!(terminate(run), Some(0));
    assert!(mark.is_some(), "no mark within {took:?}");
}

struct Recorded;

impl main_loop::EndCall for Recorded {
    fn end(&self, status: i32) {
        panic!("the service run made the ending call with {status}");
    }
}

fn serve_here(dir: &Path) -> (i32, String) {
    serve_with(dir, Box::new(TestWrite::default()))
}

fn serve_with(dir: &Path, writer: Box<dyn StateWrite>) -> (i32, String) {
    let stop = AtomicBool::new(true);
    let mut err = Vec::new();
    let status = serve(
        &data_dir(dir),
        writer,
        SleepLoop(Instant::now()),
        &stop,
        Arc::new(Recorded),
        &mut err,
    );
    (status, String::from_utf8(err).unwrap())
}

#[test]
fn a_missing_data_directory_is_made_with_mode_0700_and_an_open_one_is_set_to_0700() {
    use std::os::unix::fs::PermissionsExt;
    let root = kept_dir("service-tests-");
    let missing = root.join("below").join("data");
    assert_eq!(serve_here(&missing), (0, String::new()));
    let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&missing), 0o700);
    assert_eq!(mode(&root.join("below")), 0o700);
    assert_eq!(
        names(&missing),
        vec!["daemon-state.json".to_owned(), "run".to_owned()]
    );
    let open = root.join("open");
    std::fs::create_dir(&open).unwrap();
    std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(serve_here(&open), (0, String::new()));
    assert_eq!(mode(&open), 0o700);
    assert!(backup_mark(&open, false).is_some());
}

#[test]
fn a_data_directory_that_leads_nowhere_or_is_a_file_exits_1_and_makes_nothing() {
    let root = kept_dir("service-tests-");
    let link = root.join("link");
    std::os::unix::fs::symlink(root.join("gone"), &link).unwrap();
    let (status, err) = serve_here(&link);
    assert_eq!(status, 1);
    assert!(err.contains("link that leads nowhere"), "{err}");
    let file = root.join("file");
    std::fs::write(&file, b"a shortcut").unwrap();
    let (status, err) = serve_here(&file);
    assert_eq!(status, 1);
    assert!(err.contains("no directory"), "{err}");
    assert_eq!(names(&root), vec!["file".to_owned(), "link".to_owned()]);
    assert_eq!(std::fs::read(&file).unwrap(), b"a shortcut");
}

#[test]
fn a_state_file_that_does_not_load_is_named_by_a_line_and_left_as_it_is() {
    let dir = kept_dir("service-tests-");
    let path = dir.join("daemon-state.json");
    std::fs::write(&path, b"{\"version\":7}").unwrap();
    let (status, err) = serve_here(&dir);
    assert_eq!(status, 0);
    assert_eq!(
        err,
        "halvo-capture: state_file=newer_version version=7; capture stays paused and the file is left as it is\n"
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"{\"version\":7}");
    std::fs::write(&path, b"{").unwrap();
    let (_, err) = serve_here(&dir);
    assert!(
        err.starts_with("halvo-capture: state_file=corrupt;"),
        "{err}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"{");
}

#[test]
fn each_fault_of_the_data_directory_check_has_a_line_of_its_own() {
    use halvo_control::paths::{DataDirFault, EntryAtDataDir};
    let faults = [
        DataDirFault::Entry(EntryAtDataDir::LinkLeadsNowhere),
        DataDirFault::Entry(EntryAtDataDir::NotADirectory),
        DataDirFault::Entry(EntryAtDataDir::CannotBeRead),
        DataDirFault::OtherOwner,
        DataDirFault::LinkedAndOpenToOthers,
        DataDirFault::Io(std::io::ErrorKind::PermissionDenied),
        DataDirFault::OwnOnAVolumeThatIgnoresOwnership,
        DataDirFault::LinkedToAVolumeThatIgnoresOwnership,
        DataDirFault::ModeDidNotTake,
    ];
    let mut lines: Vec<String> = faults.into_iter().map(super::obstructed_line).collect();
    assert!(lines
        .iter()
        .all(|line| line.starts_with("halvo-capture: data_dir=obstructed cause=")));
    assert_eq!(
        lines[5],
        "halvo-capture: data_dir=obstructed cause=io error=permission denied"
    );
    lines.sort();
    lines.dedup();
    assert_eq!(lines.len(), 9);
}

struct LongLoop {
    now: Arc<std::sync::Mutex<Duration>>,
    stop: Arc<AtomicBool>,
}

const STOP_AT: Duration = Duration::from_secs(1_000);

impl MainLoop for LongLoop {
    fn turn(&mut self, wait: Duration) {
        if self.stop.load(Ordering::SeqCst) {
            std::thread::sleep(wait);
        }
        let mut now = self.now.lock().unwrap();
        *now += wait;
        if *now >= STOP_AT {
            self.stop.store(true, Ordering::SeqCst);
        }
    }

    fn now(&self) -> Duration {
        *self.now.lock().unwrap()
    }
}

#[test]
fn the_wait_goes_on_past_its_slices_until_the_stop_flag() {
    let dir = kept_dir("service-tests-");
    let stop = Arc::new(AtomicBool::new(false));
    let now = Arc::new(std::sync::Mutex::new(Duration::ZERO));
    let mut err = Vec::new();
    let status = serve(
        &data_dir(&dir),
        Box::new(TestWrite::default()),
        LongLoop {
            now: now.clone(),
            stop: stop.clone(),
        },
        &stop,
        Arc::new(Recorded),
        &mut err,
    );
    assert_eq!(status, 0, "{}", String::from_utf8_lossy(&err));
    assert!(
        *now.lock().unwrap() >= STOP_AT,
        "the wait ended at {:?}, before the stop flag",
        now.lock().unwrap()
    );
}

struct ModeBack(PathBuf);

impl Drop for ModeBack {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o700));
    }
}

fn shut_folder(root: &Path, mode: u32) -> (PathBuf, ModeBack) {
    use std::os::unix::fs::PermissionsExt;
    let shut = root.join("shut");
    std::fs::create_dir(&shut).unwrap();
    let back = ModeBack(shut.clone());
    std::fs::set_permissions(&shut, std::fs::Permissions::from_mode(mode)).unwrap();
    (shut, back)
}

#[test]
fn a_data_directory_that_cannot_be_made_ends_the_run_with_1_and_the_line_of_its_kind() {
    let root = kept_dir("service-tests-");
    let (shut, back) = shut_folder(&root, 0o500);
    let missing = shut.join("data");
    let (status, err) = serve_here(&missing);
    let line = format!(
        "halvo-capture: the data directory could not be made: {}\n",
        std::io::ErrorKind::PermissionDenied
    );
    assert_eq!((status, err.as_str()), (1, line.as_str()));
    drop(back);
    assert!(!missing.exists());
}

#[test]
fn a_data_directory_that_cannot_be_opened_ends_the_run_with_1_and_one_line_that_holds_no_path() {
    let root = kept_dir("service-tests-");
    let (shut, back) = shut_folder(&root, 0o000);
    let (status, err) = serve_here(&shut);
    assert_eq!(status, 1, "{err}");
    assert_eq!(err.lines().count(), 1, "{err}");
    assert!(err.starts_with("halvo-capture: "), "{err}");
    assert!(!err.contains(root.to_str().unwrap()), "{err}");
    drop(back);
    assert!(names(&shut).is_empty(), "{:?}", names(&shut));
}

#[test]
fn a_start_on_a_held_directory_answers_0_prints_nothing_and_changes_nothing() {
    let dir = kept_dir("service-tests-");
    let held = InstanceLock::take(&data_dir(&dir)).unwrap().unwrap();
    let before = listing(&dir);
    let (status, err) = serve_here(&dir);
    assert_eq!((status, err.as_str()), (0, ""));
    assert_eq!(listing(&dir), before);
    assert!(names(&dir).is_empty(), "{:?}", names(&dir));
    assert!(backup_mark(&dir, false).is_none());
    drop(held);
}

#[test]
fn a_missing_data_directory_below_a_link_that_leads_nowhere_ends_the_run_with_the_line_of_that_link(
) {
    let root = kept_dir("service-tests-");
    let away = root.join("away");
    std::os::unix::fs::symlink(root.join("gone"), &away).unwrap();
    let missing = away.join("data");
    let (status, err) = serve_here(&missing);
    assert_eq!(
        (status, err.as_str()),
        (
            1,
            "halvo-capture: the data directory's path meets a link that leads nowhere\n"
        )
    );
    assert!(!root.join("gone").exists());
}

#[test]
fn a_first_state_write_that_fails_is_named_by_one_line_with_its_kind() {
    let dir = kept_dir("service-tests-");
    let writer = TestWrite::default();
    writer.failing.store(true, Ordering::SeqCst);
    let (status, err) = serve_with(&dir, Box::new(writer.clone()));
    let line = format!(
        "halvo-capture: state_file=write_failed error={}\n",
        std::io::ErrorKind::StorageFull
    );
    assert_eq!((status, err), (0, line));
    assert_eq!(writer.calls.load(Ordering::SeqCst), 1);
}

struct SearchBack(PathBuf);

impl Drop for SearchBack {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o700));
    }
}

#[test]
fn a_held_service_run_whose_folder_above_cannot_be_searched_cannot_bind_and_exits_1_writing_nothing(
) {
    use std::os::unix::fs::PermissionsExt;
    let root = kept_dir("service-tests-");
    let above = root.join("above");
    let data = above.join("data");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o700)).unwrap();
    let named = data_dir(&data);
    let lock = InstanceLock::take(&named).unwrap().unwrap();
    let back = SearchBack(above.clone());
    std::fs::set_permissions(&above, std::fs::Permissions::from_mode(0o000)).unwrap();
    assert!(
        std::fs::metadata(&data).is_err(),
        "the folder above the data directory still lets its name be walked, so the case is not made here"
    );
    let stop = AtomicBool::new(true);
    let mut err = Vec::new();
    let status = serve_held(
        lock,
        &named,
        Box::new(TestWrite::default()),
        SleepLoop(Instant::now()),
        &stop,
        Arc::new(Recorded),
        &mut err,
    );
    drop(back);
    let err = String::from_utf8(err).unwrap();
    assert_eq!(status, 1, "{err}");
    assert!(
        err.ends_with("halvo-capture: the control channel could not be bound: permission denied\n"),
        "{err}"
    );
    assert!(names(&data).is_empty(), "{:?}", names(&data));
}

const SECOND: Duration = Duration::from_secs(1);
const DAY: Duration = Duration::from_secs(86_400);

fn made_store(dir: &Path) -> Box<dyn CaptureStore + Send> {
    let Opened::ReadWrite { store, .. } = DataDirStore::at(dir).create(Custody::RootFile) else {
        panic!("no store was made in {}", dir.display());
    };
    store
}

fn segment_at<S: CaptureStore + ?Sized>(store: &mut S, at: SystemTime, payload: &[u8]) {
    let id = SegmentId::mint().unwrap();
    let nanos = i64::try_from(at.duration_since(UNIX_EPOCH).unwrap().as_nanos()).unwrap();
    let day = UtcDay::from_days_since_epoch(nanos.div_euclid(86_400_000_000_000)).unwrap();
    let entries = [KindEntry {
        kind: EntryKind::of(Modality::ForegroundApp),
        events: 1,
        bytes: payload.len() as u64,
    }];
    store
        .write_batch(SegmentWrite::new(&id, day, nanos, nanos, &entries, payload))
        .unwrap();
}

fn owner_over<S: CaptureStore>(
    store: S,
    clocks: &ManualClocks,
    cap: Option<ByteBound>,
) -> (Owner<S, ManualClocks>, OwnerHandle, SinkHandle) {
    let (sink, writer) = crate::sink::channel(64);
    let (owner, handle, _reports) = Owner::open(OwnerParts {
        writer,
        store,
        clocks: clocks.clone(),
        floor: open_floor(),
        cap,
        shred_chunk: SHRED_CHUNK,
    });
    (owner, handle, sink)
}

#[test]
fn a_record_past_its_window_is_unreadable_within_one_deadline_on_the_real_store() {
    let dir = kept_dir("upkeep-tests-");
    let mut store = made_store(&dir);
    segment_at(&mut *store, noon() - 15 * DAY, b"fifteen days\n");
    segment_at(
        &mut *store,
        noon() - 14 * DAY + 30 * SECOND,
        b"due 30 s after the open\n",
    );
    segment_at(&mut *store, noon() - 13 * DAY, b"thirteen days\n");
    let clocks = ManualClocks::at(noon());
    let (mut owner, _handle, _sink) =
        owner_over(store, &clocks, Some(ByteBound::of_cap(StorageCap::DEFAULT)));
    clocks.advance(MAINTENANCE_INTERVAL + SECOND);
    assert!(owner.pass(Duration::ZERO), "the owner stopped");
    let stamped = owner.snapshot().last_expiry_at;
    drop(owner);
    assert_eq!(
        DataDirStore::at(&dir).read_back(),
        [None, None, Some(b"thirteen days\n".to_vec())],
        "after the open's pass and one deadline, a segment past its window still decrypts or the younger does not"
    );
    assert_eq!(
        stamped,
        Some(clocks.wall()),
        "last_expiry_at is not the deadline's wall time"
    );
}

type Expiry = Result<(usize, bool), MaintFault>;

#[derive(Clone, Default)]
struct Expiries(Arc<Mutex<Vec<Expiry>>>);

impl Expiries {
    fn taken(&self) -> Vec<Expiry> {
        self.0.lock().unwrap().clone()
    }
}

struct Counted<S> {
    store: S,
    expiries: Expiries,
}

impl<S: MaintainStore> MaintainStore for Counted<S> {
    fn expire_due(&mut self, now: SystemTime, max_segments: usize) -> Result<Pass, MaintFault> {
        let answer = self.store.expire_due(now, max_segments);
        self.expiries.0.lock().unwrap().push(
            answer
                .as_ref()
                .map(|pass| (pass.shredded.len(), pass.more_due))
                .map_err(|fault| *fault),
        );
        answer
    }

    fn evict_to(&mut self, bound: ByteBound, max_segments: usize) -> Result<Pass, MaintFault> {
        self.store.evict_to(bound, max_segments)
    }

    fn usage(&mut self) -> Result<Usage, MaintFault> {
        self.store.usage()
    }
}

impl<S: CaptureStore> CaptureStore for Counted<S> {
    fn write_batch(&mut self, write: SegmentWrite<'_>) -> Result<Written, WriteFault> {
        self.store.write_batch(write)
    }

    fn volume_free_bytes(&self) -> io::Result<u64> {
        self.store.volume_free_bytes()
    }
}

#[test]
fn a_start_over_a_backlog_expires_all_of_it_in_chunks() {
    let dir = kept_dir("upkeep-tests-");
    let mut store = made_store(&dir);
    for _ in 0..65 {
        segment_at(&mut *store, noon() - 15 * DAY, b"due\n");
    }
    let expiries = Expiries::default();
    let counted = Counted {
        store,
        expiries: expiries.clone(),
    };
    let clocks = ManualClocks::at(noon());
    let (mut owner, _handle, _sink) = owner_over(
        counted,
        &clocks,
        Some(ByteBound::of_cap(StorageCap::DEFAULT)),
    );
    for _ in 0..3 {
        assert!(owner.pass(Duration::ZERO), "the owner stopped");
    }
    drop(owner);
    assert_eq!(
        expiries.taken(),
        [Ok((SHRED_CHUNK, true)), Ok((1, false))],
        "the start did not expire the backlog of 65 in a chunk of 64 at the open and one more in the next loop pass"
    );
    let read = DataDirStore::at(&dir).read_back();
    assert_eq!(read.len(), 65);
    assert!(
        read.iter().all(Option::is_none),
        "a segment of the backlog still decrypts"
    );
}

fn asked<S: CaptureStore>(
    owner: &mut Owner<S, ManualClocks>,
    handle: &OwnerHandle,
    command: super::owner::Command,
) -> Answer {
    let pending = handle.ask(command).unwrap();
    assert!(owner.pass(Duration::ZERO), "the owner stopped");
    pending
        .try_answer()
        .expect("the owner did not answer in one pass")
}

fn flushed_once<S: CaptureStore>(
    owner: &mut Owner<S, ManualClocks>,
    handle: &OwnerHandle,
    sink: &SinkHandle,
) {
    assert!(matches!(
        asked(owner, handle, super::owner::Command::ClosePause),
        Answer::Resumed(_)
    ));
    assert_eq!(
        sink.emit_event(foreground(&AT_NOON, Duration::ZERO)),
        Emitted::Queued
    );
    assert!(matches!(
        asked(owner, handle, super::owner::Command::Settle),
        Answer::Settled(_)
    ));
    for _ in 0..2 {
        assert!(owner.pass(Duration::ZERO), "the owner stopped");
    }
}

fn reopened(dir: &Path) -> Box<dyn CaptureStore + Send> {
    let Opened::ReadWrite { store, .. } = DataDirStore::at(dir).open(Access::ReadWrite) else {
        panic!("the store did not open again in {}", dir.display());
    };
    store
}

fn holds_the_flushed_record(segment: &Option<Vec<u8>>) -> bool {
    segment
        .as_ref()
        .is_some_and(|plaintext| plaintext.windows(APP.len()).any(|at| at == APP.as_bytes()))
}

#[test]
fn an_eviction_after_a_flush_brings_the_disk_total_under_the_cap() {
    let old = vec![b'o'; 256 * 1024];
    let dir = kept_dir("upkeep-tests-");
    let mut store = made_store(&dir);
    for hours in (1..=4).rev() {
        segment_at(&mut *store, noon() - hours * 3600 * SECOND, &old);
    }
    let at_the_start = store.usage().unwrap().disk_bytes;
    let bound = ByteBound::raw(at_the_start);
    let clocks = ManualClocks::at(noon());
    let (mut owner, handle, sink) = owner_over(store, &clocks, Some(bound));
    flushed_once(&mut owner, &handle, &sink);
    let after_the_flush = owner.snapshot().disk_bytes;
    drop(owner);
    let read = DataDirStore::at(&dir).read_back();
    assert_eq!(read.len(), 5, "the flush stored no segment");
    assert!(
        read[0].is_none(),
        "the oldest segment still decrypts after a flush took the total over the bound"
    );
    assert!(
        read[1..4].iter().all(Option::is_some) && holds_the_flushed_record(&read[4]),
        "eviction went on past the bound or took the youngest segment"
    );
    assert!(
        after_the_flush.is_some_and(|disk| disk <= bound.bytes()),
        "the disk total after the eviction is {after_the_flush:?}, over the bound {bound:?}"
    );

    let mut store = reopened(&dir);
    let under = ByteBound::raw(store.usage().unwrap().disk_bytes - 1);
    let (mut owner, _handle, _sink) = owner_over(store, &clocks, Some(under));
    for _ in 0..2 {
        assert!(owner.pass(Duration::ZERO), "the owner stopped");
    }
    let after_the_start = owner.snapshot().disk_bytes;
    drop(owner);
    let read = DataDirStore::at(&dir).read_back();
    assert!(
        read[1].is_none() && read[2..].iter().all(Option::is_some),
        "a start over the bound did not evict the oldest live segment alone"
    );
    assert!(
        after_the_start.is_some_and(|disk| disk <= under.bytes()),
        "the disk total after the start's eviction is {after_the_start:?}, over the bound {under:?}"
    );

    let unknown = super::cap_bound(Cap::OutOfRange(StorageCap::MAX.bytes() + 1));
    assert_eq!(unknown, None, "a stored cap out of range gave a bound");
    let (mut owner, handle, sink) = owner_over(reopened(&dir), &clocks, unknown);
    flushed_once(&mut owner, &handle, &sink);
    drop(owner);
    let read = DataDirStore::at(&dir).read_back();
    assert_eq!(read.len(), 6, "the flush stored no segment");
    assert!(
        read[2..].iter().all(Option::is_some),
        "a segment was shredded under a stored cap out of range"
    );
}

#[test]
fn a_maintenance_only_store_expires_and_takes_no_batch() {
    let dir = kept_dir("upkeep-tests-");
    let mut store = made_store(&dir);
    segment_at(&mut *store, noon() - 15 * DAY, b"due\n");
    segment_at(&mut *store, noon() - DAY, b"kept\n");
    drop(store);
    let (slot, held) = try_open(
        &DataDirStore::at(&dir),
        |recorded| select(true, System::Macos, recorded),
        CreateRule::StoreMade,
        SECOND,
    );
    let Some(Held::Maintenance { store, .. }) = held else {
        panic!("the policy did not open the store for maintenance alone: {slot:?}");
    };
    let clocks = ManualClocks::at(noon());
    let mut upkeep = Upkeep::new(
        store,
        clocks.clone(),
        Some(ByteBound::of_cap(StorageCap::DEFAULT)),
    );
    let mut waits = vec![upkeep.wait()];
    for _ in 0..2 {
        upkeep.turn();
        waits.push(upkeep.wait());
        clocks.advance(MAINTENANCE_INTERVAL);
    }
    drop(upkeep);
    assert_eq!(
        DataDirStore::at(&dir).read_back(),
        [None, Some(b"kept\n".to_vec())],
        "the pass on a maintenance handle left its due segment, or the store holds a segment that it did not hold before"
    );
    assert_eq!(
        waits,
        [Duration::ZERO, MAINTENANCE_INTERVAL, MAINTENANCE_INTERVAL],
        "the pass was not due at the start and then every 60 s"
    );
}

struct Above<S> {
    store: S,
    evictions: Arc<Mutex<Vec<ByteBound>>>,
}

const ABOVE: u64 = StorageCap::DEFAULT.bytes();

impl<S: MaintainStore> MaintainStore for Above<S> {
    fn expire_due(&mut self, now: SystemTime, max_segments: usize) -> Result<Pass, MaintFault> {
        self.store.expire_due(now, max_segments)
    }

    fn evict_to(&mut self, bound: ByteBound, max_segments: usize) -> Result<Pass, MaintFault> {
        self.evictions.lock().unwrap().push(bound);
        self.store.evict_to(
            ByteBound::raw(bound.bytes().saturating_sub(ABOVE)),
            max_segments,
        )
    }

    fn usage(&mut self) -> Result<Usage, MaintFault> {
        self.store.usage().map(|usage| Usage {
            disk_bytes: usage.disk_bytes + ABOVE,
            logical_bytes: usage.logical_bytes,
        })
    }
}

impl<S: CaptureStore> CaptureStore for Above<S> {
    fn write_batch(&mut self, write: SegmentWrite<'_>) -> Result<Written, WriteFault> {
        self.store.write_batch(write)
    }

    fn volume_free_bytes(&self) -> io::Result<u64> {
        self.store.volume_free_bytes()
    }
}

#[test]
fn a_state_file_that_does_not_load_gives_no_bound_until_a_valid_cap_is_set() {
    let old = vec![b'o'; 256 * 1024];
    let clocks = ManualClocks::at(noon());
    let unloaded = b"{\"storage_cap_bytes\":".to_vec();
    let out_of_range =
        format!("{{\"storage_cap_bytes\":{}}}", StorageCap::MAX.bytes() + 1).into_bytes();
    for (stored, loads) in [(unloaded, false), (out_of_range, true)] {
        let dir = kept_dir("cap-tests-");
        let mut store = made_store(&dir);
        for hours in (1..=4).rev() {
            segment_at(&mut *store, noon() - hours * 3600 * SECOND, &old);
        }
        let file = dir.join(halvo_control::paths::STATE_FILE);
        std::fs::write(&file, &stored).unwrap();
        let mut state =
            StateFile::open(StateDir::open(&dir).unwrap(), super::DurableState::writer());
        assert!(
            !matches!(state.state().storage_cap, Cap::Known(_)),
            "loads={loads}: the cap reads {:?}, a known cap",
            state.state().storage_cap
        );
        let bound = super::cap_bound(state.state().storage_cap);
        assert_eq!(bound, None, "loads={loads}: the state file gave a bound");
        let evictions = Arc::new(Mutex::new(Vec::new()));
        let above = Above {
            store,
            evictions: evictions.clone(),
        };
        let (mut owner, handle, sink) = owner_over(above, &clocks, bound);
        flushed_once(&mut owner, &handle, &sink);
        let disk = owner.snapshot().disk_bytes.unwrap();
        assert!(
            disk > ABOVE,
            "the stand-in store stands at {disk}, not above the default cap"
        );
        assert_eq!(
            *evictions.lock().unwrap(),
            [],
            "loads={loads}: a life with no bound ran an eviction"
        );

        let cap = StorageCap::from_bytes(disk - 1).unwrap();
        let (written, bound) = super::set_cap(&mut state, cap);
        let answer = super::cap_set_answer(&written, cap, Some(disk));
        if loads {
            assert!(
                matches!(answer, Ok(true)),
                "a set-cap below the store's bytes answered {answer:?}, not evicting yes"
            );
            assert_eq!(bound, Some(ByteBound::of_cap(cap)));
        } else {
            let code = answer.map_err(|refused| halvo_control::proto::Reply::Error(refused).code());
            assert_eq!(
                code,
                Err(Some(halvo_control::proto::ErrorCode::StateFile)),
                "a set-cap on a state file that does not load did not answer state_file"
            );
            assert_eq!(
                bound, None,
                "a set-cap on a state file that does not load gave a bound"
            );
            assert_eq!(
                std::fs::read(&file).unwrap(),
                stored,
                "a set-cap wrote into a state file that does not load"
            );
            assert!(!matches!(state.state().storage_cap, Cap::Known(_)));
        }
        assert!(matches!(
            asked(&mut owner, &handle, super::owner::Command::SetBound(bound)),
            Answer::BoundSet
        ));
        for _ in 0..2 {
            assert!(owner.pass(Duration::ZERO), "the owner stopped");
        }
        let after = owner.snapshot().disk_bytes;
        drop(owner);
        let read = DataDirStore::at(&dir).read_back();
        assert_eq!(read.len(), 5, "the flush stored no segment");
        if loads {
            assert_eq!(
                evictions.lock().unwrap().first(),
                Some(&ByteBound::of_cap(cap)),
                "the pass after the set-cap did not evict to the new bound"
            );
            assert!(
                read[0].is_none() && read[1..].iter().all(Option::is_some),
                "the pass after the set-cap did not evict the oldest segment alone"
            );
            assert!(
                after.is_some_and(|disk| disk <= cap.bytes()),
                "the disk total after the set-cap is {after:?}, over the cap {cap:?}"
            );
        } else {
            assert_eq!(
                *evictions.lock().unwrap(),
                [],
                "a life whose state file does not load ran an eviction after a set-cap"
            );
            assert!(
                read.iter().all(Option::is_some),
                "a segment was shredded in a life whose state file does not load"
            );
        }
    }
}

#[test]
fn a_wait_with_no_window_reads_the_stop_flag_and_serves_a_queued_request() {
    let dir = kept_dir("wait-tests-");
    let listening = super::server::unix::Listening::bind(&data_dir(&dir)).unwrap();
    let facts = super::server::RunFacts {
        started_at: halvo_control::model::UtcSecond::from_unix_seconds(0).unwrap(),
        parent: halvo_control::proto::Parent::None,
        last_stop: halvo_control::proto::LastStop::None,
        rss_peak_bytes: 0,
    };
    let serving = super::Serving {
        listening: &listening,
        facts: &facts,
        obstructed: None,
    };
    let mut supervisor = super::supervisor::Supervisor::new(
        halvo_control::proto::Platform::Macos,
        super::NoSources,
        SleepLoop(Instant::now()),
    );
    let stop = AtomicBool::new(true);
    let none = super::HeldStore::None;
    assert!(
        matches!(
            super::wait_serving(&mut supervisor, &serving, Duration::ZERO, &stop, &none),
            super::Waited::Ended
        ),
        "a wait with no window did not read the stop flag"
    );

    stop.store(false, Ordering::SeqCst);
    let asking = {
        let dir = data_dir(&dir);
        std::thread::spawn(move || {
            halvo_control::client::system_connector(&dir).and_then(|connector| {
                halvo_control::client::Client::new(connector)
                    .send(&halvo_control::proto::Request::Status { indicator: false })
            })
        })
    };
    let end = Instant::now() + Duration::from_secs(5);
    while !asking.is_finished() && Instant::now() < end {
        let _ = super::wait_serving(&mut supervisor, &serving, Duration::ZERO, &stop, &none);
        std::thread::sleep(Duration::from_millis(10));
    }
    let answer = asking.join().unwrap();
    assert!(
        matches!(answer, Ok(halvo_control::proto::Reply::Status(_))),
        "a request queued behind waits with no window was not served: {answer:?}"
    );
}

struct PanicsAtWrite {
    inner: Box<dyn super::port::CaptureStore + Send>,
    died: Arc<std::sync::Mutex<Option<Instant>>>,
}

impl super::port::MaintainStore for PanicsAtWrite {
    fn expire_due(
        &mut self,
        now: SystemTime,
        max_segments: usize,
    ) -> Result<super::port::Pass, super::port::MaintFault> {
        self.inner.expire_due(now, max_segments)
    }

    fn evict_to(
        &mut self,
        bound: super::port::ByteBound,
        max_segments: usize,
    ) -> Result<super::port::Pass, super::port::MaintFault> {
        self.inner.evict_to(bound, max_segments)
    }

    fn usage(&mut self) -> Result<super::port::Usage, super::port::MaintFault> {
        self.inner.usage()
    }
}

impl super::port::CaptureStore for PanicsAtWrite {
    fn write_batch(
        &mut self,
        _write: super::port::SegmentWrite<'_>,
    ) -> Result<super::port::Written, super::port::WriteFault> {
        *self.died.lock().unwrap() = Some(Instant::now());
        panic!("the store of this case panics at its first write");
    }

    fn volume_free_bytes(&self) -> std::io::Result<u64> {
        self.inner.volume_free_bytes()
    }
}

#[test]
fn a_service_run_whose_owner_thread_ended_ends_through_its_shutdown_entry_with_1_inside_the_budget()
{
    use super::state::Desired;
    let dir = kept_dir("service-tests-");
    std::fs::create_dir_all(&dir).unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    StateFile::open(
        StateDir::open(&dir).unwrap(),
        Box::new(TestWrite::default()),
    )
    .update(|state| state.desired = Desired::Running)
    .unwrap();
    let factory = super::sources::tests::Factory::adjusting(|source| {
        if source.name == super::SourceName::Foreground {
            source.on_begin = Some(Box::new(|ctx: &crate::source::SourceContext| {
                let _ = ctx.sink().emit_event(super::model_store::foreground_now());
            }));
        }
    });
    let died = Arc::new(std::sync::Mutex::new(None));
    let marked = died.clone();
    let parts = super::ServiceParts {
        lease: super::lease::Lease::never_lapsing(),
        wrap_store: Some(Box::new(move |inner| {
            Box::new(PanicsAtWrite {
                inner,
                died: marked,
            })
        })),
        ..super::ServiceParts::of(factory)
    };
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(60));
        flag.store(true, Ordering::SeqCst);
    });
    let quiet = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let mut err = Vec::new();
    let status = super::serve_on(
        &data_dir(&dir),
        Box::new(TestWrite::default()),
        SleepLoop(Instant::now()),
        &stop,
        Arc::new(Recorded),
        &mut err,
        parts,
    );
    let ended = Instant::now();
    std::panic::set_hook(quiet);
    let err = String::from_utf8(err).unwrap();
    let died = died
        .lock()
        .unwrap()
        .expect("the owner's store was never written");
    let took = ended.duration_since(died);
    assert!(
        took < Duration::from_millis(super::supervisor::SHUTDOWN_BUDGET_MS),
        "the run ended {took:?} after its owner's thread died: {err}"
    );
    assert_eq!(status, 1, "{err}");
    assert!(
        err.contains("halvo-capture: owner ended; events accepted and not persisted="),
        "{err}"
    );
}

/// The store of a life of a session's end: once `slow` reads true each write
/// takes `takes` before it reaches the store, and its drop is marked, so that
/// the next life opens the store after this life's owner let it go.
struct SlowAtWrite {
    inner: Box<dyn CaptureStore + Send>,
    slow: Arc<AtomicBool>,
    takes: Duration,
    dropped: Arc<AtomicBool>,
}

impl Drop for SlowAtWrite {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::SeqCst);
    }
}

impl MaintainStore for SlowAtWrite {
    fn expire_due(&mut self, now: SystemTime, max_segments: usize) -> Result<Pass, MaintFault> {
        self.inner.expire_due(now, max_segments)
    }

    fn evict_to(&mut self, bound: ByteBound, max_segments: usize) -> Result<Pass, MaintFault> {
        self.inner.evict_to(bound, max_segments)
    }

    fn usage(&mut self) -> Result<Usage, MaintFault> {
        self.inner.usage()
    }
}

impl CaptureStore for SlowAtWrite {
    fn write_batch(&mut self, write: SegmentWrite<'_>) -> Result<Written, WriteFault> {
        if self.slow.load(Ordering::SeqCst) {
            std::thread::sleep(self.takes);
        }
        self.inner.write_batch(write)
    }

    fn volume_free_bytes(&self) -> std::io::Result<u64> {
        self.inner.volume_free_bytes()
    }
}

/// The ending call of a life of a session's end, which the watchdog makes at
/// the end of the budget: recorded, where the product ends the process.
struct EndedWith(Arc<Mutex<Option<i32>>>);

impl main_loop::EndCall for EndedWith {
    fn end(&self, status: i32) {
        *self.0.lock().unwrap() = Some(status);
    }
}

/// Sets the stop flag of a life when the client of the life ends, also on a
/// panic of its asks, so that no life waits for a stop that never comes.
struct StopAtTheEnd(Arc<AtomicBool>);

impl Drop for StopAtTheEnd {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

struct SessionLife {
    status: i32,
    first_status: halvo_control::proto::DaemonStatus,
    err: String,
    ended: Arc<Mutex<Option<i32>>>,
    store_let_go: Arc<AtomicBool>,
}

/// One service run of `dir` on this thread, which a session's end ends: a
/// client reads the life's first Status, waits for an event pending in the
/// open store, makes each write of the store take `write_takes` and stops
/// the life, whose shutdown entry runs with the session's budget.
fn a_life_that_the_session_ends(dir: &Path, write_takes: Duration) -> SessionLife {
    use halvo_control::proto::StoreState;
    let factory = super::sources::tests::Factory::adjusting(|source| {
        if source.name == super::SourceName::Foreground {
            source.on_begin = Some(Box::new(|ctx: &crate::source::SourceContext| {
                let _ = ctx.sink().emit_event(super::model_store::foreground_now());
            }));
        }
    });
    let slow = Arc::new(AtomicBool::new(false));
    let store_let_go = Arc::new(AtomicBool::new(false));
    let (slowed, dropped) = (slow.clone(), store_let_go.clone());
    let parts = super::ServiceParts {
        lease: super::lease::Lease::never_lapsing(),
        wrap_store: Some(Box::new(move |inner| {
            Box::new(SlowAtWrite {
                inner,
                slow: slowed,
                takes: write_takes,
                dropped,
            })
        })),
        ..super::ServiceParts::of(factory)
    };
    let stop = Arc::new(AtomicBool::new(false));
    let client = {
        let (dir, stop) = (dir.to_path_buf(), StopAtTheEnd(stop.clone()));
        std::thread::spawn(move || {
            let first = status_until(&dir, "a first Status", |_| true);
            status_until(&dir, "an event pending in the open store", |status| {
                status.fields().store == StoreState::Open && status.fields().events_pending > 0
            });
            slow.store(true, Ordering::SeqCst);
            drop(stop);
            first
        })
    };
    let ended = Arc::new(Mutex::new(None));
    let mut err = Vec::new();
    signal::end_the_session_on_this_thread(true);
    let status = super::serve_on(
        &data_dir(dir),
        Box::new(TestWrite::default()),
        SleepLoop(Instant::now()),
        &stop,
        Arc::new(EndedWith(ended.clone())),
        &mut err,
        parts,
    );
    signal::end_the_session_on_this_thread(false);
    let err = String::from_utf8_lossy(&err).into_owned();
    let first_status = client
        .join()
        .unwrap_or_else(|_| panic!("the client of the life failed; the life's stderr:\n{err}"));
    SessionLife {
        status,
        first_status,
        err,
        ended,
        store_let_go,
    }
}

#[test]
fn a_session_end_that_does_not_fit_leaves_the_marker_and_the_next_start_reads_unclean() {
    use halvo_control::proto::LastStop;
    let (dir, _marks) = running_dir(false);
    let marker = data_dir(&dir).run_marker();
    let budget = Duration::from_millis(halvo_control::SESSION_END_BUDGET_MS);
    let let_go = |life: &SessionLife| {
        wait_until(READY_WITHIN, "the life's owner let its store go", || {
            life.store_let_go.load(Ordering::SeqCst)
        });
    };

    let first = a_life_that_the_session_ends(&dir, budget + Duration::from_secs(2));
    assert_eq!(first.first_status.fields().last_stop, LastStop::None);
    wait_until(budget, "the watchdog's ending call", || {
        first.ended.lock().unwrap().is_some()
    });
    assert_ne!(first.status, halvo_control::exit::CLEAN, "{}", first.err);
    assert!(
        marker.exists(),
        "the marker went with a settle that did not fit: {}",
        first.err
    );
    let_go(&first);

    let second = a_life_that_the_session_ends(&dir, Duration::ZERO);
    let printed = halvo_control::ctl_text::status_text(&second.first_status);
    assert!(
        printed.lines().any(|line| line == "last_stop=unclean"),
        "{printed}"
    );
    assert_eq!(second.status, halvo_control::exit::CLEAN, "{}", second.err);
    assert!(
        !marker.exists(),
        "the marker stays after a settle that fits: {}",
        second.err
    );
    let_go(&second);

    let third = a_life_that_the_session_ends(&dir, Duration::ZERO);
    assert_eq!(third.first_status.fields().last_stop, LastStop::Clean);
    let_go(&third);
}

#[test]
fn a_start_that_does_not_bind_leaves_the_marker_that_it_found_and_the_next_start_reads_unclean() {
    use halvo_control::proto::LastStop;
    let (dir, _marks) = running_dir(false);
    let named = data_dir(&dir);
    let (found, _) = super::run_marker::begin(&named);
    assert_eq!(
        found,
        LastStop::None,
        "a life that ended unclean left its marker"
    );

    let socket =
        halvo_control::paths::socket_path(&named, &halvo_control::paths::SystemAccount).unwrap();
    std::fs::create_dir_all(&socket).unwrap();
    let stop = AtomicBool::new(true);
    let mut err = Vec::new();
    let status = serve_held(
        InstanceLock::take(&named).unwrap().unwrap(),
        &named,
        Box::new(TestWrite::default()),
        SleepLoop(Instant::now()),
        &stop,
        Arc::new(Recorded),
        &mut err,
    );
    let err = String::from_utf8(err).unwrap();
    assert_eq!(status, 1, "{err}");
    assert!(
        err.contains("halvo-capture: the control channel could not be bound: "),
        "{err}"
    );
    assert!(
        named.run_marker().exists(),
        "the start that did not bind took the marker that it found away: {err}"
    );
    std::fs::remove_dir(&socket).unwrap();

    let next = a_life_that_the_session_ends(&dir, Duration::ZERO);
    assert_eq!(
        next.first_status.fields().last_stop,
        LastStop::Unclean,
        "{}",
        next.err
    );
    wait_until(READY_WITHIN, "the life's owner let its store go", || {
        next.store_let_go.load(Ordering::SeqCst)
    });
}

const LIFE_DIR: &str = "HALVO_LIFE_DIR";
const LIFE_MARKS: &str = "HALVO_LIFE_MARKS";
const LIFE: &str = "HALVO_LIFE";
const LIFE_CHILD: &str = "daemon::process_tests::life_child";
const FIRST_LIFE: &str = "first";
const SECOND_LIFE: &str = "second";
const ENDING_LIFE: &str = "ending";
const STOP_HELD_AT_MOST: Duration = Duration::from_millis(5_000);

fn begun(marks: &Path) -> usize {
    std::fs::read_to_string(marks.join("begun")).map_or(0, |text| text.lines().count())
}

#[test]
fn life_child() {
    let (Some(dir), Some(marks), Some(life)) = (
        std::env::var_os(LIFE_DIR),
        std::env::var_os(LIFE_MARKS),
        std::env::var_os(LIFE),
    ) else {
        return;
    };
    signal::prepare().unwrap();
    let marks = PathBuf::from(marks);
    let life = life.to_string_lossy().into_owned();
    let unstopped = life == FIRST_LIFE;
    let held_open = life == ENDING_LIFE;
    let factory = super::sources::tests::Factory::adjusting(move |source| {
        if held_open && source.name == super::SourceName::Screen {
            let (stopping, answered) = (marks.join("stopping"), marks.join("answered"));
            source.on_stop = Some(Box::new(move |_: &crate::source::SourceContext| {
                std::fs::write(&stopping, b"").unwrap();
                let started = Instant::now();
                while !answered.exists() && started.elapsed() < STOP_HELD_AT_MOST {
                    std::thread::sleep(Duration::from_millis(5));
                }
            }));
        }
        if source.name != super::SourceName::Foreground {
            return;
        }
        let begun = marks.join("begun");
        source.on_begin = Some(Box::new(move |_: &crate::source::SourceContext| {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&begun)
                .unwrap();
            file.write_all(b"begun\n").unwrap();
        }));
        if unstopped {
            source.stop = Err(crate::source::CaptureError::Timeout {
                stage: "foreground_stop",
                ms: 1_000,
            });
        }
    });
    let parts = super::ServiceParts {
        lease: if life == SECOND_LIFE {
            super::lease::Lease::NONE
        } else {
            super::lease::Lease::never_lapsing()
        },
        ..super::ServiceParts::of(factory)
    };
    let status = super::serve_on(
        &DataDir::from_flag(PathBuf::from(dir)).unwrap(),
        Box::new(TestWrite::default()),
        SleepLoop(Instant::now()),
        signal::flag(),
        Arc::new(main_loop::ExitNow),
        &mut std::io::stderr(),
        parts,
    );
    std::process::exit(status);
}

fn life(dir: &Path, marks: &Path, life: &str) -> Service {
    Service(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", LIFE_CHILD, "--test-threads=1"])
            .env(LIFE_DIR, dir)
            .env(LIFE_MARKS, marks)
            .env(LIFE, life)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    )
}

fn running_dir(screen: bool) -> (PathBuf, PathBuf) {
    use super::state::Desired;
    let root = kept_dir("life-tests-");
    let dir = root.join("data");
    let marks = root.join("marks");
    std::fs::create_dir(&marks).unwrap();
    std::fs::create_dir(&dir).unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    StateFile::open(
        StateDir::open(&dir).unwrap(),
        Box::new(TestWrite::default()),
    )
    .update(|state| {
        state.desired = Desired::Running;
        state
            .switches
            .set(crate::floor::Switch::ScreenActivity, screen);
    })
    .unwrap();
    (dir, marks)
}

fn ask(
    dir: &Path,
    request: &halvo_control::proto::Request,
) -> Result<halvo_control::proto::Reply, halvo_control::client::ClientError> {
    halvo_control::client::system_connector(&data_dir(dir))
        .and_then(|connector| halvo_control::client::Client::new(connector).send(request))
}

fn wait_until(within: Duration, what: &str, done: impl Fn() -> bool) {
    let started = Instant::now();
    while !done() {
        assert!(started.elapsed() < within, "{what} within {within:?}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn ended_within(service: &mut Service, within: Duration) -> Option<i32> {
    let started = Instant::now();
    loop {
        if let Some(status) = service.0.try_wait().unwrap() {
            return status.code();
        }
        assert!(
            started.elapsed() < within,
            "the child did not end within {within:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn files_under(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut walk = vec![root.to_path_buf()];
    while let Some(at) = walk.pop() {
        let meta = std::fs::symlink_metadata(&at).unwrap();
        if meta.is_dir() {
            for entry in std::fs::read_dir(&at).unwrap() {
                walk.push(entry.unwrap().path());
            }
        } else if meta.is_file() {
            found.push(at);
        }
    }
    found
}

fn holds(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|at| at == needle)
}

#[test]
fn a_delete_goes_on_past_a_source_that_does_not_stop_and_the_life_ends_with_5() {
    use halvo_control::proto::{Capture, Reply, Request, StoreState};
    let (dir, marks) = running_dir(false);
    let mut first = life(&dir, &marks, FIRST_LIFE);
    wait_until(READY_WITHIN, "the first life's source began", || {
        begun(&marks) == 1
    });
    let shown = match ask(&dir, &Request::Status { indicator: false }) {
        Ok(Reply::Status(status)) => status.fields().store_id,
        other => panic!("the status of the first life: {other:?}"),
    };
    let kept: Vec<Vec<u8>> = std::iter::once(dir.join("store").join("store.key"))
        .chain(files_under(&dir.join("roots")))
        .map(|path| std::fs::read(path).unwrap())
        .collect();
    assert_eq!(kept.len(), 2, "store.key and one root file");

    let reply = ask(&dir, &Request::DeleteAll { store: shown });
    let status = ended_within(&mut first, Duration::from_secs(60));
    assert!(matches!(reply, Ok(Reply::Deleted(_))), "{reply:?}");
    assert_eq!(status, Some(halvo_control::exit::SOURCE_NOT_STOPPED));
    for file in files_under(&dir) {
        let bytes = std::fs::read(&file).unwrap();
        assert!(
            !kept.iter().any(|key| holds(&bytes, key)),
            "{} holds the bytes of a key of the deleted store",
            file.display()
        );
    }
    assert_eq!(
        std::fs::read_to_string(data_dir(&dir).run_marker()).unwrap(),
        "last_stop=source_not_stopped kind=delete_all final=yes not_stopped=foreground:timeout\n",
        "the run marker's last record"
    );

    let second = life(&dir, &marks, SECOND_LIFE);
    wait_until(READY_WITHIN, "the second life's store opened", || {
        matches!(
            ask(&dir, &Request::Status { indicator: false }),
            Ok(Reply::Status(status)) if status.fields().store == StoreState::Open
        )
    });
    let claimed = ask(&dir, &Request::Status { indicator: true });
    assert!(
        matches!(&claimed, Ok(Reply::Status(status)) if status.fields().capture == Capture::AwaitingResume),
        "{claimed:?}"
    );
    std::thread::sleep(Duration::from_millis(1_500));
    assert_eq!(begun(&marks), 1, "a source began in the held life");
    let resumed = ask(&dir, &Request::Resume);
    assert!(
        matches!(resumed, Ok(Reply::CaptureChanged(_))),
        "{resumed:?}"
    );
    let _ = ask(&dir, &Request::Status { indicator: true });
    wait_until(
        Duration::from_secs(10),
        "a source began after the Resume",
        || begun(&marks) == 2,
    );
    assert_eq!(terminate(second), Some(0));
}

#[test]
fn a_delete_that_ends_not_whole_reads_deleting_with_the_field_and_a_second_delete_runs_again() {
    use halvo_control::proto::{Reply, Request, StatusFields, StoreState};
    struct Immutable(std::ffi::CString);
    impl Drop for Immutable {
        fn drop(&mut self) {
            unsafe { libc::chflags(self.0.as_ptr(), 0) };
        }
    }
    let (dir, marks) = running_dir(false);
    let mut stopping = life(&dir, &marks, "stopping");
    wait_until(READY_WITHIN, "the life's source began", || {
        begun(&marks) == 1
    });
    let status = |when: &str| -> StatusFields {
        match ask(&dir, &Request::Status { indicator: false }) {
            Ok(Reply::Status(status)) => status.into_fields(),
            other => panic!("the status {when}: {other:?}"),
        }
    };
    let shown = status("before the delete").store_id;
    assert!(shown.is_some(), "a store stands before the delete");
    let derived = dir.join("derived");
    std::fs::create_dir(&derived).unwrap();
    let kept = derived.join("kept");
    std::fs::write(&kept, b"").unwrap();
    let flagged = Immutable(std::ffi::CString::new(kept.to_str().unwrap()).unwrap());
    assert_eq!(
        unsafe { libc::chflags(flagged.0.as_ptr(), libc::UF_IMMUTABLE) },
        0
    );

    let first = ask(&dir, &Request::DeleteAll { store: shown });
    let Ok(Reply::Deleted(first)) = first else {
        panic!("the first delete: {first:?}");
    };
    assert_ne!(first.report.fields().entries_left, 0, "{first:?}");
    assert_eq!(first.store, None, "{first:?}");
    let after = status("after a delete that ended not whole");
    assert_eq!(
        (after.store, after.store_delete_unfinished, after.store_id),
        (StoreState::Deleting, true, None)
    );
    assert_eq!(stopping.0.try_wait().unwrap(), None, "the life goes on");

    drop(flagged);
    let second = ask(&dir, &Request::DeleteAll { store: None });
    let Ok(Reply::Deleted(second)) = second else {
        panic!("the second delete: {second:?}");
    };
    assert_eq!(second.report.fields().entries_left, 0, "{second:?}");
    let made = second.store.expect("the second delete made a store");
    assert_ne!(Some(made), shown);
    assert!(!derived.exists());
    wait_until(READY_WITHIN, "the new store opened", || {
        let now = status("after the second delete");
        now.store == StoreState::Open && now.store_id == Some(made) && !now.store_delete_unfinished
    });
    assert_eq!(terminate(stopping), Some(0));
}

#[test]
fn a_pause_in_an_ending_life_is_answered_and_the_next_start_reads_paused() {
    use halvo_control::proto::{Reply, Request};
    let (dir, marks) = running_dir(true);
    let mut ending = life(&dir, &marks, ENDING_LIFE);
    wait_until(READY_WITHIN, "the source began", || begun(&marks) == 1);
    let pid = libc::pid_t::try_from(ending.0.id()).unwrap();
    assert_eq!(unsafe { libc::kill(pid, libc::SIGTERM) }, 0);
    wait_until(
        Duration::from_secs(30),
        "the shutdown entry asked the source to stop",
        || marks.join("stopping").exists(),
    );
    let paused = ask(&dir, &Request::Pause);
    std::fs::write(marks.join("answered"), b"").unwrap();
    let status = ended_within(&mut ending, Duration::from_secs(60));
    assert!(
        matches!(&paused, Ok(Reply::CaptureChanged(status)) if status.fields().desired == halvo_control::model::CaptureState::Paused),
        "{paused:?}"
    );
    assert_eq!(status, Some(halvo_control::exit::CLEAN));
    let state = StateFile::open(
        StateDir::open(&dir).unwrap(),
        Box::new(TestWrite::default()),
    );
    assert_eq!(
        state.state().desired,
        super::state::Desired::Paused,
        "the state file as the next start reads it"
    );
}

#[derive(Clone, Default)]
struct StandInProbe {
    answers: Arc<Mutex<std::collections::VecDeque<Probed>>>,
    asked: Arc<Mutex<Vec<Vec<Permission>>>>,
}

impl ProbeRunner for StandInProbe {
    fn start(&mut self, permissions: &[Permission]) {
        self.asked.lock().unwrap().push(permissions.to_vec());
    }

    fn poll(&mut self) -> Probed {
        self.answers
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Probed::Running)
    }
}

impl StandInProbe {
    fn answer(&self, permission: Permission, grant: GrantState) {
        self.answers
            .lock()
            .unwrap()
            .push_back(Probed::Answered(vec![(permission, grant)]));
    }

    fn asked(&self) -> Vec<Vec<Permission>> {
        self.asked.lock().unwrap().clone()
    }
}

struct JumpLoop {
    started: Instant,
    jump: Arc<Mutex<Duration>>,
}

impl MainLoop for JumpLoop {
    fn turn(&mut self, wait: Duration) {
        std::thread::sleep(wait);
    }

    fn now(&self) -> Duration {
        self.started.elapsed() + *self.jump.lock().unwrap()
    }
}

struct StopOnPanic(Arc<AtomicBool>);

impl Drop for StopOnPanic {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.0.store(true, Ordering::SeqCst);
        }
    }
}

type Start = (String, Vec<String>, Duration);

#[derive(Clone, Default)]
struct StandInStarter {
    starts: Arc<Mutex<Vec<Start>>>,
    runs: Arc<AtomicBool>,
}

impl RequestStarter for StandInStarter {
    fn start(&mut self, program: &str, args: &[&str], deadline: Duration) -> io::Result<()> {
        self.starts.lock().unwrap().push((
            program.to_owned(),
            args.iter().map(|arg| (*arg).to_owned()).collect(),
            deadline,
        ));
        self.runs.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn running(&mut self) -> bool {
        self.runs.load(Ordering::SeqCst)
    }
}

impl StandInStarter {
    fn starts(&self) -> Vec<Start> {
        self.starts.lock().unwrap().clone()
    }
}

struct Probing {
    probe: StandInProbe,
    starter: StandInStarter,
    jump: Arc<Mutex<Duration>>,
    stop: Arc<AtomicBool>,
}

impl Probing {
    fn new() -> Probing {
        let stop = Arc::new(AtomicBool::new(false));
        let late = stop.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(180));
            late.store(true, Ordering::SeqCst);
        });
        Probing {
            probe: StandInProbe::default(),
            starter: StandInStarter::default(),
            jump: Arc::default(),
            stop,
        }
    }

    fn serve(&self, dir: &Path, factory: super::sources::tests::Factory) -> (i32, String) {
        let parts = super::ServiceParts {
            lease: super::lease::Lease::never_lapsing(),
            probe: Some(Box::new(self.probe.clone())),
            requests: Some(Box::new(self.starter.clone())),
            typing: true,
            ..super::ServiceParts::of(factory)
        };
        let mut err = Vec::new();
        let status = super::serve_on(
            &data_dir(dir),
            Box::new(TestWrite::default()),
            JumpLoop {
                started: Instant::now(),
                jump: self.jump.clone(),
            },
            &self.stop,
            Arc::new(Recorded),
            &mut err,
            parts,
        );
        (status, String::from_utf8(err).unwrap())
    }
}

fn status_until(
    dir: &Path,
    what: &str,
    done: impl Fn(&halvo_control::proto::DaemonStatus) -> bool,
) -> halvo_control::proto::DaemonStatus {
    asked_status_until(dir, false, what, done)
}

fn asked_status_until(
    dir: &Path,
    indicator: bool,
    what: &str,
    done: impl Fn(&halvo_control::proto::DaemonStatus) -> bool,
) -> halvo_control::proto::DaemonStatus {
    use halvo_control::proto::{Reply, Request};
    let started = Instant::now();
    loop {
        if let Ok(Reply::Status(status)) = ask(dir, &Request::Status { indicator }) {
            if done(&status) {
                return status;
            }
        }
        assert!(
            started.elapsed() < READY_WITHIN,
            "{what} within {READY_WITHIN:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn grant_row(
    status: &halvo_control::proto::DaemonStatus,
    permission: Permission,
) -> halvo_control::proto::PermissionLine {
    *status
        .fields()
        .permissions
        .iter()
        .find(|line| line.name == permission)
        .unwrap()
}

fn source_row(
    status: &halvo_control::proto::DaemonStatus,
    name: super::SourceName,
) -> halvo_control::proto::SourceState {
    status
        .fields()
        .sources
        .iter()
        .find(|line| line.name == name)
        .unwrap()
        .state
        .clone()
}

fn once_granted_in(dir: &Path) -> Vec<Permission> {
    StateFile::open(StateDir::open(dir).unwrap(), Box::new(TestWrite::default()))
        .state()
        .permissions_once_granted
        .clone()
}

#[test]
fn a_service_run_reads_the_grants_from_its_probe_and_ends_with_4_at_a_new_grant() {
    use halvo_control::proto::{Reason, SourceState as Line};
    let (dir, _) = running_dir(false);
    let first = Probing::new();
    let factory = super::sources::tests::Factory::new();
    let calls = factory.calls.clone();
    let reading = {
        let (dir, probe, jump, stop) = (
            dir.clone(),
            first.probe.clone(),
            first.jump.clone(),
            first.stop.clone(),
        );
        std::thread::spawn(move || {
            let _stop = StopOnPanic(stop);
            wait_until(READY_WITHIN, "the foreground source began", || {
                !calls.of("begin foreground").is_empty()
            });
            wait_until(READY_WITHIN, "the probe was started", || {
                !probe.asked().is_empty()
            });
            let begun_before_the_answer = calls.of("begin");
            probe.answer(Permission::InputMonitoring, GrantState::NotGranted);
            let read = status_until(&dir, "the status reads the probe's answer", |status| {
                grant_row(status, Permission::InputMonitoring).grant == GrantState::NotGranted
                    && matches!(
                        source_row(status, super::SourceName::Input),
                        Line::Failed {
                            reason: Reason::Permission,
                            ..
                        }
                    )
            });
            probe.answer(Permission::InputMonitoring, GrantState::Granted);
            *jump.lock().unwrap() += super::supervisor::PROBE_INTERVAL + Duration::from_secs(1);
            (begun_before_the_answer, read)
        })
    };
    let (status, err) = first.serve(&dir, factory);
    let (begun_before_the_answer, read) = reading.join().unwrap();
    assert!(
        begun_before_the_answer.contains(&"begin foreground".to_owned())
            && !begun_before_the_answer.contains(&"begin input".to_owned()),
        "the sources begun before the probe's first answer: {begun_before_the_answer:?}"
    );
    assert_eq!(
        grant_row(&read, Permission::InputMonitoring).at_start,
        GrantState::NotGranted
    );
    let asked = first.probe.asked();
    assert!(
        asked.len() >= 2
            && asked
                .iter()
                .all(|named| named == &[Permission::InputMonitoring]),
        "what the probe was asked: {asked:?}"
    );
    assert_eq!(
        status,
        halvo_control::exit::RESTART,
        "the life at a new grant: {err}"
    );
    assert!(
        once_granted_in(&dir).is_empty(),
        "the life that ended at the new grant left the state file as it is"
    );

    let second = Probing::new();
    second
        .probe
        .answer(Permission::InputMonitoring, GrantState::Granted);
    let factory = super::sources::tests::Factory::new();
    let calls = factory.calls.clone();
    let watching = {
        let (dir, stop) = (dir.clone(), second.stop.clone());
        std::thread::spawn(move || {
            let _stop = StopOnPanic(stop.clone());
            wait_until(READY_WITHIN, "the input source began", || {
                calls.of("begin input").len() == 1
            });
            wait_until(READY_WITHIN, "the state file holds the grant", || {
                once_granted_in(&dir) == [Permission::InputMonitoring]
            });
            stop.store(true, Ordering::SeqCst);
        })
    };
    let (status, err) = second.serve(&dir, factory);
    watching.join().unwrap();
    assert_eq!(status, halvo_control::exit::CLEAN, "the second life: {err}");
    assert_eq!(once_granted_in(&dir), [Permission::InputMonitoring]);

    assert_eq!(
        (
            super::system_parts(super::NoSources)
                .probe
                .as_deref()
                .and_then(|probe| probe.program().map(str::to_owned)),
            super::seam::scripted_parts().probe.is_none(),
        ),
        (Some("halvo-capture".to_owned()), true),
        "the probe of the service's own parts, and none in the seam's"
    );
}

fn lost_row(status: &halvo_control::proto::DaemonStatus, permission: Permission) -> bool {
    status
        .fields()
        .onboarding
        .permissions
        .iter()
        .find(|row| row.permission == permission)
        .unwrap()
        .lost
}

fn state_bytes(dir: &Path) -> Vec<u8> {
    std::fs::read(dir.join(halvo_control::paths::STATE_FILE)).unwrap()
}

fn corrupt_dir() -> PathBuf {
    let dir = kept_dir("permission-tests-").join("data");
    std::fs::create_dir(&dir).unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    std::fs::write(dir.join(halvo_control::paths::STATE_FILE), b"{\"v\":").unwrap();
    dir
}

#[test]
fn an_acknowledged_lost_grant_leaves_the_record_and_its_row_reads_lost_no_more() {
    use halvo_control::proto::{ErrorCode, Reply, Request};
    let (dir, _) = running_dir(false);
    StateFile::open(
        StateDir::open(&dir).unwrap(),
        Box::new(TestWrite::default()),
    )
    .update(|state| state.permissions_once_granted = vec![Permission::InputMonitoring])
    .unwrap();
    let life = Probing::new();
    let asking = {
        let (dir, probe, jump, stop) = (
            dir.clone(),
            life.probe.clone(),
            life.jump.clone(),
            life.stop.clone(),
        );
        std::thread::spawn(move || {
            let _stop = StopOnPanic(stop.clone());
            let not_lost = |status: &halvo_control::proto::DaemonStatus| {
                !lost_row(status, Permission::InputMonitoring)
                    && status.fields().onboarding.lost_grants.is_empty()
                    && grant_row(status, Permission::InputMonitoring).once_granted
            };
            status_until(&dir, "the store is open", |status| {
                status.fields().store == halvo_control::proto::StoreState::Open
            });
            wait_until(READY_WITHIN, "the probe was started", || {
                !probe.asked().is_empty()
            });
            let unanswered = status_until(&dir, "a status before the first answer", |_| true);
            assert!(
                not_lost(&unanswered),
                "before the first answer: {unanswered:?}"
            );
            probe.answers.lock().unwrap().push_back(Probed::Failed);
            wait_until(READY_WITHIN, "the failed probe was read", || {
                probe.answers.lock().unwrap().is_empty()
            });
            let failed = status_until(&dir, "a status after a failed probe", |_| true);
            assert!(not_lost(&failed), "after a failed probe: {failed:?}");
            probe.answer(Permission::InputMonitoring, GrantState::NotGranted);
            *jump.lock().unwrap() +=
                super::supervisor::PROBE_INTERVAL_MISSING + Duration::from_secs(1);
            let lost = status_until(&dir, "the row reads lost", |status| {
                grant_row(status, Permission::InputMonitoring).grant == GrantState::NotGranted
            });
            let acknowledged = ask(
                &dir,
                &Request::AcknowledgeLostPermission {
                    permission: Permission::InputMonitoring,
                },
            );
            let record = once_granted_in(&dir);
            let before = state_bytes(&dir);
            let not_held = ask(
                &dir,
                &Request::AcknowledgeLostPermission {
                    permission: Permission::ScreenAndSystemAudioRecording,
                },
            );
            let after = state_bytes(&dir);
            stop.store(true, Ordering::SeqCst);
            (lost, acknowledged, record, not_held, before == after)
        })
    };
    let (status, err) = life.serve(&dir, super::sources::tests::Factory::new());
    let (lost, acknowledged, record, not_held, unchanged) = asking.join().unwrap();
    assert!(lost_row(&lost, Permission::InputMonitoring), "{lost:?}");
    match &acknowledged {
        Ok(Reply::Status(status)) => assert!(
            !lost_row(status, Permission::InputMonitoring),
            "the reply's row: {status:?}"
        ),
        other => panic!("the acknowledge: {other:?}"),
    }
    assert!(record.is_empty(), "the record after it: {record:?}");
    assert!(matches!(not_held, Ok(Reply::Status(_))), "{not_held:?}");
    assert!(
        unchanged,
        "an acknowledge of a permission that the record does not hold wrote the file"
    );
    assert_eq!(status, halvo_control::exit::CLEAN, "{err}");

    let corrupt = corrupt_dir();
    let life = Probing::new();
    let asking = {
        let (dir, stop) = (corrupt.clone(), life.stop.clone());
        std::thread::spawn(move || {
            let _stop = StopOnPanic(stop.clone());
            status_until(&dir, "the status of a life on a corrupt file", |_| true);
            let acknowledged = ask(
                &dir,
                &Request::AcknowledgeLostPermission {
                    permission: Permission::InputMonitoring,
                },
            );
            stop.store(true, Ordering::SeqCst);
            acknowledged
        })
    };
    let (status, err) = life.serve(&corrupt, super::sources::tests::Factory::new());
    let acknowledged = asking.join().unwrap();
    assert_eq!(
        acknowledged.as_ref().ok().and_then(Reply::code),
        Some(ErrorCode::StateFile),
        "{acknowledged:?}"
    );
    assert_eq!(
        state_bytes(&corrupt),
        b"{\"v\":",
        "the file that does not load"
    );
    assert_eq!(status, halvo_control::exit::CLEAN, "{err}");
}

fn asked_in(dir: &Path) -> Vec<Permission> {
    StateFile::open(StateDir::open(dir).unwrap(), Box::new(TestWrite::default()))
        .state()
        .permissions_asked
        .clone()
}

#[test]
fn an_ask_records_the_permission_starts_one_request_child_and_answers_requested() {
    use halvo_control::onboarding::RequestOutcome;
    use halvo_control::proto::{ErrorCode, PermissionRequested, Reply, Request};
    fn outcome_of(
        reply: &Result<Reply, halvo_control::client::ClientError>,
    ) -> Option<RequestOutcome> {
        match reply {
            Ok(Reply::PermissionRequested(PermissionRequested { outcome })) => Some(*outcome),
            _ => None,
        }
    }
    let request = |permission| Request::RequestPermission { permission };
    let (dir, _) = running_dir(false);
    let life = Probing::new();
    let asking = {
        let (dir, probe, starter, stop) = (
            dir.clone(),
            life.probe.clone(),
            life.starter.clone(),
            life.stop.clone(),
        );
        std::thread::spawn(move || {
            let _stop = StopOnPanic(stop.clone());
            status_until(&dir, "the store is open", |status| {
                status.fields().store == halvo_control::proto::StoreState::Open
            });
            wait_until(READY_WITHIN, "the probe was started", || {
                !probe.asked().is_empty()
            });
            let first = ask(&dir, &request(Permission::Accessibility));
            assert_eq!(
                outcome_of(&first),
                Some(RequestOutcome::Requested),
                "{first:?}"
            );
            let one_start = (
                super::permissions::PROGRAM.to_owned(),
                vec![
                    "--request-permission".to_owned(),
                    "accessibility".to_owned(),
                ],
                Duration::from_secs(60),
            );
            assert_eq!(starter.starts(), std::slice::from_ref(&one_start));
            assert_eq!(asked_in(&dir), [Permission::Accessibility]);
            let next = status_until(&dir, "a status after the ask", |_| true);
            assert!(
                grant_row(&next, Permission::Accessibility).asked,
                "{next:?}"
            );

            let again = ask(&dir, &request(Permission::Accessibility));
            assert_eq!(
                outcome_of(&again),
                Some(RequestOutcome::Requested),
                "{again:?}"
            );
            let other = ask(&dir, &request(Permission::ScreenAndSystemAudioRecording));
            assert_eq!(
                other.as_ref().ok().and_then(Reply::code),
                Some(ErrorCode::Busy),
                "{other:?}"
            );
            assert_eq!(starter.starts(), [one_start], "a start while one runs");
            assert_eq!(asked_in(&dir), [Permission::Accessibility]);

            probe.answer(Permission::InputMonitoring, GrantState::Granted);
            status_until(&dir, "the row reads granted", |status| {
                grant_row(status, Permission::InputMonitoring).grant == GrantState::Granted
            });
            let before = state_bytes(&dir);
            let granted = ask(&dir, &request(Permission::InputMonitoring));
            assert_eq!(
                outcome_of(&granted),
                Some(RequestOutcome::AlreadyGranted),
                "{granted:?}"
            );
            assert_eq!(
                state_bytes(&dir),
                before,
                "an ask of a granted permission wrote"
            );
            assert_eq!(
                starter.starts().len(),
                1,
                "a start for a granted permission"
            );

            let other_form = ask(
                &dir,
                &Request::PermissionAsked {
                    permission: Permission::InputMonitoring,
                },
            );
            assert_eq!(
                other_form.as_ref().ok().and_then(Reply::code),
                Some(ErrorCode::UnsupportedOp),
                "{other_form:?}"
            );
            stop.store(true, Ordering::SeqCst);
        })
    };
    let (status, err) = life.serve(&dir, super::sources::tests::Factory::new());
    asking.join().unwrap();
    assert_eq!(status, halvo_control::exit::CLEAN, "{err}");

    let corrupt = corrupt_dir();
    let life = Probing::new();
    let asking = {
        let (dir, stop) = (corrupt.clone(), life.stop.clone());
        std::thread::spawn(move || {
            let _stop = StopOnPanic(stop.clone());
            status_until(&dir, "the status of a life on a corrupt file", |_| true);
            let refused = ask(&dir, &request(Permission::Accessibility));
            stop.store(true, Ordering::SeqCst);
            refused
        })
    };
    let (status, err) = life.serve(&corrupt, super::sources::tests::Factory::new());
    let refused = asking.join().unwrap();
    assert_eq!(
        refused.as_ref().ok().and_then(Reply::code),
        Some(ErrorCode::StateFile),
        "{refused:?}"
    );
    assert!(
        life.starter.starts().is_empty(),
        "a start on a corrupt file"
    );
    assert_eq!(state_bytes(&corrupt), b"{\"v\":");
    assert_eq!(status, halvo_control::exit::CLEAN, "{err}");
}

#[test]
fn a_paused_service_reads_the_grants_of_its_switches_and_ends_with_4_at_a_new_grant() {
    use halvo_control::proto::{Capture, Request};
    let screen = Permission::ScreenAndSystemAudioRecording;
    let input = Permission::InputMonitoring;
    let both =
        move |on_screen, on_input| Probed::Answered(vec![(screen, on_screen), (input, on_input)]);
    let (dir, _) = running_dir(true);
    StateFile::open(
        StateDir::open(&dir).unwrap(),
        Box::new(TestWrite::default()),
    )
    .update(|state| state.desired = super::state::Desired::Paused)
    .unwrap();
    let life = Probing::new();
    let queue = life.probe.answers.clone();
    queue
        .lock()
        .unwrap()
        .push_back(both(GrantState::NotGranted, GrantState::NotGranted));
    let asking = {
        let (dir, probe, jump, stop) = (
            dir.clone(),
            life.probe.clone(),
            life.jump.clone(),
            life.stop.clone(),
        );
        std::thread::spawn(move || {
            let _stop = StopOnPanic(stop);
            let jump_by = |by: Duration| *jump.lock().unwrap() += by;
            let page = || ask(&dir, &Request::Status { indicator: false });
            let a_few_ticks = || std::thread::sleep(Duration::from_millis(400));
            asked_status_until(&dir, true, "the rows read the first answer", |status| {
                status.fields().capture == Capture::Paused
                    && grant_row(status, screen).grant == GrantState::NotGranted
                    && grant_row(status, input).grant == GrantState::NotGranted
            });
            let first = probe.asked();
            assert!(
                first.len() == 1
                    && first[0].len() == 2
                    && first[0].contains(&screen)
                    && first[0].contains(&input),
                "what a paused service asks first: {first:?}"
            );

            jump_by(Duration::from_secs(4));
            a_few_ticks();
            let _ = asked_status_until(&dir, true, "a claim", |_| true);
            a_few_ticks();
            assert_eq!(
                probe.asked().len(),
                1,
                "a probe within 60 s with claims alone"
            );

            queue
                .lock()
                .unwrap()
                .push_back(both(GrantState::NotGranted, GrantState::NotGranted));
            let _ = page();
            jump_by(Duration::from_secs(4));
            wait_until(READY_WITHIN, "a probe 3 s after the page's Status", || {
                probe.asked().len() == 2
            });
            wait_until(READY_WITHIN, "the second answer was read", || {
                queue.lock().unwrap().is_empty()
            });

            jump_by(super::supervisor::CLIENT_POLL_WINDOW + Duration::from_secs(1));
            a_few_ticks();
            let _ = asked_status_until(&dir, true, "a claim", |_| true);
            a_few_ticks();
            assert_eq!(
                probe.asked().len(),
                2,
                "a probe within 60 s once the window had passed"
            );

            queue
                .lock()
                .unwrap()
                .push_back(both(GrantState::Granted, GrantState::NotGranted));
            let _ = page();
            jump_by(Duration::from_secs(4));
            probe.asked()
        })
    };
    let (status, err) = life.serve(&dir, super::sources::tests::Factory::new());
    let asked = asking.join().unwrap();
    assert!(
        asked
            .iter()
            .all(|named| !named.contains(&Permission::Accessibility)),
        "{asked:?}"
    );
    assert_eq!(status, halvo_control::exit::RESTART, "{err}");
    let state = StateFile::open(
        StateDir::open(&dir).unwrap(),
        Box::new(TestWrite::default()),
    );
    assert_eq!(
        (
            state.state().desired,
            state.state().permissions_once_granted.clone()
        ),
        (super::state::Desired::Paused, Vec::new()),
        "the life that ended at the new grant left the state file as it is"
    );
}
