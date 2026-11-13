mod support;

use halvo_coord::items::ConfirmAnswer;
use halvo_coord::mem::Call;
use halvo_coord::types::{Action, Class, Limits, Presenter, RunEvent};
use halvo_coord_app::Hub;
use serde_json::{json, Value};
use std::sync::Arc;
use support::{
    agent, app, ask, calls_since, fixture, invoke, mark, started, world, Counts, Dialogs, Shared,
    DAY, T0,
};

fn confirming(class: Class) -> (Arc<Hub<Shared>>, Dialogs, String) {
    let dialogs = Dialogs::default();
    let hub = Hub::new(
        world(1, Limits::PRODUCT),
        Limits::PRODUCT,
        Counts::default().sink(),
        dialogs.boxed(),
    );
    let c = hub.coordinator();
    c.tick();
    c.control(&agent(0), Action::RunNow).unwrap();
    let run = started(&hub).pop().unwrap();
    c.on_run(&run, RunEvent::Hello).unwrap();
    c.on_run(&run, ask(&run, 0, class)).unwrap();
    let items = c.snapshot().items;
    assert_eq!(items.len(), 1, "{items:?}");
    let presenter = if class == Class::Payment {
        Presenter::Native
    } else {
        Presenter::Page
    };
    assert_eq!(items[0].presenter(), Some(presenter));
    let item = items[0].item().to_string();
    (hub, dialogs, item)
}

fn answers(calls: &[Call]) -> usize {
    calls
        .iter()
        .filter(|c| matches!(c, Call::AnswerConfirm(..)))
        .count()
}

#[test]
fn agents_answer_with_allow_for_a_native_item_answers_presenter_also_while_its_dialog_is_open() {
    let (hub, dialogs, item) = confirming(Class::Payment);
    let (_app, main, _other) = app(hub.clone());
    let allow = json!({"item": item, "choice": "allow"});
    let m = mark(&hub);
    assert_eq!(
        invoke(&main, "agents_answer", allow.clone()),
        Ok(fixture("reply-error-presenter"))
    );
    assert_eq!(
        invoke(&main, "agents_present", json!({"item": item})),
        Ok(fixture("reply-ok"))
    );
    assert_eq!(dialogs.count(), 1, "the presenter got the item");
    for _ in 0..2 {
        assert_eq!(
            invoke(&main, "agents_answer", allow.clone()),
            Ok(fixture("reply-error-presenter")),
            "while the dialog is open"
        );
    }
    assert_eq!(
        answers(&calls_since(&hub, m)),
        0,
        "the page answered nothing"
    );
    assert_eq!(hub.coordinator().snapshot().items.len(), 1);

    let open = dialogs.take();
    assert_eq!(open[0].item().to_string(), item);
    open[0].on_screen();
    let arm = Limits::PRODUCT.arm_ms;
    hub.coordinator()
        .ports()
        .inner()
        .with(|p| p.set_clock(T0 + arm, DAY));
    hub.coordinator().tick();
    assert_eq!(open[0].answer(ConfirmAnswer::Allow), Ok(()));
    assert_eq!(
        answers(&calls_since(&hub, m)),
        1,
        "the dialog's answer reached the run"
    );
    assert!(hub.coordinator().snapshot().items.is_empty());
    drop(open);
    assert_eq!(
        invoke(&main, "agents_present", json!({"item": item})),
        Ok(fixture("reply-error-unknown_item"))
    );
}

#[test]
fn a_second_press_of_present_while_the_dialog_is_open_shows_no_second_dialog() {
    let (hub, dialogs, item) = confirming(Class::Payment);
    let (_app, main, _other) = app(hub.clone());
    for _ in 0..3 {
        assert_eq!(
            invoke(&main, "agents_present", json!({"item": item})),
            Ok(fixture("reply-ok"))
        );
    }
    assert_eq!(dialogs.count(), 1);
    drop(dialogs.take());
    assert_eq!(
        invoke(&main, "agents_present", json!({"item": item})),
        Ok(fixture("reply-ok"))
    );
    assert_eq!(dialogs.count(), 1, "a closed dialog can be shown again");
}

#[test]
fn the_page_presents_no_item_of_its_own_in_a_native_dialog() {
    let (hub, dialogs, item) = confirming(Class::Send);
    let (_app, main, _other) = app(hub.clone());
    assert_eq!(
        invoke(&main, "agents_present", json!({"item": item})),
        Ok(fixture("reply-error-presenter"))
    );
    assert_eq!(dialogs.count(), 0);
}

#[test]
fn a_native_item_has_the_form_of_its_fixture() {
    let (hub, _dialogs, _item) = confirming(Class::Payment);
    let (_app, main, _other) = app(hub);
    let state: Value =
        serde_json::from_str(&invoke(&main, "agents_snapshot", json!({})).unwrap()).unwrap();
    let native: Value = serde_json::from_str(&fixture("state-needs_confirm_native")).unwrap();
    let keys = |v: &Value| v.as_object().map(|m| m.keys().cloned().collect::<Vec<_>>());
    assert_eq!(
        keys(&state["items"][0]),
        keys(&native["snapshot"]["items"][0])
    );
    assert_eq!(state["items"][0]["presenter"], json!("native"));
    assert_eq!(
        state["items"][0]["choices"],
        native["snapshot"]["items"][0]["choices"]
    );
}
