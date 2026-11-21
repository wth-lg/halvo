//! The owner slot on Windows: the channel is two anonymous pipes whose owner's ends the child inherits and finds by
//! their handle values, as it does its two ends of a worker's channel; the start time of a process is the creation
//! time of `GetProcessTimes`; an orphan is ended by `TerminateProcess` on the handle whose creation time was read.

use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{self, ErrorKind, PipeReader, PipeWriter};
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::process::{Child, Command};

use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, SetHandleInformation, ERROR_ACCESS_DENIED, FILETIME, HANDLE,
    HANDLE_FLAG_INHERIT, WAIT_OBJECT_0,
};
use windows_sys::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, TerminateProcess, WaitForSingleObject,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
};

use super::{Holder, Owner, ORPHAN_TERM_WAIT};

/// The slot's end of the channel on which it writes, and the end that its reader thread reads.
pub(super) type Writes = PipeWriter;
pub(super) type Reads = PipeReader;

/// The slot's ends of a new channel.
pub(super) struct Channel {
    pub(super) writes: Writes,
    pub(super) reads: Reads,
}

/// Starts `command` with the owner's ends of two new pipes made inheritable and named by `--channel=<r>,<w>`, and
/// with the two ends of `worker`, where it holds them, made inheritable and named by `--worker=<r>,<w>`. The ends
/// are inheritable from here until the start returns.
pub(super) fn launch(
    command: &mut Command,
    worker: Option<(PipeReader, PipeWriter)>,
) -> io::Result<(Child, Channel)> {
    let (owner_reads, writes) = std::io::pipe()?;
    let (reads, owner_writes) = std::io::pipe()?;
    let ends = [owner_reads.as_raw_handle(), owner_writes.as_raw_handle()];
    let pair = worker.as_ref().map(|(worker_reads, worker_writes)| {
        [worker_reads.as_raw_handle(), worker_writes.as_raw_handle()]
    });
    for end in ends.into_iter().chain(pair.into_iter().flatten()) {
        // SAFETY: the handle is an open end that this process owns; the call sets its inherit flag alone.
        if unsafe { SetHandleInformation(end, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) } == 0 {
            return Err(io::Error::last_os_error());
        }
    }
    command.arg(format!(
        "--channel={},{}",
        ends[0] as usize, ends[1] as usize
    ));
    if let Some(pair) = pair {
        command.arg(format!(
            "--worker={},{}",
            pair[0] as usize, pair[1] as usize
        ));
    }
    let child = command.spawn();
    drop(owner_reads);
    drop(owner_writes);
    drop(worker);
    Ok((child?, Channel { writes, reads }))
}

/// Closes the slot's end on which it writes: the owner reads the end of its channel.
pub(super) fn close(writes: Writes) {
    drop(writes);
}

/// Whether the pid file found at the slot's make may be read: on Windows no owner and no mode is checked.
pub(super) fn private(_meta: &Metadata) -> bool {
    true
}

/// Makes `derived/` where it is missing.
pub(super) fn private_dir(dir: &Path) -> io::Result<()> {
    match fs::create_dir(dir) {
        Err(error) if error.kind() != ErrorKind::AlreadyExists => return Err(error),
        _ => {}
    }
    if fs::symlink_metadata(dir)?.is_dir() {
        Ok(())
    } else {
        Err(ErrorKind::AlreadyExists.into())
    }
}

/// A new file; it fails where any entry stands at the name.
pub(super) fn create_private(path: &Path) -> io::Result<File> {
    OpenOptions::new().write(true).create_new(true).open(path)
}

/// A process handle of `pid` with `access`, closed when it is dropped.
struct Process(HANDLE);

impl Process {
    fn open(pid: u32, access: u32) -> Option<Process> {
        // SAFETY: OpenProcess takes plain values and returns a new handle or null.
        let handle = unsafe { OpenProcess(access, 0, pid) };
        (!handle.is_null()).then_some(Process(handle))
    }

    /// The creation time of the process in microseconds since the epoch.
    fn created(&self) -> Option<u64> {
        let zero = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
        // SAFETY: the handle is open, and the call writes the four FILETIME values that the pointers name.
        let read =
            unsafe { GetProcessTimes(self.0, &mut created, &mut exited, &mut kernel, &mut user) };
        if read == 0 {
            return None;
        }
        let ticks = (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime);
        ticks
            .checked_sub(116_444_736_000_000_000)
            .map(|since| since / 10)
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        // SAFETY: the handle is open and owned by this value alone.
        unsafe { CloseHandle(self.0) };
    }
}

/// The creation time of the process `pid` in microseconds since the epoch, or none.
pub(super) fn creation_time(pid: u32) -> Option<u64> {
    Process::open(pid, PROCESS_QUERY_LIMITED_INFORMATION)?.created()
}

/// Who holds `pid`: a process that this process may open counts as this user's, one whose open is refused as another
/// user's.
pub(super) fn holder(pid: u32) -> Holder {
    if Process::open(pid, PROCESS_QUERY_LIMITED_INFORMATION).is_some() {
        return Holder::ThisUser;
    }
    // SAFETY: GetLastError reads the error of this thread's last call, the OpenProcess above.
    if unsafe { GetLastError() } == ERROR_ACCESS_DENIED {
        Holder::OtherUser
    } else {
        Holder::Nobody
    }
}

/// Ends the orphan `owner` by `TerminateProcess` on the handle whose creation time was read right before. True when
/// no process of that pid and start time runs after the call.
pub(super) fn end(owner: Owner) -> bool {
    let access = PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | PROCESS_SYNCHRONIZE;
    let Some(process) = Process::open(owner.pid, access) else {
        return holder(owner.pid) == Holder::Nobody;
    };
    if process.created() != Some(owner.started) {
        return true;
    }
    let wait = u32::try_from(ORPHAN_TERM_WAIT.as_millis()).unwrap_or(u32::MAX);
    // SAFETY: the handle is open with the rights to end the process and to wait for it.
    unsafe {
        TerminateProcess(process.0, 1) != 0 && WaitForSingleObject(process.0, wait) == WAIT_OBJECT_0
    }
}
