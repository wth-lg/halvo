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

const PASSWORDS_WITH_SECURE_INPUT: &str = "While a password field holds secure input, Halvo records no keystroke; a terminal that does not ask for secure input gets no such protection.";

const PASSWORDS_WITH_NO_TYPING_ON_WINDOWS: &str = "Halvo observes no typing on Windows in this version, so it records nothing that you type into a password field.";

const PASSWORDS_WITH_HALVO_S_DETECTOR: &str = "While a password field has the focus, Halvo records no keystroke; to find the field, Halvo reads the controls of the window in front through UI Automation, which uses some memory in the browser.";

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
            "While a password field has the focus, Halvo records no keystroke; to find the field, Halvo reads the controls of the window in front through UI Automation, which uses some memory in the browser."
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
    const PH: &str = "While a password field has the focus, Halvo records no keystroke; to find the field, Halvo reads the controls of the window in front through UI Automation, which uses some memory in the browser.";
    const PW: &str = "Halvo cannot confirm that it leaves out password fields. While typing is observed, Halvo may note each key that you press in a password field, never which key. Pause capture before you type a password.";
    const TA: &str = "A key that you press before a program or page has reacted, for example while it is frozen or still loading, can be noted even if it ends up in a password field: when it was pressed, in which app and what kind of key, never which letter or digit. To be sure, pause capture before you type a password.";
    const TITLE_ON: &str = "Halvo records the title text of the window in front; the title of a private window is not stored where Halvo can tell the window is private.";
    const KEY_CLASS_ON: &str = "Halvo records a class for each key you press, never the character.";
    const POINTER_ON: &str = "Halvo records a class for each click or scroll, never its position.";

    fn line_values() -> Vec<(&'static str, crate::proto::SourceState, bool)> {
        use crate::proto::{Reason, SourceState};
        let impaired = SourceState::Impaired {
            reason: Reason::Permission,
            error: None,
        };
        let failed = SourceState::Failed {
            reason: Reason::Timeout,
            error: None,
        };
        vec![
            ("off", SourceState::Off, false),
            ("off with the mark", SourceState::Off, true),
            ("running", SourceState::Running, false),
            ("running with the mark", SourceState::Running, true),
            ("impaired", impaired.clone(), false),
            ("impaired with the mark", impaired, true),
            ("stopped", SourceState::Stopped { by_user: false }, false),
            (
                "stopped with the mark",
                SourceState::Stopped { by_user: false },
                true,
            ),
            (
                "stopped reason=user",
                SourceState::Stopped { by_user: true },
                false,
            ),
            (
                "stopped reason=user with the mark",
                SourceState::Stopped { by_user: true },
                true,
            ),
            ("failed", failed.clone(), false),
            ("failed with the mark", failed, true),
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
        let keyboard = [
            Some(K0),
            Some(K2),
            Some(K1),
            Some(K1),
            Some(K1),
            Some(K1),
            None,
            Some(K2),
            None,
            Some(K2),
            Some(K2),
            Some(K2),
        ];
        let screen = [
            Some(S0),
            Some(S2),
            Some(S1),
            Some(S1),
            Some(S1),
            Some(S1),
            None,
            Some(S2),
            None,
            Some(S2),
            Some(S2),
            Some(S2),
        ];
        let mut wrong = Vec::new();
        let mut rows = 0;
        for (i, (value, state, mark)) in line_values().into_iter().enumerate() {
            let columns = [
                (
                    "Windows, key_class on, beside the keyboard line",
                    Platform::Windows,
                    SourceName::Keyboard,
                    &default_on[..],
                    list(
                        &[
                            vec![(TitleText, TITLE_ON)],
                            keyboard[i].map(|t| (KeyClass, t)).into_iter().collect(),
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
                            keyboard[i].map(|t| (KeyClass, t)).into_iter().collect(),
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
                            screen[i].map(|t| (ScreenActivity, t)).into_iter().collect(),
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
                            screen[i].map(|t| (ScreenActivity, t)).into_iter().collect(),
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
        let classes = [
            "off", "may run", "runs", "runs", "runs", "runs", "stopped", "may run", "stopped",
            "may run", "may run", "may run",
        ];
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
                for (i, (value, state, mark)) in line_values().into_iter().enumerate() {
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
                    let want = cell(platform, guard, classes[i]);
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
        const WHY_ON: &str = "lets Halvo note screen activity, a record that the screen changed with no stored image; system audio stays off";
        let off_rows = [
            S0, WHY_ON, WHY_ON, WHY_ON, WHY_ON, WHY_ON, WHY_ON, WHY_ON, WHY_ON, WHY_ON, WHY_ON,
            WHY_ON,
        ];
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
            for (i, (value, state, mark)) in line_values().into_iter().enumerate() {
                rows += 1;
                let status = status_beside(
                    Platform::Macos,
                    SourceName::Screen,
                    &state,
                    mark,
                    on,
                    PasswordGuard::Os,
                );
                let want = if switch_on { WHY_ON } else { off_rows[i] };
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
