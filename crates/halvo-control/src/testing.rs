use crate::model::{CaptureState, Modality, StorageCap, StoreId, UtcSecond};
use crate::onboarding::{assess, AssessInput, GrantState, Grants, Permission, PermissionSet};
use crate::proto::{
    Cap, Capture, Custody, DaemonStatus, DaemonVersion, Health, KindLine, LastStop, Lease, OnOff,
    Parent, PasswordGuard, PermissionLine, Platform, SourceLine, SourceName, SourceState,
    StateFile, StatusFields, StoreState, SwitchLine, SwitchName,
};

/// The store id of the status fixtures of gate-2.
pub const FIXTURE_STORE_ID: StoreId = StoreId::from_bytes([
    0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef,
]);

/// A store id whose last byte is `n` and whose other bytes are zero.
pub const fn store_id(n: u8) -> StoreId {
    let mut bytes = [0u8; 16];
    bytes[15] = n;
    StoreId::from_bytes(bytes)
}

fn at(text: &str) -> UtcSecond {
    text.parse().expect("a fixed instant")
}

/// The onboarding object that a daemon reports for these permission facts.
pub fn onboarding_of(
    platform: Platform,
    lines: &[PermissionLine],
) -> crate::onboarding::OnboardingState {
    let set = |pick: fn(&PermissionLine) -> bool| -> PermissionSet {
        lines.iter().filter(|l| pick(l)).map(|l| l.name).collect()
    };
    let now = lines.iter().fold(Grants::default(), |grants, line| {
        grants.with(line.name, line.grant)
    });
    let required: &[Permission] = match platform {
        Platform::Macos => &Permission::R1_MACOS,
        Platform::Windows => &[],
    };
    assess(&AssessInput {
        required,
        now,
        granted_at_capture_start: set(|l| l.at_start == GrantState::Granted),
        asked: set(|l| l.asked),
        once_granted: set(|l| l.once_granted),
        macos_major: None,
        settings_entry_name: "Halvo",
        screen_activity: false,
    })
}

/// The status of a daemon with its default switches that captures with every grant, as
/// gate-2's fixtures c2-status-macos.txt and c2-status-windows.txt show it.
pub fn sample_status(platform: Platform) -> DaemonStatus {
    let running = |name| SourceLine {
        name,
        state: SourceState::Running,
    };
    let off = |name| SourceLine {
        name,
        state: SourceState::Off,
    };
    let (secure, withheld, permissions, sources, enabled, guard, custody) = match platform {
        Platform::Macos => (
            Some(0),
            None,
            Permission::R1_MACOS
                .iter()
                .map(|&name| PermissionLine {
                    name,
                    grant: GrantState::Granted,
                    at_start: GrantState::Granted,
                    asked: true,
                    once_granted: true,
                })
                .collect(),
            vec![
                running(SourceName::Foreground),
                off(SourceName::Screen),
                running(SourceName::Input),
                off(SourceName::SystemAudio),
                off(SourceName::Microphone),
            ],
            vec![SourceName::Foreground, SourceName::Input],
            PasswordGuard::Os,
            Custody::RootFile,
        ),
        Platform::Windows => (
            None,
            Some(false),
            Vec::new(),
            vec![
                running(SourceName::Foreground),
                off(SourceName::Screen),
                off(SourceName::Keyboard),
                running(SourceName::Pointer),
                running(SourceName::WindowTitle),
                off(SourceName::SystemAudio),
            ],
            vec![
                SourceName::Foreground,
                SourceName::Pointer,
                SourceName::WindowTitle,
            ],
            PasswordGuard::None,
            Custody::Dpapi,
        ),
    };
    let onboarding = onboarding_of(platform, &permissions);
    DaemonStatus::new(StatusFields {
        daemon_version: DaemonVersion {
            major: 0,
            minor: 0,
            patch: 0,
        },
        pid: 4242,
        started_at: at("2026-11-12T09:00:00Z"),
        rss_peak_bytes: 25_264_128,
        parent: Parent::Watched,
        desired: CaptureState::Running,
        capture: Capture::Running,
        capture_reason: None,
        lease: Lease::Fresh,
        store: StoreState::Open,
        store_id: Some(FIXTURE_STORE_ID),
        store_error: None,
        store_retry_in_s: None,
        store_bytes: Some(1_048_576),
        store_logical_bytes: Some(524_288),
        cap_bytes: Cap::Known(StorageCap::DEFAULT),
        drain: Health::Healthy,
        drain_error: None,
        drain_since: None,
        events_pending: 0,
        events_lost: 0,
        dropped_frames: 0,
        pointer_coalesced: 0,
        tap_disabled_timeout: 0,
        tap_disabled_user_input: 0,
        late_events: 0,
        max_lag_ms: 0,
        secure_input_keystrokes: secure,
        keys_withheld: withheld,
        title_read_errors: 0,
        last_flush_at: Some(at("2026-11-12T09:00:05Z")),
        expiry: Health::Healthy,
        expiry_error: None,
        last_expiry_at: Some(at("2026-11-12T09:00:01Z")),
        last_eviction_at: None,
        last_stop: LastStop::Clean,
        last_stop_newest_capture: None,
        state_file: StateFile::Ok,
        platform,
        enabled_sources: enabled,
        switches: SwitchName::ALL
            .iter()
            .map(|&name| SwitchLine {
                name,
                value: if name.default_on() {
                    OnOff::On
                } else {
                    OnOff::Off
                },
            })
            .collect(),
        permissions,
        sources,
        kinds: Modality::ALL
            .iter()
            .map(|&name| KindLine {
                name,
                dropped: 0,
                refused: 0,
            })
            .collect(),
        password_guard: guard,
        screen_filter_excludes_app: None,
        custody,
        onboarding,
        ext: Vec::new(),
    })
    .expect("the sample status holds together")
}
