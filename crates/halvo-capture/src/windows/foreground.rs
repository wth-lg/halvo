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
    use windows::core::PWSTR;
    // SAFETY: `pid`, `buf` and `len` are live locals that the calls write,
    // `len` is the length of `buf`, and the process handle that OpenProcess
    // returns is closed once, after the query.
    unsafe {
        if hwnd.0.is_null() {
            return None;
        }
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return None;
        }
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

struct ForegroundState {
    producer: Producer,
    last_app: Arc<Mutex<Option<String>>>,
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
    let Some(app) = foreground_app_for(hwnd) else {
        return;
    };
    FOREGROUND_CTX.with(|cell| {
        if let Some(state) = cell.borrow().as_ref() {
            if let Ok(mut last) = state.last_app.lock() {
                *last = Some(app.clone());
            }
            state.producer.push(foreground_packet(app, time));
        }
    });
}

impl CaptureSource for ForegroundAppSource {
    fn begin(&mut self, ctx: SourceContext) -> Result<Started, CaptureError> {
        let last_app = Arc::clone(&self.last_app);
        let installer = self.installer;
        self.life
            .begin(&ctx, "halvo-foreground-forward", move |producer| {
                let state = ForegroundState { producer, last_app };
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
