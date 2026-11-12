//! Windows capture backend.

pub mod apps;
pub mod audio;
pub mod foreground;
pub mod input;
pub mod screen;
pub mod window_title;

use crate::daemon::{DaemonError, TimedRunReport};
use crate::event::{CaptureStreamEvent, CapturedAt, Modality, SegmentKeyId, SegmentRef};
use crate::sink::SinkHandle;
use crate::source::CaptureError;
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

/// The readiness signal a pump body sends once it is ready to receive messages.
/// [`PumpReady::report`] hands the pump thread's id back to the starting thread,
/// which blocks on it in [`spawn_pump`], so a `stop` can never run before the
/// pump has recorded a real thread id.
#[cfg(target_os = "windows")]
pub(crate) struct PumpReady(std::sync::mpsc::Sender<PumpThreadId>);

#[cfg(target_os = "windows")]
impl PumpReady {
    /// Report this thread's id. Call once the message queue exists (after the
    /// window or hook is created) and the loop is about to run.
    pub(crate) fn report(&self) {
        let _ = self.0.send(PumpThreadId::current());
    }
}

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
        .map_err(|e| CaptureError::Start(format!("pump thread: {e}")))?;
    match rx.recv() {
        Ok(id) => Ok(PumpHandle { id, join }),
        Err(_) => {
            let _ = join.join();
            Err(CaptureError::Start(
                "pump thread exited before reporting its thread id".to_string(),
            ))
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
            ready.report();
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

    #[cfg(target_os = "windows")]
    #[test]
    fn spawn_pump_fails_when_the_body_never_reports() {
        let result = spawn_pump("halvo-pump-silent".to_string(), |_ready| {
            // never reports
        });
        assert!(
            matches!(result, Err(CaptureError::Start(_))),
            "a body that never reports must fail the start"
        );
    }
}
