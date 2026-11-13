//! Window-title capture via `SetWinEventHook`.

use crate::source::{
    CaptureError, CaptureSource, SourceContext, SourceCounters, SourceHealth, Started,
};

use super::{Lifecycle, Observed, Packet, Producer, PumpReady};

#[cfg(target_os = "windows")]
use super::foreground::{
    resolve_front, set_win_event_hook, unhook_win_event, HookInstaller, HookRemover,
};
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::HWND;
#[cfg(target_os = "windows")]
use windows::Win32::UI::Accessibility::{UnhookWinEvent, HWINEVENTHOOK};
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowTextW, InternalGetWindowText, CHILDID_SELF,
    EVENT_OBJECT_NAMECHANGE, EVENT_SYSTEM_FOREGROUND, OBJID_WINDOW,
};

pub(crate) const TITLE_READ_UNITS: usize = crate::floor::text::TEXT_MAX_BYTES;

pub(crate) fn title_packet(title: &str, exe_path: Option<String>, tick: u32) -> Option<Packet> {
    if title.trim().is_empty() {
        return None;
    }
    Some(Packet::observed(
        Observed::WindowTitle,
        exe_path,
        title.len() as u64,
        tick,
    ))
}

#[cfg(target_os = "windows")]
pub(crate) fn keeps_title_event(window: HWND, object: i32, child: i32, front: HWND) -> bool {
    !front.0.is_null()
        && window == front
        && object == OBJID_WINDOW.0
        && child == CHILDID_SELF as i32
}

#[cfg(target_os = "windows")]
pub(crate) fn read_window_title(hwnd: HWND) -> String {
    if hwnd.0.is_null() {
        return String::new();
    }
    let mut buf = [0u16; TITLE_READ_UNITS + 1];
    // SAFETY: `buf` is a live buffer whose length the call takes from the
    // slice; a handle that is no longer a window gives 0.
    let mut written = usize::try_from(unsafe { GetWindowTextW(hwnd, &mut buf) }).unwrap_or(0);
    if written == 0 {
        // SAFETY: as above; the call reads the stored text and sends no
        // message.
        written = usize::try_from(unsafe { InternalGetWindowText(hwnd, &mut buf) }).unwrap_or(0);
    }
    let written = written.min(TITLE_READ_UNITS);
    let mut title = String::from_utf16_lossy(&buf[..written]);
    let mut end = title.len().min(crate::floor::text::TEXT_MAX_BYTES);
    while !title.is_char_boundary(end) {
        end -= 1;
    }
    title.truncate(end);
    title
}

#[derive(Debug, Default)]
pub(crate) struct LastTitle {
    emitted: Option<Seen>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Seen {
    window: usize,
    digest: TitleDigest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TitleDigest(u64);

impl TitleDigest {
    fn of(title: &str) -> TitleDigest {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        title.hash(&mut hasher);
        TitleDigest(hasher.finish())
    }
}

impl LastTitle {
    pub(crate) fn is_new(&mut self, window: usize, title: &str) -> bool {
        let seen = Some(Seen {
            window,
            digest: TitleDigest::of(title),
        });
        if self.emitted == seen {
            return false;
        }
        self.emitted = seen;
        true
    }

    pub(crate) fn shows_new(&mut self, window: usize, title: &str) -> bool {
        self.is_new(window, title) && !title.trim().is_empty()
    }
}

struct TitleState {
    producer: Producer,
    last: LastTitle,
}

/// Window-title capture source.
pub struct WindowTitleSource {
    life: Lifecycle,
    installer: HookInstaller,
    remover: HookRemover,
}

impl WindowTitleSource {
    pub fn new() -> Self {
        Self {
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
    id_child: i32,
    _thread: u32,
    time: u32,
) {
    // SAFETY: GetForegroundWindow takes no argument and returns a handle value.
    let front = unsafe { GetForegroundWindow() };
    if keeps_title_event(hwnd, id_object, id_child, front) {
        emit_front_title(front, time);
    }
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn front_proc(
    _hook: HWINEVENTHOOK,
    _event: u32,
    _hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _thread: u32,
    time: u32,
) {
    // SAFETY: GetForegroundWindow takes no argument and returns a handle value.
    emit_front_title(unsafe { GetForegroundWindow() }, time);
}

#[cfg(target_os = "windows")]
fn emit_front_title(front: HWND, time: u32) {
    if front.0.is_null() {
        return;
    }
    let title = read_window_title(front);
    TITLE_CTX.with(|cell| {
        if let Some(state) = cell.borrow_mut().as_mut() {
            if !state.last.shows_new(front.0 as usize, &title) {
                return;
            }
            let app = resolve_front(front).and_then(|front| front.path);
            if let Some(packet) = title_packet(&title, app, time) {
                state.producer.push(packet);
            }
        }
    });
}

impl CaptureSource for WindowTitleSource {
    fn begin(&mut self, ctx: SourceContext) -> Result<Started, CaptureError> {
        let installer = self.installer;
        let remover = self.remover;
        self.life
            .begin(&ctx, "halvo-window-title-forward", move |producer| {
                let state = TitleState {
                    producer,
                    last: LastTitle::default(),
                };
                super::spawn_pump("halvo-window-title".to_string(), move |ready, signal| {
                    run_title_hook(state, (installer, remover), ready, &signal)
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
    (installer, remover): (HookInstaller, HookRemover),
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
    let front_hook = installer(EVENT_SYSTEM_FOREGROUND, Some(front_proc));
    if front_hook.is_invalid() {
        let error = super::last_os_error("SetWinEventHook");
        // SAFETY: `hook` is the valid handle that this thread installed above.
        unsafe {
            let _ = UnhookWinEvent(hook);
        }
        TITLE_CTX.with(|cell| *cell.borrow_mut() = None);
        ready.report(Err(error));
        return;
    }
    super::ensure_message_queue();
    ready.report(Ok(()));
    // SAFETY: GetForegroundWindow takes no argument and returns a handle value.
    emit_front_title(unsafe { GetForegroundWindow() }, super::now_tick());
    super::pump_messages(signal);
    signal.report_removal("UnhookWinEvent", remover(front_hook));
    signal.report_removal("UnhookWinEvent", remover(hook));
    TITLE_CTX.with(|cell| *cell.borrow_mut() = None);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Modality;

    #[test]
    fn a_non_empty_title_maps_to_a_window_title_packet() {
        let packet = title_packet("Inbox - Mail", Some("C:\\mail.exe".to_string()), 3)
            .expect("a non-empty title produces a packet");
        assert_eq!(packet.kind(), Modality::WindowTitle);
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

    unsafe extern "system" fn plain_proc(
        hwnd: HWND,
        msg: u32,
        wparam: windows::Win32::Foundation::WPARAM,
        lparam: windows::Win32::Foundation::LPARAM,
    ) -> windows::Win32::Foundation::LRESULT {
        // SAFETY: the arguments are the ones that the system passed.
        unsafe {
            windows::Win32::UI::WindowsAndMessaging::DefWindowProcW(hwnd, msg, wparam, lparam)
        }
    }

    fn plain_class() -> windows::core::PCWSTR {
        use windows::Win32::UI::WindowsAndMessaging::{RegisterClassW, WNDCLASSW};
        static REGISTERED: std::sync::Once = std::sync::Once::new();
        let name = windows::core::w!("HalvoTitleReadTest");
        REGISTERED.call_once(|| {
            let class = WNDCLASSW {
                lpfnWndProc: Some(plain_proc),
                lpszClassName: name,
                ..Default::default()
            };
            // SAFETY: `class` names a static wide string and a procedure with
            // the WNDPROC signature.
            unsafe { RegisterClassW(&class) };
        });
        name
    }

    impl TitledWindow {
        fn new(title: &[u16]) -> TitledWindow {
            Self::made(title).0
        }

        fn made(title: &[u16]) -> (TitledWindow, bool) {
            use windows::Win32::UI::WindowsAndMessaging::{
                CreateWindowExW, SetWindowTextW, WINDOW_EX_STYLE, WINDOW_STYLE,
            };
            // SAFETY: the class is registered by plain_class, whose procedure
            // is DefWindowProcW; the call makes a hidden top-level window with
            // no parent, menu or creation data.
            let hwnd = unsafe {
                CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    plain_class(),
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
            let set = unsafe { SetWindowTextW(hwnd, windows::core::PCWSTR(text.as_ptr())) }.is_ok();
            (TitledWindow(hwnd), set)
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
    fn a_repeat_of_the_last_title_of_a_window_is_dropped() {
        let mut last = LastTitle::default();
        assert!(last.is_new(1, "Inbox"));
        assert!(!last.is_new(1, "Inbox"));
        assert!(last.is_new(2, "Inbox"));
        assert!(last.is_new(2, "Inbox (1)"));
        assert!(last.is_new(1, "Inbox"));
    }

    #[test]
    fn a_return_after_a_window_with_no_title_emits_the_title_again() {
        let mut last = LastTitle::default();
        assert!(last.shows_new(1, "Inbox"));
        assert!(!last.shows_new(2, ""));
        assert!(!last.shows_new(2, "   "));
        assert!(last.shows_new(1, "Inbox"));
        assert!(!last.shows_new(1, "Inbox"));
    }

    fn title_facts(units: usize) -> (String, usize, usize) {
        use windows::Win32::UI::WindowsAndMessaging::GetWindowTextLengthW;
        let long: Vec<u16> = std::iter::repeat_n(u16::from(b'x'), units).collect();
        let (window, set) = TitledWindow::made(&long);
        // SAFETY: the calls read the text of the window made above, into live
        // buffers whose lengths they take from the slices.
        let (held, capped, whole, internal) = unsafe {
            let held = usize::try_from(GetWindowTextLengthW(window.0)).unwrap_or(0);
            let mut capped = [0u16; TITLE_READ_UNITS + 1];
            let capped = GetWindowTextW(window.0, &mut capped);
            let mut whole = vec![0u16; held + 1];
            let whole = GetWindowTextW(window.0, &mut whole);
            let mut internal = [0u16; TITLE_READ_UNITS + 1];
            let internal = InternalGetWindowText(window.0, &mut internal);
            (held, capped, whole, internal)
        };
        let read = read_window_title(window.0);
        let facts = format!(
            "units={units} set={set} held={held} get_window_text_capped={capped} \
             get_window_text_whole={whole} internal_get_window_text_capped={internal} \
             read_bytes={}",
            read.len()
        );
        (facts, held, read.len())
    }

    #[test]
    fn a_title_of_100000_units_is_read_up_to_the_cap() {
        let mut lines = Vec::new();
        let mut failing = Vec::new();
        let mut longest_held = 0;
        for units in [100, 1_024, 1_025, 2_000, 32_767, 100_000] {
            let (facts, held, read) = title_facts(units);
            longest_held = longest_held.max(held);
            let wanted = held.min(crate::floor::text::TEXT_MAX_BYTES);
            if read != wanted {
                failing.push(units);
            }
            eprintln!("TITLE-READ {facts}");
            lines.push(facts);
        }
        let facts = lines.join("; ");
        assert!(
            longest_held > crate::floor::text::TEXT_MAX_BYTES,
            "INCONCLUSIVE: the system held no title above the cap: {facts}"
        );
        assert!(
            failing.is_empty(),
            "the read gave other than the held title cut at the cap for {failing:?} units: {facts}"
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

    fn removal_refused(_hook: HWINEVENTHOOK) -> windows::core::Result<()> {
        Err(windows::core::Error::from_hresult(
            windows::Win32::Foundation::ERROR_INVALID_HANDLE.to_hresult(),
        ))
    }

    #[test]
    fn a_title_source_whose_unhook_fails_answers_the_os_error_of_the_unhook() {
        let _serial = crate::windows::tests::serial();
        let (sink, _writer) = crate::sink::channel(4);
        let mut source = WindowTitleSource::with_remover(removal_refused);
        let started = source.begin(crate::source::SourceContext::new(sink));
        let stopped = source.shutdown();
        assert!(started.is_ok(), "{started:?}");
        assert_eq!(
            stopped,
            Err(CaptureError::Os {
                stage: "UnhookWinEvent",
                domain: "HRESULT",
                code: i64::from(
                    windows::Win32::Foundation::ERROR_INVALID_HANDLE
                        .to_hresult()
                        .0
                ),
            })
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
