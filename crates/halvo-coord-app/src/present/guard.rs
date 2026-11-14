#[cfg(any(test, windows))]
use std::cell::Cell;
#[cfg(any(test, windows))]
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::{Duration, Instant};

/// The hold on every control of one show of the native dialog (I4.9, ruling RD-91): a press
/// answers only once the show's report on screen has returned and `arm` has passed since the
/// latest of that return, of each later moment at which the show's window became the active
/// window, and of each press that the hold refused, so that a refused press starts the hold
/// anew. The hold is decided at the press from the instants that the caller reads on the
/// monotonic clock. The report on screen comes at the first moment at which the window became
/// the active window, and never before it.
#[derive(Clone, Copy, Debug)]
pub struct Guard {
    arm: Duration,
    active: bool,
    from: Option<Instant>,
}

impl Guard {
    /// The hold of a new show, which holds every control for `arm`.
    pub fn new(arm: Duration) -> Guard {
        Guard {
            arm,
            active: false,
            from: None,
        }
    }

    /// The show's window became the active window at `now`; `true` the first time, when the
    /// show reports itself on screen. Once the report has returned the hold starts anew.
    pub fn active(&mut self, now: Instant) -> bool {
        if let Some(from) = self.from {
            self.from = Some(from.max(now));
        }
        !std::mem::replace(&mut self.active, true)
    }

    /// The show's report on screen returned at `now`: the hold starts anew.
    pub fn reported(&mut self, now: Instant) {
        self.from = Some(self.from.map_or(now, |from| from.max(now)));
    }

    /// A press of any control at `now`: whether it answers. A press that the hold refuses
    /// answers nothing and starts the hold anew.
    pub fn press(&mut self, now: Instant) -> bool {
        match self.from {
            Some(from) if now.saturating_duration_since(from) >= self.arm => true,
            Some(from) => {
                self.from = Some(from.max(now));
                false
            }
            None => false,
        }
    }
}

/// One show of a dialog whose window's messages all come on the thread of the show's call, the
/// task dialog of Windows: its hold and its report on screen.
#[cfg(any(test, windows))]
pub struct Show<'a> {
    guard: Cell<Guard>,
    on_screen: &'a dyn Fn(),
}

#[cfg(any(test, windows))]
impl<'a> Show<'a> {
    /// A show that holds every control for `arm` and reports itself on screen through
    /// `on_screen`.
    pub fn new(arm: Duration, on_screen: &'a dyn Fn()) -> Show<'a> {
        Show {
            guard: Cell::new(Guard::new(arm)),
            on_screen,
        }
    }

    /// The show's window became the active window: the hold starts anew at `clock()`, and the
    /// first time the show reports itself on screen and the hold starts again at the reading of
    /// `clock` after that report returned, also when the report panicked.
    pub fn active(&self, clock: &dyn Fn() -> Instant) {
        if self.with(|guard| guard.active(clock())) {
            let _ = catch_unwind(AssertUnwindSafe(self.on_screen));
            self.with(|guard| guard.reported(clock()));
        }
    }

    /// A press of any control at `now`: whether it answers.
    pub fn press(&self, now: Instant) -> bool {
        self.with(|guard| guard.press(now))
    }

    fn with<T>(&self, f: impl FnOnce(&mut Guard) -> T) -> T {
        let mut guard = self.guard.get();
        let out = f(&mut guard);
        self.guard.set(guard);
        out
    }
}
