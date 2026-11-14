use super::guard::Guard;
use std::time::Instant;

/// One reading of the alert's moment on macOS: whether the app is active and whether the alert's
/// window is its key window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pair {
    /// The app is the active app.
    pub active: bool,
    /// The alert's window is the app's key window.
    pub key: bool,
}

impl Pair {
    /// Whether the alert's window is the active window: the key window of the active app.
    pub fn both(self) -> bool {
        self.active && self.key
    }
}

/// What makes the alert's moment be decided (ruling RD-103 item 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// The alert's window became the key window, or the app became active: read in the level form.
    Become,
    /// The alert's window resigned key, or the app resigned active: no reading is made.
    Resign,
    /// The repeating reading on the main run loop, which acts on an edge alone.
    Reading,
    /// A press of a control of the alert; `modal` when the button's window is the modal window.
    Press { modal: bool },
}

/// What the caller does after a decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// Nothing.
    Nothing,
    /// The window became the active window and the hold starts anew; `first` for the show's first
    /// moment, at which the show reports itself on screen.
    Moment { first: bool },
    /// The press answers: the session ends with the button's response.
    Answer,
    /// The press answers nothing; `moment` when it took a moment, `Some(true)` for the first.
    Refused { moment: Option<bool> },
}

/// The moment of one show of the alert (ruling RD-103): the pair that the last reading stored.
/// A press answers only after a moment that a reading on the main thread observed for this show,
/// and only `arm` after the latest of the return of the first moment's report, each later moment
/// and the last refused press. A become notice reading both true is a moment, also when the
/// stored pair was already true; a resign notice stores not true without a reading; the repeating
/// reading and a press take a moment on an edge alone (the stored pair not both true, the pair
/// now both true), and a reading of true after true moves nothing. A press that takes a moment
/// is refused; a press while the pair is not both true, or while the button's window is not the
/// modal window, answers nothing and starts an existing hold anew.
#[derive(Clone, Copy, Debug, Default)]
pub struct Moment {
    both: bool,
}

impl Moment {
    /// Decides `event` at `now` over `guard`, with the pair `read` now, `None` at a resign.
    pub fn decide(
        &mut self,
        guard: &mut Guard,
        event: Event,
        read: Option<Pair>,
        now: Instant,
    ) -> Decision {
        let was = self.both;
        let both = read.is_some_and(Pair::both);
        self.both = both && event != Event::Resign;
        match event {
            Event::Resign => Decision::Nothing,
            Event::Become if both => Decision::Moment {
                first: guard.active(now),
            },
            Event::Reading if both && !was => Decision::Moment {
                first: guard.active(now),
            },
            Event::Become | Event::Reading => Decision::Nothing,
            Event::Press { .. } if both && !was => Decision::Refused {
                moment: Some(guard.active(now)),
            },
            Event::Press { modal } if !(both && modal) => {
                guard.refuse(now);
                Decision::Refused { moment: None }
            }
            Event::Press { .. } if guard.press(now) => Decision::Answer,
            Event::Press { .. } => Decision::Refused { moment: None },
        }
    }
}
