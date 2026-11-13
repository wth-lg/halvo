//! verifier-35's probe of unit U4.4 on a Windows runner, for one scratch run. It prints counts and yes or no.
//! First: which handles a runner that the library starts holds beyond its three standard ones: a host on piped
//! streams makes a marker pipe inheritable on purpose and starts two runs whose runner is a stand-in that
//! this test builds; the stand-in writes through the marker handle and through the host's two standard
//! handles by their numbers.
//! Second: the unit's three cases and its control with `Stdio::inherit()` at line 250, in a copy of the tracked
//! files under the target directory, and what the hard-kill case's stand-in logged there and at the head.
//! On another system than Windows both tests do nothing.

#![allow(dead_code, unused_imports)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const STANDIN: &str = r####"// verifier-35: the runner stand-in of the handle probe. The test builds it at run time.
// It looks for handles beyond its three standard ones, writes through three handle numbers that its
// environment names, appends one line of counts to the report file, and then either says hello and
// waits for the end of its input (at most 45 s) or exits with the status 2.
use std::io::{Read, Write};

#[cfg(windows)]
mod win {
    use std::ffi::c_void;
    pub type Handle = *mut c_void;
    #[link(name = "kernel32")]
    extern "system" {
        pub fn GetStdHandle(id: u32) -> Handle;
        pub fn GetHandleInformation(h: Handle, flags: *mut u32) -> i32;
        pub fn WriteFile(
            h: Handle,
            buf: *const u8,
            len: u32,
            written: *mut u32,
            overlapped: *mut c_void,
        ) -> i32;
    }
}

#[cfg(windows)]
fn env_num(key: &str) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

#[cfg(windows)]
fn through(key: &str, text: &[u8]) -> (bool, u32) {
    let h = env_num(key);
    let mut flags = 0u32;
    let valid = h != 0 && unsafe { win::GetHandleInformation(h as win::Handle, &mut flags) } != 0;
    let mut wrote = 0u32;
    if valid {
        unsafe {
            win::WriteFile(
                h as win::Handle,
                text.as_ptr(),
                text.len() as u32,
                &mut wrote,
                std::ptr::null_mut(),
            );
        }
    }
    (valid, wrote)
}

#[cfg(windows)]
fn probe() -> String {
    let yes = |b: bool| if b { "yes" } else { "no" };
    let (mark_valid, mark_wrote) = through("V35_MARK_HANDLE", b"V35-VIA-MARK-HANDLE\n");
    let (err_valid, err_wrote) = through("V35_HOST_ERR", b"V35-VIA-HANDLE-ERR\n");
    let (out_valid, out_wrote) = through("V35_HOST_OUT", b"V35-VIA-HANDLE-OUT\n");
    let own: Vec<usize> = [0xFFFF_FFF6u32, 0xFFFF_FFF5, 0xFFFF_FFF4]
        .iter()
        .map(|id| unsafe { win::GetStdHandle(*id) } as usize)
        .collect();
    let (mut valid, mut beyond, mut inheritable_beyond) = (0u32, 0u32, 0u32);
    let mut h = 4usize;
    while h <= 0x4000 {
        let mut flags = 0u32;
        if unsafe { win::GetHandleInformation(h as win::Handle, &mut flags) } != 0 {
            valid += 1;
            if !own.contains(&h) {
                beyond += 1;
                if flags & 1 != 0 {
                    inheritable_beyond += 1;
                }
            }
        }
        h += 4;
    }
    format!(
        "standin valid={valid} beyond={beyond} inheritable_beyond={inheritable_beyond} mark_valid={} mark_wrote={mark_wrote} host_err_valid={} host_err_wrote={err_wrote} host_out_valid={} host_out_wrote={out_wrote}",
        yes(mark_valid),
        yes(err_valid),
        yes(out_valid)
    )
}

#[cfg(not(windows))]
fn probe() -> String {
    "standin not-windows".to_owned()
}

fn arg_after(args: &[String], key: &str) -> String {
    args.iter()
        .position(|a| a == key)
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_default()
}

fn main() {
    let line = probe();
    if let Ok(path) = std::env::var("V35_REPORT") {
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = f.write_all(format!("{line}\n").as_bytes());
        }
    }
    if std::env::var("V35_HELLO").as_deref() != Ok("1") {
        std::process::exit(2);
    }
    let args: Vec<String> = std::env::args().collect();
    let json = format!(
        r#"{{"t":"hello","run":"{}","agent":"{}","v":1}}"#,
        arg_after(&args, "--run"),
        arg_after(&args, "--agent")
    );
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(&(json.len() as u32).to_be_bytes());
    let _ = out.write_all(json.as_bytes());
    let _ = out.flush();
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(45));
        std::process::exit(3);
    });
    let mut sink = Vec::new();
    let _ = std::io::stdin().lock().read_to_end(&mut sink);
    std::process::exit(1);
}
"####;

/// One line for the job's log. A passing test's `println!` is captured, so the line goes to the process's own
/// standard error and once more through a child that inherits the standard output.
fn emit(line: &str) {
    let _ = writeln!(std::io::stderr(), "v35[direct] {line}");
    if cfg!(windows) {
        let _ = Command::new("cmd")
            .args(["/C", "echo", &format!("v35[echo] {line}")])
            .stdin(Stdio::null())
            .status();
    }
}

fn yes(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "no"
    }
}

fn count(hay: &[u8], needle: &[u8]) -> usize {
    hay.windows(needle.len()).filter(|w| *w == needle).count()
}

fn files_under(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files_under(&path, found);
        } else {
            found.push(path);
        }
    }
}

/// What the hard-kill case's stand-in logged under `tmp`: the count of its logs, of those with the line `eof`,
/// and of those with `stderr-after-eof wrote=yes`.
fn hard_kill_logs(tmp: &Path) -> (usize, usize, usize) {
    let mut files = Vec::new();
    files_under(&tmp.join("agent-client"), &mut files);
    let (mut logs, mut eof, mut wrote) = (0, 0, 0);
    for path in files {
        let text = path.to_string_lossy().replace('\\', "/");
        if !text.contains("/hard-kill-stderr-") || !text.ends_with(".log") {
            continue;
        }
        let Ok(body) = std::fs::read_to_string(&path) else {
            continue;
        };
        if !body.contains("runner pid=") {
            continue;
        }
        logs += 1;
        if body.lines().any(|l| l == "eof") {
            eof += 1;
        }
        if body.contains("stderr-after-eof wrote=yes") {
            wrote += 1;
        }
    }
    (logs, eof, wrote)
}

#[cfg(windows)]
mod probe {
    use super::*;
    use std::ffi::c_void;
    use std::num::NonZeroU32;
    use std::sync::Arc;

    use halvo_agent_client::{
        Config, Confirmer, Human, IssueRefused, LinkError, MarkSink, OriginAnswer, OwnerLink,
        Parts, RunEnd, RunSpec, Secrets, Supervisor,
    };
    use halvo_schema::executable::frame::{
        ConfirmDecision, ConfirmRequest, HandbackRequest, SecretReply,
    };
    use halvo_schema::executable::names::{Answer, Mode};
    use halvo_schema::executable::text::{Name, RunId, WebOrigin};
    use halvo_schema::request::{decode, CoordinatorRole, Decoded};
    use halvo_schema::{AgentId, WorkflowId};

    type Handle = *mut c_void;

    #[link(name = "kernel32")]
    extern "system" {
        fn CreatePipe(read: *mut Handle, write: *mut Handle, attrs: *mut c_void, size: u32) -> i32;
        fn SetHandleInformation(h: Handle, mask: u32, flags: u32) -> i32;
        fn GetHandleInformation(h: Handle, flags: *mut u32) -> i32;
        fn GetStdHandle(id: u32) -> Handle;
        fn PeekNamedPipe(
            h: Handle,
            buf: *mut u8,
            size: u32,
            read: *mut u32,
            avail: *mut u32,
            left: *mut u32,
        ) -> i32;
        fn ReadFile(
            h: Handle,
            buf: *mut u8,
            len: u32,
            read: *mut u32,
            overlapped: *mut c_void,
        ) -> i32;
    }

    struct Quiet;

    impl OwnerLink for Quiet {
        fn request(&self, request: &[u8]) -> Result<Vec<u8>, LinkError> {
            let Ok(Decoded::Raw(raw)) = decode(request) else {
                return Err(LinkError);
            };
            let body = raw.body().get();
            let ok = if body.contains(r#""op":"add_origin""#) {
                r#"{"rev":2}"#
            } else if body.contains(r#""op":"end_run""#) {
                r#"{"ok":"yes"}"#
            } else {
                r#"{"stored":1}"#
            };
            Ok(format!(r#"{{"v":1,"id":"{}","ok":{ok}}}"#, raw.id().as_str()).into_bytes())
        }
    }

    impl Confirmer for Quiet {
        fn answers(&self, _run: &RunId, request: &ConfirmRequest) -> Vec<Answer> {
            vec![Answer::AllowOnce; request.effects.len()]
        }

        fn issued(&self, _run: &RunId, _decision: &ConfirmDecision) -> Result<(), IssueRefused> {
            Ok(())
        }
    }

    impl Human for Quiet {
        fn handback(&self, _run: &RunId, _request: &HandbackRequest) {}

        fn allow_origin(&self, _run: &RunId, _origin: &WebOrigin) -> OriginAnswer {
            OriginAnswer::Allow
        }

        fn progress(&self, _run: &RunId, _step: NonZeroU32, _of: NonZeroU32) {}
    }

    impl Secrets for Quiet {
        fn read(&self, _name: &Name) -> SecretReply {
            SecretReply::None
        }
    }

    impl MarkSink for Quiet {
        fn start(&self, _run: &RunId) {}

        fn end(&self, _run: &RunId, _end: &RunEnd) {}
    }

    fn say(text: &str) {
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{text}");
        let _ = out.flush();
    }

    fn spec(salt: u8) -> RunSpec {
        let mut b = [salt; 16];
        b[..4].copy_from_slice(&std::process::id().to_be_bytes());
        let mut agent = b;
        agent[15] = 0xa0;
        let mut workflow = b;
        workflow[15] = 0xb0;
        RunSpec {
            run: RunId::from_random(b),
            agent: AgentId::from_random(agent),
            workflow: WorkflowId::from_random(workflow),
            mode: Mode::Replay,
            browser: PathBuf::from("none"),
            model: None,
            headed: false,
        }
    }

    fn inheritable(h: Handle) -> bool {
        let mut flags = 0u32;
        (unsafe { GetHandleInformation(h, &mut flags) }) != 0 && flags & 1 != 0
    }

    /// The host: the library in this process, whose two standard streams the test holds as pipes.
    pub fn host() {
        let dir = PathBuf::from(std::env::var("V35_DIR").unwrap_or_default());
        let (mut read, mut write): (Handle, Handle) = (std::ptr::null_mut(), std::ptr::null_mut());
        let made = unsafe { CreatePipe(&mut read, &mut write, std::ptr::null_mut(), 0) } != 0
            && unsafe { SetHandleInformation(write, 1, 1) } != 0;
        let out = unsafe { GetStdHandle(0xFFFF_FFF5) };
        let err = unsafe { GetStdHandle(0xFFFF_FFF4) };
        say(&format!(
            "host marker-pipe-made {} own-stdout-inheritable {} own-stderr-inheritable {}",
            yes(made),
            yes(inheritable(out)),
            yes(inheritable(err))
        ));
        let report = dir.join("report.txt");
        std::env::set_var("V35_MARK_HANDLE", (write as usize).to_string());
        std::env::set_var("V35_HOST_OUT", (out as usize).to_string());
        std::env::set_var("V35_HOST_ERR", (err as usize).to_string());
        std::env::set_var("V35_REPORT", &report);
        std::env::set_var("V35_HELLO", "1");
        let data = dir.join("data");
        let _ = std::fs::create_dir_all(&data);
        let quiet = Arc::new(Quiet);
        let parts = Parts {
            link: quiet.clone(),
            confirmer: quiet.clone(),
            human: quiet.clone(),
            secrets: quiet.clone(),
            marks: quiet,
        };
        let sup = Supervisor::new(
            CoordinatorRole::claim(),
            Config::new(data, dir.join("standin.exe")),
            parts,
        );
        let (a, b) = (spec(1), spec(2));
        let (run_a, run_b) = (a.run.clone(), b.run.clone());
        say(&format!("host started-a {}", yes(sup.start(a).is_ok())));
        say(&format!("host started-b {}", yes(sup.start(b).is_ok())));
        let until = Instant::now() + Duration::from_secs(20);
        while Instant::now() < until
            && std::fs::read_to_string(&report)
                .unwrap_or_default()
                .lines()
                .count()
                < 2
        {
            std::thread::sleep(Duration::from_millis(50));
        }
        let mut avail = 0u32;
        let mut got = 0u32;
        let mut buf = [0u8; 256];
        if made
            && unsafe {
                PeekNamedPipe(
                    read,
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null_mut(),
                    &mut avail,
                    std::ptr::null_mut(),
                )
            } != 0
            && avail > 0
        {
            unsafe {
                ReadFile(
                    read,
                    buf.as_mut_ptr(),
                    avail.min(256),
                    &mut got,
                    std::ptr::null_mut(),
                );
            }
        }
        say(&format!(
            "host marker-pipe-bytes {got} marker-lines {}",
            count(&buf[..got as usize], b"V35-VIA-MARK-HANDLE")
        ));
        let _ = sup.kill(&run_a);
        let _ = sup.kill(&run_b);
        say("host done");
        std::process::exit(0);
    }

    fn read_all(
        mut stream: impl std::io::Read + Send + 'static,
    ) -> std::thread::JoinHandle<Vec<u8>> {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = stream.read_to_end(&mut bytes);
            bytes
        })
    }

    pub fn handles() {
        let dir =
            Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("v35h-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("standin.rs"), STANDIN).unwrap();
        let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());
        let built = Command::new(rustc)
            .args(["--edition", "2021", "-C", "debuginfo=0", "-o"])
            .arg(dir.join("standin.exe"))
            .arg(dir.join("standin.rs"))
            .env("TMP", &dir)
            .env("TEMP", &dir)
            .current_dir(&dir)
            .stdin(Stdio::null())
            .output();
        let built_ok = built.as_ref().is_ok_and(|o| o.status.success());
        emit(&format!("handles: stand-in built: {}", yes(built_ok)));
        if !built_ok {
            return;
        }
        let mut host = Command::new(std::env::current_exe().unwrap())
            .args([
                "v35_host_role",
                "--exact",
                "--nocapture",
                "--test-threads",
                "1",
            ])
            .env("V35_ROLE", "host")
            .env("V35_DIR", &dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let out = read_all(host.stdout.take().unwrap());
        let err = read_all(host.stderr.take().unwrap());
        let until = Instant::now() + Duration::from_secs(90);
        let mut ended = false;
        while Instant::now() < until {
            if matches!(host.try_wait(), Ok(Some(_))) {
                ended = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        if !ended {
            let _ = host.kill();
        }
        let _ = host.wait();
        let (out, err) = (
            out.join().unwrap_or_default(),
            err.join().unwrap_or_default(),
        );
        emit(&format!(
            "handles: host ended by itself: {}; its standard output {} bytes, its standard error {} bytes",
            yes(ended),
            out.len(),
            err.len()
        ));
        for line in String::from_utf8_lossy(&out).lines() {
            if line.starts_with("host ") {
                emit(&format!("handles: {line}"));
            }
        }
        emit(&format!(
            "handles: lines that a runner wrote through the host's handle numbers and that the test read: in the host's standard error {}, in its standard output {}",
            count(&err, b"V35-VIA-HANDLE-ERR"),
            count(&out, b"V35-VIA-HANDLE-OUT")
        ));
        let report = std::fs::read_to_string(dir.join("report.txt")).unwrap_or_default();
        emit(&format!(
            "handles: report lines of the runners: {}",
            report.lines().count()
        ));
        for (i, line) in report.lines().enumerate() {
            if line.starts_with("standin ") {
                emit(&format!("handles: runner {} {line}", i + 1));
            }
        }
    }

    fn copy_tracked(root: &Path, to: &Path) -> (usize, usize) {
        let listed = Command::new("git")
            .args(["ls-files", "-z"])
            .current_dir(root)
            .stdin(Stdio::null())
            .output()
            .map(|o| o.stdout)
            .unwrap_or_default();
        let (mut copied, mut failed) = (0, 0);
        for name in listed.split(|b| *b == 0).filter(|n| !n.is_empty()) {
            let rel = String::from_utf8_lossy(name).into_owned();
            if rel.ends_with("tests/v35_handles.rs") {
                continue;
            }
            let (from, dest) = (root.join(&rel), to.join(&rel));
            if !from.is_file() {
                continue;
            }
            if let Some(parent) = dest.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if std::fs::copy(&from, &dest).is_ok() {
                copied += 1;
            } else {
                failed += 1;
            }
        }
        (copied, failed)
    }

    pub fn inherit() {
        let tmp = Path::new(env!("CARGO_TARGET_TMPDIR"));
        let (logs, eof, wrote) = hard_kill_logs(tmp);
        emit(&format!(
            "head: the hard-kill case's stand-in logs under the target directory: {logs}; with the line eof: {eof}; with stderr-after-eof wrote=yes: {wrote}"
        ));
        if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true") {
            emit("inherit: not a hosted job, nothing run");
            return;
        }
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
        let base = tmp.join(format!("v35i-{}", std::process::id()));
        let (ws, target, scratch) = (base.join("w"), base.join("t"), base.join("s"));
        std::fs::create_dir_all(&scratch).unwrap();
        let (copied, failed) = copy_tracked(&root, &ws);
        emit(&format!(
            "inherit: tracked files copied: {copied}, not copied: {failed}"
        ));
        let sup = ws.join("crates/halvo-agent-client/src/supervisor.rs");
        let text = std::fs::read_to_string(&sup).unwrap_or_default();
        let places = text.matches(".stderr(Stdio::null());").count();
        std::fs::write(
            &sup,
            text.replace(".stderr(Stdio::null());", ".stderr(Stdio::inherit());"),
        )
        .unwrap();
        emit(&format!(
            "inherit: places where the copy's supervisor.rs got Stdio::inherit(): {places}"
        ));
        let mut cargo = Command::new("cargo")
            .args([
                "test",
                "-p",
                "halvo-agent-client",
                "--locked",
                "--no-fail-fast",
                "--test",
                "stderr",
                "--test",
                "kill",
                "--",
                "standard_error",
                "stand_in_started_by_hand",
            ])
            .current_dir(&ws)
            .env("CARGO_TARGET_DIR", &target)
            .env("TMP", &scratch)
            .env("TEMP", &scratch)
            .env("TMPDIR", &scratch)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let out = read_all(cargo.stdout.take().unwrap());
        let err = read_all(cargo.stderr.take().unwrap());
        let started = Instant::now();
        let mut code = None;
        while started.elapsed() < Duration::from_secs(1500) {
            if let Ok(Some(status)) = cargo.try_wait() {
                code = status.code();
                break;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        if code.is_none() {
            let _ = Command::new("taskkill")
                .args(["/T", "/F", "/PID", &cargo.id().to_string()])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            let _ = cargo.kill();
        }
        let _ = cargo.wait();
        let mut text = String::from_utf8_lossy(&out.join().unwrap_or_default()).into_owned();
        text.push_str(&String::from_utf8_lossy(&err.join().unwrap_or_default()));
        emit(&format!(
            "inherit: cargo test of the copy ended in {} s with the status {}",
            started.elapsed().as_secs(),
            code.map_or_else(|| "none".to_owned(), |c| c.to_string())
        ));
        let cases = [
            ("the marker case", "the_marker_that_the_runner"),
            ("the flood case", "more_than_a_mebibyte"),
            ("the control", "the_stand_in_started_by_hand"),
            (
                "the hard-kill case",
                "a_write_on_standard_error_after_the_end_of_file",
            ),
        ];
        for (what, start) in cases {
            let result = text
                .lines()
                .find(|l| l.starts_with(&format!("test {start}")) && l.contains(" ... "))
                .map_or(
                    "not run",
                    |l| if l.ends_with("ok") { "ok" } else { "FAILED" },
                );
            let place = text
                .lines()
                .find(|l| l.contains(&format!("thread '{start}")) && l.contains("panicked at"))
                .and_then(|l| l.rsplit("panicked at ").next())
                .map(|p| {
                    let p = p.replace('\\', "/");
                    let file = if p.contains("tests/kill.rs") {
                        "kill.rs"
                    } else {
                        "stderr.rs"
                    };
                    let line = p.split(':').nth(1).unwrap_or("?").to_owned();
                    format!("{file} line {line}")
                })
                .unwrap_or_else(|| "no panic".to_owned());
            emit(&format!("inherit: {what}: {result} ({place})"));
        }
        let (logs, eof, wrote) = hard_kill_logs(&target.join("tmp"));
        emit(&format!(
            "inherit: the hard-kill case's stand-in logs in the copy: {logs}; with the line eof: {eof}; with stderr-after-eof wrote=yes: {wrote}"
        ));
    }
}

#[test]
fn v35_host_role() {
    #[cfg(windows)]
    if std::env::var("V35_ROLE").as_deref() == Ok("host") {
        probe::host();
    }
}

#[test]
fn v35_handles_that_a_windows_runner_holds_beyond_its_three() {
    #[cfg(windows)]
    probe::handles();
    #[cfg(not(windows))]
    emit("handles: not a windows runner, nothing run");
}

#[test]
fn v35_the_units_cases_with_line_250_as_inherit_in_a_copy() {
    #[cfg(windows)]
    probe::inherit();
    #[cfg(not(windows))]
    emit("inherit: not a windows runner, nothing run");
}
