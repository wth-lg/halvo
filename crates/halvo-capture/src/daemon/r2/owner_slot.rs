//! The owner slot (contracts.md C24.4 items 2 to 4 of the workflows run): the one place of the daemon that starts
//! `halvo-owner`, the owner of the derived store, hands it the key and reaches it.
//!
//! - The first call that needs the owner starts it from the directory that the caller names
//!   (`children::beside`): `halvo-owner --data-dir=<absolute dir> --first-live=<n> --channel=<r>,<w>`. The child holds
//!   its standard streams and the two ends of its channel, and no other descriptor of the daemon.
//! - After the start and before it asks the caller for the key, the slot writes `derived/owner.pid`, one line
//!   `pid=<n> started=<microseconds since the epoch>`: under a temporary name in `derived/`, made new with mode
//!   0600, and then renamed into place, so that the slot never leaves a pid file without its whole line. `derived/`
//!   has mode 0700. The start time is the delete walk's reading (`owner_pid::start_time_of`), and where that answers
//!   none, on Windows, the creation time of the process. A file that cannot be written ends the child, and no key is
//!   asked for.
//! - The key goes as its 32 bytes on the owner's standard input, which the slot then closes; the slot holds it no
//!   longer than that write and no key between two starts. An owner that ends, after a refused key (exit 65 or 77)
//!   or for any cause, is reaped and its pid file removed before a call starts the next, and the call that finds the
//!   end answers `store_unavailable`. At most one owner lives.
//! - When the slot is made it reads a `derived/owner.pid` that an earlier daemon left. A file of another user or of a
//!   mode wider than 0600, a whole line that is not the slot's line, and a process of this user with that pid whose
//!   start time cannot be read are trusted for nothing: no signal, no unlink, and the slot starts no owner
//!   ([`OwnerSlot::distrust`] names the cause). A process of that pid and start time is ended, its start time read
//!   once more right before the signal (SIGTERM, after 5 s SIGKILL; on Windows `TerminateProcess`), and the file
//!   removed. A file that holds no whole line (empty or cut) names no process, and a pid that a process of another
//!   user holds names no owner of this user: such a file is removed, as where no process has the pid or its start
//!   time differs, and nothing is signalled. What keeps each removal safe is the store's own lock: an owner that
//!   still lives holds it, and a second owner fails at its open.
//! - [`OwnerSlot::relay`] writes one request frame and returns the owner's next frame that is no report; each report
//!   goes to [`Caller::report`]. [`OwnerSlot::turn`] gives the owner's channel to `dispatch_unknown`.
//! - [`OwnerSlot::stop`], and the drop of the slot, close the channel, wait 2 s for the owner's exit, then kill it,
//!   reap it and remove its pid file.
//! - [`OwnerSlot::start_with_worker`] stops the owner that runs and starts one with `--worker=<r>,<w>`, the owner's
//!   two ends of a worker's channel, which that child holds beside its standard streams and its channel.

use std::ffi::OsString;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, ErrorKind, PipeReader, PipeWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use halvo_control::paths::DERIVED;
use halvo_control::proto::{ErrorCode, ErrorReply};
use halvo_control::r2::OwnerChannel;
use halvo_control::wire::{
    read_frame, write_frame, FrameError, REPLY_MAX_BYTES, REQUEST_MAX_BYTES,
};

use crate::custody::StoreKey;
use crate::daemon::children::beside;
use crate::daemon::owner_pid::{owner_of, start_time_of, Owner};

#[cfg(target_os = "macos")]
#[path = "owner_slot_macos.rs"]
mod sys;
#[cfg(windows)]
#[path = "owner_slot_windows.rs"]
mod sys;

/// The program that the slot starts.
pub const PROGRAM: &str = "halvo-owner";

/// How long a call waits for the owner's reply, from the start of the call: the start of the owner among it.
pub const REPLY_WAIT: Duration = Duration::from_secs(4);

/// How long the stop waits for the owner's exit after it closed the channel, before it kills the owner.
pub const STOP_WAIT: Duration = Duration::from_secs(2);

/// How long an orphan has to end after SIGTERM, or on Windows after `TerminateProcess`.
const ORPHAN_TERM_WAIT: Duration = Duration::from_secs(5);

/// The step of every wait for a process.
const POLL: Duration = Duration::from_millis(10);

/// The owner's pid file under the data directory, as the registry names it.
const PID_FILE: &str = match DERIVED.owner_pid_file() {
    Some(file) => file,
    None => panic!("the registry names no owner pid file for derived"),
};

/// What the daemon gives the slot. Every method may be called from another thread than the slot's.
pub trait Caller: Send + Sync {
    /// The key of the derived store for one start, asked once the pid file stands; `None` when the daemon has no
    /// key, and the slot then ends the child. The slot drops the key right after it has written it.
    fn key(&self) -> Option<StoreKey>;

    /// The lowest live ordinal of the capture store, read at each start for `--first-live`.
    fn first_live(&self) -> u64;

    /// Whether the delete of everything runs: while it does, every call answers `busy` and no owner starts.
    fn deleting(&self) -> bool;

    /// One report frame of the owner (contracts.md C24.9), as it came, on the reader thread of the slot.
    fn report(&self, frame: &[u8]);
}

/// Where the slot finds the owner and the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    /// The directory that holds `halvo-owner`: the daemon's own, `children::own_dir`.
    pub program_dir: PathBuf,
    /// The data directory; the slot makes it absolute.
    pub data_dir: PathBuf,
    /// `--idle-ms` of the owner; none for the owner's default.
    pub idle_ms: Option<u32>,
}

/// Why the slot trusts the pid file that it found when it was made for nothing. Such a slot starts no owner, and
/// every call answers `store_unavailable`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Distrust {
    /// The file is no regular file, or it belongs to another user or has a mode wider than 0600.
    NotPrivate,
    /// The file cannot be read, or it holds a whole line that is not one line `pid=<n> started=<n>` of a pid
    /// above 1.
    Unreadable,
    /// A process of this user has the pid that the file names, and its start time cannot be read.
    StartTimeUnread { pid: u32 },
    /// The process of the pid and the start time that the file names did not end.
    NotEnded { pid: u32 },
}

impl Distrust {
    fn text(self) -> &'static str {
        match self {
            Distrust::NotPrivate => {
                "derived/owner.pid is no regular file, belongs to another user or has a mode wider than 0600"
            }
            Distrust::Unreadable => "derived/owner.pid cannot be read or does not hold its line",
            Distrust::StartTimeUnread { .. } => {
                "the start time of the process of this user that derived/owner.pid names cannot be read"
            }
            Distrust::NotEnded { .. } => "the process that derived/owner.pid names did not end",
        }
    }
}

impl fmt::Display for Distrust {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Distrust::StartTimeUnread { pid } | Distrust::NotEnded { pid } => {
                write!(f, "{} (pid {pid}); no owner is started", self.text())
            }
            Distrust::NotPrivate | Distrust::Unreadable => {
                write!(f, "{}; no owner is started", self.text())
            }
        }
    }
}

/// Who holds a pid in the system's process table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Holder {
    /// No process has the pid.
    Nobody,
    /// A process of this user (macOS: `kill` with the signal 0 succeeds; Windows: the process opens).
    ThisUser,
    /// A process that this user may not reach (macOS: EPERM; Windows: the open is refused).
    OtherUser,
}

/// What the slot reads of the process table, and how it ends an orphan, when it takes over a pid file that an
/// earlier daemon left. The daemon's is [`SystemTable`]; a test gives a stand-in.
pub trait ProcessTable {
    /// The start time of `pid` in microseconds since the epoch, or none where it cannot be read.
    fn start_time(&self, pid: u32) -> Option<u64>;

    /// Who holds `pid`.
    fn holder(&self, pid: u32) -> Holder;

    /// Ends the orphan `owner`, its start time read once more right before each signal: true when no process of
    /// that pid and start time runs after the call.
    fn end(&self, owner: Owner) -> bool;
}

/// The system's process table.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemTable;

impl ProcessTable for SystemTable {
    fn start_time(&self, pid: u32) -> Option<u64> {
        start_time(pid)
    }

    fn holder(&self, pid: u32) -> Holder {
        sys::holder(pid)
    }

    fn end(&self, owner: Owner) -> bool {
        sys::end(owner)
    }
}

/// The one owner of the derived store that the daemon starts, with its pid file.
pub struct OwnerSlot {
    program: PathBuf,
    data_dir: PathBuf,
    derived: PathBuf,
    pid_file: PathBuf,
    idle_ms: Option<u32>,
    caller: Arc<dyn Caller>,
    distrust: Option<Distrust>,
    running: Option<Running>,
}

impl OwnerSlot {
    /// Makes the slot over `place` and reads the pid file that an earlier daemon left. No owner starts here.
    pub fn new(place: Place, caller: Arc<dyn Caller>) -> OwnerSlot {
        OwnerSlot::with_table(place, caller, &SystemTable)
    }

    /// Makes the slot as [`OwnerSlot::new`] does, with `table` as the process table that the take-over of an earlier
    /// pid file reads. The starts of the slot read the system's.
    pub fn with_table(
        place: Place,
        caller: Arc<dyn Caller>,
        table: &dyn ProcessTable,
    ) -> OwnerSlot {
        let data_dir = std::path::absolute(&place.data_dir).unwrap_or(place.data_dir);
        let pid_file = data_dir.join(PID_FILE);
        let distrust = take_over(&pid_file, table);
        OwnerSlot {
            program: beside(&place.program_dir, PROGRAM),
            derived: data_dir.join(DERIVED.name()),
            pid_file,
            data_dir,
            idle_ms: place.idle_ms,
            caller,
            distrust,
            running: None,
        }
    }

    /// Why the slot trusts the pid file that it found for nothing, or none.
    pub fn distrust(&self) -> Option<Distrust> {
        self.distrust
    }

    /// The pid of the owner that the slot holds and has not reaped.
    pub fn pid(&self) -> Option<u32> {
        self.running
            .as_ref()
            .filter(|running| !running.reaped)
            .map(|running| running.child.id())
    }

    /// Relays one request to the owner (contracts.md C24.4 items 2 to 4) and returns its next frame that is no
    /// report, of at most 262,144 bytes. A request of no byte or of more than 4,096 is `invalid_argument` and is not
    /// sent; during the delete of everything the call is `busy`; with no owner, or an owner that ended, it is
    /// `store_unavailable`; with no reply within 4 s of the call's start it is `internal`.
    pub fn relay(&mut self, request: &[u8]) -> Result<Vec<u8>, ErrorReply> {
        let by = Instant::now() + REPLY_WAIT;
        if request.is_empty() || request.len() > REQUEST_MAX_BYTES {
            return Err(ErrorReply::new(
                ErrorCode::InvalidArgument,
                "a relayed request holds 1 to 4096 bytes",
            ));
        }
        self.turn()?.exchange(request, by)
    }

    /// The owner's channel for one exchange, as `dispatch_unknown` takes it, with the owner started where none
    /// runs. The refusals are those of [`OwnerSlot::relay`]. An owner whose channel ends during the turn is reaped
    /// when the turn is dropped.
    pub fn turn(&mut self) -> Result<Turn<'_>, ErrorReply> {
        if self.caller.deleting() {
            return Err(ErrorReply::new(
                ErrorCode::Busy,
                "the delete of everything runs",
            ));
        }
        if let Some(distrust) = self.distrust {
            return Err(unavailable(distrust.text()));
        }
        if self.running.as_mut().is_some_and(Running::over) {
            if let Some(mut over) = self.running.take() {
                over.finish(&self.pid_file, Duration::ZERO);
            }
        }
        if self.running.is_none() {
            self.running = Some(self.start(None)?);
        }
        match self.running.as_mut() {
            Some(running) => Ok(Turn {
                running,
                pid_file: &self.pid_file,
            }),
            None => Err(unavailable("the owner of the derived store does not run")),
        }
    }

    /// Closes the channel, waits 2 s for the owner's exit, then kills it, reaps it and removes its pid file.
    pub fn stop(&mut self) {
        if let Some(mut running) = self.running.take() {
            running.finish(&self.pid_file, STOP_WAIT);
        }
    }

    /// Stops the owner that runs and starts one that holds `worker`, the owner's two ends of a worker's channel: the
    /// end that it reads and the end that it writes, which it gets as `--worker=<r>,<w>`. An owner gets its ends at
    /// its start alone, so the owner of a worker is one that this call started for it. The refusals are those of
    /// [`OwnerSlot::relay`], and after a refusal no owner runs.
    pub fn start_with_worker(
        &mut self,
        worker: (PipeReader, PipeWriter),
    ) -> Result<(), ErrorReply> {
        if self.caller.deleting() {
            return Err(ErrorReply::new(
                ErrorCode::Busy,
                "the delete of everything runs",
            ));
        }
        if let Some(distrust) = self.distrust {
            return Err(unavailable(distrust.text()));
        }
        self.stop();
        self.running = Some(self.start(Some(worker))?);
        Ok(())
    }

    /// Starts the owner: the child, with its ends of a worker's channel where `worker` holds them, its pid file,
    /// its reader thread, and then its key.
    fn start(&self, worker: Option<(PipeReader, PipeWriter)>) -> Result<Running, ErrorReply> {
        sys::private_dir(&self.derived)
            .map_err(|_| unavailable("the folder derived cannot be made private"))?;
        let mut data_dir = OsString::from("--data-dir=");
        data_dir.push(&self.data_dir);
        let mut command = Command::new(&self.program);
        command
            .arg(data_dir)
            .arg(format!("--first-live={}", self.caller.first_live()));
        if let Some(ms) = self.idle_ms {
            command.arg(format!("--idle-ms={ms}"));
        }
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());
        let (mut child, channel) = sys::launch(&mut command, worker)
            .map_err(|_| unavailable("the owner of the derived store does not start"))?;
        let pid = child.id();
        let (Some(mut stdin), Some(started)) = (child.stdin.take(), start_time(pid)) else {
            end_child(&mut child, Duration::ZERO);
            return Err(unavailable("the start of the owner cannot be read"));
        };
        let line = format!("pid={pid} started={started}\n");
        if write_pid_file(&self.pid_file, &line).is_err() {
            end_child(&mut child, Duration::ZERO);
            return Err(unavailable("derived/owner.pid cannot be written"));
        }
        let Ok(frames) = spawn_reader(channel.reads, Arc::clone(&self.caller)) else {
            end_child(&mut child, Duration::ZERO);
            remove_ours(&self.pid_file, &line);
            return Err(unavailable(
                "the reader of the owner's channel does not start",
            ));
        };
        let mut running = Running {
            child,
            line,
            writes: Some(channel.writes),
            frames,
            owed: 0,
            ended: false,
            reaped: false,
        };
        let Some(key) = self.caller.key() else {
            running.finish(&self.pid_file, Duration::ZERO);
            return Err(unavailable("no key of the derived store is given"));
        };
        let written = stdin.write_all(key.expose());
        drop(key);
        drop(stdin);
        if written.is_err() {
            running.finish(&self.pid_file, Duration::ZERO);
            return Err(ended());
        }
        Ok(running)
    }
}

impl Drop for OwnerSlot {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The owner's channel for one exchange. It implements `OwnerChannel`, so that `dispatch_unknown` runs on it.
pub struct Turn<'a> {
    running: &'a mut Running,
    pid_file: &'a Path,
}

impl Turn<'_> {
    fn exchange(&mut self, request: &[u8], by: Instant) -> Result<Vec<u8>, ErrorReply> {
        if let Err(error) = self.running.write(request) {
            return Err(match error.kind() {
                ErrorKind::BrokenPipe => ended(),
                _ => ErrorReply::new(
                    ErrorCode::Internal,
                    "the request cannot be written to the owner of the derived store",
                ),
            });
        }
        match self
            .running
            .answer(by.saturating_duration_since(Instant::now()))
        {
            Ok(Some(reply)) => Ok(reply),
            Ok(None) => Err(ended()),
            Err(_) => Err(ErrorReply::new(
                ErrorCode::Internal,
                "the owner of the derived store did not reply within 4 s",
            )),
        }
    }
}

impl OwnerChannel for Turn<'_> {
    #[cfg(target_os = "macos")]
    fn send_stream(&mut self, frame: &[u8], end: std::os::fd::BorrowedFd<'_>) -> io::Result<()> {
        use std::os::fd::AsFd;
        let Some(writes) = self.running.writes.as_ref() else {
            return Err(ErrorKind::BrokenPipe.into());
        };
        let sent = halvo_control::r2::send_with_end(writes.as_fd(), frame, end);
        if sent.is_err() {
            self.running.ended = true;
        }
        sent
    }

    #[cfg(windows)]
    fn owner_process(&self) -> std::os::windows::io::BorrowedHandle<'_> {
        use std::os::windows::io::AsHandle;
        self.running.child.as_handle()
    }

    #[cfg(windows)]
    fn send_stream(&mut self, frame: &[u8]) -> io::Result<()> {
        self.running.write(frame)
    }

    fn answer(&mut self, within: Duration) -> io::Result<Option<Vec<u8>>> {
        self.running.answer(within)
    }
}

impl Drop for Turn<'_> {
    fn drop(&mut self) {
        if self.running.ended {
            self.running.finish(self.pid_file, STOP_WAIT);
        }
    }
}

/// A started owner.
struct Running {
    child: Child,
    /// The line of the pid file that this start wrote.
    line: String,
    /// The slot's end of the channel on which it writes, none once closed.
    writes: Option<sys::Writes>,
    /// The owner's frames that are no report, from the reader thread.
    frames: Receiver<Vec<u8>>,
    /// The answers that the owner still owes to calls that stopped waiting; each is passed over when it comes.
    owed: usize,
    /// The channel ended or a write on it failed: the owner is ended at the end of the turn.
    ended: bool,
    /// The child is reaped and its pid file removed.
    reaped: bool,
}

impl Running {
    /// Whether a call must reap this owner before it starts the next: it is reaped, its channel ended, or it exited.
    fn over(&mut self) -> bool {
        self.reaped || self.ended || !matches!(self.child.try_wait(), Ok(None))
    }

    /// Writes one frame on the channel. A failed write leaves the channel in no known state, so the owner is ended.
    fn write(&mut self, frame: &[u8]) -> io::Result<()> {
        let Some(writes) = self.writes.as_mut() else {
            return Err(ErrorKind::BrokenPipe.into());
        };
        let written = write_frame(writes, frame, REQUEST_MAX_BYTES).map_err(|error| match error {
            FrameError::Io(error) => error,
            _ => ErrorKind::InvalidInput.into(),
        });
        if written.is_err() {
            self.ended = true;
        }
        written
    }

    /// The owner's next frame that is no report, waiting at most `within`: none at the end of the channel, and an
    /// error of kind `TimedOut` when no frame came, whose frame is then passed over when it comes.
    fn answer(&mut self, within: Duration) -> io::Result<Option<Vec<u8>>> {
        let until = Instant::now() + within;
        loop {
            match self
                .frames
                .recv_timeout(until.saturating_duration_since(Instant::now()))
            {
                Ok(_) if self.owed > 0 => self.owed -= 1,
                Ok(frame) => return Ok(Some(frame)),
                Err(RecvTimeoutError::Timeout) => {
                    self.owed += 1;
                    return Err(ErrorKind::TimedOut.into());
                }
                Err(RecvTimeoutError::Disconnected) => {
                    self.ended = true;
                    return Ok(None);
                }
            }
        }
    }

    /// Closes the channel, waits `wait` for the owner's exit, then kills it, reaps it and removes the pid file
    /// that this start wrote.
    fn finish(&mut self, pid_file: &Path, wait: Duration) {
        if self.reaped {
            return;
        }
        if let Some(writes) = self.writes.take() {
            sys::close(writes);
        }
        end_child(&mut self.child, wait);
        remove_ours(pid_file, &self.line);
        self.reaped = true;
    }
}

/// Removes the pid file where it still holds `line`, the line that one start of this slot wrote.
fn remove_ours(pid_file: &Path, line: &str) {
    if fs::read_to_string(pid_file).is_ok_and(|text| text == line) {
        let _ = fs::remove_file(pid_file);
    }
}

/// The start time of `pid` in microseconds since the epoch: the delete walk's reading, and where that answers none,
/// the system's creation time of the process.
fn start_time(pid: u32) -> Option<u64> {
    start_time_of(pid).or_else(|| sys::creation_time(pid))
}

/// Reads the pid file that an earlier daemon left (ruling 5 of the unit, as the coordinator turned it at 17:52:26Z):
/// the cause when it is trusted for nothing, none when no file stands or after the file was removed.
fn take_over(pid_file: &Path, table: &dyn ProcessTable) -> Option<Distrust> {
    let meta = match fs::symlink_metadata(pid_file) {
        Ok(meta) => meta,
        Err(error) if error.kind() == ErrorKind::NotFound => return None,
        Err(_) => return Some(Distrust::Unreadable),
    };
    if !meta.is_file() || !sys::private(&meta) {
        return Some(Distrust::NotPrivate);
    }
    let whole = match fs::read(pid_file) {
        Ok(bytes) => bytes.ends_with(b"\n"),
        Err(error) if error.kind() == ErrorKind::NotFound => return None,
        Err(_) => return Some(Distrust::Unreadable),
    };
    if whole {
        let owner: Owner = match owner_of(pid_file) {
            Ok(Some(owner)) => owner,
            Ok(None) => return None,
            Err(_) => return Some(Distrust::Unreadable),
        };
        match table.start_time(owner.pid) {
            Some(started) if started == owner.started => {
                if !table.end(owner) {
                    return Some(Distrust::NotEnded { pid: owner.pid });
                }
            }
            Some(_) => {}
            None => {
                if table.holder(owner.pid) == Holder::ThisUser {
                    return Some(Distrust::StartTimeUnread { pid: owner.pid });
                }
            }
        }
    }
    let _ = fs::remove_file(pid_file);
    None
}

/// Writes the pid file: the one line into a new file of the temporary name beside it, made private and synced, and
/// then renamed into place, so that no pid file without its whole line ever stands at the name. A temporary file that
/// an earlier start left is removed first; a pid file that stands is never written over.
fn write_pid_file(path: &Path, line: &str) -> io::Result<()> {
    if fs::symlink_metadata(path).is_ok() {
        return Err(ErrorKind::AlreadyExists.into());
    }
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".new");
    let new = path.with_file_name(name);
    let _ = fs::remove_file(&new);
    let mut file: File = sys::create_private(&new)?;
    let placed = file
        .write_all(line.as_bytes())
        .and_then(|()| file.sync_all())
        .and_then(|()| fs::rename(&new, path));
    if placed.is_err() {
        drop(file);
        let _ = fs::remove_file(&new);
    }
    placed
}

/// Waits `wait` for the child's exit, then kills it, and reaps it.
fn end_child(child: &mut Child, wait: Duration) {
    let until = Instant::now() + wait;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) if Instant::now() < until => std::thread::sleep(POLL),
            Ok(None) | Err(_) => break,
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Starts the reader thread of the channel: each report goes to the caller, every other frame to the receiver, until
/// the channel ends or fails.
fn spawn_reader(mut reads: sys::Reads, caller: Arc<dyn Caller>) -> io::Result<Receiver<Vec<u8>>> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("halvo-owner-channel".to_owned())
        .spawn(move || {
            while let Ok(frame) = read_frame(&mut reads, REPLY_MAX_BYTES) {
                if is_report(&frame) {
                    caller.report(&frame);
                } else if tx.send(frame).is_err() {
                    return;
                }
            }
        })?;
    Ok(rx)
}

/// Whether a frame is a report: a JSON object whose one key is `report` (contracts.md C24.9).
fn is_report(frame: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Map<String, serde_json::Value>>(frame)
        .is_ok_and(|object| object.len() == 1 && object.contains_key("report"))
}

fn unavailable(why: &'static str) -> ErrorReply {
    ErrorReply::new(ErrorCode::StoreUnavailable, why)
}

fn ended() -> ErrorReply {
    unavailable("the owner of the derived store ended")
}
