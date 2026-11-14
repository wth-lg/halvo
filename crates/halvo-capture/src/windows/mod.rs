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
    /// bytes.
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

pub(crate) use packet::{Observed, Packet};

mod packet {
    use crate::event::Modality;
    use crate::secure_input::KeystrokeClearance;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Observed {
        Pointer,
        ForegroundApp,
        WindowTitle,
        ScreenFrame,
    }

    pub(crate) struct Packet {
        kind: Modality,
        pub(crate) app: Option<String>,
        pub(crate) byte_len: u64,
        pub(crate) tick: u32,
    }

    impl Packet {
        /// ```compile_fail,E0603
        /// use halvo_capture::windows::Packet;
        /// ```
        /// ```
        /// use halvo_capture::windows::EventPlacement;
        /// let _ = EventPlacement::unpersisted(2);
        /// ```
        pub(crate) fn keystroke(
            clearance: KeystrokeClearance,
            app: Option<String>,
            byte_len: u64,
            tick: u32,
        ) -> Packet {
            let _ = clearance.key_age();
            Packet {
                kind: Modality::Keystroke,
                app,
                byte_len,
                tick,
            }
        }

        pub(crate) fn observed(
            kind: Observed,
            app: Option<String>,
            byte_len: u64,
            tick: u32,
        ) -> Packet {
            let kind = match kind {
                Observed::Pointer => Modality::Pointer,
                Observed::ForegroundApp => Modality::ForegroundApp,
                Observed::WindowTitle => Modality::WindowTitle,
                Observed::ScreenFrame => Modality::ScreenFrame,
            };
            Packet {
                kind,
                app,
                byte_len,
                tick,
            }
        }

        pub(crate) fn kind(&self) -> Modality {
            self.kind
        }
    }
}

#[link(name = "kernel32")]
extern "system" {
    fn GetTickCount() -> u32;
}

pub(crate) fn now_tick() -> u32 {
    // SAFETY: GetTickCount takes no argument and reads a counter.
    unsafe { GetTickCount() }
}

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
/// queue dropped, packets and the pending motion sample that the queue still
/// held when it was closed, and motion samples that a newer one replaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HandOffCounts {
    pub lost: u64,
    pub coalesced: u64,
}

pub(crate) const HANDOFF_CAPACITY: usize = 1_024;

pub(crate) const MOTION_INTERVAL: Duration = Duration::from_millis(100);

struct Queue {
    packets: VecDeque<Packet>,
    motion: Option<Packet>,
    closed: bool,
    writer_gone: bool,
}

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
        let mut queue = self.shared.lock();
        queue.closed = true;
        discard(&mut queue, &self.shared.counts);
        drop(queue);
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
            discard(&mut queue, &shared.counts);
            queue.closed = true;
            queue.writer_gone = true;
            return;
        }
    }
}

fn discard(queue: &mut Queue, counts: &LossCounts) {
    let held = queue.packets.len() + usize::from(queue.motion.is_some());
    counts.lose(held as u64);
    queue.packets.clear();
    queue.motion = None;
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
        packet.kind(),
        packet.app.unwrap_or_default(),
        placement,
        wall_clock,
    ))
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
}

impl Lifecycle {
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

pub(crate) fn app_of(cell: &Mutex<Option<String>>) -> Option<String> {
    cell.lock().ok().and_then(|app| app.clone())
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
    read_at_stop(STOP_READ_LIMIT);
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
        Packet::observed(Observed::Pointer, Some(app.to_string()), 12, now_tick())
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

    fn keystroke(app: &str) -> Packet {
        Packet::keystroke(cleared(), Some(app.to_string()), 2, now_tick())
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
            assert!(forward_one(&sink, keystroke("filler")).is_ok());
            let ctx = SourceContext::new(sink.clone());
            let mut life = Lifecycle::default();
            life.begin(&ctx, "halvo-test-forward-1", |producer| {
                for i in 1..=5 {
                    producer.push(keystroke(&format!("old-{i}")));
                }
                producer.push_motion(pointer("old-motion"));
                quiet_pump()
            })
            .expect("the first run");
            wait_for(|| life.handoff.as_ref().is_some_and(|h| h.queued() == 4));
            let first_stop = life.shutdown();
            let lost_at_the_stop = life.handoff_counts().lost;
            let filler = writer.recv().expect("the filler");
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
                arrivals.push((
                    received.event().foreground_app.clone(),
                    received.is_before(barrier),
                ));
            }
            let second_stop = life.shutdown();
            drop((life, ctx, sink));
            while let Ok(received) = writer.recv_timeout(Duration::from_secs(3_600)) {
                arrivals.push((
                    received.event().foreground_app.clone(),
                    received.is_before(barrier),
                ));
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
            assert_eq!(filler.foreground_app, "filler");
            assert_eq!(first_stop, Ok(()));
            assert_eq!(second_stop, Ok(()));
            assert_eq!(
                lost_at_the_stop, 5,
                "the four queued packets and the pending sample"
            );
        });
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
        assert!(forward_one(&sink, pointer("filler")).is_ok());
        let handoff = HandOff::start(sink, "halvo-test-forward", Arc::default()).expect("hand-off");
        let producer = handoff.producer();
        producer.push(pointer("parked"));
        wait_until("the forwarder took no packet", || handoff.queued() == 0);
        handoff.close();
        for _ in 0..3 {
            producer.push(pointer("after-close"));
        }
        producer.push_motion(pointer("after-close"));
        let lost = handoff.counts().lost;
        let queued = handoff.queued();
        let pending = handoff.pending_motion();
        let filler = writer.recv().expect("the filler");
        let mut apps = Vec::new();
        while let Ok(received) = writer.recv_timeout(Duration::from_secs(1)) {
            apps.push(received.event().foreground_app.clone());
        }
        assert_eq!(filler.foreground_app, "filler");
        assert_eq!(
            lost, 4,
            "each push into the closed hand-off is counted lost"
        );
        assert_eq!(queued, 0, "a push into the closed hand-off was queued");
        assert!(
            !pending,
            "a motion sample is pending in the closed hand-off"
        );
        assert_eq!(apps, ["parked"], "only the parked packet reaches the sink");
    }

    #[test]
    fn a_gone_writer_counts_the_queued_packets_and_the_pending_motion_sample() {
        let (sink, writer) = crate::sink::channel(1);
        assert!(forward_one(&sink, pointer("filler")).is_ok());
        let handoff = HandOff::start(sink, "halvo-test-forward", Arc::default()).expect("hand-off");
        let producer = handoff.producer();
        producer.push(pointer("parked"));
        wait_until("the forwarder took no packet", || handoff.queued() == 0);
        for _ in 0..3 {
            producer.push(pointer("queued"));
        }
        producer.push_motion(pointer("pending"));
        let queued = handoff.queued();
        let pending = handoff.pending_motion();
        drop(writer);
        wait_until("the forwarder did not see the gone writer", || {
            handoff.writer_gone()
        });
        assert_eq!(queued, 3);
        assert!(pending, "no motion sample was pending");
        assert_eq!(
            handoff.counts().lost,
            4,
            "the three queued packets and the pending motion sample"
        );
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
    fn motion_samples_reach_the_sink_at_most_once_in_100_ms() {
        let (sink, writer) = crate::sink::channel(4_096);
        let handoff = HandOff::start(sink, "halvo-test-forward", Arc::default()).expect("hand-off");
        let producer = handoff.producer();
        let began = Instant::now();
        let mut pushed = 0u32;
        while began.elapsed() < Duration::from_millis(350) {
            producer.push_motion(pointer("motion"));
            pushed += 1;
            std::thread::sleep(Duration::from_millis(1));
        }
        std::thread::sleep(Duration::from_millis(250));
        let mut sent = 0;
        while writer.try_recv().is_ok() {
            sent += 1;
        }
        assert!(pushed >= 20, "INCONCLUSIVE: {pushed} pushes in 350 ms");
        assert!(
            (3..=6).contains(&sent),
            "{sent} motion samples reached the sink for {pushed} pushes in 350 ms"
        );
    }

    #[test]
    fn an_event_carries_the_time_of_its_os_event() {
        let (sink, writer) = crate::sink::channel(4);
        let before = std::time::SystemTime::now();
        let packet = Packet::keystroke(cleared(), None, 2, now_tick().wrapping_sub(250));
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
