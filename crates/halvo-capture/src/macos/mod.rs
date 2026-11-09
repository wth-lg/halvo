//! macOS capture backend.

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

/// The screen capture frame-rate cap.
pub const FRAME_FPS: i32 = 2;

/// How often the foreground poller samples `NSWorkspace.frontmostApplication`.
pub const FOREGROUND_POLL: Duration = Duration::from_millis(400);

/// The bounded sink capacity.
pub const SINK_CAPACITY: usize = 2048;

/// The current foreground app's bundle id, shared from the foreground poller to
/// the other sources so every event is stamped with the app that was focused.
#[derive(Debug, Clone, Default)]
pub struct SharedForeground(Arc<Mutex<String>>);

impl SharedForeground {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(String::new())))
    }

    /// The current foreground bundle id, or an empty string before the first
    /// poll.
    pub fn get(&self) -> String {
        self.0.lock().map(|g| g.clone()).unwrap_or_default()
    }

    /// Replace the current foreground bundle id.
    pub fn set(&self, bundle_id: String) {
        if let Ok(mut g) = self.0.lock() {
            *g = bundle_id;
        }
    }
}

/// Store placement for an event whose payload bytes are not persisted in this
/// phase. Placement is an explicit sentinel: the nil segment id and key id 0.
/// `byte_len` carries the payload's measured size where the source knows it.
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
pub fn timed_run(seconds: u64) -> Result<TimedRunReport, DaemonError> {
    let (handle, writer) = sink::channel(SINK_CAPACITY);
    let counts = Arc::new(Counts::default());
    let drain = {
        let counts = counts.clone();
        std::thread::spawn(move || drain_counting(writer, &counts))
    };

    let foreground_state = SharedForeground::new();

    let mut screen = screen::ScreenSource::new(FRAME_FPS, foreground_state.clone());
    let mut input = input::InputSource::new(foreground_state.clone());
    let secure_counter = input.secure_blocked_handle();
    let mut foreground =
        foreground::ForegroundSource::new(FOREGROUND_POLL, foreground_state.clone());
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
