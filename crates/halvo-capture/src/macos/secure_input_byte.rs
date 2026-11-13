use crate::daemon::DaemonError;
use crate::secure_input_macos::SecureInputRefused;

pub(crate) fn secure_input_on(byte: u8) -> bool {
    byte != 0
}

pub(crate) fn hold_status(status: i32) -> Result<(), i32> {
    if status == 0 {
        Ok(())
    } else {
        Err(status)
    }
}

pub(crate) fn take_hold(enable: fn() -> i32) -> Result<(), i32> {
    hold_status(enable())
}

pub(crate) fn release_hold(disable: fn() -> i32) {
    disable();
}

pub(crate) fn refusal_or_none(refusal: Option<SecureInputRefused>) -> Result<(), DaemonError> {
    match refusal {
        None => Ok(()),
        Some(SecureInputRefused(status)) => Err(DaemonError::SecureInputRefused(status)),
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

    #[test]
    fn a_run_goes_on_with_no_refusal_and_ends_with_an_error_on_a_refusal() {
        assert!(refusal_or_none(None).is_ok());
        match refusal_or_none(Some(SecureInputRefused(-50))) {
            Err(error) => assert_eq!(
                error.to_string(),
                "the system refused the secure-input hold (status -50)"
            ),
            Ok(()) => panic!("a refused hold let the run go on"),
        }
    }
}
