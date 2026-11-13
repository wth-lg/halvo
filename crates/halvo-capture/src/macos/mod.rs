//! macOS capture backend.
#![deny(clippy::undocumented_unsafe_blocks)]

pub mod apps;
pub mod audio;
pub mod foreground;
pub mod input;
pub mod mic;
pub mod screen;
pub(crate) mod secure_input_byte;
pub mod window_title;

use std::sync::Arc;
use std::time::Duration;

use crate::daemon::{DaemonError, TimedRunReport};
use crate::event::Modality;
use crate::sink::{self, SinkWriter};
use crate::source::CaptureSource;
use foreground::{foreground_state, FOREGROUND_POLL};

/// The bounded sink capacity.
pub const SINK_CAPACITY: usize = 2048;

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

/// Run every macOS source for a bounded window and report per-modality counts.
/// Reads the gate's secure-input switch from the environment, then delegates to
/// [`run_sources`], which takes it as a parameter.
pub fn timed_run(seconds: u64) -> Result<TimedRunReport, DaemonError> {
    let hold_secure_input = std::env::var("HALVO_GATE_SECURE_INPUT").as_deref() == Ok("1");
    run_sources(seconds, hold_secure_input)
}

fn run_sources(seconds: u64, hold_secure_input: bool) -> Result<TimedRunReport, DaemonError> {
    let (handle, writer) = sink::channel(SINK_CAPACITY);
    let counts = Arc::new(Counts::default());
    let drain = {
        let counts = counts.clone();
        std::thread::spawn(move || drain_counting(writer, &counts))
    };

    let (foreground_writer, foreground_reader) = foreground_state();

    let mut screen = screen::ScreenSource::new(screen::SCREEN_FRAME_CAP, foreground_reader.clone());
    let mut input = input::InputSource::new(foreground_reader);
    let secure_violations = input.violations_handle();
    let mut foreground = foreground::ForegroundSource::new(FOREGROUND_POLL, foreground_writer);
    let mut audio = audio::SystemAudioSource::disabled();
    let mut mic = mic::MicrophoneSource::disabled();

    let starts: [(&'static str, Result<(), crate::source::CaptureError>); 5] = [
        ("foreground", foreground.start(handle.clone())),
        ("screen", screen.start(handle.clone())),
        ("input", input.start(handle.clone())),
        ("system_audio", audio.start(handle.clone())),
        ("microphone", mic.start(handle.clone())),
    ];
    let failed: Vec<SourceStartFailure> = starts
        .into_iter()
        .filter_map(|(name, result)| result.err().map(|error| SourceStartFailure { name, error }))
        .collect();
    for failure in &failed {
        eprintln!("halvo-capture: {failure}");
    }

    let secure_hold = secure_input_byte::hold_or_error(
        hold_secure_input.then(crate::secure_input_macos::hold_secure_input),
    );

    if secure_hold.is_ok() {
        std::thread::sleep(Duration::from_secs(seconds));
    }

    let refused = secure_hold.map(drop);

    foreground.stop();
    screen.stop();
    input.stop();
    audio.stop();
    mic.stop();

    drop(handle);
    drain
        .join()
        .map_err(|_| DaemonError::Store("drain thread panicked".into()))?;

    refused?;

    Ok(TimedRunReport {
        seconds,
        screen_frame: counts.screen_frame.load(),
        keystroke: counts.keystroke.load(),
        pointer: counts.pointer.load(),
        foreground_app: counts.foreground_app.load(),
        window_title: counts.window_title.load(),
        secure_input_keystrokes: secure_violations.count(),
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
