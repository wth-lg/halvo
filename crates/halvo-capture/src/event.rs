use std::fmt;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use uuid::{ContextV7, Timestamp, Uuid};

use crate::floor::exclude::{Keep, ProcessId, Subject};
use crate::floor::kind;
use crate::floor::payload::{Button, KeyClass, Observation, PointerFold, Title};
use crate::floor::text::{CapturedText, TextField};
use crate::floor::{self};
use crate::secure_input::KeystrokeClearance;

pub use halvo_control::model::Modality;

/// The app that an event belongs to, in one of two forms, so that no record
/// holds a mix of them: on macOS the bundle id, else the executable path; on
/// Windows the executable path. Each string passes the floor's text entry, so
/// it is at most 1,024 bytes; a path keeps its tail.
pub struct AppIdentity(AppForm);

enum AppForm {
    BundleId(CapturedText),
    ExecutablePath(CapturedText),
    Unknown,
}

impl AppIdentity {
    /// A bundle id; an empty string is [`AppIdentity::unknown`].
    pub fn bundle_id(raw: String) -> AppIdentity {
        AppIdentity::entered(TextField::BundleId, raw)
    }

    /// An executable path; an empty string is [`AppIdentity::unknown`].
    pub fn executable_path(raw: String) -> AppIdentity {
        AppIdentity::entered(TextField::ExecutablePath, raw)
    }

    /// No identity could be read; the record holds `null`.
    pub const fn unknown() -> AppIdentity {
        AppIdentity(AppForm::Unknown)
    }

    pub fn is_unknown(&self) -> bool {
        matches!(self.0, AppForm::Unknown)
    }

    /// The captured string of the identity and its field, or `None` for an
    /// unknown app.
    pub(crate) fn text(&self) -> Option<(&CapturedText, TextField)> {
        match &self.0 {
            AppForm::BundleId(text) => Some((text, TextField::BundleId)),
            AppForm::ExecutablePath(text) => Some((text, TextField::ExecutablePath)),
            AppForm::Unknown => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn heap_bytes(&self) -> usize {
        match &self.0 {
            AppForm::BundleId(text) | AppForm::ExecutablePath(text) => text.heap_bytes(),
            AppForm::Unknown => 0,
        }
    }

    /// Whether a bundle id is one of `bundle_ids`, or the file name of an
    /// executable path one of `executables`, ignoring ASCII case.
    pub(crate) fn is_listed(&self, bundle_ids: &[&str], executables: &[&str]) -> bool {
        match &self.0 {
            AppForm::BundleId(text) => text.names_one_of(bundle_ids, false),
            AppForm::ExecutablePath(text) => text.names_one_of(executables, true),
            AppForm::Unknown => false,
        }
    }

    fn entered(field: TextField, raw: String) -> AppIdentity {
        if raw.is_empty() {
            return AppIdentity::unknown();
        }
        let text = CapturedText::enter(field, raw);
        AppIdentity(match field {
            TextField::ExecutablePath => AppForm::ExecutablePath(text),
            _ => AppForm::BundleId(text),
        })
    }
}

impl fmt::Debug for AppIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self.0 {
            AppForm::BundleId(_) => "AppIdentity(bundle id)",
            AppForm::ExecutablePath(_) => "AppIdentity(executable path)",
            AppForm::Unknown => "AppIdentity(unknown)",
        })
    }
}

/// Signed unix nanoseconds of `instant`: negative before 1970, saturating at
/// the bounds of `i64`. The one time codec of the floor's records.
pub fn unix_nanos(instant: SystemTime) -> i64 {
    match instant.duration_since(UNIX_EPOCH) {
        Ok(after) => i64::try_from(after.as_nanos()).unwrap_or(i64::MAX),
        Err(before) => i64::try_from(before.duration().as_nanos())
            .map(|nanos| -nanos)
            .unwrap_or(i64::MIN),
    }
}

/// The instant of signed unix nanoseconds, the inverse of [`unix_nanos`]; `None`
/// only where the platform's clock cannot hold the instant.
pub fn from_unix_nanos(nanos: i64) -> Option<SystemTime> {
    let offset = Duration::from_nanos(nanos.unsigned_abs());
    if nanos >= 0 {
        UNIX_EPOCH.checked_add(offset)
    } else {
        UNIX_EPOCH.checked_sub(offset)
    }
}

/// `nanos` rounded down to a multiple of `resolution`; at the lowest `i64`
/// values, where no lower multiple exists, the next multiple up.
pub(crate) fn coarsened(nanos: i64, resolution: Duration) -> i64 {
    let step = i64::try_from(resolution.as_nanos())
        .unwrap_or(i64::MAX)
        .max(1);
    let steps = nanos.div_euclid(step);
    steps
        .checked_mul(step)
        .unwrap_or_else(|| (steps + 1).saturating_mul(step))
}

/// The id of a stored record: a UUID of version 7 made from the record's
/// instant, clamped at the epoch, by the one maker of this module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EventId(Uuid);

impl EventId {
    fn made_at(nanos: i64, context: &Mutex<ContextV7>) -> EventId {
        let nanos = u64::try_from(nanos).unwrap_or(0);
        let seconds = nanos / 1_000_000_000;
        let subsec = u32::try_from(nanos % 1_000_000_000).unwrap_or(0);
        let context = context.lock().unwrap_or_else(PoisonError::into_inner);
        let timestamp = Timestamp::from_unix(&*context, seconds, subsec);
        EventId(Uuid::new_v7(timestamp))
    }

    pub const fn uuid(&self) -> Uuid {
        self.0
    }
}

/// The clock of the floor constructors and the id contexts that keep the ids
/// of one clock in order: one for records stored at
/// [`crate::floor::KEY_TIME_RESOLUTION`] and one for the others, so that no
/// id holds a finer time than its record. Sources use [`EventClock::system`].
pub struct EventClock {
    read: fn() -> SystemTime,
    fine: Mutex<ContextV7>,
    coarse: Mutex<ContextV7>,
}

static SYSTEM_CLOCK: EventClock = EventClock::new(SystemTime::now);

impl EventClock {
    /// A clock that reads `read`, with id contexts of its own.
    pub const fn new(read: fn() -> SystemTime) -> EventClock {
        EventClock {
            read,
            fine: Mutex::new(ContextV7::new()),
            coarse: Mutex::new(ContextV7::new()),
        }
    }

    /// The wall clock with the crate's id contexts.
    pub fn system() -> &'static EventClock {
        &SYSTEM_CLOCK
    }

    /// The reading moved back by `age`, never later than the reading, at the
    /// stored resolution of `observation`, with its id.
    fn stamp(&self, age: Option<Duration>, observation: &Observation) -> (i64, EventId) {
        let reading = unix_nanos((self.read)());
        let age = age.map_or(0, |age| i64::try_from(age.as_nanos()).unwrap_or(i64::MAX));
        let resolution = floor::resolution_of(observation);
        let at = coarsened(reading.saturating_sub(age), resolution);
        (at, self.id_at(at, resolution))
    }

    /// The id of a record at `at`, stored at `resolution`, from the id context
    /// of that resolution.
    pub(crate) fn id_at(&self, at: i64, resolution: Duration) -> EventId {
        let context = if resolution == floor::TIME_RESOLUTION {
            &self.fine
        } else {
            &self.coarse
        };
        EventId::made_at(at, context)
    }
}

/// The value that a capture source emits: what it observed, the app and the
/// process it belongs to, its instant and its id. It holds no number and no
/// placement; the sink numbers it. Each constructor takes the keep verdict of
/// its kind, reads the clock and moves the reading back by `age`, the age of
/// the OS event. It prints a placeholder.
pub struct FloorEvent {
    id: EventId,
    at: i64,
    app: AppIdentity,
    pid: ProcessId,
    observation: Observation,
}

impl FloorEvent {
    pub fn foreground_app(
        keep: Keep<kind::ForegroundApp>,
        age: Option<Duration>,
        clock: &EventClock,
    ) -> FloorEvent {
        FloorEvent::made(keep.into_parts().0, Observation::ForegroundApp, age, clock)
    }

    /// The site of `title` is kept only when the switch `site_read` was on for
    /// the verdict.
    pub fn window_title(
        keep: Keep<kind::WindowTitle>,
        mut title: Title,
        age: Option<Duration>,
        clock: &EventClock,
    ) -> FloorEvent {
        let (subject, site_read) = keep.into_parts();
        if !site_read {
            title.site = None;
        }
        FloorEvent::made(subject, Observation::WindowTitle(title), age, clock)
    }

    /// A keystroke needs the clearance of [`crate::secure_input::check`] for the
    /// key, given by value; its age is the age that the check judged.
    pub fn keystroke(
        keep: Keep<kind::Keystroke>,
        clearance: KeystrokeClearance,
        class: KeyClass,
        clock: &EventClock,
    ) -> FloorEvent {
        let age = clearance.key_age();
        FloorEvent::made(
            keep.into_parts().0,
            Observation::Keystroke(class),
            age,
            clock,
        )
    }

    pub fn click(
        keep: Keep<kind::Pointer>,
        button: Button,
        age: Option<Duration>,
        clock: &EventClock,
    ) -> FloorEvent {
        FloorEvent::made(keep.into_parts().0, Observation::Click(button), age, clock)
    }

    pub fn screen_frame(
        keep: Keep<kind::ScreenFrame>,
        age: Option<Duration>,
        clock: &EventClock,
    ) -> FloorEvent {
        FloorEvent::made(keep.into_parts().0, Observation::ScreenFrame, age, clock)
    }

    fn made(
        subject: Subject,
        observation: Observation,
        age: Option<Duration>,
        clock: &EventClock,
    ) -> FloorEvent {
        let (app, pid) = subject.into_parts();
        let (at, id) = clock.stamp(age, &observation);
        FloorEvent {
            id,
            at,
            app,
            pid,
            observation,
        }
    }

    pub fn kind(&self) -> Modality {
        self.observation.kind()
    }

    pub fn id(&self) -> EventId {
        self.id
    }

    /// The instant of the event in signed unix nanoseconds, at the stored
    /// resolution of its kind.
    pub fn at(&self) -> i64 {
        self.at
    }

    pub(crate) fn app(&self) -> &AppIdentity {
        &self.app
    }

    pub(crate) fn pid(&self) -> ProcessId {
        self.pid
    }

    pub(crate) fn observation(&self) -> &Observation {
        &self.observation
    }

    /// The bytes that the captured strings of the event hold on the heap.
    #[cfg(test)]
    pub(crate) fn heap_bytes(&self) -> usize {
        let title = match &self.observation {
            Observation::WindowTitle(title) => title.text.heap_bytes(),
            _ => 0,
        };
        self.app.heap_bytes() + title + self.site_heap_bytes()
    }

    #[cfg(test)]
    pub(crate) fn site_heap_bytes(&self) -> usize {
        match &self.observation {
            Observation::WindowTitle(title) => {
                title.site.as_ref().map_or(0, |site| site.heap_bytes())
            }
            _ => 0,
        }
    }
}

impl fmt::Debug for FloorEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("FloorEvent(..)")
    }
}

/// A fold record: the one value that
/// [`crate::sink::SinkHandle::emit_continuous`] takes.
pub struct FoldEvent(FloorEvent);

impl FoldEvent {
    pub fn new(
        keep: Keep<kind::Pointer>,
        fold: PointerFold,
        age: Option<Duration>,
        clock: &EventClock,
    ) -> FoldEvent {
        FoldEvent(FloorEvent::made(
            keep.into_parts().0,
            Observation::Fold(fold),
            age,
            clock,
        ))
    }

    /// The class and the count, which a source adds to its next interval when a
    /// full queue hands the record back.
    pub fn fold(&self) -> PointerFold {
        match self.0.observation {
            Observation::Fold(fold) => fold,
            _ => unreachable!("a fold event holds a fold"),
        }
    }

    pub(crate) fn into_event(self) -> FloorEvent {
        self.0
    }
}

impl fmt::Debug for FoldEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("FoldEvent(..)")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetentionClass {
    Raw,
    Derived,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct MonotonicSequence(u64);

impl MonotonicSequence {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapturedAt {
    pub wall_clock: SystemTime,
    pub sequence: MonotonicSequence,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SegmentRef {
    pub segment_id: Uuid,
    pub byte_offset: u64,
    pub byte_len: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SegmentKeyId(pub i64);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureStreamEvent {
    pub event_id: Uuid,
    pub captured_at: CapturedAt,
    pub modality: Modality,
    pub foreground_app: String,
    pub segment_ref: SegmentRef,
    pub segment_key_id: SegmentKeyId,
    pub is_derived: bool,
    pub retention_class: RetentionClass,
}

impl CaptureStreamEvent {
    pub fn new_raw(
        captured_at: CapturedAt,
        modality: Modality,
        foreground_app: String,
        segment_ref: SegmentRef,
        segment_key_id: SegmentKeyId,
    ) -> Self {
        Self {
            event_id: Uuid::now_v7(),
            captured_at,
            modality,
            foreground_app,
            segment_ref,
            segment_key_id,
            is_derived: false,
            retention_class: RetentionClass::Raw,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::UNIX_EPOCH;

    #[test]
    fn constructs_event_with_every_field() {
        let event = CaptureStreamEvent {
            event_id: Uuid::now_v7(),
            captured_at: CapturedAt {
                wall_clock: UNIX_EPOCH,
                sequence: MonotonicSequence::new(1),
            },
            modality: Modality::Keystroke,
            foreground_app: "com.apple.Terminal".to_string(),
            segment_ref: SegmentRef {
                segment_id: Uuid::now_v7(),
                byte_offset: 0,
                byte_len: 128,
            },
            segment_key_id: SegmentKeyId(7),
            is_derived: false,
            retention_class: RetentionClass::Raw,
        };

        assert_eq!(event.modality, Modality::Keystroke);
        assert_eq!(event.retention_class, RetentionClass::Raw);
        assert!(!event.is_derived);
        assert_eq!(event.foreground_app, "com.apple.Terminal");
        assert_eq!(event.segment_ref.byte_len, 128);
        assert_eq!(event.segment_key_id.0, 7);
        assert_eq!(event.captured_at.sequence.get(), 1);
    }

    #[test]
    fn new_raw_is_raw_and_not_derived() {
        let event = CaptureStreamEvent::new_raw(
            CapturedAt {
                wall_clock: UNIX_EPOCH,
                sequence: MonotonicSequence::new(0),
            },
            Modality::ScreenFrame,
            String::new(),
            SegmentRef {
                segment_id: Uuid::now_v7(),
                byte_offset: 0,
                byte_len: 0,
            },
            SegmentKeyId(1),
        );
        assert!(!event.is_derived);
        assert_eq!(event.retention_class, RetentionClass::Raw);
    }

    fn stored_app(app: &AppIdentity) -> Option<(TextField, String)> {
        match &app.0 {
            AppForm::BundleId(text) => Some((
                TextField::BundleId,
                crate::floor::cases::stored(text, TextField::BundleId),
            )),
            AppForm::ExecutablePath(text) => Some((
                TextField::ExecutablePath,
                crate::floor::cases::stored(text, TextField::ExecutablePath),
            )),
            AppForm::Unknown => None,
        }
    }

    #[test]
    fn a_64_kib_app_id_is_cut_to_the_cap_on_a_character_boundary() {
        let bundle = format!("com.example.{}", "\u{e9}".repeat(32 * 1024));
        assert!(bundle.len() > 64 * 1024);
        let (field, kept) = stored_app(&AppIdentity::bundle_id(bundle.clone())).unwrap();
        assert_eq!(field, TextField::BundleId);
        assert!(kept.len() <= 1024 && kept.len() >= 1021, "{}", kept.len());
        assert!(bundle.starts_with(&kept));

        let path = format!(
            "/Applications/{}/Tool.app/Contents/MacOS/tool",
            "\u{e9}".repeat(32 * 1024)
        );
        let (field, kept) = stored_app(&AppIdentity::executable_path(path.clone())).unwrap();
        assert_eq!(field, TextField::ExecutablePath);
        assert!(kept.len() <= 1024 && kept.len() >= 1021, "{}", kept.len());
        assert!(path.ends_with(&kept));
        assert!(kept.ends_with("/Contents/MacOS/tool"));
    }

    #[test]
    fn an_unknown_app_is_unknown_not_an_empty_string() {
        assert!(AppIdentity::bundle_id(String::new()).is_unknown());
        assert!(AppIdentity::executable_path(String::new()).is_unknown());
        assert!(AppIdentity::unknown().is_unknown());
        assert!(stored_app(&AppIdentity::bundle_id(String::new())).is_none());
        let named = AppIdentity::bundle_id("com.apple.Terminal".to_string());
        assert!(!named.is_unknown());
        assert_eq!(
            stored_app(&named),
            Some((TextField::BundleId, "com.apple.Terminal".to_string()))
        );
    }

    use crate::floor::cases::keep;
    use crate::floor::payload::{FoldClass, FoldCount, TitleWhy};
    use crate::floor::site::Site;
    use crate::floor::{KEY_TIME_RESOLUTION, TIME_RESOLUTION};
    use crate::secure_input::{check, FixedProbe, SecureInputReading};

    const SECOND: i64 = 1_000_000_000;
    const MILLI: i64 = 1_000_000;

    fn terminal() -> AppIdentity {
        AppIdentity::bundle_id("com.apple.Terminal".to_string())
    }

    fn id_millis(id: EventId) -> i64 {
        let (seconds, nanos) = id.uuid().get_timestamp().unwrap().to_unix();
        i64::try_from(seconds * 1000 + u64::from(nanos) / 1_000_000).unwrap()
    }

    fn app_event(age: Option<Duration>, clock: &EventClock) -> FloorEvent {
        FloorEvent::foreground_app(keep(terminal(), 7), age, clock)
    }

    fn key_event(age: Option<Duration>, clock: &EventClock) -> FloorEvent {
        let keep = keep(terminal(), 7);
        let cleared = check(&FixedProbe(SecureInputReading::Inactive), age).unwrap();
        FloorEvent::keystroke(keep, cleared, KeyClass::Printable, clock)
    }

    #[test]
    fn a_clock_before_1970_goes_through_the_constructor() {
        fn before_1970() -> SystemTime {
            UNIX_EPOCH - Duration::from_secs(86_400 * 365) - Duration::from_millis(250)
        }
        let clock = EventClock::new(before_1970);
        let event = app_event(None, &clock);
        assert_eq!(event.at(), -(86_400 * 365 * SECOND + 250 * MILLI));
        assert_eq!(from_unix_nanos(event.at()), Some(before_1970()));
        assert_eq!(event.id().uuid().get_version_num(), 7);
        assert_eq!(id_millis(event.id()), 0);
        let key = key_event(None, &clock);
        assert_eq!(key.at(), -(86_400 * 365 + 1) * SECOND);
        assert_eq!(key.id().uuid().get_version_num(), 7);
        assert_eq!(id_millis(key.id()), 0);
    }

    #[test]
    fn the_codec_saturates_at_the_i64_bounds() {
        assert_eq!(unix_nanos(UNIX_EPOCH), 0);
        let largest = UNIX_EPOCH + Duration::from_nanos(i64::MAX.unsigned_abs());
        let smallest = UNIX_EPOCH - Duration::from_nanos(i64::MIN.unsigned_abs());
        let day = Duration::from_secs(86_400);
        if let Some(later) = largest.checked_add(day) {
            assert_eq!(unix_nanos(later), i64::MAX);
        }
        if let Some(earlier) = smallest.checked_sub(day) {
            assert_eq!(unix_nanos(earlier), i64::MIN);
        }
        assert!(unix_nanos(largest) > i64::MAX - 100);
        assert!(unix_nanos(smallest) < i64::MIN + 100);
        for nanos in [
            -9_223_372_036_854_775_800,
            -1_500_000_000,
            -100,
            0,
            100,
            1_700_000_000_123_456_700,
            9_223_372_036_854_775_800,
        ] {
            assert_eq!(
                from_unix_nanos(nanos).map(unix_nanos),
                Some(nanos),
                "{nanos}"
            );
        }
        assert_eq!(coarsened(-1_500_000_000, KEY_TIME_RESOLUTION), -2 * SECOND);
        assert_eq!(coarsened(1_500_000_000, KEY_TIME_RESOLUTION), SECOND);
        assert_eq!(coarsened(-1, TIME_RESOLUTION), -MILLI);
        assert_eq!(
            coarsened(i64::MAX, KEY_TIME_RESOLUTION),
            9_223_372_036 * SECOND
        );
        let bottom = coarsened(i64::MIN, KEY_TIME_RESOLUTION);
        assert_eq!(bottom, -9_223_372_036 * SECOND);
        assert_eq!(coarsened(i64::MIN, Duration::from_nanos(1)), i64::MIN);
    }

    #[test]
    fn the_stamp_lies_between_two_clock_reads() {
        let clock = EventClock::new(SystemTime::now);
        let first = unix_nanos(SystemTime::now());
        let event = app_event(None, &clock);
        let key = key_event(None, &clock);
        let last = unix_nanos(SystemTime::now());
        assert!(first > 1_600_000_000 * SECOND);
        assert!(coarsened(first, TIME_RESOLUTION) <= event.at() && event.at() <= last);
        assert!(coarsened(first, KEY_TIME_RESOLUTION) <= key.at() && key.at() <= last);
        assert_eq!(id_millis(event.id()) * MILLI, event.at());
        assert_eq!(id_millis(key.id()) * MILLI, key.at());
    }

    #[test]
    fn an_age_moves_the_stamp_back() {
        fn fixed() -> SystemTime {
            UNIX_EPOCH + Duration::from_secs(1_800_000_000) + Duration::from_millis(700)
        }
        let now = unix_nanos(fixed());
        let aged = app_event(Some(Duration::from_millis(300)), &EventClock::new(fixed));
        assert_eq!(aged.at(), now - 300 * MILLI);
        assert_eq!(id_millis(aged.id()) * MILLI, aged.at());
        let clock = EventClock::new(fixed);
        let newer = app_event(None, &clock);
        assert_eq!(newer.at(), now);
        let older = app_event(Some(Duration::from_millis(300)), &clock);
        assert_eq!(older.at(), now - 300 * MILLI);
        assert_eq!(id_millis(older.id()), id_millis(newer.id()));
        assert!(older.id().uuid() > newer.id().uuid());
        fn early() -> SystemTime {
            UNIX_EPOCH + Duration::from_secs(1_800_000_000) + Duration::from_millis(100)
        }
        let key = key_event(Some(Duration::from_millis(200)), &EventClock::new(early));
        assert_eq!(key.at(), 1_799_999_999 * SECOND);
        let oldest = app_event(Some(Duration::MAX), &EventClock::new(fixed));
        assert!(oldest.at() < 0);
        assert_eq!(id_millis(oldest.id()), 0);
    }

    #[test]
    fn a_keystroke_takes_its_age_from_the_clearance() {
        fn early() -> SystemTime {
            UNIX_EPOCH + Duration::from_secs(1_800_000_000) + Duration::from_millis(100)
        }
        let probe = FixedProbe(SecureInputReading::Inactive);
        let aged = check(&probe, Some(Duration::from_millis(200))).unwrap();
        assert_eq!(aged.key_age(), Some(Duration::from_millis(200)));
        let key = FloorEvent::keystroke(
            keep(terminal(), 7),
            aged,
            KeyClass::Tab,
            &EventClock::new(early),
        );
        assert_eq!(key.at(), 1_799_999_999 * SECOND);
        let on_time = check(&probe, None).unwrap();
        assert_eq!(on_time.key_age(), None);
        let key = FloorEvent::keystroke(
            keep(terminal(), 7),
            on_time,
            KeyClass::Tab,
            &EventClock::new(early),
        );
        assert_eq!(key.at(), 1_800_000_000 * SECOND);
    }

    #[test]
    fn every_id_is_version_7() {
        let clock = EventClock::system();
        let site = Site::cut("https://www.example.com/".to_string());
        let title = Title::new("Inbox".to_string(), TitleWhy::AppChange, site);
        let fold = PointerFold::new(FoldClass::Scroll, FoldCount::one());
        let ids = [
            app_event(None, clock).id(),
            key_event(None, clock).id(),
            FloorEvent::window_title(keep(terminal(), 7), title, None, clock).id(),
            FloorEvent::click(keep(terminal(), 7), Button::Primary, None, clock).id(),
            FloorEvent::screen_frame(keep(terminal(), 7), None, clock).id(),
            FoldEvent::new(keep(terminal(), 7), fold, None, clock)
                .into_event()
                .id(),
        ];
        for (index, id) in ids.iter().enumerate() {
            assert_eq!(id.uuid().get_version_num(), 7, "{index}");
            assert_eq!(id.uuid().get_variant(), uuid::Variant::RFC4122, "{index}");
            assert!(!ids[..index].contains(id), "{index}");
        }
    }

    #[test]
    fn debug_text_holds_no_captured_string() {
        let name = "com.example.SecretTool";
        let words = "Quarterly salaries of the board";
        let address = "https://payroll.example.org/run";
        let site = || Site::cut(address.to_string()).unwrap();
        let entered = || words.to_string();
        let app = || AppIdentity::bundle_id(name.to_string());
        let title = Title::new(entered(), TitleWhy::TitleChange, Some(site()));
        let clock = EventClock::system();
        let event = FloorEvent::window_title(keep(app(), 7), title, None, clock);
        let fold = PointerFold::new(FoldClass::Drag, FoldCount::one());
        let shown = [
            format!("{:?}", app()),
            format!(
                "{:?}",
                AppIdentity::executable_path(format!("C:\\{name}.exe"))
            ),
            format!("{:?}", CapturedText::enter(TextField::Title, entered())),
            format!("{:?}", site()),
            format!(
                "{:?}",
                Title::new(entered(), TitleWhy::AppChange, Some(site()))
            ),
            format!("{event:?}"),
            format!("{:?}", FoldEvent::new(keep(app(), 7), fold, None, clock)),
            format!("{:?}", Subject::new(app(), ProcessId::new(4242))),
            format!("{:?}", keep::<kind::WindowTitle>(app(), 7)),
        ];
        for text in &shown {
            for planted in ["Secret", "salaries", "payroll", "example", "4242"] {
                assert!(!text.contains(planted), "{text} holds {planted}");
            }
            assert!(
                !text.chars().any(|c| c.is_ascii_digit()),
                "{text} holds a number"
            );
        }
    }
}
