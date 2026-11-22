//! The source set of a Windows service run.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use halvo_control::proto::SourceName;

use super::foreground::ForegroundAppSource;
use super::input::RawInputSource;
use super::window_title::WindowTitleSource;
use crate::daemon::sources::SourceFactory;
use crate::secure_input::windows::PasswordGate;
use crate::source::{CaptureError, CaptureSource, SourceContext, SourceHealth, Started};

/// The factory of a Windows service run: the foreground source, the pointer
/// source of Raw Input and the window_title source. It makes no keyboard
/// source, no screen source and no system_audio source for any name, whatever
/// the state file's switches say: no service run takes typing (contracts.md
/// C13.8), and the screen source waits for the cut that brings its second
/// session. The supervisor holds at most one source of a name, so the run has
/// one Raw Input registrant for the pointer and none for the keyboard.
///
/// The set holds one password gate. Its detector runs on a thread of its own,
/// with no keyboard source, from the begin of the set's first content source
/// to the stop of its last one.
#[derive(Default)]
pub struct ServiceSet {
    last_app: Arc<Mutex<Option<String>>>,
    hold: Arc<GateHold>,
}

impl ServiceSet {
    /// The set of a service run.
    pub fn new() -> ServiceSet {
        ServiceSet::default()
    }

    #[cfg(test)]
    fn with_gate(gate: PasswordGate) -> ServiceSet {
        ServiceSet {
            hold: Arc::new(GateHold {
                gate: Arc::new(gate),
                ..GateHold::default()
            }),
            ..ServiceSet::default()
        }
    }

    /// The set's one password gate.
    pub fn password_gate(&self) -> Arc<PasswordGate> {
        Arc::clone(&self.hold.gate)
    }

    fn gated<S: CaptureSource>(&self, source: S) -> Gated<S> {
        Gated {
            source,
            hold: Arc::clone(&self.hold),
            holding: false,
        }
    }
}

/// The set's gate and the number of its content sources that run.
#[derive(Debug, Default)]
struct GateHold {
    gate: Arc<PasswordGate>,
    holders: Mutex<usize>,
    said: AtomicBool,
    #[cfg(test)]
    lines_said: std::sync::atomic::AtomicU32,
}

impl GateHold {
    fn take(&self) {
        let mut holders = self.holders.lock().unwrap_or_else(PoisonError::into_inner);
        if *holders == 0 {
            if let Err(error) = self.gate.attach() {
                if !self.said.swap(true, Ordering::Relaxed) {
                    #[cfg(test)]
                    self.lines_said.fetch_add(1, Ordering::Relaxed);
                    eprintln!(
                        "halvo-capture: the password-field detector did not start ({error}); \
                         window titles and screen frames are dropped"
                    );
                }
            }
        }
        *holders += 1;
    }

    fn give_back(&self) {
        let mut holders = self.holders.lock().unwrap_or_else(PoisonError::into_inner);
        *holders = holders.saturating_sub(1);
        if *holders == 0 {
            let _ = self.gate.detach(super::INPUT_STOP_DEADLINE);
        }
    }
}

/// A content source of the service set: it holds the set's password gate
/// from its begin to its stop, so the gate's detector runs before the source
/// takes its first title or frame.
pub struct Gated<S> {
    source: S,
    hold: Arc<GateHold>,
    holding: bool,
}

impl<S> Gated<S> {
    /// The source inside.
    pub fn source(&self) -> &S {
        &self.source
    }

    fn give_back(&mut self) {
        if std::mem::take(&mut self.holding) {
            self.hold.give_back();
        }
    }
}

impl<S: CaptureSource> CaptureSource for Gated<S> {
    fn begin(&mut self, ctx: SourceContext) -> Result<Started, CaptureError> {
        let took = !self.holding;
        if took {
            self.hold.take();
            self.holding = true;
        }
        let begun = self.source.begin(ctx);
        if took && !matches!(begun, Ok(Started::Running)) {
            self.give_back();
        }
        begun
    }

    fn shutdown(&mut self) -> Result<(), CaptureError> {
        let stopped = self.source.shutdown();
        if stopped.is_ok() {
            self.give_back();
        }
        stopped
    }

    fn health(&self) -> SourceHealth {
        self.source.health()
    }
}

impl<S> Drop for Gated<S> {
    fn drop(&mut self) {
        self.give_back();
    }
}

impl SourceFactory for ServiceSet {
    fn make(&mut self, name: SourceName) -> Option<Box<dyn CaptureSource>> {
        match name {
            SourceName::Foreground => Some(Box::new(ForegroundAppSource::new())),
            SourceName::Pointer => Some(Box::new(RawInputSource::pointer(Arc::clone(
                &self.last_app,
            )))),
            SourceName::WindowTitle => Some(Box::new(self.gated(WindowTitleSource::new()))),
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
    use crate::secure_input::windows::{FieldState, PasswordGate};
    use crate::source::{CaptureError, CaptureSource, SourceContext, SourceState, Started};

    struct StandIn;

    impl CaptureSource for StandIn {
        fn begin(&mut self, _ctx: SourceContext) -> Result<Started, CaptureError> {
            Ok(Started::Running)
        }

        fn shutdown(&mut self) -> Result<(), CaptureError> {
            Ok(())
        }
    }

    #[test]
    fn the_windows_service_set_attaches_the_detector_with_no_keyboard_source() {
        use std::sync::atomic::Ordering;
        let _serial = crate::windows::tests::serial();
        let (sink, _writer) = crate::sink::channel(4);
        let ctx = SourceContext::new(sink);
        let mut set = ServiceSet::new();
        let gate = set.password_gate();
        let mut first = set.gated(StandIn);
        let mut second = set.gated(StandIn);
        let made = gate.is_attached();
        let first_begun = first.begin(ctx.clone());
        let after_the_first_begin = gate.is_attached();
        let second_begun = second.begin(ctx.clone());
        let first_stopped = first.shutdown();
        let after_the_first_stop = gate.is_attached();
        let second_stopped = second.shutdown();
        let after_the_last_stop = gate.is_attached();
        assert_eq!(
            (
                made,
                after_the_first_begin,
                after_the_first_stop,
                after_the_last_stop
            ),
            (false, true, true, false)
        );
        assert_eq!(
            (first_begun, second_begun),
            (Ok(Started::Running), Ok(Started::Running))
        );
        assert_eq!((first_stopped, second_stopped), (Ok(()), Ok(())));
        for name in [SourceName::Keyboard, SourceName::Input, SourceName::Screen] {
            assert!(set.make(name).is_none(), "{name:?}");
        }

        let failing = ServiceSet::with_gate(PasswordGate::whose_attach_fails());
        let mut source = failing.gated(StandIn);
        let begun = source.begin(ctx.clone());
        let attached = failing.hold.gate.is_attached();
        let reading = failing.hold.gate.field_now();
        let _ = source.shutdown();
        let again = source.begin(ctx);
        let _ = source.shutdown();
        assert_eq!((begun, again), (Ok(Started::Running), Ok(Started::Running)));
        assert!(!attached, "a failed attach left a detector");
        assert_eq!(reading, FieldState::Unknown);
        assert_eq!(failing.hold.lines_said.load(Ordering::Relaxed), 1);
    }

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
