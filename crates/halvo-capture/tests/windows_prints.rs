#![cfg(target_os = "windows")]
#![deny(clippy::undocumented_unsafe_blocks)]

use core::ffi::c_void;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use windows::core::{Interface, BSTR, GUID, HRESULT, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, E_NOINTERFACE, E_POINTER, HINSTANCE, HWND, LPARAM, LRESULT, S_OK, WPARAM,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::System::Variant::{VARIANT, VT_BOOL};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationCacheRequest, IUIAutomationElement,
    IUIAutomationFocusChangedEventHandler, IUIAutomationFocusChangedEventHandler_Vtbl,
    IUIAutomationPropertyChangedEventHandler, IUIAutomationPropertyChangedEventHandler_Vtbl,
    TreeScope_Element, TreeScope_Subtree, UIA_ClassNamePropertyId, UIA_ControlTypePropertyId,
    UIA_FrameworkIdPropertyId, UIA_IsPasswordPropertyId, UIA_ProcessIdPropertyId, UIA_PROPERTY_ID,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, SetFocus, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_A, VK_MENU,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, EnumWindows, GetClassNameW,
    GetForegroundWindow, GetGUIThreadInfo, GetWindowLongW, GetWindowTextW,
    GetWindowThreadProcessId, PeekMessageW, RegisterClassW, SendMessageW, SetForegroundWindow,
    SetWindowLongW, TranslateMessage, ES_PASSWORD, GUITHREADINFO, GWL_STYLE, MSG, PM_REMOVE,
    WINDOW_EX_STYLE, WINDOW_STYLE, WNDCLASSW, WS_CHILD, WS_OVERLAPPEDWINDOW, WS_TABSTOP,
    WS_VISIBLE,
};

fn say(message: impl AsRef<str>) {
    println!("PRINT {}", message.as_ref());
}

fn unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
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

fn file_of(pid: u32) -> String {
    // SAFETY: OpenProcess takes plain values; the handle is closed below.
    let Ok(handle) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }) else {
        return "unreadable".to_string();
    };
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    // SAFETY: `buf` is a live buffer whose length `len` holds; the call
    // writes the path and its length.
    let read = unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
    };
    // SAFETY: the handle was opened above and is closed once.
    unsafe {
        let _ = CloseHandle(handle);
    }
    if read.is_err() {
        return "unreadable".to_string();
    }
    let path = String::from_utf16_lossy(&buf[..len as usize]);
    path.rsplit(['/', '\\']).next().unwrap_or("").to_string()
}

fn focus_window_class() -> String {
    let mut info = GUITHREADINFO {
        cbSize: core::mem::size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: GetForegroundWindow and GetWindowThreadProcessId read handle
    // values; `info` is a live GUITHREADINFO whose size field is set.
    let focus = unsafe {
        let thread = GetWindowThreadProcessId(GetForegroundWindow(), None);
        if thread == 0 || GetGUIThreadInfo(thread, &mut info).is_err() {
            return "unreadable".to_string();
        }
        info.hwndFocus
    };
    if focus.0.is_null() {
        return "none".to_string();
    }
    let mut class = [0u16; 128];
    // SAFETY: `class` is a live buffer whose length the call takes.
    let len = unsafe { GetClassNameW(focus, &mut class) };
    String::from_utf16_lossy(&class[..usize::try_from(len).unwrap_or(0).min(class.len())])
}

fn text(value: windows::core::Result<BSTR>) -> String {
    match value {
        Ok(value) if !value.is_empty() => value.to_string().replace(' ', "_"),
        Ok(_) => "empty".to_string(),
        Err(_) => "unreadable".to_string(),
    }
}

#[derive(Debug, Clone)]
struct Props {
    framework: String,
    element_class: String,
    window_class: String,
    control_type: i32,
    file: String,
    is_password: i32,
}

impl Props {
    fn line(&self) -> String {
        format!(
            "framework={} element_class={} window_class={} control_type={} file={} is_password={}",
            self.framework,
            self.element_class,
            self.window_class,
            self.control_type,
            self.file,
            self.is_password
        )
    }
}

fn cached_props(element: &IUIAutomationElement) -> Props {
    // SAFETY: the element is live and its cache holds the properties that the
    // cache request of the client names.
    unsafe {
        Props {
            framework: text(element.CachedFrameworkId()),
            element_class: text(element.CachedClassName()),
            window_class: focus_window_class(),
            control_type: element.CachedControlType().map_or(-1, |t| t.0),
            file: element
                .CachedProcessId()
                .ok()
                .and_then(|pid| u32::try_from(pid).ok())
                .map_or("unreadable".to_string(), file_of),
            is_password: element
                .CachedIsPassword()
                .map_or(-1, |b| i32::from(b.as_bool())),
        }
    }
}

fn current_props(element: &IUIAutomationElement) -> Props {
    // SAFETY: the element is live for the calls.
    unsafe {
        Props {
            framework: text(element.CurrentFrameworkId()),
            element_class: text(element.CurrentClassName()),
            window_class: focus_window_class(),
            control_type: element.CurrentControlType().map_or(-1, |t| t.0),
            file: element
                .CurrentProcessId()
                .ok()
                .and_then(|pid| u32::try_from(pid).ok())
                .map_or("unreadable".to_string(), file_of),
            is_password: element
                .CurrentIsPassword()
                .map_or(-1, |b| i32::from(b.as_bool())),
        }
    }
}

enum Note {
    Focus(Props),
    Password(u128, i32, &'static str),
}

#[repr(C)]
struct FocusSink {
    vtable: *const IUIAutomationFocusChangedEventHandler_Vtbl,
    refs: AtomicU32,
    notes: Mutex<mpsc::Sender<Note>>,
}

static FOCUS_VTABLE: IUIAutomationFocusChangedEventHandler_Vtbl =
    IUIAutomationFocusChangedEventHandler_Vtbl {
        base__: windows::core::IUnknown_Vtbl {
            QueryInterface: focus_query,
            AddRef: sink_add_ref::<FocusSink>,
            Release: sink_release::<FocusSink>,
        },
        HandleFocusChangedEvent: focus_event,
    };

#[repr(C)]
struct PropertySink {
    vtable: *const IUIAutomationPropertyChangedEventHandler_Vtbl,
    refs: AtomicU32,
    notes: Mutex<mpsc::Sender<Note>>,
    from: &'static str,
}

static PROPERTY_VTABLE: IUIAutomationPropertyChangedEventHandler_Vtbl =
    IUIAutomationPropertyChangedEventHandler_Vtbl {
        base__: windows::core::IUnknown_Vtbl {
            QueryInterface: property_query,
            AddRef: sink_add_ref::<PropertySink>,
            Release: sink_release::<PropertySink>,
        },
        HandlePropertyChangedEvent: property_event,
    };

trait Counted {
    fn refs(&self) -> &AtomicU32;
}

impl Counted for FocusSink {
    fn refs(&self) -> &AtomicU32 {
        &self.refs
    }
}

impl Counted for PropertySink {
    fn refs(&self) -> &AtomicU32 {
        &self.refs
    }
}

unsafe extern "system" fn sink_add_ref<T: Counted>(this: *mut c_void) -> u32 {
    // SAFETY: `this` is a live sink of type T.
    let sink = unsafe { &*(this as *const T) };
    sink.refs().fetch_add(1, Ordering::Relaxed) + 1
}

unsafe extern "system" fn sink_release<T: Counted>(this: *mut c_void) -> u32 {
    // SAFETY: `this` is a live sink of type T.
    let left = unsafe { &*(this as *const T) }
        .refs()
        .fetch_sub(1, Ordering::AcqRel)
        - 1;
    if left == 0 {
        // SAFETY: the last reference is gone, and the sink was made by
        // Box::new.
        drop(unsafe { Box::from_raw(this as *mut T) });
    }
    left
}

fn query(this: *mut c_void, iid: *const GUID, out: *mut *mut c_void, own: GUID) -> HRESULT {
    if iid.is_null() || out.is_null() {
        return E_POINTER;
    }
    // SAFETY: the system passes a live IID and a live out pointer.
    let wanted = unsafe { *iid };
    if wanted == windows::core::IUnknown::IID || wanted == own {
        // SAFETY: `this` is a live sink whose first field is a vtable with
        // the IUnknown methods first, and `out` is writable.
        unsafe {
            let vtable = *(this as *const *const windows::core::IUnknown_Vtbl);
            ((*vtable).AddRef)(this);
            *out = this;
        }
        S_OK
    } else {
        // SAFETY: `out` is writable.
        unsafe { *out = core::ptr::null_mut() };
        E_NOINTERFACE
    }
}

unsafe extern "system" fn focus_query(
    this: *mut c_void,
    iid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    query(this, iid, out, IUIAutomationFocusChangedEventHandler::IID)
}

unsafe extern "system" fn property_query(
    this: *mut c_void,
    iid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    query(
        this,
        iid,
        out,
        IUIAutomationPropertyChangedEventHandler::IID,
    )
}

unsafe extern "system" fn focus_event(this: *mut c_void, sender: *mut c_void) -> HRESULT {
    // SAFETY: `this` is a live FocusSink, and `sender` a borrowed element or
    // null.
    let (sink, element) = unsafe {
        (
            &*(this as *const FocusSink),
            IUIAutomationElement::from_raw_borrowed(&sender),
        )
    };
    if let Some(element) = element {
        let props = cached_props(element);
        if let Ok(notes) = sink.notes.lock() {
            let _ = notes.send(Note::Focus(props));
        }
    }
    S_OK
}

unsafe extern "system" fn property_event(
    this: *mut c_void,
    _sender: *mut c_void,
    _property: UIA_PROPERTY_ID,
    value: VARIANT,
) -> HRESULT {
    // SAFETY: `this` is a live PropertySink.
    let sink = unsafe { &*(this as *const PropertySink) };
    // SAFETY: the tag names the member of the union that the value holds; the
    // value is the caller's and is not freed here.
    let answer = unsafe {
        let inner = &value.Anonymous.Anonymous;
        if inner.vt == VT_BOOL {
            i32::from(inner.Anonymous.boolVal.0 != 0)
        } else {
            -1
        }
    };
    core::mem::forget(value);
    if let Ok(notes) = sink.notes.lock() {
        let _ = notes.send(Note::Password(unix_ms(), answer, sink.from));
    }
    S_OK
}

fn focus_sink(notes: mpsc::Sender<Note>) -> IUIAutomationFocusChangedEventHandler {
    let raw = Box::into_raw(Box::new(FocusSink {
        vtable: &FOCUS_VTABLE,
        refs: AtomicU32::new(1),
        notes: Mutex::new(notes),
    })) as *mut c_void;
    // SAFETY: `raw` points at a FocusSink whose first field is the vtable of
    // the interface and whose count holds the one reference returned.
    unsafe { IUIAutomationFocusChangedEventHandler::from_raw(raw) }
}

fn property_sink(
    notes: mpsc::Sender<Note>,
    from: &'static str,
) -> IUIAutomationPropertyChangedEventHandler {
    let raw = Box::into_raw(Box::new(PropertySink {
        vtable: &PROPERTY_VTABLE,
        refs: AtomicU32::new(1),
        notes: Mutex::new(notes),
        from,
    })) as *mut c_void;
    // SAFETY: as for the focus sink.
    unsafe { IUIAutomationPropertyChangedEventHandler::from_raw(raw) }
}

struct Client {
    automation: IUIAutomation,
    cache: IUIAutomationCacheRequest,
}

impl Client {
    fn new() -> Client {
        // SAFETY: CoInitializeEx joins this thread to the MTA; a second call
        // on the same thread answers that it is joined already.
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        // SAFETY: CoCreateInstance makes the system's UI Automation object.
        let automation: IUIAutomation =
            unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }
                .expect("the UI Automation client");
        // SAFETY: the calls take the live automation object and property ids.
        let cache = unsafe {
            let cache = automation.CreateCacheRequest().expect("a cache request");
            for property in [
                UIA_FrameworkIdPropertyId,
                UIA_ClassNamePropertyId,
                UIA_ControlTypePropertyId,
                UIA_ProcessIdPropertyId,
                UIA_IsPasswordPropertyId,
            ] {
                let _ = cache.AddProperty(property);
            }
            cache
        };
        Client { automation, cache }
    }

    fn focused(&self) -> Option<IUIAutomationElement> {
        // SAFETY: the automation object is live for the call.
        unsafe { self.automation.GetFocusedElement() }.ok()
    }
}

const NATIVE_ENV: &str = "HALVO_PRINT_NATIVE";
const NATIVE_TEST: &str = "print_helper_native_window";

unsafe extern "system" fn native_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: the arguments are the ones that the system passed.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn pump_for(span: Duration) {
    let end = Instant::now() + span;
    let mut msg = MSG::default();
    while Instant::now() < end {
        // SAFETY: `msg` is a live MSG that PeekMessageW fills, and the same
        // value goes to TranslateMessage and DispatchMessageW.
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
#[ignore = "helper: the native target of the prints; does nothing unless a print starts it"]
fn print_helper_native_window() {
    let Ok(mode) = std::env::var(NATIVE_ENV) else {
        return;
    };
    const EM_SETPASSWORDCHAR: u32 = 0x00CC;
    let class = HSTRING::from("HalvoPrintNative");
    let title = HSTRING::from("halvo-print-native");
    // SAFETY: a null module name asks for the handle of this executable.
    let hinstance: HINSTANCE = unsafe { GetModuleHandleW(None) }
        .expect("the module")
        .into();
    let wnd_class = WNDCLASSW {
        lpfnWndProc: Some(native_wnd_proc),
        hInstance: hinstance,
        lpszClassName: PCWSTR(class.as_ptr()),
        ..Default::default()
    };
    // SAFETY: `wnd_class` points at `class`, which lives to the end.
    let registered = unsafe { RegisterClassW(&wnd_class) };
    assert!(registered != 0, "RegisterClassW");
    let edit = HSTRING::from("EDIT");
    // SAFETY: the class names and the title are live HSTRINGs.
    let (window, plain, password) = unsafe {
        let window = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(class.as_ptr()),
            PCWSTR(title.as_ptr()),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            100,
            100,
            360,
            200,
            None,
            None,
            Some(hinstance),
            None,
        )
        .expect("the window");
        let child = |y: i32, style: WINDOW_STYLE| {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                PCWSTR(edit.as_ptr()),
                PCWSTR::null(),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | style,
                10,
                y,
                300,
                24,
                Some(window),
                None,
                Some(hinstance),
                None,
            )
            .expect("an edit")
        };
        (
            window,
            child(10, WINDOW_STYLE(0)),
            child(44, WINDOW_STYLE(ES_PASSWORD as u32)),
        )
    };
    pump_for(Duration::from_millis(200));
    send(&[key_input(VK_MENU, false)]);
    // SAFETY: SetForegroundWindow and SetFocus take this thread's handles.
    unsafe {
        let _ = SetForegroundWindow(window);
    }
    send(&[key_input(VK_MENU, true)]);
    let target = if mode.starts_with("password") {
        password
    } else {
        plain
    };
    // SAFETY: as above.
    unsafe {
        let _ = SetFocus(Some(target));
    }
    println!("PRINTC ready");
    if let Some(hold) = mode.strip_prefix("swap-") {
        let hold: u64 = hold.parse().unwrap_or(0);
        pump_for(Duration::from_millis(1500));
        // SAFETY: the calls take this thread's edit control and plain values.
        unsafe {
            let style = GetWindowLongW(plain, GWL_STYLE);
            let _ = SetWindowLongW(plain, GWL_STYLE, style | ES_PASSWORD);
            let _ = SendMessageW(plain, EM_SETPASSWORDCHAR, Some(WPARAM(0x25CF)), None);
        }
        println!("PRINTC swap at={}", unix_ms());
        std::thread::sleep(Duration::from_millis(hold));
        pump_for(Duration::from_millis(2000));
        // SAFETY: as above.
        unsafe {
            let style = GetWindowLongW(plain, GWL_STYLE);
            let _ = SetWindowLongW(plain, GWL_STYLE, style & !ES_PASSWORD);
            let _ = SendMessageW(plain, EM_SETPASSWORDCHAR, Some(WPARAM(0)), None);
        }
        println!("PRINTC back at={}", unix_ms());
        std::thread::sleep(Duration::from_millis(hold));
    }
    pump_for(Duration::from_secs(4));
}

fn native_target(mode: &str) -> (Child, mpsc::Receiver<String>) {
    let exe = std::env::current_exe().expect("the test executable");
    let mut child = Command::new(exe)
        .args([
            "--ignored",
            "--exact",
            NATIVE_TEST,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(NATIVE_ENV, mode)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("the native target");
    let stdout = child.stdout.take().expect("its stdout");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some(at) = line.find("PRINTC ") {
                let _ = tx.send(line[at + 7..].to_string());
            }
        }
    });
    (child, rx)
}

const PAGE: &str = r#"<!doctype html>
<html><head><meta charset="utf-8"><title>halvo-prints</title></head>
<body>
<input id="plain" type="text">
<input id="password" type="password">
<script>
const query = new URLSearchParams(location.search);
const field = query.get('field') || 'plain';
const hold = Number(query.get('hold') || '0');
const swap = query.get('swap') === '1';
const marks = { swap: 0, back: 0 };
const busy = (ms) => { const end = Date.now() + ms; while (Date.now() < end) {} };
const show = () => {
  document.title = 'halvo-prints ' + query.get('run') + ' swap=' + marks.swap + ' back=' + marks.back + ' end';
};
document.getElementById(field).focus();
if (swap) {
  setTimeout(() => {
    marks.swap = Date.now();
    document.getElementById('plain').type = 'password';
    show();
    busy(hold);
  }, 1500);
  setTimeout(() => {
    marks.back = Date.now();
    document.getElementById('plain').type = 'text';
    show();
    busy(hold);
  }, 3500);
}
show();
</script>
</body></html>
"#;

struct Browser {
    name: &'static str,
    paths: &'static [&'static str],
    chromium: bool,
}

const BROWSERS: [Browser; 3] = [
    Browser {
        name: "chrome",
        paths: &[
            r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
        ],
        chromium: true,
    },
    Browser {
        name: "edge",
        paths: &[
            r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
            r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
        ],
        chromium: true,
    },
    Browser {
        name: "firefox",
        paths: &[
            r"C:\Program Files\Mozilla Firefox\firefox.exe",
            r"C:\Program Files (x86)\Mozilla Firefox\firefox.exe",
        ],
        chromium: false,
    },
];

fn window_titled(prefix: &str) -> Option<HWND> {
    unsafe extern "system" fn each(hwnd: HWND, found: LPARAM) -> windows::core::BOOL {
        let mut buf = [0u16; 512];
        // SAFETY: `buf` is a live buffer; `found` is the address of the
        // vector that the caller holds across the enumeration on this thread.
        unsafe {
            let len = GetWindowTextW(hwnd, &mut buf);
            let title = String::from_utf16_lossy(&buf[..usize::try_from(len).unwrap_or(0)]);
            (*(found.0 as *mut Vec<(HWND, String)>)).push((hwnd, title));
        }
        windows::core::BOOL::from(true)
    }
    let mut all: Vec<(HWND, String)> = Vec::new();
    // SAFETY: the callback writes `all` through a pointer made from an
    // exclusive borrow, only while this enumeration runs on this thread.
    unsafe {
        let _ = EnumWindows(
            Some(each),
            LPARAM(&mut all as *mut Vec<(HWND, String)> as isize),
        );
    }
    all.into_iter()
        .find(|(_, title)| title.starts_with(prefix))
        .map(|(hwnd, _)| hwnd)
}

fn title_number(hwnd: HWND, key: &str) -> Option<u128> {
    let mut buf = [0u16; 512];
    // SAFETY: `buf` is a live buffer whose length the call takes.
    let len = unsafe { GetWindowTextW(hwnd, &mut buf) };
    let title = String::from_utf16_lossy(&buf[..usize::try_from(len).unwrap_or(0)]);
    let at = title.find(key)? + key.len();
    title[at..].split(' ').next()?.parse().ok()
}

fn kill_all(file: &str) {
    let _ = Command::new("taskkill.exe")
        .args(["/IM", file, "/F", "/T"])
        .output();
    std::thread::sleep(Duration::from_millis(700));
}

struct Launch {
    profile: tempfile::TempDir,
    page: String,
}

impl Launch {
    fn new() -> Launch {
        let profile = tempfile::tempdir().expect("a profile directory");
        let prefs = "user_pref(\"browser.shell.checkDefaultBrowser\", false);\n\
                     user_pref(\"browser.aboutwelcome.enabled\", false);\n\
                     user_pref(\"datareporting.policy.dataSubmissionEnabled\", false);\n\
                     user_pref(\"browser.startup.homepage_override.mstone\", \"ignore\");\n\
                     user_pref(\"trailhead.firstrun.didSeeAboutWelcome\", true);\n";
        let _ = std::fs::write(profile.path().join("user.js"), prefs);
        let page_path = profile.path().join("prints.html");
        std::fs::write(&page_path, PAGE).expect("the page");
        let page = format!("file:///{}", page_path.to_string_lossy().replace('\\', "/"));
        Launch { profile, page }
    }

    fn open(&self, exe: &Path, chromium: bool, query: &str, run: u32) -> Option<HWND> {
        let url = format!("{}?{query}&run={run}", self.page);
        let profile = self.profile.path().to_string_lossy().into_owned();
        let args: Vec<String> = if chromium {
            vec![
                format!("--user-data-dir={profile}"),
                "--no-first-run".to_string(),
                "--no-default-browser-check".to_string(),
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
        Command::new(exe)
            .args(&args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let prefix = format!("halvo-prints {run} ");
        let began = Instant::now();
        while began.elapsed() < Duration::from_secs(30) {
            if let Some(hwnd) = window_titled(&prefix) {
                send(&[key_input(VK_MENU, false)]);
                // SAFETY: SetForegroundWindow takes a handle value.
                unsafe {
                    let _ = SetForegroundWindow(hwnd);
                }
                send(&[key_input(VK_MENU, true)]);
                return Some(hwnd);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        None
    }
}

fn renamed_firefox(into: &Path) -> Option<PathBuf> {
    let source = Path::new(BROWSERS[2].paths.iter().find(|p| Path::new(p).is_file())?).parent()?;
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
    let renamed = into.join("renamed-browser.exe");
    std::fs::rename(into.join("firefox.exe"), &renamed).ok()?;
    Some(renamed)
}

fn cold_and_warm(target: &str, field: &str, launch: u32, file: &str, open: impl FnOnce() -> bool) {
    let client = Client::new();
    let (tx, rx) = mpsc::channel();
    let sink = focus_sink(tx);
    // SAFETY: the cache request and the sink are live COM objects.
    let added = unsafe {
        client
            .automation
            .AddFocusChangedEventHandler(&client.cache, &sink)
    };
    let opened = open();
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut cold: Option<Props> = None;
    let mut seen = 0u32;
    while cold.is_none() && Instant::now() < deadline {
        if let Ok(Note::Focus(props)) = rx.recv_timeout(Duration::from_millis(200)) {
            seen += 1;
            if props.file.eq_ignore_ascii_case(file) {
                cold = Some(props);
            }
        }
    }
    std::thread::sleep(Duration::from_millis(1500));
    let warm = client.focused().map(|element| current_props(&element));
    // SAFETY: the handler is the one added above.
    let _ = unsafe { client.automation.RemoveFocusChangedEventHandler(&sink) };
    say(format!(
        "q3 target={target} field={field} launch={launch} handler_added={} opened={} focus_events_before_the_target={} when=cold {}",
        u8::from(added.is_ok()),
        u8::from(opened),
        seen.saturating_sub(u32::from(cold.is_some())),
        cold.map_or("none=1".to_string(), |props| props.line()),
    ));
    say(format!(
        "q3 target={target} field={field} launch={launch} when=warm {}",
        warm.map_or("none=1".to_string(), |props| props.line()),
    ));
}

#[test]
#[ignore = "print: needs an interactive Windows desktop and the browsers of the hosted runner"]
fn print_q3_the_kinds_of_the_fixture_targets() {
    const LAUNCHES: u32 = 10;
    let copy_dir = tempfile::tempdir().expect("a directory for the renamed copy");
    let renamed = renamed_firefox(copy_dir.path());
    say(format!(
        "q3 renamed_copy_made={}",
        u8::from(renamed.is_some())
    ));
    let exe_name = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_default();
    for field in ["plain", "password"] {
        for launch in 0..LAUNCHES {
            cold_and_warm("native", field, launch, &exe_name, || {
                let (mut child, lines) = native_target(field);
                let ready = lines.recv_timeout(Duration::from_secs(20)).is_ok();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_secs(5));
                    let _ = child.kill();
                    let _ = child.wait();
                });
                ready
            });
        }
    }
    let mut targets: Vec<(&str, PathBuf, bool)> = BROWSERS
        .iter()
        .filter_map(|b| {
            let path = b.paths.iter().find(|p| Path::new(p).is_file())?;
            Some((b.name, PathBuf::from(path), b.chromium))
        })
        .collect();
    if let Some(renamed) = renamed {
        targets.push(("renamed-firefox", renamed, false));
    }
    let mut run = 0;
    for (name, exe, chromium) in targets {
        let file = exe
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        say(format!("q3 target={name} installed=1"));
        for field in ["plain", "password"] {
            for launch in 0..LAUNCHES {
                run += 1;
                let browser = Launch::new();
                cold_and_warm(name, field, launch, &file, || {
                    browser
                        .open(&exe, chromium, &format!("field={field}"), run)
                        .is_some()
                });
                kill_all(&file);
            }
        }
    }
}

fn first_reading(
    client: &Client,
    wanted: bool,
    until: Instant,
) -> (Option<u128>, Option<IUIAutomationElement>) {
    while Instant::now() < until {
        if let Some(element) = client.focused() {
            // SAFETY: the element is live for the call.
            let answer = unsafe { element.CurrentIsPassword() }.map(|b| b.as_bool());
            if answer == Ok(wanted) {
                return (Some(unix_ms()), Some(element));
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    (None, None)
}

fn delay(at: Option<u128>, from: Option<u128>) -> String {
    match (at, from) {
        (Some(at), Some(from)) => (at as i128 - from as i128).to_string(),
        _ => "none".to_string(),
    }
}

type SwapMarks = Box<dyn Fn() -> (Option<u128>, Option<u128>)>;

fn one_swap(target: &str, hold: u64, round: u32, open: impl FnOnce() -> Option<SwapMarks>) {
    let client = Client::new();
    let Some(marks) = open() else {
        say(format!(
            "q2g target={target} hold_ms={hold} round={round} opened=0"
        ));
        return;
    };
    std::thread::sleep(Duration::from_millis(400));
    let before = client.focused();
    let (tx, rx) = mpsc::channel();
    let on_element = property_sink(tx.clone(), "element");
    let on_root = property_sink(tx, "root");
    // SAFETY: the elements, the cache and the sinks are live COM objects.
    let (element_added, root_added) = unsafe {
        let element_added = before.as_ref().map(|element| {
            client
                .automation
                .AddPropertyChangedEventHandlerNativeArray(
                    element,
                    TreeScope_Element,
                    None,
                    &on_element,
                    &[UIA_IsPasswordPropertyId],
                )
                .is_ok()
        });
        let root_added = client.automation.GetRootElement().ok().map(|root| {
            client
                .automation
                .AddPropertyChangedEventHandlerNativeArray(
                    &root,
                    TreeScope_Subtree,
                    None,
                    &on_root,
                    &[UIA_IsPasswordPropertyId],
                )
                .is_ok()
        });
        (element_added, root_added)
    };
    let until = Instant::now() + Duration::from_secs(4);
    let (password_at, after) = first_reading(&client, true, until);
    let (text_at, _) = first_reading(&client, false, Instant::now() + Duration::from_secs(4));
    std::thread::sleep(Duration::from_millis(500));
    let (swap, back) = marks();
    let same = match (before.as_ref(), after.as_ref()) {
        // SAFETY: both elements are live for the call.
        (Some(a), Some(b)) => unsafe { client.automation.CompareElements(a, b) }
            .map_or(-1, |same| i32::from(same.as_bool())),
        _ => -1,
    };
    let mut events = Vec::new();
    while let Ok(note) = rx.try_recv() {
        if let Note::Password(at, value, from) = note {
            events.push(format!(
                "{from}:{value}:{}",
                delay(Some(at), if value == 1 { swap } else { back })
            ));
        }
    }
    // SAFETY: the client is live; the call removes every handler it added.
    let _ = unsafe { client.automation.RemoveAllEventHandlers() };
    say(format!(
        "q2g target={target} hold_ms={hold} round={round} opened=1 element_handler_added={} root_handler_added={} \
         swap_stamped={} first_password_read_after_swap_ms={} first_text_read_after_back_ms={} \
         element_after_swap_compares_equal={same} property_events={} events=[{}]",
        element_added.map_or(-1, i32::from),
        root_added.map_or(-1, i32::from),
        u8::from(swap.is_some()),
        delay(password_at, swap),
        delay(text_at, back),
        events.len(),
        events.join(","),
    ));
}

#[test]
#[ignore = "print: needs an interactive Windows desktop and the browsers of the hosted runner"]
fn print_q2g_a_field_that_becomes_a_password_field_while_it_has_the_focus() {
    const ROUNDS: u32 = 3;
    for hold in [0u64, 150, 300] {
        for round in 0..ROUNDS {
            one_swap("native", hold, round, || {
                let (mut child, lines) = native_target(&format!("swap-{hold}"));
                lines.recv_timeout(Duration::from_secs(20)).ok()?;
                let lines = Mutex::new(lines);
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_secs(12));
                    let _ = child.kill();
                    let _ = child.wait();
                });
                Some(Box::new(move || {
                    let lines = lines.lock().expect("the lines");
                    let mut swap = None;
                    let mut back = None;
                    while let Ok(line) = lines.try_recv() {
                        let at = line
                            .split("at=")
                            .nth(1)
                            .and_then(|v| v.trim().parse::<u128>().ok());
                        if line.starts_with("swap") {
                            swap = at;
                        } else if line.starts_with("back") {
                            back = at;
                        }
                    }
                    (swap, back)
                }))
            });
        }
    }
    let mut run = 1000;
    for browser in BROWSERS.iter().filter(|b| b.chromium) {
        let Some(path) = browser.paths.iter().find(|p| Path::new(p).is_file()) else {
            say(format!("q2g target={} installed=0", browser.name));
            continue;
        };
        let exe = PathBuf::from(path);
        let file = exe
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        for hold in [0u64, 150, 300] {
            for round in 0..ROUNDS {
                run += 1;
                let launch = Launch::new();
                one_swap(browser.name, hold, round, || {
                    let hwnd =
                        launch.open(&exe, true, &format!("field=plain&swap=1&hold={hold}"), run)?;
                    Some(Box::new(move || {
                        (title_number(hwnd, " swap="), title_number(hwnd, " back="))
                    }))
                });
                kill_all(&file);
            }
        }
    }
}

fn powershell_number(script: &str) -> f64 {
    Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", script])
        .output()
        .ok()
        .and_then(|out| String::from_utf8_lossy(&out.stdout).trim().parse().ok())
        .unwrap_or(-1.0)
}

#[test]
#[ignore = "print: needs an interactive Windows desktop and the browsers of the hosted runner"]
fn print_q2g_the_cost_of_one_read_for_every_key() {
    let mut run = 2000;
    for browser in BROWSERS.iter().filter(|b| b.chromium) {
        let Some(path) = browser.paths.iter().find(|p| Path::new(p).is_file()) else {
            continue;
        };
        let exe = PathBuf::from(path);
        let file = exe
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let process = file.trim_end_matches(".exe").to_string();
        for reads in [false, true] {
            run += 1;
            let launch = Launch::new();
            let opened = launch.open(&exe, true, "field=plain", run).is_some();
            std::thread::sleep(Duration::from_secs(3));
            let client = Client::new();
            let cpu = || {
                powershell_number(&format!(
                    "(Get-Process {process} -ErrorAction SilentlyContinue | Measure-Object -Property CPU -Sum).Sum"
                ))
            };
            let memory = || {
                powershell_number(&format!(
                    "(Get-Process {process} -ErrorAction SilentlyContinue | Measure-Object -Property WorkingSet64 -Sum).Sum"
                ))
            };
            let cpu_before = cpu();
            let memory_before = memory();
            let began = Instant::now();
            let mut keys = 0u32;
            let mut read_ms_total = 0u128;
            while began.elapsed() < Duration::from_secs(60) {
                send(&[key_input(VK_A, false), key_input(VK_A, true)]);
                keys += 1;
                if reads {
                    let at = Instant::now();
                    if let Some(element) = client.focused() {
                        // SAFETY: the element is live for the call.
                        let _ = unsafe { element.CurrentIsPassword() };
                    }
                    read_ms_total += at.elapsed().as_millis();
                }
                std::thread::sleep(Duration::from_millis(60));
            }
            let cpu_after = cpu();
            let memory_after = memory();
            say(format!(
                "q2g-cost target={} one_read_for_every_key={} opened={} keys={keys} \
                 cpu_seconds_used={:.2} working_set_mb_before={:.1} working_set_mb_after={:.1} \
                 mean_read_ms={:.1}",
                browser.name,
                u8::from(reads),
                u8::from(opened),
                cpu_after - cpu_before,
                memory_before / 1_048_576.0,
                memory_after / 1_048_576.0,
                if reads && keys > 0 {
                    read_ms_total as f64 / f64::from(keys)
                } else {
                    0.0
                },
            ));
            kill_all(&file);
        }
    }
}
