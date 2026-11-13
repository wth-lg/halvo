//! System-audio capture source.

use crate::event::Modality;
use crate::sink::SinkHandle;
use crate::source::{CaptureError, CaptureSource};

pub struct SystemAudioSource;

impl SystemAudioSource {
    pub fn disabled() -> Self {
        Self
    }
}

impl CaptureSource for SystemAudioSource {
    fn modality(&self) -> Modality {
        Modality::SystemAudio
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
    fn the_off_source_emits_no_event_between_start_and_stop() {
        let (handle, writer) = sink::channel(1);
        let mut source = SystemAudioSource::disabled();
        let _ = source.start(handle.clone());
        source.stop();
        drop(handle);
        assert!(writer.recv().is_none(), "the off source emitted an event");
    }

    #[test]
    fn modality_is_system_audio() {
        assert_eq!(
            SystemAudioSource::disabled().modality(),
            Modality::SystemAudio
        );
    }
}
