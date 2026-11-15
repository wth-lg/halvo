use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

use objc2::rc::autoreleasepool;
use objc2_app_kit::NSWorkspace;
use uuid::Uuid;

use crate::event::{CaptureStreamEvent, CapturedAt, Modality, SegmentKeyId, SegmentRef};
use crate::macos::run_loop::RunLoopTurns;
use crate::macos::source_failure;
use crate::macos::window_title::{self, TitleError, TitleLength};
use crate::sink::{SinkError, SinkHandle};
use crate::source::{
    CaptureError, CaptureSource, EmitOutcome, EventSink, HealthReason, SourceContext,
    SourceCounters, SourceHealth, SourceState, Started, StopCause,
};

pub const FOREGROUND_POLL: Duration = Duration::from_millis(400);

pub const FOREGROUND_STOP_DEADLINE: Duration = Duration::from_secs(1);

pub const TITLE_RETRY_WINDOW: Duration = FOREGROUND_POLL.saturating_mul(5);

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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Foreground {
    pub identity: AppIdentity,
    pub pid: libc::pid_t,
}

/// The write side of the app in front.
///  Dropping it clears the value.
#[derive(Debug)]
pub struct ForegroundWriter(Arc<Mutex<Option<Foreground>>>);

#[derive(Debug, Clone)]
pub struct ForegroundReader(Arc<Mutex<Option<Foreground>>>);

pub(crate) fn foreground_state(
    _one: crate::macos::run_loop::OneWriter,
) -> (ForegroundWriter, ForegroundReader) {
    writer_and_reader()
}

fn writer_and_reader() -> (ForegroundWriter, ForegroundReader) {
    let shared = Arc::new(Mutex::new(None));
    (ForegroundWriter(shared.clone()), ForegroundReader(shared))
}

#[cfg(test)]
pub(crate) fn foreground_state_for_test() -> (ForegroundWriter, ForegroundReader) {
    writer_and_reader()
}

impl ForegroundWriter {
    fn set(&self, front: Option<Foreground>) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = front;
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
}

impl Drop for ForegroundWriter {
    fn drop(&mut self) {
        self.set(None);
    }
}

impl ForegroundReader {
    /// The app in front, or `None` before the first reading and while nothing is
    /// in front.
    pub fn get(&self) -> Option<Foreground> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unpersisted {
    pub byte_len: u64,
}

impl Unpersisted {
    pub const EMPTY: Self = Self { byte_len: 0 };

    pub fn placement(self) -> (SegmentRef, SegmentKeyId) {
        (
            SegmentRef {
                segment_id: Uuid::nil(),
                byte_offset: 0,
                byte_len: self.byte_len,
            },
            SegmentKeyId(0),
        )
    }
}

pub(crate) fn emit_metadata_only(
    sink: &SinkHandle,
    modality: Modality,
    foreground: Option<Foreground>,
    placement: Unpersisted,
) -> Result<EmitOutcome, SinkError> {
    let foreground_app = foreground
        .map(|front| front.identity.text().to_owned())
        .unwrap_or_default();
    let (segment_ref, segment_key_id) = placement.placement();
    sink.emit(CaptureStreamEvent::new_raw(
        CapturedAt {
            wall_clock: SystemTime::now(),
            sequence: sink.next_sequence(),
        },
        modality,
        foreground_app,
        segment_ref,
        segment_key_id,
    ))
}

fn executable_path(pid: libc::pid_t) -> Option<String> {
    let mut buffer = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    // SAFETY: `buffer` is writable for the size passed, and proc_pidpath writes
    // at most that many bytes and returns how many it wrote.
    let written =
        unsafe { libc::proc_pidpath(pid, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
    let written = usize::try_from(written).ok().filter(|&n| n > 0)?;
    buffer.truncate(written.min(buffer.len()));
    Some(String::from_utf8_lossy(&buffer).into_owned())
}

fn bundle_identifier(text: &objc2_foundation::NSString) -> String {
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
type TitleRead = Result<Option<TitleLength>, TitleError>;
type TitleReader = Box<dyn FnMut(libc::pid_t) -> TitleRead + Send>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TitleRequest {
    pid: libc::pid_t,
    change: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TitleAnswer {
    change: u64,
    read: TitleRead,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TitleState {
    Asked,
    AskAgain,
    Settled,
}

#[derive(Default)]
struct TitleHealth {
    errors: AtomicU64,
    failing: AtomicBool,
    not_trusted: AtomicBool,
}

fn retry_ticks(interval: Duration) -> u32 {
    let interval = interval.as_millis().max(1);
    u32::try_from(TITLE_RETRY_WINDOW.as_millis() / interval).unwrap_or(u32::MAX)
}

struct Poller {
    writer: ForegroundWriter,
    front: Option<Foreground>,
    change: u64,
    title: TitleState,
    retries_left: u32,
    retry_ticks: u32,
    health: Arc<TitleHealth>,
}

impl Poller {
    fn new(writer: ForegroundWriter, retry_ticks: u32, health: Arc<TitleHealth>) -> Self {
        Self {
            writer,
            front: None,
            change: 0,
            title: TitleState::Settled,
            retries_left: 0,
            retry_ticks,
            health,
        }
    }

    fn take_answer(&mut self, answer: TitleAnswer, sink: &SinkHandle) {
        if answer.change != self.change || self.title != TitleState::Asked {
            return;
        }
        match answer.read {
            Ok(Some(title)) => {
                self.title = TitleState::Settled;
                self.health.failing.store(false, Ordering::Relaxed);
                self.health.not_trusted.store(false, Ordering::Relaxed);
                let _ = emit_metadata_only(
                    sink,
                    Modality::WindowTitle,
                    self.front.clone(),
                    Unpersisted {
                        byte_len: title.bytes(),
                    },
                );
            }
            Ok(None) if self.retries_left > 0 => {
                self.retries_left -= 1;
                self.title = TitleState::AskAgain;
            }
            Ok(None) => self.title = TitleState::Settled,
            Err(TitleError::NotTrusted) => {
                self.title = TitleState::Settled;
                self.health.not_trusted.store(true, Ordering::Relaxed);
            }
            Err(_) => {
                self.title = TitleState::Settled;
                self.health.errors.fetch_add(1, Ordering::Relaxed);
                self.health.failing.store(true, Ordering::Relaxed);
            }
        }
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
                self.title = TitleState::Settled;
                self.writer.set(None);
            }
            return None;
        };
        if self.front.as_ref().map(|known| known.pid) != Some(front.pid) {
            self.change += 1;
            self.front = Some(front.clone());
            self.writer.set(Some(front.clone()));
            let _ = emit_metadata_only(
                sink,
                Modality::ForegroundApp,
                Some(front.clone()),
                Unpersisted::EMPTY,
            );
            self.title = TitleState::Asked;
            self.retries_left = self.retry_ticks;
        } else if self.title == TitleState::AskAgain {
            self.title = TitleState::Asked;
        } else {
            return None;
        }
        Some(TitleRequest {
            pid: front.pid,
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
    fn spawn(mut read_title: TitleReader) -> Result<(Self, JoinHandle<()>), CaptureError> {
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
                    let read = catch_unwind(AssertUnwindSafe(|| read_title(request.pid)))
                        .unwrap_or(Err(TitleError::ReaderPanicked));
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
    stopped: bool,
}

impl ForegroundSource {
    pub fn new(interval: Duration, foreground: ForegroundWriter, turns: RunLoopTurns) -> Self {
        Self::with_readers(
            interval,
            foreground,
            turns,
            Box::new(read_frontmost),
            Box::new(window_title::focused_window_title),
        )
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
            stopped: false,
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
        let poller = Poller::new(writer, retry_ticks(self.interval), self.health.clone());
        let (stop, stop_rx) = channel();
        let sink = ctx.sink().clone();
        let interval = self.interval;
        let started = TitleThread::spawn(read_title).and_then(|(titles, title)| {
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
            return Ok(());
        };
        let ended_before = !running.stop_sent && running.thread.is_finished();
        let _ = running.stop.send(());
        running.stop_sent = true;
        let called = Instant::now();
        while !running.ended() {
            if called.elapsed() >= FOREGROUND_STOP_DEADLINE {
                return Err(CaptureError::Timeout {
                    stage: "foreground_join",
                    ms: FOREGROUND_STOP_DEADLINE.as_millis() as u64,
                });
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        let joined = self
            .running
            .take()
            .map(|running| (running.thread.join(), running.title.join()));
        if let (false, Some((Ok(()), Ok(())))) = (ended_before, joined) {
            self.stopped = true;
            Ok(())
        } else {
            self.failure = Some(CaptureError::ThreadEnded);
            Err(CaptureError::ThreadEnded)
        }
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
        SourceHealth {
            state,
            counters: SourceCounters {
                title_read_errors: self.health.errors.load(Ordering::Relaxed),
                ..SourceCounters::default()
            },
        }
    }
}

impl Drop for ForegroundSource {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

    fn events(handle: SinkHandle, writer: &SinkWriter) -> Vec<(Modality, String, u64)> {
        drop(handle);
        let mut kinds = Vec::new();
        loop {
            match writer.recv_timeout(Duration::from_secs(2)) {
                Ok(received) => {
                    let event = received.event();
                    kinds.push((
                        event.modality,
                        event.foreground_app.clone(),
                        event.segment_ref.byte_len,
                    ));
                }
                Err(RecvTimeoutError::Disconnected) => return kinds,
                Err(RecvTimeoutError::Timeout) => panic!("something still holds the sink's handle"),
            }
        }
    }

    fn take(writer: &SinkWriter, n: usize) -> Vec<(Modality, String, u64)> {
        (0..n)
            .map(|_| {
                let received = writer
                    .recv_timeout(Duration::from_secs(5))
                    .expect("an event within 5 s");
                let event = received.event();
                (
                    event.modality,
                    event.foreground_app.clone(),
                    event.segment_ref.byte_len,
                )
            })
            .collect()
    }

    fn poll(
        readings: Vec<Option<Foreground>>,
    ) -> (Vec<(Modality, String, u64)>, Option<Foreground>) {
        let (writer, reader) = foreground_state_for_test();
        let mut poller = Poller::new(writer, 5, Arc::default());
        let (handle, sink_writer) = sink::channel(64);
        for reading in readings {
            poller.step(reading, Vec::new(), &handle);
        }
        let front = reader.get();
        drop(poller);
        (events(handle, &sink_writer), front)
    }

    fn fg(text: &str) -> (Modality, String, u64) {
        (Modality::ForegroundApp, text.to_string(), 0)
    }

    #[test]
    fn unpersisted_placement_is_the_nil_sentinel() {
        let (seg, key) = Unpersisted { byte_len: 128 }.placement();
        assert!(seg.segment_id.is_nil());
        assert_eq!(seg.byte_len, 128);
        assert_eq!(key.0, 0);
        assert_eq!(Unpersisted::EMPTY.placement().0.byte_len, 0);
    }

    #[test]
    fn a_metadata_only_event_carries_the_identity_and_no_persisted_bytes() {
        let (handle, writer) = sink::channel(2);
        emit_metadata_only(
            &handle,
            Modality::WindowTitle,
            app("com.apple.Terminal", 7),
            Unpersisted { byte_len: 7 },
        )
        .unwrap();
        emit_metadata_only(&handle, Modality::Pointer, None, Unpersisted::EMPTY).unwrap();
        assert_eq!(
            events(handle, &writer),
            [
                (Modality::WindowTitle, "com.apple.Terminal".to_string(), 7),
                (Modality::Pointer, String::new(), 0)
            ]
        );
    }

    #[test]
    fn two_emits_carry_increasing_sequences_and_wall_clocks_between_the_calls() {
        let (handle, writer) = sink::channel(2);
        let before = SystemTime::now();
        for modality in [Modality::ForegroundApp, Modality::WindowTitle] {
            emit_metadata_only(&handle, modality, None, Unpersisted::EMPTY).unwrap();
        }
        let after = SystemTime::now();
        drop(handle);
        let stamps: Vec<CapturedAt> = std::iter::from_fn(|| writer.recv())
            .map(|event| event.captured_at)
            .collect();
        assert_eq!(stamps.len(), 2);
        assert!(
            stamps[0].sequence < stamps[1].sequence,
            "sequences {:?} then {:?}",
            stamps[0].sequence,
            stamps[1].sequence
        );
        for stamp in &stamps {
            assert!(
                before <= stamp.wall_clock && stamp.wall_clock <= after,
                "wall clock {:?} outside {before:?} to {after:?}",
                stamp.wall_clock
            );
        }
        assert!(stamps[0].wall_clock <= stamps[1].wall_clock);
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

    fn title(bytes: u64) -> TitleRead {
        Ok(Some(TitleLength::for_test(bytes)))
    }

    #[test]
    fn three_answers_of_no_window_and_then_a_title_give_exactly_one_title_event() {
        let (writer, _reader) = foreground_state_for_test();
        let health = Arc::new(TitleHealth::default());
        let mut poller = Poller::new(writer, 5, health.clone());
        let (handle, sink_writer) = sink::channel(64);
        let mut asked = vec![poller.step(app("com.example.a", 10), Vec::new(), &handle)];
        for _ in 0..3 {
            asked.push(poller.step(app("com.example.a", 10), answer(1, Ok(None)), &handle));
        }
        asked.push(poller.step(app("com.example.a", 10), answer(1, title(9)), &handle));
        asked.push(poller.step(app("com.example.a", 10), answer(1, title(9)), &handle));
        drop(poller);
        let request = Some(TitleRequest { pid: 10, change: 1 });
        assert_eq!(asked, [request, request, request, request, None, None]);
        assert_eq!(
            events(handle, &sink_writer),
            [
                fg("com.example.a"),
                (Modality::WindowTitle, "com.example.a".into(), 9)
            ]
        );
        assert_eq!(health.errors.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn the_poller_asks_again_for_five_ticks_and_then_stops() {
        assert_eq!(retry_ticks(FOREGROUND_POLL), 5);
        assert_eq!(TITLE_RETRY_WINDOW, Duration::from_millis(2000));
        let (writer, _reader) = foreground_state_for_test();
        let mut poller = Poller::new(writer, retry_ticks(FOREGROUND_POLL), Arc::default());
        let (handle, _sink_writer) = sink::channel(64);
        poller.step(app("com.example.a", 10), Vec::new(), &handle);
        let asked = (0..7)
            .filter(|_| {
                poller
                    .step(app("com.example.a", 10), answer(1, Ok(None)), &handle)
                    .is_some()
            })
            .count();
        assert_eq!(asked, 5);
    }

    #[test]
    fn an_accessibility_error_is_counted_and_not_asked_again() {
        let (writer, _reader) = foreground_state_for_test();
        let health = Arc::new(TitleHealth::default());
        let mut poller = Poller::new(writer, 5, health.clone());
        let (handle, _sink_writer) = sink::channel(64);
        poller.step(app("com.example.a", 10), Vec::new(), &handle);
        let again = poller.step(
            app("com.example.a", 10),
            answer(1, Err(TitleError::Accessibility { code: -25204 })),
            &handle,
        );
        assert_eq!(again, None);
        assert_eq!(health.errors.load(Ordering::Relaxed), 1);
        assert!(health.failing.load(Ordering::Relaxed));
    }

    #[test]
    fn an_answer_for_an_app_no_longer_in_front_is_dropped() {
        let (writer, _reader) = foreground_state_for_test();
        let mut poller = Poller::new(writer, 5, Arc::default());
        let (handle, sink_writer) = sink::channel(64);
        poller.step(app("com.example.a", 10), Vec::new(), &handle);
        poller.step(app("com.example.b", 20), Vec::new(), &handle);
        poller.step(app("com.example.b", 20), answer(1, title(9)), &handle);
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
                let _ = released.recv_timeout(Duration::from_secs(10));
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
                .map(|r| r.event().foreground_app.clone()),
            Ok("a".to_string())
        );
        switched.store(true, Ordering::SeqCst);
        let changed = Instant::now();
        let next = writer
            .recv_timeout(Duration::from_secs(3))
            .map(|r| r.event().foreground_app.clone());
        let reached = changed.elapsed();
        let called = Instant::now();
        let shut = source.shutdown();
        let stopped_in = called.elapsed();
        let _ = release.send(());
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
            let _ = entered_tx.send(());
            std::thread::sleep(Duration::from_millis(300));
            let _ = left_tx.send(());
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
            Box::new(|_| Ok(Some(TitleLength::for_test(5)))),
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
            [fg("a"), (Modality::WindowTitle, "a".to_string(), 5)],
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
        let mut poller = Poller::new(writer, 5, health.clone());
        let (handle, _sink_writer) = sink::channel(64);
        poller.step(app("com.example.a", 10), Vec::new(), &handle);
        let again = poller.step(
            app("com.example.a", 10),
            answer(1, Err(TitleError::NotTrusted)),
            &handle,
        );
        assert_eq!(again, None);
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
