//! macOS capture backend.
//!
//! Each modality is one [`CaptureSource`](crate::source::CaptureSource) that
//! emits the [`CaptureStreamEvent`](crate::event::CaptureStreamEvent) into the
//! single-writer sink:
//!
//! - [`screen`] — ScreenCaptureKit `SCStream` with a frame-rate throttle;
//! - [`input`] — one listen-only `CGEventTap` for keystroke and pointer;
//! - [`foreground`] — `NSWorkspace` frontmost app, change-detected, which also
//!   drives [`window_title`] on each change;
//! - [`audio`] / [`mic`] — present but off by default; no camera in R1.
//!
//! Secure input is held by construction: the keystroke tap is listen-only (it
//! cannot inject), Halvo's own app is excluded from every captured frame, and the
//! OS withholds keystrokes from the tap while a password field is focused
//! (see [`crate::secure_input_macos`]).
//!
//! # Memory discipline
//! A frame is never buffered beyond the bounded sink. The stream is throttled to
//! [`FRAME_FPS`] frames per second, and the frame handler reads nothing out of the
//! sample buffer: it records that a frame occurred and returns, dropping the
//! buffer immediately. The event therefore carries no pixels, so resident memory
//! stays flat over a run regardless of its length.

pub mod apps;
pub mod audio;
pub mod foreground;
pub mod input;
pub mod mic;
pub mod screen;
pub mod window_title;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use uuid::Uuid;

use crate::daemon::{DaemonError, TimedRunReport};
use crate::event::{SegmentKeyId, SegmentRef};
use crate::sink::{self, SinkWriter};
use crate::source::CaptureSource;

/// The screen capture frame-rate cap. Low by design: it bounds both CPU and the
/// event rate of the heaviest modality, and the daemon needs a change record, not
/// a video. Two frames per second over a 600-second run is 1200 frames, far more
/// than the gate needs, at negligible cost.
pub const FRAME_FPS: i32 = 2;

/// How often the foreground poller samples `NSWorkspace.frontmostApplication`.
/// 2.5 Hz catches every real app switch while costing one property read.
pub const FOREGROUND_POLL: Duration = Duration::from_millis(400);

/// The bounded sink capacity. Past it, frames (droppable) are dropped and
/// keystrokes block, per the sink's backpressure policy.
pub const SINK_CAPACITY: usize = 2048;

/// The current foreground app's bundle id, shared from the foreground poller to
/// the other sources so every event is stamped with the app that was focused.
/// One writer (the foreground poller); the rest only read.
#[derive(Debug, Clone, Default)]
pub struct SharedForeground(Arc<Mutex<String>>);

impl SharedForeground {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(String::new())))
    }

    /// The current foreground bundle id, or an empty string before the first
    /// poll. A poisoned lock degrades to empty rather than panicking in a
    /// capture callback.
    pub fn get(&self) -> String {
        self.0.lock().map(|g| g.clone()).unwrap_or_default()
    }

    /// Replace the current foreground bundle id (foreground poller only).
    pub fn set(&self, bundle_id: String) {
        if let Ok(mut g) = self.0.lock() {
            *g = bundle_id;
        }
    }
}

/// Store placement for an event whose payload bytes are not persisted in this
/// phase. The capture sources (P3) emit event metadata; the daemon's
/// source-to-store wiring (P6) assigns real placement when it writes the payload
/// to the encrypted segment store. Until then placement is an explicit sentinel:
/// the nil segment id and key id 0. `byte_len` carries the payload's measured
/// size where the source knows it, so the record stays informative.
pub fn unpersisted_placement(byte_len: u64) -> (SegmentRef, SegmentKeyId) {
    (
        SegmentRef {
            segment_id: Uuid::nil(),
            byte_offset: 0,
            byte_len,
        },
        SegmentKeyId(0),
    )
}

/// Run every macOS source for a bounded window and report per-modality counts.
///
/// This is the gate's `--timed-run` entry point. It owns the single
/// [`SinkWriter`] through a counting drain thread (the daemon's store wiring is
/// P6), starts the sources, runs for `seconds`, then stops them and reports how
/// many events of each modality reached the writer, plus the count of keystrokes
/// seen while secure input was active — which must be zero.
pub fn timed_run(seconds: u64) -> Result<TimedRunReport, DaemonError> {
    let (handle, writer) = sink::channel(SINK_CAPACITY);
    let counts = Arc::new(Counts::default());
    let drain = {
        let counts = counts.clone();
        std::thread::spawn(move || drain_counting(writer, &counts))
    };

    let foreground_state = SharedForeground::new();

    // Build the sources. Screen is on; audio and mic exist but are off; no
    // camera. The input source exposes the secure-input counter the report needs.
    let mut screen = screen::ScreenSource::new(FRAME_FPS, foreground_state.clone());
    let mut input = input::InputSource::new(foreground_state.clone());
    let secure_counter = input.secure_blocked_handle();
    let mut foreground =
        foreground::ForegroundSource::new(FOREGROUND_POLL, foreground_state.clone());
    let mut audio = audio::SystemAudioSource::disabled();
    let mut mic = mic::MicrophoneSource::disabled();

    // Start every source. A source that cannot start (for example without a
    // granted permission) is reported, not fatal: the run still measures the
    // others, and the gate grants permissions before its run.
    let sources: [(&str, Result<(), crate::source::CaptureError>); 5] = [
        ("foreground", foreground.start(handle.clone())),
        ("screen", screen.start(handle.clone())),
        ("input", input.start(handle.clone())),
        ("system_audio", audio.start(handle.clone())),
        ("microphone", mic.start(handle.clone())),
    ];
    for (name, result) in sources {
        if let Err(e) = result {
            eprintln!("halvo-capture: source {name} did not start: {e}");
        }
    }

    // The gate's secure-input sub-check sets HALVO_GATE_SECURE_INPUT=1. In that
    // mode Halvo holds secure input for the whole run, the exact condition a
    // focused password field creates, so the run proves the listen-only tap
    // records zero keystrokes while it is held. Any keystrokes during the window
    // are OS-suppressed and reported via `secure_input_keystrokes`, which must be
    // zero. The OS also releases the hold if the process exits early.
    let secure_gate = std::env::var("HALVO_GATE_SECURE_INPUT").as_deref() == Ok("1");
    if secure_gate {
        crate::secure_input_macos::hold_secure_input();
    }

    std::thread::sleep(Duration::from_secs(seconds));

    if secure_gate {
        crate::secure_input_macos::release_secure_input();
    }

    foreground.stop();
    screen.stop();
    input.stop();
    audio.stop();
    mic.stop();

    // Drop the last handle so the drain thread sees the channel close and exits.
    drop(handle);
    drain
        .join()
        .map_err(|_| DaemonError::Store("drain thread panicked".into()))?;

    Ok(TimedRunReport {
        seconds,
        screen_frame: counts.screen_frame.load(),
        keystroke: counts.keystroke.load(),
        pointer: counts.pointer.load(),
        foreground_app: counts.foreground_app.load(),
        window_title: counts.window_title.load(),
        secure_input_keystrokes: secure_counter.load(),
    })
}

/// Per-modality tallies filled by the drain thread.
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

/// Drain the sink, tallying each event by modality, until the channel closes.
/// This stands in for the P6 daemon drain: it owns the sole [`SinkWriter`], so
/// there is still exactly one reader of the stream.
fn drain_counting(writer: SinkWriter, counts: &Counts) {
    use crate::event::Modality;
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
mod tests {
    use super::*;

    #[test]
    fn unpersisted_placement_is_the_nil_sentinel() {
        let (seg, key) = unpersisted_placement(128);
        assert!(seg.segment_id.is_nil());
        assert_eq!(seg.byte_len, 128);
        assert_eq!(key.0, 0);
    }

    #[test]
    fn shared_foreground_round_trips() {
        let fg = SharedForeground::new();
        assert_eq!(fg.get(), "");
        fg.set("com.apple.Terminal".into());
        assert_eq!(fg.get(), "com.apple.Terminal");
    }
}
