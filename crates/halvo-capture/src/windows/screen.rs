//! Screen capture via Windows.Graphics.Capture (`FrameArrived`).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE};
use windows_capture::capture::{
    CaptureControl, Context, GraphicsCaptureApiError, GraphicsCaptureApiHandler,
};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::{GraphicsCaptureApi, InternalCaptureControl};
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
const CURSOR_FLOOR: &str =
    "screen capture without the cursor needs Windows 10 build 19041 or later";

/// What the running Windows build supports, as the capture settings need it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CaptureSupport {
    pub(crate) minimum_update_interval: bool,
    pub(crate) cursor_setting: bool,
}

/// The capture settings that the build allows: the interval to ask for, and
/// the interval within which the handler drops a frame when the build takes
/// no minimum update interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScreenChoice {
    pub(crate) interval: MinimumUpdateIntervalSettings,
    pub(crate) drop_within: Option<Duration>,
}

fn probe_support() -> Result<CaptureSupport, CaptureError> {
    Ok(CaptureSupport {
        minimum_update_interval: GraphicsCaptureApi::is_minimum_update_interval_supported()
            .map_err(|e| api_error("probe the minimum update interval", e))?,
        cursor_setting: GraphicsCaptureApi::is_cursor_settings_supported()
            .map_err(|e| api_error("probe the cursor setting", e))?,
    })
}

/// Choose the settings from what `probe` answers, asked once.
pub(crate) fn choose_settings(
    probe: impl FnOnce() -> Result<CaptureSupport, CaptureError>,
    min_interval: Duration,
) -> Result<ScreenChoice, CaptureError> {
    let support = probe()?;
    if !support.cursor_setting {
        return Err(CaptureError::Start(CURSOR_FLOOR.to_string()));
    }
    if support.minimum_update_interval {
        return Ok(ScreenChoice {
            interval: MinimumUpdateIntervalSettings::Custom(min_interval),
            drop_within: None,
        });
    }
    Ok(ScreenChoice {
        interval: MinimumUpdateIntervalSettings::Default,
        drop_within: Some(min_interval),
    })
}

/// Whether the handler keeps a frame that arrives at `now`, given the last
/// frame that it kept.
fn keep_frame(
    drop_within: Option<Duration>,
    last_kept: &mut Option<Instant>,
    now: Instant,
) -> bool {
    let Some(within) = drop_within else {
        return true;
    };
    if last_kept.is_some_and(|at| now.saturating_duration_since(at) < within) {
        return false;
    }
    *last_kept = Some(now);
    true
}
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
    drop_within: Option<Duration>,
    closed: Arc<AtomicBool>,
}

/// The `windows-capture` frame handler: one packet for each frame that it
/// keeps, handed to the source's queue.
pub struct ScreenHandler {
    producer: Producer,
    last_app: Arc<Mutex<Option<String>>>,
    drop_within: Option<Duration>,
    last_kept: Option<Instant>,
    #[allow(dead_code)]
    closed: Arc<AtomicBool>,
}

impl GraphicsCaptureApiHandler for ScreenHandler {
    type Flags = ScreenFlags;
    type Error = CaptureError;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self {
            producer: ctx.flags.producer,
            last_app: ctx.flags.last_app,
            drop_within: ctx.flags.drop_within,
            last_kept: None,
            closed: ctx.flags.closed,
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        _control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        if !keep_frame(self.drop_within, &mut self.last_kept, Instant::now()) {
            return Ok(());
        }
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
    closed: Arc<AtomicBool>,
    stopped: bool,
}

impl ScreenSource {
    pub fn new(last_app: Arc<Mutex<Option<String>>>) -> Self {
        Self {
            last_app,
            min_interval: DEFAULT_MIN_FRAME_INTERVAL,
            control: None,
            handoff: None,
            closed: Arc::default(),
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
        let choice = choose_settings(probe_support, self.min_interval)?;
        let monitor = Monitor::primary().map_err(monitor_error)?;
        let handoff = HandOff::start(ctx.sink().clone(), "halvo-screen-forward")?;
        let closed = Arc::new(AtomicBool::new(false));
        let flags = ScreenFlags {
            producer: handoff.producer(),
            last_app: Arc::clone(&self.last_app),
            drop_within: choice.drop_within,
            closed: Arc::clone(&closed),
        };
        let settings = Settings::new(
            monitor,
            CursorCaptureSettings::WithoutCursor,
            DrawBorderSettings::Default,
            SecondaryWindowSettings::Default,
            choice.interval,
            DirtyRegionSettings::Default,
            ColorFormat::Bgra8,
            flags,
        );
        let control = ScreenHandler::start_free_threaded(settings).map_err(start_error)?;
        self.control = Some(control);
        self.handoff = Some(handoff);
        self.closed = closed;
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
        SourceHealth {
            state: screen_state(
                self.control.as_ref().map(Control::is_finished),
                self.closed.load(Ordering::Relaxed),
                self.handoff.as_ref().is_some_and(HandOff::writer_gone),
                self.stopped,
            ),
            counters: SourceCounters::default(),
        }
    }
}

impl Drop for ScreenSource {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

/// The state of a screen source: `session` is `None` with no session, or
/// whether the session's capture thread has finished.
fn screen_state(
    session: Option<bool>,
    item_closed: bool,
    writer_gone: bool,
    stopped: bool,
) -> SourceState {
    match session {
        Some(finished) if finished || item_closed || writer_gone => super::thread_ended(),
        Some(_) => SourceState::Running,
        None if stopped => SourceState::Stopped(StopCause::Request),
        None => SourceState::NotReported,
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

fn api_error(
    stage: &'static str,
    error: windows_capture::graphics_capture_api::Error,
) -> CaptureError {
    match error {
        windows_capture::graphics_capture_api::Error::WindowsError(e) => super::os_error(stage, &e),
        _ => CaptureError::Start(SCREEN_START_FAILED.to_string()),
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

    fn asked(
        support: CaptureSupport,
    ) -> (
        Result<ScreenChoice, CaptureError>,
        std::rc::Rc<std::cell::Cell<u32>>,
    ) {
        let calls = std::rc::Rc::new(std::cell::Cell::new(0));
        let counted = std::rc::Rc::clone(&calls);
        let choice = choose_settings(
            move || {
                counted.set(counted.get() + 1);
                Ok(support)
            },
            Duration::from_millis(500),
        );
        (choice, calls)
    }

    #[test]
    fn the_settings_choice_passes_the_default_interval_when_the_probe_answers_no() {
        let (choice, calls) = asked(CaptureSupport {
            minimum_update_interval: false,
            cursor_setting: true,
        });
        assert_eq!(calls.get(), 1, "the probe was not asked once");
        assert_eq!(
            choice,
            Ok(ScreenChoice {
                interval: MinimumUpdateIntervalSettings::Default,
                drop_within: Some(Duration::from_millis(500)),
            })
        );
    }

    #[test]
    fn the_settings_choice_passes_the_custom_interval_when_the_probe_answers_yes() {
        let (choice, calls) = asked(CaptureSupport {
            minimum_update_interval: true,
            cursor_setting: true,
        });
        assert_eq!(calls.get(), 1, "the probe was not asked once");
        assert_eq!(
            choice,
            Ok(ScreenChoice {
                interval: MinimumUpdateIntervalSettings::Custom(Duration::from_millis(500)),
                drop_within: None,
            })
        );
    }

    #[test]
    fn the_settings_choice_refuses_a_build_without_the_cursor_setting_and_names_the_build() {
        let (choice, calls) = asked(CaptureSupport {
            minimum_update_interval: true,
            cursor_setting: false,
        });
        assert_eq!(calls.get(), 1, "the probe was not asked once");
        assert!(
            matches!(&choice, Err(CaptureError::Start(text)) if text.contains("19041")),
            "{choice:?}"
        );
    }

    #[test]
    fn the_handler_keeps_one_frame_per_interval_when_it_drops_by_time() {
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        let within = Some(Duration::from_millis(500));
        let mut last = None;
        let kept: Vec<bool> = [0, 100, 499, 500, 900, 1_000]
            .into_iter()
            .map(|ms| keep_frame(within, &mut last, at(ms)))
            .collect();
        assert_eq!(kept, [true, false, false, true, false, true]);
        let mut last = None;
        assert!([0, 1, 2]
            .into_iter()
            .all(|ms| keep_frame(None, &mut last, at(ms))));
    }

    #[test]
    fn a_closed_capture_item_reads_as_failed_health() {
        let (sink, _writer) = crate::sink::channel(4);
        let handoff = HandOff::start(sink, "halvo-test-screen-forward").expect("hand-off");
        let closed = Arc::new(AtomicBool::new(false));
        let mut handler = ScreenHandler {
            producer: handoff.producer(),
            last_app: Arc::default(),
            drop_within: None,
            last_kept: None,
            closed: Arc::clone(&closed),
        };
        assert!(handler.on_closed().is_ok());
        let failed = crate::windows::thread_ended();
        assert_eq!(
            screen_state(Some(false), closed.load(Ordering::Relaxed), false, false),
            failed
        );
        assert_eq!(
            screen_state(Some(false), false, false, false),
            SourceState::Running
        );
        assert_eq!(screen_state(Some(true), false, false, false), failed);
        assert_eq!(screen_state(Some(false), false, true, false), failed);
        assert_eq!(
            screen_state(None, false, false, true),
            SourceState::Stopped(StopCause::Request)
        );
        assert_eq!(
            screen_state(None, false, false, false),
            SourceState::NotReported
        );
    }

    #[test]
    fn a_zero_area_frame_is_not_recorded() {
        assert!(frame_packet(0, 1080, String::new(), 1).is_none());
        assert!(frame_packet(1920, 0, String::new(), 1).is_none());
    }
}
