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
use crate::macos::run_loop::RunLoopTurns;
use crate::macos::source_failure;
use crate::macos::window_title::{self, TitleError, TitleText};
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
            ),
            reader,
        )
    }
}

impl Drop for ForegroundWriter {
    fn drop(&mut self) {
        *self.state.lock().unwrap_or_else(PoisonError::into_inner) = FrontState::default();
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
    private_dropped: AtomicU64,
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
        if window == PrivateWindow::Private {
            self.health.private_dropped.fetch_add(1, Ordering::Relaxed);
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

impl TitleThread {
    fn spawn(
        mut read_title: TitleReader,
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
                while let Some(request) = next_request(&shared) {
                    let refused = || {
                        !matches!(
                            verdict_of::<floor_kind::WindowTitle>(
                                &floor,
                                &probe,
                                &request.front,
                                window_of(&request.front),
                            ),
                            Some(Verdict::Keep(_))
                        )
                    };
                    let read = if refused() {
                        Ok(None)
                    } else {
                        let read = catch_unwind(AssertUnwindSafe(|| read_title(request.front.pid)))
                            .unwrap_or(Err(TitleError::ReaderPanicked));
                        if refused() {
                            Ok(None)
                        } else {
                            read
                        }
                    };
                    let answer = TitleAnswer {
                        change: request.change,
                        read,
                    };
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
    readers: Option<(FrontReader, TitleReader, ForegroundWriter)>,
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
    ) -> Self {
        let read_front = foreground.poll_reader();
        Self::with_readers(interval, foreground, turns, read_front, read_title)
    }

    fn with_readers(
        interval: Duration,
        foreground: ForegroundWriter,
        turns: RunLoopTurns,
        read_front: FrontReader,
        read_title: TitleReader,
    ) -> Self {
        Self {
            interval,
            turns,
            readers: Some((read_front, read_title, foreground)),
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
        let Some((read_front, read_title, writer)) = self.readers.take() else {
            return Err(CaptureError::Start(
                "the foreground source starts once".into(),
            ));
        };
        let mut poller = Poller::new(writer, self.health.clone());
        poller.floor = ctx.floor().clone();
        let (stop, stop_rx) = channel();
        let sink = ctx.sink().clone();
        let interval = self.interval;
        let started = TitleThread::spawn(read_title, poller.floor.clone(), poller.probe).and_then(
            |(titles, title)| {
                std::thread::Builder::new()
                    .name("halvo-foreground".into())
                    .spawn(move || poll_loop(read_front, poller, titles, sink, interval, stop_rx))
                    .map(|thread| (thread, title))
                    .map_err(thread_start_failure)
            },
        );
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
        poller.step(chrome, answer(1, read(&long)), &handle);
        assert_eq!(
            titles(&writer),
            [],
            "the private ending lies past unit 1,024"
        );
        assert_eq!(poller.health.private_dropped.load(Ordering::Relaxed), 1);
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
        let (thread, joined) = TitleThread::spawn(reader, poller.floor.clone(), poller.probe)
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
        let (thread, joined) = TitleThread::spawn(reader, FloorHandle::default(), probe)
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
        let (thread, joined) = TitleThread::spawn(reader, poller.floor.clone(), probe)
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
            "com.google.Chrome",
            "com.microsoft.edgemac",
            "org.mozilla.firefox",
            "com.apple.Safari",
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
        let chrome = app("com.google.Chrome", 40);
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
        assert_eq!(poller.health.private_dropped.load(Ordering::Relaxed), 1);
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
            ForegroundSource::with_readers(interval, writer, turns, read_front, read_title),
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
}
