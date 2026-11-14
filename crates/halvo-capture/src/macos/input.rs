use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use core_foundation::base::TCFType;
use core_foundation::date::CFDate;
use core_foundation::runloop::{
    kCFRunLoopCommonModes, kCFRunLoopDefaultMode, CFRunLoop, CFRunLoopSource, CFRunLoopTimer,
    CFRunLoopTimerContext, CFRunLoopTimerInvalidate, CFRunLoopTimerRef,
};
use core_graphics::event::{
    CGEvent, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventTapProxy, CGEventType, CallbackResult,
};

use crate::event::Modality;
use crate::macos::foreground::{emit_metadata_only, ForegroundReader, Unpersisted};
use crate::macos::source_failure;
use crate::secure_input::SecureInputState;
use crate::secure_input_macos::{
    hold_secure_input, secure_input_active, SecureInputHold, SecureInputRefused,
};
use crate::sink::SinkHandle;
use crate::source::{
    CaptureError, CaptureSource, HeldSamples, SourceContext, SourceCounters, SourceHealth,
    SourceState, Started, StopCause, SuppressedKeys,
};

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventTapEnable(tap: *mut c_void, enable: bool);
    fn CGPreflightListenEventAccess() -> bool;
}

pub const INPUT_STOP_DEADLINE: Duration = Duration::from_secs(1);

const RUN_SLICE: Duration = Duration::from_millis(100);

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
            // SAFETY: outside tests `port` is set only by `SystemTapMaker::enable`,
            // to the mach port of the tap that it returns inside a
            // `PortClearedFirst`, whose Drop sets it back to null before that
            // tap drops. So a port that is not null belongs to a tap that is alive,
            // as long as the load above and this call do not straddle the drop:
            // the one caller outside tests is the tap's callback, which runs on
            // the tap thread, and that thread drops the tap only after its run
            // loop returned. `enable` is `CGEventTapEnable`, set by `unset`, or a
            // test's function that only reads its arguments.
            unsafe { (self.enable)(port, true) };
        }
    }
}

/// A tap with the port that names it. Its Drop clears the port, and only then
/// does the tap drop, with the fields.
struct PortClearedFirst<T> {
    port: TapPort,
    tap: T,
}

impl<T> Drop for PortClearedFirst<T> {
    fn drop(&mut self) {
        self.port.store(std::ptr::null_mut());
        let _ = &self.tap;
    }
}

/// The input source's figures.
#[derive(Clone, Default)]
pub struct InputCounters(Arc<InputFigures>);

#[derive(Default)]
struct InputFigures {
    suppressed_keys: AtomicU64,
    tap_disabled_timeout: AtomicU64,
    tap_disabled_user_input: AtomicU64,
    late_events: AtomicU64,
    max_lag_ms: AtomicU64,
    pointer_coalesced: AtomicU64,
    keys_withheld: AtomicBool,
    secure_released_keystrokes: AtomicU64,
    secure_held_keystrokes: AtomicU64,
    secure_held_suppressed_down: AtomicU64,
    secure_held_suppressed_up: AtomicU64,
    secure_held_suppressed_modifier: AtomicU64,
    secure_input_held: AtomicU64,
    secure_input_samples: AtomicU64,
    secure_window_pointer: AtomicU64,
    secure_window_keys_typed: AtomicBool,
}

impl InputCounters {
    pub fn new() -> Self {
        Self::default()
    }

    fn count_suppressed_key(&self) {
        self.0.suppressed_keys.fetch_add(1, Ordering::Relaxed);
    }

    /// Key events that reached the tap while secure input was active. Each was
    /// suppressed and never emitted.
    pub fn suppressed_keys(&self) -> u64 {
        self.0.suppressed_keys.load(Ordering::Relaxed)
    }

    pub fn figures(&self) -> SourceCounters {
        let f = &self.0;
        let read = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
        SourceCounters {
            tap_disabled_timeout: read(&f.tap_disabled_timeout),
            tap_disabled_user_input: read(&f.tap_disabled_user_input),
            late_events: read(&f.late_events),
            max_lag_ms: read(&f.max_lag_ms),
            secure_input_keystrokes: read(&f.suppressed_keys),
            title_read_errors: 0,
            pointer_coalesced: read(&f.pointer_coalesced),
            keys_withheld: f.keys_withheld.load(Ordering::Relaxed),
            secure_released_keystrokes: read(&f.secure_released_keystrokes),
            secure_held_keystrokes: read(&f.secure_held_keystrokes),
            secure_held_suppressed: SuppressedKeys {
                down: read(&f.secure_held_suppressed_down),
                up: read(&f.secure_held_suppressed_up),
                modifier: read(&f.secure_held_suppressed_modifier),
            },
            secure_input_held: HeldSamples {
                held: read(&f.secure_input_held),
                taken: read(&f.secure_input_samples),
            },
            secure_window_pointer: read(&f.secure_window_pointer),
            secure_window_keys_typed: f.secure_window_keys_typed.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Classification {
    Record(Modality),
    Suppress,
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
                Classification::Suppress
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

struct TapSettings {
    location: CGEventTapLocation,
    placement: CGEventTapPlacement,
    options: CGEventTapOptions,
    events: Vec<CGEventType>,
    secure: fn() -> SecureInputState,
}

fn tap_settings() -> TapSettings {
    TapSettings {
        location: TAP_LOCATION,
        placement: TAP_PLACEMENT,
        options: TAP_OPTIONS,
        events: events_of_interest(),
        secure: current_secure_state,
    }
}

fn create_tap(
    settings: TapSettings,
    callback: impl Fn(CGEventTapProxy, CGEventType, &CGEvent) -> CallbackResult + Send + 'static,
) -> Result<CGEventTap<'static>, ()> {
    CGEventTap::new(
        settings.location,
        settings.placement,
        settings.options,
        settings.events,
        callback,
    )
}

trait TapMaker {
    type Tap: 'static;
    type Enabled: 'static;

    fn make<F>(self, settings: TapSettings, callback: F) -> Result<Self::Tap, ()>
    where
        F: Fn(CGEventTapProxy, CGEventType, &CGEvent) -> CallbackResult + Send + 'static;

    fn listen_denied() -> bool;

    fn enable(tap: Self::Tap, port: TapPort) -> Result<Self::Enabled, CaptureError>;
}

struct SystemTapMaker;

impl TapMaker for SystemTapMaker {
    type Tap = CGEventTap<'static>;
    type Enabled = PortClearedFirst<(CFRunLoopSource, CGEventTap<'static>)>;

    fn make<F>(self, settings: TapSettings, callback: F) -> Result<Self::Tap, ()>
    where
        F: Fn(CGEventTapProxy, CGEventType, &CGEvent) -> CallbackResult + Send + 'static,
    {
        create_tap(settings, callback)
    }

    fn listen_denied() -> bool {
        // SAFETY: a parameterless predicate of the system with no side effect.
        !unsafe { CGPreflightListenEventAccess() }
    }

    fn enable(tap: Self::Tap, port: TapPort) -> Result<Self::Enabled, CaptureError> {
        let source = tap
            .mach_port()
            .create_runloop_source(0)
            .map_err(|()| CaptureError::Start("the tap's run loop source was not made".into()))?;

        port.store(tap.mach_port().as_concrete_TypeRef() as *mut c_void);

        let run_loop = CFRunLoop::get_current();
        // SAFETY: `kCFRunLoopCommonModes` is a constant CFString that
        // CoreFoundation exports and never writes.
        run_loop.add_source(&source, unsafe { kCFRunLoopCommonModes });
        tap.enable();
        Ok(PortClearedFirst {
            port,
            tap: (source, tap),
        })
    }
}

fn tap_refusal(denied: bool) -> CaptureError {
    if denied {
        CaptureError::PermissionDenied(Modality::Keystroke)
    } else {
        CaptureError::Os {
            stage: "event_tap_create",
            domain: "CoreGraphics",
            code: 0,
        }
    }
}

struct TapContext {
    sink: SinkHandle,
    foreground: ForegroundReader,
    counters: InputCounters,
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
        Classification::Suppress => ctx.counters.count_suppressed_key(),
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

fn tap_context(
    sink: SinkHandle,
    foreground: ForegroundReader,
    counters: InputCounters,
    settings: &TapSettings,
) -> TapContext {
    TapContext {
        sink,
        foreground,
        counters,
        port: TapPort::unset(),
        secure: settings.secure,
    }
}

fn tap_callback(
    ctx: TapContext,
) -> impl Fn(CGEventTapProxy, CGEventType, &CGEvent) -> CallbackResult + Send + 'static {
    move |_proxy, etype, _event: &CGEvent| {
        on_tap_event(&ctx, etype);
        CallbackResult::Keep
    }
}

fn install_tap<M: TapMaker>(
    ctx: TapContext,
    settings: TapSettings,
    maker: M,
) -> Result<M::Enabled, CaptureError> {
    let port = ctx.port.clone();
    let tap = maker
        .make(settings, tap_callback(ctx))
        .map_err(|()| tap_refusal(M::listen_denied()))?;
    M::enable(tap, port)
}

/// When the tap thread holds secure input: the same state that a focused
/// password field asserts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoldPlan {
    /// The tap thread never holds secure input.
    Never,
    /// The tap thread holds secure input from this long after its tap is made
    /// until its run loop has ended. Zero holds it before the tap is enabled.
    After(Duration),
}

type Hold = fn() -> Result<SecureInputHold, SecureInputRefused>;

struct HoldSlot {
    hold: Hold,
    held: Option<SecureInputHold>,
    refusal: Arc<Mutex<Option<SecureInputRefused>>>,
}

impl HoldSlot {
    fn take(&mut self) {
        match (self.hold)() {
            Ok(held) => self.held = Some(held),
            Err(refused) => {
                *self.refusal.lock().unwrap_or_else(PoisonError::into_inner) = Some(refused)
            }
        }
    }
}

/// A hold slot at a fixed address with the timer that holds that address. Its
/// Drop invalidates the timer, and only then does the slot drop.
struct TimedHold {
    slot: Box<HoldSlot>,
    timer: Option<CFRunLoopTimer>,
    invalidate: fn(&CFRunLoopTimer),
}

impl TimedHold {
    fn new(slot: HoldSlot) -> Self {
        Self {
            slot: Box::new(slot),
            timer: None,
            invalidate,
        }
    }

    fn take_now(&mut self) {
        self.slot.take();
    }

    fn take_after(&mut self, delay: Duration) {
        let info = (&mut *self.slot as *mut HoldSlot).cast::<c_void>();
        let timer = timer_at(delay, hold_timer_fired, info);
        add_to_this_run_loop(&timer);
        self.timer = Some(timer);
    }
}

impl Drop for TimedHold {
    fn drop(&mut self) {
        if let Some(timer) = self.timer.take() {
            (self.invalidate)(&timer);
        }
    }
}

extern "C" fn hold_timer_fired(_timer: CFRunLoopTimerRef, info: *mut c_void) {
    // SAFETY: `info` is the boxed `HoldSlot` of a `TimedHold`, whose box keeps
    // it at one address and whose Drop invalidates this timer before the slot
    // drops; the timer fires on the tap thread alone, which owns the TimedHold
    // and touches the slot only outside the run loop, so no other reference to
    // the slot exists.
    let slot = unsafe { &mut *info.cast::<HoldSlot>() };
    slot.take();
}

extern "C" fn idle_timer_fired(_timer: CFRunLoopTimerRef, _info: *mut c_void) {}

fn timer_at(
    delay: Duration,
    callout: extern "C" fn(CFRunLoopTimerRef, *mut c_void),
    info: *mut c_void,
) -> CFRunLoopTimer {
    let mut context = CFRunLoopTimerContext {
        version: 0,
        info,
        retain: None,
        release: None,
        copyDescription: None,
    };
    CFRunLoopTimer::new(
        CFDate::now().abs_time() + delay.as_secs_f64(),
        0.0,
        0,
        0,
        callout,
        &mut context,
    )
}

fn add_to_this_run_loop(timer: &CFRunLoopTimer) {
    // SAFETY: `kCFRunLoopCommonModes` is a constant CFString that
    // CoreFoundation exports and never writes.
    CFRunLoop::get_current().add_timer(timer, unsafe { kCFRunLoopCommonModes });
}

fn invalidate(timer: &CFRunLoopTimer) {
    // SAFETY: `timer` is a live timer; invalidating it only removes it from
    // every run loop, so its callout never runs again.
    unsafe { CFRunLoopTimerInvalidate(timer.as_concrete_TypeRef()) };
}

fn spawn_failure(_error: std::io::Error) -> CaptureError {
    CaptureError::Start("the tap thread was not started".into())
}

struct TapThread {
    run_loop: CFRunLoop,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl TapThread {
    fn spawn<K: 'static>(
        install: impl FnOnce() -> Result<K, CaptureError> + Send + 'static,
        plan: HoldPlan,
        hold: Hold,
        refusal: Arc<Mutex<Option<SecureInputRefused>>>,
        before_run: impl FnOnce() + Send + 'static,
    ) -> Result<Self, CaptureError> {
        let stop = Arc::new(AtomicBool::new(false));
        let (loop_tx, loop_rx) = channel::<Result<CFRunLoop, CaptureError>>();
        let thread = {
            let stop = stop.clone();
            std::thread::Builder::new()
                .name("halvo-input-tap".into())
                .spawn(move || {
                    let mut slot = TimedHold::new(HoldSlot {
                        hold,
                        held: None,
                        refusal,
                    });
                    if plan == HoldPlan::After(Duration::ZERO) {
                        slot.take_now();
                    }
                    let installed = match install() {
                        Ok(installed) => installed,
                        Err(e) => {
                            let _ = loop_tx.send(Err(e));
                            return;
                        }
                    };
                    if let HoldPlan::After(delay) = plan {
                        if !delay.is_zero() {
                            slot.take_after(delay);
                        }
                    }
                    let idle = timer_at(
                        Duration::from_secs(86_400 * 365),
                        idle_timer_fired,
                        std::ptr::null_mut(),
                    );
                    add_to_this_run_loop(&idle);
                    let _ = loop_tx.send(Ok(CFRunLoop::get_current()));
                    before_run();
                    while !stop.load(Ordering::SeqCst) {
                        // SAFETY: `kCFRunLoopDefaultMode` is a constant CFString
                        // that CoreFoundation exports and never writes.
                        CFRunLoop::run_in_mode(unsafe { kCFRunLoopDefaultMode }, RUN_SLICE, false);
                    }
                    drop(installed);
                    drop(slot);
                    invalidate(&idle);
                })
                .map_err(spawn_failure)?
        };

        match loop_rx.recv() {
            Ok(Ok(run_loop)) => Ok(Self {
                run_loop,
                stop,
                thread: Some(thread),
            }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => {
                let _ = thread.join();
                Err(CaptureError::ThreadEnded)
            }
        }
    }

    fn request_stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.run_loop.stop();
    }

    fn ended(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }

    fn stop_within(&mut self, deadline: Duration) -> Result<(), CaptureError> {
        let ended_before = !self.stop.load(Ordering::SeqCst) && self.ended();
        self.request_stop();
        let called = Instant::now();
        while !self.ended() {
            if called.elapsed() >= deadline {
                return Err(CaptureError::Timeout {
                    stage: "input_tap_join",
                    ms: deadline.as_millis() as u64,
                });
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        let joined = self.thread.take().map(JoinHandle::join);
        match (ended_before, joined) {
            (false, Some(Ok(()))) => Ok(()),
            _ => Err(CaptureError::ThreadEnded),
        }
    }
}

impl Drop for TapThread {
    fn drop(&mut self) {
        self.request_stop();
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

pub struct InputSource {
    foreground: ForegroundReader,
    counters: InputCounters,
    plan: HoldPlan,
    hold: Hold,
    refusal: Arc<Mutex<Option<SecureInputRefused>>>,
    tap: Option<TapThread>,
    failure: Option<CaptureError>,
    stopped: bool,
}

impl InputSource {
    pub fn new(foreground: ForegroundReader, counters: InputCounters, plan: HoldPlan) -> Self {
        Self {
            foreground,
            counters,
            plan,
            hold: hold_secure_input,
            refusal: Arc::new(Mutex::new(None)),
            tap: None,
            failure: None,
            stopped: false,
        }
    }

    /// The status with which the system refused the hold of the plan, once the
    /// tap thread asked for it.
    pub fn hold_refusal(&self) -> Option<SecureInputRefused> {
        *self.refusal.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn begin_with<M: TapMaker + Send + 'static>(
        &mut self,
        sink: SinkHandle,
        maker: M,
        before_run: impl FnOnce() + Send + 'static,
    ) -> Result<Started, CaptureError> {
        if self.tap.is_some() {
            return Err(CaptureError::AlreadyStarted);
        }
        *self.refusal.lock().unwrap_or_else(PoisonError::into_inner) = None;
        let settings = tap_settings();
        let ctx = tap_context(
            sink,
            self.foreground.clone(),
            self.counters.clone(),
            &settings,
        );
        let started = TapThread::spawn(
            move || install_tap(ctx, settings, maker),
            self.plan,
            self.hold,
            self.refusal.clone(),
            before_run,
        );
        match started {
            Ok(tap) => {
                self.tap = Some(tap);
                self.failure = None;
                self.stopped = false;
                Ok(Started::Running)
            }
            Err(e) => {
                self.failure = Some(e.clone());
                Err(e)
            }
        }
    }
}

impl CaptureSource for InputSource {
    fn begin(&mut self, ctx: SourceContext) -> Result<Started, CaptureError> {
        self.begin_with(ctx.sink().clone(), SystemTapMaker, || {})
    }

    fn shutdown(&mut self) -> Result<(), CaptureError> {
        let Some(tap) = self.tap.as_mut() else {
            return Ok(());
        };
        let answer = tap.stop_within(INPUT_STOP_DEADLINE);
        match &answer {
            Ok(()) => {
                self.tap = None;
                self.stopped = true;
            }
            Err(CaptureError::ThreadEnded) => {
                self.tap = None;
                self.failure = Some(CaptureError::ThreadEnded);
            }
            Err(_) => {}
        }
        answer
    }

    fn health(&self) -> SourceHealth {
        let state = match (&self.tap, &self.failure) {
            (Some(tap), _) if tap.ended() => source_failure(CaptureError::ThreadEnded),
            (Some(_), _) => SourceState::Running,
            (None, Some(error)) => source_failure(error.clone()),
            (None, None) if self.stopped => SourceState::Stopped(StopCause::Request),
            (None, None) => SourceState::NotReported,
        };
        SourceHealth {
            state,
            counters: self.counters.figures(),
        }
    }
}

impl Drop for InputSource {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

#[cfg(test)]
fn events_until_the_sink_closes(writer: &crate::sink::SinkWriter) -> Vec<Modality> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let mut kinds = Vec::new();
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        match writer.recv_timeout(left) {
            Ok(received) => kinds.push(received.event().modality),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return kinds,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => panic!(
                "the sink did not close within 2 s: something still holds the sink's handle \
                 (events so far: {kinds:?})"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::macos::foreground::foreground_state_for_test;
    use crate::secure_input_macos::test_support::{
        agreed_reading, one_hold_at_a_time, reads_on_within,
    };
    use crate::sink;
    use core_foundation::date::CFDate;
    use core_foundation::runloop::{CFRunLoopTimer, CFRunLoopTimerRef};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
    use std::cell::{Cell, RefCell};
    use std::sync::atomic::AtomicUsize;
    use std::sync::mpsc::RecvTimeoutError;
    use std::sync::{Mutex, PoisonError};
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
                Classification::Suppress,
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
        let (_writer, reader) = foreground_state_for_test();
        let ctx = TapContext {
            sink: handle,
            foreground: reader,
            counters: InputCounters::new(),
            port: TapPort::unset(),
            secure: read_secure,
        };
        (ctx, writer)
    }

    #[test]
    fn a_key_event_read_as_secure_counts_one_suppression_and_records_nothing() {
        let (ctx, writer) = context(|| SecureInputState::Active);
        on_tap_event(&ctx, CGEventType::KeyDown);
        assert_eq!(ctx.counters.suppressed_keys(), 1);
        drop(ctx);
        assert!(
            events_until_the_sink_closes(&writer).is_empty(),
            "a keystroke was recorded"
        );
    }

    #[test]
    fn the_production_reader_reads_a_real_hold_as_active() {
        let one = one_hold_at_a_time();
        let Some(hold) = one.hold_or_skip("the_production_reader_reads_a_real_hold_as_active")
        else {
            return;
        };
        assert!(
            reads_on_within(
                || current_secure_state() == SecureInputState::Active,
                Duration::from_secs(2)
            ),
            "during an accepted hold the production reader does not read Active"
        );
        drop(hold);
    }

    struct CountingMaker(Arc<AtomicUsize>);

    impl TapMaker for CountingMaker {
        type Tap = ();
        type Enabled = ();

        fn make<F>(self, _settings: TapSettings, _callback: F) -> Result<(), ()>
        where
            F: Fn(CGEventTapProxy, CGEventType, &CGEvent) -> CallbackResult + Send + 'static,
        {
            self.0.fetch_add(1, Ordering::SeqCst);
            Err(())
        }

        fn listen_denied() -> bool {
            false
        }

        fn enable(_tap: (), _port: TapPort) -> Result<(), CaptureError> {
            Ok(())
        }
    }

    #[test]
    fn a_key_down_read_off_does_not_answer_for_the_key_up_and_modifier_that_follow() {
        static READS: AtomicUsize = AtomicUsize::new(0);
        fn off_then_on() -> SecureInputState {
            if READS.fetch_add(1, Ordering::SeqCst) == 0 {
                SecureInputState::Inactive
            } else {
                SecureInputState::Active
            }
        }
        let (ctx, writer) = context(off_then_on);
        for etype in [
            CGEventType::KeyDown,
            CGEventType::KeyUp,
            CGEventType::FlagsChanged,
        ] {
            on_tap_event(&ctx, etype);
        }
        let suppressed = ctx.counters.suppressed_keys();
        drop(ctx);
        let recorded = events_until_the_sink_closes(&writer);
        assert_eq!(
            (recorded.len(), suppressed),
            (1, 2),
            "a key-down read off, then a key-up and a modifier read on: (recorded, suppressed)"
        );
        assert_eq!(READS.load(Ordering::SeqCst), 3, "reads of the secure state");
    }

    #[test]
    fn the_reader_that_the_tap_gets_asks_the_system_once_for_each_call() {
        const CALLS: u64 = 1024;
        let reader = tap_settings().secure;
        let before = crate::secure_input_macos::system_reads_on_this_thread();
        for _ in 0..CALLS {
            let _ = reader();
        }
        assert_eq!(
            crate::secure_input_macos::system_reads_on_this_thread() - before,
            CALLS,
            "calls that reached the system from this thread for {CALLS} reads"
        );
    }

    thread_local! {
        static STAND_IN_ANSWER: Cell<SecureInputState> =
            const { Cell::new(SecureInputState::Inactive) };
        static STAND_IN_ASKED: Cell<usize> = const { Cell::new(0) };
    }

    fn stand_in_reader() -> SecureInputState {
        STAND_IN_ASKED.with(|asked| asked.set(asked.get() + 1));
        STAND_IN_ANSWER.with(Cell::get)
    }

    struct EventOutcome {
        recorded: Vec<Modality>,
        suppressed: u64,
        asked: usize,
    }

    /// Hands each event, with the answer that the stand-in reader gives for it,
    /// to one closure from `tap_callback`, and reads what that event alone came to.
    fn through_one_callback(events: &[(CGEventType, SecureInputState)]) -> Vec<EventOutcome> {
        let (ctx, writer) = context(stand_in_reader);
        let counters = ctx.counters.clone();
        let callback = tap_callback(ctx);
        let source =
            CGEventSource::new(CGEventSourceStateID::Private).expect("a private event source");
        let event = CGEvent::new(source).expect("an event object of type null, never posted");
        events
            .iter()
            .map(|&(etype, answer)| {
                STAND_IN_ANSWER.with(|cell| cell.set(answer));
                STAND_IN_ASKED.with(|cell| cell.set(0));
                let suppressed_before = counters.suppressed_keys();
                let _ = callback(std::ptr::null(), etype, &event);
                let mut recorded = Vec::new();
                while let Ok(received) = writer.try_recv() {
                    recorded.push(received.event().modality);
                }
                EventOutcome {
                    recorded,
                    suppressed: counters.suppressed_keys() - suppressed_before,
                    asked: STAND_IN_ASKED.with(Cell::get),
                }
            })
            .collect()
    }

    fn holds_for_its_own_answer(
        etype: CGEventType,
        answer: SecureInputState,
        outcome: &EventOutcome,
    ) -> Result<(), String> {
        let EventOutcome {
            recorded,
            suppressed,
            asked,
        } = outcome;
        match tap_class(etype) {
            Some(TapClass::KeyPress | TapClass::OtherKey) => {
                let (want_recorded, want_suppressed) = match answer {
                    SecureInputState::Inactive => (vec![Modality::Keystroke], 0),
                    SecureInputState::Active => (vec![], 1),
                };
                if (recorded, *suppressed) != (&want_recorded, want_suppressed) {
                    return Err(format!(
                        "(a) {etype:?} answered {answer:?}: recorded {recorded:?} and suppressed \
                         {suppressed}, not {want_recorded:?} and {want_suppressed}"
                    ));
                }
                if *asked != 1 {
                    return Err(format!(
                        "(b) {etype:?} answered {answer:?}: the reader was asked {asked} times, \
                         not once"
                    ));
                }
            }
            Some(TapClass::DiscretePointer | TapClass::ContinuousPointer) => {
                if (recorded.as_slice(), *suppressed) != ([Modality::Pointer].as_slice(), 0) {
                    return Err(format!(
                        "(c) {etype:?} answered {answer:?}: recorded {recorded:?} and suppressed \
                         {suppressed}, not one pointer event and 0"
                    ));
                }
                if *asked > 1 {
                    return Err(format!(
                        "(c) {etype:?} answered {answer:?}: the reader was asked {asked} times, \
                         more than once"
                    ));
                }
            }
            None => return Err(format!("{etype:?} is not a type of TAP_EVENTS")),
        }
        Ok(())
    }

    #[test]
    fn each_event_of_every_pair_through_the_callback_is_decided_by_its_own_reading() {
        let answers = [SecureInputState::Inactive, SecureInputState::Active];
        for &(first, _) in &TAP_EVENTS {
            for &(second, _) in &TAP_EVENTS {
                for first_answer in answers {
                    for second_answer in answers {
                        let events = [(first, first_answer), (second, second_answer)];
                        let outcomes = through_one_callback(&events);
                        for (place, (&(etype, answer), outcome)) in
                            events.iter().zip(&outcomes).enumerate()
                        {
                            if let Err(failure) = holds_for_its_own_answer(etype, answer, outcome) {
                                panic!(
                                    "the pair {first:?} answered {first_answer:?}, then \
                                     {second:?} answered {second_answer:?}: event {} of 2: \
                                     {failure}",
                                    place + 1
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn each_event_of_a_drawn_sequence_through_the_callback_is_decided_by_its_own_reading() {
        const SEED: u64 = 0x5EC0_4E1D_2026_1113;
        const LENGTH: usize = 256;
        let mut state = SEED;
        let mut draw = || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            state >> 33
        };
        let events: Vec<(CGEventType, SecureInputState)> = (0..LENGTH)
            .map(|_| {
                let etype = TAP_EVENTS[(draw() % TAP_EVENTS.len() as u64) as usize].0;
                let answer = if draw() % 2 == 0 {
                    SecureInputState::Inactive
                } else {
                    SecureInputState::Active
                };
                (etype, answer)
            })
            .collect();
        let is_key = |etype| {
            matches!(
                tap_class(etype),
                Some(TapClass::KeyPress | TapClass::OtherKey)
            )
        };
        let key_after_a_pointer_answered_otherwise = events
            .windows(2)
            .filter(|two| !is_key(two[0].0) && is_key(two[1].0) && two[0].1 != two[1].1)
            .count();
        assert!(
            key_after_a_pointer_answered_otherwise > 0,
            "the seed {SEED:#x} draws no key event right after a pointer event answered otherwise"
        );
        let outcomes = through_one_callback(&events);
        for (place, (&(etype, answer), outcome)) in events.iter().zip(&outcomes).enumerate() {
            if let Err(failure) = holds_for_its_own_answer(etype, answer, outcome) {
                panic!(
                    "the sequence of {LENGTH} events from the seed {SEED:#x}: the first event \
                     that fails is at place {place} (from 0): {failure}"
                );
            }
        }
    }

    static TIMED_TAP_ENABLED: Mutex<Option<Instant>> = Mutex::new(None);
    static TIMED_HOLD_TAKEN: Mutex<Option<Instant>> = Mutex::new(None);

    fn timed_take() -> i32 {
        *TIMED_HOLD_TAKEN
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(Instant::now());
        0
    }

    fn timed_give_back() -> i32 {
        0
    }

    fn timed_hold() -> Result<SecureInputHold, SecureInputRefused> {
        crate::secure_input_macos::hold_with(timed_take, timed_give_back)
    }

    struct TimingMaker;

    impl TapMaker for TimingMaker {
        type Tap = ();
        type Enabled = ();

        fn make<F>(self, _settings: TapSettings, _callback: F) -> Result<(), ()>
        where
            F: Fn(CGEventTapProxy, CGEventType, &CGEvent) -> CallbackResult + Send + 'static,
        {
            Ok(())
        }

        fn listen_denied() -> bool {
            false
        }

        fn enable(_tap: (), _port: TapPort) -> Result<(), CaptureError> {
            *TIMED_TAP_ENABLED
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = Some(Instant::now());
            Ok(())
        }
    }

    #[test]
    fn a_plan_with_a_delay_holds_no_sooner_than_its_delay_after_the_tap_is_enabled() {
        let delay = Duration::from_millis(300);
        let (handle, _writer) = sink::channel(8);
        let mut source = source_with(HoldPlan::After(delay));
        source.hold = timed_hold;
        source
            .begin_with(handle, TimingMaker, || {})
            .expect("the source starts with a timing tap");
        let started = Instant::now();
        while TIMED_HOLD_TAKEN
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_none()
        {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "the plan took no hold in 5 s"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        source.shutdown().expect("the source stops");
        let enabled = TIMED_TAP_ENABLED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .expect("the tap was enabled");
        let taken = TIMED_HOLD_TAKEN
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .expect("the hold was taken");
        assert!(
            taken >= enabled + delay,
            "the hold was taken {:?} after the tap was enabled, before the plan's {delay:?}",
            taken.saturating_duration_since(enabled)
        );
    }

    #[test]
    fn the_callback_reads_the_secure_state_again_for_every_event() {
        static READS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        fn off_then_on() -> SecureInputState {
            if READS.fetch_add(1, Ordering::SeqCst) == 0 {
                SecureInputState::Inactive
            } else {
                SecureInputState::Active
            }
        }
        let (ctx, writer) = context(off_then_on);
        for _ in 0..3 {
            on_tap_event(&ctx, CGEventType::KeyDown);
        }
        let suppressed = ctx.counters.suppressed_keys();
        drop(ctx);
        let recorded = events_until_the_sink_closes(&writer);
        assert_eq!(
            (recorded.len(), suppressed),
            (1, 2),
            "three key-downs read as off, on, on: (recorded, suppressed)"
        );
        assert_eq!(READS.load(Ordering::SeqCst), 3, "reads of the secure state");
    }

    #[test]
    fn a_new_source_with_a_hold_plan_holds_secure_input_through_the_system_while_it_runs() {
        let _one = one_hold_at_a_time();
        let limit = Duration::from_secs(2);
        let on = || crate::secure_input_macos::secure_input_active();
        let before = agreed_reading(on, limit);
        let (handle, _writer) = sink::channel(8);
        let (_fg_writer, reader) = foreground_state_for_test();
        let mut source = InputSource::new(
            reader,
            InputCounters::new(),
            HoldPlan::After(Duration::ZERO),
        );
        let (ended_tx, _ended_rx) = channel();
        source
            .begin_with(handle, FakeMaker(ended_tx), || {})
            .expect("the source starts with a stand-in tap");
        if let Some(refused) = source.hold_refusal() {
            let _ = source.shutdown();
            eprintln!(
                "SKIP a_new_source_with_a_hold_plan_holds_secure_input_through_the_system_while_it_runs: \
                 the system refused the hold with status {}",
                refused.0
            );
            return;
        }
        let during = reads_on_within(on, limit);
        let shut = source.shutdown();
        let after = agreed_reading(on, limit);
        assert!(!before.0, "secure input read on before begin");
        assert!(during, "on while the source runs");
        assert_eq!(shut, Ok(()));
        assert!(!after.0, "secure input still read on after shutdown");
    }

    static STAND_IN_HELD: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);
    static STAND_IN_RELEASED_ON: Mutex<Option<String>> = Mutex::new(None);

    fn stand_in_take() -> i32 {
        STAND_IN_HELD.fetch_add(1, Ordering::SeqCst);
        0
    }

    fn stand_in_give_back() -> i32 {
        STAND_IN_HELD.fetch_sub(1, Ordering::SeqCst);
        *STAND_IN_RELEASED_ON
            .lock()
            .unwrap_or_else(PoisonError::into_inner) =
            std::thread::current().name().map(str::to_owned);
        0
    }

    fn stand_in_hold() -> Result<SecureInputHold, SecureInputRefused> {
        crate::secure_input_macos::hold_with(stand_in_take, stand_in_give_back)
    }

    #[test]
    fn a_hold_under_a_blocked_emit_outlives_a_timed_out_shutdown_and_the_drop_until_the_sink_is_read(
    ) {
        let (handle, writer) = sink::channel(0);
        let (_fg_writer, reader) = foreground_state_for_test();
        let mut source = InputSource::new(
            reader,
            InputCounters::new(),
            HoldPlan::After(Duration::ZERO),
        );
        source.hold = stand_in_hold;
        let blocked = handle.clone();
        let (in_emit_tx, in_emit_rx) = channel::<()>();
        let (ended_tx, _ended_rx) = channel();
        source
            .begin_with(handle, FakeMaker(ended_tx), move || {
                let _ = in_emit_tx.send(());
                let _ = emit_metadata_only(&blocked, Modality::Keystroke, None, Unpersisted::EMPTY);
            })
            .expect("the source starts with a stand-in tap");
        in_emit_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("the tap thread reached its emit");
        let shut = source.shutdown();
        drop(source);
        let held_after_the_drop = STAND_IN_HELD.load(Ordering::SeqCst);
        let read = writer
            .recv_timeout(Duration::from_secs(5))
            .map(|received| received.event().modality);
        let started = Instant::now();
        while STAND_IN_HELD.load(Ordering::SeqCst) != 0 {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "the hold was not given back after the sink was read"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        let released_on = STAND_IN_RELEASED_ON
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        assert_eq!(
            shut,
            Err(CaptureError::Timeout {
                stage: "input_tap_join",
                ms: 1000
            })
        );
        assert_eq!(read, Ok(Modality::Keystroke));
        assert_eq!(
            released_on.as_deref(),
            Some("halvo-input-tap"),
            "the tap thread gives the hold back once its blocked emit returns"
        );
        assert_eq!(
            held_after_the_drop, 1,
            "the hold is the tap thread's: after a timed-out shutdown and the drop it is still \
             taken while the emit is blocked, until that thread ends or the process ends"
        );
    }

    #[test]
    fn the_production_reader_reads_off_on_during_a_hold_and_off_again() {
        let one = one_hold_at_a_time();
        let limit = Duration::from_secs(2);
        let reader_on = || current_secure_state() == SecureInputState::Active;
        let before = agreed_reading(reader_on, limit);
        let Some(hold) =
            one.hold_or_skip("the_production_reader_reads_off_on_during_a_hold_and_off_again")
        else {
            return;
        };
        let during = reads_on_within(reader_on, limit);
        drop(hold);
        let after = agreed_reading(reader_on, limit);
        assert_eq!(before.0, before.1, "before the hold: (reader, session)");
        assert!(
            during,
            "during an accepted hold the production reader does not read Active"
        );
        assert_eq!(after.0, after.1, "after the hold: (reader, session)");
    }

    #[test]
    fn the_callback_that_the_tap_gets_counts_a_key_read_as_secure() {
        let (handle, writer) = sink::channel(8);
        let (_writer, reader) = foreground_state_for_test();
        let counters = InputCounters::new();
        let mut settings = tap_settings();
        settings.secure = || SecureInputState::Active;
        let callback = tap_callback(tap_context(handle, reader, counters.clone(), &settings));
        let source =
            CGEventSource::new(CGEventSourceStateID::Private).expect("a private event source");
        let event = CGEvent::new(source).expect("an event object of type null, never posted");
        let _ = callback(std::ptr::null(), CGEventType::KeyDown, &event);
        assert_eq!(counters.suppressed_keys(), 1);
        drop(callback);
        assert!(
            events_until_the_sink_closes(&writer).is_empty(),
            "a key read as secure was recorded"
        );
    }

    type KeptCallback =
        Box<dyn Fn(CGEventTapProxy, CGEventType, &CGEvent) -> CallbackResult + Send>;

    struct KeepingMaker(Arc<Mutex<Option<KeptCallback>>>);

    impl TapMaker for KeepingMaker {
        type Tap = ();
        type Enabled = ();

        fn make<F>(self, _settings: TapSettings, callback: F) -> Result<(), ()>
        where
            F: Fn(CGEventTapProxy, CGEventType, &CGEvent) -> CallbackResult + Send + 'static,
        {
            *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(Box::new(callback));
            Err(())
        }

        fn listen_denied() -> bool {
            true
        }

        fn enable(_tap: (), _port: TapPort) -> Result<(), CaptureError> {
            Ok(())
        }
    }

    #[test]
    fn a_started_source_counts_a_key_under_a_real_hold_through_the_callback_that_it_installs() {
        let kept = Arc::new(Mutex::new(None));
        let (handle, writer) = sink::channel(8);
        let (_fg_writer, reader) = foreground_state_for_test();
        let counters = InputCounters::new();
        let mut source = InputSource::new(reader, counters.clone(), HoldPlan::Never);
        let started = source.begin_with(handle, KeepingMaker(kept.clone()), || {});
        assert!(
            matches!(
                started,
                Err(CaptureError::PermissionDenied(Modality::Keystroke))
            ),
            "start did not return the permission error of a tap that was not made"
        );
        let callback = kept
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
            .expect("the maker was given a callback");
        let one = one_hold_at_a_time();
        let Some(hold) = one.hold_or_skip(
            "a_started_source_counts_a_key_under_a_real_hold_through_the_callback_that_it_installs",
        ) else {
            return;
        };
        let source_of_events =
            CGEventSource::new(CGEventSourceStateID::Private).expect("a private event source");
        let event =
            CGEvent::new(source_of_events).expect("an event object of type null, never posted");
        let _ = callback(std::ptr::null(), CGEventType::KeyDown, &event);
        drop(hold);
        drop(callback);
        assert_eq!(
            counters.suppressed_keys(),
            1,
            "a key under a real hold was not counted on the source's handle"
        );
        assert!(
            events_until_the_sink_closes(&writer).is_empty(),
            "a key under a real hold was recorded"
        );
    }

    #[test]
    fn the_callback_that_the_tap_gets_reenables_a_disabled_tap() {
        ENABLE_CALLS.with(|calls| calls.borrow_mut().clear());
        let (handle, _writer) = sink::channel(8);
        let (_fg_writer, reader) = foreground_state_for_test();
        let counters = InputCounters::new();
        let settings = tap_settings();
        let mut ctx = tap_context(handle, reader, counters.clone(), &settings);
        ctx.port = TapPort::with_enable(record_enable);
        let tap = std::ptr::NonNull::<c_void>::dangling().as_ptr();
        ctx.port.store(tap);
        let callback = tap_callback(ctx);
        let source =
            CGEventSource::new(CGEventSourceStateID::Private).expect("a private event source");
        let event = CGEvent::new(source).expect("an event object of type null, never posted");
        let _ = callback(std::ptr::null(), CGEventType::TapDisabledByTimeout, &event);
        ENABLE_CALLS.with(|calls| {
            assert_eq!(
                *calls.borrow(),
                [(tap as usize, true)],
                "a disable event through the tap's callback did not re-enable the tap"
            );
        });
        assert_eq!(counters.suppressed_keys(), 0);
    }

    #[test]
    fn the_tap_is_created_listen_only() {
        assert_eq!(TAP_OPTIONS as u32, 1);
    }

    #[test]
    fn the_tap_reads_the_secure_state_of_the_os() {
        assert!(std::ptr::fn_addr_eq(
            tap_settings().secure,
            current_secure_state as fn() -> SecureInputState
        ));
    }

    #[test]
    fn the_tap_is_requested_listen_only_at_the_hid_location() {
        let settings = tap_settings();
        assert_eq!(
            (
                settings.options as u32,
                settings.location as u32,
                settings.placement as u32
            ),
            (1, 0, 0)
        );
    }

    #[test]
    fn the_tap_is_requested_for_the_events_of_the_table() {
        let mask = tap_settings()
            .events
            .iter()
            .fold(0u64, |mask, &etype| mask | 1 << (etype as u64));
        assert_eq!(mask, 0x0E40_1CFE);
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

    #[test]
    fn a_disable_event_reenables_the_tap() {
        for etype in [
            CGEventType::TapDisabledByTimeout,
            CGEventType::TapDisabledByUserInput,
        ] {
            ENABLE_CALLS.with(|calls| calls.borrow_mut().clear());
            let (mut ctx, writer) = context(|| SecureInputState::Inactive);
            ctx.port = TapPort::with_enable(record_enable);
            let tap = std::ptr::NonNull::<c_void>::dangling().as_ptr();
            ctx.port.store(tap);
            on_tap_event(&ctx, etype);
            ENABLE_CALLS.with(|calls| {
                assert_eq!(*calls.borrow(), [(tap as usize, true)], "{etype:?}");
            });
            assert_eq!(ctx.counters.suppressed_keys(), 0);
            drop(ctx);
            assert!(
                events_until_the_sink_closes(&writer).is_empty(),
                "{etype:?} was recorded"
            );
        }
    }

    #[test]
    fn a_clone_of_the_port_calls_nothing_once_the_port_guard_drops() {
        let port = TapPort::with_enable(record_enable);
        let clone = port.clone();
        port.store(std::ptr::NonNull::<c_void>::dangling().as_ptr());
        drop(PortClearedFirst { port, tap: () });
        clone.reenable();
        ENABLE_CALLS.with(|calls| assert_eq!(*calls.borrow(), []));
    }

    struct SeesThePort(TapPort, Arc<Mutex<Option<bool>>>);

    impl Drop for SeesThePort {
        fn drop(&mut self) {
            let cleared = self.0.port.load(Ordering::Relaxed).is_null();
            *self.1.lock().unwrap_or_else(PoisonError::into_inner) = Some(cleared);
        }
    }

    #[test]
    fn the_port_is_cleared_before_the_tap_drops() {
        let port = TapPort::with_enable(record_enable);
        port.store(std::ptr::NonNull::<c_void>::dangling().as_ptr());
        let seen = Arc::new(Mutex::new(None));
        drop(PortClearedFirst {
            port: port.clone(),
            tap: SeesThePort(port, seen.clone()),
        });
        assert_eq!(
            *seen.lock().unwrap_or_else(PoisonError::into_inner),
            Some(true),
            "the tap dropped while its port was still set"
        );
    }

    fn invalidate_and_log(timer: &CFRunLoopTimer) {
        log_step("invalidate");
        invalidate(timer);
    }

    #[test]
    fn a_timed_hold_invalidates_its_timer_before_its_slot_releases_the_hold() {
        let _one = HOLD_LOG_TESTS
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        HOLD_LOG
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
        let mut hold = TimedHold::new(HoldSlot {
            hold: shim_hold,
            held: None,
            refusal: Arc::new(Mutex::new(None)),
        });
        hold.invalidate = invalidate_and_log;
        hold.take_now();
        let address = &*hold.slot as *const HoldSlot;
        hold.take_after(Duration::from_secs(3600));
        let moved = std::mem::replace(
            &mut hold,
            TimedHold::new(HoldSlot {
                hold: shim_hold,
                held: None,
                refusal: Arc::new(Mutex::new(None)),
            }),
        );
        let same_address = std::ptr::eq(&*moved.slot, address);
        drop(moved);
        let log = std::mem::take(&mut *HOLD_LOG.lock().unwrap_or_else(PoisonError::into_inner));
        let steps: Vec<&str> = log.iter().map(|&(step, _)| step).collect();
        assert!(same_address, "the slot moved when its owner moved");
        assert_eq!(steps, ["enable", "invalidate", "disable"]);
    }

    extern "C" fn never_fires(_timer: CFRunLoopTimerRef, _info: *mut c_void) {}

    #[test]
    fn stop_ends_the_tap_thread_within_one_second() {
        for cycle in 0..3 {
            let (alive_tx, alive_rx) = channel::<()>();
            let mut tap = TapThread::spawn(
                move || {
                    let timer = CFRunLoopTimer::new(
                        CFDate::now().abs_time() + 3600.0,
                        0.0,
                        0,
                        0,
                        never_fires,
                        std::ptr::null_mut(),
                    );
                    // SAFETY: `kCFRunLoopCommonModes` is a constant CFString that
                    // CoreFoundation exports and never writes.
                    CFRunLoop::get_current().add_timer(&timer, unsafe { kCFRunLoopCommonModes });
                    Ok((timer, alive_tx))
                },
                HoldPlan::Never,
                hold_secure_input,
                Arc::new(Mutex::new(None)),
                || {},
            )
            .expect("the tap thread starts");
            tap.wait_until_its_run_loop_waits();
            let deadline = Duration::from_secs(1);
            let stop_called = std::time::Instant::now();
            let stopped = tap.stop_within(deadline);
            let ended = alive_rx.recv_timeout(deadline.saturating_sub(stop_called.elapsed()));
            let took = stop_called.elapsed();
            assert!(
                stopped.is_ok() && ended == Err(RecvTimeoutError::Disconnected) && took <= deadline,
                "cycle {cycle}: the tap thread ended {took:?} after stop was called ({ended:?})"
            );
        }
    }
    struct FakeMaker(std::sync::mpsc::Sender<()>);

    impl TapMaker for FakeMaker {
        type Tap = std::sync::mpsc::Sender<()>;
        type Enabled = std::sync::mpsc::Sender<()>;

        fn make<F>(self, _settings: TapSettings, _callback: F) -> Result<Self::Tap, ()>
        where
            F: Fn(CGEventTapProxy, CGEventType, &CGEvent) -> CallbackResult + Send + 'static,
        {
            Ok(self.0)
        }

        fn listen_denied() -> bool {
            false
        }

        fn enable(tap: Self::Tap, _port: TapPort) -> Result<Self::Enabled, CaptureError> {
            Ok(tap)
        }
    }

    fn source_with(plan: HoldPlan) -> InputSource {
        let (_writer, reader) = foreground_state_for_test();
        InputSource::new(reader, InputCounters::new(), plan)
    }

    fn wait_until_the_tap_thread_ended(source: &InputSource) {
        let started = Instant::now();
        while !source.tap.as_ref().is_some_and(TapThread::ended) {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "the tap thread did not end in 5 s"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn a_second_begin_is_refused_and_shutdown_ends_the_tap_thread_within_one_second() {
        let (handle, _writer) = sink::channel(8);
        let mut source = source_with(HoldPlan::Never);
        let (ended_tx, ended_rx) = channel();
        let first = source.begin_with(handle.clone(), FakeMaker(ended_tx), || {});
        let opened = Arc::new(AtomicUsize::new(0));
        let second = source.begin_with(handle, CountingMaker(opened.clone()), || {});
        let running = source.health().state;
        let called = Instant::now();
        let shut = source.shutdown();
        let took = called.elapsed();
        assert_eq!(first, Ok(Started::Running), "the first begin");
        assert_eq!(second, Err(CaptureError::AlreadyStarted), "a second begin");
        assert_eq!(
            opened.load(Ordering::SeqCst),
            0,
            "a second begin made a tap"
        );
        assert_eq!(running, SourceState::Running, "the state while it runs");
        assert!(
            shut == Ok(()) && took <= INPUT_STOP_DEADLINE,
            "shutdown answered {shut:?} after {took:?}"
        );
        assert_eq!(
            ended_rx.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Disconnected),
            "the tap thread still holds its tap after shutdown"
        );
        assert_eq!(
            source.health().state,
            SourceState::Stopped(StopCause::Request)
        );
    }

    #[test]
    fn a_dropped_source_ends_its_tap_thread_within_one_second() {
        let (handle, _writer) = sink::channel(8);
        let mut source = source_with(HoldPlan::Never);
        let (ended_tx, ended_rx) = channel();
        source
            .begin_with(handle, FakeMaker(ended_tx), || {})
            .expect("the source starts with a fake tap");
        let called = Instant::now();
        drop(source);
        let took = called.elapsed();
        assert_eq!(
            ended_rx.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Disconnected),
            "the tap thread still held its tap when the drop returned"
        );
        assert!(took <= INPUT_STOP_DEADLINE, "the drop took {took:?}");
    }

    #[test]
    fn a_stop_before_the_run_loop_runs_ends_the_tap_thread_within_one_second() {
        let (ended_tx, ended_rx) = channel::<()>();
        let mut tap = TapThread::spawn(
            move || Ok(ended_tx),
            HoldPlan::Never,
            hold_secure_input,
            Arc::new(Mutex::new(None)),
            || std::thread::sleep(Duration::from_millis(200)),
        )
        .expect("the tap thread starts");
        let called = Instant::now();
        let stopped = tap.stop_within(INPUT_STOP_DEADLINE);
        let took = called.elapsed();
        assert!(
            stopped.is_ok() && took <= INPUT_STOP_DEADLINE,
            "a stop sent 200 ms before the run loop ran: {stopped:?} after {took:?}"
        );
        assert_eq!(
            ended_rx.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Disconnected)
        );
    }

    #[test]
    fn shutdown_answers_thread_ended_when_the_tap_thread_had_ended() {
        let (handle, _writer) = sink::channel(8);
        let mut source = source_with(HoldPlan::Never);
        let (ended_tx, _ended_rx) = channel();
        source
            .begin_with(handle, FakeMaker(ended_tx), || {
                panic!("scripted end of the tap thread")
            })
            .expect("the source starts with a fake tap");
        wait_until_the_tap_thread_ended(&source);
        let state = source.health().state;
        let shut = source.shutdown();
        assert_eq!(
            state,
            SourceState::Failed {
                reason: crate::source::HealthReason::ThreadEnded,
                error: CaptureError::ThreadEnded
            }
        );
        assert_eq!(shut, Err(CaptureError::ThreadEnded));
        assert_eq!(
            source.health().state,
            state,
            "a tap thread that ended on its own still reads failed after shutdown answered ThreadEnded"
        );
    }

    #[test]
    fn shutdown_answers_timeout_at_the_deadline_and_the_source_stays_started() {
        let (handle, _writer) = sink::channel(8);
        let mut source = source_with(HoldPlan::Never);
        let (ended_tx, _ended_rx) = channel();
        source
            .begin_with(handle.clone(), FakeMaker(ended_tx), || {
                std::thread::sleep(Duration::from_millis(1500))
            })
            .expect("the source starts with a fake tap");
        let called = Instant::now();
        let first = source.shutdown();
        let took = called.elapsed();
        let opened = Arc::new(AtomicUsize::new(0));
        let again = source.begin_with(handle, CountingMaker(opened.clone()), || {});
        let second = source.shutdown();
        assert_eq!(
            first,
            Err(CaptureError::Timeout {
                stage: "input_tap_join",
                ms: 1000
            })
        );
        assert!(
            took >= INPUT_STOP_DEADLINE && took < INPUT_STOP_DEADLINE + Duration::from_millis(500),
            "the first shutdown returned after {took:?}"
        );
        assert_eq!(again, Err(CaptureError::AlreadyStarted));
        assert_eq!(
            opened.load(Ordering::SeqCst),
            0,
            "a begin after the timeout made a tap"
        );
        assert_eq!(second, Ok(()));
    }

    #[test]
    fn a_recorded_key_and_pointer_carry_the_app_in_front() {
        let (handle, writer) = sink::channel(8);
        let (fg_writer, reader) = foreground_state_for_test();
        fg_writer.set_for_test(Some(crate::macos::foreground::tests_support::front(
            "com.example.editor",
            41,
        )));
        let ctx = tap_context(handle, reader, InputCounters::new(), &tap_settings());
        dispatch_tap_event(&ctx, CGEventType::KeyDown, SecureInputState::Inactive);
        dispatch_tap_event(&ctx, CGEventType::MouseMoved, SecureInputState::Inactive);
        drop(ctx);
        let apps: Vec<String> = std::iter::from_fn(|| writer.recv())
            .map(|event| event.foreground_app)
            .collect();
        assert_eq!(apps, ["com.example.editor", "com.example.editor"]);
    }

    #[test]
    fn each_type_of_the_table_has_its_class() {
        use TapClass::{
            ContinuousPointer as C, DiscretePointer as D, KeyPress as K, OtherKey as O,
        };
        let classes: Vec<(u32, TapClass)> = TAP_EVENTS
            .iter()
            .map(|&(etype, class)| (etype as u32, class))
            .collect();
        assert_eq!(
            classes,
            [
                (10, K),
                (11, O),
                (12, O),
                (1, D),
                (2, D),
                (3, D),
                (4, D),
                (25, D),
                (26, D),
                (5, C),
                (6, C),
                (7, C),
                (27, C),
                (22, C)
            ]
        );
    }

    #[test]
    fn a_failed_thread_start_carries_fixed_text() {
        assert_eq!(
            spawn_failure(std::io::Error::other("the words of the system")),
            CaptureError::Start("the tap thread was not started".into())
        );
    }

    #[test]
    fn only_a_denial_of_the_system_is_a_permission_error() {
        assert_eq!(
            tap_refusal(true),
            CaptureError::PermissionDenied(Modality::Keystroke)
        );
        assert_eq!(
            tap_refusal(false),
            CaptureError::Os {
                stage: "event_tap_create",
                domain: "CoreGraphics",
                code: 0
            }
        );
    }

    #[test]
    fn a_second_source_built_on_the_same_handle_reads_the_first_sources_count() {
        let counters = InputCounters::new();
        let (handle, _writer) = sink::channel(8);
        let (_writer, reader) = foreground_state_for_test();
        let first = InputSource::new(reader.clone(), counters.clone(), HoldPlan::Never);
        let ctx = tap_context(
            handle,
            reader.clone(),
            first.counters.clone(),
            &tap_settings(),
        );
        dispatch_tap_event(&ctx, CGEventType::KeyDown, SecureInputState::Active);
        drop((ctx, first));
        let second = InputSource::new(reader, counters, HoldPlan::Never);
        assert_eq!(second.health().counters.secure_input_keystrokes, 1);
    }

    static HOLD_LOG: Mutex<Vec<(&'static str, std::thread::ThreadId)>> = Mutex::new(Vec::new());
    static HOLD_LOG_TESTS: Mutex<()> = Mutex::new(());

    fn log_step(step: &'static str) {
        HOLD_LOG
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((step, std::thread::current().id()));
    }

    fn shim_enable() -> i32 {
        log_step("enable");
        0
    }

    fn shim_refuse() -> i32 {
        log_step("enable");
        -50
    }

    fn shim_disable() -> i32 {
        log_step("disable");
        0
    }

    fn shim_hold() -> Result<SecureInputHold, SecureInputRefused> {
        crate::secure_input_macos::hold_with(shim_enable, shim_disable)
    }

    fn shim_refused_hold() -> Result<SecureInputHold, SecureInputRefused> {
        crate::secure_input_macos::hold_with(shim_refuse, shim_disable)
    }

    struct LoggingMaker;

    struct LoggedTap;

    impl Drop for LoggedTap {
        fn drop(&mut self) {
            log_step("loop end");
        }
    }

    impl TapMaker for LoggingMaker {
        type Tap = ();
        type Enabled = LoggedTap;

        fn make<F>(self, _settings: TapSettings, _callback: F) -> Result<(), ()>
        where
            F: Fn(CGEventTapProxy, CGEventType, &CGEvent) -> CallbackResult + Send + 'static,
        {
            Ok(())
        }

        fn listen_denied() -> bool {
            false
        }

        fn enable(_tap: (), _port: TapPort) -> Result<LoggedTap, CaptureError> {
            log_step("tap enable");
            Ok(LoggedTap)
        }
    }

    fn held_run(
        plan: HoldPlan,
        hold: Hold,
        wait_for: &[&str],
    ) -> (Vec<&'static str>, usize, bool, Option<SecureInputRefused>) {
        let _one = HOLD_LOG_TESTS
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        HOLD_LOG
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
        let (handle, _writer) = sink::channel(8);
        let mut source = source_with(plan);
        source.hold = hold;
        source
            .begin_with(handle, LoggingMaker, || {})
            .expect("the source starts with a logging tap");
        let started = Instant::now();
        while !wait_for.iter().all(|step| {
            HOLD_LOG
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .iter()
                .any(|(logged, _)| logged == step)
        }) {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "no {wait_for:?} in 5 s"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        source.shutdown().expect("the source stops");
        let refusal = source.hold_refusal();
        let log = std::mem::take(&mut *HOLD_LOG.lock().unwrap_or_else(PoisonError::into_inner));
        let threads: std::collections::HashSet<_> = log.iter().map(|&(_, id)| id).collect();
        let on_this_thread = threads.contains(&std::thread::current().id());
        (
            log.iter().map(|&(step, _)| step).collect(),
            threads.len(),
            on_this_thread,
            refusal,
        )
    }

    #[test]
    fn a_zero_plan_holds_before_the_tap_is_enabled_and_releases_after_the_loop_on_one_thread() {
        let (steps, threads, on_this_thread, refusal) =
            held_run(HoldPlan::After(Duration::ZERO), shim_hold, &["tap enable"]);
        assert_eq!(steps, ["enable", "tap enable", "loop end", "disable"]);
        assert_eq!(
            (threads, on_this_thread),
            (1, false),
            "every call runs on the tap thread"
        );
        assert_eq!(refusal, None);
    }

    #[test]
    fn a_later_plan_holds_from_a_timer_on_the_tap_thread() {
        let (steps, threads, on_this_thread, refusal) = held_run(
            HoldPlan::After(Duration::from_millis(50)),
            shim_hold,
            &["tap enable", "enable"],
        );
        assert_eq!(steps, ["tap enable", "enable", "loop end", "disable"]);
        assert_eq!((threads, on_this_thread), (1, false));
        assert_eq!(refusal, None);
    }

    #[test]
    fn a_refused_hold_is_kept_for_the_run_and_releases_nothing() {
        let (steps, _, _, refusal) = held_run(
            HoldPlan::After(Duration::ZERO),
            shim_refused_hold,
            &["tap enable"],
        );
        assert_eq!(steps, ["enable", "tap enable", "loop end"]);
        assert_eq!(refusal, Some(SecureInputRefused(-50)));
    }

    #[test]
    fn a_later_begin_reports_no_refusal_of_an_earlier_one() {
        let _one = HOLD_LOG_TESTS
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let (handle, _writer) = sink::channel(8);
        let mut source = source_with(HoldPlan::After(Duration::ZERO));
        source.hold = shim_refused_hold;
        source
            .begin_with(handle.clone(), LoggingMaker, || {})
            .expect("the first begin starts");
        source.shutdown().expect("the first run stops");
        let first = source.hold_refusal();
        source.hold = shim_hold;
        source
            .begin_with(handle, LoggingMaker, || {})
            .expect("the second begin starts");
        source.shutdown().expect("the second run stops");
        assert_eq!(first, Some(SecureInputRefused(-50)));
        assert_eq!(source.hold_refusal(), None);
    }

    struct FailingMaker;

    impl TapMaker for FailingMaker {
        type Tap = ();
        type Enabled = ();

        fn make<F>(self, _settings: TapSettings, _callback: F) -> Result<(), ()>
        where
            F: Fn(CGEventTapProxy, CGEventType, &CGEvent) -> CallbackResult + Send + 'static,
        {
            log_step("make refused");
            Err(())
        }

        fn listen_denied() -> bool {
            true
        }

        fn enable(_tap: (), _port: TapPort) -> Result<(), CaptureError> {
            Ok(())
        }
    }

    #[test]
    fn a_hold_taken_before_a_tap_that_is_not_made_is_released_on_the_tap_thread() {
        let _one = HOLD_LOG_TESTS
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        HOLD_LOG
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
        let (handle, _writer) = sink::channel(8);
        let mut source = source_with(HoldPlan::After(Duration::ZERO));
        source.hold = shim_hold;
        let started = source.begin_with(handle, FailingMaker, || {});
        let log = std::mem::take(&mut *HOLD_LOG.lock().unwrap_or_else(PoisonError::into_inner));
        let steps: Vec<&str> = log.iter().map(|&(step, _)| step).collect();
        let threads: std::collections::HashSet<_> = log.iter().map(|&(_, id)| id).collect();
        assert_eq!(
            started,
            Err(CaptureError::PermissionDenied(Modality::Keystroke))
        );
        assert_eq!(steps, ["enable", "make refused", "disable"]);
        assert_eq!(threads.len(), 1);
        assert!(!threads.contains(&std::thread::current().id()));
    }

    #[test]
    fn a_plan_of_never_takes_no_hold() {
        let (steps, _, _, refusal) = held_run(HoldPlan::Never, shim_hold, &["tap enable"]);
        assert_eq!(steps, ["tap enable", "loop end"]);
        assert_eq!(refusal, None);
    }
}

#[cfg(test)]
mod granted_tests {
    use super::*;
    use crate::macos::foreground::foreground_state_for_test;
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

    #[test]
    fn tap_information_has_the_layout_of_cg_event_tap_information() {
        use std::mem::{align_of, offset_of, size_of};
        assert_eq!(
            (size_of::<TapInformation>(), align_of::<TapInformation>()),
            (48, 8)
        );
        assert_eq!(
            [
                offset_of!(TapInformation, event_tap_id),
                offset_of!(TapInformation, tap_point),
                offset_of!(TapInformation, options),
                offset_of!(TapInformation, events_of_interest),
                offset_of!(TapInformation, tapping_process),
                offset_of!(TapInformation, process_being_tapped),
                offset_of!(TapInformation, enabled),
                offset_of!(TapInformation, min_usec_latency),
                offset_of!(TapInformation, avg_usec_latency),
                offset_of!(TapInformation, max_usec_latency),
            ],
            [0, 4, 8, 16, 24, 28, 32, 36, 40, 44]
        );
    }

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
        let (_writer, reader) = foreground_state_for_test();
        let mut source = InputSource::new(reader, InputCounters::new(), HoldPlan::Never);
        source
            .begin(crate::source::SourceContext::new(handle))
            .expect("the input source starts");
        let taps = own_taps();
        source
            .tap
            .as_ref()
            .expect("a started source holds its tap thread")
            .wait_until_its_run_loop_waits();
        source.shutdown().expect("the input source stops");
        drain.join().expect("the drain thread ends");
        assert_eq!(taps.len(), 1, "taps of this process while the source runs");
        assert_eq!(taps[0].options, 1, "the live tap is not listen-only");
    }

    #[test]
    #[ignore = "needs Input Monitoring for the process that runs the tests"]
    fn reenable_turns_a_disabled_tap_back_on() {
        let (handle, drain) = drained_sink();
        let (_writer, reader) = foreground_state_for_test();
        let port = TapPort::unset();
        let ctx = TapContext {
            sink: handle,
            foreground: reader,
            counters: InputCounters::new(),
            port: port.clone(),
            secure: current_secure_state,
        };
        let mut tap = TapThread::spawn(
            move || install_tap(ctx, tap_settings(), SystemTapMaker),
            HoldPlan::Never,
            hold_secure_input,
            Arc::new(Mutex::new(None)),
            || {},
        )
        .expect("the tap starts");
        let live = port.port.load(Ordering::Relaxed);
        // SAFETY: `live` is the mach port of the tap that the tap thread keeps
        // until `stop_within` below.
        let enabled_at_start = unsafe { CGEventTapIsEnabled(live) };
        // SAFETY: as above.
        unsafe { CGEventTapEnable(live, false) };
        // SAFETY: as above.
        let enabled_after_disable = unsafe { CGEventTapIsEnabled(live) };
        port.reenable();
        // SAFETY: as above.
        let enabled_after_reenable = unsafe { CGEventTapIsEnabled(live) };
        tap.wait_until_its_run_loop_waits();
        tap.stop_within(INPUT_STOP_DEADLINE).expect("the tap stops");
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
        let (_writer, reader) = foreground_state_for_test();
        let mut source = InputSource::new(reader, InputCounters::new(), HoldPlan::Never);
        source
            .begin(crate::source::SourceContext::new(handle))
            .expect("the input source starts");
        let running = (own_taps().len(), tap_threads());
        source
            .tap
            .as_ref()
            .expect("a started source holds its tap thread")
            .wait_until_its_run_loop_waits();
        source.shutdown().expect("the input source stops");
        drain.join().expect("the drain thread ends");
        assert_eq!(
            running,
            (1, 1),
            "taps and tap threads while the source runs"
        );
        assert_eq!((own_taps().len(), tap_threads()), (0, 0));
    }

    fn started_source() -> (InputSource, std::thread::JoinHandle<()>, SinkHandle) {
        let (handle, drain) = drained_sink();
        let (_writer, reader) = foreground_state_for_test();
        let mut source = InputSource::new(reader, InputCounters::new(), HoldPlan::Never);
        source
            .begin(crate::source::SourceContext::new(handle.clone()))
            .expect("the input source starts");
        (source, drain, handle)
    }

    #[test]
    #[ignore = "needs Input Monitoring for the process that runs the tests"]
    fn start_stop_cycles_lose_no_stop() {
        for cycle in 0..5 {
            let (mut source, drain, handle) = started_source();
            let called = std::time::Instant::now();
            let stopped = source.shutdown();
            let took = called.elapsed();
            drop((source, handle));
            drain.join().expect("the drain thread ends");
            assert!(
                stopped.is_ok() && took <= INPUT_STOP_DEADLINE,
                "cycle {cycle}: shutdown {stopped:?} after {took:?}"
            );
            assert_eq!((own_taps().len(), tap_threads()), (0, 0), "cycle {cycle}");
        }
    }

    #[test]
    #[ignore = "needs Input Monitoring for the process that runs the tests"]
    fn a_second_start_is_refused_and_stop_leaves_no_tap() {
        let (mut source, drain, handle) = started_source();
        let second = source.begin(crate::source::SourceContext::new(handle.clone()));
        let running = (own_taps().len(), tap_threads());
        let stopped = source.shutdown();
        drop((source, handle));
        drain.join().expect("the drain thread ends");
        assert_eq!(second, Err(CaptureError::AlreadyStarted));
        assert_eq!(
            running,
            (1, 1),
            "taps and tap threads while the source runs"
        );
        assert_eq!(stopped, Ok(()));
        assert_eq!((own_taps().len(), tap_threads()), (0, 0));
    }

    #[test]
    #[ignore = "needs Input Monitoring for the process that runs the tests"]
    fn drop_without_stop_leaves_no_tap() {
        let (source, drain, handle) = started_source();
        drop((source, handle));
        drain.join().expect("the drain thread ends");
        assert_eq!((own_taps().len(), tap_threads()), (0, 0));
    }
}

#[cfg(test)]
mod secure_input_invariant {
    use super::*;
    use crate::sink;

    fn context(capacity: usize) -> (TapContext, sink::SinkWriter) {
        let (handle, writer) = sink::channel(capacity);
        let (_writer, reader) = crate::macos::foreground::foreground_state_for_test();
        let ctx = TapContext {
            sink: handle,
            foreground: reader,
            counters: InputCounters::new(),
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
        assert_eq!(ctx.counters.suppressed_keys(), 3);
        drop(ctx);
        assert!(
            events_until_the_sink_closes(&writer).is_empty(),
            "a keystroke was recorded while secure input was active"
        );
    }

    #[test]
    fn a_suppressed_key_raises_the_count_from_zero_to_one() {
        let (ctx, _writer) = context(8);
        assert_eq!(ctx.counters.suppressed_keys(), 0);
        dispatch_tap_event(&ctx, CGEventType::KeyDown, SecureInputState::Active);
        assert_eq!(ctx.counters.suppressed_keys(), 1);
        assert_eq!(ctx.counters.suppressed_keys(), 1);
    }

    #[test]
    fn pointer_events_record_under_secure_input_without_a_violation() {
        let (ctx, writer) = context(8);
        dispatch_tap_event(&ctx, CGEventType::MouseMoved, SecureInputState::Active);
        dispatch_tap_event(&ctx, CGEventType::LeftMouseDown, SecureInputState::Active);
        assert_eq!(ctx.counters.suppressed_keys(), 0);
        drop(ctx);
        let recorded = events_until_the_sink_closes(&writer);
        assert_eq!(recorded, [Modality::Pointer, Modality::Pointer]);
    }

    #[test]
    fn a_key_event_without_secure_input_records_a_keystroke() {
        let (ctx, writer) = context(8);
        dispatch_tap_event(&ctx, CGEventType::KeyDown, SecureInputState::Inactive);
        assert_eq!(ctx.counters.suppressed_keys(), 0);
        drop(ctx);
        assert_eq!(events_until_the_sink_closes(&writer), [Modality::Keystroke]);
    }
}
