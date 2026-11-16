//! Test-only parts of the floor that the crate's tests share: the text filter
//! points of the tests, and one platform-neutral list of input transitions with
//! the events that they give, which the macOS and the Windows mapper tests run.

use std::sync::Arc;

use super::exclude::{FloorHandle, Keep, NoLineage, PrivateWindow, ProcessId, Subject, Verdict};
use super::kind::Recorded;
use super::payload::{Button, FoldClass, KeyClass};
use super::text::{write_through, CapturedText, TextField, TextFilterPoint};
use super::{Switch, Switches};
use crate::event::AppIdentity;
use crate::secure_input::{FixedProbe, SecureInputReading};

/// Every switch on.
pub(crate) fn all_switches_on() -> Switches {
    let mut switches = Switches::default();
    for switch in Switch::ALL {
        switches.set(switch, true);
    }
    switches
}

/// The keep verdict of the decision for an event of kind `K` of `app` in
/// process `pid`, with every switch on, no root and an inactive reading.
pub(crate) fn keep<K: Recorded>(app: AppIdentity, pid: u32) -> Keep<K> {
    let floor = FloorHandle::new(None, Arc::new(NoLineage), all_switches_on());
    let subject = Subject::new(app, ProcessId::new(pid));
    let probe = FixedProbe(SecureInputReading::Inactive);
    match floor.decide::<K>(subject, PrivateWindow::Unknown, &probe) {
        Verdict::Keep(keep) => keep,
        Verdict::Drop(reason) => panic!("the decision dropped the event: {reason:?}"),
    }
}

/// A point that stores every text unchanged.
pub(crate) struct IdentityPoint;

impl TextFilterPoint for IdentityPoint {
    type Stored = String;

    fn filter(&self, _field: TextField, text: &str) -> String {
        text.to_owned()
    }
}

/// A point that records each call and returns a marker in place of the text:
/// `marker-1.example` for the first call, `marker-2.example` for the second.
pub(crate) struct RecordingPoint {
    calls: std::cell::RefCell<Vec<(TextField, String)>>,
}

impl RecordingPoint {
    pub(crate) fn new() -> RecordingPoint {
        RecordingPoint {
            calls: std::cell::RefCell::new(Vec::new()),
        }
    }

    pub(crate) fn calls(&self) -> Vec<(TextField, String)> {
        self.calls.borrow().clone()
    }
}

impl TextFilterPoint for RecordingPoint {
    type Stored = String;

    fn filter(&self, field: TextField, text: &str) -> String {
        let mut calls = self.calls.borrow_mut();
        calls.push((field, text.to_owned()));
        format!("marker-{}.example", calls.len())
    }
}

/// A point that returns every app id and title unchanged and a site of
/// [`super::site::SITE_MAX_BYTES`] bytes in four labels: with texts of control
/// characters at their cap it gives the longest record.
pub(crate) struct LongestPoint;

impl TextFilterPoint for LongestPoint {
    type Stored = String;

    fn filter(&self, field: TextField, text: &str) -> String {
        match field {
            TextField::Site => format!(
                "{}.{}.{}.{}",
                "a".repeat(63),
                "b".repeat(63),
                "c".repeat(63),
                "d".repeat(super::site::SITE_MAX_BYTES - 3 * 64)
            ),
            TextField::BundleId | TextField::ExecutablePath | TextField::Title => text.to_owned(),
        }
    }
}

/// As [`LongestPoint`], with a title of this many control characters.
pub(crate) struct Lengthening(pub(crate) usize);

impl TextFilterPoint for Lengthening {
    type Stored = String;

    fn filter(&self, field: TextField, text: &str) -> String {
        match field {
            TextField::Title => "\u{1}".repeat(self.0),
            _ => LongestPoint.filter(field, text),
        }
    }
}

/// The text that the encoder writes for `text` in `field` through
/// [`IdentityPoint`].
pub(crate) fn stored(text: &CapturedText, field: TextField) -> String {
    let mut out = String::new();
    let changed = write_through(text, field, &IdentityPoint, &mut out);
    assert!(!changed, "the identity point changed a text");
    serde_json::from_str(&out).expect("the writer wrote a JSON string")
}

/// A key as a mapper sees it, with no character and no code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Key {
    Letter,
    Digit,
    Space,
    Punctuation,
    Backspace,
    Delete,
    Insert,
    Arrow,
    Home,
    End,
    PageUp,
    PageDown,
    Enter,
    Tab,
    Escape,
    Function,
    Modifier(Modifier),
}

/// A modifier key. Command or Windows, Control and Option or Alt make a chord;
/// Shift does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Modifier {
    Shift,
    Control,
    OptionOrAlt,
    CommandOrWindows,
}

/// One input transition, in the same form on both systems.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Input {
    KeyDown(Key),
    /// An automatic repeat of a held key.
    KeyRepeat(Key),
    KeyUp(Key),
    ButtonDown(Button),
    ButtonUp(Button),
    Move,
    /// A move with a button held.
    Drag,
    Scroll,
    /// The end of one fold interval.
    IntervalEnd,
}

/// One event that a mapper must give for the inputs of a case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Expected {
    Key(KeyClass),
    Click(Button),
    Fold(FoldClass, u64),
}

/// A mapper under test: the macOS or the Windows mapping of OS events, fed the
/// inputs of one case from a fresh state.
pub(crate) trait Mapper {
    fn input(&mut self, input: Input) -> Vec<Expected>;
}

pub(crate) struct Case {
    pub(crate) name: &'static str,
    pub(crate) inputs: &'static [Input],
    pub(crate) expected: &'static [Expected],
}

use Expected::{Click, Fold, Key as K};
use Input::{ButtonDown, ButtonUp, Drag, IntervalEnd, KeyDown, KeyRepeat, KeyUp, Move, Scroll};

const SHIFT: Key = Key::Modifier(Modifier::Shift);
const CONTROL: Key = Key::Modifier(Modifier::Control);
const ALT: Key = Key::Modifier(Modifier::OptionOrAlt);
const COMMAND: Key = Key::Modifier(Modifier::CommandOrWindows);
const PRIMARY: Button = Button::Primary;

/// The definition of one event for each kind of input (contracts.md C13.2), as
/// cases. Fold records come at the end of an interval in the order scroll,
/// drag, move.
pub(crate) const CASES: &[Case] = &[
    Case {
        name: "a printable key",
        inputs: &[KeyDown(Key::Letter), KeyUp(Key::Letter)],
        expected: &[K(KeyClass::Printable)],
    },
    Case {
        name: "a held key gives one event",
        inputs: &[
            KeyDown(Key::Letter),
            KeyRepeat(Key::Letter),
            KeyRepeat(Key::Letter),
            KeyRepeat(Key::Letter),
            KeyUp(Key::Letter),
        ],
        expected: &[K(KeyClass::Printable)],
    },
    Case {
        name: "a digit, a space and punctuation are printable",
        inputs: &[
            KeyDown(Key::Digit),
            KeyUp(Key::Digit),
            KeyDown(Key::Space),
            KeyUp(Key::Space),
            KeyDown(Key::Punctuation),
            KeyUp(Key::Punctuation),
        ],
        expected: &[
            K(KeyClass::Printable),
            K(KeyClass::Printable),
            K(KeyClass::Printable),
        ],
    },
    Case {
        name: "editing keys",
        inputs: &[
            KeyDown(Key::Backspace),
            KeyUp(Key::Backspace),
            KeyDown(Key::Delete),
            KeyUp(Key::Delete),
            KeyDown(Key::Insert),
            KeyUp(Key::Insert),
        ],
        expected: &[
            K(KeyClass::Editing),
            K(KeyClass::Editing),
            K(KeyClass::Editing),
        ],
    },
    Case {
        name: "navigation keys",
        inputs: &[
            KeyDown(Key::Arrow),
            KeyUp(Key::Arrow),
            KeyDown(Key::Home),
            KeyUp(Key::Home),
            KeyDown(Key::End),
            KeyUp(Key::End),
            KeyDown(Key::PageUp),
            KeyUp(Key::PageUp),
            KeyDown(Key::PageDown),
            KeyUp(Key::PageDown),
        ],
        expected: &[
            K(KeyClass::Navigation),
            K(KeyClass::Navigation),
            K(KeyClass::Navigation),
            K(KeyClass::Navigation),
            K(KeyClass::Navigation),
        ],
    },
    Case {
        name: "enter and tab",
        inputs: &[
            KeyDown(Key::Enter),
            KeyUp(Key::Enter),
            KeyDown(Key::Tab),
            KeyUp(Key::Tab),
        ],
        expected: &[K(KeyClass::Enter), K(KeyClass::Tab)],
    },
    Case {
        name: "escape and a function key are other",
        inputs: &[
            KeyDown(Key::Escape),
            KeyUp(Key::Escape),
            KeyDown(Key::Function),
            KeyUp(Key::Function),
        ],
        expected: &[K(KeyClass::Other), K(KeyClass::Other)],
    },
    Case {
        name: "a modifier down and up alone gives one event",
        inputs: &[KeyDown(SHIFT), KeyUp(SHIFT)],
        expected: &[K(KeyClass::Other)],
    },
    Case {
        name: "a held modifier gives one event",
        inputs: &[
            KeyDown(CONTROL),
            KeyRepeat(CONTROL),
            KeyRepeat(CONTROL),
            KeyUp(CONTROL),
        ],
        expected: &[K(KeyClass::Other)],
    },
    Case {
        name: "a shortcut gives two events",
        inputs: &[
            KeyDown(COMMAND),
            KeyDown(Key::Letter),
            KeyUp(Key::Letter),
            KeyUp(COMMAND),
        ],
        expected: &[K(KeyClass::Other), K(KeyClass::Shortcut)],
    },
    Case {
        name: "control with an arrow is a shortcut",
        inputs: &[
            KeyDown(CONTROL),
            KeyDown(Key::Arrow),
            KeyUp(Key::Arrow),
            KeyUp(CONTROL),
        ],
        expected: &[K(KeyClass::Other), K(KeyClass::Shortcut)],
    },
    Case {
        name: "option or alt with a key is a shortcut",
        inputs: &[
            KeyDown(ALT),
            KeyDown(Key::Enter),
            KeyUp(Key::Enter),
            KeyUp(ALT),
        ],
        expected: &[K(KeyClass::Other), K(KeyClass::Shortcut)],
    },
    Case {
        name: "shift with a letter is no shortcut",
        inputs: &[
            KeyDown(SHIFT),
            KeyDown(Key::Letter),
            KeyUp(Key::Letter),
            KeyUp(SHIFT),
        ],
        expected: &[K(KeyClass::Other), K(KeyClass::Printable)],
    },
    Case {
        name: "two modifiers and a key give three events",
        inputs: &[
            KeyDown(CONTROL),
            KeyDown(SHIFT),
            KeyDown(Key::Tab),
            KeyUp(Key::Tab),
            KeyUp(SHIFT),
            KeyUp(CONTROL),
        ],
        expected: &[
            K(KeyClass::Other),
            K(KeyClass::Other),
            K(KeyClass::Shortcut),
        ],
    },
    Case {
        name: "a key after the chord ends is no shortcut",
        inputs: &[
            KeyDown(COMMAND),
            KeyUp(COMMAND),
            KeyDown(Key::Letter),
            KeyUp(Key::Letter),
        ],
        expected: &[K(KeyClass::Other), K(KeyClass::Printable)],
    },
    Case {
        name: "a key-up alone is no event",
        inputs: &[KeyUp(Key::Letter), KeyUp(SHIFT)],
        expected: &[],
    },
    Case {
        name: "a click of each button",
        inputs: &[
            ButtonDown(Button::Primary),
            ButtonUp(Button::Primary),
            ButtonDown(Button::Secondary),
            ButtonUp(Button::Secondary),
            ButtonDown(Button::Other),
            ButtonUp(Button::Other),
        ],
        expected: &[
            Click(Button::Primary),
            Click(Button::Secondary),
            Click(Button::Other),
        ],
    },
    Case {
        name: "a button release is no event",
        inputs: &[ButtonUp(PRIMARY)],
        expected: &[],
    },
    Case {
        name: "moves fold into one record for the interval",
        inputs: &[Move, Move, Move, Move, Move, IntervalEnd],
        expected: &[Fold(FoldClass::Move, 5)],
    },
    Case {
        name: "scrolls fold into one record for the interval",
        inputs: &[Scroll, Scroll, Scroll, Scroll, IntervalEnd],
        expected: &[Fold(FoldClass::Scroll, 4)],
    },
    Case {
        name: "a drag is a click and a fold of drags",
        inputs: &[
            ButtonDown(PRIMARY),
            Drag,
            Drag,
            Drag,
            ButtonUp(PRIMARY),
            IntervalEnd,
        ],
        expected: &[Click(PRIMARY), Fold(FoldClass::Drag, 3)],
    },
    Case {
        name: "each interval has its own record",
        inputs: &[Move, Move, IntervalEnd, Move, Move, Move, IntervalEnd],
        expected: &[Fold(FoldClass::Move, 2), Fold(FoldClass::Move, 3)],
    },
    Case {
        name: "an interval with no activity has no record",
        inputs: &[IntervalEnd, IntervalEnd],
        expected: &[],
    },
    Case {
        name: "each class folds apart in one interval",
        inputs: &[Move, Scroll, Move, Drag, IntervalEnd],
        expected: &[
            Fold(FoldClass::Scroll, 1),
            Fold(FoldClass::Drag, 1),
            Fold(FoldClass::Move, 2),
        ],
    },
    Case {
        name: "keys and pointer input interleave",
        inputs: &[
            Move,
            KeyDown(Key::Letter),
            ButtonDown(PRIMARY),
            KeyUp(Key::Letter),
            ButtonUp(PRIMARY),
            Move,
            IntervalEnd,
        ],
        expected: &[
            K(KeyClass::Printable),
            Click(PRIMARY),
            Fold(FoldClass::Move, 2),
        ],
    },
];

/// The names of the cases whose events `new_mapper()` does not give exactly.
pub(crate) fn failing_cases<M: Mapper>(new_mapper: impl Fn() -> M) -> Vec<&'static str> {
    CASES
        .iter()
        .filter(|case| {
            let mut mapper = new_mapper();
            let given: Vec<Expected> = case
                .inputs
                .iter()
                .flat_map(|input| mapper.input(*input))
                .collect();
            given != case.expected
        })
        .map(|case| case.name)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The definition of C13.2 written as a mapper.
    #[derive(Default)]
    struct Reference {
        chord_held: u32,
        folds: [u64; 3],
        on_key_up: bool,
        on_repeat: bool,
        lone_modifier_dropped: bool,
    }

    fn chord(modifier: Modifier) -> bool {
        !matches!(modifier, Modifier::Shift)
    }

    fn class_of(key: Key) -> KeyClass {
        match key {
            Key::Letter | Key::Digit | Key::Space | Key::Punctuation => KeyClass::Printable,
            Key::Backspace | Key::Delete | Key::Insert => KeyClass::Editing,
            Key::Arrow | Key::Home | Key::End | Key::PageUp | Key::PageDown => KeyClass::Navigation,
            Key::Enter => KeyClass::Enter,
            Key::Tab => KeyClass::Tab,
            Key::Escape | Key::Function | Key::Modifier(_) => KeyClass::Other,
        }
    }

    impl Reference {
        fn key(&self, key: Key) -> Vec<Expected> {
            let class = match key {
                Key::Modifier(_) => KeyClass::Other,
                _ if self.chord_held > 0 => KeyClass::Shortcut,
                _ => class_of(key),
            };
            vec![K(class)]
        }
    }

    impl Mapper for Reference {
        fn input(&mut self, input: Input) -> Vec<Expected> {
            match input {
                KeyDown(key) => {
                    let events = self.key(key);
                    if let Key::Modifier(modifier) = key {
                        self.chord_held += u32::from(chord(modifier));
                        if self.lone_modifier_dropped {
                            return Vec::new();
                        }
                    }
                    if self.on_key_up {
                        return Vec::new();
                    }
                    events
                }
                KeyRepeat(key) if self.on_repeat => self.key(key),
                KeyRepeat(_) => Vec::new(),
                KeyUp(key) => {
                    if let Key::Modifier(modifier) = key {
                        self.chord_held -= u32::from(chord(modifier) && self.chord_held > 0);
                    }
                    if self.on_key_up {
                        return self.key(key);
                    }
                    Vec::new()
                }
                ButtonDown(button) => vec![Click(button)],
                ButtonUp(_) => Vec::new(),
                Scroll => {
                    self.folds[0] += 1;
                    Vec::new()
                }
                Drag => {
                    self.folds[1] += 1;
                    Vec::new()
                }
                Move => {
                    self.folds[2] += 1;
                    Vec::new()
                }
                IntervalEnd => {
                    let folds = std::mem::take(&mut self.folds);
                    FoldClass::ALL
                        .into_iter()
                        .zip(folds)
                        .filter(|(_, count)| *count > 0)
                        .map(|(class, count)| Fold(class, count))
                        .collect()
                }
            }
        }
    }

    #[test]
    fn the_list_is_complete_and_consistent() {
        let expected: Vec<Expected> = CASES
            .iter()
            .flat_map(|case| case.expected.iter().copied())
            .collect();
        for class in KeyClass::ALL {
            assert!(expected.contains(&K(class)), "no case gives {class:?}");
        }
        for button in Button::ALL {
            assert!(
                expected.contains(&Click(button)),
                "no case clicks {button:?}"
            );
        }
        for class in FoldClass::ALL {
            assert!(
                expected
                    .iter()
                    .any(|event| matches!(event, Fold(c, _) if *c == class)),
                "no case folds {class:?}"
            );
        }
        let inputs: Vec<Input> = CASES
            .iter()
            .flat_map(|case| case.inputs.iter().copied())
            .collect();
        let keys = [
            Key::Letter,
            Key::Digit,
            Key::Space,
            Key::Punctuation,
            Key::Backspace,
            Key::Delete,
            Key::Insert,
            Key::Arrow,
            Key::Home,
            Key::End,
            Key::PageUp,
            Key::PageDown,
            Key::Enter,
            Key::Tab,
            Key::Escape,
            Key::Function,
            SHIFT,
            CONTROL,
            ALT,
            COMMAND,
        ];
        for key in keys {
            assert!(inputs.contains(&KeyDown(key)), "no case presses {key:?}");
        }
        for input in [
            Move,
            Drag,
            Scroll,
            IntervalEnd,
            KeyRepeat(Key::Letter),
            KeyRepeat(CONTROL),
            ButtonUp(PRIMARY),
        ] {
            assert!(inputs.contains(&input), "no case has {input:?}");
        }
        let named = |name: &str| CASES.iter().find(|case| case.name == name).expect(name);
        assert_eq!(named("a held key gives one event").expected.len(), 1);
        assert_eq!(
            named("a modifier down and up alone gives one event")
                .expected
                .len(),
            1
        );
        assert_eq!(named("a shortcut gives two events").expected.len(), 2);
        for (index, case) in CASES.iter().enumerate() {
            assert!(
                CASES[..index].iter().all(|other| other.name != case.name),
                "{}",
                case.name
            );
        }
        assert_eq!(failing_cases(Reference::default), Vec::<&str>::new());
        let on_key_up = failing_cases(|| Reference {
            on_key_up: true,
            ..Reference::default()
        });
        assert!(
            on_key_up.contains(&"a key-up alone is no event"),
            "{on_key_up:?}"
        );
        let on_repeat = failing_cases(|| Reference {
            on_repeat: true,
            ..Reference::default()
        });
        assert!(
            on_repeat.contains(&"a held key gives one event"),
            "{on_repeat:?}"
        );
        let dropped = failing_cases(|| Reference {
            lone_modifier_dropped: true,
            ..Reference::default()
        });
        assert!(
            dropped.contains(&"a modifier down and up alone gives one event"),
            "{dropped:?}"
        );
    }
}
