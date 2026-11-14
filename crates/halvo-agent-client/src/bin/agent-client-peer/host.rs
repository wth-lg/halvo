//! The library in a process of its own, so that a test can end that process by the system's hard kill and read its
//! two streams: `host DATA_DIR SCRIPT OWNER_LOG [HOLD_DIR]` starts one run whose runner is this binary with SCRIPT,
//! prints `started <run id>`, answers every act with yes, logs each request that reaches its owner as one line of
//! OWNER_LOG, and prints `end <RunEnd as Debug>` when the run has ended. With HOLD_DIR, `Confirmer::answers` prints
//! `answers-in`, makes the file HOLD_DIR/asked, waits until the file HOLD_DIR/release exists or 60 seconds have
//! passed, and prints `answers-out` before it answers. Before the start it writes into SCRIPT, for `{host-mark}`,
//! the number of a descriptor or handle of SCRIPT.mark that it holds open and inheritable (on macOS a descriptor
//! of at least 200 without close-on-exec), and on Windows for `{host-out}` and `{host-err}` the numbers of its own
//! standard output and standard error handles, and for `{host-pid}` its own process number. On macOS, for
//! `{host-held}`, it first holds open without close-on-exec a file at 3, the read end of a pipe at 101, a directory
//! at 102, a kqueue at 103 and one end of a UNIX socket pair at 104, the write end at 256 and the other socket end at
//! 5000 or the highest number below its raised limit of open files, and writes the line of what it holds.

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use halvo_agent_client::{
    Config, Confirmer, Human, IssueRefused, LinkError, MarkSink, OriginAnswer, OwnerLink, Parts,
    RunEnd, RunSpec, Secrets, Supervisor,
};
use halvo_schema::executable::frame::{
    ConfirmDecision, ConfirmRequest, HandbackRequest, SecretReply,
};
use halvo_schema::executable::names::{Answer, Mode};
use halvo_schema::executable::text::{Name, RunId, WebOrigin};
use halvo_schema::request::{decode, CoordinatorRole, Decoded};
use halvo_schema::{AgentId, WorkflowId};

struct Owner(Mutex<std::fs::File>);

impl OwnerLink for Owner {
    fn request(&self, request: &[u8]) -> Result<Vec<u8>, LinkError> {
        let mut line = request.to_vec();
        line.push(b'\n');
        if let Ok(mut f) = self.0.lock() {
            let _ = f.write_all(&line);
        }
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

struct Yes {
    hold: Option<PathBuf>,
}

const HOLD_MAX: Duration = Duration::from_secs(60);

fn say(text: &str) {
    let mut out = io::stdout().lock();
    let _ = writeln!(out, "{text}");
    let _ = out.flush();
}

fn hold_until_released(dir: &Path) {
    say("answers-in");
    let _ = std::fs::write(dir.join("asked"), b"");
    let deadline = std::time::Instant::now() + HOLD_MAX;
    while !dir.join("release").exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    say("answers-out");
}

impl Confirmer for Yes {
    fn answers(&self, _run: &RunId, request: &ConfirmRequest) -> Vec<Answer> {
        if let Some(dir) = &self.hold {
            hold_until_released(dir);
        }
        vec![Answer::AllowOnce; request.effects.len()]
    }

    fn issued(&self, _run: &RunId, _decision: &ConfirmDecision) -> Result<(), IssueRefused> {
        Ok(())
    }
}

impl Human for Yes {
    fn handback(&self, _run: &RunId, _request: &HandbackRequest) {}

    fn allow_origin(&self, _run: &RunId, _origin: &WebOrigin) -> OriginAnswer {
        OriginAnswer::Allow
    }

    fn progress(&self, _run: &RunId, _step: NonZeroU32, _of: NonZeroU32) {}
}

impl Secrets for Yes {
    fn read(&self, _name: &Name) -> SecretReply {
        SecretReply::None
    }
}

impl MarkSink for Yes {
    fn start(&self, _run: &RunId) {}

    fn end(&self, _run: &RunId, end: &RunEnd) {
        say(&format!("end {end:?}"));
    }
}

fn id16(salt: u8) -> [u8; 16] {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut b = [salt; 16];
    b[..4].copy_from_slice(&std::process::id().to_be_bytes());
    b[4..].copy_from_slice(&nanos.to_be_bytes()[4..]);
    b
}

#[cfg(unix)]
fn held_open(file: std::fs::File) -> Option<String> {
    use std::os::fd::IntoRawFd;
    extern "C" {
        fn fcntl(fd: i32, cmd: i32, ...) -> i32;
    }
    const F_DUPFD: i32 = 0;
    let fd = file.into_raw_fd();
    // SAFETY: F_DUPFD takes the number of the open file `fd` and a lowest number, and reads no memory of this process.
    let high = unsafe { fcntl(fd, F_DUPFD, 200) };
    (high >= 200).then(|| high.to_string())
}

#[cfg(windows)]
fn held_open(file: std::fs::File) -> Option<String> {
    use std::os::windows::io::IntoRawHandle;
    #[link(name = "kernel32")]
    extern "system" {
        fn SetHandleInformation(handle: *mut std::ffi::c_void, mask: u32, flags: u32) -> i32;
    }
    let handle = file.into_raw_handle();
    // SAFETY: `handle` is the open handle of `file`, which this process keeps for its life, and the call changes its flag alone.
    (unsafe { SetHandleInformation(handle, 1, 1) } != 0).then(|| (handle as usize).to_string())
}

#[cfg(unix)]
fn held_kinds(script: &Path) -> Option<String> {
    use std::os::fd::IntoRawFd;
    use std::os::unix::net::UnixStream;
    extern "C" {
        fn fcntl(fd: i32, cmd: i32, ...) -> i32;
        fn dup2(from: i32, to: i32) -> i32;
        fn getrlimit(resource: i32, limit: *mut [u64; 2]) -> i32;
        fn setrlimit(resource: i32, limit: *const [u64; 2]) -> i32;
        fn kqueue() -> i32;
    }
    const F_GETFD: i32 = 1;
    const F_SETFD: i32 = 2;
    const RLIMIT_NOFILE: i32 = 8;
    let mut limit = [0u64; 2];
    // SAFETY: `limit` is a live, writable pair of the layout of struct rlimit, which the call fills.
    if unsafe { getrlimit(RLIMIT_NOFILE, &mut limit) } != 0 {
        return None;
    }
    if limit[0] < 5001 {
        limit[0] = limit[1].min(5001);
        // SAFETY: `limit` is a live pair of the layout of struct rlimit, which the call reads.
        if unsafe { setrlimit(RLIMIT_NOFILE, &limit) } != 0 {
            return None;
        }
    }
    let high = i32::try_from(limit[0].min(5001) - 1).ok()?;
    if high < 257 {
        return None;
    }
    // SAFETY: F_GETFD reads the flags of a number and changes nothing, whether or not it is open.
    let before = if unsafe { fcntl(3, F_GETFD) } == -1 {
        "closed"
    } else {
        "open"
    };
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(format!("{}.held", script.display()))
        .ok()?
        .into_raw_fd();
    let (read, write) = std::io::pipe().ok()?;
    let (socket_a, socket_b) = UnixStream::pair().ok()?;
    let dir = std::fs::File::open(script.parent()?).ok()?.into_raw_fd();
    // SAFETY: kqueue takes no argument and returns a new descriptor or -1.
    let queue = unsafe { kqueue() };
    if queue < 0 {
        return None;
    }
    let places = [
        (3, file, "file"),
        (101, read.into_raw_fd(), "pipe-read"),
        (102, dir, "directory"),
        (103, queue, "kqueue"),
        (104, socket_b.into_raw_fd(), "socket"),
        (256, write.into_raw_fd(), "pipe-write"),
        (high, socket_a.into_raw_fd(), "socket"),
    ];
    let mut line = format!("held limit={} 3-before={before}", limit[0]);
    for (number, from, kind) in places {
        // SAFETY: dup2 and fcntl take descriptor numbers; `from` is open and stays open, as the host keeps it for
        // its life, and `number` is below the limit of open files.
        let placed = unsafe { dup2(from, number) == number && fcntl(number, F_SETFD, 0) == 0 };
        if !placed {
            return None;
        }
        line.push_str(&format!(" {number}:{kind}"));
    }
    Some(line)
}

#[cfg(windows)]
fn held_kinds(_script: &Path) -> Option<String> {
    Some("held none".to_owned())
}

#[cfg(unix)]
fn own_numbers() -> (String, String) {
    ("1".to_owned(), "2".to_owned())
}

#[cfg(windows)]
fn own_numbers() -> (String, String) {
    use std::os::windows::io::AsRawHandle;
    (
        (io::stdout().as_raw_handle() as usize).to_string(),
        (io::stderr().as_raw_handle() as usize).to_string(),
    )
}

fn fill(script: &Path) -> Option<()> {
    let text = std::fs::read_to_string(script).ok()?;
    if !text.contains("{host-") {
        return Some(());
    }
    let held = if text.contains("{host-held}") {
        held_kinds(script)?
    } else {
        String::new()
    };
    let mark = OpenOptions::new()
        .create(true)
        .append(true)
        .open(format!("{}.mark", script.display()))
        .ok()?;
    let mark = held_open(mark)?;
    let (out, err) = own_numbers();
    let text = text
        .replace("{host-held}", &held)
        .replace("{host-pid}", &std::process::id().to_string())
        .replace("{host-mark}", &mark)
        .replace("{host-out}", &out)
        .replace("{host-err}", &err);
    std::fs::write(script, text).ok()
}

pub fn run(args: &[String]) {
    let (data, script, owner_log, hold) = match args {
        [data, script, owner_log] => (data, script, owner_log, None),
        [data, script, owner_log, hold] => (data, script, owner_log, Some(PathBuf::from(hold))),
        _ => std::process::exit(64),
    };
    let Ok(log) = OpenOptions::new().create(true).append(true).open(owner_log) else {
        std::process::exit(70);
    };
    let Ok(me) = std::env::current_exe() else {
        std::process::exit(70);
    };
    if fill(Path::new(script)).is_none() {
        std::process::exit(70);
    }
    let yes = Arc::new(Yes { hold });
    let parts = Parts {
        link: Arc::new(Owner(Mutex::new(log))),
        confirmer: yes.clone(),
        human: yes.clone(),
        secrets: yes.clone(),
        marks: yes,
    };
    let sup = Supervisor::new(
        CoordinatorRole::claim(),
        Config::new(PathBuf::from(data), me),
        parts,
    );
    let spec = RunSpec {
        run: RunId::from_random(id16(1)),
        agent: AgentId::from_random(id16(2)),
        workflow: WorkflowId::from_random(id16(3)),
        mode: Mode::Replay,
        browser: PathBuf::from(script),
        model: None,
        headed: false,
    };
    let run = spec.run.clone();
    match sup.start(spec) {
        Ok(()) => say(&format!("started {run}")),
        Err(e) => say(&format!("refused {e}")),
    }
    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}
