#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod app;
mod check_page;
mod commands;
mod engine;
mod launch;
mod poller;
mod tray;
mod view_state;

#[cfg(test)]
mod tests;

use halvo_control::onboarding::{open_settings_pane, Permission};
use halvo_control::paths::DataDir;

pub fn open_pane(permission: Permission) -> Result<(), String> {
    open_settings_pane(permission).map_err(|e| e.to_string())
}

fn main() {
    match launch::parse(std::env::args_os().skip(1)) {
        Ok(launch::Launch::Version) => println!("{}", app::VERSION),
        Ok(launch::Launch::Run) => run(),
        Ok(launch::Launch::CheckPage { data_dir, within }) => {
            std::process::exit(check_page::run(data_dir, within))
        }
        Err(usage) => {
            eprintln!("halvo: {usage}\n{}", launch::USAGE);
            std::process::exit(launch::Exit::Usage.code());
        }
    }
}

fn run() {
    let parts = app::Parts {
        state: commands::capture::AppState::new(DataDir::for_current_user, open_pane),
        tray: Some(tray::install),
        poll: true,
        window_visible: true,
        #[cfg(windows)]
        webview_data_dir: None,
    };
    let app = match app::build(tauri::Builder::default(), parts).build(app::context()) {
        Ok(app) => app,
        Err(e) => {
            eprintln!("halvo: the app did not start: {e}");
            std::process::exit(1);
        }
    };
    app.run(|_, _| {});
}
