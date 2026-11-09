//! macOS capture backend.

pub mod apps;
pub mod audio;
pub mod foreground;
pub mod input;
pub mod mic;
pub mod screen;
pub mod window_title;

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use uuid::Uuid;

use crate::daemon::{DaemonError, TimedRunReport};
use crate::event::{CaptureStreamEvent, CapturedAt, Modality, SegmentKeyId, SegmentRef};
use crate::sink::{self, SinkError, SinkHandle, SinkWriter};
use crate::source::{CaptureSource, EmitOutcome, EventSink};

/// The cap on the screen stream's frame rate.
pub const SCREEN_FRAME_CAP_FPS: i32 = 2;

/// How often the foreground poller samples `NSWorkspace.frontmostApplication`.
pub const FOREGROUND_POLL: Duration = Duration::from_millis(400);

/// The bounded sink capacity.
pub const SINK_CAPACITY: usize = 2048;

/// The write side of the current foreground bundle id. Not `Clone`: the
/// foreground poller is its only holder.
#[derive(Debug)]
pub struct ForegroundWriter(Arc<Mutex<String>>);

/// A read side of the current foreground bundle id, one per source that stamps
/// events with the focused app.
#[derive(Debug, Clone)]
pub struct ForegroundReader(Arc<Mutex<String>>);

/// The foreground state: one writer and a reader to clone for the other sources.
pub fn foreground_state() -> (ForegroundWriter, ForegroundReader) {
    let shared = Arc::new(Mutex::new(String::new()));
    (ForegroundWriter(shared.clone()), ForegroundReader(shared))
}

impl ForegroundWriter {
    /// Replace the current foreground bundle id.
    pub fn set(&self, bundle_id: String) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = bundle_id;
    }
}

impl ForegroundReader {
    /// The current foreground bundle id, or an empty string before the first
    /// poll.
    pub fn get(&self) -> String {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// Store placement for an event whose payload bytes are not persisted in this
/// phase: the nil segment id and key id 0, carrying the payload's measured size
/// where the source knows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unpersisted {
    pub byte_len: u64,
}

impl Unpersisted {
    pub const EMPTY: Self = Self { byte_len: 0 };

    pub fn placement(self) -> (SegmentRef, SegmentKeyId) {
        (
            SegmentRef {
                segment_id: Uuid::nil(),
                byte_offset: 0,
                byte_len: self.byte_len,
            },
            SegmentKeyId(0),
        )
    }
}

/// Emit an event that carries only metadata: the modality, the foreground app
/// and the [`Unpersisted`] placement. No payload bytes are read or persisted.
pub(crate) fn emit_metadata_only(
    sink: &SinkHandle,
    modality: Modality,
    foreground_app: String,
    placement: Unpersisted,
) -> Result<EmitOutcome, SinkError> {
    let (segment_ref, segment_key_id) = placement.placement();
    sink.emit(CaptureStreamEvent::new_raw(
        CapturedAt {
            wall_clock: SystemTime::now(),
            sequence: sink.next_sequence(),
        },
        modality,
        foreground_app,
        segment_ref,
        segment_key_id,
    ))
}

/// Run every macOS source for a bounded window and report per-modality counts.
/// Reads the gate's secure-input switch from the environment, then delegates to
/// [`run_sources`], which takes it as a parameter.
pub fn timed_run(seconds: u64) -> Result<TimedRunReport, DaemonError> {
    let secure_gate = std::env::var("HALVO_GATE_SECURE_INPUT").as_deref() == Ok("1");
    run_sources(seconds, secure_gate)
}

/// Run every macOS source for `seconds`. When `secure_gate` is set, hold secure
/// input for the window so the zero-keystroke invariant can be measured.
fn run_sources(seconds: u64, secure_gate: bool) -> Result<TimedRunReport, DaemonError> {
    let (handle, writer) = sink::channel(SINK_CAPACITY);
    let counts = Arc::new(Counts::default());
    let drain = {
        let counts = counts.clone();
        std::thread::spawn(move || drain_counting(writer, &counts))
    };

    let (foreground_writer, foreground_reader) = foreground_state();

    let mut screen = screen::ScreenSource::new(SCREEN_FRAME_CAP_FPS, foreground_reader.clone());
    let mut input = input::InputSource::new(foreground_reader);
    let secure_violations = input.violations_handle();
    let mut foreground = foreground::ForegroundSource::new(FOREGROUND_POLL, foreground_writer);
    let mut audio = audio::SystemAudioSource::disabled();
    let mut mic = mic::MicrophoneSource::disabled();

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

    let _secure_hold = secure_gate.then(crate::secure_input_macos::hold_secure_input);

    std::thread::sleep(Duration::from_secs(seconds));

    drop(_secure_hold);

    foreground.stop();
    screen.stop();
    input.stop();
    audio.stop();
    mic.stop();

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpersisted_placement_is_the_nil_sentinel() {
        let (seg, key) = Unpersisted { byte_len: 128 }.placement();
        assert!(seg.segment_id.is_nil());
        assert_eq!(seg.byte_len, 128);
        assert_eq!(key.0, 0);
        assert_eq!(Unpersisted::EMPTY.placement().0.byte_len, 0);
    }

    #[test]
    fn foreground_state_flows_from_the_writer_to_every_reader() {
        let (writer, reader) = foreground_state();
        let other = reader.clone();
        assert_eq!(reader.get(), "");
        writer.set("com.apple.Terminal".into());
        assert_eq!(reader.get(), "com.apple.Terminal");
        assert_eq!(other.get(), "com.apple.Terminal");
    }

    #[test]
    fn a_metadata_only_event_carries_no_persisted_bytes() {
        let (handle, writer) = sink::channel(1);
        emit_metadata_only(
            &handle,
            Modality::WindowTitle,
            "com.apple.Terminal".into(),
            Unpersisted { byte_len: 7 },
        )
        .unwrap();
        drop(handle);
        let event = writer.recv().unwrap();
        assert_eq!(event.modality, Modality::WindowTitle);
        assert_eq!(event.foreground_app, "com.apple.Terminal");
        assert!(event.segment_ref.segment_id.is_nil());
        assert_eq!(event.segment_ref.byte_len, 7);
        assert_eq!(event.segment_key_id.0, 0);
        assert!(!event.is_derived);
    }
}
