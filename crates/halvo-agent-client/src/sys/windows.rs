#![allow(unsafe_code)]

use std::ffi::c_void;
use std::io;
use std::mem::size_of;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};

type Handle = *mut c_void;
type Bool = i32;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FileTime {
    low: u32,
    high: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct BasicLimit {
    per_process_user_time: i64,
    per_job_user_time: i64,
    limit_flags: u32,
    minimum_working_set: usize,
    maximum_working_set: usize,
    active_process_limit: u32,
    affinity: usize,
    priority_class: u32,
    scheduling_class: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct IoCounters {
    counts: [u64; 6],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct ExtendedLimit {
    basic: BasicLimit,
    io: IoCounters,
    process_memory_limit: usize,
    job_memory_limit: usize,
    peak_process_memory: usize,
    peak_job_memory: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct BasicAccounting {
    total_user_time: i64,
    total_kernel_time: i64,
    period_user_time: i64,
    period_kernel_time: i64,
    page_faults: u32,
    total_processes: u32,
    active_processes: u32,
    terminated_processes: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct ThreadEntry {
    size: u32,
    usage: u32,
    thread_id: u32,
    owner_pid: u32,
    base_priority: i32,
    delta_priority: i32,
    flags: u32,
}

#[cfg(target_pointer_width = "64")]
const _: () = assert!(size_of::<ExtendedLimit>() == 144);
const _: () = assert!(size_of::<BasicAccounting>() == 48);
const _: () = assert!(size_of::<ThreadEntry>() == 28);

const TH32CS_SNAPTHREAD: u32 = 0x0000_0004;
const THREAD_SUSPEND_RESUME: u32 = 0x0002;
const INVALID_HANDLE_VALUE: Handle = usize::MAX as Handle;

const JOB_OBJECT_BASIC_ACCOUNTING: i32 = 1;
const JOB_OBJECT_EXTENDED_LIMIT: i32 = 9;
const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x2000;
const PROCESS_TERMINATE: u32 = 0x0001;
const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
const SYNCHRONIZE: u32 = 0x0010_0000;
const ERROR_INVALID_PARAMETER: i32 = 87;
const ERROR_ACCESS_DENIED: i32 = 5;
const STILL_ACTIVE: u32 = 259;
const WAIT_OBJECT_0: u32 = 0;
const BCRYPT_USE_SYSTEM_PREFERRED_RNG: u32 = 2;

/// The creation flag that starts a process with its first thread suspended.
pub(crate) const CREATE_SUSPENDED: u32 = 0x0000_0004;

#[link(name = "kernel32")]
extern "system" {
    fn CreateJobObjectW(attributes: *const c_void, name: *const u16) -> Handle;
    fn SetInformationJobObject(job: Handle, class: i32, info: *const c_void, len: u32) -> Bool;
    fn QueryInformationJobObject(
        job: Handle,
        class: i32,
        info: *mut c_void,
        len: u32,
        returned: *mut u32,
    ) -> Bool;
    fn AssignProcessToJobObject(job: Handle, process: Handle) -> Bool;
    fn TerminateJobObject(job: Handle, code: u32) -> Bool;
    fn IsProcessInJob(process: Handle, job: Handle, result: *mut Bool) -> Bool;
    fn ResumeThread(thread: Handle) -> u32;
    fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> Handle;
    fn Thread32First(snapshot: Handle, entry: *mut ThreadEntry) -> Bool;
    fn Thread32Next(snapshot: Handle, entry: *mut ThreadEntry) -> Bool;
    fn OpenThread(access: u32, inherit: Bool, thread_id: u32) -> Handle;
    fn OpenProcess(access: u32, inherit: Bool, pid: u32) -> Handle;
    fn GetProcessTimes(
        process: Handle,
        created: *mut FileTime,
        exited: *mut FileTime,
        kernel: *mut FileTime,
        user: *mut FileTime,
    ) -> Bool;
    fn GetExitCodeProcess(process: Handle, code: *mut u32) -> Bool;
    fn TerminateProcess(process: Handle, code: u32) -> Bool;
    fn WaitForSingleObject(handle: Handle, millis: u32) -> u32;
}

#[link(name = "bcrypt")]
extern "system" {
    fn BCryptGenRandom(algorithm: *mut c_void, buffer: *mut u8, len: u32, flags: u32) -> i32;
}

/// A job object of the library's own: its processes all end when its handle closes, and none may break away.
pub(crate) struct Job(OwnedHandle);

impl Job {
    pub(crate) fn new() -> io::Result<Job> {
        let raw = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if raw.is_null() {
            return Err(io::Error::last_os_error());
        }
        let job = Job(unsafe { OwnedHandle::from_raw_handle(raw) });
        let mut limit = ExtendedLimit::default();
        limit.basic.limit_flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let ok = unsafe {
            SetInformationJobObject(
                job.raw(),
                JOB_OBJECT_EXTENDED_LIMIT,
                (&limit as *const ExtendedLimit).cast(),
                size_of::<ExtendedLimit>() as u32,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(job)
    }

    fn raw(&self) -> Handle {
        self.0.as_raw_handle()
    }

    pub(crate) fn assign(&self, process: RawHandle) -> io::Result<()> {
        if unsafe { AssignProcessToJobObject(self.raw(), process) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub(crate) fn terminate(&self) -> bool {
        unsafe { TerminateJobObject(self.raw(), 1) != 0 }
    }

    pub(crate) fn active_processes(&self) -> Option<u32> {
        let mut acc = BasicAccounting::default();
        let ok = unsafe {
            QueryInformationJobObject(
                self.raw(),
                JOB_OBJECT_BASIC_ACCOUNTING,
                (&mut acc as *mut BasicAccounting).cast(),
                size_of::<BasicAccounting>() as u32,
                std::ptr::null_mut(),
            )
        };
        (ok != 0).then_some(acc.active_processes)
    }

    /// Whether the process `pid` with the creation time `created_us` runs in this job.
    pub(crate) fn holds(&self, pid: u32, created_us: u64) -> bool {
        let Ok(h) = open(pid, PROCESS_QUERY_LIMITED_INFORMATION) else {
            return false;
        };
        if created_of(h.as_raw_handle()) != Some(created_us) {
            return false;
        }
        let mut inside: Bool = 0;
        let ok = unsafe { IsProcessInJob(h.as_raw_handle(), self.raw(), &mut inside) };
        ok != 0 && inside != 0
    }
}

/// Resumes every thread of the process `pid`, which the start made suspended with its one first thread; an error
/// when no thread of it could be resumed.
pub(crate) fn resume(pid: u32) -> io::Result<()> {
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snap.is_null() || snap == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let snap = unsafe { OwnedHandle::from_raw_handle(snap) };
    let mut entry = ThreadEntry {
        size: size_of::<ThreadEntry>() as u32,
        ..ThreadEntry::default()
    };
    let mut resumed = 0;
    let mut more = unsafe { Thread32First(snap.as_raw_handle(), &mut entry) };
    while more != 0 {
        if entry.owner_pid == pid {
            let raw = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.thread_id) };
            if !raw.is_null() {
                let thread = unsafe { OwnedHandle::from_raw_handle(raw) };
                if unsafe { ResumeThread(thread.as_raw_handle()) } != u32::MAX {
                    resumed += 1;
                }
            }
        }
        more = unsafe { Thread32Next(snap.as_raw_handle(), &mut entry) };
    }
    if resumed == 0 {
        return Err(io::Error::other("no thread resumed"));
    }
    Ok(())
}

fn open(pid: u32, access: u32) -> io::Result<OwnedHandle> {
    let raw = unsafe { OpenProcess(access, 0, pid) };
    if raw.is_null() {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { OwnedHandle::from_raw_handle(raw) })
}

/// The creation time of the process of `handle` in microseconds since the Unix epoch, as the process table records
/// it; none when it cannot be read or is not after the epoch.
pub(crate) fn created_of(handle: RawHandle) -> Option<u64> {
    let (mut created, mut exited, mut kernel, mut user) = Default::default();
    let ok = unsafe { GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) };
    if ok == 0 {
        return None;
    }
    let created: FileTime = created;
    let ticks = (u64::from(created.high) << 32) | u64::from(created.low);
    ticks
        .checked_sub(116_444_736_000_000_000)
        .map(|t| t / 10)
        .filter(|us| *us > 0)
}

fn still_active(handle: RawHandle) -> bool {
    let mut code = 0u32;
    let ok = unsafe { GetExitCodeProcess(handle, &mut code) };
    ok != 0 && code == STILL_ACTIVE
}

/// The creation time of the running process `pid`.
pub(crate) fn creation_time(pid: u32) -> Option<u64> {
    let h = open(pid, PROCESS_QUERY_LIMITED_INFORMATION).ok()?;
    let created = created_of(h.as_raw_handle())?;
    still_active(h.as_raw_handle()).then_some(created)
}

/// One read of a process by its number and creation time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Read {
    Same,
    Gone,
    Unknown,
}

/// Whether the process `pid` runs with the creation time `created_us`. A number that no process has, and one whose
/// process this user may not open to read its times, are `Gone`: a process of a run runs under the library's own
/// token, which may always read it.
pub(crate) fn read(pid: u32, created_us: u64) -> Read {
    match open(pid, PROCESS_QUERY_LIMITED_INFORMATION) {
        Ok(h) => match created_of(h.as_raw_handle()) {
            Some(c) if c == created_us && still_active(h.as_raw_handle()) => Read::Same,
            Some(_) => Read::Gone,
            None => Read::Unknown,
        },
        Err(e)
            if matches!(
                e.raw_os_error(),
                Some(ERROR_INVALID_PARAMETER | ERROR_ACCESS_DENIED)
            ) =>
        {
            Read::Gone
        }
        Err(_) => Read::Unknown,
    }
}

/// What an end of one process by its number and creation time did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum End {
    Ended,
    Gone,
    Stays,
}

/// Ends the process `pid` only when it still has the creation time `created_us`, read through the handle that
/// ends it, and waits at most `wait_ms` for it to be gone.
pub(crate) fn end(pid: u32, created_us: u64, wait_ms: u32) -> End {
    let h = match open(
        pid,
        PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE,
    ) {
        Ok(h) => h,
        Err(_) => {
            return match read(pid, created_us) {
                Read::Gone => End::Gone,
                Read::Same | Read::Unknown => End::Stays,
            };
        }
    };
    let raw = h.as_raw_handle();
    if created_of(raw) != Some(created_us) || !still_active(raw) {
        return End::Gone;
    }
    if unsafe { TerminateProcess(raw, 1) } == 0 {
        return End::Stays;
    }
    if unsafe { WaitForSingleObject(raw, wait_ms) } == WAIT_OBJECT_0 {
        End::Ended
    } else {
        End::Stays
    }
}

pub(crate) fn random16() -> io::Result<[u8; 16]> {
    let mut buf = [0u8; 16];
    let status = unsafe {
        BCryptGenRandom(
            std::ptr::null_mut(),
            buf.as_mut_ptr(),
            buf.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status != 0 {
        return Err(io::Error::other("BCryptGenRandom"));
    }
    Ok(buf)
}
