//! The hub's presenter of `dialog_hub` over a stand-in of the system's dialog (I4.9, ruling
//! RD-77 part 12): the dialog shows the card that the core composes for a payment and nothing
//! else, and its press answers that payment alone. The hub runs over the memory ports of the
//! test targets' rig (`tests/support`), so that these tests run with the crate's unit tests.

#[path = "../../../tests/support/mod.rs"]
mod support;

use crate::{dialog_hub, Card, Core, Dialog, Hub, Reply};
use halvo_coord::items::{ConfirmAnswer, NativeCard};
use halvo_coord::types::{
    Action, AskedEffect, BoundField, BoundFrom, Choice, Class, ConfirmAsk, Digest, EffectId,
    ItemId, Limits, Name, RunEvent, RunId,
};
use halvo_coord::words;
use serde_json::json;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use support::{agent, started, world, Counts, Shared};

const WAIT: Duration = Duration::from_secs(10);
const FIRST: &str = "42.10 USD";

/// One show of the stand-in: the card that it was handed and the press that the test sends.
#[derive(Clone)]
struct Show {
    card: Card,
    press: Sender<ConfirmAnswer>,
}

/// A stand-in of the system's dialog that records the card of each show, reports the show on
/// screen and answers the press that the test sends to that show.
#[derive(Default)]
struct StandIn {
    shows: Mutex<Vec<Show>>,
}

impl Dialog for StandIn {
    fn show(&self, card: &Card, _: Duration, on_screen: &dyn Fn()) -> ConfirmAnswer {
        let (press, pressed) = channel();
        on_screen();
        self.shows.lock().unwrap().push(Show {
            card: card.clone(),
            press,
        });
        pressed.recv_timeout(WAIT).unwrap_or(ConfirmAnswer::Reject)
    }
}

impl StandIn {
    fn shows(&self, count: usize) -> Vec<Show> {
        until(|| self.shows.lock().unwrap().len() >= count);
        self.shows.lock().unwrap().clone()
    }
}

fn until(done: impl Fn() -> bool) -> bool {
    let end = Instant::now() + WAIT;
    while Instant::now() < end {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    done()
}

/// The hub of `dialog_hub` over the memory ports of `agents` manual agents, `max_live_runs` of
/// them at once, with the product's other limits, and `dialog`; ticked once.
fn hub_of(agents: usize, dialog: &Arc<StandIn>) -> Arc<Hub<Shared>> {
    let limits = Limits {
        max_live_runs: agents as u32,
        ..Limits::PRODUCT
    };
    let hub = dialog_hub(
        world(agents, limits),
        limits,
        Counts::default().sink(),
        dialog.clone(),
    );
    Core::tick(&*hub);
    hub
}

/// A run of agent `n` that asks for one payment of `amount`, each other field with a value of
/// its own; its run and its item.
fn payment(hub: &Hub<Shared>, n: usize, amount: &str) -> (RunId, ItemId) {
    let c = hub.coordinator();
    c.control(&agent(n), Action::RunNow).unwrap();
    let run = started(hub).pop().unwrap();
    let field = |name: &str, from: BoundFrom, value: &str| BoundField {
        name: Name::parse(name).unwrap(),
        from,
        value: vec![value.into()],
    };
    let ask = ConfirmAsk::new(
        run.clone(),
        agent(n),
        vec![AskedEffect {
            effect: EffectId::parse(&format!("{:032x}", 0xe1)).unwrap(),
            class: Class::Payment,
            digest: Digest::parse(&"ab".repeat(32)).unwrap(),
            digest_ok: true,
            fields: vec![
                field("payee", BoundFrom::Slot, "City Power"),
                field("amount", BoundFrom::Extract, amount),
                field("instrument", BoundFrom::Slot, "Card ending 4242"),
                field("origin", BoundFrom::Const, "Power Portal"),
            ],
            new_origin: None,
        }],
    )
    .unwrap();
    let before: Vec<ItemId> = items(hub);
    c.on_run(&run, RunEvent::Hello).unwrap();
    c.on_run(&run, RunEvent::Confirm(ask)).unwrap();
    let new: Vec<ItemId> = items(hub)
        .into_iter()
        .filter(|i| !before.contains(i))
        .collect();
    assert_eq!(new.len(), 1, "one item for the payment of {amount}");
    (run, new[0].clone())
}

fn items(hub: &Hub<Shared>) -> Vec<ItemId> {
    hub.coordinator()
        .snapshot()
        .items
        .iter()
        .map(|i| i.item().clone())
        .collect()
}

fn present(hub: &Hub<Shared>, item: &ItemId) -> Reply {
    hub.call("agents_present", Some(&json!({ "item": item.to_string() })))
}

/// Whether `card` holds the title, the lines and the two control texts of `native` and nothing
/// else, the rejecting control first.
fn is_native(card: &Card, native: &NativeCard) -> bool {
    (native.reject().choice, native.allow().choice) == (Choice::Reject, Choice::Allow)
        && card.title() == native.title()
        && card.lines() == native.lines()
        && card.reject() == native.reject().text
        && card.allow() == native.allow().text
}

/// The lines of `card` that hold `amount`.
fn holding<'a>(card: &'a Card, amount: &str) -> Vec<&'a str> {
    card.lines()
        .iter()
        .filter(|l| l.contains(amount))
        .map(String::as_str)
        .collect()
}

#[test]
fn the_dialog_shows_the_card_that_the_core_composes_for_a_payment_and_nothing_else() {
    let dialog = Arc::new(StandIn::default());
    let hub = hub_of(1, &dialog);
    let (_, item) = payment(&hub, 0, FIRST);
    assert_eq!(present(&hub, &item), Reply::Ok);
    let shows = dialog.shows(1);
    assert_eq!(shows.len(), 1, "one show");
    let native = hub.coordinator().native_card(&item).unwrap();
    let card = &shows[0].card;
    assert!(is_native(card, &native), "{card:?}\nis not\n{native:?}");
    let label = words::text("field.payment.amount", &[]).unwrap();
    let source = words::text("from.site", &[]).unwrap();
    let line = words::text(
        "confirm.field",
        &[("label", &label), ("source", &source), ("value", FIRST)],
    )
    .unwrap();
    assert_eq!(
        holding(card, FIRST),
        [line.as_str()],
        "the amount stands in one line, the line of field.payment.amount"
    );
}
