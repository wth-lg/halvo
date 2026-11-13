use serde::Serialize;

use crate::model::StorageCap;
use crate::proto::{
    Cap, Custody, DaemonStatus, OnOff, PasswordGuard, Platform, SourceLine, SourceName,
    SourceState, SwitchLine, SwitchName,
};

pub const SCREEN_ACTIVITY_NOT_RECORDED: &str = "Screen activity is not recorded.";

pub const TYPING_NOT_OBSERVED_ON_WINDOWS: &str =
    "Typing is not observed on Windows in this version.";

/// In place of [`TYPING_NOT_OBSERVED_ON_WINDOWS`] while the Windows keyboard source line is not
/// off; the words are the owner's to change (owner's list, entry I-73).
pub const TYPING_BEING_OBSERVED: &str = "Typing is being observed. Pause capture to stop it.";

/// In place of [`SCREEN_ACTIVITY_NOT_RECORDED`] while the screen source line is not off and the
/// switch is off; the words are the owner's to change (owner's list, entry I-73).
pub const SCREEN_ACTIVITY_RECORDED_WITH_THE_SETTING_OFF: &str =
    "Screen activity is being recorded although the setting is off. Pause capture to stop it.";

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

/// The sentences of `recorded` for the switches a daemon reports, in their order. A sentence
/// that says a source does not record is given only while that source's line reads off (a
/// source with no line reads as off); beside a line that is not off, a sentence that says the
/// source runs stands in its place. It is private: [`texts`] is the one way in, and it passes
/// every source line of a status, which the status type requires for its platform.
fn recorded(platform: Platform, switches: &[SwitchLine], sources: &[SourceLine]) -> Vec<Recorded> {
    let off = |name: SourceName| {
        sources
            .iter()
            .find(|s| s.name == name)
            .is_none_or(|s| s.state == SourceState::Off)
    };
    let mut out = Vec::new();
    for line in switches {
        let on = line.value == OnOff::On;
        let text = match (platform, line.name, on) {
            (Platform::Windows, SwitchName::KeyClass, _) if off(SourceName::Keyboard) => {
                Some(TYPING_NOT_OBSERVED_ON_WINDOWS)
            }
            (Platform::Windows, SwitchName::KeyClass, _) => Some(TYPING_BEING_OBSERVED),
            (_, SwitchName::ScreenActivity, false) if off(SourceName::Screen) => {
                Some(SCREEN_ACTIVITY_NOT_RECORDED)
            }
            (_, SwitchName::ScreenActivity, false) => {
                Some(SCREEN_ACTIVITY_RECORDED_WITH_THE_SETTING_OFF)
            }
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

/// The sentence about passwords for each guard that a status reports.
pub const fn passwords(platform: Platform, guard: PasswordGuard) -> &'static str {
    match (platform, guard) {
        (Platform::Macos, _) | (Platform::Windows, PasswordGuard::Os) => "While a password field holds secure input, Halvo records no keystroke; a terminal that does not ask for secure input gets no such protection.",
        (Platform::Windows, PasswordGuard::None) => "Halvo observes no typing on Windows in this version, so it records nothing that you type into a password field.",
        (Platform::Windows, PasswordGuard::Halvo) => "While a password field has the focus, Halvo records no keystroke; to find the field, Halvo reads the controls of the window in front through UI Automation, which uses some memory in the browser.",
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
    pub backup_limit: Option<&'static str>,
    pub limits: &'static str,
    pub custody: &'static str,
    pub storage: String,
}

pub fn texts(status: &DaemonStatus) -> Texts {
    let f = status.fields();
    Texts {
        recorded: recorded(f.platform, &f.switches, &f.sources),
        passwords: passwords(f.platform, f.password_guard),
        backup_limit: backup_limit(f.platform),
        limits: LIMITS,
        custody: custody_text(f.custody),
        storage: storage(f.cap_bytes),
    }
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
            },
            SourceLine {
                name: SourceName::Screen,
                state: impaired,
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

    const WAITS: &str = "this pins what the head says; the rule waits for a ruling (CT-8 E9, \
                         evidence/eng-manager/draft-rulings-control2-round3.md)";

    #[test]
    fn e9_recorded_beside_a_stopped_or_failed_keyboard_or_screen_line_is_pinned_as_the_head_gives_it(
    ) {
        use crate::proto::{Reason, SourceLine, SourceName, SourceState};
        for state in [
            SourceState::Stopped { by_user: false },
            SourceState::Stopped { by_user: true },
            SourceState::Failed {
                reason: Reason::Permission,
                error: None,
            },
        ] {
            let keyboard = [SourceLine {
                name: SourceName::Keyboard,
                state: state.clone(),
            }];
            assert_eq!(
                texts_of(&recorded(Platform::Windows, &switches(&[]), &keyboard)),
                vec![
                    "Typing is being observed. Pause capture to stop it.",
                    "Screen activity is not recorded."
                ],
                "a Windows keyboard line {state:?} and no screen line: {WAITS}"
            );
            let screen = [SourceLine {
                name: SourceName::Screen,
                state: state.clone(),
            }];
            for platform in [Platform::Macos, Platform::Windows] {
                assert!(
                    texts_of(&recorded(platform, &switches(&[]), &screen)).contains(
                        &"Screen activity is being recorded although the setting is off. Pause capture to stop it."
                    ),
                    "{platform:?}, a screen line {state:?} beside its switch off: {WAITS}"
                );
            }
        }
    }

    #[test]
    fn e9_the_password_sentence_of_windows_with_no_guard_beside_a_running_keyboard_is_pinned_as_the_head_gives_it(
    ) {
        use crate::proto::{SourceName, SourceState};
        let mut f = sample_status(Platform::Windows).into_fields();
        assert_eq!(f.password_guard, PasswordGuard::None);
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
        assert_eq!(
            texts(&status).passwords,
            "Halvo observes no typing on Windows in this version, so it records nothing that you type into a password field.",
            "{WAITS}"
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
            &[],
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
            WINDOWS_CAPTURE_BORDER,
            LIMITS,
            UNSIGNED_BUILD_CUSTODY,
        ]);
        for platform in [Platform::Macos, Platform::Windows] {
            for &guard in PasswordGuard::ALL {
                sentences.push(passwords(platform, guard));
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
            passwords(Platform::Macos, PasswordGuard::Os),
            "While a password field holds secure input, Halvo records no keystroke; a terminal that does not ask for secure input gets no such protection."
        );
        assert_eq!(
            passwords(Platform::Windows, PasswordGuard::None),
            "Halvo observes no typing on Windows in this version, so it records nothing that you type into a password field."
        );
        assert_eq!(
            passwords(Platform::Windows, PasswordGuard::Halvo),
            "While a password field has the focus, Halvo records no keystroke; to find the field, Halvo reads the controls of the window in front through UI Automation, which uses some memory in the browser."
        );
        for platform in [Platform::Macos, Platform::Windows] {
            for &guard in PasswordGuard::ALL {
                assert!(passwords(platform, guard).ends_with('.'));
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
}
