//! Screen capture via Windows.Graphics.Capture (`FrameArrived`).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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

use crate::source::{
    CaptureError, CaptureSource, HealthReason, SourceContext, SourceCounters, SourceHealth,
    SourceState, Started, StopCause,
};

use super::{HandOff, HandOffCounts, LossCounts, Observed, Packet, Producer};

/// The default minimum interval between captured frames.
pub const DEFAULT_MIN_FRAME_INTERVAL: Duration = Duration::from_millis(500);

const BGRA8_BYTES_PER_PIXEL: u64 = 4;

const SINK_GONE: &str = "the capture writer has stopped";
const NO_PRIMARY_MONITOR: &str = "the primary monitor could not be read";
const CURSOR_FLOOR: &str =
    "screen capture without the cursor needs Windows 10 build 19041 or later";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CaptureSupport {
    pub(crate) minimum_update_interval: bool,
    pub(crate) cursor_setting: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScreenChoice {
    Interval(Duration),
    DropWithin(Duration),
}

impl ScreenChoice {
    fn interval(self) -> MinimumUpdateIntervalSettings {
        match self {
            ScreenChoice::Interval(every) => MinimumUpdateIntervalSettings::Custom(every),
            ScreenChoice::DropWithin(_) => MinimumUpdateIntervalSettings::Default,
        }
    }

    fn drop_within(self) -> Option<Duration> {
        match self {
            ScreenChoice::Interval(_) => None,
            ScreenChoice::DropWithin(within) => Some(within),
        }
    }
}

fn probe_support() -> Result<CaptureSupport, CaptureError> {
    Ok(CaptureSupport {
        minimum_update_interval: GraphicsCaptureApi::is_minimum_update_interval_supported()
            .map_err(|e| api_error("probe the minimum update interval", e))?,
        cursor_setting: GraphicsCaptureApi::is_cursor_settings_supported()
            .map_err(|e| api_error("probe the cursor setting", e))?,
    })
}

mod apartment {
    use std::sync::{Mutex, PoisonError};

    use crate::source::CaptureError;

    #[derive(Debug, Clone, Copy)]
    pub(super) struct MtaHeld(());

    pub(super) fn hold_process_mta() -> Result<MtaHeld, CaptureError> {
        static HELD: Mutex<bool> = Mutex::new(false);
        let mut held = HELD.lock().unwrap_or_else(PoisonError::into_inner);
        if !*held {
            // SAFETY: CoIncrementMTAUsage takes no pointer of ours; the cookie
            // that it returns is never passed to CoDecrementMTAUsage, so the
            // reference lasts for the life of the process.
            unsafe { windows::Win32::System::Com::CoIncrementMTAUsage() }
                .map_err(|e| crate::windows::os_error("CoIncrementMTAUsage", &e))?;
            *held = true;
        }
        Ok(MtaHeld(()))
    }
}

use apartment::{hold_process_mta, MtaHeld};

fn start_session(
    settings: Settings<ScreenFlags, Monitor>,
    _mta: MtaHeld,
) -> Result<Control, CaptureError> {
    ScreenHandler::start_free_threaded(settings).map_err(start_error)
}

pub(crate) fn choose_settings(
    probe: impl FnOnce() -> Result<CaptureSupport, CaptureError>,
    min_interval: Duration,
) -> Result<ScreenChoice, CaptureError> {
    let support = probe()?;
    if !support.cursor_setting {
        return Err(CaptureError::Start(CURSOR_FLOOR.to_string()));
    }
    if support.minimum_update_interval {
        return Ok(ScreenChoice::Interval(min_interval));
    }
    Ok(ScreenChoice::DropWithin(min_interval))
}

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

pub(crate) fn frame_packet(
    width: u32,
    height: u32,
    foreground_app: Option<String>,
    tick: u32,
) -> Option<Packet> {
    if width == 0 || height == 0 {
        return None;
    }
    Some(Packet::observed(
        Observed::ScreenFrame,
        foreground_app,
        u64::from(width) * u64::from(height) * BGRA8_BYTES_PER_PIXEL,
        tick,
    ))
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
        let app = super::app_of(&self.last_app);
        if let Some(packet) = frame_packet(width, height, app, super::now_tick()) {
            self.producer.push(packet);
        }
        if self.producer.writer_gone() {
            return Err(CaptureError::Start(SINK_GONE.to_string()));
        }
        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        self.closed.store(true, Ordering::Relaxed);
        Ok(())
    }
}

type Control = CaptureControl<ScreenHandler, CaptureError>;

/// Screen capture source over the primary monitor, the main display by choice
/// (RD-39). A closed capture item, a capture thread that ended and a writer
/// that is gone each read as failed health. The source asks for the system's
/// capture border (`DrawBorderSettings::Default`) by decision RD-04.
pub struct ScreenSource {
    last_app: Arc<Mutex<Option<String>>>,
    min_interval: Duration,
    life: ScreenLife<Control>,
    handoff: Option<HandOff>,
    counts: Arc<LossCounts>,
    closed: Arc<AtomicBool>,
}

impl ScreenSource {
    pub fn new(last_app: Arc<Mutex<Option<String>>>) -> Self {
        Self {
            last_app,
            min_interval: DEFAULT_MIN_FRAME_INTERVAL,
            life: ScreenLife::default(),
            handoff: None,
            counts: Arc::default(),
            closed: Arc::default(),
        }
    }

    /// Override the minimum interval between frames (the frame-rate throttle).
    pub fn with_min_interval(mut self, interval: Duration) -> Self {
        self.min_interval = interval;
        self
    }

    /// The counts of this source's hand-off queues since it was made.
    pub fn handoff_counts(&self) -> HandOffCounts {
        self.counts.read()
    }
}

impl CaptureSource for ScreenSource {
    fn begin(&mut self, ctx: SourceContext) -> Result<Started, CaptureError> {
        if self.life.is_busy() {
            return Err(CaptureError::AlreadyStarted);
        }
        let mta = hold_process_mta()?;
        let choice = choose_settings(probe_support, self.min_interval)?;
        let monitor = Monitor::primary().map_err(monitor_error)?;
        let handoff = HandOff::start(
            ctx.sink().clone(),
            "halvo-screen-forward",
            Arc::clone(&self.counts),
        )?;
        let closed = Arc::new(AtomicBool::new(false));
        let flags = ScreenFlags {
            producer: handoff.producer(),
            last_app: Arc::clone(&self.last_app),
            drop_within: choice.drop_within(),
            closed: Arc::clone(&closed),
        };
        let settings = Settings::new(
            monitor,
            CursorCaptureSettings::WithoutCursor,
            DrawBorderSettings::Default,
            SecondaryWindowSettings::Default,
            choice.interval(),
            DirtyRegionSettings::Default,
            ColorFormat::Bgra8,
            flags,
        );
        let control = start_session(settings, mta)?;
        self.life.started(control);
        self.handoff = Some(handoff);
        self.closed = closed;
        Ok(Started::Running)
    }

    fn shutdown(&mut self) -> Result<(), CaptureError> {
        let stopped = self.life.shutdown(super::SCREEN_STOP_DEADLINE);
        if let Some(handoff) = &self.handoff {
            handoff.close();
        }
        stopped
    }

    fn health(&self) -> SourceHealth {
        SourceHealth {
            state: self.life.state(
                self.closed.load(Ordering::Relaxed),
                self.handoff.as_ref().is_some_and(HandOff::writer_gone),
            ),
            counters: SourceCounters::default(),
        }
    }
}

impl Drop for ScreenSource {
    fn drop(&mut self) {
        self.life.on_drop(super::SCREEN_STOP_DEADLINE);
        if let Some(handoff) = &self.handoff {
            handoff.close();
        }
    }
}

trait Session: Send + 'static {
    fn finished(&self) -> bool;
    fn end(self) -> Result<(), CaptureError>;
    fn ask_to_end(self);
}

impl Session for Control {
    fn finished(&self) -> bool {
        self.is_finished()
    }

    fn end(self) -> Result<(), CaptureError> {
        use windows_capture::capture::CaptureControlError;
        self.stop().map_err(|error| match error {
            CaptureControlError::FailedToPostThreadMessage => {
                super::last_os_error("PostThreadMessageW")
            }
            _ => CaptureError::ThreadEnded,
        })
    }

    fn ask_to_end(self) {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Foundation::{HANDLE, LPARAM, WPARAM};
        use windows::Win32::System::Threading::GetThreadId;
        use windows::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};
        self.halt_handle().store(true, Ordering::Relaxed);
        let thread = self.into_thread_handle();
        // SAFETY: GetThreadId reads the capture thread's handle, which
        // `thread` holds open for the call.
        let id = unsafe { GetThreadId(HANDLE(thread.as_raw_handle())) };
        if id != 0 {
            // SAFETY: PostThreadMessageW takes a thread id and two plain
            // values; `thread` holds the capture thread's handle across the
            // call, so the id names that thread.
            unsafe {
                let _ = PostThreadMessageW(id, WM_QUIT, WPARAM(0), LPARAM(0));
            }
        }
        drop(thread);
    }
}

type StopBody = Box<dyn FnOnce() + Send + 'static>;

type StopSpawner = fn(StopBody) -> std::io::Result<()>;

fn spawn_stop_thread(body: StopBody) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("halvo-screen-stop".to_string())
        .spawn(body)
        .map(|_| ())
}

struct Stopping {
    done: std::sync::mpsc::Receiver<Result<(), CaptureError>>,
    ended_before: bool,
}

impl Stopping {
    fn start<S: Session>(session: S, spawn: StopSpawner) -> Result<Self, (S, CaptureError)> {
        let ended_before = session.finished();
        let (give, take) = std::sync::mpsc::channel::<S>();
        let (done, stopped) = std::sync::mpsc::channel();
        let spawned = spawn(Box::new(move || {
            if let Ok(session) = take.recv() {
                let _ = done.send(session.end());
            }
        }));
        if let Err(e) = spawned {
            let error = CaptureError::Os {
                stage: "spawn the screen stop thread",
                domain: "io",
                code: i64::from(e.raw_os_error().unwrap_or(0)),
            };
            return Err((session, error));
        }
        if let Err(std::sync::mpsc::SendError(session)) = give.send(session) {
            return Err((session, CaptureError::ThreadEnded));
        }
        Ok(Self {
            done: stopped,
            ended_before,
        })
    }

    fn wait(&self, deadline: Duration) -> Option<Result<(), CaptureError>> {
        match self.done.recv_timeout(deadline) {
            Ok(Ok(())) if !self.ended_before => Some(Ok(())),
            Ok(Err(error @ CaptureError::Os { .. })) => Some(Err(error)),
            Ok(_) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                Some(Err(CaptureError::ThreadEnded))
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => None,
        }
    }
}

struct ScreenLife<S> {
    session: Option<S>,
    stopping: Option<Stopping>,
    timed_out: Option<CaptureError>,
    stop_failed: Option<CaptureError>,
    end_refused: Option<CaptureError>,
    stopped: bool,
    spawn: StopSpawner,
}

impl<S> Default for ScreenLife<S> {
    fn default() -> Self {
        Self {
            session: None,
            stopping: None,
            timed_out: None,
            stop_failed: None,
            end_refused: None,
            stopped: false,
            spawn: spawn_stop_thread,
        }
    }
}

#[cfg(test)]
impl<S> ScreenLife<S> {
    fn with_spawner(spawn: StopSpawner) -> Self {
        Self {
            spawn,
            ..Self::default()
        }
    }
}

impl<S: Session> ScreenLife<S> {
    fn is_busy(&self) -> bool {
        self.session.is_some() || self.stopping.is_some() || self.end_refused.is_some()
    }

    fn started(&mut self, session: S) {
        self.session = Some(session);
        self.stopped = false;
    }

    fn shutdown(&mut self, deadline: Duration) -> Result<(), CaptureError> {
        if let Some(error) = &self.end_refused {
            return Err(error.clone());
        }
        if let Some(session) = self.session.take() {
            match Stopping::start(session, self.spawn) {
                Ok(stopping) => {
                    self.stopping = Some(stopping);
                    self.stop_failed = None;
                }
                Err((session, error)) => {
                    self.session = Some(session);
                    self.stop_failed = Some(error.clone());
                    return Err(error);
                }
            }
        }
        let Some(stopping) = &self.stopping else {
            return Ok(());
        };
        let Some(stopped) = stopping.wait(deadline) else {
            let timeout = CaptureError::Timeout {
                stage: "stop",
                ms: u64::try_from(deadline.as_millis()).unwrap_or(u64::MAX),
            };
            self.timed_out = Some(timeout.clone());
            return Err(timeout);
        };
        self.stopping = None;
        self.timed_out = None;
        if let Err(error @ CaptureError::Os { .. }) = &stopped {
            self.end_refused = Some(error.clone());
            return stopped;
        }
        self.stopped = true;
        stopped
    }

    fn on_drop(&mut self, deadline: Duration) {
        if self.timed_out.is_some() && self.stopping.is_some() {
            self.stopping = None;
            return;
        }
        let _ = self.shutdown(deadline);
        if let Some(session) = self.session.take() {
            session.ask_to_end();
        }
    }

    fn state(&self, item_closed: bool, writer_gone: bool) -> SourceState {
        if let Some(error) = &self.end_refused {
            return SourceState::Failed {
                reason: HealthReason::OsError,
                error: error.clone(),
            };
        }
        if let (Some(_), Some(timeout)) = (&self.stopping, &self.timed_out) {
            return SourceState::Failed {
                reason: HealthReason::Timeout,
                error: timeout.clone(),
            };
        }
        if let (Some(_), Some(error)) = (&self.session, &self.stop_failed) {
            return SourceState::Failed {
                reason: HealthReason::OsError,
                error: error.clone(),
            };
        }
        match &self.session {
            Some(_) if writer_gone => super::failed_with(HealthReason::WriterGone),
            Some(_) if item_closed => super::failed_with(HealthReason::ItemClosed),
            Some(session) if session.finished() => super::thread_ended(),
            Some(_) => SourceState::Running,
            None if self.stopped => SourceState::Stopped(StopCause::Request),
            None => SourceState::NotReported,
        }
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
    use crate::event::Modality;

    #[test]
    fn a_frame_maps_to_a_screen_frame_packet_of_its_byte_count() {
        let packet = frame_packet(1920, 1080, Some("C:\\browser.exe".to_string()), 1)
            .expect("a sized frame produces a packet");
        assert_eq!(packet.kind(), Modality::ScreenFrame);
        assert_eq!(packet.app.as_deref(), Some("C:\\browser.exe"));
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
        let choice = choice.expect("a build with the cursor setting gets a choice");
        assert_eq!(choice.interval(), MinimumUpdateIntervalSettings::Default);
        assert_eq!(choice.drop_within(), Some(Duration::from_millis(500)));
    }

    #[test]
    fn the_settings_choice_passes_the_custom_interval_when_the_probe_answers_yes() {
        let (choice, calls) = asked(CaptureSupport {
            minimum_update_interval: true,
            cursor_setting: true,
        });
        assert_eq!(calls.get(), 1, "the probe was not asked once");
        let choice = choice.expect("a build with the cursor setting gets a choice");
        assert_eq!(
            choice.interval(),
            MinimumUpdateIntervalSettings::Custom(Duration::from_millis(500))
        );
        assert_eq!(choice.drop_within(), None);
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

    struct FakeSession {
        finished: bool,
        ends_well: bool,
        end_takes: Duration,
        fails_with: Option<CaptureError>,
    }

    impl Session for FakeSession {
        fn finished(&self) -> bool {
            self.finished
        }

        fn end(self) -> Result<(), CaptureError> {
            std::thread::sleep(self.end_takes);
            if let Some(error) = self.fails_with {
                return Err(error);
            }
            if self.ends_well {
                Ok(())
            } else {
                Err(CaptureError::ThreadEnded)
            }
        }

        fn ask_to_end(self) {}
    }

    fn fake(finished: bool, ends_well: bool, end_takes_ms: u64) -> FakeSession {
        FakeSession {
            finished,
            ends_well,
            end_takes: Duration::from_millis(end_takes_ms),
            fails_with: None,
        }
    }

    #[test]
    fn a_session_whose_end_is_refused_answers_its_os_error_and_reads_failed() {
        let refused = CaptureError::Os {
            stage: "PostThreadMessageW",
            domain: "HRESULT",
            code: 5,
        };
        let mut life = ScreenLife::default();
        life.started(FakeSession {
            fails_with: Some(refused.clone()),
            ..fake(false, false, 0)
        });
        let stopped = life.shutdown(Duration::from_secs(1));
        assert_eq!(stopped, Err(refused.clone()));
        assert_eq!(
            life.state(false, false),
            SourceState::Failed {
                reason: HealthReason::OsError,
                error: refused,
            }
        );
        assert!(
            life.is_busy(),
            "a source whose stop failed counts as started"
        );
    }

    #[test]
    fn a_closed_capture_item_reads_as_failed_health() {
        let (sink, _writer) = crate::sink::channel(4);
        let handoff =
            HandOff::start(sink, "halvo-test-screen-forward", Arc::default()).expect("hand-off");
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
        let failed_with = |reason| SourceState::Failed {
            reason,
            error: CaptureError::ThreadEnded,
        };
        let mut life = ScreenLife::default();
        assert_eq!(life.state(false, false), SourceState::NotReported);
        life.started(fake(false, true, 0));
        assert_eq!(
            life.state(closed.load(Ordering::Relaxed), false),
            failed_with(HealthReason::ItemClosed)
        );
        assert_eq!(life.state(false, false), SourceState::Running);
        assert_eq!(
            life.state(false, true),
            failed_with(HealthReason::WriterGone)
        );
        assert_eq!(
            life.state(true, true),
            failed_with(HealthReason::WriterGone)
        );
        assert_eq!(life.shutdown(Duration::from_secs(1)), Ok(()));
        assert_eq!(
            life.state(false, false),
            SourceState::Stopped(StopCause::Request)
        );
        life.started(fake(true, true, 0));
        assert_eq!(life.state(false, false), failed);
    }

    #[test]
    fn a_stop_reports_a_session_that_had_ended_before_or_did_not_end_well() {
        let stop = |session| {
            Stopping::start(session, spawn_stop_thread)
                .map_err(|(_, error)| error)
                .expect("the stop thread")
                .wait(Duration::from_secs(2))
        };
        assert_eq!(stop(fake(false, true, 0)), Some(Ok(())));
        assert_eq!(
            stop(fake(true, true, 0)),
            Some(Err(CaptureError::ThreadEnded))
        );
        assert_eq!(
            stop(fake(false, false, 0)),
            Some(Err(CaptureError::ThreadEnded))
        );
    }

    #[test]
    fn a_dropped_screen_source_whose_stop_timed_out_waits_no_more() {
        let mut life = ScreenLife::default();
        life.started(fake(false, true, 3_000));
        let first = life.shutdown(Duration::from_millis(300));
        assert!(
            matches!(first, Err(CaptureError::Timeout { .. })),
            "{first:?}"
        );
        let began = Instant::now();
        life.on_drop(Duration::from_millis(300));
        let waited = began.elapsed();
        assert!(
            waited < Duration::from_millis(100),
            "the drop waited {waited:?}"
        );
    }

    #[test]
    fn a_stop_that_does_not_end_in_time_keeps_the_session_busy_and_failed() {
        let mut life = ScreenLife::default();
        life.started(fake(false, true, 1_500));
        let first = life.shutdown(Duration::from_millis(300));
        let busy = life.is_busy();
        let state = life.state(false, false);
        let second = life.shutdown(Duration::from_millis(300));
        let third = life.shutdown(Duration::from_secs(3));
        let after = life.state(false, false);
        assert_eq!(
            first,
            Err(CaptureError::Timeout {
                stage: "stop",
                ms: 300
            })
        );
        assert!(busy, "a source whose stop timed out took a new begin");
        assert_eq!(
            state,
            SourceState::Failed {
                reason: crate::source::HealthReason::Timeout,
                error: CaptureError::Timeout {
                    stage: "stop",
                    ms: 300
                },
            }
        );
        assert!(
            matches!(second, Err(CaptureError::Timeout { .. })),
            "{second:?}"
        );
        assert_eq!(third, Ok(()), "the stop that ended late was not joined");
        assert_eq!(after, SourceState::Stopped(StopCause::Request));
        assert!(!life.is_busy());
    }

    struct Watched {
        ended: Arc<AtomicBool>,
        dropped: Arc<AtomicBool>,
    }

    impl Session for Watched {
        fn finished(&self) -> bool {
            false
        }

        fn end(self) -> Result<(), CaptureError> {
            self.ended.store(true, Ordering::SeqCst);
            Ok(())
        }

        fn ask_to_end(self) {}
    }

    impl Drop for Watched {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::SeqCst);
        }
    }

    static STOP_SPAWNS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

    fn refuses_twice(body: StopBody) -> std::io::Result<()> {
        if STOP_SPAWNS.fetch_add(1, Ordering::SeqCst) < 2 {
            return Err(std::io::Error::from_raw_os_error(8));
        }
        spawn_stop_thread(body)
    }

    #[test]
    fn a_stop_whose_thread_cannot_be_spawned_keeps_the_session() {
        let ended = Arc::new(AtomicBool::new(false));
        let dropped = Arc::new(AtomicBool::new(false));
        let mut life = ScreenLife::with_spawner(refuses_twice);
        life.started(Watched {
            ended: Arc::clone(&ended),
            dropped: Arc::clone(&dropped),
        });
        let first = life.shutdown(Duration::from_millis(300));
        let busy = life.is_busy();
        let state = life.state(false, false);
        let kept = !dropped.load(Ordering::SeqCst);
        let ended_at_first = ended.load(Ordering::SeqCst);
        let second = life.shutdown(Duration::from_millis(300));
        let busy_after_second = life.is_busy();
        life.on_drop(Duration::from_secs(2));
        let refused = CaptureError::Os {
            stage: "spawn the screen stop thread",
            domain: "io",
            code: 8,
        };
        assert_eq!(first, Err(refused.clone()));
        assert!(
            busy,
            "the source whose stop thread could not start would take a new begin"
        );
        assert_eq!(
            state,
            SourceState::Failed {
                reason: HealthReason::OsError,
                error: refused.clone(),
            }
        );
        assert!(kept, "the session was dropped without a stop");
        assert!(!ended_at_first, "the session ended without a stop thread");
        assert_eq!(second, Err(refused));
        assert!(
            busy_after_second,
            "a second refused stop let the session go"
        );
        assert_eq!(
            STOP_SPAWNS.load(Ordering::SeqCst),
            3,
            "the second shutdown and the drop each try the stop again"
        );
        assert!(
            ended.load(Ordering::SeqCst),
            "the drop did not stop the session"
        );
        assert!(!life.is_busy());
    }

    struct Asked {
        ended: Arc<AtomicBool>,
        asked: Arc<AtomicBool>,
    }

    impl Session for Asked {
        fn finished(&self) -> bool {
            false
        }

        fn end(self) -> Result<(), CaptureError> {
            self.ended.store(true, Ordering::SeqCst);
            Ok(())
        }

        fn ask_to_end(self) {
            self.asked.store(true, Ordering::SeqCst);
        }
    }

    static REFUSED_SPAWNS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

    fn always_refuses(_body: StopBody) -> std::io::Result<()> {
        REFUSED_SPAWNS.fetch_add(1, Ordering::SeqCst);
        Err(std::io::Error::from_raw_os_error(8))
    }

    #[test]
    fn a_drop_whose_stop_thread_is_always_refused_still_ends_the_session() {
        let ended = Arc::new(AtomicBool::new(false));
        let asked = Arc::new(AtomicBool::new(false));
        let mut life = ScreenLife::with_spawner(always_refuses);
        life.started(Asked {
            ended: Arc::clone(&ended),
            asked: Arc::clone(&asked),
        });
        let before = REFUSED_SPAWNS.load(Ordering::SeqCst);
        life.on_drop(Duration::from_millis(300));
        drop(life);
        let tried = REFUSED_SPAWNS.load(Ordering::SeqCst) - before;
        assert_eq!(tried, 1, "the drop tried its stop thread once");
        assert!(
            asked.load(Ordering::SeqCst),
            "the session went with the source and nothing asked it to end"
        );
        assert!(
            !ended.load(Ordering::SeqCst),
            "the drop ran the session's joining stop"
        );
    }

    #[test]
    fn a_dropped_screen_source_whose_stop_thread_is_refused_ends_its_capture() {
        use std::sync::mpsc::RecvTimeoutError;
        let _serial = crate::windows::tests::serial();
        let (sink, writer) = crate::sink::channel(64);
        let mut source = ScreenSource::new(Arc::default());
        source.life = ScreenLife::with_spawner(always_refuses);
        let started = source.begin(crate::source::SourceContext::new(sink));
        let first = writer.recv_timeout(Duration::from_secs(10)).is_ok();
        drop(source);
        let dropped = Instant::now();
        let bound = Duration::from_secs(15);
        let mut frames_after = 0u32;
        let closed = loop {
            let left = bound.saturating_sub(dropped.elapsed());
            match writer.recv_timeout(left) {
                Ok(_) => frames_after += 1,
                Err(RecvTimeoutError::Disconnected) => break true,
                Err(RecvTimeoutError::Timeout) => break false,
            }
        };
        assert!(matches!(started, Ok(Started::Running)), "{started:?}");
        assert!(
            first,
            "INCONCLUSIVE: no frame arrived in 10 s before the drop"
        );
        assert!(
            closed,
            "the capture went on after the drop: the sink stayed open for {bound:?}, {frames_after} frames after the drop"
        );
    }

    #[test]
    fn a_frame_pushed_after_a_screen_shutdown_is_counted_lost_and_stays_out_of_the_sink() {
        let (sink, writer) = crate::sink::channel(16);
        let mut source = ScreenSource::new(Arc::default());
        let handoff = HandOff::start(
            sink,
            "halvo-test-screen-forward",
            Arc::clone(&source.counts),
        )
        .expect("hand-off");
        let producer = handoff.producer();
        source.handoff = Some(handoff);
        let stopped = source.shutdown();
        producer.push(frame_packet(1920, 1080, None, 1).expect("a frame"));
        let arrived = writer.recv_timeout(Duration::from_millis(500)).is_ok();
        assert_eq!(stopped, Ok(()));
        assert!(
            !arrived,
            "a frame pushed after the shutdown reached the sink"
        );
        assert_eq!(source.handoff_counts().lost, 1);
    }

    static ONE_REFUSAL: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

    fn refuses_once(body: StopBody) -> std::io::Result<()> {
        if ONE_REFUSAL.fetch_add(1, Ordering::SeqCst) == 0 {
            return Err(std::io::Error::from_raw_os_error(8));
        }
        spawn_stop_thread(body)
    }

    #[test]
    fn a_stop_that_starts_after_a_refused_one_clears_the_refusal() {
        let mut life = ScreenLife::with_spawner(refuses_once);
        life.started(fake(false, true, 0));
        let refused = life.shutdown(Duration::from_secs(2));
        let while_refused = life.state(false, false);
        let stopped = life.shutdown(Duration::from_secs(2));
        let free = !life.is_busy();
        life.started(fake(false, true, 0));
        let after_a_new_start = life.state(false, false);
        let _ = life.shutdown(Duration::from_secs(2));
        assert!(
            matches!(refused, Err(CaptureError::Os { .. })),
            "{refused:?}"
        );
        assert!(
            matches!(while_refused, SourceState::Failed { .. }),
            "{while_refused:?}"
        );
        assert_eq!(stopped, Ok(()));
        assert!(free, "a source whose stop went through refuses a begin");
        assert_eq!(after_a_new_start, SourceState::Running);
    }

    #[test]
    fn the_process_mta_reference_is_taken_once_and_holds_an_mta_for_a_new_thread() {
        use windows::Win32::System::Com::{
            CoGetApartmentType, APTTYPE, APTTYPEQUALIFIER, APTTYPEQUALIFIER_IMPLICIT_MTA,
            APTTYPE_MTA,
        };
        assert!(hold_process_mta().is_ok());
        assert!(hold_process_mta().is_ok());
        let apartment = std::thread::spawn(|| {
            let mut kind = APTTYPE::default();
            let mut qualifier = APTTYPEQUALIFIER::default();
            // SAFETY: both pointers are live locals that the call writes.
            unsafe { CoGetApartmentType(&mut kind, &mut qualifier) }.map(|()| (kind, qualifier))
        })
        .join()
        .expect("the probe thread");
        assert_eq!(apartment, Ok((APTTYPE_MTA, APTTYPEQUALIFIER_IMPLICIT_MTA)));
    }

    #[test]
    fn a_zero_area_frame_is_not_recorded() {
        assert!(frame_packet(0, 1080, None, 1).is_none());
        assert!(frame_packet(1920, 0, None, 1).is_none());
    }
}
