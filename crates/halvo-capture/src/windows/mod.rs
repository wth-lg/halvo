//! Windows capture backend.
#![deny(clippy::undocumented_unsafe_blocks)]

pub mod apps;
pub mod audio;
pub mod exclusion;
pub mod foreground;
pub mod input;
pub mod screen;
pub mod window_title;

use crate::daemon::{DaemonError, TimedRunReport};
use crate::event::{AppIdentity, FloorEvent, FoldEvent, Modality};
use crate::floor::exclude::{
    DropReason, FloorHandle, Keep, PrivateWindow, ProcessId, Subject, Verdict,
};
use crate::floor::kind::Recorded;
use crate::floor::payload::PointerFold;
use crate::floor::Switch;
use crate::secure_input::windows::PasswordFieldProbe;
use crate::sink::{Emitted, FoldEmitted, SinkHandle};
use crate::source::{
    CaptureError, HealthReason, SourceContext, SourceCounters, SourceHealth, SourceState, Started,
    StopCause,
};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

#[link(name = "kernel32")]
extern "system" {
    fn GetTickCount() -> u32;
}

pub(crate) fn now_tick() -> u32 {
    // SAFETY: GetTickCount takes no argument and reads a counter.
    unsafe { GetTickCount() }
}

/// The time of an OS event, in the milliseconds of GetTickCount.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OsTick(u32);

impl OsTick {
    pub(crate) const fn new(tick: u32) -> OsTick {
        OsTick(tick)
    }

    /// The age of the event at the tick `now`; a tick after `now` is no age.
    pub(crate) fn age_at(self, now: u32) -> Duration {
        let age = now.wrapping_sub(self.0);
        if age > i32::MAX as u32 {
            Duration::ZERO
        } else {
            Duration::from_millis(u64::from(age))
        }
    }

    /// The age of the event now, which the floor constructors take.
    pub(crate) fn age(self) -> Option<Duration> {
        Some(self.age_at(now_tick()))
    }
}

/// The process that an event belongs to and the path of its executable, when
/// it could be read.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FrontApp {
    pub(crate) pid: u32,
    pub(crate) path: Option<String>,
}

impl FrontApp {
    pub(crate) fn subject(&self) -> Subject {
        let app = self
            .path
            .clone()
            .map_or_else(AppIdentity::unknown, AppIdentity::executable_path);
        Subject::new(app, ProcessId::new(self.pid))
    }
}

/// The events that a source dropped at the floor's decision, one count for
/// each reason; it holds no content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Drops {
    pub switch_off: u64,
    pub listed_app: u64,
    pub exclusion_tree: u64,
    pub unknown_lineage: u64,
    pub private_window: u64,
    pub secure_input: u64,
}

#[derive(Debug, Default)]
pub(crate) struct DropCounts([AtomicU64; 6]);

impl DropCounts {
    const fn index(reason: DropReason) -> usize {
        match reason {
            DropReason::SwitchOff => 0,
            DropReason::ListedApp => 1,
            DropReason::ExclusionTree => 2,
            DropReason::UnknownLineage => 3,
            DropReason::PrivateWindow => 4,
            DropReason::SecureInput => 5,
        }
    }

    pub(crate) fn count(&self, reason: DropReason) {
        self.0[Self::index(reason)].fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn read(&self) -> Drops {
        let at = |reason| self.0[Self::index(reason)].load(Ordering::Relaxed);
        Drops {
            switch_off: at(DropReason::SwitchOff),
            listed_app: at(DropReason::ListedApp),
            exclusion_tree: at(DropReason::ExclusionTree),
            unknown_lineage: at(DropReason::UnknownLineage),
            private_window: at(DropReason::PrivateWindow),
            secure_input: at(DropReason::SecureInput),
        }
    }
}

/// Whether a source cannot place `front` against the rules: a process whose
/// image it could not read, which no list entry and no tree rule can match,
/// gives no event of any kind, and with no app known to be in front a screen
/// frame is dropped (contracts.md C13.7). Such a drop is counted under
/// unknown_lineage.
fn unplaced<K: Recorded>(front: &FrontApp) -> bool {
    front.path.is_none() && (front.pid != 0 || K::KIND == Modality::ScreenFrame)
}

/// The floor's decision as a source asks it: the keep verdict of one event, or
/// a drop that is counted under its reason.
#[derive(Clone)]
pub(crate) struct Floor {
    handle: FloorHandle,
    drops: Arc<DropCounts>,
}

impl Floor {
    pub(crate) fn new(handle: FloorHandle, drops: Arc<DropCounts>) -> Floor {
        Floor { handle, drops }
    }

    pub(crate) fn is_on(&self, switch: Switch) -> bool {
        self.handle.switches().is_on(switch)
    }

    pub(crate) fn keep<K: Recorded>(
        &self,
        front: &FrontApp,
        window: PrivateWindow,
        probe: &PasswordFieldProbe,
    ) -> Option<Keep<K>> {
        if unplaced::<K>(front) {
            self.drops.count(DropReason::UnknownLineage);
            return None;
        }
        match self.handle.decide::<K>(front.subject(), window, probe) {
            Verdict::Keep(keep) => Some(keep),
            Verdict::Drop(reason) => {
                self.drops.count(reason);
                None
            }
        }
    }
}

impl Floor {
    /// The decision on a value built after its events, such as a fold: a
    /// fresh decision, except that an unknown lineage keeps `held`, the
    /// decision that the last event got when it came, since its process may
    /// have ended since.
    pub(crate) fn keep_built_later<K: Recorded>(
        &self,
        front: &FrontApp,
        held: Option<Keep<K>>,
    ) -> Option<Keep<K>> {
        if unplaced::<K>(front) {
            self.drops.count(DropReason::UnknownLineage);
            return None;
        }
        match self.handle.decide::<K>(
            front.subject(),
            PrivateWindow::Unknown,
            &PasswordFieldProbe::default(),
        ) {
            Verdict::Keep(keep) => Some(keep),
            Verdict::Drop(DropReason::UnknownLineage) if held.is_some() => held,
            Verdict::Drop(reason) => {
                self.drops.count(reason);
                None
            }
        }
    }
}

/// The counts of a source's hand-off queue: events that a full or closed queue
/// dropped, and pointer folds that a full sink handed back to be added to the
/// next interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HandOffCounts {
    pub lost: u64,
    pub coalesced: u64,
    /// The OS events that a pointer source's open folds held when it stopped:
    /// a stop makes no record of them.
    pub pointer_dropped_at_stop: u64,
}

pub(crate) const HANDOFF_CAPACITY: usize = 1_024;

struct Queue {
    events: VecDeque<FloorEvent>,
    closed: bool,
    writer_gone: bool,
}

#[derive(Default)]
pub(crate) struct LossCounts {
    lost: AtomicU64,
    coalesced: AtomicU64,
    dropped_at_stop: AtomicU64,
}

impl LossCounts {
    fn lose(&self, events: u64) {
        self.lost.fetch_add(events, Ordering::Relaxed);
    }

    pub(crate) fn read(&self) -> HandOffCounts {
        HandOffCounts {
            lost: self.lost.load(Ordering::Relaxed),
            coalesced: self.coalesced.load(Ordering::Relaxed),
            pointer_dropped_at_stop: self.dropped_at_stop.load(Ordering::Relaxed),
        }
    }
}

struct Shared {
    queue: Mutex<Queue>,
    wake: Condvar,
    sink: SinkHandle,
    counts: Arc<LossCounts>,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Queue> {
        self.queue.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

pub(crate) struct HandOff {
    shared: Arc<Shared>,
}

#[derive(Clone)]
pub(crate) struct Producer {
    shared: Arc<Shared>,
}

impl HandOff {
    pub(crate) fn start(
        sink: SinkHandle,
        name: &str,
        counts: Arc<LossCounts>,
    ) -> Result<Self, CaptureError> {
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue {
                events: VecDeque::new(),
                closed: false,
                writer_gone: false,
            }),
            wake: Condvar::new(),
            sink,
            counts,
        });
        let forwarded = Arc::clone(&shared);
        std::thread::Builder::new()
            .name(name.to_string())
            .spawn(move || forward(&forwarded))
            .map_err(|e| CaptureError::Os {
                stage: "spawn the forwarder thread",
                domain: "io",
                code: i64::from(e.raw_os_error().unwrap_or(0)),
            })?;
        Ok(Self { shared })
    }

    pub(crate) fn producer(&self) -> Producer {
        Producer {
            shared: Arc::clone(&self.shared),
        }
    }

    #[cfg(test)]
    fn counts(&self) -> HandOffCounts {
        self.shared.counts.read()
    }

    pub(crate) fn close(&self) {
        let mut queue = self.shared.lock();
        queue.closed = true;
        discard(&mut queue, &self.shared.counts);
        drop(queue);
        self.shared.wake.notify_all();
    }

    #[cfg(test)]
    fn queued(&self) -> usize {
        self.shared.lock().events.len()
    }
}

impl Drop for HandOff {
    fn drop(&mut self) {
        self.close();
    }
}

impl Producer {
    pub(crate) fn push(&self, event: FloorEvent) {
        let mut queue = self.shared.lock();
        if queue.closed || queue.events.len() >= HANDOFF_CAPACITY {
            self.shared.counts.lose(1);
            return;
        }
        queue.events.push_back(event);
        drop(queue);
        self.shared.wake.notify_one();
    }

    /// Hands a fold record to the sink, which never blocks for one. A full
    /// sink hands it back and its fold is returned, to be added to the next
    /// interval of its class; a closed hand-off counts it lost.
    pub(crate) fn emit_fold(&self, fold: FoldEvent) -> Option<PointerFold> {
        let mut queue = self.shared.lock();
        if queue.closed {
            self.shared.counts.lose(1);
            return None;
        }
        match self.shared.sink.emit_continuous(fold) {
            FoldEmitted::Queued => None,
            FoldEmitted::Coalesced(back) => {
                self.shared.counts.coalesced.fetch_add(1, Ordering::Relaxed);
                Some(back.fold())
            }
            FoldEmitted::Refused => {
                self.shared.counts.lose(1);
                discard(&mut queue, &self.shared.counts);
                queue.closed = true;
                queue.writer_gone = true;
                None
            }
        }
    }

    pub(crate) fn writer_gone(&self) -> bool {
        self.shared.lock().writer_gone
    }

    /// Counts the OS events of the open folds that a stop dropped.
    pub(crate) fn drop_at_stop(&self, events: u64) {
        self.shared
            .counts
            .dropped_at_stop
            .fetch_add(events, Ordering::Relaxed);
    }
}

impl HandOff {
    pub(crate) fn writer_gone(&self) -> bool {
        self.shared.lock().writer_gone
    }
}

fn forward(shared: &Shared) {
    while let Some(event) = next_event(shared) {
        if forward_one(&shared.sink, event) == Emitted::Refused {
            let mut queue = shared.lock();
            discard(&mut queue, &shared.counts);
            queue.closed = true;
            queue.writer_gone = true;
            return;
        }
    }
}

fn discard(queue: &mut Queue, counts: &LossCounts) {
    counts.lose(queue.events.len() as u64);
    queue.events.clear();
}

fn next_event(shared: &Shared) -> Option<FloorEvent> {
    let mut queue = shared.lock();
    loop {
        if let Some(event) = queue.events.pop_front() {
            return Some(event);
        }
        if queue.closed {
            return None;
        }
        queue = shared
            .wake
            .wait(queue)
            .unwrap_or_else(PoisonError::into_inner);
    }
}

/// The one emit of a source's forwarder.
pub(crate) fn forward_one(sink: &SinkHandle, event: FloorEvent) -> Emitted {
    sink.emit_event(event)
}

#[cfg(target_os = "windows")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PumpThreadId(u32);

#[cfg(target_os = "windows")]
impl PumpThreadId {
    pub(crate) fn current() -> Self {
        // SAFETY: GetCurrentThreadId takes no argument and reads this
        // thread's id.
        Self(unsafe { windows::Win32::System::Threading::GetCurrentThreadId() })
    }

    pub(crate) fn get(self) -> u32 {
        self.0
    }
}

pub(crate) struct PumpReady(std::sync::mpsc::Sender<Result<PumpThreadId, CaptureError>>);

impl PumpReady {
    pub(crate) fn report(self, outcome: Result<(), CaptureError>) {
        let _ = self.0.send(outcome.map(|()| PumpThreadId::current()));
    }
}

pub(crate) fn last_os_error(stage: &'static str) -> CaptureError {
    os_error(stage, &windows::core::Error::from_thread())
}

pub(crate) fn os_error(stage: &'static str, error: &windows::core::Error) -> CaptureError {
    CaptureError::Os {
        stage,
        domain: "HRESULT",
        code: i64::from(error.code().0),
    }
}

const PUMP_ENDED_UNREPORTED: &str = "the pump thread ended before it reported its setup";

pub const INPUT_STOP_DEADLINE: Duration = Duration::from_millis(1_000);

pub const SCREEN_STOP_DEADLINE: Duration = Duration::from_millis(6_000);

const STOP_POLL: Duration = Duration::from_millis(5);

pub const STOP_READ_LIMIT: usize = 4_096;

fn deadline_ms(deadline: Duration) -> u64 {
    u64::try_from(deadline.as_millis()).unwrap_or(u64::MAX)
}

pub(crate) struct StopSignal(
    windows::Win32::Foundation::HANDLE,
    Mutex<Option<CaptureError>>,
);

// SAFETY: an event handle names a kernel object of the process, which any of
// its threads may set and wait on.
unsafe impl Send for StopSignal {}
// SAFETY: as for Send; SetEvent and a wait on the same event may run on two
// threads at once.
unsafe impl Sync for StopSignal {}

impl StopSignal {
    fn new() -> Result<Self, CaptureError> {
        use windows::Win32::System::Threading::CreateEventW;
        // SAFETY: CreateEventW with no attributes and no name makes a new
        // unnamed manual-reset event, initially unset.
        let event = unsafe { CreateEventW(None, true, false, windows::core::PCWSTR::null()) }
            .map_err(|e| os_error("CreateEventW", &e))?;
        Ok(Self(event, Mutex::new(None)))
    }

    pub(crate) fn report_removal(&self, stage: &'static str, removed: windows::core::Result<()>) {
        if let Err(error) = removed {
            self.1
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get_or_insert_with(|| os_error(stage, &error));
        }
    }

    fn removal_failure(&self) -> Option<CaptureError> {
        self.1
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn set(&self) {
        // SAFETY: `self.0` is the live event that this value owns.
        unsafe {
            let _ = windows::Win32::System::Threading::SetEvent(self.0);
        }
    }
}

impl Drop for StopSignal {
    fn drop(&mut self) {
        // SAFETY: `self.0` is the event that this value owns; it is closed once.
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

pub(crate) struct PumpHandle {
    id: PumpThreadId,
    join: Option<std::thread::JoinHandle<()>>,
    signal: Arc<StopSignal>,
    stop_requested: bool,
    post: fn(PumpThreadId),
}

impl PumpHandle {
    #[cfg(test)]
    fn with_post(mut self, post: fn(PumpThreadId)) -> Self {
        self.post = post;
        self
    }

    pub(crate) fn stop(&mut self, deadline: Duration) -> Result<(), CaptureError> {
        let Some(join) = self.join.take() else {
            return Ok(());
        };
        let ended_before = join.is_finished() && !self.stop_requested;
        self.stop_requested = true;
        let began = Instant::now();
        self.signal.set();
        loop {
            (self.post)(self.id);
            if join.is_finished() {
                break;
            }
            if began.elapsed() >= deadline {
                self.join = Some(join);
                return Err(CaptureError::Timeout {
                    stage: "stop",
                    ms: deadline_ms(deadline),
                });
            }
            std::thread::sleep(STOP_POLL);
        }
        if join.join().is_err() || ended_before {
            return Err(CaptureError::ThreadEnded);
        }
        match self.signal.removal_failure() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn has_ended(&self) -> bool {
        self.join
            .as_ref()
            .is_none_or(std::thread::JoinHandle::is_finished)
    }

    fn let_go(&mut self) {
        if let Some(join) = self.join.take() {
            self.signal.set();
            (self.post)(self.id);
            drop(join);
        }
    }
}

impl Drop for PumpHandle {
    fn drop(&mut self) {
        if self.stop_requested {
            self.let_go();
        } else {
            let _ = self.stop(INPUT_STOP_DEADLINE);
        }
    }
}

#[derive(Default)]
pub(crate) struct Lifecycle {
    pump: Option<PumpHandle>,
    handoff: Option<HandOff>,
    counts: Arc<LossCounts>,
    timed_out: Option<CaptureError>,
    removal_failed: Option<CaptureError>,
    stopped: bool,
    off: bool,
}

impl Lifecycle {
    /// A begin whose switch is off: nothing is installed and the health reads
    /// Off until a begin with the switch on.
    pub(crate) fn off(&mut self) -> Result<Started, CaptureError> {
        if self.pump.is_some() || self.removal_failed.is_some() {
            return Err(CaptureError::AlreadyStarted);
        }
        self.off = true;
        Ok(Started::Off)
    }

    pub(crate) fn begin(
        &mut self,
        ctx: &SourceContext,
        forwarder: &str,
        spawn: impl FnOnce(Producer) -> Result<PumpHandle, CaptureError>,
    ) -> Result<Started, CaptureError> {
        if self.pump.is_some() || self.removal_failed.is_some() {
            return Err(CaptureError::AlreadyStarted);
        }
        let handoff = HandOff::start(ctx.sink().clone(), forwarder, Arc::clone(&self.counts))?;
        let pump = spawn(handoff.producer())?;
        self.pump = Some(pump);
        self.handoff = Some(handoff);
        self.stopped = false;
        self.off = false;
        Ok(Started::Running)
    }

    pub(crate) fn is_running(&self) -> bool {
        self.pump.is_some()
    }

    pub(crate) fn shutdown(&mut self) -> Result<(), CaptureError> {
        if let Some(error) = &self.removal_failed {
            return Err(error.clone());
        }
        let Some(pump) = self.pump.as_mut() else {
            return Ok(());
        };
        let stopped = pump.stop(INPUT_STOP_DEADLINE);
        if let Some(handoff) = &self.handoff {
            handoff.close();
        }
        if let Err(timeout @ CaptureError::Timeout { .. }) = &stopped {
            self.timed_out = Some(timeout.clone());
            return stopped;
        }
        self.pump = None;
        self.timed_out = None;
        if let Err(error @ CaptureError::Os { .. }) = &stopped {
            self.removal_failed = Some(error.clone());
            return stopped;
        }
        self.stopped = true;
        stopped
    }

    pub(crate) fn health(&self, counters: SourceCounters) -> SourceHealth {
        let writer_gone = self.handoff.as_ref().is_some_and(HandOff::writer_gone);
        if let Some(error) = &self.removal_failed {
            return SourceHealth {
                state: SourceState::Failed {
                    reason: HealthReason::OsError,
                    error: error.clone(),
                },
                counters,
            };
        }
        let state = match (&self.pump, &self.timed_out) {
            (Some(_), Some(timeout)) => SourceState::Failed {
                reason: HealthReason::Timeout,
                error: timeout.clone(),
            },
            (Some(_), None) if writer_gone => failed_with(HealthReason::WriterGone),
            (Some(pump), None) if pump.has_ended() => thread_ended(),
            (Some(_), None) => SourceState::Running,
            (None, _) if self.off => SourceState::Off,
            (None, _) if self.stopped => SourceState::Stopped(StopCause::Request),
            (None, _) => SourceState::NotReported,
        };
        SourceHealth { state, counters }
    }

    pub(crate) fn handoff_counts(&self) -> HandOffCounts {
        self.counts.read()
    }

    pub(crate) fn removal_failed(&mut self, error: CaptureError) {
        self.removal_failed.get_or_insert(error);
    }
}

impl Drop for Lifecycle {
    fn drop(&mut self) {
        if self.timed_out.is_none() {
            let _ = self.shutdown();
        }
    }
}

/// The process of the window in front, read again only when the window in
/// front changes, and the process of the top-level window under the pointer,
/// read again only when that window changes.
#[derive(Debug, Default)]
pub(crate) struct FrontCache {
    window: usize,
    front: FrontApp,
    under: usize,
    under_front: FrontApp,
}

impl FrontCache {
    pub(crate) fn now(&mut self) -> FrontApp {
        // SAFETY: GetForegroundWindow takes no argument and returns a handle value.
        let window = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
        if window.0 as usize != self.window {
            self.window = window.0 as usize;
            self.front = foreground::resolve_front(window).unwrap_or_default();
        }
        self.front.clone()
    }

    /// The process of the top-level window under the pointer, which a pointer
    /// sample belongs to; where the pointer's place or the window there cannot
    /// be read, the process of the window in front.
    pub(crate) fn under_pointer(&mut self) -> FrontApp {
        use windows::Win32::Foundation::POINT;
        use windows::Win32::UI::WindowsAndMessaging::{
            GetAncestor, GetCursorPos, WindowFromPoint, GA_ROOT,
        };
        let mut place = POINT::default();
        // SAFETY: `place` is a live POINT that the call writes into.
        if unsafe { GetCursorPos(&mut place) }.is_err() {
            return self.now();
        }
        // SAFETY: WindowFromPoint and GetAncestor take plain values and
        // return handle values.
        let window = unsafe { GetAncestor(WindowFromPoint(place), GA_ROOT) };
        if window.0.is_null() {
            return self.now();
        }
        if window.0 as usize != self.under {
            self.under = window.0 as usize;
            self.under_front = foreground::resolve_front(window).unwrap_or_default();
        }
        self.under_front.clone()
    }
}

pub(crate) fn thread_ended() -> SourceState {
    failed_with(HealthReason::ThreadEnded)
}

pub(crate) fn failed_with(reason: HealthReason) -> SourceState {
    SourceState::Failed {
        reason,
        error: CaptureError::ThreadEnded,
    }
}

pub(crate) fn spawn_pump<F>(name: String, body: F) -> Result<PumpHandle, CaptureError>
where
    F: FnOnce(PumpReady, Arc<StopSignal>) + Send + 'static,
{
    let signal = Arc::new(StopSignal::new()?);
    let pump_signal = Arc::clone(&signal);
    let (tx, rx) = std::sync::mpsc::channel();
    let join = std::thread::Builder::new()
        .name(name)
        .spawn(move || body(PumpReady(tx), pump_signal))
        .map_err(|e| CaptureError::Os {
            stage: "spawn the pump thread",
            domain: "io",
            code: i64::from(e.raw_os_error().unwrap_or(0)),
        })?;
    match rx.recv() {
        Ok(Ok(id)) => Ok(PumpHandle {
            id,
            join: Some(join),
            signal,
            stop_requested: false,
            post: post_quit_to,
        }),
        Ok(Err(error)) => {
            let _ = join.join();
            Err(error)
        }
        Err(_) => {
            let _ = join.join();
            Err(CaptureError::Start(PUMP_ENDED_UNREPORTED.to_string()))
        }
    }
}

pub(crate) fn post_quit_to(thread_id: PumpThreadId) {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};
    // SAFETY: PostThreadMessageW takes a thread id and two plain values. A
    // pump handle posts only while it holds its thread's join handle, so the
    // id names that thread: a thread with no queue, or one that ended, makes
    // the call fail and changes nothing, and no other thread can take the id
    // while the handle is open.
    unsafe {
        let _ = PostThreadMessageW(thread_id.get(), WM_QUIT, WPARAM(0), LPARAM(0));
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn ensure_message_queue() {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{PeekMessageW, MSG, PM_NOREMOVE};
    let mut msg = MSG::default();
    // SAFETY: `msg` is a live MSG that PeekMessageW may fill; PM_NOREMOVE
    // leaves the queue as it is.
    unsafe {
        let _ = PeekMessageW(
            &mut msg,
            Some(HWND(std::ptr::null_mut())),
            0,
            0,
            PM_NOREMOVE,
        );
    }
}

pub(crate) fn pump_messages(signal: &StopSignal) {
    use windows::Win32::Foundation::WAIT_OBJECT_0;
    use windows::Win32::System::Threading::INFINITE;
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, MsgWaitForMultipleObjectsEx, PeekMessageW, TranslateMessage, MSG,
        MWMO_INPUTAVAILABLE, PM_REMOVE, QS_ALLINPUT, WM_QUIT,
    };
    let mut msg = MSG::default();
    'pump: loop {
        // SAFETY: `msg` is a live MSG that PeekMessageW fills, and the same
        // value goes to TranslateMessage and DispatchMessageW.
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    break 'pump;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        // SAFETY: the slice holds the live event of `signal`, which outlives
        // the wait.
        let woke = unsafe {
            MsgWaitForMultipleObjectsEx(
                Some(&[signal.0]),
                INFINITE,
                QS_ALLINPUT,
                MWMO_INPUTAVAILABLE,
            )
        };
        if woke.0 != WAIT_OBJECT_0.0 + 1 {
            break;
        }
    }
    READING_AT_STOP.with(|reading| reading.set(true));
    read_at_stop(STOP_READ_LIMIT);
    READING_AT_STOP.with(|reading| reading.set(false));
}

thread_local! {
    static READING_AT_STOP: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether this pump thread is reading its queue after a stop was asked: the
/// queue is emptied, and no message read then becomes a record, since nothing
/// is written after a person has stopped the capture.
pub(crate) fn reading_at_stop() -> bool {
    READING_AT_STOP.with(std::cell::Cell::get)
}

fn read_at_stop(limit: usize) {
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE, WM_QUIT,
    };
    let mut msg = MSG::default();
    for _ in 0..limit {
        // SAFETY: `msg` is a live MSG that PeekMessageW fills, and the same
        // value goes to TranslateMessage and DispatchMessageW.
        unsafe {
            if !PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                return;
            }
            if msg.message != WM_QUIT {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}

pub fn timed_run(seconds: u64) -> Result<TimedRunReport, DaemonError> {
    let _ = seconds;
    Err(DaemonError::NotImplemented(
        "Windows functional capture (deferred to a permissioned interactive desktop)",
    ))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::event::Modality;

    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    pub(crate) fn serial() -> std::sync::MutexGuard<'static, ()> {
        SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(crate) fn hook_without_callback(
        event: u32,
        _callback: windows::Win32::UI::Accessibility::WINEVENTPROC,
    ) -> windows::Win32::UI::Accessibility::HWINEVENTHOOK {
        // SAFETY: SetWinEventHook takes the event range, no module and no
        // callback, and passes no pointer of ours.
        unsafe {
            windows::Win32::UI::Accessibility::SetWinEventHook(
                event,
                event,
                None,
                None,
                0,
                0,
                super::foreground::HOOK_FLAGS,
            )
        }
    }

    pub(crate) fn refused_hook_error() -> CaptureError {
        CaptureError::Os {
            stage: "SetWinEventHook",
            domain: "HRESULT",
            code: i64::from(
                windows::Win32::Foundation::ERROR_INVALID_FILTER_PROC
                    .to_hresult()
                    .0,
            ),
        }
    }

    #[test]
    fn the_age_of_an_event_is_read_across_the_wrap_of_the_tick_count() {
        let at = |now_tick, event_tick| OsTick::new(event_tick).age_at(now_tick);
        assert_eq!(at(5_000, 4_750), Duration::from_millis(250));
        assert_eq!(at(100, u32::MAX - 149), Duration::from_millis(250));
        assert_eq!(at(4_750, 5_000), Duration::ZERO);
        assert_eq!(at(5_000, 5_000), Duration::ZERO);
    }

    const SHAPE_APP: &str = "C:\\Apps\\halvo-shape\\tool.exe";
    const MARKER: &str = "halvo-shape-marker-7c1";

    /// One event of each kind, each built by its Windows path and handed to a
    /// source's hand-off: the floor's decision, then core-2's constructor.
    fn one_of_each_kind(producer: &Producer) {
        use crate::floor::exclude::{FloorHandle, NoLineage};
        use crate::floor::payload::TitleWhy;
        use crate::floor::{kind, Switches};
        use crate::secure_input::windows::PasswordFieldProbe;
        let mut switches = Switches::default();
        switches.set(Switch::ScreenActivity, true);
        let floor = Floor::new(
            FloorHandle::new(None, Arc::new(NoLineage), switches),
            Arc::default(),
        );
        let front = FrontApp {
            pid: 4242,
            path: Some(SHAPE_APP.to_string()),
        };
        let tick = OsTick::new(now_tick());
        let probe = PasswordFieldProbe::default();
        let clock = crate::event::EventClock::system();
        let keep = floor
            .keep::<kind::ForegroundApp>(&front, PrivateWindow::Unknown, &probe)
            .expect("kept");
        producer.push(FloorEvent::foreground_app(keep, tick.age(), clock));
        let title = (format!("Inbox {MARKER}"), TitleWhy::TitleChange);
        producer.push(
            window_title::title_event(&floor, None, (&front, PrivateWindow::Unknown), title, tick)
                .expect("a title"),
        );
        let keep = floor
            .keep::<kind::Keystroke>(&front, PrivateWindow::Unknown, &probe)
            .expect("kept");
        producer.push(FloorEvent::keystroke(
            keep,
            cleared(),
            input::class_of(0x41),
            clock,
        ));
        let mut folds = input::Folds::default();
        let mut held = 0;
        for (moved, flags) in [
            (false, 0x0001),
            (false, 0x0002),
            (true, 0),
            (true, 0),
            (true, 0),
        ] {
            let parts = input::pointer_parts(&mut held, moved, flags);
            input::pointer_records((&floor, producer, &mut folds), &front, parts, now_tick());
        }
        for due in folds.due(now_tick(), true).into_iter().flatten() {
            input::emit_due(&floor, producer, &mut folds, due);
        }
        producer.push(screen::frame_event(&floor, None, &front).expect("a frame"));
    }

    fn shape(record: &crate::floor::record::DecodedRecord) -> String {
        use crate::floor::record::DecodedPayload;
        let payload = match record.payload() {
            DecodedPayload::ForegroundApp => "{}".to_string(),
            DecodedPayload::WindowTitle {
                text,
                cut,
                why,
                site,
            } => format!(
                "text={} cut={cut} why={} site={}",
                text.as_str(),
                why.as_str(),
                site.is_some()
            ),
            DecodedPayload::Keystroke(class) => format!("class={}", class.as_str()),
            DecodedPayload::Click(button) => format!("class=click button={}", button.as_str()),
            DecodedPayload::Fold { class, count } => {
                format!("class={} count={count}", class.as_str())
            }
            DecodedPayload::ScreenFrame => "{}".to_string(),
            #[allow(unreachable_patterns)]
            _ => "other".to_string(),
        };
        format!("{} {payload}", record.kind())
    }

    #[test]
    fn one_event_of_each_kind_reaches_the_model_store_in_the_shape_of_c13() {
        use crate::daemon::model_store::Harness;
        use crate::daemon::owner::Command;
        let mut harness = Harness::new();
        harness.ask(Command::ClosePause);
        let handoff = HandOff::start(harness.sink.clone(), "halvo-test-shape", Arc::default())
            .expect("hand-off");
        one_of_each_kind(&handoff.producer());
        let accepted = || {
            Modality::ALL
                .into_iter()
                .map(|kind| harness.sink.counts().of(kind).accepted)
                .sum::<u64>()
        };
        let began = Instant::now();
        while accepted() < 6 && began.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
        }
        harness.ask(Command::Settle);
        let records = harness.store.records();
        let mut shapes: Vec<String> = records.iter().map(shape).collect();
        shapes.sort();
        let mut expected = vec![
            "foreground_app {}".to_string(),
            "keystroke class=printable".to_string(),
            "pointer class=click button=primary".to_string(),
            "pointer class=move count=3".to_string(),
            "screen_frame {}".to_string(),
            format!("window_title text=Inbox {MARKER} cut=false why=title_change site=false"),
        ];
        expected.sort();
        assert_eq!(shapes, expected, "a kind has no record of its shape");
        for record in &records {
            assert_eq!(record.app(), Some(SHAPE_APP), "{}", shape(record));
            let resolution: i64 = match record.kind() {
                "keystroke" => 1_000_000_000,
                "pointer" if shape(record).contains("click") => 1_000_000_000,
                _ => 1_000_000,
            };
            assert_eq!(record.at() % resolution, 0, "{}", shape(record));
        }
        let with_marker: Vec<String> = records
            .iter()
            .map(shape)
            .filter(|shape| shape.contains(MARKER))
            .collect();
        assert_eq!(with_marker.len(), 1, "{with_marker:?}");
        assert!(with_marker[0].starts_with("window_title text=Inbox "));
        assert!(records
            .iter()
            .all(|record| !record.app().unwrap_or_default().contains(MARKER)));
    }

    #[test]
    fn the_content_probe_of_a_gate_reads_its_field_state() {
        use crate::secure_input::windows::{FieldState, PasswordGate};
        use crate::secure_input::{check, SecureInputReading, Withheld};
        let reading = |gate: &PasswordGate| match check(&gate.content_probe(), None) {
            Ok(_) => SecureInputReading::Inactive,
            Err(Withheld::Active) => SecureInputReading::Active,
            Err(Withheld::Unknown | Withheld::Late) => SecureInputReading::Unknown,
        };
        let gate = PasswordGate::new();
        assert_eq!(reading(&gate), SecureInputReading::Unknown);
        for (state, wanted) in [
            (FieldState::Password, SecureInputReading::Active),
            (FieldState::Clear, SecureInputReading::Inactive),
            (FieldState::Unknown, SecureInputReading::Unknown),
            (FieldState::Password, SecureInputReading::Active),
        ] {
            gate.field_at(now_tick(), state);
            assert_eq!(gate.field_now(), state);
            assert_eq!(reading(&gate), wanted, "{state:?}");
        }
    }

    #[test]
    fn a_front_whose_process_cannot_be_read_gives_no_event_of_any_kind() {
        use crate::floor::exclude::{FloorHandle, NoLineage};
        use crate::floor::{kind, Switches};
        use crate::secure_input::windows::PasswordFieldProbe;
        let mut switches = Switches::default();
        switches.set(Switch::ScreenActivity, true);
        let drops = Arc::new(DropCounts::default());
        let floor = Floor::new(
            FloorHandle::new(None, Arc::new(NoLineage), switches),
            Arc::clone(&drops),
        );
        let unread = FrontApp {
            pid: 79,
            path: None,
        };
        let probe = PasswordFieldProbe::default();
        let window = PrivateWindow::Unknown;
        let kept = [
            floor
                .keep::<kind::ForegroundApp>(&unread, window, &probe)
                .is_some(),
            floor
                .keep::<kind::WindowTitle>(&unread, window, &probe)
                .is_some(),
            floor
                .keep::<kind::Keystroke>(&unread, window, &probe)
                .is_some(),
            floor
                .keep::<kind::Pointer>(&unread, window, &probe)
                .is_some(),
            floor
                .keep::<kind::ScreenFrame>(&unread, window, &probe)
                .is_some(),
        ];
        let unknown_lineage = drops.read().unknown_lineage;
        assert!(
            kept == [false; 5] && unknown_lineage == 5,
            "kept={kept:?} unknown_lineage={unknown_lineage}"
        );
    }

    #[test]
    fn a_listed_app_in_front_drops_every_kind_and_counts_each_drop() {
        use crate::floor::exclude::{FloorHandle, NoLineage};
        use crate::floor::{kind, Switches};
        use crate::secure_input::windows::PasswordFieldProbe;
        let mut switches = Switches::default();
        switches.set(Switch::ScreenActivity, true);
        let drops = Arc::new(DropCounts::default());
        let floor = Floor::new(
            FloorHandle::new(None, Arc::new(NoLineage), switches),
            Arc::clone(&drops),
        );
        let listed = FrontApp {
            pid: 77,
            path: Some("C:\\Program Files\\KeePassXC\\KeePassXC.exe".to_string()),
        };
        let other = FrontApp {
            pid: 78,
            path: Some("C:\\Program Files\\Notes\\notes.exe".to_string()),
        };
        let probe = PasswordFieldProbe::default();
        let window = PrivateWindow::Unknown;
        for front in [&listed, &other] {
            let kept = [
                floor
                    .keep::<kind::ForegroundApp>(front, window, &probe)
                    .is_some(),
                floor
                    .keep::<kind::WindowTitle>(front, window, &probe)
                    .is_some(),
                floor
                    .keep::<kind::Keystroke>(front, window, &probe)
                    .is_some(),
                floor.keep::<kind::Pointer>(front, window, &probe).is_some(),
                floor
                    .keep::<kind::ScreenFrame>(front, window, &probe)
                    .is_some(),
            ];
            assert_eq!(kept, [front == &other; 5], "{front:?}");
        }
        assert_eq!(
            drops.read(),
            Drops {
                listed_app: 5,
                ..Drops::default()
            }
        );
        drops.count(DropReason::ExclusionTree);
        drops.count(DropReason::UnknownLineage);
        drops.count(DropReason::PrivateWindow);
        drops.count(DropReason::SecureInput);
        drops.count(DropReason::SwitchOff);
        assert_eq!(
            drops.read(),
            Drops {
                switch_off: 1,
                listed_app: 5,
                exclusion_tree: 1,
                unknown_lineage: 1,
                private_window: 1,
                secure_input: 1,
            }
        );
    }

    #[test]
    fn timed_run_reports_the_deferral() {
        assert!(matches!(
            timed_run(600),
            Err(DaemonError::NotImplemented(_))
        ));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn spawn_pump_blocks_until_the_thread_reports_a_real_id() {
        let starter = PumpThreadId::current();
        let mut handle = spawn_pump("halvo-pump-test".to_string(), |ready, _signal| {
            ready.report(Ok(()));
        })
        .expect("pump reports its id");
        assert_ne!(
            handle.id.get(),
            0,
            "a reported pump id is never the zero sentinel"
        );
        assert_ne!(
            handle.id, starter,
            "the reported id is the pump thread's, not the starter's"
        );
        let began = Instant::now();
        while !handle.has_ended() {
            assert!(
                began.elapsed() < Duration::from_secs(5),
                "the body did not end"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            handle.stop(INPUT_STOP_DEADLINE),
            Err(CaptureError::ThreadEnded),
            "a pump that had ended before its stop"
        );
        assert_eq!(handle.stop(INPUT_STOP_DEADLINE), Ok(()), "a second stop");
    }

    #[test]
    fn a_pump_that_ended_without_a_stop_reads_as_failed() {
        let (sink, _writer) = crate::sink::channel(4);
        let mut life = Lifecycle::default();
        let started = life.begin(&SourceContext::new(sink), "halvo-test-forward", |_| {
            spawn_pump("halvo-pump-ends".to_string(), |ready, _signal| {
                ready.report(Ok(()));
            })
        });
        assert_eq!(started, Ok(Started::Running));
        let began = Instant::now();
        while life.health(SourceCounters::default()).state == SourceState::Running {
            assert!(
                began.elapsed() < Duration::from_secs(5),
                "the pump did not end"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(life.health(SourceCounters::default()).state, thread_ended());
        assert_eq!(life.shutdown(), Err(CaptureError::ThreadEnded));
        assert_eq!(
            life.health(SourceCounters::default()).state,
            SourceState::Stopped(StopCause::Request)
        );
    }

    fn pointer(app: &str) -> FloorEvent {
        let keep = crate::floor::cases::keep(AppIdentity::executable_path(app.to_string()), 7);
        FloorEvent::click(
            keep,
            crate::floor::payload::Button::Primary,
            None,
            crate::event::EventClock::system(),
        )
    }

    /// The app of a received floor value, read back through the encoder.
    pub(crate) fn tag(received: crate::sink::Received) -> String {
        let floor = received.into_floor().expect("a floor value");
        let line = crate::floor::record::encode(&floor, &crate::floor::cases::IdentityPoint)
            .expect("a record");
        crate::floor::record::decode(line.as_str())
            .expect("the decoder reads the record")
            .app()
            .unwrap_or_default()
            .to_string()
    }

    pub(crate) fn cleared() -> crate::secure_input::KeystrokeClearance {
        use crate::secure_input::{check, FixedProbe, SecureInputReading};
        check(&FixedProbe(SecureInputReading::Inactive), None).expect("an inactive reading")
    }

    fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
        let began = Instant::now();
        while !done() {
            assert!(began.elapsed() < Duration::from_secs(5), "{what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn keystroke(app: &str) -> FloorEvent {
        let keep = crate::floor::cases::keep(AppIdentity::executable_path(app.to_string()), 7);
        FloorEvent::keystroke(
            keep,
            cleared(),
            crate::floor::payload::KeyClass::Printable,
            crate::event::EventClock::system(),
        )
    }

    fn wait_for(mut done: impl FnMut() -> bool) {
        while !done() {
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn within(limit: Duration, body: impl FnOnce() + Send + 'static) {
        let (done, finished) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            body();
            let _ = done.send(());
        });
        match finished.recv_timeout(limit) {
            Ok(()) => worker.join().expect("the test body"),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                if let Err(payload) = worker.join() {
                    std::panic::resume_unwind(payload);
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                panic!("the test did not end within {limit:?}")
            }
        }
    }

    fn quiet_pump() -> Result<PumpHandle, CaptureError> {
        spawn_pump("halvo-pump-quiet".to_string(), |ready, signal| {
            ensure_message_queue();
            ready.report(Ok(()));
            pump_messages(&signal);
        })
    }

    #[test]
    fn packets_that_a_stopped_run_held_do_not_arrive_after_a_barrier_drawn_after_the_stop() {
        within(Duration::from_secs(60), || {
            let (sink, writer) = crate::sink::channel(1);
            assert!(forward_one(&sink, keystroke("filler")) == Emitted::Queued);
            let ctx = SourceContext::new(sink.clone());
            let mut life = Lifecycle::default();
            life.begin(&ctx, "halvo-test-forward-1", |producer| {
                for i in 1..=5 {
                    producer.push(keystroke(&format!("old-{i}")));
                }
                quiet_pump()
            })
            .expect("the first run");
            wait_for(|| life.handoff.as_ref().is_some_and(|h| h.queued() == 4));
            let first_stop = life.shutdown();
            let lost_at_the_stop = life.handoff_counts().lost;
            let filler = tag(writer
                .recv_timeout(Duration::from_secs(5))
                .expect("the filler"));
            wait_for(|| sink.counts().of(Modality::Keystroke).accepted >= 2);
            let barrier = writer.barrier();
            life.begin(&ctx, "halvo-test-forward-2", |producer| {
                producer.push(keystroke("new-1"));
                producer.push(keystroke("new-2"));
                quiet_pump()
            })
            .expect("the second run");
            let mut arrivals = Vec::new();
            let new_arrived = |arrivals: &Vec<(String, bool)>| {
                arrivals
                    .iter()
                    .filter(|(app, _)| app.starts_with("new-"))
                    .count()
            };
            while new_arrived(&arrivals) < 2 {
                let received = writer
                    .recv_timeout(Duration::from_secs(3_600))
                    .expect("an event");
                let before = received.is_before(barrier);
                arrivals.push((tag(received), before));
            }
            let second_stop = life.shutdown();
            drop((life, ctx, sink));
            while let Ok(received) = writer.recv_timeout(Duration::from_secs(3_600)) {
                let before = received.is_before(barrier);
                arrivals.push((tag(received), before));
            }
            let old_after: Vec<&str> = arrivals
                .iter()
                .filter(|(app, before)| app.starts_with("old-") && !before)
                .map(|(app, _)| app.as_str())
                .collect();
            let first_new = arrivals
                .iter()
                .position(|(app, _)| app.starts_with("new-"))
                .expect("a packet of the second run");
            let old_after_a_new = arrivals[first_new..]
                .iter()
                .filter(|(app, _)| app.starts_with("old-"))
                .count();
            assert_eq!(
                old_after.len(),
                1,
                "{} packets of the stopped run arrive after the barrier: {arrivals:?}",
                old_after.len()
            );
            assert_eq!(old_after, ["old-1"], "{arrivals:?}");
            assert_eq!(old_after_a_new, 0, "{arrivals:?}");
            assert_eq!(filler, "filler");
            assert_eq!(first_stop, Ok(()));
            assert_eq!(second_stop, Ok(()));
            assert_eq!(lost_at_the_stop, 4, "the four queued packets");
        });
    }

    #[test]
    fn a_full_queue_drops_a_packet_and_counts_it_lost() {
        let (sink, _writer) = crate::sink::channel(1);
        assert!(forward_one(&sink, pointer("filler")) == Emitted::Queued);
        let handoff = HandOff::start(sink, "halvo-test-forward", Arc::default()).expect("hand-off");
        let producer = handoff.producer();
        producer.push(pointer("taken"));
        wait_until("the forwarder took no packet", || handoff.queued() == 0);
        for _ in 0..HANDOFF_CAPACITY + 3 {
            producer.push(pointer("queued"));
        }
        assert_eq!(handoff.queued(), HANDOFF_CAPACITY);
        assert_eq!(
            handoff.counts(),
            HandOffCounts {
                lost: 3,
                coalesced: 0,
                pointer_dropped_at_stop: 0
            }
        );
    }

    #[test]
    fn a_gone_writer_closes_the_hand_off_and_counts_what_it_held() {
        let (sink, writer) = crate::sink::channel(1);
        drop(writer);
        let handoff = HandOff::start(sink, "halvo-test-forward", Arc::default()).expect("hand-off");
        let producer = handoff.producer();
        producer.push(pointer("refused"));
        wait_until("the forwarder did not see the gone writer", || {
            producer.writer_gone()
        });
        producer.push(pointer("after"));
        assert!(handoff.writer_gone());
        assert_eq!(handoff.counts().lost, 1, "the push after the gone writer");
    }

    #[test]
    fn a_source_whose_writer_is_gone_reads_failed_with_the_reason_writer_gone() {
        let (sink, writer) = crate::sink::channel(1);
        drop(writer);
        let mut life = Lifecycle::default();
        let mut producer = None;
        let started = life.begin(&SourceContext::new(sink), "halvo-test-forward", |made| {
            producer = Some(made);
            quiet_pump()
        });
        assert_eq!(started, Ok(Started::Running));
        let producer = producer.expect("the producer");
        producer.push(pointer("refused"));
        wait_until("the forwarder did not see the gone writer", || {
            producer.writer_gone()
        });
        assert_eq!(
            life.health(SourceCounters::default()).state,
            SourceState::Failed {
                reason: HealthReason::WriterGone,
                error: CaptureError::ThreadEnded,
            }
        );
        assert_eq!(life.shutdown(), Ok(()));
    }

    #[test]
    fn a_closed_hand_off_takes_no_packet_while_its_forwarder_is_parked() {
        let (sink, writer) = crate::sink::channel(1);
        assert!(forward_one(&sink, pointer("filler")) == Emitted::Queued);
        let handoff = HandOff::start(sink, "halvo-test-forward", Arc::default()).expect("hand-off");
        let producer = handoff.producer();
        producer.push(pointer("parked"));
        wait_until("the forwarder took no packet", || handoff.queued() == 0);
        handoff.close();
        for _ in 0..3 {
            producer.push(pointer("after-close"));
        }
        let lost = handoff.counts().lost;
        let queued = handoff.queued();
        let filler = tag(writer
            .recv_timeout(Duration::from_secs(5))
            .expect("the filler"));
        let mut apps = Vec::new();
        while let Ok(received) = writer.recv_timeout(Duration::from_secs(1)) {
            apps.push(tag(received));
        }
        assert_eq!(filler, "filler");
        assert_eq!(
            lost, 3,
            "each push into the closed hand-off is counted lost"
        );
        assert_eq!(queued, 0, "a push into the closed hand-off was queued");
        assert_eq!(apps, ["parked"], "only the parked packet reaches the sink");
    }

    #[test]
    fn a_gone_writer_counts_the_queued_events() {
        let (sink, writer) = crate::sink::channel(1);
        assert!(forward_one(&sink, pointer("filler")) == Emitted::Queued);
        let handoff = HandOff::start(sink, "halvo-test-forward", Arc::default()).expect("hand-off");
        let producer = handoff.producer();
        producer.push(pointer("parked"));
        wait_until("the forwarder took no packet", || handoff.queued() == 0);
        for _ in 0..3 {
            producer.push(pointer("queued"));
        }
        let queued = handoff.queued();
        drop(writer);
        wait_until("the forwarder did not see the gone writer", || {
            handoff.writer_gone()
        });
        assert_eq!(queued, 3);
        assert_eq!(handoff.counts().lost, 3, "the three queued events");
    }

    #[test]
    fn a_push_does_not_wait_for_a_full_sink() {
        let (sink, writer) = crate::sink::channel(1);
        assert_eq!(forward_one(&sink, pointer("filler")), Emitted::Queued);
        let handoff = HandOff::start(sink, "halvo-test-forward", Arc::default()).expect("hand-off");
        let producer = handoff.producer();
        let (pushed, all_pushed) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for index in 0..3 {
                producer.push(pointer(&format!("button-{index}")));
            }
            let _ = pushed.send(());
        });
        assert!(
            all_pushed.recv_timeout(Duration::from_secs(2)).is_ok(),
            "a push waited for the full sink"
        );
        let mut apps = Vec::new();
        while let Ok(received) = writer.recv_timeout(Duration::from_secs(1)) {
            apps.push(tag(received));
        }
        assert_eq!(apps, ["filler", "button-0", "button-1", "button-2"]);
        assert_eq!(handoff.counts().lost, 0);
    }

    fn stuck_source(sink: SinkHandle) -> Lifecycle {
        let mut life = Lifecycle::default();
        let started = life.begin(&SourceContext::new(sink), "halvo-test-forward", |_| {
            spawn_pump("halvo-pump-stuck".to_string(), |ready, _signal| {
                ready.report(Ok(()));
                std::thread::sleep(INPUT_STOP_DEADLINE * 3);
            })
        });
        assert_eq!(started, Ok(Started::Running));
        life
    }

    #[test]
    fn a_dropped_source_whose_pump_does_not_end_waits_its_deadline_once() {
        let (sink, _writer) = crate::sink::channel(4);
        let life = stuck_source(sink);
        let began = Instant::now();
        drop(life);
        let waited = began.elapsed();
        assert!(
            waited < INPUT_STOP_DEADLINE * 3 / 2,
            "the drop waited {waited:?}"
        );
    }

    #[test]
    fn a_dropped_source_whose_stop_timed_out_waits_no_more() {
        let (sink, _writer) = crate::sink::channel(4);
        let mut life = stuck_source(sink);
        let stopped = life.shutdown();
        assert!(
            matches!(stopped, Err(CaptureError::Timeout { .. })),
            "{stopped:?}"
        );
        let began = Instant::now();
        drop(life);
        let waited = began.elapsed();
        assert!(
            waited < Duration::from_millis(250),
            "the drop waited {waited:?}"
        );
    }

    #[test]
    fn stop_deadlines_are_the_first_values_of_c12_item_7() {
        assert_eq!(INPUT_STOP_DEADLINE, Duration::from_millis(1_000));
        assert_eq!(SCREEN_STOP_DEADLINE, Duration::from_millis(6_000));
    }

    #[test]
    fn a_stop_that_timed_out_keeps_the_pump_known_until_it_ends() {
        let (sink, _writer) = crate::sink::channel(4);
        let mut life = Lifecycle::default();
        let alive = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let body_alive = Arc::clone(&alive);
        let started = life.begin(
            &SourceContext::new(sink.clone()),
            "halvo-test-forward",
            move |_| {
                spawn_pump("halvo-pump-stuck".to_string(), move |ready, _signal| {
                    ready.report(Ok(()));
                    std::thread::sleep(INPUT_STOP_DEADLINE * 3);
                    body_alive.store(false, std::sync::atomic::Ordering::SeqCst);
                })
            },
        );
        assert_eq!(started, Ok(Started::Running));
        let timeout = CaptureError::Timeout {
            stage: "stop",
            ms: 1_000,
        };
        let first = life.shutdown();
        let state = life.health(SourceCounters::default()).state;
        let second = life.begin(&SourceContext::new(sink), "halvo-test-forward-2", |_| {
            spawn_pump("halvo-pump-second".to_string(), |ready, signal| {
                ensure_message_queue();
                ready.report(Ok(()));
                pump_messages(&signal);
            })
        });
        let second_stop = life.shutdown();
        let still_alive = alive.load(std::sync::atomic::Ordering::SeqCst);
        wait_until("the stuck pump did not end", || {
            !alive.load(std::sync::atomic::Ordering::SeqCst)
        });
        let late = life.shutdown();
        assert!(still_alive, "the stuck pump ended before the checks");
        assert_eq!(first, Err(timeout.clone()));
        assert_eq!(
            state,
            SourceState::Failed {
                reason: HealthReason::Timeout,
                error: timeout,
            }
        );
        assert_eq!(second, Err(CaptureError::AlreadyStarted));
        assert!(
            matches!(second_stop, Err(CaptureError::Timeout { .. })),
            "a second stop of the stuck pump: {second_stop:?}"
        );
        assert_eq!(late, Ok(()), "the pump that ended late was not joined");
        assert_eq!(
            life.health(SourceCounters::default()).state,
            SourceState::Stopped(StopCause::Request)
        );
    }

    #[test]
    fn packets_that_a_stopped_run_still_held_count_after_a_restart() {
        let (sink, writer) = crate::sink::channel(1);
        assert!(forward_one(&sink, pointer("filler")) == Emitted::Queued);
        let ctx = SourceContext::new(sink.clone());
        let quiet_pump = || {
            spawn_pump("halvo-pump-quiet".to_string(), |ready, signal| {
                ensure_message_queue();
                ready.report(Ok(()));
                pump_messages(&signal);
            })
        };
        let mut life = Lifecycle::default();
        life.begin(&ctx, "halvo-test-forward-1", |producer| {
            for _ in 0..5 {
                producer.push(pointer("button"));
            }
            quiet_pump()
        })
        .expect("the first run");
        wait_until("the forwarder took no packet", || {
            life.handoff.as_ref().is_some_and(|h| h.queued() == 4)
        });
        assert_eq!(life.shutdown(), Ok(()));
        life.begin(&ctx, "halvo-test-forward-2", |_| quiet_pump())
            .expect("the second run");
        drop(writer);
        wait_until("the blocked emit was not refused", || {
            sink.counts().of(Modality::Pointer).refused >= 1
        });
        wait_until("the old forwarder did not count what it held", || {
            life.handoff_counts().lost >= 4
        });
        assert_eq!(sink.counts().of(Modality::Pointer).refused, 1);
        assert_eq!(life.handoff_counts().lost, 4);
        let _ = life.shutdown();
    }

    #[test]
    fn an_event_carries_the_time_of_its_os_event() {
        let (sink, writer) = crate::sink::channel(4);
        let before = std::time::SystemTime::now();
        let keep = crate::floor::cases::keep(AppIdentity::unknown(), 7);
        let age = OsTick::new(now_tick().wrapping_sub(250)).age();
        let event = FloorEvent::foreground_app(keep, age, crate::event::EventClock::system());
        assert_eq!(forward_one(&sink, event), Emitted::Queued);
        let after = std::time::SystemTime::now();
        let received = writer.try_recv().expect("the event is queued");
        let stamped = received.event().captured_at.wall_clock;
        let early = before - Duration::from_millis(320);
        let late = after - Duration::from_millis(180);
        assert!(
            early <= stamped && stamped <= late,
            "the event is stamped {:?} before the forward, not about 250 ms",
            after.duration_since(stamped).unwrap_or_default()
        );
    }

    #[test]
    fn stop_ends_a_pump_whose_first_quit_post_fails_inside_its_deadline() {
        let mut pump = spawn_pump("halvo-pump-late-queue".to_string(), |ready, signal| {
            ready.report(Ok(()));
            std::thread::sleep(Duration::from_millis(300));
            ensure_message_queue();
            pump_messages(&signal);
        })
        .expect("the pump reports");
        let (done, stopped) = std::sync::mpsc::channel();
        let began = Instant::now();
        std::thread::spawn(move || {
            let _ = done.send(pump.stop(INPUT_STOP_DEADLINE));
        });
        let outcome = stopped.recv_timeout(Duration::from_secs(3));
        let waited = began.elapsed();
        assert!(
            matches!(outcome, Ok(Ok(()))),
            "{outcome:?} after {waited:?}"
        );
        assert!(waited < INPUT_STOP_DEADLINE, "{waited:?}");
    }

    static POSTS: AtomicU64 = AtomicU64::new(0);

    fn counted_post(id: PumpThreadId) {
        POSTS.fetch_add(1, Ordering::SeqCst);
        post_quit_to(id);
    }

    #[test]
    fn a_handle_whose_thread_was_joined_posts_nothing_on_drop() {
        let posts_by = |drop_handle: PumpHandle| {
            let before = POSTS.load(Ordering::SeqCst);
            drop(drop_handle);
            POSTS.load(Ordering::SeqCst) - before
        };
        let mut joined = quiet_pump().expect("the pump").with_post(counted_post);
        let joined_stop = joined.stop(INPUT_STOP_DEADLINE);
        let after_ok = posts_by(joined);
        let mut ended = spawn_pump("halvo-pump-ends".to_string(), |ready, _signal| {
            ready.report(Ok(()));
        })
        .expect("the pump")
        .with_post(counted_post);
        wait_until("the body did not end", || ended.has_ended());
        let ended_stop = ended.stop(INPUT_STOP_DEADLINE);
        let after_thread_ended = posts_by(ended);
        let (release, gate) = std::sync::mpsc::channel::<()>();
        let mut stuck = spawn_pump("halvo-pump-stuck".to_string(), move |ready, _signal| {
            ready.report(Ok(()));
            let _ = gate.recv();
        })
        .expect("the pump")
        .with_post(counted_post);
        let stuck_stop = stuck.stop(Duration::from_millis(100));
        let after_timeout = posts_by(stuck);
        let _ = release.send(());
        assert_eq!(joined_stop, Ok(()));
        assert_eq!(ended_stop, Err(CaptureError::ThreadEnded));
        assert!(
            matches!(stuck_stop, Err(CaptureError::Timeout { .. })),
            "{stuck_stop:?}"
        );
        assert_eq!(
            after_ok, 0,
            "the drop after a stop that answered Ok posted {after_ok} quit messages"
        );
        assert_eq!(
            after_thread_ended, 0,
            "the drop after a stop that answered ThreadEnded posted {after_thread_ended} quit messages"
        );
        assert_eq!(
            after_timeout, 1,
            "the drop after a stop that timed out posts its quit message once"
        );
    }

    static HELD_HANDLE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    static NAMED_AT_POST: AtomicU64 = AtomicU64::new(0);
    static OPENED_AT_POST: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);
    static ORDER_POSTS: AtomicU64 = AtomicU64::new(0);

    fn post_that_reads_the_handle(id: PumpThreadId) {
        use windows::Win32::Foundation::{CloseHandle, HANDLE};
        use windows::Win32::System::Threading::{
            GetThreadId, OpenThread, THREAD_QUERY_LIMITED_INFORMATION,
        };
        let raw = HELD_HANDLE.load(Ordering::SeqCst) as *mut core::ffi::c_void;
        // SAFETY: GetThreadId reads a handle value and answers 0 for one that
        // names no thread; it reads no memory of ours.
        let named = unsafe { GetThreadId(HANDLE(raw)) };
        NAMED_AT_POST.store(u64::from(named), Ordering::SeqCst);
        // SAFETY: OpenThread takes an access mask, a flag and a thread id and
        // answers a new handle or an error; it reads no memory of ours.
        let opened = unsafe { OpenThread(THREAD_QUERY_LIMITED_INFORMATION, false, id.get()) };
        OPENED_AT_POST.store(opened.is_ok(), Ordering::SeqCst);
        if let Ok(handle) = opened {
            // SAFETY: `handle` is the handle that OpenThread just answered,
            // closed once.
            unsafe {
                let _ = CloseHandle(handle);
            }
        }
        ORDER_POSTS.fetch_add(1, Ordering::SeqCst);
        post_quit_to(id);
    }

    #[test]
    fn the_drop_after_a_timeout_posts_while_it_holds_the_thread_handle() {
        use std::os::windows::io::AsRawHandle;
        let (release, gate) = std::sync::mpsc::channel::<()>();
        let mut stuck = spawn_pump("halvo-pump-order".to_string(), move |ready, _signal| {
            ready.report(Ok(()));
            let _ = gate.recv();
        })
        .expect("the pump")
        .with_post(post_that_reads_the_handle);
        let stopped = stuck.stop(Duration::from_millis(50));
        let _ = release.send(());
        let join = stuck
            .join
            .as_ref()
            .expect("a stop that timed out keeps the join handle");
        wait_until("the pump thread did not end", || join.is_finished());
        HELD_HANDLE.store(join.as_raw_handle() as usize, Ordering::SeqCst);
        let id = u64::from(stuck.id.get());
        let before = ORDER_POSTS.load(Ordering::SeqCst);
        drop(stuck);
        let posts = ORDER_POSTS.load(Ordering::SeqCst) - before;
        let named = NAMED_AT_POST.load(Ordering::SeqCst);
        let opened = OPENED_AT_POST.load(Ordering::SeqCst);
        assert!(
            matches!(stopped, Err(CaptureError::Timeout { .. })),
            "{stopped:?}"
        );
        assert_eq!(posts, 1, "the drop after a timeout posts once");
        assert_eq!(
            (named, opened),
            (id, true),
            "the drop posted after it had let the thread handle go: at the post the handle named thread {named}, the pump was thread {id}, and the id opened: {opened}"
        );
    }

    fn no_post(_id: PumpThreadId) {}

    #[test]
    fn a_pump_that_ends_at_its_stop_event_reads_what_is_queued() {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Foundation::{HANDLE, LPARAM, WPARAM};
        use windows::Win32::System::Threading::{ResumeThread, SuspendThread};
        use windows::Win32::UI::WindowsAndMessaging::{
            PeekMessageW, PostThreadMessageW, MSG, PM_REMOVE, WM_USER,
        };
        const QUEUED: u32 = 50;
        let (count, counted) = std::sync::mpsc::channel();
        let mut pump = spawn_pump("halvo-pump-event-end".to_string(), move |ready, signal| {
            ensure_message_queue();
            ready.report(Ok(()));
            pump_messages(&signal);
            let mut msg = MSG::default();
            let mut left = 0u32;
            // SAFETY: `msg` is a live MSG that PeekMessageW fills.
            while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
                left += 1;
            }
            let _ = count.send(left);
        })
        .expect("the pump")
        .with_post(no_post);
        std::thread::sleep(Duration::from_millis(200));
        let raw = HANDLE(
            pump.join
                .as_ref()
                .expect("a running pump holds its join handle")
                .as_raw_handle(),
        );
        // SAFETY: `raw` is the pump thread's handle, which `pump` holds open.
        let suspended = unsafe { SuspendThread(raw) };
        pump.signal.set();
        let mut posted = 0;
        for _ in 0..QUEUED {
            // SAFETY: PostThreadMessageW takes the pump's id and two plain
            // values; `pump` holds the thread's handle.
            if unsafe { PostThreadMessageW(pump.id.get(), WM_USER, WPARAM(0), LPARAM(0)) }.is_ok() {
                posted += 1;
            }
        }
        // SAFETY: `raw` is the handle that was suspended above.
        let resumed = unsafe { ResumeThread(raw) };
        let left = counted.recv_timeout(Duration::from_secs(5));
        let _ = pump.stop(INPUT_STOP_DEADLINE);
        assert_ne!(
            suspended,
            u32::MAX,
            "INCONCLUSIVE: the pump was not suspended"
        );
        assert_ne!(resumed, u32::MAX, "INCONCLUSIVE: the pump was not resumed");
        assert_eq!(posted, QUEUED, "INCONCLUSIVE: a post failed");
        assert_eq!(
            left,
            Ok(0),
            "messages left after the pump ended at its stop event with {QUEUED} queued"
        );
    }

    #[test]
    fn a_timed_out_stop_has_closed_the_hand_off_for_what_the_stuck_pump_pushes_later() {
        let (sink, writer) = crate::sink::channel(16);
        let mut life = Lifecycle::default();
        let (go, gate) = std::sync::mpsc::channel::<()>();
        let (pushed, all_pushed) = std::sync::mpsc::channel::<()>();
        let started = life.begin(
            &SourceContext::new(sink),
            "halvo-test-forward",
            move |producer| {
                spawn_pump("halvo-pump-late-push".to_string(), move |ready, _signal| {
                    ready.report(Ok(()));
                    if gate.recv().is_ok() {
                        for _ in 0..3 {
                            producer.push(keystroke("after-the-timeout"));
                        }
                        let _ = pushed.send(());
                    }
                })
            },
        );
        let stopped = life.shutdown();
        let _ = go.send(());
        let late_pushes = all_pushed.recv_timeout(Duration::from_secs(5));
        let mut arrived = 0;
        while writer.recv_timeout(Duration::from_millis(500)).is_ok() {
            arrived += 1;
        }
        let lost = life.handoff_counts().lost;
        assert_eq!(started, Ok(Started::Running));
        assert!(
            matches!(stopped, Err(CaptureError::Timeout { .. })),
            "{stopped:?}"
        );
        assert!(late_pushes.is_ok(), "the stuck pump did not push");
        assert_eq!(
            arrived, 0,
            "packets pushed after the timeout reached the sink"
        );
        assert_eq!(
            lost, 3,
            "each packet pushed after the timeout is counted lost"
        );
    }

    #[test]
    fn the_read_at_a_stop_takes_what_is_queued_and_ends_at_its_bound() {
        use windows::Win32::Foundation::{LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{
            PeekMessageW, PostThreadMessageW, MSG, PM_REMOVE, WM_USER,
        };
        const BEYOND: usize = 100;
        let (posted, left) = std::thread::spawn(|| {
            ensure_message_queue();
            let me = PumpThreadId::current();
            post_quit_to(me);
            let mut posted = 0;
            for _ in 0..STOP_READ_LIMIT + BEYOND {
                // SAFETY: PostThreadMessageW takes this thread's id and two
                // plain values.
                if unsafe { PostThreadMessageW(me.get(), WM_USER, WPARAM(0), LPARAM(0)) }.is_ok() {
                    posted += 1;
                }
            }
            let signal = StopSignal::new().expect("the stop event");
            pump_messages(&signal);
            let mut msg = MSG::default();
            let mut left = 0;
            // SAFETY: `msg` is a live MSG that PeekMessageW fills.
            while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
                left += 1;
            }
            (posted, left)
        })
        .join()
        .expect("the pump thread");
        assert_eq!(
            posted,
            STOP_READ_LIMIT + BEYOND,
            "INCONCLUSIVE: a post failed"
        );
        assert_eq!(
            left, BEYOND,
            "messages left after the pump returned at its quit message"
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn spawn_pump_fails_when_the_body_never_reports() {
        let result = spawn_pump("halvo-pump-silent".to_string(), |_ready, _signal| {});
        assert!(
            matches!(result, Err(CaptureError::Start(ref text)) if text == PUMP_ENDED_UNREPORTED),
            "a body that never reports must fail the start"
        );
        let failed = spawn_pump("halvo-pump-failed".to_string(), |ready, _signal| {
            ready.report(Err(CaptureError::Os {
                stage: "a scripted step",
                domain: "HRESULT",
                code: -1,
            }));
        });
        assert!(matches!(
            failed,
            Err(CaptureError::Os {
                stage: "a scripted step",
                code: -1,
                ..
            })
        ));
    }
}
