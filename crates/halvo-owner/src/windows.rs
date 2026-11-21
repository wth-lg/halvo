//! The code that only Windows builds: the error mode, the inherited handles, the two handles of an end of a stream
//! and the local date.

use std::fs::File;
use std::io;
use std::os::windows::io::{FromRawHandle, RawHandle};

use windows_sys::Win32::Foundation::{
    SetHandleInformation, FILETIME, HANDLE, HANDLE_FLAG_INHERIT, SYSTEMTIME,
};
use windows_sys::Win32::Storage::FileSystem::{GetFileType, FILE_TYPE_PIPE};
use windows_sys::Win32::System::Diagnostics::Debug::{
    SetErrorMode, SEM_FAILCRITICALERRORS, SEM_NOGPFAULTERRORBOX, SEM_NOOPENFILEERRORBOX,
    THREAD_ERROR_MODE,
};
use windows_sys::Win32::System::ErrorReporting::{WerSetFlags, WER_FAULT_REPORTING_FLAG_NOHEAP};
use windows_sys::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};

/// The error mode of the owner: no dialog of Windows Error Reporting and none of a critical error.
const ERROR_MODE: THREAD_ERROR_MODE =
    SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX;

/// The start call: the error mode above, and a report of Windows Error Reporting that holds no heap, so that no
/// report of the owner holds the key.
pub(crate) fn crash_reports_off() -> io::Result<()> {
    // SAFETY: SetErrorMode takes flags and returns the mode before; it touches no memory of the caller.
    unsafe { SetErrorMode(ERROR_MODE) };
    // SAFETY: WerSetFlags takes flags for this process; it touches no memory of the caller.
    let result = unsafe { WerSetFlags(WER_FAULT_REPORTING_FLAG_NOHEAP) };
    if result < 0 {
        return Err(io::Error::from_raw_os_error(result));
    }
    Ok(())
}

/// Takes the handle `raw`, which the starter let the owner inherit or made in the owner's process, as the owner's own
/// and stops passing it on to a child. It refuses a value that is no open handle of this process.
pub(crate) fn adopt(raw: u64) -> io::Result<File> {
    let value = usize::try_from(raw)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "not a handle value"))?;
    let handle = value as HANDLE;
    // SAFETY: SetHandleInformation takes a handle value; on a value that is no open handle it fails and changes
    // nothing.
    if unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the starter gave this process the handle for the owner, `Args::parse` refuses two ends of one value and
    // `stream_end` two values that are one, and nothing else in this process owns it, so it is wrapped once.
    Ok(unsafe { File::from_raw_handle(handle as RawHandle) })
}

/// The read end of the starter's channel as its reader thread holds it. On Windows nothing comes beside the bytes of
/// a channel: a frame `stream` names its two handles by their values.
pub(crate) fn starter_end(end: File) -> File {
    end
}

/// Takes `raw` as the owner's own where it names a pipe, and touches no value that names anything else or nothing.
fn pipe(raw: u64) -> Option<File> {
    let value = usize::try_from(raw).ok()?;
    // SAFETY: GetFileType takes a handle value; on a value that is no open handle it answers that the type is
    // unknown and changes nothing.
    if unsafe { GetFileType(value as HANDLE) } != FILE_TYPE_PIPE {
        return None;
    }
    adopt(raw).ok()
}

/// The end of a stream that a frame `stream` names: the handle on which the reader thread reads, then the handle on
/// which the owner writes. A frame that names no values, two values that are one and a value that names no pipe give
/// none; a value that names a pipe is then closed, and no other value is touched.
pub(crate) fn stream_end(handles: Option<(u64, u64)>, _came: Vec<File>) -> Option<(File, File)> {
    let (reads, writes) = handles?;
    let one = reads == writes;
    let reads = pipe(reads);
    let writes = if one { None } else { pipe(writes) };
    reads.zip(writes)
}

/// Ends the reader thread of a stream where the system can. Nothing wakes a read of an anonymous pipe that blocks,
/// so on Windows the handle that reads stays open, with the place of its stream, until the far end closes the handle
/// on which it writes.
pub(crate) fn shut(_end: &File) {}

/// The seconds from 1601-01-01 to 1970-01-01.
const FILETIME_EPOCH_S: i64 = 11_644_473_600;

/// The local date (year, month 1 to 12, day 1 to 31) of a second since 1970-01-01, by the time zone that the system
/// has in force.
pub(crate) fn local_date(secs: i64) -> Option<(i64, u32, u32)> {
    let ticks = secs
        .checked_add(FILETIME_EPOCH_S)?
        .checked_mul(10_000_000)?;
    let ticks = u64::try_from(ticks).ok()?;
    let at = FILETIME {
        dwLowDateTime: (ticks & 0xffff_ffff) as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    // SAFETY: an all-zero SYSTEMTIME is a valid value of the type.
    let mut utc: SYSTEMTIME = unsafe { std::mem::zeroed() };
    // SAFETY: as above.
    let mut local: SYSTEMTIME = unsafe { std::mem::zeroed() };
    // SAFETY: the call reads `at` and writes `utc`, both alive through the call.
    if unsafe { FileTimeToSystemTime(&at, &mut utc) } == 0 {
        return None;
    }
    // SAFETY: a null zone asks for the zone in force; the call reads `utc` and writes `local`, both alive through it.
    if unsafe { SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut local) } == 0 {
        return None;
    }
    Some((
        i64::from(local.wYear),
        u32::from(local.wMonth),
        u32::from(local.wDay),
    ))
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::System::Diagnostics::Debug::GetErrorMode;
    use windows_sys::Win32::System::ErrorReporting::{
        WerGetFlags, WER_FAULT_REPORTING_FLAG_NOHEAP,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    use super::ERROR_MODE;

    /// Rule 6: after the start call the error mode holds the flags of the call and the reports of Windows Error
    /// Reporting hold no heap.
    #[test]
    fn core_files_are_off_after_the_start_call() {
        super::crash_reports_off().unwrap();
        // SAFETY: GetErrorMode takes nothing and touches no memory of the caller.
        assert_eq!(unsafe { GetErrorMode() } & ERROR_MODE, ERROR_MODE);
        let mut flags = 0;
        // SAFETY: the pseudo handle of this process, and one flags value that lives through the call.
        let result = unsafe { WerGetFlags(GetCurrentProcess(), &mut flags) };
        assert!(result >= 0, "WerGetFlags answered {result:#x}");
        assert_eq!(
            flags & WER_FAULT_REPORTING_FLAG_NOHEAP,
            WER_FAULT_REPORTING_FLAG_NOHEAP
        );
    }
}
