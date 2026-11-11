// Shared helpers of the throwaway probes. Every printed line holds counts, sizes,
// times and booleans only: CI logs of this repository are public.
#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::io::Write;
use std::mem::size_of;

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Security::*;
use windows::Win32::System::Diagnostics::ToolHelp::*;
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX};
use windows::Win32::System::Threading::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Microseconds on the performance counter, which every process of the machine shares.
pub fn now_us() -> i64 {
    let mut c = 0i64;
    let mut f = 0i64;
    unsafe {
        let _ = QueryPerformanceCounter(&mut c);
        let _ = QueryPerformanceFrequency(&mut f);
    }
    ((c as i128) * 1_000_000 / (f.max(1) as i128)) as i64
}

pub fn win32_code(e: &windows::core::Error) -> u32 {
    (e.code().0 as u32) & 0xFFFF
}

pub fn append_line(path: &str, line: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{line}");
        let _ = f.sync_all();
    }
}

#[derive(Clone, Debug)]
pub struct Proc {
    pub pid: u32,
    pub ppid: u32,
    pub exe: String,
}

pub fn snapshot() -> Vec<Proc> {
    let mut v = Vec::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return v;
        };
        let mut e = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        if Process32FirstW(snap, &mut e).is_ok() {
            loop {
                let n = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
                v.push(Proc { pid: e.th32ProcessID, ppid: e.th32ParentProcessID, exe: String::from_utf16_lossy(&e.szExeFile[..n]) });
                if Process32NextW(snap, &mut e).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    v
}

pub struct OwnedHandle(pub HANDLE);
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_invalid() && !self.0 .0.is_null() {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

pub fn open_query(pid: u32) -> Option<OwnedHandle> {
    unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok().map(OwnedHandle) }
}

fn ft(f: FILETIME) -> u64 {
    ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64
}

/// Creation time (100 ns units) and kernel plus user CPU time (100 ns units).
pub fn times(h: HANDLE) -> Option<(u64, u64)> {
    let (mut c, mut e, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
    unsafe { GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u).ok()? };
    Some((ft(c), ft(k) + ft(u)))
}

pub fn creation_time(pid: u32) -> Option<u64> {
    let h = open_query(pid)?;
    times(h.0).map(|t| t.0)
}

/// Working set, peak working set and private bytes.
pub fn memory(h: HANDLE) -> Option<(u64, u64, u64)> {
    let mut m = PROCESS_MEMORY_COUNTERS_EX { cb: size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32, ..Default::default() };
    unsafe {
        GetProcessMemoryInfo(h, &mut m as *mut _ as *mut PROCESS_MEMORY_COUNTERS, size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32).ok()?;
    }
    Some((m.WorkingSetSize as u64, m.PeakWorkingSetSize as u64, m.PrivateUsage as u64))
}

/// The pids under `root` (root included), following parent links whose parent was created
/// no later than the child, so a reused parent pid does not adopt a stranger.
pub fn descendants(root: u32, procs: &[Proc]) -> Vec<u32> {
    let mut kids: HashMap<u32, Vec<u32>> = HashMap::new();
    for p in procs {
        if p.pid != p.ppid {
            kids.entry(p.ppid).or_default().push(p.pid);
        }
    }
    let mut out = vec![root];
    let mut seen: HashSet<u32> = HashSet::from([root]);
    let mut i = 0;
    while i < out.len() {
        let cur = out[i];
        let cur_ct = creation_time(cur);
        if let Some(ks) = kids.get(&cur) {
            for &k in ks {
                if seen.contains(&k) {
                    continue;
                }
                let ok = match (cur_ct, creation_time(k)) {
                    (Some(a), Some(b)) => a <= b,
                    _ => true,
                };
                if ok {
                    seen.insert(k);
                    out.push(k);
                }
            }
        }
        i += 1;
    }
    out
}

pub fn token_of(pid: u32) -> Option<OwnedHandle> {
    let h = open_query(pid)?;
    let mut tok = HANDLE::default();
    unsafe { OpenProcessToken(h.0, TOKEN_QUERY, &mut tok).ok()? };
    Some(OwnedHandle(tok))
}

pub fn token_elevated(tok: HANDLE) -> Option<bool> {
    let mut e = TOKEN_ELEVATION::default();
    let mut n = 0u32;
    unsafe {
        GetTokenInformation(tok, TokenElevation, Some(&mut e as *mut _ as *mut c_void), size_of::<TOKEN_ELEVATION>() as u32, &mut n).ok()?;
    }
    Some(e.TokenIsElevated != 0)
}

pub fn token_integrity_rid(tok: HANDLE) -> Option<u32> {
    let mut buf = vec![0u8; 256];
    let mut n = 0u32;
    unsafe {
        GetTokenInformation(tok, TokenIntegrityLevel, Some(buf.as_mut_ptr() as *mut c_void), buf.len() as u32, &mut n).ok()?;
        let label = &*(buf.as_ptr() as *const TOKEN_MANDATORY_LABEL);
        let sid = label.Label.Sid;
        let count = *GetSidSubAuthorityCount(sid);
        Some(*GetSidSubAuthority(sid, (count as u32).saturating_sub(1)))
    }
}

/// The token user's SID as bytes, for comparison.
pub fn token_user_sid(tok: HANDLE) -> Option<Vec<u8>> {
    let mut buf = vec![0u8; 256];
    let mut n = 0u32;
    unsafe {
        GetTokenInformation(tok, TokenUser, Some(buf.as_mut_ptr() as *mut c_void), buf.len() as u32, &mut n).ok()?;
        let tu = &*(buf.as_ptr() as *const TOKEN_USER);
        let sid = tu.User.Sid;
        let len = GetLengthSid(sid) as usize;
        Some(std::slice::from_raw_parts(sid.0 as *const u8, len).to_vec())
    }
}

pub fn token_user_sid_string(tok: HANDLE) -> Option<String> {
    let mut buf = vec![0u8; 256];
    let mut n = 0u32;
    unsafe {
        GetTokenInformation(tok, TokenUser, Some(buf.as_mut_ptr() as *mut c_void), buf.len() as u32, &mut n).ok()?;
        let tu = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut s = PWSTR::null();
        windows::Win32::Security::Authorization::ConvertSidToStringSidW(tu.User.Sid, &mut s).ok()?;
        let out = s.to_string().ok();
        let _ = LocalFree(Some(HLOCAL(s.0 as *mut c_void)));
        out
    }
}

pub fn self_token() -> Option<OwnedHandle> {
    let mut tok = HANDLE::default();
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut tok).ok()? };
    Some(OwnedHandle(tok))
}

#[derive(Clone, Debug)]
pub struct Win {
    pub hwnd: isize,
    pub pid: u32,
    pub visible: bool,
    pub class: String,
    pub top: bool,
}

unsafe extern "system" fn enum_cb(hwnd: HWND, lp: LPARAM) -> windows::core::BOOL {
    let v = &mut *(lp.0 as *mut Vec<HWND>);
    v.push(hwnd);
    TRUE
}

pub fn class_of(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

pub fn pid_of(hwnd: HWND) -> u32 {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid
}

/// Every top-level window, and with `children` every child window too.
pub fn windows_all(children: bool) -> Vec<Win> {
    let mut tops: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(enum_cb), LPARAM(&mut tops as *mut _ as isize));
    }
    let mut out = Vec::new();
    for &h in &tops {
        out.push(Win { hwnd: h.0 as isize, pid: pid_of(h), visible: unsafe { IsWindowVisible(h).as_bool() }, class: class_of(h), top: true });
        if children {
            let mut kids: Vec<HWND> = Vec::new();
            unsafe {
                let _ = EnumChildWindows(Some(h), Some(enum_cb), LPARAM(&mut kids as *mut _ as isize));
            }
            for k in kids {
                out.push(Win { hwnd: k.0 as isize, pid: pid_of(k), visible: unsafe { IsWindowVisible(k).as_bool() }, class: class_of(k), top: false });
            }
        }
    }
    out
}

pub fn hwnd(v: isize) -> HWND {
    HWND(v as *mut c_void)
}

pub fn window_text(h: HWND) -> String {
    let mut buf = [0u16; 512];
    let n = unsafe { GetWindowTextW(h, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// Start a command line; returns (process handle, pid).
pub fn spawn(cmdline: &str, flags: PROCESS_CREATION_FLAGS) -> windows::core::Result<(OwnedHandle, u32)> {
    let mut cmd = wide(cmdline);
    let si = STARTUPINFOW { cb: size_of::<STARTUPINFOW>() as u32, ..Default::default() };
    let mut pi = PROCESS_INFORMATION::default();
    unsafe {
        CreateProcessW(PCWSTR::null(), Some(PWSTR(cmd.as_mut_ptr())), None, None, false, flags, None, PCWSTR::null(), &si, &mut pi)?;
        let _ = CloseHandle(pi.hThread);
    }
    Ok((OwnedHandle(pi.hProcess), pi.dwProcessId))
}

pub fn wait(h: HANDLE, ms: u32) -> bool {
    unsafe { WaitForSingleObject(h, ms) == WAIT_OBJECT_0 }
}

pub fn exit_code(h: HANDLE) -> Option<u32> {
    let mut c = 0u32;
    unsafe { GetExitCodeProcess(h, &mut c).ok()? };
    Some(c)
}

pub fn terminate(h: HANDLE) -> bool {
    unsafe { TerminateProcess(h, 1).is_ok() }
}

pub fn median(v: &mut [f64]) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

pub fn arg(args: &[String], key: &str) -> Option<String> {
    args.iter().position(|a| a == key).and_then(|i| args.get(i + 1).cloned())
}

pub fn flag(args: &[String], key: &str) -> bool {
    args.iter().any(|a| a == key)
}
