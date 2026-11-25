//! Window-title capture via `SetWinEventHook`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::event::{EventClock, FloorEvent};
use crate::floor::exclude::PrivateWindow;
use crate::floor::kind;
use crate::floor::payload::{Title, TitleWhy};
use crate::floor::site::Site;
use crate::floor::Switch;
use crate::secure_input::windows::{ContentReading, PasswordFieldProbe, PasswordGate};
use crate::source::{
    CaptureError, CaptureSource, SourceContext, SourceCounters, SourceHealth, Started,
};

use super::exclusion::{chrome_window_is_private, private_window, Browser};
use super::{DropCounts, Drops, Floor, FrontApp, Lifecycle, OsTick, Producer, PumpReady};

#[cfg(target_os = "windows")]
use super::foreground::{
    is_a_frame_of_the_host, resolve_front, set_win_event_hook, unhook_win_event, HookInstaller,
    HookRemover,
};
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::HWND;
#[cfg(target_os = "windows")]
use windows::Win32::UI::Accessibility::{UnhookWinEvent, HWINEVENTHOOK};
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetForegroundWindow, GetWindowTextW, InternalGetWindowText, KillTimer, SetTimer,
    CHILDID_SELF, EVENT_OBJECT_NAMECHANGE, EVENT_SYSTEM_FOREGROUND, OBJID_WINDOW,
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
/// process with `probe`, the gate's reading of a read of that window, then
/// core-2's title constructor with its reason. Nothing of the text leaves
/// when the decision drops it.
pub(crate) fn title_event_at(
    floor: &Floor,
    probe: &PasswordFieldProbe,
    (front, window): (&FrontApp, PrivateWindow),
    (title, why): (String, TitleWhy),
    tick: OsTick,
) -> Option<FloorEvent> {
    let keep = floor.keep::<kind::WindowTitle>(front, window, probe)?;
    Some(FloorEvent::window_title(
        keep,
        Title::new(title, why, site_of(front.pid)),
        tick.age(),
        EventClock::system(),
    ))
}

/// The title event at the gate's reading of this moment, whatever window it
/// is of: the floor's decision at each reading, for the tests.
#[cfg(test)]
pub(crate) fn title_event(
    floor: &Floor,
    gate: Option<&PasswordGate>,
    (front, window): (&FrontApp, PrivateWindow),
    (title, why): (String, TitleWhy),
    tick: OsTick,
) -> Option<FloorEvent> {
    let probe = gate.map_or_else(PasswordFieldProbe::default, PasswordGate::content_probe);
    title_event_at(floor, &probe, (front, window), (title, why), tick)
}

/// How long a title ask waits, looked at every [`TITLE_LOOK_STEP`], for the
/// gate's read of its own window: twice the detector's attach deadline
/// (`INPUT_STOP_DEADLINE`, 1,000 ms), which waits that long for a first read
/// and keeps a detector whose read comes later. The wait holds no event: the
/// callbacks take the ask and return, and a timer of the title thread looks.
pub(crate) const TITLE_WAIT_BOUND: Duration = Duration::from_millis(2_000);

/// How often the title thread looks at a waiting ask inside the bound.
pub(crate) const TITLE_LOOK_STEP: Duration = Duration::from_millis(16);

/// How often it looks behind the bound: the ask stays while its window is in
/// front, and the first read of that window decides it.
pub(crate) const TITLE_LATE_STEP: Duration = Duration::from_millis(250);

fn ms_of(duration: Duration) -> u32 {
    u32::try_from(duration.as_millis()).unwrap_or(u32::MAX)
}

/// What asked a title of the window in front.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AskBy {
    /// The start of the source.
    Start,
    /// A change of the window in front.
    Front,
    /// A change of the title of the window in front.
    Name,
}

/// A title asked of the window in front.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TitleAsk {
    /// The window, as GetForegroundWindow gave it.
    pub(crate) window: usize,
    pub(crate) why: TitleWhy,
    /// The OS time of the event that asked, which the record carries.
    pub(crate) tick: u32,
    /// The number of the last read that the gate had asked when the title
    /// source told it that the window came in front: only a read numbered
    /// behind it decides; none where no gate runs.
    pub(crate) after: Option<u64>,
    /// The OS time at which the title thread took the ask: the wait's start.
    pub(crate) asked: u32,
}

/// What a look at the waiting ask gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Look {
    /// No ask waits.
    Idle,
    /// The gate's newest read is of the ask's window and counts for it: its
    /// reading decides the title; none where no gate runs, and then the
    /// probe that reads Unknown decides.
    Decide(TitleAsk, Option<ContentReading>),
    /// The ask waits; the next look comes after this many milliseconds.
    Wait(u32),
    /// Another window is in front: the ask ended with nothing written.
    Left,
}

/// The one title ask that waits on the title thread, and the window in front
/// with the number of its switch.
#[derive(Debug, Default)]
pub(crate) struct Waiting {
    ask: Option<TitleAsk>,
    visit: Option<(usize, Option<u64>)>,
}

impl Waiting {
    /// Takes an ask of `window` by the event of OS time `tick`, at `now`. A
    /// change of the window in front, the start, and a change of the title of
    /// a window that no switch named tell the gate of a switch by `switched`,
    /// which asks the gate for a read and answers the number behind which a
    /// read counts; a change of the title of the window of the visit takes
    /// the visit's number. An ask of the window whose ask waits leaves that
    /// ask as it is; an ask of another window takes its place. Answers whether
    /// it took the place of an ask of another window.
    pub(crate) fn ask(
        &mut self,
        (window, by): (usize, AskBy),
        (tick, now): (u32, u32),
        switched: impl FnOnce() -> Option<u64>,
    ) -> bool {
        let after = match self.visit {
            Some((seen, after)) if seen == window && by == AskBy::Name => after,
            _ => switched(),
        };
        self.visit = Some((window, after));
        if self.ask.is_some_and(|waiting| waiting.window == window) {
            return false;
        }
        let why = match by {
            AskBy::Start | AskBy::Front => TitleWhy::AppChange,
            AskBy::Name => TitleWhy::TitleChange,
        };
        self.ask
            .replace(TitleAsk {
                window,
                why,
                tick,
                after,
                asked: now,
            })
            .is_some()
    }

    /// Ends the waiting ask at the source's stop: whether one waited.
    pub(crate) fn stop(&mut self) -> bool {
        self.ask.take().is_some()
    }

    /// Looks at the waiting ask while `front` is the window in front (0 for
    /// none): where the gate's newest read is of the ask's window and
    /// numbered behind its switch, its reading decides it, at once and
    /// whatever came before; another window in front ends it; else it waits,
    /// with no bound while its window stays in front.
    pub(crate) fn look(&mut self, gate: Option<&PasswordGate>, front: usize, now: u32) -> Look {
        let Some(ask) = self.ask else {
            return Look::Idle;
        };
        if front != 0 && front != ask.window {
            self.ask = None;
            return Look::Left;
        }
        let Some(gate) = gate else {
            self.ask = None;
            return Look::Decide(ask, None);
        };
        if front == ask.window {
            if let Some(reading) = gate.content_reading_of(ask.window, ask.after) {
                self.ask = None;
                return Look::Decide(ask, Some(reading));
            }
        }
        if OsTick::new(ask.asked).age_at(now) < TITLE_WAIT_BOUND {
            Look::Wait(ms_of(TITLE_LOOK_STEP))
        } else {
            Look::Wait(ms_of(TITLE_LATE_STEP))
        }
    }
}

/// The counts of a title source's asks since it was made.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TitleWaits {
    /// Asks decided at the event that asked, by a read of the window that
    /// had already landed.
    pub at_once: u64,
    /// Asks decided inside the bound, by a read that landed while they waited.
    pub after_a_wait: u64,
    /// Asks decided behind the bound, by a read that landed while their
    /// window stayed in front.
    pub behind_the_bound: u64,
    /// Asks that an ask of another window took the place of.
    pub replaced: u64,
    /// Asks that ended because another window was in front.
    pub left: u64,
    /// Asks that ended with no read of their window: an ask of another
    /// window took their place, another window was in front, or the source
    /// stopped while they waited.
    pub ended_with_no_read: u64,
    /// The longest time from an ask to its decision, in milliseconds.
    pub longest_wait_ms: u64,
}

#[derive(Debug, Default)]
struct WaitCounts {
    at_once: AtomicU64,
    after_a_wait: AtomicU64,
    behind_the_bound: AtomicU64,
    replaced: AtomicU64,
    left: AtomicU64,
    ended_with_no_read: AtomicU64,
    longest_wait_ms: AtomicU64,
}

impl WaitCounts {
    fn read(&self) -> TitleWaits {
        TitleWaits {
            at_once: self.at_once.load(Ordering::Relaxed),
            after_a_wait: self.after_a_wait.load(Ordering::Relaxed),
            behind_the_bound: self.behind_the_bound.load(Ordering::Relaxed),
            replaced: self.replaced.load(Ordering::Relaxed),
            left: self.left.load(Ordering::Relaxed),
            ended_with_no_read: self.ended_with_no_read.load(Ordering::Relaxed),
            longest_wait_ms: self.longest_wait_ms.load(Ordering::Relaxed),
        }
    }

    /// Counts an ask that ended with no read of its window, in `why`'s count
    /// where it has one.
    fn ended_unread(&self, why: Option<&AtomicU64>) {
        if let Some(count) = why {
            count.fetch_add(1, Ordering::Relaxed);
        }
        self.ended_with_no_read.fetch_add(1, Ordering::Relaxed);
    }

    /// Counts a decision `waited` after its ask, at the ask's own look or at
    /// a later one: at once only at the look of the event that made the ask.
    fn decided(&self, waited: Duration, at_the_ask: bool) {
        let count = if at_the_ask && waited.is_zero() {
            &self.at_once
        } else if waited < TITLE_WAIT_BOUND {
            &self.after_a_wait
        } else {
            &self.behind_the_bound
        };
        count.fetch_add(1, Ordering::Relaxed);
        let waited = u64::try_from(waited.as_millis()).unwrap_or(u64::MAX);
        self.longest_wait_ms.fetch_max(waited, Ordering::Relaxed);
    }
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

    /// A frame of the frame host came in front with `title`, and rule P
    /// wrote no title of it: the rule holds the frame from now on, as a
    /// decision for the frame did before rule P, so that the title of the
    /// window left is new again at the return to it.
    pub(crate) fn passed_by_a_frame(&mut self, window: usize, title: &str) {
        let _ = self.is_new(window, title);
    }
}

struct TitleState {
    producer: Producer,
    floor: Floor,
    gate: Option<Arc<PasswordGate>>,
    last: LastTitle,
    waiting: Waiting,
    counts: Arc<WaitCounts>,
    timer: Option<(usize, u32)>,
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
/// it, since the named pane and the buttons that tell it may not exist yet.
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
            chrome_window_is_private(automation.as_ref()?, window)
        })
    }
}

/// Window-title capture source.
pub struct WindowTitleSource {
    life: Lifecycle,
    drops: Arc<DropCounts>,
    waits: Arc<WaitCounts>,
    gate: Option<Arc<PasswordGate>>,
    installer: HookInstaller,
    remover: HookRemover,
}

impl WindowTitleSource {
    pub fn new() -> Self {
        Self {
            life: Lifecycle::default(),
            drops: Arc::default(),
            waits: Arc::default(),
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

    /// The counts of this source's title asks: how each ended and the
    /// longest wait.
    pub fn waits(&self) -> TitleWaits {
        self.waits.read()
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
        ask_title(front, time, AskBy::Name);
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
    ask_title(unsafe { GetForegroundWindow() }, time, AskBy::Front);
}

/// Takes a title ask of the window in front and looks at it once: the
/// callback returns at once, decided or not.
#[cfg(target_os = "windows")]
fn ask_title(front: HWND, time: u32, by: AskBy) {
    if front.0.is_null() || super::reading_at_stop() {
        return;
    }
    TITLE_CTX.with(|cell| {
        if let Some(state) = cell.borrow_mut().as_mut() {
            let now = super::now_tick();
            let window = front.0 as usize;
            let gate = state.gate.clone();
            let switched = || gate.map(|gate| gate.switched_to(window, now));
            if state.waiting.ask((window, by), (time, now), switched) {
                state.counts.ended_unread(Some(&state.counts.replaced));
            }
            look_at_the_ask(state, now, true);
        }
    });
}

/// The title thread's timer: one more look at the waiting ask.
#[cfg(target_os = "windows")]
unsafe extern "system" fn look_timer(_hwnd: HWND, _message: u32, _id: usize, _time: u32) {
    if super::reading_at_stop() {
        return;
    }
    TITLE_CTX.with(|cell| {
        if let Some(state) = cell.borrow_mut().as_mut() {
            look_at_the_ask(state, super::now_tick(), false);
        }
    });
}

/// One look at the waiting ask: a decision writes the title, or not, by the
/// floor; a wait sets the timer to its step; an end stops the timer. A
/// decision for a window of the frame host, a frame in which Windows shows
/// another program's window, writes no title, whatever the read answered, and
/// counts one event left out under unknown lineage.
#[cfg(target_os = "windows")]
fn look_at_the_ask(state: &mut TitleState, now: u32, at_the_ask: bool) {
    // SAFETY: GetForegroundWindow takes no argument and returns a handle value.
    let front = unsafe { GetForegroundWindow() }.0 as usize;
    match state.waiting.look(state.gate.as_deref(), front, now) {
        Look::Idle => state.stop_timer(),
        Look::Left => {
            state.counts.ended_unread(Some(&state.counts.left));
            state.stop_timer();
        }
        Look::Decide(ask, reading) => {
            state.stop_timer();
            state
                .counts
                .decided(OsTick::new(ask.asked).age_at(now), at_the_ask);
            let window = HWND(ask.window as *mut core::ffi::c_void);
            if is_a_frame_of_the_host(window) {
                let read = read_window_title(window);
                state.last.passed_by_a_frame(ask.window, &read.text);
                state
                    .floor
                    .left_out(crate::floor::exclude::DropReason::UnknownLineage);
            } else {
                decide_title(state, ask, reading);
            }
        }
        Look::Wait(step) => state.start_timer(step),
    }
}

/// The title of the ask's window, decided by `reading`, the gate's reading
/// of a read of that window, or by the probe that reads Unknown where no
/// gate runs.
#[cfg(target_os = "windows")]
fn decide_title(state: &mut TitleState, ask: TitleAsk, reading: Option<ContentReading>) {
    let front = HWND(ask.window as *mut core::ffi::c_void);
    let read = read_window_title(front);
    if !state.last.shows_new(ask.window, &read.text) {
        return;
    }
    let app = resolve_front(front).unwrap_or_default();
    let window = if is_ghost_of_a_hung_window(&class_of(front), app.path.as_deref()) {
        PrivateWindow::Private
    } else {
        told_private(Browser::of(app.path.as_deref()), &read, || {
            CHROME_QUERY.with(|query| query.borrow_mut().ask(front, ask.tick))
        })
    };
    let probe = match (state.gate.as_deref(), reading) {
        (Some(gate), Some(reading)) => gate.probe_of(reading),
        _ => PasswordFieldProbe::default(),
    };
    let made = title_event_at(
        &state.floor,
        &probe,
        (&app, window),
        (read.text, ask.why),
        OsTick::new(ask.tick),
    );
    if let Some(event) = made {
        state.producer.push(event);
    }
}

#[cfg(target_os = "windows")]
impl TitleState {
    /// Sets the thread's look timer to `step` milliseconds, or keeps it where
    /// it runs at that step already.
    fn start_timer(&mut self, step: u32) {
        if self.timer.is_some_and(|(_, set)| set == step) {
            return;
        }
        let id = self.timer.map_or(0, |(id, _)| id);
        // SAFETY: a timer of this thread with no window, whose procedure has
        // the TIMERPROC signature; an id that this thread's SetTimer gave is
        // set again in place.
        let set = unsafe { SetTimer(None, id, step, Some(look_timer)) };
        self.timer = (set != 0).then_some((set, step));
    }

    /// Stops the thread's look timer, where one runs.
    fn stop_timer(&mut self) {
        if let Some((id, _)) = self.timer.take() {
            // SAFETY: `id` is the timer that this thread's SetTimer gave.
            unsafe {
                let _ = KillTimer(None, id);
            }
        }
    }
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
        let counts = Arc::clone(&self.waits);
        self.life
            .begin(&ctx, "halvo-window-title-forward", move |producer| {
                let state = TitleState {
                    producer,
                    floor,
                    gate,
                    last: LastTitle::default(),
                    waiting: Waiting::default(),
                    counts,
                    timer: None,
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
    ask_title(
        unsafe { GetForegroundWindow() },
        super::now_tick(),
        AskBy::Start,
    );
    super::pump_messages(signal);
    TITLE_CTX.with(|cell| {
        if let Some(state) = cell.borrow_mut().as_mut() {
            state.stop_timer();
            if state.waiting.stop() {
                state.counts.ended_unread(None);
            }
            if let Some(gate) = &state.gate {
                gate.no_more_switches();
            }
        }
    });
    signal.report_removal("UnhookWinEvent", remover(front_hook));
    signal.report_removal("UnhookWinEvent", remover(hook));
    TITLE_CTX.with(|cell| *cell.borrow_mut() = None);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The window left, the window that comes in front, and a third window.
    const A: usize = 0x100;
    const B: usize = 0x200;
    const C: usize = 0x300;

    fn answered(is_password: bool) -> crate::secure_input::windows::FocusRead {
        crate::secure_input::windows::FocusRead::Answered {
            is_password,
            protected: false,
            kind: crate::secure_input::windows::Kind::NativeEdit,
        }
    }

    /// A title thread's ask with a gate and a floor of its own: what each
    /// look gives, and whether a decided title is written.
    struct Bench {
        floor: Floor,
        drops: Arc<DropCounts>,
        gate: PasswordGate,
        waiting: Waiting,
    }

    impl Bench {
        fn new() -> Bench {
            use crate::floor::exclude::FloorHandle;
            let drops = Arc::new(DropCounts::default());
            Bench {
                floor: Floor::new(FloorHandle::default(), Arc::clone(&drops)),
                drops,
                gate: PasswordGate::new(),
                waiting: Waiting::default(),
            }
        }

        fn ask(&mut self, window: usize, by: AskBy, tick: u32) -> bool {
            let gate = &self.gate;
            self.waiting.ask((window, by), (tick, tick), || {
                Some(gate.switched_to(window, tick))
            })
        }

        /// One look while `front` is in front: Some(written) where it decided
        /// the title, None where the ask waits or ended.
        fn look(&mut self, front: usize, now: u32) -> Option<bool> {
            match self.waiting.look(Some(&self.gate), front, now) {
                Look::Decide(ask, reading) => {
                    assert_eq!(ask.window, front, "a decision for another window");
                    let probe = reading.map_or_else(PasswordFieldProbe::default, |reading| {
                        self.gate.probe_of(reading)
                    });
                    let app = FrontApp {
                        pid: 9,
                        path: Some("C:\\Apps\\vault.exe".to_string()),
                    };
                    Some(
                        title_event_at(
                            &self.floor,
                            &probe,
                            (&app, PrivateWindow::Unknown),
                            ("Vault".to_string(), ask.why),
                            OsTick::new(ask.tick),
                        )
                        .is_some(),
                    )
                }
                Look::Idle | Look::Wait(_) | Look::Left => None,
            }
        }
    }

    #[test]
    fn order_1_a_read_that_landed_before_its_switch_was_taken_waits_for_the_read_asked_at_it() {
        for (field, written) in [(true, false), (false, true)] {
            let mut bench = Bench::new();
            bench.gate.focus_read_in(Some(A), 900, answered(false));
            bench.gate.focus_read_in(Some(B), 1_004, answered(field));
            bench.ask(B, AskBy::Front, 1_000);
            let before_the_asked_read = bench.look(B, 1_010);
            bench.gate.own_read_in(Some(B), 1_100, answered(field));
            assert_eq!(
                (before_the_asked_read, bench.look(B, 1_116)),
                (None, Some(written)),
                "password field {field}"
            );
        }
    }

    #[test]
    fn a_stale_return_is_decided_by_the_read_asked_at_the_return() {
        let mut bench = Bench::new();
        bench.ask(B, AskBy::Front, 500);
        bench.gate.focus_read_in(Some(B), 510, answered(false));
        let first_visit = bench.look(B, 516);
        bench.ask(A, AskBy::Front, 800);
        let in_a_window_that_gave_no_read = bench.look(A, 900);
        bench.ask(B, AskBy::Front, 1_000);
        let at_the_return = bench.look(B, 1_000);
        let asked = bench.gate.reads_asked_at().is_some_and(|due| due <= 1_100);
        bench.gate.own_read_in(Some(B), 1_100, answered(true));
        let at_the_read_asked_at_the_return = bench.look(B, 1_116);
        assert_eq!(
            (
                first_visit,
                in_a_window_that_gave_no_read,
                at_the_return,
                asked,
                at_the_read_asked_at_the_return
            ),
            (Some(true), None, None, true, Some(false))
        );
    }

    #[test]
    fn a_read_answered_by_the_kept_password_element_decides_no_title_of_the_window_in_front() {
        use crate::floor::exclude::{FloorHandle, NoLineage};
        use crate::floor::Switches;
        use crate::secure_input::windows::{
            answers_for_its_element, with_kept, FieldState, Kept, KeptElement, ReadBy,
        };
        use FieldState::{Clear, Password};
        use KeptElement::{Other, Same};
        let arms = [
            (
                "the kept element by an event",
                Some(1),
                Same,
                Password,
                ReadBy::Event,
                1,
                false,
                true,
            ),
            (
                "the kept element by an own read",
                Some(1),
                Same,
                Password,
                ReadBy::OwnRead,
                1,
                false,
                true,
            ),
            (
                "a read of a password field",
                Some(1),
                Other,
                Password,
                ReadBy::OwnRead,
                1,
                false,
                true,
            ),
            (
                "no kept element",
                None,
                Other,
                Clear,
                ReadBy::OwnRead,
                1,
                false,
                true,
            ),
            (
                "a focus event's own read",
                Some(1),
                Other,
                Clear,
                ReadBy::Event,
                1,
                false,
                true,
            ),
            (
                "a read that lets the kept element go",
                Some(1),
                Other,
                Clear,
                ReadBy::OwnRead,
                2,
                false,
                true,
            ),
            (
                "another element, no move counted",
                Some(1),
                Other,
                Clear,
                ReadBy::OwnRead,
                1,
                false,
                false,
            ),
            (
                "another element, an event in flight",
                Some(1),
                Other,
                Clear,
                ReadBy::OwnRead,
                2,
                true,
                false,
            ),
        ];
        let wrong: Vec<&str> = arms
            .iter()
            .filter(|(_, kept, element, read, by, moves, in_flight, own)| {
                let (state, _) = with_kept(*kept, *element, *read, *by, *moves, *in_flight);
                answers_for_its_element(*element, *read, state) != *own
            })
            .map(|(arm, ..)| *arm)
            .collect();
        assert!(wrong.is_empty(), "arms told otherwise: {wrong:?}");
        let mut bench = Bench::new();
        let mut screen_on = Switches::default();
        screen_on.set(crate::floor::Switch::ScreenActivity, true);
        let frames = Floor::new(
            FloorHandle::new(None, Arc::new(NoLineage), screen_on),
            Arc::new(DropCounts::default()),
        );
        let app = FrontApp {
            pid: 9,
            path: Some("C:\\Apps\\vault.exe".to_string()),
        };
        let frame = |gate: &PasswordGate| {
            super::super::screen::frame_event_in(&frames, Some(gate), &app, (B, false)).is_some()
        };
        bench.gate.focus_read_in(Some(A), 900, answered(true));
        bench.ask(B, AskBy::Front, 1_000);
        let (state, kept) = with_kept(Some(1), Other, Clear, ReadBy::OwnRead, 1, false);
        let window = answers_for_its_element(Other, Clear, state).then_some(B);
        bench.gate.own_read_in(window, 1_100, answered(true));
        let title_at_the_kept_answer = bench.look(B, 1_116);
        let frame_at_the_kept_answer = frame(&bench.gate);
        bench.gate.focus_read_in(Some(B), 1_200, answered(false));
        let title_at_the_event = bench.look(B, 1_216);
        let frame_at_the_event = frame(&bench.gate);
        assert_eq!(
            (
                (state, kept, window),
                title_at_the_kept_answer,
                frame_at_the_kept_answer,
                title_at_the_event,
                frame_at_the_event
            ),
            ((Password, Kept::Leave, None), None, false, Some(true), true),
            "the read answered by the kept element of the window left, then the focus event's \
             read of the window in front: the title and a frame at each"
        );
    }

    #[test]
    fn a_window_that_gives_no_focus_event_is_decided_by_the_read_asked_at_its_switch() {
        let mut bench = Bench::new();
        bench.gate.focus_read_in(Some(A), 900, answered(false));
        bench.ask(B, AskBy::Front, 1_000);
        let asked = bench.gate.reads_asked_at().is_some_and(|due| due <= 1_100);
        let before = bench.look(B, 1_050);
        bench.gate.own_read_in(Some(B), 1_100, answered(false));
        assert_eq!(
            (asked, before, bench.look(B, 1_116)),
            (true, None, Some(true))
        );
    }

    #[test]
    fn order_2_a_switch_before_the_read_waits_and_the_read_of_that_window_decides() {
        use crate::secure_input::windows::{FocusRead, Kind};
        let firefox = FocusRead::Answered {
            is_password: false,
            protected: false,
            kind: Kind::Firefox,
        };
        let cases = [
            (
                "clear-then-password",
                answered(false),
                answered(true),
                false,
            ),
            (
                "no-answer-then-password",
                FocusRead::NoAnswer { protected: false },
                answered(true),
                false,
            ),
            ("password-then-clear", answered(true), answered(false), true),
            (
                "firefox-then-no-answer",
                firefox,
                FocusRead::NoAnswer { protected: false },
                true,
            ),
            (
                "unreadable-then-clear",
                FocusRead::Unreadable,
                answered(false),
                true,
            ),
        ];
        for (case, left, entered, written) in cases {
            let mut bench = Bench::new();
            bench.gate.focus_read_in(Some(A), 900, left);
            bench.ask(B, AskBy::Front, 1_000);
            let at_the_switch = bench.look(B, 1_000);
            bench.gate.focus_read_in(Some(B), 1_010, entered);
            let at_the_read = bench.look(B, 1_016);
            assert_eq!(
                (at_the_switch, at_the_read),
                (None, Some(written)),
                "{case}"
            );
        }
    }

    #[test]
    fn order_3_a_read_that_lands_behind_the_bound_decides_while_its_window_stays_in_front() {
        let mut bench = Bench::new();
        bench.gate.focus_read_in(Some(A), 900, answered(false));
        bench.ask(B, AskBy::Front, 1_000);
        let inside = bench.waiting.look(Some(&bench.gate), B, 2_999);
        let behind = bench.waiting.look(Some(&bench.gate), B, 3_000);
        let much_later = bench.waiting.look(Some(&bench.gate), B, 60_000);
        bench.gate.focus_read_in(Some(B), 60_100, answered(true));
        let decided = bench.look(B, 60_250);
        assert_eq!(
            (inside, behind, much_later, decided),
            (
                Look::Wait(16),
                Look::Wait(250),
                Look::Wait(250),
                Some(false)
            )
        );
        assert_eq!(bench.waiting.look(Some(&bench.gate), B, 60_500), Look::Idle);
    }

    #[test]
    fn order_4_a_read_of_another_window_or_of_none_decides_nothing() {
        let mut bench = Bench::new();
        bench.gate.focus_read_in(Some(A), 900, answered(false));
        bench.ask(B, AskBy::Front, 1_000);
        bench.gate.focus_read_in(Some(A), 1_005, answered(false));
        let at_a_late_read_of_the_window_left = bench.look(B, 1_008);
        bench.gate.focus_read_in(Some(C), 1_010, answered(false));
        let at_a_read_of_a_third_window = bench.look(B, 1_012);
        bench.gate.focus_read_in(None, 1_014, answered(false));
        let at_a_read_of_no_window = bench.look(B, 1_016);
        bench.gate.own_read_in(Some(B), 1_020, answered(true));
        let at_the_own_read_of_the_window = bench.look(B, 1_024);
        assert_eq!(
            (
                at_a_late_read_of_the_window_left,
                at_a_read_of_a_third_window,
                at_a_read_of_no_window,
                at_the_own_read_of_the_window
            ),
            (None, None, None, Some(false))
        );
    }

    #[test]
    fn order_5_a_second_change_inside_the_wait_takes_the_place_of_the_ask() {
        let mut bench = Bench::new();
        bench.gate.focus_read_in(Some(A), 900, answered(false));
        let first = bench.ask(B, AskBy::Front, 1_000);
        let at_b = bench.look(B, 1_000);
        let replaced = bench.ask(C, AskBy::Front, 1_050);
        bench.gate.focus_read_in(Some(B), 1_060, answered(false));
        let at_the_read_of_b = bench.look(C, 1_064);
        bench.gate.focus_read_in(Some(C), 1_070, answered(true));
        let at_the_read_of_c = bench.look(C, 1_080);
        assert_eq!(
            (first, at_b, replaced, at_the_read_of_b, at_the_read_of_c),
            (false, None, true, None, Some(false))
        );
        assert_eq!(bench.waiting.look(Some(&bench.gate), B, 1_100), Look::Idle);
    }

    #[test]
    fn order_6_a_start_before_the_gates_first_read_waits_for_it() {
        let mut bench = Bench::new();
        bench.gate.started_at(1_000);
        bench.gate.focus_read_in(Some(B), 1_002, answered(false));
        bench.ask(B, AskBy::Start, 1_005);
        let before_the_first_read = bench.look(B, 1_005);
        bench.gate.own_read_in(Some(B), 1_400, answered(true));
        let at_the_first_read = bench.look(B, 1_416);
        let mut started = Bench::new();
        started.gate.own_read_in(Some(B), 900, answered(false));
        started.ask(B, AskBy::Start, 1_000);
        let a_read_before_the_start = started.look(B, 1_000);
        started.gate.own_read_in(Some(B), 1_100, answered(false));
        let the_read_asked_at_the_start = started.look(B, 1_116);
        assert_eq!(
            (
                before_the_first_read,
                at_the_first_read,
                a_read_before_the_start,
                the_read_asked_at_the_start
            ),
            (None, Some(false), None, Some(true))
        );
    }

    #[test]
    fn order_7_the_windows_own_element_while_the_focus_is_in_a_child_is_of_no_window() {
        use crate::secure_input::windows::{
            window_of_read, ElementPlace, FocusPlace, ReadBy, ReadElement,
        };
        const EDIT: usize = 0x210;
        const PID: Option<u32> = Some(7);
        let focus = |handle, root| {
            Some(FocusPlace {
                handle,
                root,
                pid: PID,
            })
        };
        let element = |handle: Option<usize>, root: Option<usize>, pid| {
            ReadElement::Read(ElementPlace { handle, root, pid })
        };
        let own_window = element(Some(B), Some(B), PID);
        let cases = [
            (
                "own-window-focus-in-a-child",
                window_of_read(own_window, ReadBy::Event, None, focus(EDIT, B)),
                None,
            ),
            (
                "own-window-focus-on-it",
                window_of_read(own_window, ReadBy::Event, None, focus(B, B)),
                Some(B),
            ),
            (
                "a-child-with-a-handle",
                window_of_read(
                    element(Some(EDIT), Some(B), PID),
                    ReadBy::Event,
                    None,
                    focus(EDIT, B),
                ),
                Some(B),
            ),
            (
                "an-element-of-another-window",
                window_of_read(
                    element(Some(0x110), Some(A), PID),
                    ReadBy::Event,
                    None,
                    focus(EDIT, B),
                ),
                Some(A),
            ),
            (
                "no-handle-by-an-event",
                window_of_read(
                    element(None, None, PID),
                    ReadBy::Event,
                    None,
                    focus(EDIT, B),
                ),
                None,
            ),
            (
                "no-handle-by-an-own-read",
                window_of_read(
                    element(None, None, PID),
                    ReadBy::OwnRead,
                    focus(EDIT, B),
                    focus(EDIT, B),
                ),
                Some(B),
            ),
            (
                "no-handle-of-another-process",
                window_of_read(
                    element(None, None, Some(8)),
                    ReadBy::OwnRead,
                    focus(EDIT, B),
                    focus(EDIT, B),
                ),
                None,
            ),
            (
                "no-handle-across-a-switch",
                window_of_read(
                    element(None, None, PID),
                    ReadBy::OwnRead,
                    focus(0x110, A),
                    focus(EDIT, B),
                ),
                None,
            ),
            (
                "failed-by-an-event",
                window_of_read(ReadElement::Failed, ReadBy::Event, None, focus(EDIT, B)),
                Some(B),
            ),
            (
                "failed-own-read",
                window_of_read(
                    ReadElement::Failed,
                    ReadBy::OwnRead,
                    focus(EDIT, B),
                    focus(EDIT, B),
                ),
                Some(B),
            ),
            (
                "failed-across-a-switch",
                window_of_read(
                    ReadElement::Failed,
                    ReadBy::OwnRead,
                    focus(0x110, A),
                    focus(EDIT, B),
                ),
                None,
            ),
            (
                "failed-with-no-focus",
                window_of_read(ReadElement::Failed, ReadBy::Event, None, None),
                None,
            ),
        ];
        let wrong: Vec<String> = cases
            .iter()
            .filter(|(_, told, wanted)| told != wanted)
            .map(|(case, told, _)| format!("{case}={told:?}"))
            .collect();
        assert!(wrong.is_empty(), "windows told otherwise: {wrong:?}");
        let mut bench = Bench::new();
        bench.gate.focus_read_in(Some(A), 900, answered(false));
        bench.ask(B, AskBy::Front, 1_000);
        bench.gate.focus_read_in(None, 1_004, answered(false));
        let at_the_windows_own_element = bench.look(B, 1_008);
        bench.gate.own_read_in(Some(B), 1_010, answered(true));
        let at_its_password_field = bench.look(B, 1_016);
        let mut early = Bench::new();
        early.ask(B, AskBy::Front, 1_000);
        early.gate.focus_read_in(Some(B), 1_004, answered(false));
        let a_read_of_the_window_before_the_focus_moved = early.look(B, 1_008);
        assert_eq!(
            (
                at_the_windows_own_element,
                at_its_password_field,
                a_read_of_the_window_before_the_focus_moved
            ),
            (None, Some(false), Some(true))
        );
    }

    #[test]
    fn order_8_a_return_is_decided_by_a_read_made_behind_the_return() {
        let mut bench = Bench::new();
        bench.ask(B, AskBy::Front, 500);
        bench.gate.focus_read_in(Some(B), 600, answered(false));
        let first_visit = bench.look(B, 610);
        bench.ask(A, AskBy::Front, 800);
        let in_a_window_that_gave_no_read = bench.look(A, 900);
        bench.ask(B, AskBy::Front, 1_000);
        let back_at_the_earlier_read = bench.look(B, 1_000);
        bench.gate.focus_read_in(Some(B), 1_010, answered(false));
        let at_a_read_behind_the_return = bench.look(B, 1_016);
        assert_eq!(
            (
                first_visit,
                in_a_window_that_gave_no_read,
                back_at_the_earlier_read,
                at_a_read_behind_the_return
            ),
            (Some(true), None, None, Some(true))
        );
    }

    #[test]
    fn order_9_a_desktop_switch_is_a_reading_of_no_window() {
        let mut bench = Bench::new();
        bench.ask(B, AskBy::Front, 1_000);
        bench.gate.focus_read_in(Some(B), 1_010, answered(false));
        let at_the_switch = bench.look(B, 1_016);
        bench.gate.switch_desktop_at(1_100);
        bench.ask(B, AskBy::Name, 1_200);
        let a_name_change_after_the_desktop_switch = bench.look(B, 1_200);
        bench.gate.focus_read_in(Some(B), 1_300, answered(true));
        let at_the_read_behind_it = bench.look(B, 1_316);
        assert_eq!(
            (
                at_the_switch,
                a_name_change_after_the_desktop_switch,
                at_the_read_behind_it
            ),
            (Some(true), None, Some(false))
        );
        assert_eq!(bench.drops.read().secure_input, 1);
    }

    #[test]
    fn point_4_password_and_a_failed_read_and_firefox_drop_and_no_answer_keeps() {
        use crate::secure_input::windows::{FocusRead, Kind};
        let cases = [
            ("password", answered(true), false),
            ("protected", FocusRead::NoAnswer { protected: true }, false),
            ("failed", FocusRead::Unreadable, false),
            (
                "firefox",
                FocusRead::Answered {
                    is_password: false,
                    protected: false,
                    kind: Kind::Firefox,
                },
                false,
            ),
            ("no-answer", FocusRead::NoAnswer { protected: false }, true),
            ("clear", answered(false), true),
        ];
        for (case, read, written) in cases {
            let mut bench = Bench::new();
            bench.ask(B, AskBy::Front, 1_000);
            bench.gate.focus_read_in(Some(B), 1_010, read);
            assert_eq!(bench.look(B, 1_016), Some(written), "{case}");
        }
    }

    #[test]
    fn the_wait_holds_no_event_and_ends_with_its_window() {
        let mut bench = Bench::new();
        bench.ask(B, AskBy::Front, 1_000);
        let began = std::time::Instant::now();
        let first = bench.waiting.look(Some(&bench.gate), B, 1_000);
        let took = began.elapsed();
        let at_no_window_in_front = bench.waiting.look(Some(&bench.gate), 0, 1_100);
        let another_in_front = bench.waiting.look(Some(&bench.gate), C, 1_200);
        let after = bench.waiting.look(Some(&bench.gate), C, 1_300);
        assert_eq!(
            (first, at_no_window_in_front, another_in_front, after),
            (Look::Wait(16), Look::Wait(16), Look::Left, Look::Idle)
        );
        assert!(took < Duration::from_millis(100), "a look took {took:?}");
        bench.ask(B, AskBy::Front, 1_400);
        let waited_at_the_stop = bench.waiting.stop();
        let after_the_stop = bench.waiting.look(Some(&bench.gate), B, 1_500);
        assert_eq!((waited_at_the_stop, after_the_stop), (true, Look::Idle));
        let counts = WaitCounts::default();
        counts.ended_unread(Some(&counts.replaced));
        counts.ended_unread(Some(&counts.left));
        counts.ended_unread(None);
        counts.decided(Duration::ZERO, true);
        counts.decided(Duration::from_millis(40), false);
        counts.decided(Duration::from_millis(2_500), false);
        assert_eq!(
            counts.read(),
            TitleWaits {
                at_once: 1,
                after_a_wait: 1,
                behind_the_bound: 1,
                replaced: 1,
                left: 1,
                ended_with_no_read: 3,
                longest_wait_ms: 2_500,
            }
        );
    }

    #[test]
    fn a_name_change_while_the_ask_waits_keeps_that_ask_and_its_time() {
        let mut bench = Bench::new();
        bench.ask(B, AskBy::Front, 1_000);
        let replaced = bench.ask(B, AskBy::Name, 1_050);
        bench.gate.focus_read_in(Some(B), 1_020, answered(false));
        let decided = bench.waiting.look(Some(&bench.gate), B, 1_060);
        assert!(!replaced, "a name change took the place of the ask");
        assert!(
            matches!(
                decided,
                Look::Decide(
                    TitleAsk {
                        why: TitleWhy::AppChange,
                        tick: 1_000,
                        ..
                    },
                    Some(_)
                )
            ),
            "{decided:?}"
        );
        let mut later = Bench::new();
        later.gate.focus_read_in(Some(B), 1_200, answered(true));
        later.ask(B, AskBy::Name, 2_000);
        let before_the_asked_read = later.look(B, 2_000);
        later.gate.own_read_in(Some(B), 2_100, answered(true));
        assert_eq!(
            (before_the_asked_read, later.look(B, 2_116)),
            (None, Some(false)),
            "a name change of a window that no switch named, at a password field"
        );
    }

    #[test]
    fn a_title_source_with_no_gate_decides_at_once_and_records_nothing() {
        let mut bench = Bench::new();
        bench.ask(B, AskBy::Front, 1_000);
        let decided = bench.waiting.look(None, B, 1_000);
        assert_eq!(
            decided,
            Look::Decide(
                TitleAsk {
                    window: B,
                    why: TitleWhy::AppChange,
                    tick: 1_000,
                    after: Some(0),
                    asked: 1_000,
                },
                None
            )
        );
    }

    #[test]
    fn the_window_of_a_read_changes_no_answer_of_a_key() {
        use crate::secure_input::windows::FocusRead;
        let reads = [
            answered(true),
            answered(false),
            FocusRead::NoAnswer { protected: false },
            FocusRead::Unreadable,
        ];
        for read in reads {
            let answers: Vec<_> = [None, Some(A), Some(B)]
                .into_iter()
                .map(|window| {
                    let gate = PasswordGate::new();
                    let key = gate.hold(1_000, false, 7);
                    gate.focus_read_in(window, 1_010, read);
                    gate.landing_at(1_012, crate::secure_input::windows::FieldState::Clear);
                    gate.own_read_in(window, 1_130, read);
                    let decided = |now| gate.clearance(&key, now).map(|answer| answer.map(|_| ()));
                    (
                        gate.field_now(),
                        gate.typing_gate(),
                        decided(1_140),
                        decided(1_400),
                    )
                })
                .collect();
            assert!(
                answers.windows(2).all(|pair| pair[0] == pair[1]),
                "{read:?}: {answers:?}"
            );
        }
    }

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
    fn a_failed_read_of_the_password_property_drops_the_title_and_gives_a_key_the_state_of_before()
    {
        use crate::floor::exclude::FloorHandle;
        use crate::secure_input::windows::{
            focus_read_of, state_of, ContentReading, FocusRead, Kind, PasswordProperty,
            ProtectedState, UnknownBy,
        };
        let drops = Arc::new(DropCounts::default());
        let floor = Floor::new(FloorHandle::default(), Arc::clone(&drops));
        let front = FrontApp {
            pid: 9,
            path: Some("C:\\Apps\\mail.exe".to_string()),
        };
        let gate = PasswordGate::new();
        let title_at = |read: FocusRead| {
            gate.focus_read_at(super::super::now_tick(), read, false);
            let kept = title_event(
                &floor,
                Some(&gate),
                (&front, PrivateWindow::Unknown),
                ("Inbox".to_string(), TitleWhy::TitleChange),
                OsTick::new(super::super::now_tick()),
            )
            .is_some();
            (gate.content_reading(), kept)
        };
        let not_protected = ProtectedState::NotProtected;
        let failed = focus_read_of(PasswordProperty::ReadFailed, not_protected, Kind::Other);
        assert_eq!(failed, FocusRead::Unreadable);
        assert_eq!(
            title_at(failed),
            (ContentReading::Unknown(UnknownBy::Unreadable), false),
            "a failed read of the password property drops the title"
        );
        let no_answer = focus_read_of(PasswordProperty::NoAnswer, not_protected, Kind::Other);
        assert_eq!(
            title_at(no_answer),
            (ContentReading::NoAnswer, true),
            "a read with no answer keeps it"
        );
        let failed_protected = focus_read_of(
            PasswordProperty::ReadFailed,
            ProtectedState::Protected,
            Kind::Other,
        );
        assert_eq!(
            title_at(failed_protected),
            (ContentReading::Password, false),
            "a failed read of a protected element"
        );
        for (state, protected) in [
            (ProtectedState::NotProtected, false),
            (ProtectedState::Protected, true),
        ] {
            assert_eq!(
                state_of(
                    focus_read_of(PasswordProperty::ReadFailed, state, Kind::Other),
                    false
                ),
                state_of(FocusRead::NoAnswer { protected }, false),
                "the key's state of a failed read is the one of a read with no answer, \
                 protected {protected}"
            );
        }
        assert_eq!(
            focus_read_of(
                PasswordProperty::Answered(true),
                not_protected,
                Kind::NativeEdit
            ),
            FocusRead::Answered {
                is_password: true,
                protected: false,
                kind: Kind::NativeEdit
            }
        );
        assert_eq!(drops.read().secure_input, 2);
    }

    #[test]
    fn a_protected_state_that_could_not_be_read_drops_the_title_and_gives_a_key_the_state_of_before(
    ) {
        use crate::floor::exclude::FloorHandle;
        use crate::secure_input::windows::{
            focus_read_of, state_of, ClearBy, ContentReading, FocusRead, Kind, PasswordProperty,
            ProtectedState, UnknownBy,
        };
        let drops = Arc::new(DropCounts::default());
        let floor = Floor::new(FloorHandle::default(), Arc::clone(&drops));
        let front = FrontApp {
            pid: 9,
            path: Some("C:\\Apps\\mail.exe".to_string()),
        };
        let gate = PasswordGate::new();
        let title_at = |read: FocusRead| {
            gate.focus_read_at(super::super::now_tick(), read, false);
            let kept = title_event(
                &floor,
                Some(&gate),
                (&front, PrivateWindow::Unknown),
                ("Inbox".to_string(), TitleWhy::TitleChange),
                OsTick::new(super::super::now_tick()),
            )
            .is_some();
            (gate.content_reading(), kept)
        };
        let failed = ProtectedState::ReadFailed;
        let not_said = focus_read_of(PasswordProperty::NoAnswer, failed, Kind::Other);
        assert_eq!(not_said, FocusRead::Unreadable);
        assert_eq!(
            title_at(not_said),
            (ContentReading::Unknown(UnknownBy::Unreadable), false),
            "no answer on the password property and a failed read of the protected state"
        );
        let said_nothing = focus_read_of(
            PasswordProperty::NoAnswer,
            ProtectedState::NotProtected,
            Kind::Other,
        );
        assert_eq!(
            title_at(said_nothing),
            (ContentReading::NoAnswer, true),
            "a program that was asked both and said nothing keeps the title"
        );
        let answered_no =
            focus_read_of(PasswordProperty::Answered(false), failed, Kind::NativeEdit);
        assert_eq!(
            title_at(answered_no),
            (ContentReading::Clear(ClearBy::NativeEdit), true),
            "an element that answered no keeps the title as before this unit"
        );
        for password in [
            PasswordProperty::Answered(true),
            PasswordProperty::Answered(false),
            PasswordProperty::NoAnswer,
            PasswordProperty::ReadFailed,
        ] {
            assert_eq!(
                state_of(focus_read_of(password, failed, Kind::NativeEdit), false),
                state_of(
                    focus_read_of(password, ProtectedState::NotProtected, Kind::NativeEdit),
                    false
                ),
                "the key's state where the protected state could not be read is the one of a \
                 state read as not protected, as before this commit: {password:?}"
            );
        }
        assert_eq!(drops.read().secure_input, 1);
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
        use super::super::exclusion::private_of_names;
        let told =
            |names: &[&str]| private_window(Browser::Chrome, "Notes", || private_of_names(names));
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
    fn a_return_from_a_frame_of_the_host_emits_the_title_again() {
        let mut last = LastTitle::default();
        let first = last.shows_new(1, "Inbox");
        last.passed_by_a_frame(2, "Settings");
        let at_the_return = last.shows_new(1, "Inbox");
        let once_more = last.shows_new(1, "Inbox");
        assert_eq!(
            (first, at_the_return, once_more),
            (true, true, false),
            "a title, the same title at the return from a frame of the host, and again"
        );
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
