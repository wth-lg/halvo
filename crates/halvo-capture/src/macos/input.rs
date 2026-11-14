use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

use core_foundation::base::TCFType;
use core_foundation::date::CFDate;
use core_foundation::runloop::{
    kCFRunLoopCommonModes, kCFRunLoopDefaultMode, CFRunLoop, CFRunLoopSource, CFRunLoopTimer,
    CFRunLoopTimerContext, CFRunLoopTimerInvalidate, CFRunLoopTimerRef,
};
use core_graphics::event::{
    CGEvent, CGEventRef, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventTapProxy, CGEventType, CallbackResult,
};

use crate::event::Modality;
use crate::macos::foreground::{emit_metadata_only, Foreground, ForegroundReader, Unpersisted};
use crate::macos::source_failure;
use crate::secure_input_macos::hold::{
    timing, Admission, Bounds, Cause, HeldKeys, Monotonic, Step, Timing, Uptime, READING_STEP,
    SESSION_STEP,
};
use crate::secure_input_macos::{
    hold_secure_input, read_session, secure_input_reading, Reading, SecureInputHold,
    SecureInputRefused, Session,
};
use crate::sink::SinkHandle;
use crate::source::{
    CaptureError, CaptureSource, HealthReason, HeldSamples, SourceContext, SourceCounters,
    SourceHealth, SourceState, Started, StopCause, SuppressedKeys,
};

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventTapEnable(tap: *mut c_void, enable: bool);
    fn CGPreflightListenEventAccess() -> bool;
    fn CGEventGetTimestamp(event: *const c_void) -> u64;
}

extern "C" {
    fn clock_gettime_nsec_np(clock: libc::clockid_t) -> u64;
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
    stalled: AtomicBool,
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

    fn count_late_key(&self) {
        self.0.late_events.fetch_add(1, Ordering::Relaxed);
    }

    fn withhold(&self) {
        self.0.keys_withheld.store(true, Ordering::Relaxed);
    }

    fn set_stalled(&self, stalled: bool) {
        self.0.stalled.store(stalled, Ordering::Relaxed);
    }

    fn stalled(&self) -> bool {
        self.0.stalled.load(Ordering::Relaxed)
    }

    /// Key presses that reached the tap while secure input was active. Each was
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

/// The flag bits of the modifier keys: the device-independent ones and those of
/// each side.
const MODIFIER_BITS: u64 = 0x00DF_0000 | 0x0000_207F;

fn modifier_pressed(flags: u64, before: Option<u64>) -> bool {
    before.is_some_and(|before| flags & !before & MODIFIER_BITS != 0)
}

/// What the source makes of an event of its mask.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A key down or the press of a modifier: held, and recorded at its release.
    Key,
    /// A key up or the release of a modifier: a reading alone, never recorded.
    Reading,
    Pointer,
    Ignore,
}

fn kind_of(etype: CGEventType, flags: u64, before: Option<u64>) -> Kind {
    match tap_class(etype) {
        Some(TapClass::KeyPress) => Kind::Key,
        Some(TapClass::OtherKey)
            if etype as u32 == CGEventType::FlagsChanged as u32
                && modifier_pressed(flags, before) =>
        {
            Kind::Key
        }
        Some(TapClass::OtherKey) => Kind::Reading,
        Some(TapClass::DiscretePointer | TapClass::ContinuousPointer) => Kind::Pointer,
        None => Kind::Ignore,
    }
}

fn events_of_interest() -> Vec<CGEventType> {
    TAP_EVENTS.iter().map(|&(etype, _)| etype).collect()
}

const TAP_LOCATION: CGEventTapLocation = CGEventTapLocation::HID;
const TAP_PLACEMENT: CGEventTapPlacement = CGEventTapPlacement::HeadInsertEventTap;
const TAP_OPTIONS: CGEventTapOptions = CGEventTapOptions::ListenOnly;

/// The two clocks of the hold: CLOCK_MONOTONIC_RAW for the bound and the gaps,
/// CLOCK_UPTIME_RAW for a key's lag against its OS time.
#[derive(Clone, Copy)]
struct Clocks {
    monotonic: fn() -> Monotonic,
    uptime: fn() -> Uptime,
}

fn monotonic_raw() -> Monotonic {
    // SAFETY: clock_gettime_nsec_np takes a clock id by value and returns a
    // number; it touches no memory of the caller.
    Monotonic(unsafe { clock_gettime_nsec_np(libc::CLOCK_MONOTONIC_RAW) })
}

fn uptime_raw() -> Uptime {
    // SAFETY: as in `monotonic_raw`.
    Uptime(unsafe { clock_gettime_nsec_np(libc::CLOCK_UPTIME_RAW) })
}

const CLOCKS: Clocks = Clocks {
    monotonic: monotonic_raw,
    uptime: uptime_raw,
};

fn os_time(event: &CGEvent) -> Option<Uptime> {
    let event: &CGEventRef = event;
    // SAFETY: a reference to a foreign-types borrowed type is the pointer of the
    // C object, here the live event that the tap hands to its callback; the
    // call only reads it.
    let stamp = unsafe { CGEventGetTimestamp(std::ptr::from_ref(event).cast()) };
    Some(Uptime(stamp))
}

struct TapSettings {
    location: CGEventTapLocation,
    placement: CGEventTapPlacement,
    options: CGEventTapOptions,
    events: Vec<CGEventType>,
    secure: fn(Session) -> Reading,
    session: fn() -> Session,
    clocks: Clocks,
}

fn tap_settings() -> TapSettings {
    TapSettings {
        location: TAP_LOCATION,
        placement: TAP_PLACEMENT,
        options: TAP_OPTIONS,
        events: events_of_interest(),
        secure: secure_input_reading,
        session: read_session,
        clocks: CLOCKS,
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

/// A held key: its own time and the app in front at its arrival, and nothing
/// of the event.
struct HeldKey {
    instant: SystemTime,
    app: Option<Foreground>,
}

/// The repeating timer of the readings, made on the tap thread.
struct ReadingTimer(CFRunLoopTimer);

// SAFETY: a ReadingTimer is made, added to a run loop and invalidated on the
// thread whose run loop runs it, which is the thread that owns the hold state
// that holds it; that state is made empty on the thread that starts the source
// and holds no timer until it is on its own thread.
unsafe impl Send for ReadingTimer {}

struct HoldState {
    keys: HeldKeys<HeldKey>,
    flags: Option<u64>,
    session: Session,
    session_at: Option<Monotonic>,
    timer: Option<ReadingTimer>,
}

type SharedHold = Arc<Mutex<HoldState>>;

fn lock(hold: &SharedHold) -> MutexGuard<'_, HoldState> {
    hold.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What the daemon's own take of secure input drops: the held keys of the run.
struct OwnTake {
    hold: SharedHold,
    counters: InputCounters,
}

impl OwnTake {
    fn taken(&self) {
        let dropped = lock(&self.hold).keys.drop_all();
        if dropped > 0 {
            self.counters.withhold();
        }
    }
}

#[derive(Clone, Copy)]
struct TimerCalls {
    make: fn(*mut c_void) -> CFRunLoopTimer,
    invalidate: fn(&CFRunLoopTimer),
}

fn reading_timer(info: *mut c_void) -> CFRunLoopTimer {
    let mut context = CFRunLoopTimerContext {
        version: 0,
        info,
        retain: None,
        release: None,
        copyDescription: None,
    };
    let step = READING_STEP.as_secs_f64();
    let timer = CFRunLoopTimer::new(
        CFDate::now().abs_time() + step,
        step,
        0,
        0,
        reading_timer_fired,
        &mut context,
    );
    add_to_this_run_loop(&timer);
    timer
}

const TIMER_CALLS: TimerCalls = TimerCalls {
    make: reading_timer,
    invalidate,
};

/// What the callback knows of one event of the tap.
#[derive(Debug, Clone, Copy)]
struct TapEvent {
    etype: CGEventType,
    flags: u64,
    timing: Timing,
}

struct TapContext {
    sink: SinkHandle,
    foreground: ForegroundReader,
    counters: InputCounters,
    port: TapPort,
    secure: fn(Session) -> Reading,
    session: fn() -> Session,
    clocks: Clocks,
    stop: Arc<AtomicBool>,
    timers: TimerCalls,
    hold: SharedHold,
}

impl TapContext {
    fn read_session(&self, state: &mut HoldState) {
        state.session = (self.session)();
        state.session_at = Some((self.clocks.monotonic)());
    }

    fn reading(&self, state: &mut HoldState, key: bool) -> (Reading, Monotonic) {
        let now = (self.clocks.monotonic)();
        let stale = state
            .session_at
            .is_none_or(|at| now < at || Duration::from_nanos(now.0 - at.0) >= SESSION_STEP);
        if stale && (key || !state.keys.is_empty()) {
            self.read_session(state);
        }
        let reading = (self.secure)(state.session);
        (reading, (self.clocks.monotonic)())
    }

    fn dropped(&self, keys: usize, cause: Cause) {
        if keys > 0 && cause.withholds() {
            self.counters.withhold();
        }
        if keys > 0 && cause.timing_fault() {
            self.counters.set_stalled(true);
        }
    }

    fn admitted(&self, admission: Admission, timing: Timing) {
        match admission {
            Admission::Held => {}
            Admission::Suppressed => self.counters.count_suppressed_key(),
            Admission::Withheld { cause, .. } => {
                self.counters.withhold();
                if cause.timing_fault() {
                    self.counters.set_stalled(true);
                }
            }
        }
        if let Timing::Late { lag } = timing {
            if lag > crate::secure_input_macos::hold::LATE_BOUND {
                self.counters.count_late_key();
            }
        }
    }

    fn release(&self, state: &mut HoldState, mut step: Step<HeldKey>) {
        loop {
            let key = match step {
                Step::Waiting => return,
                Step::Dropped { keys, cause } => return self.dropped(keys, cause),
                Step::Released(key) => key,
            };
            if self.stop.load(Ordering::SeqCst) {
                state.keys.drop_all();
                return;
            }
            let emitted = emit_metadata_only(
                &self.sink,
                Modality::Keystroke,
                key.app,
                Unpersisted::EMPTY,
                key.instant,
            );
            if emitted.is_err() {
                state.keys.drop_all();
                return;
            }
            self.counters.set_stalled(false);
            if state.keys.is_empty() {
                return;
            }
            let (reading, at) = self.reading(state, false);
            step = state.keys.reading(reading, at);
        }
    }

    fn settle_timer(&self, state: &mut HoldState) {
        match (state.keys.is_empty(), &state.timer) {
            (false, None) => {
                let info = std::ptr::from_ref(self).cast_mut().cast::<c_void>();
                state.timer = Some(ReadingTimer((self.timers.make)(info)));
            }
            (true, Some(_)) => {
                if let Some(ReadingTimer(timer)) = state.timer.take() {
                    (self.timers.invalidate)(&timer);
                }
            }
            _ => {}
        }
    }
}

impl Drop for TapContext {
    fn drop(&mut self) {
        let timer = lock(&self.hold).timer.take();
        if let Some(ReadingTimer(timer)) = timer {
            (self.timers.invalidate)(&timer);
        }
    }
}

fn on_tap_event(ctx: &TapContext, event: TapEvent) {
    let hold = ctx.hold.clone();
    let mut state = lock(&hold);
    if is_tap_disabled(event.etype) {
        let keys = state.keys.drop_all();
        ctx.dropped(keys, Cause::TapSwitchedOff);
        ctx.settle_timer(&mut state);
        drop(state);
        ctx.port.reenable();
        return;
    }
    let kind = kind_of(event.etype, event.flags, state.flags);
    state.flags = Some(event.flags);
    if kind == Kind::Ignore {
        return;
    }
    if ctx.stop.load(Ordering::SeqCst) {
        state.keys.drop_all();
    } else {
        let arrival = (ctx.clocks.monotonic)();
        let (reading, at) = ctx.reading(&mut state, kind == Kind::Key);
        let step = match (kind, event.timing) {
            (Kind::Key | Kind::Reading, Timing::Late { .. }) => {
                state.keys.late_reading(at, Cause::LateKey)
            }
            (Kind::Key | Kind::Reading, Timing::Untimed) => {
                state.keys.late_reading(at, Cause::UntimedKey)
            }
            _ => state.keys.reading(reading, at),
        };
        if kind == Kind::Key {
            let lag = match event.timing {
                Timing::InTime { lag } => lag,
                _ => Duration::ZERO,
            };
            let key = HeldKey {
                instant: SystemTime::now()
                    .checked_sub(lag)
                    .unwrap_or(SystemTime::UNIX_EPOCH),
                app: ctx.foreground.get(),
            };
            let admission = state.keys.admit(reading, event.timing, arrival, key);
            ctx.admitted(admission, event.timing);
        }
        ctx.release(&mut state, step);
    }
    ctx.settle_timer(&mut state);
    drop(state);
    if kind == Kind::Pointer {
        let _ = emit_metadata_only(
            &ctx.sink,
            Modality::Pointer,
            ctx.foreground.get(),
            Unpersisted::EMPTY,
            SystemTime::now(),
        );
    }
}

fn on_reading_tick(ctx: &TapContext) {
    let hold = ctx.hold.clone();
    let mut state = lock(&hold);
    if ctx.stop.load(Ordering::SeqCst) {
        state.keys.drop_all();
    } else {
        let (reading, at) = ctx.reading(&mut state, false);
        let step = state.keys.reading(reading, at);
        ctx.release(&mut state, step);
    }
    ctx.settle_timer(&mut state);
}

extern "C" fn reading_timer_fired(_timer: CFRunLoopTimerRef, info: *mut c_void) {
    // SAFETY: `info` is the address of the TapContext that armed this timer, in
    // the box that the tap's callback owns; that context invalidates the timer
    // in its Drop, before its fields drop, and the timer fires on the tap thread
    // alone, which reads the context only through shared references.
    let ctx = unsafe { &*info.cast::<TapContext>() };
    on_reading_tick(ctx);
}

fn tap_context(
    sink: SinkHandle,
    foreground: ForegroundReader,
    counters: InputCounters,
    settings: &TapSettings,
    bounds: Bounds,
    stop: Arc<AtomicBool>,
) -> TapContext {
    TapContext {
        sink,
        foreground,
        counters,
        port: TapPort::unset(),
        secure: settings.secure,
        session: settings.session,
        clocks: settings.clocks,
        stop,
        timers: TIMER_CALLS,
        hold: Arc::new(Mutex::new(HoldState {
            keys: HeldKeys::new(bounds),
            flags: None,
            session: Session::NoDictionary,
            session_at: None,
            timer: None,
        })),
    }
}

fn tap_event(ctx: &TapContext, etype: CGEventType, event: &CGEvent) -> TapEvent {
    let arrival = (ctx.clocks.uptime)();
    TapEvent {
        etype,
        flags: event.get_flags().bits(),
        timing: timing(arrival, os_time(event), lock(&ctx.hold).keys.bounds().gap),
    }
}

fn tap_callback(
    ctx: TapContext,
) -> impl Fn(CGEventTapProxy, CGEventType, &CGEvent) -> CallbackResult + Send + 'static {
    let ctx = Box::new(ctx);
    ctx.read_session(&mut lock(&ctx.hold));
    move |_proxy, etype, event: &CGEvent| {
        on_tap_event(&ctx, tap_event(&ctx, etype, event));
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
    own_take: Option<OwnTake>,
}

impl HoldSlot {
    fn take(&mut self) {
        match (self.hold)() {
            Ok(held) => {
                self.held = Some(held);
                if let Some(own_take) = &self.own_take {
                    own_take.taken();
                }
            }
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

struct TapPlan {
    plan: HoldPlan,
    hold: Hold,
    refusal: Arc<Mutex<Option<SecureInputRefused>>>,
    own_take: Option<OwnTake>,
}

impl TapThread {
    fn spawn<K: 'static>(
        install: impl FnOnce() -> Result<K, CaptureError> + Send + 'static,
        tap_plan: TapPlan,
        stop: Arc<AtomicBool>,
        before_run: impl FnOnce() + Send + 'static,
    ) -> Result<Self, CaptureError> {
        let (loop_tx, loop_rx) = channel::<Result<CFRunLoop, CaptureError>>();
        let thread = {
            let stop = stop.clone();
            std::thread::Builder::new()
                .name("halvo-input-tap".into())
                .spawn(move || {
                    let TapPlan {
                        plan,
                        hold,
                        refusal,
                        own_take,
                    } = tap_plan;
                    let mut slot = TimedHold::new(HoldSlot {
                        hold,
                        held: None,
                        refusal,
                        own_take,
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
    bounds: Bounds,
    session: fn() -> Session,
    refusal: Arc<Mutex<Option<SecureInputRefused>>>,
    tap: Option<TapThread>,
    failure: Option<CaptureError>,
    stopped: bool,
}

impl InputSource {
    pub fn new(foreground: ForegroundReader, counters: InputCounters, plan: HoldPlan) -> Self {
        Self::with_bounds(foreground, counters, plan, Bounds::PRODUCT)
    }

    /// A source whose keys are held for `bound` in place of the product's
    /// bound, for tests in a process of their own. A build without debug
    /// assertions has no such constructor.
    #[cfg(debug_assertions)]
    pub fn with_hold_bound(
        foreground: ForegroundReader,
        counters: InputCounters,
        plan: HoldPlan,
        bound: Duration,
    ) -> Self {
        Self::with_bounds(
            foreground,
            counters,
            plan,
            Bounds {
                hold: bound,
                ..Bounds::PRODUCT
            },
        )
    }

    /// A source with a shorter bound whose session is read by `session` in
    /// place of the system's session: for the hosted stall tests alone, whose
    /// runner may have no session at the console. Only a test build of this
    /// crate, which turns the feature `stall-seam` on, has it.
    #[cfg(all(debug_assertions, feature = "stall-seam"))]
    pub fn with_hold_bound_and_session(
        foreground: ForegroundReader,
        counters: InputCounters,
        plan: HoldPlan,
        bound: Duration,
        session: fn() -> Session,
    ) -> Self {
        let mut source = Self::with_hold_bound(foreground, counters, plan, bound);
        source.session = session;
        source
    }

    fn with_bounds(
        foreground: ForegroundReader,
        counters: InputCounters,
        plan: HoldPlan,
        bounds: Bounds,
    ) -> Self {
        Self {
            foreground,
            counters,
            plan,
            hold: hold_secure_input,
            bounds,
            session: read_session,
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
        self.counters.set_stalled(false);
        let settings = TapSettings {
            session: self.session,
            ..tap_settings()
        };
        let stop = Arc::new(AtomicBool::new(false));
        let ctx = tap_context(
            sink,
            self.foreground.clone(),
            self.counters.clone(),
            &settings,
            self.bounds,
            stop.clone(),
        );
        let own_take = OwnTake {
            hold: ctx.hold.clone(),
            counters: self.counters.clone(),
        };
        let started = TapThread::spawn(
            move || install_tap(ctx, settings, maker),
            TapPlan {
                plan: self.plan,
                hold: self.hold,
                refusal: self.refusal.clone(),
                own_take: Some(own_take),
            },
            stop,
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
            (Some(_), _) if self.counters.stalled() => SourceState::Impaired {
                reason: HealthReason::RunLoopStalled,
            },
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

#[allow(unexpected_cfgs)]
mod proofs {
    #[cfg(halvo_proof = "held_key_printed")]
    fn held_key_printed(key: &super::HeldKey) -> String {
        format!("{key:?}")
    }

    #[cfg(halvo_proof = "held_key_printed_twin")]
    fn held_key_printed_twin(key: &super::HeldKey) -> String {
        format!("{:?}", key.instant)
    }

    #[cfg(halvo_proof = "hold_bound_in_a_release_build")]
    fn hold_bound_in_a_release_build(
        foreground: crate::macos::foreground::ForegroundReader,
    ) -> super::InputSource {
        super::InputSource::with_hold_bound(
            foreground,
            super::InputCounters::new(),
            super::HoldPlan::Never,
            std::time::Duration::ZERO,
        )
    }

    #[cfg(halvo_proof = "hold_bound_in_a_release_build_twin")]
    fn hold_bound_in_a_release_build_twin(
        foreground: crate::macos::foreground::ForegroundReader,
    ) -> super::InputSource {
        super::InputSource::new(
            foreground,
            super::InputCounters::new(),
            super::HoldPlan::Never,
        )
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
    use crate::secure_input_macos::hold::Timing;
    use crate::secure_input_macos::test_support::{
        agreed_reading, one_hold_at_a_time, reads_on_within,
    };
    use crate::sink;
    use core_foundation::date::CFDate;
    use core_foundation::runloop::{CFRunLoopTimer, CFRunLoopTimerRef};
    use core_graphics::event::CGEventFlags;
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
    use std::cell::{Cell, RefCell};
    use std::sync::atomic::AtomicUsize;
    use std::sync::mpsc::RecvTimeoutError;
    use std::sync::{Mutex, PoisonError};
    use std::time::Duration;

    extern "C" {
        fn CGEventSetTimestamp(event: *const c_void, timestamp: u64);
    }

    const MS: u64 = 1_000_000;
    const START: u64 = 1_000_000 * MS;

    thread_local! {
        static ANSWER: Cell<Reading> = const { Cell::new(Reading::Off) };
        static ANSWERS: RefCell<Vec<Reading>> = const { RefCell::new(Vec::new()) };
        static ASKED: Cell<usize> = const { Cell::new(0) };
        static SESSION: Cell<Session> = const { Cell::new(Session::AtConsole) };
        static SESSION_READS: Cell<usize> = const { Cell::new(0) };
        static MONOTONIC: Cell<u64> = const { Cell::new(START) };
        static UPTIME: Cell<u64> = const { Cell::new(START) };
        static AUTO_STEP: Cell<u64> = const { Cell::new(0) };
        static STOP_AT_ASK: RefCell<Option<(usize, Arc<AtomicBool>)>> = const { RefCell::new(None) };
        static JUMP_AT_ASK: Cell<Option<usize>> = const { Cell::new(None) };
    }

    fn stand_in_reader(session: Session) -> Reading {
        let asked = ASKED.with(|asked| {
            asked.set(asked.get() + 1);
            asked.get()
        });
        STOP_AT_ASK.with(|stop| {
            if let Some((at, flag)) = &*stop.borrow() {
                if *at == asked {
                    flag.store(true, Ordering::SeqCst);
                }
            }
        });
        if JUMP_AT_ASK.with(Cell::get) == Some(asked) {
            MONOTONIC.with(|now| now.set(now.get() + 40 * MS));
        }
        if session != Session::AtConsole {
            return Reading::Unknown;
        }
        ANSWERS.with(|answers| {
            let mut answers = answers.borrow_mut();
            if answers.is_empty() {
                ANSWER.with(Cell::get)
            } else {
                answers.remove(0)
            }
        })
    }

    fn stand_in_session() -> Session {
        SESSION_READS.with(|reads| reads.set(reads.get() + 1));
        SESSION.with(Cell::get)
    }

    fn stand_in_monotonic() -> Monotonic {
        let step = AUTO_STEP.with(Cell::get);
        MONOTONIC.with(|now| {
            now.set(now.get() + step);
            Monotonic(now.get())
        })
    }

    fn stand_in_uptime() -> Uptime {
        Uptime(UPTIME.with(Cell::get))
    }

    const STAND_IN_CLOCKS: Clocks = Clocks {
        monotonic: stand_in_monotonic,
        uptime: stand_in_uptime,
    };

    fn advance(ms: u64) {
        MONOTONIC.with(|now| now.set(now.get() + ms * MS));
        UPTIME.with(|now| now.set(now.get() + ms * MS));
    }

    fn answer(reading: Reading) {
        ANSWER.with(|answer| answer.set(reading));
    }

    fn asked() -> usize {
        ASKED.with(Cell::get)
    }

    fn reset_stand_ins() {
        answer(Reading::Off);
        ANSWERS.with(|answers| answers.borrow_mut().clear());
        ASKED.with(|asked| asked.set(0));
        SESSION.with(|session| session.set(Session::AtConsole));
        SESSION_READS.with(|reads| reads.set(0));
        MONOTONIC.with(|now| now.set(START));
        UPTIME.with(|now| now.set(START));
        AUTO_STEP.with(|step| step.set(0));
        STOP_AT_ASK.with(|stop| *stop.borrow_mut() = None);
        JUMP_AT_ASK.with(|jump| jump.set(None));
    }

    fn stand_in_settings() -> TapSettings {
        TapSettings {
            secure: stand_in_reader,
            session: stand_in_session,
            clocks: STAND_IN_CLOCKS,
            ..tap_settings()
        }
    }

    struct Held {
        ctx: Box<TapContext>,
        writer: sink::SinkWriter,
        counters: InputCounters,
        fg_writer: crate::macos::foreground::ForegroundWriter,
    }

    fn held_with(bound_ms: u64, cap: usize, capacity: usize) -> Held {
        reset_stand_ins();
        let (handle, writer) = sink::channel(capacity);
        let (fg_writer, reader) = foreground_state_for_test();
        let counters = InputCounters::new();
        let ctx = Box::new(tap_context(
            handle,
            reader,
            counters.clone(),
            &stand_in_settings(),
            Bounds {
                hold: Duration::from_millis(bound_ms),
                gap: Duration::from_millis(30),
                cap,
            },
            Arc::new(AtomicBool::new(false)),
        ));
        ctx.read_session(&mut lock(&ctx.hold));
        Held {
            ctx,
            writer,
            counters,
            fg_writer,
        }
    }

    fn held(bound_ms: u64) -> Held {
        held_with(bound_ms, 64, 64)
    }

    fn event(etype: CGEventType) -> TapEvent {
        TapEvent {
            etype,
            flags: 0,
            timing: Timing::InTime {
                lag: Duration::ZERO,
            },
        }
    }

    fn with_flags(etype: CGEventType, flags: u64) -> TapEvent {
        TapEvent {
            flags,
            ..event(etype)
        }
    }

    fn key_down(ctx: &TapContext) {
        advance(1);
        on_tap_event(ctx, event(CGEventType::KeyDown));
    }

    fn tick(ctx: &TapContext) {
        advance(10);
        on_reading_tick(ctx);
    }

    fn ticks(ctx: &TapContext, ms: u64) {
        for _ in 0..ms / 10 {
            tick(ctx);
        }
    }

    fn received(writer: &sink::SinkWriter) -> Vec<crate::sink::Received> {
        std::iter::from_fn(|| writer.try_recv().ok()).collect()
    }

    fn keystrokes(writer: &sink::SinkWriter) -> usize {
        received(writer)
            .iter()
            .filter(|received| received.event().modality == Modality::Keystroke)
            .count()
    }

    fn held_now(ctx: &TapContext) -> usize {
        lock(&ctx.hold).keys.len()
    }

    #[test]
    fn the_source_holds_a_key_down_and_a_modifier_press_and_nothing_else_of_the_keys() {
        let shift = CGEventFlags::CGEventFlagShift.bits();
        let left_shift = 0x2;
        let command = CGEventFlags::CGEventFlagCommand.bits();
        let flags = CGEventType::FlagsChanged;
        assert_eq!(kind_of(CGEventType::KeyDown, 0, None), Kind::Key);
        assert_eq!(kind_of(CGEventType::KeyUp, 0, Some(0)), Kind::Reading);
        assert_eq!(
            kind_of(flags, shift | left_shift, Some(0)),
            Kind::Key,
            "a shift press"
        );
        assert_eq!(
            kind_of(flags, command | shift, Some(shift)),
            Kind::Key,
            "a command press while shift is down"
        );
        assert_eq!(
            kind_of(flags, 0, Some(shift | left_shift)),
            Kind::Reading,
            "a shift release"
        );
        assert_eq!(
            kind_of(flags, shift, Some(command | shift)),
            Kind::Reading,
            "a command release while shift stays down"
        );
        assert_eq!(
            kind_of(flags, shift | 0x4, Some(shift | left_shift)),
            Kind::Key,
            "the right shift pressed while the left one is down"
        );
        assert_eq!(
            kind_of(flags, 0x100, Some(0)),
            Kind::Reading,
            "a bit that is no modifier"
        );
    }

    #[test]
    fn a_flags_change_with_no_event_before_it_is_no_press() {
        let shift = CGEventFlags::CGEventFlagShift.bits();
        assert_eq!(
            kind_of(CGEventType::FlagsChanged, shift, None),
            Kind::Reading
        );
    }

    #[test]
    fn pointer_events_are_recorded_and_the_tap_notices_are_no_event_of_the_mask() {
        for &(etype, class) in &TAP_EVENTS {
            if matches!(
                class,
                TapClass::DiscretePointer | TapClass::ContinuousPointer
            ) {
                assert_eq!(kind_of(etype, 0, Some(0)), Kind::Pointer, "{etype:?}");
            }
        }
        for etype in [
            CGEventType::TapDisabledByTimeout,
            CGEventType::TapDisabledByUserInput,
        ] {
            assert_eq!(kind_of(etype, 0, Some(0)), Kind::Ignore);
            assert!(is_tap_disabled(etype));
        }
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
    fn the_tap_listens_for_exactly_the_types_that_it_holds_records_or_reads() {
        let mask = events_of_interest();
        for etype in EVERY_EVENT_TYPE {
            let masked = mask.iter().any(|&listed| listed as u32 == etype as u32);
            let handled = kind_of(etype, 0, Some(0)) != Kind::Ignore;
            assert_eq!(
                masked, handled,
                "{etype:?}: masked {masked}, handled {handled}"
            );
        }
    }

    #[test]
    fn the_tap_mask_is_0x0e401cfe() {
        let mask = events_of_interest()
            .iter()
            .fold(0u64, |mask, &etype| mask | 1 << (etype as u64));
        assert_eq!(mask, 0x0E40_1CFE);
    }

    #[test]
    fn a_key_read_off_is_recorded_only_when_it_is_as_old_as_the_bound() {
        let held = held(1_000);
        key_down(&held.ctx);
        ticks(&held.ctx, 990);
        assert_eq!(keystrokes(&held.writer), 0, "recorded before the bound");
        tick(&held.ctx);
        assert_eq!(keystrokes(&held.writer), 1, "recorded at the bound");
        assert_eq!(held_now(&held.ctx), 0);
    }

    #[test]
    fn a_key_read_as_secure_counts_one_suppression_and_is_never_recorded() {
        let held = held(1_000);
        answer(Reading::On);
        key_down(&held.ctx);
        answer(Reading::Off);
        ticks(&held.ctx, 2_000);
        assert_eq!(held.counters.suppressed_keys(), 1);
        assert_eq!(keystrokes(&held.writer), 0);
    }

    #[test]
    fn a_key_down_read_off_is_dropped_when_the_key_up_after_it_reads_on() {
        let held = held(1_000);
        key_down(&held.ctx);
        answer(Reading::On);
        advance(1);
        on_tap_event(&held.ctx, event(CGEventType::KeyUp));
        answer(Reading::Off);
        ticks(&held.ctx, 2_000);
        assert_eq!(
            (keystrokes(&held.writer), held.counters.suppressed_keys()),
            (0, 0),
            "a key down read off, then a key up read on: (recorded, suppressed)"
        );
        assert!(held.counters.figures().keys_withheld);
    }

    #[test]
    fn three_key_downs_read_off_on_on_record_nothing_and_count_two_suppressions() {
        let held = held(1_000);
        ANSWERS.with(|answers| {
            *answers.borrow_mut() = vec![Reading::Off, Reading::On, Reading::On];
        });
        for _ in 0..3 {
            key_down(&held.ctx);
        }
        assert_eq!(asked(), 3, "readings of the three key downs");
        ticks(&held.ctx, 2_000);
        assert_eq!(
            (keystrokes(&held.writer), held.counters.suppressed_keys()),
            (0, 2)
        );
    }

    #[test]
    fn a_key_up_and_a_modifier_release_read_off_are_never_recorded() {
        let held = held(1_000);
        let shift = CGEventFlags::CGEventFlagShift.bits();
        on_tap_event(&held.ctx, with_flags(CGEventType::FlagsChanged, shift));
        advance(1);
        on_tap_event(&held.ctx, with_flags(CGEventType::FlagsChanged, 0));
        advance(1);
        on_tap_event(&held.ctx, event(CGEventType::KeyUp));
        ticks(&held.ctx, 2_000);
        assert_eq!(keystrokes(&held.writer), 0);
    }

    #[test]
    fn a_modifier_press_read_off_is_held_and_recorded_at_the_bound() {
        let held = held(1_000);
        let shift = CGEventFlags::CGEventFlagShift.bits();
        on_tap_event(&held.ctx, with_flags(CGEventType::KeyUp, 0));
        advance(1);
        on_tap_event(&held.ctx, with_flags(CGEventType::FlagsChanged, shift));
        assert_eq!(held_now(&held.ctx), 1);
        ticks(&held.ctx, 1_010);
        assert_eq!(keystrokes(&held.writer), 1);
    }

    fn figures_but_withheld(counters: &InputCounters) -> SourceCounters {
        SourceCounters {
            keys_withheld: false,
            ..counters.figures()
        }
    }

    #[test]
    fn a_tick_that_reads_on_drops_every_held_key_and_counts_nothing() {
        let held = held(1_000);
        for _ in 0..3 {
            key_down(&held.ctx);
        }
        let before = held.counters.figures();
        answer(Reading::On);
        tick(&held.ctx);
        answer(Reading::Off);
        ticks(&held.ctx, 2_000);
        assert_eq!(keystrokes(&held.writer), 0);
        assert!(held.counters.figures().keys_withheld);
        assert_eq!(figures_but_withheld(&held.counters), before);
    }

    #[test]
    fn a_session_that_leaves_the_console_drops_every_held_key() {
        let held = held(1_000);
        key_down(&held.ctx);
        SESSION.with(|session| session.set(Session::NotAtConsole));
        ticks(&held.ctx, 2_000);
        assert_eq!(keystrokes(&held.writer), 0);
        assert!(held.counters.figures().keys_withheld);
    }

    #[test]
    fn while_the_session_reads_unknown_no_key_is_held() {
        let held = held(1_000);
        SESSION.with(|session| session.set(Session::NoDictionary));
        ticks(&held.ctx, 300);
        key_down(&held.ctx);
        assert_eq!(held_now(&held.ctx), 0);
        assert!(held.counters.figures().keys_withheld);
        assert_eq!(held.counters.suppressed_keys(), 0);
    }

    #[test]
    fn the_session_is_read_at_the_start_and_again_after_its_step_while_keys_are_held() {
        let held = held(10_000);
        let reads = || SESSION_READS.with(Cell::get);
        assert_eq!(reads(), 1, "at the start");
        advance(1_000);
        on_tap_event(&held.ctx, event(CGEventType::MouseMoved));
        assert_eq!(reads(), 1, "no key held: no read");
        key_down(&held.ctx);
        assert_eq!(reads(), 2, "a key arrives after the step: one read");
        ticks(&held.ctx, 240);
        assert_eq!(reads(), 2, "inside the step");
        ticks(&held.ctx, 10);
        assert_eq!(reads(), 3, "at the step");
        ticks(&held.ctx, 1_000);
        assert_eq!(reads(), 7, "every 250 ms while held");
    }

    #[test]
    fn a_gap_between_two_readings_drops_every_held_key_and_stalls_the_source_until_a_release() {
        let held = held(1_000);
        key_down(&held.ctx);
        advance(31);
        on_reading_tick(&held.ctx);
        assert_eq!(held_now(&held.ctx), 0, "a gap of 31 ms");
        assert!(held.counters.stalled());
        assert!(held.counters.figures().keys_withheld);
        key_down(&held.ctx);
        ticks(&held.ctx, 990);
        assert!(held.counters.stalled(), "no key released yet");
        tick(&held.ctx);
        assert_eq!(keystrokes(&held.writer), 1);
        assert!(!held.counters.stalled(), "a key was released");
    }

    #[test]
    fn a_key_that_arrives_after_a_gap_drops_the_keys_held_before_it() {
        let held = held(1_000);
        key_down(&held.ctx);
        advance(31);
        on_tap_event(&held.ctx, event(CGEventType::KeyDown));
        assert_eq!(
            held_now(&held.ctx),
            1,
            "the key that came after the gap alone"
        );
        assert!(held.counters.stalled());
        ticks(&held.ctx, 1_010);
        assert_eq!(keystrokes(&held.writer), 1);
    }

    #[test]
    fn a_gap_of_the_bound_keeps_the_held_keys() {
        let held = held(1_000);
        key_down(&held.ctx);
        advance(30);
        on_reading_tick(&held.ctx);
        assert_eq!(held_now(&held.ctx), 1);
        assert!(!held.counters.stalled());
    }

    #[test]
    fn the_gap_is_counted_on_the_monotonic_clock() {
        let held = held(1_000);
        key_down(&held.ctx);
        MONOTONIC.with(|now| now.set(now.get() + 40 * MS));
        on_reading_tick(&held.ctx);
        assert_eq!(
            held_now(&held.ctx),
            0,
            "40 ms passed on the monotonic clock"
        );
        let held_too = self::held(1_000);
        key_down(&held_too.ctx);
        UPTIME.with(|now| now.set(now.get() + 40 * MS));
        MONOTONIC.with(|now| now.set(now.get() + 5 * MS));
        on_reading_tick(&held_too.ctx);
        assert_eq!(
            held_now(&held_too.ctx),
            1,
            "40 ms passed on the uptime clock alone"
        );
    }

    #[test]
    fn a_late_key_drops_every_held_key_and_a_lag_above_250_ms_counts_as_late() {
        let held = held(1_000);
        key_down(&held.ctx);
        advance(1);
        on_tap_event(
            &held.ctx,
            TapEvent {
                timing: Timing::Late {
                    lag: Duration::from_millis(31),
                },
                ..event(CGEventType::KeyDown)
            },
        );
        assert_eq!(held_now(&held.ctx), 0);
        assert!(held.counters.stalled());
        assert_eq!(held.counters.figures().late_events, 0);
        advance(1);
        on_tap_event(
            &held.ctx,
            TapEvent {
                timing: Timing::Late {
                    lag: Duration::from_millis(251),
                },
                ..event(CGEventType::KeyDown)
            },
        );
        assert_eq!(held.counters.figures().late_events, 1);
        ticks(&held.ctx, 2_000);
        assert_eq!(keystrokes(&held.writer), 0);
    }

    #[test]
    fn an_untimed_key_drops_every_held_key() {
        let held = held(1_000);
        key_down(&held.ctx);
        advance(1);
        on_tap_event(
            &held.ctx,
            TapEvent {
                timing: Timing::Untimed,
                ..event(CGEventType::KeyDown)
            },
        );
        assert_eq!(held_now(&held.ctx), 0);
        assert!(held.counters.figures().keys_withheld);
        ticks(&held.ctx, 2_000);
        assert_eq!(keystrokes(&held.writer), 0);
    }

    #[test]
    fn the_callback_reads_an_events_os_time_against_the_uptime_clock() {
        let now = START;
        for (name, stamp, withheld) in [
            ("31 ms late", now - 31 * MS, true),
            ("a time of 0", 0, true),
            ("after the arrival", now + 1, true),
            ("30 ms late", now - 30 * MS, false),
        ] {
            let held = held(1_000);
            let callback = tap_callback(*held.ctx);
            let event = event_object(CGEventType::KeyDown, 0);
            set_stamp(&event, stamp);
            let _ = callback(std::ptr::null(), CGEventType::KeyDown, &event);
            assert_eq!(held.counters.figures().keys_withheld, withheld, "{name}");
        }
    }

    #[test]
    fn a_switched_off_tap_drops_every_held_key_before_it_is_switched_on_again() {
        ENABLE_CALLS.with(|calls| calls.borrow_mut().clear());
        let mut held = held(1_000);
        held.ctx.port = TapPort::with_enable(record_enable);
        let tap = std::ptr::NonNull::<c_void>::dangling().as_ptr();
        held.ctx.port.store(tap);
        for etype in [
            CGEventType::TapDisabledByTimeout,
            CGEventType::TapDisabledByUserInput,
        ] {
            key_down(&held.ctx);
            on_tap_event(&held.ctx, event(etype));
            assert_eq!(held_now(&held.ctx), 0, "{etype:?}");
        }
        ENABLE_CALLS.with(|calls| {
            assert_eq!(
                *calls.borrow(),
                [(tap as usize, true), (tap as usize, true)]
            );
        });
        assert!(held.counters.stalled());
        assert!(held.counters.figures().keys_withheld);
        ticks(&held.ctx, 2_000);
        assert_eq!(keystrokes(&held.writer), 0);
    }

    #[test]
    fn the_daemons_own_take_drops_every_held_key() {
        let held = held(1_000);
        key_down(&held.ctx);
        key_down(&held.ctx);
        let mut slot = HoldSlot {
            hold: || crate::secure_input_macos::hold_with(|| 0, || 0),
            held: None,
            refusal: Arc::new(Mutex::new(None)),
            own_take: Some(OwnTake {
                hold: held.ctx.hold.clone(),
                counters: held.counters.clone(),
            }),
        };
        slot.take();
        assert_eq!(held_now(&held.ctx), 0);
        assert!(held.counters.figures().keys_withheld);
        drop(slot);
        ticks(&held.ctx, 2_000);
        assert_eq!(keystrokes(&held.writer), 0);
    }

    #[test]
    fn a_stop_empties_the_hold_records_nothing_and_withholds_nothing() {
        let held = held(1_000);
        key_down(&held.ctx);
        held.ctx.stop.store(true, Ordering::SeqCst);
        ticks(&held.ctx, 2_000);
        assert_eq!(keystrokes(&held.writer), 0);
        assert_eq!(held_now(&held.ctx), 0);
        assert!(!held.counters.figures().keys_withheld);
    }

    #[test]
    fn a_stop_asked_between_two_releases_ends_the_release() {
        let held = held(1_000);
        key_down(&held.ctx);
        key_down(&held.ctx);
        ticks(&held.ctx, 990);
        let asked_before = asked();
        STOP_AT_ASK
            .with(|stop| *stop.borrow_mut() = Some((asked_before + 2, held.ctx.stop.clone())));
        tick(&held.ctx);
        assert_eq!(
            keystrokes(&held.writer),
            1,
            "the key released before the stop"
        );
        assert_eq!(held_now(&held.ctx), 0);
        assert!(!held.counters.figures().keys_withheld);
    }

    #[test]
    fn each_release_takes_a_reading_of_its_own() {
        let held = held(1_000);
        key_down(&held.ctx);
        key_down(&held.ctx);
        key_down(&held.ctx);
        ticks(&held.ctx, 990);
        let before = asked();
        ANSWERS
            .with(|answers| *answers.borrow_mut() = vec![Reading::Off, Reading::Off, Reading::On]);
        tick(&held.ctx);
        assert_eq!(asked() - before, 3, "a reading for each release");
        assert_eq!(keystrokes(&held.writer), 2, "the third reading read on");
        assert_eq!(held_now(&held.ctx), 0);
    }

    #[test]
    fn a_reading_after_a_release_that_finds_a_gap_drops_the_keys_behind_it() {
        let held = held(1_000);
        key_down(&held.ctx);
        key_down(&held.ctx);
        ticks(&held.ctx, 990);
        JUMP_AT_ASK.with(|jump| jump.set(Some(asked() + 2)));
        tick(&held.ctx);
        assert_eq!(keystrokes(&held.writer), 1);
        assert_eq!(held_now(&held.ctx), 0);
        assert!(held.counters.stalled());
    }

    #[test]
    fn a_full_hold_drops_every_held_key_and_the_key_that_found_it_full() {
        let held = held_with(1_000, 3, 64);
        for _ in 0..3 {
            key_down(&held.ctx);
        }
        assert_eq!(held_now(&held.ctx), 3);
        key_down(&held.ctx);
        assert_eq!(held_now(&held.ctx), 0);
        assert!(held.counters.stalled());
        ticks(&held.ctx, 2_000);
        assert_eq!(keystrokes(&held.writer), 0);
    }

    #[test]
    fn a_refused_emit_drops_every_held_key_and_withholds_nothing() {
        let held = held(1_000);
        key_down(&held.ctx);
        key_down(&held.ctx);
        let Held {
            ctx,
            writer,
            counters,
            fg_writer,
        } = held;
        drop(writer);
        ticks(&ctx, 990);
        assert_eq!(held_now(&ctx), 2, "before the bound");
        tick(&ctx);
        assert_eq!(
            held_now(&ctx),
            0,
            "the first refused emit drops the key behind it"
        );
        assert!(!counters.figures().keys_withheld);
        drop((ctx, fg_writer));
    }

    #[test]
    fn a_blocked_emit_ends_as_a_gap_that_drops_the_keys_behind_it() {
        let held = held_with(0, 64, 0);
        let mut ctx = held.ctx;
        ctx.clocks = CLOCKS;
        let writer = held.writer;
        let reader = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            let mut kinds = Vec::new();
            while let Ok(received) = writer.recv_timeout(Duration::from_millis(500)) {
                kinds.push(received.event().modality);
            }
            kinds
        });
        on_tap_event(&ctx, event(CGEventType::KeyDown));
        on_tap_event(&ctx, event(CGEventType::KeyDown));
        assert_eq!(
            held_now(&ctx),
            0,
            "the reading after the blocked emit found a gap"
        );
        assert!(held.counters.stalled());
        assert!(held.counters.figures().keys_withheld);
        drop((ctx, held.fg_writer));
        assert_eq!(
            reader.join().expect("the reader ends"),
            [Modality::Keystroke]
        );
    }

    #[test]
    fn a_released_key_carries_its_own_instant_the_app_of_its_arrival_and_a_number_of_its_release() {
        let held = held(1_000);
        held.fg_writer
            .set_for_test(Some(crate::macos::foreground::tests_support::front(
                "com.example.editor",
                41,
            )));
        let before_arrival = SystemTime::now();
        advance(1);
        on_tap_event(
            &held.ctx,
            TapEvent {
                timing: Timing::InTime {
                    lag: Duration::from_millis(20),
                },
                ..event(CGEventType::KeyDown)
            },
        );
        held.fg_writer
            .set_for_test(Some(crate::macos::foreground::tests_support::front(
                "com.example.other",
                42,
            )));
        advance(1);
        on_tap_event(&held.ctx, event(CGEventType::MouseMoved));
        ticks(&held.ctx, 1_010);
        let records: Vec<(
            Modality,
            String,
            SystemTime,
            crate::event::MonotonicSequence,
        )> = received(&held.writer)
            .iter()
            .map(|r| {
                let e = r.event();
                (
                    e.modality,
                    e.foreground_app.clone(),
                    e.captured_at.wall_clock,
                    e.captured_at.sequence,
                )
            })
            .collect();
        assert_eq!(records.len(), 2);
        let (pointer, key) = (&records[0], &records[1]);
        assert_eq!(
            (pointer.0, pointer.1.as_str()),
            (Modality::Pointer, "com.example.other")
        );
        assert_eq!(
            (key.0, key.1.as_str()),
            (Modality::Keystroke, "com.example.editor")
        );
        assert!(
            key.2 <= before_arrival - Duration::from_millis(19),
            "the key's instant is its own time, its arrival less its lag"
        );
        assert!(pointer.3 < key.3, "the key takes its number at its release");
    }

    #[test]
    fn a_held_key_holds_its_time_and_app_alone() {
        struct Mirror {
            _instant: SystemTime,
            _app: Option<Foreground>,
        }
        assert_eq!(
            std::mem::size_of::<HeldKey>(),
            std::mem::size_of::<Mirror>()
        );
    }

    #[test]
    fn the_held_keys_at_their_cap_stay_under_the_bytes_of_the_ledger() {
        const LEDGER_BYTES: usize = 4_718_592;
        const SLOT_ALLOWANCE: usize = 128;
        let slot = crate::secure_input_macos::hold::slot_bytes::<HeldKey>();
        assert!(slot <= SLOT_ALLOWANCE, "a slot of {slot} bytes");
        let bytes = crate::secure_input_macos::hold::QUEUE_CAP
            * (SLOT_ALLOWANCE + crate::floor::text::TEXT_MAX_BYTES);
        assert_eq!(bytes, LEDGER_BYTES);
    }

    thread_local! {
        static READ_TIMES: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
    }

    fn timed_reader(session: Session) -> Reading {
        let reading = secure_input_reading(session);
        READ_TIMES.with(|times| times.borrow_mut().push(monotonic_raw().0));
        reading
    }

    fn per_call_ns(calls: u32, call: impl Fn()) -> u128 {
        let started = Instant::now();
        for _ in 0..calls {
            call();
        }
        started.elapsed().as_nanos() / u128::from(calls)
    }

    #[test]
    #[ignore = "a measurement of MA-12 F3: run it through the timed helper"]
    fn measure_the_readings_of_the_hold() {
        let session = read_session();
        let read_ns = per_call_ns(10_000, || {
            let _ = secure_input_reading(Session::AtConsole);
        });
        let session_ns = per_call_ns(1_000, || {
            let _ = read_session();
        });
        let (handle, writer) = sink::channel(64);
        let (_fg_writer, reader) = foreground_state_for_test();
        let settings = TapSettings {
            secure: timed_reader,
            ..tap_settings()
        };
        let ctx = tap_context(
            handle,
            reader,
            InputCounters::new(),
            &settings,
            Bounds {
                hold: Duration::from_secs(60),
                ..Bounds::PRODUCT
            },
            Arc::new(AtomicBool::new(false)),
        );
        ctx.read_session(&mut lock(&ctx.hold));
        let ctx = Box::new(ctx);
        let callback_event = TapEvent {
            etype: CGEventType::KeyDown,
            flags: 0,
            timing: Timing::InTime {
                lag: Duration::ZERO,
            },
        };
        on_tap_event(&ctx, callback_event);
        READ_TIMES.with(|times| times.borrow_mut().clear());
        let window = Duration::from_secs(5);
        // SAFETY: `kCFRunLoopDefaultMode` is a constant CFString that
        // CoreFoundation exports and never writes.
        CFRunLoop::run_in_mode(unsafe { kCFRunLoopDefaultMode }, window, false);
        let held = held_now(&ctx);
        drop(ctx);
        drop(writer);
        let times = READ_TIMES.with(|times| times.borrow().clone());
        let mut spacing: Vec<u64> = times.windows(2).map(|two| two[1] - two[0]).collect();
        spacing.sort_unstable();
        let at = |share: f64| {
            spacing
                .get(((spacing.len() as f64 - 1.0) * share).round() as usize)
                .map_or(0, |ns| ns / 1_000)
        };
        let over_gap = spacing.iter().filter(|&&ns| ns > 30 * MS).count();
        println!(
            "measure dictionary={} on_console={} read_ns={read_ns} session_ns={session_ns} readings={} \
             wakeups_per_s={:.1} spacing_p50_us={} spacing_p99_us={} spacing_max_us={} \
             gaps_over_30ms={over_gap} key_still_held={held}",
            u8::from(session != Session::NoDictionary),
            u8::from(session == Session::AtConsole),
            times.len(),
            times.len() as f64 / window.as_secs_f64(),
            at(0.5),
            at(0.99),
            spacing.last().map_or(0, |ns| ns / 1_000)
        );
    }

    #[test]
    fn the_reading_timer_exists_only_while_keys_are_held() {
        let held = held(1_000);
        assert!(lock(&held.ctx.hold).timer.is_none());
        key_down(&held.ctx);
        assert!(lock(&held.ctx.hold).timer.is_some());
        ticks(&held.ctx, 1_010);
        assert_eq!(keystrokes(&held.writer), 1);
        assert!(lock(&held.ctx.hold).timer.is_none());
    }

    #[test]
    fn the_reading_timer_on_the_run_loop_of_the_thread_releases_a_due_key() {
        let held = held(20);
        AUTO_STEP.with(|step| step.set(5 * MS));
        on_tap_event(&held.ctx, event(CGEventType::KeyDown));
        let started = Instant::now();
        while keystrokes_seen(&held.writer) == 0 {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "no release in 5 s"
            );
            // SAFETY: `kCFRunLoopDefaultMode` is a constant CFString that
            // CoreFoundation exports and never writes.
            CFRunLoop::run_in_mode(
                unsafe { kCFRunLoopDefaultMode },
                Duration::from_millis(20),
                false,
            );
        }
        assert!(lock(&held.ctx.hold).timer.is_none());
    }

    fn keystrokes_seen(writer: &sink::SinkWriter) -> usize {
        keystrokes(writer)
    }

    static INVALIDATED_WITH_THE_SINK_OPEN: Mutex<Vec<bool>> = Mutex::new(Vec::new());
    static SINK_PROBE: Mutex<Option<sink::SinkWriter>> = Mutex::new(None);

    fn invalidate_and_look_at_the_sink(timer: &CFRunLoopTimer) {
        let open = SINK_PROBE
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .is_some_and(|writer| {
                !matches!(
                    writer.try_recv(),
                    Err(std::sync::mpsc::TryRecvError::Disconnected)
                )
            });
        INVALIDATED_WITH_THE_SINK_OPEN
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(open);
        invalidate(timer);
    }

    #[test]
    fn the_context_invalidates_its_timer_before_its_fields_drop() {
        let held = held(1_000);
        let mut ctx = held.ctx;
        ctx.timers.invalidate = invalidate_and_look_at_the_sink;
        *SINK_PROBE.lock().unwrap_or_else(PoisonError::into_inner) = Some(held.writer);
        key_down(&ctx);
        assert!(lock(&ctx.hold).timer.is_some());
        drop(ctx);
        assert_eq!(
            *INVALIDATED_WITH_THE_SINK_OPEN
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
            [true],
            "the timer was invalidated once, while the context still held the sink"
        );
    }

    #[test]
    fn a_pointer_event_is_recorded_at_once_and_its_reading_drops_the_held_keys() {
        let held = held(1_000);
        key_down(&held.ctx);
        answer(Reading::On);
        advance(1);
        on_tap_event(&held.ctx, event(CGEventType::LeftMouseDown));
        assert_eq!(held_now(&held.ctx), 0);
        let kinds: Vec<Modality> = received(&held.writer)
            .iter()
            .map(|r| r.event().modality)
            .collect();
        assert_eq!(kinds, [Modality::Pointer]);
    }

    fn event_object(etype: CGEventType, flags: u64) -> CGEvent {
        let source =
            CGEventSource::new(CGEventSourceStateID::Private).expect("a private event source");
        let event = CGEvent::new(source).expect("an event object, never posted");
        event.set_type(etype);
        event.set_flags(CGEventFlags::from_bits_retain(flags));
        event
    }

    fn set_stamp(event: &CGEvent, stamp: u64) {
        let event: &CGEventRef = event;
        // SAFETY: the pointer of a live event object that this test made and
        // never posts; the call only writes its time stamp.
        unsafe { CGEventSetTimestamp(std::ptr::from_ref(event).cast(), stamp) };
    }

    struct EventOutcome {
        pointers: usize,
        suppressed: u64,
        asked: usize,
    }

    fn through_the_callback(events: &[(CGEventType, Reading)]) -> (Vec<EventOutcome>, usize) {
        let held = held(1_000);
        let counters = held.counters.clone();
        let writer = held.writer;
        let callback = tap_callback(*held.ctx);
        let event = event_object(CGEventType::Null, 0);
        let call = |etype: CGEventType| {
            event.set_type(etype);
            set_stamp(&event, UPTIME.with(Cell::get));
            let _ = callback(std::ptr::null(), etype, &event);
        };
        let mut keys = 0;
        let outcomes = events
            .iter()
            .map(|&(etype, reading)| {
                advance(1);
                answer(reading);
                let asked_before = asked();
                let suppressed_before = counters.suppressed_keys();
                call(etype);
                let mut pointers = 0;
                for received in received(&writer) {
                    match received.event().modality {
                        Modality::Pointer => pointers += 1,
                        _ => keys += 1,
                    }
                }
                EventOutcome {
                    pointers,
                    suppressed: counters.suppressed_keys() - suppressed_before,
                    asked: asked() - asked_before,
                }
            })
            .collect();
        assert_eq!(keys, 0, "a key was recorded before the bound");
        answer(Reading::Off);
        for _ in 0..45 {
            advance(25);
            call(CGEventType::MouseMoved);
            keys += keystrokes(&writer);
        }
        (outcomes, keys)
    }

    fn released_at_the_end(events: &[(CGEventType, Reading)]) -> usize {
        let mut held = 0;
        for &(etype, reading) in events {
            if reading != Reading::Off {
                held = 0;
            }
            if etype as u32 == CGEventType::KeyDown as u32 && reading == Reading::Off {
                held += 1;
            }
        }
        held
    }

    fn holds_for_its_own_reading(
        etype: CGEventType,
        reading: Reading,
        outcome: &EventOutcome,
    ) -> Result<(), String> {
        let EventOutcome {
            pointers,
            suppressed,
            asked,
        } = outcome;
        let is_pointer = matches!(
            tap_class(etype),
            Some(TapClass::DiscretePointer | TapClass::ContinuousPointer)
        );
        let key_down = etype as u32 == CGEventType::KeyDown as u32;
        let want = (
            usize::from(is_pointer),
            u64::from(key_down && reading == Reading::On),
        );
        if (*pointers, *suppressed) != want {
            return Err(format!(
                "{etype:?} read {reading:?}: pointers {pointers}, suppressed {suppressed}, \
                 not {want:?}"
            ));
        }
        if *asked != 1 {
            return Err(format!(
                "{etype:?} read {reading:?}: the reader was asked {asked} times, not once"
            ));
        }
        Ok(())
    }

    #[test]
    fn each_event_of_every_pair_through_the_callback_is_decided_by_its_own_reading_and_those_after_it(
    ) {
        let readings = [Reading::Off, Reading::On];
        for &(first, _) in &TAP_EVENTS {
            for &(second, _) in &TAP_EVENTS {
                for first_reading in readings {
                    for second_reading in readings {
                        let events = [(first, first_reading), (second, second_reading)];
                        let (outcomes, recorded) = through_the_callback(&events);
                        for (place, (&(etype, reading), outcome)) in
                            events.iter().zip(&outcomes).enumerate()
                        {
                            if let Err(failure) = holds_for_its_own_reading(etype, reading, outcome)
                            {
                                panic!(
                                    "the pair {first:?} read {first_reading:?}, then {second:?} \
                                     read {second_reading:?}: event {} of 2: {failure}",
                                    place + 1
                                );
                            }
                        }
                        assert_eq!(
                            recorded,
                            released_at_the_end(&events),
                            "the pair {first:?} read {first_reading:?}, then {second:?} read \
                             {second_reading:?}: keys recorded after the bound"
                        );
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
        let events: Vec<(CGEventType, Reading)> = (0..LENGTH)
            .map(|_| {
                let etype = TAP_EVENTS[(draw() % TAP_EVENTS.len() as u64) as usize].0;
                let reading = if draw() % 8 == 0 {
                    Reading::On
                } else {
                    Reading::Off
                };
                (etype, reading)
            })
            .collect();
        let is_key = |etype| {
            matches!(
                tap_class(etype),
                Some(TapClass::KeyPress | TapClass::OtherKey)
            )
        };
        let key_after_a_pointer_read_otherwise = events
            .windows(2)
            .filter(|two| !is_key(two[0].0) && is_key(two[1].0) && two[0].1 != two[1].1)
            .count();
        assert!(
            key_after_a_pointer_read_otherwise > 0,
            "the seed {SEED:#x} draws no key event right after a pointer event read otherwise"
        );
        let (outcomes, recorded) = through_the_callback(&events);
        for (place, (&(etype, reading), outcome)) in events.iter().zip(&outcomes).enumerate() {
            if let Err(failure) = holds_for_its_own_reading(etype, reading, outcome) {
                panic!(
                    "the sequence of {LENGTH} events from the seed {SEED:#x}: the first event \
                     that fails is at place {place} (from 0): {failure}"
                );
            }
        }
        let want = released_at_the_end(&events);
        assert!(want > 0, "the seed {SEED:#x} leaves no key held at the end");
        assert_eq!(recorded, want, "keys recorded after the bound");
    }

    #[test]
    fn the_reader_that_the_tap_gets_asks_the_system_once_for_each_call() {
        const CALLS: u64 = 1024;
        let reader = tap_settings().secure;
        let before = crate::secure_input_macos::system_reads_on_this_thread();
        for _ in 0..CALLS {
            let _ = reader(Session::AtConsole);
        }
        assert_eq!(
            crate::secure_input_macos::system_reads_on_this_thread() - before,
            CALLS,
            "calls that reached the system from this thread for {CALLS} reads"
        );
    }

    #[test]
    fn the_production_reader_reads_a_real_hold_as_on() {
        let one = one_hold_at_a_time();
        let Some(hold) = one.hold_or_skip("the_production_reader_reads_a_real_hold_as_on") else {
            return;
        };
        let session = read_session();
        assert!(
            reads_on_within(
                || secure_input_reading(session) == Reading::On,
                Duration::from_secs(2)
            ),
            "during an accepted hold the production reader does not read on"
        );
        drop(hold);
    }

    #[test]
    fn the_production_reader_reads_off_on_during_a_hold_and_off_again() {
        let one = one_hold_at_a_time();
        let limit = Duration::from_secs(2);
        let session = read_session();
        let reader_on = || secure_input_reading(session) == Reading::On;
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
            "during an accepted hold the production reader does not read on"
        );
        assert_eq!(after.0, after.1, "after the hold: (reader, session)");
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
                let _ = emit_metadata_only(
                    &blocked,
                    Modality::Keystroke,
                    None,
                    Unpersisted::EMPTY,
                    SystemTime::now(),
                );
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
    fn the_callback_that_the_tap_gets_counts_a_key_read_as_secure() {
        let (handle, writer) = sink::channel(8);
        let (_writer, reader) = foreground_state_for_test();
        let counters = InputCounters::new();
        let mut settings = tap_settings();
        settings.secure = |_| Reading::On;
        settings.session = || Session::AtConsole;
        let callback = tap_callback(tap_context(
            handle,
            reader,
            counters.clone(),
            &settings,
            Bounds::PRODUCT,
            Arc::new(AtomicBool::new(false)),
        ));
        let event = event_object(CGEventType::KeyDown, 0);
        set_stamp(&event, uptime_raw().0);
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
        let on = reads_on_within(
            crate::secure_input_macos::secure_input_active,
            Duration::from_secs(2),
        );
        let event = event_object(CGEventType::KeyDown, 0);
        set_stamp(&event, uptime_raw().0);
        let _ = callback(std::ptr::null(), CGEventType::KeyDown, &event);
        drop(hold);
        drop(callback);
        assert!(on, "the hold did not read on within 2 s");
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
        let mut ctx = tap_context(
            handle,
            reader,
            counters.clone(),
            &settings,
            Bounds::PRODUCT,
            Arc::new(AtomicBool::new(false)),
        );
        ctx.port = TapPort::with_enable(record_enable);
        let tap = std::ptr::NonNull::<c_void>::dangling().as_ptr();
        ctx.port.store(tap);
        let callback = tap_callback(ctx);
        let event = event_object(CGEventType::Null, 0);
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
    fn the_tap_reads_the_secure_state_the_session_and_the_clocks_of_the_os() {
        let settings = tap_settings();
        assert!(std::ptr::fn_addr_eq(
            settings.secure,
            secure_input_reading as fn(Session) -> Reading
        ));
        assert!(std::ptr::fn_addr_eq(
            settings.session,
            read_session as fn() -> Session
        ));
        assert!(std::ptr::fn_addr_eq(
            settings.clocks.monotonic,
            monotonic_raw as fn() -> Monotonic
        ));
        assert!(std::ptr::fn_addr_eq(
            settings.clocks.uptime,
            uptime_raw as fn() -> Uptime
        ));
    }

    #[test]
    fn the_clocks_are_the_raw_monotonic_and_uptime_clocks() {
        // SAFETY: as in `monotonic_raw`.
        let monotonic = unsafe { clock_gettime_nsec_np(libc::CLOCK_MONOTONIC_RAW) };
        let read = monotonic_raw().0;
        // SAFETY: as in `monotonic_raw`.
        let monotonic_after = unsafe { clock_gettime_nsec_np(libc::CLOCK_MONOTONIC_RAW) };
        assert!((monotonic..=monotonic_after).contains(&read));
        // SAFETY: as in `monotonic_raw`.
        let uptime = unsafe { clock_gettime_nsec_np(libc::CLOCK_UPTIME_RAW) };
        let read = uptime_raw().0;
        // SAFETY: as in `monotonic_raw`.
        let uptime_after = unsafe { clock_gettime_nsec_np(libc::CLOCK_UPTIME_RAW) };
        assert!((uptime..=uptime_after).contains(&read));
    }

    #[test]
    fn the_product_constructors_read_the_session_of_the_system() {
        let source = source_with(HoldPlan::Never);
        assert!(std::ptr::fn_addr_eq(
            source.session,
            read_session as fn() -> Session
        ));
        let (_writer, reader) = foreground_state_for_test();
        let short = InputSource::with_hold_bound(
            reader,
            InputCounters::new(),
            HoldPlan::Never,
            Duration::from_millis(5),
        );
        assert!(std::ptr::fn_addr_eq(
            short.session,
            read_session as fn() -> Session
        ));
    }

    #[test]
    fn a_started_source_reads_the_session_through_its_own_reader() {
        let kept = Arc::new(Mutex::new(None));
        let (handle, _writer) = sink::channel(8);
        let (_fg_writer, reader) = foreground_state_for_test();
        let counters = InputCounters::new();
        let mut source = InputSource::new(reader, counters.clone(), HoldPlan::Never);
        source.session = || Session::NoDictionary;
        let _ = source.begin_with(handle, KeepingMaker(kept.clone()), || {});
        let callback = kept
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
            .expect("the maker was given a callback");
        let event = event_object(CGEventType::KeyDown, 0);
        set_stamp(&event, uptime_raw().0);
        let _ = callback(std::ptr::null(), CGEventType::KeyDown, &event);
        assert!(
            counters.figures().keys_withheld,
            "a key under a session with no dictionary is withheld"
        );
    }

    #[test]
    fn the_product_constructor_holds_for_the_bound_of_the_product() {
        let source = source_with(HoldPlan::Never);
        assert_eq!(source.bounds, Bounds::PRODUCT);
        let (_writer, reader) = foreground_state_for_test();
        let short = InputSource::with_hold_bound(
            reader,
            InputCounters::new(),
            HoldPlan::Never,
            Duration::from_millis(5),
        );
        assert_eq!(
            short.bounds,
            Bounds {
                hold: Duration::from_millis(5),
                ..Bounds::PRODUCT
            }
        );
    }

    #[test]
    fn a_started_source_reads_impaired_while_it_is_stalled() {
        let (handle, _writer) = sink::channel(8);
        let mut source = source_with(HoldPlan::Never);
        let (ended_tx, _ended_rx) = channel();
        source
            .begin_with(handle, FakeMaker(ended_tx), || {})
            .expect("the source starts with a fake tap");
        assert_eq!(source.health().state, SourceState::Running);
        source.counters.set_stalled(true);
        assert_eq!(
            source.health().state,
            SourceState::Impaired {
                reason: HealthReason::RunLoopStalled
            }
        );
        source.counters.set_stalled(false);
        assert_eq!(source.health().state, SourceState::Running);
        source.shutdown().expect("the source stops");
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

    fn slot_of(hold: Hold) -> HoldSlot {
        HoldSlot {
            hold,
            held: None,
            refusal: Arc::new(Mutex::new(None)),
            own_take: None,
        }
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
        let mut hold = TimedHold::new(slot_of(shim_hold));
        hold.invalidate = invalidate_and_log;
        hold.take_now();
        let address = &*hold.slot as *const HoldSlot;
        hold.take_after(Duration::from_secs(3600));
        let moved = std::mem::replace(&mut hold, TimedHold::new(slot_of(shim_hold)));
        let same_address = std::ptr::eq(&*moved.slot, address);
        drop(moved);
        let log = std::mem::take(&mut *HOLD_LOG.lock().unwrap_or_else(PoisonError::into_inner));
        let steps: Vec<&str> = log.iter().map(|&(step, _)| step).collect();
        assert!(same_address, "the slot moved when its owner moved");
        assert_eq!(steps, ["enable", "invalidate", "disable"]);
    }

    extern "C" fn never_fires(_timer: CFRunLoopTimerRef, _info: *mut c_void) {}

    fn plan_of(plan: HoldPlan) -> TapPlan {
        TapPlan {
            plan,
            hold: hold_secure_input,
            refusal: Arc::new(Mutex::new(None)),
            own_take: None,
        }
    }

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
                plan_of(HoldPlan::Never),
                Arc::new(AtomicBool::new(false)),
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
            plan_of(HoldPlan::Never),
            Arc::new(AtomicBool::new(false)),
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
        let (_writer, reader) = foreground_state_for_test();
        let first = InputSource::new(reader.clone(), counters.clone(), HoldPlan::Never);
        let held = held(1_000);
        let (handle, _sink_writer) = sink::channel(8);
        let ctx = tap_context(
            handle,
            reader.clone(),
            first.counters.clone(),
            &stand_in_settings(),
            Bounds::PRODUCT,
            Arc::new(AtomicBool::new(false)),
        );
        ctx.read_session(&mut lock(&ctx.hold));
        answer(Reading::On);
        on_tap_event(&ctx, event(CGEventType::KeyDown));
        drop((ctx, first, held));
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
    fn a_late_key_at_the_bound_drops_the_key_its_own_reading_would_release() {
        let held = held(100);
        key_down(&held.ctx);
        ticks(&held.ctx, 90);
        assert_eq!(
            held_now(&held.ctx),
            1,
            "the key waits until it is 100 ms old"
        );
        advance(10);
        on_tap_event(
            &held.ctx,
            TapEvent {
                timing: Timing::Late {
                    lag: Duration::from_millis(40),
                },
                ..event(CGEventType::KeyDown)
            },
        );
        assert_eq!(held_now(&held.ctx), 0);
        assert!(held.counters.figures().keys_withheld);
        assert!(held.counters.stalled(), "a late key is a timing fault");
        ticks(&held.ctx, 200);
        assert_eq!(keystrokes(&held.writer), 0);
    }

    #[test]
    fn an_untimed_key_at_the_bound_drops_every_held_key_with_the_oldest() {
        let held = held(100);
        key_down(&held.ctx);
        key_down(&held.ctx);
        ticks(&held.ctx, 90);
        advance(10);
        on_tap_event(
            &held.ctx,
            TapEvent {
                timing: Timing::Untimed,
                ..event(CGEventType::KeyDown)
            },
        );
        assert_eq!(held_now(&held.ctx), 0);
        ticks(&held.ctx, 200);
        assert_eq!(keystrokes(&held.writer), 0);
    }

    #[test]
    fn a_late_key_up_drops_every_held_key() {
        let held = held(100);
        key_down(&held.ctx);
        ticks(&held.ctx, 50);
        advance(5);
        on_tap_event(
            &held.ctx,
            TapEvent {
                timing: Timing::Late {
                    lag: Duration::from_millis(40),
                },
                ..event(CGEventType::KeyUp)
            },
        );
        assert_eq!(held_now(&held.ctx), 0);
        assert!(held.counters.stalled());
        ticks(&held.ctx, 200);
        assert_eq!(keystrokes(&held.writer), 0);
    }

    #[test]
    fn a_reading_of_on_after_a_stop_is_asked_withholds_nothing() {
        let held = held(1_000);
        key_down(&held.ctx);
        held.ctx.stop.store(true, Ordering::SeqCst);
        answer(Reading::On);
        tick(&held.ctx);
        advance(1);
        on_tap_event(&held.ctx, event(CGEventType::KeyUp));
        assert_eq!(held_now(&held.ctx), 0);
        assert_eq!(keystrokes(&held.writer), 0);
        assert!(!held.counters.figures().keys_withheld);
    }

    #[test]
    fn a_key_event_after_a_stop_is_asked_moves_nothing() {
        let held = held(1_000);
        key_down(&held.ctx);
        held.ctx.stop.store(true, Ordering::SeqCst);
        answer(Reading::On);
        advance(1);
        on_tap_event(&held.ctx, event(CGEventType::KeyUp));
        assert_eq!(held_now(&held.ctx), 0);
        assert!(!held.counters.figures().keys_withheld);
        assert_eq!(held.counters.suppressed_keys(), 0);
    }

    #[test]
    fn a_gap_after_a_stop_is_asked_leaves_the_source_unstalled() {
        let held = held(1_000);
        key_down(&held.ctx);
        held.ctx.stop.store(true, Ordering::SeqCst);
        advance(40);
        on_reading_tick(&held.ctx);
        advance(40);
        on_tap_event(&held.ctx, event(CGEventType::KeyDown));
        assert_eq!(
            held_now(&held.ctx),
            0,
            "no key is held after the stop is asked"
        );
        assert!(!held.counters.stalled());
        assert!(!held.counters.figures().keys_withheld);
        assert!(lock(&held.ctx.hold).timer.is_none());
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
        let stop = Arc::new(AtomicBool::new(false));
        let ctx = tap_context(
            handle,
            reader,
            InputCounters::new(),
            &tap_settings(),
            Bounds::PRODUCT,
            stop.clone(),
        );
        let port = ctx.port.clone();
        let mut tap = TapThread::spawn(
            move || install_tap(ctx, tap_settings(), SystemTapMaker),
            TapPlan {
                plan: HoldPlan::Never,
                hold: hold_secure_input,
                refusal: Arc::new(Mutex::new(None)),
                own_take: None,
            },
            stop,
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
    use crate::secure_input_macos::hold::Timing;
    use crate::sink;

    fn context(secure: fn(Session) -> Reading) -> (TapContext, sink::SinkWriter) {
        let (handle, writer) = sink::channel(8);
        let (_writer, reader) = crate::macos::foreground::foreground_state_for_test();
        let settings = TapSettings {
            secure,
            session: || Session::AtConsole,
            ..tap_settings()
        };
        let ctx = tap_context(
            handle,
            reader,
            InputCounters::new(),
            &settings,
            Bounds {
                hold: Duration::ZERO,
                ..Bounds::PRODUCT
            },
            Arc::new(AtomicBool::new(false)),
        );
        ctx.read_session(&mut lock(&ctx.hold));
        (ctx, writer)
    }

    fn event(etype: CGEventType, flags: u64) -> TapEvent {
        TapEvent {
            etype,
            flags,
            timing: Timing::InTime {
                lag: Duration::ZERO,
            },
        }
    }

    #[test]
    fn a_key_press_under_secure_input_records_nothing_and_is_counted() {
        let (ctx, writer) = context(|_| Reading::On);
        let shift = core_graphics::event::CGEventFlags::CGEventFlagShift.bits();
        on_tap_event(&ctx, event(CGEventType::KeyDown, 0));
        on_tap_event(&ctx, event(CGEventType::KeyUp, 0));
        on_tap_event(&ctx, event(CGEventType::FlagsChanged, shift));
        on_tap_event(&ctx, event(CGEventType::FlagsChanged, 0));
        on_tap_event(&ctx, event(CGEventType::MouseMoved, 0));
        assert_eq!(
            ctx.counters.suppressed_keys(),
            2,
            "a key down and a shift press"
        );
        drop(ctx);
        assert_eq!(
            events_until_the_sink_closes(&writer),
            [Modality::Pointer],
            "a keystroke was recorded while secure input was active"
        );
    }

    #[test]
    fn a_suppressed_key_raises_the_count_from_zero_to_one() {
        let (ctx, _writer) = context(|_| Reading::On);
        assert_eq!(ctx.counters.suppressed_keys(), 0);
        on_tap_event(&ctx, event(CGEventType::KeyDown, 0));
        assert_eq!(ctx.counters.suppressed_keys(), 1);
        assert_eq!(ctx.counters.suppressed_keys(), 1);
    }

    #[test]
    fn pointer_events_record_under_secure_input_without_a_count() {
        let (ctx, writer) = context(|_| Reading::On);
        on_tap_event(&ctx, event(CGEventType::MouseMoved, 0));
        on_tap_event(&ctx, event(CGEventType::LeftMouseDown, 0));
        assert_eq!(ctx.counters.suppressed_keys(), 0);
        drop(ctx);
        let recorded = events_until_the_sink_closes(&writer);
        assert_eq!(recorded, [Modality::Pointer, Modality::Pointer]);
    }

    #[test]
    fn a_key_event_without_secure_input_is_recorded_at_its_release_and_not_at_its_arrival() {
        let (ctx, writer) = context(|_| Reading::Off);
        on_tap_event(&ctx, event(CGEventType::KeyDown, 0));
        assert_eq!(ctx.counters.suppressed_keys(), 0);
        assert!(
            writer.try_recv().is_err(),
            "a key was recorded at its arrival"
        );
        on_tap_event(&ctx, event(CGEventType::MouseMoved, 0));
        drop(ctx);
        assert_eq!(
            events_until_the_sink_closes(&writer),
            [Modality::Keystroke, Modality::Pointer]
        );
    }
}
