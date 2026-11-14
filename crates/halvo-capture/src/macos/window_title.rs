use std::ffi::c_void;
use std::time::Duration;

use core_foundation::base::{CFRange, CFType, CFTypeID, CFTypeRef, TCFType};
use core_foundation::string::{CFString, CFStringGetCharacters, CFStringGetLength, CFStringRef};

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateApplication(pid: libc::pid_t) -> *const c_void;
    fn AXUIElementCopyAttributeValue(
        element: *const c_void,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> i32;
    fn AXUIElementSetMessagingTimeout(element: *const c_void, seconds: f32) -> i32;
    fn AXUIElementGetTypeID() -> CFTypeID;
    #[allow(clashing_extern_declarations)]
    fn AXIsProcessTrusted() -> u8;
}

const AX_ERROR_SUCCESS: i32 = 0;
const AX_ERROR_NO_VALUE: i32 = -25212;
const AX_ERROR_ATTRIBUTE_UNSUPPORTED: i32 = -25205;

pub const TITLE_READ_CAP_UNITS: usize = 1024;

pub const TITLE_READ_DEADLINE: Duration = crate::macos::foreground::FOREGROUND_POLL;

/// The UTF-8 length of the bounded read of a title, with each unpaired
/// surrogate counted as the 3 bytes of U+FFFD.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TitleLength(u64);

impl TitleLength {
    pub fn bytes(self) -> u64 {
        self.0
    }

    #[cfg(test)]
    pub(crate) const fn for_test(bytes: u64) -> Self {
        Self(bytes)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleError {
    NotTrusted,
    Accessibility {
        code: i32,
    },
    NotAnElement,
    /// The system made no Accessibility element for the process id.
    NoApplicationElement,
    NotAString,
    ReaderPanicked,
}

pub fn accessibility_trusted() -> bool {
    // SAFETY: parameterless predicate with no side effects; the header
    // declares it as returning Boolean, an unsigned char.
    unsafe { AXIsProcessTrusted() != 0 }
}

pub(crate) fn lossy_utf8_len(units: &[u16]) -> u64 {
    char::decode_utf16(units.iter().copied())
        .map(|unit| unit.unwrap_or(char::REPLACEMENT_CHARACTER).len_utf8() as u64)
        .sum()
}

fn bounded_length(text: &CFString) -> TitleLength {
    let string = text.as_concrete_TypeRef();
    // SAFETY: `string` is a live CFString that `text` holds.
    let total = unsafe { CFStringGetLength(string) };
    let take = usize::try_from(total)
        .unwrap_or(0)
        .min(TITLE_READ_CAP_UNITS);
    let mut units = vec![0u16; take];
    // SAFETY: the range starts at 0 and holds `take` units, at most the
    // string's length, and `units` has room for `take` units.
    unsafe { CFStringGetCharacters(string, CFRange::init(0, take as isize), units.as_mut_ptr()) };
    TitleLength(lossy_utf8_len(&units))
}

fn copy_attribute(element: &CFType, attribute: &str) -> Result<Option<CFType>, TitleError> {
    if element.type_of() != ax_element_type() {
        return Err(TitleError::NotAnElement);
    }
    let key = CFString::new(attribute);
    let mut value: CFTypeRef = std::ptr::null();
    // SAFETY: `element` is a live AXUIElement, checked by its type id above;
    // `key` outlives the call.
    let code = unsafe {
        AXUIElementCopyAttributeValue(
            element.as_CFTypeRef(),
            key.as_concrete_TypeRef(),
            &mut value,
        )
    };
    match code {
        AX_ERROR_SUCCESS if !value.is_null() => {
            // SAFETY: AX copy APIs follow the Create rule.
            Ok(Some(unsafe { CFType::wrap_under_create_rule(value) }))
        }
        AX_ERROR_SUCCESS | AX_ERROR_NO_VALUE | AX_ERROR_ATTRIBUTE_UNSUPPORTED => Ok(None),
        code => Err(TitleError::Accessibility { code }),
    }
}

fn ax_element_type() -> CFTypeID {
    // SAFETY: parameterless getter of a constant type id.
    unsafe { AXUIElementGetTypeID() }
}

struct AxElement<'a>(&'a CFType);

impl<'a> AxElement<'a> {
    fn checked(value: &'a CFType) -> Option<Self> {
        (value.type_of() == ax_element_type()).then_some(Self(value))
    }
}

fn set_deadline(element: AxElement<'_>) {
    let element = element.0;
    // SAFETY: `element` is a live AXUIElement for the call: an AxElement is
    // made only from a value whose type id is AXUIElement's.
    unsafe {
        AXUIElementSetMessagingTimeout(element.as_CFTypeRef(), TITLE_READ_DEADLINE.as_secs_f32())
    };
}

fn window_title_length(
    window: &CFType,
    copy: impl FnOnce(&CFType) -> Result<Option<CFType>, TitleError>,
) -> Result<Option<TitleLength>, TitleError> {
    if window.type_of() != ax_element_type() {
        return Err(TitleError::NotAnElement);
    }
    let Some(title) = copy(window)? else {
        return Ok(None);
    };
    let title = title.downcast::<CFString>().ok_or(TitleError::NotAString)?;
    Ok(a_title(bounded_length(&title)))
}

fn a_title(length: TitleLength) -> Option<TitleLength> {
    (length.bytes() > 0).then_some(length)
}

/// The length of the title of the focused window of the application with
/// process id `pid`. `Ok(None)` means that the app has no focused window or that
/// the window has no title.
pub fn focused_window_title(pid: libc::pid_t) -> Result<Option<TitleLength>, TitleError> {
    if !accessibility_trusted() {
        return Err(TitleError::NotTrusted);
    }
    // SAFETY: AXUIElementCreateApplication returns a +1 reference or null.
    let app_ref = unsafe { AXUIElementCreateApplication(pid) };
    if app_ref.is_null() {
        return Err(TitleError::NoApplicationElement);
    }
    // SAFETY: the Create rule gives a +1 reference.
    let app = unsafe { CFType::wrap_under_create_rule(app_ref) };
    if let Some(app) = AxElement::checked(&app) {
        set_deadline(app);
    }
    let Some(window) = copy_attribute(&app, "AXFocusedWindow")? else {
        return Ok(None);
    };
    if let Some(window) = AxElement::checked(&window) {
        set_deadline(window);
    }
    window_title_length(&window, |window| copy_attribute(window, "AXTitle"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_title_is_no_title() {
        assert_eq!(a_title(TitleLength::for_test(0)), None);
        assert_eq!(
            a_title(TitleLength::for_test(1)),
            Some(TitleLength::for_test(1))
        );
    }

    #[test]
    fn a_string_is_no_accessibility_element_and_gets_no_deadline() {
        let not_an_element = CFString::new("AXWindow").as_CFType();
        assert!(AxElement::checked(&not_an_element).is_none());
    }
    use core_foundation::base::kCFAllocatorDefault;
    use core_foundation::string::CFStringCreateWithCharacters;
    use std::cell::Cell;

    fn utf16_string(units: &[u16]) -> CFString {
        // SAFETY: `units` holds `units.len()` UTF-16 units for the call; the
        // create call copies them and returns a +1 reference.
        unsafe {
            CFString::wrap_under_create_rule(CFStringCreateWithCharacters(
                kCFAllocatorDefault,
                units.as_ptr(),
                units.len() as isize,
            ))
        }
    }

    #[test]
    fn a_title_with_an_unpaired_surrogate_has_length_5_and_no_panic() {
        let title = utf16_string(&[0x41, 0xD800, 0x42]);
        assert_eq!(bounded_length(&title), TitleLength(5));
    }

    #[test]
    fn a_title_over_the_cap_reports_the_cap() {
        let units = vec![u16::from(b'a'); TITLE_READ_CAP_UNITS + 976];
        assert_eq!(
            bounded_length(&utf16_string(&units)),
            TitleLength(TITLE_READ_CAP_UNITS as u64)
        );
    }

    #[test]
    fn the_lossy_length_counts_each_unpaired_surrogate_as_three_bytes() {
        assert_eq!(lossy_utf8_len(&[]), 0);
        assert_eq!(lossy_utf8_len(&[0x41, 0xD800, 0x42]), 5);
        assert_eq!(lossy_utf8_len(&[0xDC00]), 3);
        assert_eq!(lossy_utf8_len(&[0xD83D, 0xDE00]), 4);
        assert_eq!(lossy_utf8_len(&[0x00E9]), 2);
    }

    #[test]
    fn a_string_passed_as_the_window_gives_an_error_and_no_accessibility_call() {
        let calls = Cell::new(0);
        let not_an_element = CFString::new("AXWindow").as_CFType();
        let read = window_title_length(&not_an_element, |_| {
            calls.set(calls.get() + 1);
            Ok(None)
        });
        assert_eq!((read, calls.get()), (Err(TitleError::NotAnElement), 0));
    }

    #[test]
    fn the_read_of_a_bogus_pid_gives_no_title() {
        assert!(!matches!(focused_window_title(-1), Ok(Some(_))));
    }

    #[test]
    fn the_deadline_is_one_poll_interval_and_the_cap_1024_units() {
        assert_eq!(TITLE_READ_DEADLINE, Duration::from_millis(400));
        assert_eq!(TITLE_READ_CAP_UNITS, 1024);
    }
}
