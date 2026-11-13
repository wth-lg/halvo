use super::{decide, Card, Dialog, Outcome};
use halvo_coord::items::ConfirmAnswer;
use std::collections::VecDeque;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// The dialog of the walk (I4.9, I13.2), behind the cargo feature `scripted`, which a packaged
/// build never has. Each show reports itself on screen, then records the card it was handed,
/// then waits for the script's next press, at most `wait`; the press goes through the app's
/// mapping, and a show that gets no press by then answers as a time limit does, `reject`.
pub struct Scripted {
    script: Mutex<Script>,
    moved: Condvar,
    wait: Duration,
}

#[derive(Default)]
struct Script {
    presses: VecDeque<ConfirmAnswer>,
    cards: Vec<Card>,
}

impl Scripted {
    /// A dialog with an empty script whose shows wait at most `wait` for a press.
    pub fn new(wait: Duration) -> Scripted {
        Scripted {
            script: Mutex::new(Script::default()),
            moved: Condvar::new(),
            wait,
        }
    }

    fn script(&self) -> MutexGuard<'_, Script> {
        self.script.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Adds the press of one control, the rejecting one for `reject` and the allowing one for
    /// `allow`, to the end of the script.
    pub fn press(&self, control: ConfirmAnswer) {
        self.script().presses.push_back(control);
        self.moved.notify_all();
    }

    /// The cards that the dialog was handed, one per show, in order.
    pub fn cards(&self) -> Vec<Card> {
        self.script().cards.clone()
    }

    /// Waits at most `limit` until the dialog has been shown `shows` times; whether it has.
    pub fn wait_shown(&self, shows: usize, limit: Duration) -> bool {
        let end = Instant::now() + limit;
        let mut script = self.script();
        while script.cards.len() < shows {
            let now = Instant::now();
            if now >= end {
                return false;
            }
            script = self
                .moved
                .wait_timeout(script, end - now)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        true
    }
}

impl Dialog for Scripted {
    fn show(&self, card: &Card, on_screen: &dyn Fn()) -> ConfirmAnswer {
        on_screen();
        let end = Instant::now() + self.wait;
        let mut script = self.script();
        script.cards.push(card.clone());
        self.moved.notify_all();
        let outcome = loop {
            if let Some(press) = script.presses.pop_front() {
                break Outcome::Response(press);
            }
            let now = Instant::now();
            if now >= end {
                break Outcome::TimedOut;
            }
            script = self
                .moved
                .wait_timeout(script, end - now)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        };
        decide(outcome, ConfirmAnswer::Allow)
    }
}
