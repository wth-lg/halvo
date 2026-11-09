//! Microphone capture source: present, but off by default in R1.

use crate::event::Modality;
use crate::sink::SinkHandle;
use crate::source::{CaptureError, CaptureSource};

/// Microphone capture. Off by default; R1 does not wire the capture.
pub struct MicrophoneSource;

impl MicrophoneSource {
    pub fn disabled() -> Self {
        Self
    }
}

impl CaptureSource for MicrophoneSource {
    fn modality(&self) -> Modality {
        Modality::Microphone
    }

    fn start(&mut self, _sink: SinkHandle) -> Result<(), CaptureError> {
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
        assert!(source.start(handle).is_ok());
        source.stop();
    }

    #[test]
    fn modality_is_microphone() {
        assert_eq!(
            MicrophoneSource::disabled().modality(),
            Modality::Microphone
        );
    }
}
