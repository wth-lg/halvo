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
