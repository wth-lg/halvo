#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Window {
    pub id: u64,
    pub pid: u32,
    pub owner: String,
}

pub const PROMPT_OWNERS: [&str; 6] = [
    "SecurityAgent",
    "coreautha",
    "UserNotificationCenter",
    "universalAccessAuthWarn",
    "consent.exe",
    "CredentialUIBroker.exe",
];

/// Changes of the front application: `prompt` for the browser or a prompt, `other` counts the rest.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FrontSeen {
    first: Option<String>,
    last: Option<String>,
    pub prompt: bool,
    pub other: u32,
}

impl FrontSeen {
    /// Whether `token` differs from the last token that `observe` took.
    pub fn is_new(&self, token: &str) -> bool {
        self.last.as_deref() != Some(token)
    }

    /// Takes one reading: a change to any token but the first sets `prompt` or adds one to `other`.
    pub fn observe(&mut self, token: &str, pid: Option<u32>, owner: &str, browser: &[u32]) {
        if self.first.is_none() {
            self.first = Some(token.to_string());
        }
        if !self.is_new(token) {
            return;
        }
        self.last = Some(token.to_string());
        if self.first.as_deref() == Some(token) {
            return;
        }
        if pid.is_some_and(|p| browser.contains(&p)) || PROMPT_OWNERS.contains(&owner) {
            self.prompt = true;
        } else {
            self.other += 1;
        }
    }
}

/// The number after the first `pid` that `=` and digits follow, past spaces and quotes: `"pid"=42`.
pub fn pid_in(text: &str) -> Option<u32> {
    let mut rest = text;
    while let Some(i) = rest.find("pid") {
        rest = &rest[i + 3..];
        let after = rest.trim_start_matches([' ', '"']);
        if let Some(value) = after.strip_prefix('=') {
            let digits: String = value
                .trim_start_matches([' ', '"'])
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if let Ok(p) = digits.parse() {
                return Some(p);
            }
        }
    }
    None
}

pub fn raised(before: &[Window], after: &[Window], browser: &[u32]) -> Vec<Window> {
    after
        .iter()
        .filter(|w| !before.iter().any(|b| b.id == w.id && b.pid == w.pid))
        .filter(|w| browser.contains(&w.pid) || PROMPT_OWNERS.contains(&w.owner.as_str()))
        .cloned()
        .collect()
}

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::{c_char, c_void, CStr, CString};

    use super::Window;

    const UTF8: u32 = 0x0800_0100;
    const SINT64: isize = 4;
    const ON_SCREEN_ONLY: u32 = 1;
    const EXCLUDE_DESKTOP: u32 = 16;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGWindowListCopyWindowInfo(option: u32, relative_to: u32) -> *const c_void;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFArrayGetCount(array: *const c_void) -> isize;
        fn CFArrayGetValueAtIndex(array: *const c_void, index: isize) -> *const c_void;
        fn CFDictionaryGetValue(dict: *const c_void, key: *const c_void) -> *const c_void;
        fn CFStringCreateWithCString(
            alloc: *const c_void,
            text: *const c_char,
            encoding: u32,
        ) -> *const c_void;
        fn CFStringGetCString(
            text: *const c_void,
            buffer: *mut c_char,
            size: isize,
            encoding: u32,
        ) -> u8;
        fn CFNumberGetValue(number: *const c_void, kind: isize, out: *mut c_void) -> u8;
        fn CFRelease(object: *const c_void);
    }

    struct Key(*const c_void);

    impl Key {
        fn new(name: &str) -> Key {
            let c = CString::new(name).unwrap_or_default();
            // SAFETY: `c` is a live NUL-terminated string; the null allocator is the default one, and
            // the new string that the call returns, or null, is released by the Key's drop.
            Key(unsafe { CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), UTF8) })
        }
    }

    impl Drop for Key {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: the string was created by CFStringCreateWithCString in `Key::new`, is owned
                // by this Key alone and is released once, here.
                unsafe { CFRelease(self.0) };
            }
        }
    }

    fn number(dict: *const c_void, key: &Key) -> Option<i64> {
        // SAFETY: `dict` is a dictionary of the window list that `windows` holds until its release,
        // and the call only reads the key; the value that it returns is borrowed and not released.
        let v = unsafe { CFDictionaryGetValue(dict, key.0) };
        if v.is_null() {
            return None;
        }
        let mut out = 0i64;
        // SAFETY: `v` is a non-null value of a number key of the window list (a CFNumber), and the
        // type SINT64 writes eight bytes into `out`, a live i64.
        let ok = unsafe { CFNumberGetValue(v, SINT64, (&mut out as *mut i64).cast()) };
        (ok != 0).then_some(out)
    }

    fn text(dict: *const c_void, key: &Key) -> String {
        // SAFETY: as in `number`: a dictionary that `windows` holds, a key that the call only reads,
        // and a borrowed value.
        let v = unsafe { CFDictionaryGetValue(dict, key.0) };
        if v.is_null() {
            return String::new();
        }
        let mut buf = vec![0 as c_char; 512];
        // SAFETY: `v` is a non-null value of the owner-name key (a CFString), and the call writes at
        // most `buf.len()` bytes, the size that it is told, into `buf`, a live buffer.
        let ok = unsafe { CFStringGetCString(v, buf.as_mut_ptr(), buf.len() as isize, UTF8) };
        if ok == 0 {
            return String::new();
        }
        // SAFETY: the call above returned true, so it wrote a NUL-terminated string inside `buf`.
        unsafe { CStr::from_ptr(buf.as_ptr()) }
            .to_string_lossy()
            .into_owned()
    }

    pub fn windows() -> Option<Vec<Window>> {
        // SAFETY: the call takes plain values and returns a new array or null, which is checked; the
        // array is owned here and released below.
        let list = unsafe { CGWindowListCopyWindowInfo(ON_SCREEN_ONLY | EXCLUDE_DESKTOP, 0) };
        if list.is_null() {
            return None;
        }
        let pid_key = Key::new("kCGWindowOwnerPID");
        let owner_key = Key::new("kCGWindowOwnerName");
        let number_key = Key::new("kCGWindowNumber");
        let mut out = Vec::new();
        // SAFETY: `list` is the non-null array copied above and not yet released.
        let n = unsafe { CFArrayGetCount(list) };
        for i in 0..n {
            // SAFETY: `i` is below the count of `list`, which is not yet released; the value is
            // borrowed and not released.
            let dict = unsafe { CFArrayGetValueAtIndex(list, i) };
            if dict.is_null() {
                continue;
            }
            out.push(Window {
                id: number(dict, &number_key).unwrap_or(-1) as u64,
                pid: number(dict, &pid_key).unwrap_or(-1) as u32,
                owner: text(dict, &owner_key),
            });
        }
        // SAFETY: `list` was copied above, so it is owned here; it is released once and not used after.
        unsafe { CFRelease(list) };
        Some(out)
    }

    pub fn front() -> Option<String> {
        let out = std::process::Command::new("lsappinfo")
            .arg("front")
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// The pid of the application that a token of `front` names, read with `lsappinfo info`.
    pub fn front_pid(token: &str) -> Option<u32> {
        let out = std::process::Command::new("lsappinfo")
            .args(["info", "-only", "pid", token])
            .output()
            .ok()?;
        super::pid_in(&String::from_utf8_lossy(&out.stdout))
    }
}

#[cfg(windows)]
mod win {
    use windows_sys::core::BOOL;
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetForegroundWindow, GetWindowThreadProcessId, IsWindowVisible,
    };

    use super::Window;

    unsafe extern "system" fn collect(hwnd: HWND, data: LPARAM) -> BOOL {
        let list = &mut *(data as *mut Vec<(u64, u32)>);
        if IsWindowVisible(hwnd) != 0 {
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, &mut pid);
            list.push((hwnd as u64, pid));
        }
        1
    }

    fn names() -> Vec<(u32, String)> {
        r3_web::procs::table()
            .unwrap_or_default()
            .into_iter()
            .map(|p| {
                (
                    p.pid,
                    p.exe
                        .map(|e| e.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                )
            })
            .collect()
    }

    pub fn windows() -> Option<Vec<Window>> {
        let mut list: Vec<(u64, u32)> = Vec::new();
        // SAFETY: `collect` reads its LPARAM as the Vec<(u64, u32)> that `list` is, which lives on
        // this frame until EnumWindows returns, and EnumWindows calls it on this thread alone.
        let ok =
            unsafe { EnumWindows(Some(collect), (&mut list as *mut Vec<(u64, u32)>) as LPARAM) };
        if ok == 0 {
            return None;
        }
        let names = names();
        Some(
            list.into_iter()
                .map(|(id, pid)| Window {
                    id,
                    pid,
                    owner: names
                        .iter()
                        .find(|(p, _)| *p == pid)
                        .map(|(_, n)| n.clone())
                        .unwrap_or_default(),
                })
                .collect(),
        )
    }

    pub fn front() -> Option<String> {
        // SAFETY: the call takes no argument and returns a window handle or null, which is checked.
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.is_null() {
            return Some("none".into());
        }
        let mut pid = 0u32;
        // SAFETY: the call only reads `hwnd` (a window closed since gives 0) and writes `pid`, a live
        // u32.
        unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        Some(format!("{} {}", hwnd as u64, pid))
    }

    /// The pid in a token of `front`, which is `<window> <pid>`.
    pub fn front_pid(token: &str) -> Option<u32> {
        token.split(' ').nth(1)?.parse().ok()
    }
}

#[cfg(target_os = "macos")]
pub use mac::{front, front_pid, windows};
#[cfg(windows)]
pub use win::{front, front_pid, windows};

#[cfg(not(any(target_os = "macos", windows)))]
pub fn windows() -> Option<Vec<Window>> {
    None
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn front() -> Option<String> {
    None
}

/// On a system other than macOS and Windows no front application is read: always `None`.
#[cfg(not(any(target_os = "macos", windows)))]
pub fn front_pid(_token: &str) -> Option<u32> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(id: u64, pid: u32, owner: &str) -> Window {
        Window {
            id,
            pid,
            owner: owner.into(),
        }
    }

    #[test]
    fn raised_takes_new_windows_of_the_browser_or_of_a_prompt() {
        let before = vec![w(1, 10, "Finder"), w(2, 11, "SecurityAgent")];
        let after = vec![
            w(1, 10, "Finder"),
            w(2, 11, "SecurityAgent"),
            w(3, 12, "Mail"),
            w(4, 50, "Google Chrome"),
            w(5, 13, "SecurityAgent"),
        ];
        assert_eq!(
            raised(&before, &after, &[50]),
            vec![w(4, 50, "Google Chrome"), w(5, 13, "SecurityAgent")]
        );
        assert!(raised(&after, &after, &[50]).is_empty());
    }

    fn replay(steps: &[(&str, Option<u32>, &str)], browser: &[u32]) -> FrontSeen {
        let mut seen = FrontSeen::default();
        for (token, pid, owner) in steps {
            seen.observe(token, *pid, owner, browser);
        }
        seen
    }

    #[test]
    fn a_front_change_counts_as_a_prompt_only_for_the_browser_or_a_prompt_process() {
        let other = replay(
            &[
                ("ASN:0x0-0x1:", Some(100), "Finder"),
                ("ASN:0x0-0x1:", Some(100), "Finder"),
                ("ASN:0x0-0x2:", Some(200), "Mail"),
                ("ASN:0x0-0x1:", Some(100), "Finder"),
            ],
            &[500, 501],
        );
        assert!(!other.prompt);
        assert_eq!(other.other, 1);
        let prompt = replay(
            &[
                ("ASN:0x0-0x1:", Some(100), "Finder"),
                ("ASN:0x0-0x3:", Some(300), "SecurityAgent"),
            ],
            &[500, 501],
        );
        assert!(prompt.prompt);
        assert_eq!(prompt.other, 0);
        let browser = replay(
            &[
                ("ASN:0x0-0x1:", Some(100), "Finder"),
                ("ASN:0x0-0x4:", Some(501), "Google Chrome Helper"),
            ],
            &[500, 501],
        );
        assert!(browser.prompt);
    }

    #[test]
    fn pid_in_reads_the_pid_field() {
        assert_eq!(
            pid_in(
                "\"X\" ASN:0x0-0x1:\n    bundleID=\"com.example.x\" pid = 1303 type=\"Foreground\""
            ),
            Some(1303)
        );
        assert_eq!(pid_in("\"pid\"=42"), Some(42));
        assert_eq!(pid_in("rapid path, no field"), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_window_list_reads_without_a_prompt() {
        let list = windows().expect("window list");
        assert!(list.iter().all(|w| w.id > 0));
        let token = front().expect("front");
        assert!(front_pid(&token).is_some());
    }
}
