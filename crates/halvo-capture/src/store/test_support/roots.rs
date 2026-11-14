use crate::custody::dev_fallback::DevFallbackCustody;
use crate::custody::{KeyCustody, RootId};

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
