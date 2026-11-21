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

    /// What a process snapshot holds of `pid`.
    fn listing(&self, pid: u32) -> Listing;
}

/// What a process snapshot holds of one process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Listing {
    /// The process runs, with this parent id.
    Parent(u32),
    /// The snapshot does not list the process: it has ended, though a handle
    /// that another process holds may still open it.
    NotListed,
    /// No snapshot could be taken.
    NoSnapshot,
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

    fn listing(&self, pid: u32) -> Listing {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        };
        // SAFETY: CreateToolhelp32Snapshot takes plain values; the snapshot
        // handle that it returns is closed once, after the walk.
        let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
            return Listing::NoSnapshot;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: core::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut listing = Listing::NotListed;
        // SAFETY: `entry` is a live PROCESSENTRY32W whose size field holds its
        // size, and `snapshot` is open for the walk.
        let mut more = unsafe { Process32FirstW(snapshot, &mut entry) }.is_ok();
        while more {
            if entry.th32ProcessID == pid {
                listing = Listing::Parent(entry.th32ParentProcessID);
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
        listing
    }
}

/// The parent of a process id for the floor's lineage walk. A parent whose
/// creation is later than its child's is a reused id: the real parent is gone
/// and the walk ends outside the tree. A process that runs and cannot be opened
/// ends the walk outside the tree too, since a process under the root runs as
/// the root's user and opens; a process that has ended, which a handle may
/// still open, ends it outside the tree as well, and a process that is gone, or
/// a snapshot that cannot be taken, gives an unknown parent.
/// Each answer is kept for the process as long as the process keeps its
/// creation time and a parent that it names keeps its own.
pub struct ProcessLineage<T = SystemTable> {
    table: T,
    cache: Mutex<HashMap<u32, CachedParent>>,
}

/// A kept answer: the process's creation time, its parent, and the parent's
/// creation time when it names one.
type CachedParent = (u64, Parent, Option<u64>);

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
        match self.table.listing(pid) {
            Listing::NoSnapshot => Parent::Unknown,
            Listing::NotListed => Parent::None,
            Listing::Parent(parent) if parent == 0 || parent == pid => Parent::None,
            Listing::Parent(parent) => match self.table.created(parent) {
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
            return match self.table.listing(pid) {
                Listing::Parent(_) => Parent::None,
                Listing::NotListed | Listing::NoSnapshot => Parent::Unknown,
            };
        };
        let created_of = |parent: Parent| match parent {
            Parent::Of(parent) => self.table.created(parent.get()),
            Parent::None | Parent::Unknown => None,
        };
        let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((at, parent, parent_created)) = cache.get(&pid) {
            if *at == created && created_of(*parent) == *parent_created {
                return *parent;
            }
        }
        let parent = self.read(pid, created);
        if parent != Parent::Unknown {
            if cache.len() >= LINEAGE_CACHE_MAX {
                cache.clear();
            }
            cache.insert(pid, (created, parent, created_of(parent)));
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

/// The bare brand name that ends the window text of a normal Edge window.
const EDGE_BRAND: &str = "Microsoft Edge";

/// Whether `c` counts as nothing in an Edge window text: a zero-width
/// character or a mark of writing direction.
fn is_invisible_mark(c: char) -> bool {
    matches!(
        c,
        '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2069}' | '\u{FEFF}'
    )
}

/// Whether an Edge window text is that of a normal window: it ends with the
/// bare brand name, and the last letter, digit or closing bracket before the
/// brand is not a closing bracket. So a text is private where nothing but
/// characters that are neither letters nor digits (a hyphen, an en dash, a
/// middle dot, a blank of any kind) stands between a closing bracket, where
/// the InPrivate marker ends, and the brand. Zero-width characters and marks
/// of writing direction count as nothing anywhere in the text, so the match
/// of the brand tolerates them inside, before and after the brand, and
/// blanks after it; any other character after the brand fails the match.
fn edge_text_is_normal(text: &str) -> bool {
    let text: String = text.chars().filter(|&c| !is_invisible_mark(c)).collect();
    let Some(head) = text.trim_end().strip_suffix(EDGE_BRAND) else {
        return false;
    };
    let last = head
        .chars()
        .rev()
        .find(|&c| c.is_alphanumeric() || matches!(c, ']' | '\u{FF3D}'));
    !matches!(last, Some(']' | '\u{FF3D}'))
}

/// The private-window input of a title of `browser` (C13.7 item 4). Edge
/// fails closed: a title is kept only where the window text ends with the
/// bare brand name with no bracketed marker before it, and every other text,
/// the private marker of any interface language among them, is told Private;
/// never by a word alone. Every title of Firefox is told Private, so no
/// Firefox title is kept: under some interface languages the title of a
/// private window ends with the brand name as a normal one does, and no rule
/// on the text tells the two apart. A Chrome window is private unless `query`
/// tells it as not private, so a query with no answer drops its title; every
/// other window is Unknown, which drops nothing.
pub(crate) fn private_window(
    browser: Browser,
    title: &str,
    query: impl FnOnce() -> Option<bool>,
) -> PrivateWindow {
    let private = match browser {
        Browser::Edge => !edge_text_is_normal(title),
        Browser::Firefox => true,
        Browser::Chrome => query() != Some(false),
        Browser::Other => false,
    };
    if private {
        PrivateWindow::Private
    } else {
        PrivateWindow::Unknown
    }
}

/// The English names of Chrome's toolbar buttons by which the query knows that
/// a window's interface is English.
const ENGLISH_TOOLBAR_NAMES: [&str; 3] = ["Back", "Forward", "Reload"];

/// What the names of a Chrome window's buttons tell of it: `Some(true)` when
/// one holds the word Incognito, `Some(false)` when the window is known to
/// have an English interface (a button named Back, Forward or Reload) and none
/// holds that word, and `None` for every other window, a window in another
/// language among them, so that its title is dropped.
pub(crate) fn incognito_of_names<S: AsRef<str>>(names: &[S]) -> Option<bool> {
    if names
        .iter()
        .any(|name| name.as_ref().to_ascii_lowercase().contains("incognito"))
    {
        return Some(true);
    }
    let english = names.iter().any(|name| {
        ENGLISH_TOOLBAR_NAMES
            .iter()
            .any(|english| name.as_ref().trim().eq_ignore_ascii_case(english))
    });
    english.then_some(false)
}

/// What the names of a Chrome window's buttons tell of it, given the names of
/// its toolbar's own buttons and of every button below the window: `Some(true)`
/// when any button holds the word Incognito, and otherwise what the toolbar's
/// names tell by [`incognito_of_names`], so that a button named Back, Forward
/// or Reload outside the toolbar (a bookmark, an extension, the page) makes no
/// window English.
pub(crate) fn incognito_of_buttons<S: AsRef<str>>(toolbar: &[S], all: &[S]) -> Option<bool> {
    if all
        .iter()
        .any(|name| name.as_ref().to_ascii_lowercase().contains("incognito"))
    {
        return Some(true);
    }
    incognito_of_names(toolbar)
}

/// What the UI Automation query tells of a Chrome window, by the names of its
/// buttons ([`incognito_of_buttons`]): the English names count only among the
/// own buttons of the first element below the window whose control type is
/// ToolBar, and a window with no such element is told by the word Incognito
/// alone; `None` when the query gives no answer.
/// The calling thread must be in an apartment of COM.
#[cfg(target_os = "windows")]
pub(crate) fn chrome_window_is_incognito(
    automation: &windows::Win32::UI::Accessibility::IUIAutomation,
    window: windows::Win32::Foundation::HWND,
) -> Option<bool> {
    use windows::Win32::System::Variant::{VARIANT, VT_I4};
    use windows::Win32::UI::Accessibility::{
        IUIAutomationElementArray, TreeScope_Children, TreeScope_Descendants,
        UIA_ButtonControlTypeId, UIA_ControlTypePropertyId, UIA_ToolBarControlTypeId,
    };
    let of_type = |control: i32| {
        let mut value = VARIANT::default();
        // SAFETY: the value holds nothing; the tag and the member that it
        // names are written together, and an integer owns nothing to free.
        unsafe {
            (*value.Anonymous.Anonymous).vt = VT_I4;
            (*value.Anonymous.Anonymous).Anonymous.lVal = control;
        }
        value
    };
    let names = |found: &IUIAutomationElementArray| -> Option<Vec<String>> {
        // SAFETY: the calls read a live array of elements and their names.
        unsafe {
            let count = found.Length().ok()?;
            let mut names = Vec::new();
            for index in 0..count {
                names.push(
                    found
                        .GetElement(index)
                        .ok()?
                        .CurrentName()
                        .ok()?
                        .to_string(),
                );
            }
            Some(names)
        }
    };
    // SAFETY: the calls take the live client, a window handle value and the
    // conditions that they made; each returns an owned COM pointer or an error.
    unsafe {
        let element = automation.ElementFromHandle(window).ok()?;
        let button = automation
            .CreatePropertyCondition(
                UIA_ControlTypePropertyId,
                &of_type(UIA_ButtonControlTypeId.0),
            )
            .ok()?;
        let toolbar = automation
            .CreatePropertyCondition(
                UIA_ControlTypePropertyId,
                &of_type(UIA_ToolBarControlTypeId.0),
            )
            .ok()?;
        let all = names(&element.FindAll(TreeScope_Descendants, &button).ok()?)?;
        let toolbar_names = match element.FindFirst(TreeScope_Descendants, &toolbar) {
            Ok(toolbar) => names(&toolbar.FindAll(TreeScope_Children, &button).ok()?)?,
            Err(_) => Vec::new(),
        };
        incognito_of_buttons(&toolbar_names, &all)
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
        unopened: Vec<u32>,
        ended: Vec<u32>,
        snapshots: AtomicUsize,
    }

    impl Table {
        fn with(mut self, pid: u32, parent: u32, created: u64) -> Table {
            self.processes.insert(pid, (parent, created));
            self
        }

        fn unopen(mut self, pid: u32) -> Table {
            self.unopened.push(pid);
            self
        }

        fn end(mut self, pid: u32) -> Table {
            self.ended.push(pid);
            self
        }
    }

    impl ProcessTable for Table {
        fn created(&self, pid: u32) -> Option<u64> {
            if self.unopened.contains(&pid) {
                return None;
            }
            self.processes.get(&pid).map(|(_, created)| *created)
        }

        fn listing(&self, pid: u32) -> Listing {
            self.snapshots.fetch_add(1, Ordering::SeqCst);
            if self.ended.contains(&pid) {
                return Listing::NotListed;
            }
            self.processes
                .get(&pid)
                .map_or(Listing::NotListed, |(parent, _)| Listing::Parent(*parent))
        }
    }

    const ROOT: u32 = 100;
    const CHILD: u32 = 200;
    const GRANDCHILD: u32 = 300;
    const UNRELATED: u32 = 400;
    const REUSED: u32 = 500;
    const LOOP_A: u32 = 600;
    const LOOP_B: u32 = 700;
    const UNOPENED: u32 = 800;
    const UNDER_UNOPENED: u32 = 900;
    const ENDED: u32 = 1000;
    const UNDER_ENDED: u32 = 1100;

    /// Where the walk up from `pid` ends: in the tree under ROOT, outside it,
    /// at an unknown parent, or in a loop.
    fn walk_end(lineage: &impl Lineage, pid: u32) -> &'static str {
        let mut at = pid;
        for _ in 0..16 {
            match lineage.parent_of(ProcessId::new(at)) {
                Parent::Of(parent) if parent.get() == ROOT => return "in_tree",
                Parent::Of(parent) => at = parent.get(),
                Parent::None => return "outside",
                Parent::Unknown => return "unknown",
            }
        }
        "loop"
    }

    #[test]
    fn a_cached_parent_reused_by_a_child_of_the_root() {
        const APP: u32 = 2_000;
        const PARENT: u32 = 2_100;
        let mut lineage = ProcessLineage::over(
            Table::default()
                .with(4, 0, 1)
                .with(ROOT, 4, 10)
                .with(PARENT, 4, 40)
                .with(APP, PARENT, 50),
        );
        let before = walk_end(&lineage, APP);
        lineage.table.processes.insert(PARENT, (ROOT, 60));
        let after_reuse_with_cache = walk_end(&lineage, APP);
        let fresh = ProcessLineage::over(
            Table::default()
                .with(4, 0, 1)
                .with(ROOT, 4, 10)
                .with(PARENT, ROOT, 60)
                .with(APP, PARENT, 50),
        );
        let after_reuse_without_cache = walk_end(&fresh, APP);
        assert!(
            before == "outside" && after_reuse_with_cache == after_reuse_without_cache,
            "before={before} after_reuse_with_cache={after_reuse_with_cache} \
             after_reuse_without_cache={after_reuse_without_cache}"
        );
    }

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
            .with(UNOPENED, 4, 2)
            .with(UNDER_UNOPENED, UNOPENED, 60)
            .unopen(UNOPENED)
            .with(ENDED, ROOT, 25)
            .with(UNDER_ENDED, ENDED, 35)
            .end(ENDED)
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
            (UNOPENED, None),
            (UNDER_UNOPENED, None),
            (UNDER_ENDED, None),
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
                (20, Parent::Of(ProcessId::new(ROOT)), Some(10)),
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
    fn a_chrome_button_outside_the_toolbar_does_not_tell_the_window_not_private() {
        let toolbar = ["Zur\u{fc}ck", "Vorw\u{e4}rts", "Neu laden"];
        let all = ["Zur\u{fc}ck", "Vorw\u{e4}rts", "Neu laden", "Back"];
        let english = ["Back", "Forward", "Reload"];
        let cases = [
            (
                "german-toolbar-and-a-button-named-back-elsewhere",
                incognito_of_buttons(&toolbar, &all),
                None,
            ),
            (
                "english-toolbar",
                incognito_of_buttons(&english, &english),
                Some(false),
            ),
        ];
        let wrong: Vec<&str> = cases
            .iter()
            .filter(|(_, told, wanted)| told != wanted)
            .map(|(case, ..)| *case)
            .collect();
        assert!(
            wrong.is_empty(),
            "told against the window's toolbar: {wrong:?}"
        );
    }

    #[test]
    fn no_firefox_title_is_kept_normal_or_private_in_any_language() {
        let never = || -> Option<bool> { panic!("the query ran for a window that is not Chrome") };
        let cases = [
            ("english-normal", "PAGE \u{2014} Mozilla Firefox"),
            (
                "english-private",
                "PAGE \u{2014} Mozilla Firefox Private Browsing",
            ),
            ("french-normal", "Accueil \u{2014} Mozilla Firefox"),
            (
                "french-private",
                "PAGE \u{2014} Navigation priv\u{E9}e de Mozilla Firefox",
            ),
        ];
        let kept: Vec<&str> = cases
            .iter()
            .filter(|(_, title)| {
                private_window(Browser::Firefox, title, never) != PrivateWindow::Private
            })
            .map(|(case, _)| *case)
            .collect();
        assert!(
            kept.is_empty(),
            "Firefox titles told as not private: {kept:?}"
        );
    }

    #[test]
    fn an_edge_title_is_kept_only_with_the_bare_brand_ending() {
        let told = |browser, title: &str| private_window(browser, title, || None);
        let cases = [
            (
                "edge-bracketed-marker",
                told(
                    Browser::Edge,
                    "Seite - [InPrivate-Modus] - Microsoft\u{200B} Edge",
                ),
                PrivateWindow::Private,
            ),
            (
                "edge-plain",
                told(Browser::Edge, "Seite - Microsoft\u{200B} Edge"),
                PrivateWindow::Unknown,
            ),
            (
                "edge-german-inprivate-as-printed",
                told(
                    Browser::Edge,
                    "Neuer InPrivate-Tab - [InPrivate] \u{2013} Microsoft\u{200B} Edge",
                ),
                PrivateWindow::Private,
            ),
            (
                "edge-japanese-inprivate-as-printed",
                told(
                    Browser::Edge,
                    "\u{65B0}\u{3057}\u{3044} InPrivate \u{30BF}\u{30D6} - [InPrivate] - Microsoft\u{200B} Edge",
                ),
                PrivateWindow::Private,
            ),
            (
                "edge-middle-dot-separator",
                told(
                    Browser::Edge,
                    "Seite - [InPrivate] \u{00B7} Microsoft\u{200B} Edge",
                ),
                PrivateWindow::Private,
            ),
            (
                "edge-right-to-left-mark-after-the-bracket",
                told(
                    Browser::Edge,
                    "Seite - [InPrivate]\u{200F} - Microsoft\u{200B} Edge",
                ),
                PrivateWindow::Private,
            ),
        ];
        let wrong: Vec<&str> = cases
            .iter()
            .filter(|(_, told, wanted)| told != wanted)
            .map(|(case, ..)| *case)
            .collect();
        assert!(
            wrong.is_empty(),
            "told against the window's ending: {wrong:?}"
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
