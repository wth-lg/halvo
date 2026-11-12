//! Foreground-application capture via `SetWinEventHook`.

use std::sync::{Arc, Mutex};

use crate::event::Modality;
use crate::sink::SinkHandle;
use crate::source::{CaptureError, CaptureSource, EventSink};

use super::{build_event, EventPlacement, PumpHandle, PumpReady};

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

/// The flags shared by both WinEvent hook installers: deliver out of context
/// (through the thread's message queue) and skip Halvo's own process.
#[cfg(target_os = "windows")]
pub(crate) const HOOK_FLAGS: u32 = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;

/// Installs a WinEvent hook for one event on the calling thread and returns its
/// handle, which is invalid when the installation failed.
pub(crate) type HookInstaller = fn(u32, WINEVENTPROC) -> HWINEVENTHOOK;

pub(crate) fn set_win_event_hook(event: u32, callback: WINEVENTPROC) -> HWINEVENTHOOK {
    // SAFETY: SetWinEventHook takes the event range, no module and a callback
    // with the WINEVENTPROC signature; out-of-context hooks need no module.
    unsafe { SetWinEventHook(event, event, None, callback, 0, 0, HOOK_FLAGS) }
}

/// Build the foreground-app event for `exe_path`.
pub fn map_foreground(
    sink: &SinkHandle,
    exe_path: String,
    placement: EventPlacement,
) -> crate::event::CaptureStreamEvent {
    build_event(sink, Modality::ForegroundApp, exe_path, placement)
}

/// The executable path of the current foreground window's process, or `None` when
/// there is no foreground window or the process cannot be opened (for example a
/// higher-integrity process across the UIPI boundary).
#[cfg(target_os = "windows")]
pub fn current_foreground_app() -> Option<String> {
    unsafe {
        let hwnd = GetForegroundWindow();
        foreground_app_for(hwnd)
    }
}

/// The executable path of `hwnd`'s process, or `None` for a null window, a window
/// whose process id is 0, a process that cannot be opened, or a failed query.
/// `None` distinguishes "no app observed" from an app whose path is the empty
/// string, which cannot happen.
#[cfg(target_os = "windows")]
pub(crate) fn foreground_app_for(hwnd: HWND) -> Option<String> {
    use windows::core::PWSTR;
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

/// Shared state a running foreground hook exposes to its callback and to the
/// input sources: the sink to emit through and the last foreground app the hook
/// observed (`None` until the first observation).
pub struct ForegroundState {
    pub(crate) sink: SinkHandle,
    pub(crate) last_app: Arc<Mutex<Option<String>>>,
}

/// Foreground-app capture source. `start` installs the WinEvent hook on its own
/// pump thread; `stop` quits that thread and unhooks.
pub struct ForegroundAppSource {
    last_app: Arc<Mutex<Option<String>>>,
    pump: Option<PumpHandle>,
    installer: HookInstaller,
}

impl ForegroundAppSource {
    pub fn new() -> Self {
        Self {
            last_app: Arc::new(Mutex::new(None)),
            pump: None,
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
    _time: u32,
) {
    let Some(app) = foreground_app_for(hwnd) else {
        return;
    };
    FOREGROUND_CTX.with(|cell| {
        if let Some(state) = cell.borrow().as_ref() {
            if let Ok(mut last) = state.last_app.lock() {
                *last = Some(app.clone());
            }
            let event = map_foreground(&state.sink, app, EventPlacement::unpersisted(0));
            let _ = state.sink.emit(event);
        }
    });
}

impl CaptureSource for ForegroundAppSource {
    fn modality(&self) -> Modality {
        Modality::ForegroundApp
    }

    fn start(&mut self, sink: SinkHandle) -> Result<(), CaptureError> {
        let state = ForegroundState {
            sink,
            last_app: Arc::clone(&self.last_app),
        };
        let installer = self.installer;
        let pump = super::spawn_pump("halvo-foreground".to_string(), move |ready| {
            run_foreground_hook(state, installer, ready)
        })?;
        self.pump = Some(pump);
        Ok(())
    }

    fn stop(&mut self) {
        if let Some(pump) = self.pump.take() {
            pump.stop();
        }
    }
}

#[cfg(target_os = "windows")]
fn run_foreground_hook(state: ForegroundState, installer: HookInstaller, ready: PumpReady) {
    FOREGROUND_CTX.with(|cell| *cell.borrow_mut() = Some(state));
    let hook = installer(EVENT_SYSTEM_FOREGROUND, Some(foreground_proc));
    super::ensure_message_queue();
    ready.report();
    super::pump_messages();
    if !hook.is_invalid() {
        unsafe {
            let _ = UnhookWinEvent(hook);
        }
    }
    FOREGROUND_CTX.with(|cell| *cell.borrow_mut() = None);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::windows::tests::test_placement;

    #[test]
    fn map_foreground_builds_a_foreground_app_event() {
        let (sink, _w) = crate::sink::channel(4);
        let event = map_foreground(
            &sink,
            "C:\\Windows\\explorer.exe".to_string(),
            test_placement(),
        );
        assert_eq!(event.modality, Modality::ForegroundApp);
        assert_eq!(event.foreground_app, "C:\\Windows\\explorer.exe");
        assert!(!event.is_derived);
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
}
