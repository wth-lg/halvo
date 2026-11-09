//! Installed-software inventory from the registry.
//!
//! The installed-application list lives under the `Uninstall` registry keys, in
//! HKLM and HKCU and their 32-bit `WOW6432Node` mirrors (researcher-1 #12). Unlike
//! the other Windows signals this is a scan, not a push event: it has no place in
//! the [`crate::event::Modality`] set, so it is surfaced as an
//! [`InstalledSoftwareSnapshot`] rather than forced into the capture stream. New
//! installs and removals are noticed with `RegNotifyChangeKeyValue` on the keys,
//! which blocks until something under them changes, so the inventory is refreshed
//! on change instead of polled.
//!
//! The parse from raw registry values to an [`InstalledApp`] is pure and unit
//! tested; the registry read and the change watch are deferred with the rest of
//! Windows functional capture (they need a live registry), and are compiled and
//! type-checked here.

#[cfg(target_os = "windows")]
use std::io;

/// One installed application, as the inventory records it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct InstalledApp {
    pub name: String,
    pub version: Option<String>,
    pub publisher: Option<String>,
}

/// The raw values read from one `Uninstall` subkey, before filtering. Kept
/// separate from the registry read so the filter rules are testable without a
/// registry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawUninstallEntry {
    pub display_name: Option<String>,
    pub display_version: Option<String>,
    pub publisher: Option<String>,
    /// `SystemComponent` DWORD: `Some(1)` marks an OS component, not a user app.
    pub system_component: Option<u32>,
}

/// Turn one raw `Uninstall` subkey into an [`InstalledApp`], or drop it. An entry
/// is dropped when it has no display name (an update or orphan stub) or when it is
/// flagged a system component, so the inventory lists user-facing applications
/// only.
pub fn parse_entry(raw: &RawUninstallEntry) -> Option<InstalledApp> {
    if raw.system_component == Some(1) {
        return None;
    }
    let name = raw.display_name.as_ref()?.trim();
    if name.is_empty() {
        return None;
    }
    Some(InstalledApp {
        name: name.to_string(),
        version: raw.display_version.as_ref().map(|v| v.trim().to_string()),
        publisher: raw.publisher.as_ref().map(|p| p.trim().to_string()),
    })
}

/// A point-in-time list of installed applications, deduplicated and sorted so two
/// scans of an unchanged system compare equal.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InstalledSoftwareSnapshot {
    pub apps: Vec<InstalledApp>,
}

impl InstalledSoftwareSnapshot {
    /// Build a snapshot from raw `Uninstall` entries: filter to user apps, drop
    /// exact duplicates (the same app appears under both the native and the
    /// `WOW6432Node` mirror), and sort so the result is stable and comparable.
    pub fn from_entries<I>(entries: I) -> Self
    where
        I: IntoIterator<Item = RawUninstallEntry>,
    {
        let mut apps: Vec<InstalledApp> = entries
            .into_iter()
            .filter_map(|e| parse_entry(&e))
            .collect();
        apps.sort();
        apps.dedup();
        Self { apps }
    }

    pub fn len(&self) -> usize {
        self.apps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.apps.is_empty()
    }
}

/// The registry roots the inventory scans: HKLM and HKCU, each at the native and
/// the 32-bit `WOW6432Node` `Uninstall` path.
#[cfg(target_os = "windows")]
const UNINSTALL_PATHS: &[&str] = &[
    "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
    "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
];

/// Read one `Uninstall` subkey into a [`RawUninstallEntry`]. Missing values are
/// `None`; only read failures other than "value not found" would surface, and
/// those are treated as absent so one broken entry never fails the whole scan.
#[cfg(target_os = "windows")]
fn read_entry(key: &winreg::RegKey) -> RawUninstallEntry {
    RawUninstallEntry {
        display_name: key.get_value("DisplayName").ok(),
        display_version: key.get_value("DisplayVersion").ok(),
        publisher: key.get_value("Publisher").ok(),
        system_component: key.get_value::<u32, _>("SystemComponent").ok(),
    }
}

/// Scan HKLM and HKCU for installed applications and return the snapshot. Deferred
/// at runtime with functional capture; compiled and type-checked here.
#[cfg(target_os = "windows")]
pub fn scan_installed() -> io::Result<InstalledSoftwareSnapshot> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ};
    use winreg::RegKey;

    let mut entries = Vec::new();
    for root in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        let hive = RegKey::predef(root);
        for path in UNINSTALL_PATHS {
            let Ok(uninstall) = hive.open_subkey_with_flags(path, KEY_READ) else {
                continue;
            };
            for name in uninstall.enum_keys().flatten() {
                if let Ok(sub) = uninstall.open_subkey_with_flags(&name, KEY_READ) {
                    entries.push(read_entry(&sub));
                }
            }
        }
    }
    Ok(InstalledSoftwareSnapshot::from_entries(entries))
}

/// Block until something under the HKCU `Uninstall` key changes, so the caller
/// can rescan only when the installed set actually moves rather than polling.
/// Deferred at runtime with functional capture; compiled and type-checked here.
#[cfg(target_os = "windows")]
pub fn wait_for_change() -> io::Result<()> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegNotifyChangeKeyValue, REG_NOTIFY_CHANGE_LAST_SET, REG_NOTIFY_CHANGE_NAME,
    };
    use winreg::enums::{HKEY_CURRENT_USER, KEY_NOTIFY};
    use winreg::RegKey;

    let key =
        RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(UNINSTALL_PATHS[0], KEY_NOTIFY)?;
    // Synchronous watch: the call returns when a subkey is added or removed or a
    // value is set, which is exactly an install or uninstall.
    let status = unsafe {
        RegNotifyChangeKeyValue(
            key.raw_handle(),
            1, // watch the whole subtree
            REG_NOTIFY_CHANGE_NAME | REG_NOTIFY_CHANGE_LAST_SET,
            std::ptr::null_mut(),
            0, // synchronous: block until a change
        )
    };
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(status as i32))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str) -> RawUninstallEntry {
        RawUninstallEntry {
            display_name: Some(name.to_string()),
            display_version: Some("1.0".to_string()),
            publisher: Some("Acme".to_string()),
            system_component: None,
        }
    }

    #[test]
    fn an_entry_without_a_display_name_is_dropped() {
        assert!(parse_entry(&RawUninstallEntry::default()).is_none());
        assert!(parse_entry(&RawUninstallEntry {
            display_name: Some("   ".to_string()),
            ..Default::default()
        })
        .is_none());
    }

    #[test]
    fn a_system_component_is_dropped() {
        let raw = RawUninstallEntry {
            display_name: Some("KB5000000".to_string()),
            system_component: Some(1),
            ..Default::default()
        };
        assert!(parse_entry(&raw).is_none());
    }

    #[test]
    fn a_user_app_parses_with_its_version_and_publisher() {
        let app = parse_entry(&entry("Contoso Editor")).unwrap();
        assert_eq!(app.name, "Contoso Editor");
        assert_eq!(app.version.as_deref(), Some("1.0"));
        assert_eq!(app.publisher.as_deref(), Some("Acme"));
    }

    #[test]
    fn a_snapshot_dedupes_the_wow6432node_mirror_and_sorts() {
        let snap = InstalledSoftwareSnapshot::from_entries([
            entry("Zeta"),
            entry("Alpha"),
            entry("Zeta"), // the same app under the 32-bit mirror
        ]);
        assert_eq!(snap.len(), 2);
        assert_eq!(snap.apps[0].name, "Alpha");
        assert_eq!(snap.apps[1].name, "Zeta");
    }

    #[test]
    fn two_scans_of_the_same_set_compare_equal() {
        let a = InstalledSoftwareSnapshot::from_entries([entry("A"), entry("B")]);
        let b = InstalledSoftwareSnapshot::from_entries([entry("B"), entry("A")]);
        assert_eq!(a, b);
    }
}
