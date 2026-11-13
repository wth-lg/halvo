use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

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
    pub(crate) closes: u64,
    pub(crate) shown: bool,
}

impl Bell {
    /// A bell whose window is shown, with the app's sink of the count of agents under
    /// `needs_you` (I10.3).
    pub fn new(tray: Box<dyn Fn(u32) + Send + Sync>) -> Bell {
        Bell {
            state: Mutex::new(Mark {
                changes: 0,
                closes: 0,
                shown: true,
            }),
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

    pub(crate) fn show(&self) {
        self.lock().shown = true;
        self.rung.notify_all();
    }

    pub(crate) fn close(&self) {
        let mut mark = self.lock();
        mark.shown = false;
        mark.closes += 1;
        drop(mark);
        self.rung.notify_all();
    }

    pub(crate) fn mark(&self) -> Mark {
        *self.lock()
    }

    pub(crate) fn wait(&self, seen: Mark, until: Instant) -> Mark {
        let mut mark = self.lock();
        loop {
            let now = Instant::now();
            if *mark != seen || now >= until {
                return *mark;
            }
            mark = self
                .rung
                .wait_timeout(mark, until - now)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
}
