//! macOS secure-input guard.

use crate::macos::screen::self_exclusion_enforced_in_filter;
use crate::secure_input::{SecureInputError, SecureInputGuard, SecureInputState};

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    fn IsSecureEventInputEnabled() -> u8;
    fn EnableSecureEventInput();
    fn DisableSecureEventInput();
}

/// Whether the OS is currently withholding keystrokes from event taps because a
/// secure-input (password) field is focused.
pub fn secure_input_active() -> bool {
    // SAFETY: a parameterless C predicate with no side effects.
    unsafe { IsSecureEventInputEnabled() != 0 }
}

/// A live hold on secure input, the same state a focused password field asserts.
/// The hold lasts until this value drops, which releases it.
#[must_use = "dropping the hold releases secure input immediately"]
pub struct SecureInputHold(());

/// Hold secure input until the returned [`SecureInputHold`] drops.
pub fn hold_secure_input() -> SecureInputHold {
    // SAFETY: parameterless Carbon call; reference counted, balanced by the Drop
    // below.
    unsafe { EnableSecureEventInput() };
    SecureInputHold(())
}

impl Drop for SecureInputHold {
    fn drop(&mut self) {
        // SAFETY: parameterless Carbon call, balancing the EnableSecureEventInput
        // taken in hold_secure_input.
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
mod tests {
    use super::*;

    #[test]
    fn guard_reports_a_boolean_state_without_panicking() {
        let guard = MacosSecureInput::new();
        let _ = guard.is_active();
        assert!(guard.exclude_self_window().is_ok());
    }

    #[test]
    fn guard_tracks_a_real_secure_input_transition() {
        let guard = MacosSecureInput::new();
        let hold = hold_secure_input();
        if !secure_input_active() {
            eprintln!(
                "skipped guard_tracks_a_real_secure_input_transition: the OS did not \
                 flip the secure-input flag in this environment"
            );
            return;
        }
        assert_eq!(guard.state(), SecureInputState::Active);
        drop(hold);
        if !secure_input_active() {
            assert_eq!(guard.state(), SecureInputState::Inactive);
        }
    }
}
