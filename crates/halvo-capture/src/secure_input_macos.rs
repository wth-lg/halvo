//! macOS secure-input guard.
#![deny(clippy::undocumented_unsafe_blocks)]

use crate::macos::screen::self_exclusion_enforced_in_filter;
use crate::macos::secure_input_byte::{secure_input_on, secure_input_state};
use crate::secure_input::{SecureInputError, SecureInputGuard, SecureInputState};

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    fn IsSecureEventInputEnabled() -> u8;
    fn EnableSecureEventInput() -> i32;
    fn DisableSecureEventInput() -> i32;
}

/// Whether the OS is currently withholding keystrokes from event taps.
pub fn secure_input_active() -> bool {
    // SAFETY: a parameterless C predicate with no side effects. CarbonEventsCore.h
    // marks it "Not thread safe" (line 3056 of the SDK on this Mac), so no other
    // thread may call it or the hold and release below at the same time. That is
    // not held until macos-2: the tap thread reads it on every event while the
    // run thread may hold or release.
    secure_input_on(unsafe { IsSecureEventInputEnabled() })
}

/// A live hold on secure input, the same state a focused password field asserts.
/// The hold lasts until this value drops, which releases it.
#[must_use = "dropping the hold releases secure input immediately"]
pub struct SecureInputHold {
    #[cfg(test)]
    enable: fn() -> i32,
    release: fn() -> i32,
}

/// The status other than noErr with which `EnableSecureEventInput` refused a
/// hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecureInputRefused(pub i32);

/// Hold secure input until the returned [`SecureInputHold`] drops, or the
/// status with which the system refused the hold.
pub fn hold_secure_input() -> Result<SecureInputHold, SecureInputRefused> {
    hold_with(enable_secure_event_input, disable_secure_event_input)
}

fn enable_secure_event_input() -> i32 {
    // SAFETY: parameterless Carbon call that returns an OSStatus, as
    // CarbonEventsCore.h declares it (line 3004). The header marks it "Not
    // thread safe" (line 2996 of the SDK on this Mac), so no other thread may
    // call it or the release and the predicate at the same time. That is not
    // held until macos-2: the tap thread may read the predicate meanwhile.
    unsafe { EnableSecureEventInput() }
}

fn hold_with(
    enable: fn() -> i32,
    release: fn() -> i32,
) -> Result<SecureInputHold, SecureInputRefused> {
    crate::macos::secure_input_byte::take_hold(enable)
        .map(|()| SecureInputHold {
            #[cfg(test)]
            enable,
            release,
        })
        .map_err(SecureInputRefused)
}

pub(crate) fn disable_secure_event_input() -> i32 {
    // SAFETY: parameterless Carbon call that returns an OSStatus, as
    // CarbonEventsCore.h declares it (line 3040). The header marks it "Not
    // thread safe" (line 3032 of the SDK on this Mac), so no other thread may
    // call it or the hold and the predicate at the same time. That is not held
    // until macos-2: the tap thread may read the predicate meanwhile.
    unsafe { DisableSecureEventInput() }
}

impl Drop for SecureInputHold {
    fn drop(&mut self) {
        crate::macos::secure_input_byte::release_hold(self.release);
    }
}

/// The macOS [`SecureInputGuard`].
#[derive(Debug, Clone, Copy, Default)]
pub struct MacosSecureInput;

impl MacosSecureInput {
    pub const fn new() -> Self {
        Self
    }
}

impl SecureInputGuard for MacosSecureInput {
    fn state(&self) -> SecureInputState {
        secure_input_state(secure_input_active())
    }

    fn exclude_self_window(&self) -> Result<(), SecureInputError> {
        self_exclusion_enforced_in_filter()
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::{
        disable_secure_event_input, enable_secure_event_input, hold_secure_input, hold_with,
        DisableSecureEventInput, EnableSecureEventInput, SecureInputHold, SecureInputRefused,
    };
    use std::sync::atomic::{AtomicIsize, Ordering};
    use std::sync::{Mutex, MutexGuard, PoisonError};
    use std::time::{Duration, Instant};

    static HOLDS: Mutex<()> = Mutex::new(());

    static OWN_HOLDS: AtomicIsize = AtomicIsize::new(0);

    const HOLDS_OF_ALL_PROCESSES: &str = "/private/tmp/halvo-capture-secure-input-tests.lock";

    fn counted_enable() -> i32 {
        let status = enable_secure_event_input();
        if status == 0 {
            OWN_HOLDS.fetch_add(1, Ordering::SeqCst);
        }
        status
    }

    fn counted_release() -> i32 {
        OWN_HOLDS.fetch_sub(1, Ordering::SeqCst);
        disable_secure_event_input()
    }

    fn own_holds() -> isize {
        OWN_HOLDS.load(Ordering::SeqCst)
    }

    pub(crate) struct OneHold {
        own_before: isize,
        _file: std::fs::File,
        _guard: MutexGuard<'static, ()>,
    }

    impl Drop for OneHold {
        fn drop(&mut self) {
            let left = own_holds() - self.own_before;
            for _ in 0..left {
                counted_release();
            }
            if left != 0 && !std::thread::panicking() {
                panic!(
                    "the test changed the count of this process's secure-input holds by {left} \
                     (the guard gave back the holds that were left: {})",
                    left.max(0)
                );
            }
        }
    }

    pub(crate) fn one_hold_at_a_time() -> OneHold {
        let guard = HOLDS.lock().unwrap_or_else(PoisonError::into_inner);
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(HOLDS_OF_ALL_PROCESSES)
            .expect("the lock file of the hold tests opens");
        // SAFETY: `file` is open, so its descriptor is valid for the call, and
        // flock only takes a lock on it, which the descriptor's close releases.
        let locked = unsafe { libc::flock(std::os::fd::AsRawFd::as_raw_fd(&file), libc::LOCK_EX) };
        assert_eq!(locked, 0, "the lock of the hold tests was not taken");
        OneHold {
            own_before: own_holds(),
            _file: file,
            _guard: guard,
        }
    }

    impl OneHold {
        pub(crate) fn hold_or_skip(&self, test: &str) -> Option<SecureInputHold> {
            self.held_or_skip(
                test,
                "hold_with on the system's calls",
                hold_with(counted_enable, counted_release),
            )
        }

        pub(crate) fn production_hold_or_skip(&self, test: &str) -> Option<SecureInputHold> {
            self.held_or_skip(test, "hold_secure_input", hold_secure_input())
        }

        fn held_or_skip(
            &self,
            test: &str,
            maker: &str,
            held: Result<SecureInputHold, SecureInputRefused>,
        ) -> Option<SecureInputHold> {
            let refused = match held {
                Ok(hold) => return Some(hold),
                Err(refused) => refused,
            };
            // SAFETY: parameterless Carbon call that returns an OSStatus. `self`
            // holds the lock that every test that holds or releases takes;
            // another test may read the predicate meanwhile, which the header
            // does not allow and which macos-2 settles, as for the hold above.
            let system = unsafe { EnableSecureEventInput() };
            if system == 0 {
                // SAFETY: as above; it balances the hold that the line above took.
                unsafe { DisableSecureEventInput() };
                panic!(
                    "{test}: {maker} reported the status {} although the system accepts a hold",
                    refused.0
                );
            }
            eprintln!(
                "SKIP {test}: EnableSecureEventInput refused the hold with status {system} \
                 ({maker}: {})",
                refused.0
            );
            None
        }
    }

    pub(crate) fn reads_on_within(reader: impl Fn() -> bool, limit: Duration) -> bool {
        let started = Instant::now();
        loop {
            if reader() {
                return true;
            }
            if started.elapsed() >= limit {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    pub(crate) fn session_shows_secure_input() -> bool {
        let out = std::process::Command::new("/usr/sbin/ioreg")
            .args(["-l", "-w", "0", "-d", "1"])
            .output()
            .expect("ioreg runs");
        assert!(out.status.success(), "ioreg exited with {}", out.status);
        String::from_utf8_lossy(&out.stdout).contains("\"kCGSSessionSecureInputPID\"")
    }

    pub(crate) fn agreed_reading(reader: impl Fn() -> bool, limit: Duration) -> (bool, bool) {
        let started = Instant::now();
        loop {
            let session = session_shows_secure_input();
            let read = reader();
            let settled = session_shows_secure_input() == session;
            if (settled && read == session) || started.elapsed() >= limit {
                return (read, session);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{agreed_reading, one_hold_at_a_time, reads_on_within};
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    #[test]
    fn a_refused_hold_gives_its_status_and_no_hold() {
        assert!(matches!(
            hold_with(|| -50, || 0),
            Err(SecureInputRefused(-50))
        ));
    }

    #[test]
    fn each_held_hold_makes_one_release_call_on_its_drop_and_a_refused_hold_none() {
        static RELEASES: AtomicUsize = AtomicUsize::new(0);
        fn counted_release() -> i32 {
            RELEASES.fetch_add(1, Ordering::SeqCst);
            0
        }
        let releases = || RELEASES.load(Ordering::SeqCst);
        let held = hold_with(|| 0, counted_release).expect("a hold with the status 0 is held");
        assert_eq!(releases(), 0, "release calls before the drop of the hold");
        drop(held);
        assert_eq!(releases(), 1, "release calls after the drop of a held hold");
        let refused = hold_with(|| -50, counted_release);
        assert!(matches!(refused, Err(SecureInputRefused(-50))));
        drop(refused);
        assert_eq!(
            releases(),
            1,
            "release calls after the drop of a refused hold"
        );
        let first = hold_with(|| 0, counted_release).expect("the first of two holds is held");
        let second = hold_with(|| 0, counted_release).expect("the second of two holds is held");
        drop((first, second));
        assert_eq!(
            releases(),
            3,
            "release calls after the drops of two more holds"
        );
    }

    #[test]
    fn the_production_hold_takes_and_gives_back_secure_input_through_the_system_calls() {
        let one = one_hold_at_a_time();
        let Some(hold) = one.production_hold_or_skip(
            "the_production_hold_takes_and_gives_back_secure_input_through_the_system_calls",
        ) else {
            return;
        };
        let takes = std::ptr::fn_addr_eq(hold.enable, enable_secure_event_input as fn() -> i32);
        let gives_back =
            std::ptr::fn_addr_eq(hold.release, disable_secure_event_input as fn() -> i32);
        if gives_back {
            drop(hold);
        } else {
            std::mem::forget(hold);
            disable_secure_event_input();
        }
        assert!(
            takes,
            "the hold that hold_secure_input makes was not taken through EnableSecureEventInput"
        );
        assert!(
            gives_back,
            "the hold that hold_secure_input makes does not give secure input back through \
             DisableSecureEventInput"
        );
    }

    #[test]
    fn a_dropped_hold_gives_the_reading_of_before_back() {
        let one = one_hold_at_a_time();
        let limit = Duration::from_secs(2);
        let before = agreed_reading(secure_input_active, limit);
        let Some(hold) = one.hold_or_skip("a_dropped_hold_gives_the_reading_of_before_back") else {
            return;
        };
        drop(hold);
        let started = std::time::Instant::now();
        while secure_input_active() != before.0 {
            assert!(
                started.elapsed() < limit,
                "2 s after the hold dropped the predicate reads {} and read {} before the hold",
                secure_input_active(),
                before.0
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn guard_reports_a_boolean_state_without_panicking() {
        let guard = MacosSecureInput::new();
        let _ = guard.is_active();
        assert!(guard.exclude_self_window().is_ok());
    }

    #[test]
    fn the_predicate_follows_the_session_reading_through_a_hold() {
        let one = one_hold_at_a_time();
        let limit = Duration::from_secs(2);
        let before = agreed_reading(secure_input_active, limit);
        let Some(hold) =
            one.hold_or_skip("the_predicate_follows_the_session_reading_through_a_hold")
        else {
            return;
        };
        let during = agreed_reading(secure_input_active, limit);
        drop(hold);
        let after = agreed_reading(secure_input_active, limit);
        assert_eq!(before.0, before.1, "before the hold: (predicate, session)");
        assert_eq!(
            during,
            (true, true),
            "during the hold: (predicate, session)"
        );
        assert_eq!(after.0, after.1, "after the hold: (predicate, session)");
    }

    #[test]
    fn the_guard_state_reads_off_on_during_a_hold_and_off_again() {
        let one = one_hold_at_a_time();
        let limit = Duration::from_secs(2);
        let guard_on = || MacosSecureInput::new().state() == SecureInputState::Active;
        let before = agreed_reading(guard_on, limit);
        let Some(hold) =
            one.hold_or_skip("the_guard_state_reads_off_on_during_a_hold_and_off_again")
        else {
            return;
        };
        let during = reads_on_within(guard_on, limit);
        drop(hold);
        let after = agreed_reading(guard_on, limit);
        assert_eq!(before.0, before.1, "before the hold: (guard, session)");
        assert!(
            during,
            "during an accepted hold the guard does not read Active"
        );
        assert_eq!(after.0, after.1, "after the hold: (guard, session)");
    }
}
