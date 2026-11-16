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
/// the active window, and never before it. The type holds the instants and decides; the caller
/// reads the window's state, makes the report, ends the session at an answer, and reads the clock
/// under the same lock as each decision of the show, so that the instants come in order: a clock
/// read before the first moment and handed in after the report's return would shorten the hold.
/// The callers are the alert of macOS through `Moment` and the task dialog of Windows through
/// `Show`, both in `system.rs`.
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

    /// A press at `now` that a check outside the hold refused (ruling RD-103 item 3): it answers
    /// nothing, and like a press that the hold refuses it starts an existing hold anew at `now`;
    /// a show with no start keeps none.
    #[cfg(any(test, target_os = "macos"))]
    pub fn refuse(&mut self, now: Instant) -> bool {
        if let Some(from) = self.from {
            self.from = Some(from.max(now));
        }
        false
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
/// task dialog of Windows: its hold and its report on screen, which comes at the first reading at
/// which the window is the active window and visible to the person (ruling RD-112 (b)); until that
/// report has returned every press is refused. Its caller, the task dialog in `system.rs`, reads
/// the window's activity and visibility at TDN_CREATED, at each activation and at the window's own
/// show.
#[cfg(any(test, windows))]
pub struct Show<'a> {
    guard: Cell<Guard>,
    lost: Cell<bool>,
    reported: Cell<bool>,
    on_screen: &'a dyn Fn(),
}

#[cfg(any(test, windows))]
impl<'a> Show<'a> {
    /// A show that holds every control for `arm` and reports itself on screen through
    /// `on_screen`.
    pub fn new(arm: Duration, on_screen: &'a dyn Fn()) -> Show<'a> {
        Show {
            guard: Cell::new(Guard::new(arm)),
            lost: Cell::new(false),
            reported: Cell::new(false),
            on_screen,
        }
    }

    /// The show's window became the active window, `visible` as read now (ruling RD-112 (b)):
    /// once the show has reported, the hold starts anew at `clock()`; before it, the show reports
    /// itself on screen only when the window is also visible, and an activation of a window that
    /// the person cannot see moves nothing, so that every press stays refused.
    pub fn active(&self, visible: bool, clock: &dyn Fn() -> Instant) {
        if visible || self.reported.get() {
            self.seen(clock);
        }
    }

    /// The show's window became visible, `active` when it is the active window as read now: the
    /// show's report on screen when it has made none and the window is active; nothing else.
    pub fn shown(&self, active: bool, clock: &dyn Fn() -> Instant) {
        if active && !self.reported.get() {
            self.seen(clock);
        }
    }

    /// The window is the active window and visible: the hold starts anew at `clock()`, and the
    /// first time the show reports itself on screen and the hold starts again at the reading of
    /// `clock` after that report returned, also when the report panicked.
    fn seen(&self, clock: &dyn Fn() -> Instant) {
        if self.with(|guard| guard.active(clock())) {
            self.reported.set(true);
            let _ = catch_unwind(AssertUnwindSafe(self.on_screen));
            self.with(|guard| guard.reported(clock()));
        }
    }

    /// A press of any control at `now`: whether it answers.
    pub fn press(&self, now: Instant) -> bool {
        self.with(|guard| guard.press(now))
    }

    /// The show cannot see its window become the active window again (the subclass of its window
    /// was not set): from now on the allowing control answers nothing, and any other control
    /// closes the show at once, so that it answers `reject`.
    pub fn lose(&self) {
        self.lost.set(true);
    }

    /// A press at `now` of the allowing control, for `allows`, or of another control: whether it
    /// closes the show.
    pub fn click(&self, allows: bool, now: Instant) -> bool {
        if self.lost.get() {
            return !allows;
        }
        self.press(now)
    }

    fn with<T>(&self, f: impl FnOnce(&mut Guard) -> T) -> T {
        let mut guard = self.guard.get();
        let out = f(&mut guard);
        self.guard.set(guard);
        out
    }
}
