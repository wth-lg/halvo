//! Screen capture via Windows.Graphics.Capture (`FrameArrived`).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE};
use windows_capture::capture::{
    CaptureControl, Context, GraphicsCaptureApiError, GraphicsCaptureApiHandler,
};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::monitor::Monitor;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
    MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};

use crate::event::Modality;
use crate::source::{
    CaptureError, CaptureSource, SourceContext, SourceCounters, SourceHealth, SourceState, Started,
    StopCause,
};

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
const NO_PRIMARY_MONITOR: &str = "the primary monitor could not be read";
const SCREEN_START_FAILED: &str = "the screen capture could not start";

/// The packet of a frame of the given size, or `None` for a zero-area frame,
/// which carries no image. No pixel is read: the size is the frame's byte
/// count in Bgra8.
pub(crate) fn frame_packet(
    width: u32,
    height: u32,
    foreground_app: String,
    tick: u32,
) -> Option<Packet> {
    if width == 0 || height == 0 {
        return None;
    }
    Some(Packet {
        kind: Modality::ScreenFrame,
        app: foreground_app,
        byte_len: u64::from(width) * u64::from(height) * BGRA8_BYTES_PER_PIXEL,
        tick,
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
        if let Some(packet) = frame_packet(width, height, app, super::now_tick()) {
            self.producer.push(packet);
        }
        if self.producer.writer_gone() {
            return Err(CaptureError::Start(SINK_GONE.to_string()));
        }
        Ok(())
    }
}

type Control = CaptureControl<ScreenHandler, CaptureError>;

/// Screen capture source over the primary monitor, read again at each `begin`.
/// `begin` opens a capture session on its own thread; `shutdown` ends it
/// inside [`super::SCREEN_STOP_DEADLINE`], and dropping the source does the
/// same.
pub struct ScreenSource {
    last_app: Arc<Mutex<Option<String>>>,
    min_interval: Duration,
    control: Option<Control>,
    handoff: Option<HandOff>,
    stopped: bool,
}

impl ScreenSource {
    pub fn new(last_app: Arc<Mutex<Option<String>>>) -> Self {
        Self {
            last_app,
            min_interval: DEFAULT_MIN_FRAME_INTERVAL,
            control: None,
            handoff: None,
            stopped: false,
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
    fn begin(&mut self, ctx: SourceContext) -> Result<Started, CaptureError> {
        if self.control.is_some() {
            return Err(CaptureError::AlreadyStarted);
        }
        let monitor = Monitor::primary().map_err(monitor_error)?;
        let handoff = HandOff::start(ctx.sink().clone(), "halvo-screen-forward")?;
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
        let control = ScreenHandler::start_free_threaded(settings).map_err(start_error)?;
        self.control = Some(control);
        self.handoff = Some(handoff);
        self.stopped = false;
        Ok(Started::Running)
    }

    fn shutdown(&mut self) -> Result<(), CaptureError> {
        let Some(control) = self.control.take() else {
            return Ok(());
        };
        let stopped = stop_within(control, super::SCREEN_STOP_DEADLINE);
        if let Some(handoff) = &self.handoff {
            handoff.close();
        }
        self.stopped = true;
        stopped
    }

    fn health(&self) -> SourceHealth {
        let writer_gone = self.handoff.as_ref().is_some_and(HandOff::writer_gone);
        let state = match &self.control {
            Some(control) if control.is_finished() || writer_gone => super::thread_ended(),
            Some(_) => SourceState::Running,
            None if self.stopped => SourceState::Stopped(StopCause::Request),
            None => SourceState::NotReported,
        };
        SourceHealth {
            state,
            counters: SourceCounters::default(),
        }
    }
}

impl Drop for ScreenSource {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

/// Stop the capture on a thread of its own and wait for it up to `deadline`:
/// `CaptureControl::stop` joins the capture thread without a bound.
fn stop_within(control: Control, deadline: Duration) -> Result<(), CaptureError> {
    let ended_before = control.is_finished();
    let (done, stopped) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("halvo-screen-stop".to_string())
        .spawn(move || {
            let _ = done.send(control.stop().is_ok());
        })
        .map_err(|e| CaptureError::Os {
            stage: "spawn the screen stop thread",
            domain: "io",
            code: i64::from(e.raw_os_error().unwrap_or(0)),
        })?;
    match stopped.recv_timeout(deadline) {
        Ok(true) if !ended_before => Ok(()),
        Ok(_) => Err(CaptureError::ThreadEnded),
        Err(_) => Err(CaptureError::Timeout {
            stage: "stop",
            ms: u64::try_from(deadline.as_millis()).unwrap_or(u64::MAX),
        }),
    }
}

fn monitor_error(error: windows_capture::monitor::Error) -> CaptureError {
    match error {
        windows_capture::monitor::Error::WindowsError(e) => super::os_error("primary monitor", &e),
        _ => CaptureError::Start(NO_PRIMARY_MONITOR.to_string()),
    }
}

fn start_error(error: GraphicsCaptureApiError<CaptureError>) -> CaptureError {
    use windows_capture::graphics_capture_api::Error as Api;
    match error {
        GraphicsCaptureApiError::NewHandlerError(e)
        | GraphicsCaptureApiError::FrameHandlerError(e) => e,
        GraphicsCaptureApiError::GraphicsCaptureApiError(Api::WindowsError(e)) => {
            super::os_error("start the screen capture", &e)
        }
        GraphicsCaptureApiError::DirectXError(windows_capture::d3d11::Error::WindowsError(e)) => {
            super::os_error("create the Direct3D device", &e)
        }
        _ => CaptureError::Start(SCREEN_START_FAILED.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_maps_to_a_screen_frame_packet_of_its_byte_count() {
        let packet = frame_packet(1920, 1080, "C:\\browser.exe".to_string(), 1)
            .expect("a sized frame produces a packet");
        assert_eq!(packet.kind, Modality::ScreenFrame);
        assert_eq!(packet.app, "C:\\browser.exe");
        assert_eq!(packet.byte_len, 1920 * 1080 * 4);
    }

    #[test]
    fn a_zero_area_frame_is_not_recorded() {
        assert!(frame_packet(0, 1080, String::new(), 1).is_none());
        assert!(frame_packet(1920, 0, String::new(), 1).is_none());
    }
}
