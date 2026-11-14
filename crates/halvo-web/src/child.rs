use std::path::{Path, PathBuf};

pub const VAR: &str = "HALVO_WEB_CHILD";
pub const ENTRY: &str = "child::entry";

/// The child that a test starts on the pipe and that holds it for up to 10 s: on Windows the
/// start adds `--remote-debugging-io-pipes=…` to its arguments, which a system program refuses at
/// once (runs 43474813792 and 43498989750: `PING.EXE` ended before the test's read), so the child
/// is this test binary running `pipe_child_entry` alone, with the start's argument after `--`,
/// where the harness takes it as a filter that names no test. On macOS the start places the pipe
/// at descriptors 3 and 4 and adds no argument, and the child is `/bin/sleep`.
pub fn pipe_child() -> (PathBuf, Vec<std::ffi::OsString>) {
    if cfg!(windows) {
        (
            std::env::current_exe().unwrap(),
            vec![
                "--exact".into(),
                "--test-threads=1".into(),
                "--quiet".into(),
                "child::pipe_child_entry".into(),
                "--".into(),
            ],
        )
    } else {
        (PathBuf::from("/bin/sleep"), vec!["10".into()])
    }
}

/// The arguments that run this test binary as the entry `entry` alone, with `extra` after `--`,
/// where the harness takes them as filters that name no test.
pub fn entry_args(entry: &str, extra: &[String]) -> Vec<std::ffi::OsString> {
    let mut v: Vec<std::ffi::OsString> = ["--exact", "--test-threads=1", "--quiet", entry, "--"]
        .iter()
        .map(|a| a.into())
        .collect();
    v.extend(extra.iter().map(|a| a.into()));
    v
}

/// A stand-in browser of the fenced test of CP-189 G7: with `--hold-parent` it starts this test
/// binary as `hold_child_entry`, a child that names no profile, and runs for up to 60 s; in a run
/// of the tests it returns at once.
#[test]
fn hold_parent_entry() {
    use crate::start::Start;
    if !std::env::args().any(|a| a == "--hold-parent") {
        return;
    }
    let me = std::env::current_exe().unwrap();
    let _child = std::process::Command::new(&me)
        .args(entry_args(
            "child::hold_child_entry",
            &["--hold-child".to_string()],
        ))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .start();
    std::thread::sleep(std::time::Duration::from_secs(60));
}

/// The child of `hold_parent_entry`, and the stand-in executor of the same test: with
/// `--hold-child` it runs for up to 60 s; in a run of the tests it returns at once.
#[test]
fn hold_child_entry() {
    if std::env::args().any(|a| a == "--hold-child") {
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
}

/// The child of `pipe_child` on Windows: with the start's pipe argument it holds the pipe's ends
/// for up to 10 s; in a run of the tests, with no such argument, it returns at once.
#[test]
fn pipe_child_entry() {
    if std::env::args().any(|a| a.starts_with("--remote-debugging-io-pipes=")) {
        std::thread::sleep(std::time::Duration::from_secs(10));
    }
}

pub fn scratch(name: &str) -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let target = exe.ancestors().nth(3).unwrap().to_path_buf();
    let dir = target
        .join("tmp")
        .join(format!("halvo-web-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn report_args(out: &Path) -> Vec<std::ffi::OsString> {
    vec![
        ENTRY.into(),
        "--exact".into(),
        "--quiet".into(),
        "--test-threads=1".into(),
        "--".into(),
        out.as_os_str().to_owned(),
    ]
}

pub struct Report {
    pub names: Vec<String>,
    pub values: Vec<String>,
    pub dir: String,
    pub null: [bool; 3],
}

pub fn read_report(out: &Path) -> Report {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let text = loop {
        if let Ok(t) = std::fs::read_to_string(out) {
            if t.ends_with("end\n") {
                break t;
            }
        }
        assert!(std::time::Instant::now() < deadline, "no report");
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    let mut r = Report {
        names: Vec::new(),
        values: Vec::new(),
        dir: String::new(),
        null: [false; 3],
    };
    for line in text.lines() {
        if let Some(kv) = line.strip_prefix("env ") {
            let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
            r.names.push(k.to_string());
            r.values.push(v.to_string());
        } else if let Some(d) = line.strip_prefix("dir ") {
            r.dir = d.to_string();
        } else if let Some(n) = line.strip_prefix("null ") {
            for (i, w) in n.split(' ').enumerate().take(3) {
                r.null[i] = w == "yes";
            }
        }
    }
    r.names.sort();
    r
}

/// The names of the environment that this process was started with: on macOS the block that the
/// start passed, read from `KERN_PROCARGS2`, so that a variable that a framework of the system sets
/// in the process later (`__CF_USER_TEXT_ENCODING`) is not taken for one that the start passed;
/// elsewhere the process's environment.
pub fn initial_env() -> Vec<String> {
    #[cfg(target_os = "macos")]
    {
        let mut mib = [
            libc::CTL_KERN,
            libc::KERN_PROCARGS2,
            std::process::id() as i32,
        ];
        let mut size: libc::size_t = 0;
        // SAFETY: a null buffer asks for the size of this process's arguments area.
        let asked = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                3,
                std::ptr::null_mut(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        let mut buf = vec![0u8; size];
        // SAFETY: the buffer holds `size` bytes, which the call is told.
        let read = asked == 0
            && unsafe {
                libc::sysctl(
                    mib.as_mut_ptr(),
                    3,
                    buf.as_mut_ptr() as *mut libc::c_void,
                    &mut size,
                    std::ptr::null_mut(),
                    0,
                )
            } == 0;
        if !read || size < 4 {
            return vec!["unread".into()];
        }
        buf.truncate(size);
        let argc = i32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]).max(0) as usize;
        let rest = &buf[4..];
        let mut at = rest.iter().position(|b| *b == 0).unwrap_or(rest.len());
        while at < rest.len() && rest[at] == 0 {
            at += 1;
        }
        rest[at..]
            .split(|b| *b == 0)
            .skip(argc)
            .take_while(|e| !e.is_empty())
            .map(|e| {
                let e = String::from_utf8_lossy(e);
                e.split('=').next().unwrap_or("").to_string()
            })
            .collect()
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::env::vars_os()
            .map(|(k, _)| k.to_string_lossy().into_owned())
            .collect()
    }
}

#[cfg(unix)]
fn is_null(fd: i32) -> bool {
    use std::os::unix::fs::MetadataExt;
    let Ok(null) = std::fs::metadata("/dev/null") else {
        return false;
    };
    // SAFETY: fstat writes into a local stat buffer for a descriptor number; an unopened number
    // gives an error and the buffer is not read.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstat(fd, &mut st) } != 0 {
        return false;
    }
    st.st_mode & libc::S_IFMT == libc::S_IFCHR && st.st_rdev as u64 == null.rdev()
}

#[cfg(windows)]
fn is_null(which: u32) -> bool {
    use windows_sys::Wdk::Foundation::NtQueryObject;
    use windows_sys::Win32::System::Console::GetStdHandle;
    // SAFETY: GetStdHandle reads this process's standard handle and returns it or null.
    let h = unsafe { GetStdHandle(which) };
    if h.is_null() {
        return false;
    }
    let mut buf = vec![0u64; 128];
    let mut len = 0u32;
    // SAFETY: the buffer holds `buf.len() * 8` bytes, which the call is told; class 1 is the
    // object's name, a UNICODE_STRING whose buffer lies inside the same output buffer.
    let status = unsafe {
        NtQueryObject(
            h,
            1,
            buf.as_mut_ptr().cast(),
            (buf.len() * 8) as u32,
            &mut len,
        )
    };
    if status != 0 {
        return false;
    }
    // SAFETY: on success the buffer starts with a UNICODE_STRING that the call filled.
    let name = unsafe {
        let s = &*(buf.as_ptr() as *const windows_sys::Win32::Foundation::UNICODE_STRING);
        std::slice::from_raw_parts(s.Buffer, s.Length as usize / 2)
    };
    String::from_utf16_lossy(name).eq_ignore_ascii_case(r"\Device\Null")
}

#[test]
fn entry() {
    if std::env::var_os(VAR).is_none() {
        return;
    }
    let out = std::env::args().skip_while(|a| a != "--").nth(1);
    let mut text = String::new();
    for k in initial_env() {
        let v = std::env::var_os(&k).unwrap_or_default();
        text.push_str(&format!("env {}={}\n", k, v.to_string_lossy()));
    }
    let dir = std::env::current_dir()
        .map(|d| d.to_string_lossy().into_owned())
        .unwrap_or_default();
    text.push_str(&format!("dir {dir}\n"));
    #[cfg(unix)]
    let streams = [0, 1, 2];
    #[cfg(windows)]
    let streams = [
        windows_sys::Win32::System::Console::STD_INPUT_HANDLE,
        windows_sys::Win32::System::Console::STD_OUTPUT_HANDLE,
        windows_sys::Win32::System::Console::STD_ERROR_HANDLE,
    ];
    let words: Vec<&str> = streams
        .iter()
        .map(|s| if is_null(*s) { "yes" } else { "no" })
        .collect();
    text.push_str(&format!("null {}\nend\n", words.join(" ")));
    if let Some(out) = out {
        let _ = std::fs::write(out, text);
    }
    std::process::exit(0);
}
