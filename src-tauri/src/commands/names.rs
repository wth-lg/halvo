//! The command names of each module of the view. build.rs and the app both
//! compile this file, and each reads only a part of it.
#![allow(dead_code)]

/// The command names of one module of the view.
pub struct Names {
    pub module: &'static str,
    pub commands: &'static [&'static str],
}

pub const CAPTURE: Names = Names {
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
};

/// Every module of the view, in the order of the module table.
pub const MODULES: &[&Names] = &[&CAPTURE];
