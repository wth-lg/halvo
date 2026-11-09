//! Microphone capture source: present, but off by default in R1.

use crate::event::Modality;
use crate::sink::SinkHandle;
use crate::source::{CaptureError, CaptureSource};

/// Microphone capture. Off by default.
pub struct MicrophoneSource {
    enabled: bool,
}

impl MicrophoneSource {
    pub fn disabled() -> Self {
        Self { enabled: false }
    }

    /// Construct it enabled. R1 does not wire the capture, so [`Self::start`]
    /// returns [`CaptureError::NotImplemented`].
    pub fn enabled() -> Self {
        Self { enabled: true }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}

impl CaptureSource for MicrophoneSource {
    fn modality(&self) -> Modality {
        Modality::Microphone
    }

    fn start(&mut self, _sink: SinkHandle) -> Result<(), CaptureError> {
        if self.enabled {
            return Err(CaptureError::NotImplemented(
                "microphone capture is off by default in R1",
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
        let mut source = MicrophoneSource::disabled();
        assert!(!source.is_enabled());
        assert!(source.start(handle).is_ok());
        source.stop();
    }

    #[test]
    fn enabled_start_reports_not_implemented_in_r1() {
        let (handle, _writer) = sink::channel(1);
        let mut source = MicrophoneSource::enabled();
        assert!(matches!(
            source.start(handle),
            Err(CaptureError::NotImplemented(_))
        ));
    }

    #[test]
    fn modality_is_microphone() {
        assert_eq!(
            MicrophoneSource::disabled().modality(),
            Modality::Microphone
        );
    }
}
