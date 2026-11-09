//! Windows capture backend.

pub mod apps;
pub mod audio;
pub mod foreground;
pub mod input;
pub mod screen;
pub mod window_title;

use crate::daemon::{DaemonError, TimedRunReport};
use crate::event::{
    CaptureStreamEvent, CapturedAt, Modality, RetentionClass, SegmentKeyId, SegmentRef,
};
use crate::sink::SinkHandle;
use crate::source::CaptureError;

/// Where a captured payload's bytes live in the encrypted segment store: the
/// segment ref plus the per-segment key id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventPlacement {
    pub segment_ref: SegmentRef,
    pub segment_key_id: SegmentKeyId,
}

/// The store-write seam the capture sources depend on but do not implement. A
/// source hands its raw payload here and gets back the [`EventPlacement`] to put
/// on the stream event.
pub trait PlacementSink: Send + Sync {
    /// Persist `payload` under `retention` and return where it landed.
    fn place(
        &self,
        payload: &[u8],
        retention: RetentionClass,
    ) -> Result<EventPlacement, CaptureError>;
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

#[cfg(target_os = "windows")]
pub(crate) fn current_thread_id() -> u32 {
    unsafe { windows::Win32::System::Threading::GetCurrentThreadId() }
}

#[cfg(target_os = "windows")]
pub(crate) fn post_quit_to(thread_id: u32) {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};
    if thread_id != 0 {
        unsafe {
            let _ = PostThreadMessageW(thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
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
mod tests {
    use super::*;
    use uuid::Uuid;

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
        assert_eq!(event.retention_class, RetentionClass::Raw);
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
}
