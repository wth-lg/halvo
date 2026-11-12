//! Keystroke and pointer capture via one listen-only `CGEventTap`.

use std::ffi::c_void;
use std::sync::atomic::{AtomicPtr, AtomicU64, Ordering};
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::thread::JoinHandle;

use core_foundation::base::TCFType;
use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop, CFRunLoopSource};
use core_graphics::event::{
    CGEvent, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventTapProxy, CGEventType, CallbackResult,
};

use crate::event::Modality;
use crate::macos::foreground::{emit_metadata_only, ForegroundReader, Unpersisted};
use crate::secure_input::SecureInputState;
use crate::secure_input_macos::secure_input_active;
use crate::sink::SinkHandle;
use crate::source::{CaptureError, CaptureSource};

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventTapEnable(tap: *mut c_void, enable: bool);
}

type TapEnable = unsafe extern "C" fn(tap: *mut c_void, enable: bool);

#[derive(Clone)]
struct TapPort {
    port: Arc<AtomicPtr<c_void>>,
    enable: TapEnable,
}

impl TapPort {
    fn unset() -> Self {
        Self::with_enable(CGEventTapEnable)
    }

    fn with_enable(enable: TapEnable) -> Self {
        Self {
            port: Arc::new(AtomicPtr::new(std::ptr::null_mut())),
            enable,
        }
    }

    fn store(&self, port: *mut c_void) {
        self.port.store(port, Ordering::Relaxed);
    }

    fn reenable(&self) {
        let port = self.port.load(Ordering::Relaxed);
        if !port.is_null() {
            // SAFETY: `port` is not null, so `store` received it, and
            // `install_tap` stores only the mach port of a tap that it created.
            // Callbacks run only on the tap thread's run loop, which stops
            // before the tap drops. `enable` is `CGEventTapEnable`, set by
            // `unset`, or a test's function that only reads its arguments.
            unsafe { (self.enable)(port, true) };
        }
    }
}

/// The count of keystrokes the tap saw while secure input was active.
#[derive(Clone)]
pub struct SecureInputViolations(Arc<AtomicU64>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecureInputViolated(pub u64);

impl SecureInputViolations {
    fn new() -> Self {
        Self(Arc::new(AtomicU64::new(0)))
    }

    fn record(&self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }

    /// The number of keystrokes seen under secure input.
    pub fn count(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }

    /// `Ok(())` only when no keystroke was recorded under secure input.
    pub fn verify_none(&self) -> Result<(), SecureInputViolated> {
        match self.count() {
            0 => Ok(()),
            n => Err(SecureInputViolated(n)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Classification {
    Record(Modality),
    SecureSuppressed,
    Ignore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TapClass {
    KeyPress,
    OtherKey,
    DiscretePointer,
    ContinuousPointer,
}

const TAP_EVENTS: [(CGEventType, TapClass); 14] = [
    (CGEventType::KeyDown, TapClass::KeyPress),
    (CGEventType::KeyUp, TapClass::OtherKey),
    (CGEventType::FlagsChanged, TapClass::OtherKey),
    (CGEventType::LeftMouseDown, TapClass::DiscretePointer),
    (CGEventType::LeftMouseUp, TapClass::DiscretePointer),
    (CGEventType::RightMouseDown, TapClass::DiscretePointer),
    (CGEventType::RightMouseUp, TapClass::DiscretePointer),
    (CGEventType::OtherMouseDown, TapClass::DiscretePointer),
    (CGEventType::OtherMouseUp, TapClass::DiscretePointer),
    (CGEventType::MouseMoved, TapClass::ContinuousPointer),
    (CGEventType::LeftMouseDragged, TapClass::ContinuousPointer),
    (CGEventType::RightMouseDragged, TapClass::ContinuousPointer),
    (CGEventType::OtherMouseDragged, TapClass::ContinuousPointer),
    (CGEventType::ScrollWheel, TapClass::ContinuousPointer),
];

fn tap_class(etype: CGEventType) -> Option<TapClass> {
    TAP_EVENTS
        .iter()
        .find(|&&(listed, _)| listed as u32 == etype as u32)
        .map(|&(_, class)| class)
}

fn is_tap_disabled(etype: CGEventType) -> bool {
    matches!(
        etype,
        CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput
    )
}

fn classify(etype: CGEventType, secure: SecureInputState) -> Classification {
    match tap_class(etype) {
        Some(TapClass::KeyPress | TapClass::OtherKey) => {
            if secure == SecureInputState::Inactive {
                Classification::Record(Modality::Keystroke)
            } else {
                Classification::SecureSuppressed
            }
        }
        Some(TapClass::DiscretePointer | TapClass::ContinuousPointer) => {
            Classification::Record(Modality::Pointer)
        }
        None => Classification::Ignore,
    }
}

fn events_of_interest() -> Vec<CGEventType> {
    TAP_EVENTS.iter().map(|&(etype, _)| etype).collect()
}

const TAP_LOCATION: CGEventTapLocation = CGEventTapLocation::HID;
const TAP_PLACEMENT: CGEventTapPlacement = CGEventTapPlacement::HeadInsertEventTap;
const TAP_OPTIONS: CGEventTapOptions = CGEventTapOptions::ListenOnly;

fn create_tap(
    callback: impl Fn(CGEventTapProxy, CGEventType, &CGEvent) -> CallbackResult + Send + 'static,
) -> Result<CGEventTap<'static>, ()> {
    CGEventTap::new(
        TAP_LOCATION,
        TAP_PLACEMENT,
        TAP_OPTIONS,
        events_of_interest(),
        callback,
    )
}

struct TapContext {
    sink: SinkHandle,
    foreground: ForegroundReader,
    violations: SecureInputViolations,
    port: TapPort,
    secure: fn() -> SecureInputState,
}

fn dispatch_tap_event(ctx: &TapContext, etype: CGEventType, secure: SecureInputState) {
    match classify(etype, secure) {
        Classification::Record(modality) => {
            let _ = emit_metadata_only(
                &ctx.sink,
                modality,
                ctx.foreground.get(),
                Unpersisted::EMPTY,
            );
        }
        Classification::SecureSuppressed => ctx.violations.record(),
        Classification::Ignore => {}
    }
}

fn on_tap_event(ctx: &TapContext, etype: CGEventType) {
    if is_tap_disabled(etype) {
        ctx.port.reenable();
        return;
    }
    dispatch_tap_event(ctx, etype, (ctx.secure)());
}

fn current_secure_state() -> SecureInputState {
    if secure_input_active() {
        SecureInputState::Active
    } else {
        SecureInputState::Inactive
    }
}

fn install_tap(ctx: TapContext) -> Result<(CFRunLoopSource, CGEventTap<'static>), CaptureError> {
    let port = ctx.port.clone();
    let tap = create_tap(move |_proxy, etype, _event| {
        on_tap_event(&ctx, etype);
        CallbackResult::Keep
    })
    .map_err(|()| CaptureError::PermissionDenied(Modality::Keystroke))?;

    port.store(tap.mach_port().as_concrete_TypeRef() as *mut c_void);

    let source = tap
        .mach_port()
        .create_runloop_source(0)
        .map_err(|()| CaptureError::Start("could not create run loop source".into()))?;
    let run_loop = CFRunLoop::get_current();
    // SAFETY: `kCFRunLoopCommonModes` is a constant CFString that
    // CoreFoundation exports and never writes; reading it has no effect.
    run_loop.add_source(&source, unsafe { kCFRunLoopCommonModes });
    tap.enable();
    Ok((source, tap))
}

struct TapThread {
    run_loop: CFRunLoop,
    thread: JoinHandle<()>,
}

impl TapThread {
    fn spawn<K: 'static>(
        install: impl FnOnce() -> Result<K, CaptureError> + Send + 'static,
    ) -> Result<Self, CaptureError> {
        let (loop_tx, loop_rx) = channel::<Result<CFRunLoop, CaptureError>>();
        let thread = std::thread::Builder::new()
            .name("halvo-input-tap".into())
            .spawn(move || {
                let installed = match install() {
                    Ok(installed) => installed,
                    Err(e) => {
                        let _ = loop_tx.send(Err(e));
                        return;
                    }
                };
                let _ = loop_tx.send(Ok(CFRunLoop::get_current()));
                CFRunLoop::run_current();
                drop(installed);
            })
            .map_err(|e| CaptureError::Start(format!("tap thread: {e}")))?;

        match loop_rx.recv() {
            Ok(Ok(run_loop)) => Ok(Self { run_loop, thread }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => {
                let _ = thread.join();
                Err(CaptureError::Start(
                    "tap thread stopped before reporting".into(),
                ))
            }
        }
    }

    fn stop(self) {
        let Self { run_loop, thread } = self;
        run_loop.stop();
        drop(run_loop);
        let _ = thread.join();
    }
}

#[cfg(test)]
impl TapThread {
    fn wait_until_its_run_loop_waits(&self) {
        let started = std::time::Instant::now();
        // SAFETY: `run_loop` is a retained reference to the tap thread's run
        // loop, and CFRunLoopIsWaiting only reads the state of that loop.
        while unsafe {
            core_foundation::runloop::CFRunLoopIsWaiting(self.run_loop.as_concrete_TypeRef())
        } == 0
        {
            assert!(
                started.elapsed() < std::time::Duration::from_secs(5),
                "the tap thread's run loop did not start within 5 s"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
}

/// The keystroke + pointer source.
pub struct InputSource {
    foreground: ForegroundReader,
    violations: SecureInputViolations,
    tap: Option<TapThread>,
}

impl InputSource {
    pub fn new(foreground: ForegroundReader) -> Self {
        Self {
            foreground,
            violations: SecureInputViolations::new(),
            tap: None,
        }
    }

    pub fn violations_handle(&self) -> SecureInputViolations {
        self.violations.clone()
    }
}

impl CaptureSource for InputSource {
    fn modality(&self) -> Modality {
        Modality::Keystroke
    }

    fn start(&mut self, sink: SinkHandle) -> Result<(), CaptureError> {
        let ctx = TapContext {
            sink,
            foreground: self.foreground.clone(),
            violations: self.violations.clone(),
            port: TapPort::unset(),
            secure: current_secure_state,
        };
        self.tap = Some(TapThread::spawn(move || install_tap(ctx))?);
        Ok(())
    }

    fn stop(&mut self) {
        if let Some(tap) = self.tap.take() {
            tap.stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::macos::foreground::foreground_state;
    use crate::secure_input_macos::hold_secure_input;
    use crate::secure_input_macos::test_support::{one_hold_at_a_time, session_reading_within};
    use crate::sink;
    use core_foundation::date::CFDate;
    use core_foundation::runloop::{CFRunLoopTimer, CFRunLoopTimerRef};
    use std::cell::RefCell;
    use std::sync::mpsc::RecvTimeoutError;
    use std::time::Duration;

    #[test]
    fn key_events_under_secure_input_are_suppressed_not_recorded() {
        for etype in [
            CGEventType::KeyDown,
            CGEventType::KeyUp,
            CGEventType::FlagsChanged,
        ] {
            assert_eq!(
                classify(etype, SecureInputState::Active),
                Classification::SecureSuppressed,
                "a key event must not record while secure input is active"
            );
        }
    }

    #[test]
    fn key_events_record_a_keystroke_when_secure_input_is_inactive() {
        assert_eq!(
            classify(CGEventType::KeyDown, SecureInputState::Inactive),
            Classification::Record(Modality::Keystroke)
        );
    }

    #[test]
    fn pointer_events_record_regardless_of_secure_input() {
        for secure in [SecureInputState::Active, SecureInputState::Inactive] {
            assert_eq!(
                classify(CGEventType::MouseMoved, secure),
                Classification::Record(Modality::Pointer)
            );
            assert_eq!(
                classify(CGEventType::LeftMouseDown, secure),
                Classification::Record(Modality::Pointer)
            );
        }
    }

    #[test]
    fn disable_events_are_ignored_by_classification() {
        for secure in [SecureInputState::Active, SecureInputState::Inactive] {
            assert_eq!(
                classify(CGEventType::TapDisabledByTimeout, secure),
                Classification::Ignore
            );
            assert_eq!(
                classify(CGEventType::TapDisabledByUserInput, secure),
                Classification::Ignore
            );
        }
        assert!(is_tap_disabled(CGEventType::TapDisabledByTimeout));
        assert!(is_tap_disabled(CGEventType::TapDisabledByUserInput));
        assert!(!is_tap_disabled(CGEventType::KeyDown));
    }

    const EVERY_EVENT_TYPE: [CGEventType; 19] = [
        CGEventType::Null,
        CGEventType::LeftMouseDown,
        CGEventType::LeftMouseUp,
        CGEventType::RightMouseDown,
        CGEventType::RightMouseUp,
        CGEventType::MouseMoved,
        CGEventType::LeftMouseDragged,
        CGEventType::RightMouseDragged,
        CGEventType::KeyDown,
        CGEventType::KeyUp,
        CGEventType::FlagsChanged,
        CGEventType::ScrollWheel,
        CGEventType::TabletPointer,
        CGEventType::TabletProximity,
        CGEventType::OtherMouseDown,
        CGEventType::OtherMouseUp,
        CGEventType::OtherMouseDragged,
        CGEventType::TapDisabledByTimeout,
        CGEventType::TapDisabledByUserInput,
    ];

    #[test]
    fn the_tap_listens_for_exactly_the_types_that_it_records_or_suppresses() {
        let mask = events_of_interest();
        for etype in EVERY_EVENT_TYPE {
            let masked = mask.iter().any(|&listed| listed as u32 == etype as u32);
            for secure in [SecureInputState::Active, SecureInputState::Inactive] {
                let handled = classify(etype, secure) != Classification::Ignore;
                assert_eq!(
                    masked, handled,
                    "{etype:?} under {secure:?}: masked {masked}, recorded or suppressed {handled}"
                );
            }
        }
    }

    #[test]
    fn the_tap_mask_is_0x0e401cfe() {
        let mask = events_of_interest()
            .iter()
            .fold(0u64, |mask, &etype| mask | 1 << (etype as u64));
        assert_eq!(mask, 0x0E40_1CFE);
    }

    fn context(read_secure: fn() -> SecureInputState) -> (TapContext, sink::SinkWriter) {
        let (handle, writer) = sink::channel(8);
        let (_writer, reader) = foreground_state();
        let ctx = TapContext {
            sink: handle,
            foreground: reader,
            violations: SecureInputViolations::new(),
            port: TapPort::unset(),
            secure: read_secure,
        };
        (ctx, writer)
    }

    #[test]
    fn a_key_event_read_as_secure_counts_one_suppression_and_records_nothing() {
        let (ctx, writer) = context(|| SecureInputState::Active);
        on_tap_event(&ctx, CGEventType::KeyDown);
        assert_eq!(ctx.violations.count(), 1);
        drop(ctx);
        assert!(writer.recv().is_none(), "a keystroke was recorded");
    }

    #[test]
    fn the_production_reader_reads_a_real_hold_as_active() {
        let _one = one_hold_at_a_time();
        let hold = hold_secure_input();
        if !session_reading_within(true, Duration::from_secs(2)) {
            eprintln!(
                "SKIP the_production_reader_reads_a_real_hold_as_active: the session shows no \
                 secure-input pid while this process holds secure input"
            );
            return;
        }
        assert_eq!(current_secure_state(), SecureInputState::Active);
        drop(hold);
    }

    #[test]
    fn the_tap_is_created_listen_only() {
        assert_eq!(TAP_OPTIONS as u32, 1);
    }

    thread_local! {
        static ENABLE_CALLS: RefCell<Vec<(usize, bool)>> = const { RefCell::new(Vec::new()) };
    }

    extern "C" fn record_enable(tap: *mut c_void, enable: bool) {
        ENABLE_CALLS.with(|calls| calls.borrow_mut().push((tap as usize, enable)));
    }

    #[test]
    fn reenable_passes_true_to_the_enable_function() {
        let port = TapPort::with_enable(record_enable);
        port.reenable();
        let tap = std::ptr::NonNull::<c_void>::dangling().as_ptr();
        port.store(tap);
        port.reenable();
        ENABLE_CALLS.with(|calls| assert_eq!(*calls.borrow(), [(tap as usize, true)]));
    }

    extern "C" fn never_fires(_timer: CFRunLoopTimerRef, _info: *mut c_void) {}

    #[test]
    fn stop_ends_the_tap_thread_within_one_second() {
        for cycle in 0..3 {
            let (alive_tx, alive_rx) = channel::<()>();
            let tap = TapThread::spawn(move || {
                let timer = CFRunLoopTimer::new(
                    CFDate::now().abs_time() + 3600.0,
                    0.0,
                    0,
                    0,
                    never_fires,
                    std::ptr::null_mut(),
                );
                // SAFETY: `kCFRunLoopCommonModes` is a constant CFString that
                // CoreFoundation exports and never writes; reading it has no effect.
                CFRunLoop::get_current().add_timer(&timer, unsafe { kCFRunLoopCommonModes });
                Ok((timer, alive_tx))
            })
            .expect("the tap thread starts");
            tap.wait_until_its_run_loop_waits();
            tap.stop();
            assert_eq!(
                alive_rx.recv_timeout(Duration::from_secs(1)),
                Err(RecvTimeoutError::Disconnected),
                "cycle {cycle}: the tap thread still runs 1 s after stop"
            );
        }
    }
}

#[cfg(test)]
mod granted_tests {
    use super::*;
    use crate::macos::foreground::foreground_state;
    use crate::sink;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct TapInformation {
        event_tap_id: u32,
        tap_point: u32,
        options: u32,
        events_of_interest: u64,
        tapping_process: libc::pid_t,
        process_being_tapped: libc::pid_t,
        enabled: bool,
        min_usec_latency: f32,
        avg_usec_latency: f32,
        max_usec_latency: f32,
    }

    const NO_TAP: TapInformation = TapInformation {
        event_tap_id: 0,
        tap_point: 0,
        options: 0,
        events_of_interest: 0,
        tapping_process: 0,
        process_being_tapped: 0,
        enabled: false,
        min_usec_latency: 0.0,
        avg_usec_latency: 0.0,
        max_usec_latency: 0.0,
    };

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGGetEventTapList(max: u32, list: *mut TapInformation, count: *mut u32) -> i32;
        fn CGEventTapIsEnabled(tap: *mut c_void) -> bool;
    }

    const PROC_PIDLISTTHREADS: libc::c_int = 6;

    fn own_taps() -> Vec<TapInformation> {
        let mut list = [NO_TAP; 256];
        let mut count = 0u32;
        // SAFETY: `list` holds 256 entries, the maximum passed, and `count` is a
        // valid place for the number of entries written.
        let err = unsafe { CGGetEventTapList(256, list.as_mut_ptr(), &mut count) };
        assert_eq!(err, 0, "CGGetEventTapList failed");
        let own = std::process::id() as libc::pid_t;
        list[..(count as usize).min(list.len())]
            .iter()
            .copied()
            .filter(|tap| tap.tapping_process == own)
            .collect()
    }

    fn thread_name(thread_id: u64) -> Option<String> {
        // SAFETY: `proc_threadinfo` holds integers and a char array only, for
        // which all-zero bytes are a valid value.
        let mut info: libc::proc_threadinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<libc::proc_threadinfo>() as libc::c_int;
        // SAFETY: `info` is a writable `proc_threadinfo` of `size` bytes.
        let got = unsafe {
            libc::proc_pidinfo(
                std::process::id() as libc::c_int,
                libc::PROC_PIDTHREADINFO,
                thread_id,
                (&mut info as *mut libc::proc_threadinfo).cast(),
                size,
            )
        };
        (got == size).then(|| {
            info.pth_name
                .iter()
                .take_while(|&&c| c != 0)
                .map(|&c| c as u8 as char)
                .collect()
        })
    }

    fn tap_threads() -> usize {
        let mut ids = [0u64; 1024];
        // SAFETY: `ids` is a writable buffer of the byte size passed.
        let bytes = unsafe {
            libc::proc_pidinfo(
                std::process::id() as libc::c_int,
                PROC_PIDLISTTHREADS,
                0,
                ids.as_mut_ptr().cast(),
                std::mem::size_of_val(&ids) as libc::c_int,
            )
        };
        assert!(bytes > 0, "proc_pidinfo could not list the threads");
        ids[..bytes as usize / std::mem::size_of::<u64>()]
            .iter()
            .filter(|&&id| thread_name(id).as_deref() == Some("halvo-input-tap"))
            .count()
    }

    fn drained_sink() -> (SinkHandle, std::thread::JoinHandle<()>) {
        let (handle, writer) = sink::channel(64);
        let drain = std::thread::spawn(move || while writer.recv().is_some() {});
        (handle, drain)
    }

    #[test]
    #[ignore = "needs Input Monitoring for the process that runs the tests"]
    fn the_live_tap_is_listen_only() {
        let (handle, drain) = drained_sink();
        let (_writer, reader) = foreground_state();
        let mut source = InputSource::new(reader);
        source.start(handle).expect("the input source starts");
        let taps = own_taps();
        source
            .tap
            .as_ref()
            .expect("a started source holds its tap thread")
            .wait_until_its_run_loop_waits();
        source.stop();
        drain.join().expect("the drain thread ends");
        assert_eq!(taps.len(), 1, "taps of this process while the source runs");
        assert_eq!(taps[0].options, 1, "the live tap is not listen-only");
    }

    #[test]
    #[ignore = "needs Input Monitoring for the process that runs the tests"]
    fn reenable_turns_a_disabled_tap_back_on() {
        let (handle, drain) = drained_sink();
        let (_writer, reader) = foreground_state();
        let port = TapPort::unset();
        let ctx = TapContext {
            sink: handle,
            foreground: reader,
            violations: SecureInputViolations::new(),
            port: port.clone(),
            secure: current_secure_state,
        };
        let tap = TapThread::spawn(move || install_tap(ctx)).expect("the tap starts");
        let live = port.port.load(Ordering::Relaxed);
        // SAFETY: `live` is the mach port of the tap that the tap thread keeps
        // until `stop` below.
        let enabled_at_start = unsafe { CGEventTapIsEnabled(live) };
        // SAFETY: as above.
        unsafe { CGEventTapEnable(live, false) };
        // SAFETY: as above.
        let enabled_after_disable = unsafe { CGEventTapIsEnabled(live) };
        port.reenable();
        // SAFETY: as above.
        let enabled_after_reenable = unsafe { CGEventTapIsEnabled(live) };
        tap.wait_until_its_run_loop_waits();
        tap.stop();
        drain.join().expect("the drain thread ends");
        assert_eq!(
            (
                enabled_at_start,
                enabled_after_disable,
                enabled_after_reenable
            ),
            (true, false, true)
        );
    }

    #[test]
    #[ignore = "needs Input Monitoring for the process that runs the tests"]
    fn start_then_stop_leaves_no_tap_and_no_tap_thread() {
        let (handle, drain) = drained_sink();
        let (_writer, reader) = foreground_state();
        let mut source = InputSource::new(reader);
        source.start(handle).expect("the input source starts");
        let running = (own_taps().len(), tap_threads());
        source
            .tap
            .as_ref()
            .expect("a started source holds its tap thread")
            .wait_until_its_run_loop_waits();
        source.stop();
        drain.join().expect("the drain thread ends");
        assert_eq!(
            running,
            (1, 1),
            "taps and tap threads while the source runs"
        );
        assert_eq!((own_taps().len(), tap_threads()), (0, 0));
    }
}

#[cfg(test)]
mod secure_input_invariant {
    use super::*;
    use crate::sink;

    fn context(capacity: usize) -> (TapContext, sink::SinkWriter) {
        let (handle, writer) = sink::channel(capacity);
        let (_writer, reader) = crate::macos::foreground::foreground_state();
        let ctx = TapContext {
            sink: handle,
            foreground: reader,
            violations: SecureInputViolations::new(),
            port: TapPort::unset(),
            secure: current_secure_state,
        };
        (ctx, writer)
    }

    #[test]
    fn a_key_event_under_secure_input_records_nothing_and_is_a_violation() {
        let (ctx, writer) = context(8);
        for etype in [
            CGEventType::KeyDown,
            CGEventType::KeyUp,
            CGEventType::FlagsChanged,
        ] {
            dispatch_tap_event(&ctx, etype, SecureInputState::Active);
        }
        assert_eq!(ctx.violations.verify_none(), Err(SecureInputViolated(3)));
        drop(ctx);
        assert!(
            writer.recv().is_none(),
            "a keystroke was recorded while secure input was active"
        );
    }

    #[test]
    fn a_violation_fails_the_zero_check_and_a_clean_run_passes_it() {
        let (ctx, _writer) = context(8);
        assert_eq!(ctx.violations.verify_none(), Ok(()));
        dispatch_tap_event(&ctx, CGEventType::KeyDown, SecureInputState::Active);
        assert_eq!(ctx.violations.verify_none(), Err(SecureInputViolated(1)));
        assert_eq!(ctx.violations.count(), 1);
    }

    #[test]
    fn pointer_events_record_under_secure_input_without_a_violation() {
        let (ctx, writer) = context(8);
        dispatch_tap_event(&ctx, CGEventType::MouseMoved, SecureInputState::Active);
        dispatch_tap_event(&ctx, CGEventType::LeftMouseDown, SecureInputState::Active);
        assert_eq!(ctx.violations.verify_none(), Ok(()));
        drop(ctx);
        let recorded: Vec<Modality> = std::iter::from_fn(|| writer.recv())
            .map(|e| e.modality)
            .collect();
        assert_eq!(recorded, [Modality::Pointer, Modality::Pointer]);
    }

    #[test]
    fn a_key_event_without_secure_input_records_a_keystroke() {
        let (ctx, writer) = context(8);
        dispatch_tap_event(&ctx, CGEventType::KeyDown, SecureInputState::Inactive);
        assert_eq!(ctx.violations.verify_none(), Ok(()));
        drop(ctx);
        assert_eq!(writer.recv().map(|e| e.modality), Some(Modality::Keystroke));
    }
}
