use std::fmt;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

pub const APP_BUNDLE_ID: &str = "com.halvo.desktop";

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("not a spelling of a {what}")]
pub struct UnknownSpelling {
    pub(crate) what: &'static str,
}

pub(crate) fn from_text<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: FromStr,
    T::Err: fmt::Display,
{
    let text = String::deserialize(deserializer)?;
    text.parse().map_err(serde::de::Error::custom)
}

macro_rules! one_spelling {
    ($kind:ident, $what:literal, [$($variant:ident => $text:literal),+ $(,)?]) => {
        impl $kind {
            pub const ALL: &'static [$kind] = &[$($kind::$variant),+];

            pub const fn as_str(self) -> &'static str {
                match self {
                    $($kind::$variant => $text),+
                }
            }
        }

        impl std::str::FromStr for $kind {
            type Err = $crate::model::UnknownSpelling;

            fn from_str(text: &str) -> Result<Self, Self::Err> {
                $kind::ALL
                    .iter()
                    .copied()
                    .find(|value| value.as_str() == text)
                    .ok_or($crate::model::UnknownSpelling { what: $what })
            }
        }

        impl serde::Serialize for $kind {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_str())
            }
        }

        impl<'de> serde::Deserialize<'de> for $kind {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                $crate::model::from_text(deserializer)
            }
        }
    };
}

pub(crate) use one_spelling;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureState {
    Running,
    Paused,
}

impl CaptureState {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ObservationWindow {
    rolling_days: u32,
    first_report_day: u32,
}

impl<'de> Deserialize<'de> for ObservationWindow {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Read {
            rolling_days: u32,
            first_report_day: u32,
        }
        let Read {
            rolling_days,
            first_report_day,
        } = Read::deserialize(deserializer)?;
        let read = Self {
            rolling_days,
            first_report_day,
        };
        if read == Self::r1() {
            Ok(read)
        } else {
            Err(serde::de::Error::custom(
                "the observation window is fixed at 14 days with the first report on day 7",
            ))
        }
    }
}

impl ObservationWindow {
    pub const ROLLING_DAYS: u32 = 14;
    pub const FIRST_REPORT_DAY: u32 = 7;

    pub const fn r1() -> Self {
        Self {
            rolling_days: Self::ROLLING_DAYS,
            first_report_day: Self::FIRST_REPORT_DAY,
        }
    }

    pub const fn rolling_days(self) -> u32 {
        self.rolling_days
    }

    pub const fn first_report_day(self) -> u32 {
        self.first_report_day
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CapError {
    #[error("the storage cap must be at least {min} bytes")]
    TooSmall { min: u64 },
    #[error("the storage cap must be at most {max} bytes")]
    TooLarge { max: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct StorageCap(u64);

impl StorageCap {
    pub const GIB: u64 = 1024 * 1024 * 1024;
    pub const ESTIMATED_BYTES_PER_DAY: u64 = Self::GIB;
    pub const MIN: StorageCap = StorageCap(Self::ESTIMATED_BYTES_PER_DAY);
    pub const MAX: StorageCap = StorageCap(1024 * Self::GIB);
    const HEADROOM_BYTES: u64 = 11 * Self::GIB;
    pub const DEFAULT: StorageCap = StorageCap(
        ObservationWindow::ROLLING_DAYS as u64 * Self::ESTIMATED_BYTES_PER_DAY
            + Self::HEADROOM_BYTES,
    );

    pub fn from_bytes(bytes: u64) -> Result<Self, CapError> {
        if bytes < Self::MIN.0 {
            return Err(CapError::TooSmall { min: Self::MIN.0 });
        }
        if bytes > Self::MAX.0 {
            return Err(CapError::TooLarge { max: Self::MAX.0 });
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DayError {
    #[error("not a date in the form YYYY-MM-DD")]
    Format,
    #[error("the date lies outside 0000-01-01 to 9999-12-31")]
    OutOfRange,
    #[error("the month has no such day")]
    NoSuchDay,
}

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
    pub const MIN: UtcDay = UtcDay(MIN_DAYS);
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

    pub fn next(self) -> Option<Self> {
        Self::from_days_since_epoch(self.0.checked_add(1)?).ok()
    }

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InstantError {
    #[error("not an instant in the form YYYY-MM-DDTHH:MM:SSZ")]
    Format,
    #[error(transparent)]
    Day(#[from] DayError),
}

/// A UTC instant in whole seconds, written `YYYY-MM-DDTHH:MM:SSZ`, inside the days that
/// [`UtcDay`] holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UtcSecond(i64);

impl UtcSecond {
    pub fn from_unix_seconds(seconds: i64) -> Result<Self, DayError> {
        UtcDay::from_days_since_epoch(seconds.div_euclid(SECONDS_PER_DAY as i64))?;
        Ok(Self(seconds))
    }

    pub const fn unix_seconds(self) -> i64 {
        self.0
    }

    pub fn day(self) -> UtcDay {
        UtcDay(self.0.div_euclid(SECONDS_PER_DAY as i64))
    }

    pub fn from_day_and_time(
        day: UtcDay,
        hour: u32,
        minute: u32,
        second: u32,
    ) -> Result<Self, InstantError> {
        if hour > 23 || minute > 59 || second > 59 {
            return Err(InstantError::Format);
        }
        let within = i64::from(hour * 3_600 + minute * 60 + second);
        Ok(Self(day.0 * SECONDS_PER_DAY as i64 + within))
    }
}

impl fmt::Display for UtcSecond {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let within = self.0.rem_euclid(SECONDS_PER_DAY as i64);
        write!(
            f,
            "{}T{:02}:{:02}:{:02}Z",
            self.day(),
            within / 3_600,
            within % 3_600 / 60,
            within % 60
        )
    }
}

impl FromStr for UtcSecond {
    type Err = InstantError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let bytes = text.as_bytes();
        if bytes.len() != 20
            || bytes[10] != b'T'
            || bytes[13] != b':'
            || bytes[16] != b':'
            || bytes[19] != b'Z'
        {
            return Err(InstantError::Format);
        }
        let day: UtcDay = text[..10].parse()?;
        let two = |at: usize| match (bytes[at], bytes[at + 1]) {
            (high @ b'0'..=b'9', low @ b'0'..=b'9') => {
                Ok(u32::from(high - b'0') * 10 + u32::from(low - b'0'))
            }
            _ => Err(InstantError::Format),
        };
        Self::from_day_and_time(day, two(11)?, two(14)?, two(17)?)
    }
}

impl Serialize for UtcSecond {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for UtcSecond {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        from_text(deserializer)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryRow {
    pub day: UtcDay,
    pub modality: Modality,
    pub events: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InventoryError {
    #[error("the inventory holds two rows for {day} and {}", .modality.as_str())]
    DuplicateRow { day: UtcDay, modality: Modality },
    #[error("the inventory's total events or bytes exceed what 64 bits hold")]
    TotalTooLarge,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct Inventory {
    rows: Vec<InventoryRow>,
}

impl Inventory {
    pub fn from_rows(mut rows: Vec<InventoryRow>) -> Result<Self, InventoryError> {
        rows.sort_by(|a, b| {
            a.day
                .cmp(&b.day)
                .then_with(|| modality_order(a.modality).cmp(&modality_order(b.modality)))
        });
        if let Some(pair) = rows
            .windows(2)
            .find(|pair| (pair[0].day, pair[0].modality) == (pair[1].day, pair[1].modality))
        {
            return Err(InventoryError::DuplicateRow {
                day: pair[0].day,
                modality: pair[0].modality,
            });
        }
        let total = |field: fn(&InventoryRow) -> u64| {
            rows.iter()
                .try_fold(0u64, |sum, row| sum.checked_add(field(row)))
        };
        if total(|row| row.events).is_none() || total(|row| row.bytes).is_none() {
            return Err(InventoryError::TotalTooLarge);
        }
        Ok(Self { rows })
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
        Self::from_rows(rows).map_err(serde::de::Error::custom)
    }
}

fn modality_order(modality: Modality) -> usize {
    Modality::ALL
        .iter()
        .position(|&kind| kind == modality)
        .unwrap_or(Modality::ALL.len())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("not a store id: 32 lowercase hex digits, or the same with hyphens after 8, 12, 16 and 20")]
pub struct StoreIdError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StoreId([u8; 16]);

impl StoreId {
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

impl fmt::Display for StoreId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|byte| write!(f, "{byte:02x}"))
    }
}

impl FromStr for StoreId {
    type Err = StoreIdError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let bytes = text.as_bytes();
        let digits: Vec<u8> = match bytes.len() {
            32 => bytes.to_vec(),
            36 if [8, 13, 18, 23].iter().all(|&at| bytes[at] == b'-') => bytes
                .iter()
                .enumerate()
                .filter(|(at, _)| ![8, 13, 18, 23].contains(at))
                .map(|(_, &byte)| byte)
                .collect(),
            _ => return Err(StoreIdError),
        };
        let nibble = |digit: u8| match digit {
            b'0'..=b'9' => Ok(digit - b'0'),
            b'a'..=b'f' => Ok(digit - b'a' + 10),
            _ => Err(StoreIdError),
        };
        let mut id = [0u8; 16];
        let (pairs, _) = digits.as_chunks::<2>();
        for (byte, [high, low]) in id.iter_mut().zip(pairs) {
            *byte = nibble(*high)? << 4 | nibble(*low)?;
        }
        Ok(Self(id))
    }
}

impl Serialize for StoreId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for StoreId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        from_text(deserializer)
    }
}

#[cfg(test)]
pub(crate) fn every_key_is_read<T: serde::de::DeserializeOwned>(object: &serde_json::Value) {
    let keys = object.as_object().unwrap();
    assert!(
        serde_json::from_value::<T>(object.clone()).is_ok(),
        "{object}"
    );
    for key in keys.keys() {
        let mut without = keys.clone();
        without.remove(key);
        assert!(
            serde_json::from_value::<T>(serde_json::Value::Object(without)).is_err(),
            "a reply without {key} was read"
        );
    }
}

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
    fn a_36_character_store_id_needs_hyphens_at_8_13_18_and_23() {
        assert!("0123456789abcdef0123456789abcdef0123"
            .parse::<StoreId>()
            .is_err());
        assert!("01234567x89abxcdefx0123x456789abcdef"
            .parse::<StoreId>()
            .is_err());
        assert_eq!(
            "01234567-89ab-cdef-0123-456789abcdef"
                .parse::<StoreId>()
                .map(|id| id.to_string()),
            Ok("0123456789abcdef0123456789abcdef".to_string())
        );
    }

    #[test]
    fn a_store_id_writes_32_lowercase_hex_and_reads_the_hyphenated_form_too() {
        let id = StoreId::from_bytes([
            0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55,
            0x66, 0x77,
        ]);
        let plain = "0123456789abcdef0011223344556677";
        let hyphenated = "01234567-89ab-cdef-0011-223344556677";
        assert_eq!(id.to_string(), plain);
        assert_eq!(plain.parse::<StoreId>(), Ok(id));
        assert_eq!(hyphenated.parse::<StoreId>(), Ok(id));
        assert_eq!(hyphenated.parse::<StoreId>().unwrap().to_string(), plain);
        assert_eq!(serde_json::to_string(&id).unwrap(), format!("\"{plain}\""));
        let read = serde_json::from_str::<StoreId>(&format!("\"{hyphenated}\"")).unwrap();
        assert_eq!(read, id);
        assert_eq!(id.as_bytes()[15], 0x77);
        for bad in [
            "0123456789ABCDEF0011223344556677",
            "01234567-89AB-CDEF-0011-223344556677",
            "0123456789abcdef001122334455667",
            "0123456789abcdef00112233445566778",
            "0123456789abcdef001122334455667g",
            "0123456-789ab-cdef-0011-223344556677",
            "01234567-89ab-cdef-0011223344556677",
            "01234567-89ab-cdef-0011-2233445566-7",
            "{01234567-89ab-cdef-0011-223344556677}",
            "urn:uuid:01234567-89ab-cdef-0011-223344556677",
            " 0123456789abcdef0011223344556677",
            "",
        ] {
            assert!(bad.parse::<StoreId>().is_err(), "{bad:?}");
            assert!(serde_json::from_str::<StoreId>(&format!("\"{bad}\"")).is_err());
        }
    }

    fn inventory_row(days: i64, modality: Modality, events: u64, bytes: u64) -> InventoryRow {
        InventoryRow {
            day: UtcDay::from_days_since_epoch(days).unwrap(),
            modality,
            events,
            bytes,
        }
    }

    const INVENTORY_JSON: &str = r#"{"rows":[{"day":"2026-11-08","modality":"screen_frame","events":3,"bytes":300},{"day":"2026-11-09","modality":"keystroke","events":7,"bytes":70}]}"#;

    #[test]
    fn a_row_or_an_inventory_without_one_of_its_keys_is_refused() {
        every_key_is_read::<InventoryRow>(
            &serde_json::from_str(
                r#"{"day":"2026-11-08","modality":"screen_frame","events":3,"bytes":300}"#,
            )
            .unwrap(),
        );
        every_key_is_read::<Inventory>(&serde_json::from_str(INVENTORY_JSON).unwrap());
    }

    #[test]
    fn a_delete_report_without_one_of_its_keys_is_refused_and_keeps_64_bit_counts() {
        every_key_is_read::<ShredView>(
            &serde_json::from_str(r#"{"segments_shredded":2,"keys_destroyed":3}"#).unwrap(),
        );
        let wide = r#"{"segments_shredded":4294967296,"keys_destroyed":4294967297}"#;
        let report = ShredView {
            segments_shredded: 4_294_967_296,
            keys_destroyed: 4_294_967_297,
        };
        assert_eq!(serde_json::to_string(&report).unwrap(), wide);
        assert_eq!(serde_json::from_str::<ShredView>(wide).unwrap(), report);
    }

    #[test]
    fn the_inventory_object_and_its_row_have_golden_json() {
        let inventory = Inventory::from_rows(vec![
            inventory_row(20_766, Modality::Keystroke, 7, 70),
            inventory_row(20_765, Modality::ScreenFrame, 3, 300),
        ])
        .unwrap();
        assert_eq!(serde_json::to_string(&inventory).unwrap(), INVENTORY_JSON);
        assert_eq!(
            serde_json::from_str::<Inventory>(INVENTORY_JSON).unwrap(),
            inventory
        );
        let row_json = r#"{"day":"2026-11-08","modality":"screen_frame","events":3,"bytes":300}"#;
        assert_eq!(
            serde_json::to_string(&inventory.rows()[0]).unwrap(),
            row_json
        );
        assert_eq!(
            serde_json::from_str::<InventoryRow>(row_json).unwrap(),
            inventory.rows()[0]
        );
        assert_eq!(
            serde_json::to_string(&Inventory::default()).unwrap(),
            r#"{"rows":[]}"#
        );
    }

    #[test]
    fn within_a_day_the_rows_follow_the_kind_order_also_after_reading_an_unsorted_reply() {
        let shuffled = [
            Modality::Camera,
            Modality::Pointer,
            Modality::ScreenFrame,
            Modality::WindowTitle,
            Modality::Microphone,
            Modality::Keystroke,
            Modality::SystemAudio,
            Modality::ForegroundApp,
        ];
        let mut rows: Vec<InventoryRow> = shuffled
            .iter()
            .zip(1..)
            .map(|(&kind, events)| inventory_row(20_766, kind, events, 1))
            .collect();
        rows.push(inventory_row(20_765, Modality::Camera, 9, 1));
        let built = Inventory::from_rows(rows.clone()).unwrap();
        let reply = serde_json::to_string(&serde_json::json!({ "rows": rows })).unwrap();
        let read = serde_json::from_str::<Inventory>(&reply).unwrap();
        let mut expected = vec![(20_765, Modality::Camera)];
        expected.extend(Modality::ALL.map(|kind| (20_766, kind)));
        for inventory in [built, read] {
            let order: Vec<(i64, Modality)> = inventory
                .rows()
                .iter()
                .map(|row| (row.day.days_since_epoch(), row.modality))
                .collect();
            assert_eq!(order, expected);
        }
    }

    #[test]
    fn a_reply_with_two_rows_for_one_day_and_kind_is_an_error() {
        let rows = vec![
            inventory_row(20_766, Modality::Keystroke, 1, 10),
            inventory_row(20_766, Modality::Pointer, 1, 10),
            inventory_row(20_766, Modality::Keystroke, 2, 20),
        ];
        assert_eq!(
            Inventory::from_rows(rows.clone()),
            Err(InventoryError::DuplicateRow {
                day: UtcDay::from_days_since_epoch(20_766).unwrap(),
                modality: Modality::Keystroke,
            })
        );
        let reply = serde_json::to_string(&serde_json::json!({ "rows": rows })).unwrap();
        assert!(serde_json::from_str::<Inventory>(&reply).is_err());
    }

    #[test]
    fn an_inventory_whose_totals_exceed_u64_is_an_error_and_never_a_panic() {
        let too_many_bytes = vec![
            inventory_row(20_765, Modality::Keystroke, 1, u64::MAX),
            inventory_row(20_766, Modality::Keystroke, 1, 1),
        ];
        let too_many_events = vec![
            inventory_row(20_765, Modality::Keystroke, u64::MAX, 1),
            inventory_row(20_765, Modality::Pointer, 1, 1),
        ];
        for rows in [too_many_bytes, too_many_events] {
            assert_eq!(
                Inventory::from_rows(rows.clone()),
                Err(InventoryError::TotalTooLarge)
            );
            let reply = serde_json::to_string(&serde_json::json!({ "rows": rows })).unwrap();
            assert!(serde_json::from_str::<Inventory>(&reply).is_err());
        }
        let full = Inventory::from_rows(vec![
            inventory_row(20_765, Modality::Keystroke, u64::MAX - 1, u64::MAX - 1),
            inventory_row(20_766, Modality::Keystroke, 1, 1),
        ])
        .unwrap();
        assert_eq!(
            (full.total_events(), full.total_bytes()),
            (u64::MAX, u64::MAX)
        );
    }

    #[test]
    fn a_window_other_than_the_fixed_one_is_never_read() {
        for text in [
            r#"{"rolling_days":30,"first_report_day":7}"#,
            r#"{"rolling_days":14,"first_report_day":0}"#,
            r#"{"rolling_days":0,"first_report_day":0}"#,
            r#"{"rolling_days":14}"#,
        ] {
            assert!(
                serde_json::from_str::<ObservationWindow>(text).is_err(),
                "{text}"
            );
        }
        let window = ObservationWindow::r1();
        assert_eq!((window.rolling_days(), window.first_report_day()), (14, 7));
    }

    #[test]
    fn the_observation_window_and_the_delete_report_have_golden_json() {
        let window_json = r#"{"rolling_days":14,"first_report_day":7}"#;
        assert_eq!(
            serde_json::to_string(&ObservationWindow::r1()).unwrap(),
            window_json
        );
        assert_eq!(
            serde_json::from_str::<ObservationWindow>(window_json).unwrap(),
            ObservationWindow::r1()
        );
        let report = ShredView {
            segments_shredded: 2,
            keys_destroyed: 3,
        };
        let report_json = r#"{"segments_shredded":2,"keys_destroyed":3}"#;
        assert_eq!(serde_json::to_string(&report).unwrap(), report_json);
        assert_eq!(
            serde_json::from_str::<ShredView>(report_json).unwrap(),
            report
        );
    }

    #[test]
    fn frames_are_droppable_but_input_is_not() {
        assert!(Modality::ScreenFrame.is_droppable_under_backpressure());
        assert!(Modality::SystemAudio.is_droppable_under_backpressure());
        assert!(!Modality::Keystroke.is_droppable_under_backpressure());
        assert!(!Modality::Pointer.is_droppable_under_backpressure());
        assert!(!Modality::ForegroundApp.is_droppable_under_backpressure());
        assert!(!Modality::WindowTitle.is_droppable_under_backpressure());
        assert!(Modality::Microphone.is_droppable_under_backpressure());
        assert!(Modality::Camera.is_droppable_under_backpressure());
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
    fn a_cap_has_a_floor_and_a_ceiling_with_a_typed_error() {
        assert_eq!(StorageCap::MIN.bytes(), 1_073_741_824);
        assert_eq!(StorageCap::MAX.bytes(), 1_099_511_627_776);
        assert_eq!(StorageCap::DEFAULT.bytes(), 26_843_545_600);
        assert!(StorageCap::MAX.bytes() < 1 << 53);
        let too_large = Err(CapError::TooLarge {
            max: 1_099_511_627_776,
        });
        let too_small = Err(CapError::TooSmall { min: 1_073_741_824 });
        assert_eq!(StorageCap::from_bytes(u64::MAX), too_large);
        assert_eq!(StorageCap::from_bytes(1_099_511_627_777), too_large);
        assert_eq!(StorageCap::from_bytes(0), too_small);
        assert_eq!(StorageCap::from_bytes(1_073_741_823), too_small);
        assert_eq!(
            StorageCap::from_bytes(1_099_511_627_776),
            Ok(StorageCap::MAX)
        );
        assert_eq!(StorageCap::from_bytes(1_073_741_824), Ok(StorageCap::MIN));
        for stored in [u64::MAX, 1_099_511_627_777] {
            assert!(serde_json::from_str::<StorageCap>(&stored.to_string()).is_err());
        }
        assert_eq!(
            CapError::TooLarge {
                max: 1_099_511_627_776
            }
            .to_string(),
            "the storage cap must be at most 1099511627776 bytes"
        );
        assert_eq!(
            CapError::TooSmall { min: 1_073_741_824 }.to_string(),
            "the storage cap must be at least 1073741824 bytes"
        );
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
        let inv = Inventory::from_rows(vec![row(12, 1), row(10, 2), row(11, 3)]).unwrap();
        let days: Vec<i64> = inv
            .rows()
            .iter()
            .map(|r| r.day.days_since_epoch())
            .collect();
        assert_eq!(days, vec![10, 11, 12]);
        assert_eq!(inv.total_bytes(), 6);
        assert_eq!(inv.total_events(), 3);
    }

    #[test]
    fn a_day_text_with_another_byte_in_place_of_a_hyphen_is_refused() {
        for text in ["2026-11x09", "2026-11009", "2026x11-09", "2026011-09"] {
            assert!(text.parse::<UtcDay>().is_err(), "{text}");
        }
    }

    #[test]
    fn an_observation_window_without_one_of_its_keys_is_refused() {
        every_key_is_read::<ObservationWindow>(
            &serde_json::json!({"rolling_days": 14, "first_report_day": 7}),
        );
    }

    #[test]
    fn an_instant_writes_and_reads_the_utc_form_with_a_z() {
        for (seconds, text) in [
            (0, "1970-01-01T00:00:00Z"),
            (1_794_474_000, "2026-11-12T09:00:00Z"),
            (1_794_474_005, "2026-11-12T09:00:05Z"),
            (-1, "1969-12-31T23:59:59Z"),
            (253_402_300_799, "9999-12-31T23:59:59Z"),
            (-62_167_219_200, "0000-01-01T00:00:00Z"),
        ] {
            let at = UtcSecond::from_unix_seconds(seconds).unwrap();
            assert_eq!(at.to_string(), text);
            assert_eq!(text.parse::<UtcSecond>().unwrap(), at);
            assert_eq!(serde_json::to_string(&at).unwrap(), format!("\"{text}\""));
            assert_eq!(
                serde_json::from_str::<UtcSecond>(&format!("\"{text}\"")).unwrap(),
                at
            );
        }
        assert!(UtcSecond::from_unix_seconds(253_402_300_800).is_err());
        assert!(UtcSecond::from_unix_seconds(-62_167_219_201).is_err());
        assert!(UtcSecond::from_unix_seconds(i64::MIN).is_err());
        assert!(UtcSecond::from_unix_seconds(i64::MAX).is_err());
    }

    #[test]
    fn an_instant_in_another_form_is_refused() {
        for text in [
            "2026-11-12T09:00:00",
            "2026-11-12 09:00:00Z",
            "2026-11-12T09:00:00z",
            "2026-11-12T24:00:00Z",
            "2026-11-12T09:60:00Z",
            "2026-11-12T09:00:60Z",
            "2026-11-12T9:00:00ZZ",
            "2026-11-12T09:00:00+00:00",
            "2026-02-30T09:00:00Z",
            "2026-11-12T0a:00:00Z",
            "",
        ] {
            assert!(text.parse::<UtcSecond>().is_err(), "{text}");
        }
    }
}
