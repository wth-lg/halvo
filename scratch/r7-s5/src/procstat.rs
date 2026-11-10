//! Runs a child process to its end and reports its wall time and peak resident set.

use std::io::{self, Read};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub struct Measured {
    /// `None` when the child was ended by a signal.
    pub exit_code: Option<i32>,
    /// From just before the spawn call to just after the child was reaped: process
    /// creation, loading, the child's own work and its exit.
    pub wall: Duration,
    /// `ru_maxrss` on macOS, the peak working set on Windows.
    pub peak_rss_bytes: u64,
    pub stdout: String,
}

/// The child's standard error goes straight to ours.
pub fn run(command: &mut Command) -> io::Result<Measured> {
    command.stdin(Stdio::null()).stdout(Stdio::piped());
    let started = Instant::now();
    let mut child = command.spawn()?;
    let mut stdout = Vec::new();
    if let Some(mut pipe) = child.stdout.take() {
        // The child closes its end when it exits, so this returns at exit.
        pipe.read_to_end(&mut stdout)?;
    }
    let (exit_code, peak_rss_bytes) = reap(&mut child)?;
    let wall = started.elapsed();
    Ok(Measured {
        exit_code,
        wall,
        peak_rss_bytes,
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
    })
}

#[cfg(unix)]
fn reap(child: &mut Child) -> io::Result<(Option<i32>, u64)> {
    let pid = child.id() as libc::pid_t;
    let mut status = 0;
    // SAFETY: `rusage` is plain integers, for which all-zero is a valid value.
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    loop {
        // SAFETY: `status` and `usage` are valid for writes, and `pid` is our own child
        // that nothing else waits for.
        let reaped = unsafe { libc::wait4(pid, &mut status, 0, &mut usage) };
        if reaped == pid {
            break;
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
    let exit_code = libc::WIFEXITED(status).then(|| libc::WEXITSTATUS(status));
    // macOS reports ru_maxrss in bytes, other Unix systems in kibibytes.
    let unit = if cfg!(target_os = "macos") { 1 } else { 1024 };
    Ok((exit_code, usage.ru_maxrss as u64 * unit))
}

#[cfg(windows)]
fn reap(child: &mut Child) -> io::Result<(Option<i32>, u64)> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };

    let status = child.wait()?;
    let size = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
    // SAFETY: the struct is plain integers, for which all-zero is a valid value.
    let mut counters: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
    counters.cb = size;
    // SAFETY: the handle stays open until `child` is dropped, and `counters` is valid
    // for writes of `size` bytes.
    let ok = unsafe { K32GetProcessMemoryInfo(child.as_raw_handle(), &mut counters, size) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((status.code(), counters.PeakWorkingSetSize as u64))
}
