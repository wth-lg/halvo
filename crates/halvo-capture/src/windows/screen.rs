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

use crate::event::Modality;
use crate::sink::SinkHandle;
use crate::source::{CaptureError, CaptureSource};

use super::{HandOff, HandOffCounts, Packet, Producer};

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

const SINK_GONE: &str = "the capture writer has stopped";

/// The packet of a frame of the given size, or `None` for a zero-area frame,
/// which carries no image. No pixel is read: the size is the frame's byte
/// count in Bgra8.
pub(crate) fn frame_packet(width: u32, height: u32, foreground_app: String) -> Option<Packet> {
    if width == 0 || height == 0 {
        return None;
    }
    Some(Packet {
        kind: Modality::ScreenFrame,
        app: foreground_app,
        byte_len: u64::from(width) * u64::from(height) * BGRA8_BYTES_PER_PIXEL,
    })
}

/// What the capture handler needs, passed through the settings flags.
pub struct ScreenFlags {
    producer: Producer,
    last_app: Arc<Mutex<Option<String>>>,
}

/// The `windows-capture` frame handler: one packet for each frame, handed to
/// the source's queue.
pub struct ScreenHandler {
    producer: Producer,
    last_app: Arc<Mutex<Option<String>>>,
}

impl GraphicsCaptureApiHandler for ScreenHandler {
    type Flags = ScreenFlags;
    type Error = CaptureError;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self {
            producer: ctx.flags.producer,
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
        if let Some(packet) = frame_packet(width, height, app) {
            self.producer.push(packet);
        }
        if self.producer.writer_gone() {
            return Err(CaptureError::Start(SINK_GONE.to_string()));
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
    handoff: Option<HandOff>,
}

impl ScreenSource {
    pub fn new(last_app: Arc<Mutex<Option<String>>>) -> Self {
        Self {
            last_app,
            min_interval: DEFAULT_MIN_FRAME_INTERVAL,
            control: None,
            handoff: None,
        }
    }

    /// Override the minimum interval between frames (the frame-rate throttle).
    pub fn with_min_interval(mut self, interval: Duration) -> Self {
        self.min_interval = interval;
        self
    }

    /// The counts of this source's hand-off queue in its last run.
    pub fn handoff_counts(&self) -> HandOffCounts {
        self.handoff
            .as_ref()
            .map(HandOff::counts)
            .unwrap_or_default()
    }
}

impl CaptureSource for ScreenSource {
    fn modality(&self) -> Modality {
        Modality::ScreenFrame
    }

    fn start(&mut self, sink: SinkHandle) -> Result<(), CaptureError> {
        let monitor =
            Monitor::primary().map_err(|e| CaptureError::Start(format!("primary monitor: {e}")))?;
        let handoff = HandOff::start(sink, "halvo-screen-forward")?;
        let flags = ScreenFlags {
            producer: handoff.producer(),
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
        self.handoff = Some(handoff);
        Ok(())
    }

    fn stop(&mut self) {
        if let Some(control) = self.control.take() {
            let _ = control.stop();
        }
        if let Some(handoff) = &self.handoff {
            handoff.close();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_maps_to_a_screen_frame_packet_of_its_byte_count() {
        let packet = frame_packet(1920, 1080, "C:\\browser.exe".to_string())
            .expect("a sized frame produces a packet");
        assert_eq!(packet.kind, Modality::ScreenFrame);
        assert_eq!(packet.app, "C:\\browser.exe");
        assert_eq!(packet.byte_len, 1920 * 1080 * 4);
    }

    #[test]
    fn a_zero_area_frame_is_not_recorded() {
        assert!(frame_packet(0, 1080, String::new()).is_none());
        assert!(frame_packet(1920, 0, String::new()).is_none());
    }
}
