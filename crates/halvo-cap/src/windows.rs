use std::ffi::c_void;
use std::io;
use std::mem::{size_of, zeroed};
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::process::Child;
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, WAIT_TIMEOUT};
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
    OpenThread, ResumeThread, CREATE_SUSPENDED, THREAD_SUSPEND_RESUME,
};
use windows_sys::Win32::System::IO::{
    CreateIoCompletionPort, GetQueuedCompletionStatus, OVERLAPPED,
};

use crate::{CapError, ChildSpec, Exit, Limit, Mode, Outcome, UnixNanos};

pub(crate) const MODE: Mode = Mode::Job;
pub(crate) const PLATFORM: &str = "windows";

const MINIMUM_WORKING_SET: usize = 1_048_576;
const DRAIN_LIMIT: Duration = Duration::from_secs(10);
const DRAIN_STEP_MS: u32 = 100;

struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

struct Job {
    job: Handle,
    port: Handle,
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
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_PROCESS_MEMORY
            | JOB_OBJECT_LIMIT_WORKINGSET
            | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        info.BasicLimitInformation.MinimumWorkingSetSize = bytes.min(MINIMUM_WORKING_SET);
        info.BasicLimitInformation.MaximumWorkingSetSize = bytes;
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
            return Err(last("SetInformationJobObject"));
        }
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
        Ok(Job { job, port })
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

pub(crate) fn check_supported(limit: Limit) -> Result<(), CapError> {
    Job::new(limit).map(|_| ())
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
        Err(_) => return Ok(Outcome::not_started(start)),
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
    unsafe { TerminateJobObject(job.job.0, 1) };
    let capped = job.memory_limit_reported()?;
    let peak_bytes = job.peak_process_memory()?;
    Ok(Outcome {
        exit: Exit::Code(status.code().unwrap_or(-1)),
        peak_bytes,
        capped,
        start,
        end,
    })
}
