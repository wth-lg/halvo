//! The reading of the byte that `IsSecureEventInputEnabled` returns.

pub(crate) fn secure_input_on(byte: u8) -> bool {
    byte != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_reads_off_and_one_reads_on() {
        assert!(!secure_input_on(0));
        assert!(secure_input_on(1));
    }
}
