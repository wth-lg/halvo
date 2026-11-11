// verprobe: verifier-4's probe of the Job cap of halvo-cap on Windows (claim F).
// verprobe <mode> <bytes> <hold ms> [<path>]

#[cfg(not(windows))]
fn main() {
    println!("verprobe runs on Windows only");
}

#[cfg(windows)]
fn main() {
    std::process::exit(win::run());
}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;
    use std::io::Write;
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    use std::time::Duration;

    type Handle = *mut c_void;

    #[repr(C)]
    struct Counters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set: usize,
        working_set: usize,
        quota_peak_paged: usize,
        quota_paged: usize,
        quota_peak_nonpaged: usize,
        quota_nonpaged: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
        private_usage: usize,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> Handle;
        fn VirtualAlloc(addr: *mut c_void, size: usize, kind: u32, protect: u32) -> *mut c_void;
        fn CreateFileMappingW(
            file: Handle,
            attrs: *mut c_void,
            protect: u32,
            size_high: u32,
            size_low: u32,
            name: *const u16,
        ) -> Handle;
        fn MapViewOfFile(mapping: Handle, access: u32, offset_high: u32, offset_low: u32, bytes: usize) -> *mut c_void;
        fn K32GetProcessMemoryInfo(process: Handle, counters: *mut Counters, cb: u32) -> i32;
        fn IsProcessInJob(process: Handle, job: Handle, result: *mut i32) -> i32;
        fn GetLastError() -> u32;
    }

    const MEM_COMMIT_RESERVE: u32 = 0x3000;
    const PAGE_READWRITE: u32 = 0x04;
    const FILE_MAP_ALL_ACCESS: u32 = 0x000F_001F;
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    const STEP: usize = 1_048_576;
    const PAGE: usize = 4096;

    struct Out {
        file: Option<std::fs::File>,
    }

    impl Out {
        fn line(&mut self, text: String) {
            let text = format!("verprobe pid={} {text}", std::process::id());
            match &mut self.file {
                Some(file) => {
                    let _ = writeln!(file, "{text}");
                    let _ = file.flush();
                }
                None => println!("{text}"),
            }
        }
    }

    fn counters() -> String {
        let mut c: Counters = unsafe { std::mem::zeroed() };
        c.cb = std::mem::size_of::<Counters>() as u32;
        let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
        if ok == 0 {
            return format!("counters error={}", unsafe { GetLastError() });
        }
        format!(
            "counters working_set={} peak_working_set={} private_usage={} pagefile_usage={} peak_pagefile_usage={}",
            c.working_set, c.peak_working_set, c.private_usage, c.pagefile_usage, c.peak_pagefile_usage
        )
    }

    fn in_job() -> String {
        let mut result = 0i32;
        let ok = unsafe { IsProcessInJob(GetCurrentProcess(), std::ptr::null_mut(), &mut result) };
        if ok == 0 {
            return format!("in_job=error-{}", unsafe { GetLastError() });
        }
        format!("in_job={}", if result != 0 { "yes" } else { "no" })
    }

    fn touch(start: *mut u8, bytes: usize) {
        let mut offset = 0;
        while offset < bytes {
            unsafe { start.add(offset).write_volatile((offset / PAGE % 251 + 1) as u8) };
            offset += PAGE;
        }
    }

    // Commits and writes `bytes` in steps of 1 MiB; the count reached and whether a step was refused.
    fn plain(bytes: usize, out: &mut Out) -> (usize, bool) {
        let mut total = 0;
        while total < bytes {
            let size = (bytes - total).min(STEP);
            let block = unsafe { VirtualAlloc(std::ptr::null_mut(), size, MEM_COMMIT_RESERVE, PAGE_READWRITE) };
            if block.is_null() {
                out.line(format!("refused allocated_bytes={total} error={}", unsafe { GetLastError() }));
                return (total, true);
            }
            touch(block.cast(), size);
            total += size;
        }
        out.line(format!("allocated_bytes={total} {}", counters()));
        (total, false)
    }

    // After the hold: reads every page of the view again and counts those that still hold
    // what was written.
    fn mapped_and_held(file: Handle, bytes: usize, hold: Duration, out: &mut Out) -> bool {
        let Some(view) = mapped(file, bytes, out) else {
            return false;
        };
        std::thread::sleep(hold);
        let mut kept = 0usize;
        let mut offset = 0;
        while offset < bytes {
            if unsafe { view.add(offset).read_volatile() } == (offset / PAGE % 251 + 1) as u8 {
                kept += 1;
            }
            offset += PAGE;
        }
        out.line(format!("pages that still hold their data={kept} of {}", bytes / PAGE));
        true
    }

    fn mapped(file: Handle, bytes: usize, out: &mut Out) -> Option<*mut u8> {
        let mapping = unsafe {
            CreateFileMappingW(
                file,
                std::ptr::null_mut(),
                PAGE_READWRITE,
                (bytes as u64 >> 32) as u32,
                bytes as u32,
                std::ptr::null(),
            )
        };
        if mapping.is_null() {
            out.line(format!("CreateFileMappingW refused error={}", unsafe { GetLastError() }));
            return None;
        }
        let view = unsafe { MapViewOfFile(mapping, FILE_MAP_ALL_ACCESS, 0, 0, bytes) };
        if view.is_null() {
            out.line(format!("MapViewOfFile refused error={}", unsafe { GetLastError() }));
            return None;
        }
        touch(view.cast(), bytes);
        out.line(format!("mapped and written bytes={bytes} {}", counters()));
        Some(view.cast())
    }

    pub fn run() -> i32 {
        let a: Vec<String> = std::env::args().collect();
        let mode = a.get(1).map(String::as_str).unwrap_or("");
        let bytes: usize = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
        let hold = Duration::from_millis(a.get(3).and_then(|s| s.parse().ok()).unwrap_or(0));
        let path = a.get(4).cloned().unwrap_or_default();
        let exe = std::env::current_exe().expect("current_exe");
        let mut out = Out { file: None };
        if mode == "plain-log" {
            out.file = std::fs::OpenOptions::new().create(true).append(true).open(&path).ok();
        }
        out.line(format!("start mode={mode} bytes={bytes} {} {}", in_job(), counters()));
        match mode {
            "plain" | "plain-log" => {
                let (_, refused) = plain(bytes, &mut out);
                std::thread::sleep(hold);
                out.line(format!("end {}", counters()));
                if refused {
                    7
                } else {
                    0
                }
            }
            "two" => {
                let half = bytes / 2;
                let child = Command::new(&exe)
                    .args(["plain", &half.to_string(), &hold.as_millis().to_string()])
                    .spawn();
                let (_, refused) = plain(half, &mut out);
                std::thread::sleep(hold);
                let status = child.and_then(|mut c| c.wait());
                out.line(format!("end second process: {status:?} {}", counters()));
                if refused || !status.map(|s| s.success()).unwrap_or(false) {
                    7
                } else {
                    0
                }
            }
            "section" => {
                let ok = mapped_and_held(-1isize as Handle, bytes, hold, &mut out);
                out.line(format!("end {}", counters()));
                if ok {
                    0
                } else {
                    7
                }
            }
            "mapfile" => {
                let file = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(&path)
                    .expect("open the file to map");
                file.set_len(bytes as u64).expect("set_len");
                let ok = mapped_and_held(file.as_raw_handle() as Handle, bytes, hold, &mut out);
                out.line(format!("end {}", counters()));
                if ok {
                    0
                } else {
                    7
                }
            }
            "breakaway" => {
                let child = Command::new(&exe)
                    .args(["plain-log", &bytes.to_string(), &hold.as_millis().to_string(), &path])
                    .creation_flags(CREATE_BREAKAWAY_FROM_JOB)
                    .spawn();
                match child {
                    Ok(mut c) => {
                        let status = c.wait();
                        out.line(format!("breakaway child started and ended: {status:?}"));
                    }
                    Err(e) => out.line(format!("breakaway child not started: {e}")),
                }
                out.line(format!("log of the child: {:?}", std::fs::read_to_string(&path)));
                0
            }
            "schtasks" => {
                // A task of the Task Scheduler, created and run by this user, starts the process.
                let line = format!("{} plain-log {} {} {}", exe.display(), bytes, hold.as_millis(), path);
                let name = format!("r2-verifier4-u02-{}", std::process::id());
                let made = Command::new("schtasks")
                    .args(["/create", "/tn", &name, "/tr", &line, "/sc", "once", "/st", "23:59", "/f"])
                    .status();
                out.line(format!("schtasks /create ended: {made:?}"));
                let ran = Command::new("schtasks").args(["/run", "/tn", &name]).status();
                out.line(format!("schtasks /run ended: {ran:?}"));
                std::thread::sleep(hold + Duration::from_millis(5000));
                out.line(format!("log of the process that the task started: {:?}", std::fs::read_to_string(&path)));
                let gone = Command::new("schtasks").args(["/delete", "/tn", &name, "/f"]).status();
                out.line(format!("schtasks /delete ended: {gone:?}"));
                out.line(format!("end {}", counters()));
                0
            }
            "wmi" => {
                let script = format!(
                    "$r = Invoke-CimMethod -ClassName Win32_Process -MethodName Create -Arguments @{{CommandLine='{} plain-log {} {} {}'}}; Write-Output ('wmi ReturnValue=' + $r.ReturnValue + ' ProcessId=' + $r.ProcessId)",
                    exe.display(),
                    bytes,
                    hold.as_millis(),
                    path
                );
                let status = Command::new("powershell")
                    .args(["-NoProfile", "-NonInteractive", "-Command", &script])
                    .status();
                out.line(format!("powershell ended: {status:?}"));
                std::thread::sleep(hold + Duration::from_millis(4000));
                out.line(format!("log of the process that WMI started: {:?}", std::fs::read_to_string(&path)));
                out.line(format!("end {}", counters()));
                0
            }
            _ => 64,
        }
    }
}
