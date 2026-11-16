//! The Windows probe of `secure_input`. Windows sets no secure-input flag, so
//! the probe is the password-field detector of the keyboard source; until that
//! detector lands, every reading is [`SecureInputReading::Unknown`] and no
//! Windows keystroke gets a clearance.

use super::{seal, SecureInputProbe, SecureInputReading};

/// The probe of the Windows keyboard source.
#[derive(Debug, Clone, Copy, Default)]
pub struct PasswordFieldProbe;

impl seal::Sealed for PasswordFieldProbe {}

impl SecureInputProbe for PasswordFieldProbe {
    fn read(&self) -> SecureInputReading {
        SecureInputReading::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secure_input::{check, Withheld};

    #[test]
    fn the_probe_reads_unknown_and_gives_no_clearance() {
        assert_eq!(PasswordFieldProbe.read(), SecureInputReading::Unknown);
        assert_eq!(
            check(&PasswordFieldProbe, None).unwrap_err(),
            Withheld::Unknown
        );
    }
}
