//! Window-title capture via `SetWinEventHook`.

use crate::event::Modality;
use crate::sink::SinkHandle;
use crate::source::{CaptureError, CaptureSource};

use super::{HandOff, Packet, Producer, PumpHandle, PumpReady};

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

/// The packet of a title, or `None` for an empty title or one of whitespace
/// only, so a cleared caption records nothing.
pub(crate) fn title_packet(title: &str, exe_path: String, tick: u32) -> Option<Packet> {
    if title.trim().is_empty() {
        return None;
    }
    Some(Packet {
        kind: Modality::WindowTitle,
        app: exe_path,
        byte_len: title.len() as u64,
        tick,
    })
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

struct TitleState {
    producer: Producer,
}

/// Window-title capture source. `start` installs the name-change hook on its own
/// pump thread; `stop` quits that thread and unhooks.
pub struct WindowTitleSource {
    pump: Option<PumpHandle>,
    handoff: Option<HandOff>,
    installer: HookInstaller,
}

impl WindowTitleSource {
    /// The counts of this source's hand-off queue in its last run.
    pub fn handoff_counts(&self) -> super::HandOffCounts {
        self.handoff
            .as_ref()
            .map(HandOff::counts)
            .unwrap_or_default()
    }

    pub fn new() -> Self {
        Self {
            pump: None,
            handoff: None,
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
    time: u32,
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
            if let Some(packet) = title_packet(&title, app, time) {
                state.producer.push(packet);
            }
        }
    });
}

impl CaptureSource for WindowTitleSource {
    fn modality(&self) -> Modality {
        Modality::WindowTitle
    }

    fn start(&mut self, sink: SinkHandle) -> Result<(), CaptureError> {
        let handoff = HandOff::start(sink, "halvo-window-title-forward")?;
        let state = TitleState {
            producer: handoff.producer(),
        };
        let installer = self.installer;
        let pump = super::spawn_pump("halvo-window-title".to_string(), move |ready| {
            run_title_hook(state, installer, ready)
        })?;
        self.pump = Some(pump);
        self.handoff = Some(handoff);
        Ok(())
    }

    fn stop(&mut self) {
        if let Some(pump) = self.pump.take() {
            pump.stop();
        }
        if let Some(handoff) = &self.handoff {
            handoff.close();
        }
    }
}

#[cfg(target_os = "windows")]
fn run_title_hook(state: TitleState, installer: HookInstaller, ready: PumpReady) {
    TITLE_CTX.with(|cell| *cell.borrow_mut() = Some(state));
    let hook = installer(EVENT_OBJECT_NAMECHANGE, Some(title_proc));
    if hook.is_invalid() {
        let error = super::last_os_error("SetWinEventHook");
        TITLE_CTX.with(|cell| *cell.borrow_mut() = None);
        ready.report(Err(error));
        return;
    }
    super::ensure_message_queue();
    ready.report(Ok(()));
    super::pump_messages();
    // SAFETY: `hook` is the valid handle that this thread installed above.
    unsafe {
        let _ = UnhookWinEvent(hook);
    }
    TITLE_CTX.with(|cell| *cell.borrow_mut() = None);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_non_empty_title_maps_to_a_window_title_packet() {
        let packet = title_packet("Inbox - Mail", "C:\\mail.exe".to_string(), 3)
            .expect("a non-empty title produces a packet");
        assert_eq!(packet.kind, Modality::WindowTitle);
        assert_eq!(packet.app, "C:\\mail.exe");
        assert_eq!(packet.byte_len, 12);
        assert_eq!(packet.tick, 3);
    }

    #[test]
    fn an_empty_or_whitespace_title_produces_no_packet() {
        assert!(title_packet("", "a.exe".to_string(), 3).is_none());
        assert!(title_packet("   ", "a.exe".to_string(), 3).is_none());
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
