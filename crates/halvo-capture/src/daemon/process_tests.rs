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
