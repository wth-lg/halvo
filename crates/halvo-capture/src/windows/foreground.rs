//! Foreground-application capture via `SetWinEventHook`.

use std::sync::{Arc, Mutex};

use crate::event::Modality;
use crate::source::{
    CaptureError, CaptureSource, SourceContext, SourceCounters, SourceHealth, Started,
};

use super::{Lifecycle, Packet, Producer, PumpReady};

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{CloseHandle, HWND, MAX_PATH};
#[cfg(target_os = "windows")]
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
#[cfg(target_os = "windows")]
use windows::Win32::UI::Accessibility::{
    SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK, WINEVENTPROC,
};
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, EVENT_SYSTEM_FOREGROUND, WINEVENT_OUTOFCONTEXT,
    WINEVENT_SKIPOWNPROCESS,
};

#[cfg(target_os = "windows")]
pub(crate) const HOOK_FLAGS: u32 = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;

pub(crate) type HookInstaller = fn(u32, WINEVENTPROC) -> HWINEVENTHOOK;

pub(crate) fn set_win_event_hook(event: u32, callback: WINEVENTPROC) -> HWINEVENTHOOK {
    // SAFETY: SetWinEventHook takes the event range, no module and a callback
    // with the WINEVENTPROC signature; out-of-context hooks need no module.
    unsafe { SetWinEventHook(event, event, None, callback, 0, 0, HOOK_FLAGS) }
}

pub(crate) fn foreground_packet(exe_path: String, tick: u32) -> Packet {
    Packet {
        kind: Modality::ForegroundApp,
        app: exe_path,
        byte_len: 0,
        tick,
    }
}

/// The executable path of the current foreground window's process, or `None` when
/// there is no foreground window or the process cannot be opened.
#[cfg(target_os = "windows")]
pub fn current_foreground_app() -> Option<String> {
    // SAFETY: GetForegroundWindow takes no argument and returns a handle value.
    let hwnd = unsafe { GetForegroundWindow() };
    foreground_app_for(hwnd)
}

#[cfg(target_os = "windows")]
pub(crate) fn foreground_app_for(hwnd: HWND) -> Option<String> {
    if hwnd.0.is_null() {
        return None;
    }
    let mut pid: u32 = 0;
    // SAFETY: `pid` is a live local that the call writes.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid == 0 {
        return None;
    }
    process_path(pid)
}

#[cfg(target_os = "windows")]
fn process_path(pid: u32) -> Option<String> {
    use windows::core::PWSTR;
    // SAFETY: `buf` and `len` are live locals that the call writes, `len` is
    // the length of `buf`, and the process handle that OpenProcess returns is
    // closed once, after the query.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; MAX_PATH as usize];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        );
        let _ = CloseHandle(handle);
        if ok.is_ok() {
            Some(String::from_utf16_lossy(&buf[..len as usize]))
        } else {
            None
        }
    }
}

/// The app in front as one read gives it: the process that a foreground
/// change is keyed on, and its executable path when the path can be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FrontApp {
    pub(crate) pid: u32,
    pub(crate) path: Option<String>,
}

/// What one foreground event changes: the foreground record to emit, with its
/// app or none, and whether the title of the window in front is read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Change {
    pub(crate) emit: Option<Option<String>>,
    pub(crate) read_title: bool,
}

/// The state of the foreground callback between two events.
#[derive(Debug, Default)]
pub(crate) struct FrontTracker {
    last_pid: Option<u32>,
}

impl FrontTracker {
    /// The change of one foreground event for `hwnd`, whose app `resolve`
    /// reads; it writes the app in front into `cell`.
    pub(crate) fn on_window<W>(
        &mut self,
        cell: &Mutex<Option<String>>,
        hwnd: W,
        resolve: impl Fn(W) -> Option<FrontApp>,
    ) -> Change {
        let Some(front) = resolve(hwnd) else {
            return Change {
                emit: None,
                read_title: false,
            };
        };
        let Some(path) = front.path else {
            return Change {
                emit: None,
                read_title: false,
            };
        };
        if let Ok(mut last) = cell.lock() {
            *last = Some(path.clone());
        }
        self.last_pid = Some(front.pid);
        Change {
            emit: Some(Some(path)),
            read_title: true,
        }
    }
}

/// The process that a frame-host window hosts: the first of its child
/// windows' processes that is not the frame host's own.
pub(crate) fn hosted_pid(frame_pid: u32, child_pids: &[u32]) -> Option<u32> {
    let _ = (frame_pid, child_pids);
    None
}

/// Whether `path` names the frame host of packaged apps.
pub(crate) fn is_frame_host(path: &str) -> bool {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    name.eq_ignore_ascii_case("ApplicationFrameHost.exe")
}

/// The executable path as an event carries it.
pub(crate) fn capped_app(path: String) -> String {
    path
}

struct ForegroundState {
    producer: Producer,
    last_app: Arc<Mutex<Option<String>>>,
    tracker: FrontTracker,
}

/// Foreground-app capture source. `begin` installs the WinEvent hook on its
/// own pump thread.
pub struct ForegroundAppSource {
    last_app: Arc<Mutex<Option<String>>>,
    life: Lifecycle,
    installer: HookInstaller,
}

impl ForegroundAppSource {
    pub fn new() -> Self {
        Self {
            last_app: Arc::new(Mutex::new(None)),
            life: Lifecycle::default(),
            installer: set_win_event_hook,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_installer(installer: HookInstaller) -> Self {
        Self {
            installer,
            ..Self::new()
        }
    }

    /// The shared last-foreground-app cell.
    pub fn last_app(&self) -> Arc<Mutex<Option<String>>> {
        Arc::clone(&self.last_app)
    }

    /// The counts of this source's hand-off queues since it was made.
    pub fn handoff_counts(&self) -> super::HandOffCounts {
        self.life.handoff_counts()
    }
}

impl Default for ForegroundAppSource {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "windows")]
thread_local! {
    static FOREGROUND_CTX: std::cell::RefCell<Option<ForegroundState>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn foreground_proc(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _thread: u32,
    time: u32,
) {
    FOREGROUND_CTX.with(|cell| {
        if let Some(state) = cell.borrow_mut().as_mut() {
            let change = state
                .tracker
                .on_window(&state.last_app, hwnd, resolve_front);
            if let Some(Some(app)) = change.emit {
                state.producer.push(foreground_packet(app, time));
            }
        }
    });
}

#[cfg(target_os = "windows")]
pub(crate) fn resolve_front(hwnd: HWND) -> Option<FrontApp> {
    if hwnd.0.is_null() {
        return None;
    }
    let mut pid: u32 = 0;
    // SAFETY: `pid` is a live local that the call writes.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid == 0 {
        return None;
    }
    let path = foreground_app_for(hwnd).map(capped_app);
    if path.as_deref().is_some_and(is_frame_host) {
        if let Some(hosted) = hosted_pid(pid, &child_pids(hwnd)) {
            return Some(FrontApp {
                pid: hosted,
                path: process_path(hosted).map(capped_app),
            });
        }
    }
    Some(FrontApp { pid, path })
}

#[cfg(target_os = "windows")]
fn child_pids(hwnd: HWND) -> Vec<u32> {
    use windows::core::BOOL;
    use windows::Win32::Foundation::LPARAM;
    use windows::Win32::UI::WindowsAndMessaging::EnumChildWindows;

    unsafe extern "system" fn add(child: HWND, found: LPARAM) -> BOOL {
        let mut pid: u32 = 0;
        // SAFETY: `pid` is a live local that the call writes, and `found`
        // is the address of the vector that `child_pids` holds across the
        // enumeration, which runs on this thread.
        unsafe {
            GetWindowThreadProcessId(child, Some(&mut pid));
            (*(found.0 as *mut Vec<u32>)).push(pid);
        }
        BOOL::from(true)
    }

    let mut found: Vec<u32> = Vec::new();
    // SAFETY: the callback has the WNDENUMPROC signature and reads `found`
    // only while this call runs.
    unsafe {
        let _ = EnumChildWindows(
            Some(hwnd),
            Some(add),
            LPARAM(&mut found as *mut Vec<u32> as isize),
        );
    }
    found
}

impl CaptureSource for ForegroundAppSource {
    fn begin(&mut self, ctx: SourceContext) -> Result<Started, CaptureError> {
        let last_app = Arc::clone(&self.last_app);
        let installer = self.installer;
        self.life
            .begin(&ctx, "halvo-foreground-forward", move |producer| {
                let state = ForegroundState {
                    producer,
                    last_app,
                    tracker: FrontTracker::default(),
                };
                super::spawn_pump("halvo-foreground".to_string(), move |ready, signal| {
                    run_foreground_hook(state, installer, ready, &signal)
                })
            })
    }

    fn shutdown(&mut self) -> Result<(), CaptureError> {
        self.life.shutdown()
    }

    fn health(&self) -> SourceHealth {
        self.life.health(SourceCounters::default())
    }
}

#[cfg(target_os = "windows")]
fn run_foreground_hook(
    state: ForegroundState,
    installer: HookInstaller,
    ready: PumpReady,
    signal: &super::StopSignal,
) {
    FOREGROUND_CTX.with(|cell| *cell.borrow_mut() = Some(state));
    let hook = installer(EVENT_SYSTEM_FOREGROUND, Some(foreground_proc));
    if hook.is_invalid() {
        let error = super::last_os_error("SetWinEventHook");
        FOREGROUND_CTX.with(|cell| *cell.borrow_mut() = None);
        ready.report(Err(error));
        return;
    }
    super::ensure_message_queue();
    ready.report(Ok(()));
    super::pump_messages(signal);
    // SAFETY: `hook` is the valid handle that this thread installed above.
    unsafe {
        let _ = UnhookWinEvent(hook);
    }
    FOREGROUND_CTX.with(|cell| *cell.borrow_mut() = None);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_foreground_change_maps_to_a_foreground_app_packet() {
        let packet = foreground_packet("C:\\Windows\\explorer.exe".to_string(), 5);
        assert_eq!(packet.kind, Modality::ForegroundApp);
        assert_eq!(packet.app, "C:\\Windows\\explorer.exe");
        assert_eq!(packet.tick, 5);
    }

    fn app(pid: u32, path: &str) -> Option<FrontApp> {
        Some(FrontApp {
            pid,
            path: Some(path.to_string()),
        })
    }

    #[test]
    fn a_lookup_that_fails_after_a_success_leaves_the_cell_none() {
        let cell = Mutex::new(None);
        let mut tracker = FrontTracker::default();
        let resolver = |step: u32| match step {
            0 => app(10, "C:\\apps\\editor.exe"),
            _ => None,
        };
        let first = tracker.on_window(&cell, 0, resolver);
        assert_eq!(first.emit, Some(Some("C:\\apps\\editor.exe".to_string())));
        assert_eq!(
            cell.lock().unwrap().as_deref(),
            Some("C:\\apps\\editor.exe")
        );
        let _ = tracker.on_window(&cell, 1, resolver);
        assert_eq!(*cell.lock().unwrap(), None, "the cell kept a stale app");
    }

    #[test]
    fn a_new_process_whose_path_cannot_be_read_gives_a_record_with_no_app() {
        let cell = Mutex::new(Some("C:\\apps\\editor.exe".to_string()));
        let mut tracker = FrontTracker::default();
        let change = tracker.on_window(&cell, 0, |_| {
            Some(FrontApp {
                pid: 11,
                path: None,
            })
        });
        assert_eq!(change.emit, Some(None));
        assert_eq!(*cell.lock().unwrap(), None);
    }

    #[test]
    fn a_frame_host_window_names_the_process_that_it_hosts() {
        assert_eq!(hosted_pid(40, &[40, 40, 77, 81]), Some(77));
        assert_eq!(hosted_pid(40, &[40]), None);
        assert_eq!(hosted_pid(40, &[]), None);
        assert!(is_frame_host(
            "C:\\Windows\\System32\\ApplicationFrameHost.exe"
        ));
        assert!(is_frame_host("applicationframehost.EXE"));
        assert!(!is_frame_host("C:\\Windows\\explorer.exe"));
    }

    #[test]
    fn a_path_of_2000_bytes_keeps_at_most_1024_bytes_that_end_in_the_executable_name() {
        let mut path = String::from("C:\\");
        while path.len() + 4 <= 1_991 {
            path.push_str("\u{e9}d\\");
        }
        while path.len() < 1_991 {
            path.push('a');
        }
        path.push_str("\\tool.exe");
        assert_eq!(path.len(), 2_000);
        let kept = capped_app(path.clone());
        assert!(
            kept.len() <= crate::floor::text::TEXT_MAX_BYTES,
            "{}",
            kept.len()
        );
        assert!(kept.ends_with("\\tool.exe"));
        assert!(path.ends_with(&kept));
        assert!(kept.len() >= crate::floor::text::TEXT_MAX_BYTES - 1);
    }

    #[test]
    fn a_second_begin_is_refused_and_health_follows_the_lifecycle() {
        use crate::source::{SourceContext, SourceState, Started, StopCause};
        let _serial = crate::windows::tests::serial();
        let (sink, _writer) = crate::sink::channel(64);
        let mut source = ForegroundAppSource::new();
        let before = source.health().state;
        let first = source.begin(SourceContext::new(sink.clone()));
        let second = source.begin(SourceContext::new(sink.clone()));
        let running = source.health().state;
        let stopped = source.shutdown();
        let after = source.health().state;
        let again = source.begin(SourceContext::new(sink));
        let _ = source.shutdown();
        assert_eq!(before, SourceState::NotReported);
        assert!(matches!(first, Ok(Started::Running)), "{first:?}");
        assert_eq!(second, Err(CaptureError::AlreadyStarted));
        assert_eq!(running, SourceState::Running);
        assert_eq!(stopped, Ok(()));
        assert_eq!(after, SourceState::Stopped(StopCause::Request));
        assert!(matches!(again, Ok(Started::Running)), "{again:?}");
    }

    fn no_hook(_event: u32, _callback: WINEVENTPROC) -> HWINEVENTHOOK {
        HWINEVENTHOOK::default()
    }

    #[test]
    fn a_foreground_hook_that_does_not_install_fails_the_start_with_its_step() {
        let _serial = crate::windows::tests::serial();
        let (sink, _writer) = crate::sink::channel(4);
        let mut source = ForegroundAppSource::with_installer(no_hook);
        let started = source.begin(crate::source::SourceContext::new(sink));
        let _ = source.shutdown();
        assert!(
            matches!(
                started,
                Err(CaptureError::Os {
                    stage: "SetWinEventHook",
                    domain: "HRESULT",
                    ..
                })
            ),
            "{started:?}"
        );
    }

    #[test]
    fn a_foreground_hook_that_the_system_refuses_reports_the_os_code() {
        let _serial = crate::windows::tests::serial();
        let (sink, _writer) = crate::sink::channel(4);
        let mut source =
            ForegroundAppSource::with_installer(crate::windows::tests::hook_without_callback);
        let started = source.begin(crate::source::SourceContext::new(sink));
        let _ = source.shutdown();
        assert_eq!(started, Err(crate::windows::tests::refused_hook_error()));
    }
}
