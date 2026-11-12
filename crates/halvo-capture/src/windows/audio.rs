//! System-audio capture, present but off by default.

use crate::event::Modality;
use crate::sink::SinkHandle;
use crate::source::{CaptureError, CaptureSource};

/// The system-audio source. The default constructor leaves it disabled, so audio
/// is never captured unless something explicitly enables it.
pub struct SystemAudioSource {
    enabled: bool,
}

impl SystemAudioSource {
    /// A disabled source: system audio off by default.
    pub fn disabled() -> Self {
        Self { enabled: false }
    }

    /// An enabled source.
    pub fn enabled() -> Self {
        Self { enabled: true }
    }

    /// Whether this source would capture audio.
    pub fn is_enabled(self) -> bool {
        self.enabled
    }
}

impl Default for SystemAudioSource {
    fn default() -> Self {
        Self::disabled()
    }
}

impl CaptureSource for SystemAudioSource {
    fn modality(&self) -> Modality {
        Modality::SystemAudio
    }

    fn start(&mut self, _sink: SinkHandle) -> Result<(), CaptureError> {
        if !self.enabled {
            return Ok(());
        }
        Err(CaptureError::NotImplemented(
            "Windows system-audio WASAPI loopback",
        ))
    }

    fn stop(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_audio_is_off_by_default() {
        assert!(!SystemAudioSource::default().is_enabled());
        assert!(!SystemAudioSource::disabled().is_enabled());
    }

    #[test]
    fn a_disabled_source_answers_off_and_yields_no_event() {
        use crate::source::{SourceContext, SourceState, Started};
        let (sink, writer) = crate::sink::channel(4);
        let mut source = SystemAudioSource::disabled();
        let started = source.begin(SourceContext::new(sink.clone()));
        let state = source.health().state;
        let stopped = source.shutdown();
        assert_eq!(started, Ok(Started::Off));
        assert_eq!(state, SourceState::Off);
        assert_eq!(stopped, Ok(()));
        assert!(
            matches!(
                writer.recv_timeout(std::time::Duration::from_millis(200)),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            ),
            "the off source yielded an event"
        );
        drop(sink);
    }

    #[test]
    fn an_enabled_source_defers_to_functional_capture() {
        let (sink, _writer) = crate::sink::channel(1);
        let mut source = SystemAudioSource::enabled();
        assert!(matches!(
            source.start(sink),
            Err(CaptureError::NotImplemented(_))
        ));
    }
}
