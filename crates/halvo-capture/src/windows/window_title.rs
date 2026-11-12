//! Window-title capture via `SetWinEventHook`.

use crate::event::{CaptureStreamEvent, Modality};
use crate::sink::SinkHandle;
use crate::source::{CaptureError, CaptureSource, EventSink};

use super::{build_event, EventPlacement, PumpHandle, PumpReady};

#[cfg(target_os = "windows")]
use super::foreground::{foreground_app_for, set_win_event_hook, HookInstaller};
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::HWND;
#[cfg(target_os = "windows")]
use windows::Win32::UI::Accessibility::{UnhookWinEvent, HWINEVENTHOOK};
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowTextLengthW, GetWindowTextW, EVENT_OBJECT_NAMECHANGE, OBJID_WINDOW,
};

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

/// The state a running window-title hook needs: the sink to emit through. The
/// title and its app are read from the window each callback, so unlike the
/// foreground hook this carries no last-app cell.
pub struct TitleState {
    pub(crate) sink: SinkHandle,
}

/// Window-title capture source. `start` installs the name-change hook on its own
/// pump thread; `stop` quits that thread and unhooks.
pub struct WindowTitleSource {
    pump: Option<PumpHandle>,
    installer: HookInstaller,
}

impl WindowTitleSource {
    pub fn new() -> Self {
        Self {
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
}

impl Default for WindowTitleSource {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "windows")]
thread_local! {
    static TITLE_CTX: std::cell::RefCell<Option<TitleState>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(target_os = "windows")]
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
    let app = foreground_app_for(hwnd).unwrap_or_default();
    TITLE_CTX.with(|cell| {
        if let Some(state) = cell.borrow().as_ref() {
            let placement = EventPlacement::unpersisted(title.len() as u64);
            if let Some(event) = map_window_title(&state.sink, &title, app, placement) {
                let _ = state.sink.emit(event);
            }
        }
    });
}

impl CaptureSource for WindowTitleSource {
    fn modality(&self) -> Modality {
        Modality::WindowTitle
    }

    fn start(&mut self, sink: SinkHandle) -> Result<(), CaptureError> {
        let state = TitleState { sink };
        let installer = self.installer;
        let pump = super::spawn_pump("halvo-window-title".to_string(), move |ready| {
            run_title_hook(state, installer, ready)
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
fn run_title_hook(state: TitleState, installer: HookInstaller, ready: PumpReady) {
    TITLE_CTX.with(|cell| *cell.borrow_mut() = Some(state));
    let hook = installer(EVENT_OBJECT_NAMECHANGE, Some(title_proc));
    super::ensure_message_queue();
    ready.report();
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

    fn no_hook(
        _event: u32,
        _callback: windows::Win32::UI::Accessibility::WINEVENTPROC,
    ) -> HWINEVENTHOOK {
        HWINEVENTHOOK::default()
    }

    #[test]
    fn a_title_hook_that_does_not_install_fails_the_start_with_its_step() {
        let _serial = crate::windows::tests::serial();
        let (sink, _writer) = crate::sink::channel(4);
        let mut source = WindowTitleSource::with_installer(no_hook);
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
