//! Focused-window title via the Accessibility API (`AXUIElement`).
//!
//! On each foreground change the foreground source reads the newly focused app's
//! focused-window title through `AXUIElementCopyAttributeValue`. This needs the
//! Accessibility grant; without it the read returns `None` and no window-title
//! event is emitted, which is a missing signal, never a crash.
//!
//! Titles are read only on a change event, not polled, so this is event-triggered
//! as the plan requires.

use std::ffi::c_void;

use core_foundation::base::TCFType;
use core_foundation::string::{CFString, CFStringRef};

// AXUIElement lives in the ApplicationServices umbrella (HIServices). The element
// refs are CoreFoundation objects, released through `CFRelease`.
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateApplication(pid: libc::pid_t) -> *const c_void;
    fn AXUIElementCopyAttributeValue(
        element: *const c_void,
        attribute: CFStringRef,
        value: *mut *const c_void,
    ) -> i32;
    fn AXIsProcessTrusted() -> bool;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: *const c_void);
}

const AX_ERROR_SUCCESS: i32 = 0;

/// Whether Halvo has the Accessibility grant needed to read window titles.
pub fn accessibility_trusted() -> bool {
    // SAFETY: parameterless predicate, no side effects.
    unsafe { AXIsProcessTrusted() }
}

/// Copy one AX attribute whose value is itself an AX/CF object, returning the raw
/// owned ref (caller releases) or null.
fn copy_attribute(element: *const c_void, attribute: &str) -> *const c_void {
    let key = CFString::new(attribute);
    let mut value: *const c_void = std::ptr::null();
    // SAFETY: `element` is a live AX element; `key` outlives the call; `value`
    // receives a +1 ref on success.
    let err =
        unsafe { AXUIElementCopyAttributeValue(element, key.as_concrete_TypeRef(), &mut value) };
    if err != AX_ERROR_SUCCESS {
        std::ptr::null()
    } else {
        value
    }
}

/// The title of the focused window of the application with process id `pid`, or
/// `None` if Accessibility is not granted, the app has no focused window, or that
/// window has no title.
pub fn focused_window_title(pid: libc::pid_t) -> Option<String> {
    if !accessibility_trusted() {
        return None;
    }

    // SAFETY: AXUIElementCreateApplication returns a +1 ref or null.
    let app = unsafe { AXUIElementCreateApplication(pid) };
    if app.is_null() {
        return None;
    }

    let focused_window = copy_attribute(app, "AXFocusedWindow");
    // SAFETY: `app` is a +1 ref we own.
    unsafe { CFRelease(app) };
    if focused_window.is_null() {
        return None;
    }

    let title_ref = copy_attribute(focused_window, "AXTitle");
    // SAFETY: `focused_window` is a +1 ref we own.
    unsafe { CFRelease(focused_window) };
    if title_ref.is_null() {
        return None;
    }

    // The title value is a CFString; take ownership of the +1 ref and read it.
    // SAFETY: `title_ref` is a live, owned CFStringRef.
    let title = unsafe { CFString::wrap_under_create_rule(title_ref as CFStringRef) };
    let text = title.to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trusted_predicate_does_not_panic() {
        // Reading the trust state is always safe; the value depends on the grant.
        let _ = accessibility_trusted();
    }

    #[test]
    fn title_of_a_bogus_pid_is_none() {
        // A pid that owns no AX-reachable app yields no title, never a crash.
        // (Also returns None early when Accessibility is not granted.)
        assert_eq!(focused_window_title(-1), None);
    }
}
