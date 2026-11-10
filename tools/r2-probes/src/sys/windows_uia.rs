use core::{ffi::c_void, mem::size_of};
use windows::core::{BOOL, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, FILETIME, HWND, LPARAM, RPC_E_CHANGED_MODE};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::System::Diagnostics::ToolHelp::*;
use windows::Win32::System::ProcessStatus::*;
use windows::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub fn uia() -> Result<IUIAutomation, String> {
    unsafe {
        let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
        if hr.is_err() && hr != RPC_E_CHANGED_MODE {
            return Err(format!("CoInitializeEx: {hr:?}"));
        }
        CoCreateInstance::<_, IUIAutomation>(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)
            .or_else(|_| {
                CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
            })
            .map_err(|e| format!("CoCreateInstance: {e}"))
    }
}
pub fn element_from_hwnd(a: &IUIAutomation, hwnd: isize) -> Result<IUIAutomationElement, String> {
    unsafe { a.ElementFromHandle(HWND(hwnd as *mut c_void)) }
        .map_err(|e| format!("ElementFromHandle: {e}"))
}
pub fn focused(a: &IUIAutomation) -> Result<IUIAutomationElement, String> {
    unsafe { a.GetFocusedElement() }.map_err(|e| format!("GetFocusedElement: {e}"))
}
pub fn name(e: &IUIAutomationElement) -> String {
    unsafe { e.CurrentName() }
        .map(|b| b.to_string())
        .unwrap_or_default()
}
pub fn value(e: &IUIAutomationElement) -> Option<String> {
    unsafe {
        let p: IUIAutomationValuePattern = e.GetCurrentPatternAs(UIA_ValuePatternId).ok()?;
        p.CurrentValue().ok().map(|b| b.to_string())
    }
}
pub fn class_name(e: &IUIAutomationElement) -> String {
    unsafe { e.CurrentClassName() }
        .map(|b| b.to_string())
        .unwrap_or_default()
}
pub fn control_type(e: &IUIAutomationElement) -> i32 {
    unsafe { e.CurrentControlType() }.map(|t| t.0).unwrap_or(0)
}
pub fn automation_id(e: &IUIAutomationElement) -> String {
    unsafe { e.CurrentAutomationId() }
        .map(|b| b.to_string())
        .unwrap_or_default()
}
pub fn children(a: &IUIAutomation, e: &IUIAutomationElement) -> Vec<IUIAutomationElement> {
    let mut out = Vec::new();
    unsafe {
        let Ok(walker) = a.ControlViewWalker() else {
            return out;
        };
        let mut next = walker.GetFirstChildElement(e).ok();
        while let Some(child) = next {
            next = walker.GetNextSiblingElement(&child).ok();
            out.push(child);
        }
    }
    out
}
pub fn help_text(e: &IUIAutomationElement) -> String {
    unsafe { e.CurrentHelpText() }
        .map(|b| b.to_string())
        .unwrap_or_default()
}
struct EnumCtx {
    pid: u32,
    out: Vec<(isize, u32, String, String)>,
}
unsafe extern "system" fn enum_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let ctx = unsafe { &mut *(lparam.0 as *mut EnumCtx) };
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if (ctx.pid == 0 || pid == ctx.pid) && unsafe { IsWindowVisible(hwnd) }.as_bool() {
        let (mut t, mut c) = ([0u16; 512], [0u16; 256]);
        let n = unsafe { GetWindowTextW(hwnd, &mut t) }.max(0) as usize;
        let m = unsafe { GetClassNameW(hwnd, &mut c) }.max(0) as usize;
        ctx.out.push((
            hwnd.0 as isize,
            pid,
            String::from_utf16_lossy(&t[..n]),
            String::from_utf16_lossy(&c[..m]),
        ));
    }
    BOOL(1)
}
pub fn top_windows(pid: u32) -> Vec<(isize, u32, String, String)> {
    let mut ctx = EnumCtx {
        pid,
        out: Vec::new(),
    };
    let _ = unsafe { EnumWindows(Some(enum_cb), LPARAM(&mut ctx as *mut EnumCtx as isize)) };
    ctx.out
}
fn current_mode() -> Option<DEVMODEW> {
    let mut dm = DEVMODEW {
        dmSize: size_of::<DEVMODEW>() as u16,
        ..Default::default()
    };
    unsafe { EnumDisplaySettingsW(PCWSTR::null(), ENUM_CURRENT_SETTINGS, &mut dm) }
        .as_bool()
        .then_some(dm)
}
pub fn display_size() -> (u32, u32) {
    match current_mode() {
        Some(dm) => (dm.dmPelsWidth, dm.dmPelsHeight),
        None => unsafe {
            (
                GetSystemMetrics(SM_CXSCREEN).max(0) as u32,
                GetSystemMetrics(SM_CYSCREEN).max(0) as u32,
            )
        },
    }
}
pub fn set_display_size(w: u32, h: u32) -> Result<(u32, u32), String> {
    let mut dm = current_mode().ok_or("EnumDisplaySettingsW failed")?;
    (dm.dmPelsWidth, dm.dmPelsHeight, dm.dmFields) = (w, h, DM_PELSWIDTH | DM_PELSHEIGHT);
    let r = unsafe { ChangeDisplaySettingsExW(PCWSTR::null(), Some(&dm), None, CDS_TYPE(0), None) };
    if r != DISP_CHANGE_SUCCESSFUL {
        return Err(format!(
            "ChangeDisplaySettingsExW({w}x{h}): DISP_CHANGE {}",
            r.0
        ));
    }
    Ok(display_size())
}
pub fn process_tree(root_pid: u32) -> Vec<u32> {
    let mut pairs: Vec<(u32, u32)> = Vec::new();
    unsafe {
        if let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            let mut pe = PROCESSENTRY32W {
                dwSize: size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            let mut more = Process32FirstW(snap, &mut pe).is_ok();
            while more {
                pairs.push((pe.th32ProcessID, pe.th32ParentProcessID));
                more = Process32NextW(snap, &mut pe).is_ok();
            }
            let _ = CloseHandle(snap);
        }
    }
    let (mut out, mut i) = (vec![root_pid], 0);
    while i < out.len() {
        for &(pid, ppid) in &pairs {
            if ppid == out[i] && pid != 0 && !out.contains(&pid) {
                out.push(pid);
            }
        }
        i += 1;
    }
    out
}
pub struct ProcessUsage {
    pub cpu_ms: f64,
    pub private_bytes: u64,
    pub peak_pagefile_bytes: u64,
}
pub fn process_usage(pid: u32) -> Option<ProcessUsage> {
    let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let ticks = |f: FILETIME| (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime);
    let (mut c, mut e, mut k, mut u) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    let mut m = PROCESS_MEMORY_COUNTERS_EX {
        cb: size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        ..Default::default()
    };
    let ok = unsafe {
        let t = GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u).is_ok();
        let r = K32GetProcessMemoryInfo(
            h,
            &mut m as *mut PROCESS_MEMORY_COUNTERS_EX as *mut PROCESS_MEMORY_COUNTERS,
            m.cb,
        );
        let _ = CloseHandle(h);
        t && r.as_bool()
    };
    ok.then(|| ProcessUsage {
        cpu_ms: (ticks(k) + ticks(u)) as f64 / 10_000.0,
        private_bytes: m.PrivateUsage as u64,
        peak_pagefile_bytes: m.PeakPagefileUsage as u64,
    })
}
