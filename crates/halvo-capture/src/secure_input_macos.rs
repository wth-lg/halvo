//! macOS secure-input guard.
#![deny(clippy::undocumented_unsafe_blocks)]

use crate::macos::screen::self_exclusion_enforced_in_filter;
use crate::macos::secure_input_byte::secure_input_on;
use crate::secure_input::{SecureInputError, SecureInputGuard, SecureInputState};

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    fn IsSecureEventInputEnabled() -> u8;
    fn EnableSecureEventInput();
    fn DisableSecureEventInput();
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

/// Hold secure input until the returned [`SecureInputHold`] drops.
pub fn hold_secure_input() -> SecureInputHold {
    // SAFETY: parameterless Carbon call. CarbonEventsCore.h marks it "Not thread
    // safe" (line 2996 of the SDK on this Mac), so no other thread may call it or
    // the release and the predicate at the same time. That is not held until
    // macos-2: the tap thread may read the predicate meanwhile.
    unsafe { EnableSecureEventInput() };
    SecureInputHold(())
}

impl Drop for SecureInputHold {
    fn drop(&mut self) {
        // SAFETY: parameterless Carbon call. CarbonEventsCore.h marks it "Not
        // thread safe" (line 3032 of the SDK on this Mac), so no other thread
        // may call it or the hold and the predicate at the same time. That is
        // not held until macos-2: the tap thread may read the predicate meanwhile.
        unsafe { DisableSecureEventInput() };
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
        if secure_input_active() {
            SecureInputState::Active
        } else {
            SecureInputState::Inactive
        }
    }

    fn exclude_self_window(&self) -> Result<(), SecureInputError> {
        self_exclusion_enforced_in_filter()
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::{Mutex, MutexGuard, PoisonError};
    use std::time::{Duration, Instant};

    static HOLDS: Mutex<()> = Mutex::new(());

    pub(crate) fn one_hold_at_a_time() -> MutexGuard<'static, ()> {
        HOLDS.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn session_shows_secure_input() -> bool {
        let out = std::process::Command::new("/usr/sbin/ioreg")
            .args(["-l", "-w", "0", "-d", "1"])
            .output()
            .expect("ioreg runs");
        assert!(out.status.success(), "ioreg exited with {}", out.status);
        String::from_utf8_lossy(&out.stdout).contains("\"kCGSSessionSecureInputPID\"")
    }

    pub(crate) fn session_reading_within(want: bool, limit: Duration) -> bool {
        let started = Instant::now();
        loop {
            let reading = session_shows_secure_input();
            if reading == want || started.elapsed() >= limit {
                return reading;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    pub(crate) fn agreed_reading(predicate: impl Fn() -> bool, limit: Duration) -> (bool, bool) {
        let started = Instant::now();
        loop {
            let session = session_shows_secure_input();
            let read = predicate();
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
    use super::test_support::{agreed_reading, one_hold_at_a_time, session_reading_within};
    use super::*;
    use std::time::Duration;

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
        let hold = hold_secure_input();
        if !session_reading_within(true, limit) {
            eprintln!(
                "SKIP the_predicate_follows_the_session_reading_through_a_hold: the session \
                 shows no secure-input pid while this process holds secure input"
            );
            return;
        }
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
}
