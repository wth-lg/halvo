//! Keystroke and pointer capture via Raw Input (`WM_INPUT`).
//!
//! Raw Input with `RIDEV_INPUTSINK` delivers background keyboard and mouse data
//! to a message-only window as `WM_INPUT` messages (researcher-1 #15). It is a
//! listen-only path: it cannot read from a higher-integrity process (the UIPI
//! boundary) and the secure desktop (UAC prompts, the logon screen) runs where no
//! user-level app can hook it, so credential entry is withheld by the OS, not by
//! Halvo policy. Halvo adds no lower-level hook and no bypass.
//!
//! One source handles one device (keyboard or pointer) so it has one
//! [`crate::event::Modality`]. The decode from a raw packet to a small owned value
//! and the mapping from that value to a stream event are pure and unit tested; the
//! window, device registration and message pump are compiled and type-checked here
//! and exercised on a real desktop (functional capture is deferred).

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use crate::event::{CaptureStreamEvent, Modality, RetentionClass};
use crate::sink::SinkHandle;
use crate::source::{CaptureError, CaptureSource, EventSink};

use super::{build_event, EventPlacement, PlacementSink};

/// `RI_KEY_BREAK` (winuser.h): set in a raw keyboard packet's flags on key-up.
const RI_KEY_BREAK: u16 = 0x01;
/// Raw input header device types (winuser.h): mouse is 0, keyboard is 1.
const RIM_TYPE_MOUSE: u32 = 0;
const RIM_TYPE_KEYBOARD: u32 = 1;

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

    /// The HID usage (page 0x01) this device registers for: keyboard 0x06, mouse
    /// 0x02 (USB HID usage tables).
    fn hid_usage(self) -> u16 {
        match self {
            RawDevice::Keyboard => 0x06,
            RawDevice::Pointer => 0x02,
        }
    }
}

/// A decoded keyboard packet: the virtual-key code and whether this is a key-up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyStroke {
    pub vkey: u16,
    pub released: bool,
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

/// Map a decoded keystroke to a stream event, or `None` to skip it. Only key-down
/// transitions are recorded, so one physical press is one event, not two.
pub fn map_keystroke(
    sink: &SinkHandle,
    key: KeyStroke,
    foreground_app: String,
    placement: EventPlacement,
) -> Option<CaptureStreamEvent> {
    if key.released {
        return None;
    }
    Some(build_event(
        sink,
        Modality::Keystroke,
        foreground_app,
        placement,
    ))
}

/// Map a decoded pointer sample to a stream event. Every sample is recorded; the
/// payload (movement and buttons) is placed in the store by the caller.
pub fn map_pointer(
    sink: &SinkHandle,
    _sample: PointerSample,
    foreground_app: String,
    placement: EventPlacement,
) -> CaptureStreamEvent {
    build_event(sink, Modality::Pointer, foreground_app, placement)
}

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
#[cfg(target_os = "windows")]
use windows::Win32::UI::Input::{
    GetRawInputData, RegisterRawInputDevices, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE,
    RAWINPUTDEVICE_FLAGS, RAWINPUTHEADER, RID_INPUT,
};
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, PostQuitMessage, RegisterClassW,
    UnregisterClassW, HWND_MESSAGE, WINDOW_EX_STYLE, WINDOW_STYLE, WM_DESTROY, WM_INPUT, WNDCLASSW,
};

/// `RIDEV_INPUTSINK` (winuser.h): receive input even when not the foreground
/// window — the whole point of a background capture.
#[cfg(target_os = "windows")]
const RIDEV_INPUTSINK: u32 = 0x0000_0100;
/// `RIDEV_REMOVE` (winuser.h): stop receiving input for a device.
#[cfg(target_os = "windows")]
const RIDEV_REMOVE: u32 = 0x0000_0001;
/// HID usage page 0x01 (generic desktop) covers keyboard and mouse.
#[cfg(target_os = "windows")]
const HID_USAGE_PAGE_GENERIC: u16 = 0x01;

/// The per-thread emit context a running raw-input window reads in its window
/// procedure.
#[cfg(target_os = "windows")]
struct InputState {
    device: RawDevice,
    sink: SinkHandle,
    placement: Arc<dyn PlacementSink>,
    last_app: Arc<Mutex<String>>,
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
    placement: Arc<dyn PlacementSink>,
    last_app: Arc<Mutex<String>>,
    thread_id: Arc<AtomicU32>,
    pump: Option<JoinHandle<()>>,
}

impl RawInputSource {
    /// A keyboard source, attributing keystrokes to `last_app` (shared with the
    /// foreground source).
    pub fn keyboard(placement: Arc<dyn PlacementSink>, last_app: Arc<Mutex<String>>) -> Self {
        Self::new(RawDevice::Keyboard, placement, last_app)
    }

    /// A pointer source.
    pub fn pointer(placement: Arc<dyn PlacementSink>, last_app: Arc<Mutex<String>>) -> Self {
        Self::new(RawDevice::Pointer, placement, last_app)
    }

    fn new(
        device: RawDevice,
        placement: Arc<dyn PlacementSink>,
        last_app: Arc<Mutex<String>>,
    ) -> Self {
        Self {
            device,
            placement,
            last_app,
            thread_id: Arc::new(AtomicU32::new(0)),
            pump: None,
        }
    }
}

/// The attributed foreground app for an input event: the last app the foreground
/// hook saw, or an empty string before it has seen one.
#[cfg(target_os = "windows")]
fn attributed_app(last_app: &Mutex<String>) -> String {
    last_app.lock().map(|g| g.clone()).unwrap_or_default()
}

#[cfg(target_os = "windows")]
fn emit_from_raw(raw: &RAWINPUT) {
    INPUT_CTX.with(|cell| {
        let borrow = cell.borrow();
        let Some(state) = borrow.as_ref() else {
            return;
        };
        let app = attributed_app(&state.last_app);
        match (state.device, raw.header.dwType) {
            (RawDevice::Keyboard, RIM_TYPE_KEYBOARD) => {
                let kb = unsafe { raw.data.keyboard };
                let key = KeyStroke {
                    vkey: kb.VKey,
                    released: kb.Flags & RI_KEY_BREAK != 0,
                };
                if let Ok(placement) = state.placement.place(&[], RetentionClass::Raw) {
                    if let Some(event) = map_keystroke(&state.sink, key, app, placement) {
                        let _ = state.sink.emit(event);
                    }
                }
            }
            (RawDevice::Pointer, RIM_TYPE_MOUSE) => {
                let m = unsafe { raw.data.mouse };
                let sample = PointerSample {
                    last_x: m.lLastX,
                    last_y: m.lLastY,
                    button_flags: unsafe { m.Anonymous.Anonymous.usButtonFlags },
                    wheel_delta: unsafe { m.Anonymous.Anonymous.usButtonData } as i16,
                };
                if let Ok(placement) = state.placement.place(&[], RetentionClass::Raw) {
                    let event = map_pointer(&state.sink, sample, app, placement);
                    let _ = state.sink.emit(event);
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
            read_raw_input(HRAWINPUT(lparam.0 as *mut core::ffi::c_void));
            // WM_INPUT still needs default processing for system cleanup.
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Read the raw-input packet behind a `WM_INPUT` and emit its event.
#[cfg(target_os = "windows")]
fn read_raw_input(handle: HRAWINPUT) {
    let header_size = core::mem::size_of::<RAWINPUTHEADER>() as u32;
    let mut size: u32 = 0;
    // First call sizes the buffer.
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
    // The buffer begins with a RAWINPUT; it was sized to hold one.
    let raw = unsafe { &*(buf.as_ptr() as *const RAWINPUT) };
    emit_from_raw(raw);
}

impl CaptureSource for RawInputSource {
    fn modality(&self) -> Modality {
        self.device.modality()
    }

    fn start(&mut self, sink: SinkHandle) -> Result<(), CaptureError> {
        let state = InputState {
            device: self.device,
            sink,
            placement: Arc::clone(&self.placement),
            last_app: Arc::clone(&self.last_app),
        };
        let thread_id = Arc::clone(&self.thread_id);
        let pump = std::thread::Builder::new()
            .name(format!("halvo-rawinput-{:?}", self.device))
            .spawn(move || run_raw_input(state, &thread_id))
            .map_err(|e| CaptureError::Start(format!("raw-input pump thread: {e}")))?;
        self.pump = Some(pump);
        Ok(())
    }

    fn stop(&mut self) {
        super::post_quit_to(self.thread_id.load(Ordering::SeqCst));
        if let Some(pump) = self.pump.take() {
            let _ = pump.join();
        }
    }
}

/// Open a message-only window, register the device for background raw input, pump
/// until `WM_QUIT`, then unregister and destroy the window. Runs on the source's
/// pump thread.
#[cfg(target_os = "windows")]
fn run_raw_input(state: InputState, thread_id: &AtomicU32) {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;

    // Record the pump thread id first, so a stop() that races start() can always
    // post WM_QUIT rather than join a thread that never learned its own id.
    thread_id.store(super::current_thread_id(), Ordering::SeqCst);

    let device = state.device;
    let class_name = HSTRING::from(match device {
        RawDevice::Keyboard => "HalvoRawKeyboard",
        RawDevice::Pointer => "HalvoRawPointer",
    });

    let hinstance = match unsafe { GetModuleHandleW(None) } {
        Ok(h) => h,
        Err(_) => return,
    };

    let class = WNDCLASSW {
        lpfnWndProc: Some(wnd_proc),
        hInstance: hinstance.into(),
        lpszClassName: PCWSTR(class_name.as_ptr()),
        ..Default::default()
    };
    let atom = unsafe { RegisterClassW(&class) };
    if atom == 0 {
        return;
    }

    let hwnd = unsafe {
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
            Some(hinstance.into()),
            None,
        )
    };
    let hwnd = match hwnd {
        Ok(h) => h,
        Err(_) => {
            unsafe {
                let _ = UnregisterClassW(PCWSTR(class_name.as_ptr()), Some(hinstance.into()));
            }
            return;
        }
    };

    INPUT_CTX.with(|cell| *cell.borrow_mut() = Some(state));

    let dev = RAWINPUTDEVICE {
        usUsagePage: HID_USAGE_PAGE_GENERIC,
        usUsage: device.hid_usage(),
        dwFlags: RAWINPUTDEVICE_FLAGS(RIDEV_INPUTSINK),
        hwndTarget: hwnd,
    };
    let registered = unsafe {
        RegisterRawInputDevices(&[dev], core::mem::size_of::<RAWINPUTDEVICE>() as u32).is_ok()
    };

    if registered {
        super::pump_messages();
        // Unregister the device before the window goes away.
        let remove = RAWINPUTDEVICE {
            usUsagePage: HID_USAGE_PAGE_GENERIC,
            usUsage: device.hid_usage(),
            dwFlags: RAWINPUTDEVICE_FLAGS(RIDEV_REMOVE),
            hwndTarget: HWND(core::ptr::null_mut()),
        };
        unsafe {
            let _ =
                RegisterRawInputDevices(&[remove], core::mem::size_of::<RAWINPUTDEVICE>() as u32);
        }
    }

    unsafe {
        let _ = DestroyWindow(hwnd);
        let _ = UnregisterClassW(PCWSTR(class_name.as_ptr()), Some(hinstance.into()));
    }
    INPUT_CTX.with(|cell| *cell.borrow_mut() = None);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::windows::tests::test_placement;

    #[test]
    fn a_key_down_maps_to_a_keystroke_event() {
        let (sink, _w) = crate::sink::channel(4);
        let key = KeyStroke {
            vkey: 0x41,
            released: false,
        };
        let event =
            map_keystroke(&sink, key, "C:\\editor.exe".to_string(), test_placement()).unwrap();
        assert_eq!(event.modality, Modality::Keystroke);
        assert_eq!(event.foreground_app, "C:\\editor.exe");
    }

    #[test]
    fn a_key_up_is_not_recorded() {
        let (sink, _w) = crate::sink::channel(4);
        let key = KeyStroke {
            vkey: 0x41,
            released: true,
        };
        assert!(map_keystroke(&sink, key, String::new(), test_placement()).is_none());
    }

    #[test]
    fn a_pointer_sample_maps_to_a_pointer_event() {
        let (sink, _w) = crate::sink::channel(4);
        let sample = PointerSample {
            last_x: 3,
            last_y: -2,
            button_flags: 0,
            wheel_delta: 0,
        };
        let event = map_pointer(&sink, sample, "C:\\game.exe".to_string(), test_placement());
        assert_eq!(event.modality, Modality::Pointer);
        assert_eq!(event.foreground_app, "C:\\game.exe");
    }

    #[test]
    fn keyboard_and_pointer_devices_carry_their_modality() {
        assert_eq!(RawDevice::Keyboard.modality(), Modality::Keystroke);
        assert_eq!(RawDevice::Pointer.modality(), Modality::Pointer);
    }
}
