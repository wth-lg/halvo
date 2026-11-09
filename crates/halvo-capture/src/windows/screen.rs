//! Screen capture via Windows.Graphics.Capture (`FrameArrived`).
//!
//! The `windows-capture` crate wraps Windows.Graphics.Capture and calls
//! `on_frame_arrived` for each composed frame (researcher-1 #8). Two properties
//! hold by construction:
//!
//! - **Own windows are excluded.** A window that sets
//!   `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)` is omitted from the
//!   captured surface, so Halvo never records its own overlay
//!   ([`exclude_window_from_capture`]). DRM-protected media composes black.
//! - **Frames are not buffered.** The handler reads a frame's metadata and emits
//!   one event, holding no queue of frames. Under backpressure the sink drops
//!   screen frames (they are the droppable modality), and a minimum update
//!   interval throttles the frame rate, so resident memory stays bounded. The
//!   measured Windows memory number is deferred with functional capture.
//!
//! The frame-to-event mapping is pure and unit tested; the capture session is
//! compiled and type-checked here and run on a real desktop (deferred).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE};
use windows_capture::capture::{Context, GraphicsCaptureApiHandler};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::monitor::Monitor;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
    MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};

use crate::event::{CaptureStreamEvent, Modality, RetentionClass};
use crate::sink::SinkHandle;
use crate::source::{CaptureError, CaptureSource, EventSink};

use super::{build_event, EventPlacement, PlacementSink};

/// The default minimum interval between captured frames. Throttling the frame
/// rate bounds CPU and the rate of payloads the store must absorb; the exact
/// value is a tuning parameter the measured run will set (deferred).
pub const DEFAULT_MIN_FRAME_INTERVAL: Duration = Duration::from_millis(500);

/// Exclude a window from screen capture, so Halvo never records its own windows.
/// Call on each Halvo top-level window.
pub fn exclude_window_from_capture(hwnd: HWND) -> Result<(), CaptureError> {
    unsafe { SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE) }
        .map_err(|e| CaptureError::Start(format!("WDA_EXCLUDEFROMCAPTURE: {e}")))
}

/// Build the screen-frame event for a frame of the given size, or `None` for a
/// degenerate zero-area frame (which carries no image and is not recorded).
pub fn map_frame(
    sink: &SinkHandle,
    width: u32,
    height: u32,
    foreground_app: String,
    placement: EventPlacement,
) -> Option<CaptureStreamEvent> {
    if width == 0 || height == 0 {
        return None;
    }
    Some(build_event(
        sink,
        Modality::ScreenFrame,
        foreground_app,
        placement,
    ))
}

/// The dependencies the capture handler needs, passed through the settings flags.
pub struct ScreenFlags {
    pub sink: SinkHandle,
    pub placement: Arc<dyn PlacementSink>,
    pub last_app: Arc<Mutex<String>>,
}

/// The `windows-capture` frame handler. Built by the capture thread from the
/// flags; `on_frame_arrived` emits one event per frame and buffers nothing.
pub struct ScreenHandler {
    sink: SinkHandle,
    placement: Arc<dyn PlacementSink>,
    last_app: Arc<Mutex<String>>,
}

impl GraphicsCaptureApiHandler for ScreenHandler {
    type Flags = ScreenFlags;
    type Error = CaptureError;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self {
            sink: ctx.flags.sink,
            placement: ctx.flags.placement,
            last_app: ctx.flags.last_app,
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        _control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        let (width, height) = (frame.width(), frame.height());
        let app = self.last_app.lock().map(|g| g.clone()).unwrap_or_default();
        // Metadata only: the frame's pixels stream to the store through the
        // placement sink (deferred). No frame is held past this call, so no window
        // of frames accumulates in memory.
        let placement = self.placement.place(&[], RetentionClass::Raw)?;
        if let Some(event) = map_frame(&self.sink, width, height, app, placement) {
            // ScreenFrame is droppable: a full sink drops the frame rather than
            // block capture, which is what keeps memory bounded under load.
            let _ = self.sink.emit(event);
        }
        Ok(())
    }
}

/// Screen capture source over the primary monitor. `start` opens a capture
/// session on its own thread; `stop` ends it.
pub struct ScreenSource {
    placement: Arc<dyn PlacementSink>,
    last_app: Arc<Mutex<String>>,
    min_interval: Duration,
    control: Option<windows_capture::capture::CaptureControl<ScreenHandler, CaptureError>>,
}

impl ScreenSource {
    pub fn new(placement: Arc<dyn PlacementSink>, last_app: Arc<Mutex<String>>) -> Self {
        Self {
            placement,
            last_app,
            min_interval: DEFAULT_MIN_FRAME_INTERVAL,
            control: None,
        }
    }

    /// Override the minimum interval between frames (the frame-rate throttle).
    pub fn with_min_interval(mut self, interval: Duration) -> Self {
        self.min_interval = interval;
        self
    }
}

impl CaptureSource for ScreenSource {
    fn modality(&self) -> Modality {
        Modality::ScreenFrame
    }

    fn start(&mut self, sink: SinkHandle) -> Result<(), CaptureError> {
        let monitor =
            Monitor::primary().map_err(|e| CaptureError::Start(format!("primary monitor: {e}")))?;
        let flags = ScreenFlags {
            sink,
            placement: Arc::clone(&self.placement),
            last_app: Arc::clone(&self.last_app),
        };
        let settings = Settings::new(
            monitor,
            CursorCaptureSettings::WithoutCursor,
            DrawBorderSettings::Default,
            SecondaryWindowSettings::Default,
            MinimumUpdateIntervalSettings::Custom(self.min_interval),
            DirtyRegionSettings::Default,
            ColorFormat::Bgra8,
            flags,
        );
        let control = ScreenHandler::start_free_threaded(settings)
            .map_err(|e| CaptureError::Start(format!("screen capture: {e}")))?;
        self.control = Some(control);
        Ok(())
    }

    fn stop(&mut self) {
        if let Some(control) = self.control.take() {
            let _ = control.stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::windows::tests::test_placement;

    #[test]
    fn a_frame_maps_to_a_droppable_screen_frame_event() {
        let (sink, _w) = crate::sink::channel(4);
        let event = map_frame(
            &sink,
            1920,
            1080,
            "C:\\browser.exe".to_string(),
            test_placement(),
        )
        .expect("a sized frame produces an event");
        assert_eq!(event.modality, Modality::ScreenFrame);
        assert!(event.modality.is_droppable_under_backpressure());
    }

    #[test]
    fn a_zero_area_frame_is_not_recorded() {
        let (sink, _w) = crate::sink::channel(4);
        assert!(map_frame(&sink, 0, 1080, String::new(), test_placement()).is_none());
        assert!(map_frame(&sink, 1920, 0, String::new(), test_placement()).is_none());
    }
}
