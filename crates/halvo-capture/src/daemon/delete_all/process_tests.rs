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

use super::{at_start, rows, run, AtStart, Hands, Offline, Owner, Row, SystemHands};
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
                .unwrap_store_key(&wrapped, KeyLabel::STORE)
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

const DERIVED: Row = Row {
    name: "derived",
    key_files: &["derived/master.key"],
    owner_pid_file: Some("derived/owner.pid"),
};

const SPARE: Row = Row {
    name: "spare",
    key_files: &["spare/spare.key"],
    owner_pid_file: Some("spare/owner.pid"),
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
    std::fs::create_dir(dir.join("spare")).unwrap();
    std::fs::write(dir.join("spare/spare.key"), [9u8; 32]).unwrap();
    std::fs::write(dir.join("planted.txt"), b"not the registry's").unwrap();
    std::fs::write(
        dir.join("daemon-state.json"),
        b"{\"version\":1,\"desired\":\"paused\",\"store_made\":true}",
    )
    .unwrap();
    let mut table = rows();
    table.extend([DERIVED, SPARE]);
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
        deleted.contains(" entries_removed=6 entries_left=0 unknown_entries=1 intent=written"),
        "{deleted}"
    );
    assert!(!deleted.contains("store_id="), "{deleted}");
    assert_eq!(lines.next(), None);
    assert!(!dir.join("store/store.key").exists());
    assert!(segment_files(&dir).is_empty());
    assert!(root_files(&dir).is_empty(), "{:?}", root_files(&dir));
    assert!(!dir.join("derived").exists());
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
    assert_eq!((slot, tries, handed), (Slot::Deleting, 0, false));
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
