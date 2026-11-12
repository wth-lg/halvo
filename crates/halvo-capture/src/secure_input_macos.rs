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
pub struct SecureInputHold(());

/// The status other than noErr with which `EnableSecureEventInput` refused a
/// hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecureInputRefused(pub i32);

/// Hold secure input until the returned [`SecureInputHold`] drops, or the
/// status with which the system refused the hold.
pub fn hold_secure_input() -> Result<SecureInputHold, SecureInputRefused> {
    hold_with(enable_secure_event_input)
}

fn enable_secure_event_input() -> i32 {
    // SAFETY: parameterless Carbon call that returns an OSStatus, as
    // CarbonEventsCore.h declares it (line 3004). The header marks it "Not
    // thread safe" (line 2996 of the SDK on this Mac), so no other thread may
    // call it or the release and the predicate at the same time. That is not
    // held until macos-2: the tap thread may read the predicate meanwhile.
    unsafe { EnableSecureEventInput() }
}

fn hold_with(enable: fn() -> i32) -> Result<SecureInputHold, SecureInputRefused> {
    crate::macos::secure_input_byte::take_hold(enable)
        .map(|()| SecureInputHold(()))
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
        crate::macos::secure_input_byte::release_hold(disable_secure_event_input);
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
        hold_secure_input, DisableSecureEventInput, EnableSecureEventInput, SecureInputHold,
    };
    use std::sync::{Mutex, MutexGuard, PoisonError};
    use std::time::{Duration, Instant};

    static HOLDS: Mutex<()> = Mutex::new(());

    const HOLDS_OF_ALL_PROCESSES: &str = "/private/tmp/halvo-capture-secure-input-tests.lock";

    pub(crate) struct OneHold {
        _file: std::fs::File,
        _guard: MutexGuard<'static, ()>,
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
            _file: file,
            _guard: guard,
        }
    }

    pub(crate) fn hold_or_skip(test: &str) -> Option<SecureInputHold> {
        let refused = match hold_secure_input() {
            Ok(hold) => return Some(hold),
            Err(refused) => refused,
        };
        // SAFETY: parameterless Carbon call that returns an OSStatus. The caller
        // holds `one_hold_at_a_time`, which every test that holds or releases
        // takes; another test may read the predicate meanwhile, which the
        // header does not allow and which macos-2 settles, as for the hold above.
        let system = unsafe { EnableSecureEventInput() };
        if system == 0 {
            // SAFETY: as above; it balances the hold that the line above took.
            unsafe { DisableSecureEventInput() };
            panic!(
                "{test}: hold_secure_input reported the status {} although the system \
                 accepts a hold",
                refused.0
            );
        }
        eprintln!(
            "SKIP {test}: EnableSecureEventInput refused the hold with status {system} \
             (hold_secure_input: {})",
            refused.0
        );
        None
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
    use super::test_support::{agreed_reading, hold_or_skip, one_hold_at_a_time, reads_on_within};
    use super::*;
    use std::time::Duration;

    #[test]
    fn a_refused_hold_gives_its_status_and_no_hold() {
        assert!(matches!(hold_with(|| -50), Err(SecureInputRefused(-50))));
    }

    #[test]
    fn a_dropped_hold_gives_the_reading_of_before_back() {
        let _one = one_hold_at_a_time();
        let limit = Duration::from_secs(2);
        let before = agreed_reading(secure_input_active, limit);
        let Some(hold) = hold_or_skip("a_dropped_hold_gives_the_reading_of_before_back") else {
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
        let _one = one_hold_at_a_time();
        let limit = Duration::from_secs(2);
        let before = agreed_reading(secure_input_active, limit);
        let Some(hold) = hold_or_skip("the_predicate_follows_the_session_reading_through_a_hold")
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
        let _one = one_hold_at_a_time();
        let limit = Duration::from_secs(2);
        let guard_on = || MacosSecureInput::new().state() == SecureInputState::Active;
        let before = agreed_reading(guard_on, limit);
        let Some(hold) = hold_or_skip("the_guard_state_reads_off_on_during_a_hold_and_off_again")
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
