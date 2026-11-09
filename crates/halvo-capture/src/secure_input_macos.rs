//! macOS secure-input guard.
//!
//! Secure input is held by construction, not by policy. Halvo reads keystrokes
//! only through a listen-only `CGEventTap` (see [`crate::macos::input`]), and the
//! OS stops delivering key events to *every* event tap while a secure-input field
//! (an `NSSecureTextField` / password field) has focus. So a password field
//! records zero keystrokes because the events never reach Halvo at all, which the
//! gate's secure-input sub-check confirms.
//!
//! This module exposes the one place that reads the OS secure-input flag
//! ([`secure_input_active`]) and the [`crate::secure_input::SecureInputGuard`]
//! implementation the daemon binds to. Self-window exclusion from screen capture
//! is enforced structurally by the `SCContentFilter` in [`crate::macos::screen`],
//! which excludes Halvo's own running application from every frame.

use crate::secure_input::{SecureInputError, SecureInputGuard, SecureInputState};

// Secure-input calls live in the Carbon umbrella (HIToolbox). `IsSecureEventInputEnabled`
// is a read-only global predicate; `Enable`/`DisableSecureEventInput` are the
// reference-counted calls a password field itself makes. None needs permission,
// and the OS releases a process's hold automatically when it exits.
#[link(name = "Carbon", kind = "framework")]
extern "C" {
    fn IsSecureEventInputEnabled() -> u8;
    fn EnableSecureEventInput();
    fn DisableSecureEventInput();
}

/// Whether the OS is currently withholding keystrokes from event taps because a
/// secure-input (password) field is focused. The single read site for the flag.
pub fn secure_input_active() -> bool {
    // SAFETY: a parameterless C predicate with no side effects.
    unsafe { IsSecureEventInputEnabled() != 0 }
}

/// Hold secure input, the same call a focused password field makes. Used by the
/// gate's secure-input demonstration mode to put the capture pipeline under the
/// exact condition a password field creates, so the gate can confirm the
/// listen-only tap records zero keystrokes while it is held. Paired with
/// [`release_secure_input`]; the OS also releases the hold when the process exits.
pub fn hold_secure_input() {
    // SAFETY: parameterless Carbon call; reference counted, released on exit.
    unsafe { EnableSecureEventInput() };
}

/// Release a hold taken by [`hold_secure_input`].
pub fn release_secure_input() {
    // SAFETY: parameterless Carbon call, balancing one `hold_secure_input`.
    unsafe { DisableSecureEventInput() };
}

/// The macOS [`SecureInputGuard`]. Zero-sized: the secure-input state is global
/// OS state, read live on each query, so there is nothing per-instance to hold.
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
        // Enforced by construction in `crate::macos::screen`: the stream's
        // `SCContentFilter` excludes Halvo's own running application, so Halvo's
        // windows never appear in a captured frame. The capture daemon also runs
        // as a windowless process, so there is nothing of its own to record.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_reports_a_boolean_state_without_panicking() {
        // Always safe to query; in a normal session this is Inactive.
        let guard = MacosSecureInput::new();
        let _ = guard.is_active();
        assert!(guard.exclude_self_window().is_ok());
    }

    #[test]
    fn guard_tracks_a_real_secure_input_transition() {
        // Drive the OS flag and confirm the guard follows it. On a headless CI
        // runner `EnableSecureEventInput` may not take global effect (no active
        // app), so only assert the strong direction when the OS actually flips
        // the flag; the test is meaningful locally and never flaky in CI.
        let guard = MacosSecureInput::new();
        hold_secure_input();
        let active_seen = secure_input_active();
        if active_seen {
            assert_eq!(guard.state(), SecureInputState::Active);
        }
        release_secure_input();
        // After disabling our own request, if nothing else holds secure input,
        // the guard reports Inactive again.
        if active_seen && !secure_input_active() {
            assert_eq!(guard.state(), SecureInputState::Inactive);
        }
    }
}
