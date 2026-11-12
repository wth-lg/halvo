//! The stored model that both binaries share: the capture kinds, the capture
//! state, the UTC day, the inventory, the storage cap, the observation window and
//! the delete report.

use std::fmt;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// The bundle id of the app (RD-08): the login-item label and a component of the
/// Windows data directory.
pub const APP_BUNDLE_ID: &str = "com.halvo.desktop";

/// A spelling that the one codec does not know.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("not a spelling of a {what}")]
pub struct UnknownSpelling {
    what: &'static str,
}

fn from_text<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: FromStr,
    T::Err: fmt::Display,
{
    let text = String::deserialize(deserializer)?;
    text.parse().map_err(serde::de::Error::custom)
}

/// What a capture event observed. Closed set; adding a modality is a code change,
/// not an open string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Modality {
    ScreenFrame,
    Keystroke,
    Pointer,
    ForegroundApp,
    WindowTitle,
    SystemAudio,
    Microphone,
    Camera,
}

impl Modality {
    /// Every kind, in the order of contracts.md 0.2.
    pub const ALL: [Modality; 8] = [
        Modality::ScreenFrame,
        Modality::Keystroke,
        Modality::Pointer,
        Modality::ForegroundApp,
        Modality::WindowTitle,
        Modality::SystemAudio,
        Modality::Microphone,
        Modality::Camera,
    ];

    /// The one spelling of the kind (contracts.md 0.2): serde, the store and every
    /// text line write this and read nothing else.
    pub const fn as_str(self) -> &'static str {
        match self {
            Modality::ScreenFrame => "screen_frame",
            Modality::Keystroke => "keystroke",
            Modality::Pointer => "pointer",
            Modality::ForegroundApp => "foreground_app",
            Modality::WindowTitle => "window_title",
            Modality::SystemAudio => "system_audio",
            Modality::Microphone => "microphone",
            Modality::Camera => "camera",
        }
    }

    /// Whether the sink may drop this modality under backpressure. Continuous
    /// high-rate streams (frames, audio, camera) are droppable; discrete input
    /// and context events are not, so a keystroke is never lost to backpressure.
    pub fn is_droppable_under_backpressure(self) -> bool {
        matches!(
            self,
            Modality::ScreenFrame | Modality::SystemAudio | Modality::Microphone | Modality::Camera
        )
    }
}

impl FromStr for Modality {
    type Err = UnknownSpelling;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Modality::ALL
            .into_iter()
            .find(|kind| kind.as_str() == text)
            .ok_or(UnknownSpelling { what: "kind" })
    }
}

impl Serialize for Modality {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Modality {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        from_text(deserializer)
    }
}

/// Whether capture is currently running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureState {
    Running,
    Paused,
}

impl CaptureState {
    /// The one spelling of the state.
    pub const fn as_str(self) -> &'static str {
        match self {
            CaptureState::Running => "running",
            CaptureState::Paused => "paused",
        }
    }
}

impl FromStr for CaptureState {
    type Err = UnknownSpelling;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        [CaptureState::Running, CaptureState::Paused]
            .into_iter()
            .find(|state| state.as_str() == text)
            .ok_or(UnknownSpelling {
                what: "capture state",
            })
    }
}

impl Serialize for CaptureState {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for CaptureState {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        from_text(deserializer)
    }
}

/// The fixed observation window: what is kept and when the first report is due.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservationWindow {
    pub rolling_days: u32,
    pub first_report_day: u32,
}

impl ObservationWindow {
    pub const ROLLING_DAYS: u32 = 14;
    pub const FIRST_REPORT_DAY: u32 = 7;

    /// The R1 window: 14 rolling days, the first report on day 7.
    pub const fn r1() -> Self {
        Self {
            rolling_days: Self::ROLLING_DAYS,
            first_report_day: Self::FIRST_REPORT_DAY,
        }
    }
}

/// Why a storage cap was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CapError {
    #[error("the storage cap must be at least {min} bytes")]
    TooSmall { min: u64 },
}

/// The largest number of bytes the store may hold before the oldest days are
/// crypto-shredded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct StorageCap(u64);

impl StorageCap {
    pub const GIB: u64 = 1024 * 1024 * 1024;
    pub const ESTIMATED_BYTES_PER_DAY: u64 = Self::GIB;
    pub const MIN: StorageCap = StorageCap(Self::ESTIMATED_BYTES_PER_DAY);
    const HEADROOM_BYTES: u64 = 11 * Self::GIB;
    pub const DEFAULT: StorageCap = StorageCap(
        ObservationWindow::ROLLING_DAYS as u64 * Self::ESTIMATED_BYTES_PER_DAY
            + Self::HEADROOM_BYTES,
    );

    /// A cap of `bytes`, or the bound it breaks.
    pub fn from_bytes(bytes: u64) -> Result<Self, CapError> {
        if bytes < Self::MIN.0 {
            return Err(CapError::TooSmall { min: Self::MIN.0 });
        }
        Ok(Self(bytes))
    }

    pub const fn bytes(self) -> u64 {
        self.0
    }
}

impl Default for StorageCap {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl<'de> Deserialize<'de> for StorageCap {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let bytes = u64::deserialize(deserializer)?;
        Self::from_bytes(bytes).map_err(serde::de::Error::custom)
    }
}

/// Why a day was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DayError {
    #[error("not a date in the form YYYY-MM-DD")]
    Format,
    #[error("the date lies outside 0000-01-01 to 9999-12-31")]
    OutOfRange,
    #[error("the month has no such day")]
    NoSuchDay,
}

/// A UTC calendar day from 0000-01-01 to 9999-12-31, counted in days since
/// 1970-01-01. Every constructor refuses a day outside that range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UtcDay(i64);

const SECONDS_PER_DAY: u64 = 86_400;
const DAYS_PER_YEAR: i64 = 365;
const DAYS_PER_4_YEARS: i64 = 4 * DAYS_PER_YEAR + 1;
const DAYS_PER_100_YEARS: i64 = 25 * DAYS_PER_4_YEARS - 1;
const YEARS_PER_ERA: i64 = 400;
const DAYS_PER_ERA: i64 = 4 * DAYS_PER_100_YEARS + 1;
const DAYS_PER_5_MONTHS_FROM_MARCH: i64 = 31 + 30 + 31 + 30 + 31;
const MONTHS_FROM_MARCH_TO_JANUARY: i64 = 10;
const DAYS_FROM_0000_03_01_TO_UNIX_EPOCH: i64 = 719_468;

const MIN_YEAR: i64 = 0;
const MAX_YEAR: i64 = 9999;
const MIN_DAYS: i64 = -719_528;
const MAX_DAYS: i64 = 2_932_896;

impl UtcDay {
    /// 0000-01-01.
    pub const MIN: UtcDay = UtcDay(MIN_DAYS);
    /// 9999-12-31.
    pub const MAX: UtcDay = UtcDay(MAX_DAYS);

    pub fn from_days_since_epoch(days: i64) -> Result<Self, DayError> {
        if (MIN_DAYS..=MAX_DAYS).contains(&days) {
            Ok(Self(days))
        } else {
            Err(DayError::OutOfRange)
        }
    }

    pub const fn days_since_epoch(self) -> i64 {
        self.0
    }

    /// The UTC day that holds `at`, rounded down: one nanosecond before the epoch
    /// is day -1.
    pub fn from_system_time(at: SystemTime) -> Result<Self, DayError> {
        let days = match at.duration_since(UNIX_EPOCH) {
            Ok(after) => i64::try_from(after.as_secs() / SECONDS_PER_DAY)
                .map_err(|_| DayError::OutOfRange)?,
            Err(before) => {
                let before = before.duration();
                let whole = before.as_secs() / SECONDS_PER_DAY;
                let part = before.as_secs() % SECONDS_PER_DAY != 0 || before.subsec_nanos() != 0;
                let days =
                    i64::try_from(whole + u64::from(part)).map_err(|_| DayError::OutOfRange)?;
                -days
            }
        };
        Self::from_days_since_epoch(days)
    }

    /// The day after, or `None` after 9999-12-31.
    pub fn next(self) -> Option<Self> {
        Self::from_days_since_epoch(self.0.checked_add(1)?).ok()
    }

    /// The proleptic Gregorian year, month and day.
    pub fn to_year_month_day(self) -> (i64, u32, u32) {
        let days_from_0000_03_01 = self.0 + DAYS_FROM_0000_03_01_TO_UNIX_EPOCH;
        let era = if days_from_0000_03_01 >= 0 {
            days_from_0000_03_01
        } else {
            days_from_0000_03_01 - (DAYS_PER_ERA - 1)
        } / DAYS_PER_ERA;
        let day_of_era = days_from_0000_03_01 - era * DAYS_PER_ERA;
        let year_of_era = (day_of_era - day_of_era / (DAYS_PER_4_YEARS - 1)
            + day_of_era / DAYS_PER_100_YEARS
            - day_of_era / (DAYS_PER_ERA - 1))
            / DAYS_PER_YEAR;
        let day_of_march_year =
            day_of_era - (DAYS_PER_YEAR * year_of_era + year_of_era / 4 - year_of_era / 100);
        let month_from_march = (5 * day_of_march_year + 2) / DAYS_PER_5_MONTHS_FROM_MARCH;
        let day = (day_of_march_year - (DAYS_PER_5_MONTHS_FROM_MARCH * month_from_march + 2) / 5
            + 1) as u32;
        let month = if month_from_march < MONTHS_FROM_MARCH_TO_JANUARY {
            month_from_march + 3
        } else {
            month_from_march - 9
        } as u32;
        let year = year_of_era + era * YEARS_PER_ERA + i64::from(month <= 2);
        (year, month, day)
    }

    /// The day of a calendar date. The year range is checked before any
    /// arithmetic, and a day the month lacks is refused by converting back.
    pub fn from_year_month_day(year: i64, month: u32, day: u32) -> Result<Self, DayError> {
        if !(MIN_YEAR..=MAX_YEAR).contains(&year) {
            return Err(DayError::OutOfRange);
        }
        if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return Err(DayError::NoSuchDay);
        }
        let days = days_from_civil(year, month, day).ok_or(DayError::OutOfRange)?;
        let found = Self(days);
        if found.to_year_month_day() != (year, month, day) {
            return Err(DayError::NoSuchDay);
        }
        Ok(found)
    }
}

fn days_from_civil(year: i64, month: u32, day: u32) -> Option<i64> {
    let march_year = if month <= 2 {
        year.checked_sub(1)?
    } else {
        year
    };
    let era = if march_year >= 0 {
        march_year
    } else {
        march_year.checked_sub(YEARS_PER_ERA - 1)?
    }
    .checked_div(YEARS_PER_ERA)?;
    let year_of_era = march_year.checked_sub(era.checked_mul(YEARS_PER_ERA)?)?;
    let month_from_march = if month > 2 {
        i64::from(month) - 3
    } else {
        i64::from(month) + 9
    };
    let day_of_march_year = DAYS_PER_5_MONTHS_FROM_MARCH
        .checked_mul(month_from_march)?
        .checked_add(2)?
        .checked_div(5)?
        .checked_add(i64::from(day))?
        .checked_sub(1)?;
    let day_of_era = year_of_era
        .checked_mul(DAYS_PER_YEAR)?
        .checked_add(year_of_era / 4)?
        .checked_sub(year_of_era / 100)?
        .checked_add(day_of_march_year)?;
    era.checked_mul(DAYS_PER_ERA)?
        .checked_add(day_of_era)?
        .checked_sub(DAYS_FROM_0000_03_01_TO_UNIX_EPOCH)
}

impl fmt::Display for UtcDay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (y, m, d) = self.to_year_month_day();
        write!(f, "{y:04}-{m:02}-{d:02}")
    }
}

impl FromStr for UtcDay {
    type Err = DayError;

    /// Exactly ten ASCII characters, YYYY-MM-DD, with no sign.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = s.as_bytes();
        if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
            return Err(DayError::Format);
        }
        let number = |digits: &[u8]| {
            digits.iter().try_fold(0u32, |value, &digit| {
                if digit.is_ascii_digit() {
                    Ok(value * 10 + u32::from(digit - b'0'))
                } else {
                    Err(DayError::Format)
                }
            })
        };
        let year = number(&bytes[0..4])?;
        let month = number(&bytes[5..7])?;
        let day = number(&bytes[8..10])?;
        Self::from_year_month_day(i64::from(year), month, day)
    }
}

impl Serialize for UtcDay {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for UtcDay {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        from_text(deserializer)
    }
}

/// The events and bytes of one capture kind on one UTC day.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryRow {
    pub day: UtcDay,
    pub modality: Modality,
    pub events: u64,
    pub bytes: u64,
}

/// The per-day rollup of the store, oldest day first and, within a day, in the
/// kind order of contracts.md 0.2.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct Inventory {
    rows: Vec<InventoryRow>,
}

impl Inventory {
    /// The inventory of `rows`, put in day and kind order.
    pub fn from_rows(mut rows: Vec<InventoryRow>) -> Self {
        rows.sort_by(|a, b| {
            a.day
                .cmp(&b.day)
                .then_with(|| modality_order(a.modality).cmp(&modality_order(b.modality)))
        });
        Self { rows }
    }

    pub fn rows(&self) -> &[InventoryRow] {
        &self.rows
    }

    pub fn total_bytes(&self) -> u64 {
        self.rows.iter().map(|r| r.bytes).sum()
    }

    pub fn total_events(&self) -> u64 {
        self.rows.iter().map(|r| r.events).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

impl<'de> Deserialize<'de> for Inventory {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Rows {
            rows: Vec<InventoryRow>,
        }
        let Rows { rows } = Rows::deserialize(deserializer)?;
        Ok(Self::from_rows(rows))
    }
}

fn modality_order(modality: Modality) -> u8 {
    match modality {
        Modality::ScreenFrame => 0,
        Modality::Keystroke => 1,
        Modality::Pointer => 2,
        Modality::ForegroundApp => 3,
        Modality::WindowTitle => 4,
        Modality::SystemAudio => 5,
        Modality::Microphone => 6,
        Modality::Camera => 7,
    }
}

/// What a delete-all reports: the segments it shredded and the keys it destroyed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShredView {
    pub segments_shredded: u64,
    pub keys_destroyed: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn the_window_is_fixed_and_not_a_setting() {
        assert_eq!(ObservationWindow::ROLLING_DAYS, 14);
        assert_eq!(ObservationWindow::FIRST_REPORT_DAY, 7);
    }

    const KIND_SPELLINGS: [(Modality, &str); 8] = [
        (Modality::ScreenFrame, "screen_frame"),
        (Modality::Keystroke, "keystroke"),
        (Modality::Pointer, "pointer"),
        (Modality::ForegroundApp, "foreground_app"),
        (Modality::WindowTitle, "window_title"),
        (Modality::SystemAudio, "system_audio"),
        (Modality::Microphone, "microphone"),
        (Modality::Camera, "camera"),
    ];

    #[test]
    fn each_kind_has_one_spelling_for_as_str_serde_and_from_str() {
        assert_eq!(KIND_SPELLINGS.map(|(kind, _)| kind), Modality::ALL);
        for (kind, text) in KIND_SPELLINGS {
            let quoted = format!("\"{text}\"");
            assert_eq!(kind.as_str(), text);
            assert_eq!(serde_json::to_string(&kind).unwrap(), quoted);
            assert_eq!(text.parse::<Modality>(), Ok(kind));
            assert_eq!(serde_json::from_str::<Modality>(&quoted).unwrap(), kind);
        }
    }

    #[test]
    fn a_kind_spelling_the_codec_does_not_know_is_an_error() {
        for text in [
            "ScreenFrame",
            "Keystroke",
            "ForegroundApp",
            "screen-frame",
            "SCREEN_FRAME",
            "Screen_frame",
            " screen_frame",
            "screen_frame ",
            "screenframe",
            "run_mark",
            "",
        ] {
            assert!(text.parse::<Modality>().is_err(), "{text:?}");
            let quoted = format!("\"{text}\"");
            assert!(
                serde_json::from_str::<Modality>(&quoted).is_err(),
                "{text:?}"
            );
        }
    }

    #[test]
    fn each_capture_state_has_one_spelling_for_as_str_serde_and_from_str() {
        for (state, text) in [
            (CaptureState::Running, "running"),
            (CaptureState::Paused, "paused"),
        ] {
            let quoted = format!("\"{text}\"");
            assert_eq!(state.as_str(), text);
            assert_eq!(serde_json::to_string(&state).unwrap(), quoted);
            assert_eq!(text.parse::<CaptureState>(), Ok(state));
            assert_eq!(
                serde_json::from_str::<CaptureState>(&quoted).unwrap(),
                state
            );
        }
        for text in ["Running", "Paused", "RUNNING", "pausing", "stopped", ""] {
            assert!(text.parse::<CaptureState>().is_err(), "{text:?}");
            let quoted = format!("\"{text}\"");
            assert!(
                serde_json::from_str::<CaptureState>(&quoted).is_err(),
                "{text:?}"
            );
        }
    }

    #[test]
    fn frames_are_droppable_but_input_is_not() {
        assert!(Modality::ScreenFrame.is_droppable_under_backpressure());
        assert!(Modality::SystemAudio.is_droppable_under_backpressure());
        assert!(!Modality::Keystroke.is_droppable_under_backpressure());
        assert!(!Modality::Pointer.is_droppable_under_backpressure());
        assert!(!Modality::ForegroundApp.is_droppable_under_backpressure());
    }

    #[test]
    fn the_default_cap_holds_the_rolling_window_and_matches_recall_smallest_default() {
        let window_bytes =
            u64::from(ObservationWindow::ROLLING_DAYS) * StorageCap::ESTIMATED_BYTES_PER_DAY;
        assert!(StorageCap::DEFAULT.bytes() > window_bytes);
        assert_eq!(StorageCap::DEFAULT.bytes(), 25 * StorageCap::GIB);
        assert_eq!(StorageCap::MIN.bytes(), StorageCap::ESTIMATED_BYTES_PER_DAY);
    }

    #[test]
    fn a_stored_cap_below_the_minimum_is_an_error() {
        let parse = |bytes: u64| serde_json::from_str::<StorageCap>(&bytes.to_string());
        for bytes in [0, 5, StorageCap::MIN.bytes() - 1] {
            assert!(parse(bytes).is_err(), "{bytes}");
        }
        assert_eq!(parse(StorageCap::MIN.bytes()).unwrap(), StorageCap::MIN);
        assert_eq!(
            parse(2 * StorageCap::GIB).unwrap().bytes(),
            2 * StorageCap::GIB
        );
        let json = serde_json::to_string(&StorageCap::DEFAULT).unwrap();
        assert_eq!(json, StorageCap::DEFAULT.bytes().to_string());
    }

    #[test]
    fn a_cap_below_the_minimum_is_rejected() {
        assert!(StorageCap::from_bytes(StorageCap::MIN.bytes() - 1).is_err());
        assert!(StorageCap::from_bytes(StorageCap::MIN.bytes()).is_ok());
    }

    fn refused_day_texts() -> Vec<String> {
        let mut texts: Vec<String> = [
            "2026-02-31",
            "2026-04-31",
            "2026-02-30",
            "2025-02-29",
            "1900-02-29",
            "+2026-11-9",
            "+2026-11-09",
            "2026-011-0009",
            "2026-13-01",
            "2026-00-10",
            "2026-11-00",
            "2026-11-32",
            "-001-01-01",
            "-0001-01-01",
            "10000-01-01",
            "9223372036854775807-01-01",
            "2026-1-09",
            "2026-11-9",
            "2026/11/09",
            "2026-11-09 ",
            " 2026-11-09",
            "2026-11-09T00:00:00Z",
            "",
        ]
        .map(String::from)
        .to_vec();
        texts.push(format!("2026-11-0{}", char::from_u32(0x0669).unwrap()));
        texts
    }

    #[test]
    fn utc_day_parsing_refuses_impossible_and_non_canonical_dates() {
        for text in refused_day_texts() {
            assert!(text.parse::<UtcDay>().is_err(), "{text:?}");
            let quoted = format!("\"{text}\"");
            assert!(serde_json::from_str::<UtcDay>(&quoted).is_err(), "{text:?}");
        }
        for text in ["2024-02-29", "2000-02-29", "0000-01-01", "9999-12-31"] {
            assert_eq!(text.parse::<UtcDay>().unwrap().to_string(), text);
        }
    }

    #[test]
    fn utc_day_a_year_of_i64_max_is_an_error_and_never_a_panic() {
        let text = format!("{}-01-01", i64::MAX);
        assert!(std::panic::catch_unwind(|| text.parse::<UtcDay>().is_err()).unwrap_or(false));
    }

    #[test]
    fn utc_day_every_constructor_is_bounded_to_the_years_0000_to_9999_and_never_panics() {
        for (year, month, day) in [
            (i64::MAX, 1, 1),
            (i64::MIN, 1, 1),
            (-1, 12, 31),
            (10_000, 1, 1),
            (2026, 2, 31),
            (2026, 13, 1),
            (2026, 0, 1),
            (2026, 1, 0),
            (2026, u32::MAX, u32::MAX),
        ] {
            assert!(
                UtcDay::from_year_month_day(year, month, day).is_err(),
                "{year}-{month}-{day}"
            );
        }
        let first = UtcDay::from_year_month_day(0, 1, 1).unwrap();
        let last = UtcDay::from_year_month_day(9999, 12, 31).unwrap();
        assert_eq!((first, last), (UtcDay::MIN, UtcDay::MAX));
        assert_eq!(first.to_string(), "0000-01-01");
        assert_eq!(last.to_string(), "9999-12-31");
        assert_eq!(first.days_since_epoch(), -719_528);
        assert_eq!(last.days_since_epoch(), 2_932_896);
        assert_eq!(UtcDay::from_days_since_epoch(-719_528), Ok(first));
        assert_eq!(UtcDay::from_days_since_epoch(2_932_896), Ok(last));
        for days in [-719_529, 2_932_897, i64::MAX, i64::MIN] {
            assert_eq!(
                UtcDay::from_days_since_epoch(days),
                Err(DayError::OutOfRange),
                "{days}"
            );
        }
        assert_eq!(last.next(), None);
        assert_eq!(
            UtcDay::from_days_since_epoch(2_932_895).unwrap().next(),
            Some(last)
        );
        let after_last = UNIX_EPOCH + Duration::from_secs(253_402_300_800);
        assert_eq!(
            UtcDay::from_system_time(after_last),
            Err(DayError::OutOfRange)
        );
        let last_second = UNIX_EPOCH + Duration::from_secs(253_402_300_799);
        assert_eq!(UtcDay::from_system_time(last_second), Ok(last));
        if let Some(at) = UNIX_EPOCH.checked_sub(Duration::from_secs(62_167_219_200)) {
            assert_eq!(UtcDay::from_system_time(at), Ok(first));
        }
        if let Some(at) = UNIX_EPOCH.checked_sub(Duration::from_secs(62_167_219_201)) {
            assert_eq!(UtcDay::from_system_time(at), Err(DayError::OutOfRange));
        }
    }

    #[test]
    fn utc_day_from_system_time_floors_to_the_utc_day() {
        let tick = Duration::from_nanos(if cfg!(windows) { 100 } else { 1 });
        let day = |at: SystemTime| UtcDay::from_system_time(at).unwrap().days_since_epoch();
        let day_secs = Duration::from_secs(86_400);
        assert_eq!(day(UNIX_EPOCH), 0);
        assert_eq!(day(UNIX_EPOCH + day_secs - tick), 0);
        assert_eq!(day(UNIX_EPOCH + day_secs), 1);
        assert_ne!(UNIX_EPOCH - tick, UNIX_EPOCH);
        assert_eq!(day(UNIX_EPOCH - tick), -1);
        assert_eq!(day(UNIX_EPOCH - day_secs), -1);
        assert_eq!(day(UNIX_EPOCH - day_secs - tick), -2);
    }

    #[test]
    fn utc_day_deserializes_its_text_and_a_table_of_days_round_trips_through_json() {
        let day = serde_json::from_str::<UtcDay>("\"2026-11-09\"").unwrap();
        assert_eq!(day.days_since_epoch(), 20_766);
        for (days, text) in [
            (-719_528, "0000-01-01"),
            (-1, "1969-12-31"),
            (0, "1970-01-01"),
            (11_016, "2000-02-29"),
            (19_723, "2024-01-01"),
            (20_766, "2026-11-09"),
            (2_932_896, "9999-12-31"),
        ] {
            let day = UtcDay::from_days_since_epoch(days).unwrap();
            let json = serde_json::to_string(&day).unwrap();
            assert_eq!(json, format!("\"{text}\""));
            assert_eq!(serde_json::from_str::<UtcDay>(&json).unwrap(), day);
            assert_eq!(text.parse::<UtcDay>().unwrap(), day);
        }
    }

    #[test]
    fn utc_day_round_trips_iso_dates() {
        assert_eq!(
            UtcDay::from_days_since_epoch(0).unwrap().to_string(),
            "1970-01-01"
        );
        assert_eq!(
            UtcDay::from_days_since_epoch(19_723).unwrap().to_string(),
            "2024-01-01"
        );
        assert_eq!(
            UtcDay::from_days_since_epoch(20_766).unwrap().to_string(),
            "2026-11-09"
        );
        assert_eq!(
            "2026-11-09".parse::<UtcDay>().unwrap(),
            UtcDay::from_days_since_epoch(20_766).unwrap()
        );
        let json = serde_json::to_string(&UtcDay::from_days_since_epoch(20_766).unwrap()).unwrap();
        assert_eq!(json, "\"2026-11-09\"");
    }

    #[test]
    fn utc_day_every_day_across_two_eras_round_trips_through_year_month_day() {
        for days in -DAYS_PER_ERA..=DAYS_PER_ERA {
            let day = UtcDay::from_days_since_epoch(days).unwrap();
            let (y, m, d) = day.to_year_month_day();
            assert!((1..=12).contains(&m) && (1..=31).contains(&d), "{days}");
            assert_eq!(UtcDay::from_year_month_day(y, m, d), Ok(day), "{y}-{m}-{d}");
        }
        assert_eq!(
            UtcDay::from_year_month_day(2000, 2, 29)
                .unwrap()
                .next()
                .unwrap()
                .to_string(),
            "2000-03-01"
        );
        assert_eq!(
            UtcDay::from_year_month_day(1900, 2, 28)
                .unwrap()
                .next()
                .unwrap()
                .to_string(),
            "1900-03-01"
        );
    }

    #[test]
    fn an_inventory_built_from_unsorted_rows_is_in_day_order() {
        let row = |day, bytes| InventoryRow {
            day: UtcDay::from_days_since_epoch(day).unwrap(),
            modality: Modality::ScreenFrame,
            events: 1,
            bytes,
        };
        let inv = Inventory::from_rows(vec![row(12, 1), row(10, 2), row(11, 3)]);
        let days: Vec<i64> = inv
            .rows()
            .iter()
            .map(|r| r.day.days_since_epoch())
            .collect();
        assert_eq!(days, vec![10, 11, 12]);
        assert_eq!(inv.total_bytes(), 6);
        assert_eq!(inv.total_events(), 3);
    }
}
