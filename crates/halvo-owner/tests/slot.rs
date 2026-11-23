//! The tests of the daemon's owner slot (halvo-capture, daemon::r2::owner_slot): each makes a slot over a new data
//! directory, with the directory of the built owner as the place of the program, and reads what the slot did.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use halvo_capture::custody::StoreKey;
use halvo_capture::daemon::owner_pid::Owner;
use halvo_capture::daemon::r2::owner_slot::{
    Caller, Distrust, Holder, OwnerSlot, Place, ProcessTable,
};
use halvo_control::proto::ErrorCode;
use halvo_schema_core::category::SeedCategory;
use halvo_schema_core::ids::{Position, RequestId, StoreId, SummaryId, WorkflowId};
use halvo_schema_core::record::{
    Episode, FloorLayer, Instant, ObservedStep, Ordinal, Part, Tagged, Text,
};
use halvo_schema_core::request::{
    decode_reply, AnalysisRole, AnalysisWrite, ErrorName, InterfaceRead, InterfaceRole, ReplyResult,
};
use halvo_schema_core::site::AppId;
use halvo_seam::frame::{read_frame, write_frame, CLIENT_FRAME_MAX, SERVER_FRAME_MAX};

const KEY: [u8; 32] = [7; 32];
const OTHER_KEY: [u8; 32] = [8; 32];

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
        if let Err(e) = fs::remove_dir_all(&self.0) {
            eprintln!(
                "the scratch directory {} was not removed: {e}",
                self.0.display()
            );
        }
    }
}

/// What the test's key source read when the slot asked it for the key: the text of the pid file, the file's mode and
/// the mode of its folder.
#[derive(Debug, Clone)]
struct Seen {
    text: String,
    file_mode: u32,
    folder_mode: u32,
}

/// The test's daemon: a key source that reads the pid file when it is asked, the flag of the delete, and the ordinals
/// of shredded segments that it names for a start.
struct Daemon {
    key: [u8; 32],
    pid_file: PathBuf,
    deleting: AtomicBool,
    seen: Mutex<Vec<Seen>>,
    gone: Mutex<Vec<u64>>,
}

impl Daemon {
    fn new(scratch: &Scratch, key: [u8; 32]) -> Arc<Daemon> {
        Arc::new(Daemon {
            key,
            pid_file: scratch.pid_file(),
            deleting: AtomicBool::new(false),
            seen: Mutex::new(Vec::new()),
            gone: Mutex::new(Vec::new()),
        })
    }

    fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
}

#[cfg(unix)]
fn mode_of(path: &Path) -> u32 {
    use std::os::unix::fs::MetadataExt;
    fs::symlink_metadata(path).map_or(0, |meta| meta.mode() & 0o7777)
}

/// The modes that the pid file and its folder must have; a system without modes reads none.
#[cfg(unix)]
const MODES: (u32, u32) = (0o600, 0o700);
#[cfg(not(unix))]
const MODES: (u32, u32) = (0, 0);

#[cfg(not(unix))]
fn mode_of(_: &Path) -> u32 {
    0
}

impl Caller for Daemon {
    fn key(&self) -> Option<StoreKey> {
        let text = fs::read_to_string(&self.pid_file).unwrap_or_default();
        let folder = self.pid_file.parent().unwrap();
        self.seen.lock().unwrap().push(Seen {
            text,
            file_mode: mode_of(&self.pid_file),
            folder_mode: mode_of(folder),
        });
        Some(StoreKey::from_bytes(self.key))
    }

    fn first_live(&self) -> u64 {
        1
    }

    fn deleting(&self) -> bool {
        self.deleting.load(Ordering::SeqCst)
    }

    fn report(&self, _: &[u8]) {}

    fn gone(&self) -> Vec<u64> {
        self.gone.lock().unwrap().clone()
    }
}

/// The place of a slot over `scratch` that starts the built owner.
fn place(scratch: &Scratch, idle_ms: Option<u32>) -> Place {
    let program = Path::new(env!("CARGO_BIN_EXE_halvo-owner"));
    Place {
        program_dir: program.parent().unwrap().to_path_buf(),
        data_dir: scratch.0.clone(),
        idle_ms,
    }
}

/// A slot over `scratch` that starts the built owner.
fn slot(scratch: &Scratch, daemon: &Arc<Daemon>, idle_ms: Option<u32>) -> OwnerSlot {
    OwnerSlot::new(
        place(scratch, idle_ms),
        Arc::clone(daemon) as Arc<dyn Caller>,
    )
}

/// The pid that a stand-in table answers for: no process of this Mac's table is read for it.
const STAND_IN_PID: u32 = 4_000_002;

/// A process table of the test that reads no start time and answers one holder for every pid; it ends no orphan.
struct StandIn(Holder);

impl ProcessTable for StandIn {
    fn start_time(&self, _: u32) -> Option<u64> {
        None
    }

    fn holder(&self, _: u32) -> Holder {
        self.0
    }

    fn end(&self, owner: Owner) -> bool {
        panic!("the stand-in table was asked to end {owner:?}")
    }
}

/// A slot over `scratch` whose take-over reads the stand-in table that answers `holder`.
fn slot_over_stand_in(scratch: &Scratch, daemon: &Arc<Daemon>, holder: Holder) -> OwnerSlot {
    OwnerSlot::with_table(
        place(scratch, None),
        Arc::clone(daemon) as Arc<dyn Caller>,
        &StandIn(holder),
    )
}

/// The pid and the start time of a line `pid=<n> started=<n>` with its end of line.
fn line_of(text: &str) -> (u32, u64) {
    let line = text.strip_suffix('\n').unwrap();
    let (pid, started) = line.split_once(' ').unwrap();
    (
        pid.strip_prefix("pid=").unwrap().parse().unwrap(),
        started.strip_prefix("started=").unwrap().parse().unwrap(),
    )
}

/// That no process has the pid any more.
#[cfg(unix)]
fn assert_gone(pid: u32) {
    let pid = libc::pid_t::try_from(pid).unwrap();
    // SAFETY: kill with the signal 0 sends nothing.
    let found = unsafe { libc::kill(pid, 0) };
    let error = std::io::Error::last_os_error().raw_os_error();
    assert!(
        found == -1 && error == Some(libc::ESRCH),
        "the process {pid} is still there"
    );
}

#[cfg(not(unix))]
fn assert_gone(_: u32) {}

fn request_id() -> RequestId {
    static NEXT: AtomicU8 = AtomicU8::new(1);
    RequestId::from_random([NEXT.fetch_add(1, Ordering::Relaxed); 16])
}

/// A `runner` request, a role that the starter's channel does not admit.
fn runner_request() -> Vec<u8> {
    let id = serde_json::to_string(&request_id()).unwrap();
    format!(r#"{{"v":1,"id":{id},"request":"runner","body":{{"probe":1}}}}"#).into_bytes()
}

fn read_workflows() -> Vec<u8> {
    InterfaceRead::workflows(&InterfaceRole::claim(), request_id(), None)
        .to_bytes()
        .unwrap()
}

/// Ruling 1: no owner runs before the first call that needs it; that call starts it and gets its reply.
#[test]
fn the_first_call_starts_the_owner() {
    let scratch = Scratch::new("slot-first-call");
    let daemon = Daemon::new(&scratch, KEY);
    let mut slot = slot(&scratch, &daemon, None);
    assert_eq!(slot.pid(), None, "an owner runs before the first call");
    assert!(!scratch.pid_file().exists());
    let reply = decode_reply(&slot.relay(&read_workflows()).unwrap()).unwrap();
    assert!(reply.outcome.is_ok(), "{:?}", reply.outcome);
    assert!(slot.pid().is_some());
}

/// Ruling 2: when the slot asks for the key, the pid file stands with one line of the child's pid and start time,
/// of mode 0600 in a folder of mode 0700.
#[test]
fn the_pid_file_comes_before_the_key() {
    let scratch = Scratch::new("slot-pid-file");
    let daemon = Daemon::new(&scratch, KEY);
    let mut slot = slot(&scratch, &daemon, None);
    slot.relay(&read_workflows()).unwrap();
    let pid = slot.pid().unwrap();
    let seen = daemon.seen();
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert_eq!(seen[0].text.lines().count(), 1, "{seen:?}");
    let (named, started) = line_of(&seen[0].text);
    assert_eq!(named, pid);
    assert!(started > 0);
    #[cfg(target_os = "macos")]
    assert_eq!(
        Some(started),
        halvo_capture::daemon::owner_pid::start_time_of(pid)
    );
    assert_eq!((seen[0].file_mode, seen[0].folder_mode), MODES, "{seen:?}");
}

/// Ruling 3: an owner that the key does not open is reaped, with its pid file, and the call answers
/// store_unavailable.
#[test]
fn a_wrong_key_is_store_unavailable() {
    let scratch = Scratch::new("slot-wrong-key");
    let first = Daemon::new(&scratch, KEY);
    let mut made = slot(&scratch, &first, None);
    made.relay(&read_workflows()).unwrap();
    drop(made);
    let second = Daemon::new(&scratch, OTHER_KEY);
    let mut slot = slot(&scratch, &second, None);
    let refused = slot.relay(&read_workflows()).unwrap_err();
    assert_eq!(refused.code(), ErrorCode::StoreUnavailable, "{refused:?}");
    assert_eq!(slot.pid(), None);
    let seen = second.seen();
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert_gone(line_of(&seen[0].text).0);
    assert!(!scratch.pid_file().exists());
}

/// Ruling 4: an owner that ended at its idle time is reaped before the next call starts the next owner.
#[test]
fn the_next_owner_starts_after_the_reap() {
    let scratch = Scratch::new("slot-next-owner");
    let daemon = Daemon::new(&scratch, KEY);
    let mut slot = slot(&scratch, &daemon, Some(200));
    slot.relay(&read_workflows()).unwrap();
    let first = slot.pid().unwrap();
    std::thread::sleep(Duration::from_secs(1));
    slot.relay(&read_workflows()).unwrap();
    let second = slot.pid().unwrap();
    assert_ne!(first, second);
    assert_gone(first);
    let text = fs::read_to_string(scratch.pid_file()).unwrap();
    assert_eq!(line_of(&text).0, second);
}

/// Writes a pid file of `text` with `mode`, in a folder derived of mode 0700, as an earlier daemon would have left it.
#[cfg(unix)]
fn leave_pid_file(scratch: &Scratch, text: &str, mode: u32) {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(scratch.0.join("derived"))
        .unwrap();
    fs::write(scratch.pid_file(), text).unwrap();
    fs::set_permissions(scratch.pid_file(), fs::Permissions::from_mode(mode)).unwrap();
}

/// Writes a pid file of `text` in a folder derived, as an earlier daemon would have left it; a system without modes
/// takes none.
#[cfg(not(unix))]
fn leave_pid_file(scratch: &Scratch, text: &str, _: u32) {
    fs::create_dir_all(scratch.0.join("derived")).unwrap();
    fs::write(scratch.pid_file(), text).unwrap();
}

/// A child of the test that lives until it is ended, with its pid and start time; a thread of the test reaps it and
/// hands its exit status over.
#[cfg(target_os = "macos")]
fn sleeper() -> (
    u32,
    u64,
    std::sync::mpsc::Receiver<std::process::ExitStatus>,
) {
    let mut child = std::process::Command::new("/bin/sleep")
        .arg("60")
        .spawn()
        .unwrap();
    let pid = child.id();
    let started = halvo_capture::daemon::owner_pid::start_time_of(pid).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait().unwrap());
    });
    (pid, started, rx)
}

/// Ruling 5 (c) and (d): an orphan of the pid and the start time of the file is ended; one whose start time is off
/// by one lives; the file is removed both times.
#[cfg(target_os = "macos")]
#[test]
fn an_orphan_is_ended_by_both() {
    use halvo_capture::daemon::owner_pid::start_time_of;
    use std::os::unix::process::ExitStatusExt;
    let scratch = Scratch::new("slot-orphan");
    let daemon = Daemon::new(&scratch, KEY);
    for off in [0, 1] {
        let (pid, started, status) = sleeper();
        leave_pid_file(
            &scratch,
            &format!("pid={pid} started={}\n", started + off),
            0o600,
        );
        let slot = slot(&scratch, &daemon, None);
        assert_eq!(slot.distrust(), None);
        assert!(!scratch.pid_file().exists(), "off by {off}: the file stays");
        if off == 0 {
            let ended = status.recv_timeout(Duration::from_secs(30)).unwrap();
            assert_eq!(ended.signal(), Some(libc::SIGTERM));
        } else {
            assert_eq!(start_time_of(pid), Some(started), "the orphan was ended");
            // SAFETY: kill takes a pid and a signal number; the pid is the test's own child, not yet reaped.
            unsafe { libc::kill(libc::pid_t::try_from(pid).unwrap(), libc::SIGKILL) };
            status.recv_timeout(Duration::from_secs(30)).unwrap();
        }
        assert_eq!(slot.pid(), None);
    }
    assert!(daemon.seen().is_empty());
}

/// Ruling 5 (a): a true line in a file of mode 0644 is trusted for nothing: the process lives, the file stays, and
/// no owner starts.
#[cfg(target_os = "macos")]
#[test]
fn a_pid_file_of_a_wider_mode_is_trusted_for_nothing() {
    use halvo_capture::daemon::owner_pid::start_time_of;
    let scratch = Scratch::new("slot-wider-mode");
    let daemon = Daemon::new(&scratch, KEY);
    let (pid, started, status) = sleeper();
    let text = format!("pid={pid} started={started}\n");
    leave_pid_file(&scratch, &text, 0o644);
    let mut slot = slot(&scratch, &daemon, None);
    assert_eq!(slot.distrust(), Some(Distrust::NotPrivate));
    assert_eq!(start_time_of(pid), Some(started), "the process was ended");
    assert_eq!(fs::read_to_string(scratch.pid_file()).unwrap(), text);
    let refused = slot.relay(&read_workflows()).unwrap_err();
    assert_eq!(refused.code(), ErrorCode::StoreUnavailable, "{refused:?}");
    assert_eq!(slot.pid(), None);
    assert!(daemon.seen().is_empty(), "a key was asked for");
    assert_eq!(fs::read_to_string(scratch.pid_file()).unwrap(), text);
    // SAFETY: kill takes a pid and a signal number; the pid is the test's own child, not yet reaped.
    unsafe { libc::kill(libc::pid_t::try_from(pid).unwrap(), libc::SIGKILL) };
    status.recv_timeout(Duration::from_secs(30)).unwrap();
}

/// Ruling 5 (b), through a stand-in table: a line that names a process of this user whose start time cannot be read
/// is no match: nothing is signalled, the file stays, and no owner starts.
#[test]
fn a_start_time_that_cannot_be_read_is_no_match() {
    let scratch = Scratch::new("slot-unread-start");
    let daemon = Daemon::new(&scratch, KEY);
    let text = format!("pid={STAND_IN_PID} started=1\n");
    leave_pid_file(&scratch, &text, 0o600);
    let mut slot = slot_over_stand_in(&scratch, &daemon, Holder::ThisUser);
    assert_eq!(
        slot.distrust(),
        Some(Distrust::StartTimeUnread { pid: STAND_IN_PID })
    );
    assert_eq!(fs::read_to_string(scratch.pid_file()).unwrap(), text);
    let refused = slot.relay(&read_workflows()).unwrap_err();
    assert_eq!(refused.code(), ErrorCode::StoreUnavailable, "{refused:?}");
    assert_eq!(slot.pid(), None);
    assert!(daemon.seen().is_empty(), "a key was asked for");
}

/// The coordinator's turn of ruling 5 (B2), through a stand-in table: a pid that a process of another user holds
/// names no owner of this user: the file is removed, nothing is signalled, and the first call starts the owner.
#[test]
fn a_stale_pid_of_another_users_process_is_removed_and_the_owner_starts() {
    let scratch = Scratch::new("slot-stale-pid");
    let daemon = Daemon::new(&scratch, KEY);
    leave_pid_file(&scratch, &format!("pid={STAND_IN_PID} started=1\n"), 0o600);
    let mut slot = slot_over_stand_in(&scratch, &daemon, Holder::OtherUser);
    assert_eq!(slot.distrust(), None);
    assert!(!scratch.pid_file().exists(), "the stale file stays");
    let reply = decode_reply(&slot.relay(&read_workflows()).unwrap()).unwrap();
    assert!(reply.outcome.is_ok(), "{:?}", reply.outcome);
    let text = fs::read_to_string(scratch.pid_file()).unwrap();
    assert_eq!(Some(line_of(&text).0), slot.pid());
}

/// The coordinator's turn of ruling 5 (B1): an empty pid file of mode 0600, as a daemon's death between the file's
/// making and its write leaves it, names no process: it is removed, and after the first call the file holds the new
/// owner's line and the call has its reply.
#[test]
fn a_pid_file_with_no_whole_line_is_removed_and_the_owner_starts() {
    let scratch = Scratch::new("slot-no-whole-line");
    let daemon = Daemon::new(&scratch, KEY);
    leave_pid_file(&scratch, "", 0o600);
    let mut slot = slot(&scratch, &daemon, None);
    assert_eq!(slot.distrust(), None);
    assert!(!scratch.pid_file().exists(), "the empty file stays");
    let reply = decode_reply(&slot.relay(&read_workflows()).unwrap()).unwrap();
    assert!(reply.outcome.is_ok(), "{:?}", reply.outcome);
    let text = fs::read_to_string(scratch.pid_file()).unwrap();
    assert_eq!(text.lines().count(), 1, "{text:?}");
    assert_eq!(Some(line_of(&text).0), slot.pid());
}

/// Ruling 6: a request above 4,096 bytes is invalid_argument and starts nothing, the delete of everything answers
/// busy and starts nothing, and the owner answers role to a role that the starter's channel does not admit.
#[test]
fn the_exchange_refuses() {
    let scratch = Scratch::new("slot-refuses");
    let daemon = Daemon::new(&scratch, KEY);
    let mut slot = slot(&scratch, &daemon, None);
    let long = slot.relay(&vec![b' '; 4_097]).unwrap_err();
    assert_eq!(long.code(), ErrorCode::InvalidArgument, "{long:?}");
    assert_eq!(slot.pid(), None);
    daemon.deleting.store(true, Ordering::SeqCst);
    let busy = slot.relay(&read_workflows()).unwrap_err();
    assert_eq!(busy.code(), ErrorCode::Busy, "{busy:?}");
    assert_eq!(slot.pid(), None);
    assert!(daemon.seen().is_empty());
    daemon.deleting.store(false, Ordering::SeqCst);
    let reply = decode_reply(&slot.relay(&runner_request()).unwrap()).unwrap();
    assert_eq!(reply.outcome.unwrap_err(), ErrorName::Role);
}

/// Ruling 7: the stop leaves no owner and no pid file.
#[test]
fn stop_leaves_no_owner_and_no_pid_file() {
    let scratch = Scratch::new("slot-stop");
    let daemon = Daemon::new(&scratch, KEY);
    let mut slot = slot(&scratch, &daemon, None);
    slot.relay(&read_workflows()).unwrap();
    let pid = slot.pid().unwrap();
    slot.stop();
    assert_eq!(slot.pid(), None);
    assert!(!scratch.pid_file().exists());
    assert_gone(pid);
}

/// Sentence 8: after the delete walk has run with the registry's rows over a data directory in which the slot started
/// an owner, that owner is no live process of its start time and derived holds no entry.
#[cfg(target_os = "macos")]
#[test]
fn a_delete_of_everything_leaves_no_entry_under_derived() {
    use halvo_capture::daemon::delete_all::offline;
    use halvo_capture::daemon::owner_pid::{owner_of, start_time_of};
    let scratch = Scratch::new("slot-delete-all");
    let daemon = Daemon::new(&scratch, KEY);
    let mut slot = slot(&scratch, &daemon, None);
    slot.relay(&read_workflows()).unwrap();
    let owner = owner_of(&scratch.pid_file()).unwrap().unwrap();
    assert_eq!(Some(owner.pid), slot.pid());
    let derived = scratch.0.join("derived");
    fs::write(derived.join("master.key"), [9u8; 32]).unwrap();
    let data_dir = halvo_control::paths::DataDir::from_flag(scratch.0.clone()).unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let status = offline(&data_dir, &mut out, &mut err);
    let printed = format!(
        "status {status}, out: {}, err: {}",
        String::from_utf8_lossy(&out),
        String::from_utf8_lossy(&err)
    );
    assert_ne!(
        start_time_of(owner.pid),
        Some(owner.started),
        "the owner lives: {printed}"
    );
    let left: Vec<PathBuf> = fs::read_dir(&derived).map_or_else(
        |_| Vec::new(),
        |entries| entries.map(|entry| entry.unwrap().path()).collect(),
    );
    assert!(left.is_empty(), "derived holds {left:?}: {printed}");
    assert_eq!(status, 0, "{printed}");
    drop(slot);
}

/// An episode of the segment `n` alone, with the summary id of `n`, of one key for every `n`, two days before today.
fn episode(n: u8) -> Episode {
    const DAY_NS: i64 = 86_400_000_000_000;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    let day = u32::try_from(i64::try_from(now.as_nanos()).unwrap().div_euclid(DAY_NS)).unwrap() - 2;
    let start = i64::from(day) * DAY_NS + 64_800_000_000_000 + i64::from(n);
    Episode {
        id: SummaryId::from_random([n; 16]),
        category: SeedCategory::UtilityBillPayment,
        site: None,
        app: AppId::parse("com.example.browser").unwrap(),
        day,
        part: Part::Evening,
        minutes: 5,
        keys: None,
        pointer: None,
        segments: vec![Ordinal::new(u64::from(n)).unwrap()],
        expires: Instant(9_000_000_000_000_000_000),
        start: Instant(start),
        end: Instant(start + 1_000),
        steps: vec![ObservedStep {
            app: None,
            title: Some(Tagged {
                value: Text::parse("Pay bill").unwrap(),
                layer: FloorLayer::Floor,
            }),
            input: None,
            site: None,
            text: None,
        }],
    }
}

/// The ids of the workflows that the owner's reply to a read of the workflows names.
fn workflows(reply: &[u8]) -> Vec<WorkflowId> {
    match decode_reply(reply).unwrap().outcome {
        Ok(ReplyResult::Workflows(page)) => page
            .workflows
            .iter()
            .map(|item| item.workflow.id().clone())
            .collect(),
        other => panic!("the read of the workflows answers {other:?}"),
    }
}

/// The ordinals that each evidence row of the owner's reply to a read of the evidence names.
fn evidence_segments(reply: &[u8]) -> Vec<Vec<u64>> {
    match decode_reply(reply).unwrap().outcome {
        Ok(ReplyResult::Evidence(page)) => page
            .evidence
            .iter()
            .map(|row| row.segments.iter().map(|o| o.get()).collect())
            .collect(),
        other => panic!("the read of the evidence answers {other:?}"),
    }
}

fn read_evidence(workflow: &WorkflowId) -> Vec<u8> {
    InterfaceRead::evidence(
        &InterfaceRole::claim(),
        request_id(),
        workflow.clone(),
        None,
    )
    .to_bytes()
    .unwrap()
}

/// The start with none alive (the deletion's promise): an owner started for a worker commits the evidence rows of the
/// segments 1, 2 and 3; the daemon then names the segment 2, above the lowest live one 1, as shredded, and the next
/// start hands it to its owner before the first request, whose answer names the rows of the segments 1 and 3 alone.
#[test]
fn a_start_hands_its_owner_the_shredded_ordinals_before_the_first_request() {
    let scratch = Scratch::new("slot-gone");
    let daemon = Daemon::new(&scratch, KEY);
    let mut slot = slot(&scratch, &daemon, None);
    let (owner_reads, mut worker_writes) = std::io::pipe().unwrap();
    let (mut worker_reads, owner_writes) = std::io::pipe().unwrap();
    slot.start_with_worker((owner_reads, owner_writes)).unwrap();
    let store = StoreId::parse(&"1".repeat(32)).unwrap();
    let mut write = AnalysisWrite::new(&AnalysisRole::claim(), request_id(), store);
    for n in 1..=3 {
        write.push(episode(n)).unwrap();
    }
    write.commit(Position { seg: 4, idx: 0 });
    write_frame(
        &mut worker_writes,
        &write.to_bytes().unwrap(),
        CLIENT_FRAME_MAX,
    )
    .unwrap();
    let reply = read_frame(&mut worker_reads, SERVER_FRAME_MAX)
        .unwrap()
        .unwrap();
    assert!(matches!(
        decode_reply(&reply).unwrap().outcome,
        Ok(ReplyResult::Null)
    ));
    let ids = workflows(&slot.relay(&read_workflows()).unwrap());
    assert_eq!(
        ids.len(),
        1,
        "the three summaries of one key make one workflow"
    );
    let read = read_evidence(&ids[0]);
    assert_eq!(
        evidence_segments(&slot.relay(&read).unwrap()),
        [[1], [2], [3]]
    );
    slot.stop();
    drop((worker_writes, worker_reads));

    *daemon.gone.lock().unwrap() = vec![2];
    let read = read_evidence(&ids[0]);
    assert_eq!(evidence_segments(&slot.relay(&read).unwrap()), [[1], [3]]);
}
