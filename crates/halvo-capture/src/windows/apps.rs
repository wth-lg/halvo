//! Installed-software inventory from the registry and the package manager.

#[cfg(target_os = "windows")]
use std::io;
#[cfg(target_os = "windows")]
use std::time::Duration;

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
    /// True when the entry names a `ParentKeyName`: an update or a part of
    /// another entry, not an application of its own.
    pub has_parent: bool,
}

/// Turn one raw `Uninstall` subkey into an [`InstalledApp`], or drop it. An entry
/// is dropped when it has no display name (an update or orphan stub), when it is
/// flagged a system component, or when it names a parent entry, so the inventory
/// lists user-facing applications only.
pub fn parse_entry(raw: &RawUninstallEntry) -> Option<InstalledApp> {
    if raw.is_system_component || raw.has_parent {
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

/// The installed applications, sorted and with no doubles.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InstalledSoftwareSnapshot {
    apps: Vec<InstalledApp>,
}

impl InstalledSoftwareSnapshot {
    /// Build a snapshot from raw `Uninstall` entries and the apps of the
    /// package manager: filter to user apps, drop exact duplicates (the same
    /// app appears under both the native and the `WOW6432Node` mirror), and
    /// sort so the result is stable and comparable.
    pub fn from_entries<I>(entries: I) -> Self
    where
        I: IntoIterator<Item = RawUninstallEntry>,
    {
        Self::from_parts(entries, Vec::new())
    }

    fn from_parts<I>(entries: I, packaged: Vec<InstalledApp>) -> Self
    where
        I: IntoIterator<Item = RawUninstallEntry>,
    {
        let mut apps: Vec<InstalledApp> = entries
            .into_iter()
            .filter_map(|e| parse_entry(&e))
            .chain(packaged)
            .collect();
        apps.sort();
        apps.dedup();
        Self { apps }
    }

    pub fn apps(&self) -> &[InstalledApp] {
        &self.apps
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
const UNINSTALL: &str = "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall";

#[cfg(target_os = "windows")]
const UNINSTALL_WOW: &str = "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall";

/// The keys that the scan reads and the watcher watches: the 32-bit mirror
/// exists under the machine's hive alone.
#[cfg(target_os = "windows")]
const SCAN_TARGETS: [(UninstallRoot, &str); 3] = [
    (UninstallRoot::LocalMachine, UNINSTALL),
    (UninstallRoot::LocalMachine, UNINSTALL_WOW),
    (UninstallRoot::CurrentUser, UNINSTALL),
];

#[cfg(target_os = "windows")]
fn read_entry(key: &OpenKey) -> RawUninstallEntry {
    let key = &key.0;
    RawUninstallEntry {
        display_name: key.get_value("DisplayName").ok(),
        display_version: key.get_value("DisplayVersion").ok(),
        publisher: key.get_value("Publisher").ok(),
        is_system_component: key.get_value::<u32, _>("SystemComponent").ok() == Some(1),
        has_parent: key
            .get_value::<String, _>("ParentKeyName")
            .is_ok_and(|parent| !parent.trim().is_empty()),
    }
}

/// An open registry key; the handle is closed when the value drops.
#[cfg(target_os = "windows")]
struct OpenKey(winreg::RegKey);

#[cfg(target_os = "windows")]
impl OpenKey {
    fn open(parent: &winreg::RegKey, path: &str, access: u32) -> io::Result<OpenKey> {
        let key = parent.open_subkey_with_flags(path, access)?;
        #[cfg(test)]
        tests::OPEN_KEYS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(OpenKey(key))
    }
}

#[cfg(target_os = "windows")]
impl Drop for OpenKey {
    fn drop(&mut self) {
        #[cfg(test)]
        tests::OPEN_KEYS.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Scan every root and path that the watcher watches, and the packages of the
/// package manager for this user, and return the snapshot. A key that does not
/// exist is passed over; when none opens, or an enumeration fails, the scan
/// fails.
#[cfg(target_os = "windows")]
pub fn scan_installed() -> io::Result<InstalledSoftwareSnapshot> {
    let packaged = packaged_apps()?;
    scan_targets(&SCAN_TARGETS, packaged)
}

#[cfg(target_os = "windows")]
fn scan_targets(
    targets: &[(UninstallRoot, &str)],
    packaged: Vec<InstalledApp>,
) -> io::Result<InstalledSoftwareSnapshot> {
    use winreg::enums::KEY_READ;
    use winreg::RegKey;

    let mut entries = Vec::new();
    let mut opened = 0usize;
    for (root, path) in targets {
        let hive = RegKey::predef(root.hkey());
        let uninstall = match OpenKey::open(&hive, path, KEY_READ) {
            Ok(key) => key,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        opened += 1;
        for name in uninstall.0.enum_keys() {
            let name = name?;
            if let Ok(sub) = OpenKey::open(&uninstall.0, &name, KEY_READ) {
                entries.push(read_entry(&sub));
                drop(sub);
            }
        }
        drop(uninstall);
    }
    if opened == 0 {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no Uninstall key of the inventory could be opened",
        ));
    }
    Ok(InstalledSoftwareSnapshot::from_parts(entries, packaged))
}

/// The main packages that the package manager lists for this user: no
/// framework and no resource package.
#[cfg(target_os = "windows")]
fn packaged_apps() -> io::Result<Vec<InstalledApp>> {
    use windows::core::HSTRING;
    use windows::Management::Deployment::PackageManager;

    let os = |error: windows::core::Error| io::Error::from_raw_os_error(error.code().0);
    hold_process_mta()?;
    let manager = PackageManager::new().map_err(os)?;
    let mut apps = Vec::new();
    for package in manager
        .FindPackagesByUserSecurityId(&HSTRING::new())
        .map_err(os)?
    {
        if package.IsFramework().map_err(os)? || package.IsResourcePackage().map_err(os)? {
            continue;
        }
        let name = package.DisplayName().map_err(os)?.to_string();
        if name.trim().is_empty() {
            continue;
        }
        let version = package
            .Id()
            .and_then(|id| id.Version())
            .ok()
            .map(|v| format!("{}.{}.{}.{}", v.Major, v.Minor, v.Build, v.Revision));
        let publisher = package
            .PublisherDisplayName()
            .ok()
            .map(|publisher| publisher.to_string());
        apps.push(InstalledApp {
            name: name.trim().to_string(),
            version,
            publisher,
        });
    }
    Ok(apps)
}

#[cfg(target_os = "windows")]
fn hold_process_mta() -> io::Result<()> {
    use std::sync::{Mutex, PoisonError};
    static HELD: Mutex<bool> = Mutex::new(false);
    let mut held = HELD.lock().unwrap_or_else(PoisonError::into_inner);
    if !*held {
        // SAFETY: CoIncrementMTAUsage takes no pointer of ours; the cookie
        // that it returns is never passed to CoDecrementMTAUsage, so the
        // reference lasts for the life of the process.
        unsafe { windows::Win32::System::Com::CoIncrementMTAUsage() }
            .map_err(|error| io::Error::from_raw_os_error(error.code().0))?;
        *held = true;
    }
    Ok(())
}

/// An unnamed manual-reset event; the handle is closed when the value drops.
#[cfg(target_os = "windows")]
struct Event(windows_sys::Win32::Foundation::HANDLE);

// SAFETY: an event handle names a kernel object of the process, which any of
// its threads may set and wait on.
#[cfg(target_os = "windows")]
unsafe impl Send for Event {}
// SAFETY: as for Send; SetEvent and a wait on the same event may run on two
// threads at once.
#[cfg(target_os = "windows")]
unsafe impl Sync for Event {}

#[cfg(target_os = "windows")]
impl Event {
    fn new() -> io::Result<Event> {
        use windows_sys::Win32::Foundation::{FALSE, TRUE};
        use windows_sys::Win32::System::Threading::CreateEventW;
        // SAFETY: CreateEventW with null attributes and a null name makes a new
        // unnamed manual-reset event, initially unset.
        let event = unsafe { CreateEventW(std::ptr::null(), TRUE, FALSE, std::ptr::null()) };
        if event.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(Event(event))
    }

    fn set(&self) {
        // SAFETY: `self.0` is the live event that this value owns.
        unsafe {
            let _ = windows_sys::Win32::System::Threading::SetEvent(self.0);
        }
    }

    fn reset(&self) {
        // SAFETY: `self.0` is the live event that this value owns.
        unsafe {
            let _ = windows_sys::Win32::System::Threading::ResetEvent(self.0);
        }
    }
}

#[cfg(target_os = "windows")]
impl Drop for Event {
    fn drop(&mut self) {
        // SAFETY: `self.0` is the event that this value owns; it is closed once.
        unsafe {
            let _ = windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

/// One watched key and its event, armed with one filter for the life of the
/// handle; dropping the value closes both.
#[cfg(target_os = "windows")]
struct Watched {
    target: usize,
    at_parent: bool,
    key: OpenKey,
    event: Event,
}

#[cfg(target_os = "windows")]
impl Watched {
    /// The key of `path` under `root`, or, while it does not exist, its nearest
    /// parent that does, watched for a new subkey.
    fn arm(target: usize, root: UninstallRoot, path: &str) -> io::Result<Option<Watched>> {
        use winreg::enums::KEY_NOTIFY;
        use winreg::RegKey;

        let hive = RegKey::predef(root.hkey());
        let mut at = path;
        let mut at_parent = false;
        let key = loop {
            match OpenKey::open(&hive, at, KEY_NOTIFY) {
                Ok(key) => break key,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    let Some((parent, _)) = at.rsplit_once('\\') else {
                        return Ok(None);
                    };
                    at = parent;
                    at_parent = true;
                }
                Err(error) => return Err(error),
            }
        };
        let watched = Watched {
            target,
            at_parent,
            key,
            event: Event::new()?,
        };
        watched.notify()?;
        Ok(Some(watched))
    }

    fn notify(&self) -> io::Result<()> {
        use windows_sys::Win32::Foundation::{ERROR_SUCCESS, FALSE, TRUE};
        use windows_sys::Win32::System::Registry::{
            RegNotifyChangeKeyValue, REG_NOTIFY_CHANGE_LAST_SET, REG_NOTIFY_CHANGE_NAME,
            REG_NOTIFY_THREAD_AGNOSTIC,
        };
        let (subtree, filter) = if self.at_parent {
            (FALSE, REG_NOTIFY_CHANGE_NAME | REG_NOTIFY_THREAD_AGNOSTIC)
        } else {
            (
                TRUE,
                REG_NOTIFY_CHANGE_NAME | REG_NOTIFY_CHANGE_LAST_SET | REG_NOTIFY_THREAD_AGNOSTIC,
            )
        };
        self.event.reset();
        // SAFETY: the key is open for the life of this value, which outlives the
        // notification, and the event is the live event of this value.
        let status = unsafe {
            RegNotifyChangeKeyValue(self.key.0.raw_handle(), subtree, filter, self.event.0, TRUE)
        };
        if status != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        Ok(())
    }
}

/// What one wait of the watcher saw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchOutcome {
    /// A watched key changed; every key is armed again before the wait
    /// returns, so a change made while the caller rescans wakes the next wait.
    Changed,
    /// The stop event was set.
    Stopped,
    /// The time given to the wait passed with no change.
    TimedOut,
    /// No key and no parent of a key could be watched.
    NothingToWatch,
}

/// Sets the stop event of a watcher from another thread.
#[cfg(target_os = "windows")]
#[derive(Clone)]
pub struct WatchStopper(std::sync::Arc<Event>);

#[cfg(target_os = "windows")]
impl WatchStopper {
    pub fn stop(&self) {
        self.0.set();
    }
}

/// The watch over the keys that the scan reads. It owns each key and its event
/// across waits and closes them when it drops.
#[cfg(target_os = "windows")]
pub struct InstalledSoftwareWatcher {
    targets: Vec<(UninstallRoot, &'static str)>,
    watched: Vec<Watched>,
    stop: std::sync::Arc<Event>,
}

#[cfg(target_os = "windows")]
impl InstalledSoftwareWatcher {
    /// Arms a watch on every key that the scan reads.
    pub fn new() -> io::Result<InstalledSoftwareWatcher> {
        InstalledSoftwareWatcher::over(SCAN_TARGETS.to_vec())
    }

    fn over(targets: Vec<(UninstallRoot, &'static str)>) -> io::Result<Self> {
        let mut watcher = InstalledSoftwareWatcher {
            targets,
            watched: Vec::new(),
            stop: std::sync::Arc::new(Event::new()?),
        };
        for index in 0..watcher.targets.len() {
            watcher.arm(index)?;
        }
        Ok(watcher)
    }

    fn arm(&mut self, target: usize) -> io::Result<()> {
        self.watched.retain(|watched| watched.target != target);
        let (root, path) = self.targets[target];
        if let Some(watched) = Watched::arm(target, root, path)? {
            self.watched.push(watched);
        }
        Ok(())
    }

    pub fn stopper(&self) -> WatchStopper {
        WatchStopper(std::sync::Arc::clone(&self.stop))
    }

    /// Waits for a change of a watched key, the stop event or `limit`, with no
    /// limit for `None`. Each key that signalled is armed again before the
    /// wait returns, and a key that was watched at its parent is watched itself
    /// once it exists.
    pub fn wait(&mut self, limit: Option<Duration>) -> io::Result<WatchOutcome> {
        use windows_sys::Win32::Foundation::{
            FALSE, HANDLE, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
        };
        use windows_sys::Win32::System::Threading::{WaitForMultipleObjects, INFINITE};

        if self.watched.is_empty() {
            return Ok(WatchOutcome::NothingToWatch);
        }
        let mut handles: Vec<HANDLE> = Vec::with_capacity(self.watched.len() + 1);
        handles.push(self.stop.0);
        handles.extend(self.watched.iter().map(|watched| watched.event.0));
        let ms = limit.map_or(INFINITE, |limit| {
            u32::try_from(limit.as_millis()).unwrap_or(INFINITE - 1)
        });
        // SAFETY: the pointer and the count describe `handles`, which holds
        // live event handles that this watcher owns and that outlive the wait.
        let woke =
            unsafe { WaitForMultipleObjects(handles.len() as u32, handles.as_ptr(), FALSE, ms) };
        if woke == WAIT_FAILED {
            return Err(io::Error::last_os_error());
        }
        if woke == WAIT_TIMEOUT {
            return Ok(WatchOutcome::TimedOut);
        }
        if woke == WAIT_OBJECT_0 {
            return Ok(WatchOutcome::Stopped);
        }
        let signalled: Vec<usize> = self
            .watched
            .iter()
            .filter(|watched| is_set(&watched.event))
            .map(|watched| watched.target)
            .collect();
        for target in signalled {
            self.arm(target)?;
        }
        Ok(WatchOutcome::Changed)
    }
}

#[cfg(target_os = "windows")]
fn is_set(event: &Event) -> bool {
    use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
    use windows_sys::Win32::System::Threading::WaitForSingleObject;
    // SAFETY: the event is live for the call; a wait of 0 ms reads its state.
    unsafe { WaitForSingleObject(event.0, 0) == WAIT_OBJECT_0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "windows")]
    pub(super) static OPEN_KEYS: std::sync::atomic::AtomicIsize =
        std::sync::atomic::AtomicIsize::new(0);

    fn entry(name: &str) -> RawUninstallEntry {
        RawUninstallEntry {
            display_name: Some(name.to_string()),
            display_version: Some("1.0".to_string()),
            publisher: Some("Acme".to_string()),
            is_system_component: false,
            has_parent: false,
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
    fn an_entry_that_names_a_parent_is_dropped() {
        let raw = RawUninstallEntry {
            has_parent: true,
            ..entry("Contoso Editor Update 2")
        };
        assert!(parse_entry(&raw).is_none());
        assert!(parse_entry(&entry("Contoso Editor Update 2")).is_some());
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
        assert_eq!(snap.apps()[0].name, "Alpha");
        assert_eq!(snap.apps()[1].name, "Zeta");
    }

    #[test]
    fn two_scans_of_the_same_set_compare_equal() {
        let a = InstalledSoftwareSnapshot::from_entries([entry("A"), entry("B")]);
        let b = InstalledSoftwareSnapshot::from_entries([entry("B"), entry("A")]);
        assert_eq!(a, b);
    }

    #[cfg(target_os = "windows")]
    fn serial() -> std::sync::MutexGuard<'static, ()> {
        static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
        SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[cfg(target_os = "windows")]
    struct ScratchKey(String);

    #[cfg(target_os = "windows")]
    impl ScratchKey {
        fn new(name: &str) -> ScratchKey {
            let path = format!(
                "SOFTWARE\\HalvoCaptureTest\\{}-{name}-{}",
                std::process::id(),
                crate::windows::now_tick()
            );
            ScratchKey(path)
        }

        fn uninstall(&self) -> &'static str {
            Box::leak(format!("{}\\Uninstall", self.0).into_boxed_str())
        }

        fn write(&self, sub: &str, value: &str) {
            let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
            let (key, _) = hkcu
                .create_subkey(format!("{}\\Uninstall\\{sub}", self.0))
                .expect("create the scratch entry");
            key.set_value("DisplayName", &value.to_string())
                .expect("write the scratch name");
        }
    }

    #[cfg(target_os = "windows")]
    impl Drop for ScratchKey {
        fn drop(&mut self) {
            let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
            let _ = hkcu.delete_subkey_all(&self.0);
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn the_scan_and_the_watch_read_one_list_with_no_per_user_mirror() {
        let _serial = serial();
        let watcher = InstalledSoftwareWatcher::new().expect("the watcher");
        assert_eq!(watcher.targets, SCAN_TARGETS.to_vec());
        assert!(!SCAN_TARGETS.contains(&(UninstallRoot::CurrentUser, UNINSTALL_WOW)));
        assert!(SCAN_TARGETS.contains(&(UninstallRoot::LocalMachine, UNINSTALL_WOW)));
        assert!(SCAN_TARGETS.contains(&(UninstallRoot::CurrentUser, UNINSTALL)));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn a_scan_with_no_key_that_opens_fails() {
        let _serial = serial();
        let absent = ScratchKey::new("absent");
        let scanned = scan_targets(
            &[(UninstallRoot::CurrentUser, absent.uninstall())],
            Vec::new(),
        );
        assert!(scanned.is_err(), "{scanned:?}");
        absent.write("Entry", "Halvo Scan Probe");
        let scanned = scan_targets(
            &[(UninstallRoot::CurrentUser, absent.uninstall())],
            Vec::new(),
        )
        .expect("a scan of a key that opens");
        assert_eq!(scanned.len(), 1);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn a_watch_armed_before_the_key_exists_wakes_when_it_is_made() {
        let _serial = serial();
        let scratch = ScratchKey::new("late");
        let mut watcher =
            InstalledSoftwareWatcher::over(vec![(UninstallRoot::CurrentUser, scratch.uninstall())])
                .expect("the watcher");
        assert_eq!(
            watcher
                .wait(Some(Duration::from_millis(300)))
                .expect("a wait"),
            WatchOutcome::TimedOut,
            "a wake before any change"
        );
        scratch.write("A", "Halvo Watch Probe A");
        let mut woke = false;
        for _ in 0..4 {
            match watcher.wait(Some(Duration::from_secs(5))).expect("a wait") {
                WatchOutcome::Changed => {
                    woke = true;
                    if watcher.watched.iter().all(|watched| !watched.at_parent) {
                        break;
                    }
                }
                other => panic!("the wait answered {other:?}"),
            }
        }
        assert!(woke, "no wake within 5 s of the key's creation");
        assert!(watcher.watched.iter().all(|watched| !watched.at_parent));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn a_change_made_between_two_waits_wakes_the_second() {
        let _serial = serial();
        let scratch = ScratchKey::new("rearm");
        scratch.write("Seed", "Halvo Watch Seed");
        let mut watcher =
            InstalledSoftwareWatcher::over(vec![(UninstallRoot::CurrentUser, scratch.uninstall())])
                .expect("the watcher");
        scratch.write("A", "Halvo Watch Probe A");
        assert_eq!(
            watcher
                .wait(Some(Duration::from_secs(5)))
                .expect("the first wait"),
            WatchOutcome::Changed
        );
        scratch.write("B", "Halvo Watch Probe B");
        assert_eq!(
            watcher
                .wait(Some(Duration::from_secs(5)))
                .expect("the second wait"),
            WatchOutcome::Changed,
            "a change made before the second wait was missed"
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn the_stop_event_ends_a_wait_and_nothing_to_watch_has_its_own_outcome() {
        let _serial = serial();
        let scratch = ScratchKey::new("stop");
        scratch.write("Seed", "Halvo Watch Seed");
        let mut watcher =
            InstalledSoftwareWatcher::over(vec![(UninstallRoot::CurrentUser, scratch.uninstall())])
                .expect("the watcher");
        let stopper = watcher.stopper();
        let stopping = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            stopper.stop();
        });
        assert_eq!(
            watcher.wait(Some(Duration::from_secs(5))).expect("a wait"),
            WatchOutcome::Stopped
        );
        stopping.join().expect("the stopping thread");
        let mut empty = InstalledSoftwareWatcher::over(Vec::new()).expect("an empty watcher");
        assert_eq!(
            empty.wait(Some(Duration::from_millis(10))).expect("a wait"),
            WatchOutcome::NothingToWatch
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn the_keys_are_closed_when_the_scan_and_the_watch_end() {
        use std::sync::atomic::Ordering;
        let _serial = serial();
        let scratch = ScratchKey::new("closed");
        scratch.write("A", "Halvo Close Probe A");
        scratch.write("B", "Halvo Close Probe B");
        let before = OPEN_KEYS.load(Ordering::SeqCst);
        let scanned = scan_targets(
            &[(UninstallRoot::CurrentUser, scratch.uninstall())],
            Vec::new(),
        )
        .expect("the scan");
        assert_eq!(scanned.len(), 2);
        assert_eq!(
            OPEN_KEYS.load(Ordering::SeqCst),
            before,
            "the scan left keys open"
        );
        let watcher =
            InstalledSoftwareWatcher::over(vec![(UninstallRoot::CurrentUser, scratch.uninstall())])
                .expect("the watcher");
        assert_eq!(OPEN_KEYS.load(Ordering::SeqCst), before + 1);
        drop(watcher);
        assert_eq!(
            OPEN_KEYS.load(Ordering::SeqCst),
            before,
            "the watch left keys open"
        );
    }
}
