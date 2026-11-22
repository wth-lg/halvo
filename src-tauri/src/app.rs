use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use halvo_coord_app::{Agents, Core};
use tauri::webview::PageLoadEvent;
use tauri::{
    AppHandle, Builder, Context, Manager, Runtime, Url, WebviewUrl, WebviewWindowBuilder, Window,
    WindowEvent,
};

use crate::commands::{self, capture::AppState};
use crate::engine::{EngineSite, GuardAnswer, GuardInstall, GuardInstalled, NotInstalled};
use crate::launch::Exit;
use crate::poller;
use crate::tray::{self, TrayInstalled};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The script that tells the page that the window hides: the close handler of
/// `build` sends it before it hides the window, and the page then disarms
/// Delete everything; the page load of a window that is not visible sends it
/// too, so that a page that was never shown asks nothing on a timer. The page test ui/tests/page/window-scripts.test.js runs
/// this text in the stub page, and the Rust test
/// `the_close_handler_tells_the_page_before_it_hides_the_window` reads the
/// text of the handler, because the mock runtime runs no script. Only the real
/// window shows that the engine runs the script before the window hides.
pub const WINDOW_HIDDEN: &str = "window.Halvo && Halvo.views && Halvo.views.window('hidden')";
/// The script that tells the page that the window shows, sent by the tray's
/// `show_main_window` after it shows the window. The same page test runs this text; only the real window shows
/// that the engine runs it.
pub const WINDOW_SHOWN: &str = "window.Halvo && Halvo.views && Halvo.views.window('shown')";

pub type TrayInstall<R> = fn(&AppHandle<R>) -> tauri::Result<TrayInstalled>;

pub struct Parts<R: Runtime> {
    pub state: AppState,
    pub tray: Option<TrayInstall<R>>,
    pub guard: GuardInstall<R>,
    pub poll: bool,
    pub window_visible: bool,
    pub engine_rules_dir: Option<PathBuf>,
    #[cfg(windows)]
    pub webview_data_dir: Option<PathBuf>,
}

pub const GUARD_WAIT: Duration = Duration::from_secs(20);

pub const ENGINE_RULES_DIR: &str = "engine-rules";

pub enum Opening {
    Load(GuardInstalled),
    RunWithoutWindow(NotInstalled),
    End(NotInstalled),
}

pub fn decide(answer: GuardAnswer, tray: bool) -> Opening {
    match (answer, tray) {
        (Ok(installed), _) => Opening::Load(installed),
        (Err(reason), true) => Opening::RunWithoutWindow(reason),
        (Err(reason), false) => Opening::End(reason),
    }
}

pub fn not_opened_line(reason: &str) -> String {
    format!(
        "halvo: the window main is not opened, because its engine guard is not installed: {reason}"
    )
}

pub struct WindowMissing {
    pub restart_cures: bool,
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
            let control = Arc::clone(&parts.state.control);
            let views = Arc::clone(&parts.state.views);
            let mut start: commands::agents::Start = Box::new(|| {});
            if app.try_state::<Agents>().is_none() {
                let handle = app.handle().clone();
                let (agents, steps) = commands::agents::core(
                    app.handle(),
                    Box::new(move |count| tray::set_attention(&handle, count)),
                    &control,
                    parts.state.data_dir.as_ref(),
                );
                app.manage(agents);
                start = steps;
            }
            app.manage(parts.state);
            app.manage(PageLoads::default());
            app.manage(Hooks::default());
            let ticker = app.handle().clone();
            std::thread::spawn(move || {
                start();
                tick(&ticker)
            });
            let (sender, answer) = mpsc::channel();
            match main_window(
                app.handle(),
                #[cfg(windows)]
                parts.webview_data_dir,
            ) {
                Ok(window) => match engine_site(app.handle(), parts.engine_rules_dir) {
                    Ok(site) => (parts.guard)(&window, site, sender),
                    Err(e) => {
                        let _ = sender.send(Err(NotInstalled::from(e)));
                    }
                },
                Err(e) => {
                    let _ = sender.send(Err(NotInstalled::from(format!(
                        "the window main cannot be built: {e}"
                    ))));
                }
            }
            let tray = match parts.tray.map(|install| install(app.handle())) {
                Some(Ok(installed)) => Some(installed),
                Some(Err(e)) => {
                    eprintln!("halvo: the app runs without a tray item: {e}");
                    None
                }
                None => None,
            };
            let with_tray = tray.is_some();
            app.state::<AppState>()
                .tray_installed
                .store(with_tray, Ordering::SeqCst);
            let handle = app.handle().clone();
            let modules = commands::table::<R>()
                .iter()
                .map(|module| module.registration)
                .collect();
            views.set_drawer(poller::Drawer::new(
                Box::new(move |view| {
                    tray::refresh(&handle, view);
                    tick(&handle);
                }),
                modules,
            ));
            if parts.poll {
                poller::start(control, views, tray);
            }
            let visible = parts.window_visible;
            match answer.try_recv() {
                Ok(answer) => open(app.handle(), decide(answer, with_tray), visible),
                Err(TryRecvError::Disconnected) => open(
                    app.handle(),
                    decide(Err(NotInstalled::from(closure_not_run())), with_tray),
                    visible,
                ),
                Err(TryRecvError::Empty) => {
                    wait_for_the_guard(app.handle().clone(), answer, with_tray, visible)
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                hide_on_close(window);
            }
        })
        .on_page_load(|webview, payload| {
            if payload.event() == PageLoadEvent::Finished
                && webview.label() == "main"
                && is_app_url(payload.url())
            {
                let _ = webview.window().set_title(&format!("Halvo {VERSION}"));
                if !webview.window().is_visible().unwrap_or(false) {
                    let _ = webview.eval(WINDOW_HIDDEN);
                }
                if let Some(loads) = webview.try_state::<PageLoads>() {
                    loads.0.fetch_add(1, Ordering::SeqCst);
                }
            }
        })
}

/// What the close handler does once it has kept the window: it tells the page that the window
/// hides, hides the window, and then calls `Core::hidden` for the window main, since no window
/// event stands behind a hide.
pub(crate) fn hide_on_close<R: Runtime>(window: &Window<R>) {
    if let Some(webview) = window.app_handle().get_webview_window(window.label()) {
        let _ = webview.eval(WINDOW_HIDDEN);
    }
    let _ = window.hide();
    if window.label() == "main" {
        if let Some(agents) = core_of(window.app_handle()) {
            agents.hidden();
        }
    }
}

/// The state of the hooks: whether the tray's Quit has begun, after which no hook calls the
/// Core, and the turn that a tick holds while it runs, so that the quit hook starts after a
/// tick that has begun and no tick starts after it.
#[derive(Default)]
pub struct Hooks {
    quitting: AtomicBool,
    turn: Mutex<()>,
}

/// Whether the tray's Quit has begun.
pub(crate) fn quitting<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.try_state::<Hooks>()
        .is_some_and(|hooks| hooks.quitting.load(Ordering::SeqCst))
}

/// The app's Core for a hook, or none once the tray's Quit has begun.
pub(crate) fn core_of<R: Runtime>(app: &AppHandle<R>) -> Option<Arc<dyn Core>> {
    if quitting(app) {
        return None;
    }
    app.try_state::<Agents>().map(|agents| agents.0.clone())
}

/// `Core::tick`, called off the main thread: once at the start and at each draw of the view,
/// which the poller makes at each of its passes. A tick that panics ends no later tick.
pub(crate) fn tick<R: Runtime>(app: &AppHandle<R>) {
    let Some(hooks) = app.try_state::<Hooks>() else {
        return;
    };
    let _turn = hooks.turn.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(agents) = core_of(app) {
        let _ = catch_unwind(AssertUnwindSafe(|| agents.tick()));
    }
}

/// Whether this is the first Quit of the tray. From it on no hook calls the Core, and when it
/// returns, a tick that had begun has ended.
pub(crate) fn begin_quit<R: Runtime>(app: &AppHandle<R>) -> bool {
    let Some(hooks) = app.try_state::<Hooks>() else {
        return true;
    };
    let _turn = hooks.turn.lock().unwrap_or_else(PoisonError::into_inner);
    !hooks.quitting.swap(true, Ordering::SeqCst)
}

/// The navigation rule of the window main: whether it may navigate to
/// `address`, the string as the engine reports it. A string that the URL reader
/// refuses is refused, and a parsed address is allowed only when `is_app_url`
/// allows it. Tauri's builder hands the rule a parsed address, and the app
/// gives the rule that address's string. The rule refuses every address outside
/// the app's own origin that the runtime puts to it. An address that the
/// runtime's own reader refuses is allowed by the runtime without the rule
/// (tauri-runtime-wry 2.12.3); it is reached only by script in the page, and
/// the means of `engine::guard` decide it at the engine. On windows-2025-vs2026
/// 20261103.298.1 a navigation to https://xn--zz.com/p?q#f, a form that the reader
/// refuses, opened 8 connections with no layer and 0 with every engine means in,
/// and the same form over http with a port of the job opened 2 connections with 2
/// requests and then 0 (hosted run 43632388796, job 132561673817, log lines 5924,
/// 5921, 5934 and 5931). The plain http form at port 80 on Windows: not
/// measurable, because the system holds port 80 on the runner. On
/// macos-15-arm64 20261104.0514.1 the three forms: not measured, the control read
/// 0 (job 132561673633). Row A2-1b on macOS: not measured: with no layer 0 of the
/// 70 refused forms left the app on macos-15-arm64 20261104.0514.1 (WebKit
/// 20621.3.11.11.3) beside both controls above 0 (hosted run 43585044597, job
/// 132417605652, log lines 6766 to 6769; ruling AP-12 point 2 (d) leaves the 70
/// forms out of later runs); what refuses them was not read, and Halvo's means had
/// nothing to refuse.
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
pub(crate) fn main_window<R: Runtime>(
    app: &AppHandle<R>,
    #[cfg(windows)] data_dir: Option<PathBuf>,
) -> tauri::Result<tauri::WebviewWindow<R>> {
    let mut config = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == "main")
        .cloned()
        .ok_or_else(|| tauri::Error::WindowNotFound)?;
    if let Some(state) = app.try_state::<crate::commands::capture::AppState>() {
        state.keep_last_delete(crate::view_state::LastDelete::None);
    }
    config.url = WebviewUrl::External(blank());
    let builder = WebviewWindowBuilder::from_config(app, &config)?
        .on_navigation(|url| may_navigate(url.as_str()))
        .on_download(|_, _| false)
        .initialization_script_for_all_frames(NO_PEER_SCRIPT)
        .visible(false);
    #[cfg(target_os = "macos")]
    let builder = match crate::engine::configuration_at_creation() {
        Some(configuration) => builder.with_webview_configuration(configuration),
        None => builder,
    };
    #[cfg(windows)]
    let builder = builder.additional_browser_args(ENGINE_ARGS);
    #[cfg(windows)]
    let builder = match data_dir {
        Some(dir) => builder.data_directory(dir),
        None => builder,
    };
    builder.build()
}

#[cfg(windows)]
pub const ENGINE_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --host-resolver-rules=\"MAP * ~NOTFOUND\" --proxy-server=http://255.255.255.255:9 --proxy-bypass-list=<-loopback> --webrtc-ip-handling-policy=disable_non_proxied_udp";

pub const NO_PEER_SCRIPT: &str = "(function () { for (const name of ['RTCPeerConnection', 'webkitRTCPeerConnection', 'RTCSessionDescription', 'RTCIceCandidate', 'RTCDataChannel']) { try { Object.defineProperty(window, name, { value: undefined, writable: false, configurable: false }); } catch (e) {} } })();";

#[cfg(any(windows, test))]
pub const ENGINE_ARGUMENTS_VARIABLE: &str = "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS";

/// Takes the engine's own variable out of the process environment. Called once, as the first
/// statement of `main` on Windows, before any thread exists, so that no other thread reads the
/// environment while it changes (ruling AP-17 G9).
#[cfg(windows)]
pub fn take_engine_arguments_out_of_the_environment() {
    std::env::remove_var(ENGINE_ARGUMENTS_VARIABLE);
}

pub fn blank() -> Url {
    Url::parse("about:blank").expect("about:blank is an address")
}

pub fn app_url() -> Url {
    let (scheme, host) = APP_ORIGIN;
    Url::parse(&format!("{scheme}://{host}/index.html")).expect("the app's address is an address")
}

fn engine_site<R: Runtime>(
    app: &AppHandle<R>,
    given: Option<PathBuf>,
) -> Result<EngineSite, String> {
    let rules_dir = match given {
        Some(dir) => dir,
        None => app
            .path()
            .app_cache_dir()
            .map_err(|e| format!("the app has no cache directory for its rule list: {e}"))?
            .join(ENGINE_RULES_DIR),
    };
    Ok(EngineSite { rules_dir })
}

fn closure_not_run() -> String {
    "the install's closure did not run".to_string()
}

fn wait_for_the_guard<R: Runtime>(
    app: AppHandle<R>,
    answer: Receiver<GuardAnswer>,
    with_tray: bool,
    visible: bool,
) {
    std::thread::spawn(move || {
        let answer = match answer.recv_timeout(GUARD_WAIT) {
            Ok(answer) => answer,
            Err(RecvTimeoutError::Disconnected) => Err(NotInstalled::from(closure_not_run())),
            Err(RecvTimeoutError::Timeout) => Err(NotInstalled::from(format!(
                "no answer within {} s",
                GUARD_WAIT.as_secs()
            ))),
        };
        let opening = decide(answer, with_tray);
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || open(&handle, opening, visible));
    });
}

fn open<R: Runtime>(app: &AppHandle<R>, opening: Opening, visible: bool) {
    let opening = match opening {
        Opening::Load(installed) => match load(app, visible) {
            Ok(()) => {
                app.manage(installed);
                return;
            }
            Err(e) => decide(
                Err(NotInstalled::from(format!(
                    "the app's page did not load: {e}"
                ))),
                app.state::<AppState>()
                    .tray_installed
                    .load(Ordering::SeqCst),
            ),
        },
        not_loaded => not_loaded,
    };
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.destroy();
        if let Some(agents) = core_of(app) {
            agents.destroyed();
        }
    }
    match opening {
        Opening::Load(_) => {}
        Opening::RunWithoutWindow(refusal) => {
            eprintln!("{}", not_opened_line(&refusal.reason));
            app.manage(WindowMissing {
                restart_cures: refusal.restart_cures,
            });
            let views = std::sync::Arc::clone(&app.state::<AppState>().views);
            std::thread::spawn(move || views.draw_now());
        }
        Opening::End(refusal) => {
            eprintln!("{}", not_opened_line(&refusal.reason));
            app.exit(Exit::GuardNotInstalled.code());
        }
    }
}

fn load<R: Runtime>(app: &AppHandle<R>, visible: bool) -> tauri::Result<()> {
    let window = app
        .get_webview_window("main")
        .ok_or(tauri::Error::WindowNotFound)?;
    window.navigate(app_url())?;
    if visible {
        window.show()?;
    }
    Ok(())
}
