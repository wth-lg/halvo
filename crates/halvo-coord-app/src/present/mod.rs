mod card;
mod chooser;
mod guard;
#[cfg(any(test, target_os = "macos"))]
mod moment;
mod outcome;
#[cfg(any(test, feature = "scripted"))]
mod scripted;
mod system;
#[cfg(test)]
mod tests;

pub use card::Card;
pub use chooser::SystemChooser;
pub use outcome::{decide, Outcome};
#[cfg(feature = "scripted")]
pub use scripted::Scripted;
pub use system::System;

use crate::hub::{Hub, Native, Present};
use halvo_coord::guard::Policy;
use halvo_coord::items::ConfirmAnswer;
use halvo_coord::ports::{Analysis, Clock, Files, Owner, Rng, Runs, Senders};
use halvo_coord::types::{Cause, ConfigView, ItemId, Limits};
use std::cell::Cell;
use std::sync::{Arc, OnceLock, Weak};
use std::thread;
use std::time::Duration;

/// A dialog that shows a card and returns the person's answer (I4.9): `allow` only for the
/// press on its allowing control. `show` returns when the dialog has closed; it calls
/// `on_screen` on the calling thread once the dialog is on screen, and not at all when the
/// dialog was not shown. The system's dialog holds every control of each show for `arm`
/// (ruling RD-91): it reports the show on screen no earlier than the moment at which its window
/// became the active window, and a press inside `arm` after the return of that report, after
/// each later moment at which the window became the active window again or after a press that
/// it refused answers nothing.
pub trait Dialog: Send + Sync + 'static {
    /// Shows `card`, holding every control for `arm`, and returns the answer.
    fn show(&self, card: &Card, arm: Duration, on_screen: &dyn Fn()) -> ConfirmAnswer;
}

/// The reader that the app hands the presenter: the card of a native item, or the cause why
/// the item has none.
pub type Read = dyn Fn(&ItemId) -> Result<Card, Cause> + Send + Sync;

/// The presenter of native confirmations (I4.9). `present` returns at once; on a thread of its
/// own it reads the item's card, shows it in `D`, reports each show on screen through
/// `Native::on_screen`, and hands the answer to `Native::answer`, `reject` for a show that never
/// reported itself on screen. An answer refused as `early`
/// shows the dialog once more, as long as the dialog was on screen; any other reply ends the
/// thread, and so does a card that the reader refuses or that breaks the composed form (ruling
/// RD-90 item 10), before anything is shown and with no answer. The `Native` is dropped when the thread ends, so the item can be shown again by a
/// later `agents_present`. Each show holds every control of the dialog for `arm`, the limit
/// `arm_ms` (I4.2), so `armed` for a native item says that the core takes an answer, not that
/// the dialog's controls are ready (ruling RD-91 item 5).
pub struct DialogPresenter<D: Dialog> {
    dialog: Arc<D>,
    read: Arc<Read>,
    arm: Duration,
}

impl<D: Dialog> DialogPresenter<D> {
    /// A presenter that shows the cards that `read` gives in `dialog`, each show holding every
    /// control for `arm`, the limit `arm_ms`.
    pub fn new(
        dialog: Arc<D>,
        read: impl Fn(&ItemId) -> Result<Card, Cause> + Send + Sync + 'static,
        arm: Duration,
    ) -> DialogPresenter<D> {
        DialogPresenter {
            dialog,
            read: Arc::new(read),
            arm,
        }
    }

    fn spawn<T: Target + Send + 'static>(&self, target: T) {
        let dialog = self.dialog.clone();
        let read = self.read.clone();
        let arm = self.arm;
        let _ = thread::Builder::new()
            .name("halvo-dialog".into())
            .spawn(move || serve(&target, &*read, &*dialog, arm));
    }
}

impl<D: Dialog> Present for DialogPresenter<D> {
    fn present(&self, dialog: Native) {
        self.spawn(dialog);
    }
}

/// A hub of a new life over `ports` and `limits`, as `Hub::new` makes it, whose native
/// confirmations go to a `DialogPresenter` over `dialog` (I4.9): its reader is the card of the
/// native dialog that the hub's own coordinator composes for the item
/// (`Coordinator::native_card`, ruling RD-77 part 12), taken by `Card::native`, and its hold is
/// `arm_ms` of `limits`. The reader is bound to the hub once `Hub::new` has returned and holds it
/// weakly, so that the presenter keeps no hub alive; a read before that bind or after the hub
/// has gone answers `no_service`, and the presenter then shows nothing and answers nothing.
pub fn dialog_hub<P, D>(
    ports: P,
    limits: Limits,
    tray: Box<dyn Fn(u32) + Send + Sync>,
    dialog: Arc<D>,
) -> Arc<Hub<P>>
where
    P: Owner
        + Runs
        + Analysis
        + Clock
        + Rng
        + Policy<Config = <P as Owner>::Config>
        + Senders
        + Files
        + Send
        + Sync
        + 'static,
    <P as Owner>::Config: ConfigView + Clone + Send + 'static,
    D: Dialog,
{
    let bound: Arc<OnceLock<Weak<Hub<P>>>> = Arc::new(OnceLock::new());
    let hub_of = bound.clone();
    let read = move |item: &ItemId| {
        let hub = hub_of
            .get()
            .and_then(Weak::upgrade)
            .ok_or(Cause::NoService)?;
        let card = hub.coordinator().native_card(item)?;
        Card::native(&card)
    };
    let arm = Duration::from_millis(limits.arm_ms);
    let presenter = DialogPresenter::new(dialog, read, arm);
    let hub = Hub::new(ports, limits, tray, Box::new(presenter));
    let _ = bound.set(Arc::downgrade(&hub));
    hub
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

fn serve<T: Target + ?Sized, D: Dialog + ?Sized>(
    target: &T,
    read: &Read,
    dialog: &D,
    arm: Duration,
) {
    let Some(card) = read(target.item()).ok().filter(Card::whole) else {
        return;
    };
    loop {
        let shown = Cell::new(false);
        let answer = dialog.show(&card, arm, &|| {
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
