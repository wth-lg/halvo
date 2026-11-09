//! Window-title capture via `SetWinEventHook`.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

use crate::event::{CaptureStreamEvent, Modality};
use crate::sink::SinkHandle;
use crate::source::{CaptureError, CaptureSource, EventSink};

use super::foreground::{foreground_app_for, ForegroundState};
use super::{build_event, EventPlacement, PlacementSink};

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::HWND;
#[cfg(target_os = "windows")]
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowTextLengthW, GetWindowTextW, OBJID_WINDOW, WINEVENT_OUTOFCONTEXT,
    WINEVENT_SKIPOWNPROCESS,
};

pub const NAME_CHANGE_EVENT: u32 = 0x800C;

/// Build the window-title event, or `None` for an empty title. A title made only
/// of whitespace is treated as empty, so a cleared caption records nothing.
pub fn map_window_title(
    sink: &SinkHandle,
    title: &str,
    exe_path: String,
    placement: EventPlacement,
) -> Option<CaptureStreamEvent> {
    if title.trim().is_empty() {
        return None;
    }
    Some(build_event(
        sink,
        Modality::WindowTitle,
        exe_path,
        placement,
    ))
}

#[cfg(target_os = "windows")]
pub(crate) fn read_window_title(hwnd: HWND) -> String {
    unsafe {
        if hwnd.0.is_null() {
            return String::new();
        }
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return String::new();
        }
        // +1 for the terminating NUL GetWindowTextW writes.
        let mut buf = vec![0u16; len as usize + 1];
        let written = GetWindowTextW(hwnd, &mut buf);
        String::from_utf16_lossy(&buf[..written as usize])
    }
}

/// Window-title capture source. `start` installs the name-change hook on its own
/// pump thread; `stop` quits that thread and unhooks.
pub struct WindowTitleSource {
    placement: Arc<dyn PlacementSink>,
    thread_id: Arc<AtomicU32>,
    pump: Option<JoinHandle<()>>,
}

impl WindowTitleSource {
    pub fn new(placement: Arc<dyn PlacementSink>) -> Self {
        Self {
            placement,
            thread_id: Arc::new(AtomicU32::new(0)),
            pump: None,
        }
    }
}

#[cfg(target_os = "windows")]
thread_local! {
    static TITLE_CTX: std::cell::RefCell<Option<ForegroundState>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(target_os = "windows")]
#[allow(clippy::too_many_arguments)]
unsafe extern "system" fn title_proc(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    id_object: i32,
    _id_child: i32,
    _thread: u32,
    _time: u32,
) {
    // Only the window object's own name is a window title; child objects
    // (menus, list items) raise the same event and are not titles.
    if id_object != OBJID_WINDOW.0 {
        return;
    }
    let title = read_window_title(hwnd);
    if title.trim().is_empty() {
        return;
    }
    let app = foreground_app_for(hwnd);
    TITLE_CTX.with(|cell| {
        if let Some(state) = cell.borrow().as_ref() {
            if let Ok(placement) = state
                .placement
                .place(title.as_bytes(), crate::event::RetentionClass::Raw)
            {
                if let Some(event) = map_window_title(&state.sink, &title, app, placement) {
                    let _ = state.sink.emit(event);
                }
            }
        }
    });
}

impl CaptureSource for WindowTitleSource {
    fn modality(&self) -> Modality {
        Modality::WindowTitle
    }

    fn start(&mut self, sink: SinkHandle) -> Result<(), CaptureError> {
        let state = ForegroundState {
            sink,
            placement: Arc::clone(&self.placement),
            last_app: Arc::new(std::sync::Mutex::new(String::new())),
        };
        let thread_id = Arc::clone(&self.thread_id);
        let pump = std::thread::Builder::new()
            .name("halvo-window-title".to_string())
            .spawn(move || run_title_hook(state, &thread_id))
            .map_err(|e| CaptureError::Start(format!("window-title pump thread: {e}")))?;
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
fn run_title_hook(state: ForegroundState, thread_id: &AtomicU32) {
    TITLE_CTX.with(|cell| *cell.borrow_mut() = Some(state));
    thread_id.store(super::current_thread_id(), Ordering::SeqCst);
    let hook = unsafe {
        SetWinEventHook(
            NAME_CHANGE_EVENT,
            NAME_CHANGE_EVENT,
            None,
            Some(title_proc),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        )
    };
    super::pump_messages();
    if !hook.is_invalid() {
        unsafe {
            let _ = UnhookWinEvent(hook);
        }
    }
    TITLE_CTX.with(|cell| *cell.borrow_mut() = None);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::windows::tests::test_placement;

    #[test]
    fn name_change_event_is_the_object_namechange_constant() {
        assert_eq!(NAME_CHANGE_EVENT, 0x800C);
        #[cfg(target_os = "windows")]
        assert_eq!(
            NAME_CHANGE_EVENT,
            windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_NAMECHANGE
        );
    }

    #[test]
    fn a_non_empty_title_maps_to_a_window_title_event() {
        let (sink, _w) = crate::sink::channel(4);
        let event = map_window_title(
            &sink,
            "Inbox - Mail",
            "C:\\mail.exe".to_string(),
            test_placement(),
        )
        .expect("a non-empty title produces an event");
        assert_eq!(event.modality, Modality::WindowTitle);
        assert_eq!(event.foreground_app, "C:\\mail.exe");
    }

    #[test]
    fn an_empty_or_whitespace_title_produces_no_event() {
        let (sink, _w) = crate::sink::channel(4);
        assert!(map_window_title(&sink, "", "a.exe".to_string(), test_placement()).is_none());
        assert!(map_window_title(&sink, "   ", "a.exe".to_string(), test_placement()).is_none());
    }
}
