//! System-audio capture source: present, but off by default in R1.
//!
//! Halvo records system audio only when the owner turns it on. The source exists
//! so the pipeline and the consent UI can reason about it, but it is constructed
//! disabled and starting it is a no-op that captures nothing. Turning it on is a
//! later-phase concern (it rides the ScreenCaptureKit stream's audio output), so
//! an enabled start returns a clear "not in R1" error rather than silently doing
//! nothing.

use crate::event::Modality;
use crate::sink::SinkHandle;
use crate::source::{CaptureError, CaptureSource};

/// System-audio capture. Off by default; see the module note.
pub struct SystemAudioSource {
    enabled: bool,
}

impl SystemAudioSource {
    /// The R1 default: present but off.
    pub fn disabled() -> Self {
        Self { enabled: false }
    }

    /// Construct it enabled. R1 does not wire the capture, so [`Self::start`]
    /// returns [`CaptureError::NotImplemented`]; the constructor exists so the
    /// off/on seam is real and the enabled branch is covered.
    pub fn enabled() -> Self {
        Self { enabled: true }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}

impl CaptureSource for SystemAudioSource {
    fn modality(&self) -> Modality {
        Modality::SystemAudio
    }

    fn start(&mut self, _sink: SinkHandle) -> Result<(), CaptureError> {
        if self.enabled {
            return Err(CaptureError::NotImplemented(
                "system audio capture is off by default in R1",
            ));
        }
        Ok(())
    }

    fn stop(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sink;

    #[test]
    fn disabled_by_default_and_start_is_a_no_op() {
        let (handle, _writer) = sink::channel(1);
        let mut source = SystemAudioSource::disabled();
        assert!(!source.is_enabled());
        assert!(source.start(handle).is_ok());
        source.stop();
    }

    #[test]
    fn enabled_start_reports_not_implemented_in_r1() {
        let (handle, _writer) = sink::channel(1);
        let mut source = SystemAudioSource::enabled();
        assert!(matches!(
            source.start(handle),
            Err(CaptureError::NotImplemented(_))
        ));
    }

    #[test]
    fn modality_is_system_audio() {
        assert_eq!(
            SystemAudioSource::disabled().modality(),
            Modality::SystemAudio
        );
    }
}
