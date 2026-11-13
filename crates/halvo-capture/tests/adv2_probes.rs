#![cfg(target_os = "windows")]
#![deny(clippy::undocumented_unsafe_blocks)]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use halvo_capture::event::{CaptureStreamEvent, Modality};
use halvo_capture::secure_input::windows::PasswordGate;
use halvo_capture::sink::{channel, SinkHandle, SinkWriter};
use halvo_capture::source::{CaptureSource, SourceContext};
use halvo_capture::windows::input::RawInputSource;

use windows::core::{BOOL, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, SetFocus, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_A, VK_MENU,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, DispatchMessageW, EnumWindows, GetForegroundWindow,
    GetWindowTextW, PeekMessageW, SetForegroundWindow, ShowWindow, TranslateMessage, MSG,
    PM_REMOVE, SW_SHOWNOACTIVATE, WINDOW_EX_STYLE, WS_CHILD, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};

fn say(test: &str, message: impl AsRef<str>) {
    println!("ADV2 {test} {}", message.as_ref());
}

fn with_deadline<F>(test: &'static str, limit: Duration, body: F)
where
    F: FnOnce() + Send + 'static,
{
    let (done, finished) = mpsc::channel();
    let worker = std::thread::Builder::new()
        .name(format!("adv2-{test}"))
        .spawn(move || {
            body();
            let _ = done.send(());
        })
        .expect("spawn the probe body");
    match finished.recv_timeout(limit) {
        Ok(()) => worker.join().expect("probe body finished"),
        Err(RecvTimeoutError::Timeout) => {
            say(
                test,
                format!("result=TIMEOUT limit_secs={}", limit.as_secs()),
            );
            panic!("{test} did not finish within {limit:?}");
        }
        Err(RecvTimeoutError::Disconnected) => {
            if let Err(payload) = worker.join() {
                std::panic::resume_unwind(payload);
            }
        }
    }
}

fn foreground_window() -> HWND {
    // SAFETY: GetForegroundWindow takes no argument and returns a handle value.
    unsafe { GetForegroundWindow() }
}

fn set_foreground(hwnd: HWND) {
    // SAFETY: SetForegroundWindow takes a window handle by value; a stale
    // handle makes the call fail and changes nothing.
    unsafe {
        let _ = SetForegroundWindow(hwnd);
    }
}

fn key_input(key: VIRTUAL_KEY, released: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: 0,
                dwFlags: if released {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send(inputs: &[INPUT]) -> u32 {
    // SAFETY: `inputs` is a live slice of INPUT values and the size argument
    // is the size of one INPUT, as SendInput requires.
    unsafe { SendInput(inputs, core::mem::size_of::<INPUT>() as i32) }
}

fn letters(n: u32, gap: Duration) -> u32 {
    let mut accepted = 0;
    for _ in 0..n {
        accepted += send(&[key_input(VK_A, false)]);
        std::thread::sleep(Duration::from_millis(20));
        accepted += send(&[key_input(VK_A, true)]);
        std::thread::sleep(gap);
    }
    accepted
}

struct Events(mpsc::Receiver<CaptureStreamEvent>);

impl Events {
    fn drain_for(&self, window: Duration) -> Vec<CaptureStreamEvent> {
        let deadline = Instant::now() + window;
        let mut events = Vec::new();
        loop {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            match self.0.recv_timeout(deadline - now) {
                Ok(event) => events.push(event),
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        events
    }
}

fn forward(writer: SinkWriter) -> Events {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("adv2-sink-forward".to_string())
        .spawn(move || {
            while let Some(event) = writer.recv() {
                if tx.send(event).is_err() {
                    break;
                }
            }
        })
        .expect("spawn the sink forwarder");
    Events(rx)
}

fn capture(capacity: usize) -> (SinkHandle, Events) {
    let (sink, writer) = channel(capacity);
    (sink, forward(writer))
}

fn keystrokes(events: &[CaptureStreamEvent]) -> usize {
    events
        .iter()
        .filter(|e| e.modality == Modality::Keystroke)
        .count()
}

fn scratch_dir() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("adv2-")
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .expect("temp dir under CARGO_TARGET_TMPDIR")
}

fn windows_titled(prefix: &str) -> Option<(HWND, String)> {
    unsafe extern "system" fn each(hwnd: HWND, found: LPARAM) -> BOOL {
        let mut buf = [0u16; 512];
        // SAFETY: `buf` is a live buffer whose length the call takes, and
        // `found` is the address of the vector that the caller holds across
        // the enumeration on this thread, made from an exclusive borrow.
        unsafe {
            let len = GetWindowTextW(hwnd, &mut buf);
            let title = String::from_utf16_lossy(&buf[..usize::try_from(len).unwrap_or(0)]);
            (*(found.0 as *mut Vec<(HWND, String)>)).push((hwnd, title));
        }
        BOOL::from(true)
    }
    let mut all: Vec<(HWND, String)> = Vec::new();
    // SAFETY: the callback has the WNDENUMPROC signature and writes `all`
    // through a pointer made from an exclusive borrow, only while this
    // enumeration runs on this thread; nothing else touches `all` meanwhile.
    unsafe {
        let _ = EnumWindows(
            Some(each),
            LPARAM(&mut all as *mut Vec<(HWND, String)> as isize),
        );
    }
    all.into_iter().find(|(_, title)| title.starts_with(prefix))
}

fn title_of(hwnd: HWND) -> String {
    let mut buf = [0u16; 512];
    // SAFETY: `buf` is a live buffer whose length the call takes.
    let len = unsafe { GetWindowTextW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..usize::try_from(len).unwrap_or(0)])
}

fn number_after(title: &str, key: &str) -> i64 {
    title
        .find(key)
        .and_then(|at| title[at + key.len()..].split(' ').next())
        .and_then(|v| v.parse().ok())
        .unwrap_or(-1)
}

const PAGE: &str = r#"<!doctype html>
<html><head><meta charset="utf-8"><title>adv2-fixture</title></head>
<body>
<input id="t" type="text">
<input id="p" type="password">
<script>
const q = new URLSearchParams(location.search);
const phase = q.get('phase') || 'N';
const run = q.get('run') || '0';
let asText = 0, asPassword = 0, pk = 0;
const t = document.getElementById('t');
const p = document.getElementById('p');
function show() {
  document.title = 'adv2-fixture ' + phase + run + ' t_text=' + asText + ' t_password=' + asPassword +
    ' p=' + pk + ' t_type_password=' + (t.type === 'password' ? 1 : 0) + ' end';
}
t.addEventListener('keydown', (e) => {
  if (e.repeat) return;
  if (t.type === 'password') asPassword++; else asText++;
  show();
});
p.addEventListener('keydown', (e) => { if (e.repeat) return; pk++; show(); });
if (phase === 'P') p.focus(); else t.focus();
if (phase === 'S') setTimeout(() => { t.type = 'password'; show(); }, 600);
if (phase === 'K') t.addEventListener('keyup', () => { t.type = 'password'; show(); }, { once: true });
show();
</script>
</body></html>
"#;

struct Browser {
    exe: PathBuf,
    chromium: bool,
    profile: tempfile::TempDir,
    page: String,
    runs: u32,
}

impl Browser {
    fn new(exe: PathBuf, chromium: bool) -> Option<Browser> {
        if !exe.is_file() {
            return None;
        }
        let profile = scratch_dir();
        if !chromium {
            let prefs = "user_pref(\"browser.shell.checkDefaultBrowser\", false);\n\
                         user_pref(\"browser.aboutwelcome.enabled\", false);\n\
                         user_pref(\"datareporting.policy.dataSubmissionEnabled\", false);\n\
                         user_pref(\"browser.startup.homepage_override.mstone\", \"ignore\");\n\
                         user_pref(\"startup.homepage_welcome_url\", \"\");\n\
                         user_pref(\"trailhead.firstrun.didSeeAboutWelcome\", true);\n";
            std::fs::write(profile.path().join("user.js"), prefs).ok()?;
        }
        let page_path = profile.path().join("adv2.html");
        std::fs::write(&page_path, PAGE).ok()?;
        let page = format!("file:///{}", page_path.to_string_lossy().replace('\\', "/"));
        Some(Browser {
            exe,
            chromium,
            profile,
            page,
            runs: 0,
        })
    }

    fn launch(&mut self, phase: char) -> String {
        self.runs += 1;
        let url = format!("{}?phase={phase}&run={}", self.page, self.runs);
        let profile = self.profile.path().to_string_lossy().into_owned();
        let args: Vec<String> = if self.chromium {
            vec![
                format!("--user-data-dir={profile}"),
                "--no-first-run".to_string(),
                "--no-default-browser-check".to_string(),
                "--disable-features=Translate".to_string(),
                "--new-window".to_string(),
                url,
            ]
        } else {
            vec![
                "-profile".to_string(),
                profile,
                "-new-window".to_string(),
                url,
            ]
        };
        let _ = Command::new(&self.exe)
            .args(&args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        format!("adv2-fixture {phase}{} ", self.runs)
    }

    fn front(prefix: &str) -> Option<HWND> {
        let began = Instant::now();
        while began.elapsed() < Duration::from_secs(40) {
            if let Some((hwnd, _)) = windows_titled(prefix) {
                for _ in 0..10 {
                    if foreground_window() == hwnd {
                        break;
                    }
                    send(&[key_input(VK_MENU, false)]);
                    set_foreground(hwnd);
                    send(&[key_input(VK_MENU, true)]);
                    std::thread::sleep(Duration::from_millis(200));
                }
                return Some(hwnd);
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        None
    }

    fn kill(&self) {
        let image = self
            .exe
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let _ = Command::new("taskkill.exe")
            .args(["/IM", &image, "/F", "/T"])
            .output();
        std::thread::sleep(Duration::from_millis(1500));
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        self.kill();
    }
}

struct Round {
    window_found: bool,
    foreground_at_send: bool,
    foreground_after: bool,
    accepted: u32,
    events: usize,
    t_text: i64,
    t_password: i64,
    p: i64,
    gate: String,
}

impl Round {
    fn line(&self) -> String {
        format!(
            "window_found={} foreground_at_send={} foreground_after={} sendinput_accepted={} \
             keystroke_events={} page_t_text={} page_t_password={} page_p={} gate_after={}",
            self.window_found,
            self.foreground_at_send,
            self.foreground_after,
            self.accepted,
            self.events,
            self.t_text,
            self.t_password,
            self.p,
            self.gate,
        )
    }
}

fn type_and_count(
    hwnd: Option<HWND>,
    keyboard: &RawInputSource,
    events: &Events,
    count: u32,
    gap: Duration,
) -> Round {
    let _ = events.drain_for(Duration::from_millis(300));
    let foreground_at_send = hwnd.is_some_and(|h| foreground_window() == h);
    let accepted = letters(count, gap);
    let seen = events.drain_for(Duration::from_millis(1500));
    let foreground_after = hwnd.is_some_and(|h| foreground_window() == h);
    let title = hwnd.map(title_of).unwrap_or_default();
    Round {
        window_found: hwnd.is_some(),
        foreground_at_send,
        foreground_after,
        accepted,
        events: keystrokes(&seen),
        t_text: number_after(&title, " t_text="),
        t_password: number_after(&title, " t_password="),
        p: number_after(&title, " p="),
        gate: format!("{:?}", keyboard.field_state()),
    }
}

fn keyboard_started() -> (RawInputSource, Events, bool) {
    let (sink, events) = capture(8192);
    let mut keyboard = RawInputSource::keyboard(Arc::new(Mutex::new(None)), PasswordGate::new());
    let started = keyboard.begin(SourceContext::new(sink)).is_ok();
    (keyboard, events, started)
}

fn chromium_exe(name: &str) -> Option<PathBuf> {
    let paths: &[&str] = match name {
        "chrome" => &[
            r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
        ],
        _ => &[
            r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
            r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
        ],
    };
    paths.iter().map(PathBuf::from).find(|p| p.is_file())
}

fn type_swap(test: &'static str, name: &'static str) {
    with_deadline(test, Duration::from_secs(240), move || {
        let exe = chromium_exe(name);
        say(test, format!("browser={name} installed={}", exe.is_some()));
        let Some(exe) = exe else {
            panic!("INCONCLUSIVE: {name} is not installed");
        };
        let mut browser = Browser::new(exe, true).expect("profile");
        let (mut keyboard, events, started) = keyboard_started();
        say(test, format!("keyboard_start_ok={started}"));
        let mut results = Vec::new();
        for (phase, wait) in [('W', 0u64), ('N', 0), ('P', 0), ('S', 900), ('K', 0)] {
            let prefix = browser.launch(phase);
            let hwnd = Browser::front(&prefix);
            std::thread::sleep(Duration::from_millis(1200 + wait));
            if phase == 'W' {
                continue;
            }
            let round = type_and_count(hwnd, &keyboard, &events, 5, Duration::from_millis(150));
            say(
                test,
                format!("browser={name} phase={phase} {}", round.line()),
            );
            results.push((phase, round));
        }
        let stopped = keyboard.shutdown().is_ok();
        say(test, format!("keyboard_stop_ok={stopped}"));
        let get = |p: char| results.iter().find(|(q, _)| *q == p).map(|(_, r)| r);
        let (n, pw, s, k) = (get('N'), get('P'), get('S'), get('K'));
        say(
            test,
            format!(
                "SUMMARY browser={name} control_text_events={} control_password_events={} \
                 swap_by_timer_keys_into_password_type={} swap_by_timer_events={} \
                 swap_on_first_key_keys_into_password_type={} swap_on_first_key_events={}",
                n.map_or(-1, |r| r.events as i64),
                pw.map_or(-1, |r| r.events as i64),
                s.map_or(-1, |r| r.t_password),
                s.map_or(-1, |r| r.events as i64),
                k.map_or(-1, |r| r.t_password),
                k.map_or(-1, |r| r.events as i64),
            ),
        );
    });
}

#[test]
#[ignore = "adv2 probe: needs an interactive Windows desktop and Chrome"]
fn adv2_type_swap_chrome() {
    type_swap("swap-chrome", "chrome");
}

#[test]
#[ignore = "adv2 probe: needs an interactive Windows desktop and Edge"]
fn adv2_type_swap_edge() {
    type_swap("swap-edge", "edge");
}

fn copy_tree(from: &Path, to: &Path, rename_from: &str, rename_to: &str) -> std::io::Result<u64> {
    std::fs::create_dir_all(to)?;
    let mut files = 0;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();
        let kind = entry.file_type()?;
        let target_name = if name.to_string_lossy().eq_ignore_ascii_case(rename_from) {
            std::ffi::OsString::from(rename_to)
        } else {
            name
        };
        let target = to.join(target_name);
        if kind.is_dir() {
            files += copy_tree(&entry.path(), &target, rename_from, rename_to)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
            files += 1;
        }
    }
    Ok(files)
}

const GECKO_COPY_NAME: &str = "geckoprobe.exe";

fn gecko_rounds(test: &str, name: &'static str, exe: PathBuf) -> Vec<(char, Round)> {
    let mut out = Vec::new();
    for (variant, attach_first) in [('a', true), ('a', true), ('b', false), ('b', false)] {
        let mut browser = Browser::new(exe.clone(), false).expect("profile");
        let (round, start_ok) = if attach_first {
            let (mut keyboard, events, started) = keyboard_started();
            let prefix = browser.launch('P');
            let hwnd = Browser::front(&prefix);
            std::thread::sleep(Duration::from_millis(1500));
            let round = type_and_count(hwnd, &keyboard, &events, 5, Duration::from_millis(150));
            let _ = keyboard.shutdown();
            (round, started)
        } else {
            let prefix = browser.launch('P');
            let hwnd = Browser::front(&prefix);
            std::thread::sleep(Duration::from_millis(1500));
            let (mut keyboard, events, started) = keyboard_started();
            std::thread::sleep(Duration::from_millis(600));
            let round = type_and_count(hwnd, &keyboard, &events, 5, Duration::from_millis(150));
            let _ = keyboard.shutdown();
            (round, started)
        };
        say(
            test,
            format!(
                "browser={name} variant={variant} attach_before_launch={attach_first} keyboard_start_ok={start_ok} {}",
                round.line()
            ),
        );
        out.push((variant, round));
        drop(browser);
    }
    out
}

#[test]
#[ignore = "adv2 probe: needs an interactive Windows desktop and Firefox"]
fn adv2_gecko_under_another_file_name() {
    with_deadline("gecko-name", Duration::from_secs(420), || {
        let test = "gecko-name";
        let install = [
            r"C:\Program Files\Mozilla Firefox",
            r"C:\Program Files (x86)\Mozilla Firefox",
        ]
        .iter()
        .map(PathBuf::from)
        .find(|dir| dir.join("firefox.exe").is_file());
        say(test, format!("firefox_installed={}", install.is_some()));
        let Some(install) = install else {
            panic!("INCONCLUSIVE: Firefox is not installed");
        };
        let copy_root = scratch_dir();
        let copy = copy_root.path().join("gecko");
        let copied = copy_tree(&install, &copy, "firefox.exe", GECKO_COPY_NAME);
        let renamed_exe = copy.join(GECKO_COPY_NAME);
        say(
            test,
            format!(
                "copied_files={} renamed_exe_present={} firefox_exe_in_copy={}",
                copied.as_ref().map_or(-1, |n| *n as i64),
                renamed_exe.is_file(),
                copy.join("firefox.exe").is_file()
            ),
        );
        let renamed = gecko_rounds(test, "gecko-renamed", renamed_exe);
        let real = gecko_rounds(test, "firefox", install.join("firefox.exe"));
        let sum = |rounds: &[(char, Round)], v: char, f: &dyn Fn(&Round) -> i64| -> i64 {
            rounds
                .iter()
                .filter(|(x, _)| *x == v)
                .map(|(_, r)| f(r))
                .sum()
        };
        for (label, rounds) in [("gecko-renamed", &renamed), ("firefox", &real)] {
            for v in ['a', 'b'] {
                say(
                    test,
                    format!(
                        "SUMMARY browser={label} variant={v} rounds=2 keys_into_password_field={} keystroke_events={} foreground_at_send_rounds={}",
                        sum(rounds, v, &|r| r.p.max(0)),
                        sum(rounds, v, &|r| r.events as i64),
                        sum(rounds, v, &|r| i64::from(r.foreground_at_send)),
                    ),
                );
            }
        }
    });
}

fn hopper(stop: Arc<AtomicBool>, hops: Arc<AtomicU32>, ready: mpsc::Sender<bool>) {
    let class = windows::core::w!("STATIC");
    let edit = windows::core::w!("EDIT");
    // SAFETY: the class names are static wide strings of system classes, the
    // parent of each child is the live window made just before, and no
    // pointer argument outlives the call.
    let made = unsafe {
        let parent = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            windows::core::w!("adv2-hopper"),
            WS_OVERLAPPEDWINDOW,
            0,
            0,
            300,
            160,
            None,
            None,
            None,
            None,
        );
        parent.and_then(|parent| {
            let a = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                edit,
                PCWSTR::null(),
                WS_CHILD | WS_VISIBLE,
                10,
                10,
                200,
                24,
                Some(parent),
                None,
                None,
                None,
            )?;
            let b = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                edit,
                PCWSTR::null(),
                WS_CHILD | WS_VISIBLE,
                10,
                50,
                200,
                24,
                Some(parent),
                None,
                None,
                None,
            )?;
            let _ = ShowWindow(parent, SW_SHOWNOACTIVATE);
            Ok((parent, a, b))
        })
    };
    let _ = ready.send(made.is_ok());
    let Ok((parent, a, b)) = made else {
        return;
    };
    let mut msg = MSG::default();
    let mut i = 0u32;
    while !stop.load(Ordering::Relaxed) {
        let target = if i.is_multiple_of(2) { a } else { b };
        // SAFETY: `target` is a live child window of this thread.
        if unsafe { SetFocus(Some(target)) }.is_ok() {
            hops.fetch_add(1, Ordering::Relaxed);
        }
        i += 1;
        let until = Instant::now() + Duration::from_millis(100);
        while Instant::now() < until {
            // SAFETY: `msg` is a live MSG that PeekMessageW fills, and the
            // same value goes to TranslateMessage and DispatchMessageW.
            unsafe {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    // SAFETY: `parent` is the live window that this thread made.
    unsafe {
        let _ = DestroyWindow(parent);
    }
}

#[test]
#[ignore = "adv2 probe: needs an interactive Windows desktop and Edge"]
fn adv2_focus_moves_in_a_background_window() {
    with_deadline("background-focus", Duration::from_secs(240), || {
        let test = "background-focus";
        let exe = chromium_exe("edge");
        say(test, format!("browser=edge installed={}", exe.is_some()));
        let Some(exe) = exe else {
            panic!("INCONCLUSIVE: Edge is not installed");
        };
        let mut browser = Browser::new(exe, true).expect("profile");
        let (mut keyboard, events, started) = keyboard_started();
        say(test, format!("keyboard_start_ok={started}"));
        let prefix = browser.launch('W');
        let _ = Browser::front(&prefix);
        std::thread::sleep(Duration::from_millis(1200));
        for with_hopper in [false, true] {
            let prefix = browser.launch('P');
            let hwnd = Browser::front(&prefix);
            std::thread::sleep(Duration::from_millis(1200));
            let stop = Arc::new(AtomicBool::new(false));
            let hops = Arc::new(AtomicU32::new(0));
            let mut thread = None;
            let mut hopper_ok = false;
            if with_hopper {
                let (tx, rx) = mpsc::channel();
                let (s, h) = (Arc::clone(&stop), Arc::clone(&hops));
                thread = Some(std::thread::spawn(move || hopper(s, h, tx)));
                hopper_ok = rx.recv_timeout(Duration::from_secs(5)).unwrap_or(false);
                std::thread::sleep(Duration::from_millis(800));
            }
            let round = type_and_count(hwnd, &keyboard, &events, 10, Duration::from_millis(250));
            stop.store(true, Ordering::Relaxed);
            if let Some(thread) = thread {
                let _ = thread.join();
            }
            say(
                test,
                format!(
                    "SUMMARY browser=edge background_hopper={with_hopper} hopper_window_made={hopper_ok} \
                     background_focus_moves={} {}",
                    hops.load(Ordering::Relaxed),
                    round.line()
                ),
            );
        }
        let stopped = keyboard.shutdown().is_ok();
        say(test, format!("keyboard_stop_ok={stopped}"));
    });
}
