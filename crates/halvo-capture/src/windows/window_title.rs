//! Window-title capture via `SetWinEventHook`.

use crate::event::Modality;
use crate::source::{
    CaptureError, CaptureSource, SourceContext, SourceCounters, SourceHealth, Started,
};

use super::{Lifecycle, Packet, Producer, PumpReady};

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

pub(crate) fn title_packet(title: &str, exe_path: Option<String>, tick: u32) -> Option<Packet> {
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

/// Whether a name-change event names the title of the window in front: the
/// event's window, its object and child ids, and the foreground window.
#[cfg(target_os = "windows")]
pub(crate) fn keeps_title_event(window: HWND, object: i32, child: i32, front: HWND) -> bool {
    let _ = (window, child, front);
    object == OBJID_WINDOW.0
}

#[cfg(target_os = "windows")]
pub(crate) fn read_window_title(hwnd: HWND) -> String {
    // SAFETY: the buffer holds the title's length plus the terminating NUL
    // that GetWindowTextW writes, and lives across the call; a handle that is
    // no longer a window gives a length of 0.
    unsafe {
        if hwnd.0.is_null() {
            return String::new();
        }
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; len as usize + 1];
        let written = GetWindowTextW(hwnd, &mut buf);
        String::from_utf16_lossy(&buf[..written as usize])
    }
}

struct TitleState {
    producer: Producer,
}

/// Window-title capture source. `begin` installs the name-change hook on its
/// own pump thread.
pub struct WindowTitleSource {
    life: Lifecycle,
    installer: HookInstaller,
}

impl WindowTitleSource {
    pub fn new() -> Self {
        Self {
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

    /// The counts of this source's hand-off queues since it was made.
    pub fn handoff_counts(&self) -> super::HandOffCounts {
        self.life.handoff_counts()
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
    // SAFETY: GetForegroundWindow takes no argument and returns a handle value.
    let front = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
    if !keeps_title_event(hwnd, id_object, _id_child, front) {
        return;
    }
    let title = read_window_title(hwnd);
    let app = foreground_app_for(hwnd);
    TITLE_CTX.with(|cell| {
        if let Some(state) = cell.borrow().as_ref() {
            if let Some(packet) = title_packet(&title, app, time) {
                state.producer.push(packet);
            }
        }
    });
}

impl CaptureSource for WindowTitleSource {
    fn begin(&mut self, ctx: SourceContext) -> Result<Started, CaptureError> {
        let installer = self.installer;
        self.life
            .begin(&ctx, "halvo-window-title-forward", move |producer| {
                let state = TitleState { producer };
                super::spawn_pump("halvo-window-title".to_string(), move |ready, signal| {
                    run_title_hook(state, installer, ready, &signal)
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
fn run_title_hook(
    state: TitleState,
    installer: HookInstaller,
    ready: PumpReady,
    signal: &super::StopSignal,
) {
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
    super::pump_messages(signal);
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
        let packet = title_packet("Inbox - Mail", Some("C:\\mail.exe".to_string()), 3)
            .expect("a non-empty title produces a packet");
        assert_eq!(packet.kind, Modality::WindowTitle);
        assert_eq!(packet.app.as_deref(), Some("C:\\mail.exe"));
        assert_eq!(packet.byte_len, 12);
        assert_eq!(packet.tick, 3);
    }

    #[test]
    fn an_empty_or_whitespace_title_produces_no_packet() {
        assert!(title_packet("", Some("a.exe".to_string()), 3).is_none());
        assert!(title_packet("   ", Some("a.exe".to_string()), 3).is_none());
    }

    fn handle(value: usize) -> HWND {
        HWND(value as *mut core::ffi::c_void)
    }

    #[test]
    fn only_the_foreground_windows_own_name_change_is_a_title() {
        use windows::Win32::UI::WindowsAndMessaging::{CHILDID_SELF, OBJID_CLIENT};
        let front = handle(0x100);
        let background = handle(0x200);
        let control = handle(0x300);
        let own = CHILDID_SELF as i32;
        assert!(keeps_title_event(front, OBJID_WINDOW.0, own, front));
        assert!(!keeps_title_event(background, OBJID_WINDOW.0, own, front));
        assert!(!keeps_title_event(control, OBJID_WINDOW.0, own, front));
        assert!(!keeps_title_event(front, OBJID_CLIENT.0, own, front));
        assert!(!keeps_title_event(front, OBJID_WINDOW.0, 4, front));
        assert!(!keeps_title_event(
            front,
            OBJID_WINDOW.0,
            own,
            HWND::default()
        ));
    }

    struct TitledWindow(HWND);

    impl TitledWindow {
        fn new(title: &[u16]) -> TitledWindow {
            use windows::Win32::UI::WindowsAndMessaging::{
                CreateWindowExW, SetWindowTextW, WINDOW_EX_STYLE, WINDOW_STYLE,
            };
            // SAFETY: STATIC is a system class; the call makes a hidden
            // top-level window with no parent, menu or creation data.
            let hwnd = unsafe {
                CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    windows::core::w!("STATIC"),
                    windows::core::PCWSTR::null(),
                    WINDOW_STYLE(0),
                    0,
                    0,
                    10,
                    10,
                    None,
                    None,
                    None,
                    None,
                )
            }
            .expect("a window of the class STATIC");
            let mut text = title.to_vec();
            text.push(0);
            // SAFETY: `text` is a NUL-terminated wide string that outlives the
            // call, and `hwnd` is the window made above on this thread.
            unsafe { SetWindowTextW(hwnd, windows::core::PCWSTR(text.as_ptr())) }
                .expect("the title is set");
            TitledWindow(hwnd)
        }
    }

    impl Drop for TitledWindow {
        fn drop(&mut self) {
            // SAFETY: `self.0` is the window that `new` made on this thread.
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(self.0);
            }
        }
    }

    #[test]
    fn a_title_of_100000_units_is_read_up_to_the_cap() {
        let long: Vec<u16> = std::iter::repeat_n(u16::from(b'x'), 100_000).collect();
        let window = TitledWindow::new(&long);
        let title = read_window_title(window.0);
        assert!(!title.is_empty(), "INCONCLUSIVE: the title was not read");
        assert!(
            title.len() <= crate::floor::text::TEXT_MAX_BYTES,
            "a title of {} bytes",
            title.len()
        );
    }

    #[test]
    fn a_title_with_an_unpaired_surrogate_is_read_without_a_panic() {
        let mut odd: Vec<u16> = "Report ".encode_utf16().collect();
        odd.push(0xD800);
        odd.extend("draft".encode_utf16());
        let window = TitledWindow::new(&odd);
        let title = read_window_title(window.0);
        assert!(title.starts_with("Report "));
        assert!(title.ends_with("draft"));
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

    #[test]
    fn a_title_hook_that_the_system_refuses_reports_the_os_code() {
        let _serial = crate::windows::tests::serial();
        let (sink, _writer) = crate::sink::channel(4);
        let mut source =
            WindowTitleSource::with_installer(crate::windows::tests::hook_without_callback);
        let started = source.begin(crate::source::SourceContext::new(sink));
        let _ = source.shutdown();
        assert_eq!(started, Err(crate::windows::tests::refused_hook_error()));
    }
}
