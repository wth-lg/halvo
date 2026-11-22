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
    #[cfg(windows)]
    pub webview_environment: EnvironmentMaker,
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
            let window = main_window(
                app.handle(),
                #[cfg(windows)]
                parts.webview_data_dir,
                #[cfg(windows)]
                parts.webview_environment,
            );
            #[cfg(windows)]
            if let Ok(window) = &window {
                keep_the_crash_report_folder(window, &app.state::<AppState>().crash_reports);
            }
            match window {
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
                    commands::agents::hold_by_the_view(&handle, view);
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
/// navigation to the hook. On Windows the window is content protected: the
/// system keeps it out of every capture, Halvo's own screen record among them
/// (display affinity WDA_EXCLUDEFROMCAPTURE, which `--check-page` prints); and
/// its web view's general autofill is off, so that the web view keeps no entry
/// of what a person types into the page and offers none (`--check-page` prints
/// it too).
pub(crate) fn main_window<R: Runtime>(
    app: &AppHandle<R>,
    #[cfg(windows)] data_dir: Option<PathBuf>,
    #[cfg(windows)] environment: EnvironmentMaker,
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
    let builder = builder.content_protected(true);
    #[cfg(windows)]
    let builder = builder.general_autofill_enabled(false);
    #[cfg(windows)]
    let builder = match environment(engine_options(), &engine_folder(app, data_dir.as_deref())?)
        .map_err(std::io::Error::other)?
    {
        Some(made) => builder.with_environment(made),
        None => builder,
    };
    #[cfg(windows)]
    let builder = match data_dir {
        Some(dir) => builder.data_directory(dir),
        None => builder,
    };
    builder.build()
}

/// Keeps the folder in which the web view of `window` writes its crash reports, as its
/// environment names it (ICoreWebView2Environment11::FailureReportFolderPath), in `folder`, for
/// the end of a whole Delete everything; where it cannot be read the cell stays empty.
#[cfg(windows)]
fn keep_the_crash_report_folder<R: Runtime>(
    window: &tauri::WebviewWindow<R>,
    folder: &Arc<std::sync::OnceLock<PathBuf>>,
) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2Environment11, ICoreWebView2_2,
    };
    use windows_core::{Interface, PWSTR};
    let folder = Arc::clone(folder);
    let _ = window.with_webview(move |webview| {
        let read = || -> windows_core::Result<PathBuf> {
            // SAFETY: wry keeps the controller alive as long as the web view, and with_webview
            // hands it to this closure on the thread that created it, the one thread WebView2 lets
            // call it; each returned interface holds its own reference.
            let core = unsafe { webview.controller().CoreWebView2() }?;
            // SAFETY: as above, on the window's thread; the environment comes back with a
            // reference of its own.
            let environment = unsafe { core.cast::<ICoreWebView2_2>()?.Environment() }?;
            let environment: ICoreWebView2Environment11 = environment.cast()?;
            let mut path = PWSTR::null();
            // SAFETY: on the window's thread; on success the call writes a string allocated with
            // CoTaskMemAlloc that this caller owns, and take_pwstr frees it once with
            // CoTaskMemFree; on an error `path` stays null and is not read.
            unsafe { environment.FailureReportFolderPath(&mut path) }?;
            Ok(PathBuf::from(webview2_com::take_pwstr(path)))
        };
        if let Ok(path) = read() {
            let _ = folder.set(path);
        }
    });
}

#[cfg(windows)]
pub const ENGINE_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --host-resolver-rules=\"MAP * ~NOTFOUND\" --proxy-server=http://255.255.255.255:9 --proxy-bypass-list=<-loopback> --webrtc-ip-handling-policy=disable_non_proxied_udp";

/// Makes the web view's environment of the window main from the options that its builder hands
/// it, on the web view's data folder: `system_environment` in the app. A test hands a stand-in
/// that reads the options and makes none, so that the mock runtime goes on as before.
#[cfg(windows)]
pub type EnvironmentMaker = fn(
    webview2_com::CoreWebView2EnvironmentOptions,
    &std::path::Path,
) -> Result<
    Option<webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Environment>,
    String,
>;

/// The options of the web view's environment: custom crash reporting, with which the web view
/// sends no crash report to Microsoft and keeps its dumps on this computer, and what wry 0.57.1
/// sets in an environment of its own (src/webview2/mod.rs lines 308 to 355), which a given
/// environment replaces: the browser arguments, extensions off, the person's display language and
/// the default scroll bar.
#[cfg(windows)]
pub fn engine_options() -> webview2_com::CoreWebView2EnvironmentOptions {
    use webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_SCROLLBAR_STYLE_DEFAULT;
    let options = webview2_com::CoreWebView2EnvironmentOptions::default();
    // SAFETY: the options are this function's own and no COM object holds them yet, so nothing
    // reads their cells while these setters write them.
    unsafe {
        options.set_is_custom_crash_reporting_enabled(true);
        options.set_additional_browser_arguments(ENGINE_ARGS.to_string());
        options.set_are_browser_extensions_enabled(false);
        options.set_language(display_language());
        options.set_scroll_bar_style(COREWEBVIEW2_SCROLLBAR_STYLE_DEFAULT);
    }
    options
}

/// The person's display language as wry 0.57.1 reads it for its own environment: the locale name
/// of the user's interface language, neutral names allowed.
#[cfg(windows)]
pub fn display_language() -> String {
    windows_core::link!("kernel32.dll" "system" fn GetUserDefaultUILanguage() -> u16);
    windows_core::link!("kernel32.dll" "system" fn LCIDToLocaleName(locale: u32, name: *mut u16, size: i32, flags: u32) -> i32);
    const LOCALE_ALLOW_NEUTRAL_NAMES: u32 = 0x0800_0000;
    let mut name = [0u16; 85];
    // SAFETY: the buffer is this function's own and its length is the size handed to the call,
    // which writes no more than that; the other call takes and reads nothing.
    unsafe {
        LCIDToLocaleName(
            u32::from(GetUserDefaultUILanguage()),
            name.as_mut_ptr(),
            i32::try_from(name.len()).unwrap_or(0),
            LOCALE_ALLOW_NEUTRAL_NAMES,
        );
    }
    let end = name.iter().position(|&c| c == 0).unwrap_or(name.len());
    String::from_utf16_lossy(&name[..end])
}

/// The web view's data folder: the given one, else the app's local data folder, which tauri
/// 2.12.5 gives the web view when the window names none (src/manager/webview.rs lines 559 to 574).
#[cfg(windows)]
fn engine_folder<R: Runtime>(
    app: &AppHandle<R>,
    given: Option<&std::path::Path>,
) -> tauri::Result<PathBuf> {
    match given {
        Some(dir) => Ok(dir.to_path_buf()),
        None => app.path().app_local_data_dir(),
    }
}

/// The folder in which the web view of Halvo's own window writes its crash reports, by the rule
/// of its data folder with none given (`engine_folder`): the app's local data folder as tauri
/// 2.12.5 names it (src/path/desktop.rs lines 265 to 271: the account's LocalAppData known
/// folder, then the app's identifier), then EBWebView, Crashpad, reports. None where the account
/// names no LocalAppData. The one function of both removals: the start empties the folder before
/// the app is built, and the form `--remove-crash-reports` empties it with no window.
#[cfg(windows)]
pub fn crash_report_folder(config: &tauri::Config) -> Option<PathBuf> {
    use halvo_control::paths::AccountLookup;
    let local = halvo_control::paths::SystemAccount.base_dir().ok()??;
    Some(reports_under(
        &PathBuf::from(local).join(&config.identifier),
    ))
}

/// The folder of the crash reports under the web view's data folder `data`, which the web view's
/// environment answers (test 2 of win-crash-reports).
#[cfg(windows)]
fn reports_under(data: &std::path::Path) -> PathBuf {
    data.join("EBWebView").join("Crashpad").join("reports")
}

/// The system's maker of the web view's environment, on the main thread as the runtime's own:
/// the folder is made as tauri makes it, and an environment that cannot be made fails the window,
/// with no fall-back to the runtime's own environment.
///
/// It first initializes COM for the main thread, apartment threaded, as wry 0.57.1 does as the
/// first line of making its web view (src/webview2/mod.rs line 121): the setup hook, which calls
/// this, runs before wry makes any web view and before tao 0.37.2 calls OleInitialize when it
/// makes a window (src/platform_impl/windows/window.rs line 1140), so nothing has initialized COM
/// on that thread yet, and WebView2 refuses the environment with CO_E_NOTINITIALIZED. Its answer
/// is not read as a failure (S_FALSE where COM was initialized before), and COM stays
/// initialized for the thread, as wry leaves it.
#[cfg(windows)]
pub fn system_environment(
    options: webview2_com::CoreWebView2EnvironmentOptions,
    folder: &std::path::Path,
) -> Result<Option<webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Environment>, String>
{
    windows_core::link!("ole32.dll" "system" fn CoInitializeEx(reserved: *const core::ffi::c_void, model: u32) -> i32);
    const COINIT_APARTMENTTHREADED: u32 = 0x2;
    // SAFETY: CoInitializeEx takes a null reserved pointer and the threading model by value.
    unsafe {
        CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED);
    }
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        CreateCoreWebView2EnvironmentWithOptions, ICoreWebView2EnvironmentOptions,
    };
    std::fs::create_dir_all(folder)
        .map_err(|e| format!("the web view's folder cannot be made: {e}"))?;
    let folder = windows_core::HSTRING::from(folder.as_os_str());
    let options = ICoreWebView2EnvironmentOptions::from(options);
    let (sender, made) = mpsc::channel();
    let handler = webview2_com::CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(
        move |result, environment| {
            let _ = sender.send(result.map(|()| environment));
            Ok(())
        },
    ));
    // SAFETY: the folder, the options and the handler are this function's own and live through
    // the call; the handler sends its answer to `made`, which the pump below reads.
    unsafe {
        CreateCoreWebView2EnvironmentWithOptions(
            windows_core::PCWSTR::null(),
            &folder,
            &options,
            &handler,
        )
    }
    .map_err(|e| format!("the web view's environment cannot be asked for: {e}"))?;
    match webview2_com::wait_with_pump(made) {
        Ok(Ok(Some(environment))) => Ok(Some(environment)),
        Ok(Ok(None)) => Err("the web view gave no environment".to_string()),
        Ok(Err(e)) => Err(format!("the web view's environment was not made: {e}")),
        Err(e) => Err(format!("the web view's environment had no answer: {e:?}")),
    }
}

pub const NO_PEER_SCRIPT: &str ="(function () { for (const name of ['RTCPeerConnection', 'webkitRTCPeerConnection', 'RTCSessionDescription', 'RTCIceCandidate', 'RTCDataChannel']) { try { Object.defineProperty(window, name, { value: undefined, writable: false, configurable: false }); } catch (e) {} } })();";

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

/// The step of a start before the app and its window are built: every file under `folder`, the
/// folder of the crash reports of the web view of Halvo's own window, goes by app-4b's
/// `every_file_goes`, folders kept, and only then `make` builds the app, whose setup makes the
/// window. With no folder nothing is removed.
#[cfg(any(windows, test))]
pub(crate) fn after_the_reports_go<T>(
    folder: Option<&std::path::Path>,
    make: impl FnOnce() -> T,
) -> T {
    if let Some(folder) = folder {
        let _ =
            crate::commands::capture::every_file_goes(folder, &|file| std::fs::remove_file(file));
    }
    make()
}

/// The launch form `--remove-crash-reports` on `folder`, the folder of the crash reports of the web
/// view of Halvo's own window, with no window and no web view: every file under it goes by
/// `remove`, folders kept. Its one line, `crash_reports folder=<path> removed=<n> left=<n>`, and
/// its end: 0 where nothing was there or every file went, 1 where one stays. `left` counts the
/// files that stand under the folder after the walk; a folder or an entry that cannot be read
/// counts as one.
#[cfg(any(windows, test))]
pub fn crash_reports_form(
    folder: &std::path::Path,
    remove: &dyn Fn(&std::path::Path) -> std::io::Result<()>,
) -> (String, Exit) {
    let removed = std::cell::Cell::new(0u64);
    let every_file_went = crate::commands::capture::every_file_goes(folder, &|file| {
        let gone = remove(file);
        if gone.is_ok() {
            removed.set(removed.get() + 1);
        }
        gone
    });
    let left = match files_under(folder) {
        0 if !every_file_went => 1,
        left => left,
    };
    let line = format!(
        "crash_reports folder={} removed={} left={left}",
        halvo_control::paths::one_line(&folder.to_string_lossy()),
        removed.get()
    );
    (
        line,
        Exit::ReportsRemoved {
            one_stays: left > 0,
        },
    )
}

/// The files that stand under `dir`, in every folder below it, folders not counted; a folder
/// that stands and cannot be read, and an entry that cannot be read, count as one each.
#[cfg(any(windows, test))]
fn files_under(dir: &std::path::Path) -> u64 {
    match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .map(
                |entry| match entry.and_then(|e| Ok((e.path(), e.file_type()?))) {
                    Ok((path, kind)) if kind.is_dir() => files_under(&path),
                    _ => 1,
                },
            )
            .sum(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
        Err(_) => 1,
    }
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
            crate::stop_hook::stopped();
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

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;

    /// A new folder of this test binary under target/tmp, by the test's `name` and this process.
    /// This module is compiled again by tests/page_guard.rs, which has no `crate::tests`.
    fn scratch(name: &str) -> PathBuf {
        let target = std::env::current_exe()
            .expect("the test binary has a path")
            .ancestors()
            .nth(3)
            .expect("the test binary sits under target/<profile>/deps")
            .to_path_buf();
        let dir = target
            .join("tmp")
            .join(format!("halvo-app-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("the scratch folder can be made");
        dir
    }

    /// The files under `dir`, in every folder below it; folders are not listed.
    fn files_in(dir: &Path) -> Vec<PathBuf> {
        let mut files = Vec::new();
        for entry in std::fs::read_dir(dir).expect("the folder reads") {
            let path = entry.expect("an entry reads").path();
            if path.is_dir() {
                files.extend(files_in(&path));
            } else {
                files.push(path);
            }
        }
        files
    }

    /// Test 1 of win-crash-reports: at a start, the crash reports of an earlier session go before
    /// the window is made. When the maker of the window is called, no file stands under the
    /// folder, also in a folder below it, and the folders stay.
    #[test]
    fn every_crash_report_of_an_earlier_session_is_gone_before_the_maker_of_the_window_is_called() {
        let folder = scratch("start-crash-reports");
        std::fs::create_dir_all(folder.join("below")).expect("a folder below");
        for file in ["a.dmp", "below/b.dmp"] {
            std::fs::write(folder.join(file), b"a dump of the window").expect("a report");
        }
        let at_the_maker = after_the_reports_go(Some(&folder), || files_in(&folder));
        assert_eq!(
            at_the_maker,
            Vec::<PathBuf>::new(),
            "files under the folder when the maker of the window is called"
        );
        assert!(folder.join("below").is_dir(), "the folders stay");
    }

    /// Test 3 of win-crash-reports: the form `--remove-crash-reports`, given alone, on a stand-in
    /// folder: its one line and its end where nothing is there, where every file goes, and where
    /// a file stays.
    #[test]
    fn the_form_tells_in_one_line_what_went_and_what_stays_and_ends_with_0_or_1() {
        use crate::launch::{parse, Launch, Usage};
        let args = |list: &[&str]| -> Vec<std::ffi::OsString> {
            list.iter().map(std::ffi::OsString::from).collect()
        };
        assert_eq!(
            parse(args(&["--remove-crash-reports"])),
            Ok(Launch::RemoveCrashReports)
        );
        assert_eq!(
            parse(args(&["--remove-crash-reports", "--background"])),
            Err(Usage::Extra("--remove-crash-reports".into()))
        );
        let folder = scratch("form-crash-reports").join("reports");
        let shown = halvo_control::paths::one_line(&folder.to_string_lossy());
        let line = |removed: u64, left: u64| {
            format!("crash_reports folder={shown} removed={removed} left={left}")
        };
        let remove = |file: &Path| std::fs::remove_file(file);
        let (said, exit) = crash_reports_form(&folder, &remove);
        assert_eq!((said, exit.code()), (line(0, 0), 0), "nothing there");
        std::fs::create_dir_all(folder.join("below")).expect("a folder below");
        let report = |name: &str| {
            let file = folder.join(name);
            std::fs::write(&file, b"a dump of the window").expect("a report");
            file
        };
        report("a.dmp");
        report("below/b.dmp");
        let (said, exit) = crash_reports_form(&folder, &remove);
        assert_eq!((said, exit.code()), (line(2, 0), 0), "every file went");
        let (a, b) = (report("a.dmp"), report("b.dmp"));
        let refuses_b = |file: &Path| {
            if file == b.as_path() {
                Err(std::io::Error::other("held by the web view"))
            } else {
                std::fs::remove_file(file)
            }
        };
        let (said, exit) = crash_reports_form(&folder, &refuses_b);
        assert_eq!((said, exit.code()), (line(1, 1), 1), "one stays");
        assert!(!a.exists() && b.exists());
    }

    /// What the web view's environment answers as the folder of its crash reports
    /// (ICoreWebView2Environment11::FailureReportFolderPath), on the thread that made it.
    #[cfg(windows)]
    fn failure_report_folder(
        environment: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Environment,
    ) -> windows_core::Result<PathBuf> {
        use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Environment11;
        use windows_core::{Interface, PWSTR};
        let environment: ICoreWebView2Environment11 = environment.cast()?;
        let mut path = PWSTR::null();
        // SAFETY: on the thread that made the environment; on success the call writes a string
        // allocated with CoTaskMemAlloc that this caller owns, and take_pwstr frees it once with
        // CoTaskMemFree; on an error `path` stays null and is not read.
        unsafe { environment.FailureReportFolderPath(&mut path) }?;
        Ok(PathBuf::from(webview2_com::take_pwstr(path)))
    }

    /// Test 2 of win-crash-reports: the one function names the folder that the web view's
    /// environment answers. Its data folder is the one that `engine_folder` gives the window with
    /// none given; and an environment made as the app makes it, with the app's options, answers
    /// EBWebView, Crashpad, reports under its data folder (made on a scratch folder, so that no
    /// web view of the account's own folder starts).
    #[cfg(windows)]
    #[test]
    fn the_one_function_names_the_folder_of_the_crash_reports_that_the_web_views_environment_answers(
    ) {
        let app = tauri::test::mock_builder()
            .build(context())
            .expect("the app builds on the mock runtime");
        let data = engine_folder(app.handle(), None).expect("the app's local data folder");
        assert_eq!(
            crash_report_folder(app.config()),
            Some(reports_under(&data))
        );
        let folder = scratch("environment-crash-reports");
        let environment = system_environment(engine_options(), &folder)
            .expect("the environment is made")
            .expect("an environment");
        assert_eq!(
            failure_report_folder(&environment).expect("the environment answers its folder"),
            reports_under(&folder)
        );
    }
}
