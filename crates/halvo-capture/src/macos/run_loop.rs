use std::ffi::c_void;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use core_foundation::date::CFDate;
use core_foundation::runloop::{
    kCFRunLoopCommonModes, kCFRunLoopDefaultMode, CFRunLoop, CFRunLoopTimer, CFRunLoopTimerRef,
};

use crate::macos::foreground::{ForegroundReader, ForegroundWriter};

pub const TURN_SLICE: Duration = Duration::from_millis(100);

pub const STALLED_AFTER: Duration = crate::macos::foreground::FOREGROUND_POLL.saturating_mul(5);

static CLAIMED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimError {
    NotMainThread,
    AlreadyClaimed,
}

/// The stamp of the last turn of the main run loop. Only [`MainRunLoop`] makes
/// one.
#[derive(Clone)]
pub struct RunLoopTurns {
    last: Arc<Mutex<Instant>>,
}

impl RunLoopTurns {
    fn starting_now() -> Self {
        Self {
            last: Arc::new(Mutex::new(Instant::now())),
        }
    }

    fn stamp(&self, at: Instant) {
        *self.last.lock().unwrap_or_else(PoisonError::into_inner) = at;
    }

    /// Whether the last turn is older than [`STALLED_AFTER`] at `now`.
    pub fn stalled(&self, now: Instant) -> bool {
        let last = *self.last.lock().unwrap_or_else(PoisonError::into_inner);
        now.saturating_duration_since(last) > STALLED_AFTER
    }

    #[cfg(test)]
    pub(crate) fn for_test(last: Instant) -> Self {
        let turns = Self::starting_now();
        turns.stamp(last);
        turns
    }
}

extern "C" fn keep_the_loop_waiting(_timer: CFRunLoopTimerRef, _info: *mut c_void) {}

/// The value that the one maker of the foreground writer takes. Only this file
/// makes one, in `claim_on`, which runs once in a process.
pub(crate) struct OneWriter(());

/// The helper that turns the main run loop. It stays on the thread that made it:
///
/// ```
/// fn on_one_thread<T>() {}
/// on_one_thread::<halvo_capture::macos::run_loop::MainRunLoop>();
/// ```
///
/// ```compile_fail
/// fn across_threads<T: Send>() {}
/// across_threads::<halvo_capture::macos::run_loop::MainRunLoop>();
/// ```
pub struct MainRunLoop {
    turns: RunLoopTurns,
    foreground: Option<(ForegroundWriter, ForegroundReader)>,
    _waiting: CFRunLoopTimer,
    _not_send: PhantomData<*const ()>,
}

impl MainRunLoop {
    /// The helper, made once for the process on its main thread.
    pub fn claim() -> Result<Self, ClaimError> {
        claim_on(is_main_thread())
    }

    pub fn turns(&self) -> RunLoopTurns {
        self.turns.clone()
    }

    pub fn foreground_state(&mut self) -> Option<(ForegroundWriter, ForegroundReader)> {
        self.foreground.take()
    }

    pub fn turn_for(&self, wait: Duration) {
        let until = Instant::now() + wait;
        loop {
            let now = Instant::now();
            self.turns.stamp(now);
            if now >= until {
                return;
            }
            // SAFETY: `kCFRunLoopDefaultMode` is a constant CFString that
            // CoreFoundation exports and never writes.
            let mode = unsafe { kCFRunLoopDefaultMode };
            CFRunLoop::run_in_mode(mode, TURN_SLICE.min(until - now), false);
        }
    }
}

fn is_main_thread() -> bool {
    // SAFETY: a parameterless query of the calling thread with no side effect.
    unsafe { libc::pthread_main_np() != 0 }
}

fn claim_on(main_thread: bool) -> Result<MainRunLoop, ClaimError> {
    if !main_thread {
        return Err(ClaimError::NotMainThread);
    }
    if CLAIMED.swap(true, Ordering::SeqCst) {
        return Err(ClaimError::AlreadyClaimed);
    }
    let waiting = CFRunLoopTimer::new(
        CFDate::now().abs_time() + 86_400.0 * 365.0,
        0.0,
        0,
        0,
        keep_the_loop_waiting,
        std::ptr::null_mut(),
    );
    // SAFETY: `kCFRunLoopCommonModes` is a constant CFString that
    // CoreFoundation exports and never writes.
    CFRunLoop::get_current().add_timer(&waiting, unsafe { kCFRunLoopCommonModes });
    Ok(MainRunLoop {
        turns: RunLoopTurns::starting_now(),
        foreground: Some(crate::macos::foreground::foreground_state(OneWriter(()))),
        _waiting: waiting,
        _not_send: PhantomData,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_foundation::base::TCFType;

    #[test]
    fn the_helper_cannot_be_made_on_a_test_thread() {
        assert!(
            !is_main_thread(),
            "libtest ran this test on the main thread"
        );
        assert!(matches!(
            MainRunLoop::claim(),
            Err(ClaimError::NotMainThread)
        ));
    }

    struct InWait {
        turns: RunLoopTurns,
        firings: u32,
        seen: Option<(Instant, bool)>,
    }

    extern "C" fn read_the_stamp(_timer: CFRunLoopTimerRef, info: *mut c_void) {
        // SAFETY: `info` is the test's `InWait`, which outlives the timer: the
        // test invalidates the timer before the value drops, and the timer fires
        // on the test's own thread, inside its turn_for, while the test touches
        // the value nowhere else.
        let in_wait = unsafe { &mut *info.cast::<InWait>() };
        in_wait.firings += 1;
        if in_wait.firings == 2 {
            let last = *in_wait
                .turns
                .last
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            in_wait.seen = Some((last, in_wait.turns.stalled(Instant::now())));
        }
    }

    #[test]
    fn a_claimed_helper_hands_out_the_writer_once_turns_and_refuses_a_second_claim() {
        let mut helper = claim_on(true).expect("the one claim of this test process");
        let first = helper.foreground_state().is_some();
        let second = helper.foreground_state().is_some();
        let mut in_wait = InWait {
            turns: helper.turns(),
            firings: 0,
            seen: None,
        };
        let mut context = core_foundation::runloop::CFRunLoopTimerContext {
            version: 0,
            info: (&mut in_wait as *mut InWait).cast::<c_void>(),
            retain: None,
            release: None,
            copyDescription: None,
        };
        let timer = CFRunLoopTimer::new(
            CFDate::now().abs_time() + 0.05,
            0.1,
            0,
            0,
            read_the_stamp,
            &mut context,
        );
        // SAFETY: `kCFRunLoopCommonModes` is a constant CFString that
        // CoreFoundation exports and never writes.
        CFRunLoop::get_current().add_timer(&timer, unsafe { kCFRunLoopCommonModes });
        let started = Instant::now();
        helper.turn_for(Duration::from_millis(600));
        // SAFETY: `timer` is a live timer; invalidating it only removes it from
        // every run loop, so its callout never runs again.
        unsafe { core_foundation::runloop::CFRunLoopTimerInvalidate(timer.as_concrete_TypeRef()) };
        let (in_wait_stamp, in_wait_stalled) =
            in_wait.seen.expect("the timer fired twice inside the wait");
        assert!(
            in_wait_stamp >= started,
            "inside the wait the last stamp was {:?} before the wait began",
            started.duration_since(in_wait_stamp)
        );
        assert!(!in_wait_stalled, "the helper read stalled inside its wait");
        let last = *helper
            .turns
            .last
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let again = claim_on(true);
        assert_eq!((first, second), (true, false), "the pair handed out");
        assert!(
            last >= started + Duration::from_millis(600),
            "the last turn was stamped {:?} after the start",
            last.duration_since(started)
        );
        assert!(matches!(again, Err(ClaimError::AlreadyClaimed)));
    }

    #[test]
    fn a_stamp_older_than_five_intervals_reads_stalled_and_a_fresh_one_does_not() {
        let now = Instant::now();
        let old = now
            .checked_sub(STALLED_AFTER + Duration::from_millis(1))
            .expect("the clock reads past the threshold");
        assert!(RunLoopTurns::for_test(old).stalled(now));
        assert!(!RunLoopTurns::for_test(now).stalled(now));
        assert_eq!(STALLED_AFTER, Duration::from_millis(2000));
    }
}
