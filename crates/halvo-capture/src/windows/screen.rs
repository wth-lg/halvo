//! Screen capture via Windows.Graphics.Capture (`FrameArrived`).

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

use crate::event::{CaptureStreamEvent, Modality};
use crate::sink::{SinkError, SinkHandle};
use crate::source::{CaptureError, CaptureSource, EmitOutcome, EventSink};

use super::{build_event, EventPlacement};

/// The default minimum interval between captured frames.
pub const DEFAULT_MIN_FRAME_INTERVAL: Duration = Duration::from_millis(500);

/// Bytes per pixel of the captured `Bgra8` frame, used to measure the frame's
/// payload size for the metadata-only placement.
const BGRA8_BYTES_PER_PIXEL: u64 = 4;

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
    pub last_app: Arc<Mutex<Option<String>>>,
}

/// The `windows-capture` frame handler. Built by the capture thread from the
/// flags; `on_frame_arrived` emits one event per frame and buffers nothing.
pub struct ScreenHandler {
    sink: SinkHandle,
    last_app: Arc<Mutex<Option<String>>>,
}

impl GraphicsCaptureApiHandler for ScreenHandler {
    type Flags = ScreenFlags;
    type Error = CaptureError;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self {
            sink: ctx.flags.sink,
            last_app: ctx.flags.last_app,
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        _control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        let (width, height) = (frame.width(), frame.height());
        let app = self
            .last_app
            .lock()
            .ok()
            .and_then(|g| g.clone())
            .unwrap_or_default();
        // Metadata-only: no pixels are read or held (the memory bound), but the
        // frame's measured byte size rides in the placement so the dimensions are
        // not lost. P6 writes the pixels to the store and assigns a real placement.
        let byte_len = width as u64 * height as u64 * BGRA8_BYTES_PER_PIXEL;
        let placement = EventPlacement::unpersisted(byte_len);
        if let Some(event) = map_frame(&self.sink, width, height, app, placement) {
            match self.sink.emit(event) {
                Ok(EmitOutcome::Queued | EmitOutcome::Dropped) => {}
                Err(SinkError::Disconnected) => {
                    return Err(CaptureError::Start(
                        "screen sink disconnected; the capture writer has stopped".to_string(),
                    ))
                }
            }
        }
        Ok(())
    }
}

/// Screen capture source over the primary monitor. `start` opens a capture
/// session on its own thread; `stop` ends it.
pub struct ScreenSource {
    last_app: Arc<Mutex<Option<String>>>,
    min_interval: Duration,
    control: Option<windows_capture::capture::CaptureControl<ScreenHandler, CaptureError>>,
}

impl ScreenSource {
    pub fn new(last_app: Arc<Mutex<Option<String>>>) -> Self {
        Self {
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
