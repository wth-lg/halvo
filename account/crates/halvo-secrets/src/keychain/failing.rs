use super::Keychain;

pub(crate) const WRITE_PERMISSION: i32 = -61;

/// Builds with debug assertions and the features `hosted-store-tests` and `keychain`, on
/// macOS only: the step of the move into the Keychain that `Store::with_move_fault` makes
/// fail.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveFault {
    /// Every add is refused with -61, the OSStatus of a write permission error, and adds
    /// nothing.
    Add,
    /// Every read of an item's bytes gives them with the last byte changed.
    ReadBack,
}

pub(crate) struct Failing<K> {
    pub(crate) inner: K,
    pub(crate) fault: MoveFault,
}

impl<K: Keychain> Keychain for Failing<K> {
    fn count(&self, service: &str, account: &str) -> Result<usize, i32> {
        self.inner.count(service, account)
    }

    fn data(&self, service: &str, account: &str) -> Result<Vec<u8>, i32> {
        let mut bytes = self.inner.data(service, account)?;
        if self.fault == MoveFault::ReadBack {
            if let Some(last) = bytes.last_mut() {
                *last ^= 1;
            }
        }
        Ok(bytes)
    }

    fn add(&self, service: &str, account: &str, bytes: &[u8]) -> Result<(), i32> {
        match self.fault {
            MoveFault::Add => Err(WRITE_PERMISSION),
            MoveFault::ReadBack => self.inner.add(service, account, bytes),
        }
    }

    fn delete(&self, service: &str, account: &str) -> Result<(), i32> {
        self.inner.delete(service, account)
    }
}
