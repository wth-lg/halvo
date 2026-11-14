//! Keystroke and pointer capture via Raw Input (`WM_INPUT`).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use crate::floor::payload::KeyClass;
use crate::source::{
    CaptureError, CaptureSource, SourceContext, SourceCounters, SourceHealth, Started,
};

use super::{Lifecycle, Observed, Packet, Producer, PumpReady};
use crate::secure_input::windows::{HeldKey, PasswordGate};
use crate::secure_input::KeystrokeClearance;

/// Which device a source listens to; also its modality.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawDevice {
    Keyboard,
    Pointer,
}

impl RawDevice {
    fn hid_usage(self) -> u16 {
        use windows::Win32::Devices::HumanInterfaceDevice::{
            HID_USAGE_GENERIC_KEYBOARD, HID_USAGE_GENERIC_MOUSE,
        };
        match self {
            RawDevice::Keyboard => HID_USAGE_GENERIC_KEYBOARD,
            RawDevice::Pointer => HID_USAGE_GENERIC_MOUSE,
        }
    }
}

/// A decoded keyboard packet: the virtual-key code and whether this is a key-up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyStroke {
    pub vkey: u16,
    pub released: bool,
}

impl KeyStroke {
    /// The measured size of this key's decoded payload: the virtual-key code.
    pub const fn payload_len(self) -> u64 {
        core::mem::size_of::<u16>() as u64
    }
}

/// A decoded mouse packet: relative movement, the button-transition flags and the
/// wheel delta, straight from the raw record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PointerSample {
    pub last_x: i32,
    pub last_y: i32,
    pub button_flags: u16,
    pub wheel_delta: i16,
}

impl PointerSample {
    /// The measured size of this sample's decoded payload: the two movement
    /// deltas, the button flags and the wheel delta.
    pub const fn payload_len(self) -> u64 {
        (core::mem::size_of::<i32>() * 2
            + core::mem::size_of::<u16>()
            + core::mem::size_of::<i16>()) as u64
    }
}

impl PointerSample {
    /// A sample that only moves the pointer: no button, wheel or other flag.
    pub const fn is_motion_only(self) -> bool {
        self.button_flags == 0
    }
}

#[derive(Debug)]
pub(crate) struct KeyState {
    down: [bool; 256],
}

impl Default for KeyState {
    fn default() -> Self {
        Self { down: [false; 256] }
    }
}

pub(crate) const NO_KEY: u16 = 0xFF;

const CHORD_KEYS: [u16; 8] = [
    VK_CONTROL.0,
    VK_LCONTROL.0,
    VK_RCONTROL.0,
    VK_MENU.0,
    VK_LMENU.0,
    VK_RMENU.0,
    VK_LWIN.0,
    VK_RWIN.0,
];

const FOCUS_KEYS: [u16; 5] = [VK_TAB.0, VK_RETURN.0, VK_ESCAPE.0, VK_LWIN.0, VK_RWIN.0];

pub(crate) fn moves_focus(vkey: u16) -> bool {
    FOCUS_KEYS.contains(&vkey)
}

const BUTTON_DOWN_FLAGS: u16 = (RI_MOUSE_LEFT_BUTTON_DOWN
    | RI_MOUSE_RIGHT_BUTTON_DOWN
    | RI_MOUSE_MIDDLE_BUTTON_DOWN
    | RI_MOUSE_BUTTON_4_DOWN
    | RI_MOUSE_BUTTON_5_DOWN) as u16;

pub(crate) fn button_went_down(button_flags: u16) -> bool {
    button_flags & BUTTON_DOWN_FLAGS != 0
}

pub(crate) fn button_flags_of(bytes: &[u8]) -> Option<u16> {
    let header = core::mem::size_of::<RAWINPUTHEADER>();
    let kind = u32::from_ne_bytes(bytes.get(..4)?.try_into().ok()?);
    if kind != RIM_TYPEMOUSE.0 || bytes.len() < header + core::mem::size_of::<RAWMOUSE>() {
        return None;
    }
    let at = header + core::mem::offset_of!(RAWMOUSE, Anonymous);
    Some(u16::from_ne_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

struct ClickRoute {
    gate: Option<Arc<PasswordGate>>,
    keyboard_window: Option<usize>,
    keyboard_register: Option<RegisterCall>,
    owner: Option<RawDevice>,
}

static CLICKS: Mutex<ClickRoute> = Mutex::new(ClickRoute {
    gate: None,
    keyboard_window: None,
    keyboard_register: None,
    owner: None,
});

fn clicks() -> std::sync::MutexGuard<'static, ClickRoute> {
    CLICKS.lock().unwrap_or_else(PoisonError::into_inner)
}

const WM_TAKE_MOUSE: u32 = WM_APP + 1;

/// A test's message that holds the pump in its dispatch for the
/// milliseconds that it carries (W3-7 O4).
#[cfg(test)]
const WM_HOLD_FOR_TEST: u32 = WM_APP + 2;

fn mouse_registration(hwnd: HWND) -> RAWINPUTDEVICE {
    RAWINPUTDEVICE {
        usUsagePage: HID_USAGE_PAGE_GENERIC,
        usUsage: RawDevice::Pointer.hid_usage(),
        dwFlags: RIDEV_INPUTSINK,
        hwndTarget: hwnd,
    }
}

/// The keyboard's window registers the mouse when no source has it, and
/// ends the time with no click route that the pointer's removal began. While
/// the registration fails that time stays open, so the gate withholds every
/// key (W3-7 L7), and the gate's poll tries again until the call stands.
fn keyboard_takes_mouse(hwnd: HWND, register: RegisterCall) -> windows::core::Result<()> {
    let mut route = clicks();
    if route.owner.is_some() {
        return Ok(());
    }
    register(&mouse_registration(hwnd))?;
    route.owner = Some(RawDevice::Keyboard);
    if let Some(gate) = &route.gate {
        gate.clicks_back(super::now_tick());
    }
    Ok(())
}

/// The keyboard's window takes the mouse again when no source has it, with
/// the register call of its start.
fn keyboard_takes_mouse_again() {
    let (window, register) = {
        let route = clicks();
        if route.owner.is_some() {
            return;
        }
        (route.keyboard_window, route.keyboard_register)
    };
    if let (Some(window), Some(register)) = (window, register) {
        let _ = keyboard_takes_mouse(HWND(window as *mut core::ffi::c_void), register);
    }
}

fn keyboard_lets_mouse_go(remove: RemoveCall) -> windows::core::Result<()> {
    let mut route = clicks();
    route.gate = None;
    route.keyboard_window = None;
    route.keyboard_register = None;
    if route.owner != Some(RawDevice::Keyboard) {
        return Ok(());
    }
    route.owner = None;
    remove(&removal_of(RawDevice::Pointer))
}

/// The pointer source gives the mouse up, with the route's lock held since
/// before its registration was removed: the gate withholds keys from the
/// removal until the keyboard's window has the mouse (W3-7 L7, O4), and no
/// other start or stop comes between the removal and the change of the
/// owner (O6). A pointer that no longer owns the mouse changes nothing.
fn pointer_lets_mouse_go(mut route: std::sync::MutexGuard<'static, ClickRoute>, removed_at: u32) {
    if route.owner != Some(RawDevice::Pointer) {
        return;
    }
    route.owner = None;
    if let Some(gate) = &route.gate {
        gate.clicks_lost(removed_at);
    }
    if let Some(window) = route.keyboard_window {
        // SAFETY: PostMessageW takes a window handle and plain values; a
        // window that is gone makes the call fail and changes nothing.
        unsafe {
            let _ = PostMessageW(
                Some(HWND(window as *mut core::ffi::c_void)),
                WM_TAKE_MOUSE,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
}

fn click_from_the_pointer_pump(button_flags: u16, tick: u32) {
    if !button_went_down(button_flags) {
        return;
    }
    if let Some(gate) = clicks().gate.as_ref() {
        gate.latch_after(tick);
    }
}

pub(crate) fn latch_on_button_down(gate: &PasswordGate, bytes: &[u8], tick: u32) -> bool {
    let down = button_flags_of(bytes).is_some_and(button_went_down);
    if down {
        gate.latch(tick);
    }
    down
}

fn is_modifier(vkey: u16) -> bool {
    matches!(vkey, 0x10..=0x12 | 0xA0..=0xA5) || vkey == VK_LWIN.0 || vkey == VK_RWIN.0
}

pub(crate) fn class_of(vkey: u16) -> KeyClass {
    match vkey {
        0x41..=0x5A | 0x30..=0x39 | 0x20 | 0x60..=0x6F | 0xBA..=0xC0 | 0xDB..=0xDF | 0xE2 => {
            KeyClass::Printable
        }
        0x08 | 0x2D | 0x2E => KeyClass::Editing,
        0x21..=0x28 => KeyClass::Navigation,
        0x0D => KeyClass::Enter,
        0x09 => KeyClass::Tab,
        _ => KeyClass::Other,
    }
}

pub(crate) fn map_keystroke(state: &mut KeyState, key: KeyStroke) -> Option<KeyClass> {
    if key.vkey >= NO_KEY {
        return None;
    }
    let down = &mut state.down[usize::from(key.vkey)];
    if key.released {
        *down = false;
        return None;
    }
    if std::mem::replace(down, true) {
        return None;
    }
    if is_modifier(key.vkey) {
        return Some(KeyClass::Other);
    }
    if CHORD_KEYS
        .iter()
        .any(|&chord| state.down[usize::from(chord)])
    {
        return Some(KeyClass::Shortcut);
    }
    Some(class_of(key.vkey))
}

pub(crate) fn keystroke_packet(
    clearance: KeystrokeClearance,
    key: KeyStroke,
    foreground_app: Option<String>,
    tick: u32,
) -> Packet {
    Packet::keystroke(clearance, foreground_app, key.payload_len(), tick)
}

pub(crate) fn pointer_packet(
    sample: PointerSample,
    foreground_app: Option<String>,
    tick: u32,
) -> Packet {
    Packet::observed(
        Observed::Pointer,
        foreground_app,
        sample.payload_len(),
        tick,
    )
}

#[cfg(target_os = "windows")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RawInputPacket {
    Keyboard(KeyStroke),
    Pointer(PointerSample),
}

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
#[cfg(target_os = "windows")]
use windows::Win32::UI::Input::KeyboardAndMouse::{
    VK_CONTROL, VK_ESCAPE, VK_LCONTROL, VK_LMENU, VK_LWIN, VK_MENU, VK_RCONTROL, VK_RETURN,
    VK_RMENU, VK_RWIN, VK_TAB,
};
#[cfg(target_os = "windows")]
use windows::Win32::UI::Input::{
    GetRawInputData, RegisterRawInputDevices, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER,
    RAWKEYBOARD, RAWMOUSE, RIDEV_INPUTSINK, RIDEV_REMOVE, RID_INPUT, RIM_TYPEKEYBOARD,
    RIM_TYPEMOUSE,
};
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetMessageTime, PostMessageW, PostQuitMessage,
    RegisterClassW, HWND_MESSAGE, RI_KEY_BREAK, RI_MOUSE_BUTTON_4_DOWN, RI_MOUSE_BUTTON_5_DOWN,
    RI_MOUSE_LEFT_BUTTON_DOWN, RI_MOUSE_MIDDLE_BUTTON_DOWN, RI_MOUSE_RIGHT_BUTTON_DOWN,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_DESTROY, WM_INPUT, WNDCLASSW,
};

#[cfg(target_os = "windows")]
const HID_USAGE_PAGE_GENERIC: u16 =
    windows::Win32::Devices::HumanInterfaceDevice::HID_USAGE_PAGE_GENERIC;

struct InputState {
    device: RawDevice,
    producer: Producer,
    last_app: Arc<Mutex<Option<String>>>,
    keys: KeyState,
    gate: Option<GateState>,
}

struct GateState {
    gate: Arc<PasswordGate>,
    held: VecDeque<(HeldKey, KeyStroke, Option<String>)>,
    counts: Arc<GateCounts>,
}

#[derive(Debug, Default)]
struct GateCounts {
    /// Whether a key reached the source's handler more than LATE_KEY_MAX
    /// after its OS time: a yes or no that says the source ran behind, kept as
    /// `withheld` is (W3-7 Q1 (b)).
    late: AtomicBool,
    withheld: AtomicBool,
    /// The largest lag from a key's OS time to its handler, in ms.
    max_lag_ms: AtomicU64,
}

impl GateCounts {
    fn note_lag(&self, lag_ms: u32) {
        if lag_ms > ms_of(crate::secure_input::LATE_KEY_MAX) {
            self.late.store(true, Ordering::Relaxed);
        }
        self.max_lag_ms
            .fetch_max(u64::from(lag_ms), Ordering::Relaxed);
    }
}

fn ms_of(duration: std::time::Duration) -> u32 {
    u32::try_from(duration.as_millis()).unwrap_or(u32::MAX)
}

const GATE_POLL_MS: u32 = 15;

/// Hands on the held keys that the gate has decided, oldest first. A key that
/// is still undecided at a stop is withheld like a key that the gate withheld,
/// and no number counts it (W3-7 Q1 (c)).
fn release_held(gate: &mut GateState, producer: &Producer, at_stop: bool) {
    release_held_at(
        gate,
        producer,
        at_stop,
        (super::now_tick(), foreground_thread()),
        crate::secure_input::windows::focus_window_is_password_edit,
    );
}

/// [`release_held`] at the tick and with the thread in front that `at`
/// gives, reading the style of the native edit under the focus through
/// `password_edit_now`.
fn release_held_at(
    gate: &mut GateState,
    producer: &Producer,
    at_stop: bool,
    (now, front): (u32, u32),
    password_edit_now: fn() -> bool,
) {
    while let Some((key, _, _)) = gate.held.front() {
        let held_out =
            now.wrapping_sub(key.tick()) >= ms_of(crate::secure_input::windows::KEY_HOLD);
        let edit_now = held_out && password_edit_now();
        let answer = gate.gate.decide(key, now, front, edit_now);
        if answer.is_none() && !at_stop {
            break;
        }
        let Some((key, stroke, app)) = gate.held.pop_front() else {
            break;
        };
        #[cfg(feature = "test-seam")]
        seam::note(stroke.vkey, &answer, key.tick(), now);
        match answer {
            Some((Ok(clearance), _)) => {
                producer.push(keystroke_packet(clearance, stroke, app, key.tick()));
            }
            None | Some((Err(_), _)) => {
                gate.counts.withheld.store(true, Ordering::Relaxed);
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn foreground_thread() -> u32 {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    // SAFETY: GetForegroundWindow and GetWindowThreadProcessId read handle
    // values and take no pointer that outlives the call.
    unsafe { GetWindowThreadProcessId(GetForegroundWindow(), None) }
}

/// What a test build reads of the keys that a keyboard source decided. Only
/// the feature `test-seam` builds it, and only the crate's dev-dependency on
/// itself turns that feature on, so no product build has it. It holds the
/// class of each key and never its virtual-key code.
#[cfg(feature = "test-seam")]
pub mod seam {
    use std::sync::{Mutex, PoisonError};

    pub use crate::floor::payload::KeyClass;
    pub use crate::secure_input::windows::Rule;
    use crate::secure_input::{KeystrokeClearance, Withheld};

    /// What the keyboard source did with a key.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Verdict {
        Recorded,
        Late,
        WithheldPassword,
        WithheldUnknown,
        /// Still undecided when its source stopped.
        LostAtStop,
    }

    /// One key that a keyboard source decided, in the order of the decisions.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Decided {
        pub class: KeyClass,
        pub verdict: Verdict,
        /// The rule that decided the key; none for a key lost at a stop.
        pub rule: Option<Rule>,
        /// The OS time of the key, in ticks of GetTickCount.
        pub os_tick: u32,
        /// The tick at which the source decided the key.
        pub decided_tick: u32,
    }

    /// The most decisions that the seam keeps for a reader; an older one is
    /// dropped first.
    pub const KEPT: usize = 4096;

    static DECIDED: Mutex<std::collections::VecDeque<Decided>> =
        Mutex::new(std::collections::VecDeque::new());

    /// The marker of a build with the seam (ruling W3-8 (b) (3)): a byte
    /// string that stands in a build with the feature test-seam and in no
    /// build without it, so that a check can read a built file for it.
    pub static MARKER: [u8; 42] = *b"halvo-capture:test-seam:this-build-has-it!";

    pub(super) fn note(
        vkey: u16,
        answer: &Option<(Result<KeystrokeClearance, Withheld>, Rule)>,
        os_tick: u32,
        decided_tick: u32,
    ) {
        let verdict = match answer {
            None => Verdict::LostAtStop,
            Some((Ok(_), _)) => Verdict::Recorded,
            Some((Err(Withheld::Late), _)) => Verdict::Late,
            Some((Err(Withheld::Active), _)) => Verdict::WithheldPassword,
            Some((Err(Withheld::Unknown), _)) => Verdict::WithheldUnknown,
        };
        std::hint::black_box(&MARKER);
        let mut decided = DECIDED.lock().unwrap_or_else(PoisonError::into_inner);
        if decided.len() == KEPT {
            decided.pop_front();
        }
        decided.push_back(Decided {
            class: super::class_of(vkey),
            verdict,
            rule: answer.as_ref().map(|(_, rule)| *rule),
            os_tick,
            decided_tick,
        });
    }

    /// One mark that reached a keyboard source's gate, in the order of their
    /// arrival: what it is (an event, an own read, a latch, a start, a stop),
    /// its state, whether an own read's element moved, its ticks and what the
    /// reading saw of its element (framework, window class, file name and
    /// kind: fixed strings of the programs, never a title or a text).
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct MarkSeen {
        pub what: &'static str,
        pub state: Option<crate::secure_input::windows::FieldState>,
        pub moved: Option<bool>,
        pub at: Option<u32>,
        pub began: u32,
        pub element: Option<String>,
    }

    static MARKS: Mutex<std::collections::VecDeque<MarkSeen>> =
        Mutex::new(std::collections::VecDeque::new());

    pub(crate) fn note_mark(
        what: &'static str,
        state: Option<crate::secure_input::windows::FieldState>,
        moved: Option<bool>,
        at: Option<u32>,
        began: u32,
        element: Option<String>,
    ) {
        let mut marks = MARKS.lock().unwrap_or_else(PoisonError::into_inner);
        if marks.len() == KEPT {
            marks.pop_front();
        }
        marks.push_back(MarkSeen {
            what,
            state,
            moved,
            at,
            began,
            element,
        });
    }

    /// The marks since the last call, oldest first, at most [`KEPT`].
    pub fn take_marks() -> Vec<MarkSeen> {
        std::mem::take(&mut *MARKS.lock().unwrap_or_else(PoisonError::into_inner))
            .into_iter()
            .collect()
    }

    /// The keys decided since the last call, oldest first, at most
    /// [`KEPT`] of them. Nothing of the gate reads them: the seam is read
    /// by a test alone and printed by nothing of the crate.
    pub fn take() -> Vec<Decided> {
        std::mem::take(&mut *DECIDED.lock().unwrap_or_else(PoisonError::into_inner))
            .into_iter()
            .collect()
    }
}

thread_local! {
    static INPUT_CTX: std::cell::RefCell<Option<InputState>> =
        const { std::cell::RefCell::new(None) };
}

impl RawDevice {
    fn index(self) -> usize {
        match self {
            RawDevice::Keyboard => 0,
            RawDevice::Pointer => 1,
        }
    }

    fn class_name(self) -> windows::core::PCWSTR {
        match self {
            RawDevice::Keyboard => windows::core::w!("HalvoRawKeyboard"),
            RawDevice::Pointer => windows::core::w!("HalvoRawPointer"),
        }
    }
}

static DEVICES_IN_USE: Mutex<[bool; 2]> = Mutex::new([false; 2]);

static CLASSES_REGISTERED: Mutex<[bool; 2]> = Mutex::new([false; 2]);

struct DeviceClaim(RawDevice);

impl DeviceClaim {
    fn take(device: RawDevice) -> Result<Self, CaptureError> {
        let mut in_use = DEVICES_IN_USE
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if in_use[device.index()] {
            return Err(CaptureError::AlreadyStarted);
        }
        in_use[device.index()] = true;
        Ok(Self(device))
    }
}

impl Drop for DeviceClaim {
    fn drop(&mut self) {
        DEVICES_IN_USE
            .lock()
            .unwrap_or_else(PoisonError::into_inner)[self.0.index()] = false;
    }
}

#[derive(Clone, Copy)]
struct StartCalls {
    module: fn() -> windows::core::Result<HINSTANCE>,
    register_class: fn(&WNDCLASSW) -> u16,
    create_window: fn(RawDevice, HINSTANCE) -> windows::core::Result<HWND>,
    register_device: RegisterCall,
    remove_device: RemoveCall,
    /// Makes the keyboard pump's poll timer; 0 when it fails.
    set_timer: fn() -> usize,
}

const SYSTEM_CALLS: StartCalls = StartCalls {
    module: module_of_the_process,
    register_class: register_window_class,
    create_window: create_message_window,
    register_device: register_raw_input_device,
    remove_device: register_raw_input_device,
    set_timer: set_poll_timer,
};

fn set_poll_timer() -> usize {
    // SAFETY: SetTimer with no window makes a timer of this thread, whose
    // callback has the TIMERPROC signature; the pump dispatches it.
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::SetTimer(None, 0, GATE_POLL_MS, Some(gate_poll))
    }
}

pub(crate) const REMOVE_STAGE: &str = "RegisterRawInputDevices(RIDEV_REMOVE)";

fn removal_of(device: RawDevice) -> RAWINPUTDEVICE {
    RAWINPUTDEVICE {
        usUsagePage: HID_USAGE_PAGE_GENERIC,
        usUsage: device.hid_usage(),
        dwFlags: RIDEV_REMOVE,
        hwndTarget: HWND(core::ptr::null_mut()),
    }
}

type Removed = Arc<Mutex<bool>>;

fn remove_once(
    removed: &Removed,
    device: RawDevice,
    remove: RemoveCall,
) -> windows::core::Result<()> {
    let mut done = removed.lock().unwrap_or_else(PoisonError::into_inner);
    if std::mem::replace(&mut *done, true) {
        return Ok(());
    }
    remove(&removal_of(device))
}

type RemoveCall = fn(&RAWINPUTDEVICE) -> windows::core::Result<()>;

type RegisterCall = fn(&RAWINPUTDEVICE) -> windows::core::Result<()>;

pub(crate) const MOUSE_REMOVE_STAGE: &str = "RegisterRawInputDevices(RIDEV_REMOVE, mouse)";

fn module_of_the_process() -> windows::core::Result<HINSTANCE> {
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    // SAFETY: GetModuleHandleW with no name reads the handle of this process's
    // executable and takes no pointer.
    unsafe { GetModuleHandleW(None) }.map(Into::into)
}

fn register_window_class(class: &WNDCLASSW) -> u16 {
    // SAFETY: `class` is a live WNDCLASSW whose name is a static wide string
    // and whose procedure has the WNDPROC signature.
    unsafe { RegisterClassW(class) }
}

fn create_message_window(device: RawDevice, hinstance: HINSTANCE) -> windows::core::Result<HWND> {
    // SAFETY: the class name is a static wide string of a registered class,
    // and HWND_MESSAGE makes a message-only window with no other pointer.
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            device.class_name(),
            device.class_name(),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(hinstance),
            None,
        )
    }
}

fn register_raw_input_device(device: &RAWINPUTDEVICE) -> windows::core::Result<()> {
    // SAFETY: the slice holds one initialized RAWINPUTDEVICE, and the size
    // argument is the size of that struct.
    unsafe {
        RegisterRawInputDevices(
            core::slice::from_ref(device),
            core::mem::size_of::<RAWINPUTDEVICE>() as u32,
        )
    }
}

fn register_class(
    device: RawDevice,
    hinstance: HINSTANCE,
    register: fn(&WNDCLASSW) -> u16,
) -> Result<(), CaptureError> {
    let mut registered = CLASSES_REGISTERED
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if registered[device.index()] {
        return Ok(());
    }
    let class = WNDCLASSW {
        lpfnWndProc: Some(wnd_proc),
        hInstance: hinstance,
        lpszClassName: device.class_name(),
        ..Default::default()
    };
    if register(&class) == 0 {
        return Err(super::last_os_error("RegisterClassW"));
    }
    registered[device.index()] = true;
    Ok(())
}

/// Raw Input capture source for one device.
pub struct RawInputSource {
    device: RawDevice,
    last_app: Arc<Mutex<Option<String>>>,
    life: Lifecycle,
    calls: StartCalls,
    removed: Removed,
    gate: Option<Arc<PasswordGate>>,
    gate_counts: Arc<GateCounts>,
}

impl RawInputSource {
    /// A keyboard source, attributing keystrokes to `last_app` (shared with the
    /// foreground source).
    pub fn keyboard(last_app: Arc<Mutex<Option<String>>>, gate: PasswordGate) -> Self {
        Self {
            gate: Some(Arc::new(gate)),
            ..Self::new(RawDevice::Keyboard, last_app)
        }
    }

    pub fn pointer(last_app: Arc<Mutex<Option<String>>>) -> Self {
        Self::new(RawDevice::Pointer, last_app)
    }

    fn new(device: RawDevice, last_app: Arc<Mutex<Option<String>>>) -> Self {
        Self {
            device,
            last_app,
            life: Lifecycle::default(),
            calls: SYSTEM_CALLS,
            removed: Removed::default(),
            gate: None,
            gate_counts: Arc::default(),
        }
    }

    #[cfg(test)]
    fn with_calls(device: RawDevice, calls: StartCalls) -> Self {
        let gate = (device == RawDevice::Keyboard).then(|| Arc::new(PasswordGate::new()));
        Self {
            calls,
            gate,
            ..Self::new(device, Arc::new(Mutex::new(None)))
        }
    }

    /// The counts of this source's hand-off queues since it was made.
    pub fn handoff_counts(&self) -> super::HandOffCounts {
        self.life.handoff_counts()
    }

    /// The state that the password gate of a keyboard source read last.
    pub fn field_state(&self) -> Option<crate::secure_input::windows::FieldState> {
        self.gate.as_ref().map(|gate| gate.field_now())
    }
}

#[cfg(target_os = "windows")]
fn decode_raw_input(bytes: &[u8]) -> Option<RawInputPacket> {
    #[repr(C, align(8))]
    struct Aligned([u8; core::mem::size_of::<RAWINPUT>()]);

    let header = core::mem::size_of::<RAWINPUTHEADER>();
    let kind = u32::from_ne_bytes(bytes.get(..4)?.try_into().ok()?);
    let record = if kind == RIM_TYPEKEYBOARD.0 {
        core::mem::size_of::<RAWKEYBOARD>()
    } else if kind == RIM_TYPEMOUSE.0 {
        core::mem::size_of::<RAWMOUSE>()
    } else {
        return None;
    };
    if bytes.len() < header + record {
        return None;
    }
    let mut aligned = Aligned([0u8; core::mem::size_of::<RAWINPUT>()]);
    let n = bytes.len().min(aligned.0.len());
    aligned.0[..n].copy_from_slice(&bytes[..n]);
    // SAFETY: `aligned` is align(8) >= align_of::<RAWINPUT>(), is exactly
    // size_of::<RAWINPUT>() bytes, and is fully initialized; and every bit
    // pattern is a valid RAWINPUT, because it is made of integers, a raw
    // device handle and a union of integer records, with no reference, enum
    // or bool in it.
    let raw: &RAWINPUT = unsafe { &*(aligned.0.as_ptr() as *const RAWINPUT) };

    if raw.header.dwType == RIM_TYPEKEYBOARD.0 {
        // SAFETY: the header names a keyboard record, so `keyboard` is the
        // member of the union that the bytes hold; every bit pattern is valid.
        let kb = unsafe { raw.data.keyboard };
        Some(RawInputPacket::Keyboard(KeyStroke {
            vkey: kb.VKey,
            released: kb.Flags & RI_KEY_BREAK as u16 != 0,
        }))
    } else if raw.header.dwType == RIM_TYPEMOUSE.0 {
        // SAFETY: the header names a mouse record, so `mouse` is the member of
        // the union that the bytes hold; every bit pattern is valid.
        let m = unsafe { raw.data.mouse };
        // SAFETY: both members of the inner union are plain integers over the
        // same bytes, so reading the flag and data words is valid.
        let (button_flags, button_data) = unsafe {
            (
                m.Anonymous.Anonymous.usButtonFlags,
                m.Anonymous.Anonymous.usButtonData,
            )
        };
        Some(RawInputPacket::Pointer(PointerSample {
            last_x: m.lLastX,
            last_y: m.lLastY,
            button_flags,
            wheel_delta: button_data as i16,
        }))
    } else {
        None
    }
}

#[cfg(target_os = "windows")]
fn emit_from_raw(packet: RawInputPacket, tick: u32) {
    INPUT_CTX.with(|cell| {
        let mut borrow = cell.borrow_mut();
        let Some(state) = borrow.as_mut() else {
            return;
        };
        let app = super::app_of(&state.last_app);
        match (state.device, packet) {
            (RawDevice::Keyboard, RawInputPacket::Keyboard(key)) => {
                let Some(gate) = state.gate.as_mut() else {
                    return;
                };
                if map_keystroke(&mut state.keys, key).is_none() {
                    return;
                }
                gate.counts.note_lag(super::now_tick().wrapping_sub(tick));
                let held = gate.gate.hold(
                    tick,
                    crate::secure_input::windows::focus_window_is_password_edit(),
                    foreground_thread(),
                );
                if moves_focus(key.vkey) {
                    gate.gate.latch(tick);
                }
                gate.held.push_back((held, key, app));
                release_held(gate, &state.producer, false);
            }
            (RawDevice::Pointer, RawInputPacket::Pointer(sample)) => {
                click_from_the_pointer_pump(sample.button_flags, tick);
                let packet = pointer_packet(sample, app, tick);
                if sample.is_motion_only() {
                    state.producer.push_motion(packet);
                } else {
                    state.producer.push(packet);
                }
            }
            _ => {}
        }
    });
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_INPUT => {
            read_raw_input(
                HRAWINPUT(lparam.0 as *mut core::ffi::c_void),
                message_tick(),
            );
            // WM_INPUT docs: the application must call DefWindowProc so the system
            // can perform cleanup.
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_TAKE_MOUSE => {
            keyboard_takes_mouse_again();
            LRESULT(0)
        }
        #[cfg(test)]
        WM_HOLD_FOR_TEST => {
            std::thread::sleep(std::time::Duration::from_millis(lparam.0 as u64));
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

#[cfg(target_os = "windows")]
fn message_tick() -> u32 {
    // SAFETY: GetMessageTime takes no argument and reads the time of the
    // message that this thread's pump took last.
    unsafe { GetMessageTime() as u32 }
}

fn read_raw_input(handle: HRAWINPUT, tick: u32) {
    let header_size = core::mem::size_of::<RAWINPUTHEADER>() as u32;
    let mut size: u32 = 0;
    // SAFETY: with no data pointer GetRawInputData writes the packet's size
    // into `size`, a live local, and reads the handle that WM_INPUT gave.
    let probe = unsafe { GetRawInputData(handle, RID_INPUT, None, &mut size, header_size) };
    if probe != 0 || size == 0 {
        return;
    }
    let mut buf = vec![0u8; size as usize];
    // SAFETY: `buf` holds `size` bytes, the size that the first call gave,
    // and outlives the call that fills it.
    let read = unsafe {
        GetRawInputData(
            handle,
            RID_INPUT,
            Some(buf.as_mut_ptr() as *mut core::ffi::c_void),
            &mut size,
            header_size,
        )
    };
    if read != size {
        return;
    }
    if keyboard_reads_a_click(&buf, tick) {
        return;
    }
    if let Some(packet) = decode_raw_input(&buf) {
        emit_from_raw(packet, tick);
    }
}

fn keyboard_reads_a_click(bytes: &[u8], tick: u32) -> bool {
    INPUT_CTX.with(|cell| {
        let borrow = cell.borrow();
        let Some(state) = borrow.as_ref() else {
            return false;
        };
        let (RawDevice::Keyboard, Some(gate)) = (state.device, state.gate.as_ref()) else {
            return false;
        };
        if button_flags_of(bytes).is_none() {
            return false;
        }
        latch_on_button_down(&gate.gate, bytes, tick);
        true
    })
}

impl CaptureSource for RawInputSource {
    fn begin(&mut self, ctx: SourceContext) -> Result<Started, CaptureError> {
        if self.life.is_running() {
            return Err(CaptureError::AlreadyStarted);
        }
        let claim = DeviceClaim::take(self.device)?;
        let device = self.device;
        let last_app = Arc::clone(&self.last_app);
        let calls = self.calls;
        let removed = Removed::default();
        self.removed = Arc::clone(&removed);
        if let Some(gate) = &self.gate {
            gate.attach()?;
        }
        let gate = self.gate.as_ref().map(|gate| GateState {
            gate: Arc::clone(gate),
            held: VecDeque::new(),
            counts: Arc::clone(&self.gate_counts),
        });
        let begun = self
            .life
            .begin(&ctx, "halvo-rawinput-forward", move |producer| {
                let state = InputState {
                    device,
                    producer,
                    last_app,
                    keys: KeyState::default(),
                    gate,
                };
                let name = format!("halvo-rawinput-{device:?}");
                super::spawn_pump(name, move |ready, signal| {
                    run_raw_input(state, (claim, removed), calls, ready, &signal)
                })
            });
        if begun.is_err() {
            if let Some(gate) = &self.gate {
                let _ = gate.detach(super::INPUT_STOP_DEADLINE);
            }
        }
        begun
    }

    fn shutdown(&mut self) -> Result<(), CaptureError> {
        let stopped = self.life.shutdown();
        if let Err(CaptureError::Timeout { .. }) = stopped {
            if self.device == RawDevice::Keyboard {
                let _ = remove_once(&self.removed, self.device, self.calls.remove_device);
                let _ = keyboard_lets_mouse_go(self.calls.remove_device);
            } else {
                // W3-7 O4: the pump that did not stop in time still owns
                // the mouse; its registration is removed here and the
                // keyboard's window is told to take the mouse.
                let route = clicks();
                let _ = remove_once(&self.removed, self.device, self.calls.remove_device);
                pointer_lets_mouse_go(route, super::now_tick());
            }
        }
        let Some(gate) = &self.gate else {
            return stopped;
        };
        match (stopped, gate.detach(super::INPUT_STOP_DEADLINE)) {
            (Err(error), _) => Err(error),
            (Ok(()), Err(error)) => {
                self.life.removal_failed(error.clone());
                Err(error)
            }
            (Ok(()), Ok(())) => Ok(()),
        }
    }

    fn health(&self) -> SourceHealth {
        let counters = match self.device {
            RawDevice::Keyboard => SourceCounters {
                late_events: u64::from(self.gate_counts.late.load(Ordering::Relaxed)),
                max_lag_ms: self.gate_counts.max_lag_ms.load(Ordering::Relaxed),
                keys_withheld: self.gate_counts.withheld.load(Ordering::Relaxed),
                ..SourceCounters::default()
            },
            RawDevice::Pointer => SourceCounters {
                pointer_coalesced: self.life.handoff_counts().coalesced,
                ..SourceCounters::default()
            },
        };
        self.life.health(counters)
    }
}

struct DeviceGuard<'a> {
    device: RawDevice,
    removed: Removed,
    remove: RemoveCall,
    signal: &'a super::StopSignal,
    /// Let go after the click route has changed (W3-7 O6).
    claim: Option<DeviceClaim>,
}

impl Drop for DeviceGuard<'_> {
    fn drop(&mut self) {
        let removed = remove_once(&self.removed, self.device, self.remove);
        self.signal.report_removal(REMOVE_STAGE, removed);
    }
}

struct CtxGuard;

impl Drop for CtxGuard {
    fn drop(&mut self) {
        INPUT_CTX.with(|cell| *cell.borrow_mut() = None);
    }
}

struct WindowGuard {
    hwnd: HWND,
}

impl Drop for WindowGuard {
    fn drop(&mut self) {
        // SAFETY: `hwnd` is the window that this thread created; a window that
        // is already gone makes the call fail and changes nothing.
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

fn run_raw_input(
    state: InputState,
    (claim, removed): (DeviceClaim, Removed),
    calls: StartCalls,
    ready: PumpReady,
    signal: &super::StopSignal,
) {
    let device = state.device;
    let gate = state.gate.as_ref().map(|held| Arc::clone(&held.gate));
    let hinstance: HINSTANCE = match (calls.module)() {
        Ok(h) => h,
        Err(error) => {
            ready.report(Err(super::os_error("GetModuleHandleW", &error)));
            return;
        }
    };
    if let Err(error) = register_class(device, hinstance, calls.register_class) {
        ready.report(Err(error));
        return;
    }
    let hwnd = match (calls.create_window)(device, hinstance) {
        Ok(h) => h,
        Err(error) => {
            ready.report(Err(super::os_error("CreateWindowExW", &error)));
            return;
        }
    };
    let _window_guard = WindowGuard { hwnd };

    INPUT_CTX.with(|cell| *cell.borrow_mut() = Some(state));
    let ctx_guard = CtxGuard;

    let dev = RAWINPUTDEVICE {
        usUsagePage: HID_USAGE_PAGE_GENERIC,
        usUsage: device.hid_usage(),
        dwFlags: RIDEV_INPUTSINK,
        hwndTarget: hwnd,
    };
    // The pointer's registration replaces the keyboard's mouse
    // registration, so it is made with the owner's change under the route's
    // lock (W3-7 O6).
    let registered = if device == RawDevice::Pointer {
        let mut route = clicks();
        let registered = (calls.register_device)(&dev);
        if registered.is_ok() {
            route.owner = Some(RawDevice::Pointer);
        }
        registered
    } else {
        (calls.register_device)(&dev)
    };
    if let Err(error) = registered {
        ready.report(Err(super::os_error("RegisterRawInputDevices", &error)));
        return;
    }
    let mut device_guard = DeviceGuard {
        device,
        removed,
        remove: calls.remove_device,
        signal,
        claim: Some(claim),
    };
    if device == RawDevice::Keyboard {
        {
            let mut route = clicks();
            route.gate = gate;
            route.keyboard_window = Some(hwnd.0 as usize);
            route.keyboard_register = Some(calls.register_device);
        }
        if let Err(error) = keyboard_takes_mouse(hwnd, calls.register_device) {
            let _ = keyboard_lets_mouse_go(calls.remove_device);
            ready.report(Err(super::os_error(
                "RegisterRawInputDevices(mouse)",
                &error,
            )));
            return;
        }
    }

    super::ensure_message_queue();
    let poll = if device == RawDevice::Keyboard {
        let poll = (calls.set_timer)();
        if poll == 0 {
            let error = super::last_os_error("SetTimer");
            // W3-7 O5: a start that fails here leaves no click route set.
            let _ = keyboard_lets_mouse_go(calls.remove_device);
            ready.report(Err(error));
            return;
        }
        Some(poll)
    } else {
        None
    };
    ready.report(Ok(()));
    super::pump_messages(signal);
    if device == RawDevice::Keyboard {
        drop(device_guard);
        signal.report_removal(
            MOUSE_REMOVE_STAGE,
            keyboard_lets_mouse_go(calls.remove_device),
        );
    } else {
        let claim = device_guard.claim.take();
        let route = clicks();
        drop(device_guard);
        pointer_lets_mouse_go(route, super::now_tick());
        drop(claim);
    }
    INPUT_CTX.with(|cell| {
        if let Some(state) = cell.borrow_mut().as_mut() {
            if let Some(gate) = state.gate.as_mut() {
                release_held(gate, &state.producer, true);
            }
        }
    });
    if let Some(poll) = poll {
        // SAFETY: `poll` is the timer of this thread made above.
        let killed = unsafe { windows::Win32::UI::WindowsAndMessaging::KillTimer(None, poll) };
        signal.report_removal("KillTimer", killed);
    }
    drop(ctx_guard);
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn gate_poll(_hwnd: HWND, _msg: u32, _id: usize, _time: u32) {
    keyboard_takes_mouse_again();
    INPUT_CTX.with(|cell| {
        if let Some(state) = cell.borrow_mut().as_mut() {
            if let Some(gate) = state.gate.as_mut() {
                release_held(gate, &state.producer, false);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secure_input::windows::Rule;

    #[test]
    fn a_cleared_key_maps_to_a_keystroke_packet() {
        let packet = keystroke_packet(
            crate::windows::tests::cleared(),
            KeyStroke {
                vkey: 0x41,
                released: false,
            },
            Some("C:\\editor.exe".to_string()),
            7,
        );
        assert_eq!(packet.kind(), crate::event::Modality::Keystroke);
        assert_eq!(packet.app.as_deref(), Some("C:\\editor.exe"));
        assert_eq!(packet.byte_len, 2);
        assert_eq!(packet.tick, 7);
    }

    #[test]
    fn a_key_up_is_not_recorded() {
        let key = KeyStroke {
            vkey: 0x41,
            released: true,
        };
        assert_eq!(map_keystroke(&mut KeyState::default(), key), None);
    }

    #[test]
    fn keys_tab_enter_escape_and_the_windows_keys_move_the_focus() {
        for vkey in [VK_TAB, VK_RETURN, VK_ESCAPE, VK_LWIN, VK_RWIN] {
            assert!(moves_focus(vkey.0), "{:#x}", vkey.0);
        }
        for vkey in [0x41, 0x20, 0x10, 0x11, 0x12, 0x25, 0x7E] {
            assert!(!moves_focus(vkey), "{vkey:#x}");
        }
    }

    fn mouse_record(button_flags: u16, last_x: i32) -> Vec<u8> {
        use windows::Win32::UI::Input::{RAWINPUT, RAWINPUT_0, RAWMOUSE, RAWMOUSE_0, RAWMOUSE_0_0};
        let raw = RAWINPUT {
            header: RAWINPUTHEADER {
                dwType: RIM_TYPEMOUSE.0,
                ..Default::default()
            },
            data: RAWINPUT_0 {
                mouse: RAWMOUSE {
                    lLastX: last_x,
                    Anonymous: RAWMOUSE_0 {
                        Anonymous: RAWMOUSE_0_0 {
                            usButtonFlags: button_flags,
                            usButtonData: 0,
                        },
                    },
                    ..Default::default()
                },
            },
        };
        // SAFETY: `raw` is a live, initialized RAWINPUT, and the slice covers
        // exactly its bytes while `raw` lives.
        unsafe {
            core::slice::from_raw_parts(
                &raw as *const RAWINPUT as *const u8,
                core::mem::size_of::<RAWINPUT>(),
            )
        }
        .to_vec()
    }

    fn clear_gate() -> PasswordGate {
        let gate = PasswordGate::new();
        gate.read_at(900, crate::secure_input::windows::FieldState::Clear, None);
        gate
    }

    #[test]
    fn a_raw_button_down_latches_the_gate() {
        for flags in [
            RI_MOUSE_LEFT_BUTTON_DOWN,
            RI_MOUSE_RIGHT_BUTTON_DOWN,
            RI_MOUSE_MIDDLE_BUTTON_DOWN,
            RI_MOUSE_BUTTON_4_DOWN,
            RI_MOUSE_BUTTON_5_DOWN,
        ] {
            let gate = clear_gate();
            assert!(latch_on_button_down(
                &gate,
                &mouse_record(flags as u16, 0),
                1_000
            ));
            let key = gate.hold(1_010, false, 0);
            assert!(
                gate.clearance(&key, 1_200).is_none(),
                "a key after a button-down did not wait for the read: {flags:#x}"
            );
        }
    }

    #[test]
    fn a_raw_mouse_message_that_is_no_button_down_does_not_latch() {
        use windows::Win32::UI::WindowsAndMessaging::{
            RI_MOUSE_LEFT_BUTTON_UP, RI_MOUSE_RIGHT_BUTTON_UP, RI_MOUSE_WHEEL,
        };
        for flags in [
            0,
            RI_MOUSE_LEFT_BUTTON_UP,
            RI_MOUSE_RIGHT_BUTTON_UP,
            RI_MOUSE_WHEEL,
        ] {
            let gate = clear_gate();
            assert!(!latch_on_button_down(
                &gate,
                &mouse_record(flags as u16, 0x155),
                1_000
            ));
            let key = gate.hold(1_010, false, 0);
            gate.read_at(
                1_130,
                crate::secure_input::windows::FieldState::Clear,
                Some(false),
            );
            assert!(
                matches!(gate.clearance(&key, 1_200), Some(Ok(_))),
                "a raw message with flags {flags:#x} closed the gate"
            );
        }
        let keyboard = keyboard_packet(0x41, 0);
        // SAFETY: `keyboard` is a live, initialized RAWINPUT, and the slice
        // covers exactly its bytes while `keyboard` lives.
        let bytes = unsafe {
            core::slice::from_raw_parts(
                &keyboard as *const windows::Win32::UI::Input::RAWINPUT as *const u8,
                core::mem::size_of::<windows::Win32::UI::Input::RAWINPUT>(),
            )
        };
        assert_eq!(button_flags_of(bytes), None);
        assert_eq!(button_flags_of(&mouse_record(1, 0)[..20]), None);
    }

    static REGISTERED: Mutex<Vec<(u16, bool, usize)>> = Mutex::new(Vec::new());

    fn register_counted(device: &RAWINPUTDEVICE) -> windows::core::Result<()> {
        REGISTERED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((
                device.usUsage,
                device.dwFlags == RIDEV_REMOVE,
                device.hwndTarget.0 as usize,
            ));
        register_raw_input_device(device)
    }

    fn registrations() -> Vec<(u16, bool)> {
        std::mem::take(&mut *REGISTERED.lock().unwrap_or_else(PoisonError::into_inner))
            .into_iter()
            .map(|(usage, removal, _)| (usage, removal))
            .collect()
    }

    /// Whether the seam recorded the mouse for `window` after the last
    /// removal of the mouse.
    fn mouse_registered_again_for(window: usize) -> bool {
        let calls = REGISTERED.lock().unwrap_or_else(PoisonError::into_inner);
        let mouse = RawDevice::Pointer.hid_usage();
        let Some(removal) = calls
            .iter()
            .rposition(|(usage, removal, _)| *usage == mouse && *removal)
        else {
            return false;
        };
        calls[removal..].contains(&(mouse, false, window))
    }

    fn calls_for_tests() -> StartCalls {
        StartCalls {
            register_device: register_counted,
            remove_device: register_counted,
            ..SYSTEM_CALLS
        }
    }

    #[test]
    fn the_keyboard_takes_the_mouse_back_when_the_pointer_source_stops() {
        use windows::Win32::UI::WindowsAndMessaging::RI_MOUSE_LEFT_BUTTON_DOWN;
        let _serial = crate::windows::tests::serial();
        let _ = registrations();
        let (sink, _writer) = crate::sink::channel(64);
        let mut keyboard = RawInputSource::with_calls(RawDevice::Keyboard, calls_for_tests());
        let keyboard_started = keyboard.begin(crate::source::SourceContext::new(sink.clone()));
        let keyboard_window = clicks().keyboard_window;
        let owner_with_the_keyboard = clicks().owner;
        let mut pointer = RawInputSource::with_calls(RawDevice::Pointer, calls_for_tests());
        let pointer_started = pointer.begin(crate::source::SourceContext::new(sink));
        let owner_with_both = clicks().owner;
        let pointer_stopped = pointer.shutdown();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut taken_back = false;
        while !taken_back && std::time::Instant::now() < deadline {
            taken_back = keyboard_window.is_some_and(mouse_registered_again_for);
            if !taken_back {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
        let owner_after = clicks().owner;
        let gate = clicks().gate.clone();
        let (latches_before, latched, latches_after) = match &gate {
            Some(gate) => {
                let tick = super::super::now_tick();
                let latches_before = gate.latch_count();
                let latched = latch_on_button_down(
                    gate,
                    &mouse_record(RI_MOUSE_LEFT_BUTTON_DOWN as u16, 0),
                    tick,
                );
                (latches_before, latched, gate.latch_count())
            }
            None => (0, false, 0),
        };
        let keyboard_stopped = keyboard.shutdown();
        let _ = registrations();
        assert!(keyboard_started.is_ok(), "{keyboard_started:?}");
        assert!(pointer_started.is_ok(), "{pointer_started:?}");
        assert_eq!(owner_with_the_keyboard, Some(RawDevice::Keyboard));
        assert_eq!(owner_with_both, Some(RawDevice::Pointer));
        assert_eq!(pointer_stopped, Ok(()));
        assert!(
            taken_back,
            "the keyboard's window did not register the mouse again within 5 s of the pointer's stop"
        );
        assert_eq!(owner_after, Some(RawDevice::Keyboard));
        assert!(latched, "the raw button-down did not latch the gate");
        assert_eq!(
            latches_after,
            latches_before + 1,
            "the raw button-down left no latch in the keyboard's gate"
        );
        assert_eq!(keyboard_stopped, Ok(()));
    }

    /// A mouse registration call that fails while `FAIL_MOUSE` is set, and
    /// one that holds its thread for `HOLD_AFTER_MOUSE_MS` after a mouse
    /// registration that it made; both are counted.
    static FAIL_MOUSE: AtomicBool = AtomicBool::new(false);
    static HOLD_AFTER_MOUSE_MS: AtomicU64 = AtomicU64::new(0);

    fn register_mouse_seam(device: &RAWINPUTDEVICE) -> windows::core::Result<()> {
        let mouse =
            device.usUsage == RawDevice::Pointer.hid_usage() && device.dwFlags != RIDEV_REMOVE;
        if mouse && FAIL_MOUSE.load(Ordering::SeqCst) {
            return Err(windows::core::Error::from_hresult(
                windows::Win32::Foundation::E_ACCESSDENIED,
            ));
        }
        let made = register_counted(device);
        let hold = if mouse {
            HOLD_AFTER_MOUSE_MS.swap(0, Ordering::SeqCst)
        } else {
            0
        };
        if hold > 0 {
            std::thread::sleep(std::time::Duration::from_millis(hold));
        }
        made
    }

    fn calls_with_the_mouse_seam() -> StartCalls {
        StartCalls {
            register_device: register_mouse_seam,
            remove_device: register_counted,
            ..SYSTEM_CALLS
        }
    }

    /// The rule that decides a key typed at `tick`, at 300 ms after it.
    fn rule_for_a_key_at(gate: &PasswordGate, tick: u32) -> Option<Rule> {
        let key = gate.hold(tick, false, 0);
        gate.decide(&key, tick.wrapping_add(300), 0, false)
            .map(|(_, rule)| rule)
    }

    fn wait_for(limit: std::time::Duration, mut done: impl FnMut() -> bool) -> bool {
        let deadline = std::time::Instant::now() + limit;
        loop {
            if done() {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    /// W3-7 L7: a mouse registration that fails at the hand-over holds the
    /// gate closed, and the keyboard's window registers the mouse when the
    /// call stands again.
    #[test]
    fn a_failed_mouse_registration_at_the_hand_over_holds_the_gate_closed_until_it_stands() {
        let _serial = crate::windows::tests::serial();
        let _ = registrations();
        FAIL_MOUSE.store(false, Ordering::SeqCst);
        let (sink, _writer) = crate::sink::channel(64);
        let mut keyboard =
            RawInputSource::with_calls(RawDevice::Keyboard, calls_with_the_mouse_seam());
        let keyboard_started = keyboard.begin(crate::source::SourceContext::new(sink.clone()));
        let keyboard_window = clicks().keyboard_window;
        let gate = clicks().gate.clone().expect("the keyboard's gate");
        let mut pointer = RawInputSource::with_calls(RawDevice::Pointer, calls_for_tests());
        let pointer_started = pointer.begin(crate::source::SourceContext::new(sink));
        FAIL_MOUSE.store(true, Ordering::SeqCst);
        let pointer_stopped = pointer.shutdown();
        std::thread::sleep(std::time::Duration::from_millis(200));
        let owner_while_failing = clicks().owner;
        let rule_while_failing = rule_for_a_key_at(&gate, super::super::now_tick());
        FAIL_MOUSE.store(false, Ordering::SeqCst);
        let stood = wait_for(std::time::Duration::from_secs(2), || {
            keyboard_window.is_some_and(mouse_registered_again_for)
        });
        let owner_after = clicks().owner;
        std::thread::sleep(std::time::Duration::from_millis(200));
        let rule_after = rule_for_a_key_at(&gate, super::super::now_tick());
        let keyboard_stopped = keyboard.shutdown();
        let _ = registrations();
        assert!(keyboard_started.is_ok(), "{keyboard_started:?}");
        assert!(pointer_started.is_ok(), "{pointer_started:?}");
        assert_eq!(pointer_stopped, Ok(()));
        assert_eq!(owner_while_failing, None);
        assert_eq!(rule_while_failing, Some(Rule::NoClickRoute));
        assert!(
            stood,
            "the keyboard's window did not register the mouse when the call stood again"
        );
        assert_eq!(owner_after, Some(RawDevice::Keyboard));
        assert_ne!(rule_after, Some(Rule::NoClickRoute));
        assert_eq!(keyboard_stopped, Ok(()));
    }

    /// W3-7 O4: a pointer whose pump does not stop in time gives the mouse
    /// to the keyboard's window, and the gate withholds the keys between.
    #[test]
    fn a_pointer_stop_that_times_out_hands_the_mouse_to_the_keyboard() {
        use windows::Win32::UI::WindowsAndMessaging::{FindWindowExW, PostMessageW};
        let _serial = crate::windows::tests::serial();
        let _ = registrations();
        let (sink, _writer) = crate::sink::channel(64);
        let mut keyboard = RawInputSource::with_calls(RawDevice::Keyboard, calls_for_tests());
        let keyboard_started = keyboard.begin(crate::source::SourceContext::new(sink.clone()));
        let keyboard_window = clicks().keyboard_window;
        let gate = clicks().gate.clone().expect("the keyboard's gate");
        let mut pointer = RawInputSource::with_calls(RawDevice::Pointer, calls_for_tests());
        let pointer_started = pointer.begin(crate::source::SourceContext::new(sink));
        // SAFETY: the class name is a static wide string; HWND_MESSAGE
        // searches the message-only windows.
        let pointer_window = unsafe {
            FindWindowExW(
                Some(HWND_MESSAGE),
                None,
                RawDevice::Pointer.class_name(),
                None,
            )
        };
        let held = pointer_window.as_ref().is_ok_and(|window| {
            // SAFETY: PostMessageW takes a window handle and plain values.
            unsafe {
                PostMessageW(Some(*window), WM_HOLD_FOR_TEST, WPARAM(0), LPARAM(4_000)).is_ok()
            }
        });
        std::thread::sleep(std::time::Duration::from_millis(100));
        let _ = registrations();
        let pointer_stopped = pointer.shutdown();
        let stopped_at = super::super::now_tick();
        let taken = wait_for(std::time::Duration::from_secs(1), || {
            keyboard_window.is_some_and(mouse_registered_again_for)
        });
        let owner_after = clicks().owner;
        let rule_at_the_stop = rule_for_a_key_at(&gate, stopped_at.wrapping_sub(50));
        std::thread::sleep(std::time::Duration::from_millis(4_200));
        let owner_after_the_hold = clicks().owner;
        let keyboard_stopped = keyboard.shutdown();
        let _ = registrations();
        assert!(keyboard_started.is_ok(), "{keyboard_started:?}");
        assert!(pointer_started.is_ok(), "{pointer_started:?}");
        assert!(held, "the test could not hold the pointer's pump");
        assert!(
            matches!(pointer_stopped, Err(CaptureError::Timeout { .. })),
            "{pointer_stopped:?}"
        );
        assert!(
            taken,
            "the keyboard's window had no mouse registration within 1 s of the Timeout"
        );
        assert_eq!(owner_after, Some(RawDevice::Keyboard));
        assert_eq!(rule_at_the_stop, Some(Rule::NoClickRoute));
        assert_eq!(owner_after_the_hold, Some(RawDevice::Keyboard));
        assert_eq!(keyboard_stopped, Ok(()));
    }

    fn no_timer() -> usize {
        0
    }

    /// W3-7 O5: a keyboard start that fails at its timer leaves no click
    /// route, so that the next start registers the mouse for its window.
    #[test]
    fn a_keyboard_start_that_fails_at_its_timer_leaves_no_click_route() {
        let _serial = crate::windows::tests::serial();
        let _ = registrations();
        let (sink, _writer) = crate::sink::channel(64);
        let mut failing = RawInputSource::with_calls(
            RawDevice::Keyboard,
            StartCalls {
                set_timer: no_timer,
                ..calls_for_tests()
            },
        );
        let failed = failing.begin(crate::source::SourceContext::new(sink.clone()));
        let owner_after_the_failure = clicks().owner;
        let _ = registrations();
        let mut keyboard = RawInputSource::with_calls(RawDevice::Keyboard, calls_for_tests());
        let started = keyboard.begin(crate::source::SourceContext::new(sink));
        let window = clicks().keyboard_window;
        let calls = REGISTERED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let stopped = keyboard.shutdown();
        let _ = registrations();
        let mouse = RawDevice::Pointer.hid_usage();
        assert!(
            matches!(&failed, Err(CaptureError::Os { stage, .. }) if *stage == "SetTimer"),
            "{failed:?}"
        );
        assert_eq!(owner_after_the_failure, None);
        assert!(started.is_ok(), "{started:?}");
        assert!(
            window.is_some_and(|window| calls.contains(&(mouse, false, window))),
            "the second start registered no mouse for its window"
        );
        assert_eq!(stopped, Ok(()));
    }

    /// W3-7 O6: the pointer's registration and the change of the owner are
    /// one step, so a keyboard stop between them does not remove the
    /// pointer's registration.
    #[test]
    fn a_keyboard_stop_during_the_pointers_registration_leaves_the_pointers_mouse() {
        let _serial = crate::windows::tests::serial();
        let _ = registrations();
        let (sink, _writer) = crate::sink::channel(64);
        let mut keyboard = RawInputSource::with_calls(RawDevice::Keyboard, calls_for_tests());
        let keyboard_started = keyboard.begin(crate::source::SourceContext::new(sink.clone()));
        HOLD_AFTER_MOUSE_MS.store(400, Ordering::SeqCst);
        let starting = std::thread::spawn(move || {
            let mut pointer =
                RawInputSource::with_calls(RawDevice::Pointer, calls_with_the_mouse_seam());
            let started = pointer.begin(crate::source::SourceContext::new(sink));
            (pointer, started)
        });
        std::thread::sleep(std::time::Duration::from_millis(150));
        let keyboard_stopped = keyboard.shutdown();
        let (mut pointer, pointer_started) = starting.join().expect("the pointer's start");
        let owner = clicks().owner;
        let calls = registrations();
        let pointer_stopped = pointer.shutdown();
        let _ = registrations();
        HOLD_AFTER_MOUSE_MS.store(0, Ordering::SeqCst);
        let mouse = RawDevice::Pointer.hid_usage();
        let last_mouse_call = calls
            .iter()
            .rev()
            .find(|(usage, _)| *usage == mouse)
            .copied();
        assert!(keyboard_started.is_ok(), "{keyboard_started:?}");
        assert!(pointer_started.is_ok(), "{pointer_started:?}");
        assert_eq!(keyboard_stopped, Ok(()));
        assert_eq!(owner, Some(RawDevice::Pointer));
        assert_eq!(
            last_mouse_call,
            Some((mouse, false)),
            "the keyboard's stop removed the pointer's mouse registration: {calls:?}"
        );
        assert_eq!(pointer_stopped, Ok(()));
    }

    #[test]
    fn the_pointer_keeps_the_mouse_when_the_keyboard_source_stops() {
        let _serial = crate::windows::tests::serial();
        let _ = registrations();
        let (sink, _writer) = crate::sink::channel(64);
        let mut keyboard = RawInputSource::with_calls(RawDevice::Keyboard, calls_for_tests());
        let keyboard_started = keyboard.begin(crate::source::SourceContext::new(sink.clone()));
        let mut pointer = RawInputSource::with_calls(RawDevice::Pointer, calls_for_tests());
        let pointer_started = pointer.begin(crate::source::SourceContext::new(sink));
        let _ = registrations();
        let keyboard_stopped = keyboard.shutdown();
        let calls_at_the_keyboard_stop = registrations();
        let owner_after = clicks().owner;
        let pointer_stopped = pointer.shutdown();
        let calls_at_the_pointer_stop = registrations();
        let mouse = RawDevice::Pointer.hid_usage();
        assert!(keyboard_started.is_ok(), "{keyboard_started:?}");
        assert!(pointer_started.is_ok(), "{pointer_started:?}");
        assert_eq!(keyboard_stopped, Ok(()));
        assert_eq!(
            calls_at_the_keyboard_stop,
            vec![(RawDevice::Keyboard.hid_usage(), true)],
            "the keyboard's stop touched the pointer's mouse registration"
        );
        assert_eq!(owner_after, Some(RawDevice::Pointer));
        assert_eq!(pointer_stopped, Ok(()));
        assert_eq!(calls_at_the_pointer_stop, vec![(mouse, true)]);
        assert_eq!(clicks().owner, None);
    }

    #[test]
    fn a_click_closes_the_gate_while_no_pointer_source_runs() {
        let _serial = crate::windows::tests::serial();
        let _ = registrations();
        let (sink, _writer) = crate::sink::channel(64);
        let mut source = RawInputSource::with_calls(
            RawDevice::Keyboard,
            StartCalls {
                register_device: register_counted,
                remove_device: register_counted,
                ..SYSTEM_CALLS
            },
        );
        let started = source.begin(crate::source::SourceContext::new(sink));
        let owner = clicks().owner;
        let routed = clicks().gate.is_some();
        let stopped = source.shutdown();
        let calls = registrations();
        let mouse = RawDevice::Pointer.hid_usage();
        let keyboard = RawDevice::Keyboard.hid_usage();
        assert!(started.is_ok(), "{started:?}");
        assert_eq!(
            owner,
            Some(RawDevice::Keyboard),
            "the keyboard source did not take the mouse"
        );
        assert!(
            routed,
            "the click route held no gate while the keyboard source ran"
        );
        assert_eq!(stopped, Ok(()));
        assert_eq!(
            calls,
            vec![
                (keyboard, false),
                (mouse, false),
                (keyboard, true),
                (mouse, true)
            ]
        );
        assert_eq!(clicks().owner, None);
    }

    #[test]
    fn the_pump_and_the_stopping_thread_make_one_removal_call_between_them() {
        let _serial = crate::windows::tests::serial();
        let _ = registrations();
        let removed = Removed::default();
        let first = remove_once(&removed, RawDevice::Keyboard, register_counted);
        let second = remove_once(&removed, RawDevice::Keyboard, register_counted);
        let calls = registrations();
        let _ = (first, second);
        assert_eq!(calls, vec![(RawDevice::Keyboard.hid_usage(), true)]);
    }

    #[test]
    fn decided_keys_go_out_in_input_order_and_a_key_undecided_at_the_stop_is_withheld_uncounted() {
        let (sink, writer) = crate::sink::channel(8);
        let handoff = super::super::HandOff::start(sink, "halvo-test-forward", Arc::default())
            .expect("hand-off");
        let producer = handoff.producer();
        let mut state = GateState {
            gate: Arc::new(clear_gate()),
            held: VecDeque::new(),
            counts: Arc::default(),
        };
        let now = super::super::now_tick();
        let stroke = KeyStroke {
            vkey: 0x41,
            released: false,
        };
        let front = foreground_thread();
        state.gate.read_at(
            now.wrapping_sub(200),
            crate::secure_input::windows::FieldState::Clear,
            None,
        );
        let early = now.wrapping_sub(160);
        let later = now.wrapping_sub(150);
        for (tick, app) in [(early, "first"), (later, "second")] {
            let held = state.gate.hold(tick, false, front);
            state.held.push_back((held, stroke, Some(app.to_string())));
        }
        state.gate.read_at(
            now.wrapping_sub(25),
            crate::secure_input::windows::FieldState::Clear,
            Some(false),
        );
        state.gate.latch(now);
        let waiting = state.gate.hold(now, false, front);
        state
            .held
            .push_back((waiting, stroke, Some("third".to_string())));
        release_held(&mut state, &producer, false);
        let still_held = state.held.len();
        release_held(&mut state, &producer, true);
        let mut apps = Vec::new();
        while let Ok(received) = writer.recv_timeout(std::time::Duration::from_secs(1)) {
            apps.push(received.event().foreground_app.clone());
            if apps.len() == 2 {
                break;
            }
        }
        assert_eq!(still_held, 1);
        assert_eq!(apps, vec!["first".to_string(), "second".to_string()]);
        assert!(state.held.is_empty());
        assert_eq!(handoff.counts().lost, 0);
        assert!(state.counts.withheld.load(Ordering::Relaxed));
    }

    /// A gate state with a hand-off, as the keyboard pump holds them.
    fn lag_rig() -> (GateState, super::super::HandOff, crate::sink::SinkWriter) {
        let (sink, writer) = crate::sink::channel(64);
        let handoff = super::super::HandOff::start(sink, "halvo-test-forward", Arc::default())
            .expect("hand-off");
        let state = GateState {
            gate: Arc::new(PasswordGate::new()),
            held: VecDeque::new(),
            counts: Arc::default(),
        };
        (state, handoff, writer)
    }

    /// One key as the pump handles it: its lag noted, held, then released.
    fn handled(state: &mut GateState, producer: &Producer, os_tick: u32) {
        let now = super::super::now_tick();
        state.counts.note_lag(now.wrapping_sub(os_tick));
        let key = state.gate.hold(os_tick, false, foreground_thread());
        state.held.push_back((
            key,
            KeyStroke {
                vkey: 0x41,
                released: false,
            },
            None,
        ));
        release_held(state, producer, false);
    }

    /// Every number of the source: the late value, the largest lag, the
    /// withheld flag and the hand-off's counts.
    fn numbers(state: &GateState, handoff: &super::super::HandOff) -> (bool, u64, bool, String) {
        (
            state.counts.late.load(Ordering::Relaxed),
            state.counts.max_lag_ms.load(Ordering::Relaxed),
            state.counts.withheld.load(Ordering::Relaxed),
            format!("{:?}", handoff.counts()),
        )
    }

    fn sent(writer: &crate::sink::SinkWriter) -> usize {
        let mut sent = 0;
        while writer
            .recv_timeout(std::time::Duration::from_millis(200))
            .is_ok()
        {
            sent += 1;
        }
        sent
    }

    #[test]
    fn five_keys_of_a_password_field_handled_300_ms_late_move_no_number() {
        let (mut state, handoff, writer) = lag_rig();
        let producer = handoff.producer();
        let now = super::super::now_tick();
        state.gate.read_at(
            now.wrapping_sub(400),
            crate::secure_input::windows::FieldState::Password,
            None,
        );
        let before = numbers(&state, &handoff);
        handled(&mut state, &producer, now.wrapping_sub(300));
        let after_one = numbers(&state, &handoff);
        for step in 1..5u32 {
            handled(
                &mut state,
                &producer,
                now.wrapping_sub(300).wrapping_add(step),
            );
        }
        let after_five = numbers(&state, &handoff);
        assert!(!before.0 && !before.2, "{before:?}");
        assert!(after_one.0 && after_one.2, "{after_one:?}");
        assert_eq!(after_one.3, after_five.3);
        assert_eq!((after_one.0, after_one.2), (after_five.0, after_five.2));
        assert!(state.held.is_empty());
        assert_eq!(sent(&writer), 0);
    }

    #[test]
    fn late_keys_of_a_password_field_stay_withheld_after_a_move_to_a_plain_field() {
        use crate::secure_input::windows::FieldState;
        let (mut state, handoff, writer) = lag_rig();
        let producer = handoff.producer();
        let now = super::super::now_tick();
        state
            .gate
            .read_at(now.wrapping_sub(400), FieldState::Password, None);
        state
            .gate
            .field_at(now.wrapping_sub(250), FieldState::Clear);
        state
            .gate
            .read_at(now.wrapping_sub(240), FieldState::Clear, Some(true));
        for step in 0..5u32 {
            handled(
                &mut state,
                &producer,
                now.wrapping_sub(300).wrapping_add(step),
            );
        }
        assert!(state.counts.withheld.load(Ordering::Relaxed));
        assert_eq!(sent(&writer), 0);
    }

    #[test]
    fn late_keys_stay_withheld_when_the_password_mark_comes_after_their_input_time() {
        use crate::secure_input::windows::FieldState;
        let (mut state, handoff, writer) = lag_rig();
        let producer = handoff.producer();
        let now = super::super::now_tick();
        state
            .gate
            .read_at(now.wrapping_sub(400), FieldState::Clear, None);
        state
            .gate
            .field_at(now.wrapping_sub(280), FieldState::Password);
        for step in 0..5u32 {
            handled(
                &mut state,
                &producer,
                now.wrapping_sub(300).wrapping_add(step),
            );
        }
        assert!(state.counts.withheld.load(Ordering::Relaxed));
        assert_eq!(sent(&writer), 0);
    }

    #[test]
    fn five_late_keys_of_a_plain_field_are_withheld_and_set_the_late_value_alone() {
        use crate::secure_input::windows::FieldState;
        let (mut state, handoff, writer) = lag_rig();
        let producer = handoff.producer();
        let now = super::super::now_tick();
        state
            .gate
            .read_at(now.wrapping_sub(400), FieldState::Clear, None);
        state
            .gate
            .read_at(now.wrapping_sub(170), FieldState::Clear, Some(false));
        for step in 0..5u32 {
            handled(
                &mut state,
                &producer,
                now.wrapping_sub(300).wrapping_add(step),
            );
        }
        let (late, max_lag, withheld, _) = numbers(&state, &handoff);
        assert!(late && withheld, "{late} {withheld}");
        assert!(max_lag >= 300, "{max_lag}");
        assert_eq!(handoff.counts().lost, 0);
        assert_eq!(sent(&writer), 0);
    }

    #[test]
    fn a_password_typed_during_two_seconds_of_lag_moves_no_number() {
        use crate::secure_input::windows::FieldState;
        let (mut state, handoff, writer) = lag_rig();
        let producer = handoff.producer();
        let now = super::super::now_tick();
        state
            .gate
            .read_at(now.wrapping_sub(2_400), FieldState::Clear, None);
        for step in 0..12u32 {
            handled(
                &mut state,
                &producer,
                now.wrapping_sub(2_300).wrapping_add(step * 100),
            );
        }
        let around_before = numbers(&state, &handoff);
        state
            .gate
            .field_at(now.wrapping_sub(1_050), FieldState::Password);
        for step in 0..8u32 {
            handled(
                &mut state,
                &producer,
                now.wrapping_sub(1_000).wrapping_add(step * 80),
            );
        }
        let around_after = numbers(&state, &handoff);
        assert_eq!(around_before, around_after);
        assert_eq!(sent(&writer), 0);
    }

    static HOLD_THE_REGISTER_MS: AtomicU64 = AtomicU64::new(0);
    static REGISTER_HELD: AtomicBool = AtomicBool::new(false);

    /// The seam's register call, which holds the keyboard's pump for the
    /// milliseconds that the test stores, so that the pump takes keys late.
    fn register_held(device: &RAWINPUTDEVICE) -> windows::core::Result<()> {
        let hold = HOLD_THE_REGISTER_MS.swap(0, Ordering::SeqCst);
        if hold > 0 {
            REGISTER_HELD.store(true, Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(hold));
            REGISTER_HELD.store(false, Ordering::SeqCst);
        }
        register_raw_input_device(device)
    }

    /// A visible window of the test's own on a thread of its own that pumps
    /// its messages, brought to the front so that the keys that a test sends
    /// go to it; a window that nobody pumps would hold the detector's read.
    struct OwnWindow {
        stop: Arc<AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl OwnWindow {
        fn in_front() -> (OwnWindow, bool) {
            use windows::Win32::UI::Input::KeyboardAndMouse::{
                SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
                KEYEVENTF_KEYUP, VK_MENU,
            };
            use windows::Win32::UI::WindowsAndMessaging::{
                CreateWindowExW, DestroyWindow, DispatchMessageW, GetForegroundWindow,
                PeekMessageW, SetForegroundWindow, ShowWindow, TranslateMessage, MSG, PM_REMOVE,
                SW_SHOW, WINDOW_EX_STYLE, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
            };
            let stop = Arc::new(AtomicBool::new(false));
            let (tx, rx) = std::sync::mpsc::channel();
            let thread_stop = Arc::clone(&stop);
            let thread = std::thread::spawn(move || {
                let class = windows::core::HSTRING::from("STATIC");
                // SAFETY: the class name is a live HSTRING; the window belongs
                // to this thread, which destroys it before it ends.
                let Ok(hwnd) = (unsafe {
                    CreateWindowExW(
                        WINDOW_EX_STYLE(0),
                        windows::core::PCWSTR(class.as_ptr()),
                        windows::core::PCWSTR::null(),
                        WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                        80,
                        80,
                        240,
                        120,
                        None,
                        None,
                        None,
                        None,
                    )
                }) else {
                    let _ = tx.send(false);
                    return;
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
                let pump = || {
                    let mut msg = MSG::default();
                    // SAFETY: `msg` is a live MSG that PeekMessageW fills, and
                    // the same value goes to TranslateMessage and
                    // DispatchMessageW.
                    unsafe {
                        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                            let _ = TranslateMessage(&msg);
                            DispatchMessageW(&msg);
                        }
                    }
                };
                // SAFETY: the calls take this thread's window handle and a
                // slice of one live INPUT value with the size of one INPUT.
                unsafe {
                    let _ = ShowWindow(hwnd, SW_SHOW);
                    let _ = SendInput(&[alt(false)], core::mem::size_of::<INPUT>() as i32);
                    let _ = SetForegroundWindow(hwnd);
                    let _ = SendInput(&[alt(true)], core::mem::size_of::<INPUT>() as i32);
                }
                for _ in 0..30 {
                    pump();
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                // SAFETY: GetForegroundWindow takes no argument.
                let _ = tx.send(unsafe { GetForegroundWindow() } == hwnd);
                while !thread_stop.load(Ordering::SeqCst) {
                    pump();
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                // SAFETY: the window belongs to this thread and is destroyed
                // once.
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
                pump();
            });
            let in_front = rx
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap_or(false);
            (
                OwnWindow {
                    stop,
                    thread: Some(thread),
                },
                in_front,
            )
        }
    }

    impl Drop for OwnWindow {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    /// K6 of W3-7, from the verify step's probe: a key of a password field
    /// and a click of the same tick from the pointer pump, with both sources
    /// running, leave the key withheld, as a click one tick period later does.
    #[test]
    #[ignore = "functional: needs an interactive Windows desktop; a step of the windows job names it"]
    fn a_click_of_the_same_tick_from_the_pointer_pump_leaves_a_password_key_withheld() {
        use crate::secure_input::windows::FieldState;
        use windows::Win32::UI::WindowsAndMessaging::RI_MOUSE_LEFT_BUTTON_DOWN;
        let _serial = crate::windows::tests::serial();
        let _ = registrations();
        let (sink, _writer) = crate::sink::channel(64);
        let mut keyboard = RawInputSource::with_calls(RawDevice::Keyboard, calls_for_tests());
        let keyboard_started = keyboard.begin(crate::source::SourceContext::new(sink.clone()));
        let mut pointer = RawInputSource::with_calls(RawDevice::Pointer, calls_for_tests());
        let pointer_started = pointer.begin(crate::source::SourceContext::new(sink));
        let gate = clicks().gate.clone();
        let answer = |click_after_ms: u32| {
            gate.as_ref().map(|gate| {
                let tick = super::super::now_tick();
                gate.field_at(tick, FieldState::Password);
                let key = gate.hold(tick.wrapping_add(1), false, foreground_thread());
                click_from_the_pointer_pump(
                    RI_MOUSE_LEFT_BUTTON_DOWN as u16,
                    tick.wrapping_add(1 + click_after_ms),
                );
                gate.field_at(tick.wrapping_add(40), FieldState::Clear);
                gate.read_at(tick.wrapping_add(130), FieldState::Clear, Some(true));
                matches!(gate.clearance(&key, tick.wrapping_add(200)), Some(Err(_)))
            })
        };
        let same_tick = answer(0);
        let one_period_later = answer(16);
        let pointer_stopped = pointer.shutdown();
        let keyboard_stopped = keyboard.shutdown();
        let _ = registrations();
        println!(
            "TF same-tick keyboard_started={} pointer_started={} withheld_with_a_click_of_the_same_tick={same_tick:?} withheld_with_a_click_16_ms_later={one_period_later:?} pointer_stopped={} keyboard_stopped={}",
            keyboard_started.is_ok(),
            pointer_started.is_ok(),
            pointer_stopped.is_ok(),
            keyboard_stopped.is_ok()
        );
        assert!(
            keyboard_started.is_ok() && pointer_started.is_ok(),
            "INCONCLUSIVE: a source did not start"
        );
        assert_eq!(
            one_period_later,
            Some(true),
            "the control: a click 16 ms later"
        );
        assert_eq!(
            same_tick,
            Some(true),
            "a click of the same tick cleared a key of a password field"
        );
    }

    /// K5 and Q1 (d) of W3-7, from the verify step's probe: a real key that
    /// the keyboard's own pump takes late, because the pump is held in the
    /// register call of the mouse handover, while the gate read Password at
    /// the key and a focus mark of a later move reaches the gate. No key is
    /// recorded, and a key taken 300 ms late moves no count.
    #[test]
    #[ignore = "functional: needs an interactive Windows desktop; a step of the windows job names it"]
    fn a_key_that_a_held_pump_takes_late_is_not_recorded_and_moves_no_count() {
        use crate::secure_input::windows::FieldState;
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
            KEYEVENTF_KEYUP, VK_A,
        };
        let key = |released: bool| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_A,
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
        let _serial = crate::windows::tests::serial();
        let (own, in_front) = OwnWindow::in_front();
        let (sink, writer) = crate::sink::channel(256);
        let (tx, events) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            while let Some(event) = writer.recv() {
                let keystroke = event.modality == crate::event::Modality::Keystroke;
                if tx.send(keystroke).is_err() {
                    break;
                }
            }
        });
        let mut keyboard = RawInputSource::with_calls(
            RawDevice::Keyboard,
            StartCalls {
                register_device: register_held,
                ..SYSTEM_CALLS
            },
        );
        let started = keyboard.begin(crate::source::SourceContext::new(sink));
        let gate = clicks().gate.clone();
        let window = clicks().keyboard_window;
        let mut recorded = Vec::new();
        let mut late_300 = (0, false);
        if let (Ok(_), Some(gate), Some(window)) = (&started, gate.as_ref(), window) {
            let rounds: [(&str, u64); 6] = [
                ("on-time-1", 0),
                ("held-150-1", 150),
                ("on-time-2", 0),
                ("held-150-2", 150),
                ("held-180", 180),
                ("held-300", 300),
            ];
            for (name, hold_ms) in rounds {
                std::thread::sleep(std::time::Duration::from_millis(300));
                while events.try_recv().is_ok() {}
                let before = keyboard.health().counters;
                gate.field_at(super::super::now_tick(), FieldState::Password);
                let mut held = false;
                if hold_ms > 0 {
                    HOLD_THE_REGISTER_MS.store(hold_ms, Ordering::SeqCst);
                    clicks().owner = None;
                    // SAFETY: PostMessageW takes a window handle and plain
                    // values; a window that is gone makes the call fail.
                    unsafe {
                        let _ = PostMessageW(
                            Some(HWND(window as *mut core::ffi::c_void)),
                            WM_TAKE_MOUSE,
                            WPARAM(0),
                            LPARAM(0),
                        );
                    }
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
                    while !held && std::time::Instant::now() < deadline {
                        held = REGISTER_HELD.load(Ordering::SeqCst);
                        if !held {
                            std::thread::sleep(std::time::Duration::from_millis(1));
                        }
                    }
                }
                // SAFETY: the slice holds two live INPUT values and the size
                // argument is the size of one INPUT.
                let accepted = unsafe {
                    SendInput(
                        &[key(false), key(true)],
                        core::mem::size_of::<INPUT>() as i32,
                    )
                };
                std::thread::sleep(std::time::Duration::from_millis(30));
                gate.field_at(super::super::now_tick(), FieldState::Clear);
                std::thread::sleep(std::time::Duration::from_millis(hold_ms + 500));
                let keystroke_events = events.try_iter().filter(|keystroke| *keystroke).count();
                let after = keyboard.health().counters;
                println!(
                    "TF held-pump round={name} hold_ms={hold_ms} pump_was_held={held} accepted={accepted} keystroke_events={keystroke_events} late_value_before={} late_value_after={} keys_withheld={}",
                    before.late_events, after.late_events, after.keys_withheld
                );
                if name == "held-300" {
                    late_300 = (after.late_events, after.keys_withheld);
                }
                recorded.push(keystroke_events);
            }
        }
        let stopped = keyboard.shutdown();
        drop(own);
        println!(
            "TF held-pump own_window_in_front={in_front} started={} stopped={} rounds={}",
            started.is_ok(),
            stopped.is_ok(),
            recorded.len()
        );
        assert!(
            started.is_ok(),
            "INCONCLUSIVE: the keyboard source did not start"
        );
        assert_eq!(recorded.len(), 6, "INCONCLUSIVE: the rounds did not run");
        assert_eq!(
            recorded.iter().sum::<usize>(),
            0,
            "a key typed while the gate read Password was recorded"
        );
        assert!(
            late_300.0 <= 1,
            "the late value is a yes or no and read {}",
            late_300.0
        );
        assert!(late_300.1, "keys_withheld did not read yes");
    }

    /// Runs a drawn sequence of marks and keys through a gate with the
    /// release of the keyboard pump and answers the apps of the keys that it
    /// recorded and the withheld flag. With `read_the_seam` a reader takes
    /// the seam after every release.
    fn drawn_sequence(seed: u64, read_the_seam: bool) -> (Vec<String>, bool) {
        use crate::secure_input::windows::FieldState;
        let (mut state, handoff, writer) = lag_rig();
        let producer = handoff.producer();
        let front = 7;
        let mut draw = seed;
        let mut next = move || {
            draw = draw
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (draw >> 33) as u32
        };
        let state_of = |r: u32| match r % 10 {
            8 => FieldState::Password,
            9 => FieldState::Unknown,
            _ => FieldState::Clear,
        };
        let mut tick = 10_000u32;
        state.gate.read_at(tick, FieldState::Clear, None);
        for key in 0..400u32 {
            let r = next();
            tick = tick.wrapping_add(r % 15);
            match (r >> 8) % 20 {
                0 => state.gate.field_at(tick, state_of(r >> 12)),
                1..=8 => state
                    .gate
                    .read_at(tick, state_of(r >> 12), Some((r >> 16) % 2 == 0)),
                9 => state.gate.latch(tick),
                10 => state.gate.latch_after(tick.wrapping_sub(r % 20)),
                _ => {
                    let held = state.gate.hold(tick, false, front);
                    state.held.push_back((
                        held,
                        KeyStroke {
                            vkey: 0x41,
                            released: false,
                        },
                        Some(key.to_string()),
                    ));
                }
            }
            release_held_at(&mut state, &producer, false, (tick, front), || false);
            if read_the_seam {
                let _ = seam::take();
            }
        }
        release_held_at(&mut state, &producer, true, (tick, front), || false);
        let mut apps = Vec::new();
        while let Ok(received) = writer.recv_timeout(std::time::Duration::from_millis(300)) {
            apps.push(received.event().foreground_app.clone());
        }
        (apps, state.counts.withheld.load(Ordering::Relaxed))
    }

    #[test]
    fn a_gate_with_a_reader_of_the_seam_and_one_without_give_the_same_verdicts() {
        for seed in [3, 41, 977] {
            let read = drawn_sequence(seed, true);
            let unread = drawn_sequence(seed, false);
            assert!(
                !read.0.is_empty() && read.1,
                "the drawn sequence decided too little: {seed}"
            );
            assert_eq!(read, unread, "seed {seed}");
        }
    }

    #[test]
    fn the_seam_keeps_at_most_its_bound_and_the_test_build_holds_its_marker() {
        let reversed = b"!ti-sah-dliub-siht:maes-tset:erutpac-ovlah";
        let needle: Vec<u8> = reversed.iter().rev().copied().collect();
        let exe = std::env::current_exe().expect("the test executable");
        let bytes = std::fs::read(exe).expect("read the test executable");
        let holds = bytes
            .windows(needle.len())
            .any(|window| window == needle.as_slice());
        assert!(holds, "the test build does not hold the seam's marker");
        let answer = Some((
            Err(crate::secure_input::Withheld::Unknown),
            crate::secure_input::windows::Rule::NotClearedInTime,
        ));
        for _ in 0..seam::KEPT + 10 {
            seam::note(0x41, &answer, 1, 2);
        }
        assert!(seam::take().len() <= seam::KEPT);
    }

    fn vkey_of(key: crate::floor::cases::Key) -> u16 {
        use crate::floor::cases::{Key, Modifier};
        match key {
            Key::Letter => 0x41,
            Key::Digit => 0x31,
            Key::Space => 0x20,
            Key::Punctuation => 0xBE,
            Key::Backspace => 0x08,
            Key::Delete => 0x2E,
            Key::Insert => 0x2D,
            Key::Arrow => 0x25,
            Key::Home => 0x24,
            Key::End => 0x23,
            Key::PageUp => 0x21,
            Key::PageDown => 0x22,
            Key::Enter => 0x0D,
            Key::Tab => 0x09,
            Key::Escape => 0x1B,
            Key::Function => 0x70,
            Key::Modifier(Modifier::Shift) => 0x10,
            Key::Modifier(Modifier::Control) => 0x11,
            Key::Modifier(Modifier::OptionOrAlt) => 0x12,
            Key::Modifier(Modifier::CommandOrWindows) => 0x5B,
        }
    }

    #[test]
    fn map_keystroke_gives_the_events_of_the_shared_list() {
        use crate::floor::cases::{Expected, Input, CASES};
        let mut ran = 0;
        let mut failing = Vec::new();
        for case in CASES {
            let keys_only = case.inputs.iter().all(|input| {
                matches!(
                    input,
                    Input::KeyDown(_) | Input::KeyRepeat(_) | Input::KeyUp(_)
                )
            });
            if !keys_only {
                continue;
            }
            ran += 1;
            let mut state = KeyState::default();
            let given: Vec<Expected> = case
                .inputs
                .iter()
                .filter_map(|input| {
                    let (key, released) = match *input {
                        Input::KeyDown(key) | Input::KeyRepeat(key) => (key, false),
                        Input::KeyUp(key) => (key, true),
                        _ => unreachable!("a case of keys alone"),
                    };
                    let stroke = KeyStroke {
                        vkey: vkey_of(key),
                        released,
                    };
                    map_keystroke(&mut state, stroke).map(Expected::Key)
                })
                .collect();
            if given != case.expected {
                failing.push(case.name);
            }
        }
        assert!(ran >= 15, "only {ran} cases of keys alone ran");
        assert!(failing.is_empty(), "failing cases: {failing:?}");
    }

    #[test]
    fn a_packet_of_vkey_255_is_no_event_and_changes_no_state() {
        let mut state = KeyState::default();
        let fake = KeyStroke {
            vkey: NO_KEY,
            released: false,
        };
        assert_eq!(map_keystroke(&mut state, fake), None);
        assert_eq!(map_keystroke(&mut state, fake), None);
        let letter = KeyStroke {
            vkey: 0x41,
            released: false,
        };
        assert_eq!(map_keystroke(&mut state, letter), Some(KeyClass::Printable));
        assert_eq!(map_keystroke(&mut state, letter), None);
    }

    #[test]
    fn a_pointer_sample_maps_to_a_pointer_packet_and_only_a_flagless_one_is_motion() {
        let packet = pointer_packet(
            PointerSample::default(),
            Some("C:\\game.exe".to_string()),
            9,
        );
        assert_eq!(packet.kind(), crate::event::Modality::Pointer);
        assert_eq!(packet.app.as_deref(), Some("C:\\game.exe"));
        assert_eq!(packet.byte_len, 12);
        assert_eq!(packet.tick, 9);
        assert!(PointerSample::default().is_motion_only());
        let wheel = PointerSample {
            button_flags: 0x0400,
            ..PointerSample::default()
        };
        assert!(!wheel.is_motion_only());
    }

    #[test]
    fn decoded_payload_sizes_are_the_measured_field_sizes() {
        assert_eq!(
            KeyStroke {
                vkey: 0,
                released: false
            }
            .payload_len(),
            2
        );
        assert_eq!(PointerSample::default().payload_len(), 12);
    }

    #[cfg(target_os = "windows")]
    fn keyboard_packet(vkey: u16, flags: u16) -> windows::Win32::UI::Input::RAWINPUT {
        use windows::Win32::UI::Input::{RAWINPUT, RAWINPUTHEADER, RAWINPUT_0, RAWKEYBOARD};
        RAWINPUT {
            header: RAWINPUTHEADER {
                dwType: RIM_TYPEKEYBOARD.0,
                ..Default::default()
            },
            data: RAWINPUT_0 {
                keyboard: RAWKEYBOARD {
                    VKey: vkey,
                    Flags: flags,
                    ..Default::default()
                },
            },
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn decode_reads_a_keyboard_packet_from_a_misaligned_buffer() {
        use windows::Win32::UI::Input::RAWINPUT;
        let raw = keyboard_packet(0x41, 0);
        // SAFETY: `raw` is a live, initialized RAWINPUT, and the slice covers
        // exactly its bytes for as long as `raw` lives.
        let bytes = unsafe {
            core::slice::from_raw_parts(
                &raw as *const RAWINPUT as *const u8,
                core::mem::size_of::<RAWINPUT>(),
            )
        };
        let mut backing = vec![0u8; bytes.len() + 8];
        let off = 1usize;
        backing[off..off + bytes.len()].copy_from_slice(bytes);
        assert_ne!(
            (backing.as_ptr() as usize + off) % 8,
            0,
            "slice is misaligned"
        );
        let packet = decode_raw_input(&backing[off..off + bytes.len()]).expect("keyboard packet");
        assert_eq!(
            packet,
            RawInputPacket::Keyboard(KeyStroke {
                vkey: 0x41,
                released: false
            })
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn decode_reads_the_break_flag_as_a_key_up() {
        use windows::Win32::UI::Input::RAWINPUT;
        let raw = keyboard_packet(0x42, RI_KEY_BREAK as u16);
        // SAFETY: `raw` is a live, initialized RAWINPUT, and the slice covers
        // exactly its bytes for as long as `raw` lives.
        let bytes = unsafe {
            core::slice::from_raw_parts(
                &raw as *const RAWINPUT as *const u8,
                core::mem::size_of::<RAWINPUT>(),
            )
        };
        let packet = decode_raw_input(bytes).expect("keyboard packet");
        assert_eq!(
            packet,
            RawInputPacket::Keyboard(KeyStroke {
                vkey: 0x42,
                released: true
            })
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn decode_reads_a_mouse_packet() {
        use windows::Win32::UI::Input::{
            RAWINPUT, RAWINPUTHEADER, RAWINPUT_0, RAWMOUSE, RAWMOUSE_0, RAWMOUSE_0_0,
        };
        let raw = RAWINPUT {
            header: RAWINPUTHEADER {
                dwType: RIM_TYPEMOUSE.0,
                ..Default::default()
            },
            data: RAWINPUT_0 {
                mouse: RAWMOUSE {
                    lLastX: 5,
                    lLastY: -3,
                    Anonymous: RAWMOUSE_0 {
                        Anonymous: RAWMOUSE_0_0 {
                            usButtonFlags: 0,
                            usButtonData: 120,
                        },
                    },
                    ..Default::default()
                },
            },
        };
        // SAFETY: `raw` is a live, initialized RAWINPUT, and the slice covers
        // exactly its bytes for as long as `raw` lives.
        let bytes = unsafe {
            core::slice::from_raw_parts(
                &raw as *const RAWINPUT as *const u8,
                core::mem::size_of::<RAWINPUT>(),
            )
        };
        let packet = decode_raw_input(bytes).expect("mouse packet");
        assert_eq!(
            packet,
            RawInputPacket::Pointer(PointerSample {
                last_x: 5,
                last_y: -3,
                button_flags: 0,
                wheel_delta: 120,
            })
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn decode_rejects_a_short_buffer() {
        assert!(decode_raw_input(&[0u8; 2]).is_none());
    }

    #[cfg(target_os = "windows")]
    fn header_of(kind: u32) -> Vec<u8> {
        use windows::Win32::UI::Input::RAWINPUTHEADER;
        let header = RAWINPUTHEADER {
            dwType: kind,
            dwSize: core::mem::size_of::<RAWINPUTHEADER>() as u32,
            ..Default::default()
        };
        // SAFETY: `header` is a live, initialized RAWINPUTHEADER, and the slice
        // covers exactly its bytes while `header` lives.
        unsafe {
            core::slice::from_raw_parts(
                &header as *const RAWINPUTHEADER as *const u8,
                core::mem::size_of::<RAWINPUTHEADER>(),
            )
        }
        .to_vec()
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn decode_rejects_a_buffer_of_exactly_the_header_or_one_byte_short_of_its_record() {
        use windows::Win32::UI::Input::{RAWINPUTHEADER, RAWKEYBOARD, RAWMOUSE};
        let keyboard = header_of(RIM_TYPEKEYBOARD.0);
        let mouse = header_of(RIM_TYPEMOUSE.0);
        assert_eq!(keyboard.len(), core::mem::size_of::<RAWINPUTHEADER>());
        assert_eq!(decode_raw_input(&keyboard), None);
        assert_eq!(decode_raw_input(&mouse), None);
        let mut short_key = keyboard.clone();
        short_key.resize(keyboard.len() + core::mem::size_of::<RAWKEYBOARD>() - 1, 0);
        assert_eq!(decode_raw_input(&short_key), None);
        let mut short_mouse = mouse.clone();
        short_mouse.resize(mouse.len() + core::mem::size_of::<RAWMOUSE>() - 1, 0);
        assert_eq!(decode_raw_input(&short_mouse), None);
        let mut whole_key = keyboard;
        whole_key.resize(short_key.len() + 1, 0);
        assert!(decode_raw_input(&whole_key).is_some());
    }

    #[test]
    fn a_keyboard_source_refuses_a_second_begin_and_starts_again_after_its_stop() {
        use crate::source::{SourceContext, SourceState, Started};
        let _serial = crate::windows::tests::serial();
        let (sink, _writer) = crate::sink::channel(64);
        let mut source = RawInputSource::keyboard(Arc::new(Mutex::new(None)), PasswordGate::new());
        let first = source.begin(SourceContext::new(sink.clone()));
        let second = source.begin(SourceContext::new(sink.clone()));
        let running = source.health().state;
        let stopped = source.shutdown();
        let again = source.begin(SourceContext::new(sink));
        let _ = source.shutdown();
        assert!(matches!(first, Ok(Started::Running)), "{first:?}");
        assert_eq!(second, Err(CaptureError::AlreadyStarted));
        assert_eq!(running, SourceState::Running);
        assert_eq!(stopped, Ok(()));
        assert!(matches!(again, Ok(Started::Running)), "{again:?}");
    }

    #[test]
    fn a_second_keyboard_source_in_the_process_waits_for_the_first_to_stop() {
        let _serial = crate::windows::tests::serial();
        let (sink, _writer) = crate::sink::channel(64);
        let mut first = RawInputSource::keyboard(Arc::new(Mutex::new(None)), PasswordGate::new());
        let mut second = RawInputSource::keyboard(Arc::new(Mutex::new(None)), PasswordGate::new());
        let first_started = first.begin(crate::source::SourceContext::new(sink.clone()));
        let while_first_runs = second.begin(crate::source::SourceContext::new(sink.clone()));
        let first_stopped = first.shutdown();
        let after_first_stopped = second.begin(crate::source::SourceContext::new(sink));
        let _ = second.shutdown();
        assert!(first_started.is_ok(), "{first_started:?}");
        assert_eq!(while_first_runs, Err(CaptureError::AlreadyStarted));
        assert_eq!(first_stopped, Ok(()));
        assert!(after_first_stopped.is_ok(), "{after_first_stopped:?}");
    }

    use windows::Win32::Foundation::{
        ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, ERROR_MOD_NOT_FOUND, ERROR_NOT_ENOUGH_MEMORY,
        WIN32_ERROR,
    };

    fn refusal(code: WIN32_ERROR) -> windows::core::Error {
        windows::core::Error::from_hresult(code.to_hresult())
    }

    fn refused_at(stage: &'static str, code: WIN32_ERROR) -> CaptureError {
        CaptureError::Os {
            stage,
            domain: "HRESULT",
            code: i64::from(code.to_hresult().0),
        }
    }

    fn no_module() -> windows::core::Result<HINSTANCE> {
        Err(refusal(ERROR_MOD_NOT_FOUND))
    }

    fn class_refused(_class: &WNDCLASSW) -> u16 {
        // SAFETY: SetLastError writes a plain value into this thread's last
        // error.
        unsafe { windows::Win32::Foundation::SetLastError(ERROR_ACCESS_DENIED) };
        0
    }

    fn window_refused(_device: RawDevice, _hinstance: HINSTANCE) -> windows::core::Result<HWND> {
        Err(refusal(ERROR_NOT_ENOUGH_MEMORY))
    }

    static LAST_WINDOW: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    fn recorded_window(device: RawDevice, hinstance: HINSTANCE) -> windows::core::Result<HWND> {
        let hwnd = create_message_window(device, hinstance)?;
        LAST_WINDOW.store(hwnd.0 as usize, std::sync::atomic::Ordering::SeqCst);
        Ok(hwnd)
    }

    fn registration_refused(_device: &RAWINPUTDEVICE) -> windows::core::Result<()> {
        Err(refusal(ERROR_INVALID_PARAMETER))
    }

    fn start_with(calls: StartCalls) -> Result<crate::source::Started, CaptureError> {
        let (sink, _writer) = crate::sink::channel(4);
        let mut source = RawInputSource::with_calls(RawDevice::Keyboard, calls);
        let started = source.begin(crate::source::SourceContext::new(sink));
        let _ = source.shutdown();
        started
    }

    fn a_later_start_is_taken() -> bool {
        start_with(SYSTEM_CALLS).is_ok()
    }

    fn removal_refused(_device: &RAWINPUTDEVICE) -> windows::core::Result<()> {
        Err(refusal(ERROR_ACCESS_DENIED))
    }

    #[test]
    fn a_raw_input_source_whose_removal_is_refused_answers_the_os_error_of_the_removal() {
        use crate::source::{SourceState, Started};
        let _serial = crate::windows::tests::serial();
        let (sink, _writer) = crate::sink::channel(4);
        let mut source = RawInputSource::with_calls(
            RawDevice::Keyboard,
            StartCalls {
                remove_device: removal_refused,
                ..SYSTEM_CALLS
            },
        );
        let started = source.begin(crate::source::SourceContext::new(sink.clone()));
        let stopped = source.shutdown();
        let health = source.health().state;
        let again = source.begin(crate::source::SourceContext::new(sink));
        let refused = refused_at(REMOVE_STAGE, ERROR_ACCESS_DENIED);
        assert!(matches!(started, Ok(Started::Running)), "{started:?}");
        assert_eq!(stopped, Err(refused.clone()));
        assert_eq!(
            health,
            SourceState::Failed {
                reason: crate::source::HealthReason::OsError,
                error: refused,
            }
        );
        assert_eq!(again, Err(CaptureError::AlreadyStarted));
        assert!(
            a_later_start_is_taken(),
            "a start after the refused removal was not taken"
        );
    }

    #[test]
    fn a_raw_input_source_whose_module_handle_is_refused_reports_the_step_and_code() {
        let _serial = crate::windows::tests::serial();
        let started = start_with(StartCalls {
            module: no_module,
            ..SYSTEM_CALLS
        });
        let later = a_later_start_is_taken();
        assert_eq!(
            started,
            Err(refused_at("GetModuleHandleW", ERROR_MOD_NOT_FOUND))
        );
        assert!(later, "a start after the refused one was not taken");
    }

    #[test]
    fn a_raw_input_source_whose_class_is_refused_reports_the_step_and_code() {
        let _serial = crate::windows::tests::serial();
        let index = RawDevice::Keyboard.index();
        let before = CLASSES_REGISTERED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)[index];
        CLASSES_REGISTERED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)[index] = false;
        let started = start_with(StartCalls {
            register_class: class_refused,
            ..SYSTEM_CALLS
        });
        let marked_after = CLASSES_REGISTERED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)[index];
        CLASSES_REGISTERED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)[index] = before;
        let later = a_later_start_is_taken();
        assert_eq!(
            started,
            Err(refused_at("RegisterClassW", ERROR_ACCESS_DENIED))
        );
        assert!(!marked_after, "a refused class was marked as registered");
        assert!(later, "a start after the refused one was not taken");
    }

    #[test]
    fn a_raw_input_source_whose_window_is_refused_reports_the_step_and_code() {
        let _serial = crate::windows::tests::serial();
        let started = start_with(StartCalls {
            create_window: window_refused,
            ..SYSTEM_CALLS
        });
        let later = a_later_start_is_taken();
        assert_eq!(
            started,
            Err(refused_at("CreateWindowExW", ERROR_NOT_ENOUGH_MEMORY))
        );
        assert!(later, "a start after the refused one was not taken");
    }

    #[test]
    fn a_raw_input_source_whose_registration_is_refused_reports_the_step_and_code() {
        use windows::Win32::UI::WindowsAndMessaging::IsWindow;
        let _serial = crate::windows::tests::serial();
        LAST_WINDOW.store(0, std::sync::atomic::Ordering::SeqCst);
        let started = start_with(StartCalls {
            create_window: recorded_window,
            register_device: registration_refused,
            ..SYSTEM_CALLS
        });
        let window = LAST_WINDOW.load(std::sync::atomic::Ordering::SeqCst);
        // SAFETY: IsWindow reads a handle value and answers false for one
        // that names no window; it reads no memory of ours.
        let window_left =
            unsafe { IsWindow(Some(HWND(window as *mut core::ffi::c_void))) }.as_bool();
        let later = a_later_start_is_taken();
        assert_eq!(
            started,
            Err(refused_at(
                "RegisterRawInputDevices",
                ERROR_INVALID_PARAMETER
            ))
        );
        assert_ne!(window, 0, "INCONCLUSIVE: the window was not made");
        assert!(!window_left, "the failed start left its window");
        assert!(later, "a start after the refused one was not taken");
    }
}
