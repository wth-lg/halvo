// S5 stand-in of the app: a GUI-subsystem program that shows no window, starts the stand-in
// daemon as its child with an inherited pipe read end as standard input (explicit handle
// list, CREATE_NO_WINDOW), and with `--form job` also holds a kill-on-close job around it.
// It keeps a hidden top-level window so that session-end messages can reach it.
#![windows_subsystem = "windows"]

#[path = "util.rs"]
mod util;

use std::ffi::c_void;
use std::mem::size_of;
use std::sync::OnceLock;

use util::*;
use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Security::SECURITY_ATTRIBUTES;
use windows::Win32::Storage::FileSystem::*;
use windows::Win32::System::JobObjects::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Pipes::CreatePipe;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::WindowsAndMessaging::*;

static MARKER: OnceLock<String> = OnceLock::new();

unsafe extern "system" fn wndproc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_QUERYENDSESSION => {
            append_line(MARKER.get().unwrap(), &format!("starter_event=queryendsession t_us={}", now_us()));
            LRESULT(1)
        }
        WM_ENDSESSION => {
            append_line(MARKER.get().unwrap(), &format!("starter_event=endsession ending={} t_us={}", wp.0 != 0, now_us()));
            LRESULT(0)
        }
        _ => DefWindowProcW(h, msg, wp, lp),
    }
}

fn inheritable_nul() -> HANDLE {
    let sa = SECURITY_ATTRIBUTES { nLength: size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: std::ptr::null_mut(), bInheritHandle: TRUE };
    unsafe {
        CreateFileW(w!("NUL"), (FILE_GENERIC_READ | FILE_GENERIC_WRITE).0, FILE_SHARE_READ | FILE_SHARE_WRITE, Some(&sa), OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, None)
            .unwrap_or_default()
    }
}

/// Start `cmdline` inheriting exactly `inherit`, with stdin = `stdin`.
fn spawn_with_handles(cmdline: &str, stdin: HANDLE, out: HANDLE, inherit: &[HANDLE], flags: PROCESS_CREATION_FLAGS) -> windows::core::Result<PROCESS_INFORMATION> {
    unsafe {
        let mut size = 0usize;
        let _ = InitializeProcThreadAttributeList(None, 1, None, &mut size);
        let mut buf = vec![0u8; size];
        let list = LPPROC_THREAD_ATTRIBUTE_LIST(buf.as_mut_ptr() as *mut c_void);
        InitializeProcThreadAttributeList(Some(list), 1, None, &mut size)?;
        UpdateProcThreadAttribute(list, 0, PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize, Some(inherit.as_ptr() as *const c_void), std::mem::size_of_val(inherit), None, None)?;
        let mut si = STARTUPINFOEXW::default();
        si.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        si.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        si.StartupInfo.hStdInput = stdin;
        si.StartupInfo.hStdOutput = out;
        si.StartupInfo.hStdError = out;
        si.lpAttributeList = list;
        let mut cmd = wide(cmdline);
        let mut pi = PROCESS_INFORMATION::default();
        let r = CreateProcessW(PCWSTR::null(), Some(PWSTR(cmd.as_mut_ptr())), None, None, true, EXTENDED_STARTUPINFO_PRESENT | CREATE_NO_WINDOW | flags, None, PCWSTR::null(), &si.StartupInfo, &mut pi);
        DeleteProcThreadAttributeList(list);
        r.map(|_| pi)
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let child = arg(&args, "--child").unwrap_or_default();
    let marker = arg(&args, "--marker").unwrap_or_default();
    let ready = arg(&args, "--ready").unwrap_or_default();
    let form = arg(&args, "--form").unwrap_or_else(|| "handle".into());
    let flush_ms = arg(&args, "--flush-ms").unwrap_or_else(|| "200".into());
    let leak = flag(&args, "--leak");
    let background = flag(&args, "--background");
    let _ = MARKER.set(marker.clone());
    append_line(&marker, &format!("starter_event=start background={background} form={form} leak={leak} t_us={}", now_us()));

    unsafe {
        let sa = SECURITY_ATTRIBUTES { nLength: size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: std::ptr::null_mut(), bInheritHandle: TRUE };
        let (mut r, mut wr) = (HANDLE::default(), HANDLE::default());
        CreatePipe(&mut r, &mut wr, Some(&sa), 0).expect("pipe");
        // The write end stays in this process only (it is not in the child's handle list).
        let _ = SetHandleInformation(wr, HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0));
        let nul = inheritable_nul();
        let me = std::process::id();
        let cmd = format!("\"{child}\" child --parent-pid {me} --marker \"{marker}\" --flush-ms {flush_ms}");
        let job = if form == "job" {
            let j = CreateJobObjectW(None, PCWSTR::null()).expect("job");
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(j, JobObjectExtendedLimitInformation, &info as *const _ as *const c_void, size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32).expect("job limit");
            Some(j)
        } else {
            None
        };
        let flags = if job.is_some() { CREATE_SUSPENDED } else { PROCESS_CREATION_FLAGS(0) };
        let pi = spawn_with_handles(&cmd, r, nul, &[r, nul], flags).expect("child");
        if let Some(j) = job {
            let assigned = AssignProcessToJobObject(j, pi.hProcess).is_ok();
            append_line(&marker, &format!("starter_event=job assigned={assigned}"));
            ResumeThread(pi.hThread);
        }
        let mut leak_pid = 0u32;
        if leak {
            // A second process that also holds the write end, as a leaked handle would.
            let _ = SetHandleInformation(wr, HANDLE_FLAG_INHERIT.0, HANDLE_FLAG_INHERIT);
            let lcmd = format!("\"{child}\" sleep 60");
            if let Ok(lp) = spawn_with_handles(&lcmd, nul, nul, &[nul, wr], PROCESS_CREATION_FLAGS(0)) {
                leak_pid = lp.dwProcessId;
                let _ = CloseHandle(lp.hThread);
                let _ = CloseHandle(lp.hProcess);
            }
            let _ = SetHandleInformation(wr, HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0));
        }
        let _ = CloseHandle(r);
        let _ = CloseHandle(pi.hThread);

        let hinst = GetModuleHandleW(None).unwrap_or_default();
        let wc = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: hinst.into(), lpszClassName: w!("R1SpikeStarterEnd"), ..Default::default() };
        RegisterClassW(&wc);
        let win = CreateWindowExW(WINDOW_EX_STYLE(0), w!("R1SpikeStarterEnd"), w!(""), WS_OVERLAPPEDWINDOW, 0, 0, 0, 0, None, None, Some(hinst.into()), None);
        let hwnd_val = win.map(|h| h.0 as isize).unwrap_or(0);
        append_line(&ready, &format!("ready child_pid={} leak_pid={leak_pid} starter_hwnd={hwnd_val} t_us={}", pi.dwProcessId, now_us()));
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let _ = wr;
    }
}
