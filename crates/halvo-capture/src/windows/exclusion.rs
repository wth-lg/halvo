//! The exclusion inputs of the Windows sources (contracts.md C13.7): the
//! parent of a process, read from a process snapshot with a creation-time check
//! against reused ids, and the private-window input of a title.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use crate::floor::exclude::{Lineage, Parent, PrivateWindow, ProcessId};

/// The most processes whose parent the lineage keeps; a full cache is emptied.
pub(crate) const LINEAGE_CACHE_MAX: usize = 4096;

/// Where the lineage reads processes.
pub trait ProcessTable: Send + Sync {
    /// The creation time of the running process `pid`, in the 100 ns units of
    /// a FILETIME, or `None` when it is gone or cannot be opened.
    fn created(&self, pid: u32) -> Option<u64>;

    /// The parent id that a process snapshot holds for `pid`, or `None` when
    /// the snapshot does not hold it.
    fn parent(&self, pid: u32) -> Option<u32>;
}

/// The process table of the system.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemTable;

impl ProcessTable for SystemTable {
    fn created(&self, pid: u32) -> Option<u64> {
        use windows::Win32::Foundation::{CloseHandle, FILETIME};
        use windows::Win32::System::Threading::{
            GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        // SAFETY: OpenProcess takes plain values; the handle that it returns is
        // closed once, after the query.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
        let mut times = [FILETIME::default(); 4];
        let [created, exited, kernel, user] = &mut times;
        // SAFETY: the four pointers are live locals that the call writes, and
        // `handle` is open for the call.
        let read = unsafe { GetProcessTimes(handle, created, exited, kernel, user) };
        // SAFETY: `handle` is the handle that OpenProcess returned above.
        unsafe {
            let _ = CloseHandle(handle);
        }
        read.ok()?;
        Some((u64::from(times[0].dwHighDateTime) << 32) | u64::from(times[0].dwLowDateTime))
    }

    fn parent(&self, pid: u32) -> Option<u32> {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        };
        // SAFETY: CreateToolhelp32Snapshot takes plain values; the snapshot
        // handle that it returns is closed once, after the walk.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }.ok()?;
        let mut entry = PROCESSENTRY32W {
            dwSize: core::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut parent = None;
        // SAFETY: `entry` is a live PROCESSENTRY32W whose size field holds its
        // size, and `snapshot` is open for the walk.
        let mut more = unsafe { Process32FirstW(snapshot, &mut entry) }.is_ok();
        while more {
            if entry.th32ProcessID == pid {
                parent = Some(entry.th32ParentProcessID);
                break;
            }
            // SAFETY: as for Process32FirstW above.
            more = unsafe { Process32NextW(snapshot, &mut entry) }.is_ok();
        }
        // SAFETY: `snapshot` is the handle that CreateToolhelp32Snapshot
        // returned above.
        unsafe {
            let _ = CloseHandle(snapshot);
        }
        parent
    }
}

/// The parent of a process id for the floor's lineage walk. A parent whose
/// creation is later than its child's is a reused id: the real parent is gone
/// and the walk ends outside the tree. Each answer is kept for the process as
/// long as the process keeps its creation time.
pub struct ProcessLineage<T = SystemTable> {
    table: T,
    cache: Mutex<HashMap<u32, (u64, Parent)>>,
}

impl ProcessLineage<SystemTable> {
    /// The lineage over the system's process table.
    pub fn new() -> ProcessLineage<SystemTable> {
        ProcessLineage::over(SystemTable)
    }
}

impl Default for ProcessLineage<SystemTable> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: ProcessTable> ProcessLineage<T> {
    pub(crate) fn over(table: T) -> ProcessLineage<T> {
        ProcessLineage {
            table,
            cache: Mutex::new(HashMap::new()),
        }
    }

    fn read(&self, pid: u32, created: u64) -> Parent {
        match self.table.parent(pid) {
            None => Parent::Unknown,
            Some(parent) if parent == 0 || parent == pid => Parent::None,
            Some(parent) => match self.table.created(parent) {
                Some(parent_created) if parent_created <= created => {
                    Parent::Of(ProcessId::new(parent))
                }
                Some(_) | None => Parent::None,
            },
        }
    }
}

impl<T: ProcessTable> Lineage for ProcessLineage<T> {
    fn parent_of(&self, pid: ProcessId) -> Parent {
        let pid = pid.get();
        let Some(created) = self.table.created(pid) else {
            return Parent::Unknown;
        };
        let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((at, parent)) = cache.get(&pid) {
            if *at == created {
                return *parent;
            }
        }
        let parent = self.read(pid, created);
        if parent != Parent::Unknown {
            if cache.len() >= LINEAGE_CACHE_MAX {
                cache.clear();
            }
            cache.insert(pid, (created, parent));
        }
        parent
    }
}

/// The browsers whose private windows a title source tells apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Browser {
    Edge,
    Firefox,
    Chrome,
    Other,
}

impl Browser {
    /// The browser of an executable path, by its file name, ignoring ASCII case.
    pub(crate) fn of(path: Option<&str>) -> Browser {
        let Some(path) = path else {
            return Browser::Other;
        };
        let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
        [
            ("msedge.exe", Browser::Edge),
            ("firefox.exe", Browser::Firefox),
            ("chrome.exe", Browser::Chrome),
        ]
        .into_iter()
        .find(|(file, _)| name.eq_ignore_ascii_case(file))
        .map_or(Browser::Other, |(_, browser)| browser)
    }
}

const EDGE_PRIVATE_ENDING: &str = "[InPrivate] - Microsoft Edge";

const FIREFOX_PRIVATE_ENDING: &str = "Mozilla Firefox Private Browsing";

/// The private-window input of a title of `browser` (C13.7 item 4). Edge and
/// Firefox are told by the ending of the title that the source has just read,
/// and never by a word alone; a Chrome window is private unless `query` tells
/// it as not private, so a query with no answer drops its title; every other
/// window is Unknown, which drops nothing.
pub(crate) fn private_window(
    browser: Browser,
    title: &str,
    query: impl FnOnce() -> Option<bool>,
) -> PrivateWindow {
    let private = match browser {
        Browser::Edge => title
            .replace('\u{200B}', "")
            .trim_end()
            .ends_with(EDGE_PRIVATE_ENDING),
        Browser::Firefox => title.trim_end().ends_with(FIREFOX_PRIVATE_ENDING),
        Browser::Chrome => query() != Some(false),
        Browser::Other => false,
    };
    if private {
        PrivateWindow::Private
    } else {
        PrivateWindow::Unknown
    }
}

/// What the UI Automation query tells of a Chrome window: `Some(true)` when a
/// button of the window is named Incognito, `Some(false)` when the window holds
/// buttons and none is, and `None` when the query gives no answer. The calling
/// thread must be in an apartment of COM.
#[cfg(target_os = "windows")]
pub(crate) fn chrome_window_is_incognito(
    automation: &windows::Win32::UI::Accessibility::IUIAutomation,
    window: windows::Win32::Foundation::HWND,
) -> Option<bool> {
    use windows::Win32::System::Variant::{VARIANT, VT_I4};
    use windows::Win32::UI::Accessibility::{
        TreeScope_Descendants, UIA_ButtonControlTypeId, UIA_ControlTypePropertyId,
    };
    let mut button = VARIANT::default();
    // SAFETY: the value holds nothing; the tag and the member that it names
    // are written together, and an integer owns nothing to free.
    unsafe {
        (*button.Anonymous.Anonymous).vt = VT_I4;
        (*button.Anonymous.Anonymous).Anonymous.lVal = UIA_ButtonControlTypeId.0;
    }
    // SAFETY: the calls take the live client, a window handle value and the
    // condition that they made; each returns an owned COM pointer or an error.
    unsafe {
        let element = automation.ElementFromHandle(window).ok()?;
        let condition = automation
            .CreatePropertyCondition(UIA_ControlTypePropertyId, &button)
            .ok()?;
        let buttons = element.FindAll(TreeScope_Descendants, &condition).ok()?;
        let count = buttons.Length().ok()?;
        if count == 0 {
            return None;
        }
        for index in 0..count {
            let name = buttons.GetElement(index).ok()?.CurrentName().ok()?;
            if name.to_string().to_ascii_lowercase().contains("incognito") {
                return Some(true);
            }
        }
        Some(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::AppIdentity;
    use crate::floor::exclude::{DropReason, FloorHandle, Subject, Verdict};
    use crate::floor::kind;
    use crate::floor::Switches;
    use crate::secure_input::windows::PasswordFieldProbe;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[derive(Default)]
    struct Table {
        processes: HashMap<u32, (u32, u64)>,
        snapshots: AtomicUsize,
    }

    impl Table {
        fn with(mut self, pid: u32, parent: u32, created: u64) -> Table {
            self.processes.insert(pid, (parent, created));
            self
        }
    }

    impl ProcessTable for Table {
        fn created(&self, pid: u32) -> Option<u64> {
            self.processes.get(&pid).map(|(_, created)| *created)
        }

        fn parent(&self, pid: u32) -> Option<u32> {
            self.snapshots.fetch_add(1, Ordering::SeqCst);
            self.processes.get(&pid).map(|(parent, _)| *parent)
        }
    }

    const ROOT: u32 = 100;
    const CHILD: u32 = 200;
    const GRANDCHILD: u32 = 300;
    const UNRELATED: u32 = 400;
    const REUSED: u32 = 500;
    const LOOP_A: u32 = 600;
    const LOOP_B: u32 = 700;

    fn table() -> Table {
        Table::default()
            .with(4, 0, 1)
            .with(ROOT, 4, 10)
            .with(CHILD, ROOT, 20)
            .with(GRANDCHILD, CHILD, 30)
            .with(UNRELATED, 4, 15)
            .with(REUSED, ROOT, 5)
            .with(LOOP_A, LOOP_B, 40)
            .with(LOOP_B, LOOP_A, 40)
    }

    fn decide(lineage: Arc<dyn Lineage>, pid: u32) -> Option<DropReason> {
        let floor = FloorHandle::new(Some(ProcessId::new(ROOT)), lineage, Switches::default());
        let app = AppIdentity::executable_path("C:\\Apps\\tool.exe".to_string());
        let subject = Subject::new(app, ProcessId::new(pid));
        let probe = PasswordFieldProbe::default();
        match floor.decide::<kind::Pointer>(subject, PrivateWindow::Unknown, &probe) {
            Verdict::Keep(_) => None,
            Verdict::Drop(reason) => Some(reason),
        }
    }

    #[test]
    fn the_lineage_over_a_process_table_places_each_process_in_or_out_of_the_tree() {
        let lineage: Arc<dyn Lineage> = Arc::new(ProcessLineage::over(table()));
        let tree = Some(DropReason::ExclusionTree);
        for (pid, expected) in [
            (ROOT, tree),
            (CHILD, tree),
            (GRANDCHILD, tree),
            (UNRELATED, None),
            (REUSED, None),
            (LOOP_A, Some(DropReason::UnknownLineage)),
            (999, Some(DropReason::UnknownLineage)),
        ] {
            assert_eq!(decide(Arc::clone(&lineage), pid), expected, "pid {pid}");
        }
        assert_eq!(
            lineage.parent_of(ProcessId::new(GRANDCHILD)),
            Parent::Of(ProcessId::new(CHILD))
        );
        assert_eq!(lineage.parent_of(ProcessId::new(REUSED)), Parent::None);
        assert_eq!(lineage.parent_of(ProcessId::new(4)), Parent::None);
        assert_eq!(lineage.parent_of(ProcessId::new(999)), Parent::Unknown);
    }

    #[test]
    fn the_lineage_keeps_a_parent_while_the_process_keeps_its_creation_time() {
        let lineage = ProcessLineage::over(table());
        for _ in 0..3 {
            assert_eq!(
                lineage.parent_of(ProcessId::new(CHILD)),
                Parent::Of(ProcessId::new(ROOT))
            );
        }
        assert_eq!(lineage.table.snapshots.load(Ordering::SeqCst), 1);
        let mut moved = table();
        moved.processes.insert(CHILD, (UNRELATED, 50));
        let lineage = ProcessLineage {
            table: moved,
            cache: Mutex::new(HashMap::from([(
                CHILD,
                (20, Parent::Of(ProcessId::new(ROOT))),
            )])),
        };
        assert_eq!(
            lineage.parent_of(ProcessId::new(CHILD)),
            Parent::Of(ProcessId::new(UNRELATED)),
            "a reused id kept the parent of the process that had it before"
        );
    }

    #[test]
    fn the_system_lineage_reads_this_process_under_its_parent() {
        let lineage = ProcessLineage::new();
        let own = std::process::id();
        let Parent::Of(parent) = lineage.parent_of(ProcessId::new(own)) else {
            panic!("the parent of this test process could not be read");
        };
        let floor = FloorHandle::new(Some(parent), Arc::new(lineage), Switches::default());
        let app = AppIdentity::executable_path("C:\\Apps\\tool.exe".to_string());
        let probe = PasswordFieldProbe::default();
        let verdict = floor.decide::<kind::ForegroundApp>(
            Subject::new(app, ProcessId::new(own)),
            PrivateWindow::Unknown,
            &probe,
        );
        assert!(
            matches!(verdict, Verdict::Drop(DropReason::ExclusionTree)),
            "{verdict:?}"
        );
    }

    #[test]
    fn a_title_of_a_private_window_is_told_by_its_ending_alone() {
        let never = || -> Option<bool> { panic!("the query ran for a window that is not Chrome") };
        let private = PrivateWindow::Private;
        let unknown = PrivateWindow::Unknown;
        for (browser, title, expected) in [
            (
                Browser::Edge,
                "New tab - [InPrivate] - Microsoft\u{200B} Edge",
                private,
            ),
            (
                Browser::Edge,
                "New tab - [InPrivate] - Microsoft Edge",
                private,
            ),
            (
                Browser::Edge,
                "Private notes - Microsoft\u{200B} Edge",
                unknown,
            ),
            (
                Browser::Edge,
                "[InPrivate] browsing tips - Microsoft\u{200B} Edge",
                unknown,
            ),
            (
                Browser::Firefox,
                "Inbox \u{2014} Mozilla Firefox Private Browsing",
                private,
            ),
            (
                Browser::Firefox,
                "Private Browsing - Mozilla Firefox",
                unknown,
            ),
            (
                Browser::Other,
                "Notes - Mozilla Firefox Private Browsing",
                unknown,
            ),
            (Browser::Other, "x - [InPrivate] - Microsoft Edge", unknown),
        ] {
            assert_eq!(private_window(browser, title, never), expected, "{title}");
        }
        assert_eq!(
            Browser::of(Some("C:\\Program Files\\Edge\\MSEDGE.EXE")),
            Browser::Edge
        );
        assert_eq!(
            Browser::of(Some("C:\\Firefox\\firefox.exe")),
            Browser::Firefox
        );
        assert_eq!(Browser::of(Some("C:\\Chrome\\chrome.exe")), Browser::Chrome);
        assert_eq!(
            Browser::of(Some("C:\\Chrome\\chrome.exe.bak")),
            Browser::Other
        );
        assert_eq!(Browser::of(None), Browser::Other);
    }

    #[test]
    fn a_chrome_window_is_kept_only_when_the_query_tells_it_as_not_private() {
        let title = "Inbox - Google Chrome";
        for (answer, expected) in [
            (Some(false), PrivateWindow::Unknown),
            (Some(true), PrivateWindow::Private),
            (None, PrivateWindow::Private),
        ] {
            assert_eq!(
                private_window(Browser::Chrome, title, || answer),
                expected,
                "{answer:?}"
            );
        }
    }
}
