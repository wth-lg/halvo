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
pub(crate) enum FocusRead {
    Answered {
        is_password: bool,
        protected: bool,
        firefox: bool,
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
        FocusRead::Answered { firefox: true, .. } => FieldState::Unknown,
        FocusRead::Answered {
            is_password: false, ..
        } => FieldState::Clear,
        FocusRead::NoAnswer { protected: false } | FocusRead::Unreadable => FieldState::Unknown,
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

pub const LATCH_SETTLE: Duration = Duration::from_millis(100);

const MARKS_KEPT: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mark {
    Field(FieldState),
    Read(FieldState),
    Latch(LatchFrom),
}

#[derive(Debug, Clone, Copy)]
struct Entry {
    seq: u64,
    tick: u32,
    mark: Mark,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct HeldKey {
    tick: u32,
    seen: u64,
    password_edit: bool,
}

impl HeldKey {
    pub(crate) fn tick(&self) -> u32 {
        self.tick
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Judgment {
    Wait,
    Decided(SecureInputReading),
}

fn ms(duration: Duration) -> u32 {
    u32::try_from(duration.as_millis()).unwrap_or(u32::MAX)
}

fn most_restrictive(a: FieldState, b: FieldState) -> FieldState {
    match (a, b) {
        (FieldState::Password, _) | (_, FieldState::Password) => FieldState::Password,
        (FieldState::Unknown, _) | (_, FieldState::Unknown) => FieldState::Unknown,
        _ => FieldState::Clear,
    }
}

fn judge(marks: &VecDeque<Entry>, key: &HeldKey, now: u32) -> Judgment {
    if key.password_edit {
        return Judgment::Decided(SecureInputReading::Active);
    }
    let not_after_key = |tick: u32| key.tick.wrapping_sub(tick) <= i32::MAX as u32;
    let opens = |entry: &&Entry| {
        entry.seq < key.seen
            || entry.mark == Mark::Latch(LatchFrom::AnotherThread) && not_after_key(entry.tick)
    };
    let opening = marks.iter().filter(opens).max_by_key(|entry| entry.seq);
    let (mut state, from) = match opening.map(|entry| (entry.seq, entry.mark)) {
        None => (FieldState::Unknown, key.seen),
        Some((_, Mark::Field(state) | Mark::Read(state))) => (state, key.seen),
        Some((latch, Mark::Latch(_))) => {
            let mut seen: Option<FieldState> = None;
            let mut decided = None;
            for entry in marks.iter().filter(|entry| entry.seq > latch) {
                match entry.mark {
                    Mark::Latch(_) => {
                        return Judgment::Decided(SecureInputReading::Unknown);
                    }
                    Mark::Field(state) => {
                        seen = Some(seen.map_or(state, |held| most_restrictive(held, state)));
                    }
                    Mark::Read(state) => {
                        let held = seen.map_or(state, |held| most_restrictive(held, state));
                        decided = Some((held, key.seen.max(entry.seq + 1)));
                        break;
                    }
                }
            }
            match decided {
                Some(decided) => decided,
                None => return Judgment::Wait,
            }
        }
    };
    for entry in marks.iter().filter(|entry| entry.seq >= from) {
        match entry.mark {
            Mark::Latch(LatchFrom::TheInputThread) => break,
            Mark::Latch(LatchFrom::AnotherThread) if !not_after_key(entry.tick) => break,
            Mark::Latch(LatchFrom::AnotherThread) => {}
            Mark::Field(later) | Mark::Read(later) => state = most_restrictive(state, later),
        }
    }
    match state {
        FieldState::Password => Judgment::Decided(SecureInputReading::Active),
        FieldState::Unknown => Judgment::Decided(SecureInputReading::Unknown),
        FieldState::Clear if now.wrapping_sub(key.tick) < ms(KEY_HOLD) => Judgment::Wait,
        FieldState::Clear => Judgment::Decided(SecureInputReading::Inactive),
    }
}

/// The probe of the Windows keyboard source: the gate's reading for one key.
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
    stopping: bool,
}

impl Marks {
    fn push(&mut self, mark: Mark) {
        self.push_at(0, mark);
    }

    fn push_at(&mut self, tick: u32, mark: Mark) {
        if self.entries.len() == MARKS_KEPT {
            self.entries.pop_front();
        }
        self.entries.push_back(Entry {
            seq: self.next_seq,
            tick,
            mark,
        });
        self.next_seq += 1;
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

    fn field(&self, state: FieldState) {
        self.lock().push(Mark::Field(state));
    }

    fn read(&self, state: FieldState) {
        let mut marks = self.lock();
        marks.push(Mark::Read(state));
        marks.settle_due = None;
    }

    fn latch(&self, tick: u32, from: LatchFrom) {
        let mut marks = self.lock();
        marks.push_at(tick, Mark::Latch(from));
        marks.settle_due = Some(tick.wrapping_add(ms(LATCH_SETTLE)));
    }

    fn desktop_switched(&self, tick: u32) {
        self.field(FieldState::Unknown);
        self.latch(tick, LatchFrom::AnotherThread);
    }
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
    removals: detector::Removals,
}

impl PasswordGate {
    /// A gate whose detector is not attached.
    pub fn new() -> PasswordGate {
        PasswordGate::default()
    }

    pub(crate) fn hold(&self, tick: u32, password_edit: bool) -> HeldKey {
        HeldKey {
            tick,
            seen: self.shared.lock().next_seq,
            password_edit,
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

    pub(crate) fn clearance(
        &self,
        key: &HeldKey,
        now: u32,
    ) -> Option<Result<KeystrokeClearance, Withheld>> {
        let age = Duration::from_millis(u64::from(now.wrapping_sub(key.tick)));
        let reading = match judge(&self.shared.lock().entries, key, now) {
            Judgment::Decided(reading) => reading,
            Judgment::Wait if age > super::LATE_KEY_MAX => SecureInputReading::Unknown,
            Judgment::Wait => return None,
        };
        Some(check(&PasswordFieldProbe(reading), Some(age)))
    }

    /// The state that the gate's last field mark holds.
    pub fn field_now(&self) -> FieldState {
        self.shared
            .lock()
            .entries
            .iter()
            .rev()
            .find_map(|entry| match entry.mark {
                Mark::Field(state) | Mark::Read(state) => Some(state),
                Mark::Latch(_) => None,
            })
            .unwrap_or(FieldState::Unknown)
    }

    #[cfg(test)]
    pub(crate) fn field(&self, state: FieldState) {
        self.shared.field(state);
    }

    #[cfg(test)]
    pub(crate) fn read(&self, state: FieldState) {
        self.shared.read(state);
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
        SetWinEventHook, UIA_IsPasswordPropertyId, UIA_LegacyIAccessiblePatternId,
        UIA_LegacyIAccessibleStatePropertyId, UIA_ProcessIdPropertyId, UnhookWinEvent,
        HWINEVENTHOOK,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetClassNameW, GetForegroundWindow, GetGUIThreadInfo, GetWindowLongW,
        GetWindowThreadProcessId, MsgWaitForMultipleObjectsEx, PeekMessageW, TranslateMessage,
        ES_PASSWORD, EVENT_SYSTEM_DESKTOPSWITCH, GUITHREADINFO, GWL_STYLE, MSG,
        MWMO_INPUTAVAILABLE, PM_REMOVE, QS_ALLINPUT, STATE_SYSTEM_PROTECTED, WINEVENT_OUTOFCONTEXT,
    };

    use super::{FieldState, FocusRead, PasswordGate, Shared};
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
            {
                let mut marks = self.shared.lock();
                *marks = super::Marks::default();
                marks.next_seq = 0;
            }
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
            marks.push(super::Mark::Field(FieldState::Unknown));
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
        last_password: std::sync::Mutex<Option<IUIAutomationElement>>,
    }

    // SAFETY: the objects of the system's UI Automation client are free
    // threaded: any thread of the process's MTA may call them, and the
    // element that the reader keeps is guarded by its mutex.
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

    fn is_firefox(pid: i32) -> bool {
        u32::try_from(pid)
            .ok()
            .and_then(crate::windows::foreground::process_path)
            .is_some_and(|path| {
                path.rsplit(['/', '\\'])
                    .next()
                    .is_some_and(|name| name.eq_ignore_ascii_case("firefox.exe"))
            })
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
            let firefox = unsafe { element.CachedProcessId() }.is_ok_and(is_firefox);
            match is_password {
                Some(is_password) => FocusRead::Answered {
                    is_password,
                    protected,
                    firefox,
                },
                None => FocusRead::NoAnswer { protected },
            }
        }

        fn state(&self, element: Option<&IUIAutomationElement>) -> FieldState {
            let Some(element) = element else {
                return super::unreadable_state();
            };
            let mut last = self
                .last_password
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let same = last.as_ref().is_some_and(|previous| {
                // SAFETY: both elements are live for the call.
                unsafe { self.automation.CompareElements(previous, element) }
                    .is_ok_and(|same| same.as_bool())
            });
            let state = super::state_of(self.read(element), same);
            *last = (state == FieldState::Password).then(|| element.clone());
            state
        }

        fn focused(&self) -> FieldState {
            // SAFETY: the cache request is live for the call.
            let element = unsafe { self.automation.GetFocusedElementBuildCache(&self.cache) };
            self.state(element.ok().as_ref())
        }
    }

    #[repr(C)]
    struct FocusHandler {
        vtable: *const IUIAutomationFocusChangedEventHandler_Vtbl,
        refs: AtomicU32,
        shared: Arc<Shared>,
        reader: Arc<Reader>,
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
        let state = handler.reader.state(element);
        handler.shared.field(state);
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

    fn start(
        shared: &Arc<Shared>,
    ) -> Result<(Arc<Reader>, IUIAutomationFocusChangedEventHandler), CaptureError> {
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
        let reader = Arc::new(Reader {
            automation,
            cache,
            last_password: std::sync::Mutex::new(None),
        });
        let handler = FocusHandler {
            vtable: &HANDLER_VTABLE,
            refs: AtomicU32::new(1),
            shared: Arc::clone(shared),
            reader: Arc::clone(&reader),
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

    fn run(
        shared: &Arc<Shared>,
        wake: &Event,
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
        let (reader, handler) = match start(shared) {
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
        shared.read(reader.focused());
        let _ = ready.send(Ok(()));
        let mut msg = MSG::default();
        loop {
            let (stopping, due) = {
                let marks = shared.lock();
                (marks.stopping, marks.settle_due)
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
                let state = reader.focused();
                let mut marks = shared.lock();
                if marks.settle_due == due {
                    marks.push(super::Mark::Read(state));
                    marks.settle_due = None;
                }
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
        use super::super::{HeldKey, Judgment, Mark};
        use super::*;
        use crate::secure_input::SecureInputReading;
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
        fn after_attach_the_gate_reads_unknown_until_the_detectors_first_read() {
            let gate = PasswordGate::new();
            let before = HeldKey {
                tick: 1,
                seen: 0,
                password_edit: false,
            };
            gate.attach().expect("the detector starts");
            let first = gate.shared.lock().entries.front().map(|entry| entry.mark);
            let judged = super::super::judge(&gate.shared.lock().entries, &before, 1_000);
            let detached = gate.detach(crate::windows::INPUT_STOP_DEADLINE);
            assert!(matches!(first, Some(Mark::Read(_))), "{first:?}");
            assert_eq!(judged, Judgment::Decided(SecureInputReading::Unknown));
            assert_eq!(detached, Ok(()));
        }

        #[test]
        fn detach_ends_the_detectors_thread_and_a_new_attach_starts_one() {
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
            let both = PasswordGate::with_removals(Removals {
                handler: handler_refused,
                desktop_hook: desktop_refused,
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

        #[test]
        fn detach_answers_timeout_when_the_detector_does_not_end_in_time() {
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

    #[test]
    fn only_an_answer_that_the_field_is_no_password_field_reads_clear() {
        let answered = |is_password, protected, firefox| FocusRead::Answered {
            is_password,
            protected,
            firefox,
        };
        assert_eq!(
            state_of(answered(true, false, false), false),
            FieldState::Password
        );
        assert_eq!(
            state_of(answered(false, false, false), false),
            FieldState::Clear
        );
        assert_eq!(
            state_of(answered(false, true, false), false),
            FieldState::Password
        );
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
            state_of(answered(false, false, true), false),
            FieldState::Unknown
        );
        assert_eq!(
            state_of(answered(true, false, true), false),
            FieldState::Password
        );
        assert_eq!(
            state_of(answered(false, false, false), true),
            FieldState::Password
        );
    }

    fn reading(gate: &PasswordGate, key: &HeldKey, now: u32) -> Option<Result<(), Withheld>> {
        gate.clearance(key, now).map(|answer| answer.map(|_| ()))
    }

    const HOLD: u32 = 120;

    #[test]
    fn a_key_in_a_clear_field_waits_its_hold_and_then_gets_a_clearance() {
        let gate = PasswordGate::new();
        gate.field(FieldState::Clear);
        let key = gate.hold(2_000, false);
        assert_eq!(reading(&gate, &key, 2_000), None);
        assert_eq!(reading(&gate, &key, 2_000 + HOLD - 1), None);
        assert_eq!(reading(&gate, &key, 2_000 + HOLD), Some(Ok(())));
    }

    #[test]
    fn a_key_with_no_reading_yet_is_unknown_and_a_password_field_withholds_at_once() {
        let gate = PasswordGate::new();
        let first = gate.hold(10, false);
        assert_eq!(reading(&gate, &first, 10), Some(Err(Withheld::Unknown)));
        gate.field(FieldState::Password);
        let key = gate.hold(30, false);
        assert_eq!(reading(&gate, &key, 30), Some(Err(Withheld::Active)));
    }

    #[test]
    fn a_focus_event_that_arrives_after_the_key_within_its_hold_decides_against_it() {
        let gate = PasswordGate::new();
        gate.field(FieldState::Clear);
        let key = gate.hold(2_000, false);
        gate.field(FieldState::Password);
        assert_eq!(reading(&gate, &key, 2_200), Some(Err(Withheld::Active)));
        gate.field(FieldState::Clear);
        let other = gate.hold(3_000, false);
        gate.field(FieldState::Unknown);
        assert_eq!(reading(&gate, &other, 3_200), Some(Err(Withheld::Unknown)));
    }

    #[test]
    fn a_key_after_a_latch_waits_for_the_detectors_own_read() {
        let gate = PasswordGate::new();
        gate.field(FieldState::Clear);
        let tab = gate.hold(2_000, false);
        gate.latch(2_000);
        let typed = gate.hold(2_010, false);
        assert_eq!(reading(&gate, &typed, 2_200), None);
        gate.field(FieldState::Password);
        assert_eq!(reading(&gate, &typed, 2_200), None);
        gate.read(FieldState::Password);
        assert_eq!(reading(&gate, &tab, 2_200), Some(Ok(())));
        assert_eq!(reading(&gate, &typed, 2_200), Some(Err(Withheld::Active)));
    }

    #[test]
    fn a_key_that_waits_past_the_late_bound_is_late() {
        let gate = PasswordGate::new();
        gate.field(FieldState::Clear);
        gate.latch(1_500);
        let key = gate.hold(1_510, false);
        assert_eq!(reading(&gate, &key, 1_700), None);
        assert_eq!(reading(&gate, &key, 1_761), Some(Err(Withheld::Late)));
    }

    #[test]
    fn a_password_edit_under_the_focus_withholds_the_key_whatever_the_detector_read() {
        let gate = PasswordGate::new();
        gate.field(FieldState::Clear);
        let key = gate.hold(2_000, true);
        assert_eq!(reading(&gate, &key, 2_200), Some(Err(Withheld::Active)));
    }

    #[test]
    fn the_hold_is_read_across_the_wrap_of_the_tick_count() {
        let gate = PasswordGate::new();
        gate.field(FieldState::Clear);
        let key = gate.hold(u32::MAX - 50, false);
        assert_eq!(reading(&gate, &key, 30), None);
        assert_eq!(reading(&gate, &key, 80), Some(Ok(())));
    }

    fn cleared(gate: &PasswordGate, key: &HeldKey, now: u32) -> bool {
        reading(gate, key, now) == Some(Ok(()))
    }

    #[test]
    fn a_click_that_reaches_the_gate_after_a_later_key_still_closes_it() {
        let gate = PasswordGate::new();
        gate.field(FieldState::Clear);
        let key = gate.hold(1_000, false);
        gate.latch_after(990);
        assert_eq!(reading(&gate, &key, 1_200), None);
        gate.read(FieldState::Password);
        assert_eq!(reading(&gate, &key, 1_200), Some(Err(Withheld::Active)));
    }

    #[test]
    fn a_focus_event_after_a_latch_does_not_decide_before_the_detector_read_again() {
        let gate = PasswordGate::new();
        gate.field(FieldState::Clear);
        gate.latch(1_000);
        let key = gate.hold(1_010, false);
        gate.field(FieldState::Clear);
        assert_eq!(reading(&gate, &key, 1_200), None);
        gate.read(FieldState::Password);
        assert_eq!(reading(&gate, &key, 1_200), Some(Err(Withheld::Active)));
        gate.latch(1_300);
        let next = gate.hold(1_310, false);
        gate.field(FieldState::Password);
        gate.read(FieldState::Clear);
        assert_eq!(reading(&gate, &next, 1_500), Some(Err(Withheld::Active)));
        gate.latch(1_600);
        let clear = gate.hold(1_610, false);
        gate.read(FieldState::Clear);
        assert_eq!(reading(&gate, &clear, 1_800), Some(Ok(())));
    }

    #[test]
    fn a_click_before_the_key_across_the_wrap_of_the_tick_count_closes_the_gate() {
        let gate = PasswordGate::new();
        gate.field(FieldState::Clear);
        let key = gate.hold(5, false);
        gate.latch_after(u32::MAX - 5);
        gate.field(FieldState::Password);
        assert!(!cleared(&gate, &key, 200));
    }

    #[test]
    fn a_click_after_the_key_leaves_the_key_to_the_read_before_it() {
        let gate = PasswordGate::new();
        gate.field(FieldState::Clear);
        let key = gate.hold(1_000, false);
        gate.latch_after(1_030);
        gate.field(FieldState::Password);
        assert!(cleared(&gate, &key, 1_200));
    }

    #[test]
    fn a_second_focus_move_before_the_first_read_leaves_the_key_unknown() {
        let gate = PasswordGate::new();
        gate.field(FieldState::Clear);
        gate.latch(1_000);
        let key = gate.hold(1_010, false);
        gate.latch(1_020);
        gate.field(FieldState::Clear);
        assert_eq!(reading(&gate, &key, 1_200), Some(Err(Withheld::Unknown)));
    }

    #[test]
    fn a_desktop_switch_reads_unknown_and_closes_the_gate() {
        let gate = PasswordGate::new();
        gate.read(FieldState::Clear);
        let before = gate.hold(1_000, false);
        gate.shared.desktop_switched(1_010);
        let after = gate.hold(1_020, false);
        assert_eq!(reading(&gate, &before, 1_200), Some(Err(Withheld::Unknown)));
        assert_eq!(reading(&gate, &after, 1_200), None);
        assert_eq!(gate.field_now(), FieldState::Unknown);
        gate.read(FieldState::Clear);
        assert_eq!(reading(&gate, &after, 1_200), Some(Ok(())));
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
