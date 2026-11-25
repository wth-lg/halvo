#![deny(clippy::let_underscore_must_use)]

use std::ffi::c_void;
use std::fmt;
use std::time::Duration;

use core_foundation::base::{CFRange, CFType, CFTypeID, CFTypeRef, TCFType};
use core_foundation::string::{CFString, CFStringGetCharacters, CFStringGetLength, CFStringRef};

use crate::floor::exclude::PrivateWindow;
use crate::floor::site::Site;

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
    fn AXIsProcessTrusted() -> u8;
}

const AX_ERROR_SUCCESS: i32 = 0;
const AX_ERROR_NO_VALUE: i32 = -25212;
const AX_ERROR_ATTRIBUTE_UNSUPPORTED: i32 = -25205;

pub const TITLE_READ_CAP_UNITS: usize = 1024;

/// The UTF-16 units at the end of a title that the private-window rule reads.
pub const TITLE_ENDING_UNITS: usize = 64;

pub const TITLE_READ_DEADLINE: Duration = crate::macos::foreground::FOREGROUND_POLL;

const HEAD_READ_UNITS: usize = TITLE_READ_CAP_UNITS + 1;

const _: () = assert!(crate::floor::text::TEXT_MAX_BYTES <= TITLE_READ_CAP_UNITS);

/// The bounded read of a window title: at most one UTF-16 unit more than
/// [`TITLE_READ_CAP_UNITS`] from its start and at most [`TITLE_ENDING_UNITS`]
/// from its end, each with an unpaired surrogate read as U+FFFD. A unit takes
/// at least one byte, so the floor's text entry, which keeps at most
/// [`TITLE_READ_CAP_UNITS`] bytes, cuts every title longer than the cap and
/// records the cut. It prints a placeholder.
#[derive(Clone, PartialEq, Eq)]
pub struct TitleText {
    text: String,
    ending: String,
    private_words: Option<&'static str>,
}

impl TitleText {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn into_text(self) -> String {
        self.text
    }

    pub fn ending(&self) -> &str {
        &self.ending
    }

    /// The same title, read from a window of a browser whose private window
    /// ends its title with `words`.
    pub(crate) fn of_a_browser_ending_private_with(self, words: &'static str) -> Self {
        Self {
            private_words: Some(words),
            ..self
        }
    }

    /// Whether the title ends with the words that its browser puts at the end
    /// of a private window's title.
    pub(crate) fn is_private(&self) -> bool {
        self.private_words
            .is_some_and(|words| self.ending.ends_with(words))
    }

    #[cfg(test)]
    pub(crate) fn for_test(text: &str) -> Self {
        let units: Vec<u16> = text.encode_utf16().collect();
        let start = units.len().saturating_sub(TITLE_ENDING_UNITS);
        Self {
            text: lossy(&units[..units.len().min(HEAD_READ_UNITS)]),
            ending: lossy(&units[start..]),
            private_words: None,
        }
    }
}

impl fmt::Debug for TitleText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TitleText(..)")
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

fn lossy(units: &[u16]) -> String {
    char::decode_utf16(units.iter().copied())
        .map(|unit| unit.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

fn units_of(string: CFStringRef, start: usize, take: usize) -> Vec<u16> {
    let mut units = vec![0u16; take];
    // SAFETY: the caller passes a range inside the string's length, and
    // `units` has room for `take` units.
    unsafe {
        CFStringGetCharacters(
            string,
            CFRange::init(start as isize, take as isize),
            units.as_mut_ptr(),
        )
    };
    units
}

fn bounded_title(text: &CFString) -> TitleText {
    let string = text.as_concrete_TypeRef();
    // SAFETY: `string` is a live CFString that `text` holds.
    let total = usize::try_from(unsafe { CFStringGetLength(string) }).unwrap_or(0);
    let head = units_of(string, 0, total.min(HEAD_READ_UNITS));
    let start = total.saturating_sub(TITLE_ENDING_UNITS);
    TitleText {
        text: lossy(&head),
        ending: lossy(&units_of(string, start, total - start)),
        private_words: None,
    }
}

/// The name of the localization of a browser's bundle under which the rules
/// keep its titles: the bundle's `en`, for which [`ENGLISH_ENDINGS`] holds
/// the US-English words of a private window. Whether a bundle's `en` holds
/// those words is each bundle's own.
pub(crate) const ENGLISH: &str = "en";

/// Chrome and Edge, each matched whole, with the words that end the title of
/// its private window in the localization [`ENGLISH`]. Safari is not here: its
/// titles are not read.
pub(crate) const ENGLISH_ENDINGS: [(&str, &str); 2] = [
    ("com.google.Chrome", "(Incognito)"),
    ("com.microsoft.edgemac", "(InPrivate)"),
];

/// Whether `place`, a bundle's place as the system gives it, lies under
/// /Applications, /System/Applications or the Applications folder of `home`,
/// set against those places as text alone: no file is read and no link is
/// followed, and a place with a `..` component lies under none.
pub(crate) fn in_applications(place: &str, home: Option<&str>) -> bool {
    if place.split('/').any(|part| part == "..") {
        return false;
    }
    let mut folders = vec![
        "/Applications/".to_string(),
        "/System/Applications/".to_string(),
    ];
    folders.extend(home.map(|home| format!("{}/Applications/", home.trim_end_matches('/'))));
    folders
        .iter()
        .any(|folder| place.starts_with(folder.as_str()))
}

const BROWSER_BUNDLE_IDS: [&str; 34] = [
    "com.google.Chrome",
    "com.google.chrome.for.testing",
    "com.microsoft.edgemac",
    "org.mozilla.firefox",
    "org.mozilla.nightly",
    "org.torproject.torbrowser",
    "org.chromium.Chromium",
    "org.mozilla.",
    "com.apple.Safari",
    "com.brave.Browser",
    "company.thebrowser.Browser",
    "company.thebrowser.dia",
    "com.operasoftware.Opera",
    "com.vivaldi.Vivaldi",
    "org.torproject.",
    "com.duckduckgo.macos.browser",
    "com.kagi.kagimacOS",
    "org.chromium.",
    "com.pushplaylabs.sidekick",
    "net.waterfox.waterfox",
    "io.gitlab.librewolf-community",
    "app.zen-browser.zen",
    "one.ablaze.floorp",
    "net.mullvad.mullvadbrowser",
    "ru.yandex.desktop.yandex-browser",
    "ai.perplexity.comet",
    "com.sigmaos.sigmaos.macos",
    "com.naver.Whale",
    "com.hiddenreflex.Epic",
    "com.ghostery.dawn",
    "com.avast.browser",
    "com.avg.browser",
    "com.ecosia.macapp",
    "de.iron.Iron",
];

/// What the closed list of browsers says of an app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Listed {
    /// One of [`ENGLISH_ENDINGS`], matched whole: the browser and its words.
    Ending(&'static str, &'static str),
    /// Named by a full id of the list, matched as the head of the app's id
    /// (so com.google.Chrome.canary counts): a browser whatever the system
    /// lists.
    Browser,
    /// Under a whole domain of the list alone: a browser unless the system's
    /// list was read and does not name it.
    Domain,
    /// Not on the list.
    Other,
}

pub(crate) fn listed(bundle_id: &str) -> Listed {
    if let Some(&(browser, words)) = ENGLISH_ENDINGS
        .iter()
        .find(|(browser, _)| bundle_id.eq_ignore_ascii_case(browser))
    {
        return Listed::Ending(browser, words);
    }
    let mut domain = false;
    for entry in BROWSER_BUNDLE_IDS {
        let named = bundle_id
            .get(..entry.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(entry));
        match (named, entry.ends_with('.')) {
            (true, false) => return Listed::Browser,
            (true, true) => domain = true,
            (false, _) => {}
        }
    }
    if domain {
        Listed::Domain
    } else {
        Listed::Other
    }
}

/// `Private` for an app that a full id of the closed list names, other than
/// the three of [`ENGLISH_ENDINGS`]: its titles are not read, whatever the
/// system says. Every other app's titles are left to the title rules.
pub(crate) fn window_privacy(bundle_id: Option<&str>) -> PrivateWindow {
    if bundle_id.is_some_and(|id| listed(id) == Listed::Browser) {
        PrivateWindow::Private
    } else {
        PrivateWindow::Unknown
    }
}

pub(crate) fn site_of(_pid: libc::pid_t) -> Option<Site> {
    #[cfg(feature = "r2")]
    if let Some(address) = crate::daemon::r2::site_reader(_pid) {
        return Site::cut(address);
    }
    None
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

fn window_title(
    window: &CFType,
    copy: impl FnOnce(&CFType) -> Result<Option<CFType>, TitleError>,
) -> Result<Option<TitleText>, TitleError> {
    if window.type_of() != ax_element_type() {
        return Err(TitleError::NotAnElement);
    }
    let Some(title) = copy(window)? else {
        return Ok(None);
    };
    let title = title.downcast::<CFString>().ok_or(TitleError::NotAString)?;
    Ok(a_title(bounded_title(&title)))
}

fn a_title(title: TitleText) -> Option<TitleText> {
    (!title.text.is_empty()).then_some(title)
}

fn application_element(pid: libc::pid_t) -> Result<CFType, TitleError> {
    // SAFETY: AXUIElementCreateApplication returns a +1 reference or null.
    let app_ref = unsafe { AXUIElementCreateApplication(pid) };
    if app_ref.is_null() {
        return Err(TitleError::NoApplicationElement);
    }
    // SAFETY: the Create rule gives a +1 reference.
    Ok(unsafe { CFType::wrap_under_create_rule(app_ref) })
}

fn focused_title_of(
    app: &CFType,
    mut copy: impl FnMut(&CFType, &'static str) -> Result<Option<CFType>, TitleError>,
) -> Result<Option<TitleText>, TitleError> {
    if let Some(app) = AxElement::checked(app) {
        set_deadline(app);
    }
    let Some(window) = copy(app, "AXFocusedWindow")? else {
        return Ok(None);
    };
    if let Some(window) = AxElement::checked(&window) {
        set_deadline(window);
    }
    window_title(&window, |window| copy(window, "AXTitle"))
}

/// The title of the focused window of the application with process id `pid`.
/// `Ok(None)` means that the app has no focused window or that the window has
/// no title.
pub fn focused_window_title(pid: libc::pid_t) -> Result<Option<TitleText>, TitleError> {
    if !accessibility_trusted() {
        return Err(TitleError::NotTrusted);
    }
    let app = application_element(pid)?;
    focused_title_of(&app, |element, attribute| {
        copy_attribute(element, attribute)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_foundation::base::kCFAllocatorDefault;
    use core_foundation::string::CFStringCreateWithCharacters;
    use std::cell::{Cell, RefCell};

    #[test]
    fn an_empty_title_is_no_title() {
        assert_eq!(a_title(TitleText::for_test("")), None);
        assert_eq!(
            a_title(TitleText::for_test("a")),
            Some(TitleText::for_test("a"))
        );
    }

    #[test]
    fn a_string_is_no_accessibility_element_and_gets_no_deadline() {
        let not_an_element = CFString::new("AXWindow").as_CFType();
        assert!(AxElement::checked(&not_an_element).is_none());
    }

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
    fn a_title_with_an_unpaired_surrogate_reads_it_as_the_replacement_character() {
        let title = bounded_title(&utf16_string(&[0x41, 0xD800, 0x42]));
        assert_eq!(title.text(), "A\u{FFFD}B");
        assert_eq!(
            bounded_title(&utf16_string(&[0xD83D, 0xDE00, 0x00E9])).text(),
            "\u{1F600}\u{E9}"
        );
    }

    #[test]
    fn a_title_over_the_cap_reads_one_unit_past_the_cap_from_its_start() {
        let mut units = vec![u16::from(b'a'); TITLE_READ_CAP_UNITS + 976];
        units.extend("Page - Google Chrome (Incognito)".encode_utf16());
        let title = bounded_title(&utf16_string(&units));
        assert_eq!(title.text(), "a".repeat(TITLE_READ_CAP_UNITS + 1));
    }

    #[test]
    fn a_title_of_2000_ascii_units_from_the_bounded_read_enters_the_floor_as_cut() {
        let title = bounded_title(&utf16_string(&vec![u16::from(b'a'); 2_000]));
        let entered = crate::floor::payload::Title::new(
            title.into_text(),
            crate::floor::payload::TitleWhy::AppChange,
            None,
        );
        assert_eq!(
            (entered.text.heap_bytes(), entered.text.was_cut()),
            (1024, true)
        );
    }

    #[test]
    fn a_string_passed_as_the_window_gives_an_error_and_no_accessibility_call() {
        let calls = Cell::new(0);
        let not_an_element = CFString::new("AXWindow").as_CFType();
        let read = window_title(&not_an_element, |_| {
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

    #[test]
    fn ten_title_reads_through_an_element_of_this_process_ask_only_for_the_focused_window_and_its_title(
    ) {
        let own = libc::pid_t::try_from(std::process::id()).expect("a pid");
        let app = application_element(own).expect("an element of this process");
        let asked = RefCell::new(Vec::new());
        for _ in 0..10 {
            let read = focused_title_of(&app, |element, attribute| {
                asked.borrow_mut().push(attribute);
                match attribute {
                    "AXFocusedWindow" => Ok(Some(element.clone())),
                    _ => Ok(Some(CFString::new("Inbox").as_CFType())),
                }
            });
            assert_eq!(
                read.map(|title| title.map(TitleText::into_text)),
                Ok(Some("Inbox".into()))
            );
        }
        let asked = asked.into_inner();
        assert_eq!(asked.len(), 20);
        assert!(
            asked
                .iter()
                .all(|&attribute| attribute == "AXFocusedWindow" || attribute == "AXTitle"),
            "{asked:?}"
        );
    }

    #[test]
    fn each_browser_of_the_closed_list_reads_private_and_no_other_app_does() {
        for browser in [
            "com.google.Chrome.canary",
            "com.microsoft.edgemac.Dev",
            "com.apple.SafariTechnologyPreview",
            "com.brave.Browser.beta",
            "company.thebrowser.Browser",
            "com.operasoftware.OperaGX",
            "com.vivaldi.Vivaldi",
            "com.duckduckgo.macos.browser",
            "com.kagi.kagimacOS",
        ] {
            assert_eq!(
                window_privacy(Some(browser)),
                PrivateWindow::Private,
                "{browser}"
            );
        }
        for other in [Some("com.example.editor"), Some("com.apple.TextEdit"), None] {
            assert_eq!(window_privacy(other), PrivateWindow::Unknown, "{other:?}");
        }
    }

    #[test]
    fn a_chrome_title_is_kept_only_where_it_ends_with_its_own_name_and_no_edge_title_is_kept() {
        let kept = |id: &str, title: &str| match listed(id) {
            Listed::Ending(_, words) => !TitleText::for_test(title)
                .of_a_browser_ending_private_with(words)
                .is_private(),
            _ => false,
        };
        let chrome = "com.google.Chrome";
        let edge = "com.microsoft.edgemac";
        let cases = [
            ("ordinary", chrome, "Inbox - Google Chrome", true),
            (
                "sound",
                chrome,
                "Inbox - Audio playing - Google Chrome",
                true,
            ),
            ("pop-up", chrome, "Sign in - Google Chrome", true),
            ("edge-ordinary", edge, "Inbox - Microsoft Edge", false),
            ("profiles", chrome, "Inbox - Google Chrome - Work", false),
            (
                "incognito",
                chrome,
                "Inbox - Google Chrome (Incognito)",
                false,
            ),
            ("guest", chrome, "Inbox - Google Chrome (Guest)", false),
            (
                "isolated",
                chrome,
                "Inbox - Google Chrome (Isolated)",
                false,
            ),
            (
                "incognito-pop-up",
                chrome,
                "Sign in - Google Chrome (Incognito)",
                false,
            ),
            ("app-window", chrome, "Docs", false),
            ("dialog", chrome, "example.com says", false),
            (
                "dialog-of-a-frame",
                chrome,
                "An embedded page at example.com says",
                false,
            ),
            ("dialog-of-a-data-page", chrome, "This page says", false),
            ("beta-ending", chrome, "Inbox - Google Chrome Beta", false),
            (
                "page-named-like-the-browser",
                chrome,
                "Notes - Google Chrome - Google Chrome (Incognito)",
                false,
            ),
            (
                "edge-bracketed-marker",
                edge,
                "Inbox - [InPrivate] - Microsoft Edge",
                false,
            ),
            (
                "edge-inprivate",
                edge,
                "Inbox - Microsoft Edge (InPrivate)",
                false,
            ),
            (
                "edge-profile",
                edge,
                "Inbox - Microsoft Edge - Personal",
                false,
            ),
        ];
        let wrong: Vec<&str> = cases
            .iter()
            .filter(|(_, id, title, wanted)| kept(id, title) != *wanted)
            .map(|(case, ..)| *case)
            .collect();
        assert!(
            wrong.is_empty(),
            "told against the browser's own name: {wrong:?}"
        );
    }

    #[test]
    fn the_site_reader_point_gives_no_site_without_the_feature() {
        assert!(site_of(1).is_none());
    }
}
