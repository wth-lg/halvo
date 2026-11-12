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

    /// Whether a grant takes effect in the capturing process only after it
    /// restarts: the screen and Input Monitoring (RD-29).
    pub const fn restarts_capture(self) -> bool {
        matches!(
            self,
            Permission::ScreenAndSystemAudioRecording | Permission::InputMonitoring
        )
    }

    const fn index(self) -> usize {
        match self {
            Permission::ScreenAndSystemAudioRecording => 0,
            Permission::Accessibility => 1,
            Permission::InputMonitoring => 2,
        }
    }
}

/// A set of permissions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct PermissionSet(u8);

impl PermissionSet {
    pub const EMPTY: PermissionSet = PermissionSet(0);

    pub const fn contains(self, permission: Permission) -> bool {
        self.0 & (1 << permission.index()) != 0
    }

    pub fn insert(&mut self, permission: Permission) {
        self.0 |= 1 << permission.index();
    }

    /// The members, in the order of `Permission::R1_MACOS`.
    pub fn iter(self) -> impl Iterator<Item = Permission> {
        Permission::R1_MACOS
            .into_iter()
            .filter(move |&permission| self.contains(permission))
    }
}

impl FromIterator<Permission> for PermissionSet {
    fn from_iter<I: IntoIterator<Item = Permission>>(permissions: I) -> Self {
        let mut set = Self::EMPTY;
        permissions
            .into_iter()
            .for_each(|permission| set.insert(permission));
        set
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

/// What the system answers now for each permission; a permission that was not
/// checked reads not_determined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grants([GrantState; 3]);

impl Default for Grants {
    fn default() -> Self {
        Self([GrantState::NotDetermined; 3])
    }
}

impl Grants {
    pub const fn of(self, permission: Permission) -> GrantState {
        self.0[permission.index()]
    }

    pub fn with(mut self, permission: Permission, grant: GrantState) -> Self {
        self.0[permission.index()] = grant;
        self
    }

    /// The permissions that read granted.
    pub fn granted(self) -> PermissionSet {
        Permission::R1_MACOS
            .into_iter()
            .filter(|&permission| self.of(permission) == GrantState::Granted)
            .collect()
    }
}

/// Reads and requests the system's permissions.
pub trait PermissionProbe {
    fn check(&self, permission: Permission) -> GrantState;
    fn request(&self, permission: Permission) -> RequestOutcome;
    fn macos_major_version(&self) -> Option<u32> {
        None
    }
}

/// One permission row of the onboarding view (contracts.md C10.6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionStatus {
    pub permission: Permission,
    pub pane_name: String,
    pub settings_url: String,
    pub why: String,
    pub grant: GrantState,
    pub lost: bool,
    pub asked: bool,
}

/// What a walk-back step asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WalkbackAction {
    RequestFromSystem,
    OpenSettingsPane,
    RestartCapture,
}

/// One step that brings a missing permission back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WalkbackStep {
    pub permission: Permission,
    pub action: WalkbackAction,
    pub settings_url: String,
    pub text: String,
}

/// The onboarding object of contracts.md C10.6.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OnboardingState {
    pub permissions: Vec<PermissionStatus>,
    pub lost_grants: Vec<Permission>,
    pub all_granted: bool,
    pub walkback: Vec<WalkbackStep>,
}

/// What `assess` reads (contracts.md C10.6).
#[derive(Debug, Clone, Copy)]
pub struct AssessInput<'a> {
    /// The permissions to walk, in order.
    pub required: &'a [Permission],
    /// What the system answers now.
    pub now: Grants,
    /// The permissions granted when the capturing process started.
    pub at_start: PermissionSet,
    /// The permissions that Halvo has asked macOS for.
    pub asked: PermissionSet,
    /// The permissions that were granted once.
    pub once_granted: PermissionSet,
    /// The running macOS major, when known.
    pub macos_major: Option<u32>,
    /// The name of the entry that the person turns on in System Settings.
    pub entry_name: &'a str,
    /// Whether the switch screen_activity is on (contracts.md C13.5).
    pub screen_activity: bool,
}

/// The onboarding state of `input.required`. `all_granted` is true when every
/// required permission reads granted now. The walk-back ends with one
/// restart_capture step while a permission of the restart class is missing, or
/// is granted now and was not when the capturing process started; the step
/// names the first such permission in required order.
pub fn assess(input: &AssessInput<'_>) -> OnboardingState {
    let mut permissions = Vec::with_capacity(input.required.len());
    let mut lost_grants = Vec::new();
    let mut walkback = Vec::new();
    let mut restart = None;

    for &permission in input.required {
        let pane_name = permission.pane_title_on_macos(input.macos_major);
        let grant = input.now.of(permission);
        let granted = grant == GrantState::Granted;
        let lost = !granted && input.once_granted.contains(permission);
        if !granted {
            if lost {
                lost_grants.push(permission);
            }
            walkback.extend(walkback_for(permission, pane_name, lost, input.entry_name));
        }
        if restart.is_none()
            && permission.restarts_capture()
            && !(granted && input.at_start.contains(permission))
        {
            restart = Some((permission, pane_name));
        }
        permissions.push(PermissionStatus {
            permission,
            pane_name: pane_name.to_string(),
            settings_url: permission.settings_url().to_string(),
            why: permission.why(input.screen_activity).to_string(),
            grant,
            lost,
            asked: input.asked.contains(permission),
        });
    }
    if let Some((permission, pane)) = restart {
        walkback.push(WalkbackStep {
            permission,
            action: WalkbackAction::RestartCapture,
            settings_url: permission.settings_url().to_string(),
            text: format!(
                "Click Restart recorder: Halvo restarts its recorder so that macOS applies the {pane} permission."
            ),
        });
    }

    OnboardingState {
        all_granted: permissions.iter().all(|p| p.grant == GrantState::Granted),
        permissions,
        lost_grants,
        walkback,
    }
}

fn walkback_for(permission: Permission, pane: &str, lost: bool, entry: &str) -> Vec<WalkbackStep> {
    let step = |action, text: String| WalkbackStep {
        permission,
        action,
        settings_url: permission.settings_url().to_string(),
        text,
    };
    let lead = if lost {
        format!("macOS removed Halvo's {pane} access. This happens after a macOS update, a periodic re-check or a new Halvo version. ")
    } else {
        String::new()
    };
    let relist = if matches!(
        permission,
        Permission::Accessibility | Permission::InputMonitoring
    ) {
        format!(" If {entry} is already listed and still not working, remove it with the minus button and add it again.")
    } else {
        String::new()
    };
    vec![
        step(
            WalkbackAction::RequestFromSystem,
            format!("{lead}Click Allow when macOS asks about {pane}."),
        ),
        step(
            WalkbackAction::OpenSettingsPane,
            format!(
                "Open System Settings > Privacy & Security > {pane} and turn {entry} on.{relist}"
            ),
        ),
    ]
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

    use Permission::{Accessibility, InputMonitoring, ScreenAndSystemAudioRecording as Screen};

    fn input<'a>(required: &'a [Permission], now: &[(Permission, GrantState)]) -> AssessInput<'a> {
        let now = now
            .iter()
            .fold(Grants::default(), |grants, &(permission, grant)| {
                grants.with(permission, grant)
            });
        AssessInput {
            required,
            now,
            at_start: now.granted(),
            asked: PermissionSet::EMPTY,
            once_granted: PermissionSet::EMPTY,
            macos_major: Some(26),
            entry_name: "Halvo",
            screen_activity: false,
        }
    }

    fn sets() -> Vec<PermissionSet> {
        (0..8u8)
            .map(|bits| {
                Permission::R1_MACOS
                    .into_iter()
                    .enumerate()
                    .filter(|(at, _)| bits & (1 << at) != 0)
                    .map(|(_, permission)| permission)
                    .collect()
            })
            .collect()
    }

    fn every_state(mut visit: impl FnMut(&AssessInput<'_>, &OnboardingState)) {
        let grants = [
            GrantState::Granted,
            GrantState::NotGranted,
            GrantState::NotDetermined,
        ];
        for screen in grants {
            for accessibility in grants {
                for input_monitoring in grants {
                    let now = Grants::default()
                        .with(Screen, screen)
                        .with(Accessibility, accessibility)
                        .with(InputMonitoring, input_monitoring);
                    for at_start in sets() {
                        for asked in sets() {
                            for once_granted in sets() {
                                for macos_major in [None, Some(26), Some(27)] {
                                    let input = AssessInput {
                                        required: &Permission::R1_MACOS,
                                        now,
                                        at_start,
                                        asked,
                                        once_granted,
                                        macos_major,
                                        entry_name: "Halvo",
                                        screen_activity: false,
                                    };
                                    visit(&input, &assess(&input));
                                }
                            }
                        }
                    }
                }
            }
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
        let mut input = input(
            &Permission::R1_MACOS,
            &[
                (Screen, GrantState::NotGranted),
                (Accessibility, GrantState::Granted),
                (InputMonitoring, GrantState::Granted),
            ],
        );
        input.once_granted = [Screen].into_iter().collect();
        let state = assess(&input);
        assert_eq!(state.lost_grants, vec![Screen]);
        assert!(!state.all_granted);
        assert!(state.permissions[0].lost);
        assert!(state.walkback.iter().all(|s| s.permission == Screen));
        assert_eq!(
            state.walkback.last().map(|s| s.action),
            Some(WalkbackAction::RestartCapture)
        );
    }

    #[test]
    fn a_never_granted_permission_is_missing_not_lost() {
        let state = assess(&input(
            &[InputMonitoring],
            &[(InputMonitoring, GrantState::NotDetermined)],
        ));
        assert!(state.lost_grants.is_empty());
        assert!(!state.permissions[0].lost);
        assert_eq!(state.permissions[0].grant, GrantState::NotDetermined);
        assert!(state
            .walkback
            .iter()
            .any(|s| s.action == WalkbackAction::OpenSettingsPane
                && s.settings_url == InputMonitoring.settings_url()));
    }

    #[test]
    fn input_monitoring_not_granted_ends_with_the_restart_step() {
        for grant in [GrantState::NotGranted, GrantState::NotDetermined] {
            let state = assess(&input(&[InputMonitoring], &[(InputMonitoring, grant)]));
            let last = state.walkback.last().unwrap();
            assert_eq!(last.action, WalkbackAction::RestartCapture);
            assert_eq!(last.permission, InputMonitoring);
        }
    }

    #[test]
    fn three_missing_permissions_give_one_restart_step_in_last_place() {
        let state = assess(&input(&Permission::R1_MACOS, &[]));
        let restarts: Vec<usize> = state
            .walkback
            .iter()
            .enumerate()
            .filter(|(_, s)| s.action == WalkbackAction::RestartCapture)
            .map(|(at, _)| at)
            .collect();
        assert_eq!(restarts, vec![state.walkback.len() - 1]);
        assert_eq!(state.walkback.last().unwrap().permission, Screen);
    }

    #[test]
    fn a_capture_grant_that_came_after_the_process_started_asks_for_the_restart_alone() {
        let granted = [
            (Screen, GrantState::Granted),
            (Accessibility, GrantState::Granted),
            (InputMonitoring, GrantState::Granted),
        ];
        let mut input = input(&Permission::R1_MACOS, &granted);
        input.at_start = [Screen, Accessibility].into_iter().collect();
        let state = assess(&input);
        assert!(state.all_granted);
        assert_eq!(state.walkback.len(), 1);
        assert_eq!(state.walkback[0].action, WalkbackAction::RestartCapture);
        assert_eq!(state.walkback[0].permission, InputMonitoring);
        input.at_start = [Screen, InputMonitoring].into_iter().collect();
        assert!(assess(&input).walkback.is_empty());
        input.at_start = Permission::R1_MACOS.into_iter().collect();
        assert!(assess(&input).walkback.is_empty());
    }

    #[test]
    fn accessibility_needs_no_restart_step() {
        let state = assess(&input(
            &[Accessibility],
            &[(Accessibility, GrantState::NotGranted)],
        ));
        assert!(!state.walkback.is_empty());
        assert!(state
            .walkback
            .iter()
            .all(|s| s.action != WalkbackAction::RestartCapture));
    }

    #[test]
    fn in_every_state_one_restart_step_at_most_comes_last_and_no_text_says_quit() {
        let mut states = 0;
        every_state(|input, state| {
            states += 1;
            let restarts = state
                .walkback
                .iter()
                .filter(|s| s.action == WalkbackAction::RestartCapture)
                .count();
            let wanted = input.required.iter().any(|&p| {
                p.restarts_capture()
                    && (input.now.of(p) != GrantState::Granted || !input.at_start.contains(p))
            });
            assert_eq!(restarts, usize::from(wanted));
            if wanted {
                assert_eq!(
                    state.walkback.last().map(|s| s.action),
                    Some(WalkbackAction::RestartCapture)
                );
            }
            assert_eq!(
                state.all_granted,
                input
                    .required
                    .iter()
                    .all(|&p| input.now.of(p) == GrantState::Granted)
            );
            for step in &state.walkback {
                assert!(!step.text.contains("Quit"), "{}", step.text);
            }
        });
        assert_eq!(states, 27 * 8 * 8 * 8 * 3);
    }

    #[test]
    fn all_granted_yields_no_walk_back() {
        let granted: Vec<(Permission, GrantState)> = Permission::R1_MACOS
            .iter()
            .map(|p| (*p, GrantState::Granted))
            .collect();
        let state = assess(&input(&Permission::R1_MACOS, &granted));
        assert!(state.all_granted);
        assert!(state.walkback.is_empty());
        assert!(state
            .permissions
            .iter()
            .all(|p| p.grant == GrantState::Granted));
    }

    #[test]
    fn the_walk_back_actions_have_their_wire_spellings() {
        for (action, text) in [
            (WalkbackAction::RequestFromSystem, "\"request_from_system\""),
            (WalkbackAction::OpenSettingsPane, "\"open_settings_pane\""),
            (WalkbackAction::RestartCapture, "\"restart_capture\""),
        ] {
            assert_eq!(serde_json::to_string(&action).unwrap(), text);
            assert_eq!(
                serde_json::from_str::<WalkbackAction>(text).unwrap(),
                action
            );
        }
        assert!(serde_json::from_str::<WalkbackAction>("\"relaunch\"").is_err());
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
        let mut input = input(&[Accessibility], &[(Accessibility, GrantState::NotGranted)]);
        input.macos_major = Some(27);
        let state = assess(&input);
        assert_eq!(
            state.permissions[0].pane_name,
            "Device Control and Data Access"
        );
        assert!(state.walkback[0]
            .text
            .contains("Device Control and Data Access"));
    }

    const ONBOARDING_JSON: &str = r#"{"permissions":[{"permission":"accessibility","pane_name":"Accessibility","settings_url":"x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility","why":"the why text","grant":"not_granted","lost":true,"asked":true},{"permission":"input_monitoring","pane_name":"Input Monitoring","settings_url":"x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent","why":"another why text","grant":"not_determined","lost":false,"asked":false}],"lost_grants":["accessibility"],"all_granted":false,"walkback":[{"permission":"accessibility","action":"open_settings_pane","settings_url":"x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility","text":"a step text"},{"permission":"input_monitoring","action":"request_from_system","settings_url":"x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent","text":"another step text"}]}"#;

    #[test]
    fn the_onboarding_object_has_golden_json_and_reads_back() {
        let row = |permission: Permission, why: &str, grant, lost, asked| PermissionStatus {
            permission,
            pane_name: permission.pane_title_on_macos(Some(26)).to_string(),
            settings_url: permission.settings_url().to_string(),
            why: why.to_string(),
            grant,
            lost,
            asked,
        };
        let step = |permission: Permission, action, text: &str| WalkbackStep {
            permission,
            action,
            settings_url: permission.settings_url().to_string(),
            text: text.to_string(),
        };
        let state = OnboardingState {
            permissions: vec![
                row(
                    Permission::Accessibility,
                    "the why text",
                    GrantState::NotGranted,
                    true,
                    true,
                ),
                row(
                    Permission::InputMonitoring,
                    "another why text",
                    GrantState::NotDetermined,
                    false,
                    false,
                ),
            ],
            lost_grants: vec![Permission::Accessibility],
            all_granted: false,
            walkback: vec![
                step(
                    Permission::Accessibility,
                    WalkbackAction::OpenSettingsPane,
                    "a step text",
                ),
                step(
                    Permission::InputMonitoring,
                    WalkbackAction::RequestFromSystem,
                    "another step text",
                ),
            ],
        };
        assert_eq!(serde_json::to_string(&state).unwrap(), ONBOARDING_JSON);
        assert_eq!(
            serde_json::from_str::<OnboardingState>(ONBOARDING_JSON).unwrap(),
            state
        );
    }

    #[test]
    fn an_assessed_onboarding_state_reads_back_from_a_buffer() {
        let mut input = input(
            &Permission::R1_MACOS,
            &[(Accessibility, GrantState::Granted)],
        );
        input.once_granted = [InputMonitoring].into_iter().collect();
        let state = assess(&input);
        let buffer: String = serde_json::to_string(&state).unwrap();
        let read: OnboardingState = serde_json::from_str(&buffer).unwrap();
        drop(buffer);
        assert_eq!(read, state);
    }

    #[test]
    fn permissions_serialize_in_snake_case_for_the_view() {
        let json = serde_json::to_string(&Permission::ScreenAndSystemAudioRecording).unwrap();
        assert_eq!(json, "\"screen_and_system_audio_recording\"");
        let json = serde_json::to_string(&GrantState::NotDetermined).unwrap();
        assert_eq!(json, "\"not_determined\"");
    }
}
