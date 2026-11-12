use std::path::PathBuf;
use std::sync::Mutex;

use halvo_capture::consent::onboarding::{Permission, PermissionProbe};
use halvo_capture::consent::status::{
    ConsentController, DataStore, SettingsFile, SETTINGS_FILE_NAME,
};
use tauri::webview::PageLoadEvent;
use tauri::{
    AppHandle, Builder, Context, Manager, Runtime, Url, WebviewWindowBuilder, WindowEvent,
};

use crate::commands::{self, capture::AppState};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// What the app is built from: the directory of the consent settings (the
/// app config directory when `None`), the store, its one-line description,
/// the permission probe, the opener of a settings pane and the tray installer.
pub struct Parts<R: Runtime> {
    pub config_dir: Option<PathBuf>,
    pub store: Box<dyn DataStore + Send>,
    pub store_backend: &'static str,
    pub probe: Box<dyn PermissionProbe + Send + Sync>,
    pub open_pane: fn(Permission) -> Result<(), String>,
    pub tray: fn(&AppHandle<R>) -> tauri::Result<()>,
}

/// The context generated from tauri.conf.json, the capabilities and ui/app.
pub fn context<R: Runtime>() -> Context<R> {
    tauri::generate_context!()
}

/// Adds the commands, the set-up and the window rules to `builder`.
pub fn build<R: Runtime>(builder: Builder<R>, parts: Parts<R>) -> Builder<R> {
    builder
        .invoke_handler(commands::handler())
        .setup(move |app| {
            let config_dir = match parts.config_dir {
                Some(dir) => dir,
                None => app.path().app_config_dir()?,
            };
            let settings = SettingsFile::new(config_dir.join(SETTINGS_FILE_NAME));
            let controller =
                ConsentController::open(parts.store, settings.clone()).map_err(|e| {
                    format!(
                        "could not open the consent settings at {}: {e}",
                        settings.path().display()
                    )
                })?;
            app.manage(AppState {
                controller: Mutex::new(controller),
                probe: parts.probe,
                store_backend: parts.store_backend,
                open_pane: parts.open_pane,
            });
            main_window(app.handle())?;
            (parts.tray)(app.handle())?;
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
            }
        })
}

/// Whether `url` is of the app's own origin, the only place the window may
/// navigate to: `tauri://localhost` on macOS, `http://tauri.localhost` on Windows.
pub fn is_app_url(url: &Url) -> bool {
    let (scheme, host) = if cfg!(windows) {
        ("http", "tauri.localhost")
    } else {
        ("tauri", "localhost")
    };
    url.scheme() == scheme && url.host_str() == Some(host) && url.port().is_none()
}

/// Builds the window main from its entry in tauri.conf.json, with the
/// navigation rule `is_app_url`.
fn main_window<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == "main")
        .cloned()
        .ok_or_else(|| tauri::Error::WindowNotFound)?;
    WebviewWindowBuilder::from_config(app, &config)?
        .on_navigation(is_app_url)
        .build()?;
    Ok(())
}
