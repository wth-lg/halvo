#![allow(unsafe_code)]
#![deny(clippy::undocumented_unsafe_blocks)]

use std::ffi::{c_void, OsString};
use std::io;
use std::mem::{size_of, MaybeUninit};
use std::os::unix::ffi::OsStringExt;
use std::os::unix::process::CommandExt;
use std::process::Command;

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

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct ProcFdInfo {
    proc_fd: i32,
    proc_fdtype: u32,
}

const _: () = assert!(size_of::<ProcFdInfo>() == 8);

const PROC_PIDLISTFDS: i32 = 1;
const PROC_PIDTBSDINFO: i32 = 3;
const CS_DARWIN_USER_TEMP_DIR: i32 = 65537;
const F_SETFD: i32 = 2;
const FD_CLOEXEC: i32 = 1;
const EMFILE: i32 = 24;
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
    fn fcntl(fd: i32, cmd: i32, ...) -> i32;
    fn confstr(name: i32, buf: *mut c_void, len: usize) -> usize;
}

/// The per-user temporary directory that the system answers by `confstr`, read from no variable of this process;
/// none when it cannot be read.
pub(crate) fn user_temp_dir() -> Option<OsString> {
    // SAFETY: with a null buffer and a length of 0, confstr writes nothing and returns the length that it needs.
    let len = unsafe { confstr(CS_DARWIN_USER_TEMP_DIR, std::ptr::null_mut(), 0) };
    if len < 2 {
        return None;
    }
    let mut buf = vec![0u8; len];
    // SAFETY: `buf` is live and writable for `buf.len()` bytes, the length passed, and confstr writes at most that.
    let got = unsafe { confstr(CS_DARWIN_USER_TEMP_DIR, buf.as_mut_ptr().cast(), buf.len()) };
    if got != len {
        return None;
    }
    buf.truncate(len - 1);
    Some(OsString::from_vec(buf))
}

/// Makes the process that `cmd` starts close, at its exec and so before it runs, every descriptor beyond its
/// standard input, output and error. The child reads its list of open descriptors after the fork, into room that
/// this process makes beforehand for twice as many as it holds now; a list that cannot be read, that fills the room
/// or a descriptor that cannot be marked fails the start, so that no start goes on with a descriptor left open.
/// Two limits: no test makes the list call or a mark fail in the child, because nothing can make them fail there,
/// so the tests hold `listed` and `mark_close_on_exec` instead; and no test holds that the room is twice the count:
/// the doubling guards a start against descriptors that other threads open between the count and the fork, and a
/// start that outruns its room fails closed.
pub(crate) fn close_beyond_three(cmd: &mut Command) {
    let own = i32::try_from(std::process::id()).unwrap_or(0);
    // SAFETY: with a null buffer and a size of 0, proc_pidinfo writes nothing and returns the size that the list needs.
    let held = unsafe { proc_pidinfo(own, PROC_PIDLISTFDS, 0, std::ptr::null_mut(), 0) };
    let room = usize::try_from(held).unwrap_or(0) / size_of::<ProcFdInfo>() * 2 + 64;
    let mut list = vec![ProcFdInfo::default(); room];
    // SAFETY: the step runs in the child between fork and exec, where only async-signal-safe calls may be made: it calls getpid, proc_pidinfo and fcntl alone, allocates nothing, since `list` is made here beforehand and moved in, and takes no lock.
    unsafe {
        cmd.pre_exec(move || close_at_exec_beyond_three(&mut list));
    }
}

fn close_at_exec_beyond_three(list: &mut [ProcFdInfo]) -> io::Result<()> {
    let room = i32::try_from(std::mem::size_of_val(list))
        .map_err(|_| io::Error::from_raw_os_error(EMFILE))?;
    let pid = i32::try_from(std::process::id()).map_err(|_| io::Error::from_raw_os_error(ESRCH))?;
    // SAFETY: `list` is live and writable for `room` bytes, the size passed, and the call writes at most that.
    let got = unsafe { proc_pidinfo(pid, PROC_PIDLISTFDS, 0, list.as_mut_ptr().cast(), room) };
    let open = listed(got, room)?;
    mark_close_on_exec(&list[..open])
}

/// The count of entries in `got`, the answer of a list call into `room` bytes: an answer of 0 or less is no list,
/// and an answer that fills the room may have been cut short, so both refuse. Allocates nothing and takes no lock.
fn listed(got: i32, room: i32) -> io::Result<usize> {
    if got <= 0 || got >= room {
        return Err(io::Error::from_raw_os_error(EMFILE));
    }
    Ok(usize::try_from(got).unwrap_or(0) / size_of::<ProcFdInfo>())
}

/// Marks close-on-exec each descriptor of `entries` beyond the standard three; the first that cannot be marked
/// gives its error. Allocates nothing and takes no lock.
fn mark_close_on_exec(entries: &[ProcFdInfo]) -> io::Result<()> {
    for entry in entries {
        // SAFETY: F_SETFD takes a descriptor number and an int flag, and the call reads no memory of this process.
        if entry.proc_fd > 2 && unsafe { fcntl(entry.proc_fd, F_SETFD, FD_CLOEXEC) } == -1 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
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
    // SAFETY: `buf` is live and writable for `size` bytes, the size of the struct passed, and the call writes at most that.
    let got = unsafe { proc_pidinfo(p, PROC_PIDTBSDINFO, 0, buf.as_mut_ptr().cast(), size) };
    if got != size {
        return match io::Error::last_os_error().raw_os_error() {
            Some(ESRCH) => Read::Gone,
            _ => Read::Unknown,
        };
    }
    // SAFETY: the call above wrote all `size` bytes of `buf`, as `got == size` shows, and every field of the struct is plain data.
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
    // SAFETY: `info` is live and writable for 128 bytes, more than the siginfo_t that waitid writes, and WNOWAIT leaves the child unreaped.
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
        // SAFETY: kill takes two ints and touches no memory of this process; as_pid keeps 0, -1 and other forms that kill reads otherwise out.
        Some(g) => unsafe { kill(-g, SIGKILL) == 0 },
        None => false,
    }
}

/// Sends SIGSTOP to the process group `pgid`, with the guard of `kill_group`.
pub(crate) fn stop_group(pgid: u32) -> bool {
    match as_pid(pgid) {
        // SAFETY: kill takes two ints and touches no memory of this process; as_pid keeps 0, -1 and other forms that kill reads otherwise out.
        Some(g) => unsafe { kill(-g, SIGSTOP) == 0 },
        None => false,
    }
}

/// Sends SIGKILL to the one process `pid`, never to a number below 2 or above the largest pid.
pub(crate) fn kill_pid(pid: u32) -> bool {
    match as_pid(pid) {
        // SAFETY: kill takes two ints and touches no memory of this process; as_pid keeps 0, 1 and numbers beyond the largest pid out.
        Some(p) => unsafe { kill(p, SIGKILL) == 0 },
        None => false,
    }
}

/// Every process of the table whose information can be read.
pub(crate) fn table() -> io::Result<Vec<Info>> {
    // SAFETY: with a null buffer and a size of 0, proc_listallpids writes nothing and returns the count of processes.
    let needed = unsafe { proc_listallpids(std::ptr::null_mut(), 0) };
    if needed <= 0 {
        return Err(io::Error::last_os_error());
    }
    let mut pids = vec![0i32; usize::try_from(needed).unwrap_or(0) + 512];
    let bytes = i32::try_from(pids.len() * size_of::<i32>()).unwrap_or(i32::MAX);
    // SAFETY: `pids` is live and writable for `bytes` bytes, the size passed, and the call writes at most that.
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
    // SAFETY: `buf` is live and writable for `buf.len()` bytes, the length passed, which is at most the 256 that getentropy takes.
    if unsafe { getentropy(buf.as_mut_ptr().cast(), buf.len()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EBADF: i32 = 9;

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
    fn a_list_answer_of_0_or_less_or_one_that_fills_its_room_refuses_and_one_entry_less_passes() {
        let room = 100 * size_of::<ProcFdInfo>() as i32;
        let entry = size_of::<ProcFdInfo>() as i32;
        let refused = |got| listed(got, room).map_err(|e| e.raw_os_error());
        assert_eq!(refused(0), Err(Some(EMFILE)), "an answer of 0");
        assert_eq!(refused(-1), Err(Some(EMFILE)), "an answer below 0");
        assert_eq!(
            refused(room),
            Err(Some(EMFILE)),
            "an answer that fills the room"
        );
        assert_eq!(
            refused(room - entry).ok(),
            Some(99),
            "one entry less than the room"
        );
    }

    #[test]
    fn a_number_that_cannot_be_marked_gives_its_error() {
        let not_open = ProcFdInfo {
            proc_fd: i32::MAX,
            proc_fdtype: 0,
        };
        assert_eq!(
            mark_close_on_exec(&[not_open]).map_err(|e| e.raw_os_error()),
            Err(Some(EBADF)),
            "a number above the limit of open files, which no descriptor of this process has"
        );
    }

    #[test]
    fn a_list_of_descriptors_that_fills_its_room_fails_the_start() {
        let mut room = [ProcFdInfo::default(); 1];
        let refused = close_at_exec_beyond_three(&mut room);
        assert_eq!(
            refused.map_err(|e| e.raw_os_error()),
            Err(Some(EMFILE)),
            "a process holds at least its three standard descriptors, which do not fit one entry"
        );
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
