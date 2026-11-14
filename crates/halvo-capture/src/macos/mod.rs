#![deny(clippy::undocumented_unsafe_blocks)]

pub mod apps;
pub mod audio;
pub mod foreground;
pub mod input;
pub mod mic;
pub mod run_loop;
pub mod screen;
pub(crate) mod secure_input_byte;
pub mod window_title;

use std::sync::Arc;
use std::time::Duration;

use crate::daemon::{DaemonError, TimedRunReport};
use crate::event::Modality;
use crate::sink::{self, SinkWriter};
use crate::source::{
    CaptureError, CaptureSource, HealthReason, SourceContext, SourceState, Started,
};
use foreground::FOREGROUND_POLL;

pub const SINK_CAPACITY: usize = 2048;

pub(crate) fn source_failure(error: CaptureError) -> SourceState {
    let reason = match &error {
        CaptureError::PermissionDenied(_) => HealthReason::Permission,
        CaptureError::Timeout { .. } => HealthReason::Timeout,
        CaptureError::ThreadEnded => HealthReason::ThreadEnded,
        CaptureError::NotImplemented(_) => HealthReason::NotImplemented,
        CaptureError::Os { .. } | CaptureError::Start(_) | CaptureError::AlreadyStarted => {
            HealthReason::OsError
        }
    };
    SourceState::Failed { reason, error }
}

#[derive(Debug)]
struct SourceStartFailure {
    name: &'static str,
    error: crate::source::CaptureError,
}

impl std::fmt::Display for SourceStartFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "source {} did not start: {}", self.name, self.error)
    }
}

pub(crate) fn timed_run(seconds: u64) -> Result<TimedRunReport, DaemonError> {
    let hold_secure_input = std::env::var("HALVO_GATE_SECURE_INPUT").as_deref() == Ok("1");
    run_sources(seconds, hold_secure_input)
}

fn run_sources(seconds: u64, hold_secure_input: bool) -> Result<TimedRunReport, DaemonError> {
    let off_the_main_thread =
        || DaemonError::NotImplemented("a second timed run, or one off the main thread,");
    let mut main_loop = run_loop::MainRunLoop::claim().map_err(|_| off_the_main_thread())?;
    let (foreground_writer, foreground_reader) = main_loop
        .foreground_state()
        .ok_or_else(off_the_main_thread)?;

    let (handle, writer) = sink::channel(SINK_CAPACITY);
    let counts = Arc::new(Counts::default());
    let drain = {
        let counts = counts.clone();
        std::thread::spawn(move || drain_counting(writer, &counts))
    };

    let mut screen = screen::ScreenSource::new(screen::SCREEN_FRAME_CAP, foreground_reader.clone());
    let input_counters = input::InputCounters::new();
    let plan = if hold_secure_input {
        input::HoldPlan::After(Duration::ZERO)
    } else {
        input::HoldPlan::Never
    };
    let mut input = input::InputSource::new(foreground_reader, input_counters.clone(), plan);
    let mut foreground =
        foreground::ForegroundSource::new(FOREGROUND_POLL, foreground_writer, main_loop.turns());
    let mut audio = audio::SystemAudioSource::disabled();
    let mut mic = mic::MicrophoneSource::disabled();

    let ctx = SourceContext::new(handle);
    let starts: [(&'static str, Result<Started, CaptureError>); 5] = [
        ("foreground", foreground.begin(ctx.clone())),
        ("screen", screen.begin(ctx.clone())),
        ("input", input.begin(ctx.clone())),
        ("system_audio", audio.begin(ctx.clone())),
        ("microphone", mic.begin(ctx.clone())),
    ];
    drop(ctx);
    let failed: Vec<SourceStartFailure> = starts
        .into_iter()
        .filter_map(|(name, result)| result.err().map(|error| SourceStartFailure { name, error }))
        .collect();
    for failure in &failed {
        eprintln!("halvo-capture: {failure}");
    }

    main_loop.turn_for(Duration::from_secs(seconds));

    let stops: [(&'static str, Result<(), CaptureError>); 5] = [
        ("input", input.shutdown()),
        ("foreground", foreground.shutdown()),
        ("screen", screen.shutdown()),
        ("system_audio", audio.shutdown()),
        ("microphone", mic.shutdown()),
    ];
    for (name, stopped) in stops {
        if let Err(error) = stopped {
            eprintln!("halvo-capture: source {name} did not stop: {error}");
        }
    }
    let refusal = input.hold_refusal();
    drop((input, foreground, screen, audio, mic));

    drain
        .join()
        .map_err(|_| DaemonError::Store("drain thread panicked".into()))?;

    secure_input_byte::refusal_or_none(refusal)?;

    Ok(TimedRunReport {
        seconds,
        screen_frame: counts.screen_frame.load(),
        keystroke: counts.keystroke.load(),
        pointer: counts.pointer.load(),
        foreground_app: counts.foreground_app.load(),
        window_title: counts.window_title.load(),
        secure_input_keystrokes: input_counters.suppressed_keys(),
    })
}

#[derive(Default)]
struct Counts {
    screen_frame: Counter,
    keystroke: Counter,
    pointer: Counter,
    foreground_app: Counter,
    window_title: Counter,
}

#[derive(Default)]
struct Counter(std::sync::atomic::AtomicU64);

impl Counter {
    fn inc(&self) {
        self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    fn load(&self) -> u64 {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }
}

fn drain_counting(writer: SinkWriter, counts: &Counts) {
    while let Some(event) = writer.recv() {
        match event.modality {
            Modality::ScreenFrame => counts.screen_frame.inc(),
            Modality::Keystroke => counts.keystroke.inc(),
            Modality::Pointer => counts.pointer.inc(),
            Modality::ForegroundApp => counts.foreground_app.inc(),
            Modality::WindowTitle => counts.window_title.inc(),
            Modality::SystemAudio | Modality::Microphone | Modality::Camera => {}
        }
    }
}

#[cfg(test)]
pub(crate) mod off_source_tests {
    use std::sync::mpsc::RecvTimeoutError;
    use std::time::{Duration, Instant};

    use crate::sink;
    use crate::source::{CaptureError, CaptureSource, SourceContext, SourceState, Started};

    pub(crate) fn the_off_source_keeps_its_lifecycle(mut source: impl CaptureSource) {
        let (handle, writer) = sink::channel(8);
        let first = source.begin(SourceContext::new(handle.clone()));
        let second = source.begin(SourceContext::new(handle.clone()));
        let called = Instant::now();
        let shut = source.shutdown();
        let took = called.elapsed();
        let again = source.begin(SourceContext::new(handle.clone()));
        let state = source.health().state;
        drop(handle);
        let received = writer
            .recv_timeout(Duration::from_secs(2))
            .map(|received| received.event().modality);
        assert_eq!(first, Ok(Started::Off), "the first begin");
        assert_eq!(second, Err(CaptureError::AlreadyStarted), "a second begin");
        assert!(
            shut.is_ok() && took < Duration::from_secs(1),
            "shutdown {shut:?} after {took:?}"
        );
        assert_eq!(again, Ok(Started::Off), "a begin after the shutdown");
        assert_eq!(state, SourceState::Off, "the health state");
        assert_eq!(
            received,
            Err(RecvTimeoutError::Disconnected),
            "the off source emitted an event or kept a handle of the sink"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_error_of_a_source_reads_its_own_health_reason() {
        let reason_of = |error| match source_failure(error) {
            SourceState::Failed { reason, .. } => reason,
            other => panic!("source_failure answered {other:?}"),
        };
        assert_eq!(
            reason_of(CaptureError::Timeout {
                stage: "input_tap_join",
                ms: 1000
            }),
            HealthReason::Timeout
        );
        assert_eq!(
            reason_of(CaptureError::ThreadEnded),
            HealthReason::ThreadEnded
        );
        assert_eq!(
            reason_of(CaptureError::PermissionDenied(Modality::Keystroke)),
            HealthReason::Permission
        );
        assert_eq!(
            reason_of(CaptureError::Os {
                stage: "stop_capture",
                domain: "ScreenCaptureKit",
                code: -3808
            }),
            HealthReason::OsError
        );
    }
}
