mod card;
mod outcome;
#[cfg(any(test, feature = "scripted"))]
mod scripted;
mod system;
#[cfg(test)]
mod tests;

pub use card::Card;
pub use outcome::{decide, Outcome};
#[cfg(feature = "scripted")]
pub use scripted::Scripted;
pub use system::System;

use crate::hub::{Native, Present};
use halvo_coord::items::ConfirmAnswer;
use halvo_coord::types::{Cause, ItemId};
use std::cell::Cell;
use std::sync::Arc;
use std::thread;

/// A dialog that shows a card and returns the person's answer (I4.9): `allow` only for the
/// press on its allowing control. `show` returns when the dialog has closed; it calls
/// `on_screen` on the calling thread once the dialog is on screen, and not at all when the
/// dialog was not shown.
pub trait Dialog: Send + Sync + 'static {
    /// Shows `card` and returns the answer.
    fn show(&self, card: &Card, on_screen: &dyn Fn()) -> ConfirmAnswer;
}

/// The reader that the app hands the presenter: the card of a native item, or the cause why
/// the item has none.
pub type Read = dyn Fn(&ItemId) -> Result<Card, Cause> + Send + Sync;

/// The presenter of native confirmations (I4.9). `present` returns at once; on a thread of its
/// own it reads the item's card, shows it in `D`, reports each show on screen through
/// `Native::on_screen`, and hands the answer to `Native::answer`, `reject` for a show that never
/// reported itself on screen. An answer refused as `early`
/// shows the dialog once more, as long as the dialog was on screen; any other reply ends the
/// thread, and so does a card that the reader refuses, before anything is shown and with no
/// answer. The `Native` is dropped when the thread ends, so the item can be shown again by a
/// later `agents_present`.
pub struct DialogPresenter<D: Dialog> {
    dialog: Arc<D>,
    read: Arc<Read>,
}

impl<D: Dialog> DialogPresenter<D> {
    /// A presenter that shows the cards that `read` gives in `dialog`.
    pub fn new(
        dialog: Arc<D>,
        read: impl Fn(&ItemId) -> Result<Card, Cause> + Send + Sync + 'static,
    ) -> DialogPresenter<D> {
        DialogPresenter {
            dialog,
            read: Arc::new(read),
        }
    }

    fn spawn<T: Target + Send + 'static>(&self, target: T) {
        let dialog = self.dialog.clone();
        let read = self.read.clone();
        let _ = thread::Builder::new()
            .name("halvo-dialog".into())
            .spawn(move || serve(&target, &*read, &*dialog));
    }
}

impl<D: Dialog> Present for DialogPresenter<D> {
    fn present(&self, dialog: Native) {
        self.spawn(dialog);
    }
}

trait Target {
    fn item(&self) -> &ItemId;
    fn on_screen(&self);
    fn answer(&self, answer: ConfirmAnswer) -> Result<(), Cause>;
}

impl Target for Native {
    fn item(&self) -> &ItemId {
        Native::item(self)
    }

    fn on_screen(&self) {
        Native::on_screen(self);
    }

    fn answer(&self, answer: ConfirmAnswer) -> Result<(), Cause> {
        Native::answer(self, answer)
    }
}

fn serve<T: Target + ?Sized, D: Dialog + ?Sized>(target: &T, read: &Read, dialog: &D) {
    let Ok(card) = read(target.item()) else {
        return;
    };
    loop {
        let shown = Cell::new(false);
        let answer = dialog.show(&card, &|| {
            shown.set(true);
            target.on_screen();
        });
        let answer = if shown.get() {
            answer
        } else {
            ConfirmAnswer::Reject
        };
        match target.answer(answer) {
            Err(Cause::Early) if shown.get() => continue,
            _ => return,
        }
    }
}
