//! The lifeline of contracts.md C14 item 2: standard input is the read end of
//! a pipe whose write end the app alone holds, and the parent that started
//! the run is the one that the start reads. On macOS the start reads the
//! parent's pid; on Windows it opens the parent by its pid and holds that
//! handle, whose start time it holds against its own.

use std::io::{ErrorKind, Read, Write};
#[cfg(target_os = "macos")]
use std::os::unix::fs::FileTypeExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::ParentWatch;

/// Why a lifeline did not start.
#[derive(Debug)]
pub enum Refused {
    /// Standard input is no pipe.
    NotAPipe,
    /// The parent was gone before the start: on macOS the process was handed
    /// to launchd (pid 1); on Windows no process runs at the parent's pid,
    /// the parent has ended, or the process there started after this one.
    ParentGone,
    /// The watch did not start: its thread, or on Windows the open of the
    /// parent or the read of a start time.
    Watcher(std::io::Error),
}

/// The lifeline of a run started with `--lifeline`: a watcher thread reads
/// standard input until its end of file or an error, and every read of
/// [`ParentWatch::ended`] also reads the parent: on macOS its pid against the
/// one of the start, for a write end that leaked into another process, and on
/// Windows the handle of the parent, which is signalled once it has ended.
pub struct Lifeline {
    closed: Arc<AtomicBool>,
    parent: u32,
    #[cfg(windows)]
    handle: crate::daemon::owner_pid::ProcessHandle,
}

impl Lifeline {
    /// Starts the watch: standard input must be a pipe, and a parent that is
    /// gone already refuses it.
    #[cfg(target_os = "macos")]
    pub fn start() -> Result<Lifeline, Refused> {
        if !input_is_a_pipe() {
            return Err(Refused::NotAPipe);
        }
        let parent = std::os::unix::process::parent_id();
        if parent == 1 {
            return Err(Refused::ParentGone);
        }
        let closed = watch_the_input()?;
        Ok(Lifeline { closed, parent })
    }

    /// Starts the watch: standard input must be a pipe; the parent's pid
    /// comes from the process table, and the start opens the parent by it
    /// to wait on. A pid that names no process, a parent that has ended and
    /// a process that started after this one are a parent that is gone.
    #[cfg(windows)]
    pub fn start() -> Result<Lifeline, Refused> {
        use crate::daemon::owner_pid::{start_time_of, Opened, ProcessHandle};
        use windows::Win32::System::Threading::{
            PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
        };
        if !input_is_a_pipe() {
            return Err(Refused::NotAPipe);
        }
        let parent = parent_pid().map_err(Refused::Watcher)?;
        if parent == 0 {
            return Err(Refused::ParentGone);
        }
        let access = PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION;
        let handle = match ProcessHandle::open_or_refused(parent, access) {
            Opened::Process(handle) => handle,
            Opened::NoProcess => return Err(Refused::ParentGone),
            Opened::Refused => {
                return Err(Refused::Watcher(ErrorKind::PermissionDenied.into()));
            }
        };
        let unread = || Refused::Watcher(std::io::Error::other("a start time cannot be read"));
        let own = start_time_of(std::process::id()).ok_or_else(unread)?;
        let theirs = handle.started().ok_or_else(unread)?;
        if theirs > own || handle.has_ended() {
            return Err(Refused::ParentGone);
        }
        let closed = watch_the_input()?;
        Ok(Lifeline {
            closed,
            parent,
            handle,
        })
    }

    /// The parent's pid as the start read it: the exclusion root of the run.
    pub fn parent(&self) -> u32 {
        self.parent
    }
}

impl ParentWatch for Lifeline {
    #[cfg(target_os = "macos")]
    fn ended(&self) -> bool {
        self.closed.load(Ordering::SeqCst) || std::os::unix::process::parent_id() != self.parent
    }

    #[cfg(windows)]
    fn ended(&self) -> bool {
        self.closed.load(Ordering::SeqCst) || self.handle.has_ended()
    }
}

/// Starts the thread that reads standard input until its end of file or an
/// error, and answers the flag that it sets then.
fn watch_the_input() -> Result<Arc<AtomicBool>, Refused> {
    let closed = Arc::new(AtomicBool::new(false));
    let watched = closed.clone();
    std::thread::Builder::new()
        .name("halvo-lifeline".to_owned())
        .spawn(move || {
            let mut input = std::io::stdin().lock();
            let mut bytes = [0_u8; 64];
            loop {
                match input.read(&mut bytes) {
                    Ok(0) => break,
                    Ok(_) => {}
                    Err(error) if error.kind() == ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
            watched.store(true, Ordering::SeqCst);
        })
        .map_err(Refused::Watcher)?;
    Ok(closed)
}

#[cfg(target_os = "macos")]
fn input_is_a_pipe() -> bool {
    use std::os::fd::AsFd;
    std::io::stdin()
        .as_fd()
        .try_clone_to_owned()
        .map(std::fs::File::from)
        .and_then(|input| input.metadata())
        .is_ok_and(|meta| meta.file_type().is_fifo())
}

#[cfg(windows)]
fn input_is_a_pipe() -> bool {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{GetFileType, FILE_TYPE_PIPE};
    let input = HANDLE(std::io::stdin().as_raw_handle());
    // SAFETY: GetFileType reads the type of the handle that it is given and
    // answers FILE_TYPE_UNKNOWN for a null or closed one; no memory is passed.
    unsafe { GetFileType(input) == FILE_TYPE_PIPE }
}

/// The pid of the process that started this one, from a Toolhelp snapshot of
/// the process table: the parent pid of this process's own entry.
#[cfg(windows)]
fn parent_pid() -> std::io::Result<u32> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    let own = std::process::id();
    // SAFETY: the call takes plain values and returns a new snapshot handle,
    // which is closed below, or an error.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }?;
    let mut entry = PROCESSENTRY32W {
        dwSize: u32::try_from(std::mem::size_of::<PROCESSENTRY32W>()).unwrap_or(u32::MAX),
        ..Default::default()
    };
    // SAFETY: the snapshot is open, and `entry` is a live PROCESSENTRY32W whose
    // dwSize holds its size, which the call fills.
    let mut read = unsafe { Process32FirstW(snapshot, &mut entry) };
    let mut parent = None;
    while read.is_ok() {
        if entry.th32ProcessID == own {
            parent = Some(entry.th32ParentProcessID);
            break;
        }
        // SAFETY: as for Process32FirstW above.
        read = unsafe { Process32NextW(snapshot, &mut entry) };
    }
    // SAFETY: the snapshot is open and owned here alone.
    let _ = unsafe { CloseHandle(snapshot) };
    parent.ok_or_else(|| std::io::Error::other("this process is not in the process table"))
}

/// The lifeline of the mode `--service-run --lifeline`, or the status that
/// ends the mode before its store opens: 64 for a standard input that is no
/// pipe, 0 for a parent that is gone already and 1 for a watch that did not
/// start, each but the second with its line on `err`.
pub fn start_or_status(err: &mut dyn Write) -> Result<Lifeline, i32> {
    match Lifeline::start() {
        Ok(lifeline) => Ok(lifeline),
        Err(Refused::NotAPipe) => {
            let _ = writeln!(
                err,
                "halvo-capture: --lifeline needs a pipe on standard input"
            );
            Err(halvo_control::exit::USAGE)
        }
        Err(Refused::ParentGone) => Err(halvo_control::exit::CLEAN),
        Err(Refused::Watcher(error)) => {
            let _ = writeln!(
                err,
                "halvo-capture: the lifeline's watcher did not start: {}",
                error.kind()
            );
            Err(halvo_control::exit::ERROR)
        }
    }
}
