//! The loop that a run waits in: on macOS the process main thread's run loop,
//! turned in slices of at most one tick.

use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// One firing of the supervisor's timer: the longest slice that the loop is
/// turned before the supervisor reads its inputs again.
pub const TICK: Duration = Duration::from_millis(100);

/// The longest that the self-check allows between the queue of a block on the
/// main run loop and its run.
pub const SELF_CHECK_BOUND: Duration = Duration::from_millis(250);

/// How long the self-check waits for its block.
pub const SELF_CHECK_WINDOW: Duration = Duration::from_secs(2);

/// The loop that the supervisor turns while a run waits.
pub trait MainLoop {
    /// Turns the loop for at most `wait`: on macOS the main run loop runs its
    /// sources and blocks for that long.
    fn turn(&mut self, wait: Duration);

    /// The loop's monotonic time.
    fn now(&self) -> Duration;
}

/// The call that ends the process with `status`. The product's ending ends
/// it at once, with no unwinding, no exit handler, no destructor and no join,
/// and does not return (ruling PL-3 part (4)); a test's records what it would
/// have ended and returns.
pub trait EndCall: Send + Sync {
    fn end(&self, status: i32);
}

/// The product's ending: `_exit` on macOS, `TerminateProcess` on the process
/// itself on Windows.
pub struct ExitNow;

impl EndCall for ExitNow {
    fn end(&self, status: i32) {
        #[cfg(target_os = "macos")]
        unsafe {
            libc::_exit(status)
        }
        #[cfg(windows)]
        {
            use windows::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
            let _ = unsafe { TerminateProcess(GetCurrentProcess(), status as u32) };
            std::process::exit(status)
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        {
            std::process::exit(status)
        }
    }
}

/// A thread that exists from the start of a run, so that arming it creates
/// nothing: once armed, it makes the ending call with the armed status at the
/// deadline. A second arm keeps the earlier of the two deadlines and carries
/// its own status, the later reading of how the run ends; a disarm takes the
/// arm back, and a later arm counts again.
#[derive(Clone)]
pub struct Watchdog {
    state: Arc<Armed>,
}

/// The deadline and the status of an armed watchdog, and the wake of its
/// thread.
type Armed = (Mutex<Option<(Instant, i32)>>, Condvar);

impl Watchdog {
    /// Starts the thread of a watchdog that ends by `ending`.
    pub fn start(ending: Arc<dyn EndCall>) -> Watchdog {
        let state: Arc<Armed> = Arc::default();
        let watched = state.clone();
        let _ = std::thread::Builder::new()
            .name("halvo-watchdog".to_owned())
            .spawn(move || {
                let (armed, wake) = &*watched;
                let mut guard = armed.lock().unwrap_or_else(PoisonError::into_inner);
                loop {
                    match *guard {
                        None => {
                            guard = wake.wait(guard).unwrap_or_else(PoisonError::into_inner);
                        }
                        Some((deadline, status)) => {
                            let now = Instant::now();
                            if now >= deadline {
                                drop(guard);
                                ending.end(status);
                                return;
                            }
                            guard = wake
                                .wait_timeout(guard, deadline - now)
                                .unwrap_or_else(PoisonError::into_inner)
                                .0;
                        }
                    }
                }
            });
        Watchdog { state }
    }

    /// Arms the watchdog to end with `status` once `after` has passed. When it
    /// is armed already, the earlier of the two deadlines stays and `status`
    /// replaces the armed status: a stop that fails turns an entry's arm into
    /// 5 without moving its deadline later.
    pub fn arm(&self, after: Duration, status: i32) {
        let (armed, wake) = &*self.state;
        let mut guard = armed.lock().unwrap_or_else(PoisonError::into_inner);
        let deadline = Instant::now() + after;
        let deadline = match *guard {
            Some((earlier, _)) => earlier.min(deadline),
            None => deadline,
        };
        *guard = Some((deadline, status));
        wake.notify_all();
    }

    /// Puts `status` in an arm that stands, its deadline kept; an unarmed
    /// watchdog stays unarmed (ruling DM-26 F1).
    pub fn set_status(&self, status: i32) {
        let (armed, _) = &*self.state;
        let mut guard = armed.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((deadline, _)) = *guard {
            *guard = Some((deadline, status));
        }
    }

    /// Takes the arm back: the watchdog ends nothing until it is armed again.
    pub fn disarm(&self) {
        let (armed, wake) = &*self.state;
        *armed.lock().unwrap_or_else(PoisonError::into_inner) = None;
        wake.notify_all();
    }

    /// The status that the watchdog is armed with, if it is.
    pub fn armed(&self) -> Option<i32> {
        self.armed_until().map(|(_, status)| status)
    }

    /// The deadline and the status that the watchdog is armed with, if it is.
    pub fn armed_until(&self) -> Option<(Instant, i32)> {
        let (armed, _) = &*self.state;
        *armed.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The slice that a wait until `until` turns next at `now`: `None` once
/// `until` has passed, else at most [`TICK`].
pub fn slice(now: Duration, until: Duration) -> Option<Duration> {
    let left = until.checked_sub(now).filter(|left| !left.is_zero())?;
    Some(left.min(TICK))
}

/// The self-check's line and status: `run_loop_block_ms=<n>` with the time
/// from the queue of the block to its run, or the window when it never ran,
/// and 0 when the block ran within [`SELF_CHECK_BOUND`], else 1.
pub fn self_check_answer(ran_after: Option<Duration>) -> (String, i32) {
    let shown = ran_after.unwrap_or(SELF_CHECK_WINDOW);
    let status = match ran_after {
        Some(after) if after <= SELF_CHECK_BOUND => halvo_control::exit::CLEAN,
        Some(_) | None => halvo_control::exit::ERROR,
    };
    (format!("run_loop_block_ms={}", shown.as_millis()), status)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The ending of the crate's own tests: it panics, so that a test that
    /// reaches the product's end fails and ends no process.
    pub(crate) struct PanicInTests;

    impl EndCall for PanicInTests {
        fn end(&self, status: i32) {
            panic!("a test reached the process's ending call with status {status}");
        }
    }

    #[test]
    fn a_slice_is_at_most_one_tick_and_none_after_the_end() {
        let at = Duration::from_secs;
        assert_eq!(slice(at(0), at(5)), Some(TICK));
        assert_eq!(
            slice(Duration::from_millis(4_950), at(5)),
            Some(Duration::from_millis(50))
        );
        assert_eq!(slice(at(5), at(5)), None);
        assert_eq!(slice(at(6), at(5)), None);
    }

    #[test]
    fn the_self_check_passes_a_block_that_ran_within_250_ms_only() {
        assert_eq!(
            self_check_answer(Some(Duration::from_millis(3))),
            ("run_loop_block_ms=3".to_owned(), 0)
        );
        assert_eq!(
            self_check_answer(Some(SELF_CHECK_BOUND)),
            ("run_loop_block_ms=250".to_owned(), 0)
        );
        assert_eq!(
            self_check_answer(Some(Duration::from_millis(251))),
            ("run_loop_block_ms=251".to_owned(), 1)
        );
        assert_eq!(
            self_check_answer(None),
            ("run_loop_block_ms=2000".to_owned(), 1)
        );
    }
}
