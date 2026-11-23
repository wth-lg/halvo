//! The worker slot in the capture service's run: the one form of the slot that the daemon holds, and the five calls
//! of the run that make it, flag the app, stop it, delete it and serve a request of protocol v2 on it.
//!
//! - THE FORM. One lock holds the slot of the open capture store with the daemon's caller over its data directory
//!   ([`WorkerSlot`], [`DaemonCaller`]). Beside the lock stand two marks, of a delete and of a stop, and a third,
//!   the deadline of the request `defer` (`Deferral` of daemon/r2/defer.rs). The driver of
//!   the slot ([`Driver`]) holds the form as its [`Slot`] and its [`Source`]. A pass, [`stop`], [`delete`] and an
//!   exchange of `serve` each hold the lock for their whole length; the supervisor's tick never takes it, as it
//!   hands the driver's thread the loop's time alone (`when::tick`). With no slot in the form a pass answers
//!   [`Answer::Idle`], and [`Answer::Deleting`] while the mark of a delete stands.
//! - THE MARKS. [`delete`] and [`stop`] each set their mark before they ask for the lock, so whoever gets the lock
//!   before them finds the mark: a pass starts nothing and touches no slot, and an exchange is refused with no owner
//!   started. Each of the two so waits behind one exchange or one pass at most. The mark of a delete stands until
//!   the next [`open`]; the mark of a stop stands until that stop has dropped the slot, after which the form holds
//!   no slot and answers the same.
//! - THE DEADLINE. `serve` sets the third mark at each request `defer`, with no lock of the form, and [`open`],
//!   [`stop`] and [`delete`] leave it (defer.rs holds its rule). While it holds, a pass turns the caller's
//!   yes into a no: the slot ends a living worker by its kill, with the line `analysis end=ended`, and starts none,
//!   and the owner that ran keeps running. A pass that held the lock at the request can still start one worker,
//!   which the next pass ends.
//! - THE SHREDDED SEGMENTS. Before the slot's own pass, each pass of the driver hands the owner that runs the
//!   ordinals that the store's maintenance shredded since the last hand-over (`OwnerSlot::hand_over`), under the
//!   lock, whatever the maintenance pass that shredded them answered: it starts no owner and stops none, so a
//!   worker's batch goes on. With no owner running they wait for the next start, which hands them on before its
//!   first request. An [`open`] or a [`delete`] makes the cell forget them; the next start then takes those
//!   segments by `purge_below` or by the keyless list of the fresh read from ordinal 0.
//! - THE SOURCE. The form's source gives what [`DaemonCaller::reading`] gives at the clock's reading of the call:
//!   the origin, with the watermark as the one that a yes carries; none with no slot in the form.
//! - [`open`], at each open of the capture store: the cell of the cursor's reads forgets its figures, its ask and the
//!   passes' ordinals (`daemon_caller::cell`), and a new caller and a new slot over the data directory go into the
//!   form, so neither the cell nor the caller of a fresh store keeps a figure or a position of an earlier one: no
//!   owner starts before a read of the store that is open. Its first call in a life of the daemon makes the driver
//!   and starts its thread ([`Driver::run`]). Where the directory of the programs cannot be read or the driver's
//!   thread does not start, it writes one line and makes nothing, and the service runs on:
//!
//!   ```text
//!   halvo-capture: analysis is not made: <the fault>
//!   ```
//!
//! - [`seen`], at each request that the service serves: a status that claims the indicator is the app's liveness
//!   signal (`when::app_seen`); no other request is.
//! - [`stop`], at the shutdown entry of every ending of the service's run: sets its mark, takes the slot out of the
//!   form and drops it on the calling thread, which ends the worker, closes the cursor and stops the owner
//!   ([`WorkerSlot::delete`]). The driver's thread is not joined: every later pass answers [`Answer::Idle`].
//! - [`delete`], at a delete of everything, after the capture store is closed and before the delete's steps: sets
//!   the mark, drops the slot as [`stop`] does, and makes the cell forget. The next [`open`] clears the mark, and
//!   makes the cell forget once more: a store owner that did not answer its shutdown in time can keep a read of the
//!   store that goes after the delete's forget, and its last pass comes before the store's lock is given up, so
//!   before the fresh store opens.
//! - `serve`, on macOS and on Windows, at a request of an accepted connection that the v1 table does not hold and that
//!   is no run mark, on the connection's own thread and behind the server's peer check. A request `defer` is answered
//!   first (`halvo_control::r2::serve_defer`), with no lock taken and no owner started, in every state of the form. A
//!   request that the control crate's module of protocol v2 does not serve (`halvo_control::r2::serves`) is left to
//!   the server, with no lock taken.
//!   For any other the call takes the form's lock. While the mark of a delete stands the client gets the error
//!   `busy`; while the mark of a stop stands, or with no slot in the form, `store_unavailable`; a refusal of the
//!   owner slot's turn (`OwnerSlot::turn`) goes out as it stands. Each is one error reply of v1, written after the
//!   lock is given up. Otherwise `halvo_control::r2::dispatch_unknown` runs on the owner's channel for one
//!   exchange, and the lock is held until it has returned and the turn is dropped: a start of the owner where none
//!   runs, at most 4 s for the owner's answer, at most 1 s for the end of the client's request on macOS, and 2 s more
//!   where the owner's channel ended.

use std::fmt;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use halvo_control::proto::Request;
use halvo_schema::Position;

use crate::daemon::r2::daemon_caller::{cell, DaemonCaller};
use crate::daemon::r2::defer::Deferral;
use crate::daemon::r2::owner_slot::{Caller, Place};
use crate::daemon::r2::when::{self, Driver, Readings, Slot, Source};
use crate::daemon::r2::worker_slot::{Answer, MayRun, Origin, WorkerSlot};

/// The slot of the open capture store, with the caller over its data directory.
struct Open {
    caller: Arc<DaemonCaller>,
    slot: WorkerSlot,
}

/// What the form's lock holds.
struct Held {
    /// The slot of the open store; none before the first open, and after a stop or a delete.
    open: Option<Open>,
    /// The driver's thread runs.
    driven: bool,
}

/// The one form of the worker slot that the daemon holds: the lock, the two marks beside it, which are read under the
/// lock and set before it is asked for, and the deadline of `defer`, which is set with no lock of the form.
struct Form {
    held: Mutex<Held>,
    /// The mark of a delete: [`delete`] sets it before it asks for the lock and once more under it, so that an
    /// [`open`] that held the lock in between does not clear it; the next [`open`] clears it.
    deleting: AtomicBool,
    /// The mark of a stop: the calls of [`stop`] that have not yet dropped the slot. Each adds one before it asks
    /// for the lock and takes it back under the lock, once the slot is out.
    stopping: AtomicUsize,
    /// The deadline of `defer`: while it holds, a pass turns a yes into a no. `serve` sets it; [`open`], [`stop`] and
    /// [`delete`] leave it.
    deferred: Deferral,
}

impl Form {
    fn held(&self) -> MutexGuard<'_, Held> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Whether the mark of a delete stands.
    fn deleting(&self) -> bool {
        self.deleting.load(Ordering::SeqCst)
    }

    /// Whether the mark of a stop stands.
    fn stopping(&self) -> bool {
        self.stopping.load(Ordering::SeqCst) != 0
    }

    /// The owner's channel for one exchange, from the slot that `held` holds, with the owner started where none
    /// runs; or the refusal: `busy` while the mark of a delete stands, `store_unavailable` while the mark of a stop
    /// stands and with no slot, and else the refusal of the owner slot's turn as it stands. No owner starts for a
    /// refusal of a mark or of a missing slot.
    #[cfg(any(target_os = "macos", windows))]
    fn turn<'a>(
        &self,
        held: &'a mut Held,
    ) -> Result<crate::daemon::r2::owner_slot::Turn<'a>, halvo_control::proto::ErrorReply> {
        use halvo_control::proto::{ErrorCode, ErrorReply};
        if self.deleting() {
            return Err(ErrorReply::new(
                ErrorCode::Busy,
                "the delete of everything runs",
            ));
        }
        if self.stopping() {
            return Err(ErrorReply::new(
                ErrorCode::StoreUnavailable,
                "the capture service's run is ending",
            ));
        }
        match held.open.as_mut() {
            Some(open) => open.slot.owner().turn(),
            None => Err(ErrorReply::new(
                ErrorCode::StoreUnavailable,
                "no capture store is open",
            )),
        }
    }
}

/// The form of the process.
static FORM: Form = Form {
    held: Mutex::new(Held {
        open: None,
        driven: false,
    }),
    deleting: AtomicBool::new(false),
    stopping: AtomicUsize::new(0),
    deferred: Deferral::new(),
};

impl Slot for &'static Form {
    fn pass(&mut self, now: Instant, may_run: MayRun, origin: &Origin) -> Answer {
        let mut held = self.held();
        if self.deleting() {
            return Answer::Deleting;
        }
        if self.stopping() {
            return Answer::Idle;
        }
        match held.open.as_mut() {
            Some(open) => {
                open.slot.owner().hand_over();
                let may_run = self.deferred.may_run(may_run, Instant::now());
                open.slot.pass(now, may_run, origin)
            }
            None => Answer::Idle,
        }
    }
}

impl Source for &'static Form {
    fn origin(&mut self) -> Option<(Origin, Option<Position>)> {
        let caller = Arc::clone(&self.held().open.as_ref()?.caller);
        let reading = caller.reading(Instant::now())?;
        Some((reading.origin, Some(reading.watermark)))
    }
}

/// One writer of lines that `open`, the caller and the slot share. A line goes to the writer whole.
#[derive(Clone)]
struct Lines(Arc<Mutex<Box<dyn Write + Send>>>);

impl Lines {
    fn writer(&self) -> MutexGuard<'_, Box<dyn Write + Send>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Writes one line of a making that failed; a line that cannot be written is dropped.
    fn not_made(&self, fault: &str, error: &io::Error) {
        let mut writer = self.writer();
        let _ = writeln!(
            writer,
            "halvo-capture: analysis is not made: {fault}: {}",
            error.kind()
        );
        let _ = writer.flush();
    }
}

impl Write for Lines {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.writer().write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.writer().flush()
    }

    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.writer().write_all(bytes)
    }

    fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> io::Result<()> {
        self.writer().write_fmt(args)
    }
}

/// Makes the worker slot of the capture store that opened over `data_dir`, with the programs in `program_dir` (the
/// daemon's own, `children::own_dir`), and `log` as the writer of the lines of the slot, of its caller and of this
/// call. The cell forgets its figures, its ask and the passes' ordinals, a new caller and a new slot go into the form,
/// the slot that stood there is dropped, and the mark of a delete is cleared. The first call in a life makes the
/// driver over `readings` and starts its thread; a later call drops `readings`. Where `program_dir` is an error, or
/// the driver's thread does not start, the call writes one line and makes nothing.
pub fn open<R: Readings + 'static>(
    data_dir: &Path,
    program_dir: io::Result<PathBuf>,
    readings: R,
    log: Box<dyn Write + Send>,
) {
    let lines = Lines(Arc::new(Mutex::new(log)));
    let program_dir = match program_dir {
        Ok(program_dir) => program_dir,
        Err(error) => {
            lines.not_made(
                "the directory of the daemon's programs cannot be read",
                &error,
            );
            return;
        }
    };
    let mut held = FORM.held();
    if !held.driven {
        if let Err(error) = Driver::new(readings, &FORM, &FORM).run() {
            lines.not_made("the thread of its driver did not start", &error);
            return;
        }
        held.driven = true;
    }
    let caller = Arc::new(DaemonCaller::new(data_dir, Box::new(lines.clone())));
    let place = Place {
        program_dir,
        data_dir: data_dir.to_path_buf(),
        idle_ms: None,
    };
    let slot = WorkerSlot::new(
        place,
        Arc::clone(&caller) as Arc<dyn Caller>,
        Box::new(lines),
    );
    drop(held.open.take());
    cell().forget();
    held.open = Some(Open { caller, slot });
    FORM.deleting.store(false, Ordering::SeqCst);
}

/// The app's liveness signal: a request that claims the indicator, `Request::Status { indicator: true }`, makes it
/// (`when::app_seen`), and no other request does.
pub fn seen(request: &Request) {
    if request.claims_the_indicator() {
        when::app_seen();
    }
}

/// The stop at an ending of the service's run: the mark of a stop is set before the lock is asked for, then the slot
/// leaves the form and is dropped on the calling thread, which ends the worker, closes the cursor and stops the
/// owner, and the mark is taken back under the lock.
pub fn stop() {
    FORM.stopping.fetch_add(1, Ordering::SeqCst);
    let mut held = FORM.held();
    drop(held.open.take());
    FORM.stopping.fetch_sub(1, Ordering::SeqCst);
}

/// The delete of everything, after the capture store is closed and before the delete's steps: the mark is set before
/// the lock is asked for and once more under it, the slot leaves the form and is dropped as [`stop`] drops it, and
/// then the cell forgets its figures, its ask and the passes' ordinals.
pub fn delete() {
    FORM.deleting.store(true, Ordering::SeqCst);
    let mut held = FORM.held();
    FORM.deleting.store(true, Ordering::SeqCst);
    drop(held.open.take());
    cell().forget();
}

/// The accepted connection of the control server that [`serve`] takes: the Unix socket on macOS, the pipe's stream,
/// opened for overlapped I/O, on Windows.
#[cfg(target_os = "macos")]
pub type Accepted<'a> = &'a std::os::unix::net::UnixStream;
/// The accepted connection of the control server that [`serve`] takes: the Unix socket on macOS, the pipe's stream,
/// opened for overlapped I/O, on Windows.
#[cfg(windows)]
pub type Accepted<'a> = &'a mut halvo_control::client_windows::PipeStream;

/// The connection as the handlers of the control crate borrow it: the socket itself on macOS, its handle on Windows.
#[cfg(target_os = "macos")]
fn borrowed<'a>(connection: &'a Accepted<'_>) -> halvo_control::r2::Connection<'a> {
    connection
}

/// The connection as the handlers of the control crate borrow it: the socket itself on macOS, its handle on Windows.
#[cfg(windows)]
fn borrowed<'a>(connection: &'a Accepted<'_>) -> halvo_control::r2::Connection<'a> {
    use std::os::windows::io::AsHandle;
    connection.as_handle()
}

/// Serves `request`, a request that the v1 table does not hold and that is no run mark, on `connection`, the accepted
/// connection of the control server, whose peer the server has checked: true when the call answered the request on
/// the connection, false when the server is to answer it as v1 does. The call adds no check of the peer.
///
/// A request `defer` is answered first, by `serve_defer` with the setter of the form's deadline, with no lock taken
/// and no owner started, in every state of the form. A request that the control crate's module of protocol v2 does
/// not serve is false, with no lock taken and no owner started. For any other the call takes the form's lock: under a
/// mark, with no slot, and where the owner slot refuses its turn, the client gets that one error reply of v1, with no
/// end of a stream beside it, written through `connection` after the lock is given up; otherwise `dispatch_unknown`
/// runs on the owner's channel, and the lock is held until it has returned and the turn is dropped. On Windows the
/// handlers get the handle of the pipe and write on it with their own overlapped writes, and the refusal goes through
/// the stream, whose writes wait at most the timeout that the server set.
#[cfg(any(target_os = "macos", windows))]
pub fn serve(request: &halvo_control::proto::UnknownRequest, mut connection: Accepted<'_>) -> bool {
    use halvo_control::proto::{encode_reply, Reply};
    use halvo_control::wire::{write_frame, REPLY_MAX_BYTES};
    if halvo_control::r2::serve_defer(request, borrowed(&connection), |until| {
        FORM.deferred.set(until)
    }) {
        return true;
    }
    if !halvo_control::r2::serves(request) {
        return false;
    }
    let mut held = FORM.held();
    let refusal = match FORM.turn(&mut held) {
        Ok(mut turn) => {
            return halvo_control::r2::dispatch_unknown(request, borrowed(&connection), &mut turn)
        }
        Err(refusal) => refusal,
    };
    drop(held);
    // The client reads no reply when this write fails, and no end was handed over.
    let _ = write_frame(
        &mut connection,
        &encode_reply(&Reply::Error(refusal)),
        REPLY_MAX_BYTES,
    );
    true
}
