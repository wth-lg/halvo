//! The source set of a Windows service run.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use halvo_control::proto::SourceName;

use super::foreground::ForegroundAppSource;
use super::input::{GateCounts, RawInputSource};
use super::screen::ScreenSource;
use super::window_title::WindowTitleSource;
use crate::daemon::sources::SourceFactory;
use crate::secure_input::windows::{Attached, PasswordGate};
use crate::source::{
    CaptureError, CaptureSource, SourceContext, SourceCounters, SourceHealth, SourceState, Started,
    StopCause,
};

/// The factory of a Windows service run: the foreground source, the pointer
/// and the keyboard source of Raw Input, the window_title source and the
/// screen source, which the run asks for only while the switch
/// `screen_activity` is on. It makes the keyboard source while the build's
/// one constant `TYPING_ON_WINDOWS` is true, and no system_audio source for
/// any name. The supervisor holds at most one source of a name, so the run
/// has one Raw Input registrant for each device.
///
/// The set holds one password gate. Its detector runs on a thread of its own
/// from the begin of the first of the set's holders to the stop of the last:
/// the keyboard, window_title and screen sources each hold the gate while they
/// run, and none of them attaches or detaches the detector by itself, so a
/// stop of one of them leaves the detector to the others. The window_title
/// and screen sources read the gate, and the keyboard source notes a key only
/// through it. The set keeps one set of counts for the run, which its
/// keyboard source writes and its figures read.
///
/// The set makes one screen source, at its first make of the name; every make
/// behind it gives a spent source that starts nothing, so that a process
/// holds at most one screen session (form B of contracts.md C14.3). The spent
/// source reads stopped, or failed with the first source's error where that
/// source's begin failed. A run makes one set.
#[derive(Default)]
pub struct ServiceSet {
    last_app: Arc<Mutex<Option<String>>>,
    hold: Arc<GateHold>,
    counts: Arc<GateCounts>,
    typing_holders: Arc<AtomicUsize>,
    screen_made: bool,
    screen_failed: Arc<Mutex<Option<CaptureError>>>,
    #[cfg(test)]
    first_screen: Option<Box<dyn CaptureSource>>,
    #[cfg(test)]
    typing_off: bool,
}

impl ServiceSet {
    /// The set of a service run.
    pub fn new() -> ServiceSet {
        ServiceSet::default()
    }

    /// The set of a build whose `TYPING_ON_WINDOWS` is false.
    #[cfg(test)]
    fn with_typing_off() -> ServiceSet {
        ServiceSet {
            typing_off: true,
            ..ServiceSet::default()
        }
    }

    /// Whether the set makes a keyboard source: the build's one constant
    /// `TYPING_ON_WINDOWS`.
    fn typing_on(&self) -> bool {
        #[cfg(test)]
        if self.typing_off {
            return false;
        }
        halvo_control::texts::TYPING_ON_WINDOWS
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

    /// The set whose first screen source is `first` in place of the gated
    /// one.
    #[cfg(test)]
    pub(crate) fn with_first_screen(first: Box<dyn CaptureSource>) -> ServiceSet {
        ServiceSet {
            first_screen: Some(first),
            ..ServiceSet::default()
        }
    }

    /// The set's one password gate.
    pub fn password_gate(&self) -> Arc<PasswordGate> {
        Arc::clone(&self.hold.gate)
    }

    /// A title source that reads the set's gate and holds it while it runs.
    pub fn gated_title(&self, source: WindowTitleSource) -> Gated<WindowTitleSource> {
        self.gated(source.with_password_gate(self.password_gate()))
    }

    /// A screen source that reads the set's gate and holds it while it runs.
    pub fn gated_screen(&self, source: ScreenSource) -> Gated<ScreenSource> {
        self.gated(source.with_password_gate(self.password_gate()))
    }

    /// The set's screen source: the gated one at the first call, the spent
    /// one at every call behind it.
    fn screen(&mut self) -> Box<dyn CaptureSource> {
        if std::mem::replace(&mut self.screen_made, true) {
            let failed = self
                .screen_failed
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone();
            return Box::new(SpentScreen { failed });
        }
        #[cfg(test)]
        if let Some(source) = self.first_screen.take() {
            return Box::new(FirstScreen {
                source,
                failed: Arc::clone(&self.screen_failed),
            });
        }
        Box::new(FirstScreen {
            source: Box::new(self.gated_screen(ScreenSource::new(Arc::clone(&self.last_app)))),
            failed: Arc::clone(&self.screen_failed),
        })
    }

    fn gated<S: CaptureSource>(&self, source: S) -> Gated<S> {
        Gated {
            source,
            hold: Arc::clone(&self.hold),
            holding: false,
            typing: None,
        }
    }

    /// The set's keyboard source: one more holder of the set's gate, which
    /// counts in the set's counts, and whose begin fails where the gate has no
    /// detector after its take.
    fn gated_keyboard(&self) -> Gated<RawInputSource> {
        Gated {
            source: RawInputSource::keyboard_of_the_set(
                self.password_gate(),
                Arc::clone(&self.counts),
            ),
            hold: Arc::clone(&self.hold),
            holding: false,
            typing: Some(Arc::clone(&self.typing_holders)),
        }
    }
}

impl crate::daemon::supervisor::SystemSources for ServiceSet {
    /// What the set's keyboard source counted in the run, and the reading of
    /// the set's gate while a keyboard source holds it; nothing of typing
    /// where no keyboard source ran.
    fn figures(&self) -> SourceCounters {
        SourceCounters {
            late_events: u64::from(self.counts.late.load(Ordering::Relaxed)),
            max_lag_ms: self.counts.max_lag_ms.load(Ordering::Relaxed),
            keys_withheld: self.counts.withheld.load(Ordering::Relaxed),
            typing_gate: (self.typing_holders.load(Ordering::Relaxed) > 0)
                .then(|| self.hold.gate.typing_gate()),
            ..SourceCounters::default()
        }
    }
}

/// The set's gate and its holders.
#[derive(Debug, Default)]
struct GateHold {
    gate: Arc<PasswordGate>,
    holders: Mutex<Holders>,
    said: AtomicBool,
    #[cfg(test)]
    lines_said: std::sync::atomic::AtomicU32,
}

/// The number of the set's sources that hold its gate, and the error of the
/// detector's attach where the first of them could not attach it.
#[derive(Debug, Default)]
struct Holders {
    count: usize,
    failed: Option<CaptureError>,
}

impl GateHold {
    /// Takes one hold: the first attaches the detector. Answers the error of
    /// that attach where the gate has no detector.
    fn take(&self) -> Result<(), CaptureError> {
        let mut holders = self.holders.lock().unwrap_or_else(PoisonError::into_inner);
        if holders.count == 0 {
            let attached = self.gate.attach_waiting();
            let line = match &attached {
                Ok(Attached::Read) => None,
                Ok(Attached::Late) => Some(
                    "halvo-capture: the password-field detector's first read is late; \
                     window titles and screen frames are dropped until it lands"
                        .to_string(),
                ),
                Err(error) => Some(format!(
                    "halvo-capture: the password-field detector did not start ({error}); \
                     window titles and screen frames are dropped"
                )),
            };
            if let Some(line) = line {
                if !self.said.swap(true, Ordering::Relaxed) {
                    #[cfg(test)]
                    self.lines_said.fetch_add(1, Ordering::Relaxed);
                    eprintln!("{line}");
                }
            }
            holders.failed = attached.err();
        }
        holders.count += 1;
        holders.failed.clone().map_or(Ok(()), Err)
    }

    /// Gives one hold back: the last detaches the detector.
    fn give_back(&self) {
        let mut holders = self.holders.lock().unwrap_or_else(PoisonError::into_inner);
        holders.count = holders.count.saturating_sub(1);
        if holders.count == 0 {
            holders.failed = None;
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
    /// For the set's keyboard source alone: the count of the set's keyboard
    /// sources that hold the gate, which the set's figures read.
    typing: Option<Arc<AtomicUsize>>,
}

impl<S> Gated<S> {
    /// The source inside.
    pub fn source(&self) -> &S {
        &self.source
    }

    fn give_back(&mut self) {
        if std::mem::take(&mut self.holding) {
            if let Some(typing) = &self.typing {
                typing.fetch_sub(1, Ordering::Relaxed);
            }
            self.hold.give_back();
        }
    }
}

impl<S: CaptureSource> CaptureSource for Gated<S> {
    fn begin(&mut self, ctx: SourceContext) -> Result<Started, CaptureError> {
        let took = !self.holding;
        if took {
            let detector = self.hold.take();
            self.holding = true;
            if let Some(typing) = &self.typing {
                typing.fetch_add(1, Ordering::Relaxed);
                if let Err(error) = detector {
                    self.give_back();
                    return Err(error);
                }
            }
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

/// The set's one screen source: it keeps the error of its begin for the
/// spent sources behind it.
struct FirstScreen {
    source: Box<dyn CaptureSource>,
    failed: Arc<Mutex<Option<CaptureError>>>,
}

impl CaptureSource for FirstScreen {
    fn begin(&mut self, ctx: SourceContext) -> Result<Started, CaptureError> {
        let begun = self.source.begin(ctx);
        if let Err(error) = &begun {
            *self.failed.lock().unwrap_or_else(PoisonError::into_inner) = Some(error.clone());
        }
        begun
    }

    fn shutdown(&mut self) -> Result<(), CaptureError> {
        self.source.shutdown()
    }

    fn health(&self) -> SourceHealth {
        self.source.health()
    }
}

/// What the set gives for the screen behind its one screen source: a source
/// that starts nothing and takes no gate. It reads stopped, and answers its
/// begin with the first source's error where that source's begin failed.
struct SpentScreen {
    failed: Option<CaptureError>,
}

impl CaptureSource for SpentScreen {
    fn begin(&mut self, _ctx: SourceContext) -> Result<Started, CaptureError> {
        match &self.failed {
            Some(error) => Err(error.clone()),
            None => Ok(Started::Off),
        }
    }

    fn shutdown(&mut self) -> Result<(), CaptureError> {
        Ok(())
    }

    fn health(&self) -> SourceHealth {
        let state = match &self.failed {
            Some(error) => crate::daemon::sources::failure(error.clone()),
            None => SourceState::Stopped(StopCause::Request),
        };
        SourceHealth {
            state,
            counters: SourceCounters::default(),
        }
    }
}

impl SourceFactory for ServiceSet {
    fn make(&mut self, name: SourceName) -> Option<Box<dyn CaptureSource>> {
        match name {
            SourceName::Foreground => Some(Box::new(ForegroundAppSource::new())),
            SourceName::Pointer => Some(Box::new(RawInputSource::pointer(Arc::clone(
                &self.last_app,
            )))),
            SourceName::WindowTitle => Some(Box::new(self.gated_title(WindowTitleSource::new()))),
            SourceName::Screen => Some(self.screen()),
            SourceName::Keyboard if self.typing_on() => Some(Box::new(self.gated_keyboard())),
            SourceName::Keyboard
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
        assert!(set.make(SourceName::Input).is_none());

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
    fn a_title_is_kept_where_the_element_answers_no_password_field_and_dropped_where_it_cannot() {
        use std::sync::Arc;

        use super::super::screen::frame_event;
        use super::super::window_title::{title_event, WindowTitleSource};
        use super::super::{now_tick, DropCounts, Floor, FrontApp, OsTick};
        use crate::floor::exclude::{FloorHandle, NoLineage, PrivateWindow};
        use crate::floor::payload::TitleWhy;
        use crate::floor::{Switch, Switches};
        use crate::secure_input::windows::{
            content_of, state_of, ClearBy, ContentReading, FocusRead, Kind, UnknownBy,
        };
        use crate::secure_input::{SecureInputProbe, SecureInputReading};
        use ContentReading::{Clear, NoAnswer, Password, Unknown};

        let answered = |is_password, protected, kind| FocusRead::Answered {
            is_password,
            protected,
            kind,
        };
        let no_answer = |protected| FocusRead::NoAnswer { protected };
        let rows = [
            (
                "native-edit",
                answered(false, false, Kind::NativeEdit),
                false,
                Clear(ClearBy::NativeEdit),
                FieldState::Clear,
            ),
            (
                "chrome",
                answered(false, false, Kind::Chrome),
                false,
                Clear(ClearBy::Chrome),
                FieldState::Clear,
            ),
            (
                "edge",
                answered(false, false, Kind::Edge),
                false,
                Clear(ClearBy::Edge),
                FieldState::Clear,
            ),
            (
                "other-kind",
                answered(false, false, Kind::Other),
                false,
                Clear(ClearBy::OtherKind),
                FieldState::Unknown,
            ),
            (
                "firefox",
                answered(false, false, Kind::Firefox),
                false,
                Unknown(UnknownBy::Firefox),
                FieldState::Unknown,
            ),
            (
                "gecko",
                answered(false, false, Kind::Other),
                true,
                Unknown(UnknownBy::Firefox),
                FieldState::Unknown,
            ),
            (
                "no-answer",
                no_answer(false),
                false,
                NoAnswer,
                FieldState::Unknown,
            ),
            (
                "no-answer-in-firefox",
                no_answer(false),
                true,
                Unknown(UnknownBy::Firefox),
                FieldState::Unknown,
            ),
            (
                "unreadable",
                FocusRead::Unreadable,
                false,
                Unknown(UnknownBy::Unreadable),
                FieldState::Unknown,
            ),
            (
                "password",
                answered(true, false, Kind::Other),
                false,
                Password,
                FieldState::Password,
            ),
            (
                "protected",
                answered(false, true, Kind::Chrome),
                false,
                Password,
                FieldState::Password,
            ),
            (
                "protected-no-answer",
                no_answer(true),
                false,
                Password,
                FieldState::Password,
            ),
        ];
        let wrong: Vec<&str> = rows
            .iter()
            .filter(|(_, read, firefox, content, key)| {
                state_of(*read, false) != *key || content_of(*read, *firefox, *key) != *content
            })
            .map(|(row, ..)| *row)
            .collect();
        assert!(wrong.is_empty(), "rows with another reading: {wrong:?}");
        assert_eq!(
            content_of(
                answered(false, false, Kind::NativeEdit),
                false,
                FieldState::Password
            ),
            Password,
            "a read of the key's kept password element"
        );

        let set = ServiceSet::new();
        let title = set.gated_title(WindowTitleSource::new());
        let gate = Arc::clone(title.source().password_gate().expect("the set's gate"));
        assert!(Arc::ptr_eq(&gate, &set.password_gate()));
        let drops = Arc::new(DropCounts::default());
        let mut screen_on = Switches::default();
        screen_on.set(Switch::ScreenActivity, true);
        let floor = Floor::new(
            FloorHandle::new(None, Arc::new(NoLineage), screen_on),
            Arc::clone(&drops),
        );
        let front = FrontApp {
            pid: 9,
            path: Some("C:\\Apps\\mail.exe".to_string()),
        };
        let kept_now = || {
            let title = title_event(
                &floor,
                Some(&gate),
                (&front, PrivateWindow::Unknown),
                ("Inbox".to_string(), TitleWhy::TitleChange),
                OsTick::new(now_tick()),
            )
            .is_some();
            let frame = frame_event(&floor, Some(&gate), &front).is_some();
            (
                gate.content_reading(),
                title,
                frame,
                gate.content_probe().read(),
            )
        };
        let at = |read: FocusRead, firefox: bool| {
            gate.focus_read_at(now_tick(), read, firefox);
            kept_now()
        };
        let dropped = |content| (content, false, false, SecureInputReading::Unknown);
        let before_the_first_read = kept_now();
        gate.switch_desktop_at(now_tick());
        let after_a_desktop_switch = kept_now();
        let at_password = at(answered(true, false, Kind::NativeEdit), false);
        let at_clear = at(answered(false, false, Kind::Other), false);
        let at_no_answer = at(no_answer(false), false);
        let at_no_answer_in_firefox = at(no_answer(false), true);
        let at_unreadable = at(FocusRead::Unreadable, false);
        gate.focus_read_at(now_tick(), no_answer(false), false);
        gate.switch_desktop_at(now_tick());
        let after_no_answer_and_a_desktop_switch = kept_now();
        assert_eq!(
            before_the_first_read,
            dropped(Unknown(UnknownBy::BeforeFirstRead)),
            "before the first read"
        );
        assert_eq!(
            after_a_desktop_switch,
            dropped(Unknown(UnknownBy::DesktopSwitch)),
            "after a desktop switch"
        );
        assert_eq!(
            at_password,
            (Password, false, false, SecureInputReading::Active),
            "at Password"
        );
        assert_eq!(
            at_clear,
            (
                Clear(ClearBy::OtherKind),
                true,
                true,
                SecureInputReading::Inactive
            ),
            "at Clear"
        );
        assert_eq!(
            at_no_answer,
            (NoAnswer, true, false, SecureInputReading::NoAnswer),
            "at no answer: the title is kept, the frame is not"
        );
        assert_eq!(
            at_no_answer_in_firefox,
            dropped(Unknown(UnknownBy::Firefox)),
            "at no answer in Firefox"
        );
        assert_eq!(
            at_unreadable,
            dropped(Unknown(UnknownBy::Unreadable)),
            "where no element can be read"
        );
        assert_eq!(
            after_no_answer_and_a_desktop_switch,
            dropped(Unknown(UnknownBy::DesktopSwitch)),
            "a desktop switch after a read with no answer"
        );
        assert_eq!(drops.read().secure_input, 13);
    }

    /// The set makes a keyboard source and a service run starts it while the
    /// build's one constant TYPING_ON_WINDOWS is true; a set and a start of a
    /// build with it false make and start none, and the first-run text of such
    /// a build says that it does not record what you type, as before the switch.
    #[test]
    fn the_windows_service_set_makes_the_keyboard_source_by_the_one_constant_and_none_with_it_false(
    ) {
        use halvo_control::texts::{first_run_with, TYPING_NOT_RECORDED, TYPING_ON_WINDOWS};

        use crate::daemon::sources::starting_with_typing;

        let mut switches = Switches::default();
        for switch in Switch::ALL {
            switches.set(switch, true);
        }
        for (mut set, typing) in [
            (ServiceSet::new(), TYPING_ON_WINDOWS),
            (ServiceSet::with_typing_off(), false),
        ] {
            let mut made_by_the_set = vec![SourceName::Foreground, SourceName::Screen];
            if typing {
                made_by_the_set.push(SourceName::Keyboard);
            }
            made_by_the_set.extend([SourceName::Pointer, SourceName::WindowTitle]);
            let made: Vec<SourceName> = SourceName::WINDOWS
                .into_iter()
                .filter(|name| {
                    let made = set.make(*name);
                    if let Some(source) = &made {
                        assert_eq!(
                            source.health().state,
                            SourceState::NotReported,
                            "{name:?} began"
                        );
                    }
                    made.is_some()
                })
                .collect();
            assert_eq!(made, made_by_the_set, "typing {typing}");
            let wanted =
                starting_with_typing(Platform::Windows, RunKind::Service, switches, typing);
            assert_eq!(
                wanted.contains(&SourceName::Keyboard),
                typing,
                "typing {typing}: {wanted:?}"
            );
            let started: Vec<SourceName> = wanted
                .into_iter()
                .filter(|name| set.make(*name).is_some())
                .collect();
            assert_eq!(started, made_by_the_set, "typing {typing}");
        }
        assert_eq!(
            starting(Platform::Windows, RunKind::Service, switches),
            starting_with_typing(
                Platform::Windows,
                RunKind::Service,
                switches,
                TYPING_ON_WINDOWS
            )
        );
        let off = first_run_with(Platform::Windows, false);
        assert!(
            off.recorded
                .iter()
                .any(|row| row.text == TYPING_NOT_RECORDED),
            "{:?}",
            off.recorded
        );
        assert_eq!(off.typed_ahead, None);
    }

    /// A stop of typing first, while titles and screen activity read the
    /// set's gate, leaves the detector to them, and a restart of typing under
    /// them changes nothing of it: the keyboard source is one more holder of
    /// the set's one gate.
    #[test]
    fn a_stop_of_typing_first_leaves_the_detector_to_titles_and_screen() {
        let _serial = crate::windows::tests::serial();
        let (sink, _writer) = crate::sink::channel(64);
        let ctx = SourceContext::new(sink);
        let mut set = ServiceSet::new();
        let gate = set.password_gate();
        let mut keyboard = set
            .make(SourceName::Keyboard)
            .expect("the set's keyboard source");
        let mut titles = set.gated(StandIn);
        let mut screen = set.gated(StandIn);
        let begun = [
            keyboard.begin(ctx.clone()),
            titles.begin(ctx.clone()),
            screen.begin(ctx.clone()),
        ];
        let all_begun = gate.is_attached();
        let typing_stopped = keyboard.shutdown();
        let after_typing = gate.is_attached();
        drop(keyboard);
        let mut again = set.make(SourceName::Keyboard).expect("a restart's make");
        let restarted = again.begin(ctx.clone());
        let after_the_restart = gate.is_attached();
        let stopped_again = again.shutdown();
        let after_its_stop = gate.is_attached();
        let titles_stopped = titles.shutdown();
        let after_titles = gate.is_attached();
        let screen_stopped = screen.shutdown();
        let after_the_last = gate.is_attached();
        assert_eq!(
            begun,
            [
                Ok(Started::Running),
                Ok(Started::Running),
                Ok(Started::Running)
            ]
        );
        assert_eq!(
            (typing_stopped, restarted, stopped_again),
            (Ok(()), Ok(Started::Running), Ok(()))
        );
        assert_eq!((titles_stopped, screen_stopped), (Ok(()), Ok(())));
        assert_eq!(
            [
                all_begun,
                after_typing,
                after_the_restart,
                after_its_stop,
                after_titles,
                after_the_last
            ],
            [true, true, true, true, true, false],
            "the detector attached: all begun, typing stopped first, typing restarted, \
             typing stopped again, titles stopped, screen stopped last"
        );
    }

    /// A stop of titles and of screen activity first, while typing runs,
    /// leaves the detector to typing until its own stop.
    #[test]
    fn a_stop_of_titles_and_screen_first_leaves_the_detector_to_typing() {
        let _serial = crate::windows::tests::serial();
        let (sink, _writer) = crate::sink::channel(64);
        let ctx = SourceContext::new(sink);
        let mut set = ServiceSet::new();
        let gate = set.password_gate();
        let mut screen = set.gated(StandIn);
        let mut keyboard = set
            .make(SourceName::Keyboard)
            .expect("the set's keyboard source");
        let mut titles = set.gated(StandIn);
        let begun = [
            screen.begin(ctx.clone()),
            keyboard.begin(ctx.clone()),
            titles.begin(ctx.clone()),
        ];
        let all_begun = gate.is_attached();
        let titles_stopped = titles.shutdown();
        let after_titles = gate.is_attached();
        let screen_stopped = screen.shutdown();
        let after_screen = gate.is_attached();
        let typing_stopped = keyboard.shutdown();
        let after_typing = gate.is_attached();
        assert_eq!(
            begun,
            [
                Ok(Started::Running),
                Ok(Started::Running),
                Ok(Started::Running)
            ]
        );
        assert_eq!(
            (titles_stopped, screen_stopped, typing_stopped),
            (Ok(()), Ok(()), Ok(()))
        );
        assert_eq!(
            [all_begun, after_titles, after_screen, after_typing],
            [true, true, true, false],
            "the detector attached: all begun, titles stopped, screen stopped, typing stopped last"
        );
    }

    /// A set with typing makes ONE detector: its keyboard source, begun alone,
    /// holds the set's own gate, which titles and screen activity read.
    #[test]
    fn a_set_with_typing_makes_one_detector_on_its_one_gate() {
        use std::sync::Arc;

        use super::super::window_title::WindowTitleSource;

        let _serial = crate::windows::tests::serial();
        let (sink, _writer) = crate::sink::channel(64);
        let ctx = SourceContext::new(sink);
        let mut set = ServiceSet::new();
        let gate = set.password_gate();
        let mut keyboard = set
            .make(SourceName::Keyboard)
            .expect("the set's keyboard source");
        let made = gate.is_attached();
        let begun = keyboard.begin(ctx);
        let typing_alone = gate.is_attached();
        let title = set.gated_title(WindowTitleSource::new());
        let titles_read = title.source().password_gate().map(Arc::clone);
        let stopped = keyboard.shutdown();
        let after = gate.is_attached();
        assert_eq!(
            (made, begun, typing_alone, stopped, after),
            (false, Ok(Started::Running), true, Ok(()), false),
            "the set's gate: made, typing begun alone, typing stopped"
        );
        assert!(titles_read.is_some_and(|read| Arc::ptr_eq(&read, &gate)));
    }

    /// What the set's figures say of typing is what its keyboard source
    /// counted in the run, and no reading of the gate where no keyboard source
    /// holds it.
    #[test]
    fn the_figures_of_the_set_are_what_its_keyboard_source_counted_and_nothing_where_none_runs() {
        use std::sync::atomic::Ordering;

        use crate::daemon::supervisor::SystemSources;
        use crate::source::SourceCounters;

        let _serial = crate::windows::tests::serial();
        let (sink, _writer) = crate::sink::channel(64);
        let ctx = SourceContext::new(sink);
        let mut set = ServiceSet::new();
        let before = set.figures();
        let mut keyboard = set
            .make(SourceName::Keyboard)
            .expect("the set's keyboard source");
        let begun = keyboard.begin(ctx);
        set.counts.withheld.store(true, Ordering::Relaxed);
        set.counts.late.store(true, Ordering::Relaxed);
        set.counts.max_lag_ms.store(300, Ordering::Relaxed);
        let own = keyboard.health().counters;
        let running = set.figures();
        let stopped = keyboard.shutdown();
        let after = set.figures();
        assert_eq!((begun, stopped), (Ok(Started::Running), Ok(())));
        assert_eq!(
            before,
            SourceCounters::default(),
            "before any keyboard source"
        );
        assert_eq!(
            (own.keys_withheld, own.late_events, own.max_lag_ms),
            (true, 1, 300),
            "the keyboard source counts in the set's counts"
        );
        assert!(
            own.typing_gate.is_some(),
            "the keyboard source reads its gate"
        );
        assert_eq!(
            (
                running.keys_withheld,
                running.late_events,
                running.max_lag_ms,
                running.typing_gate.is_some()
            ),
            (true, 1, 300, true),
            "the set's figures while its keyboard source runs"
        );
        assert_eq!(
            (
                after.keys_withheld,
                after.late_events,
                after.max_lag_ms,
                after.typing_gate
            ),
            (true, 1, 300, None),
            "the set's figures behind its keyboard source's stop"
        );
    }

    /// Begins the screen source with the switch off in its context, so that
    /// it answers off before any call of the system.
    #[test]
    fn the_windows_service_set_makes_the_screen_once_through_the_gate() {
        use std::sync::atomic::Ordering;

        use crate::source::StopCause;

        let _serial = crate::windows::tests::serial();
        let asked = |on| {
            let mut switches = Switches::default();
            switches.set(Switch::ScreenActivity, on);
            starting(Platform::Windows, RunKind::Service, switches)
                .into_iter()
                .filter(|name| *name == SourceName::Screen)
                .collect::<Vec<_>>()
        };
        let mut set = ServiceSet::with_gate(PasswordGate::whose_attach_fails());
        let with_the_switch_off: Vec<_> = asked(false)
            .into_iter()
            .filter_map(|name| set.make(name))
            .collect();
        let mut with_it_on: Vec<_> = asked(true)
            .into_iter()
            .filter_map(|name| set.make(name))
            .collect();
        let fresh: Vec<SourceState> = with_it_on
            .iter()
            .map(|source| source.health().state)
            .collect();
        let (sink, _writer) = crate::sink::channel(4);
        let ctx = SourceContext::new(sink);
        let begun: Vec<_> = with_it_on
            .iter_mut()
            .map(|source| source.begin(ctx.clone()))
            .collect();
        let gate_lines = set.hold.lines_said.load(Ordering::Relaxed);
        let stopped: Vec<_> = with_it_on
            .iter_mut()
            .map(|source| source.shutdown())
            .collect();
        drop(with_it_on);
        let behind_the_stop: Vec<_> = (0..2)
            .map(|_| {
                let mut source = set.make(SourceName::Screen).expect("a make of the screen");
                let before = source.health().state;
                let begun = source.begin(ctx.clone());
                let after = source.health().state;
                (before, begun, after, source.shutdown())
            })
            .collect();
        assert!(with_the_switch_off.is_empty(), "made with the switch off");
        assert_eq!(fresh, [SourceState::NotReported], "made with the switch on");
        assert_eq!(begun, [Ok(Started::Off)]);
        assert_eq!(
            gate_lines, 1,
            "the screen source began without the set's gate"
        );
        assert_eq!(stopped, [Ok(())]);
        let spent = (
            SourceState::Stopped(StopCause::Request),
            Ok(Started::Off),
            SourceState::Stopped(StopCause::Request),
            Ok(()),
        );
        assert_eq!(behind_the_stop, [spent.clone(), spent]);
    }
}
