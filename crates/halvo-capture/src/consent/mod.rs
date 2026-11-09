//! Consent and capture-status types the view binds to. Traits and types only;
//! the persistent status indicator and the just-in-time permission onboarding are
//! phase P5 (status.rs, onboarding.rs), added as the sibling files already
//! declared here, so P5 never edits this file.

pub mod onboarding;
pub mod status;

use serde::{Deserialize, Serialize};

use crate::event::Modality;

/// Whether capture is currently running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureState {
    Running,
    Paused,
}

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

/// What the data view reads and controls. Phase P5 implements it against the
/// store and the daemon.
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
