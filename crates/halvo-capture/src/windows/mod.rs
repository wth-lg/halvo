//! Windows capture backend.

pub mod apps;
pub mod audio;
pub mod foreground;
pub mod input;
pub mod screen;
pub mod window_title;

use crate::daemon::{DaemonError, TimedRunReport};
use crate::event::{CaptureStreamEvent, CapturedAt, Modality, SegmentKeyId, SegmentRef};
use crate::sink::{SinkError, SinkHandle};
use crate::source::{CaptureError, EmitOutcome, EventSink};
use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};
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

/// Build the one raw stream event for a captured payload: stamp the sink's next
/// monotonic sequence, record the foreground app and the store placement, and
/// mark it raw.
pub fn build_event(
    sink: &SinkHandle,
    modality: Modality,
    foreground_app: String,
    placement: EventPlacement,
) -> CaptureStreamEvent {
    let captured_at = CapturedAt {
        wall_clock: std::time::SystemTime::now(),
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
    #[allow(dead_code)]
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
    counts: HandOffCounts,
}

struct Shared {
    queue: Mutex<Queue>,
    wake: Condvar,
    sink: SinkHandle,
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
    pub(crate) fn start(sink: SinkHandle, name: &str) -> Result<Self, CaptureError> {
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue {
                packets: VecDeque::new(),
                motion: None,
                closed: false,
                writer_gone: false,
                counts: HandOffCounts::default(),
            }),
            wake: Condvar::new(),
            sink,
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

    pub(crate) fn counts(&self) -> HandOffCounts {
        self.shared.lock().counts
    }

    pub(crate) fn close(&self) {
        self.shared.lock().closed = true;
        self.shared.wake.notify_all();
    }

    #[cfg(test)]
    fn pending_motion(&self) -> bool {
        self.shared.lock().motion.is_some()
    }
}

impl Drop for HandOff {
    fn drop(&mut self) {
        self.close();
    }
}

impl Producer {
    /// Queue a packet that is never collapsed: a key, a button, a title.
    pub(crate) fn push(&self, packet: Packet) {
        let mut queue = self.shared.lock();
        if queue.closed || queue.packets.len() >= HANDOFF_CAPACITY {
            queue.counts.lost += 1;
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
            queue.counts.lost += 1;
            return;
        }
        if queue.motion.replace(packet).is_some() {
            queue.counts.coalesced += 1;
        }
        drop(queue);
        self.shared.wake.notify_one();
    }

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
            queue.counts.lost += left as u64;
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
    sink.emit(build_event(sink, packet.kind, packet.app, placement))
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

/// A started pump thread: its reported id and its join handle. `stop` posts
/// `WM_QUIT` to the recorded id and joins, so teardown always targets the real
/// pump thread.
#[cfg(target_os = "windows")]
pub(crate) struct PumpHandle {
    id: PumpThreadId,
    join: std::thread::JoinHandle<()>,
}

#[cfg(target_os = "windows")]
impl PumpHandle {
    pub(crate) fn stop(self) {
        post_quit_to(self.id);
        let _ = self.join.join();
    }
}

/// Spawn `body` on a named pump thread and block until it reports its thread id
/// through [`PumpReady`]. Returns once the pump is ready, so the caller's `stop`
/// always has a real [`PumpThreadId`] to post `WM_QUIT` to. If the body returns
/// before it reports (a setup failure), the sender drops, `recv` errors, and the
/// start fails instead of handing back a pump that never came up.
#[cfg(target_os = "windows")]
pub(crate) fn spawn_pump<F>(name: String, body: F) -> Result<PumpHandle, CaptureError>
where
    F: FnOnce(PumpReady) + Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel();
    let join = std::thread::Builder::new()
        .name(name)
        .spawn(move || body(PumpReady(tx)))
        .map_err(|e| CaptureError::Os {
            stage: "spawn the pump thread",
            domain: "io",
            code: i64::from(e.raw_os_error().unwrap_or(0)),
        })?;
    match rx.recv() {
        Ok(Ok(id)) => Ok(PumpHandle { id, join }),
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

#[cfg(target_os = "windows")]
pub(crate) fn post_quit_to(thread_id: PumpThreadId) {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};
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

/// Pump the current thread's message queue until `WM_QUIT`. A WinEvent hook or a
/// message-only window delivers its callbacks on the thread that runs this loop.
#[cfg(target_os = "windows")]
pub(crate) fn pump_messages() {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, TranslateMessage, MSG,
    };
    let mut msg = MSG::default();
    unsafe {
        // GetMessageW returns 0 on WM_QUIT and -1 on error; either ends the pump.
        while GetMessageW(&mut msg, Some(HWND(std::ptr::null_mut())), 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
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
        );
        assert_eq!(event.modality, Modality::Keystroke);
        assert!(!event.is_derived);
        assert_eq!(event.retention_class, crate::event::RetentionClass::Raw);
        assert_eq!(event.foreground_app, "C:\\Windows\\explorer.exe");
    }

    #[test]
    fn build_event_takes_a_fresh_monotonic_sequence() {
        let (sink, _writer) = crate::sink::channel(8);
        let a = build_event(&sink, Modality::Pointer, String::new(), test_placement());
        let b = build_event(&sink, Modality::Pointer, String::new(), test_placement());
        assert!(a.captured_at.sequence < b.captured_at.sequence);
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
        let handle = spawn_pump("halvo-pump-test".to_string(), |ready| {
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
        // stop() posts WM_QUIT to the (now-exited) pump thread and joins; a post
        // to a finished thread fails harmlessly, so this must not hang or panic.
        handle.stop();
    }

    fn pointer(app: &str) -> Packet {
        Packet {
            kind: Modality::Pointer,
            app: app.to_string(),
            byte_len: 12,
            tick: now_tick(),
        }
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
        let handoff = HandOff::start(sink.clone(), "halvo-test-forward").expect("hand-off");
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

    #[cfg(target_os = "windows")]
    #[test]
    fn spawn_pump_fails_when_the_body_never_reports() {
        let result = spawn_pump("halvo-pump-silent".to_string(), |_ready| {
            // never reports
        });
        assert!(
            matches!(result, Err(CaptureError::Start(ref text)) if text == PUMP_ENDED_UNREPORTED),
            "a body that never reports must fail the start"
        );
        let failed = spawn_pump("halvo-pump-failed".to_string(), |ready| {
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
