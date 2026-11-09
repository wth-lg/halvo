use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::onboarding::Permission;
use super::{CaptureState, CaptureStatus, ConsentError, ConsentView, ModalityConsent};
use crate::event::Modality;
use crate::store::{SegmentExpiry, ShredReport, StoreError, StoreResult};

pub const SETTINGS_FILE_NAME: &str = "consent.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ObservationWindow {
    pub rolling_days: u32,
    pub first_report_day: u32,
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
}

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

    pub fn from_bytes(bytes: u64) -> Result<Self, ConsentError> {
        if bytes < Self::MIN.0 {
            return Err(ConsentError::Action(format!(
                "the storage cap must be at least {} bytes (one day of capture); {bytes} was given",
                Self::MIN.0
            )));
        }
        Ok(Self(bytes))
    }

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

    pub fn from_system_time(at: SystemTime) -> Self {
        let days = match at.duration_since(UNIX_EPOCH) {
            Ok(after) => (after.as_secs() / SECONDS_PER_DAY) as i64,
            Err(before) => -((before.duration().as_secs().div_ceil(SECONDS_PER_DAY)) as i64),
        };
        Self(days)
    }

    pub fn today() -> Self {
        Self::from_system_time(SystemTime::now())
    }

    pub const fn next(self) -> Self {
        Self(self.0 + 1)
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
    type Err = ConsentError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bad = || ConsentError::Action(format!("not a YYYY-MM-DD date: {s:?}"));
        let mut parts = s.split('-');
        let year: i64 = parts.next().and_then(|p| p.parse().ok()).ok_or_else(bad)?;
        let month: u32 = parts.next().and_then(|p| p.parse().ok()).ok_or_else(bad)?;
        let day: u32 = parts.next().and_then(|p| p.parse().ok()).ok_or_else(bad)?;
        if parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return Err(bad());
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryRow {
    pub day: UtcDay,
    pub modality: Modality,
    pub events: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct Inventory {
    rows: Vec<InventoryRow>,
}

impl Inventory {
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

pub trait StoreInventory {
    fn inventory(&self) -> StoreResult<Inventory>;

    /// Crypto-shred every segment captured before the start of `day` (UTC), so
    /// the store is left holding `day` and newer only.
    fn crypto_shred_before(&mut self, day: UtcDay) -> StoreResult<ShredReport>;
}

pub trait DataStore: SegmentExpiry + StoreInventory {}

impl<T: SegmentExpiry + StoreInventory> DataStore for T {}

impl From<StoreError> for ConsentError {
    fn from(error: StoreError) -> Self {
        ConsentError::Action(error.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ConsentSettings {
    pub version: u32,
    pub capture: CaptureState,
    pub storage_cap: StorageCap,
    pub permissions_once_granted: Vec<Permission>,
}

impl Default for ConsentSettings {
    fn default() -> Self {
        Self {
            version: 1,
            capture: CaptureState::Running,
            storage_cap: StorageCap::DEFAULT,
            permissions_once_granted: Vec::new(),
        }
    }
}

impl ConsentSettings {
    pub fn load(path: &Path) -> Result<Self, ConsentError> {
        SettingsFile::new(path.to_path_buf()).load()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsFile {
    path: PathBuf,
}

impl SettingsFile {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<ConsentSettings, ConsentError> {
        match fs::read(&self.path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| {
                ConsentError::Action(format!(
                    "{} is not valid consent settings: {e}",
                    self.path.display()
                ))
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ConsentSettings::default()),
            Err(e) => Err(ConsentError::Action(format!(
                "could not read {}: {e}",
                self.path.display()
            ))),
        }
    }

    pub fn save(&self, settings: &ConsentSettings) -> Result<(), ConsentError> {
        let io = |what: &str, e: std::io::Error| {
            ConsentError::Action(format!("could not {what} {}: {e}", self.path.display()))
        };
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir).map_err(|e| io("create the directory for", e))?;
        }
        let bytes = serde_json::to_vec_pretty(settings)
            .map_err(|e| ConsentError::Action(format!("could not encode consent settings: {e}")))?;
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, bytes).map_err(|e| io("write", e))?;
        fs::rename(&tmp, &self.path).map_err(|e| io("replace", e))
    }
}

pub fn r1_modality_consent(modality: Modality) -> ModalityConsent {
    match modality {
        Modality::ScreenFrame
        | Modality::Keystroke
        | Modality::Pointer
        | Modality::ForegroundApp
        | Modality::WindowTitle => ModalityConsent::Enabled,
        Modality::SystemAudio | Modality::Microphone | Modality::Camera => {
            ModalityConsent::Disabled
        }
    }
}

pub fn eviction_cutoff(inventory: &Inventory, cap: StorageCap) -> Option<UtcDay> {
    let cap_bytes = cap.bytes();
    let mut remaining = inventory.total_bytes();
    if remaining <= cap_bytes {
        return None;
    }
    let mut cutoff = None;
    for row in inventory.rows() {
        if remaining <= cap_bytes {
            break;
        }
        cutoff = Some(row.day.next());
        remaining = remaining.saturating_sub(row.bytes);
    }
    cutoff
}

pub struct ConsentController {
    store: Box<dyn DataStore + Send>,
    settings_file: SettingsFile,
    settings: ConsentSettings,
    last_inventory: Inventory,
}

impl ConsentController {
    pub fn open(
        store: Box<dyn DataStore + Send>,
        settings_file: SettingsFile,
    ) -> Result<Self, ConsentError> {
        let settings = settings_file.load()?;
        let mut controller = Self {
            store,
            settings_file,
            settings,
            last_inventory: Inventory::default(),
        };
        controller.refresh_inventory()?;
        controller.enforce_storage_cap()?;
        Ok(controller)
    }

    pub const fn window(&self) -> ObservationWindow {
        ObservationWindow::r1()
    }

    pub fn settings(&self) -> &ConsentSettings {
        &self.settings
    }

    pub fn storage_cap(&self) -> StorageCap {
        self.settings.storage_cap
    }

    pub fn inventory(&mut self) -> Result<&Inventory, ConsentError> {
        self.refresh_inventory()?;
        Ok(&self.last_inventory)
    }

    fn refresh_inventory(&mut self) -> Result<(), ConsentError> {
        self.last_inventory = self.store.inventory()?;
        Ok(())
    }

    pub fn delete_everything(&mut self) -> Result<ShredReport, ConsentError> {
        let report = self.store.crypto_shred_all()?;
        self.refresh_inventory()?;
        Ok(report)
    }

    pub fn set_storage_cap(
        &mut self,
        cap: StorageCap,
    ) -> Result<Option<ShredReport>, ConsentError> {
        if self.settings.storage_cap != cap {
            self.settings.storage_cap = cap;
            self.settings_file.save(&self.settings)?;
        }
        self.enforce_storage_cap()
    }

    pub fn enforce_storage_cap(&mut self) -> Result<Option<ShredReport>, ConsentError> {
        self.refresh_inventory()?;
        let Some(cutoff) = eviction_cutoff(&self.last_inventory, self.settings.storage_cap) else {
            return Ok(None);
        };
        let report = self.store.crypto_shred_before(cutoff)?;
        self.refresh_inventory()?;
        Ok(Some(report))
    }

    pub fn record_granted_permissions(
        &mut self,
        granted_now: &[Permission],
    ) -> Result<(), ConsentError> {
        let mut grew = false;
        for permission in granted_now {
            if !self.settings.permissions_once_granted.contains(permission) {
                self.settings.permissions_once_granted.push(*permission);
                grew = true;
            }
        }
        if grew {
            self.settings_file.save(&self.settings)?;
        }
        Ok(())
    }
}

impl ConsentView for ConsentController {
    fn status(&self) -> CaptureStatus {
        CaptureStatus {
            state: self.settings.capture,
            store_bytes: self.last_inventory.total_bytes(),
            store_cap_bytes: self.settings.storage_cap.bytes(),
        }
    }

    fn modality_consent(&self, modality: Modality) -> ModalityConsent {
        r1_modality_consent(modality)
    }

    fn set_capture_state(&mut self, state: CaptureState) -> Result<(), ConsentError> {
        if self.settings.capture == state {
            return Ok(());
        }
        self.settings.capture = state;
        self.settings_file.save(&self.settings)
    }

    fn delete_all(&mut self) -> Result<(), ConsentError> {
        self.delete_everything().map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consent::{CaptureState, ConsentView, ModalityConsent};
    use crate::event::Modality;
    use crate::store::{SegmentExpiry, ShredReport, StoreResult};

    struct MockStore {
        rows: Vec<InventoryRow>,
        shred_all_calls: u32,
        shred_before_calls: Vec<UtcDay>,
    }

    impl SegmentExpiry for MockStore {
        fn crypto_shred_expired(&mut self) -> StoreResult<ShredReport> {
            Ok(ShredReport::default())
        }
        fn crypto_shred_all(&mut self) -> StoreResult<ShredReport> {
            self.shred_all_calls += 1;
            let n = self.rows.len() as u64;
            self.rows.clear();
            Ok(ShredReport {
                segments_shredded: n,
                keys_destroyed: n + 1,
            })
        }
    }

    impl StoreInventory for MockStore {
        fn inventory(&self) -> StoreResult<Inventory> {
            Ok(Inventory::from_rows(self.rows.clone()))
        }
        fn crypto_shred_before(&mut self, day: UtcDay) -> StoreResult<ShredReport> {
            self.shred_before_calls.push(day);
            let before = self.rows.len() as u64;
            self.rows.retain(|r| r.day >= day);
            let n = before - self.rows.len() as u64;
            Ok(ShredReport {
                segments_shredded: n,
                keys_destroyed: n,
            })
        }
    }

    fn row(day: i64, modality: Modality, events: u64, bytes: u64) -> InventoryRow {
        InventoryRow {
            day: UtcDay::from_days_since_epoch(day),
            modality,
            events,
            bytes,
        }
    }

    fn temp_settings() -> SettingsFile {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("halvo-consent-test-{}-{n}", std::process::id()));
        SettingsFile::new(dir.join("consent.json"))
    }

    fn controller(rows: Vec<InventoryRow>) -> ConsentController {
        ConsentController::open(
            Box::new(MockStore {
                rows,
                shred_all_calls: 0,
                shred_before_calls: vec![],
            }),
            temp_settings(),
        )
        .unwrap()
    }

    #[test]
    fn the_window_is_fixed_and_not_a_setting() {
        assert_eq!(ObservationWindow::ROLLING_DAYS, 14);
        assert_eq!(ObservationWindow::FIRST_REPORT_DAY, 7);
    }

    #[test]
    fn status_starts_running_and_reflects_pause_and_resume() {
        let mut c = controller(vec![row(0, Modality::Keystroke, 1, 10)]);
        assert_eq!(c.status().state, CaptureState::Running);
        c.set_capture_state(CaptureState::Paused).unwrap();
        assert_eq!(c.status().state, CaptureState::Paused);
        c.set_capture_state(CaptureState::Paused).unwrap();
        assert_eq!(c.status().state, CaptureState::Paused);
        c.set_capture_state(CaptureState::Running).unwrap();
        assert_eq!(c.status().state, CaptureState::Running);
    }

    #[test]
    fn pause_persists_so_the_daemon_can_read_it() {
        let file = temp_settings();
        let mut c = ConsentController::open(
            Box::new(MockStore {
                rows: vec![],
                shred_all_calls: 0,
                shred_before_calls: vec![],
            }),
            file.clone(),
        )
        .unwrap();
        c.set_capture_state(CaptureState::Paused).unwrap();
        let reloaded = file.load().unwrap();
        assert_eq!(reloaded.capture, CaptureState::Paused);
        let reopened = ConsentController::open(
            Box::new(MockStore {
                rows: vec![],
                shred_all_calls: 0,
                shred_before_calls: vec![],
            }),
            file,
        )
        .unwrap();
        assert_eq!(reopened.status().state, CaptureState::Paused);
    }

    #[test]
    fn inventory_lists_counts_and_bytes_by_modality_and_day_oldest_first() {
        let mut c = controller(vec![
            row(20_000, Modality::ScreenFrame, 5, 500),
            row(19_999, Modality::Keystroke, 7, 70),
            row(19_999, Modality::ScreenFrame, 3, 300),
        ]);
        let inv = c.inventory().unwrap();
        assert_eq!(inv.rows().len(), 3);
        assert_eq!(inv.rows()[0].day, UtcDay::from_days_since_epoch(19_999));
        assert_eq!(inv.rows()[2].day, UtcDay::from_days_since_epoch(20_000));
        assert_eq!(inv.total_bytes(), 870);
        assert_eq!(inv.total_events(), 15);
        assert_eq!(c.status().store_bytes, 870);
    }

    #[test]
    fn delete_all_calls_the_store_shred_all_and_empties_the_inventory() {
        let mut c = controller(vec![
            row(1, Modality::Pointer, 2, 20),
            row(2, Modality::Keystroke, 2, 20),
        ]);
        let report = c.delete_everything().unwrap();
        assert_eq!(report.segments_shredded, 2);
        assert_eq!(report.keys_destroyed, 3);
        assert!(c.inventory().unwrap().is_empty());
        assert_eq!(c.status().store_bytes, 0);
        let again = c.delete_everything().unwrap();
        assert_eq!(again.segments_shredded, 0);
    }

    #[test]
    fn delete_all_through_the_consent_view_trait() {
        let mut c = controller(vec![row(1, Modality::Pointer, 2, 20)]);
        ConsentView::delete_all(&mut c).unwrap();
        assert!(c.inventory().unwrap().is_empty());
    }

    #[test]
    fn the_cap_cutoff_removes_the_oldest_days_first() {
        let gib = StorageCap::GIB;
        let cap = |bytes| StorageCap::from_bytes(bytes).unwrap();
        let inv = Inventory::from_rows(vec![
            row(10, Modality::ScreenFrame, 1, 4 * gib),
            row(11, Modality::ScreenFrame, 1, 4 * gib),
            row(12, Modality::ScreenFrame, 1, 4 * gib),
        ]);
        assert_eq!(eviction_cutoff(&inv, cap(12 * gib)), None);
        assert_eq!(
            eviction_cutoff(&inv, cap(9 * gib)),
            Some(UtcDay::from_days_since_epoch(11))
        );
        assert_eq!(
            eviction_cutoff(&inv, cap(4 * gib)),
            Some(UtcDay::from_days_since_epoch(12))
        );
        assert_eq!(
            eviction_cutoff(&inv, StorageCap::MIN),
            Some(UtcDay::from_days_since_epoch(13))
        );
    }

    #[test]
    fn an_inventory_built_from_unsorted_rows_still_evicts_the_oldest_day_only() {
        let gib = StorageCap::GIB;
        let inv = Inventory::from_rows(vec![
            row(12, Modality::ScreenFrame, 1, gib),
            row(10, Modality::ScreenFrame, 1, gib),
            row(11, Modality::ScreenFrame, 1, gib),
        ]);
        let days: Vec<i64> = inv
            .rows()
            .iter()
            .map(|r| r.day.days_since_epoch())
            .collect();
        assert_eq!(days, vec![10, 11, 12]);
        assert_eq!(
            eviction_cutoff(&inv, StorageCap::from_bytes(2 * gib).unwrap()),
            Some(UtcDay::from_days_since_epoch(11))
        );
    }

    #[test]
    fn lowering_the_cap_shreds_oldest_first_until_under_the_cap() {
        let gib = StorageCap::GIB;
        let mut c = controller(vec![
            row(10, Modality::ScreenFrame, 1, gib),
            row(11, Modality::ScreenFrame, 1, gib),
            row(12, Modality::ScreenFrame, 1, gib),
        ]);
        assert_eq!(c.storage_cap(), StorageCap::DEFAULT);
        let report = c
            .set_storage_cap(StorageCap::from_bytes(2 * gib).unwrap())
            .unwrap();
        assert_eq!(report.map(|r| r.segments_shredded), Some(1));
        let inv = c.inventory().unwrap();
        assert_eq!(inv.rows().len(), 2);
        assert_eq!(inv.rows()[0].day, UtcDay::from_days_since_epoch(11));
        assert_eq!(c.status().store_cap_bytes, 2 * gib);
        let second = c.enforce_storage_cap().unwrap();
        assert_eq!(second, None);
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
    fn a_zero_cap_in_the_settings_file_is_clamped_to_the_minimum_and_shreds_nothing_at_open() {
        let file = temp_settings();
        std::fs::create_dir_all(file.path().parent().unwrap()).unwrap();
        std::fs::write(file.path(), r#"{"storage_cap": 0}"#).unwrap();
        assert_eq!(file.load().unwrap().storage_cap, StorageCap::MIN);
        let mut c = ConsentController::open(
            Box::new(MockStore {
                rows: vec![
                    row(20_764, Modality::ScreenFrame, 1, 10),
                    row(20_765, Modality::ScreenFrame, 1, 10),
                    row(20_766, Modality::ScreenFrame, 1, 10),
                ],
                shred_all_calls: 0,
                shred_before_calls: vec![],
            }),
            file,
        )
        .unwrap();
        assert_eq!(c.storage_cap(), StorageCap::MIN);
        assert_eq!(c.status().store_cap_bytes, StorageCap::MIN.bytes());
        assert_eq!(c.inventory().unwrap().rows().len(), 3);
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
    fn audio_and_camera_are_off_by_default_and_the_r1_modalities_on() {
        let c = controller(vec![]);
        for m in [
            Modality::SystemAudio,
            Modality::Microphone,
            Modality::Camera,
        ] {
            assert_eq!(c.modality_consent(m), ModalityConsent::Disabled, "{m:?}");
        }
        for m in [
            Modality::ScreenFrame,
            Modality::Keystroke,
            Modality::Pointer,
            Modality::ForegroundApp,
            Modality::WindowTitle,
        ] {
            assert_eq!(c.modality_consent(m), ModalityConsent::Enabled, "{m:?}");
        }
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
    fn settings_load_defaults_when_the_file_is_missing_and_save_is_atomic() {
        let file = temp_settings();
        let s = file.load().unwrap();
        assert_eq!(s.capture, CaptureState::Running);
        assert_eq!(s.storage_cap, StorageCap::DEFAULT);
        assert!(s.permissions_once_granted.is_empty());
        file.save(&s).unwrap();
        file.save(&s).unwrap();
        assert!(!file.path().with_extension("json.tmp").exists());
        assert_eq!(file.load().unwrap(), s);
    }
}
