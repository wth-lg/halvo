pub struct Names {
    #[cfg(test)]
    pub module: &'static str,
    pub commands: &'static [&'static str],
    /// The commands that the page calls at load and whose replies
    /// `--check-page` waits for.
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
        "last_delete",
    ],
    reads: &[
        "capture_status",
        "inventory",
        "onboarding_state",
        "last_delete",
    ],
};

pub const WORKFLOWS: Names = Names {
    #[cfg(test)]
    module: "workflows",
    commands: &[
        "workflows",
        "approve_workflow",
        "dismiss_workflow",
        "delete_workflow",
    ],
    reads: &[],
};

pub const MODULES: &[&Names] = &[&CAPTURE, &WORKFLOWS];
