use std::sync::Arc;

use halvo_control::client::{ClientError, DeleteOutcome};
use halvo_control::model::{StorageCap, StoreId};
use halvo_control::onboarding::{Permission, RequestOutcome};
use halvo_control::proto::{CapSet, DaemonStatus, InventoryReply, Platform};
use halvo_control::testing::{sample_status, ScriptedConnector, FIXTURE_STORE_ID};
use serde_json::{json, Value};
use tauri::Manager;

use super::{
    invoke, macos_fixture_daemon, main_window, scratch_dir, test_app_from, test_app_with,
    test_parts, TestApp, APP_URL,
};
use crate::app;
use crate::commands::capture::AppState;
use crate::commands::control::{Control, Made, Places};
use crate::poller::{self, PollAnswer, Views};
use crate::tray;

fn call(test: &TestApp, command: &str, args: Value) -> Result<Value, Value> {
    invoke(&main_window(&test.app), APP_URL, command, args)
}

fn no_pane(_: Permission) -> Result<(), String> {
    Ok(())
}

fn gone() -> ClientError {
    ClientError::Unreachable {
        endpoint: "x".to_string(),
    }
}

/// Status answers running; every mutation answers the error that `answer` makes.
struct AtTheMutation {
    places: Arc<Places>,
    answer: fn() -> ClientError,
}

impl Control for AtTheMutation {
    fn places(&self) -> &Arc<Places> {
        &self.places
    }
    fn status(&self, _: Made, _: bool) -> Result<DaemonStatus, ClientError> {
        Ok(sample_status(Platform::Macos))
    }
    fn inventory(&self, _: Made) -> Result<InventoryReply, ClientError> {
        Err((self.answer)())
    }
    fn pause(&self, _: Made) -> Result<DaemonStatus, ClientError> {
        Err((self.answer)())
    }
    fn resume(&self, _: Made) -> Result<DaemonStatus, ClientError> {
        Err((self.answer)())
    }
    fn delete_all(&self, _: Made, _: Option<StoreId>) -> Result<DeleteOutcome, ClientError> {
        Err((self.answer)())
    }
    fn set_storage_cap(&self, _: Made, _: StorageCap) -> Result<CapSet, ClientError> {
        Err((self.answer)())
    }
    fn request_permission(&self, _: Made, _: Permission) -> Result<RequestOutcome, ClientError> {
        Err((self.answer)())
    }
}

fn app_with_mutations_answering(answer: fn() -> ClientError, name: &str) -> TestApp {
    let state = AppState::with_control(
        Arc::new(AtTheMutation {
            places: Arc::default(),
            answer,
        }),
        None,
        no_pane,
    );
    test_app_from(
        test_parts(scratch_dir(name), state),
        app::context(),
        ScriptedConnector::new(macos_fixture_daemon()),
    )
}

#[test]
fn after_a_deleted_answer_the_app_serves_no_store_of_before_and_after_one_poll_the_store_of_the_answer(
) {
    let test = test_app_with(scratch_dir("answers-deleted"), macos_fixture_daemon());
    test.poll();
    let old = FIXTURE_STORE_ID.to_string();
    let reply = call(&test, "delete_all", json!({ "storeId": old })).expect("a reply");
    assert_eq!(reply["outcome"], json!("deleted"));
    let after = call(&test, "capture_status", json!({})).expect("a status");
    assert_eq!(after["service"], json!("connecting"));
    assert_eq!(after["platform"], json!("macos"));
    for key in [
        "store",
        "store_bytes",
        "store_logical_bytes",
        "store_cap_bytes",
    ] {
        assert_eq!(after[key], Value::Null, "{key} after the answer");
    }
    test.poll();
    let polled = call(&test, "capture_status", json!({})).expect("a status");
    assert_eq!(polled["store"]["id"], reply["new_store_id"]);
    assert_ne!(polled["store"]["id"], json!(old));
}

#[test]
fn after_an_already_deleted_answer_the_app_serves_no_store_of_before() {
    let test = test_app_with(scratch_dir("answers-already"), macos_fixture_daemon());
    test.poll();
    let old = json!(FIXTURE_STORE_ID.to_string());
    call(&test, "delete_all", json!({ "storeId": old })).expect("a reply");
    test.poll();
    let again = call(&test, "delete_all", json!({ "storeId": old })).expect("a reply");
    assert_eq!(again["outcome"], json!("already_deleted"));
    let after = call(&test, "capture_status", json!({})).expect("a status");
    assert_eq!(after["service"], json!("connecting"));
    assert_eq!(after["store"], Value::Null);
}

#[test]
fn a_press_right_after_a_delete_is_never_answered_refused_for_the_shown_id() {
    let test = test_app_with(scratch_dir("answers-second-press"), macos_fixture_daemon());
    test.poll();
    call(
        &test,
        "delete_all",
        json!({ "storeId": FIXTURE_STORE_ID.to_string() }),
    )
    .expect("a reply");
    let shown = call(&test, "capture_status", json!({})).expect("a status");
    assert_eq!(
        shown["store"],
        Value::Null,
        "the page has no store id to arm before a poll made after the answer"
    );
    test.poll();
    let shown = call(&test, "capture_status", json!({})).expect("a status");
    let second = call(
        &test,
        "delete_all",
        json!({ "storeId": shown["store"]["id"] }),
    )
    .expect("a reply");
    assert_ne!(second["outcome"], json!("delete_refused"));
}

#[test]
fn a_pause_or_a_cap_answered_not_running_is_followed_by_not_running_on_the_page_and_on_the_tray() {
    for command in ["set_capture_state", "set_storage_cap"] {
        let test = app_with_mutations_answering(gone, "answers-not-running");
        test.poll();
        let args = if command == "set_capture_state" {
            json!({ "capture": "paused" })
        } else {
            json!({ "capBytes": 10_737_418_240u64 })
        };
        let reply = call(&test, command, args).expect("a reply");
        assert_eq!(reply["service"], json!("not_running"), "{command}");
        let read = call(&test, "capture_status", json!({})).expect("a status");
        assert_eq!(read["service"], json!("not_running"), "{command}");
        let view = test.app.state::<AppState>().views.view();
        assert_eq!(
            tray::labels(&view).status_text,
            "Capture service not running",
            "{command}"
        );
    }
}

#[test]
fn the_trays_own_pause_answered_not_running_is_followed_by_not_running() {
    let views = Views::default();
    assert_eq!(
        views.take_poll(1, &Ok(sample_status(Platform::Macos))),
        PollAnswer::Stands
    );
    tray::take_toggle_answer(&views, 3, &Err(gone()));
    assert_eq!(
        tray::labels(&views.view()).status_text,
        "Capture service not running"
    );
    assert_eq!(
        views.take_poll(2, &Ok(sample_status(Platform::Macos))),
        PollAnswer::DroppedAsOlderThanTheLastMutation
    );
}

#[test]
fn an_answer_returned_as_err_leaves_the_view_and_moves_the_mark() {
    let views = Views::default();
    views.take_poll(1, &Ok(sample_status(Platform::Macos)));
    let before = views.view();
    poller::take_command_answer(&views, 3, &Err(ClientError::Deadline { seconds: 5 }));
    assert_eq!(views.view(), before);
    assert_eq!(
        views.take_poll(2, &Ok(sample_status(Platform::Macos))),
        PollAnswer::DroppedAsOlderThanTheLastMutation
    );
    assert_eq!(
        views.take_poll(3, &Ok(sample_status(Platform::Macos))),
        PollAnswer::Stands
    );
}

#[test]
fn a_delete_whose_answer_never_came_leaves_the_connecting_view() {
    for (name, answer) in [
        (
            "deadline",
            (|| ClientError::Deadline { seconds: 5 }) as fn() -> ClientError,
        ),
        ("channel", || {
            ClientError::Io(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "x"))
        }),
    ] {
        let test = app_with_mutations_answering(answer, "answers-delete-lost");
        test.poll();
        let reply = call(&test, "delete_all", json!({ "storeId": null }));
        assert!(reply.is_err(), "{name}");
        let read = call(&test, "capture_status", json!({})).expect("a status");
        assert_eq!(read["service"], json!("connecting"), "{name}");
        assert_eq!(read["store"], Value::Null, "{name}");
    }
}

#[test]
fn a_delete_answer_wakes_the_poller_before_its_period() {
    let views = Arc::new(Views::default());
    let sleeper = Arc::clone(&views);
    let started = std::time::Instant::now();
    let slept = std::thread::spawn(move || {
        sleeper.sleep(std::time::Duration::from_secs(30));
        started.elapsed()
    });
    std::thread::sleep(std::time::Duration::from_millis(100));
    views.take_delete(1);
    let elapsed = slept.join().expect("the sleeper ends");
    assert!(elapsed < std::time::Duration::from_secs(10), "{elapsed:?}");
}

#[test]
fn the_kept_delete_is_none_at_start_answered_after_a_delete_and_none_after_one_that_failed() {
    let test = test_app_with(scratch_dir("answers-kept"), macos_fixture_daemon());
    test.poll();
    assert_eq!(
        call(&test, "last_delete", json!({})).expect("a reply"),
        json!({ "state": "none" })
    );
    let reply = call(
        &test,
        "delete_all",
        json!({ "storeId": FIXTURE_STORE_ID.to_string() }),
    )
    .expect("a reply");
    let kept = call(&test, "last_delete", json!({})).expect("a reply");
    assert_eq!(kept["state"], json!("answered"));
    assert_eq!(kept["answer"], reply);
    assert!(
        kept["at_ms"]
            .as_u64()
            .is_some_and(|ms| ms > 1_700_000_000_000),
        "{kept}"
    );
    let failing = app_with_mutations_answering(
        || ClientError::Deadline { seconds: 5 },
        "answers-kept-failed",
    );
    failing.poll();
    assert!(call(&failing, "delete_all", json!({ "storeId": null })).is_err());
    assert_eq!(
        call(&failing, "last_delete", json!({})).expect("a reply"),
        json!({ "state": "none" })
    );
}

#[test]
fn the_kept_delete_is_dropped_when_the_window_main_is_built_again() {
    let test = test_app_with(scratch_dir("answers-kept-new-view"), macos_fixture_daemon());
    test.poll();
    call(
        &test,
        "delete_all",
        json!({ "storeId": FIXTURE_STORE_ID.to_string() }),
    )
    .expect("a reply");
    assert_eq!(
        call(&test, "last_delete", json!({})).expect("a reply")["state"],
        json!("answered")
    );
    let _ = app::main_window(
        test.app.handle(),
        #[cfg(windows)]
        None,
    );
    assert_eq!(
        *test
            .app
            .state::<AppState>()
            .last_delete
            .lock()
            .expect("the kept delete"),
        crate::view_state::LastDelete::None
    );
}

#[test]
fn a_pause_answered_between_the_poll_and_the_draw_is_drawn_last() {
    use super::states::{status_with, stopped};
    use halvo_control::proto::Capture;
    use std::sync::{mpsc, Mutex};
    let views = Arc::new(Views::default());
    let drawn = Arc::new(Mutex::new(Vec::<&'static str>::new()));
    let (inside, entered) = mpsc::channel::<()>();
    let (go, held) = mpsc::channel::<()>();
    let held = Mutex::new(Some(held));
    let recorder = Arc::clone(&drawn);
    views.set_drawer(poller::Drawer::new(
        Box::new(move |view| {
            let first = held.lock().expect("the gate").take();
            if let Some(held) = first {
                inside.send(()).expect("the test waits");
                held.recv().expect("the test releases the first draw");
            }
            recorder
                .lock()
                .expect("the drawn lines")
                .push(tray::labels(view).status_text);
        }),
        Vec::new(),
    ));
    let control = AtTheMutation {
        places: Arc::default(),
        answer: gone,
    };
    let rounds = Arc::clone(&views);
    let round = std::thread::spawn(move || {
        poller::round(&control, &rounds, None);
    });
    entered.recv().expect("the round's draw has begun");
    let paused = status_with(Platform::Macos, |f| {
        f.desired = halvo_control::model::CaptureState::Paused;
        f.capture = Capture::Paused;
        stopped(f);
    });
    poller::take_command_answer(&views, 100, &Ok(paused));
    let drawer = Arc::clone(&views);
    let pause_draw = std::thread::spawn(move || drawer.draw_now());
    std::thread::sleep(std::time::Duration::from_millis(200));
    go.send(()).expect("the first draw waits");
    round.join().expect("the round ends");
    pause_draw.join().expect("the pause's draw ends");
    assert_eq!(
        *drawn.lock().expect("the drawn lines"),
        ["Capturing", "Paused"],
        "the later reading of Views is drawn last"
    );
}
