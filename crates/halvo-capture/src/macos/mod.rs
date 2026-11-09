//! macOS capture backend.

pub mod apps;
pub mod audio;
pub mod foreground;
pub mod input;
pub mod mic;
pub mod screen;
pub mod window_title;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use uuid::Uuid;

use crate::custody::dev_fallback::DevFallbackCustody;
use crate::custody::{KeyCustody, RootId};
use crate::daemon::{DaemonError, TimedRunReport};
use crate::event::{CaptureStreamEvent, CapturedAt, Modality, SegmentKeyId, SegmentRef};
use crate::sink::{self, SinkError, SinkHandle};
use crate::source::{CaptureSource, EmitOutcome, EventSink};
use crate::store::db::EncryptedStore;
use crate::store::DEFAULT_RETENTION;

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

/// A source that failed to start. Reported, not fatal: the timed run continues
/// with the sources that did start.
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

/// Run every macOS source for the gate's bounded `--timed-run`, persisting the
/// captured events into a throwaway encrypted store and reporting per-modality
/// counts. The store is fresh and single-use: when `HALVO_STORE_DIR` is set the
/// run writes into it and leaves the files for the gate to measure and remove;
/// otherwise it uses a unique temp directory it removes when done. Either way the
/// store's custody root is destroyed at the end, so the measurement leaves no
/// keychain item and no decryptable ciphertext behind.
pub fn timed_run(seconds: u64, secure_gate: bool) -> Result<TimedRunReport, DaemonError> {
    let (dir, remove_dir) = match std::env::var("HALVO_STORE_DIR") {
        Ok(d) => (PathBuf::from(d), false),
        Err(_) => (
            std::env::temp_dir().join(format!("halvo-timedrun-{}", Uuid::now_v7())),
            true,
        ),
    };
    let root_id = RootId::fresh();
    let store = EncryptedStore::create(
        &dir,
        Box::new(DevFallbackCustody::new(root_id)),
        DEFAULT_RETENTION,
    )
    .map_err(|e| DaemonError::Store(e.to_string()))?;

    let result = run_capture(
        RunMode::Timed(Duration::from_secs(seconds)),
        secure_gate,
        store,
    );

    // The throwaway measurement store is crypto-shredded by destroying its root,
    // so the keychain is left clean and the ciphertext left on disk is
    // undecryptable; the directory is removed unless the gate owns it.
    let _ = DevFallbackCustody::new(root_id).destroy_root();
    if remove_dir {
        let _ = std::fs::remove_dir_all(&dir);
    }

    result.map(|mut report| {
        report.seconds = seconds;
        report
    })
}

/// Run the always-on capture daemon, persisting into the store at `store_dir`,
/// until the process is terminated. The daemon is the sole store writer.
pub fn service_run(store_dir: PathBuf) -> Result<(), DaemonError> {
    let store = crate::daemon::open_or_create_store(&store_dir)
        .map_err(|e| DaemonError::Store(e.to_string()))?;
    run_capture(RunMode::UntilSignal, false, store).map(|_| ())
}

/// How long a capture run lasts.
enum RunMode {
    /// A bounded run of this duration (the gate's timed run).
    Timed(Duration),
    /// An unbounded run until the process receives SIGTERM or SIGINT (the
    /// always-on daemon under launchd).
    UntilSignal,
}

/// Start every macOS source, drain the single-writer sink into `store` on a
/// writer thread (the sole store writer), run for the mode's lifetime, then stop
/// the sources and flush. When `secure_gate` is set, hold secure input for the
/// window so the zero-keystroke invariant can be measured.
fn run_capture(
    mode: RunMode,
    secure_gate: bool,
    store: EncryptedStore,
) -> Result<TimedRunReport, DaemonError> {
    let (handle, writer) = sink::channel(SINK_CAPACITY);
    let counts = Arc::new(Counts::default());
    let drain = {
        let counts = counts.clone();
        std::thread::spawn(move || -> Result<u64, DaemonError> {
            let mut store = store;
            crate::daemon::drain_to_store(&writer, &mut store, |event| {
                counts.record(event.modality)
            })
        })
    };

    let (foreground_writer, foreground_reader) = foreground_state();

    let mut screen = screen::ScreenSource::new(SCREEN_FRAME_CAP_FPS, foreground_reader.clone());
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

    let _secure_hold = secure_gate.then(crate::secure_input_macos::hold_secure_input);

    match mode {
        RunMode::Timed(duration) => std::thread::sleep(duration),
        RunMode::UntilSignal => park_until_terminated(),
    }

    drop(_secure_hold);

    foreground.stop();
    screen.stop();
    input.stop();
    audio.stop();
    mic.stop();

    drop(handle);
    let segments_written = drain
        .join()
        .map_err(|_| DaemonError::Store("drain thread panicked".into()))??;

    Ok(TimedRunReport {
        seconds: 0,
        screen_frame: counts.screen_frame.load(),
        keystroke: counts.keystroke.load(),
        pointer: counts.pointer.load(),
        foreground_app: counts.foreground_app.load(),
        window_title: counts.window_title.load(),
        secure_input_keystrokes: secure_violations.count(),
        segments_written,
        failed_sources: failed
            .iter()
            .map(|f| format!("{}: {}", f.name, f.error))
            .collect(),
    })
}

/// Block until the process receives SIGTERM or SIGINT, so the always-on daemon
/// runs until launchd (or a user) stops it, then returns so the sources stop and
/// the final batch flushes.
fn park_until_terminated() {
    use std::sync::atomic::{AtomicBool, Ordering};

    static TERMINATED: AtomicBool = AtomicBool::new(false);
    extern "C" fn on_signal(_sig: libc::c_int) {
        TERMINATED.store(true, Ordering::SeqCst);
    }
    // SAFETY: installing a handler for two termination signals; the handler only
    // stores to a static atomic, which is async-signal-safe.
    let handler = on_signal as *const () as libc::sighandler_t;
    unsafe {
        libc::signal(libc::SIGTERM, handler);
        libc::signal(libc::SIGINT, handler);
    }
    while !TERMINATED.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[derive(Default)]
struct Counts {
    screen_frame: Counter,
    keystroke: Counter,
    pointer: Counter,
    foreground_app: Counter,
    window_title: Counter,
}

impl Counts {
    fn record(&self, modality: Modality) {
        match modality {
            Modality::ScreenFrame => self.screen_frame.inc(),
            Modality::Keystroke => self.keystroke.inc(),
            Modality::Pointer => self.pointer.inc(),
            Modality::ForegroundApp => self.foreground_app.inc(),
            Modality::WindowTitle => self.window_title.inc(),
            Modality::SystemAudio | Modality::Microphone | Modality::Camera => {}
        }
    }
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
