use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use halvo_control::client::{ClientError, Connector, DeleteOutcome, NoTransport};
use halvo_control::model::{StorageCap, StoreId};
use halvo_control::onboarding::{Permission, RequestOutcome};
use halvo_control::paths::DataDir;
use halvo_control::proto::{
    CapSet, Capture, DaemonStatus, InventoryReply, Platform, Request, StateFile, StoreErrorKind,
    StoreState,
};
use halvo_control::testing::{
    sample_status, store_id, ScriptedConnector, ScriptedDaemon, FIXTURE_STORE_ID,
};
use serde_json::{json, Value};
use tauri::Manager;

use super::states::{status_with, stopped, store};
use super::{
    invoke, macos_fixture_daemon, main_window, scratch_dir, test_app_from, test_app_with,
    test_parts, TestApp, APP_URL,
};
use crate::app;
use crate::commands::capture::AppState;
use crate::commands::control::{Control, Gated, Made, Places};
use crate::commands::names::{Names, CAPTURE};
use crate::commands::Registration;
use crate::poller::{self, PollAnswer, Views};
use crate::tray;
use crate::view_state::{NoStatusService, RunningView, Service, Status, StatusView};

fn running_view(view: StatusView) -> RunningView {
    match view {
        StatusView::Running(view) => *view,
        StatusView::NoStatus(view) => panic!("a running view, not {:?}", view.service),
    }
}

fn no_pane(_: Permission) -> Result<(), String> {
    Ok(())
}

fn app_on(daemon: ScriptedDaemon, name: &str) -> TestApp {
    test_app_with(scratch_dir(name), daemon)
}

fn call(test: &TestApp, command: &str, args: Value) -> Result<Value, Value> {
    invoke(&main_window(&test.app), APP_URL, command, args)
}

fn hex(id: StoreId) -> String {
    id.to_string()
}

#[test]
fn the_app_state_of_a_failing_resolver_builds_and_its_set_up_returns_ok() {
    let state = AppState::new(|| Err::<DataDir, &str>("no home"), no_pane);
    assert!(state.data_dir.is_none());
    let connector = ScriptedConnector::new(macos_fixture_daemon());
    let test = test_app_from(
        test_parts(scratch_dir("failing-resolver"), state),
        app::context(),
        connector,
    );
    let view = test.poll();
    assert_eq!(view.service(), Service::HeldUnreachable);
    assert_eq!(
        call(&test, "capture_status", json!({})).map(|v| v["service"].clone()),
        Ok(json!("held_unreachable"))
    );
}

#[test]
fn the_app_state_of_this_systems_environment_builds_and_its_set_up_returns_ok() {
    let state = AppState::new(DataDir::for_current_user, no_pane);
    let connector = ScriptedConnector::new(macos_fixture_daemon());
    let test = test_app_from(
        test_parts(scratch_dir("environment"), state),
        app::context(),
        connector,
    );
    assert_eq!(
        call(&test, "capture_status", json!({})).map(|v| v["service"].clone()),
        Ok(json!("connecting"))
    );
}

fn tray_that_fails(
    _: &tauri::AppHandle<tauri::test::MockRuntime>,
) -> tauri::Result<tray::TrayInstalled> {
    Err(tauri::Error::WindowNotFound)
}

#[test]
fn a_tray_that_fails_to_install_is_no_set_up_error_and_the_state_records_no_tray() {
    let connector = ScriptedConnector::new(macos_fixture_daemon());
    let mut parts = test_parts(scratch_dir("no-tray"), super::state_of(&connector));
    parts.tray = Some(tray_that_fails);
    let test = test_app_from(parts, app::context(), connector);
    let state = test.app.state::<AppState>();
    assert!(!state.tray_installed.load(Ordering::SeqCst));
}

#[test]
fn a_stale_id_returns_already_deleted_and_deletes_nothing() {
    let test = app_on(macos_fixture_daemon(), "stale-id");
    test.poll();
    let stale = store_id(0x42);
    assert_eq!(
        call(&test, "delete_all", json!({ "storeId": hex(stale) })),
        Ok(json!({ "outcome": "already_deleted", "current_store_id": hex(FIXTURE_STORE_ID) }))
    );
    assert_eq!(
        test.daemon.status().fields().store_id,
        Some(FIXTURE_STORE_ID)
    );
    assert_eq!(
        test.daemon.requests().last(),
        Some(&Request::DeleteAll { store: Some(stale) })
    );
}

#[test]
fn an_app_started_on_a_sealed_store_sends_the_id_of_the_status_and_the_store_is_deleted() {
    let sealed = status_with(Platform::Macos, |f| {
        store(
            f,
            StoreState::Sealed,
            Some(StoreErrorKind::RootCorrupt),
            None,
        )
    });
    let test = app_on(ScriptedDaemon::new(sealed), "sealed");
    let view = test.poll();
    let shown = running_view(view)
        .store
        .id
        .expect("the sealed store shows its id");
    let reply = call(&test, "delete_all", json!({ "storeId": hex(shown) })).expect("a reply");
    assert_eq!(reply["outcome"], json!("deleted"));
    assert_eq!(
        test.daemon.requests().last(),
        Some(&Request::DeleteAll { store: Some(shown) })
    );
}

#[test]
fn an_unavailable_store_takes_the_delete_with_the_id_of_the_status() {
    let unavailable = status_with(Platform::Macos, |f| {
        store(
            f,
            StoreState::Unavailable,
            Some(StoreErrorKind::Io),
            Some(5),
        )
    });
    let test = app_on(ScriptedDaemon::new(unavailable), "unavailable");
    let view = test.poll();
    let store = running_view(view).store;
    assert_eq!(store.state, StoreState::Unavailable);
    assert_eq!(store.retry_in_s, Some(5));
    let id = store.id.expect("an id");
    let reply = call(&test, "delete_all", json!({ "storeId": hex(id) })).expect("a reply");
    assert_eq!(reply["outcome"], json!("deleted"));
    assert_eq!(test.deleting_requests(), 1);
}

#[test]
fn two_deletes_in_a_row_with_the_first_id_delete_one_store() {
    let test = app_on(macos_fixture_daemon(), "two-deletes");
    test.poll();
    let first = hex(FIXTURE_STORE_ID);
    let one = call(&test, "delete_all", json!({ "storeId": first })).expect("a reply");
    assert_eq!(one["outcome"], json!("deleted"));
    let fresh = test.daemon.status().fields().store_id;
    let two = call(&test, "delete_all", json!({ "storeId": first })).expect("a reply");
    assert_eq!(two["outcome"], json!("already_deleted"));
    assert_eq!(
        test.daemon.status().fields().store_id,
        fresh,
        "the second delete removed nothing"
    );
}

#[test]
fn delete_refused_carries_no_id_and_the_app_sends_no_second_delete() {
    let test = app_on(macos_fixture_daemon(), "refused");
    test.poll();
    assert_eq!(
        call(&test, "delete_all", json!({ "storeId": null })),
        Ok(json!({ "outcome": "delete_refused" }))
    );
    test.poll();
    assert_eq!(test.deleting_requests(), 1);
    assert_eq!(
        test.daemon.status().fields().store_id,
        Some(FIXTURE_STORE_ID)
    );
}

#[test]
fn a_store_id_that_is_no_id_is_an_error_and_sends_nothing() {
    let test = app_on(macos_fixture_daemon(), "bad-id");
    assert_eq!(
        call(&test, "delete_all", json!({ "storeId": "not-an-id" })),
        Err(json!({ "error": "bad_store_id" }))
    );
    assert_eq!(test.deleting_requests(), 0);
}

#[test]
fn a_deleted_report_reads_nothing_left_only_with_the_root_destroyed_and_nothing_not_followed() {
    use crate::view_state::left_nothing;
    use halvo_control::proto::{DeleteFields, IntentState, RootDestroyed};
    let base = DeleteFields {
        segments_shredded: Some(2),
        keys_destroyed: Some(3),
        root_destroyed: RootDestroyed::Yes,
        root_id: None,
        files_left: 0,
        database_left: false,
        not_followed: 0,
        entries_removed: 0,
        entries_left: 0,
        unknown_entries: 0,
        intent: IntentState::Written,
    };
    assert!(left_nothing(&base));
    assert!(!left_nothing(&DeleteFields {
        not_followed: 1,
        ..base
    }));
    assert!(!left_nothing(&DeleteFields {
        root_destroyed: RootDestroyed::No,
        root_id: Some(store_id(9)),
        ..base
    }));
    assert!(!left_nothing(&DeleteFields {
        root_destroyed: RootDestroyed::Unknown,
        ..base
    }));
}

#[test]
fn a_pause_whose_reply_does_not_reach_the_end_is_not_reached() {
    let pausing = status_with(Platform::Macos, |f| {
        f.desired = halvo_control::model::CaptureState::Paused;
        f.capture = Capture::Pausing;
    });
    let test = app_on(ScriptedDaemon::new(pausing), "pausing");
    let reply = call(&test, "set_capture_state", json!({ "capture": "paused" })).expect("a reply");
    assert_eq!(reply["reached"], json!(false));
    assert_eq!(reply["capture"], json!("pausing"));
    assert_eq!(reply["desired"], json!("paused"));
}

#[test]
fn a_pause_whose_state_file_is_not_written_is_not_reached() {
    for state_file in [
        StateFile::WriteFailed,
        StateFile::Corrupt,
        StateFile::NewerVersion,
    ] {
        let status = status_with(Platform::Macos, |f| f.state_file = state_file);
        let test = app_on(ScriptedDaemon::new(status), "state-file");
        let reply =
            call(&test, "set_capture_state", json!({ "capture": "paused" })).expect("a reply");
        assert_eq!(reply["capture"], json!("paused"), "{state_file:?}");
        assert_eq!(reply["reached"], json!(false), "{state_file:?}");
    }
    let test = app_on(macos_fixture_daemon(), "state-file-ok");
    let reply = call(&test, "set_capture_state", json!({ "capture": "paused" })).expect("a reply");
    assert_eq!(reply["reached"], json!(true));
}

#[test]
fn a_mutation_in_flight_answers_busy_as_an_action_error() {
    let mut daemon = macos_fixture_daemon();
    daemon.set_mutation_in_flight(true);
    let test = app_on(daemon, "busy");
    assert_eq!(
        call(&test, "set_capture_state", json!({ "capture": "paused" })),
        Err(json!({ "error": "busy" }))
    );
}

type Answer = fn() -> ClientError;

#[derive(Clone)]
struct Answering(Answer);

impl Connector for Answering {
    type Transport = NoTransport;

    fn endpoint(&self) -> String {
        "answering".to_string()
    }

    fn connect(&self, _: Duration) -> Result<NoTransport, ClientError> {
        Err((self.0)())
    }
}

fn app_answering(answer: Answer, name: &str) -> TestApp {
    let state = AppState::with_control(Arc::new(Gated::new(Answering(answer))), None, no_pane);
    let connector = ScriptedConnector::new(macos_fixture_daemon());
    test_app_from(
        test_parts(scratch_dir(name), state),
        app::context(),
        connector,
    )
}

#[test]
fn with_no_daemon_every_command_answers_ok_with_the_not_running_state_and_claims_nothing() {
    let test = app_answering(
        || ClientError::Unreachable {
            endpoint: "x".to_string(),
        },
        "not-running",
    );
    assert_eq!(test.poll().service(), Service::NotRunning);
    let status = call(&test, "capture_status", json!({})).expect("a view");
    assert_eq!(status["service"], json!("not_running"));
    assert_eq!(status["desired"], Value::Null);
    let set = call(&test, "set_capture_state", json!({ "capture": "paused" })).expect("a view");
    assert_eq!(set["service"], json!("not_running"));
    assert_eq!(set["reached"], json!(false));
    let inventory = call(&test, "inventory", json!({})).expect("an inventory");
    assert_eq!(inventory["available"], json!(false));
    assert_eq!(inventory["rows"], json!([]));
    assert_eq!(
        call(&test, "delete_all", json!({ "storeId": null })),
        Err(json!({ "error": "not_answering", "service": "not_running" }))
    );
    assert_eq!(
        call(&test, "onboarding_state", json!({})),
        Ok(json!({ "permissions": [], "lost_grants": [], "all_granted": false, "walkback": [] }))
    );
}

#[test]
fn a_held_directory_and_a_refused_status_are_states_of_their_own_and_never_not_running() {
    let cases: [(Answer, &str); 4] = [
        (
            || ClientError::DataDirInUse {
                endpoint: "x".to_string(),
            },
            "held_unreachable",
        ),
        (
            || {
                let broken = sample_status(Platform::Macos).into_fields();
                let mut f = broken;
                f.desired = halvo_control::model::CaptureState::Running;
                f.capture = Capture::Paused;
                ClientError::StateDoesNotHoldTogether(
                    DaemonStatus::new(f).expect_err("paused beside running is refused"),
                )
            },
            "status_refused",
        ),
        (|| ClientError::Unsupported, "unsupported"),
        (|| ClientError::Deadline { seconds: 2 }, "held_unreachable"),
    ];
    for (answer, service) in cases {
        let test = app_answering(answer, service);
        test.poll();
        let view = call(&test, "capture_status", json!({})).expect("a view");
        assert_eq!(view["service"], json!(service));
        assert_eq!(view["store"], Value::Null);
        assert_eq!(view["capture"], Value::Null);
    }
}

#[derive(Default)]
struct Fake {
    places: Arc<Places>,
    answers: Mutex<VecDeque<Result<DaemonStatus, ClientError>>>,
    claims: Mutex<Vec<bool>>,
}

impl Fake {
    fn with(answers: Vec<Result<DaemonStatus, ClientError>>) -> Self {
        Fake {
            answers: Mutex::new(answers.into()),
            ..Fake::default()
        }
    }
}

impl Control for Fake {
    fn places(&self) -> &Arc<Places> {
        &self.places
    }

    fn status(&self, _: Made, claim: bool) -> Result<DaemonStatus, ClientError> {
        self.claims.lock().unwrap().push(claim);
        self.answers
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Err(ClientError::Unsupported))
    }

    fn inventory(&self, _: Made) -> Result<InventoryReply, ClientError> {
        Err(ClientError::Unsupported)
    }

    fn pause(&self, _: Made) -> Result<DaemonStatus, ClientError> {
        Err(ClientError::Unsupported)
    }

    fn resume(&self, _: Made) -> Result<DaemonStatus, ClientError> {
        Err(ClientError::Unsupported)
    }

    fn delete_all(&self, _: Made, _: Option<StoreId>) -> Result<DeleteOutcome, ClientError> {
        Err(ClientError::Unsupported)
    }

    fn set_storage_cap(&self, _: Made, _: StorageCap) -> Result<CapSet, ClientError> {
        Err(ClientError::Unsupported)
    }

    fn request_permission(&self, _: Made, _: Permission) -> Result<RequestOutcome, ClientError> {
        Err(ClientError::Unsupported)
    }
}

#[test]
fn the_poller_runs_a_fake_client_through_unreachable_starting_and_running_and_the_tray_follows() {
    let starting = status_with(Platform::Macos, |f| f.capture = Capture::Starting);
    let fake = Fake::with(vec![
        Err(ClientError::Unreachable {
            endpoint: "x".to_string(),
        }),
        Ok(starting),
        Ok(sample_status(Platform::Macos)),
    ]);
    let views = Views::default();
    assert_eq!(views.view().service(), Service::Connecting);
    assert_eq!(tray::labels(&views.view()).status_text, "Connecting");
    let texts: Vec<&str> = (0..3)
        .map(|_| tray::labels(&poller::pass(&fake, &views, None)).status_text)
        .collect();
    assert_eq!(
        texts,
        ["Capture service not running", "Starting", "Capturing"]
    );
}

#[test]
fn the_poller_claims_the_indicator_only_with_the_proof_of_an_installed_tray() {
    let fake = Fake::default();
    let views = Views::default();
    poller::pass(&fake, &views, None);
    let installed = tray::installed_for_tests();
    poller::pass(&fake, &views, Some(&installed));
    assert_eq!(*fake.claims.lock().unwrap(), [false, true]);
}

#[test]
fn a_status_made_before_a_mutation_was_answered_does_not_replace_its_view() {
    let views = Views::default();
    let paused = status_with(Platform::Macos, |f| {
        f.desired = halvo_control::model::CaptureState::Paused;
        f.capture = Capture::Paused;
        stopped(f);
    });
    let running = Ok(sample_status(Platform::Macos));
    assert_eq!(views.take_poll(3, &running), PollAnswer::Stands);
    views.take_mutation(5, &paused);
    assert_eq!(
        views.take_poll(4, &running),
        PollAnswer::DroppedAsOlderThanTheLastMutation,
        "a poll made before the answer is dropped"
    );
    assert_eq!(running_view(views.view()).capture, Capture::Paused);
    assert_eq!(
        views.take_poll(5, &running),
        PollAnswer::Stands,
        "a poll made after the answer stands"
    );
    assert_eq!(running_view(views.view()).capture, Capture::Running);
}

#[test]
fn a_poll_of_the_app_made_before_a_pause_was_answered_keeps_the_pause() {
    let test = app_on(macos_fixture_daemon(), "order");
    let state = test.app.state::<AppState>();
    let early = state.control.places().make();
    let number = early.number();
    let answer = state.control.status(early, false);
    let reply = call(&test, "set_capture_state", json!({ "capture": "paused" })).expect("a reply");
    assert_eq!(reply["capture"], json!("paused"));
    assert_eq!(
        state.views.take_poll(number, &answer),
        PollAnswer::DroppedAsOlderThanTheLastMutation
    );
    assert_eq!(
        call(&test, "capture_status", json!({})).map(|v| v["capture"].clone()),
        Ok(json!("paused"))
    );
}

struct Counting(AtomicUsize);

impl Registration for Counting {
    fn names(&self) -> &'static Names {
        &CAPTURE
    }

    fn on_status(&self, status: &Status) {
        let _ = status.service();
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

struct Second;

impl Registration for Second {
    fn names(&self) -> &'static Names {
        &CAPTURE
    }
}

#[test]
fn on_status_is_made_once_per_pass_for_every_module_and_a_second_module_compiles_with_the_defaults()
{
    let counting = Counting(AtomicUsize::new(0));
    let second = Second;
    second.on_status(&StatusView::no_status(
        NoStatusService::NotRunning,
        Platform::Macos,
    ));
    let modules: [&dyn Registration; 3] = [&crate::commands::capture::Capture, &counting, &second];
    let fake = Fake::with(vec![
        Ok(sample_status(Platform::Macos)),
        Err(ClientError::DataDirInUse {
            endpoint: "x".to_string(),
        }),
    ]);
    let views = Views::default();
    let services: Vec<Service> = (0..3)
        .map(|_| poller::round(&fake, &views, None, &modules, &|_| {}).service())
        .collect();
    assert_eq!(
        services,
        [
            Service::Running,
            Service::HeldUnreachable,
            Service::Unsupported
        ]
    );
    assert_eq!(counting.0.load(Ordering::SeqCst), 3);
}

#[cfg(target_os = "macos")]
#[test]
fn the_commands_reach_the_fake_server_over_the_socket_of_a_data_directory() {
    use halvo_control::testing::ScriptedUnixServer;
    let base = std::env::current_exe()
        .expect("the test binary has a path")
        .ancestors()
        .nth(3)
        .expect("target")
        .join("tmp");
    std::fs::create_dir_all(&base).expect("the scratch base");
    let root = base.join(format!("h58-{}", std::process::id()));
    std::fs::create_dir_all(&root).expect("a scratch directory");
    let dir = DataDir::from_flag(root.clone()).expect("an absolute directory");
    let server =
        ScriptedUnixServer::start(&dir, macos_fixture_daemon()).expect("the server starts");
    let state = AppState::new(|| DataDir::from_flag(root), no_pane);
    let connector = ScriptedConnector::new(macos_fixture_daemon());
    let test = test_app_from(
        test_parts(scratch_dir("socket"), state),
        app::context(),
        connector,
    );
    assert_eq!(test.poll().service(), Service::Running);
    let reply = call(
        &test,
        "delete_all",
        json!({ "storeId": hex(FIXTURE_STORE_ID) }),
    )
    .expect("a reply");
    assert_eq!(reply["outcome"], json!("deleted"));
    assert!(server.requests().contains(&Request::DeleteAll {
        store: Some(FIXTURE_STORE_ID)
    }));
    drop(server);
}

#[test]
fn a_poll_made_after_a_mutation_was_answered_and_taken_first_is_not_replaced_by_the_mutation() {
    let views = Views::default();
    let paused = status_with(Platform::Macos, |f| {
        f.desired = halvo_control::model::CaptureState::Paused;
        f.capture = Capture::Paused;
        stopped(f);
    });
    assert_eq!(views.take_poll(7, &Ok(paused)), PollAnswer::Stands);
    let running = sample_status(Platform::Macos);
    views.take_mutation(5, &running);
    assert_eq!(
        running_view(views.view()).capture,
        Capture::Paused,
        "the poll of call 7 was made after the mutation's answer at mark 5"
    );
    assert_eq!(
        views.take_poll(4, &Ok(sample_status(Platform::Macos))),
        PollAnswer::DroppedAsOlderThanTheLastMutation
    );
}

#[test]
fn a_service_of_another_version_reads_update_the_app_and_never_not_running_or_paused() {
    let cases: [(Answer, &str); 3] = [
        (
            || ClientError::ReplyOfAnotherVersion { version: 2 },
            "reply of another version",
        ),
        (
            || {
                ClientError::ValueUnknownToV1(
                    "frozen"
                        .parse::<Capture>()
                        .expect_err("a value that v1 does not know"),
                )
            },
            "value unknown to v1",
        ),
        (
            || {
                ClientError::Daemon(halvo_control::proto::ErrorReply {
                    code: halvo_control::proto::ErrorCode::UnsupportedVersion,
                    message: "unsupported version".to_string(),
                })
            },
            "the daemon refused this version",
        ),
    ];
    for (answer, name) in cases {
        let test = app_answering(answer, "another-version");
        assert_eq!(test.poll().service(), Service::AnotherVersion, "{name}");
        let view = call(&test, "capture_status", json!({})).expect("a view");
        assert_eq!(view["service"], json!("another_version"), "{name}");
        for key in ["desired", "capture", "store", "texts"] {
            assert_eq!(view[key], Value::Null, "{name}: {key}");
        }
        assert_eq!(
            call(&test, "set_capture_state", json!({ "capture": "paused" })),
            Err(json!({ "error": "update_the_app" })),
            "{name}"
        );
        let labels = tray::labels(&test.poll());
        assert_eq!(labels.status_text, "Update Halvo", "{name}");
        assert!(!labels.tooltip.contains("not running") && !labels.tooltip.contains("paused"));
    }
}

#[test]
fn a_broken_reply_stays_a_broken_reply_and_not_an_update() {
    let test = app_answering(|| ClientError::Protocol("a frame".to_string()), "broken");
    assert_eq!(test.poll().service(), Service::StatusRefused);
    assert_eq!(
        call(&test, "set_capture_state", json!({ "capture": "paused" })),
        Err(json!({ "error": "broken_reply" }))
    );
}

#[test]
fn a_delete_with_no_id_while_the_store_opens_is_busy_and_deletes_nothing() {
    let opening = status_with(Platform::Macos, |f| {
        store(f, StoreState::Opening, None, None);
        f.capture = Capture::Starting;
        f.capture_reason = None;
        f.store_id = None;
    });
    let test = app_on(ScriptedDaemon::new(opening), "opening-busy");
    test.poll();
    assert_eq!(
        call(&test, "delete_all", json!({ "storeId": null })),
        Err(json!({ "error": "busy" }))
    );
    assert_eq!(test.daemon.status().fields().store, StoreState::Opening);
}

struct Recording(Arc<Mutex<Vec<&'static str>>>);

impl Registration for Recording {
    fn names(&self) -> &'static Names {
        &CAPTURE
    }

    fn on_status(&self, _status: &Status) {
        self.0.lock().expect("the order").push("module");
    }
}

#[test]
fn a_round_refreshes_the_tray_before_it_tells_the_modules() {
    let order = Arc::new(Mutex::new(Vec::new()));
    let module = Recording(Arc::clone(&order));
    let modules: [&dyn Registration; 1] = [&module];
    let fake = Fake::with(vec![Ok(sample_status(Platform::Macos))]);
    let views = Views::default();
    let refreshed = Arc::clone(&order);
    poller::round(&fake, &views, None, &modules, &|_| {
        refreshed.lock().expect("the order").push("refresh");
    });
    assert_eq!(*order.lock().expect("the order"), ["refresh", "module"]);
}

#[derive(Default)]
struct RecordingControl {
    places: Arc<Places>,
    calls: Mutex<Vec<&'static str>>,
}

impl RecordingControl {
    fn record(&self, call: &'static str) {
        self.calls.lock().expect("the calls").push(call);
    }
}

impl Control for RecordingControl {
    fn places(&self) -> &Arc<Places> {
        &self.places
    }

    fn status(&self, _: Made, _: bool) -> Result<DaemonStatus, ClientError> {
        self.record("status");
        Ok(sample_status(Platform::Macos))
    }

    fn inventory(&self, _: Made) -> Result<InventoryReply, ClientError> {
        self.record("inventory");
        Err(ClientError::Unsupported)
    }

    fn pause(&self, _: Made) -> Result<DaemonStatus, ClientError> {
        self.record("pause");
        Ok(sample_status(Platform::Macos))
    }

    fn resume(&self, _: Made) -> Result<DaemonStatus, ClientError> {
        self.record("resume");
        Ok(sample_status(Platform::Macos))
    }

    fn delete_all(&self, _: Made, _: Option<StoreId>) -> Result<DeleteOutcome, ClientError> {
        self.record("delete_all");
        Err(ClientError::Unsupported)
    }

    fn set_storage_cap(&self, _: Made, _: StorageCap) -> Result<CapSet, ClientError> {
        self.record("set_storage_cap");
        Err(ClientError::Unsupported)
    }

    fn request_permission(&self, _: Made, _: Permission) -> Result<RequestOutcome, ClientError> {
        self.record("request_permission");
        Err(ClientError::Unsupported)
    }
}

#[test]
fn every_read_command_of_every_module_makes_no_writing_call() {
    let control = Arc::new(RecordingControl::default());
    let state = AppState::with_control(control.clone(), None, no_pane);
    let test = test_app_from(
        test_parts(scratch_dir("reads-write-nothing"), state),
        app::context(),
        ScriptedConnector::new(macos_fixture_daemon()),
    );
    let args = json!({
        "capture": "paused",
        "storeId": "x",
        "capBytes": 1,
        "permission": "accessibility",
    });
    let mut asked = 0;
    for module in crate::commands::names::MODULES {
        for read in module.reads {
            let _ = call(&test, read, args.clone());
            asked += 1;
        }
    }
    assert_eq!(asked, 3);
    let calls = control.calls.lock().expect("the calls").clone();
    assert!(calls.contains(&"inventory"), "{calls:?}");
    for write in [
        "pause",
        "resume",
        "delete_all",
        "set_storage_cap",
        "request_permission",
    ] {
        assert!(
            !calls.contains(&write),
            "{write} was made by a read: {calls:?}"
        );
    }
}
