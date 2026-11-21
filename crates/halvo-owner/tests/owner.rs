//! The process tests of the owner: each starts the built binary as a child, hands it the key on standard input and
//! the ends of its channels, and reads what it writes.

use std::io::{PipeReader, PipeWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant as Mono, SystemTime, UNIX_EPOCH};

use halvo_owner::exit;
use halvo_schema_core::category::SeedCategory;
use halvo_schema_core::ids::{Position, RequestId, StoreId, SummaryId, WorkflowId};
use halvo_schema_core::record::{
    Episode, FloorLayer, Instant, ObservedStep, Ordinal, Part, Tagged, Text,
};
use halvo_schema_core::request::{
    decode_reply, AnalysisRole, AnalysisWrite, InterfaceRead, InterfaceRole, Reply, ReplyResult,
};
use halvo_schema_core::site::AppId;
use halvo_seam::frame::{read_frame, write_frame, CLIENT_FRAME_MAX, SERVER_FRAME_MAX};
use serde_json::Value;

const KEY: [u8; 32] = [7; 32];
const OTHER_KEY: [u8; 32] = [8; 32];
const DAY_NS: i64 = 86_400_000_000_000;
const FAR: i64 = 9_000_000_000_000_000_000;
/// The longest wait for a frame or an exit that the owner owes at once.
const WAIT: Duration = Duration::from_secs(30);

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

/// A running owner with the test's ends of its channels.
struct Owner {
    child: Child,
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

/// Starts the built owner on `dir` with `key` on standard input, the lowest live ordinal `first_live`, the
/// starter's channel and, where `worker` holds, the worker's pipe pair.
fn start(dir: &Path, key: [u8; 32], first_live: u64, worker: bool, extra: &[&str]) -> Owner {
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
        .args(extra)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    let mut child = launch(&mut cmd, ends);
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(&key).unwrap();
    drop(stdin);
    let (worker, worker_frames) = match test_ends {
        Some((writes, reads)) => (Some(writes), Some(frames(reads))),
        None => (None, None),
    };
    Owner {
        child,
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
    use std::sync::Mutex;
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

fn is_report(frame: &[u8]) -> Option<Value> {
    let value: Value = serde_json::from_slice(frame).unwrap();
    let object = value.as_object()?;
    (object.len() == 1 && object.contains_key("report")).then(|| value["report"].clone())
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
            if is_report(&frame).is_none() {
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

    /// The next report on the starter's channel for which `wanted` holds, read before `deadline`, with the text of
    /// its frame, whose keys stand in the order in which the owner wrote them.
    fn report_where(
        &mut self,
        deadline: Mono,
        wanted: impl Fn(&Value) -> bool,
    ) -> Option<(Value, String)> {
        loop {
            let left = deadline.saturating_duration_since(Mono::now());
            let frame = self.frames.recv_timeout(left).ok()?;
            if let Some(report) = is_report(&frame).filter(|report| wanted(report)) {
                return Some((report, String::from_utf8(frame).unwrap()));
            }
        }
    }

    /// Ends the starter's channel: the owner reads its end.
    fn close(&mut self) {
        self.requests = None;
    }

    /// The exit code of the owner within `within`, or none.
    fn exit_code(&mut self, within: Duration) -> Option<i32> {
        let deadline = Mono::now() + within;
        while Mono::now() < deadline {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status.code();
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        None
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
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

fn analysis_write(episodes: Vec<Episode>, commit: Position) -> Vec<u8> {
    let store = StoreId::parse(&"1".repeat(32)).unwrap();
    let mut write = AnalysisWrite::new(&AnalysisRole::claim(), request_id(), store);
    for episode in episodes {
        write.push(episode).unwrap();
    }
    write.commit(commit);
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
fn episode(n: u8, day: u32, expires: i64) -> Episode {
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
        expires: Instant(expires),
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

/// Rule 1: the owner opens the store with the key on standard input; an owner with another key answers no request
/// and exits with the code of the refusal, and the store opens with the first key again.
#[test]
fn the_owner_opens_with_the_key_on_standard_input_and_refuses_another() {
    let scratch = Scratch::new("owner-key");

    let mut first = start(&scratch.0, KEY, 1, false, &[]);
    first.send(&read_workflows());
    assert_eq!(workflows(first.reply()), []);
    first.close();
    assert_eq!(first.exit_code(WAIT), Some(exit::END));
    assert!(scratch.0.join("derived").is_dir());

    let mut other = start(&scratch.0, OTHER_KEY, 1, false, &[]);
    let _ = write_frame(
        other.requests.as_mut().unwrap(),
        &read_workflows(),
        CLIENT_FRAME_MAX,
    );
    assert_eq!(other.exit_code(WAIT), Some(exit::SEALED));
    assert_eq!(
        other.frames.recv_timeout(WAIT),
        Err(RecvTimeoutError::Disconnected),
        "the owner with another key writes nothing"
    );

    let mut third = start(&scratch.0, KEY, 1, false, &[]);
    third.send(&read_workflows());
    assert_eq!(workflows(third.reply()), []);
    third.close();
    assert_eq!(third.exit_code(WAIT), Some(exit::END));
}

/// Rule 3 and the idle half of rule 5: with the lowest live ordinal 2 the owner removes the evidence row of the
/// segment 1 before its first read, and with its channel open and no request it exits 0 after its idle time.
#[test]
fn the_owner_purges_below_the_lowest_live_ordinal_before_its_first_read() {
    let scratch = Scratch::new("owner-purge");
    let day = two_days_ago();

    let mut first = start(&scratch.0, KEY, 1, true, &[]);
    first.write(&analysis_write(
        vec![
            episode(1, day, FAR),
            episode(2, day, FAR),
            episode(3, day, FAR),
        ],
        Position { seg: 4, idx: 0 },
    ));
    assert!(matches!(
        first.worker_reply().outcome,
        Ok(ReplyResult::Null)
    ));
    first.send(&read_workflows());
    let ids = workflows(first.reply());
    assert_eq!(
        ids.len(),
        1,
        "the three summaries of one key make one workflow"
    );
    first.send(&read_evidence(&ids[0]));
    assert_eq!(evidence_segments(first.reply()), [[1], [2], [3]]);
    first.close();
    assert_eq!(first.exit_code(WAIT), Some(exit::END));

    let mut second = start(&scratch.0, KEY, 2, false, &["--idle-ms=1000"]);
    second.send(&read_evidence(&ids[0]));
    assert_eq!(evidence_segments(second.reply()), [[2], [3]]);
    assert_eq!(second.exit_code(WAIT), Some(exit::END));
    assert!(
        second.requests.is_some(),
        "the channel was open when the owner exited"
    );
}

/// Rule 4 and the other half of rule 5: a report with the four values follows a commit; with no request the owner
/// runs the expiry at its check and reports the window row gone; after the end of its channel it writes a last report
/// and exits 0.
#[test]
fn the_owner_reports_each_commit_expires_by_itself_and_reports_before_it_exits() {
    let scratch = Scratch::new("owner-expiry");
    let mut owner = start(
        &scratch.0,
        KEY,
        1,
        true,
        &["--check-ms=1000", "--idle-ms=60000"],
    );
    let opened = owner.report_where(Mono::now() + WAIT, |_| true);
    assert!(opened.is_some(), "the owner reports its open");

    let expires = now_ns() + 2_000_000_000;
    owner.write(&analysis_write(
        vec![episode(1, two_days_ago(), expires)],
        Position { seg: 2, idx: 0 },
    ));
    assert!(matches!(
        owner.worker_reply().outcome,
        Ok(ReplyResult::Null)
    ));
    let (committed, text) = owner
        .report_where(Mono::now() + WAIT, |r| r["counts"]["window"] == 1)
        .expect("no report shows the committed window row");
    assert!(
        text.starts_with(r#"{"report":{"revisions":{"workflow":"#),
        "{text}"
    );
    assert!(
        text.contains(r#"},"counts":{"window":1,"bounded":1,"kept":0},"disk_bytes":"#),
        "{text}"
    );
    let families = committed["revisions"].as_object().unwrap();
    assert!(families.values().all(Value::is_u64));
    assert!(committed["disk_bytes"].as_u64().unwrap() > 0);
    assert_eq!(committed["expiry"], expires);

    let expired = owner.report_where(Mono::now() + Duration::from_secs(10), |r| {
        r["counts"]["window"] == 0
    });
    assert!(
        expired.is_some(),
        "no report of the expiry within ten seconds"
    );

    owner.close();
    let mut last = None;
    while let Ok(frame) = owner.frames.recv_timeout(WAIT) {
        last = Some(is_report(&frame).expect("a frame after the end of the channel is no report"));
    }
    let last = last.expect("no last report after the end of the channel");
    assert_eq!(last["counts"]["window"], 0);
    assert_eq!(owner.exit_code(WAIT), Some(exit::END));
}
