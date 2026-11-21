//! The worker slot (contracts.md C23.5, C23.11 and C24.5 of the workflows run): the one place of the daemon that
//! starts `halvo-analysis`, the analysis worker, and ends it. The slot reads no signal of the machine: its caller
//! says when a worker may run and from which position.
//!
//! - A start runs `halvo-analysis --read=<r>,<w> --owner=<r>,<w>`, with `--store` and `--position` when the caller
//!   names a committed position, from the directory of the daemon's programs (`children::beside`), under the cap of
//!   [`CAP_BYTES`] bytes through halvo-cap. The worker holds its standard streams, its four ends and no other
//!   descriptor of the daemon; on Windows it runs in the Job of the cap and is created with the list of exactly the
//!   handles that it inherits, so it holds no end of the owner's channel.
//! - The read channel is one stream, a socket pair on macOS and two pipes on Windows. The cursor serves its other
//!   end on a thread of the slot (`cursor().serve`). The owner's channel is a pipe pair whose other ends the owner
//!   gets from a start of its own ([`OwnerSlot::start_with_worker`]): each start of a worker stops the owner that
//!   runs and starts one with the pair.
//! - One worker lives at a time: while one lives, a start starts nothing.
//! - The committed position is the one that the owner's report frame holds in its key `position`. Its null is the
//!   zero position. A position of another store than the capture store starts no worker, and the slot writes one
//!   line for it.
//! - [`WorkerSlot::end`] kills the worker, on Windows through the Job, because the worker takes no request to end.
//!   It waits at most 2 s for the reaping, closes the cursor (`Cursor::drop_open`), closes the slot's side of the
//!   stream and joins the serving thread.
//! - [`WorkerSlot::pass`] gets the clock's reading and the caller's answer: on no it ends the worker, on yes it
//!   starts one when none lives and the rule of the ends allows it.
//! - [`WorkerSlot::delete`], and the drop of the slot, end the worker, then close the cursor, then stop the owner.
//!   While the delete of everything runs (`Caller::deleting`), nothing starts.
//! - The rule of the ends: an end that the slot made counts for nothing. Every other end counts at the position
//!   that its worker was started from, a position of one capture store: after the first the next start at that
//!   position waits 1 min, after the second 10 min, and after the third none starts at that position in this life
//!   of the daemon. After an exit 0 the caller names a new committed position, where nothing is counted, so the
//!   next start follows at once; an exit 0 that leaves the position where it was waits as every other end does. So
//!   the caller says yes only while the capture store holds a record after the committed position: three workers
//!   that find nothing to read end the starts at that position. A start that fails, of the owner or of the worker,
//!   counts as an end, and the slot writes one line for it:
//!
//!   ```text
//!   halvo-capture: analysis start=failed what=<stream|owner|cap|worker|thread> code=<n or ->
//!   ```
//!
//!   `what` names the step that failed: the ends of the two channels, the owner's start, the cap, the worker's
//!   program, or a thread of the slot. `code` is the system's error number where the step has one.
//! - For each end of a worker the slot writes one line:
//!
//!   ```text
//!   halvo-capture: analysis end=<committed|cap|code|signal|ended> code=<n or -> peak_bytes=<n>
//!   ```
//!
//!   `committed` is an exit 0, `ended` an end that the slot made, `cap` an end by the cap or a fault of the cap,
//!   `code` any other exit code and `signal` a signal. `code` holds the exit code where the worker exited with one
//!   and the slot did not end it. `peak_bytes` is the cap's reading. No line holds a value of a record.

use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use halvo_cap::{CapError, Ender, EndsSpec, Exit, Limit, Outcome};
use halvo_schema::{Position, StoreId};

use crate::daemon::children::beside;
use crate::daemon::r2::cursor::cursor;
use crate::daemon::r2::owner_slot::{Caller, OwnerSlot, Place};

#[cfg(target_os = "macos")]
mod stream_macos;
#[cfg(target_os = "macos")]
use stream_macos as sys;
#[cfg(windows)]
mod stream_windows;
#[cfg(windows)]
use stream_windows as sys;

/// The program that the slot starts.
pub const PROGRAM: &str = "halvo-analysis";

/// The cap of a worker in bytes, W: 256 MiB.
pub const CAP_BYTES: u64 = 268_435_456;

/// How long an end waits for the reaping of the worker, and then for the end of the serving thread.
pub const REAP_WAIT: Duration = Duration::from_secs(2);

/// How long the next start at a position waits after the first end that counts there, and after the second. After
/// the third none starts at that position.
pub const START_WAITS: [Duration; 2] = [Duration::from_secs(60), Duration::from_secs(600)];

/// The step of a wait for a thread.
const POLL: Duration = Duration::from_millis(10);

/// The line of a committed position that names another store than the capture store.
const OTHER_STORE_LINE: &str =
    "halvo-capture: analysis is not started: the committed position names another store than the capture store";

/// A committed position, as the owner's report frame holds it in its key `position`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Committed {
    /// The store that the derived store is bound to.
    pub store: StoreId,
    /// The position up to which batches are committed.
    pub position: Position,
}

/// Where a worker reads from, as the slot's caller names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    /// The id of the capture store, whose records the read channel serves.
    pub capture: StoreId,
    /// The committed position; none for the report's null, the zero position.
    pub committed: Option<Committed>,
}

/// What a call of the slot did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// The call started a worker.
    Started,
    /// A worker lives, and the call started nothing.
    Lives,
    /// The caller's answer was no: no worker lives.
    Idle,
    /// The delete of everything runs, and nothing starts.
    Deleting,
    /// The committed position names another store than the capture store, and nothing starts.
    OtherStore,
    /// The wait after an end at this position has not passed.
    Waits,
    /// Three ends counted at this position: none starts there in this life of the daemon.
    GivenUp,
    /// The owner or the worker did not start.
    NotStarted,
}

/// How a worker ended, as the line of its end names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EndKind {
    Committed,
    Cap,
    Code,
    Signal,
    Ended,
}

impl EndKind {
    fn word(self) -> &'static str {
        match self {
            EndKind::Committed => "committed",
            EndKind::Cap => "cap",
            EndKind::Code => "code",
            EndKind::Signal => "signal",
            EndKind::Ended => "ended",
        }
    }
}

/// The kind of an end and its line. `reached` says that a kill of the slot reached the worker before it was seen
/// ended; a worker that had exited 0 in that moment keeps its exit.
fn end_of(outcome: &Result<Outcome, CapError>, reached: bool) -> (EndKind, String) {
    let (exit, peak_bytes, capped) = match outcome {
        Ok(outcome) => (Some(outcome.exit), outcome.peak_bytes, outcome.capped),
        Err(_) => (None, 0, true),
    };
    let kind = match exit {
        Some(Exit::Code(0)) => EndKind::Committed,
        _ if reached => EndKind::Ended,
        _ if capped => EndKind::Cap,
        Some(Exit::Signal(_)) => EndKind::Signal,
        Some(Exit::Code(_) | Exit::NotStarted) | None => EndKind::Code,
    };
    let code = match exit {
        Some(Exit::Code(code)) if kind != EndKind::Ended => code.to_string(),
        _ => "-".to_owned(),
    };
    let line = format!(
        "halvo-capture: analysis end={} code={code} peak_bytes={peak_bytes}",
        kind.word()
    );
    (kind, line)
}

/// Why a start failed: the step, and the system's error number where the step has one.
struct StartFault {
    what: &'static str,
    code: Option<i32>,
}

impl StartFault {
    fn of(what: &'static str, error: &io::Error) -> StartFault {
        StartFault {
            what,
            code: error.raw_os_error(),
        }
    }

    fn line(&self) -> String {
        let code = self.code.map_or("-".to_owned(), |code| code.to_string());
        format!(
            "halvo-capture: analysis start=failed what={} code={code}",
            self.what
        )
    }
}

/// The writer of the slot's lines, which the slot and the thread that waits for a worker share.
#[derive(Clone)]
struct Log(Arc<Mutex<Box<dyn Write + Send>>>);

impl Log {
    /// Writes one line; a line that cannot be written is dropped.
    fn line(&self, line: &str) {
        let mut writer = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = writeln!(writer, "{line}");
        let _ = writer.flush();
    }
}

/// The ends that count at one position.
struct Counted {
    /// The capture store and the committed position that the workers were started from.
    at: Origin,
    /// How many ends counted there, one or more.
    count: usize,
    /// The clock's reading at which the slot saw the last of them.
    last: Instant,
}

/// The worker that lives, with the two threads of the slot that serve it.
struct Living {
    pid: u32,
    /// The capture store and the committed position that it was started from.
    from: Origin,
    ender: Ender,
    /// The kind of its end, from the thread that waits for it, after the line of the end is written.
    ends: Receiver<EndKind>,
    waiting: JoinHandle<()>,
    serving: JoinHandle<()>,
    closer: sys::Closer,
}

impl Living {
    /// After the worker's end: closes the cursor and the slot's side of the stream and joins the serving thread.
    /// The thread that waits for the worker is joined when `reaped` says that it handed the end over.
    fn clear(self, reaped: bool) {
        cursor().drop_open();
        self.closer.close();
        join_within(self.serving, REAP_WAIT);
        join_within(
            self.waiting,
            if reaped { REAP_WAIT } else { Duration::ZERO },
        );
    }
}

/// Joins `thread` when it ends within `wait`, and leaves it to end on its own otherwise.
fn join_within(thread: JoinHandle<()>, wait: Duration) {
    let until = Instant::now() + wait;
    while !thread.is_finished() {
        if Instant::now() >= until {
            return;
        }
        std::thread::sleep(POLL);
    }
    let _ = thread.join();
}

/// The one worker of the analysis that the daemon starts, with the owner slot whose owner it is started beside.
pub struct WorkerSlot {
    program: PathBuf,
    owner: OwnerSlot,
    caller: Arc<dyn Caller>,
    log: Log,
    living: Option<Living>,
    counted: Option<Counted>,
    /// The line of a position of another store is written, and no start has named a position of the capture store
    /// since.
    told_other_store: bool,
}

impl WorkerSlot {
    /// Makes the slot over `place`, with the owner slot that it holds, and with `log` as the writer of its lines.
    /// Nothing starts here.
    pub fn new(place: Place, caller: Arc<dyn Caller>, log: Box<dyn Write + Send>) -> WorkerSlot {
        WorkerSlot {
            program: beside(&place.program_dir, PROGRAM),
            owner: OwnerSlot::new(place, Arc::clone(&caller)),
            caller,
            log: Log(Arc::new(Mutex::new(log))),
            living: None,
            counted: None,
            told_other_store: false,
        }
    }

    /// The owner slot. A worker's owner is the one that the start of the worker started.
    pub fn owner(&mut self) -> &mut OwnerSlot {
        &mut self.owner
    }

    /// The pid of the worker that the slot started and has not seen ended.
    pub fn pid(&self) -> Option<u32> {
        self.living.as_ref().map(|living| living.pid)
    }

    /// One pass of the slot's caller, with the clock's reading `now`: on `may_run` it starts a worker as
    /// [`WorkerSlot::start`] does, and otherwise it ends the worker that lives.
    pub fn pass(&mut self, now: Instant, may_run: bool, origin: &Origin) -> Answer {
        if may_run {
            return self.start(now, origin);
        }
        self.settle(now);
        self.end();
        Answer::Idle
    }

    /// Starts a worker that reads from `origin`, unless one lives, the delete of everything runs, the committed
    /// position names another store, or the rule of the ends holds the start back at the clock's reading `now`.
    pub fn start(&mut self, now: Instant, origin: &Origin) -> Answer {
        self.settle(now);
        if self.living.is_some() {
            return Answer::Lives;
        }
        if self.caller.deleting() {
            return Answer::Deleting;
        }
        if origin
            .committed
            .as_ref()
            .is_some_and(|committed| committed.store != origin.capture)
        {
            if !self.told_other_store {
                self.log.line(OTHER_STORE_LINE);
                self.told_other_store = true;
            }
            return Answer::OtherStore;
        }
        self.told_other_store = false;
        if let Some(counted) = self
            .counted
            .as_ref()
            .filter(|counted| counted.at == *origin)
        {
            match START_WAITS.get(counted.count.saturating_sub(1)) {
                Some(wait) if now.saturating_duration_since(counted.last) >= *wait => {}
                Some(_) => return Answer::Waits,
                None => return Answer::GivenUp,
            }
        }
        match self.launch(origin) {
            Ok(living) => {
                self.living = Some(living);
                Answer::Started
            }
            Err(fault) => {
                self.log.line(&fault.line());
                self.count(now, origin.clone());
                Answer::NotStarted
            }
        }
    }

    /// Ends the worker that lives: kills it, waits at most 2 s for its reaping, closes the cursor and the slot's side
    /// of the stream and joins the serving thread. The end counts for nothing.
    pub fn end(&mut self) {
        let Some(living) = self.living.take() else {
            return;
        };
        living.ender.end();
        let reaped = living.ends.recv_timeout(REAP_WAIT).is_ok();
        living.clear(reaped);
    }

    /// The slot's part of a delete of everything: ends the worker, then closes the cursor, then stops the owner.
    pub fn delete(&mut self) {
        self.end();
        cursor().drop_open();
        self.owner.stop();
    }

    /// Takes the end of a worker that ended on its own: the slot's side is cleared, and the end counts at the
    /// position that the worker was started from.
    fn settle(&mut self, now: Instant) {
        let ended = match self.living.as_ref().map(|living| living.ends.try_recv()) {
            None | Some(Err(mpsc::TryRecvError::Empty)) => return,
            Some(Ok(kind)) => Some(kind),
            Some(Err(mpsc::TryRecvError::Disconnected)) => None,
        };
        let Some(living) = self.living.take() else {
            return;
        };
        let from = living.from.clone();
        living.clear(true);
        if ended != Some(EndKind::Ended) {
            self.count(now, from);
        }
    }

    /// Counts one end at the position `at`.
    fn count(&mut self, now: Instant, at: Origin) {
        let before = self
            .counted
            .as_ref()
            .filter(|counted| counted.at == at)
            .map_or(0, |counted| counted.count);
        self.counted = Some(Counted {
            at,
            count: before + 1,
            last: now,
        });
    }

    /// Starts the owner with its ends of a new pair, then the worker with its four ends, then the thread that serves
    /// the read channel and the thread that waits for the worker. The fault when one of them did not start; nothing
    /// of the worker is left then.
    fn launch(&mut self, origin: &Origin) -> Result<Living, StartFault> {
        let ends = |error: io::Error| StartFault::of("stream", &error);
        let stream = sys::stream().map_err(ends)?;
        let (owner_reads, worker_writes) = io::pipe().map_err(ends)?;
        let (worker_reads, owner_writes) = io::pipe().map_err(ends)?;
        self.owner
            .start_with_worker((owner_reads, owner_writes))
            .map_err(|_| StartFault {
                what: "owner",
                code: None,
            })?;
        let mut spec = EndsSpec::new(self.program.clone());
        let (read, write) = stream.worker;
        let read = (spec.inherit(read), spec.inherit(write));
        let owner = (spec.inherit(worker_reads), spec.inherit(worker_writes));
        spec.args
            .push(format!("--read={},{}", read.0, read.1).into());
        spec.args
            .push(format!("--owner={},{}", owner.0, owner.1).into());
        if let Some(committed) = &origin.committed {
            spec.args
                .push(format!("--store={}", committed.store.as_str()).into());
            let Position { seg, idx } = committed.position;
            spec.args.push(format!("--position={seg},{idx}").into());
        }
        let cap = |code| StartFault { what: "cap", code };
        let limit = Limit::new(CAP_BYTES).ok_or(cap(None))?;
        let started =
            halvo_cap::start(spec, limit).map_err(|error| cap(error.source.raw_os_error()))?;
        let Some(pid) = started.pid() else {
            return Err(StartFault {
                what: "worker",
                code: None,
            });
        };
        let ender = started.ender();
        let (input, output, closer) = (stream.input, stream.output, stream.closer);
        let serving = std::thread::Builder::new()
            .name("halvo-analysis-read".to_owned())
            .spawn(move || {
                let _ = cursor().serve(input, output);
            });
        let serving = match serving {
            Ok(serving) => serving,
            Err(error) => {
                ender.end();
                let _ = started.wait();
                return Err(StartFault::of("thread", &error));
            }
        };
        let (tx, ends) = mpsc::channel();
        let (log, reaches) = (self.log.clone(), ender.clone());
        let waiting = std::thread::Builder::new()
            .name("halvo-analysis-wait".to_owned())
            .spawn(move || {
                let outcome = started.wait();
                let (kind, line) = end_of(&outcome, reaches.reached());
                log.line(&line);
                let _ = tx.send(kind);
            });
        let waiting = match waiting {
            Ok(waiting) => waiting,
            Err(error) => {
                cursor().drop_open();
                closer.close();
                join_within(serving, REAP_WAIT);
                return Err(StartFault::of("thread", &error));
            }
        };
        Ok(Living {
            pid,
            from: origin.clone(),
            ender,
            ends,
            waiting,
            serving,
            closer,
        })
    }
}

impl Drop for WorkerSlot {
    fn drop(&mut self) {
        self.delete();
    }
}
