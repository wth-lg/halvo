//! Window-title capture via `SetWinEventHook`.

use std::sync::Arc;

use crate::event::{EventClock, FloorEvent};
use crate::floor::exclude::PrivateWindow;
use crate::floor::kind;
use crate::floor::payload::{Title, TitleWhy};
use crate::floor::site::Site;
use crate::floor::Switch;
use crate::secure_input::windows::{PasswordFieldProbe, PasswordGate};
use crate::source::{
    CaptureError, CaptureSource, SourceContext, SourceCounters, SourceHealth, Started,
};

use super::exclusion::{chrome_window_is_incognito, private_window, Browser};
use super::{DropCounts, Drops, Floor, FrontApp, Lifecycle, OsTick, Producer, PumpReady};

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
    GetClassNameW, GetForegroundWindow, GetWindowTextW, InternalGetWindowText, CHILDID_SELF,
    EVENT_OBJECT_NAMECHANGE, EVENT_SYSTEM_FOREGROUND, OBJID_WINDOW,
};

/// The most UTF-16 units of a window text that the title path reads: twice
/// the longest page title that Chromium keeps (4,096 characters, its
/// `kMaxTitleChars`), with room for a browser's ending. The private check reads
/// the text whole up to this bound, and the floor's entry cuts what it stores.
pub(crate) const TITLE_READ_UNITS: usize = 8_192;

/// A window text as the title path read it: whole up to [`TITLE_READ_UNITS`],
/// and whether the read reached the bound, so that the window may hold more.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReadTitle {
    pub(crate) text: String,
    pub(crate) at_bound: bool,
}

/// The private-window input of a read title: a text of Edge or Firefox that
/// reached the read's bound is told Private, since its ending may lie past
/// the bound; every other text is told by [`private_window`] on the whole text.
pub(crate) fn told_private(
    browser: Browser,
    read: &ReadTitle,
    query: impl FnOnce() -> Option<bool>,
) -> PrivateWindow {
    if read.at_bound && matches!(browser, Browser::Edge | Browser::Firefox) {
        return PrivateWindow::Private;
    }
    private_window(browser, &read.text, query)
}

/// The site of the browser window of process `_pid`: the site reader point of
/// contracts.md C18 item 3, where a later run registers its reader; R1 reads no
/// site. The point is keyed by the process id alone, and one browser process
/// holds both normal and private windows, so a reader registered under the
/// feature `r2` must tell the window itself.
pub(crate) fn site_of(_pid: u32) -> Option<Site> {
    #[cfg(feature = "r2")]
    if let Some(address) = crate::daemon::r2::site_reader(_pid) {
        return Site::cut(address);
    }
    None
}

/// The title event of the window in front: the floor's decision on its
/// process, with the reading of the keyboard source's gate where one runs,
/// then core-2's title constructor with its reason. Nothing of the text
/// leaves when the decision drops it.
pub(crate) fn title_event(
    floor: &Floor,
    gate: Option<&PasswordGate>,
    (front, window): (&FrontApp, PrivateWindow),
    (title, why): (String, TitleWhy),
    tick: OsTick,
) -> Option<FloorEvent> {
    let probe = gate.map_or_else(PasswordFieldProbe::default, PasswordGate::content_probe);
    let keep = floor.keep::<kind::WindowTitle>(front, window, &probe)?;
    Some(FloorEvent::window_title(
        keep,
        Title::new(title, why, site_of(front.pid)),
        tick.age(),
        EventClock::system(),
    ))
}

/// Whether the window in front is the ghost that the system shows for a hung
/// window, by its class name and the file name of its process: the class
/// `Ghost`, or a window of `dwm.exe`, ignoring ASCII case. Such a window may
/// carry the caption of a private window or of a listed app, so its title is
/// dropped as the title of a private window.
pub(crate) fn is_ghost_of_a_hung_window(class: &str, process_file: Option<&str>) -> bool {
    let owner = process_file.map(|path| path.rsplit(['/', '\\']).next().unwrap_or(path));
    class.eq_ignore_ascii_case("Ghost")
        || owner.is_some_and(|file| file.eq_ignore_ascii_case("dwm.exe"))
}

#[cfg(target_os = "windows")]
fn class_of(window: HWND) -> String {
    let mut buf = [0u16; 256];
    // SAFETY: `buf` is a live buffer whose length the call takes from the
    // slice; a handle that is no longer a window gives 0.
    let len = unsafe { GetClassNameW(window, &mut buf) };
    String::from_utf16_lossy(&buf[..usize::try_from(len).unwrap_or(0)])
}

#[cfg(target_os = "windows")]
pub(crate) fn keeps_title_event(window: HWND, object: i32, child: i32, front: HWND) -> bool {
    !front.0.is_null()
        && window == front
        && object == OBJID_WINDOW.0
        && child == CHILDID_SELF as i32
}

#[cfg(target_os = "windows")]
pub(crate) fn read_window_title(hwnd: HWND) -> ReadTitle {
    if hwnd.0.is_null() {
        return ReadTitle {
            text: String::new(),
            at_bound: false,
        };
    }
    let mut buf = vec![0u16; TITLE_READ_UNITS + 1];
    // SAFETY: `buf` is a live buffer whose length the call takes from the
    // slice; a handle that is no longer a window gives 0.
    let mut written = usize::try_from(unsafe { GetWindowTextW(hwnd, &mut buf) }).unwrap_or(0);
    if written == 0 {
        // SAFETY: as above; the call reads the stored text and sends no
        // message.
        written = usize::try_from(unsafe { InternalGetWindowText(hwnd, &mut buf) }).unwrap_or(0);
    }
    let written = written.min(TITLE_READ_UNITS);
    ReadTitle {
        text: String::from_utf16_lossy(&buf[..written]),
        at_bound: written == TITLE_READ_UNITS,
    }
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
    floor: Floor,
    gate: Option<Arc<PasswordGate>>,
    last: LastTitle,
}

/// The most answers of the Chrome query that the title thread keeps.
const CHROME_ANSWERS_KEPT: usize = 256;

/// The answer of the Chrome query for `window`: a kept answer, or `query`. Only
/// an answer of private is kept for the window's life; an answer of not
/// private is asked again at the next title, since a window may show its
/// Incognito button after its first title.
pub(crate) fn kept_answer(
    answers: &mut std::collections::HashMap<usize, bool>,
    window: usize,
    query: impl FnOnce() -> Option<bool>,
) -> Option<bool> {
    if let Some(answer) = answers.get(&window) {
        return Some(*answer);
    }
    let answer = query()?;
    if answer {
        if answers.len() >= CHROME_ANSWERS_KEPT {
            answers.clear();
        }
        answers.insert(window, answer);
    }
    Some(answer)
}

/// How long a Chrome window is told Private after the title path first saw
/// it, since its Incognito button may not exist yet.
const CHROME_YOUNG_MS: u32 = 2_000;

/// The answers of the Chrome query on the title thread, with the OS time at
/// which the title path first saw each window.
#[derive(Debug, Default)]
pub(crate) struct ChromeAnswers {
    answers: std::collections::HashMap<usize, bool>,
    first_seen: std::collections::HashMap<usize, u32>,
}

impl ChromeAnswers {
    /// The answer for `window` at the OS time `now`: no answer, so that its
    /// title is dropped, for the window's first two seconds as the title path
    /// first saw it; after that, [`kept_answer`].
    pub(crate) fn answer(
        &mut self,
        window: usize,
        now: u32,
        query: impl FnOnce() -> Option<bool>,
    ) -> Option<bool> {
        if self.first_seen.len() >= CHROME_ANSWERS_KEPT && !self.first_seen.contains_key(&window) {
            self.first_seen.clear();
        }
        let first = *self.first_seen.entry(window).or_insert(now);
        if now.wrapping_sub(first) < CHROME_YOUNG_MS {
            return None;
        }
        kept_answer(&mut self.answers, window, query)
    }
}

/// The UI Automation query of a Chrome window, made on the title thread, with
/// each answer kept for its window.
#[derive(Default)]
struct ChromeQuery {
    automation: Option<windows::Win32::UI::Accessibility::IUIAutomation>,
    answers: ChromeAnswers,
}

impl ChromeQuery {
    fn ask(&mut self, window: HWND, now: u32) -> Option<bool> {
        use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
        use windows::Win32::UI::Accessibility::CUIAutomation;
        let ChromeQuery {
            automation,
            answers,
        } = self;
        answers.answer(window.0 as usize, now, || {
            if automation.is_none() {
                // SAFETY: CoCreateInstance makes the system's UI Automation
                // object for this thread, which joined the process's MTA at
                // its start.
                *automation =
                    unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }.ok();
            }
            chrome_window_is_incognito(automation.as_ref()?, window)
        })
    }
}

/// Window-title capture source.
pub struct WindowTitleSource {
    life: Lifecycle,
    drops: Arc<DropCounts>,
    gate: Option<Arc<PasswordGate>>,
    installer: HookInstaller,
    remover: HookRemover,
}

impl WindowTitleSource {
    pub fn new() -> Self {
        Self {
            life: Lifecycle::default(),
            drops: Arc::default(),
            gate: None,
            installer: set_win_event_hook,
            remover: unhook_win_event,
        }
    }

    /// A title source that reads a password gate, the one of the service
    /// set or of a keyboard source, by the gate's content reading.
    pub fn with_password_gate(mut self, gate: Arc<PasswordGate>) -> Self {
        self.gate = Some(gate);
        self
    }

    #[cfg(test)]
    pub(crate) fn password_gate(&self) -> Option<&Arc<PasswordGate>> {
        self.gate.as_ref()
    }

    /// The events that this source dropped at the floor's decision.
    pub fn drops(&self) -> Drops {
        self.drops.read()
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
    static CHROME_QUERY: std::cell::RefCell<ChromeQuery> =
        std::cell::RefCell::new(ChromeQuery::default());
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
        emit_front_title(front, time, TitleWhy::TitleChange);
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
    emit_front_title(unsafe { GetForegroundWindow() }, time, TitleWhy::AppChange);
}

#[cfg(target_os = "windows")]
fn emit_front_title(front: HWND, time: u32, why: TitleWhy) {
    if front.0.is_null() || super::reading_at_stop() {
        return;
    }
    let read = read_window_title(front);
    TITLE_CTX.with(|cell| {
        if let Some(state) = cell.borrow_mut().as_mut() {
            if !state.last.shows_new(front.0 as usize, &read.text) {
                return;
            }
            let app = resolve_front(front).unwrap_or_default();
            let window = if is_ghost_of_a_hung_window(&class_of(front), app.path.as_deref()) {
                PrivateWindow::Private
            } else {
                told_private(Browser::of(app.path.as_deref()), &read, || {
                    CHROME_QUERY.with(|query| query.borrow_mut().ask(front, time))
                })
            };
            let made = title_event(
                &state.floor,
                state.gate.as_deref(),
                (&app, window),
                (read.text, why),
                OsTick::new(time),
            );
            if let Some(event) = made {
                state.producer.push(event);
            }
        }
    });
}

/// Leaves the apartment that the title thread joined, after the thread's
/// state, which holds the Chrome query's COM object, is gone.
struct Apartment(bool);

impl Drop for Apartment {
    fn drop(&mut self) {
        TITLE_CTX.with(|cell| *cell.borrow_mut() = None);
        CHROME_QUERY.with(|query| *query.borrow_mut() = ChromeQuery::default());
        if self.0 {
            // SAFETY: this thread's CoInitializeEx succeeded once, and no COM
            // object of this thread is left: the title state was dropped above.
            unsafe { windows::Win32::System::Com::CoUninitialize() };
        }
    }
}

impl CaptureSource for WindowTitleSource {
    fn begin(&mut self, ctx: SourceContext) -> Result<Started, CaptureError> {
        let installer = self.installer;
        let remover = self.remover;
        let floor = Floor::new(ctx.floor().clone(), Arc::clone(&self.drops));
        if !floor.is_on(Switch::TitleText) {
            return self.life.off();
        }
        let gate = self.gate.clone();
        self.life
            .begin(&ctx, "halvo-window-title-forward", move |producer| {
                let state = TitleState {
                    producer,
                    floor,
                    gate,
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
    // SAFETY: CoInitializeEx joins this new thread to the process's MTA; a
    // refusal leaves the Chrome query with no answer.
    let joined = unsafe {
        windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_MULTITHREADED,
        )
    }
    .is_ok();
    let _apartment = Apartment(joined);
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
    emit_front_title(
        unsafe { GetForegroundWindow() },
        super::now_tick(),
        TitleWhy::AppChange,
    );
    super::pump_messages(signal);
    signal.report_removal("UnhookWinEvent", remover(front_hook));
    signal.report_removal("UnhookWinEvent", remover(hook));
    TITLE_CTX.with(|cell| *cell.borrow_mut() = None);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_title_leaves_no_event_while_the_gate_reads_password_or_unknown_and_one_while_it_reads_clear_or_no_answer(
    ) {
        use crate::floor::exclude::FloorHandle;
        use crate::secure_input::windows::{ContentReading, FocusRead, Kind};
        let drops = Arc::new(DropCounts::default());
        let floor = Floor::new(FloorHandle::default(), Arc::clone(&drops));
        let front = FrontApp {
            pid: 9,
            path: Some("C:\\Apps\\mail.exe".to_string()),
        };
        let gate = PasswordGate::new();
        let title = |gate: Option<&PasswordGate>| {
            title_event(
                &floor,
                gate,
                (&front, PrivateWindow::Unknown),
                ("Inbox".to_string(), TitleWhy::TitleChange),
                OsTick::new(super::super::now_tick()),
            )
            .is_some()
        };
        assert!(!title(None), "a title with no gate");
        assert!(!title(Some(&gate)), "a title while the gate reads Unknown");
        let read = |is_password| FocusRead::Answered {
            is_password,
            protected: false,
            kind: Kind::NativeEdit,
        };
        gate.focus_read_at(super::super::now_tick(), read(true), false);
        assert!(!title(Some(&gate)), "a title while the gate reads Password");
        gate.focus_read_at(super::super::now_tick(), read(false), false);
        assert!(title(Some(&gate)), "a title while the gate reads Clear");
        let no_answer = |protected| FocusRead::NoAnswer { protected };
        gate.focus_read_at(super::super::now_tick(), no_answer(false), false);
        assert_eq!(gate.content_reading(), ContentReading::NoAnswer);
        assert!(
            title(Some(&gate)),
            "a title while the program gives no answer"
        );
        gate.focus_read_at(super::super::now_tick(), no_answer(true), false);
        assert!(
            !title(Some(&gate)),
            "a title at no answer with protected set"
        );
        gate.focus_read_at(super::super::now_tick(), no_answer(false), true);
        assert!(!title(Some(&gate)), "a title at no answer in Firefox");
        assert_eq!(drops.read().secure_input, 5);
        let private = title_event(
            &floor,
            None,
            (&front, PrivateWindow::Private),
            ("Inbox".to_string(), TitleWhy::TitleChange),
            OsTick::new(super::super::now_tick()),
        );
        assert!(private.is_none());
        assert_eq!(drops.read().private_window, 1);
    }

    #[test]
    fn a_title_whose_process_was_not_resolved_is_not_kept() {
        use crate::floor::exclude::FloorHandle;
        let drops = Arc::new(DropCounts::default());
        let floor = Floor::new(FloorHandle::default(), Arc::clone(&drops));
        let made = title_event(
            &floor,
            None,
            (&FrontApp::default(), PrivateWindow::Unknown),
            ("Notes".to_string(), TitleWhy::TitleChange),
            OsTick::new(super::super::now_tick()),
        )
        .is_some();
        let unknown_lineage = drops.read().unknown_lineage;
        assert!(
            !made && unknown_lineage == 1,
            "title_record={made} unknown_lineage={unknown_lineage}"
        );
    }

    #[test]
    fn a_title_source_whose_switch_is_off_answers_off() {
        use crate::floor::exclude::{FloorHandle, NoLineage};
        use crate::floor::Switches;
        let mut switches = Switches::default();
        switches.set(Switch::TitleText, false);
        let (sink, _writer) = crate::sink::channel(4);
        let ctx =
            SourceContext::with_floor(sink, FloorHandle::new(None, Arc::new(NoLineage), switches));
        let mut source = WindowTitleSource::new();
        assert_eq!(source.begin(ctx), Ok(Started::Off));
        assert_eq!(source.health().state, crate::source::SourceState::Off);
        assert_eq!(source.shutdown(), Ok(()));
    }

    #[test]
    fn the_ghost_of_a_hung_window_is_told_by_its_class_or_its_owner() {
        let dwm = Some("C:\\Windows\\System32\\dwm.exe");
        let cases = [
            (
                "ghost-of-dwm",
                is_ghost_of_a_hung_window("Ghost", dwm),
                true,
            ),
            (
                "ghost-unknown-owner",
                is_ghost_of_a_hung_window("Ghost", None),
                true,
            ),
            (
                "ghost-other-case",
                is_ghost_of_a_hung_window("GHOST", dwm),
                true,
            ),
            (
                "other-class-of-dwm",
                is_ghost_of_a_hung_window("Halvo", dwm),
                true,
            ),
            (
                "notepad",
                is_ghost_of_a_hung_window("Notepad", Some("C:\\Windows\\notepad.exe")),
                false,
            ),
            (
                "no-owner",
                is_ghost_of_a_hung_window("Notepad", None),
                false,
            ),
        ];
        let wrong: Vec<&str> = cases
            .iter()
            .filter(|(_, told, wanted)| told != wanted)
            .map(|(case, ..)| *case)
            .collect();
        assert!(
            wrong.is_empty(),
            "a window told against its class and owner: {wrong:?}"
        );
    }

    #[test]
    fn a_chrome_window_in_another_language_is_told_private() {
        use super::super::exclusion::incognito_of_names;
        let told =
            |names: &[&str]| private_window(Browser::Chrome, "Notes", || incognito_of_names(names));
        let cases = [
            (
                "german-incognito",
                told(&["Zur\u{fc}ck", "Vorw\u{e4}rts", "Neu laden", "Inkognito"]),
                PrivateWindow::Private,
            ),
            (
                "spanish-incognito",
                told(&[
                    "Atr\u{e1}s",
                    "Adelante",
                    "Volver a cargar",
                    "Inc\u{f3}gnito",
                ]),
                PrivateWindow::Private,
            ),
            (
                "french-incognito",
                told(&[
                    "Pr\u{e9}c\u{e9}dente",
                    "Suivante",
                    "Actualiser",
                    "Navigation priv\u{e9}e",
                ]),
                PrivateWindow::Private,
            ),
            (
                "english-incognito",
                told(&["Back", "Forward", "Reload", "Incognito"]),
                PrivateWindow::Private,
            ),
            (
                "english-plain",
                told(&["Back", "Forward", "Reload", "Person 1"]),
                PrivateWindow::Unknown,
            ),
        ];
        let wrong: Vec<&str> = cases
            .iter()
            .filter(|(_, told, wanted)| told != wanted)
            .map(|(case, ..)| *case)
            .collect();
        let mut answers = std::collections::HashMap::new();
        let first = kept_answer(&mut answers, 7, || Some(false));
        let second = kept_answer(&mut answers, 7, || Some(true));
        let mut chrome = ChromeAnswers::default();
        let young = [10_000, 10_500, 12_000].map(|now| chrome.answer(9, now, || Some(false)));
        assert!(
            wrong.is_empty()
                && first == Some(false)
                && second == Some(true)
                && young == [None, None, Some(false)],
            "told against its window: {wrong:?}; answers for one window: {first:?} then {second:?}; \
             answers in a window's first two seconds and after: {young:?}"
        );
    }

    #[test]
    fn the_site_reader_point_gives_no_site_in_r1() {
        assert!(site_of(std::process::id()).is_none());
        assert!(site_of(0).is_none());
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
            .expect("a window of the class HalvoTitleReadTest");
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

    fn title_facts(units: usize) -> (String, usize, usize, bool) {
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
             read_bytes={} at_bound={}",
            read.text.len(),
            read.at_bound
        );
        (facts, held, read.text.len(), read.at_bound)
    }

    #[test]
    fn a_title_of_100000_units_is_read_up_to_the_cap() {
        let mut lines = Vec::new();
        let mut failing = Vec::new();
        let mut longest_held = 0;
        for units in [100, 1_024, 1_025, 2_000, 32_767, 100_000] {
            let (facts, held, read, at_bound) = title_facts(units);
            longest_held = longest_held.max(held);
            let wanted = held.min(TITLE_READ_UNITS);
            if read != wanted || at_bound != (held >= TITLE_READ_UNITS) {
                failing.push(units);
            }
            eprintln!("TITLE-READ {facts}");
            lines.push(facts);
        }
        let facts = lines.join("; ");
        assert!(
            longest_held > TITLE_READ_UNITS,
            "INCONCLUSIVE: the system held no title above the read's bound: {facts}"
        );
        assert!(
            failing.is_empty(),
            "the read gave other than the held title up to the read's bound for {failing:?} units: {facts}"
        );
    }

    #[test]
    fn a_title_with_an_unpaired_surrogate_is_read_without_a_panic() {
        let mut odd: Vec<u16> = "Report ".encode_utf16().collect();
        odd.push(0xD800);
        odd.extend("draft".encode_utf16());
        let window = TitledWindow::new(&odd);
        let title = read_window_title(window.0).text;
        assert!(title.starts_with("Report "));
        assert!(title.ends_with("draft"));
    }

    /// The text of `window` as the title path reads it, and what the title
    /// path tells of the window for `browser` when the Chrome query answers
    /// nothing.
    fn as_the_title_path_reads(window: HWND, browser: Browser) -> (String, PrivateWindow) {
        let read = read_window_title(window);
        let told = told_private(browser, &read, || None);
        (read.text, told)
    }

    #[test]
    fn a_long_private_title_is_told_before_the_cut() {
        let edge = "[InPrivate] - Microsoft\u{200B} Edge";
        let firefox = "Mozilla Firefox Private Browsing";
        let cases = [
            ("edge-ascii-1000", Browser::Edge, "x".repeat(1_000), edge),
            (
                "edge-cyrillic-500",
                Browser::Edge,
                "\u{436}".repeat(500),
                edge,
            ),
            ("edge-cjk-340", Browser::Edge, "\u{6F22}".repeat(340), edge),
            (
                "firefox-ascii-1000",
                Browser::Firefox,
                "x".repeat(1_000),
                firefox,
            ),
        ];
        let mut kept = Vec::new();
        for (case, browser, page, ending) in cases {
            let text: Vec<u16> = format!("{page} - {ending}").encode_utf16().collect();
            let window = TitledWindow::new(&text);
            let (read, told) = as_the_title_path_reads(window.0, browser);
            if told != PrivateWindow::Private {
                kept.push(format!("{case} read_bytes={} told={told:?}", read.len()));
            }
        }
        assert!(
            kept.is_empty(),
            "a private window was told otherwise: {}",
            kept.join("; ")
        );
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
