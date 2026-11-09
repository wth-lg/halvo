//! Just-in-time permission onboarding (phase P5).
//!
//! macOS gates capture behind three Privacy & Security panes: Screen & System
//! Audio Recording (frames; system audio shares the grant but stays off in R1),
//! Accessibility (window titles) and Input Monitoring (the listen-only event tap
//! for keystrokes and pointer). Microphone and camera are not requested in R1.
//! Each pane is deep-linked with the `x-apple.systempreferences:` scheme, so a
//! non-technical user lands on the exact switch (researcher-3 and researcher-4).
//!
//! macOS also takes grants away: it re-prompts for screen recording
//! periodically, resets grants after an OS update and after the app's code
//! signature changes. [`assess`] compares what the OS reports now with the
//! permissions Halvo has held before and turns every gap into a [`WalkbackStep`]
//! list the view shows: ask the system, open the pane, relaunch where needed.
//!
//! Windows has no privacy-pane gate for screen or input capture (researcher-4),
//! so [`Permission::required_on_this_platform`] is empty there and the view
//! shows no onboarding.

use serde::{Deserialize, Serialize};

use super::ConsentError;

/// The macOS Privacy & Security grants R1 capture needs. A closed set: the
/// microphone and camera panes are not in it because those modalities are off
/// and not requested in R1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    ScreenAndSystemAudioRecording,
    Accessibility,
    InputMonitoring,
}

impl Permission {
    /// The three R1 grants, in the order the view asks for them.
    pub const R1_MACOS: [Permission; 3] = [
        Permission::ScreenAndSystemAudioRecording,
        Permission::Accessibility,
        Permission::InputMonitoring,
    ];

    /// The grants this platform gates capture behind: all three on macOS, none
    /// on Windows, where screen and input capture have no Privacy page gate.
    pub fn required_on_this_platform() -> &'static [Permission] {
        if cfg!(target_os = "macos") {
            &Self::R1_MACOS
        } else {
            &[]
        }
    }

    /// The pane's name as System Settings shows it.
    pub const fn pane_name(self) -> &'static str {
        match self {
            Permission::ScreenAndSystemAudioRecording => "Screen & System Audio Recording",
            Permission::Accessibility => "Accessibility",
            Permission::InputMonitoring => "Input Monitoring",
        }
    }

    /// The pane's title as the running macOS shows it. macOS 27 retitled the
    /// Accessibility privacy pane "Device Control and Data Access" (measured on
    /// 27.2: the `Privacy_Accessibility` anchor opens a window with that title
    /// and the accessibility app list); the deep link is unchanged.
    pub const fn pane_title_on_macos(self, major_version: Option<u32>) -> &'static str {
        match (self, major_version) {
            (Permission::Accessibility, Some(major)) if major >= 27 => {
                "Device Control and Data Access"
            }
            _ => self.pane_name(),
        }
    }

    /// The deep link that opens the exact pane in System Settings.
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

    /// Why Halvo needs it, in the user's terms.
    pub const fn why(self) -> &'static str {
        match self {
            Permission::ScreenAndSystemAudioRecording => {
                "lets Halvo see your screen so it can learn your workflows; system audio stays off"
            }
            Permission::Accessibility => "lets Halvo read window titles so it knows which document or page you are in",
            Permission::InputMonitoring => {
                "lets Halvo notice typing and mouse activity; passwords are never recorded because macOS hides secure fields"
            }
        }
    }

    /// Whether macOS shows its own consent dialog when the app asks. The
    /// Accessibility pane has no supported prompt, so the walk-back opens the
    /// pane directly.
    pub const fn has_system_prompt(self) -> bool {
        matches!(
            self,
            Permission::ScreenAndSystemAudioRecording | Permission::InputMonitoring
        )
    }

    /// Whether a new grant takes effect only after the app restarts. Screen
    /// recording does; the other two apply to the running process.
    pub const fn needs_relaunch_after_grant(self) -> bool {
        matches!(self, Permission::ScreenAndSystemAudioRecording)
    }
}

/// What the OS reports for one permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantState {
    Granted,
    /// The pane lists Halvo with its switch off, or the OS answers no.
    NotGranted,
    /// The user has never been asked (only Input Monitoring reports this).
    NotDetermined,
}

/// What happened when the app asked the system for a grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestOutcome {
    /// The grant is already held.
    AlreadyGranted,
    /// macOS showed (or queued) its consent dialog; re-check after the user answers.
    PromptShown,
    /// macOS will not prompt again (already denied once); the pane is the only way.
    NoPromptAvailable,
}

/// Reads and requests grants from the OS. The macOS probe is
/// [`platform_probe`]; tests use a fake.
pub trait PermissionProbe {
    fn check(&self, permission: Permission) -> GrantState;
    fn request(&self, permission: Permission) -> RequestOutcome;
    /// The macOS major version, for pane titles that changed between releases.
    /// `None` off macOS or when it cannot be read.
    fn macos_major_version(&self) -> Option<u32> {
        None
    }
}

/// One permission as the view shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PermissionStatus {
    pub permission: Permission,
    pub pane_name: &'static str,
    pub settings_url: &'static str,
    pub why: &'static str,
    pub grant: GrantState,
    /// Granted before on this install and not granted now.
    pub lost: bool,
}

/// A step that walks the user back to a working grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WalkbackAction {
    RequestFromSystem,
    OpenSettingsPane,
    Relaunch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WalkbackStep {
    pub permission: Permission,
    pub action: WalkbackAction,
    pub settings_url: &'static str,
    pub text: String,
}

/// The onboarding state the view polls: every required permission, which ones
/// were lost, the steps that walk the user back, and the grants held now (for
/// the caller to record).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OnboardingState {
    pub permissions: Vec<PermissionStatus>,
    pub lost_grants: Vec<Permission>,
    pub granted_now: Vec<Permission>,
    pub all_granted: bool,
    pub walkback: Vec<WalkbackStep>,
}

/// Compare what the OS grants now against `once_granted` for every permission
/// in `required`, and build the walk-back for each gap.
pub fn assess(
    probe: &dyn PermissionProbe,
    required: &[Permission],
    once_granted: &[Permission],
) -> OnboardingState {
    let mut permissions = Vec::with_capacity(required.len());
    let mut lost_grants = Vec::new();
    let mut granted_now = Vec::new();
    let mut walkback = Vec::new();
    let macos_major = probe.macos_major_version();

    for &permission in required {
        let pane_name = permission.pane_title_on_macos(macos_major);
        let grant = probe.check(permission);
        let held_before = once_granted.contains(&permission);
        let lost = grant != GrantState::Granted && held_before;
        if grant == GrantState::Granted {
            granted_now.push(permission);
        } else {
            if lost {
                lost_grants.push(permission);
            }
            walkback.extend(walkback_for(permission, pane_name, lost));
        }
        permissions.push(PermissionStatus {
            permission,
            pane_name,
            settings_url: permission.settings_url(),
            why: permission.why(),
            grant,
            lost,
        });
    }

    OnboardingState {
        all_granted: walkback.is_empty(),
        permissions,
        lost_grants,
        granted_now,
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
    if permission.has_system_prompt() {
        steps.push(step(
            WalkbackAction::RequestFromSystem,
            format!("{lead}Click Allow when macOS asks about {pane}."),
        ));
    }
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
        format!(
            "{}Open System Settings > Privacy & Security > {pane} and turn Halvo on.{relist}",
            if steps.is_empty() { lead.as_str() } else { "" }
        ),
    ));
    if permission.needs_relaunch_after_grant() {
        steps.push(step(
            WalkbackAction::Relaunch,
            "Quit and reopen Halvo; macOS applies a new screen recording grant only to a fresh launch.".to_string(),
        ));
    }
    steps
}

/// Open the pane for `permission` in System Settings through its deep link.
pub fn open_settings_pane(permission: Permission) -> Result<(), ConsentError> {
    #[cfg(target_os = "macos")]
    {
        let status = std::process::Command::new("/usr/bin/open")
            .arg(permission.settings_url())
            .status()
            .map_err(|e| ConsentError::Action(format!("could not run open: {e}")))?;
        if status.success() {
            Ok(())
        } else {
            Err(ConsentError::Action(format!(
                "open exited with {status} for {}",
                permission.settings_url()
            )))
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = permission;
        Err(ConsentError::NotImplemented(
            "System Settings deep links on this platform",
        ))
    }
}

/// The probe for the platform this binary runs on.
pub fn platform_probe() -> Box<dyn PermissionProbe + Send + Sync> {
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::TccProbe)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Box::new(NoGateProbe)
    }
}

/// A platform with no permission gate: every permission reads as granted.
/// Windows uses it (researcher-4: no Privacy page gate for screen or input).
#[cfg(not(target_os = "macos"))]
pub struct NoGateProbe;

#[cfg(not(target_os = "macos"))]
impl PermissionProbe for NoGateProbe {
    fn check(&self, _permission: Permission) -> GrantState {
        GrantState::Granted
    }
    fn request(&self, _permission: Permission) -> RequestOutcome {
        RequestOutcome::AlreadyGranted
    }
}

/// The macOS probe over the TCC query functions: `CGPreflightScreenCaptureAccess`
/// (screen), `AXIsProcessTrusted` (accessibility) and `IOHIDCheckAccess` (input
/// monitoring), with the matching request calls where the OS has a prompt.
#[cfg(target_os = "macos")]
pub mod macos {
    use super::{GrantState, Permission, PermissionProbe, RequestOutcome};

    const IOHID_REQUEST_TYPE_LISTEN_EVENT: u32 = 1;
    const IOHID_ACCESS_TYPE_GRANTED: u32 = 0;
    const IOHID_ACCESS_TYPE_DENIED: u32 = 1;

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGPreflightScreenCaptureAccess() -> bool;
        fn CGRequestScreenCaptureAccess() -> bool;
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXIsProcessTrusted() -> bool;
    }

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOHIDCheckAccess(request_type: u32) -> u32;
        fn IOHIDRequestAccess(request_type: u32) -> bool;
    }

    pub struct TccProbe;

    /// Read the major version from `sw_vers -productVersion` once.
    fn macos_major_version() -> Option<u32> {
        static MAJOR: std::sync::OnceLock<Option<u32>> = std::sync::OnceLock::new();
        *MAJOR.get_or_init(|| {
            let output = std::process::Command::new("/usr/bin/sw_vers")
                .arg("-productVersion")
                .output()
                .ok()?;
            let text = String::from_utf8(output.stdout).ok()?;
            text.trim().split('.').next()?.parse().ok()
        })
    }

    impl PermissionProbe for TccProbe {
        fn macos_major_version(&self) -> Option<u32> {
            macos_major_version()
        }

        fn check(&self, permission: Permission) -> GrantState {
            match permission {
                Permission::ScreenAndSystemAudioRecording => {
                    // SAFETY: a plain query with no arguments, documented thread-safe.
                    if unsafe { CGPreflightScreenCaptureAccess() } {
                        GrantState::Granted
                    } else {
                        GrantState::NotGranted
                    }
                }
                Permission::Accessibility => {
                    // SAFETY: a plain query with no arguments.
                    if unsafe { AXIsProcessTrusted() } {
                        GrantState::Granted
                    } else {
                        GrantState::NotGranted
                    }
                }
                Permission::InputMonitoring => {
                    // SAFETY: takes one enum value by value and returns an enum.
                    match unsafe { IOHIDCheckAccess(IOHID_REQUEST_TYPE_LISTEN_EVENT) } {
                        IOHID_ACCESS_TYPE_GRANTED => GrantState::Granted,
                        IOHID_ACCESS_TYPE_DENIED => GrantState::NotGranted,
                        _ => GrantState::NotDetermined,
                    }
                }
            }
        }

        fn request(&self, permission: Permission) -> RequestOutcome {
            if self.check(permission) == GrantState::Granted {
                return RequestOutcome::AlreadyGranted;
            }
            match permission {
                Permission::ScreenAndSystemAudioRecording => {
                    // SAFETY: no arguments; shows the system prompt at most once per install.
                    if unsafe { CGRequestScreenCaptureAccess() } {
                        RequestOutcome::AlreadyGranted
                    } else {
                        RequestOutcome::PromptShown
                    }
                }
                Permission::InputMonitoring => {
                    // SAFETY: one enum argument by value.
                    if unsafe { IOHIDRequestAccess(IOHID_REQUEST_TYPE_LISTEN_EVENT) } {
                        RequestOutcome::AlreadyGranted
                    } else {
                        RequestOutcome::PromptShown
                    }
                }
                Permission::Accessibility => RequestOutcome::NoPromptAvailable,
            }
        }
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
            RequestOutcome::PromptShown
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
    fn accessibility_has_no_system_prompt_and_no_relaunch_step() {
        let probe = FakeProbe(vec![(Permission::Accessibility, GrantState::NotGranted)]);
        let state = assess(&probe, &[Permission::Accessibility], &[]);
        let actions: Vec<WalkbackAction> = state.walkback.iter().map(|s| s.action).collect();
        assert_eq!(actions, vec![WalkbackAction::OpenSettingsPane]);
    }

    #[test]
    fn all_granted_yields_no_walk_back_and_records_every_grant() {
        let probe = FakeProbe(
            Permission::R1_MACOS
                .iter()
                .map(|p| (*p, GrantState::Granted))
                .collect(),
        );
        let state = assess(&probe, &Permission::R1_MACOS, &[]);
        assert!(state.all_granted);
        assert!(state.walkback.is_empty());
        assert_eq!(state.granted_now, Permission::R1_MACOS.to_vec());
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
                RequestOutcome::NoPromptAvailable
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
