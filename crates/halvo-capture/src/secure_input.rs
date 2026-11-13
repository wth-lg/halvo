//! Secure-input state and the clearance that a keystroke needs.
//!
//! macOS: the probe reads the system's secure event input flag, which a focused
//! password field sets; its file is the child module `secure_input::macos`
//! (`secure_input_macos.rs`). Windows: the system sets no such flag, so the probe
//! is the password-field detector of the child module `secure_input::windows`
//! (`windows/secure_input.rs`), which reads [`SecureInputReading::Unknown`] until
//! the detector lands. A keystroke value is built only with a
//! [`KeystrokeClearance`], which [`check`] gives only for an inactive reading of
//! a key that is not late.

use std::time::Duration;

#[cfg(target_os = "macos")]
#[path = "secure_input_macos.rs"]
pub mod macos;

#[cfg(target_os = "windows")]
#[path = "windows/secure_input.rs"]
pub mod windows;

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
    /// The current secure-input state as the old two-valued type; a source
    /// takes its clearance from [`check`] instead.
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

/// One reading of a probe. No answer is [`SecureInputReading::Unknown`], never
/// [`SecureInputReading::Inactive`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecureInputReading {
    /// A password field has the focus, or the system withholds keys for it.
    Active,
    /// The probe read the state and no password field has the focus.
    Inactive,
    /// The probe could not read the state.
    Unknown,
}

mod seal {
    pub trait Sealed {}
}

/// A source of [`SecureInputReading`]s. Only a module inside `secure_input`
/// can implement it: the macOS and the Windows probe are its child modules.
pub trait SecureInputProbe: seal::Sealed {
    /// The state for the event that the caller handles now.
    fn read(&self) -> SecureInputReading;
}

/// The age above which a key event is late: it is counted, not recorded, and is
/// not a secure-input violation.
pub const LATE_KEY_MAX: Duration = Duration::from_millis(250);

/// The witness that a keystroke may be recorded: the probe read inactive for
/// it and it was not late. It holds the age that [`check`] judged, which the
/// keystroke constructor stamps with, so a key is built with no other age. It
/// is not `Clone`. Its maker is private to this module, which Rust opens to
/// this module and its child modules, the two probe files among them.
#[derive(Debug)]
pub struct KeystrokeClearance {
    age: Option<Duration>,
}

impl KeystrokeClearance {
    fn issue(age: Option<Duration>) -> Self {
        KeystrokeClearance { age }
    }

    /// The age of the key event that the clearance was given for.
    pub fn key_age(&self) -> Option<Duration> {
        self.age
    }
}

/// Why [`check`] gave no clearance, so that the source raises the right counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Withheld {
    /// The reading was active: counted in `secure_input_keystrokes`.
    Active,
    /// The reading was unknown.
    Unknown,
    /// The key was handled more than [`LATE_KEY_MAX`] after its OS time:
    /// counted in `late_events`.
    Late,
}

/// The clearance for one key event whose OS timestamp is `key_age` old, or why
/// there is none. A late key gets none whatever the reading; with no age the
/// key counts as on time.
pub fn check(
    probe: &dyn SecureInputProbe,
    key_age: Option<Duration>,
) -> Result<KeystrokeClearance, Withheld> {
    if key_age.is_some_and(|age| age > LATE_KEY_MAX) {
        return Err(Withheld::Late);
    }
    match probe.read() {
        SecureInputReading::Inactive => Ok(KeystrokeClearance::issue(key_age)),
        SecureInputReading::Active => Err(Withheld::Active),
        SecureInputReading::Unknown => Err(Withheld::Unknown),
    }
}

/// A clearance with no reading for a key of `age`, for the prefill helper alone
/// (crates/halvo-capture/src/daemon/prefill.rs), which feeds synthetic values
/// into the sink and places them in the past by their age. A build without
/// debug assertions has no such function.
#[cfg(debug_assertions)]
pub fn synthetic_clearance(age: Option<Duration>) -> KeystrokeClearance {
    KeystrokeClearance::issue(age)
}

/// A probe that answers one fixed reading, for tests of the crate.
#[cfg(test)]
pub(crate) struct FixedProbe(pub(crate) SecureInputReading);

#[cfg(test)]
impl seal::Sealed for FixedProbe {}

#[cfg(test)]
impl SecureInputProbe for FixedProbe {
    fn read(&self) -> SecureInputReading {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_unknown_and_late_yield_no_clearance() {
        let on_time = [None, Some(Duration::ZERO), Some(LATE_KEY_MAX)];
        for age in on_time {
            let inactive = FixedProbe(SecureInputReading::Inactive);
            assert!(check(&inactive, age).is_ok(), "{age:?}");
            let active = FixedProbe(SecureInputReading::Active);
            assert_eq!(
                check(&active, age).unwrap_err(),
                Withheld::Active,
                "{age:?}"
            );
            let unknown = FixedProbe(SecureInputReading::Unknown);
            assert_eq!(
                check(&unknown, age).unwrap_err(),
                Withheld::Unknown,
                "{age:?}"
            );
        }
        for age in on_time {
            let cleared = check(&FixedProbe(SecureInputReading::Inactive), age).unwrap();
            assert_eq!(cleared.key_age(), age);
        }
        let late = Some(LATE_KEY_MAX + Duration::from_nanos(1));
        for reading in [
            SecureInputReading::Inactive,
            SecureInputReading::Active,
            SecureInputReading::Unknown,
        ] {
            assert_eq!(
                check(&FixedProbe(reading), late).unwrap_err(),
                Withheld::Late
            );
        }
        assert_eq!(LATE_KEY_MAX, Duration::from_millis(250));
    }
}
