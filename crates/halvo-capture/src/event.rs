//! The capture stream event: the one record every capture source emits.
//!
//! The field set and the two enums are the kickoff plan's "Capture stream event"
//! exactly. The types are built so illegal states cannot be represented: the
//! modality and retention class are closed enums, and the monotonic sequence is
//! handed out only by the single-writer sink (see [`crate::sink`]).

use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub use halvo_control::model::Modality;

/// How long a record may be kept. Raw capture and derived data have different
/// retention bounds; the enum is closed so every store path handles both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetentionClass {
    Raw,
    Derived,
}

/// A monotonic sequence number within one capture stream. The ordering authority
/// is the single-writer sink; see [`crate::sink::SinkHandle::next_sequence`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct MonotonicSequence(u64);

impl MonotonicSequence {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// When a capture event happened: a wall-clock UTC instant plus the stream's
/// monotonic sequence. The sequence totally orders events even when two share a
/// wall-clock reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapturedAt {
    pub wall_clock: SystemTime,
    pub sequence: MonotonicSequence,
}

/// A handle into the encrypted segment store: which segment holds this event's
/// bytes and the byte range within it. The store (phase P2) owns the file format.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SegmentRef {
    pub segment_id: Uuid,
    pub byte_offset: u64,
    pub byte_len: u64,
}

/// Selects the per-segment key that encrypts a segment, so destroying one key
/// crypto-shreds exactly one segment. The integer is the key table's primary key
/// inside the encrypted store (phase P2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SegmentKeyId(pub i64);

/// The capture stream event (kickoff plan, Core data structures).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureStreamEvent {
    /// UUID v7, so event ids sort by creation time.
    pub event_id: Uuid,
    pub captured_at: CapturedAt,
    pub modality: Modality,
    /// Bundle id (macOS) or executable path (Windows) of the foreground app.
    pub foreground_app: String,
    pub segment_ref: SegmentRef,
    pub segment_key_id: SegmentKeyId,
    /// False for raw capture; true for anything derived from it.
    pub is_derived: bool,
    pub retention_class: RetentionClass,
}

impl CaptureStreamEvent {
    /// A raw capture event with a fresh UUID v7 id. The caller supplies the
    /// timestamp (from the sink's sequence authority), modality and placement;
    /// `is_derived` is false and the retention class is raw by construction.
    pub fn new_raw(
        captured_at: CapturedAt,
        modality: Modality,
        foreground_app: String,
        segment_ref: SegmentRef,
        segment_key_id: SegmentKeyId,
    ) -> Self {
        Self {
            event_id: Uuid::now_v7(),
            captured_at,
            modality,
            foreground_app,
            segment_ref,
            segment_key_id,
            is_derived: false,
            retention_class: RetentionClass::Raw,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::UNIX_EPOCH;

    #[test]
    fn constructs_event_with_every_field() {
        let event = CaptureStreamEvent {
            event_id: Uuid::now_v7(),
            captured_at: CapturedAt {
                wall_clock: UNIX_EPOCH,
                sequence: MonotonicSequence::new(1),
            },
            modality: Modality::Keystroke,
            foreground_app: "com.apple.Terminal".to_string(),
            segment_ref: SegmentRef {
                segment_id: Uuid::now_v7(),
                byte_offset: 0,
                byte_len: 128,
            },
            segment_key_id: SegmentKeyId(7),
            is_derived: false,
            retention_class: RetentionClass::Raw,
        };

        assert_eq!(event.modality, Modality::Keystroke);
        assert_eq!(event.retention_class, RetentionClass::Raw);
        assert!(!event.is_derived);
        assert_eq!(event.foreground_app, "com.apple.Terminal");
        assert_eq!(event.segment_ref.byte_len, 128);
        assert_eq!(event.segment_key_id.0, 7);
        assert_eq!(event.captured_at.sequence.get(), 1);
    }

    #[test]
    fn new_raw_is_raw_and_not_derived() {
        let event = CaptureStreamEvent::new_raw(
            CapturedAt {
                wall_clock: UNIX_EPOCH,
                sequence: MonotonicSequence::new(0),
            },
            Modality::ScreenFrame,
            String::new(),
            SegmentRef {
                segment_id: Uuid::now_v7(),
                byte_offset: 0,
                byte_len: 0,
            },
            SegmentKeyId(1),
        );
        assert!(!event.is_derived);
        assert_eq!(event.retention_class, RetentionClass::Raw);
    }
}
