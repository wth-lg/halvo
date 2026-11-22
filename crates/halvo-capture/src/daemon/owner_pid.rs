//! The owner pid file of a row of the registry and the start time of a
//! process, in every build: the delete walk (`delete_all`, which names them
//! again) and the owner slot of the workflows run read them.

use std::io;
use std::path::Path;

/// The process that a row's owner pid file names: its pid and its start
/// time in microseconds since the epoch, as [`start_time_of`] reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Owner {
    pub pid: u32,
    pub started: u64,
}

/// Reads the owner pid file at `file`, one line
/// `pid=<n> started=<start time in microseconds since the epoch>`: `None`
/// when no file stands there, an error of the kind InvalidData when the
/// file does not hold that line or names the pid 0 or 1, which no signal may
/// reach.
pub fn owner_of(file: &Path) -> io::Result<Option<Owner>> {
    let text = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let unreadable = || io::Error::from(io::ErrorKind::InvalidData);
    let line = text.strip_suffix('\n').unwrap_or(&text);
    let (pid, started) = line.split_once(' ').ok_or_else(unreadable)?;
    let pid = pid.strip_prefix("pid=").ok_or_else(unreadable)?;
    let started = started.strip_prefix("started=").ok_or_else(unreadable)?;
    match (pid.parse(), started.parse()) {
        (Ok(pid), Ok(started)) if pid > 1 => Ok(Some(Owner { pid, started })),
        _ => Err(unreadable()),
    }
}

/// The start time of the process `pid` in microseconds since the epoch, as
/// the system's process table records it (on macOS `proc_pidinfo` with
/// `PROC_PIDTBSDINFO`, its fields `pbi_start_tvsec` and `pbi_start_tvusec`,
/// the wall clock at the process's start), or `None` when no process of that
/// pid runs or the table cannot be read.
#[cfg(target_os = "macos")]
pub fn start_time_of(pid: u32) -> Option<u64> {
    let pid = libc::c_int::try_from(pid).ok()?;
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size = libc::c_int::try_from(std::mem::size_of::<libc::proc_bsdinfo>()).ok()?;
    // SAFETY: `info` points to writable memory of `size` bytes, the size of
    // the record that PROC_PIDTBSDINFO fills.
    let filled = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if filled != size {
        return None;
    }
    // SAFETY: proc_pidinfo filled the whole record.
    let info = unsafe { info.assume_init() };
    Some(
        info.pbi_start_tvsec
            .saturating_mul(1_000_000)
            .saturating_add(info.pbi_start_tvusec),
    )
}

/// The start time of the process `pid` in microseconds since the epoch, as
/// Windows records it (its creation time, read with `GetProcessTimes` on a
/// handle opened by the pid), or `None` when no process of that pid can be
/// opened or its times cannot be read.
#[cfg(windows)]
pub fn start_time_of(pid: u32) -> Option<u64> {
    use windows::Win32::System::Threading::PROCESS_QUERY_LIMITED_INFORMATION;
    ProcessHandle::open(pid, PROCESS_QUERY_LIMITED_INFORMATION)?.started()
}

/// The start time of a process: no build of another system reads it.
#[cfg(not(any(target_os = "macos", windows)))]
pub fn start_time_of(_pid: u32) -> Option<u64> {
    None
}

/// One handle of a process, opened by its pid and closed when it drops.
#[cfg(windows)]
pub struct ProcessHandle(windows::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl ProcessHandle {
    /// Opens the process `pid` with `access`: `None` when no process of that
    /// pid runs or the open is refused.
    pub fn open(
        pid: u32,
        access: windows::Win32::System::Threading::PROCESS_ACCESS_RIGHTS,
    ) -> Option<ProcessHandle> {
        // SAFETY: OpenProcess takes plain values and returns a new handle,
        // which this value owns from here on, or an error.
        unsafe { windows::Win32::System::Threading::OpenProcess(access, false, pid) }
            .ok()
            .map(ProcessHandle)
    }

    /// The creation time of the process in microseconds since the epoch,
    /// read on this handle.
    pub fn started(&self) -> Option<u64> {
        use windows::Win32::Foundation::FILETIME;
        let (mut created, mut exited, mut kernel, mut user) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        // SAFETY: the handle is open, and the call writes the four FILETIME
        // values that the pointers name, each of which lives across the call.
        unsafe {
            windows::Win32::System::Threading::GetProcessTimes(
                self.0,
                &mut created,
                &mut exited,
                &mut kernel,
                &mut user,
            )
        }
        .ok()?;
        let ticks = (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime);
        ticks
            .checked_sub(116_444_736_000_000_000)
            .map(|since| since / 10)
    }

    /// Ends the process with the exit status `status` and waits at most
    /// `bound` for it to end: true when it has ended within the bound, also
    /// when it ended by itself before the call. The handle needs the rights
    /// to end the process and to wait for it.
    pub fn end_within(&self, status: u32, bound: std::time::Duration) -> bool {
        use windows::Win32::Foundation::WAIT_OBJECT_0;
        use windows::Win32::System::Threading::{TerminateProcess, WaitForSingleObject};
        let wait = u32::try_from(bound.as_millis()).unwrap_or(u32::MAX);
        // SAFETY: the handle is open with the right to end the process.
        let _ = unsafe { TerminateProcess(self.0, status) };
        // SAFETY: the handle is open with the right to wait for the process.
        unsafe { WaitForSingleObject(self.0, wait) == WAIT_OBJECT_0 }
    }
}

#[cfg(windows)]
impl Drop for ProcessHandle {
    fn drop(&mut self) {
        // SAFETY: the handle is open and owned by this value alone.
        let _ = unsafe { windows::Win32::Foundation::CloseHandle(self.0) };
    }
}
