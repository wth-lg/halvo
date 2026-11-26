#![deny(clippy::let_underscore_must_use)]

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use objc2::rc::autoreleasepool;
use objc2_app_kit::NSWorkspace;

use crate::event::{EventClock, FloorEvent};
use crate::floor::exclude::{
    DropReason, FloorHandle, Keep, PrivateWindow, ProcessId, Subject, Verdict,
};
use crate::floor::kind::{self as floor_kind, Recorded};
use crate::floor::payload::{Title, TitleWhy};
use crate::floor::Switch;
use crate::macos::browser_reads::{self, LanguageReads, Languages};
use crate::macos::run_loop::RunLoopTurns;
use crate::macos::source_failure;
use crate::macos::window_title::{self, Listed, TitleError, TitleText};
use crate::secure_input_macos::SystemProbe;
use crate::sink::SinkHandle;
use crate::source::{
    CaptureError, CaptureSource, HealthReason, SourceContext, SourceCounters, SourceHealth,
    SourceState, Started, StopCause,
};

pub const FOREGROUND_POLL: Duration = Duration::from_millis(400);

pub const FOREGROUND_STOP_DEADLINE: Duration = Duration::from_secs(1);

pub const IDENTITY_CAP_BYTES: usize = 1024;

const IDENTITY_READ_CAP_UNITS: usize = 1024;

/// A text of at most [`IDENTITY_CAP_BYTES`] bytes that is not empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CappedText(String);

impl CappedText {
    fn new(text: &str) -> Option<Self> {
        if text.is_empty() {
            return None;
        }
        let mut end = text.len().min(IDENTITY_CAP_BYTES);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        Some(Self(text[..end].to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What names an app: its bundle id, else the path of its executable, else
/// nothing that the system gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppIdentity {
    BundleId(CappedText),
    ExecutablePath(CappedText),
    Unknown,
}

impl AppIdentity {
    fn of(bundle_id: Option<&str>, executable_path: Option<&str>) -> Self {
        match (
            bundle_id.and_then(CappedText::new),
            executable_path.and_then(CappedText::new),
        ) {
            (Some(id), _) => Self::BundleId(id),
            (None, Some(path)) => Self::ExecutablePath(path),
            (None, None) => Self::Unknown,
        }
    }

    pub fn text(&self) -> &str {
        match self {
            Self::BundleId(text) | Self::ExecutablePath(text) => text.as_str(),
            Self::Unknown => "",
        }
    }

    /// The same identity in the form of a floor event.
    pub fn to_event(&self) -> crate::event::AppIdentity {
        match self {
            Self::BundleId(text) => crate::event::AppIdentity::bundle_id(text.as_str().to_owned()),
            Self::ExecutablePath(text) => {
                crate::event::AppIdentity::executable_path(text.as_str().to_owned())
            }
            Self::Unknown => crate::event::AppIdentity::unknown(),
        }
    }
}

/// Halvo's own app, by one predicate for the screen filter and for the rule on
/// the app in front: the bundle id [`HalvoApp::BUNDLE_ID`], or the executable
/// `halvo` beside the daemon's own executable, which an unbundled build has in
/// place of a bundle id. The whole path of a process in front is read through
/// `path_of`, which is the system's read of a pid's executable path.
#[derive(Debug, Clone)]
pub struct HalvoApp {
    sibling: Option<String>,
    path_of: fn(libc::pid_t) -> Option<String>,
}

impl HalvoApp {
    pub const BUNDLE_ID: &'static str = "com.halvo.desktop";

    /// The app whose executable is the file `halvo` beside this process's own
    /// executable, read through its links.
    pub fn beside_this_executable() -> Self {
        Self::beside(std::env::current_exe().ok().as_deref())
    }

    /// The app whose executable is the file `halvo` beside `executable`: that
    /// file read through its links where it exists, else the file `halvo` in
    /// the directory read through its links, so that a link in the directory
    /// path and a `halvo` that comes later still give the path that the
    /// system reports for its process.
    fn beside(executable: Option<&std::path::Path>) -> Self {
        let sibling = executable
            .and_then(std::path::Path::parent)
            .map(|dir| {
                let path = dir.join("halvo");
                std::fs::canonicalize(&path)
                    .or_else(|_| std::fs::canonicalize(dir).map(|dir| dir.join("halvo")))
                    .unwrap_or(path)
            })
            .and_then(|path| path.to_str().map(str::to_owned));
        Self {
            sibling,
            path_of: executable_path,
        }
    }

    /// The app whose executable is `sibling`.
    pub fn with_sibling(sibling: &str) -> Self {
        Self {
            sibling: Some(sibling.to_owned()),
            path_of: executable_path,
        }
    }

    /// Whether an app with this bundle id (empty when it has none) and the
    /// executable at `path` is Halvo's.
    pub fn selects(&self, bundle_id: &str, path: Option<&str>) -> bool {
        bundle_id == Self::BUNDLE_ID
            || path.is_some_and(|path| self.sibling.as_deref() == Some(path))
    }

    /// Whether the app in front is Halvo's: its identity, else the path of its
    /// process. A path identity is Halvo's only when the whole path of the
    /// front is the sibling.
    pub fn selects_front(&self, front: &Foreground) -> bool {
        match &front.identity {
            AppIdentity::BundleId(id) if id.as_str() == Self::BUNDLE_ID => true,
            AppIdentity::ExecutablePath(path) => self.selects_path(path, front.pid),
            AppIdentity::BundleId(_) | AppIdentity::Unknown => {
                self.selects("", (self.path_of)(front.pid).as_deref())
            }
        }
    }

    /// Whether the path identity `path` of the process `pid` is the sibling. An
    /// identity shorter than the cap less three bytes was not cut, so it is the
    /// whole path. A longer one may be a cut: when it equals the sibling as the
    /// cap cuts it, the whole path of `pid` decides, and a whole path that
    /// cannot be read counts as Halvo's own (ruling MA-37 C3).
    fn selects_path(&self, path: &CappedText, pid: libc::pid_t) -> bool {
        let Some(sibling) = self.sibling.as_deref() else {
            return false;
        };
        if path.as_str().len() < IDENTITY_CAP_BYTES - 3 {
            return path.as_str() == sibling;
        }
        if CappedText::new(sibling).is_none_or(|cut| cut.as_str() != path.as_str()) {
            return false;
        }
        (self.path_of)(pid).is_none_or(|whole| whole == sibling)
    }
}

#[cfg(test)]
impl HalvoApp {
    /// The same app, with `path_of` as the read of a process's whole path.
    pub(crate) fn reading_paths_by(self, path_of: fn(libc::pid_t) -> Option<String>) -> Self {
        Self { path_of, ..self }
    }
}

/// The parent of a process, read from this Mac's process table: the probe that
/// the exclusion decision walks to place a process under the exclusion root.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessLineage;

impl crate::floor::exclude::Lineage for ProcessLineage {
    fn parent_of(&self, pid: crate::floor::exclude::ProcessId) -> crate::floor::exclude::Parent {
        use crate::floor::exclude::{Parent, ProcessId};
        let Ok(pid) = libc::c_int::try_from(pid.get()) else {
            return Parent::Unknown;
        };
        let mut info = std::mem::MaybeUninit::<libc::proc_bsdshortinfo>::zeroed();
        let size = std::mem::size_of::<libc::proc_bsdshortinfo>();
        // SAFETY: `info` is writable for `size` bytes, the size passed, and
        // proc_pidinfo writes at most that many bytes and returns how many it
        // wrote.
        let written = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDT_SHORTBSDINFO,
                0,
                info.as_mut_ptr().cast(),
                size as libc::c_int,
            )
        };
        if usize::try_from(written).ok() != Some(size) {
            return Parent::Unknown;
        }
        // SAFETY: proc_pidinfo wrote the whole value, as the check above shows.
        let parent = unsafe { info.assume_init() }.pbsi_ppid;
        if parent == 0 || parent == pid.unsigned_abs() {
            Parent::None
        } else {
            Parent::Of(ProcessId::new(parent))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Foreground {
    pub identity: AppIdentity,
    pub pid: libc::pid_t,
}

/// The read of the app in front that the poll makes, which a reader makes
/// again at a frame, so that both go through the same reader.
type FrontRead = Arc<dyn Fn() -> Option<Foreground> + Send + Sync>;

/// The poll's last answer, and whether the poll has answered since it began
/// and has not ended.
#[derive(Debug, Default)]
struct FrontState {
    front: Option<Foreground>,
    answered: bool,
}

/// The write side of the app in front.
///  Dropping it clears the value.
pub struct ForegroundWriter {
    state: Arc<Mutex<FrontState>>,
    read: FrontRead,
}

#[derive(Clone)]
pub struct ForegroundReader {
    state: Arc<Mutex<FrontState>>,
    read: FrontRead,
}

impl std::fmt::Debug for ForegroundWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ForegroundWriter")
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for ForegroundReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ForegroundReader")
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

pub(crate) fn foreground_state(
    _one: crate::macos::run_loop::OneWriter,
) -> (ForegroundWriter, ForegroundReader) {
    writer_and_reader(Arc::new(read_frontmost))
}

fn writer_and_reader(read: FrontRead) -> (ForegroundWriter, ForegroundReader) {
    let state = Arc::new(Mutex::new(FrontState::default()));
    (
        ForegroundWriter {
            state: state.clone(),
            read: read.clone(),
        },
        ForegroundReader { state, read },
    )
}

#[cfg(test)]
pub(crate) fn foreground_state_for_test() -> (ForegroundWriter, ForegroundReader) {
    let state = Arc::new(Mutex::new(FrontState::default()));
    let last = state.clone();
    let read: FrontRead = Arc::new(move || {
        last.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .front
            .clone()
    });
    (
        ForegroundWriter {
            state: state.clone(),
            read: read.clone(),
        },
        ForegroundReader { state, read },
    )
}

impl ForegroundWriter {
    fn set(&self, front: Option<Foreground>) {
        *self.state.lock().unwrap_or_else(PoisonError::into_inner) = FrontState {
            front,
            answered: true,
        };
    }

    /// The poll's reader: the same read that a reader makes at a frame.
    fn poll_reader(&self) -> FrontReader {
        let read = self.read.clone();
        Box::new(move || read())
    }
}

#[cfg(test)]
impl ForegroundWriter {
    pub(crate) fn set_for_test(&self, front: Option<Foreground>) {
        self.set(front);
    }
}

#[cfg(test)]
pub(crate) mod tests_support {
    use super::{AppIdentity, Foreground};

    pub(crate) fn front(bundle_id: &str, pid: libc::pid_t) -> Foreground {
        Foreground {
            identity: AppIdentity::of(Some(bundle_id), None),
            pid,
        }
    }

    pub(crate) fn unbundled(path: &str) -> AppIdentity {
        AppIdentity::of(None, Some(path))
    }

    pub(crate) fn stored_and_read(
        stored: Foreground,
        read: Foreground,
    ) -> (super::ForegroundWriter, super::ForegroundReader) {
        let (writer, reader) =
            super::writer_and_reader(std::sync::Arc::new(move || Some(read.clone())));
        writer.set(Some(stored));
        (writer, reader)
    }

    /// A foreground source with the product's poll interval whose poll reads
    /// `system_front`, the stand-in of the system's app in front, through the
    /// writer's read as the product's source does, and the reader of its value.
    pub(crate) fn polled_source(
        system_front: std::sync::Arc<std::sync::Mutex<Option<Foreground>>>,
    ) -> (super::ForegroundSource, super::ForegroundReader) {
        let read: super::FrontRead = std::sync::Arc::new(move || {
            system_front
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        });
        let (writer, reader) = super::writer_and_reader(read);
        (
            super::ForegroundSource::polling(
                super::FOREGROUND_POLL,
                writer,
                super::RunLoopTurns::for_test(std::time::Instant::now()),
                Box::new(|_| Ok(None)),
                super::TitleRules::unread(),
            ),
            reader,
        )
    }

    /// The product's writer and reader over the product's read of the app in
    /// front, the call that `foreground_state` makes, answered as the poll's
    /// first answer is: the writer makes one read through the poll's reader and
    /// stores it when it names an app; the drop of the writer clears the state.
    pub(crate) fn answered_by_the_system() -> (super::ForegroundWriter, super::ForegroundReader) {
        let (writer, reader) = super::writer_and_reader(std::sync::Arc::new(super::read_frontmost));
        let mut read = writer.poll_reader();
        if let Some(front) = read() {
            writer.set(Some(front));
        }
        (writer, reader)
    }
}

impl Drop for ForegroundWriter {
    fn drop(&mut self) {
        *self.state.lock().unwrap_or_else(PoisonError::into_inner) = FrontState::default();
    }
}

impl ForegroundWriter {
    /// A writer on the same state and read, for the next foreground source of
    /// a service run once the source that held the last one has stopped; its
    /// drop clears the value, as the drop of every writer does.
    pub(crate) fn share(&self) -> ForegroundWriter {
        ForegroundWriter {
            state: self.state.clone(),
            read: self.read.clone(),
        }
    }
}

#[cfg(test)]
impl ForegroundSource {
    /// The product's source of [`ForegroundSource::new`] whose title reader
    /// is a stand-in that reads no title, for a test that must read no title
    /// of the machine that runs it.
    pub(crate) fn with_stand_in_title(
        interval: Duration,
        foreground: ForegroundWriter,
        turns: RunLoopTurns,
    ) -> Self {
        let mut source = Self::new(interval, foreground, turns);
        if let Some(readers) = source.readers.as_mut() {
            readers.1 = Box::new(|_| Ok(None));
        }
        source
    }
}

#[cfg(debug_assertions)]
impl ForegroundReader {
    /// A reader with no writer, which reads nothing in front: for tests in a
    /// process of their own, which cannot claim the main run loop.
    pub fn nothing_in_front() -> Self {
        writer_and_reader(Arc::new(|| None)).1
    }
}

#[cfg(all(debug_assertions, feature = "stall-seam"))]
impl ForegroundReader {
    /// A reader with no writer whose app in front is always the executable at
    /// `executable_path` with `pid`: for the stall fixture alone, and no release
    /// build holds it.
    pub fn fixed_front(executable_path: &str, pid: libc::pid_t) -> Self {
        let front = Foreground {
            identity: AppIdentity::of(None, Some(executable_path)),
            pid,
        };
        let read = front.clone();
        Self {
            state: Arc::new(Mutex::new(FrontState {
                front: Some(front),
                answered: true,
            })),
            read: Arc::new(move || Some(read.clone())),
        }
    }
}

impl ForegroundReader {
    /// The app in front, or `None` before the first reading and while nothing is
    /// in front.
    pub fn get(&self) -> Option<Foreground> {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .front
            .clone()
    }

    /// The app in front read now, through the reader that the poll uses; `None`
    /// before the poll has answered once, after the poll has ended, and when
    /// the read fails or names nothing in front.
    pub(crate) fn read_now(&self) -> Option<Foreground> {
        if !self
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .answered
        {
            return None;
        }
        (self.read)()
    }
}

pub(crate) fn executable_path(pid: libc::pid_t) -> Option<String> {
    let mut buffer = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    // SAFETY: `buffer` is writable for the size passed, and proc_pidpath writes
    // at most that many bytes and returns how many it wrote.
    let written =
        unsafe { libc::proc_pidpath(pid, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
    let written = usize::try_from(written).ok().filter(|&n| n > 0)?;
    buffer.truncate(written.min(buffer.len()));
    Some(String::from_utf8_lossy(&buffer).into_owned())
}

/// The text of a bundle id: at most the cap of UTF-16 units, an unpaired
/// surrogate read as U+FFFD; it never aborts.
pub(crate) fn bundle_identifier(text: &objc2_foundation::NSString) -> String {
    let string = (text as *const objc2_foundation::NSString).cast::<std::ffi::c_void>()
        as core_foundation::string::CFStringRef;
    // SAFETY: an NSString is toll-free bridged with CFString, and `text`
    // keeps the string alive for the call.
    let total = unsafe { core_foundation::string::CFStringGetLength(string) };
    let take = usize::try_from(total)
        .unwrap_or(0)
        .min(IDENTITY_READ_CAP_UNITS);
    let mut units = vec![0u16; take];
    // SAFETY: the range starts at 0 and holds `take` units, at most the
    // string's length, and `units` has room for `take` units.
    unsafe {
        core_foundation::string::CFStringGetCharacters(
            string,
            core_foundation::base::CFRange::init(0, take as isize),
            units.as_mut_ptr(),
        )
    };
    String::from_utf16_lossy(&units)
}

fn read_frontmost() -> Option<Foreground> {
    autoreleasepool(|_| {
        let workspace = NSWorkspace::sharedWorkspace();
        let app = workspace.frontmostApplication()?;
        let pid = app.processIdentifier();
        let bundle_id = app.bundleIdentifier().map(|s| bundle_identifier(&s));
        let path = executable_path(pid);
        Some(Foreground {
            identity: AppIdentity::of(bundle_id.as_deref(), path.as_deref()),
            pid,
        })
    })
}

type FrontReader = Box<dyn FnMut() -> Option<Foreground> + Send>;
type TitleRead = Result<Option<TitleText>, TitleError>;
type TitleReader = Box<dyn FnMut(libc::pid_t) -> TitleRead + Send>;

#[derive(Debug, Clone, PartialEq, Eq)]
struct TitleRequest {
    front: Foreground,
    change: u64,
}

/// The system's list of handlers of https links as bundle ids, or `None` when
/// it could not be read: the one seam of that list, which a test replaces.
type HandlerRead = Box<dyn FnMut() -> Option<Vec<String>> + Send>;

/// What the title rules make of a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TitleRule {
    /// No browser: its title is read and kept as any app's.
    Kept,
    /// Chrome or Edge in a process that runs in English: its title is read,
    /// and left out unless the rule by that browser's own name keeps it.
    NameEnding(window_title::OwnName),
    /// Any other browser, and the two under another language or at a failed
    /// reading: no title is read.
    NoTitle,
}

impl TitleRule {
    fn mark(self, title: TitleText) -> TitleText {
        match self {
            Self::NameEnding(own) => title.of_a_browser_kept_by_its_name(own),
            Self::Kept | Self::NoTitle => title,
        }
    }
}

/// The kept value of a place that a browser's language is read from, and its
/// epoch in the machine's ticks: taken after the readings at the source's
/// start, or after the readings of the last change of the app in front at
/// which this place gave another value than the one before.
struct Place {
    value: Languages,
    epoch: u64,
}

impl Place {
    fn read(&mut self, value: Languages, now: u64) {
        if value != self.value {
            *self = Self { value, epoch: now };
        }
    }
}

/// The decision for one process of Chrome or Edge, found by the browser, the
/// pid and the start of the process in ticks: taken at its first title
/// request and kept until another process of that browser is decided.
struct Decided {
    browser: &'static str,
    pid: libc::pid_t,
    started: u64,
    english: bool,
}

/// The rules that tell, on the title thread, whether the title of the app in
/// front is read and by which browser's own name it is kept, with the reads of
/// the system that they make: the places of the apps that the system lists for
/// https links, and the place of the app in front, at most once for each change
/// of the app in front whose request reaches the rules; the two places of each
/// browser's language (its own value and the person's global list), each after
/// a synchronize call, at the source's start and at each such change; the start
/// of a process of Chrome or Edge at each of its title requests; and its
/// localization once, read only from a bundle under an Applications folder.
pub(crate) struct TitleRules {
    handlers: HandlerRead,
    languages: Box<dyn LanguageReads>,
    home: Option<String>,
    change: Option<u64>,
    list: Option<Option<Vec<String>>>,
    global: Option<Place>,
    own: Vec<(&'static str, Place)>,
    decided: Vec<Decided>,
}

impl TitleRules {
    fn new(handlers: HandlerRead, languages: Box<dyn LanguageReads>) -> Self {
        let home = std::env::var("HOME")
            .ok()
            .filter(|home| home.starts_with('/'));
        Self {
            handlers,
            languages,
            home,
            change: None,
            list: None,
            global: None,
            own: Vec::new(),
            decided: Vec::new(),
        }
    }

    fn system() -> Self {
        Self::new(
            Box::new(browser_reads::https_handlers),
            Box::new(browser_reads::SystemLanguages),
        )
    }

    /// Reads both places of each browser at the source's start; the ticks
    /// after those readings are their first epoch, so a browser that already
    /// runs keeps no title until it starts again.
    fn begin(&mut self) {
        let global = self.languages.global();
        let own: Vec<(&'static str, Languages)> = window_title::NAME_ENDINGS
            .iter()
            .map(|&(browser, _)| (browser, self.languages.own(browser)))
            .collect();
        let now = self.languages.now();
        self.global = Some(Place {
            value: global,
            epoch: now,
        });
        self.own = own
            .into_iter()
            .map(|(browser, value)| (browser, Place { value, epoch: now }))
            .collect();
    }

    /// At a change of the app in front whose request reaches the rules: reads
    /// both places of each browser again and, after those readings, sets the
    /// value and the epoch of each place that gave another value, so that each
    /// kept value and its epoch follow every change that a reading sees; and
    /// forgets the system's list.
    fn at_change(&mut self) {
        let global = self.languages.global();
        let own: Vec<Languages> = self
            .own
            .iter()
            .map(|(browser, _)| self.languages.own(browser))
            .collect();
        let now = self.languages.now();
        if let Some(place) = self.global.as_mut() {
            place.read(global, now);
        }
        for ((_, place), value) in self.own.iter_mut().zip(own) {
            place.read(value, now);
        }
        self.list = None;
    }

    fn rule_of(&mut self, request: &TitleRequest) -> TitleRule {
        if self.change != Some(request.change) {
            self.change = Some(request.change);
            self.at_change();
        }
        let AppIdentity::BundleId(id) = &request.front.identity else {
            return TitleRule::Kept;
        };
        let id = id.as_str();
        match window_title::listed(id) {
            Listed::Ending(browser, own) => {
                if self.english(browser, id, request.front.pid) {
                    TitleRule::NameEnding(own)
                } else {
                    TitleRule::NoTitle
                }
            }
            Listed::Browser => TitleRule::NoTitle,
            Listed::Domain => match self.named_by_system(id, request.front.pid) {
                Some(false) => TitleRule::Kept,
                Some(true) | None => TitleRule::NoTitle,
            },
            Listed::Other => match self.named_by_system(id, request.front.pid) {
                Some(true) => TitleRule::NoTitle,
                Some(false) | None => TitleRule::Kept,
            },
        }
    }

    /// Whether the system's list names the place of the app `id` with `pid`,
    /// both as the system gives them, set against each other as text with one
    /// closing slash taken off; `None` when the list or the place could not be
    /// read.
    fn named_by_system(&mut self, id: &str, pid: libc::pid_t) -> Option<bool> {
        let list = self.list.get_or_insert_with(|| (self.handlers)()).clone()?;
        let place = self.languages.place(pid, id)?;
        let place = place.trim_end_matches('/');
        Some(
            list.iter()
                .any(|named| named.trim_end_matches('/') == place),
        )
    }

    /// Whether the process `pid` of `browser` runs in English: its start is
    /// read at each title request, and a decision is found by the browser, the
    /// pid and that start; another start is decided anew.
    fn english(&mut self, browser: &'static str, id: &str, pid: libc::pid_t) -> bool {
        let Some(started) = self.languages.started(pid) else {
            return false;
        };
        if let Some(decided) = self.decided.iter().find(|decided| {
            decided.browser == browser && decided.pid == pid && decided.started == started
        }) {
            return decided.english;
        }
        let english = self.first_title(browser, id, pid, started);
        self.decided.retain(|decided| decided.browser != browser);
        self.decided.push(Decided {
            browser,
            pid,
            started,
            english,
        });
        english
    }

    /// A process of `browser` that started at the tick `started` runs in
    /// English when it started after the epochs of both its places, which the
    /// readings at this change of the app in front have just moved if they
    /// gave another value, its bundle lies under an Applications folder, and
    /// the bundle takes the localization [`window_title::ENGLISH`] under the
    /// kept list: its own value, else the person's global list.
    fn first_title(
        &mut self,
        browser: &'static str,
        id: &str,
        pid: libc::pid_t,
        started: u64,
    ) -> bool {
        let (Some(global), Some((_, own))) = (
            self.global.as_ref(),
            self.own.iter().find(|(name, _)| *name == browser),
        ) else {
            return false;
        };
        let epoch = global.epoch.max(own.epoch);
        let list = match (&own.value, &global.value) {
            (Languages::List(list), _) | (Languages::NoValue, Languages::List(list)) => {
                list.clone()
            }
            _ => return false,
        };
        if started <= epoch {
            return false;
        }
        let Some(place) = self.languages.place(pid, id) else {
            return false;
        };
        if !window_title::in_applications(&place, self.home.as_deref()) {
            return false;
        }
        self.languages.localization(&place, &list).as_deref() == Some(window_title::ENGLISH)
    }
}

#[cfg(test)]
impl TitleRules {
    /// Rules whose reads of the system all fail, and read nothing of it: the
    /// closed list alone stands, and Chrome and Edge keep no title.
    pub(crate) fn unread() -> Self {
        struct Unread;
        impl LanguageReads for Unread {
            fn own(&mut self, _bundle_id: &str) -> Languages {
                Languages::Failed
            }
            fn global(&mut self) -> Languages {
                Languages::Failed
            }
            fn now(&mut self) -> u64 {
                0
            }
            fn started(&mut self, _pid: libc::pid_t) -> Option<u64> {
                None
            }
            fn place(&mut self, _pid: libc::pid_t, _bundle_id: &str) -> Option<String> {
                None
            }
            fn localization(&mut self, _place: &str, _languages: &[String]) -> Option<String> {
                None
            }
        }
        Self::new(Box::new(|| None), Box::new(Unread))
    }
}

fn window_of(front: &Foreground) -> PrivateWindow {
    let bundle_id = match &front.identity {
        AppIdentity::BundleId(id) => Some(id.as_str()),
        AppIdentity::ExecutablePath(_) | AppIdentity::Unknown => None,
    };
    window_title::window_privacy(bundle_id)
}

fn verdict_of<K: Recorded>(
    floor: &FloorHandle,
    probe: &SystemProbe,
    front: &Foreground,
    window: PrivateWindow,
) -> Option<Verdict<K>> {
    let pid = ProcessId::new(u32::try_from(front.pid).ok()?);
    let subject = Subject::new(front.identity.to_event(), pid);
    Some(floor.decide::<K>(subject, window, probe))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TitleAnswer {
    change: u64,
    read: TitleRead,
}

#[derive(Default)]
struct TitleHealth {
    errors: AtomicU64,
    failing: AtomicBool,
    not_trusted: AtomicBool,
    stop_asked: AtomicBool,
    dropped_at_stop: AtomicU64,
    /// The titles that the rule by the name leaves out at their read, with any
    /// title read of a window that the closed list tells private.
    left_out_by_the_name: AtomicU64,
}

impl TitleHealth {
    fn emits(&self) -> bool {
        if self.stop_asked.load(Ordering::SeqCst) {
            self.dropped_at_stop.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        true
    }
}

struct Poller {
    writer: ForegroundWriter,
    front: Option<Foreground>,
    change: u64,
    last_title: Option<String>,
    titled: bool,
    health: Arc<TitleHealth>,
    floor: FloorHandle,
    probe: SystemProbe,
    clock: &'static EventClock,
}

impl Poller {
    fn new(writer: ForegroundWriter, health: Arc<TitleHealth>) -> Self {
        Self {
            writer,
            front: None,
            change: 0,
            last_title: None,
            titled: false,
            health,
            floor: FloorHandle::default(),
            probe: SystemProbe::new(),
            clock: EventClock::system(),
        }
    }

    fn verdict<K: Recorded>(
        &self,
        front: &Foreground,
        window: PrivateWindow,
    ) -> Option<Verdict<K>> {
        verdict_of::<K>(&self.floor, &self.probe, front, window)
    }

    fn keep<K: Recorded>(&self, front: &Foreground, window: PrivateWindow) -> Option<Keep<K>> {
        match self.verdict::<K>(front, window)? {
            Verdict::Keep(keep) => Some(keep),
            Verdict::Drop(_) => None,
        }
    }

    fn dropped(&mut self, reason: DropReason) {
        if reason == DropReason::SecureInput {
            self.change += 1;
        }
    }

    fn take_answer(&mut self, answer: TitleAnswer, sink: &SinkHandle) {
        if answer.change != self.change {
            return;
        }
        match answer.read {
            Ok(Some(title)) => {
                self.health.failing.store(false, Ordering::Relaxed);
                self.health.not_trusted.store(false, Ordering::Relaxed);
                self.title_read(title, sink);
            }
            Ok(None) => {}
            Err(TitleError::NotTrusted) => {
                self.health.not_trusted.store(true, Ordering::Relaxed);
            }
            Err(_) => {
                self.health.errors.fetch_add(1, Ordering::Relaxed);
                self.health.failing.store(true, Ordering::Relaxed);
            }
        }
    }

    fn title_read(&mut self, title: TitleText, sink: &SinkHandle) {
        if self.last_title.as_deref() == Some(title.text()) {
            return;
        }
        let Some(front) = self.front.clone() else {
            return;
        };
        let window = window_of(&front);
        if window == PrivateWindow::Private || title.left_out_by_its_browser() {
            self.health
                .left_out_by_the_name
                .fetch_add(1, Ordering::Relaxed);
            self.last_title = Some(title.into_text());
            return;
        }
        let keep = match self.verdict::<floor_kind::WindowTitle>(&front, window) {
            Some(Verdict::Keep(keep)) => keep,
            Some(Verdict::Drop(reason)) => return self.dropped(reason),
            None => return,
        };
        if !self.health.emits() {
            return;
        }
        let why = if self.titled {
            TitleWhy::TitleChange
        } else {
            TitleWhy::AppChange
        };
        let text = title.into_text();
        self.last_title = Some(text.clone());
        self.titled = true;
        let site = window_title::site_of(front.pid);
        sink.emit_event(FloorEvent::window_title(
            keep,
            Title::new(text, why, site),
            None,
            self.clock,
        ));
    }

    fn step(
        &mut self,
        reading: Option<Foreground>,
        answers: Vec<TitleAnswer>,
        sink: &SinkHandle,
    ) -> Option<TitleRequest> {
        for answer in answers {
            self.take_answer(answer, sink);
        }
        let Some(front) = reading else {
            if self.front.take().is_some() {
                self.change += 1;
                self.writer.set(None);
            }
            return None;
        };
        if self.front.as_ref().map(|known| known.pid) != Some(front.pid) {
            self.change += 1;
            self.front = Some(front.clone());
            self.writer.set(Some(front.clone()));
            self.last_title = None;
            self.titled = false;
            if let Some(keep) =
                self.keep::<floor_kind::ForegroundApp>(&front, PrivateWindow::Unknown)
            {
                if self.health.emits() {
                    sink.emit_event(FloorEvent::foreground_app(keep, None, self.clock));
                }
            }
        }
        if !self.floor.switches().is_on(Switch::TitleText) {
            return None;
        }
        match self.verdict::<floor_kind::WindowTitle>(&front, window_of(&front)) {
            Some(Verdict::Keep(_)) => {}
            Some(Verdict::Drop(reason)) => {
                self.dropped(reason);
                return None;
            }
            None => return None,
        }
        Some(TitleRequest {
            front,
            change: self.change,
        })
    }
}

struct TitleQueue {
    latest: Option<TitleRequest>,
    closed: bool,
}

struct TitleThread {
    queue: Arc<(Mutex<TitleQueue>, Condvar)>,
    answers: Receiver<TitleAnswer>,
}

/// The title thread's answer to one request: no title where the floor refuses
/// it before or after the read, or where the title rules read none; else the
/// title read, marked with its browser's own name where the rules keep a
/// title only by that name.
fn answer_of(
    request: &TitleRequest,
    rules: &mut TitleRules,
    read_title: &mut TitleReader,
    floor: &FloorHandle,
    probe: &SystemProbe,
) -> TitleAnswer {
    let refused = || {
        !matches!(
            verdict_of::<floor_kind::WindowTitle>(
                floor,
                probe,
                &request.front,
                window_of(&request.front),
            ),
            Some(Verdict::Keep(_))
        )
    };
    let read = if refused() {
        Ok(None)
    } else {
        match catch_unwind(AssertUnwindSafe(|| rules.rule_of(request)))
            .unwrap_or(TitleRule::NoTitle)
        {
            TitleRule::NoTitle => Ok(None),
            rule => {
                let read = catch_unwind(AssertUnwindSafe(|| read_title(request.front.pid)))
                    .unwrap_or(Err(TitleError::ReaderPanicked));
                if refused() {
                    Ok(None)
                } else {
                    read.map(|title| title.map(|title| rule.mark(title)))
                }
            }
        }
    };
    TitleAnswer {
        change: request.change,
        read,
    }
}

impl TitleThread {
    fn spawn(
        mut read_title: TitleReader,
        mut rules: TitleRules,
        floor: FloorHandle,
        probe: SystemProbe,
    ) -> Result<(Self, JoinHandle<()>), CaptureError> {
        let queue = Arc::new((
            Mutex::new(TitleQueue {
                latest: None,
                closed: false,
            }),
            Condvar::new(),
        ));
        let (tx, answers) = channel();
        let shared = queue.clone();
        let thread = std::thread::Builder::new()
            .name("halvo-window-title".into())
            .spawn(move || {
                catch_unwind(AssertUnwindSafe(|| rules.begin())).ok();
                while let Some(request) = next_request(&shared) {
                    let answer = answer_of(&request, &mut rules, &mut read_title, &floor, &probe);
                    if tx.send(answer).is_err() {
                        return;
                    }
                }
            })
            .map_err(thread_start_failure)?;
        Ok((Self { queue, answers }, thread))
    }

    fn ask(&self, request: TitleRequest) {
        let (queue, wake) = &*self.queue;
        queue.lock().unwrap_or_else(PoisonError::into_inner).latest = Some(request);
        wake.notify_one();
    }

    fn answers(&self) -> Vec<TitleAnswer> {
        self.answers.try_iter().collect()
    }
}

impl Drop for TitleThread {
    fn drop(&mut self) {
        let (queue, wake) = &*self.queue;
        queue.lock().unwrap_or_else(PoisonError::into_inner).closed = true;
        wake.notify_all();
    }
}

fn next_request(queue: &(Mutex<TitleQueue>, Condvar)) -> Option<TitleRequest> {
    let (queue, wake) = queue;
    let mut queue = queue.lock().unwrap_or_else(PoisonError::into_inner);
    loop {
        if queue.closed {
            return None;
        }
        if let Some(request) = queue.latest.take() {
            return Some(request);
        }
        queue = wake.wait(queue).unwrap_or_else(PoisonError::into_inner);
    }
}

fn thread_start_failure(_error: std::io::Error) -> CaptureError {
    CaptureError::Start("the foreground thread was not started".into())
}

fn poll_loop(
    mut read_front: FrontReader,
    mut poller: Poller,
    titles: TitleThread,
    sink: SinkHandle,
    interval: Duration,
    stop: Receiver<()>,
) {
    loop {
        if let Some(request) = poller.step(read_front(), titles.answers(), &sink) {
            titles.ask(request);
        }
        match stop.recv_timeout(interval) {
            Err(RecvTimeoutError::Timeout) => {}
            Ok(()) | Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

struct Running {
    stop: Sender<()>,
    stop_sent: bool,
    thread: JoinHandle<()>,
    title: JoinHandle<()>,
}

impl Running {
    fn ended(&self) -> bool {
        self.thread.is_finished() && self.title.is_finished()
    }
}

pub struct ForegroundSource {
    interval: Duration,
    turns: RunLoopTurns,
    readers: Option<(FrontReader, TitleReader, TitleRules, ForegroundWriter)>,
    health: Arc<TitleHealth>,
    running: Option<Running>,
    failure: Option<CaptureError>,
    unstopped: Option<CaptureError>,
    stopped: bool,
    #[cfg(test)]
    real: bool,
}

impl ForegroundSource {
    pub fn new(interval: Duration, foreground: ForegroundWriter, turns: RunLoopTurns) -> Self {
        #[cfg_attr(not(test), allow(unused_mut))]
        let mut source = Self::polling(
            interval,
            foreground,
            turns,
            Box::new(window_title::focused_window_title),
            TitleRules::system(),
        );
        #[cfg(test)]
        {
            source.real = true;
        }
        source
    }

    /// A source whose poll reads the app in front through the writer's read,
    /// the one that its readers make again at a frame.
    fn polling(
        interval: Duration,
        foreground: ForegroundWriter,
        turns: RunLoopTurns,
        read_title: TitleReader,
        rules: TitleRules,
    ) -> Self {
        let read_front = foreground.poll_reader();
        Self::with_readers(interval, foreground, turns, read_front, read_title, rules)
    }

    fn with_readers(
        interval: Duration,
        foreground: ForegroundWriter,
        turns: RunLoopTurns,
        read_front: FrontReader,
        read_title: TitleReader,
        rules: TitleRules,
    ) -> Self {
        Self {
            interval,
            turns,
            readers: Some((read_front, read_title, rules, foreground)),
            health: Arc::new(TitleHealth::default()),
            running: None,
            failure: None,
            unstopped: None,
            stopped: false,
            #[cfg(test)]
            real: false,
        }
    }

    fn running_state(&self) -> SourceState {
        let impaired = |reason| SourceState::Impaired { reason };
        if self.turns.stalled(Instant::now()) {
            impaired(HealthReason::RunLoopStalled)
        } else if self.health.not_trusted.load(Ordering::Relaxed) {
            impaired(HealthReason::Permission)
        } else if self.health.failing.load(Ordering::Relaxed) {
            impaired(HealthReason::TitleReadErrors)
        } else {
            SourceState::Running
        }
    }
}

impl CaptureSource for ForegroundSource {
    fn begin(&mut self, ctx: SourceContext) -> Result<Started, CaptureError> {
        if self.running.is_some() {
            return Err(CaptureError::AlreadyStarted);
        }
        let Some((read_front, read_title, rules, writer)) = self.readers.take() else {
            return Err(CaptureError::Start(
                "the foreground source starts once".into(),
            ));
        };
        let mut poller = Poller::new(writer, self.health.clone());
        poller.floor = ctx.floor().clone();
        let (stop, stop_rx) = channel();
        let sink = ctx.sink().clone();
        let interval = self.interval;
        let started = TitleThread::spawn(read_title, rules, poller.floor.clone(), poller.probe)
            .and_then(|(titles, title)| {
                std::thread::Builder::new()
                    .name("halvo-foreground".into())
                    .spawn(move || poll_loop(read_front, poller, titles, sink, interval, stop_rx))
                    .map(|thread| (thread, title))
                    .map_err(thread_start_failure)
            });
        match started {
            Ok((thread, title)) => {
                self.running = Some(Running {
                    stop,
                    stop_sent: false,
                    thread,
                    title,
                });
                self.failure = None;
                self.stopped = false;
                Ok(Started::Running)
            }
            Err(error) => {
                self.failure = Some(error.clone());
                Err(error)
            }
        }
    }

    fn shutdown(&mut self) -> Result<(), CaptureError> {
        let Some(running) = self.running.as_mut() else {
            return self.unstopped.clone().map_or(Ok(()), Err);
        };
        let ended_before = !running.stop_sent && running.thread.is_finished();
        self.health.stop_asked.store(true, Ordering::SeqCst);
        running.stop.send(()).ok();
        running.stop_sent = true;
        let called = Instant::now();
        while !running.ended() {
            if called.elapsed() >= FOREGROUND_STOP_DEADLINE {
                return self.answered(Err(CaptureError::Timeout {
                    stage: "foreground_join",
                    ms: FOREGROUND_STOP_DEADLINE.as_millis() as u64,
                }));
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        let joined = self
            .running
            .take()
            .map(|running| (running.thread.join(), running.title.join()));
        let answer = if let (false, Some((Ok(()), Ok(())))) = (ended_before, joined) {
            Ok(())
        } else {
            Err(CaptureError::ThreadEnded)
        };
        self.answered(answer)
    }

    fn health(&self) -> SourceHealth {
        let state = match (&self.running, &self.failure) {
            (Some(running), _) if running.thread.is_finished() => {
                source_failure(CaptureError::ThreadEnded)
            }
            (Some(_), _) => self.running_state(),
            (None, Some(error)) => source_failure(error.clone()),
            (None, None) if self.stopped => SourceState::Stopped(StopCause::Request),
            (None, None) => SourceState::NotReported,
        };
        let state = self.unstopped.clone().map_or(state, source_failure);
        SourceHealth {
            state,
            counters: SourceCounters {
                title_read_errors: self.health.errors.load(Ordering::Relaxed),
                ..SourceCounters::default()
            },
        }
    }
}

impl ForegroundSource {
    fn answered(&mut self, answer: Result<(), CaptureError>) -> Result<(), CaptureError> {
        match &answer {
            Ok(()) => {
                self.unstopped = None;
                self.stopped = true;
            }
            Err(CaptureError::ThreadEnded) => {
                self.unstopped = None;
                self.failure = Some(CaptureError::ThreadEnded);
            }
            Err(error) => self.unstopped = Some(error.clone()),
        }
        #[cfg(test)]
        note_shutdown_answer("foreground", self.real, &answer);
        answer
    }
}

impl Drop for ForegroundSource {
    fn drop(&mut self) {
        if self.unstopped.is_some() {
            return;
        }
        self.shutdown().ok();
    }
}

/// Appends one line for the `shutdown` answer of a source that was begun through
/// its product path to the file that `HALVO_SHUTDOWN_ANSWERS` names, for the
/// count that scripts/r1/ci.d/12-test.sh prints; nothing when the variable is
/// unset or the source is a stand-in.
#[cfg(test)]
pub(crate) fn note_shutdown_answer(source: &str, real: bool, answer: &Result<(), CaptureError>) {
    use std::io::Write;
    let Some(path) = std::env::var_os("HALVO_SHUTDOWN_ANSWERS") else {
        return;
    };
    if !real {
        return;
    }
    let line = format!("shutdown source={source} ok={}\n", answer.is_ok());
    let written = std::fs::OpenOptions::new()
        .append(true)
        .open(path)
        .and_then(|mut file| file.write_all(line.as_bytes()));
    assert!(
        written.is_ok(),
        "the file of HALVO_SHUTDOWN_ANSWERS takes no line: {written:?}"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Modality;
    use crate::sink::{self, SinkWriter};
    use core_foundation::base::{kCFAllocatorDefault, TCFType};
    use core_foundation::string::{CFString, CFStringCreateWithCharacters};

    fn utf16_string(units: &[u16]) -> CFString {
        // SAFETY: `units` holds `units.len()` UTF-16 units for the call; the
        // create call copies them and returns a +1 reference.
        unsafe {
            CFString::wrap_under_create_rule(CFStringCreateWithCharacters(
                kCFAllocatorDefault,
                units.as_ptr(),
                units.len() as isize,
            ))
        }
    }

    fn as_nsstring(text: &CFString) -> &objc2_foundation::NSString {
        // SAFETY: a CFString is toll-free bridged with NSString, and the
        // reference borrows `text`, which keeps the string alive.
        unsafe { &*(text.as_concrete_TypeRef() as *const objc2_foundation::NSString) }
    }

    #[test]
    fn a_bundle_identifier_with_an_unpaired_surrogate_is_read_lossily_and_within_the_cap() {
        let evil = utf16_string(&[0x63, 0x6F, 0x6D, 0x2E, 0x65, 0x76, 0x69, 0x6C, 0xD800]);
        eprintln!("before the conversion of a bundle identifier");
        let id = bundle_identifier(as_nsstring(&evil));
        eprintln!("after the conversion of a bundle identifier");
        let identity = AppIdentity::of(
            Some(&id),
            Some("/Applications/Evil.app/Contents/MacOS/Evil"),
        );
        assert_eq!(identity.text(), "com.evil\u{FFFD}");
        assert!(matches!(identity, AppIdentity::BundleId(_)));
        let long = utf16_string(&vec![u16::from(b'a'); IDENTITY_READ_CAP_UNITS + 976]);
        let read = bundle_identifier(as_nsstring(&long));
        assert_eq!(read, "a".repeat(IDENTITY_READ_CAP_UNITS));
        assert!(read.len() <= IDENTITY_CAP_BYTES);
    }

    fn app(bundle_id: &str, pid: libc::pid_t) -> Option<Foreground> {
        Some(Foreground {
            identity: AppIdentity::of(Some(bundle_id), None),
            pid,
        })
    }

    fn unbundled(path: &str, pid: libc::pid_t) -> Option<Foreground> {
        Some(Foreground {
            identity: AppIdentity::of(None, Some(path)),
            pid,
        })
    }

    fn fresh_turns() -> RunLoopTurns {
        RunLoopTurns::for_test(Instant::now())
    }

    fn shown(received: crate::sink::Received) -> (Modality, String, String) {
        let floor = received.into_floor().expect("a floor value");
        let event = floor.event();
        let app = event
            .app()
            .text()
            .map(|(text, field)| crate::floor::cases::stored(text, field))
            .unwrap_or_default();
        let title = match event.observation() {
            crate::floor::payload::Observation::WindowTitle(title) => {
                crate::floor::cases::stored(&title.text, crate::floor::text::TextField::Title)
            }
            _ => String::new(),
        };
        (event.kind(), app, title)
    }

    fn events(handle: SinkHandle, writer: &SinkWriter) -> Vec<(Modality, String, String)> {
        drop(handle);
        let mut kinds = Vec::new();
        loop {
            match writer.recv_timeout(Duration::from_secs(2)) {
                Ok(received) => kinds.push(shown(received)),
                Err(RecvTimeoutError::Disconnected) => return kinds,
                Err(RecvTimeoutError::Timeout) => panic!("something still holds the sink's handle"),
            }
        }
    }

    fn take(writer: &SinkWriter, n: usize) -> Vec<(Modality, String, String)> {
        (0..n)
            .map(|_| {
                shown(
                    writer
                        .recv_timeout(Duration::from_secs(5))
                        .expect("an event within 5 s"),
                )
            })
            .collect()
    }

    fn poll(
        readings: Vec<Option<Foreground>>,
    ) -> (Vec<(Modality, String, String)>, Option<Foreground>) {
        let (writer, reader) = foreground_state_for_test();
        let mut poller = Poller::new(writer, Arc::default());
        let (handle, sink_writer) = sink::channel(64);
        for reading in readings {
            poller.step(reading, Vec::new(), &handle);
        }
        let front = reader.get();
        drop(poller);
        (events(handle, &sink_writer), front)
    }

    fn fg(text: &str) -> (Modality, String, String) {
        (Modality::ForegroundApp, text.to_string(), String::new())
    }

    fn floor_events(writer: &SinkWriter) -> Vec<crate::sink::FloorReceived> {
        std::iter::from_fn(|| writer.try_recv().ok())
            .filter_map(|received| received.into_floor().ok())
            .collect()
    }

    fn titles(writer: &SinkWriter) -> Vec<(String, &'static str, bool)> {
        floor_events(writer)
            .into_iter()
            .filter_map(|floor| match floor.event().observation() {
                crate::floor::payload::Observation::WindowTitle(title) => {
                    let Title { text, why, site } = title;
                    assert!(site.is_none(), "no site reader in R1");
                    Some((
                        crate::floor::cases::stored(text, crate::floor::text::TextField::Title),
                        why.as_str(),
                        text.was_cut(),
                    ))
                }
                _ => None,
            })
            .collect()
    }

    fn kinds(writer: &SinkWriter) -> Vec<Modality> {
        floor_events(writer)
            .iter()
            .map(|floor| floor.event().kind())
            .collect()
    }

    fn read(text: &str) -> TitleRead {
        Ok(Some(TitleText::for_test(text)))
    }

    thread_local! {
        static PROBE_BYTE: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
    }

    fn probe_byte() -> u8 {
        PROBE_BYTE.with(std::cell::Cell::get)
    }

    fn at_console() -> crate::secure_input_macos::Session {
        crate::secure_input_macos::Session::AtConsole
    }

    fn poller_with(floor: FloorHandle) -> (Poller, SinkHandle, SinkWriter) {
        let (writer, _reader) = foreground_state_for_test();
        let mut poller = Poller::new(writer, Arc::default());
        poller.floor = floor;
        poller.probe = SystemProbe::with_readers(at_console, probe_byte);
        let (handle, sink_writer) = sink::channel(64);
        (poller, handle, sink_writer)
    }

    #[test]
    fn a_title_record_holds_its_text_its_cut_and_why_and_no_length() {
        let (mut poller, handle, writer) = poller_with(FloorHandle::default());
        let front = app("com.example.a", 10);
        poller.step(front.clone(), Vec::new(), &handle);
        poller.step(front, answer(1, read("Quarterly plan")), &handle);
        let records: Vec<serde_json::Value> = floor_events(&writer)
            .into_iter()
            .map(|floor| {
                let record =
                    crate::floor::record::encode(&floor, &crate::floor::cases::IdentityPoint)
                        .expect("the encoder writes the record");
                crate::floor::record::decode(record.as_str())
                    .expect("the decoder reads the record");
                serde_json::from_str(record.as_str()).expect("a record is a JSON object")
            })
            .collect();
        let shapes: Vec<(String, String, String)> = records
            .iter()
            .map(|record| {
                (
                    record["kind"].to_string(),
                    record["app"].to_string(),
                    record["p"].to_string(),
                )
            })
            .collect();
        assert_eq!(
            shapes,
            [
                (
                    "\"foreground_app\"".to_string(),
                    "\"com.example.a\"".to_string(),
                    "{}".to_string()
                ),
                (
                    "\"window_title\"".to_string(),
                    "\"com.example.a\"".to_string(),
                    r#"{"cut":false,"text":"Quarterly plan","why":"app_change"}"#.to_string()
                ),
            ]
        );
        drop(poller);
    }

    #[test]
    fn a_title_of_the_poller_passes_the_owners_flush_as_its_text_its_cut_and_why() {
        use crate::daemon::model_store::Harness;
        use crate::daemon::owner::Command;
        use crate::floor::record::DecodedPayload;
        let mut harness = Harness::new();
        harness.ask(Command::ClosePause);
        let (writer, _reader) = foreground_state_for_test();
        let mut poller = Poller::new(writer, Arc::default());
        poller.probe = SystemProbe::with_readers(at_console, probe_byte);
        let front = app("com.example.a", 10);
        poller.step(front.clone(), Vec::new(), &harness.sink);
        poller.step(front, answer(1, read("Quarterly plan")), &harness.sink);
        harness.ask(Command::Settle);
        let records = harness.store.records();
        assert_eq!(
            records
                .iter()
                .map(|record| record.kind())
                .collect::<Vec<_>>(),
            ["foreground_app", "window_title"]
        );
        match records[1].payload() {
            DecodedPayload::WindowTitle {
                text,
                cut,
                why,
                site,
            } => {
                assert_eq!(
                    (text.as_str(), *cut, why.as_str(), site.is_none()),
                    ("Quarterly plan", false, "app_change", true)
                );
            }
            _ => panic!("the second record is no title"),
        }
        assert_eq!(records[1].app(), Some("com.example.a"));
        drop(poller);
    }

    #[test]
    fn a_private_window_whose_title_is_longer_than_the_cap_makes_no_title() {
        let (mut poller, handle, writer) = poller_with(FloorHandle::default());
        let chrome = app("com.google.Chrome", 40);
        poller.step(chrome.clone(), Vec::new(), &handle);
        let long = format!("{} - Google Chrome (Incognito)", "a".repeat(1_100));
        let (_, own) = window_title::NAME_ENDINGS[0];
        let english = TitleText::for_test(&long).of_a_browser_kept_by_its_name(own);
        poller.step(chrome, answer(1, Ok(Some(english))), &handle);
        assert_eq!(
            titles(&writer),
            [],
            "the private ending lies past unit 1,024"
        );
        assert_eq!(
            poller.health.left_out_by_the_name.load(Ordering::Relaxed),
            1
        );
        drop(poller);
    }

    #[test]
    fn a_title_asked_before_an_active_reading_and_answered_after_it_is_not_emitted() {
        let (mut poller, handle, writer) = poller_with(FloorHandle::default());
        let front = app("com.example.a", 10);
        let asked = poller.step(front.clone(), Vec::new(), &handle);
        assert!(
            asked.is_some(),
            "tick 1 reads inactive and asks for the title"
        );
        PROBE_BYTE.with(|byte| byte.set(1));
        let during = poller.step(front.clone(), Vec::new(), &handle);
        assert_eq!(during, None, "tick 2 reads active and asks nothing");
        PROBE_BYTE.with(|byte| byte.set(0));
        poller.step(
            front.clone(),
            answer(1, read("ASKED-BEFORE-ACTIVE")),
            &handle,
        );
        let stored = titles(&writer);
        drop(poller);
        assert_eq!(
            stored,
            [],
            "the answer of a request older than an active reading"
        );
    }

    static INSIDE_BYTE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

    fn inside_byte() -> u8 {
        INSIDE_BYTE.load(Ordering::SeqCst)
    }

    #[test]
    fn a_title_read_inside_an_active_reading_between_two_ticks_is_not_emitted() {
        let (mut poller, handle, writer) = poller_with(FloorHandle::default());
        poller.probe = SystemProbe::with_readers(at_console, inside_byte);
        let front = app("com.example.a", 10);
        let (go, wait) = std::sync::mpsc::channel::<()>();
        let (seen, read_under) = std::sync::mpsc::channel::<u8>();
        let (entered, came) = std::sync::mpsc::channel::<()>();
        let reader: TitleReader = Box::new(move |_pid| {
            entered.send(()).ok();
            wait.recv().ok();
            seen.send(inside_byte()).ok();
            read("READ-WHILE-ACTIVE")
        });
        let (thread, joined) = TitleThread::spawn(
            reader,
            TitleRules::unread(),
            poller.floor.clone(),
            poller.probe,
        )
        .expect("the title thread of the test");
        let request = poller
            .step(front.clone(), thread.answers(), &handle)
            .expect("tick 1 reads inactive and asks for the title");
        thread.ask(request);
        came.recv_timeout(Duration::from_secs(60))
            .expect("the title thread calls its reader");
        INSIDE_BYTE.store(1, Ordering::SeqCst);
        go.send(()).expect("the reader waits for the test");
        let byte_at_the_read = read_under.recv_timeout(Duration::from_secs(60));
        let mut answers = Vec::new();
        for _ in 0..60_000 {
            answers = thread.answers();
            if !answers.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        INSIDE_BYTE.store(0, Ordering::SeqCst);
        let answered = answers.len();
        poller.step(front.clone(), answers, &handle);
        let stored = titles(&writer);
        drop(thread);
        joined.join().expect("the title thread ends by returning");
        drop(poller);
        assert_eq!(byte_at_the_read, Ok(1), "the title was read while active");
        assert_eq!(answered, 1, "the read answered before tick 2");
        assert_eq!(
            stored,
            [],
            "a title that was read while the reading was active"
        );
    }

    fn active_byte() -> u8 {
        1
    }

    fn answers_within(thread: &TitleThread, limit: Duration) -> Vec<TitleAnswer> {
        let started = Instant::now();
        loop {
            let answers = thread.answers();
            if !answers.is_empty() || started.elapsed() >= limit {
                return answers;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn the_title_thread_calls_no_reader_when_the_reading_is_active_before_its_read() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = calls.clone();
        let reader: TitleReader = Box::new(move |_pid| {
            counted.fetch_add(1, Ordering::SeqCst);
            read("NOT-TO-BE-READ")
        });
        let probe = SystemProbe::with_readers(at_console, active_byte);
        let (thread, joined) =
            TitleThread::spawn(reader, TitleRules::unread(), FloorHandle::default(), probe)
                .expect("the title thread of the test");
        thread.ask(TitleRequest {
            front: super::tests_support::front("com.example.a", 10),
            change: 1,
        });
        let answers = answers_within(&thread, Duration::from_secs(60));
        drop(thread);
        joined.join().expect("the title thread ends by returning");
        assert_eq!(
            answers,
            [TitleAnswer {
                change: 1,
                read: Ok(None)
            }]
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "calls of the reader while the reading was active before the read"
        );
    }

    fn reads_within(reader: impl Fn() -> bool, want: bool, limit: Duration) -> bool {
        let started = Instant::now();
        loop {
            if reader() == want {
                return true;
            }
            if started.elapsed() >= limit {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    #[ignore = "takes the system's secure-input hold: a hosted runner runs it with HALVO_HOSTED_TITLE_HOLD=1"]
    fn a_title_read_while_the_system_hold_is_taken_between_two_poller_steps_is_not_stored() {
        const NAME: &str =
            "a_title_read_while_the_system_hold_is_taken_between_two_poller_steps_is_not_stored";
        if std::env::var("HALVO_HOSTED_TITLE_HOLD").as_deref() != Ok("1") {
            println!("{NAME}: HALVO_HOSTED_TITLE_HOLD is not 1, so no hold is taken");
            return;
        }
        if !crate::secure_input_macos::test_support::hosted_or_refuse(NAME) {
            return;
        }
        let one = crate::secure_input_macos::test_support::one_hold_at_a_time();
        let probe = SystemProbe::new();
        let (mut poller, handle, writer) = poller_with(FloorHandle::default());
        poller.probe = probe;
        let front = app("com.example.a", 10);
        let floor = poller.floor.clone();
        let active = || {
            matches!(
                verdict_of::<floor_kind::WindowTitle>(
                    &floor,
                    &probe,
                    &super::tests_support::front("com.example.a", 10),
                    PrivateWindow::Unknown
                ),
                Some(Verdict::Drop(DropReason::SecureInput))
            )
        };
        let (entered, came) = std::sync::mpsc::channel::<()>();
        let (go, wait) = std::sync::mpsc::channel::<()>();
        let reader: TitleReader = Box::new(move |_pid| {
            entered.send(()).ok();
            wait.recv().ok();
            read("READ-UNDER-THE-SYSTEM-HOLD")
        });
        let (thread, joined) =
            TitleThread::spawn(reader, TitleRules::unread(), poller.floor.clone(), probe)
                .expect("the title thread of the test");
        let request = poller
            .step(front.clone(), thread.answers(), &handle)
            .expect("step 1 reads secure input off and asks for the title");
        thread.ask(request);
        came.recv_timeout(Duration::from_secs(60))
            .expect("the title thread calls its reader");
        let Some(hold) = one.hold_or_skip(NAME) else {
            go.send(()).ok();
            drop(thread);
            joined.join().expect("the title thread ends by returning");
            return;
        };
        let on = reads_within(active, true, Duration::from_secs(2));
        go.send(()).expect("the reader waits for the test");
        let answers = answers_within(&thread, Duration::from_secs(60));
        drop(hold);
        let off = reads_within(active, false, Duration::from_secs(2));
        let answered = answers.len();
        poller.step(front.clone(), answers, &handle);
        let stored = titles(&writer);
        drop(thread);
        joined.join().expect("the title thread ends by returning");
        drop(poller);
        assert!(
            on,
            "during the hold the system's probe reads active within 2 s"
        );
        assert!(
            off,
            "after the hold the system's probe reads off within 2 s"
        );
        assert_eq!(answered, 1, "the title thread answered before step 2");
        assert_eq!(
            stored,
            [],
            "a title read while the system's hold was taken, between two steps"
        );
    }

    #[test]
    fn an_ascii_title_longer_than_the_unit_cap_is_stored_with_cut_true() {
        use crate::daemon::model_store::Harness;
        use crate::daemon::owner::Command;
        use crate::floor::record::DecodedPayload;
        let mut harness = Harness::new();
        harness.ask(Command::ClosePause);
        let (writer, _reader) = foreground_state_for_test();
        let mut poller = Poller::new(writer, Arc::default());
        poller.probe = SystemProbe::with_readers(at_console, probe_byte);
        let front = app("com.example.a", 10);
        poller.step(front.clone(), Vec::new(), &harness.sink);
        poller.step(front, answer(1, read(&"a".repeat(2_000))), &harness.sink);
        harness.ask(Command::Settle);
        let stored: Vec<(usize, bool)> = harness
            .store
            .records()
            .iter()
            .filter_map(|record| match record.payload() {
                DecodedPayload::WindowTitle { text, cut, .. } => Some((text.as_str().len(), *cut)),
                _ => None,
            })
            .collect();
        drop(poller);
        assert_eq!(stored.len(), 1, "one title record");
        assert_eq!(stored[0], (0, true));
    }

    #[test]
    fn on_a_mac_no_title_of_a_browsers_window_is_kept_in_english_or_german_while_another_apps_are()
    {
        let visit = [
            "Quarterly plan - Google Docs",
            "New Tab - Google Chrome (Incognito)",
            "Quartalsplan - Google Docs",
            "Neuer Tab - Google Chrome (Inkognito)",
        ];
        for bundle_id in [
            "com.brave.Browser",
            "company.thebrowser.Browser",
            "com.example.editor",
        ] {
            let (mut poller, handle, writer) = poller_with(FloorHandle::default());
            let front = app(bundle_id, 40);
            let asked = poller.step(front.clone(), Vec::new(), &handle);
            for title in visit {
                poller.step(front.clone(), answer(1, read(title)), &handle);
            }
            let made = kinds(&writer);
            drop(poller);
            let titles = made
                .iter()
                .filter(|&&kind| kind == Modality::WindowTitle)
                .count();
            let browser = bundle_id != "com.example.editor";
            assert_eq!(
                (asked.is_some(), made.first().copied(), titles),
                if browser {
                    (false, Some(Modality::ForegroundApp), 0)
                } else {
                    (true, Some(Modality::ForegroundApp), visit.len())
                },
                "{bundle_id}"
            );
        }
    }

    #[test]
    fn readings_a_a_b_of_one_app_give_two_title_events_with_a_and_b() {
        let (mut poller, handle, writer) = poller_with(FloorHandle::default());
        let front = app("com.example.a", 10);
        let first = poller.step(front.clone(), Vec::new(), &handle);
        assert_eq!(
            first,
            Some(TitleRequest {
                front: super::tests_support::front("com.example.a", 10),
                change: 1,
            })
        );
        for text in ["A", "A", "B"] {
            let asked = poller.step(front.clone(), answer(1, read(text)), &handle);
            assert_eq!(asked, first, "the title is asked on every tick");
        }
        assert_eq!(
            kinds(&writer),
            [
                Modality::ForegroundApp,
                Modality::WindowTitle,
                Modality::WindowTitle
            ]
        );
        drop(poller);
    }

    #[test]
    fn a_title_event_holds_its_text_its_cut_and_why_it_was_made() {
        let (mut poller, handle, writer) = poller_with(FloorHandle::default());
        let front = app("com.example.a", 10);
        poller.step(front.clone(), Vec::new(), &handle);
        poller.step(front.clone(), answer(1, read("A")), &handle);
        let long = "\u{e9}".repeat(600);
        poller.step(front.clone(), answer(1, read(&long)), &handle);
        let made = titles(&writer);
        assert_eq!(made.len(), 2);
        assert_eq!(made[0], ("A".to_string(), "app_change", false));
        assert_eq!((made[1].1, made[1].2), ("title_change", true));
        assert_eq!(
            made[1].0,
            "\u{e9}".repeat(512),
            "cut to 1,024 bytes on a character boundary"
        );
    }

    #[test]
    fn with_an_active_probe_no_title_is_asked_and_b_comes_on_the_first_tick_after() {
        let (mut poller, handle, writer) = poller_with(FloorHandle::default());
        let front = app("com.example.a", 10);
        PROBE_BYTE.with(|byte| byte.set(1));
        let asked: Vec<_> = (0..3)
            .map(|_| poller.step(front.clone(), Vec::new(), &handle))
            .collect();
        assert_eq!(
            asked,
            [None, None, None],
            "no read while secure input is active"
        );
        PROBE_BYTE.with(|byte| byte.set(0));
        let request = poller
            .step(front.clone(), Vec::new(), &handle)
            .expect("the first tick after asks for the title");
        assert_eq!(request.front.pid, 10);
        poller.step(front.clone(), answer(request.change, read("B")), &handle);
        assert_eq!(titles(&writer), [("B".to_string(), "app_change", false)]);
        drop(poller);
    }

    #[test]
    fn a_title_read_under_an_active_probe_is_not_emitted() {
        let (mut poller, handle, writer) = poller_with(FloorHandle::default());
        let front = app("com.example.a", 10);
        poller.step(front.clone(), Vec::new(), &handle);
        PROBE_BYTE.with(|byte| byte.set(1));
        poller.step(front.clone(), answer(1, read("A")), &handle);
        PROBE_BYTE.with(|byte| byte.set(0));
        assert_eq!(titles(&writer), []);
        let again = poller
            .step(front.clone(), Vec::new(), &handle)
            .expect("the first tick after asks for the title");
        poller.step(front.clone(), answer(again.change, read("A")), &handle);
        assert_eq!(
            titles(&writer).len(),
            1,
            "the same title once secure input has ended"
        );
        drop(poller);
    }

    #[test]
    fn with_title_text_off_no_title_is_ever_asked() {
        let mut switches = crate::floor::Switches::default();
        switches.set(Switch::TitleText, false);
        let floor = FloorHandle::new(None, Arc::new(crate::floor::exclude::NoLineage), switches);
        let (mut poller, handle, writer) = poller_with(floor);
        let front = app("com.example.a", 10);
        let asked: Vec<_> = (0..3)
            .map(|_| poller.step(front.clone(), Vec::new(), &handle))
            .collect();
        assert_eq!(asked, [None, None, None]);
        assert_eq!(kinds(&writer), [Modality::ForegroundApp]);
        drop(poller);
    }

    struct UnderRoot;

    impl crate::floor::exclude::Lineage for UnderRoot {
        fn parent_of(&self, pid: ProcessId) -> crate::floor::exclude::Parent {
            match pid.get() {
                10 => crate::floor::exclude::Parent::Of(ProcessId::new(500)),
                _ => crate::floor::exclude::Parent::None,
            }
        }
    }

    #[test]
    fn a_listed_app_or_a_process_under_the_root_gives_no_event_and_no_title_read() {
        let rooted = FloorHandle::new(
            Some(ProcessId::new(500)),
            Arc::new(UnderRoot),
            crate::floor::Switches::default(),
        );
        for (floor, front) in [
            (FloorHandle::default(), app("com.apple.Passwords", 20)),
            (rooted.clone(), app("com.example.a", 10)),
        ] {
            let (mut poller, handle, writer) = poller_with(floor);
            let asked: Vec<_> = (0..3)
                .map(|_| poller.step(front.clone(), Vec::new(), &handle))
                .collect();
            assert_eq!(asked, [None, None, None]);
            assert_eq!(kinds(&writer), []);
            drop(poller);
        }
        let (mut poller, handle, writer) = poller_with(rooted);
        let other = app("com.example.b", 30);
        assert!(poller.step(other.clone(), Vec::new(), &handle).is_some());
        poller.step(other, answer(1, read("B")), &handle);
        assert_eq!(
            kinds(&writer),
            [Modality::ForegroundApp, Modality::WindowTitle],
            "a process outside the tree"
        );
        drop(poller);
    }

    #[test]
    fn a_private_window_of_its_browser_makes_no_title_and_counts_the_drop() {
        let (mut poller, handle, writer) = poller_with(FloorHandle::default());
        let chrome = app("com.brave.Browser", 40);
        poller.step(chrome.clone(), Vec::new(), &handle);
        poller.step(
            chrome.clone(),
            answer(1, read("New Tab - Google Chrome (Incognito)")),
            &handle,
        );
        poller.step(
            chrome.clone(),
            answer(1, read("New Tab - Google Chrome (Incognito)")),
            &handle,
        );
        assert_eq!(titles(&writer), []);
        assert_eq!(
            poller.health.left_out_by_the_name.load(Ordering::Relaxed),
            1
        );
        poller.step(
            chrome,
            answer(1, read("Private Browsing tips - Google Chrome")),
            &handle,
        );
        assert_eq!(
            titles(&writer).len(),
            0,
            "no title of a browser's window is kept"
        );
        drop(poller);
    }

    /// A tick of the machine's count, `ticks` after the first of these tests.
    fn at(ticks: u64) -> u64 {
        1_000_000 + ticks
    }

    fn list_of(language: &str) -> Languages {
        Languages::List(vec![language.to_string()])
    }

    /// A stand-in of the system's reads of a browser's language and of the
    /// app in front: the global list and the tick count as the test sets
    /// them, no value of a browser's own, the start of each process as the
    /// test sets it, an app's place /Applications/<its id>.app unless the test
    /// sets another, and as the localization of a bundle the code of the first
    /// language of the list, with each place asked for it noted. It reads
    /// nothing of the system.
    #[derive(Clone)]
    struct SaidLanguage(Arc<Mutex<Said>>);

    struct Said {
        global: Languages,
        clock: u64,
        started: Vec<(libc::pid_t, u64)>,
        places: Vec<(libc::pid_t, String)>,
        asked: Vec<String>,
    }

    impl SaidLanguage {
        fn new(global: Languages) -> Self {
            Self(Arc::new(Mutex::new(Said {
                global,
                clock: 0,
                started: Vec::new(),
                places: Vec::new(),
                asked: Vec::new(),
            })))
        }

        fn said(&self) -> std::sync::MutexGuard<'_, Said> {
            self.0.lock().expect("the stand-in's lock")
        }

        fn set(&self, global: Languages) {
            self.said().global = global;
        }

        fn started(&self, pid: libc::pid_t, at: u64) {
            let mut said = self.said();
            said.started.retain(|(process, _)| *process != pid);
            said.started.push((pid, at));
        }

        fn lies(&self, pid: libc::pid_t, place: &str) {
            self.said().places.push((pid, place.to_string()));
        }
    }

    impl LanguageReads for SaidLanguage {
        fn own(&mut self, _bundle_id: &str) -> Languages {
            Languages::NoValue
        }

        fn global(&mut self) -> Languages {
            self.said().global.clone()
        }

        fn now(&mut self) -> u64 {
            self.said().clock
        }

        fn started(&mut self, pid: libc::pid_t) -> Option<u64> {
            let said = self.said();
            Some(said.started.iter().find(|(process, _)| *process == pid)?.1)
        }

        fn place(&mut self, pid: libc::pid_t, bundle_id: &str) -> Option<String> {
            let said = self.said();
            Some(
                said.places
                    .iter()
                    .find(|(process, _)| *process == pid)
                    .map_or_else(
                        || format!("/Applications/{bundle_id}.app"),
                        |(_, place)| place.clone(),
                    ),
            )
        }

        fn localization(&mut self, place: &str, languages: &[String]) -> Option<String> {
            self.said().asked.push(place.to_string());
            Some(languages.first()?.split('-').next()?.to_string())
        }
    }

    /// A poll with title rules over stand-ins of the system, whose title
    /// thread's answers `answer_of` makes here, on the test's own thread.
    struct Through {
        poller: Poller,
        handle: SinkHandle,
        writer: SinkWriter,
        rules: TitleRules,
        said: SaidLanguage,
        reads: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl Through {
        /// `handlers` stands in for the places of the system's list (`None`:
        /// a failed read), `language` for the reads of a language; the source
        /// began at the tick `began`.
        fn new(handlers: Option<&[&str]>, language: &SaidLanguage, began: u64) -> Self {
            let list: Option<Vec<String>> =
                handlers.map(|places| places.iter().map(|place| place.to_string()).collect());
            let mut rules =
                TitleRules::new(Box::new(move || list.clone()), Box::new(language.clone()));
            language.said().clock = began;
            rules.begin();
            let (poller, handle, writer) = poller_with(FloorHandle::default());
            Self {
                poller,
                handle,
                writer,
                rules,
                said: language.clone(),
                reads: Arc::default(),
            }
        }

        /// The app `front` in front at the tick `now`, its window titled
        /// `title`: the poll that asks for the title, the title thread's
        /// answer, the poll that takes it.
        fn shows(&mut self, front: &Option<Foreground>, title: &str, now: u64) {
            self.said.said().clock = now;
            let text = title.to_string();
            let reads = self.reads.clone();
            let mut reader: TitleReader = Box::new(move |_pid| {
                reads.fetch_add(1, Ordering::SeqCst);
                read(&text)
            });
            let answers = match self.poller.step(front.clone(), Vec::new(), &self.handle) {
                Some(request) => vec![answer_of(
                    &request,
                    &mut self.rules,
                    &mut reader,
                    &self.poller.floor,
                    &self.poller.probe,
                )],
                None => Vec::new(),
            };
            self.poller.step(front.clone(), answers, &self.handle);
        }

        /// The kinds of the events made since the last call, the texts of its
        /// title events, the titles read from a window and the titles that the
        /// rule by the name left out.
        fn made(&self) -> (Vec<Modality>, Vec<String>, usize, u64) {
            let events = floor_events(&self.writer);
            let texts = events
                .iter()
                .filter_map(|floor| match floor.event().observation() {
                    crate::floor::payload::Observation::WindowTitle(title) => {
                        Some(crate::floor::cases::stored(
                            &title.text,
                            crate::floor::text::TextField::Title,
                        ))
                    }
                    _ => None,
                })
                .collect();
            (
                events.iter().map(|floor| floor.event().kind()).collect(),
                texts,
                self.reads.swap(0, Ordering::SeqCst),
                self.poller
                    .health
                    .left_out_by_the_name
                    .swap(0, Ordering::Relaxed),
            )
        }
    }

    #[test]
    fn an_app_that_the_system_lists_for_https_links_keeps_no_title_while_one_in_neither_list_and_thunderbird_keep_theirs(
    ) {
        let visit = ["Quarterly plan - Reader", "New Tab - Reader (Incognito)"];
        let system = [
            "/Applications/com.example.webreader.app/",
            "/Applications/org.mozilla.firefox.app",
        ];
        for (bundle_id, kept) in [
            ("com.example.webreader", &[][..]),
            ("com.example.editor", &visit[..]),
            ("org.mozilla.thunderbird", &visit[..]),
        ] {
            let mut through =
                Through::new(Some(&system), &SaidLanguage::new(list_of("en-US")), at(0));
            let front = app(bundle_id, 40);
            for title in visit {
                through.shows(&front, title, at(10));
            }
            let (_, titles, reads, _) = through.made();
            assert_eq!(
                (titles, reads),
                (
                    kept.iter().map(|title| title.to_string()).collect(),
                    kept.len()
                ),
                "{bundle_id}"
            );
        }
    }

    #[test]
    fn chrome_and_edge_keep_a_normal_title_and_drop_a_private_one_under_english_alone_and_safari_and_a_browser_outside_the_applications_folders_keep_none(
    ) {
        let chrome_normal = "Quarterly plan - Google Docs - Google Chrome";
        let edge_normal = "Quarterly plan - Google Docs - Microsoft Edge";
        let outside = "/Volumes/Stick/Google Chrome.app";
        for (bundle_id, private, place, language, kept, reads, dropped) in [
            (
                "com.google.Chrome",
                "New Tab - Google Chrome (Incognito)",
                None,
                list_of("de-DE"),
                &[][..],
                0,
                0,
            ),
            (
                "com.google.Chrome",
                "New Tab - Google Chrome (Incognito)",
                None,
                list_of("en-US"),
                &[chrome_normal][..],
                2,
                1,
            ),
            (
                "com.google.Chrome",
                "New Tab - Google Chrome (Incognito)",
                None,
                Languages::Failed,
                &[][..],
                0,
                0,
            ),
            (
                "com.microsoft.edgemac",
                "New Tab - Microsoft Edge (InPrivate)",
                None,
                list_of("de-DE"),
                &[][..],
                0,
                0,
            ),
            (
                "com.microsoft.edgemac",
                "New Tab - Microsoft Edge (InPrivate)",
                None,
                list_of("en-US"),
                &[edge_normal][..],
                2,
                1,
            ),
            (
                "com.microsoft.edgemac",
                "New Tab - Microsoft Edge (InPrivate)",
                None,
                Languages::Failed,
                &[][..],
                0,
                0,
            ),
            (
                "com.apple.Safari",
                "Start Page, Private Browsing",
                None,
                list_of("en-US"),
                &[][..],
                0,
                0,
            ),
            (
                "com.google.Chrome",
                "New Tab - Google Chrome (Incognito)",
                Some(outside),
                list_of("en-US"),
                &[][..],
                0,
                0,
            ),
        ] {
            let normal = if bundle_id == "com.microsoft.edgemac" {
                edge_normal
            } else {
                chrome_normal
            };
            let said = SaidLanguage::new(language.clone());
            said.started(40, at(5));
            if let Some(place) = place {
                said.lies(40, place);
            }
            let mut through = Through::new(None, &said, at(0));
            let front = app(bundle_id, 40);
            for title in [normal, private] {
                through.shows(&front, title, at(10));
            }
            let (_, titles, read, left_out) = through.made();
            let asked_outside = said.said().asked.iter().any(|asked| asked == outside);
            assert_eq!(
                (titles, read, left_out, asked_outside),
                (
                    kept.iter().map(|title| title.to_string()).collect(),
                    reads,
                    dropped,
                    false
                ),
                "{bundle_id} at {place:?} under {language:?}"
            );
        }
    }

    #[test]
    fn under_english_firefox_and_another_browser_of_the_closed_list_keep_no_title_and_their_app_is_recorded(
    ) {
        let system = ["/Applications/com.example.webreader.app"];
        for (bundle_id, private) in [
            ("org.mozilla.firefox", "New Tab \u{2014} Private Browsing"),
            ("com.brave.Browser", "New Private Tab - Brave"),
            ("org.torproject.torbrowser", "Tor Browser"),
        ] {
            let said = SaidLanguage::new(list_of("en-US"));
            said.started(40, at(5));
            let mut through = Through::new(Some(&system), &said, at(0));
            let front = app(bundle_id, 40);
            for title in ["Quarterly plan - Google Docs", private] {
                through.shows(&front, title, at(10));
            }
            assert_eq!(
                through.made(),
                (vec![Modality::ForegroundApp], Vec::new(), 0, 0),
                "{bundle_id}"
            );
        }
    }

    #[test]
    fn a_language_changed_after_a_browser_started_keeps_no_title_of_that_process_and_the_next_one_has_its_titles(
    ) {
        let normal = "Quarterly plan - Google Docs - Google Chrome";
        let private = "New Tab - Google Chrome (Incognito)";
        let chrome = |pid| app("com.google.Chrome", pid);

        // A change between a process's start and its first title.
        let said = SaidLanguage::new(list_of("de-DE"));
        let mut through = Through::new(None, &said, at(0));
        said.started(41, at(10));
        said.set(list_of("en-US"));
        for title in [normal, private] {
            through.shows(&chrome(41), title, at(20));
        }
        let (_, changed, _, _) = through.made();
        said.started(42, at(30));
        for title in [normal, private] {
            through.shows(&chrome(42), title, at(40));
        }
        let (_, next, _, dropped) = through.made();
        assert_eq!(
            (changed, next, dropped),
            (Vec::<String>::new(), vec![normal.to_string()], 1)
        );

        // A change and its undoing across a switch of apps, with a process
        // that started between them.
        let said = SaidLanguage::new(list_of("en-US"));
        let mut through = Through::new(None, &said, at(0));
        through.shows(&app("com.example.editor", 10), "Settings", at(10));
        said.set(list_of("de-DE"));
        through.shows(&app("com.example.mail", 11), "Inbox", at(20));
        said.started(43, at(30));
        through.shows(&app("com.example.editor", 10), "Settings", at(40));
        said.set(list_of("en-US"));
        through.shows(&app("com.example.mail", 11), "Inbox", at(50));
        for title in [normal, private] {
            through.shows(&chrome(43), title, at(60));
        }
        let (_, titles, _, _) = through.made();
        assert_eq!(titles, ["Settings", "Inbox", "Settings", "Inbox"]);

        // A later process with the pid of a process decided English.
        let said = SaidLanguage::new(list_of("en-US"));
        let mut through = Through::new(None, &said, at(0));
        said.started(44, at(10));
        through.shows(&chrome(44), normal, at(20));
        said.set(list_of("de-DE"));
        through.shows(&app("com.example.editor", 10), "Settings", at(30));
        said.started(44, at(40));
        for title in [normal, private] {
            through.shows(&chrome(44), title, at(50));
        }
        let (_, titles, _, _) = through.made();
        assert_eq!(titles, [normal, "Settings"]);

        // A start that the ticks put before the source's start.
        let said = SaidLanguage::new(list_of("en-US"));
        said.started(45, at(50));
        let mut through = Through::new(None, &said, at(100));
        for title in [normal, private] {
            through.shows(&chrome(45), title, at(110));
        }
        assert_eq!(through.made().1, Vec::<String>::new());
    }

    #[test]
    fn a_failed_title_read_is_counted_and_asked_again_on_the_next_tick() {
        let (mut poller, handle, _writer) = poller_with(FloorHandle::default());
        let front = app("com.example.a", 10);
        poller.step(front.clone(), Vec::new(), &handle);
        let again = poller.step(
            front,
            answer(1, Err(TitleError::Accessibility { code: -25204 })),
            &handle,
        );
        assert_eq!(
            again,
            Some(TitleRequest {
                front: super::tests_support::front("com.example.a", 10),
                change: 1,
            })
        );
        assert_eq!(poller.health.errors.load(Ordering::Relaxed), 1);
        assert!(poller.health.failing.load(Ordering::Relaxed));
        drop(poller);
    }

    #[test]
    fn a_then_b_then_a_gives_three_foreground_app_events_and_the_reader_ends_at_a() {
        let (events, front) = poll(vec![
            app("com.example.a", 10),
            app("com.example.b", 20),
            app("com.example.a", 10),
        ]);
        assert_eq!(
            events,
            [
                fg("com.example.a"),
                fg("com.example.b"),
                fg("com.example.a")
            ]
        );
        assert_eq!(front, app("com.example.a", 10));
    }

    #[test]
    fn an_app_then_two_unbundled_apps_then_the_app_give_four_events_with_two_paths() {
        let (events, _) = poll(vec![
            app("com.example.a", 10),
            unbundled("/opt/tools/first", 100),
            unbundled("/opt/tools/second", 200),
            app("com.example.a", 10),
        ]);
        assert_eq!(
            events,
            [
                fg("com.example.a"),
                fg("/opt/tools/first"),
                fg("/opt/tools/second"),
                fg("com.example.a")
            ]
        );
    }

    #[test]
    fn an_unbundled_first_app_gives_one_event() {
        let (events, _) = poll(vec![unbundled("/opt/tools/first", 100)]);
        assert_eq!(events, [fg("/opt/tools/first")]);
    }

    #[test]
    fn one_bundle_id_under_two_pids_gives_two_events() {
        let (events, _) = poll(vec![app("com.example.a", 100), app("com.example.a", 200)]);
        assert_eq!(events, [fg("com.example.a"), fg("com.example.a")]);
    }

    #[test]
    fn a_then_nothing_then_a_emits_a_twice_and_ends_at_a() {
        let (events, front) = poll(vec![
            app("com.example.a", 10),
            None,
            app("com.example.a", 10),
        ]);
        assert_eq!(events, [fg("com.example.a"), fg("com.example.a")]);
        assert_eq!(front, app("com.example.a", 10));
        let (_, front) = poll(vec![app("com.example.a", 10), None]);
        assert_eq!(front, None, "nothing in front leaves the value set");
    }

    #[test]
    fn an_unchanged_app_for_five_readings_gives_one_event() {
        let (events, front) = poll((0..5).map(|_| app("com.example.a", 10)).collect());
        assert_eq!(events, [fg("com.example.a")]);
        assert_eq!(front, app("com.example.a", 10));
    }

    #[test]
    fn the_writer_clears_the_value_when_it_drops() {
        let (writer, reader) = foreground_state_for_test();
        writer.set(app("com.example.a", 10));
        drop(writer);
        assert_eq!(reader.get(), None);
    }

    #[test]
    fn a_64_kib_identity_of_multibyte_characters_is_cut_to_1024_bytes_at_a_boundary() {
        let long = "\u{20AC}".repeat(64 * 1024 / 3);
        for identity in [
            AppIdentity::of(Some(&long), None),
            AppIdentity::of(None, Some(&long)),
        ] {
            let text = identity.text();
            assert!(
                text.len() <= IDENTITY_CAP_BYTES && text.len() > IDENTITY_CAP_BYTES - 4,
                "{} bytes",
                text.len()
            );
            assert!(text.chars().all(|c| c == '\u{20AC}'));
        }
    }

    #[test]
    fn an_empty_bundle_id_falls_back_to_the_path_and_an_empty_path_to_unknown() {
        assert_eq!(
            AppIdentity::of(Some(""), Some("/opt/tools/first")).text(),
            "/opt/tools/first"
        );
        assert_eq!(AppIdentity::of(Some(""), Some("")), AppIdentity::Unknown);
        assert_eq!(AppIdentity::of(None, None).text(), "");
    }

    fn answer(change: u64, read: TitleRead) -> Vec<TitleAnswer> {
        vec![TitleAnswer { change, read }]
    }

    fn title(text: &str) -> TitleRead {
        Ok(Some(TitleText::for_test(text)))
    }

    #[test]
    fn three_answers_of_no_window_and_then_a_title_give_exactly_one_title_event() {
        let (writer, _reader) = foreground_state_for_test();
        let health = Arc::new(TitleHealth::default());
        let mut poller = Poller::new(writer, health.clone());
        let (handle, sink_writer) = sink::channel(64);
        let mut asked = vec![poller.step(app("com.example.a", 10), Vec::new(), &handle)];
        for _ in 0..3 {
            asked.push(poller.step(app("com.example.a", 10), answer(1, Ok(None)), &handle));
        }
        asked.push(poller.step(
            app("com.example.a", 10),
            answer(1, title("Inbox (9)")),
            &handle,
        ));
        asked.push(poller.step(
            app("com.example.a", 10),
            answer(1, title("Inbox (9)")),
            &handle,
        ));
        drop(poller);
        let request = Some(TitleRequest {
            front: super::tests_support::front("com.example.a", 10),
            change: 1,
        });
        assert_eq!(asked, vec![request; 6], "the title is asked on every tick");
        assert_eq!(
            events(handle, &sink_writer),
            [
                fg("com.example.a"),
                (
                    Modality::WindowTitle,
                    "com.example.a".into(),
                    "Inbox (9)".into()
                )
            ]
        );
        assert_eq!(health.errors.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn the_lineage_reads_the_parent_of_this_process_and_none_for_launchd_and_unknown_for_no_process(
    ) {
        use crate::floor::exclude::{Lineage, Parent, ProcessId};
        // SAFETY: getppid takes no argument and cannot fail.
        let parent = unsafe { libc::getppid() };
        assert_eq!(
            ProcessLineage.parent_of(ProcessId::new(std::process::id())),
            Parent::Of(ProcessId::new(parent as u32))
        );
        assert_eq!(ProcessLineage.parent_of(ProcessId::new(1)), Parent::None);
        assert_eq!(
            ProcessLineage.parent_of(ProcessId::new(i32::MAX as u32)),
            Parent::Unknown
        );
        assert_eq!(
            ProcessLineage.parent_of(ProcessId::new(u32::MAX)),
            Parent::Unknown
        );
    }

    #[test]
    fn the_predicate_selects_halvo_by_bundle_id_or_by_the_sibling_executable_and_nothing_else() {
        let own_path =
            executable_path(std::process::id() as libc::pid_t).expect("the path of this process");
        let halvo = HalvoApp::with_sibling(&own_path);
        let own = std::process::id() as libc::pid_t;
        let fronts = [
            ("the bundle id", app(HalvoApp::BUNDLE_ID, 70), true),
            ("the sibling by its path", unbundled(&own_path, 71), true),
            (
                "another bundle id with the sibling's process",
                app("com.example.renamed", own),
                true,
            ),
            ("another app", app("com.example.editor", 1), false),
            ("another path", unbundled("/opt/elsewhere/halvo", 72), false),
        ];
        for (what, front, wanted) in fronts {
            let front = front.expect("a front");
            assert_eq!(halvo.selects_front(&front), wanted, "{what}");
        }
        assert!(halvo.selects(HalvoApp::BUNDLE_ID, None));
        assert!(halvo.selects("", Some(&own_path)));
        assert!(!halvo.selects("", None));
        assert!(!halvo.selects("com.example.editor", Some("/opt/elsewhere/halvo")));
        let sibling = HalvoApp::beside_this_executable();
        let beside = std::env::current_exe()
            .expect("the test's own executable")
            .with_file_name("halvo");
        assert!(sibling.selects("", beside.to_str()));
    }

    #[test]
    fn an_unbundled_front_whose_path_is_the_sibling_longer_than_the_cap_is_halvos() {
        fn the_long_sibling(_pid: libc::pid_t) -> Option<String> {
            Some(format!("/{}/halvo", "a".repeat(1040)))
        }
        let long_sibling = format!("/{}/halvo", "a".repeat(1040));
        let halvo = HalvoApp::with_sibling(&long_sibling).reading_paths_by(the_long_sibling);
        let front = unbundled(&long_sibling, 920).expect("a front");
        assert!(long_sibling.len() > IDENTITY_CAP_BYTES);
        assert!(front.identity.text().len() <= IDENTITY_CAP_BYTES);
        assert!(halvo.selects_front(&front), "the long sibling in front");
        let other = unbundled(&format!("/{}/other", "b".repeat(1040)), 921).expect("a front");
        assert!(!halvo.selects_front(&other), "another long path");
    }

    fn cut_pair() -> (String, String) {
        let prefix = format!("/{}", "a".repeat(1023));
        (format!("{prefix}/halvo"), format!("{prefix}/another"))
    }

    fn the_other_program(_pid: libc::pid_t) -> Option<String> {
        Some(cut_pair().1)
    }

    fn the_sibling(_pid: libc::pid_t) -> Option<String> {
        Some(cut_pair().0)
    }

    fn no_path(_pid: libc::pid_t) -> Option<String> {
        None
    }

    #[test]
    fn a_front_whose_path_shares_the_cut_of_the_sibling_is_halvos_only_when_its_whole_path_is_the_sibling(
    ) {
        let prefix = format!("/{}", "a".repeat(1023));
        let (sibling, other) = (format!("{prefix}/halvo"), format!("{prefix}/another"));
        let own = std::process::id() as libc::pid_t;
        let front = unbundled(&other, own).expect("a front");
        assert!(other.len() > IDENTITY_CAP_BYTES);
        assert_eq!(
            CappedText::new(&sibling),
            CappedText::new(&other),
            "the two paths share the cap's cut"
        );
        let halvo = HalvoApp::with_sibling(&sibling);
        assert!(!halvo.selects("", Some(&other)), "the filter");
        assert!(
            !halvo.selects_front(&front),
            "another program in front, whose whole path is this test's own"
        );
    }

    #[test]
    fn a_cut_path_that_equals_the_cut_sibling_is_decided_by_the_whole_path_and_is_halvos_when_unread(
    ) {
        let (sibling, other) = cut_pair();
        let front = unbundled(&other, 72).expect("a front");
        let halvo = HalvoApp::with_sibling(&sibling);
        assert!(
            !halvo
                .clone()
                .reading_paths_by(the_other_program)
                .selects_front(&front),
            "the other program's whole path"
        );
        assert!(
            halvo
                .clone()
                .reading_paths_by(the_sibling)
                .selects_front(&unbundled(&sibling, 73).expect("a front")),
            "the sibling's whole path"
        );
        let unread = halvo.reading_paths_by(no_path);
        assert!(
            unread.selects_front(&front),
            "a cut path that equals the cut sibling, whole path unread"
        );
        let elsewhere = format!("/{}/another", "b".repeat(1023));
        assert!(
            !unread.selects_front(&unbundled(&elsewhere, 74).expect("a front")),
            "a cut path that is not the cut sibling"
        );
        assert!(
            !unread.selects_front(
                &unbundled("/Applications/Other.app/Contents/MacOS/other", 75).expect("a front")
            ),
            "a short path, not the sibling"
        );
        let short = "/Applications/Halvo/halvo";
        assert!(
            HalvoApp::with_sibling(short)
                .reading_paths_by(the_other_program)
                .selects_front(&unbundled(short, 76).expect("a front")),
            "an identity that the cap did not cut is the whole path, read no more"
        );
        let no_sibling = HalvoApp::beside(None).reading_paths_by(the_other_program);
        assert!(
            !no_sibling.selects_front(&front),
            "an app with no sibling took a cut path for its own"
        );
        assert!(
            !no_sibling.selects_front(&unbundled(short, 77).expect("a front")),
            "an app with no sibling took a short path for its own"
        );
    }

    #[test]
    fn a_cut_that_backs_off_inside_a_character_is_decided_by_the_whole_path() {
        fn the_sibling_with_an_e_acute(_pid: libc::pid_t) -> Option<String> {
            Some(format!("/{}\u{e9}/halvo", "a".repeat(1022)))
        }
        let sibling = the_sibling_with_an_e_acute(0).expect("the sibling");
        let front = unbundled(&sibling, 74).expect("a front");
        assert_eq!(sibling.len(), 1031);
        assert_eq!(
            front.identity.text().len(),
            IDENTITY_CAP_BYTES - 1,
            "the cap backs off inside the e acute"
        );
        let halvo = HalvoApp::with_sibling(&sibling).reading_paths_by(the_sibling_with_an_e_acute);
        assert!(
            halvo.selects_front(&front),
            "Halvo's own front, whose identity the cap cut inside a character"
        );
    }

    #[test]
    fn a_sibling_that_is_itself_a_link_is_read_through_the_link() {
        let root = tempfile::tempdir().expect("a temporary directory");
        let dir = root.path().join("dir");
        std::fs::create_dir(&dir).expect("the directory");
        std::fs::write(dir.join("halvo-real"), b"").expect("the file that the link names");
        std::os::unix::fs::symlink(dir.join("halvo-real"), dir.join("halvo"))
            .expect("halvo as a link");
        let halvo = HalvoApp::beside(Some(&dir.join("halvo-capture")));
        let canonical = std::fs::canonicalize(dir.join("halvo-real")).expect("the canonical path");
        assert!(
            halvo.selects("", canonical.to_str()),
            "{:?} against {canonical:?}",
            halvo.sibling
        );
        assert!(
            !halvo.selects("", dir.join("halvo").to_str()),
            "the path of the link itself"
        );
    }

    #[test]
    fn a_sibling_beside_an_executable_in_a_linked_directory_is_the_canonical_path_once_it_exists() {
        let root = tempfile::tempdir().expect("a temporary directory");
        let real = root.path().join("real");
        let link = root.path().join("link");
        std::fs::create_dir(&real).expect("the real directory");
        std::os::unix::fs::symlink(&real, &link).expect("the link to it");
        let halvo = HalvoApp::beside(Some(&link.join("halvo-capture")));
        std::fs::write(real.join("halvo"), b"").expect("the sibling, made after the app");
        let canonical = std::fs::canonicalize(real.join("halvo")).expect("the canonical path");
        assert!(
            halvo.selects("", canonical.to_str()),
            "{:?} against {canonical:?}",
            halvo.sibling
        );
        let through_the_link = link.join("halvo");
        assert_ne!(through_the_link, canonical);
        assert!(!halvo.selects("", through_the_link.to_str()));
    }

    #[test]
    fn an_identity_becomes_the_same_identity_of_a_floor_event() {
        let bundle = AppIdentity::of(Some("com.example.editor"), Some("/x/Editor"));
        assert!(bundle.to_event().is_listed(&["com.example.editor"], &[]));
        let path = AppIdentity::of(None, Some("/opt/tools/Tool"));
        assert!(path.to_event().is_listed(&[], &["Tool"]));
        assert!(AppIdentity::Unknown.to_event().is_unknown());
    }

    #[test]
    fn a_change_and_a_title_seen_after_a_stop_is_asked_are_counted_and_not_emitted() {
        let (writer, _reader) = foreground_state_for_test();
        let health: Arc<TitleHealth> = Arc::default();
        let mut poller = Poller::new(writer, health.clone());
        let (handle, sink_writer) = sink::channel(64);
        poller.step(app("com.example.a", 10), Vec::new(), &handle);
        health.stop_asked.store(true, Ordering::SeqCst);
        poller.step(
            app("com.example.b", 20),
            answer(1, title("Inbox (9)")),
            &handle,
        );
        drop(poller);
        assert_eq!(events(handle, &sink_writer), [fg("com.example.a")]);
        assert_eq!(health.dropped_at_stop.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_shutdown_that_answered_timeout_reads_failed_with_reason_timeout_and_its_drop_waits_nothing(
    ) {
        let (handle, _writer) = sink::channel(64);
        let (release, released) = channel::<()>();
        let read_front: FrontReader = Box::new(move || {
            released.recv_timeout(Duration::from_secs(3)).ok();
            app("a", 1)
        });
        let (mut source, _reader) = source_with(
            FOREGROUND_POLL,
            fresh_turns(),
            read_front,
            Box::new(|_| Ok(None)),
        );
        source
            .begin(SourceContext::new(handle))
            .expect("the source starts");
        let shut = source.shutdown();
        let state = source.health().state;
        let dropped = Instant::now();
        drop(source);
        let drop_took = dropped.elapsed();
        drop(release);
        let timeout = CaptureError::Timeout {
            stage: "foreground_join",
            ms: 1000,
        };
        assert_eq!(shut, Err(timeout.clone()));
        assert_eq!(
            state,
            SourceState::Failed {
                reason: HealthReason::Timeout,
                error: timeout
            }
        );
        assert!(
            drop_took < Duration::from_millis(100),
            "the drop after a Timeout waited {drop_took:?}"
        );
    }

    #[test]
    fn an_answer_for_an_app_no_longer_in_front_is_dropped() {
        let (writer, _reader) = foreground_state_for_test();
        let mut poller = Poller::new(writer, Arc::default());
        let (handle, sink_writer) = sink::channel(64);
        poller.step(app("com.example.a", 10), Vec::new(), &handle);
        poller.step(app("com.example.b", 20), Vec::new(), &handle);
        poller.step(
            app("com.example.b", 20),
            answer(1, title("Inbox (9)")),
            &handle,
        );
        drop(poller);
        assert_eq!(
            events(handle, &sink_writer),
            [fg("com.example.a"), fg("com.example.b")]
        );
    }

    type Script = Arc<Mutex<Vec<Option<Foreground>>>>;

    fn scripted_front(script: Script) -> FrontReader {
        Box::new(move || {
            let mut left = script.lock().unwrap_or_else(PoisonError::into_inner);
            if left.len() > 1 {
                left.remove(0)
            } else {
                left.first().cloned().flatten()
            }
        })
    }

    fn source_with(
        interval: Duration,
        turns: RunLoopTurns,
        read_front: FrontReader,
        read_title: TitleReader,
    ) -> (ForegroundSource, ForegroundReader) {
        let (writer, reader) = foreground_state_for_test();
        (
            ForegroundSource::with_readers(
                interval,
                writer,
                turns,
                read_front,
                read_title,
                TitleRules::unread(),
            ),
            reader,
        )
    }

    fn wait_for(what: &str, done: impl Fn() -> bool) {
        let started = Instant::now();
        while !done() {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "{what} within 5 s"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn a_front_read_while_the_stop_is_asked_makes_no_record_and_is_counted() {
        let (handle, writer) = sink::channel(64);
        let (reading, reads) = channel::<()>();
        let (open, gate) = channel::<()>();
        let calls = Arc::new(AtomicU64::new(0));
        let read_front: FrontReader = {
            let calls = calls.clone();
            Box::new(move || {
                if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    return app("a", 1);
                }
                reading.send(()).ok();
                gate.recv().ok();
                app("b", 2)
            })
        };
        let (mut source, _reader) = source_with(
            Duration::from_millis(1),
            fresh_turns(),
            read_front,
            Box::new(|_| Ok(None)),
        );
        source
            .begin(SourceContext::new(handle.clone()))
            .expect("the source starts without a grant");
        let first = take(&writer, 1);
        reads
            .recv_timeout(Duration::from_secs(5))
            .expect("the second reading began");
        let health = source.health.clone();
        let opener = std::thread::spawn(move || {
            let waited = Instant::now();
            while !health.stop_asked.load(Ordering::SeqCst)
                && waited.elapsed() < Duration::from_millis(500)
            {
                std::thread::yield_now();
            }
            open.send(()).ok();
        });
        let shut = source.shutdown();
        opener.join().expect("the opener ends");
        let dropped = source.health.dropped_at_stop.load(Ordering::SeqCst);
        drop(source);
        assert_eq!(shut, Ok(()));
        assert_eq!(first, [fg("a")]);
        assert_eq!(
            events(handle, &writer),
            [],
            "a record of the front read while the stop was asked"
        );
        assert_eq!(dropped, 1);
    }

    #[test]
    fn a_started_source_emits_on_each_change_of_the_front_app() {
        let (handle, writer) = sink::channel(64);
        let script: Script = Arc::new(Mutex::new(vec![
            app("a", 1),
            app("b", 2),
            app("a", 1),
            app("a", 1),
        ]));
        let (mut source, reader) = source_with(
            Duration::from_millis(1),
            fresh_turns(),
            scripted_front(script.clone()),
            Box::new(|_pid| Ok(None)),
        );
        source
            .begin(SourceContext::new(handle.clone()))
            .expect("the source starts without a grant");
        let first = take(&writer, 3);
        let front = reader.get();
        source.shutdown().expect("the source stops");
        let after = reader.get();
        drop(source);
        assert_eq!(first, [fg("a"), fg("b"), fg("a")]);
        assert_eq!(events(handle, &writer), []);
        assert_eq!(front, app("a", 1));
        assert_eq!(after, None, "the stopped source left its last reading set");
    }

    #[test]
    fn a_second_begin_is_refused_shutdown_returns_within_one_second_and_drop_stops_the_thread() {
        let (handle, _writer) = sink::channel(64);
        let (alive_tx, alive_rx) = channel::<()>();
        let read_front: FrontReader = Box::new(move || {
            let _alive = &alive_tx;
            app("a", 1)
        });
        let (mut source, _reader) = source_with(
            FOREGROUND_POLL,
            fresh_turns(),
            read_front,
            Box::new(|_| Ok(None)),
        );
        let first = source.begin(SourceContext::new(handle.clone()));
        let second = source.begin(SourceContext::new(handle.clone()));
        let running = source.health().state;
        let called = Instant::now();
        let shut = source.shutdown();
        let took = called.elapsed();
        assert_eq!(first, Ok(Started::Running));
        assert_eq!(second, Err(CaptureError::AlreadyStarted));
        assert_eq!(running, SourceState::Running);
        assert!(
            shut == Ok(()) && took <= FOREGROUND_STOP_DEADLINE,
            "shutdown {shut:?} after {took:?}"
        );
        assert_eq!(
            alive_rx.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Disconnected)
        );
        assert_eq!(
            source.health().state,
            SourceState::Stopped(StopCause::Request)
        );

        let (alive_tx, alive_rx) = channel::<()>();
        let read_front: FrontReader = Box::new(move || {
            let _alive = &alive_tx;
            app("a", 1)
        });
        let (mut dropped, _reader) = source_with(
            FOREGROUND_POLL,
            fresh_turns(),
            read_front,
            Box::new(|_| Ok(None)),
        );
        dropped
            .begin(SourceContext::new(handle))
            .expect("the source starts");
        let called = Instant::now();
        drop(dropped);
        assert_eq!(
            alive_rx.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Disconnected),
            "the poll thread still ran when the drop returned"
        );
        assert!(called.elapsed() <= FOREGROUND_STOP_DEADLINE);
    }

    #[test]
    fn a_title_read_that_blocks_past_the_deadline_delays_no_change_and_shutdown_waits_for_it_up_to_its_deadline(
    ) {
        let (handle, writer) = sink::channel(64);
        let switched = Arc::new(AtomicBool::new(false));
        let reading = Arc::new(AtomicBool::new(false));
        let front_switch = switched.clone();
        let read_front: FrontReader = Box::new(move || {
            if front_switch.load(Ordering::SeqCst) {
                app("b", 2)
            } else {
                app("a", 1)
            }
        });
        let title_reading = reading.clone();
        let (release, released) = channel::<()>();
        let read_title: TitleReader = Box::new(move |pid| {
            if pid == 1 {
                title_reading.store(true, Ordering::SeqCst);
                released.recv_timeout(Duration::from_secs(10)).ok();
            }
            Ok(None)
        });
        let (mut source, _reader) =
            source_with(FOREGROUND_POLL, fresh_turns(), read_front, read_title);
        source
            .begin(SourceContext::new(handle.clone()))
            .expect("the source starts");
        wait_for("the title read of the first app", || {
            reading.load(Ordering::SeqCst)
        });
        assert_eq!(
            writer
                .recv_timeout(Duration::from_secs(2))
                .map(|r| shown(r).1),
            Ok("a".to_string())
        );
        switched.store(true, Ordering::SeqCst);
        let changed = Instant::now();
        let next = writer
            .recv_timeout(Duration::from_secs(3))
            .map(|r| shown(r).1);
        let reached = changed.elapsed();
        let called = Instant::now();
        let shut = source.shutdown();
        let stopped_in = called.elapsed();
        release.send(()).ok();
        let again = source.shutdown();
        assert_eq!(next, Ok("b".to_string()));
        assert!(
            reached < FOREGROUND_POLL * 2,
            "the next change reached the sink {reached:?} after it happened, while the title read \
             was still blocked"
        );
        assert_eq!(
            shut,
            Err(CaptureError::Timeout {
                stage: "foreground_join",
                ms: 1000
            })
        );
        assert!(
            stopped_in >= FOREGROUND_STOP_DEADLINE
                && stopped_in < FOREGROUND_STOP_DEADLINE + Duration::from_millis(200),
            "the first shutdown returned after {stopped_in:?}"
        );
        assert_eq!(again, Ok(()), "the shutdown after the read ended");
    }

    #[test]
    fn shutdown_answers_ok_only_after_the_title_thread_left_its_read() {
        let (handle, _writer) = sink::channel(64);
        let (entered_tx, entered_rx) = channel::<()>();
        let (left_tx, left_rx) = channel::<()>();
        let read_title: TitleReader = Box::new(move |_pid| {
            entered_tx.send(()).ok();
            std::thread::sleep(Duration::from_millis(300));
            left_tx.send(()).ok();
            Ok(None)
        });
        let (mut source, _reader) = source_with(
            FOREGROUND_POLL,
            fresh_turns(),
            Box::new(|| app("a", 1)),
            read_title,
        );
        source
            .begin(SourceContext::new(handle))
            .expect("the source starts");
        entered_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("a title read began");
        let called = Instant::now();
        let shut = source.shutdown();
        let took = called.elapsed();
        assert!(
            shut == Ok(()) && took <= FOREGROUND_STOP_DEADLINE,
            "shutdown {shut:?} after {took:?}"
        );
        assert_eq!(
            left_rx.try_recv(),
            Ok(()),
            "shutdown answered Ok while the title thread was inside its read"
        );
    }

    #[test]
    fn shutdown_ends_an_idle_title_thread_within_one_second() {
        let (handle, writer) = sink::channel(64);
        let (mut source, _reader) = source_with(
            FOREGROUND_POLL,
            fresh_turns(),
            Box::new(|| app("a", 1)),
            Box::new(|_| Ok(Some(TitleText::for_test("Inbox")))),
        );
        source
            .begin(SourceContext::new(handle))
            .expect("the source starts");
        let first = take(&writer, 2);
        let called = Instant::now();
        let shut = source.shutdown();
        let took = called.elapsed();
        assert_eq!(
            first,
            [
                fg("a"),
                (Modality::WindowTitle, "a".to_string(), "Inbox".to_string())
            ],
            "the title was read once, so the title thread waits idle"
        );
        assert!(
            shut == Ok(()) && took <= FOREGROUND_STOP_DEADLINE,
            "shutdown {shut:?} after {took:?}"
        );
    }

    #[test]
    fn a_title_read_that_panics_leaves_the_poll_thread_live_and_counts_an_error() {
        let (handle, writer) = sink::channel(64);
        let script: Script = Arc::new(Mutex::new(vec![
            app("a", 1),
            app("b", 2),
            app("c", 3),
            app("d", 4),
        ]));
        let (mut source, _reader) = source_with(
            Duration::from_millis(1),
            fresh_turns(),
            scripted_front(script.clone()),
            Box::new(|_| panic!("scripted panic of a title read")),
        );
        source
            .begin(SourceContext::new(handle.clone()))
            .expect("the source starts");
        let first = take(&writer, 4);
        wait_for("a title error is counted", || {
            source.health().counters.title_read_errors > 0
        });
        let live = source
            .running
            .as_ref()
            .is_some_and(|running| !running.thread.is_finished());
        let state = source.health().state;
        source.shutdown().expect("the source stops");
        drop(source);
        assert!(live, "the poll thread ended after a title read panicked");
        assert_eq!(
            state,
            SourceState::Impaired {
                reason: HealthReason::TitleReadErrors
            }
        );
        assert_eq!(first, [fg("a"), fg("b"), fg("c"), fg("d")]);
        assert_eq!(events(handle, &writer), []);
    }

    #[test]
    fn shutdown_answers_thread_ended_when_the_poll_thread_had_ended() {
        let (handle, _writer) = sink::channel(64);
        let (mut source, _reader) = source_with(
            Duration::from_millis(1),
            fresh_turns(),
            Box::new(|| panic!("scripted panic of the front reader")),
            Box::new(|_| Ok(None)),
        );
        source
            .begin(SourceContext::new(handle))
            .expect("the source starts");
        wait_for("the poll thread ends", || {
            source
                .running
                .as_ref()
                .is_some_and(|running| running.thread.is_finished())
        });
        let state = source.health().state;
        let shut = source.shutdown();
        assert_eq!(
            state,
            SourceState::Failed {
                reason: HealthReason::ThreadEnded,
                error: CaptureError::ThreadEnded
            }
        );
        assert_eq!(shut, Err(CaptureError::ThreadEnded));
        assert_eq!(
            source.health().state,
            state,
            "a poll thread that ended on its own reads as stopped once shutdown ran"
        );
    }

    #[test]
    fn a_missing_grant_reads_impaired_and_counts_no_title_read_error() {
        let (writer, _reader) = foreground_state_for_test();
        let health = Arc::new(TitleHealth::default());
        let mut poller = Poller::new(writer, health.clone());
        let (handle, _sink_writer) = sink::channel(64);
        poller.step(app("com.example.a", 10), Vec::new(), &handle);
        let again = poller.step(
            app("com.example.a", 10),
            answer(1, Err(TitleError::NotTrusted)),
            &handle,
        );
        assert_eq!(
            again,
            Some(TitleRequest {
                front: super::tests_support::front("com.example.a", 10),
                change: 1,
            }),
            "asked again, so a grant given later is read"
        );
        assert_eq!(health.errors.load(Ordering::Relaxed), 0);
        assert!(health.not_trusted.load(Ordering::Relaxed));
        assert!(!health.failing.load(Ordering::Relaxed));
    }

    #[test]
    fn a_stale_stamp_of_the_main_run_loop_reads_impaired() {
        let (handle, _writer) = sink::channel(64);
        let old = Instant::now()
            .checked_sub(Duration::from_secs(3))
            .expect("the clock reads past 3 s");
        let (mut source, _reader) = source_with(
            FOREGROUND_POLL,
            RunLoopTurns::for_test(old),
            Box::new(|| app("a", 1)),
            Box::new(|_| Ok(None)),
        );
        source
            .begin(SourceContext::new(handle))
            .expect("the source starts");
        let state = source.health().state;
        source.shutdown().expect("the source stops");
        assert_eq!(
            state,
            SourceState::Impaired {
                reason: HealthReason::RunLoopStalled
            }
        );
    }

    #[test]
    fn a_failed_thread_start_carries_fixed_text() {
        assert_eq!(
            thread_start_failure(std::io::Error::other("the words of the system")),
            CaptureError::Start("the foreground thread was not started".into())
        );
    }

    // The hosted probe 66-mac-browser-language, which never lands: everything
    // from here to the end of the module.

    /// The titles of the probe's two pages: the first window's and the
    /// private window's.
    const MBL_TITLE: &str = "Halvo probe page";
    const MBL_PRIVATE_TITLE: &str = "Halvo probe private page";

    /// `vnode_fdinfo` of the system's `sys/proc_info.h`, which the libc crate
    /// does not hold: a `proc_fileinfo` of 24 bytes, then the file's
    /// `vnode_info`, whose `vi_stat` holds the device and the inode of the
    /// file. It holds no path.
    #[repr(C)]
    struct MblOpenFile {
        _file: [u64; 3],
        info: libc::vnode_info,
    }

    /// The flavor `PROC_PIDFDVNODEINFO` of that header: the info of an open
    /// file with no path.
    const MBL_FD_VNODE_INFO: libc::c_int = 1;

    const _: () = assert!(std::mem::size_of::<MblOpenFile>() == 176);

    /// The probe's count line: the lines of a state that it printed, and of
    /// them those whose title was read, was refused for the missing grant or
    /// was not there, and the states whose browser did not start.
    #[derive(Default)]
    struct MblCount {
        lines: u32,
        read: u32,
        refused: u32,
        no_title: u32,
        not_started: u32,
    }

    impl MblCount {
        fn title(&mut self, title: &TitleRead) {
            self.lines += 1;
            match title {
                Ok(Some(_)) => self.read += 1,
                Err(TitleError::NotTrusted) => self.refused += 1,
                Ok(None) | Err(_) => self.no_title += 1,
            }
        }
    }

    /// Serves the probe's pages on 127.0.0.1 from a thread of its own and
    /// gives its port: `/private` holds the private window's title, every
    /// other path the first window's.
    fn mbl_serve() -> u16 {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let port = listener.local_addr().expect("the port").port();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(&stream);
                let mut first = String::new();
                if reader.read_line(&mut first).is_err() {
                    continue;
                }
                loop {
                    let mut header = String::new();
                    match reader.read_line(&mut header) {
                        Ok(0) | Err(_) => break,
                        Ok(_) if header.trim().is_empty() => break,
                        Ok(_) => {}
                    }
                }
                let title = if first.contains("/private") {
                    MBL_PRIVATE_TITLE
                } else {
                    MBL_TITLE
                };
                let body = format!(
                    "<!doctype html><html><head><title>{title}</title></head><body><p>probe</p></body></html>"
                );
                let mut stream = &stream;
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .ok();
            }
        });
        port
    }

    /// The version that an app's Info.plist names, or `unread`.
    fn mbl_version(bundle: &str) -> String {
        std::process::Command::new("/usr/bin/plutil")
            .args([
                "-extract",
                "CFBundleShortVersionString",
                "raw",
                "-o",
                "-",
                &format!("{bundle}/Contents/Info.plist"),
            ])
            .output()
            .ok()
            .filter(|out| out.status.success())
            .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
            .unwrap_or_else(|| "unread".to_string())
    }

    /// `text` with every sign written as its code point that is no letter,
    /// digit or blank and no plain sign of ASCII, or that the probe's lines
    /// use to part their fields.
    fn mbl_text(text: &str) -> String {
        text.chars()
            .map(|c| {
                let plain = c.is_alphanumeric()
                    || c == ' '
                    || (c.is_ascii_graphic()
                        && !matches!(c, '"' | '\\' | '|' | '[' | ']' | '{' | '}' | '<' | '>'));
                if plain {
                    c.to_string()
                } else {
                    format!("<U+{:04X}>", u32::from(c))
                }
            })
            .collect()
    }

    fn mbl_yes(yes: bool) -> &'static str {
        if yes {
            "yes"
        } else {
            "no"
        }
    }

    fn mbl_errno() -> i32 {
        std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
    }

    /// Runs the system's `defaults` tool with `args` and says whether it
    /// exited with 0.
    fn mbl_defaults(args: &[&str]) -> bool {
        std::process::Command::new("/usr/bin/defaults")
            .args(args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    /// A language read as the probe prints it.
    fn mbl_languages(read: &Languages) -> String {
        match read {
            Languages::Failed => "failed".to_string(),
            Languages::NoValue => "no_value".to_string(),
            Languages::List(list) => format!("list:{}", mbl_text(&list.join(","))),
        }
    }

    /// What one read of a process's open files gives: how many open files
    /// the list named, the device and the inode of each that is a file of a
    /// disk, and how many info calls did not answer, with the errno of the
    /// last.
    struct MblNumbers {
        fds: usize,
        numbers: Vec<(u32, u64)>,
        unread: u32,
        unread_errno: i32,
    }

    /// The device and the inode of each file of a disk that the process `pid`
    /// holds open, by the list of its open files and then, for each, the call
    /// that gives a file's info with no path: no name of a file is asked for.
    /// `Err` holds the errno of a list call that did not answer.
    fn mbl_numbers(pid: libc::pid_t) -> Result<MblNumbers, i32> {
        let entry = std::mem::size_of::<libc::proc_fdinfo>();
        // SAFETY: with a null buffer of no bytes the call writes nothing and
        // answers the bytes that the list takes.
        let need =
            unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDLISTFDS, 0, std::ptr::null_mut(), 0) };
        if need <= 0 {
            return Err(mbl_errno());
        }
        let room = usize::try_from(need).unwrap_or(0) / entry + 64;
        let mut list = vec![
            libc::proc_fdinfo {
                proc_fd: 0,
                proc_fdtype: 0,
            };
            room
        ];
        let bytes = libc::c_int::try_from(room * entry).unwrap_or(libc::c_int::MAX);
        // SAFETY: `list` is writable for `bytes` bytes, and the call writes at
        // most that many.
        let wrote = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDLISTFDS,
                0,
                list.as_mut_ptr().cast(),
                bytes,
            )
        };
        if wrote <= 0 {
            return Err(mbl_errno());
        }
        list.truncate(usize::try_from(wrote).unwrap_or(0) / entry);
        let size = libc::c_int::try_from(std::mem::size_of::<MblOpenFile>()).expect("a small size");
        let (mut unread, mut unread_errno) = (0, 0);
        let mut numbers = Vec::new();
        for open in &list {
            if i64::from(open.proc_fdtype) != i64::from(libc::PROX_FDTYPE_VNODE) {
                continue;
            }
            let mut info = std::mem::MaybeUninit::<MblOpenFile>::zeroed();
            // SAFETY: `info` is writable for a whole vnode_fdinfo, the struct
            // of this flavor, which is all that the call writes.
            let answer = unsafe {
                libc::proc_pidfdinfo(
                    pid,
                    open.proc_fd,
                    MBL_FD_VNODE_INFO,
                    info.as_mut_ptr().cast(),
                    size,
                )
            };
            if answer != size {
                unread += 1;
                unread_errno = mbl_errno();
                continue;
            }
            // SAFETY: the buffer was zeroed before the call, and every bit
            // pattern is a value of this struct of plain numbers.
            let info = unsafe { info.assume_init() };
            numbers.push((info.info.vi_stat.vst_dev, info.info.vi_stat.vst_ino));
        }
        Ok(MblNumbers {
            fds: list.len(),
            numbers,
            unread,
            unread_errno,
        })
    }

    /// The language files inside the bundle at `place`, read from the disk
    /// itself: the code, the device and the inode of each
    /// `Contents/Frameworks/<a framework>/Versions/<a version>/Resources/<code>.lproj/locale.pak`,
    /// each file once, also where two version folders name it.
    fn mbl_bundle_language_files(place: &str) -> Vec<(String, u64, u64)> {
        use std::os::unix::fs::MetadataExt;
        let entries = |folder: &std::path::Path| -> Vec<std::path::PathBuf> {
            std::fs::read_dir(folder)
                .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
                .unwrap_or_default()
        };
        let mut files: Vec<(String, u64, u64)> = Vec::new();
        let frameworks = std::path::Path::new(place).join("Contents/Frameworks");
        for framework in entries(&frameworks) {
            for version in entries(&framework.join("Versions")) {
                for folder in entries(&version.join("Resources")) {
                    let Some(code) = folder
                        .file_name()
                        .and_then(|name| name.to_str())
                        .and_then(|name| name.strip_suffix(".lproj"))
                    else {
                        continue;
                    };
                    let Ok(file) = std::fs::metadata(folder.join("locale.pak")) else {
                        continue;
                    };
                    let known = files
                        .iter()
                        .any(|(_, dev, ino)| (*dev, *ino) == (file.dev(), file.ino()));
                    if file.is_file() && !known {
                        files.push((code.to_string(), file.dev(), file.ino()));
                    }
                }
            }
        }
        files
    }

    /// One reading of the process `pid` by numbers alone, as the probe prints
    /// it: the open files of a disk, the language files that the disk holds
    /// inside the bundle at `place`, how many of those language files an open
    /// file carries the numbers of (`held`, and `held_by_inode` by the inode
    /// alone), how many open files carry such numbers (`open_with_numbers`),
    /// and the code told: of the one held language file, `none` or `many:`.
    /// No name of an open file is asked for or printed.
    fn mbl_holds(pid: libc::pid_t, place: Option<&str>) -> String {
        let begun = Instant::now();
        let read = mbl_numbers(pid);
        let ms = begun.elapsed().as_millis();
        let MblNumbers {
            fds,
            numbers,
            unread,
            unread_errno,
        } = match read {
            Ok(read) => read,
            Err(errno) => return format!("num{{list=errno:{errno} ms={ms}}}"),
        };
        let begun = Instant::now();
        let bundle = place.map(mbl_bundle_language_files).unwrap_or_default();
        let disk_ms = begun.elapsed().as_millis();
        let (mut held, mut by_inode, mut open_with_numbers) = (Vec::new(), 0, 0);
        for (code, dev, ino) in &bundle {
            let carrying = numbers
                .iter()
                .filter(|(open_dev, open_ino)| u64::from(*open_dev) == *dev && open_ino == ino)
                .count();
            if carrying > 0 {
                held.push(code.as_str());
            }
            open_with_numbers += carrying;
            if numbers.iter().any(|(_, open_ino)| open_ino == ino) {
                by_inode += 1;
            }
        }
        let lang = match held.as_slice() {
            [] => "none".to_string(),
            [one] => mbl_text(one),
            many => format!("many:{}", mbl_text(&many.join(","))),
        };
        format!(
            "num{{list=ok fds={fds} files={} unread={unread} unread_errno={unread_errno} ms={ms} bundle_language_files={} disk_ms={disk_ms} held={} held_by_inode={by_inode} open_with_numbers={open_with_numbers} lang={lang}}}",
            numbers.len(),
            bundle.len(),
            held.len()
        )
    }

    /// The titles that [`window_title::titles_under`] gives, read up to six
    /// times, half a second apart, until the read answers with at least one.
    fn mbl_titles(pid: libc::pid_t, container: Option<&str>, list: &str) -> String {
        let mut last = "unread".to_string();
        for _ in 0..6 {
            let begun = Instant::now();
            let read = window_title::titles_under(pid, container, list);
            let ms = begun.elapsed().as_millis();
            last = match read {
                Ok(titles) if !titles.is_empty() => {
                    let titles: Vec<String> = titles.iter().map(|title| mbl_text(title)).collect();
                    return format!("ok:[{}] ms:{ms}", titles.join("|"));
                }
                Ok(_) => format!("empty ms:{ms}"),
                Err(error) => {
                    let error = format!("{error:?}").replace(' ', "");
                    format!("error:{error} ms:{ms}")
                }
            };
            std::thread::sleep(Duration::from_millis(500));
        }
        last
    }

    /// The title that the capture's own read gives for the browser of `pid`,
    /// read every 250 ms until it holds `wanted` or the read is refused, for
    /// at most 30 s.
    fn mbl_title(pid: libc::pid_t, wanted: &str) -> TitleRead {
        let until = Instant::now() + Duration::from_secs(30);
        loop {
            let read = window_title::focused_window_title(pid);
            let holds = matches!(&read, Ok(Some(title)) if title.text().contains(wanted));
            if holds || matches!(read, Err(TitleError::NotTrusted)) || Instant::now() >= until {
                return read;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    /// A title read as the probe prints it, with what the rule by the
    /// browser's own name does with it.
    fn mbl_title_words(read: &TitleRead, id: &str) -> String {
        match read {
            Ok(Some(title)) => {
                let name_rule = match window_title::listed(id) {
                    Listed::Ending(_, own) => {
                        if title
                            .clone()
                            .of_a_browser_kept_by_its_name(own)
                            .left_out_by_its_browser()
                        {
                            "left_out"
                        } else {
                            "kept"
                        }
                    }
                    Listed::Browser | Listed::Domain | Listed::Other => "not_listed",
                };
                format!(
                    "name_rule={name_rule} title=\"{}\" end64=\"{}\"",
                    mbl_text(title.text()),
                    mbl_text(title.ending())
                )
            }
            Ok(None) => "title=none".to_string(),
            Err(TitleError::NotTrusted) => "title=refused grant=missing".to_string(),
            Err(error) => {
                let error = format!("{error:?}").replace(' ', "");
                format!("title=error:{error}")
            }
        }
    }

    /// What the product's rules say today of the process `pid` of the
    /// browser `id` at the change `change` of the app in front.
    fn mbl_rule(rules: &mut TitleRules, id: &str, pid: libc::pid_t, change: u64) -> &'static str {
        let request = TitleRequest {
            front: tests_support::front(id, pid),
            change,
        };
        match rules.rule_of(&request) {
            TitleRule::NameEnding(_) => "english",
            TitleRule::NoTitle => "not_english",
            TitleRule::Kept => "no_browser",
        }
    }

    /// The app in front as the product reads it.
    fn mbl_front() -> (Option<libc::pid_t>, String) {
        match read_frontmost() {
            Some(front) => {
                let id = match &front.identity {
                    AppIdentity::BundleId(id) => mbl_text(id.as_str()),
                    AppIdentity::ExecutablePath(_) => "no_bundle_id".to_string(),
                    AppIdentity::Unknown => "unknown".to_string(),
                };
                (
                    Some(front.pid),
                    format!("front_pid={} front_id={id}", front.pid),
                )
            }
            None => (None, "front_pid=unread front_id=unread".to_string()),
        }
    }

    /// Starts `binary` on the profile folder `profile` with the probe's
    /// flags, the flags `before` the page `url` and the arguments `after` it.
    fn mbl_start(
        binary: &str,
        profile: &std::path::Path,
        before: &[&str],
        url: &str,
        after: &[&str],
    ) -> std::io::Result<std::process::Child> {
        std::process::Command::new(binary)
            .arg(format!("--user-data-dir={}", profile.display()))
            .args([
                "--no-first-run",
                "--no-default-browser-check",
                "--disable-search-engine-choice-screen",
                "--use-mock-keychain",
                "--new-window",
            ])
            .args(before)
            .arg(url)
            .args(after)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
    }

    /// Ends the process of `child` where it still runs: a TERM, five
    /// seconds, then a KILL.
    fn mbl_end(mut child: std::process::Child) {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        if let Ok(pid) = libc::pid_t::try_from(child.id()) {
            // SAFETY: kill takes plain values; the pid is this test's own
            // child, which has not been waited for.
            unsafe {
                libc::kill(pid, libc::SIGTERM);
            }
        }
        let until = Instant::now() + Duration::from_secs(5);
        while Instant::now() < until {
            if matches!(child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        child.kill().ok();
        child.wait().ok();
    }

    /// The numbers on this test's own process alone: it starts nothing and
    /// reads no other process.
    #[test]
    fn the_numbers_of_a_file_that_this_test_holds_open_are_those_that_the_disk_gives() {
        use std::os::unix::fs::MetadataExt;
        let me = libc::pid_t::try_from(std::process::id()).expect("a pid");
        let app = std::env::temp_dir()
            .join(format!("halvo-mbl-numbers-{me}"))
            .join("Made Browser.app");
        let resources = app
            .join("Contents/Frameworks/Made Browser Framework.framework/Versions/1.2.3/Resources");
        for code in ["de", "en"] {
            let folder = resources.join(format!("{code}.lproj"));
            std::fs::create_dir_all(&folder).expect("the folder");
            std::fs::write(folder.join("locale.pak"), code).expect("the file");
        }
        let place = app.to_str().expect("a path of plain text");
        let german_path = resources.join("de.lproj/locale.pak");
        let none = mbl_holds(me, Some(place));
        let german = std::fs::File::open(&german_path).expect("the German file");
        let one = mbl_holds(me, Some(place));
        let of_the_disk = std::fs::metadata(&german_path).expect("its numbers on the disk");
        let read = mbl_numbers(me).expect("the list of this process");
        let by_the_call = read
            .numbers
            .iter()
            .filter(|(dev, ino)| u64::from(*dev) == of_the_disk.dev() && *ino == of_the_disk.ino())
            .count();
        let english =
            std::fs::File::open(resources.join("en.lproj/locale.pak")).expect("the English file");
        let two = mbl_holds(me, Some(place));
        drop(german);
        drop(english);
        let closed = mbl_holds(me, Some(place));
        println!("MBL own none={none}");
        println!(
            "MBL own one={one} disk_dev={} disk_ino={} open_files_with_these_numbers={by_the_call}",
            of_the_disk.dev(),
            of_the_disk.ino()
        );
        println!("MBL own two={two}");
        println!("MBL own closed={closed}");
        assert!(
            none.contains(" held=0 ") && none.contains("lang=none"),
            "{none}"
        );
        assert_eq!(
            by_the_call, 1,
            "one open file carries the numbers that the disk gives"
        );
        assert!(one.contains(" held=1 ") && one.contains("lang=de"), "{one}");
        assert!(
            two.contains(" held=2 ") && two.contains("lang=many:"),
            "{two}"
        );
        assert!(
            closed.contains(" held=0 ") && closed.contains("lang=none"),
            "{closed}"
        );
    }

    #[test]
    #[ignore = "hosted alone: it starts a real Chrome and a real Edge; the check 66-mac-browser-language runs it"]
    fn a_real_chrome_and_edge_print_the_language_that_each_process_tells() {
        if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
            || std::env::var("HALVO_HOSTED_BROWSER_LANGUAGE").as_deref() != Ok("1")
        {
            return;
        }
        println!();
        let port = mbl_serve();
        let browsers = [
            (
                "chrome",
                "/Applications/Google Chrome.app",
                "Google Chrome",
                "com.google.Chrome",
                "--incognito",
            ),
            (
                "edge",
                "/Applications/Microsoft Edge.app",
                "Microsoft Edge",
                "com.microsoft.edgemac",
                "--inprivate",
            ),
        ];
        let home = std::env::var("HOME").ok();
        let mut count = MblCount::default();
        for (name, bundle, binary_name, id, private_flag) in browsers {
            let version = mbl_version(bundle);
            let binary = format!("{bundle}/Contents/MacOS/{binary_name}");
            for state in ["plain", "argument", "domain"] {
                let mut langs = browser_reads::SystemLanguages;
                let mut prior: Option<Vec<String>> = None;
                let domain = match state {
                    "argument" => format!(
                        "probe_key_written:{}",
                        mbl_yes(mbl_defaults(&[
                            "write",
                            id,
                            "HalvoProbeKey",
                            "-bool",
                            "true"
                        ]))
                    ),
                    "domain" => {
                        if mbl_defaults(&["read", id, "AppleLanguages"]) {
                            if let Languages::List(list) = langs.own(id) {
                                prior = Some(list);
                            }
                        }
                        format!(
                            "AppleLanguages_de_written:{}",
                            mbl_yes(mbl_defaults(&[
                                "write",
                                id,
                                "AppleLanguages",
                                "-array",
                                "de"
                            ]))
                        )
                    }
                    _ => "as_found".to_string(),
                };
                let put_back = |prior: &Option<Vec<String>>| match state {
                    "argument" => mbl_defaults(&["delete", id, "HalvoProbeKey"]),
                    "domain" => match prior {
                        Some(list) => {
                            let mut args = vec!["write", id, "AppleLanguages", "-array"];
                            args.extend(list.iter().map(String::as_str));
                            mbl_defaults(&args)
                        }
                        None => mbl_defaults(&["delete", id, "AppleLanguages"]),
                    },
                    _ => true,
                };
                let mut rules = TitleRules::system();
                rules.begin();
                let profile = std::env::temp_dir().join(format!("halvo-mbl-{name}-{state}"));
                std::fs::create_dir_all(&profile).ok();
                let url = format!("http://127.0.0.1:{port}/page");
                let after: &[&str] = if state == "argument" {
                    &["-AppleLanguages", "(de)"]
                } else {
                    &[]
                };
                let Ok(child) = mbl_start(&binary, &profile, &[], &url, after) else {
                    count.not_started += 1;
                    let back = put_back(&prior);
                    for window in ["", "-private"] {
                        count.lines += 1;
                        println!(
                            "MBL browser={name} version={version} state={state}{window} started=no domain={domain} put_back={}",
                            mbl_yes(back)
                        );
                    }
                    continue;
                };
                let child_pid = libc::pid_t::try_from(child.id()).expect("a pid");
                let mut title = mbl_title(child_pid, MBL_TITLE);
                let (front_pid, front) = mbl_front();
                let pid = match front_pid {
                    Some(front_pid)
                        if front_pid != child_pid && langs.place(front_pid, id).is_some() =>
                    {
                        title = mbl_title(front_pid, MBL_TITLE);
                        front_pid
                    }
                    _ => child_pid,
                };
                let tick = |langs: &mut browser_reads::SystemLanguages| {
                    langs
                        .started(pid)
                        .map_or_else(|| "unread".to_string(), |start| start.to_string())
                };
                let started = tick(&mut langs);
                let place = langs.place(pid, id);
                let at_first = mbl_holds(pid, place.as_deref());
                let menu = mbl_titles(pid, Some("AXMenuBar"), "AXChildren");
                let (own, global) = (langs.own(id), langs.global());
                let list = match (&own, &global) {
                    (Languages::List(list), _) | (Languages::NoValue, Languages::List(list)) => {
                        Some(list.clone())
                    }
                    _ => None,
                };
                let localization = match (&place, &list) {
                    (Some(place), Some(list)) => langs
                        .localization(place, list)
                        .unwrap_or_else(|| "none".to_string()),
                    _ => "unread".to_string(),
                };
                let in_applications = place.as_deref().map_or("unread", |place| {
                    mbl_yes(window_title::in_applications(place, home.as_deref()))
                });
                let rule = mbl_rule(&mut rules, id, pid, 1);
                std::thread::sleep(Duration::from_secs(20));
                let at_20s = mbl_holds(pid, place.as_deref());
                count.title(&title);
                println!(
                    "MBL browser={name} version={version} state={state} pid={pid} child_pid={child_pid} {front} started={started} domain={domain} own={} global={} localization={} in_applications={in_applications} rule_today={rule} {} menu={menu} at_first={at_first} at_20s={at_20s}",
                    mbl_languages(&own),
                    mbl_languages(&global),
                    mbl_text(&localization),
                    mbl_title_words(&title, id),
                );
                let private_url = format!("http://127.0.0.1:{port}/private");
                let second = mbl_start(&binary, &profile, &[private_flag], &private_url, &[]);
                let private_title = mbl_title(pid, MBL_PRIVATE_TITLE);
                let (_, private_front) = mbl_front();
                let at_private = mbl_holds(pid, place.as_deref());
                let private_menu = mbl_titles(pid, Some("AXMenuBar"), "AXChildren");
                let windows = mbl_titles(pid, None, "AXWindows");
                let private_rule = mbl_rule(&mut rules, id, pid, 2);
                let private_started = tick(&mut langs);
                let second_started = second.is_ok();
                if let Ok(second) = second {
                    mbl_end(second);
                }
                mbl_end(child);
                let back = put_back(&prior);
                let own_after = langs.own(id);
                count.title(&private_title);
                println!(
                    "MBL browser={name} version={version} state={state}-private pid={pid} child_pid={child_pid} {private_front} started={private_started} second_start={} rule_today={private_rule} {} windows={windows} menu={private_menu} at_private={at_private} put_back={} own_after={}",
                    mbl_yes(second_started),
                    mbl_title_words(&private_title, id),
                    mbl_yes(back),
                    mbl_languages(&own_after),
                );
            }
        }
        println!("MBL other_user pid=1 {}", mbl_holds(1, None));
        println!(
            "MBL lines={} read={} refused={} no_title={} not_started={}",
            count.lines, count.read, count.refused, count.no_title, count.not_started
        );
        assert_eq!(
            count.refused, 0,
            "the accessibility grant is missing for the test's process"
        );
        assert_eq!(count.not_started, 0, "a browser did not start");
    }
}
