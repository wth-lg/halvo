use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use tauri::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use tauri::http::Response;
use tauri::ipc::Channel;
use tauri::tray::TrayIconBuilder;
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri::{Webview, Window, WindowEvent, Wry};

use crate::control::{self, Answer, ErrCause, Line, SHOW_BOUND, WINDOW_BOUND};
use crate::pages::{self, SCHEME};
use crate::sync::{self, Change, Hub};
use crate::waits::{self, Reports};
use crate::{Arm, Options, SyncWay};

/// The label of the trial's one window.
pub const WINDOW: &str = "main";

/// The event of the sync way `events`.
pub const CHANGE_EVENT: &str = "r4-change";

const COUNTS_WAIT: Duration = Duration::from_millis(1000);

const LABEL_FREE_WAIT: Duration = Duration::from_secs(5);

struct PageState {
    alive: bool,
    loaded: bool,
    pending_show: Option<u64>,
}

/// The core of the trial app: it answers the control lines and the page's calls.
pub struct TrialCore {
    app: AppHandle,
    options: Options,
    page: Mutex<PageState>,
    frames: Reports<()>,
    counts: waits::Counts,
    hub: Hub,
    channel: Mutex<Option<Channel<Change>>>,
    sending: AtomicBool,
    bumps: AtomicU64,
    window_ops: Mutex<()>,
}

pub(crate) fn make(options: Options, listener: std::net::TcpListener) -> tauri::Builder<Wry> {
    let pages_dir = options.pages.clone();
    tauri::Builder::default()
        .register_uri_scheme_protocol(SCHEME, move |_ctx, request| {
            let served = pages::serve(&pages_dir, request.uri().path());
            Response::builder()
                .status(served.status)
                .header(CONTENT_TYPE, served.content_type)
                .header("X-Content-Type-Options", "nosniff")
                .header(CACHE_CONTROL, "no-store")
                .body(served.body)
                .unwrap_or_default()
        })
        .invoke_handler(tauri::generate_handler![
            config, bump, frame, counts, report, next, subscribe
        ])
        .on_page_load(|webview, payload| {
            if let Some(core) = core_of(webview.app_handle()) {
                core.page_load(webview, payload.event());
            }
        })
        .on_window_event(|window, event| {
            if let Some(core) = core_of(window.app_handle()) {
                core.window_event(window, event);
            }
        })
        .setup(move |app| {
            let core = Arc::new(TrialCore {
                app: app.handle().clone(),
                options: options.clone(),
                page: Mutex::new(PageState {
                    alive: false,
                    loaded: false,
                    pending_show: None,
                }),
                frames: Reports::default(),
                counts: waits::Counts::default(),
                hub: Hub::default(),
                channel: Mutex::new(None),
                sending: AtomicBool::new(false),
                bumps: AtomicU64::new(0),
                window_ops: Mutex::new(()),
            });
            app.manage(core.clone());
            let port = listener.local_addr()?.port();
            if options.tray {
                TrayIconBuilder::with_id("r4-trial")
                    .title("R4 trial")
                    .tooltip("R4 trial")
                    .build(app)?;
            }
            core.make_window()?;
            control::serve(listener, Arc::new(core.clone()))?;
            print_line(&format!("control port={port}"));
            core.made(true);
            Ok(())
        })
}

fn core_of(app: &AppHandle) -> Option<Arc<TrialCore>> {
    app.try_state::<Arc<TrialCore>>().map(|s| s.inner().clone())
}

impl TrialCore {
    fn window(&self) -> Option<WebviewWindow> {
        if !self.page().alive {
            return None;
        }
        self.app.get_webview_window(WINDOW)
    }

    fn page(&self) -> std::sync::MutexGuard<'_, PageState> {
        self.page.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn make_window(&self) -> tauri::Result<WebviewWindow> {
        let url = pages::page_url("index.html")
            .parse()
            .map_err(tauri::Error::InvalidUrl)?;
        let builder = WebviewWindowBuilder::new(&self.app, WINDOW, WebviewUrl::CustomProtocol(url))
            .title("Trial")
            .inner_size(620.0, 720.0)
            .visible(false);
        #[cfg(target_os = "macos")]
        let builder = if self.options.arm == Arm::A2 {
            builder.background_throttling(tauri::utils::config::BackgroundThrottlingPolicy::Suspend)
        } else {
            builder
        };
        let free_by = Instant::now() + LABEL_FREE_WAIT;
        while self.app.get_webview_window(WINDOW).is_some() && Instant::now() < free_by {
            thread::sleep(Duration::from_millis(10));
        }
        {
            self.page().loaded = false;
            self.counts.forget();
        }
        let window = builder.build()?;
        self.page().alive = true;
        Ok(window)
    }

    fn made(&self, built: bool) {
        if cfg!(target_os = "macos") && self.options.arm == Arm::A2 {
            quiet_line("background_throttling", built);
        }
    }

    fn eval(window: &WebviewWindow, script: &str) {
        let _ = window.eval(script);
    }

    fn show(&self) -> Answer {
        let started = Instant::now();
        let Some(token) = self.frames.begin_alone() else {
            return Answer::Err(ErrCause::Busy);
        };
        {
            let _ops = self.window_ops.lock().unwrap_or_else(|e| e.into_inner());
            self.hub.open();
            let window = match self.window() {
                Some(window) => window,
                None => match self.make_window() {
                    Ok(window) => {
                        self.made(true);
                        window
                    }
                    Err(e) => {
                        self.made(false);
                        eprintln!("r4-trial-app: window: {e}");
                        self.frames.cancel();
                        return Answer::Err(ErrCause::NoWindow);
                    }
                },
            };
            if self.options.arm == Arm::A2 {
                quiet(&window, false);
            }
            let _ = window.show();
            let loaded = {
                let mut page = self.page();
                page.pending_show = Some(token);
                page.loaded
            };
            if loaded {
                if matches!(self.options.arm, Arm::A | Arm::A2) {
                    Self::eval(&window, "window.__r4Window && window.__r4Window('show')");
                }
                Self::eval(
                    &window,
                    &format!("window.__r4AppFrame && window.__r4AppFrame({token})"),
                );
            }
        }
        let waited = self.frames.wait(token, SHOW_BOUND);
        {
            let mut page = self.page();
            if page.pending_show == Some(token) {
                page.pending_show = None;
            }
        }
        control::shown(waited, started.elapsed())
    }

    fn hide(&self) -> Answer {
        let _ops = self.window_ops.lock().unwrap_or_else(|e| e.into_inner());
        let Some(window) = self.window() else {
            return Answer::Err(ErrCause::NoWindow);
        };
        self.read_counts(&window);
        self.hub.close();
        if matches!(self.options.arm, Arm::A | Arm::A2) {
            Self::eval(&window, "window.__r4Window && window.__r4Window('hide')");
        }
        if self.options.arm == Arm::A2 {
            quiet(&window, true);
        }
        let _ = window.hide();
        if waits::until(WINDOW_BOUND, || window.is_visible().ok() == Some(false)) {
            Answer::Ok
        } else {
            Answer::Err(ErrCause::Timeout)
        }
    }

    fn destroy(&self) -> Answer {
        let _ops = self.window_ops.lock().unwrap_or_else(|e| e.into_inner());
        let Some(window) = self.window() else {
            return Answer::Err(ErrCause::NoWindow);
        };
        self.hub.close();
        self.page().alive = false;
        self.frames.cancel();
        let _ = window.destroy();
        self.forget_page();
        if waits::until(WINDOW_BOUND, || {
            self.app.get_webview_window(WINDOW).is_none()
        }) {
            Answer::Ok
        } else {
            Answer::Err(ErrCause::Timeout)
        }
    }

    fn forget_page(&self) {
        self.page().loaded = false;
        self.counts.forget();
        *self.channel.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    fn load(&self, file: &str) -> Answer {
        let _ops = self.window_ops.lock().unwrap_or_else(|e| e.into_inner());
        let Some(window) = self.window() else {
            return Answer::Err(ErrCause::NoWindow);
        };
        let Some(address) = pages::page_to_load(&self.options.pages, file) else {
            return Answer::Err(ErrCause::NoFile);
        };
        let Ok(url) = address.parse() else {
            return Answer::Err(ErrCause::UnknownLine);
        };
        self.forget_page();
        let _ = window.navigate(url);
        Answer::Ok
    }

    fn sample(&self) -> Answer {
        let pid = std::process::id();
        let Some(window) = self.window() else {
            return Answer::Sampled {
                pid,
                ticks: None,
                frames: None,
            };
        };
        let counts = self.read_counts(&window);
        Answer::Sampled {
            pid,
            ticks: counts.map(|c| c.0),
            frames: counts.map(|c| c.1),
        }
    }

    fn read_counts(&self, window: &WebviewWindow) -> Option<(u64, u64)> {
        self.counts.read(
            |token| {
                Self::eval(
                    window,
                    &format!("window.__r4AppCounts && window.__r4AppCounts({token})"),
                )
            },
            COUNTS_WAIT,
        )
    }

    fn send(self: &Arc<Self>, count: u64, per_second: u64) -> Answer {
        if self.sending.swap(true, Ordering::SeqCst) {
            return Answer::Err(ErrCause::Busy);
        }
        let core = self.clone();
        let spawned = thread::Builder::new()
            .name("r4-send".into())
            .spawn(move || {
                let started = Instant::now();
                let mut sent = 0;
                while sent < count {
                    let due = sync::due(count, per_second, started.elapsed());
                    while sent < due {
                        sent += 1;
                        core.deliver(sync::change(sent, count));
                    }
                    if sent < count {
                        thread::sleep(Duration::from_millis(1));
                    }
                }
                print_line(&format!("core send_done sent={sent}"));
                core.sending.store(false, Ordering::SeqCst);
            });
        if spawned.is_err() {
            self.sending.store(false, Ordering::SeqCst);
            return Answer::Err(ErrCause::Busy);
        }
        Answer::Ok
    }

    fn deliver(&self, change: Change) {
        match self.options.sync {
            Some(SyncWay::Channel) => {
                let channel = self
                    .channel
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone();
                if let Some(channel) = channel {
                    let _ = channel.send(change);
                }
            }
            Some(SyncWay::Events) => {
                let _ = self.app.emit(CHANGE_EVENT, change);
            }
            Some(SyncWay::Get) | None => self.hub.publish(change),
        }
    }

    fn page_load(&self, webview: &Webview<Wry>, event: PageLoadEvent) {
        let mut page = self.page();
        match event {
            PageLoadEvent::Started => {
                page.loaded = false;
                self.counts.forget();
            }
            PageLoadEvent::Finished => {
                page.loaded = true;
                if let Some(token) = page.pending_show {
                    let _ = webview.eval(format!(
                        "window.__r4AppFrame && window.__r4AppFrame({token})"
                    ));
                }
            }
        }
    }

    fn window_event(self: &Arc<Self>, window: &Window<Wry>, event: &WindowEvent) {
        match event {
            WindowEvent::CloseRequested { api, .. } if self.options.arm != Arm::B => {
                api.prevent_close();
                if window.label() == WINDOW {
                    let core = self.clone();
                    let _ = thread::Builder::new()
                        .name("r4-close".into())
                        .spawn(move || core.hide());
                }
            }
            WindowEvent::CloseRequested { .. } => {
                self.hub.close();
                self.page().alive = false;
                self.frames.cancel();
                self.forget_page();
            }
            _ => {}
        }
    }
}

impl control::Core for Arc<TrialCore> {
    fn answer(&self, line: Line) -> Answer {
        match line {
            Line::Show => self.show(),
            Line::Hide => self.hide(),
            Line::Destroy => self.destroy(),
            Line::Load(file) => self.load(&file),
            Line::Send { count, per_second } => self.send(count, per_second),
            Line::Sample => self.sample(),
            Line::Quit => Answer::Ok,
        }
    }

    fn quit(&self) {
        self.app.exit(0);
    }
}

fn print_line(line: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

fn quiet_line(call: &str, ok: bool) {
    let system = if cfg!(windows) { "windows" } else { "macos" };
    let result = if ok { "ok" } else { "err" };
    print_line(&format!(
        "core quiet system={system} call={call} result={result}"
    ));
}

#[cfg(windows)]
fn quiet(window: &WebviewWindow, quiet: bool) {
    let queued = window.with_webview(move |webview| {
        use webview2_com::Microsoft::Web::WebView2::Win32::{
            ICoreWebView2_19, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW,
            COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
        };
        use windows::core::Interface;
        let controller = webview.controller();
        let level = if quiet {
            COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
        } else {
            COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
        };
        unsafe {
            quiet_line("set_is_visible", controller.SetIsVisible(!quiet).is_ok());
            let set = controller
                .CoreWebView2()
                .and_then(|core| core.cast::<ICoreWebView2_19>())
                .and_then(|core| core.SetMemoryUsageTargetLevel(level));
            quiet_line("set_memory_usage_target_level", set.is_ok());
        }
    });
    if queued.is_err() {
        quiet_line("with_webview", false);
    }
}

#[cfg(not(windows))]
fn quiet(_window: &WebviewWindow, _quiet: bool) {}

#[tauri::command]
fn config(core: State<'_, Arc<TrialCore>>) -> serde_json::Value {
    serde_json::json!({
        "arm": core.options.arm.name(),
        "sync": core.options.sync.map(SyncWay::name),
    })
}

#[tauri::command]
fn bump(core: State<'_, Arc<TrialCore>>) -> u64 {
    core.bumps.fetch_add(1, Ordering::SeqCst) + 1
}

#[tauri::command]
fn frame(core: State<'_, Arc<TrialCore>>, gen: u64) {
    core.frames.deliver(gen, ());
}

#[tauri::command]
fn counts(core: State<'_, Arc<TrialCore>>, token: u64, ticks: u64, frames: u64) {
    core.counts.deliver(token, (ticks, frames));
}

#[tauri::command]
fn report(line: String) {
    let line: String = line
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    print_line(&format!("page {line}"));
}

#[tauri::command]
async fn next(core: State<'_, Arc<TrialCore>>, seq: u64) -> Result<serde_json::Value, String> {
    let core = core.inner().clone();
    tauri::async_runtime::spawn_blocking(move || core.hub.wait_next(seq).to_json())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn subscribe(core: State<'_, Arc<TrialCore>>, channel: Channel<Change>) {
    *core.channel.lock().unwrap_or_else(|e| e.into_inner()) = Some(channel);
}
