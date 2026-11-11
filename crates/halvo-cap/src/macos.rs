use std::io;
use std::mem::MaybeUninit;
use std::os::unix::process::ExitStatusExt;
use std::process::Child;
use std::time::{Duration, Instant};

use crate::{CapError, ChildSpec, Exit, Limit, Mode, Outcome, UnixNanos};

pub(crate) const MODE: Mode = Mode::Poll;
pub(crate) const PLATFORM: &str = "macos";

const POLL: Duration = Duration::from_millis(250);

fn is_breach(read: u64, limit: Limit) -> bool {
    read > limit.bytes()
}

fn footprint(pid: libc::pid_t) -> io::Result<u64> {
    let mut info = MaybeUninit::<libc::rusage_info_v4>::zeroed();
    let rc = unsafe { libc::proc_pid_rusage(pid, libc::RUSAGE_INFO_V4, info.as_mut_ptr().cast()) };
    if rc == 0 {
        Ok(unsafe { info.assume_init() }.ri_lifetime_max_phys_footprint)
    } else {
        Err(io::Error::last_os_error())
    }
}

struct ExitWatch {
    queue: libc::c_int,
}

impl Drop for ExitWatch {
    fn drop(&mut self) {
        unsafe { libc::close(self.queue) };
    }
}

impl ExitWatch {
    fn new(pid: libc::pid_t) -> io::Result<(ExitWatch, bool)> {
        let queue = unsafe { libc::kqueue() };
        if queue < 0 {
            return Err(io::Error::last_os_error());
        }
        let watch = ExitWatch { queue };
        let change = libc::kevent {
            ident: pid as libc::uintptr_t,
            filter: libc::EVFILT_PROC,
            flags: libc::EV_ADD | libc::EV_ONESHOT,
            fflags: libc::NOTE_EXIT,
            data: 0,
            udata: std::ptr::null_mut(),
        };
        let rc =
            unsafe { libc::kevent(queue, &change, 1, std::ptr::null_mut(), 0, std::ptr::null()) };
        if rc < 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ESRCH) {
                return Ok((watch, true));
            }
            return Err(error);
        }
        Ok((watch, false))
    }

    fn ended_within(&self, timeout: Duration) -> io::Result<bool> {
        let wait = libc::timespec {
            tv_sec: timeout.as_secs() as libc::time_t,
            tv_nsec: timeout.subsec_nanos() as libc::c_long,
        };
        let mut event = MaybeUninit::<libc::kevent>::zeroed();
        let rc = unsafe {
            libc::kevent(
                self.queue,
                std::ptr::null(),
                0,
                event.as_mut_ptr(),
                1,
                &wait,
            )
        };
        if rc < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                return Ok(false);
            }
            return Err(error);
        }
        Ok(rc > 0)
    }
}

fn give_up(child: &mut Child, call: &'static str, source: io::Error) -> CapError {
    let _ = child.kill();
    let _ = child.wait();
    CapError { call, source }
}

pub(crate) fn check_supported(_limit: Limit) -> Result<(), CapError> {
    footprint(std::process::id() as libc::pid_t)
        .map(|_| ())
        .map_err(|source| CapError {
            call: "proc_pid_rusage",
            source,
        })?;
    ExitWatch::new(std::process::id() as libc::pid_t)
        .map(|_| ())
        .map_err(|source| CapError {
            call: "kqueue",
            source,
        })
}

pub(crate) fn run(spec: ChildSpec, limit: Limit) -> Result<Outcome, CapError> {
    let mut command = spec.into_command();
    let spawned = command.spawn();
    let start = UnixNanos::now();
    drop(command);
    let mut child = match spawned {
        Ok(child) => child,
        Err(_) => return Ok(Outcome::not_started(start)),
    };
    let begun = Instant::now();
    let pid = child.id() as libc::pid_t;
    let (watch, mut ended) = match ExitWatch::new(pid) {
        Ok(found) => found,
        Err(source) => return Err(give_up(&mut child, "kevent", source)),
    };
    let mut end = ended.then(UnixNanos::now);
    let mut peak = 0u64;
    let mut breach = false;
    let mut killed = false;
    let mut ticks = 0u32;
    loop {
        match footprint(pid) {
            Ok(read) => peak = peak.max(read),
            Err(source) => {
                if !ended {
                    match watch.ended_within(Duration::ZERO) {
                        Ok(true) => {
                            ended = true;
                            end = Some(UnixNanos::now());
                        }
                        Ok(false) => return Err(give_up(&mut child, "proc_pid_rusage", source)),
                        Err(source) => return Err(give_up(&mut child, "kevent", source)),
                    }
                }
            }
        }
        if is_breach(peak, limit) {
            breach = true;
            if !ended && !killed {
                let _ = child.kill();
                killed = true;
            }
        }
        if ended {
            break;
        }
        ticks += 1;
        let wait = (begun + POLL * ticks).saturating_duration_since(Instant::now());
        match watch.ended_within(wait) {
            Ok(true) => {
                ended = true;
                end = Some(UnixNanos::now());
            }
            Ok(false) => {}
            Err(source) => return Err(give_up(&mut child, "kevent", source)),
        }
    }
    let status = child.wait().map_err(|source| CapError {
        call: "waitpid",
        source,
    })?;
    let exit = match (status.code(), status.signal()) {
        (Some(code), _) => Exit::Code(code),
        (None, Some(signal)) => Exit::Signal(signal),
        (None, None) => Exit::Code(status.into_raw()),
    };
    Ok(Outcome {
        exit,
        peak_bytes: peak,
        capped: breach,
        start,
        end: end.unwrap_or_else(UnixNanos::now),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_read_above_the_limit_is_a_breach_and_a_read_at_or_below_it_is_none() {
        let limit = Limit::new(1_000_000).unwrap();
        assert!(!is_breach(500_000, limit));
        assert!(!is_breach(750_000, limit));
        assert!(!is_breach(999_999, limit));
        assert!(!is_breach(1_000_000, limit));
        assert!(is_breach(1_000_001, limit));
    }
}
