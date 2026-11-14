use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus};
use std::time::{Duration, Instant};

/// How long tree-peak sleeps after each reading of the tree: 50 ms on macOS, 250 ms elsewhere.
pub const PERIOD: Duration = if cfg!(target_os = "macos") {
    Duration::from_millis(50)
} else {
    Duration::from_millis(250)
};
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
    // SAFETY: rusage_info_v4 is a C struct of integers, for which all-zero bytes are a valid value.
    let mut info: libc::rusage_info_v4 = unsafe { std::mem::zeroed() };
    // SAFETY: the flavor RUSAGE_INFO_V4 writes one rusage_info_v4 into `info`, a live local of that
    // type.
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
    let start = now_us();
    let child: Child = cmd.spawn()?;
    follow(child, start, stop, &mut |pid| {
        // SAFETY: kill takes a pid and a signal number and touches no memory of this process.
        unsafe { libc::kill(pid as libc::c_int, libc::SIGTERM) };
    })
}

#[cfg(target_os = "macos")]
enum Run {
    Live(Child),
    Ended(ExitStatus, Instant),
}

#[cfg(target_os = "macos")]
fn follow(
    child: Child,
    start: u64,
    stop: &dyn Stopper,
    term: &mut dyn FnMut(u32),
) -> std::io::Result<Measured> {
    use r3_web::procs::{adopted, descendants, running, same, table, Proc};
    let first = table()?;
    let command_exe = first
        .iter()
        .find(|p| p.pid == child.id())
        .and_then(|p| p.exe.clone());
    let mut m = Measured::default();
    let mut seen: Vec<Proc> = first.into_iter().filter(|p| p.pid == child.id()).collect();
    let mut taken: Vec<Proc> = Vec::new();
    let mut lifetime: std::collections::BTreeMap<u32, u64> = std::collections::BTreeMap::new();
    let mut run = Run::Live(child);
    let mut signalled = false;
    let status = loop {
        let fresh: Vec<Proc> = table()?
            .into_iter()
            .filter(|p| p.started_us >= start)
            .collect();
        if let (true, Run::Live(child)) = (seen.is_empty(), &run) {
            seen.extend(fresh.iter().filter(|p| p.pid == child.id()).cloned());
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
        let reaped = match &mut run {
            Run::Live(child) => child.try_wait()?.map(|status| (status, child.id())),
            Run::Ended(..) => None,
        };
        if let Some((status, pid)) = reaped {
            seen.retain(|p| p.pid != pid);
            run = Run::Ended(status, Instant::now());
        }
        if stop.stopped() && !signalled {
            signalled = true;
            match &run {
                Run::Live(child) => term(child.id()),
                Run::Ended(..) => {
                    for p in seen.iter().filter(|p| running(p)) {
                        term(p.pid);
                    }
                }
            }
        }
        if let Run::Ended(status, at) = &run {
            let living = members.iter().filter(|p| running(p)).count() as u64;
            if living == 0 || at.elapsed() > ORPHAN_WAIT {
                m.left = living;
                break *status;
            }
        }
        std::thread::sleep(PERIOD);
    };
    m.adopted = taken.len() as u64;
    m.lifetime_max_sum = lifetime.values().sum();
    m.exit = exit_text(status);
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
    // SAFETY: two null pointers ask for no security attributes and no name; the call returns a
    // new job handle or null, which is checked.
    let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if job.is_null() {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: JOBOBJECT_EXTENDED_LIMIT_INFORMATION is a C struct of integers, for which all-zero
    // bytes are a valid value.
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    // SAFETY: `job` is the open job handle, and the call reads the size that it is told from
    // `limits`, a live local of that type.
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
    // SAFETY: `job` is the open job handle, and the process handle is the one that `child` owns and
    // keeps open.
    let assigned = unsafe { AssignProcessToJobObject(job, child.as_raw_handle() as _) };
    if assigned == 0 {
        let e = std::io::Error::last_os_error();
        let _ = child.kill();
        // SAFETY: `job` is the open job handle, closed once here before the return.
        unsafe { CloseHandle(job) };
        return Err(e);
    }
    let pid = child.id();
    // SAFETY: the call takes plain values; an invalid handle makes Thread32First below fail, which
    // ends the loop.
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    // SAFETY: THREADENTRY32 is a C struct of integers, for which all-zero bytes are a valid value;
    // its dwSize is set next.
    let mut entry: THREADENTRY32 = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
    // SAFETY: `snap` is the snapshot (an invalid one makes the call fail), and `entry` is a live
    // THREADENTRY32 whose dwSize is its size.
    let mut ok = unsafe { Thread32First(snap, &mut entry) };
    while ok != 0 {
        if entry.th32OwnerProcessID == pid {
            // SAFETY: the call takes plain values and returns a thread handle or null, which is
            // checked.
            let h = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
            if !h.is_null() {
                // SAFETY: `h` is the thread handle that OpenThread gave just above, opened to resume;
                // it is resumed, then closed once.
                unsafe {
                    ResumeThread(h);
                    CloseHandle(h);
                }
            }
        }
        // SAFETY: as for Thread32First: the snapshot and the same live entry.
        ok = unsafe { Thread32Next(snap, &mut entry) };
    }
    // SAFETY: `snap` was made above and is closed once here, not used after.
    unsafe { CloseHandle(snap) };
    let mut m = Measured::default();
    let mut exited: Option<(ExitStatus, Instant)> = None;
    let mut signalled = false;
    loop {
        // SAFETY: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION is a C struct of integers, for which
        // all-zero bytes are a valid value.
        let mut basic: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: `job` is the open job handle; the call writes at most the size that it is told into
        // `basic`, a live local of that type, and takes a null pointer for the length that it wrote.
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
    // SAFETY: as for `limits`: a C struct of integers.
    let mut ext: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: `job` is the open job handle; the query writes at most the size that it is told into
    // `ext`, a live local; TerminateJobObject ends the processes of this job alone; the handle is
    // closed once and not used after.
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

    #[cfg(target_os = "macos")]
    struct AfterEnd {
        command: r3_web::procs::Proc,
        ended: std::cell::Cell<Option<Instant>>,
    }

    #[cfg(target_os = "macos")]
    impl Stopper for AfterEnd {
        fn stopped(&self) -> bool {
            if r3_web::procs::running(&self.command) {
                return false;
            }
            let at = self.ended.get().unwrap_or_else(Instant::now);
            self.ended.set(Some(at));
            at.elapsed() > Duration::from_millis(500)
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_stop_after_the_command_ended_signals_the_living_member_and_not_the_command() {
        let start = now_us();
        let child = Command::new("/bin/sh")
            .args(["-c", "/bin/sleep 30 & /bin/sleep 1"])
            .spawn()
            .unwrap();
        let root = child.id();
        let command = r3_web::procs::table()
            .unwrap()
            .into_iter()
            .find(|p| p.pid == root)
            .expect("the command is in the table");
        let stop = AfterEnd {
            command,
            ended: std::cell::Cell::new(None),
        };
        let mut sent: Vec<u32> = Vec::new();
        let m = follow(child, start, &stop, &mut |pid| {
            sent.push(pid);
            if pid != root {
                // SAFETY: kill takes a pid and a signal number and touches no memory of this process.
                unsafe { libc::kill(pid as libc::c_int, libc::SIGTERM) };
            }
        })
        .unwrap();
        assert!(!sent.contains(&root), "sent={sent:?} root={root}");
        assert!(!sent.is_empty(), "{m:?}");
        assert_eq!((m.exit.as_str(), m.left), ("0", 0), "sent={sent:?} {m:?}");
    }
}
