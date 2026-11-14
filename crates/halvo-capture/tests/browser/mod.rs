use super::*;
use halvo_capture::secure_input::windows::FieldState;
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
const c = { text: 0, password: 0, tp: 0, tt: 0, downs: 0, round: 0 };
let armed = false;
let timers = [];
const busy = (ms) => { const end = performance.now() + ms; while (performance.now() < end) {} };
const where = () => document.activeElement ? (document.activeElement.id || 'body') : 'none';
function show() {
  const r = password.getBoundingClientRect();
  const top = window.screenY + (window.outerHeight - window.innerHeight);
  const left = window.screenX + (window.outerWidth - window.innerWidth) / 2;
  document.title = 'halvo-phase ' + phase + run + ' text=' + c.text + ' password=' + c.password +
    ' tp=' + c.tp + ' tt=' + c.tt + ' downs=' + c.downs + ' round=' + c.round +
    ' focus=' + where() + ' ptype=' + plain.type +
    ' px=' + Math.round(left + r.left + r.width / 2) + ' py=' + Math.round(top + r.top + r.height / 2) + ' end';
}
const later = (ms, f) => timers.push(setTimeout(() => { f(); show(); }, ms));
const swap = () => { busy(hold); plain.type = 'password'; busy(hold); };
for (const el of [plain, password]) {
  el.addEventListener('input', () => { if (el.type === 'password') c.password++; else c.text++; show(); });
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
  // The burst's swap is armed 1 s after the reset, so that the control's
  // letters before it go into the text input (W3-10 T5).
  armed = phase !== 'burst';
  if (phase === 'burst') later(1000, () => { armed = true; });
  if (phase === 'cold' || phase === 'show-hide' || phase === 'show-control') password.focus();
  else if (phase === 'swap-focus') { button.focus(); later(300, () => plain.focus()); }
  // The burst's page moves its own focus there and back, so that the gate's
  // newest mark is a focus event of this page and not a stale event of the
  // window that lost the front (W3-10 T5; run w39-2).
  else if (phase === 'burst') { button.focus(); later(150, () => plain.focus()); }
  else plain.focus();
  if (phase === 'swap-timer' || phase === 'tab-after') later(400, () => { armed = false; swap(); });
  if (phase === 'show-hide') { later(300, () => { password.type = 'text'; }); later(1300, () => { password.type = 'password'; }); }
  if (phase === 'show-control') {
    later(300, () => { button.focus(); password.type = 'text'; });
    later(600, () => password.focus());
    later(1300, () => { password.type = 'password'; });
  }
  if (phase === 'self') later(600, () => { busy(hold); password.focus(); });
  show();
}
document.addEventListener('focusin', () => show());
document.addEventListener('keydown', (e) => {
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
    })
}

#[derive(Debug, Default, Clone, Copy)]
struct Decisions {
    letters: usize,
    letters_decided: usize,
    tabs: usize,
    other: usize,
    first_tab_rule: Option<seam::Rule>,
    first_letter_rule: Option<seam::Rule>,
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
                std::thread::sleep(Duration::from_millis(700));
                say(
                    test,
                    format!(
                        "browser={} phase={phase} hold={hold} opened=1 foreground={front} method={method}",
                        self.name
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
    fn start() -> Keyboard {
        let (sink, events) = capture(8192);
        let mut source = RawInputSource::keyboard(Arc::new(Mutex::new(None)), PasswordGate::new());
        let start = source.begin(SourceContext::new(sink));
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
    let newest: Vec<String> = marks
        .iter()
        .rev()
        .take(3)
        .map(|mark| {
            let file = mark
                .element
                .as_deref()
                .and_then(|e| e.split(' ').find(|part| part.starts_with("file=")))
                .unwrap_or("file=none");
            format!("{}:{:?}:{file}", mark.what, mark.state)
        })
        .collect();
    (element, newest.join(","))
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
}

impl Round {
    fn password_letters_recorded(&self) -> i64 {
        (self.keys.letters as i64 - self.page.text).max(0)
    }

    fn line(&self) -> String {
        format!(
            "counted={} letters_sent={} landed_text={} landed_password={} tabs_into_password={} \
             tabs_into_text={} button_downs={} focus_after={} letters_decided={} letters_recorded={} \
             tabs_recorded={} other_recorded={} first_letter_rule={:?} first_tab_rule={:?}",
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
        )
    }
}

fn end_round(keyboard: &Keyboard, hwnd: HWND, sent: i64, counted_before: bool) -> Round {
    keyboard.settle(Duration::from_millis(800));
    let keys = decisions();
    let page = page_read(hwnd).unwrap_or_default();
    Round {
        counted: counted_before,
        sent,
        page,
        keys,
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
            (r.counted && (r.keys.letters > 0 || r.keys.other > 0))
                || r.password_letters_recorded() > 0
        })
        .count();
    say(
        test,
        format!(
            "phase={phase} rounds={} counted={counted} failing_rounds={failing}",
            rounds.len()
        ),
    );
    assert_eq!(
        failing, 0,
        "{phase}: a letter of the password input, or an event of another class, was recorded"
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
        let mut browser = open_browser(test, name);
        let keyboard = Keyboard::start();
        assert!(keyboard.start.is_ok(), "the keyboard source must start");
        let mut all = Vec::new();
        for kind in [StallKind::Tab, StallKind::Click] {
            for hold in [150, 300] {
                let rounds = stall_rounds(test, &mut browser, &keyboard, kind, hold, 5);
                all.push((format!("{kind:?}-{hold}"), rounds));
            }
        }
        let late = keyboard.source.health().counters.late_events;
        let stopped = keyboard.stop();
        say(
            test,
            format!("keyboard_stop={} late_value={late}", show(&stopped)),
        );
        for (phase, rounds) in &all {
            assert_stall(test, phase, rounds, 3);
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
        let keyboard = Keyboard::start();
        assert!(keyboard.start.is_ok(), "the keyboard source must start");
        let mut counted = 0;
        for hold in [150, 300] {
            for r in stall_rounds(test, &mut browser, &keyboard, StallKind::SelfMove, hold, 5) {
                counted += usize::from(r.counted);
                recorded += r.password_letters_recorded();
            }
        }
        let _ = keyboard.stop();
        say(
            test,
            format!("phase=self counted={counted} password_letters_recorded={recorded}"),
        );
        out.store(recorded, Ordering::SeqCst);
        assert!(
            counted >= 5,
            "INCONCLUSIVE: the self-move phase counted {counted} rounds"
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
    }
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
        let mut browser = open_browser(test, name);
        let keyboard = Keyboard::start();
        assert!(keyboard.start.is_ok(), "the keyboard source must start");
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
            for round in 0..3 {
                let (r, _) = swap_round(test, name, &keyboard, hwnd, phase, round);
                counted += u32::from(r.counted);
                let failed = if phase == "show-hide" {
                    r.keys.letters > 0
                } else {
                    r.password_letters_recorded() > 0
                };
                if failed {
                    failures.push(format!("{phase}#{round}"));
                }
            }
            if counted < 2 {
                thin.push(phase);
            }
        }
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
            failures.is_empty(),
            "a letter of a password field was recorded: {failures:?}"
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
        let keyboard = Keyboard::start();
        assert!(keyboard.start.is_ok(), "the keyboard source must start");
        let mut counted = 0;
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
                    r.counted = front
                        && (phase == "swap-focus" || start.focus == "plain")
                        && r.page.password >= 1;
                    counted += usize::from(r.counted);
                    recorded += r.password_letters_recorded();
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
            format!("phase=held-swaps counted={counted} password_letters_recorded={recorded}"),
        );
        out.store(recorded, Ordering::SeqCst);
        assert!(
            counted >= 8,
            "INCONCLUSIVE: the held swaps counted {counted} rounds"
        );
    });
    total.load(Ordering::SeqCst)
}

pub(super) fn burst_phase(test: &'static str, name: &'static str) -> i64 {
    let total = Arc::new(AtomicI64::new(0));
    let out = Arc::clone(&total);
    with_deadline(test, Duration::from_secs(240), move || {
        let mut recorded = 0;
        source_built_by_test(test, "keyboard");
        let mut browser = open_browser(test, name);
        let keyboard = Keyboard::start();
        assert!(keyboard.start.is_ok(), "the keyboard source must start");
        let hwnd = browser
            .open(test, "burst", 0)
            .expect("INCONCLUSIVE: no page window");
        let mut counted = 0;
        for round in 0..5 {
            let _ = seam::take_marks();
            let start = reset(&keyboard, hwnd).unwrap_or_default();
            let front = foreground_window() == hwnd;
            let control_sent = i64::from(type_letters_slowly(3) / 2);
            let control = keyboard.keystrokes_for(Duration::from_millis(600));
            super::control_reads(test, "burst-control", &seam::take_marks(), &seam::take());
            std::thread::sleep(Duration::from_millis(250));
            let gate_before = keyboard.source.field_state();
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
            let password_recorded = r.keys.letters as i64;
            if r.counted {
                recorded += password_recorded;
            }
            say(
                test,
                format!(
                    "browser={name} phase=burst round={round} control_letters_sent={control_sent} \
                     control_keystroke_events={control} gate_before_the_burst={gate_before:?} {}",
                    r.line()
                ),
            );
        }
        let _ = keyboard.stop();
        say(
            test,
            format!("phase=burst counted={counted} password_letters_recorded={recorded}"),
        );
        out.store(recorded, Ordering::SeqCst);
        assert!(
            counted >= 1,
            "the burst counted no round that began from an open gate"
        );
    });
    total.load(Ordering::SeqCst)
}

pub(super) fn letter_and_tab_out_of_a_swapped_field(test: &'static str, name: &'static str) -> i64 {
    let total = Arc::new(AtomicI64::new(0));
    let out = Arc::clone(&total);
    with_deadline(test, Duration::from_secs(240), move || {
        let mut recorded = 0;
        source_built_by_test(test, "keyboard");
        let mut browser = open_browser(test, name);
        let keyboard = Keyboard::start();
        assert!(keyboard.start.is_ok(), "the keyboard source must start");
        let hwnd = browser
            .open(test, "tab-after", 0)
            .expect("INCONCLUSIVE: no page window");
        let mut counted = 0;
        for round in 0..5 {
            let _ = seam::take_marks();
            let start = reset(&keyboard, hwnd).unwrap_or_default();
            let front = foreground_window() == hwnd;
            std::thread::sleep(Duration::from_millis(800));
            let swapped = page_read(hwnd).is_some_and(|p| p.plain_type == "password");
            key(VK_A);
            std::thread::sleep(Duration::from_millis(50));
            key(VK_TAB);
            keyboard.settle(Duration::from_millis(800));
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
            };
            r.counted = front
                && start.focus == "plain"
                && swapped
                && r.page.password == 1
                && r.page.tabs_password == 1;
            counted += u32::from(r.counted);
            if r.counted {
                recorded += r.keys.letters as i64 + r.keys.tabs as i64;
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
            format!("phase=tab-after counted={counted} password_keys_recorded={recorded}"),
        );
        out.store(recorded, Ordering::SeqCst);
        assert!(
            counted >= 3,
            "INCONCLUSIVE: the phase counted {counted} rounds"
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
        let keyboard = Keyboard::start();
        assert!(keyboard.start.is_ok(), "the keyboard source must start");
        let _ = seam::take_marks();
        let cold = browser
            .open_with(test, "cold", 0, true)
            .expect("INCONCLUSIVE: no page window");
        let cold_front = foreground_window() == cold;
        let cold_state = keyboard.state_within(FieldState::Password, Duration::from_secs(5));
        let (cold_element, cold_newest) = marks_seen(&file);
        let cold_page = page_read(cold).unwrap_or_default();
        let _ = seam::take();
        let sent = i64::from(type_letters_slowly(3) / 2);
        let cold_round = end_round(&keyboard, cold, sent, cold_front);
        say(
            test,
            format!(
                "browser={name} order=cold foreground={cold_front} focus_before={} gate_while_focused={cold_state:?} \
                 {} newest_marks=[{cold_newest}] {cold_element}",
                cold_page.focus,
                cold_round.line()
            ),
        );

        let warm = browser
            .open(test, "warm", 0)
            .expect("INCONCLUSIVE: no page window");
        let _ = seam::take_marks();
        let start = reset(&keyboard, warm).unwrap_or_default();
        let warm_front = foreground_window() == warm;
        let plain_state = keyboard.state_within(FieldState::Clear, Duration::from_secs(3));
        let _ = seam::take();
        let sent = i64::from(type_letters_slowly(5) / 2);
        keyboard.settle(Duration::from_millis(800));
        let control_keys = seam::take();
        let control_marks = seam::take_marks();
        super::control_reads(test, "warm-control", &control_marks, &control_keys);
        let plain_element = element_in(&control_marks, &file);
        let control = Round {
            counted: warm_front,
            sent,
            page: page_read(warm).unwrap_or_default(),
            keys: decisions_of(&control_keys),
        };
        key(VK_F22);
        let warm_state = keyboard.state_within(FieldState::Password, Duration::from_secs(3));
        let warm_element = element_seen(&file);
        let _ = seam::take();
        let sent = i64::from(type_letters_slowly(3) / 2);
        let warm_round = end_round(&keyboard, warm, sent, warm_front);
        let stopped = keyboard.stop();
        say(
            test,
            format!(
                "browser={name} order=warm-control foreground={warm_front} focus_before={} \
                 gate_while_focused={plain_state:?} {} {plain_element}",
                start.focus,
                control.line()
            ),
        );
        say(
            test,
            format!(
                "browser={name} order=warm gate_while_focused={warm_state:?} {} {warm_element} keyboard_stop={}",
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
            cold_element.contains("is_password=1"),
            "cold: the gate's reading was not of the password input"
        );
        assert_eq!(
            cold_round.keys.letters, 0,
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
            control.keys.letters, 5,
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
            warm_round.keys.letters, 0,
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
        let keyboard = Keyboard::start();
        assert!(keyboard.start.is_ok(), "the keyboard source must start");
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
            front && if browser.chromium { landed } else { read_the_copy },
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
            plain.keys.letters + password.keys.letters,
            0,
            "a letter typed into the renamed copy was recorded"
        );
    });
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
