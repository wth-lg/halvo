#![cfg(all(target_os = "macos", debug_assertions))]

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use halvo_control::model::UtcDay;
use halvo_control::paths::DataDir;

use super::{rows, run, Hands, Offline, Owner, Row, SystemHands};
use crate::custody::dev_fallback::DevFallbackCustody;
use crate::custody::{KeyCustody, KeyLabel, RootId, WrappedKey};
use crate::daemon::custody_policy::{self, Custody};
use crate::daemon::instance::tests::kept_dir;
use crate::daemon::instance::InstanceLock;
use crate::daemon::port::{
    DataDirStore, EntryKind, KindEntry, Opened, SegmentId, SegmentWrite, StoreHost,
};
use crate::daemon::{parse, Mode};
use crate::event::Modality;

const HOLD: &str = "HALVO_DELETE_ALL_HOLD";
const HELD_TEST: &str =
    "daemon::delete_all::process_tests::a_held_store_answers_busy_and_removes_nothing";

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
