use serde::Serialize;

use crate::model::{Modality, StorageCap};
use crate::onboarding::{Permission, PermissionStatus};
use crate::proto::{
    source_of, Cap, Custody, DaemonStatus, OnOff, PasswordGuard, Platform, SourceLine, SourceName,
    SourceState, SwitchLine, SwitchName,
};

/// Typing on a Mac: whether a source that the product builds asks its tap for key events. On
/// by the owner's word of 2026-11-22 (typing is on in the first build on both systems: never
/// which key, and nothing at a password field), so a Mac build notes what kind of key is
/// pressed while the switch key_class is on and Input Monitoring is granted. The capture crate's
/// macOS input source reads it as its one switch of key events, and [`first_run`] as its row on
/// typing.
pub const TYPING_ON_MAC: bool = true;

/// Typing on Windows: whether a service run of a Windows build starts the keyboard source,
/// which notes a key only under the password gate of win-6. On by the owner's word of
/// 2026-11-22 and the default of entry I-156, so a Windows build notes what kind of key is
/// pressed while the switch key_class is on. It is read in three places: the Windows service
/// set's make of the keyboard source, the start of a service run, and [`first_run`]. Set to
/// false, a Windows build makes and starts no keyboard source and its first-run text says that
/// it does not record what you type, as before this switch.
pub const TYPING_ON_WINDOWS: bool = true;

/// The row on typing where no typing is recorded: on a Mac whose build takes no typing
/// ([`TYPING_ON_MAC`]) in the first-run text, and on Windows while the keyboard source line
/// reads off.
pub const TYPING_NOT_RECORDED: &str = "Halvo does not record what you type.";

/// The sentence of the first-run text on the search for repeated tasks, which the consent
/// says in one plain sentence.
pub const REPEATED_TASKS: &str =
    "Halvo looks through what it recorded for tasks that repeat, on this computer alone.";

pub const SCREEN_ACTIVITY_NOT_RECORDED: &str = "Screen activity is not recorded.";

/// In place of [`TYPING_NOT_RECORDED`] while the Windows keyboard source line runs (reads
/// running or impaired).
pub const TYPING_BEING_OBSERVED: &str = "Typing is being observed. Pause capture to stop it.";

/// In place of [`SCREEN_ACTIVITY_NOT_RECORDED`] while the screen source line runs (reads running
/// or impaired) and the switch screen_activity is off.
pub const SCREEN_ACTIVITY_RECORDED_WITH_THE_SETTING_OFF: &str =
    "Screen activity is being recorded although the setting is off. Pause capture to stop it.";

/// In place of [`TYPING_NOT_RECORDED`] while the Windows keyboard source line reads failed,
/// carries the mark `unstopped` beside stopped or off, or is missing.
pub const TYPING_MAY_BE_OBSERVED: &str = "Typing may still be observed: Halvo's capture service reports a fault in that part. To be sure it is off, pause capture or quit Halvo.";

/// In place of [`SCREEN_ACTIVITY_NOT_RECORDED`] while screen_activity is off and the screen source
/// line reads failed, carries the mark `unstopped` beside stopped or off, or is missing.
pub const SCREEN_ACTIVITY_MAY_BE_RECORDED_WITH_THE_SETTING_OFF: &str = "Screen activity may still be recorded although the setting is off: Halvo's capture service reports a fault in that part. To be sure it is off, pause capture or quit Halvo.";

pub const PASSWORDS_NOT_CONFIRMED: &str = "Halvo cannot confirm that it leaves out password fields. While typing is observed, Halvo may note each key that you press in a password field and what kind of key it is, never which letter, digit or sign. Pause capture before you type a password.";

/// The passwords row on Windows in the place of the Mac's [`PASSWORDS_NOT_CONFIRMED`]: the guard
/// is not Halvo's while the keyboard source line is not off. The Windows gate notes no key in a
/// field that it cannot read (U1, sentence W2 (b)).
pub const PASSWORDS_FIELD_UNKNOWN_ON_WINDOWS: &str =
    "Halvo cannot tell which field you type in, so it records no key until it can.";

pub const KEY_TYPED_AHEAD_CAN_BE_NOTED: &str = "A key that you press before a program or page has moved to its password field, for example while it is frozen or still loading, can be noted: when it was pressed, in which app and what kind of key, never which letter, digit or sign. To be sure, wait until the cursor blinks in the password field, or pause capture before you sign in.";

/// The Windows sentence in the place of [`KEY_TYPED_AHEAD_CAN_BE_NOTED`]: beside the passwords row
/// of the Windows gate, in the view and in the first-run text (U1, sentence W3).
pub const KEY_TYPED_AHEAD_ON_WINDOWS: &str = "A key that you press before a program or page has moved to its password field, for example while it is frozen or busy, can in rare cases be noted: when you pressed it, in which app and what kind of key it was, never which letter, digit or sign. To be sure, wait until the cursor blinks in the password field, or pause capture before you sign in.";

/// What a Mac's page says while its status reads keys_withheld yes: an item and no warning
/// (ruling PR-30 PR10), in the coordinator's words after three plain readers, the owner's to
/// change. A withheld key is dropped from the macOS keyboard source's queue and written nowhere.
pub const KEYS_WITHHELD_ON_A_MAC: &str = "Halvo did not save part of what you typed, because it may have been a password. What Halvo leaves out this way is never saved. You do not need to do anything.";

/// What a Windows page says while its status reads keys_withheld yes, an item and no warning as on
/// a Mac. The Windows gate withholds every key after which the text of a field that it reads as
/// plain does not change, so the flag stands in nearly every use (U1, sentence W5).
pub const KEYS_WITHHELD_ON_WINDOWS: &str = "Halvo saves a key only when, just after you pressed it, the text changed in a field that is not a password field. It did not save the other keys that you pressed. What Halvo leaves out this way is never saved. You do not need to do anything.";

const PASSWORDS_WITH_SECURE_INPUT: &str = "While a password field holds secure input, Halvo records no keystroke; a terminal that does not ask for secure input gets no such protection.";

/// The passwords row on Windows while the keyboard source line reads off, in the coordinator's
/// words: the gate of the Windows service set holds window titles and screen activity back
/// while a password field has the focus.
const PASSWORDS_BY_THE_GATE_ON_WINDOWS: &str = "Halvo does not record what you type. While it can tell that a password field has the focus, Halvo records no window title and no screen activity. To tell, it reads the controls of the window in front through UI Automation, which uses some memory in the browser.";

/// The passwords row on Windows while the keyboard source line is not off and the guard is
/// Halvo's; its first clause is U1's sentence W2 (a).
const PASSWORDS_WITH_HALVO_S_DETECTOR: &str = "Halvo records a key only when, just after you press it, the text changes in a field that is not a password field; to find the field, Halvo reads the controls of the window in front through UI Automation, which uses some memory in the browser. While Halvo records typing on Windows, Windows hands Halvo every mouse event, also when pointer recording is off. Halvo looks at one thing in it, whether a button was pressed and when, and uses that to stop recording keys the moment you click into a password field. It does not look at where the pointer is or how it moves, and it stores nothing of the mouse.";

pub const UNSIGNED_BUILD_CUSTODY: &str = "This is an unsigned build: the key that protects your data is kept in a file with owner-only permissions in your account.";

pub const WINDOWS_CAPTURE_BORDER: &str =
    "While Halvo notes screen activity, Windows draws a border around the screen.";

pub const LIMITS: &str = "A copy of Halvo's data made earlier, such as a backup, keeps what it held: expiry does not reach it, and after a delete-all a copy of the whole data folder can still be read.";

/// The typing row on Windows in the place of the Mac's row of [`recorded_when_on`]: the switch
/// key_class is on and the keyboard source line runs. The Windows gate notes a key only on a
/// change of the text of a field that it reads as plain (U1, sentence W1). No Windows build runs
/// its keyboard source before typing is switched on there.
pub const KEYS_RECORDED_ON_WINDOWS: &str = "Halvo records a key only when, just after you press it, the text changes in a field that Windows does not report as a password field. It records when you pressed the key, in which app, and what kind of key it was: a letter, digit or sign, or a delete key. It never records which letter, digit or sign. Halvo can tell this only in Chrome and Edge and in the plain text boxes of classic Windows programs: in every other program it records no key. It records no Return or Enter, no Tab, no arrow key, no Escape and no shortcut.";

/// What Halvo records while `switch` is on.
pub const fn recorded_when_on(switch: SwitchName) -> &'static str {
    match switch {
        SwitchName::TitleText => "Halvo records the title text of the window in front; the title of a private window is not stored where Halvo can tell the window is private.",
        SwitchName::KeyClass => "Halvo records each key you press, when and in which app, and what kind of key it is: a letter, digit or sign, a delete key, an arrow or page key, Return or Enter, Tab, a shortcut, or another key. It never records which letter, digit or sign.",
        SwitchName::PointerClass => "Halvo records each click and which button you pressed, and how many times you scroll, drag or move the pointer each second, never where on the screen.",
        SwitchName::ScreenActivity => "Halvo notes screen activity, a record that the screen changed, and stores no image.",
        SwitchName::SiteRead => "Halvo records the site of the page in front in a browser, never the page's address or content.",
    }
}

/// The first row of `recorded` on both systems: what Halvo records with no switch, the app in
/// front at each change.
pub const APP_IN_FRONT: &str = "Halvo records which app is in front, each time that changes.";

/// One sentence of `recorded`, keyed by the switch that it is about; none for the row
/// [`APP_IN_FRONT`], which no switch turns off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Recorded {
    pub switch: Option<SwitchName>,
    pub text: &'static str,
}

const APP_IN_FRONT_ROW: Recorded = Recorded {
    switch: None,
    text: APP_IN_FRONT,
};

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
                    SourceClass::Off => Some(TYPING_NOT_RECORDED),
                    SourceClass::Runs if on => Some(KEYS_RECORDED_ON_WINDOWS),
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
                switch: Some(line.name),
                text,
            });
        }
        if platform == Platform::Windows && line.name == SwitchName::ScreenActivity && on {
            out.push(Recorded {
                switch: Some(line.name),
                text: WINDOWS_CAPTURE_BORDER,
            });
        }
    }
    out
}

fn passwords(platform: Platform, guard: PasswordGuard, typing: SourceClass) -> &'static str {
    match (platform, guard, typing) {
        (Platform::Windows, _, SourceClass::Off) => PASSWORDS_BY_THE_GATE_ON_WINDOWS,
        (Platform::Macos, PasswordGuard::Os, _) => PASSWORDS_WITH_SECURE_INPUT,
        (Platform::Windows, PasswordGuard::Halvo, _) => PASSWORDS_WITH_HALVO_S_DETECTOR,
        (Platform::Windows, _, _) => PASSWORDS_FIELD_UNKNOWN_ON_WINDOWS,
        _ => PASSWORDS_NOT_CONFIRMED,
    }
}

/// The sentence on a key pressed early, beside the passwords row of each system's guard.
fn typed_ahead(passwords: &str) -> Option<&'static str> {
    match passwords {
        PASSWORDS_WITH_SECURE_INPUT => Some(KEY_TYPED_AHEAD_CAN_BE_NOTED),
        PASSWORDS_WITH_HALVO_S_DETECTOR => Some(KEY_TYPED_AHEAD_ON_WINDOWS),
        _ => None,
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

/// The cap sentence of a status whose `cap_not_set` stands: the cap that a reset of the settings
/// stores, which is not set and not one that could not be read.
pub const NO_STORAGE_LIMIT: &str =
    "Halvo has no storage limit and removes nothing to make room until you set one.";

/// The cap sentence, in GB of 1,073,741,824 bytes with at most one decimal; for an unknown cap,
/// [`NO_STORAGE_LIMIT`] where `not_set` stands, and otherwise the sentence of a limit that could
/// not be read.
pub fn storage(cap: Cap, not_set: bool) -> String {
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
        Cap::Unknown if not_set => NO_STORAGE_LIMIT.to_string(),
        Cap::Unknown => "Halvo could not read its storage limit and removes nothing to make room until you set one.".to_string(),
    }
}

/// The table of the view (contracts.md C10.2 `texts`) for one status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Texts {
    pub recorded: Vec<Recorded>,
    pub passwords: &'static str,
    /// [`KEY_TYPED_AHEAD_CAN_BE_NOTED`] wherever `passwords` is the macOS secure-input sentence,
    /// [`KEY_TYPED_AHEAD_ON_WINDOWS`] wherever it is the row of the Windows gate, and no key
    /// elsewhere.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub typed_ahead: Option<&'static str>,
    /// [`KEYS_WITHHELD_ON_A_MAC`] on a Mac and [`KEYS_WITHHELD_ON_WINDOWS`] on Windows whose
    /// status reads keys_withheld yes, and no key elsewhere.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keys_withheld: Option<&'static str>,
    pub backup_limit: Option<&'static str>,
    pub limits: &'static str,
    pub custody: &'static str,
    pub storage: String,
    /// [`REPEATED_TASKS`] in the first-run text of [`first_run`], and no key elsewhere.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repeated_tasks: Option<&'static str>,
}

pub fn texts(status: &DaemonStatus) -> Texts {
    let f = status.fields();
    let typing = source_of(Modality::Keystroke, f.platform)
        .map_or(CLASS_OF_A_MISSING_LINE, |source| {
            class_of(&f.sources, source)
        });
    let passwords = passwords(f.platform, f.password_guard, typing);
    Texts {
        recorded: std::iter::once(APP_IN_FRONT_ROW)
            .chain(recorded(f.platform, &f.switches, &f.sources))
            .collect(),
        passwords,
        typed_ahead: typed_ahead(passwords),
        keys_withheld: f.keys_withheld.then_some(match f.platform {
            Platform::Macos => KEYS_WITHHELD_ON_A_MAC,
            Platform::Windows => KEYS_WITHHELD_ON_WINDOWS,
        }),
        backup_limit: backup_limit(f.platform),
        limits: LIMITS,
        custody: custody_text(f.custody),
        storage: storage(f.cap_bytes, f.cap_not_set),
        repeated_tasks: None,
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

/// The table of the first-run text (contracts.md C10.6 and C10.7) before any status: the rows
/// of the default switches (contracts.md C13.5) with every source off, on a Mac with the row on
/// typing by [`TYPING_ON_MAC`] behind the row on clicks; on a Mac with typing on also the
/// sentence on secure input and [`KEY_TYPED_AHEAD_CAN_BE_NOTED`], since the press on Start is
/// the consent to typing, and with typing off neither; on Windows, by [`TYPING_ON_WINDOWS`],
/// with typing on the rows of a keyboard source that runs under the gate:
/// [`KEYS_RECORDED_ON_WINDOWS`] in the place of its typing row, the gate's passwords row and
/// [`KEY_TYPED_AHEAD_ON_WINDOWS`], and with typing off the rows of a keyboard source that is
/// off; the backup limit, the limits, the custody on which the platform's build captures, the
/// storage of the default cap and the sentence on repeated tasks.
pub fn first_run(platform: Platform) -> Texts {
    first_run_with(
        platform,
        match platform {
            Platform::Macos => TYPING_ON_MAC,
            Platform::Windows => TYPING_ON_WINDOWS,
        },
    )
}

/// The first-run text of `platform` with typing on or off: [`first_run`] passes the build's
/// constant, and a test passes the other value.
pub fn first_run_with(platform: Platform, typing_on: bool) -> Texts {
    let switches: Vec<SwitchLine> = SwitchName::ALL
        .iter()
        .map(|&name| SwitchLine {
            name,
            value: if name.default_on() {
                OnOff::On
            } else {
                OnOff::Off
            },
        })
        .collect();
    let off = |name| SourceLine {
        name,
        state: SourceState::Off,
        unstopped: false,
    };
    let (keyboard, guard) = if platform == Platform::Windows && typing_on {
        (SourceState::Running, PasswordGuard::Halvo)
    } else {
        (SourceState::Off, PasswordGuard::None)
    };
    let sources = [
        SourceLine {
            state: keyboard,
            ..off(SourceName::Keyboard)
        },
        off(SourceName::Screen),
    ];
    let mut rows = recorded(platform, &switches, &sources);
    let mut passwords = passwords(platform, guard, class(&sources[0]));
    if platform == Platform::Macos {
        rows.retain(|row| row.switch != Some(SwitchName::KeyClass));
        let typing = Recorded {
            switch: Some(SwitchName::KeyClass),
            text: if typing_on {
                recorded_when_on(SwitchName::KeyClass)
            } else {
                TYPING_NOT_RECORDED
            },
        };
        let behind_the_clicks = rows
            .iter()
            .position(|row| row.switch == Some(SwitchName::PointerClass))
            .map_or(rows.len(), |at| at + 1);
        rows.insert(behind_the_clicks, typing);
        passwords = if typing_on {
            PASSWORDS_WITH_SECURE_INPUT
        } else {
            ""
        };
    }
    rows.insert(0, APP_IN_FRONT_ROW);
    Texts {
        recorded: rows,
        passwords,
        typed_ahead: typed_ahead(passwords),
        keys_withheld: None,
        backup_limit: backup_limit(platform),
        limits: LIMITS,
        custody: custody_text(match platform {
            Platform::Macos => Custody::RootFile,
            Platform::Windows => Custody::Dpapi,
        }),
        storage: storage(Cap::Known(StorageCap::DEFAULT), false),
        repeated_tasks: Some(REPEATED_TASKS),
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

    #[test]
    fn pr10_withheld_keys_on_a_mac_are_an_item_of_the_texts_that_is_no_warning() {
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
            "Halvo did not save part of what you typed, because it may have been a password. What Halvo leaves out this way is never saved. You do not need to do anything."
        );
        for (platform, yes) in [(Platform::Macos, false), (Platform::Windows, false)] {
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
        assert!(!told.contains(&TYPING_NOT_RECORDED), "{told:?}");
        assert!(!told.contains(&SCREEN_ACTIVITY_NOT_RECORDED), "{told:?}");
    }

    #[test]
    fn g2_the_view_says_typing_is_observed_while_the_windows_keyboard_source_runs() {
        use crate::proto::{SourceName, SourceState};
        for (key_class, row) in [
            (OnOff::On, KEYS_RECORDED_ON_WINDOWS),
            (
                OnOff::Off,
                "Typing is being observed. Pause capture to stop it.",
            ),
        ] {
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
            for line in &mut f.switches {
                if line.name == SwitchName::KeyClass {
                    line.value = key_class;
                }
            }
            let status = DaemonStatus::new(f).unwrap();
            let told = texts_of(&texts(&status).recorded);
            assert!(!told.contains(&TYPING_NOT_RECORDED), "{told:?}");
            assert!(told.contains(&row), "{key_class:?}: {told:?}");
        }
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
        assert_eq!(TYPING_NOT_RECORDED, "Halvo does not record what you type.");
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
                "Halvo records each key you press, when and in which app, and what kind of key it is: a letter, digit or sign, a delete key, an arrow or page key, Return or Enter, Tab, a shortcut, or another key. It never records which letter, digit or sign.",
            ),
            (
                SwitchName::PointerClass,
                "Halvo records each click and which button you pressed, and how many times you scroll, drag or move the pointer each second, never where on the screen.",
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
        let keys: Vec<SwitchName> = recorded
            .iter()
            .map(|r| r.switch.expect("a row of a switch"))
            .collect();
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
    fn a_default_status_on_windows_says_what_it_records_of_typing_and_names_no_border() {
        let recorded = recorded(
            Platform::Windows,
            &sample_status(Platform::Windows).fields().switches,
            &sample_status(Platform::Windows).fields().sources,
        );
        assert_eq!(
            texts_of(&recorded),
            [
                recorded_when_on(SwitchName::TitleText),
                KEYS_RECORDED_ON_WINDOWS,
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
            &windows_keyboard_off().fields().sources,
        );
        assert_eq!(
            texts_of(&on),
            [
                TYPING_NOT_RECORDED,
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
            TYPING_NOT_RECORDED,
            TYPING_BEING_OBSERVED,
            TYPING_MAY_BE_OBSERVED,
            SCREEN_ACTIVITY_RECORDED_WITH_THE_SETTING_OFF,
            SCREEN_ACTIVITY_MAY_BE_RECORDED_WITH_THE_SETTING_OFF,
            KEY_TYPED_AHEAD_CAN_BE_NOTED,
            WINDOWS_CAPTURE_BORDER,
            LIMITS,
            UNSIGNED_BUILD_CUSTODY,
            KEYS_RECORDED_ON_WINDOWS,
            KEY_TYPED_AHEAD_ON_WINDOWS,
            KEYS_WITHHELD_ON_WINDOWS,
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
            "Halvo does not record what you type. While it can tell that a password field has the focus, Halvo records no window title and no screen activity. To tell, it reads the controls of the window in front through UI Automation, which uses some memory in the browser."
        );
        assert_eq!(
            passwords(Platform::Windows, PasswordGuard::Halvo, SourceClass::Runs),
            "Halvo records a key only when, just after you press it, the text changes in a field that is not a password field; to find the field, Halvo reads the controls of the window in front through UI Automation, which uses some memory in the browser. While Halvo records typing on Windows, Windows hands Halvo every mouse event, also when pointer recording is off. Halvo looks at one thing in it, whether a button was pressed and when, and uses that to stop recording keys the moment you click into a password field. It does not look at where the pointer is or how it moves, and it stores nothing of the mouse."
        );
        assert_eq!(
            passwords(Platform::Windows, PasswordGuard::None, SourceClass::Runs),
            "Halvo cannot tell which field you type in, so it records no key until it can."
        );
        assert_eq!(
            passwords(Platform::Macos, PasswordGuard::None, SourceClass::Runs),
            "Halvo cannot confirm that it leaves out password fields. While typing is observed, Halvo may note each key that you press in a password field and what kind of key it is, never which letter, digit or sign. Pause capture before you type a password."
        );
        assert_eq!(
            KEY_TYPED_AHEAD_CAN_BE_NOTED,
            "A key that you press before a program or page has moved to its password field, for example while it is frozen or still loading, can be noted: when it was pressed, in which app and what kind of key, never which letter, digit or sign. To be sure, wait until the cursor blinks in the password field, or pause capture before you sign in."
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
            storage(Cap::Known(StorageCap::DEFAULT), false),
            "Halvo keeps at most 25 GB and removes the oldest days first when it reaches that."
        );
        assert_eq!(
            storage(Cap::Known(StorageCap::MIN), false),
            "Halvo keeps at most 1 GB and removes the oldest days first when it reaches that."
        );
        assert_eq!(
            storage(
                Cap::Known(StorageCap::from_bytes(1_610_612_736).unwrap()),
                false
            ),
            "Halvo keeps at most 1.5 GB and removes the oldest days first when it reaches that."
        );
        assert_eq!(
            storage(Cap::Unknown, false),
            "Halvo could not read its storage limit and removes nothing to make room until you set one."
        );
        assert_eq!(
            storage(Cap::Unknown, true),
            "Halvo has no storage limit and removes nothing to make room until you set one."
        );
    }

    #[test]
    fn the_table_of_a_status_has_the_six_keys_of_the_view() {
        let json = serde_json::to_value(texts(&windows_keyboard_off())).unwrap();
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
        assert_eq!(json["recorded"][0]["switch"], serde_json::Value::Null);
        assert_eq!(json["recorded"][0]["text"], APP_IN_FRONT);
        assert_eq!(json["recorded"][2]["switch"], "key_class");
        assert_eq!(json["recorded"][2]["text"], TYPING_NOT_RECORDED);
        let mac = serde_json::to_value(texts(&sample_status(Platform::Macos))).unwrap();
        assert_eq!(mac["backup_limit"], serde_json::Value::Null);
        assert_eq!(mac["custody"], UNSIGNED_BUILD_CUSTODY);
    }

    const K0: &str = "Halvo does not record what you type.";
    const K1: &str = "Typing is being observed. Pause capture to stop it.";
    const K2: &str = "Typing may still be observed: Halvo's capture service reports a fault in that part. To be sure it is off, pause capture or quit Halvo.";
    const S0: &str = "Screen activity is not recorded.";
    const S1: &str =
        "Screen activity is being recorded although the setting is off. Pause capture to stop it.";
    const S2: &str = "Screen activity may still be recorded although the setting is off: Halvo's capture service reports a fault in that part. To be sure it is off, pause capture or quit Halvo.";
    const P0: &str = "Halvo does not record what you type. While it can tell that a password field has the focus, Halvo records no window title and no screen activity. To tell, it reads the controls of the window in front through UI Automation, which uses some memory in the browser.";
    const PM: &str = "While a password field holds secure input, Halvo records no keystroke; a terminal that does not ask for secure input gets no such protection.";
    const PH: &str = "Halvo records a key only when, just after you press it, the text changes in a field that is not a password field; to find the field, Halvo reads the controls of the window in front through UI Automation, which uses some memory in the browser. While Halvo records typing on Windows, Windows hands Halvo every mouse event, also when pointer recording is off. Halvo looks at one thing in it, whether a button was pressed and when, and uses that to stop recording keys the moment you click into a password field. It does not look at where the pointer is or how it moves, and it stores nothing of the mouse.";
    const PW: &str = "Halvo cannot confirm that it leaves out password fields. While typing is observed, Halvo may note each key that you press in a password field and what kind of key it is, never which letter, digit or sign. Pause capture before you type a password.";
    const TA: &str = "A key that you press before a program or page has moved to its password field, for example while it is frozen or still loading, can be noted: when it was pressed, in which app and what kind of key, never which letter, digit or sign. To be sure, wait until the cursor blinks in the password field, or pause capture before you sign in.";
    const KW: &str = KEYS_RECORDED_ON_WINDOWS;
    const PF: &str = PASSWORDS_FIELD_UNKNOWN_ON_WINDOWS;
    const TW: &str = KEY_TYPED_AHEAD_ON_WINDOWS;
    const TITLE_ON: &str = "Halvo records the title text of the window in front; the title of a private window is not stored where Halvo can tell the window is private.";
    const KEY_CLASS_ON: &str = "Halvo records each key you press, when and in which app, and what kind of key it is: a letter, digit or sign, a delete key, an arrow or page key, Return or Enter, Tab, a shortcut, or another key. It never records which letter, digit or sign.";
    const POINTER_ON: &str = "Halvo records each click and which button you pressed, and how many times you scroll, drag or move the pointer each second, never where on the screen.";

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
        std::iter::once(APP_IN_FRONT_ROW)
            .chain(entries.iter().map(|&(switch, text)| Recorded {
                switch: Some(switch),
                text,
            }))
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
                                .map(|t| (KeyClass, if t == K1 { KW } else { t }))
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
                                (KeyClass, KW),
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
                (Platform::Windows, _, _) => PF,
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
                    let want_typed_ahead = match want {
                        PM => Some(TA),
                        PH => Some(TW),
                        _ => None,
                    };
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
                PF,
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
                PF,
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

    #[test]
    fn the_first_run_row_on_typing_follows_typing_on_mac() {
        let rows = |typing_on_mac: bool| -> Vec<(Option<SwitchName>, &'static str)> {
            first_run_with(Platform::Macos, typing_on_mac)
                .recorded
                .iter()
                .map(|row| (row.switch, row.text))
                .collect()
        };
        let app = (None, APP_IN_FRONT);
        let title = (
            Some(SwitchName::TitleText),
            recorded_when_on(SwitchName::TitleText),
        );
        let clicks = (
            Some(SwitchName::PointerClass),
            recorded_when_on(SwitchName::PointerClass),
        );
        let screen = (
            Some(SwitchName::ScreenActivity),
            SCREEN_ACTIVITY_NOT_RECORDED,
        );
        assert_eq!(
            rows(false),
            [
                app,
                title,
                clicks,
                (Some(SwitchName::KeyClass), TYPING_NOT_RECORDED),
                screen
            ],
            "typing off: the row that no typing is recorded, behind the row on clicks"
        );
        assert_eq!(
            rows(true),
            [
                app,
                title,
                clicks,
                (
                    Some(SwitchName::KeyClass),
                    recorded_when_on(SwitchName::KeyClass)
                ),
                screen
            ],
            "typing on: the standing row of key_class in its place"
        );
        let sentences = |typing_on_mac: bool| {
            let t = first_run_with(Platform::Macos, typing_on_mac);
            (t.passwords, t.typed_ahead)
        };
        assert_eq!(
            sentences(false),
            ("", None),
            "typing off: no sentence on passwords and none on a key pressed early"
        );
        assert_eq!(
            sentences(true),
            (
                "While a password field holds secure input, Halvo records no keystroke; a terminal that does not ask for secure input gets no such protection.",
                Some("A key that you press before a program or page has moved to its password field, for example while it is frozen or still loading, can be noted: when it was pressed, in which app and what kind of key, never which letter, digit or sign. To be sure, wait until the cursor blinks in the password field, or pause capture before you sign in.")
            ),
            "typing on: the sentence on secure input and the sentence on a key pressed early"
        );
        assert_eq!(
            first_run(Platform::Macos),
            first_run_with(Platform::Macos, TYPING_ON_MAC),
            "the first-run text reads the one constant"
        );
    }

    #[test]
    fn app_4a_on_windows_while_no_typing_is_observed_the_passwords_row_names_the_gate_of_a_password_field(
    ) {
        let sentence = "Halvo does not record what you type. While it can tell that a password field has the focus, Halvo records no window title and no screen activity. To tell, it reads the controls of the window in front through UI Automation, which uses some memory in the browser.";
        for &guard in PasswordGuard::ALL {
            assert_eq!(
                passwords(Platform::Windows, guard, SourceClass::Off),
                sentence,
                "{guard:?}"
            );
        }
        assert_eq!(texts(&windows_keyboard_off()).passwords, sentence);
        assert_eq!(first_run_with(Platform::Windows, false).passwords, sentence);
    }

    /// A byte copy of the coordinator's file of U1's ruled sentences, kept beside the tests.
    const U1_RULED: &str = include_str!("u1-ruled-sentences.txt");
    const PRIVACY_LIMITS: &str = include_str!("../../../docs/privacy-limits.md");

    /// The `count` lines under the line of the ruled copy that begins with `label`.
    fn ruled_lines(label: &str, count: usize) -> Vec<String> {
        let text = lf(U1_RULED);
        let mut lines = text.lines();
        lines
            .by_ref()
            .find(|line| line.starts_with(label))
            .unwrap_or_else(|| panic!("the ruled copy has no line {label}"));
        let under: Vec<String> = lines.take(count).map(str::to_string).collect();
        assert_eq!(under.len(), count, "the ruled copy under {label}");
        under
    }

    /// The line under the line of the ruled copy that begins with `label`.
    fn ruled(label: &str) -> String {
        ruled_lines(label, 1).remove(0)
    }

    fn one_space(text: &str) -> String {
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// The rest of the passwords row of the Windows gate behind its first clause, as it stood
    /// before U1.
    const DETECTOR_ROW_REST: &str = " to find the field, Halvo reads the controls of the window in front through UI Automation, which uses some memory in the browser. While Halvo records typing on Windows, Windows hands Halvo every mouse event, also when pointer recording is off. Halvo looks at one thing in it, whether a button was pressed and when, and uses that to stop recording keys the moment you click into a password field. It does not look at where the pointer is or how it moves, and it stores nothing of the mouse.";

    #[test]
    fn u1_each_windows_sentence_is_the_ruled_one_to_the_letter() {
        assert_eq!(KEYS_RECORDED_ON_WINDOWS, ruled("W1,"), "W1");
        let clause = ruled("W2 (a),");
        assert!(clause.ends_with(';'), "W2 (a) is a first clause: {clause}");
        assert_eq!(
            PASSWORDS_WITH_HALVO_S_DETECTOR,
            format!("{clause}{DETECTOR_ROW_REST}"),
            "W2 (a), the rest of the row as it stands"
        );
        assert_eq!(
            PASSWORDS_FIELD_UNKNOWN_ON_WINDOWS,
            ruled("W2 (b),"),
            "W2 (b)"
        );
        assert_eq!(KEY_TYPED_AHEAD_ON_WINDOWS, ruled("W3,"), "W3");
        assert_eq!(KEYS_WITHHELD_ON_WINDOWS, ruled("W5,"), "W5");
        for (name, sentence) in [
            ("W1", KEYS_RECORDED_ON_WINDOWS),
            ("W2 (b)", PASSWORDS_FIELD_UNKNOWN_ON_WINDOWS),
            ("W3", KEY_TYPED_AHEAD_ON_WINDOWS),
            ("W5", KEYS_WITHHELD_ON_WINDOWS),
        ] {
            assert!(
                !sentence.starts_with("On Windows"),
                "{name}: a row of the app"
            );
        }
        let doc = one_space(&lf(PRIVACY_LIMITS));
        let w1 = KEYS_RECORDED_ON_WINDOWS
            .strip_prefix("Halvo ")
            .expect("W1 begins with Halvo");
        let l0 = ruled("L0,");
        let l0 = l0.strip_suffix('.').expect("L0 ends with a period");
        for (name, text) in [
            (
                "W1 with L0 behind it and the unit's tag before its period",
                format!("On Windows, Halvo {w1} {l0} (win-6)."),
            ),
            ("L2", ruled("L2,")),
            (
                "L3 with L1 behind it",
                format!("{} {}", ruled("L3,"), ruled("L1,")),
            ),
            ("L4", ruled("L4,")),
            (
                "W3 under the label",
                format!("Limit on Windows: {KEY_TYPED_AHEAD_ON_WINDOWS}"),
            ),
        ] {
            assert!(
                doc.contains(&one_space(&text)),
                "docs/privacy-limits.md: {name}"
            );
        }
    }

    /// U1's L5: under "Limit on Windows:" the two lead-in clauses of the ruled copy, each pinned row
    /// of win-6's table and each order that it names with no row, in the words of the table's last
    /// column as the limits write them (no capitals for stress; the two references to the table read
    /// as the coordinator ruled them).
    #[test]
    fn u1_l5_the_windows_limit_lists_every_pinned_row_and_every_named_order() {
        const ONE: &str = " One key typed at the password field is noted, as a time, an app and a kind, never which key.";
        let rows = [
            "Type the first character of a password into a classic Windows program while it is busy, before it has moved the cursor to its password box, where the program itself fills or clears the box that has the cursor and then moves the cursor to the password box on its own a moment later.",
            "In a classic Windows program that has frozen for about half a second, press Tab to go to the password box and type the first character of the password about 0.4 s later, before the program wakes; the program, when it wakes, fills or clears the box that still has the cursor before it follows the Tab.",
            "Type two characters a third of a second apart into a classic Windows program that has frozen for more than a quarter of a second, where the program, on waking, moves the cursor from the box you typed into to its password box by itself (no Tab, no click).",
            "In a classic Windows program that freezes for about half a second, type the last character of the user name, press Tab, and type the first character of the password about 0.4 s later, all before the program wakes.",
            "In a classic Windows program, click a button or a menu item that pastes into the box that has the cursor, type the first character of the password at once, and have the program move the cursor to its password box on its own a moment later.",
            "Type two characters within about a hundredth of a second of each other into a web page's text box, on a page whose script, after the first character, freezes for a moment and then moves the cursor to its password box by itself.",
        ]
        .map(|row| format!("{row}{ONE}"));
        let style_mask = "Type a password into a box that a page only draws as dots on a web page in Chrome or Edge. Every key typed there is noted, as a time, an app and a kind, never which key, as in any field read as plain.";
        let named = [
            "Type two characters into a Windows program that is busy while a password manager fills its password box, where the program, on waking, moves the cursor to the password box by itself. It needs a password box that reports its changes, which the classic Windows password box did not do in the runs.",
            "As in ways 5 and 6 above, but type the password character more than about a third of a second after the click, or after the Enter.",
            "In a classic Windows program that has frozen, press Tab into its password box (or click it), type two characters about half a second later, and have the program, when it wakes, take the first in the password box and then move the cursor back to the box before it by itself, while Windows sends no notice of the Tab's move.",
        ]
        .map(|order| format!("{order}{ONE}"));
        let lead_ins = ruled_lines("ADDED", 2);
        let mut want = vec![format!(
            "Limit on Windows: {KEY_TYPED_AHEAD_ON_WINDOWS} {}",
            lead_ins[0]
        )];
        let numbered = |items: &[String]| -> Vec<String> {
            items
                .iter()
                .enumerate()
                .map(|(at, item)| format!("{}. {item}", at + 1))
                .collect()
        };
        want.extend(numbered(
            &rows
                .iter()
                .cloned()
                .chain([style_mask.to_string()])
                .collect::<Vec<_>>(),
        ));
        want.push(lead_ins[1].clone());
        want.extend(numbered(&named));
        let doc = one_space(&lf(PRIVACY_LIMITS));
        assert!(
            doc.contains(&one_space(&want.join(" "))),
            "the limit on Windows and its two lists, in this order and nothing between"
        );
        assert!(!doc.contains("ONE"), "no capitals for stress in the limits");
    }

    fn guard_of(platform: Platform) -> PasswordGuard {
        match platform {
            Platform::Macos => PasswordGuard::Os,
            Platform::Windows => PasswordGuard::Halvo,
        }
    }

    /// The Windows sample status with its keyboard line off and no guard, as a run reads it
    /// while key_class is off.
    fn windows_keyboard_off() -> DaemonStatus {
        status_beside(
            Platform::Windows,
            crate::proto::SourceName::Keyboard,
            &crate::proto::SourceState::Off,
            false,
            &[SwitchName::TitleText, SwitchName::PointerClass],
            PasswordGuard::None,
        )
    }

    fn typing_of(platform: Platform) -> crate::proto::SourceName {
        match platform {
            Platform::Macos => crate::proto::SourceName::Input,
            Platform::Windows => crate::proto::SourceName::Keyboard,
        }
    }

    const DEFAULT_ON: [SwitchName; 3] = [
        SwitchName::TitleText,
        SwitchName::KeyClass,
        SwitchName::PointerClass,
    ];

    #[test]
    fn u1_w1_the_typing_row_of_a_running_windows_keyboard_line_with_key_class_on() {
        use crate::proto::{Reason, SourceState};
        let impaired = SourceState::Impaired {
            reason: Reason::Permission,
            error: None,
        };
        for state in [SourceState::Running, impaired] {
            let row = |platform| {
                let status = status_beside(
                    platform,
                    typing_of(platform),
                    &state,
                    false,
                    &DEFAULT_ON,
                    guard_of(platform),
                );
                texts(&status)
                    .recorded
                    .iter()
                    .find(|r| r.switch == Some(SwitchName::KeyClass))
                    .map(|r| r.text)
            };
            assert_eq!(row(Platform::Windows), Some(KW), "{state:?}");
            assert_eq!(row(Platform::Macos), Some(KEY_CLASS_ON), "{state:?}");
        }
    }

    #[test]
    fn u1_w2a_the_passwords_row_under_the_gate() {
        use crate::proto::SourceState;
        let shown = |platform, guard| {
            texts(&status_beside(
                platform,
                typing_of(platform),
                &SourceState::Running,
                false,
                &DEFAULT_ON,
                guard,
            ))
            .passwords
        };
        let row = shown(Platform::Windows, PasswordGuard::Halvo);
        assert_eq!(row, PH);
        assert!(row.starts_with(&ruled("W2 (a),")), "{row}");
        assert_eq!(shown(Platform::Macos, PasswordGuard::Halvo), PW);
        assert_eq!(shown(Platform::Macos, PasswordGuard::Os), PM);
    }

    #[test]
    fn u1_w2b_the_passwords_row_where_the_guard_is_not_halvo() {
        use crate::proto::SourceState;
        let shown = |platform, state: &SourceState, mark, guard| {
            texts(&status_beside(
                platform,
                typing_of(platform),
                state,
                mark,
                &DEFAULT_ON,
                guard,
            ))
            .passwords
        };
        assert_eq!(
            shown(
                Platform::Windows,
                &SourceState::Off,
                true,
                PasswordGuard::None
            ),
            PF,
            "the one state of a real Windows status: the keyboard line off with the mark"
        );
        assert_eq!(
            shown(
                Platform::Windows,
                &SourceState::Running,
                false,
                PasswordGuard::Os
            ),
            PF
        );
        assert_eq!(
            shown(
                Platform::Macos,
                &SourceState::Off,
                true,
                PasswordGuard::None
            ),
            PW
        );
    }

    #[test]
    fn u1_w3_the_key_typed_ahead_beside_the_gate_row() {
        use crate::proto::SourceState;
        let shown = |platform, state: &SourceState, guard| {
            texts(&status_beside(
                platform,
                typing_of(platform),
                state,
                false,
                &DEFAULT_ON,
                guard,
            ))
        };
        let gate = shown(
            Platform::Windows,
            &SourceState::Running,
            PasswordGuard::Halvo,
        );
        assert_eq!(gate.typed_ahead, Some(TW));
        assert_eq!(serde_json::to_value(&gate).unwrap()["typed_ahead"], TW);
        let off = shown(Platform::Windows, &SourceState::Off, PasswordGuard::None);
        assert_eq!(off.typed_ahead, None);
        let mac = shown(Platform::Macos, &SourceState::Running, PasswordGuard::Os);
        assert_eq!(mac.typed_ahead, Some(TA));
    }

    #[test]
    fn u1_w4_the_windows_first_run_text_with_typing_on() {
        let rows = |t: &Texts| -> Vec<&'static str> { texts_of(&t.recorded) };
        let on = first_run_with(Platform::Windows, true);
        assert_eq!(
            rows(&on),
            [APP_IN_FRONT, TITLE_ON, KW, POINTER_ON, S0],
            "W1 in the place of the Windows typing row"
        );
        assert_eq!((on.passwords, on.typed_ahead), (PH, Some(TW)));
        assert_eq!(
            first_run(Platform::Windows),
            first_run_with(Platform::Windows, TYPING_ON_WINDOWS),
            "the first-run text reads the one constant"
        );
        let off = first_run_with(Platform::Windows, false);
        assert_eq!(rows(&off), [APP_IN_FRONT, TITLE_ON, K0, POINTER_ON, S0]);
        assert_eq!((off.passwords, off.typed_ahead), (P0, None));
        let mac = first_run(Platform::Macos);
        assert_eq!(mac, first_run_with(Platform::Macos, TYPING_ON_MAC));
        assert_eq!(
            rows(&first_run_with(Platform::Macos, true)),
            [APP_IN_FRONT, TITLE_ON, POINTER_ON, KEY_CLASS_ON, S0]
        );
        assert_eq!(first_run_with(Platform::Macos, true).typed_ahead, Some(TA));
    }

    #[test]
    fn u1_w5_withheld_keys_on_windows() {
        use crate::proto::SourceState;
        for (state, guard) in [
            (SourceState::Off, PasswordGuard::None),
            (SourceState::Running, PasswordGuard::Halvo),
        ] {
            let shown = |platform, yes| {
                let mut f = status_beside(
                    platform,
                    typing_of(platform),
                    &state,
                    false,
                    &DEFAULT_ON,
                    guard,
                )
                .into_fields();
                f.keys_withheld = yes;
                texts(&DaemonStatus::new(f).unwrap())
            };
            let win = shown(Platform::Windows, true);
            assert_eq!(
                win.keys_withheld,
                Some(KEYS_WITHHELD_ON_WINDOWS),
                "{state:?}"
            );
            assert_eq!(
                serde_json::to_value(&win).unwrap()["keys_withheld"],
                KEYS_WITHHELD_ON_WINDOWS
            );
            let none = serde_json::to_value(shown(Platform::Windows, false)).unwrap();
            assert!(none.get("keys_withheld").is_none(), "{state:?}");
            assert_eq!(
                shown(Platform::Macos, true).keys_withheld,
                Some(KEYS_WITHHELD_ON_A_MAC),
                "{state:?}"
            );
        }
    }
}
