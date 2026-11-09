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
use crate::macos::{emit_metadata_only, ForegroundReader, Unpersisted};
use crate::secure_input_macos::secure_input_active;
use crate::sink::SinkHandle;
use crate::source::{CaptureError, CaptureSource};

// Re-enable a tap the OS disabled (by timeout or by a security event). Declared
// here so the callback can re-enable its own tap without owning the wrapper.
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventTapEnable(tap: *mut c_void, enable: bool);
}

/// A read handle on the count of keystrokes seen while secure input was active.
#[derive(Clone)]
pub struct SecureBlockedCounter(Arc<AtomicU64>);

impl SecureBlockedCounter {
    pub fn load(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}

pub(crate) fn classify_for_record(etype: CGEventType, secure_active: bool) -> Option<Modality> {
    match etype {
        CGEventType::KeyDown | CGEventType::KeyUp | CGEventType::FlagsChanged => {
            if secure_active {
                None
            } else {
                Some(Modality::Keystroke)
            }
        }
        CGEventType::LeftMouseDown
        | CGEventType::LeftMouseUp
        | CGEventType::RightMouseDown
        | CGEventType::RightMouseUp
        | CGEventType::OtherMouseDown
        | CGEventType::OtherMouseUp
        | CGEventType::MouseMoved
        | CGEventType::LeftMouseDragged
        | CGEventType::RightMouseDragged
        | CGEventType::OtherMouseDragged
        | CGEventType::ScrollWheel => Some(Modality::Pointer),
        _ => None,
    }
}

fn events_of_interest() -> Vec<CGEventType> {
    vec![
        CGEventType::KeyDown,
        CGEventType::KeyUp,
        CGEventType::FlagsChanged,
        CGEventType::LeftMouseDown,
        CGEventType::LeftMouseUp,
        CGEventType::RightMouseDown,
        CGEventType::RightMouseUp,
        CGEventType::OtherMouseDown,
        CGEventType::OtherMouseUp,
        CGEventType::MouseMoved,
        CGEventType::LeftMouseDragged,
        CGEventType::RightMouseDragged,
        CGEventType::OtherMouseDragged,
        CGEventType::ScrollWheel,
    ]
}

/// The keystroke + pointer source.
pub struct InputSource {
    foreground: ForegroundReader,
    secure_blocked: Arc<AtomicU64>,
    run_loop: Option<CFRunLoop>,
    thread: Option<JoinHandle<()>>,
}

impl InputSource {
    pub fn new(foreground: ForegroundReader) -> Self {
        Self {
            foreground,
            secure_blocked: Arc::new(AtomicU64::new(0)),
            run_loop: None,
            thread: None,
        }
    }

    pub fn secure_blocked_handle(&self) -> SecureBlockedCounter {
        SecureBlockedCounter(self.secure_blocked.clone())
    }
}

impl CaptureSource for InputSource {
    fn modality(&self) -> Modality {
        Modality::Keystroke
    }

    fn start(&mut self, sink: SinkHandle) -> Result<(), CaptureError> {
        let foreground = self.foreground.clone();
        let secure_blocked = self.secure_blocked.clone();
        let port_slot: Arc<AtomicPtr<c_void>> = Arc::new(AtomicPtr::new(std::ptr::null_mut()));

        let (loop_tx, loop_rx) = channel::<Option<CFRunLoop>>();
        let thread = {
            let port_slot = port_slot.clone();
            std::thread::Builder::new()
                .name("halvo-input-tap".into())
                .spawn(move || {
                    let callback = {
                        let port_slot = port_slot.clone();
                        move |_proxy: core_graphics::event::CGEventTapProxy,
                              etype: CGEventType,
                              _event: &core_graphics::event::CGEvent| {
                            if matches!(
                                etype,
                                CGEventType::TapDisabledByTimeout
                                    | CGEventType::TapDisabledByUserInput
                            ) {
                                let port = port_slot.load(Ordering::Relaxed);
                                if !port.is_null() {
                                    // SAFETY: `port` is this tap's live mach port.
                                    unsafe { CGEventTapEnable(port, true) };
                                }
                                return CallbackResult::Keep;
                            }

                            let secure = secure_input_active();
                            match classify_for_record(etype, secure) {
                                Some(modality) => {
                                    let _ = emit_metadata_only(
                                        &sink,
                                        modality,
                                        foreground.get(),
                                        Unpersisted::EMPTY,
                                    );
                                }
                                None => {
                                    if secure
                                        && matches!(
                                            etype,
                                            CGEventType::KeyDown
                                                | CGEventType::KeyUp
                                                | CGEventType::FlagsChanged
                                        )
                                    {
                                        secure_blocked.fetch_add(1, Ordering::Relaxed);
                                    }
                                }
                            }
                            CallbackResult::Keep
                        }
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
                            let _ = loop_tx.send(None);
                            return;
                        }
                    };

                    port_slot.store(
                        tap.mach_port().as_concrete_TypeRef() as *mut c_void,
                        Ordering::Relaxed,
                    );

                    let source = match tap.mach_port().create_runloop_source(0) {
                        Ok(source) => source,
                        Err(()) => {
                            let _ = loop_tx.send(None);
                            return;
                        }
                    };
                    let run_loop = CFRunLoop::get_current();
                    run_loop.add_source(&source, unsafe { kCFRunLoopCommonModes });
                    tap.enable();
                    let _ = loop_tx.send(Some(run_loop));

                    CFRunLoop::run_current();
                })
                .map_err(|e| CaptureError::Start(format!("tap thread: {e}")))?
        };

        match loop_rx.recv() {
            Ok(Some(run_loop)) => {
                self.run_loop = Some(run_loop);
                self.thread = Some(thread);
                Ok(())
            }
            _ => {
                let _ = thread.join();
                Err(CaptureError::PermissionDenied(Modality::Keystroke))
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
    fn keystrokes_are_suppressed_while_secure_input_is_active() {
        assert_eq!(
            classify_for_record(CGEventType::KeyDown, true),
            None,
            "key down must not record while secure input is active"
        );
        assert_eq!(classify_for_record(CGEventType::KeyUp, true), None);
        assert_eq!(classify_for_record(CGEventType::FlagsChanged, true), None);
    }

    #[test]
    fn keystrokes_record_when_secure_input_is_inactive() {
        assert_eq!(
            classify_for_record(CGEventType::KeyDown, false),
            Some(Modality::Keystroke)
        );
    }

    #[test]
    fn pointer_records_regardless_of_secure_input() {
        for secure in [false, true] {
            assert_eq!(
                classify_for_record(CGEventType::MouseMoved, secure),
                Some(Modality::Pointer)
            );
            assert_eq!(
                classify_for_record(CGEventType::LeftMouseDown, secure),
                Some(Modality::Pointer)
            );
        }
    }

    #[test]
    fn disable_events_are_not_recorded() {
        assert_eq!(
            classify_for_record(CGEventType::TapDisabledByTimeout, false),
            None
        );
        assert_eq!(
            classify_for_record(CGEventType::TapDisabledByUserInput, false),
            None
        );
    }
}
