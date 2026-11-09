//! Focused-window title via the Accessibility API (`AXUIElement`).

use std::ffi::c_void;

use core_foundation::base::{CFType, CFTypeRef, TCFType};
use core_foundation::string::{CFString, CFStringRef};

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateApplication(pid: libc::pid_t) -> *const c_void;
    fn AXUIElementCopyAttributeValue(
        element: *const c_void,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> i32;
    fn AXIsProcessTrusted() -> bool;
}

const AX_ERROR_SUCCESS: i32 = 0;

/// Whether Halvo has the Accessibility grant needed to read window titles.
pub fn accessibility_trusted() -> bool {
    // SAFETY: parameterless predicate, no side effects.
    unsafe { AXIsProcessTrusted() }
}

/// Copy one Accessibility attribute of `element` as an owned [`CFType`], or
/// `None` if the attribute is absent. The returned value frees itself on drop.
fn copy_attribute(element: *const c_void, attribute: &str) -> Option<CFType> {
    let key = CFString::new(attribute);
    let mut value: CFTypeRef = std::ptr::null();
    // SAFETY: `element` is a live AX element; `key` outlives the call; on success
    // `value` receives a +1 reference, taken over by the wrap below.
    let err =
        unsafe { AXUIElementCopyAttributeValue(element, key.as_concrete_TypeRef(), &mut value) };
    if err != AX_ERROR_SUCCESS || value.is_null() {
        return None;
    }
    // SAFETY: AX copy APIs follow the Create rule; wrap the +1 ref as an owned
    // CFType whose Drop releases it.
    Some(unsafe { CFType::wrap_under_create_rule(value) })
}

/// The title of the focused window of the application with process id `pid`, or
/// `None` if Accessibility is not granted, the app has no focused window, or that
/// window has no title.
pub fn focused_window_title(pid: libc::pid_t) -> Option<String> {
    if !accessibility_trusted() {
        return None;
    }

    // SAFETY: AXUIElementCreateApplication returns a +1 ref or null.
    let app_ref = unsafe { AXUIElementCreateApplication(pid) };
    if app_ref.is_null() {
        return None;
    }
    // SAFETY: the Create rule gives a +1 ref; own it so Drop releases it.
    let app = unsafe { CFType::wrap_under_create_rule(app_ref) };

    let focused_window = copy_attribute(app.as_CFTypeRef(), "AXFocusedWindow")?;
    let title = copy_attribute(focused_window.as_CFTypeRef(), "AXTitle")?
        .downcast::<CFString>()?
        .to_string();
    if title.is_empty() {
        None
    } else {
        Some(title)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trusted_predicate_does_not_panic() {
        let _ = accessibility_trusted();
    }

    #[test]
    fn title_of_a_bogus_pid_is_none() {
        assert_eq!(focused_window_title(-1), None);
    }
}
