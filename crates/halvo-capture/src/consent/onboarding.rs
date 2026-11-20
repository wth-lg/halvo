pub use halvo_control::onboarding::{
    open_settings_pane, GrantState, OnboardingState, PaneError, Permission, PermissionProbe,
    PermissionStatus, RequestOutcome, WalkbackAction, WalkbackStep,
};
use halvo_control::onboarding::{AssessInput, Grants, PermissionSet};

/// The onboarding state of `required` as `probe` reads it now, with four fixed
/// inputs: what the probe reads now counts as granted at the capture start, no
/// permission counts as asked, the settings entry is named Halvo, and the
/// switch screen_activity is off.
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
        granted_at_capture_start: now.granted(),
        asked: PermissionSet::EMPTY,
        once_granted: once_granted.iter().copied().collect(),
        macos_major: probe.macos_major_version(),
        settings_entry_name: "Halvo",
        screen_activity: false,
    })
}

#[cfg(test)]
mod wrapper_tests {
    use super::{assess, GrantState, Permission, PermissionProbe, RequestOutcome};

    struct AllOff;

    impl PermissionProbe for AllOff {
        fn check(&self, _permission: Permission) -> GrantState {
            GrantState::NotGranted
        }
        fn request(&self, _permission: Permission) -> RequestOutcome {
            RequestOutcome::AlreadyGranted
        }
    }

    #[test]
    fn the_wrapper_hands_on_the_permissions_once_granted() {
        let required = [Permission::Accessibility, Permission::InputMonitoring];
        let state = assess(&AllOff, &required, &[Permission::Accessibility]);
        assert_eq!(state.lost_grants, [Permission::Accessibility]);
        assert!(state.permissions[0].lost);
        assert!(!state.permissions[1].lost);
    }
}
