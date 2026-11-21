use std::io;
use std::mem::MaybeUninit;
use std::os::fd::AsRawFd;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use crate::{CapError, ChildSpec, EndsSpec, Exit, Limit, Mode, Outcome, UnixNanos};

pub(crate) const MODE: Mode = Mode::Poll;
pub(crate) const PLATFORM: &str = "macos";

const POLL: Duration = Duration::from_millis(250);

/// An end that a child inherits.
pub(crate) type End = std::os::fd::OwnedFd;

/// The descriptor at which the child finds the end of the index `index`.
pub(crate) fn name_of(index: usize, _end: &End) -> u64 {
    3 + index as u64
}

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

/// What the call that ends a child and the call that reaps it share.
struct EndState {
    /// The child is reaped, or the next call of the waiting thread reaps it: its pid names
    /// it no longer.
    reaping: bool,
    /// A call of `end` sent its signal.
    reached: bool,
}

/// The call that ends one started child. The signal goes out under the lock that the
/// reaping takes first, so it never reaches a later process of the child's pid.
pub(crate) struct Ending {
    /// The pid of the child; none for a program that could not be started.
    pid: Option<libc::pid_t>,
    state: Mutex<EndState>,
}

impl Ending {
    fn of(pid: Option<libc::pid_t>) -> Ending {
        Ending {
            pid,
            state: Mutex::new(EndState {
                reaping: false,
                reached: false,
            }),
        }
    }

    pub(crate) fn end(&self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(pid) = self.pid else {
            return;
        };
        if state.reaping {
            return;
        }
        // SAFETY: kill takes a pid and a signal number and touches no memory; the child of this
        // pid is not reaped while the lock is held, so the pid names the child.
        if unsafe { libc::kill(pid, libc::SIGKILL) } == 0 {
            state.reached = true;
        }
    }

    pub(crate) fn reached(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .reached
    }

    /// Says that the waiting thread reaps the child next: no later call of `end` signals.
    fn reaps(&self) {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .reaping = true;
    }
}

/// A started child and what its wait reads.
struct Running {
    child: Child,
    watch: ExitWatch,
    ended: bool,
    begun: Instant,
}

/// One child under a limit, started and not yet waited for.
pub(crate) struct Started {
    /// None for a program that could not be started, and once the wait has taken it.
    running: Option<Running>,
    /// The system's error number of a start that failed.
    refused: Option<i32>,
    limit: Limit,
    start: UnixNanos,
    ending: Arc<Ending>,
}

impl Drop for Started {
    fn drop(&mut self) {
        if let Some(mut running) = self.running.take() {
            self.ending.reaps();
            let _ = running.child.kill();
            let _ = running.child.wait();
        }
    }
}

fn give_up(child: &mut Child, ending: &Ending, call: &'static str, source: io::Error) -> CapError {
    ending.reaps();
    let _ = child.kill();
    let _ = child.wait();
    CapError { call, source }
}

pub(crate) fn check_supported(_limit: Limit) -> Result<Mode, CapError> {
    footprint(std::process::id() as libc::pid_t)
        .map(|_| ())
        .map_err(|source| CapError {
            call: "proc_pid_rusage",
            source,
        })?;
    ExitWatch::new(std::process::id() as libc::pid_t)
        .map(|_| MODE)
        .map_err(|source| CapError {
            call: "kqueue",
            source,
        })
}

pub(crate) fn run(spec: ChildSpec, limit: Limit) -> Result<Outcome, CapError> {
    run_reading(spec, limit, footprint)
}

fn run_reading(
    spec: ChildSpec,
    limit: Limit,
    footprint_of: impl FnMut(libc::pid_t) -> io::Result<u64>,
) -> Result<Outcome, CapError> {
    begin(spec.into_command(), limit)?.wait_reading(footprint_of)
}

pub(crate) fn start(spec: EndsSpec, limit: Limit) -> Result<Started, CapError> {
    begin(with_ends(spec), limit)
}

/// The highest descriptor number, plus one, that the child's start looks at.
fn descriptor_limit() -> libc::c_int {
    // SAFETY: sysconf reads a limit of the system and touches no memory.
    match unsafe { libc::sysconf(libc::_SC_OPEN_MAX) } {
        limit if limit > 3 => libc::c_int::try_from(limit.min(65_536)).unwrap_or(65_536),
        _ => 65_536,
    }
}

/// The command of a child that inherits ends: the end of the index n at the descriptor
/// 3 + n, and close-on-exec on every other descriptor above the standard streams. The
/// command owns the ends, so this process closes them when the command is dropped.
fn with_ends(spec: EndsSpec) -> Command {
    let mut command = Command::new(spec.program);
    command
        .args(spec.args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    let ends = spec.ends;
    let first_free = 3 + libc::c_int::try_from(ends.len()).unwrap_or(libc::c_int::MAX - 3);
    let limit = descriptor_limit();
    let mut high: Vec<libc::c_int> = vec![-1; ends.len()];
    // SAFETY: the closure runs in the child between fork and exec and calls fcntl and dup2
    // alone, which are async-signal-safe; it writes into `high`, which was allocated before
    // the fork. It first copies each end above the places of all, so that no dup2 writes
    // over an end before it is copied.
    unsafe {
        command.pre_exec(move || {
            for (copy, end) in high.iter_mut().zip(&ends) {
                *copy = libc::fcntl(end.as_raw_fd(), libc::F_DUPFD_CLOEXEC, first_free);
                if *copy < 0 {
                    return Err(io::Error::last_os_error());
                }
            }
            for (place, copy) in (3..).zip(&high) {
                if libc::dup2(*copy, place) < 0 {
                    return Err(io::Error::last_os_error());
                }
            }
            for descriptor in first_free..limit {
                let flags = libc::fcntl(descriptor, libc::F_GETFD);
                if flags >= 0 && flags & libc::FD_CLOEXEC == 0 {
                    libc::fcntl(descriptor, libc::F_SETFD, flags | libc::FD_CLOEXEC);
                }
            }
            Ok(())
        });
    }
    command
}

/// Starts `command` and sets the watch for the child's end.
fn begin(mut command: Command, limit: Limit) -> Result<Started, CapError> {
    let spawned = command.spawn();
    let start = UnixNanos::now();
    drop(command);
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) => {
            return Ok(Started {
                running: None,
                refused: error.raw_os_error(),
                limit,
                start,
                ending: Arc::new(Ending::of(None)),
            })
        }
    };
    let begun = Instant::now();
    let pid = child.id() as libc::pid_t;
    let ending = Arc::new(Ending::of(Some(pid)));
    let (watch, ended) = match ExitWatch::new(pid) {
        Ok(found) => found,
        Err(source) => return Err(give_up(&mut child, &ending, "kevent", source)),
    };
    Ok(Started {
        running: Some(Running {
            child,
            watch,
            ended,
            begun,
        }),
        refused: None,
        limit,
        start,
        ending,
    })
}

impl Started {
    pub(crate) fn pid(&self) -> Option<u32> {
        self.running.as_ref().map(|running| running.child.id())
    }

    pub(crate) fn start_error(&self) -> Option<i32> {
        self.refused
    }

    pub(crate) fn ending(&self) -> Arc<Ending> {
        Arc::clone(&self.ending)
    }

    pub(crate) fn wait(self) -> Result<Outcome, CapError> {
        self.wait_reading(footprint)
    }

    fn wait_reading(
        mut self,
        mut footprint_of: impl FnMut(libc::pid_t) -> io::Result<u64>,
    ) -> Result<Outcome, CapError> {
        let Some(running) = self.running.take() else {
            return Ok(Outcome::not_started(self.start, MODE));
        };
        let Running {
            mut child,
            watch,
            mut ended,
            begun,
        } = running;
        let (limit, start) = (self.limit, self.start);
        let ending = Arc::clone(&self.ending);
        let pid = child.id() as libc::pid_t;
        let mut end = ended.then(UnixNanos::now);
        let mut peak = 0u64;
        let mut breach = false;
        let mut killed = false;
        let mut ticks = 0u32;
        loop {
            match footprint_of(pid) {
                Ok(read) => peak = peak.max(read),
                Err(source) => {
                    if !ended {
                        match watch.ended_within(Duration::ZERO) {
                            Ok(true) => {
                                ended = true;
                                end = Some(UnixNanos::now());
                            }
                            Ok(false) => {
                                return Err(give_up(&mut child, &ending, "proc_pid_rusage", source))
                            }
                            Err(source) => {
                                return Err(give_up(&mut child, &ending, "kevent", source))
                            }
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
                Err(source) => return Err(give_up(&mut child, &ending, "kevent", source)),
            }
        }
        ending.reaps();
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
            mode: MODE,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sleeper() -> ChildSpec {
        let mut spec = ChildSpec::new("/bin/sleep", ["0.6"]);
        spec.stdout = std::process::Stdio::null();
        spec
    }

    #[test]
    fn a_run_whose_reads_equal_the_limit_is_not_capped() {
        let limit = Limit::new(1_000_000).unwrap();
        let outcome = run_reading(sleeper(), limit, |_| Ok(1_000_000)).unwrap();
        assert_eq!(
            (outcome.exit, outcome.capped, outcome.peak_bytes),
            (Exit::Code(0), false, 1_000_000),
            "{outcome:?}"
        );
    }

    #[test]
    fn a_run_whose_read_passes_the_limit_by_one_byte_is_capped_and_killed() {
        let limit = Limit::new(1_000_000).unwrap();
        let outcome = run_reading(sleeper(), limit, |_| Ok(1_000_001)).unwrap();
        assert_eq!(
            (outcome.exit, outcome.capped),
            (Exit::Signal(9), true),
            "{outcome:?}"
        );
    }

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
