//! The code that only Windows builds: the error mode, the inherited handles and the offset of local time.

use std::fs::File;
use std::io;
use std::os::windows::io::{FromRawHandle, RawHandle};

use windows_sys::Win32::Foundation::{
    SetHandleInformation, FILETIME, HANDLE, HANDLE_FLAG_INHERIT, SYSTEMTIME,
};
use windows_sys::Win32::System::Diagnostics::Debug::{
    SetErrorMode, SEM_FAILCRITICALERRORS, SEM_NOGPFAULTERRORBOX, SEM_NOOPENFILEERRORBOX,
    THREAD_ERROR_MODE,
};
use windows_sys::Win32::System::ErrorReporting::{WerSetFlags, WER_FAULT_REPORTING_FLAG_NOHEAP};
use windows_sys::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};

/// The error mode of the worker: no dialog of Windows Error Reporting and none of a critical error.
const ERROR_MODE: THREAD_ERROR_MODE =
    SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX;

/// The start call: the error mode above, and a report of Windows Error Reporting that holds no heap, so that no
/// report of the worker holds a record.
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

/// Takes the inherited handle `raw` as the worker's own and stops passing it on to a child. It refuses a value that
/// is no open handle of this process.
pub(crate) fn adopt(raw: u64) -> io::Result<File> {
    let value = usize::try_from(raw)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "not a handle value"))?;
    let handle = value as HANDLE;
    // SAFETY: SetHandleInformation takes a handle value; on a value that is no open handle it fails and changes
    // nothing.
    if unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the starter gave this process the handle for the worker, `Args::parse` refuses two ends of one value,
    // and nothing else in this process owns it, so it is wrapped once.
    Ok(unsafe { File::from_raw_handle(handle as RawHandle) })
}

/// The seconds from 1601-01-01 to 1970-01-01.
const FILETIME_EPOCH_S: i64 = 11_644_473_600;

/// The offset of local time from UTC, in seconds, at a second since 1970-01-01, by the time zone that the system
/// has in force.
pub(crate) fn utc_offset(secs: i64) -> Option<i32> {
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
    let local_secs = days_from_civil(
        i64::from(local.wYear),
        i64::from(local.wMonth),
        i64::from(local.wDay),
    ) * 86_400
        + i64::from(local.wHour) * 3_600
        + i64::from(local.wMinute) * 60
        + i64::from(local.wSecond);
    i32::try_from(local_secs - secs).ok()
}

/// The days from 1970-01-01 to a date of the proleptic Gregorian calendar.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}
