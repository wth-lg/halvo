use super::*;
use halvo_capture::secure_input::windows::FieldState;

mod evidence;
use halvo_capture::source::Started;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSE_EVENT_FLAGS, VK_A,
    VK_F22, VK_F24, VK_TAB,
};

const PHASE_PAGE: &str = r#"<!doctype html>
<html><head><meta charset="utf-8"><title>halvo-phase</title>
<style>body{margin:0} input,button{display:block;width:320px;height:40px;margin:24px;font-size:20px}</style>
</head>
<body>
<input id="plain" type="text">
<input id="password" type="password">
<button id="show">show</button>
<script>
const q = new URLSearchParams(location.search);
const phase = q.get('phase') || 'none';
const hold = Number(q.get('hold') || '0');
const run = q.get('run') || '0';
const plain = document.getElementById('plain');
const password = document.getElementById('password');
const button = document.getElementById('show');
const c = { text: 0, password: 0, tp: 0, tt: 0, downs: 0, round: 0, seq: '', lt: [], base: Date.now(), ev: -1, mv: -1, kd: -1 };
let armed = false;
let timers = [];
const busy = (ms) => { const end = performance.now() + ms; while (performance.now() < end) {} };
const where = () => document.activeElement ? (document.activeElement.id || 'body') : 'none';
function show() {
  const r = password.getBoundingClientRect();
  const t = plain.getBoundingClientRect();
  const top = window.screenY + (window.outerHeight - window.innerHeight);
  const left = window.screenX + (window.outerWidth - window.innerWidth) / 2;
  document.title = 'halvo-phase ' + phase + run + ' text=' + c.text + ' password=' + c.password +
    ' tp=' + c.tp + ' tt=' + c.tt + ' downs=' + c.downs + ' round=' + c.round +
    ' focus=' + where() + ' ptype=' + plain.type +
    ' px=' + Math.round(left + r.left + r.width / 2) + ' py=' + Math.round(top + r.top + r.height / 2) +
    ' tx=' + Math.round(left + t.left + t.width / 2) + ' ty=' + Math.round(top + t.top + t.height / 2) +
    ' seq=' + (c.seq || '-') + ' lt=' + (c.lt.length ? c.lt.join(',') : '-') + ' ev=' + c.ev +
    ' mv=' + c.mv + ' kd=' + c.kd + ' end';
}
const later = (ms, f) => timers.push(setTimeout(() => { f(); show(); }, ms));
const freed = () => { c.ev = Date.now() - c.base; };
const swap = () => { busy(hold); plain.type = 'password'; busy(hold); freed(); };
for (const el of [plain, password]) {
  el.addEventListener('input', () => {
    if (el.type === 'password') c.password++; else c.text++;
    c.seq += el.type === 'password' ? 'p' : 't';
    c.lt.push(Date.now() - c.base);
    show();
  });
  el.addEventListener('keydown', (e) => {
    if (e.key === 'Tab') { if (el.type === 'password') c.tp++; else c.tt++; show(); }
  });
}
plain.addEventListener('keydown', (e) => {
  if (e.key === 'Tab' && phase === 'tab') busy(hold);
  if (e.key !== 'F24' && armed && (phase === 'swap-keydown' || phase === 'burst')) { armed = false; swap(); }
});
plain.addEventListener('keyup', (e) => {
  if (e.key !== 'F24' && armed && phase === 'swap-keyup') { armed = false; swap(); }
});
plain.addEventListener('focus', () => {
  if (armed && phase === 'swap-focus') { armed = false; swap(); }
});
password.addEventListener('mousedown', () => { c.downs++; show(); if (phase === 'click') busy(hold); });
function reset() {
  for (const t of timers) clearTimeout(t);
  timers = [];
  plain.type = 'text'; password.type = 'password'; plain.value = ''; password.value = '';
  password.tabIndex = phase === 'tab-after' ? -1 : 0;
  c.text = 0; c.password = 0; c.tp = 0; c.tt = 0; c.downs = 0; c.round++;
  c.seq = ''; c.lt = []; c.base = Date.now(); c.ev = -1; c.mv = -1; c.kd = -1;
  // The burst's swap is armed 1 s after the reset, so that the control's
  // letters before it go into the text input (W3-10 T5).
  armed = phase !== 'burst';
  if (phase === 'burst') later(1000, () => { armed = true; });
  if (phase === 'cold' || phase === 'show-hide' || phase === 'show-control') password.focus();
  else if (phase === 'swap-focus') { button.focus(); later(300, () => plain.focus()); }
  // The burst's page moves its own focus there and back, so that the gate's
  // newest mark is a focus event of this page and not a stale event of the
  // window that lost the front (W3-10 T5; run w39-2).
  else if (phase === 'after-start') {
    later(2500, () => {
      button.focus();
      later(150, () => { plain.focus(); c.mv = Date.now() - c.base; });
    });
  }
  else if (phase === 'burst' || phase === 'control') {
    button.focus();
    later(150, () => { plain.focus(); c.mv = Date.now() - c.base; });
  }
  else plain.focus();
  if (phase === 'swap-timer' || phase === 'tab-after') later(400, () => { armed = false; swap(); });
  if (phase === 'show-hide') { later(300, () => { password.type = 'text'; }); later(1300, () => { password.type = 'password'; }); }
  if (phase === 'show-control') {
    later(300, () => { button.focus(); password.type = 'text'; });
    later(600, () => password.focus());
    later(1300, () => { password.type = 'password'; });
  }
  if (phase === 'self') later(600, () => { busy(hold); password.focus(); freed(); });
  show();
}
document.addEventListener('focusin', () => show());
document.addEventListener('keydown', (e) => {
  if (e.key !== 'F24' && e.key !== 'F22' && c.kd < 0) { c.kd = Date.now() - c.base; show(); }
  if (e.key === 'F24') { e.preventDefault(); reset(); }
  if (e.key === 'F22') { e.preventDefault(); password.focus(); show(); }
}, true);
reset();
</script>
</body></html>
"#;

#[derive(Debug, Default, Clone)]
struct PageRead {
    text: i64,
    password: i64,
    tabs_password: i64,
    tabs_text: i64,
    downs: i64,
    focus: String,
    plain_type: String,
    px: i64,
    py: i64,
    tx: i64,
    ty: i64,
    landings: String,
    landed_at: Vec<i64>,
    freed_at: i64,
    moved_at: i64,
    first_key_at: i64,
}

fn page_read(hwnd: HWND) -> Option<PageRead> {
    let title = title_of(hwnd);
    let field = |key: &str| -> Option<String> {
        let at = title.find(&format!(" {key}="))? + key.len() + 2;
        Some(title[at..].split(' ').next()?.to_string())
    };
    let number = |key: &str| field(key).and_then(|v| v.parse::<i64>().ok());
    Some(PageRead {
        text: number("text")?,
        password: number("password")?,
        tabs_password: number("tp")?,
        tabs_text: number("tt")?,
        downs: number("downs")?,
        focus: field("focus")?,
        plain_type: field("ptype")?,
        px: number("px")?,
        py: number("py")?,
        tx: number("tx")?,
        ty: number("ty")?,
        landings: field("seq").map(|seq| if seq == "-" { String::new() } else { seq })?,
        landed_at: field("lt").map(|list| {
            list.split(',')
                .filter_map(|at| at.parse::<i64>().ok())
                .collect()
        })?,
        freed_at: number("ev")?,
        moved_at: number("mv")?,
        first_key_at: number("kd")?,
    })
}

#[derive(Debug, Default, Clone)]
struct Decisions {
    letters: usize,
    letters_decided: usize,
    tabs: usize,
    other: usize,
    first_tab_rule: Option<seam::Rule>,
    first_letter_rule: Option<seam::Rule>,
    letters_in_order: Vec<bool>,
}

fn decisions() -> Decisions {
    decisions_of(&seam::take())
}

fn decisions_of(keys: &[seam::Decided]) -> Decisions {
    let mut d = Decisions::default();
    for key in keys.iter().copied() {
        let recorded = key.verdict == seam::Verdict::Recorded;
        match key.class {
            seam::KeyClass::Printable => {
                d.letters_decided += 1;
                if d.first_letter_rule.is_none() {
                    d.first_letter_rule = key.rule;
                }
                d.letters += usize::from(recorded);
                d.letters_in_order.push(recorded);
            }
            seam::KeyClass::Tab => {
                if d.first_tab_rule.is_none() {
                    d.first_tab_rule = key.rule;
                }
                d.tabs += usize::from(recorded);
            }
            _ => d.other += usize::from(recorded),
        }
    }
    d
}

fn mouse_input(dx: i32, dy: i32, flags: MOUSE_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn click_at(x: i64, y: i64) {
    let (cx, cy) = screen_size();
    let to = |v: i64, span: i32| (v * 65_535 / i64::from(span - 1)) as i32;
    send(&[mouse_input(
        to(x, cx),
        to(y, cy),
        MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE,
    )]);
    std::thread::sleep(Duration::from_millis(40));
    send(&[
        mouse_input(0, 0, MOUSEEVENTF_LEFTDOWN),
        mouse_input(0, 0, MOUSEEVENTF_LEFTUP),
    ]);
}

fn key(vkey: VIRTUAL_KEY) -> u32 {
    send(&[key_input(vkey, false), key_input(vkey, true)])
}

fn letters_for(span: Duration, every: Duration) -> i64 {
    let began = Instant::now();
    let mut sent = 0;
    while began.elapsed() < span {
        key(VK_A);
        sent += 1;
        std::thread::sleep(every);
    }
    sent
}

fn bring_to_front(hwnd: HWND, click: bool) -> (bool, u8) {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;
    // SAFETY: the window calls take a window handle by value.
    unsafe {
        let _ = SetForegroundWindow(hwnd);
    }
    std::thread::sleep(Duration::from_millis(200));
    if foreground_window() == hwnd {
        return (true, 1);
    }
    let mut rect = RECT::default();
    // SAFETY: `rect` is a live RECT that the call fills.
    if click && unsafe { GetWindowRect(hwnd, &mut rect) }.is_ok() {
        click_at(
            i64::from((rect.left + rect.right) / 2),
            i64::from(rect.top + 12),
        );
        std::thread::sleep(Duration::from_millis(300));
        if foreground_window() == hwnd {
            return (true, 2);
        }
    }
    // SAFETY: GetWindowThreadProcessId takes a handle and no out pointer
    // here; GetCurrentThreadId takes no argument.
    let (front, this) = unsafe {
        (
            GetWindowThreadProcessId(foreground_window(), None),
            GetCurrentThreadId(),
        )
    };
    if front != 0 && front != this {
        // SAFETY: the two threads' input is joined only around the window
        // calls and detached again before the block ends.
        unsafe {
            let _ = AttachThreadInput(this, front, true);
            let _ = BringWindowToTop(hwnd);
            let _ = SetForegroundWindow(hwnd);
            let _ = AttachThreadInput(this, front, false);
        }
    }
    std::thread::sleep(Duration::from_millis(200));
    if foreground_window() == hwnd {
        return (true, 3);
    }
    // SAFETY: ShowWindow takes a window handle by value.
    unsafe {
        let _ = ShowWindow(hwnd, SW_MINIMIZE);
        let _ = ShowWindow(hwnd, SW_RESTORE);
    }
    std::thread::sleep(Duration::from_millis(300));
    let there = foreground_window() == hwnd;
    (there, if there { 4 } else { 0 })
}

struct PhaseBrowser {
    name: &'static str,
    exe: PathBuf,
    chromium: bool,
    profile: tempfile::TempDir,
    page: String,
    runs: u32,
    front_tick: u32,
    front_method: u8,
}

impl PhaseBrowser {
    fn installed(browser: &'static Browser) -> Option<PhaseBrowser> {
        let exe = browser
            .paths
            .iter()
            .find(|path| Path::new(path).is_file())?;
        PhaseBrowser::at(browser.name, PathBuf::from(exe), browser.chromium)
    }

    fn at(name: &'static str, exe: PathBuf, chromium: bool) -> Option<PhaseBrowser> {
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
        let page_path = profile.path().join("phase.html");
        std::fs::write(&page_path, PHASE_PAGE).ok()?;
        let page = format!("file:///{}", page_path.to_string_lossy().replace('\\', "/"));
        Some(PhaseBrowser {
            name,
            exe,
            chromium,
            profile,
            page,
            runs: 0,
            front_tick: 0,
            front_method: 0,
        })
    }

    fn image(&self) -> String {
        self.exe
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    fn kill(&self) {
        let _ = Command::new("taskkill.exe")
            .args(["/IM", &self.image(), "/F", "/T"])
            .output();
        std::thread::sleep(Duration::from_millis(700));
    }

    fn open(&mut self, test: &str, phase: &str, hold: u64) -> Option<HWND> {
        self.open_with(test, phase, hold, false)
    }

    fn open_with(&mut self, test: &str, phase: &str, hold: u64, click: bool) -> Option<HWND> {
        self.runs += 1;
        let url = format!("{}?phase={phase}&hold={hold}&run={}", self.page, self.runs);
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
        Command::new(&self.exe)
            .args(&args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let prefix = format!("halvo-phase {phase}{} ", self.runs);
        let began = Instant::now();
        while began.elapsed() < Duration::from_secs(40) {
            if let Some((hwnd, _)) = window_titled(&prefix) {
                std::thread::sleep(Duration::from_millis(800));
                let (front, method) = bring_to_front(hwnd, click);
                // SAFETY: GetTickCount takes no argument and reads a counter.
                self.front_tick = unsafe { GetTickCount() };
                self.front_method = method;
                std::thread::sleep(Duration::from_millis(700));
                say(
                    test,
                    format!(
                        "browser={} phase={phase} hold={hold} opened=1 foreground={front} method={method} front_tick={}",
                        self.name, self.front_tick
                    ),
                );
                return Some(hwnd);
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        say(
            test,
            format!("browser={} phase={phase} hold={hold} opened=0", self.name),
        );
        None
    }
}

impl Drop for PhaseBrowser {
    fn drop(&mut self) {
        self.kill();
    }
}

struct Keyboard {
    source: RawInputSource,
    events: Events,
    start: Result<Started, CaptureError>,
}

impl Keyboard {
    fn start(test: &str) -> Keyboard {
        let (sink, events) = capture(8192);
        let mut source = RawInputSource::keyboard(Arc::new(Mutex::new(None)), PasswordGate::new());
        let start = source.begin(SourceContext::new(sink));
        super::start_report(test, "keyboard", &start);
        Keyboard {
            source,
            events,
            start,
        }
    }

    fn stop(mut self) -> Result<(), CaptureError> {
        stop_counted(&mut self.source, "keyboard")
    }

    fn state_within(&self, wanted: FieldState, limit: Duration) -> Option<FieldState> {
        let began = Instant::now();
        loop {
            let state = self.source.field_state();
            if state == Some(wanted) || began.elapsed() >= limit {
                return state;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn settle(&self, span: Duration) {
        let _ = self.events.drain_for(span);
    }

    fn keystrokes_for(&self, span: Duration) -> usize {
        count(&self.events.drain_for(span), Modality::Keystroke)
    }
}

fn element_seen(file: &str) -> String {
    marks_seen(file).0
}

fn marks_with_elements(marks: &[seam::MarkSeen]) -> String {
    let listed: Vec<String> = marks
        .iter()
        .map(|mark| {
            format!(
                "{}@{}:{:?}:{:?}:{}",
                mark.what,
                mark.at.unwrap_or(mark.began),
                mark.state,
                mark.moved,
                mark.element.as_deref().unwrap_or("none").replace(' ', "_")
            )
        })
        .collect();
    listed.join(",")
}

fn element_in(marks: &[seam::MarkSeen], file: &str) -> String {
    marks
        .iter()
        .rev()
        .filter_map(|mark| mark.element.as_deref())
        .find(|element| {
            element
                .to_ascii_lowercase()
                .contains(&format!("file={}", file.to_ascii_lowercase()))
        })
        .unwrap_or("element=none")
        .to_string()
}

fn marks_seen(file: &str) -> (String, String) {
    let marks = seam::take_marks();
    let element = marks
        .iter()
        .rev()
        .filter_map(|mark| mark.element.as_deref())
        .find(|element| {
            element
                .to_ascii_lowercase()
                .contains(&format!("file={}", file.to_ascii_lowercase()))
        })
        .unwrap_or("element=none")
        .to_string();
    (element, newest_marks(&marks))
}

fn part_of<'a>(mark: &'a seam::MarkSeen, key: &str, none: &'a str) -> &'a str {
    mark.element
        .as_deref()
        .and_then(|e| e.split(' ').find(|part| part.starts_with(key)))
        .unwrap_or(none)
}

fn newest_marks(marks: &[seam::MarkSeen]) -> String {
    let newest: Vec<String> = marks
        .iter()
        .rev()
        .take(3)
        .map(|mark| {
            format!(
                "{}:{:?}:{}:{}:{}",
                mark.what,
                mark.state,
                part_of(mark, "file=", "file=none"),
                part_of(mark, "pid_read=", "pid_read=none"),
                part_of(mark, "file_read=", "file_read=none"),
            )
        })
        .collect();
    newest.join(",")
}

fn file_reads(marks: &[seam::MarkSeen]) -> String {
    let (mut ok, mut failed, mut not_read, mut none) = (0, 0, 0, 0);
    for mark in marks
        .iter()
        .filter(|m| m.what == "read" || m.what == "event")
    {
        match part_of(mark, "file_read=", "") {
            "file_read=ok" => ok += 1,
            "" => none += 1,
            read if read.starts_with("file_read=error") => failed += 1,
            _ => not_read += 1,
        }
    }
    format!("file_reads=ok:{ok},failed:{failed},not_read:{not_read},no_element:{none}")
}

fn seen_of(marks: &[seam::MarkSeen]) -> Vec<evidence::Seen> {
    marks
        .iter()
        .map(|mark| evidence::Seen {
            what: mark.what,
            password: mark.state == Some(FieldState::Password),
            element: mark.element.clone(),
            at: mark.at.unwrap_or(mark.began),
        })
        .collect()
}

fn reset(keyboard: &Keyboard, hwnd: HWND) -> Option<PageRead> {
    key(VK_F24);
    keyboard.settle(Duration::from_millis(450));
    let _ = seam::take();
    page_read(hwnd)
}

#[derive(Debug, Default, Clone)]
struct Round {
    counted: bool,
    sent: i64,
    page: PageRead,
    keys: Decisions,
    sink: usize,
    why: Vec<&'static str>,
}

const LANDED_WHILE_HELD_MS: i64 = 100;

impl Round {
    fn by_key(&self) -> Option<Vec<(bool, u8, Option<i64>)>> {
        let landings = self.page.landings.as_bytes();
        (self.keys.letters_in_order.len() == landings.len()).then(|| {
            self.keys
                .letters_in_order
                .iter()
                .zip(landings)
                .enumerate()
                .map(|(i, (recorded, landing))| {
                    (*recorded, *landing, self.page.landed_at.get(i).copied())
                })
                .collect()
        })
    }

    fn password_letters_recorded(&self) -> i64 {
        match self.by_key() {
            Some(keys) => keys
                .iter()
                .filter(|(recorded, landing, _)| *recorded && *landing == b'p')
                .count() as i64,
            None if self.page.password > 0 || self.page.landings.contains('p') => {
                self.keys.letters as i64
            }
            None => 0,
        }
    }

    fn password_letters_recorded_while_the_page_was_held(&self) -> i64 {
        let Some(keys) = self.by_key() else {
            return 0;
        };
        if self.page.freed_at < 0 {
            return 0;
        }
        keys.iter()
            .filter(|(recorded, landing, at)| {
                *recorded
                    && *landing == b'p'
                    && at.is_some_and(|at| at <= self.page.freed_at + LANDED_WHILE_HELD_MS)
            })
            .count() as i64
    }

    fn sink_differs(&self) -> bool {
        self.sink != self.keys.letters + self.keys.tabs + self.keys.other
    }

    fn uncounted_because(&self) -> String {
        if self.counted {
            "none".to_string()
        } else if self.why.is_empty() {
            "the_page_counts".to_string()
        } else {
            self.why.join(",")
        }
    }

    fn line(&self) -> String {
        format!(
            "counted={} letters_sent={} landed_text={} landed_password={} tabs_into_password={} \
             tabs_into_text={} button_downs={} focus_after={} letters_decided={} letters_recorded={} \
             tabs_recorded={} other_recorded={} first_letter_rule={:?} first_tab_rule={:?} \
             keystroke_events={} sink_differs_from_the_seam={} landings={} landed_at_ms={} freed_at_ms={} \
             password_letters_recorded_by_key={} uncounted_because={}",
            u8::from(self.counted),
            self.sent,
            self.page.text,
            self.page.password,
            self.page.tabs_password,
            self.page.tabs_text,
            self.page.downs,
            self.page.focus,
            self.keys.letters_decided,
            self.keys.letters,
            self.keys.tabs,
            self.keys.other,
            self.keys.first_letter_rule,
            self.keys.first_tab_rule,
            self.sink,
            u8::from(self.sink_differs()),
            if self.page.landings.is_empty() {
                "-"
            } else {
                &self.page.landings
            },
            self.page
                .landed_at
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(","),
            self.page.freed_at,
            self.password_letters_recorded(),
            self.uncounted_because(),
        )
    }
}

struct EventCounter {
    stop: Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<Vec<(u32, u32, u32)>>>,
}

thread_local! {
    static COUNTED: std::cell::RefCell<std::collections::HashMap<(u32, u32), u32>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

unsafe extern "system" fn count_event(
    _hook: windows::Win32::UI::Accessibility::HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    _object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    let mut pid = 0;
    // SAFETY: GetWindowThreadProcessId writes the process id into a live u32;
    // a null or gone window leaves it 0.
    unsafe {
        let _ =
            windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(hwnd, Some(&mut pid));
    }
    COUNTED.with(|counted| *counted.borrow_mut().entry((event, pid)).or_insert(0) += 1);
}

impl EventCounter {
    fn start() -> EventCounter {
        use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent};
        use windows::Win32::UI::WindowsAndMessaging::{
            DispatchMessageW, PeekMessageW, TranslateMessage, EVENT_OBJECT_FOCUS,
            EVENT_SYSTEM_FOREGROUND, MSG, PM_REMOVE, WINEVENT_OUTOFCONTEXT,
        };
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let thread = std::thread::spawn(move || {
            // SAFETY: out-of-context hooks of the test's own with a callback of
            // the WINEVENTPROC signature, served by this thread's pump and
            // removed by it before it ends.
            let hooks = unsafe {
                [
                    SetWinEventHook(
                        EVENT_SYSTEM_FOREGROUND,
                        EVENT_SYSTEM_FOREGROUND,
                        None,
                        Some(count_event),
                        0,
                        0,
                        WINEVENT_OUTOFCONTEXT,
                    ),
                    SetWinEventHook(
                        EVENT_OBJECT_FOCUS,
                        EVENT_OBJECT_FOCUS,
                        None,
                        Some(count_event),
                        0,
                        0,
                        WINEVENT_OUTOFCONTEXT,
                    ),
                ]
            };
            let mut msg = MSG::default();
            while !thread_stop.load(Ordering::SeqCst) {
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
            for hook in hooks {
                if !hook.is_invalid() {
                    // SAFETY: the hook was set by this thread above.
                    let _ = unsafe { UnhookWinEvent(hook) };
                }
            }
            COUNTED.with(|counted| {
                counted
                    .borrow()
                    .iter()
                    .map(|((event, pid), count)| (*event, *pid, *count))
                    .collect()
            })
        });
        EventCounter {
            stop,
            thread: Some(thread),
        }
    }

    fn stop(mut self) -> String {
        self.stop.store(true, Ordering::SeqCst);
        let counted = self
            .thread
            .take()
            .and_then(|thread| thread.join().ok())
            .unwrap_or_default();
        let mut by_process: std::collections::BTreeMap<String, (u32, u32)> =
            std::collections::BTreeMap::new();
        for (event, pid, count) in counted {
            let file = seam::process_file_of(pid).unwrap_or_else(|| format!("pid{pid}"));
            let entry = by_process.entry(file).or_insert((0, 0));
            if event == windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_FOREGROUND {
                entry.0 += count;
            } else {
                entry.1 += count;
            }
        }
        by_process
            .iter()
            .map(|(file, (foreground, focus))| {
                format!("{file}:foreground={foreground}:focus={focus}")
            })
            .collect::<Vec<_>>()
            .join(",")
    }
}

impl Drop for EventCounter {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn handler_counts() -> String {
    let seen = seam::detector();
    format!("{}/{}", seen.handler_entered, seen.handler_ended)
}

fn not_in_front(front: bool) -> Option<&'static str> {
    (!front).then_some("own_window_not_in_front")
}

fn page_control(
    test: &str,
    browser: &mut PhaseBrowser,
    keyboard: &Keyboard,
    which: &str,
    counter: Option<EventCounter>,
    opened: Option<HWND>,
) -> usize {
    let opened = opened.or_else(|| browser.open(test, "control", 0));
    let Some(hwnd) = opened else {
        say(
            test,
            format!("browser={} control={which} page=none", browser.name),
        );
        return 0;
    };
    let name = browser.name;
    let first = counter.is_some();
    if first {
        say(
            test,
            format!(
                "browser={name} control={which} first_control page_front_tick={} way_to_the_front={} \
                 handler_counts_after_the_front={}",
                browser.front_tick,
                browser.front_method,
                handler_counts()
            ),
        );
    }
    let mut counter = counter;
    let mut slips = 0;
    let mut typed = |label: &str| loop {
        let attempt = if slips == 0 {
            label.to_string()
        } else {
            format!("{label}-after-slip-{slips}")
        };
        let (sink, second, slipped) = one_control(test, name, keyboard, hwnd, &attempt, first);
        if let Some(counter) = counter.take() {
            say(
                test,
                format!(
                    "browser={name} control={attempt} system_events_from_the_browser_start_to_the_last_letter=[{}]",
                    counter.stop()
                ),
            );
            let before_the_read = handler_counts();
            let uia = seam::focused_element_properties();
            let _ = reset(keyboard, hwnd);
            say(
                test,
                format!(
                    "browser={name} control={attempt} after_the_letters handler_counts_before_the_read={before_the_read} \
                     uia_of_the_focused_element {uia} handler_counts_after_one_more_reset={}",
                    handler_counts()
                ),
            );
        }
        if !slipped {
            return (sink, second);
        }
        slips += 1;
        assert!(
            slips < 3,
            "the control's schedule slipped three times: the page's own move did not come before its first letter"
        );
    };
    let (sink, second) = typed(which);
    if sink >= 5 {
        return sink;
    }
    match second {
        Err(reason) => {
            say(
                test,
                format!("browser={name} control={which} second_control=refused reason={reason}"),
            );
            sink
        }
        Ok(()) => {
            say(
                test,
                format!("browser={name} control={which} second_control=taken first_control_keystroke_events={sink}"),
            );
            typed(&format!("{which}-second")).0
        }
    }
}

fn one_control(
    test: &str,
    name: &str,
    keyboard: &Keyboard,
    hwnd: HWND,
    which: &str,
    first: bool,
) -> (usize, Result<(), &'static str>, bool) {
    let before_the_reset = handler_counts();
    let start = reset(keyboard, hwnd).unwrap_or_default();
    let front = foreground_window() == hwnd;
    let gate_before = keyboard.source.field_state();
    let latches_before = keyboard.source.open_latches().unwrap_or_default();
    let sent = i64::from(type_letters_slowly(5) / 2);
    let sink = keyboard.keystrokes_for(Duration::from_millis(800));
    let keys = seam::take();
    let page = page_read(hwnd).unwrap_or_default();
    let marks = seam::take_marks();
    let after_the_letters = handler_counts();
    let largest_lag = keyboard.source.health().counters.max_lag_ms;
    let r = Round {
        counted: front,
        sent,
        page,
        keys: decisions_of(&keys),
        sink,
        why: Vec::new(),
    };
    say(
        test,
        format!(
            "browser={name} control={which} foreground={front} focus_at_the_reset={} gate_before_the_letters={gate_before:?} {}",
            start.focus,
            r.line()
        ),
    );
    let letters: Vec<&seam::Decided> = keys
        .iter()
        .filter(|key| key.class == seam::KeyClass::Printable)
        .collect();
    let listed: Vec<String> = letters
        .iter()
        .map(|key| {
            format!(
                "input@{}:decided@{}:{:?}:{:?}",
                key.os_tick, key.decided_tick, key.verdict, key.rule
            )
        })
        .collect();
    let mut files: std::collections::HashMap<u32, String> = std::collections::HashMap::new();
    let mut front_of = |pid: u32| {
        files
            .entry(pid)
            .or_insert_with(|| seam::process_file_of(pid).unwrap_or_else(|| format!("pid{pid}")))
            .clone()
    };
    let short_element = |element: Option<&str>| {
        element.map_or("none".to_string(), |element| {
            if first {
                return element.replace(' ', "/");
            }
            element
                .split(' ')
                .filter(|part| part.starts_with("kind=") || part.starts_with("is_password="))
                .collect::<Vec<_>>()
                .join("/")
        })
    };
    let all_marks: Vec<String> = marks
        .iter()
        .map(|mark| {
            let front = if first {
                format!(":front={}", front_of(mark.front_pid))
            } else {
                String::new()
            };
            format!(
                "{}@{}->{}:{:?}:{:?}:{}{front}",
                mark.what,
                mark.began,
                mark.at.map_or("none".to_string(), |at| at.to_string()),
                mark.state,
                mark.moved,
                short_element(mark.element.as_deref())
            )
        })
        .collect();
    let open_latches = latches_before
        .strip_prefix("count=")
        .and_then(|rest| rest.split(' ').next())
        .and_then(|count| count.parse::<usize>().ok());
    let slipped =
        r.page.first_key_at >= 0 && (r.page.moved_at < 0 || r.page.moved_at >= r.page.first_key_at);
    let state = if slipped {
        "schedule_slipped"
    } else if r.sink >= 5 {
        "full"
    } else if gate_before != Some(FieldState::Clear) {
        "gate_not_clear"
    } else if open_latches == Some(0)
        && letters.iter().all(|key| {
            key.verdict == seam::Verdict::Recorded || key.rule == Some(seam::Rule::NotClearedInTime)
        })
        && letters
            .iter()
            .find(|key| key.verdict != seam::Verdict::Recorded)
            .is_some_and(|key| {
                marks.iter().any(|mark| {
                    mark.what == "read"
                        && mark.at.is_some_and(|at| {
                            at.wrapping_sub(key.os_tick) as i32 > 250
                                && mark.began.wrapping_sub(key.os_tick) as i32 >= 0
                        })
                })
            })
    {
        "late_read_no_latch"
    } else {
        "other"
    };
    say(
        test,
        format!(
            "browser={name} control={which} control_state={state} page_moved_at_ms={} first_key_at_page_ms={} \
             handler_counts_before_the_reset={before_the_reset} handler_counts_after_the_letters={after_the_letters} \
             pump_largest_lag_ms={largest_lag} \
             open_latches_before_the_first_letter={latches_before} letters=[{}] \
             marks_since_the_start_kept_by_the_seam={} marks=[{}]",
            r.page.moved_at,
            r.page.first_key_at,
            listed.join(","),
            all_marks.len(),
            all_marks.join(",")
        ),
    );
    if r.sink < 5 {
        super::detector_line(test, "page-control");
    }
    if slipped {
        say(
            test,
            format!(
                "browser={name} control={which} schedule_slipped page_moved_at_ms={} first_key_at_page_ms={} \
                 what it recorded is not read",
                r.page.moved_at, r.page.first_key_at
            ),
        );
        return (r.sink, Err("schedule_slipped"), true);
    }
    assert!(
        front && start.focus == "plain" && sent == 5 && r.page.text == 5,
        "INCONCLUSIVE: the control's page was not in front or did not take its 5 letters"
    );
    assert!(
        !r.sink_differs(),
        "the control's sink held another number of keystroke events than the seam recorded"
    );
    let second = if r.sink < 5 {
        let proof_letters: Vec<evidence::Letter> = letters
            .iter()
            .map(|key| evidence::Letter {
                input: key.os_tick,
                recorded: key.verdict == seam::Verdict::Recorded,
                not_cleared_in_time: key.rule == Some(seam::Rule::NotClearedInTime),
            })
            .collect();
        let proof_marks: Vec<evidence::Mark> = marks
            .iter()
            .map(|mark| evidence::Mark {
                what: mark.what,
                reading: mark.state.map(|state| match state {
                    FieldState::Clear => evidence::Reading::Clear,
                    FieldState::Password => evidence::Reading::Password,
                    FieldState::Unknown => evidence::Reading::Unknown,
                }),
                another_element: mark.moved == Some(seam::Moved::Another),
                began: mark.began,
                at: mark.at,
            })
            .collect();
        evidence::second_control_allowed(
            &proof_letters,
            &proof_marks,
            gate_before == Some(FieldState::Clear),
            open_latches.unwrap_or(usize::MAX),
            r.sink,
        )
    } else {
        Err("the_control_is_full")
    };
    (r.sink, second, false)
}

fn end_round(keyboard: &Keyboard, hwnd: HWND, sent: i64, counted_before: bool) -> Round {
    let sink = keyboard.keystrokes_for(Duration::from_millis(800));
    let keys = decisions();
    let page = page_read(hwnd).unwrap_or_default();
    Round {
        counted: counted_before,
        sent,
        page,
        keys,
        sink,
        why: Vec::new(),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum StallKind {
    Tab,
    Click,
    SelfMove,
}

fn stall_rounds(
    test: &str,
    browser: &mut PhaseBrowser,
    keyboard: &Keyboard,
    kind: StallKind,
    hold: u64,
    rounds: u32,
) -> Vec<Round> {
    let phase = match kind {
        StallKind::Tab => "tab",
        StallKind::Click => "click",
        StallKind::SelfMove => "self",
    };
    let Some(hwnd) = browser.open(test, phase, hold) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for round in 0..rounds {
        let start = reset(keyboard, hwnd).unwrap_or_default();
        let front = foreground_window() == hwnd;
        let ready = front && start.focus == "plain";
        let sent = match kind {
            StallKind::Tab => {
                send(&[
                    key_input(VK_TAB, false),
                    key_input(VK_TAB, true),
                    key_input(VK_A, false),
                    key_input(VK_A, true),
                ]);
                1 + letters_for(Duration::from_millis(hold + 300), Duration::from_millis(60))
            }
            StallKind::Click => {
                let (cx, cy) = screen_size();
                let to = |v: i64, span: i32| (v * 65_535 / i64::from(span - 1)) as i32;
                send(&[mouse_input(
                    to(start.px, cx),
                    to(start.py, cy),
                    MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE,
                )]);
                std::thread::sleep(Duration::from_millis(40));
                send(&[
                    mouse_input(0, 0, MOUSEEVENTF_LEFTDOWN),
                    mouse_input(0, 0, MOUSEEVENTF_LEFTUP),
                    key_input(VK_A, false),
                    key_input(VK_A, true),
                ]);
                1 + letters_for(Duration::from_millis(hold + 300), Duration::from_millis(60))
            }
            StallKind::SelfMove => {
                std::thread::sleep(Duration::from_millis(150));
                letters_for(Duration::from_millis(1_400), Duration::from_millis(60))
            }
        };
        let mut r = end_round(keyboard, hwnd, sent, ready);
        r.why.extend(not_in_front(front));
        if start.focus != "plain" {
            r.why.push("not_on_the_text_input_at_the_reset");
        }
        r.counted = ready
            && match kind {
                StallKind::Tab => r.page.text == 0 && r.page.password == sent,
                StallKind::Click => {
                    r.page.downs == 1 && r.page.text == 0 && r.page.password == sent
                }
                StallKind::SelfMove => r.page.password >= 1,
            };
        say(
            test,
            format!(
                "browser={} phase={phase} hold={hold} round={round} foreground={front} {}",
                browser.name,
                r.line()
            ),
        );
        out.push(r);
    }
    out
}

fn assert_stall(test: &str, phase: &str, rounds: &[Round], wanted: usize) -> usize {
    let counted = rounds.iter().filter(|r| r.counted).count();
    let failing = rounds
        .iter()
        .filter(|r| {
            (r.counted && (r.keys.letters > 0 || r.keys.other > 0 || r.sink > r.keys.tabs))
                || r.password_letters_recorded() > 0
                || r.sink_differs()
        })
        .count();
    say(
        test,
        format!(
            "phase={phase} rounds={} counted={counted} counted_rounds={counted} failing_rounds={failing}",
            rounds.len()
        ),
    );
    assert_eq!(
        failing, 0,
        "{phase}: a letter of the password input, or an event of another class, was recorded, \
         or the sink held another number of keystroke events than the seam recorded"
    );
    assert!(
        counted >= wanted,
        "INCONCLUSIVE: {phase} counted {counted} rounds of {}",
        rounds.len()
    );
    counted
}

fn chromium(name: &str) -> &'static Browser {
    if name == "edge" {
        &EDGE
    } else {
        &CHROME
    }
}

fn open_browser(test: &str, name: &str) -> PhaseBrowser {
    let browser = PhaseBrowser::installed(chromium(name));
    say(
        test,
        format!("browser={name} installed={}", browser.is_some()),
    );
    let browser = browser.expect("the browser is not installed on this runner: FAIL");
    browser.kill();
    browser
}

pub(super) fn stall_phases(test: &'static str, name: &'static str) {
    with_deadline(test, Duration::from_secs(420), move || {
        env_report(test);
        source_built_by_test(test, "keyboard");
        let mut counter = Some(EventCounter::start());
        let mut browser = open_browser(test, name);
        let first_page = browser.open(test, "control", 0);
        let keyboard = Keyboard::start(test);
        assert!(
            keyboard.start.is_ok(),
            "the keyboard source must start: {:?}",
            keyboard.start
        );
        let before = page_control(
            test,
            &mut browser,
            &keyboard,
            "before",
            counter.take(),
            first_page,
        );
        let mut all = Vec::new();
        for kind in [StallKind::Tab, StallKind::Click] {
            for hold in [150, 300] {
                let rounds = stall_rounds(
                    test,
                    &mut browser,
                    &keyboard,
                    kind,
                    hold,
                    super::stall::rounds_for(1, 5),
                );
                all.push((format!("{kind:?}-{hold}"), rounds));
            }
        }
        let after = page_control(test, &mut browser, &keyboard, "after", None, None);
        let late = keyboard.source.health().counters.late_events;
        let stopped = keyboard.stop();
        say(
            test,
            format!("keyboard_stop={} late_value={late}", show(&stopped)),
        );
        assert!(
            before == 5 && after == 5,
            "a control recorded fewer than 5 letters of the text input: control_before={before} control_after={after}"
        );
        for (phase, rounds) in &all {
            assert_stall(test, phase, rounds, super::stall::rounds_for(1, 3) as usize);
        }
    });
}

pub(super) fn self_move_phases(test: &'static str, name: &'static str) -> i64 {
    let total = Arc::new(AtomicI64::new(0));
    let out = Arc::clone(&total);
    with_deadline(test, Duration::from_secs(300), move || {
        let mut recorded = 0;
        source_built_by_test(test, "keyboard");
        let mut browser = open_browser(test, name);
        let keyboard = Keyboard::start(test);
        assert!(
            keyboard.start.is_ok(),
            "the keyboard source must start: {:?}",
            keyboard.start
        );
        let mut counted = 0;
        let mut another_way = 0;
        let mut sink_differs = 0;
        for hold in [150, 300] {
            for r in stall_rounds(
                test,
                &mut browser,
                &keyboard,
                StallKind::SelfMove,
                hold,
                super::stall::rounds_for(1, 5),
            ) {
                counted += usize::from(r.counted);
                let held = r.password_letters_recorded_while_the_page_was_held();
                if r.counted {
                    recorded += held;
                    another_way += r.password_letters_recorded() - held;
                } else {
                    another_way += r.password_letters_recorded();
                }
                another_way += (r.keys.other + r.keys.tabs) as i64;
                sink_differs += usize::from(r.sink_differs());
            }
        }
        let _ = keyboard.stop();
        say(
            test,
            format!(
                "phase=self counted={counted} password_letters_recorded_while_the_page_was_held={recorded} \
                 keys_recorded_another_way={another_way} rounds_where_the_sink_differs_from_the_seam={sink_differs}"
            ),
        );
        out.store(recorded, Ordering::SeqCst);
        assert!(
            counted >= super::stall::rounds_for(1, 5) as usize,
            "INCONCLUSIVE: the self-move phase counted {counted} rounds"
        );
        assert_eq!(
            (another_way, sink_differs),
            (0, 0),
            "a key was recorded in another way than a letter that landed in the password input \
             while the page held its thread before its own move, or the sink differs from the seam"
        );
    });
    total.load(Ordering::SeqCst)
}

fn swap_round(
    test: &str,
    browser_name: &str,
    keyboard: &Keyboard,
    hwnd: HWND,
    phase: &str,
    round: u32,
) -> (Round, Option<Round>) {
    let start = reset(keyboard, hwnd).unwrap_or_default();
    let front = foreground_window() == hwnd;
    let mut shown = None;
    let sent = match phase {
        "swap-timer" | "swap-focus" => {
            std::thread::sleep(Duration::from_millis(900));
            i64::from(type_letters_slowly(5) / 2)
        }
        "swap-keydown" | "swap-keyup" => {
            std::thread::sleep(Duration::from_millis(300));
            i64::from(type_letters_slowly(5) / 2)
        }
        "show-hide" => {
            std::thread::sleep(Duration::from_millis(500));
            let first = i64::from(type_letters_slowly(3) / 2);
            std::thread::sleep(Duration::from_millis(1_000));
            first + i64::from(type_letters_slowly(3) / 2)
        }
        "show-control" => {
            std::thread::sleep(Duration::from_millis(800));
            let first = i64::from(type_letters_slowly(3) / 2);
            shown = Some(end_round(keyboard, hwnd, first, front));
            std::thread::sleep(Duration::from_millis(200));
            i64::from(type_letters_slowly(3) / 2)
        }
        _ => 0,
    };
    let mut r = end_round(keyboard, hwnd, sent, front);
    if let Some(shown) = &shown {
        r.page.text -= shown.page.text;
        r.page.password -= shown.page.password;
        let first = shown.page.landings.len().min(r.page.landings.len());
        r.page.landings = r.page.landings[first..].to_string();
        let first = shown.page.landed_at.len().min(r.page.landed_at.len());
        r.page.landed_at.drain(..first);
    }
    r.why.extend(not_in_front(front));
    let start_ok = match phase {
        "show-hide" | "show-control" => start.focus == "password",
        "swap-focus" => true,
        _ => start.focus == "plain",
    };
    r.counted = front && start_ok && r.page.text + r.page.password == sent;
    if let Some(shown) = &shown {
        say(
            test,
            format!(
                "browser={browser_name} phase={phase} round={round} part=shown printed_only=1 {}",
                shown.line()
            ),
        );
    }
    say(
        test,
        format!(
            "browser={browser_name} phase={phase} round={round} {}",
            r.line()
        ),
    );
    (r, shown)
}

pub(super) fn swap_phases(test: &'static str, name: &'static str) {
    with_deadline(test, Duration::from_secs(420), move || {
        env_report(test);
        source_built_by_test(test, "keyboard");
        let mut counter = Some(EventCounter::start());
        let mut browser = open_browser(test, name);
        let first_page = browser.open(test, "control", 0);
        let keyboard = Keyboard::start(test);
        assert!(
            keyboard.start.is_ok(),
            "the keyboard source must start: {:?}",
            keyboard.start
        );
        let before = page_control(
            test,
            &mut browser,
            &keyboard,
            "before",
            counter.take(),
            first_page,
        );
        let mut failures = Vec::new();
        let mut thin = Vec::new();
        for phase in [
            "swap-timer",
            "swap-focus",
            "swap-keydown",
            "swap-keyup",
            "show-hide",
            "show-control",
        ] {
            let Some(hwnd) = browser.open(test, phase, 0) else {
                thin.push(phase);
                continue;
            };
            let mut counted = 0;
            let rounds = super::stall::rounds_for(1, 3);
            for round in 0..rounds {
                let (r, shown) = swap_round(test, name, &keyboard, hwnd, phase, round);
                counted += u32::from(r.counted);
                let failed = r.sink_differs()
                    || shown.as_ref().is_some_and(Round::sink_differs)
                    || r.keys.other > 0
                    || if phase == "show-hide" {
                        r.keys.letters > 0
                    } else {
                        r.password_letters_recorded() > 0
                    };
                if failed {
                    failures.push(format!("{phase}#{round}"));
                }
            }
            say(
                test,
                format!("browser={name} phase={phase} rounds={rounds} counted_rounds={counted}"),
            );
            if counted < rounds.min(2) {
                thin.push(phase);
            }
        }
        let after = page_control(test, &mut browser, &keyboard, "after", None, None);
        let stopped = keyboard.stop();
        say(
            test,
            format!(
                "keyboard_stop={} failing_rounds={} thin_phases={}",
                show(&stopped),
                failures.len(),
                thin.len()
            ),
        );
        assert!(
            before == 5 && after == 5,
            "a control recorded fewer than 5 letters of the text input: control_before={before} control_after={after}"
        );
        assert!(
            failures.is_empty(),
            "a letter of a password field was recorded, or the sink differs from the seam: {failures:?}"
        );
        assert!(
            thin.is_empty(),
            "INCONCLUSIVE: phases with fewer than 2 counted rounds: {thin:?}"
        );
    });
}

pub(super) fn held_swap_phases(test: &'static str, name: &'static str) -> i64 {
    let total = Arc::new(AtomicI64::new(0));
    let out = Arc::clone(&total);
    with_deadline(test, Duration::from_secs(600), move || {
        let mut recorded = 0;
        source_built_by_test(test, "keyboard");
        let mut browser = open_browser(test, name);
        let keyboard = Keyboard::start(test);
        assert!(
            keyboard.start.is_ok(),
            "the keyboard source must start: {:?}",
            keyboard.start
        );
        let mut counted = 0;
        let mut another_way = 0;
        let mut sink_differs = 0;
        for phase in ["swap-timer", "swap-focus", "swap-keydown", "swap-keyup"] {
            for hold in [150, 300] {
                let Some(hwnd) = browser.open(test, phase, hold) else {
                    continue;
                };
                for round in 0..2 {
                    let start = reset(&keyboard, hwnd).unwrap_or_default();
                    let front = foreground_window() == hwnd;
                    if phase == "swap-timer" || phase == "swap-focus" {
                        std::thread::sleep(Duration::from_millis(150));
                    }
                    let sent = letters_for(Duration::from_millis(1_300), Duration::from_millis(60));
                    let mut r = end_round(&keyboard, hwnd, sent, front);
                    r.why.extend(not_in_front(front));
                    r.counted = front
                        && (phase == "swap-focus" || start.focus == "plain")
                        && r.page.password >= 1;
                    counted += usize::from(r.counted);
                    let held = r.password_letters_recorded_while_the_page_was_held();
                    if r.counted {
                        recorded += held;
                        another_way += r.password_letters_recorded() - held;
                    } else {
                        another_way += r.password_letters_recorded();
                    }
                    another_way += (r.keys.other + r.keys.tabs) as i64;
                    sink_differs += usize::from(r.sink_differs());
                    say(
                        test,
                        format!(
                            "browser={name} phase={phase}-held hold={hold} round={round} {}",
                            r.line()
                        ),
                    );
                }
            }
        }
        let _ = keyboard.stop();
        say(
            test,
            format!(
                "phase=held-swaps counted={counted} password_letters_recorded_while_the_page_was_held={recorded} \
                 keys_recorded_another_way={another_way} rounds_where_the_sink_differs_from_the_seam={sink_differs}"
            ),
        );
        out.store(recorded, Ordering::SeqCst);
        assert!(
            counted >= 8,
            "INCONCLUSIVE: the held swaps counted {counted} rounds"
        );
        assert_eq!(
            (another_way, sink_differs),
            (0, 0),
            "a key was recorded in another way than a letter that landed in the swapped input \
             while the page held its thread, or the sink differs from the seam"
        );
    });
    total.load(Ordering::SeqCst)
}

mod burst_rounds {
    use crate::seam::Rule;

    /// The keys that a counted round of the burst recorded, by kind.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub(super) struct Records {
        pub(super) letters: usize,
        pub(super) tabs: usize,
        pub(super) other: usize,
        pub(super) keystroke_events: usize,
    }

    /// What the verdict of a counted round of the burst reads.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) struct Counted {
        pub(super) first_letter_rule: Option<Rule>,
        pub(super) records: Records,
        pub(super) freed_at_ms: i64,
    }

    /// How a counted round of the burst that passed was decided.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) enum Decided {
        ByARuleOfTheSwap,
        ByTheTimeRule { freed_at_ms: i64 },
    }

    /// The verdict of one counted round of the burst.
    pub(super) fn decided(round: &Counted) -> Result<Decided, String> {
        let r = round.records;
        if [r.letters, r.tabs, r.other, r.keystroke_events]
            .iter()
            .any(|&count| count > 0)
        {
            return Err(format!(
                "the burst recorded letters_recorded={} tabs_recorded={} other_recorded={} \
                 keystroke_events={} with first_letter_rule={:?}",
                r.letters, r.tabs, r.other, r.keystroke_events, round.first_letter_rule
            ));
        }
        match round.first_letter_rule {
            None => Err(format!(
                "the burst's first letter was decided by {:?} and not by a rule of the swap's case",
                round.first_letter_rule
            )),
            Some(Rule::NotClearedInTime) => Ok(Decided::ByTheTimeRule {
                freed_at_ms: round.freed_at_ms,
            }),
            Some(_) => Ok(Decided::ByARuleOfTheSwap),
        }
    }

    fn counted(first_letter_rule: Option<Rule>, records: Records) -> Counted {
        Counted {
            first_letter_rule,
            records,
            freed_at_ms: 1564,
        }
    }

    fn fails_whatever_the_rule(records: Records) {
        for rule in [
            None,
            Some(Rule::NotClearedInTime),
            Some(Rule::MarkAfterTheKey),
        ] {
            let verdict = decided(&counted(rule, records));
            assert!(
                verdict.is_err(),
                "a round that recorded {records:?} passed under {rule:?}: {verdict:?}"
            );
        }
    }

    #[test]
    #[ignore = "a made-up round of the burst: 52-windows-browser runs it by its exact name"]
    fn a_counted_round_that_the_time_rule_decided_with_nothing_recorded_passes() {
        assert_eq!(
            decided(&counted(Some(Rule::NotClearedInTime), Records::default())),
            Ok(Decided::ByTheTimeRule { freed_at_ms: 1564 })
        );
    }

    #[test]
    #[ignore = "a made-up round of the burst: 52-windows-browser runs it by its exact name"]
    fn a_counted_round_that_a_rule_of_the_swap_decided_with_nothing_recorded_passes() {
        assert_eq!(
            decided(&counted(Some(Rule::MarkAfterTheKey), Records::default())),
            Ok(Decided::ByARuleOfTheSwap)
        );
    }

    #[test]
    #[ignore = "a made-up round of the burst: 52-windows-browser runs it by its exact name"]
    fn a_counted_round_whose_first_letter_no_rule_decided_fails() {
        assert!(decided(&counted(None, Records::default())).is_err());
    }

    #[test]
    #[ignore = "a made-up round of the burst: 52-windows-browser runs it by its exact name"]
    fn a_counted_round_with_a_recorded_letter_fails_whatever_the_rule() {
        fails_whatever_the_rule(Records {
            letters: 1,
            ..Records::default()
        });
    }

    #[test]
    #[ignore = "a made-up round of the burst: 52-windows-browser runs it by its exact name"]
    fn a_counted_round_with_a_recorded_tab_fails_whatever_the_rule() {
        fails_whatever_the_rule(Records {
            tabs: 1,
            ..Records::default()
        });
    }

    #[test]
    #[ignore = "a made-up round of the burst: 52-windows-browser runs it by its exact name"]
    fn a_counted_round_with_another_recorded_key_fails_whatever_the_rule() {
        fails_whatever_the_rule(Records {
            other: 1,
            ..Records::default()
        });
    }

    #[test]
    #[ignore = "a made-up round of the burst: 52-windows-browser runs it by its exact name"]
    fn a_counted_round_with_a_keystroke_event_fails_whatever_the_rule() {
        fails_whatever_the_rule(Records {
            keystroke_events: 1,
            ..Records::default()
        });
    }
}

pub(super) fn burst_phase(test: &'static str, name: &'static str) -> i64 {
    let total = Arc::new(AtomicI64::new(0));
    let out = Arc::clone(&total);
    with_deadline(test, Duration::from_secs(240), move || {
        let mut recorded = 0;
        source_built_by_test(test, "keyboard");
        let mut browser = open_browser(test, name);
        let keyboard = Keyboard::start(test);
        assert!(
            keyboard.start.is_ok(),
            "the keyboard source must start: {:?}",
            keyboard.start
        );
        let hwnd = browser
            .open(test, "burst", 0)
            .expect("INCONCLUSIVE: no page window");
        let mut counted = 0;
        let mut sink_differs = 0;
        let mut closed_before_the_step = 0;
        let mut waited_for_the_type = 0;
        for round in 0..5 {
            let _ = seam::take_marks();
            let gate_before_the_step = keyboard.source.field_state();
            closed_before_the_step += u32::from(gate_before_the_step != Some(FieldState::Clear));
            let place = page_read(hwnd).unwrap_or_default();
            click_at(place.tx, place.ty);
            std::thread::sleep(Duration::from_millis(130));
            let mut start = reset(&keyboard, hwnd).unwrap_or_default();
            let type_at_the_reset = start.plain_type.clone();
            if start.plain_type != "text" {
                waited_for_the_type += 1;
                let began = Instant::now();
                while start.plain_type != "text" && began.elapsed() < Duration::from_millis(450) {
                    std::thread::sleep(Duration::from_millis(20));
                    start = page_read(hwnd).unwrap_or_default();
                }
            }
            if start.plain_type != "text" {
                say(
                    test,
                    format!(
                        "browser={name} phase=burst round={round} uncounted: the page's input type \
                         after the reset is {} focus={} text={} password={}",
                        start.plain_type, start.focus, start.text, start.password
                    ),
                );
                continue;
            }
            let front = foreground_window() == hwnd;
            let control_sent = i64::from(type_letters_slowly(3) / 2);
            let control = keyboard.keystrokes_for(Duration::from_millis(600));
            let control_marks = seam::take_marks();
            let control_keys = seam::take();
            let control_by_the_seam = decisions_of(&control_keys);
            sink_differs += usize::from(
                control
                    != control_by_the_seam.letters
                        + control_by_the_seam.tabs
                        + control_by_the_seam.other,
            );
            super::control_reads(test, "burst-control", &control_marks, &control_keys);
            std::thread::sleep(Duration::from_millis(250));
            let gate_before = keyboard.source.field_state();
            let control_page = page_read(hwnd).unwrap_or_default();
            let on_the_text_input = front
                && start.focus == "plain"
                && control_page.focus == "plain"
                && control_page.text == control_sent;
            if on_the_text_input && (gate_before != Some(FieldState::Clear) || control < 3) {
                super::detector_line(test, &format!("burst-round-{round}"));
                let element = control_marks
                    .iter()
                    .rev()
                    .find_map(|mark| mark.element.as_deref())
                    .unwrap_or("none")
                    .replace(' ', "_");
                say(
                    test,
                    format!(
                        "browser={name} phase=burst round={round} gate_before_the_step={gate_before_the_step:?} \
                         gate_before_the_burst={gate_before:?} element={element} control_letters_sent={control_sent} \
                         control_keystroke_events={control} marks=[{}]",
                        marks_with_elements(&control_marks)
                    ),
                );
                panic!(
                    "the gate was not open on the text input after a click and the page's own move: \
                     round {round}, gate {gate_before:?}, control {control} of 3"
                );
            }
            let mut burst = Vec::new();
            for _ in 0..5 {
                burst.push(key_input(VK_A, false));
                burst.push(key_input(VK_A, true));
            }
            let sent = i64::from(send(&burst) / 2);
            let mut r = end_round(&keyboard, hwnd, sent, front);
            let open = gate_before == Some(FieldState::Clear);
            r.counted = front
                && start.focus == "plain"
                && control == 3
                && open
                && r.page.text == control_sent
                && r.page.password == sent;
            counted += u32::from(r.counted);
            r.why.extend(not_in_front(front));
            let password_recorded = r.password_letters_recorded();
            if r.counted {
                recorded += password_recorded;
            }
            sink_differs += usize::from(r.sink_differs());
            say(
                test,
                format!(
                    "browser={name} phase=burst round={round} gate_before_the_step={gate_before_the_step:?} \
                     type_at_the_reset={type_at_the_reset} control_letters_sent={control_sent} \
                     control_keystroke_events={control} gate_before_the_burst={gate_before:?} {}",
                    r.line()
                ),
            );
            if r.counted {
                if let Err(why) = burst_rounds::decided(&burst_rounds::Counted {
                    first_letter_rule: r.keys.first_letter_rule,
                    records: burst_rounds::Records {
                        letters: r.keys.letters,
                        tabs: r.keys.tabs,
                        other: r.keys.other,
                        keystroke_events: r.sink,
                    },
                    freed_at_ms: r.page.freed_at,
                }) {
                    panic!("round {round}: {why}");
                }
            }
        }
        let _ = keyboard.stop();
        say(
            test,
            format!(
                "phase=burst counted={counted} password_letters_recorded={recorded} \
                 rounds_whose_step_found_a_closed_gate={closed_before_the_step} \
                 rounds_that_waited_for_the_page_type={waited_for_the_type} \
                 rounds_where_the_sink_differs_from_the_seam={sink_differs}"
            ),
        );
        out.store(recorded, Ordering::SeqCst);
        assert!(
            counted >= 1,
            "the burst counted no round that began from an open gate"
        );
        assert_eq!(
            sink_differs, 0,
            "the sink held another number of keystroke events than the seam recorded"
        );
    });
    total.load(Ordering::SeqCst)
}

fn letter_and_tab_out_of_a_swapped_field(test: &'static str, name: &'static str) -> i64 {
    let total = Arc::new(AtomicI64::new(0));
    let out = Arc::clone(&total);
    with_deadline(test, Duration::from_secs(240), move || {
        let mut recorded = 0;
        source_built_by_test(test, "keyboard");
        let mut browser = open_browser(test, name);
        let keyboard = Keyboard::start(test);
        assert!(
            keyboard.start.is_ok(),
            "the keyboard source must start: {:?}",
            keyboard.start
        );
        let hwnd = browser
            .open(test, "tab-after", 0)
            .expect("INCONCLUSIVE: no page window");
        let mut counted = 0;
        let mut sink_differs = 0;
        let rounds = super::stall::rounds_for(1, 5);
        for round in 0..rounds {
            let _ = seam::take_marks();
            let start = reset(&keyboard, hwnd).unwrap_or_default();
            let front = foreground_window() == hwnd;
            std::thread::sleep(Duration::from_millis(800));
            let swapped = page_read(hwnd).is_some_and(|p| p.plain_type == "password");
            key(VK_A);
            std::thread::sleep(Duration::from_millis(50));
            key(VK_TAB);
            let sink = keyboard.keystrokes_for(Duration::from_millis(800));
            let keys = seam::take();
            super::mark_trail(
                test,
                &format!("tab-after round={round}"),
                &seam::take_marks(),
                &keys,
            );
            let mut r = Round {
                counted: front,
                sent: 1,
                page: page_read(hwnd).unwrap_or_default(),
                keys: decisions_of(&keys),
                sink,
                why: not_in_front(front).into_iter().collect(),
            };
            r.counted = front
                && start.focus == "plain"
                && swapped
                && r.page.password == 1
                && r.page.tabs_password == 1;
            counted += u32::from(r.counted);
            if r.counted {
                recorded += (r.keys.letters + r.keys.tabs + r.keys.other).max(r.sink) as i64;
            }
            if r.sink_differs() {
                sink_differs += 1;
            }
            say(
                test,
                format!(
                    "browser={name} phase=tab-after round={round} swapped={swapped} {}",
                    r.line()
                ),
            );
        }
        let _ = keyboard.stop();
        say(
            test,
            format!(
                "phase=tab-after counted={counted} password_keys_recorded={recorded} \
                 rounds_where_the_sink_differs_from_the_seam={sink_differs}"
            ),
        );
        out.store(recorded, Ordering::SeqCst);
        assert!(
            counted >= rounds.min(3),
            "INCONCLUSIVE: the phase counted {counted} rounds"
        );
        assert_eq!(
            sink_differs, 0,
            "the sink held another number of keystroke events than the seam recorded"
        );
    });
    total.load(Ordering::SeqCst)
}

pub(super) fn cold_and_warm(test: &'static str, name: &'static str) {
    with_deadline(test, Duration::from_secs(300), move || {
        env_report(test);
        source_built_by_test(test, "keyboard");
        let mut browser = open_browser(test, name);
        let file = browser.image();
        let keyboard = Keyboard::start(test);
        assert!(
            keyboard.start.is_ok(),
            "the keyboard source must start: {:?}",
            keyboard.start
        );
        let _ = seam::take_marks();
        let cold = browser
            .open_with(test, "cold", 0, true)
            .expect("INCONCLUSIVE: no page window");
        let cold_front = foreground_window() == cold;
        let cold_state = keyboard.state_within(FieldState::Password, Duration::from_secs(5));
        let mut cold_marks = seam::take_marks();
        let cold_newest = newest_marks(&cold_marks);
        let cold_page = page_read(cold).unwrap_or_default();
        let _ = seam::take();
        let sent = i64::from(type_letters_slowly(3) / 2);
        let cold_sink = keyboard.keystrokes_for(Duration::from_millis(800));
        let cold_keys = seam::take();
        cold_marks.extend(seam::take_marks());
        let cold_round = Round {
            counted: cold_front,
            sent,
            page: page_read(cold).unwrap_or_default(),
            keys: decisions_of(&cold_keys),
            sink: cold_sink,
            why: not_in_front(cold_front).into_iter().collect(),
        };
        let cold_seen = seen_of(&cold_marks);
        let cold_evidence = cold_keys
            .iter()
            .find(|key| key.class == seam::KeyClass::Printable)
            .ok_or_else(|| "no letter was decided".to_string())
            .and_then(|key| evidence::cold_evidence(&cold_seen, key.os_tick, key.decided_tick));
        let cold_older = evidence::newest_mark_of(&cold_seen, &file);
        say(
            test,
            format!(
                "browser={name} order=cold foreground={cold_front} focus_before={} gate_while_focused={cold_state:?} \
                 {} evidence_marks={} newest_marks=[{cold_newest}] {} older_mark_of_the_page=[{cold_older}]",
                cold_page.focus,
                cold_round.line(),
                cold_evidence
                    .as_ref()
                    .map_or_else(|why| why.replace(' ', "_"), usize::to_string),
                file_reads(&cold_marks),
            ),
        );

        let warm = browser
            .open(test, "warm", 0)
            .expect("INCONCLUSIVE: no page window");
        let _ = seam::take_marks();
        let start = reset(&keyboard, warm).unwrap_or_default();
        let warm_front = foreground_window() == warm;
        let plain_state = keyboard.state_within(FieldState::Clear, Duration::from_secs(3));
        if plain_state == Some(FieldState::Unknown) {
            super::detector_line(test, "warm-control");
        }
        let _ = seam::take();
        let sent = i64::from(type_letters_slowly(5) / 2);
        let control_sink = keyboard.keystrokes_for(Duration::from_millis(800));
        let control_keys = seam::take();
        let control_marks = seam::take_marks();
        super::control_reads(test, "warm-control", &control_marks, &control_keys);
        let plain_element = element_in(&control_marks, &file);
        let control_reads = file_reads(&control_marks);
        let control = Round {
            counted: warm_front,
            sent,
            page: page_read(warm).unwrap_or_default(),
            keys: decisions_of(&control_keys),
            sink: control_sink,
            why: not_in_front(warm_front).into_iter().collect(),
        };
        key(VK_F22);
        let warm_state = keyboard.state_within(FieldState::Password, Duration::from_secs(3));
        let warm_marks = seam::take_marks();
        let warm_element = element_in(&warm_marks, &file);
        let warm_newest = newest_marks(&warm_marks);
        let warm_reads = file_reads(&warm_marks);
        let _ = seam::take();
        let sent = i64::from(type_letters_slowly(3) / 2);
        let warm_round = end_round(&keyboard, warm, sent, warm_front);
        let stopped = keyboard.stop();
        say(
            test,
            format!(
                "browser={name} order=warm-control foreground={warm_front} focus_before={} \
                 gate_while_focused={plain_state:?} {} {control_reads} {plain_element}",
                start.focus,
                control.line()
            ),
        );
        say(
            test,
            format!(
                "browser={name} order=warm gate_while_focused={warm_state:?} {} newest_marks=[{warm_newest}] \
                 {warm_reads} {warm_element} keyboard_stop={}",
                warm_round.line(),
                show(&stopped)
            ),
        );
        assert!(
            cold_front
                && warm_front
                && cold_round.page.password == 3
                && warm_round.page.password == 3,
            "INCONCLUSIVE: the page window was not in front or did not take the letters"
        );
        assert_eq!(
            cold_state,
            Some(FieldState::Password),
            "cold: the gate did not read Password while the input had the focus"
        );
        assert!(
            cold_evidence.is_ok(),
            "cold: the marks that decided the letters were not a reading of the password input: {cold_evidence:?}"
        );
        assert_eq!(
            (cold_round.keys.letters, cold_round.sink),
            (0, 0),
            "cold: a letter of the password input was recorded"
        );
        assert_eq!(
            plain_state,
            Some(FieldState::Clear),
            "warm: the gate did not read Clear in the plain input"
        );
        assert_eq!(
            control.page.text, 5,
            "INCONCLUSIVE: the plain input did not take the control's letters"
        );
        assert_eq!(
            (control.keys.letters, control.sink),
            (5, 5),
            "warm: the control's letters of the plain input were not recorded"
        );
        assert_eq!(
            warm_state,
            Some(FieldState::Password),
            "warm: the gate did not read Password while the input had the focus"
        );
        assert!(
            warm_element.contains("is_password=1"),
            "warm: the gate's reading was not of the password input"
        );
        assert_eq!(
            (warm_round.keys.letters, warm_round.sink),
            (0, 0),
            "warm: a letter of the password input was recorded"
        );
    });
}

fn renamed_copy(source_exe: &Path, into: &Path, name: &str) -> Option<PathBuf> {
    let source = source_exe.parent()?;
    let copied = Command::new("robocopy.exe")
        .args([
            source.to_string_lossy().as_ref(),
            into.to_string_lossy().as_ref(),
            "/E",
            "/NFL",
            "/NDL",
            "/NJH",
            "/NJS",
            "/NP",
        ])
        .output()
        .ok()?;
    let _ = copied;
    let renamed = into.join(name);
    let original = into.join(source_exe.file_name()?);
    std::fs::rename(original, &renamed).ok()?;
    Some(renamed)
}

pub(super) fn renamed_copy_phases(test: &'static str, browser: &'static Browser) {
    with_deadline(test, Duration::from_secs(420), move || {
        env_report(test);
        source_built_by_test(test, "keyboard");
        let exe = browser
            .paths
            .iter()
            .map(PathBuf::from)
            .find(|p| p.is_file());
        say(
            test,
            format!("browser={} installed={}", browser.name, exe.is_some()),
        );
        let exe = exe.expect("the browser is not installed on this runner: FAIL");
        let dir = scratch_dir();
        let copy = renamed_copy(&exe, &dir.path().join("copy"), "renamed-browser.exe");
        say(test, format!("renamed_copy_made={}", copy.is_some()));
        let copy = copy.expect("INCONCLUSIVE: the copy of the browser was not made");
        let mut renamed = PhaseBrowser::at(browser.name, copy, browser.chromium)
            .expect("INCONCLUSIVE: no profile for the copy");
        renamed.kill();
        let keyboard = Keyboard::start(test);
        assert!(
            keyboard.start.is_ok(),
            "the keyboard source must start: {:?}",
            keyboard.start
        );
        let _ = seam::take_marks();
        let hwnd = renamed
            .open(test, "warm", 0)
            .expect("INCONCLUSIVE: no page window");
        let start = reset(&keyboard, hwnd).unwrap_or_default();
        let front = foreground_window() == hwnd;
        let plain_state = keyboard.state_within(FieldState::Clear, Duration::from_secs(2));
        let plain_element = element_seen("renamed-browser.exe");
        let sent = i64::from(type_letters_slowly(5) / 2);
        let plain = end_round(&keyboard, hwnd, sent, front);
        key(VK_F22);
        std::thread::sleep(Duration::from_millis(500));
        let password_state = keyboard.source.field_state();
        let password_element = element_seen("renamed-browser.exe");
        let _ = seam::take();
        let sent = i64::from(type_letters_slowly(5) / 2);
        let password = end_round(&keyboard, hwnd, sent, front);
        let stopped = keyboard.stop();
        say(
            test,
            format!(
                "browser=renamed-{} field=plain foreground={front} focus_before={} gate_while_focused={plain_state:?} {} {plain_element}",
                browser.name,
                start.focus,
                plain.line()
            ),
        );
        say(
            test,
            format!(
                "browser=renamed-{} field=password gate_while_focused={password_state:?} {} {password_element} keyboard_stop={}",
                browser.name,
                password.line(),
                show(&stopped)
            ),
        );
        let landed = plain.page.text == 5 && password.page.password == 5;
        let read_the_copy = plain_element.contains("file=renamed-browser.exe");
        assert!(
            front
                && if browser.renamed_copy_keys_reach_the_page {
                    landed
                } else {
                    read_the_copy
                },
            "INCONCLUSIVE: the copy's window was not in front, or the copy did not take the letters"
        );
        assert_eq!(
            plain_state,
            Some(FieldState::Unknown),
            "the copy's plain input did not read Unknown"
        );
        assert!(
            matches!(
                password_state,
                Some(FieldState::Unknown | FieldState::Password)
            ),
            "the copy's password input read neither Unknown nor Password"
        );
        assert_eq!(
            (
                plain.keys.letters + password.keys.letters,
                plain.sink + password.sink
            ),
            (0, 0),
            "a letter typed into the renamed copy was recorded"
        );
    });
}

fn wait_for_the_pages_own_move(hwnd: HWND) -> PageRead {
    let began = Instant::now();
    let mut page = page_read(hwnd).unwrap_or_default();
    while page.moved_at < 0 && began.elapsed() < Duration::from_secs(8) {
        std::thread::sleep(Duration::from_millis(50));
        page = page_read(hwnd).unwrap_or_default();
    }
    std::thread::sleep(Duration::from_millis(300));
    page_read(hwnd).unwrap_or(page)
}

fn full_marks(marks: &[seam::MarkSeen]) -> String {
    let mut files: std::collections::HashMap<u32, String> = std::collections::HashMap::new();
    marks
        .iter()
        .map(|mark| {
            let front = files
                .entry(mark.front_pid)
                .or_insert_with(|| {
                    seam::process_file_of(mark.front_pid)
                        .unwrap_or_else(|| format!("pid{}", mark.front_pid))
                })
                .clone();
            format!(
                "{}@{}->{}:{:?}:{:?}:{}:front={front}",
                mark.what,
                mark.began,
                mark.at.map_or("none".to_string(), |at| at.to_string()),
                mark.state,
                mark.moved,
                mark.element.as_deref().unwrap_or("none").replace(' ', "/")
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

pub(super) fn after_a_start_behind_another_window(test: &'static str, name: &'static str) {
    with_deadline(test, Duration::from_secs(240), move || {
        env_report(test);
        source_built_by_test(test, "keyboard");
        let counter = EventCounter::start();
        let mut browser = open_browser(test, name);
        let keyboard = Keyboard::start(test);
        assert!(
            keyboard.start.is_ok(),
            "the keyboard source must start: {:?}",
            keyboard.start
        );
        let hwnd = browser
            .open(test, "after-start", 0)
            .expect("INCONCLUSIVE: no page window");
        let front_tick = browser.front_tick;
        let at_the_front = handler_counts();
        let moved = wait_for_the_pages_own_move(hwnd);
        let front = foreground_window() == hwnd;
        let gate_before = keyboard.source.field_state();
        let latches_before = keyboard.source.open_latches().unwrap_or_default();
        let _ = seam::take();
        let sent = i64::from(type_letters_slowly(5) / 2);
        let sink = keyboard.keystrokes_for(Duration::from_millis(800));
        let keys = seam::take();
        let marks = seam::take_marks();
        let after_the_letters = handler_counts();
        let page = page_read(hwnd).unwrap_or_default();
        let withheld = keyboard.source.health().counters.keys_withheld;
        let events = counter.stop();
        let letters: Vec<&seam::Decided> = keys
            .iter()
            .filter(|key| key.class == seam::KeyClass::Printable)
            .collect();
        let typed: Vec<evidence::Typed> = letters
            .iter()
            .map(|key| evidence::Typed {
                recorded: key.verdict == seam::Verdict::Recorded,
                state_at_the_key: key.rule == Some(seam::Rule::StateAtTheKey),
            })
            .collect();
        let proof_marks: Vec<evidence::Mark> = marks
            .iter()
            .map(|mark| evidence::Mark {
                what: mark.what,
                reading: mark.state.map(|state| match state {
                    FieldState::Clear => evidence::Reading::Clear,
                    FieldState::Password => evidence::Reading::Password,
                    FieldState::Unknown => evidence::Reading::Unknown,
                }),
                another_element: mark.moved == Some(seam::Moved::Another),
                began: mark.began,
                at: mark.at,
            })
            .collect();
        let open_latches = latches_before
            .strip_prefix("count=")
            .and_then(|rest| rest.split(' ').next())
            .and_then(|count| count.parse::<usize>().ok())
            .unwrap_or(usize::MAX);
        let verdict = evidence::after_start_state_met(&evidence::AfterStart {
            letters: &typed,
            marks: &proof_marks,
            gate_unknown_before: gate_before == Some(FieldState::Unknown),
            open_latches_before: open_latches,
            front_tick,
            handler_counts_unchanged: at_the_front == after_the_letters,
            page_moved_its_focus: moved.moved_at >= 0,
        });
        let listed: Vec<String> = letters
            .iter()
            .map(|key| {
                format!(
                    "input@{}:decided@{}:{:?}:{:?}",
                    key.os_tick, key.decided_tick, key.verdict, key.rule
                )
            })
            .collect();
        say(
            test,
            format!(
                "browser={name} first_typing foreground={front} page_front_tick={front_tick} way_to_the_front={} \
                 page_moved_at_ms={} gate_before_the_letters={gate_before:?} \
                 open_latches_before_the_first_letter={latches_before} letters_sent={sent} landed_text={} \
                 keystroke_events={sink} keys_withheld={withheld} handler_counts_after_the_front={at_the_front} \
                 handler_counts_after_the_letters={after_the_letters} letters=[{}] \
                 system_events_from_the_browser_start_to_the_last_letter=[{events}] marks=[{}]",
                browser.front_method,
                moved.moved_at,
                page.text,
                listed.join(","),
                full_marks(&marks)
            ),
        );
        let before_the_read = handler_counts();
        let uia = seam::focused_element_properties();
        std::thread::sleep(Duration::from_millis(1_000));
        say(
            test,
            format!(
                "browser={name} first_typing after_the_letters handler_counts_before_the_read={before_the_read} \
                 uia_of_the_focused_element {uia} handler_counts_1_s_after_the_read={}",
                handler_counts()
            ),
        );
        assert!(
            front && sent == 5 && page.text == 5,
            "INCONCLUSIVE: the page was not in front or did not take the 5 letters"
        );
        match verdict {
            Err(reason) => {
                say(
                    test,
                    format!("browser={name} after_start_state=not_met reason={reason}"),
                );
                assert_eq!(
                    sink, 5,
                    "a short first typing that does not meet the state after a start behind another window: {reason}"
                );
            }
            Ok(()) => {
                say(test, format!("browser={name} after_start_state=met"));
                assert_eq!(
                    (sink, withheld),
                    (0, true),
                    "the state after the start: a key was recorded, or keys_withheld did not read yes"
                );
                let again = browser
                    .open(test, "control", 0)
                    .expect("INCONCLUSIVE: no new page window");
                let reopened = wait_for_the_pages_own_move(again);
                let _ = seam::take();
                let sent = i64::from(type_letters_slowly(5) / 2);
                let sink = keyboard.keystrokes_for(Duration::from_millis(800));
                let keys = seam::take();
                let marks = seam::take_marks();
                let page = page_read(again).unwrap_or_default();
                say(
                    test,
                    format!(
                        "browser={name} reopened_page foreground={} page_moved_at_ms={} letters_sent={sent} \
                         landed_text={} keystroke_events={sink} letters_recorded_by_the_seam={} \
                         handler_counts={} marks=[{}]",
                        foreground_window() == again,
                        reopened.moved_at,
                        page.text,
                        decisions_of(&keys).letters,
                        handler_counts(),
                        full_marks(&marks)
                    ),
                );
                assert_eq!(
                    sink, 5,
                    "the reopened page read short: the source recorded nothing there"
                );
            }
        }
        let _ = keyboard.stop();
    });
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop and Chrome"]
fn after_a_start_behind_another_window_in_chrome() {
    after_a_start_behind_another_window("browser-after-start-chrome", "chrome");
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop and Edge"]
fn after_a_start_behind_another_window_in_edge() {
    after_a_start_behind_another_window("browser-after-start-edge", "edge");
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop and Chrome"]
fn stall_phases_of_the_page_in_chrome() {
    stall_phases("browser-stall-chrome", "chrome");
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop and Edge"]
fn stall_phases_of_the_page_in_edge() {
    stall_phases("browser-stall-edge", "edge");
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop and Chrome"]
fn swap_phases_of_the_page_in_chrome() {
    swap_phases("browser-swap-chrome", "chrome");
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop and Edge"]
fn swap_phases_of_the_page_in_edge() {
    swap_phases("browser-swap-edge", "edge");
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop and Chrome"]
fn a_burst_whose_first_letter_swaps_records_nothing_in_chrome() {
    let recorded = burst_phase("browser-burst-chrome", "chrome");
    assert_eq!(
        recorded, 0,
        "a letter of the burst that landed in a password field was recorded"
    );
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop and Edge"]
fn a_burst_whose_first_letter_swaps_records_nothing_in_edge() {
    let recorded = burst_phase("browser-burst-edge", "edge");
    assert_eq!(
        recorded, 0,
        "a letter of the burst that landed in a password field was recorded"
    );
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop and Chrome"]
fn a_letter_and_a_tab_out_of_a_swapped_field_record_nothing_in_chrome() {
    let recorded = letter_and_tab_out_of_a_swapped_field("browser-tab-after-chrome", "chrome");
    assert_eq!(
        recorded, 0,
        "a letter or a Tab that landed in a password field was recorded"
    );
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop and Edge"]
fn a_letter_and_a_tab_out_of_a_swapped_field_record_nothing_in_edge() {
    let recorded = letter_and_tab_out_of_a_swapped_field("browser-tab-after-edge", "edge");
    assert_eq!(
        recorded, 0,
        "a letter or a Tab that landed in a password field was recorded"
    );
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop and Chrome"]
fn cold_and_warm_orders_in_chrome() {
    cold_and_warm("browser-cold-chrome", "chrome");
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop and Edge"]
fn cold_and_warm_orders_in_edge() {
    cold_and_warm("browser-cold-edge", "edge");
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop and Firefox"]
fn a_renamed_copy_of_firefox_reads_unknown_and_records_nothing() {
    renamed_copy_phases("browser-renamed-firefox", &FIREFOX);
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop and Chrome"]
fn a_renamed_copy_of_chrome_reads_unknown_and_records_nothing() {
    renamed_copy_phases("browser-renamed-chrome", &CHROME);
}
