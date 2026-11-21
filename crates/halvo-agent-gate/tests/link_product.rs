//! The command `task` of `agent-gate` over the product link (contracts.md A14.5, plan.md U9.4): with `--link product`
//! the command makes no gate host and reaches the product's owner through U9.2's link, `StreamLink` over `ClientEnds`
//! over the call `owner_stream` of a client of its own, and its line holds `store=owner`.
//!
//! Each test starts on ends of its own: a stand-in of the capture service's server on a control socket under a data
//! directory of the test, and in the first test the owner's binary behind that server. The test ends and waits for
//! every child that it starts.
#![cfg(target_os = "macos")]

mod common;

use std::io::{PipeReader, PipeWriter, Write};
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant as Mono, SystemTime, UNIX_EPOCH};

use common::Guarded;
use halvo_agent_client::link::product::ClientEnds;
use halvo_agent_client::link::StreamLink;
use halvo_agent_client::OwnerLink;
use halvo_control::client::{system_connector, Client};
use halvo_control::paths::{socket_path, DataDir, SystemAccount};
use halvo_control::proto::{decode_request, encode_reply, unknown_answer, Decoded};
use halvo_control::r2::{dispatch_unknown, mark_request, send_with_end, OwnerChannel};
use halvo_control::wire::{self, REPLY_MAX_BYTES, REQUEST_MAX_BYTES};
use halvo_schema::executable::body::{
    CoordinatorCall, CoordinatorReply, ListCheckpoints, ListedCheckpoint,
};
use halvo_schema::executable::names::YesNo;
use halvo_schema::executable::text::Null;
use halvo_schema::ids::{Position, RequestId, StoreId, SummaryId, WorkflowId};
use halvo_schema::record::{
    Episode, FloorLayer, Instant, ObservedStep, Ordinal, Part, Tagged, Text,
};
use halvo_schema::request::{
    decode_reply, AnalysisRole, AnalysisWrite, CoordinatorRole, CoordinatorWrite,
    InterfaceDecision, InterfaceRead, InterfaceRole, Reply, ReplyResult,
};
use halvo_schema::site::AppId;
use halvo_schema::SeedCategory;
use halvo_seam::frame::{read_frame, write_frame, CLIENT_FRAME_MAX, SERVER_FRAME_MAX};

/// The owner's key, which the test hands it on standard input.
const KEY: [u8; 32] = [7; 32];
const DAY_NS: i64 = 86_400_000_000_000;
const FAR: i64 = 9_000_000_000_000_000_000;
/// The longest wait for a frame that the owner owes at once.
const WAIT: Duration = Duration::from_secs(30);
/// The longest wait for a play of `fill` from start to end.
const PLAY: Duration = Duration::from_secs(180);

/// The keys of the line of A14.5, in its order.
const KEYS: [&str; 15] = [
    "task",
    "name",
    "mode",
    "seed",
    "store",
    "steps",
    "prompts",
    "handbacks",
    "confirmations",
    "effects",
    "unknown",
    "model_calls",
    "marks",
    "fixture",
    "result",
];

fn scratch(name: &str) -> PathBuf {
    halvo_test_dir::make(Path::new(env!("CARGO_TARGET_TMPDIR")), name).expect("a scratch directory")
}

/// A new directory `name` in `parent`.
fn dir_in(parent: &Path, name: &str) -> PathBuf {
    let dir = parent.join(name);
    std::fs::create_dir(&dir).expect("a directory of the test");
    dir
}

fn request_id() -> RequestId {
    static NEXT: AtomicU8 = AtomicU8::new(1);
    RequestId::from_random([NEXT.fetch_add(1, Ordering::Relaxed); 16])
}

/// A running owner with the test's ends of its starter's channel and of its worker's channel.
struct Owner {
    child: Child,
    requests: PipeWriter,
    frames: Receiver<Vec<u8>>,
    worker: PipeWriter,
    worker_frames: Receiver<Vec<u8>>,
}

impl Drop for Owner {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
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

fn is_report(frame: &[u8]) -> bool {
    let value: serde_json::Value = serde_json::from_slice(frame).expect("a JSON frame");
    value
        .as_object()
        .is_some_and(|object| object.len() == 1 && object.contains_key("report"))
}

/// Starts the owner's binary `binary` on `dir` with the key on standard input, with a starter's channel on which an
/// end of a stream travels (one socket pair, whose one end and a copy of it by `dup` are the owner's two ends, as
/// owner.rs of halvo-owner has it) and a worker's channel of two pipes.
fn start_owner(binary: &Path, dir: &Path) -> Owner {
    use std::os::fd::{AsRawFd, OwnedFd};
    use std::os::unix::process::CommandExt;
    let (owner_end, test_end) = UnixStream::pair().expect("a socket pair");
    let copy = |end: &UnixStream| OwnedFd::from(end.try_clone().expect("a copy of an end"));
    let owner_reads = PipeReader::from(copy(&owner_end));
    let owner_writes = PipeWriter::from(OwnedFd::from(owner_end));
    let requests = PipeWriter::from(copy(&test_end));
    let replies = PipeReader::from(OwnedFd::from(test_end));
    let (worker_reads, worker) = std::io::pipe().expect("a pipe");
    let (worker_replies, worker_writes) = std::io::pipe().expect("a pipe");

    let places = [
        (owner_reads.as_raw_fd(), 3),
        (owner_writes.as_raw_fd(), 4),
        (worker_reads.as_raw_fd(), 5),
        (worker_writes.as_raw_fd(), 6),
    ];
    let mut cmd = Command::new(binary);
    cmd.arg(format!("--data-dir={}", dir.display()))
        .arg("--first-live=1")
        .arg("--channel=3,4")
        .arg("--worker=5,6")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
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
    let mut child = cmd.spawn().expect("the owner starts");
    drop((owner_reads, owner_writes, worker_reads, worker_writes));
    let mut stdin = child.stdin.take().expect("the owner's standard input");
    stdin.write_all(&KEY).expect("the key");
    drop(stdin);
    Owner {
        child,
        requests,
        frames: frames(replies),
        worker,
        worker_frames: frames(worker_replies),
    }
}

impl Owner {
    /// The reply to one request on the starter's channel: the next frame there that is no report.
    fn ask(&mut self, request: &[u8]) -> Reply {
        write_frame(&mut self.requests, request, CLIENT_FRAME_MAX).expect("a request");
        loop {
            let frame = self
                .frames
                .recv_timeout(WAIT)
                .expect("the owner sends no reply");
            if !is_report(&frame) {
                return decode_reply(&frame).expect("a reply");
            }
        }
    }

    /// The reply to one write on the worker's channel.
    fn write(&mut self, request: &[u8]) -> Reply {
        write_frame(&mut self.worker, request, CLIENT_FRAME_MAX).expect("a write");
        let frame = self
            .worker_frames
            .recv_timeout(WAIT)
            .expect("the owner sends no reply to the worker");
        decode_reply(&frame).expect("a reply")
    }
}

/// An episode of the segment `n` of the category `spreadsheet_fill`, of one key for every `n`.
fn episode(n: u8, day: u32) -> Episode {
    let start = i64::from(day) * DAY_NS + 64_800_000_000_000 + i64::from(n);
    Episode {
        id: SummaryId::from_random([n; 16]),
        category: SeedCategory::SpreadsheetFill,
        site: None,
        app: AppId::parse("com.example.browser").expect("an app"),
        day,
        part: Part::Evening,
        minutes: 5,
        keys: None,
        pointer: None,
        segments: vec![Ordinal::new(u64::from(n)).expect("an ordinal")],
        expires: Instant(FAR),
        start: Instant(start),
        end: Instant(start + 1_000),
        steps: vec![ObservedStep {
            app: None,
            title: Some(Tagged {
                value: Text::parse("Fill the ledger").expect("a title"),
                layer: FloorLayer::Floor,
            }),
            input: None,
            site: None,
            text: None,
        }],
    }
}

/// Makes one approved workflow of `spreadsheet_fill` in the owner's store through the owner's own channels, as the
/// product makes one: three summaries of one key on the worker's channel, from which the recurrence step proposes it,
/// then the person's `approve` on the starter's channel. Returns its id.
fn approved_workflow(owner: &mut Owner) -> WorkflowId {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("a clock after 1970");
    let day = u32::try_from(
        i64::try_from(now.as_nanos())
            .expect("a time")
            .div_euclid(DAY_NS),
    )
    .expect("a day")
        - 2;
    let store = StoreId::parse(&"1".repeat(32)).expect("a store id");
    let mut write = AnalysisWrite::new(&AnalysisRole::claim(), request_id(), store);
    for n in 1..=3 {
        write.push(episode(n, day)).expect("an episode");
    }
    write.commit(Position { seg: 4, idx: 0 });
    let written = owner.write(&write.to_bytes().expect("a write"));
    assert!(
        matches!(written.outcome, Ok(ReplyResult::Null)),
        "{written:?}"
    );

    let read = InterfaceRead::workflows(&InterfaceRole::claim(), request_id(), None);
    let ids: Vec<WorkflowId> = match owner.ask(&read.to_bytes().expect("a read")).outcome {
        Ok(ReplyResult::Workflows(page)) => page
            .workflows
            .iter()
            .map(|item| item.workflow.id().clone())
            .collect(),
        other => panic!("the read of the workflows answers {other:?}"),
    };
    assert_eq!(
        ids.len(),
        1,
        "the three summaries of one key make one workflow"
    );
    let approve = InterfaceDecision::approve(&InterfaceRole::claim(), request_id(), ids[0].clone());
    let approved = owner.ask(&approve.to_bytes().expect("a decision"));
    assert!(
        matches!(approved.outcome, Ok(ReplyResult::Null)),
        "{approved:?}"
    );
    ids[0].clone()
}

/// The test's ends of the starter's channel, as the daemon's handler of `owner_stream` uses them (owner.rs of
/// halvo-owner).
struct Starter<'a>(&'a mut Owner);

impl OwnerChannel for Starter<'_> {
    fn send_stream(
        &mut self,
        frame: &[u8],
        end: std::os::fd::BorrowedFd<'_>,
    ) -> std::io::Result<()> {
        use std::os::fd::AsFd;
        send_with_end(self.0.requests.as_fd(), frame, end)
    }

    fn answer(&mut self, within: Duration) -> std::io::Result<Option<Vec<u8>>> {
        let deadline = Mono::now() + within;
        loop {
            let left = deadline.saturating_duration_since(Mono::now());
            match self.0.frames.recv_timeout(left) {
                Ok(frame) if is_report(&frame) => {}
                Ok(frame) => return Ok(Some(frame)),
                Err(RecvTimeoutError::Timeout) => return Err(std::io::ErrorKind::TimedOut.into()),
                Err(RecvTimeoutError::Disconnected) => return Ok(None),
            }
        }
    }
}

/// How the stand-in server answers a request `owner_stream`.
enum Answer<'a> {
    /// As the daemon's server answers it with the workflows run's handler: `dispatch_unknown` with the test's ends of
    /// the owner's starter's channel.
    Dispatch(&'a mut Owner),
    /// As the capture service's server answers a request that it does not serve: `mark_request` finds no mark in it,
    /// and the server writes the answer to an unknown request.
    Unknown,
}

/// The data directory of a stand-in server, with its control socket where the product's client looks for it.
fn control_dir(parent: &Path) -> (PathBuf, UnixListener) {
    let dir = dir_in(parent, "product");
    let data_dir = DataDir::from_flag(&dir).expect("a data directory");
    let socket = socket_path(&data_dir, &SystemAccount).expect("a socket path");
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(socket.parent().expect("the socket's directory"))
        .expect("the socket's directory");
    let listener = UnixListener::bind(&socket).expect("the control socket");
    listener
        .set_nonblocking(true)
        .expect("a listener that polls");
    (dir, listener)
}

/// Serves each connection on `listener` until `stop` holds, one request on each, as the capture service's server
/// reads it, and returns how many requests `owner_stream` it answered.
fn serve(listener: &UnixListener, mut answer: Answer<'_>, stop: &AtomicBool) -> usize {
    let mut served = 0;
    while !stop.load(Ordering::Relaxed) {
        let connection = match listener.accept() {
            Ok((connection, _)) => connection,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(10));
                continue;
            }
            Err(e) => panic!("the stand-in server accepts no connection: {e}"),
        };
        connection
            .set_nonblocking(false)
            .expect("a blocking connection");
        let frame = wire::read_frame(&mut &connection, REQUEST_MAX_BYTES).expect("a request");
        let Ok(Decoded::Unknown(request)) = decode_request(&frame) else {
            panic!("the request is no body of version 2");
        };
        assert_eq!(request.op(), Some("owner_stream"));
        match &mut answer {
            Answer::Dispatch(owner) => {
                assert!(
                    dispatch_unknown(&request, &connection, &mut Starter(owner)),
                    "dispatch_unknown wrote no reply"
                );
            }
            Answer::Unknown => {
                assert!(mark_request(&request).is_none(), "owner_stream is no mark");
                let reply = encode_reply(&unknown_answer(&request));
                wire::write_frame(&mut &connection, &reply, REPLY_MAX_BYTES).expect("the answer");
            }
        }
        served += 1;
    }
    served
}

/// Sets its flag when it is dropped, also when a panic unwinds past it.
struct StopOnDrop<'a>(&'a AtomicBool);

impl Drop for StopOnDrop<'_> {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// Runs `play` while a thread of the test serves `listener`, then stops that thread, also when `play` panics: what
/// `play` returned and how many requests `owner_stream` the server answered.
fn with_server<T>(
    listener: &UnixListener,
    answer: Answer<'_>,
    play: impl FnOnce() -> T,
) -> (T, usize) {
    let stop = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let server = scope.spawn(|| serve(listener, answer, &stop));
        let played = {
            let _stop = StopOnDrop(&stop);
            play()
        };
        (played, server.join().expect("the stand-in server"))
    })
}

/// The command's form for `fill` of seed 1 over the product link.
fn fill_over_product<'a>(workflow: &'a str, data_dir: &'a str) -> [&'a str; 12] {
    [
        "task",
        "fill",
        "--mode",
        "replay",
        "--seed",
        "1",
        "--link",
        "product",
        "--workflow",
        workflow,
        "--data-dir",
        data_dir,
    ]
}

/// The checkpoints that the owner's store holds, read with `list_checkpoints` through the product's link over a
/// client of the test's own. The owner's stream serves no read of the agents or of their history; each checkpoint
/// says whether the run's record is stored (`ended`).
fn listed_checkpoints(data_dir: &Path) -> Vec<ListedCheckpoint> {
    let data_dir = DataDir::from_flag(data_dir).expect("a data directory");
    let client = Client::new(system_connector(&data_dir).expect("a connector"));
    let link = StreamLink::new(ClientEnds::new(move || client.owner_stream()));
    let id = request_id();
    let call = CoordinatorCall::ListCheckpoints(ListCheckpoints {
        workflow: Null,
        agent: Null,
        after: None,
    });
    let write = CoordinatorWrite::new(&CoordinatorRole::claim(), id.clone(), call);
    let reply = link
        .request(&write.to_bytes().expect("a write"))
        .expect("the owner's reply to list_checkpoints");
    let reply = decode_reply(&reply).expect("a reply");
    assert_eq!(reply.id, Some(id));
    let Ok(ReplyResult::Raw(raw)) = reply.outcome else {
        panic!("list_checkpoints answers {:?}", reply.outcome);
    };
    match CoordinatorReply::parse(raw.get().as_bytes()) {
        Ok(CoordinatorReply::Checkpoints(page)) if page.next.is_none() => page.checkpoints,
        other => panic!("list_checkpoints answers {other:?}"),
    }
}

/// U9.4 sentence 1: over the product link, with a stand-in server that calls `dispatch_unknown` in front of the
/// owner's binary, the command prints for `fill`, seed 1, the one line of A14.5, its keys in its order, with
/// `store=owner` and `result=completed`. It makes no gate host and no store of its own: the owner's store holds the
/// checkpoint of the command's run of the workflow and the run's record, as `list_checkpoints` reads them afterwards.
#[test]
fn a_fill_over_the_product_link_reaches_the_owner_and_prints_store_owner() {
    common::build_beside("halvo-runner", "halvo-runner");
    let owner_binary = common::build_beside("halvo-owner", "halvo-owner");
    let root = scratch("link-product-fill");
    let tmp = dir_in(&root, "tmp");
    let mut owner = start_owner(&owner_binary, &dir_in(&root, "owner"));
    let workflow = approved_workflow(&mut owner);
    let (data_dir, listener) = control_dir(&root);
    let data_dir_text = data_dir.to_str().expect("a UTF-8 path");

    let ((out, checkpoints), served) = with_server(&listener, Answer::Dispatch(&mut owner), || {
        let form = fill_over_product(workflow.as_str(), data_dir_text);
        let out = Guarded::run_in(&form, &tmp, PLAY);
        (out, listed_checkpoints(&data_dir))
    });

    assert_eq!(out.code, Some(0), "{}{}", out.stdout, out.stderr);
    let lines: Vec<&str> = out.stdout.lines().collect();
    assert_eq!(lines.len(), 1, "{}", out.stdout);
    let words: Vec<&str> = lines[0].split(' ').collect();
    let keys: Vec<&str> = words
        .iter()
        .map(|word| word.split('=').next().unwrap_or(word))
        .collect();
    assert_eq!(keys, KEYS, "{}", lines[0]);
    assert!(
        lines[0].starts_with("task name=fill mode=replay seed=1 "),
        "{}",
        lines[0]
    );
    for pair in [
        "store=owner",
        "result=completed",
        "fixture=ok",
        "marks=start,end",
    ] {
        assert!(words.contains(&pair), "{pair} in {}", lines[0]);
    }
    assert_eq!(
        served, 2,
        "one stream for the command and one for the test's read"
    );
    let of_workflow: Vec<&ListedCheckpoint> = checkpoints
        .iter()
        .filter(|checkpoint| checkpoint.workflow == workflow)
        .collect();
    assert_eq!(of_workflow.len(), 1, "{checkpoints:?}");
    assert_eq!(
        of_workflow[0].ended,
        YesNo::Yes,
        "the owner's store holds the record of the run: {checkpoints:?}"
    );
}

/// U9.4 sentence 2: against a server that answers `owner_stream` as an unknown request, as the capture service's
/// server does, the command prints no line with `result=completed`: it refuses with `usage`, prints no line on
/// standard output, and exits 2.
#[test]
fn a_server_that_does_not_serve_owner_stream_gives_no_completed_line() {
    let root = scratch("link-product-unknown");
    let tmp = dir_in(&root, "tmp");
    let (data_dir, listener) = control_dir(&root);
    let workflow = WorkflowId::from_random([9; 16]);
    let data_dir_text = data_dir.to_str().expect("a UTF-8 path");

    let (out, served) = with_server(&listener, Answer::Unknown, || {
        let form = fill_over_product(workflow.as_str(), data_dir_text);
        Guarded::run_in(&form, &tmp, PLAY)
    });

    assert!(!out.stdout.contains("result=completed"), "{}", out.stdout);
    assert!(out.stdout.is_empty(), "{}", out.stdout);
    assert_eq!(out.code, Some(2), "{}", out.stderr);
    assert_eq!(
        out.stderr,
        "REFUSED usage: the product link gave no reply to the body create_agent\n"
    );
    assert_eq!(served, 1, "the command asked the server for one end");
}
