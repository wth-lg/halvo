//! Capture status, stored-data inventory, pause and the storage cap: the types
//! the data view binds to and the controller behind its commands (phase P5).
//!
//! The view reads and deletes through [`DataStore`], which is the store's expiry
//! (delete) side plus the read-only [`StoreInventory`]. It never holds the append
//! side ([`crate::store::SegmentStore`]), so the UI process cannot become a second
//! writer of the segment store; the capture daemon stays the sole writer.
//!
//! Pause and the storage cap live in one small settings file, [`SettingsFile`],
//! whose only writer is the consent view. The capture daemon reads it (through
//! [`ConsentSettings::load`]) and emits nothing while the state is
//! [`CaptureState::Paused`]; the state is one enum in one place, so a paused
//! capture cannot also be recording.
//!
//! The observation window is fixed ([`ObservationWindow`]) and is never asked
//! about: the user controls what is kept with pause, delete-all and the cap.

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

/// The bundle identifier the settings directory is named after. It must match
/// `identifier` in src-tauri/tauri.conf.json so the app and the daemon read the
/// same file.
pub const APP_IDENTIFIER: &str = "com.halvo.desktop";

/// The file name of the consent settings inside the app's config directory.
pub const SETTINGS_FILE_NAME: &str = "consent.json";

/// The rolling observation window (decision item 5). Capture never stops; raw
/// capture older than the window is crypto-shredded by the store's expiry pass;
/// the first workflow report comes after the first report day. Fixed in R1 and
/// not a setting, so no onboarding step asks about it.
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

/// The storage cap in bytes. When the store grows past it, the oldest days are
/// crypto-shredded first until the store is under the cap again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StorageCap(u64);

impl StorageCap {
    pub const GIB: u64 = 1024 * 1024 * 1024;
    /// 25 GiB: the smallest default Microsoft Recall ships (researcher-3 #27),
    /// above the 14 GB a 14-day window costs at the 1 GB/day Screenpipe figure.
    pub const DEFAULT: StorageCap = StorageCap(25 * Self::GIB);
    /// Below one day of capture the cap would shred the current day on every
    /// pass, so 1 GiB is the floor.
    pub const MIN: StorageCap = StorageCap(Self::GIB);

    pub fn from_bytes(bytes: u64) -> Result<Self, ConsentError> {
        if bytes < Self::MIN.0 {
            return Err(ConsentError::Action(format!(
                "the storage cap must be at least {} bytes (1 GiB); {bytes} was given",
                Self::MIN.0
            )));
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

/// A calendar day in UTC, as days since 1970-01-01. The inventory is bucketed by
/// it and the cap evicts whole days, oldest first. Serializes as `YYYY-MM-DD`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UtcDay(i64);

impl UtcDay {
    pub const fn from_days_since_epoch(days: i64) -> Self {
        Self(days)
    }

    pub const fn days_since_epoch(self) -> i64 {
        self.0
    }

    pub fn from_system_time(at: SystemTime) -> Self {
        let days = match at.duration_since(UNIX_EPOCH) {
            Ok(after) => (after.as_secs() / 86_400) as i64,
            Err(before) => -((before.duration().as_secs().div_ceil(86_400)) as i64),
        };
        Self(days)
    }

    pub fn today() -> Self {
        Self::from_system_time(SystemTime::now())
    }

    pub const fn next(self) -> Self {
        Self(self.0 + 1)
    }

    /// Proleptic Gregorian (year, month, day) for this day.
    pub fn civil(self) -> (i64, u32, u32) {
        let z = self.0 + 719_468;
        let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        let year = yoe + era * 400 + i64::from(month <= 2);
        (year, month, day)
    }

    pub fn from_civil(year: i64, month: u32, day: u32) -> Self {
        let y = if month <= 2 { year - 1 } else { year };
        let era = if y >= 0 { y } else { y - 399 } / 400;
        let yoe = y - era * 400;
        let m = i64::from(month);
        let mp = if m > 2 { m - 3 } else { m + 9 };
        let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        Self(era * 146_097 + doe - 719_468)
    }
}

impl fmt::Display for UtcDay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (y, m, d) = self.civil();
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
        Ok(Self::from_civil(year, month, day))
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

/// One inventory line: how many events of one modality were captured on one day
/// and how many bytes they occupy in the store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryRow {
    pub day: UtcDay,
    pub modality: Modality,
    pub events: u64,
    pub bytes: u64,
}

/// What is stored, by modality and day, oldest day first. This is the list the
/// data view shows so the user can see what Halvo holds before deleting it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Inventory {
    pub rows: Vec<InventoryRow>,
}

impl Inventory {
    /// Build an inventory, sorting rows by day then modality so the order is the
    /// same whichever store produced them.
    pub fn from_rows(mut rows: Vec<InventoryRow>) -> Self {
        rows.sort_by(|a, b| {
            a.day
                .cmp(&b.day)
                .then_with(|| modality_order(a.modality).cmp(&modality_order(b.modality)))
        });
        Self { rows }
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

/// The read side the data view needs that [`crate::store::SegmentStore`] does
/// not declare: the stored inventory, and oldest-first eviction for the cap. The
/// encrypted store (phase P2) implements it next to its expiry pass; until P6
/// wires that store into the app, the app binds a stand-in that holds nothing.
pub trait StoreInventory {
    /// Everything stored, by modality and day.
    fn inventory(&self) -> StoreResult<Inventory>;

    /// Crypto-shred every segment captured before the start of `day` (UTC), so
    /// the store is left holding `day` and newer only.
    fn crypto_shred_before(&mut self, day: UtcDay) -> StoreResult<ShredReport>;
}

/// Everything the data view touches in the store: delete (expiry) and read
/// (inventory). Deliberately not [`crate::store::SegmentStore`]: the view never
/// appends.
pub trait DataStore: SegmentExpiry + StoreInventory {}

impl<T: SegmentExpiry + StoreInventory> DataStore for T {}

impl From<StoreError> for ConsentError {
    fn from(error: StoreError) -> Self {
        ConsentError::Action(error.to_string())
    }
}

/// The user's capture controls, persisted. One writer: the consent view. The
/// daemon reads `capture` and emits nothing while it is paused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ConsentSettings {
    pub version: u32,
    pub capture: CaptureState,
    pub storage_cap: StorageCap,
    /// Every permission Halvo has held at some point on this install. A member
    /// that is not granted now is a lost grant the onboarding walks back.
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
    /// Load the settings from the default path; a missing file is the defaults.
    pub fn load_default() -> Result<Self, ConsentError> {
        SettingsFile::default_location().load()
    }

    /// Load the settings from `path`; a missing file is the defaults.
    pub fn load(path: &Path) -> Result<Self, ConsentError> {
        SettingsFile::new(path.to_path_buf()).load()
    }
}

/// The consent settings file and its atomic writer. Safe to save twice: each
/// save writes a sibling temp file and renames it over the target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsFile {
    path: PathBuf,
}

impl SettingsFile {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// `<app config dir>/consent.json`, where the config dir is the platform's
    /// per-user application directory named after [`APP_IDENTIFIER`]: the same
    /// directory Tauri's `app_config_dir` resolves for this identifier.
    pub fn default_location() -> Self {
        Self::new(default_config_dir().join(SETTINGS_FILE_NAME))
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

fn default_config_dir() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    {
        home.unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("Library/Application Support")
            .join(APP_IDENTIFIER)
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .or(home)
            .unwrap_or_else(std::env::temp_dir)
            .join(APP_IDENTIFIER)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| home.map(|h| h.join(".config")))
            .unwrap_or_else(std::env::temp_dir)
            .join(APP_IDENTIFIER)
    }
}

/// The modality policy for R1: screen, keystroke, pointer, foreground app and
/// window title are on; system audio, microphone and camera are off and are not
/// requested (researcher-3 item 10). Not a setting in R1.
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

/// The first day to keep so that the store fits under `cap_bytes`, removing
/// whole days oldest first; `None` when the store already fits. Every segment
/// before the returned day is shredded.
pub fn eviction_cutoff(inventory: &Inventory, cap_bytes: u64) -> Option<UtcDay> {
    let mut remaining = inventory.total_bytes();
    if remaining <= cap_bytes {
        return None;
    }
    let mut cutoff = None;
    for row in &inventory.rows {
        if remaining <= cap_bytes {
            break;
        }
        match cutoff {
            Some(day) if day > row.day => {}
            _ => cutoff = Some(row.day.next()),
        }
        remaining = remaining.saturating_sub(row.bytes);
    }
    cutoff
}

/// The controller behind the data view: status, inventory, pause, delete-all
/// and the storage cap, against a [`DataStore`] and one [`SettingsFile`].
pub struct ConsentController {
    store: Box<dyn DataStore + Send>,
    settings_file: SettingsFile,
    settings: ConsentSettings,
    last_inventory: Inventory,
}

impl ConsentController {
    /// Load the settings, read the inventory and enforce the cap once, so a
    /// store that grew past the cap while the view was closed is trimmed on open.
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

    /// Re-read the inventory from the store and return it.
    pub fn inventory(&mut self) -> Result<&Inventory, ConsentError> {
        self.refresh_inventory()?;
        Ok(&self.last_inventory)
    }

    fn refresh_inventory(&mut self) -> Result<(), ConsentError> {
        self.last_inventory = self.store.inventory()?;
        Ok(())
    }

    /// Crypto-shred everything through the store's delete-all. Safe twice: a
    /// second call shreds nothing and reports zero.
    pub fn delete_everything(&mut self) -> Result<ShredReport, ConsentError> {
        let report = self.store.crypto_shred_all()?;
        self.refresh_inventory()?;
        Ok(report)
    }

    /// Set and persist the cap, then trim the store to it, oldest days first.
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

    /// Trim the store to the cap, oldest days first. `Ok(None)` when nothing
    /// had to go. Safe twice.
    pub fn enforce_storage_cap(&mut self) -> Result<Option<ShredReport>, ConsentError> {
        self.refresh_inventory()?;
        let Some(cutoff) = eviction_cutoff(&self.last_inventory, self.settings.storage_cap.bytes())
        else {
            return Ok(None);
        };
        let report = self.store.crypto_shred_before(cutoff)?;
        self.refresh_inventory()?;
        Ok(Some(report))
    }

    /// Record which permissions are granted now, so a later loss is detected.
    /// Persists only when the set grew.
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

    /// Pause or resume. Persisted so the daemon sees it; a no-op when already
    /// in that state.
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
        assert_eq!(inv.rows.len(), 3);
        assert_eq!(inv.rows[0].day, UtcDay::from_days_since_epoch(19_999));
        assert_eq!(inv.rows[2].day, UtcDay::from_days_since_epoch(20_000));
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
        assert!(c.inventory().unwrap().rows.is_empty());
        assert_eq!(c.status().store_bytes, 0);
        let again = c.delete_everything().unwrap();
        assert_eq!(again.segments_shredded, 0);
    }

    #[test]
    fn delete_all_through_the_consent_view_trait() {
        let mut c = controller(vec![row(1, Modality::Pointer, 2, 20)]);
        ConsentView::delete_all(&mut c).unwrap();
        assert!(c.inventory().unwrap().rows.is_empty());
    }

    #[test]
    fn the_cap_cutoff_removes_the_oldest_days_first() {
        let inv = Inventory::from_rows(vec![
            row(10, Modality::ScreenFrame, 1, 400),
            row(11, Modality::ScreenFrame, 1, 400),
            row(12, Modality::ScreenFrame, 1, 400),
        ]);
        assert_eq!(eviction_cutoff(&inv, 1200), None);
        assert_eq!(
            eviction_cutoff(&inv, 900),
            Some(UtcDay::from_days_since_epoch(11))
        );
        assert_eq!(
            eviction_cutoff(&inv, 400),
            Some(UtcDay::from_days_since_epoch(12))
        );
        assert_eq!(
            eviction_cutoff(&inv, 100),
            Some(UtcDay::from_days_since_epoch(13))
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
        assert_eq!(inv.rows.len(), 2);
        assert_eq!(inv.rows[0].day, UtcDay::from_days_since_epoch(11));
        assert_eq!(c.status().store_cap_bytes, 2 * gib);
        let second = c.enforce_storage_cap().unwrap();
        assert_eq!(second, None);
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
