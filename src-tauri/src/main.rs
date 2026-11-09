#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod commands;
mod tray;
mod unwired_store;

use std::sync::Mutex;

use halvo_capture::consent::onboarding::platform_probe;
use halvo_capture::consent::status::{ConsentController, SettingsFile, SETTINGS_FILE_NAME};
use tauri::webview::PageLoadEvent;
use tauri::{Manager, WindowEvent};

use commands::AppState;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    if std::env::args().skip(1).any(|arg| arg == "--version") {
        println!("{VERSION}");
        return;
    }

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::capture_status,
            commands::inventory,
            commands::set_capture_state,
            commands::delete_all,
            commands::set_storage_cap,
            commands::onboarding_state,
            commands::request_permission,
            commands::open_permission_pane,
        ])
        .setup(|app| {
            let settings = SettingsFile::new(app.path().app_config_dir()?.join(SETTINGS_FILE_NAME));
            let controller =
                ConsentController::open(Box::new(unwired_store::UnwiredStore), settings.clone())
                    .map_err(|e| {
                        format!(
                            "could not open the consent settings at {}: {e}",
                            settings.path().display()
                        )
                    })?;
            app.manage(AppState {
                controller: Mutex::new(controller),
                probe: platform_probe(),
                store_backend: unwired_store::DESCRIPTION,
            });
            tray::install(app.handle())?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .on_page_load(|webview, payload| {
            if payload.event() == PageLoadEvent::Finished {
                let _ = webview.window().set_title(&format!("Halvo {VERSION}"));
                let _ = webview.eval(format!(
                    "document.getElementById('version').textContent = '{VERSION}';"
                ));
            }
        })
        .run(tauri::generate_context!())
        .expect("failed to start Halvo");
}
