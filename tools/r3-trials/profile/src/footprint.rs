use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus};
use std::time::{Duration, Instant};

pub const PERIOD: Duration = Duration::from_millis(250);
pub const ORPHAN_WAIT: Duration = Duration::from_secs(20);

#[derive(Clone, Debug, Default)]
pub struct Measured {
    pub exit: String,
    pub peak: u64,
    pub processes_max: u64,
    pub adopted: u64,
    pub samples: u64,
    pub parts: Vec<(String, u64)>,
    pub left: u64,
    pub lifetime_max_sum: u64,
}

pub fn exit_text(status: ExitStatus) -> String {
    if let Some(code) = status.code() {
        return code.to_string();
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = status.signal() {
            return format!("sig{sig}");
        }
    }
    "unknown".into()
}

pub fn now_us() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0)
}

pub const SYSTEM_DIRS: [&str; 6] = [
    "/bin/",
    "/sbin/",
    "/usr/bin/",
    "/usr/sbin/",
    "/usr/libexec/",
    "/System/",
];

pub fn adoption_roots(command_exe: Option<&std::path::Path>, exes: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = exes
        .iter()
        .map(|e| r3_web::procs::bundle_root(e))
        .filter(|r| r.to_string_lossy().ends_with(".app"))
        .collect();
    if let Some(exe) = command_exe {
        let text = exe.to_string_lossy();
        if !SYSTEM_DIRS.iter().any(|d| text.starts_with(d)) {
            roots.push(exe.to_path_buf());
        }
    }
    roots.sort();
    roots.dedup();
    roots
}

#[cfg(target_os = "macos")]
pub fn footprints(pid: u32) -> Option<(u64, u64)> {
    let mut info: libc::rusage_info_v4 = unsafe { std::mem::zeroed() };
    let r = unsafe {
        libc::proc_pid_rusage(
            pid as libc::c_int,
            libc::RUSAGE_INFO_V4,
            (&mut info as *mut libc::rusage_info_v4).cast::<libc::rusage_info_t>(),
        )
    };
    (r == 0).then_some((info.ri_phys_footprint, info.ri_lifetime_max_phys_footprint))
}

#[cfg(target_os = "macos")]
pub fn phys_footprint(pid: u32) -> Option<u64> {
    footprints(pid).map(|(now, _)| now)
}

pub trait Stopper {
    fn stopped(&self) -> bool;
}

#[cfg(target_os = "macos")]
pub fn measure(cmd: &mut Command, stop: &dyn Stopper) -> std::io::Result<Measured> {
    use r3_web::procs::{adopted, descendants, running, same, table, Proc};
    let start = now_us();
    let mut child: Child = cmd.spawn()?;
    let root = child.id();
    let first = table()?;
    let command_exe = first
        .iter()
        .find(|p| p.pid == root)
        .and_then(|p| p.exe.clone());
    let mut m = Measured::default();
    let mut seen: Vec<Proc> = first.into_iter().filter(|p| p.pid == root).collect();
    let mut taken: Vec<Proc> = Vec::new();
    let mut lifetime: std::collections::BTreeMap<u32, u64> = std::collections::BTreeMap::new();
    let mut exited: Option<(ExitStatus, Instant)> = None;
    let mut signalled = false;
    loop {
        let fresh: Vec<Proc> = table()?
            .into_iter()
            .filter(|p| p.started_us >= start)
            .collect();
        if seen.is_empty() {
            seen.extend(fresh.iter().filter(|p| p.pid == root).cloned());
        }
        for p in descendants(&fresh, &seen) {
            if !seen.iter().any(|s| same(s, &p)) {
                seen.push(p);
            }
        }
        let exes: Vec<PathBuf> = fresh
            .iter()
            .filter(|p| seen.iter().any(|s| same(s, p)))
            .filter_map(|p| p.exe.clone())
            .collect();
        let roots = adoption_roots(command_exe.as_deref(), &exes);
        for p in adopted(&fresh, &seen, start, &roots) {
            if !taken.iter().any(|t| same(t, &p)) {
                taken.push(p);
            }
        }
        let members: Vec<&Proc> = fresh
            .iter()
            .filter(|p| seen.iter().any(|s| same(s, p)) || taken.iter().any(|t| same(t, p)))
            .collect();
        let mut sum = 0u64;
        let mut parts: Vec<(String, u64)> = Vec::new();
        for p in &members {
            if let Some((bytes, max)) = footprints(p.pid) {
                sum += bytes;
                let slot = lifetime.entry(p.pid).or_insert(0);
                *slot = (*slot).max(max);
                let name = p
                    .exe
                    .as_ref()
                    .and_then(|e| e.file_name())
                    .map(|n| n.to_string_lossy().replace(' ', "_"))
                    .unwrap_or_else(|| "-".into());
                match parts.iter_mut().find(|(n, _)| *n == name) {
                    Some(entry) => entry.1 += bytes,
                    None => parts.push((name, bytes)),
                }
            }
        }
        m.samples += 1;
        m.processes_max = m.processes_max.max(members.len() as u64);
        if sum > m.peak {
            m.peak = sum;
            parts.sort();
            m.parts = parts;
        }
        if exited.is_none() {
            if let Some(status) = child.try_wait()? {
                exited = Some((status, Instant::now()));
            }
        }
        if stop.stopped() && !signalled {
            signalled = true;
            unsafe { libc::kill(root as libc::c_int, libc::SIGTERM) };
        }
        if let Some((_, at)) = exited {
            let living = members.iter().filter(|p| running(p)).count() as u64;
            if living == 0 || at.elapsed() > ORPHAN_WAIT {
                m.left = living;
                break;
            }
        }
        std::thread::sleep(PERIOD);
    }
    m.adopted = taken.len() as u64;
    m.lifetime_max_sum = lifetime.values().sum();
    m.exit = exited.map(|(s, _)| exit_text(s)).unwrap_or_default();
    Ok(m)
}

#[cfg(windows)]
pub fn measure(cmd: &mut Command, stop: &dyn Stopper) -> std::io::Result<Measured> {
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
    };
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicAccountingInformation,
        JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
        TerminateJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::{
        OpenThread, ResumeThread, CREATE_SUSPENDED, THREAD_SUSPEND_RESUME,
    };
    let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if job.is_null() {
        return Err(std::io::Error::last_os_error());
    }
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    cmd.creation_flags(CREATE_SUSPENDED);
    let mut child: Child = cmd.spawn()?;
    let assigned = unsafe { AssignProcessToJobObject(job, child.as_raw_handle() as _) };
    if assigned == 0 {
        let e = std::io::Error::last_os_error();
        let _ = child.kill();
        unsafe { CloseHandle(job) };
        return Err(e);
    }
    let pid = child.id();
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    let mut entry: THREADENTRY32 = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
    let mut ok = unsafe { Thread32First(snap, &mut entry) };
    while ok != 0 {
        if entry.th32OwnerProcessID == pid {
            let h = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
            if !h.is_null() {
                unsafe {
                    ResumeThread(h);
                    CloseHandle(h);
                }
            }
        }
        ok = unsafe { Thread32Next(snap, &mut entry) };
    }
    unsafe { CloseHandle(snap) };
    let mut m = Measured::default();
    let mut exited: Option<(ExitStatus, Instant)> = None;
    let mut signalled = false;
    loop {
        let mut basic: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { std::mem::zeroed() };
        unsafe {
            QueryInformationJobObject(
                job,
                JobObjectBasicAccountingInformation,
                (&mut basic as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                std::mem::size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                std::ptr::null_mut(),
            )
        };
        m.samples += 1;
        m.processes_max = m.processes_max.max(basic.ActiveProcesses as u64);
        m.adopted = basic.TotalProcesses as u64;
        if exited.is_none() {
            if let Some(status) = child.try_wait()? {
                exited = Some((status, Instant::now()));
            }
        }
        if stop.stopped() && !signalled {
            signalled = true;
        }
        if let Some((_, at)) = exited {
            if basic.ActiveProcesses == 0 || at.elapsed() > ORPHAN_WAIT {
                m.left = basic.ActiveProcesses as u64;
                break;
            }
        }
        std::thread::sleep(PERIOD);
    }
    let mut ext: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    unsafe {
        QueryInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            (&mut ext as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            std::ptr::null_mut(),
        );
        if m.left > 0 {
            TerminateJobObject(job, 1);
        }
        CloseHandle(job);
    }
    m.peak = ext.PeakJobMemoryUsed as u64;
    m.exit = exited.map(|(s, _)| exit_text(s)).unwrap_or_default();
    Ok(m)
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn measure(_cmd: &mut Command, _stop: &dyn Stopper) -> std::io::Result<Measured> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "tree-peak reads macOS and Windows only",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adoption_roots_take_app_bundles_and_the_command_binary_only() {
        let exes = vec![
            PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
            PathBuf::from("/usr/bin/profiles"),
            PathBuf::from("/opt/node/bin/node"),
        ];
        assert_eq!(
            adoption_roots(
                Some(std::path::Path::new("/w/target/release/web-trial")),
                &exes
            ),
            vec![
                PathBuf::from("/Applications/Google Chrome.app"),
                PathBuf::from("/w/target/release/web-trial"),
            ]
        );
        assert_eq!(
            adoption_roots(Some(std::path::Path::new("/bin/sh")), &exes),
            vec![PathBuf::from("/Applications/Google Chrome.app")]
        );
    }
}
