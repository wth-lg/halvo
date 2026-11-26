//! The run marker of a service run, run/running of the data directory
//! (contracts.md C3.2 step 5 and its stop, D-05): read at the start to set
//! `last_stop`, written anew for the life that starts, and written once more
//! by a life that ends in order, with the line `ended=` and the trigger of
//! that end in the words of contracts.md C2.3 (C14 item 4), and at the
//! decision of a stop by a Stop request or a signal, with that line beside
//! its process line, which an end with 1, the watchdog's end and a kill
//! leave. It holds the process id of its life, and the hold of ruling
//! PL-3 part (5) while that stands: a record of a life that a source which
//! did not stop ended, or the line `hold=yes` that carries such a record's
//! hold on through the lives after it until a Resume lifts it. Beside the
//! marker, run/ holds the note run/typing-off of a turn of typing to off
//! that the state file did not take, which keeps typing off through the
//! lives after it until a turn on lets it go.

use std::io::{self, Read, Write};
#[cfg(target_os = "macos")]
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::Path;

use halvo_control::model::UtcSecond;
use halvo_control::paths::DataDir;
use halvo_control::proto::{LastStop, LostRecord, StopKind};

use super::board::LastLife;
use super::state::StateDir;

const MARKER_READ_MAX_BYTES: u64 = 4_096;

const HOLD_LINE: &str = "hold=yes";

const ENDED: &str = "ended=";

const PID: &str = "pid=";

const RECORD: &str = "last_stop=";

const LOST: &str = "lost=";

const TYPING_OFF: &str = "typing-off";

const TYPING_OFF_NEW: &str = "typing-off.new";

const TYPING_OFF_LINE: &str = "key_class=off\n";

const KEYS_RESTART: &str = "keys-restart";

const KEYS_RESTART_NEW: &str = "keys-restart.new";

const KEYS_RESTART_LINE: &str = "input_monitoring=restarted\n";

/// run/ of the folder that the instance lock holds, where every write of the
/// marker after the start of a life goes: on macOS a descriptor of run/,
/// opened through the descriptor of the held folder as the state file's
/// folder is, so that a write lands in that folder wherever it is moved and
/// never makes an entry at the folder's name; on Windows, where the folder
/// is not moved while the lock holds it, run/ by the folder's path, as the
/// state file's folder is there.
pub struct HeldRun {
    #[cfg(target_os = "macos")]
    fd: std::os::fd::OwnedFd,
    #[cfg(windows)]
    path: std::path::PathBuf,
}

impl HeldRun {
    /// run/ of the held folder `folder` where it stands, opened following no
    /// link; where none stands it makes none and answers NotFound, so that a
    /// run whose start made no run/ writes nothing into the folder that it
    /// holds.
    #[cfg(target_os = "macos")]
    pub fn of(folder: &StateDir) -> io::Result<HeldRun> {
        use std::os::fd::{AsRawFd, FromRawFd};
        let at = folder.descriptor().as_raw_fd();
        let name = std::ffi::CString::new(halvo_control::paths::RUN.name())
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        // SAFETY: `at` is the descriptor of `folder`, open across the call, and `name` is a
        // NUL-terminated string that lives across it; without O_CREAT openat reads no mode
        // argument.
        let fd = unsafe {
            libc::openat(
                at,
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd == -1 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `fd` is the descriptor that openat returned and checked, owned by nothing
        // else.
        let fd = unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) };
        Ok(HeldRun { fd })
    }

    /// run/ of the held folder `folder`, by its path, where it stands as a
    /// folder; where none stands it makes none and answers NotFound; a run/
    /// whose entry cannot be read answers that error, and one that is no
    /// folder NotADirectory, as the open of the descriptor does on macOS.
    #[cfg(windows)]
    pub fn of(folder: &StateDir) -> io::Result<HeldRun> {
        let path = folder.path().join(halvo_control::paths::RUN.name());
        if !std::fs::symlink_metadata(&path)?.is_dir() {
            return Err(io::ErrorKind::NotADirectory.into());
        }
        Ok(HeldRun { path })
    }

    /// The same run/ on a descriptor of its own.
    #[cfg(target_os = "macos")]
    pub fn again(&self) -> io::Result<HeldRun> {
        Ok(HeldRun {
            fd: self.fd.try_clone()?,
        })
    }

    /// The same run/, by its path.
    #[cfg(windows)]
    pub fn again(&self) -> io::Result<HeldRun> {
        Ok(HeldRun {
            path: self.path.clone(),
        })
    }
}

/// Whether the note of a turn of typing to off that the state file did not
/// take stands in run/ of the held folder, as a start reads it through `run`:
/// where no run/ stands no note can, so none; where neither run/typing-off
/// nor run/typing-off.new (a write of the note that began and did not end)
/// stands, none; any entry at either name, whatever it holds, and a run/
/// or a name that cannot be read, read as kept, the side of no capture.
pub fn typing_kept(run: &io::Result<HeldRun>) -> bool {
    match run {
        Ok(run) => [TYPING_OFF, TYPING_OFF_NEW]
            .iter()
            .any(|name| !absent_in(run, name)),
        Err(error) => error.kind() != io::ErrorKind::NotFound,
    }
}

/// The one writer of the note: run/typing-off with the line `key_class=off`,
/// by the durable write of the marker, through `run`, the held folder.
pub fn keep_typing_off(run: &HeldRun) -> io::Result<()> {
    replace_named_in(run, TYPING_OFF, TYPING_OFF_LINE.as_bytes())
}

/// The one remover of the note: run/typing-off and a run/typing-off.new
/// that a write left, each through `run`, the held folder; a name where
/// nothing stands is no error.
pub fn let_typing_go(run: &HeldRun) -> io::Result<()> {
    remove_named_in(run, TYPING_OFF)?;
    remove_named_in(run, TYPING_OFF_NEW)
}

/// Whether the note of a life that ended for a grant of Input Monitoring
/// whose tap held no key type stands in run/ of the held folder, as a start
/// reads it through `run`: where no run/ stands, none; any entry at
/// run/keys-restart or run/keys-restart.new, and a run/ or a name that
/// cannot be read, read as standing, the side of no further end.
pub fn keys_restart_stands(run: &io::Result<HeldRun>) -> bool {
    match run {
        Ok(run) => [KEYS_RESTART, KEYS_RESTART_NEW]
            .iter()
            .any(|name| !absent_in(run, name)),
        Err(error) => error.kind() != io::ErrorKind::NotFound,
    }
}

/// The one writer of that note: run/keys-restart, by the durable write of
/// the marker, through `run`, the held folder.
pub fn keep_keys_restart(run: &HeldRun) -> io::Result<()> {
    replace_named_in(run, KEYS_RESTART, KEYS_RESTART_LINE.as_bytes())
}

/// The one remover of that note, and of a run/keys-restart.new that a write
/// left; a name where nothing stands is no error.
pub fn let_keys_restart_go(run: &HeldRun) -> io::Result<()> {
    remove_named_in(run, KEYS_RESTART)?;
    remove_named_in(run, KEYS_RESTART_NEW)
}

/// What a start found in the marker and made of it: its reading, the result
/// of its write, the loss of the last life that the marker held, and the
/// marker that it found, which [`give_back`] puts back.
pub struct Begun {
    pub last_stop: LastStop,
    pub written: io::Result<()>,
    pub lost: Option<LastLife>,
    found: Option<String>,
}

/// Why the last life of the data directory ended, as its run marker tells a
/// start that no person's press stands behind (`--no-press`, contracts.md
/// C14 item 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LastEnd {
    /// A life left the line of the trigger of its end: at an end with 0
    /// alone or after the hold, and at the decision of a stop by a Stop
    /// request or a signal, beside its process line, where the end did not
    /// write the clean form over it.
    Ended(StopKind),
    /// A life left its own process line alone, or the record of ruling PL-3:
    /// a kill or a crash before any decision of a stop, an end with 4 or 5,
    /// an end with 1 that no Stop request and no signal began, or a decision
    /// whose line could not be written.
    NotInOrder,
    /// No marker, a marker that cannot be read, or one with no line that
    /// tells why its life ended, as a service of an older build leaves at
    /// its end in order.
    Untold,
}

/// What a start that no person's press stands behind does behind the last
/// life's end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoPress {
    Serves,
    WaitsForAPress,
}

impl LastEnd {
    /// The one table of what a start with `--no-press` does behind each end
    /// (the coordinator's rulings of log lines 17942, 17965, 18046 and
    /// 18137): behind a Stop request or a signal, with 0 or with 1, and where
    /// the marker does not tell, it waits for a press, erring toward no
    /// capture; behind the lifeline's end, a session's end, a delete's end,
    /// and a process line alone or a record, it serves as any start does. The
    /// other kinds of C2.3 end no life with 0 or 1 and so stand in no line; a
    /// line that names one serves.
    pub fn no_press(self) -> NoPress {
        match self {
            LastEnd::Ended(kind) => match kind {
                StopKind::Stop => NoPress::WaitsForAPress,
                StopKind::Signal => NoPress::WaitsForAPress,
                StopKind::Lifeline => NoPress::Serves,
                StopKind::SessionEnd => NoPress::Serves,
                StopKind::DeleteAll => NoPress::Serves,
                StopKind::Pause => NoPress::Serves,
                StopKind::Lapse => NoPress::Serves,
                StopKind::GrantLost => NoPress::Serves,
                StopKind::GrantAppeared => NoPress::Serves,
            },
            LastEnd::NotInOrder => NoPress::Serves,
            LastEnd::Untold => NoPress::WaitsForAPress,
        }
    }
}

/// Reads the marker that the last life left and writes the marker of this
/// one, with the hold when the marker that stood held it. The loss that the
/// last life carried in its line of a loss is read into [`Begun`], and the
/// marker of this life holds no such line, so a loss stands for one life. No marker and no
/// run/ before it is a first life (`none`); no marker in a run/ that stands,
/// or a marker whose lines are the hold and the line of an end in order,
/// either or both, is a life that ended in order (`clean`); any other marker
/// that stands is a life that did not end in order (`unclean`). A marker that
/// cannot be read, or one that cannot be written, is an error; the reading is
/// then `unclean`.
pub fn begin(dir: &DataDir) -> Begun {
    let run = dir.run_dir();
    let run_stood = std::fs::symlink_metadata(&run).is_ok_and(|meta| meta.is_dir());
    let read = read(dir);
    let last_stop = match &read {
        Ok(None) if run_stood => LastStop::Clean,
        Ok(None) => LastStop::None,
        Ok(Some(text)) if ended_in_order(text) => LastStop::Clean,
        Ok(Some(_)) | Err(_) => LastStop::Unclean,
    };
    let found = read.as_ref().ok().cloned().flatten();
    let lost = found.as_deref().and_then(lost_of);
    let written = read.and_then(|text| write(dir, text.as_deref().is_some_and(holds_the_hold)));
    Begun {
        last_stop,
        written,
        lost,
        found,
    }
}

/// Reads why the last life ended, for a start that no person's press stands
/// behind, and writes nothing: the line of an end names its trigger, also
/// beside a process line, a word that is no kind of C2.3 tells nothing, and
/// with no such line the process line of a life or a record is a life that
/// did not end in order.
pub fn last_end(dir: &DataDir) -> LastEnd {
    let Ok(Some(text)) = read(dir) else {
        return LastEnd::Untold;
    };
    if let Some(word) = text.lines().find_map(|line| line.strip_prefix(ENDED)) {
        return word.parse().map_or(LastEnd::Untold, LastEnd::Ended);
    }
    if text
        .lines()
        .any(|line| line.starts_with(PID) || line.starts_with(RECORD))
    {
        LastEnd::NotInOrder
    } else {
        LastEnd::Untold
    }
}

/// At a start that cannot bind its channel: where the start read the marker
/// clean or found none, it puts back the marker that it found (no marker
/// stays none; the hold, the line of an end in order and the line of a loss
/// stay), so that the next start reads what this one read; where it found
/// the marker of a life that did not end in order, the marker that `begin`
/// wrote in its place stays, so that the next start reads unclean too, and
/// the line of a loss that the marker held goes with it: no later start
/// reads that loss; where `begin` wrote no marker, it writes nothing. It
/// writes through `run`, the held folder.
pub fn give_back(run: &HeldRun, begun: &Begun) -> io::Result<()> {
    if begun.last_stop == LastStop::Unclean || begun.written.is_err() {
        return Ok(());
    }
    match &begun.found {
        Some(text) => replace_in(run, text.as_bytes()),
        None => remove_in(run),
    }
}

/// Takes the line of a loss out of the marker that stands in `run`, the
/// held folder, as the offline delete of everything does before its steps:
/// where the marker holds such a line, it is written back without it by the
/// durable write, every other line byte for byte; where it holds none, or no
/// marker stands, nothing is written.
pub fn forget_the_loss(run: &HeldRun) -> io::Result<()> {
    let Some(text) = read_in(run)? else {
        return Ok(());
    };
    if !text.lines().any(|line| line.starts_with(LOST)) {
        return Ok(());
    }
    let kept: String = text
        .split_inclusive('\n')
        .filter(|line| !line.starts_with(LOST))
        .collect();
    replace_in(run, kept.as_bytes())
}

/// Whether the marker that stands holds the hold of ruling PL-3 part (5).
pub fn held(dir: &DataDir) -> bool {
    read(dir).is_ok_and(|text| text.as_deref().is_some_and(holds_the_hold))
}

/// What the end of a life leaves in its marker, by its exit status and the
/// trigger of its end (contracts.md C3.2 Stop, C14 item 4): at 0 [`end`]; at
/// 1 where a Stop request or a signal began the end, the form of
/// [`decided`] once more, so that the next start reads it unclean, as before,
/// and a start that no press stands behind reads the trigger; at every other
/// end the marker stays as the life left it: its process line alone, the
/// form of the decision, or the record of ruling PL-3 at an end with 5.
/// At 1 a marker that cannot be read, or a write that fails, gives what
/// [`decided`] gives. Where `lost` holds what the life did not store and a
/// trigger other than a delete's end stands, those two writes, and no other,
/// carry it in the line of the loss, which the next start reads ([`begin`]).
/// Every read and write goes through `run`, the held folder.
pub fn at_the_end(
    run: &HeldRun,
    status: i32,
    ended_by: Option<StopKind>,
    lost: Option<&LostRecord>,
) -> io::Result<()> {
    let carried = ended_by
        .filter(|kind| *kind != StopKind::DeleteAll)
        .zip(lost);
    match (status, ended_by) {
        (halvo_control::exit::CLEAN, _) => end(run, ended_by, carried),
        (halvo_control::exit::ERROR, Some(kind @ (StopKind::Stop | StopKind::Signal))) => {
            decision(run, kind, carried)
        }
        _ => Ok(()),
    }
}

/// The decision of a stop (contracts.md C3.2 Stop, C14 item 4), written where
/// a Stop request is accepted or the run reads that a signal set its stop
/// flag, before anything of the end that can hang or fail: the process line
/// of the life, the hold while it stands and the line of `kind`, by the
/// durable write. An end with 0 writes its clean form over it ([`end`]); an
/// end with 1, the watchdog's end and a kill leave it, which the next start
/// reads unclean and a start that no press stands behind reads as `kind`.
/// Where the marker cannot be read, or the write fails before its rename,
/// the marker stays as it stands, the hold kept; where only the sync of run/
/// behind the rename fails, on a Mac, the decision stands and the call
/// answers that error. It reads and writes through `run`, the held folder.
pub fn decided(run: &HeldRun, kind: StopKind) -> io::Result<()> {
    decision(run, kind, None)
}

fn decision(
    run: &HeldRun,
    kind: StopKind,
    carried: Option<(StopKind, &LostRecord)>,
) -> io::Result<()> {
    let found = read_in(run)?;
    let mut text = format!("{PID}{}\n", std::process::id());
    if found.as_deref().is_some_and(holds_the_hold) {
        text.push_str(HOLD_LINE);
        text.push('\n');
    }
    text.push_str(ENDED);
    text.push_str(kind.as_str());
    text.push('\n');
    if let Some((kind, lost)) = carried {
        text.push_str(&lost_line(kind, lost));
    }
    replace_in(run, text.as_bytes())
}

/// Ends the marker of a life that ended in order: in its place the hold,
/// while it stands, the line of `ended_by`, the trigger of the end, and the
/// line of the loss that `carried` holds, by the durable write. With no
/// trigger, or where that write fails while no hold stands, a removal of the
/// marker and a sync of run/ are tried, as an end in order did before the
/// line, which a start that no press stands behind reads, where the removal
/// was made, as an end that does not tell. It reads and writes through
/// `run`, the held folder.
pub fn end(
    run: &HeldRun,
    ended_by: Option<StopKind>,
    carried: Option<(StopKind, &LostRecord)>,
) -> io::Result<()> {
    let hold = read_in(run).is_ok_and(|text| text.as_deref().is_some_and(holds_the_hold));
    let mut text = String::new();
    if hold {
        text.push_str(HOLD_LINE);
        text.push('\n');
    }
    if let Some(kind) = ended_by {
        text.push_str(ENDED);
        text.push_str(kind.as_str());
        text.push('\n');
    }
    if let Some((kind, lost)) = carried {
        text.push_str(&lost_line(kind, lost));
    }
    if text.is_empty() {
        return remove_in(run);
    }
    let written = replace_in(run, text.as_bytes());
    if written.is_err() && !hold {
        let _ = remove_in(run);
    }
    written
}

fn ended_in_order(text: &str) -> bool {
    let text = text.trim_end();
    !text.is_empty()
        && text.lines().all(|line| {
            line == HOLD_LINE
                || line
                    .strip_prefix(ENDED)
                    .is_some_and(|word| word.parse::<StopKind>().is_ok())
                || line
                    .strip_prefix(LOST)
                    .is_some_and(|rest| lost_in(rest).is_some())
        })
}

/// The line of the loss of a life that ended behind `kind`, in the words of
/// the status's `last_stop_lost` (contracts.md C2.3) after the kind.
fn lost_line(kind: StopKind, lost: &LostRecord) -> String {
    let second = |at: Option<UtcSecond>| at.map_or_else(|| "-".to_owned(), |at| at.to_string());
    format!(
        "{LOST}{} events={} first={} last={} error={} final={}\n",
        kind.as_str(),
        lost.events,
        second(lost.first),
        second(lost.last),
        lost.error.as_ref().map_or("-", |error| error.as_str()),
        if lost.final_figures { "yes" } else { "no" },
    )
}

/// The loss of the last life in the marker's `text`: its first line of a
/// loss, where that line reads whole.
fn lost_of(text: &str) -> Option<LastLife> {
    text.lines()
        .find_map(|line| line.strip_prefix(LOST))
        .and_then(lost_in)
}

fn lost_in(rest: &str) -> Option<LastLife> {
    let mut words = rest.split(' ');
    let kind = words.next()?.parse::<StopKind>().ok()?;
    let mut field = |name: &str| words.next().and_then(|word| word.strip_prefix(name));
    let events = field("events=")?.parse().ok()?;
    let second = |word: &str| match word {
        "-" => Some(None),
        word => word.parse::<UtcSecond>().ok().map(Some),
    };
    let first = second(field("first=")?)?;
    let last = second(field("last=")?)?;
    let error = match field("error=")? {
        "-" => None,
        word => Some(word.parse().ok()?),
    };
    let final_figures = match field("final=")? {
        "yes" => true,
        "no" => false,
        _ => return None,
    };
    if words.next().is_some() {
        return None;
    }
    Some(LastLife {
        kind,
        lost: LostRecord {
            events,
            first,
            last,
            error,
            final_figures,
        },
    })
}

fn holds_the_hold(text: &str) -> bool {
    text.lines()
        .any(|line| line == HOLD_LINE || line.starts_with(RECORD))
}

fn read(dir: &DataDir) -> io::Result<Option<String>> {
    match std::fs::File::open(dir.run_marker()) {
        Ok(file) => {
            let mut held = Vec::new();
            file.take(MARKER_READ_MAX_BYTES).read_to_end(&mut held)?;
            Ok(Some(String::from_utf8_lossy(&held).into_owned()))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn write(dir: &DataDir, hold: bool) -> io::Result<()> {
    let mut text = format!("{PID}{}\n", std::process::id());
    if hold {
        text.push_str(HOLD_LINE);
        text.push('\n');
    }
    replace(dir, text.as_bytes())
}

fn read_from(file: std::fs::File) -> io::Result<Option<String>> {
    let mut held = Vec::new();
    file.take(MARKER_READ_MAX_BYTES).read_to_end(&mut held)?;
    Ok(Some(String::from_utf8_lossy(&held).into_owned()))
}

#[cfg(target_os = "macos")]
fn name_in(name: &str) -> io::Result<std::ffi::CString> {
    std::ffi::CString::new(name).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))
}

#[cfg(target_os = "macos")]
fn open_in(run: &HeldRun, name: &str, flags: libc::c_int) -> io::Result<std::fs::File> {
    use std::os::fd::{AsRawFd, FromRawFd};
    let name = name_in(name)?;
    // SAFETY: the descriptor of `run` is open across the call, `name` is NUL-terminated and
    // lives across it, and the mode is passed as the unsigned int that openat reads with
    // O_CREAT.
    let fd = unsafe {
        libc::openat(
            run.fd.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600 as libc::c_uint,
        )
    };
    if fd == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `fd` is the descriptor that openat returned and checked, owned by nothing else.
    Ok(unsafe { std::fs::File::from_raw_fd(fd) })
}

#[cfg(target_os = "macos")]
fn sync_run(run: &HeldRun) -> io::Result<()> {
    std::fs::File::from(run.fd.try_clone()?).sync_all()
}

#[cfg(target_os = "macos")]
fn read_in(run: &HeldRun) -> io::Result<Option<String>> {
    match open_in(run, halvo_control::paths::RUN_MARKER, libc::O_RDONLY) {
        Ok(file) => read_from(file),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn replace_in(run: &HeldRun, bytes: &[u8]) -> io::Result<()> {
    replace_named_in(run, halvo_control::paths::RUN_MARKER, bytes)
}

fn remove_in(run: &HeldRun) -> io::Result<()> {
    remove_named_in(run, halvo_control::paths::RUN_MARKER)
}

#[cfg(target_os = "macos")]
fn replace_named_in(run: &HeldRun, name: &str, bytes: &[u8]) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    let next = format!("{name}.new");
    let mut file = open_in(run, &next, libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    let (from, to) = (name_in(&next)?, name_in(name)?);
    let at = run.fd.as_raw_fd();
    // SAFETY: both names are NUL-terminated and live across the call, and `at` is the
    // descriptor that `run` owns and keeps open across it.
    if unsafe { libc::renameat(at, from.as_ptr(), at, to.as_ptr()) } == -1 {
        return Err(io::Error::last_os_error());
    }
    sync_run(run)
}

#[cfg(target_os = "macos")]
fn remove_named_in(run: &HeldRun, name: &str) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    let name = name_in(name)?;
    // SAFETY: the name is NUL-terminated and lives across the call, and the descriptor of
    // `run` is open across it.
    if unsafe { libc::unlinkat(run.fd.as_raw_fd(), name.as_ptr(), 0) } == -1 {
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::NotFound {
            return Ok(());
        }
        return Err(error);
    }
    sync_run(run)
}

#[cfg(target_os = "macos")]
fn absent_in(run: &HeldRun, name: &str) -> bool {
    use std::os::fd::AsRawFd;
    let Ok(name) = name_in(name) else {
        return false;
    };
    // SAFETY: an all-zero stat is a valid value of the plain C struct that fstatat fills.
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: the name is NUL-terminated and lives across the call, the descriptor of `run` is
    // open across it, and `stat` is a writable stat that outlives the call.
    let found = unsafe {
        libc::fstatat(
            run.fd.as_raw_fd(),
            name.as_ptr(),
            &mut stat,
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    found == -1 && io::Error::last_os_error().kind() == io::ErrorKind::NotFound
}

#[cfg(windows)]
fn read_in(run: &HeldRun) -> io::Result<Option<String>> {
    match std::fs::File::open(run.path.join(halvo_control::paths::RUN_MARKER)) {
        Ok(file) => read_from(file),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(windows)]
fn replace_named_in(run: &HeldRun, name: &str, bytes: &[u8]) -> io::Result<()> {
    let path = run.path.join(name);
    let next = run.path.join(format!("{name}.new"));
    let mut file = std::fs::File::create(&next)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&next, &path)
}

#[cfg(windows)]
fn remove_named_in(run: &HeldRun, name: &str) -> io::Result<()> {
    match std::fs::remove_file(run.path.join(name)) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

#[cfg(windows)]
fn absent_in(run: &HeldRun, name: &str) -> bool {
    std::fs::symlink_metadata(run.path.join(name))
        .is_err_and(|error| error.kind() == io::ErrorKind::NotFound)
}

fn replace(dir: &DataDir, bytes: &[u8]) -> io::Result<()> {
    let run = dir.run_dir();
    make_own_folder(&run)?;
    let path = dir.run_marker();
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".new");
    let next = path.with_file_name(name);
    let mut file = std::fs::File::create(&next)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&next, &path)?;
    sync_folder(&run)
}

#[cfg(target_os = "macos")]
fn make_own_folder(run: &Path) -> io::Result<()> {
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(run)?;
    std::fs::set_permissions(run, std::fs::Permissions::from_mode(0o700))
}

#[cfg(windows)]
fn make_own_folder(run: &Path) -> io::Result<()> {
    std::fs::create_dir_all(run)
}

#[cfg(target_os = "macos")]
fn sync_folder(folder: &Path) -> io::Result<()> {
    std::fs::File::open(folder)?.sync_all()
}

#[cfg(windows)]
fn sync_folder(_folder: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> (tempfile::TempDir, DataDir) {
        let root = tempfile::tempdir().unwrap();
        let dir = DataDir::from_flag(root.path().join("data").to_str().unwrap()).unwrap();
        (root, dir)
    }

    fn marker(dir: &DataDir) -> Option<String> {
        std::fs::read_to_string(dir.run_marker()).ok()
    }

    fn run_of(dir: &DataDir) -> HeldRun {
        HeldRun::of(&StateDir::open(dir.path()).unwrap()).unwrap()
    }

    fn stand(dir: &DataDir, text: &str) {
        replace(dir, text.as_bytes()).unwrap();
    }

    #[test]
    fn an_end_in_order_leaves_the_line_of_its_trigger_which_a_start_reads_clean_with_the_hold_as_before(
    ) {
        for kind in [
            StopKind::Stop,
            StopKind::Signal,
            StopKind::Lifeline,
            StopKind::SessionEnd,
            StopKind::DeleteAll,
        ] {
            let (_root, dir) = scratch();
            assert_eq!(begin(&dir).last_stop, LastStop::None, "{kind:?}");
            end(&run_of(&dir), Some(kind), None).unwrap();
            assert_eq!(
                marker(&dir),
                Some(format!("ended={}\n", kind.as_str())),
                "{kind:?}: the end leaves its line in place of the marker"
            );
            assert!(!held(&dir), "{kind:?}: no hold behind an end with none");
            let next = begin(&dir);
            assert_eq!(next.last_stop, LastStop::Clean, "{kind:?}");
            assert!(next.written.is_ok(), "{kind:?}");
            assert!(!held(&dir), "{kind:?}: the next life holds no hold");
        }

        let (_root, dir) = scratch();
        stand(
            &dir,
            "last_stop=source_not_stopped kind=pause final=yes not_stopped=input:timeout\n",
        );
        assert_eq!(begin(&dir).last_stop, LastStop::Unclean);
        assert!(held(&dir), "a record's hold goes on into the next life");
        end(&run_of(&dir), Some(StopKind::Lifeline), None).unwrap();
        assert_eq!(
            marker(&dir).as_deref(),
            Some("hold=yes\nended=lifeline\n"),
            "the hold stands before the line"
        );
        assert!(held(&dir), "the end keeps the hold");
        assert_eq!(begin(&dir).last_stop, LastStop::Clean);
        assert!(held(&dir), "the next life holds the hold");
        stand(&dir, &format!("pid={}\n", std::process::id()));
        assert!(!held(&dir), "a Resume's marker lifts the hold");
        end(&run_of(&dir), Some(StopKind::Signal), None).unwrap();
        assert_eq!(marker(&dir).as_deref(), Some("ended=signal\n"));
        assert_eq!(begin(&dir).last_stop, LastStop::Clean);
        assert!(!held(&dir), "the hold stays lifted");

        for (text, read) in [
            ("hold=yes\n", LastStop::Clean),
            ("ended=bogus\n", LastStop::Unclean),
            ("", LastStop::Unclean),
            ("pid=1\n", LastStop::Unclean),
        ] {
            let (_root, dir) = scratch();
            stand(&dir, text);
            assert_eq!(begin(&dir).last_stop, read, "{text:?}");
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_decision_written_after_the_folder_moved_lands_in_the_held_folder_and_nothing_appears_at_the_name(
    ) {
        let root = tempfile::tempdir().unwrap();
        let (data, moved) = (root.path().join("data"), root.path().join("moved"));
        let dir = DataDir::from_flag(data.to_str().unwrap()).unwrap();
        assert!(begin(&dir).written.is_ok());
        let run = run_of(&dir);
        std::fs::rename(&data, &moved).unwrap();
        std::fs::create_dir(&data).unwrap();
        decided(&run, StopKind::Stop).unwrap();
        let names = |folder: &std::path::Path| -> Vec<String> {
            std::fs::read_dir(folder)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        };
        assert_eq!(names(&data), Vec::<String>::new(), "nothing at the name");
        assert_eq!(
            std::fs::read_to_string(moved.join("run").join("running")).unwrap(),
            format!("pid={}\nended=stop\n", std::process::id()),
            "the line in the held folder"
        );
        at_the_end(&run, halvo_control::exit::CLEAN, Some(StopKind::Stop), None).unwrap();
        assert_eq!(names(&data), Vec::<String>::new(), "nothing at the name");
        assert_eq!(
            std::fs::read_to_string(moved.join("run").join("running")).unwrap(),
            "ended=stop\n",
            "the end's line in the held folder"
        );
    }

    #[test]
    fn a_stop_decided_whose_end_is_cut_off_leaves_its_line_beside_the_process_line_and_a_start_with_no_press_waits(
    ) {
        let pid = format!("pid={}\n", std::process::id());
        let record =
            "last_stop=source_not_stopped kind=pause final=yes not_stopped=input:timeout\n";
        for kind in [StopKind::Stop, StopKind::Signal] {
            for hold in [false, true] {
                let (_root, dir) = scratch();
                if hold {
                    stand(&dir, record);
                }
                assert!(begin(&dir).written.is_ok(), "{kind:?}");
                decided(&run_of(&dir), kind).unwrap();
                let hold_line = if hold { "hold=yes\n" } else { "" };
                assert_eq!(
                    marker(&dir),
                    Some(format!("{pid}{hold_line}ended={}\n", kind.as_str())),
                    "{kind:?}: the end is cut off behind the decision"
                );
                assert_eq!(
                    last_end(&dir).no_press(),
                    NoPress::WaitsForAPress,
                    "{kind:?}: a start with --no-press ends with 7"
                );
                assert_eq!(held(&dir), hold, "{kind:?}: the hold as before");
                assert_eq!(
                    begin(&dir).last_stop,
                    LastStop::Unclean,
                    "{kind:?}: a start without the argument reads unclean, as behind a kill"
                );
            }
        }
        let (_root, dir) = scratch();
        assert!(begin(&dir).written.is_ok());
        decided(&run_of(&dir), StopKind::Stop).unwrap();
        at_the_end(
            &run_of(&dir),
            halvo_control::exit::SOURCE_NOT_STOPPED,
            Some(StopKind::Stop),
            None,
        )
        .unwrap();
        assert_eq!(
            marker(&dir),
            Some(format!("{pid}ended=stop\n")),
            "the shutdown entry's own end with 5 leaves the decision"
        );
        assert_eq!(last_end(&dir).no_press(), NoPress::WaitsForAPress);
        let (_root, dir) = scratch();
        assert!(begin(&dir).written.is_ok());
        decided(&run_of(&dir), StopKind::Stop).unwrap();
        at_the_end(
            &run_of(&dir),
            halvo_control::exit::CLEAN,
            Some(StopKind::Stop),
            None,
        )
        .unwrap();
        assert_eq!(
            marker(&dir).as_deref(),
            Some("ended=stop\n"),
            "an end with 0 behind the decision writes the clean form"
        );
        assert_eq!(begin(&dir).last_stop, LastStop::Clean);
    }

    #[test]
    fn a_stop_or_a_signal_that_ends_with_1_leaves_its_line_beside_the_process_line_and_a_start_with_no_press_waits(
    ) {
        use halvo_control::exit::{CLEAN, ERROR, RESTART, SOURCE_NOT_STOPPED};
        let pid = format!("pid={}\n", std::process::id());
        let record =
            "last_stop=source_not_stopped kind=pause final=yes not_stopped=input:timeout\n";
        for (end, kind) in [
            ("no answer to the last settle", StopKind::Stop),
            ("the drain still fails at the last settle", StopKind::Signal),
            ("events lost between the two settles", StopKind::Stop),
            (
                "the store's owner silent for 1 s at its close",
                StopKind::Signal,
            ),
        ] {
            for hold in [false, true] {
                let (_root, dir) = scratch();
                if hold {
                    stand(&dir, record);
                }
                assert!(begin(&dir).written.is_ok(), "{end}");
                at_the_end(&run_of(&dir), ERROR, Some(kind), None).unwrap();
                let hold_line = if hold { "hold=yes\n" } else { "" };
                assert_eq!(
                    marker(&dir),
                    Some(format!("{pid}{hold_line}ended={}\n", kind.as_str())),
                    "{end}: the line beside the process line"
                );
                assert_eq!(last_end(&dir), LastEnd::Ended(kind), "{end}");
                assert_eq!(last_end(&dir).no_press(), NoPress::WaitsForAPress, "{end}");
                assert_eq!(held(&dir), hold, "{end}: the hold as before");
                assert_eq!(
                    begin(&dir).last_stop,
                    LastStop::Unclean,
                    "{end}: a start without the argument reads unclean, as before"
                );
            }
        }
        for (status, ended_by, stood, left) in [
            (
                CLEAN,
                Some(StopKind::Stop),
                None,
                Some("ended=stop\n".to_owned()),
            ),
            (ERROR, Some(StopKind::Lifeline), None, Some(pid.clone())),
            (ERROR, None, None, Some(pid.clone())),
            (RESTART, None, None, Some(pid.clone())),
            (
                SOURCE_NOT_STOPPED,
                Some(StopKind::Stop),
                Some(record),
                Some(record.to_owned()),
            ),
        ] {
            let (_root, dir) = scratch();
            assert!(begin(&dir).written.is_ok());
            if let Some(stood) = stood {
                stand(&dir, stood);
            }
            at_the_end(&run_of(&dir), status, ended_by, None).unwrap();
            assert_eq!(marker(&dir), left, "{status} {ended_by:?}");
        }
    }

    #[test]
    fn a_start_that_no_press_stands_behind_waits_behind_a_stop_a_signal_or_an_untold_end_and_serves_behind_every_other(
    ) {
        use NoPress::{Serves, WaitsForAPress};
        let rows: [(Option<&str>, LastEnd, NoPress); 14] = [
            (Some("ended=stop\n"), LastEnd::Ended(StopKind::Stop), WaitsForAPress),
            (Some("hold=yes\nended=stop\n"), LastEnd::Ended(StopKind::Stop), WaitsForAPress),
            (Some("ended=signal\n"), LastEnd::Ended(StopKind::Signal), WaitsForAPress),
            (Some("ended=lifeline\n"), LastEnd::Ended(StopKind::Lifeline), Serves),
            (Some("ended=session_end\n"), LastEnd::Ended(StopKind::SessionEnd), Serves),
            (Some("ended=delete_all\n"), LastEnd::Ended(StopKind::DeleteAll), Serves),
            (Some("hold=yes\nended=lifeline\n"), LastEnd::Ended(StopKind::Lifeline), Serves),
            (Some("pid=4242\n"), LastEnd::NotInOrder, Serves),
            (Some("pid=4242\nhold=yes\n"), LastEnd::NotInOrder, Serves),
            (
                Some("last_stop=source_not_stopped kind=lapse final=yes not_stopped=screen:timeout\n"),
                LastEnd::NotInOrder,
                Serves,
            ),
            (None, LastEnd::Untold, WaitsForAPress),
            (Some("hold=yes\n"), LastEnd::Untold, WaitsForAPress),
            (Some("ended=bogus\n"), LastEnd::Untold, WaitsForAPress),
            (Some(""), LastEnd::Untold, WaitsForAPress),
        ];
        for (text, end, does) in rows {
            let (_root, dir) = scratch();
            if let Some(text) = text {
                stand(&dir, text);
            }
            assert_eq!(last_end(&dir), end, "{text:?}");
            assert_eq!(last_end(&dir).no_press(), does, "{text:?}");
            assert_eq!(
                marker(&dir).as_deref(),
                text,
                "{text:?}: the read writes nothing"
            );
        }
        for kind in StopKind::ALL {
            let does = if matches!(*kind, StopKind::Stop | StopKind::Signal) {
                WaitsForAPress
            } else {
                Serves
            };
            assert_eq!(LastEnd::Ended(*kind).no_press(), does, "{kind:?}");
        }

        let (_root, dir) = scratch();
        std::fs::create_dir_all(dir.run_marker()).unwrap();
        assert_eq!(
            last_end(&dir),
            LastEnd::Untold,
            "a marker that cannot be read"
        );
    }

    #[test]
    fn a_run_that_is_no_folder_reads_kept_and_no_run_reads_none_on_both_systems() {
        let (_root, dir) = scratch();
        std::fs::create_dir_all(dir.path()).unwrap();
        let opened = || HeldRun::of(&StateDir::open(dir.path()).unwrap());
        assert!(!typing_kept(&opened()), "no run/ stands: none");
        std::fs::write(dir.run_dir(), "").unwrap();
        assert!(
            typing_kept(&opened()),
            "a run/ that is no folder cannot be read: kept"
        );
    }

    #[test]
    fn the_note_of_typing_off_reads_none_only_where_nothing_stands_at_its_two_names() {
        let (_root, dir) = scratch();
        std::fs::create_dir_all(dir.path()).unwrap();
        let no_run = HeldRun::of(&StateDir::open(dir.path()).unwrap());
        assert!(!typing_kept(&no_run), "no run/ stands, so no note can");
        assert!(
            typing_kept(&Err(io::ErrorKind::PermissionDenied.into())),
            "a run/ that cannot be opened reads kept"
        );
        std::fs::create_dir_all(dir.run_dir()).unwrap();
        let run = run_of(&dir);
        let kept = |run: &HeldRun| typing_kept(&run.again());
        assert!(!kept(&run), "nothing at either name");
        keep_typing_off(&run).unwrap();
        let note = dir.run_dir().join(TYPING_OFF);
        assert_eq!(std::fs::read_to_string(&note).unwrap(), "key_class=off\n");
        assert!(!dir.run_dir().join(TYPING_OFF_NEW).exists());
        assert!(kept(&run));
        let_typing_go(&run).unwrap();
        assert!(!kept(&run) && !note.exists(), "let go");
        for name in [TYPING_OFF, TYPING_OFF_NEW] {
            let at = dir.run_dir().join(name);
            for text in ["key_class=on\n", "", "key_class=off"] {
                std::fs::write(&at, text).unwrap();
                assert!(kept(&run), "{name} holding {text:?}");
                let_typing_go(&run).unwrap();
                assert!(!at.exists(), "{name}");
            }
            std::fs::create_dir(&at).unwrap();
            assert!(kept(&run), "a folder at {name}");
            assert!(let_typing_go(&run).is_err(), "a folder at {name} stays");
            std::fs::remove_dir(&at).unwrap();
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(dir.path().join("nowhere"), &at).unwrap();
                assert!(kept(&run), "a link at {name}");
                let_typing_go(&run).unwrap();
            }
        }
        #[cfg(target_os = "macos")]
        {
            keep_typing_off(&run).unwrap();
            let moved = dir.path().with_file_name("moved");
            std::fs::rename(dir.path(), &moved).unwrap();
            assert!(
                kept(&run),
                "read through the held run/ behind a move of the folder"
            );
            let_typing_go(&run).unwrap();
            assert!(!moved.join("run").join(TYPING_OFF).exists());
        }
    }

    #[test]
    fn an_offline_delete_takes_the_loss_line_out_of_the_marker_and_a_marker_it_cannot_write_leaves_the_delete_as_it_was(
    ) {
        let delete = |dir: &DataDir| {
            let rows = super::super::delete_all::rows();
            let how = super::super::delete_all::Offline {
                rows: &rows,
                select: super::super::custody_policy::of_this_build,
                hands: &super::super::delete_all::SystemHands,
            };
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let status = super::super::delete_all::run(dir, &how, &mut out, &mut err);
            (status, out, err)
        };
        let with_a_loss =
            "pid=1\nhold=yes\nended=stop\nlost=stop events=3 first=- last=- error=- final=yes\n";
        let (_root, dir) = scratch();
        stand(&dir, with_a_loss);
        let taken = delete(&dir);
        assert_eq!(
            taken.0,
            halvo_control::exit::CLEAN,
            "{}",
            String::from_utf8_lossy(&taken.2)
        );
        assert_eq!(
            marker(&dir).as_deref(),
            Some("pid=1\nhold=yes\nended=stop\n"),
            "the loss line goes and every other line stays byte for byte"
        );
        assert_eq!(
            begin(&dir).lost,
            None,
            "no start reads a loss behind the press"
        );

        let (_root, dir) = scratch();
        stand(&dir, "ended=stop\n");
        let before = std::fs::metadata(dir.run_marker())
            .unwrap()
            .modified()
            .unwrap();
        let plain = delete(&dir);
        assert_eq!(plain.0, halvo_control::exit::CLEAN);
        assert_eq!(marker(&dir).as_deref(), Some("ended=stop\n"));
        assert_eq!(
            std::fs::metadata(dir.run_marker())
                .unwrap()
                .modified()
                .unwrap(),
            before,
            "a marker with no loss line is not written"
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let (_root, dir) = scratch();
            stand(&dir, with_a_loss);
            let run = dir.run_dir();
            std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o500)).unwrap();
            let refused = delete(&dir);
            std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o700)).unwrap();
            let report = |out: &[u8]| {
                String::from_utf8_lossy(out)
                    .lines()
                    .skip(1)
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            };
            assert_eq!(
                (refused.0, report(&refused.1), &refused.2),
                (taken.0, report(&taken.1), &taken.2),
                "a marker that cannot be written back leaves the delete's outcome and report as they were"
            );
            assert_eq!(marker(&dir).as_deref(), Some(with_a_loss), "the line stays");
        }
    }
}
