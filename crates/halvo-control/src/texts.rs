use serde::Serialize;

use crate::model::{Modality, StorageCap};
use crate::onboarding::{Permission, PermissionStatus};
use crate::proto::{
    source_of, Cap, Custody, DaemonStatus, OnOff, PasswordGuard, Platform, SourceLine, SourceName,
    SourceState, SwitchLine, SwitchName,
};

pub const SCREEN_ACTIVITY_NOT_RECORDED: &str = "Screen activity is not recorded.";

pub const TYPING_NOT_OBSERVED_ON_WINDOWS: &str =
    "Typing is not observed on Windows in this version.";

/// In place of [`TYPING_NOT_OBSERVED_ON_WINDOWS`] while the Windows keyboard source line runs
/// (reads running or impaired).
pub const TYPING_BEING_OBSERVED: &str = "Typing is being observed. Pause capture to stop it.";

/// In place of [`SCREEN_ACTIVITY_NOT_RECORDED`] while the screen source line runs (reads running
/// or impaired) and the switch screen_activity is off.
pub const SCREEN_ACTIVITY_RECORDED_WITH_THE_SETTING_OFF: &str =
    "Screen activity is being recorded although the setting is off. Pause capture to stop it.";

/// In place of [`TYPING_NOT_OBSERVED_ON_WINDOWS`] while the Windows keyboard source line reads
/// failed, carries the mark `unstopped` beside stopped or off, or is missing.
pub const TYPING_MAY_BE_OBSERVED: &str = "Typing may still be observed: Halvo's capture service reports a fault in that part. To be sure it is off, pause capture or quit Halvo.";

/// In place of [`SCREEN_ACTIVITY_NOT_RECORDED`] while screen_activity is off and the screen source
/// line reads failed, carries the mark `unstopped` beside stopped or off, or is missing.
pub const SCREEN_ACTIVITY_MAY_BE_RECORDED_WITH_THE_SETTING_OFF: &str = "Screen activity may still be recorded although the setting is off: Halvo's capture service reports a fault in that part. To be sure it is off, pause capture or quit Halvo.";

pub const PASSWORDS_NOT_CONFIRMED: &str = "Halvo cannot confirm that it leaves out password fields. While typing is observed, Halvo may note each key that you press in a password field, never which key. Pause capture before you type a password.";

pub const KEY_TYPED_AHEAD_CAN_BE_NOTED: &str = "A key that you press before a program or page has reacted, for example while it is frozen or still loading, can be noted even if it ends up in a password field: when it was pressed, in which app and what kind of key, never which letter or digit. To be sure, pause capture before you type a password.";

/// What a Mac's page says while its status reads keys_withheld yes: an item and no warning
/// (ruling PR-30 PR10). The team's default, in the owner's table of sentences and the owner's
/// to change; it says no more than [`KEY_TYPED_AHEAD_CAN_BE_NOTED`] does (MA-12 H5).
pub const KEYS_WITHHELD_ON_A_MAC: &str = "Halvo left out some of the keys that you pressed, because they may have gone into a password field.";

const PASSWORDS_WITH_SECURE_INPUT: &str = "While a password field holds secure input, Halvo records no keystroke; a terminal that does not ask for secure input gets no such protection.";

const PASSWORDS_WITH_NO_TYPING_ON_WINDOWS: &str = "Halvo observes no typing on Windows in this version, so it records nothing that you type into a password field.";

const PASSWORDS_WITH_HALVO_S_DETECTOR: &str = "While a password field has the focus, Halvo records no keystroke; to find the field, Halvo reads the controls of the window in front through UI Automation, which uses some memory in the browser. While Halvo records typing on Windows, Windows hands Halvo every mouse event, also when pointer recording is off. Halvo looks at one thing in it, whether a button was pressed and when, and uses that to stop recording keys the moment you click into a password field. It does not look at where the pointer is or how it moves, and it stores nothing of the mouse.";

pub const UNSIGNED_BUILD_CUSTODY: &str = "This is an unsigned build: the key that protects your data is kept in a file with owner-only permissions in your account.";

pub const WINDOWS_CAPTURE_BORDER: &str =
    "While Halvo notes screen activity, Windows draws a border around the screen.";

pub const LIMITS: &str = "A copy of Halvo's data made earlier, such as a backup, keeps what it held: expiry does not reach it, and after a delete-all a copy of the whole data folder can still be read.";

/// What Halvo records while `switch` is on.
pub const fn recorded_when_on(switch: SwitchName) -> &'static str {
    match switch {
        SwitchName::TitleText => "Halvo records the title text of the window in front; the title of a private window is not stored where Halvo can tell the window is private.",
        SwitchName::KeyClass => "Halvo records a class for each key you press, never the character.",
        SwitchName::PointerClass => "Halvo records a class for each click or scroll, never its position.",
        SwitchName::ScreenActivity => "Halvo notes screen activity, a record that the screen changed, and stores no image.",
        SwitchName::SiteRead => "Halvo records the site of the page in front in a browser, never the page's address or content.",
    }
}

/// One sentence of `recorded`, keyed by the switch that it is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Recorded {
    pub switch: SwitchName,
    pub text: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceClass {
    Off,
    Runs,
    Stopped,
    MayRun,
}

fn class(line: &SourceLine) -> SourceClass {
    match (&line.state, line.unstopped) {
        (SourceState::Running | SourceState::Impaired { .. }, _) => SourceClass::Runs,
        (SourceState::Failed { .. }, _)
        | (SourceState::Stopped { .. } | SourceState::Off, true) => SourceClass::MayRun,
        (SourceState::Stopped { .. }, false) => SourceClass::Stopped,
        (SourceState::Off, false) => SourceClass::Off,
    }
}

const CLASS_OF_A_MISSING_LINE: SourceClass = SourceClass::MayRun;

fn class_of(sources: &[SourceLine], name: SourceName) -> SourceClass {
    sources
        .iter()
        .find(|s| s.name == name)
        .map_or(CLASS_OF_A_MISSING_LINE, class)
}

fn recorded(platform: Platform, switches: &[SwitchLine], sources: &[SourceLine]) -> Vec<Recorded> {
    let mut out = Vec::new();
    for line in switches {
        let on = line.value == OnOff::On;
        let text = match (platform, line.name, on) {
            (Platform::Windows, SwitchName::KeyClass, _) => {
                match class_of(sources, SourceName::Keyboard) {
                    SourceClass::Off => Some(TYPING_NOT_OBSERVED_ON_WINDOWS),
                    SourceClass::Runs => Some(TYPING_BEING_OBSERVED),
                    SourceClass::Stopped => None,
                    SourceClass::MayRun => Some(TYPING_MAY_BE_OBSERVED),
                }
            }
            (_, SwitchName::ScreenActivity, false) => match class_of(sources, SourceName::Screen) {
                SourceClass::Off => Some(SCREEN_ACTIVITY_NOT_RECORDED),
                SourceClass::Runs => Some(SCREEN_ACTIVITY_RECORDED_WITH_THE_SETTING_OFF),
                SourceClass::Stopped => None,
                SourceClass::MayRun => Some(SCREEN_ACTIVITY_MAY_BE_RECORDED_WITH_THE_SETTING_OFF),
            },
            (_, name, true) => Some(recorded_when_on(name)),
            (_, _, false) => None,
        };
        if let Some(text) = text {
            out.push(Recorded {
                switch: line.name,
                text,
            });
        }
        if platform == Platform::Windows && line.name == SwitchName::ScreenActivity && on {
            out.push(Recorded {
                switch: line.name,
                text: WINDOWS_CAPTURE_BORDER,
            });
        }
    }
    out
}

fn passwords(platform: Platform, guard: PasswordGuard, typing: SourceClass) -> &'static str {
    match (platform, guard, typing) {
        (Platform::Windows, _, SourceClass::Off) => PASSWORDS_WITH_NO_TYPING_ON_WINDOWS,
        (Platform::Macos, PasswordGuard::Os, _) => PASSWORDS_WITH_SECURE_INPUT,
        (Platform::Windows, PasswordGuard::Halvo, _) => PASSWORDS_WITH_HALVO_S_DETECTOR,
        _ => PASSWORDS_NOT_CONFIRMED,
    }
}

pub const fn backup_limit(platform: Platform) -> Option<&'static str> {
    match platform {
        Platform::Macos => None,
        Platform::Windows => Some("Windows backups and restore points can keep a copy of Halvo's data that deleting it in Halvo does not remove."),
    }
}

/// The label of the key custody; none claims that an administrator cannot read the key.
pub const fn custody_text(custody: Custody) -> &'static str {
    match custody {
        Custody::RootFile => UNSIGNED_BUILD_CUSTODY,
        Custody::Dpapi => "Windows protects the key that protects your data with your account.",
        Custody::SecureEnclave => {
            "The key that protects your data is kept in this Mac's Secure Enclave."
        }
    }
}

/// The cap sentence, in GB of 1,073,741,824 bytes with at most one decimal.
pub fn storage(cap: Cap) -> String {
    match cap {
        Cap::Known(cap) => {
            let tenths = (cap.bytes() * 10 + StorageCap::GIB / 2) / StorageCap::GIB;
            let size = if tenths.is_multiple_of(10) {
                format!("{}", tenths / 10)
            } else {
                format!("{}.{}", tenths / 10, tenths % 10)
            };
            format!("Halvo keeps at most {size} GB and removes the oldest days first when it reaches that.")
        }
        Cap::Unknown => "Halvo could not read its storage limit and removes nothing to make room until you set one.".to_string(),
    }
}

/// The table of the view (contracts.md C10.2 `texts`) for one status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Texts {
    pub recorded: Vec<Recorded>,
    pub passwords: &'static str,
    /// [`KEY_TYPED_AHEAD_CAN_BE_NOTED`] wherever `passwords` is the macOS secure-input sentence,
    /// and no key elsewhere.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub typed_ahead: Option<&'static str>,
    /// [`KEYS_WITHHELD_ON_A_MAC`] on a Mac whose status reads keys_withheld yes, and no key
    /// elsewhere.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keys_withheld: Option<&'static str>,
    pub backup_limit: Option<&'static str>,
    pub limits: &'static str,
    pub custody: &'static str,
    pub storage: String,
}

pub fn texts(status: &DaemonStatus) -> Texts {
    let f = status.fields();
    let typing = source_of(Modality::Keystroke, f.platform)
        .map_or(CLASS_OF_A_MISSING_LINE, |source| {
            class_of(&f.sources, source)
        });
    let passwords = passwords(f.platform, f.password_guard, typing);
    Texts {
        recorded: recorded(f.platform, &f.switches, &f.sources),
        passwords,
        typed_ahead: (passwords == PASSWORDS_WITH_SECURE_INPUT)
            .then_some(KEY_TYPED_AHEAD_CAN_BE_NOTED),
        keys_withheld: (f.platform == Platform::Macos && f.keys_withheld)
            .then_some(KEYS_WITHHELD_ON_A_MAC),
        backup_limit: backup_limit(f.platform),
        limits: LIMITS,
        custody: custody_text(f.custody),
        storage: storage(f.cap_bytes),
    }
}

/// The onboarding rows of a status for a view: each row as the status holds it,
/// but the `why` of the screen row is [`SCREEN_ACTIVITY_NOT_RECORDED`] only while `recorded` of
/// the same status gives that sentence, and otherwise the row's sentence for the switch
/// screen_activity on.
pub fn onboarding_rows(status: &DaemonStatus) -> Vec<PermissionStatus> {
    let f = status.fields();
    let says_not_recorded = recorded(f.platform, &f.switches, &f.sources)
        .iter()
        .any(|r| r.text == SCREEN_ACTIVITY_NOT_RECORDED);
    f.onboarding
        .permissions
        .iter()
        .cloned()
        .map(|mut row| {
            if row.permission == Permission::ScreenAndSystemAudioRecording && !says_not_recorded {
                row.why = Permission::ScreenAndSystemAudioRecording
                    .why(true)
                    .to_string();
            }
            row
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lf(checked_out: &str) -> String {
        checked_out.replace("\r\n", "\n")
    }
    use crate::model::StorageCap;
    use crate::proto::{Cap, Custody, OnOff, PasswordGuard, Platform, SwitchLine, SwitchName};
    use crate::testing::sample_status;

    const STATUS_MACOS: &str =
        include_str!("../../../scripts/r1-gate/fixtures/c2-status-macos.txt");
    const STATUS_WINDOWS: &str =
        include_str!("../../../scripts/r1-gate/fixtures/c2-status-windows.txt");

    #[test]
    fn pr10_withheld_keys_on_a_mac_are_an_item_of_the_texts_that_is_no_warning_and_windows_shows_none(
    ) {
        let withheld = |platform: Platform, yes: bool| {
            let mut f = sample_status(platform).into_fields();
            f.keys_withheld = yes;
            texts(&DaemonStatus::new(f).unwrap())
        };
        assert_eq!(
            withheld(Platform::Macos, true).keys_withheld,
            Some(KEYS_WITHHELD_ON_A_MAC)
        );
        assert_eq!(
            KEYS_WITHHELD_ON_A_MAC,
            "Halvo left out some of the keys that you pressed, because they may have gone into a password field."
        );
        for (platform, yes) in [
            (Platform::Macos, false),
            (Platform::Windows, true),
            (Platform::Windows, false),
        ] {
            let shown = withheld(platform, yes);
            assert_eq!(shown.keys_withheld, None, "{platform:?} {yes}");
            let json = serde_json::to_value(&shown).unwrap();
            assert!(json.get("keys_withheld").is_none(), "{platform:?} {yes}");
        }
        let shown = withheld(Platform::Macos, true);
        assert_eq!(
            shown.passwords,
            texts(&sample_status(Platform::Macos)).passwords
        );
        assert_eq!(shown.typed_ahead, Some(KEY_TYPED_AHEAD_CAN_BE_NOTED));
        for warning in ["may still", "fault", "Pause capture", "cannot confirm"] {
            assert!(!KEYS_WITHHELD_ON_A_MAC.contains(warning), "{warning}");
        }
    }

    fn switches(on: &[SwitchName]) -> Vec<SwitchLine> {
        SwitchName::ALL
            .iter()
            .map(|&name| SwitchLine {
                name,
                value: if on.contains(&name) {
                    OnOff::On
                } else {
                    OnOff::Off
                },
            })
            .collect()
    }

    #[test]
    fn g1_the_view_says_screen_activity_is_recorded_while_the_screen_source_runs_with_its_switch_off(
    ) {
        use crate::proto::{SourceName, SourceState};
        let mut f = sample_status(Platform::Macos).into_fields();
        let screen = f
            .sources
            .iter()
            .position(|s| s.name == SourceName::Screen)
            .unwrap();
        f.sources[screen].state = SourceState::Running;
        f.enabled_sources = vec![
            SourceName::Foreground,
            SourceName::Screen,
            SourceName::Input,
        ];
        let status = DaemonStatus::new(f).unwrap();
        let told = texts_of(&texts(&status).recorded);
        assert!(!told.contains(&SCREEN_ACTIVITY_NOT_RECORDED), "{told:?}");
        assert!(
            told.contains(
                &"Screen activity is being recorded although the setting is off. Pause capture to stop it."
            ),
            "{told:?}"
        );
    }

    #[test]
    fn an_impaired_source_line_counts_as_running_in_recorded() {
        use crate::proto::{Reason, SourceLine, SourceName, SourceState};
        let impaired = SourceState::Impaired {
            reason: Reason::Permission,
            error: None,
        };
        let sources = [
            SourceLine {
                name: SourceName::Keyboard,
                state: impaired.clone(),
                unstopped: false,
            },
            SourceLine {
                name: SourceName::Screen,
                state: impaired,
                unstopped: false,
            },
        ];
        let told = texts_of(&recorded(Platform::Windows, &switches(&[]), &sources));
        assert!(told.contains(&TYPING_BEING_OBSERVED), "{told:?}");
        assert!(
            told.contains(&SCREEN_ACTIVITY_RECORDED_WITH_THE_SETTING_OFF),
            "{told:?}"
        );
        assert!(!told.contains(&TYPING_NOT_OBSERVED_ON_WINDOWS), "{told:?}");
        assert!(!told.contains(&SCREEN_ACTIVITY_NOT_RECORDED), "{told:?}");
    }

    #[test]
    fn g2_the_view_says_typing_is_observed_while_the_windows_keyboard_source_runs() {
        use crate::proto::{SourceName, SourceState};
        let mut f = sample_status(Platform::Windows).into_fields();
        let keyboard = f
            .sources
            .iter()
            .position(|s| s.name == SourceName::Keyboard)
            .unwrap();
        f.sources[keyboard].state = SourceState::Running;
        f.enabled_sources = vec![
            SourceName::Foreground,
            SourceName::Keyboard,
            SourceName::Pointer,
            SourceName::WindowTitle,
        ];
        let status = DaemonStatus::new(f).unwrap();
        let told = texts_of(&texts(&status).recorded);
        assert!(!told.contains(&TYPING_NOT_OBSERVED_ON_WINDOWS), "{told:?}");
        assert!(
            told.contains(&"Typing is being observed. Pause capture to stop it."),
            "{told:?}"
        );
    }

    fn texts_of(recorded: &[Recorded]) -> Vec<&'static str> {
        recorded.iter().map(|r| r.text).collect()
    }

    #[test]
    fn the_three_sentences_of_the_first_run_text_are_these_literals() {
        assert_eq!(
            SCREEN_ACTIVITY_NOT_RECORDED,
            "Screen activity is not recorded."
        );
        assert_eq!(
            TYPING_NOT_OBSERVED_ON_WINDOWS,
            "Typing is not observed on Windows in this version."
        );
        assert_eq!(
            UNSIGNED_BUILD_CUSTODY,
            "This is an unsigned build: the key that protects your data is kept in a file with owner-only permissions in your account."
        );
        assert_eq!(
            crate::onboarding::SCREEN_ACTIVITY_NOT_RECORDED,
            SCREEN_ACTIVITY_NOT_RECORDED
        );
    }

    #[test]
    fn each_switch_has_its_sentence_for_the_on_state_as_these_literals() {
        let expected = [
            (
                SwitchName::TitleText,
                "Halvo records the title text of the window in front; the title of a private window is not stored where Halvo can tell the window is private.",
            ),
            (
                SwitchName::KeyClass,
                "Halvo records a class for each key you press, never the character.",
            ),
            (
                SwitchName::PointerClass,
                "Halvo records a class for each click or scroll, never its position.",
            ),
            (
                SwitchName::ScreenActivity,
                "Halvo notes screen activity, a record that the screen changed, and stores no image.",
            ),
            (
                SwitchName::SiteRead,
                "Halvo records the site of the page in front in a browser, never the page's address or content.",
            ),
        ];
        assert_eq!(expected.len(), SwitchName::ALL.len());
        for (name, text) in expected {
            assert_eq!(recorded_when_on(name), text, "{name:?}");
        }
        assert_eq!(
            WINDOWS_CAPTURE_BORDER,
            "While Halvo notes screen activity, Windows draws a border around the screen."
        );
    }

    #[test]
    fn every_switch_of_gate_2_s_status_fixtures_has_a_sentence_for_its_on_state() {
        for text in [lf(STATUS_MACOS), lf(STATUS_WINDOWS)] {
            let names: Vec<&str> = text
                .lines()
                .filter_map(|l| l.strip_prefix("switch name="))
                .filter_map(|l| l.split(' ').next())
                .collect();
            assert_eq!(names.len(), 5);
            for name in names {
                let switch: SwitchName = name.parse().unwrap();
                assert!(!recorded_when_on(switch).is_empty(), "{name}");
            }
        }
    }

    #[test]
    fn a_default_status_on_macos_lists_the_three_classes_and_says_screen_activity_is_not_recorded()
    {
        let recorded = recorded(
            Platform::Macos,
            &sample_status(Platform::Macos).fields().switches,
            &sample_status(Platform::Macos).fields().sources,
        );
        assert_eq!(
            texts_of(&recorded),
            [
                recorded_when_on(SwitchName::TitleText),
                recorded_when_on(SwitchName::KeyClass),
                recorded_when_on(SwitchName::PointerClass),
                SCREEN_ACTIVITY_NOT_RECORDED,
            ]
        );
        let keys: Vec<SwitchName> = recorded.iter().map(|r| r.switch).collect();
        assert_eq!(
            keys,
            [
                SwitchName::TitleText,
                SwitchName::KeyClass,
                SwitchName::PointerClass,
                SwitchName::ScreenActivity
            ]
        );
    }

    #[test]
    fn a_default_status_on_windows_says_typing_is_not_observed_and_names_no_border() {
        let recorded = recorded(
            Platform::Windows,
            &sample_status(Platform::Windows).fields().switches,
            &sample_status(Platform::Windows).fields().sources,
        );
        assert_eq!(
            texts_of(&recorded),
            [
                recorded_when_on(SwitchName::TitleText),
                TYPING_NOT_OBSERVED_ON_WINDOWS,
                recorded_when_on(SwitchName::PointerClass),
                SCREEN_ACTIVITY_NOT_RECORDED,
            ]
        );
        assert!(!texts_of(&recorded).contains(&WINDOWS_CAPTURE_BORDER));
    }

    #[test]
    fn the_windows_border_sentence_shows_only_while_screen_activity_is_on() {
        let on = recorded(
            Platform::Windows,
            &switches(&[SwitchName::ScreenActivity]),
            &sample_status(Platform::Windows).fields().sources,
        );
        assert_eq!(
            texts_of(&on),
            [
                TYPING_NOT_OBSERVED_ON_WINDOWS,
                recorded_when_on(SwitchName::ScreenActivity),
                WINDOWS_CAPTURE_BORDER
            ]
        );
        let mac = recorded(
            Platform::Macos,
            &switches(&[SwitchName::ScreenActivity]),
            &[],
        );
        assert_eq!(
            texts_of(&mac),
            [recorded_when_on(SwitchName::ScreenActivity)]
        );
        let all = recorded(Platform::Macos, &switches(SwitchName::ALL), &[]);
        assert_eq!(all.len(), 5);
        assert!(!texts_of(&all).contains(&SCREEN_ACTIVITY_NOT_RECORDED));
    }

    #[test]
    fn no_sentence_names_a_stored_image_a_key_s_character_or_a_pointer_position_as_recorded() {
        let mut sentences: Vec<&str> = SwitchName::ALL
            .iter()
            .map(|&s| recorded_when_on(s))
            .collect();
        sentences.extend([
            SCREEN_ACTIVITY_NOT_RECORDED,
            TYPING_NOT_OBSERVED_ON_WINDOWS,
            TYPING_BEING_OBSERVED,
            TYPING_MAY_BE_OBSERVED,
            SCREEN_ACTIVITY_RECORDED_WITH_THE_SETTING_OFF,
            SCREEN_ACTIVITY_MAY_BE_RECORDED_WITH_THE_SETTING_OFF,
            KEY_TYPED_AHEAD_CAN_BE_NOTED,
            WINDOWS_CAPTURE_BORDER,
            LIMITS,
            UNSIGNED_BUILD_CUSTODY,
        ]);
        for platform in [Platform::Macos, Platform::Windows] {
            for &guard in PasswordGuard::ALL {
                for typing in [
                    SourceClass::Off,
                    SourceClass::Runs,
                    SourceClass::Stopped,
                    SourceClass::MayRun,
                ] {
                    sentences.push(passwords(platform, guard, typing));
                }
            }
            sentences.extend(backup_limit(platform));
        }
        for &custody in Custody::ALL {
            sentences.push(custody_text(custody));
        }
        for sentence in sentences {
            for word in ["image", "character", "position", "address", "content"] {
                for (at, _) in sentence.match_indices(word) {
                    let before = &sentence[..at];
                    assert!(
                        [
                            "no ",
                            "never the ",
                            "never its ",
                            "never the page's ",
                            "never the page's address or "
                        ]
                        .iter()
                        .any(|lead| before.ends_with(lead)),
                        "{sentence}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_password_sentences_are_these_literals_for_each_guard_and_system() {
        assert_eq!(
            passwords(Platform::Macos, PasswordGuard::Os, SourceClass::Runs),
            "While a password field holds secure input, Halvo records no keystroke; a terminal that does not ask for secure input gets no such protection."
        );
        assert_eq!(
            passwords(Platform::Windows, PasswordGuard::None, SourceClass::Off),
            "Halvo observes no typing on Windows in this version, so it records nothing that you type into a password field."
        );
        assert_eq!(
            passwords(Platform::Windows, PasswordGuard::Halvo, SourceClass::Runs),
            "While a password field has the focus, Halvo records no keystroke; to find the field, Halvo reads the controls of the window in front through UI Automation, which uses some memory in the browser. While Halvo records typing on Windows, Windows hands Halvo every mouse event, also when pointer recording is off. Halvo looks at one thing in it, whether a button was pressed and when, and uses that to stop recording keys the moment you click into a password field. It does not look at where the pointer is or how it moves, and it stores nothing of the mouse."
        );
        assert_eq!(
            passwords(Platform::Windows, PasswordGuard::None, SourceClass::Runs),
            "Halvo cannot confirm that it leaves out password fields. While typing is observed, Halvo may note each key that you press in a password field, never which key. Pause capture before you type a password."
        );
        assert_eq!(
            KEY_TYPED_AHEAD_CAN_BE_NOTED,
            "A key that you press before a program or page has reacted, for example while it is frozen or still loading, can be noted even if it ends up in a password field: when it was pressed, in which app and what kind of key, never which letter or digit. To be sure, pause capture before you type a password."
        );
        for platform in [Platform::Macos, Platform::Windows] {
            for &guard in PasswordGuard::ALL {
                for typing in [
                    SourceClass::Off,
                    SourceClass::Runs,
                    SourceClass::Stopped,
                    SourceClass::MayRun,
                ] {
                    assert!(passwords(platform, guard, typing).ends_with('.'));
                }
            }
        }
    }

    #[test]
    fn the_backup_sentence_is_the_windows_one_and_none_on_macos() {
        assert_eq!(backup_limit(Platform::Macos), None);
        assert_eq!(
            backup_limit(Platform::Windows),
            Some("Windows backups and restore points can keep a copy of Halvo's data that deleting it in Halvo does not remove.")
        );
    }

    #[test]
    fn the_limits_sentence_is_this_literal_on_both_systems() {
        assert_eq!(
            LIMITS,
            "A copy of Halvo's data made earlier, such as a backup, keeps what it held: expiry does not reach it, and after a delete-all a copy of the whole data folder can still be read."
        );
        assert_eq!(texts(&sample_status(Platform::Macos)).limits, LIMITS);
        assert_eq!(texts(&sample_status(Platform::Windows)).limits, LIMITS);
    }

    #[test]
    fn the_custody_sentences_label_the_unsigned_build_and_claim_nothing_of_an_administrator() {
        assert_eq!(custody_text(Custody::RootFile), UNSIGNED_BUILD_CUSTODY);
        assert_eq!(
            custody_text(Custody::Dpapi),
            "Windows protects the key that protects your data with your account."
        );
        assert_eq!(
            custody_text(Custody::SecureEnclave),
            "The key that protects your data is kept in this Mac's Secure Enclave."
        );
        for &custody in Custody::ALL {
            assert!(!custody_text(custody)
                .to_lowercase()
                .contains("administrator"));
        }
    }

    #[test]
    fn the_storage_sentence_names_the_cap_or_says_it_is_unknown() {
        assert_eq!(
            storage(Cap::Known(StorageCap::DEFAULT)),
            "Halvo keeps at most 25 GB and removes the oldest days first when it reaches that."
        );
        assert_eq!(
            storage(Cap::Known(StorageCap::MIN)),
            "Halvo keeps at most 1 GB and removes the oldest days first when it reaches that."
        );
        assert_eq!(
            storage(Cap::Known(StorageCap::from_bytes(1_610_612_736).unwrap())),
            "Halvo keeps at most 1.5 GB and removes the oldest days first when it reaches that."
        );
        assert_eq!(
            storage(Cap::Unknown),
            "Halvo could not read its storage limit and removes nothing to make room until you set one."
        );
    }

    #[test]
    fn the_table_of_a_status_has_the_six_keys_of_the_view() {
        let json = serde_json::to_value(texts(&sample_status(Platform::Windows))).unwrap();
        let keys: Vec<&str> = json
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        let mut want = [
            "recorded",
            "passwords",
            "backup_limit",
            "limits",
            "custody",
            "storage",
        ];
        want.sort_unstable();
        let mut got = keys.clone();
        got.sort_unstable();
        assert_eq!(got, want);
        assert_eq!(json["recorded"][1]["switch"], "key_class");
        assert_eq!(json["recorded"][1]["text"], TYPING_NOT_OBSERVED_ON_WINDOWS);
        let mac = serde_json::to_value(texts(&sample_status(Platform::Macos))).unwrap();
        assert_eq!(mac["backup_limit"], serde_json::Value::Null);
        assert_eq!(mac["custody"], UNSIGNED_BUILD_CUSTODY);
    }

    const K0: &str = "Typing is not observed on Windows in this version.";
    const K1: &str = "Typing is being observed. Pause capture to stop it.";
    const K2: &str = "Typing may still be observed: Halvo's capture service reports a fault in that part. To be sure it is off, pause capture or quit Halvo.";
    const S0: &str = "Screen activity is not recorded.";
    const S1: &str =
        "Screen activity is being recorded although the setting is off. Pause capture to stop it.";
    const S2: &str = "Screen activity may still be recorded although the setting is off: Halvo's capture service reports a fault in that part. To be sure it is off, pause capture or quit Halvo.";
    const P0: &str = "Halvo observes no typing on Windows in this version, so it records nothing that you type into a password field.";
    const PM: &str = "While a password field holds secure input, Halvo records no keystroke; a terminal that does not ask for secure input gets no such protection.";
    const PH: &str = "While a password field has the focus, Halvo records no keystroke; to find the field, Halvo reads the controls of the window in front through UI Automation, which uses some memory in the browser. While Halvo records typing on Windows, Windows hands Halvo every mouse event, also when pointer recording is off. Halvo looks at one thing in it, whether a button was pressed and when, and uses that to stop recording keys the moment you click into a password field. It does not look at where the pointer is or how it moves, and it stores nothing of the mouse.";
    const PW: &str = "Halvo cannot confirm that it leaves out password fields. While typing is observed, Halvo may note each key that you press in a password field, never which key. Pause capture before you type a password.";
    const TA: &str = "A key that you press before a program or page has reacted, for example while it is frozen or still loading, can be noted even if it ends up in a password field: when it was pressed, in which app and what kind of key, never which letter or digit. To be sure, pause capture before you type a password.";
    const TITLE_ON: &str = "Halvo records the title text of the window in front; the title of a private window is not stored where Halvo can tell the window is private.";
    const KEY_CLASS_ON: &str = "Halvo records a class for each key you press, never the character.";
    const POINTER_ON: &str = "Halvo records a class for each click or scroll, never its position.";

    struct LineRow {
        value: &'static str,
        state: crate::proto::SourceState,
        mark: bool,
        windows_keyboard_sentence: Option<&'static str>,
        screen_sentence_while_off: Option<&'static str>,
        typing_class: &'static str,
        screen_why_while_off: &'static str,
    }

    const WHY_ON: &str = "lets Halvo note screen activity, a record that the screen changed with no stored image; system audio stays off";

    fn line_rows() -> Vec<LineRow> {
        use crate::proto::{Reason, SourceState};
        let impaired = SourceState::Impaired {
            reason: Reason::Permission,
            error: None,
        };
        let failed = SourceState::Failed {
            reason: Reason::Timeout,
            error: None,
        };
        let stopped = SourceState::Stopped { by_user: false };
        let by_user = SourceState::Stopped { by_user: true };
        let row = |value,
                   state,
                   mark,
                   windows_keyboard_sentence,
                   screen_sentence_while_off,
                   typing_class,
                   screen_why_while_off| LineRow {
            value,
            state,
            mark,
            windows_keyboard_sentence,
            screen_sentence_while_off,
            typing_class,
            screen_why_while_off,
        };
        vec![
            row(
                "off",
                SourceState::Off,
                false,
                Some(K0),
                Some(S0),
                "off",
                S0,
            ),
            row(
                "off with the mark",
                SourceState::Off,
                true,
                Some(K2),
                Some(S2),
                "may run",
                WHY_ON,
            ),
            row(
                "running",
                SourceState::Running,
                false,
                Some(K1),
                Some(S1),
                "runs",
                WHY_ON,
            ),
            row(
                "running with the mark",
                SourceState::Running,
                true,
                Some(K1),
                Some(S1),
                "runs",
                WHY_ON,
            ),
            row(
                "impaired",
                impaired.clone(),
                false,
                Some(K1),
                Some(S1),
                "runs",
                WHY_ON,
            ),
            row(
                "impaired with the mark",
                impaired,
                true,
                Some(K1),
                Some(S1),
                "runs",
                WHY_ON,
            ),
            row(
                "stopped",
                stopped.clone(),
                false,
                None,
                None,
                "stopped",
                WHY_ON,
            ),
            row(
                "stopped with the mark",
                stopped,
                true,
                Some(K2),
                Some(S2),
                "may run",
                WHY_ON,
            ),
            row(
                "stopped reason=user",
                by_user.clone(),
                false,
                None,
                None,
                "stopped",
                WHY_ON,
            ),
            row(
                "stopped reason=user with the mark",
                by_user,
                true,
                Some(K2),
                Some(S2),
                "may run",
                WHY_ON,
            ),
            row(
                "failed",
                failed.clone(),
                false,
                Some(K2),
                Some(S2),
                "may run",
                WHY_ON,
            ),
            row(
                "failed with the mark",
                failed,
                true,
                Some(K2),
                Some(S2),
                "may run",
                WHY_ON,
            ),
        ]
    }

    fn status_beside(
        platform: Platform,
        source: crate::proto::SourceName,
        state: &crate::proto::SourceState,
        mark: bool,
        on: &[SwitchName],
        guard: PasswordGuard,
    ) -> DaemonStatus {
        use crate::proto::{Capture, SourceState};
        let mut f = sample_status(platform).into_fields();
        for line in &mut f.sources {
            if line.name == source {
                line.state = state.clone();
                line.unstopped = mark;
            }
        }
        f.enabled_sources = f
            .sources
            .iter()
            .filter(|l| l.state != SourceState::Off)
            .map(|l| l.name)
            .collect();
        f.switches = switches(on);
        f.password_guard = guard;
        f.password_guard_bound_ms = (guard != PasswordGuard::None).then_some(30_000);
        f.capture = Capture::Pausing;
        f.capture_reason = None;
        DaemonStatus::new(f).expect("capture pausing takes every line and the mark")
    }

    fn list(entries: &[(SwitchName, &'static str)]) -> Vec<Recorded> {
        entries
            .iter()
            .map(|&(switch, text)| Recorded { switch, text })
            .collect()
    }

    #[test]
    fn ct9_recorded_beside_each_value_of_the_keyboard_and_the_screen_line_is_the_table_of_the_ruling(
    ) {
        use crate::proto::SourceName;
        use SwitchName::{KeyClass, PointerClass, ScreenActivity, TitleText};
        let default_on = [TitleText, KeyClass, PointerClass];
        let key_class_off = [TitleText, PointerClass];
        let mut wrong = Vec::new();
        let mut rows = 0;
        for LineRow {
            value,
            state,
            mark,
            windows_keyboard_sentence,
            screen_sentence_while_off,
            ..
        } in line_rows()
        {
            let columns = [
                (
                    "Windows, key_class on, beside the keyboard line",
                    Platform::Windows,
                    SourceName::Keyboard,
                    &default_on[..],
                    list(
                        &[
                            vec![(TitleText, TITLE_ON)],
                            windows_keyboard_sentence
                                .map(|t| (KeyClass, t))
                                .into_iter()
                                .collect(),
                            vec![(PointerClass, POINTER_ON), (ScreenActivity, S0)],
                        ]
                        .concat(),
                    ),
                ),
                (
                    "Windows, key_class off, beside the keyboard line",
                    Platform::Windows,
                    SourceName::Keyboard,
                    &key_class_off[..],
                    list(
                        &[
                            vec![(TitleText, TITLE_ON)],
                            windows_keyboard_sentence
                                .map(|t| (KeyClass, t))
                                .into_iter()
                                .collect(),
                            vec![(PointerClass, POINTER_ON), (ScreenActivity, S0)],
                        ]
                        .concat(),
                    ),
                ),
                (
                    "macOS, screen_activity off, beside the screen line",
                    Platform::Macos,
                    SourceName::Screen,
                    &default_on[..],
                    list(
                        &[
                            vec![
                                (TitleText, TITLE_ON),
                                (KeyClass, KEY_CLASS_ON),
                                (PointerClass, POINTER_ON),
                            ],
                            screen_sentence_while_off
                                .map(|t| (ScreenActivity, t))
                                .into_iter()
                                .collect(),
                        ]
                        .concat(),
                    ),
                ),
                (
                    "Windows, screen_activity off, beside the screen line",
                    Platform::Windows,
                    SourceName::Screen,
                    &default_on[..],
                    list(
                        &[
                            vec![
                                (TitleText, TITLE_ON),
                                (KeyClass, K0),
                                (PointerClass, POINTER_ON),
                            ],
                            screen_sentence_while_off
                                .map(|t| (ScreenActivity, t))
                                .into_iter()
                                .collect(),
                        ]
                        .concat(),
                    ),
                ),
            ];
            for (column, platform, source, on, want) in columns {
                rows += 1;
                let guard = match platform {
                    Platform::Macos => PasswordGuard::Os,
                    Platform::Windows => PasswordGuard::None,
                };
                let status = status_beside(platform, source, &state, mark, on, guard);
                let got = texts(&status).recorded;
                if got != want {
                    wrong.push(format!(
                        "cell [{column}] x [{value}]: got {:?}, want {:?}",
                        texts_of(&got),
                        texts_of(&want)
                    ));
                }
            }
        }
        assert_eq!(rows, 48);
        assert!(
            wrong.is_empty(),
            "{} rows:\n{}",
            wrong.len(),
            wrong.join("\n")
        );
    }

    #[test]
    fn ct9_passwords_and_typed_ahead_for_each_platform_guard_and_value_of_the_typing_line_are_the_tables_of_the_rulings(
    ) {
        use crate::proto::SourceName;
        let cell = |platform: Platform, guard: PasswordGuard, class: &str| -> &'static str {
            match (platform, guard, class) {
                (Platform::Macos, PasswordGuard::Os, _) => PM,
                (Platform::Macos, _, _) => PW,
                (Platform::Windows, _, "off") => P0,
                (Platform::Windows, PasswordGuard::Halvo, _) => PH,
                (Platform::Windows, _, _) => PW,
            }
        };
        let mut wrong = Vec::new();
        let mut rows = 0;
        for (platform, typing) in [
            (Platform::Macos, SourceName::Input),
            (Platform::Windows, SourceName::Keyboard),
        ] {
            for guard in [PasswordGuard::Os, PasswordGuard::None, PasswordGuard::Halvo] {
                for LineRow {
                    value,
                    state,
                    mark,
                    typing_class,
                    ..
                } in line_rows()
                {
                    rows += 1;
                    let status = status_beside(
                        platform,
                        typing,
                        &state,
                        mark,
                        &[
                            SwitchName::TitleText,
                            SwitchName::KeyClass,
                            SwitchName::PointerClass,
                        ],
                        guard,
                    );
                    let held = status
                        .fields()
                        .sources
                        .iter()
                        .find(|l| l.name == typing)
                        .map(class)
                        .map(|c| match c {
                            SourceClass::Off => "off",
                            SourceClass::Runs => "runs",
                            SourceClass::Stopped => "stopped",
                            SourceClass::MayRun => "may run",
                        });
                    if held != Some(typing_class) {
                        wrong.push(format!(
                            "the class of the {} line [{value}]: {held:?}, the row says {typing_class:?}",
                            typing.as_str()
                        ));
                    }
                    let want = cell(platform, guard, typing_class);
                    let want_typed_ahead = (want == PM).then_some(TA);
                    let got = texts(&status);
                    if got.passwords != want {
                        wrong.push(format!(
                            "cell [{platform:?}, {guard:?}] x [{} {value}]: got {:?}",
                            typing.as_str(),
                            got.passwords
                        ));
                    }
                    if got.typed_ahead != want_typed_ahead {
                        wrong.push(format!(
                            "typed_ahead of cell [{platform:?}, {guard:?}] x [{} {value}]: got {:?}",
                            typing.as_str(),
                            got.typed_ahead
                        ));
                    }
                    let json = serde_json::to_value(&got).unwrap();
                    if json.get("typed_ahead").is_some() != want_typed_ahead.is_some() {
                        wrong.push(format!(
                            "the key typed_ahead of cell [{platform:?}, {guard:?}] x [{} {value}]: {json}",
                            typing.as_str()
                        ));
                    }
                }
            }
        }
        assert_eq!(rows, 72);
        assert!(
            wrong.is_empty(),
            "{} rows:\n{}",
            wrong.len(),
            wrong.join("\n")
        );
    }

    #[test]
    fn ct9_the_why_of_the_screen_row_says_not_recorded_only_where_recorded_says_it() {
        use crate::proto::SourceName;
        let mut wrong = Vec::new();
        let mut rows = 0;
        for (switch_on, on) in [
            (
                false,
                &[
                    SwitchName::TitleText,
                    SwitchName::KeyClass,
                    SwitchName::PointerClass,
                ][..],
            ),
            (
                true,
                &[
                    SwitchName::TitleText,
                    SwitchName::KeyClass,
                    SwitchName::PointerClass,
                    SwitchName::ScreenActivity,
                ][..],
            ),
        ] {
            for LineRow {
                value,
                state,
                mark,
                screen_why_while_off,
                ..
            } in line_rows()
            {
                rows += 1;
                let status = status_beside(
                    Platform::Macos,
                    SourceName::Screen,
                    &state,
                    mark,
                    on,
                    PasswordGuard::Os,
                );
                let want = if switch_on {
                    WHY_ON
                } else {
                    screen_why_while_off
                };
                let held = &status.fields().onboarding.permissions;
                let got = onboarding_rows(&status);
                let mut want_rows = held.clone();
                for row in &mut want_rows {
                    if row.permission == Permission::ScreenAndSystemAudioRecording {
                        row.why = want.to_string();
                    }
                }
                if got != want_rows {
                    wrong.push(format!(
                        "cell [screen_activity {}] x [screen {value}]: got {:?}",
                        if switch_on { "on" } else { "off" },
                        got.iter().map(|r| r.why.as_str()).collect::<Vec<_>>()
                    ));
                }
            }
        }
        assert_eq!(rows, 24);
        assert!(
            wrong.is_empty(),
            "{} rows:\n{}",
            wrong.len(),
            wrong.join("\n")
        );
    }

    #[test]
    fn ct11_the_type_takes_the_three_kinds_of_status_the_client_offers_a_pause_and_a_delete_and_the_texts_promise_nothing(
    ) {
        use crate::client::{Client, DeleteOutcome};
        use crate::proto::{
            decode_reply, encode_reply, Capture, CaptureReason, Reason, Reply, SourceName,
            SourceState,
        };
        use crate::testing::{ScriptedConnector, ScriptedDaemon};
        let failed = SourceState::Failed {
            reason: Reason::Timeout,
            error: None,
        };
        let on = [
            SwitchName::TitleText,
            SwitchName::KeyClass,
            SwitchName::PointerClass,
        ];
        let kinds = [
            (
                "macOS with the guard none beside an input line that may run",
                status_beside(
                    Platform::Macos,
                    SourceName::Input,
                    &failed,
                    false,
                    &on,
                    PasswordGuard::None,
                ),
                PW,
                false,
                false,
            ),
            (
                "macOS with the guard halvo beside an input line that may run",
                status_beside(
                    Platform::Macos,
                    SourceName::Input,
                    &failed,
                    true,
                    &on,
                    PasswordGuard::Halvo,
                ),
                PW,
                false,
                false,
            ),
            (
                "Windows with the guard os beside a keyboard line that may run",
                status_beside(
                    Platform::Windows,
                    SourceName::Keyboard,
                    &failed,
                    false,
                    &on,
                    PasswordGuard::Os,
                ),
                PW,
                false,
                true,
            ),
            (
                "the mark beside a keyboard line that reads off",
                status_beside(
                    Platform::Windows,
                    SourceName::Keyboard,
                    &SourceState::Off,
                    true,
                    &on,
                    PasswordGuard::None,
                ),
                PW,
                false,
                true,
            ),
            (
                "the mark beside a screen line that reads stopped",
                status_beside(
                    Platform::Macos,
                    SourceName::Screen,
                    &SourceState::Stopped { by_user: false },
                    true,
                    &on,
                    PasswordGuard::Os,
                ),
                PM,
                true,
                false,
            ),
            (
                "a screen row that says S0 beside a screen line that runs",
                status_beside(
                    Platform::Macos,
                    SourceName::Screen,
                    &SourceState::Running,
                    false,
                    &on,
                    PasswordGuard::Os,
                ),
                PM,
                true,
                false,
            ),
        ];
        for (kind, pausing, want_passwords, screen_runs, keyboard_runs) in kinds {
            let mut f = pausing.into_fields();
            let required: Vec<&crate::proto::SourceLine> = f
                .sources
                .iter()
                .filter(|l| l.state != SourceState::Off)
                .collect();
            let running = required
                .iter()
                .filter(|l| matches!(l.state, SourceState::Running | SourceState::Impaired { .. }))
                .count();
            f.capture = if running == required.len() {
                Capture::Running
            } else if running > 0 {
                Capture::Partial
            } else {
                Capture::Faulted
            };
            f.capture_reason = (f.capture == Capture::Faulted).then_some(CaptureReason::NoSource);
            let status = DaemonStatus::new(f).unwrap_or_else(|e| panic!("{kind}: {e}"));
            let frame = encode_reply(&Reply::Status(status.clone()));
            assert_eq!(
                decode_reply(&frame),
                Ok(Reply::Status(status.clone())),
                "{kind}"
            );
            let connector = ScriptedConnector::new(ScriptedDaemon::new(status.clone()));
            let client = Client::new(connector.clone());
            let read = client.status().unwrap_or_else(|e| panic!("{kind}: {e}"));
            assert_eq!(read, status, "{kind}");
            let id = read.fields().store_id;
            assert!(id.is_some(), "{kind}");
            assert!(client.pause().is_ok(), "{kind}: the pause");
            assert!(
                matches!(client.delete_all(id), Ok(DeleteOutcome::Deleted(_))),
                "{kind}: the delete armed with the shown id"
            );
            let f = status.fields();
            let told = texts(&status);
            assert_eq!(
                told.passwords, want_passwords,
                "{kind}: the password sentence of the table"
            );
            let sentences = texts_of(&told.recorded);
            if screen_runs {
                assert!(!sentences.contains(&S0), "{kind}: {sentences:?}");
                let rows = onboarding_rows(&status);
                assert!(rows.iter().all(|r| r.why != S0), "{kind}: the screen row");
                assert!(
                    f.onboarding.permissions.iter().any(|r| r.why == S0),
                    "{kind}: the status itself holds S0 in its screen row"
                );
            }
            if keyboard_runs {
                assert!(!sentences.contains(&K0), "{kind}: {sentences:?}");
            }
            assert!(
                screen_runs || keyboard_runs || f.platform == Platform::Macos,
                "{kind}"
            );
        }
    }

    #[test]
    fn ct9_a_source_with_no_line_is_never_read_as_off() {
        assert_eq!(
            texts_of(&recorded(Platform::Windows, &switches(&[]), &[])),
            [K2, S2]
        );
        assert_eq!(
            texts_of(&recorded(Platform::Macos, &switches(&[]), &[])),
            [S2]
        );
    }
}
