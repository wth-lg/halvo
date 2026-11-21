//! The code that only unix builds: the start call, the inherited descriptors and the local date.

use std::fs::File;
use std::io;
use std::os::fd::FromRawFd;
use std::sync::Once;

/// The start call: the limit of a core file is 0, soft and hard, so no core file of the owner holds the key. A crash
/// report of the system (ReportCrash on macOS) holds no image of the memory and is not turned off.
pub(crate) fn crash_reports_off() -> io::Result<()> {
    let zero = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: setrlimit reads the one rlimit value that the pointer names, which lives through the call.
    if unsafe { libc::setrlimit(libc::RLIMIT_CORE, &zero) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Takes the inherited descriptor `raw` as the owner's own, close-on-exec. It refuses 0, 1 and 2, a number above the
/// range of a descriptor, and a descriptor that is not open.
pub(crate) fn adopt(raw: u64) -> io::Result<File> {
    let fd = i32::try_from(raw)
        .ok()
        .filter(|fd| *fd > 2)
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "not an inherited descriptor")
        })?;
    // SAFETY: fcntl takes a descriptor number; on a number that is not open it fails with EBADF and changes nothing.
    if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the starter placed the descriptor in this process for the owner, `Args::parse` refuses two ends of one
    // number, and nothing else in this process owns it, so it is wrapped once.
    Ok(unsafe { File::from_raw_fd(fd) })
}

extern "C" {
    fn tzset();
}

static TIME_ZONE: Once = Once::new();

/// The local date (year, month 1 to 12, day 1 to 31) of a second since 1970-01-01, by the system's time zone.
pub(crate) fn local_date(secs: i64) -> Option<(i64, u32, u32)> {
    // SAFETY: tzset reads the environment and the time zone files and sets the C library's state, once.
    TIME_ZONE.call_once(|| unsafe { tzset() });
    let at = libc::time_t::try_from(secs).ok()?;
    // SAFETY: an all-zero tm is a valid value of the type, whose one pointer is then null.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: localtime_r reads `at` and writes `tm` alone, both alive through the call.
    if unsafe { libc::localtime_r(&at, &mut tm) }.is_null() {
        return None;
    }
    Some((
        i64::from(tm.tm_year) + 1900,
        u32::try_from(tm.tm_mon + 1).ok()?,
        u32::try_from(tm.tm_mday).ok()?,
    ))
}

#[cfg(test)]
mod tests {
    /// Rule 6: after the start call the limit of a core file reads 0, soft and hard.
    #[test]
    fn core_files_are_off_after_the_start_call() {
        super::crash_reports_off().unwrap();
        let mut limit = libc::rlimit {
            rlim_cur: 1,
            rlim_max: 1,
        };
        // SAFETY: getrlimit writes the one rlimit value that the pointer names, which lives through the call.
        assert_eq!(unsafe { libc::getrlimit(libc::RLIMIT_CORE, &mut limit) }, 0);
        assert_eq!((limit.rlim_cur, limit.rlim_max), (0, 0));
    }
}
