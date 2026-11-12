//! Keystroke and pointer capture via one listen-only `CGEventTap`.

use std::ffi::c_void;
use std::sync::atomic::{AtomicPtr, AtomicU64, Ordering};
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::thread::JoinHandle;

use core_foundation::base::TCFType;
use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
use core_graphics::event::{
    CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
    CallbackResult,
};

use crate::event::Modality;
use crate::macos::foreground::{emit_metadata_only, ForegroundReader, Unpersisted};
use crate::secure_input::SecureInputState;
use crate::secure_input_macos::secure_input_active;
use crate::sink::SinkHandle;
use crate::source::{CaptureError, CaptureSource};

// Re-enable a tap the OS disabled (by timeout or by a security event). Declared
// here so the callback can re-enable its own tap without owning the wrapper.
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventTapEnable(tap: *mut c_void, enable: bool);
}

/// The live mach port of this source's tap, shared so the callback can re-enable
/// a tap the OS disables without owning the `CGEventTap` wrapper.
#[derive(Clone)]
struct TapPort(Arc<AtomicPtr<c_void>>);

impl TapPort {
    fn unset() -> Self {
        Self(Arc::new(AtomicPtr::new(std::ptr::null_mut())))
    }

    fn store(&self, port: *mut c_void) {
        self.0.store(port, Ordering::Relaxed);
    }

    /// Re-enable the tap, if its port is live. A disabled tap stays dead until
    /// re-enabled, so the callback calls this on every disable event.
    fn reenable(&self) {
        let port = self.0.load(Ordering::Relaxed);
        if !port.is_null() {
            // SAFETY: `port` is this tap's live mach port. It is stored only
            // after `CGEventTap::new` succeeds; callbacks run only on the tap
            // thread's run loop, which stops before `tap` drops.
            unsafe { CGEventTapEnable(port, true) };
        }
    }
}

/// The count of keystrokes the tap saw while secure input was active. The
/// product guarantee is that this is always zero: a non-zero value is one or
/// more keystrokes the OS should have withheld but the tap recorded.
#[derive(Clone)]
pub struct SecureInputViolations(Arc<AtomicU64>);

/// A non-zero [`SecureInputViolations`] count: the number of keystrokes seen
/// under secure input, which must never happen.
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

/// What the tap does with one event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Classification {
    /// Record an event of this modality.
    Record(Modality),
    /// A key event under secure input: record nothing, count a violation.
    SecureSuppressed,
    /// An event the tap does not record.
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

/// What the tap callback needs to turn one OS event into an emit or a violation.
struct TapContext {
    sink: SinkHandle,
    foreground: ForegroundReader,
    violations: SecureInputViolations,
    port: TapPort,
}

/// Classify one event and act: emit a metadata-only event, count a secure-input
/// violation, or ignore it. The secure-input state is read once and passed in,
/// so "key event under secure input" is decided in exactly one place.
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

fn current_secure_state() -> SecureInputState {
    if secure_input_active() {
        SecureInputState::Active
    } else {
        SecureInputState::Inactive
    }
}

/// The keystroke + pointer source.
pub struct InputSource {
    foreground: ForegroundReader,
    violations: SecureInputViolations,
    run_loop: Option<CFRunLoop>,
    thread: Option<JoinHandle<()>>,
}

impl InputSource {
    pub fn new(foreground: ForegroundReader) -> Self {
        Self {
            foreground,
            violations: SecureInputViolations::new(),
            run_loop: None,
            thread: None,
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
        };
        let port = ctx.port.clone();

        let (loop_tx, loop_rx) = channel::<Result<CFRunLoop, CaptureError>>();
        let thread = std::thread::Builder::new()
            .name("halvo-input-tap".into())
            .spawn(move || {
                let callback =
                    move |_proxy: core_graphics::event::CGEventTapProxy,
                          etype: CGEventType,
                          _event: &core_graphics::event::CGEvent| {
                        if is_tap_disabled(etype) {
                            ctx.port.reenable();
                            return CallbackResult::Keep;
                        }
                        dispatch_tap_event(&ctx, etype, current_secure_state());
                        CallbackResult::Keep
                    };

                let tap = match CGEventTap::new(
                    CGEventTapLocation::HID,
                    CGEventTapPlacement::HeadInsertEventTap,
                    CGEventTapOptions::ListenOnly,
                    events_of_interest(),
                    callback,
                ) {
                    Ok(tap) => tap,
                    Err(()) => {
                        let _ =
                            loop_tx.send(Err(CaptureError::PermissionDenied(Modality::Keystroke)));
                        return;
                    }
                };

                port.store(tap.mach_port().as_concrete_TypeRef() as *mut c_void);

                let source = match tap.mach_port().create_runloop_source(0) {
                    Ok(source) => source,
                    Err(()) => {
                        let _ = loop_tx.send(Err(CaptureError::Start(
                            "could not create run loop source".into(),
                        )));
                        return;
                    }
                };
                let run_loop = CFRunLoop::get_current();
                // SAFETY: `kCFRunLoopCommonModes` is a constant CFString that
                // CoreFoundation exports and never writes; reading it has no effect.
                run_loop.add_source(&source, unsafe { kCFRunLoopCommonModes });
                tap.enable();
                let _ = loop_tx.send(Ok(run_loop));

                CFRunLoop::run_current();
            })
            .map_err(|e| CaptureError::Start(format!("tap thread: {e}")))?;

        match loop_rx.recv() {
            Ok(Ok(run_loop)) => {
                self.run_loop = Some(run_loop);
                self.thread = Some(thread);
                Ok(())
            }
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

    fn stop(&mut self) {
        if let Some(run_loop) = self.run_loop.take() {
            run_loop.stop();
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
