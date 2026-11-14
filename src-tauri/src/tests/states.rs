use halvo_control::model::{CaptureState, UtcSecond};
use halvo_control::onboarding::{GrantState, Permission};
use halvo_control::proto::{
    Cap, Capture, CaptureReason, Custody, DaemonStatus, DrainError, ExpiryError, Health, LastStop,
    Lease, LostRecord, Platform, Reason, SourceError, SourceLine, SourceName, SourceState,
    StatusFields, StopKind, StopSourceLine, StoreErrorKind, StoreState,
};
use halvo_control::testing::{onboarding_of, sample_status, FIXTURE_STORE_ID};
use serde_json::Value;

use super::{fixtures_dir, read_json};
use crate::view_state::{Service, StatusView};

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
    let none = |service: Service| StatusView {
        platform: mac,
        ..StatusView::without_status(service)
    };
    vec![
        ("connecting", none(Service::Connecting)),
        ("not-running", none(Service::NotRunning)),
        ("held-unreachable", none(Service::HeldUnreachable)),
        ("status-refused", none(Service::StatusRefused)),
        ("unsupported", none(Service::Unsupported)),
        ("another-version", none(Service::AnotherVersion)),
        ("running", of(sample_status(mac))),
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
        Service::AnotherVersion => None,
    };
    std::iter::successors(Some(Service::Connecting), |service| following(*service)).collect()
}

#[test]
fn the_states_show_every_service_every_capture_and_every_store_state() {
    let views: Vec<(&str, StatusView)> = states();
    let services: Vec<Service> = views.iter().map(|(_, view)| view.service).collect();
    let captures: Vec<Capture> = views.iter().filter_map(|(_, view)| view.capture).collect();
    let stores: Vec<StoreState> = views
        .iter()
        .filter_map(|(_, view)| view.store.as_ref().map(|store| store.state))
        .collect();
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
    assert_eq!(
        StatusView::without_status(Service::NotRunning).platform,
        expected
    );
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
                "last_exit",
                "lease",
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
        if view.service != Service::Running {
            for (key, value) in &map {
                if ["service", "platform", "window", "last_exit"].contains(&key.as_str()) {
                    continue;
                }
                assert_eq!(value, &Value::Null, "{name}: {key}");
            }
        }
    }
}

#[test]
fn no_reply_holds_the_key_of_the_old_store_description() {
    let key = ["store", "backend"].join("_");
    for (name, view) in states() {
        let text = serde_json::to_string(&view).expect("the view writes");
        assert!(!text.contains(&key), "{name}");
    }
    for entry in std::fs::read_dir(fixtures_dir()).expect("the fixtures are listed") {
        let path = entry.expect("an entry").path();
        let text = std::fs::read_to_string(&path).expect("a fixture reads");
        assert!(!text.contains(&key), "{}", path.display());
    }
}

#[test]
fn the_store_id_and_the_custody_sentence_come_from_the_status() {
    let running = StatusView::of(&sample_status(Platform::Macos));
    assert_eq!(
        running.store.as_ref().and_then(|s| s.id),
        Some(FIXTURE_STORE_ID)
    );
    let custody = |view: &StatusView| view.texts.as_ref().map(|t| t.custody);
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
