//! Keystroke and pointer capture via Raw Input (`WM_INPUT`).

use std::sync::{Arc, Mutex, PoisonError};

use crate::event::Modality;
use crate::source::{
    CaptureError, CaptureSource, SourceContext, SourceCounters, SourceHealth, Started,
};

use super::{Lifecycle, Packet, Producer, PumpReady};

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
    /// The measured size of this key's decoded payload: the virtual-key code. A
    /// metadata-only event carries this as its placement's `byte_len`, so the
    /// quantity of captured key data is visible before the store holds the bytes.
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
    /// deltas, the button flags and the wheel delta. Carried as the metadata-only
    /// event's placement `byte_len`.
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

pub(crate) fn keystroke_packet(
    key: KeyStroke,
    foreground_app: String,
    tick: u32,
) -> Option<Packet> {
    if key.released {
        return None;
    }
    Some(Packet {
        kind: Modality::Keystroke,
        app: foreground_app,
        byte_len: key.payload_len(),
        tick,
    })
}

pub(crate) fn pointer_packet(sample: PointerSample, foreground_app: String, tick: u32) -> Packet {
    Packet {
        kind: Modality::Pointer,
        app: foreground_app,
        byte_len: sample.payload_len(),
        tick,
    }
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
use windows::Win32::UI::Input::{
    GetRawInputData, RegisterRawInputDevices, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER,
    RIDEV_INPUTSINK, RIDEV_REMOVE, RID_INPUT, RIM_TYPEKEYBOARD, RIM_TYPEMOUSE,
};
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetMessageTime, PostQuitMessage,
    RegisterClassW, HWND_MESSAGE, RI_KEY_BREAK, WINDOW_EX_STYLE, WINDOW_STYLE, WM_DESTROY,
    WM_INPUT, WNDCLASSW,
};

#[cfg(target_os = "windows")]
const HID_USAGE_PAGE_GENERIC: u16 =
    windows::Win32::Devices::HumanInterfaceDevice::HID_USAGE_PAGE_GENERIC;

struct InputState {
    device: RawDevice,
    producer: Producer,
    last_app: Arc<Mutex<Option<String>>>,
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
    register_device: fn(&RAWINPUTDEVICE) -> windows::core::Result<()>,
}

const SYSTEM_CALLS: StartCalls = StartCalls {
    module: module_of_the_process,
    register_class: register_window_class,
    create_window: create_message_window,
    register_device: register_raw_input_device,
};

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

/// Raw Input capture source for one device. `begin` opens a message-only
/// window on its own pump thread and registers the device. One source of each
/// device runs in a process at a time.
pub struct RawInputSource {
    device: RawDevice,
    last_app: Arc<Mutex<Option<String>>>,
    life: Lifecycle,
    calls: StartCalls,
}

impl RawInputSource {
    /// A keyboard source, attributing keystrokes to `last_app` (shared with the
    /// foreground source).
    pub fn keyboard(last_app: Arc<Mutex<Option<String>>>) -> Self {
        Self::new(RawDevice::Keyboard, last_app)
    }

    /// A pointer source.
    pub fn pointer(last_app: Arc<Mutex<Option<String>>>) -> Self {
        Self::new(RawDevice::Pointer, last_app)
    }

    fn new(device: RawDevice, last_app: Arc<Mutex<Option<String>>>) -> Self {
        Self {
            device,
            last_app,
            life: Lifecycle::default(),
            calls: SYSTEM_CALLS,
        }
    }

    #[cfg(test)]
    fn with_calls(device: RawDevice, calls: StartCalls) -> Self {
        Self {
            calls,
            ..Self::new(device, Arc::new(Mutex::new(None)))
        }
    }

    /// The counts of this source's hand-off queues since it was made.
    pub fn handoff_counts(&self) -> super::HandOffCounts {
        self.life.handoff_counts()
    }
}

#[cfg(target_os = "windows")]
fn attributed_app(last_app: &Mutex<Option<String>>) -> String {
    last_app
        .lock()
        .ok()
        .and_then(|g| g.clone())
        .unwrap_or_default()
}

#[cfg(target_os = "windows")]
fn decode_raw_input(bytes: &[u8]) -> Option<RawInputPacket> {
    #[repr(C, align(8))]
    struct Aligned([u8; core::mem::size_of::<RAWINPUT>()]);

    if bytes.len() < core::mem::size_of::<RAWINPUTHEADER>() {
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
        let borrow = cell.borrow();
        let Some(state) = borrow.as_ref() else {
            return;
        };
        let app = attributed_app(&state.last_app);
        match (state.device, packet) {
            (RawDevice::Keyboard, RawInputPacket::Keyboard(key)) => {
                if let Some(packet) = keystroke_packet(key, app, tick) {
                    state.producer.push(packet);
                }
            }
            (RawDevice::Pointer, RawInputPacket::Pointer(sample)) => {
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
    if let Some(packet) = decode_raw_input(&buf) {
        emit_from_raw(packet, tick);
    }
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
        self.life
            .begin(&ctx, "halvo-rawinput-forward", move |producer| {
                let state = InputState {
                    device,
                    producer,
                    last_app,
                };
                let name = format!("halvo-rawinput-{device:?}");
                super::spawn_pump(name, move |ready, signal| {
                    run_raw_input(state, claim, calls, ready, &signal)
                })
            })
    }

    fn shutdown(&mut self) -> Result<(), CaptureError> {
        self.life.shutdown()
    }

    fn health(&self) -> SourceHealth {
        let counters = match self.device {
            RawDevice::Keyboard => SourceCounters::default(),
            RawDevice::Pointer => SourceCounters {
                pointer_coalesced: self.life.handoff_counts().coalesced,
                ..SourceCounters::default()
            },
        };
        self.life.health(counters)
    }
}

struct DeviceGuard {
    usage_page: u16,
    usage: u16,
    _claim: DeviceClaim,
}

impl Drop for DeviceGuard {
    fn drop(&mut self) {
        let remove = RAWINPUTDEVICE {
            usUsagePage: self.usage_page,
            usUsage: self.usage,
            dwFlags: RIDEV_REMOVE,
            hwndTarget: HWND(core::ptr::null_mut()),
        };
        // SAFETY: the slice holds one initialized RAWINPUTDEVICE that removes
        // the registration, and the size argument is the size of that struct.
        unsafe {
            let _ =
                RegisterRawInputDevices(&[remove], core::mem::size_of::<RAWINPUTDEVICE>() as u32);
        }
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
    claim: DeviceClaim,
    calls: StartCalls,
    ready: PumpReady,
    signal: &super::StopSignal,
) {
    let device = state.device;
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
    let _ctx_guard = CtxGuard;

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
    let _device_guard = DeviceGuard {
        usage_page: HID_USAGE_PAGE_GENERIC,
        usage: device.hid_usage(),
        _claim: claim,
    };

    super::ensure_message_queue();
    ready.report(Ok(()));
    super::pump_messages(signal);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_down_maps_to_a_keystroke_packet() {
        let key = KeyStroke {
            vkey: 0x41,
            released: false,
        };
        let packet = keystroke_packet(key, "C:\\editor.exe".to_string(), 7).unwrap();
        assert_eq!(packet.kind, Modality::Keystroke);
        assert_eq!(packet.app, "C:\\editor.exe");
        assert_eq!(packet.byte_len, 2);
        assert_eq!(packet.tick, 7);
    }

    #[test]
    fn a_key_up_is_not_recorded() {
        let key = KeyStroke {
            vkey: 0x41,
            released: true,
        };
        assert!(keystroke_packet(key, String::new(), 7).is_none());
    }

    #[test]
    fn a_pointer_sample_maps_to_a_pointer_packet_and_only_a_flagless_one_is_motion() {
        let packet = pointer_packet(PointerSample::default(), "C:\\game.exe".to_string(), 9);
        assert_eq!(packet.kind, Modality::Pointer);
        assert_eq!(packet.app, "C:\\game.exe");
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

    #[test]
    fn a_keyboard_source_refuses_a_second_begin_and_starts_again_after_its_stop() {
        use crate::source::{SourceContext, SourceState, Started};
        let _serial = crate::windows::tests::serial();
        let (sink, _writer) = crate::sink::channel(64);
        let mut source = RawInputSource::keyboard(Arc::new(Mutex::new(None)));
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
        let mut first = RawInputSource::keyboard(Arc::new(Mutex::new(None)));
        let mut second = RawInputSource::keyboard(Arc::new(Mutex::new(None)));
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
