use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use halvo_control::model::{Inventory, InventoryRow, ObservationWindow, StorageCap, UtcDay};

use super::onboarding::Permission;
use super::{CaptureState, CaptureStatus, ConsentError, ConsentView, ModalityConsent};
use crate::event::Modality;
use crate::store::{SegmentExpiry, ShredReport, StoreError, StoreResult};

pub const SETTINGS_FILE_NAME: &str = "consent.json";

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
        match row.day.next() {
            Some(next) => cutoff = Some(next),
            None => break,
        }
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
            Ok(Inventory::from_rows(self.rows.clone()).unwrap())
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
            day: UtcDay::from_days_since_epoch(day).unwrap(),
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
        assert_eq!(
            inv.rows()[0].day,
            UtcDay::from_days_since_epoch(19_999).unwrap()
        );
        assert_eq!(
            inv.rows()[2].day,
            UtcDay::from_days_since_epoch(20_000).unwrap()
        );
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
        ])
        .unwrap();
        assert_eq!(eviction_cutoff(&inv, cap(12 * gib)), None);
        assert_eq!(
            eviction_cutoff(&inv, cap(9 * gib)),
            Some(UtcDay::from_days_since_epoch(11).unwrap())
        );
        assert_eq!(
            eviction_cutoff(&inv, cap(4 * gib)),
            Some(UtcDay::from_days_since_epoch(12).unwrap())
        );
        assert_eq!(
            eviction_cutoff(&inv, StorageCap::MIN),
            Some(UtcDay::from_days_since_epoch(13).unwrap())
        );
    }

    #[test]
    fn an_inventory_built_from_unsorted_rows_still_evicts_the_oldest_day_only() {
        let gib = StorageCap::GIB;
        let inv = Inventory::from_rows(vec![
            row(12, Modality::ScreenFrame, 1, gib),
            row(10, Modality::ScreenFrame, 1, gib),
            row(11, Modality::ScreenFrame, 1, gib),
        ])
        .unwrap();
        let days: Vec<i64> = inv
            .rows()
            .iter()
            .map(|r| r.day.days_since_epoch())
            .collect();
        assert_eq!(days, vec![10, 11, 12]);
        assert_eq!(
            eviction_cutoff(&inv, StorageCap::from_bytes(2 * gib).unwrap()),
            Some(UtcDay::from_days_since_epoch(11).unwrap())
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
        assert_eq!(
            inv.rows()[0].day,
            UtcDay::from_days_since_epoch(11).unwrap()
        );
        assert_eq!(c.status().store_cap_bytes, 2 * gib);
        let second = c.enforce_storage_cap().unwrap();
        assert_eq!(second, None);
    }

    #[test]
    fn a_cap_below_the_minimum_in_the_settings_file_is_an_open_error_and_shreds_nothing() {
        struct NoShredStore;
        impl SegmentExpiry for NoShredStore {
            fn crypto_shred_expired(&mut self) -> StoreResult<ShredReport> {
                panic!("no shred may run")
            }
            fn crypto_shred_all(&mut self) -> StoreResult<ShredReport> {
                panic!("no shred may run")
            }
        }
        impl StoreInventory for NoShredStore {
            fn inventory(&self) -> StoreResult<Inventory> {
                panic!("the store is not read before the settings load")
            }
            fn crypto_shred_before(&mut self, _day: UtcDay) -> StoreResult<ShredReport> {
                panic!("no shred may run")
            }
        }
        for stored in ["0", "5", "1073741823"] {
            let file = temp_settings();
            std::fs::create_dir_all(file.path().parent().unwrap()).unwrap();
            std::fs::write(file.path(), format!(r#"{{"storage_cap": {stored}}}"#)).unwrap();
            assert!(file.load().is_err(), "{stored}");
            assert!(
                ConsentController::open(Box::new(NoShredStore), file).is_err(),
                "{stored}"
            );
        }
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
