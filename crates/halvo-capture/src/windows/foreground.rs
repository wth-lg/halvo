//! Foreground-application capture via `SetWinEventHook`.

use std::sync::{Arc, Mutex};

use crate::source::{
    CaptureError, CaptureSource, SourceContext, SourceCounters, SourceHealth, Started,
};

use super::{Lifecycle, Observed, Packet, Producer, PumpReady};

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

pub(crate) type HookRemover = fn(HWINEVENTHOOK) -> windows::core::Result<()>;

pub(crate) fn unhook_win_event(hook: HWINEVENTHOOK) -> windows::core::Result<()> {
    // SAFETY: `hook` is a hook that the calling thread installed; a handle
    // that names no hook makes the call fail.
    unsafe { UnhookWinEvent(hook) }.ok()
}

pub(crate) fn set_win_event_hook(event: u32, callback: WINEVENTPROC) -> HWINEVENTHOOK {
    // SAFETY: SetWinEventHook takes the event range, no module and a callback
    // with the WINEVENTPROC signature; out-of-context hooks need no module.
    unsafe { SetWinEventHook(event, event, None, callback, 0, 0, HOOK_FLAGS) }
}

pub(crate) fn foreground_packet(exe_path: Option<String>, tick: u32) -> Packet {
    Packet::observed(Observed::ForegroundApp, exe_path, 0, tick)
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
pub(crate) fn process_path(pid: u32) -> Option<String> {
    use windows::core::PWSTR;
    // SAFETY: OpenProcess takes plain values; the handle that it returns is
    // closed once, after the queries.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let path = image_path_with(|buf, len| {
        // SAFETY: `buf` and `len` are live, `len` holds the length of `buf`,
        // and `handle` is open for the call.
        unsafe {
            QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), len)
        }
    });
    // SAFETY: `handle` is the handle that OpenProcess returned above.
    unsafe {
        let _ = CloseHandle(handle);
    }
    path
}

const LONGEST_PATH_UNITS: usize = 32_768;

#[cfg(target_os = "windows")]
pub(crate) fn image_path_with(
    mut query: impl FnMut(&mut [u16], &mut u32) -> windows::core::Result<()>,
) -> Option<String> {
    use windows::Win32::Foundation::ERROR_INSUFFICIENT_BUFFER;
    for units in [MAX_PATH as usize, LONGEST_PATH_UNITS] {
        let mut buf = vec![0u16; units];
        let mut len = units as u32;
        match query(&mut buf, &mut len) {
            Ok(()) => {
                let len = (len as usize).min(units);
                return Some(capped_app(String::from_utf16_lossy(&buf[..len])));
            }
            Err(error) if error.code() == ERROR_INSUFFICIENT_BUFFER.to_hresult() => continue,
            Err(_) => return None,
        }
    }
    None
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FrontApp {
    pub(crate) pid: u32,
    pub(crate) path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Change {
    pub(crate) emit: Option<Option<String>>,
    pub(crate) read_title: bool,
}

#[derive(Debug, Default)]
pub(crate) struct FrontTracker {
    last_pid: Option<u32>,
}

impl FrontTracker {
    pub(crate) fn on_window<W>(
        &mut self,
        cell: &Mutex<Option<String>>,
        hwnd: W,
        resolve: impl Fn(W) -> Option<FrontApp>,
    ) -> Change {
        let front = resolve(hwnd);
        if let Ok(mut last) = cell.lock() {
            *last = front.as_ref().and_then(|front| front.path.clone());
        }
        let Some(front) = front else {
            return Change {
                emit: None,
                read_title: false,
            };
        };
        if self.last_pid == Some(front.pid) {
            return Change {
                emit: None,
                read_title: true,
            };
        }
        self.last_pid = Some(front.pid);
        Change {
            emit: Some(front.path),
            read_title: true,
        }
    }
}

pub(crate) fn hosted_pid(frame_pid: u32, child_pids: &[u32]) -> Option<u32> {
    child_pids
        .iter()
        .copied()
        .find(|&pid| pid != 0 && pid != frame_pid)
}

pub(crate) fn is_frame_host(path: &str) -> bool {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    name.eq_ignore_ascii_case("ApplicationFrameHost.exe")
}

pub(crate) fn capped_app(mut path: String) -> String {
    let cap = crate::floor::text::TEXT_MAX_BYTES;
    if path.len() <= cap {
        return path;
    }
    let mut start = path.len() - cap;
    while !path.is_char_boundary(start) {
        start += 1;
    }
    path.drain(..start);
    path
}

struct ForegroundState {
    producer: Producer,
    last_app: Arc<Mutex<Option<String>>>,
    tracker: FrontTracker,
}

/// Foreground-app capture source.
pub struct ForegroundAppSource {
    last_app: Arc<Mutex<Option<String>>>,
    life: Lifecycle,
    installer: HookInstaller,
    remover: HookRemover,
}

impl ForegroundAppSource {
    pub fn new() -> Self {
        Self {
            last_app: Arc::new(Mutex::new(None)),
            life: Lifecycle::default(),
            installer: set_win_event_hook,
            remover: unhook_win_event,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_remover(remover: HookRemover) -> Self {
        Self {
            remover,
            ..Self::new()
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
    on_foreground_window(hwnd, time);
}

#[cfg(target_os = "windows")]
pub(crate) const FOREGROUND_POLL_MS: u32 = 400;

#[cfg(target_os = "windows")]
unsafe extern "system" fn foreground_poll(_hwnd: HWND, _msg: u32, _id: usize, time: u32) {
    // SAFETY: GetForegroundWindow takes no argument and returns a handle value.
    on_foreground_window(unsafe { GetForegroundWindow() }, time);
}

#[cfg(target_os = "windows")]
fn on_foreground_window(hwnd: HWND, time: u32) {
    FOREGROUND_CTX.with(|cell| {
        if let Some(state) = cell.borrow_mut().as_mut() {
            let change = state
                .tracker
                .on_window(&state.last_app, hwnd, resolve_front);
            if let Some(app) = change.emit {
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
    let path = process_path(pid);
    if path.as_deref().is_some_and(is_frame_host) {
        if let Some(hosted) = hosted_pid(pid, &child_pids(hwnd)) {
            return Some(FrontApp {
                pid: hosted,
                path: process_path(hosted),
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
    // SAFETY: the callback has the WNDENUMPROC signature and writes `found`
    // through a pointer made from an exclusive borrow, only while this
    // enumeration runs on this thread; nothing else touches `found` meanwhile.
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
        let remover = self.remover;
        self.life
            .begin(&ctx, "halvo-foreground-forward", move |producer| {
                let state = ForegroundState {
                    producer,
                    last_app,
                    tracker: FrontTracker::default(),
                };
                super::spawn_pump("halvo-foreground".to_string(), move |ready, signal| {
                    run_foreground_hook(state, (installer, remover), ready, &signal)
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
    (installer, remover): (HookInstaller, HookRemover),
    ready: PumpReady,
    signal: &super::StopSignal,
) {
    // SAFETY: GetForegroundWindow takes no argument and returns a handle value.
    let front = resolve_front(unsafe { GetForegroundWindow() });
    if let Ok(mut last) = state.last_app.lock() {
        *last = front.and_then(|front| front.path);
    }
    FOREGROUND_CTX.with(|cell| *cell.borrow_mut() = Some(state));
    let hook = installer(EVENT_SYSTEM_FOREGROUND, Some(foreground_proc));
    if hook.is_invalid() {
        let error = super::last_os_error("SetWinEventHook");
        FOREGROUND_CTX.with(|cell| *cell.borrow_mut() = None);
        ready.report(Err(error));
        return;
    }
    super::ensure_message_queue();
    // SAFETY: SetTimer with no window makes a timer of this thread, whose
    // callback has the TIMERPROC signature; the pump below dispatches it.
    let poll = unsafe {
        windows::Win32::UI::WindowsAndMessaging::SetTimer(
            None,
            0,
            FOREGROUND_POLL_MS,
            Some(foreground_poll),
        )
    };
    if poll == 0 {
        let error = super::last_os_error("SetTimer");
        let _ = remover(hook);
        FOREGROUND_CTX.with(|cell| *cell.borrow_mut() = None);
        ready.report(Err(error));
        return;
    }
    ready.report(Ok(()));
    super::pump_messages(signal);
    // SAFETY: `poll` is the timer of this thread made above.
    let killed = unsafe { windows::Win32::UI::WindowsAndMessaging::KillTimer(None, poll) };
    signal.report_removal("KillTimer", killed);
    signal.report_removal("UnhookWinEvent", remover(hook));
    FOREGROUND_CTX.with(|cell| *cell.borrow_mut() = None);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Modality;

    #[test]
    fn a_foreground_change_maps_to_a_foreground_app_packet() {
        let packet = foreground_packet(Some("C:\\Windows\\explorer.exe".to_string()), 5);
        assert_eq!(packet.kind(), Modality::ForegroundApp);
        assert_eq!(packet.app.as_deref(), Some("C:\\Windows\\explorer.exe"));
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
    fn two_events_for_one_app_give_one_foreground_record_and_two_title_reads() {
        let cell = Mutex::new(None);
        let mut tracker = FrontTracker::default();
        let changes: Vec<Change> = (0..2)
            .map(|_| tracker.on_window(&cell, 0, |_| app(10, "C:\\apps\\editor.exe")))
            .collect();
        let records = changes.iter().filter(|c| c.emit.is_some()).count();
        let reads = changes.iter().filter(|c| c.read_title).count();
        assert_eq!((records, reads), (1, 2));
    }

    #[test]
    fn two_hosted_apps_under_one_frame_host_give_two_foreground_records() {
        let cell = Mutex::new(None);
        let mut tracker = FrontTracker::default();
        let hosted = |pid: u32| {
            app(
                pid,
                if pid == 21 {
                    "C:\\Program Files\\WindowsApps\\Calculator.exe"
                } else {
                    "C:\\Windows\\ImmersiveControlPanel\\SystemSettings.exe"
                },
            )
        };
        let first = tracker.on_window(&cell, 21, hosted);
        let second = tracker.on_window(&cell, 22, hosted);
        let again = tracker.on_window(&cell, 22, hosted);
        assert!(first.emit.is_some() && second.emit.is_some());
        assert_eq!(again.emit, None);
    }

    #[test]
    fn a_path_longer_than_max_path_is_read_again_with_the_largest_buffer() {
        use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_INSUFFICIENT_BUFFER};
        let long: Vec<u16> = "C:\\"
            .encode_utf16()
            .chain(std::iter::repeat_n(u16::from(b'a'), 400))
            .chain("\\tool.exe".encode_utf16())
            .collect();
        let mut sizes = Vec::new();
        let path = image_path_with(|buf, len| {
            sizes.push(buf.len());
            if buf.len() < long.len() {
                return Err(windows::core::Error::from_hresult(
                    ERROR_INSUFFICIENT_BUFFER.to_hresult(),
                ));
            }
            buf[..long.len()].copy_from_slice(&long);
            *len = long.len() as u32;
            Ok(())
        });
        assert_eq!(sizes, vec![MAX_PATH as usize, LONGEST_PATH_UNITS]);
        assert_eq!(path.map(|p| p.len()), Some(long.len()));
        let refused = image_path_with(|_, _| {
            Err(windows::core::Error::from_hresult(
                ERROR_ACCESS_DENIED.to_hresult(),
            ))
        });
        assert_eq!(refused, None);
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

    fn removal_refused(_hook: HWINEVENTHOOK) -> windows::core::Result<()> {
        Err(windows::core::Error::from_hresult(
            windows::Win32::Foundation::ERROR_INVALID_HANDLE.to_hresult(),
        ))
    }

    #[test]
    fn a_foreground_source_whose_unhook_fails_answers_the_os_error_of_the_unhook() {
        use crate::source::{HealthReason, SourceState};
        let _serial = crate::windows::tests::serial();
        let (sink, _writer) = crate::sink::channel(4);
        let mut source = ForegroundAppSource::with_remover(removal_refused);
        let started = source.begin(crate::source::SourceContext::new(sink));
        let stopped = source.shutdown();
        let refused = CaptureError::Os {
            stage: "UnhookWinEvent",
            domain: "HRESULT",
            code: i64::from(
                windows::Win32::Foundation::ERROR_INVALID_HANDLE
                    .to_hresult()
                    .0,
            ),
        };
        assert!(started.is_ok(), "{started:?}");
        assert_eq!(stopped, Err(refused.clone()));
        assert_eq!(
            source.health().state,
            SourceState::Failed {
                reason: HealthReason::OsError,
                error: refused,
            }
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
