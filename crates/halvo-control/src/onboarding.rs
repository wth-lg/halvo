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

    /// The title of the permission's pane in System Settings on a macOS major:
    /// the row of `MACOS_ROWS` at or below it, and the floor's row below the floor
    /// or when the major is unknown.
    pub fn pane_title_on_macos(self, major_version: Option<u32>) -> &'static str {
        macos_row(major_version).pane_title(self)
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

    /// Take a permission out, as the acknowledge of a lost permission does with
    /// the once-granted set.
    pub fn remove(&mut self, permission: Permission) {
        self.0 &= !(1 << permission.index());
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

/// The oldest macOS major that Halvo supports (RD-09); package-mac declares the
/// same minimum.
pub const MACOS_FLOOR_MAJOR: u32 = 14;

/// What macOS shows for the permissions on one major version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacosRow {
    pub major: u32,
    pub screen_pane: &'static str,
    pub accessibility_pane: &'static str,
    pub input_monitoring_pane: &'static str,
    /// The button of the permission alert that leads to System Settings: observed
    /// on 27; the other rows carry the same words until someone observes them.
    pub alert_button: &'static str,
}

impl MacosRow {
    pub const fn pane_title(&self, permission: Permission) -> &'static str {
        match permission {
            Permission::ScreenAndSystemAudioRecording => self.screen_pane,
            Permission::Accessibility => self.accessibility_pane,
            Permission::InputMonitoring => self.input_monitoring_pane,
        }
    }
}

const SCREEN_PANE: &str = "Screen & System Audio Recording";
const INPUT_MONITORING_PANE: &str = "Input Monitoring";
const ALERT_BUTTON: &str = "Open System Settings";

/// The supported macOS majors, from the floor up. Nobody saw a pane title on a
/// version other than 27.2.
pub const MACOS_ROWS: [MacosRow; 4] = [
    MacosRow {
        major: MACOS_FLOOR_MAJOR,
        screen_pane: SCREEN_PANE,
        accessibility_pane: "Accessibility",
        input_monitoring_pane: INPUT_MONITORING_PANE,
        alert_button: ALERT_BUTTON,
    },
    MacosRow {
        major: 15,
        screen_pane: SCREEN_PANE,
        accessibility_pane: "Accessibility",
        input_monitoring_pane: INPUT_MONITORING_PANE,
        alert_button: ALERT_BUTTON,
    },
    MacosRow {
        major: 26,
        screen_pane: SCREEN_PANE,
        accessibility_pane: "Accessibility",
        input_monitoring_pane: INPUT_MONITORING_PANE,
        alert_button: ALERT_BUTTON,
    },
    MacosRow {
        major: 27,
        screen_pane: SCREEN_PANE,
        accessibility_pane: "Device Control and Data Access",
        input_monitoring_pane: INPUT_MONITORING_PANE,
        alert_button: ALERT_BUTTON,
    },
];

/// The row of a macOS major: the last row at or below it, and the floor's row
/// below the floor or when the major is unknown.
fn macos_row(major: Option<u32>) -> &'static MacosRow {
    MACOS_ROWS
        .iter()
        .rev()
        .find(|row| major.is_some_and(|major| major >= row.major))
        .unwrap_or(&MACOS_ROWS[0])
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

/// The onboarding state of `input.required`; any permission may read
/// not_determined. `all_granted` is true when every required permission reads
/// granted now. A missing permission gets a request step unless it is in the
/// asked set, lost, or a denied Input Monitoring; Input Monitoring that reads
/// not_determined always gets one, because only its check tells a denial apart. The walk-back ends with one
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
            walkback.extend(walkback_for(&StepContext {
                permission,
                grant,
                pane: pane_name,
                lost,
                asked: input.asked.contains(permission),
                entry: input.entry_name,
                alert_button: macos_row(input.macos_major).alert_button,
            }));
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
                "Halvo restarts its recorder so that macOS applies the {pane} permission."
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

struct StepContext<'a> {
    permission: Permission,
    grant: GrantState,
    pane: &'a str,
    lost: bool,
    asked: bool,
    entry: &'a str,
    alert_button: &'a str,
}

fn walkback_for(context: &StepContext<'_>) -> Vec<WalkbackStep> {
    let StepContext {
        permission,
        grant,
        pane,
        lost,
        asked,
        entry,
        alert_button,
    } = *context;
    let step = |action, text: String| WalkbackStep {
        permission,
        action,
        settings_url: permission.settings_url().to_string(),
        text,
    };
    let undetermined_input_monitoring =
        permission == Permission::InputMonitoring && grant == GrantState::NotDetermined;
    let denied = permission == Permission::InputMonitoring && grant == GrantState::NotGranted;
    let mut steps = Vec::with_capacity(2);
    if undetermined_input_monitoring || !(asked || lost || denied) {
        steps.push(step(
            WalkbackAction::RequestFromSystem,
            format!("Click Ask now, then click {alert_button} in the message from macOS, and turn on {entry} under {pane}."),
        ));
    }
    let lead = if lost {
        format!("{entry} no longer has the {pane} permission. If you turned it off, nothing else is needed. ")
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
    steps.push(step(
        WalkbackAction::OpenSettingsPane,
        format!(
            "{lead}Open System Settings > Privacy & Security > {pane} and turn on {entry}.{relist}"
        ),
    ));
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
    fn the_three_settings_urls_and_floor_pane_titles_are_these_literals() {
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
        let floor = Some(MACOS_FLOOR_MAJOR);
        assert_eq!(
            Permission::ScreenAndSystemAudioRecording.pane_title_on_macos(floor),
            "Screen & System Audio Recording"
        );
        assert_eq!(
            Permission::Accessibility.pane_title_on_macos(floor),
            "Accessibility"
        );
        assert_eq!(
            Permission::InputMonitoring.pane_title_on_macos(floor),
            "Input Monitoring"
        );
    }

    #[test]
    fn the_required_list_is_the_three_macos_permissions_on_macos_and_empty_elsewhere() {
        let required = Permission::required_on_this_platform();
        if cfg!(target_os = "macos") {
            assert_eq!(required, Permission::R1_MACOS);
        } else {
            assert!(required.is_empty());
        }
        let names: Vec<&str> = Permission::R1_MACOS
            .iter()
            .map(|p| p.pane_title_on_macos(None))
            .collect();
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
    fn the_pane_titles_of_every_supported_major_are_pinned_from_the_floor_up() {
        assert_eq!(MACOS_FLOOR_MAJOR, 14);
        assert_eq!(MACOS_ROWS.map(|row| row.major), [14, 15, 26, 27]);
        let before_27 = [
            "Screen & System Audio Recording",
            "Accessibility",
            "Input Monitoring",
        ];
        let from_27 = [
            "Screen & System Audio Recording",
            "Device Control and Data Access",
            "Input Monitoring",
        ];
        for (major, titles) in [
            (14, before_27),
            (15, before_27),
            (26, before_27),
            (27, from_27),
        ] {
            let got = Permission::R1_MACOS.map(|p| p.pane_title_on_macos(Some(major)));
            assert_eq!(got, titles, "{major}");
        }
        for row in MACOS_ROWS {
            assert_eq!(
                [
                    row.screen_pane,
                    row.accessibility_pane,
                    row.input_monitoring_pane
                ],
                Permission::R1_MACOS.map(|p| p.pane_title_on_macos(Some(row.major)))
            );
        }
        assert_eq!(
            Permission::R1_MACOS.map(|p| p.pane_title_on_macos(Some(28))),
            from_27
        );
        assert_eq!(
            Permission::R1_MACOS.map(|p| p.pane_title_on_macos(None)),
            before_27
        );
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
            assert_eq!(
                p.pane_title_on_macos(Some(27)),
                p.pane_title_on_macos(Some(26))
            );
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

    fn step_text(
        input: &AssessInput<'_>,
        permission: Permission,
        action: WalkbackAction,
    ) -> String {
        assess(input)
            .walkback
            .into_iter()
            .find(|s| s.permission == permission && s.action == action)
            .map(|s| s.text)
            .unwrap_or_default()
    }

    #[test]
    fn no_step_text_names_a_button_the_alert_lacks_or_doubles_a_word() {
        every_state(|_, state| {
            for step in &state.walkback {
                assert!(!step.text.contains("Allow"), "{}", step.text);
                assert!(!step.text.contains("Restart recorder"), "{}", step.text);
                assert!(!step.text.contains("Access access"), "{}", step.text);
                assert!(!step.text.contains("access access"), "{}", step.text);
            }
        });
    }

    #[test]
    fn a_permission_already_asked_about_gets_no_request_step() {
        every_state(|input, state| {
            for step in &state.walkback {
                if step.action == WalkbackAction::RequestFromSystem {
                    let p = step.permission;
                    let undetermined_input_monitoring =
                        p == InputMonitoring && input.now.of(p) == GrantState::NotDetermined;
                    assert!(
                        undetermined_input_monitoring || !input.asked.contains(p),
                        "{p:?}"
                    );
                }
            }
        });
    }

    #[test]
    fn a_lost_or_denied_permission_starts_at_the_settings_step_unless_input_monitoring_is_undetermined(
    ) {
        let first_action =
            |input: &AssessInput<'_>| assess(input).walkback.first().map(|s| s.action);
        let mut lost = input(&[Accessibility], &[(Accessibility, GrantState::NotGranted)]);
        lost.once_granted = [Accessibility].into_iter().collect();
        assert_eq!(first_action(&lost), Some(WalkbackAction::OpenSettingsPane));
        let denied = input(
            &[InputMonitoring],
            &[(InputMonitoring, GrantState::NotGranted)],
        );
        assert_eq!(
            first_action(&denied),
            Some(WalkbackAction::OpenSettingsPane)
        );
        let mut asked = input(&[Screen], &[(Screen, GrantState::NotGranted)]);
        asked.asked = [Screen].into_iter().collect();
        assert_eq!(first_action(&asked), Some(WalkbackAction::OpenSettingsPane));
        let mut undetermined = input(
            &[InputMonitoring],
            &[(InputMonitoring, GrantState::NotDetermined)],
        );
        undetermined.asked = [InputMonitoring].into_iter().collect();
        undetermined.once_granted = [InputMonitoring].into_iter().collect();
        assert_eq!(
            first_action(&undetermined),
            Some(WalkbackAction::RequestFromSystem)
        );
        let fresh = input(
            &[Screen, Accessibility],
            &[(Screen, GrantState::NotGranted)],
        );
        let state = assess(&fresh);
        assert_eq!(
            state.walkback.first().map(|s| (s.permission, s.action)),
            Some((Screen, WalkbackAction::RequestFromSystem))
        );
        assert_eq!(state.permissions[1].grant, GrantState::NotDetermined);
    }

    #[test]
    fn the_request_step_names_the_alert_button_of_each_supported_major() {
        assert_eq!(MACOS_ROWS.map(|row| row.major), [14, 15, 26, 27]);
        for row in MACOS_ROWS {
            assert_eq!(row.alert_button, "Open System Settings", "{}", row.major);
            let mut input = input(&[InputMonitoring], &[]);
            input.macos_major = Some(row.major);
            let text = step_text(&input, InputMonitoring, WalkbackAction::RequestFromSystem);
            assert!(text.contains("click Open System Settings"), "{text}");
        }
    }

    #[test]
    fn the_entry_name_is_an_input() {
        let mut input = input(&[Accessibility], &[]);
        input.entry_name = "Halvo Dev";
        for action in [
            WalkbackAction::RequestFromSystem,
            WalkbackAction::OpenSettingsPane,
        ] {
            let text = step_text(&input, Accessibility, action);
            assert!(text.contains("turn on Halvo Dev"), "{text}");
        }
    }

    #[test]
    fn every_step_text_is_pinned_by_a_literal() {
        let screen = "Screen & System Audio Recording";
        let relist = " If Halvo is already listed and still not working, remove it with the minus button and add it again.";
        let request = |pane: &str| {
            format!("Click Ask now, then click Open System Settings in the message from macOS, and turn on Halvo under {pane}.")
        };
        let settings = |pane: &str, relist: &str| {
            format!("Open System Settings > Privacy & Security > {pane} and turn on Halvo.{relist}")
        };
        let lost = |pane: &str| {
            format!("Halvo no longer has the {pane} permission. If you turned it off, nothing else is needed. ")
        };
        let restart = |pane: &str| {
            format!("Halvo restarts its recorder so that macOS applies the {pane} permission.")
        };
        let cases = [
            (Screen, 26, screen, ""),
            (Accessibility, 26, "Accessibility", relist),
            (Accessibility, 27, "Device Control and Data Access", relist),
            (InputMonitoring, 27, "Input Monitoring", relist),
        ];
        for (permission, major, pane, relist) in cases {
            let required = [permission];
            let mut fresh = input(&required, &[]);
            fresh.macos_major = Some(major);
            assert_eq!(
                step_text(&fresh, permission, WalkbackAction::RequestFromSystem),
                request(pane)
            );
            assert_eq!(
                step_text(&fresh, permission, WalkbackAction::OpenSettingsPane),
                settings(pane, relist)
            );
            let mut gone = fresh;
            gone.once_granted = [permission].into_iter().collect();
            assert_eq!(
                step_text(&gone, permission, WalkbackAction::OpenSettingsPane),
                lost(pane) + &settings(pane, relist)
            );
            let expected_restart = if permission.restarts_capture() {
                restart(pane)
            } else {
                String::new()
            };
            assert_eq!(
                step_text(&fresh, permission, WalkbackAction::RestartCapture),
                expected_restart
            );
        }
    }

    #[test]
    fn a_lost_permission_is_not_blamed_on_macos() {
        every_state(|_, state| {
            for step in &state.walkback {
                assert!(!step.text.contains("macOS removed"), "{}", step.text);
            }
        });
    }

    #[test]
    fn an_acknowledged_lost_permission_is_no_longer_lost_and_still_not_granted() {
        let mut input = input(&[Screen], &[(Screen, GrantState::NotGranted)]);
        input.once_granted = [Screen, InputMonitoring].into_iter().collect();
        let state = assess(&input);
        assert_eq!(state.lost_grants, vec![Screen]);
        assert!(state.permissions[0].lost);
        input.once_granted.remove(Screen);
        assert_eq!(
            input.once_granted.iter().collect::<Vec<_>>(),
            vec![InputMonitoring]
        );
        let state = assess(&input);
        assert!(state.lost_grants.is_empty());
        assert!(!state.permissions[0].lost);
        assert_eq!(state.permissions[0].grant, GrantState::NotGranted);
        assert!(!state.all_granted);
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
