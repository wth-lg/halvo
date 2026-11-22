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

/// The reading of a window title or a screen frame: its own reading of the
/// same read of the focused element that the key's state comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentReading {
    /// The element answered that it is no password field.
    Clear(ClearBy),
    /// The element is a password field, or the key's state of the same read
    /// is Password.
    Password,
    /// No reading that a title or a frame may be kept by.
    Unknown(UnknownBy),
}

impl Default for ContentReading {
    fn default() -> Self {
        ContentReading::Unknown(UnknownBy::BeforeFirstRead)
    }
}

/// The kind of element whose answer made a content reading Clear.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClearBy {
    /// A native Win32 edit control.
    NativeEdit,
    /// Chrome.
    Chrome,
    /// Edge.
    Edge,
    /// Any other kind that answered.
    OtherKind,
}

/// Why a content reading is Unknown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnknownBy {
    /// The detector has made no read since it started, or it is stopped.
    BeforeFirstRead,
    /// Firefox, by its file name or by its engine Gecko.
    Firefox,
    /// The element gave no answer.
    NoAnswer,
    /// No element could be read.
    Unreadable,
    /// The desktop switched.
    DesktopSwitch,
}

/// The content readings that the sources of a gate asked of it, Clear by
/// the kind that answered and Unknown by its cause.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GateReadings {
    /// Clear by a native edit control.
    pub native_edit: u64,
    /// Clear by Chrome.
    pub chrome: u64,
    /// Clear by Edge.
    pub edge: u64,
    /// Clear by another kind that answered.
    pub other_kind: u64,
    /// Password. Its word is not the bare `password`, because GitHub's log
    /// of a run masks a bare `password=` with its value.
    pub password_field: u64,
    /// Unknown before the detector's first read.
    pub before_first_read: u64,
    /// Unknown in Firefox.
    pub firefox: u64,
    /// Unknown with no answer.
    pub no_answer: u64,
    /// Unknown with no readable element.
    pub unreadable: u64,
    /// Unknown after a desktop switch.
    pub desktop_switch: u64,
}

impl GateReadings {
    /// The readings that were Clear.
    pub fn clear(&self) -> u64 {
        self.native_edit + self.chrome + self.edge + self.other_kind
    }

    /// The readings that were Unknown.
    pub fn unknown(&self) -> u64 {
        self.before_first_read
            + self.firefox
            + self.no_answer
            + self.unreadable
            + self.desktop_switch
    }

    fn count(&mut self, reading: ContentReading) {
        let field = match reading {
            ContentReading::Clear(ClearBy::NativeEdit) => &mut self.native_edit,
            ContentReading::Clear(ClearBy::Chrome) => &mut self.chrome,
            ContentReading::Clear(ClearBy::Edge) => &mut self.edge,
            ContentReading::Clear(ClearBy::OtherKind) => &mut self.other_kind,
            ContentReading::Password => &mut self.password_field,
            ContentReading::Unknown(UnknownBy::BeforeFirstRead) => &mut self.before_first_read,
            ContentReading::Unknown(UnknownBy::Firefox) => &mut self.firefox,
            ContentReading::Unknown(UnknownBy::NoAnswer) => &mut self.no_answer,
            ContentReading::Unknown(UnknownBy::Unreadable) => &mut self.unreadable,
            ContentReading::Unknown(UnknownBy::DesktopSwitch) => &mut self.desktop_switch,
        };
        *field += 1;
    }
}

/// The content reading of one read: `gecko` says whether the element's
/// framework is Gecko, and `key` is the key's state of the same read, with
/// the kept password element.
pub(crate) fn content_of(read: FocusRead, gecko: bool, key: FieldState) -> ContentReading {
    if key == FieldState::Password {
        return ContentReading::Password;
    }
    match read {
        FocusRead::Answered {
            is_password: true, ..
        }
        | FocusRead::Answered {
            protected: true, ..
        }
        | FocusRead::NoAnswer { protected: true } => ContentReading::Password,
        FocusRead::Answered { kind, .. } => match kind {
            Kind::Firefox => ContentReading::Unknown(UnknownBy::Firefox),
            _ if gecko => ContentReading::Unknown(UnknownBy::Firefox),
            Kind::NativeEdit => ContentReading::Clear(ClearBy::NativeEdit),
            Kind::Chrome => ContentReading::Clear(ClearBy::Chrome),
            Kind::Edge => ContentReading::Clear(ClearBy::Edge),
            Kind::Other => ContentReading::Clear(ClearBy::OtherKind),
        },
        FocusRead::NoAnswer { protected: false } => ContentReading::Unknown(UnknownBy::NoAnswer),
        FocusRead::Unreadable => ContentReading::Unknown(UnknownBy::Unreadable),
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
    events_in_flight: bool,
) -> (FieldState, Kept) {
    match kept {
        Some(_) if element == KeptElement::Same && by == ReadBy::Event => {
            (FieldState::Password, Kept::Set)
        }
        Some(_) if element == KeptElement::Same => (FieldState::Password, Kept::Leave),
        _ if read == FieldState::Password => (FieldState::Password, Kept::Set),
        None => (read, Kept::Leave),
        Some(_) if by == ReadBy::Event => (read, Kept::Leave),
        Some(at) if lets_go(at, moves, events_in_flight) => (read, Kept::Drop),
        Some(_) => (FieldState::Password, Kept::Leave),
    }
}

pub(crate) fn lets_go(kept_move: u64, read_move: u64, events_in_flight: bool) -> bool {
    !events_in_flight && read_move > kept_move
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

/// The step of the system's tick count, by which the pointer pump's forwarded
/// tick must pass the end of a key's hold.
pub const TICK_STEP: Duration = Duration::from_millis(16);

/// The age past which a mark no longer gives the state at a key; far below the
/// 2^31 ms at which a tick's offset turns round.
pub const MARK_AGE_LIMIT: Duration = Duration::from_secs(24 * 60 * 60);

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
    pushed: std::time::Instant,
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
    /// A mark from the key's own span left the gate's ring of marks before
    /// the key was decided.
    MarksLost,
    /// The pointer source owned the click route, and its pump had not
    /// forwarded its input past the key's hold within 250 ms.
    ClicksNotForwarded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Judgment {
    Wait(u32),
    Decided(SecureInputReading, Rule),
}

fn ms(duration: Duration) -> u32 {
    u32::try_from(duration.as_millis()).unwrap_or(u32::MAX)
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

fn offset(tick: u32, from: u32) -> i64 {
    i64::from(tick.wrapping_sub(from) as i32)
}

fn most_restrictive(a: FieldState, b: FieldState) -> FieldState {
    match (a, b) {
        (FieldState::Password, _) | (_, FieldState::Password) => FieldState::Password,
        (FieldState::Unknown, _) | (_, FieldState::Unknown) => FieldState::Unknown,
        _ => FieldState::Clear,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OpenLatch {
    seq: u64,
    at: u32,
    from: LatchFrom,
    based: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ClosedLatch {
    seq: u64,
    at: u32,
    from: LatchFrom,
    closing_seq: u64,
    closing_tick: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Gap {
    from: u32,
    back: Option<u32>,
}

fn behind_the_key(seq: u64, at: u32, from: LatchFrom, key: &HeldKey) -> bool {
    seq < key.seen
        || (from == LatchFrom::AnotherThread
            && (0..=i64::from(ms(MARK_AGE_LIMIT))).contains(&offset(key.tick, at)))
}

fn judge(m: &Marks, key: &HeldKey, now: u32, front: u32, password_edit_now: bool) -> Judgment {
    if key.password_edit || password_edit_now {
        return Judgment::Decided(SecureInputReading::Active, Rule::PasswordEdit);
    }
    let late = now.wrapping_sub(key.tick) > ms(super::LATE_KEY_MAX);
    if late {
        let rule = if m.clicks_not_forwarded(key)
            && judge_marks(m, key, now)
                == Judgment::Decided(SecureInputReading::Inactive, Rule::ReadAfterTheHold)
        {
            Rule::ClicksNotForwarded
        } else {
            Rule::NotClearedInTime
        };
        return Judgment::Decided(SecureInputReading::Unknown, rule);
    }
    if key.front != front {
        return Judgment::Decided(SecureInputReading::Unknown, Rule::AnotherThreadInFront);
    }
    match judge_marks(m, key, now) {
        Judgment::Decided(SecureInputReading::Inactive, Rule::ReadAfterTheHold)
            if m.clicks_not_forwarded(key) =>
        {
            Judgment::Wait(now.wrapping_add(ms(READ_STEP)))
        }
        judged => judged,
    }
}

fn judge_marks(m: &Marks, key: &HeldKey, now: u32) -> Judgment {
    if m.left_the_ring > key.seen {
        return Judgment::Decided(SecureInputReading::Unknown, Rule::MarksLost);
    }
    let bound = i64::from(ms(super::LATE_KEY_MAX) + ms(KEY_HOLD));
    let after_key = |entry: &Entry| {
        entry.seq >= key.seen
            || entry
                .at
                .is_none_or(|at| (0..=bound).contains(&offset(at, key.tick)))
    };
    let before_first_read = |entry: &Entry| {
        m.started.is_some_and(|started| entry.seq > started)
            && m.first_read.is_none_or(|read| entry.seq < read)
    };
    let opening = m
        .entries
        .iter()
        .rev()
        .filter(|entry| !after_key(entry))
        .find_map(|entry| {
            let state = entry.mark.state()?;
            let tick = entry.at.unwrap_or(entry.began);
            let age = offset(key.tick, tick);
            let stale = !(0..=i64::from(ms(MARK_AGE_LIMIT))).contains(&age)
                || entry.pushed.elapsed() > MARK_AGE_LIMIT;
            let clear_event = matches!(entry.mark, Mark::Field(FieldState::Clear));
            Some(if stale || (clear_event && before_first_read(entry)) {
                FieldState::Unknown
            } else {
                state
            })
        })
        .unwrap_or(FieldState::Unknown);
    let mut floor = i64::from(ms(KEY_HOLD));
    let mut floor_seq = key.seen;
    for latch in m
        .closed
        .iter()
        .filter(|latch| behind_the_key(latch.seq, latch.at, latch.from, key))
    {
        floor = floor.max(offset(latch.closing_tick, key.tick) + i64::from(ms(KEY_HOLD)));
        floor_seq = floor_seq.max(latch.closing_seq + 1);
    }
    let open_latch = m
        .open
        .iter()
        .find(|latch| behind_the_key(latch.seq, latch.at, latch.from, key))
        .map(|latch| latch.at);
    let deciding = if open_latch.is_some() {
        None
    } else {
        m.entries.iter().find(|entry| {
            entry.seq >= floor_seq
                && matches!(entry.mark, Mark::Read { .. })
                && offset(entry.began, key.tick) >= floor
        })
    };
    let upto = deciding.map_or(u64::MAX, |read| read.seq);
    let later = m
        .entries
        .iter()
        .filter(|entry| after_key(entry) && entry.seq <= upto)
        .filter_map(|entry| entry.mark.state())
        .fold(FieldState::Clear, most_restrictive);
    let moved = m.entries.iter().any(|entry| {
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
            let last_read = m
                .entries
                .iter()
                .rev()
                .find(|entry| matches!(entry.mark, Mark::Read { .. }))
                .map(|entry| entry.began.wrapping_add(ms(READ_STEP)));
            let floor_tick = key
                .tick
                .wrapping_add(u32::try_from(floor.max(0)).unwrap_or(u32::MAX));
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
    left_the_ring: u64,
    started: Option<u64>,
    first_read: Option<u64>,
    open: VecDeque<OpenLatch>,
    closed: VecDeque<ClosedLatch>,
    latch_floor: u64,
    settle_due: Option<u32>,
    read_due: Option<u32>,
    moves: u64,
    events_in_flight: u32,
    stopping: bool,
    gap: Option<Gap>,
    pointer_owns: bool,
    pointer_since: Option<u32>,
    forwarded: Option<u32>,
    content: ContentReading,
    writer: Option<u64>,
}

impl Marks {
    /// Writes the content reading of a read: one with no writer (a test's
    /// own mark) always, one of a detector only while that detector is the
    /// gate's writer. A focus event of a started detector writes nothing
    /// before that detector's first own read, as the key's start rule.
    fn set_content(&mut self, by: Option<u64>, content: ContentReading) {
        if by.is_none() || by == self.writer {
            self.content = content;
        }
    }

    /// The content reading that an own read which began at the mark `since`
    /// may write: never Clear where the newest mark that came after it holds
    /// Password, because that mark (a focus event) is a newer reading of the
    /// key's state than the read's own; the reading is then Password.
    fn content_of_an_own_read(&self, since: u64, content: ContentReading) -> ContentReading {
        let newer = self
            .entries
            .iter()
            .rev()
            .take_while(|entry| entry.seq >= since)
            .find_map(|entry| entry.mark.state());
        match content {
            ContentReading::Clear(_) if newer == Some(FieldState::Password) => {
                ContentReading::Password
            }
            content => content,
        }
    }

    /// Ends the writer of the content reading: a detector that its attach or
    /// its detach gave up writes no content reading from then on, and the
    /// reading is Unknown until an attach succeeds.
    fn give_up_the_writer(&mut self) {
        self.writer = None;
        self.content = ContentReading::default();
    }

    fn clicks_unseen_for(&self, key: &HeldKey) -> bool {
        self.gap.is_some_and(|gap| match gap.back {
            None => true,
            Some(back) => {
                offset(gap.from, key.tick) <= i64::from(ms(KEY_HOLD)) && offset(back, key.tick) >= 0
            }
        })
    }

    fn clicks_not_forwarded(&self, key: &HeldKey) -> bool {
        self.pointer_owns
            && !self.forwarded.is_some_and(|forwarded| {
                (i64::from(ms(KEY_HOLD) + ms(TICK_STEP))..=i64::from(i32::MAX))
                    .contains(&offset(forwarded, key.tick))
            })
    }

    fn drop_old_gap(&mut self, now: u32) {
        if self.gap.is_some_and(|gap| {
            gap.back
                .is_some_and(|back| offset(now, back) > i64::from(ms(super::LATE_KEY_MAX)))
        }) {
            self.gap = None;
        }
    }

    fn drop_old_closed_latches(&mut self, now: u32) {
        self.closed
            .retain(|latch| offset(now, latch.closing_tick) <= i64::from(ms(super::LATE_KEY_MAX)));
    }

    fn push(&mut self, at: Option<u32>, began: u32, mark: Mark) {
        if self.entries.len() == MARKS_KEPT {
            if let Some(gone) = self.entries.pop_front() {
                self.left_the_ring = gone.seq + 1;
            }
        }
        let seq = self.next_seq;
        self.entries.push_back(Entry {
            seq,
            at,
            began,
            mark,
            pushed: std::time::Instant::now(),
        });
        self.next_seq += 1;
        match mark {
            Mark::Latch { from, at } => self.open.push_back(OpenLatch {
                seq,
                at,
                from,
                based: false,
            }),
            Mark::Read { moved, .. } => self.read_came(seq, began, at.unwrap_or(began), moved),
            Mark::Field(_) | Mark::Started => {}
        }
    }

    fn read_came(&mut self, seq: u64, began: u32, tick: u32, moved: Moved) {
        let first_after_start = self.started.is_some() && self.first_read.is_none();
        if first_after_start {
            self.first_read = Some(seq);
        }
        let started = self.started;
        let base_of = |latch: &OpenLatch, floor: u64| {
            seq > latch.seq
                && seq >= floor
                && (not_after(latch.at.wrapping_add(ms(LATCH_SETTLE)), began)
                    || (first_after_start && started.is_some_and(|start| latch.seq > start)))
        };
        let Some(oldest) = self.open.front_mut() else {
            return;
        };
        if !oldest.based {
            oldest.based = base_of(oldest, self.latch_floor);
            return;
        }
        if moved != Moved::Another {
            return;
        }
        if let Some(closed) = self.open.pop_front() {
            self.closed.push_back(ClosedLatch {
                seq: closed.seq,
                at: closed.at,
                from: closed.from,
                closing_seq: seq,
                closing_tick: tick,
            });
        }
        self.latch_floor = seq;
        let floor = self.latch_floor;
        if let Some(next) = self.open.front_mut() {
            next.based = base_of(next, floor);
        }
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
    readings: Mutex<GateReadings>,
    writers: std::sync::atomic::AtomicU64,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Marks> {
        self.marks.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn field(&self, at: u32, state: FieldState, content: ContentReading, by: Option<u64>) {
        let _ = self.begin_event();
        self.field_seen(at, state, (content, by), None);
    }

    /// Makes a new detector the only writer of the content reading.
    fn open_writer(&self) -> u64 {
        let writer = self
            .writers
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1;
        self.lock().writer = Some(writer);
        writer
    }

    fn begin_event(&self) -> u64 {
        let mut marks = self.lock();
        marks.moves += 1;
        marks.events_in_flight += 1;
        marks.moves
    }

    fn field_seen(
        &self,
        at: u32,
        state: FieldState,
        (content, by): (ContentReading, Option<u64>),
        seen: Option<String>,
    ) {
        let mut marks = self.lock();
        marks.push(Some(at), at, Mark::Field(state));
        if !(marks.started.is_some() && marks.first_read.is_none()) {
            marks.set_content(by, content);
        }
        marks.events_in_flight = marks.events_in_flight.saturating_sub(1);
        marks.ask_read(at);
        note_mark("event", Some(state), None, Some(at), at, seen);
    }

    fn read_seen(
        &self,
        began: u32,
        at: u32,
        (state, content, by, since): (FieldState, ContentReading, Option<u64>, u64),
        moved: Moved,
        seen: Option<String>,
    ) {
        note_mark("read", Some(state), Some(moved), Some(at), began, seen);
        let mut marks = self.lock();
        let content = marks.content_of_an_own_read(since, content);
        marks.push(Some(at), began, Mark::Read { state, moved });
        marks.set_content(by, content);
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
        let kept = Marks {
            next_seq: marks.next_seq,
            left_the_ring: marks.next_seq,
            open: std::mem::take(&mut marks.open),
            moves: marks.moves,
            gap: marks.gap,
            pointer_owns: marks.pointer_owns,
            pointer_since: marks.pointer_since,
            forwarded: marks.forwarded,
            ..Marks::default()
        };
        *marks = kept;
        let seq = marks.next_seq;
        marks.started = Some(seq);
        marks.push(Some(at), at, Mark::Started);
    }

    fn desktop_switched(&self, tick: u32, by: Option<u64>) {
        self.field(
            tick,
            FieldState::Unknown,
            ContentReading::Unknown(UnknownBy::DesktopSwitch),
            by,
        );
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

/// What an attach does when the detector's first read has not come by its
/// deadline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AtTheDeadline {
    /// The keyboard source: the attach gives the detector up.
    GiveUp,
    /// The service set: the attach keeps the detector, whose first read may
    /// land late; the content reading stays Unknown until it does.
    WaitForIt,
}

/// How an attach that did not fail ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Attached {
    /// The detector made its first read before the attach returned.
    Read,
    /// The detector runs and its first read has not landed yet.
    Late,
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
        marks.gap = Some(match marks.gap {
            Some(gap) => Gap {
                from: if offset(tick, gap.from) < 0 {
                    tick
                } else {
                    gap.from
                },
                back: None,
            },
            None => Gap {
                from: tick,
                back: None,
            },
        });
    }

    pub(crate) fn clicks_back(&self, tick: u32) {
        note_mark("clicks-back", None, None, Some(tick), tick, None);
        let mut marks = self.shared.lock();
        if let Some(gap) = marks.gap.as_mut() {
            if gap.back.is_none() {
                gap.back = Some(tick);
            }
        }
    }

    pub(crate) fn pointer_takes_the_clicks(&self, tick: u32) {
        let mut marks = self.shared.lock();
        marks.pointer_owns = true;
        marks.pointer_since = Some(tick);
        marks.forwarded = None;
    }

    pub(crate) fn pointer_did_not_take_the_clicks(&self) {
        let mut marks = self.shared.lock();
        marks.pointer_owns = false;
        marks.pointer_since = None;
        marks.forwarded = None;
    }

    pub(crate) fn pointer_lets_the_clicks_go(&self, fallback: u32) {
        let from = {
            let mut marks = self.shared.lock();
            let from = marks.forwarded.or(marks.pointer_since).unwrap_or(fallback);
            marks.pointer_owns = false;
            marks.pointer_since = None;
            marks.forwarded = None;
            from
        };
        self.clicks_lost(from);
    }

    pub(crate) fn pointer_forwarded(&self, tick: u32) {
        let mut marks = self.shared.lock();
        marks.forwarded = Some(match marks.forwarded {
            Some(forwarded) if offset(tick, forwarded) < 0 => forwarded,
            _ => tick,
        });
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
            marks.drop_old_gap(now);
            marks.drop_old_closed_latches(now);
            if marks.clicks_unseen_for(key) {
                return Some((Err(Withheld::Unknown), Rule::NoClickRoute));
            }
            let judged = judge(&marks, key, now, front, password_edit_now);
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
            Rule::NotClearedInTime | Rule::ClicksNotForwarded => Err(Withheld::Unknown),
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

    /// The reading of a window title or a screen frame now.
    pub fn content_reading(&self) -> ContentReading {
        self.shared.lock().content
    }

    /// The content readings that the sources of this gate asked of it.
    pub fn readings(&self) -> GateReadings {
        *self
            .shared
            .readings
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn content_probe(&self) -> PasswordFieldProbe {
        let reading = self.content_reading();
        self.shared
            .readings
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .count(reading);
        PasswordFieldProbe(match reading {
            ContentReading::Password => SecureInputReading::Active,
            ContentReading::Clear(_) => SecureInputReading::Inactive,
            ContentReading::Unknown(_) => SecureInputReading::Unknown,
        })
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
        let content = self.content_reading();
        self.shared.field(at, state, content, None);
    }

    /// A focus event of one read, as the detector marks it with no kept
    /// password element: the key's state and the content reading of the read.
    #[cfg(test)]
    pub(crate) fn focus_read_at(&self, at: u32, read: FocusRead, gecko: bool) {
        let key = state_of(read, false);
        self.shared
            .field(at, key, content_of(read, gecko, key), None);
    }

    #[cfg(test)]
    pub(crate) fn switch_desktop_at(&self, at: u32) {
        self.shared.desktop_switched(at, None);
    }

    #[cfg(test)]
    pub(crate) fn read_at(&self, began: u32, state: FieldState, moved: Moved) {
        let (content, since) = {
            let marks = self.shared.lock();
            (marks.content, marks.next_seq)
        };
        self.shared
            .read_seen(began, began, (state, content, None, since), moved, None);
    }

    #[cfg(test)]
    pub(crate) fn started_at(&self, at: u32) {
        self.shared.started(at);
    }

    #[cfg(test)]
    pub(crate) fn latch_count(&self) -> usize {
        self.shared.lock().open.len()
    }

    #[cfg(feature = "test-seam")]
    pub(crate) fn open_latches_text(&self) -> String {
        let marks = self.shared.lock();
        let listed: Vec<String> = marks
            .open
            .iter()
            .map(|latch| {
                format!(
                    "seq{}@{}:{:?}:based={}",
                    latch.seq,
                    latch.at,
                    latch.from,
                    u8::from(latch.based)
                )
            })
            .collect();
        format!("count={} [{}]", listed.len(), listed.join(","))
    }

    #[cfg(test)]
    pub(crate) fn pointer_since(&self) -> Option<u32> {
        self.shared.lock().pointer_since
    }
}

#[cfg(target_os = "windows")]
pub(crate) use detector::focus_window_is_password_edit;
#[cfg(feature = "test-seam")]
pub(crate) use detector::focused_element_properties;

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
        #[cfg(test)]
        pub(super) before_first_read: fn(),
        #[cfg(test)]
        pub(super) set_desktop_hook: fn() -> HWINEVENTHOOK,
        #[cfg(test)]
        pub(super) attach_deadline: Option<std::time::Duration>,
        #[cfg(test)]
        pub(super) at_receive: fn(),
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
                #[cfg(test)]
                before_first_read: || {},
                #[cfg(test)]
                set_desktop_hook,
                #[cfg(test)]
                attach_deadline: None,
                #[cfg(test)]
                at_receive: || {},
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

        #[cfg(test)]
        pub(crate) fn whose_attach_fails() -> PasswordGate {
            fn no_desktop_hook() -> HWINEVENTHOOK {
                HWINEVENTHOOK::default()
            }
            PasswordGate::with_removals(Removals {
                set_desktop_hook: no_desktop_hook,
                ..Removals::default()
            })
        }

        #[cfg(test)]
        pub(crate) fn is_attached(&self) -> bool {
            self.detector
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_some()
        }

        pub(crate) fn attach(&self) -> Result<(), CaptureError> {
            self.attach_with(super::AtTheDeadline::GiveUp).map(|_| ())
        }

        /// The service set's attach: a detector whose first read has not come
        /// by the deadline is kept, and the attach answers Late.
        pub(crate) fn attach_waiting(&self) -> Result<super::Attached, CaptureError> {
            self.attach_with(super::AtTheDeadline::WaitForIt)
        }

        fn attach_with(
            &self,
            at_the_deadline: super::AtTheDeadline,
        ) -> Result<super::Attached, CaptureError> {
            if self
                .detector
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_some()
            {
                return Ok(super::Attached::Read);
            }
            self.wait_for_the_stale_detector()?;
            #[cfg(feature = "test-seam")]
            crate::windows::input::seam::detector_started();
            let at = tick();
            self.shared.started(at);
            self.clicks_lost(at);
            let writer = self.shared.open_writer();
            let wake = match Event::new() {
                Ok(wake) => Arc::new(wake),
                Err(error) => {
                    self.shared.lock().give_up_the_writer();
                    return Err(error);
                }
            };
            let (ready, started) = std::sync::mpsc::channel();
            let shared = Arc::clone(&self.shared);
            let thread_wake = Arc::clone(&wake);
            let removals = self.removals;
            let join = std::thread::Builder::new()
                .name("halvo-password-detector".to_string())
                .spawn(move || run(&shared, &thread_wake, (removals, writer), ready))
                .map_err(|e| {
                    self.shared.lock().give_up_the_writer();
                    CaptureError::Os {
                        stage: "spawn the password detector thread",
                        domain: "io",
                        code: i64::from(e.raw_os_error().unwrap_or(0)),
                    }
                })?;
            let deadline = crate::windows::INPUT_STOP_DEADLINE;
            #[cfg(test)]
            let deadline = self.removals.attach_deadline.unwrap_or(deadline);
            #[cfg(test)]
            (self.removals.at_receive)();
            match started.recv_timeout(deadline) {
                Ok(Ok(())) => {
                    *self
                        .detector
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Running {
                        wake,
                        join: Some(join),
                    });
                    Ok(super::Attached::Read)
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
                    if at_the_deadline == super::AtTheDeadline::WaitForIt =>
                {
                    *self
                        .detector
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Running {
                        wake,
                        join: Some(join),
                    });
                    Ok(super::Attached::Late)
                }
                Ok(Err(error)) => {
                    self.shared.lock().give_up_the_writer();
                    let _ = join.join();
                    Err(error)
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    {
                        let mut marks = self.shared.lock();
                        marks.stopping = true;
                        marks.give_up_the_writer();
                    }
                    wake.set();
                    *self
                        .stale
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(join);
                    Err(CaptureError::Timeout {
                        stage: "wait for the password detector's first read",
                        ms: u64::try_from(deadline.as_millis()).unwrap_or(u64::MAX),
                    })
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    self.shared.lock().give_up_the_writer();
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
            #[cfg(test)]
            let deadline = self.removals.attach_deadline.unwrap_or(deadline);
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
            {
                let mut marks = self.shared.lock();
                marks.stopping = true;
                marks.give_up_the_writer();
            }
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
            marks.content = super::ContentReading::default();
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
    }

    #[cfg(feature = "test-seam")]
    thread_local! {
        static SEEN: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[cfg_attr(not(feature = "test-seam"), allow(dead_code))]
    pub(super) enum FileRead {
        Read,
        NoProcessId,
        Open(i32),
        Query(i32),
    }

    impl FileRead {
        #[cfg(feature = "test-seam")]
        fn text(self) -> String {
            match self {
                FileRead::Read => "ok".to_string(),
                FileRead::NoProcessId => "not-read:no-process-id".to_string(),
                FileRead::Open(code) => format!("error:open:{code:#010x}"),
                FileRead::Query(code) => format!("error:query:{code:#010x}"),
            }
        }
    }

    pub(super) fn process_file(pid: Option<i32>) -> (Option<String>, FileRead) {
        use windows::core::PWSTR;
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
            PROCESS_QUERY_LIMITED_INFORMATION,
        };
        let Some(pid) = pid.and_then(|pid| u32::try_from(pid).ok()) else {
            return (None, FileRead::NoProcessId);
        };
        // SAFETY: OpenProcess takes plain values; the handle that it returns is
        // closed once, after the query.
        let handle = match unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) } {
            Ok(handle) => handle,
            Err(error) => return (None, FileRead::Open(error.code().0)),
        };
        let mut failed = 0;
        let path = crate::windows::foreground::image_path_with(|buf, len| {
            // SAFETY: `buf` and `len` are live, `len` holds the length of
            // `buf`, and `handle` is open for the call.
            let queried = unsafe {
                QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), len)
            };
            if let Err(error) = &queried {
                failed = error.code().0;
            }
            queried
        });
        // SAFETY: `handle` is the handle that OpenProcess returned above.
        unsafe {
            let _ = CloseHandle(handle);
        }
        match path {
            Some(path) => (
                path.rsplit(['/', '\\']).next().map(str::to_string),
                FileRead::Read,
            ),
            None => (None, FileRead::Query(failed)),
        }
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
        fn read(&self, element: &IUIAutomationElement) -> (FocusRead, bool) {
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
            let pid = unsafe { element.CachedProcessId() }.ok();
            let (file, _file_read) = process_file(pid);
            let class = focus_window_class();
            let kind = kind_of(framework.as_deref(), class.as_deref(), file.as_deref());
            #[cfg(feature = "test-seam")]
            self.saw(format!(
                "framework={} window_class={} file={} kind={kind:?} is_password={} protected={} \
                 pid_read={} file_read={}",
                framework.as_deref().unwrap_or("none"),
                class.as_deref().unwrap_or("none"),
                file.as_deref().unwrap_or("none"),
                is_password.map_or("none", |p| if p { "1" } else { "0" }),
                u8::from(protected),
                u8::from(pid.is_some()),
                _file_read.text(),
            ));
            let gecko = framework
                .as_deref()
                .is_some_and(|id| id.eq_ignore_ascii_case("Gecko"));
            let read = match is_password {
                Some(is_password) => FocusRead::Answered {
                    is_password,
                    protected,
                    kind,
                },
                None => FocusRead::NoAnswer { protected },
            };
            (read, gecko)
        }

        #[cfg(feature = "test-seam")]
        fn saw(&self, what: String) {
            SEEN.with(|seen| *seen.borrow_mut() = Some(what));
        }

        fn seen(&self) -> Option<String> {
            #[cfg(feature = "test-seam")]
            return SEEN.with(|seen| seen.borrow_mut().take());
            #[cfg(not(feature = "test-seam"))]
            None
        }

        fn state(
            &self,
            element: Option<&IUIAutomationElement>,
            by: super::ReadBy,
            moves: u64,
            events_in_flight: bool,
        ) -> (FieldState, super::ContentReading) {
            let Some(element) = element else {
                #[cfg(feature = "test-seam")]
                self.saw("no_element".to_string());
                return (
                    super::unreadable_state(),
                    super::ContentReading::Unknown(super::UnknownBy::Unreadable),
                );
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
            let mut focus = None;
            let read = if same {
                #[cfg(feature = "test-seam")]
                self.saw(
                    "the_kept_password_element pid_read=not-read file_read=not-read:the-kept-element"
                        .to_string(),
                );
                FieldState::Password
            } else {
                let (read, gecko) = self.read(element);
                focus = Some((read, gecko));
                super::state_of(read, false)
            };
            let (state, change) = super::with_kept(
                kept.as_ref().map(|(_, at)| *at),
                of_the_kept,
                read,
                by,
                moves,
                events_in_flight,
            );
            match change {
                super::Kept::Leave => {}
                super::Kept::Set => *kept = Some((element.clone(), moves)),
                super::Kept::Drop => *kept = None,
            }
            let content = focus.map_or(super::ContentReading::Password, |(read, gecko)| {
                super::content_of(read, gecko, state)
            });
            (state, content)
        }

        fn own(
            &self,
            moves: u64,
            events_in_flight: bool,
        ) -> ((FieldState, super::ContentReading), super::Moved) {
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
                self.state(
                    element.as_ref(),
                    super::ReadBy::OwnRead,
                    moves,
                    events_in_flight,
                ),
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
        writer: Option<u64>,
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
        #[cfg(feature = "test-seam")]
        crate::windows::input::seam::handler_entered();
        // SAFETY: `this` is a live FocusHandler, and `sender` a borrowed
        // element or null.
        let (handler, element) = unsafe {
            (
                &*(this as *const FocusHandler),
                IUIAutomationElement::from_raw_borrowed(&sender),
            )
        };
        let moves = handler.shared.begin_event();
        let (state, content) = handler
            .reader
            .state(element, super::ReadBy::Event, moves, false);
        #[cfg(test)]
        IN_FLIGHT.with(|hook| {
            if let Some(hook) = hook.borrow().as_ref() {
                hook();
            }
        });
        handler.shared.field_seen(
            tick(),
            state,
            (content, handler.writer),
            handler.reader.seen(),
        );
        handler.wake.set();
        #[cfg(feature = "test-seam")]
        crate::windows::input::seam::handler_ended();
        S_OK
    }

    #[cfg(test)]
    thread_local! {
        static IN_FLIGHT: std::cell::RefCell<Option<Box<dyn Fn()>>> =
            const { std::cell::RefCell::new(None) };
    }

    thread_local! {
        static DESKTOP_GATE: std::cell::RefCell<Option<(Arc<Shared>, Option<u64>)>> =
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
            if let Some((shared, writer)) = cell.borrow().as_ref() {
                shared.desktop_switched(time, *writer);
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
        })
    }

    fn start(
        shared: &Arc<Shared>,
        wake: &Arc<Event>,
        writer: u64,
    ) -> Result<(Arc<Reader>, IUIAutomationFocusChangedEventHandler), CaptureError> {
        let reader = Arc::new(new_reader()?);
        let handler = FocusHandler {
            vtable: &HANDLER_VTABLE,
            refs: AtomicU32::new(1),
            shared: Arc::clone(shared),
            reader: Arc::clone(&reader),
            wake: Arc::clone(wake),
            writer: Some(writer),
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

    fn own_read(shared: &Shared, reader: &Reader, writer: Option<u64>) {
        let began = tick();
        #[cfg(feature = "test-seam")]
        crate::windows::input::seam::own_read_began(began);
        let (moves, events_in_flight, since) = {
            let marks = shared.lock();
            (marks.moves, marks.events_in_flight > 0, marks.next_seq)
        };
        let ((state, content), moved) = reader.own(moves, events_in_flight);
        shared.read_seen(
            began,
            tick(),
            (state, content, writer, since),
            moved,
            reader.seen(),
        );
        #[cfg(feature = "test-seam")]
        crate::windows::input::seam::own_read_ended();
    }

    fn set_desktop_hook() -> HWINEVENTHOOK {
        // SAFETY: an out-of-context hook with no module and a callback with the
        // WINEVENTPROC signature, served by the calling thread's pump.
        unsafe {
            SetWinEventHook(
                EVENT_SYSTEM_DESKTOPSWITCH,
                EVENT_SYSTEM_DESKTOPSWITCH,
                None,
                Some(desktop_proc),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            )
        }
    }

    fn run(
        shared: &Arc<Shared>,
        wake: &Arc<Event>,
        (removals, writer): (Removals, u64),
        ready: std::sync::mpsc::Sender<Result<(), CaptureError>>,
    ) -> Result<(), CaptureError> {
        // SAFETY: CoInitializeEx joins this new thread to the process's MTA.
        let joined = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if let Err(error) = joined.ok() {
            let _ = ready.send(Err(os("CoInitializeEx", &error)));
            return Ok(());
        }
        let _com = Com;
        let (reader, handler) = match start(shared, wake, writer) {
            Ok(started) => started,
            Err(error) => {
                let _ = ready.send(Err(error));
                return Ok(());
            }
        };
        DESKTOP_GATE.with(|cell| *cell.borrow_mut() = Some((Arc::clone(shared), Some(writer))));
        #[cfg(test)]
        let desktop = (removals.set_desktop_hook)();
        #[cfg(not(test))]
        let desktop = set_desktop_hook();
        if desktop.is_invalid() {
            let error = windows::core::Error::from_thread();
            DESKTOP_GATE.with(|cell| *cell.borrow_mut() = None);
            let _ = (removals.handler)(&reader.automation, &handler);
            let _ = ready.send(Err(os(
                "SetWinEventHook(EVENT_SYSTEM_DESKTOPSWITCH)",
                &error,
            )));
            return Ok(());
        }
        #[cfg(test)]
        (removals.before_first_read)();
        own_read(shared, &reader, Some(writer));
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
                own_read(shared, &reader, Some(writer));
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

    #[cfg(feature = "test-seam")]
    pub(crate) fn focused_element_properties() -> String {
        // SAFETY: joins the calling thread to the process's MTA; `_com` undoes
        // it when the call succeeded.
        let joined = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if let Err(error) = joined.ok() {
            return format!("uia=none com={:#x}", error.code().0);
        }
        let _com = Com;
        // SAFETY: CoCreateInstance makes the system's UI Automation object for
        // this MTA thread.
        let automation: windows::core::Result<IUIAutomation> =
            unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) };
        let Ok(automation) = automation else {
            return "uia=none".to_string();
        };
        // SAFETY: the call takes the live automation object.
        let Ok(element) = (unsafe { automation.GetFocusedElement() }) else {
            return "uia=no_focused_element".to_string();
        };
        let text = |value: windows::core::Result<windows::core::BSTR>| match value {
            Ok(value) if value.is_empty() => "-".to_string(),
            Ok(value) => value.to_string().replace([' ', '\r', '\n'], "_"),
            Err(error) => format!("err:{:#x}", error.code().0),
        };
        // SAFETY: each call reads a current property of the live element.
        unsafe {
            let legacy = element
                .GetCurrentPropertyValue(UIA_LegacyIAccessibleStatePropertyId)
                .ok()
                .and_then(|mut value| variant_i32(&mut value));
            format!(
                "is_password={} legacy_state={} protected={} control_type={} \
                 localized_control_type={} class_name={} framework={} aria_role={} \
                 aria_properties={} automation_id={} item_type={}",
                element.CurrentIsPassword().map_or_else(
                    |e| format!("err:{:#x}", e.code().0),
                    |p| p.as_bool().to_string()
                ),
                legacy.map_or("none".to_string(), |state| format!("{state:#x}")),
                legacy.is_some_and(|state| state as u32 & STATE_SYSTEM_PROTECTED != 0),
                element
                    .CurrentControlType()
                    .map_or_else(|e| format!("err:{:#x}", e.code().0), |t| t.0.to_string()),
                text(element.CurrentLocalizedControlType()),
                text(element.CurrentClassName()),
                text(element.CurrentFrameworkId()),
                text(element.CurrentAriaRole()),
                text(element.CurrentAriaProperties()),
                text(element.CurrentAutomationId()),
                text(element.CurrentItemType()),
            )
        }
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

        static HOLD_REMOVAL: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);

        fn handler_held(
            automation: &IUIAutomation,
            handler: &IUIAutomationFocusChangedEventHandler,
        ) -> windows::core::Result<()> {
            while HOLD_REMOVAL.load(std::sync::atomic::Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(10));
            }
            remove_handler(automation, handler)
        }

        fn refused_at(stage: &'static str) -> CaptureError {
            os(stage, &refused())
        }

        const UNTIMED: Duration = Duration::MAX;

        fn untimed() -> Removals {
            Removals {
                attach_deadline: Some(UNTIMED),
                ..Removals::default()
            }
        }

        fn until(fact: impl Fn() -> bool) {
            while !fact() {
                std::thread::sleep(Duration::from_millis(5));
            }
        }

        #[test]
        fn until_asks_its_fact_again_until_it_holds_and_then_no_more() {
            use std::sync::atomic::{AtomicUsize, Ordering};
            let asked = AtomicUsize::new(0);
            until(|| asked.fetch_add(1, Ordering::SeqCst) + 1 == 3);
            assert_eq!(asked.load(Ordering::SeqCst), 3);
        }

        struct Hold(&'static std::sync::atomic::AtomicBool);

        impl Hold {
            fn on(flag: &'static std::sync::atomic::AtomicBool) -> Hold {
                flag.store(true, std::sync::atomic::Ordering::SeqCst);
                Hold(flag)
            }
        }

        impl Drop for Hold {
            fn drop(&mut self) {
                self.0.store(false, std::sync::atomic::Ordering::SeqCst);
            }
        }

        #[test]
        fn a_hold_is_let_go_when_its_test_unwinds() {
            static HELD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
            let unwound = std::panic::catch_unwind(|| {
                let _hold = Hold::on(&HELD);
                panic!("an unwind inside the hold");
            });
            assert!(unwound.is_err(), "the closure did not unwind");
            assert!(
                !HELD.load(std::sync::atomic::Ordering::SeqCst),
                "the hold outlived the unwind of its test"
            );
        }

        #[test]
        fn after_attach_the_first_mark_is_the_start_and_attach_returns_after_the_detectors_own_read(
        ) {
            let _serial = crate::windows::tests::serial();
            let gate = PasswordGate::with_removals(untimed());
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
            let judged = super::super::judge(&gate.shared.lock(), &before, at, 0, false);
            let detached = gate.detach(UNTIMED);
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
            let (own, content) = reader.state(None, super::super::ReadBy::OwnRead, 0, false);
            let (event, _) = reader.state(None, super::super::ReadBy::Event, 3, false);
            let seen = reader.seen();
            drop(reader);
            assert_eq!(own, FieldState::Unknown);
            assert_eq!(event, FieldState::Unknown);
            assert_eq!(
                content,
                super::super::ContentReading::Unknown(super::super::UnknownBy::Unreadable)
            );
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
            DESKTOP_GATE.with(|cell| *cell.borrow_mut() = Some((Arc::clone(&gate.shared), None)));
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
                EditInFront::open_with(0)
            }

            fn open_with(style: u32) -> (EditInFront, bool) {
                use std::sync::atomic::{AtomicBool, Ordering};
                use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
                use windows::Win32::UI::Input::KeyboardAndMouse::{
                    SendInput, SetFocus, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
                    KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VK_MENU,
                };
                use windows::Win32::UI::WindowsAndMessaging::{
                    BringWindowToTop, CreateWindowExW, DestroyWindow, SetForegroundWindow,
                    ShowWindow, SW_SHOW, WINDOW_EX_STYLE, WINDOW_STYLE, WS_OVERLAPPEDWINDOW,
                    WS_VISIBLE,
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
                            WS_OVERLAPPEDWINDOW | WS_VISIBLE | WINDOW_STYLE(style),
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
                writer: None,
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
            let gate = PasswordGate::with_removals(untimed());
            gate.attach().expect("the detector starts");
            let first = gate.detach(UNTIMED);
            let running = gate
                .detector
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_some();
            gate.attach().expect("the detector starts again");
            let second = gate.detach(UNTIMED);
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
                ..untimed()
            });
            both.attach().expect("the detector starts");
            let both = both.detach(UNTIMED);
            let hook = PasswordGate::with_removals(Removals {
                desktop_hook: desktop_refused,
                ..untimed()
            });
            hook.attach().expect("the detector starts");
            let hook = hook.detach(UNTIMED);
            assert_eq!(both, Err(refused_at("RemoveFocusChangedEventHandler")));
            assert_eq!(hook, Err(refused_at("UnhookWinEvent")));
        }

        static HOLD_READS: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);

        static READ_HELD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

        fn read_held() {
            while HOLD_READS.load(std::sync::atomic::Ordering::SeqCst) {
                READ_HELD.store(true, std::sync::atomic::Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(10));
            }
        }

        #[test]
        fn an_attach_after_a_timed_out_detach_is_refused_while_the_old_detector_runs() {
            use std::sync::atomic::Ordering;
            let _serial = crate::windows::tests::serial();
            HOLD_READS.store(false, Ordering::SeqCst);
            let mut gate = PasswordGate::with_removals(Removals {
                before_read: read_held,
                ..untimed()
            });
            READ_HELD.store(false, Ordering::SeqCst);
            gate.attach().expect("the detector starts");
            let reads = Hold::on(&HOLD_READS);
            gate.shared.lock().ask_read(tick());
            gate.wake_detector();
            until(|| READ_HELD.load(Ordering::SeqCst));
            let stopped = gate.detach(Duration::from_millis(200));
            gate.removals.attach_deadline = None;
            let second = gate.attach();
            let running_after_the_second = gate
                .detector
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_some();
            gate.removals.attach_deadline = Some(UNTIMED);
            drop(reads);
            let third = gate.attach();
            let detached = gate.detach(UNTIMED);
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
            use std::sync::atomic::Ordering;
            let _serial = crate::windows::tests::serial();
            HOLD_REMOVAL.store(false, Ordering::SeqCst);
            let gate = PasswordGate::with_removals(Removals {
                handler: handler_held,
                ..untimed()
            });
            gate.attach().expect("the detector starts");
            let removal = Hold::on(&HOLD_REMOVAL);
            let deadline = Duration::from_millis(200);
            let stopped = gate.detach(deadline);
            drop(removal);
            let stale = gate
                .stale
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            if let Some(stale) = stale {
                let _ = stale.join();
            }
            assert_eq!(
                stopped,
                Err(CaptureError::Timeout {
                    stage: "stop the password detector",
                    ms: 200,
                })
            );
        }

        static HANDLERS_REMOVED: std::sync::atomic::AtomicUsize =
            std::sync::atomic::AtomicUsize::new(0);

        fn handler_counted(
            automation: &IUIAutomation,
            handler: &IUIAutomationFocusChangedEventHandler,
        ) -> windows::core::Result<()> {
            HANDLERS_REMOVED.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let removed = remove_handler(automation, handler);
            // SAFETY: SetLastError takes a plain value for the calling thread.
            unsafe {
                windows::Win32::Foundation::SetLastError(
                    windows::Win32::Foundation::ERROR_INVALID_PARAMETER,
                );
            }
            removed
        }

        fn no_desktop_hook() -> HWINEVENTHOOK {
            // SAFETY: SetLastError takes a plain value for the calling thread.
            unsafe {
                windows::Win32::Foundation::SetLastError(
                    windows::Win32::Foundation::ERROR_ACCESS_DENIED,
                );
            }
            HWINEVENTHOOK::default()
        }

        #[test]
        fn a_desktop_hook_that_cannot_be_set_refuses_the_attach_and_removes_the_focus_handler() {
            use std::sync::atomic::Ordering;
            let _serial = crate::windows::tests::serial();
            HANDLERS_REMOVED.store(0, Ordering::SeqCst);
            let control = PasswordGate::with_removals(Removals {
                handler: handler_counted,
                ..untimed()
            });
            let control_attached = control.attach();
            let removed_by_the_control = HANDLERS_REMOVED.load(Ordering::SeqCst);
            let control_detached = control.detach(UNTIMED);
            HANDLERS_REMOVED.store(0, Ordering::SeqCst);
            let gate = PasswordGate::with_removals(Removals {
                handler: handler_counted,
                set_desktop_hook: no_desktop_hook,
                ..untimed()
            });
            let attached = gate.attach();
            let running = gate
                .detector
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_some();
            let removed = HANDLERS_REMOVED.load(Ordering::SeqCst);
            assert_eq!(control_attached, Ok(()));
            assert_eq!(removed_by_the_control, 0);
            assert_eq!(control_detached, Ok(()));
            assert!(
                matches!(
                    &attached,
                    Err(CaptureError::Os {
                        stage: "SetWinEventHook(EVENT_SYSTEM_DESKTOPSWITCH)",
                        ..
                    })
                ),
                "{attached:?}"
            );
            assert!(!running, "a refused attach keeps a detector");
            assert_eq!(removed, 1, "the focus handler was not removed");
            assert_eq!(
                attached,
                Err(CaptureError::Os {
                    stage: "SetWinEventHook(EVENT_SYSTEM_DESKTOPSWITCH)",
                    domain: "HRESULT",
                    code: i64::from(
                        windows::Win32::Foundation::ERROR_ACCESS_DENIED
                            .to_hresult()
                            .0
                    ),
                }),
                "the refusal does not carry the code of the call that set the hook"
            );
        }

        static HOLD_FIRST_READ: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);

        fn first_read_held() {
            while HOLD_FIRST_READ.load(std::sync::atomic::Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(10));
            }
        }

        static HOLD_RECEIVE: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);

        static RECEIVES_MET: std::sync::atomic::AtomicUsize =
            std::sync::atomic::AtomicUsize::new(0);

        static RECEIVE_TURNS: std::sync::atomic::AtomicUsize =
            std::sync::atomic::AtomicUsize::new(0);

        fn receive_met() {
            RECEIVES_MET.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            while HOLD_RECEIVE.load(std::sync::atomic::Ordering::SeqCst) {
                RECEIVE_TURNS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(10));
            }
        }

        #[test]
        fn receive_met_keeps_its_caller_while_the_receive_is_held() {
            use std::sync::atomic::Ordering;
            let _serial = crate::windows::tests::serial();
            HOLD_RECEIVE.store(false, Ordering::SeqCst);
            RECEIVES_MET.store(0, Ordering::SeqCst);
            RECEIVE_TURNS.store(0, Ordering::SeqCst);
            let receive = Hold::on(&HOLD_RECEIVE);
            let caller = std::thread::spawn(receive_met);
            until(|| RECEIVE_TURNS.load(Ordering::SeqCst) >= 2);
            let ended_under_the_hold = caller.is_finished();
            drop(receive);
            caller.join().expect("the caller of receive_met ends");
            assert_eq!(RECEIVES_MET.load(Ordering::SeqCst), 1);
            assert!(
                !ended_under_the_hold,
                "receive_met let its caller go while the receive was held"
            );
        }

        #[test]
        fn a_detector_that_its_attach_or_its_detach_gave_up_writes_no_content_reading() {
            use super::super::{ClearBy, ContentReading, UnknownBy};
            use std::sync::atomic::Ordering;
            let _serial = crate::windows::tests::serial();
            let (edit, in_front) = EditInFront::open();
            let ended = |gate: &PasswordGate| {
                gate.stale
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .as_ref()
                    .is_none_or(|join| join.is_finished())
            };
            HOLD_FIRST_READ.store(false, Ordering::SeqCst);
            HOLD_READS.store(false, Ordering::SeqCst);
            let mut gate = PasswordGate::with_removals(Removals {
                before_first_read: first_read_held,
                before_read: read_held,
                attach_deadline: Some(Duration::from_millis(300)),
                ..Removals::default()
            });
            let first_read = Hold::on(&HOLD_FIRST_READ);
            let given_up = gate.attach();
            drop(first_read);
            until(|| ended(&gate));
            let after_a_late_first_read = gate.content_reading();
            gate.removals.attach_deadline = Some(UNTIMED);
            let attached = gate.attach();
            let while_it_runs = gate.content_reading();
            READ_HELD.store(false, Ordering::SeqCst);
            let reads = Hold::on(&HOLD_READS);
            gate.shared.lock().ask_read(tick());
            gate.wake_detector();
            until(|| READ_HELD.load(Ordering::SeqCst));
            let stopped = gate.detach(Duration::from_millis(200));
            let after_a_timed_out_detach = gate.content_reading();
            gate.removals.attach_deadline = None;
            let refused = gate.attach();
            let after_a_refused_attach = gate.content_reading();
            drop(reads);
            until(|| ended(&gate));
            let after_the_held_read = gate.content_reading();
            drop(edit);
            assert!(
                in_front,
                "INCONCLUSIVE: the test's edit did not come to the front"
            );
            assert!(
                matches!(given_up, Err(CaptureError::Timeout { .. })),
                "{given_up:?}"
            );
            assert_eq!(attached, Ok(()));
            assert_eq!(
                while_it_runs,
                ContentReading::Clear(ClearBy::NativeEdit),
                "the control: a detector that runs reads the edit"
            );
            assert!(
                matches!(stopped, Err(CaptureError::Timeout { .. })),
                "{stopped:?}"
            );
            assert!(
                matches!(refused, Err(CaptureError::Timeout { .. })),
                "{refused:?}"
            );
            let unknown = ContentReading::Unknown(UnknownBy::BeforeFirstRead);
            assert_eq!(
                [
                    after_a_late_first_read,
                    after_a_timed_out_detach,
                    after_a_refused_attach,
                    after_the_held_read,
                ],
                [unknown; 4],
                "after a late first read, a timed-out detach, a refused attach and the held read"
            );
        }

        #[test]
        fn a_first_read_that_lands_after_the_deadline_holds_and_nothing_is_kept_before_it() {
            use super::super::{Attached, ClearBy, ContentReading, UnknownBy};
            use crate::floor::exclude::{FloorHandle, PrivateWindow};
            use crate::floor::payload::TitleWhy;
            use crate::windows::window_title::title_event;
            use crate::windows::{now_tick, Floor, FrontApp, OsTick};
            use std::sync::atomic::Ordering;
            let _serial = crate::windows::tests::serial();
            let (edit, in_front) = EditInFront::open();
            let floor = Floor::new(FloorHandle::default(), Arc::default());
            let front = FrontApp {
                pid: 9,
                path: Some("C:\\Apps\\mail.exe".to_string()),
            };
            let kept = |gate: &PasswordGate| {
                title_event(
                    &floor,
                    Some(gate),
                    (&front, PrivateWindow::Unknown),
                    ("Inbox".to_string(), TitleWhy::TitleChange),
                    OsTick::new(now_tick()),
                )
                .is_some()
            };
            HOLD_FIRST_READ.store(false, Ordering::SeqCst);
            let gate = PasswordGate::with_removals(Removals {
                before_first_read: first_read_held,
                attach_deadline: Some(Duration::from_millis(300)),
                ..Removals::default()
            });
            let first_read = Hold::on(&HOLD_FIRST_READ);
            let attached = gate.attach_waiting();
            let running_while_late = gate.is_attached();
            let before = (gate.content_reading(), kept(&gate));
            drop(first_read);
            let landed = std::time::Instant::now();
            while !matches!(gate.content_reading(), ContentReading::Clear(_))
                && landed.elapsed() < Duration::from_secs(10)
            {
                std::thread::sleep(Duration::from_millis(5));
            }
            let after = (gate.content_reading(), kept(&gate));
            let detached = gate.detach(UNTIMED);
            let after_the_detach = gate.content_reading();
            let second = PasswordGate::with_removals(Removals {
                before_first_read: first_read_held,
                attach_deadline: Some(Duration::from_millis(300)),
                ..Removals::default()
            });
            let first_read = Hold::on(&HOLD_FIRST_READ);
            let second_attached = second.attach_waiting();
            let (password, password_in_front) = EditInFront::open_with(ES_PASSWORD as u32);
            let before_with_a_password_field = (second.content_reading(), kept(&second));
            drop(first_read);
            let landed = std::time::Instant::now();
            while second.content_reading() == ContentReading::Unknown(UnknownBy::BeforeFirstRead)
                && landed.elapsed() < Duration::from_secs(10)
            {
                std::thread::sleep(Duration::from_millis(5));
            }
            let after_with_a_password_field = (second.content_reading(), kept(&second));
            let _ = second.detach(UNTIMED);
            drop(password);
            drop(edit);
            assert!(
                in_front && password_in_front,
                "INCONCLUSIVE: the test's edits did not come to the front"
            );
            assert_eq!(attached, Ok(Attached::Late));
            assert_eq!(second_attached, Ok(Attached::Late));
            assert_eq!(
                before_with_a_password_field,
                (ContentReading::Unknown(UnknownBy::BeforeFirstRead), false),
                "a focus event to a password field before the first read lands"
            );
            assert_eq!(
                after_with_a_password_field,
                (ContentReading::Password, false),
                "the first read lands with a password field in the focus"
            );
            assert!(running_while_late, "the late detector was not kept");
            assert_eq!(
                before,
                (ContentReading::Unknown(UnknownBy::BeforeFirstRead), false),
                "before the first read lands: the reading and a title"
            );
            assert_eq!(
                after,
                (ContentReading::Clear(ClearBy::NativeEdit), true),
                "after the first read lands: the reading and a title"
            );
            assert_eq!(detached, Ok(()));
            assert_eq!(
                after_the_detach,
                ContentReading::Unknown(UnknownBy::BeforeFirstRead)
            );
        }

        #[test]
        fn an_attach_whose_first_read_does_not_answer_times_out_and_stops_that_detector() {
            use std::sync::atomic::Ordering;
            let _serial = crate::windows::tests::serial();
            HOLD_FIRST_READ.store(false, Ordering::SeqCst);
            HOLD_RECEIVE.store(false, Ordering::SeqCst);
            let gate = Arc::new(PasswordGate::with_removals(Removals {
                before_first_read: first_read_held,
                at_receive: receive_met,
                ..Removals::default()
            }));
            let first_read = Hold::on(&HOLD_FIRST_READ);
            let receive = Hold::on(&HOLD_RECEIVE);
            RECEIVES_MET.store(0, Ordering::SeqCst);
            let waiting = {
                let gate = Arc::clone(&gate);
                std::thread::spawn(move || gate.attach())
            };
            until(|| RECEIVES_MET.load(Ordering::SeqCst) == 1);
            let free_during_the_wait = gate.detector.try_lock().is_ok();
            drop(receive);
            let first = waiting.join().expect("the attaching thread ends");
            let second = gate.attach();
            let mut gate = Arc::into_inner(gate).expect("the attaching thread let the gate go");
            gate.removals.attach_deadline = Some(UNTIMED);
            drop(first_read);
            let third = gate.attach();
            let detached = gate.detach(UNTIMED);
            assert_eq!(
                first,
                Err(CaptureError::Timeout {
                    stage: "wait for the password detector's first read",
                    ms: 1_000,
                })
            );
            assert!(
                free_during_the_wait,
                "the attach held the detector's lock across its wait"
            );
            assert_eq!(
                second,
                Err(CaptureError::Timeout {
                    stage: "wait for the earlier password detector to end",
                    ms: 1_000,
                })
            );
            assert_eq!(third, Ok(()), "the stopped detector did not end");
            assert_eq!(detached, Ok(()));
        }

        #[test]
        fn the_process_file_is_the_own_executables_name_and_nothing_for_an_ended_process() {
            let exe = std::env::current_exe().expect("the test's executable");
            let name = exe
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string);
            let mut child = std::process::Command::new("cmd")
                .args(["/c", "exit"])
                .spawn()
                .expect("a child process");
            let pid = child.id();
            let ended = child.wait();
            drop(child);
            let own = process_file(i32::try_from(std::process::id()).ok());
            let of_the_ended = process_file(i32::try_from(pid).ok());
            assert!(ended.is_ok(), "INCONCLUSIVE: the child did not end");
            assert_eq!(own, (name, FileRead::Read));
            assert_eq!(of_the_ended.0, None, "{of_the_ended:?}");
        }

        fn element_of(reader: &Reader, hwnd: usize) -> IUIAutomationElement {
            // SAFETY: the window lives while its EditInFront lives, and the
            // cache request is live for the call.
            unsafe {
                reader
                    .automation
                    .ElementFromHandleBuildCache(HWND(hwnd as *mut c_void), &reader.cache)
            }
            .expect("the edit's element")
        }

        fn handler_of(
            gate: &PasswordGate,
            reader: &Arc<Reader>,
        ) -> IUIAutomationFocusChangedEventHandler {
            FocusHandler {
                vtable: &HANDLER_VTABLE,
                refs: AtomicU32::new(1),
                shared: Arc::clone(&gate.shared),
                reader: Arc::clone(reader),
                wake: Arc::new(Event::new().expect("an event")),
                writer: None,
            }
            .into_interface()
        }

        #[test]
        fn an_event_of_another_element_does_not_let_the_kept_password_element_go() {
            const EM_SETPASSWORDCHAR: u32 = 0x00CC;
            use windows::Win32::Foundation::{LPARAM, WPARAM};
            use windows::Win32::UI::WindowsAndMessaging::{SendMessageW, ES_PASSWORD};
            let _serial = crate::windows::tests::serial();
            // SAFETY: joins this test thread to the process's MTA; `_com`
            // undoes it after the reader and the handler are dropped.
            let joined = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            assert!(joined.is_ok(), "{joined:?}");
            let _com = Com;
            let (other, _) = EditInFront::open();
            let (kept, in_front) = EditInFront::open_with(ES_PASSWORD as u32);
            assert!(
                in_front,
                "INCONCLUSIVE: the test's password edit did not come to the front"
            );
            let reader = Arc::new(new_reader().expect("the reader is made"));
            let kept_element = element_of(&reader, kept.hwnd);
            let other_element = element_of(&reader, other.hwnd);
            let gate = PasswordGate::new();
            let handler = handler_of(&gate, &reader);
            gate.read_at(
                tick().wrapping_sub(400),
                FieldState::Clear,
                Moved::NotCompared,
            );
            // SAFETY: the handler and the element are live COM objects for
            // the call, which borrows both.
            let first_answer = unsafe { handler_focus(handler.as_raw(), kept_element.as_raw()) };
            let at_the_kept = gate.field_now();
            // SAFETY: SendMessageW takes the live edit's handle and plain
            // values; the edit's thread pumps its messages.
            unsafe {
                let _ = SendMessageW(
                    HWND(kept.hwnd as *mut c_void),
                    EM_SETPASSWORDCHAR,
                    Some(WPARAM(0)),
                    Some(LPARAM(0)),
                );
            }
            let unmasked =
                super::super::state_of(reader.read(&element_of(&reader, kept.hwnd)).0, false);
            // SAFETY: as above.
            let second_answer = unsafe { handler_focus(handler.as_raw(), other_element.as_raw()) };
            std::thread::sleep(Duration::from_millis(50));
            let key = gate.hold(tick(), false, 0);
            std::thread::sleep(Duration::from_millis(150));
            own_read(&gate.shared, &reader, None);
            let at_the_own_read = gate.field_now();
            let decided = gate
                .decide(&key, key.tick.wrapping_add(200), 0, false)
                .map(|(answer, rule)| (answer.is_ok(), rule));
            println!(
                "TF r1 at_the_kept={at_the_kept:?} unmasked={unmasked:?} \
                 at_the_own_read={at_the_own_read:?} decided={decided:?}"
            );
            drop(handler);
            drop(kept_element);
            drop(other_element);
            drop(reader);
            drop(kept);
            drop(other);
            assert_eq!((first_answer, second_answer), (S_OK, S_OK));
            assert_eq!(
                at_the_kept,
                FieldState::Password,
                "INCONCLUSIVE: the password edit's event did not read Password"
            );
            assert_eq!(
                unmasked,
                FieldState::Clear,
                "INCONCLUSIVE: the unmasked edit does not read Clear on its own"
            );
            assert_eq!(at_the_own_read, FieldState::Password);
            assert_eq!(
                decided.map(|(cleared, _)| cleared),
                Some(false),
                "{decided:?}"
            );
        }

        #[test]
        fn an_own_read_between_a_focus_events_start_and_its_mark_does_not_let_its_reading_go() {
            use windows::Win32::UI::WindowsAndMessaging::ES_PASSWORD;
            let _serial = crate::windows::tests::serial();
            // SAFETY: joins this test thread to the process's MTA; `_com`
            // undoes it after the reader and the handler are dropped.
            let joined = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            assert!(joined.is_ok(), "{joined:?}");
            let _com = Com;
            let (password, _) = EditInFront::open_with(ES_PASSWORD as u32);
            let (plain, in_front) = EditInFront::open();
            assert!(
                in_front,
                "INCONCLUSIVE: the test's plain edit did not come to the front"
            );
            let reader = Arc::new(new_reader().expect("the reader is made"));
            let password_element = element_of(&reader, password.hwnd);
            let gate = PasswordGate::new();
            let handler = handler_of(&gate, &reader);
            gate.read_at(
                tick().wrapping_sub(400),
                FieldState::Clear,
                Moved::NotCompared,
            );
            let in_flight = Arc::new(std::sync::Mutex::new(None));
            {
                let shared = Arc::clone(&gate.shared);
                let reader = Arc::clone(&reader);
                let in_flight = Arc::clone(&in_flight);
                IN_FLIGHT.with(|hook| {
                    *hook.borrow_mut() = Some(Box::new(move || {
                        shared.latch(tick(), super::super::LatchFrom::TheInputThread);
                        own_read(&shared, &reader, None);
                        let read =
                            shared
                                .lock()
                                .entries
                                .back()
                                .and_then(|entry| match entry.mark {
                                    super::super::Mark::Read { state, .. } => Some(state),
                                    _ => None,
                                });
                        *in_flight
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner) = read;
                    }));
                });
            }
            // SAFETY: the handler and the element are live COM objects for
            // the call, which borrows both.
            let answer = unsafe { handler_focus(handler.as_raw(), password_element.as_raw()) };
            IN_FLIGHT.with(|hook| *hook.borrow_mut() = None);
            let at_the_mark = gate.field_now();
            let read_in_flight = *in_flight
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            println!(
                "TF handler-order read_in_flight={read_in_flight:?} at_the_mark={at_the_mark:?}"
            );
            drop(handler);
            drop(password_element);
            drop(reader);
            drop(plain);
            drop(password);
            assert_eq!(answer, S_OK);
            assert_eq!(
                at_the_mark,
                FieldState::Password,
                "INCONCLUSIVE: the password edit's event did not read Password"
            );
            assert_eq!(
                read_in_flight,
                Some(FieldState::Password),
                "an own read of another element let the event's reading go before its mark"
            );
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
            with_kept(Some(3), Same, Clear, Event, 9, false),
            (Password, Kept::Set)
        );
        assert_eq!(
            with_kept(None, Other, Password, Event, 4, false),
            (Password, Kept::Set)
        );
        assert_eq!(
            with_kept(Some(3), Other, Clear, Event, 9, false),
            (Clear, Kept::Leave)
        );
        assert_eq!(
            with_kept(Some(3), Other, Clear, OwnRead, 3, false),
            (Password, Kept::Leave)
        );
        assert_eq!(
            with_kept(Some(3), Other, Clear, OwnRead, 4, false),
            (Clear, Kept::Drop)
        );
        assert_eq!(
            with_kept(None, Other, Clear, OwnRead, 4, false),
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
    fn a_read_that_shows_a_move_after_the_keys_time_and_before_the_pump_took_it_withholds_the_key()
    {
        let gate = plain();
        gate.read_at(2_050, FieldState::Clear, Moved::Another);
        let letter = key(&gate, 2_000);
        gate.read_at(2_125, FieldState::Clear, Moved::Same);
        assert_eq!(reading(&gate, &letter, 2_130), Some(Err(Withheld::Unknown)));
        assert_eq!(rule(&gate, &letter, 2_130), Some(Rule::ElementNotTheSame));
    }

    #[test]
    fn a_key_at_a_gate_that_reads_unknown_is_withheld_at_once_and_asks_for_no_read() {
        let gate = PasswordGate::new();
        gate.started_at(1_000);
        gate.read_at(1_005, FieldState::Unknown, Moved::NotCompared);
        for at in [2_000, 2_300, 2_600] {
            let withheld = key(&gate, at);
            assert_eq!(
                reading(&gate, &withheld, at + 1),
                Some(Err(Withheld::Unknown))
            );
            assert_eq!(rule(&gate, &withheld, at + 1), Some(Rule::StateAtTheKey));
        }
        assert_eq!(gate.shared.lock().read_due, None);
        let open = plain();
        let waiting = key(&open, 2_000);
        assert_eq!(reading(&open, &waiting, 2_001), None);
        assert_eq!(open.shared.lock().read_due, Some(2_000 + HOLD));
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
        gate.shared.desktop_switched(2_010, None);
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
    fn a_read_that_ends_in_an_error_reads_unknown() {
        assert_eq!(unreadable_state(), FieldState::Unknown);
    }

    #[test]
    fn key_hold_is_above_the_largest_delay_that_s3_measured() {
        assert!(KEY_HOLD > S3_LARGEST_KEY_TO_FOCUS_EVENT);
        assert_eq!(S3_LARGEST_KEY_TO_FOCUS_EVENT, Duration::from_micros(75_300));
    }

    #[test]
    fn a_read_older_than_half_the_tick_range_is_no_deciding_read() {
        let gate = PasswordGate::new();
        let k: u32 = 3_000_000_000;
        let old = k.wrapping_sub(0x8000_0000).wrapping_sub(500);
        gate.read_at(old, FieldState::Clear, Moved::Same);
        gate.read_at(k - 10, FieldState::Clear, Moved::Same);
        let letter = key(&gate, k);
        gate.field_at(k + 75, FieldState::Password);
        assert_ne!(reading(&gate, &letter, k + 80), Some(Ok(())));
    }

    #[test]
    fn an_open_latch_from_another_thread_holds_a_key_half_the_tick_range_later() {
        let gate = PasswordGate::new();
        gate.latch_after(2_000);
        let k: u32 = 2_000u32.wrapping_add(0x8000_0000).wrapping_add(10);
        gate.read_at(k - 10, FieldState::Clear, Moved::Same);
        let letter = key(&gate, k);
        gate.read_at(k + 120, FieldState::Clear, Moved::Same);
        assert_ne!(reading(&gate, &letter, k + 125), Some(Ok(())));
    }

    #[test]
    fn a_click_route_lost_half_the_tick_range_ago_still_withholds_a_key() {
        let gate = PasswordGate::new();
        gate.clicks_lost(2_000);
        let k: u32 = 2_000u32.wrapping_add(0x8000_0000).wrapping_add(10);
        gate.read_at(k - 10, FieldState::Clear, Moved::Same);
        let letter = key(&gate, k);
        gate.read_at(k + 120, FieldState::Clear, Moved::Same);
        assert_ne!(reading(&gate, &letter, k + 125), Some(Ok(())));
    }

    #[test]
    fn a_state_mark_older_than_the_age_limit_reads_unknown_at_the_key() {
        let gate = PasswordGate::new();
        let k: u32 = 100_000_000;
        gate.read_at(k - 86_400_001, FieldState::Clear, Moved::NotCompared);
        let letter = key(&gate, k);
        gate.read_at(k + 120, FieldState::Clear, Moved::Same);
        assert_eq!(
            reading(&gate, &letter, k + 130),
            Some(Err(Withheld::Unknown))
        );
    }

    fn behind_a_latch_and_reads(
        latch: fn(&PasswordGate, u32),
        reads: u32,
    ) -> Option<Result<(), Withheld>> {
        let gate = PasswordGate::new();
        gate.read_at(1_000, FieldState::Clear, Moved::NotCompared);
        latch(&gate, 2_000);
        let mut t = 2_100;
        for _ in 0..reads {
            gate.read_at(t, FieldState::Clear, Moved::Same);
            t += 30;
        }
        let letter = key(&gate, t + 5);
        gate.read_at(t + 130, FieldState::Clear, Moved::Same);
        gate.read_at(t + 160, FieldState::Clear, Moved::Same);
        reading(&gate, &letter, t + 200)
    }

    #[test]
    fn a_latch_that_no_move_closes_never_leaves_the_gate() {
        let first = (200..600).find(|&reads| {
            behind_a_latch_and_reads(|gate, at| gate.latch(at), reads) == Some(Ok(()))
        });
        assert_eq!(first, None, "the first count of reads that clears");
    }

    #[test]
    fn a_click_that_moves_nothing_then_300_reads_of_the_same_element_withhold_every_key() {
        let gate = PasswordGate::new();
        gate.read_at(1_000, FieldState::Clear, Moved::NotCompared);
        gate.latch_after(2_000);
        let mut t = 2_100;
        let mut cleared = 0;
        for round in 0..300 {
            gate.read_at(t, FieldState::Clear, Moved::Same);
            if round % 10 == 0 {
                let letter = key(&gate, t + 5);
                gate.read_at(t + 130, FieldState::Clear, Moved::Same);
                gate.read_at(t + 160, FieldState::Clear, Moved::Same);
                cleared += usize::from(reading(&gate, &letter, t + 200) == Some(Ok(())));
                t += 160;
            }
            t += 30;
        }
        assert_eq!(cleared, 0);
    }

    #[test]
    fn a_key_with_a_password_mark_and_then_256_clear_reads_inside_its_250_ms_is_withheld() {
        let gate = plain();
        let letter = key(&gate, 2_000);
        gate.field_at(2_010, FieldState::Password);
        for i in 0..256u32 {
            gate.read_at(2_120 + i / 2, FieldState::Clear, Moved::Same);
        }
        assert_eq!(reading(&gate, &letter, 2_249), Some(Err(Withheld::Unknown)));
    }

    #[test]
    fn a_mark_of_a_held_keys_span_that_leaves_the_ring_withholds_it_by_a_rule_of_its_own() {
        let gate = plain();
        let letter = key(&gate, 2_000);
        for i in 0..257u32 {
            gate.read_at(2_120 + i / 2, FieldState::Clear, Moved::Same);
        }
        assert_eq!(rule(&gate, &letter, 2_249), Some(Rule::MarksLost));
    }

    #[test]
    fn the_ring_keeps_the_newest_256_marks_and_every_latch_that_is_open() {
        let gate = PasswordGate::new();
        gate.latch(0);
        for at in 1..300u32 {
            gate.read_at(at, FieldState::Clear, Moved::Same);
        }
        let marks = gate.shared.lock();
        assert_eq!(marks.entries.len(), 256);
        assert_eq!(marks.entries.front().map(|entry| entry.began), Some(44));
        assert_eq!(marks.open.len(), 1);
    }

    #[derive(Debug, Clone, Copy)]
    struct ReferenceLatch {
        seq: u64,
        closed_by: Option<u64>,
    }

    fn reference_latch_states(marks: &VecDeque<Entry>) -> Vec<ReferenceLatch> {
        let reads: Vec<&Entry> = marks
            .iter()
            .filter(|entry| matches!(entry.mark, Mark::Read { .. }))
            .collect();
        let mut states = Vec::new();
        let mut floor = 0u64;
        let mut blocked = false;
        for entry in marks.iter() {
            let Mark::Latch { at, .. } = entry.mark else {
                continue;
            };
            let mut state = ReferenceLatch {
                seq: entry.seq,
                closed_by: None,
            };
            if !blocked {
                let settle = at.wrapping_add(ms(LATCH_SETTLE));
                let base = reads.iter().find(|read| {
                    read.seq > entry.seq && read.seq >= floor && not_after(settle, read.began)
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
                        state.closed_by = Some(read.seq);
                    }
                    None => blocked = true,
                }
            }
            states.push(state);
        }
        states
    }

    #[test]
    fn the_latches_that_the_gate_keeps_are_those_of_the_rule_over_every_mark() {
        for seed in 1..=200u64 {
            let gate = PasswordGate::new();
            let mut draw = seed;
            let mut next = move || {
                draw = draw
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                (draw >> 33) as u32
            };
            let mut tick = 10_000u32;
            for _ in 0..200 {
                let r = next();
                tick = tick.wrapping_add(r % 40);
                match (r >> 8) % 8 {
                    0 => gate.latch(tick),
                    1 => gate.latch_after(tick),
                    2 => gate.read_at(tick, FieldState::Clear, Moved::Another),
                    3 => gate.read_at(tick, FieldState::Clear, Moved::NotCompared),
                    _ => gate.read_at(tick, FieldState::Clear, Moved::Same),
                }
                let marks = gate.shared.lock();
                let reference = reference_latch_states(&marks.entries);
                let open: Vec<u64> = reference
                    .iter()
                    .filter(|latch| latch.closed_by.is_none())
                    .map(|latch| latch.seq)
                    .collect();
                let kept_open: Vec<u64> = marks.open.iter().map(|latch| latch.seq).collect();
                assert_eq!(kept_open, open, "seed {seed}");
                for latch in reference.iter().filter(|latch| latch.closed_by.is_some()) {
                    let kept = marks.closed.iter().find(|closed| closed.seq == latch.seq);
                    if let Some(kept) = kept {
                        assert_eq!(Some(kept.closing_seq), latch.closed_by, "seed {seed}");
                    }
                }
            }
        }
    }

    fn a_click_forwarded_late_by_the_pointer(click_before: Option<u32>) -> Option<(bool, Rule)> {
        let k: u32 = 200_000;
        let gate = PasswordGate::new();
        gate.read_at(k - 100_000, FieldState::Clear, Moved::NotCompared);
        gate.pointer_takes_the_clicks(k - 99_000);
        let letter = gate.hold(k, false, 7);
        if let Some(before) = click_before {
            gate.latch_after(k - before);
        }
        gate.read_at(k + 125, FieldState::Clear, Moved::Same);
        gate.read_at(k + 155, FieldState::Clear, Moved::Same);
        gate.pointer_forwarded(k + 160);
        gate.decide(&letter, k + 170, 7, false)
            .map(|(answer, rule)| (answer.is_ok(), rule))
    }

    #[test]
    fn a_click_that_the_pointer_forwards_late_holds_a_key_whatever_its_distance_before_it() {
        assert_eq!(
            a_click_forwarded_late_by_the_pointer(None),
            Some((true, Rule::ReadAfterTheHold)),
            "the control: with no click the forwarded tick past the hold clears the key"
        );
        for before in [10, 300, 370, 371, 400, 2_000, 60_000] {
            assert_ne!(
                a_click_forwarded_late_by_the_pointer(Some(before)).map(|(cleared, _)| cleared),
                Some(true),
                "a key after a click {before} ms before it, which the pointer forwarded late, was cleared"
            );
        }
    }

    #[test]
    fn a_late_click_of_the_keys_own_tick_holds_the_key() {
        let gate = plain();
        let letter = key(&gate, 2_000);
        gate.latch_after(2_000);
        gate.read_at(2_120, FieldState::Clear, Moved::Same);
        gate.read_at(2_150, FieldState::Clear, Moved::Same);
        assert_eq!(reading(&gate, &letter, 2_160), None);
    }

    #[test]
    fn a_second_gap_of_the_click_route_does_not_hide_the_first() {
        let gate = plain();
        let letter = key(&gate, 2_001);
        gate.clicks_lost(1_990);
        gate.clicks_back(2_050);
        gate.clicks_lost(2_130);
        gate.clicks_back(2_140);
        gate.read_at(2_125, FieldState::Clear, Moved::Same);
        gate.read_at(2_155, FieldState::Clear, Moved::Same);
        assert_ne!(reading(&gate, &letter, 2_160), Some(Ok(())));
        let gate = plain();
        let letter = key(&gate, 2_000);
        gate.clicks_lost(2_010);
        gate.clicks_back(2_030);
        gate.read_at(2_120, FieldState::Clear, Moved::Same);
        gate.clicks_lost(2_200);
        gate.clicks_back(2_210);
        assert_ne!(reading(&gate, &letter, 2_215), Some(Ok(())));
    }

    #[test]
    fn in_the_pumps_order_a_second_gap_records_no_key_typed_after_an_unseen_click() {
        let gate = plain();
        let mut held = VecDeque::new();
        let mut out = Vec::new();
        let release = |gate: &PasswordGate,
                       held: &mut VecDeque<(&'static str, HeldKey)>,
                       now: u32,
                       out: &mut Vec<(&'static str, bool)>| {
            while let Some((_, waiting)) = held.front() {
                let Some((answer, _)) = gate.decide(waiting, now, FRONT, false) else {
                    break;
                };
                if let Some((name, _)) = held.pop_front() {
                    out.push((name, answer.is_ok()));
                }
            }
        };
        gate.latch_after(1_850);
        held.push_back(("K0", key(&gate, 1_900)));
        release(&gate, &mut held, 1_900, &mut out);
        gate.read_at(1_955, FieldState::Clear, Moved::Same);
        release(&gate, &mut held, 1_960, &mut out);
        gate.read_at(2_010, FieldState::Clear, Moved::Another);
        release(&gate, &mut held, 2_015, &mut out);
        gate.clicks_lost(2_021);
        held.push_back(("K", key(&gate, 2_022)));
        release(&gate, &mut held, 2_022, &mut out);
        gate.clicks_back(2_030);
        release(&gate, &mut held, 2_030, &mut out);
        gate.read_at(2_040, FieldState::Clear, Moved::Same);
        for t in [2_045, 2_060, 2_075, 2_090, 2_105, 2_120, 2_135] {
            release(&gate, &mut held, t, &mut out);
        }
        gate.clicks_lost(2_143);
        gate.clicks_back(2_145);
        release(&gate, &mut held, 2_145, &mut out);
        gate.read_at(2_130, FieldState::Clear, Moved::Same);
        release(&gate, &mut held, 2_150, &mut out);
        gate.read_at(2_160, FieldState::Clear, Moved::Same);
        for t in [2_165, 2_180, 2_195] {
            release(&gate, &mut held, t, &mut out);
        }
        assert!(
            !out.iter().any(|(name, recorded)| *name == "K" && *recorded),
            "{out:?}"
        );
    }

    #[test]
    fn a_route_lost_inside_the_hold_and_not_back_withholds_the_key() {
        let gate = plain();
        let letter = key(&gate, 2_000);
        gate.clicks_lost(2_050);
        gate.read_at(2_120, FieldState::Clear, Moved::Same);
        assert_ne!(reading(&gate, &letter, 2_130), Some(Ok(())));
    }

    #[test]
    fn a_second_loss_of_the_route_keeps_the_first() {
        let gate = plain();
        gate.clicks_lost(1_990);
        let letter = key(&gate, 2_000);
        gate.clicks_lost(2_200);
        gate.read_at(2_120, FieldState::Clear, Moved::Same);
        assert_ne!(reading(&gate, &letter, 2_210), Some(Ok(())));
    }

    #[test]
    fn a_time_with_no_click_route_from_a_forwarded_tick_before_the_stop_meets_the_hold() {
        let gate = plain();
        let letter = key(&gate, 2_000);
        gate.clicks_lost(1_995);
        gate.clicks_back(2_060);
        gate.read_at(2_120, FieldState::Clear, Moved::Same);
        assert_eq!(rule(&gate, &letter, 2_130), Some(Rule::NoClickRoute));
    }

    #[test]
    fn a_key_waits_while_the_pointer_owns_the_clicks_and_has_not_forwarded_past_its_hold() {
        let gate = plain();
        gate.pointer_takes_the_clicks(1_000);
        let letter = key(&gate, 2_010);
        gate.read_at(2_135, FieldState::Clear, Moved::Same);
        gate.read_at(2_165, FieldState::Clear, Moved::Same);
        let before_the_click_came = reading(&gate, &letter, 2_170);
        gate.latch_after(2_000);
        assert_ne!(before_the_click_came, Some(Ok(())));
        let gate = plain();
        gate.pointer_takes_the_clicks(1_000);
        let letter = key(&gate, 2_000);
        gate.read_at(2_120, FieldState::Clear, Moved::Same);
        let answer = reading(&gate, &letter, 2_130);
        gate.latch_after(1_990);
        assert_ne!(answer, Some(Ok(())));
    }

    #[test]
    fn a_forwarded_tick_past_the_hold_clears_the_key() {
        let gate = plain();
        gate.pointer_takes_the_clicks(1_000);
        let letter = key(&gate, 2_000);
        gate.read_at(2_120, FieldState::Clear, Moved::Same);
        gate.pointer_forwarded(2_000 + HOLD + 15);
        assert_eq!(reading(&gate, &letter, 2_140), None);
        gate.pointer_forwarded(2_000 + HOLD + 16);
        assert_eq!(reading(&gate, &letter, 2_140), Some(Ok(())));
    }

    #[test]
    fn a_pointer_pump_that_forwards_nothing_withholds_the_key_at_250_ms() {
        let gate = plain();
        gate.pointer_takes_the_clicks(1_000);
        let letter = key(&gate, 2_000);
        gate.read_at(2_120, FieldState::Clear, Moved::Same);
        assert_eq!(reading(&gate, &letter, 2_200), None);
        assert_eq!(reading(&gate, &letter, 2_251), Some(Err(Withheld::Unknown)));
        assert_eq!(rule(&gate, &letter, 2_251), Some(Rule::ClicksNotForwarded));
    }

    #[test]
    fn an_own_read_writes_no_clear_over_a_password_event_that_came_after_it_began() {
        use FieldState::{Clear, Password};
        let gate = PasswordGate::new();
        let since = |gate: &PasswordGate| gate.shared.lock().next_seq;
        let clear = ContentReading::Clear(ClearBy::NativeEdit);
        let event = |gate: &PasswordGate, at: u32, state: FieldState, content: ContentReading| {
            let _ = gate.shared.begin_event();
            gate.shared.field_seen(at, state, (content, None), None);
        };
        let own_read = |gate: &PasswordGate, began: u32, since: u64| {
            gate.shared.read_seen(
                began,
                began + 5,
                (Clear, clear, None, since),
                Moved::Another,
                None,
            );
            gate.content_reading()
        };
        gate.started_at(1_000);
        let first_began = since(&gate);
        event(&gate, 1_010, Password, ContentReading::Password);
        let first = own_read(&gate, 1_005, first_began);
        event(&gate, 1_100, Clear, clear);
        let later_began = since(&gate);
        event(&gate, 1_110, Password, ContentReading::Password);
        let later = own_read(&gate, 1_105, later_began);
        let control_began = since(&gate);
        let control = own_read(&gate, 1_200, control_began);
        assert_eq!(
            first,
            ContentReading::Password,
            "the first own read, overtaken by a password event"
        );
        assert_eq!(
            later,
            ContentReading::Password,
            "a later own read, overtaken by a password event"
        );
        assert_eq!(
            control, clear,
            "an own read that began after the password event"
        );
    }

    #[test]
    fn the_focus_event_that_made_a_password_reading_is_not_the_move_that_lets_it_go() {
        use FieldState::{Clear, Password};
        use KeptElement::Other;
        use ReadBy::OwnRead;
        let gate = PasswordGate::new();
        let kept = gate.shared.begin_event();
        gate.shared
            .field_seen(2_000, Password, (ContentReading::Password, None), None);
        let read_move = gate.shared.lock().moves;
        assert!(!lets_go(kept, read_move, false));
        assert_eq!(
            with_kept(Some(kept), Other, Clear, OwnRead, read_move, false),
            (Password, Kept::Leave)
        );
        gate.latch(2_100);
        let after_a_latch = gate.shared.lock().moves;
        assert!(lets_go(kept, after_a_latch, false));
        assert!(!lets_go(kept, after_a_latch, true));
    }

    #[test]
    fn the_deciding_reads_own_state_joins_the_verdict() {
        let gate = plain();
        let letter = key(&gate, 2_000);
        gate.read_at(2_120, FieldState::Password, Moved::Same);
        assert_eq!(reading(&gate, &letter, 2_130), Some(Err(Withheld::Active)));
    }

    #[test]
    fn with_no_kept_element_a_read_keeps_its_own_state() {
        use FieldState::Unknown;
        use KeptElement::Other;
        use ReadBy::{Event, OwnRead};
        assert_eq!(
            with_kept(None, Other, Unknown, OwnRead, 4, false),
            (Unknown, Kept::Leave)
        );
        assert_eq!(
            with_kept(None, Other, Unknown, Event, 4, false),
            (Unknown, Kept::Leave)
        );
    }

    #[test]
    fn a_read_that_cannot_compare_closes_no_latch() {
        let gate = plain();
        gate.latch(2_000);
        gate.read_at(2_100, FieldState::Clear, Moved::Same);
        gate.read_at(2_130, FieldState::Clear, Moved::NotCompared);
        gate.read_at(2_160, FieldState::Clear, Moved::NotCompared);
        let letter = key(&gate, 2_200);
        gate.read_at(2_320, FieldState::Clear, Moved::Same);
        gate.read_at(2_350, FieldState::Clear, Moved::Same);
        assert_ne!(reading(&gate, &letter, 2_360), Some(Ok(())));
    }

    #[test]
    fn an_own_read_at_the_keys_own_tick_that_shows_a_move_withholds_the_key() {
        let gate = plain();
        gate.read_at(2_000, FieldState::Clear, Moved::Another);
        let letter = key(&gate, 2_000);
        gate.read_at(2_120, FieldState::Clear, Moved::Same);
        gate.read_at(2_150, FieldState::Clear, Moved::Same);
        assert_eq!(reading(&gate, &letter, 2_160), Some(Err(Withheld::Unknown)));
    }

    #[test]
    fn after_a_desktop_switch_a_clear_own_read_opens_nothing_without_a_move() {
        let gate = plain();
        gate.shared.desktop_switched(1_900, None);
        gate.read_at(2_000, FieldState::Clear, Moved::Same);
        let letter = key(&gate, 2_010);
        gate.read_at(2_130, FieldState::Clear, Moved::Same);
        gate.read_at(2_160, FieldState::Clear, Moved::Same);
        assert_ne!(reading(&gate, &letter, 2_170), Some(Ok(())));
    }
}
