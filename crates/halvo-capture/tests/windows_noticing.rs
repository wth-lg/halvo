#![cfg(all(target_os = "windows", feature = "r2"))]
#![deny(clippy::undocumented_unsafe_blocks)]

//! The walk of the check 59-windows-noticing (scripts/r1/ci.d/59-windows-noticing.sh): the
//! windows and the input of one run that shows the noticing of a repeated task on a hosted
//! Windows machine. The check copies this test's executable to `EXCEL.EXE` and to
//! `noticing-pass.exe` and starts both by name from its own shell; the program under the file
//! name `EXCEL.EXE` is the check's own and not Microsoft Excel.
//!
//! The one test has two roles, by the switch `HALVO_NOTICING_ROLE`:
//!
//! - `workbook` shows the window `Budget.xlsx - Excel` and a second window `Notes`, serves its
//!   messages for the whole run and takes one step at a time from the check: the check writes
//!   the step's name into `<n>.go` of the directory `HALVO_NOTICING_DIR`, and the walk writes
//!   the step's one line into `<n>.done`. Every input is a SendInput call. The walk reads no
//!   count of the capture service: the check reads them. Its last step asks the service of
//!   the data directory `HALVO_NOTICING_DATA_DIR` for the workflows, with the request that the
//!   app's command `workflows` sends.
//! - `passer` shows a window whose title is 40 signs longer than the workbook's, so that the
//!   check tells a title record of that window from one of the workbook's by its bytes, and
//!   hands the front back to the workbook's window right behind the message that activated it,
//!   before it serves another message.
//!
//! No click of the walk lands outside its own two windows: before each click it reads the
//! top-level window under its click point.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use halvo_control::client::{system_connector, Client, ClientError};
use halvo_control::paths::DataDir;
use halvo_control::proto::ErrorCode;
use halvo_schema::request::{decode_reply, InterfaceRead, InterfaceRole, ReplyResult};
use halvo_schema::{RequestId, Trigger, Workflow, WorkflowId, WorkflowItem, WorkflowState};

use windows::core::{BOOL, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows::Win32::Graphics::Gdi::CreateSolidBrush;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetLastInputInfo, SendInput, INPUT, INPUT_0, INPUT_MOUSE, LASTINPUTINFO, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEINPUT, MOUSE_EVENT_FLAGS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, EnumWindows, GetAncestor, GetClassNameW,
    GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId, IsWindow, IsWindowVisible,
    PeekMessageW, RegisterClassW, SetCursorPos, SetForegroundWindow, SetWindowPos, ShowWindow,
    TranslateMessage, WindowFromPoint, EVENT_SYSTEM_FOREGROUND, GA_ROOT, HWND_TOPMOST, MSG,
    PM_REMOVE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_SHOWNOACTIVATE, WINEVENT_OUTOFCONTEXT,
    WM_ACTIVATE, WNDCLASSW, WS_EX_TOPMOST, WS_OVERLAPPEDWINDOW,
};

#[link(name = "kernel32")]
extern "system" {
    fn GetTickCount() -> u32;
}

const DIR_SWITCH: &str = "HALVO_NOTICING_DIR";
const DATA_DIR_SWITCH: &str = "HALVO_NOTICING_DATA_DIR";
const ROLE_SWITCH: &str = "HALVO_NOTICING_ROLE";

const WORKBOOK_TITLE: &str = "Budget.xlsx - Excel";
const NOTES_TITLE: &str = "Notes";
const PASS_TITLE: &str = "Pass - the second program of the check hands the front back";

// The check reads the titles of a return as the workbook's where their bytes lie within 4 a
// title of the workbook's first title record; a return gains two titles at most.
const _: () = assert!(PASS_TITLE.len() == WORKBOOK_TITLE.len() + 40);
const APP_FILE: &str = "halvo.exe";

const WIDTH: i32 = 420;
const HEIGHT: i32 = 240;

/// The clicks of one occasion.
const CLICKS: u32 = 12;

/// How long Halvo's own window stays in front at the return `own`.
const OWN_STAY: Duration = Duration::from_millis(3_000);

/// The longest life of either role, under the limit of the check's script.
const WALK_LIMIT: Duration = Duration::from_secs(1_750);

/// One foreground event as Windows raised it: the window, the event's time in the
/// milliseconds of the tick count, and the window's class and the file name of its program,
/// both read at the event.
struct Seen {
    window: usize,
    time: u32,
    class: String,
    file: String,
}

/// Set by the window procedure at the message that activates a window of this process.
static ACTIVATED: AtomicBool = AtomicBool::new(false);

/// The foreground events since the last step's end.
static SEEN: Mutex<Vec<Seen>> = Mutex::new(Vec::new());

#[test]
#[ignore = "check 59-windows-noticing runs it by name on a hosted Windows runner: it shows windows and makes pointer input"]
fn the_walk_takes_its_steps_from_the_check() {
    let hosted = std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true")
        && std::env::var("RUNNER_ENVIRONMENT").as_deref() == Ok("github-hosted");
    assert!(
        hosted,
        "the walk shows windows and makes pointer input: it runs on a GitHub-hosted Windows runner alone"
    );
    let dir = PathBuf::from(switch(DIR_SWITCH));
    let role = switch(ROLE_SWITCH);
    match role.as_str() {
        "workbook" => workbook(&dir, &switch(DATA_DIR_SWITCH)),
        "passer" => passer(&dir),
        other => panic!("{ROLE_SWITCH} is {other}, neither workbook nor passer"),
    }
}

/// The value of the switch `name`, which the check sets: an unset switch fails the test.
fn switch(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!("the switch {name} is not set: check 59-windows-noticing sets it")
    })
}

unsafe extern "system" fn walk_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_ACTIVATE && (wparam.0 & 0xFFFF) != 0 {
        ACTIVATED.store(true, Ordering::SeqCst);
    }
    // SAFETY: the four values are those that the system handed this procedure.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

unsafe extern "system" fn on_foreground(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _thread: u32,
    time: u32,
) {
    let seen = Seen {
        window: hwnd.0 as usize,
        time,
        class: class_of(hwnd),
        file: file_of(hwnd),
    };
    if let Ok(mut all) = SEEN.lock() {
        all.push(seen);
    }
}

/// A top-level window of this process, above every window that is not topmost, shown without
/// taking the front.
fn make_window(class: &str, title: &str, x: i32, y: i32) -> HWND {
    let class_name = HSTRING::from(class);
    let title = HSTRING::from(title);
    // SAFETY: a null module name asks for the handle of this executable, which stays loaded
    // for the life of the process.
    let instance: HINSTANCE = unsafe { GetModuleHandleW(None) }
        .expect("the module handle of the walk's program")
        .into();
    // SAFETY: CreateSolidBrush takes a color by value; the class keeps the brush for the life
    // of the process.
    let brush = unsafe { CreateSolidBrush(COLORREF(0x00FF_FFFF)) };
    let registration = WNDCLASSW {
        lpfnWndProc: Some(walk_wnd_proc),
        hInstance: instance,
        lpszClassName: PCWSTR(class_name.as_ptr()),
        hbrBackground: brush,
        ..Default::default()
    };
    // SAFETY: `registration` points at `class_name`, which lives across the call; the class
    // stays registered for the life of the process.
    let atom = unsafe { RegisterClassW(&registration) };
    assert!(
        atom != 0,
        "RegisterClassW: {}",
        windows::core::Error::from_thread()
    );
    // SAFETY: the class name and the title point at live HSTRINGs for the call, and the class
    // was registered above.
    let window = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST,
            PCWSTR(class_name.as_ptr()),
            PCWSTR(title.as_ptr()),
            WS_OVERLAPPEDWINDOW,
            x,
            y,
            WIDTH,
            HEIGHT,
            None,
            None,
            Some(instance),
            None,
        )
    }
    .expect("CreateWindowExW");
    // SAFETY: ShowWindow takes this thread's window handle by value.
    unsafe {
        let _ = ShowWindow(window, SW_SHOWNOACTIVATE);
    }
    window
}

/// Serves the messages of this thread for `window`.
fn pump(window: Duration) {
    let deadline = Instant::now() + window;
    let mut msg = MSG::default();
    loop {
        // SAFETY: `msg` is a live MSG that PeekMessageW fills, and the same value is handed
        // to TranslateMessage and DispatchMessageW.
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn front() -> HWND {
    // SAFETY: GetForegroundWindow takes no argument and returns a handle value.
    unsafe { GetForegroundWindow() }
}

fn set_front(window: HWND) {
    // SAFETY: SetForegroundWindow takes a window handle by value; a stale handle makes the
    // call fail and changes nothing.
    unsafe {
        let _ = SetForegroundWindow(window);
    }
}

/// Serves messages until `window` is in front, `within` at most; whether it came in front.
fn wait_front(window: HWND, within: Duration) -> bool {
    let deadline = Instant::now() + within;
    loop {
        if front() == window {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        pump(Duration::from_millis(20));
    }
}

/// `text` as one word of a step's line: no white space, and `-` for nothing.
fn word(text: &str) -> String {
    let word: String = text
        .chars()
        .map(|c| if c.is_whitespace() { '_' } else { c })
        .collect();
    if word.is_empty() {
        "-".to_string()
    } else {
        word
    }
}

fn class_of(window: HWND) -> String {
    let mut buf = [0u16; 128];
    // SAFETY: `buf` is a live buffer whose length the call takes.
    let len = unsafe { GetClassNameW(window, &mut buf) };
    word(&String::from_utf16_lossy(
        &buf[..usize::try_from(len).unwrap_or(0)],
    ))
}

/// The file name of the program that owns `window`, or `-` where it cannot be read.
fn file_of(window: HWND) -> String {
    let mut pid = 0u32;
    // SAFETY: `pid` is a live u32 that the call writes the process id into.
    unsafe {
        GetWindowThreadProcessId(window, Some(&mut pid));
    }
    // SAFETY: OpenProcess takes plain values; the handle is closed once below.
    let Ok(handle) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }) else {
        return "-".to_string();
    };
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    // SAFETY: `buf` and `len` are live, `len` holds the length of `buf`, and `handle` is open
    // for the call.
    let read = unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
    };
    // SAFETY: `handle` is the handle that OpenProcess returned above.
    unsafe {
        let _ = CloseHandle(handle);
    }
    if read.is_err() {
        return "-".to_string();
    }
    let path = String::from_utf16_lossy(&buf[..(len as usize).min(buf.len())]);
    word(path.rsplit(['\\', '/']).next().unwrap_or(""))
}

fn rect_of(window: HWND) -> Option<RECT> {
    let mut rect = RECT::default();
    // SAFETY: `rect` is a live RECT that the call writes into.
    unsafe { GetWindowRect(window, &mut rect) }.ok()?;
    Some(rect)
}

/// The top-level window under `point`.
fn window_at(point: POINT) -> HWND {
    // SAFETY: WindowFromPoint and GetAncestor take plain values.
    unsafe { GetAncestor(WindowFromPoint(point), GA_ROOT) }
}

fn mouse(flags: MOUSE_EVENT_FLAGS, dx: i32, dy: i32) -> INPUT {
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

fn send(inputs: &[INPUT]) -> u32 {
    // SAFETY: `inputs` is a live slice of INPUT values and the size argument is the size of
    // one INPUT, as SendInput requires.
    unsafe { SendInput(inputs, core::mem::size_of::<INPUT>() as i32) }
}

/// One press and one release of the left button where the pointer stands: 1 where the system
/// took both.
fn click() -> u32 {
    let down = send(&[mouse(MOUSEEVENTF_LEFTDOWN, 0, 0)]);
    std::thread::sleep(Duration::from_millis(40));
    let up = send(&[mouse(MOUSEEVENTF_LEFTUP, 0, 0)]);
    down.min(up)
}

/// The line of a step that another window kept from its click.
fn covered_by(cover: HWND) -> String {
    format!(
        "ok=no why=covered front_class={} front_file={}",
        class_of(cover),
        file_of(cover)
    )
}

/// Puts the pointer on the middle of `window`, where `window` is the top-level window under
/// that point; where another window covers it, `window` is put above once more, and a second
/// miss is the line of the step.
fn aim(window: HWND) -> Result<POINT, String> {
    let Some(rect) = rect_of(window) else {
        return Err("ok=no why=no-window".to_string());
    };
    let point = POINT {
        x: (rect.left + rect.right) / 2,
        y: (rect.top + rect.bottom) / 2,
    };
    if window_at(point) != window {
        // SAFETY: SetWindowPos takes this thread's window handle by value.
        unsafe {
            let _ = SetWindowPos(
                window,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
        pump(Duration::from_millis(300));
        let cover = window_at(point);
        if cover != window {
            return Err(covered_by(cover));
        }
    }
    // SAFETY: SetCursorPos takes plain values.
    unsafe {
        let _ = SetCursorPos(point.x, point.y);
    }
    pump(Duration::from_millis(50));
    Ok(point)
}

fn not_in_front(sent: u32) -> String {
    let now = front();
    format!(
        "ok=no why=not-in-front sent={sent} front_class={} front_file={}",
        class_of(now),
        file_of(now)
    )
}

/// One click into `window`, which brings it in front: the line of the step.
fn click_into(window: HWND) -> String {
    if let Err(line) = aim(window) {
        return line;
    }
    let sent = click();
    if sent == 1 && wait_front(window, Duration::from_secs(3)) {
        "ok=yes front=own".to_string()
    } else {
        not_in_front(sent)
    }
}

/// The 12 clicks of an occasion into `window`, which is in front: the pointer is put on its
/// click point once, and each click is a press and a release with no movement.
fn clicks(window: HWND) -> String {
    if front() != window {
        return not_in_front(0);
    }
    let point = match aim(window) {
        Ok(point) => point,
        Err(line) => return line,
    };
    let mut sent = 0;
    for _ in 0..CLICKS {
        let cover = window_at(point);
        if cover != window {
            return covered_by(cover);
        }
        sent += click();
        pump(Duration::from_millis(260));
    }
    if front() == window {
        format!("ok=yes sent={sent} front=own")
    } else {
        not_in_front(sent)
    }
}

fn top_windows() -> Vec<HWND> {
    unsafe extern "system" fn each(hwnd: HWND, found: LPARAM) -> BOOL {
        // SAFETY: `found` is the address of the vector that the caller holds across the
        // enumeration on this thread.
        unsafe { (*(found.0 as *mut Vec<HWND>)).push(hwnd) };
        BOOL::from(true)
    }
    let mut all: Vec<HWND> = Vec::new();
    // SAFETY: the callback has the WNDENUMPROC signature and writes `all` through a pointer
    // made from an exclusive borrow, only while this enumeration runs on this thread; nothing
    // else touches `all` meanwhile.
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut all as *mut Vec<HWND> as isize));
    }
    all
}

/// The largest visible top-level window of the app, Halvo's own window.
fn app_window() -> Option<HWND> {
    top_windows()
        .into_iter()
        .filter(|window| {
            // SAFETY: IsWindowVisible reads a handle value.
            unsafe { IsWindowVisible(*window) }.as_bool()
                && file_of(*window).eq_ignore_ascii_case(APP_FILE)
        })
        .max_by_key(|window| {
            rect_of(*window).map_or(0, |rect| {
                i64::from(rect.right - rect.left) * i64::from(rect.bottom - rect.top)
            })
        })
}

fn seen_count() -> usize {
    SEEN.lock().map_or(0, |all| all.len())
}

fn clear_seen() {
    if let Ok(mut all) = SEEN.lock() {
        all.clear();
    }
}

/// The milliseconds between the first foreground event of `first` and the next one of `then`,
/// by the events' own times; none where both did not come in that order.
fn stay_between(first: HWND, then: HWND) -> Option<u32> {
    let all = SEEN.lock().ok()?;
    let at = all
        .iter()
        .position(|seen| seen.window == first.0 as usize)?;
    let back = all[at + 1..]
        .iter()
        .find(|seen| seen.window == then.0 as usize)?;
    Some(back.time.wrapping_sub(all[at].time))
}

/// The class and the file name of the last window that came in front since the last step and
/// is none of `own`, or `-` and `-`.
fn last_other(own: &[HWND]) -> (String, String) {
    let found = SEEN.lock().ok().and_then(|all| {
        all.iter()
            .rev()
            .find(|seen| own.iter().all(|window| window.0 as usize != seen.window))
            .map(|seen| (seen.class.clone(), seen.file.clone()))
    });
    found.unwrap_or_else(|| ("-".to_string(), "-".to_string()))
}

/// The time since the last input of the session, in milliseconds.
fn idle_ms() -> u32 {
    let mut info = LASTINPUTINFO {
        cbSize: core::mem::size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    // SAFETY: the call writes the structure that `info` is, whose size `cbSize` names.
    if !unsafe { GetLastInputInfo(&mut info) }.as_bool() {
        return 0;
    }
    // SAFETY: GetTickCount takes no argument and returns a value.
    unsafe { GetTickCount() }.wrapping_sub(info.dwTime)
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

/// The step `ready`: whether the app shows a window, within 20 s.
fn ready(workbook: HWND, notes: HWND) -> String {
    let deadline = Instant::now() + Duration::from_secs(20);
    let found = loop {
        if app_window().is_some() {
            break true;
        }
        if Instant::now() >= deadline {
            break false;
        }
        pump(Duration::from_millis(250));
    };
    format!(
        "ok=yes pid={} halvo_window={} workbook={:#x} notes={:#x}",
        std::process::id(),
        yes_no(found),
        workbook.0 as usize,
        notes.0 as usize
    )
}

/// The window whose handle the file `name` of `dir` holds, within 20 s.
fn window_of_file(dir: &Path, name: &str) -> Option<HWND> {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let read = std::fs::read_to_string(dir.join(name))
            .ok()
            .and_then(|text| text.trim().parse::<usize>().ok())
            .map(|value| HWND(value as *mut core::ffi::c_void));
        if let Some(window) = read {
            // SAFETY: IsWindow reads a handle value.
            if unsafe { IsWindow(Some(window)) }.as_bool() {
                return Some(window);
            }
        }
        if Instant::now() >= deadline {
            return None;
        }
        pump(Duration::from_millis(100));
    }
}

/// The return `own`: Halvo's own window gets the front from this process, which holds it,
/// stays in front for `OWN_STAY`, and one click into the workbook's window brings that back.
/// The walk sends no input into Halvo's window. The step counts the foreground events of its
/// own time alone: it first serves the events that came late behind the step before, and
/// forgets them.
fn return_own(workbook: HWND) -> String {
    pump(Duration::from_millis(50));
    clear_seen();
    let refused =
        |events: usize| format!("ok=yes away=refused stay_ms=0 fg_events={events} front=own");
    let Some(app) = app_window() else {
        return refused(0);
    };
    let mut away = "call";
    set_front(app);
    if !wait_front(app, Duration::from_millis(700)) {
        // The system lets the process that gave the last input set the window in front: one
        // pointer move of one pixel and back, over the walk's own window, then the call again.
        away = "move-call";
        send(&[mouse(MOUSEEVENTF_MOVE, 1, 0)]);
        send(&[mouse(MOUSEEVENTF_MOVE, -1, 0)]);
        set_front(app);
        if !wait_front(app, Duration::from_secs(2)) {
            return refused(seen_count());
        }
    }
    let arrived = Instant::now();
    pump(OWN_STAY);
    let stay = arrived.elapsed().as_millis();
    let back = click_into(workbook);
    if back.starts_with("ok=yes") {
        format!(
            "ok=yes away={away} stay_ms={stay} fg_events={} front=own",
            seen_count()
        )
    } else {
        back
    }
}

/// The return `short`: the passer's window gets the front from this process, which holds it,
/// and hands it back at once. Where the front does not come back within 3 s, one click into
/// the workbook's window brings it back and the stay reads 3000. The step counts the foreground
/// events of its own time alone, as the return `own` does: a late event of the step before,
/// or of the try before, would hide an order in which Windows raised none.
fn return_short(workbook: HWND, passer: HWND) -> String {
    pump(Duration::from_millis(50));
    clear_seen();
    set_front(passer);
    let deadline = Instant::now() + Duration::from_secs(3);
    let stay = loop {
        pump(Duration::from_millis(10));
        if let Some(stay) = stay_between(passer, workbook) {
            break Some(stay);
        }
        if Instant::now() >= deadline {
            break None;
        }
    };
    let events = seen_count();
    match stay {
        Some(stay) if wait_front(workbook, Duration::from_millis(500)) => {
            format!("ok=yes stay_ms={stay} fg_events={events} front=own")
        }
        _ => {
            let back = click_into(workbook);
            if back.starts_with("ok=yes") {
                format!("ok=yes stay_ms=3000 fg_events={events} front=own")
            } else {
                back
            }
        }
    }
}

/// The step `look`: the window in front, the system's idle time, and the last window of
/// another program that came in front since the last step.
fn look(workbook: HWND, notes: HWND) -> String {
    let idle = idle_ms();
    let now = front();
    let (class, file) = last_other(&[workbook, notes]);
    let front_text = if now == workbook {
        "front=own".to_string()
    } else {
        format!(
            "front=other front_class={} front_file={}",
            class_of(now),
            file_of(now)
        )
    };
    format!("ok=yes {front_text} idle_ms={idle} last_other_class={class} last_other_file={file}")
}

/// 16 bytes of the keys of std's `RandomState`, as the app's command mints a request id.
fn random_bytes() -> [u8; 16] {
    let mut bytes = [0u8; 16];
    for half in bytes.chunks_mut(8) {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u8(0);
        half.copy_from_slice(&hasher.finish().to_le_bytes());
    }
    bytes
}

/// One read of every page of the workflows from the service of `data_dir`, with the request
/// that the app's command sends: the state, the number of workflows in the state proposed and
/// of all stored ones, and the category, the number of supporting summaries and the trigger of
/// the proposal with the most summaries.
fn read_row(data_dir: &str) -> String {
    let Ok(dir) = DataDir::from_flag(data_dir) else {
        return "state=no_data_dir".to_string();
    };
    let Ok(connector) = system_connector(&dir) else {
        return "state=no_connector".to_string();
    };
    let client = Client::new(connector);
    let role = InterfaceRole::claim();
    let mut items: Vec<WorkflowItem> = Vec::new();
    let mut after: Option<WorkflowId> = None;
    loop {
        let id = RequestId::from_random(random_bytes());
        let Ok(request) = InterfaceRead::workflows(&role, id, after.clone()).to_bytes() else {
            return "state=no_request".to_string();
        };
        let bytes = match client.relay(&request) {
            Ok(bytes) => bytes,
            Err(ClientError::Daemon(reply)) if reply.code() == ErrorCode::StoreUnavailable => {
                return "state=store_unavailable".to_string();
            }
            Err(ClientError::Unreachable { .. }) => return "state=not_running".to_string(),
            Err(_) => return "state=no_answer".to_string(),
        };
        let page = match decode_reply(&bytes).map(|reply| reply.outcome) {
            Ok(Ok(ReplyResult::Workflows(page))) => page,
            _ => return "state=broken_reply".to_string(),
        };
        items.extend(page.workflows);
        match page.next {
            Some(next) if after.as_ref().is_none_or(|before| next > *before) => {
                after = Some(next);
            }
            Some(_) => return "state=broken_reply".to_string(),
            None => break,
        }
    }
    let proposed: Vec<(usize, String, bool)> = items
        .iter()
        .filter_map(|item| match &item.workflow {
            Workflow::Full(found) if found.state == WorkflowState::Proposed => Some((
                item.summaries.len(),
                found.category.as_str().to_string(),
                found.trigger == Trigger::Manual,
            )),
            _ => None,
        })
        .collect();
    let stored = items.len();
    match proposed.iter().max_by_key(|(count, _, _)| *count) {
        Some((count, category, by_hand)) => format!(
            "state=running proposals={} stored={stored} category={category} count={count} by_hand={}",
            proposed.len(),
            yes_no(*by_hand)
        ),
        None => format!("state=running proposals=0 stored={stored} category=- count=0 by_hand=-"),
    }
}

/// The step `row`: the read, tried again for 20 s while the derived store cannot be opened.
fn row(data_dir: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let line = read_row(data_dir);
        if line != "state=store_unavailable" || Instant::now() >= deadline {
            return format!("ok=yes {line}");
        }
        pump(Duration::from_secs(1));
    }
}

/// Writes `text` to `path` whole: to a file beside it first, then by a rename.
fn write_whole(path: &Path, text: &str) {
    let beside = path.with_extension("tmp");
    std::fs::write(&beside, text).expect("a file of the walk's directory can be written");
    std::fs::rename(&beside, path).expect("a file of the walk's directory can be renamed");
}

/// The name of the step `n`, once the check has written its file whole.
fn step_of(dir: &Path, n: u32) -> Option<String> {
    let text = std::fs::read_to_string(dir.join(format!("{n}.go"))).ok()?;
    text.ends_with('\n').then(|| text.trim().to_string())
}

fn workbook(dir: &Path, data_dir: &str) {
    let workbook = make_window("HalvoNoticingWorkbook", WORKBOOK_TITLE, 60, 60);
    let notes = make_window("HalvoNoticingNotes", NOTES_TITLE, 60, 340);
    write_whole(
        &dir.join("workbook.hwnd"),
        &format!("{}\n", workbook.0 as usize),
    );
    // SAFETY: SetWinEventHook takes the event range, no module and a callback with the
    // WINEVENTPROC signature; out-of-context hooks need no module.
    let hook = unsafe {
        SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(on_foreground),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        )
    };
    assert!(
        !hook.is_invalid(),
        "SetWinEventHook: {}",
        windows::core::Error::from_thread()
    );
    println!(
        "walk: role=workbook, a program of the check under the file name EXCEL.EXE, not Microsoft Excel; its window: {WORKBOOK_TITLE}"
    );
    let began = Instant::now();
    let mut passer: Option<HWND> = None;
    let mut next = 1u32;
    while began.elapsed() < WALK_LIMIT && !dir.join("end.go").exists() {
        pump(Duration::from_millis(50));
        let Some(step) = step_of(dir, next) else {
            continue;
        };
        let no_passer = || "ok=no why=no-passer-window".to_string();
        let line = match step.as_str() {
            "ready" => ready(workbook, notes),
            "passer" => {
                passer = window_of_file(dir, "passer.hwnd");
                passer.map_or_else(no_passer, |window| {
                    format!("ok=yes passer={:#x}", window.0 as usize)
                })
            }
            "front" => click_into(workbook),
            "notes" => click_into(notes),
            "clicks" => clicks(workbook),
            "own" => return_own(workbook),
            "short" => passer.map_or_else(no_passer, |window| return_short(workbook, window)),
            "look" => look(workbook, notes),
            "row" => row(data_dir),
            _ => "ok=no why=unknown-step".to_string(),
        };
        clear_seen();
        let line = format!("walk: step={step} {line}");
        println!("{line}");
        write_whole(&dir.join(format!("{next}.done")), &format!("{line}\n"));
        next += 1;
    }
    // SAFETY: `hook` is the valid handle that this thread installed above.
    unsafe {
        let _ = UnhookWinEvent(hook);
    }
}

fn passer(dir: &Path) {
    let workbook = window_of_file(dir, "workbook.hwnd")
        .expect("the workbook's window, whose handle the walk writes at its start");
    let window = make_window("HalvoNoticingPass", PASS_TITLE, 520, 60);
    write_whole(
        &dir.join("passer.hwnd"),
        &format!("{}\n", window.0 as usize),
    );
    println!("walk: role=passer, a second program of the check; its window: {PASS_TITLE}");
    let began = Instant::now();
    let mut checked = Instant::now();
    let mut handed = 0u32;
    let mut msg = MSG::default();
    while began.elapsed() < WALK_LIMIT {
        // SAFETY: `msg` is a live MSG that PeekMessageW fills.
        let got = unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool();
        if got {
            // SAFETY: `msg` is the message that PeekMessageW filled above.
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        // The message that activates the window is served inside the calls above: right
        // behind it, before another message is served, the front goes back to the workbook.
        if ACTIVATED.swap(false, Ordering::SeqCst) {
            set_front(workbook);
            handed += 1;
            println!("walk: role=passer handed the front back, time {handed}");
        }
        if !got {
            if checked.elapsed() >= Duration::from_millis(200) {
                if dir.join("end.go").exists() {
                    break;
                }
                checked = Instant::now();
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
