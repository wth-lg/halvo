use crate::flow::{NoRandom, NotOpened, Opener, Random};
use crate::origin::PageUrl;

/// Opens a page in the person's default browser: through `/usr/bin/open` on macOS and
/// through `ShellExecuteW` on Windows. No test runs it, because no test opens a window.
pub struct Browser;

impl Opener for Browser {
    fn open(&self, page: &PageUrl) -> Result<(), NotOpened> {
        open_page(page.as_str())
    }
}

#[cfg(target_os = "macos")]
fn open_page(url: &str) -> Result<(), NotOpened> {
    use std::process::{Command, Stdio};
    let status = Command::new("/usr/bin/open")
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|_| NotOpened)?;
    if status.success() {
        Ok(())
    } else {
        Err(NotOpened)
    }
}

#[cfg(windows)]
fn open_page(url: &str) -> Result<(), NotOpened> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let wide = |text: &str| -> Vec<u16> { OsStr::new(text).encode_wide().chain([0]).collect() };
    let verb = wide("open");
    let file = wide(url);
    // SAFETY: `verb` and `file` are NUL-terminated UTF-16 strings that live until the call
    // returns, a null window, parameter list and directory are documented as allowed, and
    // ShellExecuteW keeps no pointer after it returns.
    let instance = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    if instance as isize > 32 {
        Ok(())
    } else {
        Err(NotOpened)
    }
}

/// The random bytes of the system, through getrandom.
pub struct SystemRandom;

impl Random for SystemRandom {
    fn fill(&self, bytes: &mut [u8; 48]) -> Result<(), NoRandom> {
        getrandom::getrandom(bytes).map_err(|_| NoRandom)
    }
}
