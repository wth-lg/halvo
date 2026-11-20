#![deny(clippy::undocumented_unsafe_blocks)]

use crate::macos::secure_input_byte::secure_input_on;

mod carbon {
    use std::sync::{Mutex, MutexGuard, PoisonError};

    #[link(name = "Carbon", kind = "framework")]
    extern "C" {
        fn IsSecureEventInputEnabled() -> u8;
        fn EnableSecureEventInput() -> i32;
        fn DisableSecureEventInput() -> i32;
    }

    static CARBON_CALLS: Mutex<()> = Mutex::new(());

    /// The lock of the three calls, held. Only `one_carbon_call_at_a_time`
    /// makes one, and each call of the three takes it.
    pub(super) struct OneCall<'a> {
        _held: MutexGuard<'a, ()>,
        _not_send_or_sync: std::marker::PhantomData<*const ()>,
    }

    const _: fn() = || {
        trait AmbiguousIfSendOrSync<Marker> {
            fn check() {}
        }
        impl<T: ?Sized> AmbiguousIfSendOrSync<()> for T {}
        struct IsSend;
        impl<T: ?Sized + Send> AmbiguousIfSendOrSync<IsSend> for T {}
        struct IsSync;
        impl<T: ?Sized + Sync> AmbiguousIfSendOrSync<IsSync> for T {}
        let _ = <OneCall<'static> as AmbiguousIfSendOrSync<_>>::check;
    };

    #[cfg(test)]
    thread_local! {
        static SYSTEM_READS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    }

    #[cfg(all(test, debug_assertions))]
    pub(super) fn system_reads_on_this_thread() -> u64 {
        SYSTEM_READS.with(std::cell::Cell::get)
    }

    pub(super) fn one_carbon_call_at_a_time<T>(call: impl FnOnce(&OneCall<'_>) -> T) -> T {
        let one = OneCall {
            _held: CARBON_CALLS.lock().unwrap_or_else(PoisonError::into_inner),
            _not_send_or_sync: std::marker::PhantomData,
        };
        call(&one)
    }

    pub(super) fn is_secure_event_input_enabled(_one: &OneCall<'_>) -> u8 {
        #[cfg(test)]
        SYSTEM_READS.with(|reads| reads.set(reads.get() + 1));
        // SAFETY: a parameterless C predicate with no side effects.
        // CarbonEventsCore.h marks it "Not thread safe" (line 3056 of the SDK on
        // this Mac): no other of the three calls runs at the same time, because
        // each needs a OneCall, which exists only while its holder holds the lock
        // and, being neither Send nor Sync, cannot reach another thread.
        unsafe { IsSecureEventInputEnabled() }
    }

    pub(super) fn enable_secure_event_input(_one: &OneCall<'_>) -> i32 {
        // SAFETY: parameterless Carbon call that returns an OSStatus, as
        // CarbonEventsCore.h declares it (line 3004). The header marks it "Not
        // thread safe" (line 2996 of the SDK on this Mac): no other of the three
        // calls runs at the same time, because each needs a OneCall, which exists
        // only while its holder holds the lock and, being neither Send nor Sync,
        // cannot reach another thread.
        unsafe { EnableSecureEventInput() }
    }

    pub(super) fn disable_secure_event_input(_one: &OneCall<'_>) -> i32 {
        // SAFETY: parameterless Carbon call that returns an OSStatus, as
        // CarbonEventsCore.h declares it (line 3040). The header marks it "Not
        // thread safe" (line 3032 of the SDK on this Mac): no other of the three
        // calls runs at the same time, because each needs a OneCall, which exists
        // only while its holder holds the lock and, being neither Send nor Sync,
        // cannot reach another thread.
        unsafe { DisableSecureEventInput() }
    }
}

#[cfg(all(test, debug_assertions))]
pub(crate) fn system_reads_on_this_thread() -> u64 {
    carbon::system_reads_on_this_thread()
}

pub fn secure_input_active() -> bool {
    secure_input_on(carbon::one_carbon_call_at_a_time(
        carbon::is_secure_event_input_enabled,
    ))
}

#[path = "secure_input_macos_hold.rs"]
pub mod hold;

/// One reading of the secure-input state: off, on, or unknown when the session
/// is not known to be at the console.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reading {
    Off,
    On,
    Unknown,
}

/// The login session of this process, as its session dictionary gives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Session {
    AtConsole,
    NotAtConsole,
    NoDictionary,
}

/// The session from what its dictionary holds: `None` when there is no
/// dictionary, else whether its on-console key reads true.
pub fn session_of(on_console: Option<bool>) -> Session {
    match on_console {
        None => Session::NoDictionary,
        Some(true) => Session::AtConsole,
        Some(false) => Session::NotAtConsole,
    }
}

/// Reads the session dictionary of this process (CGSession.h).
pub fn read_session() -> Session {
    session_of(session::on_console())
}

/// The reading under `session`: unknown unless the session is at the console.
pub fn reading_under(session: Session, secure: impl FnOnce() -> bool) -> Reading {
    match session {
        Session::AtConsole if secure() => Reading::On,
        Session::AtConsole => Reading::Off,
        Session::NotAtConsole | Session::NoDictionary => Reading::Unknown,
    }
}

pub fn secure_input_reading(session: Session) -> Reading {
    reading_under(session, secure_input_active)
}

/// The queued notices of a focus change into a secure field of the program in
/// front (rulings MA-12 E and MA-20 (7)(c)): a thread of its own follows the
/// program in front with an Accessibility observer and, for every notice whose
/// element is a secure text field or cannot be read within its deadline, moves
/// the notice mark to the uptime of that notice. A page sets no Accessibility
/// attribute here, so a browser's page gives no notice (ruling PR-30 PR15).
pub mod notice {
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
    use std::sync::Arc;
    use std::thread::JoinHandle;
    use std::time::{Duration, Instant};

    use core_foundation::base::{CFType, CFTypeRef, TCFType};
    use core_foundation::date::CFDate;
    use core_foundation::runloop::{
        kCFRunLoopDefaultMode, CFRunLoop, CFRunLoopSource, CFRunLoopSourceInvalidate,
        CFRunLoopSourceRef, CFRunLoopTimer, CFRunLoopTimerContext, CFRunLoopTimerInvalidate,
        CFRunLoopTimerRef,
    };
    use core_foundation::string::{CFString, CFStringRef};

    use super::hold::Uptime;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXUIElementCreateApplication(pid: libc::pid_t) -> CFTypeRef;
        fn AXUIElementCopyAttributeValue(
            element: CFTypeRef,
            attribute: CFStringRef,
            value: *mut CFTypeRef,
        ) -> i32;
        fn AXUIElementSetMessagingTimeout(element: CFTypeRef, seconds: f32) -> i32;
        fn AXObserverCreate(
            pid: libc::pid_t,
            callback: ObserverCallback,
            observer: *mut CFTypeRef,
        ) -> i32;
        fn AXObserverAddNotification(
            observer: CFTypeRef,
            element: CFTypeRef,
            notification: CFStringRef,
            refcon: *mut c_void,
        ) -> i32;
        fn AXObserverGetRunLoopSource(observer: CFTypeRef) -> CFRunLoopSourceRef;
    }

    type ObserverCallback = unsafe extern "C" fn(CFTypeRef, CFTypeRef, CFStringRef, *mut c_void);

    const AX_ERROR_SUCCESS: i32 = 0;
    const AX_ERROR_FAILURE: i32 = -25200;
    const AX_ERROR_CANNOT_COMPLETE: i32 = -25204;
    const AX_ERROR_ATTRIBUTE_UNSUPPORTED: i32 = -25205;
    const AX_ERROR_API_DISABLED: i32 = -25211;
    const AX_ERROR_NO_VALUE: i32 = -25212;

    /// The step of the thread: it reads which program is in front, and tries a
    /// registration, at most once in each step, and waits on a timer of its run
    /// loop between two steps.
    pub const NOTICE_FRONT_STEP: Duration = Duration::from_millis(50);

    /// The deadline of one question to the program in front: a read of an
    /// element's subrole, of its focused element, or the registration.
    pub const NOTICE_READ_DEADLINE: Duration = Duration::from_millis(250);

    /// How long a program in front whose registration was refused for lack of
    /// the Accessibility grant waits before it is tried again, unless another
    /// program comes in front first.
    pub const NO_GRANT_RETRY: Duration = Duration::from_secs(1);

    /// The subrole of a secure text field.
    pub const SECURE_SUBROLE: &str = "AXSecureTextField";

    /// What a notice, or the read at the start of a watch, says of the element
    /// that has the focus.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Focus {
        /// A secure text field.
        Secure,
        /// An element that answered and is no secure text field.
        Other,
        /// An element that did not answer within the deadline, or answered
        /// with an error: a reason to drop, as a secure field is.
        Doubt,
    }

    impl Focus {
        /// Whether this focus moves the notice mark.
        pub fn marks(self) -> bool {
            self != Self::Other
        }
    }

    /// The focus from the read of an element's subrole: its text, or the
    /// Accessibility error of the read. No value and an unsupported attribute
    /// are an element that is no secure field; every other error is a doubt.
    pub fn focus_of(subrole: Result<Option<&str>, i32>) -> Focus {
        match subrole {
            Ok(Some(SECURE_SUBROLE)) => Focus::Secure,
            Ok(_) => Focus::Other,
            Err(AX_ERROR_NO_VALUE | AX_ERROR_ATTRIBUTE_UNSUPPORTED) => Focus::Other,
            Err(_) => Focus::Doubt,
        }
    }

    /// Where a notice came from.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Seen {
        /// A notice of a focus change.
        Notice,
        /// The read of the focused element when the watch of a program began.
        Start,
        /// A registration that the program in front did not answer in time.
        Unanswered,
    }

    /// What moved the notice mark last: a notice of a focus change into a
    /// secure field, a doubt (a notice or a registration that did not answer),
    /// or the read at the start of a watch.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum MarkKind {
        Notice,
        Doubt,
        Start,
    }

    impl MarkKind {
        /// The kind of a mark by where it came from and what it said.
        pub fn of(seen: Seen, focus: Focus) -> Self {
            match (seen, focus) {
                (Seen::Start, _) => Self::Start,
                (Seen::Unanswered, _) | (Seen::Notice, Focus::Doubt) => Self::Doubt,
                (Seen::Notice, _) => Self::Notice,
            }
        }

        fn code(self) -> u8 {
            match self {
                Self::Notice => 0,
                Self::Doubt => 1,
                Self::Start => 2,
            }
        }

        fn from_code(code: u8) -> Self {
            match code {
                1 => Self::Doubt,
                2 => Self::Start,
                _ => Self::Notice,
            }
        }
    }

    #[derive(Default)]
    struct MarkState {
        at: AtomicU64,
        kind: AtomicU8,
        doubt: AtomicBool,
    }

    /// The uptime of the newest notice that marks, with its kind, and whether
    /// a doubt stands over the program in front. The uptime only moves forward,
    /// and every end of a doubt moves it to the time at which the doubt ended.
    #[derive(Clone, Default)]
    pub struct NoticeMark(Arc<MarkState>);

    impl NoticeMark {
        pub fn new() -> Self {
            Self::default()
        }

        /// Moves the mark to `at` unless it already stands later. The kind is
        /// that of the call that moved the mark last: a mark older than the
        /// newest leaves both.
        pub fn mark(&self, at: Uptime, kind: MarkKind) {
            if self.0.at.fetch_max(at.nanos(), Ordering::SeqCst) <= at.nanos() {
                self.0.kind.store(kind.code(), Ordering::SeqCst);
            }
        }

        /// The newest mark, none before the first.
        pub fn get(&self) -> Option<Uptime> {
            match self.0.at.load(Ordering::SeqCst) {
                0 => None,
                at => Some(Uptime::from_nanos(at)),
            }
        }

        /// The kind of the newest mark.
        pub fn kind(&self) -> MarkKind {
            MarkKind::from_code(self.0.kind.load(Ordering::SeqCst))
        }

        /// Whether a doubt stands: the program in front refused its
        /// registration by not answering, and no registration of it has
        /// succeeded since, nor has another program come in front. Nothing in
        /// front ends no doubt. A doubt ends only by such a registration, by
        /// another program in front, or by the end of the thread, and each end
        /// first moves the mark to the time at which the doubt ended. So a key
        /// that arrives while a doubt stands is dropped at its own reading,
        /// and a key whose OS time is not after the end is dropped at the
        /// first reading after it: neither is ever released.
        pub fn doubt(&self) -> bool {
            self.0.doubt.load(Ordering::SeqCst)
        }

        pub(crate) fn set_doubt(&self, standing: bool) {
            self.0.doubt.store(standing, Ordering::SeqCst);
        }

        /// Ends a doubt at `at`: the mark moves to `at` before the flag is
        /// cleared, so a reading that finds no doubt finds the mark.
        pub(crate) fn end_doubt(&self, at: Uptime) {
            self.mark(at, MarkKind::Doubt);
            self.set_doubt(false);
        }
    }

    /// What the thread hands to its caller for each notice: where it came
    /// from and its focus; the caller may log it.
    pub type OnNotice = Box<dyn Fn(Seen, Focus) + Send + Sync>;

    /// An observer of a program's focus changes: its run loop source, which
    /// the thread adds to its run loop, and the observer that it keeps.
    pub(crate) struct Observed {
        pub(crate) source: Option<CFRunLoopSource>,
        pub(crate) observer: Option<CFType>,
    }

    /// The Accessibility calls of the thread. The product uses `SYSTEM`; a
    /// build with debug assertions and the feature stall-seam may hand in
    /// others.
    #[derive(Clone, Copy)]
    pub(crate) struct AxCalls {
        pub(crate) app: fn(libc::pid_t) -> Option<CFType>,
        pub(crate) set_deadline: fn(&CFType, Duration),
        pub(crate) observe: fn(libc::pid_t, &CFType, *mut c_void) -> Result<Observed, i32>,
        pub(crate) focused: fn(&CFType) -> Result<CFType, i32>,
        pub(crate) subrole: fn(&CFType) -> Result<Option<String>, i32>,
    }

    pub(crate) const SYSTEM: AxCalls = AxCalls {
        app: system_app,
        set_deadline: system_set_deadline,
        observe: system_observe,
        focused: system_focused,
        subrole: system_subrole,
    };

    pub(crate) struct Context {
        pub(crate) mark: NoticeMark,
        pub(crate) uptime: fn() -> Uptime,
        pub(crate) on_notice: OnNotice,
        pub(crate) calls: AxCalls,
    }

    impl Context {
        pub(crate) fn seen(&self, seen: Seen, focus: Focus) {
            (self.on_notice)(seen, focus);
            if focus.marks() {
                self.mark.mark((self.uptime)(), MarkKind::of(seen, focus));
            }
        }

        fn focus_of_element(&self, element: &CFType) -> Focus {
            (self.calls.set_deadline)(element, NOTICE_READ_DEADLINE);
            match (self.calls.subrole)(element) {
                Ok(subrole) => focus_of(Ok(subrole.as_deref())),
                Err(code) => focus_of(Err(code)),
            }
        }
    }

    fn system_app(pid: libc::pid_t) -> Option<CFType> {
        // SAFETY: AXUIElementCreateApplication takes a pid by value and returns
        // a +1 reference or null.
        let app = unsafe { AXUIElementCreateApplication(pid) };
        if app.is_null() {
            return None;
        }
        // SAFETY: a +1 reference that this wrapper now owns and releases.
        Some(unsafe { CFType::wrap_under_create_rule(app) })
    }

    fn system_set_deadline(element: &CFType, deadline: Duration) {
        // SAFETY: `element` is a live AXUIElement for the call; the call only
        // sets the deadline of the questions that this process asks it.
        unsafe { AXUIElementSetMessagingTimeout(element.as_CFTypeRef(), deadline.as_secs_f32()) };
    }

    fn copy_attribute(element: &CFType, name: &'static str) -> Result<Option<CFType>, i32> {
        let attribute = CFString::from_static_string(name);
        let mut value: CFTypeRef = std::ptr::null();
        // SAFETY: `element` is a live AXUIElement, `attribute` a live CFString
        // and `value` a writable pointer that the call sets to a +1 reference
        // on success.
        let code = unsafe {
            AXUIElementCopyAttributeValue(
                element.as_CFTypeRef(),
                attribute.as_concrete_TypeRef(),
                &mut value,
            )
        };
        if code != AX_ERROR_SUCCESS {
            return Err(code);
        }
        if value.is_null() {
            return Ok(None);
        }
        // SAFETY: the call succeeded and gave a +1 reference that this wrapper
        // now owns and releases.
        Ok(Some(unsafe { CFType::wrap_under_create_rule(value) }))
    }

    fn system_subrole(element: &CFType) -> Result<Option<String>, i32> {
        Ok(copy_attribute(element, "AXSubrole")?
            .and_then(|value| value.downcast::<CFString>().map(|text| text.to_string())))
    }

    fn system_focused(app: &CFType) -> Result<CFType, i32> {
        copy_attribute(app, "AXFocusedUIElement")?.ok_or(AX_ERROR_NO_VALUE)
    }

    fn system_observe(
        pid: libc::pid_t,
        app: &CFType,
        refcon: *mut c_void,
    ) -> Result<Observed, i32> {
        let mut observer: CFTypeRef = std::ptr::null();
        // SAFETY: `notice_came` has the signature of an AXObserverCallback and
        // `observer` is a writable pointer that the call sets to a +1 reference
        // on success.
        let code = unsafe { AXObserverCreate(pid, notice_came, &mut observer) };
        if code != AX_ERROR_SUCCESS {
            return Err(code);
        }
        if observer.is_null() {
            return Err(AX_ERROR_FAILURE);
        }
        // SAFETY: a +1 reference that this wrapper now owns and releases.
        let observer = unsafe { CFType::wrap_under_create_rule(observer) };
        let notification = CFString::from_static_string("AXFocusedUIElementChanged");
        // SAFETY: `observer` and `app` are live; `refcon` is the thread's
        // Context, which outlives the observer: the thread releases the
        // observer before it drops the context.
        let code = unsafe {
            AXObserverAddNotification(
                observer.as_CFTypeRef(),
                app.as_CFTypeRef(),
                notification.as_concrete_TypeRef(),
                refcon,
            )
        };
        if code != AX_ERROR_SUCCESS {
            return Err(code);
        }
        // SAFETY: `observer` is a live AXObserver; the call returns its run
        // loop source under the get rule, which the wrapper retains.
        let source = unsafe {
            CFRunLoopSource::wrap_under_get_rule(AXObserverGetRunLoopSource(
                observer.as_CFTypeRef(),
            ))
        };
        Ok(Observed {
            source: Some(source),
            observer: Some(observer),
        })
    }

    pub(crate) unsafe extern "C" fn notice_came(
        _observer: CFTypeRef,
        element: CFTypeRef,
        _notification: CFStringRef,
        refcon: *mut c_void,
    ) {
        if refcon.is_null() || element.is_null() {
            return;
        }
        // SAFETY: `refcon` is the Context that the watch registered, which the
        // thread keeps alive in a box until after the observer is released.
        let context = unsafe { &*refcon.cast::<Context>() };
        // SAFETY: `element` is the live AXUIElement that the observer hands to
        // its callback; retaining it under the get rule keeps it for the read.
        let element = unsafe { CFType::wrap_under_get_rule(element) };
        let focus = context.focus_of_element(&element);
        context.seen(Seen::Notice, focus);
    }

    struct Watch {
        pid: libc::pid_t,
        source: Option<CFRunLoopSource>,
        _observer: Option<CFType>,
    }

    fn watch(pid: libc::pid_t, context: &Context) -> Result<Watch, i32> {
        let calls = context.calls;
        let app = (calls.app)(pid).ok_or(AX_ERROR_FAILURE)?;
        (calls.set_deadline)(&app, NOTICE_READ_DEADLINE);
        let refcon = std::ptr::from_ref(context).cast_mut().cast();
        let observed = match (calls.observe)(pid, &app, refcon) {
            Ok(observed) => observed,
            Err(code) => {
                if code == AX_ERROR_CANNOT_COMPLETE {
                    context.seen(Seen::Unanswered, Focus::Doubt);
                }
                return Err(code);
            }
        };
        if let Some(source) = &observed.source {
            // SAFETY: kCFRunLoopDefaultMode is a constant CFString that
            // CoreFoundation exports and never writes.
            CFRunLoop::get_current().add_source(source, unsafe { kCFRunLoopDefaultMode });
        }
        let focus = match (calls.focused)(&app) {
            Ok(element) => context.focus_of_element(&element),
            Err(code) => focus_of(Err(code)),
        };
        context.seen(Seen::Start, focus);
        Ok(Watch {
            pid,
            source: observed.source,
            _observer: observed.observer,
        })
    }

    fn unwatch(watched: &mut Option<Watch>) {
        if let Some(watch) = watched.take() {
            if let Some(source) = &watch.source {
                // SAFETY: kCFRunLoopDefaultMode is a constant CFString that
                // CoreFoundation exports and never writes.
                CFRunLoop::get_current().remove_source(source, unsafe { kCFRunLoopDefaultMode });
                // SAFETY: `source` is a live run loop source that this wrapper
                // retains; invalidating it stops its callbacks.
                unsafe { CFRunLoopSourceInvalidate(source.as_concrete_TypeRef()) };
            }
        }
    }

    extern "C" fn step_fired(_timer: CFRunLoopTimerRef, _info: *mut c_void) {}

    fn step_timer() -> CFRunLoopTimer {
        let mut context = CFRunLoopTimerContext {
            version: 0,
            info: std::ptr::null_mut(),
            retain: None,
            release: None,
            copyDescription: None,
        };
        let step = NOTICE_FRONT_STEP.as_secs_f64();
        let timer = CFRunLoopTimer::new(
            CFDate::now().abs_time() + step,
            step,
            0,
            0,
            step_fired,
            &mut context,
        );
        // SAFETY: kCFRunLoopDefaultMode is a constant CFString that
        // CoreFoundation exports and never writes.
        CFRunLoop::get_current().add_timer(&timer, unsafe { kCFRunLoopDefaultMode });
        timer
    }

    /// What the thread did with the program in front, for a test build.
    #[cfg(test)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub(crate) struct Steps {
        pub(crate) front_reads: u64,
        pub(crate) tries: u64,
    }

    /// Ends a standing doubt when the thread ends, by its stop or by a panic.
    struct DoubtEndsWithTheThread<'a>(&'a Context);

    impl Drop for DoubtEndsWithTheThread<'_> {
        fn drop(&mut self) {
            if self.0.mark.doubt() {
                self.0.mark.end_doubt((self.0.uptime)());
            }
        }
    }

    #[cfg(test)]
    pub(crate) mod wait_seam {
        use std::cell::Cell;
        use std::time::Duration;

        use core_foundation::runloop::CFRunLoopRunResult;
        use core_foundation::string::CFStringRef;

        thread_local! {
            pub(crate) static HANDED: Cell<Option<Duration>> = const { Cell::new(None) };
        }

        pub(crate) struct CFRunLoop;

        impl CFRunLoop {
            pub(crate) fn run_in_mode(
                mode: CFStringRef,
                duration: Duration,
                return_after_source_handled: bool,
            ) -> CFRunLoopRunResult {
                HANDED.with(|handed| handed.set(Some(duration)));
                core_foundation::runloop::CFRunLoop::run_in_mode(
                    mode,
                    duration,
                    return_after_source_handled,
                )
            }
        }
    }

    fn run(
        front: &(dyn Fn() -> Option<libc::pid_t> + Send),
        context: &Context,
        stop: &AtomicBool,
        steps: &StepCounts,
    ) {
        #[cfg(test)]
        use self::wait_seam::CFRunLoop;
        let _doubt_ends = DoubtEndsWithTheThread(context);
        let timer = step_timer();
        let mut watched: Option<Watch> = None;
        let mut doubt_of: Option<libc::pid_t> = None;
        let mut no_grant: Option<(libc::pid_t, Instant)> = None;
        while !stop.load(Ordering::SeqCst) {
            steps.front_reads.fetch_add(1, Ordering::SeqCst);
            let pid = front();
            if doubt_of.is_some() && pid.is_some() && doubt_of != pid {
                doubt_of = None;
                context.mark.end_doubt((context.uptime)());
            }
            if watched.as_ref().map(|watch| watch.pid) != pid {
                unwatch(&mut watched);
                let waits_for_the_grant = no_grant.is_some_and(|(refused, at)| {
                    Some(refused) == pid && at.elapsed() < NO_GRANT_RETRY
                });
                if let (Some(pid), false) = (pid, waits_for_the_grant) {
                    steps.tries.fetch_add(1, Ordering::SeqCst);
                    match watch(pid, context) {
                        Ok(watch) => {
                            watched = Some(watch);
                            no_grant = None;
                            if doubt_of.take().is_some() {
                                context.mark.end_doubt((context.uptime)());
                            }
                        }
                        Err(AX_ERROR_CANNOT_COMPLETE) => {
                            doubt_of = Some(pid);
                            context.mark.set_doubt(true);
                        }
                        Err(AX_ERROR_API_DISABLED) => no_grant = Some((pid, Instant::now())),
                        Err(_) => {}
                    }
                }
            }
            // SAFETY: kCFRunLoopDefaultMode is a constant CFString that
            // CoreFoundation exports and never writes.
            CFRunLoop::run_in_mode(unsafe { kCFRunLoopDefaultMode }, NOTICE_FRONT_STEP, false);
        }
        unwatch(&mut watched);
        // SAFETY: `timer` is a live timer of this thread's run loop; the call
        // removes it from every mode.
        unsafe { CFRunLoopTimerInvalidate(timer.as_concrete_TypeRef()) };
    }

    #[derive(Default)]
    struct StepCounts {
        front_reads: AtomicU64,
        tries: AtomicU64,
    }

    /// The thread that follows the program in front and moves the notice mark.
    pub struct NoticeThread {
        run_loop: CFRunLoop,
        stop: Arc<AtomicBool>,
        #[cfg(test)]
        steps: Arc<StepCounts>,
        thread: Option<JoinHandle<()>>,
    }

    impl NoticeThread {
        /// Starts the thread. `front` names the process in front, `uptime`
        /// reads CLOCK_UPTIME_RAW, the clock of an event's OS time (the source
        /// hands in its own reader), and `on_notice` sees every notice before
        /// the mark moves.
        pub fn spawn(
            front: Box<dyn Fn() -> Option<libc::pid_t> + Send>,
            mark: NoticeMark,
            uptime: fn() -> Uptime,
            on_notice: OnNotice,
        ) -> std::io::Result<Self> {
            Self::spawn_calling(front, mark, uptime, on_notice, SYSTEM)
        }

        /// The thread with Accessibility calls of a test's own, in a build
        /// with debug assertions and the feature stall-seam.
        #[cfg(all(test, debug_assertions, feature = "stall-seam"))]
        pub(crate) fn spawn_with(
            front: Box<dyn Fn() -> Option<libc::pid_t> + Send>,
            mark: NoticeMark,
            uptime: fn() -> Uptime,
            on_notice: OnNotice,
            calls: AxCalls,
        ) -> std::io::Result<Self> {
            Self::spawn_calling(front, mark, uptime, on_notice, calls)
        }

        fn spawn_calling(
            front: Box<dyn Fn() -> Option<libc::pid_t> + Send>,
            mark: NoticeMark,
            uptime: fn() -> Uptime,
            on_notice: OnNotice,
            calls: AxCalls,
        ) -> std::io::Result<Self> {
            let stop = Arc::new(AtomicBool::new(false));
            let steps = Arc::new(StepCounts::default());
            let (loop_tx, loop_rx) = std::sync::mpsc::channel();
            let thread = {
                let stop = stop.clone();
                let steps = steps.clone();
                std::thread::Builder::new()
                    .name("halvo-input-notice".into())
                    .spawn(move || {
                        let context = Box::new(Context {
                            mark,
                            uptime,
                            on_notice,
                            calls,
                        });
                        let _ = loop_tx.send(CFRunLoop::get_current());
                        run(front.as_ref(), &context, &stop, &steps);
                        drop(context);
                    })?
            };
            let run_loop = loop_rx
                .recv()
                .map_err(|_| std::io::Error::other("the notice thread ended at its start"))?;
            Ok(Self {
                run_loop,
                stop,
                #[cfg(test)]
                steps,
                thread: Some(thread),
            })
        }

        #[cfg(test)]
        pub(crate) fn steps(&self) -> Steps {
            Steps {
                front_reads: self.steps.front_reads.load(Ordering::SeqCst),
                tries: self.steps.tries.load(Ordering::SeqCst),
            }
        }

        /// Asks the thread to stop and waits for its end until `deadline`:
        /// false when it did not end in time.
        pub fn stop_within(&mut self, deadline: Duration) -> bool {
            self.stop.store(true, Ordering::SeqCst);
            self.run_loop.stop();
            let called = Instant::now();
            while self
                .thread
                .as_ref()
                .is_some_and(|thread| !thread.is_finished())
            {
                if called.elapsed() >= deadline {
                    return false;
                }
                std::thread::sleep(Duration::from_millis(2));
                self.run_loop.stop();
            }
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
            true
        }
    }

    impl Drop for NoticeThread {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            self.run_loop.stop();
        }
    }
}

mod session {
    use core_foundation::base::{CFType, TCFType};
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
    use core_foundation::string::CFString;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGSessionCopyCurrentDictionary() -> CFDictionaryRef;
    }

    pub(super) fn on_console() -> Option<bool> {
        // SAFETY: a parameterless call that returns a dictionary that the caller
        // owns, or null.
        let dictionary = unsafe { CGSessionCopyCurrentDictionary() };
        if dictionary.is_null() {
            return None;
        }
        // SAFETY: `dictionary` is not null and the copy rule gives its one
        // reference to this wrapper, which releases it.
        let dictionary: CFDictionary<CFString, CFType> =
            unsafe { CFDictionary::wrap_under_create_rule(dictionary) };
        let key = CFString::from_static_string("kCGSSessionOnConsoleKey");
        Some(
            dictionary
                .find(&key)
                .and_then(|value| value.downcast::<CFBoolean>())
                .is_some_and(bool::from),
        )
    }
}

#[must_use = "dropping the hold releases secure input immediately"]
pub struct SecureInputHold {
    #[cfg(test)]
    enable: fn() -> i32,
    release: fn() -> i32,
}

/// The status other than noErr with which `EnableSecureEventInput` refused a
/// hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecureInputRefused(pub i32);

pub fn hold_secure_input() -> Result<SecureInputHold, SecureInputRefused> {
    hold_with(enable_secure_event_input, disable_secure_event_input)
}

fn enable_secure_event_input() -> i32 {
    carbon::one_carbon_call_at_a_time(carbon::enable_secure_event_input)
}

pub(crate) fn hold_with(
    enable: fn() -> i32,
    release: fn() -> i32,
) -> Result<SecureInputHold, SecureInputRefused> {
    crate::macos::secure_input_byte::take_hold(enable)
        .map(|()| SecureInputHold {
            #[cfg(test)]
            enable,
            release,
        })
        .map_err(SecureInputRefused)
}

pub(crate) fn disable_secure_event_input() -> i32 {
    carbon::one_carbon_call_at_a_time(carbon::disable_secure_event_input)
}

impl Drop for SecureInputHold {
    fn drop(&mut self) {
        crate::macos::secure_input_byte::release_hold(self.release);
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::{
        disable_secure_event_input, enable_secure_event_input, hold_secure_input, hold_with,
        SecureInputHold, SecureInputRefused,
    };
    use std::sync::atomic::{AtomicIsize, Ordering};
    use std::sync::{Mutex, MutexGuard, PoisonError};
    use std::time::{Duration, Instant};

    static HOLDS: Mutex<()> = Mutex::new(());

    static OWN_HOLDS: AtomicIsize = AtomicIsize::new(0);

    const HOLDS_OF_ALL_PROCESSES: &str = "/private/tmp/halvo-capture-secure-input-tests.lock";

    fn counted_enable() -> i32 {
        let status = enable_secure_event_input();
        if status == 0 {
            OWN_HOLDS.fetch_add(1, Ordering::SeqCst);
        }
        status
    }

    fn counted_release() -> i32 {
        OWN_HOLDS.fetch_sub(1, Ordering::SeqCst);
        disable_secure_event_input()
    }

    fn own_holds() -> isize {
        OWN_HOLDS.load(Ordering::SeqCst)
    }

    pub(crate) struct OneHold {
        own_before: isize,
        _file: std::fs::File,
        _guard: MutexGuard<'static, ()>,
    }

    impl Drop for OneHold {
        fn drop(&mut self) {
            let left = own_holds() - self.own_before;
            for _ in 0..left {
                counted_release();
            }
            if left != 0 && !std::thread::panicking() {
                panic!(
                    "the test changed the count of this process's secure-input holds by {left} \
                     (the guard gave back the holds that were left: {})",
                    left.max(0)
                );
            }
        }
    }

    pub(crate) fn one_hold_at_a_time() -> OneHold {
        let guard = HOLDS.lock().unwrap_or_else(PoisonError::into_inner);
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(HOLDS_OF_ALL_PROCESSES)
            .expect("the lock file of the hold tests opens");
        // SAFETY: `file` is open, so its descriptor is valid for the call, and
        // flock only takes a lock on it, which the descriptor's close releases.
        let locked = unsafe { libc::flock(std::os::fd::AsRawFd::as_raw_fd(&file), libc::LOCK_EX) };
        assert_eq!(locked, 0, "the lock of the hold tests was not taken");
        OneHold {
            own_before: own_holds(),
            _file: file,
            _guard: guard,
        }
    }

    impl OneHold {
        pub(crate) fn hold_or_skip(&self, test: &str) -> Option<SecureInputHold> {
            self.held_or_skip(
                test,
                "hold_with on the system's calls",
                hold_with(counted_enable, counted_release),
            )
        }

        pub(crate) fn production_hold_or_skip(&self, test: &str) -> Option<SecureInputHold> {
            self.held_or_skip(test, "hold_secure_input", hold_secure_input())
        }

        fn held_or_skip(
            &self,
            test: &str,
            maker: &str,
            held: Result<SecureInputHold, SecureInputRefused>,
        ) -> Option<SecureInputHold> {
            let refused = match held {
                Ok(hold) => return Some(hold),
                Err(refused) => refused,
            };
            let system = enable_secure_event_input();
            if system == 0 {
                disable_secure_event_input();
                panic!(
                    "{test}: {maker} reported the status {} although the system accepts a hold",
                    refused.0
                );
            }
            eprintln!(
                "SKIP {test}: EnableSecureEventInput refused the hold with status {system} \
                 ({maker}: {})",
                refused.0
            );
            None
        }
    }

    #[cfg(debug_assertions)]
    pub(crate) fn reads_on_within(reader: impl Fn() -> bool, limit: Duration) -> bool {
        let started = Instant::now();
        loop {
            if reader() {
                return true;
            }
            if started.elapsed() >= limit {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    pub(crate) fn session_shows_secure_input() -> bool {
        let out = std::process::Command::new("/usr/sbin/ioreg")
            .args(["-l", "-w", "0", "-d", "1"])
            .output()
            .expect("ioreg runs");
        assert!(out.status.success(), "ioreg exited with {}", out.status);
        String::from_utf8_lossy(&out.stdout).contains("\"kCGSSessionSecureInputPID\"")
    }

    pub(crate) fn agreed_reading(reader: impl Fn() -> bool, limit: Duration) -> (bool, bool) {
        let started = Instant::now();
        loop {
            let session = session_shows_secure_input();
            let read = reader();
            let settled = session_shows_secure_input() == session;
            if (settled && read == session) || started.elapsed() >= limit {
                return (read, session);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn no_two_carbon_calls_run_at_the_same_time() {
        let inside = AtomicUsize::new(0);
        let most = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    for _ in 0..5 {
                        super::carbon::one_carbon_call_at_a_time(|_| {
                            let now = inside.fetch_add(1, Ordering::SeqCst) + 1;
                            most.fetch_max(now, Ordering::SeqCst);
                            std::thread::sleep(Duration::from_millis(2));
                            inside.fetch_sub(1, Ordering::SeqCst);
                        });
                    }
                });
            }
        });
        assert_eq!(
            most.load(Ordering::SeqCst),
            1,
            "calls that were inside at the same time"
        );
    }

    #[test]
    fn a_notice_marks_for_a_secure_field_and_a_doubt_and_not_for_another_element() {
        use super::notice::{focus_of, Focus};
        let cases = [
            (Ok(Some("AXSecureTextField")), Focus::Secure),
            (Ok(Some("AXTextField")), Focus::Other),
            (Ok(Some("AXSearchField")), Focus::Other),
            (Ok(None), Focus::Other),
            (Err(-25212), Focus::Other),
            (Err(-25205), Focus::Other),
            (Err(-25204), Focus::Doubt),
            (Err(-25202), Focus::Doubt),
            (Err(-25211), Focus::Doubt),
            (Err(-25200), Focus::Doubt),
        ];
        for (subrole, want) in cases {
            assert_eq!(focus_of(subrole), want, "{subrole:?}");
        }
        assert!(Focus::Secure.marks());
        assert!(Focus::Doubt.marks());
        assert!(!Focus::Other.marks());
    }

    #[test]
    fn the_notice_constants_are_their_first_values() {
        use super::notice::{NOTICE_FRONT_STEP, NOTICE_READ_DEADLINE, NO_GRANT_RETRY};
        use std::time::Duration;
        assert_eq!(NOTICE_FRONT_STEP, Duration::from_millis(50));
        assert_eq!(NOTICE_READ_DEADLINE, Duration::from_millis(250));
        assert_eq!(NO_GRANT_RETRY, Duration::from_secs(1));
    }

    #[test]
    fn an_older_mark_leaves_the_kind_of_the_newest_mark() {
        use super::hold::Uptime;
        use super::notice::{MarkKind, NoticeMark};
        let mark = NoticeMark::new();
        mark.mark(Uptime::from_nanos(20), MarkKind::Start);
        mark.mark(Uptime::from_nanos(10), MarkKind::Notice);
        assert_eq!(
            (mark.get(), mark.kind()),
            (Some(Uptime::from_nanos(20)), MarkKind::Start)
        );
        mark.mark(Uptime::from_nanos(20), MarkKind::Doubt);
        assert_eq!(
            mark.kind(),
            MarkKind::Doubt,
            "a mark at the same time moves the kind"
        );
    }

    #[test]
    fn the_notice_mark_only_moves_forward() {
        use super::hold::Uptime;
        use super::notice::{MarkKind, NoticeMark};
        let mark = NoticeMark::new();
        assert_eq!(mark.get(), None);
        mark.mark(Uptime::from_nanos(10), MarkKind::Notice);
        mark.mark(Uptime::from_nanos(5), MarkKind::Start);
        assert_eq!(mark.get(), Some(Uptime::from_nanos(10)));
        mark.clone().mark(Uptime::from_nanos(20), MarkKind::Doubt);
        assert_eq!(
            mark.get(),
            Some(Uptime::from_nanos(20)),
            "a clone moves the same mark"
        );
        assert_eq!(mark.kind(), MarkKind::Doubt);
    }

    #[cfg(all(debug_assertions, feature = "stall-seam"))]
    mod notice_thread {
        use super::super::hold::Uptime;
        use super::super::notice::{
            notice_came, wait_seam, AxCalls, Context, Focus, MarkKind, NoticeMark, NoticeThread,
            Observed, NOTICE_FRONT_STEP, NOTICE_READ_DEADLINE, NO_GRANT_RETRY,
        };
        use core_foundation::base::{kCFAllocatorDefault, CFType, TCFType};
        use core_foundation::runloop::{
            kCFRunLoopDefaultMode, kCFRunLoopEntry, CFRunLoop, CFRunLoopActivity,
            CFRunLoopObserver, CFRunLoopObserverContext, CFRunLoopObserverCreate,
            CFRunLoopObserverRef, CFRunLoopSource, CFRunLoopSourceContext, CFRunLoopSourceCreate,
            CFRunLoopSourceIsValid, CFRunLoopSourceRef,
        };
        use core_foundation::string::CFString;
        use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, AtomicUsize, Ordering};
        use std::sync::{Mutex, MutexGuard, PoisonError};
        use std::time::{Duration, Instant};

        static SERIAL: Mutex<()> = Mutex::new(());
        static FRONT: AtomicI32 = AtomicI32::new(0);
        static PANIC_FRONT: AtomicBool = AtomicBool::new(false);
        static OBSERVE: AtomicI32 = AtomicI32::new(0);
        static OBSERVE_PID: AtomicI32 = AtomicI32::new(0);
        static DELAY_MS: AtomicU64 = AtomicU64::new(0);
        static DELAY_PID: AtomicI32 = AtomicI32::new(0);
        static REAL_SOURCE: AtomicBool = AtomicBool::new(false);
        static SOURCES: Mutex<Vec<(i32, usize)>> = Mutex::new(Vec::new());
        static SUBROLE: AtomicI32 = AtomicI32::new(0);
        static FOCUSED: AtomicI32 = AtomicI32::new(0);
        static NOW: AtomicU64 = AtomicU64::new(0);
        static DEADLINES: Mutex<Vec<Duration>> = Mutex::new(Vec::new());
        static OBSERVED: Mutex<Vec<(i32, Instant)>> = Mutex::new(Vec::new());

        const TEN_S: u64 = 10_000_000_000;

        fn serial() -> MutexGuard<'static, ()> {
            let guard = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
            FRONT.store(0, Ordering::SeqCst);
            PANIC_FRONT.store(false, Ordering::SeqCst);
            OBSERVE.store(0, Ordering::SeqCst);
            OBSERVE_PID.store(0, Ordering::SeqCst);
            DELAY_MS.store(0, Ordering::SeqCst);
            DELAY_PID.store(0, Ordering::SeqCst);
            REAL_SOURCE.store(false, Ordering::SeqCst);
            SOURCES
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clear();
            SUBROLE.store(0, Ordering::SeqCst);
            FOCUSED.store(0, Ordering::SeqCst);
            NOW.store(TEN_S, Ordering::SeqCst);
            DEADLINES
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clear();
            OBSERVED
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clear();
            guard
        }

        fn element(name: &'static str) -> CFType {
            CFString::from_static_string(name).as_CFType()
        }

        fn app(_pid: libc::pid_t) -> Option<CFType> {
            Some(element("app"))
        }

        fn set_deadline(_element: &CFType, deadline: Duration) {
            DEADLINES
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(deadline);
        }

        extern "C" fn performs_nothing(_info: *const std::ffi::c_void) {}

        fn source_of(pid: libc::pid_t) -> Option<CFRunLoopSource> {
            if !REAL_SOURCE.load(Ordering::SeqCst) {
                return None;
            }
            let mut context = CFRunLoopSourceContext {
                version: 0,
                info: std::ptr::null_mut(),
                retain: None,
                release: None,
                copyDescription: None,
                equal: None,
                hash: None,
                schedule: None,
                cancel: None,
                perform: performs_nothing,
            };
            // SAFETY: `context` is a valid version 0 context that the call
            // copies; the call returns a +1 reference.
            let source = unsafe { CFRunLoopSourceCreate(kCFAllocatorDefault, 0, &mut context) };
            // SAFETY: a +1 reference that this wrapper now owns and releases.
            let source = unsafe { CFRunLoopSource::wrap_under_create_rule(source) };
            let kept = source.clone();
            SOURCES
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push((pid, kept.as_concrete_TypeRef() as usize));
            std::mem::forget(kept);
            Some(source)
        }

        fn source_valid(source: usize) -> bool {
            // SAFETY: `source` is a run loop source that `source_of` retained
            // once more and never released, so it is live for the call.
            unsafe { CFRunLoopSourceIsValid(source as CFRunLoopSourceRef) != 0 }
        }

        fn observe(
            pid: libc::pid_t,
            _app: &CFType,
            _refcon: *mut std::ffi::c_void,
        ) -> Result<Observed, i32> {
            OBSERVED
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push((pid, Instant::now()));
            let delayed = DELAY_PID.load(Ordering::SeqCst);
            if delayed == 0 || delayed == pid {
                std::thread::sleep(Duration::from_millis(DELAY_MS.load(Ordering::SeqCst)));
            }
            let refused = OBSERVE_PID.load(Ordering::SeqCst);
            match OBSERVE.load(Ordering::SeqCst) {
                code if code != 0 && (refused == 0 || refused == pid) => Err(code),
                _ => Ok(Observed {
                    source: source_of(pid),
                    observer: None,
                }),
            }
        }

        fn focused(_app: &CFType) -> Result<CFType, i32> {
            match FOCUSED.load(Ordering::SeqCst) {
                0 => Ok(element("focused")),
                code => Err(code),
            }
        }

        fn subrole(_element: &CFType) -> Result<Option<String>, i32> {
            match SUBROLE.load(Ordering::SeqCst) {
                0 => Ok(Some(String::from("AXSecureTextField"))),
                1 => Ok(Some(String::from("AXTextField"))),
                code => Err(code),
            }
        }

        fn front() -> Option<libc::pid_t> {
            assert!(
                !PANIC_FRONT.load(Ordering::SeqCst),
                "the stand-in front reader panics"
            );
            match FRONT.load(Ordering::SeqCst) {
                0 => None,
                pid => Some(pid),
            }
        }

        fn now() -> Uptime {
            Uptime::from_nanos(NOW.load(Ordering::SeqCst))
        }

        const STAND_IN: AxCalls = AxCalls {
            app,
            set_deadline,
            observe,
            focused,
            subrole,
        };

        fn started(mark: &NoticeMark) -> NoticeThread {
            NoticeThread::spawn_with(
                Box::new(front),
                mark.clone(),
                now,
                Box::new(|_, _| {}),
                STAND_IN,
            )
            .expect("the notice thread starts")
        }

        static WAITS: AtomicUsize = AtomicUsize::new(0);
        static READS: Mutex<Vec<usize>> = Mutex::new(Vec::new());
        static SWITCH_AT: AtomicUsize = AtomicUsize::new(0);
        static SWITCH_TO: AtomicI32 = AtomicI32::new(0);
        static REGISTERED: Mutex<Vec<(i32, usize, usize)>> = Mutex::new(Vec::new());
        static WAITS_HANDED: Mutex<Vec<Option<Duration>>> = Mutex::new(Vec::new());

        const GIVE_UP: Duration = Duration::from_secs(20);

        extern "C" fn a_wait_begins(
            _observer: CFRunLoopObserverRef,
            _activity: CFRunLoopActivity,
            _info: *mut std::ffi::c_void,
        ) {
            WAITS.fetch_add(1, Ordering::SeqCst);
        }

        fn count_the_waits_of_this_thread() {
            let mut context = CFRunLoopObserverContext {
                version: 0,
                info: std::ptr::null_mut(),
                retain: None,
                release: None,
                copyDescription: None,
            };
            // SAFETY: `context` is a valid version 0 context that the call
            // copies; the call returns a +1 reference that the wrapper owns.
            let observer = unsafe {
                CFRunLoopObserver::wrap_under_create_rule(CFRunLoopObserverCreate(
                    kCFAllocatorDefault,
                    kCFRunLoopEntry,
                    1,
                    0,
                    a_wait_begins,
                    &mut context,
                ))
            };
            // SAFETY: kCFRunLoopDefaultMode is a constant CFString that
            // CoreFoundation exports and never writes.
            CFRunLoop::get_current().add_observer(&observer, unsafe { kCFRunLoopDefaultMode });
        }

        fn counted_front() -> Option<libc::pid_t> {
            let read = {
                let mut reads = READS.lock().unwrap_or_else(PoisonError::into_inner);
                reads.push(WAITS.load(Ordering::SeqCst));
                reads.len()
            };
            WAITS_HANDED
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(wait_seam::HANDED.with(std::cell::Cell::get));
            if read == 1 {
                count_the_waits_of_this_thread();
            }
            if read == SWITCH_AT.load(Ordering::SeqCst) {
                FRONT.store(SWITCH_TO.load(Ordering::SeqCst), Ordering::SeqCst);
            }
            front()
        }

        fn counted_observe(
            pid: libc::pid_t,
            app: &CFType,
            refcon: *mut std::ffi::c_void,
        ) -> Result<Observed, i32> {
            let reads = READS.lock().unwrap_or_else(PoisonError::into_inner).len();
            REGISTERED
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push((pid, reads, WAITS.load(Ordering::SeqCst)));
            observe(pid, app, refcon)
        }

        const COUNTED: AxCalls = AxCalls {
            observe: counted_observe,
            ..STAND_IN
        };

        fn counted(mark: &NoticeMark, switch: Option<(usize, i32)>) -> NoticeThread {
            WAITS.store(0, Ordering::SeqCst);
            READS.lock().unwrap_or_else(PoisonError::into_inner).clear();
            REGISTERED
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clear();
            WAITS_HANDED
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clear();
            let (at, to) = switch.unwrap_or((0, 0));
            SWITCH_AT.store(at, Ordering::SeqCst);
            SWITCH_TO.store(to, Ordering::SeqCst);
            NoticeThread::spawn_with(
                Box::new(counted_front),
                mark.clone(),
                now,
                Box::new(|_, _| {}),
                COUNTED,
            )
            .expect("the notice thread starts")
        }

        fn steps_in(span: Duration) -> usize {
            usize::try_from(span.as_nanos() / NOTICE_FRONT_STEP.as_nanos())
                .expect("a count of steps")
        }

        fn after_waits(count: usize) {
            until(GIVE_UP, || WAITS.load(Ordering::SeqCst) >= count);
        }

        fn observed_of(pid: i32) -> Vec<Instant> {
            OBSERVED
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .iter()
                .filter(|(of, _)| *of == pid)
                .map(|(_, at)| *at)
                .collect()
        }

        fn until(within: Duration, done: impl Fn() -> bool) -> bool {
            let deadline = Instant::now() + within;
            while Instant::now() < deadline {
                if done() {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            done()
        }

        fn in_doubt_of_1(mark: &NoticeMark) -> NoticeThread {
            FRONT.store(1, Ordering::SeqCst);
            OBSERVE.store(-25204, Ordering::SeqCst);
            let thread = started(mark);
            assert!(
                until(Duration::from_millis(500), || mark.doubt()),
                "a doubt stands over program 1"
            );
            assert_eq!(mark.get(), Some(Uptime::from_nanos(TEN_S)));
            NOW.store(2 * TEN_S, Ordering::SeqCst);
            thread
        }

        #[test]
        fn a_doubt_ended_by_a_registration_of_its_program_moves_the_mark_to_its_end() {
            let _serial = serial();
            let mark = NoticeMark::new();
            let mut thread = in_doubt_of_1(&mark);
            SUBROLE.store(1, Ordering::SeqCst);
            OBSERVE.store(0, Ordering::SeqCst);
            let ended = until(Duration::from_millis(500), || !mark.doubt());
            assert!(thread.stop_within(Duration::from_millis(100)));
            assert!(ended, "a registration of that program ends the doubt");
            assert_eq!(
                (mark.get(), mark.kind()),
                (Some(Uptime::from_nanos(2 * TEN_S)), MarkKind::Doubt),
                "the end of the doubt moves the mark, although the Start read found another element"
            );
        }

        #[test]
        fn a_doubt_ended_by_another_program_in_front_moves_the_mark_before_that_program_is_tried() {
            let _serial = serial();
            let mark = NoticeMark::new();
            let mut thread = in_doubt_of_1(&mark);
            DELAY_PID.store(2, Ordering::SeqCst);
            DELAY_MS.store(NOTICE_READ_DEADLINE.as_millis() as u64, Ordering::SeqCst);
            FRONT.store(2, Ordering::SeqCst);
            let tried = until(Duration::from_millis(500), || !observed_of(2).is_empty());
            let during = (mark.doubt(), mark.get());
            NOW.store(3 * TEN_S, Ordering::SeqCst);
            let refused = until(Duration::from_millis(600), || mark.doubt());
            let after = (mark.doubt(), mark.get());
            assert!(thread.stop_within(
                NOTICE_READ_DEADLINE + NOTICE_FRONT_STEP + Duration::from_millis(100)
            ));
            assert!(tried && refused);
            assert_eq!(
                during,
                (false, Some(Uptime::from_nanos(2 * TEN_S))),
                "while program 2 is asked: the doubt over 1 has ended at the front read, with a mark"
            );
            assert_eq!(
                after,
                (true, Some(Uptime::from_nanos(3 * TEN_S))),
                "program 2 did not answer: its Unanswered mark and a doubt of its own"
            );
        }

        #[test]
        fn a_doubt_ended_by_the_threads_stop_moves_the_mark_to_its_end() {
            let _serial = serial();
            let mark = NoticeMark::new();
            let mut thread = in_doubt_of_1(&mark);
            assert!(thread.stop_within(Duration::from_millis(100)));
            assert_eq!(
                (mark.doubt(), mark.get()),
                (false, Some(Uptime::from_nanos(2 * TEN_S)))
            );
        }

        #[test]
        fn a_doubt_ended_by_the_threads_end_moves_the_mark_to_its_end() {
            let _serial = serial();
            let mark = NoticeMark::new();
            let mut thread = in_doubt_of_1(&mark);
            PANIC_FRONT.store(true, Ordering::SeqCst);
            let ended = until(Duration::from_millis(500), || !mark.doubt());
            assert!(thread.stop_within(Duration::from_millis(100)));
            assert!(ended, "a thread that ends ends its doubt");
            assert_eq!(mark.get(), Some(Uptime::from_nanos(2 * TEN_S)));
        }

        #[test]
        fn a_doubt_stands_while_nothing_is_in_front() {
            let _serial = serial();
            let mark = NoticeMark::new();
            let mut thread = in_doubt_of_1(&mark);
            FRONT.store(0, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(200));
            let standing = (mark.doubt(), mark.get());
            assert!(thread.stop_within(Duration::from_millis(100)));
            assert_eq!(
                standing,
                (true, Some(Uptime::from_nanos(TEN_S))),
                "nothing in front is no other program and no registration that succeeded"
            );
        }

        #[test]
        fn a_doubt_stands_through_each_retry_of_its_program() {
            let _serial = serial();
            DELAY_MS.store(200, Ordering::SeqCst);
            let mark = NoticeMark::new();
            let mut thread = in_doubt_of_1(&mark);
            let mut gaps = 0;
            let started = Instant::now();
            while started.elapsed() < Duration::from_millis(800) {
                if !mark.doubt() {
                    gaps += 1;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            let tries = observed_of(1).len();
            assert!(thread.stop_within(
                NOTICE_READ_DEADLINE + NOTICE_FRONT_STEP + Duration::from_millis(100)
            ));
            assert!(tries >= 3, "the program was tried again: {tries}");
            assert_eq!(
                gaps, 0,
                "reads of the doubt that found none while program 1 was retried"
            );
        }

        #[test]
        fn a_program_refused_for_lack_of_the_grant_is_tried_at_once_when_it_comes_back_in_front() {
            let _serial = serial();
            SUBROLE.store(1, Ordering::SeqCst);
            OBSERVE.store(-25211, Ordering::SeqCst);
            OBSERVE_PID.store(1, Ordering::SeqCst);
            FRONT.store(1, Ordering::SeqCst);
            let mark = NoticeMark::new();
            let mut thread = started(&mark);
            assert!(until(Duration::from_millis(300), || observed_of(1).len() == 1));
            let refused = observed_of(1)[0];
            FRONT.store(2, Ordering::SeqCst);
            assert!(until(Duration::from_millis(500), || observed_of(2).len() == 1));
            OBSERVE.store(0, Ordering::SeqCst);
            FRONT.store(1, Ordering::SeqCst);
            let tried = until(NO_GRANT_RETRY + Duration::from_millis(500), || {
                observed_of(1).len() == 2
            });
            let again = observed_of(1).get(1).map(|at| at.duration_since(refused));
            assert!(thread.stop_within(Duration::from_millis(100)));
            assert!(tried, "tried again when it came back");
            assert!(
                again.is_some_and(|again| again < NO_GRANT_RETRY),
                "tried again {again:?} after its refusal: another program in front ended the wait of {NO_GRANT_RETRY:?}"
            );
        }

        #[test]
        fn another_program_in_front_after_a_refused_grant_is_tried_before_the_wait_of_the_grant_ends(
        ) {
            let _serial = serial();
            SUBROLE.store(1, Ordering::SeqCst);
            OBSERVE.store(-25211, Ordering::SeqCst);
            OBSERVE_PID.store(1, Ordering::SeqCst);
            FRONT.store(1, Ordering::SeqCst);
            let mark = NoticeMark::new();
            let mut thread = started(&mark);
            assert!(until(Duration::from_millis(300), || observed_of(1).len() == 1));
            let refused = observed_of(1)[0];
            FRONT.store(2, Ordering::SeqCst);
            let tried = until(NO_GRANT_RETRY + Duration::from_millis(500), || {
                observed_of(2).len() == 1
            });
            let after = observed_of(2).first().map(|at| at.duration_since(refused));
            assert!(thread.stop_within(Duration::from_millis(100)));
            assert!(tried);
            assert!(
                after.is_some_and(|after| after < NO_GRANT_RETRY),
                "program 2 tried {after:?} after the refusal of program 1: the wait of {NO_GRANT_RETRY:?} is program 1's alone"
            );
        }

        #[test]
        fn only_a_registration_that_does_not_answer_makes_a_doubt() {
            let _serial = serial();
            for code in [-25200, -25205, -25207, -25208, -25211, -25212] {
                FRONT.store(1, Ordering::SeqCst);
                OBSERVE.store(code, Ordering::SeqCst);
                let mark = NoticeMark::new();
                let mut thread = started(&mark);
                std::thread::sleep(Duration::from_millis(150));
                let seen = (mark.doubt(), mark.get());
                assert!(thread.stop_within(Duration::from_millis(100)));
                assert_eq!(seen, (false, None), "{code}");
            }
        }

        #[test]
        fn a_start_read_whose_focused_element_has_no_value_or_no_such_attribute_moves_no_mark() {
            let _serial = serial();
            for code in [-25212, -25205] {
                FOCUSED.store(code, Ordering::SeqCst);
                FRONT.store(1, Ordering::SeqCst);
                let mark = NoticeMark::new();
                let mut thread = started(&mark);
                let registered = until(Duration::from_millis(300), || !observed_of(1).is_empty());
                std::thread::sleep(Duration::from_millis(50));
                assert!(thread.stop_within(Duration::from_millis(100)));
                assert!(registered);
                assert_eq!(mark.get(), None, "{code}");
                OBSERVED
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .clear();
            }
        }

        #[test]
        fn a_start_read_of_another_element_moves_no_mark() {
            let _serial = serial();
            SUBROLE.store(1, Ordering::SeqCst);
            FRONT.store(1, Ordering::SeqCst);
            let mark = NoticeMark::new();
            let mut thread = started(&mark);
            let registered = until(Duration::from_millis(300), || !observed_of(1).is_empty());
            std::thread::sleep(Duration::from_millis(50));
            assert!(thread.stop_within(Duration::from_millis(100)));
            assert!(registered);
            assert_eq!(mark.get(), None);
        }

        /// It reads a prefix of 20 counted steps, one second of steps, and asks one read of the
        /// front in each, which is what a test that counts the thread's steps can hold.
        #[test]
        fn the_thread_reads_the_front_at_least_ten_times_in_a_second() {
            let _serial = serial();
            let mark = NoticeMark::new();
            let mut thread = counted(&mark, None);
            let second = steps_in(Duration::from_secs(1));
            after_waits(second);
            let reads = READS.lock().unwrap_or_else(PoisonError::into_inner).clone();
            let steps = thread.steps();
            assert!(thread.stop_within(GIVE_UP), "the thread ends when asked");
            let in_the_second: Vec<usize> = reads.into_iter().take(second).collect();
            assert!(
                in_the_second.len() >= 10,
                "one read in each step of {NOTICE_FRONT_STEP:?}, with room: {steps:?}"
            );
            assert_eq!(
                in_the_second,
                (0..second).collect::<Vec<usize>>(),
                "the waits of the thread before each read of the front, one read in each step: {steps:?}"
            );
        }

        #[test]
        fn the_thread_hands_each_of_its_waits_one_step() {
            let _serial = serial();
            let mark = NoticeMark::new();
            let mut thread = counted(&mark, None);
            let second = steps_in(Duration::from_secs(1));
            after_waits(second + 1);
            let handed = WAITS_HANDED
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone();
            assert!(thread.stop_within(GIVE_UP), "the thread ends when asked");
            let after_each_wait: Vec<Option<Duration>> =
                handed.into_iter().skip(1).take(second).collect();
            assert_eq!(
                after_each_wait,
                vec![Some(NOTICE_FRONT_STEP); second],
                "the time that the thread handed to each of its waits, read at the read after it"
            );
        }

        #[test]
        fn a_front_change_takes_the_old_programs_source_off_its_run_loop() {
            let _serial = serial();
            REAL_SOURCE.store(true, Ordering::SeqCst);
            SUBROLE.store(1, Ordering::SeqCst);
            FRONT.store(1, Ordering::SeqCst);
            let mark = NoticeMark::new();
            let mut thread = started(&mark);
            assert!(until(Duration::from_millis(300), || !observed_of(1).is_empty()));
            FRONT.store(2, Ordering::SeqCst);
            assert!(until(Duration::from_millis(300), || !observed_of(2).is_empty()));
            std::thread::sleep(Duration::from_millis(20));
            let sources = SOURCES
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone();
            let valid: Vec<(i32, bool)> = sources
                .iter()
                .map(|&(pid, source)| (pid, source_valid(source)))
                .collect();
            assert!(thread.stop_within(Duration::from_millis(100)));
            assert_eq!(
                valid,
                [(1, false), (2, true)],
                "the source of the program no longer in front is taken off; the new one is live"
            );
        }

        #[test]
        fn the_thread_waits_a_step_when_nothing_is_in_front() {
            let _serial = serial();
            let mark = NoticeMark::new();
            let mut thread = started(&mark);
            std::thread::sleep(Duration::from_secs(1));
            let steps = thread.steps();
            assert!(
                thread.stop_within(Duration::from_millis(100)),
                "it stops at once"
            );
            assert!(steps.front_reads <= 22, "front reads in 1 s: {steps:?}");
            assert_eq!(steps.tries, 0);
            assert_eq!(mark.get(), None);
        }

        #[test]
        fn the_thread_waits_a_step_after_each_refused_registration() {
            let _serial = serial();
            for code in [-25211, -25208, -25207, -25204] {
                FRONT.store(4242, Ordering::SeqCst);
                OBSERVE.store(code, Ordering::SeqCst);
                let mark = NoticeMark::new();
                let mut thread = started(&mark);
                std::thread::sleep(Duration::from_secs(1));
                let steps = thread.steps();
                assert!(
                    thread.stop_within(Duration::from_millis(100)),
                    "{code}: it stops at once"
                );
                assert!(steps.front_reads <= 22, "{code}: {steps:?}");
                assert!(steps.tries <= 22, "{code}: {steps:?}");
                if code == -25211 {
                    assert!(
                        steps.tries <= 2,
                        "a refusal for lack of the grant is tried again at most once a second: {steps:?}"
                    );
                }
            }
        }

        #[test]
        fn a_registration_that_does_not_answer_leaves_a_standing_doubt_until_one_answers_or_another_program_comes_in_front(
        ) {
            let _serial = serial();
            FRONT.store(1, Ordering::SeqCst);
            OBSERVE.store(-25204, Ordering::SeqCst);
            let mark = NoticeMark::new();
            let mut thread = started(&mark);
            std::thread::sleep(Duration::from_millis(200));
            assert!(
                mark.doubt(),
                "a doubt stands while the program does not answer"
            );
            assert_eq!(
                mark.get(),
                Some(Uptime::from_nanos(TEN_S)),
                "the Unanswered mark"
            );
            assert_eq!(mark.kind(), MarkKind::Doubt);
            OBSERVE.store(0, Ordering::SeqCst);
            SUBROLE.store(1, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(200));
            assert!(
                !mark.doubt(),
                "a registration of that program that succeeds ends it"
            );
            FRONT.store(2, Ordering::SeqCst);
            OBSERVE.store(-25204, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(200));
            assert!(mark.doubt());
            FRONT.store(3, Ordering::SeqCst);
            OBSERVE.store(-25208, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(200));
            assert!(!mark.doubt(), "another program in front ends it");
            assert!(thread.stop_within(Duration::from_millis(100)));
        }

        #[test]
        fn a_registration_whose_focused_element_does_not_answer_marks_a_doubt_at_the_start() {
            let _serial = serial();
            FOCUSED.store(-25204, Ordering::SeqCst);
            FRONT.store(1, Ordering::SeqCst);
            let mark = NoticeMark::new();
            let mut thread = started(&mark);
            std::thread::sleep(Duration::from_millis(200));
            assert!(thread.stop_within(Duration::from_millis(100)));
            assert_eq!(
                mark.get(),
                Some(Uptime::from_nanos(TEN_S)),
                "a focused element that does not answer is a doubt, and the Start read marks it"
            );
            assert_eq!(mark.kind(), MarkKind::Start);
        }

        #[test]
        fn every_registration_makes_the_start_read_on_the_clock_that_it_is_handed() {
            let _serial = serial();
            FRONT.store(1, Ordering::SeqCst);
            let mark = NoticeMark::new();
            let mut thread = started(&mark);
            std::thread::sleep(Duration::from_millis(200));
            assert_eq!(mark.get(), Some(Uptime::from_nanos(TEN_S)));
            assert_eq!(mark.kind(), MarkKind::Start);
            NOW.store(2 * TEN_S, Ordering::SeqCst);
            FRONT.store(2, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(200));
            assert_eq!(
                mark.get(),
                Some(Uptime::from_nanos(2 * TEN_S)),
                "the watch of the next program in front makes its Start read too"
            );
            assert!(thread.stop_within(Duration::from_millis(100)));
            let deadlines = DEADLINES
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone();
            assert!(
                deadlines.len() >= 4,
                "the app and its focused element, twice: {deadlines:?}"
            );
            assert!(deadlines
                .iter()
                .all(|&deadline| deadline == NOTICE_READ_DEADLINE));
        }

        #[test]
        fn the_thread_registers_a_new_program_in_front_within_one_step() {
            let _serial = serial();
            SUBROLE.store(1, Ordering::SeqCst);
            FRONT.store(1, Ordering::SeqCst);
            let change = steps_in(Duration::from_millis(200)) + 1;
            let mark = NoticeMark::new();
            let mut thread = counted(&mark, Some((change, 2)));
            after_waits(change - 1 + steps_in(Duration::from_millis(300)));
            let registered = REGISTERED
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone();
            assert!(thread.stop_within(GIVE_UP), "the thread ends when asked");
            let of_2: Vec<(usize, usize)> = registered
                .iter()
                .filter(|&&(pid, _, _)| pid == 2)
                .map(|&(_, read, waits)| (read, waits))
                .collect();
            assert_eq!(
                of_2.len(),
                1,
                "one registration of the new program: {registered:?}"
            );
            let within = (change, change - 1);
            assert_eq!(
                of_2[0],
                within,
                "registered at (front read, waits) {:?} after the change at front read {change}; within one step is {within:?}",
                of_2[0]
            );
        }

        #[test]
        fn a_notice_whose_element_does_not_answer_is_a_doubt_and_each_element_gets_the_deadline() {
            let _serial = serial();
            let mark = NoticeMark::new();
            let context = Context {
                mark: mark.clone(),
                uptime: now,
                on_notice: Box::new(|_, _| {}),
                calls: STAND_IN,
            };
            let refcon = std::ptr::from_ref(&context).cast_mut().cast();
            let focused = element("focused");
            SUBROLE.store(1, Ordering::SeqCst);
            // SAFETY: `refcon` is a live Context and `focused` a live CF value
            // for the call; the stand-in calls read no Accessibility element.
            unsafe {
                notice_came(
                    std::ptr::null(),
                    focused.as_CFTypeRef(),
                    std::ptr::null(),
                    refcon,
                )
            };
            assert_eq!(mark.get(), None, "another element moves nothing");
            SUBROLE.store(-25204, Ordering::SeqCst);
            // SAFETY: as above.
            unsafe {
                notice_came(
                    std::ptr::null(),
                    focused.as_CFTypeRef(),
                    std::ptr::null(),
                    refcon,
                )
            };
            assert_eq!(mark.get(), Some(Uptime::from_nanos(TEN_S)));
            assert_eq!(mark.kind(), MarkKind::Doubt);
            context.seen(super::super::notice::Seen::Notice, Focus::Secure);
            assert_eq!(mark.kind(), MarkKind::Notice);
            let deadlines = DEADLINES
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone();
            assert_eq!(deadlines, [NOTICE_READ_DEADLINE, NOTICE_READ_DEADLINE]);
        }
    }

    use super::test_support::{agreed_reading, one_hold_at_a_time};
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    #[test]
    fn a_session_is_known_only_at_the_console() {
        assert_eq!(session_of(None), Session::NoDictionary);
        assert_eq!(session_of(Some(false)), Session::NotAtConsole);
        assert_eq!(session_of(Some(true)), Session::AtConsole);
    }

    #[test]
    fn a_session_not_at_the_console_or_with_no_dictionary_reads_unknown_and_asks_nothing() {
        for session in [Session::NotAtConsole, Session::NoDictionary] {
            for secure in [false, true] {
                let asked = std::cell::Cell::new(0);
                let reading = reading_under(session, || {
                    asked.set(asked.get() + 1);
                    secure
                });
                assert_eq!((reading, asked.get()), (Reading::Unknown, 0), "{session:?}");
            }
        }
    }

    #[test]
    fn a_session_at_the_console_reads_the_state() {
        assert_eq!(reading_under(Session::AtConsole, || false), Reading::Off);
        assert_eq!(reading_under(Session::AtConsole, || true), Reading::On);
    }

    #[test]
    fn the_session_of_this_process_has_a_dictionary() {
        assert_ne!(read_session(), Session::NoDictionary);
    }

    #[test]
    fn a_refused_hold_gives_its_status_and_no_hold() {
        assert!(matches!(
            hold_with(|| -50, || 0),
            Err(SecureInputRefused(-50))
        ));
    }

    #[test]
    fn each_held_hold_makes_one_release_call_on_its_drop_and_a_refused_hold_none() {
        static RELEASES: AtomicUsize = AtomicUsize::new(0);
        fn counted_release() -> i32 {
            RELEASES.fetch_add(1, Ordering::SeqCst);
            0
        }
        let releases = || RELEASES.load(Ordering::SeqCst);
        let held = hold_with(|| 0, counted_release).expect("a hold with the status 0 is held");
        assert_eq!(releases(), 0, "release calls before the drop of the hold");
        drop(held);
        assert_eq!(releases(), 1, "release calls after the drop of a held hold");
        let refused = hold_with(|| -50, counted_release);
        assert!(matches!(refused, Err(SecureInputRefused(-50))));
        drop(refused);
        assert_eq!(
            releases(),
            1,
            "release calls after the drop of a refused hold"
        );
        let first = hold_with(|| 0, counted_release).expect("the first of two holds is held");
        let second = hold_with(|| 0, counted_release).expect("the second of two holds is held");
        drop((first, second));
        assert_eq!(
            releases(),
            3,
            "release calls after the drops of two more holds"
        );
    }

    #[test]
    fn the_production_hold_takes_and_gives_back_secure_input_through_the_system_calls() {
        let one = one_hold_at_a_time();
        let Some(hold) = one.production_hold_or_skip(
            "the_production_hold_takes_and_gives_back_secure_input_through_the_system_calls",
        ) else {
            return;
        };
        let takes = std::ptr::fn_addr_eq(hold.enable, enable_secure_event_input as fn() -> i32);
        let gives_back =
            std::ptr::fn_addr_eq(hold.release, disable_secure_event_input as fn() -> i32);
        if gives_back {
            drop(hold);
        } else {
            std::mem::forget(hold);
            disable_secure_event_input();
        }
        assert!(
            takes,
            "the hold that hold_secure_input makes was not taken through EnableSecureEventInput"
        );
        assert!(
            gives_back,
            "the hold that hold_secure_input makes does not give secure input back through \
             DisableSecureEventInput"
        );
    }

    #[test]
    fn a_dropped_hold_gives_the_reading_of_before_back() {
        let one = one_hold_at_a_time();
        let limit = Duration::from_secs(2);
        let before = agreed_reading(secure_input_active, limit);
        let Some(hold) = one.hold_or_skip("a_dropped_hold_gives_the_reading_of_before_back") else {
            return;
        };
        drop(hold);
        let started = std::time::Instant::now();
        while secure_input_active() != before.0 {
            assert!(
                started.elapsed() < limit,
                "2 s after the hold dropped the predicate reads {} and read {} before the hold",
                secure_input_active(),
                before.0
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn the_predicate_follows_the_session_reading_through_a_hold() {
        let one = one_hold_at_a_time();
        let limit = Duration::from_secs(2);
        let before = agreed_reading(secure_input_active, limit);
        let Some(hold) =
            one.hold_or_skip("the_predicate_follows_the_session_reading_through_a_hold")
        else {
            return;
        };
        let during = agreed_reading(secure_input_active, limit);
        drop(hold);
        let after = agreed_reading(secure_input_active, limit);
        assert_eq!(before.0, before.1, "before the hold: (predicate, session)");
        assert_eq!(
            during,
            (true, true),
            "during the hold: (predicate, session)"
        );
        assert_eq!(after.0, after.1, "after the hold: (predicate, session)");
    }
}
