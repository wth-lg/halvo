//! Spike S3 of run 20261108-halvo-r1-capture: can a medium-integrity process
//! tell that the focused control is a password field before the first key is
//! recorded, on the hosted windows-2025 runner? Not product code; never merged.
//!
//! Subcommands: `setup`, `fixture-win32 <stop-file>`, `probe <target>`,
//! `cost <browser> <none|uia> <run>`. Output lines start with `S3 ` and hold
//! counts, times, sizes and booleans only: never a title, a text or a path.

#[cfg(not(windows))]
fn main() {
    eprintln!("spike-s3 runs on Windows only");
}

#[cfg(windows)]
fn main() {
    win::main();
}

#[cfg(windows)]
mod win {
    use std::mem::size_of;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::{Duration, Instant};

    use windows::core::{implement, w, Ref, BOOL, HSTRING, PCWSTR, PWSTR};
    use windows::Win32::Foundation::{
        CloseHandle, FILETIME, HANDLE, HWND, LPARAM, LRESULT, RECT, RPC_E_CHANGED_MODE, WPARAM,
    };
    use windows::Win32::Graphics::Gdi::{
        ChangeDisplaySettingsExW, EnumDisplaySettingsW, CDS_TYPE, COLOR_WINDOW, DEVMODEW,
        DISP_CHANGE_SUCCESSFUL, DM_PELSHEIGHT, DM_PELSWIDTH, ENUM_CURRENT_SETTINGS, HBRUSH,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
    };
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicAccountingInformation,
        JobObjectBasicProcessIdList, QueryInformationJobObject, TerminateJobObject,
        JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
    };
    use windows::Win32::System::Threading::{
        AttachThreadInput, CreateProcessW, GetCurrentProcess, GetCurrentThreadId, GetProcessTimes,
        OpenProcess, ResumeThread, CREATE_SUSPENDED, PROCESS_INFORMATION,
        PROCESS_QUERY_LIMITED_INFORMATION, STARTUPINFOW,
    };
    use windows::Win32::UI::Accessibility::{
        CUIAutomation8, IUIAutomation, IUIAutomationCacheRequest, IUIAutomationElement,
        IUIAutomationFocusChangedEventHandler, IUIAutomationFocusChangedEventHandler_Impl,
        TreeScope_Descendants, UIA_BoundingRectanglePropertyId, UIA_ControlTypePropertyId,
        UIA_EditControlTypeId, UIA_IsPasswordPropertyId, UIA_ProcessIdPropertyId,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYBD_EVENT_FLAGS,
        KEYEVENTF_KEYUP, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEINPUT, VIRTUAL_KEY,
        VK_MENU, VK_SHIFT, VK_TAB,
    };
    use windows::Win32::UI::Input::{
        GetRawInputData, RegisterRawInputDevices, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE,
        RAWINPUTHEADER, RIDEV_INPUTSINK, RID_INPUT, RIM_TYPEKEYBOARD,
    };
    use windows::Win32::UI::WindowsAndMessaging::*;

    /// dwExtraInfo of every SendInput event of this probe ("S3SP").
    const MARKER: usize = 0x5333_5350;
    const VK_X: VIRTUAL_KEY = VIRTUAL_KEY(0x58);
    const ES_PASSWORD_BIT: isize = 0x20;
    const TRIALS: usize = 20;
    const HOLD_LIMIT_MS: f64 = 250.0;
    const WIN_X: i32 = 1160;
    const WIN_Y: i32 = 40;
    const WIN_W: i32 = 740;
    const WIN_H: i32 = 1000;

    static START: OnceLock<Instant> = OnceLock::new();

    fn now_ms() -> f64 {
        START.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0
    }

    fn say(line: impl AsRef<str>) {
        println!("S3 {}", line.as_ref());
    }

    fn sleep_ms(ms: u64) {
        std::thread::sleep(Duration::from_millis(ms));
    }

    pub fn main() {
        let _ = now_ms();
        let args: Vec<String> = std::env::args().collect();
        let arg = |i: usize| args.get(i).cloned().unwrap_or_default();
        match arg(1).as_str() {
            "setup" => setup(),
            "fixture-win32" => fixture_win32(&arg(2)),
            "probe" => probe(&arg(2)),
            "cost" => cost(&arg(2), &arg(3), &arg(4)),
            "cold" => cold(&arg(2), &arg(3)),
            other => {
                say(format!("error unknown_subcommand={}", other.len()));
                std::process::exit(64);
            }
        }
    }

    // ---- display -------------------------------------------------------

    fn current_mode() -> Option<DEVMODEW> {
        let mut dm = DEVMODEW {
            dmSize: size_of::<DEVMODEW>() as u16,
            ..Default::default()
        };
        unsafe { EnumDisplaySettingsW(PCWSTR::null(), ENUM_CURRENT_SETTINGS, &mut dm) }
            .as_bool()
            .then_some(dm)
    }

    fn setup() {
        let before = current_mode().map(|m| (m.dmPelsWidth, m.dmPelsHeight));
        let result = current_mode().map(|mut dm| {
            dm.dmPelsWidth = 1920;
            dm.dmPelsHeight = 1080;
            dm.dmFields = DM_PELSWIDTH | DM_PELSHEIGHT;
            unsafe { ChangeDisplaySettingsExW(PCWSTR::null(), Some(&dm), None, CDS_TYPE(0), None) }
        });
        let after = current_mode().map(|m| (m.dmPelsWidth, m.dmPelsHeight));
        say(format!(
            "setup display_before={:?} change_ok={} display_after={:?}",
            before,
            result == Some(DISP_CHANGE_SUCCESSFUL),
            after
        ));
    }

    // ---- input ---------------------------------------------------------

    fn key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: if up {
                        KEYEVENTF_KEYUP
                    } else {
                        KEYBD_EVENT_FLAGS(0)
                    },
                    time: 0,
                    dwExtraInfo: MARKER,
                },
            },
        }
    }

    fn send(inputs: &[INPUT]) -> u32 {
        unsafe { SendInput(inputs, size_of::<INPUT>() as i32) }
    }

    fn tab() -> u32 {
        send(&[key(VK_TAB, false), key(VK_TAB, true)])
    }

    fn shift_tab() -> u32 {
        send(&[
            key(VK_SHIFT, false),
            key(VK_TAB, false),
            key(VK_TAB, true),
            key(VK_SHIFT, true),
        ])
    }

    fn type_x(n: usize) -> u32 {
        let mut sent = 0;
        for _ in 0..n {
            sent += send(&[key(VK_X, false), key(VK_X, true)]);
            sleep_ms(60);
        }
        sent
    }

    fn click(x: i32, y: i32) -> u32 {
        let _ = unsafe { SetCursorPos(x, y) };
        sleep_ms(80);
        let button = |flags| INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: MARKER,
                },
            },
        };
        send(&[button(MOUSEEVENTF_LEFTDOWN), button(MOUSEEVENTF_LEFTUP)])
    }

    // ---- ES_PASSWORD on the focus window of the foreground thread ------

    #[derive(Clone, Copy, Default)]
    struct EsRead {
        ok: bool,
        edit_class: bool,
        bit: bool,
        micros: f64,
    }

    fn es_read() -> EsRead {
        let t = Instant::now();
        let mut gti = GUITHREADINFO {
            cbSize: size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        let mut r = EsRead::default();
        if unsafe { GetGUIThreadInfo(0, &mut gti) }.is_ok() && !gti.hwndFocus.0.is_null() {
            r.ok = true;
            let mut class = [0u16; 64];
            let n = unsafe { GetClassNameW(gti.hwndFocus, &mut class) }.max(0) as usize;
            let name = String::from_utf16_lossy(&class[..n]).to_ascii_lowercase();
            r.edit_class = name == "edit" || name.starts_with("richedit");
            let style = unsafe { GetWindowLongPtrW(gti.hwndFocus, GWL_STYLE) };
            r.bit = style & ES_PASSWORD_BIT != 0;
        }
        r.micros = t.elapsed().as_secs_f64() * 1e6;
        r
    }

    // ---- Raw Input sink ------------------------------------------------

    #[derive(Clone, Copy)]
    struct RawKey {
        t: f64,
        vkey: u16,
        null_device: bool,
        marker: bool,
        flags: u16,
        es_at_arrival: EsRead,
    }

    type RawLog = Arc<Mutex<Vec<RawKey>>>;

    unsafe extern "system" fn sink_proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        unsafe { DefWindowProcW(h, m, w, l) }
    }

    fn start_raw_sink(log: RawLog) {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let class = w!("SpikeS3RawSink");
            let inst = match unsafe { GetModuleHandleW(None) } {
                Ok(i) => i,
                Err(_) => {
                    let _ = ready_tx.send(false);
                    return;
                }
            };
            let wc = WNDCLASSW {
                lpfnWndProc: Some(sink_proc),
                hInstance: inst.into(),
                lpszClassName: class,
                ..Default::default()
            };
            unsafe { RegisterClassW(&wc) };
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
            };
            let Ok(hwnd) = hwnd else {
                let _ = ready_tx.send(false);
                return;
            };
            let dev = RAWINPUTDEVICE {
                usUsagePage: 1,
                usUsage: 6,
                dwFlags: RIDEV_INPUTSINK,
                hwndTarget: hwnd,
            };
            let ok = unsafe { RegisterRawInputDevices(&[dev], size_of::<RAWINPUTDEVICE>() as u32) }
                .is_ok();
            let _ = ready_tx.send(ok);
            let mut msg = MSG::default();
            while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
                if msg.message == WM_INPUT {
                    let t = now_ms();
                    if let Some(k) = read_raw(msg.lParam, t) {
                        if let Ok(mut v) = log.lock() {
                            v.push(k);
                        }
                    }
                }
                unsafe { DispatchMessageW(&msg) };
            }
        });
        let ok = ready_rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap_or(false);
        say(format!("raw_sink registered={ok}"));
    }

    fn read_raw(lparam: LPARAM, t: f64) -> Option<RawKey> {
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
            return None;
        }
        let raw = unsafe { &*buf.as_ptr().cast::<RAWINPUT>() };
        if raw.header.dwType != RIM_TYPEKEYBOARD.0 {
            return None;
        }
        let k = unsafe { raw.data.keyboard };
        let down = u32::from(k.Flags) & RI_KEY_BREAK == 0;
        if !down {
            return None;
        }
        let es = if k.VKey == VK_X.0 {
            es_read()
        } else {
            EsRead::default()
        };
        Some(RawKey {
            t,
            vkey: k.VKey,
            null_device: raw.header.hDevice.0.is_null(),
            marker: k.ExtraInformation as usize == MARKER,
            flags: k.Flags,
            es_at_arrival: es,
        })
    }

    // ---- UI Automation -------------------------------------------------

    #[derive(Clone, Copy, Debug)]
    struct FocusEvent {
        t: f64,
        is_password: Option<bool>,
        control_type: i32,
        rect: RECT,
    }

    type FocusLog = Arc<Mutex<Vec<FocusEvent>>>;

    #[implement(IUIAutomationFocusChangedEventHandler)]
    struct FocusHandler {
        log: FocusLog,
    }

    impl IUIAutomationFocusChangedEventHandler_Impl for FocusHandler_Impl {
        fn HandleFocusChangedEvent(
            &self,
            sender: Ref<IUIAutomationElement>,
        ) -> windows::core::Result<()> {
            let t = now_ms();
            let mut ev = FocusEvent {
                t,
                is_password: None,
                control_type: 0,
                rect: RECT::default(),
            };
            if let Some(e) = sender.as_ref() {
                ev.is_password = unsafe { e.CachedIsPassword() }.ok().map(|b| b.as_bool());
                ev.control_type = unsafe { e.CachedControlType() }.map(|c| c.0).unwrap_or(0);
                ev.rect = unsafe { e.CachedBoundingRectangle() }.unwrap_or_default();
            }
            if let Ok(mut v) = self.log.lock() {
                v.push(ev);
            }
            Ok(())
        }
    }

    struct Uia {
        auto: IUIAutomation,
        cache: IUIAutomationCacheRequest,
        log: FocusLog,
    }

    fn uia_attach() -> Result<Uia, String> {
        unsafe {
            let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
            if hr.is_err() && hr != RPC_E_CHANGED_MODE {
                return Err(format!("CoInitializeEx {hr:?}"));
            }
            let auto: IUIAutomation = CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| format!("CoCreateInstance {e}"))?;
            let cache = auto
                .CreateCacheRequest()
                .map_err(|e| format!("cache {e}"))?;
            for p in [
                UIA_IsPasswordPropertyId,
                UIA_ControlTypePropertyId,
                UIA_ProcessIdPropertyId,
                UIA_BoundingRectanglePropertyId,
            ] {
                cache
                    .AddProperty(p)
                    .map_err(|e| format!("AddProperty {e}"))?;
            }
            let log: FocusLog = Arc::new(Mutex::new(Vec::new()));
            let handler: IUIAutomationFocusChangedEventHandler = FocusHandler {
                log: Arc::clone(&log),
            }
            .into();
            auto.AddFocusChangedEventHandler(&cache, &handler)
                .map_err(|e| format!("AddFocusChangedEventHandler {e}"))?;
            Ok(Uia { auto, cache, log })
        }
    }

    impl Uia {
        fn detach(&self) {
            let _ = unsafe { self.auto.RemoveAllEventHandlers() };
        }

        fn events_since(&self, t0: f64) -> Vec<FocusEvent> {
            self.log
                .lock()
                .map(|v| v.iter().filter(|e| e.t >= t0).copied().collect())
                .unwrap_or_default()
        }

        /// The first Edit focus event after `t0`, waiting up to `limit_ms`.
        fn wait_edit(&self, t0: f64, limit_ms: f64) -> Option<FocusEvent> {
            while now_ms() - t0 < limit_ms {
                if let Some(e) = self
                    .events_since(t0)
                    .into_iter()
                    .find(|e| e.control_type == UIA_EditControlTypeId.0)
                {
                    return Some(e);
                }
                sleep_ms(2);
            }
            None
        }

        /// One synchronous read of the focused element's IsPassword.
        fn sync_read(&self) -> (Option<bool>, f64) {
            let t = Instant::now();
            let r = unsafe { self.auto.GetFocusedElementBuildCache(&self.cache) }
                .ok()
                .and_then(|e| unsafe { e.CachedIsPassword() }.ok())
                .map(|b| b.as_bool());
            (r, t.elapsed().as_secs_f64() * 1e6)
        }

        /// The page's plain and password fields under `hwnd`, by the tree.
        fn fields(&self, hwnd: HWND) -> (Option<RECT>, Option<RECT>, usize) {
            let mut plain: Option<(i64, RECT)> = None;
            let mut secret = None;
            let mut edits = 0;
            unsafe {
                let Ok(root) = self.auto.ElementFromHandle(hwnd) else {
                    return (None, None, 0);
                };
                let Ok(cond) = self.auto.CreateTrueCondition() else {
                    return (None, None, 0);
                };
                let Ok(all) = root.FindAllBuildCache(TreeScope_Descendants, &cond, &self.cache)
                else {
                    return (None, None, 0);
                };
                let n = all.Length().unwrap_or(0);
                for i in 0..n {
                    let Ok(e) = all.GetElement(i) else { continue };
                    if e.CachedControlType().map(|c| c.0).unwrap_or(0) != UIA_EditControlTypeId.0 {
                        continue;
                    }
                    edits += 1;
                    let Ok(r) = e.CachedBoundingRectangle() else {
                        continue;
                    };
                    let area = i64::from(r.right - r.left) * i64::from(r.bottom - r.top);
                    let pw = e.CachedIsPassword().map(|b| b.as_bool()).unwrap_or(false);
                    if pw {
                        secret = Some(r);
                    } else if plain.map(|(a, _)| area > a).unwrap_or(true) {
                        plain = Some((area, r));
                    }
                }
            }
            (plain.map(|(_, r)| r), secret, edits)
        }
    }

    // ---- processes in a job --------------------------------------------

    struct Job {
        handle: HANDLE,
        pid: u32,
    }

    impl Job {
        fn spawn(command_line: &str) -> Result<Job, String> {
            unsafe {
                let handle =
                    CreateJobObjectW(None, PCWSTR::null()).map_err(|e| format!("job {e}"))?;
                let mut wide: Vec<u16> = command_line.encode_utf16().chain(Some(0)).collect();
                let si = STARTUPINFOW {
                    cb: size_of::<STARTUPINFOW>() as u32,
                    ..Default::default()
                };
                let mut pi = PROCESS_INFORMATION::default();
                CreateProcessW(
                    PCWSTR::null(),
                    Some(PWSTR(wide.as_mut_ptr())),
                    None,
                    None,
                    false,
                    CREATE_SUSPENDED,
                    None,
                    PCWSTR::null(),
                    &si,
                    &mut pi,
                )
                .map_err(|e| format!("CreateProcessW {e}"))?;
                let assigned = AssignProcessToJobObject(handle, pi.hProcess).is_ok();
                ResumeThread(pi.hThread);
                let _ = CloseHandle(pi.hThread);
                let _ = CloseHandle(pi.hProcess);
                if !assigned {
                    return Err("AssignProcessToJobObject failed".to_string());
                }
                Ok(Job {
                    handle,
                    pid: pi.dwProcessId,
                })
            }
        }

        fn cpu_ms(&self) -> f64 {
            let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
            let ok = unsafe {
                QueryInformationJobObject(
                    Some(self.handle),
                    JobObjectBasicAccountingInformation,
                    &mut info as *mut _ as *mut core::ffi::c_void,
                    size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                    None,
                )
            }
            .is_ok();
            if ok {
                (info.TotalUserTime + info.TotalKernelTime) as f64 / 10_000.0
            } else {
                -1.0
            }
        }

        fn pids(&self) -> Vec<u32> {
            let mut buf = [0usize; 1026];
            let ok = unsafe {
                QueryInformationJobObject(
                    Some(self.handle),
                    JobObjectBasicProcessIdList,
                    buf.as_mut_ptr() as *mut core::ffi::c_void,
                    size_of::<[usize; 1026]>() as u32,
                    None,
                )
            }
            .is_ok();
            if !ok {
                return Vec::new();
            }
            let listed = buf[0] >> 32;
            let listed = listed.min(1024);
            buf[1..1 + listed].iter().map(|&p| p as u32).collect()
        }

        /// Summed working set and private bytes of the job's processes.
        fn memory(&self) -> (u64, u64, usize) {
            let pids = self.pids();
            let (mut ws, mut private) = (0u64, 0u64);
            for &pid in &pids {
                if let Some((w, p)) = process_memory(pid) {
                    ws += w;
                    private += p;
                }
            }
            (ws, private, pids.len())
        }

        fn end(&self) {
            unsafe {
                let _ = TerminateJobObject(self.handle, 0);
                let _ = CloseHandle(self.handle);
            }
        }
    }

    fn process_memory(pid: u32) -> Option<(u64, u64)> {
        let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
        let mut m = PROCESS_MEMORY_COUNTERS_EX {
            cb: size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
            ..Default::default()
        };
        let ok = unsafe {
            let r = K32GetProcessMemoryInfo(
                h,
                &mut m as *mut PROCESS_MEMORY_COUNTERS_EX as *mut PROCESS_MEMORY_COUNTERS,
                m.cb,
            );
            let _ = CloseHandle(h);
            r.as_bool()
        };
        ok.then_some((m.WorkingSetSize as u64, m.PrivateUsage as u64))
    }

    fn self_cpu_ms() -> f64 {
        let ticks = |f: FILETIME| (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime);
        let (mut c, mut e, mut k, mut u) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        if unsafe { GetProcessTimes(GetCurrentProcess(), &mut c, &mut e, &mut k, &mut u) }.is_ok() {
            (ticks(k) + ticks(u)) as f64 / 10_000.0
        } else {
            -1.0
        }
    }

    fn processes_named(image: &str) -> usize {
        let mut n = 0;
        unsafe {
            if let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
                let mut pe = PROCESSENTRY32W {
                    dwSize: size_of::<PROCESSENTRY32W>() as u32,
                    ..Default::default()
                };
                let mut more = Process32FirstW(snap, &mut pe).is_ok();
                while more {
                    let len = pe.szExeFile.iter().position(|&c| c == 0).unwrap_or(0);
                    if String::from_utf16_lossy(&pe.szExeFile[..len]).eq_ignore_ascii_case(image) {
                        n += 1;
                    }
                    more = Process32NextW(snap, &mut pe).is_ok();
                }
                let _ = CloseHandle(snap);
            }
        }
        n
    }

    // ---- targets -------------------------------------------------------

    fn temp_dir(label: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "spike-s3-{label}-{}-{}",
            std::process::id(),
            now_ms() as u64
        ));
        let _ = std::fs::create_dir_all(&d);
        d
    }

    fn page_url() -> String {
        let p = std::env::current_dir()
            .unwrap_or_default()
            .join("tools")
            .join("spike-s3")
            .join("fixture.html");
        format!("file:///{}", p.display().to_string().replace('\\', "/"))
    }

    fn browser_exe(name: &str) -> Option<(&'static str, &'static str)> {
        match name {
            "chrome" => Some((
                r"C:\Program Files\Google\Chrome\Application\chrome.exe",
                "chrome.exe",
            )),
            "edge" => Some((
                r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
                "msedge.exe",
            )),
            "firefox" => Some((
                r"C:\Program Files\Mozilla Firefox\firefox.exe",
                "firefox.exe",
            )),
            _ => None,
        }
    }

    const FIREFOX_PREFS: &str = r#"user_pref("browser.shell.checkDefaultBrowser", false);
user_pref("browser.shell.skipDefaultBrowserCheckOnFirstRun", true);
user_pref("browser.aboutwelcome.enabled", false);
user_pref("browser.startup.upgradeDialog.enabled", false);
user_pref("browser.startup.homepage_override.mstone", "ignore");
user_pref("trailhead.firstrun.didSeeAboutWelcome", true);
user_pref("datareporting.policy.dataSubmissionPolicyBypassNotification", true);
user_pref("datareporting.healthreport.uploadEnabled", false);
user_pref("toolkit.telemetry.reportingpolicy.firstRun", false);
user_pref("signon.rememberSignons", false);
"#;

    /// The command line that starts `target` showing its fixture.
    fn command_for(target: &str, stop_file: &Path) -> Option<String> {
        match target {
            "win32" => {
                let exe = std::env::current_exe().ok()?;
                Some(format!(
                    "\"{}\" fixture-win32 \"{}\"",
                    exe.display(),
                    stop_file.display()
                ))
            }
            "wpf" => Some(format!(
                "powershell.exe -NoProfile -STA -ExecutionPolicy Bypass -File tools\\spike-s3\\fixture-wpf.ps1 -StopFile \"{}\"",
                stop_file.display()
            )),
            "chrome" | "edge" => {
                let (exe, _) = browser_exe(target)?;
                if !Path::new(exe).exists() {
                    return None;
                }
                let profile = temp_dir(target);
                Some(format!(
                    "\"{exe}\" --user-data-dir=\"{}\" --no-first-run --no-default-browser-check --disable-sync --new-window --window-position={WIN_X},{WIN_Y} --window-size={WIN_W},{WIN_H} {}",
                    profile.display(),
                    page_url()
                ))
            }
            "firefox" => {
                let (exe, _) = browser_exe(target)?;
                if !Path::new(exe).exists() {
                    return None;
                }
                let profile = temp_dir(target);
                let _ = std::fs::write(profile.join("user.js"), FIREFOX_PREFS);
                Some(format!(
                    "\"{exe}\" -no-remote -new-instance -no-deelevate -profile \"{}\" -width {WIN_W} -height {WIN_H} {}",
                    profile.display(),
                    page_url()
                ))
            }
            _ => None,
        }
    }

    struct Found {
        hwnd: HWND,
        pid: u32,
    }

    unsafe extern "system" fn find_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let out = unsafe { &mut *(lparam.0 as *mut Option<Found>) };
        if out.is_some() || !unsafe { IsWindowVisible(hwnd) }.as_bool() {
            return BOOL(1);
        }
        let mut t = [0u16; 256];
        let n = unsafe { GetWindowTextW(hwnd, &mut t) }.max(0) as usize;
        if String::from_utf16_lossy(&t[..n]).starts_with("s3 t=") {
            let mut pid = 0u32;
            unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
            *out = Some(Found { hwnd, pid });
        }
        BOOL(1)
    }

    fn find_fixture_window(limit_ms: f64) -> Option<Found> {
        let t0 = now_ms();
        while now_ms() - t0 < limit_ms {
            let mut out: Option<Found> = None;
            let _ = unsafe { EnumWindows(Some(find_cb), LPARAM(&mut out as *mut _ as isize)) };
            if out.is_some() {
                return out;
            }
            sleep_ms(200);
        }
        None
    }

    /// The field lengths from the fixture's title, `s3 t=<n> p=<n>`.
    fn title_counts(hwnd: HWND) -> (i64, i64) {
        let mut t = [0u16; 256];
        let n = unsafe { GetWindowTextW(hwnd, &mut t) }.max(0) as usize;
        let s = String::from_utf16_lossy(&t[..n]);
        let field = |key: &str| {
            s.split_whitespace()
                .find_map(|w| w.strip_prefix(key))
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(-1)
        };
        (field("t="), field("p="))
    }

    fn window_rect(hwnd: HWND) -> RECT {
        let mut r = RECT::default();
        let _ = unsafe { GetWindowRect(hwnd, &mut r) };
        r
    }

    fn activate(hwnd: HWND) -> bool {
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
        }
        sleep_ms(200);
        if unsafe { GetForegroundWindow() } == hwnd {
            return true;
        }
        send(&[key(VK_MENU, false)]);
        unsafe {
            let _ = SetForegroundWindow(hwnd);
        }
        send(&[key(VK_MENU, true)]);
        sleep_ms(200);
        if unsafe { GetForegroundWindow() } == hwnd {
            return true;
        }
        unsafe {
            let current = GetForegroundWindow();
            let other = GetWindowThreadProcessId(current, None);
            let me = GetCurrentThreadId();
            if other != 0 && other != me {
                let _ = AttachThreadInput(me, other, true);
                let _ = BringWindowToTop(hwnd);
                let _ = SetForegroundWindow(hwnd);
                let _ = AttachThreadInput(me, other, false);
            }
        }
        sleep_ms(200);
        unsafe { GetForegroundWindow() == hwnd }
    }

    /// The point that the page layout puts inside the plain field.
    fn plain_point(hwnd: HWND) -> (i32, i32) {
        let r = window_rect(hwnd);
        (
            (r.left + r.right) / 2,
            r.top + (r.bottom - r.top) * 30 / 100,
        )
    }

    fn center(r: &RECT) -> (i32, i32) {
        ((r.left + r.right) / 2, (r.top + r.bottom) / 2)
    }

    fn median(v: &mut [f64]) -> f64 {
        if v.is_empty() {
            return -1.0;
        }
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let n = v.len();
        if n % 2 == 1 {
            v[n / 2]
        } else {
            (v[n / 2 - 1] + v[n / 2]) / 2.0
        }
    }

    fn max(v: &[f64]) -> f64 {
        v.iter().copied().fold(-1.0, f64::max)
    }

    fn launch(target: &str) -> Option<(Job, Found, PathBuf)> {
        let stop = temp_dir(&format!("{target}-stop")).join("stop");
        let Some(cmd) = command_for(target, &stop) else {
            say(format!("result target={target} present=false"));
            return None;
        };
        let t0 = now_ms();
        let job = match Job::spawn(&cmd) {
            Ok(j) => j,
            Err(e) => {
                say(format!(
                    "result target={target} spawn_error_len={}",
                    e.len()
                ));
                return None;
            }
        };
        let Some(found) = find_fixture_window(60_000.0) else {
            say(format!("result target={target} window_found=false"));
            job.end();
            return None;
        };
        let in_job = job.pids().contains(&found.pid);
        say(format!(
            "launch target={target} window_ms={:.0} window_process_in_job={in_job} job_root_owns_window={}",
            now_ms() - t0,
            found.pid == job.pid
        ));
        if target != "win32" && target != "wpf" {
            unsafe {
                let _ = SetWindowPos(found.hwnd, None, WIN_X, WIN_Y, WIN_W, WIN_H, SWP_NOZORDER);
            }
        }
        sleep_ms(2500);
        Some((job, found, stop))
    }

    fn close(target: &str, job: &Job, hwnd: HWND, stop: &Path) {
        let _ = std::fs::write(stop, b"stop\n");
        if target == "win32" || target == "wpf" {
            unsafe {
                let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            sleep_ms(1500);
        }
        job.end();
        sleep_ms(1000);
    }

    // ---- probe ---------------------------------------------------------

    fn probe(target: &str) {
        let raw: RawLog = Arc::new(Mutex::new(Vec::new()));
        start_raw_sink(Arc::clone(&raw));
        let uia = match uia_attach() {
            Ok(u) => u,
            Err(e) => {
                say(format!("result target={target} uia_error_len={}", e.len()));
                return;
            }
        };
        let Some((job, found, stop)) = launch(target) else {
            uia.detach();
            return;
        };
        let hwnd = found.hwnd;
        let fg = activate(hwnd);

        // First focus after the client attached, before any tree query.
        let (px, py) = plain_point(hwnd);
        let t0 = now_ms();
        click(px, py);
        let first = uia.wait_edit(t0, 3000.0);
        let first_any = uia.events_since(t0).len();
        sleep_ms(800);
        let t1 = now_ms();
        tab();
        let first_pw = uia.wait_edit(t1, 3000.0);
        sleep_ms(500);
        shift_tab();
        sleep_ms(800);
        say(format!(
            "first target={target} foreground={fg} click_events={first_any} first_edit_ms={} first_edit_is_password={:?} first_tab_edit_ms={} first_tab_is_password={:?}",
            first.map(|e| format!("{:.1}", e.t - t0)).unwrap_or("none".into()),
            first.and_then(|e| e.is_password),
            first_pw.map(|e| format!("{:.1}", e.t - t1)).unwrap_or("none".into()),
            first_pw.and_then(|e| e.is_password),
        ));

        // Find both fields by the tree and click the plain one.
        let (plain, secret, edits) = uia.fields(hwnd);
        say(format!(
            "tree target={target} edit_elements={edits} plain_found={} password_found_by_tree={}",
            plain.is_some(),
            secret.is_some()
        ));
        let (cx, cy) = plain.map(|r| center(&r)).unwrap_or((px, py));
        click(cx, cy);
        sleep_ms(800);
        let (start_sync, _) = uia.sync_read();
        let (t_before, p_before) = title_counts(hwnd);
        say(format!(
            "start target={target} focused_is_password={start_sync:?} title_t={t_before} title_p={p_before}"
        ));

        // Latency: Tab to the password field, Shift+Tab back, 20 times each.
        let mut lat_pw = Vec::new();
        let mut lat_plain = Vec::new();
        let mut sync_us = Vec::new();
        let mut es_us = Vec::new();
        let (mut pw_ok, mut plain_ok, mut pw_missing, mut plain_missing) = (0, 0, 0, 0);
        let (mut sync_pw_ok, mut sync_plain_ok) = (0, 0);
        let (mut es_edit_pw, mut es_bit_pw, mut es_edit_plain, mut es_bit_plain) = (0, 0, 0, 0);
        let mut other_types = 0usize;
        for _ in 0..TRIALS {
            for (to_pw, send_fn) in [
                (true, tab as fn() -> u32),
                (false, shift_tab as fn() -> u32),
            ] {
                let t0 = now_ms();
                send_fn();
                let ev = uia.wait_edit(t0, 2000.0);
                other_types += uia
                    .events_since(t0)
                    .iter()
                    .filter(|e| e.control_type != UIA_EditControlTypeId.0)
                    .count();
                sleep_ms(150);
                let (s, us) = uia.sync_read();
                sync_us.push(us);
                let es = es_read();
                es_us.push(es.micros);
                match ev {
                    Some(e) => {
                        let lat = e.t - t0;
                        let right = e.is_password == Some(to_pw);
                        if to_pw {
                            lat_pw.push(lat);
                            pw_ok += right as usize;
                        } else {
                            lat_plain.push(lat);
                            plain_ok += right as usize;
                        }
                    }
                    None => {
                        if to_pw {
                            pw_missing += 1
                        } else {
                            plain_missing += 1
                        }
                    }
                }
                if to_pw {
                    sync_pw_ok += (s == Some(true)) as usize;
                    es_edit_pw += es.edit_class as usize;
                    es_bit_pw += (es.edit_class && es.bit) as usize;
                } else {
                    sync_plain_ok += (s == Some(false)) as usize;
                    es_edit_plain += es.edit_class as usize;
                    es_bit_plain += (es.edit_class && es.bit) as usize;
                }
                sleep_ms(200);
            }
        }
        say(format!(
            "latency target={target} trials={TRIALS} pw_event_right={pw_ok} pw_missing={pw_missing} plain_event_right={plain_ok} plain_missing={plain_missing} non_edit_events={other_types} lat_pw_median_ms={:.1} lat_pw_max_ms={:.1} lat_plain_median_ms={:.1} lat_plain_max_ms={:.1}",
            median(&mut lat_pw.clone()),
            max(&lat_pw),
            median(&mut lat_plain.clone()),
            max(&lat_plain)
        ));
        say(format!(
            "sync target={target} sync_read_pw_right={sync_pw_ok} sync_read_plain_right={sync_plain_ok} sync_read_median_us={:.0} sync_read_max_us={:.0}",
            median(&mut sync_us.clone()),
            max(&sync_us)
        ));
        say(format!(
            "es_password target={target} pw_focus_edit_class={es_edit_pw} pw_focus_bit={es_bit_pw} plain_focus_edit_class={es_edit_plain} plain_focus_bit={es_bit_plain} read_median_us={:.1} read_max_us={:.1}",
            median(&mut es_us.clone()),
            max(&es_us)
        ));

        // Race: Tab and X in one SendInput call, 20 times.
        let mut deltas = Vec::new();
        let (mut first_count, mut within, mut missing, mut no_key) = (0, 0, 0, 0);
        let (mut es_edit_at_key, mut es_bit_at_key) = (0, 0);
        for _ in 0..TRIALS {
            let t0 = now_ms();
            send(&[
                key(VK_TAB, false),
                key(VK_TAB, true),
                key(VK_X, false),
                key(VK_X, true),
            ]);
            let ev = uia.wait_edit(t0, 2000.0);
            sleep_ms(100);
            let k = raw
                .lock()
                .ok()
                .and_then(|v| v.iter().find(|k| k.t >= t0 && k.vkey == VK_X.0).copied());
            match (ev, k) {
                (Some(e), Some(k)) if e.is_password == Some(true) => {
                    let d = e.t - k.t;
                    deltas.push(d);
                    first_count += (d < 0.0) as usize;
                    within += (d <= HOLD_LIMIT_MS) as usize;
                    es_edit_at_key += k.es_at_arrival.edit_class as usize;
                    es_bit_at_key += (k.es_at_arrival.edit_class && k.es_at_arrival.bit) as usize;
                }
                (_, None) => no_key += 1,
                _ => missing += 1,
            }
            sleep_ms(250);
            let t1 = now_ms();
            shift_tab();
            let _ = uia.wait_edit(t1, 2000.0);
            sleep_ms(300);
        }
        let (t_mid, p_mid) = title_counts(hwnd);
        let plain_sent = type_x(5);
        sleep_ms(500);
        let (t_end, p_end) = title_counts(hwnd);
        say(format!(
            "race target={target} trials={TRIALS} measured={} event_before_key={first_count} event_within_{HOLD_LIMIT_MS:.0}ms={within} event_missing_or_wrong={missing} key_not_seen={no_key} delta_median_ms={:.1} delta_max_ms={:.1} es_edit_class_at_key={es_edit_at_key} es_bit_at_key={es_bit_at_key}",
            deltas.len(),
            median(&mut deltas.clone()),
            max(&deltas)
        ));
        say(format!(
            "liveness target={target} password_chars_before={p_before} password_chars_after_race={p_mid} plain_chars_after_race={t_mid} plain_keys_sent={} plain_chars_after_plain_keys={t_end} password_chars_end={p_end}",
            plain_sent / 2
        ));

        let keys = raw.lock().map(|v| v.clone()).unwrap_or_default();
        let null_dev = keys.iter().filter(|k| k.null_device).count();
        let marked = keys.iter().filter(|k| k.marker).count();
        let flag_bits = keys.iter().fold(0u16, |a, k| a | k.flags);
        say(format!(
            "raw target={target} key_downs={} null_device={null_dev} extra_info_marker={marked} flags_union={flag_bits:#x}",
            keys.len()
        ));
        uia.detach();
        close(target, &job, hwnd, &stop);
    }

    // ---- cold: the first move into the password field after attach -----

    fn same_rect(a: &RECT, b: &Option<RECT>) -> bool {
        b.map(|b| {
            (a.left - b.left).abs() <= 2
                && (a.top - b.top).abs() <= 2
                && (a.right - b.right).abs() <= 2
                && (a.bottom - b.bottom).abs() <= 2
        })
        .unwrap_or(false)
    }

    /// One fresh client and one fresh launch: click the plain field, then Tab
    /// and X in one SendInput call, before any tree query. Which element do
    /// the Edit focus events of the next second name, and with which
    /// IsPassword?
    fn cold(target: &str, run: &str) {
        let raw: RawLog = Arc::new(Mutex::new(Vec::new()));
        start_raw_sink(Arc::clone(&raw));
        let uia = match uia_attach() {
            Ok(u) => u,
            Err(e) => {
                say(format!("cold target={target} uia_error_len={}", e.len()));
                return;
            }
        };
        let Some((job, found, stop)) = launch(target) else {
            uia.detach();
            return;
        };
        let hwnd = found.hwnd;
        let fg = activate(hwnd);
        let (px, py) = plain_point(hwnd);
        let tc = now_ms();
        click(px, py);
        let click_ev = uia.wait_edit(tc, 3000.0);
        sleep_ms(700);
        let t0 = now_ms();
        send(&[
            key(VK_TAB, false),
            key(VK_TAB, true),
            key(VK_X, false),
            key(VK_X, true),
        ]);
        sleep_ms(1000);
        let events: Vec<FocusEvent> = uia
            .events_since(t0)
            .into_iter()
            .filter(|e| e.control_type == UIA_EditControlTypeId.0)
            .collect();
        let key_t = raw.lock().ok().and_then(|v| {
            v.iter()
                .find(|k| k.t >= t0 && k.vkey == VK_X.0)
                .map(|k| k.t)
        });
        let (plain, secret, _) = uia.fields(hwnd);
        let name = |e: &FocusEvent| {
            if same_rect(&e.rect, &secret) {
                "password"
            } else if same_rect(&e.rect, &plain) {
                "plain"
            } else {
                "other"
            }
        };
        let seq: Vec<String> = events
            .iter()
            .map(|e| {
                format!(
                    "{}:{}:{:.1}",
                    name(e),
                    match e.is_password {
                        Some(true) => "pw",
                        Some(false) => "clear",
                        None => "none",
                    },
                    key_t.map(|k| e.t - k).unwrap_or(f64::NAN)
                )
            })
            .collect();
        let first_true = events
            .iter()
            .find(|e| e.is_password == Some(true))
            .and_then(|e| key_t.map(|k| e.t - k));
        let wrong_clear = events
            .iter()
            .filter(|e| name(e) == "password" && e.is_password == Some(false))
            .count();
        let (t_chars, p_chars) = title_counts(hwnd);
        say(format!(
            "cold target={target} run={run} foreground={fg} click_event_is_password={:?} edit_events={} password_element_reported_clear={wrong_clear} first_true_after_key_ms={} plain_chars={t_chars} password_chars={p_chars} sequence={}",
            click_ev.and_then(|e| e.is_password),
            events.len(),
            first_true.map(|d| format!("{d:.1}")).unwrap_or("none".into()),
            seq.join(",")
        ));
        uia.detach();
        close(target, &job, hwnd, &stop);
    }

    // ---- cost ----------------------------------------------------------

    fn cost(browser: &str, condition: &str, run: &str) {
        let attach = condition == "uia";
        let Some((job, found, stop)) = launch(browser) else {
            return;
        };
        let fg = activate(found.hwnd);
        let (px, py) = plain_point(found.hwnd);
        click(px, py);
        sleep_ms(8000);
        let uia = if attach {
            match uia_attach() {
                Ok(u) => {
                    let _ = u.sync_read();
                    Some(u)
                }
                Err(e) => {
                    say(format!("cost browser={browser} uia_error_len={}", e.len()));
                    None
                }
            }
        } else {
            None
        };
        sleep_ms(1000);
        let cpu0 = job.cpu_ms();
        let self0 = self_cpu_ms();
        let t0 = now_ms();
        let (mut ws_max, mut private_max) = (0u64, 0u64);
        let mut step = 0u32;
        let mut tabs = 0u32;
        while now_ms() - t0 < 60_000.0 {
            step += 1;
            if step.is_multiple_of(3) {
                if tabs.is_multiple_of(2) {
                    tab();
                } else {
                    shift_tab();
                }
                tabs += 1;
            }
            let (ws, private, _) = job.memory();
            ws_max = ws_max.max(ws);
            private_max = private_max.max(private);
            let next = t0 + f64::from(step) * 1000.0;
            let wait = (next - now_ms()).max(0.0) as u64;
            sleep_ms(wait);
        }
        let cpu1 = job.cpu_ms();
        let self1 = self_cpu_ms();
        let (ws_end, private_end, procs) = job.memory();
        let (_, image) = browser_exe(browser).unwrap_or(("", ""));
        let by_name = processes_named(image);
        let events = uia
            .as_ref()
            .map(|u| u.events_since(t0).len() as i64)
            .unwrap_or(-1);
        say(format!(
            "cost browser={browser} condition={condition} run={run} foreground={fg} window_secs=60 focus_moves={tabs} cpu_ms={:.0} ws_max_bytes={ws_max} ws_end_bytes={ws_end} private_max_bytes={private_max} private_end_bytes={private_end} procs_in_job={procs} procs_by_name={by_name} client_cpu_ms={:.0} client_focus_events={events}",
            cpu1 - cpu0,
            self1 - self0
        ));
        if let Some(u) = uia {
            u.detach();
        }
        close(browser, &job, found.hwnd, &stop);
    }

    // ---- the Win32 fixture ---------------------------------------------

    static EDITS: Mutex<(isize, isize)> = Mutex::new((0, 0));

    fn set_counts_title(hwnd: HWND) {
        let (a, b) = EDITS.lock().map(|g| *g).unwrap_or((0, 0));
        let len = |h: isize| unsafe { GetWindowTextLengthW(HWND(h as *mut core::ffi::c_void)) };
        let title = HSTRING::from(format!("s3 t={} p={}", len(a), len(b)));
        let _ = unsafe { SetWindowTextW(hwnd, &title) };
    }

    unsafe extern "system" fn fixture_proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        match m {
            WM_COMMAND if (w.0 >> 16) & 0xFFFF == EN_CHANGE as usize => {
                set_counts_title(h);
                LRESULT(0)
            }
            WM_SETFOCUS => {
                let (a, _) = EDITS.lock().map(|g| *g).unwrap_or((0, 0));
                let _ = unsafe {
                    windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(HWND(
                        a as *mut core::ffi::c_void,
                    )))
                };
                LRESULT(0)
            }
            WM_DESTROY => {
                unsafe { PostQuitMessage(0) };
                LRESULT(0)
            }
            _ => unsafe { DefWindowProcW(h, m, w, l) },
        }
    }

    fn fixture_win32(stop_file: &str) {
        let class = w!("SpikeS3Win32Fixture");
        let Ok(inst) = (unsafe { GetModuleHandleW(None) }) else {
            return;
        };
        let wc = WNDCLASSW {
            lpfnWndProc: Some(fixture_proc),
            hInstance: inst.into(),
            lpszClassName: class,
            hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as isize as *mut core::ffi::c_void),
            ..Default::default()
        };
        unsafe { RegisterClassW(&wc) };
        let Ok(main) = (unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class,
                w!("s3 t=0 p=0"),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                WIN_X,
                WIN_Y,
                540,
                300,
                None,
                None,
                Some(inst.into()),
                None,
            )
        }) else {
            return;
        };
        let edit = |y: i32, extra: u32, id: isize| unsafe {
            CreateWindowExW(
                WS_EX_CLIENTEDGE,
                w!("EDIT"),
                w!(""),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32 | extra),
                10,
                y,
                500,
                100,
                Some(main),
                Some(HMENU(id as *mut core::ffi::c_void)),
                Some(inst.into()),
                None,
            )
        };
        let plain = edit(10, 0, 1).map(|h| h.0 as isize).unwrap_or(0);
        let secret = edit(130, ES_PASSWORD as u32, 2)
            .map(|h| h.0 as isize)
            .unwrap_or(0);
        if let Ok(mut g) = EDITS.lock() {
            *g = (plain, secret);
        }
        let stop = PathBuf::from(stop_file);
        let mut msg = MSG::default();
        loop {
            while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
                if msg.message == WM_QUIT {
                    return;
                }
                if !unsafe { IsDialogMessageW(main, &msg) }.as_bool() {
                    unsafe {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
            }
            if stop.exists() {
                unsafe {
                    let _ = DestroyWindow(main);
                }
            }
            unsafe {
                MsgWaitForMultipleObjects(None, false, 100, QS_ALLINPUT);
            }
        }
    }
}
