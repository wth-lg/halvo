#![cfg(all(target_os = "macos", debug_assertions))]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use halvo_control::instance_lock::{self, LockRead};
use halvo_control::paths::DataDir;

use super::instance::tests::{backup_mark, kept_dir};
use super::instance::InstanceLock;
use super::main_loop::{self, MainLoop};
use super::state::tests::TestWrite;
use super::state::{DaemonState, StateDir, StateFile, StateWrite};
use super::{serve, serve_held, signal};

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

const MADE: [&str; 3] = ["daemon-state.json", "roots", "store"];

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

const MADE_AGAIN_LINE: &str = "halvo-capture: a first create of the store that never published its key file stood unfinished; the store was made again";

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
    assert_eq!(names(&missing), vec!["daemon-state.json".to_owned()]);
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
fn the_held_service_run_opens_its_state_folder_through_the_lock_when_the_folder_above_cannot_be_searched(
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
    assert_eq!(status, 0, "{err}");
    assert!(!err.contains("folder of the data directory"), "{err}");
    assert_eq!(names(&data), vec!["daemon-state.json".to_owned()]);
}
