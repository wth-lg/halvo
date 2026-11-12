use crate::daemon::DaemonError;
use crate::secure_input::SecureInputState;
use crate::secure_input_macos::{SecureInputHold, SecureInputRefused};

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

pub(crate) fn hold_status(status: i32) -> Result<(), i32> {
    if status == 0 {
        Ok(())
    } else {
        Err(status)
    }
}

pub(crate) fn hold_or_error(
    hold: Option<Result<SecureInputHold, SecureInputRefused>>,
) -> Result<Option<SecureInputHold>, DaemonError> {
    match hold {
        None => Ok(None),
        Some(Ok(hold)) => Ok(Some(hold)),
        Some(Err(SecureInputRefused(status))) => Err(DaemonError::SecureInputRefused(status)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secure_input_macos::test_support::{hold_or_skip, one_hold_at_a_time};

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
    fn a_run_goes_on_with_no_hold_or_a_hold_and_ends_with_an_error_on_a_refusal() {
        assert!(matches!(hold_or_error(None), Ok(None)));
        match hold_or_error(Some(Err(SecureInputRefused(-50)))) {
            Err(error) => assert_eq!(
                error.to_string(),
                "the system refused the secure-input hold (status -50)"
            ),
            Ok(_) => panic!("a refused hold let the run go on"),
        }
        let _one = one_hold_at_a_time();
        let Some(hold) = hold_or_skip(
            "a_run_goes_on_with_no_hold_or_a_hold_and_ends_with_an_error_on_a_refusal",
        ) else {
            return;
        };
        assert!(
            matches!(hold_or_error(Some(Ok(hold))), Ok(Some(_))),
            "a hold did not let the run go on"
        );
    }
}
