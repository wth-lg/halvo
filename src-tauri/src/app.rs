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

/// The script that tells the page that the window hides: the close handler of
/// `build` sends it before it hides the window, and the page then disarms
/// Delete everything. The page test ui/tests/page/window-scripts.test.js runs
/// this text in the stub page, and the Rust test
/// `the_close_handler_tells_the_page_before_it_hides_the_window` reads the
/// text of the handler, because the mock runtime runs no script. Only the real
/// window shows that the engine runs the script before the window hides.
pub const WINDOW_HIDDEN: &str = "window.Halvo && Halvo.views && Halvo.views.window('hidden')";
/// The script that tells the page that the window shows, sent by the tray's
/// `show_main_window` after it shows the window. The same page test runs this text; only the real window shows
/// that the engine runs it.
pub const WINDOW_SHOWN: &str = "window.Halvo && Halvo.views && Halvo.views.window('shown')";

pub struct Parts<R: Runtime> {
    pub config_dir: Option<PathBuf>,
    pub store: Box<dyn DataStore + Send>,
    pub store_backend: &'static str,
    pub probe: Box<dyn PermissionProbe + Send + Sync>,
    pub open_pane: fn(Permission) -> Result<(), String>,
    pub tray: fn(&AppHandle<R>) -> tauri::Result<()>,
    pub window_visible: bool,
    #[cfg(windows)]
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
            main_window(
                app.handle(),
                parts.window_visible,
                #[cfg(windows)]
                parts.webview_data_dir,
            )?;
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

/// The navigation rule of the window main: whether it may navigate to
/// `address`, the string as the engine reports it. A string that the URL reader
/// refuses is refused, and a parsed address is allowed only when `is_app_url`
/// allows it. Tauri's builder hands the rule a parsed address, and the app
/// gives the rule that address's string. The rule refuses every address outside
/// the app's own origin that the runtime puts to it. An address that the
/// runtime's own reader refuses is allowed by the runtime without the rule
/// (tauri-runtime-wry 2.12.3); it is reached only by script in the page, and it
/// stays open until a unit closes it at the engine.
pub fn may_navigate(address: &str) -> bool {
    Url::parse(address).is_ok_and(|url| is_app_url(&url))
}

/// The parsed arm of `may_navigate`: whether `url` is of the app's own
/// origin, `tauri://localhost` on macOS and `http://tauri.localhost` on Windows.
pub fn is_app_url(url: &Url) -> bool {
    is_origin(url, APP_ORIGIN)
}

/// The scheme and the host of the app's own origin on this system.
pub const APP_ORIGIN: (&str, &str) = if cfg!(windows) {
    ("http", "tauri.localhost")
} else {
    ("tauri", "localhost")
};

/// Whether `url` is of the origin `scheme://host` with no port and no user
/// information.
pub fn is_origin(url: &Url, (scheme, host): (&str, &str)) -> bool {
    url.scheme() == scheme
        && url.host_str() == Some(host)
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
}

/// Builds the window main from the configuration and hands it the navigation
/// rule `may_navigate`. The Rust test
/// `the_set_up_hands_may_navigate_to_the_window_main_and_no_other_window_builder_stands`
/// reads the text of the sources, because the mock runtime calls no navigation
/// hook: one window builder, no web view builder, no child web view, and the
/// one hook line. `each_arm_of_the_rule_refuses_the_near_misses_of_its_origin`
/// calls the rule itself. Only the real window shows that the engine puts each
/// navigation to the hook.
fn main_window<R: Runtime>(
    app: &AppHandle<R>,
    visible: bool,
    #[cfg(windows)] data_dir: Option<PathBuf>,
) -> tauri::Result<()> {
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == "main")
        .cloned()
        .ok_or_else(|| tauri::Error::WindowNotFound)?;
    let builder = WebviewWindowBuilder::from_config(app, &config)?
        .on_navigation(|url| may_navigate(url.as_str()))
        .visible(visible);
    #[cfg(windows)]
    let builder = match data_dir {
        Some(dir) => builder.data_directory(dir),
        None => builder,
    };
    builder.build()?;
    Ok(())
}
