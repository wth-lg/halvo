#![deny(clippy::let_underscore_must_use)]

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use core_foundation::base::TCFType;
use core_foundation::date::CFDate;
use core_foundation::runloop::{
    kCFRunLoopCommonModes, kCFRunLoopDefaultMode, CFRunLoop, CFRunLoopSource, CFRunLoopTimer,
    CFRunLoopTimerContext, CFRunLoopTimerInvalidate, CFRunLoopTimerRef,
};
use core_graphics::event::{
    CGEvent, CGEventRef, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventTapProxy, CGEventType, CallbackResult, EventField,
};

use crate::event::Modality;
use crate::event::{EventClock, FloorEvent, FoldEvent};
use crate::floor::exclude::{
    DropReason, FloorHandle, Keep, PrivateWindow, ProcessId, Subject, Verdict,
};
use crate::floor::kind::{self as floor_kind, Recorded};
use crate::floor::payload::{Button, FoldClass, FoldCount, KeyClass, PointerFold};
use crate::floor::{Switch, Switches, POINTER_FOLD_INTERVAL};
use crate::macos::foreground::{Foreground, ForegroundReader};
use crate::macos::source_failure;
use crate::secure_input_macos::hold::{
    timing, timing_at_reading, Admission, Bounds, Cause, Entry, Marks, Monotonic, Stamps, Step,
    Timing, Uptime, PAIR_TRIES, PAIR_WIDTH, READING_STEP, SESSION_STEP, WAKE_MARGIN,
};
#[cfg(all(debug_assertions, feature = "stall-seam"))]
use crate::secure_input_macos::notice::MarkKind;
#[cfg(all(debug_assertions, feature = "stall-seam"))]
use crate::secure_input_macos::notice::{Focus, Seen};
use crate::secure_input_macos::notice::{NoticeMark, NoticeThread};
#[cfg(any(test, debug_assertions))]
use crate::secure_input_macos::{hold_secure_input, SecureInputHold};
use crate::secure_input_macos::{
    read_session, secure_input_reading, KeyQueue, Reading, Released, SecureInputRefused,
    SecureInputReleaseRefused, Session, SystemProbe,
};
use crate::sink::{Emitted, FoldEmitted, SinkHandle};
use crate::source::{
    CaptureError, CaptureSource, HealthReason, HeldSamples, SourceContext, SourceCounters,
    SourceHealth, SourceState, Started, StopCause, SuppressedKeys,
};

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventTapEnable(tap: *mut c_void, enable: bool);
    fn CGPreflightListenEventAccess() -> bool;
    fn CGEventGetTimestamp(event: *const c_void) -> u64;
    fn CGGetEventTapList(max: u32, list: *mut TapInformation, count: *mut u32) -> i32;
    #[cfg(any(test, debug_assertions))]
    fn CGEventSourceCounterForEventType(state: i32, event_type: u32) -> u32;
}

#[cfg(any(test, debug_assertions))]
const HID_SYSTEM_STATE: i32 = 1;

#[cfg(any(test, debug_assertions))]
fn hid_key_downs() -> u32 {
    // SAFETY: a counter query of the system that takes two values and touches no
    // memory of the caller.
    unsafe { CGEventSourceCounterForEventType(HID_SYSTEM_STATE, CGEventType::KeyDown as u32) }
}

/// The step of the samples of the secure-input state in the held phase.
pub const HELD_SAMPLE_STEP: Duration = Duration::from_millis(250);

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

const TAP_LIST_MAX: usize = 256;

fn own_taps() -> Option<Vec<TapInformation>> {
    let mut list = [NO_TAP; TAP_LIST_MAX];
    let mut count = 0u32;
    // SAFETY: `list` holds TAP_LIST_MAX entries, the maximum passed, and `count`
    // is a valid place for the number of entries written.
    let err = unsafe { CGGetEventTapList(TAP_LIST_MAX as u32, list.as_mut_ptr(), &mut count) };
    if err != 0 {
        return None;
    }
    let own = libc::pid_t::try_from(std::process::id()).ok()?;
    Some(
        list[..(count as usize).min(TAP_LIST_MAX)]
            .iter()
            .copied()
            .filter(|tap| tap.tapping_process == own)
            .collect(),
    )
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
    gate: Arc<Mutex<()>>,
    enable: TapEnable,
}

impl TapPort {
    fn unset() -> Self {
        Self::with_enable(CGEventTapEnable)
    }

    fn with_enable(enable: TapEnable) -> Self {
        Self {
            port: Arc::new(AtomicPtr::new(std::ptr::null_mut())),
            gate: Arc::new(Mutex::new(())),
            enable,
        }
    }

    fn store(&self, port: *mut c_void) {
        let _gate = self.gate.lock().unwrap_or_else(PoisonError::into_inner);
        self.port.store(port, Ordering::Relaxed);
    }

    fn disable(&self) {
        let _gate = self.gate.lock().unwrap_or_else(PoisonError::into_inner);
        let port = self.port.load(Ordering::Relaxed);
        if !port.is_null() {
            // SAFETY: outside tests `port` is set only by `SystemTapMaker::enable`,
            // to the mach port of the tap that it returns inside a
            // `PortClearedFirst`, whose Drop stores null through `store` before
            // that tap drops. `store` takes the gate that this call holds from
            // the load to the end of the call, so the tap of a port that is not
            // null stays alive until the call has returned, on whichever thread
            // it runs. `enable` is `CGEventTapEnable`, set by `unset`, or a
            // test's function that only reads its arguments.
            unsafe { (self.enable)(port, false) };
        }
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
    pointer_dropped_at_stop: AtomicU64,
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

    fn count_tap_disabled(&self, etype: CGEventType) {
        let counter = match etype {
            CGEventType::TapDisabledByUserInput => &self.0.tap_disabled_user_input,
            _ => &self.0.tap_disabled_timeout,
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    fn note_lag(&self, lag: Duration) {
        let ms = u64::try_from(lag.as_millis()).unwrap_or(u64::MAX);
        self.0.max_lag_ms.fetch_max(ms, Ordering::Relaxed);
    }

    fn count_recorded_key(&self, held_phase: bool) {
        let counter = if held_phase {
            &self.0.secure_held_keystrokes
        } else {
            &self.0.secure_released_keystrokes
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    fn count_held_suppressed(&self, etype: CGEventType) {
        let counter = match etype {
            CGEventType::KeyDown => &self.0.secure_held_suppressed_down,
            CGEventType::KeyUp => &self.0.secure_held_suppressed_up,
            CGEventType::FlagsChanged => &self.0.secure_held_suppressed_modifier,
            _ => return,
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    fn count_window_pointer(&self) {
        self.0.secure_window_pointer.fetch_add(1, Ordering::Relaxed);
    }

    #[cfg(any(test, debug_assertions))]
    fn sample(&self, held: bool) {
        self.0.secure_input_samples.fetch_add(1, Ordering::Relaxed);
        if held {
            self.0.secure_input_held.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[cfg(any(test, debug_assertions))]
    fn keys_typed(&self, typed: bool) {
        self.0
            .secure_window_keys_typed
            .store(typed, Ordering::Relaxed);
    }

    fn count_pointer_coalesced(&self) {
        self.0.pointer_coalesced.fetch_add(1, Ordering::Relaxed);
    }

    fn count_pointer_dropped_at_stop(&self) {
        self.0
            .pointer_dropped_at_stop
            .fetch_add(1, Ordering::Relaxed);
    }

    #[cfg(test)]
    fn pointer_dropped_at_stop(&self) -> u64 {
        self.0.pointer_dropped_at_stop.load(Ordering::Relaxed)
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

    /// Key downs that came in time and were read while secure input was on; each
    /// was suppressed and never emitted. A modifier press after a reading of on is
    /// a reading alone and is not counted, and a late or untimed press under
    /// secure input is withheld and counted as a drop of its cause.
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

/// The device-independent masks of the modifier keys in an event's flags: caps
/// lock, shift, control, option, command, help and fn; not the numeric pad.
const MODIFIER_KEY_MASKS: u64 = 0x00DF_0000;

/// The device-dependent bits of the left and right modifier keys in an event's
/// flags.
const MODIFIER_SIDE_BITS: u64 = 0x0000_207F;

/// The modifier keys by their virtual key code, each with the device-dependent
/// bit that an event's own flags hold while that key is down (0 where the key
/// has none) and the device-independent mask of its family: left and right
/// shift, left and right control, left and right option, left and right
/// command, caps lock and fn.
const MODIFIER_KEYS: [(i64, u64, u64); 10] = [
    (0x38, 0x0000_0002, 0x0002_0000),
    (0x3C, 0x0000_0004, 0x0002_0000),
    (0x3B, 0x0000_0001, 0x0004_0000),
    (0x3E, 0x0000_2000, 0x0004_0000),
    (0x3A, 0x0000_0020, 0x0008_0000),
    (0x3D, 0x0000_0040, 0x0008_0000),
    (0x37, 0x0000_0008, 0x0010_0000),
    (0x36, 0x0000_0010, 0x0010_0000),
    (0x39, 0, 0x0001_0000),
    (0x3F, 0, 0x0080_0000),
];

const _: () = {
    let mut at = 0;
    while at < MODIFIER_KEYS.len() {
        let (_, side, family) = MODIFIER_KEYS[at];
        assert!(side & !MODIFIER_SIDE_BITS == 0 && family & !MODIFIER_KEY_MASKS == 0);
        at += 1;
    }
};

const CAPS_LOCK: i64 = 0x39;

fn modifier_pressed(key_code: Option<i64>, flags: u64) -> bool {
    if key_code == Some(CAPS_LOCK) {
        return true;
    }
    let Some(&(_, side, family)) = MODIFIER_KEYS
        .iter()
        .find(|&&(code, _, _)| Some(code) == key_code)
    else {
        return false;
    };
    if side == 0 {
        flags & family != 0
    } else {
        flags & side != 0
    }
}

const CHORD_MASKS: u64 = 0x001C_0000;

fn key_class(key_code: Option<i64>, flags: u64) -> KeyClass {
    if flags & CHORD_MASKS != 0 {
        return KeyClass::Shortcut;
    }
    match key_code {
        Some(0x24 | 0x4C) => KeyClass::Enter,
        Some(0x30) => KeyClass::Tab,
        Some(0x33 | 0x72 | 0x75) => KeyClass::Editing,
        Some(0x73 | 0x74 | 0x77 | 0x79 | 0x7B..=0x7E) => KeyClass::Navigation,
        Some(0x00..=0x32 | 0x41 | 0x43 | 0x45 | 0x4B | 0x4E | 0x51..=0x59 | 0x5B..=0x5F) => {
            KeyClass::Printable
        }
        _ => KeyClass::Other,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Key(KeyClass),
    Reading,
    Click(Button),
    ButtonUp,
    Fold(FoldClass),
    Ignore,
}

impl Kind {
    fn is_key(self) -> bool {
        matches!(self, Kind::Key(_))
    }

    fn is_pointer(self) -> bool {
        matches!(self, Kind::Click(_) | Kind::ButtonUp | Kind::Fold(_))
    }
}

fn kind_of(etype: CGEventType, key_code: Option<i64>, flags: u64, autorepeat: bool) -> Kind {
    match etype {
        CGEventType::KeyDown if autorepeat => Kind::Reading,
        CGEventType::KeyDown => Kind::Key(key_class(key_code, flags)),
        CGEventType::FlagsChanged if modifier_pressed(key_code, flags) => {
            Kind::Key(KeyClass::Other)
        }
        CGEventType::KeyUp | CGEventType::FlagsChanged => Kind::Reading,
        CGEventType::LeftMouseDown => Kind::Click(Button::Primary),
        CGEventType::RightMouseDown => Kind::Click(Button::Secondary),
        CGEventType::OtherMouseDown => Kind::Click(Button::Other),
        CGEventType::LeftMouseUp | CGEventType::RightMouseUp | CGEventType::OtherMouseUp => {
            Kind::ButtonUp
        }
        CGEventType::MouseMoved => Kind::Fold(FoldClass::Move),
        CGEventType::LeftMouseDragged
        | CGEventType::RightMouseDragged
        | CGEventType::OtherMouseDragged => Kind::Fold(FoldClass::Drag),
        CGEventType::ScrollWheel => Kind::Fold(FoldClass::Scroll),
        _ => Kind::Ignore,
    }
}

#[derive(Default)]
struct Folds([Option<FoldCount>; 3]);

impl Folds {
    fn slot(&mut self, class: FoldClass) -> &mut Option<FoldCount> {
        let at = FoldClass::ALL
            .iter()
            .position(|&listed| listed == class)
            .unwrap_or(0);
        &mut self.0[at]
    }

    fn add(&mut self, class: FoldClass) {
        match self.slot(class) {
            Some(count) => count.add_one(),
            slot @ None => *slot = Some(FoldCount::one()),
        }
    }

    fn carry(&mut self, fold: PointerFold) {
        match self.slot(fold.class()) {
            Some(count) => count.add(fold.count()),
            slot @ None => *slot = Some(fold.count()),
        }
    }

    fn take(&mut self) -> Vec<PointerFold> {
        FoldClass::ALL
            .iter()
            .zip(std::mem::take(&mut self.0))
            .filter_map(|(&class, count)| count.map(|count| PointerFold::new(class, count)))
            .collect()
    }
}

fn events_of_interest(switches: Switches) -> Vec<CGEventType> {
    TAP_EVENTS
        .iter()
        .filter(|&&(etype, _)| match tap_class(etype) {
            Some(TapClass::KeyPress | TapClass::OtherKey) => switches.is_on(Switch::KeyClass),
            Some(TapClass::DiscretePointer | TapClass::ContinuousPointer) => {
                switches.is_on(Switch::PointerClass)
            }
            None => false,
        })
        .map(|&(etype, _)| etype)
        .collect()
}

fn mask_of(events: &[CGEventType]) -> u64 {
    events
        .iter()
        .fold(0u64, |mask, &etype| mask | 1 << (etype as u64))
}

const KEY_EVENTS: u64 = 1 << (CGEventType::KeyDown as u64) | 1 << (CGEventType::KeyUp as u64);

fn key_events_granted(requested: u64, granted: u64) -> bool {
    granted & requested & KEY_EVENTS == requested & KEY_EVENTS
}

const TAP_LOCATION: CGEventTapLocation = CGEventTapLocation::Session;
const TAP_PLACEMENT: CGEventTapPlacement = CGEventTapPlacement::HeadInsertEventTap;
const TAP_OPTIONS: CGEventTapOptions = CGEventTapOptions::ListenOnly;

#[derive(Clone, Copy)]
struct Clocks {
    /// The clock of every stamp of the hold (CLOCK_MONOTONIC_RAW, which goes
    /// on through sleep): readings, arrivals, releases and drops.
    stamps: fn() -> Monotonic,
    /// The clock of an event's OS time (CLOCK_UPTIME_RAW, which stops in
    /// sleep): an event's entry and the wake check.
    os_time: fn() -> Uptime,
}

fn monotonic_raw() -> Monotonic {
    // SAFETY: clock_gettime_nsec_np takes a clock id by value and returns a
    // number; it touches no memory of the caller.
    Monotonic::from_nanos(unsafe { clock_gettime_nsec_np(libc::CLOCK_MONOTONIC_RAW) })
}

fn uptime_raw() -> Uptime {
    // SAFETY: as in `monotonic_raw`.
    Uptime::from_nanos(unsafe { clock_gettime_nsec_np(libc::CLOCK_UPTIME_RAW) })
}

const CLOCKS: Clocks = Clocks {
    stamps: monotonic_raw,
    os_time: uptime_raw,
};

fn os_time(event: &CGEvent) -> Uptime {
    let event: &CGEventRef = event;
    // SAFETY: a reference to a foreign-types borrowed type is the pointer of the
    // C object, here the live event that the tap hands to its callback; the
    // call only reads it.
    Uptime::from_nanos(unsafe { CGEventGetTimestamp(std::ptr::from_ref(event).cast()) })
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
        events: events_of_interest(Switches::default()),
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
    const WATCHES_FOCUS: bool = false;

    fn make<F>(self, settings: TapSettings, callback: F) -> Result<Self::Tap, ()>
    where
        F: Fn(CGEventTapProxy, CGEventType, &CGEvent) -> CallbackResult + Send + 'static;

    fn listen_denied() -> bool;

    fn granted_events(_tap: &Self::Tap, requested: u64) -> u64 {
        requested
    }

    fn enable(tap: Self::Tap, port: TapPort) -> Result<Self::Enabled, CaptureError>;
}

struct SystemTapMaker;

impl TapMaker for SystemTapMaker {
    type Tap = CGEventTap<'static>;
    type Enabled = PortClearedFirst<(CFRunLoopSource, CGEventTap<'static>)>;
    const WATCHES_FOCUS: bool = true;

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

    fn granted_events(_tap: &Self::Tap, requested: u64) -> u64 {
        own_taps()
            .and_then(|taps| {
                taps.into_iter()
                    .filter(|tap| {
                        tap.tap_point == TAP_LOCATION as u32 && tap.options == TAP_OPTIONS as u32
                    })
                    .max_by_key(|tap| tap.event_tap_id)
            })
            .map_or(requested, |tap| tap.events_of_interest)
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

struct HeldKey {
    class: KeyClass,
    front: Option<Foreground>,
    held_phase: bool,
}

struct ReadingTimer(CFRunLoopTimer);

// SAFETY: a ReadingTimer is made, added to a run loop and invalidated on the
// thread whose run loop runs it, which is the thread that owns the hold state
// that holds it; that state is made empty on the thread that starts the source
// and holds no timer until it is on its own thread.
unsafe impl Send for ReadingTimer {}

struct HoldState {
    keys: KeyQueue<HeldKey>,
    session: Session,
    session_at: Option<Monotonic>,
    timer: Option<ReadingTimer>,
    offset: Option<i128>,
    wake_mark: Option<Uptime>,
    wake_unclocked: bool,
    notice_seen: Option<Uptime>,
    doubt_at_reading: bool,
}

type SharedHold = Arc<Mutex<HoldState>>;

fn lock(hold: &SharedHold) -> MutexGuard<'_, HoldState> {
    hold.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(any(test, debug_assertions))]
struct OwnTake {
    hold: SharedHold,
    counters: InputCounters,
    stop: Arc<AtomicBool>,
    #[cfg(all(debug_assertions, feature = "stall-seam"))]
    drops: Option<DropCounts>,
    #[cfg(all(debug_assertions, feature = "stall-seam"))]
    clock: fn() -> Monotonic,
}

#[cfg(any(test, debug_assertions))]
impl OwnTake {
    fn taken(&self) {
        let dropped = lock(&self.hold).keys.drop_all();
        let cause = if self.stop.load(Ordering::SeqCst) {
            Cause::Stop
        } else {
            Cause::OwnTake
        };
        #[cfg(all(debug_assertions, feature = "stall-seam"))]
        let own = Some((self.clock)().nanos());
        #[cfg(not(all(debug_assertions, feature = "stall-seam")))]
        let own = None;
        TapContext::count_drop(
            &self.counters,
            #[cfg(all(debug_assertions, feature = "stall-seam"))]
            self.drops.as_ref(),
            dropped,
            cause,
            own,
        );
    }
}

#[derive(Clone, Copy)]
struct TimerCalls {
    make: fn(*mut c_void, Duration) -> CFRunLoopTimer,
    fold: fn(*mut c_void, Duration) -> CFRunLoopTimer,
    invalidate: fn(&CFRunLoopTimer),
}

fn reading_timer(info: *mut c_void, step: Duration) -> CFRunLoopTimer {
    repeating_timer(info, step, reading_timer_fired)
}

fn fold_timer(info: *mut c_void, step: Duration) -> CFRunLoopTimer {
    repeating_timer(info, step, fold_timer_fired)
}

fn repeating_timer(
    info: *mut c_void,
    step: Duration,
    callout: extern "C" fn(CFRunLoopTimerRef, *mut c_void),
) -> CFRunLoopTimer {
    let mut context = CFRunLoopTimerContext {
        version: 0,
        info,
        retain: None,
        release: None,
        copyDescription: None,
    };
    let step = step.as_secs_f64();
    let timer = CFRunLoopTimer::new(
        CFDate::now().abs_time() + step,
        step,
        0,
        0,
        callout,
        &mut context,
    );
    add_to_this_run_loop(&timer);
    timer
}

const TIMER_CALLS: TimerCalls = TimerCalls {
    make: reading_timer,
    fold: fold_timer,
    invalidate,
};

#[derive(Clone, Copy)]
struct Keyed {
    event: TapEvent,
    key_code: Option<i64>,
    autorepeat: bool,
}

impl From<TapEvent> for Keyed {
    fn from(event: TapEvent) -> Self {
        Self {
            event,
            key_code: None,
            autorepeat: false,
        }
    }
}

#[derive(Clone, Copy)]
struct TapEvent {
    etype: CGEventType,
    flags: u64,
    os_time: Uptime,
    entry: Entry,
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
    step: Duration,
    hold: SharedHold,
    notices: NoticeMark,
    floor: FloorHandle,
    probe: SystemProbe,
    clock: &'static EventClock,
    folds: Mutex<Folds>,
    fold_timer: Mutex<Option<ReadingTimer>>,
    held_phase: Arc<AtomicBool>,
    #[cfg(all(debug_assertions, feature = "stall-seam"))]
    drops: Option<DropCounts>,
}

#[cfg(all(debug_assertions, feature = "stall-seam"))]
#[derive(Clone, Default)]
pub struct DropCounts(Arc<DropFigures>);

#[cfg(all(debug_assertions, feature = "stall-seam"))]
#[derive(Default)]
struct DropFigures {
    by_cause: [AtomicU64; 13],
    widest_gap_ns: AtomicU64,
    late_at_entry: AtomicU64,
    withheld_by_mark: [AtomicU64; 3],
    log: Mutex<RawLog>,
}

#[cfg(all(debug_assertions, feature = "stall-seam"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Caller {
    Event,
    Tick,
    ReleaseLoop,
}

#[cfg(all(debug_assertions, feature = "stall-seam"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrived {
    Held,
    Suppressed,
    Withheld { cause: Cause, dropped: usize },
}

/// One raw entry of the hold as a test build reads it, times on
/// CLOCK_MONOTONIC_RAW in nanoseconds. A release and a drop carry the before
/// stamp of the reading that made them; a drop that took no reading (a stop at
/// an event or a tick, the daemon's own take, the tap switched off) carries a
/// stamp of its own and is marked unread.
#[cfg(all(debug_assertions, feature = "stall-seam"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawEntry {
    Reading {
        before: u64,
        after: u64,
        value: Reading,
        caller: Caller,
        held: usize,
    },
    Arrival {
        at: u64,
        admission: Arrived,
    },
    Released {
        at: u64,
    },
    Dropped {
        at: u64,
        cause: Cause,
        keys: usize,
        unread: bool,
    },
    Notice {
        at: u64,
        seen: Seen,
        focus: Focus,
    },
}

#[cfg(all(debug_assertions, feature = "stall-seam"))]
struct RawLog {
    entries: std::collections::VecDeque<RawEntry>,
    cap: usize,
    lost: u64,
    caller: Caller,
    last_before: u64,
}

#[cfg(all(debug_assertions, feature = "stall-seam"))]
impl Default for RawLog {
    fn default() -> Self {
        Self {
            entries: std::collections::VecDeque::new(),
            cap: RAW_LOG_CAP,
            lost: 0,
            caller: Caller::Event,
            last_before: 0,
        }
    }
}

/// The count of raw entries that the log keeps before it drops its oldest.
#[cfg(all(debug_assertions, feature = "stall-seam"))]
pub const RAW_LOG_CAP: usize = 65_536;

#[cfg(all(debug_assertions, feature = "stall-seam"))]
const CAUSES: [(Cause, &str); 13] = [
    (Cause::On, "on"),
    (Cause::Unknown, "unknown"),
    (Cause::Gap, "gap"),
    (Cause::LateKey, "late"),
    (Cause::UntimedKey, "untimed"),
    (Cause::TapSwitchedOff, "tap_off"),
    (Cause::OwnTake, "own_take"),
    (Cause::Full, "full"),
    (Cause::Sleep, "sleep"),
    (Cause::Unclocked, "unclocked"),
    (Cause::Stop, "stop"),
    (Cause::Refused, "refused"),
    (Cause::Notice, "notice"),
];

#[cfg(all(debug_assertions, feature = "stall-seam"))]
impl DropCounts {
    pub fn new() -> Self {
        Self::default()
    }

    fn count(&self, cause: Cause) {
        if let Some(at) = CAUSES.iter().position(|&(listed, _)| listed == cause) {
            self.0.by_cause[at].fetch_add(1, Ordering::Relaxed);
        }
    }

    fn withheld_by(&self, kind: MarkKind, keys: usize) {
        let at = match kind {
            MarkKind::Notice => 0,
            MarkKind::Doubt => 1,
            MarkKind::Start => 2,
        };
        self.0.withheld_by_mark[at].fetch_add(keys as u64, Ordering::Relaxed);
    }

    /// The keys withheld because the notice mark moved, by what moved it last:
    /// a notice of a focus change into a secure field, a doubt, and the read
    /// at the start of a watch.
    pub fn withheld_by_mark(&self) -> (u64, u64, u64) {
        let of = |at: usize| self.0.withheld_by_mark[at].load(Ordering::Relaxed);
        (of(0), of(1), of(2))
    }

    fn late_at_entry(&self) {
        self.0.late_at_entry.fetch_add(1, Ordering::Relaxed);
    }

    /// The count of key events that were late by their lag at the callback's
    /// entry alone.
    pub fn late_by_the_entry(&self) -> u64 {
        self.0.late_at_entry.load(Ordering::Relaxed)
    }

    fn gap(&self, gap: Duration) {
        let ns = u64::try_from(gap.as_nanos()).unwrap_or(u64::MAX);
        self.0.widest_gap_ns.fetch_max(ns, Ordering::Relaxed);
    }

    pub fn by_cause(&self) -> Vec<(&'static str, u64)> {
        CAUSES
            .iter()
            .zip(&self.0.by_cause)
            .map(|(&(_, name), count)| (name, count.load(Ordering::Relaxed)))
            .collect()
    }

    pub fn of(&self, name: &str) -> u64 {
        self.by_cause()
            .into_iter()
            .find(|&(listed, _)| listed == name)
            .map_or(0, |(_, count)| count)
    }

    /// The widest gap between two readings, from before the earlier read to
    /// after the later one, while keys were held.
    pub fn widest_gap(&self) -> Duration {
        Duration::from_nanos(self.0.widest_gap_ns.load(Ordering::Relaxed))
    }

    /// Counts that keep at most `cap` raw entries, dropping the oldest.
    pub fn with_capacity(cap: usize) -> Self {
        let counts = Self::default();
        counts.log().cap = cap;
        counts
    }

    fn log(&self) -> MutexGuard<'_, RawLog> {
        self.0.log.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The raw entries since the last take, oldest first; the log is empty
    /// after it.
    pub fn take_entries(&self) -> Vec<RawEntry> {
        self.log().entries.drain(..).collect()
    }

    fn push(&self, entry: RawEntry) {
        let mut log = self.log();
        if log.cap == 0 {
            log.lost += 1;
            return;
        }
        if log.entries.len() >= log.cap {
            log.entries.pop_front();
            log.lost += 1;
        }
        log.entries.push_back(entry);
    }

    fn mark(&self, caller: Caller) {
        self.log().caller = caller;
    }

    fn reading(&self, value: Reading, at: Stamps, held: usize) {
        let caller = {
            let mut log = self.log();
            log.last_before = at.before.nanos();
            log.caller
        };
        self.push(RawEntry::Reading {
            before: at.before.nanos(),
            after: at.after.nanos(),
            value,
            caller,
            held,
        });
    }

    fn last_before(&self) -> u64 {
        self.log().last_before
    }

    fn arrival(&self, admission: Admission) {
        let admission = match admission {
            Admission::Held => Arrived::Held,
            Admission::Suppressed { .. } => Arrived::Suppressed,
            Admission::Withheld { cause, dropped } => Arrived::Withheld { cause, dropped },
        };
        let at = self.last_before();
        self.push(RawEntry::Arrival { at, admission });
    }

    fn released(&self) {
        let at = self.last_before();
        self.push(RawEntry::Released { at });
    }

    fn dropped(&self, cause: Cause, keys: usize, own: Option<u64>) {
        let at = own.unwrap_or_else(|| self.last_before());
        self.push(RawEntry::Dropped {
            at,
            cause,
            keys,
            unread: own.is_some(),
        });
    }

    fn notice(&self, at: u64, seen: Seen, focus: Focus) {
        self.push(RawEntry::Notice { at, seen, focus });
    }

    /// The count of raw entries dropped because the log was full.
    pub fn lost(&self) -> u64 {
        self.log().lost
    }
}

#[cfg(all(debug_assertions, feature = "stall-seam"))]
pub struct StallSeam {
    pub bound: Duration,
    pub gap: Duration,
    pub step: Duration,
    pub session: fn() -> Session,
    pub front: fn() -> Option<libc::pid_t>,
    pub drops: DropCounts,
}

impl TapContext {
    fn read_session(&self, state: &mut HoldState) {
        state.session = (self.session)();
        state.session_at = Some((self.clocks.stamps)());
    }

    /// The source's start: its first session read and its one pair of clocks,
    /// whose difference is kept only when the pair came close.
    fn start(&self, state: &mut HoldState) {
        self.read_session(state);
        state.offset = self.clock_pair().map(|(difference, _)| difference);
    }

    fn clock_pair(&self) -> Option<(i128, Uptime)> {
        let width = u64::try_from(PAIR_WIDTH.as_nanos()).unwrap_or(u64::MAX);
        for _ in 0..PAIR_TRIES {
            let first = (self.clocks.os_time)();
            let monotonic = (self.clocks.stamps)();
            let second = (self.clocks.os_time)();
            if second.nanos().saturating_sub(first.nanos()) <= width {
                return Some((
                    i128::from(monotonic.nanos()) - i128::from(first.nanos()),
                    second,
                ));
            }
        }
        None
    }

    /// Whether this reading reads a wake of the Mac: its three tries at the pair
    /// all failed (`wake_unclocked`), its difference rose above the kept one by
    /// more than the margin, or no difference was kept and its pair came close.
    /// A wake sets the mark that withholds every key whose own time is not
    /// after it.
    fn woke(&self, state: &mut HoldState) -> bool {
        let Some((difference, at)) = self.clock_pair() else {
            state.wake_mark = Some((self.clocks.os_time)());
            state.wake_unclocked = true;
            return true;
        };
        let margin = i128::try_from(WAKE_MARGIN.as_nanos()).unwrap_or(i128::MAX);
        match state.offset {
            Some(kept) if difference > kept.saturating_add(margin) => {
                state.wake_unclocked = false;
                state.offset = Some(difference);
                state.wake_mark = Some(at);
                true
            }
            Some(kept) => {
                state.offset = Some(kept.max(difference));
                false
            }
            None => {
                state.wake_mark = Some(at);
                state.offset = Some(difference);
                state.wake_unclocked = false;
                true
            }
        }
    }

    fn reading(&self, state: &mut HoldState, key: bool) -> (Reading, Stamps) {
        let now = (self.clocks.stamps)();
        let stale = state.session_at.is_none_or(|at| {
            now < at || Duration::from_nanos(now.nanos() - at.nanos()) >= SESSION_STEP
        });
        if stale && (key || !state.keys.is_empty()) {
            self.read_session(state);
        }
        let before = (self.clocks.stamps)();
        let reading = (self.secure)(state.session);
        let after = (self.clocks.stamps)();
        let woke = self.woke(state);
        #[cfg(all(debug_assertions, feature = "stall-seam"))]
        if let Some(drops) = &self.drops {
            if let (Some(previous), false) = (state.keys.previous(), state.keys.is_empty()) {
                drops.gap(Duration::from_nanos(
                    after.nanos().saturating_sub(previous.nanos()),
                ));
            }
            drops.reading(reading, Stamps { before, after }, state.keys.len());
        }
        if woke {
            let keys = state.keys.drop_all();
            let cause = if state.wake_unclocked {
                Cause::Unclocked
            } else {
                Cause::Sleep
            };
            self.dropped(keys, cause);
            if keys == 0 && state.wake_unclocked && !self.stop.load(Ordering::SeqCst) {
                self.counters.set_stalled(true);
            }
        }
        let doubt = self.notices.doubt();
        let mark = self.notices.get();
        state.doubt_at_reading = doubt;
        if mark > state.notice_seen || doubt {
            state.notice_seen = mark.max(state.notice_seen);
            if let Step::Dropped { keys, cause } = state.keys.noticed(Stamps { before, after }) {
                #[cfg(all(debug_assertions, feature = "stall-seam"))]
                if let (Some(drops), Cause::Notice) = (&self.drops, cause) {
                    drops.withheld_by(self.mark_kind(), keys);
                }
                self.dropped(keys, cause);
            }
        }
        if !state.keys.is_empty() && self.front_excluded() {
            state.keys.drop_all();
        }
        (reading, Stamps { before, after })
    }

    fn front_excluded(&self) -> bool {
        let Some(front) = self.foreground.get() else {
            return false;
        };
        let Ok(pid) = u32::try_from(front.pid) else {
            return true;
        };
        let subject = Subject::new(front.identity.to_event(), ProcessId::new(pid));
        matches!(
            self.floor.decide::<floor_kind::Keystroke>(
                subject,
                PrivateWindow::Unknown,
                &self.probe
            ),
            Verdict::Drop(
                DropReason::ListedApp | DropReason::ExclusionTree | DropReason::UnknownLineage
            )
        )
    }

    #[cfg(all(debug_assertions, feature = "stall-seam"))]
    fn mark_kind(&self) -> MarkKind {
        if self.notices.doubt() {
            MarkKind::Doubt
        } else {
            self.notices.kind()
        }
    }

    #[cfg(all(debug_assertions, feature = "stall-seam"))]
    fn mark(&self, caller: Caller) {
        if let Some(drops) = &self.drops {
            drops.mark(caller);
        }
    }

    fn dropped(&self, keys: usize, cause: Cause) {
        self.drop_with(keys, cause, false);
    }

    fn dropped_unread(&self, keys: usize, cause: Cause) {
        self.drop_with(keys, cause, true);
    }

    fn drop_with(&self, keys: usize, cause: Cause, unread: bool) {
        let cause = if self.stop.load(Ordering::SeqCst) {
            Cause::Stop
        } else {
            cause
        };
        #[cfg(all(debug_assertions, feature = "stall-seam"))]
        let own = unread.then(|| (self.clocks.stamps)().nanos());
        #[cfg(not(all(debug_assertions, feature = "stall-seam")))]
        let own = unread.then_some(0);
        Self::count_drop(
            &self.counters,
            #[cfg(all(debug_assertions, feature = "stall-seam"))]
            self.drops.as_ref(),
            keys,
            cause,
            own,
        );
    }

    /// Counts a drop of `keys` held keys for `cause`, whose stop the caller has
    /// read: the seam's count and entry, `keys_withheld` and the stalled flag.
    /// The tap thread and the daemon's own take count every drop here.
    fn count_drop(
        counters: &InputCounters,
        #[cfg(all(debug_assertions, feature = "stall-seam"))] drops: Option<&DropCounts>,
        keys: usize,
        cause: Cause,
        own: Option<u64>,
    ) {
        #[cfg(all(debug_assertions, feature = "stall-seam"))]
        if keys > 0 {
            if let Some(drops) = drops {
                drops.count(cause);
                drops.dropped(cause, keys, own);
            }
        }
        #[cfg(not(all(debug_assertions, feature = "stall-seam")))]
        let _ = own;
        if keys > 0 && cause.withholds() {
            counters.withhold();
        }
        if keys > 0 && cause.timing_fault() {
            counters.set_stalled(true);
        }
    }

    /// An arriving key whose event read the stop as set after its own reading:
    /// it is not admitted, the seam enters it as withheld with the cause stop,
    /// and it moves no flag and no count but late_events, which reads the lag
    /// at the entry.
    fn withheld_at_a_stop(&self, at_entry: Timing) {
        #[cfg(all(debug_assertions, feature = "stall-seam"))]
        if let Some(drops) = &self.drops {
            drops.arrival(Admission::Withheld {
                cause: Cause::Stop,
                dropped: 0,
            });
            drops.count(Cause::Stop);
        }
        if let Timing::Late { lag } = at_entry {
            if lag > crate::secure_input_macos::hold::LATE_BOUND {
                self.counters.count_late_key();
            }
        }
    }

    fn admitted(&self, admission: Admission, late: bool) {
        #[cfg(all(debug_assertions, feature = "stall-seam"))]
        if let Some(drops) = &self.drops {
            drops.arrival(admission);
        }
        match admission {
            Admission::Held => {}
            Admission::Suppressed { dropped } => {
                self.counters.count_suppressed_key();
                self.dropped(dropped, Cause::On);
            }
            Admission::Withheld { cause, .. } => {
                #[cfg(all(debug_assertions, feature = "stall-seam"))]
                if let Some(drops) = &self.drops {
                    drops.count(cause);
                    if cause == Cause::Notice {
                        drops.withheld_by(self.mark_kind(), 1);
                    }
                }
                self.counters.withhold();
                if cause.timing_fault() {
                    self.counters.set_stalled(true);
                }
            }
        }
        if late {
            self.counters.count_late_key();
        }
    }

    fn release(&self, state: &mut HoldState, mut step: Step<Released<HeldKey>>) {
        loop {
            let key = match step {
                Step::Waiting => return,
                Step::Dropped { keys, cause } => return self.dropped(keys, cause),
                Step::Released(key) => key,
            };
            if self.stop.load(Ordering::SeqCst) {
                let keys = state.keys.drop_all() + 1;
                self.dropped(keys, Cause::Stop);
                return;
            }
            let (clearance, key) = key.into_parts();
            if let Some(keep) = self.keep::<floor_kind::Keystroke>(key.front.as_ref()) {
                let event = FloorEvent::keystroke(keep, clearance, key.class, self.clock);
                if self.sink.emit_event(event) == Emitted::Refused {
                    let keys = state.keys.drop_all() + 1;
                    self.dropped(keys, Cause::Refused);
                    return;
                }
                self.counters.count_recorded_key(key.held_phase);
            }
            #[cfg(all(debug_assertions, feature = "stall-seam"))]
            if let Some(drops) = &self.drops {
                drops.released();
            }
            self.counters.set_stalled(false);
            if state.keys.is_empty() {
                return;
            }
            if self.stop.load(Ordering::SeqCst) {
                let keys = state.keys.drop_all();
                self.dropped(keys, Cause::Stop);
                return;
            }
            #[cfg(all(debug_assertions, feature = "stall-seam"))]
            self.mark(Caller::ReleaseLoop);
            let (reading, at) = self.reading(state, false);
            step = state.keys.reading(reading, at);
        }
    }

    fn settle_timer(&self, state: &mut HoldState) {
        match (state.keys.is_empty(), &state.timer) {
            (false, None) => {
                let info = std::ptr::from_ref(self).cast_mut().cast::<c_void>();
                state.timer = Some(ReadingTimer((self.timers.make)(info, self.step)));
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
        let fold = self
            .fold_timer
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(ReadingTimer(timer)) = fold {
            (self.timers.invalidate)(&timer);
        }
    }
}

impl TapContext {
    fn keep<K: Recorded>(&self, front: Option<&Foreground>) -> Option<Keep<K>> {
        let front = front?;
        let pid = ProcessId::new(u32::try_from(front.pid).ok()?);
        let subject = Subject::new(front.identity.to_event(), pid);
        match self
            .floor
            .decide::<K>(subject, PrivateWindow::Unknown, &self.probe)
        {
            Verdict::Keep(keep) => Some(keep),
            Verdict::Drop(_) => None,
        }
    }

    fn in_held_phase(&self) -> bool {
        self.held_phase.load(Ordering::SeqCst)
    }

    fn folds(&self) -> MutexGuard<'_, Folds> {
        self.folds.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn start_folds(&self) {
        let info = std::ptr::from_ref(self).cast_mut().cast::<c_void>();
        let timer = (self.timers.fold)(info, POINTER_FOLD_INTERVAL);
        *self
            .fold_timer
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(ReadingTimer(timer));
    }
}

fn event_age(event: &TapEvent) -> Option<Duration> {
    match timing(event.entry.uptime, event.os_time, Duration::MAX) {
        Timing::InTime { lag } | Timing::Late { lag } => Some(lag),
        Timing::Untimed | Timing::Asleep | Timing::Unclocked | Timing::Noticed => None,
    }
}

fn on_pointer(ctx: &TapContext, kind: Kind, event: &TapEvent) {
    if ctx.stop.load(Ordering::SeqCst) {
        ctx.counters.count_pointer_dropped_at_stop();
        return;
    }
    if ctx.in_held_phase() && matches!(kind, Kind::Click(_) | Kind::Fold(_)) {
        ctx.counters.count_window_pointer();
    }
    match kind {
        Kind::Click(button) => {
            let front = ctx.foreground.get();
            if let Some(keep) = ctx.keep::<floor_kind::Pointer>(front.as_ref()) {
                ctx.sink
                    .emit_event(FloorEvent::click(keep, button, event_age(event), ctx.clock));
            }
        }
        Kind::Fold(class) => {
            let front = ctx.foreground.get();
            if ctx.keep::<floor_kind::Pointer>(front.as_ref()).is_some() {
                ctx.folds().add(class);
            }
        }
        Kind::Key(_) | Kind::Reading | Kind::ButtonUp | Kind::Ignore => {}
    }
}

fn close_interval(ctx: &TapContext) {
    let folds = ctx.folds().take();
    if folds.is_empty() || ctx.stop.load(Ordering::SeqCst) {
        return;
    }
    let front = ctx.foreground.get();
    for fold in folds {
        let Some(keep) = ctx.keep::<floor_kind::Pointer>(front.as_ref()) else {
            continue;
        };
        match ctx
            .sink
            .emit_continuous(FoldEvent::new(keep, fold, None, ctx.clock))
        {
            FoldEmitted::Queued | FoldEmitted::Refused => {}
            FoldEmitted::Coalesced(back) => {
                ctx.counters.count_pointer_coalesced();
                ctx.folds().carry(back.fold());
            }
        }
    }
}

extern "C" fn fold_timer_fired(_timer: CFRunLoopTimerRef, info: *mut c_void) {
    // SAFETY: as in `reading_timer_fired`: `info` is the address of the boxed
    // TapContext that armed this timer and invalidates it in its Drop.
    let ctx = unsafe { &*info.cast::<TapContext>() };
    close_interval(ctx);
}

fn on_tap_event(ctx: &TapContext, keyed: impl Into<Keyed>) {
    let Keyed {
        event,
        key_code,
        autorepeat,
    } = keyed.into();
    let hold = ctx.hold.clone();
    let mut state = lock(&hold);
    if is_tap_disabled(event.etype) {
        ctx.counters.count_tap_disabled(event.etype);
        let stopped = ctx.stop.load(Ordering::SeqCst);
        let keys = state.keys.drop_all();
        ctx.dropped_unread(keys, Cause::TapSwitchedOff);
        ctx.settle_timer(&mut state);
        drop(state);
        if !stopped {
            ctx.port.reenable();
        }
        return;
    }
    let kind = match kind_of(event.etype, key_code, event.flags, autorepeat) {
        Kind::Key(_) if !ctx.floor.switches().is_on(Switch::KeyClass) => Kind::Reading,
        kind => kind,
    };
    if kind == Kind::Ignore {
        return;
    }
    if ctx.stop.load(Ordering::SeqCst) {
        let keys = state.keys.drop_all();
        ctx.dropped_unread(keys, Cause::Stop);
    } else {
        #[cfg(all(debug_assertions, feature = "stall-seam"))]
        ctx.mark(Caller::Event);
        let (reading, at) = ctx.reading(&mut state, kind.is_key());
        if ctx.stop.load(Ordering::SeqCst) {
            let keys = state.keys.drop_all();
            ctx.dropped(keys, Cause::Stop);
            if kind.is_key() {
                let lag_bound = state.keys.bounds().lag;
                ctx.withheld_at_a_stop(timing(event.entry.uptime, event.os_time, lag_bound));
            }
            ctx.settle_timer(&mut state);
            drop(state);
            if kind.is_pointer() {
                on_pointer(ctx, kind, &event);
            }
            return;
        }
        if reading == Reading::On && ctx.in_held_phase() {
            ctx.counters.count_held_suppressed(event.etype);
        }
        let arrival = at.before;
        let lag_bound = state.keys.bounds().lag;
        #[cfg(all(debug_assertions, feature = "stall-seam"))]
        let at_entry = timing(event.entry.uptime, event.os_time, lag_bound);
        let at_reading = timing_at_reading(
            event.entry,
            event.os_time,
            at.after,
            Marks {
                wake: state.wake_mark,
                notice: if state.doubt_at_reading {
                    state.notice_seen.max(Some(event.entry.uptime))
                } else {
                    state.notice_seen
                },
            },
            lag_bound,
        );
        let at_reading = if at_reading == Timing::Asleep && state.wake_unclocked {
            Timing::Unclocked
        } else {
            at_reading
        };
        #[cfg(all(debug_assertions, feature = "stall-seam"))]
        if let (Some(drops), Kind::Key(_) | Kind::Reading, Timing::Late { .. }) =
            (&ctx.drops, kind, at_entry)
        {
            drops.late_at_entry();
        }
        let step = match (kind, at_reading.refusal()) {
            (Kind::Key(_) | Kind::Reading, Some(_)) => state.keys.late_reading(at, at_reading),
            _ => state.keys.reading(reading, at),
        };
        let step = match step {
            Step::Dropped { keys, cause } => {
                ctx.dropped(keys, cause);
                Step::Waiting
            }
            other => other,
        };
        if let Kind::Key(class) = kind {
            let age = event_age(&event);
            if let Some(lag) = age {
                ctx.counters.note_lag(lag);
            }
            let key = HeldKey {
                class,
                front: ctx.foreground.get(),
                held_phase: ctx.in_held_phase(),
            };
            let admitted = state.keys.admit(reading, at_reading, arrival, age, key);
            ctx.admitted(admitted.admission, admitted.late);
        }
        ctx.release(&mut state, step);
    }
    ctx.settle_timer(&mut state);
    drop(state);
    if kind.is_pointer() {
        on_pointer(ctx, kind, &event);
    }
}

fn on_reading_tick(ctx: &TapContext) {
    let hold = ctx.hold.clone();
    let mut state = lock(&hold);
    if ctx.stop.load(Ordering::SeqCst) {
        let keys = state.keys.drop_all();
        ctx.dropped_unread(keys, Cause::Stop);
    } else {
        #[cfg(all(debug_assertions, feature = "stall-seam"))]
        ctx.mark(Caller::Tick);
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
        step: READING_STEP,
        hold: Arc::new(Mutex::new(HoldState {
            keys: KeyQueue::new(bounds),
            session: Session::NoDictionary,
            session_at: None,
            timer: None,
            offset: None,
            wake_mark: None,
            wake_unclocked: false,
            notice_seen: None,
            doubt_at_reading: false,
        })),
        notices: NoticeMark::new(),
        floor: FloorHandle::default(),
        probe: SystemProbe::new(),
        clock: EventClock::system(),
        folds: Mutex::new(Folds::default()),
        fold_timer: Mutex::new(None),
        held_phase: Arc::new(AtomicBool::new(false)),
        #[cfg(all(debug_assertions, feature = "stall-seam"))]
        drops: None,
    }
}

/// An event of the tests stamped by the real clocks, in the order of tap_event.
#[cfg(test)]
fn event_now(etype: CGEventType, flags: u64) -> TapEvent {
    let monotonic = monotonic_raw();
    let uptime = uptime_raw();
    TapEvent {
        etype,
        flags,
        os_time: uptime,
        entry: Entry { monotonic, uptime },
    }
}

fn tap_event(ctx: &TapContext, etype: CGEventType, event: &CGEvent) -> Keyed {
    let monotonic = (ctx.clocks.stamps)();
    let uptime = (ctx.clocks.os_time)();
    Keyed {
        event: TapEvent {
            etype,
            flags: event.get_flags().bits(),
            os_time: os_time(event),
            entry: Entry { monotonic, uptime },
        },
        key_code: Some(event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE)),
        autorepeat: event.get_integer_value_field(EventField::KEYBOARD_EVENT_AUTOREPEAT) != 0,
    }
}

fn tap_callback(
    ctx: TapContext,
) -> impl Fn(CGEventTapProxy, CGEventType, &CGEvent) -> CallbackResult + Send + 'static {
    let ctx = Box::new(ctx);
    ctx.start(&mut lock(&ctx.hold));
    ctx.start_folds();
    move |_proxy, etype, event: &CGEvent| {
        on_tap_event(&ctx, tap_event(&ctx, etype, event));
        CallbackResult::Keep
    }
}

fn install_tap<M: TapMaker>(
    ctx: TapContext,
    settings: TapSettings,
    maker: M,
    keys_missing: Arc<AtomicBool>,
) -> Result<M::Enabled, CaptureError> {
    let port = ctx.port.clone();
    let requested = mask_of(&settings.events);
    let tap = maker
        .make(settings, tap_callback(ctx))
        .map_err(|()| tap_refusal(M::listen_denied()))?;
    if !key_events_granted(requested, M::granted_events(&tap, requested)) {
        keys_missing.store(true, Ordering::SeqCst);
        return Err(CaptureError::PermissionDenied(Modality::Keystroke));
    }
    M::enable(tap, port)
}

/// When the tap thread holds secure input: the same state that a focused
/// password field asserts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoldPlan {
    /// The tap thread never holds secure input.
    Never,
    /// The tap thread holds secure input from this long after its tap is made
    /// until its tap is gone and the thread ends, or the process ends. Zero
    /// holds it before the tap is enabled. A build without debug assertions and
    /// without tests has no such plan (ruling MA-9 part (3)).
    #[cfg(any(test, debug_assertions))]
    After(Duration),
}

#[cfg(any(test, debug_assertions))]
type Hold = fn() -> Result<SecureInputHold, SecureInputRefused>;

#[cfg(any(test, debug_assertions))]
struct HoldSlot {
    hold: Hold,
    held: Option<SecureInputHold>,
    refusal: Arc<Mutex<Option<SecureInputRefused>>>,
    own_take: Option<OwnTake>,
    phase: Option<HeldPhase>,
}

#[cfg(any(test, debug_assertions))]
struct HeldPhase {
    held: Arc<AtomicBool>,
    counters: InputCounters,
    secure: fn(Session) -> Reading,
    session: fn() -> Session,
    typed: fn() -> u32,
    typed_at_take: Option<u32>,
    sampler: Option<ReadingTimer>,
}

#[cfg(any(test, debug_assertions))]
impl HeldPhase {
    fn enter(&mut self) {
        self.held.store(true, Ordering::SeqCst);
        self.typed_at_take = Some((self.typed)());
        let info = std::ptr::from_mut(self).cast::<c_void>();
        self.sampler = Some(ReadingTimer(repeating_timer(
            info,
            HELD_SAMPLE_STEP,
            held_sample_fired,
        )));
    }

    fn sample(&self) {
        let held = (self.secure)((self.session)()) == Reading::On;
        self.counters.sample(held);
    }

    fn end(&mut self) {
        if let Some(ReadingTimer(timer)) = self.sampler.take() {
            invalidate(&timer);
        }
        if let Some(start) = self.typed_at_take.take() {
            self.held.store(false, Ordering::SeqCst);
            self.counters.keys_typed((self.typed)() > start);
        }
    }
}

#[cfg(any(test, debug_assertions))]
impl Drop for HeldPhase {
    fn drop(&mut self) {
        self.end();
    }
}

#[cfg(any(test, debug_assertions))]
extern "C" fn held_sample_fired(_timer: CFRunLoopTimerRef, info: *mut c_void) {
    // SAFETY: `info` is the HeldPhase that armed this timer, in the boxed slot of
    // a TimedHold; the phase invalidates the timer when it ends and in its Drop,
    // and the timer fires on the tap thread alone, which owns the slot.
    let phase = unsafe { &*info.cast::<HeldPhase>() };
    phase.sample();
}

#[cfg(any(test, debug_assertions))]
impl HoldSlot {
    fn take(&mut self) {
        match (self.hold)() {
            Ok(held) => {
                self.held = Some(held);
                if let Some(own_take) = &self.own_take {
                    own_take.taken();
                }
                if let Some(phase) = &mut self.phase {
                    phase.enter();
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
#[cfg(any(test, debug_assertions))]
struct TimedHold {
    slot: Box<HoldSlot>,
    timer: Option<CFRunLoopTimer>,
    invalidate: fn(&CFRunLoopTimer),
}

#[cfg(any(test, debug_assertions))]
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

    fn give_back(mut self) -> Result<(), SecureInputReleaseRefused> {
        let held = self.slot.held.take();
        if let Some(phase) = &mut self.slot.phase {
            phase.end();
        }
        drop(self);
        held.map_or(Ok(()), SecureInputHold::release)
    }
}

#[cfg(any(test, debug_assertions))]
impl Drop for TimedHold {
    fn drop(&mut self) {
        if let Some(timer) = self.timer.take() {
            (self.invalidate)(&timer);
        }
    }
}

#[cfg(any(test, debug_assertions))]
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
    port: TapPort,
    thread: Option<JoinHandle<()>>,
}

struct TapPlan {
    plan: HoldPlan,
    #[cfg(any(test, debug_assertions))]
    hold: Hold,
    refusal: Arc<Mutex<Option<SecureInputRefused>>>,
    released: Arc<Mutex<Option<SecureInputReleaseRefused>>>,
    #[cfg(any(test, debug_assertions))]
    own_take: Option<OwnTake>,
    #[cfg(any(test, debug_assertions))]
    phase: Option<HeldPhase>,
}

impl TapThread {
    fn spawn<K: 'static>(
        install: impl FnOnce() -> Result<K, CaptureError> + Send + 'static,
        tap_plan: TapPlan,
        stop: Arc<AtomicBool>,
        port: TapPort,
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
                        #[cfg(any(test, debug_assertions))]
                        hold,
                        refusal,
                        released,
                        #[cfg(any(test, debug_assertions))]
                        own_take,
                        #[cfg(any(test, debug_assertions))]
                        phase,
                    } = tap_plan;
                    #[cfg(not(any(test, debug_assertions)))]
                    let _ = (plan, refusal);
                    #[cfg(any(test, debug_assertions))]
                    let mut slot = TimedHold::new(HoldSlot {
                        hold,
                        held: None,
                        refusal,
                        own_take,
                        phase,
                    });
                    #[cfg(any(test, debug_assertions))]
                    if plan == HoldPlan::After(Duration::ZERO) {
                        slot.take_now();
                    }
                    let installed = match install() {
                        Ok(installed) => installed,
                        Err(e) => {
                            loop_tx.send(Err(e)).ok();
                            return;
                        }
                    };
                    #[cfg(any(test, debug_assertions))]
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
                    loop_tx.send(Ok(CFRunLoop::get_current())).ok();
                    before_run();
                    while !stop.load(Ordering::SeqCst) {
                        // SAFETY: `kCFRunLoopDefaultMode` is a constant CFString
                        // that CoreFoundation exports and never writes.
                        CFRunLoop::run_in_mode(unsafe { kCFRunLoopDefaultMode }, RUN_SLICE, false);
                    }
                    drop(installed);
                    #[cfg(any(test, debug_assertions))]
                    let given_back = slot.give_back();
                    #[cfg(not(any(test, debug_assertions)))]
                    let given_back: Result<(), SecureInputReleaseRefused> = Ok(());
                    invalidate(&idle);
                    if let Err(status) = given_back {
                        *released.lock().unwrap_or_else(PoisonError::into_inner) = Some(status);
                    }
                })
                .map_err(spawn_failure)?
        };

        match loop_rx.recv() {
            Ok(Ok(run_loop)) => Ok(Self {
                run_loop,
                stop,
                port,
                thread: Some(thread),
            }),
            Ok(Err(e)) => {
                thread.join().ok();
                Err(e)
            }
            Err(_) => {
                thread.join().ok();
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
                self.port.disable();
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
    floor: FloorHandle,
    off: bool,
    keys_missing: Arc<AtomicBool>,
    #[cfg(any(test, debug_assertions))]
    typed: fn() -> u32,
    counters: InputCounters,
    plan: HoldPlan,
    #[cfg(any(test, debug_assertions))]
    hold: Hold,
    bounds: Bounds,
    step: Duration,
    session: fn() -> Session,
    front: Option<fn() -> Option<libc::pid_t>>,
    #[cfg(all(debug_assertions, feature = "stall-seam"))]
    drops: Option<DropCounts>,
    refusal: Arc<Mutex<Option<SecureInputRefused>>>,
    released: Arc<Mutex<Option<SecureInputReleaseRefused>>>,
    tap_enable: TapEnable,
    tap: Option<TapThread>,
    notice: Option<NoticeThread>,
    failure: Option<CaptureError>,
    unstopped: Option<CaptureError>,
    stopped: bool,
    #[cfg(test)]
    real: bool,
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

    /// A source that takes the bound, the gap bound, the timer's step and the
    /// session reader of `seam` and counts its drops in `seam.drops`.
    #[cfg(all(debug_assertions, feature = "stall-seam"))]
    pub fn with_seam(
        foreground: ForegroundReader,
        counters: InputCounters,
        plan: HoldPlan,
        seam: StallSeam,
    ) -> Self {
        let mut source = Self::with_bounds(
            foreground,
            counters,
            plan,
            Bounds {
                hold: seam.bound,
                gap: seam.gap,
                ..Bounds::PRODUCT
            },
        );
        source.step = seam.step;
        source.session = seam.session;
        source.front = Some(seam.front);
        source.drops = Some(seam.drops);
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
            floor: FloorHandle::default(),
            off: false,
            keys_missing: Arc::new(AtomicBool::new(false)),
            #[cfg(any(test, debug_assertions))]
            typed: hid_key_downs,
            counters,
            plan,
            #[cfg(any(test, debug_assertions))]
            hold: hold_secure_input,
            bounds,
            step: READING_STEP,
            session: read_session,
            front: None,
            #[cfg(all(debug_assertions, feature = "stall-seam"))]
            drops: None,
            refusal: Arc::new(Mutex::new(None)),
            released: Arc::new(Mutex::new(None)),
            tap_enable: CGEventTapEnable,
            tap: None,
            notice: None,
            failure: None,
            unstopped: None,
            stopped: false,
            #[cfg(test)]
            real: false,
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
        if self.unstopped.is_some() {
            return Err(CaptureError::AlreadyStarted);
        }
        *self.refusal.lock().unwrap_or_else(PoisonError::into_inner) = None;
        *self.released.lock().unwrap_or_else(PoisonError::into_inner) = None;
        self.counters.set_stalled(false);
        self.keys_missing.store(false, Ordering::SeqCst);
        let switches = self.floor.switches();
        self.off = !switches.is_on(Switch::KeyClass) && !switches.is_on(Switch::PointerClass);
        if self.off {
            self.failure = None;
            return Ok(Started::Off);
        }
        if switches.is_on(Switch::KeyClass) && M::listen_denied() {
            let refused = CaptureError::PermissionDenied(Modality::Keystroke);
            self.failure = Some(refused.clone());
            return Err(refused);
        }
        let settings = TapSettings {
            session: self.session,
            events: events_of_interest(switches),
            ..tap_settings()
        };
        let stop = Arc::new(AtomicBool::new(false));
        #[cfg_attr(not(all(debug_assertions, feature = "stall-seam")), allow(unused_mut))]
        let mut ctx = tap_context(
            sink,
            self.foreground.clone(),
            self.counters.clone(),
            &settings,
            self.bounds,
            stop.clone(),
        );
        ctx.step = self.step;
        ctx.floor = self.floor.clone();
        ctx.port = TapPort::with_enable(self.tap_enable);
        let port = ctx.port.clone();
        #[cfg(all(debug_assertions, feature = "stall-seam"))]
        {
            ctx.drops = self.drops.clone();
        }
        let notice = if M::WATCHES_FOCUS {
            match self.notice_thread(ctx.notices.clone(), notice_clock(&settings)) {
                Ok(notice) => Some(notice),
                Err(e) => {
                    self.failure = Some(e.clone());
                    return Err(e);
                }
            }
        } else {
            None
        };
        #[cfg(any(test, debug_assertions))]
        let own_take = OwnTake {
            hold: ctx.hold.clone(),
            counters: self.counters.clone(),
            stop: stop.clone(),
            #[cfg(all(debug_assertions, feature = "stall-seam"))]
            drops: ctx.drops.clone(),
            #[cfg(all(debug_assertions, feature = "stall-seam"))]
            clock: ctx.clocks.stamps,
        };
        #[cfg(any(test, debug_assertions))]
        let phase = HeldPhase {
            held: ctx.held_phase.clone(),
            counters: self.counters.clone(),
            secure: settings.secure,
            session: settings.session,
            typed: self.typed,
            typed_at_take: None,
            sampler: None,
        };
        let keys_missing = self.keys_missing.clone();
        let started = TapThread::spawn(
            move || install_tap(ctx, settings, maker, keys_missing),
            TapPlan {
                plan: self.plan,
                #[cfg(any(test, debug_assertions))]
                hold: self.hold,
                refusal: self.refusal.clone(),
                released: self.released.clone(),
                #[cfg(any(test, debug_assertions))]
                own_take: Some(own_take),
                #[cfg(any(test, debug_assertions))]
                phase: Some(phase),
            },
            stop,
            port,
            before_run,
        );
        match started {
            Ok(tap) => {
                self.tap = Some(tap);
                self.notice = notice;
                self.failure = None;
                self.stopped = false;
                Ok(Started::Running)
            }
            Err(e) => {
                if let Some(mut notice) = notice {
                    notice.stop_within(INPUT_STOP_DEADLINE);
                }
                self.failure = Some(e.clone());
                Err(e)
            }
        }
    }

    fn notice_thread(
        &self,
        mark: NoticeMark,
        uptime: fn() -> Uptime,
    ) -> Result<NoticeThread, CaptureError> {
        let front: Box<dyn Fn() -> Option<libc::pid_t> + Send> = match self.front {
            Some(front) => Box::new(front),
            None => {
                let reader = self.foreground.clone();
                Box::new(move || reader.get().map(|front| front.pid))
            }
        };
        #[cfg(all(debug_assertions, feature = "stall-seam"))]
        let on_notice: crate::secure_input_macos::notice::OnNotice = match self.drops.clone() {
            Some(drops) => {
                Box::new(move |seen, focus| drops.notice(monotonic_raw().nanos(), seen, focus))
            }
            None => Box::new(|_, _| {}),
        };
        #[cfg(not(all(debug_assertions, feature = "stall-seam")))]
        let on_notice: crate::secure_input_macos::notice::OnNotice = Box::new(|_, _| {});
        NoticeThread::spawn(front, mark, uptime, on_notice).map_err(spawn_failure)
    }
}

/// The clock of the notice mark: the reader that gives an event's entry its
/// OS time, so that a mark and a key's time are read on one clock.
fn notice_clock(settings: &TapSettings) -> fn() -> Uptime {
    settings.clocks.os_time
}

impl CaptureSource for InputSource {
    fn begin(&mut self, ctx: SourceContext) -> Result<Started, CaptureError> {
        #[cfg(test)]
        {
            self.real = true;
        }
        self.floor = ctx.floor().clone();
        self.begin_with(ctx.sink().clone(), SystemTapMaker, || {})
    }

    fn shutdown(&mut self) -> Result<(), CaptureError> {
        let Some(tap) = self.tap.as_mut() else {
            return self.unstopped.clone().map_or(Ok(()), Err);
        };
        let joined = tap.stop_within(INPUT_STOP_DEADLINE);
        if let Some(mut notice) = self.notice.take() {
            notice.stop_within(INPUT_STOP_DEADLINE);
        }
        let answer = match joined {
            Ok(()) => {
                self.tap = None;
                self.release_answer()
            }
            Err(CaptureError::ThreadEnded) => {
                self.tap = None;
                Err(CaptureError::ThreadEnded)
            }
            Err(error) => Err(error),
        };
        self.answered(answer)
    }

    /// The state of the tap thread and the counters. A named limit (rulings
    /// MA-30 M8, MA-31 N8): while the notice thread watches nothing and no
    /// doubt stands (no Accessibility grant, nothing in front, a registration
    /// refused with an answer) or has ended, the source reads running and
    /// guards as macos-2b did; while a doubt stands it drops every held key at
    /// each reading and every key that arrives, more than macos-2b did, and
    /// still reads running. Health says nothing of the notice or the doubt: a
    /// drop by doubt does not read as impaired. A late key-up, or another
    /// reading event, whose reading finds a new mark or a standing doubt
    /// leaves the stalled flag unset, because the mark's drop takes the held
    /// keys first; with no mark it sets it. The figure max_lag_ms has no
    /// writer on a Mac and reads 0.
    fn health(&self) -> SourceHealth {
        let state = match (&self.tap, &self.failure) {
            (Some(tap), _) if tap.ended() => source_failure(CaptureError::ThreadEnded),
            (Some(_), _) if self.counters.stalled() => SourceState::Impaired {
                reason: HealthReason::RunLoopStalled,
            },
            (Some(_), _) => SourceState::Running,
            (None, Some(error)) if self.keys_missing.load(Ordering::SeqCst) => {
                SourceState::Failed {
                    reason: HealthReason::NoKeyEvents,
                    error: error.clone(),
                }
            }
            (None, Some(error)) => source_failure(error.clone()),
            (None, None) if self.off => SourceState::Off,
            (None, None) if self.stopped => SourceState::Stopped(StopCause::Request),
            (None, None) => SourceState::NotReported,
        };
        let state = self.unstopped.clone().map_or(state, source_failure);
        SourceHealth {
            state,
            counters: self.counters.figures(),
        }
    }
}

impl InputSource {
    fn release_answer(&self) -> Result<(), CaptureError> {
        match *self.released.lock().unwrap_or_else(PoisonError::into_inner) {
            None => Ok(()),
            Some(SecureInputReleaseRefused(code)) => Err(CaptureError::Os {
                stage: "secure_input_release",
                domain: "Carbon",
                code: i64::from(code),
            }),
        }
    }

    fn answered(&mut self, answer: Result<(), CaptureError>) -> Result<(), CaptureError> {
        match &answer {
            Ok(()) => {
                self.unstopped = None;
                self.stopped = true;
            }
            Err(CaptureError::ThreadEnded) => {
                self.failure = Some(CaptureError::ThreadEnded);
            }
            Err(error) => self.unstopped = Some(error.clone()),
        }
        #[cfg(test)]
        crate::macos::foreground::note_shutdown_answer("input", self.real, &answer);
        answer
    }
}

impl Drop for InputSource {
    fn drop(&mut self) {
        if self.unstopped.is_some() {
            return;
        }
        self.shutdown().ok();
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

    #[cfg(halvo_proof = "tap_event_printed")]
    fn tap_event_printed(event: &super::TapEvent) -> String {
        format!("{event:?}")
    }

    #[cfg(halvo_proof = "tap_event_printed_twin")]
    fn tap_event_printed_twin(event: &super::TapEvent) -> String {
        format!("{:?}", event.os_time)
    }

    #[cfg(halvo_proof = "seam_in_a_release_build")]
    fn seam_in_a_release_build(
        foreground: crate::macos::foreground::ForegroundReader,
    ) -> super::InputSource {
        super::InputSource::with_seam(
            foreground,
            super::InputCounters::new(),
            super::HoldPlan::Never,
            todo!(),
        )
    }

    #[cfg(halvo_proof = "seam_in_a_release_build_twin")]
    fn seam_in_a_release_build_twin(
        foreground: crate::macos::foreground::ForegroundReader,
    ) -> super::InputSource {
        super::InputSource::new(
            foreground,
            super::InputCounters::new(),
            super::HoldPlan::Never,
        )
    }

    #[cfg(halvo_proof = "drop_counts_in_a_release_build")]
    fn drop_counts_in_a_release_build() -> super::DropCounts {
        super::DropCounts::new()
    }

    #[cfg(halvo_proof = "drop_counts_in_a_release_build_twin")]
    fn drop_counts_in_a_release_build_twin() -> super::InputCounters {
        super::InputCounters::new()
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

    #[cfg(halvo_proof = "hold_plan_after_in_a_release_build")]
    fn hold_plan_after_in_a_release_build() -> super::HoldPlan {
        super::HoldPlan::After(std::time::Duration::ZERO)
    }

    #[cfg(halvo_proof = "hold_plan_after_in_a_release_build_twin")]
    fn hold_plan_after_in_a_release_build_twin() -> super::HoldPlan {
        super::HoldPlan::Never
    }

    #[cfg(halvo_proof = "take_in_a_release_build")]
    fn take_in_a_release_build() -> bool {
        crate::secure_input_macos::hold_secure_input().is_ok()
    }

    #[cfg(halvo_proof = "take_in_a_release_build_twin")]
    fn take_in_a_release_build_twin() -> bool {
        crate::secure_input_macos::secure_input_active()
    }

    #[cfg(halvo_proof = "hold_with_in_a_release_build")]
    fn hold_with_in_a_release_build() -> bool {
        crate::secure_input_macos::hold_with(|| 0, || 0).is_ok()
    }

    #[cfg(halvo_proof = "hold_with_in_a_release_build_twin")]
    fn hold_with_in_a_release_build_twin() -> bool {
        crate::secure_input_macos::secure_input_active()
    }

    #[cfg(halvo_proof = "secure_input_hold_in_a_release_build")]
    fn secure_input_hold_in_a_release_build() -> Option<crate::secure_input_macos::SecureInputHold>
    {
        None
    }

    #[cfg(halvo_proof = "secure_input_hold_in_a_release_build_twin")]
    fn secure_input_hold_in_a_release_build_twin(
    ) -> Option<crate::secure_input_macos::SecureInputRefused> {
        None
    }

    #[cfg(halvo_proof = "carbon_release_in_a_release_build")]
    fn carbon_release_in_a_release_build() -> i32 {
        crate::secure_input_macos::disable_secure_event_input()
    }

    #[cfg(halvo_proof = "carbon_release_in_a_release_build_twin")]
    fn carbon_release_in_a_release_build_twin() -> bool {
        crate::secure_input_macos::secure_input_active()
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

#[cfg(all(test, debug_assertions))]
mod tests {
    use super::*;
    use crate::floor::cases;
    use crate::macos::foreground::foreground_state_for_test;
    use crate::secure_input_macos::hold::Timing;
    use crate::secure_input_macos::test_support::{
        agreed_reading, hosted_or_refuse, one_hold_at_a_time, reads_on_within, skip_allowed,
        HOLD_REQUIRED,
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
        static NOTICE_AT_ASK: RefCell<Option<(usize, NoticeMark)>> = const { RefCell::new(None) };
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
        NOTICE_AT_ASK.with(|notice| {
            if let Some((at, mark)) = &*notice.borrow() {
                if *at == asked {
                    mark.mark(Uptime::from_nanos(UPTIME.with(Cell::get)), MarkKind::Notice);
                }
            }
        });
        if JUMP_AT_ASK.with(Cell::get) == Some(asked) {
            MONOTONIC.with(|now| now.set(now.get() + 40 * MS));
            UPTIME.with(|now| now.set(now.get() + 40 * MS));
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
        UPTIME.with(|now| now.set(now.get() + step));
        MONOTONIC.with(|now| {
            now.set(now.get() + step);
            Monotonic::from_nanos(now.get())
        })
    }

    fn stand_in_uptime() -> Uptime {
        Uptime::from_nanos(UPTIME.with(Cell::get))
    }

    const STAND_IN_CLOCKS: Clocks = Clocks {
        stamps: stand_in_monotonic,
        os_time: stand_in_uptime,
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
        NOTICE_AT_ASK.with(|notice| *notice.borrow_mut() = None);
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
                lag: Duration::from_millis(30),
                cap,
            },
            Arc::new(AtomicBool::new(false)),
        ));
        ctx.start(&mut lock(&ctx.hold));
        fg_writer.set_for_test(Some(APP.front()));
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

    struct TestApp(&'static str, libc::pid_t);

    impl TestApp {
        fn front(&self) -> Foreground {
            crate::macos::foreground::tests_support::front(self.0, self.1)
        }
    }

    const APP: TestApp = TestApp("com.example.editor", 4242);
    const LISTED: TestApp = TestApp("com.apple.Passwords", 4343);

    fn pointer_records(writer: &sink::SinkWriter) -> Vec<(&'static str, u64)> {
        received(writer)
            .into_iter()
            .filter_map(|received| received.into_floor().ok())
            .filter_map(|floor| match floor.event().observation() {
                crate::floor::payload::Observation::Click(button) => Some((button.as_str(), 1)),
                crate::floor::payload::Observation::Fold(fold) => {
                    Some((fold.class().as_str(), fold.count().get()))
                }
                _ => None,
            })
            .collect()
    }

    fn pointer(ctx: &TapContext, etype: CGEventType, times: usize) {
        for _ in 0..times {
            advance(1);
            on_tap_event(ctx, event(etype));
        }
    }

    #[test]
    fn a_press_fifty_drags_and_a_release_give_one_click_and_one_drag_record_of_50() {
        let held = held(100);
        pointer(&held.ctx, CGEventType::LeftMouseDown, 1);
        pointer(&held.ctx, CGEventType::LeftMouseDragged, 50);
        pointer(&held.ctx, CGEventType::LeftMouseUp, 1);
        assert_eq!(pointer_records(&held.writer), [("primary", 1)]);
        close_interval(&held.ctx);
        assert_eq!(pointer_records(&held.writer), [("drag", 50)]);
        close_interval(&held.ctx);
        assert_eq!(
            pointer_records(&held.writer),
            [],
            "an interval with no activity"
        );
    }

    #[test]
    fn thirty_scrolls_give_one_scroll_record_of_30_and_each_button_its_click() {
        let held = held(100);
        pointer(&held.ctx, CGEventType::ScrollWheel, 30);
        pointer(&held.ctx, CGEventType::RightMouseDown, 1);
        pointer(&held.ctx, CGEventType::OtherMouseDown, 1);
        pointer(&held.ctx, CGEventType::MouseMoved, 100);
        assert_eq!(
            pointer_records(&held.writer),
            [("secondary", 1), ("other", 1)]
        );
        close_interval(&held.ctx);
        assert_eq!(
            pointer_records(&held.writer),
            [("scroll", 30), ("move", 100)]
        );
    }

    #[test]
    fn after_a_flood_of_drags_and_scrolls_a_key_is_queued_at_once() {
        let held = held_with(100, 64, 4);
        for _ in 0..1_024 {
            pointer(&held.ctx, CGEventType::LeftMouseDragged, 1);
            pointer(&held.ctx, CGEventType::ScrollWheel, 1);
        }
        key_down(&held.ctx);
        ticks(&held.ctx, 120);
        assert_eq!(keystrokes(&held.writer), 1);
        close_interval(&held.ctx);
        assert_eq!(
            pointer_records(&held.writer),
            [("scroll", 1_024), ("drag", 1_024)]
        );
    }

    #[test]
    fn a_fold_that_a_full_queue_hands_back_is_counted_and_goes_into_the_next_interval() {
        let held = held_with(100, 64, 1);
        pointer(&held.ctx, CGEventType::LeftMouseDown, 1);
        pointer(&held.ctx, CGEventType::MouseMoved, 3);
        close_interval(&held.ctx);
        assert_eq!(held.counters.figures().pointer_coalesced, 1);
        pointer(&held.ctx, CGEventType::MouseMoved, 2);
        assert_eq!(pointer_records(&held.writer), [("primary", 1)]);
        close_interval(&held.ctx);
        assert_eq!(pointer_records(&held.writer), [("move", 5)]);
        assert_eq!(held.counters.figures().pointer_coalesced, 1);
    }

    #[test]
    fn no_pointer_record_leaves_for_a_listed_app_in_front_or_with_nothing_in_front() {
        for front in [Some(LISTED.front()), None] {
            let held = held(100);
            held.fg_writer.set_for_test(front);
            pointer(&held.ctx, CGEventType::LeftMouseDown, 1);
            pointer(&held.ctx, CGEventType::MouseMoved, 5);
            close_interval(&held.ctx);
            held.fg_writer.set_for_test(Some(APP.front()));
            close_interval(&held.ctx);
            assert_eq!(pointer_records(&held.writer), []);
        }
    }

    #[test]
    fn a_click_carries_the_time_of_its_os_event() {
        let held = held(100);
        let mut click = event(CGEventType::LeftMouseDown);
        click.os_time = Uptime::from_nanos(click.entry.uptime.nanos() - 3_000 * MS);
        on_tap_event(&held.ctx, click);
        let floor = received(&held.writer)
            .into_iter()
            .find_map(|received| received.into_floor().ok())
            .expect("a click");
        let now = crate::event::unix_nanos(std::time::SystemTime::now());
        let age = now - floor.event().at();
        assert!(
            (3_000 * MS as i64..6_000 * MS as i64).contains(&age),
            "the click's time lies 3 s back, at its resolution of 1 s: {age} ns"
        );
    }

    fn event(etype: CGEventType) -> TapEvent {
        let uptime = Uptime::from_nanos(UPTIME.with(Cell::get));
        TapEvent {
            etype,
            flags: 0,
            os_time: uptime,
            entry: Entry {
                monotonic: Monotonic::from_nanos(MONOTONIC.with(Cell::get)),
                uptime,
            },
        }
    }

    fn real_event(etype: CGEventType) -> TapEvent {
        event_now(etype, 0)
    }

    fn timed(etype: CGEventType, timing: Timing) -> TapEvent {
        let now = UPTIME.with(Cell::get);
        let os_time = match timing {
            Timing::InTime { lag } | Timing::Late { lag } => {
                now - u64::try_from(lag.as_nanos()).expect("a lag of the tests")
            }
            Timing::Untimed | Timing::Asleep | Timing::Unclocked | Timing::Noticed => 0,
        };
        TapEvent {
            os_time: Uptime::from_nanos(os_time),
            ..event(etype)
        }
    }

    fn with_flags(etype: CGEventType, flags: u64) -> TapEvent {
        TapEvent {
            flags,
            ..event(etype)
        }
    }

    const LEFT_SHIFT: i64 = 0x38;
    const RIGHT_SHIFT: i64 = 0x3C;
    const LEFT_COMMAND: i64 = 0x37;
    const LEFT_SHIFT_BIT: u64 = 0x2;
    const RIGHT_SHIFT_BIT: u64 = 0x4;

    fn with_key(etype: CGEventType, key_code: i64, flags: u64) -> Keyed {
        Keyed {
            event: TapEvent {
                flags,
                ..event(etype)
            },
            key_code: Some(key_code),
            autorepeat: false,
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

    thread_local! {
        static SLOW_SESSION_NS: Cell<u64> = const { Cell::new(0) };
        static SLOW_READ_NS: Cell<u64> = const { Cell::new(0) };
        static UPTIME_STEP: Cell<u64> = const { Cell::new(0) };
        static FIELD_ON_UNTIL: Cell<u64> = const { Cell::new(0) };
        static CALLS: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
    }

    fn stall(ns: u64) {
        MONOTONIC.with(|now| now.set(now.get() + ns));
        UPTIME.with(|now| now.set(now.get() + ns));
    }

    fn sleep_of_the_mac(ms: u64) {
        MONOTONIC.with(|now| now.set(now.get() + ms * MS));
    }

    fn slow_session() -> Session {
        stall(SLOW_SESSION_NS.with(Cell::get));
        stand_in_session()
    }

    fn slow_reader(session: Session) -> Reading {
        stall(SLOW_READ_NS.with(Cell::get));
        stand_in_reader(session)
    }

    fn field_reader(session: Session) -> Reading {
        stall(SLOW_READ_NS.with(Cell::get));
        let answer = stand_in_reader(session);
        if answer == Reading::Off && MONOTONIC.with(Cell::get) < FIELD_ON_UNTIL.with(Cell::get) {
            Reading::On
        } else {
            answer
        }
    }

    fn stepped_uptime() -> Uptime {
        let step = UPTIME_STEP.with(Cell::get);
        UPTIME.with(|now| {
            now.set(now.get() + step);
            Uptime::from_nanos(now.get())
        })
    }

    fn slow_held(bound_ms: u64) -> Held {
        let mut held = held(bound_ms);
        SLOW_SESSION_NS.with(|ns| ns.set(0));
        SLOW_READ_NS.with(|ns| ns.set(0));
        UPTIME_STEP.with(|ns| ns.set(0));
        FIELD_ON_UNTIL.with(|until| until.set(0));
        held.ctx.session = slow_session;
        held.ctx.secure = slow_reader;
        held
    }

    fn key_at(ctx: &TapContext, etype: CGEventType, os_time: u64) {
        let event = event_object(etype, 0);
        set_stamp(&event, os_time);
        let tap = tap_event(ctx, etype, &event);
        on_tap_event(ctx, tap);
    }

    fn now_uptime() -> u64 {
        UPTIME.with(Cell::get)
    }

    fn fate_of_one_key(session_ms: u64, late_ms: u64, sleep_ms: u64) -> (usize, usize) {
        let mut held = slow_held(1_000);
        held.ctx.secure = field_reader;
        stall(300 * MS);
        let os_time = now_uptime();
        FIELD_ON_UNTIL.with(|until| until.set(MONOTONIC.with(Cell::get) + 20 * MS));
        stall(late_ms * MS);
        sleep_of_the_mac(sleep_ms);
        SLOW_SESSION_NS.with(|ns| ns.set(session_ms * MS));
        key_at(&held.ctx, CGEventType::KeyDown, os_time);
        SLOW_SESSION_NS.with(|ns| ns.set(0));
        let held_after = held_now(&held.ctx);
        ticks(&held.ctx, 2_000);
        (held_after, keystrokes(&held.writer))
    }

    #[test]
    fn a_key_typed_into_a_field_that_stays_secure_20_ms_is_never_recorded_whatever_comes_before_its_reading(
    ) {
        let rows = [
            ("session read 0 ms, callback on time", 0, 0, 0),
            ("session read 0 ms, callback 40 ms late", 0, 40, 0),
            ("session read 40 ms, callback on time", 40, 0, 0),
            ("session read 1,000 ms, callback on time", 1_000, 0, 0),
            ("a sleep of 60 s between the key's time and its callback, 5 ms late on the uptime clock", 0, 5, 60_000),
        ];
        for (what, session_ms, late_ms, sleep_ms) in rows {
            let (held_after, recorded) = fate_of_one_key(session_ms, late_ms, sleep_ms);
            assert_eq!((held_after, recorded), (0, 0), "{what}");
        }
    }

    #[test]
    fn a_delay_inside_the_state_read_makes_a_key_late() {
        let held = slow_held(100);
        stall(MS);
        let os_time = now_uptime();
        SLOW_READ_NS.with(|ns| ns.set(40 * MS));
        key_at(&held.ctx, CGEventType::KeyDown, os_time);
        SLOW_READ_NS.with(|ns| ns.set(0));
        assert_eq!(
            held_now(&held.ctx),
            0,
            "the key's own read ended 40 ms after its time"
        );
        assert!(held.counters.figures().keys_withheld);
        ticks(&held.ctx, 300);
        assert_eq!(keystrokes(&held.writer), 0);
    }

    #[test]
    fn the_lag_to_the_end_of_the_reading_keeps_a_key_at_30_ms_and_withholds_it_at_30_ms_and_1_ns() {
        for (read_ns, held_after) in [(30 * MS, 1), (30 * MS + 1, 0)] {
            let held = slow_held(100);
            stall(MS);
            let os_time = now_uptime();
            SLOW_READ_NS.with(|ns| ns.set(read_ns));
            key_at(&held.ctx, CGEventType::KeyDown, os_time);
            SLOW_READ_NS.with(|ns| ns.set(0));
            assert_eq!(
                held_now(&held.ctx),
                held_after,
                "a read that took {read_ns} ns"
            );
        }
    }

    #[test]
    fn a_key_whose_time_lies_after_the_entry_stamp_is_untimed() {
        let held = slow_held(100);
        key_down(&held.ctx);
        stall(MS);
        key_at(&held.ctx, CGEventType::KeyDown, now_uptime() + 1);
        assert_eq!(held_now(&held.ctx), 0);
        assert!(held.counters.stalled());
    }

    #[test]
    fn a_key_from_before_a_sleep_is_withheld_when_a_tick_came_between_the_wake_and_it() {
        let held = slow_held(100);
        stall(MS);
        let os_time = now_uptime();
        sleep_of_the_mac(60_000);
        tick(&held.ctx);
        key_at(&held.ctx, CGEventType::KeyDown, os_time);
        assert_eq!(held_now(&held.ctx), 0);
        ticks(&held.ctx, 300);
        assert_eq!(keystrokes(&held.writer), 0);
        assert!(!held.counters.stalled(), "a wake is no timing fault");
    }

    #[test]
    fn a_key_from_before_a_sleep_is_withheld_when_a_pointer_reading_came_between_the_wake_and_it() {
        let held = slow_held(100);
        stall(MS);
        let os_time = now_uptime();
        sleep_of_the_mac(60_000);
        on_tap_event(&held.ctx, event(CGEventType::MouseMoved));
        key_at(&held.ctx, CGEventType::KeyDown, os_time);
        assert_eq!(held_now(&held.ctx), 0);
        ticks(&held.ctx, 300);
        assert_eq!(keystrokes(&held.writer), 0);
    }

    #[test]
    fn two_keys_from_before_a_sleep_are_both_withheld() {
        let held = slow_held(100);
        stall(MS);
        let first = now_uptime();
        stall(MS);
        let second = now_uptime();
        sleep_of_the_mac(60_000);
        key_at(&held.ctx, CGEventType::KeyDown, first);
        key_at(&held.ctx, CGEventType::KeyDown, second);
        assert_eq!(held_now(&held.ctx), 0);
        ticks(&held.ctx, 300);
        assert_eq!(keystrokes(&held.writer), 0);
    }

    #[test]
    fn a_step_between_the_two_uptime_reads_may_add_a_drop_and_never_hides_a_sleep() {
        let mut held = slow_held(100);
        held.ctx.clocks = Clocks {
            stamps: stand_in_monotonic,
            os_time: stepped_uptime,
        };
        stall(MS);
        let os_time = now_uptime();
        sleep_of_the_mac(60_000);
        UPTIME_STEP.with(|step| step.set(5 * MS));
        key_at(&held.ctx, CGEventType::KeyDown, os_time);
        UPTIME_STEP.with(|step| step.set(0));
        assert_eq!(held_now(&held.ctx), 0);
        ticks(&held.ctx, 300);
        assert_eq!(keystrokes(&held.writer), 0);
    }

    #[test]
    fn a_key_from_before_a_sleep_is_withheld_when_its_callback_comes_after_time_passed_since_the_wake(
    ) {
        let held = slow_held(100);
        stall(MS);
        let os_time = now_uptime();
        sleep_of_the_mac(60_000);
        tick(&held.ctx);
        stall(5 * MS);
        key_at(&held.ctx, CGEventType::KeyDown, os_time);
        assert_eq!(
            held_now(&held.ctx),
            0,
            "its OS time is not after the wake mark"
        );
        ticks(&held.ctx, 300);
        assert_eq!(keystrokes(&held.writer), 0);
    }

    #[test]
    fn the_seam_counts_beside_late_the_events_late_by_the_entry_stamp_alone() {
        let (mut held, drops) = seam_held(1_000);
        advance(1);
        on_tap_event(
            &held.ctx,
            timed(
                CGEventType::KeyDown,
                Timing::Late {
                    lag: Duration::from_millis(40),
                },
            ),
        );
        assert_eq!(
            (drops.of("late"), drops.late_by_the_entry()),
            (1, 1),
            "a key 40 ms late at the entry"
        );
        SLOW_READ_NS.with(|ns| ns.set(40 * MS));
        held.ctx.secure = slow_reader;
        key_down(&held.ctx);
        SLOW_READ_NS.with(|ns| ns.set(0));
        assert_eq!(
            (drops.of("late"), drops.late_by_the_entry()),
            (2, 1),
            "the second key was late by its reading alone"
        );
    }

    #[test]
    fn an_os_time_of_0_is_untimed_and_not_late() {
        let (held, drops) = seam_held(1_000);
        advance(1);
        on_tap_event(&held.ctx, timed(CGEventType::KeyDown, Timing::Untimed));
        assert_eq!((drops.of("untimed"), drops.of("late")), (1, 0));
    }

    type DoubtRow = (&'static str, fn(&Held));

    fn held_after_a_shift_change_that_follows(clear: fn(&Held), flags: u64) -> usize {
        let held = held(100);
        advance(1);
        on_tap_event(&held.ctx, with_flags(CGEventType::KeyUp, 0));
        clear(&held);
        advance(1);
        on_tap_event(
            &held.ctx,
            with_key(CGEventType::FlagsChanged, LEFT_SHIFT, flags),
        );
        held_now(&held.ctx)
    }

    #[test]
    fn a_modifier_change_after_a_doubt_is_read_from_its_own_key_code_and_flag_bit() {
        let rows: [DoubtRow; 7] = [
            ("a tap notice", |held| {
                on_tap_event(&held.ctx, event(CGEventType::TapDisabledByTimeout));
            }),
            ("an event's own reading of on", |held| {
                answer(Reading::On);
                on_tap_event(&held.ctx, event(CGEventType::MouseMoved));
                answer(Reading::Off);
            }),
            ("a tick's reading of on", |held| {
                answer(Reading::On);
                tick(&held.ctx);
                answer(Reading::Off);
            }),
            ("a reading of unknown", |held| {
                answer(Reading::Unknown);
                tick(&held.ctx);
                answer(Reading::Off);
            }),
            ("a wake", |held| {
                sleep_of_the_mac(60_000);
                tick(&held.ctx);
            }),
            ("a late event", |held| {
                on_tap_event(
                    &held.ctx,
                    timed(
                        CGEventType::KeyUp,
                        Timing::Late {
                            lag: Duration::from_millis(40),
                        },
                    ),
                );
            }),
            ("an untimed event", |held| {
                on_tap_event(&held.ctx, timed(CGEventType::KeyUp, Timing::Untimed));
            }),
        ];
        let shift = CGEventFlags::CGEventFlagShift.bits();
        for (what, clear) in rows {
            assert_eq!(
                held_after_a_shift_change_that_follows(clear, shift | LEFT_SHIFT_BIT),
                1,
                "a press after {what}"
            );
            assert_eq!(
                held_after_a_shift_change_that_follows(clear, 0),
                0,
                "a release after {what}"
            );
        }
        assert_eq!(
            held_after_a_shift_change_that_follows(|_| {}, shift | LEFT_SHIFT_BIT),
            1,
            "the control: a press with nothing before it"
        );
    }

    #[test]
    fn a_modifier_release_after_the_tap_was_switched_off_is_not_held() {
        let held = held(100);
        advance(1);
        on_tap_event(&held.ctx, with_flags(CGEventType::KeyUp, 0x100));
        advance(1);
        on_tap_event(&held.ctx, event(CGEventType::TapDisabledByTimeout));
        advance(1);
        on_tap_event(
            &held.ctx,
            with_key(CGEventType::FlagsChanged, RIGHT_SHIFT, 0x2_0102),
        );
        assert_eq!(held_now(&held.ctx), 0);
        ticks(&held.ctx, 120);
        assert_eq!(keystrokes(&held.writer), 0);
    }

    #[test]
    fn a_tap_notice_after_a_stop_moves_no_flag_and_does_not_switch_the_tap_on() {
        ENABLE_CALLS.with(|calls| calls.borrow_mut().clear());
        let mut held = held(100);
        held.ctx.port = TapPort::with_enable(record_enable);
        let tap = std::ptr::NonNull::<c_void>::dangling().as_ptr();
        held.ctx.port.store(tap);
        key_down(&held.ctx);
        held.ctx.stop.store(true, Ordering::SeqCst);
        advance(1);
        on_tap_event(&held.ctx, event(CGEventType::TapDisabledByTimeout));
        assert_eq!(held_now(&held.ctx), 0);
        assert_eq!(
            (
                held.counters.figures().keys_withheld,
                held.counters.stalled()
            ),
            (false, false)
        );
        ENABLE_CALLS.with(|calls| assert!(calls.borrow().is_empty()));
    }

    #[test]
    fn a_stop_asked_during_the_release_loops_reading_moves_no_flag() {
        let held = held(1_000);
        key_down(&held.ctx);
        key_down(&held.ctx);
        ticks(&held.ctx, 990);
        let stop = held.ctx.stop.clone();
        STOP_AT_ASK.with(|at| *at.borrow_mut() = Some((asked() + 2, stop)));
        JUMP_AT_ASK.with(|jump| jump.set(Some(asked() + 2)));
        tick(&held.ctx);
        assert_eq!(
            (
                keystrokes(&held.writer),
                held_now(&held.ctx),
                held.counters.figures().keys_withheld,
                held.counters.stalled()
            ),
            (1, 0, false, false)
        );
    }

    #[test]
    fn the_daemons_own_take_after_a_stop_drops_the_keys_and_moves_no_flag() {
        let held = held(1_000);
        key_down(&held.ctx);
        held.ctx.stop.store(true, Ordering::SeqCst);
        let mut slot = HoldSlot {
            hold: || crate::secure_input_macos::hold_with(|| 0, || 0),
            held: None,
            refusal: Arc::new(Mutex::new(None)),
            own_take: Some(OwnTake {
                hold: held.ctx.hold.clone(),
                counters: held.counters.clone(),
                stop: held.ctx.stop.clone(),
                drops: held.ctx.drops.clone(),
                clock: held.ctx.clocks.stamps,
            }),
            phase: None,
        };
        slot.take();
        assert_eq!(held_now(&held.ctx), 0);
        assert!(!held.counters.figures().keys_withheld);
        drop(slot);
    }

    fn only_drop(entries: &[RawEntry]) -> (u64, Cause, usize, bool, u64) {
        let at_drop = entries
            .iter()
            .position(|entry| matches!(entry, RawEntry::Dropped { .. }))
            .expect("a drop entry");
        let last_reading = entries[..at_drop]
            .iter()
            .rev()
            .find_map(|entry| match *entry {
                RawEntry::Reading { before, .. } => Some(before),
                _ => None,
            })
            .unwrap_or(0);
        let drops = entries
            .iter()
            .filter(|entry| matches!(entry, RawEntry::Dropped { .. }))
            .count();
        assert_eq!(drops, 1, "one drop entry: {entries:?}");
        match entries[at_drop] {
            RawEntry::Dropped {
                at,
                cause,
                keys,
                unread,
            } => (at, cause, keys, unread, last_reading),
            _ => unreachable!(),
        }
    }

    #[test]
    fn the_seam_counts_and_enters_every_drop_by_its_cause() {
        for case in [
            "own_take",
            "stop_at_tick",
            "stop_at_event",
            "tap_off",
            "refused",
            "stop_in_release",
        ] {
            let (held, drops) = seam_held(1_000);
            key_down(&held.ctx);
            key_down(&held.ctx);
            if case != "stop_in_release" {
                stall(5 * MS);
            }
            let mut writer = Some(held.writer);
            let (cause, keys, unread) = match case {
                "own_take" => {
                    OwnTake {
                        hold: held.ctx.hold.clone(),
                        counters: held.counters.clone(),
                        stop: held.ctx.stop.clone(),
                        drops: held.ctx.drops.clone(),
                        clock: held.ctx.clocks.stamps,
                    }
                    .taken();
                    (Cause::OwnTake, 2, true)
                }
                "stop_at_tick" => {
                    held.ctx.stop.store(true, Ordering::SeqCst);
                    tick(&held.ctx);
                    (Cause::Stop, 2, true)
                }
                "stop_at_event" => {
                    held.ctx.stop.store(true, Ordering::SeqCst);
                    on_tap_event(&held.ctx, event(CGEventType::KeyDown));
                    (Cause::Stop, 2, true)
                }
                "tap_off" => {
                    on_tap_event(&held.ctx, event(CGEventType::TapDisabledByTimeout));
                    (Cause::TapSwitchedOff, 2, true)
                }
                "refused" => {
                    drop(writer.take());
                    ticks(&held.ctx, 1_000);
                    (Cause::Refused, 2, false)
                }
                _ => {
                    ticks(&held.ctx, 990);
                    let stop = held.ctx.stop.clone();
                    STOP_AT_ASK.with(|at| *at.borrow_mut() = Some((asked() + 2, stop)));
                    tick(&held.ctx);
                    assert_eq!(
                        keystrokes(writer.as_ref().expect("the writer")),
                        1,
                        "the second key was due at the release loop's reading that saw the stop asked"
                    );
                    (Cause::Stop, 1, false)
                }
            };
            let entries = drops.take_entries();
            let (at, entered, entered_keys, entered_unread, last_reading) = only_drop(&entries);
            assert_eq!(
                (entered, entered_keys, entered_unread),
                (cause, keys, unread),
                "{case}: {entries:?}"
            );
            let want = if unread {
                MONOTONIC.with(Cell::get)
            } else {
                last_reading
            };
            assert_eq!(
                at, want,
                "{case}: a drop that took no reading carries its own stamp, any other the stamp of the reading that made it"
            );
            let name = CAUSES
                .iter()
                .find(|&&(listed, _)| listed == cause)
                .map(|&(_, name)| name)
                .expect("a named cause");
            assert_eq!(drops.of(name), 1, "{case}");
            assert_eq!(held_now(&held.ctx), 0, "{case}");
        }
    }

    fn replayed_fates(entries: &[RawEntry]) -> Vec<String> {
        let mut fates: Vec<String> = Vec::new();
        let mut held: Vec<usize> = Vec::new();
        for entry in entries {
            match *entry {
                RawEntry::Arrival { admission, .. } => match admission {
                    Arrived::Held => {
                        held.push(fates.len());
                        fates.push(String::from("held"));
                    }
                    Arrived::Suppressed => fates.push(String::from("suppressed")),
                    Arrived::Withheld { cause, dropped } => {
                        assert_eq!(
                            dropped,
                            held.len(),
                            "a withheld arrival drops every held key"
                        );
                        for index in held.drain(..) {
                            fates[index] = format!("dropped {cause:?}");
                        }
                        fates.push(format!("withheld {cause:?}"));
                    }
                },
                RawEntry::Released { .. } => {
                    let index = held.remove(0);
                    fates[index] = String::from("released");
                }
                RawEntry::Dropped { cause, keys, .. } => {
                    assert_eq!(
                        keys,
                        held.len(),
                        "a drop takes every key held at that point of the log"
                    );
                    for index in held.drain(..) {
                        fates[index] = format!("dropped {cause:?}");
                    }
                }
                RawEntry::Reading { .. } | RawEntry::Notice { .. } => {}
            }
        }
        fates
    }

    #[test]
    fn a_replay_of_the_log_in_its_order_gives_each_key_its_true_fate() {
        let (held, drops) = seam_held(100);
        key_down(&held.ctx);
        key_down(&held.ctx);
        stall(40 * MS);
        key_down(&held.ctx);
        ticks(&held.ctx, 120);
        key_down(&held.ctx);
        key_down(&held.ctx);
        answer(Reading::On);
        key_down(&held.ctx);
        assert_eq!(keystrokes(&held.writer), 1);
        assert_eq!(
            replayed_fates(&drops.take_entries()),
            [
                "dropped Gap",
                "dropped Gap",
                "released",
                "dropped On",
                "dropped On",
                "suppressed"
            ],
            "the third key's own reading found the gap that dropped the first two, and the sixth key's \
             own reading of on dropped the two before it"
        );
    }

    extern "C" {
        fn CFRunLoopTimerGetInterval(timer: CFRunLoopTimerRef) -> f64;
        fn CFRunLoopTimerGetNextFireDate(timer: CFRunLoopTimerRef) -> f64;
    }

    const FIRST_FIRE_MARGIN_S: f64 = 0.25;

    #[test]
    fn the_products_timer_first_fires_one_step_after_a_held_key_and_repeats_at_the_step() {
        reset_stand_ins();
        let (handle, writer) = sink::channel(64);
        let (_fg_writer, reader) = foreground_state_for_test();
        let ctx = Box::new(tap_context(
            handle,
            reader,
            InputCounters::new(),
            &stand_in_settings(),
            Bounds::PRODUCT,
            Arc::new(AtomicBool::new(false)),
        ));
        assert!(
            std::ptr::fn_addr_eq(
                ctx.timers.make,
                reading_timer as fn(*mut c_void, Duration) -> CFRunLoopTimer
            ),
            "the product's maker of the context makes its timer with the product's timer maker"
        );
        assert_eq!(ctx.step, READING_STEP);
        ctx.start(&mut lock(&ctx.hold));
        let before = CFDate::now().abs_time();
        key_down(&ctx);
        let after = CFDate::now().abs_time();
        let (interval, first) = {
            let state = lock(&ctx.hold);
            assert_eq!(state.keys.len(), 1, "the key is held");
            let Some(ReadingTimer(timer)) = &state.timer else {
                panic!("settle_timer made no timer for a held key");
            };
            // SAFETY: both calls read a field of a timer that the context owns
            // and that stays valid while its lock is held; the run loop of this
            // thread never runs, so the timer never fires.
            unsafe {
                (
                    CFRunLoopTimerGetInterval(timer.as_concrete_TypeRef()),
                    CFRunLoopTimerGetNextFireDate(timer.as_concrete_TypeRef()),
                )
            }
        };
        let step = READING_STEP.as_secs_f64();
        assert!(
            (interval - step).abs() < 1e-9,
            "the timer repeats at the step: interval {interval} s, step {step} s"
        );
        assert!(
            first > before && first <= after + step + FIRST_FIRE_MARGIN_S,
            "the first tick comes later than the make and no later than a step and {FIRST_FIRE_MARGIN_S} s \
             after it: {} s after the make",
            first - before
        );
        drop(ctx);
        drop(writer);
    }

    #[test]
    fn an_untimed_key_up_drops_every_held_key_and_reads_withheld_and_stalled() {
        let held = held(1_000);
        key_down(&held.ctx);
        key_down(&held.ctx);
        on_tap_event(&held.ctx, timed(CGEventType::KeyUp, Timing::Untimed));
        assert_eq!(
            (
                held_now(&held.ctx),
                held.counters.figures().keys_withheld,
                held.counters.stalled()
            ),
            (0, true, true)
        );
    }

    #[test]
    fn a_key_up_whose_flags_hold_a_new_modifier_bit_is_not_held() {
        let held = held(1_000);
        key_down(&held.ctx);
        advance(1);
        on_tap_event(
            &held.ctx,
            with_key(
                CGEventType::KeyUp,
                LEFT_SHIFT,
                CGEventFlags::CGEventFlagShift.bits() | LEFT_SHIFT_BIT,
            ),
        );
        assert_eq!(
            held_now(&held.ctx),
            1,
            "a key up is a reading alone, whatever its key code and flags"
        );
    }

    #[test]
    fn a_modifier_press_through_the_events_own_fields_is_read_from_its_key_code() {
        let held = held(100);
        let shift = CGEventFlags::CGEventFlagShift.bits();
        let press = event_object(CGEventType::FlagsChanged, shift | LEFT_SHIFT_BIT);
        press.set_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE, LEFT_SHIFT);
        advance(1);
        set_stamp(&press, now_uptime());
        on_tap_event(
            &held.ctx,
            tap_event(&held.ctx, CGEventType::FlagsChanged, &press),
        );
        assert_eq!(
            held_now(&held.ctx),
            1,
            "the press of the left shift is held"
        );
        let release = event_object(CGEventType::FlagsChanged, 0);
        release.set_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE, LEFT_SHIFT);
        advance(1);
        set_stamp(&release, now_uptime());
        on_tap_event(
            &held.ctx,
            tap_event(&held.ctx, CGEventType::FlagsChanged, &release),
        );
        assert_eq!(held_now(&held.ctx), 1, "its release is a reading alone");
    }

    #[test]
    fn a_tap_notice_with_no_key_held_moves_neither_flag() {
        let held = held(1_000);
        on_tap_event(&held.ctx, event(CGEventType::TapDisabledByTimeout));
        assert_eq!(
            (
                held.counters.figures().keys_withheld,
                held.counters.stalled()
            ),
            (false, false)
        );
    }

    #[test]
    fn the_daemons_own_take_with_no_key_held_moves_no_flag() {
        let held = held(1_000);
        OwnTake {
            hold: held.ctx.hold.clone(),
            counters: held.counters.clone(),
            stop: held.ctx.stop.clone(),
            drops: held.ctx.drops.clone(),
            clock: held.ctx.clocks.stamps,
        }
        .taken();
        assert!(!held.counters.figures().keys_withheld);
    }

    #[test]
    fn a_key_under_unknown_is_withheld_and_does_not_read_as_stalled() {
        let held = held(1_000);
        answer(Reading::Unknown);
        key_down(&held.ctx);
        assert_eq!(
            (
                held_now(&held.ctx),
                held.counters.figures().keys_withheld,
                held.counters.stalled()
            ),
            (0, true, false)
        );
    }

    #[test]
    fn a_refused_emit_or_a_stop_at_a_release_leaves_the_stalled_flag_as_it_was() {
        for case in ["refused", "stop"] {
            let held = held(100);
            key_down(&held.ctx);
            on_tap_event(&held.ctx, event(CGEventType::TapDisabledByTimeout));
            assert!(held.counters.stalled(), "{case}: the tap notice stalls");
            key_down(&held.ctx);
            key_down(&held.ctx);
            let mut writer = Some(held.writer);
            if case == "refused" {
                drop(writer.take());
                ticks(&held.ctx, 100);
            } else {
                ticks(&held.ctx, 90);
                let stop = held.ctx.stop.clone();
                STOP_AT_ASK.with(|at| *at.borrow_mut() = Some((asked() + 1, stop)));
                tick(&held.ctx);
            }
            assert_eq!(
                held_now(&held.ctx),
                0,
                "{case}: the release found the {case}"
            );
            if let Some(writer) = &writer {
                assert_eq!(keystrokes(writer), 0, "{case}");
            }
            assert!(
                held.counters.stalled(),
                "{case}: a key that was not emitted does not clear the stalled flag"
            );
        }
    }

    #[test]
    fn a_stand_in_that_moves_the_monotonic_clock_alone_by_more_than_the_bound_is_a_wake() {
        let (held, drops) = seam_held(1_000);
        key_down(&held.ctx);
        sleep_of_the_mac(31);
        tick(&held.ctx);
        assert_eq!(held_now(&held.ctx), 0);
        assert_eq!(
            (drops.of("sleep"), drops.of("gap")),
            (1, 0),
            "31 ms on the monotonic clock alone is a wake and no gap"
        );
        assert!(!held.counters.stalled());
    }

    #[test]
    fn a_sleep_with_keys_held_drops_them_and_sets_keys_withheld_and_not_the_stalled_flag() {
        let held = slow_held(100);
        key_down(&held.ctx);
        key_down(&held.ctx);
        sleep_of_the_mac(60_000);
        tick(&held.ctx);
        assert_eq!(held_now(&held.ctx), 0);
        assert!(held.counters.figures().keys_withheld);
        assert!(!held.counters.stalled());
        ticks(&held.ctx, 300);
        assert_eq!(keystrokes(&held.writer), 0);
    }

    #[test]
    fn the_instant_of_a_key_under_a_slow_session_read_is_its_time_and_not_the_end_of_its_reading() {
        let held = slow_held(100);
        stall(300 * MS);
        let os_time = now_uptime();
        SLOW_SESSION_NS.with(|ns| ns.set(20 * MS));
        SLOW_READ_NS.with(|ns| ns.set(0));
        key_at(&held.ctx, CGEventType::KeyDown, os_time);
        let after_call = std::time::SystemTime::now();
        SLOW_SESSION_NS.with(|ns| ns.set(0));
        ticks(&held.ctx, 300);
        let instants: Vec<i64> = received(&held.writer)
            .into_iter()
            .filter_map(|received| received.into_floor().ok())
            .filter(|floor| floor.event().kind() == Modality::Keystroke)
            .map(|floor| floor.event().at())
            .collect();
        assert_eq!(
            instants.len(),
            1,
            "a read of 20 ms keeps the key under a lag bound of 30 ms"
        );
        assert!(
            instants[0] <= crate::event::unix_nanos(after_call - Duration::from_millis(20)),
            "the instant is the key's time, 20 ms before the end of its reading, which came before the \
             wall time read after the call"
        );
    }

    fn mark_now(ctx: &TapContext) {
        ctx.notices
            .mark(Uptime::from_nanos(now_uptime()), MarkKind::Notice);
    }

    #[test]
    fn a_notice_drops_every_held_key_at_the_next_reading_and_sets_keys_withheld_and_not_the_stalled_flag(
    ) {
        let held = held(100);
        key_down(&held.ctx);
        key_down(&held.ctx);
        assert_eq!(held_now(&held.ctx), 2);
        mark_now(&held.ctx);
        tick(&held.ctx);
        assert_eq!(held_now(&held.ctx), 0);
        assert!(held.counters.figures().keys_withheld);
        assert!(!held.counters.stalled());
        ticks(&held.ctx, 300);
        assert_eq!(keystrokes(&held.writer), 0);
    }

    #[test]
    fn a_key_whose_time_is_not_after_the_notice_mark_is_withheld_and_a_key_after_it_is_held() {
        let held = held(100);
        advance(1);
        mark_now(&held.ctx);
        on_tap_event(&held.ctx, event(CGEventType::KeyDown));
        assert_eq!(held_now(&held.ctx), 0, "a key at the mark is withheld");
        assert!(held.counters.figures().keys_withheld);
        assert!(!held.counters.stalled());
        key_down(&held.ctx);
        assert_eq!(held_now(&held.ctx), 1, "a key after the mark is held");
        ticks(&held.ctx, 300);
        assert_eq!(keystrokes(&held.writer), 1);
    }

    #[test]
    fn a_notice_drops_the_held_keys_once_and_the_keys_after_it_are_released() {
        let held = held(100);
        key_down(&held.ctx);
        mark_now(&held.ctx);
        tick(&held.ctx);
        assert_eq!(held_now(&held.ctx), 0);
        key_down(&held.ctx);
        key_down(&held.ctx);
        ticks(&held.ctx, 300);
        assert_eq!(
            keystrokes(&held.writer),
            2,
            "a mark that the source has read drops nothing at a later reading"
        );
    }

    #[test]
    fn a_notice_inside_the_release_loop_drops_the_keys_behind_the_emitted_one() {
        let held = held(100);
        key_down(&held.ctx);
        key_down(&held.ctx);
        ticks(&held.ctx, 90);
        assert_eq!(keystrokes(&held.writer), 0);
        let asked_before = asked();
        NOTICE_AT_ASK
            .with(|at| *at.borrow_mut() = Some((asked_before + 2, held.ctx.notices.clone())));
        tick(&held.ctx);
        NOTICE_AT_ASK.with(|at| *at.borrow_mut() = None);
        assert_eq!(
            asked(),
            asked_before + 2,
            "the tick's reading and one reading of the release loop"
        );
        assert_eq!(keystrokes(&held.writer), 1);
        assert_eq!(held_now(&held.ctx), 0);
        assert!(held.counters.figures().keys_withheld);
    }

    #[test]
    fn a_notice_drops_the_keys_of_an_on_time_that_fell_between_two_readings() {
        let held = held(100);
        for _ in 0..8 {
            key_down(&held.ctx);
        }
        advance(5);
        mark_now(&held.ctx);
        ticks(&held.ctx, 300);
        assert_eq!(
            keystrokes(&held.writer),
            0,
            "every reading read off, and the notice of the focus change into the secure field dropped the keys"
        );
    }

    fn outcome_of(held: &Held) -> (usize, bool, bool, u64) {
        (
            held_now(&held.ctx),
            held.counters.figures().keys_withheld,
            held.counters.stalled(),
            held.counters.figures().late_events,
        )
    }

    #[test]
    fn a_late_key_not_after_a_notice_mark_stalls_and_counts_as_with_no_mark() {
        let mut outcomes = Vec::new();
        for with_mark in [false, true] {
            let held = held(1_000);
            key_down(&held.ctx);
            advance(1);
            if with_mark {
                mark_now(&held.ctx);
            }
            on_tap_event(
                &held.ctx,
                timed(
                    CGEventType::KeyDown,
                    Timing::Late {
                        lag: Duration::from_millis(300),
                    },
                ),
            );
            let outcome = outcome_of(&held);
            ticks(&held.ctx, 2_000);
            outcomes.push((outcome, keystrokes(&held.writer)));
        }
        assert_eq!(outcomes[0], outcomes[1], "with no mark, then with a mark");
        assert!(outcomes[1].0 .2, "a late key stalls the source");
        assert_eq!(
            outcomes[1].0 .3, 1,
            "a key 300 ms late counts in late_events"
        );
    }

    #[test]
    fn a_gap_of_readings_with_a_notice_mark_inside_it_stalls_as_with_no_mark() {
        let mut outcomes = Vec::new();
        for with_mark in [false, true] {
            let held = held(1_000);
            key_down(&held.ctx);
            advance(31);
            if with_mark {
                mark_now(&held.ctx);
            }
            on_reading_tick(&held.ctx);
            let outcome = outcome_of(&held);
            ticks(&held.ctx, 2_000);
            outcomes.push((outcome, keystrokes(&held.writer)));
        }
        assert_eq!(outcomes[0], outcomes[1], "with no mark, then with a mark");
        assert!(outcomes[1].0 .2, "a gap stalls the source");
    }

    #[test]
    fn a_key_ripe_for_release_whose_reading_sees_a_new_mark_is_dropped_and_not_released() {
        let held = held(100);
        key_down(&held.ctx);
        ticks(&held.ctx, 90);
        assert_eq!(keystrokes(&held.writer), 0);
        let asked_before = asked();
        NOTICE_AT_ASK
            .with(|at| *at.borrow_mut() = Some((asked_before + 1, held.ctx.notices.clone())));
        tick(&held.ctx);
        NOTICE_AT_ASK.with(|at| *at.borrow_mut() = None);
        assert_eq!(
            asked(),
            asked_before + 1,
            "one reading, at which the key was ripe"
        );
        assert_eq!(
            keystrokes(&held.writer),
            0,
            "the reading saw the mark before any release"
        );
        assert_eq!(held_now(&held.ctx), 0);
    }

    #[test]
    fn a_late_key_at_a_stop_between_the_lag_and_the_gap_bound_counts_as_late() {
        let held = slow_held(1_000);
        lock(&held.ctx.hold).keys = KeyQueue::new(Bounds {
            hold: Duration::from_millis(1_000),
            gap: Duration::from_millis(1_000),
            lag: Duration::from_millis(30),
            cap: 64,
        });
        let stop = held.ctx.stop.clone();
        STOP_AT_ASK.with(|at| *at.borrow_mut() = Some((asked() + 1, stop)));
        advance(1);
        let typed = now_uptime() - 300 * MS;
        key_with_os_time(&held.ctx, typed);
        STOP_AT_ASK.with(|at| *at.borrow_mut() = None);
        assert_eq!(
            held.counters.figures().late_events,
            1,
            "300 ms late at a stop, under a lag bound of 30 ms and a gap bound of 1,000 ms"
        );
    }

    #[test]
    fn the_seam_counts_the_keys_withheld_by_each_kind_of_mark() {
        for (kind, want) in [
            (MarkKind::Notice, (3, 0, 0)),
            (MarkKind::Doubt, (0, 3, 0)),
            (MarkKind::Start, (0, 0, 3)),
        ] {
            let (held, drops) = seam_held(100);
            key_down(&held.ctx);
            key_down(&held.ctx);
            advance(1);
            let at = now_uptime();
            held.ctx.notices.mark(Uptime::from_nanos(at), kind);
            tick(&held.ctx);
            assert_eq!(held_now(&held.ctx), 0, "{kind:?}: the reading drops both");
            advance(1);
            key_with_os_time(&held.ctx, at);
            assert_eq!(
                held_now(&held.ctx),
                0,
                "{kind:?}: a key not after the mark is withheld"
            );
            assert_eq!(drops.withheld_by_mark(), want, "{kind:?}");
        }
    }

    #[test]
    fn v174_g_doubt_end_window() {
        let (held, _drops) = seam_held(100);
        held.ctx.notices.set_doubt(true);
        key_down(&held.ctx);
        let held_after_key = held_now(&held.ctx);
        held.ctx.notices.set_doubt(false);
        ticks(&held.ctx, 300);
        let recorded = keystrokes(&held.writer);
        println!("V174G doubt_end_window held_after_key={held_after_key} keystrokes={recorded}");
        assert_eq!(
            (held_after_key, recorded),
            (0, 0),
            "a key that arrives while a doubt stands is dropped at its own reading, and the doubt's end with no mark releases nothing"
        );
    }

    fn doubt_from_now(ctx: &TapContext) {
        ctx.notices
            .mark(Uptime::from_nanos(now_uptime()), MarkKind::Doubt);
        ctx.notices.set_doubt(true);
    }

    #[test]
    fn a_key_typed_before_a_doubt_ends_and_read_after_its_end_is_never_released() {
        let (held, _drops) = seam_held(100);
        doubt_from_now(&held.ctx);
        tick(&held.ctx);
        advance(20);
        let end = now_uptime();
        held.ctx.notices.end_doubt(Uptime::from_nanos(end));
        advance(5);
        key_with_os_time(&held.ctx, end - 5 * MS);
        ticks(&held.ctx, 300);
        assert_eq!(
            keystrokes(&held.writer),
            0,
            "OS time end - 5 ms, read at end + 5 ms"
        );
    }

    #[test]
    fn a_key_typed_into_a_program_in_doubt_before_another_comes_in_front_is_never_released() {
        let (held, _drops) = seam_held(100);
        doubt_from_now(&held.ctx);
        tick(&held.ctx);
        advance(20);
        let typed = now_uptime();
        advance(5);
        held.ctx.notices.end_doubt(Uptime::from_nanos(now_uptime()));
        advance(20);
        key_with_os_time(&held.ctx, typed);
        ticks(&held.ctx, 220);
        doubt_from_now(&held.ctx);
        ticks(&held.ctx, 100);
        assert_eq!(
            keystrokes(&held.writer),
            0,
            "typed 5 ms before the front read that names another program, read while that program is asked"
        );
    }

    #[test]
    fn a_key_read_during_the_first_try_before_the_doubt_stands_is_never_released() {
        let (held, _drops) = seam_held(1_000);
        key_down(&held.ctx);
        ticks(&held.ctx, 240);
        assert_eq!(held_now(&held.ctx), 1, "held while the try waits");
        doubt_from_now(&held.ctx);
        ticks(&held.ctx, 50);
        held.ctx.notices.end_doubt(Uptime::from_nanos(now_uptime()));
        ticks(&held.ctx, 1_100);
        assert_eq!(
            keystrokes(&held.writer),
            0,
            "the Unanswered mark, taken after the try returns, drops it"
        );
    }

    #[test]
    fn a_modifier_press_under_a_doubt_is_never_released_posted_or_not() {
        for (what, code, flags) in [
            ("a posted caps lock press", 0x39, 0x1_0000),
            (
                "a shift press with its device bit",
                LEFT_SHIFT,
                0x2_0000 | LEFT_SHIFT_BIT,
            ),
        ] {
            let (held, _drops) = seam_held(100);
            held.ctx.notices.set_doubt(true);
            advance(1);
            on_tap_event(&held.ctx, with_key(CGEventType::FlagsChanged, code, flags));
            held.ctx.notices.set_doubt(false);
            ticks(&held.ctx, 300);
            assert_eq!(keystrokes(&held.writer), 0, "{what}");
        }
    }

    #[test]
    fn a_key_under_a_doubt_is_never_released_whatever_the_mark_does_when_the_doubt_ends() {
        for end in [
            "no new mark",
            "a Start mark before its next reading",
            "a mark after its next reading",
        ] {
            let (held, _drops) = seam_held(100);
            held.ctx.notices.set_doubt(true);
            key_down(&held.ctx);
            match end {
                "no new mark" => held.ctx.notices.set_doubt(false),
                "a Start mark before its next reading" => {
                    held.ctx
                        .notices
                        .mark(Uptime::from_nanos(now_uptime()), MarkKind::Start);
                    held.ctx.notices.set_doubt(false);
                }
                _ => {
                    held.ctx.notices.set_doubt(false);
                    tick(&held.ctx);
                    advance(20);
                    held.ctx
                        .notices
                        .mark(Uptime::from_nanos(now_uptime()), MarkKind::Start);
                }
            }
            ticks(&held.ctx, 300);
            assert_eq!(keystrokes(&held.writer), 0, "{end}");
        }
    }

    #[test]
    fn a_key_typed_after_a_doubt_ends_is_released_at_the_bound() {
        let (held, _drops) = seam_held(100);
        doubt_from_now(&held.ctx);
        tick(&held.ctx);
        held.ctx.notices.end_doubt(Uptime::from_nanos(now_uptime()));
        advance(1);
        key_down(&held.ctx);
        ticks(&held.ctx, 300);
        assert_eq!(keystrokes(&held.writer), 1);
    }

    #[test]
    fn a_notice_drop_reads_a_gap_by_the_after_stamp_of_its_reading_as_with_no_mark() {
        let mut outcomes = Vec::new();
        for with_mark in [false, true] {
            let held = slow_held(1_000);
            key_down(&held.ctx);
            advance(15);
            SLOW_READ_NS.with(|ns| ns.set(20 * MS));
            if with_mark {
                mark_now(&held.ctx);
            }
            on_reading_tick(&held.ctx);
            SLOW_READ_NS.with(|ns| ns.set(0));
            let outcome = outcome_of(&held);
            ticks(&held.ctx, 2_000);
            outcomes.push((outcome, keystrokes(&held.writer)));
        }
        assert_eq!(
            outcomes[0], outcomes[1],
            "a reading that began in time and ended past the gap bound"
        );
        assert!(outcomes[1].0 .2, "the gap stalls the source");
    }

    #[test]
    fn a_late_key_up_whose_reading_finds_a_new_mark_or_a_doubt_leaves_the_stalled_flag_unset() {
        let mut stalled = Vec::new();
        for under in ["no mark", "a new mark", "a standing doubt"] {
            let held = held(1_000);
            key_down(&held.ctx);
            advance(1);
            match under {
                "a new mark" => mark_now(&held.ctx),
                "a standing doubt" => held.ctx.notices.set_doubt(true),
                _ => {}
            }
            on_tap_event(
                &held.ctx,
                timed(
                    CGEventType::KeyUp,
                    Timing::Late {
                        lag: Duration::from_millis(40),
                    },
                ),
            );
            assert_eq!(held_now(&held.ctx), 0, "{under}: the held key is dropped");
            stalled.push(held.counters.stalled());
        }
        assert_eq!(
            stalled,
            [true, false, false],
            "the late key-up stalls the source with no mark alone"
        );
    }

    #[test]
    fn a_standing_doubt_drops_the_held_keys_at_every_reading_until_it_ends() {
        let (held, drops) = seam_held(100);
        held.ctx.notices.set_doubt(true);
        for _ in 0..3 {
            key_down(&held.ctx);
            tick(&held.ctx);
            assert_eq!(held_now(&held.ctx), 0, "each reading drops what was held");
        }
        ticks(&held.ctx, 300);
        assert_eq!(keystrokes(&held.writer), 0);
        assert!(held.counters.figures().keys_withheld);
        assert!(!held.counters.stalled(), "a doubt is no timing fault");
        assert_eq!(
            drops.withheld_by_mark().1,
            3,
            "counted as withheld by doubt"
        );
        held.ctx.notices.set_doubt(false);
        key_down(&held.ctx);
        ticks(&held.ctx, 300);
        assert_eq!(
            keystrokes(&held.writer),
            1,
            "after the doubt a key is released at the bound"
        );
    }

    #[test]
    fn the_notice_mark_is_read_on_the_clock_of_an_events_os_time() {
        reset_stand_ins();
        MONOTONIC.with(|now| now.set(START + 10_000 * MS));
        let settings = stand_in_settings();
        let clock = notice_clock(&settings);
        assert_eq!(
            clock(),
            Uptime::from_nanos(START),
            "the uptime side, 10 s from the stamps"
        );
        assert_eq!(
            (settings.clocks.stamps)(),
            Monotonic::from_nanos(START + 10_000 * MS)
        );
    }

    #[test]
    fn only_the_systems_tap_maker_watches_the_focus() {
        fn watches<M: TapMaker>() -> bool {
            M::WATCHES_FOCUS
        }
        assert!(
            watches::<SystemTapMaker>(),
            "the product's maker starts the notice thread"
        );
    }

    #[test]
    fn status_never_sees_the_drop_counts_of_the_seam() {
        let run = |seam: bool| {
            let mut held = held(100);
            let drops = DropCounts::new();
            if seam {
                held.ctx.drops = Some(drops.clone());
            }
            key_down(&held.ctx);
            key_down(&held.ctx);
            answer(Reading::On);
            tick(&held.ctx);
            answer(Reading::Off);
            key_down(&held.ctx);
            advance(100);
            tick(&held.ctx);
            key_down(&held.ctx);
            mark_now(&held.ctx);
            tick(&held.ctx);
            key_down(&held.ctx);
            ticks(&held.ctx, 300);
            (held.counters.figures(), drops.by_cause())
        };
        let (with_the_seam, counted) = run(true);
        let (without_it, _) = run(false);
        assert!(
            counted.iter().filter(|&&(_, count)| count > 0).count() >= 3,
            "the seam counted drops of three causes: {counted:?}"
        );
        assert_eq!(with_the_seam, without_it);
    }

    #[test]
    fn the_seam_counts_a_notice_drop_by_its_cause_and_enters_it() {
        let (held, drops) = seam_held(100);
        key_down(&held.ctx);
        key_down(&held.ctx);
        mark_now(&held.ctx);
        tick(&held.ctx);
        assert_eq!(drops.of("notice"), 1);
        assert!(drops.take_entries().iter().any(|entry| matches!(
            entry,
            RawEntry::Dropped {
                cause: Cause::Notice,
                keys: 2,
                ..
            }
        )));
    }

    fn order_monotonic() -> Monotonic {
        CALLS.with(|calls| calls.borrow_mut().push("monotonic"));
        stand_in_monotonic()
    }

    fn order_uptime() -> Uptime {
        CALLS.with(|calls| calls.borrow_mut().push("uptime"));
        stand_in_uptime()
    }

    fn order_session() -> Session {
        CALLS.with(|calls| calls.borrow_mut().push("session"));
        stand_in_session()
    }

    fn order_reader(session: Session) -> Reading {
        CALLS.with(|calls| calls.borrow_mut().push("read"));
        stand_in_reader(session)
    }

    #[test]
    fn a_key_events_stamps_come_in_their_order() {
        let mut held = held(100);
        held.ctx.clocks = Clocks {
            stamps: order_monotonic,
            os_time: order_uptime,
        };
        held.ctx.session = order_session;
        held.ctx.secure = order_reader;
        stall(300 * MS);
        let event = event_object(CGEventType::KeyDown, 0);
        set_stamp(&event, now_uptime());
        CALLS.with(|calls| calls.borrow_mut().clear());
        let tap = tap_event(&held.ctx, CGEventType::KeyDown, &event);
        on_tap_event(&held.ctx, tap);
        let calls = CALLS.with(|calls| calls.borrow().clone());
        assert_eq!(
            calls.get(..11),
            Some(
                &[
                    "monotonic",
                    "uptime",
                    "monotonic",
                    "session",
                    "monotonic",
                    "monotonic",
                    "read",
                    "monotonic",
                    "uptime",
                    "monotonic",
                    "uptime"
                ][..]
            ),
            "the entry's monotonic and uptime stamps; the session's age, read and stamp; the \
             before stamp, the read and the after stamp; the pair of clocks; got {calls:?}"
        );
    }

    #[test]
    fn the_source_holds_a_key_down_and_a_modifier_press_and_nothing_else_of_the_keys() {
        let shift = CGEventFlags::CGEventFlagShift.bits();
        let command = CGEventFlags::CGEventFlagCommand.bits();
        let left_command = 0x8;
        let flags = CGEventType::FlagsChanged;
        assert_eq!(
            kind_of(CGEventType::KeyDown, Some(0), 0, false),
            Kind::Key(KeyClass::Printable)
        );
        assert_eq!(
            kind_of(CGEventType::KeyUp, Some(0), 0, false),
            Kind::Reading
        );
        assert_eq!(
            kind_of(flags, None, shift | LEFT_SHIFT_BIT, false),
            Kind::Reading,
            "a flags change whose key code was not read"
        );
        let rows = [
            (
                LEFT_SHIFT,
                shift | LEFT_SHIFT_BIT,
                Kind::Key(KeyClass::Other),
                "a shift press",
            ),
            (
                LEFT_COMMAND,
                command | left_command | shift | LEFT_SHIFT_BIT,
                Kind::Key(KeyClass::Other),
                "a command press while shift is down",
            ),
            (LEFT_SHIFT, 0, Kind::Reading, "a shift release"),
            (
                LEFT_COMMAND,
                shift | LEFT_SHIFT_BIT,
                Kind::Reading,
                "a command release while shift stays down",
            ),
            (
                RIGHT_SHIFT,
                shift | LEFT_SHIFT_BIT | RIGHT_SHIFT_BIT,
                Kind::Key(KeyClass::Other),
                "the right shift pressed while the left one is down",
            ),
            (
                LEFT_SHIFT,
                shift | RIGHT_SHIFT_BIT,
                Kind::Reading,
                "the left shift released while the right one stays down",
            ),
            (
                LEFT_SHIFT,
                shift,
                Kind::Reading,
                "a posted shift press, with no device bits, is a reading alone (ruling MA-30 M1)",
            ),
            (LEFT_SHIFT, 0x100, Kind::Reading, "a posted shift release"),
            (
                0x39,
                0x1_0000,
                Kind::Key(KeyClass::Other),
                "caps lock turned on",
            ),
            (
                0x39,
                0,
                Kind::Key(KeyClass::Other),
                "caps lock turned off is a press too",
            ),
            (0x3F, 0x80_0000, Kind::Key(KeyClass::Other), "a fn press"),
            (0x3F, 0, Kind::Reading, "a fn release"),
            (
                0,
                shift | LEFT_SHIFT_BIT,
                Kind::Reading,
                "a key code that is no modifier",
            ),
        ];
        for (key_code, bits, kind, what) in rows {
            assert_eq!(kind_of(flags, Some(key_code), bits, false), kind, "{what}");
        }
    }

    #[test]
    fn posted_shift_presses_and_a_release_while_the_other_stays_down_record_at_most_two() {
        let shift = 0x2_0000;
        let posted = [
            (LEFT_SHIFT, shift | 0x100),
            (RIGHT_SHIFT, shift | 0x100),
            (LEFT_SHIFT, shift | 0x100),
            (RIGHT_SHIFT, 0x100),
        ];
        for reading in [Reading::Off, Reading::On] {
            let held = held(100);
            answer(reading);
            for (code, flags) in posted {
                advance(1);
                on_tap_event(&held.ctx, with_key(CGEventType::FlagsChanged, code, flags));
            }
            answer(Reading::Off);
            ticks(&held.ctx, 300);
            let recorded = keystrokes(&held.writer);
            assert!(recorded <= 2, "{reading:?}: recorded {recorded}");
            if reading == Reading::On {
                assert_eq!(recorded, 0, "nothing under a reading of On");
            }
        }
        assert_eq!(
            kind_of(
                CGEventType::FlagsChanged,
                Some(LEFT_SHIFT),
                shift | 0x100,
                false
            ),
            Kind::Reading,
            "the left release while the right stays down is no press"
        );
    }

    #[test]
    fn a_posted_release_with_no_event_before_it_reads_no_press_with_its_family_bit_clear_or_set() {
        let shift = 0x2_0000;
        for flags in [0x100, shift | 0x100] {
            assert_eq!(
                kind_of(CGEventType::FlagsChanged, Some(LEFT_SHIFT), flags, false),
                Kind::Reading,
                "flags {flags:#x}"
            );
            let held = held(100);
            advance(1);
            on_tap_event(
                &held.ctx,
                with_key(CGEventType::FlagsChanged, LEFT_SHIFT, flags),
            );
            assert_eq!(held_now(&held.ctx), 0, "flags {flags:#x}");
        }
    }

    #[test]
    fn every_modifier_key_reads_its_press_and_release_by_its_own_bit() {
        let iokit: [(i64, u64, u64); 10] = [
            (0x38, 0x0000_0002, 0x0002_0000),
            (0x3C, 0x0000_0004, 0x0002_0000),
            (0x3B, 0x0000_0001, 0x0004_0000),
            (0x3E, 0x0000_2000, 0x0004_0000),
            (0x3A, 0x0000_0020, 0x0008_0000),
            (0x3D, 0x0000_0040, 0x0008_0000),
            (0x37, 0x0000_0008, 0x0010_0000),
            (0x36, 0x0000_0010, 0x0010_0000),
            (0x39, 0, 0x0001_0000),
            (0x3F, 0, 0x0080_0000),
        ];
        assert_eq!(
            MODIFIER_KEYS.map(|(code, _, _)| code),
            iokit.map(|(code, _, _)| code),
            "the ten keys of the table, in its order"
        );
        let flags = CGEventType::FlagsChanged;
        let mut checked = 0;
        for &(code, side, family) in iokit.iter().filter(|row| row.0 != CAPS_LOCK) {
            assert_eq!(
                kind_of(flags, Some(code), 0, false),
                Kind::Reading,
                "{code:#x} released"
            );
            if side == 0 {
                assert_eq!(
                    kind_of(flags, Some(code), family, false),
                    Kind::Key(KeyClass::Other),
                    "{code:#x} on by its family bit"
                );
                assert_eq!(
                    kind_of(flags, Some(code), 0x100, false),
                    Kind::Reading,
                    "{code:#x} off"
                );
            } else {
                assert_eq!(
                    kind_of(flags, Some(code), family | side, false),
                    Kind::Key(KeyClass::Other),
                    "{code:#x} pressed with its device bit"
                );
                let others = MODIFIER_SIDE_BITS & !side;
                assert_eq!(
                    kind_of(flags, Some(code), family | others, false),
                    Kind::Reading,
                    "{code:#x} released while the other device bits stay set"
                );
                assert_eq!(
                    kind_of(flags, Some(code), family | 0x100, false),
                    Kind::Reading,
                    "{code:#x} posted with its family bit alone"
                );
                assert_eq!(
                    kind_of(flags, Some(code), 0x100, false),
                    Kind::Reading,
                    "{code:#x} posted release"
                );
            }
            checked += 1;
        }
        assert_eq!(
            checked, 9,
            "the nine keys but caps lock, which its own test reads"
        );
    }

    #[test]
    fn a_modifier_release_with_no_event_before_it_reads_no_press() {
        let shift = CGEventFlags::CGEventFlagShift.bits();
        let held = held(100);
        advance(1);
        on_tap_event(&held.ctx, with_flags(CGEventType::KeyUp, 0));
        advance(1);
        on_tap_event(
            &held.ctx,
            with_key(
                CGEventType::FlagsChanged,
                LEFT_SHIFT,
                shift | RIGHT_SHIFT_BIT,
            ),
        );
        assert_eq!(
            held_now(&held.ctx),
            0,
            "the release of the left shift while the right one stays down, whose presses the tap never saw"
        );
        ticks(&held.ctx, 120);
        assert_eq!(keystrokes(&held.writer), 0);
    }

    #[test]
    fn button_presses_are_clicks_moves_drags_and_scrolls_fold_and_the_notices_are_no_event() {
        let rows = [
            (CGEventType::LeftMouseDown, Kind::Click(Button::Primary)),
            (CGEventType::RightMouseDown, Kind::Click(Button::Secondary)),
            (CGEventType::OtherMouseDown, Kind::Click(Button::Other)),
            (CGEventType::LeftMouseUp, Kind::ButtonUp),
            (CGEventType::RightMouseUp, Kind::ButtonUp),
            (CGEventType::OtherMouseUp, Kind::ButtonUp),
            (CGEventType::MouseMoved, Kind::Fold(FoldClass::Move)),
            (CGEventType::LeftMouseDragged, Kind::Fold(FoldClass::Drag)),
            (CGEventType::RightMouseDragged, Kind::Fold(FoldClass::Drag)),
            (CGEventType::OtherMouseDragged, Kind::Fold(FoldClass::Drag)),
            (CGEventType::ScrollWheel, Kind::Fold(FoldClass::Scroll)),
        ];
        for (etype, kind) in rows {
            assert_eq!(kind_of(etype, Some(0), 0, false), kind, "{etype:?}");
        }
        for etype in [
            CGEventType::TapDisabledByTimeout,
            CGEventType::TapDisabledByUserInput,
        ] {
            assert_eq!(kind_of(etype, Some(0), 0, false), Kind::Ignore);
            assert!(is_tap_disabled(etype));
        }
        assert!(!is_tap_disabled(CGEventType::KeyDown));
    }

    #[test]
    fn the_key_table_maps_a_fixed_list_of_codes_and_flags_to_their_class() {
        let shift = 0x2_0000;
        let control = 0x4_0000;
        let option = 0x8_0000;
        let command = 0x10_0000;
        let pad = 0x20_0000;
        let fn_bit = 0x80_0000;
        let rows: [(i64, u64, KeyClass, &str); 24] = [
            (0x00, 0, KeyClass::Printable, "a letter"),
            (0x2D, shift, KeyClass::Printable, "a letter with shift"),
            (0x12, 0, KeyClass::Printable, "a digit"),
            (0x31, 0, KeyClass::Printable, "space"),
            (0x2B, 0, KeyClass::Printable, "punctuation"),
            (0x52, pad, KeyClass::Printable, "a digit of the keypad"),
            (0x33, 0, KeyClass::Editing, "delete"),
            (0x75, fn_bit, KeyClass::Editing, "forward delete"),
            (
                0x72,
                fn_bit,
                KeyClass::Editing,
                "help, insert on a PC keyboard",
            ),
            (0x7B, pad | fn_bit, KeyClass::Navigation, "an arrow"),
            (0x7E, pad | fn_bit, KeyClass::Navigation, "the up arrow"),
            (0x73, fn_bit, KeyClass::Navigation, "home"),
            (0x77, fn_bit, KeyClass::Navigation, "end"),
            (0x74, fn_bit, KeyClass::Navigation, "page up"),
            (0x79, fn_bit, KeyClass::Navigation, "page down"),
            (0x24, 0, KeyClass::Enter, "return"),
            (0x4C, pad, KeyClass::Enter, "the enter key of the keypad"),
            (0x30, 0, KeyClass::Tab, "tab"),
            (0x35, 0, KeyClass::Other, "escape"),
            (0x7A, fn_bit, KeyClass::Other, "F1"),
            (0x00, command, KeyClass::Shortcut, "a letter with command"),
            (
                0x7B,
                control | pad | fn_bit,
                KeyClass::Shortcut,
                "an arrow with control",
            ),
            (0x00, option, KeyClass::Shortcut, "a letter with option"),
            (
                0x24,
                command | shift,
                KeyClass::Shortcut,
                "return with command and shift",
            ),
        ];
        for (code, flags, class, what) in rows {
            assert_eq!(
                kind_of(CGEventType::KeyDown, Some(code), flags, false),
                Kind::Key(class),
                "{what}"
            );
        }
        assert_eq!(
            kind_of(CGEventType::KeyDown, None, 0, false),
            Kind::Key(KeyClass::Other),
            "a key down whose key code was not read"
        );
        assert_eq!(
            kind_of(
                CGEventType::FlagsChanged,
                Some(LEFT_SHIFT),
                shift | LEFT_SHIFT_BIT,
                false
            ),
            Kind::Key(KeyClass::Other),
            "shift pressed alone"
        );
        assert_eq!(
            kind_of(
                CGEventType::FlagsChanged,
                Some(LEFT_COMMAND),
                command | 0x8 | control | 0x1,
                false
            ),
            Kind::Key(KeyClass::Other),
            "command pressed while control is held is a modifier's key down, not a shortcut"
        );
    }

    #[test]
    fn an_autorepeat_a_key_up_and_a_released_modifier_are_readings_alone() {
        let shift = 0x2_0000;
        assert_eq!(
            kind_of(CGEventType::KeyDown, Some(0x00), 0, true),
            Kind::Reading,
            "an automatic repeat"
        );
        assert_eq!(
            kind_of(CGEventType::KeyDown, Some(0x00), 0x10_0000, true),
            Kind::Reading,
            "an automatic repeat of a shortcut"
        );
        assert_eq!(
            kind_of(CGEventType::KeyUp, Some(0x00), 0, false),
            Kind::Reading
        );
        assert_eq!(
            kind_of(CGEventType::FlagsChanged, Some(LEFT_SHIFT), 0, false),
            Kind::Reading,
            "shift released"
        );
        assert_eq!(
            kind_of(
                CGEventType::FlagsChanged,
                Some(LEFT_SHIFT),
                shift | RIGHT_SHIFT_BIT,
                false
            ),
            Kind::Reading,
            "the left shift released while the right one stays down"
        );
    }

    #[test]
    fn caps_lock_is_a_press_each_time_that_it_toggles() {
        for flags in [0x1_0000, 0, 0x100] {
            assert_eq!(
                kind_of(CGEventType::FlagsChanged, Some(0x39), flags, false),
                Kind::Key(KeyClass::Other),
                "flags {flags:#x}"
            );
        }
    }

    #[derive(Default)]
    struct MacMapper {
        flags: u64,
        folds: Folds,
    }

    fn code_of(key: cases::Key) -> i64 {
        match key {
            cases::Key::Letter => 0x00,
            cases::Key::Digit => 0x12,
            cases::Key::Space => 0x31,
            cases::Key::Punctuation => 0x2B,
            cases::Key::Backspace => 0x33,
            cases::Key::Delete => 0x75,
            cases::Key::Insert => 0x72,
            cases::Key::Arrow => 0x7B,
            cases::Key::Home => 0x73,
            cases::Key::End => 0x77,
            cases::Key::PageUp => 0x74,
            cases::Key::PageDown => 0x79,
            cases::Key::Enter => 0x24,
            cases::Key::Tab => 0x30,
            cases::Key::Escape => 0x35,
            cases::Key::Function => 0x7A,
            cases::Key::Modifier(modifier) => modifier_row(modifier).0,
        }
    }

    fn modifier_row(modifier: cases::Modifier) -> (i64, u64, u64) {
        let code = match modifier {
            cases::Modifier::Shift => 0x38,
            cases::Modifier::Control => 0x3B,
            cases::Modifier::OptionOrAlt => 0x3A,
            cases::Modifier::CommandOrWindows => 0x37,
        };
        *MODIFIER_KEYS
            .iter()
            .find(|row| row.0 == code)
            .expect("a modifier of the table")
    }

    fn button_type(button: Button, down: bool) -> CGEventType {
        match (button, down) {
            (Button::Primary, true) => CGEventType::LeftMouseDown,
            (Button::Primary, false) => CGEventType::LeftMouseUp,
            (Button::Secondary, true) => CGEventType::RightMouseDown,
            (Button::Secondary, false) => CGEventType::RightMouseUp,
            (Button::Other, true) => CGEventType::OtherMouseDown,
            (Button::Other, false) => CGEventType::OtherMouseUp,
        }
    }

    impl cases::Mapper for MacMapper {
        fn input(&mut self, input: cases::Input) -> Vec<cases::Expected> {
            use cases::Input;
            let (etype, code, autorepeat) = match input {
                Input::KeyDown(cases::Key::Modifier(modifier)) => {
                    let (code, side, family) = modifier_row(modifier);
                    self.flags |= side | family;
                    (CGEventType::FlagsChanged, code, false)
                }
                Input::KeyUp(cases::Key::Modifier(modifier)) => {
                    let (code, side, family) = modifier_row(modifier);
                    self.flags &= !side;
                    if self.flags & MODIFIER_SIDE_BITS & !side == 0 {
                        self.flags &= !family;
                    }
                    (CGEventType::FlagsChanged, code, false)
                }
                Input::KeyDown(key) => (CGEventType::KeyDown, code_of(key), false),
                Input::KeyRepeat(key) => (CGEventType::KeyDown, code_of(key), true),
                Input::KeyUp(key) => (CGEventType::KeyUp, code_of(key), false),
                Input::ButtonDown(button) => (button_type(button, true), 0, false),
                Input::ButtonUp(button) => (button_type(button, false), 0, false),
                Input::Move => (CGEventType::MouseMoved, 0, false),
                Input::Drag => (CGEventType::LeftMouseDragged, 0, false),
                Input::Scroll => (CGEventType::ScrollWheel, 0, false),
                Input::IntervalEnd => {
                    return self
                        .folds
                        .take()
                        .into_iter()
                        .map(|fold| cases::Expected::Fold(fold.class(), fold.count().get()))
                        .collect();
                }
            };
            match kind_of(etype, Some(code), self.flags, autorepeat) {
                Kind::Key(class) => vec![cases::Expected::Key(class)],
                Kind::Click(button) => vec![cases::Expected::Click(button)],
                Kind::Fold(class) => {
                    self.folds.add(class);
                    Vec::new()
                }
                Kind::Reading | Kind::ButtonUp | Kind::Ignore => Vec::new(),
            }
        }
    }

    #[test]
    fn the_shared_list_of_inputs_gives_its_events_through_the_macos_mapper() {
        assert_eq!(cases::failing_cases(MacMapper::default), Vec::<&str>::new());
    }

    #[test]
    fn a_hundred_moves_fold_into_one_record_with_count_100() {
        let mut folds = Folds::default();
        for _ in 0..100 {
            match kind_of(CGEventType::MouseMoved, None, 0, false) {
                Kind::Fold(class) => folds.add(class),
                other => panic!("a move is {other:?}"),
            }
        }
        let taken = folds.take();
        assert_eq!(
            taken
                .iter()
                .map(|fold| (fold.class(), fold.count().get()))
                .collect::<Vec<_>>(),
            [(FoldClass::Move, 100)]
        );
        assert!(folds.take().is_empty(), "the interval after it is empty");
    }

    #[test]
    fn a_fold_handed_back_adds_its_count_to_the_next_interval() {
        let mut folds = Folds::default();
        folds.add(FoldClass::Scroll);
        let back = folds.take();
        folds.add(FoldClass::Scroll);
        for fold in back {
            folds.carry(fold);
        }
        assert_eq!(
            folds
                .take()
                .iter()
                .map(|fold| (fold.class(), fold.count().get()))
                .collect::<Vec<_>>(),
            [(FoldClass::Scroll, 2)]
        );
    }

    #[test]
    fn the_tap_sits_at_the_session_level() {
        assert_eq!(TAP_LOCATION as u32, CGEventTapLocation::Session as u32);
        assert_eq!(tap_settings().location as u32, 1);
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
        let mask = events_of_interest(crate::floor::Switches::default());
        for etype in EVERY_EVENT_TYPE {
            let masked = mask.iter().any(|&listed| listed as u32 == etype as u32);
            let handled = kind_of(etype, Some(0), 0, false) != Kind::Ignore;
            assert_eq!(
                masked, handled,
                "{etype:?}: masked {masked}, handled {handled}"
            );
        }
    }

    #[test]
    fn the_tap_mask_is_0x0e401cfe() {
        let mask = events_of_interest(crate::floor::Switches::default())
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
        on_tap_event(
            &held.ctx,
            with_key(CGEventType::FlagsChanged, LEFT_SHIFT, 0),
        );
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
        on_tap_event(
            &held.ctx,
            with_key(
                CGEventType::FlagsChanged,
                LEFT_SHIFT,
                shift | LEFT_SHIFT_BIT,
            ),
        );
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
            timed(
                CGEventType::KeyDown,
                Timing::Late {
                    lag: Duration::from_millis(31),
                },
            ),
        );
        assert_eq!(held_now(&held.ctx), 0);
        assert!(held.counters.stalled());
        assert_eq!(held.counters.figures().late_events, 0);
        advance(1);
        on_tap_event(
            &held.ctx,
            timed(
                CGEventType::KeyDown,
                Timing::Late {
                    lag: Duration::from_millis(251),
                },
            ),
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
        on_tap_event(&held.ctx, timed(CGEventType::KeyDown, Timing::Untimed));
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
                stop: held.ctx.stop.clone(),
                drops: held.ctx.drops.clone(),
                clock: held.ctx.clocks.stamps,
            }),
            phase: None,
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
        let asked_at_the_stop = asked();
        ticks(&held.ctx, 2_000);
        assert_eq!(
            asked(),
            asked_at_the_stop,
            "no tick after the stop takes a reading"
        );
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
        lock(&ctx.hold).offset = None;
        ctx.start(&mut lock(&ctx.hold));
        let writer = held.writer;
        let reader = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            let mut kinds = Vec::new();
            while let Ok(received) = writer.recv_timeout(Duration::from_millis(500)) {
                kinds.push(received.event().modality);
            }
            kinds
        });
        on_tap_event(&ctx, real_event(CGEventType::KeyDown));
        on_tap_event(&ctx, real_event(CGEventType::KeyDown));
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
        let before_arrival = std::time::SystemTime::now();
        advance(1);
        on_tap_event(
            &held.ctx,
            timed(
                CGEventType::KeyDown,
                Timing::InTime {
                    lag: Duration::from_millis(20),
                },
            ),
        );
        held.fg_writer
            .set_for_test(Some(crate::macos::foreground::tests_support::front(
                "com.example.other",
                42,
            )));
        advance(1);
        on_tap_event(&held.ctx, event(CGEventType::LeftMouseDown));
        ticks(&held.ctx, 1_010);
        let all = received(&held.writer);
        assert_eq!(all.len(), 2);
        assert!(
            all[0].number() < all[1].number(),
            "the key takes its number at its release"
        );
        let floor: Vec<_> = all
            .into_iter()
            .filter_map(|received| received.into_floor().ok())
            .collect();
        let (pointer, key) = (floor[0].event(), floor[1].event());
        assert_eq!(
            (pointer.kind(), pointer.pid()),
            (Modality::Pointer, ProcessId::new(42)),
            "the click's app"
        );
        assert_eq!(
            (key.kind(), key.pid()),
            (Modality::Keystroke, ProcessId::new(41)),
            "the app of the key's arrival"
        );
        assert!(
            key.at() <= crate::event::unix_nanos(before_arrival - Duration::from_millis(19)),
            "the key's instant is its own time, its arrival less its lag"
        );
    }

    #[test]
    fn a_held_key_holds_its_class_and_app_alone() {
        struct Mirror {
            _class: KeyClass,
            _held_phase: bool,
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
        let slot = KeyQueue::<HeldKey>::slot_bytes();
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
        READ_TIMES.with(|times| times.borrow_mut().push(monotonic_raw().nanos()));
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
        const SECONDS: u64 = 20;
        const NOTICE_SPACING_US: u64 = 50_000;
        const NOTICE_READINGS: usize = 300;
        let session = read_session();
        let read_ns = per_call_ns(10_000, || {
            let _ = secure_input_reading(Session::AtConsole);
        });
        let session_ns = per_call_ns(1_000, || {
            let _ = read_session();
        });
        let mut loads = [0f64; 3];
        // SAFETY: `loads` is a writable array of three doubles, the count passed.
        let _ = unsafe { libc::getloadavg(loads.as_mut_ptr(), 3) };
        let mut parent = [0u8; 256];
        // SAFETY: `parent` is writable for the size passed; proc_name writes at
        // most that many bytes of the parent's name.
        let named = unsafe {
            libc::proc_name(
                libc::getppid(),
                parent.as_mut_ptr().cast(),
                parent.len() as u32,
            )
        };
        let started_by: String = parent[..usize::try_from(named.max(0)).unwrap_or(0)]
            .iter()
            .map(|&byte| byte as char)
            .filter(char::is_ascii_alphanumeric)
            .collect();
        let (handle, writer) = sink::channel(4_096);
        let (_fg_writer, reader) = foreground_state_for_test();
        let counters = InputCounters::new();
        let settings = TapSettings {
            secure: timed_reader,
            ..tap_settings()
        };
        let ctx = Box::new(tap_context(
            handle,
            reader,
            counters.clone(),
            &settings,
            Bounds {
                hold: Duration::from_secs(3_600),
                ..Bounds::PRODUCT
            },
            Arc::new(AtomicBool::new(false)),
        ));
        ctx.start(&mut lock(&ctx.hold));
        let key = || event_now(CGEventType::KeyDown, 0);
        READ_TIMES.with(|times| times.borrow_mut().clear());
        let mut spacing = Vec::new();
        let mut drops = 0u64;
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(SECONDS) {
            if held_now(&ctx) == 0 {
                READ_TIMES.with(|times| times.borrow_mut().clear());
                on_tap_event(&ctx, key());
            }
            // SAFETY: `kCFRunLoopDefaultMode` is a constant CFString that
            // CoreFoundation exports and never writes.
            CFRunLoop::run_in_mode(
                unsafe { kCFRunLoopDefaultMode },
                Duration::from_millis(250),
                false,
            );
            let times = READ_TIMES.with(|times| std::mem::take(&mut *times.borrow_mut()));
            spacing.extend(times.windows(2).map(|two| two[1] - two[0]));
            if held_now(&ctx) == 0 {
                drops += 1;
            } else if let Some(&last) = times.last() {
                READ_TIMES.with(|times| times.borrow_mut().push(last));
            }
        }
        let seconds = started.elapsed().as_secs_f64();
        drop(ctx);
        drop(writer);
        let over = |ms: u64| spacing.iter().filter(|&&ns| ns > ms * MS).count();
        let (over_30, over_50, over_100, over_250) = (over(30), over(50), over(100), over(250));
        spacing.sort_unstable();
        let at = |share: f64| {
            spacing
                .get(((spacing.len() as f64 - 1.0) * share).round() as usize)
                .map_or(0, |ns| ns / 1_000)
        };
        let readings = spacing.len() + 1;
        let readings_by_the_step = (seconds / READING_STEP.as_secs_f64()) as u64;
        let p50_us = at(0.5);
        println!(
            "measure dictionary={} on_console={} seconds={seconds:.1} load1={:.2} \
             started_by={started_by} read_ns={read_ns} session_ns={session_ns} readings={} \
             readings_by_the_step={readings_by_the_step} spacing_p50_us={} spacing_p99_us={} spacing_max_us={} gaps_over_30ms={} \
             gaps_over_50ms={} gaps_over_100ms={} gaps_over_250ms={} drops={drops} \
             stalled_at_end={}",
            u8::from(session != Session::NoDictionary),
            u8::from(session == Session::AtConsole),
            loads[0],
            readings,
            p50_us,
            at(0.99),
            spacing.last().map_or(0, |ns| ns / 1_000),
            over_30,
            over_50,
            over_100,
            over_250,
            u8::from(counters.stalled())
        );
        if p50_us > NOTICE_SPACING_US || readings < NOTICE_READINGS {
            println!(
                "measure notice for the coordinator: spacing_p50_us={p50_us} readings={readings} \
                 (a median above {NOTICE_SPACING_US} us or fewer than {NOTICE_READINGS} readings, ruling MA-22 L4)"
            );
        }
        let widest_us = spacing.last().map_or(0, |ns| ns / 1_000);
        let decided = gap_of_the_test(Measured {
            spacing_max_us: widest_us,
            readings,
            dictionary: session != Session::NoDictionary,
            on_console: session == Session::AtConsole,
        });
        println!(
            "{}",
            gap_line(
                &decided,
                "measured",
                &widest_us.to_string(),
                &format!("{:.2}", loads[0])
            )
        );
    }

    /// The fewest readings in the measurement's window from which the gap of the
    /// hosted phases is taken (ruling MA-23 Addition 1 (4)).
    const GAP_READINGS_AT_LEAST: usize = 300;
    /// The gap of the hosted phases is this many times the widest spacing of the
    /// readings, rounded up to GAP_STEP_MS.
    const GAP_FACTOR: u64 = 2;
    const GAP_STEP_MS: u64 = 10;
    /// The gap of the hosted phases is never under the product's gap bound.
    const GAP_FLOOR_MS: u64 = 30;
    /// The widest gap for which the hosted proof is stated; no phase runs at a
    /// wider one.
    const GAP_CAP_MS: u64 = 500;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct Measured {
        spacing_max_us: u64,
        readings: usize,
        dictionary: bool,
        on_console: bool,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum GapOfTheTest {
        Gap { ms: u64 },
        AboveTheCap { ms: u64 },
        Refused { reason: String },
    }

    fn bounded(ms: u64) -> GapOfTheTest {
        let ms = ms.max(GAP_FLOOR_MS);
        if ms > GAP_CAP_MS {
            GapOfTheTest::AboveTheCap { ms }
        } else {
            GapOfTheTest::Gap { ms }
        }
    }

    fn gap_of_the_test(measured: Measured) -> GapOfTheTest {
        let refused = |reason: String| GapOfTheTest::Refused { reason };
        if !measured.dictionary {
            return refused(String::from("the session has no dictionary"));
        }
        if !measured.on_console {
            return refused(String::from("the session is not at the console"));
        }
        if measured.readings < GAP_READINGS_AT_LEAST {
            return refused(format!(
                "{} readings, fewer than {GAP_READINGS_AT_LEAST}",
                measured.readings
            ));
        }
        if measured.spacing_max_us == 0 {
            return refused(String::from("a widest spacing of 0 us"));
        }
        let step_us = GAP_STEP_MS * 1_000;
        bounded((GAP_FACTOR * measured.spacing_max_us).div_ceil(step_us) * GAP_STEP_MS)
    }

    fn gap_of_the_variable(text: &str) -> GapOfTheTest {
        match text.parse::<u64>() {
            Err(_) => GapOfTheTest::Refused {
                reason: format!("HALVO_STALL_GAP_MS is a count of milliseconds, not {text}"),
            },
            Ok(ms) => match bounded(ms) {
                GapOfTheTest::AboveTheCap { ms } => GapOfTheTest::Refused {
                    reason: format!(
                        "HALVO_STALL_GAP_MS of {ms} ms is above the cap of {GAP_CAP_MS} ms"
                    ),
                },
                decided => decided,
            },
        }
    }

    fn gap_line(decided: &GapOfTheTest, source: &str, spacing: &str, load1: &str) -> String {
        match decided {
            GapOfTheTest::Gap { ms } => format!(
                "gap_of_the_test gap_ms={ms} source={source} spacing_max_us={spacing} load1={load1}"
            ),
            GapOfTheTest::AboveTheCap { ms } => format!(
                "gap_of_the_test above_the_cap gap_ms={ms} cap_ms={GAP_CAP_MS} source={source} \
                 spacing_max_us={spacing} load1={load1}"
            ),
            GapOfTheTest::Refused { reason } => {
                format!("gap_of_the_test refused source={source} reason={reason}")
            }
        }
    }

    #[test]
    #[ignore = "the gap of the hosted phases from HALVO_STALL_GAP_MS, for 51-macos-stall"]
    fn the_gap_of_the_test_from_its_variable() {
        let text = std::env::var("HALVO_STALL_GAP_MS").unwrap_or_default();
        println!(
            "{}",
            gap_line(&gap_of_the_variable(&text), "HALVO_STALL_GAP_MS", "-", "-")
        );
    }

    #[test]
    fn the_gap_of_the_test_is_twice_the_widest_spacing_rounded_up_between_its_floor_and_cap() {
        let measured = |spacing_max_us: u64| Measured {
            spacing_max_us,
            readings: 300,
            dictionary: true,
            on_console: true,
        };
        let gap = |ms: u64| GapOfTheTest::Gap { ms };
        let rows: [(&str, Measured, GapOfTheTest); 10] = [
            ("twice 90,000 us is 180 ms", measured(90_000), gap(180)),
            (
                "twice 90,001 us rounds up to 190 ms",
                measured(90_001),
                gap(190),
            ),
            (
                "twice 90,474 us rounds up to 190 ms",
                measured(90_474),
                gap(190),
            ),
            ("twice 1 us is the floor of 30 ms", measured(1), gap(30)),
            ("twice 14,999 us is 30 ms", measured(14_999), gap(30)),
            (
                "twice 250,000 us is the cap of 500 ms",
                measured(250_000),
                gap(500),
            ),
            (
                "twice 250,001 us is above the cap",
                measured(250_001),
                GapOfTheTest::AboveTheCap { ms: 510 },
            ),
            (
                "299 readings are too few",
                Measured {
                    readings: 299,
                    ..measured(90_000)
                },
                GapOfTheTest::Refused {
                    reason: String::from("299 readings, fewer than 300"),
                },
            ),
            (
                "a widest spacing of 0 is refused",
                measured(0),
                GapOfTheTest::Refused {
                    reason: String::from("a widest spacing of 0 us"),
                },
            ),
            (
                "a session with no dictionary is refused",
                Measured {
                    dictionary: false,
                    ..measured(90_000)
                },
                GapOfTheTest::Refused {
                    reason: String::from("the session has no dictionary"),
                },
            ),
        ];
        for (what, measured, want) in rows {
            assert_eq!(gap_of_the_test(measured), want, "{what}");
        }
        assert!(
            matches!(
                gap_of_the_test(Measured {
                    on_console: false,
                    ..measured(90_000)
                }),
                GapOfTheTest::Refused { .. }
            ),
            "a session not at the console is refused"
        );
        assert_eq!(gap_of_the_variable("200"), gap(200));
        assert_eq!(
            gap_of_the_variable("10"),
            gap(30),
            "the floor holds for the variable too"
        );
        assert_eq!(gap_of_the_variable("500"), gap(500));
        assert!(
            matches!(gap_of_the_variable("501"), GapOfTheTest::Refused { .. }),
            "the variable above the cap is refused"
        );
        assert!(matches!(
            gap_of_the_variable("2x"),
            GapOfTheTest::Refused { .. }
        ));
    }

    #[test]
    fn a_key_is_judged_late_by_the_lag_bound_and_not_by_the_gap_bound() {
        let held = held(1_000);
        lock(&held.ctx.hold).keys = KeyQueue::new(Bounds {
            hold: Duration::from_millis(1_000),
            gap: Duration::from_millis(60),
            lag: Duration::from_millis(30),
            cap: 64,
        });
        let callback = tap_callback(*held.ctx);
        let event = event_object(CGEventType::KeyDown, 0);
        set_stamp(&event, START - 31 * MS);
        let _ = callback(std::ptr::null(), CGEventType::KeyDown, &event);
        assert!(
            held.counters.figures().keys_withheld,
            "a key 31 ms late is late under a lag bound of 30 ms, whatever the gap bound"
        );
    }

    fn seam_held(bound_ms: u64) -> (Held, DropCounts) {
        let mut held = held(bound_ms);
        let drops = DropCounts::new();
        held.ctx.drops = Some(drops.clone());
        (held, drops)
    }

    #[test]
    fn the_raw_entries_hold_each_reading_and_each_keys_arrival_and_fate() {
        let (held, drops) = seam_held(100);
        key_down(&held.ctx);
        key_down(&held.ctx);
        ticks(&held.ctx, 100);
        key_down(&held.ctx);
        answer(Reading::On);
        tick(&held.ctx);
        key_down(&held.ctx);
        let ms = |n: u64| START + n * MS;
        let reading = |at: u64, value: Reading, caller: Caller, held: usize| RawEntry::Reading {
            before: ms(at),
            after: ms(at),
            value,
            caller,
            held,
        };
        let mut want = vec![
            reading(1, Reading::Off, Caller::Event, 0),
            RawEntry::Arrival {
                at: ms(1),
                admission: Arrived::Held,
            },
            reading(2, Reading::Off, Caller::Event, 1),
            RawEntry::Arrival {
                at: ms(2),
                admission: Arrived::Held,
            },
        ];
        for tick_number in 1..=9 {
            want.push(reading(2 + 10 * tick_number, Reading::Off, Caller::Tick, 2));
        }
        want.extend([
            reading(102, Reading::Off, Caller::Tick, 2),
            RawEntry::Released { at: ms(102) },
            reading(102, Reading::Off, Caller::ReleaseLoop, 1),
            RawEntry::Released { at: ms(102) },
            reading(103, Reading::Off, Caller::Event, 0),
            RawEntry::Arrival {
                at: ms(103),
                admission: Arrived::Held,
            },
            reading(113, Reading::On, Caller::Tick, 1),
            RawEntry::Dropped {
                at: ms(113),
                cause: Cause::On,
                keys: 1,
                unread: false,
            },
            reading(114, Reading::On, Caller::Event, 0),
            RawEntry::Arrival {
                at: ms(114),
                admission: Arrived::Suppressed,
            },
        ]);
        assert_eq!(
            drops.take_entries(),
            want,
            "each reading with its two stamps, value, caller and held count, and each key's arrival, \
             release and drop with its stamp and cause"
        );
        assert!(drops.take_entries().is_empty(), "a take empties the log");
        assert_eq!(drops.lost(), 0);
    }

    #[test]
    fn the_counts_of_the_drops_change_no_verdict() {
        let run = |counts: Option<DropCounts>| {
            let mut held = held(100);
            held.ctx.drops.clone_from(&counts);
            ANSWERS.with(|answers| {
                *answers.borrow_mut() = (0..64)
                    .map(|n| {
                        if n % 7 == 3 {
                            Reading::On
                        } else {
                            Reading::Off
                        }
                    })
                    .collect();
            });
            for n in 0..40 {
                if n % 3 == 0 {
                    key_down(&held.ctx);
                } else {
                    tick(&held.ctx);
                }
            }
            ticks(&held.ctx, 300);
            let readings = asked();
            (keystrokes(&held.writer), held.counters.figures(), readings)
        };
        let (plain_records, plain_figures, plain_readings) = run(None);
        let counted = DropCounts::new();
        let (counted_records, counted_figures, counted_readings) = run(Some(counted.clone()));
        let full = DropCounts::with_capacity(4);
        let (full_records, full_figures, full_readings) = run(Some(full.clone()));
        assert_eq!(
            (&plain_records, &plain_figures, plain_readings),
            (&counted_records, &counted_figures, counted_readings),
            "the counts and their log change no verdict"
        );
        assert_eq!(
            (&plain_records, &plain_figures, plain_readings),
            (&full_records, &full_figures, full_readings),
            "a full log changes no verdict"
        );
        assert!(counted.of("on") > 0, "the counts saw the drops for on");
        let entries = counted.take_entries();
        let logged_readings = entries
            .iter()
            .filter(|entry| matches!(entry, RawEntry::Reading { .. }))
            .count();
        assert_eq!(
            (logged_readings, counted.lost()),
            (counted_readings, 0),
            "the log holds one entry for each reading of the state"
        );
        assert!(
            entries.iter().any(|entry| matches!(
                entry,
                RawEntry::Dropped {
                    cause: Cause::On,
                    ..
                }
            )),
            "the log holds the drops for on"
        );
        assert_eq!(full.take_entries().len(), 4, "a full log keeps its cap");
        assert!(full.lost() > 0, "a full log counts what it dropped");
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
        AUTO_STEP.with(|step| step.set(50_000));
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
        let is_pointer = matches!(kind_of(etype, None, 0, false), Kind::Click(_));
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
        if !hosted_or_refuse("the_production_reader_reads_a_real_hold_as_on") {
            return;
        }
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
        if !hosted_or_refuse("the_production_reader_reads_off_on_during_a_hold_and_off_again") {
            return;
        }
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
        if !hosted_or_refuse(
            "a_new_source_with_a_hold_plan_holds_secure_input_through_the_system_while_it_runs",
        ) {
            return;
        }
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
            source.shutdown().ok();
            assert!(
                skip_allowed(std::env::var_os(HOLD_REQUIRED).as_deref()),
                "a_new_source_with_a_hold_plan_holds_secure_input_through_the_system_while_it_runs: \
                 the system refused the hold with status {}, and {HOLD_REQUIRED}=1 asks that every \
                 hold test holds",
                refused.0
            );
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
        let (reached_tx, reached_rx) = channel::<()>();
        let (ended_tx, _ended_rx) = channel();
        source
            .begin_with(handle, FakeMaker(ended_tx), move || {
                reached_tx
                    .send(())
                    .expect("the test waits for the tap thread");
                blocked.emit_event(FloorEvent::click(
                    crate::floor::cases::keep(crate::event::AppIdentity::unknown(), 1),
                    Button::Primary,
                    None,
                    EventClock::system(),
                ));
            })
            .expect("the source starts with a stand-in tap");
        reached_rx
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
        set_stamp(&event, uptime_raw().nanos());
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
        if !hosted_or_refuse(
            "a_started_source_counts_a_key_under_a_real_hold_through_the_callback_that_it_installs",
        ) {
            return;
        }
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
        set_stamp(&event, uptime_raw().nanos());
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
            settings.clocks.stamps,
            monotonic_raw as fn() -> Monotonic
        ));
        assert!(std::ptr::fn_addr_eq(
            settings.clocks.os_time,
            uptime_raw as fn() -> Uptime
        ));
    }

    #[test]
    fn the_clocks_are_the_raw_monotonic_and_uptime_clocks() {
        // SAFETY: as in `monotonic_raw`.
        let monotonic = unsafe { clock_gettime_nsec_np(libc::CLOCK_MONOTONIC_RAW) };
        let read = monotonic_raw().nanos();
        // SAFETY: as in `monotonic_raw`.
        let monotonic_after = unsafe { clock_gettime_nsec_np(libc::CLOCK_MONOTONIC_RAW) };
        assert!((monotonic..=monotonic_after).contains(&read));
        // SAFETY: as in `monotonic_raw`.
        let uptime = unsafe { clock_gettime_nsec_np(libc::CLOCK_UPTIME_RAW) };
        let read = uptime_raw().nanos();
        // SAFETY: as in `monotonic_raw`.
        let uptime_after = unsafe { clock_gettime_nsec_np(libc::CLOCK_UPTIME_RAW) };
        assert!((uptime..=uptime_after).contains(&read));
    }

    #[test]
    #[cfg(debug_assertions)]
    fn the_product_constructors_read_the_session_of_the_system() {
        let source = source_with(HoldPlan::Never);
        assert_eq!(source.step, READING_STEP);
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
        source
            .begin_with(handle, KeepingMaker(kept.clone()), || {})
            .ok();
        let callback = kept
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
            .expect("the maker was given a callback");
        let event = event_object(CGEventType::KeyDown, 0);
        set_stamp(&event, uptime_raw().nanos());
        let _ = callback(std::ptr::null(), CGEventType::KeyDown, &event);
        assert!(
            counters.figures().keys_withheld,
            "a key under a session with no dictionary is withheld"
        );
    }

    #[test]
    #[cfg(debug_assertions)]
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
    fn the_tap_is_requested_listen_only_at_the_session_location() {
        let settings = tap_settings();
        assert_eq!(
            (
                settings.options as u32,
                settings.location as u32,
                settings.placement as u32
            ),
            (1, 1, 0)
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

    static GATE_LOG: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

    extern "C" fn slow_enable(_tap: *mut c_void, _enable: bool) {
        GATE_LOG
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push("call entered");
        std::thread::sleep(Duration::from_millis(200));
        GATE_LOG
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push("call left");
    }

    #[test]
    fn the_port_disable_waits_for_a_store_that_holds_the_gate() {
        let port = TapPort::with_enable(slow_enable);
        port.store(std::ptr::NonNull::<c_void>::dangling().as_ptr());
        let disabling = {
            let port = port.clone();
            std::thread::spawn(move || port.disable())
        };
        let started = Instant::now();
        while !GATE_LOG
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(&"call entered")
        {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "the disable made no call"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        port.store(std::ptr::null_mut());
        GATE_LOG
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push("port cleared");
        disabling.join().expect("the disabling thread ends");
        let log = GATE_LOG
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        assert_eq!(log, ["call entered", "call left", "port cleared"]);
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
            phase: None,
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
            released: Arc::new(Mutex::new(None)),
            own_take: None,
            phase: None,
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
                TapPort::unset(),
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
            TapPort::unset(),
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
    fn a_shutdown_that_answered_timeout_reads_failed_with_reason_timeout_and_its_drop_waits_nothing(
    ) {
        let (handle, _writer) = sink::channel(8);
        let mut source = source_with(HoldPlan::Never);
        let (ended_tx, _ended_rx) = channel();
        source
            .begin_with(handle.clone(), FakeMaker(ended_tx), || {
                std::thread::sleep(Duration::from_millis(3000))
            })
            .expect("the source starts with a fake tap");
        let called = Instant::now();
        let first = source.shutdown();
        let took = called.elapsed();
        let state = source.health().state;
        let opened = Arc::new(AtomicUsize::new(0));
        let again = source.begin_with(handle, CountingMaker(opened.clone()), || {});
        let dropped = Instant::now();
        drop(source);
        let drop_took = dropped.elapsed();
        let timeout = CaptureError::Timeout {
            stage: "input_tap_join",
            ms: 1000,
        };
        assert_eq!(first, Err(timeout.clone()));
        assert!(
            took >= INPUT_STOP_DEADLINE && took < INPUT_STOP_DEADLINE + Duration::from_millis(500),
            "the first shutdown returned after {took:?}"
        );
        assert_eq!(
            state,
            SourceState::Failed {
                reason: HealthReason::Timeout,
                error: timeout
            }
        );
        assert_eq!(again, Err(CaptureError::AlreadyStarted));
        assert_eq!(
            opened.load(Ordering::SeqCst),
            0,
            "a begin after the timeout made a tap"
        );
        assert!(
            drop_took < Duration::from_millis(100),
            "the drop after a Timeout waited {drop_took:?}"
        );
    }

    struct PortMaker(std::sync::mpsc::Sender<()>, *mut c_void);

    // SAFETY: the pointer is a made-up address that only the recording enable
    // function reads as a number.
    unsafe impl Send for PortMaker {}

    impl TapMaker for PortMaker {
        type Tap = (std::sync::mpsc::Sender<()>, TapPortAddress);
        type Enabled = PortClearedFirst<std::sync::mpsc::Sender<()>>;

        fn make<F>(self, _settings: TapSettings, _callback: F) -> Result<Self::Tap, ()>
        where
            F: Fn(CGEventTapProxy, CGEventType, &CGEvent) -> CallbackResult + Send + 'static,
        {
            Ok((self.0, TapPortAddress(self.1)))
        }

        fn listen_denied() -> bool {
            false
        }

        fn enable(tap: Self::Tap, port: TapPort) -> Result<Self::Enabled, CaptureError> {
            port.store(tap.1 .0);
            Ok(PortClearedFirst { port, tap: tap.0 })
        }
    }

    struct TapPortAddress(*mut c_void);

    // SAFETY: as for PortMaker: a made-up address, read as a number alone.
    unsafe impl Send for TapPortAddress {}

    #[test]
    fn the_tap_is_disabled_through_its_port_before_shutdown_answers_timeout() {
        let (handle, _writer) = sink::channel(8);
        let mut source = source_with(HoldPlan::Never);
        source.tap_enable = record_enable;
        let address = std::ptr::NonNull::<c_void>::dangling().as_ptr();
        let (ended_tx, _ended_rx) = channel();
        source
            .begin_with(handle, PortMaker(ended_tx, address), || {
                std::thread::sleep(Duration::from_millis(3000))
            })
            .expect("the source starts with a fake tap");
        let before = ENABLE_CALLS.with(|calls| calls.borrow().clone());
        let first = source.shutdown();
        let at_the_answer = ENABLE_CALLS.with(|calls| calls.borrow().clone());
        drop(source);
        assert_eq!(before, [], "the port was called before the shutdown");
        assert_eq!(
            first,
            Err(CaptureError::Timeout {
                stage: "input_tap_join",
                ms: 1000
            })
        );
        assert_eq!(
            at_the_answer,
            [(address as usize, false)],
            "the calling thread did not disable the tap through its port before the Timeout answer"
        );
    }

    #[test]
    fn a_shutdown_that_joined_the_thread_calls_nothing_through_the_port() {
        let (handle, _writer) = sink::channel(8);
        let mut source = source_with(HoldPlan::Never);
        source.tap_enable = record_enable;
        let address = std::ptr::NonNull::<c_void>::dangling().as_ptr();
        let (ended_tx, _ended_rx) = channel();
        source
            .begin_with(handle, PortMaker(ended_tx, address), || {})
            .expect("the source starts with a fake tap");
        let shut = source.shutdown();
        assert_eq!(shut, Ok(()));
        ENABLE_CALLS.with(|calls| assert_eq!(*calls.borrow(), []));
    }

    #[test]
    fn a_release_that_the_system_refuses_makes_shutdown_answer_its_os_error_and_health_read_failed()
    {
        let (handle, _writer) = sink::channel(8);
        let mut source = source_with(HoldPlan::After(Duration::ZERO));
        source.hold = || crate::secure_input_macos::hold_with(|| 0, || -50);
        let (ended_tx, _ended_rx) = channel();
        source
            .begin_with(handle.clone(), FakeMaker(ended_tx), || {})
            .expect("the source starts with a fake tap");
        let shut = source.shutdown();
        let state = source.health().state;
        let opened = Arc::new(AtomicUsize::new(0));
        let again = source.begin_with(handle, CountingMaker(opened.clone()), || {});
        let dropped = Instant::now();
        drop(source);
        let drop_took = dropped.elapsed();
        let refused = CaptureError::Os {
            stage: "secure_input_release",
            domain: "Carbon",
            code: -50,
        };
        assert_eq!(shut, Err(refused.clone()));
        assert_eq!(
            state,
            SourceState::Failed {
                reason: HealthReason::OsError,
                error: refused
            }
        );
        assert_eq!(again, Err(CaptureError::AlreadyStarted));
        assert_eq!(opened.load(Ordering::SeqCst), 0);
        assert!(
            drop_took < Duration::from_millis(100),
            "the drop after a refused release waited {drop_took:?}"
        );
    }

    #[test]
    fn a_release_that_the_system_accepts_lets_shutdown_answer_ok() {
        let (handle, _writer) = sink::channel(8);
        let mut source = source_with(HoldPlan::After(Duration::ZERO));
        source.hold = || crate::secure_input_macos::hold_with(|| 0, || 0);
        let (ended_tx, _ended_rx) = channel();
        source
            .begin_with(handle, FakeMaker(ended_tx), || {})
            .expect("the source starts with a fake tap");
        assert_eq!(source.shutdown(), Ok(()));
        assert_eq!(
            source.health().state,
            SourceState::Stopped(StopCause::Request)
        );
    }

    #[test]
    fn a_pointer_event_after_a_stop_is_asked_is_counted_and_not_emitted() {
        let held = held(1_000);
        held.ctx.stop.store(true, Ordering::SeqCst);
        advance(1);
        on_tap_event(&held.ctx, event(CGEventType::LeftMouseDown));
        let kinds: Vec<Modality> = received(&held.writer)
            .iter()
            .map(|received| received.event().modality)
            .collect();
        assert_eq!(kinds, [], "a pointer event went out after the stop");
        assert_eq!(held.counters.pointer_dropped_at_stop(), 1);
    }

    #[test]
    fn a_pointer_event_whose_own_reading_meets_a_stop_is_counted_and_not_emitted() {
        let held = held(1_000);
        STOP_AT_ASK.with(|at| *at.borrow_mut() = Some((asked() + 1, held.ctx.stop.clone())));
        advance(1);
        on_tap_event(&held.ctx, event(CGEventType::LeftMouseDown));
        let kinds: Vec<Modality> = received(&held.writer)
            .iter()
            .map(|received| received.event().modality)
            .collect();
        assert_eq!(kinds, [], "a pointer event went out after the stop");
        assert_eq!(held.counters.pointer_dropped_at_stop(), 1);
    }

    #[test]
    fn a_pointer_event_with_no_stop_is_emitted_and_counts_no_drop() {
        let held = held(1_000);
        advance(1);
        on_tap_event(&held.ctx, event(CGEventType::LeftMouseDown));
        let kinds: Vec<Modality> = received(&held.writer)
            .iter()
            .map(|received| received.event().modality)
            .collect();
        assert_eq!(kinds, [Modality::Pointer]);
        assert_eq!(held.counters.pointer_dropped_at_stop(), 0);
    }

    #[test]
    fn max_lag_ms_is_the_largest_lag_of_a_key_at_its_arrival() {
        let held = held(1_000);
        for lag in [5, 300, 20] {
            advance(1);
            on_tap_event(
                &held.ctx,
                timed(
                    CGEventType::KeyDown,
                    Timing::InTime {
                        lag: Duration::from_millis(lag),
                    },
                ),
            );
        }
        let figures = held.counters.figures();
        assert_eq!(figures.max_lag_ms, 300);
        assert_eq!(figures.late_events, 1, "only the key above 250 ms is late");
    }

    fn keystroke_classes(writer: &sink::SinkWriter) -> Vec<KeyClass> {
        received(writer)
            .into_iter()
            .filter_map(|received| received.into_floor().ok())
            .filter_map(|floor| match floor.event().observation() {
                crate::floor::payload::Observation::Keystroke(class) => Some(*class),
                _ => None,
            })
            .collect()
    }

    fn key_with_code(ctx: &TapContext, etype: CGEventType, code: i64, flags: u64) {
        advance(1);
        on_tap_event(ctx, with_key(etype, code, flags));
    }

    #[test]
    fn a_released_key_is_one_keystroke_record_that_holds_its_class_and_nothing_else() {
        let held = held(100);
        key_with_code(&held.ctx, CGEventType::KeyDown, 0x24, 0);
        key_with_code(&held.ctx, CGEventType::KeyUp, 0x24, 0);
        key_with_code(&held.ctx, CGEventType::KeyDown, 0x00, 0x10_0000);
        let mut repeat = with_key(CGEventType::KeyDown, 0x00, 0x10_0000);
        repeat.autorepeat = true;
        advance(1);
        on_tap_event(&held.ctx, repeat);
        key_with_code(
            &held.ctx,
            CGEventType::FlagsChanged,
            LEFT_SHIFT,
            0x2_0000 | LEFT_SHIFT_BIT,
        );
        key_with_code(&held.ctx, CGEventType::FlagsChanged, LEFT_SHIFT, 0);
        ticks(&held.ctx, 200);
        assert_eq!(
            keystroke_classes(&held.writer),
            [KeyClass::Enter, KeyClass::Shortcut, KeyClass::Other],
            "a key up, a repeat and a released modifier are no record"
        );
    }

    #[test]
    fn a_key_older_than_the_late_bound_raises_late_events_only_and_leaves_no_record() {
        let held = held(100);
        advance(1);
        on_tap_event(
            &held.ctx,
            timed(
                CGEventType::KeyDown,
                Timing::Late {
                    lag: Duration::from_millis(300),
                },
            ),
        );
        ticks(&held.ctx, 200);
        let figures = held.counters.figures();
        assert_eq!(figures.late_events, 1);
        assert_eq!(figures.secure_input_keystrokes, 0);
        assert_eq!(keystrokes(&held.writer), 0);
    }

    #[test]
    fn a_key_late_for_the_hold_but_not_for_the_check_is_withheld_and_not_counted_late() {
        let held = held(100);
        advance(1);
        on_tap_event(
            &held.ctx,
            timed(
                CGEventType::KeyDown,
                Timing::Late {
                    lag: Duration::from_millis(100),
                },
            ),
        );
        ticks(&held.ctx, 200);
        let figures = held.counters.figures();
        assert_eq!((figures.late_events, figures.keys_withheld), (0, true));
        assert_eq!(keystrokes(&held.writer), 0);
    }

    fn fixed_time() -> std::time::SystemTime {
        std::time::UNIX_EPOCH + Duration::from_millis(101_010)
    }

    static FIXED_CLOCK: EventClock = EventClock::new(fixed_time);

    #[test]
    fn a_released_key_is_stamped_with_its_age_at_the_arrival_and_the_hold() {
        let mut held = held(1_000);
        held.ctx.clock = &FIXED_CLOCK;
        advance(1);
        on_tap_event(
            &held.ctx,
            timed(
                CGEventType::KeyDown,
                Timing::InTime {
                    lag: Duration::from_millis(20),
                },
            ),
        );
        ticks(&held.ctx, 1_100);
        let stamps: Vec<i64> = received(&held.writer)
            .into_iter()
            .filter_map(|received| received.into_floor().ok())
            .map(|floor| floor.event().at())
            .collect();
        assert_eq!(
            stamps,
            [99_000_000_000],
            "101.010 s less 20 ms of age at the arrival and 1,000 ms held, at the resolution of 1 s"
        );
    }

    #[test]
    fn no_key_record_leaves_for_a_listed_app_in_front_at_the_arrival_or_during_the_hold() {
        let held = held(100);
        held.fg_writer.set_for_test(Some(LISTED.front()));
        key_down(&held.ctx);
        held.fg_writer.set_for_test(Some(APP.front()));
        ticks(&held.ctx, 200);
        assert_eq!(keystrokes(&held.writer), 0, "listed at the arrival");

        key_down(&held.ctx);
        tick(&held.ctx);
        held.fg_writer.set_for_test(Some(LISTED.front()));
        tick(&held.ctx);
        held.fg_writer.set_for_test(Some(APP.front()));
        ticks(&held.ctx, 200);
        assert_eq!(keystrokes(&held.writer), 0, "listed during the hold");

        key_down(&held.ctx);
        ticks(&held.ctx, 200);
        assert_eq!(keystrokes(&held.writer), 1, "any other app in front");
    }

    struct UnderRoot;

    impl crate::floor::exclude::Lineage for UnderRoot {
        fn parent_of(&self, pid: ProcessId) -> crate::floor::exclude::Parent {
            match pid.get() {
                4242 => crate::floor::exclude::Parent::Of(ProcessId::new(500)),
                _ => crate::floor::exclude::Parent::None,
            }
        }
    }

    #[test]
    fn no_key_or_pointer_record_leaves_for_a_process_under_the_exclusion_root() {
        let mut held = held(100);
        held.ctx.floor = FloorHandle::new(
            Some(ProcessId::new(500)),
            Arc::new(UnderRoot),
            crate::floor::Switches::default(),
        );
        key_down(&held.ctx);
        pointer(&held.ctx, CGEventType::LeftMouseDown, 1);
        pointer(&held.ctx, CGEventType::MouseMoved, 3);
        ticks(&held.ctx, 200);
        close_interval(&held.ctx);
        assert_eq!(received(&held.writer).len(), 0);

        held.fg_writer
            .set_for_test(Some(TestApp("com.example.other", 4444).front()));
        key_down(&held.ctx);
        pointer(&held.ctx, CGEventType::LeftMouseDown, 1);
        ticks(&held.ctx, 200);
        assert_eq!(
            received(&held.writer).len(),
            2,
            "a process outside the tree"
        );
    }

    #[test]
    fn with_the_key_switch_off_a_key_down_holds_nothing_and_gives_no_record() {
        let mut held = held(100);
        let mut switches = crate::floor::Switches::default();
        switches.set(crate::floor::Switch::KeyClass, false);
        held.ctx.floor =
            FloorHandle::new(None, Arc::new(crate::floor::exclude::NoLineage), switches);
        key_down(&held.ctx);
        assert_eq!(held_now(&held.ctx), 0);
        ticks(&held.ctx, 200);
        assert_eq!(keystrokes(&held.writer), 0);
        pointer(&held.ctx, CGEventType::LeftMouseDown, 1);
        assert_eq!(pointer_records(&held.writer), [("primary", 1)]);
    }

    #[test]
    fn with_the_pointer_switch_off_no_pointer_record_leaves() {
        let mut held = held(100);
        let mut switches = crate::floor::Switches::default();
        switches.set(crate::floor::Switch::PointerClass, false);
        held.ctx.floor =
            FloorHandle::new(None, Arc::new(crate::floor::exclude::NoLineage), switches);
        pointer(&held.ctx, CGEventType::LeftMouseDown, 1);
        pointer(&held.ctx, CGEventType::ScrollWheel, 4);
        close_interval(&held.ctx);
        assert_eq!(pointer_records(&held.writer), []);
        key_down(&held.ctx);
        ticks(&held.ctx, 200);
        assert_eq!(keystrokes(&held.writer), 1);
    }

    #[test]
    fn a_reading_of_unknown_withholds_the_key_and_records_nothing() {
        let held = held(100);
        answer(Reading::Unknown);
        key_down(&held.ctx);
        answer(Reading::Off);
        ticks(&held.ctx, 200);
        let figures = held.counters.figures();
        assert_eq!(keystroke_classes(&held.writer), []);
        assert!(figures.keys_withheld);
        assert_eq!(figures.secure_input_keystrokes, 0);
    }

    fn switches_with(key: bool, pointer: bool) -> crate::floor::Switches {
        let mut switches = crate::floor::Switches::default();
        switches.set(crate::floor::Switch::KeyClass, key);
        switches.set(crate::floor::Switch::PointerClass, pointer);
        switches
    }

    #[test]
    fn the_mask_holds_the_key_types_only_with_key_class_on_and_the_mouse_types_only_with_pointer_class_on(
    ) {
        let rows = [
            (true, true, 0x0E40_1CFE),
            (false, true, 0x0E40_00FE),
            (true, false, 0x0000_1C00),
            (false, false, 0),
        ];
        for (key, pointer, mask) in rows {
            assert_eq!(
                mask_of(&events_of_interest(switches_with(key, pointer))),
                mask,
                "key_class {key}, pointer_class {pointer}"
            );
        }
    }

    #[test]
    fn a_granted_mask_without_the_key_types_fails_and_any_other_passes() {
        assert!(!key_events_granted(0x0E40_1CFE, 0x0E40_10FE));
        assert!(!key_events_granted(0x0E40_1CFE, 0x0E40_14FE));
        assert!(key_events_granted(0x0E40_1CFE, 0x0E40_1CFE));
        assert!(
            key_events_granted(0x0E40_00FE, 0x0E40_00FE),
            "with key_class off no key type is asked for"
        );
        assert!(key_events_granted(0x0E40_00FE, 0));
    }

    struct DeniedMaker(Arc<AtomicUsize>);

    impl TapMaker for DeniedMaker {
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
            true
        }

        fn enable(_tap: (), _port: TapPort) -> Result<(), CaptureError> {
            Ok(())
        }
    }

    struct MadeMaker(Arc<AtomicUsize>);

    impl TapMaker for MadeMaker {
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

    fn source_switched(switches: crate::floor::Switches) -> InputSource {
        let (_fg_writer, reader) = foreground_state_for_test();
        let mut source = InputSource::new(reader, InputCounters::new(), HoldPlan::Never);
        source.floor = FloorHandle::new(None, Arc::new(crate::floor::exclude::NoLineage), switches);
        source
    }

    #[test]
    fn a_start_without_the_grant_for_key_events_fails_with_the_permission_error_and_makes_no_tap() {
        let (handle, _writer) = sink::channel(4);
        let made = Arc::new(AtomicUsize::new(0));
        let mut source = source_switched(switches_with(true, true));
        let begun = source.begin_with(handle, DeniedMaker(made.clone()), || {});
        assert_eq!(
            begun,
            Err(CaptureError::PermissionDenied(Modality::Keystroke))
        );
        assert_eq!(made.load(Ordering::SeqCst), 0, "no tap was asked for");
        assert!(matches!(
            source.health().state,
            SourceState::Failed {
                reason: HealthReason::Permission,
                ..
            }
        ));
    }

    #[test]
    fn with_both_switches_off_the_source_answers_off_and_makes_no_tap() {
        let (handle, _writer) = sink::channel(4);
        let made = Arc::new(AtomicUsize::new(0));
        let mut source = source_switched(switches_with(false, false));
        let begun = source.begin_with(handle, MadeMaker(made.clone()), || {});
        assert_eq!(begun, Ok(Started::Off));
        assert_eq!(made.load(Ordering::SeqCst), 0, "no tap was asked for");
        assert_eq!(source.health().state, SourceState::Off);
        assert_eq!(source.shutdown(), Ok(()));
    }

    struct KeylessMaker;

    impl TapMaker for KeylessMaker {
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

        fn granted_events(_tap: &(), _requested: u64) -> u64 {
            0x0E40_10FE
        }

        fn enable(_tap: (), _port: TapPort) -> Result<(), CaptureError> {
            panic!("a tap granted without key events is not enabled")
        }
    }

    #[test]
    fn a_tap_granted_without_the_key_types_fails_its_start_and_reads_no_key_events() {
        let held = held(100);
        let Held { ctx, .. } = held;
        let missing = Arc::new(AtomicBool::new(false));
        let installed = install_tap(*ctx, stand_in_settings(), KeylessMaker, missing.clone());
        assert_eq!(
            installed.err(),
            Some(CaptureError::PermissionDenied(Modality::Keystroke))
        );
        assert!(missing.load(Ordering::SeqCst));
        let mut source = source_switched(switches_with(true, true));
        source.keys_missing = missing;
        source.failure = Some(CaptureError::PermissionDenied(Modality::Keystroke));
        assert!(matches!(
            source.health().state,
            SourceState::Failed {
                reason: HealthReason::NoKeyEvents,
                ..
            }
        ));
    }

    #[test]
    fn each_tap_disabled_notice_raises_its_own_counter() {
        let held = held(100);
        for etype in [
            CGEventType::TapDisabledByTimeout,
            CGEventType::TapDisabledByUserInput,
            CGEventType::TapDisabledByTimeout,
        ] {
            advance(1);
            on_tap_event(&held.ctx, event(etype));
        }
        let figures = held.counters.figures();
        assert_eq!(
            (
                figures.tap_disabled_timeout,
                figures.tap_disabled_user_input
            ),
            (2, 1)
        );
    }

    thread_local! {
        static TYPED: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) };
    }

    fn stand_in_typed() -> u32 {
        TYPED.with(|typed| typed.borrow_mut().remove(0))
    }

    fn phase_of(counters: &InputCounters, held: Arc<AtomicBool>) -> HeldPhase {
        HeldPhase {
            held,
            counters: counters.clone(),
            secure: stand_in_reader,
            session: stand_in_session,
            typed: stand_in_typed,
            typed_at_take: None,
            sampler: None,
        }
    }

    #[test]
    fn the_held_phase_samples_every_250_ms() {
        assert_eq!(HELD_SAMPLE_STEP, Duration::from_millis(250));
    }

    #[test]
    fn eight_samples_under_a_held_reading_give_8_of_8_and_under_off_0_of_8() {
        for (reading, held) in [(Reading::On, 8), (Reading::Off, 0)] {
            reset_stand_ins();
            answer(reading);
            let counters = InputCounters::new();
            let phase = phase_of(&counters, Arc::new(AtomicBool::new(true)));
            for _ in 0..8 {
                phase.sample();
            }
            assert_eq!(
                counters.figures().secure_input_held,
                HeldSamples { held, taken: 8 },
                "{reading:?}"
            );
        }
    }

    #[test]
    fn hid_counts_of_10_then_12_read_keys_typed_and_10_then_10_read_none() {
        for (end, typed) in [(12, true), (10, false)] {
            reset_stand_ins();
            TYPED.with(|counts| *counts.borrow_mut() = vec![10, end]);
            let counters = InputCounters::new();
            let flag = Arc::new(AtomicBool::new(false));
            let mut phase = phase_of(&counters, flag.clone());
            phase.enter();
            assert!(flag.load(Ordering::SeqCst), "the held phase began");
            phase.end();
            assert!(!flag.load(Ordering::SeqCst), "the held phase ended");
            assert_eq!(
                counters.figures().secure_window_keys_typed,
                typed,
                "10 then {end}"
            );
        }
    }

    #[test]
    fn a_taken_hold_begins_the_held_phase_and_its_give_back_ends_it() {
        reset_stand_ins();
        TYPED.with(|counts| *counts.borrow_mut() = vec![3, 4]);
        let counters = InputCounters::new();
        let flag = Arc::new(AtomicBool::new(false));
        let mut slot = TimedHold::new(HoldSlot {
            hold: stand_in_hold,
            held: None,
            refusal: Arc::new(Mutex::new(None)),
            own_take: None,
            phase: Some(phase_of(&counters, flag.clone())),
        });
        slot.take_now();
        assert!(flag.load(Ordering::SeqCst));
        assert_eq!(slot.give_back(), Ok(()));
        assert!(!flag.load(Ordering::SeqCst));
        assert!(counters.figures().secure_window_keys_typed);
    }

    #[test]
    fn a_key_counts_in_the_phase_of_its_arrival() {
        let held = held(100);
        key_down(&held.ctx);
        held.ctx.held_phase.store(true, Ordering::SeqCst);
        key_down(&held.ctx);
        held.ctx.held_phase.store(false, Ordering::SeqCst);
        ticks(&held.ctx, 200);
        assert_eq!(keystrokes(&held.writer), 2);
        let figures = held.counters.figures();
        assert_eq!(
            (
                figures.secure_released_keystrokes,
                figures.secure_held_keystrokes
            ),
            (1, 1)
        );
    }

    #[test]
    fn a_key_recorded_in_the_released_phase_raises_the_released_count_alone() {
        let held = held(100);
        key_down(&held.ctx);
        ticks(&held.ctx, 200);
        let figures = held.counters.figures();
        assert_eq!(
            (
                figures.secure_released_keystrokes,
                figures.secure_held_keystrokes,
                figures.secure_window_pointer
            ),
            (1, 0, 0)
        );
    }

    #[test]
    fn the_key_events_suppressed_in_the_held_phase_are_counted_by_type() {
        let held = held(100);
        held.ctx.held_phase.store(true, Ordering::SeqCst);
        answer(Reading::On);
        key_with_code(&held.ctx, CGEventType::KeyDown, 0x00, 0);
        let mut repeat = with_key(CGEventType::KeyDown, 0x00, 0);
        repeat.autorepeat = true;
        advance(1);
        on_tap_event(&held.ctx, repeat);
        key_with_code(&held.ctx, CGEventType::KeyUp, 0x00, 0);
        key_with_code(
            &held.ctx,
            CGEventType::FlagsChanged,
            LEFT_SHIFT,
            0x2_0000 | LEFT_SHIFT_BIT,
        );
        key_with_code(&held.ctx, CGEventType::FlagsChanged, LEFT_SHIFT, 0);
        held.ctx.held_phase.store(false, Ordering::SeqCst);
        key_with_code(&held.ctx, CGEventType::KeyDown, 0x00, 0);
        let figures = held.counters.figures();
        assert_eq!(
            figures.secure_held_suppressed,
            SuppressedKeys {
                down: 2,
                up: 1,
                modifier: 2
            }
        );
    }

    #[test]
    fn a_pointer_action_in_the_held_phase_raises_the_pointer_counter_and_no_suppression() {
        let held = held(100);
        held.ctx.held_phase.store(true, Ordering::SeqCst);
        answer(Reading::On);
        pointer(&held.ctx, CGEventType::LeftMouseDown, 1);
        pointer(&held.ctx, CGEventType::LeftMouseUp, 1);
        pointer(&held.ctx, CGEventType::MouseMoved, 2);
        held.ctx.held_phase.store(false, Ordering::SeqCst);
        pointer(&held.ctx, CGEventType::LeftMouseDown, 1);
        let figures = held.counters.figures();
        assert_eq!(figures.secure_window_pointer, 3);
        assert_eq!(figures.secure_input_keystrokes, 0);
        assert_eq!(figures.secure_held_suppressed, SuppressedKeys::default());
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
        ctx.start(&mut lock(&ctx.hold));
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
            timed(
                CGEventType::KeyDown,
                Timing::Late {
                    lag: Duration::from_millis(40),
                },
            ),
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
        on_tap_event(&held.ctx, timed(CGEventType::KeyDown, Timing::Untimed));
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
            timed(
                CGEventType::KeyUp,
                Timing::Late {
                    lag: Duration::from_millis(40),
                },
            ),
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
        let asked_at_the_stop = asked();
        tick(&held.ctx);
        assert_eq!(
            asked(),
            asked_at_the_stop,
            "a tick after the stop takes no reading"
        );
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
        let asked_at_the_stop = asked();
        advance(1);
        on_tap_event(&held.ctx, event(CGEventType::KeyUp));
        assert_eq!(
            asked(),
            asked_at_the_stop,
            "an event after the stop takes no reading"
        );
        assert_eq!(held_now(&held.ctx), 0);
        assert!(!held.counters.figures().keys_withheld);
        assert_eq!(held.counters.suppressed_keys(), 0);
    }

    #[test]
    fn a_gap_after_a_stop_is_asked_leaves_the_source_unstalled() {
        let held = held(1_000);
        key_down(&held.ctx);
        held.ctx.stop.store(true, Ordering::SeqCst);
        let asked_at_the_stop = asked();
        advance(40);
        on_reading_tick(&held.ctx);
        assert_eq!(
            asked(),
            asked_at_the_stop,
            "a tick after the stop takes no reading"
        );
        advance(40);
        on_tap_event(&held.ctx, event(CGEventType::KeyDown));
        assert_eq!(
            asked(),
            asked_at_the_stop,
            "an event after the stop takes no reading"
        );
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

    mod drawn;

    const NO_CLOSE_STEP_NS: u64 = 200_000;

    fn no_close_pairs() {
        UPTIME_STEP.with(|step| step.set(NO_CLOSE_STEP_NS));
    }

    fn close_pairs() {
        UPTIME_STEP.with(|step| step.set(0));
    }

    fn with_stepped_uptime(mut held: Held) -> Held {
        held.ctx.clocks = Clocks {
            stamps: stand_in_monotonic,
            os_time: stepped_uptime,
        };
        held
    }

    fn held_with_no_close_pair_at_the_start(bound_ms: u64) -> Held {
        let held = with_stepped_uptime(slow_held(bound_ms));
        lock(&held.ctx.hold).offset = Some(i128::MAX);
        no_close_pairs();
        held.ctx.start(&mut lock(&held.ctx.hold));
        close_pairs();
        assert_eq!(
            lock(&held.ctx.hold).offset,
            None,
            "a start whose three pairs did not come close keeps no difference"
        );
        held
    }

    fn key_with_os_time(ctx: &TapContext, os_time: u64) {
        on_tap_event(
            ctx,
            TapEvent {
                os_time: Uptime::from_nanos(os_time),
                ..event(CGEventType::KeyDown)
            },
        );
    }

    fn pointer_now(ctx: &TapContext) {
        advance(1);
        on_tap_event(ctx, event(CGEventType::MouseMoved));
    }

    fn fate(held: &Held) -> (usize, usize, bool, bool) {
        (
            held_now(&held.ctx),
            keystrokes(&held.writer),
            held.counters.figures().keys_withheld,
            held.counters.stalled(),
        )
    }

    #[test]
    fn a_key_from_before_a_sleep_is_withheld_whether_or_not_the_start_kept_a_difference() {
        for no_close_at_the_start in [false, true] {
            let held = if no_close_at_the_start {
                held_with_no_close_pair_at_the_start(100)
            } else {
                with_stepped_uptime(slow_held(100))
            };
            stall(MS);
            let typed = now_uptime();
            sleep_of_the_mac(60_000);
            stall(5 * MS);
            key_with_os_time(&held.ctx, typed);
            let held_after = held_now(&held.ctx);
            ticks(&held.ctx, 300);
            assert_eq!(
                (held_after, keystrokes(&held.writer)),
                (0, 0),
                "a key from before a 60 s sleep, no close pair at the start: {no_close_at_the_start}"
            );
        }
    }

    #[test]
    fn with_no_close_pair_at_the_start_a_key_whose_own_reading_reads_the_session_again_after_a_sleep_is_withheld(
    ) {
        let held = held_with_no_close_pair_at_the_start(100);
        stall(MS);
        let typed = now_uptime();
        sleep_of_the_mac(60_000);
        stall(5 * MS);
        let session_reads = SESSION_READS.with(Cell::get);
        key_with_os_time(&held.ctx, typed);
        assert!(
            SESSION_READS.with(Cell::get) > session_reads,
            "the key's own reading read the session again"
        );
        ticks(&held.ctx, 300);
        assert_eq!(fate(&held), (0, 0, true, false));
    }

    #[test]
    fn the_first_key_after_a_start_with_no_close_pair_is_withheld_and_the_next_is_released() {
        let held = held_with_no_close_pair_at_the_start(100);
        key_down(&held.ctx);
        assert_eq!(
            (held_now(&held.ctx), held.counters.figures().keys_withheld),
            (0, true),
            "the first close pair after such a start is a wake, and the key is not after its mark"
        );
        key_down(&held.ctx);
        ticks(&held.ctx, 150);
        assert_eq!(
            keystrokes(&held.writer),
            1,
            "the key typed after the mark is released"
        );
    }

    #[test]
    fn with_no_close_pair_at_the_start_a_pointer_first_takes_the_pair_and_a_later_key_is_released()
    {
        let held = held_with_no_close_pair_at_the_start(100);
        pointer_now(&held.ctx);
        key_down(&held.ctx);
        assert_eq!(held_now(&held.ctx), 1);
        ticks(&held.ctx, 150);
        assert_eq!(fate(&held), (0, 1, false, false), "no key lost");
    }

    #[test]
    fn with_no_close_pair_at_the_start_a_pointer_after_a_sleep_frees_no_key_from_before_the_sleep()
    {
        let held = held_with_no_close_pair_at_the_start(100);
        stall(MS);
        let typed = now_uptime();
        sleep_of_the_mac(60_000);
        pointer_now(&held.ctx);
        stall(5 * MS);
        key_with_os_time(&held.ctx, typed);
        ticks(&held.ctx, 300);
        assert_eq!(fate(&held), (0, 0, true, false));
    }

    #[test]
    fn a_tap_notice_first_takes_no_pair_and_the_first_key_after_a_start_with_no_close_pair_is_withheld(
    ) {
        let held = held_with_no_close_pair_at_the_start(100);
        on_tap_event(&held.ctx, event(CGEventType::TapDisabledByTimeout));
        assert_eq!(
            lock(&held.ctx.hold).offset,
            None,
            "a tap notice takes no pair"
        );
        key_down(&held.ctx);
        assert_eq!(held_now(&held.ctx), 0);
        key_down(&held.ctx);
        ticks(&held.ctx, 150);
        assert_eq!(keystrokes(&held.writer), 1);
    }

    #[test]
    fn a_restart_is_a_start_and_keeps_nothing_of_the_source_before_it() {
        let first = held(100);
        key_down(&first.ctx);
        ticks(&first.ctx, 150);
        assert_eq!(
            keystrokes(&first.writer),
            1,
            "the first source releases its key"
        );
        assert!(lock(&first.ctx.hold).offset.is_some());
        drop(first);
        let second = held_with_no_close_pair_at_the_start(100);
        key_down(&second.ctx);
        assert_eq!(
            (
                held_now(&second.ctx),
                second.counters.figures().keys_withheld
            ),
            (0, true),
            "the started source after a shutdown keeps no difference of the one before it"
        );
    }

    #[test]
    fn a_change_of_the_session_unsets_no_difference_and_reads_no_wake() {
        let held = slow_held(1_000);
        let kept = lock(&held.ctx.hold).offset;
        assert!(kept.is_some());
        key_down(&held.ctx);
        SESSION.with(|session| session.set(Session::NotAtConsole));
        ticks(&held.ctx, 300);
        assert_eq!(
            held_now(&held.ctx),
            0,
            "a session not at the console drops the key"
        );
        SESSION.with(|session| session.set(Session::AtConsole));
        advance(300);
        key_down(&held.ctx);
        ticks(&held.ctx, 1_050);
        assert_eq!(
            lock(&held.ctx.hold).offset,
            kept,
            "the session's change kept the difference"
        );
        assert_eq!(
            keystrokes(&held.writer),
            1,
            "the key after the change is released, with no wake"
        );
    }

    #[test]
    fn clocks_whose_pairs_never_come_close_note_no_key_and_read_impaired() {
        let held = held_with_no_close_pair_at_the_start(100);
        no_close_pairs();
        let mut stalled_after = Vec::new();
        for _ in 0..100 {
            key_down(&held.ctx);
            stalled_after.push(held.counters.stalled());
            ticks(&held.ctx, 20);
        }
        ticks(&held.ctx, 300);
        close_pairs();
        assert_eq!(keystrokes(&held.writer), 0, "no key is recorded");
        assert!(
            stalled_after.iter().all(|stalled| *stalled),
            "impaired after every key: {} of 100",
            stalled_after.iter().filter(|stalled| **stalled).count()
        );
        assert!(held.counters.stalled(), "impaired after the last key");
    }

    #[test]
    fn one_reading_whose_pairs_fail_reads_impaired_until_the_next_release() {
        let (held, drops) = seam_held(100);
        let held = with_stepped_uptime(held);
        no_close_pairs();
        pointer_now(&held.ctx);
        close_pairs();
        assert!(
            held.counters.stalled(),
            "a failed reading that drops no key reads impaired"
        );
        assert!(
            !held.counters.figures().keys_withheld,
            "it withheld nothing"
        );
        key_down(&held.ctx);
        assert!(held.counters.stalled(), "still impaired before a release");
        ticks(&held.ctx, 150);
        assert_eq!(keystrokes(&held.writer), 1);
        assert!(!held.counters.stalled(), "the release clears it");
        key_down(&held.ctx);
        no_close_pairs();
        tick(&held.ctx);
        close_pairs();
        assert_eq!(held_now(&held.ctx), 0);
        assert!(
            held.counters.stalled(),
            "a failed reading that drops a key reads impaired"
        );
        assert_eq!(
            (drops.of("unclocked"), drops.of("sleep")),
            (1, 0),
            "the failed pair counts apart from sleep"
        );
    }

    #[test]
    fn a_key_withheld_at_a_reading_whose_pairs_fail_has_the_cause_unclocked_and_reads_impaired() {
        let (held, drops) = seam_held(100);
        let held = with_stepped_uptime(held);
        no_close_pairs();
        key_down(&held.ctx);
        close_pairs();
        assert_eq!(
            (
                held_now(&held.ctx),
                held.counters.figures().keys_withheld,
                held.counters.stalled()
            ),
            (0, true, true)
        );
        let entries = drops.take_entries();
        assert!(
            entries.iter().any(|entry| matches!(
                entry,
                RawEntry::Arrival {
                    admission: Arrived::Withheld {
                        cause: Cause::Unclocked,
                        ..
                    },
                    ..
                }
            )),
            "{entries:?}"
        );
        assert_eq!((drops.of("unclocked"), drops.of("sleep")), (1, 0));
    }

    #[test]
    fn a_failed_reading_after_a_stop_moves_no_flag() {
        let held = with_stepped_uptime(slow_held(100));
        key_down(&held.ctx);
        let stop = held.ctx.stop.clone();
        STOP_AT_ASK.with(|at| *at.borrow_mut() = Some((asked() + 1, stop)));
        no_close_pairs();
        tick(&held.ctx);
        close_pairs();
        assert_eq!(fate(&held), (0, 0, false, false));
    }

    type StopRow = (&'static str, Reading, u64);

    #[test]
    fn a_stop_asked_during_a_keys_own_reading_moves_no_flag_and_no_count() {
        let rows: [StopRow; 3] = [
            ("a late key, its reading 40 ms long", Reading::Off, 40 * MS),
            ("a key under unknown", Reading::Unknown, 0),
            ("a key under on", Reading::On, 0),
        ];
        for (what, reading, read_ns) in rows {
            let held = slow_held(1_000);
            key_down(&held.ctx);
            assert_eq!(held_now(&held.ctx), 1, "{what}");
            let before = (
                held.counters.figures().keys_withheld,
                held.counters.stalled(),
                held.counters.suppressed_keys(),
            );
            let stop = held.ctx.stop.clone();
            STOP_AT_ASK.with(|at| *at.borrow_mut() = Some((asked() + 1, stop)));
            ANSWERS.with(|answers| answers.borrow_mut().push(reading));
            SLOW_READ_NS.with(|ns| ns.set(read_ns));
            key_down(&held.ctx);
            SLOW_READ_NS.with(|ns| ns.set(0));
            let after = (
                held.counters.figures().keys_withheld,
                held.counters.stalled(),
                held.counters.suppressed_keys(),
            );
            assert_eq!((held_now(&held.ctx), after), (0, before), "{what}");
        }
    }

    #[test]
    fn an_arriving_key_at_a_stop_is_one_arrival_withheld_with_the_cause_stop() {
        let (held, drops) = seam_held(1_000);
        key_down(&held.ctx);
        let _ = drops.take_entries();
        let stop = held.ctx.stop.clone();
        STOP_AT_ASK.with(|at| *at.borrow_mut() = Some((asked() + 1, stop)));
        key_down(&held.ctx);
        let entries = drops.take_entries();
        let fates = {
            let mut all = vec![RawEntry::Arrival {
                at: 0,
                admission: Arrived::Held,
            }];
            all.extend(entries.iter().copied());
            replayed_fates(&all)
        };
        assert_eq!(fates, ["dropped Stop", "withheld Stop"], "{entries:?}");
        assert_eq!(
            entries
                .iter()
                .filter(|entry| matches!(entry, RawEntry::Arrival { .. }))
                .count(),
            1,
            "one arrival for the arriving key"
        );
        assert_eq!(
            drops.of("stop"),
            2,
            "the held key and the arriving key, both with the cause stop"
        );
    }

    #[test]
    fn a_late_key_at_a_stop_still_counts_as_a_late_event() {
        let held = slow_held(1_000);
        let stop = held.ctx.stop.clone();
        STOP_AT_ASK.with(|at| *at.borrow_mut() = Some((asked() + 1, stop)));
        advance(1);
        let typed = now_uptime() - 300 * MS;
        key_with_os_time(&held.ctx, typed);
        assert_eq!(
            (
                held.counters.figures().late_events,
                held.counters.figures().keys_withheld,
                held.counters.stalled()
            ),
            (1, false, false),
            "late_events reads the lag at the entry; the key's fate is the stop's"
        );
    }

    #[test]
    fn a_key_from_before_a_sleep_whose_callback_comes_late_is_asleep_and_no_timing_fault() {
        let held = held(1_000);
        key_down(&held.ctx);
        ticks(&held.ctx, 1_000);
        assert_eq!(
            keystrokes(&held.writer),
            1,
            "a release clears nothing that is set"
        );
        stall(MS);
        let typed = now_uptime();
        sleep_of_the_mac(60_000);
        tick(&held.ctx);
        stall(40 * MS);
        key_with_os_time(&held.ctx, typed);
        assert_eq!(
            (
                held_now(&held.ctx),
                held.counters.figures().keys_withheld,
                held.counters.stalled()
            ),
            (0, true, false),
            "the cause sleep is read before the lag"
        );
    }

    #[test]
    fn a_stop_that_lands_during_an_emit_of_the_release_loop_takes_no_reading_after_it() {
        let held = held_with(100, 64, 0);
        key_down(&held.ctx);
        key_down(&held.ctx);
        ticks(&held.ctx, 90);
        assert_eq!(held_now(&held.ctx), 2);
        let stop = held.ctx.stop.clone();
        let writer = held.writer;
        let receiver = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            stop.store(true, Ordering::SeqCst);
            let mut kinds = Vec::new();
            while let Ok(received) = writer.recv_timeout(Duration::from_millis(500)) {
                kinds.push(received.event().modality);
            }
            kinds
        });
        let asked_before = asked();
        tick(&held.ctx);
        assert_eq!(
            asked(),
            asked_before + 1,
            "no reading was taken after the emit during which the stop landed"
        );
        assert_eq!(held_now(&held.ctx), 0, "the second key was dropped");
        assert!(
            !held.counters.figures().keys_withheld,
            "a drop with the cause stop"
        );
        assert!(!held.counters.stalled());
        let Held {
            ctx,
            counters,
            fg_writer,
            ..
        } = held;
        drop((ctx, counters, fg_writer));
        assert_eq!(
            receiver.join().expect("the receiver ends"),
            [Modality::Keystroke],
            "the first key reached the writer before the stop was read"
        );
    }
}

#[cfg(test)]
mod granted_tests {
    use super::*;
    use crate::macos::foreground::foreground_state_for_test;
    use crate::sink;

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
            move || {
                install_tap(
                    ctx,
                    tap_settings(),
                    SystemTapMaker,
                    Arc::new(AtomicBool::new(false)),
                )
            },
            TapPlan {
                plan: HoldPlan::Never,
                hold: hold_secure_input,
                refusal: Arc::new(Mutex::new(None)),
                released: Arc::new(Mutex::new(None)),
                own_take: None,
                phase: None,
            },
            stop,
            port.clone(),
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

    fn context(
        secure: fn(Session) -> Reading,
    ) -> (
        TapContext,
        sink::SinkWriter,
        crate::macos::foreground::ForegroundWriter,
    ) {
        let (handle, writer) = sink::channel(8);
        let (front, reader) = crate::macos::foreground::foreground_state_for_test();
        front.set_for_test(Some(crate::macos::foreground::tests_support::front(
            "com.example.editor",
            4242,
        )));
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
        ctx.start(&mut lock(&ctx.hold));
        (ctx, writer, front)
    }

    fn event(etype: CGEventType, flags: u64) -> TapEvent {
        event_now(etype, flags)
    }

    #[test]
    fn a_key_press_under_secure_input_records_nothing_and_is_counted() {
        let (ctx, writer, _front) = context(|_| Reading::On);
        let shift = core_graphics::event::CGEventFlags::CGEventFlagShift.bits();
        on_tap_event(&ctx, event(CGEventType::KeyDown, 0));
        on_tap_event(&ctx, event(CGEventType::KeyUp, 0));
        on_tap_event(
            &ctx,
            Keyed {
                event: event(CGEventType::FlagsChanged, shift | 0x2),
                key_code: Some(0x38),
                autorepeat: false,
            },
        );
        on_tap_event(
            &ctx,
            Keyed {
                event: event(CGEventType::FlagsChanged, 0),
                key_code: Some(0x38),
                autorepeat: false,
            },
        );
        on_tap_event(&ctx, event(CGEventType::LeftMouseDown, 0));
        assert_eq!(
            ctx.counters.suppressed_keys(),
            2,
            "a key down and a shift press, each read from its own event (ruling MA-22 (d)); the release is no press"
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
        let (ctx, _writer, _front) = context(|_| Reading::On);
        assert_eq!(ctx.counters.suppressed_keys(), 0);
        on_tap_event(&ctx, event(CGEventType::KeyDown, 0));
        assert_eq!(ctx.counters.suppressed_keys(), 1);
        assert_eq!(ctx.counters.suppressed_keys(), 1);
    }

    #[test]
    fn pointer_events_record_under_secure_input_without_a_count() {
        let (ctx, writer, _front) = context(|_| Reading::On);
        on_tap_event(&ctx, event(CGEventType::RightMouseDown, 0));
        on_tap_event(&ctx, event(CGEventType::LeftMouseDown, 0));
        assert_eq!(ctx.counters.suppressed_keys(), 0);
        drop(ctx);
        let recorded = events_until_the_sink_closes(&writer);
        assert_eq!(recorded, [Modality::Pointer, Modality::Pointer]);
    }

    #[test]
    fn a_key_event_without_secure_input_is_recorded_at_its_release_and_not_at_its_arrival() {
        let (ctx, writer, _front) = context(|_| Reading::Off);
        on_tap_event(&ctx, event(CGEventType::KeyDown, 0));
        assert_eq!(ctx.counters.suppressed_keys(), 0);
        assert!(
            writer.try_recv().is_err(),
            "a key was recorded at its arrival"
        );
        on_tap_event(&ctx, event(CGEventType::LeftMouseDown, 0));
        drop(ctx);
        assert_eq!(
            events_until_the_sink_closes(&writer),
            [Modality::Keystroke, Modality::Pointer]
        );
    }
}
