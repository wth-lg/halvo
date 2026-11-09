//! System-audio capture, present but off by default.
//!
//! Halvo can record the system output mix through WASAPI loopback (researcher-4),
//! but audio is the most sensitive stream and stays off unless the user turns it
//! on. This source exists so the capability is wired and the default is explicit
//! and testable; the WASAPI loopback client itself is deferred with the rest of
//! Windows functional capture.

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

    /// An enabled source. The WASAPI loopback client is deferred, so starting one
    /// reports `NotImplemented` until functional capture lands on a real desktop.
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
            // Off by default: starting a disabled source is a no-op, not an error,
            // so the daemon can hold the source without ever capturing audio.
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
    fn a_disabled_source_starts_without_capturing() {
        let (sink, _writer) = crate::sink::channel(1);
        let mut source = SystemAudioSource::disabled();
        assert!(source.start(sink).is_ok());
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
