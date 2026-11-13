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
use crate::secure_input::{KeystrokeClearance, Withheld};

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

fn mouse_registration(hwnd: HWND) -> RAWINPUTDEVICE {
    RAWINPUTDEVICE {
        usUsagePage: HID_USAGE_PAGE_GENERIC,
        usUsage: RawDevice::Pointer.hid_usage(),
        dwFlags: RIDEV_INPUTSINK,
        hwndTarget: hwnd,
    }
}

fn keyboard_takes_mouse(hwnd: HWND, register: RegisterCall) -> windows::core::Result<()> {
    let mut route = clicks();
    if route.owner.is_some() {
        return Ok(());
    }
    register(&mouse_registration(hwnd))?;
    route.owner = Some(RawDevice::Keyboard);
    Ok(())
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

fn pointer_lets_mouse_go() {
    let mut route = clicks();
    route.owner = None;
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
    late: AtomicU64,
    withheld: AtomicBool,
}

const GATE_POLL_MS: u32 = 15;

fn release_held(gate: &mut GateState, producer: &Producer, at_stop: bool) {
    let now = super::now_tick();
    while let Some((key, _, _)) = gate.held.front() {
        let answer = gate.gate.clearance(key, now);
        if answer.is_none() && !at_stop {
            break;
        }
        let Some((key, stroke, app)) = gate.held.pop_front() else {
            break;
        };
        match answer {
            None => producer.lose(1),
            Some(Ok(clearance)) => {
                producer.push(keystroke_packet(clearance, stroke, app, key.tick()));
            }
            Some(Err(Withheld::Late)) => {
                gate.counts.late.fetch_add(1, Ordering::Relaxed);
            }
            Some(Err(Withheld::Active | Withheld::Unknown)) => {
                gate.counts.withheld.store(true, Ordering::Relaxed);
            }
        }
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
}

const SYSTEM_CALLS: StartCalls = StartCalls {
    module: module_of_the_process,
    register_class: register_window_class,
    create_window: create_message_window,
    register_device: register_raw_input_device,
    remove_device: register_raw_input_device,
};

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
                let held = gate.gate.hold(
                    tick,
                    crate::secure_input::windows::focus_window_is_password_edit(),
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
            let register = clicks().keyboard_register;
            if let Some(register) = register {
                let _ = keyboard_takes_mouse(hwnd, register);
            }
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
            let _ = remove_once(&self.removed, self.device, self.calls.remove_device);
            if self.device == RawDevice::Keyboard {
                let _ = keyboard_lets_mouse_go(self.calls.remove_device);
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
                late_events: self.gate_counts.late.load(Ordering::Relaxed),
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
    _claim: DeviceClaim,
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
    let registered = (calls.register_device)(&dev);
    if let Err(error) = registered {
        ready.report(Err(super::os_error("RegisterRawInputDevices", &error)));
        return;
    }
    let device_guard = DeviceGuard {
        device,
        removed,
        remove: calls.remove_device,
        signal,
        _claim: claim,
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
    } else {
        clicks().owner = Some(RawDevice::Pointer);
    }

    super::ensure_message_queue();
    let poll = if device == RawDevice::Keyboard {
        // SAFETY: SetTimer with no window makes a timer of this thread, whose
        // callback has the TIMERPROC signature; the pump below dispatches it.
        let poll = unsafe {
            windows::Win32::UI::WindowsAndMessaging::SetTimer(
                None,
                0,
                GATE_POLL_MS,
                Some(gate_poll),
            )
        };
        if poll == 0 {
            ready.report(Err(super::last_os_error("SetTimer")));
            return;
        }
        Some(poll)
    } else {
        None
    };
    ready.report(Ok(()));
    super::pump_messages(signal);
    drop(device_guard);
    if device == RawDevice::Keyboard {
        signal.report_removal(
            MOUSE_REMOVE_STAGE,
            keyboard_lets_mouse_go(calls.remove_device),
        );
    } else {
        pointer_lets_mouse_go();
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
        gate.read(crate::secure_input::windows::FieldState::Clear);
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
            let key = gate.hold(1_010, false);
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
            let key = gate.hold(1_010, false);
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
        let (cleared_before, latched, waits_after) = match &gate {
            Some(gate) => {
                let tick = super::super::now_tick();
                gate.read(crate::secure_input::windows::FieldState::Clear);
                let before = gate.hold(tick, false);
                let cleared_before =
                    matches!(gate.clearance(&before, tick.wrapping_add(200)), Some(Ok(_)));
                let latched = latch_on_button_down(
                    gate,
                    &mouse_record(RI_MOUSE_LEFT_BUTTON_DOWN as u16, 0),
                    tick,
                );
                let after = gate.hold(tick.wrapping_add(10), false);
                let waits_after = gate.clearance(&after, tick.wrapping_add(200)).is_none();
                (cleared_before, latched, waits_after)
            }
            None => (false, false, false),
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
        assert!(cleared_before, "a key before the click was not cleared");
        assert!(latched, "the raw button-down did not latch the gate");
        assert!(
            waits_after,
            "a key after the click did not wait for the read"
        );
        assert_eq!(keyboard_stopped, Ok(()));
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
    fn decided_keys_go_out_in_input_order_and_a_key_that_waits_at_the_stop_is_lost() {
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
        let early = now.wrapping_sub(160);
        let later = now.wrapping_sub(150);
        for (tick, app) in [(early, "first"), (later, "second")] {
            let held = state.gate.hold(tick, false);
            state.held.push_back((held, stroke, Some(app.to_string())));
        }
        state.gate.latch(now);
        let waiting = state.gate.hold(now, false);
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
        assert_eq!(handoff.counts().lost, 1);
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

#[cfg(test)]
mod adv2_probes {
    use super::*;
    use crate::secure_input::windows::FieldState;

    fn state_with(gate: PasswordGate) -> GateState {
        GateState {
            gate: Arc::new(gate),
            held: VecDeque::new(),
            counts: Arc::default(),
        }
    }

    #[test]
    #[ignore = "adv2 probe: run alone in the adv2 step"]
    fn adv2_q5_password_keys_handled_late_are_counted_one_by_one() {
        let (sink, writer) = crate::sink::channel(64);
        let handoff = super::super::HandOff::start(sink, "halvo-adv2-forward", Arc::default())
            .expect("hand-off");
        let producer = handoff.producer();
        let stroke = KeyStroke {
            vkey: 0x41,
            released: false,
        };
        let now = super::super::now_tick();

        let slow = PasswordGate::new();
        slow.field(FieldState::Password);
        let mut slow = state_with(slow);
        for i in 0..7u32 {
            let held = slow.gate.hold(now.wrapping_sub(300 + i), false);
            slow.held.push_back((held, stroke, None));
        }
        release_held(&mut slow, &producer, false);

        let latched = PasswordGate::new();
        latched.field(FieldState::Clear);
        let tab = now.wrapping_sub(400);
        latched.latch(tab);
        let mut latched = state_with(latched);
        for i in 0..4u32 {
            let held = latched.gate.hold(tab.wrapping_add(10 + 10 * i), false);
            latched.held.push_back((held, stroke, None));
        }
        latched.gate.field(FieldState::Password);
        release_held(&mut latched, &producer, false);

        let mut delivered = 0;
        while writer
            .recv_timeout(std::time::Duration::from_millis(500))
            .is_ok()
        {
            delivered += 1;
        }
        let line = format!(
            "ADV2 q5 slow_pump password_keys=7 late_events={} keys_withheld={} | behind_latch_with_password_focus_event password_keys=4 late_events={} keys_withheld={} | keystroke_events={delivered}",
            slow.counts.late.load(Ordering::Relaxed),
            slow.counts.withheld.load(Ordering::Relaxed),
            latched.counts.late.load(Ordering::Relaxed),
            latched.counts.withheld.load(Ordering::Relaxed),
        );
        println!("{line}");
        assert_eq!(delivered, 0, "{line}");
        assert_eq!(
            slow.counts.late.load(Ordering::Relaxed),
            0,
            "RD-32: no count of password keys: {line}"
        );
    }
}
