use crate::custody::dev_fallback::DevFallbackCustody;
use crate::custody::{KeyCustody, RootId};

/// The keychain root of a test, destroyed when the guard drops, also when the
/// test fails. A test holds it from its first line. The result of the destroy
/// is checked only when the thread is not already panicking, so a failing
/// test reports its own failure and does not abort on a second panic.
pub(crate) struct RootGuard(RootId);

impl RootGuard {
    pub(crate) fn new(root_id: RootId) -> Self {
        Self(root_id)
    }

    pub(crate) fn fresh() -> Self {
        Self(RootId::fresh())
    }

    pub(crate) fn id(&self) -> RootId {
        self.0
    }
}

impl Drop for RootGuard {
    fn drop(&mut self) {
        let destroyed = DevFallbackCustody::new(self.0).destroy_root();
        if !std::thread::panicking() {
            destroyed.expect("the root of the test is destroyed when the test ends");
        }
    }
}
