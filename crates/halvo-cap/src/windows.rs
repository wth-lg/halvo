use std::ffi::{c_void, OsStr, OsString};
use std::io;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsHandle, AsRawHandle, OwnedHandle};
use std::os::windows::process::CommandExt;
use std::process::Child;
use std::ptr::{null, null_mut};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    CloseHandle, SetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE,
    WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectAssociateCompletionPortInformation,
    JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation,
    QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
    JOBOBJECT_ASSOCIATE_COMPLETION_PORT, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOB_OBJECT_LIMIT_PROCESS_MEMORY, JOB_OBJECT_LIMIT_WORKINGSET,
};
use windows_sys::Win32::System::SystemServices::{
    JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO, JOB_OBJECT_MSG_PROCESS_MEMORY_LIMIT,
};
use windows_sys::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess,
    InitializeProcThreadAttributeList, OpenThread, ResumeThread, TerminateProcess,
    UpdateProcThreadAttribute, WaitForSingleObject, CREATE_SUSPENDED, EXTENDED_STARTUPINFO_PRESENT,
    INFINITE, PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_HANDLE_LIST, STARTF_USESTDHANDLES,
    STARTUPINFOEXW, THREAD_SUSPEND_RESUME,
};
use windows_sys::Win32::System::IO::{
    CreateIoCompletionPort, GetQueuedCompletionStatus, OVERLAPPED,
};

use crate::{CapError, ChildSpec, EndsSpec, Exit, Limit, Mode, Outcome, UnixNanos};

pub(crate) const MODE: Mode = Mode::Job;
pub(crate) const PLATFORM: &str = "windows";

const MINIMUM_WORKING_SET: usize = 1_048_576;
const DRAIN_LIMIT: Duration = Duration::from_secs(10);
const DRAIN_STEP_MS: u32 = 100;

/// An end that a child inherits.
pub(crate) type End = OwnedHandle;

/// The value of the handle, by which the child finds the end.
pub(crate) fn name_of(_index: usize, end: &End) -> u64 {
    end.as_raw_handle() as usize as u64
}

struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

// SAFETY: a handle is a value that names an object of the kernel, and the calls that this
// file makes on a handle may come from any thread; the handle is closed once, when the one
// value that owns it is dropped.
unsafe impl Send for Handle {}
// SAFETY: as above; no call of this file changes the value.
unsafe impl Sync for Handle {}

struct Job {
    job: Handle,
    port: Handle,
    mode: Mode,
}

fn set_limits(job: &Handle, bytes: usize, working_set: bool) -> io::Result<()> {
    let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
    info.BasicLimitInformation.LimitFlags =
        JOB_OBJECT_LIMIT_PROCESS_MEMORY | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    if working_set {
        info.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_WORKINGSET;
        info.BasicLimitInformation.MinimumWorkingSetSize = bytes.min(MINIMUM_WORKING_SET);
        info.BasicLimitInformation.MaximumWorkingSetSize = bytes;
    }
    info.ProcessMemoryLimit = bytes;
    let set = unsafe {
        SetInformationJobObject(
            job.0,
            JobObjectExtendedLimitInformation,
            (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast::<c_void>(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    if set == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

impl Job {
    fn new(limit: Limit) -> Result<Job, CapError> {
        let bytes = usize::try_from(limit.bytes()).map_err(|_| CapError {
            call: "SetInformationJobObject",
            source: io::Error::new(
                io::ErrorKind::InvalidInput,
                "the limit exceeds the address space",
            ),
        })?;
        let job = unsafe { CreateJobObjectW(null(), null()) };
        if job.is_null() {
            return Err(last("CreateJobObjectW"));
        }
        let job = Handle(job);
        let mode = crate::set_job_limits(|working_set| set_limits(&job, bytes, working_set))
            .map_err(|source| CapError {
                call: "SetInformationJobObject",
                source,
            })?;
        let port = unsafe { CreateIoCompletionPort(INVALID_HANDLE_VALUE, null_mut(), 0, 1) };
        if port.is_null() {
            return Err(last("CreateIoCompletionPort"));
        }
        let port = Handle(port);
        let association = JOBOBJECT_ASSOCIATE_COMPLETION_PORT {
            CompletionKey: job.0,
            CompletionPort: port.0,
        };
        let set = unsafe {
            SetInformationJobObject(
                job.0,
                JobObjectAssociateCompletionPortInformation,
                (&association as *const JOBOBJECT_ASSOCIATE_COMPLETION_PORT).cast::<c_void>(),
                size_of::<JOBOBJECT_ASSOCIATE_COMPLETION_PORT>() as u32,
            )
        };
        if set == 0 {
            return Err(last("SetInformationJobObject"));
        }
        Ok(Job { job, port, mode })
    }

    fn active_processes(&self) -> Result<u32, CapError> {
        let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { zeroed() };
        let read = unsafe {
            QueryInformationJobObject(
                self.job.0,
                JobObjectBasicAccountingInformation,
                (&mut info as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast::<c_void>(),
                size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                null_mut(),
            )
        };
        if read == 0 {
            return Err(last("QueryInformationJobObject"));
        }
        Ok(info.ActiveProcesses)
    }

    fn peak_process_memory(&self) -> Result<u64, CapError> {
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
        let read = unsafe {
            QueryInformationJobObject(
                self.job.0,
                JobObjectExtendedLimitInformation,
                (&mut info as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast::<c_void>(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                null_mut(),
            )
        };
        if read == 0 {
            return Err(last("QueryInformationJobObject"));
        }
        Ok(info.PeakProcessMemoryUsed as u64)
    }

    fn terminate(&self) -> Result<(), CapError> {
        if unsafe { TerminateJobObject(self.job.0, 1) } == 0 {
            return Err(last("TerminateJobObject"));
        }
        Ok(())
    }

    fn memory_limit_reported(&self) -> Result<bool, CapError> {
        let deadline = Instant::now() + DRAIN_LIMIT;
        let mut reported = false;
        loop {
            let mut message = 0u32;
            let mut key = 0usize;
            let mut overlapped: *mut OVERLAPPED = null_mut();
            let got = unsafe {
                GetQueuedCompletionStatus(
                    self.port.0,
                    &mut message,
                    &mut key,
                    &mut overlapped,
                    DRAIN_STEP_MS,
                )
            };
            if got == 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() != Some(WAIT_TIMEOUT as i32) {
                    return Err(CapError {
                        call: "GetQueuedCompletionStatus",
                        source: error,
                    });
                }
                if self.active_processes()? == 0 {
                    return Ok(reported);
                }
                if Instant::now() >= deadline {
                    return Err(CapError {
                        call: "GetQueuedCompletionStatus",
                        source: io::Error::new(
                            io::ErrorKind::TimedOut,
                            "the processes of the job did not end",
                        ),
                    });
                }
                continue;
            }
            if key != self.job.0 as usize {
                continue;
            }
            match message {
                JOB_OBJECT_MSG_PROCESS_MEMORY_LIMIT => reported = true,
                JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO => return Ok(reported),
                _ => {}
            }
        }
    }
}

fn resume(pid: u32) -> io::Result<()> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let snapshot = Handle(snapshot);
    let mut entry: THREADENTRY32 = unsafe { zeroed() };
    entry.dwSize = size_of::<THREADENTRY32>() as u32;
    let mut resumed = 0;
    let mut more = unsafe { Thread32First(snapshot.0, &mut entry) };
    while more != 0 {
        if entry.th32OwnerProcessID == pid {
            let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
            if thread.is_null() {
                return Err(io::Error::last_os_error());
            }
            let thread = Handle(thread);
            if unsafe { ResumeThread(thread.0) } == u32::MAX {
                return Err(io::Error::last_os_error());
            }
            resumed += 1;
        }
        more = unsafe { Thread32Next(snapshot.0, &mut entry) };
    }
    if resumed == 0 {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no thread of the started process",
        ));
    }
    Ok(())
}

fn last(call: &'static str) -> CapError {
    CapError {
        call,
        source: io::Error::last_os_error(),
    }
}

fn give_up(child: &mut Child, call: &'static str, source: io::Error) -> CapError {
    let _ = child.kill();
    let _ = child.wait();
    CapError { call, source }
}

pub(crate) fn check_supported(limit: Limit) -> Result<Mode, CapError> {
    Job::new(limit).map(|job| job.mode)
}

pub(crate) fn run(spec: ChildSpec, limit: Limit) -> Result<Outcome, CapError> {
    let job = Job::new(limit)?;
    let mut command = spec.into_command();
    command.creation_flags(CREATE_SUSPENDED);
    let spawned = command.spawn();
    let start = UnixNanos::now();
    drop(command);
    let mut child = match spawned {
        Ok(child) => child,
        Err(_) => return Ok(Outcome::not_started(start, job.mode)),
    };
    let process = child.as_raw_handle() as HANDLE;
    if unsafe { AssignProcessToJobObject(job.job.0, process) } == 0 {
        return Err(give_up(
            &mut child,
            "AssignProcessToJobObject",
            io::Error::last_os_error(),
        ));
    }
    if let Err(source) = resume(child.id()) {
        return Err(give_up(&mut child, "ResumeThread", source));
    }
    let status = child.wait().map_err(|source| CapError {
        call: "WaitForSingleObject",
        source,
    })?;
    let end = UnixNanos::now();
    job.terminate()?;
    let capped = job.memory_limit_reported()?;
    let peak_bytes = job.peak_process_memory()?;
    Ok(Outcome {
        exit: Exit::Code(status.code().unwrap_or(-1)),
        peak_bytes,
        capped,
        start,
        end,
        mode: job.mode,
    })
}

/// What the call that ends a child and the wait for it share.
struct EndState {
    /// The wait saw the child ended.
    ended: bool,
    /// A call of `end` ended the Job before that.
    reached: bool,
}

/// The call that ends one started child, through its Job.
pub(crate) struct Ending {
    /// The Job of the child; none for a program that could not be started.
    job: Option<Arc<Job>>,
    state: Mutex<EndState>,
}

impl Ending {
    fn of(job: Option<Arc<Job>>) -> Ending {
        Ending {
            job,
            state: Mutex::new(EndState {
                ended: false,
                reached: false,
            }),
        }
    }

    pub(crate) fn end(&self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(job) = &self.job else {
            return;
        };
        if !state.ended && job.terminate().is_ok() {
            state.reached = true;
        }
    }

    pub(crate) fn reached(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .reached
    }

    fn seen_ended(&self) {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .ended = true;
    }
}

/// A started child: its Job, its process handle and its pid.
struct Running {
    job: Arc<Job>,
    process: Handle,
    pid: u32,
}

/// One child under a limit, started and not yet waited for.
pub(crate) struct Started {
    /// None for a program that could not be started, and once the wait has taken it.
    running: Option<Running>,
    /// The system's error number of a start that failed.
    refused: Option<i32>,
    mode: Mode,
    start: UnixNanos,
    ending: Arc<Ending>,
}

impl Drop for Started {
    fn drop(&mut self) {
        if let Some(running) = self.running.take() {
            let _ = running.job.terminate();
        }
    }
}

/// The list of the handles that a start lets its process inherit, as
/// `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` names them. The list and the handle values that it
/// points to live until it is dropped, after the start.
struct HandleList {
    list: Vec<usize>,
    handles: Vec<HANDLE>,
}

impl HandleList {
    fn new(handles: Vec<HANDLE>) -> io::Result<HandleList> {
        let mut size = 0usize;
        // SAFETY: with a null list the call writes nothing but the size that a list of one
        // attribute needs into `size`.
        unsafe { InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut size) };
        if size == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut list = vec![0usize; size.div_ceil(size_of::<usize>())];
        // SAFETY: `list` is live, aligned as usize, and holds at least `size` bytes, the size
        // that the call asked for.
        if unsafe { InitializeProcThreadAttributeList(list.as_mut_ptr().cast(), 1, 0, &mut size) }
            == 0
        {
            return Err(io::Error::last_os_error());
        }
        let mut made = HandleList { list, handles };
        // SAFETY: the list was initialized above; the attribute points to the buffer of
        // `handles`, which lives on the heap as long as `made` and is not changed again.
        let updated = unsafe {
            UpdateProcThreadAttribute(
                made.ptr(),
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                made.handles.as_ptr().cast(),
                made.handles.len() * size_of::<HANDLE>(),
                null_mut(),
                null(),
            )
        };
        if updated == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(made)
    }

    fn ptr(&mut self) -> *mut c_void {
        self.list.as_mut_ptr().cast()
    }
}

impl Drop for HandleList {
    fn drop(&mut self) {
        // SAFETY: the list was initialized when `self` was made and is deleted once, here.
        unsafe { DeleteProcThreadAttributeList(self.ptr()) };
    }
}

/// `text` in UTF-16 with a NUL at its end; none for a NUL inside it.
fn wide(text: &OsStr) -> Option<Vec<u16>> {
    let mut wide: Vec<u16> = text.encode_wide().collect();
    if wide.contains(&0) {
        return None;
    }
    wide.push(0);
    Some(wide)
}

/// The command line of a start: the program in quotes, then each argument, in quotes when
/// it is empty or holds a space or a tab, with the backslashes before a quote doubled and
/// the quote escaped, as the C runtime reads a command line back into its arguments; ends
/// in a NUL. None for a NUL in any part or a quote in the program.
fn command_line(program: &OsStr, args: &[OsString]) -> Option<Vec<u16>> {
    const QUOTE: u16 = b'"' as u16;
    const BACKSLASH: u16 = b'\\' as u16;
    const SPACE: u16 = b' ' as u16;
    const TAB: u16 = b'\t' as u16;
    let program: Vec<u16> = program.encode_wide().collect();
    if program.contains(&0) || program.contains(&QUOTE) {
        return None;
    }
    let mut line = vec![QUOTE];
    line.extend_from_slice(&program);
    line.push(QUOTE);
    for arg in args {
        let arg: Vec<u16> = arg.encode_wide().collect();
        if arg.contains(&0) {
            return None;
        }
        line.push(SPACE);
        let quote = arg.is_empty() || arg.contains(&SPACE) || arg.contains(&TAB);
        if quote {
            line.push(QUOTE);
        }
        let mut backslashes = 0;
        for &unit in &arg {
            if unit == BACKSLASH {
                backslashes += 1;
            } else {
                if unit == QUOTE {
                    line.extend(std::iter::repeat_n(BACKSLASH, backslashes + 1));
                }
                backslashes = 0;
            }
            line.push(unit);
        }
        if quote {
            line.extend(std::iter::repeat_n(BACKSLASH, backslashes));
            line.push(QUOTE);
        }
    }
    line.push(0);
    Some(line)
}

/// Makes `handle` one that a child may inherit.
fn inheritable(handle: HANDLE) -> io::Result<()> {
    // SAFETY: the handle is an open handle that this process owns; the call sets its inherit
    // flag alone.
    if unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// An inheritable copy of this process's standard error; none where it has none or the copy
/// cannot be made.
fn own_stderr() -> Option<OwnedHandle> {
    let stderr = io::stderr();
    let raw = stderr.as_raw_handle();
    if raw.is_null() || raw == INVALID_HANDLE_VALUE {
        return None;
    }
    let copy = stderr.as_handle().try_clone_to_owned().ok()?;
    inheritable(copy.as_raw_handle()).ok()?;
    Some(copy)
}

/// Creates the child of `spec` suspended, with the list of exactly the handles that it
/// inherits: the null device as its standard input and output, a copy of this process's
/// standard error, and the ends of the spec. Answers its process handle, the handle of its
/// first thread and its pid.
fn create(spec: &EndsSpec) -> io::Result<(Handle, Handle, u32)> {
    let unfit = || io::Error::new(io::ErrorKind::InvalidInput, "the command line is unfit");
    let application = wide(&spec.program).ok_or_else(unfit)?;
    let mut line = command_line(&spec.program, &spec.args).ok_or_else(unfit)?;
    let nowhere = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("NUL")?;
    inheritable(nowhere.as_raw_handle())?;
    let error = own_stderr();
    let mut handles = vec![nowhere.as_raw_handle()];
    handles.extend(error.as_ref().map(AsRawHandle::as_raw_handle));
    for end in &spec.ends {
        inheritable(end.as_raw_handle())?;
        handles.push(end.as_raw_handle());
    }
    let mut list = HandleList::new(handles)?;
    // SAFETY: an all-zero STARTUPINFOEXW is a valid value of the type.
    let mut startup: STARTUPINFOEXW = unsafe { zeroed() };
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = nowhere.as_raw_handle();
    startup.StartupInfo.hStdOutput = nowhere.as_raw_handle();
    startup.StartupInfo.hStdError = error
        .as_ref()
        .map_or(nowhere.as_raw_handle(), AsRawHandle::as_raw_handle);
    startup.lpAttributeList = list.ptr();
    // SAFETY: an all-zero PROCESS_INFORMATION is a valid value of the type.
    let mut info: PROCESS_INFORMATION = unsafe { zeroed() };
    // SAFETY: `application` and `line` end in a NUL and live through the call, which may write
    // into `line` within its length; the null security attributes, environment and directory
    // give the defaults and those of this process; `startup` is a live STARTUPINFOEXW whose
    // list and whose handles live until after the call, and `info` is a live place for the
    // answer.
    let created = unsafe {
        CreateProcessW(
            application.as_ptr(),
            line.as_mut_ptr(),
            null(),
            null(),
            1,
            CREATE_SUSPENDED | EXTENDED_STARTUPINFO_PRESENT,
            null(),
            null(),
            &startup.StartupInfo,
            &mut info,
        )
    };
    if created == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((
        Handle(info.hProcess),
        Handle(info.hThread),
        info.dwProcessId,
    ))
}

/// Ends a process that was created and could not be put under the cap.
fn end_created(process: &Handle, call: &'static str, source: io::Error) -> CapError {
    let wait = u32::try_from(DRAIN_LIMIT.as_millis()).unwrap_or(u32::MAX);
    // SAFETY: the process handle is live, with every right, as CreateProcessW gave it.
    unsafe {
        TerminateProcess(process.0, 1);
        WaitForSingleObject(process.0, wait);
    }
    CapError { call, source }
}

pub(crate) fn start(spec: EndsSpec, limit: Limit) -> Result<Started, CapError> {
    let job = Arc::new(Job::new(limit)?);
    let mode = job.mode;
    let created = create(&spec);
    let start = UnixNanos::now();
    drop(spec);
    let (process, thread, pid) = match created {
        Ok(created) => created,
        Err(error) => {
            return Ok(Started {
                running: None,
                refused: error.raw_os_error(),
                mode,
                start,
                ending: Arc::new(Ending::of(None)),
            })
        }
    };
    // SAFETY: both handles are live: the Job's and the process handle of the new child.
    if unsafe { AssignProcessToJobObject(job.job.0, process.0) } == 0 {
        let source = io::Error::last_os_error();
        return Err(end_created(&process, "AssignProcessToJobObject", source));
    }
    // SAFETY: the thread handle is live, as CreateProcessW gave it.
    if unsafe { ResumeThread(thread.0) } == u32::MAX {
        let source = io::Error::last_os_error();
        return Err(end_created(&process, "ResumeThread", source));
    }
    drop(thread);
    Ok(Started {
        running: Some(Running {
            job: Arc::clone(&job),
            process,
            pid,
        }),
        refused: None,
        mode,
        start,
        ending: Arc::new(Ending::of(Some(job))),
    })
}

impl Started {
    pub(crate) fn pid(&self) -> Option<u32> {
        self.running.as_ref().map(|running| running.pid)
    }

    pub(crate) fn start_error(&self) -> Option<i32> {
        self.refused
    }

    pub(crate) fn ending(&self) -> Arc<Ending> {
        Arc::clone(&self.ending)
    }

    pub(crate) fn wait(mut self) -> Result<Outcome, CapError> {
        let Some(running) = self.running.take() else {
            return Ok(Outcome::not_started(self.start, self.mode));
        };
        let Running { job, process, .. } = running;
        // SAFETY: the process handle is live and may be waited for.
        if unsafe { WaitForSingleObject(process.0, INFINITE) } != WAIT_OBJECT_0 {
            let error = last("WaitForSingleObject");
            let _ = job.terminate();
            return Err(error);
        }
        self.ending.seen_ended();
        let mut code = 0u32;
        // SAFETY: the process handle is live, and the call writes the one value that the
        // pointer names.
        if unsafe { GetExitCodeProcess(process.0, &mut code) } == 0 {
            return Err(last("GetExitCodeProcess"));
        }
        let end = UnixNanos::now();
        job.terminate()?;
        let capped = job.memory_limit_reported()?;
        let peak_bytes = job.peak_process_memory()?;
        Ok(Outcome {
            exit: Exit::Code(code as i32),
            peak_bytes,
            capped,
            start: self.start,
            end,
            mode: self.mode,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_termination_of_the_job_names_its_call() {
        let job = Job {
            job: Handle(null_mut()),
            port: Handle(null_mut()),
            mode: Mode::Job,
        };
        let error = job.terminate().unwrap_err();
        assert_eq!(error.call, "TerminateJobObject");
    }
}
