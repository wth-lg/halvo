//! The observation of each kind that R1 records, with the fields below and no
//! other: no typed character, key code, scan code, coordinate, delta, wheel
//! amount, pixel, image or frame size. system_audio, microphone and camera have
//! no observation, so no value of those kinds exists.

use std::fmt;

use super::site::Site;
use super::text::{CapturedText, TextField};
use crate::event::Modality;

/// The class of one keystroke event. One event is one key-down transition, the
/// same on both systems; a key-up and an automatic repeat are no events. A
/// modifier key's own key-down is one event of class [`KeyClass::Other`],
/// whether the modifier is released alone or starts a chord, so a modifier
/// pressed and released alone counts once. A key-down while Command, Control,
/// Option or Alt, or the Windows key is held is one [`KeyClass::Shortcut`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyClass {
    /// A key that types a character: a letter, a digit, punctuation, space.
    Printable,
    /// Backspace, delete, insert.
    Editing,
    /// An arrow, home, end, page up, page down.
    Navigation,
    Enter,
    Tab,
    /// A key pressed while a command, control, option or alt, or Windows key is held.
    Shortcut,
    /// Any other key: a function key, escape, a modifier's own key-down.
    Other,
}

const _: () = assert!(std::mem::size_of::<KeyClass>() == 1);

impl KeyClass {
    pub const ALL: [KeyClass; 7] = [
        KeyClass::Printable,
        KeyClass::Editing,
        KeyClass::Navigation,
        KeyClass::Enter,
        KeyClass::Tab,
        KeyClass::Shortcut,
        KeyClass::Other,
    ];

    /// The spelling in a stored record.
    pub const fn as_str(self) -> &'static str {
        match self {
            KeyClass::Printable => "printable",
            KeyClass::Editing => "editing",
            KeyClass::Navigation => "navigation",
            KeyClass::Enter => "enter",
            KeyClass::Tab => "tab",
            KeyClass::Shortcut => "shortcut",
            KeyClass::Other => "other",
        }
    }
}

/// The button of one click: one pointer event for each button press.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Button {
    Primary,
    Secondary,
    Other,
}

impl Button {
    pub const ALL: [Button; 3] = [Button::Primary, Button::Secondary, Button::Other];

    /// The spelling in a stored record.
    pub const fn as_str(self) -> &'static str {
        match self {
            Button::Primary => "primary",
            Button::Secondary => "secondary",
            Button::Other => "other",
        }
    }
}

/// The pointer input that a source folds: one record for each class in each
/// interval of [`super::POINTER_FOLD_INTERVAL`] that had activity. A source
/// picks the class from the OS event type, never from the kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FoldClass {
    Scroll,
    Drag,
    Move,
}

impl FoldClass {
    pub const ALL: [FoldClass; 3] = [FoldClass::Scroll, FoldClass::Drag, FoldClass::Move];

    /// The spelling in a stored record.
    pub const fn as_str(self) -> &'static str {
        match self {
            FoldClass::Scroll => "scroll",
            FoldClass::Drag => "drag",
            FoldClass::Move => "move",
        }
    }
}

/// The number of OS events folded into one record. It starts at one and only
/// adds; no integer converts into it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FoldCount(u64);

impl FoldCount {
    /// The count of the first OS event of an interval.
    pub const fn one() -> FoldCount {
        FoldCount(1)
    }

    /// One more OS event.
    pub fn add_one(&mut self) {
        self.0 = self.0.saturating_add(1);
    }

    /// The events of a record that a full queue handed back.
    pub fn add(&mut self, other: FoldCount) {
        self.0 = self.0.saturating_add(other.0);
    }

    pub(crate) const fn get(self) -> u64 {
        self.0
    }
}

/// One fold record: a class and how many OS events it folds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointerFold {
    class: FoldClass,
    count: FoldCount,
}

impl PointerFold {
    pub const fn new(class: FoldClass, count: FoldCount) -> PointerFold {
        PointerFold { class, count }
    }

    pub const fn class(&self) -> FoldClass {
        self.class
    }

    pub const fn count(&self) -> FoldCount {
        self.count
    }
}

/// Why a title event was made: the app in front changed, or the title of its
/// focused window changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TitleWhy {
    AppChange,
    TitleChange,
}

impl TitleWhy {
    /// The spelling in a stored record.
    pub const fn as_str(self) -> &'static str {
        match self {
            TitleWhy::AppChange => "app_change",
            TitleWhy::TitleChange => "title_change",
        }
    }
}

/// The title of the focused window of the app in front, read on a change of
/// app or of title; none while secure input is active. The site is kept only
/// while the switch `site_read` is on.
pub struct Title {
    pub(crate) text: CapturedText,
    pub(crate) why: TitleWhy,
    pub(crate) site: Option<Site>,
}

impl Title {
    /// The title text passes the floor's text entry as a title: at most 1,024
    /// bytes, the head kept.
    pub fn new(text: String, why: TitleWhy, site: Option<Site>) -> Title {
        Title {
            text: CapturedText::enter(TextField::Title, text),
            why,
            site,
        }
    }
}

impl fmt::Debug for Title {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Title(..)")
    }
}

/// What one event observed, by kind.
#[derive(Debug)]
pub(crate) enum Observation {
    /// One event for each change of the frontmost process, keyed on the pid.
    ForegroundApp,
    WindowTitle(Title),
    Keystroke(KeyClass),
    /// One event for each button press.
    Click(Button),
    Fold(PointerFold),
    /// One activity record for each changed frame; an idle callback is never
    /// an event.
    ScreenFrame,
}

impl Observation {
    pub(crate) fn kind(&self) -> Modality {
        match self {
            Observation::ForegroundApp => Modality::ForegroundApp,
            Observation::WindowTitle(_) => Modality::WindowTitle,
            Observation::Keystroke(_) => Modality::Keystroke,
            Observation::Click(_) | Observation::Fold(_) => Modality::Pointer,
            Observation::ScreenFrame => Modality::ScreenFrame,
        }
    }
}

/// Whether R1 records `kind`: one exhaustive match, so a new kind needs a
/// decision here before it compiles.
pub(crate) const fn is_recorded(kind: Modality) -> bool {
    match kind {
        Modality::ScreenFrame
        | Modality::Keystroke
        | Modality::Pointer
        | Modality::ForegroundApp
        | Modality::WindowTitle => true,
        Modality::SystemAudio | Modality::Microphone | Modality::Camera => false,
    }
}
