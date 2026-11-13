use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
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

pub const WINDOW_HIDDEN: &str = "window.Halvo && Halvo.views && Halvo.views.window('hidden')";
pub const WINDOW_SHOWN: &str = "window.Halvo && Halvo.views && Halvo.views.window('shown')";

pub struct Parts<R: Runtime> {
    pub config_dir: Option<PathBuf>,
    pub store: Box<dyn DataStore + Send>,
    pub store_backend: &'static str,
    pub probe: Box<dyn PermissionProbe + Send + Sync>,
    pub open_pane: fn(Permission) -> Result<(), String>,
    pub tray: fn(&AppHandle<R>) -> tauri::Result<()>,
    pub window_visible: bool,
    pub webview_data_dir: Option<PathBuf>,
}

#[derive(Default)]
pub struct PageLoads(AtomicU32);

impl PageLoads {
    pub fn count(&self) -> u32 {
        self.0.load(Ordering::SeqCst)
    }
}

pub fn context<R: Runtime>() -> Context<R> {
    tauri::generate_context!()
}

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
            app.manage(PageLoads::default());
            main_window(app.handle(), parts.window_visible, parts.webview_data_dir)?;
            (parts.tray)(app.handle())?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if let Some(webview) = window.app_handle().get_webview_window(window.label()) {
                    let _ = webview.eval(WINDOW_HIDDEN);
                }
                let _ = window.hide();
            }
        })
        .on_page_load(|webview, payload| {
            if payload.event() == PageLoadEvent::Finished && webview.label() == "main" {
                let _ = webview.window().set_title(&format!("Halvo {VERSION}"));
                if let Some(loads) = webview.try_state::<PageLoads>() {
                    loads.0.fetch_add(1, Ordering::SeqCst);
                }
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

fn main_window<R: Runtime>(
    app: &AppHandle<R>,
    visible: bool,
    data_dir: Option<PathBuf>,
) -> tauri::Result<()> {
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == "main")
        .cloned()
        .ok_or_else(|| tauri::Error::WindowNotFound)?;
    let mut builder = WebviewWindowBuilder::from_config(app, &config)?
        .on_navigation(is_app_url)
        .visible(visible);
    if let Some(dir) = data_dir {
        builder = builder.data_directory(dir);
    }
    builder.build()?;
    Ok(())
}
