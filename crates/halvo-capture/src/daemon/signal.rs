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
