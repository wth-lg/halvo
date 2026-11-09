//! Secure-input state. Capture holds secure input by construction: Halvo uses
//! only listen-only OS capture APIs, which the OS stops delivering to while a
//! password field is focused. This module is the cross-platform seam that marks
//! that state and excludes Halvo's own windows from capture. The macOS
//! implementation is [`crate::secure_input_macos`] (phase P3); Windows enforces
//! the same property through Raw Input and the UIPI boundary (phase P4).

/// Whether the OS is currently withholding keystrokes because a secure-input
/// field (a password field) is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecureInputState {
    Active,
    Inactive,
}

/// The guard a platform backend implements so the daemon and sources can see
/// secure-input state and keep Halvo's own windows out of screen capture.
pub trait SecureInputGuard {
    /// The current secure-input state. A source consults this and records no
    /// keystroke while it is [`SecureInputState::Active`].
    fn state(&self) -> SecureInputState;

    /// Mark Halvo's own window so it never appears in captured frames.
    fn exclude_self_window(&self) -> Result<(), SecureInputError>;

    /// Convenience over [`SecureInputGuard::state`].
    fn is_active(&self) -> bool {
        self.state() == SecureInputState::Active
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SecureInputError {
    #[error("could not query secure-input state: {0}")]
    Query(String),
    #[error("could not exclude the Halvo window from capture: {0}")]
    Exclude(String),
    #[error("{0} is not implemented in this phase")]
    NotImplemented(&'static str),
}
