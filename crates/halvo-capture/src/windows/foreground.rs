//! Foreground-application capture via `SetWinEventHook`.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use crate::event::Modality;
use crate::sink::SinkHandle;
use crate::source::{CaptureError, CaptureSource, EventSink};

use super::{build_event, EventPlacement, PlacementSink};

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{CloseHandle, HWND, MAX_PATH};
#[cfg(target_os = "windows")]
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
#[cfg(target_os = "windows")]
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
};

pub const FOREGROUND_EVENT: u32 = 0x0003;

pub fn hook_flags() -> u32 {
    WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS
}

/// Build the foreground-app event for `exe_path`.
pub fn map_foreground(
    sink: &SinkHandle,
    exe_path: String,
    placement: EventPlacement,
) -> crate::event::CaptureStreamEvent {
    build_event(sink, Modality::ForegroundApp, exe_path, placement)
}

/// The executable path of the current foreground window's process, or an empty
/// string when there is no foreground window or the process cannot be opened
/// (for example a higher-integrity process across the UIPI boundary).
#[cfg(target_os = "windows")]
pub fn current_foreground_app() -> String {
    unsafe {
        let hwnd = GetForegroundWindow();
        foreground_app_for(hwnd)
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn foreground_app_for(hwnd: HWND) -> String {
    use windows::core::PWSTR;
    unsafe {
        if hwnd.0.is_null() {
            return String::new();
        }
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return String::new();
        }
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return String::new();
        };
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
            String::from_utf16_lossy(&buf[..len as usize])
        } else {
            String::new()
        }
    }
}

/// Shared state a running foreground hook exposes to its callback and to the
/// input sources: the sink to emit through, the placement seam, and the last
/// foreground app the hook observed.
pub struct ForegroundState {
    pub(crate) sink: SinkHandle,
    pub(crate) placement: Arc<dyn PlacementSink>,
    pub(crate) last_app: Arc<Mutex<String>>,
}

/// Foreground-app capture source. `start` installs the WinEvent hook on its own
/// pump thread; `stop` quits that thread and unhooks.
pub struct ForegroundAppSource {
    placement: Arc<dyn PlacementSink>,
    last_app: Arc<Mutex<String>>,
    thread_id: Arc<AtomicU32>,
    pump: Option<JoinHandle<()>>,
}

impl ForegroundAppSource {
    pub fn new(placement: Arc<dyn PlacementSink>) -> Self {
        Self {
            placement,
            last_app: Arc::new(Mutex::new(String::new())),
            thread_id: Arc::new(AtomicU32::new(0)),
            pump: None,
        }
    }

    /// The shared last-foreground-app cell.
    pub fn last_app(&self) -> Arc<Mutex<String>> {
        Arc::clone(&self.last_app)
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
    let app = foreground_app_for(hwnd);
    FOREGROUND_CTX.with(|cell| {
        if let Some(state) = cell.borrow().as_ref() {
            if let Ok(mut last) = state.last_app.lock() {
                *last = app.clone();
            }
            if let Ok(placement) = state
                .placement
                .place(app.as_bytes(), crate::event::RetentionClass::Raw)
            {
                let event = map_foreground(&state.sink, app, placement);
                let _ = state.sink.emit(event);
            }
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
            placement: Arc::clone(&self.placement),
            last_app: Arc::clone(&self.last_app),
        };
        let thread_id = Arc::clone(&self.thread_id);
        let pump = std::thread::Builder::new()
            .name("halvo-foreground".to_string())
            .spawn(move || run_foreground_hook(state, &thread_id))
            .map_err(|e| CaptureError::Start(format!("foreground pump thread: {e}")))?;
        self.pump = Some(pump);
        Ok(())
    }

    fn stop(&mut self) {
        super::post_quit_to(self.thread_id.load(Ordering::SeqCst));
        if let Some(pump) = self.pump.take() {
            let _ = pump.join();
        }
    }
}

#[cfg(target_os = "windows")]
fn run_foreground_hook(state: ForegroundState, thread_id: &AtomicU32) {
    FOREGROUND_CTX.with(|cell| *cell.borrow_mut() = Some(state));
    thread_id.store(super::current_thread_id(), Ordering::SeqCst);
    let hook = unsafe {
        SetWinEventHook(
            FOREGROUND_EVENT,
            FOREGROUND_EVENT,
            None,
            Some(foreground_proc),
            0,
            0,
            hook_flags(),
        )
    };
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
    fn foreground_event_is_the_system_foreground_constant() {
        assert_eq!(FOREGROUND_EVENT, 0x0003);
        #[cfg(target_os = "windows")]
        assert_eq!(
            FOREGROUND_EVENT,
            windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_FOREGROUND
        );
    }

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
}
