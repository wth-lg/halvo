use super::{decide, Card, Dialog, Outcome};
use halvo_coord::items::ConfirmAnswer;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// The dialog of the walk (I4.9, I13.2), behind the cargo feature `scripted`, which a packaged
/// build never has. Each show reports itself on screen, then records the card it was handed,
/// then waits at most `wait` for the script's press for that show, by its number; the press
/// goes through the app's mapping, and a show that gets no press of its own by then answers as
/// a time limit does, `reject`. A press never answers another show than the one it names, so a
/// late press, and one for a show that ended, answers nothing.
pub struct Scripted {
    script: Mutex<Script>,
    moved: Condvar,
    wait: Duration,
}

#[derive(Default)]
struct Script {
    presses: Vec<(usize, ConfirmAnswer)>,
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

    /// Adds to the script the press of one control in show `show`, counted from 1 over every
    /// show of this dialog: the rejecting control for `reject` and the allowing one for `allow`.
    pub fn press(&self, show: usize, control: ConfirmAnswer) {
        self.script().presses.push((show, control));
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
        let show = script.cards.len();
        self.moved.notify_all();
        let outcome = loop {
            if let Some(i) = script.presses.iter().position(|(n, _)| *n == show) {
                break Outcome::Response(script.presses.remove(i).1);
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
