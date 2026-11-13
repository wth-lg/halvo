#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod app;
mod check_page;
mod commands;
mod launch;
mod tray;
mod unwired_store;

#[cfg(test)]
mod tests;

use halvo_capture::consent::onboarding::{open_settings_pane, platform_probe, Permission};

pub fn open_pane(permission: Permission) -> Result<(), String> {
    open_settings_pane(permission).map_err(|e| e.to_string())
}

fn main() {
    match launch::parse(std::env::args_os().skip(1)) {
        Ok(launch::Launch::Version) => println!("{}", app::VERSION),
        Ok(launch::Launch::Run) => run(),
        Ok(launch::Launch::CheckPage { config_dir, within }) => {
            std::process::exit(check_page::run(config_dir, within))
        }
        Err(usage) => {
            eprintln!("halvo: {usage}\n{}", launch::USAGE);
            std::process::exit(launch::Exit::Usage.code());
        }
    }
}

fn run() {
    let parts = app::Parts {
        config_dir: None,
        store: Box::new(unwired_store::UnwiredStore),
        store_backend: unwired_store::DESCRIPTION,
        probe: platform_probe(),
        open_pane,
        tray: tray::install,
        window_visible: true,
        #[cfg(windows)]
        webview_data_dir: None,
    };
    app::build(tauri::Builder::default(), parts)
        .run(app::context())
        .expect("failed to start Halvo");
}
