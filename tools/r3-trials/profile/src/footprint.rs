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

pub fn adoption_roots(command_exe: Option<&std::path::Path>, exes: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = exes
        .iter()
        .map(|e| r3_web::procs::bundle_root(e))
        .filter(|r| r.to_string_lossy().ends_with(".app"))
        .collect();
    if let Some(dir) = command_exe.and_then(|e| e.parent()) {
        roots.push(dir.to_path_buf());
    }
    roots.sort();
    roots.dedup();
    roots
}

#[cfg(target_os = "macos")]
pub fn phys_footprint(pid: u32) -> Option<u64> {
    let mut info: libc::rusage_info_v2 = unsafe { std::mem::zeroed() };
    let r = unsafe {
        libc::proc_pid_rusage(
            pid as libc::c_int,
            libc::RUSAGE_INFO_V2,
            (&mut info as *mut libc::rusage_info_v2).cast::<libc::rusage_info_t>(),
        )
    };
    (r == 0).then_some(info.ri_phys_footprint)
}

pub trait Stopper {
    fn stopped(&self) -> bool;
}

#[cfg(target_os = "macos")]
pub fn measure(cmd: &mut Command, stop: &dyn Stopper) -> std::io::Result<Measured> {
    use r3_web::procs::{adopted, descendants, table};
    let start = now_us();
    let mut child: Child = cmd.spawn()?;
    let root = child.id();
    let command_exe = table()?
        .into_iter()
        .find(|p| p.pid == root)
        .and_then(|p| p.exe);
    let mut m = Measured::default();
    let mut seen: Vec<u32> = vec![root];
    let mut taken: Vec<u32> = Vec::new();
    let mut exited: Option<(ExitStatus, Instant)> = None;
    let mut signalled = false;
    loop {
        let t = table()?;
        let fresh: Vec<&r3_web::procs::Proc> = t.iter().filter(|p| p.started_us >= start).collect();
        let fresh_table: Vec<r3_web::procs::Proc> = fresh.iter().map(|p| (*p).clone()).collect();
        for pid in descendants(&fresh_table, &seen) {
            if !seen.contains(&pid) {
                seen.push(pid);
            }
        }
        let exes: Vec<PathBuf> = fresh_table
            .iter()
            .filter(|p| seen.contains(&p.pid))
            .filter_map(|p| p.exe.clone())
            .collect();
        let roots = adoption_roots(command_exe.as_deref(), &exes);
        for pid in adopted(&fresh_table, &seen, start, &roots) {
            if !taken.contains(&pid) {
                taken.push(pid);
            }
        }
        let mut all = seen.clone();
        all.extend(taken.iter().copied());
        for pid in descendants(&fresh_table, &all) {
            if !all.contains(&pid) {
                all.push(pid);
            }
            if !seen.contains(&pid) && !taken.contains(&pid) {
                taken.push(pid);
            }
        }
        let members: Vec<&r3_web::procs::Proc> = fresh_table
            .iter()
            .filter(|p| all.contains(&p.pid))
            .collect();
        let mut sum = 0u64;
        let mut parts: Vec<(String, u64)> = Vec::new();
        for p in &members {
            if let Some(bytes) = phys_footprint(p.pid) {
                sum += bytes;
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
            let living = members
                .iter()
                .filter(|p| r3_web::procs::alive(p.pid))
                .count() as u64;
            if living == 0 || at.elapsed() > ORPHAN_WAIT {
                m.left = living;
                break;
            }
        }
        std::thread::sleep(super::footprint::PERIOD);
    }
    m.adopted = taken.len() as u64;
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
    fn adoption_roots_take_app_bundles_and_the_command_directory() {
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
                PathBuf::from("/w/target/release"),
            ]
        );
    }
}
