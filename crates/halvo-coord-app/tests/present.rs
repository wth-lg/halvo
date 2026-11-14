mod support;

use halvo_coord::items::ConfirmAnswer;
use halvo_coord::mem::Call;
use halvo_coord::types::{Action, Class, Limits, Presenter, RunEvent};
use halvo_coord::types::{Cause, EffectAnswer};
use halvo_coord_app::{Core, Hub, Native, Present};
use serde_json::{json, Value};
use std::sync::Arc;
use support::{
    agent, app, ask, calls_since, fixture, invoke, mark, started, world, Counts, Dialogs, Shared,
    T0,
};

fn confirming(class: Class) -> (Arc<Hub<Shared>>, Dialogs, String) {
    let dialogs = Dialogs::default();
    let (hub, item) = confirming_with(class, dialogs.boxed());
    (hub, dialogs, item)
}

fn confirming_with(class: Class, present: Box<dyn Present>) -> (Arc<Hub<Shared>>, String) {
    let hub = Hub::new(
        world(1, Limits::PRODUCT),
        Limits::PRODUCT,
        Counts::default().sink(),
        present,
    );
    let c = hub.coordinator();
    Core::tick(&*hub);
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
    (hub, item)
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
        .with(|p| p.set_now(T0 + arm));
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

fn arm(hub: &Hub<Shared>, at: u64) {
    hub.coordinator()
        .ports()
        .inner()
        .with(|p| p.set_now(T0 + at));
    hub.coordinator().tick();
}

#[test]
fn the_dialogs_allow_reaches_the_run_as_allow_once() {
    let (hub, dialogs, item) = confirming(Class::Payment);
    let m = mark(&hub);
    assert_eq!(
        hub.call("agents_present", Some(&json!({ "item": item }))),
        halvo_coord_app::Reply::Ok
    );
    let open = dialogs.take();
    open[0].on_screen();
    arm(&hub, Limits::PRODUCT.arm_ms);
    assert_eq!(open[0].answer(ConfirmAnswer::Allow), Ok(()));
    assert_eq!(
        open[0].answer(ConfirmAnswer::Allow),
        Err(Cause::UnknownItem),
        "after an answer"
    );
    let sent: Vec<Vec<EffectAnswer>> = calls_since(&hub, m)
        .into_iter()
        .filter_map(|c| match c {
            Call::AnswerConfirm(_, answers, _) => Some(answers.iter().map(|a| a.answer).collect()),
            _ => None,
        })
        .collect();
    assert_eq!(sent, [vec![EffectAnswer::AllowOnce]]);
}

#[test]
fn an_item_whose_dialog_never_reports_on_screen_never_arms() {
    let (hub, dialogs, item) = confirming(Class::Payment);
    assert_eq!(
        hub.call("agents_present", Some(&json!({ "item": item }))),
        halvo_coord_app::Reply::Ok
    );
    let open = dialogs.take();
    arm(&hub, 20 * Limits::PRODUCT.arm_ms);
    assert_eq!(open[0].answer(ConfirmAnswer::Allow), Err(Cause::Early));
    assert_eq!(hub.coordinator().snapshot().items.len(), 1);
}

struct PresenterDroppingItsNative(Arc<std::sync::atomic::AtomicUsize>);

impl Present for PresenterDroppingItsNative {
    fn present(&self, dialog: Native) {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        drop(dialog);
    }
}

#[test]
fn a_presenter_that_drops_its_native_inside_present_can_be_handed_the_item_again() {
    let shown = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (hub, item) = confirming_with(
        Class::Payment,
        Box::new(PresenterDroppingItsNative(shown.clone())),
    );
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let args = json!({ "item": item });
        let replies = [
            hub.call("agents_present", Some(&args)),
            hub.call("agents_present", Some(&args)),
        ];
        let _ = tx.send(replies);
    });
    let replies = rx
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("agents_present returned inside 10 s");
    assert_eq!(
        replies,
        [halvo_coord_app::Reply::Ok, halvo_coord_app::Reply::Ok]
    );
    assert_eq!(shown.load(std::sync::atomic::Ordering::SeqCst), 2);
}
