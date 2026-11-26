//! What the title rules read of the system about browsers: the places of the
//! apps that the system lists as handlers of https links, the place of a
//! running app, the AppleLanguages values of a browser and of the person, the
//! machine's tick count, a process's start in those ticks, and the localization
//! that a browser's bundle takes. No read of the list or of a place opens a
//! file; only the localization reads a bundle, which the rules ask for alone
//! for a bundle under an Applications folder. Each read runs on the title
//! thread (`halvo-window-title`) alone, never on the tap thread, through the
//! seams of the title rules in [`crate::macos::foreground`], which a test
//! replaces.

#![deny(clippy::let_underscore_must_use)]

use std::ffi::c_void;

use core_foundation::array::{CFArray, CFArrayRef};
use core_foundation::base::{CFType, CFTypeRef, TCFType};
use core_foundation::bundle::{
    CFBundleCopyLocalizationsForPreferences, CFBundleCopyLocalizationsForURL,
};
use core_foundation::string::{CFString, CFStringRef};
use core_foundation::url::CFURL;
use objc2::rc::autoreleasepool;
use objc2_app_kit::{NSRunningApplication, NSWorkspace};
use objc2_foundation::{NSString, NSURL};

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFPreferencesAnyApplication: CFStringRef;
    static kCFPreferencesCurrentUser: CFStringRef;
    static kCFPreferencesAnyHost: CFStringRef;
    fn CFPreferencesAppSynchronize(application_id: CFStringRef) -> u8;
    fn CFPreferencesSynchronize(
        application_id: CFStringRef,
        user_name: CFStringRef,
        host_name: CFStringRef,
    ) -> u8;
    fn CFPreferencesCopyAppValue(key: CFStringRef, application_id: CFStringRef) -> CFTypeRef;
    fn CFPreferencesCopyValue(
        key: CFStringRef,
        application_id: CFStringRef,
        user_name: CFStringRef,
        host_name: CFStringRef,
    ) -> CFTypeRef;
    fn CFPreferencesCopyKeyList(
        application_id: CFStringRef,
        user_name: CFStringRef,
        host_name: CFStringRef,
    ) -> CFArrayRef;
}

extern "C" {
    fn mach_absolute_time() -> u64;
    fn proc_pid_rusage(
        pid: libc::c_int,
        flavor: libc::c_int,
        buffer: *mut *mut c_void,
    ) -> libc::c_int;
}

/// An AppleLanguages value as read: the read failed, the domain holds no
/// value, or the list of languages that it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Languages {
    Failed,
    NoValue,
    List(Vec<String>),
}

/// The reads of a browser's language and of the app in front: one seam,
/// which a test replaces.
pub(crate) trait LanguageReads: Send {
    /// The AppleLanguages value that the browser `bundle_id` reads: its own
    /// where it has one, else what the system's search of its domains gives.
    fn own(&mut self, bundle_id: &str) -> Languages;

    /// The person's global AppleLanguages list.
    fn global(&mut self) -> Languages;

    /// The machine's tick count now, which no change of the clock moves.
    fn now(&mut self) -> u64;

    /// The start of the process `pid` in the ticks of [`LanguageReads::now`];
    /// `None` when it cannot be read.
    fn started(&mut self, pid: libc::pid_t) -> Option<u64>;

    /// The place of the bundle of the running app `pid` whose bundle id is
    /// `bundle_id`, as the system gives it, with no read of a file; `None`
    /// when it cannot be read or the id differs.
    fn place(&mut self, pid: libc::pid_t, bundle_id: &str) -> Option<String>;

    /// The first localization of the bundle at `place` that the system's own
    /// function picks under `languages`; it reads that bundle. `None` when it
    /// cannot be read.
    fn localization(&mut self, place: &str, languages: &[String]) -> Option<String>;

    /// The code of the one language file of the bundle at `place` that the
    /// process `pid` holds open: the `<code>` of its `<code>.lproj`. `None`
    /// when the read gives no answer: the bundle or the process cannot be
    /// read, or the process holds none of the bundle's language files open,
    /// or more than one.
    fn held_language(&mut self, pid: libc::pid_t, place: &str) -> Option<String>;
}

/// The reads of the system itself.
pub(crate) struct SystemLanguages;

impl LanguageReads for SystemLanguages {
    fn own(&mut self, bundle_id: &str) -> Languages {
        let domain = CFString::new(bundle_id);
        // SAFETY: `domain` is a live CFString for the call. Its answer tells
        // whether this process's own changes were written, and it has none,
        // so the answer is not read.
        unsafe { CFPreferencesAppSynchronize(domain.as_concrete_TypeRef()) };
        if !holds_a_key(domain.as_concrete_TypeRef()) {
            return Languages::Failed;
        }
        let key = apple_languages();
        // SAFETY: `key` and `domain` are live CFStrings for the call, which
        // follows the Create rule.
        languages_of(unsafe {
            CFPreferencesCopyAppValue(key.as_concrete_TypeRef(), domain.as_concrete_TypeRef())
        })
    }

    fn global(&mut self) -> Languages {
        // SAFETY: a constant CFString of the framework, which lives as long as
        // the process.
        let domain = unsafe { kCFPreferencesAnyApplication };
        // SAFETY: the three are constant CFStrings of the framework. Its
        // answer tells whether this process's own changes were written, and it
        // has none, so the answer is not read.
        unsafe {
            CFPreferencesSynchronize(domain, kCFPreferencesCurrentUser, kCFPreferencesAnyHost)
        };
        if !holds_a_key(domain) {
            return Languages::Failed;
        }
        let key = apple_languages();
        // SAFETY: `key` is a live CFString and the other three are constant
        // CFStrings of the framework; the call follows the Create rule.
        languages_of(unsafe {
            CFPreferencesCopyValue(
                key.as_concrete_TypeRef(),
                domain,
                kCFPreferencesCurrentUser,
                kCFPreferencesAnyHost,
            )
        })
    }

    fn now(&mut self) -> u64 {
        // SAFETY: a parameterless read of the machine's tick count.
        unsafe { mach_absolute_time() }
    }

    fn started(&mut self, pid: libc::pid_t) -> Option<u64> {
        let mut info = std::mem::MaybeUninit::<libc::rusage_info_v0>::zeroed();
        // SAFETY: `info` is writable for a whole rusage_info_v0, the struct of
        // the flavor RUSAGE_INFO_V0, which is all that the call writes.
        let answer = unsafe {
            proc_pid_rusage(
                pid,
                libc::RUSAGE_INFO_V0,
                info.as_mut_ptr().cast::<*mut c_void>(),
            )
        };
        if answer != 0 {
            return None;
        }
        // SAFETY: the call answered 0, so it wrote the whole value; the
        // buffer was zeroed before.
        let start = unsafe { info.assume_init() }.ri_proc_start_abstime;
        (start != 0).then_some(start)
    }

    fn place(&mut self, pid: libc::pid_t, bundle_id: &str) -> Option<String> {
        autoreleasepool(|_| -> Option<String> {
            let app = NSRunningApplication::runningApplicationWithProcessIdentifier(pid)?;
            let id = app.bundleIdentifier()?;
            if crate::macos::foreground::bundle_identifier(&id) != bundle_id {
                return None;
            }
            let bundle = app.bundleURL()?;
            place_of(&bundle)
        })
    }

    fn localization(&mut self, place: &str, languages: &[String]) -> Option<String> {
        let bundle = CFURL::from_path(place, true)?;
        first_localization(&bundle, languages)
    }

    fn held_language(&mut self, _pid: libc::pid_t, _place: &str) -> Option<String> {
        None
    }
}

/// The places of the apps that the system lists as handlers of https links,
/// as it gives them, with no read of a file; `None` when the system names
/// none: an https link always has a handler, so an empty list is a failed
/// read.
pub(crate) fn https_handlers() -> Option<Vec<String>> {
    autoreleasepool(|_| {
        let link = NSURL::URLWithString(&NSString::from_str("https://localhost/"))?;
        let apps = NSWorkspace::sharedWorkspace().URLsForApplicationsToOpenURL(&link);
        let places: Vec<String> = apps.iter().filter_map(|app| place_of(&app)).collect();
        (!places.is_empty()).then_some(places)
    })
}

/// The path of a file URL, as NSURL gives it, with no read of a file.
fn place_of(url: &NSURL) -> Option<String> {
    url.path().map(|path| path.to_string())
}

fn apple_languages() -> CFString {
    CFString::from_static_string("AppleLanguages")
}

/// Whether the domain `domain` of the user, any host, gave at least one key:
/// the proof that it was read, since a value that it lacks and a domain that
/// cannot be read give the same NULL.
fn holds_a_key(domain: CFStringRef) -> bool {
    // SAFETY: `domain` is a live CFString and the other two are constant
    // CFStrings of the framework; the call follows the Create rule.
    let keys = unsafe {
        CFPreferencesCopyKeyList(domain, kCFPreferencesCurrentUser, kCFPreferencesAnyHost)
    };
    if keys.is_null() {
        return false;
    }
    // SAFETY: a non-null result of a Copy call is a +1 reference to a CFArray.
    let keys: CFArray = unsafe { CFArray::wrap_under_create_rule(keys) };
    !keys.is_empty()
}

/// A copied AppleLanguages value: a list of at least one string, else no
/// value (NULL) or a failed read (any other value).
fn languages_of(value: CFTypeRef) -> Languages {
    if value.is_null() {
        return Languages::NoValue;
    }
    // SAFETY: a non-null result of a Copy call is a +1 reference.
    let value = unsafe { CFType::wrap_under_create_rule(value) };
    let Some(list) = value.downcast::<CFArray>() else {
        return Languages::Failed;
    };
    let mut languages = Vec::new();
    for item in list.iter() {
        // SAFETY: the item is a live CF object that the array holds.
        let item = unsafe { CFType::wrap_under_get_rule(*item as CFTypeRef) };
        let Some(language) = item.downcast::<CFString>() else {
            return Languages::Failed;
        };
        languages.push(language.to_string());
    }
    if languages.is_empty() {
        Languages::Failed
    } else {
        Languages::List(languages)
    }
}

/// The first localization of the bundle at `bundle` that CFBundle would use
/// under the preferred languages `languages`, without reference to this
/// process's own context.
fn first_localization(bundle: &CFURL, languages: &[String]) -> Option<String> {
    // SAFETY: `bundle` is a live CFURL for the call, which follows the Create
    // rule.
    let localizations = unsafe { CFBundleCopyLocalizationsForURL(bundle.as_concrete_TypeRef()) };
    if localizations.is_null() {
        return None;
    }
    // SAFETY: a non-null result of a Copy call is a +1 reference to a CFArray.
    let localizations: CFArray = unsafe { CFArray::wrap_under_create_rule(localizations) };
    if localizations.is_empty() {
        return None;
    }
    let preferences: Vec<CFString> = languages
        .iter()
        .map(|language| CFString::new(language))
        .collect();
    let preferences = CFArray::from_CFTypes(&preferences);
    // SAFETY: both are live CFArrays of CFStrings for the call, which follows
    // the Create rule.
    let chosen = unsafe {
        CFBundleCopyLocalizationsForPreferences(
            localizations.as_concrete_TypeRef(),
            preferences.as_concrete_TypeRef(),
        )
    };
    if chosen.is_null() {
        return None;
    }
    // SAFETY: a non-null result of a Copy call is a +1 reference to a CFArray.
    let chosen: CFArray = unsafe { CFArray::wrap_under_create_rule(chosen) };
    let first = chosen.get(0)?;
    // SAFETY: the item is a live CF object that the array holds.
    let first = unsafe { CFType::wrap_under_get_rule(*first as CFTypeRef) };
    first.downcast::<CFString>().map(|text| text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The read of `pid` at `place`, made again while it gives no answer, at
    /// most 25 times: another test of this process can close a file between
    /// the read's two calls.
    fn held(pid: libc::pid_t, place: &str) -> Option<String> {
        (0..25).find_map(|_| SystemLanguages.held_language(pid, place))
    }

    #[test]
    fn the_read_finds_the_one_language_file_that_a_process_holds_open() {
        let me = libc::pid_t::try_from(std::process::id()).expect("a pid");
        let app = std::env::temp_dir()
            .join(format!("halvo-held-language-{me}"))
            .join("Made Browser.app");
        let resources = app
            .join("Contents/Frameworks/Made Browser Framework.framework/Versions/1.2.3/Resources");
        for code in ["de", "en"] {
            let folder = resources.join(format!("{code}.lproj"));
            std::fs::create_dir_all(&folder).expect("the folder");
            std::fs::write(folder.join("locale.pak"), code).expect("the file");
        }
        let place = app.to_str().expect("a path of plain text");
        let none = SystemLanguages.held_language(me, place);
        let german =
            std::fs::File::open(resources.join("de.lproj/locale.pak")).expect("the German file");
        let one = held(me, place);
        let english =
            std::fs::File::open(resources.join("en.lproj/locale.pak")).expect("the English file");
        let two = SystemLanguages.held_language(me, place);
        drop(german);
        let other = held(me, place);
        drop(english);
        assert_eq!(
            (none, one, two, other),
            (None, Some("de".to_string()), None, Some("en".to_string()))
        );
    }
}
