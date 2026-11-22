//! Permission answers. The binary's modes `--probe-permissions [P...]` and
//! `--request-permission P` ask the system in a short-lived child of the
//! daemon (contracts.md C3.6); the daemon takes no grant answer from its own
//! process, and the supervisor reads the child's lines through a
//! [`ProbeRunner`].

use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

use halvo_control::onboarding::{GrantState, Permission, PermissionProbe, RequestOutcome};

use super::children::{self, Ended};

/// How long the probe child may run before the daemon ends it.
pub const PROBE_DEADLINE: Duration = Duration::from_secs(5);

/// How long the request child may run before its starter ends it.
pub const REQUEST_DEADLINE: Duration = Duration::from_secs(60);

/// The binary whose two permission modes the daemon starts as its children.
pub const PROGRAM: &str = "halvo-capture";

/// The lines of `--probe-permissions`: `permission name=<P> grant=<G>` for each
/// permission of `permissions`, in their order, and a check of no other.
pub fn probe_lines(probe: &dyn PermissionProbe, permissions: &[Permission]) -> String {
    permissions
        .iter()
        .map(|permission| {
            format!(
                "permission name={} grant={}\n",
                permission.as_str(),
                probe.check(*permission).as_str()
            )
        })
        .collect()
}

/// The line of `--request-permission P`.
pub fn request_line(probe: &dyn PermissionProbe, permission: Permission) -> String {
    let outcome: RequestOutcome = probe.request(permission);
    format!(
        "requested name={} outcome={}\n",
        permission.as_str(),
        outcome.as_str()
    )
}

/// The answers that the probe child's lines hold; a line of another form is
/// left out.
pub fn read_probe_lines(text: &str) -> Vec<(Permission, GrantState)> {
    text.lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("permission name=")?;
            let (name, grant) = rest.split_once(" grant=")?;
            Some((
                Permission::from_str(name).ok()?,
                GrantState::from_str(grant).ok()?,
            ))
        })
        .collect()
}

/// The probe of this system: the TCC probe on macOS; none on Windows, where
/// the modes print nothing.
pub fn system_probe() -> Option<Box<dyn PermissionProbe>> {
    #[cfg(target_os = "macos")]
    {
        Some(Box::new(tcc::TccProbe))
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// What a probe answered at a poll.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probed {
    /// The probe runs.
    Running,
    /// The probe ended with these answers.
    Answered(Vec<(Permission, GrantState)>),
    /// The probe failed, passed its deadline or was never started.
    Failed,
}

/// Runs the probe for the supervisor without blocking its tick.
pub trait ProbeRunner {
    /// Starts a probe of `permissions` unless one runs.
    fn start(&mut self, permissions: &[Permission]);

    /// What the probe answered since its start.
    fn poll(&mut self) -> Probed;

    /// The program that the probe starts as its child, if it starts one.
    fn program(&self) -> Option<&str> {
        None
    }
}

/// The probe child: `halvo-capture --probe-permissions P...` started from the
/// daemon's own directory.
pub struct ChildProbe {
    dir: PathBuf,
    program: String,
    running: Option<children::Child>,
}

impl ChildProbe {
    /// The probe child of the binary `program` in `dir`.
    pub fn new(dir: PathBuf, program: &str) -> ChildProbe {
        ChildProbe {
            dir,
            program: program.to_owned(),
            running: None,
        }
    }
}

impl ProbeRunner for ChildProbe {
    fn start(&mut self, permissions: &[Permission]) {
        if self.running.is_some() {
            return;
        }
        let mut args = vec!["--probe-permissions"];
        args.extend(permissions.iter().map(|permission| permission.as_str()));
        self.running = children::start(&self.dir, &self.program, &args, PROBE_DEADLINE).ok();
    }

    fn poll(&mut self) -> Probed {
        let Some(running) = self.running.as_mut() else {
            return Probed::Failed;
        };
        let Some(ended) = running.poll() else {
            return Probed::Running;
        };
        self.running = None;
        match ended {
            Ended::Exited {
                status: Some(0),
                out,
            } => Probed::Answered(read_probe_lines(&out)),
            Ended::Exited { .. } | Ended::Deadline => Probed::Failed,
        }
    }

    fn program(&self) -> Option<&str> {
        Some(&self.program)
    }
}

/// The probe of a daemon that cannot name its own directory: it starts
/// nothing and every poll answers [`Probed::Failed`], so no answer ever reads
/// a grant.
pub struct Unanswered;

impl ProbeRunner for Unanswered {
    fn start(&mut self, _permissions: &[Permission]) {}

    fn poll(&mut self) -> Probed {
        Probed::Failed
    }
}

#[cfg(target_os = "macos")]
pub mod tcc {
    use std::ffi::c_void;
    use std::ptr;

    use halvo_control::onboarding::{GrantState, Permission, PermissionProbe, RequestOutcome};

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
        fn AXIsProcessTrusted() -> u8;
        fn AXIsProcessTrustedWithOptions(options: CFTypeRef) -> u8;
    }

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOHIDCheckAccess(request_type: u32) -> u32;
        fn IOHIDRequestAccess(request_type: u32) -> bool;
    }

    fn request_accessibility_with_prompt() -> bool {
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
            if options.is_null() {
                return AXIsProcessTrusted() != 0;
            }
            let trusted = AXIsProcessTrustedWithOptions(options) != 0;
            CFRelease(options);
            trusted
        }
    }

    /// The probe that asks TCC, in the process that calls it.
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
                    if unsafe { CGPreflightScreenCaptureAccess() } {
                        GrantState::Granted
                    } else {
                        GrantState::NotGranted
                    }
                }
                Permission::Accessibility => {
                    if unsafe { AXIsProcessTrusted() } != 0 {
                        GrantState::Granted
                    } else {
                        GrantState::NotGranted
                    }
                }
                Permission::InputMonitoring => grant_from_iohid_access(unsafe {
                    IOHIDCheckAccess(IOHID_REQUEST_TYPE_LISTEN_EVENT)
                }),
            }
        }

        fn request(&self, permission: Permission) -> RequestOutcome {
            if self.check(permission) == GrantState::Granted {
                return RequestOutcome::AlreadyGranted;
            }
            let granted = match permission {
                Permission::ScreenAndSystemAudioRecording => unsafe {
                    CGRequestScreenCaptureAccess()
                },
                Permission::InputMonitoring => unsafe {
                    IOHIDRequestAccess(IOHID_REQUEST_TYPE_LISTEN_EVENT)
                },
                Permission::Accessibility => request_accessibility_with_prompt(),
            };
            if granted {
                RequestOutcome::AlreadyGranted
            } else {
                RequestOutcome::Requested
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    struct Recording {
        checked: RefCell<Vec<Permission>>,
    }

    impl PermissionProbe for Recording {
        fn check(&self, permission: Permission) -> GrantState {
            self.checked.borrow_mut().push(permission);
            match permission {
                Permission::InputMonitoring => GrantState::NotDetermined,
                Permission::ScreenAndSystemAudioRecording => GrantState::Granted,
                Permission::Accessibility => GrantState::NotGranted,
            }
        }

        fn request(&self, permission: Permission) -> RequestOutcome {
            self.checked.borrow_mut().push(permission);
            RequestOutcome::Requested
        }
    }

    fn recording() -> Recording {
        Recording {
            checked: RefCell::new(Vec::new()),
        }
    }

    #[test]
    fn the_probe_checks_only_the_permissions_it_is_given() {
        let probe = recording();
        let named = [
            Permission::ScreenAndSystemAudioRecording,
            Permission::InputMonitoring,
        ];
        assert_eq!(
            probe_lines(&probe, &named),
            "permission name=screen_and_system_audio_recording grant=granted\n\
             permission name=input_monitoring grant=not_determined\n"
        );
        assert_eq!(*probe.checked.borrow(), named);
        let none = recording();
        assert_eq!(probe_lines(&none, &[]), "");
        assert!(
            none.checked.borrow().is_empty(),
            "a probe of nothing checked something"
        );
    }

    #[test]
    fn the_request_line_names_the_permission_and_the_outcome() {
        let probe = recording();
        assert_eq!(
            request_line(&probe, Permission::InputMonitoring),
            "requested name=input_monitoring outcome=requested\n"
        );
        assert_eq!(*probe.checked.borrow(), [Permission::InputMonitoring]);
    }

    #[test]
    fn the_probe_lines_read_back_as_the_answers() {
        let probe = recording();
        let named = [
            Permission::InputMonitoring,
            Permission::ScreenAndSystemAudioRecording,
        ];
        let text = format!("{}other line\n", probe_lines(&probe, &named));
        assert_eq!(
            read_probe_lines(&text),
            [
                (Permission::InputMonitoring, GrantState::NotDetermined),
                (
                    Permission::ScreenAndSystemAudioRecording,
                    GrantState::Granted
                ),
            ]
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_iohid_access_values_of_iohidlib_map_to_their_grants() {
        use super::tcc::grant_from_iohid_access;
        assert_eq!(grant_from_iohid_access(0), GrantState::Granted);
        assert_eq!(grant_from_iohid_access(1), GrantState::NotGranted);
        assert_eq!(grant_from_iohid_access(2), GrantState::NotDetermined);
        assert_eq!(grant_from_iohid_access(3), GrantState::NotDetermined);
        assert_eq!(grant_from_iohid_access(u32::MAX), GrantState::NotDetermined);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_input_monitoring_check_agrees_with_cg_preflight_listen_event_access() {
        #[link(name = "CoreGraphics", kind = "framework")]
        unsafe extern "C" {
            fn CGPreflightListenEventAccess() -> bool;
        }
        let preflight = unsafe { CGPreflightListenEventAccess() };
        let check = tcc::TccProbe.check(Permission::InputMonitoring);
        assert_eq!(
            check == GrantState::Granted,
            preflight,
            "check={check:?} preflight={preflight}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_child_probe_reads_the_lines_of_a_child_beside_the_daemon() {
        use std::os::unix::fs::PermissionsExt;
        let root = crate::daemon::children::tests::test_dir();
        let path = root.path().join("probe-stand-in");
        std::fs::write(
            &path,
            "#!/bin/sh\n[ \"$1\" = --probe-permissions ] || exit 64\nshift\nfor p in \"$@\"; do echo \"permission name=$p grant=not_granted\"; done\n",
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut probe = ChildProbe::new(root.path().to_path_buf(), "probe-stand-in");
        probe.start(&[Permission::InputMonitoring]);
        let answers = loop {
            match probe.poll() {
                Probed::Running => std::thread::sleep(Duration::from_millis(5)),
                other => break other,
            }
        };
        assert_eq!(
            answers,
            Probed::Answered(vec![(Permission::InputMonitoring, GrantState::NotGranted)])
        );
    }
}
