//! The calls that the binary makes for its process before any mode runs: the
//! flag that SIGINT and SIGTERM set, the file-mode mask, and the process's own
//! peak of resident memory.

#![deny(clippy::undocumented_unsafe_blocks)]

use std::io;
use std::sync::atomic::AtomicBool;

static SIGNALLED: AtomicBool = AtomicBool::new(false);

/// The flag that SIGINT and SIGTERM set once [`prepare`] has run.
pub fn flag() -> &'static AtomicBool {
    &SIGNALLED
}

/// Sets the file-mode mask to 0o077 and installs the handlers of SIGINT and
/// SIGTERM, which set [`flag`] and do nothing else. On Windows it does nothing
/// yet: the timed run there is win-5's, and a console Ctrl-C ends the process.
pub fn prepare() -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        // SAFETY: umask sets the mask of this process and has no failure; no
        // memory is passed.
        unsafe { libc::umask(0o077) };
        let handler = on_signal as extern "C" fn(libc::c_int) as *const () as libc::sighandler_t;
        for signal in [libc::SIGINT, libc::SIGTERM] {
            // SAFETY: `on_signal` is an `extern "C"` function that stores to a
            // static atomic, which is safe to do in a signal handler, and it
            // lives for the whole process.
            let before = unsafe { libc::signal(signal, handler) };
            if before == libc::SIG_ERR {
                return Err(io::Error::last_os_error());
            }
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
extern "C" fn on_signal(_signal: libc::c_int) {
    SIGNALLED.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// The process's own peak of resident memory in bytes, from `getrusage` for
/// itself, so no reaped child can raise it.
pub fn peak_rss_bytes() -> io::Result<u64> {
    #[cfg(target_os = "macos")]
    {
        let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
        // SAFETY: getrusage writes one `rusage` into the buffer that it is
        // given, which is zeroed, sized for one and lives across the call.
        let status = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
        if status != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: getrusage returned 0, so it filled the value; a zeroed
        // `rusage` is a valid value in any case.
        let usage = unsafe { usage.assume_init() };
        u64::try_from(usage.ru_maxrss).map_err(|_| io::Error::other("a negative peak"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(io::ErrorKind::Unsupported.into())
    }
}

/// The bytes in use of the process's heap: summed over every malloc zone, and
/// in the default zone alone, which on some macOS releases holds few of the
/// process's blocks (ruling DM-10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeapInUse {
    pub all_zones: u64,
    pub default_zone: u64,
}

/// The heap's bytes in use now, from `malloc_zone_statistics`.
pub fn heap_bytes_in_use() -> io::Result<HeapInUse> {
    #[cfg(target_os = "macos")]
    {
        let read = |zone: *mut libc::malloc_zone_t| {
            let mut stats = std::mem::MaybeUninit::<libc::malloc_statistics_t>::zeroed();
            // SAFETY: a null zone asks for the sum over every zone, and a zone
            // from malloc_default_zone is one that malloc keeps for the life
            // of the process; the call writes one statistics value into the
            // zeroed buffer, which is sized for one and lives across the call.
            unsafe { libc::malloc_zone_statistics(zone, stats.as_mut_ptr()) };
            // SAFETY: a zeroed malloc_statistics_t is a valid value, and the
            // call filled it.
            unsafe { stats.assume_init() }.size_in_use as u64
        };
        // SAFETY: malloc_default_zone takes no argument and returns the
        // default zone, which malloc keeps for the life of the process.
        let default_zone = unsafe { libc::malloc_default_zone() };
        Ok(HeapInUse {
            all_zones: read(std::ptr::null_mut()),
            default_zone: read(default_zone),
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(io::ErrorKind::Unsupported.into())
    }
}

/// The process's physical footprint in bytes, from `proc_pid_rusage` for
/// itself.
pub fn phys_footprint_bytes() -> io::Result<u64> {
    #[cfg(target_os = "macos")]
    {
        let mut usage = std::mem::MaybeUninit::<libc::rusage_info_v2>::zeroed();
        // SAFETY: proc_pid_rusage with RUSAGE_INFO_V2 writes one
        // rusage_info_v2 into the buffer that it is given, which is zeroed,
        // sized for one and lives across the call; the pid is this process's.
        let status = unsafe {
            libc::proc_pid_rusage(
                libc::getpid(),
                libc::RUSAGE_INFO_V2,
                usage.as_mut_ptr().cast::<libc::rusage_info_t>(),
            )
        };
        if status != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the call returned 0, so it filled the value; a zeroed
        // rusage_info_v2 is a valid value in any case.
        Ok(unsafe { usage.assume_init() }.ri_phys_footprint)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(io::ErrorKind::Unsupported.into())
    }
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "macos")]
    #[test]
    fn the_heap_and_footprint_readings_answer_with_a_size() {
        let heap = super::heap_bytes_in_use().unwrap();
        assert!(heap.all_zones > 0, "{heap:?}");
        assert!(super::phys_footprint_bytes().unwrap() > 0);
    }
}
