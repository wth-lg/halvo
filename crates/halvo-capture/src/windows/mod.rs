//! Windows capture backend.
#![deny(clippy::undocumented_unsafe_blocks)]

pub mod apps;
pub mod audio;
pub mod foreground;
pub mod input;
pub mod screen;
pub mod window_title;

use crate::daemon::{DaemonError, TimedRunReport};
use crate::event::{CaptureStreamEvent, CapturedAt, Modality, SegmentKeyId, SegmentRef};
use crate::sink::{SinkError, SinkHandle};
use crate::source::{
    CaptureError, EmitOutcome, EventSink, HealthReason, SourceContext, SourceCounters,
    SourceHealth, SourceState, Started, StopCause,
};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime};
use uuid::Uuid;

/// Where a captured payload's bytes live in the encrypted segment store: the
/// segment ref plus the per-segment key id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventPlacement {
    pub segment_ref: SegmentRef,
    pub segment_key_id: SegmentKeyId,
}

impl EventPlacement {
    /// The placement an R1 capture source puts on a metadata-only event: the
    /// decoded payload's bytes are not persisted in this phase, so the segment is
    /// the nil sentinel and the key id is 0. `byte_len` carries the decoded
    /// payload's measured size where the source knows it, so the quantity of
    /// captured data is visible in the event even before the store holds the
    /// bytes. The P6 daemon wiring replaces this single constructor with a real
    /// store write that assigns the true segment and key. The nil segment id is
    /// the signal that no payload has been persisted yet; matches the macOS
    /// backend's `unpersisted_placement`.
    pub fn unpersisted(byte_len: u64) -> Self {
        Self {
            segment_ref: SegmentRef {
                segment_id: Uuid::nil(),
                byte_offset: 0,
                byte_len,
            },
            segment_key_id: SegmentKeyId(0),
        }
    }
}

/// Build the one raw stream event for a captured payload: the capture instant
/// that the caller computed, the sink's next monotonic sequence, the foreground
/// app and the store placement, marked raw.
pub fn build_event(
    sink: &SinkHandle,
    modality: Modality,
    foreground_app: String,
    placement: EventPlacement,
    wall_clock: SystemTime,
) -> CaptureStreamEvent {
    let captured_at = CapturedAt {
        wall_clock,
        sequence: sink.next_sequence(),
    };
    CaptureStreamEvent::new_raw(
        captured_at,
        modality,
        foreground_app,
        placement.segment_ref,
        placement.segment_key_id,
    )
}

/// One captured item on its way from an OS callback to the sink.
pub(crate) struct Packet {
    pub(crate) kind: Modality,
    pub(crate) app: String,
    pub(crate) byte_len: u64,
    /// When the OS made the event, in milliseconds since the system started.
    pub(crate) tick: u32,
}

#[link(name = "kernel32")]
extern "system" {
    fn GetTickCount() -> u32;
}

/// The system's tick count now, in milliseconds since it started: the clock of
/// `GetMessageTime` and of a WinEvent's event time.
pub(crate) fn now_tick() -> u32 {
    // SAFETY: GetTickCount takes no argument and reads a counter.
    unsafe { GetTickCount() }
}

/// The wall-clock instant of an OS event made at `event_tick`: the wall clock
/// now minus the age of the event (C12.8).
pub(crate) fn capture_instant(event_tick: u32) -> SystemTime {
    instant_from_age(SystemTime::now(), now_tick(), event_tick)
}

fn instant_from_age(now: SystemTime, now_tick: u32, event_tick: u32) -> SystemTime {
    let age = now_tick.wrapping_sub(event_tick);
    let age = if age > i32::MAX as u32 { 0 } else { age };
    now.checked_sub(Duration::from_millis(u64::from(age)))
        .unwrap_or(now)
}

/// The counts of a source's hand-off queue: packets that a full or closed
/// queue dropped, and motion samples that a newer one replaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HandOffCounts {
    pub lost: u64,
    pub coalesced: u64,
}

/// Packets that a source's queue holds; a push to a full queue drops the packet
/// and counts it as lost.
pub(crate) const HANDOFF_CAPACITY: usize = 1_024;

/// The shortest time between two motion samples that a forwarder sends: the
/// samples in between collapse into the one pending sample.
pub(crate) const MOTION_INTERVAL: Duration = Duration::from_millis(100);

struct Queue {
    packets: VecDeque<Packet>,
    motion: Option<Packet>,
    closed: bool,
    writer_gone: bool,
}

/// The loss counts of a source, shared by every hand-off that the source
/// starts, so that a packet which a stopped run still held is counted after a
/// restart.
#[derive(Default)]
pub(crate) struct LossCounts {
    lost: AtomicU64,
    coalesced: AtomicU64,
}

impl LossCounts {
    fn lose(&self, packets: u64) {
        self.lost.fetch_add(packets, Ordering::Relaxed);
    }

    pub(crate) fn read(&self) -> HandOffCounts {
        HandOffCounts {
            lost: self.lost.load(Ordering::Relaxed),
            coalesced: self.coalesced.load(Ordering::Relaxed),
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

/// A source's bounded queue between its OS callback and the sink, with the
/// forwarder thread that does the blocking emit. A push never waits for the
/// sink. Dropping the hand-off closes it: the forwarder sends what the queue
/// holds and ends.
pub(crate) struct HandOff {
    shared: Arc<Shared>,
}

/// The callback's end of a hand-off.
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
                packets: VecDeque::new(),
                motion: None,
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
        self.shared.lock().closed = true;
        self.shared.wake.notify_all();
    }

    #[cfg(test)]
    fn pending_motion(&self) -> bool {
        self.shared.lock().motion.is_some()
    }

    #[cfg(test)]
    fn queued(&self) -> usize {
        self.shared.lock().packets.len()
    }
}

impl Drop for HandOff {
    fn drop(&mut self) {
        self.close();
    }
}

impl Producer {
    /// Queue a packet that is never collapsed: a key, a button, a title. A
    /// motion sample that is pending goes into the queue ahead of it, so the
    /// sink gets the source's packets in the order in which they came.
    pub(crate) fn push(&self, packet: Packet) {
        let mut queue = self.shared.lock();
        if queue.closed {
            self.shared.counts.lose(1);
            return;
        }
        if queue.packets.len() < HANDOFF_CAPACITY {
            if let Some(motion) = queue.motion.take() {
                queue.packets.push_back(motion);
            }
        }
        if queue.packets.len() >= HANDOFF_CAPACITY {
            self.shared.counts.lose(1);
            return;
        }
        queue.packets.push_back(packet);
        drop(queue);
        self.shared.wake.notify_one();
    }

    /// Make a motion sample the pending one; a sample that was pending is
    /// replaced and counted as coalesced.
    pub(crate) fn push_motion(&self, packet: Packet) {
        let mut queue = self.shared.lock();
        if queue.closed {
            self.shared.counts.lose(1);
            return;
        }
        if queue.motion.replace(packet).is_some() {
            self.shared.counts.coalesced.fetch_add(1, Ordering::Relaxed);
        }
        drop(queue);
        self.shared.wake.notify_one();
    }

    pub(crate) fn writer_gone(&self) -> bool {
        self.shared.lock().writer_gone
    }
}

impl HandOff {
    pub(crate) fn writer_gone(&self) -> bool {
        self.shared.lock().writer_gone
    }
}

fn forward(shared: &Shared) {
    let mut last_motion: Option<Instant> = None;
    while let Some(packet) = next_packet(shared, &mut last_motion) {
        if forward_one(&shared.sink, packet).is_err() {
            let mut queue = shared.lock();
            let left = queue.packets.len() + usize::from(queue.motion.is_some());
            shared.counts.lose(left as u64);
            queue.packets.clear();
            queue.motion = None;
            queue.closed = true;
            queue.writer_gone = true;
            return;
        }
    }
}

fn next_packet(shared: &Shared, last_motion: &mut Option<Instant>) -> Option<Packet> {
    let mut queue = shared.lock();
    loop {
        if let Some(packet) = queue.packets.pop_front() {
            return Some(packet);
        }
        if queue.motion.is_some() {
            let wait = last_motion.map_or(Duration::ZERO, |at| {
                MOTION_INTERVAL.saturating_sub(at.elapsed())
            });
            if wait.is_zero() || queue.closed {
                *last_motion = Some(Instant::now());
                return queue.motion.take();
            }
            queue = shared
                .wake
                .wait_timeout(queue, wait)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
            continue;
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

pub(crate) fn forward_one(sink: &SinkHandle, packet: Packet) -> Result<EmitOutcome, SinkError> {
    let placement = EventPlacement::unpersisted(packet.byte_len);
    let wall_clock = capture_instant(packet.tick);
    sink.emit(build_event(
        sink,
        packet.kind,
        packet.app,
        placement,
        wall_clock,
    ))
}

/// A live pump thread's id, as reported by the pump once its message queue
/// exists and its hook or window is up. [`PumpThreadId::current`] is the only
/// constructor and reads `GetCurrentThreadId`, which never returns 0 for a live
/// thread, so a `PumpThreadId` is always a real target: [`post_quit_to`] cannot
/// be handed a zero sentinel.
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

/// The readiness signal a pump body sends once: the outcome of its setup. An
/// `Ok` hands the pump thread's id to [`spawn_pump`], so a `stop` never runs
/// before the pump has a real thread id; an `Err` names the step that failed.
pub(crate) struct PumpReady(std::sync::mpsc::Sender<Result<PumpThreadId, CaptureError>>);

impl PumpReady {
    pub(crate) fn report(self, outcome: Result<(), CaptureError>) {
        let _ = self.0.send(outcome.map(|()| PumpThreadId::current()));
    }
}

/// The error of a failed Win32 call at `stage`, from the thread's last error.
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

/// How long `shutdown` of the keyboard, pointer, window_title and foreground
/// sources waits for its thread to end (C12.7, first value).
pub const INPUT_STOP_DEADLINE: Duration = Duration::from_millis(1_000);

/// How long `shutdown` of the screen source waits for its capture thread to
/// end (C12.7, first value).
pub const SCREEN_STOP_DEADLINE: Duration = Duration::from_millis(6_000);

const STOP_POLL: Duration = Duration::from_millis(5);

fn deadline_ms(deadline: Duration) -> u64 {
    u64::try_from(deadline.as_millis()).unwrap_or(u64::MAX)
}

/// The signal that tells a pump to end besides `WM_QUIT`: a manual-reset event
/// that the pump's wait watches beside its message queue.
pub(crate) struct StopSignal(windows::Win32::Foundation::HANDLE);

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
        Ok(Self(event))
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

/// A started pump thread: its reported id, its join handle and its stop
/// signal. Dropping it stops the pump.
pub(crate) struct PumpHandle {
    id: PumpThreadId,
    join: Option<std::thread::JoinHandle<()>>,
    signal: Arc<StopSignal>,
    stop_requested: bool,
}

impl PumpHandle {
    /// Set the stop signal and post `WM_QUIT` until the thread ends: `Ok` when
    /// it ended and was joined, [`CaptureError::ThreadEnded`] when it had ended
    /// before the first stop, [`CaptureError::Timeout`] when `deadline` passed
    /// first, which keeps the thread to be joined by a later stop.
    pub(crate) fn stop(&mut self, deadline: Duration) -> Result<(), CaptureError> {
        let Some(join) = self.join.take() else {
            return Ok(());
        };
        let ended_before = join.is_finished() && !self.stop_requested;
        self.stop_requested = true;
        let began = Instant::now();
        self.signal.set();
        loop {
            post_quit_to(self.id);
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
        Ok(())
    }

    fn has_ended(&self) -> bool {
        self.join
            .as_ref()
            .is_none_or(std::thread::JoinHandle::is_finished)
    }
}

impl Drop for PumpHandle {
    fn drop(&mut self) {
        let _ = self.stop(INPUT_STOP_DEADLINE);
    }
}

/// The run of a pump source: its pump, its hand-off and the loss counts of
/// every run. A stop that timed out keeps the pump, so that the source reads
/// as failed and refuses a new begin until a later stop joins it. Dropping it
/// stops the pump. `shutdown` returns once the pump has ended: the forwarder
/// then sends what its queue held and ends on its own, so a shutdown can
/// return while an emit is parked (C12.7).
#[derive(Default)]
pub(crate) struct Lifecycle {
    pump: Option<PumpHandle>,
    handoff: Option<HandOff>,
    counts: Arc<LossCounts>,
    timed_out: Option<CaptureError>,
    stopped: bool,
}

impl Lifecycle {
    /// Start a hand-off to the context's sink and the pump that `spawn` makes
    /// with its producer; [`CaptureError::AlreadyStarted`] while a pump runs.
    pub(crate) fn begin(
        &mut self,
        ctx: &SourceContext,
        forwarder: &str,
        spawn: impl FnOnce(Producer) -> Result<PumpHandle, CaptureError>,
    ) -> Result<Started, CaptureError> {
        if self.pump.is_some() {
            return Err(CaptureError::AlreadyStarted);
        }
        let handoff = HandOff::start(ctx.sink().clone(), forwarder, Arc::clone(&self.counts))?;
        let pump = spawn(handoff.producer())?;
        self.pump = Some(pump);
        self.handoff = Some(handoff);
        self.stopped = false;
        Ok(Started::Running)
    }

    pub(crate) fn is_running(&self) -> bool {
        self.pump.is_some()
    }

    pub(crate) fn shutdown(&mut self) -> Result<(), CaptureError> {
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
        self.stopped = true;
        stopped
    }

    pub(crate) fn health(&self, counters: SourceCounters) -> SourceHealth {
        let writer_gone = self.handoff.as_ref().is_some_and(HandOff::writer_gone);
        let state = match (&self.pump, &self.timed_out) {
            (Some(_), Some(timeout)) => SourceState::Failed {
                reason: HealthReason::Timeout,
                error: timeout.clone(),
            },
            (Some(pump), None) if pump.has_ended() || writer_gone => thread_ended(),
            (Some(_), None) => SourceState::Running,
            (None, _) if self.stopped => SourceState::Stopped(StopCause::Request),
            (None, _) => SourceState::NotReported,
        };
        SourceHealth { state, counters }
    }

    pub(crate) fn handoff_counts(&self) -> HandOffCounts {
        self.counts.read()
    }
}

impl Drop for Lifecycle {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

/// The failed state of a source whose thread ended without a stop, or whose
/// sink's writer is gone.
pub(crate) fn thread_ended() -> SourceState {
    SourceState::Failed {
        reason: HealthReason::ThreadEnded,
        error: CaptureError::ThreadEnded,
    }
}

/// Spawn `body` on a named pump thread and block until it reports its thread id
/// through [`PumpReady`]. Returns once the pump is ready, so the caller's `stop`
/// always has a real [`PumpThreadId`] to post `WM_QUIT` to. If the body returns
/// before it reports (a setup failure), the sender drops, `recv` errors, and the
/// start fails instead of handing back a pump that never came up.
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
    // SAFETY: PostThreadMessageW takes a thread id and two plain values; a
    // thread with no queue or one that ended makes it fail and changes nothing.
    unsafe {
        let _ = PostThreadMessageW(thread_id.get(), WM_QUIT, WPARAM(0), LPARAM(0));
    }
}

/// Force this thread's message queue into existence, so a `WM_QUIT` posted by a
/// `stop` that runs the instant [`PumpReady::report`] returns is queued and later
/// delivered to [`pump_messages`]. `PostThreadMessageW` fails on a thread with no
/// queue; `PeekMessageW` is the documented way to create one.
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

/// Pump the current thread's message queue until `WM_QUIT` or the stop signal.
/// A WinEvent hook or a message-only window delivers its callbacks on the
/// thread that runs this loop.
pub(crate) fn pump_messages(signal: &StopSignal) {
    use windows::Win32::Foundation::WAIT_OBJECT_0;
    use windows::Win32::System::Threading::INFINITE;
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, MsgWaitForMultipleObjectsEx, PeekMessageW, TranslateMessage, MSG,
        MWMO_INPUTAVAILABLE, PM_REMOVE, QS_ALLINPUT, WM_QUIT,
    };
    let mut msg = MSG::default();
    loop {
        // SAFETY: `msg` is a live MSG that PeekMessageW fills, and the same
        // value goes to TranslateMessage and DispatchMessageW.
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    return;
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
            return;
        }
    }
}

/// Run the Windows sources for a bounded window and report per-modality counts.
pub fn timed_run(seconds: u64) -> Result<TimedRunReport, DaemonError> {
    let _ = seconds;
    Err(DaemonError::NotImplemented(
        "Windows functional capture (deferred to a permissioned interactive desktop)",
    ))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    pub(crate) fn serial() -> std::sync::MutexGuard<'static, ()> {
        SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(crate) fn test_placement() -> EventPlacement {
        EventPlacement {
            segment_ref: SegmentRef {
                segment_id: Uuid::now_v7(),
                byte_offset: 0,
                byte_len: 0,
            },
            segment_key_id: SegmentKeyId(1),
        }
    }

    #[test]
    fn unpersisted_placement_is_the_nil_sentinel_carrying_the_measured_len() {
        let placement = EventPlacement::unpersisted(128);
        assert!(placement.segment_ref.segment_id.is_nil());
        assert_eq!(placement.segment_ref.byte_offset, 0);
        assert_eq!(placement.segment_ref.byte_len, 128);
        assert_eq!(placement.segment_key_id, SegmentKeyId(0));
    }

    #[test]
    fn build_event_is_raw_and_carries_the_modality() {
        let (sink, _writer) = crate::sink::channel(8);
        let event = build_event(
            &sink,
            Modality::Keystroke,
            "C:\\Windows\\explorer.exe".to_string(),
            test_placement(),
            SystemTime::UNIX_EPOCH,
        );
        assert_eq!(event.modality, Modality::Keystroke);
        assert!(!event.is_derived);
        assert_eq!(event.retention_class, crate::event::RetentionClass::Raw);
        assert_eq!(event.foreground_app, "C:\\Windows\\explorer.exe");
    }

    #[test]
    fn build_event_takes_the_given_instant_and_a_fresh_monotonic_sequence() {
        let (sink, _writer) = crate::sink::channel(8);
        let at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let a = build_event(
            &sink,
            Modality::Pointer,
            String::new(),
            test_placement(),
            at,
        );
        let b = build_event(
            &sink,
            Modality::Pointer,
            String::new(),
            test_placement(),
            at,
        );
        assert!(a.captured_at.sequence < b.captured_at.sequence);
        assert_eq!(a.captured_at.wall_clock, at);
    }

    #[test]
    fn the_age_of_an_event_is_read_across_the_wrap_of_the_tick_count() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(100 * 86_400);
        let at = |now_tick, event_tick| {
            now.duration_since(instant_from_age(now, now_tick, event_tick))
                .expect("not after now")
        };
        assert_eq!(at(5_000, 4_750), Duration::from_millis(250));
        assert_eq!(at(100, u32::MAX - 149), Duration::from_millis(250));
        assert_eq!(at(4_750, 5_000), Duration::ZERO);
        assert_eq!(at(5_000, 5_000), Duration::ZERO);
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
        // The body reports its id and exits without a message loop. spawn_pump
        // must not return until report() runs, so the handle's id is the pump
        // thread's real id: never 0, never the starting thread's id.
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

    fn pointer(app: &str) -> Packet {
        Packet {
            kind: Modality::Pointer,
            app: app.to_string(),
            byte_len: 12,
            tick: now_tick(),
        }
    }

    fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
        let began = Instant::now();
        while !done() {
            assert!(began.elapsed() < Duration::from_secs(5), "{what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn a_full_queue_drops_a_packet_and_counts_it_lost() {
        let (sink, _writer) = crate::sink::channel(1);
        assert!(forward_one(&sink, pointer("filler")).is_ok());
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
                coalesced: 0
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
        producer.push_motion(pointer("after"));
        assert!(handoff.writer_gone());
        assert_eq!(handoff.counts().lost, 2);
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
    fn a_motion_sample_made_before_a_button_reaches_the_sink_before_it() {
        let (sink, writer) = crate::sink::channel(16);
        let handoff = HandOff::start(sink, "halvo-test-forward", Arc::default()).expect("hand-off");
        let producer = handoff.producer();
        producer.push_motion(pointer("motion-0"));
        let first = writer
            .recv_timeout(Duration::from_secs(2))
            .expect("the first motion sample is sent");
        assert_eq!(first.event().foreground_app, "motion-0");
        producer.push_motion(pointer("motion-1"));
        std::thread::sleep(Duration::from_millis(20));
        producer.push(pointer("button"));
        let mut order = Vec::new();
        for _ in 0..2 {
            let received = writer
                .recv_timeout(Duration::from_secs(2))
                .expect("both packets reach the sink");
            order.push(received.event().foreground_app.clone());
        }
        assert_eq!(order, ["motion-1", "button"]);
    }

    #[test]
    fn packets_that_a_stopped_run_still_held_count_after_a_restart() {
        let (sink, writer) = crate::sink::channel(1);
        assert!(forward_one(&sink, pointer("filler")).is_ok());
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
        let packet = Packet {
            kind: Modality::Keystroke,
            app: String::new(),
            byte_len: 2,
            tick: now_tick().wrapping_sub(250),
        };
        assert!(forward_one(&sink, packet).is_ok());
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
    fn with_the_sink_full_one_motion_sample_is_pending_and_no_button_packet_is_lost() {
        let (sink, writer) = crate::sink::channel(1);
        let handoff =
            HandOff::start(sink.clone(), "halvo-test-forward", Arc::default()).expect("hand-off");
        assert!(matches!(
            forward_one(&sink, pointer("filler")),
            Ok(EmitOutcome::Queued)
        ));
        let producer = handoff.producer();
        let (pushed, all_pushed) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            producer.push(pointer("button-1"));
            for _ in 0..1_000 {
                producer.push_motion(pointer("motion"));
            }
            producer.push(pointer("button-2"));
            for _ in 0..1_000 {
                producer.push_motion(pointer("motion"));
            }
            let _ = pushed.send(());
        });
        assert!(
            all_pushed
                .recv_timeout(std::time::Duration::from_secs(2))
                .is_ok(),
            "a push blocked on the full sink"
        );
        assert!(handoff.pending_motion(), "no motion sample is pending");
        let counts = handoff.counts();
        assert_eq!(counts.lost, 0);
        assert!(counts.coalesced >= 1_997, "{counts:?}");
        let mut apps = Vec::new();
        while let Ok(received) = writer.recv_timeout(std::time::Duration::from_secs(1)) {
            apps.push(received.event().foreground_app.clone());
        }
        let count = |app: &str| apps.iter().filter(|a| a.as_str() == app).count();
        assert_eq!(count("filler"), 1, "{apps:?}");
        assert_eq!(count("button-1"), 1, "{apps:?}");
        assert_eq!(count("button-2"), 1, "{apps:?}");
        let motions = count("motion");
        assert!((1..=3).contains(&motions), "{apps:?}");
        assert_eq!(apps.len(), 3 + motions);
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
