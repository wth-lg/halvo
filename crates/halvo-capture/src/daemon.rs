//! The capture daemon: the always-on process that owns the single writer and
//! appends every captured event to the store. It is the sole writer of the
//! segment store; events reach it through one bounded single-writer sink, so no
//! store ever has two writers.
//!
//! The daemon batches the captured events it drains and persists each batch as
//! one encrypted segment, then records every event's row pointing into that
//! segment. The payload of an R1 segment is the canonical serialization of the
//! capture-stream events in the batch, which is the captured data this release
//! holds: the backends emit metadata-only events (pixel and audio payloads are a
//! later phase), so the event records are what the encrypted store persists.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::event::{CaptureStreamEvent, SegmentRef};
use crate::sink::SinkWriter;
use crate::store::db::EncryptedStore;
use crate::store::{SegmentStore, StoreError, DEFAULT_RETENTION};

/// How many captured events the daemon buffers before it flushes them as one
/// encrypted segment. Bounds resident memory: at most this many event records,
/// each a few hundred bytes, are held before the batch is written and cleared.
pub(crate) const SEGMENT_BATCH_MAX: usize = 512;

/// How long the drain waits for the next event before flushing a partial batch,
/// so a low event rate still persists promptly and the buffer never lingers.
pub(crate) const FLUSH_INTERVAL: Duration = Duration::from_millis(500);

/// Owns the one [`SinkWriter`] and the sole-writer store. Construct it with
/// [`CaptureDaemon::new`], then [`CaptureDaemon::run`] drains the sink into the
/// store, persisting each batch of events as one encrypted segment, until every
/// [`crate::sink::SinkHandle`] has been dropped.
pub struct CaptureDaemon {
    writer: SinkWriter,
    store: Box<dyn SegmentStore + Send>,
}

impl CaptureDaemon {
    pub fn new(writer: SinkWriter, store: Box<dyn SegmentStore + Send>) -> Self {
        Self { writer, store }
    }

    /// Drain the sink into the store until the channel closes.
    pub fn run(mut self) -> Result<(), DaemonError> {
        drain_to_store(&self.writer, self.store.as_mut(), |_| {}).map(|_| ())
    }
}

/// Serialize a batch of events into the bytes of one segment and the byte range
/// each event occupies within it. The payload is one canonical JSON record per
/// event, newline-terminated; each event's [`SegmentRef`] points at the offset
/// and length of its own record, so a reader can recover any single event.
pub(crate) fn serialize_batch(events: &[CaptureStreamEvent]) -> (Vec<u8>, Vec<(u64, u64)>) {
    let mut bytes = Vec::new();
    let mut ranges = Vec::with_capacity(events.len());
    for event in events {
        let offset = bytes.len() as u64;
        // CaptureStreamEvent is Serialize; a record always encodes.
        let record = serde_json::to_vec(event).unwrap_or_default();
        bytes.extend_from_slice(&record);
        ranges.push((offset, record.len() as u64));
        bytes.push(b'\n');
    }
    (bytes, ranges)
}

/// Write the batch as one encrypted segment and append each event's row with its
/// real segment reference, then clear the batch. Returns whether a segment was
/// written; an empty batch writes nothing.
fn flush_batch(
    store: &mut dyn SegmentStore,
    batch: &mut Vec<CaptureStreamEvent>,
) -> Result<bool, DaemonError> {
    if batch.is_empty() {
        return Ok(false);
    }
    let (bytes, ranges) = serialize_batch(batch);
    let (segment_ref, segment_key_id) = store
        .write_segment(&mut Cursor::new(bytes))
        .map_err(|e| DaemonError::Store(e.to_string()))?;
    for (event, (byte_offset, byte_len)) in batch.drain(..).zip(ranges) {
        let mut event = event;
        event.segment_ref = SegmentRef {
            segment_id: segment_ref.segment_id,
            byte_offset,
            byte_len,
        };
        event.segment_key_id = segment_key_id;
        store
            .append_event(&event)
            .map_err(|e| DaemonError::Store(e.to_string()))?;
    }
    Ok(true)
}

/// Drain `writer` into `store`, batching events into encrypted segments. Calls
/// `on_event` for every event received (the caller counts per modality), flushes
/// a full batch at [`SEGMENT_BATCH_MAX`] and a partial batch after
/// [`FLUSH_INTERVAL`] of silence, and flushes the remainder when the sink closes.
/// Returns the number of encrypted segments written.
pub(crate) fn drain_to_store(
    writer: &SinkWriter,
    store: &mut dyn SegmentStore,
    mut on_event: impl FnMut(&CaptureStreamEvent),
) -> Result<u64, DaemonError> {
    use std::sync::mpsc::RecvTimeoutError;

    let mut batch: Vec<CaptureStreamEvent> = Vec::with_capacity(SEGMENT_BATCH_MAX);
    let mut segments = 0u64;
    loop {
        match writer.receiver().recv_timeout(FLUSH_INTERVAL) {
            Ok(event) => {
                on_event(&event);
                batch.push(event);
                if batch.len() >= SEGMENT_BATCH_MAX {
                    segments += u64::from(flush_batch(store, &mut batch)?);
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                segments += u64::from(flush_batch(store, &mut batch)?)
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    segments += u64::from(flush_batch(store, &mut batch)?);
    Ok(segments)
}

/// Open the persistent capture store at `dir`, creating it on first run. A
/// directory that already holds a store is reopened through its recorded
/// custody (the owner's no-prompt unwrap); an empty directory gets a fresh store
/// under a dev-fallback root. Idempotent: a restart reopens the same store.
pub fn open_or_create_store(dir: &Path) -> Result<EncryptedStore, StoreError> {
    use crate::custody::dev_fallback::DevFallbackCustody;
    use crate::custody::RootId;
    use crate::store::db::KeyFile;

    if KeyFile::path(dir).exists() {
        EncryptedStore::open(dir, DEFAULT_RETENTION)
    } else {
        let custody = Box::new(DevFallbackCustody::new(RootId::fresh()));
        EncryptedStore::create(dir, custody, DEFAULT_RETENTION)
    }
}

/// The result of a bounded timed run: per-modality event counts, the
/// secure-input keystroke count, and any sources that failed to start.
/// [`TimedRunReport::render`] prints the exact lines the gate's timed-run,
/// secure-input and source sub-checks parse.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TimedRunReport {
    pub seconds: u64,
    pub screen_frame: u64,
    pub keystroke: u64,
    pub pointer: u64,
    pub foreground_app: u64,
    pub window_title: u64,
    pub secure_input_keystrokes: u64,
    pub segments_written: u64,
    /// Human-readable `name: error` for each source that did not start.
    pub failed_sources: Vec<String>,
}

impl TimedRunReport {
    /// Render the report in the gate's line format.
    pub fn render(&self) -> String {
        let mut out = format!(
            "modality=screen_frame count={}\n\
             modality=keystroke count={}\n\
             modality=pointer count={}\n\
             modality=foreground_app count={}\n\
             modality=window_title count={}\n\
             secure_input_keystrokes={}\n\
             segments_written={}\n\
             failed_sources={}\n",
            self.screen_frame,
            self.keystroke,
            self.pointer,
            self.foreground_app,
            self.window_title,
            self.secure_input_keystrokes,
            self.segments_written,
            self.failed_sources.len(),
        );
        for failure in &self.failed_sources {
            out.push_str(&format!("failed_source={failure}\n"));
        }
        out
    }
}

/// Run a bounded capture for the gate's `--timed-run`, dispatched to the active
/// platform backend. `secure_input` holds secure input for the window so the
/// password-field zero-keystroke invariant can be measured; the caller reads the
/// switch at the CLI boundary and passes it here.
pub fn timed_run(seconds: u64, secure_input: bool) -> Result<TimedRunReport, DaemonError> {
    #[cfg(target_os = "macos")]
    {
        crate::macos::timed_run(seconds, secure_input)
    }
    #[cfg(target_os = "windows")]
    {
        let _ = secure_input;
        crate::windows::timed_run(seconds)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (seconds, secure_input);
        Err(DaemonError::NotImplemented("timed-run on this platform"))
    }
}

/// Run the always-on capture daemon, persisting to the store at `store_dir`,
/// until the process is terminated. Dispatched to the active platform backend.
pub fn service_run(store_dir: PathBuf) -> Result<(), DaemonError> {
    #[cfg(target_os = "macos")]
    {
        crate::macos::service_run(store_dir)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = store_dir;
        Err(DaemonError::NotImplemented("service-run on this platform"))
    }
}

/// The default persistent store directory for the always-on daemon when no
/// `HALVO_STORE_DIR` override is set.
pub fn default_store_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("HALVO_STORE_DIR") {
        return PathBuf::from(dir);
    }
    let base = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    #[cfg(target_os = "macos")]
    {
        PathBuf::from(base)
            .join("Library")
            .join("Application Support")
            .join("Halvo")
            .join("store")
    }
    #[cfg(not(target_os = "macos"))]
    {
        PathBuf::from(base).join(".halvo").join("store")
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DaemonError {
    #[error("store error: {0}")]
    Store(String),
    #[error("{0} is not implemented in this phase")]
    NotImplemented(&'static str),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{CapturedAt, Modality, MonotonicSequence, SegmentKeyId};
    use std::time::UNIX_EPOCH;
    use uuid::Uuid;

    fn event(modality: Modality, seq: u64) -> CaptureStreamEvent {
        CaptureStreamEvent::new_raw(
            CapturedAt {
                wall_clock: UNIX_EPOCH,
                sequence: MonotonicSequence::new(seq),
            },
            modality,
            "com.apple.Terminal".to_string(),
            SegmentRef {
                segment_id: Uuid::nil(),
                byte_offset: 0,
                byte_len: 0,
            },
            SegmentKeyId(0),
        )
    }

    #[test]
    fn report_renders_the_gate_lines() {
        let report = TimedRunReport {
            seconds: 600,
            screen_frame: 3,
            keystroke: 2,
            pointer: 5,
            foreground_app: 1,
            window_title: 4,
            secure_input_keystrokes: 0,
            segments_written: 7,
            failed_sources: vec!["microphone: the capture permission is not granted".to_string()],
        };
        let text = report.render();
        assert!(text.contains("modality=screen_frame count=3"));
        assert!(text.contains("modality=keystroke count=2"));
        assert!(text.contains("modality=foreground_app count=1"));
        assert!(text.contains("secure_input_keystrokes=0"));
        assert!(text.contains("segments_written=7"));
        assert!(text.contains("failed_sources=1"));
        assert!(text.contains("failed_source=microphone: the capture permission is not granted"));
    }

    #[test]
    fn a_batch_serializes_to_one_record_per_event_with_recoverable_ranges() {
        let events = vec![event(Modality::Keystroke, 0), event(Modality::Pointer, 1)];
        let (bytes, ranges) = serialize_batch(&events);
        assert_eq!(ranges.len(), 2);
        for (event, (offset, len)) in events.iter().zip(&ranges) {
            let slice = &bytes[*offset as usize..(*offset + *len) as usize];
            let decoded: CaptureStreamEvent = serde_json::from_slice(slice).unwrap();
            assert_eq!(&decoded, event);
        }
    }

    #[test]
    fn the_daemon_persists_batched_events_as_segments_with_real_refs() {
        use crate::sink;
        use crate::store::db::test_support::make_store;

        let dir = tempfile::tempdir().unwrap();
        let mut store = make_store(dir.path());
        let (handle, writer) = sink::channel(16);
        for seq in 0..3 {
            use crate::source::EventSink;
            handle.emit(event(Modality::ScreenFrame, seq)).unwrap();
        }
        drop(handle);
        drain_to_store(&writer, &mut *store, |_| {}).unwrap();

        // One row per event, each pointing at a real (non-nil) segment.
        let conn = store.db().unwrap().connection();
        let rows: i64 = conn
            .query_row("SELECT count(*) FROM events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 3);
        let nil = Uuid::nil().simple().to_string();
        let real: i64 = conn
            .query_row(
                "SELECT count(*) FROM events WHERE segment_id != ?1 AND byte_len > 0",
                [nil],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(real, 3, "every persisted event points at a real segment");
        // The store holds at least one keyed segment.
        assert!(store.size_bytes().unwrap() > 0);
    }
}
