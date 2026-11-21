//! The worker slot in the capture service's run: the one form of the slot that the daemon holds, and the four calls
//! of the run that make it, flag the app, stop it and delete it.
//!
//! - THE FORM. One lock holds the slot of the open capture store with the daemon's caller over its data directory
//!   ([`WorkerSlot`], [`DaemonCaller`]), and the mark of a delete. The driver of the slot ([`Driver`]) holds the
//!   form as its [`Slot`] and its [`Source`]. A pass, [`stop`] and [`delete`] each hold the lock for their whole
//!   length; the supervisor's tick never takes it, as it hands the driver's thread the loop's time alone
//!   (`when::tick`). With no slot in the form a pass answers [`Answer::Idle`], and [`Answer::Deleting`] while the
//!   mark stands.
//! - THE SOURCE. The form's source gives what [`DaemonCaller::reading`] gives at the clock's reading of the call:
//!   the origin, with the watermark as the one that a yes carries; none with no slot in the form.
//! - [`open`], at each open of the capture store: the cell of the cursor's reads forgets its figures and its ask
//!   (`daemon_caller::cell`), and a new caller and a new slot over the data directory go into the form, so neither
//!   the cell nor the caller of a fresh store keeps a figure or a position of an earlier one: no owner starts before
//!   a read of the store that is open. Its first call in a life of the daemon makes the driver and starts its thread
//!   ([`Driver::run`]). Where the directory of the programs cannot be read or the driver's thread does not start, it
//!   writes one line and makes nothing, and the service runs on:
//!
//!   ```text
//!   halvo-capture: analysis is not made: <the fault>
//!   ```
//!
//! - [`seen`], at each request that the service serves: a status that claims the indicator is the app's liveness
//!   signal (`when::app_seen`); no other request is.
//! - [`stop`], at the shutdown entry of every ending of the service's run: takes the slot out of the form and drops
//!   it on the calling thread, which ends the worker, closes the cursor and stops the owner
//!   ([`WorkerSlot::delete`]). The driver's thread is not joined: every later pass answers [`Answer::Idle`].
//! - [`delete`], at a delete of everything, after the capture store is closed and before the delete's steps: sets
//!   the mark, drops the slot as [`stop`] does, and makes the cell forget. The next [`open`] clears the mark, and
//!   makes the cell forget once more: a store owner that did not answer its shutdown in time can keep a read of the
//!   store that goes after the delete's forget, and its last pass comes before the store's lock is given up, so
//!   before the fresh store opens.

use std::fmt;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use halvo_control::proto::Request;
use halvo_schema::Position;

use crate::daemon::r2::daemon_caller::{cell, DaemonCaller};
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
    /// The mark of a delete: set by [`delete`] and cleared by the next [`open`].
    deleting: bool,
    /// The driver's thread runs.
    driven: bool,
}

/// The one form of the worker slot that the daemon holds.
struct Form(Mutex<Held>);

impl Form {
    fn held(&self) -> MutexGuard<'_, Held> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The form of the process.
static FORM: Form = Form(Mutex::new(Held {
    open: None,
    deleting: false,
    driven: false,
}));

impl Slot for &'static Form {
    fn pass(&mut self, now: Instant, may_run: MayRun, origin: &Origin) -> Answer {
        let mut held = self.held();
        if held.deleting {
            return Answer::Deleting;
        }
        match held.open.as_mut() {
            Some(open) => open.slot.pass(now, may_run, origin),
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
/// call. The cell forgets its figures and its ask, a new caller and a new slot go into the form, the slot that stood
/// there is dropped, and the mark of a delete is cleared. The first call in a life makes the driver over `readings`
/// and starts its thread; a later call drops `readings`. Where `program_dir` is an error, or the driver's thread does
/// not start, the call writes one line and makes nothing.
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
            lines.not_made("the directory of the daemon's programs cannot be read", &error);
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
    let slot = WorkerSlot::new(place, Arc::clone(&caller) as Arc<dyn Caller>, Box::new(lines));
    drop(held.open.take());
    cell().forget();
    held.open = Some(Open { caller, slot });
    held.deleting = false;
}

/// The app's liveness signal: a request that claims the indicator, `Request::Status { indicator: true }`, makes it
/// (`when::app_seen`), and no other request does.
pub fn seen(request: &Request) {
    if request.claims_the_indicator() {
        when::app_seen();
    }
}

/// The stop at an ending of the service's run: the slot leaves the form and is dropped on the calling thread, which
/// ends the worker, closes the cursor and stops the owner.
pub fn stop() {
    let mut held = FORM.held();
    drop(held.open.take());
}

/// The delete of everything, after the capture store is closed and before the delete's steps: the mark is set, the
/// slot leaves the form and is dropped as [`stop`] drops it, and then the cell forgets its figures and its ask.
pub fn delete() {
    let mut held = FORM.held();
    held.deleting = true;
    drop(held.open.take());
    cell().forget();
}
