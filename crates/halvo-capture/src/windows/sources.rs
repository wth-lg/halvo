//! The source set of a Windows service run.

use std::sync::{Arc, Mutex};

use halvo_control::proto::SourceName;

use super::foreground::ForegroundAppSource;
use super::input::RawInputSource;
use super::window_title::WindowTitleSource;
use crate::daemon::sources::SourceFactory;
use crate::source::CaptureSource;

/// The factory of a Windows service run: the foreground source, the pointer
/// source of Raw Input and the window_title source. It makes no keyboard
/// source, no screen source and no system_audio source for any name, whatever
/// the state file's switches say: no service run takes typing (contracts.md
/// C13.8), and the screen source waits for the cut that brings its second
/// session. The supervisor holds at most one source of a name, so the run has
/// one Raw Input registrant for the pointer and none for the keyboard.
#[derive(Default)]
pub struct ServiceSet {
    last_app: Arc<Mutex<Option<String>>>,
}

impl ServiceSet {
    /// The set of a service run.
    pub fn new() -> ServiceSet {
        ServiceSet::default()
    }
}

impl SourceFactory for ServiceSet {
    fn make(&mut self, name: SourceName) -> Option<Box<dyn CaptureSource>> {
        match name {
            SourceName::Foreground => Some(Box::new(ForegroundAppSource::new())),
            SourceName::Pointer => Some(Box::new(RawInputSource::pointer(Arc::clone(
                &self.last_app,
            )))),
            SourceName::WindowTitle => Some(Box::new(WindowTitleSource::new())),
            SourceName::Keyboard
            | SourceName::Screen
            | SourceName::SystemAudio
            | SourceName::Input
            | SourceName::Microphone => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use halvo_control::proto::{Platform, SourceName};

    use super::ServiceSet;
    use crate::daemon::sources::{starting, RunKind, SourceFactory};
    use crate::floor::{Switch, Switches};
    use crate::source::SourceState;

    #[test]
    fn the_windows_service_set_makes_no_keyboard_source_whatever_the_switches_say() {
        let mut switches = Switches::default();
        for switch in Switch::ALL {
            switches.set(switch, true);
        }
        let made_by_the_set = [
            SourceName::Foreground,
            SourceName::Pointer,
            SourceName::WindowTitle,
        ];
        let mut set = ServiceSet::new();
        for name in SourceName::WINDOWS {
            let made = set.make(name);
            assert_eq!(made.is_some(), made_by_the_set.contains(&name), "{name:?}");
            if let Some(source) = made {
                assert_eq!(
                    source.health().state,
                    SourceState::NotReported,
                    "{name:?} began"
                );
            }
        }
        let wanted = starting(Platform::Windows, RunKind::Service, switches);
        assert!(!wanted.contains(&SourceName::Keyboard), "{wanted:?}");
        let started: Vec<SourceName> = wanted
            .into_iter()
            .filter(|name| set.make(*name).is_some())
            .collect();
        assert_eq!(started, made_by_the_set);
    }
}
