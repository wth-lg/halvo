use std::path::{Component, Path, PathBuf};

/// One row of the process table: the pid, the parent pid, the creation time in microseconds since
/// the Unix epoch (0 when the system does not give it) and the executable where it is readable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proc {
    pub pid: u32,
    pub ppid: u32,
    pub started_us: u64,
    pub exe: Option<PathBuf>,
}

/// Whether two rows name one process: the same pid and the same creation time.
pub fn same(a: &Proc, b: &Proc) -> bool {
    a.pid == b.pid && a.started_us == b.started_us
}

/// The roots and every process of `table` that descends from them. A process is a child of a member
/// only when the member is in `table` with its own creation time, which is known (not 0), and the
/// process's parent pid is the member's pid and its creation time is known and at or after the
/// member's. So a process whose parent pid is stale, whose parent's pid was reused, or whose parent
/// has exited does not enter, and neither does any process below a member whose creation time is
/// unknown. Roots come first.
pub fn descendants(table: &[Proc], roots: &[Proc]) -> Vec<Proc> {
    let mut tree: Vec<Proc> = Vec::new();
    for r in roots {
        if !tree.iter().any(|t| same(t, r)) {
            tree.push(r.clone());
        }
    }
    let mut i = 0;
    while i < tree.len() {
        let parent = tree[i].clone();
        i += 1;
        if parent.started_us == 0 || !table.iter().any(|p| same(p, &parent)) {
            continue;
        }
        for p in table {
            let child = p.ppid == parent.pid
                && p.pid != parent.pid
                && p.started_us != 0
                && p.started_us >= parent.started_us;
            if child && !tree.iter().any(|t| t.pid == p.pid) {
                tree.push(p.clone());
            }
        }
    }
    tree
}

/// The `.app` bundle that holds `exe` (the path up to its first component that ends in `.app`), or
/// else the directory of `exe`.
pub fn bundle_root(exe: &Path) -> PathBuf {
    let mut root = PathBuf::new();
    for c in exe.components() {
        root.push(c.as_os_str());
        if let Component::Normal(name) = c {
            if name.to_string_lossy().ends_with(".app") {
                return root;
            }
        }
    }
    exe.parent().map(Path::to_path_buf).unwrap_or_default()
}

/// The processes of `table` outside `tree` whose parent pid is 1, which started at or after
/// `since_us` and whose executable lies under one of `roots`.
pub fn adopted(table: &[Proc], tree: &[Proc], since_us: u64, roots: &[PathBuf]) -> Vec<Proc> {
    table
        .iter()
        .filter(|p| p.ppid == 1 && !tree.iter().any(|t| same(t, p)) && p.started_us >= since_us)
        .filter(|p| {
            p.exe.as_ref().is_some_and(|e| {
                roots
                    .iter()
                    .any(|r| !r.as_os_str().is_empty() && e.starts_with(r))
            })
        })
        .cloned()
        .collect()
}

#[cfg(target_os = "macos")]
const SZOMB: u32 = 5;

#[cfg(target_os = "macos")]
fn bsd_info(pid: u32) -> Option<libc::proc_bsdinfo> {
    // SAFETY: proc_bsdinfo is a C struct of integers and byte arrays, for which all-zero bytes are a
    // valid value.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    // SAFETY: the buffer is `info`, a live proc_bsdinfo owned here, and `size` is its size in bytes,
    // so the call writes inside it.
    let r = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size,
        )
    };
    (r == size).then_some(info)
}

#[cfg(target_os = "macos")]
fn started_of(info: &libc::proc_bsdinfo) -> u64 {
    info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec
}

/// Every process of the system that it can read, with its parent pid, its creation time and its
/// executable: on macOS from `proc_listallpids` and `proc_pidinfo`; on Windows from a Toolhelp
/// snapshot and `GetProcessTimes`, where a process that cannot be opened has creation time 0. Other
/// systems get an error.
#[cfg(target_os = "macos")]
pub fn table() -> std::io::Result<Vec<Proc>> {
    use std::os::unix::ffi::OsStringExt;
    // SAFETY: a null buffer of size 0 asks for the count alone; the call writes nothing.
    let needed = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    if needed <= 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut pids: Vec<libc::pid_t> = vec![0; needed as usize + 512];
    let bytes = (pids.len() * std::mem::size_of::<libc::pid_t>()) as libc::c_int;
    // SAFETY: `bytes` is the size in bytes of `pids`, a live vector owned here, so the call writes
    // inside it.
    let got = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
    if got <= 0 {
        return Err(std::io::Error::last_os_error());
    }
    pids.truncate(got as usize);
    let mut out = Vec::with_capacity(pids.len());
    for pid in pids.into_iter().filter(|p| *p > 0) {
        let Some(info) = bsd_info(pid as u32) else {
            continue;
        };
        let mut buf = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
        // SAFETY: `buf` is a live vector of `buf.len()` bytes, the size that the call is told, so it
        // writes inside it.
        let n = unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
        let exe = (n > 0).then(|| {
            buf.truncate(n as usize);
            PathBuf::from(std::ffi::OsString::from_vec(buf))
        });
        out.push(Proc {
            pid: pid as u32,
            ppid: info.pbi_ppid,
            started_us: started_of(&info),
            exe,
        });
    }
    Ok(out)
}

/// Whether the process of `p` still runs: its pid exists, still has the creation time of `p`, and
/// has not exited (on macOS it is no zombie). A pid that another process took over is not running.
#[cfg(target_os = "macos")]
pub fn running(p: &Proc) -> bool {
    bsd_info(p.pid).is_some_and(|i| started_of(&i) == p.started_us && i.pbi_status != SZOMB)
}

/// Ends the process of `p` at once (SIGKILL on macOS, `TerminateProcess` on Windows) after reading
/// its creation time again, and only when that is the creation time of `p`. Returns whether it sent
/// the kill. On Windows the read and the kill go through one handle, which keeps the pid from being
/// reused between them.
#[cfg(target_os = "macos")]
pub fn kill(p: &Proc) -> bool {
    if !bsd_info(p.pid).is_some_and(|i| started_of(&i) == p.started_us) {
        return false;
    }
    // SAFETY: kill with a pid and a signal number has no memory effect on this process; the pid was
    // read back just above with the creation time of `p`.
    unsafe { libc::kill(p.pid as libc::c_int, libc::SIGKILL) == 0 }
}

#[cfg(target_os = "macos")]
pub fn ask_end(p: &Proc) -> bool {
    if !bsd_info(p.pid).is_some_and(|i| started_of(&i) == p.started_us) {
        return false;
    }
    // SAFETY: kill with a pid and a signal number has no memory effect on this process; the pid was
    // read back just above with the creation time of `p`.
    unsafe { libc::kill(p.pid as libc::c_int, libc::SIGTERM) == 0 }
}

#[cfg(not(target_os = "macos"))]
pub fn ask_end(_p: &Proc) -> bool {
    false
}

pub fn own() -> Option<Proc> {
    table()
        .ok()?
        .into_iter()
        .find(|p| p.pid == std::process::id())
}

#[cfg(windows)]
fn open(pid: u32, access: u32) -> Option<windows_sys::Win32::Foundation::HANDLE> {
    // SAFETY: OpenProcess takes plain values and returns a new handle or null, which is checked.
    let h = unsafe { windows_sys::Win32::System::Threading::OpenProcess(access, 0, pid) };
    (!h.is_null()).then_some(h)
}

#[cfg(windows)]
fn created_us(h: windows_sys::Win32::Foundation::HANDLE) -> Option<u64> {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::GetProcessTimes;
    let zero = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
    // SAFETY: `h` is an open process handle of the caller, and the four pointers are to live
    // FILETIME values owned here.
    let ok = unsafe { GetProcessTimes(h, &mut created, &mut exited, &mut kernel, &mut user) };
    if ok == 0 {
        return None;
    }
    let ticks = ((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64;
    ticks.checked_sub(116_444_736_000_000_000).map(|t| t / 10)
}

#[cfg(windows)]
fn started_of_pid(pid: u32) -> u64 {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::PROCESS_QUERY_LIMITED_INFORMATION;
    let Some(h) = open(pid, PROCESS_QUERY_LIMITED_INFORMATION) else {
        return 0;
    };
    let t = created_us(h);
    // SAFETY: `h` was opened above, is closed once here and is not used after.
    unsafe { CloseHandle(h) };
    t.unwrap_or(0)
}

/// Every process of the system that it can read, with its parent pid, its creation time and its
/// executable: on macOS from `proc_listallpids` and `proc_pidinfo`; on Windows from a Toolhelp
/// snapshot and `GetProcessTimes`, where a process that cannot be opened has creation time 0. Other
/// systems get an error.
#[cfg(windows)]
pub fn table() -> std::io::Result<Vec<Proc>> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    // SAFETY: the call takes plain values; its handle is checked against INVALID_HANDLE_VALUE.
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snap == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error());
    }
    let mut out = Vec::new();
    // SAFETY: PROCESSENTRY32W is a C struct of integers and a u16 array, for which all-zero bytes
    // are a valid value; its dwSize is set next.
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
    // SAFETY: `snap` is the open snapshot, and `entry` is a live PROCESSENTRY32W whose dwSize is
    // its size.
    let mut ok = unsafe { Process32FirstW(snap, &mut entry) };
    while ok != 0 {
        let len = entry
            .szExeFile
            .iter()
            .position(|c| *c == 0)
            .unwrap_or(entry.szExeFile.len());
        out.push(Proc {
            pid: entry.th32ProcessID,
            ppid: entry.th32ParentProcessID,
            started_us: started_of_pid(entry.th32ProcessID),
            exe: Some(PathBuf::from(String::from_utf16_lossy(
                &entry.szExeFile[..len],
            ))),
        });
        // SAFETY: as for Process32FirstW: the open snapshot and the same live entry.
        ok = unsafe { Process32NextW(snap, &mut entry) };
    }
    // SAFETY: `snap` was opened above, is closed once here and is not used after.
    unsafe { CloseHandle(snap) };
    Ok(out)
}

/// Whether the process of `p` still runs: its pid exists, still has the creation time of `p`, and
/// has not exited (on macOS it is no zombie). A pid that another process took over is not running.
#[cfg(windows)]
pub fn running(p: &Proc) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    let Some(h) = open(p.pid, PROCESS_QUERY_LIMITED_INFORMATION) else {
        return false;
    };
    let same_process = created_us(h) == Some(p.started_us);
    let mut code = 0u32;
    // SAFETY: `h` is the handle that `open` gave above, and `code` is a live u32 owned here.
    let ok = unsafe { GetExitCodeProcess(h, &mut code) };
    // SAFETY: `h` was opened above, is closed once here and is not used after.
    unsafe { CloseHandle(h) };
    same_process && ok != 0 && code == STILL_ACTIVE as u32
}

/// Ends the process of `p` at once (SIGKILL on macOS, `TerminateProcess` on Windows) after reading
/// its creation time again, and only when that is the creation time of `p`. Returns whether it sent
/// the kill. On Windows the read and the kill go through one handle, which keeps the pid from being
/// reused between them.
#[cfg(windows)]
pub fn kill(p: &Proc) -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        TerminateProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
    };
    let Some(h) = open(p.pid, PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION) else {
        return false;
    };
    let same_process = created_us(h) == Some(p.started_us);
    let killed = same_process
        // SAFETY: `h` was opened with PROCESS_TERMINATE, and its creation time, read through the
        // same handle just above, is that of `p`, so the call ends that process and no other.
        && unsafe { TerminateProcess(h, 1) } != 0;
    // SAFETY: `h` was opened above, is closed once here and is not used after.
    unsafe { CloseHandle(h) };
    killed
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn table() -> std::io::Result<Vec<Proc>> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "the process table is read on macOS and Windows only",
    ))
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn running(_p: &Proc) -> bool {
    false
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn kill(_p: &Proc) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn ask_end_leaves_a_process_whose_start_time_differs() {
        use crate::start::Start;
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("5")
            .start()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(200));
        let row = table()
            .unwrap()
            .into_iter()
            .find(|p| p.pid == child.id())
            .unwrap();
        let other = Proc {
            started_us: row.started_us + 1,
            ..row.clone()
        };
        let sent = ask_end(&other);
        std::thread::sleep(std::time::Duration::from_millis(200));
        let alive = matches!(child.try_wait(), Ok(None));
        let asked = ask_end(&row);
        let _ = child.wait();
        assert!(!sent && alive, "the ask reached a process of another start");
        assert!(asked, "the ask did not reach the process of its own start");
    }
    use crate::start::Start;

    fn p(pid: u32, ppid: u32, started_us: u64, exe: &str) -> Proc {
        Proc {
            pid,
            ppid,
            started_us,
            exe: Some(PathBuf::from(exe)),
        }
    }

    fn pids(tree: &[Proc]) -> Vec<u32> {
        let mut v: Vec<u32> = tree.iter().map(|p| p.pid).collect();
        v.sort_unstable();
        v
    }

    #[test]
    fn descendants_walk_every_generation_once() {
        let t = vec![
            p(1, 0, 0, "/sbin/launchd"),
            p(10, 1, 5, "/x/run"),
            p(11, 10, 6, "/x/child"),
            p(12, 11, 7, "/x/grandchild"),
            p(13, 1, 8, "/x/other"),
            p(14, 12, 9, "/x/great"),
        ];
        assert_eq!(
            pids(&descendants(&t, &[t[1].clone()])),
            vec![10, 11, 12, 14]
        );
        assert_eq!(pids(&descendants(&t, &[t[4].clone()])), vec![13]);
    }

    #[test]
    fn a_stale_parent_pid_brings_no_foreign_process_into_the_tree() {
        let t = vec![
            p(500, 9, 1_000, "/b/browser"),
            p(501, 500, 1_100, "/b/helper"),
            p(777, 501, 50, "/other/program"),
            p(778, 777, 60, "/other/child"),
        ];
        assert_eq!(pids(&descendants(&t, &[t[0].clone()])), vec![500, 501]);
    }

    #[test]
    fn a_reused_pid_is_neither_walked_nor_taken_for_the_recorded_process() {
        let recorded = vec![
            p(500, 9, 1_000, "/b/browser"),
            p(501, 500, 1_100, "/b/helper"),
        ];
        let now = vec![
            p(500, 9, 1_000, "/b/browser"),
            p(501, 3, 5_000, "/other/program"),
            p(779, 501, 5_100, "/other/child"),
        ];
        let tree = descendants(&now, &recorded);
        assert_eq!(pids(&tree), vec![500, 501]);
        assert!(tree.iter().all(|m| recorded.iter().any(|r| same(r, m))));
        assert_eq!(pids(&descendants(&now, &[now[0].clone()])), vec![500]);
    }

    #[test]
    fn a_member_that_exited_or_has_no_creation_time_takes_no_child() {
        let recorded = vec![
            p(500, 9, 1_000, "/b/browser"),
            p(600, 500, 1_100, "/b/helper"),
        ];
        let now = vec![
            p(500, 9, 1_000, "/b/browser"),
            p(701, 600, 5_200, "/other/child-of-a-reuser-that-exited"),
        ];
        assert_eq!(pids(&descendants(&now, &recorded)), vec![500, 600]);
        let unknown = vec![
            p(500, 9, 0, "/b/browser"),
            p(777, 500, 50, "/other/program"),
            p(778, 777, 60, "/other/child"),
        ];
        assert_eq!(
            pids(&descendants(&unknown, &[unknown[0].clone()])),
            vec![500]
        );
    }

    #[test]
    fn bundle_root_is_the_app_or_the_directory() {
        assert_eq!(
            bundle_root(Path::new(
                "/Applications/Google Chrome.app/Contents/Frameworks/F.framework/Helpers/chrome_crashpad_handler"
            )),
            PathBuf::from("/Applications/Google Chrome.app")
        );
        assert_eq!(
            bundle_root(Path::new("/opt/chrome/chrome")),
            PathBuf::from("/opt/chrome")
        );
    }

    #[test]
    fn adopted_takes_late_orphans_of_the_same_bundle_only() {
        let app = "/Applications/Google Chrome.app";
        let t = vec![
            p(1, 0, 0, "/sbin/launchd"),
            p(20, 9, 100, &format!("{app}/Contents/MacOS/Google Chrome")),
            p(
                21,
                1,
                101,
                &format!("{app}/Contents/Frameworks/H/chrome_crashpad_handler"),
            ),
            p(
                22,
                1,
                50,
                &format!("{app}/Contents/Frameworks/H/chrome_crashpad_handler"),
            ),
            p(23, 1, 102, "/Applications/Other.app/Contents/MacOS/Other"),
            p(24, 20, 103, &format!("{app}/Contents/Frameworks/H/Helper")),
        ];
        let roots = vec![PathBuf::from(app)];
        let tree = vec![t[1].clone(), t[5].clone()];
        assert_eq!(pids(&adopted(&t, &tree, 100, &roots)), vec![21]);
    }

    fn found(pid: u32) -> Option<Proc> {
        table().unwrap().into_iter().find(|p| p.pid == pid)
    }

    fn wait_for(cond: impl Fn() -> bool) -> bool {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !cond() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        cond()
    }

    #[cfg(any(target_os = "macos", windows))]
    #[test]
    fn kill_reads_the_creation_time_again_and_acts_only_on_the_recorded_process() {
        let mut child = if cfg!(windows) {
            std::process::Command::new("ping")
                .args(["-n", "30", "127.0.0.1"])
                .stdout(std::process::Stdio::null())
                .start()
                .unwrap()
        } else {
            std::process::Command::new("/bin/sleep")
                .arg("30")
                .start()
                .unwrap()
        };
        let real = found(child.id()).expect("the child is in the table");
        assert!(real.started_us > 0, "{real:?}");
        let other = Proc {
            started_us: real.started_us + 1,
            ..real.clone()
        };
        assert!(!running(&other));
        assert!(!kill(&other));
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(running(&real));
        assert!(child.try_wait().unwrap().is_none());
        assert!(kill(&real));
        let _ = child.wait();
        assert!(wait_for(|| !running(&real)));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn table_sees_a_grandchild_and_kill_ends_it() {
        let mut child = std::process::Command::new("/bin/sh")
            .args(["-c", "/bin/sleep 30 & /bin/sleep 30; wait"])
            .start()
            .unwrap();
        let root = child.id();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let tree = loop {
            let table = table().unwrap();
            let tree = table
                .iter()
                .find(|p| p.pid == root)
                .map(|r| descendants(&table, std::slice::from_ref(r)))
                .unwrap_or_default();
            if tree.len() >= 3 || std::time::Instant::now() > deadline {
                break tree;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        assert_eq!(tree.len(), 3, "{tree:?}");
        assert!(tree.iter().all(running));
        for p in &tree {
            assert!(kill(p));
        }
        let _ = child.wait();
        assert!(wait_for(|| !tree.iter().any(running)), "{tree:?}");
    }

    #[cfg(windows)]
    #[test]
    fn table_sees_a_child_of_cmd_and_kill_ends_it() {
        let mut child = std::process::Command::new("cmd")
            .args(["/C", "ping -n 30 127.0.0.1 >NUL"])
            .start()
            .unwrap();
        let root = child.id();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let tree = loop {
            let table = table().unwrap();
            let tree = table
                .iter()
                .find(|p| p.pid == root)
                .map(|r| descendants(&table, std::slice::from_ref(r)))
                .unwrap_or_default();
            if tree.len() >= 2 || std::time::Instant::now() > deadline {
                break tree;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        assert!(tree.len() >= 2, "{tree:?}");
        for p in &tree {
            assert!(kill(p));
        }
        let _ = child.wait();
        assert!(wait_for(|| !tree.iter().any(running)), "{tree:?}");
    }
}
