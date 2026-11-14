//! Foreground-app capture via `NSWorkspace`, which also drives window titles.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime};

use objc2::rc::autoreleasepool;
use objc2_app_kit::NSWorkspace;
use uuid::Uuid;

use crate::event::{CaptureStreamEvent, CapturedAt, Modality, SegmentKeyId, SegmentRef};
use crate::macos::window_title;
use crate::sink::{SinkError, SinkHandle};
use crate::source::{CaptureError, CaptureSource, EmitOutcome, EventSink};

/// How often the foreground poller samples `NSWorkspace.frontmostApplication`.
pub const FOREGROUND_POLL: Duration = Duration::from_millis(400);

/// The write side of the current foreground bundle id.
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

struct Frontmost {
    bundle_id: String,
    pid: libc::pid_t,
}

fn read_frontmost() -> Option<Frontmost> {
    autoreleasepool(|_| {
        let workspace = NSWorkspace::sharedWorkspace();
        let app = workspace.frontmostApplication()?;
        let bundle_id = app
            .bundleIdentifier()
            .map(|s| s.to_string())
            .unwrap_or_default();
        let pid = app.processIdentifier();
        Some(Frontmost { bundle_id, pid })
    })
}

type FrontReader = Box<dyn FnMut() -> Option<Frontmost> + Send>;
type TitleReader = Box<dyn FnMut(libc::pid_t) -> Option<String> + Send>;

struct Poller {
    writer: ForegroundWriter,
    read_title: TitleReader,
    last_bundle: String,
}

impl Poller {
    fn new(writer: ForegroundWriter, read_title: TitleReader) -> Self {
        Self {
            writer,
            read_title,
            last_bundle: String::new(),
        }
    }

    fn step(&mut self, reading: Option<Frontmost>, sink: &SinkHandle) {
        let Some(front) = reading else {
            return;
        };
        if front.bundle_id == self.last_bundle {
            return;
        }
        self.last_bundle = front.bundle_id.clone();
        self.writer.set(front.bundle_id.clone());

        let _ = emit_metadata_only(
            sink,
            Modality::ForegroundApp,
            front.bundle_id.clone(),
            Unpersisted::EMPTY,
        );

        if let Some(title) = (self.read_title)(front.pid) {
            let _ = emit_metadata_only(
                sink,
                Modality::WindowTitle,
                front.bundle_id.clone(),
                Unpersisted {
                    byte_len: title.len() as u64,
                },
            );
        }
    }
}

/// The foreground-app source (also emits window-title events on each change).
pub struct ForegroundSource {
    interval: Duration,
    poller: Option<(FrontReader, Poller)>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ForegroundSource {
    pub fn new(interval: Duration, foreground: ForegroundWriter) -> Self {
        Self::with_readers(
            interval,
            foreground,
            Box::new(read_frontmost),
            Box::new(window_title::focused_window_title),
        )
    }

    fn with_readers(
        interval: Duration,
        foreground: ForegroundWriter,
        read_front: FrontReader,
        read_title: TitleReader,
    ) -> Self {
        Self {
            interval,
            poller: Some((read_front, Poller::new(foreground, read_title))),
            stop: Arc::new(AtomicBool::new(false)),
            thread: None,
        }
    }
}

impl CaptureSource for ForegroundSource {
    fn modality(&self) -> Modality {
        Modality::ForegroundApp
    }

    fn start(&mut self, sink: SinkHandle) -> Result<(), CaptureError> {
        let stop = self.stop.clone();
        let (mut read_front, mut poller) = self.poller.take().ok_or(CaptureError::Start(
            "the foreground poller was already started".into(),
        ))?;
        let interval = self.interval;

        let thread = std::thread::Builder::new()
            .name("halvo-foreground".into())
            .spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    poller.step(read_front(), &sink);
                    std::thread::sleep(interval);
                }
            })
            .map_err(|e| CaptureError::Start(format!("foreground thread: {e}")))?;

        self.thread = Some(thread);
        Ok(())
    }

    fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sink::{self, SinkWriter};

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

    #[test]
    fn two_emits_carry_increasing_sequences_and_wall_clocks_between_the_calls() {
        let (handle, writer) = sink::channel(2);
        let before = SystemTime::now();
        for modality in [Modality::ForegroundApp, Modality::WindowTitle] {
            emit_metadata_only(&handle, modality, String::new(), Unpersisted::EMPTY).unwrap();
        }
        let after = SystemTime::now();
        drop(handle);
        let stamps: Vec<CapturedAt> = std::iter::from_fn(|| writer.recv())
            .map(|event| event.captured_at)
            .collect();
        assert_eq!(stamps.len(), 2);
        assert!(
            stamps[0].sequence < stamps[1].sequence,
            "sequences {:?} then {:?}",
            stamps[0].sequence,
            stamps[1].sequence
        );
        for stamp in &stamps {
            assert!(
                before <= stamp.wall_clock && stamp.wall_clock <= after,
                "wall clock {:?} outside {before:?} to {after:?}",
                stamp.wall_clock
            );
        }
        assert!(stamps[0].wall_clock <= stamps[1].wall_clock);
    }

    fn app(bundle_id: &str, pid: libc::pid_t) -> Option<Frontmost> {
        Some(Frontmost {
            bundle_id: bundle_id.into(),
            pid,
        })
    }

    fn poll(
        readings: Vec<Option<Frontmost>>,
        read_title: TitleReader,
    ) -> (Vec<(Modality, String, u64)>, String) {
        let (writer, reader) = foreground_state();
        let mut poller = Poller::new(writer, read_title);
        let (handle, sink_writer): (SinkHandle, SinkWriter) = sink::channel(64);
        for reading in readings {
            poller.step(reading, &handle);
        }
        drop(handle);
        let events = std::iter::from_fn(|| sink_writer.recv())
            .map(|e| (e.modality, e.foreground_app, e.segment_ref.byte_len))
            .collect();
        (events, reader.get())
    }

    #[test]
    fn a_then_b_then_a_gives_three_foreground_app_events_and_the_reader_ends_at_a() {
        let (events, reader) = poll(
            vec![
                app("com.example.a", 10),
                app("com.example.b", 20),
                app("com.example.a", 10),
            ],
            Box::new(|_: libc::pid_t| None),
        );
        let app_event = |id: &str| (Modality::ForegroundApp, id.to_string(), 0);
        assert_eq!(
            events,
            [
                app_event("com.example.a"),
                app_event("com.example.b"),
                app_event("com.example.a"),
            ]
        );
        assert_eq!(reader, "com.example.a");
    }

    #[test]
    fn an_unchanged_app_for_five_readings_gives_one_event() {
        let (events, reader) = poll(
            (0..5).map(|_| app("com.example.a", 10)).collect(),
            Box::new(|_: libc::pid_t| None),
        );
        assert_eq!(
            events,
            [(Modality::ForegroundApp, "com.example.a".to_string(), 0)]
        );
        assert_eq!(reader, "com.example.a");
    }

    #[test]
    fn a_started_source_emits_on_each_change_of_the_front_app() {
        let (handle, writer) = sink::channel(64);
        let (fg_writer, reader) = foreground_state();
        let script: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(vec!["a", "b", "a"]));
        let feed = script.clone();
        let read_front: FrontReader = Box::new(move || {
            let mut left = feed.lock().unwrap_or_else(PoisonError::into_inner);
            if left.is_empty() {
                None
            } else {
                Some(Frontmost {
                    bundle_id: left.remove(0).to_string(),
                    pid: 1,
                })
            }
        });
        let read_title: TitleReader = Box::new(|_pid| None);
        let mut source = ForegroundSource::with_readers(
            Duration::from_millis(1),
            fg_writer,
            read_front,
            read_title,
        );
        source
            .start(handle)
            .expect("the source starts without a grant");
        let started = std::time::Instant::now();
        while !script
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_empty()
        {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "the loop took no reading within 5 s"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        source.stop();
        let events: Vec<(Modality, String)> = std::iter::from_fn(|| writer.recv())
            .map(|e| (e.modality, e.foreground_app))
            .collect();
        assert_eq!(
            events,
            vec![
                (Modality::ForegroundApp, "a".to_string()),
                (Modality::ForegroundApp, "b".to_string()),
                (Modality::ForegroundApp, "a".to_string()),
            ]
        );
        assert_eq!(reader.get(), "a");
    }

    #[test]
    fn a_title_read_after_a_change_gives_one_window_title_event() {
        let title_reads = Arc::new(Mutex::new(Vec::new()));
        let reads = title_reads.clone();
        let (events, _) = poll(
            (0..3).map(|_| app("com.example.a", 10)).collect(),
            Box::new(move |pid: libc::pid_t| {
                reads
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(pid);
                Some("Untitled 1".to_string())
            }),
        );
        assert_eq!(
            events,
            [
                (Modality::ForegroundApp, "com.example.a".to_string(), 0),
                (Modality::WindowTitle, "com.example.a".to_string(), 10),
            ]
        );
        assert_eq!(
            *title_reads.lock().unwrap_or_else(PoisonError::into_inner),
            [10]
        );
    }
}
