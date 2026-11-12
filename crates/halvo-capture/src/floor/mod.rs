//! The content floor: what a capture source may emit and what the store keeps.
//! No value of the floor holds a typed character, a key code, a coordinate or
//! a pixel.

use std::str::FromStr;
use std::time::Duration;

use crate::event::Modality;

pub mod payload;
pub mod site;
pub mod text;

#[cfg(test)]
pub(crate) mod cases;

/// The stored resolution of `at` for keystroke and click records (first value
/// 1,000 ms; a change goes through a plan Revision). Order inside one second is
/// kept by the sink's number.
pub const KEY_TIME_RESOLUTION: Duration = Duration::from_millis(1000);

/// The stored resolution of `at` for every other record.
pub const TIME_RESOLUTION: Duration = Duration::from_millis(1);

/// The interval of one fold record for each pointer class.
pub const POINTER_FOLD_INTERVAL: Duration = Duration::from_millis(1000);

/// The kinds that R1 records, as types, so that a keep verdict names its kind.
pub mod kind {
    use crate::event::Modality;

    /// A kind that R1 records; only the five types of this module are one.
    pub trait Recorded: sealed::Sealed {
        const KIND: Modality;
    }

    mod sealed {
        pub trait Sealed {}
    }

    macro_rules! recorded {
        ($($name:ident),*) => {$(
            #[derive(Debug, Clone, Copy, PartialEq, Eq)]
            pub struct $name;
            impl sealed::Sealed for $name {}
            impl Recorded for $name {
                const KIND: Modality = Modality::$name;
            }
        )*};
    }

    recorded!(ForegroundApp, WindowTitle, Keystroke, Pointer, ScreenFrame);
}

/// One switch for each recorded class but foreground_app (contracts.md C13.5).
/// A switch that is off refuses the keep verdict for its kind, so no value of
/// that kind exists. No switch exists for typed characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Switch {
    /// window_title records; on by default.
    TitleText,
    /// keystroke records; on by default.
    KeyClass,
    /// pointer records; on by default.
    PointerClass,
    /// screen_frame records; off by default.
    ScreenActivity,
    /// The site of a title; off by default.
    SiteRead,
}

impl Switch {
    pub const ALL: [Switch; 5] = [
        Switch::TitleText,
        Switch::KeyClass,
        Switch::PointerClass,
        Switch::ScreenActivity,
        Switch::SiteRead,
    ];

    /// The name of the switch in the state file, in a flag and in Status.
    pub const fn as_str(self) -> &'static str {
        match self {
            Switch::TitleText => "title_text",
            Switch::KeyClass => "key_class",
            Switch::PointerClass => "pointer_class",
            Switch::ScreenActivity => "screen_activity",
            Switch::SiteRead => "site_read",
        }
    }

    pub const fn is_on_by_default(self) -> bool {
        match self {
            Switch::TitleText | Switch::KeyClass | Switch::PointerClass => true,
            Switch::ScreenActivity | Switch::SiteRead => false,
        }
    }

    const fn index(self) -> usize {
        match self {
            Switch::TitleText => 0,
            Switch::KeyClass => 1,
            Switch::PointerClass => 2,
            Switch::ScreenActivity => 3,
            Switch::SiteRead => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("no switch has that name")]
pub struct UnknownSwitch;

impl FromStr for Switch {
    type Err = UnknownSwitch;

    fn from_str(name: &str) -> Result<Switch, UnknownSwitch> {
        Switch::ALL
            .into_iter()
            .find(|switch| switch.as_str() == name)
            .ok_or(UnknownSwitch)
    }
}

/// The five switches. The daemon reads them from the state file at start and
/// hands them over in the floor handle; nothing changes them while it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Switches([bool; 5]);

impl Default for Switches {
    fn default() -> Switches {
        Switches(Switch::ALL.map(Switch::is_on_by_default))
    }
}

impl Switches {
    pub const fn is_on(self, switch: Switch) -> bool {
        self.0[switch.index()]
    }

    pub fn set(&mut self, switch: Switch, on: bool) {
        self.0[switch.index()] = on;
    }
}

/// The switch of `kind`: foreground_app has none, and the kinds that R1 does
/// not record have none.
pub(crate) const fn switch_of(kind: Modality) -> Option<Switch> {
    match kind {
        Modality::WindowTitle => Some(Switch::TitleText),
        Modality::Keystroke => Some(Switch::KeyClass),
        Modality::Pointer => Some(Switch::PointerClass),
        Modality::ScreenFrame => Some(Switch::ScreenActivity),
        Modality::ForegroundApp
        | Modality::SystemAudio
        | Modality::Microphone
        | Modality::Camera => None,
    }
}

/// The stored resolution of `at` for an observation.
pub(crate) fn resolution_of(observation: &payload::Observation) -> Duration {
    match observation {
        payload::Observation::Keystroke(_) | payload::Observation::Click(_) => KEY_TIME_RESOLUTION,
        payload::Observation::ForegroundApp
        | payload::Observation::WindowTitle(_)
        | payload::Observation::Fold(_)
        | payload::Observation::ScreenFrame => TIME_RESOLUTION,
    }
}

#[cfg(test)]
mod tests {
    use super::cases::stored;
    use super::kind::Recorded;
    use super::payload::{is_recorded, KeyClass};
    use super::text::{CapturedText, TextField, TEXT_MAX_BYTES};
    use super::*;

    #[test]
    fn the_defaults_are_three_on_and_two_off() {
        let defaults = Switches::default();
        let table = [
            ("title_text", true),
            ("key_class", true),
            ("pointer_class", true),
            ("screen_activity", false),
            ("site_read", false),
        ];
        assert_eq!(Switch::ALL.len(), table.len());
        for (switch, (name, on)) in Switch::ALL.into_iter().zip(table) {
            assert_eq!(switch.as_str(), name);
            assert_eq!(defaults.is_on(switch), on, "{name}");
            assert_eq!(name.parse::<Switch>(), Ok(switch));
            let mut flipped = defaults;
            flipped.set(switch, !on);
            for other in Switch::ALL {
                assert_eq!(
                    flipped.is_on(other),
                    (other == switch) != defaults.is_on(other)
                );
            }
        }
        assert_eq!("screen_frame".parse::<Switch>(), Err(UnknownSwitch));
        assert_eq!(switch_of(Modality::ForegroundApp), None);
    }

    #[test]
    fn the_recorded_kinds_are_exactly_the_floor() {
        let recorded: Vec<Modality> = Modality::ALL
            .into_iter()
            .filter(|kind| is_recorded(*kind))
            .collect();
        let floor = [
            kind::ScreenFrame::KIND,
            kind::Keystroke::KIND,
            kind::Pointer::KIND,
            kind::ForegroundApp::KIND,
            kind::WindowTitle::KIND,
        ];
        assert_eq!(recorded, floor);
        for kind in [
            Modality::SystemAudio,
            Modality::Microphone,
            Modality::Camera,
        ] {
            assert!(!is_recorded(kind));
            assert_eq!(switch_of(kind), None);
        }
        assert_eq!(std::mem::size_of::<KeyClass>(), 1);
        assert_eq!(KEY_TIME_RESOLUTION, Duration::from_millis(1000));
        assert_eq!(TIME_RESOLUTION, Duration::from_millis(1));
        assert_eq!(POINTER_FOLD_INTERVAL, Duration::from_millis(1000));
    }

    #[test]
    fn the_text_entry_cuts_on_a_character_boundary_and_keeps_a_path_tail() {
        let short = CapturedText::enter(TextField::Title, "Inbox".to_string());
        assert_eq!(stored(&short, TextField::Title), "Inbox");
        assert!(!short.was_cut());

        let exact = "a".repeat(TEXT_MAX_BYTES);
        let whole = CapturedText::enter(TextField::Title, exact.clone());
        assert_eq!(stored(&whole, TextField::Title), exact);
        assert!(!whole.was_cut());

        let over = format!("{}é", "a".repeat(TEXT_MAX_BYTES - 1));
        assert_eq!(over.len(), TEXT_MAX_BYTES + 1);
        let head = CapturedText::enter(TextField::Title, over);
        assert_eq!(
            stored(&head, TextField::Title),
            "a".repeat(TEXT_MAX_BYTES - 1)
        );
        assert!(head.was_cut());

        let wide = format!("a{}", "\u{1F600}".repeat(256));
        let head = CapturedText::enter(TextField::BundleId, wide);
        let kept = stored(&head, TextField::BundleId);
        assert_eq!(kept, format!("a{}", "\u{1F600}".repeat(255)));
        assert_eq!(kept.len(), 1021);

        let path = format!("C:\\{}\\bin\\app.exe", "dir\\".repeat(500));
        let tail = CapturedText::enter(TextField::ExecutablePath, path.clone());
        let kept = stored(&tail, TextField::ExecutablePath);
        assert_eq!(kept.len(), TEXT_MAX_BYTES);
        assert!(kept.ends_with("\\bin\\app.exe"));
        assert!(path.ends_with(&kept));
        assert!(tail.was_cut());

        let accents = format!("{}x", "é".repeat(600));
        let tail = CapturedText::enter(TextField::ExecutablePath, accents);
        let kept = stored(&tail, TextField::ExecutablePath);
        assert_eq!(kept, format!("{}x", "é".repeat(511)));
        assert_eq!(kept.len(), TEXT_MAX_BYTES - 1);

        let title_of_accents = format!("{}x", "é".repeat(600));
        let head = CapturedText::enter(TextField::Title, title_of_accents);
        assert_eq!(stored(&head, TextField::Title), "é".repeat(512));
    }

    #[test]
    fn captured_text_prints_a_placeholder() {
        let text = CapturedText::enter(TextField::Title, "Quarterly plan".to_string());
        assert_eq!(format!("{text:?}"), "CapturedText(..)");
    }
}
