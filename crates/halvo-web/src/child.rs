use std::path::{Path, PathBuf};

pub const VAR: &str = "HALVO_WEB_CHILD";
pub const ENTRY: &str = "child::entry";

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
    for (k, v) in std::env::vars_os() {
        text.push_str(&format!(
            "env {}={}\n",
            k.to_string_lossy(),
            v.to_string_lossy()
        ));
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
