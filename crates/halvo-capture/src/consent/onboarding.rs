pub use halvo_control::onboarding::{
    open_settings_pane, GrantState, OnboardingState, PaneError, Permission, PermissionProbe,
    PermissionStatus, RequestOutcome, WalkbackAction, WalkbackStep,
};
use halvo_control::onboarding::{AssessInput, Grants, PermissionSet};

pub fn assess(
    probe: &dyn PermissionProbe,
    required: &[Permission],
    once_granted: &[Permission],
) -> OnboardingState {
    let now = required
        .iter()
        .fold(Grants::default(), |grants, &permission| {
            grants.with(permission, probe.check(permission))
        });
    halvo_control::onboarding::assess(&AssessInput {
        required,
        now,
        at_start: now.granted(),
        asked: PermissionSet::EMPTY,
        once_granted: once_granted.iter().copied().collect(),
        macos_major: probe.macos_major_version(),
        entry_name: "Halvo",
        screen_activity: false,
    })
}

pub fn platform_probe() -> Box<dyn PermissionProbe + Send + Sync> {
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::TccProbe)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Box::new(NoGateProbe)
    }
}

#[cfg(not(target_os = "macos"))]
pub struct NoGateProbe;

#[cfg(not(target_os = "macos"))]
impl PermissionProbe for NoGateProbe {
    fn check(&self, _permission: Permission) -> GrantState {
        GrantState::Granted
    }
    fn request(&self, _permission: Permission) -> RequestOutcome {
        RequestOutcome::AlreadyGranted
    }
}

#[cfg(target_os = "macos")]
pub mod macos {
    use std::ffi::c_void;
    use std::ptr;

    use super::{GrantState, Permission, PermissionProbe, RequestOutcome};

    const IOHID_REQUEST_TYPE_LISTEN_EVENT: u32 = 1;
    const IOHID_ACCESS_TYPE_GRANTED: u32 = 0;
    const IOHID_ACCESS_TYPE_DENIED: u32 = 1;

    type CFTypeRef = *const c_void;

    #[repr(C)]
    struct CFDictionaryKeyCallBacks {
        _opaque: [u8; 0],
    }

    #[repr(C)]
    struct CFDictionaryValueCallBacks {
        _opaque: [u8; 0],
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        static kCFBooleanTrue: CFTypeRef;
        static kCFTypeDictionaryKeyCallBacks: CFDictionaryKeyCallBacks;
        static kCFTypeDictionaryValueCallBacks: CFDictionaryValueCallBacks;
        fn CFDictionaryCreate(
            allocator: CFTypeRef,
            keys: *const CFTypeRef,
            values: *const CFTypeRef,
            num_values: isize,
            key_call_backs: *const CFDictionaryKeyCallBacks,
            value_call_backs: *const CFDictionaryValueCallBacks,
        ) -> CFTypeRef;
        fn CFRelease(cf: CFTypeRef);
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGPreflightScreenCaptureAccess() -> bool;
        fn CGRequestScreenCaptureAccess() -> bool;
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        static kAXTrustedCheckOptionPrompt: CFTypeRef;
        fn AXIsProcessTrusted() -> bool;
        fn AXIsProcessTrustedWithOptions(options: CFTypeRef) -> bool;
    }

    fn request_accessibility_with_prompt() -> bool {
        // SAFETY: the keys and values arrays outlive the call; the dictionary
        // holds one CFString -> CFBoolean pair, both process-lifetime constants,
        // with the CFType callbacks, and it is released once after use.
        unsafe {
            let keys = [kAXTrustedCheckOptionPrompt];
            let values = [kCFBooleanTrue];
            let options = CFDictionaryCreate(
                ptr::null(),
                keys.as_ptr(),
                values.as_ptr(),
                1,
                &raw const kCFTypeDictionaryKeyCallBacks,
                &raw const kCFTypeDictionaryValueCallBacks,
            );
            let trusted = AXIsProcessTrustedWithOptions(options);
            CFRelease(options);
            trusted
        }
    }

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOHIDCheckAccess(request_type: u32) -> u32;
        fn IOHIDRequestAccess(request_type: u32) -> bool;
    }

    pub struct TccProbe;

    pub fn grant_from_iohid_access(access: u32) -> GrantState {
        match access {
            IOHID_ACCESS_TYPE_GRANTED => GrantState::Granted,
            IOHID_ACCESS_TYPE_DENIED => GrantState::NotGranted,
            _ => GrantState::NotDetermined,
        }
    }

    fn macos_major_version() -> Option<u32> {
        static MAJOR: std::sync::OnceLock<Option<u32>> = std::sync::OnceLock::new();
        *MAJOR.get_or_init(|| {
            let output = std::process::Command::new("/usr/bin/sw_vers")
                .arg("-productVersion")
                .output()
                .ok()?;
            let text = String::from_utf8(output.stdout).ok()?;
            text.trim().split('.').next()?.parse().ok()
        })
    }

    impl PermissionProbe for TccProbe {
        fn macos_major_version(&self) -> Option<u32> {
            macos_major_version()
        }

        fn check(&self, permission: Permission) -> GrantState {
            match permission {
                Permission::ScreenAndSystemAudioRecording => {
                    // SAFETY: a plain query with no arguments, documented thread-safe.
                    if unsafe { CGPreflightScreenCaptureAccess() } {
                        GrantState::Granted
                    } else {
                        GrantState::NotGranted
                    }
                }
                Permission::Accessibility => {
                    // SAFETY: a plain query with no arguments.
                    if unsafe { AXIsProcessTrusted() } {
                        GrantState::Granted
                    } else {
                        GrantState::NotGranted
                    }
                }
                Permission::InputMonitoring => {
                    // SAFETY: takes one enum value by value and returns an enum.
                    grant_from_iohid_access(unsafe {
                        IOHIDCheckAccess(IOHID_REQUEST_TYPE_LISTEN_EVENT)
                    })
                }
            }
        }

        fn request(&self, permission: Permission) -> RequestOutcome {
            if self.check(permission) == GrantState::Granted {
                return RequestOutcome::AlreadyGranted;
            }
            match permission {
                Permission::ScreenAndSystemAudioRecording => {
                    // SAFETY: no arguments; shows the system prompt at most once per install.
                    if unsafe { CGRequestScreenCaptureAccess() } {
                        RequestOutcome::AlreadyGranted
                    } else {
                        RequestOutcome::Requested
                    }
                }
                Permission::InputMonitoring => {
                    // SAFETY: one enum argument by value.
                    if unsafe { IOHIDRequestAccess(IOHID_REQUEST_TYPE_LISTEN_EVENT) } {
                        RequestOutcome::AlreadyGranted
                    } else {
                        RequestOutcome::Requested
                    }
                }
                Permission::Accessibility => {
                    if request_accessibility_with_prompt() {
                        RequestOutcome::AlreadyGranted
                    } else {
                        RequestOutcome::Requested
                    }
                }
            }
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::macos::{grant_from_iohid_access, TccProbe};
    use super::{GrantState, Permission, PermissionProbe};

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGPreflightListenEventAccess() -> bool;
    }

    #[test]
    fn the_iohid_access_values_of_iohidlib_map_to_their_grants() {
        assert_eq!(grant_from_iohid_access(0), GrantState::Granted);
        assert_eq!(grant_from_iohid_access(1), GrantState::NotGranted);
        assert_eq!(grant_from_iohid_access(2), GrantState::NotDetermined);
        assert_eq!(grant_from_iohid_access(3), GrantState::NotDetermined);
        assert_eq!(grant_from_iohid_access(u32::MAX), GrantState::NotDetermined);
    }

    #[test]
    fn the_input_monitoring_check_agrees_with_cg_preflight_listen_event_access() {
        let preflight = unsafe { CGPreflightListenEventAccess() };
        let check = TccProbe.check(Permission::InputMonitoring);
        assert_eq!(
            check == GrantState::Granted,
            preflight,
            "check={check:?} preflight={preflight}"
        );
    }
}
