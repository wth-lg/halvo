pub struct Names {
    #[cfg(test)]
    pub module: &'static str,
    pub commands: &'static [&'static str],
    /// The commands that the page calls at load and whose replies
    /// `--check-page` waits for. The list promises nothing on writes:
    /// `onboarding_state` saves the settings file when it records a permission
    /// that was granted and not recorded yet.
    pub reads: &'static [&'static str],
}

pub const CAPTURE: Names = Names {
    #[cfg(test)]
    module: "capture",
    commands: &[
        "capture_status",
        "inventory",
        "set_capture_state",
        "delete_all",
        "set_storage_cap",
        "onboarding_state",
        "request_permission",
        "open_permission_pane",
    ],
    reads: &["capture_status", "inventory", "onboarding_state"],
};

pub const MODULES: &[&Names] = &[&CAPTURE];
