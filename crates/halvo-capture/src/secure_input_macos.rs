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

    #[cfg(test)]
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

#[cfg(test)]
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
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Arc;
    use std::thread::JoinHandle;
    use std::time::{Duration, Instant};

    use core_foundation::base::{CFType, CFTypeRef, TCFType};
    use core_foundation::runloop::{
        kCFRunLoopDefaultMode, CFRunLoop, CFRunLoopSource, CFRunLoopSourceInvalidate,
        CFRunLoopSourceRef,
    };
    use core_foundation::string::{CFString, CFStringRef};

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
    const AX_ERROR_CANNOT_COMPLETE: i32 = -25204;
    const AX_ERROR_ATTRIBUTE_UNSUPPORTED: i32 = -25205;
    const AX_ERROR_NO_VALUE: i32 = -25212;

    /// The step at which the thread reads which program is in front.
    pub const NOTICE_FRONT_STEP: Duration = Duration::from_millis(50);

    /// The deadline of one question to the program in front: a read of an
    /// element's subrole, of its focused element, or the registration.
    pub const NOTICE_READ_DEADLINE: Duration = Duration::from_millis(250);

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

    /// The uptime in nanoseconds of the newest notice that marks, 0 before
    /// any. It only moves forward.
    #[derive(Clone, Default)]
    pub struct NoticeMark(Arc<AtomicU64>);

    impl NoticeMark {
        pub fn new() -> Self {
            Self::default()
        }

        /// Moves the mark to `uptime` unless it already stands later.
        pub fn mark(&self, uptime: u64) {
            self.0.fetch_max(uptime, Ordering::SeqCst);
        }

        pub fn get(&self) -> u64 {
            self.0.load(Ordering::SeqCst)
        }
    }

    /// What the thread hands to its caller for each notice: where it came
    /// from and its focus; the caller may log it.
    pub type OnNotice = Box<dyn Fn(Seen, Focus) + Send + Sync>;

    struct Context {
        mark: NoticeMark,
        uptime: fn() -> u64,
        on_notice: OnNotice,
    }

    impl Context {
        fn seen(&self, seen: Seen, focus: Focus) {
            (self.on_notice)(seen, focus);
            if focus.marks() {
                self.mark.mark((self.uptime)());
            }
        }
    }

    fn subrole_of(element: &CFType) -> Result<Option<String>, i32> {
        // SAFETY: `element` is a live AXUIElement for the call; the call only
        // sets the deadline of the questions that this process asks it.
        unsafe {
            AXUIElementSetMessagingTimeout(
                element.as_CFTypeRef(),
                NOTICE_READ_DEADLINE.as_secs_f32(),
            )
        };
        let attribute = CFString::from_static_string("AXSubrole");
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
        let value = unsafe { CFType::wrap_under_create_rule(value) };
        Ok(value.downcast::<CFString>().map(|text| text.to_string()))
    }

    fn focused_of(app: &CFType) -> Result<CFType, i32> {
        let attribute = CFString::from_static_string("AXFocusedUIElement");
        let mut value: CFTypeRef = std::ptr::null();
        // SAFETY: `app` is a live AXUIElement, `attribute` a live CFString and
        // `value` a writable pointer that the call sets to a +1 reference on
        // success.
        let code = unsafe {
            AXUIElementCopyAttributeValue(
                app.as_CFTypeRef(),
                attribute.as_concrete_TypeRef(),
                &mut value,
            )
        };
        if code != AX_ERROR_SUCCESS {
            return Err(code);
        }
        if value.is_null() {
            return Err(AX_ERROR_NO_VALUE);
        }
        // SAFETY: the call succeeded and gave a +1 reference that this wrapper
        // now owns and releases.
        Ok(unsafe { CFType::wrap_under_create_rule(value) })
    }

    unsafe extern "C" fn notice_came(
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
        let focus = match subrole_of(&element) {
            Ok(subrole) => focus_of(Ok(subrole.as_deref())),
            Err(code) => focus_of(Err(code)),
        };
        context.seen(Seen::Notice, focus);
    }

    struct Watch {
        pid: libc::pid_t,
        source: CFRunLoopSource,
        _observer: CFType,
    }

    fn watch(pid: libc::pid_t, context: &Context) -> Option<Watch> {
        // SAFETY: AXUIElementCreateApplication takes a pid by value and returns
        // a +1 reference or null.
        let app = unsafe { AXUIElementCreateApplication(pid) };
        if app.is_null() {
            return None;
        }
        // SAFETY: a +1 reference that this wrapper now owns and releases.
        let app = unsafe { CFType::wrap_under_create_rule(app) };
        // SAFETY: `app` is a live AXUIElement; the call only sets the deadline
        // of the questions that this process asks it.
        unsafe {
            AXUIElementSetMessagingTimeout(app.as_CFTypeRef(), NOTICE_READ_DEADLINE.as_secs_f32())
        };
        let mut observer: CFTypeRef = std::ptr::null();
        // SAFETY: `notice_came` has the signature of an AXObserverCallback and
        // `observer` is a writable pointer that the call sets to a +1 reference
        // on success.
        let code = unsafe { AXObserverCreate(pid, notice_came, &mut observer) };
        if code != AX_ERROR_SUCCESS || observer.is_null() {
            return None;
        }
        // SAFETY: a +1 reference that this wrapper now owns and releases.
        let observer = unsafe { CFType::wrap_under_create_rule(observer) };
        let notification = CFString::from_static_string("AXFocusedUIElementChanged");
        // SAFETY: `observer` and `app` are live; `context` outlives the
        // observer, which the thread releases before it drops the context.
        let code = unsafe {
            AXObserverAddNotification(
                observer.as_CFTypeRef(),
                app.as_CFTypeRef(),
                notification.as_concrete_TypeRef(),
                std::ptr::from_ref(context).cast_mut().cast(),
            )
        };
        if code == AX_ERROR_CANNOT_COMPLETE {
            context.seen(Seen::Unanswered, Focus::Doubt);
        }
        if code != AX_ERROR_SUCCESS {
            return None;
        }
        // SAFETY: `observer` is a live AXObserver; the call returns its run
        // loop source under the get rule, which the wrapper retains.
        let source = unsafe {
            CFRunLoopSource::wrap_under_get_rule(AXObserverGetRunLoopSource(
                observer.as_CFTypeRef(),
            ))
        };
        // SAFETY: kCFRunLoopDefaultMode is a constant CFString that
        // CoreFoundation exports and never writes.
        CFRunLoop::get_current().add_source(&source, unsafe { kCFRunLoopDefaultMode });
        let focus = match focused_of(&app).and_then(|element| subrole_of(&element)) {
            Ok(subrole) => focus_of(Ok(subrole.as_deref())),
            Err(code) => focus_of(Err(code)),
        };
        context.seen(Seen::Start, focus);
        Some(Watch {
            pid,
            source,
            _observer: observer,
        })
    }

    fn unwatch(watched: &mut Option<Watch>) {
        if let Some(watch) = watched.take() {
            // SAFETY: kCFRunLoopDefaultMode is a constant CFString that
            // CoreFoundation exports and never writes.
            CFRunLoop::get_current().remove_source(&watch.source, unsafe { kCFRunLoopDefaultMode });
            // SAFETY: `watch.source` is a live run loop source that this
            // wrapper retains; invalidating it stops its callbacks.
            unsafe { CFRunLoopSourceInvalidate(watch.source.as_concrete_TypeRef()) };
        }
    }

    /// The thread that follows the program in front and moves the notice mark.
    pub struct NoticeThread {
        run_loop: CFRunLoop,
        stop: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
    }

    impl NoticeThread {
        /// Starts the thread. `front` names the process in front, `uptime`
        /// reads CLOCK_UPTIME_RAW in nanoseconds, the clock of an event's OS
        /// time, and `on_notice` sees every notice before the mark moves.
        pub fn spawn(
            front: Box<dyn Fn() -> Option<libc::pid_t> + Send>,
            mark: NoticeMark,
            uptime: fn() -> u64,
            on_notice: OnNotice,
        ) -> std::io::Result<Self> {
            let stop = Arc::new(AtomicBool::new(false));
            let (loop_tx, loop_rx) = std::sync::mpsc::channel();
            let thread = {
                let stop = stop.clone();
                std::thread::Builder::new()
                    .name("halvo-input-notice".into())
                    .spawn(move || {
                        let context = Box::new(Context {
                            mark,
                            uptime,
                            on_notice,
                        });
                        let _ = loop_tx.send(CFRunLoop::get_current());
                        let mut watched: Option<Watch> = None;
                        while !stop.load(Ordering::SeqCst) {
                            let pid = front();
                            if watched.as_ref().map(|watch| watch.pid) != pid {
                                unwatch(&mut watched);
                                watched = pid.and_then(|pid| watch(pid, &context));
                            }
                            // SAFETY: kCFRunLoopDefaultMode is a constant
                            // CFString that CoreFoundation exports and never
                            // writes.
                            CFRunLoop::run_in_mode(
                                unsafe { kCFRunLoopDefaultMode },
                                NOTICE_FRONT_STEP,
                                false,
                            );
                        }
                        unwatch(&mut watched);
                        drop(context);
                    })?
            };
            let run_loop = loop_rx
                .recv()
                .map_err(|_| std::io::Error::other("the notice thread ended at its start"))?;
            Ok(Self {
                run_loop,
                stop,
                thread: Some(thread),
            })
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
    fn the_notice_mark_only_moves_forward() {
        let mark = super::notice::NoticeMark::new();
        assert_eq!(mark.get(), 0);
        mark.mark(10);
        mark.mark(5);
        assert_eq!(mark.get(), 10);
        mark.clone().mark(20);
        assert_eq!(mark.get(), 20, "a clone moves the same mark");
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
