#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod app;
mod commands;
mod tray;
mod unwired_store;

#[cfg(test)]
mod tests;

use halvo_capture::consent::onboarding::{open_settings_pane, platform_probe, Permission};

fn open_pane(permission: Permission) -> Result<(), String> {
    open_settings_pane(permission).map_err(|e| e.to_string())
}

fn main() {
    if std::env::args().skip(1).any(|arg| arg == "--version") {
        println!("{}", app::VERSION);
        return;
    }

    let parts = app::Parts {
        config_dir: None,
        store: Box::new(unwired_store::UnwiredStore),
        store_backend: unwired_store::DESCRIPTION,
        probe: platform_probe(),
        open_pane,
        tray: tray::install,
    };
    app::build(tauri::Builder::default(), parts)
        .run(app::context())
        .expect("failed to start Halvo");
}
