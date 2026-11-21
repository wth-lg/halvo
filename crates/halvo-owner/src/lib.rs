//! The owner of the derived store (branch B of the workflow run): the one process of the product that opens the
//! store under `<DIR>/derived/`, and the host's step list (`steps`), from which it builds its host alone.
//!
//! # How the owner is started
//!
//! ```text
//! halvo-owner --data-dir=<absolute DIR> --first-live=<n> --channel=<r>,<w> [--worker=<r>,<w>]
//!             [--check-ms=<1..60000, default 30000>] [--idle-ms=<1..3600000, default 120000>]
//! ```
//!
//! - `r` and `w` are ends that the starter lets the owner inherit: descriptor numbers on macOS, handle values on
//!   Windows. The owner reads requests from `r` and writes replies to `w`. It marks each end close-on-exec or not
//!   inheritable. `--channel` is the starter's channel; `--worker` is the pipe pair from the worker on which analysis
//!   writes arrive. Without `--worker` the owner serves the starter's channel alone.
//! - The key is exactly 32 raw bytes on standard input, followed by the end of input. The starter closes its end
//!   after it writes them. The owner reads no key from an argument, an environment variable or a file.
//! - Before it reads the key the owner turns core files and crash reports off (`sys::crash_reports_off`).
//! - It opens the store and calls `Store::purge_below` with `--first-live`, the lowest live ordinal of the capture
//!   store, before it reads its first request.
//!
//! # How one process serves its channels and its streams
//!
//! One reader thread for each read end blocks in `read_frame` and hands each frame, its end or its fault to the main
//! thread over one bounded channel. The main thread alone holds the store. It waits on that channel until the next
//! periodic check or the end of the idle time, answers the requests in the order in which they arrive, and writes
//! every report and every reply. The starter's channel admits the role `interface`, the worker's the role `analysis`
//! and a stream the roles `runner` and `coordinator`; a request of another role gets `role` and reaches no handler.
//! A request frame holds at most 65,536 bytes and a reply frame at most 262,144. An analysis write whose `commit` is
//! null is held in memory until the write that carries the commit, because one batch is one or more writes, and a
//! worker channel that ends before its commit leaves nothing.
//!
//! # The streams
//!
//! A stream is a byte stream between the owner and a client of the capture service, on which `runner` and
//! `coordinator_write` requests reach the handlers of the host (`Store::handle`). The starter hands the owner one end
//! of a new stream with the frame `stream` on its channel, a JSON object whose one key is `stream`:
//!
//! ```text
//! {"stream":{"handles":null}}              macOS: the end is one descriptor in one SCM_RIGHTS message beside the frame
//! {"stream":{"handles":[<read>,<write>]}}  Windows: the end is two handle values of the owner's own process
//! ```
//!
//! On macOS the starter's channel is then a socket, and the owner reads it with `recvmsg` and sets close-on-exec on
//! the descriptor. There the starter keeps its own descriptor of that end open until it has read the answer; what
//! the owner gets when the starter closes it earlier stands at `hand_over` in tests/owner.rs. The owner answers the
//! frame with one frame on the starter's channel:
//!
//! ```text
//! {"stream":"taken"}    the owner holds the end and reads it on a reader thread of its own
//! {"stream":"full"}     the owner already holds four streams
//! {"stream":"refused"}  the frame is not as above: on macOS no descriptor or more than one came beside it, on
//!                       Windows the two values are one or a value names no pipe
//! ```
//!
//! With `full` and `refused` the owner closes what came and keeps the streams that it holds; on Windows it closes a
//! value that names a pipe and touches no other value. A descriptor that comes beside any other frame is closed. A
//! stream ends at an end-of-file, between two frames or inside one, at a read or a write error, and at a length
//! above 65,536: the owner writes nothing more on it, closes its end and goes on with its channels and its other
//! streams. The place of a stream is free when its reader thread has ended. After a write error the owner shuts the
//! socket down on macOS, which ends the reader at once; on Windows the handle that reads stays open, with its place,
//! until the far end closes the handle on which it writes.
//!
//! # The report
//!
//! After every committed transaction the store hands the owner its report, and the owner writes it as one frame on
//! the starter's channel before the reply to the request that made it, and once more before it exits:
//!
//! ```text
//! {"report":{"revisions":{"workflow":<n>,..},"counts":{"window":<n>,"bounded":<n>,"kept":<n>},"disk_bytes":<n>,"expiry":<instant>|null,"position":{"store":"<store id>","seg":<n>,"idx":<n>}|null}}
//! ```
//!
//! `revisions` holds the families in the order of the registry. `position` is the store id that the database is bound
//! to and the position up to which batches are committed, from the store's own table `meta`, or null while the
//! database is bound to no store: the report before the first request holds what an earlier owner committed over the
//! same directory, and the report of a committed batch its store id and commit position. A batch at or before the
//! committed position commits nothing and makes no report. Where the row of `meta` is malformed, `position` is null as
//! well: the owner stays up and answers its requests, and writes one line on standard error that names the row and
//! holds no value of it. At every periodic check the owner calls
//! `Store::expire` with no request, and writes the report of that call only when it removed a row.
//!
//! # How the owner ends
//!
//! It exits 0 at the end of the starter's channel, and after the idle time with no frame on a channel or on a stream,
//! which runs only while no worker channel is open. An open stream on which nothing arrives does not keep the owner
//! alive. The end of the worker's channel ends that channel alone, and the end of a stream that stream alone. Every
//! other exit code is one of `exit`. The owner ends no step: the liveness rule of its starter ends a step that does
//! not return.
//!
//! # What may be logged
//!
//! The owner writes to standard error only the faults of the store and of its channels, which hold no value of a
//! record, and the refusals of a step's records, whose text names an index and a rule.

mod agent;
mod learn;
pub mod steps;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix as sys;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as sys;

use std::cell::RefCell;
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::time::{Duration, Instant as Mono, SystemTime, UNIX_EPOCH};

use halvo_derived::{
    Clock, CommittedPosition, Counts, DbKey, Error, IdSource, OpenError, PositionState, Report,
    Store,
};
use halvo_schema_core::ids::StoreId;
use halvo_schema_core::kinds::{self, KindEntry};
use halvo_schema_core::record::{Episode, Instant, Ordinal};
use halvo_schema_core::request::{self, AnalysisWrite, Decoded, ErrorName, ReplyResult, Role};
use halvo_seam::frame::{read_frame, write_frame, CLIENT_FRAME_MAX, SERVER_FRAME_MAX};
use serde::ser::{SerializeMap, SerializeStruct, Serializer};
use serde::Serialize;
use serde_json::Value;

/// The exit codes of the owner.
pub mod exit {
    /// The starter's channel ended, or the idle time passed with no request.
    pub const END: i32 = 0;
    /// The arguments are not of the form of the start, or an end that they name is not open.
    pub const USAGE: i32 = 64;
    /// Standard input does not hold exactly the 32 bytes of the key.
    pub const KEY: i32 = 65;
    /// Any other failure: an open error not named here, a fault of `purge_below`, or the start call.
    pub const SOFTWARE: i32 = 70;
    /// A fault of the starter's channel: a read or a write failed, a frame was cut, or its length is out of bound.
    pub const CHANNEL: i32 = 74;
    /// Another store holds the derived database (`OpenError::Locked`).
    pub const LOCKED: i32 = 75;
    /// The key does not open the derived database (`OpenError::Sealed`).
    pub const SEALED: i32 = 77;
}

/// The default period of the check, which runs the expiry with no request.
const CHECK_DEFAULT: Duration = Duration::from_millis(30_000);
/// The default idle time after which the owner exits.
const IDLE_DEFAULT: Duration = Duration::from_millis(120_000);
/// The key's length in bytes.
const KEY_LEN: usize = 32;
/// The most streams that the owner holds at once.
const STREAMS_MAX: usize = 4;

/// Runs the owner and returns its exit code.
pub fn run() -> i32 {
    if let Err(e) = sys::crash_reports_off() {
        eprintln!("halvo-owner: core files and crash reports cannot be turned off: {e}");
        return exit::SOFTWARE;
    }
    let args = match Args::parse(std::env::args_os().skip(1)) {
        Ok(args) => args,
        Err(e) => {
            eprintln!("halvo-owner: {e}");
            return exit::USAGE;
        }
    };
    let Some(key) = read_key(&mut io::stdin()) else {
        eprintln!("halvo-owner: standard input does not hold exactly the 32 bytes of the key");
        return exit::KEY;
    };
    let ends = match Ends::adopt(&args) {
        Ok(ends) => ends,
        Err(e) => {
            eprintln!("halvo-owner: an end of a channel cannot be taken: {e}");
            return exit::USAGE;
        }
    };
    serve(&args, key, ends)
}

/// The arguments of the start.
struct Args {
    data_dir: PathBuf,
    first_live: Ordinal,
    channel: (u64, u64),
    worker: Option<(u64, u64)>,
    check: Duration,
    idle: Duration,
}

impl Args {
    fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Args, String> {
        let mut data_dir = None;
        let mut first_live = None;
        let mut channel = None;
        let mut worker = None;
        let mut check = None;
        let mut idle = None;
        for arg in args {
            let arg = arg
                .into_string()
                .map_err(|_| "an argument is not UTF-8".to_owned())?;
            let Some((name, value)) = arg.split_once('=') else {
                return Err(format!("{arg}: not of the form --name=value"));
            };
            match name {
                "--data-dir" => once(&mut data_dir, name, PathBuf::from(value))?,
                "--first-live" => {
                    let n = value
                        .parse::<u64>()
                        .ok()
                        .and_then(|n| Ordinal::new(n).ok())
                        .ok_or_else(|| format!("{name}: not an ordinal"))?;
                    once(&mut first_live, name, n)?;
                }
                "--channel" => once(&mut channel, name, pair(name, value)?)?,
                "--worker" => once(&mut worker, name, pair(name, value)?)?,
                "--check-ms" => once(&mut check, name, millis(name, value, 60_000)?)?,
                "--idle-ms" => once(&mut idle, name, millis(name, value, 3_600_000)?)?,
                _ => return Err(format!("{name}: an argument that the owner does not take")),
            }
        }
        let data_dir = data_dir.ok_or("--data-dir is missing")?;
        if !data_dir.is_absolute() {
            return Err("--data-dir is not absolute".to_owned());
        }
        let channel: (u64, u64) = channel.ok_or("--channel is missing")?;
        let mut ends = vec![channel.0, channel.1];
        if let Some((r, w)) = worker {
            ends.extend([r, w]);
        }
        ends.sort_unstable();
        if ends.windows(2).any(|two| two[0] == two[1]) {
            return Err("two ends of the channels are one".to_owned());
        }
        Ok(Args {
            data_dir,
            first_live: first_live.ok_or("--first-live is missing")?,
            channel,
            worker,
            check: check.unwrap_or(CHECK_DEFAULT),
            idle: idle.unwrap_or(IDLE_DEFAULT),
        })
    }
}

fn once<T>(slot: &mut Option<T>, name: &str, value: T) -> Result<(), String> {
    if slot.replace(value).is_some() {
        return Err(format!("{name} is given twice"));
    }
    Ok(())
}

fn pair(name: &str, value: &str) -> Result<(u64, u64), String> {
    let bad = || format!("{name}: not of the form <read end>,<write end>");
    let (r, w) = value.split_once(',').ok_or_else(bad)?;
    Ok((r.parse().map_err(|_| bad())?, w.parse().map_err(|_| bad())?))
}

fn millis(name: &str, value: &str, max: u64) -> Result<Duration, String> {
    match value.parse::<u64>() {
        Ok(n) if (1..=max).contains(&n) => Ok(Duration::from_millis(n)),
        _ => Err(format!(
            "{name}: not a count of milliseconds from 1 to {max}"
        )),
    }
}

/// Reads the key: exactly 32 bytes and then the end of input, or none. Each read asks for more bytes than the buffer
/// of standard input holds, so that the reader passes its own buffer by and the key's bytes stand in no buffer but
/// the one that this function zeroes.
fn read_key(input: &mut dyn Read) -> Option<DbKey> {
    let mut got = [0u8; KEY_LEN];
    let mut len = 0;
    let mut chunk = vec![0u8; 65_536];
    let mut fits = true;
    loop {
        match input.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if len + n > KEY_LEN {
                    fits = false;
                } else {
                    got[len..len + n].copy_from_slice(&chunk[..n]);
                    len += n;
                }
                chunk[..n].fill(0);
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => {
                fits = false;
                break;
            }
        }
    }
    let key = (fits && len == KEY_LEN).then(|| DbKey::new(got));
    got.fill(0);
    std::hint::black_box(&got);
    std::hint::black_box(&chunk);
    key
}

/// The ends of the two channels, taken from the start.
struct Ends {
    starter: (File, File),
    worker: Option<(File, File)>,
}

impl Ends {
    fn adopt(args: &Args) -> io::Result<Ends> {
        let starter = (sys::adopt(args.channel.0)?, sys::adopt(args.channel.1)?);
        let worker = match args.worker {
            Some((r, w)) => Some((sys::adopt(r)?, sys::adopt(w)?)),
            None => None,
        };
        Ok(Ends { starter, worker })
    }
}

/// The channel or the stream that a frame came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    Starter,
    Worker,
    /// A stream, by the number that the owner gave it when it took its end. No two streams of one owner have one
    /// number, so what the reader thread of an ended stream hands over is never read as a later stream's.
    Stream(u64),
}

impl Source {
    /// Whether the source admits a request of `role`: the starter's channel the role `interface`, the worker's the
    /// role `analysis`, and a stream the roles `runner` and `coordinator`. A request of a further role has none of
    /// the four, and no source admits it.
    fn admits(self, role: Option<Role>) -> bool {
        match self {
            Source::Starter => role == Some(Role::Interface),
            Source::Worker => role == Some(Role::Analysis),
            Source::Stream(_) => matches!(role, Some(Role::Runner | Role::Coordinator)),
        }
    }
}

/// What a reader thread hands the main thread.
enum Event {
    /// A frame, with the ends that came beside its bytes.
    Frame(Source, Vec<u8>, Vec<File>),
    End(Source),
    Fault(Source),
}

/// A read end of a channel or of a stream.
trait Inbound: Read + Send + 'static {
    /// The ends that came beside the bytes read since the last call. Nothing comes beside the bytes of a pipe or of
    /// a stream; `sys::starter_end` gives the read end beside whose bytes an end of a stream can come.
    fn came(&mut self) -> Vec<File> {
        Vec::new()
    }
}

impl Inbound for File {}

/// Starts the reader thread of `end`. What came beside a frame goes to the main thread with it; what came beside
/// bytes that make no frame is closed.
fn spawn_reader(source: Source, mut end: impl Inbound, tx: SyncSender<Event>) -> io::Result<()> {
    std::thread::Builder::new()
        .name(format!("{source:?} reader"))
        .spawn(move || loop {
            let read = read_frame(&mut end, CLIENT_FRAME_MAX);
            let came = end.came();
            let event = match read {
                Ok(Some(bytes)) => Event::Frame(source, bytes, came),
                Ok(None) => Event::End(source),
                Err(_) => Event::Fault(source),
            };
            let last = !matches!(event, Event::Frame(..));
            if tx.send(event).is_err() || last {
                return;
            }
        })
        .map(|_| ())
}

/// What the `handles` of a frame `stream` hold.
enum Handles {
    /// `null`: the end comes beside the frame, as on macOS.
    Null,
    /// Two handle values of the owner's own process, the one that reads and then the one that writes, as on Windows.
    Two(u64, u64),
    /// The frame is not `{"stream":{"handles":..}}` with one of the two values above.
    Other,
}

/// Reads a frame of the starter's channel as a frame `stream`: a JSON object whose one key is `stream`. Any other
/// frame is a request, and this function returns none for it.
fn stream_frame(bytes: &[u8]) -> Option<Handles> {
    let frame: Value = serde_json::from_slice(bytes).ok()?;
    let stream = frame
        .as_object()
        .filter(|keys| keys.len() == 1)?
        .get("stream")?;
    let handles = stream
        .as_object()
        .filter(|keys| keys.len() == 1)
        .and_then(|keys| keys.get("handles"));
    Some(match handles {
        Some(Value::Null) => Handles::Null,
        Some(Value::Array(values)) => match values.as_slice() {
            [reads, writes] => match (reads.as_u64(), writes.as_u64()) {
                (Some(reads), Some(writes)) => Handles::Two(reads, writes),
                _ => Handles::Other,
            },
            _ => Handles::Other,
        },
        _ => Handles::Other,
    })
}

/// A stream that the owner holds. Its place is taken from the answer `taken` until its reader thread has ended.
struct Stream {
    id: u64,
    /// The end on which the owner writes, or none once a write on it has failed.
    out: Option<File>,
}

/// The machine's clock and its local calendar.
struct SystemClock;

const SECOND_NS: i64 = 1_000_000_000;
const DAY_S: i64 = 86_400;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        let ns = |d: Duration| i64::try_from(d.as_nanos()).unwrap_or(i64::MAX);
        Instant(match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(after) => ns(after),
            Err(before) => -ns(before.duration()),
        })
    }

    fn day(&self, at: Instant) -> u32 {
        let day = local_day(at.0.div_euclid(SECOND_NS));
        u32::try_from(day.max(0)).unwrap_or(u32::MAX)
    }

    fn day_start(&self, day: u32) -> Instant {
        let day = i64::from(day);
        let (mut before, mut from) = ((day - 2) * DAY_S, (day + 2) * DAY_S);
        while from - before > 1 {
            let mid = before + (from - before) / 2;
            if local_day(mid) >= day {
                from = mid;
            } else {
                before = mid;
            }
        }
        Instant(from.saturating_mul(SECOND_NS))
    }
}

/// The local calendar day of a second since 1970-01-01, counted in days from 1970-01-01, or its UTC day where the
/// system gives no local date for it.
fn local_day(secs: i64) -> i64 {
    match sys::local_date(secs) {
        Some((year, month, day)) => days_from_civil(year, month, day),
        None => secs.div_euclid(DAY_S),
    }
}

/// The days from 1970-01-01 to a date of the proleptic Gregorian calendar.
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let (month, day) = (i64::from(month), i64::from(day));
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The operating system's random bytes.
struct OsIds;

impl IdSource for OsIds {
    fn fill(&mut self, bytes: &mut [u8]) {
        if let Err(e) = getrandom::fill(bytes) {
            eprintln!("halvo-owner: the system gives no random bytes: {e}");
            std::process::exit(exit::SOFTWARE);
        }
    }
}

/// The episodes of a batch whose commit has not arrived.
struct Pending {
    store: StoreId,
    episodes: Vec<Episode>,
}

fn serve(args: &Args, key: DbKey, ends: Ends) -> i32 {
    let reports: Rc<RefCell<Vec<Report>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&reports);
    let host = steps::list()
        .host(Box::new(SystemClock))
        .on_report(Box::new(move |report: &Report| {
            sink.borrow_mut().push(report.clone())
        }));
    let entries: Vec<&'static KindEntry> = kinds::registry().collect();
    let opened = Store::open(&args.data_dir, &key, &entries, host, &mut OsIds);
    drop(key);
    let store = match opened {
        Ok(store) => store,
        Err(e) => {
            eprintln!("halvo-owner: the derived store does not open: {e}");
            return match e {
                OpenError::Sealed => exit::SEALED,
                OpenError::Locked => exit::LOCKED,
                _ => exit::SOFTWARE,
            };
        }
    };
    let (starter_in, starter_out) = ends.starter;
    let mut owner = Owner {
        store,
        reports,
        starter: starter_out,
        worker: None,
        pending: None,
        streams: Vec::new(),
        next_stream: 0,
        told_malformed: false,
    };
    if let Err(fault) = owner.store.purge_below(args.first_live) {
        eprintln!("halvo-owner: the purge below the lowest live ordinal failed: {fault}");
        return owner.finish(exit::SOFTWARE);
    }
    if let Err(code) = owner.flush_reports() {
        return code;
    }
    let (tx, rx) = mpsc::sync_channel(2);
    let mut readers = spawn_reader(Source::Starter, sys::starter_end(starter_in), tx.clone());
    if let Some((worker_in, worker_out)) = ends.worker {
        readers = readers.and_then(|()| spawn_reader(Source::Worker, worker_in, tx.clone()));
        owner.worker = Some(worker_out);
    }
    if let Err(e) = readers {
        eprintln!("halvo-owner: a reader thread does not start: {e}");
        return owner.finish(exit::SOFTWARE);
    }
    owner.serve(&rx, &tx, args.check, args.idle)
}

/// The open store with what the main thread holds beside it.
struct Owner {
    store: Store,
    reports: Rc<RefCell<Vec<Report>>>,
    starter: File,
    worker: Option<File>,
    pending: Option<Pending>,
    /// The streams that the owner holds, at most `STREAMS_MAX`.
    streams: Vec<Stream>,
    /// The number of the next stream.
    next_stream: u64,
    /// Whether the line `MALFORMED_META` stands on standard error: the owner writes it once, at the first report
    /// that finds the row of `meta` malformed.
    told_malformed: bool,
}

/// The one line on standard error for a malformed row of the store's table `meta`. It holds no value of the row.
const MALFORMED_META: &str =
    "halvo-owner: the row of the derived store's table meta is malformed; each report holds position null";

impl Owner {
    /// Serves the channels and the streams until the owner ends. `tx` is the sender that the reader thread of each
    /// new stream gets.
    fn serve(
        &mut self,
        rx: &Receiver<Event>,
        tx: &SyncSender<Event>,
        check: Duration,
        idle: Duration,
    ) -> i32 {
        let mut quiet_since = Mono::now();
        let mut next_check = quiet_since + check;
        loop {
            let now = Mono::now();
            if now >= next_check {
                if let Err(code) = self.check() {
                    return self.finish(code);
                }
                next_check = Mono::now() + check;
                continue;
            }
            let idle_end = self.worker.is_none().then(|| quiet_since + idle);
            if idle_end.is_some_and(|end| now >= end) {
                return self.finish(exit::END);
            }
            let wake = idle_end.map_or(next_check, |end| end.min(next_check));
            match rx.recv_timeout(wake.saturating_duration_since(now)) {
                Ok(Event::Frame(Source::Worker, ..)) if self.worker.is_none() => {}
                Ok(Event::Frame(Source::Stream(id), ..)) if !self.writes_on(id) => {}
                Ok(Event::Frame(source, bytes, came)) => {
                    quiet_since = Mono::now();
                    let handles = match source {
                        Source::Starter => stream_frame(&bytes),
                        Source::Worker | Source::Stream(_) => None,
                    };
                    let done = match handles {
                        Some(handles) => self.take_stream(handles, came, tx),
                        None => {
                            drop(came);
                            self.answer(source, &bytes)
                        }
                    };
                    if let Err(code) = done {
                        return self.finish(code);
                    }
                }
                Ok(Event::End(Source::Starter)) => return self.finish(exit::END),
                Ok(Event::Fault(Source::Starter)) => {
                    eprintln!("halvo-owner: the starter's channel failed");
                    return self.finish(exit::CHANNEL);
                }
                Ok(Event::End(Source::Worker)) => {
                    self.close_worker();
                    quiet_since = Mono::now();
                }
                Ok(Event::Fault(Source::Worker)) => {
                    eprintln!("halvo-owner: the worker's channel failed; the owner closes it");
                    self.close_worker();
                    quiet_since = Mono::now();
                }
                Ok(Event::End(Source::Stream(id))) => self.free_stream(id),
                Ok(Event::Fault(Source::Stream(id))) => {
                    eprintln!("halvo-owner: a stream failed; the owner closes it");
                    self.free_stream(id);
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return self.finish(exit::END),
            }
        }
    }

    /// The periodic check: the expiry, with no request. Its report is written only when it removed a row.
    fn check(&mut self) -> Result<(), i32> {
        match self.store.expire() {
            Ok(0) => {
                self.reports.borrow_mut().clear();
                Ok(())
            }
            Ok(_) => self.flush_reports(),
            Err(fault) => {
                eprintln!("halvo-owner: the periodic expiry failed: {fault}");
                self.flush_reports()
            }
        }
    }

    /// Answers one request frame: the reports of what it committed first, on the starter's channel, then its reply on
    /// the channel or the stream that it came from. A reply that cannot be written on a stream ends that stream alone.
    fn answer(&mut self, source: Source, bytes: &[u8]) -> Result<(), i32> {
        let reply = self.reply(source, bytes);
        self.flush_reports()?;
        match source {
            Source::Starter => {
                write_frame(&mut self.starter, &reply, SERVER_FRAME_MAX).map_err(|_| {
                    eprintln!("halvo-owner: a reply cannot be written on the starter's channel");
                    exit::CHANNEL
                })
            }
            Source::Worker => {
                let written = match self.worker.as_mut() {
                    Some(out) => write_frame(out, &reply, SERVER_FRAME_MAX).is_ok(),
                    None => true,
                };
                if !written {
                    eprintln!("halvo-owner: a reply cannot be written on the worker's channel; the owner closes it");
                    self.close_worker();
                }
                Ok(())
            }
            Source::Stream(id) => {
                let out = self.streams.iter_mut().find(|stream| stream.id == id);
                let written = match out.and_then(|stream| stream.out.as_mut()) {
                    Some(out) => write_frame(out, &reply, SERVER_FRAME_MAX).is_ok(),
                    None => true,
                };
                if !written {
                    eprintln!(
                        "halvo-owner: a reply cannot be written on a stream; the owner closes it"
                    );
                    self.end_stream(id);
                }
                Ok(())
            }
        }
    }

    /// Answers a frame `stream` with one frame on the starter's channel: `taken` when the owner holds the end that
    /// came and has started its reader thread, `full` when it already holds `STREAMS_MAX` streams, and `refused`
    /// when the frame does not give one end as `sys::stream_end` reads it. With `full` and `refused` what came is
    /// closed before the answer is written, and the streams that the owner holds stay.
    fn take_stream(
        &mut self,
        handles: Handles,
        came: Vec<File>,
        tx: &SyncSender<Event>,
    ) -> Result<(), i32> {
        let end = match handles {
            Handles::Null => sys::stream_end(None, came),
            Handles::Two(reads, writes) => sys::stream_end(Some((reads, writes)), came),
            Handles::Other => {
                drop(came);
                None
            }
        };
        let answer = match end {
            None => "refused",
            Some(end) if self.streams.len() >= STREAMS_MAX => {
                drop(end);
                "full"
            }
            Some((reads, writes)) => {
                let id = self.next_stream;
                self.next_stream += 1;
                match spawn_reader(Source::Stream(id), reads, tx.clone()) {
                    Ok(()) => {
                        self.streams.push(Stream {
                            id,
                            out: Some(writes),
                        });
                        "taken"
                    }
                    Err(e) => {
                        eprintln!("halvo-owner: the reader thread of a stream does not start: {e}");
                        drop(writes);
                        "refused"
                    }
                }
            }
        };
        let frame = format!(r#"{{"stream":"{answer}"}}"#);
        write_frame(&mut self.starter, frame.as_bytes(), SERVER_FRAME_MAX).map_err(|_| {
            eprintln!("halvo-owner: an answer cannot be written on the starter's channel");
            exit::CHANNEL
        })
    }

    /// Whether the owner still writes on the stream `id`: it holds the stream and no write on it has failed.
    fn writes_on(&self, id: u64) -> bool {
        self.streams
            .iter()
            .any(|stream| stream.id == id && stream.out.is_some())
    }

    /// Ends a stream on which a write failed: the owner writes nothing more on it, answers no further frame of it
    /// and closes the end on which it writes. `sys::shut` ends its reader thread where the system can, and the
    /// place is free when that thread has ended.
    fn end_stream(&mut self, id: u64) {
        let stream = self.streams.iter_mut().find(|stream| stream.id == id);
        if let Some(out) = stream.and_then(|stream| stream.out.take()) {
            sys::shut(&out);
        }
    }

    /// Frees the place of a stream whose reader thread has ended, and closes the end on which the owner writes.
    fn free_stream(&mut self, id: u64) {
        self.streams.retain(|stream| stream.id != id);
    }

    fn reply(&mut self, source: Source, bytes: &[u8]) -> Vec<u8> {
        let (id, outcome) = match request::decode(bytes) {
            Err(e) => (e.id, Err(e.error)),
            Ok(decoded) => {
                let id = decoded.id().clone();
                let outcome = if source.admits(decoded.role()) {
                    self.request(decoded)
                } else {
                    Err(ErrorName::Role)
                };
                (Some(id), outcome)
            }
        };
        let written = match &outcome {
            Ok(result) => request::write_reply(id.as_ref(), Ok(result)),
            Err(name) => request::write_reply(id.as_ref(), Err(*name)),
        };
        let reply = written.or_else(|name| request::write_reply(id.as_ref(), Err(name)));
        match reply {
            Ok(reply) if reply.len() <= SERVER_FRAME_MAX => reply,
            _ => request::write_reply(id.as_ref(), Err(ErrorName::Internal)).unwrap_or_default(),
        }
    }

    fn request(&mut self, decoded: Decoded) -> Result<ReplyResult, ErrorName> {
        match decoded {
            Decoded::AnalysisWrite(write) => self.analysis(&write),
            Decoded::InterfaceRead(read) => named(self.store.interface_read(&read)),
            Decoded::InterfaceDecision(decision) => {
                named(self.store.decide(&decision)).map(|()| ReplyResult::Null)
            }
            Decoded::Raw(raw) => named(self.store.handle(&raw)).map(ReplyResult::Raw),
        }
    }

    /// Holds the episodes of a write whose commit is null, and commits the batch at the write that carries it.
    fn analysis(&mut self, write: &AnalysisWrite) -> Result<ReplyResult, ErrorName> {
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.store != *write.store())
        {
            return Err(ErrorName::Store);
        }
        let pending = self.pending.get_or_insert_with(|| Pending {
            store: write.store().clone(),
            episodes: Vec::new(),
        });
        pending.episodes.extend_from_slice(write.episodes());
        let Some(position) = write.commit_position() else {
            return Ok(ReplyResult::Null);
        };
        let Some(batch) = self.pending.take() else {
            return Err(ErrorName::Internal);
        };
        let outcome =
            named(
                self.store
                    .commit_batch(&batch.store, &batch.episodes, position, &mut OsIds),
            )?;
        for (at, step) in outcome.steps.iter().enumerate() {
            if let Err(refusal) = step {
                eprintln!("halvo-owner: the records of step {at} are refused: {refusal}");
            }
        }
        Ok(ReplyResult::Null)
    }

    /// Ends the worker's channel: its write end closes and a batch without its commit is dropped.
    fn close_worker(&mut self) {
        self.worker = None;
        self.pending = None;
    }

    /// Writes every report that the store handed over since the last call, in order, on the starter's channel.
    fn flush_reports(&mut self) -> Result<(), i32> {
        let reports = std::mem::take(&mut *self.reports.borrow_mut());
        for report in &reports {
            if report.position == PositionState::Malformed && !self.told_malformed {
                self.told_malformed = true;
                eprintln!("{MALFORMED_META}");
            }
            if write_frame(&mut self.starter, &report_frame(report), SERVER_FRAME_MAX).is_err() {
                eprintln!("halvo-owner: a report cannot be written on the starter's channel");
                return Err(exit::CHANNEL);
            }
        }
        Ok(())
    }

    /// Writes the last report and returns `code`. A report that cannot be written changes no code.
    fn finish(&mut self, code: i32) -> i32 {
        match self.store.report() {
            Ok(report) => self.reports.borrow_mut().push(report),
            Err(fault) => eprintln!("halvo-owner: the last report cannot be read: {fault}"),
        }
        let _ = self.flush_reports();
        code
    }
}

/// The error name of a failed call of the store; a fault is answered `internal` and written to standard error.
fn named<T>(result: Result<T, Error>) -> Result<T, ErrorName> {
    result.map_err(|e| match e {
        Error::Refused(name) => name,
        Error::Fault(fault) => {
            eprintln!("halvo-owner: a fault of the store: {fault}");
            ErrorName::Internal
        }
    })
}

/// The bytes of the report's frame (contracts C24.9), compact, with its keys in the contract's order.
pub fn report_frame(report: &Report) -> Vec<u8> {
    serde_json::to_vec(&Frame(report)).unwrap_or_default()
}

struct Frame<'a>(&'a Report);

impl Serialize for Frame<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry("report", &Body(self.0))?;
        map.end()
    }
}

struct Body<'a>(&'a Report);

impl Serialize for Body<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut body = serializer.serialize_struct("report", 5)?;
        body.serialize_field("revisions", &Revisions(&self.0.revisions))?;
        body.serialize_field("counts", &CountsOf(&self.0.counts))?;
        body.serialize_field("disk_bytes", &self.0.disk_bytes)?;
        body.serialize_field("expiry", &self.0.expiry.map(|at| at.0))?;
        let position = match &self.0.position {
            PositionState::Committed(committed) => Some(PositionOf(committed)),
            PositionState::Unbound | PositionState::Malformed => None,
        };
        body.serialize_field("position", &position)?;
        body.end()
    }
}

struct PositionOf<'a>(&'a CommittedPosition);

impl Serialize for PositionOf<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut position = serializer.serialize_struct("position", 3)?;
        position.serialize_field("store", self.0.store.as_str())?;
        position.serialize_field("seg", &self.0.position.seg)?;
        position.serialize_field("idx", &self.0.position.idx)?;
        position.end()
    }
}

struct Revisions<'a>(&'a [(&'static str, u64)]);

impl Serialize for Revisions<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (family, revision) in self.0 {
            map.serialize_entry(family, revision)?;
        }
        map.end()
    }
}

struct CountsOf<'a>(&'a Counts);

impl Serialize for CountsOf<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut counts = serializer.serialize_struct("counts", 3)?;
        counts.serialize_field("window", &self.0.window)?;
        counts.serialize_field("bounded", &self.0.bounded)?;
        counts.serialize_field("kept", &self.0.kept)?;
        counts.end()
    }
}
