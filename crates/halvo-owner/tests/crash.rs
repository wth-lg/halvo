//! The crash tests of the derived store and its key: each ends the owner, or the process that runs the delete of
//! everything, at a named step, and reads what stands on the disk and what the next start does with it. Every process
//! that a test ends is a child that the test started: an owner by `Child::kill` or by the delete walk's signal to the
//! pid in the pid file that the test wrote, and a run of this binary again by `Child::kill`.

#![cfg(any(target_os = "macos", windows))]

use std::ffi::OsStr;
use std::fs;
use std::io::{BufRead, BufReader, PipeReader, PipeWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant as Mono, SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::{STANDARD_NO_PAD, URL_SAFE_NO_PAD};
use base64::Engine as _;
use halvo_capture::custody::StoreKey;
use halvo_capture::daemon::custody_policy::Custody;
#[cfg(all(target_os = "macos", debug_assertions))]
use halvo_capture::daemon::delete_all::{Hands, SystemHands};
use halvo_capture::daemon::port::{DataDirStore, Opened, StoreHost};
use halvo_capture::daemon::r2::owner_slot::{Caller, OwnerSlot, Place};
use halvo_derived::{Clock, DbKey, Host, IdSource, Store};
use halvo_schema_core::category::SeedCategory;
use halvo_schema_core::ids::{Position, RequestId, StoreId, SummaryId, WorkflowId};
use halvo_schema_core::kinds::{self, KindEntry};
use halvo_schema_core::record::{
    Episode, FloorLayer, Instant, ObservedStep, Ordinal, Part, Proposal, RecurrenceStep,
    SummaryRead, Tagged, Text,
};
use halvo_schema_core::request::{
    decode_reply, AnalysisRole, AnalysisWrite, InterfaceRead, InterfaceRole, Reply, ReplyResult,
};
use halvo_schema_core::site::AppId;
use halvo_seam::frame::{read_frame, write_frame, CLIENT_FRAME_MAX, SERVER_FRAME_MAX};
use serde_json::Value;

/// The constant key of the tests that make no capture store.
const KEY: [u8; 32] = [7; 32];
const DAY_NS: i64 = 86_400_000_000_000;
const FAR: i64 = 9_000_000_000_000_000_000;
/// The longest wait for a frame, an exit or a line that a child owes at once.
const WAIT: Duration = Duration::from_secs(30);
/// The longest wait for a run of this binary again.
const RERUN_WAIT: Duration = Duration::from_secs(120);

/// The switch of a run of this binary again for `a_repeated_batch_runs_no_held_back_step`: the data directory, and
/// the day of the batch's episodes.
const BATCH_SWITCH: &str = "HALVO_CRASH_BATCH_DIR";
const DAY_SWITCH: &str = "HALVO_CRASH_BATCH_DAY";
const BATCH_TEST: &str = "a_repeated_batch_runs_no_held_back_step";
const AT_THE_RECURRENCE_STEP: &str = "crash-batch: at the recurrence step";

/// The switch of a run of this binary again for `the_derived_key_stands_in_no_file_line_or_argument`: the data
/// directory.
const KEY_SWITCH: &str = "HALVO_CRASH_KEY_DIR";
const KEY_TEST: &str = "the_derived_key_stands_in_no_file_line_or_argument";
const SERVED: &str = "crash-key: served";

/// The program name of the owner, which a copy of this binary bears as the stand-in, and the file beside that copy
/// into which the stand-in writes what it was started with.
const OWNER_PROGRAM: &str = "halvo-owner";
const SEEN: &str = "stand-in-seen.bin";

/// One test at a time: on Windows a child that one test starts inherits every handle that another test has made
/// inheritable at that moment, and these tests wait for the ends of their owners' channels.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

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
        if let Err(e) = fs::remove_dir_all(&self.0) {
            eprintln!(
                "the scratch directory {} was not removed: {e}",
                self.0.display()
            );
        }
    }
}

/// A running owner with the test's ends of its channels. Dropped, it kills an owner that still runs and reaps it.
struct Owner {
    child: Option<Child>,
    /// Standard input, held open where the test handed no key.
    _stdin: Option<ChildStdin>,
    requests: Option<PipeWriter>,
    frames: Receiver<Vec<u8>>,
    worker: Option<PipeWriter>,
    worker_frames: Option<Receiver<Vec<u8>>>,
}

/// Hands each frame that the owner writes on `from` to the receiver, until the end of the stream.
fn frames(mut from: PipeReader) -> Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        while let Ok(Some(frame)) = read_frame(&mut from, SERVER_FRAME_MAX) {
            if tx.send(frame).is_err() {
                return;
            }
        }
    });
    rx
}

/// Starts the built owner on `dir` with the lowest live ordinal `first_live`, the starter's channel and, where
/// `worker` holds, the worker's pipe pair. With a key the key goes on standard input, which is then closed; with none
/// standard input stays open and holds no byte.
fn start(dir: &Path, key: Option<[u8; 32]>, first_live: u64, worker: bool) -> Owner {
    let (owner_reads, requests) = std::io::pipe().unwrap();
    let (replies, owner_writes) = std::io::pipe().unwrap();
    let mut ends = vec![(owner_reads, owner_writes)];
    let mut test_ends = None;
    if worker {
        let (owner_reads, writes) = std::io::pipe().unwrap();
        let (reads, owner_writes) = std::io::pipe().unwrap();
        ends.push((owner_reads, owner_writes));
        test_ends = Some((writes, reads));
    }
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_halvo-owner"));
    cmd.arg(format!("--data-dir={}", dir.display()))
        .arg(format!("--first-live={first_live}"))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    let mut child = launch(&mut cmd, ends);
    let mut stdin = child.stdin.take();
    if let Some(key) = key {
        let mut input = stdin.take().unwrap();
        input.write_all(&key).unwrap();
    }
    let (worker, worker_frames) = match test_ends {
        Some((writes, reads)) => (Some(writes), Some(frames(reads))),
        None => (None, None),
    };
    Owner {
        child: Some(child),
        _stdin: stdin,
        requests: Some(requests),
        frames: frames(replies),
        worker,
        worker_frames,
    }
}

/// Starts `cmd` with the owner's ends of its channels at the descriptors 3 and 4, and 5 and 6 for the worker's.
#[cfg(unix)]
fn launch(cmd: &mut Command, ends: Vec<(PipeReader, PipeWriter)>) -> Child {
    use std::os::fd::AsRawFd;
    use std::os::unix::process::CommandExt;
    let mut places = Vec::new();
    for (n, (reads, writes)) in ends.iter().enumerate() {
        let r = 3 + 2 * i32::try_from(n).unwrap();
        let name = if n == 0 { "channel" } else { "worker" };
        cmd.arg(format!("--{name}={r},{}", r + 1));
        places.push((reads.as_raw_fd(), r));
        places.push((writes.as_raw_fd(), r + 1));
    }
    // SAFETY: the closure runs in the child between fork and exec and calls fcntl and dup2 alone, which are
    // async-signal-safe; it first copies each end above 9, so that no dup2 writes over an end before it is copied.
    unsafe {
        cmd.pre_exec(move || {
            let mut high = [0; 4];
            for (at, (from, _)) in places.iter().enumerate() {
                high[at] = libc::fcntl(*from, libc::F_DUPFD_CLOEXEC, 10);
                if high[at] < 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            for (at, (_, to)) in places.iter().enumerate() {
                if libc::dup2(high[at], *to) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
    let child = cmd.spawn().unwrap();
    drop(ends);
    child
}

/// Starts `cmd` with the owner's ends of its channels made inheritable and named by their handle values. One lock
/// keeps every start of this binary apart, so that no other child inherits these ends.
#[cfg(windows)]
fn launch(cmd: &mut Command, ends: Vec<(PipeReader, PipeWriter)>) -> Child {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::{SetHandleInformation, HANDLE_FLAG_INHERIT};
    static STARTS: Mutex<()> = Mutex::new(());
    let held = STARTS.lock().unwrap_or_else(|e| e.into_inner());
    for (n, (reads, writes)) in ends.iter().enumerate() {
        let (r, w) = (reads.as_raw_handle(), writes.as_raw_handle());
        for handle in [r, w] {
            // SAFETY: the handle is an open end that this process owns; the call sets its inherit flag alone.
            let set =
                unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) };
            assert_ne!(set, 0, "{}", std::io::Error::last_os_error());
        }
        let name = if n == 0 { "channel" } else { "worker" };
        cmd.arg(format!("--{name}={},{}", r as usize, w as usize));
    }
    let child = cmd.spawn();
    drop(ends);
    drop(held);
    child.unwrap()
}

fn is_report(frame: &[u8]) -> bool {
    let value: Value = serde_json::from_slice(frame).unwrap();
    value
        .as_object()
        .is_some_and(|object| object.len() == 1 && object.contains_key("report"))
}

impl Owner {
    /// Sends one request on the starter's channel.
    fn send(&mut self, request: &[u8]) {
        let requests = self.requests.as_mut().unwrap();
        write_frame(requests, request, CLIENT_FRAME_MAX).unwrap();
    }

    /// Sends one request on the worker's channel.
    fn write(&mut self, request: &[u8]) {
        let worker = self.worker.as_mut().unwrap();
        write_frame(worker, request, CLIENT_FRAME_MAX).unwrap();
    }

    /// The next frame on the starter's channel that is no report.
    fn reply(&mut self) -> Reply {
        loop {
            let frame = self
                .frames
                .recv_timeout(WAIT)
                .expect("the owner sends no reply");
            if !is_report(&frame) {
                return decode_reply(&frame).unwrap();
            }
        }
    }

    /// The next frame on the worker's channel.
    fn worker_reply(&mut self) -> Reply {
        let frames = self.worker_frames.as_ref().unwrap();
        let frame = frames
            .recv_timeout(WAIT)
            .expect("the owner sends no reply to the worker");
        decode_reply(&frame).unwrap()
    }

    /// The pid of the owner, which the test has not handed to a reaper.
    #[cfg(all(target_os = "macos", debug_assertions))]
    fn pid(&self) -> u32 {
        self.child.as_ref().unwrap().id()
    }

    /// Ends the owner by `Child::kill` and reaps it.
    fn kill(&mut self) {
        let mut child = self.child.take().unwrap();
        let _ = child.kill();
        child.wait().unwrap();
    }

    /// Hands the owner to a thread of the test, which reaps it and hands its exit status over.
    #[cfg(all(target_os = "macos", debug_assertions))]
    fn reaper(&mut self) -> Receiver<std::process::ExitStatus> {
        let mut child = self.child.take().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(child.wait().unwrap());
        });
        rx
    }

    /// Ends both channels and returns the owner's exit code within `WAIT`, or none.
    fn close_and_wait(&mut self) -> Option<i32> {
        self.requests = None;
        self.worker = None;
        let child = self.child.as_mut().unwrap();
        let deadline = Mono::now() + WAIT;
        while Mono::now() < deadline {
            if let Some(status) = child.try_wait().unwrap() {
                return status.code();
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        None
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            if child.try_wait().ok().flatten().is_none() {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
    }
}

fn request_id() -> RequestId {
    static NEXT: AtomicU8 = AtomicU8::new(1);
    RequestId::from_random([NEXT.fetch_add(1, Ordering::Relaxed); 16])
}

fn read_workflows() -> Vec<u8> {
    InterfaceRead::workflows(&InterfaceRole::claim(), request_id(), None)
        .to_bytes()
        .unwrap()
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

fn store_id() -> StoreId {
    StoreId::parse(&"1".repeat(32)).unwrap()
}

/// An analysis write of `episodes` that carries the commit `commit`, or none.
fn analysis_write(episodes: Vec<Episode>, commit: Option<Position>) -> Vec<u8> {
    let mut write = AnalysisWrite::new(&AnalysisRole::claim(), request_id(), store_id());
    for episode in episodes {
        write.push(episode).unwrap();
    }
    if let Some(commit) = commit {
        write.commit(commit);
    }
    write.to_bytes().unwrap()
}

fn now_ns() -> i64 {
    let since = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    i64::try_from(since.as_nanos()).unwrap()
}

/// Two days before the UTC day of now, a day that every local day of the owner keeps inside the bound.
fn two_days_ago() -> u32 {
    u32::try_from(now_ns().div_euclid(DAY_NS)).unwrap() - 2
}

/// An episode of the segment `n`, with the summary id of `n`, of one key for every `n`.
fn episode(n: u8, day: u32) -> Episode {
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
        expires: Instant(FAR),
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

/// The episodes of the segments 1 to 3: three summaries of one key make one workflow.
fn three(day: u32) -> Vec<Episode> {
    vec![episode(1, day), episode(2, day), episode(3, day)]
}

/// The commit after the segment `seg - 1`.
fn at(seg: u64) -> Option<Position> {
    Some(Position { seg, idx: 0 })
}

fn assert_null(reply: Reply) {
    assert!(
        matches!(reply.outcome, Ok(ReplyResult::Null)),
        "{:?}",
        reply.outcome
    );
}

fn workflows(reply: Reply) -> Vec<WorkflowId> {
    match reply.outcome {
        Ok(ReplyResult::Workflows(page)) => page
            .workflows
            .iter()
            .map(|item| item.workflow.id().clone())
            .collect(),
        other => panic!("the read of the workflows answers {other:?}"),
    }
}

fn evidence_segments(reply: Reply) -> Vec<Vec<u64>> {
    match reply.outcome {
        Ok(ReplyResult::Evidence(page)) => page
            .evidence
            .iter()
            .map(|row| row.segments.iter().map(|o| o.get()).collect())
            .collect(),
        other => panic!("the read of the evidence answers {other:?}"),
    }
}

/// Makes a capture store in `dir` under the root file's custody and drops it, and returns the derived store's key as
/// the product makes it: `derived_store_key` wraps it under that store's root in derived/master.key.
fn capture_store_and_key(dir: &Path) -> [u8; 32] {
    let host = DataDirStore::at(dir);
    let Opened::ReadWrite { store, .. } = host.create(Custody::RootFile) else {
        panic!("no capture store was made in {}", dir.display());
    };
    drop(store);
    let key = host
        .derived_store_key()
        .unwrap_or_else(|fault| panic!("no key of the derived store: {fault:?}"));
    *key.expose()
}

/// The forms in which a key's bytes could stand in a file or a line: raw, hex in either case, and base64 of the
/// standard and of the URL-safe alphabet, without padding, which a padded text holds.
fn key_forms(key: &[u8; 32]) -> Vec<(&'static str, Vec<u8>)> {
    let hex: String = key.iter().map(|byte| format!("{byte:02x}")).collect();
    vec![
        ("raw", key.to_vec()),
        ("hex", hex.clone().into_bytes()),
        ("upper-case hex", hex.to_uppercase().into_bytes()),
        ("base64", STANDARD_NO_PAD.encode(key).into_bytes()),
        ("url-safe base64", URL_SAFE_NO_PAD.encode(key).into_bytes()),
    ]
}

/// The names of the forms of `key` that `bytes` holds.
fn forms_in(bytes: &[u8], key: &[u8; 32]) -> Vec<&'static str> {
    key_forms(key)
        .into_iter()
        .filter(|(_, form)| {
            bytes
                .windows(form.len())
                .any(|place| place == form.as_slice())
        })
        .map(|(name, _)| name)
        .collect()
}

/// Every entry under `dir`, folders among them, in order; none for a `dir` that does not stand. No link is followed.
fn entries_under(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries {
            let path = entry.unwrap().path();
            let is_dir = fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_dir());
            found.push(path.clone());
            if is_dir {
                found.extend(entries_under(&path));
            }
        }
    }
    found.sort();
    found
}

/// Every regular file under `dir`, in order.
fn files_under(dir: &Path) -> Vec<PathBuf> {
    entries_under(dir)
        .into_iter()
        .filter(|path| fs::symlink_metadata(path).is_ok_and(|meta| meta.is_file()))
        .collect()
}

/// Each file under `dir` that holds a form of `key`, with the forms that it holds.
fn files_holding(dir: &Path, key: &[u8; 32]) -> Vec<(PathBuf, Vec<&'static str>)> {
    files_under(dir)
        .into_iter()
        .filter_map(|path| {
            let bytes = fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let forms = forms_in(&bytes, key);
            (!forms.is_empty()).then_some((path, forms))
        })
        .collect()
}

/// Runs this test binary again for the test `name` alone, with `switches` in its environment, standard input held
/// open by the test, standard output on a pipe and standard error as `stderr`.
fn rerun(name: &str, switches: &[(&str, &OsStr)], stderr: Stdio) -> Child {
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.args(["--exact", name, "--nocapture", "--test-threads=1"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(stderr);
    for (switch, value) in switches {
        cmd.env(switch, value);
    }
    cmd.spawn().unwrap()
}

/// The lines of a child's standard output, read on a thread of the test.
fn lines_of(child: &mut Child) -> Receiver<String> {
    let out = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(out).lines() {
            let Ok(line) = line else { return };
            if tx.send(line).is_err() {
                return;
            }
        }
    });
    rx
}

/// Waits for a line that holds `marker`; libtest may print the test's name in front of it on the same line.
fn wait_for(lines: &Receiver<String>, marker: &str) {
    let deadline = Mono::now() + RERUN_WAIT;
    loop {
        let left = deadline.saturating_duration_since(Mono::now());
        match lines.recv_timeout(left) {
            Ok(line) if line.contains(marker) => return,
            Ok(_) => {}
            Err(e) => panic!("no line {marker:?} came: {e}"),
        }
    }
}

/// Prints `line` on standard output and waits for the test to kill this process. At the end of standard input,
/// which the test holds open, the process exits and does not go on.
fn print_and_wait(line: &str) -> ! {
    let mut out = std::io::stdout().lock();
    writeln!(out, "{line}").unwrap();
    out.flush().unwrap();
    drop(out);
    let _ = std::io::copy(&mut std::io::stdin(), &mut std::io::sink());
    std::process::exit(1)
}

/// Writes derived/owner.pid for the owner `pid` as the slot writes it, `pid=<n> started=<start time>`, of mode
/// 0600 in a folder of mode 0700, and returns the start time.
#[cfg(all(target_os = "macos", debug_assertions))]
fn write_pid_file(dir: &Path, pid: u32) -> u64 {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    let started = halvo_capture::daemon::owner_pid::start_time_of(pid).unwrap();
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir.join("derived"))
        .unwrap();
    let file = dir.join("derived").join("owner.pid");
    fs::write(&file, format!("pid={pid} started={started}\n")).unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
    started
}

/// Ruled point 1 (C3.10): the delete, run while the owner holds one committed batch and one write without its commit,
/// ends that owner by its pid and start time with SIGTERM, and leaves no entry under derived/, master.key among them,
/// and no delete-all.intent.
#[cfg(all(target_os = "macos", debug_assertions))]
#[test]
fn a_delete_during_a_batch_leaves_nothing() {
    use std::os::unix::process::ExitStatusExt;
    let _serial = serial();
    let scratch = Scratch::new("crash-mid-batch");
    let key = capture_store_and_key(&scratch.0);
    let day = two_days_ago();
    let mut owner = start(&scratch.0, Some(key), 1, true);
    owner.write(&analysis_write(three(day), at(4)));
    assert_null(owner.worker_reply());
    owner.send(&read_workflows());
    assert_eq!(workflows(owner.reply()).len(), 1, "the batch is committed");
    owner.write(&analysis_write(
        vec![episode(4, day), episode(5, day)],
        None,
    ));
    assert_null(owner.worker_reply());
    write_pid_file(&scratch.0, owner.pid());
    let ended = owner.reaper();

    let data_dir = halvo_control::paths::DataDir::from_flag(scratch.0.clone()).unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let status = halvo_capture::daemon::delete_all::offline(&data_dir, &mut out, &mut err);
    let printed = format!(
        "status {status}, out: {}, err: {}",
        String::from_utf8_lossy(&out),
        String::from_utf8_lossy(&err)
    );

    let exit = ended
        .recv_timeout(WAIT)
        .unwrap_or_else(|e| panic!("the owner did not end: {e}; {printed}"));
    assert_eq!(exit.signal(), Some(libc::SIGTERM), "{printed}");
    let derived = scratch.0.join("derived");
    assert_eq!(entries_under(&derived), Vec::<PathBuf>::new(), "{printed}");
    assert!(!derived.join("master.key").exists(), "{printed}");
    assert!(
        !scratch.0.join(halvo_control::paths::INTENT_FILE).exists(),
        "{printed}"
    );
    assert_eq!(status, 0, "{printed}");
}

/// On Windows, the delete of everything against the real owner and its key, as the app runs it: the real owner makes a
/// derived row and the derived key on a real store and key, the test writes derived/owner.pid as the owner slot writes
/// it, and then halvo-capture.exe, the program that the app runs, runs with `--delete-all --data-dir DIR`. It ends the
/// owner by its pid and start time and leaves no entry under derived/, no store, no root and no intent; the next start
/// makes a new key and an owner that reads no workflow. The store's writer lock, an empty file that stays after a delete
/// on Windows until the unit win-delete-lock lands, is not counted while it is empty. The program stands beside the
/// owner when the build that made this test also made halvo-capture's test targets, as the merge gate's test step does.
#[cfg(windows)]
#[test]
fn on_windows_the_programs_offline_delete_ends_the_owner_and_leaves_no_derived_row_key_or_store() {
    let _serial = serial();
    let scratch = Scratch::new("crash-windows-delete");
    let key = capture_store_and_key(&scratch.0);
    let mut owner = start(&scratch.0, Some(key), 1, true);
    owner.write(&analysis_write(three(two_days_ago()), at(4)));
    assert_null(owner.worker_reply());
    owner.send(&read_workflows());
    assert_eq!(workflows(owner.reply()).len(), 1, "one derived row stands");
    let derived = scratch.0.join("derived");
    assert!(
        derived.join("master.key").is_file(),
        "the derived key stands"
    );
    let pid = owner.child.as_ref().unwrap().id();
    let started =
        halvo_capture::daemon::owner_pid::start_time_of(pid).expect("the owner's start time");
    fs::write(
        derived.join("owner.pid"),
        format!("pid={pid} started={started}\n"),
    )
    .unwrap();

    let program = Path::new(env!("CARGO_BIN_EXE_halvo-owner")).with_file_name("halvo-capture.exe");
    assert!(
        program.is_file(),
        "{} is not built: the test step builds it with the test targets of halvo-capture",
        program.display()
    );
    let deleted = Command::new(&program)
        .args(["--delete-all", "--data-dir"])
        .arg(&scratch.0)
        .stdin(Stdio::null())
        .output()
        .expect("the program of the offline delete runs");
    let printed = format!(
        "exit {:?}, out: {}, err: {}",
        deleted.status.code(),
        String::from_utf8_lossy(&deleted.stdout),
        String::from_utf8_lossy(&deleted.stderr)
    );
    assert_eq!(deleted.status.code(), Some(0), "{printed}");
    let child = owner.child.as_mut().unwrap();
    let deadline = Mono::now() + WAIT;
    while child.try_wait().unwrap().is_none() {
        assert!(Mono::now() < deadline, "the owner did not end: {printed}");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(entries_under(&derived), Vec::<PathBuf>::new(), "{printed}");
    assert_eq!(
        files_under(&scratch.0.join("store")),
        Vec::<PathBuf>::new(),
        "{printed}"
    );
    assert_eq!(
        files_under(&scratch.0.join("roots")),
        Vec::<PathBuf>::new(),
        "{printed}"
    );
    assert!(
        !scratch.0.join(halvo_control::paths::INTENT_FILE).exists(),
        "{printed}"
    );
    drop(owner);

    let fresh = capture_store_and_key(&scratch.0);
    assert_ne!(fresh, key, "the next start makes a new key");
    let mut next = start(&scratch.0, Some(fresh), 1, false);
    next.send(&read_workflows());
    assert_eq!(
        workflows(next.reply()),
        Vec::<WorkflowId>::new(),
        "the next start reads no workflow"
    );
}

/// The step of the delete walk's row derived at which a run of `the_next_start_finishes_a_cut_delete` is cut.
#[cfg(all(target_os = "macos", debug_assertions))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// Before the owner is ended.
    OwnerLives,
    /// Before the key file is destroyed.
    KeyFileStands,
    /// Before derived/ is removed.
    DerivedStands,
    /// With one file of derived/ removed.
    OneFileGone,
    /// With derived/ gone and the intent still there.
    DerivedGone,
}

#[cfg(all(target_os = "macos", debug_assertions))]
impl Step {
    const ALL: [Step; 5] = [
        Step::OwnerLives,
        Step::KeyFileStands,
        Step::DerivedStands,
        Step::OneFileGone,
        Step::DerivedGone,
    ];

    fn name(self) -> &'static str {
        match self {
            Step::OwnerLives => "owner-lives",
            Step::KeyFileStands => "key-file-stands",
            Step::DerivedStands => "derived-stands",
            Step::OneFileGone => "one-file-gone",
            Step::DerivedGone => "derived-gone",
        }
    }

    fn named(name: &str) -> Step {
        Step::ALL
            .into_iter()
            .find(|step| step.name() == name)
            .unwrap()
    }

    fn line(self) -> String {
        format!("crash-step: at {}", self.name())
    }
}

/// The switch of a run of this binary again for `the_next_start_finishes_a_cut_delete`: the step, and the data
/// directory.
#[cfg(all(target_os = "macos", debug_assertions))]
const STEP_SWITCH: &str = "HALVO_CRASH_STEP";
#[cfg(all(target_os = "macos", debug_assertions))]
const STEP_DIR_SWITCH: &str = "HALVO_CRASH_STEP_DIR";
#[cfg(all(target_os = "macos", debug_assertions))]
const CUT_TEST: &str = "the_next_start_finishes_a_cut_delete";

/// The walk's hands of a run that is cut: each call goes on to the product's `SystemHands`, and at the step the
/// run prints its line and waits to be killed.
#[cfg(all(target_os = "macos", debug_assertions))]
struct CutAt(Step);

#[cfg(all(target_os = "macos", debug_assertions))]
impl Hands for CutAt {
    fn end_owner(&self, owner: halvo_capture::daemon::owner_pid::Owner) -> bool {
        if self.0 == Step::OwnerLives {
            print_and_wait(&self.0.line());
        }
        SystemHands.end_owner(owner)
    }

    fn destroy_key_file(&self, data_dir: &Path, file: &str) -> bool {
        if self.0 == Step::KeyFileStands && file == "derived/master.key" {
            print_and_wait(&self.0.line());
        }
        SystemHands.destroy_key_file(data_dir, file)
    }

    fn remove_entry(&self, data_dir: &Path, name: &str) -> u64 {
        if name == "derived" {
            match self.0 {
                Step::DerivedStands => print_and_wait(&self.0.line()),
                Step::OneFileGone => {
                    let first = files_under(&data_dir.join(name))
                        .into_iter()
                        .next()
                        .expect("derived/ holds no file");
                    fs::remove_file(first).unwrap();
                    print_and_wait(&self.0.line());
                }
                Step::DerivedGone => {
                    SystemHands.remove_entry(data_dir, name);
                    print_and_wait(&self.0.line());
                }
                Step::OwnerLives | Step::KeyFileStands => {}
            }
        }
        SystemHands.remove_entry(data_dir, name)
    }
}

/// Ruled point 2 (host rules 2 and 3): for each step of the row derived, a delete that is killed at that step leaves
/// its intent, no file under the data directory that holds the key, and `derived_store_key` answers a fault and
/// makes no file; the next start's `at_start` with the product's rows and hands then finishes the delete whole: no
/// live owner, no entry under derived/, no intent.
#[cfg(all(target_os = "macos", debug_assertions))]
#[test]
fn the_next_start_finishes_a_cut_delete() {
    use halvo_capture::daemon::delete_all::{rows, run, Offline};
    if let Some(step) = std::env::var_os(STEP_SWITCH) {
        let step = Step::named(step.to_str().unwrap());
        let dir = PathBuf::from(std::env::var_os(STEP_DIR_SWITCH).unwrap());
        let rows = rows();
        let hands = CutAt(step);
        let how = Offline {
            rows: &rows,
            select: halvo_capture::daemon::custody_policy::of_this_build,
            hands: &hands,
        };
        let data_dir = halvo_control::paths::DataDir::from_flag(dir).unwrap();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = run(&data_dir, &how, &mut out, &mut err);
        panic!(
            "the delete ran past the step {step:?} and answered {status}: {}",
            String::from_utf8_lossy(&err)
        );
    }
    let _serial = serial();
    for step in Step::ALL {
        cut_and_finish(step);
    }
}

#[cfg(all(target_os = "macos", debug_assertions))]
fn cut_and_finish(step: Step) {
    use halvo_capture::daemon::delete_all::{at_start, rows, AtStart};
    use halvo_capture::daemon::owner_pid::start_time_of;
    use std::os::unix::process::ExitStatusExt;
    let scratch = Scratch::new(&format!("crash-cut-{}", step.name()));
    let dir = scratch.0.clone();
    let key = capture_store_and_key(&dir);
    let mut owner = start(&dir, Some(key), 1, true);
    owner.send(&read_workflows());
    assert_eq!(workflows(owner.reply()), [], "{step:?}");
    let pid = owner.pid();
    let started = write_pid_file(&dir, pid);
    let ended = owner.reaper();

    let mut cut = rerun(
        CUT_TEST,
        &[
            (STEP_SWITCH, OsStr::new(step.name())),
            (STEP_DIR_SWITCH, dir.as_os_str()),
        ],
        Stdio::inherit(),
    );
    let lines = lines_of(&mut cut);
    wait_for(&lines, &step.line());
    cut.kill().unwrap();
    cut.wait().unwrap();

    let intent = dir.join(halvo_control::paths::INTENT_FILE);
    assert!(intent.exists(), "{step:?}: no intent stands");
    assert_eq!(
        files_holding(&dir, &key),
        Vec::<(PathBuf, Vec<&str>)>::new(),
        "{step:?}"
    );
    let before = entries_under(&dir);
    let fault = DataDirStore::at(&dir).derived_store_key().err();
    assert!(
        fault.is_some(),
        "{step:?}: the key of the derived store was answered"
    );
    assert_eq!(
        entries_under(&dir),
        before,
        "{step:?}: the call that answered {fault:?} made an entry"
    );

    let found = at_start(&dir, &rows(), &SystemHands, &DataDirStore::at(&dir), &|| {});
    assert_eq!(found, AtStart::Finished { whole: true }, "{step:?}");
    let exit = ended
        .recv_timeout(WAIT)
        .unwrap_or_else(|e| panic!("{step:?}: the owner did not end: {e}"));
    assert_eq!(exit.signal(), Some(libc::SIGTERM), "{step:?}");
    assert_ne!(
        start_time_of(pid),
        Some(started),
        "{step:?}: the owner lives"
    );
    assert_eq!(
        entries_under(&dir.join("derived")),
        Vec::<PathBuf>::new(),
        "{step:?}"
    );
    assert!(!intent.exists(), "{step:?}: the intent stays");
    drop(owner);
}

/// Ruled point 3: after an owner that ended right after a commit and one that ended before it had its key, the next
/// owner's first read serves no evidence row below its `--first-live`.
#[test]
fn the_next_start_purges_before_its_first_read() {
    let _serial = serial();
    let scratch = Scratch::new("crash-purge");
    let mut first = start(&scratch.0, Some(KEY), 1, true);
    first.write(&analysis_write(three(two_days_ago()), at(4)));
    assert_null(first.worker_reply());
    first.send(&read_workflows());
    let ids = workflows(first.reply());
    assert_eq!(
        ids.len(),
        1,
        "the three summaries of one key make one workflow"
    );
    first.kill();

    let mut keyless = start(&scratch.0, None, 3, false);
    keyless.kill();

    let mut next = start(&scratch.0, Some(KEY), 3, false);
    next.send(&read_evidence(&ids[0]));
    assert_eq!(evidence_segments(next.reply()), [[3]]);
}

/// A recurrence step that prints a line and waits to be killed: a stop between the batch's transaction and the
/// proposals'.
struct HeldBack;

impl RecurrenceStep for HeldBack {
    fn propose(&self, _: &dyn SummaryRead) -> Vec<Proposal> {
        print_and_wait(AT_THE_RECURRENCE_STEP)
    }
}

/// The clock of a run of this binary again: the system's instant, with UTC days.
struct Utc;

impl Clock for Utc {
    fn now(&self) -> Instant {
        Instant(now_ns())
    }

    fn day(&self, at: Instant) -> u32 {
        u32::try_from(at.0.div_euclid(DAY_NS)).unwrap()
    }

    fn day_start(&self, day: u32) -> Instant {
        Instant(i64::from(day) * DAY_NS)
    }
}

/// Bytes of a counter, the ids of a run of this binary again.
struct Counted(u8);

impl IdSource for Counted {
    fn fill(&mut self, bytes: &mut [u8]) {
        for byte in bytes {
            self.0 = self.0.wrapping_add(1);
            *byte = self.0;
        }
    }
}

/// Ruled point 4 (C25.5): a run of this binary again opens the store with the same key and a host whose recurrence
/// step holds, commits the batch and is killed in that step; the real owner then answers the same batch `null` and
/// stores no workflow, and the next batch, of a later position, brings it.
#[test]
fn a_repeated_batch_runs_no_held_back_step() {
    if let Some(dir) = std::env::var_os(BATCH_SWITCH) {
        let day: u32 = std::env::var(DAY_SWITCH).unwrap().parse().unwrap();
        let entries: Vec<&'static KindEntry> = kinds::registry().collect();
        let host = Host::new(Box::new(Utc), Box::new(HeldBack));
        let mut store = Store::open(
            Path::new(&dir),
            &DbKey::new(KEY),
            &entries,
            host,
            &mut Counted(0),
        )
        .unwrap();
        let outcome = store.commit_batch(
            &store_id(),
            &three(day),
            Position { seg: 4, idx: 0 },
            &mut Counted(100),
        );
        panic!(
            "the batch ran past its recurrence step: {}",
            if outcome.is_ok() { "ok" } else { "a fault" }
        );
    }
    let _serial = serial();
    let scratch = Scratch::new("crash-repeated-batch");
    let day = two_days_ago();
    let day_text = day.to_string();
    let mut cut = rerun(
        BATCH_TEST,
        &[
            (BATCH_SWITCH, scratch.0.as_os_str()),
            (DAY_SWITCH, OsStr::new(&day_text)),
        ],
        Stdio::inherit(),
    );
    let lines = lines_of(&mut cut);
    wait_for(&lines, AT_THE_RECURRENCE_STEP);
    cut.kill().unwrap();
    cut.wait().unwrap();

    let mut owner = start(&scratch.0, Some(KEY), 1, true);
    owner.write(&analysis_write(three(day), at(4)));
    assert_null(owner.worker_reply());
    owner.send(&read_workflows());
    assert_eq!(
        workflows(owner.reply()),
        [],
        "the repeated batch made a workflow"
    );
    owner.write(&analysis_write(vec![episode(4, day)], at(5)));
    assert_null(owner.worker_reply());
    owner.send(&read_workflows());
    assert_eq!(
        workflows(owner.reply()).len(),
        1,
        "the next batch made no workflow"
    );
}

/// Ruled point 5 (U5.3b item 4): nothing of a write with a null commit is stored when the owner ends before the
/// commit comes: after two such episodes and a kill, the next owner's commit of the third stores no workflow.
#[test]
fn a_cut_batch_leaves_nothing() {
    let _serial = serial();
    let scratch = Scratch::new("crash-cut-batch");
    let day = two_days_ago();
    let mut first = start(&scratch.0, Some(KEY), 1, true);
    first.write(&analysis_write(
        vec![episode(1, day), episode(2, day)],
        None,
    ));
    assert_null(first.worker_reply());
    first.kill();

    let mut next = start(&scratch.0, Some(KEY), 1, true);
    next.write(&analysis_write(vec![episode(3, day)], at(4)));
    assert_null(next.worker_reply());
    next.send(&read_workflows());
    assert_eq!(workflows(next.reply()), []);
}

/// The test's daemon for the slot: it hands the slot the derived store's key of its data directory as the daemon
/// does, from `derived_store_key`, and counts the asks.
struct Daemon {
    dir: PathBuf,
    asked: AtomicUsize,
}

impl Daemon {
    fn new(dir: &Path) -> Arc<Daemon> {
        Arc::new(Daemon {
            dir: dir.to_path_buf(),
            asked: AtomicUsize::new(0),
        })
    }
}

impl Caller for Daemon {
    fn key(&self) -> Option<StoreKey> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        DataDirStore::at(&self.dir).derived_store_key().ok()
    }

    fn first_live(&self) -> u64 {
        1
    }

    fn deleting(&self) -> bool {
        false
    }

    fn report(&self, _: &[u8]) {}
}

/// The stand-in owner: a copy of this test binary that bears the owner's program name. Before `main` such a copy
/// writes its argument list and its whole environment to the file `SEEN` beside itself, reads its standard input to
/// nowhere and exits; under any other name the hook returns at once.
#[used]
#[cfg_attr(target_os = "macos", link_section = "__DATA,__mod_init_func")]
#[cfg_attr(windows, link_section = ".CRT$XCU")]
static STAND_IN: extern "C" fn() = stand_in;

extern "C" fn stand_in() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    if exe.file_stem() != Some(OsStr::new(OWNER_PROGRAM)) {
        return;
    }
    let mut seen = Vec::new();
    for arg in std::env::args_os() {
        seen.extend_from_slice(b"arg ");
        seen.extend_from_slice(&os_bytes(&arg));
        seen.push(b'\n');
    }
    for (name, value) in std::env::vars_os() {
        seen.extend_from_slice(b"env ");
        seen.extend_from_slice(&os_bytes(&name));
        seen.push(b'=');
        seen.extend_from_slice(&os_bytes(&value));
        seen.push(b'\n');
    }
    let written = fs::write(exe.with_file_name(SEEN), seen);
    let _ = std::io::copy(&mut std::io::stdin(), &mut std::io::sink());
    std::process::exit(if written.is_ok() { 0 } else { 70 });
}

/// The bytes of a text of the system as they came: on macOS as they are, on Windows its UTF-16 as UTF-8, where a
/// form of the key that is text stays as it is.
fn os_bytes(text: &OsStr) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        text.as_bytes().to_vec()
    }
    #[cfg(windows)]
    {
        text.to_string_lossy().into_owned().into_bytes()
    }
}

/// The half of `the_derived_key_stands_in_no_file_line_or_argument` that runs in this binary run again, whose
/// standard error is the test's pipe: an owner that the run starts itself with the worker's pipe pair commits one
/// batch and ends; then the slot starts the real owner on the same key, which reads that workflow, and the slot is
/// stopped.
fn commit_and_serve_by_the_slot(dir: &Path) {
    let key = DataDirStore::at(dir).derived_store_key().unwrap();
    let mut owner = start(dir, Some(*key.expose()), 1, true);
    drop(key);
    owner.write(&analysis_write(three(two_days_ago()), at(4)));
    assert_null(owner.worker_reply());
    assert_eq!(owner.close_and_wait(), Some(halvo_owner::exit::END));
    drop(owner);

    let program = Path::new(env!("CARGO_BIN_EXE_halvo-owner"));
    let place = Place {
        program_dir: program.parent().unwrap().to_path_buf(),
        data_dir: dir.to_path_buf(),
        idle_ms: None,
    };
    let mut slot = OwnerSlot::new(place, Daemon::new(dir) as Arc<dyn Caller>);
    let reply = decode_reply(&slot.relay(&read_workflows()).unwrap()).unwrap();
    assert_eq!(workflows(reply).len(), 1);
    slot.stop();
    println!("{SERVED}");
}

/// Ruled point 6, with the key that the product makes from a capture store: the key's bytes (raw, hex or base64)
/// stand in neither the argument list nor the environment that the slot builds for the owner, which a stand-in
/// program of the owner's name records; in no line of the owner's or the slot's standard error, taken on a pipe; and
/// in no file under the data directory after one batch and a stop of the slot, derived/master.key among them, which
/// holds the key's wrap.
#[test]
fn the_derived_key_stands_in_no_file_line_or_argument() {
    if let Some(dir) = std::env::var_os(KEY_SWITCH) {
        commit_and_serve_by_the_slot(Path::new(&dir));
        return;
    }
    let _serial = serial();
    let scratch = Scratch::new("crash-key");
    let key = capture_store_and_key(&scratch.0);
    for (name, form) in key_forms(&key) {
        let found = forms_in(&form, &key);
        assert!(
            found.contains(&name),
            "the scan misses the form {name}: {found:?}"
        );
    }

    let bin = Scratch::new("crash-key-stand-in");
    let program = bin
        .0
        .join(format!("{OWNER_PROGRAM}{}", std::env::consts::EXE_SUFFIX));
    fs::copy(std::env::current_exe().unwrap(), &program).unwrap();
    let daemon = Daemon::new(&scratch.0);
    let place = Place {
        program_dir: bin.0.clone(),
        data_dir: scratch.0.clone(),
        idle_ms: None,
    };
    let mut slot = OwnerSlot::new(place, Arc::clone(&daemon) as Arc<dyn Caller>);
    let answered = slot.relay(&read_workflows()).is_ok();
    slot.stop();
    assert!(!answered, "the stand-in answered a read");
    assert_eq!(
        daemon.asked.load(Ordering::SeqCst),
        1,
        "the slot asked for no key"
    );
    let seen = fs::read(bin.0.join(SEEN)).expect("the stand-in wrote no record");
    let text = String::from_utf8_lossy(&seen);
    assert!(
        text.contains("arg --first-live=1\n") && text.contains("\nenv "),
        "the record is not the stand-in's: {text}"
    );
    assert_eq!(
        forms_in(&seen, &key),
        Vec::<&str>::new(),
        "the argument list or the environment of the owner holds the key"
    );

    let mut child = rerun(
        KEY_TEST,
        &[(KEY_SWITCH, scratch.0.as_os_str())],
        Stdio::piped(),
    );
    let read_whole = |mut from: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = from.read_to_end(&mut bytes);
            bytes
        })
    };
    let output = read_whole(Box::new(child.stdout.take().unwrap()));
    let errors = read_whole(Box::new(child.stderr.take().unwrap()));
    let deadline = Mono::now() + RERUN_WAIT;
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Mono::now() >= deadline {
            let _ = child.kill();
            child.wait().unwrap();
            panic!("the run of the slot did not end");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let (output, errors) = (output.join().unwrap(), errors.join().unwrap());
    assert!(
        status.success() && String::from_utf8_lossy(&output).contains(SERVED),
        "the run of the slot failed: {}{}",
        String::from_utf8_lossy(&output),
        String::from_utf8_lossy(&errors)
    );
    assert_eq!(
        forms_in(&errors, &key),
        Vec::<&str>::new(),
        "standard error holds the key"
    );
    let files = files_under(&scratch.0);
    let derived = scratch.0.join("derived");
    assert!(
        files.contains(&derived.join("master.key")) && files.contains(&derived.join("derived.db")),
        "{files:?}"
    );
    assert_eq!(
        files_holding(&scratch.0, &key),
        Vec::<(PathBuf, Vec<&str>)>::new()
    );
}
