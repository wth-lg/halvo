//! The exclusion decision (contracts.md C13.7): for a kind and its subject,
//! keep or drop with a reason. Every floor constructor takes the keep verdict
//! of its kind, so the decision runs at the source, before capture, and no value
//! exists for an excluded subject.

use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;

use super::kind::Recorded;
use super::{switch_of, Switch, Switches};
use crate::event::{AppIdentity, FloorEvent, Modality};
use crate::secure_input::{SecureInputProbe, SecureInputReading};

/// The most parent steps of one lineage walk; a longer walk is an unknown
/// lineage.
pub const LINEAGE_WALK_MAX_STEPS: usize = 64;

/// Password managers and Halvo's own app, by bundle id (macOS). Only
/// `com.apple.Passwords` and `com.apple.keychainaccess` were read from a bundle;
/// the other entries are a starter table that the owner approves.
pub const LISTED_BUNDLE_IDS: &[&str] = &[
    "com.halvo.desktop",
    "com.apple.Passwords",
    "com.apple.keychainaccess",
    "com.1password.1password",
    "com.agilebits.onepassword7",
    "com.bitwarden.desktop",
    "org.keepassxc.keepassxc",
    "com.lastpass.LastPass",
    "com.dashlane.dashlanephonefinal",
];

/// The same list by executable name (Windows), matched on the last part of the
/// path and ignoring ASCII case. A starter table that the owner approves.
pub const LISTED_EXECUTABLES: &[&str] = &[
    "Halvo.exe",
    "1Password.exe",
    "Bitwarden.exe",
    "KeePassXC.exe",
    "KeePass.exe",
    "LastPass.exe",
    "Dashlane.exe",
];

/// A process id. It travels in memory only and is never serialized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProcessId(u32);

impl ProcessId {
    pub const fn new(raw: u32) -> ProcessId {
        ProcessId(raw)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

/// What a lineage probe knows of the parent of one process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Parent {
    Of(ProcessId),
    /// The process has no parent: the walk ends outside the tree.
    None,
    /// The process is gone or its parent could not be read.
    Unknown,
}

/// The parent of a process id, read from the process table of the system; the
/// macOS and the Windows source units implement it.
pub trait Lineage: Send + Sync {
    fn parent_of(&self, pid: ProcessId) -> Parent;
}

/// A lineage that knows no process. With no exclusion root nothing asks it.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoLineage;

impl Lineage for NoLineage {
    fn parent_of(&self, _pid: ProcessId) -> Parent {
        Parent::Unknown
    }
}

/// The subject of an event: the app and the process it belongs to.
pub struct Subject {
    app: AppIdentity,
    pid: ProcessId,
}

impl Subject {
    pub fn new(app: AppIdentity, pid: ProcessId) -> Subject {
        Subject { app, pid }
    }

    pub(crate) fn into_parts(self) -> (AppIdentity, ProcessId) {
        (self.app, self.pid)
    }
}

impl fmt::Debug for Subject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Subject(..)")
    }
}

/// Whether the source told the window as private (contracts.md C13.7 item 4).
/// `Unknown` never drops a title.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateWindow {
    Private,
    Unknown,
}

/// Why the decision dropped an event; the source counts it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DropReason {
    /// The switch of the kind is off.
    SwitchOff,
    /// The app in front is on the static list.
    ListedApp,
    /// The process is the exclusion root or under it.
    ExclusionTree,
    /// The lineage walk could not end: a parent unknown, a loop, or too deep.
    UnknownLineage,
    /// A title of a window that the source told as private.
    PrivateWindow,
    /// A title or a frame while the secure-input reading is active.
    SecureInput,
}

/// The keep verdict for one event of kind `K`: the one input of a floor
/// constructor of that kind that only the decision makes. It carries the
/// subject that it was given for and is not `Clone`. It holds for the instant
/// of the decision: a held verdict outlives a change of the reading or of the
/// rules, so the caller decides right before each build.
pub struct Keep<K> {
    subject: Subject,
    site_read: bool,
    kind: PhantomData<K>,
}

impl<K: Recorded> Keep<K> {
    fn issue(subject: Subject, site_read: bool) -> Keep<K> {
        Keep {
            subject,
            site_read,
            kind: PhantomData,
        }
    }

    pub(crate) fn into_parts(self) -> (Subject, bool) {
        (self.subject, self.site_read)
    }
}

impl<K: Recorded> fmt::Debug for Keep<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Keep({})", K::KIND.as_str())
    }
}

/// The answer of the decision for one event of kind `K`.
#[derive(Debug)]
pub enum Verdict<K: Recorded> {
    Keep(Keep<K>),
    Drop(DropReason),
}

/// What a source needs for the decision: the exclusion root, the lineage
/// probe, the static list and the switches. A source gets it through its
/// context value.
#[derive(Clone)]
pub struct FloorHandle {
    root: Option<ProcessId>,
    lineage: Arc<dyn Lineage>,
    switches: Switches,
}

impl Default for FloorHandle {
    /// No root, so the tree rule matches nothing; the default switches.
    fn default() -> FloorHandle {
        FloorHandle::new(None, Arc::new(NoLineage), Switches::default())
    }
}

impl fmt::Debug for FloorHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FloorHandle")
            .field("root", &self.root)
            .field("switches", &self.switches)
            .finish_non_exhaustive()
    }
}

impl FloorHandle {
    /// `root` is the process that started the daemon with `--lifeline`, or
    /// `None` in a run without it.
    pub fn new(
        root: Option<ProcessId>,
        lineage: Arc<dyn Lineage>,
        switches: Switches,
    ) -> FloorHandle {
        FloorHandle {
            root,
            lineage,
            switches,
        }
    }

    pub fn switches(&self) -> Switches {
        self.switches
    }

    /// The owner's second line over a received value: the rules that need no
    /// live state (the switches, the static list and the tree rule over the
    /// stored subject pid). Here an unknown lineage keeps the event, because
    /// the source decided on the live process table.
    pub fn recheck(&self, event: &FloorEvent) -> Result<(), DropReason> {
        if switch_of(event.kind()).is_some_and(|switch| !self.switches.is_on(switch)) {
            return Err(DropReason::SwitchOff);
        }
        if is_listed(event.app()) {
            return Err(DropReason::ListedApp);
        }
        match self
            .root
            .map(|root| walk(root, event.pid(), &*self.lineage))
        {
            Some(Tree::In) => Err(DropReason::ExclusionTree),
            Some(Tree::Out | Tree::Unknown) | None => Ok(()),
        }
    }

    /// The decision for one event of kind `K` about `subject`. `window` counts
    /// for a window_title event only. `probe` is read for a window_title or a
    /// screen_frame event only: an active reading drops it, and an unknown
    /// reading keeps it (contracts.md C13.8). The verdict holds for the instant
    /// of this call: the caller decides, and checks a key's clearance, right
    /// before each build, because a held verdict or clearance outlives a change
    /// of the reading.
    pub fn decide<K: Recorded>(
        &self,
        subject: Subject,
        window: PrivateWindow,
        probe: &dyn SecureInputProbe,
    ) -> Verdict<K> {
        let rules = Rules {
            root: self.root,
            lineage: &*self.lineage,
            switches: self.switches,
        };
        let reading = || probe.read();
        match decide(&rules, K::KIND, &subject.app, subject.pid, window, reading) {
            Ok(()) => Verdict::Keep(Keep::issue(subject, self.switches.is_on(Switch::SiteRead))),
            Err(reason) => Verdict::Drop(reason),
        }
    }
}

struct Rules<'a> {
    root: Option<ProcessId>,
    lineage: &'a dyn Lineage,
    switches: Switches,
}

/// The one decision, over its arguments and the lineage that it reads.
fn decide(
    rules: &Rules<'_>,
    kind: Modality,
    app: &AppIdentity,
    pid: ProcessId,
    window: PrivateWindow,
    reading: impl FnOnce() -> SecureInputReading,
) -> Result<(), DropReason> {
    if switch_of(kind).is_some_and(|switch| !rules.switches.is_on(switch)) {
        return Err(DropReason::SwitchOff);
    }
    if is_listed(app) {
        return Err(DropReason::ListedApp);
    }
    match rules.root.map(|root| walk(root, pid, rules.lineage)) {
        Some(Tree::In) => return Err(DropReason::ExclusionTree),
        Some(Tree::Unknown) => return Err(DropReason::UnknownLineage),
        Some(Tree::Out) | None => {}
    }
    if kind == Modality::WindowTitle && window == PrivateWindow::Private {
        return Err(DropReason::PrivateWindow);
    }
    let content = matches!(kind, Modality::WindowTitle | Modality::ScreenFrame);
    if content && reading() == SecureInputReading::Active {
        return Err(DropReason::SecureInput);
    }
    Ok(())
}

fn is_listed(app: &AppIdentity) -> bool {
    app.is_listed(LISTED_BUNDLE_IDS, LISTED_EXECUTABLES)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tree {
    In,
    Out,
    Unknown,
}

fn walk(root: ProcessId, pid: ProcessId, lineage: &dyn Lineage) -> Tree {
    let mut seen = [ProcessId(0); LINEAGE_WALK_MAX_STEPS];
    let mut current = pid;
    for step in 0..LINEAGE_WALK_MAX_STEPS {
        if current == root {
            return Tree::In;
        }
        if seen[..step].contains(&current) {
            return Tree::Unknown;
        }
        seen[step] = current;
        current = match lineage.parent_of(current) {
            Parent::Of(parent) => parent,
            Parent::None => return Tree::Out,
            Parent::Unknown => return Tree::Unknown,
        };
    }
    Tree::Unknown
}

/// A keep verdict with no live reading, for the prefill helper alone
/// (crates/halvo-capture/src/daemon/prefill.rs), which feeds synthetic values
/// of every kind into the sink. A build without debug assertions has no such
/// function.
#[cfg(debug_assertions)]
pub fn synthetic_keep<K: Recorded>(subject: Subject) -> Keep<K> {
    Keep::issue(subject, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::floor::kind;
    use crate::secure_input::FixedProbe;
    use std::collections::HashMap;

    struct Table(HashMap<u32, Parent>);

    impl Lineage for Table {
        fn parent_of(&self, pid: ProcessId) -> Parent {
            self.0.get(&pid.get()).copied().unwrap_or(Parent::Unknown)
        }
    }

    const APP: u32 = 100;

    fn table() -> Table {
        let of = |pid| Parent::Of(ProcessId::new(pid));
        Table(HashMap::from([
            (1, Parent::None),
            (APP, of(1)),
            (101, of(APP)),
            (102, of(101)),
            (200, of(1)),
            (300, of(301)),
            (301, of(300)),
            (400, of(999)),
        ]))
    }

    fn handle(root: Option<u32>, switches: Switches) -> FloorHandle {
        FloorHandle::new(root.map(ProcessId::new), Arc::new(table()), switches)
    }

    fn app(name: &str) -> AppIdentity {
        AppIdentity::bundle_id(name.to_string())
    }

    fn subject(pid: u32) -> Subject {
        Subject::new(app("com.apple.Terminal"), ProcessId::new(pid))
    }

    fn inactive() -> FixedProbe {
        FixedProbe(SecureInputReading::Inactive)
    }

    fn reason<K: Recorded>(verdict: Verdict<K>) -> Option<DropReason> {
        match verdict {
            Verdict::Keep(_) => None,
            Verdict::Drop(reason) => Some(reason),
        }
    }

    fn pointer(floor: &FloorHandle, subject: Subject) -> Option<DropReason> {
        reason(floor.decide::<kind::Pointer>(subject, PrivateWindow::Unknown, &inactive()))
    }

    #[test]
    fn the_tree_rule_drops_the_root_and_every_process_under_it() {
        let floor = handle(Some(APP), Switches::default());
        let cases = [
            (APP, Some(DropReason::ExclusionTree)),
            (101, Some(DropReason::ExclusionTree)),
            (102, Some(DropReason::ExclusionTree)),
            (200, None),
            (1, None),
            (300, Some(DropReason::UnknownLineage)),
            (400, Some(DropReason::UnknownLineage)),
            (555, Some(DropReason::UnknownLineage)),
        ];
        for (pid, expected) in cases {
            assert_eq!(pointer(&floor, subject(pid)), expected, "pid {pid}");
        }
    }

    #[test]
    fn a_walk_longer_than_the_bound_is_an_unknown_lineage() {
        let chain = |length: u32| {
            let mut parents: HashMap<u32, Parent> = (1..length)
                .map(|pid| (pid + 1, Parent::Of(ProcessId::new(pid))))
                .collect();
            parents.insert(1, Parent::None);
            Table(parents)
        };
        let steps = u32::try_from(LINEAGE_WALK_MAX_STEPS).unwrap();
        for (length, expected) in [(steps, None), (steps + 1, Some(DropReason::UnknownLineage))] {
            let floor = FloorHandle::new(
                Some(ProcessId::new(9_999)),
                Arc::new(chain(length)),
                Switches::default(),
            );
            assert_eq!(
                pointer(&floor, subject(length)),
                expected,
                "chain of {length}"
            );
        }
        assert_eq!(LINEAGE_WALK_MAX_STEPS, 64);
    }

    #[test]
    fn a_parent_loop_stops_at_the_first_repeat() {
        struct Counted(Table, std::sync::atomic::AtomicUsize);

        impl Lineage for Counted {
            fn parent_of(&self, pid: ProcessId) -> Parent {
                self.1.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                self.0.parent_of(pid)
            }
        }

        let lineage = Arc::new(Counted(table(), std::sync::atomic::AtomicUsize::new(0)));
        let floor = FloorHandle::new(
            Some(ProcessId::new(APP)),
            lineage.clone(),
            Switches::default(),
        );
        assert_eq!(
            pointer(&floor, subject(300)),
            Some(DropReason::UnknownLineage)
        );
        assert_eq!(lineage.1.load(std::sync::atomic::Ordering::Relaxed), 2);
    }

    #[test]
    fn with_no_root_the_tree_rule_matches_nothing() {
        let floor = handle(None, Switches::default());
        for pid in [APP, 101, 300, 555] {
            assert_eq!(pointer(&floor, subject(pid)), None, "pid {pid}");
        }
        let default = FloorHandle::default();
        assert_eq!(pointer(&default, subject(APP)), None);
    }

    #[test]
    fn a_listed_app_is_dropped_for_every_kind() {
        let floor = handle(None, Switches::default());
        let listed = || Subject::new(app("com.apple.Passwords"), ProcessId::new(200));
        let probe = inactive();
        let unknown = PrivateWindow::Unknown;
        let reasons = [
            reason(floor.decide::<kind::ForegroundApp>(listed(), unknown, &probe)),
            reason(floor.decide::<kind::WindowTitle>(listed(), unknown, &probe)),
            reason(floor.decide::<kind::Keystroke>(listed(), unknown, &probe)),
            reason(floor.decide::<kind::Pointer>(listed(), unknown, &probe)),
        ];
        assert_eq!(reasons, [Some(DropReason::ListedApp); 4]);
        let mut on = Switches::default();
        on.set(Switch::ScreenActivity, true);
        let floor = handle(None, on);
        let frame = floor.decide::<kind::ScreenFrame>(listed(), unknown, &probe);
        assert_eq!(reason(frame), Some(DropReason::ListedApp));
        for name in ["COM.HALVO.DESKTOP", "com.apple.keychainaccess"] {
            assert_eq!(
                pointer(&floor, Subject::new(app(name), ProcessId::new(200))),
                Some(DropReason::ListedApp),
                "{name}"
            );
        }
        let paths = [
            (
                "C:\\Program Files\\KeePass Password Safe 2\\KeePass.exe",
                Some(DropReason::ListedApp),
            ),
            (
                "D:/Apps/1Password/app/8/1password.exe",
                Some(DropReason::ListedApp),
            ),
            (
                "C:\\Program Files\\Halvo\\halvo.exe",
                Some(DropReason::ListedApp),
            ),
            ("C:\\Windows\\notepad.exe", None),
            ("C:\\Tools\\KeePass.exe.bak", None),
        ];
        for (path, expected) in paths {
            let subject = Subject::new(
                AppIdentity::executable_path(path.to_string()),
                ProcessId::new(200),
            );
            assert_eq!(pointer(&floor, subject), expected, "{path}");
        }
        let by_path_on_a_mac =
            AppIdentity::executable_path("/Applications/com.apple.Passwords".to_string());
        assert_eq!(
            pointer(&floor, Subject::new(by_path_on_a_mac, ProcessId::new(200))),
            None
        );
        assert_eq!(
            pointer(
                &floor,
                Subject::new(AppIdentity::unknown(), ProcessId::new(200))
            ),
            None
        );
        assert_eq!(
            pointer(
                &floor,
                Subject::new(app("com.apple.Passwords.helper"), ProcessId::new(200))
            ),
            None
        );
    }

    #[test]
    fn a_private_window_drops_its_title_and_unknown_keeps_it() {
        let floor = handle(None, Switches::default());
        let probe = inactive();
        let title =
            |window| reason(floor.decide::<kind::WindowTitle>(subject(200), window, &probe));
        assert_eq!(title(PrivateWindow::Unknown), None);
        assert_eq!(
            title(PrivateWindow::Private),
            Some(DropReason::PrivateWindow)
        );
        let key = floor.decide::<kind::Keystroke>(subject(200), PrivateWindow::Private, &probe);
        assert_eq!(reason(key), None);
        assert_eq!(pointer(&floor, subject(200)), None);
    }

    #[test]
    fn a_title_verdict_is_refused_while_the_reading_is_active() {
        let mut on = Switches::default();
        on.set(Switch::ScreenActivity, true);
        let floor = handle(None, on);
        let unknown = PrivateWindow::Unknown;
        for (reading, expected) in [
            (SecureInputReading::Active, Some(DropReason::SecureInput)),
            (SecureInputReading::Inactive, None),
            (SecureInputReading::Unknown, None),
        ] {
            let probe = FixedProbe(reading);
            let title = floor.decide::<kind::WindowTitle>(subject(200), unknown, &probe);
            assert_eq!(reason(title), expected, "title {reading:?}");
            let frame = floor.decide::<kind::ScreenFrame>(subject(200), unknown, &probe);
            assert_eq!(reason(frame), expected, "frame {reading:?}");
            let app = floor.decide::<kind::ForegroundApp>(subject(200), unknown, &probe);
            assert_eq!(reason(app), None, "foreground app {reading:?}");
            let key = floor.decide::<kind::Keystroke>(subject(200), unknown, &probe);
            assert_eq!(reason(key), None, "keystroke {reading:?}");
        }
    }

    #[test]
    fn a_keep_carries_the_site_switch_and_names_its_kind() {
        let probe = inactive();
        let unknown = PrivateWindow::Unknown;
        let site_of = |switches| match handle(None, switches).decide::<kind::WindowTitle>(
            subject(200),
            unknown,
            &probe,
        ) {
            Verdict::Keep(keep) => keep.into_parts().1,
            Verdict::Drop(reason) => panic!("dropped: {reason:?}"),
        };
        assert!(!site_of(Switches::default()));
        let mut on = Switches::default();
        on.set(Switch::SiteRead, true);
        assert!(site_of(on));
        let Verdict::Keep(keep) =
            handle(None, on).decide::<kind::Keystroke>(subject(200), unknown, &probe)
        else {
            panic!("a keystroke was dropped");
        };
        assert_eq!(format!("{keep:?}"), "Keep(keystroke)");
    }

    #[test]
    fn the_recheck_drops_a_listed_app_and_keeps_a_process_that_has_gone() {
        use crate::event::EventClock;
        let event = |name: &str, pid: u32| {
            let subject = Subject::new(app(name), ProcessId::new(pid));
            let keep = Keep::<kind::ForegroundApp>::issue(subject, false);
            FloorEvent::foreground_app(keep, None, EventClock::system())
        };
        let floor = handle(Some(APP), Switches::default());
        assert_eq!(floor.recheck(&event("com.apple.Terminal", 200)), Ok(()));
        assert_eq!(floor.recheck(&event("com.apple.Terminal", 555)), Ok(()));
        assert_eq!(floor.recheck(&event("com.apple.Terminal", 300)), Ok(()));
        assert_eq!(
            floor.recheck(&event("com.apple.Passwords", 200)),
            Err(DropReason::ListedApp)
        );
        assert_eq!(
            floor.recheck(&event("com.apple.Terminal", 102)),
            Err(DropReason::ExclusionTree)
        );
        let frame = {
            let subject = Subject::new(app("com.apple.Terminal"), ProcessId::new(200));
            let keep = Keep::<kind::ScreenFrame>::issue(subject, false);
            FloorEvent::screen_frame(keep, None, EventClock::system())
        };
        assert_eq!(floor.recheck(&frame), Err(DropReason::SwitchOff));
        let mut on = Switches::default();
        on.set(Switch::ScreenActivity, true);
        assert_eq!(handle(Some(APP), on).recheck(&frame), Ok(()));
    }
}
