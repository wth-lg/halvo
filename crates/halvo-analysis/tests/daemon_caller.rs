//! The tests of the daemon's own caller of the two slots (halvo-capture, daemon::r2::daemon_caller): each makes the
//! caller over a new data directory that holds a capture store, with the directory of the built worker and owner as
//! the place of the programs, and reads what the caller and a slot over it did. No thread of these tests passes the
//! cursor, so a test writes the figures of a read into the cell as the cursor does. The process has one cell, so one
//! lock keeps the tests apart, and each starts from the cell that the caller leaves while an intent stands.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use halvo_capture::daemon::custody_policy::Custody;
use halvo_capture::daemon::port::{DataDirStore, Opened, StoreHost};
use halvo_capture::daemon::r2::daemon_caller::{cell, DaemonCaller, Reading};
use halvo_capture::daemon::r2::owner_slot::{Caller, OwnerSlot, Place};
use halvo_capture::daemon::r2::worker_slot::{
    Answer, Committed, MayRun, Origin, Server, WorkerSlot,
};
use halvo_control::proto::ErrorCode;
use halvo_schema_core::ids::{Position, RequestId, StoreId};
use halvo_schema_core::request::{InterfaceRead, InterfaceRole};
use halvo_seam::fixture::FixtureTransport;

/// The longest wait for something that a slot or a child owes at once.
const WAIT: Duration = Duration::from_secs(30);

/// The file of the delete's intent in the data directory, as the registry of halvo-control names it.
const INTENT: &str = "delete-all.intent";

/// The lock that keeps the tests of this file apart.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

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
    fn bytes(&self) -> Vec<u8> {
        self.0.lock().unwrap().clone()
    }

    fn text(&self) -> String {
        String::from_utf8(self.bytes()).unwrap()
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
/// same target directory and profile, as `owner_binary` of tests/worker_slot.rs does.
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

/// The daemon's caller over a new data directory that holds a capture store, with what the test reads of it. A slot
/// that a test makes over it is dropped before it, and the lock of the tests last.
struct Rig {
    caller: Arc<DaemonCaller>,
    /// What the caller wrote.
    lines: Lines,
    /// What a worker slot over the caller wrote.
    slot_lines: Lines,
    scratch: Scratch,
    _one: MutexGuard<'static, ()>,
}

impl Drop for Rig {
    /// A test that fails prints what its caller and its slot wrote.
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("the caller wrote: {:?}", self.lines.text());
            eprintln!("the slot wrote: {:?}", self.slot_lines.text());
        }
    }
}

impl Rig {
    /// Makes a capture store in a new data directory under the root file's custody and drops it, as
    /// `capture_store_and_key` of the owner's crash tests does, and the caller over that directory. The cell is one
    /// for the process, so the caller is asked once while an intent stands, which is how the product makes the cell
    /// forget: the test starts with no figures and no ask.
    fn new(name: &str) -> Rig {
        let one = ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner);
        let scratch = Scratch::new(name);
        let host = DataDirStore::at(&scratch.0);
        let Opened::ReadWrite { store, .. } = host.create(Custody::RootFile) else {
            panic!("no capture store was made in {}", scratch.0.display());
        };
        drop(store);
        let lines = Lines::default();
        let caller = Arc::new(DaemonCaller::new(&scratch.0, Box::new(lines.clone())));
        let rig = Rig {
            caller,
            lines,
            slot_lines: Lines::default(),
            scratch,
            _one: one,
        };
        rig.write_intent();
        assert!(rig.caller.deleting(), "an intent stands and is not read");
        rig.remove_intent();
        assert!(!rig.caller.deleting(), "no intent stands and one is read");
        assert_eq!(cell().figures(), None);
        assert!(!cell().asked());
        rig
    }

    fn dir(&self) -> &Path {
        &self.scratch.0
    }

    fn write_intent(&self) {
        std::fs::write(self.dir().join(INTENT), b"{}").unwrap();
    }

    fn remove_intent(&self) {
        std::fs::remove_file(self.dir().join(INTENT)).unwrap();
    }

    fn master_key(&self) -> PathBuf {
        self.dir().join("derived").join("master.key")
    }

    fn place(&self) -> Place {
        Place {
            program_dir: owner_binary().parent().unwrap().to_path_buf(),
            data_dir: self.dir().to_path_buf(),
            idle_ms: None,
        }
    }

    fn as_caller(&self) -> Arc<dyn Caller> {
        Arc::clone(&self.caller) as Arc<dyn Caller>
    }

    /// An owner slot over the caller.
    fn owner_slot(&self) -> OwnerSlot {
        OwnerSlot::new(self.place(), self.as_caller())
    }

    /// A worker slot over the caller, whose read channel `server` serves, or with none the cursor.
    fn worker_slot(&self, server: Option<Server>) -> WorkerSlot {
        let log = Box::new(self.slot_lines.clone());
        match server {
            Some(server) => WorkerSlot::with_server(self.place(), self.as_caller(), log, server),
            None => WorkerSlot::new(self.place(), self.as_caller(), log),
        }
    }

    /// What the source gives the driver for one yes.
    fn reading(&self, now: Instant) -> Reading {
        self.caller.reading(now).expect("no read stands")
    }
}

/// The id of a capture store whose read a test writes into the cell.
fn capture() -> StoreId {
    StoreId::parse("0123456789abcdef0123456789abcdef").unwrap()
}

/// Writes the figures of one read from ordinal 0 into the cell, as the cursor does: the store, its watermark, and
/// 1 as its lowest live ordinal.
fn write_read(store: &StoreId, watermark: Position) {
    cell().keep(store.clone(), watermark, Some(1));
}

/// The yes that carries the watermark of `reading`.
fn yes(reading: &Reading) -> MayRun {
    MayRun::Yes {
        watermark: Some(reading.watermark),
    }
}

fn read_workflows() -> Vec<u8> {
    static NEXT: AtomicU8 = AtomicU8::new(1);
    let id = RequestId::from_random([NEXT.fetch_add(1, Ordering::Relaxed); 16]);
    InterfaceRead::workflows(&InterfaceRole::claim(), id, None)
        .to_bytes()
        .unwrap()
}

/// A report frame of the owner (contracts.md C24.9) whose key `position` holds `position`, or null.
fn report(position: Option<&Committed>) -> Vec<u8> {
    let position = position.map_or("null".to_owned(), |committed| {
        format!(
            r#"{{"store":"{}","seg":{},"idx":{}}}"#,
            committed.store.as_str(),
            committed.position.seg,
            committed.position.idx
        )
    });
    format!(
        r#"{{"report":{{"revisions":{{}},"counts":{{"window":0,"bounded":0,"kept":0}},"disk_bytes":0,"expiry":null,"position":{position}}}}}"#
    )
    .into_bytes()
}

/// Every file under `dir`.
fn files_under(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files_under(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn holds(text: &[u8], part: &[u8]) -> bool {
    text.windows(part.len()).any(|window| window == part)
}

/// Rule 1: two starts of the owner through the caller get the key and answer a relay, the second over the store
/// that the first made. With store/store.key moved away the next start answers `store_unavailable`, no owner is
/// left, and the caller's one line names the fault. The key's bytes, raw or as hex in either case, stand in no line
/// and in no file of the data directory: derived/master.key holds them wrapped alone.
#[test]
fn the_key_is_read_at_each_start() {
    let rig = Rig::new("caller-key");
    write_read(&capture(), Position { seg: 1, idx: 0 });
    let mut slot = rig.owner_slot();
    assert!(!rig.master_key().exists());
    slot.relay(&read_workflows()).unwrap();
    let first = slot.pid().expect("the relay started no owner");
    assert!(rig.master_key().is_file(), "the caller made no key file");
    slot.stop();
    slot.relay(&read_workflows()).unwrap();
    let second = slot.pid().expect("the relay started no owner");
    assert_ne!(first, second, "the second relay started no owner anew");
    slot.stop();
    assert_eq!(rig.lines.text(), "");

    let key_file = rig.dir().join("store").join("store.key");
    let away = rig.dir().join("store.key.away");
    std::fs::rename(&key_file, &away).unwrap();
    let refused = slot.relay(&read_workflows()).unwrap_err();
    assert_eq!(refused.code(), ErrorCode::StoreUnavailable, "{refused:?}");
    assert_eq!(slot.pid(), None);
    assert!(!rig.dir().join("derived").join("owner.pid").exists());
    assert_eq!(
        rig.lines.text(),
        "halvo-capture: no key of the derived store: no capture store stands in the data directory\n"
    );
    std::fs::rename(&away, &key_file).unwrap();

    let key = DataDirStore::at(rig.dir()).derived_store_key().unwrap();
    let hex: String = key
        .expose()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let forms = [
        ("raw", key.expose().to_vec()),
        ("hex", hex.clone().into_bytes()),
        ("upper-case hex", hex.to_uppercase().into_bytes()),
    ];
    let mut files = Vec::new();
    files_under(rig.dir(), &mut files);
    assert!(files.contains(&rig.master_key()), "{files:?}");
    for (form, bytes) in &forms {
        assert!(
            !holds(&rig.lines.bytes(), bytes),
            "a line holds the key as {form}"
        );
        for file in &files {
            let text = std::fs::read(file).unwrap();
            assert!(
                !holds(&text, bytes),
                "{} holds the key as {form}",
                file.display()
            );
        }
    }
}

/// Rule 2: while the intent file stands a pass with a yes answers that the delete runs, the caller gives no key,
/// writes no line and makes no derived/master.key, and the cell has forgotten the read. With the file gone, and a
/// read written anew, a worker starts.
#[test]
fn an_intent_on_disk_is_busy() {
    let rig = Rig::new("caller-intent");
    let mut slot = rig.worker_slot(None);
    let now = Instant::now();
    write_read(&capture(), Position { seg: 1, idx: 0 });
    let reading = rig.reading(now);
    rig.write_intent();
    assert_eq!(
        slot.pass(now, yes(&reading), &reading.origin),
        Answer::Deleting
    );
    assert!(rig.caller.key().is_none(), "a key while the intent stands");
    assert!(!rig.master_key().exists(), "derived/master.key was made");
    assert_eq!(rig.lines.text(), "");
    assert_eq!(cell().figures(), None);
    assert_eq!(slot.pid(), None);
    assert_eq!(slot.owner().pid(), None);

    rig.remove_intent();
    write_read(&capture(), Position { seg: 1, idx: 0 });
    let reading = rig.reading(now);
    assert_eq!(
        slot.pass(now, yes(&reading), &reading.origin),
        Answer::Started,
        "{}",
        rig.slot_lines.text()
    );
    assert!(slot.pid().is_some());
    assert!(rig.master_key().is_file());
}

/// Rule 3: after a null frame the source names no committed position; after two frames with positions the second;
/// after a frame that is cut short still the second; and after a null frame none again.
#[test]
fn the_newest_report_names_the_position() {
    let rig = Rig::new("caller-report");
    let store = capture();
    write_read(&store, Position { seg: 9, idx: 0 });
    let now = Instant::now();
    let committed = || rig.reading(now).origin.committed;
    let at = |seg, idx| Committed {
        store: store.clone(),
        position: Position { seg, idx },
    };
    rig.caller.report(&report(None));
    assert_eq!(committed(), None);
    rig.caller.report(&report(Some(&at(3, 5))));
    rig.caller.report(&report(Some(&at(4, 0))));
    assert_eq!(committed(), Some(at(4, 0)));
    let whole = report(Some(&at(7, 1)));
    rig.caller.report(&whole[..whole.len() - 3]);
    assert_eq!(committed(), Some(at(4, 0)));
    rig.caller.report(&report(None));
    assert_eq!(committed(), None);
}

/// Rule 6: with no read in the cell a relay answers `store_unavailable`: the slot made no folder derived/, which it
/// makes before the child, no pid file stands, and the caller was asked for no key, so it made no key file and
/// wrote no line. The refused start left an ask in the cell. After a read is written a relay is answered.
#[test]
fn no_owner_starts_before_the_first_read() {
    let rig = Rig::new("caller-first-read");
    let mut slot = rig.owner_slot();
    let refused = slot.relay(&read_workflows()).unwrap_err();
    assert_eq!(refused.code(), ErrorCode::StoreUnavailable, "{refused:?}");
    assert_eq!(slot.pid(), None);
    assert!(!rig.dir().join("derived").exists(), "derived/ was made");
    assert!(!rig.master_key().exists());
    assert_eq!(rig.lines.text(), "");
    assert!(cell().asked(), "the refused start asked for no read");

    write_read(&capture(), Position { seg: 1, idx: 0 });
    slot.relay(&read_workflows()).unwrap();
    assert!(slot.pid().is_some());
    assert!(rig.dir().join("derived").join("owner.pid").is_file());
}

/// Rule 7: the read channel is served by the fixture transport of halvo-seam over a store with no record, and the
/// cell holds that store's id and watermark. The first yes names no committed position, so the worker reads from
/// the zero position, takes no record and commits the watermark; the owner's report frame names that position and
/// the worker exits 0. The source then names the new position, and the pass that sees the worker's end starts the
/// next worker from it.
#[test]
fn a_yes_carries_the_new_position() {
    let no_file: [PathBuf; 0] = [];
    let empty = FixtureTransport::open(&no_file, 1, None).unwrap();
    let (store, mark) = (empty.store().clone(), empty.watermark());
    let server: Server = Arc::new(|input, output| {
        let no_file: [PathBuf; 0] = [];
        let mut transport = FixtureTransport::open(&no_file, 1, None).unwrap();
        let _ = transport.serve(input, output);
    });
    let rig = Rig::new("caller-yes");
    let mut slot = rig.worker_slot(Some(server));
    write_read(&store, mark);
    let now = Instant::now();
    let reading = rig.reading(now);
    let from_zero = Origin {
        capture: store.clone(),
        committed: None,
    };
    assert_eq!(reading.origin, from_zero);
    assert_eq!(reading.watermark, mark);
    assert_eq!(
        slot.pass(now, yes(&reading), &reading.origin),
        Answer::Started,
        "{}",
        rig.slot_lines.text()
    );
    let first = slot.pid().expect("the slot names no worker");

    let committed = Origin {
        capture: store.clone(),
        committed: Some(Committed {
            store,
            position: mark,
        }),
    };
    let until = Instant::now() + WAIT;
    while rig.reading(now).origin != committed {
        assert!(
            Instant::now() < until,
            "no report frame named the new position: {}",
            rig.slot_lines.text()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let answer = loop {
        let reading = rig.reading(now);
        assert_eq!(reading.origin, committed);
        let answer = slot.pass(now, yes(&reading), &reading.origin);
        if answer != Answer::Lives {
            break answer;
        }
        assert!(Instant::now() < until, "the slot did not see the end");
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(answer, Answer::Started, "{}", rig.slot_lines.text());
    let second = slot.pid().expect("the slot names no worker");
    assert_ne!(first, second);
    let text = rig.slot_lines.text();
    let first_end = text.lines().next().unwrap_or_default();
    assert!(
        first_end.starts_with("halvo-capture: analysis end=committed code=0 peak_bytes="),
        "{text}"
    );
}
