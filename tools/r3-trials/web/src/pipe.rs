use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

pub const CALL_WAIT: Duration = Duration::from_secs(15);

/// A browser driven over `--remote-debugging-pipe`, for the pipe forms of the row
/// `driver_killed` (ruling CP-24); chromiumoxide 0.9.1 has no pipe. Messages are JSON, each ended
/// by a NUL byte. On macOS the browser reads fd 3 and writes fd 4; on Windows it gets two
/// inheritable pipe handles with `--remote-debugging-io-pipes`.
pub struct PipeBrowser {
    child: Child,
    writer: File,
    messages: mpsc::Receiver<Value>,
    next_id: u64,
    pub events: Vec<Value>,
}

impl PipeBrowser {
    pub fn launch(browser: &Path, args: &[String], temp: &Path) -> Result<PipeBrowser, String> {
        let (child, reader, writer) = spawn(browser, args, temp)?;
        let (tx, messages) = mpsc::channel();
        std::thread::spawn(move || {
            let mut r = BufReader::new(reader);
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match r.read_until(0, &mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        if buf.last() == Some(&0) {
                            buf.pop();
                        }
                        if let Ok(v) = serde_json::from_slice::<Value>(&buf) {
                            if tx.send(v).is_err() {
                                break;
                            }
                        }
                    }
                }
            }
        });
        Ok(PipeBrowser {
            child,
            writer,
            messages,
            next_id: 0,
            events: Vec::new(),
        })
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Sends one command and waits for its answer at most `CALL_WAIT`; every event that comes
    /// meanwhile goes to `events`, and `on_event` sees it first.
    pub fn call(
        &mut self,
        method: &str,
        params: Value,
        session: Option<&str>,
        on_event: &mut dyn FnMut(&mut PipeBrowser, &Value),
    ) -> Result<Value, String> {
        self.next_id += 1;
        let id = self.next_id;
        let mut msg = json!({"id": id, "method": method, "params": params});
        if let Some(s) = session {
            msg["sessionId"] = json!(s);
        }
        self.send(&msg)?;
        let deadline = Instant::now() + CALL_WAIT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let m = self
                .messages
                .recv_timeout(left)
                .map_err(|_| format!("{method}: no answer"))?;
            if m.get("id").and_then(Value::as_u64) == Some(id) {
                if let Some(e) = m.get("error") {
                    return Err(format!("{method}: {}", e["message"]));
                }
                return Ok(m["result"].clone());
            }
            if m.get("method").is_some() {
                on_event(self, &m);
                self.events.push(m);
            }
        }
    }

    /// Sends one command and does not wait for its answer.
    pub fn send_only(&mut self, method: &str, params: Value, session: Option<&str>) {
        self.next_id += 1;
        let mut msg = json!({"id": self.next_id, "method": method, "params": params});
        if let Some(s) = session {
            msg["sessionId"] = json!(s);
        }
        let _ = self.send(&msg);
    }

    /// Takes the events that came for at most `wait`, passing each to `on_event`.
    pub fn pump(&mut self, wait: Duration, on_event: &mut dyn FnMut(&mut PipeBrowser, &Value)) {
        let deadline = Instant::now() + wait;
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match self.messages.recv_timeout(left) {
                Ok(m) => {
                    if m.get("method").is_some() {
                        on_event(self, &m);
                        self.events.push(m);
                    }
                }
                Err(_) => break,
            }
        }
    }

    fn send(&mut self, msg: &Value) -> Result<(), String> {
        let mut bytes = serde_json::to_vec(msg).map_err(|e| e.to_string())?;
        bytes.push(0);
        self.writer
            .write_all(&bytes)
            .and_then(|_| self.writer.flush())
            .map_err(|e| format!("pipe write {}", e.kind()))
    }

    pub fn kill(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(unix)]
fn spawn(browser: &Path, args: &[String], temp: &Path) -> Result<(Child, File, File), String> {
    use std::os::fd::FromRawFd;
    use std::os::unix::process::CommandExt;
    let mut to_browser = [0; 2];
    let mut from_browser = [0; 2];
    if unsafe { libc::pipe(to_browser.as_mut_ptr()) } != 0 {
        return Err("pipe".into());
    }
    if unsafe { libc::pipe(from_browser.as_mut_ptr()) } != 0 {
        unsafe {
            libc::close(to_browser[0]);
            libc::close(to_browser[1]);
        }
        return Err("pipe".into());
    }
    for fd in [to_browser[1], from_browser[0]] {
        unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
    }
    let browser_reads = to_browser[0];
    let browser_writes = from_browser[1];
    let mut cmd = Command::new(browser);
    cmd.args(args)
        .env("TMPDIR", temp)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    unsafe {
        cmd.pre_exec(move || {
            let reads = libc::dup(browser_reads);
            let writes = libc::dup(browser_writes);
            if reads < 0 || writes < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::dup2(reads, 3) < 0 || libc::dup2(writes, 4) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let spawned = cmd.spawn();
    unsafe {
        libc::close(browser_reads);
        libc::close(browser_writes);
    }
    let child = match spawned {
        Ok(c) => c,
        Err(e) => {
            unsafe {
                libc::close(to_browser[1]);
                libc::close(from_browser[0]);
            }
            return Err(format!("spawn {}", e.kind()));
        }
    };
    let writer = unsafe { File::from_raw_fd(to_browser[1]) };
    let reader = unsafe { File::from_raw_fd(from_browser[0]) };
    Ok((child, reader, writer))
}

#[cfg(windows)]
fn spawn(browser: &Path, args: &[String], temp: &Path) -> Result<(Child, File, File), String> {
    use std::os::windows::io::FromRawHandle;
    use windows_sys::Win32::Foundation::{
        CloseHandle, SetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT,
    };
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
    use windows_sys::Win32::System::Pipes::CreatePipe;
    let attrs = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: 1,
    };
    let mut browser_reads: HANDLE = std::ptr::null_mut();
    let mut we_write: HANDLE = std::ptr::null_mut();
    let mut we_read: HANDLE = std::ptr::null_mut();
    let mut browser_writes: HANDLE = std::ptr::null_mut();
    unsafe {
        if CreatePipe(&mut browser_reads, &mut we_write, &attrs, 0) == 0 {
            return Err("pipe".into());
        }
        if CreatePipe(&mut we_read, &mut browser_writes, &attrs, 0) == 0 {
            CloseHandle(browser_reads);
            CloseHandle(we_write);
            return Err("pipe".into());
        }
        SetHandleInformation(we_write, HANDLE_FLAG_INHERIT, 0);
        SetHandleInformation(we_read, HANDLE_FLAG_INHERIT, 0);
    }
    let mut all: Vec<String> = args.to_vec();
    all.push(format!(
        "--remote-debugging-io-pipes={},{}",
        browser_reads as usize, browser_writes as usize
    ));
    let spawned = Command::new(browser)
        .args(&all)
        .env("TMP", temp)
        .env("TEMP", temp)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    unsafe {
        CloseHandle(browser_reads);
        CloseHandle(browser_writes);
    }
    let child = match spawned {
        Ok(c) => c,
        Err(e) => {
            unsafe {
                CloseHandle(we_write);
                CloseHandle(we_read);
            }
            return Err(format!("spawn {}", e.kind()));
        }
    };
    let writer = unsafe { File::from_raw_handle(we_write as _) };
    let reader = unsafe { File::from_raw_handle(we_read as _) };
    Ok((child, reader, writer))
}
