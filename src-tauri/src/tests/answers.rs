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
    tray::take_toggle_answer(&views, 2, 3, &Err(gone()));
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
    poller::take_command_answer(&views, 2, 3, &Err(ClientError::Deadline { seconds: 5 }));
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
    views.take_delete(0, 1);
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
    poller::take_command_answer(&views, 99, 100, &Ok(paused));
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

fn store_of(views: &Views) -> (Value, Value) {
    let shown = serde_json::to_value(views.view()).expect("a view");
    (shown["store"].clone(), shown["store_bytes"].clone())
}

/// AP-17 G2, the review's case delete-01: a pause made as call 1 and a delete as call 2; the
/// delete's answer is taken first, then the pause's, whose status the service wrote before the
/// delete ran.
#[test]
fn g2_a_pause_made_before_a_delete_and_answered_after_it_shows_no_store_of_before() {
    let views = Views::default();
    assert_eq!(
        views.take_poll(0, &Ok(sample_status(Platform::Macos))),
        PollAnswer::Stands
    );
    views.take_delete(2, 3);
    views.take_mutation(1, 3, &sample_status(Platform::Macos));
    assert_eq!(store_of(&views), (Value::Null, Value::Null));
    assert_eq!(
        views.take_poll(1, &Ok(sample_status(Platform::Macos))),
        PollAnswer::DroppedAsOlderThanTheLastMutation,
        "the mark still moved"
    );
}

#[test]
fn g2_the_same_for_the_trays_toggle_and_for_a_cap_answer_that_arrives_late() {
    for take in [
        (|views: &Views| tray::take_toggle_answer(views, 1, 3, &Ok(sample_status(Platform::Macos))))
            as fn(&Views),
        |views: &Views| {
            poller::take_command_answer(views, 1, 3, &Ok(sample_status(Platform::Macos)))
        },
    ] {
        let views = Views::default();
        views.take_poll(0, &Ok(sample_status(Platform::Macos)));
        views.take_delete(2, 3);
        take(&views);
        assert_eq!(store_of(&views), (Value::Null, Value::Null));
    }
}

#[test]
fn g2_an_answer_of_a_call_made_after_the_newest_answer_replaces_the_view_as_l3_says() {
    let views = Views::default();
    views.take_poll(0, &Ok(sample_status(Platform::Macos)));
    views.take_mutation(1, 3, &sample_status(Platform::Macos));
    views.take_delete(2, 3);
    assert_eq!(
        store_of(&views),
        (Value::Null, Value::Null),
        "the delete of call 2 replaces the pause of call 1"
    );
    views.take_mutation(4, 5, &sample_status(Platform::Macos));
    assert_ne!(
        store_of(&views).0,
        Value::Null,
        "a pause of call 4, made after the delete's answer, replaces it"
    );
}

type Answer = fn() -> ClientError;

fn daemon(code: &str) -> ClientError {
    ClientError::Daemon(
        serde_json::from_value(json!({ "code": code, "message": "x" })).expect("an error reply"),
    )
}

/// AP-17 G6: a delete whose request was written and whose reply was not read as an outcome has
/// an outcome that is not known; only an answer that the contract gives before anything is
/// removed keeps the view (contracts.md C2.1 line 52, unsupported_version before the shape; line
/// 61, unsupported_op; C2.6 lines 170 and 174, busy before the store state and the ids; C15
/// item 3, line 856, peer_rejected by the client before it sends; and every error that comes
/// before the request is written).
#[test]
fn g6_a_delete_whose_reply_was_not_read_as_an_outcome_reads_connecting_and_says_its_outcome_is_not_known(
) {
    let rows: [(&str, Answer, bool); 20] = [
        ("deadline", || ClientError::Deadline { seconds: 120 }, false),
        (
            "channel",
            || ClientError::Io(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "x")),
            false,
        ),
        (
            "broken reply",
            || ClientError::Protocol("x".to_string()),
            false,
        ),
        (
            "unexpected reply",
            || ClientError::UnexpectedReply { kind: "status" },
            false,
        ),
        (
            "reply of another version",
            || ClientError::ReplyOfAnotherVersion { version: 2 },
            false,
        ),
        (
            "value unknown to v1",
            || {
                ClientError::ValueUnknownToV1(
                    "frozen"
                        .parse::<halvo_control::proto::Capture>()
                        .expect_err("a value that v1 does not know"),
                )
            },
            false,
        ),
        (
            "status that does not hold together",
            || {
                let mut f = sample_status(Platform::Macos).into_fields();
                f.desired = halvo_control::model::CaptureState::Running;
                f.capture = halvo_control::proto::Capture::Paused;
                ClientError::StateDoesNotHoldTogether(
                    DaemonStatus::new(f).expect_err("paused beside running is refused"),
                )
            },
            false,
        ),
        ("internal", || daemon("internal"), false),
        ("state_file", || daemon("state_file"), false),
        ("store_unavailable", || daemon("store_unavailable"), false),
        ("invalid_argument", || daemon("invalid_argument"), false),
        (
            "peer_rejected by the service",
            || daemon("peer_rejected"),
            false,
        ),
        ("busy", || daemon("busy"), true),
        (
            "unsupported_version",
            || daemon("unsupported_version"),
            true,
        ),
        ("unsupported_op", || daemon("unsupported_op"), true),
        ("not running", gone, true),
        (
            "peer rejected by the client",
            || ClientError::PeerRejected(halvo_control::client::PeerRejection::NotASocket),
            true,
        ),
        ("unsupported", || ClientError::Unsupported, true),
        (
            "the busy of a life that is ending",
            || {
                ClientError::ServiceEnding(
                    halvo_control::proto::ErrorReply::busy_of_an_ending_life(),
                )
            },
            true,
        ),
        (
            "an already_deleted that answers no request of this call",
            || {
                ClientError::AlreadyDeletedToThisRequest(halvo_control::proto::AlreadyDeleted {
                    current: FIXTURE_STORE_ID,
                })
            },
            true,
        ),
    ];
    for (name, answer, nothing_removed) in rows {
        let test = app_with_mutations_answering(answer, "answers-g6");
        test.poll();
        let reply = call(&test, "delete_all", json!({ "storeId": null })).expect_err(name);
        assert_eq!(
            reply["outcome_not_known"],
            json!(!nothing_removed),
            "{name}: {reply}"
        );
        let read = call(&test, "capture_status", json!({})).expect("a status");
        if nothing_removed {
            assert_ne!(read["service"], json!("connecting"), "{name}");
        } else {
            assert_eq!(read["service"], json!("connecting"), "{name}");
            assert_eq!(read["store"], Value::Null, "{name}");
        }
        assert_eq!(
            call(&test, "last_delete", json!({})).expect("a kept answer"),
            json!({ "state": "none" }),
            "{name}"
        );
    }
}

fn paused_status() -> DaemonStatus {
    use super::states::{status_with, stopped};
    status_with(Platform::Macos, |f| {
        f.desired = halvo_control::model::CaptureState::Paused;
        f.capture = halvo_control::proto::Capture::Paused;
        stopped(f);
    })
}

/// A control whose delete reads the app's kept answer at the moment it is sent, and whose cap
/// answers with a paused status; everything else as `AtTheMutation`.
struct Reading {
    places: Arc<Places>,
    app: std::sync::OnceLock<tauri::AppHandle<tauri::test::MockRuntime>>,
    kept_at_send: std::sync::Mutex<Option<crate::view_state::LastDelete>>,
    delete: fn() -> Result<DeleteOutcome, ClientError>,
}

impl Control for Reading {
    fn places(&self) -> &Arc<Places> {
        &self.places
    }
    fn status(&self, _: Made, _: bool) -> Result<DaemonStatus, ClientError> {
        Ok(sample_status(Platform::Macos))
    }
    fn inventory(&self, _: Made) -> Result<InventoryReply, ClientError> {
        Err(gone())
    }
    fn pause(&self, _: Made) -> Result<DaemonStatus, ClientError> {
        Err(gone())
    }
    fn resume(&self, _: Made) -> Result<DaemonStatus, ClientError> {
        Err(gone())
    }
    fn delete_all(&self, _: Made, _: Option<StoreId>) -> Result<DeleteOutcome, ClientError> {
        let app = self.app.get().expect("the app is set before the delete");
        let kept = app
            .state::<AppState>()
            .last_delete
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        *self.kept_at_send.lock().unwrap_or_else(|e| e.into_inner()) = Some(kept);
        (self.delete)()
    }
    fn set_storage_cap(&self, _: Made, _: StorageCap) -> Result<CapSet, ClientError> {
        Ok(CapSet {
            status: paused_status(),
            evicting: false,
        })
    }
    fn request_permission(&self, _: Made, _: Permission) -> Result<RequestOutcome, ClientError> {
        Err(gone())
    }
}

fn reading_app(
    delete: fn() -> Result<DeleteOutcome, ClientError>,
    name: &str,
) -> (TestApp, Arc<Reading>) {
    let control = Arc::new(Reading {
        places: Arc::default(),
        app: std::sync::OnceLock::new(),
        kept_at_send: std::sync::Mutex::new(None),
        delete,
    });
    let state = AppState::with_control(Arc::clone(&control) as Arc<dyn Control>, None, no_pane);
    let test = test_app_from(
        test_parts(scratch_dir(name), state),
        app::context(),
        ScriptedConnector::new(macos_fixture_daemon()),
    );
    let _ = control.app.set(test.app.handle().clone());
    (test, control)
}

/// AP-17 G4 (N1): the kept answer reads in flight at the moment the delete is sent.
#[test]
fn g4_the_kept_answer_reads_in_flight_while_the_delete_is_sent() {
    let (test, control) = reading_app(|| Ok(DeleteOutcome::Refused), "answers-g4-in-flight");
    test.poll();
    let _ = call(&test, "delete_all", json!({ "storeId": null }));
    assert_eq!(
        *control
            .kept_at_send
            .lock()
            .unwrap_or_else(|e| e.into_inner()),
        Some(crate::view_state::LastDelete::InFlight)
    );
}

/// AP-17 G4 (N1): the refused delete's arm moves the mark, so that a poll made before its
/// answer is dropped.
#[test]
fn g4_a_delete_answered_refused_moves_the_mark() {
    let (test, _) = reading_app(|| Ok(DeleteOutcome::Refused), "answers-g4-refused");
    test.poll();
    let before = test.app.state::<AppState>().control.places().next_number();
    let _ = call(&test, "delete_all", json!({ "storeId": null }));
    assert_eq!(
        test.app
            .state::<AppState>()
            .views
            .take_poll(before, &Ok(sample_status(Platform::Macos))),
        PollAnswer::DroppedAsOlderThanTheLastMutation
    );
}

/// AP-17 G4 (N1): the Ok arm of set_storage_cap takes its answer.
#[test]
fn g4_a_cap_answered_ok_replaces_the_view_with_its_status() {
    let (test, _) = reading_app(|| Ok(DeleteOutcome::Refused), "answers-g4-cap");
    test.poll();
    assert_eq!(
        tray::labels(&test.app.state::<AppState>().views.view()).status_text,
        "Capturing"
    );
    let gb = 1024 * 1024 * 1024u64;
    let _ = call(&test, "set_storage_cap", json!({ "capBytes": 10 * gb })).expect("a cap view");
    let read = call(&test, "capture_status", json!({})).expect("a status");
    assert_eq!(read["capture"], json!("paused"), "{read}");
}

/// AP-17 G4 (N1), the review's case delete-08: the tray's own Pause answered with a status
/// replaces the view and drops an older poll.
#[test]
fn g4_the_trays_own_pause_answered_paused_replaces_the_view_and_drops_an_older_poll() {
    let views = Views::default();
    assert_eq!(
        views.take_poll(1, &Ok(sample_status(Platform::Macos))),
        PollAnswer::Stands
    );
    assert_eq!(tray::labels(&views.view()).status_text, "Capturing");
    tray::take_toggle_answer(&views, 2, 3, &Ok(paused_status()));
    assert_eq!(tray::labels(&views.view()).status_text, "Paused");
    assert_eq!(
        views.take_poll(2, &Ok(sample_status(Platform::Macos))),
        PollAnswer::DroppedAsOlderThanTheLastMutation
    );
    assert_eq!(tray::labels(&views.view()).status_text, "Paused");
}

/// AP-17 G4 (N1), the review's case delete-05: a draw reads the view inside the draw lock, so a
/// draw that waited at the lock draws the view of the moment it got it.
#[test]
fn g4_a_draw_that_waits_at_the_lock_draws_the_view_of_the_moment_it_gets_the_lock() {
    let views = Arc::new(Views::default());
    views.take_poll(0, &Ok(sample_status(Platform::Macos)));
    let (entered_tx, entered) = std::sync::mpsc::channel::<()>();
    let (go, go_rx) = std::sync::mpsc::channel::<()>();
    let entered_tx = std::sync::Mutex::new(Some(entered_tx));
    let go_rx = std::sync::Mutex::new(Some(go_rx));
    let drawn = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let seen = Arc::clone(&drawn);
    views.set_drawer(poller::Drawer::new(
        Box::new(move |view| {
            seen.lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(tray::labels(view).status_text.to_string());
            if let Some(tx) = entered_tx.lock().unwrap_or_else(|e| e.into_inner()).take() {
                let _ = tx.send(());
                let rx = go_rx.lock().unwrap_or_else(|e| e.into_inner()).take();
                if let Some(rx) = rx {
                    let _ = rx.recv();
                }
            }
        }),
        Vec::new(),
    ));
    let first = Arc::clone(&views);
    let a = std::thread::spawn(move || first.draw_now());
    entered.recv().expect("the first draw holds the lock");
    let second = Arc::clone(&views);
    let b = std::thread::spawn(move || second.draw_now());
    std::thread::sleep(std::time::Duration::from_millis(300));
    views.take_mutation(1, 2, &paused_status());
    go.send(()).expect("the first draw waits");
    a.join().expect("the first draw ends");
    b.join().expect("the second draw ends");
    let drawn = drawn.lock().unwrap_or_else(|e| e.into_inner()).clone();
    assert_eq!(
        drawn.last().map(String::as_str),
        Some("Paused"),
        "{drawn:?}"
    );
}

/// AP-17 G5 (the verifier's issue 3): a delete whose work panics after it was sent leaves no
/// kept answer in flight; its outcome is not known.
#[test]
fn g5_a_delete_that_panics_leaves_no_kept_answer_in_flight() {
    let (test, control) = reading_app(
        || panic!("the delete's work panics after the request was sent"),
        "answers-g5-panic",
    );
    test.poll();
    let reply = call(&test, "delete_all", json!({ "storeId": null })).expect_err("an error");
    assert_eq!(
        *control
            .kept_at_send
            .lock()
            .unwrap_or_else(|e| e.into_inner()),
        Some(crate::view_state::LastDelete::InFlight)
    );
    assert_eq!(reply["outcome_not_known"], json!(true), "{reply}");
    assert_eq!(
        call(&test, "last_delete", json!({})).expect("a kept answer"),
        json!({ "state": "none" })
    );
    let read = call(&test, "capture_status", json!({})).expect("a status");
    assert_eq!(read["service"], json!("connecting"));
}

/// AP-19 H1, the review's case r3: a delete made as call 1 and a pause made as call 2 while the
/// delete is in flight; the service runs the pause first, and the app takes the delete's answer
/// first, then the pause's.
#[test]
fn h1_a_pause_made_while_the_delete_was_in_flight_and_run_first_by_the_service_moves_nothing() {
    let views = Views::default();
    assert_eq!(
        views.take_poll(0, &Ok(sample_status(Platform::Macos))),
        PollAnswer::Stands
    );
    views.take_delete(1, 3);
    views.take_mutation(2, 3, &sample_status(Platform::Macos));
    assert_eq!(store_of(&views), (Value::Null, Value::Null));
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Delete,
    Pause,
    Cap,
    Toggle,
}

#[derive(Clone, Copy, Debug)]
enum Event {
    Make(usize),
    Run(usize),
    Take(usize),
}

/// Every order of the calls' events: each call is made, run by the service and taken by the
/// app, in that order; the calls are made in the order of their numbers, and the service runs
/// one at a time.
fn orders(calls: usize) -> Vec<Vec<Event>> {
    fn walk(
        calls: usize,
        made: usize,
        ran: &mut Vec<bool>,
        taken: &mut Vec<bool>,
        so_far: &mut Vec<Event>,
        out: &mut Vec<Vec<Event>>,
    ) {
        if so_far.len() == 3 * calls {
            out.push(so_far.clone());
            return;
        }
        if made < calls {
            so_far.push(Event::Make(made));
            walk(calls, made + 1, ran, taken, so_far, out);
            so_far.pop();
        }
        for c in 0..made {
            if !ran[c] {
                ran[c] = true;
                so_far.push(Event::Run(c));
                walk(calls, made, ran, taken, so_far, out);
                so_far.pop();
                ran[c] = false;
            } else if !taken[c] {
                taken[c] = true;
                so_far.push(Event::Take(c));
                walk(calls, made, ran, taken, so_far, out);
                so_far.pop();
                taken[c] = false;
            }
        }
    }
    let mut out = Vec::new();
    walk(
        calls,
        0,
        &mut vec![false; calls],
        &mut vec![false; calls],
        &mut Vec::new(),
        &mut out,
    );
    out
}

/// Every arrangement of two and of three distinct calls among a delete, a pause, a cap and the
/// tray's toggle with one delete among them.
fn arrangements() -> Vec<Vec<Kind>> {
    let others = [Kind::Pause, Kind::Cap, Kind::Toggle];
    let mut out = Vec::new();
    for a in others {
        out.push(vec![Kind::Delete, a]);
        out.push(vec![a, Kind::Delete]);
        for b in others {
            if b == a {
                continue;
            }
            for at in 0..3 {
                let mut three = vec![a, b];
                three.insert(at, Kind::Delete);
                out.push(three);
            }
        }
    }
    out
}

/// AP-19 H1: for every arrangement and every order of its calls' events, no view after a
/// delete's answer names the store of before (no poll is made after it in the table).
#[test]
fn h1_no_order_of_two_or_three_calls_brings_the_store_of_before_back_after_a_deletes_answer() {
    let before = sample_status(Platform::Macos);
    let old = before.fields().store_id;
    let after = {
        let mut f = sample_status(Platform::Macos).into_fields();
        f.store_id = Some(
            "00000000000000000000000000000010"
                .parse::<StoreId>()
                .expect("an id"),
        );
        DaemonStatus::new(f).expect("a status")
    };
    let mut cases = 0usize;
    let mut bad = Vec::new();
    for kinds in arrangements() {
        let bad_before = bad.len();
        let mut counted = 0usize;
        for order in orders(kinds.len()) {
            cases += 1;
            counted += 1;
            let views = Views::default();
            views.take_poll(0, &Ok(before.clone()));
            let mut made = 0u64;
            let mut delete_ran = false;
            let mut delete_taken = false;
            let mut holds: Vec<Option<DaemonStatus>> = vec![None; kinds.len()];
            for event in &order {
                match *event {
                    Event::Make(c) => {
                        made += 1;
                        assert_eq!(
                            c as u64 + 1,
                            made,
                            "the calls are made in the order of their numbers"
                        );
                    }
                    Event::Run(c) => {
                        if kinds[c] == Kind::Delete {
                            delete_ran = true;
                        } else {
                            holds[c] = Some(if delete_ran {
                                after.clone()
                            } else {
                                before.clone()
                            });
                        }
                    }
                    Event::Take(c) => {
                        let call = c as u64 + 1;
                        let next = made + 1;
                        let status = holds[c].clone();
                        match kinds[c] {
                            Kind::Delete => {
                                views.take_delete(call, next);
                                delete_taken = true;
                            }
                            Kind::Pause => poller::take_command_answer(
                                &views,
                                call,
                                next,
                                &Ok(status.expect("run before taken")),
                            ),
                            Kind::Cap => {
                                views.take_mutation(call, next, &status.expect("run before taken"))
                            }
                            Kind::Toggle => tray::take_toggle_answer(
                                &views,
                                call,
                                next,
                                &Ok(status.expect("run before taken")),
                            ),
                        }
                        if delete_taken {
                            let shown = views.status().map(|s| s.fields().store_id);
                            if shown == Some(old) {
                                bad.push(format!("{kinds:?} {order:?}"));
                                break;
                            }
                        }
                    }
                }
            }
        }
        eprintln!(
            "h1 table: {kinds:?}: orders {counted}, bringing the store of before back {}",
            bad.len() - bad_before
        );
    }
    assert_eq!(cases, 6 * 10 + 18 * 280, "every order of every arrangement");
    assert!(
        bad.is_empty(),
        "{} of {cases} orders bring the store of before back, the first: {:?}",
        bad.len(),
        &bad[..bad.len().min(3)]
    );
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CapEnd {
    NotKnown,
    NothingChanged,
    NoStatus,
}

/// AP-19 H3: one list of the errors after which an outcome is not known serves the delete and
/// the cap Save: a cap Save that ends in any of them carries outcome_not_known, so that the page
/// shows its error words and then I-114 point 18; the errors that remove nothing change nothing.
#[test]
fn h3_a_cap_save_takes_the_outcome_from_the_same_list_as_the_delete() {
    let rows: [(&str, Answer, CapEnd); 20] = [
        (
            "deadline",
            || ClientError::Deadline { seconds: 5 },
            CapEnd::NotKnown,
        ),
        (
            "channel",
            || ClientError::Io(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "x")),
            CapEnd::NotKnown,
        ),
        (
            "broken reply",
            || ClientError::Protocol("x".to_string()),
            CapEnd::NotKnown,
        ),
        (
            "unexpected reply",
            || ClientError::UnexpectedReply { kind: "status" },
            CapEnd::NotKnown,
        ),
        (
            "reply of another version",
            || ClientError::ReplyOfAnotherVersion { version: 2 },
            CapEnd::NotKnown,
        ),
        (
            "value unknown to v1",
            || {
                ClientError::ValueUnknownToV1(
                    "frozen"
                        .parse::<halvo_control::proto::Capture>()
                        .expect_err("a value that v1 does not know"),
                )
            },
            CapEnd::NotKnown,
        ),
        (
            "status that does not hold together",
            || {
                let mut f = sample_status(Platform::Macos).into_fields();
                f.desired = halvo_control::model::CaptureState::Running;
                f.capture = halvo_control::proto::Capture::Paused;
                ClientError::StateDoesNotHoldTogether(
                    DaemonStatus::new(f).expect_err("paused beside running is refused"),
                )
            },
            CapEnd::NoStatus,
        ),
        ("internal", || daemon("internal"), CapEnd::NotKnown),
        ("state_file", || daemon("state_file"), CapEnd::NotKnown),
        (
            "store_unavailable",
            || daemon("store_unavailable"),
            CapEnd::NotKnown,
        ),
        (
            "invalid_argument",
            || daemon("invalid_argument"),
            CapEnd::NotKnown,
        ),
        (
            "peer_rejected by the service",
            || daemon("peer_rejected"),
            CapEnd::NotKnown,
        ),
        ("busy", || daemon("busy"), CapEnd::NothingChanged),
        (
            "unsupported_version",
            || daemon("unsupported_version"),
            CapEnd::NothingChanged,
        ),
        (
            "unsupported_op",
            || daemon("unsupported_op"),
            CapEnd::NothingChanged,
        ),
        ("not running", gone, CapEnd::NoStatus),
        (
            "peer rejected by the client",
            || ClientError::PeerRejected(halvo_control::client::PeerRejection::NotASocket),
            CapEnd::NothingChanged,
        ),
        ("unsupported", || ClientError::Unsupported, CapEnd::NoStatus),
        (
            "the busy of a life that is ending",
            || {
                ClientError::ServiceEnding(
                    halvo_control::proto::ErrorReply::busy_of_an_ending_life(),
                )
            },
            CapEnd::NothingChanged,
        ),
        (
            "an already_deleted that answers no request of this call",
            || {
                ClientError::AlreadyDeletedToThisRequest(halvo_control::proto::AlreadyDeleted {
                    current: FIXTURE_STORE_ID,
                })
            },
            CapEnd::NothingChanged,
        ),
    ];
    let gb = 1024 * 1024 * 1024u64;
    for (name, answer, end) in rows {
        let test = app_with_mutations_answering(answer, "answers-h3");
        test.poll();
        let reply = call(&test, "set_storage_cap", json!({ "capBytes": 10 * gb }));
        match end {
            CapEnd::NoStatus => {
                let view = reply.expect(name);
                assert_ne!(view["service"], json!("running"), "{name}: {view}");
            }
            CapEnd::NotKnown | CapEnd::NothingChanged => {
                let error = reply.expect_err(name);
                assert_eq!(
                    error["outcome_not_known"],
                    json!(end == CapEnd::NotKnown),
                    "{name}: {error}"
                );
                let removes_nothing = crate::view_state::removes_nothing(&answer());
                assert_eq!(
                    removes_nothing,
                    end == CapEnd::NothingChanged,
                    "{name}: the one list"
                );
            }
        }
    }
}
