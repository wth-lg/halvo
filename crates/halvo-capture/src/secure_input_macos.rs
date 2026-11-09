//! macOS secure-input guard.

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

/// Hold secure input, the same call a focused password field makes.
pub fn hold_secure_input() {
    // SAFETY: parameterless Carbon call; reference counted, released on exit.
    unsafe { EnableSecureEventInput() };
}

/// Release a hold taken by [`hold_secure_input`].
pub fn release_secure_input() {
    // SAFETY: parameterless Carbon call, balancing one `hold_secure_input`.
    unsafe { DisableSecureEventInput() };
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
        Ok(())
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
        hold_secure_input();
        let active_seen = secure_input_active();
        if active_seen {
            assert_eq!(guard.state(), SecureInputState::Active);
        }
        release_secure_input();
        if active_seen && !secure_input_active() {
            assert_eq!(guard.state(), SecureInputState::Inactive);
        }
    }
}
