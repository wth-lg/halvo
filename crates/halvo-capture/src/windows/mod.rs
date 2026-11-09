//! Windows capture backend.
//!
//! Each modality's source lives in its own file and emits the one
//! [`crate::event::CaptureStreamEvent`] through an [`crate::sink::SinkHandle`]:
//!
//! - [`screen`]: Windows.Graphics.Capture `FrameArrived` (the `windows-capture`
//!   crate), frame-rate throttled so no window of frames is ever buffered.
//! - [`input`]: keystroke and pointer from Raw Input (`WM_INPUT`), a listen-only
//!   path the OS withholds across the UIPI boundary and the secure desktop.
//! - [`foreground`]: the foreground app from `SetWinEventHook`
//!   (`EVENT_SYSTEM_FOREGROUND`).
//! - [`window_title`]: the focused window title from `EVENT_OBJECT_NAMECHANGE`.
//! - [`audio`]: system audio, present but off by default.
//! - [`apps`]: the installed-software inventory from the registry (a scan, not a
//!   stream event; see [`apps`]).
//!
//! **Functional capture is deferred.** Every source here type-checks for the
//! Windows target and its mapping logic is unit-tested without a desktop, which
//! is what CI confirms. Driving the OS callbacks needs an interactive logon
//! session, so [`timed_run`] reports the deferral rather than pretend to run; the
//! per-source wiring is real code, exercised on a permissioned machine, not in
//! CI. The measured Windows memory number is deferred with it.

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
/// segment ref plus the per-segment key id. A source obtains one from a
/// [`PlacementSink`] after the payload is written, then stamps it onto the event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventPlacement {
    pub segment_ref: SegmentRef,
    pub segment_key_id: SegmentKeyId,
}

/// The store-write seam the capture sources depend on but do not implement. A
/// source hands its raw payload here and gets back the [`EventPlacement`] to put
/// on the stream event. The real, store-backed implementation is the daemon
/// wiring (phase P6); the capture backend only needs the seam so its sources are
/// constructible and the mapping stays pure. The cross-platform home for this
/// seam is likely [`crate::source`], but that file is owned by an earlier phase.
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
/// mark it raw. Every Windows source funnels through this, so the event shape is
/// identical across modalities and set in exactly one place.
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

/// The current OS thread id, stored by a source so [`post_quit_to`] can stop its
/// pump from another thread.
#[cfg(target_os = "windows")]
pub(crate) fn current_thread_id() -> u32 {
    unsafe { windows::Win32::System::Threading::GetCurrentThreadId() }
}

/// Post `WM_QUIT` to a source's pump thread so its message loop returns. A zero
/// thread id (the pump never recorded one) is ignored.
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
///
/// Functional capture is deferred to a machine that can grant an interactive
/// logon session, so this reports the deferral instead of running a capture that
/// a headless host would silently record nothing from. The sources and their
/// mapping are compiled and unit-tested; only the timed functional run is
/// deferred, together with the measured Windows memory number.
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
