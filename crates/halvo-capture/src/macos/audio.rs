use crate::source::{
    CaptureError, CaptureSource, SourceContext, SourceCounters, SourceHealth, SourceState, Started,
};

pub struct SystemAudioSource {
    begun: bool,
}

impl SystemAudioSource {
    pub fn disabled() -> Self {
        Self { begun: false }
    }
}

impl CaptureSource for SystemAudioSource {
    fn begin(&mut self, _ctx: SourceContext) -> Result<Started, CaptureError> {
        if self.begun {
            return Err(CaptureError::AlreadyStarted);
        }
        self.begun = true;
        Ok(Started::Off)
    }

    fn shutdown(&mut self) -> Result<(), CaptureError> {
        self.begun = false;
        Ok(())
    }

    fn health(&self) -> SourceHealth {
        SourceHealth {
            state: SourceState::Off,
            counters: SourceCounters::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::macos::off_source_tests::the_off_source_keeps_its_lifecycle;

    #[test]
    fn the_off_source_answers_off_and_keeps_no_handle_of_the_sink() {
        the_off_source_keeps_its_lifecycle(SystemAudioSource::disabled());
    }
}
