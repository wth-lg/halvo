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

/// What a capture event observed. Closed set; adding a modality is a code change,
/// not an open string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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

/// Whether capture is currently running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureState {
    Running,
    Paused,
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

    /// A cap of `bytes`, raised to the minimum when below it.
    pub const fn clamped(bytes: u64) -> Self {
        if bytes < Self::MIN.0 {
            Self::MIN
        } else {
            Self(bytes)
        }
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
        u64::deserialize(deserializer).map(Self::clamped)
    }
}

/// Why a day was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DayError {
    #[error("not a YYYY-MM-DD date")]
    Format,
}

/// A UTC calendar day, counted in days since 1970-01-01.
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

impl UtcDay {
    pub const fn from_days_since_epoch(days: i64) -> Self {
        Self(days)
    }

    pub const fn days_since_epoch(self) -> i64 {
        self.0
    }

    /// The UTC day that holds `at`.
    pub fn from_system_time(at: SystemTime) -> Self {
        let days = match at.duration_since(UNIX_EPOCH) {
            Ok(after) => (after.as_secs() / SECONDS_PER_DAY) as i64,
            Err(before) => -((before.duration().as_secs().div_ceil(SECONDS_PER_DAY)) as i64),
        };
        Self(days)
    }

    pub const fn next(self) -> Self {
        Self(self.0 + 1)
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

    pub fn from_year_month_day(year: i64, month: u32, day: u32) -> Self {
        let march_year = if month <= 2 { year - 1 } else { year };
        let era = if march_year >= 0 {
            march_year
        } else {
            march_year - (YEARS_PER_ERA - 1)
        } / YEARS_PER_ERA;
        let year_of_era = march_year - era * YEARS_PER_ERA;
        let month_from_march = if month > 2 {
            i64::from(month) - 3
        } else {
            i64::from(month) + 9
        };
        let day_of_march_year =
            (DAYS_PER_5_MONTHS_FROM_MARCH * month_from_march + 2) / 5 + i64::from(day) - 1;
        let day_of_era =
            year_of_era * DAYS_PER_YEAR + year_of_era / 4 - year_of_era / 100 + day_of_march_year;
        Self(era * DAYS_PER_ERA + day_of_era - DAYS_FROM_0000_03_01_TO_UNIX_EPOCH)
    }
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
        let mut parts = s.split('-');
        let year: i64 = parts
            .next()
            .and_then(|p| p.parse().ok())
            .ok_or(DayError::Format)?;
        let month: u32 = parts
            .next()
            .and_then(|p| p.parse().ok())
            .ok_or(DayError::Format)?;
        let day: u32 = parts
            .next()
            .and_then(|p| p.parse().ok())
            .ok_or(DayError::Format)?;
        if parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return Err(DayError::Format);
        }
        Ok(Self::from_year_month_day(year, month, day))
    }
}

impl Serialize for UtcDay {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for UtcDay {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
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

    #[test]
    fn the_window_is_fixed_and_not_a_setting() {
        assert_eq!(ObservationWindow::ROLLING_DAYS, 14);
        assert_eq!(ObservationWindow::FIRST_REPORT_DAY, 7);
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
    fn a_deserialized_cap_below_the_minimum_becomes_the_minimum() {
        let parse = |bytes: u64| serde_json::from_str::<StorageCap>(&bytes.to_string()).unwrap();
        assert_eq!(parse(0), StorageCap::MIN);
        assert_eq!(parse(StorageCap::MIN.bytes() - 1), StorageCap::MIN);
        assert_eq!(parse(StorageCap::MIN.bytes()), StorageCap::MIN);
        assert_eq!(parse(2 * StorageCap::GIB).bytes(), 2 * StorageCap::GIB);
        let json = serde_json::to_string(&StorageCap::DEFAULT).unwrap();
        assert_eq!(json, StorageCap::DEFAULT.bytes().to_string());
    }

    #[test]
    fn a_cap_below_the_minimum_is_rejected() {
        assert!(StorageCap::from_bytes(StorageCap::MIN.bytes() - 1).is_err());
        assert!(StorageCap::from_bytes(StorageCap::MIN.bytes()).is_ok());
    }

    #[test]
    fn utc_day_round_trips_iso_dates() {
        assert_eq!(UtcDay::from_days_since_epoch(0).to_string(), "1970-01-01");
        assert_eq!(
            UtcDay::from_days_since_epoch(19_723).to_string(),
            "2024-01-01"
        );
        assert_eq!(
            UtcDay::from_days_since_epoch(20_766).to_string(),
            "2026-11-09"
        );
        assert_eq!(
            "2026-11-09".parse::<UtcDay>().unwrap(),
            UtcDay::from_days_since_epoch(20_766)
        );
        let json = serde_json::to_string(&UtcDay::from_days_since_epoch(20_766)).unwrap();
        assert_eq!(json, "\"2026-11-09\"");
    }

    #[test]
    fn every_day_across_two_eras_round_trips_through_year_month_day() {
        for days in -DAYS_PER_ERA..=DAYS_PER_ERA {
            let day = UtcDay::from_days_since_epoch(days);
            let (y, m, d) = day.to_year_month_day();
            assert!((1..=12).contains(&m) && (1..=31).contains(&d), "{days}");
            assert_eq!(UtcDay::from_year_month_day(y, m, d), day, "{y}-{m}-{d}");
        }
        assert_eq!(
            UtcDay::from_year_month_day(2000, 2, 29).next().to_string(),
            "2000-03-01"
        );
        assert_eq!(
            UtcDay::from_year_month_day(1900, 2, 28).next().to_string(),
            "1900-03-01"
        );
    }

    #[test]
    fn an_inventory_built_from_unsorted_rows_is_in_day_order() {
        let row = |day, bytes| InventoryRow {
            day: UtcDay::from_days_since_epoch(day),
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
