//! The Windows probe of `secure_input`: the password-field detector of the
//! keyboard source.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use super::{check, seal, KeystrokeClearance, SecureInputProbe, SecureInputReading, Withheld};

/// The state of the focused element as the detector reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldState {
    /// The element answered that it is no password field.
    Clear,
    /// The element is a password field.
    Password,
    /// No answer, or one that cannot be trusted.
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    NativeEdit,
    Chrome,
    Edge,
    Firefox,
    Other,
}

pub(crate) fn kind_of(framework: Option<&str>, class: Option<&str>, file: Option<&str>) -> Kind {
    let is =
        |value: Option<&str>, wanted: &str| value.is_some_and(|v| v.eq_ignore_ascii_case(wanted));
    if is(file, "firefox.exe") {
        return Kind::Firefox;
    }
    match framework {
        Some("Win32") if is(class, "Edit") => Kind::NativeEdit,
        Some("Chrome") if is(file, "chrome.exe") => Kind::Chrome,
        Some("Chrome") if is(file, "msedge.exe") => Kind::Edge,
        _ => Kind::Other,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FocusRead {
    Answered {
        is_password: bool,
        protected: bool,
        kind: Kind,
    },
    NoAnswer {
        protected: bool,
    },
    Unreadable,
}

pub(crate) fn state_of(read: FocusRead, still_the_last_password_element: bool) -> FieldState {
    if still_the_last_password_element {
        return FieldState::Password;
    }
    match read {
        FocusRead::Answered {
            is_password: true, ..
        }
        | FocusRead::Answered {
            protected: true, ..
        }
        | FocusRead::NoAnswer { protected: true } => FieldState::Password,
        FocusRead::Answered { kind, .. } => match kind {
            Kind::NativeEdit | Kind::Chrome | Kind::Edge => FieldState::Clear,
            Kind::Firefox | Kind::Other => FieldState::Unknown,
        },
        FocusRead::NoAnswer { protected: false } | FocusRead::Unreadable => FieldState::Unknown,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kept {
    Leave,
    Set,
    Drop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeptElement {
    Same,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadBy {
    OwnRead,
    Event,
}

pub(crate) fn with_kept(
    kept: Option<u64>,
    element: KeptElement,
    read: FieldState,
    by: ReadBy,
    moves: u64,
) -> (FieldState, Kept) {
    match kept {
        Some(_) if element == KeptElement::Same => (FieldState::Password, Kept::Leave),
        _ if read == FieldState::Password => (FieldState::Password, Kept::Set),
        None => (read, Kept::Leave),
        Some(_) if by == ReadBy::Event => (read, Kept::Leave),
        Some(at) if moves > at => (read, Kept::Drop),
        Some(_) => (FieldState::Password, Kept::Leave),
    }
}

pub const KEY_HOLD: Duration = Duration::from_millis(120);

/// The largest delay from a key to its focus event that spike S3 measured.
pub const S3_LARGEST_KEY_TO_FOCUS_EVENT: Duration = Duration::from_micros(75_300);

const _: () = assert!(KEY_HOLD.as_micros() > S3_LARGEST_KEY_TO_FOCUS_EVENT.as_micros());

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LatchFrom {
    TheInputThread,
    AnotherThread,
}

/// The time after a latch before which no own read is the latch's base.
pub const LATCH_SETTLE: Duration = Duration::from_millis(100);

/// The step at which the detector reads again while a key stays undecided.
pub const READ_STEP: Duration = Duration::from_millis(30);

const MARKS_KEPT: usize = 256;

/// Whether the element of an own read of the detector is the element of the
/// own read before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Moved {
    /// Both elements were read and compare equal.
    Same,
    /// Both elements were read and compare unequal.
    Another,
    /// An element is missing on either side, or the compare failed.
    NotCompared,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mark {
    Field(FieldState),
    Read { state: FieldState, moved: Moved },
    Latch { from: LatchFrom, at: u32 },
    Started,
}

impl Mark {
    fn state(self) -> Option<FieldState> {
        match self {
            Mark::Field(state) | Mark::Read { state, .. } => Some(state),
            Mark::Started => Some(FieldState::Unknown),
            Mark::Latch { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Entry {
    seq: u64,
    at: Option<u32>,
    began: u32,
    mark: Mark,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct HeldKey {
    tick: u32,
    seen: u64,
    password_edit: bool,
    front: u32,
}

impl HeldKey {
    pub(crate) fn tick(&self) -> u32 {
        self.tick
    }
}

/// The rule that decided a key, for the test seam.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    /// A native password edit had the focus at the key or at the end of its hold.
    PasswordEdit,
    /// Not cleared within 250 ms of its OS time.
    NotClearedInTime,
    /// Another thread came to the front after the key arrived.
    AnotherThreadInFront,
    /// The state that the gate held at the key's input time.
    StateAtTheKey,
    /// A mark after the key that read Password or Unknown.
    MarkAfterTheKey,
    /// An own read after the key's hold read Clear, with every mark between.
    ReadAfterTheHold,
    /// No mouse registration routed the clicks to the gate during the key's
    /// hold.
    NoClickRoute,
    /// An own read from the key to its deciding read, that read included,
    /// showed another element or could not compare the elements.
    ElementNotTheSame,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Judgment {
    Wait(u32),
    Decided(SecureInputReading, Rule),
}

fn ms(duration: Duration) -> u32 {
    u32::try_from(duration.as_millis()).unwrap_or(u32::MAX)
}

fn before(a: u32, b: u32) -> bool {
    let d = b.wrapping_sub(a);
    d != 0 && d <= i32::MAX as u32
}

fn not_after(a: u32, b: u32) -> bool {
    b.wrapping_sub(a) <= i32::MAX as u32
}

fn later_of(a: u32, b: u32) -> u32 {
    if not_after(a, b) {
        b
    } else {
        a
    }
}

fn most_restrictive(a: FieldState, b: FieldState) -> FieldState {
    match (a, b) {
        (FieldState::Password, _) | (_, FieldState::Password) => FieldState::Password,
        (FieldState::Unknown, _) | (_, FieldState::Unknown) => FieldState::Unknown,
        _ => FieldState::Clear,
    }
}

#[derive(Debug, Clone, Copy)]
struct LatchState {
    seq: u64,
    at: u32,
    from: LatchFrom,
    closed_by: Option<Entry>,
}

fn latch_states(marks: &VecDeque<Entry>, first_read_after_start: Option<u64>) -> Vec<LatchState> {
    let reads: Vec<&Entry> = marks
        .iter()
        .filter(|entry| matches!(entry.mark, Mark::Read { .. }))
        .collect();
    let started = marks
        .iter()
        .rev()
        .find(|entry| entry.mark == Mark::Started)
        .map(|entry| entry.seq);
    let mut states = Vec::new();
    let mut floor = 0u64;
    let mut blocked = false;
    for entry in marks.iter() {
        let Mark::Latch { from, at } = entry.mark else {
            continue;
        };
        let mut state = LatchState {
            seq: entry.seq,
            at,
            from,
            closed_by: None,
        };
        if !blocked {
            let settle = at.wrapping_add(ms(LATCH_SETTLE));
            let first_after_start = |read: &Entry| {
                started.is_some_and(|started| entry.seq > started)
                    && first_read_after_start == Some(read.seq)
            };
            let base = reads.iter().find(|read| {
                read.seq > entry.seq
                    && read.seq >= floor
                    && (not_after(settle, read.began) || first_after_start(read))
            });
            let closed = base.and_then(|base| {
                reads.iter().find(|read| {
                    read.seq > base.seq
                        && matches!(
                            read.mark,
                            Mark::Read {
                                moved: Moved::Another,
                                ..
                            }
                        )
                })
            });
            match closed {
                Some(read) => {
                    floor = read.seq;
                    state.closed_by = Some(**read);
                }
                None => blocked = true,
            }
        }
        states.push(state);
    }
    states
}

fn judge(
    marks: &VecDeque<Entry>,
    key: &HeldKey,
    now: u32,
    front: u32,
    password_edit_now: bool,
) -> Judgment {
    if key.password_edit || password_edit_now {
        return Judgment::Decided(SecureInputReading::Active, Rule::PasswordEdit);
    }
    if now.wrapping_sub(key.tick) > ms(super::LATE_KEY_MAX) {
        return Judgment::Decided(SecureInputReading::Unknown, Rule::NotClearedInTime);
    }
    if key.front != front {
        return Judgment::Decided(SecureInputReading::Unknown, Rule::AnotherThreadInFront);
    }
    let after_key =
        |entry: &Entry| entry.seq >= key.seen || entry.at.is_none_or(|at| !before(at, key.tick));
    let started = marks
        .iter()
        .rev()
        .find(|entry| entry.mark == Mark::Started)
        .map(|entry| entry.seq);
    let first_read = started.and_then(|started| {
        marks
            .iter()
            .find(|entry| entry.seq > started && matches!(entry.mark, Mark::Read { .. }))
            .map(|entry| entry.seq)
    });
    let before_first_read = |entry: &Entry| {
        started.is_some_and(|started| entry.seq > started)
            && first_read.is_none_or(|read| entry.seq < read)
    };
    let opening = marks
        .iter()
        .rev()
        .filter(|entry| !after_key(entry))
        .find_map(|entry| {
            let state = entry.mark.state()?;
            let clear_event = matches!(entry.mark, Mark::Field(FieldState::Clear));
            Some(if clear_event && before_first_read(entry) {
                FieldState::Unknown
            } else {
                state
            })
        })
        .unwrap_or(FieldState::Unknown);
    let latches = latch_states(marks, first_read);
    let behind = |latch: &&LatchState| match latch.from {
        LatchFrom::TheInputThread => latch.seq < key.seen,
        LatchFrom::AnotherThread => not_after(latch.at, key.tick),
    };
    let mut floor_tick = key.tick.wrapping_add(ms(KEY_HOLD));
    let mut floor_seq = 0u64;
    let mut open_latch: Option<u32> = None;
    for latch in latches.iter().filter(behind) {
        match latch.closed_by {
            Some(read) => {
                let at = read.at.unwrap_or(read.began);
                floor_tick = later_of(floor_tick, at.wrapping_add(ms(KEY_HOLD)));
                floor_seq = floor_seq.max(read.seq + 1);
            }
            None => {
                open_latch.get_or_insert(latch.at);
            }
        }
    }
    let deciding = if open_latch.is_some() {
        None
    } else {
        marks.iter().find(|entry| {
            entry.seq >= floor_seq
                && matches!(entry.mark, Mark::Read { .. })
                && not_after(floor_tick, entry.began)
        })
    };
    let upto = deciding.map_or(u64::MAX, |read| read.seq);
    let later = marks
        .iter()
        .filter(|entry| after_key(entry) && entry.seq <= upto)
        .filter_map(|entry| entry.mark.state())
        .fold(FieldState::Clear, most_restrictive);
    let moved = marks.iter().any(|entry| {
        after_key(entry)
            && entry.seq <= upto
            && matches!(entry.mark, Mark::Read { moved, .. } if moved != Moved::Same)
    });
    let rule = if opening == FieldState::Clear {
        Rule::MarkAfterTheKey
    } else {
        Rule::StateAtTheKey
    };
    match most_restrictive(opening, later) {
        FieldState::Password => Judgment::Decided(SecureInputReading::Active, rule),
        FieldState::Unknown => Judgment::Decided(SecureInputReading::Unknown, rule),
        FieldState::Clear if deciding.is_some() && moved => {
            Judgment::Decided(SecureInputReading::Unknown, Rule::ElementNotTheSame)
        }
        FieldState::Clear if deciding.is_some() => {
            Judgment::Decided(SecureInputReading::Inactive, Rule::ReadAfterTheHold)
        }
        FieldState::Clear => {
            let last_read = marks
                .iter()
                .rev()
                .find(|entry| matches!(entry.mark, Mark::Read { .. }))
                .map(|entry| entry.began.wrapping_add(ms(READ_STEP)));
            let earliest = match open_latch {
                Some(at) => later_of(now, at.wrapping_add(ms(LATCH_SETTLE))),
                None => floor_tick,
            };
            Judgment::Wait(last_read.map_or(earliest, |step| later_of(earliest, step)))
        }
    }
}

/// The probe of the Windows keyboard source: the gate's reading for one key.
/// Outside the gate's module no probe with a reading of its own can be made:
///
/// ```compile_fail,E0423
/// use halvo_capture::secure_input::windows::PasswordFieldProbe;
/// use halvo_capture::secure_input::SecureInputReading;
///
/// let _ = PasswordFieldProbe(SecureInputReading::Inactive);
/// ```
///
/// The probe that reads Unknown can be made:
///
/// ```
/// use halvo_capture::secure_input::windows::PasswordFieldProbe;
///
/// let _ = PasswordFieldProbe::default();
/// ```
#[derive(Debug, Clone, Copy)]
pub struct PasswordFieldProbe(SecureInputReading);

impl Default for PasswordFieldProbe {
    fn default() -> Self {
        PasswordFieldProbe(SecureInputReading::Unknown)
    }
}

impl seal::Sealed for PasswordFieldProbe {}

impl SecureInputProbe for PasswordFieldProbe {
    fn read(&self) -> SecureInputReading {
        self.0
    }
}

#[derive(Debug, Default)]
struct Marks {
    entries: VecDeque<Entry>,
    next_seq: u64,
    settle_due: Option<u32>,
    read_due: Option<u32>,
    moves: u64,
    stopping: bool,
    clicks_lost: Option<u32>,
    clicks_gap: Option<(u32, u32)>,
}

impl Marks {
    fn clicks_unseen_for(&self, key: u32) -> bool {
        let end = key.wrapping_add(ms(KEY_HOLD));
        self.clicks_lost.is_some_and(|lost| not_after(lost, end))
            || self
                .clicks_gap
                .is_some_and(|(lost, back)| not_after(lost, end) && not_after(key, back))
    }

    fn push(&mut self, at: Option<u32>, began: u32, mark: Mark) {
        if self.entries.len() == MARKS_KEPT {
            self.entries.pop_front();
        }
        self.entries.push_back(Entry {
            seq: self.next_seq,
            at,
            began,
            mark,
        });
        self.next_seq += 1;
    }

    fn ask_read(&mut self, from: u32) {
        self.read_due = Some(match self.read_due {
            Some(due) if not_after(due, from) => due,
            _ => from,
        });
    }
}

#[derive(Debug, Default)]
struct Shared {
    marks: Mutex<Marks>,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Marks> {
        self.marks.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn field(&self, at: u32, state: FieldState) {
        self.field_seen(at, state, None);
    }

    fn field_seen(&self, at: u32, state: FieldState, seen: Option<String>) {
        let mut marks = self.lock();
        marks.push(Some(at), at, Mark::Field(state));
        marks.moves += 1;
        marks.ask_read(at);
        note_mark("event", Some(state), None, Some(at), at, seen);
    }

    fn read_seen(
        &self,
        began: u32,
        at: u32,
        state: FieldState,
        moved: Moved,
        seen: Option<String>,
    ) {
        note_mark("read", Some(state), Some(moved), Some(at), began, seen);
        let mut marks = self.lock();
        marks.push(Some(at), began, Mark::Read { state, moved });
        if marks.settle_due.is_some_and(|due| not_after(due, began)) {
            marks.settle_due = None;
        }
        if marks.read_due.is_some_and(|due| not_after(due, began)) {
            marks.read_due = None;
        }
    }

    fn latch(&self, tick: u32, from: LatchFrom) {
        let what = match from {
            LatchFrom::TheInputThread => "latch-input",
            LatchFrom::AnotherThread => "latch-other",
        };
        note_mark(what, None, None, Some(tick), tick, None);
        let mut marks = self.lock();
        marks.push(None, tick, Mark::Latch { from, at: tick });
        marks.moves += 1;
        marks.settle_due = Some(tick.wrapping_add(ms(LATCH_SETTLE)));
    }

    fn started(&self, at: u32) {
        note_mark("start", Some(FieldState::Unknown), None, Some(at), at, None);
        let mut marks = self.lock();
        *marks = Marks::default();
        marks.push(Some(at), at, Mark::Started);
    }

    fn desktop_switched(&self, tick: u32) {
        self.field(tick, FieldState::Unknown);
        self.latch(tick, LatchFrom::AnotherThread);
    }
}

#[allow(unused_variables)]
fn note_mark(
    what: &'static str,
    state: Option<FieldState>,
    moved: Option<Moved>,
    at: Option<u32>,
    began: u32,
    seen: Option<String>,
) {
    #[cfg(all(feature = "test-seam", target_os = "windows"))]
    crate::windows::input::seam::note_mark(what, state, moved, at, began, seen);
}

pub(crate) fn unreadable_state() -> FieldState {
    state_of(FocusRead::Unreadable, false)
}

/// The gate of the Windows keyboard source and its password-field detector.
#[derive(Debug, Default)]
pub struct PasswordGate {
    shared: Arc<Shared>,
    #[cfg(target_os = "windows")]
    detector: Mutex<Option<detector::Running>>,
    #[cfg(target_os = "windows")]
    stale: Mutex<Option<std::thread::JoinHandle<Result<(), crate::source::CaptureError>>>>,
    #[cfg(target_os = "windows")]
    removals: detector::Removals,
}

impl PasswordGate {
    /// A gate whose detector is not attached.
    pub fn new() -> PasswordGate {
        PasswordGate::default()
    }

    pub(crate) fn hold(&self, tick: u32, password_edit: bool, front: u32) -> HeldKey {
        HeldKey {
            tick,
            seen: self.shared.lock().next_seq,
            password_edit,
            front,
        }
    }

    pub(crate) fn clicks_lost(&self, tick: u32) {
        note_mark(
            "clicks-lost",
            Some(FieldState::Unknown),
            None,
            Some(tick),
            tick,
            None,
        );
        let mut marks = self.shared.lock();
        if marks.clicks_lost.is_none() {
            marks.clicks_lost = Some(tick);
        }
    }

    pub(crate) fn clicks_back(&self, tick: u32) {
        note_mark("clicks-back", None, None, Some(tick), tick, None);
        let mut marks = self.shared.lock();
        if let Some(lost) = marks.clicks_lost.take() {
            marks.clicks_gap = Some((lost, tick));
        }
    }

    pub(crate) fn latch_after(&self, tick: u32) {
        self.latch_from(tick, LatchFrom::AnotherThread);
    }

    pub(crate) fn latch(&self, tick: u32) {
        self.latch_from(tick, LatchFrom::TheInputThread);
    }

    fn latch_from(&self, tick: u32, from: LatchFrom) {
        self.shared.latch(tick, from);
        self.wake_detector();
    }

    fn wake_detector(&self) {
        #[cfg(target_os = "windows")]
        if let Some(detector) = self
            .detector
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
        {
            detector.wake();
        }
    }

    pub(crate) fn decide(
        &self,
        key: &HeldKey,
        now: u32,
        front: u32,
        password_edit_now: bool,
    ) -> Option<(Result<KeystrokeClearance, Withheld>, Rule)> {
        let judged = {
            let mut marks = self.shared.lock();
            if marks.clicks_unseen_for(key.tick) {
                return Some((Err(Withheld::Unknown), Rule::NoClickRoute));
            }
            let judged = judge(&marks.entries, key, now, front, password_edit_now);
            if let Judgment::Wait(from) = judged {
                marks.ask_read(from);
            }
            judged
        };
        let (reading, rule) = match judged {
            Judgment::Decided(reading, rule) => (reading, rule),
            Judgment::Wait(_) => {
                self.wake_detector();
                return None;
            }
        };
        let age = Duration::from_millis(u64::from(now.wrapping_sub(key.tick)));
        let answer = match rule {
            Rule::NotClearedInTime => Err(Withheld::Unknown),
            _ => check(&PasswordFieldProbe(reading), Some(age)),
        };
        Some((answer, rule))
    }

    /// The state that the gate's last field mark holds.
    pub fn field_now(&self) -> FieldState {
        self.shared
            .lock()
            .entries
            .iter()
            .rev()
            .find_map(|entry| entry.mark.state())
            .unwrap_or(FieldState::Unknown)
    }

    #[cfg(test)]
    pub(crate) fn clearance(
        &self,
        key: &HeldKey,
        now: u32,
    ) -> Option<Result<KeystrokeClearance, Withheld>> {
        self.decide(key, now, key.front, false)
            .map(|(answer, _)| answer)
    }

    #[cfg(test)]
    pub(crate) fn field_at(&self, at: u32, state: FieldState) {
        self.shared.field(at, state);
    }

    #[cfg(test)]
    pub(crate) fn read_at(&self, began: u32, state: FieldState, moved: Moved) {
        self.shared.read_seen(began, began, state, moved, None);
    }

    #[cfg(test)]
    pub(crate) fn started_at(&self, at: u32) {
        self.shared.started(at);
    }

    #[cfg(test)]
    pub(crate) fn latch_count(&self) -> usize {
        self.shared
            .lock()
            .entries
            .iter()
            .filter(|entry| matches!(entry.mark, Mark::Latch { .. }))
            .count()
    }
}

#[cfg(target_os = "windows")]
pub(crate) use detector::focus_window_is_password_edit;

#[cfg(target_os = "windows")]
mod detector {
    use std::sync::Arc;

    use core::ffi::c_void;
    use std::sync::atomic::{AtomicU32, Ordering};

    use windows::core::{Interface, GUID, HRESULT};
    use windows::Win32::Foundation::{E_NOINTERFACE, E_POINTER, HANDLE, HWND, S_OK};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_MULTITHREADED,
    };
    use windows::Win32::System::Variant::{VariantClear, VARIANT, VT_BOOL, VT_I4};
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, IUIAutomationCacheRequest, IUIAutomationElement,
        IUIAutomationFocusChangedEventHandler, IUIAutomationFocusChangedEventHandler_Vtbl,
        SetWinEventHook, UIA_FrameworkIdPropertyId, UIA_IsPasswordPropertyId,
        UIA_LegacyIAccessiblePatternId, UIA_LegacyIAccessibleStatePropertyId,
        UIA_ProcessIdPropertyId, UnhookWinEvent, HWINEVENTHOOK,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetClassNameW, GetForegroundWindow, GetGUIThreadInfo, GetWindowLongW,
        GetWindowThreadProcessId, MsgWaitForMultipleObjectsEx, PeekMessageW, TranslateMessage,
        ES_PASSWORD, EVENT_SYSTEM_DESKTOPSWITCH, GUITHREADINFO, GWL_STYLE, MSG,
        MWMO_INPUTAVAILABLE, PM_REMOVE, QS_ALLINPUT, STATE_SYSTEM_PROTECTED, WINEVENT_OUTOFCONTEXT,
    };

    use super::{kind_of, FieldState, FocusRead, PasswordGate, Shared};
    use crate::source::CaptureError;

    fn os(stage: &'static str, error: &windows::core::Error) -> CaptureError {
        CaptureError::Os {
            stage,
            domain: "HRESULT",
            code: i64::from(error.code().0),
        }
    }

    fn tick() -> u32 {
        #[link(name = "kernel32")]
        extern "system" {
            fn GetTickCount() -> u32;
        }
        // SAFETY: GetTickCount takes no argument and reads a counter.
        unsafe { GetTickCount() }
    }

    struct Event(HANDLE);

    // SAFETY: an event handle names a kernel object of the process, which any
    // of its threads may set and wait on.
    unsafe impl Send for Event {}
    // SAFETY: as for Send; SetEvent and a wait on the same event may run on two
    // threads at once.
    unsafe impl Sync for Event {}

    impl Event {
        fn new() -> Result<Event, CaptureError> {
            use windows::Win32::System::Threading::CreateEventW;
            // SAFETY: CreateEventW with no attributes and no name makes a new
            // unnamed auto-reset event, initially unset.
            unsafe { CreateEventW(None, false, false, windows::core::PCWSTR::null()) }
                .map(Event)
                .map_err(|e| os("CreateEventW", &e))
        }

        fn set(&self) {
            // SAFETY: `self.0` is the live event that this value owns.
            unsafe {
                let _ = windows::Win32::System::Threading::SetEvent(self.0);
            }
        }
    }

    impl Drop for Event {
        fn drop(&mut self) {
            // SAFETY: `self.0` is the event that this value owns; it is closed once.
            unsafe {
                let _ = windows::Win32::Foundation::CloseHandle(self.0);
            }
        }
    }

    pub(super) type HandlerRemover =
        fn(&IUIAutomation, &IUIAutomationFocusChangedEventHandler) -> windows::core::Result<()>;

    pub(super) type DesktopHookRemover = fn(HWINEVENTHOOK) -> windows::core::Result<()>;

    #[derive(Clone, Copy)]
    pub(super) struct Removals {
        pub(super) handler: HandlerRemover,
        pub(super) desktop_hook: DesktopHookRemover,
        #[cfg(test)]
        pub(super) before_read: fn(),
    }

    fn remove_handler(
        automation: &IUIAutomation,
        handler: &IUIAutomationFocusChangedEventHandler,
    ) -> windows::core::Result<()> {
        // SAFETY: the handler is the one that `start` added on the calling
        // thread, and both objects are live for the call.
        unsafe { automation.RemoveFocusChangedEventHandler(handler) }
    }

    fn unhook_desktop(hook: HWINEVENTHOOK) -> windows::core::Result<()> {
        // SAFETY: `hook` is the hook that the calling thread installed.
        unsafe { UnhookWinEvent(hook) }.ok()
    }

    impl Default for Removals {
        fn default() -> Self {
            Removals {
                handler: remove_handler,
                desktop_hook: unhook_desktop,
                #[cfg(test)]
                before_read: || {},
            }
        }
    }

    impl std::fmt::Debug for Removals {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("Removals(..)")
        }
    }

    #[derive(Debug)]
    pub(super) struct Running {
        wake: Arc<Event>,
        join: Option<std::thread::JoinHandle<Result<(), CaptureError>>>,
    }

    impl std::fmt::Debug for Event {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("Event(..)")
        }
    }

    impl Running {
        pub(super) fn wake(&self) {
            self.wake.set();
        }
    }

    impl PasswordGate {
        #[cfg(test)]
        pub(super) fn with_removals(removals: Removals) -> PasswordGate {
            let mut gate = PasswordGate::default();
            gate.removals = removals;
            gate
        }

        pub(crate) fn attach(&self) -> Result<(), CaptureError> {
            let mut detector = self
                .detector
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if detector.is_some() {
                return Ok(());
            }
            self.wait_for_the_stale_detector()?;
            self.shared.started(tick());
            let wake = Arc::new(Event::new()?);
            let (ready, started) = std::sync::mpsc::channel();
            let shared = Arc::clone(&self.shared);
            let thread_wake = Arc::clone(&wake);
            let removals = self.removals;
            let join = std::thread::Builder::new()
                .name("halvo-password-detector".to_string())
                .spawn(move || run(&shared, &thread_wake, removals, ready))
                .map_err(|e| CaptureError::Os {
                    stage: "spawn the password detector thread",
                    domain: "io",
                    code: i64::from(e.raw_os_error().unwrap_or(0)),
                })?;
            match started.recv() {
                Ok(Ok(())) => {
                    *detector = Some(Running {
                        wake,
                        join: Some(join),
                    });
                    Ok(())
                }
                Ok(Err(error)) => {
                    let _ = join.join();
                    Err(error)
                }
                Err(_) => {
                    let _ = join.join();
                    Err(CaptureError::ThreadEnded)
                }
            }
        }

        fn wait_for_the_stale_detector(&self) -> Result<(), CaptureError> {
            let mut stale = self
                .stale
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(old) = stale.take() else {
                return Ok(());
            };
            let deadline = crate::windows::INPUT_STOP_DEADLINE;
            let began = std::time::Instant::now();
            while !old.is_finished() && began.elapsed() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            if !old.is_finished() {
                *stale = Some(old);
                return Err(CaptureError::Timeout {
                    stage: "wait for the earlier password detector to end",
                    ms: u64::try_from(deadline.as_millis()).unwrap_or(u64::MAX),
                });
            }
            let _ = old.join();
            Ok(())
        }

        pub(crate) fn detach(&self, deadline: std::time::Duration) -> Result<(), CaptureError> {
            let taken = self
                .detector
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            let Some(mut running) = taken else {
                return Ok(());
            };
            self.shared.lock().stopping = true;
            running.wake.set();
            let began = std::time::Instant::now();
            let Some(join) = running.join.take() else {
                return Ok(());
            };
            while !join.is_finished() {
                if began.elapsed() >= deadline {
                    *self
                        .stale
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(join);
                    return Err(CaptureError::Timeout {
                        stage: "stop the password detector",
                        ms: u64::try_from(deadline.as_millis()).unwrap_or(u64::MAX),
                    });
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            let ended = join.join().unwrap_or(Err(CaptureError::ThreadEnded));
            let mut marks = self.shared.lock();
            marks.stopping = false;
            let at = tick();
            marks.push(Some(at), at, super::Mark::Field(FieldState::Unknown));
            super::note_mark("stop", Some(FieldState::Unknown), None, Some(at), at, None);
            ended
        }
    }

    impl Drop for PasswordGate {
        fn drop(&mut self) {
            let detector = self
                .detector
                .get_mut()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(running) = detector.as_ref() {
                self.shared.lock().stopping = true;
                running.wake.set();
            }
        }
    }

    struct Reader {
        automation: IUIAutomation,
        cache: IUIAutomationCacheRequest,
        kept: std::sync::Mutex<Option<(IUIAutomationElement, u64)>>,
        last_read: std::sync::Mutex<Option<IUIAutomationElement>>,
        #[cfg(feature = "test-seam")]
        seen: std::sync::Mutex<Option<String>>,
    }

    // SAFETY: the objects of the system's UI Automation client are free
    // threaded: any thread of the process's MTA may call them, and the
    // elements that the reader keeps are guarded by their mutexes.
    unsafe impl Send for Reader {}
    // SAFETY: as for Send.
    unsafe impl Sync for Reader {}

    fn variant_bool(value: &mut VARIANT) -> Option<bool> {
        // SAFETY: the tag names the member of the union that the value holds,
        // and VariantClear frees what the value owns once it has been read.
        unsafe {
            let inner = &value.Anonymous.Anonymous;
            let read = (inner.vt == VT_BOOL).then(|| inner.Anonymous.boolVal.0 != 0);
            let _ = VariantClear(value);
            read
        }
    }

    fn variant_i32(value: &mut VARIANT) -> Option<i32> {
        // SAFETY: the tag names the member of the union that the value holds,
        // and VariantClear frees what the value owns once it has been read.
        unsafe {
            let inner = &value.Anonymous.Anonymous;
            let read = (inner.vt == VT_I4).then(|| inner.Anonymous.lVal);
            let _ = VariantClear(value);
            read
        }
    }

    const DEFAULT_IS_NO_ANSWER: bool = true;

    pub(super) fn process_file(pid: Option<i32>) -> Option<String> {
        let path = crate::windows::foreground::process_path(u32::try_from(pid?).ok()?)?;
        path.rsplit(['/', '\\']).next().map(str::to_string)
    }

    fn focus_window_class() -> Option<String> {
        let mut info = GUITHREADINFO {
            cbSize: core::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        // SAFETY: GetForegroundWindow and GetWindowThreadProcessId read handle
        // values; `info` is a live GUITHREADINFO whose size field is set.
        let focus = unsafe {
            let thread = GetWindowThreadProcessId(GetForegroundWindow(), None);
            if thread == 0 || GetGUIThreadInfo(thread, &mut info).is_err() {
                return None;
            }
            info.hwndFocus
        };
        if focus.0.is_null() {
            return None;
        }
        let mut class = [0u16; 64];
        // SAFETY: `class` is a live buffer whose length the call takes.
        let len = unsafe { GetClassNameW(focus, &mut class) };
        let len = usize::try_from(len).unwrap_or(0).min(class.len());
        (len > 0).then(|| String::from_utf16_lossy(&class[..len]))
    }

    impl Reader {
        fn read(&self, element: &IUIAutomationElement) -> FocusRead {
            // SAFETY: `element` is a live element whose cache holds the
            // properties that the cache request names.
            let is_password = unsafe {
                element
                    .GetCachedPropertyValueEx(UIA_IsPasswordPropertyId, DEFAULT_IS_NO_ANSWER)
                    .ok()
                    .and_then(|mut value| variant_bool(&mut value))
            };
            // SAFETY: as above.
            let protected = unsafe {
                element
                    .GetCachedPropertyValueEx(
                        UIA_LegacyIAccessibleStatePropertyId,
                        DEFAULT_IS_NO_ANSWER,
                    )
                    .ok()
                    .and_then(|mut value| variant_i32(&mut value))
            }
            .is_some_and(|state| state as u32 & STATE_SYSTEM_PROTECTED != 0);
            // SAFETY: as above.
            let framework = unsafe { element.CachedFrameworkId() }
                .ok()
                .map(|id| id.to_string())
                .filter(|id| !id.is_empty());
            // SAFETY: as above.
            let file = process_file(unsafe { element.CachedProcessId() }.ok());
            let class = focus_window_class();
            let kind = kind_of(framework.as_deref(), class.as_deref(), file.as_deref());
            #[cfg(feature = "test-seam")]
            self.saw(format!(
                "framework={} window_class={} file={} kind={kind:?} is_password={} protected={}",
                framework.as_deref().unwrap_or("none"),
                class.as_deref().unwrap_or("none"),
                file.as_deref().unwrap_or("none"),
                is_password.map_or("none", |p| if p { "1" } else { "0" }),
                u8::from(protected),
            ));
            match is_password {
                Some(is_password) => FocusRead::Answered {
                    is_password,
                    protected,
                    kind,
                },
                None => FocusRead::NoAnswer { protected },
            }
        }

        #[cfg(feature = "test-seam")]
        fn saw(&self, what: String) {
            *self
                .seen
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(what);
        }

        fn seen(&self) -> Option<String> {
            #[cfg(feature = "test-seam")]
            return self
                .seen
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            #[cfg(not(feature = "test-seam"))]
            None
        }

        fn state(
            &self,
            element: Option<&IUIAutomationElement>,
            by: super::ReadBy,
            moves: u64,
        ) -> FieldState {
            let Some(element) = element else {
                #[cfg(feature = "test-seam")]
                self.saw("no_element".to_string());
                return super::unreadable_state();
            };
            let mut kept = self
                .kept
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let same = kept.as_ref().is_some_and(|(previous, _)| {
                // SAFETY: both elements are live for the call.
                unsafe { self.automation.CompareElements(previous, element) }
                    .is_ok_and(|same| same.as_bool())
            });
            let of_the_kept = if same {
                super::KeptElement::Same
            } else {
                super::KeptElement::Other
            };
            let read = if same {
                #[cfg(feature = "test-seam")]
                self.saw("the_kept_password_element".to_string());
                FieldState::Password
            } else {
                super::state_of(self.read(element), false)
            };
            let (state, change) = super::with_kept(
                kept.as_ref().map(|(_, at)| *at),
                of_the_kept,
                read,
                by,
                moves,
            );
            match change {
                super::Kept::Leave => {}
                super::Kept::Set => *kept = Some((element.clone(), moves)),
                super::Kept::Drop => *kept = None,
            }
            state
        }

        fn own(&self, moves: u64) -> (FieldState, super::Moved) {
            // SAFETY: the cache request is live for the call.
            let element = unsafe { self.automation.GetFocusedElementBuildCache(&self.cache) }.ok();
            let mut last = self
                .last_read
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let moved = match (last.as_ref(), element.as_ref()) {
                (Some(previous), Some(element)) => {
                    // SAFETY: both elements are live for the call.
                    match unsafe { self.automation.CompareElements(previous, element) } {
                        Ok(same) if same.as_bool() => super::Moved::Same,
                        Ok(_) => super::Moved::Another,
                        Err(_) => super::Moved::NotCompared,
                    }
                }
                _ => super::Moved::NotCompared,
            };
            *last = element.clone();
            drop(last);
            (
                self.state(element.as_ref(), super::ReadBy::OwnRead, moves),
                moved,
            )
        }
    }

    #[repr(C)]
    struct FocusHandler {
        vtable: *const IUIAutomationFocusChangedEventHandler_Vtbl,
        refs: AtomicU32,
        shared: Arc<Shared>,
        reader: Arc<Reader>,
        wake: Arc<Event>,
    }

    static HANDLER_VTABLE: IUIAutomationFocusChangedEventHandler_Vtbl =
        IUIAutomationFocusChangedEventHandler_Vtbl {
            base__: windows::core::IUnknown_Vtbl {
                QueryInterface: handler_query,
                AddRef: handler_add_ref,
                Release: handler_release,
            },
            HandleFocusChangedEvent: handler_focus,
        };

    impl FocusHandler {
        fn into_interface(self) -> IUIAutomationFocusChangedEventHandler {
            let raw = Box::into_raw(Box::new(self)) as *mut c_void;
            // SAFETY: `raw` points at a FocusHandler whose first field is the
            // vtable of the interface and whose count holds the one reference
            // that the returned value owns.
            unsafe { IUIAutomationFocusChangedEventHandler::from_raw(raw) }
        }
    }

    unsafe extern "system" fn handler_query(
        this: *mut c_void,
        iid: *const GUID,
        out: *mut *mut c_void,
    ) -> HRESULT {
        if iid.is_null() || out.is_null() {
            return E_POINTER;
        }
        // SAFETY: the system passes a live IID and a live out pointer.
        let wanted = unsafe { *iid };
        if wanted == windows::core::IUnknown::IID
            || wanted == IUIAutomationFocusChangedEventHandler::IID
        {
            // SAFETY: `this` is a live FocusHandler, and `out` is writable.
            unsafe {
                handler_add_ref(this);
                *out = this;
            }
            S_OK
        } else {
            // SAFETY: `out` is writable.
            unsafe { *out = core::ptr::null_mut() };
            E_NOINTERFACE
        }
    }

    unsafe extern "system" fn handler_add_ref(this: *mut c_void) -> u32 {
        // SAFETY: `this` is a live FocusHandler.
        let handler = unsafe { &*(this as *const FocusHandler) };
        handler.refs.fetch_add(1, Ordering::Relaxed) + 1
    }

    unsafe extern "system" fn handler_release(this: *mut c_void) -> u32 {
        // SAFETY: `this` is a live FocusHandler.
        let left = unsafe { &*(this as *const FocusHandler) }
            .refs
            .fetch_sub(1, Ordering::AcqRel)
            - 1;
        if left == 0 {
            // SAFETY: the last reference is gone, and the value was made by
            // Box::new in `into_interface`.
            drop(unsafe { Box::from_raw(this as *mut FocusHandler) });
        }
        left
    }

    unsafe extern "system" fn handler_focus(this: *mut c_void, sender: *mut c_void) -> HRESULT {
        // SAFETY: `this` is a live FocusHandler, and `sender` a borrowed
        // element or null.
        let (handler, element) = unsafe {
            (
                &*(this as *const FocusHandler),
                IUIAutomationElement::from_raw_borrowed(&sender),
            )
        };
        let moves = handler.shared.lock().moves;
        let state = handler.reader.state(element, super::ReadBy::Event, moves);
        handler
            .shared
            .field_seen(tick(), state, handler.reader.seen());
        handler.wake.set();
        S_OK
    }

    thread_local! {
        static DESKTOP_GATE: std::cell::RefCell<Option<Arc<Shared>>> =
            const { std::cell::RefCell::new(None) };
    }

    unsafe extern "system" fn desktop_proc(
        _hook: HWINEVENTHOOK,
        _event: u32,
        _hwnd: HWND,
        _object: i32,
        _child: i32,
        _thread: u32,
        time: u32,
    ) {
        DESKTOP_GATE.with(|cell| {
            if let Some(shared) = cell.borrow().as_ref() {
                shared.desktop_switched(time);
            }
        });
    }

    struct Com;

    impl Drop for Com {
        fn drop(&mut self) {
            // SAFETY: this thread's CoInitializeEx succeeded, once.
            unsafe { CoUninitialize() };
        }
    }

    fn new_reader() -> Result<Reader, CaptureError> {
        // SAFETY: CoCreateInstance makes the system's UI Automation object for
        // this MTA thread.
        let automation: IUIAutomation =
            unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }
                .map_err(|e| os("CoCreateInstance(CUIAutomation)", &e))?;
        // SAFETY: the calls take the live automation object and property ids.
        let cache = unsafe {
            let cache = automation
                .CreateCacheRequest()
                .map_err(|e| os("CreateCacheRequest", &e))?;
            for property in [
                UIA_IsPasswordPropertyId,
                UIA_ProcessIdPropertyId,
                UIA_LegacyIAccessibleStatePropertyId,
                UIA_FrameworkIdPropertyId,
            ] {
                cache
                    .AddProperty(property)
                    .map_err(|e| os("AddProperty", &e))?;
            }
            cache
                .AddPattern(UIA_LegacyIAccessiblePatternId)
                .map_err(|e| os("AddPattern", &e))?;
            cache
        };
        Ok(Reader {
            automation,
            cache,
            kept: std::sync::Mutex::new(None),
            last_read: std::sync::Mutex::new(None),
            #[cfg(feature = "test-seam")]
            seen: std::sync::Mutex::new(None),
        })
    }

    fn start(
        shared: &Arc<Shared>,
        wake: &Arc<Event>,
    ) -> Result<(Arc<Reader>, IUIAutomationFocusChangedEventHandler), CaptureError> {
        let reader = Arc::new(new_reader()?);
        let handler = FocusHandler {
            vtable: &HANDLER_VTABLE,
            refs: AtomicU32::new(1),
            shared: Arc::clone(shared),
            reader: Arc::clone(&reader),
            wake: Arc::clone(wake),
        }
        .into_interface();
        // SAFETY: the cache request and the handler are live COM objects.
        unsafe {
            reader
                .automation
                .AddFocusChangedEventHandler(&reader.cache, &handler)
        }
        .map_err(|e| os("AddFocusChangedEventHandler", &e))?;
        Ok((reader, handler))
    }

    fn own_read(shared: &Shared, reader: &Reader) {
        let began = tick();
        let moves = shared.lock().moves;
        let (state, moved) = reader.own(moves);
        shared.read_seen(began, tick(), state, moved, reader.seen());
    }

    fn run(
        shared: &Arc<Shared>,
        wake: &Arc<Event>,
        removals: Removals,
        ready: std::sync::mpsc::Sender<Result<(), CaptureError>>,
    ) -> Result<(), CaptureError> {
        // SAFETY: CoInitializeEx joins this new thread to the process's MTA.
        let joined = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if let Err(error) = joined.ok() {
            let _ = ready.send(Err(os("CoInitializeEx", &error)));
            return Ok(());
        }
        let _com = Com;
        let (reader, handler) = match start(shared, wake) {
            Ok(started) => started,
            Err(error) => {
                let _ = ready.send(Err(error));
                return Ok(());
            }
        };
        DESKTOP_GATE.with(|cell| *cell.borrow_mut() = Some(Arc::clone(shared)));
        // SAFETY: an out-of-context hook with no module and a callback with the
        // WINEVENTPROC signature, served by this thread's pump.
        let desktop = unsafe {
            SetWinEventHook(
                EVENT_SYSTEM_DESKTOPSWITCH,
                EVENT_SYSTEM_DESKTOPSWITCH,
                None,
                Some(desktop_proc),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            )
        };
        own_read(shared, &reader);
        let _ = ready.send(Ok(()));
        let mut msg = MSG::default();
        loop {
            let (stopping, due) = {
                let marks = shared.lock();
                let due = match (marks.settle_due, marks.read_due) {
                    (Some(a), Some(b)) => Some(if super::not_after(a, b) { a } else { b }),
                    (due, None) | (None, due) => due,
                };
                (marks.stopping, due)
            };
            if stopping {
                break;
            }
            let wait = due.map_or(u32::MAX, |due| {
                let left = due.wrapping_sub(tick());
                if left > i32::MAX as u32 {
                    0
                } else {
                    left
                }
            });
            if wait == 0 {
                #[cfg(test)]
                (removals.before_read)();
                own_read(shared, &reader);
                continue;
            }
            // SAFETY: the slice holds the live wake event, which outlives the
            // wait.
            let _ = unsafe {
                MsgWaitForMultipleObjectsEx(Some(&[wake.0]), wait, QS_ALLINPUT, MWMO_INPUTAVAILABLE)
            };
            // SAFETY: `msg` is a live MSG that PeekMessageW fills, and the same
            // value goes to TranslateMessage and DispatchMessageW.
            unsafe {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
        DESKTOP_GATE.with(|cell| *cell.borrow_mut() = None);
        let mut removed = Ok(());
        if let Err(error) = (removals.handler)(&reader.automation, &handler) {
            removed = removed.and(Err(os("RemoveFocusChangedEventHandler", &error)));
        }
        if !desktop.is_invalid() {
            if let Err(error) = (removals.desktop_hook)(desktop) {
                removed = removed.and(Err(os("UnhookWinEvent", &error)));
            }
        }
        drop(handler);
        drop(reader);
        removed
    }

    pub(crate) fn focus_window_is_password_edit() -> bool {
        let mut info = GUITHREADINFO {
            cbSize: core::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        // SAFETY: GetForegroundWindow and GetWindowThreadProcessId read handle
        // values; `info` is a live GUITHREADINFO whose size field is set.
        let focus = unsafe {
            let thread = GetWindowThreadProcessId(GetForegroundWindow(), None);
            if thread == 0 || GetGUIThreadInfo(thread, &mut info).is_err() {
                return false;
            }
            info.hwndFocus
        };
        if focus.0.is_null() {
            return false;
        }
        let mut class = [0u16; 16];
        // SAFETY: `class` is a live buffer whose length the call takes.
        let len = unsafe { GetClassNameW(focus, &mut class) };
        let len = usize::try_from(len).unwrap_or(0).min(class.len());
        if !String::from_utf16_lossy(&class[..len]).eq_ignore_ascii_case("Edit") {
            return false;
        }
        // SAFETY: GetWindowLongW reads a style value of a window handle.
        let style = unsafe { GetWindowLongW(focus, GWL_STYLE) };
        style as u32 & ES_PASSWORD as u32 != 0
    }

    #[cfg(test)]
    mod tests {
        use super::super::{HeldKey, Judgment, Mark, Moved};
        use super::*;
        use crate::secure_input::{SecureInputReading, Withheld};
        use std::time::Duration;

        fn refused() -> windows::core::Error {
            windows::core::Error::from_hresult(
                windows::Win32::Foundation::ERROR_INVALID_HANDLE.to_hresult(),
            )
        }

        fn handler_refused(
            _automation: &IUIAutomation,
            _handler: &IUIAutomationFocusChangedEventHandler,
        ) -> windows::core::Result<()> {
            Err(refused())
        }

        fn desktop_refused(_hook: HWINEVENTHOOK) -> windows::core::Result<()> {
            Err(refused())
        }

        fn handler_slow(
            automation: &IUIAutomation,
            handler: &IUIAutomationFocusChangedEventHandler,
        ) -> windows::core::Result<()> {
            std::thread::sleep(Duration::from_millis(1_500));
            remove_handler(automation, handler)
        }

        fn refused_at(stage: &'static str) -> CaptureError {
            os(stage, &refused())
        }

        #[test]
        fn after_attach_the_first_mark_is_the_start_and_attach_returns_after_the_detectors_own_read(
        ) {
            let _serial = crate::windows::tests::serial();
            let gate = PasswordGate::new();
            gate.attach().expect("the detector starts");
            let (first, own_read, at) = {
                let marks = gate.shared.lock();
                let first = marks.entries.front().copied();
                let own_read = marks
                    .entries
                    .iter()
                    .any(|entry| matches!(entry.mark, Mark::Read { .. }));
                (
                    first.map(|entry| entry.mark),
                    own_read,
                    first.and_then(|entry| entry.at),
                )
            };
            let at = at.unwrap_or(0);
            let before = HeldKey {
                tick: at,
                seen: 0,
                password_edit: false,
                front: 0,
            };
            let judged = super::super::judge(&gate.shared.lock().entries, &before, at, 0, false);
            let detached = gate.detach(crate::windows::INPUT_STOP_DEADLINE);
            assert_eq!(first, Some(Mark::Started));
            assert!(own_read, "attach returned before the detector's own read");
            assert!(
                matches!(judged, Judgment::Decided(SecureInputReading::Unknown, _)),
                "{judged:?}"
            );
            assert_eq!(detached, Ok(()));
        }

        #[test]
        fn a_reading_with_no_element_reads_unknown_through_the_detectors_reader() {
            let _serial = crate::windows::tests::serial();
            // SAFETY: joins this test thread to the process's MTA; `_com`
            // undoes it after the reader is dropped.
            let joined = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            assert!(joined.is_ok(), "{joined:?}");
            let _com = Com;
            let reader = new_reader().expect("the reader is made");
            let own = reader.state(None, super::super::ReadBy::OwnRead, 0);
            let event = reader.state(None, super::super::ReadBy::Event, 3);
            let seen = reader.seen();
            drop(reader);
            assert_eq!(own, FieldState::Unknown);
            assert_eq!(event, FieldState::Unknown);
            #[cfg(feature = "test-seam")]
            assert_eq!(seen.as_deref(), Some("no_element"));
            #[cfg(not(feature = "test-seam"))]
            assert_eq!(seen, None);
        }

        #[test]
        fn the_desktop_hook_callback_reads_unknown_and_closes_the_gate() {
            let gate = PasswordGate::new();
            gate.read_at(1_000, FieldState::Clear, Moved::NotCompared);
            let before = gate.hold(2_000, false, 0);
            DESKTOP_GATE.with(|cell| *cell.borrow_mut() = Some(Arc::clone(&gate.shared)));
            // SAFETY: the callback reads its thread's gate and no argument but
            // the time.
            unsafe {
                desktop_proc(
                    HWINEVENTHOOK::default(),
                    EVENT_SYSTEM_DESKTOPSWITCH,
                    HWND::default(),
                    0,
                    0,
                    0,
                    2_010,
                );
            }
            DESKTOP_GATE.with(|cell| *cell.borrow_mut() = None);
            let switched = gate.field_now();
            let after = gate.hold(2_020, false, 0);
            gate.read_at(2_140, FieldState::Clear, Moved::Another);
            let decided = |key: &HeldKey| {
                gate.decide(key, 2_150, 0, false)
                    .map(|(answer, _)| answer.map(|_| ()))
            };
            assert_eq!(switched, FieldState::Unknown);
            assert_eq!(decided(&before), Some(Err(Withheld::Unknown)));
            assert_eq!(decided(&after), Some(Err(Withheld::Unknown)));
        }

        struct EditInFront {
            hwnd: usize,
            stop: Arc<std::sync::atomic::AtomicBool>,
            thread: Option<std::thread::JoinHandle<()>>,
        }

        impl EditInFront {
            fn open() -> (EditInFront, bool) {
                use std::sync::atomic::{AtomicBool, Ordering};
                use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
                use windows::Win32::UI::Input::KeyboardAndMouse::{
                    SendInput, SetFocus, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
                    KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VK_MENU,
                };
                use windows::Win32::UI::WindowsAndMessaging::{
                    BringWindowToTop, CreateWindowExW, DestroyWindow, SetForegroundWindow,
                    ShowWindow, SW_SHOW, WINDOW_EX_STYLE, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
                };
                let stop = Arc::new(AtomicBool::new(false));
                let (tx, rx) = std::sync::mpsc::channel();
                let thread_stop = Arc::clone(&stop);
                let thread = std::thread::spawn(move || {
                    let class = windows::core::HSTRING::from("EDIT");
                    // SAFETY: the class name is a live HSTRING; the window
                    // belongs to this thread, which destroys it before it ends.
                    let Ok(hwnd) = (unsafe {
                        CreateWindowExW(
                            WINDOW_EX_STYLE(0),
                            windows::core::PCWSTR(class.as_ptr()),
                            windows::core::PCWSTR::null(),
                            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                            80,
                            80,
                            320,
                            120,
                            None,
                            None,
                            None,
                            None,
                        )
                    }) else {
                        let _ = tx.send((0usize, false));
                        return;
                    };
                    let pump = |span: Duration| {
                        let deadline = std::time::Instant::now() + span;
                        let mut msg = MSG::default();
                        loop {
                            // SAFETY: `msg` is a live MSG that PeekMessageW
                            // fills, and the same value goes to
                            // TranslateMessage and DispatchMessageW.
                            unsafe {
                                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                                    let _ = TranslateMessage(&msg);
                                    DispatchMessageW(&msg);
                                }
                            }
                            let left =
                                deadline.saturating_duration_since(std::time::Instant::now());
                            if left.is_zero() {
                                break;
                            }
                            let left = u32::try_from(left.as_millis()).unwrap_or(u32::MAX).max(1);
                            // SAFETY: no handle is passed; the call waits for
                            // this thread's queue or the time.
                            let _ = unsafe {
                                MsgWaitForMultipleObjectsEx(
                                    None,
                                    left,
                                    QS_ALLINPUT,
                                    MWMO_INPUTAVAILABLE,
                                )
                            };
                        }
                    };
                    let front = || {
                        // SAFETY: GetForegroundWindow takes no argument.
                        let foreground = unsafe { GetForegroundWindow() };
                        foreground == hwnd
                    };
                    let alt = |released: bool| INPUT {
                        r#type: INPUT_KEYBOARD,
                        Anonymous: INPUT_0 {
                            ki: KEYBDINPUT {
                                wVk: VK_MENU,
                                wScan: 0,
                                dwFlags: if released {
                                    KEYEVENTF_KEYUP
                                } else {
                                    KEYBD_EVENT_FLAGS(0)
                                },
                                time: 0,
                                dwExtraInfo: 0,
                            },
                        },
                    };
                    // SAFETY: the calls take this thread's window handle.
                    unsafe {
                        let _ = ShowWindow(hwnd, SW_SHOW);
                        let _ = SetForegroundWindow(hwnd);
                    }
                    pump(Duration::from_millis(200));
                    if !front() {
                        // SAFETY: a slice of one live INPUT value with the
                        // size of one INPUT, and this thread's window handle.
                        unsafe {
                            let _ = SendInput(&[alt(false)], core::mem::size_of::<INPUT>() as i32);
                            let _ = SetForegroundWindow(hwnd);
                            let _ = SendInput(&[alt(true)], core::mem::size_of::<INPUT>() as i32);
                        }
                        pump(Duration::from_millis(200));
                    }
                    if !front() {
                        // SAFETY: GetWindowThreadProcessId takes a handle and
                        // no out pointer here; the two threads' input is
                        // joined only around the window calls.
                        unsafe {
                            let other = GetWindowThreadProcessId(GetForegroundWindow(), None);
                            let this = GetCurrentThreadId();
                            if other != 0 && other != this {
                                let _ = AttachThreadInput(this, other, true);
                                let _ = BringWindowToTop(hwnd);
                                let _ = SetForegroundWindow(hwnd);
                                let _ = AttachThreadInput(this, other, false);
                            }
                        }
                        pump(Duration::from_millis(200));
                    }
                    // SAFETY: SetFocus takes this thread's window handle.
                    unsafe {
                        let _ = SetFocus(Some(hwnd));
                    }
                    pump(Duration::from_millis(200));
                    let _ = tx.send((hwnd.0 as usize, front()));
                    while !thread_stop.load(Ordering::SeqCst) {
                        pump(Duration::from_millis(50));
                    }
                    // SAFETY: the window belongs to this thread and is
                    // destroyed once.
                    unsafe {
                        let _ = DestroyWindow(hwnd);
                    }
                    pump(Duration::from_millis(20));
                });
                let (hwnd, in_front) = rx
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap_or((0, false));
                (
                    EditInFront {
                        hwnd,
                        stop,
                        thread: Some(thread),
                    },
                    in_front,
                )
            }
        }

        impl Drop for EditInFront {
            fn drop(&mut self) {
                self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
                if let Some(thread) = self.thread.take() {
                    let _ = thread.join();
                }
            }
        }

        #[test]
        fn a_focus_events_clear_from_the_callback_clears_no_key() {
            let _serial = crate::windows::tests::serial();
            // SAFETY: joins this test thread to the process's MTA; `_com`
            // undoes it after the reader and the handler are dropped.
            let joined = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            assert!(joined.is_ok(), "{joined:?}");
            let _com = Com;
            let (edit, in_front) = EditInFront::open();
            assert!(
                in_front,
                "INCONCLUSIVE: the test's edit did not come to the front"
            );
            let reader = Arc::new(new_reader().expect("the reader is made"));
            // SAFETY: the edit's handle is live while `edit` lives, and the
            // cache request is live for the call.
            let element = unsafe {
                reader
                    .automation
                    .ElementFromHandleBuildCache(HWND(edit.hwnd as *mut c_void), &reader.cache)
            }
            .expect("the edit's element");
            let gate = PasswordGate::new();
            let handler = FocusHandler {
                vtable: &HANDLER_VTABLE,
                refs: AtomicU32::new(1),
                shared: Arc::clone(&gate.shared),
                reader: Arc::clone(&reader),
                wake: Arc::new(Event::new().expect("an event")),
            }
            .into_interface();
            let start = tick();
            gate.read_at(
                start.wrapping_sub(400),
                FieldState::Password,
                Moved::NotCompared,
            );
            let at_password = gate.field_now();
            // SAFETY: the handler and the element are live COM objects for
            // the call, which borrows both.
            let first_answer = unsafe { handler_focus(handler.as_raw(), element.as_raw()) };
            let after_the_event = gate.field_now();
            std::thread::sleep(Duration::from_millis(50));
            let key = gate.hold(tick(), false, 0);
            std::thread::sleep(Duration::from_millis(150));
            // SAFETY: as above.
            let second_answer = unsafe { handler_focus(handler.as_raw(), element.as_raw()) };
            let decide = |now: u32| {
                gate.decide(&key, now, 0, false)
                    .map(|(answer, rule)| (answer.is_ok(), rule))
            };
            let within_the_bound = decide(key.tick.wrapping_add(200));
            let at_the_bound = decide(key.tick.wrapping_add(260));
            drop(handler);
            drop(element);
            drop(reader);
            drop(edit);
            assert_eq!((first_answer, second_answer), (S_OK, S_OK));
            assert_eq!(at_password, FieldState::Password);
            assert_eq!(
                after_the_event,
                FieldState::Clear,
                "INCONCLUSIVE: the callback's reading of the edit was not Clear"
            );
            assert_eq!(
                within_the_bound, None,
                "a focus event's Clear cleared the key"
            );
            assert_eq!(
                at_the_bound,
                Some((false, super::super::Rule::NotClearedInTime))
            );
        }

        #[test]
        fn detach_ends_the_detectors_thread_and_a_new_attach_starts_one() {
            let _serial = crate::windows::tests::serial();
            let gate = PasswordGate::new();
            gate.attach().expect("the detector starts");
            let first = gate.detach(crate::windows::INPUT_STOP_DEADLINE);
            let running = gate
                .detector
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_some();
            gate.attach().expect("the detector starts again");
            let second = gate.detach(crate::windows::INPUT_STOP_DEADLINE);
            assert_eq!(first, Ok(()));
            assert!(!running, "a detached gate still holds its detector");
            assert_eq!(second, Ok(()));
        }

        #[test]
        fn detach_answers_the_first_of_its_removal_calls_that_failed() {
            let _serial = crate::windows::tests::serial();
            let both = PasswordGate::with_removals(Removals {
                handler: handler_refused,
                desktop_hook: desktop_refused,
                ..Removals::default()
            });
            both.attach().expect("the detector starts");
            let both = both.detach(crate::windows::INPUT_STOP_DEADLINE);
            let hook = PasswordGate::with_removals(Removals {
                desktop_hook: desktop_refused,
                ..Removals::default()
            });
            hook.attach().expect("the detector starts");
            let hook = hook.detach(crate::windows::INPUT_STOP_DEADLINE);
            assert_eq!(both, Err(refused_at("RemoveFocusChangedEventHandler")));
            assert_eq!(hook, Err(refused_at("UnhookWinEvent")));
        }

        static HOLD_READS: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);

        fn read_held() {
            while HOLD_READS.load(std::sync::atomic::Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(10));
            }
        }

        #[test]
        fn an_attach_after_a_timed_out_detach_is_refused_while_the_old_detector_runs() {
            use std::sync::atomic::Ordering;
            let _serial = crate::windows::tests::serial();
            let gate = PasswordGate::with_removals(Removals {
                before_read: read_held,
                ..Removals::default()
            });
            gate.attach().expect("the detector starts");
            HOLD_READS.store(true, Ordering::SeqCst);
            gate.shared.lock().ask_read(tick());
            gate.wake_detector();
            std::thread::sleep(Duration::from_millis(300));
            let stopped = gate.detach(Duration::from_millis(200));
            let second = gate.attach();
            let running_after_the_second = gate
                .detector
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_some();
            HOLD_READS.store(false, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(300));
            let third = gate.attach();
            let detached = gate.detach(crate::windows::INPUT_STOP_DEADLINE);
            assert!(
                matches!(stopped, Err(CaptureError::Timeout { .. })),
                "{stopped:?}"
            );
            assert_eq!(
                second,
                Err(CaptureError::Timeout {
                    stage: "wait for the earlier password detector to end",
                    ms: 1_000,
                })
            );
            assert!(
                !running_after_the_second,
                "a second detector runs beside the held one"
            );
            assert_eq!(third, Ok(()));
            assert_eq!(detached, Ok(()));
        }

        #[test]
        fn detach_answers_timeout_when_the_detector_does_not_end_in_time() {
            let _serial = crate::windows::tests::serial();
            let gate = PasswordGate::with_removals(Removals {
                handler: handler_slow,
                ..Removals::default()
            });
            gate.attach().expect("the detector starts");
            let deadline = Duration::from_millis(200);
            let stopped = gate.detach(deadline);
            assert_eq!(
                stopped,
                Err(CaptureError::Timeout {
                    stage: "stop the password detector",
                    ms: 200,
                })
            );
            std::thread::sleep(Duration::from_millis(1_800));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_probe_reads_unknown_by_default_and_gives_no_clearance() {
        assert_eq!(
            PasswordFieldProbe::default().read(),
            SecureInputReading::Unknown
        );
        assert_eq!(
            check(&PasswordFieldProbe::default(), None).unwrap_err(),
            Withheld::Unknown
        );
    }

    fn answered(is_password: bool, protected: bool, kind: Kind) -> FocusRead {
        FocusRead::Answered {
            is_password,
            protected,
            kind,
        }
    }

    #[test]
    fn only_an_answer_of_a_listed_kind_that_the_field_is_no_password_field_reads_clear() {
        for kind in [Kind::NativeEdit, Kind::Chrome, Kind::Edge] {
            assert_eq!(
                state_of(answered(false, false, kind), false),
                FieldState::Clear
            );
            assert_eq!(
                state_of(answered(true, false, kind), false),
                FieldState::Password
            );
            assert_eq!(
                state_of(answered(false, true, kind), false),
                FieldState::Password
            );
        }
        for kind in [Kind::Firefox, Kind::Other] {
            assert_eq!(
                state_of(answered(false, false, kind), false),
                FieldState::Unknown,
                "{kind:?}"
            );
            assert_eq!(
                state_of(answered(true, false, kind), false),
                FieldState::Password
            );
        }
        assert_eq!(
            state_of(FocusRead::NoAnswer { protected: false }, false),
            FieldState::Unknown
        );
        assert_eq!(
            state_of(FocusRead::NoAnswer { protected: true }, false),
            FieldState::Password
        );
        assert_eq!(state_of(FocusRead::Unreadable, false), FieldState::Unknown);
        assert_eq!(
            state_of(answered(false, false, Kind::Chrome), true),
            FieldState::Password
        );
    }

    #[test]
    fn a_kind_is_told_by_two_things_and_every_other_value_is_no_kind() {
        type Row = (
            Option<&'static str>,
            Option<&'static str>,
            Option<&'static str>,
            Kind,
        );
        let rows: [Row; 16] = [
            (
                Some("Win32"),
                Some("Edit"),
                Some("app.exe"),
                Kind::NativeEdit,
            ),
            (Some("Win32"), Some("EDIT"), None, Kind::NativeEdit),
            (
                Some("Win32"),
                Some("RichEdit20W"),
                Some("app.exe"),
                Kind::Other,
            ),
            (Some("Win32"), None, Some("app.exe"), Kind::Other),
            (None, Some("Edit"), Some("app.exe"), Kind::Other),
            (
                Some("Chrome"),
                Some("Chrome_RenderWidgetHostHWND"),
                Some("chrome.exe"),
                Kind::Chrome,
            ),
            (Some("Chrome"), None, Some("CHROME.EXE"), Kind::Chrome),
            (Some("Chrome"), None, Some("msedge.exe"), Kind::Edge),
            (Some("Chrome"), None, Some("MSEdge.Exe"), Kind::Edge),
            (Some("chrome"), None, Some("chrome.exe"), Kind::Other),
            (
                Some("Chrome"),
                None,
                Some("renamed-chrome.exe"),
                Kind::Other,
            ),
            (Some("Chrome"), None, None, Kind::Other),
            (Some("Gecko"), None, Some("firefox.exe"), Kind::Firefox),
            (
                Some("Chrome"),
                Some("Edit"),
                Some("FireFox.exe"),
                Kind::Firefox,
            ),
            (
                Some("Gecko"),
                None,
                Some("renamed-firefox.exe"),
                Kind::Other,
            ),
            (Some(""), Some("Edit"), Some("app.exe"), Kind::Other),
        ];
        for (framework, class, file, kind) in rows {
            assert_eq!(
                kind_of(framework, class, file),
                kind,
                "{framework:?} {class:?} {file:?}"
            );
        }
    }

    #[test]
    fn the_kept_password_element_is_let_go_by_an_own_read_after_a_focus_move_alone() {
        use FieldState::{Clear, Password};
        use KeptElement::{Other, Same};
        use ReadBy::{Event, OwnRead};
        assert_eq!(
            with_kept(Some(3), Same, Clear, Event, 9),
            (Password, Kept::Leave)
        );
        assert_eq!(
            with_kept(None, Other, Password, Event, 4),
            (Password, Kept::Set)
        );
        assert_eq!(
            with_kept(Some(3), Other, Clear, Event, 9),
            (Clear, Kept::Leave)
        );
        assert_eq!(
            with_kept(Some(3), Other, Clear, OwnRead, 3),
            (Password, Kept::Leave)
        );
        assert_eq!(
            with_kept(Some(3), Other, Clear, OwnRead, 4),
            (Clear, Kept::Drop)
        );
        assert_eq!(
            with_kept(None, Other, Clear, OwnRead, 4),
            (Clear, Kept::Leave)
        );
    }

    const FRONT: u32 = 7;
    const HOLD: u32 = 120;

    fn key(gate: &PasswordGate, tick: u32) -> HeldKey {
        gate.hold(tick, false, FRONT)
    }

    fn reading(gate: &PasswordGate, key: &HeldKey, now: u32) -> Option<Result<(), Withheld>> {
        gate.decide(key, now, FRONT, false)
            .map(|(answer, _)| answer.map(|_| ()))
    }

    fn rule(gate: &PasswordGate, key: &HeldKey, now: u32) -> Option<Rule> {
        gate.decide(key, now, FRONT, false).map(|(_, rule)| rule)
    }

    fn plain() -> PasswordGate {
        let gate = PasswordGate::new();
        gate.read_at(1_000, FieldState::Clear, Moved::NotCompared);
        gate
    }

    #[test]
    fn a_key_in_a_clear_field_waits_for_an_own_read_that_began_after_its_hold() {
        let gate = plain();
        let key = key(&gate, 2_000);
        assert_eq!(reading(&gate, &key, 2_200), None);
        gate.read_at(2_000 + HOLD - 1, FieldState::Clear, Moved::Same);
        assert_eq!(reading(&gate, &key, 2_200), None);
        gate.read_at(2_000 + HOLD, FieldState::Clear, Moved::Same);
        assert_eq!(reading(&gate, &key, 2_200), Some(Ok(())));
        assert_eq!(rule(&gate, &key, 2_200), Some(Rule::ReadAfterTheHold));
    }

    #[test]
    fn a_focus_events_clear_never_clears_a_key() {
        let gate = plain();
        let key = key(&gate, 2_000);
        gate.field_at(2_130, FieldState::Clear);
        gate.field_at(2_160, FieldState::Clear);
        assert_eq!(reading(&gate, &key, 2_240), None);
        assert_eq!(reading(&gate, &key, 2_251), Some(Err(Withheld::Unknown)));
    }

    #[test]
    fn a_focus_key_is_withheld_when_a_password_or_unknown_mark_comes_while_it_is_held() {
        let cases = [
            (FieldState::Password, Withheld::Active),
            (FieldState::Unknown, Withheld::Unknown),
        ];
        for (mark, withheld) in cases {
            let gate = plain();
            let tab = key(&gate, 2_000);
            gate.latch(2_000);
            gate.field_at(2_040, mark);
            gate.read_at(2_130, FieldState::Clear, Moved::Another);
            assert_eq!(reading(&gate, &tab, 2_200), Some(Err(withheld)), "{mark:?}");
        }
    }

    #[test]
    fn a_key_with_no_reading_yet_is_unknown_and_a_password_field_withholds_at_once() {
        let gate = PasswordGate::new();
        let first = key(&gate, 10);
        assert_eq!(reading(&gate, &first, 10), Some(Err(Withheld::Unknown)));
        gate.read_at(20, FieldState::Password, Moved::NotCompared);
        let key = key(&gate, 30);
        assert_eq!(reading(&gate, &key, 30), Some(Err(Withheld::Active)));
        assert_eq!(rule(&gate, &key, 30), Some(Rule::StateAtTheKey));
    }

    #[test]
    fn a_password_or_unknown_mark_after_the_key_decides_against_it_at_once() {
        let gate = plain();
        let key1 = key(&gate, 2_000);
        gate.field_at(2_010, FieldState::Password);
        assert_eq!(reading(&gate, &key1, 2_011), Some(Err(Withheld::Active)));
        assert_eq!(rule(&gate, &key1, 2_011), Some(Rule::MarkAfterTheKey));
        let gate = plain();
        let key2 = key(&gate, 3_000);
        gate.field_at(3_010, FieldState::Unknown);
        assert_eq!(reading(&gate, &key2, 3_011), Some(Err(Withheld::Unknown)));
    }

    #[test]
    fn a_password_mark_after_a_later_focus_key_still_decides_against_the_key() {
        let gate = plain();
        let letter = key(&gate, 2_000);
        gate.latch(2_010);
        gate.field_at(2_040, FieldState::Password);
        gate.read_at(2_130, FieldState::Clear, Moved::Another);
        assert_eq!(reading(&gate, &letter, 2_200), Some(Err(Withheld::Active)));
    }

    #[test]
    fn a_password_mark_after_a_later_click_still_decides_against_the_key() {
        let gate = plain();
        let letter = key(&gate, 2_000);
        gate.latch_after(2_010);
        gate.field_at(2_040, FieldState::Password);
        gate.read_at(2_130, FieldState::Clear, Moved::Another);
        assert_eq!(reading(&gate, &letter, 2_200), Some(Err(Withheld::Active)));
    }

    #[test]
    fn the_last_key_of_a_password_field_stays_withheld_after_an_enter_moves_the_focus() {
        let gate = PasswordGate::new();
        gate.read_at(1_000, FieldState::Password, Moved::NotCompared);
        let last = key(&gate, 2_000);
        let enter = key(&gate, 2_010);
        gate.latch(2_010);
        gate.field_at(2_040, FieldState::Clear);
        gate.read_at(2_130, FieldState::Clear, Moved::Another);
        gate.read_at(2_260, FieldState::Clear, Moved::Same);
        assert_eq!(reading(&gate, &last, 2_240), Some(Err(Withheld::Active)));
        assert_eq!(reading(&gate, &enter, 2_240), Some(Err(Withheld::Active)));
    }

    #[test]
    fn the_last_key_of_a_password_field_stays_withheld_after_a_click_moves_the_focus() {
        let gate = PasswordGate::new();
        gate.read_at(1_000, FieldState::Password, Moved::NotCompared);
        let last = key(&gate, 2_000);
        gate.latch_after(2_010);
        gate.field_at(2_040, FieldState::Clear);
        gate.read_at(2_130, FieldState::Clear, Moved::Another);
        assert_eq!(reading(&gate, &last, 2_240), Some(Err(Withheld::Active)));
    }

    #[test]
    fn a_key_behind_a_latch_waits_for_an_own_read_that_shows_a_move() {
        let gate = plain();
        gate.latch(2_000);
        let letter = key(&gate, 2_001);
        gate.read_at(2_100, FieldState::Clear, Moved::Same);
        gate.read_at(2_130, FieldState::Clear, Moved::Same);
        gate.read_at(2_160, FieldState::Clear, Moved::Same);
        assert_eq!(reading(&gate, &letter, 2_200), None);
        gate.read_at(2_190, FieldState::Clear, Moved::Another);
        assert_eq!(reading(&gate, &letter, 2_240), None);
        gate.read_at(2_309, FieldState::Clear, Moved::Same);
        assert_eq!(reading(&gate, &letter, 2_240), None);
        gate.read_at(2_310, FieldState::Clear, Moved::Same);
        assert_eq!(reading(&gate, &letter, 2_240), Some(Err(Withheld::Unknown)));
    }

    #[test]
    fn a_read_before_the_settle_time_is_no_base_of_the_latch() {
        let gate = plain();
        gate.latch(2_000);
        let letter = key(&gate, 2_001);
        gate.read_at(2_050, FieldState::Clear, Moved::Same);
        gate.read_at(2_060, FieldState::Clear, Moved::Another);
        gate.read_at(2_100, FieldState::Clear, Moved::Same);
        gate.read_at(2_180, FieldState::Clear, Moved::Same);
        assert_eq!(reading(&gate, &letter, 2_240), None);
        assert_eq!(reading(&gate, &letter, 2_252), Some(Err(Withheld::Unknown)));
    }

    #[test]
    fn a_focus_event_closes_no_latch() {
        let gate = plain();
        gate.latch(2_000);
        let letter = key(&gate, 2_001);
        gate.read_at(2_100, FieldState::Clear, Moved::Same);
        gate.field_at(2_105, FieldState::Clear);
        gate.read_at(2_130, FieldState::Clear, Moved::Same);
        gate.read_at(2_230, FieldState::Clear, Moved::Same);
        assert_eq!(reading(&gate, &letter, 2_240), None);
        assert_eq!(reading(&gate, &letter, 2_252), Some(Err(Withheld::Unknown)));
    }

    #[test]
    fn each_move_answers_at_most_the_oldest_open_latch() {
        let gate = plain();
        gate.latch(2_000);
        gate.latch(2_030);
        let letter = key(&gate, 2_060);
        gate.read_at(2_130, FieldState::Clear, Moved::Same);
        gate.read_at(2_160, FieldState::Clear, Moved::Another);
        gate.read_at(2_285, FieldState::Clear, Moved::Same);
        assert_eq!(reading(&gate, &letter, 2_290), None);
        gate.read_at(2_290, FieldState::Clear, Moved::Another);
        gate.read_at(2_305, FieldState::Clear, Moved::Same);
        assert_eq!(reading(&gate, &letter, 2_305), None);
    }

    #[test]
    fn a_key_held_from_before_another_thread_came_in_front_reads_unknown() {
        let gate = plain();
        let key = key(&gate, 2_000);
        gate.read_at(2_130, FieldState::Clear, Moved::Same);
        assert_eq!(
            gate.decide(&key, 2_140, FRONT + 1, false)
                .map(|(answer, rule)| (answer.map(|_| ()), rule)),
            Some((Err(Withheld::Unknown), Rule::AnotherThreadInFront))
        );
    }

    #[test]
    fn a_key_not_cleared_within_250_ms_is_withheld_unknown_and_never_late() {
        let gate = plain();
        gate.latch(1_500);
        let key1 = key(&gate, 1_510);
        assert_eq!(reading(&gate, &key1, 1_700), None);
        assert_eq!(reading(&gate, &key1, 1_761), Some(Err(Withheld::Unknown)));
        let gate = plain();
        gate.read_at(2_200, FieldState::Clear, Moved::Same);
        let late = key(&gate, 2_000);
        assert_eq!(reading(&gate, &late, 2_300), Some(Err(Withheld::Unknown)));
        assert_eq!(rule(&gate, &late, 2_300), Some(Rule::NotClearedInTime));
    }

    #[test]
    fn a_password_edit_under_the_focus_at_the_key_or_at_the_end_of_its_hold_withholds_it() {
        let gate = plain();
        let at_key = gate.hold(2_000, true, FRONT);
        gate.read_at(2_130, FieldState::Clear, Moved::Same);
        assert_eq!(reading(&gate, &at_key, 2_200), Some(Err(Withheld::Active)));
        let later = key(&gate, 2_000);
        assert_eq!(
            gate.decide(&later, 2_200, FRONT, true)
                .map(|(answer, rule)| (answer.map(|_| ()), rule)),
            Some((Err(Withheld::Active), Rule::PasswordEdit))
        );
    }

    fn handled_late(behind: u32) -> Option<Result<(), Withheld>> {
        let gate = PasswordGate::new();
        gate.read_at(1_000, FieldState::Password, Moved::NotCompared);
        gate.field_at(2_030, FieldState::Clear);
        gate.read_at(2_031, FieldState::Clear, Moved::Another);
        let key = key(&gate, 2_000);
        gate.read_at(2_000 + behind.max(HOLD), FieldState::Clear, Moved::Same);
        reading(&gate, &key, 2_000 + behind.max(HOLD))
    }

    #[test]
    fn a_key_is_ordered_by_its_input_time_against_every_mark() {
        assert_eq!(handled_late(80), Some(Err(Withheld::Active)));
        assert_eq!(handled_late(130), Some(Err(Withheld::Active)));
        assert_eq!(handled_late(251), Some(Err(Withheld::Unknown)));
    }

    #[test]
    fn a_mark_whose_time_is_not_known_opens_for_no_key() {
        let gate = PasswordGate::new();
        gate.read_at(1_000, FieldState::Password, Moved::NotCompared);
        gate.shared
            .lock()
            .push(None, 0, Mark::Field(FieldState::Clear));
        let key = key(&gate, 2_000);
        gate.read_at(2_130, FieldState::Clear, Moved::Another);
        assert_eq!(reading(&gate, &key, 2_140), Some(Err(Withheld::Active)));
    }

    #[test]
    fn a_click_of_the_same_tick_after_a_password_key_does_not_clear_it() {
        let gate = PasswordGate::new();
        gate.read_at(1_000, FieldState::Password, Moved::NotCompared);
        let key = key(&gate, 2_000);
        gate.latch_after(2_000);
        gate.read_at(2_100, FieldState::Clear, Moved::Another);
        gate.read_at(2_130, FieldState::Clear, Moved::Another);
        gate.read_at(2_260, FieldState::Clear, Moved::Same);
        assert_eq!(reading(&gate, &key, 2_240), Some(Err(Withheld::Active)));
    }

    #[test]
    fn a_key_after_a_click_of_the_same_tick_waits_for_a_move() {
        let gate = plain();
        gate.latch_after(2_000);
        let key = key(&gate, 2_000);
        gate.read_at(2_100, FieldState::Clear, Moved::Same);
        gate.read_at(2_130, FieldState::Clear, Moved::Same);
        assert_eq!(reading(&gate, &key, 2_240), None);
    }

    #[test]
    fn a_key_after_alt_tab_with_alt_held_past_the_settle_time_waits_for_a_move() {
        let gate = plain();
        gate.latch(1_000);
        let key = key(&gate, 1_100);
        gate.read_at(1_100, FieldState::Clear, Moved::Same);
        gate.read_at(1_130, FieldState::Clear, Moved::Same);
        gate.read_at(1_220, FieldState::Clear, Moved::Same);
        assert_eq!(reading(&gate, &key, 1_240), None);
        gate.read_at(1_300, FieldState::Clear, Moved::Another);
        assert_eq!(reading(&gate, &key, 1_351), Some(Err(Withheld::Unknown)));
    }

    #[test]
    fn after_a_start_the_gate_reads_unknown_until_the_detectors_own_read() {
        let gate = PasswordGate::new();
        gate.started_at(1_000);
        gate.field_at(1_005, FieldState::Clear);
        let key1 = key(&gate, 1_010);
        assert_eq!(reading(&gate, &key1, 1_010), Some(Err(Withheld::Unknown)));
        let gate = PasswordGate::new();
        gate.started_at(1_000);
        gate.field_at(1_005, FieldState::Password);
        let key2 = key(&gate, 1_010);
        assert_eq!(reading(&gate, &key2, 1_010), Some(Err(Withheld::Active)));
        let gate = PasswordGate::new();
        gate.started_at(1_000);
        gate.read_at(1_004, FieldState::Clear, Moved::NotCompared);
        let key3 = key(&gate, 1_010);
        gate.read_at(1_130, FieldState::Clear, Moved::Same);
        assert_eq!(reading(&gate, &key3, 1_140), Some(Ok(())));
    }

    #[test]
    fn the_first_own_read_after_a_start_is_the_base_of_a_latch_made_before_it() {
        let gate = PasswordGate::new();
        gate.started_at(1_000);
        gate.latch(1_002);
        gate.read_at(1_004, FieldState::Clear, Moved::NotCompared);
        let key = key(&gate, 1_005);
        gate.read_at(1_030, FieldState::Clear, Moved::Another);
        gate.read_at(1_149, FieldState::Clear, Moved::Same);
        assert_eq!(reading(&gate, &key, 1_200), None);
        gate.read_at(1_150, FieldState::Clear, Moved::Same);
        assert_eq!(reading(&gate, &key, 1_200), Some(Err(Withheld::Unknown)));
    }

    fn c5(armed: Moved, after_the_hold: Moved) -> (PasswordGate, HeldKey, HeldKey) {
        let gate = plain();
        gate.read_at(1_990, FieldState::Clear, Moved::Same);
        let letter = key(&gate, 2_000);
        let tab = key(&gate, 2_050);
        gate.latch(2_050);
        gate.field_at(2_060, FieldState::Clear);
        gate.read_at(2_065, FieldState::Clear, armed);
        gate.read_at(2_120, FieldState::Clear, after_the_hold);
        gate.read_at(2_170, FieldState::Clear, Moved::Same);
        gate.read_at(2_200, FieldState::Clear, Moved::Same);
        (gate, letter, tab)
    }

    #[test]
    fn a_letter_and_a_tab_before_the_read_that_their_move_armed_are_withheld_in_either_order() {
        for (armed, after_the_hold) in
            [(Moved::Another, Moved::Same), (Moved::Same, Moved::Another)]
        {
            let (gate, letter, tab) = c5(armed, after_the_hold);
            assert_eq!(
                reading(&gate, &letter, 2_210),
                Some(Err(Withheld::Unknown)),
                "letter {armed:?} {after_the_hold:?}"
            );
            assert_eq!(
                reading(&gate, &tab, 2_210),
                Some(Err(Withheld::Unknown)),
                "tab {armed:?} {after_the_hold:?}"
            );
        }
    }

    #[test]
    fn a_read_that_shows_a_move_withholds_the_key_by_a_rule_of_its_own() {
        let (gate, letter, tab) = c5(Moved::Another, Moved::Same);
        assert_eq!(rule(&gate, &letter, 2_210), Some(Rule::ElementNotTheSame));
        assert_eq!(rule(&gate, &tab, 2_210), Some(Rule::ElementNotTheSame));
    }

    #[test]
    fn a_key_before_the_read_that_closes_a_latch_behind_it_is_never_cleared() {
        for click in [false, true] {
            let gate = plain();
            if click {
                gate.latch_after(1_990);
            } else {
                gate.latch(1_990);
            }
            let letter = key(&gate, 2_000);
            gate.read_at(2_100, FieldState::Clear, Moved::Same);
            gate.read_at(2_130, FieldState::Clear, Moved::Another);
            let after = key(&gate, 2_140);
            gate.read_at(2_250, FieldState::Clear, Moved::Same);
            gate.read_at(2_260, FieldState::Clear, Moved::Same);
            assert_eq!(
                reading(&gate, &letter, 2_250),
                Some(Err(Withheld::Unknown)),
                "click={click}"
            );
            assert_eq!(reading(&gate, &after, 2_265), Some(Ok(())), "click={click}");
        }
    }

    #[test]
    fn a_read_that_cannot_compare_from_the_key_to_its_deciding_read_withholds_the_key() {
        let gate = plain();
        gate.read_at(1_990, FieldState::Clear, Moved::Same);
        let letter = key(&gate, 2_000);
        gate.read_at(2_030, FieldState::Clear, Moved::NotCompared);
        gate.read_at(2_120, FieldState::Clear, Moved::Same);
        assert_eq!(reading(&gate, &letter, 2_130), Some(Err(Withheld::Unknown)));
        let gate = plain();
        gate.read_at(1_990, FieldState::Clear, Moved::Same);
        let letter = key(&gate, 2_000);
        gate.read_at(2_120, FieldState::Clear, Moved::NotCompared);
        assert_eq!(reading(&gate, &letter, 2_130), Some(Err(Withheld::Unknown)));
        let gate = PasswordGate::new();
        gate.started_at(1_000);
        let first = key(&gate, 1_010);
        gate.read_at(1_130, FieldState::Clear, Moved::NotCompared);
        assert_eq!(reading(&gate, &first, 1_140), Some(Err(Withheld::Unknown)));
        let after = key(&gate, 1_140);
        gate.read_at(1_260, FieldState::Clear, Moved::Same);
        assert_eq!(reading(&gate, &after, 1_270), Some(Ok(())));
    }

    #[test]
    fn a_focus_that_leaves_for_a_password_field_and_returns_within_one_read_step_withholds_the_key()
    {
        let gate = plain();
        gate.read_at(1_990, FieldState::Clear, Moved::Same);
        let letter = key(&gate, 2_000);
        gate.field_at(2_010, FieldState::Password);
        gate.field_at(2_040, FieldState::Clear);
        gate.read_at(2_120, FieldState::Clear, Moved::Same);
        assert_eq!(reading(&gate, &letter, 2_130), Some(Err(Withheld::Active)));
        let gate = plain();
        gate.read_at(1_990, FieldState::Clear, Moved::Same);
        gate.field_at(1_995, FieldState::Password);
        let letter = key(&gate, 2_000);
        gate.field_at(2_040, FieldState::Clear);
        gate.read_at(2_120, FieldState::Clear, Moved::Same);
        assert_eq!(reading(&gate, &letter, 2_130), Some(Err(Withheld::Active)));
    }

    #[test]
    fn a_desktop_switch_reads_unknown_and_closes_the_gate() {
        let gate = plain();
        let before = key(&gate, 2_000);
        gate.shared.desktop_switched(2_010);
        let after = key(&gate, 2_020);
        assert_eq!(reading(&gate, &before, 2_200), Some(Err(Withheld::Unknown)));
        assert_eq!(gate.field_now(), FieldState::Unknown);
        gate.read_at(2_120, FieldState::Clear, Moved::Another);
        assert_eq!(reading(&gate, &after, 2_200), Some(Err(Withheld::Unknown)));
    }

    #[test]
    fn a_key_whose_hold_meets_a_time_with_no_click_route_is_withheld() {
        let gate = plain();
        let before = key(&gate, 1_800);
        let edge = key(&gate, 1_900);
        gate.read_at(1_925, FieldState::Clear, Moved::Same);
        gate.clicks_lost(2_000);
        let inside = key(&gate, 2_050);
        let while_open = rule(&gate, &inside, 2_060);
        gate.clicks_back(2_100);
        let after = key(&gate, 2_101);
        gate.read_at(2_221, FieldState::Clear, Moved::Same);
        assert_eq!(reading(&gate, &before, 1_930), Some(Ok(())));
        assert_eq!(while_open, Some(Rule::NoClickRoute));
        assert_eq!(rule(&gate, &edge, 2_030), Some(Rule::NoClickRoute));
        assert_eq!(rule(&gate, &inside, 2_230), Some(Rule::NoClickRoute));
        assert_eq!(reading(&gate, &after, 2_230), Some(Ok(())));
        assert_eq!(rule(&gate, &after, 2_230), Some(Rule::ReadAfterTheHold));
    }

    #[test]
    fn the_hold_is_read_across_the_wrap_of_the_tick_count() {
        let gate = PasswordGate::new();
        gate.read_at(u32::MAX - 100, FieldState::Clear, Moved::NotCompared);
        let key = key(&gate, u32::MAX - 50);
        gate.read_at(68, FieldState::Clear, Moved::Same);
        assert_eq!(reading(&gate, &key, 80), None);
        gate.read_at(69, FieldState::Clear, Moved::Same);
        assert_eq!(reading(&gate, &key, 80), Some(Ok(())));
    }

    #[test]
    fn an_undecided_key_asks_for_an_own_read_at_the_end_of_its_hold() {
        let gate = plain();
        let key = key(&gate, 2_000);
        assert_eq!(reading(&gate, &key, 2_010), None);
        assert_eq!(gate.shared.lock().read_due, Some(2_000 + HOLD));
        gate.read_at(2_000 + HOLD, FieldState::Clear, Moved::Same);
        assert_eq!(gate.shared.lock().read_due, None);
    }

    #[test]
    fn the_gate_keeps_the_newest_256_marks() {
        let gate = PasswordGate::new();
        for at in 0..300u32 {
            gate.read_at(at, FieldState::Clear, Moved::Same);
        }
        let marks = gate.shared.lock();
        assert_eq!(marks.entries.len(), 256);
        assert_eq!(marks.entries.front().map(|entry| entry.began), Some(44));
    }

    #[test]
    fn a_read_that_ends_in_an_error_reads_unknown() {
        assert_eq!(unreadable_state(), FieldState::Unknown);
    }

    #[test]
    fn key_hold_is_above_the_largest_delay_that_s3_measured() {
        assert!(KEY_HOLD > S3_LARGEST_KEY_TO_FOCUS_EVENT);
        assert_eq!(S3_LARGEST_KEY_TO_FOCUS_EVENT, Duration::from_micros(75_300));
    }
}
