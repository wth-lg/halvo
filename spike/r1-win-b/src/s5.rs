// S5: the stand-in daemon (`child`), the lifetime trials (`lifetime`), the Run value (`runvalue`)
// and the named-pipe DACL (`pipe-server`, `pipe-client`).
use std::ffi::c_void;
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use crate::util::*;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Security::Authorization::*;
use windows::Win32::Security::*;
use windows::Win32::Storage::FileSystem::*;
use windows::Win32::System::Console::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Pipes::*;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::WindowsAndMessaging::*;

struct ChildState {
    marker: String,
    flush_ms: u64,
}
static CHILD: OnceLock<ChildState> = OnceLock::new();
static STOPPING: AtomicBool = AtomicBool::new(false);

/// The stop path: one simulated final flush (1 MiB written and flushed, then `flush_ms` more),
/// with a line before and after. Runs once, whoever calls it first.
fn stop_path(reason: &str) {
    let st = CHILD.get().unwrap();
    if STOPPING.swap(true, Ordering::SeqCst) {
        append_line(&st.marker, &format!("child_event=also reason={reason} t_us={}", now_us()));
        return;
    }
    append_line(&st.marker, &format!("child_event=stop_begin reason={reason} t_us={}", now_us()));
    let flush_file = format!("{}.flush", st.marker);
    if let Ok(mut f) = std::fs::File::create(&flush_file) {
        use std::io::Write;
        let _ = f.write_all(&vec![0x5au8; 1 << 20]);
        let _ = f.sync_all();
    }
    std::thread::sleep(Duration::from_millis(st.flush_ms));
    append_line(&st.marker, &format!("child_event=stop_done reason={reason} t_us={}", now_us()));
    std::process::exit(0);
}

unsafe extern "system" fn child_wndproc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_QUERYENDSESSION => {
            append_line(&CHILD.get().unwrap().marker, &format!("child_event=queryendsession t_us={}", now_us()));
            LRESULT(1)
        }
        WM_ENDSESSION => {
            if wp.0 != 0 {
                stop_path("endsession");
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            stop_path("close");
            LRESULT(0)
        }
        _ => DefWindowProcW(h, msg, wp, lp),
    }
}

unsafe extern "system" fn ctrl_handler(t: u32) -> windows::core::BOOL {
    let reason = match t {
        CTRL_C_EVENT => "ctrl_c",
        CTRL_BREAK_EVENT => "ctrl_break",
        CTRL_CLOSE_EVENT => "ctrl_close",
        CTRL_LOGOFF_EVENT => "ctrl_logoff",
        CTRL_SHUTDOWN_EVENT => "ctrl_shutdown",
        _ => "ctrl_other",
    };
    stop_path(reason);
    TRUE
}

pub fn child(args: &[String]) {
    let marker = arg(args, "--marker").unwrap_or_default();
    let flush_ms = arg(args, "--flush-ms").and_then(|s| s.parse().ok()).unwrap_or(200);
    let ppid: u32 = arg(args, "--parent-pid").and_then(|s| s.parse().ok()).unwrap_or(0);
    let _ = CHILD.set(ChildState { marker: marker.clone(), flush_ms });
    unsafe {
        let _ = SetConsoleCtrlHandler(Some(ctrl_handler), true);
    }
    let stdin = unsafe { GetStdHandle(STD_INPUT_HANDLE).unwrap_or_default() };
    let stdin_is_pipe = unsafe { GetFileType(stdin) == FILE_TYPE_PIPE };
    // The parent's process handle, opened by pid at start; its creation time must precede ours.
    let parent = unsafe { OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, false, ppid).ok() };
    let parent_older = match (parent.and_then(|h| times(h).map(|t| t.0)), times(unsafe { GetCurrentProcess() }).map(|t| t.0)) {
        (Some(a), Some(b)) => a <= b,
        _ => false,
    };
    append_line(&marker, &format!("child_event=started stdin_is_pipe={stdin_is_pipe} parent_handle={} parent_older={parent_older} t_us={}", parent.is_some(), now_us()));
    let stdin_raw = stdin.0 as isize;
    std::thread::spawn(move || {
        let h = HANDLE(stdin_raw as *mut c_void);
        let mut buf = [0u8; 64];
        loop {
            let mut n = 0u32;
            let r = unsafe { ReadFile(h, Some(&mut buf), Some(&mut n), None) };
            if r.is_err() || n == 0 {
                stop_path("eof");
                return;
            }
        }
    });
    if let Some(p) = parent {
        let raw = p.0 as isize;
        std::thread::spawn(move || {
            unsafe { WaitForSingleObject(HANDLE(raw as *mut c_void), INFINITE) };
            stop_path("parent");
        });
    }
    unsafe {
        let hinst = GetModuleHandleW(None).unwrap_or_default();
        let wc = WNDCLASSW { lpfnWndProc: Some(child_wndproc), hInstance: hinst.into(), lpszClassName: w!("R1SpikeChildEnd"), ..Default::default() };
        RegisterClassW(&wc);
        let _ = CreateWindowExW(WINDOW_EX_STYLE(0), w!("R1SpikeChildEnd"), w!(""), WS_OVERLAPPEDWINDOW, 0, 0, 0, 0, None, None, Some(hinst.into()), None);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

pub fn sleep(args: &[String]) {
    let s: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(60);
    std::thread::sleep(Duration::from_secs(s));
}

fn read_kv(path: &str, event_key: &str, event: &str, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    for line in text.lines() {
        let toks: Vec<&str> = line.split(' ').collect();
        if toks.iter().any(|t| *t == format!("{event_key}={event}")) {
            for t in &toks {
                if let Some(v) = t.strip_prefix(&format!("{key}=")) {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

fn find_window(class: &str, pid: u32) -> Option<HWND> {
    windows_all(false).into_iter().find(|w| w.pid == pid && w.class == class).map(|w| hwnd(w.hwnd))
}

fn send(h: HWND, msg: u32, wp: usize, lp: isize, timeout: u32) -> (bool, i64) {
    let t0 = now_us();
    let mut res = 0usize;
    let r = unsafe { SendMessageTimeoutW(h, msg, WPARAM(wp), LPARAM(lp), SMTO_ABORTIFHUNG, timeout, Some(&mut res)) };
    (r.0 != 0, now_us() - t0)
}

/// One trial: start the stand-in app, end it one way, read what the stand-in daemon wrote.
fn trial(dir: &str, starter: &str, child: &str, form: &str, end: &str, flush_ms: u64, rep: usize) -> String {
    let tag = format!("{form}-{end}-{flush_ms}-{rep}");
    let marker = format!("{dir}\\m-{tag}.txt");
    let ready = format!("{dir}\\r-{tag}.txt");
    let leak = if end == "leakkill" { " --leak" } else { "" };
    let cmd = format!("\"{starter}\" --background --child \"{child}\" --marker \"{marker}\" --ready \"{ready}\" --form {form} --flush-ms {flush_ms}{leak}");
    let Ok((sh, _spid)) = spawn(&cmd, PROCESS_CREATION_FLAGS(0)) else {
        return format!("S5 lifetime form={form} end={end} flush_ms={flush_ms} rep={rep} started=false");
    };
    let t_start = Instant::now();
    let mut child_pid = 0u32;
    let mut leak_pid = 0u32;
    let mut starter_hwnd = 0isize;
    while t_start.elapsed() < Duration::from_secs(15) {
        if let Ok(t) = std::fs::read_to_string(&ready) {
            for tok in t.split_whitespace() {
                if let Some(v) = tok.strip_prefix("child_pid=") { child_pid = v.parse().unwrap_or(0); }
                if let Some(v) = tok.strip_prefix("leak_pid=") { leak_pid = v.parse().unwrap_or(0); }
                if let Some(v) = tok.strip_prefix("starter_hwnd=") { starter_hwnd = v.parse().unwrap_or(0); }
            }
            if child_pid != 0 && read_kv(&marker, "child_event", "started", "t_us").is_some() && find_window("R1SpikeChildEnd", child_pid).is_some() {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let ch = unsafe { OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE, false, child_pid).ok().map(OwnedHandle) };
    let child_window = find_window("R1SpikeChildEnd", child_pid);
    let mut extra = String::new();
    let t0 = now_us();
    match end {
        "kill" | "leakkill" => {
            terminate(sh.0);
        }
        _ => {
            let sw = hwnd(starter_hwnd);
            let cw = child_window.unwrap_or_default();
            // The system's order: WM_QUERYENDSESSION to every app, then WM_ENDSESSION; a process
            // may be ended once its WM_ENDSESSION returns.
            let (q1, _) = send(sw, WM_QUERYENDSESSION, 0, ENDSESSION_LOGOFF as isize, 5000);
            let (q2, _) = send(cw, WM_QUERYENDSESSION, 0, ENDSESSION_LOGOFF as isize, 5000);
            let cw_raw = cw.0 as isize;
            let th = std::thread::spawn(move || send(hwnd(cw_raw), WM_ENDSESSION, 1, ENDSESSION_LOGOFF as isize, 20000));
            let (e1, e1_us) = send(sw, WM_ENDSESSION, 1, ENDSESSION_LOGOFF as isize, 5000);
            terminate(sh.0);
            let t_term = now_us();
            let (e2, e2_us) = th.join().unwrap_or((false, 0));
            extra = format!(" query_starter={q1} query_child={q2} endsession_starter_ok={e1} starter_handler_ms={:.1} endsession_child_returned={e2} child_handler_ms={:.1} starter_ended_after_ms={:.1}", e1_us as f64 / 1000.0, e2_us as f64 / 1000.0, (t_term - t0) as f64 / 1000.0);
        }
    }
    let exited = ch.as_ref().map(|h| wait(h.0, 15000)).unwrap_or(false);
    let t_exit_seen = now_us();
    let code = ch.as_ref().and_then(|h| exit_code(h.0)).unwrap_or(u32::MAX);
    let mut left = false;
    if !exited {
        if let Some(h) = &ch {
            left = terminate(h.0);
        }
    }
    if leak_pid != 0 {
        // The leaker is this trial's own process: started by our starter after t_start.
        if let Some(h) = unsafe { OpenProcess(PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION, false, leak_pid).ok().map(OwnedHandle) } {
            terminate(h.0);
        }
    }
    let begin = read_kv(&marker, "child_event", "stop_begin", "t_us").and_then(|v| v.parse::<i64>().ok());
    let done = read_kv(&marker, "child_event", "stop_done", "t_us").and_then(|v| v.parse::<i64>().ok());
    let reason = read_kv(&marker, "child_event", "stop_begin", "reason").unwrap_or_else(|| "-".into());
    let ms = |v: Option<i64>| v.map(|x| format!("{:.1}", (x - t0) as f64 / 1000.0)).unwrap_or_else(|| "-".into());
    format!(
        "S5 lifetime form={form} end={end} flush_ms={flush_ms} rep={rep} child_started={} stop_begin={} stop_done={} reason={reason} begin_after_ms={} done_after_ms={} child_exited={exited} exit_code={code} exit_seen_after_ms={:.1} child_killed_by_probe={left}{extra}",
        child_pid != 0,
        begin.is_some(),
        done.is_some(),
        ms(begin),
        ms(done),
        (t_exit_seen - t0) as f64 / 1000.0
    )
}

pub fn lifetime(args: &[String]) {
    let dir = arg(args, "--dir").unwrap();
    let starter = arg(args, "--starter").unwrap();
    let child = arg(args, "--child").unwrap();
    let reps: usize = arg(args, "--reps").and_then(|s| s.parse().ok()).unwrap_or(10);
    let cases: &[(&str, &str, u64)] = &[
        ("handle", "kill", 200),
        ("handle", "leakkill", 200),
        ("handle", "session", 200),
        ("handle", "session", 2500),
        ("job", "kill", 200),
        ("job", "session", 200),
        ("job", "session", 2500),
    ];
    for &(form, end, flush) in cases {
        for rep in 0..reps {
            println!("{}", trial(&dir, &starter, &child, form, end, flush, rep));
        }
    }
}

/// The Run value as the account that runs this: write, read back, start the exact string,
/// check the started app and its child, then remove the value.
pub fn runvalue(args: &[String]) {
    use winreg::enums::*;
    use winreg::RegKey;
    let dir = arg(args, "--dir").unwrap();
    let starter = arg(args, "--starter").unwrap();
    let child = arg(args, "--child").unwrap();
    let name = arg(args, "--name").unwrap_or_else(|| "R1SpikeHalvo".into());
    let keep = flag(args, "--keep");
    let marker = format!("{dir}\\m-runvalue.txt");
    let ready = format!("{dir}\\r-runvalue.txt");
    let value = format!("\"{starter}\" --background --child \"{child}\" --marker \"{marker}\" --ready \"{ready}\" --form handle --flush-ms 200");
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let path = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
    let w = hkcu.create_subkey(path).and_then(|(k, _)| k.set_value(&name, &value));
    let read: Option<String> = hkcu.open_subkey(path).ok().and_then(|k| k.get_value(&name).ok());
    let raw = hkcu.open_subkey(path).ok().and_then(|k| k.get_raw_value(&name).ok());
    let is_sz = raw.as_ref().map(|r| r.vtype == REG_SZ).unwrap_or(false);
    let tok = self_token();
    let me_elev = tok.as_ref().and_then(|t| token_elevated(t.0));
    let me_il = tok.as_ref().and_then(|t| token_integrity_rid(t.0));
    println!(
        "S5 runvalue write_ok={} read_back_equal={} type_reg_sz={is_sz} value_chars={} under_260={} writer_elevated={:?} writer_integrity_rid=0x{:x}",
        w.is_ok(),
        read.as_deref() == Some(value.as_str()),
        value.encode_utf16().count(),
        value.encode_utf16().count() < 260,
        me_elev,
        me_il.unwrap_or(0)
    );
    let approved_before = startup_approved(&name);
    // Start the exact string that was read back, as one command line, with no console flag and
    // the default window state, under this account's own token.
    let started = spawn(read.as_deref().unwrap_or(""), PROCESS_CREATION_FLAGS(0));
    let mut line = String::from("S5 runvalue_start");
    if let Ok((h, spid)) = &started {
        let t = Instant::now();
        let mut child_pid = 0u32;
        while t.elapsed() < Duration::from_secs(15) {
            if let Ok(txt) = std::fs::read_to_string(&ready) {
                for tok in txt.split_whitespace() {
                    if let Some(v) = tok.strip_prefix("child_pid=") {
                        child_pid = v.parse().unwrap_or(0);
                    }
                }
            }
            if child_pid != 0 && read_kv(&marker, "child_event", "started", "t_us").is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        std::thread::sleep(Duration::from_millis(500));
        let procs = snapshot();
        let kids: Vec<&Proc> = procs.iter().filter(|p| p.ppid == *spid).collect();
        let kid_is_child = kids.iter().any(|p| p.pid == child_pid);
        let st = token_of(*spid);
        let ct = token_of(child_pid);
        let wins = windows_all(false);
        let vis_starter = wins.iter().filter(|w| w.pid == *spid && w.visible).count();
        let vis_child = wins.iter().filter(|w| w.pid == child_pid && w.visible).count();
        let console_wins = wins.iter().filter(|w| (w.pid == child_pid || w.pid == *spid) && w.class == "ConsoleWindowClass").count();
        let stdin_pipe = read_kv(&marker, "child_event", "started", "stdin_is_pipe").unwrap_or_default();
        line += &format!(
            " started=true app_alive={} children_of_app={} one_child_is_daemon={kid_is_child} token_elevated={:?} app_integrity_rid=0x{:x} child_token_elevated={:?} visible_windows_app={vis_starter} visible_windows_child={vis_child} console_windows={console_wins} child_stdin_is_pipe={stdin_pipe}",
            !wait(h.0, 0),
            kids.len(),
            st.as_ref().and_then(|t| token_elevated(t.0)),
            st.as_ref().and_then(|t| token_integrity_rid(t.0)).unwrap_or(0),
            ct.as_ref().and_then(|t| token_elevated(t.0)),
        );
        terminate(h.0);
        let ch = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, child_pid).ok().map(OwnedHandle) };
        let gone = ch.as_ref().map(|c| wait(c.0, 10000)).unwrap_or(true);
        line += &format!(" child_gone_after_app_kill={gone} child_flushed={}", read_kv(&marker, "child_event", "stop_done", "t_us").is_some());
    } else {
        line += " started=false";
    }
    println!("{line}");
    println!("S5 runvalue_startup_approved before_present={} before_first_byte={}", approved_before.is_some(), approved_before.and_then(|b| b.first().copied()).map(|b| b.to_string()).unwrap_or("-".into()));
    if !keep {
        let del = hkcu.open_subkey_with_flags(path, KEY_SET_VALUE).and_then(|k| k.delete_value(&name));
        let after: Option<String> = hkcu.open_subkey(path).ok().and_then(|k| k.get_value(&name).ok());
        println!("S5 runvalue_remove delete_ok={} value_gone={}", del.is_ok(), after.is_none());
    }
}

/// The StartupApproved\Run value of that name, if any (bytes).
pub fn startup_approved(name: &str) -> Option<Vec<u8>> {
    use winreg::enums::*;
    use winreg::RegKey;
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run")
        .ok()
        .and_then(|k| k.get_raw_value(name).ok())
        .map(|r| r.bytes.to_vec())
}

pub fn startup_state(args: &[String]) {
    use winreg::enums::*;
    use winreg::RegKey;
    let name = arg(args, "--name").unwrap_or_else(|| "R1SpikeHalvo".into());
    let label = arg(args, "--label").unwrap_or_else(|| "state".into());
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let run: Option<String> = hkcu.open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run").ok().and_then(|k| k.get_value(&name).ok());
    let ap = startup_approved(&name);
    let approved_values = hkcu
        .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run")
        .map(|k| k.enum_values().count())
        .unwrap_or(0);
    let hklm_ap = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run")
        .ok()
        .and_then(|k| k.get_raw_value(&name).ok())
        .is_some();
    println!(
        "S5 startup_state label={label} run_value_present={} approved_present={} approved_len={} approved_first_byte={} approved_tail_nonzero={} approved_values_in_key={approved_values} hklm_approved_present={hklm_ap}",
        run.is_some(),
        ap.is_some(),
        ap.as_ref().map(|b| b.len()).unwrap_or(0),
        ap.as_ref().and_then(|b| b.first().copied()).map(|b| format!("0x{b:02x}")).unwrap_or("-".into()),
        ap.as_ref().map(|b| b.iter().skip(4).any(|&x| x != 0)).unwrap_or(false),
    );
}

pub fn runvalue_set(args: &[String]) {
    use winreg::enums::*;
    use winreg::RegKey;
    let name = arg(args, "--name").unwrap_or_else(|| "R1SpikeHalvo".into());
    let value = arg(args, "--value").unwrap_or_default();
    let (k, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run").unwrap();
    let del = flag(args, "--delete");
    if del {
        println!("S5 runvalue_set delete_ok={}", k.delete_value(&name).is_ok());
        if let Ok(ap) = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run", KEY_SET_VALUE) {
            println!("S5 runvalue_set approved_delete_ok={}", ap.delete_value(&name).is_ok());
        }
    } else {
        println!("S5 runvalue_set write_ok={}", k.set_value(&name, &value).is_ok());
    }
}

fn pipe_name(dir: &str, which: &str) -> String {
    std::fs::read_to_string(format!("{dir}\\pipe-{which}.txt")).unwrap_or_default().trim().to_string()
}

/// A pipe server as C15.2 has it: first instance, remote clients rejected, at most 5 instances,
/// the DACL `D:P(A;;GA;;;<SID>)`; it drops a client whose token user is not its own.
pub fn pipe_server(args: &[String]) {
    let dir = arg(args, "--dir").unwrap();
    let which = arg(args, "--which").unwrap_or_else(|| "a".into());
    let clients: usize = arg(args, "--clients").and_then(|s| s.parse().ok()).unwrap_or(3);
    let secs: u64 = arg(args, "--secs").and_then(|s| s.parse().ok()).unwrap_or(60);
    let sddl_kind = arg(args, "--sddl").unwrap_or_else(|| "self".into());
    let tok = self_token().unwrap();
    let sid = token_user_sid_string(tok.0).unwrap_or_default();
    let me = token_user_sid(tok.0).unwrap_or_default();
    let name = arg(args, "--name").unwrap_or_else(|| format!("\\\\.\\pipe\\r1spike-{sid}-{which}"));
    let sddl = if sddl_kind == "everyone" { "D:P(A;;GA;;;WD)".to_string() } else { format!("D:P(A;;GA;;;{sid})") };
    let mut psd = PSECURITY_DESCRIPTOR::default();
    let ok_sd = unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(PCWSTR(wide(&sddl).as_ptr()), SDDL_REVISION_1, &mut psd, None).is_ok() };
    let sa = SECURITY_ATTRIBUTES { nLength: size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: psd.0, bInheritHandle: FALSE };
    let wname = wide(&name);
    let mk = |first: bool| unsafe {
        let mut fl = PIPE_ACCESS_DUPLEX;
        if first {
            fl |= FILE_FLAG_FIRST_PIPE_INSTANCE;
        }
        CreateNamedPipeW(PCWSTR(wname.as_ptr()), fl, PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS, 5, 4096, 4096, 0, Some(&sa))
    };
    let h = mk(true);
    let created = !h.is_invalid();
    let err = if created { 0 } else { unsafe { GetLastError().0 } };
    println!("S5 pipe_server which={which} sddl={sddl_kind} sd_ok={ok_sd} first_instance_created={created} create_error={err}");
    std::fs::write(format!("{dir}\\pipe-{which}.txt"), &name).ok();
    std::fs::write(format!("{dir}\\pipe-{which}.ready"), if created { "1" } else { "0" }).ok();
    if !created {
        return;
    }
    let t = Instant::now();
    let mut cur = h;
    let mut served = 0;
    while served < clients && t.elapsed() < Duration::from_secs(secs) {
        let c = unsafe { ConnectNamedPipe(cur, None) };
        let connected = c.is_ok() || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
        if !connected {
            std::thread::sleep(Duration::from_millis(50));
            continue;
        }
        let mut cpid = 0u32;
        unsafe {
            let _ = GetNamedPipeClientProcessId(cur, &mut cpid);
        }
        let imp = unsafe { ImpersonateNamedPipeClient(cur).is_ok() };
        let mut same = None;
        let mut level = -1i32;
        if imp {
            let mut tt = HANDLE::default();
            if unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, true, &mut tt).is_ok() } {
                let tt = OwnedHandle(tt);
                same = token_user_sid(tt.0).map(|s| s == me);
                let mut lv = SECURITY_IMPERSONATION_LEVEL::default();
                let mut n = 0u32;
                unsafe {
                    let _ = GetTokenInformation(tt.0, TokenImpersonationLevel, Some(&mut lv as *mut _ as *mut c_void), size_of::<SECURITY_IMPERSONATION_LEVEL>() as u32, &mut n);
                }
                level = lv.0;
            }
            unsafe {
                let _ = RevertToSelf();
            }
        }
        let mut buf = [0u8; 64];
        let mut n = 0u32;
        let _ = unsafe { ReadFile(cur, Some(&mut buf), Some(&mut n), None) };
        let answered = if same == Some(true) {
            let mut w = 0u32;
            unsafe { WriteFile(cur, Some(b"ok\n"), Some(&mut w), None).is_ok() }
        } else {
            false
        };
        println!("S5 pipe_server_client which={which} n={served} client_pid_known={} impersonated={imp} impersonation_level={level} same_user={same:?} answered={answered}", cpid != 0);
        unsafe {
            let _ = FlushFileBuffers(cur);
            let _ = DisconnectNamedPipe(cur);
        }
        served += 1;
        let _ = cur;
    }
    unsafe {
        let _ = CloseHandle(cur);
    }
    cur = HANDLE::default();
    let _ = cur;
}

pub fn pipe_client(args: &[String]) {
    let dir = arg(args, "--dir").unwrap();
    let which = arg(args, "--which").unwrap_or_else(|| "a".into());
    let label = arg(args, "--label").unwrap_or_else(|| "client".into());
    let name = pipe_name(&dir, &which);
    let wname = wide(&name);
    let r = unsafe {
        CreateFileW(PCWSTR(wname.as_ptr()), (FILE_GENERIC_READ | FILE_GENERIC_WRITE).0, FILE_SHARE_NONE, None, OPEN_EXISTING, FILE_FLAGS_AND_ATTRIBUTES(SECURITY_SQOS_PRESENT.0 | SECURITY_IDENTIFICATION.0), None)
    };
    match r {
        Ok(h) => {
            let h = OwnedHandle(h);
            let mut spid = 0u32;
            unsafe {
                let _ = GetNamedPipeServerProcessId(h.0, &mut spid);
            }
            let me = self_token().and_then(|t| token_user_sid(t.0));
            let server_same = token_of(spid).and_then(|t| token_user_sid(t.0)).map(|s| Some(s) == me);
            let mut w = 0u32;
            let sent = unsafe { WriteFile(h.0, Some(b"status\n"), Some(&mut w), None).is_ok() };
            let mut buf = [0u8; 16];
            let mut n = 0u32;
            let got = unsafe { ReadFile(h.0, Some(&mut buf), Some(&mut n), None).is_ok() } && n > 0;
            println!("S5 pipe_client label={label} which={which} opened=true error=0 server_pid_known={} server_user_is_self={server_same:?} sent={sent} got_reply={got}", spid != 0);
        }
        Err(e) => {
            println!("S5 pipe_client label={label} which={which} opened=false error={}", win32_code(&e));
        }
    }
}

pub fn session_end_settings() {
    use winreg::enums::*;
    use winreg::RegKey;
    let d = RegKey::predef(HKEY_CURRENT_USER).open_subkey("Control Panel\\Desktop").ok();
    let get = |k: &Option<RegKey>, n: &str| -> String {
        k.as_ref().and_then(|k| k.get_value::<String, _>(n).ok()).unwrap_or_else(|| "absent".into())
    };
    let c = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey("SYSTEM\\CurrentControlSet\\Control").ok();
    println!(
        "S5 session_end_settings hkcu_WaitToKillAppTimeout={} hkcu_HungAppTimeout={} hkcu_AutoEndTasks={} hklm_WaitToKillServiceTimeout={}",
        get(&d, "WaitToKillAppTimeout"),
        get(&d, "HungAppTimeout"),
        get(&d, "AutoEndTasks"),
        get(&c, "WaitToKillServiceTimeout")
    );
}
