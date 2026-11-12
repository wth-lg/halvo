//! Keystroke and pointer capture via Raw Input (`WM_INPUT`).

use std::sync::{Arc, Mutex};

use crate::event::Modality;
use crate::sink::SinkHandle;
use crate::source::{CaptureError, CaptureSource};

use super::{HandOff, Packet, Producer, PumpHandle, PumpReady};

/// Which device a source listens to; also its modality.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawDevice {
    Keyboard,
    Pointer,
}

impl RawDevice {
    fn modality(self) -> Modality {
        match self {
            RawDevice::Keyboard => Modality::Keystroke,
            RawDevice::Pointer => Modality::Pointer,
        }
    }

    #[cfg(target_os = "windows")]
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

/// The packet of a decoded key, or `None` for a key-up: one press is one event.
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

/// A decoded raw input packet: either a keyboard or a mouse record.
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
    RegisterClassW, UnregisterClassW, HWND_MESSAGE, RI_KEY_BREAK, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_DESTROY, WM_INPUT, WNDCLASSW,
};

#[cfg(target_os = "windows")]
const HID_USAGE_PAGE_GENERIC: u16 =
    windows::Win32::Devices::HumanInterfaceDevice::HID_USAGE_PAGE_GENERIC;

#[cfg(target_os = "windows")]
struct InputState {
    device: RawDevice,
    producer: Producer,
    last_app: Arc<Mutex<Option<String>>>,
}

#[cfg(target_os = "windows")]
thread_local! {
    static INPUT_CTX: std::cell::RefCell<Option<InputState>> =
        const { std::cell::RefCell::new(None) };
}

/// Raw Input capture source for one device. `start` opens a message-only window on
/// its own pump thread and registers the device; `stop` quits the pump and tears
/// the window down.
pub struct RawInputSource {
    device: RawDevice,
    last_app: Arc<Mutex<Option<String>>>,
    pump: Option<PumpHandle>,
    handoff: Option<HandOff>,
}

impl RawInputSource {
    /// The counts of this source's hand-off queue in its last run.
    pub fn handoff_counts(&self) -> super::HandOffCounts {
        self.handoff
            .as_ref()
            .map(HandOff::counts)
            .unwrap_or_default()
    }

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
            pump: None,
            handoff: None,
        }
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

/// Copy `bytes` into an 8-aligned, full-size `RAWINPUT` so no field is read
/// through a misaligned or under-sized reference, then decode the keyboard or
/// mouse record. Returns `None` for any other record or a buffer too short to
/// hold the header.
#[cfg(target_os = "windows")]
fn decode_raw_input(bytes: &[u8]) -> Option<RawInputPacket> {
    #[repr(C, align(8))]
    struct Aligned([u8; core::mem::size_of::<RAWINPUT>()]);

    if bytes.len() < core::mem::size_of::<RAWINPUTHEADER>() {
        return None;
    }
    // RAWINPUT is a repr(C) plain-data type (integers and unions, no niches), so
    // any byte pattern is a valid value and zero-padding past `bytes` is sound.
    let mut aligned = Aligned([0u8; core::mem::size_of::<RAWINPUT>()]);
    let n = bytes.len().min(aligned.0.len());
    aligned.0[..n].copy_from_slice(&bytes[..n]);
    // SAFETY: `aligned` is align(8) >= align_of::<RAWINPUT>(), is exactly
    // size_of::<RAWINPUT>() bytes, and is fully initialized.
    let raw: &RAWINPUT = unsafe { &*(aligned.0.as_ptr() as *const RAWINPUT) };

    if raw.header.dwType == RIM_TYPEKEYBOARD.0 {
        let kb = unsafe { raw.data.keyboard };
        Some(RawInputPacket::Keyboard(KeyStroke {
            vkey: kb.VKey,
            released: kb.Flags & RI_KEY_BREAK as u16 != 0,
        }))
    } else if raw.header.dwType == RIM_TYPEMOUSE.0 {
        let m = unsafe { raw.data.mouse };
        Some(RawInputPacket::Pointer(PointerSample {
            last_x: m.lLastX,
            last_y: m.lLastY,
            button_flags: unsafe { m.Anonymous.Anonymous.usButtonFlags },
            wheel_delta: unsafe { m.Anonymous.Anonymous.usButtonData } as i16,
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
    // GetRawInputData with pData NULL writes the required size to pcbSize and
    // returns 0: the only way to size the buffer for a variable-length packet.
    let probe = unsafe { GetRawInputData(handle, RID_INPUT, None, &mut size, header_size) };
    if probe != 0 || size == 0 {
        return;
    }
    let mut buf = vec![0u8; size as usize];
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
    fn modality(&self) -> Modality {
        self.device.modality()
    }

    fn start(&mut self, sink: SinkHandle) -> Result<(), CaptureError> {
        let handoff = HandOff::start(sink, "halvo-rawinput-forward")?;
        let state = InputState {
            device: self.device,
            producer: handoff.producer(),
            last_app: Arc::clone(&self.last_app),
        };
        let name = format!("halvo-rawinput-{:?}", self.device);
        let pump = super::spawn_pump(name, move |ready, signal| {
            run_raw_input(state, ready, &signal)
        })?;
        self.pump = Some(pump);
        self.handoff = Some(handoff);
        Ok(())
    }

    fn stop(&mut self) {
        if let Some(mut pump) = self.pump.take() {
            let _ = pump.stop(super::INPUT_STOP_DEADLINE);
        }
        if let Some(handoff) = &self.handoff {
            handoff.close();
        }
    }
}

/// Unregisters the raw input device on drop, so a scope exit or early return
/// tears registration down in order rather than relying on prose.
#[cfg(target_os = "windows")]
struct DeviceGuard {
    usage_page: u16,
    usage: u16,
}

#[cfg(target_os = "windows")]
impl Drop for DeviceGuard {
    fn drop(&mut self) {
        let remove = RAWINPUTDEVICE {
            usUsagePage: self.usage_page,
            usUsage: self.usage,
            dwFlags: RIDEV_REMOVE,
            hwndTarget: HWND(core::ptr::null_mut()),
        };
        unsafe {
            let _ =
                RegisterRawInputDevices(&[remove], core::mem::size_of::<RAWINPUTDEVICE>() as u32);
        }
    }
}

/// Clears the thread-local input context on drop, after the device is gone.
#[cfg(target_os = "windows")]
struct CtxGuard;

#[cfg(target_os = "windows")]
impl Drop for CtxGuard {
    fn drop(&mut self) {
        INPUT_CTX.with(|cell| *cell.borrow_mut() = None);
    }
}

/// Destroys the message-only window on drop.
#[cfg(target_os = "windows")]
struct WindowGuard {
    hwnd: HWND,
}

#[cfg(target_os = "windows")]
impl Drop for WindowGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

/// Unregisters the window class on drop (last, after the window is destroyed).
#[cfg(target_os = "windows")]
struct ClassGuard {
    class_name: windows::core::HSTRING,
    hinstance: HINSTANCE,
}

#[cfg(target_os = "windows")]
impl Drop for ClassGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = UnregisterClassW(
                windows::core::PCWSTR(self.class_name.as_ptr()),
                Some(self.hinstance),
            );
        }
    }
}

#[cfg(target_os = "windows")]
fn run_raw_input(state: InputState, ready: PumpReady, signal: &super::StopSignal) {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;

    let device = state.device;
    let class_name = HSTRING::from(match device {
        RawDevice::Keyboard => "HalvoRawKeyboard",
        RawDevice::Pointer => "HalvoRawPointer",
    });

    // SAFETY: GetModuleHandleW with no name reads the handle of this process's
    // executable and takes no pointer.
    let hinstance: HINSTANCE = match unsafe { GetModuleHandleW(None) } {
        Ok(h) => h.into(),
        Err(error) => {
            ready.report(Err(super::os_error("GetModuleHandleW", &error)));
            return;
        }
    };

    let class = WNDCLASSW {
        lpfnWndProc: Some(wnd_proc),
        hInstance: hinstance,
        lpszClassName: PCWSTR(class_name.as_ptr()),
        ..Default::default()
    };
    // SAFETY: `class` names a live wide string and a window procedure with the
    // WNDPROC signature, both of which outlive the call.
    let atom = unsafe { RegisterClassW(&class) };
    if atom == 0 {
        ready.report(Err(super::last_os_error("RegisterClassW")));
        return;
    }
    // Teardown runs in reverse declaration order: device, context, window, class.
    let _class_guard = ClassGuard {
        class_name: class_name.clone(),
        hinstance,
    };

    let hwnd = match unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(class_name.as_ptr()),
            PCWSTR(class_name.as_ptr()),
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
    } {
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
    // SAFETY: the slice holds one initialized RAWINPUTDEVICE whose target is
    // the window above, and the size argument is the size of that struct.
    let registered =
        unsafe { RegisterRawInputDevices(&[dev], core::mem::size_of::<RAWINPUTDEVICE>() as u32) };
    if let Err(error) = registered {
        ready.report(Err(super::os_error("RegisterRawInputDevices", &error)));
        return;
    }
    let _device_guard = DeviceGuard {
        usage_page: HID_USAGE_PAGE_GENERIC,
        usage: device.hid_usage(),
    };

    // The window exists, so the thread has a message queue; force it in and report
    // readiness, then a stop() that races in posts WM_QUIT to a queue that keeps it.
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
    fn keyboard_and_pointer_devices_carry_their_modality() {
        assert_eq!(RawDevice::Keyboard.modality(), Modality::Keystroke);
        assert_eq!(RawDevice::Pointer.modality(), Modality::Pointer);
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
        let bytes = unsafe {
            core::slice::from_raw_parts(
                &raw as *const RAWINPUT as *const u8,
                core::mem::size_of::<RAWINPUT>(),
            )
        };
        // Lay the bytes down at an odd offset so the source slice is misaligned
        // for RAWINPUT (align 8); the decoder must still read it soundly.
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
    fn a_second_keyboard_source_names_the_step_that_failed() {
        let _serial = crate::windows::tests::serial();
        let (sink, _writer) = crate::sink::channel(64);
        let mut first = RawInputSource::keyboard(Arc::new(Mutex::new(None)));
        let mut second = RawInputSource::keyboard(Arc::new(Mutex::new(None)));
        let first_started = first.begin(crate::source::SourceContext::new(sink.clone()));
        let second_started = second.begin(crate::source::SourceContext::new(sink));
        let _ = second.shutdown();
        let _ = first.shutdown();
        assert!(first_started.is_ok(), "{first_started:?}");
        assert!(
            matches!(
                second_started,
                Err(CaptureError::Os {
                    stage: "RegisterClassW",
                    domain: "HRESULT",
                    ..
                })
            ),
            "{second_started:?}"
        );
    }
}
