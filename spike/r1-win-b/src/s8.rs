// S8: read the address field of the focused browser through UI Automation, setting no
// attribute; its cost; the browser's CPU and memory in 60 s windows before and after the first
// read; the settle time after a cold launch. Also the Settings toggle of S5 (startup apps).
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::util::*;
use windows::core::{w, Interface, BSTR, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Threading::*;
use windows::Win32::System::Variant::{VARIANT, VT_I4};
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::*;

struct Server {
    port: u16,
    requests: Arc<AtomicUsize>,
    first_us: Arc<AtomicI64>,
}

fn serve() -> Server {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let requests = Arc::new(AtomicUsize::new(0));
    let first_us = Arc::new(AtomicI64::new(0));
    let (r2, f2) = (requests.clone(), first_us.clone());
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let (r3, f3) = (r2.clone(), f2.clone());
            std::thread::spawn(move || {
                let mut s = s;
                let mut buf = [0u8; 2048];
                let n = s.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                if req.starts_with("GET /fx-") {
                    if r3.fetch_add(1, Ordering::SeqCst) == 0 {
                        f3.store(now_us(), Ordering::SeqCst);
                    }
                }
                let body = "<!doctype html><html><head><title>R1 fixture</title></head><body><p>Fixture page of a throwaway probe.</p></body></html>";
                let resp = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                let _ = s.write_all(resp.as_bytes());
            });
        }
    });
    Server { port, requests, first_us }
}

fn uia() -> IUIAutomation {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).unwrap()
    }
}

fn find_edit(u: &IUIAutomation, h: HWND) -> windows::core::Result<IUIAutomationElement> {
    find_field(u, h).map(|(e, _, _)| e)
}

/// The address field: the first Edit with a value, else the first ComboBox or Edit among all
/// descendants whose value pattern gives a non-empty value. Returns (element, kind, candidates).
fn find_field(u: &IUIAutomation, h: HWND) -> windows::core::Result<(IUIAutomationElement, &'static str, i32)> {
    unsafe {
        let el = u.ElementFromHandle(h)?;
        let cond_for = |id: i32| -> windows::core::Result<IUIAutomationCondition> {
            let mut v = VARIANT::default();
            (*v.Anonymous.Anonymous).vt = VT_I4;
            (*v.Anonymous.Anonymous).Anonymous.lVal = id;
            u.CreatePropertyCondition(UIA_ControlTypePropertyId, &v)
        };
        let edit = cond_for(UIA_EditControlTypeId.0)?;
        if let Ok(e) = el.FindFirst(TreeScope_Descendants, &edit) {
            if value_of(&e).map(|v| !v.is_empty()).unwrap_or(false) {
                return Ok((e, "edit_first", 1));
            }
        }
        let combo = cond_for(UIA_ComboBoxControlTypeId.0)?;
        let either = u.CreateOrCondition(&edit, &combo)?;
        let arr = el.FindAll(TreeScope_Descendants, &either)?;
        let n = arr.Length().unwrap_or(0);
        for i in 0..n {
            if let Ok(e) = arr.GetElement(i) {
                if value_of(&e).map(|v| !v.is_empty()).unwrap_or(false) {
                    let kind = if e.CurrentControlType().map(|t| t == UIA_ComboBoxControlTypeId).unwrap_or(false) { "combobox" } else { "edit_any" };
                    return Ok((e, kind, n));
                }
            }
        }
        Err(windows::core::Error::from_hresult(windows::core::HRESULT(0x80004005u32 as i32)))
    }
}

fn value_of(e: &IUIAutomationElement) -> windows::core::Result<String> {
    unsafe {
        let vp: IUIAutomationValuePattern = e.GetCurrentPatternAs(UIA_ValuePatternId)?;
        let v: BSTR = vp.CurrentValue()?;
        Ok(v.to_string())
    }
}

/// The largest visible top-level window of the browser's tree.
fn browser_window(tree: &[u32]) -> Option<HWND> {
    let mut best: Option<(i64, HWND)> = None;
    for w in windows_all(false) {
        if w.top && w.visible && tree.contains(&w.pid) {
            let h = hwnd(w.hwnd);
            let mut rc = RECT::default();
            unsafe {
                let _ = GetWindowRect(h, &mut rc);
            }
            let area = ((rc.right - rc.left) as i64) * ((rc.bottom - rc.top) as i64);
            if best.map(|b| area > b.0).unwrap_or(true) {
                best = Some((area, h));
            }
        }
    }
    best.map(|b| b.1)
}

struct Window {
    cpu_ms: f64,
    max_ws: u64,
    end_ws: u64,
    procs_start: usize,
    procs_end: usize,
    churn: usize,
}

/// A sampling window: CPU time of the tree's processes that live through it, and its working set.
fn measure(root: u32, secs: u64) -> Window {
    let tree0 = descendants(root, &snapshot());
    let mut cpu0: HashMap<u32, u64> = HashMap::new();
    for &p in &tree0 {
        if let Some((_, c)) = open_query(p).and_then(|h| times(h.0)) {
            cpu0.insert(p, c);
        }
    }
    let mut max_ws = 0u64;
    let t = Instant::now();
    let mut seen: std::collections::HashSet<u32> = tree0.iter().copied().collect();
    while t.elapsed() < Duration::from_secs(secs) {
        let tree = descendants(root, &snapshot());
        let mut sum = 0;
        for &p in &tree {
            seen.insert(p);
            if let Some((ws, _, _)) = open_query(p).and_then(|h| memory(h.0)) {
                sum += ws;
            }
        }
        max_ws = max_ws.max(sum);
        std::thread::sleep(Duration::from_millis(1000));
    }
    let tree1 = descendants(root, &snapshot());
    let mut cpu = 0u64;
    let mut end_ws = 0u64;
    for &p in &tree1 {
        let h = open_query(p);
        if let (Some(c0), Some((_, c1))) = (cpu0.get(&p), h.as_ref().and_then(|h| times(h.0))) {
            cpu += c1.saturating_sub(*c0);
        }
        if let Some((ws, _, _)) = h.as_ref().and_then(|h| memory(h.0)) {
            end_ws += ws;
        }
    }
    let churn = seen.len() - tree0.iter().filter(|p| tree1.contains(p)).count();
    Window { cpu_ms: cpu as f64 / 10_000.0, max_ws, end_ws, procs_start: tree0.len(), procs_end: tree1.len(), churn }
}

fn launch(kind: &str, exe: &str, prof: &str, url: &str) -> Option<(OwnedHandle, u32)> {
    std::fs::create_dir_all(prof).ok()?;
    let cmd = if kind == "firefox" {
        let prefs = [
            "user_pref(\"browser.shell.checkDefaultBrowser\", false);",
            "user_pref(\"browser.aboutwelcome.enabled\", false);",
            "user_pref(\"browser.startup.homepage_override.mstone\", \"ignore\");",
            "user_pref(\"datareporting.policy.dataSubmissionPolicyBypassNotification\", true);",
            "user_pref(\"trailhead.firstrun.didSeeAboutWelcome\", true);",
            "user_pref(\"browser.shell.didSkipDefaultBrowserCheckOnFirstRun\", true);",
            "user_pref(\"toolkit.telemetry.reportingpolicy.firstRun\", false);",
        ];
        std::fs::write(format!("{prof}\\user.js"), prefs.join("\r\n")).ok()?;
        format!("\"{exe}\" -no-remote -new-instance -profile \"{prof}\" {url}")
    } else {
        format!("\"{exe}\" --user-data-dir=\"{prof}\" --no-first-run --no-default-browser-check --disable-sync --new-window {url}")
    };
    spawn(&cmd, PROCESS_CREATION_FLAGS(0)).ok()
}

fn read_once(u: &IUIAutomation, tree: &[u32], token: &str) -> (bool, bool, usize, f64, bool) {
    let fg = unsafe { GetForegroundWindow() };
    let fg_in = tree.contains(&pid_of(fg));
    let h = if fg_in { fg } else { browser_window(tree).unwrap_or_default() };
    let t = now_us();
    let r = find_edit(u, h).and_then(|e| value_of(&e));
    let ms = (now_us() - t) as f64 / 1000.0;
    match r {
        Ok(v) => (true, v.contains(token), v.len(), ms, fg_in),
        Err(_) => (false, false, 0, ms, fg_in),
    }
}

pub fn site(args: &[String]) {
    let kind = arg(args, "--browser").unwrap();
    let exe = arg(args, "--exe").unwrap();
    let dir = arg(args, "--dir").unwrap();
    let secs: u64 = arg(args, "--secs").and_then(|s| s.parse().ok()).unwrap_or(60);
    let reads: usize = arg(args, "--reads").and_then(|s| s.parse().ok()).unwrap_or(20);
    let present = std::path::Path::new(&exe).exists();
    println!("S8 browser={kind} present={present}");
    if !present {
        return;
    }
    let srv = serve();
    let u = uia();

    // Part 1: cold launch, no UIA call until the measuring windows are done.
    let token = format!("fx-{kind}-a");
    let url = format!("http://127.0.0.1:{}/{token}.html", srv.port);
    let mut attempt = 0;
    let (h, pid) = loop {
        attempt += 1;
        let Some((h, pid)) = launch(&kind, &exe, &format!("{dir}\\prof-{kind}-a{attempt}"), &url) else {
            println!("S8 browser={kind} part=effect started=false");
            return;
        };
        let t = Instant::now();
        while srv.requests.load(Ordering::SeqCst) == 0 && t.elapsed() < Duration::from_secs(45) {
            std::thread::sleep(Duration::from_millis(100));
        }
        if srv.requests.load(Ordering::SeqCst) > 0 || attempt >= 2 {
            break (h, pid);
        }
        terminate(h.0);
        wait(h.0, 5000);
    };
    let requested = srv.requests.load(Ordering::SeqCst) > 0;
    std::thread::sleep(Duration::from_secs(10));
    let a = measure(pid, secs);
    let b = measure(pid, secs);
    let c = measure(pid, secs);
    let tree = descendants(pid, &snapshot());
    let (ok1, match1, len1, ms1, fg1) = read_once(&u, &tree, &token);
    let kind1 = browser_window(&tree).and_then(|h| find_field(&u, h).ok()).map(|(_, k, n)| format!("{k}:{n}")).unwrap_or_else(|| "none".into());
    let mut locate = Vec::new();
    let mut matches = 0;
    for _ in 1..reads {
        let (ok, m, _, ms, _) = read_once(&u, &tree, &token);
        if ok {
            locate.push(ms);
        }
        if m {
            matches += 1;
        }
    }
    // Reads of the value of an element already found (what a reader that caches it pays).
    let fgh = browser_window(&tree).unwrap_or_default();
    let mut cached = Vec::new();
    if let Ok(e) = find_edit(&u, fgh) {
        for _ in 0..reads {
            let t = now_us();
            if value_of(&e).is_ok() {
                cached.push((now_us() - t) as f64 / 1000.0);
            }
        }
    }
    let after = measure(pid, secs);
    let after2 = measure(pid, secs);
    println!(
        "S8 browser={kind} part=effect launches={attempt} page_requested={requested} field_kind={kind1} first_read_ok={ok1} first_read_has_fixture_token={match1} first_read_value_len={len1} first_read_ms={ms1:.1} browser_was_foreground={fg1} later_reads_ok={} later_reads_with_token={matches} locate_and_read_median_ms={:.1} cached_value_read_median_ms={:.2} cached_reads={}",
        locate.len(),
        median(&mut locate),
        median(&mut cached),
        cached.len()
    );
    for (name, w) in [("warm_up", &a), ("before_b", &b), ("before_c", &c), ("after_a", &after), ("after_b", &after2)] {
        println!(
            "S8 browser={kind} window={name} secs={secs} cpu_ms={:.1} max_summed_working_set_bytes={} end_summed_working_set_bytes={} processes_start={} processes_end={} processes_churn={}",
            w.cpu_ms, w.max_ws, w.end_ws, w.procs_start, w.procs_end, w.churn
        );
    }
    terminate(h.0);
    wait(h.0, 10000);
    std::thread::sleep(Duration::from_secs(3));

    // Part 2: cold launch with a new profile, reading every 250 ms from the launch on.
    let token2 = format!("fx-{kind}-b");
    let url2 = format!("http://127.0.0.1:{}/{token2}.html", srv.port);
    let before_req = srv.requests.load(Ordering::SeqCst);
    let t_launch = now_us();
    let Some((h2, pid2)) = launch(&kind, &exe, &format!("{dir}\\prof-{kind}-b"), &url2) else {
        println!("S8 browser={kind} part=settle started=false");
        return;
    };
    let t = Instant::now();
    let mut settle: Option<f64> = None;
    let mut polls = 0;
    let mut req_at: Option<f64> = None;
    while t.elapsed() < Duration::from_secs(60) {
        if req_at.is_none() && srv.requests.load(Ordering::SeqCst) > before_req {
            req_at = Some((now_us() - t_launch) as f64 / 1000.0);
        }
        let tree = descendants(pid2, &snapshot());
        let (_, m, _, _, _) = read_once(&u, &tree, &token2);
        polls += 1;
        if m {
            settle = Some((now_us() - t_launch) as f64 / 1000.0);
            break;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    println!(
        "S8 browser={kind} part=settle polls={polls} read_with_token={} settle_ms_from_launch={} page_request_ms_from_launch={}",
        settle.is_some(),
        settle.map(|v| format!("{v:.0}")).unwrap_or("-".into()),
        req_at.map(|v| format!("{v:.0}")).unwrap_or("-".into())
    );
    terminate(h2.0);
    wait(h2.0, 10000);
}

/// S5: switch the startup entry off in Settings > Apps > Startup through UI Automation.
pub fn startup_toggle(args: &[String]) {
    let needles: Vec<String> = arg(args, "--needles").unwrap_or_else(|| "r1spike,r1starter".into()).split(',').map(|s| s.to_lowercase()).collect();
    let name = arg(args, "--name").unwrap_or_else(|| "R1SpikeHalvo".into());
    let u = uia();
    unsafe {
        ShellExecuteW(None, w!("open"), w!("ms-settings:startupapps"), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
    }
    let t = Instant::now();
    let mut toggles_seen = 0;
    let mut named_seen = 0;
    let mut toggled = 0;
    let mut settings_window = false;
    let mut state_before = -1;
    let mut state_after = -1;
    'outer: while t.elapsed() < Duration::from_secs(45) {
        std::thread::sleep(Duration::from_millis(1500));
        for wdw in windows_all(false) {
            if !(wdw.top && wdw.visible && wdw.class == "ApplicationFrameWindow") {
                continue;
            }
            settings_window = true;
            unsafe {
                let Ok(root) = u.ElementFromHandle(hwnd(wdw.hwnd)) else { continue };
                let Ok(all) = u.CreateTrueCondition() else { continue };
                let Ok(arr) = root.FindAll(TreeScope_Descendants, &all) else { continue };
                let n = arr.Length().unwrap_or(0);
                toggles_seen = 0;
                named_seen = 0;
                for i in 0..n {
                    let Ok(e) = arr.GetElement(i) else { continue };
                    let nm = e.CurrentName().map(|b| b.to_string().to_lowercase()).unwrap_or_default();
                    let hit = needles.iter().any(|nd| nm.contains(nd.as_str()));
                    if hit {
                        named_seen += 1;
                    }
                    if let Ok(tp) = e.GetCurrentPatternAs::<IUIAutomationTogglePattern>(UIA_TogglePatternId) {
                        toggles_seen += 1;
                        if hit {
                            state_before = tp.CurrentToggleState().map(|s| s.0).unwrap_or(-1);
                            if tp.Toggle().is_ok() {
                                toggled += 1;
                                std::thread::sleep(Duration::from_millis(2000));
                                state_after = tp.CurrentToggleState().map(|s| s.0).unwrap_or(-1);
                                break 'outer;
                            }
                        }
                    }
                }
            }
        }
    }
    let ap = crate::s5::startup_approved(&name);
    println!(
        "S5 startup_toggle settings_window={settings_window} toggles_seen={toggles_seen} elements_with_needle={named_seen} toggled={toggled} toggle_state_before={state_before} toggle_state_after={state_after} approved_present_after={} approved_first_byte_after={}",
        ap.is_some(),
        ap.as_ref().and_then(|b| b.first().copied()).map(|b| format!("0x{b:02x}")).unwrap_or("-".into())
    );
    let _ = <IUIAutomation as Interface>::IID;
}
