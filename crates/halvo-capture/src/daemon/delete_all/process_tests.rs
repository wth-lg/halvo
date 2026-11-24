#![cfg(all(target_os = "macos", debug_assertions))]

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use halvo_control::model::UtcDay;
use halvo_control::paths::DataDir;

use super::{at_start, rows, run, start_time_of, AtStart, Hands, Offline, Owner, Row, SystemHands};
use crate::custody::dev_fallback::DevFallbackCustody;
use crate::custody::{KeyCustody, KeyLabel, RootId, WrappedKey};
use crate::daemon::custody_policy::{self, Custody, OpenAs, Selection};
use crate::daemon::instance::tests::kept_dir;
use crate::daemon::instance::InstanceLock;
use crate::daemon::owner::open::{CreateRule, Held, Opener};
use crate::daemon::port::{
    Access, DataDirStore, EntryKind, KindEntry, Opened, Probe, SegmentId, SegmentWrite, StoreHost,
};
use crate::daemon::slot::Slot;
use crate::daemon::{parse, Mode, TailAnswer};
use crate::event::Modality;

const HOLD: &str = "HALVO_DELETE_ALL_HOLD";
const HELD_TEST: &str =
    "daemon::delete_all::process_tests::a_held_store_answers_busy_and_removes_nothing";
const STORE_HOLD: &str = "HALVO_DELETE_ALL_STORE_HOLD";
const START_TEST: &str = "daemon::delete_all::process_tests::a_start_whose_finish_cannot_delete_keeps_the_intent_and_opens_nothing";

fn data_dir(path: &Path) -> DataDir {
    DataDir::from_flag(path.to_str().unwrap()).unwrap()
}

fn a_store_with_a_segment(dir: &Path) -> String {
    let Opened::ReadWrite { mut store, id } = DataDirStore::at(dir).create(Custody::RootFile)
    else {
        panic!("no store was made");
    };
    let entries = [KindEntry {
        kind: EntryKind::of(Modality::Keystroke),
        events: 1,
        bytes: 3,
    }];
    let segment = SegmentId::mint().unwrap();
    let day = UtcDay::from_days_since_epoch(20_000).unwrap();
    let at = 20_000 * 86_400 * 1_000_000_000_i64;
    store
        .write_batch(SegmentWrite::new(&segment, day, at, at, &entries, b"{}\n"))
        .unwrap();
    id.to_string()
}

fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut open = vec![dir.to_path_buf()];
    while let Some(folder) = open.pop() {
        for entry in std::fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            let kind = std::fs::symlink_metadata(&path).unwrap().file_type();
            if kind.is_dir() {
                open.push(path);
            } else {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    files_under(dir)
        .into_iter()
        .map(|file| {
            let bytes = std::fs::read(&file).unwrap();
            (file, bytes)
        })
        .collect()
}

fn segment_files(dir: &Path) -> Vec<PathBuf> {
    files_under(dir)
        .into_iter()
        .filter(|file| file.extension().is_some_and(|ext| ext == "seg"))
        .collect()
}

fn root_files(dir: &Path) -> Vec<PathBuf> {
    let roots = dir.join("roots");
    if !roots.exists() {
        return Vec::new();
    }
    files_under(&roots)
}

fn files_that_unwrap(dir: &Path, key_file: &[u8]) -> Vec<PathBuf> {
    files_that_unwrap_under(dir, key_file, KeyLabel::STORE)
}

fn files_that_unwrap_under(dir: &Path, key_file: &[u8], label: KeyLabel) -> Vec<PathBuf> {
    let parsed: serde_json::Value = serde_json::from_slice(key_file).unwrap();
    let root_id = RootId::parse(parsed["root_id"].as_str().unwrap()).unwrap();
    let wrapped = WrappedKey(
        BASE64
            .decode(parsed["wrapped_key"].as_str().unwrap())
            .unwrap(),
    );
    let probe = kept_dir("delete-all-probe-");
    std::fs::set_permissions(&probe, std::fs::Permissions::from_mode(0o700)).unwrap();
    let at = probe.join(format!("{root_id}.root"));
    files_under(dir)
        .into_iter()
        .filter(|file| {
            std::fs::write(&at, std::fs::read(file).unwrap()).unwrap();
            std::fs::set_permissions(&at, std::fs::Permissions::from_mode(0o600)).unwrap();
            DevFallbackCustody::new(root_id, probe.clone())
                .unwrap_store_key(&wrapped, label)
                .is_ok()
        })
        .collect()
}

fn offline_run(dir: &Path, how: &Offline<'_>) -> (i32, String, String) {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let status = run(&data_dir(dir), how, &mut out, &mut err);
    (
        status,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

struct Recorded {
    dir: PathBuf,
    log: Mutex<Vec<String>>,
}

impl Hands for Recorded {
    fn end_owner(&self, owner: Owner) -> bool {
        let standing = [
            "derived/derived.db",
            "derived/master.key",
            "derived/owner.pid",
        ]
        .iter()
        .all(|file| self.dir.join(file).exists());
        self.log.lock().unwrap().push(format!(
            "end pid={} started={} all_standing={standing}",
            owner.pid, owner.started
        ));
        true
    }

    fn destroy_key_file(&self, data_dir: &Path, file: &str) -> bool {
        self.log.lock().unwrap().push(format!("key {file}"));
        SystemHands.destroy_key_file(data_dir, file)
    }

    fn remove_entry(&self, data_dir: &Path, name: &str) -> u64 {
        self.log.lock().unwrap().push(format!("remove {name}"));
        SystemHands.remove_entry(data_dir, name)
    }
}

const SPARE: Row = Row {
    name: "spare",
    key_files: &["spare/spare.key"],
    owner_pid_file: Some("spare/owner.pid"),
    marks_dir: None,
};

#[test]
fn the_offline_delete_leaves_no_store() {
    let dir = kept_dir("delete-all-offline-");
    let id = a_store_with_a_segment(&dir);
    let key_file = std::fs::read(dir.join("store/store.key")).unwrap();
    assert_eq!(files_that_unwrap(&dir, &key_file).len(), 1);
    std::fs::create_dir(dir.join("derived")).unwrap();
    std::fs::write(dir.join("derived/derived.db"), b"derived rows").unwrap();
    std::fs::write(dir.join("derived/master.key"), [7u8; 64]).unwrap();
    std::fs::write(
        dir.join("derived/owner.pid"),
        format!("pid={} started=1700000000123456\n", std::process::id()),
    )
    .unwrap();
    let profile = dir.join("agents/profiles/50000000000000000000000000000001");
    std::fs::create_dir_all(&profile).unwrap();
    std::fs::write(profile.join("Preferences"), b"a runner's profile").unwrap();
    std::fs::create_dir(dir.join("spare")).unwrap();
    std::fs::write(dir.join("spare/spare.key"), [9u8; 32]).unwrap();
    std::fs::write(dir.join("planted.txt"), b"not the registry's").unwrap();
    std::fs::write(
        dir.join("daemon-state.json"),
        b"{\"version\":1,\"desired\":\"paused\",\"store_made\":true}",
    )
    .unwrap();
    let mut table = rows();
    table.extend([SPARE]);
    let hands = Recorded {
        dir: dir.clone(),
        log: Mutex::new(Vec::new()),
    };
    let how = Offline {
        rows: &table,
        select: custody_policy::of_this_build,
        hands: &hands,
    };
    let given = ["--delete-all", "--data-dir", dir.to_str().unwrap()].map(Into::into);
    assert_eq!(
        parse(given, &halvo_control::paths::SystemAccount),
        Ok(Mode::DeleteAllOfADataDir {
            data_dir: data_dir(&dir)
        })
    );

    let (status, out, err) = offline_run(&dir, &how);

    assert_eq!(status, 0, "out: {out} err: {err}");
    let shown = halvo_control::paths::one_line(dir.to_str().unwrap());
    let mut lines = out.lines();
    assert_eq!(lines.next(), Some(format!("data_dir={shown}").as_str()));
    let deleted = lines.next().unwrap();
    assert!(
        deleted.starts_with("deleted segments_shredded="),
        "{deleted}"
    );
    assert!(deleted.contains(" root_destroyed=yes "), "{deleted}");
    assert!(
        deleted.contains(" entries_removed=7 entries_left=0 unknown_entries=1 intent=written"),
        "{deleted}"
    );
    assert!(!deleted.contains("store_id="), "{deleted}");
    assert_eq!(lines.next(), None);
    assert!(!dir.join("store/store.key").exists());
    assert!(segment_files(&dir).is_empty());
    assert!(root_files(&dir).is_empty(), "{:?}", root_files(&dir));
    assert!(!dir.join("derived").exists());
    assert!(!dir.join("agents").exists());
    assert!(!dir.join("spare").exists());
    assert!(!dir.join("delete-all.intent").exists());
    assert_eq!(
        std::fs::read(dir.join("planted.txt")).unwrap(),
        b"not the registry's"
    );
    let state = std::fs::read_to_string(dir.join("daemon-state.json")).unwrap();
    assert!(!state.contains("\"store_made\":true"), "{state}");
    assert_eq!(
        *hands.log.lock().unwrap(),
        [
            format!(
                "end pid={} started=1700000000123456 all_standing=true",
                std::process::id()
            ),
            "key derived/master.key".to_owned(),
            "remove derived".to_owned(),
            "remove agents".to_owned(),
            "key spare/spare.key".to_owned(),
            "remove spare".to_owned(),
        ]
    );
    assert!(
        files_that_unwrap(&dir, &key_file).is_empty(),
        "a file under the data directory still unwraps the store's key, store id {id}"
    );

    let (again, out, err) = offline_run(&dir, &how);
    assert_eq!(again, 0, "err: {err}");
    assert_eq!(out, format!("data_dir={shown}\nno_store\n"));
}

#[test]
fn a_delete_ends_the_derived_key_and_a_new_store_makes_a_new_one() {
    let dir = kept_dir("delete-all-derived-");
    a_store_with_a_segment(&dir);
    let derived = KeyLabel::new("derived").unwrap();
    let first = DataDirStore::at(&dir).derived_store_key().unwrap();
    let key_file = std::fs::read(dir.join("derived/master.key")).unwrap();
    assert_eq!(files_that_unwrap_under(&dir, &key_file, derived).len(), 1);
    let mut table = rows();
    if !table.iter().any(|row| row.name == "derived") {
        table.push(Row {
            name: "derived",
            key_files: &["derived/master.key"],
            owner_pid_file: Some("derived/owner.pid"),
            marks_dir: None,
        });
    }
    let how = Offline {
        rows: &table,
        select: custody_policy::of_this_build,
        hands: &SystemHands,
    };

    let (status, out, err) = offline_run(&dir, &how);

    assert_eq!(status, 0, "out: {out} err: {err}");
    assert!(!dir.join("derived/master.key").exists());
    assert!(!dir.join("derived").exists());
    assert!(files_that_unwrap_under(&dir, &key_file, derived).is_empty());
    assert!(matches!(
        DataDirStore::at(&dir).create(Custody::RootFile),
        Opened::ReadWrite { .. }
    ));
    let second = DataDirStore::at(&dir).derived_store_key().unwrap();
    assert_ne!(second.expose(), first.expose());
}

#[test]
fn the_offline_delete_in_a_linked_data_directory_answers_0() {
    let root = kept_dir("delete-all-linked-");
    let target = root.join("data");
    std::fs::create_dir(&target).unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o700)).unwrap();
    let linked = root.join("linked");
    std::os::unix::fs::symlink(&target, &linked).unwrap();
    a_store_with_a_segment(&linked);
    std::fs::write(
        target.join("daemon-state.json"),
        b"{\"version\":1,\"desired\":\"paused\",\"store_made\":true}",
    )
    .unwrap();
    let table = rows();
    let how = Offline {
        rows: &table,
        select: custody_policy::of_this_build,
        hands: &SystemHands,
    };

    let (status, out, err) = offline_run(&linked, &how);

    let state: serde_json::Value =
        serde_json::from_slice(&std::fs::read(target.join("daemon-state.json")).unwrap()).unwrap();
    let host = DataDirStore::at(&linked);
    let start = at_start(&linked, &rows(), &SystemHands, &host, &|| {});
    assert_eq!(
        (
            status,
            out.contains(" intent=written"),
            state["store_made"].as_bool().unwrap_or(false),
            start,
            host.probe()
        ),
        (0, true, false, AtStart::NoIntent, Probe::NoKeyFile),
        "out: {out} err: {err}"
    );
    assert!(!target.join("store/store.key").exists());
    assert!(segment_files(&target).is_empty());
    assert!(root_files(&target).is_empty(), "{:?}", root_files(&target));
    assert!(!target.join(super::INTENT_FILE).exists());
}

#[test]
fn a_start_in_a_linked_data_directory_finishes_the_delete_and_a_new_store_opens() {
    let root = kept_dir("delete-all-linked-start-");
    let target = root.join("data");
    std::fs::create_dir(&target).unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o700)).unwrap();
    let linked = root.join("linked");
    std::os::unix::fs::symlink(&target, &linked).unwrap();
    let old = a_store_with_a_segment(&linked);
    let key_file = std::fs::read(target.join("store/store.key")).unwrap();
    assert_eq!(files_that_unwrap(&target, &key_file).len(), 1);
    std::fs::write(
        target.join(super::INTENT_FILE),
        format!("{{\"v\":1,\"started_at\":\"2026-11-21T00:00:00Z\",\"entries\":[{{\"name\":\"store\",\"custody_kind\":\"root_file\",\"root_id\":\"{old}\"}}]}}"),
    )
    .unwrap();
    let host = DataDirStore::at(&linked);

    let found = at_start(&linked, &rows(), &SystemHands, &host, &|| {});

    assert_eq!(found, AtStart::Finished { whole: true });
    assert!(!target.join("store/store.key").exists());
    assert!(segment_files(&target).is_empty());
    assert!(files_that_unwrap(&target, &key_file).is_empty());
    assert!(!target.join(super::INTENT_FILE).exists());
    assert!(matches!(
        host.create(Custody::RootFile),
        Opened::ReadWrite { .. }
    ));
    assert!(matches!(
        host.open(Access::ReadWrite),
        Opened::ReadWrite { .. }
    ));
}

#[test]
fn every_state_on_disk_is_deleted_by_the_offline_caller() {
    fn maintenance_alone(recorded: Option<Custody>) -> Selection {
        match recorded {
            Some(_) => Selection::Open(OpenAs::MaintenanceOnly),
            None => Selection::Create(None),
        }
    }
    let table = rows();

    let unopened = kept_dir("delete-all-unopened-");
    a_store_with_a_segment(&unopened);
    let key_file = std::fs::read(unopened.join("store/store.key")).unwrap();
    std::fs::write(unopened.join("store/store.key"), vec![0u8; key_file.len()]).unwrap();
    let how = Offline {
        rows: &table,
        select: custody_policy::of_this_build,
        hands: &SystemHands,
    };
    let (status, out, err) = offline_run(&unopened, &how);
    assert_eq!(status, 0, "a store that does not open: out {out} err {err}");
    assert!(!unopened.join("store/store.key").exists());
    assert!(segment_files(&unopened).is_empty());
    assert!(root_files(&unopened).is_empty());
    assert!(files_that_unwrap(&unopened, &key_file).is_empty());

    let rejected = kept_dir("delete-all-rejected-");
    a_store_with_a_segment(&rejected);
    let key_file = std::fs::read(rejected.join("store/store.key")).unwrap();
    let how = Offline {
        rows: &table,
        select: maintenance_alone,
        hands: &SystemHands,
    };
    let (status, out, err) = offline_run(&rejected, &how);
    assert_eq!(status, 0, "a policy-rejected store: out {out} err {err}");
    assert!(
        out.contains("deleted segments_shredded=1 keys_destroyed=2 "),
        "the handle of the maintenance slot did not delete: {out}"
    );
    assert!(!rejected.join("store/store.key").exists());
    assert!(segment_files(&rejected).is_empty());
    assert!(root_files(&rejected).is_empty());
    assert!(files_that_unwrap(&rejected, &key_file).is_empty());

    let refused = kept_dir("delete-all-refused-");
    let id = a_store_with_a_segment(&refused);
    let root = root_files(&refused);
    assert_eq!(root.len(), 1);
    let root_name = root[0].file_name().unwrap().to_str().unwrap().to_owned();
    struct Immutable(std::ffi::CString);
    impl Immutable {
        fn set(path: &Path) -> Immutable {
            let path = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
            assert_eq!(
                unsafe { libc::chflags(path.as_ptr(), libc::UF_IMMUTABLE) },
                0
            );
            Immutable(path)
        }
    }
    impl Drop for Immutable {
        fn drop(&mut self) {
            unsafe { libc::chflags(self.0.as_ptr(), 0) };
        }
    }
    let immutable = Immutable::set(&root[0]);
    let how = Offline {
        rows: &table,
        select: custody_policy::of_this_build,
        hands: &SystemHands,
    };
    let (status, out, err) = offline_run(&refused, &how);
    drop(immutable);
    assert_eq!(status, 1, "a refused root: out {out} err {err}");
    assert!(!refused.join("store/store.key").exists());
    assert!(!refused.join("store/store.db").exists());
    assert!(segment_files(&refused).is_empty());
    assert!(root[0].exists());
    assert!(out.contains(" root_destroyed=no "), "{out}");
    assert!(
        out.contains(&format!("root_id={id}")) || err.contains(&root_name),
        "the root that stays is not named: out {out} err {err}"
    );
}

#[test]
fn a_held_store_answers_busy_and_removes_nothing() {
    if let Some(dir) = std::env::var_os(HOLD) {
        let held = InstanceLock::take(&data_dir(Path::new(&dir)))
            .unwrap()
            .unwrap();
        let mut out = std::io::stdout().lock();
        writeln!(out, "delete-all-holder: held").unwrap();
        out.flush().unwrap();
        let mut rest = Vec::new();
        let _ = std::io::stdin().read_to_end(&mut rest);
        drop(held);
        return;
    }
    let dir = kept_dir("delete-all-held-");
    a_store_with_a_segment(&dir);
    let before = snapshot(&dir);
    let mut holder = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", HELD_TEST, "--nocapture", "--test-threads=1"])
        .env(HOLD, &dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(holder.stdout.take().unwrap()).lines();
    let started = Instant::now();
    loop {
        let line = lines.next().expect("the holder ended early").unwrap();
        if line.contains("delete-all-holder: held") {
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(60));
    }
    let table = rows();
    let how = Offline {
        rows: &table,
        select: custody_policy::of_this_build,
        hands: &SystemHands,
    };

    let (status, out, err) = offline_run(&dir, &how);

    drop(holder.stdin.take());
    for line in lines.by_ref() {
        line.unwrap();
    }
    assert!(holder.wait().unwrap().success(), "the holder failed");
    assert_eq!(status, 75, "out: {out} err: {err}");
    assert_eq!(snapshot(&dir), before, "a held directory was touched");
}

struct WatchedHost {
    host: DataDirStore,
    calls: Arc<Mutex<Vec<&'static str>>>,
    gate: Mutex<Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>>,
}

impl StoreHost for WatchedHost {
    fn probe(&self) -> Probe {
        self.calls.lock().unwrap().push("probe");
        self.host.probe()
    }

    fn open(&self, access: Access) -> Opened {
        self.calls.lock().unwrap().push("open");
        self.host.open(access)
    }

    fn create(&self, custody: Custody) -> Opened {
        self.calls.lock().unwrap().push("create");
        self.host.create(custody)
    }

    fn delete(&self) -> TailAnswer {
        self.calls.lock().unwrap().push("delete");
        if let Some((began, go)) = self.gate.lock().unwrap().take() {
            began.send(()).unwrap();
            go.recv().unwrap();
        }
        self.host.delete()
    }
}

#[test]
fn an_intent_at_start_is_finished_before_the_open() {
    let dir = kept_dir("delete-all-at-start-");
    let old = a_store_with_a_segment(&dir);
    let old_root = root_files(&dir);
    assert_eq!(old_root.len(), 1);
    std::fs::write(
        dir.join("delete-all.intent"),
        format!("{{\"v\":1,\"started_at\":\"2026-11-21T00:00:00Z\",\"entries\":[{{\"name\":\"store\",\"custody_kind\":\"root_file\",\"root_id\":\"{old}\"}}]}}"),
    )
    .unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let (began, begun) = mpsc::channel();
    let (asked, go) = mpsc::channel();
    let host = WatchedHost {
        host: DataDirStore::at(&dir),
        calls: calls.clone(),
        gate: Mutex::new(Some((began, go))),
    };
    let path = dir.clone();
    let finish = Box::new(move |host: &dyn StoreHost, deleting: &dyn Fn()| {
        at_start(&path, &rows(), &SystemHands, host, deleting)
    });
    let opener = Opener::start_with(
        finish,
        Box::new(host),
        custody_policy::of_this_build,
        CreateRule::FirstRun,
    )
    .unwrap();

    begun.recv_timeout(Duration::from_secs(60)).unwrap();
    assert_eq!(opener.slot(), Slot::Deleting);
    assert_eq!(*calls.lock().unwrap(), ["probe", "delete"]);
    asked.send(()).unwrap();
    let started = Instant::now();
    let held = loop {
        if let Some(held) = opener.take() {
            break held;
        }
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "the open never handed a store; slot {:?}",
            opener.slot()
        );
        std::thread::sleep(Duration::from_millis(10));
    };

    let Held::Capture { id, .. } = held else {
        panic!("the fresh store did not open for capture");
    };
    assert_ne!(id.to_string(), old);
    assert_eq!(opener.slot(), Slot::Open { id });
    let calls = calls.lock().unwrap().clone();
    assert_eq!(calls[..2], ["probe", "delete"]);
    assert!(calls[2..].contains(&"create"), "{calls:?}");
    assert!(!old_root[0].exists());
    assert!(!dir.join("delete-all.intent").exists());
    assert_eq!(
        at_start(&dir, &rows(), &SystemHands, &DataDirStore::at(&dir), &|| {}),
        AtStart::NoIntent
    );
}

fn opener_after_an_intent(dir: &Path) -> (Opener, mpsc::Receiver<AtStart>) {
    let (found, answer) = mpsc::channel();
    let path = dir.to_path_buf();
    let finish = Box::new(move |host: &dyn StoreHost, deleting: &dyn Fn()| {
        let at = at_start(&path, &rows(), &SystemHands, host, deleting);
        found.send(at).unwrap();
        at
    });
    let opener = Opener::start_with(
        finish,
        Box::new(DataDirStore::at(dir)),
        custody_policy::of_this_build,
        CreateRule::FirstRun,
    )
    .unwrap();
    (opener, answer)
}

#[test]
fn a_start_whose_finish_cannot_delete_keeps_the_intent_and_opens_nothing() {
    if let Some(dir) = std::env::var_os(STORE_HOLD) {
        let Opened::ReadWrite { store, .. } =
            DataDirStore::at(Path::new(&dir)).open(Access::ReadWrite)
        else {
            panic!("the holder did not open the store");
        };
        let mut out = std::io::stdout().lock();
        writeln!(out, "delete-all-store-holder: held").unwrap();
        out.flush().unwrap();
        let mut rest = Vec::new();
        let _ = std::io::stdin().read_to_end(&mut rest);
        drop(store);
        return;
    }
    let dir = kept_dir("delete-all-start-held-");
    let old = a_store_with_a_segment(&dir);
    let old_root = root_files(&dir);
    assert_eq!(old_root.len(), 1);
    std::fs::write(
        dir.join("delete-all.intent"),
        format!("{{\"v\":1,\"started_at\":\"2026-11-21T00:00:00Z\",\"entries\":[{{\"name\":\"store\",\"custody_kind\":\"root_file\",\"root_id\":\"{old}\"}}]}}"),
    )
    .unwrap();
    let mut holder = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", START_TEST, "--nocapture", "--test-threads=1"])
        .env(STORE_HOLD, &dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(holder.stdout.take().unwrap()).lines();
    let started = Instant::now();
    loop {
        let line = lines.next().expect("the holder ended early").unwrap();
        if line.contains("delete-all-store-holder: held") {
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(60));
    }

    let (first, answer) = opener_after_an_intent(&dir);
    let found = answer.recv_timeout(Duration::from_secs(60)).unwrap();
    std::thread::sleep(Duration::from_millis(500));
    let intent_stays = dir.join("delete-all.intent").exists();
    let (slot, tries) = (first.slot(), first.tries());
    first.stop();
    assert!(first.wait_for_its_call(Duration::from_secs(60)));
    let handed = first.take().is_some();
    drop(holder.stdin.take());
    for line in lines.by_ref() {
        line.unwrap();
    }
    assert!(holder.wait().unwrap().success(), "the holder failed");

    assert_eq!(found, AtStart::Finished { whole: false });
    assert!(
        intent_stays,
        "a finish that deleted nothing removed the intent"
    );
    assert_eq!((slot, tries, handed), (Slot::DeleteUnfinished, 0, false));
    assert!(dir.join("store/store.key").exists());

    let (second, answer) = opener_after_an_intent(&dir);
    assert_eq!(
        answer.recv_timeout(Duration::from_secs(60)).unwrap(),
        AtStart::Finished { whole: true }
    );
    let started = Instant::now();
    let held = loop {
        if let Some(held) = second.take() {
            break held;
        }
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "the second start never handed a store; slot {:?}",
            second.slot()
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    let Held::Capture { id, .. } = held else {
        panic!("the fresh store did not open for capture");
    };
    assert_ne!(id.to_string(), old);
    assert!(!old_root[0].exists());
    assert!(!dir.join("delete-all.intent").exists());
}

#[test]
fn a_start_whose_retry_is_again_not_whole_sets_the_field_too() {
    use halvo_control::proto::{
        decode_reply, delete_decision, encode_reply, DeleteDecision, Life, Platform, Reply,
        StoreState,
    };
    let dir = kept_dir("delete-all-start-again-");
    let old = a_store_with_a_segment(&dir);
    std::fs::write(
        dir.join("delete-all.intent"),
        format!("{{\"v\":1,\"started_at\":\"2026-11-21T00:00:00Z\",\"entries\":[{{\"name\":\"store\",\"custody_kind\":\"root_file\",\"root_id\":\"{old}\"}}]}}"),
    )
    .unwrap();
    let mut holder = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", START_TEST, "--nocapture", "--test-threads=1"])
        .env(STORE_HOLD, &dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(holder.stdout.take().unwrap()).lines();
    let started = Instant::now();
    loop {
        let line = lines.next().expect("the holder ended early").unwrap();
        if line.contains("delete-all-store-holder: held") {
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(60));
    }

    let (opener, answer) = opener_after_an_intent(&dir);
    let found = answer.recv_timeout(Duration::from_secs(60)).unwrap();
    std::thread::sleep(Duration::from_millis(500));
    let slot = opener.slot();
    opener.stop();
    assert!(opener.wait_for_its_call(Duration::from_secs(60)));
    let handed = opener.take().is_some();
    drop(holder.stdin.take());
    for line in lines.by_ref() {
        line.unwrap();
    }
    assert!(holder.wait().unwrap().success(), "the holder failed");
    assert_eq!((found, handed), (AtStart::Finished { whole: false }, false));

    let parts = crate::daemon::server::StatusParts {
        board: crate::daemon::board::Board::new(Platform::Macos),
        desired: crate::daemon::state::Desired::Running,
        lease: halvo_control::proto::Lease::Fresh,
        slot,
        obstructed: None,
        health: crate::daemon::state::StateHealth::Ok,
        cap: crate::daemon::state::Cap::Known(halvo_control::model::StorageCap::DEFAULT),
        switches: crate::floor::Switches::default(),
        once_granted: Vec::new(),
        asked: Vec::new(),
        input_keys: false,
        held: false,
        typing_off_unsaved: None,
        left_out_in_doubt: false,
    };
    let facts = crate::daemon::server::RunFacts {
        started_at: halvo_control::model::UtcSecond::from_unix_seconds(1_800_000_000).unwrap(),
        parent: halvo_control::proto::Parent::None,
        last_stop: halvo_control::proto::LastStop::None,
        rss_peak_bytes: 0,
    };
    let status = crate::daemon::server::status_of(&parts, &facts).unwrap();
    let Ok(Reply::Status(read)) = decode_reply(&encode_reply(&Reply::Status(status))) else {
        panic!("the status did not cross the wire");
    };
    let f = read.fields();
    assert_eq!(
        (f.store, f.store_delete_unfinished, f.store_id),
        (StoreState::Deleting, true, None)
    );
    assert_eq!(
        delete_decision(
            Life::Going,
            f.capture,
            f.store,
            f.store_error,
            f.store_delete_unfinished,
            f.store_id,
            None
        ),
        DeleteDecision::Accept
    );
}

#[test]
fn a_finish_whose_intent_cannot_be_removed_is_not_whole() {
    struct Immutable(std::ffi::CString);
    impl Drop for Immutable {
        fn drop(&mut self) {
            unsafe { libc::chflags(self.0.as_ptr(), 0) };
        }
    }
    let dir = kept_dir("delete-all-intent-kept-");
    let old = a_store_with_a_segment(&dir);
    let old_root = root_files(&dir);
    assert_eq!(old_root.len(), 1);
    let intent = dir.join("delete-all.intent");
    std::fs::write(
        &intent,
        format!("{{\"v\":1,\"started_at\":\"2026-11-21T00:00:00Z\",\"entries\":[{{\"name\":\"store\",\"custody_kind\":\"root_file\",\"root_id\":\"{old}\"}}]}}"),
    )
    .unwrap();
    let flagged = Immutable(std::ffi::CString::new(intent.to_str().unwrap()).unwrap());
    assert_eq!(
        unsafe { libc::chflags(flagged.0.as_ptr(), libc::UF_IMMUTABLE) },
        0
    );

    let found = at_start(&dir, &rows(), &SystemHands, &DataDirStore::at(&dir), &|| {});
    drop(flagged);

    assert!(!dir.join("store/store.key").exists());
    assert!(!old_root[0].exists());
    assert!(segment_files(&dir).is_empty());
    assert!(intent.exists());
    assert_eq!(
        found,
        AtStart::Finished { whole: false },
        "a finish whose intent stays read whole"
    );
}

#[test]
fn an_offline_delete_whose_folder_is_not_flushed_exits_1_and_keeps_the_intent() {
    /// The product's hands, which take the read permission from the folder
    /// roots once the walk has removed an entry, so that the flush after the
    /// walk cannot open that folder.
    struct ClosingRoots(PathBuf);
    impl Hands for ClosingRoots {
        fn end_owner(&self, owner: Owner) -> bool {
            SystemHands.end_owner(owner)
        }

        fn destroy_key_file(&self, data_dir: &Path, file: &str) -> bool {
            SystemHands.destroy_key_file(data_dir, file)
        }

        fn remove_entry(&self, data_dir: &Path, name: &str) -> u64 {
            let left = SystemHands.remove_entry(data_dir, name);
            std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o000)).unwrap();
            left
        }
    }
    /// Gives the folder its mode back when the test ends, also after a panic.
    struct ModeBack(PathBuf, u32);
    impl Drop for ModeBack {
        fn drop(&mut self) {
            let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(self.1));
        }
    }
    let dir = kept_dir("delete-all-not-flushed-");
    a_store_with_a_segment(&dir);
    std::fs::create_dir(dir.join("derived")).unwrap();
    std::fs::write(dir.join("derived/derived.db"), b"derived rows").unwrap();
    let roots = dir.join("roots");
    let mode = std::fs::metadata(&roots).unwrap().permissions().mode() & 0o7777;
    let mode_back = ModeBack(roots.clone(), mode);
    let table = rows();
    let hands = ClosingRoots(roots);
    let how = Offline {
        rows: &table,
        select: custody_policy::of_this_build,
        hands: &hands,
    };

    let (status, out, err) = offline_run(&dir, &how);
    drop(mode_back);

    assert_eq!(status, 1, "out: {out} err: {err}");
    let deleted = out.lines().nth(1).unwrap_or_default();
    assert!(
        deleted.starts_with("deleted ") && deleted.contains(" entries_left=0 "),
        "{out}"
    );
    assert_eq!(
        err,
        "halvo-capture: the folder roots was not flushed to the drive; the intent stays, and the next start finishes the delete\n"
    );
    assert!(dir.join("delete-all.intent").exists());
    assert!(!dir.join("store/store.key").exists());
    assert!(root_files(&dir).is_empty(), "{:?}", root_files(&dir));
    assert!(!dir.join("derived").exists());
}

/// A plain child of the test in a runner's or a browser's place: /bin/cat, which lives until the end of its
/// standard input, which the test holds. A thread reaps it and hands its exit status over; dropped, the stand
/// closes that input, so the test itself ends no process by its pid.
struct Stand {
    pid: u32,
    created: u64,
    input: Option<std::process::ChildStdin>,
    ended: mpsc::Receiver<std::process::ExitStatus>,
}

impl Stand {
    fn start() -> Stand {
        let mut child = Command::new("/bin/cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let input = child.stdin.take();
        let pid = child.id();
        let created = start_time_of(pid).unwrap();
        let (tx, ended) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(child.wait().unwrap());
        });
        Stand {
            pid,
            created,
            input,
            ended,
        }
    }

    /// The line `<word> pid=<n> created=<microseconds> group=<n>` of a mark, with `created` as the creation time.
    fn line(&self, word: &str, created: u64) -> String {
        format!(
            "{word} pid={} created={created} group={}\n",
            self.pid, self.pid
        )
    }
}

impl Drop for Stand {
    fn drop(&mut self) {
        drop(self.input.take());
        let _ = self.ended.recv_timeout(Duration::from_secs(60));
    }
}

/// Writes the mark of the run `run` under agents/marks of `dir` with the bytes `text`.
fn write_mark(dir: &Path, run: &str, text: &[u8]) {
    let marks = dir.join("agents/marks");
    std::fs::create_dir_all(&marks).unwrap();
    std::fs::write(marks.join(run), text).unwrap();
}

fn product_how(table: &[Row]) -> Offline<'_> {
    Offline {
        rows: table,
        select: custody_policy::of_this_build,
        hands: &SystemHands,
    }
}

#[test]
fn a_runner_that_a_mark_names_is_ended() {
    use std::os::unix::process::ExitStatusExt;
    let dir = kept_dir("delete-all-marks-ended-");
    a_store_with_a_segment(&dir);
    let runner = Stand::start();
    let browser = Stand::start();
    let run = "60000000000000000000000000000001";
    let text = format!(
        "mark run={run}\n{}{}",
        runner.line("runner", runner.created),
        browser.line("browser", browser.created)
    );
    write_mark(&dir, run, text.as_bytes());
    let table = rows();

    let (status, out, err) = offline_run(&dir, &product_how(&table));

    for (name, stand) in [("runner", &runner), ("browser", &browser)] {
        let ended = stand
            .ended
            .recv_timeout(Duration::from_secs(30))
            .unwrap_or_else(|e| panic!("the {name} did not end: {e}; out: {out} err: {err}"));
        assert_eq!(ended.signal(), Some(libc::SIGTERM), "the {name}");
    }
    assert_eq!((status, err.as_str()), (0, ""), "out: {out}");
    assert!(!dir.join("agents").exists());
}

#[test]
fn a_live_pid_with_another_creation_time_is_left() {
    let dir = kept_dir("delete-all-marks-left-");
    a_store_with_a_segment(&dir);
    let runner = Stand::start();
    let run = "60000000000000000000000000000002";
    let text = format!(
        "mark run={run}\n{}",
        runner.line("runner", runner.created + 1)
    );
    write_mark(&dir, run, text.as_bytes());
    let table = rows();

    let (status, out, err) = offline_run(&dir, &product_how(&table));

    assert_eq!(
        start_time_of(runner.pid),
        Some(runner.created),
        "the process was ended; out: {out} err: {err}"
    );
    assert!(runner.ended.try_recv().is_err(), "the process ended");
    assert_eq!((status, err.as_str()), (0, ""), "out: {out}");
    assert!(!dir.join("agents").exists());
}

#[test]
fn an_unreadable_mark_is_passed_and_counted() {
    let dir = kept_dir("delete-all-marks-unreadable-");
    a_store_with_a_segment(&dir);
    write_mark(
        &dir,
        "60000000000000000000000000000003",
        b"mark run=60000000000000000000000000000004\n",
    );
    write_mark(
        &dir,
        "60000000000000000000000000000005",
        b"mark run=6000000000000000000000000000000\xff\n",
    );
    let table = rows();

    let (status, out, err) = offline_run(&dir, &product_how(&table));

    assert_eq!(
        err,
        "halvo-capture: 2 of the runs' marks could not be read and were passed; what they name was not ended\n",
        "out: {out}"
    );
    assert_eq!(status, 0, "out: {out}");
    assert!(out.contains(" entries_left=0 "), "{out}");
    assert!(!dir.join("agents").exists());
}

#[test]
fn a_walk_whose_store_delete_fails_prints_what_it_removed_and_what_stays_and_ends_1() {
    fn entries_of(folder: &Path, besides: &[&str]) -> u64 {
        std::fs::read_dir(folder)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                if besides.contains(&entry.file_name().to_str().unwrap()) {
                    return 0;
                }
                1 + if entry.file_type().unwrap().is_dir() {
                    entries_of(&entry.path(), &[])
                } else {
                    0
                }
            })
            .sum()
    }
    let dir = kept_dir("delete-all-store-fails-");
    a_store_with_a_segment(&dir);
    std::fs::create_dir(dir.join("derived")).unwrap();
    std::fs::write(dir.join("derived/derived.db"), b"derived rows").unwrap();
    std::fs::write(
        dir.join("derived/master.key"),
        b"a key of the derived store",
    )
    .unwrap();
    std::fs::create_dir(dir.join("agents")).unwrap();
    std::fs::write(dir.join("agents/profile"), b"a runner's profile").unwrap();
    let table = rows();

    let fails = super::stand_in::fails();
    let (status, out, err) = offline_run(&dir, &product_how(&table));
    drop(fails);

    let files_left = entries_of(
        &dir.join("store"),
        &[
            "store.key",
            "store.db",
            "store.db-wal",
            "store.db-shm",
            "store.db-journal",
        ],
    );
    assert!(files_left > 0, "the segment does not stand");
    let shown = halvo_control::paths::one_line(dir.to_str().unwrap());
    assert_eq!(
        (status, out),
        (
            1,
            format!(
                "data_dir={shown}\ndeleted segments_shredded=unknown keys_destroyed=unknown root_destroyed=unknown root_id=- files_left={files_left} database_left=yes not_followed=0 entries_removed=4 entries_left=2 unknown_entries=0 intent=written\n"
            )
        ),
        "err: {err}"
    );
    assert!(
        err.contains("halvo-capture: a stand-in of the store's delete failed\n"),
        "{err}"
    );
    assert!(!dir.join("derived").exists());
    assert!(!dir.join("agents").exists());
    assert!(dir.join("store/store.key").exists());
    assert_eq!(root_files(&dir).len(), 1);
}

#[test]
fn a_data_folder_that_cannot_be_used_ends_66_and_removes_nothing() {
    let root = kept_dir("delete-all-unusable-");
    let file = root.join("file");
    std::fs::write(&file, b"a shortcut").unwrap();
    let link = root.join("link");
    std::os::unix::fs::symlink(root.join("gone"), &link).unwrap();
    let table = rows();

    for (dir, cause) in [
        (&file, "an entry that is no directory"),
        (&link, "a link that leads nowhere"),
    ] {
        let (status, out, err) = offline_run(dir, &product_how(&table));
        let shown = halvo_control::paths::one_line(dir.to_str().unwrap());
        assert_eq!(
            (status, out),
            (
                halvo_control::exit::DATA_DIR_UNUSABLE,
                format!("data_dir={shown}\n")
            ),
            "err: {err}"
        );
        assert_eq!(
            err,
            format!("halvo-capture: the data directory's path meets {cause}\n")
        );
    }
    let mut names: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    names.sort();
    assert_eq!(names, ["file", "link"]);
    assert_eq!(std::fs::read(&file).unwrap(), b"a shortcut");
}
