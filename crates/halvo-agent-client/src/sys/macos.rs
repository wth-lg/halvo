#![allow(unsafe_code)]

use std::ffi::c_void;
use std::io;
use std::mem::{size_of, MaybeUninit};

#[repr(C)]
#[derive(Clone, Copy)]
struct ProcBsdInfo {
    pbi_flags: u32,
    pbi_status: u32,
    pbi_xstatus: u32,
    pbi_pid: u32,
    pbi_ppid: u32,
    pbi_uid: u32,
    pbi_gid: u32,
    pbi_ruid: u32,
    pbi_rgid: u32,
    pbi_svuid: u32,
    pbi_svgid: u32,
    rfu_1: u32,
    pbi_comm: [u8; 16],
    pbi_name: [u8; 32],
    pbi_nfiles: u32,
    pbi_pgid: u32,
    pbi_pjobc: u32,
    e_tdev: u32,
    e_tpgid: u32,
    pbi_nice: i32,
    pbi_start_tvsec: u64,
    pbi_start_tvusec: u64,
}

const _: () = assert!(size_of::<ProcBsdInfo>() == 136);

const PROC_PIDTBSDINFO: i32 = 3;
const SZOMB: u32 = 5;
const SIGKILL: i32 = 9;
const SIGSTOP: i32 = 17;
const ESRCH: i32 = 3;

const P_PID: i32 = 1;
const WNOHANG: i32 = 0x01;
const WEXITED: i32 = 0x04;
const WNOWAIT: i32 = 0x20;
const ECHILD: i32 = 10;

extern "C" {
    fn waitid(idtype: i32, id: u32, info: *mut c_void, options: i32) -> i32;
    fn proc_pidinfo(pid: i32, flavor: i32, arg: u64, buffer: *mut c_void, size: i32) -> i32;
    fn proc_listallpids(buffer: *mut c_void, size: i32) -> i32;
    fn kill(pid: i32, sig: i32) -> i32;
    fn getentropy(buffer: *mut c_void, len: usize) -> i32;
}

/// What the process table holds for one process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Info {
    pub pid: u32,
    pub ppid: u32,
    pub pgid: u32,
    pub started_us: u64,
    pub zombie: bool,
}

/// One read of the process table for a pid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Read {
    Found(Info),
    Gone,
    Unknown,
}

fn as_pid(n: u32) -> Option<i32> {
    i32::try_from(n).ok().filter(|p| *p > 1)
}

pub(crate) fn info(pid: u32) -> Read {
    let Some(p) = as_pid(pid) else {
        return Read::Unknown;
    };
    let mut buf = MaybeUninit::<ProcBsdInfo>::zeroed();
    let size = size_of::<ProcBsdInfo>() as i32;
    let got = unsafe { proc_pidinfo(p, PROC_PIDTBSDINFO, 0, buf.as_mut_ptr().cast(), size) };
    if got != size {
        return match io::Error::last_os_error().raw_os_error() {
            Some(ESRCH) => Read::Gone,
            _ => Read::Unknown,
        };
    }
    let i = unsafe { buf.assume_init() };
    let started_us = i
        .pbi_start_tvsec
        .checked_mul(1_000_000)
        .and_then(|s| s.checked_add(i.pbi_start_tvusec));
    match started_us {
        Some(started_us) if i.pbi_pid == pid => Read::Found(Info {
            pid,
            ppid: i.pbi_ppid,
            pgid: i.pbi_pgid,
            started_us,
            zombie: i.pbi_status == SZOMB,
        }),
        _ => Read::Unknown,
    }
}

/// The state of a child of this process, read without reaping it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ChildState {
    /// It runs, so its number is its own.
    Running,
    /// It has exited and is not reaped, so its number and its group's number are still held.
    Exited,
    /// It is reaped, by this process or by the system for a parent that ignores SIGCHLD, or it is no child; its
    /// number may belong to another process.
    Gone,
}

pub(crate) fn child_state(pid: u32) -> ChildState {
    let mut info = [0u64; 16];
    let r = unsafe {
        waitid(
            P_PID,
            pid,
            info.as_mut_ptr().cast(),
            WEXITED | WNOHANG | WNOWAIT,
        )
    };
    if r != 0 {
        return match io::Error::last_os_error().raw_os_error() {
            Some(ECHILD) => ChildState::Gone,
            _ => ChildState::Running,
        };
    }
    let si_pid = (info[1] >> 32) as u32;
    if si_pid == pid {
        ChildState::Exited
    } else {
        ChildState::Running
    }
}

/// Sends SIGKILL to the process group `pgid`, never to a group below 2 or above the largest pid: `kill` reads
/// 0, -1 and a negative number of another form as the caller's own group or as every process.
pub(crate) fn kill_group(pgid: u32) -> bool {
    match as_pid(pgid) {
        Some(g) => unsafe { kill(-g, SIGKILL) == 0 },
        None => false,
    }
}

/// Sends SIGSTOP to the process group `pgid`, with the guard of `kill_group`.
pub(crate) fn stop_group(pgid: u32) -> bool {
    match as_pid(pgid) {
        Some(g) => unsafe { kill(-g, SIGSTOP) == 0 },
        None => false,
    }
}

/// Sends SIGKILL to the one process `pid`, never to a number below 2 or above the largest pid.
pub(crate) fn kill_pid(pid: u32) -> bool {
    match as_pid(pid) {
        Some(p) => unsafe { kill(p, SIGKILL) == 0 },
        None => false,
    }
}

/// Every process of the table whose information can be read.
pub(crate) fn table() -> io::Result<Vec<Info>> {
    let needed = unsafe { proc_listallpids(std::ptr::null_mut(), 0) };
    if needed <= 0 {
        return Err(io::Error::last_os_error());
    }
    let mut pids = vec![0i32; usize::try_from(needed).unwrap_or(0) + 512];
    let bytes = i32::try_from(pids.len() * size_of::<i32>()).unwrap_or(i32::MAX);
    let got = unsafe { proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
    if got <= 0 {
        return Err(io::Error::last_os_error());
    }
    pids.truncate(usize::try_from(got).unwrap_or(0));
    Ok(pids
        .into_iter()
        .filter_map(|p| u32::try_from(p).ok())
        .filter_map(|p| match info(p) {
            Read::Found(i) => Some(i),
            _ => None,
        })
        .collect())
}

pub(crate) fn random16() -> io::Result<[u8; 16]> {
    let mut buf = [0u8; 16];
    if unsafe { getentropy(buf.as_mut_ptr().cast(), buf.len()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pid_guard_takes_neither_0_nor_1_nor_a_number_beyond_the_largest_pid() {
        assert_eq!(as_pid(0), None);
        assert_eq!(as_pid(1), None);
        assert_eq!(as_pid(2), Some(2));
        assert_eq!(as_pid(2_147_483_647), Some(i32::MAX));
        assert_eq!(as_pid(2_147_483_648), None);
        assert_eq!(as_pid(u32::MAX), None);
    }

    #[test]
    fn a_child_reads_running_then_exited_while_unreaped_then_gone() {
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("0.3")
            .spawn()
            .unwrap();
        let pid = child.id();
        assert_eq!(child_state(pid), ChildState::Running);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while child_state(pid) == ChildState::Running && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(child_state(pid), ChildState::Exited);
        assert_eq!(child_state(pid), ChildState::Exited);
        assert!(child.wait().unwrap().success());
        assert_eq!(child_state(pid), ChildState::Gone);
        assert_eq!(child_state(std::process::id()), ChildState::Gone);
    }
}
