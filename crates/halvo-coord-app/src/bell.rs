use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};

/// What the waiting call and the quit hook wait on: the coordinator's change of state, which the
/// port `Page` rings, and the window `main` shown or closed by the app's hooks; and the sink of
/// the tray's count, which the port `Tray` calls. The coordinator calls both ports with its lock
/// held, so neither the bell nor the tray's sink calls back into the coordinator.
pub struct Bell {
    state: Mutex<Mark>,
    rung: Condvar,
    tray: Box<dyn Fn(u32) + Send + Sync>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Mark {
    pub(crate) changes: u64,
}

impl Bell {
    /// A bell with the app's sink of the count of agents under
    /// `needs_you` (I10.3).
    pub fn new(tray: Box<dyn Fn(u32) + Send + Sync>) -> Bell {
        Bell {
            state: Mutex::new(Mark { changes: 0 }),
            rung: Condvar::new(),
            tray,
        }
    }

    fn lock(&self) -> MutexGuard<'_, Mark> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn ring(&self) {
        self.lock().changes += 1;
        self.rung.notify_all();
    }

    pub(crate) fn attention(&self, count: u32) {
        (self.tray)(count);
    }
}
