//! Installed-software inventory from the registry.

#[cfg(target_os = "windows")]
use std::io;

/// One installed application, as the inventory records it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct InstalledApp {
    pub name: String,
    pub version: Option<String>,
    pub publisher: Option<String>,
}

/// The raw values read from one `Uninstall` subkey, before filtering.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawUninstallEntry {
    pub display_name: Option<String>,
    pub display_version: Option<String>,
    pub publisher: Option<String>,
    /// True when the entry's `SystemComponent` DWORD is 1: an OS component, not a
    /// user-facing application. The DWORD is parsed to this bool at the registry
    /// boundary, so the magic `1` lives in one place.
    pub is_system_component: bool,
}

/// Turn one raw `Uninstall` subkey into an [`InstalledApp`], or drop it. An entry
/// is dropped when it has no display name (an update or orphan stub) or when it is
/// flagged a system component, so the inventory lists user-facing applications
/// only.
pub fn parse_entry(raw: &RawUninstallEntry) -> Option<InstalledApp> {
    if raw.is_system_component {
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

#[cfg(target_os = "windows")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UninstallRoot {
    LocalMachine,
    CurrentUser,
}

#[cfg(target_os = "windows")]
impl UninstallRoot {
    fn hkey(self) -> winreg::HKEY {
        use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
        match self {
            UninstallRoot::LocalMachine => HKEY_LOCAL_MACHINE,
            UninstallRoot::CurrentUser => HKEY_CURRENT_USER,
        }
    }
}

#[cfg(target_os = "windows")]
const UNINSTALL_ROOTS: [UninstallRoot; 2] =
    [UninstallRoot::LocalMachine, UninstallRoot::CurrentUser];

#[cfg(target_os = "windows")]
const UNINSTALL_PATHS: &[&str] = &[
    "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
    "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
];

#[cfg(target_os = "windows")]
fn scan_targets() -> Vec<(UninstallRoot, &'static str)> {
    let mut targets = Vec::new();
    for root in UNINSTALL_ROOTS {
        for path in UNINSTALL_PATHS {
            targets.push((root, *path));
        }
    }
    targets
}

#[cfg(target_os = "windows")]
fn read_entry(key: &winreg::RegKey) -> RawUninstallEntry {
    RawUninstallEntry {
        display_name: key.get_value("DisplayName").ok(),
        display_version: key.get_value("DisplayVersion").ok(),
        publisher: key.get_value("Publisher").ok(),
        is_system_component: key.get_value::<u32, _>("SystemComponent").ok() == Some(1),
    }
}

/// Scan every watched root and path for installed applications and return the
/// snapshot.
#[cfg(target_os = "windows")]
pub fn scan_installed() -> io::Result<InstalledSoftwareSnapshot> {
    use winreg::enums::KEY_READ;
    use winreg::RegKey;

    let mut entries = Vec::new();
    for (root, path) in scan_targets() {
        let hive = RegKey::predef(root.hkey());
        let Ok(uninstall) = hive.open_subkey_with_flags(path, KEY_READ) else {
            continue;
        };
        for name in uninstall.enum_keys().flatten() {
            if let Ok(sub) = uninstall.open_subkey_with_flags(&name, KEY_READ) {
                entries.push(read_entry(&sub));
            }
        }
    }
    Ok(InstalledSoftwareSnapshot::from_entries(entries))
}

#[cfg(target_os = "windows")]
pub fn wait_for_change() -> io::Result<()> {
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_SUCCESS, FALSE, HANDLE, TRUE, WAIT_FAILED,
    };
    use windows_sys::Win32::System::Registry::{
        RegNotifyChangeKeyValue, REG_NOTIFY_CHANGE_LAST_SET, REG_NOTIFY_CHANGE_NAME,
    };
    use windows_sys::Win32::System::Threading::{CreateEventW, WaitForMultipleObjects, INFINITE};
    use winreg::enums::KEY_NOTIFY;
    use winreg::RegKey;

    let filter = REG_NOTIFY_CHANGE_NAME | REG_NOTIFY_CHANGE_LAST_SET;
    let mut keys = Vec::new();
    let mut events: Vec<HANDLE> = Vec::new();

    let close_all = |events: &[HANDLE]| {
        for event in events {
            // SAFETY: each handle is an event that this function created and
            // has not closed; each is closed once here.
            unsafe {
                let _ = CloseHandle(*event);
            }
        }
    };

    for (root, path) in scan_targets() {
        let Ok(key) = RegKey::predef(root.hkey()).open_subkey_with_flags(path, KEY_NOTIFY) else {
            continue;
        };
        // SAFETY: CreateEventW with null attributes and a null name makes a new
        // unnamed manual-reset event, initially unset.
        let event = unsafe { CreateEventW(std::ptr::null(), TRUE, FALSE, std::ptr::null()) };
        if event.is_null() {
            let err = io::Error::last_os_error();
            close_all(&events);
            return Err(err);
        }
        // SAFETY: `key` is an open registry key that outlives the wait, and
        // `event` is the live event created above.
        let status =
            unsafe { RegNotifyChangeKeyValue(key.raw_handle(), TRUE, filter, event, TRUE) };
        if status != ERROR_SUCCESS {
            // SAFETY: `event` is the live event created above, not yet in
            // `events`, so it is closed once here.
            unsafe {
                let _ = CloseHandle(event);
            }
            close_all(&events);
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        keys.push(key);
        events.push(event);
    }

    if events.is_empty() {
        return Ok(());
    }

    // SAFETY: the pointer and the count describe `events`, which holds live
    // event handles and outlives the wait.
    let wait =
        unsafe { WaitForMultipleObjects(events.len() as u32, events.as_ptr(), FALSE, INFINITE) };
    close_all(&events);
    drop(keys);
    if wait == WAIT_FAILED {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str) -> RawUninstallEntry {
        RawUninstallEntry {
            display_name: Some(name.to_string()),
            display_version: Some("1.0".to_string()),
            publisher: Some("Acme".to_string()),
            is_system_component: false,
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
            is_system_component: true,
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
        let snap =
            InstalledSoftwareSnapshot::from_entries([entry("Zeta"), entry("Alpha"), entry("Zeta")]);
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

    #[cfg(target_os = "windows")]
    #[test]
    fn the_watch_covers_every_root_and_path_the_scan_reads() {
        let targets = scan_targets();
        assert_eq!(targets.len(), UNINSTALL_ROOTS.len() * UNINSTALL_PATHS.len());
        for root in UNINSTALL_ROOTS {
            for path in UNINSTALL_PATHS {
                assert!(
                    targets.contains(&(root, *path)),
                    "scan/watch set is missing {root:?} {path}"
                );
            }
        }
        assert!(targets
            .iter()
            .any(|(root, _)| *root == UninstallRoot::LocalMachine));
    }
}
