//! What the title rules read of the system about browsers: the apps that the
//! system lists as handlers of https links, the AppleLanguages values of a
//! browser and of the person, and of a browser's process its start and the
//! localization that its bundle takes. Each read runs on the title thread
//! (`halvo-window-title`) alone, never on the tap thread, through the seams of
//! the title rules in [`crate::macos::foreground`], which a test replaces.

#![deny(clippy::let_underscore_must_use)]

use std::ffi::c_void;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use core_foundation::array::{CFArray, CFArrayRef};
use core_foundation::base::{CFType, CFTypeRef, TCFType};
use core_foundation::bundle::{
    kCFBundleIdentifierKey, CFBundleCopyInfoDictionaryForURL,
    CFBundleCopyLocalizationsForPreferences, CFBundleCopyLocalizationsForURL,
};
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::{CFString, CFStringRef};
use core_foundation::url::CFURLRef;
use objc2::rc::autoreleasepool;
use objc2_app_kit::{NSRunningApplication, NSWorkspace};
use objc2_foundation::{NSString, NSURL};

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFPreferencesAnyApplication: CFStringRef;
    static kCFPreferencesCurrentUser: CFStringRef;
    static kCFPreferencesAnyHost: CFStringRef;
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

/// An AppleLanguages value as read: the read failed, the domain holds no
/// value, or the list of languages that it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Languages {
    Failed,
    NoValue,
    List(Vec<String>),
}

/// The reads of a browser's language: one seam, which a test replaces.
pub(crate) trait LanguageReads: Send {
    /// The AppleLanguages value that the browser `bundle_id` reads: its own
    /// where it has one, else what the system's search of its domains gives.
    fn own(&mut self, bundle_id: &str) -> Languages;

    /// The person's global AppleLanguages list.
    fn global(&mut self) -> Languages;

    /// When the process `pid` of the app `bundle_id` started, and the first
    /// localization of its bundle that the system's own function picks under
    /// `languages`; `None` when either cannot be read.
    fn process(
        &mut self,
        pid: libc::pid_t,
        bundle_id: &str,
        languages: &[String],
    ) -> Option<(SystemTime, String)>;
}

/// The reads of the system itself.
pub(crate) struct SystemLanguages;

impl LanguageReads for SystemLanguages {
    fn own(&mut self, bundle_id: &str) -> Languages {
        let domain = CFString::new(bundle_id);
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

    fn process(
        &mut self,
        pid: libc::pid_t,
        bundle_id: &str,
        languages: &[String],
    ) -> Option<(SystemTime, String)> {
        let started = start_of(pid)?;
        let localization = autoreleasepool(|_| -> Option<String> {
            let app = NSRunningApplication::runningApplicationWithProcessIdentifier(pid)?;
            let id = app.bundleIdentifier()?;
            if crate::macos::foreground::bundle_identifier(&id) != bundle_id {
                return None;
            }
            let bundle = app.bundleURL()?;
            first_localization(&bundle, languages)
        })?;
        Some((started, localization))
    }
}

/// The bundle ids of the apps that the system lists as handlers of https
/// links, or `None` when the system names none: an https link always has a
/// handler, so an empty list is a failed read.
pub(crate) fn https_handlers() -> Option<Vec<String>> {
    autoreleasepool(|_| {
        let link = NSURL::URLWithString(&NSString::from_str("https://localhost/"))?;
        let apps = NSWorkspace::sharedWorkspace().URLsForApplicationsToOpenURL(&link);
        let ids: Vec<String> = apps.iter().filter_map(|app| bundle_id_of(&app)).collect();
        (!ids.is_empty()).then_some(ids)
    })
}

fn apple_languages() -> CFString {
    CFString::from_static_string("AppleLanguages")
}

fn cf_url(url: &NSURL) -> CFURLRef {
    (url as *const NSURL).cast::<c_void>() as CFURLRef
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

/// The start of the process `pid` on the wall clock, from the process table.
fn start_of(pid: libc::pid_t) -> Option<SystemTime> {
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size = std::mem::size_of::<libc::proc_bsdinfo>();
    // SAFETY: `info` is writable for `size` bytes, the size passed, and
    // proc_pidinfo writes at most that many bytes and returns how many it
    // wrote.
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size as libc::c_int,
        )
    };
    if usize::try_from(written).ok() != Some(size) {
        return None;
    }
    // SAFETY: proc_pidinfo wrote the whole value, as the check above shows.
    let info = unsafe { info.assume_init() };
    let since = Duration::from_secs(info.pbi_start_tvsec)
        .checked_add(Duration::from_micros(info.pbi_start_tvusec))?;
    UNIX_EPOCH.checked_add(since)
}

/// The first localization of the bundle at `bundle` that CFBundle would use
/// under the preferred languages `languages`, without reference to this
/// process's own context.
fn first_localization(bundle: &NSURL, languages: &[String]) -> Option<String> {
    // SAFETY: NSURL is toll-free bridged with CFURL and `bundle` is live for
    // the call, which follows the Create rule.
    let localizations = unsafe { CFBundleCopyLocalizationsForURL(cf_url(bundle)) };
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

/// The bundle id in the Info.plist of the app at `app`.
fn bundle_id_of(app: &NSURL) -> Option<String> {
    // SAFETY: NSURL is toll-free bridged with CFURL and `app` is live for the
    // call, which follows the Create rule.
    let info = unsafe { CFBundleCopyInfoDictionaryForURL(cf_url(app)) };
    if info.is_null() {
        return None;
    }
    // SAFETY: a non-null result of a Copy call is a +1 reference to a
    // CFDictionary.
    let info: CFDictionary<CFString, CFType> =
        unsafe { CFDictionary::wrap_under_create_rule(info) };
    // SAFETY: a constant CFString of the framework, which lives as long as the
    // process.
    let key = unsafe { CFString::wrap_under_get_rule(kCFBundleIdentifierKey) };
    let id = info.find(&key)?.downcast::<CFString>()?;
    Some(id.to_string())
}
