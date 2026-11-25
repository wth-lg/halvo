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
let back = false;
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
password.addEventListener('input', () => {
  if (back && phase === 'there-and-back') { back = false; plain.focus(); show(); }
});
plain.addEventListener('input', () => {
  if (armed && phase === 'take-one') { armed = false; setTimeout(() => { busy(300); password.focus(); freed(); show(); }, 0); }
});
plain.addEventListener('keydown', (e) => {
  if (phase === 'chat' && e.key === 'Enter') { e.preventDefault(); plain.value = ''; show(); }
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
  if (phase === 'cold' || phase === 'show-hide' || phase === 'show-control' || phase === 'other') password.focus();
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
  else if (phase === 'burst' || phase === 'control' || phase === 'chat') {
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
  if (phase === 'other') timers.push(setInterval(() => { plain.value = String(Date.now() % 100000); }, 30));
  if (phase === 'take-none') later(600, () => { busy(300); password.focus(); freed(); });
  if (phase === 'own-change' || phase === 'own-none') later(600, () => {
    busy(300); if (phase === 'own-change') plain.value = 'x'; busy(300); password.focus(); freed();
  });
  if (phase === 'there-and-back') { back = true; later(600, () => { busy(300); password.focus(); freed(); }); }
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

fn marks_with_kinds(marks: &[seam::MarkSeen]) -> String {
    let listed: Vec<String> = marks
        .iter()
        .map(|mark| {
            format!(
                "{}@{}:{:?}:{:?}:{}",
                mark.what,
                mark.at.unwrap_or(mark.began),
                mark.state,
                mark.moved,
                mark.element
                    .as_deref()
                    .and_then(|element| element.split(' ').find(|part| part.starts_with("kind=")))
                    .unwrap_or("kind=none")
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

pub(super) fn other_field_phases(test: &'static str, name: &'static str) {
    with_deadline(test, Duration::from_secs(240), move || {
        source_built_by_test(test, "keyboard");
        let mut browser = open_browser(test, name);
        let keyboard = Keyboard::start(test);
        assert!(
            keyboard.start.is_ok(),
            "the keyboard source must start: {:?}",
            keyboard.start
        );
        let rounds = super::stall::rounds_for(2, 5);
        let mut out = Vec::new();
        if let Some(hwnd) = browser.open(test, "other", 0) {
            for round in 0..rounds {
                let start = reset(&keyboard, hwnd).unwrap_or_default();
                let front = foreground_window() == hwnd;
                let sent = letters_for(Duration::from_millis(900), Duration::from_millis(60));
                let mut r = end_round(&keyboard, hwnd, sent, front);
                r.why.extend(not_in_front(front));
                r.counted = front
                    && start.focus == "password"
                    && r.page.password == sent
                    && r.page.text == 0;
                say(
                    test,
                    format!(
                        "browser={} phase=other round={round} foreground={front} {}",
                        browser.name,
                        r.line()
                    ),
                );
                out.push(r);
            }
        }
        assert_stall(test, "other", &out, usize::try_from(rounds).unwrap_or(1));
    });
}

pub(super) fn there_and_back_phases(test: &'static str, name: &'static str) {
    with_deadline(test, Duration::from_secs(240), move || {
        source_built_by_test(test, "keyboard");
        let mut browser = open_browser(test, name);
        let keyboard = Keyboard::start(test);
        assert!(
            keyboard.start.is_ok(),
            "the keyboard source must start: {:?}",
            keyboard.start
        );
        let hwnd = browser
            .open(test, "there-and-back", 0)
            .expect("INCONCLUSIVE: no page window");
        let rounds = super::stall::rounds_for(3, 5);
        let mut counted = 0;
        let mut recorded = 0;
        for round in 0..rounds {
            let start = reset(&keyboard, hwnd).unwrap_or_default();
            let front = foreground_window() == hwnd;
            std::thread::sleep(Duration::from_millis(250));
            let _ = seam::take_marks();
            send(&[
                key_input(VK_A, false),
                key_input(VK_A, true),
                key_input(VK_A, false),
                key_input(VK_A, true),
            ]);
            let mut r = end_round(&keyboard, hwnd, 2, front);
            let marks = seam::take_marks();
            r.why.extend(not_in_front(front));
            r.counted = front
                && start.focus == "plain"
                && r.page.password == 1
                && r.page.text == 1
                && r.page.landings == "pt";
            say(
                test,
                format!(
                    "browser={name} phase=there-and-back round={round} foreground={front} {} marks={}",
                    r.line(),
                    marks_with_kinds(&marks)
                ),
            );
            counted += usize::from(r.counted);
            if r.counted {
                recorded += r.password_letters_recorded();
            }
        }
        say(
            test,
            format!(
                "phase=there-and-back counted={counted} counted_rounds={counted} password_letters_recorded={recorded}"
            ),
        );
        assert_eq!(
            recorded, 0,
            "a letter that the page took at its password input between two moves was recorded"
        );
        assert!(
            counted >= usize::try_from(rounds).unwrap_or(1),
            "INCONCLUSIVE: the phase counted {counted} rounds of {rounds}"
        );
    });
}

/// Under the ruling's (L2): a chat's box that keeps the focus behind Enter.
/// Two letters, an Enter on which the page empties the box, and two letters
/// more, 400 ms between the keys: every letter is noted.
pub(super) fn chat_box_phases(test: &'static str, name: &'static str) {
    use windows::Win32::UI::Input::KeyboardAndMouse::VK_RETURN;
    with_deadline(test, Duration::from_secs(240), move || {
        source_built_by_test(test, "keyboard");
        let mut browser = open_browser(test, name);
        let keyboard = Keyboard::start(test);
        assert!(
            keyboard.start.is_ok(),
            "the keyboard source must start: {:?}",
            keyboard.start
        );
        let hwnd = browser
            .open(test, "chat", 0)
            .expect("INCONCLUSIVE: no page window");
        let rounds = super::stall::rounds_for(1, 3);
        let mut counted = 0;
        let mut noted = 0;
        for round in 0..rounds {
            let start = reset(&keyboard, hwnd).unwrap_or_default();
            let front = foreground_window() == hwnd;
            std::thread::sleep(Duration::from_millis(600));
            for vk in [VK_A, VK_A, VK_RETURN, VK_A, VK_A] {
                send(&[key_input(vk, false), key_input(vk, true)]);
                std::thread::sleep(Duration::from_millis(400));
            }
            let mut r = end_round(&keyboard, hwnd, 4, front);
            r.why.extend(not_in_front(front));
            r.counted = front
                && start.focus == "plain"
                && r.page.text == 4
                && r.page.password == 0
                && r.keys.letters_decided == 4;
            let withheld: Vec<usize> = r
                .keys
                .letters_in_order
                .iter()
                .enumerate()
                .filter(|(_, recorded)| !**recorded)
                .map(|(at, _)| at + 1)
                .collect();
            say(
                test,
                format!(
                    "browser={name} phase=chat round={round} foreground={front} withheld_letters={withheld:?} {}",
                    r.line()
                ),
            );
            counted += usize::from(r.counted);
            if r.counted {
                noted += r.keys.letters;
            }
        }
        say(
            test,
            format!("phase=chat counted={counted} counted_rounds={counted} letters_noted={noted}"),
        );
        assert!(
            counted >= usize::try_from(rounds).unwrap_or(1),
            "INCONCLUSIVE: the chat phase counted {counted} rounds of {rounds}"
        );
        assert!(
            noted == 4 * counted,
            "{noted} of {} letters noted in a chat's box behind Enter",
            4 * counted
        );
    });
}

pub(super) fn earlier_key_phases(test: &'static str, name: &'static str, earlier_key: bool) -> i64 {
    let phase = if earlier_key { "take-one" } else { "take-none" };
    held_page_rounds(test, name, phase, earlier_key)
}

/// The page row of order 10 and its twin: the page holds its main thread
/// for 300 ms while one letter is typed into its plain input, sets that
/// input's value itself (the twin does not), holds 300 ms more, and moves the
/// focus to its password input, where the letter lands. All of it is one
/// task of the page.
pub(super) fn own_change_phases(test: &'static str, name: &'static str, change: bool) -> i64 {
    let phase = if change { "own-change" } else { "own-none" };
    held_page_rounds(test, name, phase, false)
}

fn held_page_rounds(
    test: &'static str,
    name: &'static str,
    phase: &'static str,
    earlier_key: bool,
) -> i64 {
    let total = Arc::new(AtomicI64::new(0));
    let out = Arc::clone(&total);
    with_deadline(test, Duration::from_secs(240), move || {
        source_built_by_test(test, "keyboard");
        let mut browser = open_browser(test, name);
        let keyboard = Keyboard::start(test);
        assert!(
            keyboard.start.is_ok(),
            "the keyboard source must start: {:?}",
            keyboard.start
        );
        let hwnd = browser
            .open(test, phase, 0)
            .expect("INCONCLUSIVE: no page window");
        let rounds = super::stall::rounds_for(4, 8);
        let mut counted = 0;
        let mut recorded = 0;
        let mut records = 0;
        let mut beyond = 0;
        for round in 0..rounds {
            let start = reset(&keyboard, hwnd).unwrap_or_default();
            let front = foreground_window() == hwnd;
            let gap_ms = [0, 4, 8, 12][round as usize % 4];
            let sent = if earlier_key {
                send(&[key_input(VK_A, false), key_input(VK_A, true)]);
                std::thread::sleep(Duration::from_millis(gap_ms));
                send(&[key_input(VK_A, false), key_input(VK_A, true)]);
                2
            } else {
                std::thread::sleep(Duration::from_millis(250));
                key(VK_A);
                1
            };
            let seen = keyboard.events.drain_for(Duration::from_millis(900));
            let keystrokes: Vec<_> = seen
                .iter()
                .filter(|e| e.modality == Modality::Keystroke)
                .collect();
            let round_beyond = keystrokes
                .iter()
                .filter(|e| {
                    e.foreground_app.is_empty()
                        || !e.view.as_ref().is_some_and(|view| {
                            view.key_class.is_some()
                                && view.click.is_none()
                                && view.fold.is_none()
                                && view.title.is_none()
                        })
                })
                .count();
            let mut r = Round {
                counted: front,
                sent,
                page: page_read(hwnd).unwrap_or_default(),
                keys: decisions(),
                sink: keystrokes.len(),
                why: not_in_front(front).into_iter().collect(),
            };
            r.counted = front
                && start.focus == "plain"
                && r.page.password == 1
                && r.page.text == i64::from(earlier_key);
            say(
                test,
                format!(
                    "browser={name} phase={phase} round={round} gap_ms={gap_ms} foreground={front} \
                     records_beyond_time_app_and_class={round_beyond} {}",
                    r.line()
                ),
            );
            counted += usize::from(r.counted);
            records += keystrokes.len();
            beyond += round_beyond;
            if r.counted {
                recorded += r.password_letters_recorded();
            }
        }
        say(
            test,
            format!(
                "phase={phase} counted={counted} counted_rounds={counted} password_letters_recorded={recorded} \
                 keystroke_records={records} records_beyond_time_app_and_class={beyond}"
            ),
        );
        assert_eq!(
            beyond, 0,
            "a keystroke record held more than its time, its app and its class"
        );
        let wanted = if earlier_key {
            1
        } else {
            usize::try_from(rounds).unwrap_or(1)
        };
        assert!(
            counted >= wanted,
            "INCONCLUSIVE: {phase} counted {counted} rounds of {rounds}"
        );
        out.store(recorded, Ordering::SeqCst);
    });
    total.load(Ordering::SeqCst)
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

    /// What the verdict reads of a round of the burst, or `None` for a round that was not counted.
    pub(super) fn counted_of(round: &super::Round) -> Option<Counted> {
        round.counted.then_some(Counted {
            first_letter_rule: round.keys.first_letter_rule,
            records: Records {
                letters: round.keys.letters,
                tabs: round.keys.tabs,
                other: round.keys.other,
                keystroke_events: round.sink,
            },
            freed_at_ms: round.page.freed_at,
        })
    }

    /// The rounds of one browser's burst, in the short and the long form alike.
    pub(super) const ROUNDS: u32 = 5;

    const TIME_RULE_ROUNDS_TAKEN: usize = 1;

    /// The rounds of one browser's burst: the freed_at_ms of each counted round
    /// that the time rule decided, and each counted round that failed.
    #[derive(Debug, Default)]
    pub(super) struct Burst {
        freed_at_ms: Vec<i64>,
        failures: Vec<String>,
    }

    impl Burst {
        /// Takes one round of the burst; `None` stands for a round that was not counted.
        pub(super) fn take(&mut self, round: u32, counted: Option<&Counted>) {
            let Some(counted) = counted else {
                return;
            };
            match decided(counted) {
                Ok(Decided::ByTheTimeRule { freed_at_ms }) => self.freed_at_ms.push(freed_at_ms),
                Ok(Decided::ByARuleOfTheSwap) => {}
                Err(why) => self.failures.push(format!("round {round}: {why}")),
            }
        }

        /// The count of the rounds that the time rule decided and their freed_at_ms values.
        pub(super) fn fields(&self) -> String {
            let freed = if self.freed_at_ms.is_empty() {
                "-".to_string()
            } else {
                self.freed_at_ms
                    .iter()
                    .map(i64::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            };
            format!(
                "rounds_decided_by_the_time_rule={} freed_at_ms_of_those_rounds={freed}",
                self.freed_at_ms.len()
            )
        }

        /// Fails on a counted round that failed, and in both forms on more
        /// than one round that the time rule decided.
        pub(super) fn verdict(&self) -> Result<(), String> {
            if !self.failures.is_empty() {
                return Err(self.failures.join("; "));
            }
            if self.freed_at_ms.len() > TIME_RULE_ROUNDS_TAKEN {
                return Err(format!(
                    "the burst takes at most {TIME_RULE_ROUNDS_TAKEN} round of its {ROUNDS} \
                     that the time rule decided: {}",
                    self.fields()
                ));
            }
            Ok(())
        }
    }

    fn counted(first_letter_rule: Option<Rule>, records: Records) -> Counted {
        Counted {
            first_letter_rule,
            records,
            freed_at_ms: 1564,
        }
    }

    fn every_first_letter_rule() -> Vec<Option<Rule>> {
        let mut rules = vec![None];
        let mut rule = Rule::PasswordEdit;
        loop {
            rules.push(Some(rule));
            rule = match rule {
                Rule::PasswordEdit => Rule::NotClearedInTime,
                Rule::NotClearedInTime => Rule::AnotherThreadInFront,
                Rule::AnotherThreadInFront => Rule::StateAtTheKey,
                Rule::StateAtTheKey => Rule::MarkAfterTheKey,
                Rule::MarkAfterTheKey => Rule::ReadAfterTheHold,
                Rule::ReadAfterTheHold => Rule::NoClickRoute,
                Rule::NoClickRoute => Rule::ElementNotTheSame,
                Rule::ElementNotTheSame => Rule::MarksLost,
                Rule::MarksLost => Rule::ClicksNotForwarded,
                Rule::ClicksNotForwarded => Rule::LandingAfterTheLatch,
                Rule::LandingAfterTheLatch => break,
            };
        }
        rules
    }

    fn fails_whatever_the_rule(records: Records) {
        let rules = every_first_letter_rule();
        let distinct = rules
            .iter()
            .enumerate()
            .all(|(at, rule)| !rules[..at].contains(rule));
        assert!(
            rules.len() == 12 && distinct,
            "the record cases read {} rules of the first letter and not 12",
            rules.len()
        );
        for rule in rules {
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

    fn time_rule(freed_at_ms: i64) -> Counted {
        Counted {
            first_letter_rule: Some(Rule::NotClearedInTime),
            records: Records::default(),
            freed_at_ms,
        }
    }

    fn swap_rule() -> Counted {
        counted(Some(Rule::MarkAfterTheKey), Records::default())
    }

    fn burst_of(rounds: &[Option<Counted>]) -> Burst {
        let mut burst = Burst::default();
        for (round, counted) in (0..).zip(rounds) {
            burst.take(round, counted.as_ref());
        }
        burst
    }

    #[test]
    #[ignore = "a made-up round of the burst: 52-windows-browser runs it by its exact name"]
    fn the_burst_prints_the_count_of_its_counted_rounds_that_the_time_rule_decided_and_their_freed_at_ms(
    ) {
        let burst = burst_of(&[
            Some(time_rule(1564)),
            Some(swap_rule()),
            None,
            Some(time_rule(1601)),
            Some(swap_rule()),
        ]);
        assert_eq!(
            burst.fields(),
            "rounds_decided_by_the_time_rule=2 freed_at_ms_of_those_rounds=1564,1601"
        );
    }

    #[test]
    #[ignore = "a made-up round of the burst: 52-windows-browser runs it by its exact name"]
    fn a_burst_with_no_round_that_the_time_rule_decided_prints_a_count_of_zero() {
        let burst = burst_of(&[Some(swap_rule()), None, Some(swap_rule())]);
        assert_eq!(
            burst.fields(),
            "rounds_decided_by_the_time_rule=0 freed_at_ms_of_those_rounds=-"
        );
    }

    #[test]
    #[ignore = "a made-up round of the burst: 52-windows-browser runs it by its exact name"]
    fn both_forms_take_one_round_of_five_that_the_time_rule_decided_and_print_its_count() {
        let burst = burst_of(&[
            Some(swap_rule()),
            Some(swap_rule()),
            Some(time_rule(1564)),
            Some(swap_rule()),
            Some(swap_rule()),
        ]);
        assert_eq!(
            burst.fields(),
            "rounds_decided_by_the_time_rule=1 freed_at_ms_of_those_rounds=1564"
        );
        assert_eq!(burst.verdict(), Ok(()));
    }

    #[test]
    #[ignore = "a made-up round of the burst: 52-windows-browser runs it by its exact name"]
    fn both_forms_fail_two_rounds_of_five_that_the_time_rule_decided_with_their_freed_at_ms() {
        let burst = burst_of(&[
            Some(swap_rule()),
            Some(time_rule(1564)),
            Some(swap_rule()),
            Some(time_rule(1601)),
            Some(swap_rule()),
        ]);
        let verdict = burst.verdict();
        assert!(
            verdict.as_ref().is_err_and(|why| why.contains(
                "rounds_decided_by_the_time_rule=2 freed_at_ms_of_those_rounds=1564,1601"
            )),
            "{verdict:?}"
        );
    }

    #[test]
    #[ignore = "a made-up round of the burst: 52-windows-browser runs it by its exact name"]
    fn both_forms_fail_five_rounds_of_five_that_the_time_rule_decided() {
        let burst = burst_of(&[
            Some(time_rule(1501)),
            Some(time_rule(1502)),
            Some(time_rule(1503)),
            Some(time_rule(1504)),
            Some(time_rule(1505)),
        ]);
        let verdict = burst.verdict();
        assert!(
            verdict.as_ref().is_err_and(|why| why.contains(
                "rounds_decided_by_the_time_rule=5 freed_at_ms_of_those_rounds=1501,1502,1503,1504,1505"
            )),
            "{verdict:?}"
        );
    }

    #[test]
    #[ignore = "a made-up round of the burst: 52-windows-browser runs it by its exact name"]
    fn a_counted_round_reaches_the_tally_with_its_four_counts_and_its_freed_at_ms() {
        let recorded = super::Round {
            counted: true,
            sink: 4,
            keys: super::Decisions {
                letters: 1,
                tabs: 2,
                other: 3,
                first_letter_rule: Some(Rule::NotClearedInTime),
                ..super::Decisions::default()
            },
            page: super::PageRead {
                freed_at: 1564,
                ..super::PageRead::default()
            },
            ..super::Round::default()
        };
        assert_eq!(
            counted_of(&recorded),
            Some(Counted {
                first_letter_rule: Some(Rule::NotClearedInTime),
                records: Records {
                    letters: 1,
                    tabs: 2,
                    other: 3,
                    keystroke_events: 4,
                },
                freed_at_ms: 1564,
            })
        );
        let not_counted = super::Round {
            counted: false,
            ..recorded.clone()
        };
        assert_eq!(counted_of(&not_counted), None);
        let quiet = super::Round {
            sink: 0,
            keys: super::Decisions {
                first_letter_rule: Some(Rule::NotClearedInTime),
                ..super::Decisions::default()
            },
            ..recorded.clone()
        };
        let mut burst = Burst::default();
        for (round, r) in (0..).zip([&quiet, &not_counted, &quiet]) {
            burst.take(round, counted_of(r).as_ref());
        }
        assert_eq!(
            burst.fields(),
            "rounds_decided_by_the_time_rule=2 freed_at_ms_of_those_rounds=1564,1564"
        );
        let mut burst = Burst::default();
        burst.take(3, counted_of(&recorded).as_ref());
        let verdict = burst.verdict();
        assert!(
            verdict.as_ref().is_err_and(|why| why.starts_with(
                "round 3: the burst recorded letters_recorded=1 tabs_recorded=2 other_recorded=3 keystroke_events=4 "
            )),
            "{verdict:?}"
        );
    }

    #[test]
    #[ignore = "a made-up round of the burst: 52-windows-browser runs it by its exact name"]
    fn the_burst_of_a_browser_runs_five_rounds_in_both_forms() {
        assert_eq!(ROUNDS, 5);
        let mut burst = Burst::default();
        for round in 0..ROUNDS {
            burst.take(round, Some(&time_rule(1500 + i64::from(round))));
        }
        assert!(burst.verdict().is_err_and(|why| why.contains(
            "at most 1 round of its 5 that the time rule decided: rounds_decided_by_the_time_rule=5 "
        )));
    }

    #[test]
    #[ignore = "a made-up round of the burst: 52-windows-browser runs it by its exact name"]
    fn a_counted_round_that_failed_fails_the_burst_in_both_forms() {
        let recorded = counted(
            Some(Rule::NotClearedInTime),
            Records {
                letters: 1,
                ..Records::default()
            },
        );
        let burst = burst_of(&[
            Some(swap_rule()),
            None,
            Some(swap_rule()),
            Some(recorded),
            Some(swap_rule()),
        ]);
        let verdict = burst.verdict();
        assert!(
            verdict
                .as_ref()
                .is_err_and(|why| why.starts_with("round 3: ")),
            "{verdict:?}"
        );
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
        let mut tally = burst_rounds::Burst::default();
        for round in 0..burst_rounds::ROUNDS {
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
                    .and_then(|element| element.split(' ').find(|part| part.starts_with("kind=")))
                    .unwrap_or("kind=none")
                    .to_string();
                say(
                    test,
                    format!(
                        "browser={name} phase=burst round={round} gate_before_the_step={gate_before_the_step:?} \
                         gate_before_the_burst={gate_before:?} element={element} control_letters_sent={control_sent} \
                         control_keystroke_events={control} marks=[{}]",
                        marks_with_kinds(&control_marks)
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
            tally.take(round, burst_rounds::counted_of(&r).as_ref());
        }
        let _ = keyboard.stop();
        say(
            test,
            format!(
                "phase=burst counted={counted} password_letters_recorded={recorded} \
                 rounds_whose_step_found_a_closed_gate={closed_before_the_step} \
                 rounds_that_waited_for_the_page_type={waited_for_the_type} \
                 rounds_where_the_sink_differs_from_the_seam={sink_differs} {}",
                tally.fields()
            ),
        );
        if let Err(why) = tally.verdict() {
            panic!("{why}");
        }
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

/// The letters that each part of the row of a switch back sends: one word.
const SWITCH_BACK_LETTERS: usize = 5;

/// How long each wait of the row of a switch back waits for its state.
const SWITCH_BACK_WAIT: Duration = Duration::from_secs(15);

/// How long each wait of the row's warm-up waits for its state: the
/// browser's first read of its page, and its first switch away and back.
const WARM_UP_WAIT: Duration = Duration::from_secs(30);

/// How far behind a switch the own read that it asks begins at the least:
/// one settle time.
const SWITCH_READ_BEHIND_MS: i32 = 100;

/// What the row of a switch back took from the test seam and from its sink,
/// in their order.
#[derive(Default)]
struct SwitchBackSeen {
    marks: Vec<seam::MarkSeen>,
    decided: Vec<seam::Decided>,
    records: Vec<Seen>,
    noted: usize,
}

impl SwitchBackSeen {
    /// Takes what came since the last take, with one step of 20 ms on the sink.
    fn take(&mut self, events: &Events) {
        self.records
            .extend(events.drain_for(Duration::from_millis(20)));
        self.marks.extend(seam::take_marks());
        self.decided.extend(seam::take());
        while seam::next_recorded_tick().is_some() {
            self.noted += 1;
        }
    }

    /// Takes until `done` holds or `limit` ends: whether it held.
    fn until(&mut self, events: &Events, limit: Duration, done: impl Fn(&Self) -> bool) -> bool {
        let began = Instant::now();
        loop {
            self.take(events);
            if done(self) {
                return true;
            }
            if began.elapsed() >= limit {
                return false;
            }
        }
    }

    /// The newest switch of the marks from `from` whose window in front was
    /// of the process `pid`, and the first own read that began one settle
    /// time or more behind it: the read that the switch asked.
    fn switch_and_its_read(
        &self,
        from: usize,
        pid: u32,
    ) -> (Option<&seam::MarkSeen>, Option<&seam::MarkSeen>) {
        let marks = &self.marks[from.min(self.marks.len())..];
        let Some(at) = marks
            .iter()
            .rposition(|mark| mark.what == "switch" && mark.front_pid == pid)
        else {
            return (None, None);
        };
        let switch = &marks[at];
        let tick = switch.at.unwrap_or(switch.began);
        let read = marks[at..].iter().find(|mark| {
            mark.what == "read" && mark.began.wrapping_sub(tick) as i32 >= SWITCH_READ_BEHIND_MS
        });
        (Some(switch), read)
    }

    /// Whether the marks from `from` hold an event or an own read of `state`,
    /// of an element of Chrome where the state is Clear: the gate's read of
    /// a page's field.
    fn field_read(&self, from: usize, state: FieldState) -> bool {
        self.marks[from.min(self.marks.len())..].iter().any(|mark| {
            (mark.what == "event" || mark.what == "read")
                && mark.state == Some(state)
                && (state != FieldState::Clear || part_of(mark, "kind=", "") == "kind=Chrome")
        })
    }

    /// The letters decided from the decision `from` on.
    fn letters_from(&self, from: usize) -> Vec<seam::Decided> {
        self.decided[from.min(self.decided.len())..]
            .iter()
            .filter(|key| key.class == seam::KeyClass::Printable)
            .copied()
            .collect()
    }

    /// The keystroke records of the sink from the record `from` on.
    fn keystrokes_from(&self, from: usize) -> usize {
        count(
            &self.records[from.min(self.records.len())..],
            Modality::Keystroke,
        )
    }
}

/// One switch of the row of a switch back, by the window alone: the OS tick
/// of the call, which process made it (the test, or the other window), whether
/// the window came in front, and the ticks of the switch that the gate holds of
/// it and of the own read that the switch asked, as the row waited for them: a
/// switch told late shows a title thread that was held.
#[derive(Debug, Clone, Copy)]
struct Switched {
    call_at: u32,
    by: &'static str,
    pointer_over: &'static str,
    front: bool,
    at: Option<u32>,
    read_at: Option<u32>,
    told: bool,
}

impl Switched {
    /// How long after its call the title source told the gate of the switch;
    /// -1 where it did not.
    fn late_ms(&self) -> i64 {
        self.at
            .map_or(-1, |at| i64::from(at.wrapping_sub(self.call_at) as i32))
    }

    /// Whether the window came in front by its one call of
    /// SetForegroundWindow within FRONT_WAIT.
    fn came(&self) -> bool {
        self.front
    }

    /// How long after its call the read that the switch asked began; -1
    /// where none did.
    fn read_late_ms(&self) -> i64 {
        self.read_at
            .map_or(-1, |at| i64::from(at.wrapping_sub(self.call_at) as i32))
    }

    fn line(&self, which: &str) -> String {
        format!(
            "{which}_call_at={} {which}_by={} {which}_pointer_over={} {which}_front={} {which}_told={} \
             {which}_switch_at={} {which}_switch_late_ms={} {which}_read_began_at={} \
             {which}_read_late_ms={}",
            self.call_at,
            self.by,
            self.pointer_over,
            self.front,
            self.told,
            self.at.map_or(-1, i64::from),
            self.late_ms(),
            self.read_at.map_or(-1, i64::from),
            self.read_late_ms()
        )
    }
}

/// How long the row of a switch back waits for a window in front after its
/// one call of SetForegroundWindow.
const FRONT_WAIT: Duration = Duration::from_secs(5);

/// How long each try of the warm-up's switch waits for the gate to hold it.
const WARM_UP_TRY_WAIT: Duration = Duration::from_secs(5);

/// Waits up to FRONT_WAIT until `to` is in front, then up to `limit` until
/// the gate holds the switch into it since the mark `from` and the own read
/// that the switch asked.
fn switch_told(
    (to, from, call_at, by, pointer_over): (HWND, usize, u32, &'static str, &'static str),
    seen: &mut SwitchBackSeen,
    events: &Events,
    limit: Duration,
) -> Switched {
    let pid = window_pid(to);
    let front = seen.until(events, FRONT_WAIT, |_| foreground_window() == to);
    let told = seen.until(events, limit, |seen| {
        seen.switch_and_its_read(from, pid).1.is_some()
    });
    let (switch, read) = seen.switch_and_its_read(from, pid);
    Switched {
        call_at,
        by,
        pointer_over,
        front,
        at: switch.map(|mark| mark.at.unwrap_or(mark.began)),
        read_at: read.map(|mark| mark.began),
        told,
    }
}

/// Which window the pointer stands over: the page that the switch leaves,
/// the window with no edit, another window of the page's process, another
/// window, or none.
fn pointer_over(page: HWND, away: HWND) -> &'static str {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetAncestor, GetCursorPos, WindowFromPoint, GA_ROOT,
    };
    let mut at = POINT::default();
    // SAFETY: `at` is a live POINT that the call writes into.
    if unsafe { GetCursorPos(&mut at) }.is_err() {
        return "none";
    }
    // SAFETY: WindowFromPoint and GetAncestor take plain values.
    let root = unsafe { GetAncestor(WindowFromPoint(at), GA_ROOT) };
    if root.0.is_null() {
        "none"
    } else if root == page {
        "page"
    } else if root == away {
        "away_window"
    } else if window_pid(root) == window_pid(page) {
        "another_window_of_the_page"
    } else {
        "another_window"
    }
}

/// The test's switch AWAY from the page `left` into `to`: ONE pointer move of
/// the test by SendInput, one pixel and back (no click, no key), so that the
/// test is the last input's provider, which the system lets set the
/// foreground; then ONE call of SetForegroundWindow, and never the joined
/// input of bring_to_front's way 3, since a switch made through the test's
/// own input is raised in the test's process, whose events the title
/// source's hooks skip as their own. Answers the switch and the count of
/// marks before the move.
fn switch_by_the_test(
    (to, left): (HWND, HWND),
    seen: &mut SwitchBackSeen,
    events: &Events,
    limit: Duration,
) -> (Switched, usize) {
    seen.take(events);
    let from = seen.marks.len();
    let over = pointer_over(left, to);
    send(&[
        mouse_input(1, 0, MOUSEEVENTF_MOVE),
        mouse_input(-1, 0, MOUSEEVENTF_MOVE),
    ]);
    let call_at = tick_now();
    set_foreground(to);
    (
        switch_told((to, from, call_at, "test", over), seen, events, limit),
        from,
    )
}

/// The title of the window of the child mode front-on-go.
const AWAY_TITLE: &str = "halvo-verify-front-on-go";

/// The window with no edit of the child mode front-on-go, in a process of its
/// own, with the count of the goes that the test gave it.
struct AwayWindow {
    child: ChildProcess,
    hwnd: HWND,
    goes: u32,
}

/// The switch back into `to` made by the window with no edit itself, which
/// holds the front then: another process than the test raises its event, as a
/// program's or a person's switch does, and the test gives no input for it
/// (its go is a file). Answers the switch and the count of marks before the go.
fn switch_by_the_other_window(
    away: &mut AwayWindow,
    to: HWND,
    seen: &mut SwitchBackSeen,
    events: &Events,
    limit: Duration,
) -> (Switched, usize) {
    seen.take(events);
    let from = seen.marks.len();
    away.goes += 1;
    let go = format!("front-{}", away.goes);
    std::fs::write(
        away.child._dir.path().join(format!("{go}.window")),
        (to.0 as usize).to_string(),
    )
    .expect("write the window of the child's go");
    let call_at = tick_now();
    away.child.go(&go);
    (
        switch_told(
            (to, from, call_at, "other_window", "no_move"),
            seen,
            events,
            limit,
        ),
        from,
    )
}

/// The child mode front-on-go: a window with no edit that brings another
/// window in front on the test's word. For each step N the test writes the
/// file front-N.window with that window's handle in decimal, then the go
/// front-N; the child, which holds the front then, calls SetForegroundWindow
/// once for it and says whether the call answered yes. It answers its
/// messages throughout and ends after `secs` or when the test ends it.
pub(super) fn child_front_on_go(secs: u64) {
    let _window = TestWindow::create(&normal_window("HalvoVerifyFrontOnGo", AWAY_TITLE, 420, 320))
        .expect("the window");
    let program = std::env::current_exe().expect("the child's program");
    let deadline = Instant::now() + Duration::from_secs(secs);
    let mut step = 0;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let go = format!("front-{}", step + 1);
        if left.is_zero() || !wait_for_go(&go, left) {
            break;
        }
        step += 1;
        let handle = std::fs::read_to_string(program.with_file_name(format!("{go}.window")))
            .ok()
            .and_then(|text| text.trim().parse::<usize>().ok())
            .unwrap_or(0);
        // SAFETY: SetForegroundWindow takes a window handle by value; a stale
        // handle makes the call fail and changes nothing.
        let answered =
            unsafe { SetForegroundWindow(HWND(handle as *mut core::ffi::c_void)) }.as_bool();
        say(
            "child",
            format!("mode=front-on-go step={step} set_foreground_answered={answered}"),
        );
    }
    say("child", format!("mode=front-on-go goes={step}"));
}

/// Opens a page window of `phase` as PhaseBrowser::open does, but brings it
/// in front by ONE call of SetForegroundWindow, the front waited for as a
/// state up to FRONT_WAIT: no click and no joined input. Chromium alone.
fn open_by_the_window_alone(
    browser: &mut PhaseBrowser,
    (test, phase): (&str, &str),
    seen: &mut SwitchBackSeen,
    events: &Events,
) -> Option<(HWND, bool)> {
    browser.runs += 1;
    let url = format!("{}?phase={phase}&hold=0&run={}", browser.page, browser.runs);
    let profile = browser.profile.path().to_string_lossy().into_owned();
    Command::new(&browser.exe)
        .args([
            format!("--user-data-dir={profile}"),
            "--no-first-run".to_string(),
            "--no-default-browser-check".to_string(),
            "--disable-features=Translate".to_string(),
            "--new-window".to_string(),
            url,
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let prefix = format!("halvo-phase {phase}{} ", browser.runs);
    let found = seen.until(events, Duration::from_secs(40), |_| {
        window_titled(&prefix).is_some()
    });
    let (hwnd, _) = window_titled(&prefix)?;
    set_foreground(hwnd);
    let front = seen.until(events, FRONT_WAIT, |_| foreground_window() == hwnd);
    say(
        test,
        format!(
            "browser={} phase={phase} opened={found} front_by_one_call={front}",
            browser.name
        ),
    );
    Some((hwnd, front))
}

/// One part of the row of a switch back: its switch away and back, what the
/// gate and the system gave between the switch back and the first letter,
/// and its letters.
struct SwitchBackPart {
    part: &'static str,
    away: Switched,
    back: Switched,
    keys_decided_in_the_part: usize,
    page_counts_seen: bool,
    read_behind_the_switch: Option<(Option<FieldState>, i64, u32)>,
    focus_events_from_the_switch_back: usize,
    focus_events_from_the_move: Vec<String>,
    latches_from_the_move: usize,
    reads_from_the_move: Vec<String>,
    handler_entered_from_the_switch_to_the_letters: u64,
    system_events_from_the_switch_to_the_letters: String,
    gate_at_the_return: ContentReading,
    newest_before_the_first_letter: String,
    newest_is_the_read_behind_the_switch: bool,
    page_at_the_return: PageRead,
    page_after_the_letters: PageRead,
    letters_sent: u32,
    letters: Vec<seam::Decided>,
    keystroke_records: usize,
    records_noted_by_the_seam: usize,
    marks: String,
}

impl SwitchBackPart {
    fn letters_recorded(&self) -> usize {
        self.letters
            .iter()
            .filter(|key| key.verdict == seam::Verdict::Recorded)
            .count()
    }

    /// Whether the part's switches came by the window alone, the gate saw no
    /// latch (no click, no focus key) between the switch back and the first
    /// letter, and the page showed its count of the letters.
    fn came_in_order(&self) -> bool {
        self.away.came()
            && self.back.came()
            && self.latches_from_the_move == 0
            && self.page_counts_seen
    }

    /// Whether the title source told the gate of each switch of the part and
    /// the gate made the own read that the switch asked.
    fn told_in_order(&self) -> bool {
        self.away.told && self.back.told
    }

    fn line(&self) -> String {
        let (read_state, read_ms) = self
            .read_behind_the_switch
            .map_or((None, -1), |(state, ms, _)| (state, ms));
        let letters: Vec<String> = self
            .letters
            .iter()
            .map(|key| {
                format!(
                    "input@{}:decided@{}:{:?}:{:?}",
                    key.os_tick, key.decided_tick, key.verdict, key.rule
                )
            })
            .collect();
        format!(
            "part={} {} {} keys_decided_in_the_part={} page_counts_seen={} read_behind_the_switch={} \
             read_behind_the_switch_state={read_state:?} read_behind_the_switch_ms={read_ms} \
             focus_events_from_the_switch_back={} latches_from_the_move={} \
             focus_events_from_the_move=[{}] reads_from_the_move=[{}] \
             handler_entered_from_the_switch_to_the_letters={} \
             system_events_from_the_switch_to_the_letters=[{}] gate_at_the_return={:?} \
             newest_before_the_first_letter={} newest_is_the_read_behind_the_switch={} \
             page_focus_at_the_return={} letters_sent={} letters_decided={} letters_recorded={} \
             first_letter_rule={:?} letters=[{}] keystroke_records={} records_noted_by_the_seam={} \
             landed_text={} landed_password={}",
            self.part,
            self.away.line("away"),
            self.back.line("back"),
            self.keys_decided_in_the_part,
            self.page_counts_seen,
            self.read_behind_the_switch.is_some(),
            self.focus_events_from_the_switch_back,
            self.latches_from_the_move,
            self.focus_events_from_the_move.join(","),
            self.reads_from_the_move.join(","),
            self.handler_entered_from_the_switch_to_the_letters,
            self.system_events_from_the_switch_to_the_letters,
            self.gate_at_the_return,
            self.newest_before_the_first_letter,
            self.newest_is_the_read_behind_the_switch,
            self.page_at_the_return.focus,
            self.letters_sent,
            self.letters.len(),
            self.letters_recorded(),
            self.letters.first().and_then(|key| key.rule),
            letters.join(","),
            self.keystroke_records,
            self.records_noted_by_the_seam,
            self.page_after_the_letters.text,
            self.page_after_the_letters.password,
        )
    }
}

/// The OS time now, in ticks of GetTickCount, as the seam's marks carry it.
fn tick_now() -> u32 {
    // SAFETY: GetTickCount takes no argument and reads a counter.
    unsafe { GetTickCount() }
}

/// Leaves `page` for the window with no edit by the test's one call, and the
/// window with no edit brings the page back itself: no click, no key and no
/// input of the test between them, each switch waited for until the gate
/// holds the switch and the own read that it asked; then sends the letters of
/// one word and waits until the gate has decided each and the page shows its
/// count of them.
fn switch_back_part(
    part: &'static str,
    (page, away_window): (HWND, &mut AwayWindow),
    gate: &PasswordGate,
    seen: &mut SwitchBackSeen,
    events: &Events,
) -> SwitchBackPart {
    seen.take(events);
    let decided_from_away = seen.decided.len();
    let (away, from_away) =
        switch_by_the_test((away_window.hwnd, page), seen, events, SWITCH_BACK_WAIT);
    let counter = EventCounter::start();
    let handler_before = seam::detector().handler_entered;
    let (back, from) =
        switch_by_the_other_window(away_window, page, seen, events, SWITCH_BACK_WAIT);
    let handler_entered_from_the_switch_to_the_letters = seam::detector()
        .handler_entered
        .saturating_sub(handler_before);
    let system_events_from_the_switch_to_the_letters = counter.stop();
    let gate_at_the_return = gate.content_reading();
    let page_at_the_return = page_read(page).unwrap_or_default();
    let decided_from = seen.decided.len();
    let records_from = seen.records.len();
    let noted_from = seen.noted;
    let letters_sent = send_letters(SWITCH_BACK_LETTERS);
    seen.until(events, SWITCH_BACK_WAIT, |seen| {
        seen.letters_from(decided_from).len() >= SWITCH_BACK_LETTERS
    });
    let letters = seen.letters_from(decided_from);
    let recorded = letters
        .iter()
        .filter(|key| key.verdict == seam::Verdict::Recorded)
        .count();
    seen.until(events, SWITCH_BACK_WAIT, |seen| {
        seen.keystrokes_from(records_from) >= recorded
    });
    let page_counts_seen = seen.until(events, SWITCH_BACK_WAIT, |_| {
        page_read(page).is_some_and(|page| page.text + page.password >= SWITCH_BACK_LETTERS as i64)
    });
    let page_after_the_letters = page_read(page).unwrap_or_default();
    let keys_decided_in_the_part = seen.decided.len() - decided_from_away;
    let (_, read) = seen.switch_and_its_read(from, window_pid(page));
    let read_behind_the_switch = back.at.zip(read).map(|(at, read)| {
        (
            read.state,
            i64::from(read.began.wrapping_sub(at) as i32),
            read.began,
        )
    });
    let first_letter = letters.first().map(|key| key.os_tick);
    let not_after_the_first_letter = |mark: &seam::MarkSeen| {
        first_letter.is_none_or(|key| mark.at.is_some_and(|at| key.wrapping_sub(at) as i32 >= 0))
    };
    let marks_of_the_return = &seen.marks[from.min(seen.marks.len())..];
    let focus_events_from_the_switch_back = marks_of_the_return
        .iter()
        .filter(|mark| mark.what == "event" && not_after_the_first_letter(mark))
        .count();
    let marks_from_the_move: Vec<&seam::MarkSeen> = seen.marks[from_away.min(seen.marks.len())..]
        .iter()
        .filter(|mark| not_after_the_first_letter(mark))
        .collect();
    let focus_events_from_the_move: Vec<String> = marks_from_the_move
        .iter()
        .filter(|mark| mark.what == "event")
        .map(|mark| {
            format!(
                "event@{}:{:?}:{}",
                mark.at.unwrap_or(mark.began),
                mark.state,
                part_of(mark, "kind=", "kind=none")
            )
        })
        .collect();
    let latches_from_the_move = marks_from_the_move
        .iter()
        .filter(|mark| mark.what.starts_with("latch"))
        .count();
    let asked_by = |read: &seam::MarkSeen| {
        if Some(read.began) == away.read_at {
            "switch_away"
        } else if Some(read.began) == back.read_at {
            "switch_back"
        } else if marks_from_the_move.iter().any(|mark| {
            mark.what == "event"
                && mark
                    .at
                    .is_some_and(|at| (0..=30).contains(&(read.began.wrapping_sub(at) as i32)))
        }) {
            "focus_event"
        } else {
            "other"
        }
    };
    let reads_from_the_move: Vec<String> = marks_from_the_move
        .iter()
        .filter(|mark| mark.what == "read")
        .map(|read| format!("read@{}:{:?}:{}", read.began, read.state, asked_by(read)))
        .collect();
    let newest = seen.marks[from_away.min(seen.marks.len())..]
        .iter()
        .rev()
        .find(|mark| {
            (mark.what == "read" || mark.what == "event") && not_after_the_first_letter(mark)
        });
    let newest_before_the_first_letter = newest.map_or("none".to_string(), |mark| {
        format!(
            "{}@{}:{:?}",
            mark.what,
            mark.at.unwrap_or(mark.began),
            mark.state
        )
    });
    let newest_is_the_read_behind_the_switch = newest
        .zip(read_behind_the_switch)
        .is_some_and(|(mark, (_, _, began))| mark.what == "read" && mark.began == began);
    let keystroke_records = seen.keystrokes_from(records_from);
    SwitchBackPart {
        part,
        away,
        back,
        keys_decided_in_the_part,
        page_counts_seen,
        read_behind_the_switch,
        focus_events_from_the_switch_back,
        focus_events_from_the_move,
        latches_from_the_move,
        reads_from_the_move,
        handler_entered_from_the_switch_to_the_letters,
        system_events_from_the_switch_to_the_letters,
        gate_at_the_return,
        newest_before_the_first_letter,
        newest_is_the_read_behind_the_switch,
        page_at_the_return,
        page_after_the_letters,
        letters_sent,
        letters,
        keystroke_records,
        records_noted_by_the_seam: seen.noted - noted_from,
        marks: full_marks(marks_of_the_return),
    }
}

/// The keyboard source and the title source of one service set, made by the
/// set's factory as the daemon makes them, on the set's one gate, with pages
/// of `name` that a window with no edit takes the front from and gives it
/// back to, by the window alone, with no click and no key. A first page
/// warms the browser up: it moves its own focus, the gate reads its text box
/// as Clear by Chrome, and one switch away and back is told to the gate with
/// the read that each asked, so that the title source runs in time. Then a
/// page whose own load puts the cursor in its text box, and one whose own
/// load puts it in its password field; each part starts once the gate has
/// read the page's field and the title source has told the gate of the
/// page's switch, and at its return the browser gives the gate a focus event
/// of the page's field, as the hosted runs show. In the text box every
/// letter of the word is recorded; at the password field none is, and no
/// keystroke record is written.
fn switch_back(test: &'static str, name: &'static str) {
    use halvo_capture::daemon::sources::SourceFactory;
    use halvo_control::proto::SourceName;
    with_deadline(test, Duration::from_secs(180), move || {
        env_report(test);
        source_built_by_test(test, "keyboard");
        source_built_by_test(test, "window_title");
        let away_child = spawn_child("front-on-go", 170);
        let mut browser = open_browser(test, name);
        let (sink, events) = capture(4096);
        let mut seen = SwitchBackSeen::default();
        let away_came = seen.until(&events, SWITCH_BACK_WAIT, |_| {
            window_titled(AWAY_TITLE).is_some()
        });
        let (away_hwnd, _) = window_titled(AWAY_TITLE).unwrap_or_else(|| {
            panic!("INCONCLUSIVE: the window with no edit did not come: {away_came}")
        });
        let mut away = AwayWindow {
            child: away_child,
            hwnd: away_hwnd,
            goes: 0,
        };
        let mut set = ServiceSet::new();
        let gate = set.password_gate();
        let mut keyboard = set
            .make(SourceName::Keyboard)
            .expect("the set makes a keyboard source");
        let mut title = set
            .make(SourceName::WindowTitle)
            .expect("the set makes a title source");
        let keyboard_start = keyboard.begin(SourceContext::new(sink.clone()));
        let title_start = title.begin(SourceContext::new(sink.clone()));
        assert!(
            keyboard_start.is_ok() && title_start.is_ok(),
            "the sources must start: keyboard {}, title {}",
            show(&keyboard_start),
            show(&title_start)
        );
        seen.take(&events);
        let from_warm = seen.marks.len();
        let warm_page = browser
            .open(test, "after-start", 0)
            .expect("INCONCLUSIVE: no first page window");
        let warm_way = browser.front_method;
        let warm_read = seen.until(&events, WARM_UP_WAIT, |seen| {
            seen.field_read(from_warm, FieldState::Clear)
                && page_read(warm_page).is_some_and(|page| page.moved_at >= 0)
        });
        let warm_began = Instant::now();
        let mut warm_tries = 0;
        let mut warm_told = false;
        while !warm_told && warm_began.elapsed() < WARM_UP_WAIT {
            warm_tries += 1;
            let (warm_away, _) =
                switch_by_the_test((away.hwnd, warm_page), &mut seen, &events, WARM_UP_TRY_WAIT);
            let (warm_back, _) = switch_by_the_other_window(
                &mut away,
                warm_page,
                &mut seen,
                &events,
                WARM_UP_TRY_WAIT,
            );
            warm_told = warm_away.told && warm_back.told;
            say(
                test,
                format!(
                    "browser={name} warm_up try={warm_tries} way_to_the_front={warm_way} \
                     warm_read={warm_read} {} {} warm_told={warm_told} gate={:?}",
                    warm_away.line("away"),
                    warm_back.line("back"),
                    gate.content_reading()
                ),
            );
        }
        if !(warm_read && warm_told) {
            let _ = away.child.process.kill();
            let _ = away.child.wait();
            away.child.print_output();
            panic!(
                "INCONCLUSIVE: the warm-up did not end with a switch away and back that the gate \
                 was told: warm_read={warm_read} warm_told={warm_told} warm_tries={warm_tries}"
            );
        }
        let mut ready = |seen: &mut SwitchBackSeen, phase: &str, field: FieldState| {
            seen.take(&events);
            let from_open = seen.marks.len();
            let (page, front) =
                open_by_the_window_alone(&mut browser, (test, phase), seen, &events)
                    .expect("INCONCLUSIVE: no page window");
            let focus = if field == FieldState::Password {
                "password"
            } else {
                "plain"
            };
            let came = |seen: &SwitchBackSeen| {
                front
                    && foreground_window() == page
                    && seen.field_read(from_open, field)
                    && page_read(page).is_some_and(|page| page.focus == focus)
            };
            let told = |seen: &SwitchBackSeen| {
                seen.switch_and_its_read(from_open, window_pid(page))
                    .1
                    .is_some()
            };
            seen.until(&events, SWITCH_BACK_WAIT, |seen| came(seen) && told(seen));
            (page, came(seen), told(seen))
        };
        let (plain_page, plain_ready, plain_told) = ready(&mut seen, "switch", FieldState::Clear);
        let plain = switch_back_part("plain", (plain_page, &mut away), &gate, &mut seen, &events);
        let (password_page, password_ready, password_told) =
            ready(&mut seen, "cold", FieldState::Password);
        let password = switch_back_part(
            "password",
            (password_page, &mut away),
            &gate,
            &mut seen,
            &events,
        );
        let title_stop = stop_counted(title.as_mut(), "window_title");
        let keyboard_stop = stop_counted(keyboard.as_mut(), "keyboard");
        let _ = away.child.process.kill();
        let _ = away.child.wait();
        away.child.print_output();
        gate_readings(test, &gate);
        say(
            test,
            format!(
                "browser={name} keyboard_start={} title_start={} keyboard_stop={} title_stop={} \
                 plain_ready={plain_ready} plain_told={plain_told} \
                 password_ready={password_ready} password_told={password_told} goes={}",
                show(&keyboard_start),
                show(&title_start),
                show(&keyboard_stop),
                show(&title_stop),
                away.goes
            ),
        );
        for part in [&plain, &password] {
            say(test, format!("browser={name} {}", part.line()));
            say(
                test,
                format!(
                    "browser={name} part={} marks_from_the_switch_back=[{}]",
                    part.part, part.marks
                ),
            );
        }
        let letters = SWITCH_BACK_LETTERS;
        let typed = |part: &SwitchBackPart, (landed, elsewhere): (i64, i64), focus: &str| {
            part.letters_sent as usize == 2 * letters
                && part.letters.len() == letters
                && part.keys_decided_in_the_part == letters
                && part.page_at_the_return.focus == focus
                && landed == letters as i64
                && elsewhere == 0
        };
        assert!(
            plain_ready && password_ready && plain.came_in_order() && password.came_in_order(),
            "INCONCLUSIVE: a state of the row did not come: plain_ready={plain_ready} \
             password_ready={password_ready}, the lines of the parts above"
        );
        assert!(
            plain_told && password_told && plain.told_in_order() && password.told_in_order(),
            "a switch of the row came with no read that it asked: plain_told={plain_told} \
             password_told={password_told}, the lines of the parts above"
        );
        assert!(
            typed(
                &password,
                (
                    password.page_after_the_letters.password,
                    password.page_after_the_letters.text
                ),
                "password"
            ),
            "INCONCLUSIVE: the part at the password field did not type the whole word into it \
             alone, its line above"
        );
        // The seam's notes of recorded keys are no term of these asserts: this
        // test file's own sink forwarder (fn seen) takes one such note for each
        // keystroke record that it forwards, so the row's count of them reads 0
        // whenever records come; the seam's verdicts and the sink's keystroke
        // records are the reading.
        assert!(
            password.letters_recorded() == 0 && password.keystroke_records == 0,
            "a key at the password field behind a switch back was recorded: letters_recorded={} \
             keystroke_records={}",
            password.letters_recorded(),
            password.keystroke_records
        );
        assert!(
            typed(
                &plain,
                (
                    plain.page_after_the_letters.text,
                    plain.page_after_the_letters.password
                ),
                "plain"
            ),
            "INCONCLUSIVE: the part at the text box did not type the whole word into it alone, \
             its line above"
        );
        assert!(
            plain.letters_recorded() == letters && plain.keystroke_records == letters,
            "a letter of the word in the text box behind a switch back was not recorded: \
             letters_recorded={} keystroke_records={}",
            plain.letters_recorded(),
            plain.keystroke_records
        );
    });
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop and Chrome"]
fn a_switch_back_to_chrome_that_gives_a_focus_event_records_a_word_and_no_key_at_a_password_field()
{
    switch_back("browser-switch-back-chrome", "chrome");
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop and Chrome"]
fn another_field_that_changes_its_value_by_itself_records_no_password_letter_in_chrome() {
    other_field_phases("browser-other-chrome", "chrome");
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop and Edge"]
fn another_field_that_changes_its_value_by_itself_records_no_password_letter_in_edge() {
    other_field_phases("browser-other-edge", "edge");
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop and Chrome"]
fn every_letter_in_a_chat_box_that_keeps_the_focus_behind_enter_is_noted_in_chrome() {
    chat_box_phases("browser-chat-chrome", "chrome");
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop and Edge"]
fn every_letter_in_a_chat_box_that_keeps_the_focus_behind_enter_is_noted_in_edge() {
    chat_box_phases("browser-chat-edge", "edge");
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop and Chrome"]
fn a_key_at_the_password_input_between_two_moves_of_the_page_records_nothing_in_chrome() {
    there_and_back_phases("browser-there-and-back-chrome", "chrome");
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop and Edge"]
fn a_key_at_the_password_input_between_two_moves_of_the_page_records_nothing_in_edge() {
    there_and_back_phases("browser-there-and-back-edge", "edge");
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop with Chrome"]
fn the_pages_own_change_of_the_focused_input_before_its_late_move_records_nothing_in_chrome() {
    own_change_row("browser-own-change-chrome", "chrome", true);
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop with Edge"]
fn the_pages_own_change_of_the_focused_input_before_its_late_move_records_nothing_in_edge() {
    own_change_row("browser-own-change-edge", "edge", true);
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop with Chrome"]
fn a_held_page_that_changes_nothing_before_its_late_move_records_nothing_in_chrome() {
    own_change_row("browser-own-none-chrome", "chrome", false);
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop with Edge"]
fn a_held_page_that_changes_nothing_before_its_late_move_records_nothing_in_edge() {
    own_change_row("browser-own-none-edge", "edge", false);
}

fn own_change_row(test: &'static str, name: &'static str, change: bool) {
    let recorded = own_change_phases(test, name, change);
    assert!(
        recorded == 0,
        "{recorded} letters that the page took at its password input after its own change were recorded"
    );
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop and Chrome"]
fn a_key_queued_before_the_pages_late_move_records_nothing_in_chrome() {
    let recorded = earlier_key_phases("browser-take-none-chrome", "chrome", false);
    assert_eq!(
        recorded, 0,
        "a letter that landed in the password input was recorded"
    );
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop and Edge"]
fn a_key_queued_before_the_pages_late_move_records_nothing_in_edge() {
    let recorded = earlier_key_phases("browser-take-none-edge", "edge", false);
    assert_eq!(
        recorded, 0,
        "a letter that landed in the password input was recorded"
    );
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

/// The page of the Chrome row: its title is `base n`, with n counting up each
/// second, so that the window's title changes behind its first two seconds; it
/// holds a toolbar of its own with buttons named Back, Forward and Reload.
const CHROME_ROW_PAGE: &str = r#"<!doctype html>
<html><head><meta charset="utf-8"><title>chrome row</title></head><body>
<div role="toolbar" aria-label="page tools"><button>Back</button><button>Forward</button><button>Reload</button></div>
<p>chrome row</p>
<script>
const base = new URLSearchParams(location.search).get('base') || 'chromerow';
let n = 0;
function tick() { n++; document.title = base + ' ' + n; }
tick();
setInterval(tick, 1000);
</script></body></html>
"#;

/// What one window of the row gave: whether it came, came in front and came
/// back in front behind the window with no edit, the titles of chrome.exe
/// written, and the titles that the title source dropped at the private
/// window.
struct ChromeWindowPart {
    found: bool,
    front: bool,
    back: bool,
    title_events: usize,
    private_drops: u64,
}

/// Starts the runner's Chrome on the row's page under `base`, with a profile
/// of its own and with `flag` where one is given, brings the window in front,
/// puts it in full screen at once where `full_screen` is set (inside its first
/// two seconds), sends it behind the window `away` and brings it back, and
/// ends Chrome.
fn chrome_window_part(
    (exe, flag, base): (&Path, Option<&str>, &str),
    full_screen: bool,
    (events, title, away): (
        &Events,
        &halvo_capture::windows::sources::Gated<WindowTitleSource>,
        HWND,
    ),
) -> ChromeWindowPart {
    use windows::Win32::UI::Input::KeyboardAndMouse::VK_F11;
    let profile = scratch_dir();
    let page_path = profile.path().join("chrome.html");
    let page_written = std::fs::write(&page_path, CHROME_ROW_PAGE).is_ok();
    let url = format!(
        "file:///{}?base={base}",
        page_path.to_string_lossy().replace('\\', "/")
    );
    let mut args = vec![
        format!("--user-data-dir={}", profile.path().join("data").display()),
        "--no-first-run".to_string(),
        "--no-default-browser-check".to_string(),
        "--disable-features=Translate".to_string(),
    ];
    args.extend(flag.map(str::to_string));
    args.push(url);
    let before = title.source().drops();
    let started = page_written
        && Command::new(exe)
            .args(&args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .is_ok();
    let came = Instant::now();
    let mut window = None;
    while started && came.elapsed() < Duration::from_secs(20) {
        if let Some((hwnd, _)) = window_titled(&format!("{base} ")) {
            window = Some(hwnd);
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let mut seen = Vec::new();
    let (mut front, mut back) = (false, false);
    if let Some(window) = window {
        front = bring_to_front(window, false).0;
        if full_screen {
            send(&[key_input(VK_F11, false)]);
            send(&[key_input(VK_F11, true)]);
        }
        seen.extend(events.drain_for(Duration::from_millis(3_500)));
        let _ = bring_to_front(away, false);
        seen.extend(events.drain_for(Duration::from_millis(400)));
        back = bring_to_front(window, false).0;
        seen.extend(events.drain_for(Duration::from_millis(4_000)));
    }
    let _ = Command::new("taskkill.exe")
        .args(["/IM", "chrome.exe", "/F", "/T"])
        .output();
    seen.extend(events.drain_for(Duration::from_millis(900)));
    let after = title.source().drops();
    ChromeWindowPart {
        found: window.is_some(),
        front,
        back,
        title_events: seen
            .iter()
            .filter(|event| {
                event.modality == Modality::WindowTitle && event.foreground_app == "other.exe"
            })
            .count(),
        private_drops: after.private_window - before.private_window,
    }
}

/// A real ordinary Chrome window gives its titles, and a real Chrome Guest
/// window and a real Incognito window in full screen whose page holds a
/// toolbar of its own give none. Each starts on a page of the test's own, with
/// the keyboard source and the title source of one service set running, comes
/// in front, goes behind the window with no edit and comes back. The ordinary
/// window writes at least one title of chrome.exe; each private one writes
/// none, and the title source drops at least one of its titles at the private
/// window.
fn chrome_windows_keep_titles_only_when_ordinary(test: &'static str) {
    use halvo_capture::daemon::sources::SourceFactory;
    use halvo_control::proto::SourceName;
    env_report(test);
    let mut away_child = spawn_child("front-on-go", 120);
    let came = Instant::now();
    while window_titled(AWAY_TITLE).is_none() && came.elapsed() < Duration::from_secs(20) {
        std::thread::sleep(Duration::from_millis(200));
    }
    let away = window_titled(AWAY_TITLE)
        .map(|(hwnd, _)| hwnd)
        .unwrap_or_default();
    let exe = CHROME
        .paths
        .iter()
        .map(Path::new)
        .find(|path| path.is_file())
        .map(Path::to_path_buf);
    let (sink, events) = capture(8192);
    let mut set = ServiceSet::new();
    let mut keyboard = set
        .make(SourceName::Keyboard)
        .expect("the set makes a keyboard source");
    let mut title = set.gated_title(WindowTitleSource::new());
    let keyboard_start = keyboard.begin(SourceContext::new(sink.clone()));
    let title_start = title.begin(SourceContext::new(sink.clone()));
    let parts = exe.as_ref().map(|exe| {
        [
            ("ordinary", None, false, true),
            ("guest", Some("--guest"), false, false),
            ("private_full_screen", Some("--incognito"), true, false),
        ]
        .map(|(part, flag, full_screen, kept)| {
            let base = format!("chromerow{part}");
            (
                part,
                kept,
                chrome_window_part(
                    (exe.as_path(), flag, base.as_str()),
                    full_screen,
                    (&events, &title, away),
                ),
            )
        })
    });
    let title_stop = stop_counted(&mut title, "window_title");
    let keyboard_stop = stop_counted(keyboard.as_mut(), "keyboard");
    let _ = away_child.process.kill();
    let _ = away_child.process.wait();
    away_child.print_output();
    say(
        test,
        format!(
            "browser=chrome chrome_found={} keyboard_start={} title_start={} keyboard_stop={} \
             title_stop={}",
            exe.is_some(),
            show(&keyboard_start),
            show(&title_start),
            show(&keyboard_stop),
            show(&title_stop)
        ),
    );
    let parts: Vec<(&str, bool, ChromeWindowPart)> = parts.map(Vec::from).unwrap_or_default();
    for (part, _, got) in &parts {
        say(
            test,
            format!(
                "part={part} found={} front={} back={} title_events={} private_drops={}",
                got.found, got.front, got.back, got.title_events, got.private_drops
            ),
        );
    }
    let wrong: Vec<&str> = parts
        .iter()
        .filter(|(_, kept, got)| {
            let came = got.found && got.front && got.back;
            let as_ruled = if *kept {
                got.title_events > 0
            } else {
                got.title_events == 0 && got.private_drops > 0
            };
            !(came && as_ruled)
        })
        .map(|(part, ..)| *part)
        .collect();
    assert!(
        exe.is_some() && wrong.is_empty(),
        "chrome_found={} wrong={wrong:?}",
        exe.is_some()
    );
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop and Chrome"]
fn a_chrome_guest_window_gives_no_title() {
    let test = "a_chrome_guest_window_gives_no_title";
    let hosted_runner = std::env::var("GITHUB_ACTIONS").ok().as_deref() == Some("true");
    if !hosted_runner {
        say(test, format!("hosted_runner={hosted_runner}"));
        return;
    }
    with_deadline(test, Duration::from_secs(150), move || {
        chrome_windows_keep_titles_only_when_ordinary(test)
    });
}
