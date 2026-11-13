//! The Windows probe of `secure_input`: the password-field detector of the
//! keyboard source. Windows sets no secure-input flag, so a detector on a
//! thread of its own reads the focused element through UI Automation, and a
//! gate holds each key until the detector has decided for the key's input
//! time. Only a positive answer that the field is no password field is
//! Clear; a password field is Password, and every other answer, an error, a
//! start, a desktop switch and a Firefox focus are Unknown. A key gets its
//! clearance only through [`super::check`] with the gate's reading.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use super::{check, seal, KeystrokeClearance, SecureInputProbe, SecureInputReading, Withheld};

/// The state of the focused element as the detector reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldState {
    /// The element answered that it is no password field.
    Clear,
    /// The element is a password field, or was one while it kept the focus.
    Password,
    /// No answer, or one that cannot be trusted.
    Unknown,
}

/// What one read of the focused element gave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FocusRead {
    /// The provider answered the IsPassword property.
    Answered {
        is_password: bool,
        protected: bool,
        firefox: bool,
    },
    /// The provider gave the property's default, so it did not answer.
    NoAnswer { protected: bool },
    /// No element could be read.
    Unreadable,
}

/// The state of one read; `same_password_element` says that the element is
/// the one last read as Password, which stays Password while it has focus.
pub(crate) fn state_of(read: FocusRead, same_password_element: bool) -> FieldState {
    if same_password_element {
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

/// How long the gate holds each key: a focus change that the detector learns
/// of within this time after the key's input time may have come before the
/// key. A first value above the largest delay that spike S3 measured from a
/// key to its focus event, 75.3 ms.
pub const KEY_HOLD: Duration = Duration::from_millis(120);

/// How long after a key or a click that moves the focus the detector reads
/// the focused element again when no focus event has come.
pub const LATCH_SETTLE: Duration = Duration::from_millis(120);

/// The most marks that the gate keeps.
const MARKS_KEPT: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mark {
    Field(FieldState),
    Latch,
}

#[derive(Debug, Clone, Copy)]
struct Entry {
    seq: u64,
    mark: Mark,
}

/// A key that the gate holds: its input time, the marks that the gate had
/// when the key arrived and whether the focus window was a password edit.
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

/// The gate's answer for one held key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Judgment {
    /// The key waits for a later mark or for its hold to pass.
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

/// The reading for `key` at tick `now` from the gate's marks, in the order
/// the gate took them. The state before the key is the last mark taken before
/// it arrived, or after a latch the first field mark that follows the latch.
/// A field mark taken after the key and before the next latch may describe a
/// focus change that came before the key, so the key is Clear only when all
/// of them are Clear, and only after its hold has passed.
fn judge(marks: &VecDeque<Entry>, key: &HeldKey, now: u32) -> Judgment {
    if key.password_edit {
        return Judgment::Decided(SecureInputReading::Active);
    }
    let before = marks.iter().rev().find(|entry| entry.seq < key.seen);
    let mut state = match before.map(|entry| (entry.seq, entry.mark)) {
        None => FieldState::Unknown,
        Some((_, Mark::Field(state))) => state,
        Some((latch, Mark::Latch)) => {
            let decided = marks.iter().find_map(|entry| match entry.mark {
                Mark::Field(state) if entry.seq > latch => Some(state),
                _ => None,
            });
            match decided {
                Some(state) => state,
                None => return Judgment::Wait,
            }
        }
    };
    for entry in marks.iter().filter(|entry| entry.seq >= key.seen) {
        match entry.mark {
            Mark::Latch => break,
            Mark::Field(later) => state = most_restrictive(state, later),
        }
    }
    match state {
        FieldState::Password => Judgment::Decided(SecureInputReading::Active),
        FieldState::Unknown => Judgment::Decided(SecureInputReading::Unknown),
        FieldState::Clear if now.wrapping_sub(key.tick) < ms(KEY_HOLD) => Judgment::Wait,
        FieldState::Clear => Judgment::Decided(SecureInputReading::Unknown),
    }
}

/// The probe of the Windows keyboard source: the gate's reading for one key.
/// Only this module makes one with a reading other than Unknown.
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
        if self.entries.len() == MARKS_KEPT {
            self.entries.pop_front();
        }
        self.entries.push_back(Entry {
            seq: self.next_seq,
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
        let mut marks = self.lock();
        marks.push(Mark::Field(state));
        marks.settle_due = None;
    }

    fn latch(&self, tick: u32) {
        let mut marks = self.lock();
        marks.push(Mark::Latch);
        marks.settle_due = Some(tick.wrapping_add(ms(LATCH_SETTLE)));
    }
}

/// The gate of the Windows keyboard source and its password-field detector,
/// which runs on a thread of its own only while the source runs.
#[derive(Debug, Default)]
pub struct PasswordGate {
    shared: Arc<Shared>,
    #[cfg(target_os = "windows")]
    detector: Mutex<Option<detector::Running>>,
}

impl PasswordGate {
    /// A gate whose detector is not attached; every key reads Unknown.
    pub fn new() -> PasswordGate {
        PasswordGate::default()
    }

    /// The key of `tick` held from now; `password_edit` says that the focus
    /// window was an edit control with the password style when it arrived.
    pub(crate) fn hold(&self, tick: u32, password_edit: bool) -> HeldKey {
        HeldKey {
            tick,
            seen: self.shared.lock().next_seq,
            password_edit,
        }
    }

    /// Closes the gate at `tick` until the next field mark: a key or a click
    /// that moves the focus before the focus event arrives.
    pub(crate) fn latch(&self, tick: u32) {
        self.shared.latch(tick);
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

    /// The clearance for `key` at tick `now`, why there is none, or `None`
    /// while the key waits. A key that has waited past the late bound gets
    /// the answer for a late key.
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

    /// The state that the gate's last field mark holds, Unknown before the
    /// first read.
    pub fn field_now(&self) -> FieldState {
        self.shared
            .lock()
            .entries
            .iter()
            .rev()
            .find_map(|entry| match entry.mark {
                Mark::Field(state) => Some(state),
                Mark::Latch => None,
            })
            .unwrap_or(FieldState::Unknown)
    }

    #[cfg(test)]
    fn field(&self, state: FieldState) {
        self.shared.field(state);
    }
}

/// A key code whose key-down moves the focus: Tab, Enter, Escape and the two
/// Windows keys.
pub(crate) fn moves_focus(vkey: u16) -> bool {
    matches!(vkey, 0x09 | 0x0D | 0x1B | 0x5B | 0x5C)
}

#[cfg(target_os = "windows")]
pub(crate) use detector::focus_window_is_password_edit;

#[cfg(target_os = "windows")]
mod detector {
    use std::sync::Arc;

    use core::ffi::c_void;
    use std::sync::atomic::{AtomicU32, Ordering};

    use windows::core::{Interface, GUID, HRESULT};
    use windows::Win32::Foundation::{
        E_NOINTERFACE, E_POINTER, HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, S_OK, WPARAM,
    };
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
        CallNextHookEx, DispatchMessageW, GetClassNameW, GetForegroundWindow, GetGUIThreadInfo,
        GetWindowLongW, GetWindowThreadProcessId, MsgWaitForMultipleObjectsEx, PeekMessageW,
        SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, ES_PASSWORD,
        EVENT_SYSTEM_DESKTOPSWITCH, GUITHREADINFO, GWL_STYLE, HHOOK, MSG, MSLLHOOKSTRUCT,
        MWMO_INPUTAVAILABLE, PM_REMOVE, QS_ALLINPUT, STATE_SYSTEM_PROTECTED, WH_MOUSE_LL,
        WINEVENT_OUTOFCONTEXT, WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_RBUTTONDOWN, WM_XBUTTONDOWN,
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
        /// Starts the detector: the gate reads Unknown until its first read of
        /// the focused element. Fails with the step that the system refused.
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
            let join = std::thread::Builder::new()
                .name("halvo-password-detector".to_string())
                .spawn(move || run(&shared, &thread_wake, ready))
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

        /// Stops the detector and answers the first of its removal calls that
        /// failed, or `Timeout` when its thread does not end within `deadline`.
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
                    .GetCachedPropertyValueEx(UIA_IsPasswordPropertyId, true)
                    .ok()
                    .and_then(|mut value| variant_bool(&mut value))
            };
            // SAFETY: as above.
            let protected = unsafe {
                element
                    .GetCachedPropertyValueEx(UIA_LegacyIAccessibleStatePropertyId, true)
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
                return super::state_of(FocusRead::Unreadable, false);
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
        static LATCHED: std::cell::RefCell<Option<Arc<Shared>>> =
            const { std::cell::RefCell::new(None) };
    }

    unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 {
            let message = u32::try_from(wparam.0).unwrap_or(0);
            if matches!(
                message,
                WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN
            ) {
                // SAFETY: for WH_MOUSE_LL with a code of 0 or more the system
                // passes a live MSLLHOOKSTRUCT in `lparam`.
                let time = unsafe { (*(lparam.0 as *const MSLLHOOKSTRUCT)).time };
                LATCHED.with(|cell| {
                    if let Some(shared) = cell.borrow().as_ref() {
                        shared.latch(time);
                    }
                });
            }
        }
        // SAFETY: the arguments are the ones that the system passed.
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
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
        LATCHED.with(|cell| {
            if let Some(shared) = cell.borrow().as_ref() {
                shared.field(FieldState::Unknown);
                shared.latch(time);
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

    fn module() -> Option<HINSTANCE> {
        use windows::Win32::System::LibraryLoader::GetModuleHandleW;
        // SAFETY: GetModuleHandleW with no name reads this process's module.
        unsafe { GetModuleHandleW(None) }.ok().map(Into::into)
    }

    fn run(
        shared: &Arc<Shared>,
        wake: &Event,
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
        LATCHED.with(|cell| *cell.borrow_mut() = Some(Arc::clone(shared)));
        // SAFETY: the hook procedure has the HOOKPROC signature; this thread
        // pumps its messages below, which a low-level hook needs.
        let mouse: Option<HHOOK> =
            unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), module(), 0) }.ok();
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
        shared.field(reader.focused());
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
                    marks.push(super::Mark::Field(state));
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
        LATCHED.with(|cell| *cell.borrow_mut() = None);
        let mut removed = Ok(());
        // SAFETY: the handler is the one that `start` added on this thread.
        if let Err(error) = unsafe { reader.automation.RemoveFocusChangedEventHandler(&handler) } {
            removed = removed.and(Err(os("RemoveFocusChangedEventHandler", &error)));
        }
        if let Some(mouse) = mouse {
            // SAFETY: `mouse` is the hook that this thread installed above.
            if let Err(error) = unsafe { UnhookWindowsHookEx(mouse) } {
                removed = removed.and(Err(os("UnhookWindowsHookEx", &error)));
            }
        }
        if !desktop.is_invalid() {
            // SAFETY: `desktop` is the hook that this thread installed above.
            if let Err(error) = unsafe { UnhookWinEvent(desktop) }.ok() {
                removed = removed.and(Err(os("UnhookWinEvent", &error)));
            }
        }
        drop(handler);
        drop(reader);
        removed
    }

    /// Whether the window that has the keyboard focus in the foreground
    /// thread is an edit control with the password style.
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
        let other = gate.hold(3_000, false);
        gate.field(FieldState::Unknown);
        assert_eq!(reading(&gate, &other, 3_200), Some(Err(Withheld::Unknown)));
    }

    #[test]
    fn a_key_after_a_latch_waits_for_the_next_field_mark() {
        let gate = PasswordGate::new();
        gate.field(FieldState::Clear);
        let tab = gate.hold(2_000, false);
        gate.latch(2_000);
        let typed = gate.hold(2_010, false);
        assert_eq!(reading(&gate, &typed, 2_200), None);
        gate.field(FieldState::Password);
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

    #[test]
    fn keys_tab_enter_escape_and_the_windows_keys_move_the_focus() {
        for vkey in [0x09, 0x0D, 0x1B, 0x5B, 0x5C] {
            assert!(moves_focus(vkey), "{vkey:#x}");
        }
        for vkey in [0x41, 0x20, 0x10, 0x11, 0x12, 0x25, 0x7E] {
            assert!(!moves_focus(vkey), "{vkey:#x}");
        }
    }
}
