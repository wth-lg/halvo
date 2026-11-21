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

/// The start time of a process: no build of another system reads it yet.
#[cfg(not(target_os = "macos"))]
pub fn start_time_of(_pid: u32) -> Option<u64> {
    None
}
