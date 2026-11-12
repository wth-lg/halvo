//! Consent and capture-status types the view binds to. Traits and types only;
//! the persistent status indicator and the just-in-time permission onboarding are
//! phase P5 (status.rs, onboarding.rs), added as the sibling files already
//! declared here, so P5 never edits this file.

pub mod onboarding;
pub mod status;

use serde::{Deserialize, Serialize};

pub use halvo_control::model::CaptureState;

use crate::event::Modality;

/// Whether the user has enabled a modality. Microphone, camera and system audio
/// are disabled by default in R1 (researcher-3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModalityConsent {
    Enabled,
    Disabled,
}

/// The status the data view shows: running or paused, and the store's size
/// against its cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureStatus {
    pub state: CaptureState,
    pub store_bytes: u64,
    pub store_cap_bytes: u64,
}

/// What the data view reads and controls.
pub trait ConsentView {
    fn status(&self) -> CaptureStatus;
    fn modality_consent(&self, modality: Modality) -> ModalityConsent;
    fn set_capture_state(&mut self, state: CaptureState) -> Result<(), ConsentError>;
    fn delete_all(&mut self) -> Result<(), ConsentError>;
}

#[derive(Debug, thiserror::Error)]
pub enum ConsentError {
    #[error("the consent action failed: {0}")]
    Action(String),
    #[error("{0} is not implemented in this phase")]
    NotImplemented(&'static str),
}

impl From<halvo_control::model::CapError> for ConsentError {
    fn from(error: halvo_control::model::CapError) -> Self {
        ConsentError::Action(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::status::StorageCap;
    use super::ConsentError;

    fn cap(bytes: u64) -> Result<StorageCap, ConsentError> {
        Ok(StorageCap::from_bytes(bytes)?)
    }

    #[test]
    fn a_refused_cap_converts_into_the_consent_error() {
        assert_eq!(cap(StorageCap::MIN.bytes()).unwrap(), StorageCap::MIN);
        let error = cap(5).unwrap_err();
        assert!(matches!(error, ConsentError::Action(_)), "{error:?}");
        assert_eq!(
            error.to_string(),
            "the consent action failed: the storage cap must be at least 1073741824 bytes"
        );
    }
}
