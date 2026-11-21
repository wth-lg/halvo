//! The process tests of the worker: each starts the built worker as a child, hands it its ends of the read channel
//! and of the owner's channel, and reads what it sends, what it logs and how it exits. A thread of the test serves
//! the fixture transport of halvo-seam on the read channel. The owner is the built binary of halvo-owner, or a
//! thread of the test that answers every write and keeps it.

use std::collections::BTreeMap;
use std::io::{PipeReader, PipeWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::OnceLock;
use std::thread::JoinHandle;
use std::time::{Duration, Instant as Mono, SystemTime, UNIX_EPOCH};

use halvo_sample::build::render_stream;
use halvo_sample::script::Script;
use halvo_schema_core::category::SeedCategory;
use halvo_schema_core::ids::{Position, RequestId, StoreId};
use halvo_schema_core::record::{Episode, Instant, Text, Workflow, WorkflowState};
use halvo_schema_core::request::{
    self, decode_reply, Decoded, InterfaceRead, InterfaceRole, Reply, ReplyResult,
};
use halvo_schema_core::site::{AppId, Domain, SiteValue};
use halvo_seam::fixture::FixtureTransport;
use halvo_seam::frame::{read_frame, write_frame, CLIENT_FRAME_MAX, SERVER_FRAME_MAX};
use halvo_seam::message::decode_client;
use halvo_seam::record::{CaptureRecord, Edge, Payload, RecordId, Seq, Title, LINE_MAX};

/// The worker's exit codes, as the module `exit` of its source names them.
const COMMITTED: i32 = 0;
const USAGE: i32 = 64;
const READ: i32 = 74;
const OWNER: i32 = 76;

const KEY: [u8; 32] = [7; 32];
const DAY_S: i64 = 86_400;
const SECOND_NS: i64 = 1_000_000_000;
/// The longest wait for a frame or an exit that a child owes at once.
const WAIT: Duration = Duration::from_secs(60);

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

/// The time zone that each child gets on unix, where a child reads it from its environment: local time is UTC plus
/// five and a half hours, so a local midnight is no midnight of UTC.
#[cfg(unix)]
const ZONE: &str = "HLV-5:30";

/// The offset of the children's local time from UTC at a second since 1970-01-01, in seconds.
#[cfg(unix)]
fn utc_offset(_secs: i64) -> i64 {
    19_800
}

/// Gives the child the time zone `ZONE`.
#[cfg(unix)]
fn zoned(cmd: &mut Command) {
    cmd.env("TZ", ZONE);
}

/// The worker's own code for Windows, for the offset of local time alone.
#[cfg(windows)]
#[allow(dead_code)]
#[path = "../src/windows.rs"]
mod sys;

/// The offset of the children's local time from UTC at a second since 1970-01-01, in seconds: on Windows a child
/// reads the time zone of the system, so this is the machine's.
#[cfg(windows)]
fn utc_offset(secs: i64) -> i64 {
    i64::from(sys::utc_offset(secs).expect("the system gives a local time"))
}

#[cfg(windows)]
fn zoned(_cmd: &mut Command) {}

fn now_s() -> i64 {
    let since = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    i64::try_from(since.as_secs()).unwrap()
}

/// The children's local day of now, in days since 1970-01-01.
fn local_today() -> i64 {
    let now = now_s();
    (now + utc_offset(now)).div_euclid(DAY_S)
}

/// The rendered days of a development persona, by scripted day; a scripted day is a day of UTC.
fn persona(stream: &str) -> BTreeMap<u32, Vec<u8>> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/r2/scripts/seen");
    let paths: Vec<PathBuf> = [format!("{stream}.jsonl"), format!("{stream}.marked.jsonl")]
        .iter()
        .map(|name| dir.join(name))
        .collect();
    let script = Script::read(&paths).expect("the persona's script reads");
    render_stream(&script, stream, 20261113)
}

fn records(lines: &[u8]) -> Vec<CaptureRecord> {
    lines
        .split_inclusive(|b| *b == b'\n')
        .map(|line| CaptureRecord::read_line(line).expect("a rendered line reads"))
        .collect()
}

/// The UTC day of a rendered day, in days since 1970-01-01.
fn utc_day(lines: &[u8]) -> i64 {
    records(lines)[0].at().0.div_euclid(DAY_S * SECOND_NS)
}

/// The whole seconds that move the records of the UTC day `from` onto the children's local day `to`, at the same
/// time of day.
fn shift_s(from: i64, to: i64) -> i64 {
    (to - from) * DAY_S - utc_offset(to * DAY_S + DAY_S / 2)
}

/// Writes one batch file that holds the records of `lines` with every instant moved by `shift_s` whole seconds,
/// after what the file holds, and returns the records as written.
fn feed(file: &Path, lines: &[u8], shift_s: i64) -> Vec<CaptureRecord> {
    let moved: Vec<CaptureRecord> = records(lines)
        .iter()
        .map(|r| {
            CaptureRecord::new(
                r.id().clone(),
                Instant(r.at().0 + shift_s * SECOND_NS),
                r.seq(),
                r.app().cloned(),
                r.payload().clone(),
            )
            .unwrap()
        })
        .collect();
    let mut out = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)
        .unwrap();
    for record in &moved {
        out.write_all(&record.to_line()).unwrap();
    }
    moved
}

/// The seconds that `feed_repeated` puts between a record that its caller names and the record before it, in place
/// of one second, so that the record is a pause: one second more than the 331 s of the episode step's `GAP` and
/// `LOOK_BACK` together.
const PAUSE_S: i64 = 332;

/// Writes one batch file that holds `count` records on the children's local day `day`, after what the file holds,
/// and returns the records as written. The n-th record repeats the n-th of the records of `lines` that are no run
/// mark, read round and round.
/// Its instant is a whole second: the first lies at six in the morning of that day, and each later one a second
/// after the one before it, or `PAUSE_S` seconds where `pauses` holds its index. Its id and its `seq` come from
/// that second, so each record has an id of its own and the `seq` rises. Where `marks` holds its index with an
/// edge, the record is a run mark of that edge, with no app, in place of the repeated record.
///
/// With `largest`, for the reading of the worker's peak memory alone, every record is a `window_title` record of
/// the largest size that a line carries into the worker's memory: a text of 1,024 bytes, an app id of 255 bytes and
/// a site of its own of 127 bytes, so that each record is an episode of its own that the worker sends. Its line has
/// `LINE_MAX` bytes, the longest that a batch file holds, with spaces beside its object.
fn feed_repeated(
    file: &Path,
    lines: &[u8],
    count: usize,
    day: i64,
    pauses: &[usize],
    marks: &[(usize, Edge)],
    largest: bool,
) -> Vec<CaptureRecord> {
    let scripted: Vec<CaptureRecord> = records(lines)
        .into_iter()
        .filter(|record| !matches!(record.payload(), Payload::RunMark(_)))
        .collect();
    let start_s = day * DAY_S + 6 * 3_600 - utc_offset(day * DAY_S + DAY_S / 2);
    let text = "mail sent ".repeat(103);
    let mut out = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)
        .unwrap();
    let mut fed = Vec::with_capacity(count);
    for n in 0..count {
        let paused = pauses.iter().filter(|&&pause| pause <= n).count();
        let at_s =
            start_s + i64::try_from(n).unwrap() + (PAUSE_S - 1) * i64::try_from(paused).unwrap();
        let id = RecordId::parse(&format!("00000000-0000-7000-8000-{at_s:012x}")).unwrap();
        let seq = Seq::new(u64::try_from(at_s).unwrap()).unwrap();
        let mark = marks.iter().find(|(at, _)| *at == n).map(|&(_, edge)| edge);
        let (app, payload) = if let Some(edge) = mark {
            (None, Payload::RunMark(edge))
        } else if largest {
            let site = format!("{n:063}.{}", "z".repeat(63));
            let title = Title {
                text: Text::parse(&text[..1_024]).unwrap(),
                cut: Some(true),
                why: None,
                site: Some(Domain::parse(&site).unwrap()),
            };
            let app = AppId::parse(&"a".repeat(255)).unwrap();
            (Some(app), Payload::WindowTitle(title))
        } else {
            let record = &scripted[n % scripted.len()];
            (record.app().cloned(), record.payload().clone())
        };
        let record = CaptureRecord::new(id, Instant(at_s * SECOND_NS), seq, app, payload).unwrap();
        let mut line = record.to_line();
        if largest {
            line.pop();
            line.resize(LINE_MAX - 1, b' ');
            line.push(b'\n');
        }
        out.write_all(&line).unwrap();
        fed.push(record);
    }
    fed
}

/// The two ends of a child and the two ends of its peer, of one channel: each side reads what the other writes.
type Ends = (PipeReader, PipeWriter);

fn channel() -> (Ends, Ends) {
    let (peer_reads, child_writes) = std::io::pipe().unwrap();
    let (child_reads, peer_writes) = std::io::pipe().unwrap();
    ((child_reads, child_writes), (peer_reads, peer_writes))
}

/// Starts `cmd` with each pair of ends at the descriptors 3 and 4, then 5 and 6, named by `--<name>=<r>,<w>`.
#[cfg(unix)]
fn launch(cmd: &mut Command, ends: Vec<(&str, Ends)>) -> Child {
    use std::os::fd::AsRawFd;
    use std::os::unix::process::CommandExt;
    let mut places = Vec::new();
    for (n, (name, (reads, writes))) in ends.iter().enumerate() {
        let r = 3 + 2 * i32::try_from(n).unwrap();
        cmd.arg(format!("--{name}={r},{}", r + 1));
        places.push((reads.as_raw_fd(), r));
        places.push((writes.as_raw_fd(), r + 1));
    }
    assert!(places.len() <= 4);
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

/// One lock keeps every start of a child of this binary apart, so that no other child inherits the ends of one.
#[cfg(windows)]
static STARTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Starts `cmd` with each pair of ends made inheritable and named by `--<name>=<r>,<w>` with their handle values.
#[cfg(windows)]
fn launch(cmd: &mut Command, ends: Vec<(&str, Ends)>) -> Child {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::{SetHandleInformation, HANDLE_FLAG_INHERIT};
    let held = STARTS.lock().unwrap_or_else(|e| e.into_inner());
    for (name, (reads, writes)) in ends.iter() {
        let (r, w) = (reads.as_raw_handle(), writes.as_raw_handle());
        for handle in [r, w] {
            // SAFETY: the handle is an open end that this process owns; the call sets its inherit flag alone.
            let set =
                unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) };
            assert_ne!(set, 0, "{}", std::io::Error::last_os_error());
        }
        cmd.arg(format!("--{name}={},{}", r as usize, w as usize));
    }
    let child = cmd.spawn();
    drop(ends);
    drop(held);
    child.unwrap()
}

/// The worker's command. Where the variable HALVO_ANALYSIS_TIME names a file, the worker runs under
/// `/usr/bin/time -l -o <file>`, which writes the worker's peak memory there; the report of the unit holds one
/// such reading.
fn worker_command() -> Command {
    let worker = env!("CARGO_BIN_EXE_halvo-analysis");
    let mut cmd = match std::env::var_os("HALVO_ANALYSIS_TIME") {
        Some(file) if cfg!(unix) => {
            let mut cmd = Command::new("/usr/bin/time");
            cmd.arg("-l").arg("-o").arg(file).arg(worker);
            cmd
        }
        _ => Command::new(worker),
    };
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    zoned(&mut cmd);
    cmd
}

/// Starts the worker on its ends of the read channel and of the owner's channel, from the committed position
/// `from` or, with none, from the zero position.
fn start_worker(read: Ends, owner: Ends, from: Option<(&StoreId, Position)>) -> Child {
    let mut cmd = worker_command();
    if let Some((store, position)) = from {
        cmd.arg(format!("--store={}", store.as_str()));
        cmd.arg(format!("--position={},{}", position.seg, position.idx));
    }
    launch(&mut cmd, vec![("read", read), ("owner", owner)])
}

/// What a child left: its exit code, and what it wrote on standard output and on standard error.
struct Left {
    code: Option<i32>,
    out: String,
    log: String,
}

/// Waits for the child's end, at most `WAIT`.
fn left(mut child: Child) -> Left {
    let deadline = Mono::now() + WAIT;
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Mono::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the child did not end within {WAIT:?}");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let mut out = String::new();
    let mut log = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut out)
        .unwrap();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut log)
        .unwrap();
    Left {
        code: status.code(),
        out,
        log,
    }
}

/// Serves the fixture transport over `files` on the test's ends of the read channel, on a thread, until the worker
/// ends the channel or, with `messages`, until the transport has answered that many messages; then both ends
/// close.
fn serve(files: &[PathBuf], ends: Ends, messages: Option<usize>) -> JoinHandle<()> {
    let mut transport = FixtureTransport::open(files, 1, None).expect("the batch files open");
    let (mut requests, mut replies) = ends;
    std::thread::spawn(move || match messages {
        None => {
            let _ = transport.serve(requests, replies);
        }
        Some(messages) => {
            for _ in 0..messages {
                let Ok(Some(frame)) = read_frame(&mut requests, CLIENT_FRAME_MAX) else {
                    return;
                };
                let message = decode_client(&frame).expect("the worker sends a message");
                let reply = transport.handle(&message).to_payload().unwrap();
                if write_frame(&mut replies, &reply, SERVER_FRAME_MAX).is_err() {
                    return;
                }
            }
        }
    })
}

/// One analysis write as the stand-in owner read it.
struct Sent {
    store: StoreId,
    episodes: Vec<Episode>,
    commit: Option<Position>,
}

/// Plays the owner on the test's ends of the owner's channel, on a thread: it answers every analysis write with the
/// null result until the worker ends the channel, and returns the writes in their order.
fn stand_in_owner(ends: Ends) -> JoinHandle<Vec<Sent>> {
    let (mut requests, mut replies) = ends;
    std::thread::spawn(move || {
        let mut writes = Vec::new();
        while let Ok(Some(frame)) = read_frame(&mut requests, CLIENT_FRAME_MAX) {
            let Ok(Decoded::AnalysisWrite(write)) = request::decode(&frame) else {
                panic!("the worker sent a frame that is no analysis write");
            };
            let reply = request::write_reply(Some(write.id()), Ok(&ReplyResult::Null)).unwrap();
            writes.push(Sent {
                store: write.store().clone(),
                episodes: write.episodes().to_vec(),
                commit: write.commit_position(),
            });
            if write_frame(&mut replies, &reply, SERVER_FRAME_MAX).is_err() {
                break;
            }
        }
        writes
    })
}

/// The owner's binary beside the worker's. A test binary of this package gets the worker's binary from cargo and
/// no binary of another package, so the first call builds the owner's with the cargo that built this test, in the
/// same target directory and profile.
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
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        match profile_dir.file_name().and_then(|name| name.to_str()) {
            Some("debug") => {}
            Some("release") => {
                cmd.arg("--release");
            }
            other => {
                panic!("the worker's binary lies in the directory {other:?}, of no known profile")
            }
        }
        let built = launch(&mut cmd, Vec::new()).wait_with_output().unwrap();
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

/// A running owner with the test's ends of its starter's channel.
struct Owner {
    child: Child,
    requests: Option<PipeWriter>,
    frames: Receiver<Vec<u8>>,
}

/// Starts the built owner on `dir` with the key on standard input and its ends of the worker's channel.
fn start_owner(dir: &Path, worker: Ends) -> Owner {
    let (starter, (replies, requests)) = channel();
    let mut cmd = Command::new(owner_binary());
    cmd.arg(format!("--data-dir={}", dir.display()))
        .arg("--first-live=1")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    zoned(&mut cmd);
    let mut child = launch(&mut cmd, vec![("channel", starter), ("worker", worker)]);
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(&KEY).unwrap();
    drop(stdin);
    let (tx, frames) = mpsc::channel();
    let mut replies = replies;
    std::thread::spawn(move || {
        while let Ok(Some(frame)) = read_frame(&mut replies, SERVER_FRAME_MAX) {
            if tx.send(frame).is_err() {
                return;
            }
        }
    });
    Owner {
        child,
        requests: Some(requests),
        frames,
    }
}

impl Owner {
    /// Sends the interface read `workflows` on the starter's channel and returns its reply, the next frame that is
    /// no report.
    fn workflows(&mut self) -> Reply {
        static NEXT: AtomicU8 = AtomicU8::new(1);
        let id = RequestId::from_random([NEXT.fetch_add(1, Ordering::Relaxed); 16]);
        let read = InterfaceRead::workflows(&InterfaceRole::claim(), id, None)
            .to_bytes()
            .unwrap();
        write_frame(self.requests.as_mut().unwrap(), &read, CLIENT_FRAME_MAX).unwrap();
        loop {
            let frame = self
                .frames
                .recv_timeout(WAIT)
                .expect("the owner sends no reply");
            if !frame.starts_with(br#"{"report":"#) {
                return decode_reply(&frame).unwrap();
            }
        }
    }

    /// Ends the starter's channel and returns the owner's exit code.
    fn end(mut self) -> Option<i32> {
        self.requests = None;
        let deadline = Mono::now() + WAIT;
        while Mono::now() < deadline {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status.code();
            }
            std::thread::sleep(Duration::from_millis(10));
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

/// The scripted days of the persona s001 that the tests feed. On these three the persona reads its mail at
/// `corvid-post.test`, three times in all, and does no other task three times.
const DAYS: [u32; 3] = [17, 18, 23];

/// The stream and the scripted day that held the most records, 147, of the 240 scripts under fixtures/r2/scripts/seen
/// when this test was written; the reading of the worker's peak memory over a scripted day is taken over it
/// (`worker_command`), and the tests of the bound repeat its records (`feed_repeated`).
const LARGEST_DAY: (&str, u32) = ("s029", 8);

/// The most records of one batch, as the constant `RECORDS_MAX` of the worker's source holds it.
const RECORDS_MAX: usize = 16_384;

/// What one start of the worker left, over the batch file `file` from the committed position `from`, beside a
/// stand-in owner: the values of its log line and the commit position of its last write, with every write.
fn one_start(
    file: &Path,
    from: Option<(&StoreId, Position)>,
) -> ([String; 5], Position, Vec<Sent>) {
    let (values, writes) = one_start_writes(file, from);
    let commit = writes
        .last()
        .and_then(|write| write.commit)
        .expect("the last write carries no commit");
    (values, commit, writes)
}

/// What one start of the worker left, as `one_start` reads it, for a start that may send no write: the values of its
/// log line and every write.
fn one_start_writes(file: &Path, from: Option<(&StoreId, Position)>) -> ([String; 5], Vec<Sent>) {
    let (worker_read, server) = channel();
    let (worker_owner, owner) = channel();
    let serving = serve(&[file.to_owned()], server, None);
    let owner = stand_in_owner(owner);
    let left = left(start_worker(worker_read, worker_owner, from));
    assert_eq!(left.code, Some(COMMITTED), "{}", left.log);
    serving.join().unwrap();
    let writes = owner.join().unwrap();
    let values = batch_line(left.log.trim_end()).expect(&left.log);
    (values.map(str::to_owned), writes)
}

/// The values of a log line of the form
/// `batch end=<newest|defer|day|records> run_open=<yes|no> start_first=<yes|no> records=<n> episodes=<n>`, or none
/// for a line of another form.
fn batch_line(line: &str) -> Option<[&str; 5]> {
    let mut words = line.strip_prefix("batch ")?.split(' ');
    let mut values = [""; 5];
    let keys = ["end", "run_open", "start_first", "records", "episodes"];
    for (value, key) in values.iter_mut().zip(keys) {
        *value = words.next()?.strip_prefix(key)?.strip_prefix('=')?;
    }
    let yes_no = |value: &str| value == "yes" || value == "no";
    let count = |value: &str| !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit());
    let fits = words.next().is_none()
        && ["newest", "defer", "day", "records"].contains(&values[0])
        && yes_no(values[1])
        && yes_no(values[2])
        && count(values[3])
        && count(values[4]);
    fits.then_some(values)
}

/// Rule 1 and rule 2: of two days in one stream the worker takes the first alone. It sends its episodes as one
/// batch whose last write alone carries a commit, the position of the second day's first record, and its log says
/// that the batch ended at the day. Where the children's local midnight is no midnight of UTC, a midnight of UTC
/// falls on the middle record of the first day, so a worker that cut its batch by the day of UTC would end it there.
#[test]
fn a_batch_ends_at_the_local_day() {
    let scratch = Scratch::new("worker-day");
    let days = persona("s001");
    let (one, two) = (&days[&DAYS[0]], &days[&DAYS[1]]);
    let file = scratch.0.join("two-days.jsonl");
    let scripted = records(one);
    let middle = scripted[scripted.len() / 2].at().0.div_euclid(SECOND_NS);
    let local_at_utc_midnight = utc_offset(utc_day(one) * DAY_S).rem_euclid(DAY_S);
    let mut shift = shift_s(utc_day(one), utc_day(one));
    if local_at_utc_midnight != 0 {
        shift += local_at_utc_midnight - middle.rem_euclid(DAY_S);
    }
    let first = feed(&file, one, shift);
    let second = feed(&file, two, shift);
    let utc = |record: &CaptureRecord| record.at().0.div_euclid(DAY_S * SECOND_NS);
    assert!(
        local_at_utc_midnight == 0 || utc(&first[0]) != utc(&first[first.len() - 1]),
        "the first day does not cross a midnight of UTC"
    );
    let store = FixtureTransport::open(&[&file], 1, None)
        .unwrap()
        .store()
        .clone();

    let (worker_read, server) = channel();
    let (worker_owner, owner) = channel();
    let serving = serve(&[file], server, None);
    let owner = stand_in_owner(owner);
    let left = left(start_worker(worker_read, worker_owner, None));
    assert_eq!(left.code, Some(COMMITTED), "{}", left.log);
    serving.join().unwrap();
    let writes = owner.join().unwrap();

    let (last, held) = writes.split_last().expect("the worker sent no write");
    assert!(
        writes.iter().all(|write| write.store == store),
        "a write names another store than the one that the worker read"
    );
    assert!(
        held.iter().all(|write| write.commit.is_none()),
        "a write before the last carries a commit"
    );
    let second_day = Position {
        seg: 1,
        idx: u64::try_from(first.len()).unwrap(),
    };
    assert_eq!(last.commit, Some(second_day));
    let episodes: Vec<&Episode> = writes.iter().flat_map(|write| &write.episodes).collect();
    assert!(!episodes.is_empty(), "the first day holds no episode");
    assert!(
        episodes.iter().all(|e| e.end.0 <= second[0].at().0),
        "an episode holds a record of the second day"
    );
    let values = batch_line(left.log.trim_end()).expect(&left.log);
    assert_eq!(values[0], "day", "{}", left.log);
    assert_eq!(values[3], first.len().to_string(), "{}", left.log);
    assert_eq!(values[4], episodes.len().to_string(), "{}", left.log);
}

/// Rule 2: a worker whose read channel ends before its commit exits with the code of a read fault, and the owner
/// that it was started beside holds no workflow.
#[test]
fn a_worker_that_ends_before_its_commit_leaves_nothing() {
    let scratch = Scratch::new("worker-no-commit");
    let days = persona("s001");
    let file = scratch.0.join("day.jsonl");
    let lines = &days[&DAYS[0]];
    feed(&file, lines, shift_s(utc_day(lines), local_today() - 2));
    let data = scratch.0.join("data");
    std::fs::create_dir(&data).unwrap();

    let (worker_read, server) = channel();
    let (worker_owner, owner_worker) = channel();
    let mut owner = start_owner(&data, owner_worker);
    let serving = serve(&[file], server, Some(1));
    let left = left(start_worker(worker_read, worker_owner, None));
    serving.join().unwrap();
    assert_eq!(left.code, Some(READ), "{}", left.log);
    assert!(
        left.log.starts_with("halvo-analysis: read: "),
        "{}",
        left.log
    );

    match owner.workflows().outcome {
        Ok(ReplyResult::Workflows(page)) => assert!(page.workflows.is_empty(), "{page:?}"),
        other => panic!("the read of the workflows answers {other:?}"),
    }
    assert_eq!(owner.end(), Some(halvo_owner::exit::END));
}

/// Rule 3, end to end with the owner's binary: after three days of the persona, each day one start of the owner and
/// of the worker on one data directory with one key, the interface read `workflows` on the owner's starter channel
/// shows one proposal, the mail that the persona read three times.
#[test]
fn three_days_of_a_persona_give_one_proposal() {
    let scratch = Scratch::new("worker-three-days");
    let days = persona("s001");
    let last = local_today() - 2;
    let mut files = Vec::new();
    for day in DAYS {
        let lines = &days[&day];
        let to = last - i64::from(DAYS[2] - day);
        let file = scratch.0.join(format!("day-{day:03}.jsonl"));
        feed(&file, lines, shift_s(utc_day(lines), to));
        files.push(file);
    }
    let store = FixtureTransport::open(&files, 1, None)
        .unwrap()
        .store()
        .clone();
    let data = scratch.0.join("data");
    std::fs::create_dir(&data).unwrap();

    let mut from = None;
    for (n, end) in [(1, "day"), (2, "day"), (3, "newest")] {
        let (worker_read, server) = channel();
        let (worker_owner, owner_worker) = channel();
        let mut owner = start_owner(&data, owner_worker);
        let serving = serve(&files, server, None);
        let left = left(start_worker(worker_read, worker_owner, from));
        serving.join().unwrap();
        assert_eq!(left.code, Some(COMMITTED), "start {n}: {}", left.log);
        let values = batch_line(left.log.trim_end()).expect(&left.log);
        assert_eq!(values[0], end, "start {n}: {}", left.log);
        from = Some((&store, Position { seg: n + 1, idx: 0 }));

        if n == 3 {
            let page = match owner.workflows().outcome {
                Ok(ReplyResult::Workflows(page)) => page,
                other => panic!("the read of the workflows answers {other:?}"),
            };
            let proposed: Vec<_> = page
                .workflows
                .iter()
                .map(|item| match &item.workflow {
                    Workflow::Full(w) => {
                        let site = match &w.site {
                            Some(SiteValue::Domain { key, .. }) => key.as_str(),
                            other => panic!("a proposal with the site {other:?}"),
                        };
                        (w.state, w.category, site, w.recurrence.count)
                    }
                    Workflow::Dismissed(w) => panic!("a dismissed workflow: {w:?}"),
                })
                .collect();
            assert_eq!(
                proposed,
                [(
                    WorkflowState::Proposed,
                    SeedCategory::Email,
                    "corvid-post.test",
                    3
                )]
            );
            assert_eq!(page.workflows[0].summaries.len(), 3);
        }
        assert_eq!(owner.end(), Some(halvo_owner::exit::END), "start {n}");
    }
}

/// Rule 4: for one fed day the worker writes nothing on standard output and one line on standard error, of the
/// form of the batch line, which holds no title, no site and no app id of the fed records.
#[test]
fn the_log_line_holds_no_captured_content() {
    let scratch = Scratch::new("worker-log");
    let (stream, day) = LARGEST_DAY;
    let days = persona(stream);
    let file = scratch.0.join("day.jsonl");
    let lines = &days[&day];
    let fed = feed(&file, lines, shift_s(utc_day(lines), utc_day(lines)));

    let (worker_read, server) = channel();
    let (worker_owner, owner) = channel();
    let serving = serve(&[file], server, None);
    let owner = stand_in_owner(owner);
    let left = left(start_worker(worker_read, worker_owner, None));
    assert_eq!(left.code, Some(COMMITTED), "{}", left.log);
    serving.join().unwrap();
    let writes = owner.join().unwrap();

    assert_eq!(left.out, "");
    let line = left.log.strip_suffix('\n').expect(&left.log);
    let values = batch_line(line).expect(&left.log);
    let sent: usize = writes.iter().map(|write| write.episodes.len()).sum();
    assert_eq!(values[0], "newest", "{}", left.log);
    assert_eq!(values[3], fed.len().to_string(), "{}", left.log);
    assert_eq!(values[4], sent.to_string(), "{}", left.log);
    assert!(sent > 0, "the day holds no episode");

    let mut content = 0;
    for record in &fed {
        let mut texts = Vec::new();
        texts.extend(record.app().map(|app| app.as_str()));
        if let Payload::WindowTitle(title) = record.payload() {
            texts.push(title.text.as_str());
            texts.extend(title.site.as_ref().map(|site| site.as_str()));
        }
        for text in texts {
            assert!(!left.log.contains(text), "the log holds {text:?}");
            content += 1;
        }
    }
    assert!(content > 0, "the fed records hold no title, site or app id");
}

/// Rule 5: a bad argument, a closed read channel and a closed owner's channel end the worker with three codes, none
/// of them 0, and each with one line that names the end.
#[test]
fn each_end_has_its_exit_code() {
    let scratch = Scratch::new("worker-ends");
    let days = persona("s001");
    let file = scratch.0.join("day.jsonl");
    let lines = &days[&DAYS[0]];
    feed(&file, lines, shift_s(utc_day(lines), utc_day(lines)));

    let mut bad = worker_command();
    bad.arg("--nope=1");
    let usage = left(launch(&mut bad, Vec::new()));
    assert_eq!(usage.code, Some(USAGE), "{}", usage.log);
    assert_eq!(usage.log, "halvo-analysis: usage: unknown-argument\n");

    let (worker_read, server) = channel();
    let (worker_owner, owner) = channel();
    drop(server);
    let owner = stand_in_owner(owner);
    let read = left(start_worker(worker_read, worker_owner, None));
    assert_eq!(read.code, Some(READ), "{}", read.log);
    assert!(
        read.log.starts_with("halvo-analysis: read: "),
        "{}",
        read.log
    );
    assert!(owner.join().unwrap().is_empty());

    let (worker_read, server) = channel();
    let (worker_owner, owner) = channel();
    drop(owner);
    let serving = serve(&[file], server, None);
    let closed = left(start_worker(worker_read, worker_owner, None));
    serving.join().unwrap();
    assert_eq!(closed.code, Some(OWNER), "{}", closed.log);
    assert!(
        closed.log.starts_with("halvo-analysis: owner: "),
        "{}",
        closed.log
    );

    assert_eq!((USAGE, READ, OWNER), (64, 74, 76));
}

/// The bound, rule 1: of a day that holds more records than `RECORDS_MAX` and no pause, one start takes
/// `RECORDS_MAX` records. Its log says that the batch ended at the bound, and its commit is the position of the
/// record after them. Where the variable HALVO_ANALYSIS_TIME is set, for the reading of the worker's peak memory,
/// the fed records are of the largest size (`feed_repeated`), and the test prints the bytes of the fed file.
#[test]
fn a_batch_ends_at_the_bound_inside_a_day() {
    let scratch = Scratch::new("worker-bound");
    let (stream, day) = LARGEST_DAY;
    let lines = &persona(stream)[&day];
    let file = scratch.0.join("day.jsonl");
    let count = RECORDS_MAX + records(lines).len();
    let largest = std::env::var_os("HALVO_ANALYSIS_TIME").is_some();
    feed_repeated(&file, lines, count, utc_day(lines), &[], &[], largest);

    let (values, commit, _) = one_start(&file, None);
    assert_eq!(values[0], "records", "{values:?}");
    assert_eq!(values[3], RECORDS_MAX.to_string(), "{values:?}");
    let after = Position {
        seg: 1,
        idx: u64::try_from(RECORDS_MAX).unwrap(),
    };
    assert_eq!(commit, after);
    if largest {
        let bytes = std::fs::metadata(&file).unwrap().len();
        println!("the fed file: {count} records, {bytes} bytes; the batch line: {values:?}");
    }
}

/// The bound, rule 2: a batch that reaches `RECORDS_MAX` records ends before its last pause. Of a day above the
/// bound with two pauses, one start keeps the records before the second: its log counts them, its commit is the
/// position of the record at the pause, and no episode that it sends reaches that record's instant.
#[test]
fn a_batch_ends_at_its_last_pause() {
    let scratch = Scratch::new("worker-pause");
    let (stream, day) = LARGEST_DAY;
    let lines = &persona(stream)[&day];
    let file = scratch.0.join("day.jsonl");
    let count = RECORDS_MAX + records(lines).len();
    let pauses = [5_000, 9_000];
    let fed = feed_repeated(&file, lines, count, utc_day(lines), &pauses, &[], false);

    let (values, commit, writes) = one_start(&file, None);
    assert_eq!(values[0], "records", "{values:?}");
    assert_eq!(values[3], pauses[1].to_string(), "{values:?}");
    let at_pause = Position {
        seg: 1,
        idx: u64::try_from(pauses[1]).unwrap(),
    };
    assert_eq!(commit, at_pause);
    let episodes: Vec<&Episode> = writes.iter().flat_map(|write| &write.episodes).collect();
    assert!(!episodes.is_empty(), "the batch holds no episode");
    assert!(
        episodes.iter().all(|e| e.end.0 <= fed[pauses[1]].at().0),
        "an episode ends after the instant of the record at the pause"
    );
}

/// The bound, rule 3: a start from the commit of the start before reads from that record on. Of a day above the
/// bound with no pause and the day after it, three starts take the bound's records, the rest of the first day and
/// the second day, so the first day commits each of its records once and holds back no later day.
#[test]
fn a_day_above_the_bound_holds_no_later_day() {
    let scratch = Scratch::new("worker-later-day");
    let (stream, day) = LARGEST_DAY;
    let lines = &persona(stream)[&day];
    let file = scratch.0.join("two-days.jsonl");
    let rest = records(lines).len();
    let first_day = utc_day(lines);
    feed_repeated(&file, lines, RECORDS_MAX + rest, first_day, &[], &[], false);
    feed_repeated(&file, lines, rest, first_day + 1, &[], &[], false);
    let store = FixtureTransport::open(&[&file], 1, None)
        .unwrap()
        .store()
        .clone();

    let mut from = None;
    for (end, count) in [("records", RECORDS_MAX), ("day", rest), ("newest", rest)] {
        let (values, commit, _) = one_start(&file, from.map(|position| (&store, position)));
        assert_eq!(values[0], end, "{values:?}");
        assert_eq!(values[3], count.to_string(), "{values:?}");
        from = Some(commit);
    }
}

/// The store id of the fixture transport over the one batch file `file` as it is now.
fn store_of(file: &Path) -> StoreId {
    FixtureTransport::open(&[file], 1, None)
        .unwrap()
        .store()
        .clone()
}

/// The position of the record at `idx` of the one batch file.
fn at_idx(idx: usize) -> Position {
    Position {
        seg: 1,
        idx: u64::try_from(idx).unwrap(),
    }
}

/// The run, rule 1: a record is a pause only when no run is open before it. Of a day above the bound with two
/// pauses, the second inside an agent's run that ends before the bound, one start ends before the first pause.
#[test]
fn a_pause_inside_a_run_is_no_cut() {
    let scratch = Scratch::new("worker-run-pause");
    let (stream, day) = LARGEST_DAY;
    let lines = &persona(stream)[&day];
    let file = scratch.0.join("day.jsonl");
    let count = RECORDS_MAX + records(lines).len();
    let marks = [(8_000, Edge::Start), (10_000, Edge::End)];
    feed_repeated(
        &file,
        lines,
        count,
        utc_day(lines),
        &[5_000, 9_000],
        &marks,
        false,
    );

    let (values, commit, _) = one_start(&file, None);
    assert_eq!(values[3], "5000", "{values:?}");
    assert_eq!(commit, at_idx(5_000));
}

/// The run, rule 2: a batch that reaches the bound with no pause and a run open ends before that run's `start` mark,
/// and the next start reads the run from that mark, so no episode starts inside the run.
#[test]
fn a_bound_inside_a_run_ends_before_it() {
    let scratch = Scratch::new("worker-run-bound");
    let (stream, day) = LARGEST_DAY;
    let lines = &persona(stream)[&day];
    let file = scratch.0.join("day.jsonl");
    let count = RECORDS_MAX + records(lines).len();
    let marks = [(16_000, Edge::Start), (16_450, Edge::End)];
    let fed = feed_repeated(&file, lines, count, utc_day(lines), &[], &marks, false);
    let store = store_of(&file);

    let (values, commit, _) = one_start(&file, None);
    assert_eq!(values[0], "records", "{values:?}");
    assert_eq!(values[1], "no", "{values:?}");
    assert_eq!(values[3], "16000", "{values:?}");
    assert_eq!(commit, at_idx(16_000));

    let (values, _, writes) = one_start(&file, Some((&store, commit)));
    assert_eq!(values[3], "531", "{values:?}");
    let run = fed[16_000].at().0..=fed[16_450].at().0;
    let episodes: Vec<&Episode> = writes.iter().flat_map(|write| &write.episodes).collect();
    assert!(
        !episodes.is_empty(),
        "the records after the run hold no episode"
    );
    assert!(
        episodes.iter().all(|e| !run.contains(&e.start.0)),
        "an episode starts between the instants of the two marks"
    );
}

/// The run, rule 3: a batch that reaches the newest record while a run is open ends before that run's `start` mark,
/// a start from that mark keeps no record and sends nothing, and once the file holds the run's `end` mark a start
/// from the `start` mark reads the run and the records after it.
#[test]
fn a_run_open_at_the_newest_is_read_again() {
    let scratch = Scratch::new("worker-run-newest");
    let (stream, day) = LARGEST_DAY;
    let lines = &persona(stream)[&day];
    let rest = records(lines).len();
    let file = scratch.0.join("day.jsonl");
    let grown = scratch.0.join("grown.jsonl");
    let start = (rest, Edge::Start);
    feed_repeated(
        &file,
        lines,
        rest + 21,
        utc_day(lines),
        &[],
        &[start],
        false,
    );
    let both = [start, (rest + 21, Edge::End)];
    feed_repeated(
        &grown,
        lines,
        2 * rest + 22,
        utc_day(lines),
        &[],
        &both,
        false,
    );
    let store = store_of(&file);

    let (values, commit, _) = one_start(&file, None);
    assert_eq!(values[0], "newest", "{values:?}");
    assert_eq!(values[1], "no", "{values:?}");
    assert_eq!(values[3], rest.to_string(), "{values:?}");
    assert_eq!(commit, at_idx(rest));

    let (values, writes) = one_start_writes(&file, Some((&store, commit)));
    assert_eq!(values[3], "0", "{values:?}");
    assert!(
        writes.is_empty(),
        "a start that keeps no record sent a write"
    );

    let before = std::fs::read(&file).unwrap();
    assert!(std::fs::read(&grown).unwrap().starts_with(&before));
    std::fs::copy(&grown, &file).unwrap();
    let store = store_of(&file);
    let (values, _, _) = one_start(&file, Some((&store, commit)));
    assert_eq!(values[1], "no", "{values:?}");
    assert_eq!(values[3], (rest + 22).to_string(), "{values:?}");
}

/// The run, rule 4: a batch whose first record is the `start` mark of a run that is open after the bound keeps no
/// record and reads on to the run's `end` mark, whose position it commits, and the next start reads from that mark.
#[test]
fn a_run_above_the_bound_is_read_past() {
    let scratch = Scratch::new("worker-run-past");
    let (stream, day) = LARGEST_DAY;
    let lines = &persona(stream)[&day];
    let rest = records(lines).len();
    let file = scratch.0.join("day.jsonl");
    let end = 1 + RECORDS_MAX + rest;
    let marks = [(0, Edge::Start), (end, Edge::End)];
    feed_repeated(
        &file,
        lines,
        end + 1 + rest,
        utc_day(lines),
        &[],
        &marks,
        false,
    );
    let store = store_of(&file);

    let (values, commit, _) = one_start(&file, None);
    assert_eq!(values[0], "records", "{values:?}");
    assert_eq!(values[1], "no", "{values:?}");
    assert_eq!(values[3], "0", "{values:?}");
    assert_eq!(values[4], "0", "{values:?}");
    assert_eq!(commit, at_idx(end));

    let (values, _, _) = one_start(&file, Some((&store, commit)));
    assert_eq!(values[0], "newest", "{values:?}");
    assert_eq!(values[3], (rest + 1).to_string(), "{values:?}");
}
