#![allow(unsafe_code)]
#![deny(clippy::undocumented_unsafe_blocks)]

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
struct SecurityAttributes {
    len: u32,
    descriptor: *mut c_void,
    inherit: Bool,
}

#[repr(C)]
struct StartupInfo {
    cb: u32,
    reserved: *mut u16,
    desktop: *mut u16,
    title: *mut u16,
    x: u32,
    y: u32,
    x_size: u32,
    y_size: u32,
    x_count_chars: u32,
    y_count_chars: u32,
    fill_attribute: u32,
    flags: u32,
    show_window: u16,
    reserved2_len: u16,
    reserved2: *mut u8,
    std_input: Handle,
    std_output: Handle,
    std_error: Handle,
}

#[repr(C)]
struct StartupInfoEx {
    info: StartupInfo,
    attributes: *mut c_void,
}

#[repr(C)]
struct ProcessInformation {
    process: Handle,
    thread: Handle,
    pid: u32,
    tid: u32,
}

#[cfg(target_pointer_width = "64")]
const _: () = assert!(size_of::<ExtendedLimit>() == 144);
const _: () = assert!(size_of::<BasicAccounting>() == 48);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(size_of::<StartupInfo>() == 104);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(size_of::<StartupInfoEx>() == 112);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(size_of::<ProcessInformation>() == 24);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(size_of::<SecurityAttributes>() == 24);

const INVALID_HANDLE_VALUE: Handle = usize::MAX as Handle;
const EXTENDED_STARTUPINFO_PRESENT: u32 = 0x0008_0000;
const CREATE_UNICODE_ENVIRONMENT: u32 = 0x0000_0400;
const STARTF_USESTDHANDLES: u32 = 0x0000_0100;
const PROC_THREAD_ATTRIBUTE_HANDLE_LIST: usize = 0x0002_0002;
const PIPE_ACCESS_INBOUND: u32 = 0x0000_0001;
const PIPE_ACCESS_OUTBOUND: u32 = 0x0000_0002;
const FILE_FLAG_OVERLAPPED: u32 = 0x4000_0000;
const FILE_FLAG_FIRST_PIPE_INSTANCE: u32 = 0x0008_0000;
const PIPE_REJECT_REMOTE_CLIENTS: u32 = 0x0000_0008;
const GENERIC_READ: u32 = 0x8000_0000;
const GENERIC_WRITE: u32 = 0x4000_0000;
const FILE_READ_ATTRIBUTES: u32 = 0x0080;
const FILE_WRITE_ATTRIBUTES: u32 = 0x0100;
const FILE_SHARE_READ: u32 = 0x0001;
const FILE_SHARE_WRITE: u32 = 0x0002;
const OPEN_EXISTING: u32 = 3;
const PIPE_BUFFER: u32 = 4096;
const INFINITE: u32 = u32::MAX;

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

const CREATE_SUSPENDED: u32 = 0x0000_0004;

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
    fn OpenProcess(access: u32, inherit: Bool, pid: u32) -> Handle;
    fn CreateProcessW(
        application: *const u16,
        command_line: *mut u16,
        process_attributes: *const SecurityAttributes,
        thread_attributes: *const SecurityAttributes,
        inherit_handles: Bool,
        flags: u32,
        environment: *const c_void,
        directory: *const u16,
        startup: *const StartupInfo,
        information: *mut ProcessInformation,
    ) -> Bool;
    fn InitializeProcThreadAttributeList(
        list: *mut c_void,
        count: u32,
        flags: u32,
        size: *mut usize,
    ) -> Bool;
    fn UpdateProcThreadAttribute(
        list: *mut c_void,
        flags: u32,
        attribute: usize,
        value: *const c_void,
        size: usize,
        previous: *mut c_void,
        returned: *mut usize,
    ) -> Bool;
    fn DeleteProcThreadAttributeList(list: *mut c_void);
    fn CreateNamedPipeW(
        name: *const u16,
        open_mode: u32,
        pipe_mode: u32,
        instances: u32,
        out_size: u32,
        in_size: u32,
        timeout: u32,
        attributes: *const SecurityAttributes,
    ) -> Handle;
    fn CreateFileW(
        name: *const u16,
        access: u32,
        share: u32,
        attributes: *const SecurityAttributes,
        disposition: u32,
        flags: u32,
        template: Handle,
    ) -> Handle;
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

#[link(name = "kernel32")]
extern "system" {
    fn GetSystemDirectoryW(buffer: *mut u16, size: u32) -> u32;
    fn GetSystemWindowsDirectoryW(buffer: *mut u16, size: u32) -> u32;
    fn GetCurrentProcess() -> Handle;
}

const TOKEN_QUERY: u32 = 0x0008;
const TOKEN_IMPERSONATE: u32 = 0x0004;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[link(name = "bcrypt")]
extern "system" {
    fn BCryptGenRandom(algorithm: *mut c_void, buffer: *mut u8, len: u32, flags: u32) -> i32;
}

#[repr(C)]
struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

const FOLDERID_LOCAL_APP_DATA: Guid = Guid {
    data1: 0xF1B3_2785,
    data2: 0x6FBA,
    data3: 0x4FCF,
    data4: [0x9D, 0x55, 0x7B, 0x8E, 0x7F, 0x15, 0x70, 0x91],
};

#[link(name = "shell32")]
extern "system" {
    fn SHGetKnownFolderPath(id: *const Guid, flags: u32, token: Handle, path: *mut *mut u16)
        -> i32;
}

#[link(name = "ole32")]
extern "system" {
    fn CoTaskMemFree(memory: *const c_void);
}

type DirectoryCall = unsafe extern "system" fn(*mut u16, u32) -> u32;

fn directory(call: DirectoryCall) -> Option<Vec<u16>> {
    // SAFETY: with a null buffer and a size of 0 the call writes nothing and returns the size that it needs, with its NUL.
    let needed = unsafe { call(std::ptr::null_mut(), 0) };
    if needed < 2 {
        return None;
    }
    let mut buf = vec![0u16; needed as usize];
    // SAFETY: `buf` is live and writable for `needed` units, the size passed, and the call writes at most that.
    let got = unsafe { call(buf.as_mut_ptr(), needed) };
    if got == 0 || got >= needed {
        return None;
    }
    buf.truncate(got as usize);
    Some(buf)
}

/// The system directory, by `GetSystemDirectoryW`, with no NUL at its end; none when it cannot be read.
pub(crate) fn system_directory() -> Option<Vec<u16>> {
    directory(GetSystemDirectoryW)
}

/// The Windows directory, by `GetSystemWindowsDirectoryW`, with no NUL at its end; none when it cannot be read.
pub(crate) fn windows_directory() -> Option<Vec<u16>> {
    directory(GetSystemWindowsDirectoryW)
}

#[link(name = "advapi32")]
extern "system" {
    fn OpenProcessToken(process: Handle, access: u32, token: *mut Handle) -> Bool;
}

/// The account's LocalAppData known folder, by `SHGetKnownFolderPath` for the token of this process, so that the
/// shell resolves it for the account and not from this process's profile variables (with a null token it expands
/// `%USERPROFILE%` from this process's environment); none when it cannot be read.
pub(crate) fn local_app_data() -> Option<Vec<u16>> {
    let mut raw: Handle = std::ptr::null_mut();
    // SAFETY: GetCurrentProcess gives a pseudo handle that needs no close; `raw` is a live place for the token's handle.
    let opened = unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_QUERY | TOKEN_IMPERSONATE,
            &mut raw,
        )
    };
    if opened == 0 {
        return None;
    }
    let token = owned(raw).ok()?;
    let mut path: *mut u16 = std::ptr::null_mut();
    // SAFETY: the known folder's id is a live constant, `token` is a token of this process opened for query and impersonation, and `path` is a live place for the buffer that the call allocates.
    let result = unsafe {
        SHGetKnownFolderPath(
            &FOLDERID_LOCAL_APP_DATA,
            0,
            token.as_raw_handle(),
            &mut path,
        )
    };
    let found = if result == 0 && !path.is_null() {
        let mut len = 0usize;
        // SAFETY: `path` is the NUL-terminated buffer that SHGetKnownFolderPath returned on success, so every unit up to its NUL is readable.
        while unsafe { *path.add(len) } != 0 {
            len += 1;
        }
        // SAFETY: the `len` units from `path` were read just above, stand before its NUL, and are copied before the buffer is freed.
        Some(unsafe { std::slice::from_raw_parts(path, len) }.to_vec())
    } else {
        None
    };
    // SAFETY: `path` is null or the buffer that SHGetKnownFolderPath allocated for its caller, freed once here; a null pointer is no fault.
    unsafe { CoTaskMemFree(path.cast()) };
    found.filter(|p| !p.is_empty())
}

/// A job object of the library's own: its processes all end when its handle closes, and none may break away.
pub(crate) struct Job(OwnedHandle);

impl Job {
    pub(crate) fn new() -> io::Result<Job> {
        // SAFETY: null attributes and a null name ask for an unnamed job with the default security.
        let raw = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if raw.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `raw` is the new job's handle, not null as checked, and owned by nothing else.
        let job = Job(unsafe { OwnedHandle::from_raw_handle(raw) });
        let mut limit = ExtendedLimit::default();
        limit.basic.limit_flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: `limit` is a live ExtendedLimit and the length passed is its size, which the static assert holds at 144 bytes.
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
        // SAFETY: both handles are live: the job's is owned by `self` and `process` by the caller's RunProc.
        if unsafe { AssignProcessToJobObject(self.raw(), process) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub(crate) fn terminate(&self) -> bool {
        // SAFETY: the job's handle is live, owned by `self`.
        unsafe { TerminateJobObject(self.raw(), 1) != 0 }
    }

    pub(crate) fn active_processes(&self) -> Option<u32> {
        let mut acc = BasicAccounting::default();
        // SAFETY: `acc` is a live BasicAccounting and the length passed is its size, which the static assert holds at 48 bytes.
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
        // SAFETY: both handles are live for the call, and `inside` is a live place for its answer.
        let ok = unsafe { IsProcessInJob(h.as_raw_handle(), self.raw(), &mut inside) };
        ok != 0 && inside != 0
    }
}

/// A process that `start` made: its handle and its number.
pub(crate) struct Process {
    handle: OwnedHandle,
    pid: u32,
}

impl Process {
    pub(crate) fn id(&self) -> u32 {
        self.pid
    }

    pub(crate) fn raw(&self) -> RawHandle {
        self.handle.as_raw_handle()
    }

    /// The exit code once the process has exited; none while it runs or when the code cannot be read.
    pub(crate) fn try_wait(&self) -> Option<u32> {
        // SAFETY: the process handle is live, owned by `self`.
        if unsafe { WaitForSingleObject(self.raw(), 0) } != WAIT_OBJECT_0 {
            return None;
        }
        exit_code(self.raw())
    }

    /// Waits until the process has exited and gives its exit code.
    pub(crate) fn wait(&self) -> Option<u32> {
        // SAFETY: the process handle is live, owned by `self`.
        if unsafe { WaitForSingleObject(self.raw(), INFINITE) } != WAIT_OBJECT_0 {
            return None;
        }
        exit_code(self.raw())
    }

    /// Ends the process with the exit code 1; false when the system refused.
    pub(crate) fn kill(&self) -> bool {
        // SAFETY: the process handle is live, owned by `self`.
        unsafe { TerminateProcess(self.raw(), 1) != 0 }
    }
}

fn exit_code(handle: RawHandle) -> Option<u32> {
    let mut code = 0u32;
    // SAFETY: `handle` is live for the call, as its callers hold it, and `code` is a live place for the answer.
    (unsafe { GetExitCodeProcess(handle, &mut code) } != 0).then_some(code)
}

/// A process that `start` made suspended, its first thread, and this process's ends of its standard input and
/// output.
pub(crate) struct Started {
    pub(crate) process: Process,
    pub(crate) thread: OwnedHandle,
    pub(crate) stdin: OwnedHandle,
    pub(crate) stdout: OwnedHandle,
}

/// Lets the first thread of a process that `start` made suspended run.
pub(crate) fn resume(thread: &OwnedHandle) -> io::Result<()> {
    // SAFETY: the thread handle is live, owned by the caller.
    if unsafe { ResumeThread(thread.as_raw_handle()) } == u32::MAX {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// The list of the handles that a start lets its process inherit, as `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` names
/// them: the list and the handles that it points to live until it is dropped, after the start.
struct HandleList {
    _buf: Vec<usize>,
    ptr: *mut c_void,
    handles: Box<[Handle; 3]>,
}

impl HandleList {
    fn new(handles: [Handle; 3]) -> io::Result<HandleList> {
        let mut size = 0usize;
        // SAFETY: with a null list the call writes nothing but the size that a list of one attribute needs into `size`.
        unsafe { InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &mut size) };
        if size == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut buf = vec![0usize; size.div_ceil(size_of::<usize>())];
        // SAFETY: `buf` is live, aligned as usize, and holds at least `size` bytes, the size that the call asked for.
        if unsafe { InitializeProcThreadAttributeList(buf.as_mut_ptr().cast(), 1, 0, &mut size) }
            == 0
        {
            return Err(io::Error::last_os_error());
        }
        let list = HandleList {
            ptr: buf.as_mut_ptr().cast(),
            _buf: buf,
            handles: Box::new(handles),
        };
        // SAFETY: the list was initialized above and `handles` is boxed in the list, so the three handles that the attribute points to live as long as the list, as the call requires.
        let ok = unsafe {
            UpdateProcThreadAttribute(
                list.ptr(),
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
                list.handles.as_ptr().cast(),
                size_of::<[Handle; 3]>(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(list)
    }

    fn ptr(&self) -> *mut c_void {
        self.ptr
    }
}

impl Drop for HandleList {
    fn drop(&mut self) {
        // SAFETY: the list was initialized when `self` was made and is deleted once, here.
        unsafe { DeleteProcThreadAttributeList(self.ptr()) };
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

fn inheritable() -> SecurityAttributes {
    SecurityAttributes {
        len: size_of::<SecurityAttributes>() as u32,
        descriptor: std::ptr::null_mut(),
        inherit: 1,
    }
}

fn owned(raw: Handle) -> io::Result<OwnedHandle> {
    if raw.is_null() || raw == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `raw` is a new handle, neither null nor INVALID_HANDLE_VALUE as checked, and owned by nothing else.
    Ok(unsafe { OwnedHandle::from_raw_handle(raw) })
}

/// One pipe: this process's end, which is asynchronous, as the standard library's pipes are, and not inheritable,
/// and the inheritable end for the process that a start makes, which reads when `ours_reads` is false.
fn pipe(ours_reads: bool) -> io::Result<(OwnedHandle, OwnedHandle)> {
    let hex: String = random16()?.iter().map(|b| format!("{b:02x}")).collect();
    let name = wide(&format!(
        r"\\.\pipe\halvo-agent-client-{}-{hex}",
        std::process::id()
    ));
    let access = if ours_reads {
        PIPE_ACCESS_INBOUND
    } else {
        PIPE_ACCESS_OUTBOUND
    };
    // SAFETY: `name` is a live NUL-terminated name and null attributes give a handle that is not inheritable.
    let ours = owned(unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            access | FILE_FLAG_FIRST_PIPE_INSTANCE | FILE_FLAG_OVERLAPPED,
            PIPE_REJECT_REMOTE_CLIENTS,
            1,
            PIPE_BUFFER,
            PIPE_BUFFER,
            0,
            std::ptr::null(),
        )
    })?;
    let theirs_access = if ours_reads {
        GENERIC_WRITE | FILE_READ_ATTRIBUTES
    } else {
        GENERIC_READ | FILE_WRITE_ATTRIBUTES
    };
    let attributes = inheritable();
    // SAFETY: `name` is a live NUL-terminated name and `attributes` a live SECURITY_ATTRIBUTES of its own size.
    let theirs = owned(unsafe {
        CreateFileW(
            name.as_ptr(),
            theirs_access,
            0,
            &attributes,
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    })?;
    Ok((ours, theirs))
}

fn null_device() -> io::Result<OwnedHandle> {
    let name = wide("NUL");
    let attributes = inheritable();
    // SAFETY: `name` is a live NUL-terminated name and `attributes` a live SECURITY_ATTRIBUTES of its own size.
    owned(unsafe {
        CreateFileW(
            name.as_ptr(),
            GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            &attributes,
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    })
}

/// Starts the program `application` (a path ending in a NUL) suspended with `command_line` (ending in a NUL), with
/// three handles of this process inherited and no other, by `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`: the reading end of
/// a new pipe as its standard input, the writing end of another as its standard output, and the null device as its
/// standard error; this process closes its copies of those three before it returns. `environment` is a whole
/// environment block and `directory` a working directory ending in a NUL; none of either is this process's own.
pub(crate) fn start(
    application: &[u16],
    mut command_line: Vec<u16>,
    environment: Option<&[u16]>,
    directory: Option<&[u16]>,
) -> io::Result<Started> {
    let (stdin, stdin_theirs) = pipe(false)?;
    let (stdout, stdout_theirs) = pipe(true)?;
    let null = null_device()?;
    let three = [
        stdin_theirs.as_raw_handle(),
        stdout_theirs.as_raw_handle(),
        null.as_raw_handle(),
    ];
    let list = HandleList::new(three)?;
    let startup = StartupInfoEx {
        info: StartupInfo {
            cb: size_of::<StartupInfoEx>() as u32,
            reserved: std::ptr::null_mut(),
            desktop: std::ptr::null_mut(),
            title: std::ptr::null_mut(),
            x: 0,
            y: 0,
            x_size: 0,
            y_size: 0,
            x_count_chars: 0,
            y_count_chars: 0,
            fill_attribute: 0,
            flags: STARTF_USESTDHANDLES,
            show_window: 0,
            reserved2_len: 0,
            reserved2: std::ptr::null_mut(),
            std_input: three[0],
            std_output: three[1],
            std_error: three[2],
        },
        attributes: list.ptr(),
    };
    let mut info = ProcessInformation {
        process: std::ptr::null_mut(),
        thread: std::ptr::null_mut(),
        pid: 0,
        tid: 0,
    };
    let mut flags = CREATE_SUSPENDED | EXTENDED_STARTUPINFO_PRESENT | CREATE_NO_WINDOW;
    if environment.is_some() {
        flags |= CREATE_UNICODE_ENVIRONMENT;
    }
    // SAFETY: `application`, `command_line` and `directory` end in a NUL, `environment` ends in two, `startup` is a live STARTUPINFOEXW whose list lives until after the call, and `info` is a live place for the answer.
    let ok = unsafe {
        CreateProcessW(
            application.as_ptr(),
            command_line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
            flags,
            environment.map_or(std::ptr::null(), |e| e.as_ptr().cast()),
            directory.map_or(std::ptr::null(), <[u16]>::as_ptr),
            &startup.info,
            &mut info,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    drop(list);
    Ok(Started {
        process: Process {
            // SAFETY: CreateProcessW succeeded, so `info.process` is a new handle that nothing else owns.
            handle: unsafe { OwnedHandle::from_raw_handle(info.process) },
            pid: info.pid,
        },
        // SAFETY: CreateProcessW succeeded, so `info.thread` is a new handle that nothing else owns.
        thread: unsafe { OwnedHandle::from_raw_handle(info.thread) },
        stdin,
        stdout,
    })
}

fn open(pid: u32, access: u32) -> io::Result<OwnedHandle> {
    // SAFETY: OpenProcess takes plain values and returns a new handle or null.
    let raw = unsafe { OpenProcess(access, 0, pid) };
    if raw.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `raw` is a new handle, not null as checked, and owned by nothing else.
    Ok(unsafe { OwnedHandle::from_raw_handle(raw) })
}

/// The creation time of the process of `handle` in microseconds since the Unix epoch, as the process table records
/// it; none when it cannot be read or is not after the epoch.
pub(crate) fn created_of(handle: RawHandle) -> Option<u64> {
    let (mut created, mut exited, mut kernel, mut user) = Default::default();
    // SAFETY: `handle` is live for the call, as its callers hold it, and the four places are live FILETIMEs.
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
    // SAFETY: `handle` is live for the call, as its callers hold it, and `code` is a live place for the answer.
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
    // SAFETY: `raw` is the live handle `h`, opened above with the right to end the process.
    if unsafe { TerminateProcess(raw, 1) } == 0 {
        return End::Stays;
    }
    // SAFETY: `raw` is the live handle `h`, opened above with the right to wait.
    if unsafe { WaitForSingleObject(raw, wait_ms) } == WAIT_OBJECT_0 {
        End::Ended
    } else {
        End::Stays
    }
}

pub(crate) fn random16() -> io::Result<[u8; 16]> {
    let mut buf = [0u8; 16];
    // SAFETY: `buf` is live and writable for its length, the length passed, and a null algorithm with the flag for the system's preferred generator is a form that the call takes.
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
