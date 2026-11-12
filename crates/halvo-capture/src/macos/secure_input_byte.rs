use crate::secure_input::SecureInputState;

pub(crate) fn secure_input_on(byte: u8) -> bool {
    byte != 0
}

pub(crate) fn secure_input_state(on: bool) -> SecureInputState {
    if on {
        SecureInputState::Active
    } else {
        SecureInputState::Inactive
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_reads_off_and_one_reads_on() {
        assert!(!secure_input_on(0));
        assert!(secure_input_on(1));
    }

    #[test]
    fn zero_reads_off_and_every_other_byte_reads_on() {
        assert!(!secure_input_on(0));
        for byte in [1, 2, 255] {
            assert!(secure_input_on(byte), "the byte {byte} reads as off");
        }
    }
}
