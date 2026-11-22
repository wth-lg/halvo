use halvo_control::model::{CaptureState, UtcSecond};
use halvo_control::onboarding::{GrantState, Permission};
use halvo_control::proto::{
    Cap, Capture, CaptureReason, Custody, DaemonStatus, DrainError, ExpiryError, Health, LastStop,
    Lease, LostRecord, Platform, Reason, SourceError, SourceLine, SourceName, SourceState,
    StatusFields, StopKind, StopSourceLine, StoreErrorKind, StoreState, SwitchName,
};
use halvo_control::testing::{onboarding_of, sample_status, FIXTURE_STORE_ID};
use serde_json::Value;

use super::{fixtures_dir, read_json};
use crate::view_state::{NoStatusService, RunningView, Service, StatusView};

fn at(text: &str) -> UtcSecond {
    text.parse().expect("a fixed instant")
}

fn error(kind: &str, stage: &str, domain: &str, code: i64) -> SourceError {
    SourceError {
        kind: kind.parse().expect("a token"),
        stage: stage.parse().expect("a token"),
        os_domain: domain.parse().expect("a token"),
        os_code: code,
    }
}

pub fn status_with(platform: Platform, edit: impl FnOnce(&mut StatusFields)) -> DaemonStatus {
    let mut f = sample_status(platform).into_fields();
    if platform == Platform::Macos {
        f.typing = Some(true);
    }
    edit(&mut f);
    f.onboarding = onboarding_of(f.platform, &f.permissions);
    DaemonStatus::new(f).unwrap_or_else(|e| panic!("the edited status breaks: {}", e.rule()))
}

fn source(f: &mut StatusFields, name: SourceName, state: SourceState) {
    let line = f
        .sources
        .iter_mut()
        .find(|line| line.name == name)
        .expect("the source has a line");
    *line = SourceLine {
        name,
        state,
        unstopped: false,
    };
}

pub fn stopped(f: &mut StatusFields) {
    for name in [SourceName::Foreground, SourceName::Input] {
        source(f, name, SourceState::Stopped { by_user: false });
    }
}

pub fn store(
    f: &mut StatusFields,
    state: StoreState,
    error: Option<StoreErrorKind>,
    retry: Option<u64>,
) {
    f.store = state;
    f.store_error = error;
    f.store_retry_in_s = retry;
    if state != StoreState::Open {
        f.capture = Capture::Faulted;
        f.capture_reason = Some(CaptureReason::Store);
        stopped(f);
        f.store_bytes = None;
        f.store_logical_bytes = None;
    }
}

fn timeout_record(f: &mut StatusFields) {
    f.last_stop = LastStop::SourceNotStopped;
    f.last_stop_newest_capture = Some(at("2026-11-12T08:59:58Z"));
    f.last_stop_kind = Some(StopKind::Pause);
    f.last_stop_lost = LostRecord {
        events: 3,
        first: Some(at("2026-11-12T08:59:50Z")),
        last: Some(at("2026-11-12T08:59:58Z")),
        error: None,
        final_figures: false,
    };
    f.last_stop_sources = vec![StopSourceLine {
        name: SourceName::Input,
        error: error("timeout", "shutdown", "halvo", 0),
    }];
}

pub fn states() -> Vec<(&'static str, StatusView)> {
    let mac = Platform::Macos;
    let of = |status: DaemonStatus| StatusView::of(&status);
    let none = |service: NoStatusService| StatusView::no_status(service, mac);
    vec![
        ("connecting", none(NoStatusService::Connecting)),
        ("not-running", none(NoStatusService::NotRunning)),
        ("held-unreachable", none(NoStatusService::HeldUnreachable)),
        ("status-refused", none(NoStatusService::StatusRefused)),
        ("unsupported", none(NoStatusService::Unsupported)),
        ("another-version", none(NoStatusService::AnotherVersion)),
        ("not-started", {
            let hold = crate::view_state::Hold::default();
            hold.awaits_consent();
            hold.mark(none(NoStatusService::NotRunning))
        }),
        ("data-dir-unusable", {
            let hold = crate::view_state::Hold::default();
            hold.data_dir_unusable();
            hold.mark(none(NoStatusService::HeldUnreachable))
        }),
        ("running", of(status_with(mac, |_| {}))),
        ("running-windows", of(sample_status(Platform::Windows))),
        (
            "starting",
            of(status_with(mac, |f| f.capture = Capture::Starting)),
        ),
        (
            "partial",
            of(status_with(mac, |f| {
                f.capture = Capture::Partial;
                source(
                    f,
                    SourceName::Input,
                    SourceState::Failed {
                        reason: Reason::OsError,
                        error: Some(error("os_error", "start", "osstatus", -3801)),
                    },
                );
            })),
        ),
        (
            "pausing",
            of(status_with(mac, |f| {
                f.desired = CaptureState::Paused;
                f.capture = Capture::Pausing;
                source(
                    f,
                    SourceName::Input,
                    SourceState::Failed {
                        reason: Reason::Timeout,
                        error: None,
                    },
                );
            })),
        ),
        (
            "paused-sources-stopped",
            of(status_with(mac, |f| {
                f.desired = CaptureState::Paused;
                f.capture = Capture::Paused;
                stopped(f);
            })),
        ),
        (
            "no-indicator",
            of(status_with(mac, |f| {
                f.lease = Lease::Lapsed;
                f.capture = Capture::NoIndicator;
                stopped(f);
            })),
        ),
        (
            "awaiting-resume",
            of(status_with(mac, |f| {
                f.capture = Capture::AwaitingResume;
                stopped(f);
                timeout_record(f);
            })),
        ),
        (
            "faulted",
            of(status_with(mac, |f| {
                f.capture = Capture::Faulted;
                f.capture_reason = Some(CaptureReason::Permission);
                for name in [SourceName::Foreground, SourceName::Input] {
                    source(
                        f,
                        name,
                        SourceState::Failed {
                            reason: Reason::Permission,
                            error: None,
                        },
                    );
                }
            })),
        ),
        (
            "failed-source",
            of(status_with(mac, |f| {
                f.capture = Capture::Partial;
                source(
                    f,
                    SourceName::Input,
                    SourceState::Failed {
                        reason: Reason::Permission,
                        error: None,
                    },
                );
            })),
        ),
        (
            "drain-failing",
            of(status_with(mac, |f| {
                f.drain = Health::Failing;
                f.drain_error = Some(DrainError::NoSpace);
                f.drain_since = Some(at("2026-11-12T08:00:00Z"));
            })),
        ),
        (
            "expiry-failing",
            of(status_with(mac, |f| {
                f.expiry = Health::Failing;
                f.expiry_error = Some(ExpiryError::Io);
            })),
        ),
        (
            "lost-grant-source-running",
            of(status_with(mac, |f| {
                let line = f
                    .permissions
                    .iter_mut()
                    .find(|p| p.name == Permission::InputMonitoring)
                    .expect("the permission has a line");
                line.grant = GrantState::NotGranted;
                line.at_start = GrantState::Granted;
                line.once_granted = true;
            })),
        ),
        (
            "lost-grant-source-stopped",
            of(status_with(mac, |f| {
                let line = f
                    .permissions
                    .iter_mut()
                    .find(|p| p.name == Permission::InputMonitoring)
                    .expect("the permission has a line");
                line.grant = GrantState::NotGranted;
                line.at_start = GrantState::Granted;
                line.once_granted = true;
                f.capture = Capture::Partial;
                source(
                    f,
                    SourceName::Input,
                    SourceState::Failed {
                        reason: Reason::Permission,
                        error: None,
                    },
                );
            })),
        ),
        (
            "cap-unknown",
            of(status_with(mac, |f| f.cap_bytes = Cap::Unknown)),
        ),
        (
            "cap-not-set",
            of(status_with(mac, |f| {
                f.cap_bytes = Cap::Unknown;
                f.cap_not_set = true;
            })),
        ),
        (
            "custody-enclave",
            of(status_with(mac, |f| f.custody = Custody::SecureEnclave)),
        ),
        (
            "store-opening",
            of(status_with(mac, |f| {
                store(f, StoreState::Opening, None, None);
                f.capture = Capture::Starting;
                f.capture_reason = None;
                f.store_id = None;
            })),
        ),
        (
            "store-deleting",
            of(status_with(mac, |f| {
                store(f, StoreState::Deleting, None, None)
            })),
        ),
        (
            "store-deleting-unfinished",
            of(status_with(mac, |f| {
                store(f, StoreState::Deleting, None, None);
                f.store_id = None;
                f.store_delete_unfinished = true;
            })),
        ),
        (
            "store-sealed",
            of(status_with(mac, |f| {
                store(
                    f,
                    StoreState::Sealed,
                    Some(StoreErrorKind::RootCorrupt),
                    None,
                )
            })),
        ),
        (
            "store-sealed-custody-kind",
            of(status_with(mac, |f| {
                store(
                    f,
                    StoreState::Sealed,
                    Some(StoreErrorKind::CustodyKind),
                    None,
                )
            })),
        ),
        (
            "store-unavailable-io",
            of(status_with(mac, |f| {
                store(
                    f,
                    StoreState::Unavailable,
                    Some(StoreErrorKind::Io),
                    Some(5),
                )
            })),
        ),
        (
            "store-unavailable-locked",
            of(status_with(mac, |f| {
                store(
                    f,
                    StoreState::Unavailable,
                    Some(StoreErrorKind::Locked),
                    Some(5),
                )
            })),
        ),
        (
            "store-unavailable-root-denied",
            of(status_with(mac, |f| {
                store(
                    f,
                    StoreState::Unavailable,
                    Some(StoreErrorKind::RootDenied),
                    None,
                )
            })),
        ),
        (
            "store-unavailable-obstructed",
            of(status_with(mac, |f| {
                store(
                    f,
                    StoreState::Unavailable,
                    Some(StoreErrorKind::Obstructed),
                    Some(60),
                )
            })),
        ),
        (
            "store-unavailable-no-id",
            of(status_with(mac, |f| {
                store(
                    f,
                    StoreState::Unavailable,
                    Some(StoreErrorKind::Io),
                    Some(5),
                );
                f.store_id = None;
            })),
        ),
        (
            "store-no-release-custody",
            of(status_with(mac, |f| {
                store(f, StoreState::NoReleaseCustody, None, None)
            })),
        ),
        (
            "store-older",
            of(status_with(mac, |f| {
                store(
                    f,
                    StoreState::UnsupportedVersion,
                    Some(StoreErrorKind::Older),
                    None,
                )
            })),
        ),
        (
            "store-newer",
            of(status_with(mac, |f| {
                store(
                    f,
                    StoreState::UnsupportedVersion,
                    Some(StoreErrorKind::Newer),
                    None,
                )
            })),
        ),
    ]
}

fn fixture_name(name: &str) -> String {
    format!("capture_status-{name}.json")
}

#[test]
fn each_state_of_the_view_equals_its_fixture() {
    for (name, view) in states() {
        let got = serde_json::to_value(&view).expect("the view writes");
        let expected = read_json(&fixtures_dir().join(fixture_name(name)));
        assert_eq!(got, expected, "the view {name} differs from its fixture");
    }
}

fn every_service() -> Vec<Service> {
    let following = |service: Service| match service {
        Service::Connecting => Some(Service::Running),
        Service::Running => Some(Service::NotRunning),
        Service::NotRunning => Some(Service::HeldUnreachable),
        Service::HeldUnreachable => Some(Service::StatusRefused),
        Service::StatusRefused => Some(Service::Unsupported),
        Service::Unsupported => Some(Service::AnotherVersion),
        Service::AnotherVersion => Some(Service::NotStarted),
        Service::NotStarted => Some(Service::DataDirUnusable),
        Service::DataDirUnusable => None,
    };
    std::iter::successors(Some(Service::Connecting), |service| following(*service)).collect()
}

#[test]
fn the_states_show_every_service_every_capture_and_every_store_state() {
    let views: Vec<(&str, StatusView)> = states();
    let services: Vec<Service> = views.iter().map(|(_, view)| view.service()).collect();
    let running: Vec<&RunningView> = views
        .iter()
        .filter_map(|(_, view)| match view {
            StatusView::Running(view) => Some(view.as_ref()),
            StatusView::NoStatus(_) => None,
        })
        .collect();
    let captures: Vec<Capture> = running.iter().map(|view| view.capture).collect();
    let stores: Vec<StoreState> = running.iter().map(|view| view.store.state).collect();
    let unshown: Vec<String> = every_service()
        .into_iter()
        .filter(|service| !services.contains(service))
        .map(|service| format!("service {service:?}"))
        .chain(
            Capture::ALL
                .iter()
                .filter(|capture| !captures.contains(capture))
                .map(|capture| format!("capture {}", capture.as_str())),
        )
        .chain(
            StoreState::ALL
                .iter()
                .filter(|state| !stores.contains(state))
                .map(|state| format!("store {}", state.as_str())),
        )
        .collect();
    assert_eq!(unshown, Vec::<String>::new(), "no state shows these");
}

#[test]
fn a_view_with_no_status_names_the_system_that_the_app_runs_on() {
    let expected = if cfg!(windows) {
        Platform::Windows
    } else {
        Platform::Macos
    };
    let StatusView::NoStatus(view) = StatusView::without_status(NoStatusService::NotRunning) else {
        panic!("a view without status")
    };
    assert_eq!(view.platform, expected);
}

#[test]
fn with_any_service_but_running_every_daemon_field_is_null() {
    for (name, view) in states() {
        let value = serde_json::to_value(&view).expect("the view writes");
        let Value::Object(map) = value else {
            panic!("a view is an object")
        };
        let keys: Vec<&str> = map.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            [
                "capture",
                "capture_reason",
                "desired",
                "drain",
                "expiry",
                "ext",
                "hold",
                "last_exit",
                "lease",
                "login_item_loaded",
                "loss",
                "lost_grants",
                "password_guard",
                "platform",
                "screen_filter_excludes_app",
                "service",
                "sources",
                "store",
                "store_bytes",
                "store_cap_bytes",
                "store_logical_bytes",
                "texts",
                "window"
            ],
            "{name}"
        );
        if view.service() != Service::Running {
            for (key, value) in &map {
                if [
                    "service",
                    "platform",
                    "window",
                    "last_exit",
                    "hold",
                    "login_item_loaded",
                ]
                .contains(&key.as_str())
                    || (key == "texts" && view.service() == Service::NotStarted)
                {
                    continue;
                }
                assert_eq!(value, &Value::Null, "{name}: {key}");
            }
        }
    }
}

#[test]
fn no_reply_holds_the_key_of_the_old_store_description() {
    let key = "store_backend";
    for (name, view) in states() {
        let text = serde_json::to_string(&view).expect("the view writes");
        assert!(!text.contains(key), "{name}");
    }
    for entry in std::fs::read_dir(fixtures_dir()).expect("the fixtures are listed") {
        let path = entry.expect("an entry").path();
        let text = std::fs::read_to_string(&path).expect("a fixture reads");
        assert!(!text.contains(key), "{}", path.display());
    }
}

#[test]
fn the_store_id_and_the_custody_sentence_come_from_the_status() {
    let running = StatusView::of(&sample_status(Platform::Macos));
    let custody = |view: &StatusView| match view {
        StatusView::Running(view) => Some(view.texts.custody),
        StatusView::NoStatus(_) => None,
    };
    let StatusView::Running(shown) = &running else {
        panic!("the sample status makes a running view")
    };
    assert_eq!(shown.store.id, Some(FIXTURE_STORE_ID));
    assert_eq!(
        custody(&running),
        Some("This is an unsigned build: the key that protects your data is kept in a file with owner-only permissions in your account.")
    );
    let enclave = StatusView::of(&status_with(Platform::Macos, |f| {
        f.custody = Custody::SecureEnclave
    }));
    assert!(!custody(&enclave).unwrap_or("").contains("unsigned"));
}

#[test]
fn a_mac_status_with_typing_off_gets_none_of_the_four() {
    let key_row = halvo_control::texts::recorded_when_on(SwitchName::KeyClass);
    let early = halvo_control::texts::KEY_TYPED_AHEAD_CAN_BE_NOTED;
    for (platform, typing, shown) in [
        (Platform::Macos, Some(false), false),
        (Platform::Macos, None, false),
        (Platform::Macos, Some(true), true),
        (Platform::Windows, None, true),
    ] {
        let case = format!("{platform:?} typing {typing:?}");
        let status = status_with(platform, |f| {
            f.typing = typing;
            if let Some(line) = f
                .permissions
                .iter_mut()
                .find(|line| line.name == Permission::InputMonitoring)
            {
                line.grant = GrantState::NotGranted;
                line.once_granted = true;
            }
        });
        let before = halvo_control::texts::texts(&status);
        let test = super::test_app_with(
            super::scratch_dir(&format!("typing-{}", case.replace(' ', "-"))),
            halvo_control::testing::ScriptedDaemon::new(status.clone()),
        );
        test.poll();
        let window = super::main_window(&test.app);
        let view = super::invoke(
            &window,
            super::APP_URL,
            "capture_status",
            serde_json::json!({}),
        )
        .unwrap_or_else(|e| panic!("{case}: capture_status answers {e}"));
        let onboarding = super::invoke(
            &window,
            super::APP_URL,
            "onboarding_state",
            serde_json::json!({}),
        )
        .unwrap_or_else(|e| panic!("{case}: onboarding_state answers {e}"));
        let recorded: Vec<&str> = view["texts"]["recorded"]
            .as_array()
            .unwrap_or_else(|| panic!("{case}: the view lists what is recorded"))
            .iter()
            .filter_map(|row| row["text"].as_str())
            .collect();
        let others = [&view["texts"]["passwords"], &view["texts"]["typed_ahead"]]
            .into_iter()
            .filter_map(Value::as_str)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>();
        let rows: Vec<&str> = onboarding["permissions"]
            .as_array()
            .unwrap_or_else(|| panic!("{case}: onboarding_state lists rows"))
            .iter()
            .filter_map(|row| row["permission"].as_str())
            .collect();
        let steps: Vec<&str> = onboarding["walkback"]
            .as_array()
            .unwrap_or_else(|| panic!("{case}: onboarding_state lists steps"))
            .iter()
            .filter_map(|step| step["permission"].as_str())
            .collect();
        let named = |list: &Value| {
            list.as_array()
                .is_some_and(|names| names.iter().any(|n| n == "input_monitoring"))
        };
        let sources: Vec<&str> = view["sources"]
            .as_array()
            .unwrap_or_else(|| panic!("{case}: the view lists sources"))
            .iter()
            .filter_map(|line| line["name"].as_str())
            .collect();
        let mac = platform == Platform::Macos;
        assert_eq!(
            recorded.contains(&key_row),
            shown && mac,
            "{case}: the row of keys"
        );
        assert_eq!(
            sources.contains(&"input"),
            shown && mac,
            "{case}: the input source's name for typing"
        );
        assert_eq!(
            sources.contains(&"pointer"),
            !shown || !mac,
            "{case}: the name for the pointer alone"
        );
        assert_eq!(
            rows.contains(&"input_monitoring"),
            shown && mac,
            "{case}: the card"
        );
        assert_eq!(
            steps.contains(&"input_monitoring"),
            shown && mac,
            "{case}: its steps"
        );
        assert_eq!(
            named(&onboarding["lost_grants"]),
            shown && mac,
            "{case}: its lost grant"
        );
        assert_eq!(
            named(&view["lost_grants"]),
            shown && mac,
            "{case}: the banner's name"
        );
        assert_eq!(
            others.contains(&early),
            shown && mac,
            "{case}: a key pressed early"
        );
        assert_eq!(
            others.contains(&before.passwords),
            shown,
            "{case}: the sentence on passwords"
        );
        assert!(
            !rows.contains(&"screen_and_system_audio_recording"),
            "{case}: the screen's card while screen_activity is off"
        );
        if shown {
            assert_eq!(
                view["texts"],
                serde_json::to_value(&before).expect("the texts write"),
                "{case}: the texts as the status gives them"
            );
        }
        if !mac {
            assert_eq!(
                onboarding,
                serde_json::to_value(&status.fields().onboarding).expect("the rows write"),
                "{case}: a Windows status is untouched"
            );
        }
    }
}

#[test]
fn a_mac_view_names_no_row_of_keys_or_pointer_actions_while_the_input_source_fails_or_waits_for_its_grant(
) {
    let keys = halvo_control::texts::recorded_when_on(SwitchName::KeyClass);
    let pointer = halvo_control::texts::recorded_when_on(SwitchName::PointerClass);
    let others = [
        halvo_control::texts::APP_IN_FRONT,
        halvo_control::texts::recorded_when_on(SwitchName::TitleText),
        halvo_control::texts::SCREEN_ACTIVITY_NOT_RECORDED,
    ];
    for (case, capture, input, grant, shown) in [
        (
            "the grant refused",
            Capture::Partial,
            SourceState::Failed {
                reason: Reason::Permission,
                error: None,
            },
            GrantState::NotGranted,
            false,
        ),
        (
            "the grant not yet given",
            Capture::Starting,
            SourceState::Off,
            GrantState::NotDetermined,
            false,
        ),
        (
            "a tap thread that ended",
            Capture::Partial,
            SourceState::Failed {
                reason: Reason::ThreadEnded,
                error: None,
            },
            GrantState::Granted,
            false,
        ),
        (
            "the grant given",
            Capture::Running,
            SourceState::Running,
            GrantState::Granted,
            true,
        ),
    ] {
        let status = status_with(Platform::Macos, |f| {
            f.capture = capture;
            if matches!(input, SourceState::Off) {
                f.enabled_sources.retain(|&name| name != SourceName::Input);
            }
            source(f, SourceName::Input, input);
            if let Some(line) = f
                .permissions
                .iter_mut()
                .find(|line| line.name == Permission::InputMonitoring)
            {
                line.grant = grant;
            }
        });
        let StatusView::Running(view) = StatusView::of(&status) else {
            panic!("{case}: a status makes a running view")
        };
        let rows: Vec<&str> = view.texts.recorded.iter().map(|row| row.text).collect();
        assert_eq!(rows.contains(&keys), shown, "{case}: the row of keys");
        assert_eq!(
            rows.contains(&pointer),
            shown,
            "{case}: the row of pointer actions"
        );
        for row in others {
            assert!(rows.contains(&row), "{case}: {row}");
        }
        assert_eq!(
            !view.texts.passwords.is_empty(),
            shown,
            "{case}: the sentence on passwords"
        );
        assert_eq!(
            view.texts.typed_ahead.is_some(),
            shown,
            "{case}: the sentence on a key pressed early"
        );
    }
}

#[test]
fn an_acknowledged_lost_grant_reads_lost_no_more_on_the_row_and_the_tray() {
    use tauri::Manager;
    let grant = |once_granted: bool| {
        status_with(Platform::Macos, |f| {
            if let Some(line) = f
                .permissions
                .iter_mut()
                .find(|line| line.name == Permission::Accessibility)
            {
                line.grant = GrantState::NotGranted;
                line.once_granted = once_granted;
            }
        })
    };
    let lost = grant(true);
    let put_away = crate::tray::labels(&StatusView::of(&grant(false)));
    let test = super::test_app_with(
        super::scratch_dir("acknowledge"),
        halvo_control::testing::ScriptedDaemon::new(lost),
    );
    let before = crate::tray::labels(&test.poll());
    assert_ne!(
        before.status_text, put_away.status_text,
        "the tray says that a grant was lost"
    );
    let window = super::main_window(&test.app);
    let answer = super::invoke(
        &window,
        super::APP_URL,
        "acknowledge_lost_permission",
        serde_json::json!({ "permission": "accessibility" }),
    )
    .unwrap_or_else(|e| panic!("acknowledge_lost_permission answers {e}"));
    let acknowledged: Vec<halvo_control::proto::Request> = test
        .daemon
        .requests()
        .into_iter()
        .filter(|r| {
            matches!(
                r,
                halvo_control::proto::Request::AcknowledgeLostPermission { .. }
            )
        })
        .collect();
    assert_eq!(
        acknowledged,
        [halvo_control::proto::Request::AcknowledgeLostPermission {
            permission: Permission::Accessibility
        }],
        "one request of that name"
    );
    let row = answer["permissions"]
        .as_array()
        .and_then(|rows| rows.iter().find(|row| row["permission"] == "accessibility"))
        .unwrap_or_else(|| panic!("the answer holds the row: {answer}"));
    assert_eq!(
        row["lost"],
        Value::Bool(false),
        "the row reads lost no more"
    );
    assert_eq!(row["grant"], "not_granted");
    assert_eq!(
        answer["lost_grants"],
        serde_json::json!([]),
        "the banner goes"
    );
    let state = test.app.state::<crate::commands::capture::AppState>();
    let after = crate::tray::labels(&state.views.view());
    assert_eq!(after, put_away, "the tray's line goes with the answer");
}

#[derive(Default)]
struct Claims {
    places: std::sync::Arc<crate::commands::control::Places>,
    seen: std::sync::Mutex<Vec<bool>>,
}

impl Claims {
    fn taken(&self) -> Vec<bool> {
        std::mem::take(&mut *self.seen.lock().expect("the claims"))
    }
}

impl crate::commands::control::Control for Claims {
    fn places(&self) -> &std::sync::Arc<crate::commands::control::Places> {
        &self.places
    }

    fn status(
        &self,
        _: crate::commands::control::Made,
        claim: bool,
    ) -> Result<DaemonStatus, halvo_control::client::ClientError> {
        self.seen.lock().expect("the claims").push(claim);
        Ok(status_with(Platform::Macos, |_| {}))
    }

    fn status_of_a_pass(
        &self,
        made: crate::commands::control::Made,
        claim: bool,
        hold: &crate::view_state::Hold,
    ) -> Result<DaemonStatus, halvo_control::client::ClientError> {
        self.status(made, claim && !hold.stands())
    }

    fn inventory(
        &self,
        _: crate::commands::control::Made,
    ) -> Result<halvo_control::proto::InventoryReply, halvo_control::client::ClientError> {
        Err(halvo_control::client::ClientError::Unsupported)
    }

    fn pause(
        &self,
        _: crate::commands::control::Made,
    ) -> Result<DaemonStatus, halvo_control::client::ClientError> {
        Err(halvo_control::client::ClientError::Unsupported)
    }

    fn resume(
        &self,
        _: crate::commands::control::Made,
    ) -> Result<DaemonStatus, halvo_control::client::ClientError> {
        Err(halvo_control::client::ClientError::Unsupported)
    }

    fn delete_all(
        &self,
        _: crate::commands::control::Made,
        _: Option<halvo_control::model::StoreId>,
    ) -> Result<halvo_control::client::DeleteOutcome, halvo_control::client::ClientError> {
        Err(halvo_control::client::ClientError::Unsupported)
    }

    fn set_storage_cap(
        &self,
        _: crate::commands::control::Made,
        _: halvo_control::model::StorageCap,
    ) -> Result<halvo_control::proto::CapSet, halvo_control::client::ClientError> {
        Err(halvo_control::client::ClientError::Unsupported)
    }

    fn request_permission(
        &self,
        _: crate::commands::control::Made,
        _: Permission,
    ) -> Result<halvo_control::onboarding::RequestOutcome, halvo_control::client::ClientError> {
        Err(halvo_control::client::ClientError::Unsupported)
    }

    fn relay(
        &self,
        _: crate::commands::control::Made,
        _: &[u8],
    ) -> Result<Vec<u8>, halvo_control::client::ClientError> {
        Err(halvo_control::client::ClientError::Unsupported)
    }

    fn acknowledge_lost_permission(
        &self,
        _: crate::commands::control::Made,
        _: Permission,
    ) -> Result<DaemonStatus, halvo_control::client::ClientError> {
        Err(halvo_control::client::ClientError::Unsupported)
    }

    fn reset_settings(
        &self,
        _: crate::commands::control::Made,
    ) -> Result<DaemonStatus, halvo_control::client::ClientError> {
        Err(halvo_control::client::ClientError::Unsupported)
    }

    #[cfg(any(target_os = "macos", windows))]
    fn owner_stream(
        &self,
        _: crate::commands::control::Made,
    ) -> Result<halvo_control::r2::OwnerStreamEnd, halvo_control::client::ClientError> {
        Err(halvo_control::client::ClientError::Unsupported)
    }

    #[cfg(any(target_os = "macos", windows))]
    fn run_mark(
        &self,
        _: crate::commands::control::Made,
        _: halvo_control::r2::RunEdge,
    ) -> Result<halvo_control::r2::RunMarked, halvo_control::client::ClientError> {
        Err(halvo_control::client::ClientError::Unsupported)
    }

    #[cfg(any(target_os = "macos", windows))]
    fn defer(
        &self,
        _: crate::commands::control::Made,
        _: i64,
    ) -> Result<Option<i64>, halvo_control::client::ClientError> {
        Err(halvo_control::client::ClientError::Unsupported)
    }
}

#[test]
fn the_open_view_asks_a_status_without_the_claim_and_the_poller_still_claims() {
    use tauri::Manager;
    let control = std::sync::Arc::new(Claims::default());
    let state =
        crate::commands::capture::AppState::with_control(control.clone(), None, super::no_pane);
    let test = super::test_app_from(
        super::test_parts(super::scratch_dir("open-view"), state),
        crate::app::context(),
        halvo_control::testing::ScriptedConnector::new(super::macos_fixture_daemon()),
    );
    let window = super::main_window(&test.app);
    let state = test.app.state::<crate::commands::capture::AppState>();
    let tray = crate::tray::installed_for_tests();
    let pass = || crate::poller::pass(control.as_ref(), &state.views, Some(&tray));
    let ask = |command: &str| {
        super::invoke(&window, super::APP_URL, command, serde_json::json!({}))
            .unwrap_or_else(|e| panic!("{command} answers {e}"))
    };
    control.taken();
    for _ in 0..3 {
        pass();
        ask("capture_status");
    }
    assert_eq!(
        control.taken(),
        [true; 3],
        "with the view closed every Status claims"
    );
    for _ in 0..3 {
        pass();
        let rows = ask("onboarding_state");
        assert!(rows["permissions"].is_array(), "{rows}");
    }
    assert_eq!(
        control.taken(),
        [true, false, true, false, true, false],
        "the open view's Status carries no claim, and each pass beside it claims"
    );
    pass();
    assert_eq!(
        control.taken(),
        [true],
        "closed again, none goes out without"
    );
}

#[test]
#[ignore = "prints the view of each state for a review of the fixtures"]
fn print_the_views_of_the_states() {
    for (name, view) in states() {
        println!(
            "== {}\n{}",
            fixture_name(name),
            serde_json::to_string_pretty(&view).expect("JSON")
        );
    }
}
