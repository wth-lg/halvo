use std::fmt;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::floor::text::{CapturedText, TextField};

pub use halvo_control::model::Modality;

/// The app that an event belongs to, in one of two forms, so that no record
/// holds a mix of them: on macOS the bundle id, else the executable path; on
/// Windows the executable path. Each string passes the floor's text entry, so
/// it is at most 1,024 bytes; a path keeps its tail.
pub struct AppIdentity(AppForm);

enum AppForm {
    BundleId(CapturedText),
    ExecutablePath(CapturedText),
    Unknown,
}

impl AppIdentity {
    /// A bundle id; an empty string is [`AppIdentity::unknown`].
    pub fn bundle_id(raw: String) -> AppIdentity {
        AppIdentity::entered(TextField::BundleId, raw)
    }

    /// An executable path; an empty string is [`AppIdentity::unknown`].
    pub fn executable_path(raw: String) -> AppIdentity {
        AppIdentity::entered(TextField::ExecutablePath, raw)
    }

    /// No identity could be read; the record holds `null`.
    pub const fn unknown() -> AppIdentity {
        AppIdentity(AppForm::Unknown)
    }

    pub fn is_unknown(&self) -> bool {
        matches!(self.0, AppForm::Unknown)
    }

    fn entered(field: TextField, raw: String) -> AppIdentity {
        if raw.is_empty() {
            return AppIdentity::unknown();
        }
        let text = CapturedText::enter(field, raw);
        AppIdentity(match field {
            TextField::ExecutablePath => AppForm::ExecutablePath(text),
            _ => AppForm::BundleId(text),
        })
    }
}

impl fmt::Debug for AppIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self.0 {
            AppForm::BundleId(_) => "AppIdentity(bundle id)",
            AppForm::ExecutablePath(_) => "AppIdentity(executable path)",
            AppForm::Unknown => "AppIdentity(unknown)",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetentionClass {
    Raw,
    Derived,
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapturedAt {
    pub wall_clock: SystemTime,
    pub sequence: MonotonicSequence,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SegmentRef {
    pub segment_id: Uuid,
    pub byte_offset: u64,
    pub byte_len: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SegmentKeyId(pub i64);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureStreamEvent {
    pub event_id: Uuid,
    pub captured_at: CapturedAt,
    pub modality: Modality,
    pub foreground_app: String,
    pub segment_ref: SegmentRef,
    pub segment_key_id: SegmentKeyId,
    pub is_derived: bool,
    pub retention_class: RetentionClass,
}

impl CaptureStreamEvent {
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

    fn stored_app(app: &AppIdentity) -> Option<(TextField, String)> {
        match &app.0 {
            AppForm::BundleId(text) => Some((
                TextField::BundleId,
                crate::floor::cases::stored(text, TextField::BundleId),
            )),
            AppForm::ExecutablePath(text) => Some((
                TextField::ExecutablePath,
                crate::floor::cases::stored(text, TextField::ExecutablePath),
            )),
            AppForm::Unknown => None,
        }
    }

    #[test]
    fn a_64_kib_app_id_is_cut_to_the_cap_on_a_character_boundary() {
        let bundle = format!("com.example.{}", "\u{e9}".repeat(32 * 1024));
        assert!(bundle.len() > 64 * 1024);
        let (field, kept) = stored_app(&AppIdentity::bundle_id(bundle.clone())).unwrap();
        assert_eq!(field, TextField::BundleId);
        assert!(kept.len() <= 1024 && kept.len() >= 1021, "{}", kept.len());
        assert!(bundle.starts_with(&kept));

        let path = format!(
            "/Applications/{}/Tool.app/Contents/MacOS/tool",
            "\u{e9}".repeat(32 * 1024)
        );
        let (field, kept) = stored_app(&AppIdentity::executable_path(path.clone())).unwrap();
        assert_eq!(field, TextField::ExecutablePath);
        assert!(kept.len() <= 1024 && kept.len() >= 1021, "{}", kept.len());
        assert!(path.ends_with(&kept));
        assert!(kept.ends_with("/Contents/MacOS/tool"));
    }

    #[test]
    fn an_unknown_app_is_unknown_not_an_empty_string() {
        assert!(AppIdentity::bundle_id(String::new()).is_unknown());
        assert!(AppIdentity::executable_path(String::new()).is_unknown());
        assert!(AppIdentity::unknown().is_unknown());
        assert!(stored_app(&AppIdentity::bundle_id(String::new())).is_none());
        let named = AppIdentity::bundle_id("com.apple.Terminal".to_string());
        assert!(!named.is_unknown());
        assert_eq!(
            stored_app(&named),
            Some((TextField::BundleId, "com.apple.Terminal".to_string()))
        );
    }
}
