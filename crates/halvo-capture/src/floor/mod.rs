//! The content floor: what a capture source may emit and what the store keeps.
//! No value of the floor holds a typed character, a key code, a coordinate or
//! a pixel.

use std::str::FromStr;
use std::time::Duration;

use crate::event::Modality;

mod boundary_proofs;
pub mod exclude;
pub mod payload;
pub mod record;
pub mod site;
pub mod text;

#[cfg(test)]
pub(crate) mod cases;

/// A line that fails to build when `$ty` implements `$trait`: a type that
/// stands for a decision gains no derive unseen.
macro_rules! lacks {
    ($ty:ty, $trait:path) => {
        const _: fn() = || {
            trait AmbiguousIfImpl<A> {
                fn some_item() {}
            }
            impl<T: ?Sized> AmbiguousIfImpl<()> for T {}
            struct Invalid;
            impl<T: $trait> AmbiguousIfImpl<Invalid> for T {}
            let _ = <$ty as AmbiguousIfImpl<_>>::some_item;
        };
    };
}

lacks!(crate::secure_input::KeystrokeClearance, Clone);
lacks!(crate::secure_input::KeystrokeClearance, Default);
lacks!(
    crate::secure_input::KeystrokeClearance,
    serde::de::DeserializeOwned
);
lacks!(exclude::Keep<kind::Keystroke>, Clone);
lacks!(exclude::Keep<kind::Keystroke>, Default);
lacks!(exclude::Keep<kind::Keystroke>, serde::de::DeserializeOwned);
lacks!(payload::FoldCount, Default);
lacks!(payload::FoldCount, serde::de::DeserializeOwned);
lacks!(crate::event::FloorEvent, Default);

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
    fn a_stored_key_time_is_a_multiple_of_the_resolution() {
        use super::cases::keep;
        use super::payload::{Button, FoldClass, FoldCount, PointerFold};
        use crate::event::{AppIdentity, EventClock, FloorEvent, FoldEvent};
        use crate::secure_input::{check, FixedProbe, SecureInputReading};

        fn fixed() -> std::time::SystemTime {
            std::time::UNIX_EPOCH + Duration::new(1_800_000_000, 123_456_789)
        }
        let clock = EventClock::new(fixed);
        let app = || AppIdentity::bundle_id("com.apple.Terminal".to_string());
        let cleared = check(&FixedProbe(SecureInputReading::Inactive), None).unwrap();
        let key = FloorEvent::keystroke(keep(app(), 7), cleared, KeyClass::Enter, &clock);
        let click = FloorEvent::click(keep(app(), 7), Button::Secondary, None, &clock);
        let fold = PointerFold::new(FoldClass::Move, FoldCount::one());
        let moved = FoldEvent::new(keep(app(), 7), fold, None, &clock).into_event();
        let front = FloorEvent::foreground_app(keep(app(), 7), None, &clock);
        let frame = FloorEvent::screen_frame(keep(app(), 7), None, &clock);
        let second = 1_800_000_000 * 1_000_000_000;
        assert_eq!(key.at(), second);
        assert_eq!(click.at(), second);
        for fine in [moved.at(), front.at(), frame.at()] {
            assert_eq!(fine, second + 123_000_000);
        }
    }

    #[test]
    fn each_switch_off_refuses_its_kind() {
        use super::exclude::{DropReason, FloorHandle, NoLineage, PrivateWindow, Subject, Verdict};
        use crate::event::AppIdentity;
        use crate::secure_input::{FixedProbe, SecureInputReading};
        use std::sync::Arc;

        fn dropped<K: Recorded>(switches: Switches) -> Option<DropReason> {
            let floor = FloorHandle::new(None, Arc::new(NoLineage), switches);
            let app = AppIdentity::bundle_id("com.apple.Terminal".to_string());
            let subject = Subject::new(app, exclude::ProcessId::new(42));
            let probe = FixedProbe(SecureInputReading::Inactive);
            match floor.decide::<K>(subject, PrivateWindow::Unknown, &probe) {
                Verdict::Keep(_) => None,
                Verdict::Drop(reason) => Some(reason),
            }
        }

        fn each_kind(switches: Switches) -> [Option<DropReason>; 5] {
            [
                dropped::<kind::ForegroundApp>(switches),
                dropped::<kind::WindowTitle>(switches),
                dropped::<kind::Keystroke>(switches),
                dropped::<kind::Pointer>(switches),
                dropped::<kind::ScreenFrame>(switches),
            ]
        }

        let off = Some(DropReason::SwitchOff);
        assert_eq!(
            each_kind(Switches::default()),
            [None, None, None, None, off]
        );
        let mut all_on = Switches::default();
        all_on.set(Switch::ScreenActivity, true);
        assert_eq!(each_kind(all_on), [None; 5]);
        let expected = [
            (Switch::TitleText, [None, off, None, None, None]),
            (Switch::KeyClass, [None, None, off, None, None]),
            (Switch::PointerClass, [None, None, None, off, None]),
            (Switch::ScreenActivity, [None, None, None, None, off]),
            (Switch::SiteRead, [None; 5]),
        ];
        for (switch, kinds) in expected {
            let mut one_off = all_on;
            one_off.set(switch, false);
            assert_eq!(each_kind(one_off), kinds, "{switch:?}");
        }
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
    fn every_control_character_is_written_as_its_escape_and_read_back() {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for code in 0u8..0x20 {
            let c = char::from(code);
            let mut out = String::new();
            text::push_json_string(&mut out, &format!("a{c}b"));
            let escape = match code {
                0x08 => "\\b".to_string(),
                0x0c => "\\f".to_string(),
                0x0a => "\\n".to_string(),
                0x0d => "\\r".to_string(),
                0x09 => "\\t".to_string(),
                _ => format!(
                    "\\u00{}{}",
                    char::from(HEX[usize::from(code >> 4)]),
                    char::from(HEX[usize::from(code & 0xf)])
                ),
            };
            assert_eq!(out, format!("\"a{escape}b\""), "U+{code:04X}");
            let back: String = serde_json::from_str(&out).unwrap();
            assert_eq!(back, format!("a{c}b"), "U+{code:04X}");
        }
        let mut out = String::new();
        text::push_json_string(&mut out, "\u{1f}\"\\\u{7f}");
        assert_eq!(out, "\"\\u001f\\\"\\\\\u{7f}\"");
    }

    #[test]
    fn captured_text_prints_a_placeholder() {
        let text = CapturedText::enter(TextField::Title, "Quarterly plan".to_string());
        assert_eq!(format!("{text:?}"), "CapturedText(..)");
    }

    use crate::event::{AppIdentity, EventClock, FloorEvent, FoldEvent};
    use crate::floor::cases::{keep, IdentityPoint, RecordingPoint};
    use crate::floor::payload::{Button, FoldClass, FoldCount, PointerFold, Title, TitleWhy};
    use crate::floor::record::{decode, encode, DecodedPayload};
    use crate::floor::site::Site;
    use crate::secure_input::{check, FixedProbe, SecureInputReading};
    use crate::sink::{ArrivalNumber, FloorReceived};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixed() -> SystemTime {
        UNIX_EPOCH + Duration::new(1_800_000_000, 123_456_789)
    }

    fn app() -> AppIdentity {
        AppIdentity::bundle_id("com.apple.Safari".to_string())
    }

    /// The record of `event` with number 41, its id replaced by `ID` after
    /// the id is checked, and the decoder's reading of it.
    fn golden(event: FloorEvent) -> String {
        let id = event.id().uuid().hyphenated().to_string();
        let received = FloorReceived::for_test(event, ArrivalNumber::for_test(41));
        let line = encode(&received, &IdentityPoint).unwrap();
        let decoded = decode(line.as_str()).expect("the decoder reads the record");
        assert_eq!(decoded.id().hyphenated().to_string(), id);
        assert_eq!(decoded.seq(), 41);
        line.as_str().replacen(&id, "ID", 1)
    }

    #[test]
    fn golden_record_foreground_app() {
        let clock = EventClock::new(fixed);
        let event = FloorEvent::foreground_app(keep(app(), 7), None, &clock);
        assert_eq!(
            golden(event),
            r#"{"v":1,"id":"ID","at":1800000000123000000,"seq":41,"kind":"foreground_app","app":"com.apple.Safari","p":{}}"#
        );
        let unknown = FloorEvent::foreground_app(keep(AppIdentity::unknown(), 7), None, &clock);
        assert_eq!(
            golden(unknown),
            r#"{"v":1,"id":"ID","at":1800000000123000000,"seq":41,"kind":"foreground_app","app":null,"p":{}}"#
        );
    }

    #[test]
    fn golden_record_window_title() {
        let clock = EventClock::new(fixed);
        let site = Site::cut("https://mail.example.com/inbox?id=7".to_string());
        let title = Title::new("Inbox \"7\"\n".to_string(), TitleWhy::TitleChange, site);
        let path = AppIdentity::executable_path("C:\\Apps\\mail.exe".to_string());
        let event = FloorEvent::window_title(keep(path, 7), title, None, &clock);
        assert_eq!(
            golden(event),
            r#"{"v":1,"id":"ID","at":1800000000123000000,"seq":41,"kind":"window_title","app":"C:\\Apps\\mail.exe","p":{"text":"Inbox \"7\"\n","cut":false,"why":"title_change","site":{"domain":"example.com","source":"address_field"}}}"#
        );
        let title = Title::new("x".repeat(2_000), TitleWhy::AppChange, None);
        let event = FloorEvent::window_title(keep(app(), 7), title, None, &clock);
        assert_eq!(
            golden(event),
            format!(
                r#"{{"v":1,"id":"ID","at":1800000000123000000,"seq":41,"kind":"window_title","app":"com.apple.Safari","p":{{"text":"{}","cut":true,"why":"app_change"}}}}"#,
                "x".repeat(1_024)
            )
        );
    }

    #[test]
    fn golden_record_keystroke() {
        let clock = EventClock::new(fixed);
        let cleared = check(&FixedProbe(SecureInputReading::Inactive), None).unwrap();
        let event = FloorEvent::keystroke(keep(app(), 7), cleared, KeyClass::Shortcut, &clock);
        assert_eq!(
            golden(event),
            r#"{"v":1,"id":"ID","at":1800000000000000000,"seq":41,"kind":"keystroke","app":"com.apple.Safari","p":{"class":"shortcut"}}"#
        );
    }

    #[test]
    fn golden_record_pointer() {
        let clock = EventClock::new(fixed);
        let click = FloorEvent::click(keep(app(), 7), Button::Secondary, None, &clock);
        assert_eq!(
            golden(click),
            r#"{"v":1,"id":"ID","at":1800000000000000000,"seq":41,"kind":"pointer","app":"com.apple.Safari","p":{"class":"click","button":"secondary"}}"#
        );
        let mut two = FoldCount::one();
        two.add_one();
        let fold = PointerFold::new(FoldClass::Scroll, two);
        let scroll = FoldEvent::new(keep(app(), 7), fold, None, &clock).into_event();
        assert_eq!(
            golden(scroll),
            r#"{"v":1,"id":"ID","at":1800000000123000000,"seq":41,"kind":"pointer","app":"com.apple.Safari","p":{"class":"scroll","count":2}}"#
        );
    }

    #[test]
    fn golden_record_screen_frame() {
        let clock = EventClock::new(fixed);
        let event = FloorEvent::screen_frame(keep(app(), 7), None, &clock);
        assert_eq!(
            golden(event),
            r#"{"v":1,"id":"ID","at":1800000000123000000,"seq":41,"kind":"screen_frame","app":"com.apple.Safari","p":{}}"#
        );
    }

    #[test]
    fn a_title_keeps_its_site_only_while_site_read_is_on() {
        use super::exclude::{FloorHandle, NoLineage, PrivateWindow, ProcessId, Subject, Verdict};
        use std::sync::Arc;

        fn site_of(switches: Switches) -> Option<String> {
            let floor = FloorHandle::new(None, Arc::new(NoLineage), switches);
            let subject = Subject::new(app(), ProcessId::new(9));
            let probe = FixedProbe(SecureInputReading::Inactive);
            let Verdict::Keep(keep) =
                floor.decide::<kind::WindowTitle>(subject, PrivateWindow::Unknown, &probe)
            else {
                panic!("the title was dropped");
            };
            let site = Site::cut("https://news.example.org/today".to_string());
            let title = Title::new("Today".to_string(), TitleWhy::TitleChange, site);
            let event = FloorEvent::window_title(keep, title, None, EventClock::system());
            let received = FloorReceived::for_test(event, ArrivalNumber::for_test(0));
            let line = encode(&received, &IdentityPoint).unwrap();
            match decode(line.as_str()).unwrap().payload() {
                DecodedPayload::WindowTitle { site, .. } => {
                    site.as_ref().map(|site| site.as_str().to_string())
                }
                _ => panic!("not a title"),
            }
        }

        assert_eq!(site_of(Switches::default()), None);
        let mut on = Switches::default();
        on.set(Switch::SiteRead, true);
        assert_eq!(site_of(on).as_deref(), Some("example.org"));
    }

    #[test]
    fn the_text_filter_point_sees_every_string_once() {
        let site = Site::cut("https://www.planted-site.com/path".to_string());
        let title = Title::new("Planted title".to_string(), TitleWhy::AppChange, site);
        let planted = AppIdentity::bundle_id("com.example.PlantedApp".to_string());
        let event = FloorEvent::window_title(keep(planted, 7), title, None, EventClock::system());
        let received = FloorReceived::for_test(event, ArrivalNumber::for_test(0));
        let point = RecordingPoint::new();
        let line = encode(&received, &point).unwrap();
        assert_eq!(
            point.calls(),
            [
                (TextField::BundleId, "com.example.PlantedApp".to_string()),
                (TextField::Title, "Planted title".to_string()),
                (TextField::Site, "planted-site.com".to_string()),
            ]
        );
        let line = line.as_str();
        assert!(!line.to_lowercase().contains("planted"), "{line}");
        let decoded = decode(line).unwrap();
        assert_eq!(decoded.app(), Some("marker-1.example"));
        let DecodedPayload::WindowTitle {
            text, cut, site, ..
        } = decoded.payload()
        else {
            panic!("not a title");
        };
        assert_eq!(text.as_str(), "marker-2.example");
        assert!(cut, "the point changed the title");
        assert_eq!(
            site.as_ref().map(|site| site.as_str()),
            Some("marker-3.example")
        );
        let unknown =
            FloorEvent::foreground_app(keep(AppIdentity::unknown(), 7), None, EventClock::system());
        let received = FloorReceived::for_test(unknown, ArrivalNumber::for_test(1));
        let point = RecordingPoint::new();
        encode(&received, &point).unwrap();
        assert!(point.calls().is_empty());
        let path = AppIdentity::executable_path("C:\\Apps\\tool.exe".to_string());
        let front = FloorEvent::foreground_app(keep(path, 7), None, EventClock::system());
        let received = FloorReceived::for_test(front, ArrivalNumber::for_test(2));
        let point = RecordingPoint::new();
        encode(&received, &point).unwrap();
        assert_eq!(
            point.calls(),
            [(TextField::ExecutablePath, "C:\\Apps\\tool.exe".to_string())]
        );
    }

    #[test]
    fn a_title_is_entered_as_a_title_and_keeps_its_head() {
        let words = format!("{}{}", "h".repeat(TEXT_MAX_BYTES), "t".repeat(976));
        let title = Title::new(words, TitleWhy::TitleChange, None);
        let event = FloorEvent::window_title(keep(app(), 7), title, None, EventClock::system());
        let received = FloorReceived::for_test(event, ArrivalNumber::for_test(0));
        let line = encode(&received, &IdentityPoint).unwrap();
        let decoded = decode(line.as_str()).unwrap();
        let DecodedPayload::WindowTitle { text, cut, .. } = decoded.payload() else {
            panic!("not a title");
        };
        assert_eq!(text.as_str(), "h".repeat(TEXT_MAX_BYTES));
        assert!(cut);
    }
}
