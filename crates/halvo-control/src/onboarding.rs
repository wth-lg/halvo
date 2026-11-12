//! The permission onboarding the view shows: which macOS permissions capture
//! needs, their state, and the walk-back steps for a missing one.

use serde::{Deserialize, Serialize};

/// A macOS privacy permission that capture needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    ScreenAndSystemAudioRecording,
    Accessibility,
    InputMonitoring,
}

/// The sentence for the screen while the switch `screen_activity` is off
/// (contracts.md C10.6).
pub const SCREEN_ACTIVITY_NOT_RECORDED: &str = "Screen activity is not recorded.";

impl Permission {
    pub const R1_MACOS: [Permission; 3] = [
        Permission::ScreenAndSystemAudioRecording,
        Permission::Accessibility,
        Permission::InputMonitoring,
    ];

    /// The permissions capture needs on the system this was built for.
    pub fn required_on_this_platform() -> &'static [Permission] {
        if cfg!(target_os = "macos") {
            &Self::R1_MACOS
        } else {
            &[]
        }
    }

    const fn pane_name(self) -> &'static str {
        match self {
            Permission::ScreenAndSystemAudioRecording => "Screen & System Audio Recording",
            Permission::Accessibility => "Accessibility",
            Permission::InputMonitoring => "Input Monitoring",
        }
    }

    /// The title of the permission's pane in System Settings on a macOS major.
    pub const fn pane_title_on_macos(self, major_version: Option<u32>) -> &'static str {
        match (self, major_version) {
            (Permission::Accessibility, Some(major)) if major >= 27 => {
                "Device Control and Data Access"
            }
            _ => self.pane_name(),
        }
    }

    /// The System Settings deep link of the permission's pane.
    pub const fn settings_url(self) -> &'static str {
        match self {
            Permission::ScreenAndSystemAudioRecording => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"
            }
            Permission::Accessibility => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
            }
            Permission::InputMonitoring => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent"
            }
        }
    }

    /// What Halvo records with the permission (RD-01), for the state of the switch
    /// `screen_activity`.
    pub const fn why(self, screen_activity: bool) -> &'static str {
        match self {
            Permission::ScreenAndSystemAudioRecording => {
                if screen_activity {
                    "lets Halvo note screen activity, a record that the screen changed with no stored image; system audio stays off"
                } else {
                    SCREEN_ACTIVITY_NOT_RECORDED
                }
            }
            Permission::Accessibility => {
                "lets Halvo record the text of the title of the window in front"
            }
            Permission::InputMonitoring => {
                "lets Halvo record a class for each key you press, never the character, and pointer actions with no position"
            }
        }
    }

    pub const fn needs_relaunch_after_grant(self) -> bool {
        matches!(self, Permission::ScreenAndSystemAudioRecording)
    }
}

/// What the system answers for a permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantState {
    Granted,
    NotGranted,
    NotDetermined,
}

/// What a request for a permission did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestOutcome {
    AlreadyGranted,
    Requested,
}

/// Reads and requests the system's permissions.
pub trait PermissionProbe {
    fn check(&self, permission: Permission) -> GrantState;
    fn request(&self, permission: Permission) -> RequestOutcome;
    fn macos_major_version(&self) -> Option<u32> {
        None
    }
}

/// One permission row of the onboarding view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PermissionStatus {
    pub permission: Permission,
    pub pane_name: &'static str,
    pub settings_url: &'static str,
    pub why: &'static str,
    pub grant: GrantState,
    pub lost: bool,
}

/// What a walk-back step asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WalkbackAction {
    RequestFromSystem,
    OpenSettingsPane,
    Relaunch,
}

/// One step that brings a missing permission back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WalkbackStep {
    pub permission: Permission,
    pub action: WalkbackAction,
    pub settings_url: &'static str,
    pub text: String,
}

/// The onboarding object of contracts.md C10.6.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OnboardingState {
    pub permissions: Vec<PermissionStatus>,
    pub lost_grants: Vec<Permission>,
    pub all_granted: bool,
    pub walkback: Vec<WalkbackStep>,
}

/// The onboarding state of `required` as `probe` reads it; `once_granted` names
/// the permissions that were granted before.
pub fn assess(
    probe: &dyn PermissionProbe,
    required: &[Permission],
    once_granted: &[Permission],
) -> OnboardingState {
    let mut permissions = Vec::with_capacity(required.len());
    let mut lost_grants = Vec::new();
    let mut walkback = Vec::new();
    let macos_major = probe.macos_major_version();

    for &permission in required {
        let pane_name = permission.pane_title_on_macos(macos_major);
        let grant = probe.check(permission);
        let held_before = once_granted.contains(&permission);
        let lost = grant != GrantState::Granted && held_before;
        if grant != GrantState::Granted {
            if lost {
                lost_grants.push(permission);
            }
            walkback.extend(walkback_for(permission, pane_name, lost));
        }
        permissions.push(PermissionStatus {
            permission,
            pane_name,
            settings_url: permission.settings_url(),
            why: permission.why(false),
            grant,
            lost,
        });
    }

    OnboardingState {
        all_granted: walkback.is_empty(),
        permissions,
        lost_grants,
        walkback,
    }
}

fn walkback_for(permission: Permission, pane: &str, lost: bool) -> Vec<WalkbackStep> {
    let step = |action, text: String| WalkbackStep {
        permission,
        action,
        settings_url: permission.settings_url(),
        text,
    };
    let lead = if lost {
        format!("macOS removed Halvo's {pane} access. This happens after a macOS update, a periodic re-check or a new Halvo version. ")
    } else {
        String::new()
    };
    let mut steps = Vec::with_capacity(3);
    steps.push(step(
        WalkbackAction::RequestFromSystem,
        format!("{lead}Click Allow when macOS asks about {pane}."),
    ));
    let relist = if matches!(
        permission,
        Permission::Accessibility | Permission::InputMonitoring
    ) {
        " If Halvo is already listed and still not working, remove it with the minus button and add it again."
    } else {
        ""
    };
    steps.push(step(
        WalkbackAction::OpenSettingsPane,
        format!("Open System Settings > Privacy & Security > {pane} and turn Halvo on.{relist}"),
    ));
    if permission.needs_relaunch_after_grant() {
        steps.push(step(
            WalkbackAction::Relaunch,
            "Quit and reopen Halvo; macOS applies a new screen recording grant only to a fresh launch.".to_string(),
        ));
    }
    steps
}

/// Why the System Settings pane could not be opened.
#[derive(Debug, thiserror::Error)]
pub enum PaneError {
    #[error("could not run open: {0}")]
    Spawn(std::io::Error),
    #[error("open exited with {status} for {url}")]
    Exit {
        status: std::process::ExitStatus,
        url: &'static str,
    },
    #[error("{0} is not implemented on this platform")]
    NotImplemented(&'static str),
}

/// Open the permission's pane in System Settings with /usr/bin/open.
pub fn open_settings_pane(permission: Permission) -> Result<(), PaneError> {
    #[cfg(target_os = "macos")]
    {
        let url = permission.settings_url();
        let status = std::process::Command::new("/usr/bin/open")
            .arg(url)
            .status()
            .map_err(PaneError::Spawn)?;
        if status.success() {
            Ok(())
        } else {
            Err(PaneError::Exit { status, url })
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = permission;
        Err(PaneError::NotImplemented("System Settings deep links"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeProbe(Vec<(Permission, GrantState)>);
    impl PermissionProbe for FakeProbe {
        fn check(&self, permission: Permission) -> GrantState {
            self.0
                .iter()
                .find(|(p, _)| *p == permission)
                .map(|(_, g)| *g)
                .unwrap_or(GrantState::NotDetermined)
        }
        fn request(&self, _permission: Permission) -> RequestOutcome {
            RequestOutcome::Requested
        }
    }

    #[test]
    fn the_three_r1_panes_deep_link_with_the_system_settings_scheme() {
        assert_eq!(
            Permission::ScreenAndSystemAudioRecording.settings_url(),
            "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"
        );
        assert_eq!(
            Permission::Accessibility.settings_url(),
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
        );
        assert_eq!(
            Permission::InputMonitoring.settings_url(),
            "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent"
        );
        assert_eq!(
            Permission::ScreenAndSystemAudioRecording.pane_name(),
            "Screen & System Audio Recording"
        );
        assert_eq!(Permission::Accessibility.pane_name(), "Accessibility");
        assert_eq!(Permission::InputMonitoring.pane_name(), "Input Monitoring");
    }

    #[test]
    fn r1_requests_exactly_three_permissions_on_macos_and_none_elsewhere() {
        let required = Permission::required_on_this_platform();
        if cfg!(target_os = "macos") {
            assert_eq!(required, Permission::R1_MACOS);
        } else {
            assert!(required.is_empty());
        }
        let names: Vec<&str> = Permission::R1_MACOS.iter().map(|p| p.pane_name()).collect();
        assert!(!names
            .iter()
            .any(|n| n.contains("Microphone") || n.contains("Camera")));
    }

    #[test]
    fn the_why_texts_state_what_is_recorded_for_the_switch_defaults() {
        assert_eq!(
            Permission::ScreenAndSystemAudioRecording.why(false),
            "Screen activity is not recorded."
        );
        assert_eq!(
            Permission::ScreenAndSystemAudioRecording.why(true),
            "lets Halvo note screen activity, a record that the screen changed with no stored image; system audio stays off"
        );
        assert_eq!(
            Permission::Accessibility.why(false),
            "lets Halvo record the text of the title of the window in front"
        );
        assert_eq!(
            Permission::InputMonitoring.why(false),
            "lets Halvo record a class for each key you press, never the character, and pointer actions with no position"
        );
        for permission in Permission::R1_MACOS {
            assert!(!permission.why(false).contains("screen activity"));
            assert_eq!(
                permission.why(true) == permission.why(false),
                permission != Permission::ScreenAndSystemAudioRecording
            );
        }
    }

    #[test]
    fn a_grant_that_was_held_and_is_gone_is_reported_lost_with_a_walk_back() {
        let probe = FakeProbe(vec![
            (
                Permission::ScreenAndSystemAudioRecording,
                GrantState::NotGranted,
            ),
            (Permission::Accessibility, GrantState::Granted),
            (Permission::InputMonitoring, GrantState::Granted),
        ]);
        let state = assess(
            &probe,
            &Permission::R1_MACOS,
            &[Permission::ScreenAndSystemAudioRecording],
        );
        assert_eq!(
            state.lost_grants,
            vec![Permission::ScreenAndSystemAudioRecording]
        );
        assert!(!state.all_granted);
        let screen = &state.permissions[0];
        assert!(screen.lost);
        let actions: Vec<WalkbackAction> = state.walkback.iter().map(|s| s.action).collect();
        assert_eq!(
            actions,
            vec![
                WalkbackAction::RequestFromSystem,
                WalkbackAction::OpenSettingsPane,
                WalkbackAction::Relaunch
            ]
        );
        assert!(state
            .walkback
            .iter()
            .all(|s| s.permission == Permission::ScreenAndSystemAudioRecording));
        assert!(state.walkback[0].text.contains("removed"));
    }

    #[test]
    fn a_never_granted_permission_is_missing_not_lost() {
        let probe = FakeProbe(vec![(
            Permission::InputMonitoring,
            GrantState::NotDetermined,
        )]);
        let state = assess(&probe, &[Permission::InputMonitoring], &[]);
        assert!(state.lost_grants.is_empty());
        assert!(!state.permissions[0].lost);
        assert_eq!(state.permissions[0].grant, GrantState::NotDetermined);
        assert_eq!(state.walkback.len(), 2);
        assert_eq!(state.walkback[1].action, WalkbackAction::OpenSettingsPane);
        assert_eq!(
            state.walkback[1].settings_url,
            Permission::InputMonitoring.settings_url()
        );
    }

    #[test]
    fn accessibility_is_requested_from_the_system_and_needs_no_relaunch_step() {
        let probe = FakeProbe(vec![(Permission::Accessibility, GrantState::NotGranted)]);
        let state = assess(&probe, &[Permission::Accessibility], &[]);
        let actions: Vec<WalkbackAction> = state.walkback.iter().map(|s| s.action).collect();
        assert_eq!(
            actions,
            vec![
                WalkbackAction::RequestFromSystem,
                WalkbackAction::OpenSettingsPane
            ]
        );
    }

    #[test]
    fn all_granted_yields_no_walk_back() {
        let probe = FakeProbe(
            Permission::R1_MACOS
                .iter()
                .map(|p| (*p, GrantState::Granted))
                .collect(),
        );
        let state = assess(&probe, &Permission::R1_MACOS, &[]);
        assert!(state.all_granted);
        assert!(state.walkback.is_empty());
        assert!(state
            .permissions
            .iter()
            .all(|p| p.grant == GrantState::Granted));
    }

    #[test]
    fn macos_27_retitles_the_accessibility_pane_and_nothing_else() {
        assert_eq!(
            Permission::Accessibility.pane_title_on_macos(Some(27)),
            "Device Control and Data Access"
        );
        assert_eq!(
            Permission::Accessibility.pane_title_on_macos(Some(26)),
            "Accessibility"
        );
        assert_eq!(
            Permission::Accessibility.pane_title_on_macos(None),
            "Accessibility"
        );
        for p in [
            Permission::ScreenAndSystemAudioRecording,
            Permission::InputMonitoring,
        ] {
            assert_eq!(p.pane_title_on_macos(Some(27)), p.pane_name());
        }
    }

    #[test]
    fn the_walk_back_names_the_pane_as_the_running_macos_shows_it() {
        struct Probe27;
        impl PermissionProbe for Probe27 {
            fn check(&self, _: Permission) -> GrantState {
                GrantState::NotGranted
            }
            fn request(&self, _: Permission) -> RequestOutcome {
                RequestOutcome::Requested
            }
            fn macos_major_version(&self) -> Option<u32> {
                Some(27)
            }
        }
        let state = assess(&Probe27, &[Permission::Accessibility], &[]);
        assert_eq!(
            state.permissions[0].pane_name,
            "Device Control and Data Access"
        );
        assert!(state.walkback[0]
            .text
            .contains("Device Control and Data Access"));
    }

    #[test]
    fn permissions_serialize_in_snake_case_for_the_view() {
        let json = serde_json::to_string(&Permission::ScreenAndSystemAudioRecording).unwrap();
        assert_eq!(json, "\"screen_and_system_audio_recording\"");
        let json = serde_json::to_string(&GrantState::NotDetermined).unwrap();
        assert_eq!(json, "\"not_determined\"");
    }
}
