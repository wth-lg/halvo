use std::{
    mem::size_of,
    path::Path,
    time::{Duration, Instant},
};
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::{
    GetRawInputData, RegisterRawInputDevices, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER,
    RIDEV_INPUTSINK, RID_INPUT, RIM_TYPEKEYBOARD, RIM_TYPEMOUSE,
};
use windows::Win32::UI::WindowsAndMessaging::*;

unsafe extern "system" fn wndproc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(h, m, w, l) }
}

pub fn count_raw_input(
    duration_ms: u64,
    ready_file: &Path,
    stop_file: &Path,
) -> Result<[u32; 4], String> {
    let class = w!("HalvoR2RawInput");
    let inst = unsafe { GetModuleHandleW(None) }.map_err(|e| e.to_string())?;
    let wc = WNDCLASSW {
        lpfnWndProc: Some(wndproc),
        hInstance: inst.into(),
        lpszClassName: class,
        ..Default::default()
    };
    if unsafe { RegisterClassW(&wc) } == 0 {
        return Err(format!(
            "RegisterClassW: {}",
            windows::core::Error::from_thread()
        ));
    }
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            class,
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(inst.into()),
            None,
        )
    }
    .map_err(|e| e.to_string())?;
    let dev = |usage| RAWINPUTDEVICE {
        usUsagePage: 1,
        usUsage: usage,
        dwFlags: RIDEV_INPUTSINK,
        hwndTarget: hwnd,
    };
    unsafe { RegisterRawInputDevices(&[dev(6), dev(2)], size_of::<RAWINPUTDEVICE>() as u32) }
        .map_err(|e| e.to_string())?;
    std::fs::write(ready_file, b"ready\n").map_err(|e| e.to_string())?;
    let (mut n, deadline) = (
        [0u32; 4],
        Instant::now() + Duration::from_millis(duration_ms),
    );
    loop {
        let mut msg = MSG::default();
        while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
            if msg.message == WM_INPUT {
                count(msg.lParam, &mut n);
            }
            unsafe { DispatchMessageW(&msg) };
        }
        let left = deadline
            .saturating_duration_since(Instant::now())
            .as_millis() as u32;
        if left == 0 || stop_file.exists() {
            break;
        }
        unsafe { MsgWaitForMultipleObjects(None, false, left.min(50), QS_ALLINPUT) };
    }
    unsafe {
        let _ = DestroyWindow(hwnd);
    }
    Ok(n)
}

fn count(lparam: LPARAM, n: &mut [u32; 4]) {
    let header = size_of::<RAWINPUTHEADER>() as u32;
    let mut buf = [0u64; 16];
    let mut size = size_of::<[u64; 16]>() as u32;
    let got = unsafe {
        GetRawInputData(
            HRAWINPUT(lparam.0 as _),
            RID_INPUT,
            Some(buf.as_mut_ptr().cast()),
            &mut size,
            header,
        )
    };
    if got == u32::MAX || got < header {
        return;
    }
    let raw = unsafe { &*buf.as_ptr().cast::<RAWINPUT>() };
    let no_device = raw.header.hDevice.is_invalid() as u32;
    if raw.header.dwType == RIM_TYPEKEYBOARD.0 {
        let k = unsafe { raw.data.keyboard };
        if u32::from(k.Flags) & RI_KEY_BREAK == 0
            && (k.Message == WM_KEYDOWN || k.Message == WM_SYSKEYDOWN)
        {
            n[0] += 1;
            n[2] += no_device;
        }
    } else if raw.header.dwType == RIM_TYPEMOUSE.0 {
        let flags = u32::from(unsafe { raw.data.mouse.Anonymous.Anonymous.usButtonFlags });
        for bit in [
            RI_MOUSE_LEFT_BUTTON_DOWN,
            RI_MOUSE_RIGHT_BUTTON_DOWN,
            RI_MOUSE_MIDDLE_BUTTON_DOWN,
        ] {
            if flags & bit != 0 {
                n[1] += 1;
                n[3] += no_device;
            }
        }
    }
}

pub fn send_os_input(x: i32, y: i32, pause_ms: u64) -> Result<(i32, i32), String> {
    use enigo::{
        Button, Coordinate,
        Direction::{Press, Release},
        Enigo, Key, Keyboard, Mouse, Settings,
    };
    let pause = || std::thread::sleep(Duration::from_millis(pause_ms));
    let e = |err: enigo::InputError| err.to_string();
    let settings = Settings {
        windows_dw_extra_info: Some(0x4856_4C52),
        ..Settings::default()
    };
    let mut en = Enigo::new(&settings).map_err(|err| err.to_string())?;
    en.move_mouse(x, y, Coordinate::Abs).map_err(e)?;
    pause();
    let mut pointer = windows::Win32::Foundation::POINT::default();
    let _ = unsafe { GetCursorPos(&mut pointer) };
    for _ in 0..5 {
        for d in [Press, Release] {
            en.button(Button::Left, d).map_err(e)?;
            pause();
        }
    }
    for _ in 0..20 {
        for d in [Press, Release] {
            en.key(Key::A, d).map_err(e)?;
            pause();
        }
    }
    Ok((pointer.x, pointer.y))
}
