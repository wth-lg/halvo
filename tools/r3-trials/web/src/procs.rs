use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proc {
    pub pid: u32,
    pub ppid: u32,
    pub started_us: u64,
    pub exe: Option<PathBuf>,
}

pub fn descendants(table: &[Proc], roots: &[u32]) -> Vec<u32> {
    let mut tree: Vec<u32> = Vec::new();
    for r in roots {
        if !tree.contains(r) {
            tree.push(*r);
        }
    }
    let mut i = 0;
    while i < tree.len() {
        let parent = tree[i];
        for p in table {
            if p.ppid == parent && p.pid != parent && !tree.contains(&p.pid) {
                tree.push(p.pid);
            }
        }
        i += 1;
    }
    tree
}

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

pub fn adopted(table: &[Proc], tree: &[u32], since_us: u64, roots: &[PathBuf]) -> Vec<u32> {
    table
        .iter()
        .filter(|p| p.ppid == 1 && !tree.contains(&p.pid) && p.started_us >= since_us)
        .filter(|p| {
            p.exe.as_ref().is_some_and(|e| {
                roots
                    .iter()
                    .any(|r| !r.as_os_str().is_empty() && e.starts_with(r))
            })
        })
        .map(|p| p.pid)
        .collect()
}

#[cfg(target_os = "macos")]
pub fn table() -> std::io::Result<Vec<Proc>> {
    use std::os::unix::ffi::OsStringExt;
    let needed = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    if needed <= 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut pids: Vec<libc::pid_t> = vec![0; needed as usize + 512];
    let bytes = (pids.len() * std::mem::size_of::<libc::pid_t>()) as libc::c_int;
    let got = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
    if got <= 0 {
        return Err(std::io::Error::last_os_error());
    }
    pids.truncate(got as usize);
    let mut out = Vec::with_capacity(pids.len());
    for pid in pids.into_iter().filter(|p| *p > 0) {
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
        let r = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDTBSDINFO,
                0,
                (&mut info as *mut libc::proc_bsdinfo).cast(),
                size,
            )
        };
        if r != size {
            continue;
        }
        let mut buf = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
        let n = unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
        let exe = (n > 0).then(|| {
            buf.truncate(n as usize);
            PathBuf::from(std::ffi::OsString::from_vec(buf))
        });
        out.push(Proc {
            pid: pid as u32,
            ppid: info.pbi_ppid,
            started_us: info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec,
            exe,
        });
    }
    Ok(out)
}

#[cfg(windows)]
pub fn table() -> std::io::Result<Vec<Proc>> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snap == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error());
    }
    let mut out = Vec::new();
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
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
            started_us: 0,
            exe: Some(PathBuf::from(String::from_utf16_lossy(
                &entry.szExeFile[..len],
            ))),
        });
        ok = unsafe { Process32NextW(snap, &mut entry) };
    }
    unsafe { CloseHandle(snap) };
    Ok(out)
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn table() -> std::io::Result<Vec<Proc>> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "the process table is read on macOS and Windows only",
    ))
}

#[cfg(unix)]
pub fn alive(pid: u32) -> bool {
    let r = unsafe { libc_kill(pid as i32, 0) };
    r == 0 || std::io::Error::last_os_error().raw_os_error() == Some(1)
}

#[cfg(unix)]
pub fn kill(pid: u32) {
    unsafe { libc_kill(pid as i32, 9) };
}

#[cfg(unix)]
extern "C" {
    #[link_name = "kill"]
    fn libc_kill(pid: i32, sig: i32) -> i32;
}

#[cfg(windows)]
pub fn alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if h.is_null() {
        return false;
    }
    let mut code = 0u32;
    let ok = unsafe { GetExitCodeProcess(h, &mut code) };
    unsafe { CloseHandle(h) };
    ok != 0 && code == STILL_ACTIVE as u32
}

#[cfg(windows)]
pub fn kill(pid: u32) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
    let h = unsafe { OpenProcess(PROCESS_TERMINATE, 0, pid) };
    if !h.is_null() {
        unsafe {
            TerminateProcess(h, 1);
            CloseHandle(h);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pid: u32, ppid: u32, started_us: u64, exe: &str) -> Proc {
        Proc {
            pid,
            ppid,
            started_us,
            exe: Some(PathBuf::from(exe)),
        }
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
        let mut d = descendants(&t, &[10]);
        d.sort_unstable();
        assert_eq!(d, vec![10, 11, 12, 14]);
        assert_eq!(descendants(&t, &[13]), vec![13]);
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
        assert_eq!(adopted(&t, &[20, 24], 100, &roots), vec![21]);
    }

    #[cfg(unix)]
    #[test]
    fn table_sees_a_grandchild_and_kill_ends_it() {
        let mut child = std::process::Command::new("/bin/sh")
            .args(["-c", "/bin/sleep 30 & /bin/sleep 30; wait"])
            .spawn()
            .unwrap();
        let root = child.id();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let tree = loop {
            let tree = descendants(&table().unwrap(), &[root]);
            if tree.len() >= 3 || std::time::Instant::now() > deadline {
                break tree;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        assert_eq!(tree.len(), 3, "{tree:?}");
        assert!(tree.iter().all(|pid| alive(*pid)));
        for pid in &tree {
            kill(*pid);
        }
        let _ = child.wait();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while tree.iter().any(|pid| alive(*pid)) && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(tree.iter().all(|pid| !alive(*pid)), "{tree:?}");
    }

    #[cfg(windows)]
    #[test]
    fn table_sees_a_child_of_cmd_and_kill_ends_it() {
        let mut child = std::process::Command::new("cmd")
            .args(["/C", "ping -n 30 127.0.0.1 >NUL"])
            .spawn()
            .unwrap();
        let root = child.id();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let tree = loop {
            let tree = descendants(&table().unwrap(), &[root]);
            if tree.len() >= 2 || std::time::Instant::now() > deadline {
                break tree;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        assert!(tree.len() >= 2, "{tree:?}");
        for pid in &tree {
            kill(*pid);
        }
        let _ = child.wait();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while tree.iter().any(|pid| alive(*pid)) && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(tree.iter().all(|pid| !alive(*pid)), "{tree:?}");
    }
}
