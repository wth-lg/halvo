//! The process tests of the owner: each starts the built binary as a child, hands it the key on standard input and
//! the ends of its channels, and reads what it writes.

use std::io::{PipeReader, PipeWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::Mutex;
use std::time::{Duration, Instant as Mono, SystemTime, UNIX_EPOCH};

use halvo_agent_client::link::{End, Ends, Refusal, StreamLink};
use halvo_agent_client::OwnerLink;
use halvo_owner::exit;
use halvo_schema_core::category::SeedCategory;
use halvo_schema_core::ids::{Position, RequestId, StoreId, SummaryId, WorkflowId};
use halvo_schema_core::record::{
    Episode, FloorLayer, Instant, ObservedStep, Ordinal, Part, Tagged, Text,
};
use halvo_schema_core::request::{
    decode_reply, AnalysisRole, AnalysisWrite, ErrorName, InterfaceRead, InterfaceRole, Reply,
    ReplyResult,
};
use halvo_schema_core::site::AppId;
use halvo_seam::frame::{read_frame, write_frame, CLIENT_FRAME_MAX, SERVER_FRAME_MAX};
use halvo_sqlcipher::rusqlite::Connection;
use serde_json::Value;

const KEY: [u8; 32] = [7; 32];
const OTHER_KEY: [u8; 32] = [8; 32];
const DAY_NS: i64 = 86_400_000_000_000;
const FAR: i64 = 9_000_000_000_000_000_000;
/// The longest wait for a frame or an exit that the owner owes at once.
const WAIT: Duration = Duration::from_secs(30);

/// A new directory under the target's tmp directory, removed when the value is dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let parent = Path::new(env!("CARGO_TARGET_TMPDIR"));
        Scratch(halvo_test_dir::make(parent, name).unwrap())
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Err(e) = std::fs::remove_dir_all(&self.0) {
            eprintln!(
                "the scratch directory {} was not removed: {e}",
                self.0.display()
            );
        }
    }
}

/// A running owner with the test's ends of its channels.
struct Owner {
    child: Child,
    requests: Option<PipeWriter>,
    frames: Receiver<Vec<u8>>,
    worker: Option<PipeWriter>,
    worker_frames: Option<Receiver<Vec<u8>>>,
}

/// Hands each frame that the owner writes on `from` to the receiver, until the end of the stream.
fn frames(mut from: PipeReader) -> Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        while let Ok(Some(frame)) = read_frame(&mut from, SERVER_FRAME_MAX) {
            if tx.send(frame).is_err() {
                return;
            }
        }
    });
    rx
}

/// The four ends of a starter's channel: the end that the owner reads and the end that it writes, then the end that
/// the test writes and the end that it reads.
type Channel = ((PipeReader, PipeWriter), (PipeWriter, PipeReader));

/// A starter's channel of two pipes.
fn pipes() -> Channel {
    let (owner_reads, requests) = std::io::pipe().unwrap();
    let (replies, owner_writes) = std::io::pipe().unwrap();
    ((owner_reads, owner_writes), (requests, replies))
}

/// A starter's channel on which an end of a stream travels: one socket pair, whose one end and a copy of that end by
/// `dup` are the two ends of the owner, because the owner refuses two ends of one number, and whose other end and
/// its copy are the two ends of the test.
#[cfg(unix)]
fn stream_channel() -> Channel {
    use std::os::fd::OwnedFd;
    use std::os::unix::net::UnixStream;
    let (owner, test) = UnixStream::pair().unwrap();
    let copy = |end: &UnixStream| OwnedFd::from(end.try_clone().unwrap());
    (
        (copy(&owner).into(), OwnedFd::from(owner).into()),
        (copy(&test).into(), OwnedFd::from(test).into()),
    )
}

/// A starter's channel on which an end of a stream travels: on Windows the frame names the two handles of the end
/// by their values, so the channel is two pipes.
#[cfg(windows)]
fn stream_channel() -> Channel {
    pipes()
}

/// Starts the built owner on `dir` with `key` on standard input, the lowest live ordinal `first_live`, the
/// starter's channel and, where `worker` holds, the worker's pipe pair.
fn start(dir: &Path, key: [u8; 32], first_live: u64, worker: bool, extra: &[&str]) -> Owner {
    start_on(pipes(), dir, key, first_live, worker, extra)
}

/// Starts the owner as `start` does, with `channel` as the starter's channel.
fn start_on(
    channel: Channel,
    dir: &Path,
    key: [u8; 32],
    first_live: u64,
    worker: bool,
    extra: &[&str],
) -> Owner {
    start_with(
        channel,
        dir,
        key,
        first_live,
        worker,
        extra,
        Stdio::inherit(),
    )
}

/// Starts the owner as `start_on` does, with `stderr` as its standard error.
fn start_with(
    channel: Channel,
    dir: &Path,
    key: [u8; 32],
    first_live: u64,
    worker: bool,
    extra: &[&str],
    stderr: Stdio,
) -> Owner {
    let ((owner_reads, owner_writes), (requests, replies)) = channel;
    let mut ends = vec![(owner_reads, owner_writes)];
    let mut test_ends = None;
    if worker {
        let (owner_reads, writes) = std::io::pipe().unwrap();
        let (reads, owner_writes) = std::io::pipe().unwrap();
        ends.push((owner_reads, owner_writes));
        test_ends = Some((writes, reads));
    }
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_halvo-owner"));
    cmd.arg(format!("--data-dir={}", dir.display()))
        .arg(format!("--first-live={first_live}"))
        .args(extra)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(stderr);
    let mut child = launch(&mut cmd, ends);
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(&key).unwrap();
    drop(stdin);
    let (worker, worker_frames) = match test_ends {
        Some((writes, reads)) => (Some(writes), Some(frames(reads))),
        None => (None, None),
    };
    Owner {
        child,
        requests: Some(requests),
        frames: frames(replies),
        worker,
        worker_frames,
    }
}

/// Starts `cmd` with the owner's ends of its channels at the descriptors 3 and 4, and 5 and 6 for the worker's.
#[cfg(unix)]
fn launch(cmd: &mut Command, ends: Vec<(PipeReader, PipeWriter)>) -> Child {
    use std::os::fd::AsRawFd;
    use std::os::unix::process::CommandExt;
    let mut places = Vec::new();
    for (n, (reads, writes)) in ends.iter().enumerate() {
        let r = 3 + 2 * i32::try_from(n).unwrap();
        let name = if n == 0 { "channel" } else { "worker" };
        cmd.arg(format!("--{name}={r},{}", r + 1));
        places.push((reads.as_raw_fd(), r));
        places.push((writes.as_raw_fd(), r + 1));
    }
    // SAFETY: the closure runs in the child between fork and exec and calls fcntl and dup2 alone, which are
    // async-signal-safe; it first copies each end above 9, so that no dup2 writes over an end before it is copied.
    unsafe {
        cmd.pre_exec(move || {
            let mut high = [0; 4];
            for (at, (from, _)) in places.iter().enumerate() {
                high[at] = libc::fcntl(*from, libc::F_DUPFD_CLOEXEC, 10);
                if high[at] < 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            for (at, (_, to)) in places.iter().enumerate() {
                if libc::dup2(high[at], *to) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
    let child = cmd.spawn().unwrap();
    drop(ends);
    child
}

/// Starts `cmd` with the owner's ends of its channels made inheritable and named by their handle values. One lock
/// keeps every start of this binary apart, so that no other child inherits these ends.
#[cfg(windows)]
fn launch(cmd: &mut Command, ends: Vec<(PipeReader, PipeWriter)>) -> Child {
    use std::os::windows::io::AsRawHandle;
    use std::sync::Mutex;
    use windows_sys::Win32::Foundation::{SetHandleInformation, HANDLE_FLAG_INHERIT};
    static STARTS: Mutex<()> = Mutex::new(());
    let held = STARTS.lock().unwrap_or_else(|e| e.into_inner());
    for (n, (reads, writes)) in ends.iter().enumerate() {
        let (r, w) = (reads.as_raw_handle(), writes.as_raw_handle());
        for handle in [r, w] {
            // SAFETY: the handle is an open end that this process owns; the call sets its inherit flag alone.
            let set =
                unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) };
            assert_ne!(set, 0, "{}", std::io::Error::last_os_error());
        }
        let name = if n == 0 { "channel" } else { "worker" };
        cmd.arg(format!("--{name}={},{}", r as usize, w as usize));
    }
    let child = cmd.spawn();
    drop(ends);
    drop(held);
    child.unwrap()
}

fn is_report(frame: &[u8]) -> Option<Value> {
    let value: Value = serde_json::from_slice(frame).unwrap();
    let object = value.as_object()?;
    (object.len() == 1 && object.contains_key("report")).then(|| value["report"].clone())
}

impl Owner {
    /// Sends one request on the starter's channel.
    fn send(&mut self, request: &[u8]) {
        let requests = self.requests.as_mut().unwrap();
        write_frame(requests, request, CLIENT_FRAME_MAX).unwrap();
    }

    /// Sends one request on the worker's channel.
    fn write(&mut self, request: &[u8]) {
        let worker = self.worker.as_mut().unwrap();
        write_frame(worker, request, CLIENT_FRAME_MAX).unwrap();
    }

    /// The next frame on the starter's channel that is no report.
    fn reply(&mut self) -> Reply {
        loop {
            let frame = self
                .frames
                .recv_timeout(WAIT)
                .expect("the owner sends no reply");
            if is_report(&frame).is_none() {
                return decode_reply(&frame).unwrap();
            }
        }
    }

    /// The owner's answer to a frame `stream`: the text of the next frame on the starter's channel that is no report.
    fn stream_answer(&mut self) -> String {
        loop {
            let frame = self
                .frames
                .recv_timeout(WAIT)
                .expect("the owner answers no frame `stream`");
            if is_report(&frame).is_none() {
                return String::from_utf8(frame).unwrap();
            }
        }
    }

    /// The next frame on the worker's channel.
    fn worker_reply(&mut self) -> Reply {
        let frames = self.worker_frames.as_ref().unwrap();
        let frame = frames
            .recv_timeout(WAIT)
            .expect("the owner sends no reply to the worker");
        decode_reply(&frame).unwrap()
    }

    /// The next report on the starter's channel for which `wanted` holds, read before `deadline`, with the text of
    /// its frame, whose keys stand in the order in which the owner wrote them.
    fn report_where(
        &mut self,
        deadline: Mono,
        wanted: impl Fn(&Value) -> bool,
    ) -> Option<(Value, String)> {
        loop {
            let left = deadline.saturating_duration_since(Mono::now());
            let frame = self.frames.recv_timeout(left).ok()?;
            if let Some(report) = is_report(&frame).filter(|report| wanted(report)) {
                return Some((report, String::from_utf8(frame).unwrap()));
            }
        }
    }

    /// Ends the starter's channel: the owner reads its end.
    fn close(&mut self) {
        self.requests = None;
    }

    /// The exit code of the owner within `within`, or none.
    fn exit_code(&mut self, within: Duration) -> Option<i32> {
        let deadline = Mono::now() + within;
        while Mono::now() < deadline {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status.code();
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        None
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// The frame `stream` that names no handle: on macOS the end comes beside it.
const STREAM_BESIDE: &[u8] = br#"{"stream":{"handles":null}}"#;
/// The three answers of the owner to a frame `stream`.
const TAKEN: &str = r#"{"stream":"taken"}"#;
const FULL: &str = r#"{"stream":"full"}"#;
const REFUSED: &str = r#"{"stream":"refused"}"#;

/// The test's end of a stream.
struct Stream {
    #[cfg(unix)]
    writes: std::os::unix::net::UnixStream,
    #[cfg(windows)]
    writes: PipeWriter,
    /// Each frame that the owner writes on the stream, and none at its end-of-file.
    frames: Receiver<Option<Vec<u8>>>,
}

/// Hands each frame that the owner writes on a stream to the receiver, and none when the stream reads end-of-file.
/// A read that fails ends the thread with nothing handed over.
fn stream_frames(mut from: impl Read + Send + 'static) -> Receiver<Option<Vec<u8>>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        while let Ok(read) = read_frame(&mut from, SERVER_FRAME_MAX) {
            let last = read.is_none();
            if tx.send(read).is_err() || last {
                return;
            }
        }
    });
    rx
}

impl Stream {
    /// The test's end of a stream as the hand-over returns it, with a thread that reads its frames.
    #[cfg(unix)]
    fn over(end: Handed) -> Stream {
        Stream {
            frames: stream_frames(end.try_clone().unwrap()),
            writes: end,
        }
    }

    #[cfg(windows)]
    fn over((reads, writes): Handed) -> Stream {
        Stream {
            frames: stream_frames(reads),
            writes,
        }
    }

    /// Sends one request on the stream.
    fn send(&mut self, request: &[u8]) {
        write_frame(&mut self.writes, request, CLIENT_FRAME_MAX).unwrap();
    }

    /// The next reply on the stream.
    fn reply(&mut self) -> Reply {
        let frame = self
            .frames
            .recv_timeout(WAIT)
            .expect("the owner sends no reply on the stream")
            .expect("the stream ended before its reply");
        decode_reply(&frame).unwrap()
    }

    /// Whether the next thing that the test's end reads is the end-of-file of the stream.
    fn ends(&mut self) -> bool {
        self.frames.recv_timeout(WAIT) == Ok(None)
    }
}

/// Drops the test's end: the socket is shut down, so that the copy of it in the test's reader thread ends too and
/// the owner reads end-of-file. On Windows the two pipe handles of the test's end close as they are dropped.
#[cfg(unix)]
impl Drop for Stream {
    fn drop(&mut self) {
        let _ = self.writes.shutdown(std::net::Shutdown::Both);
    }
}

/// The test's end of a stream, as the hand-over returns it: a socket on macOS; on Windows the pipe that reads and the
/// pipe that writes.
#[cfg(unix)]
type Handed = std::os::unix::net::UnixStream;
#[cfg(windows)]
type Handed = (PipeReader, PipeWriter);

/// Makes a new stream, hands the owner one end of it, and returns the test's end, with a thread that reads its
/// frames, and the owner's answer.
fn hand_over(owner: &mut Owner) -> (Stream, String) {
    let (end, answer) = handed(owner);
    (Stream::over(end), answer)
}

/// Makes a new stream, a socket pair, hands the owner one end of it as one descriptor in one SCM_RIGHTS message on
/// the call that writes the frame `stream` on the starter's channel, and returns the test's end with the owner's
/// answer. The test keeps its own descriptor of the owner's end until it has read the answer, as a starter does that
/// closes what it holds after the answer: closed while the message is in flight, it leaves the owner on macOS a
/// socket on which the first write of the test's end fails with a broken pipe.
#[cfg(unix)]
fn handed(owner: &mut Owner) -> (Handed, String) {
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixStream;
    let (mine, owners) = UnixStream::pair().unwrap();
    let mut frame = u32::try_from(STREAM_BESIDE.len())
        .unwrap()
        .to_be_bytes()
        .to_vec();
    frame.extend_from_slice(STREAM_BESIDE);
    let mut iov = libc::iovec {
        iov_base: frame.as_mut_ptr().cast(),
        iov_len: frame.len(),
    };
    let one = u32::try_from(size_of::<libc::c_int>()).unwrap();
    // SAFETY: the macro computes the size of a control message from the length of its data.
    let space = unsafe { libc::CMSG_SPACE(one) } as usize;
    let mut control = vec![0u64; space.div_ceil(8)];
    // SAFETY: an all-zero msghdr is a valid value of the type, whose pointers are then null.
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.as_mut_ptr().cast();
    msg.msg_controllen = space as _;
    // SAFETY: `control` holds the bytes of one header and one descriptor, aligned for a header; the macros of the
    // system give the header and the place of its data inside those bytes.
    unsafe {
        let header = libc::CMSG_FIRSTHDR(&msg);
        (*header).cmsg_level = libc::SOL_SOCKET;
        (*header).cmsg_type = libc::SCM_RIGHTS;
        (*header).cmsg_len = libc::CMSG_LEN(one) as _;
        libc::CMSG_DATA(header)
            .cast::<libc::c_int>()
            .write_unaligned(owners.as_raw_fd());
    }
    let channel = owner.requests.as_ref().unwrap().as_raw_fd();
    // SAFETY: sendmsg reads the frame and the control message, which both live through the call, and the two
    // descriptors are open.
    let sent = unsafe { libc::sendmsg(channel, &msg, 0) };
    assert_eq!(
        usize::try_from(sent).ok(),
        Some(frame.len()),
        "{}",
        std::io::Error::last_os_error()
    );
    let answer = owner.stream_answer();
    drop(owners);
    (mine, answer)
}

/// Makes a new stream, two pipes, copies the owner's two ends of it into the owner's process, names the two copies
/// by their values in the frame `stream` on the starter's channel, and returns the test's end with the owner's
/// answer.
#[cfg(windows)]
fn handed(owner: &mut Owner) -> (Handed, String) {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::{DuplicateHandle, DUPLICATE_SAME_ACCESS, HANDLE};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let (owner_reads, writes) = std::io::pipe().unwrap();
    let (reads, owner_writes) = std::io::pipe().unwrap();
    let there = owner.child.as_raw_handle() as HANDLE;
    let give = |end: HANDLE| {
        let mut value: HANDLE = std::ptr::null_mut();
        // SAFETY: `end` is an open handle of this process and `there` is the handle of the owner's process, which
        // the child keeps open; the call writes the one handle value that the pointer names.
        let made = unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                end,
                there,
                &mut value,
                0,
                0,
                DUPLICATE_SAME_ACCESS,
            )
        };
        assert_ne!(made, 0, "{}", std::io::Error::last_os_error());
        value as usize
    };
    let read_value = give(owner_reads.as_raw_handle() as HANDLE);
    let write_value = give(owner_writes.as_raw_handle() as HANDLE);
    drop((owner_reads, owner_writes));
    owner.send(format!(r#"{{"stream":{{"handles":[{read_value},{write_value}]}}}}"#).as_bytes());
    ((reads, writes), owner.stream_answer())
}

/// The test's ends of the starter's channel, as the daemon's handler of `owner_stream` uses them.
#[cfg(any(target_os = "macos", windows))]
struct Starter<'a>(&'a mut Owner);

#[cfg(any(target_os = "macos", windows))]
impl halvo_control::r2::OwnerChannel for Starter<'_> {
    #[cfg(target_os = "macos")]
    fn send_stream(
        &mut self,
        frame: &[u8],
        end: std::os::fd::BorrowedFd<'_>,
    ) -> std::io::Result<()> {
        use std::os::fd::AsFd;
        let channel = self.0.requests.as_ref().unwrap().as_fd();
        halvo_control::r2::send_with_end(channel, frame, end)
    }

    #[cfg(windows)]
    fn owner_process(&self) -> std::os::windows::io::BorrowedHandle<'_> {
        use std::os::windows::io::AsHandle;
        self.0.child.as_handle()
    }

    #[cfg(windows)]
    fn send_stream(&mut self, frame: &[u8]) -> std::io::Result<()> {
        self.0.send(frame);
        Ok(())
    }

    fn send(&mut self, frame: &[u8]) -> std::io::Result<()> {
        self.0.send(frame);
        Ok(())
    }

    fn answer(&mut self, within: Duration) -> std::io::Result<Option<Vec<u8>>> {
        let deadline = Mono::now() + within;
        loop {
            let left = deadline.saturating_duration_since(Mono::now());
            match self.0.frames.recv_timeout(left) {
                Ok(frame) if is_report(&frame).is_some() => {}
                Ok(frame) => return Ok(Some(frame)),
                Err(RecvTimeoutError::Timeout) => return Err(std::io::ErrorKind::TimedOut.into()),
                Err(RecvTimeoutError::Disconnected) => return Ok(None),
            }
        }
    }
}

/// A connector of the test that gives the client call the client's end of the control connection, once.
#[cfg(any(target_os = "macos", windows))]
struct Once<T>(std::sync::Mutex<Option<T>>);

#[cfg(any(target_os = "macos", windows))]
impl<T: halvo_control::client::Transport> halvo_control::client::Connector for Once<T> {
    type Transport = T;

    fn endpoint(&self) -> String {
        "the test's control connection".to_owned()
    }

    fn connect(&self, _: Duration) -> Result<T, halvo_control::client::ClientError> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .take()
            .expect("the client call connects once"))
    }
}

/// The two ends of a control connection: the client's, then the server's.
#[cfg(target_os = "macos")]
fn control_connection() -> (
    std::os::unix::net::UnixStream,
    std::os::unix::net::UnixStream,
) {
    std::os::unix::net::UnixStream::pair().unwrap()
}

/// The client's end of a control connection on Windows: one end of a named pipe.
#[cfg(windows)]
struct PipeEnd(std::fs::File);

#[cfg(windows)]
impl Read for PipeEnd {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buf)
    }
}

#[cfg(windows)]
impl Write for PipeEnd {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

#[cfg(windows)]
impl std::os::windows::io::AsHandle for PipeEnd {
    fn as_handle(&self) -> std::os::windows::io::BorrowedHandle<'_> {
        self.0.as_handle()
    }
}

#[cfg(windows)]
impl halvo_control::client::Transport for PipeEnd {
    /// A pipe opened for synchronous use takes no timeout; the waits of the test bound the call.
    fn set_timeout(&mut self, _: Duration) -> std::io::Result<()> {
        Ok(())
    }

    /// A pipe has no half to close; on Windows the handler holds no copy of the client's end and waits for no end.
    fn end_request(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The two ends of a control connection: the client's and the server's end of a new named pipe that no other machine
/// may open.
#[cfg(windows)]
fn control_connection() -> (PipeEnd, std::fs::File) {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::FromRawHandle;
    // The test's own declaration: the windows-sys features of this crate do not hold the pipe functions.
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateNamedPipeW(
            name: *const u16,
            open_mode: u32,
            pipe_mode: u32,
            instances: u32,
            out_size: u32,
            in_size: u32,
            wait_ms: u32,
            security: *const std::ffi::c_void,
        ) -> *mut std::ffi::c_void;
    }
    const PIPE_ACCESS_DUPLEX: u32 = 0x3;
    const FILE_FLAG_FIRST_PIPE_INSTANCE: u32 = 0x0008_0000;
    const PIPE_REJECT_REMOTE_CLIENTS: u32 = 0x8;
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let name = format!(
        r"\\.\pipe\halvo-owner-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    let wide: Vec<u16> = std::ffi::OsStr::new(&name)
        .encode_wide()
        .chain(Some(0))
        .collect();
    // SAFETY: the name is a wide string with its NUL that lives through the call; with no security attributes the
    // handle is not inheritable.
    let server = unsafe {
        CreateNamedPipeW(
            wide.as_ptr(),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_REJECT_REMOTE_CLIENTS,
            1,
            65_536,
            65_536,
            0,
            std::ptr::null(),
        )
    };
    assert_ne!(
        server.addr(),
        usize::MAX,
        "{}",
        std::io::Error::last_os_error()
    );
    // SAFETY: the handle is new and open, and nothing else owns it.
    let server = unsafe { std::fs::File::from_raw_handle(server) };
    let client = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&name)
        .unwrap();
    (PipeEnd(client), server)
}

/// Serves one request on the server's end of the control connection as the daemon's server does: it reads the frame,
/// decodes it and hands the unknown request to `dispatch_unknown` with the test's ends of the starter's channel.
/// Returns whether the call wrote the reply.
#[cfg(target_os = "macos")]
fn serve_one(server: &std::os::unix::net::UnixStream, owner: &mut Owner) -> bool {
    let frame =
        halvo_control::wire::read_frame(&mut &*server, halvo_control::wire::REQUEST_MAX_BYTES)
            .unwrap();
    let Ok(halvo_control::proto::Decoded::Unknown(request)) =
        halvo_control::proto::decode_request(&frame)
    else {
        panic!("the request is no body of version 2");
    };
    halvo_control::r2::dispatch_unknown(&request, server, &mut Starter(owner))
}

/// Serves one request as the macOS form does, on the server's end of a named pipe.
#[cfg(windows)]
fn serve_one(server: &std::fs::File, owner: &mut Owner) -> bool {
    use std::os::windows::io::AsHandle;
    let frame =
        halvo_control::wire::read_frame(&mut &*server, halvo_control::wire::REQUEST_MAX_BYTES)
            .unwrap();
    let Ok(halvo_control::proto::Decoded::Unknown(request)) =
        halvo_control::proto::decode_request(&frame)
    else {
        panic!("the request is no body of version 2");
    };
    halvo_control::r2::dispatch_unknown(&request, server.as_handle(), &mut Starter(owner))
}

/// The test's `Stream` over the end that the client call returned.
#[cfg(target_os = "macos")]
fn stream_of(end: halvo_control::r2::OwnerStreamEnd) -> Stream {
    let mine = std::os::unix::net::UnixStream::from(std::os::fd::OwnedFd::from(end));
    Stream {
        frames: stream_frames(mine.try_clone().unwrap()),
        writes: mine,
    }
}

/// The test's `Stream` over the end that the client call returned: the handle that reads, then the one that writes.
#[cfg(windows)]
fn stream_of(end: halvo_control::r2::OwnerStreamEnd) -> Stream {
    use std::os::windows::io::OwnedHandle;
    let (reads, writes): (OwnedHandle, OwnedHandle) = end.into();
    Stream {
        frames: stream_frames(PipeReader::from(reads)),
        writes: PipeWriter::from(writes),
    }
}

/// Rulings 3 and 4 of the daemon half (contracts.md C24.13 items 5 and 6 of the workflows run): `dispatch_unknown`
/// makes a stream, hands the owner one end with the frame `stream` and, on `taken`, the client the other end with the
/// reply; the end that `owner_stream` returns reaches the owner, which answers a `runner` request on it.
#[cfg(any(target_os = "macos", windows))]
#[test]
fn the_daemon_half_hands_both_ends_over() {
    let scratch = Scratch::new("owner-stream-daemon-half");
    let mut owner = start_on(stream_channel(), &scratch.0, KEY, 1, false, &[]);
    let (client_end, server) = control_connection();
    let client = halvo_control::client::Client::new(Once(std::sync::Mutex::new(Some(client_end))));
    let (served, end) = std::thread::scope(|scope| {
        let served = scope.spawn(|| serve_one(&server, &mut owner));
        let end = client.owner_stream();
        (served.join().unwrap(), end)
    });
    assert!(served, "dispatch_unknown wrote no reply");
    let mut stream = stream_of(end.unwrap());
    stream.send(&runner_request());
    assert_eq!(stream.reply().outcome.unwrap_err(), ErrorName::Malformed);
}

/// The relay (contracts.md C24.4 of the workflows run): an interface read, encoded by `Relay::encode`, goes through
/// `dispatch_unknown` to the owner on the starter's channel as the object that the handler writes again, and the reply
/// that comes back in `relayed` is the owner's answer to that read. The server's end closes after the reply, as the
/// daemon's server closes it, so the client call reads the end of the stream.
#[cfg(any(target_os = "macos", windows))]
#[test]
fn a_relayed_interface_read_reaches_the_store() {
    let scratch = Scratch::new("relay-daemon-half");
    let mut owner = start(&scratch.0, KEY, 1, false, &[]);
    let (client_end, server) = control_connection();
    let client = halvo_control::client::Client::new(Once(std::sync::Mutex::new(Some(client_end))));
    let id = request_id();
    let read = InterfaceRead::workflows(&InterfaceRole::claim(), id.clone(), None)
        .to_bytes()
        .unwrap();
    let (served, reply) = std::thread::scope(|scope| {
        let owner = &mut owner;
        let served = scope.spawn(move || {
            let served = serve_one(&server, owner);
            drop(server);
            served
        });
        let reply = client.relay(&read);
        (served.join().unwrap(), reply)
    });
    assert!(served, "dispatch_unknown wrote no reply");
    let reply = decode_reply(&reply.unwrap()).unwrap();
    assert_eq!(reply.id, Some(id));
    assert_eq!(workflows(reply), []);
}

fn request_id() -> RequestId {
    static NEXT: AtomicU8 = AtomicU8::new(1);
    RequestId::from_random([NEXT.fetch_add(1, Ordering::Relaxed); 16])
}

/// A `runner` request, with a body that is no runner call: the runner handler of the owner's step list answers it
/// `malformed`.
fn runner_request() -> Vec<u8> {
    let id = serde_json::to_string(&request_id()).unwrap();
    format!(r#"{{"v":1,"id":{id},"request":"runner","body":{{"probe":1}}}}"#).into_bytes()
}

fn read_workflows() -> Vec<u8> {
    InterfaceRead::workflows(&InterfaceRole::claim(), request_id(), None)
        .to_bytes()
        .unwrap()
}

fn read_evidence(workflow: &WorkflowId) -> Vec<u8> {
    InterfaceRead::evidence(
        &InterfaceRole::claim(),
        request_id(),
        workflow.clone(),
        None,
    )
    .to_bytes()
    .unwrap()
}

/// The store id that `analysis_write` names.
fn analysis_store() -> String {
    "1".repeat(32)
}

fn analysis_write(episodes: Vec<Episode>, commit: Position) -> Vec<u8> {
    let store = StoreId::parse(&analysis_store()).unwrap();
    let mut write = AnalysisWrite::new(&AnalysisRole::claim(), request_id(), store);
    for episode in episodes {
        write.push(episode).unwrap();
    }
    write.commit(commit);
    write.to_bytes().unwrap()
}

fn now_ns() -> i64 {
    let since = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    i64::try_from(since.as_nanos()).unwrap()
}

/// Two days before the UTC day of now, a day that every local day of the owner keeps inside the bound.
fn two_days_ago() -> u32 {
    u32::try_from(now_ns().div_euclid(DAY_NS)).unwrap() - 2
}

/// An episode of the segment `n`, with the summary id of `n`, of one key for every `n`.
fn episode(n: u8, day: u32, expires: i64) -> Episode {
    let start = i64::from(day) * DAY_NS + 64_800_000_000_000 + i64::from(n);
    Episode {
        id: SummaryId::from_random([n; 16]),
        category: SeedCategory::UtilityBillPayment,
        site: None,
        app: AppId::parse("com.example.browser").unwrap(),
        day,
        part: Part::Evening,
        minutes: 5,
        keys: None,
        pointer: None,
        segments: vec![Ordinal::new(u64::from(n)).unwrap()],
        expires: Instant(expires),
        start: Instant(start),
        end: Instant(start + 1_000),
        steps: vec![ObservedStep {
            app: None,
            title: Some(Tagged {
                value: Text::parse("Pay bill").unwrap(),
                layer: FloorLayer::Floor,
            }),
            input: None,
            site: None,
            text: None,
        }],
    }
}

fn workflows(reply: Reply) -> Vec<WorkflowId> {
    match reply.outcome {
        Ok(ReplyResult::Workflows(page)) => page
            .workflows
            .iter()
            .map(|item| item.workflow.id().clone())
            .collect(),
        other => panic!("the read of the workflows answers {other:?}"),
    }
}

fn evidence_segments(reply: Reply) -> Vec<Vec<u64>> {
    match reply.outcome {
        Ok(ReplyResult::Evidence(page)) => page
            .evidence
            .iter()
            .map(|row| row.segments.iter().map(|o| o.get()).collect())
            .collect(),
        other => panic!("the read of the evidence answers {other:?}"),
    }
}

/// Rule 1: the owner opens the store with the key on standard input; an owner with another key answers no request
/// and exits with the code of the refusal, and the store opens with the first key again.
#[test]
fn the_owner_opens_with_the_key_on_standard_input_and_refuses_another() {
    let scratch = Scratch::new("owner-key");

    let mut first = start(&scratch.0, KEY, 1, false, &[]);
    first.send(&read_workflows());
    assert_eq!(workflows(first.reply()), []);
    first.close();
    assert_eq!(first.exit_code(WAIT), Some(exit::END));
    assert!(scratch.0.join("derived").is_dir());

    let mut other = start(&scratch.0, OTHER_KEY, 1, false, &[]);
    let _ = write_frame(
        other.requests.as_mut().unwrap(),
        &read_workflows(),
        CLIENT_FRAME_MAX,
    );
    assert_eq!(other.exit_code(WAIT), Some(exit::SEALED));
    assert_eq!(
        other.frames.recv_timeout(WAIT),
        Err(RecvTimeoutError::Disconnected),
        "the owner with another key writes nothing"
    );

    let mut third = start(&scratch.0, KEY, 1, false, &[]);
    third.send(&read_workflows());
    assert_eq!(workflows(third.reply()), []);
    third.close();
    assert_eq!(third.exit_code(WAIT), Some(exit::END));
}

/// Rule 3 and the idle half of rule 5: with the lowest live ordinal 2 the owner removes the evidence row of the
/// segment 1 before its first read, and with its channel open and no request it exits 0 after its idle time.
#[test]
fn the_owner_purges_below_the_lowest_live_ordinal_before_its_first_read() {
    let scratch = Scratch::new("owner-purge");
    let day = two_days_ago();

    let mut first = start(&scratch.0, KEY, 1, true, &[]);
    first.write(&analysis_write(
        vec![
            episode(1, day, FAR),
            episode(2, day, FAR),
            episode(3, day, FAR),
        ],
        Position { seg: 4, idx: 0 },
    ));
    assert!(matches!(
        first.worker_reply().outcome,
        Ok(ReplyResult::Null)
    ));
    first.send(&read_workflows());
    let ids = workflows(first.reply());
    assert_eq!(
        ids.len(),
        1,
        "the three summaries of one key make one workflow"
    );
    first.send(&read_evidence(&ids[0]));
    assert_eq!(evidence_segments(first.reply()), [[1], [2], [3]]);
    first.close();
    assert_eq!(first.exit_code(WAIT), Some(exit::END));

    let mut second = start(&scratch.0, KEY, 2, false, &["--idle-ms=1000"]);
    second.send(&read_evidence(&ids[0]));
    assert_eq!(evidence_segments(second.reply()), [[2], [3]]);
    assert_eq!(second.exit_code(WAIT), Some(exit::END));
    assert!(
        second.requests.is_some(),
        "the channel was open when the owner exited"
    );
}

/// Rule 4 and the other half of rule 5: a report with the four values follows a commit; with no request the owner
/// runs the expiry at its check and reports the window row gone; after the end of its channel it writes a last report
/// and exits 0.
#[test]
fn the_owner_reports_each_commit_expires_by_itself_and_reports_before_it_exits() {
    let scratch = Scratch::new("owner-expiry");
    let mut owner = start(
        &scratch.0,
        KEY,
        1,
        true,
        &["--check-ms=1000", "--idle-ms=60000"],
    );
    let opened = owner.report_where(Mono::now() + WAIT, |_| true);
    assert!(opened.is_some(), "the owner reports its open");

    let expires = now_ns() + 2_000_000_000;
    owner.write(&analysis_write(
        vec![episode(1, two_days_ago(), expires)],
        Position { seg: 2, idx: 0 },
    ));
    assert!(matches!(
        owner.worker_reply().outcome,
        Ok(ReplyResult::Null)
    ));
    let (committed, text) = owner
        .report_where(Mono::now() + WAIT, |r| r["counts"]["window"] == 1)
        .expect("no report shows the committed window row");
    assert!(
        text.starts_with(r#"{"report":{"revisions":{"workflow":"#),
        "{text}"
    );
    assert!(
        text.contains(r#"},"counts":{"window":1,"bounded":1,"kept":0},"disk_bytes":"#),
        "{text}"
    );
    let families = committed["revisions"].as_object().unwrap();
    assert!(families.values().all(Value::is_u64));
    assert!(committed["disk_bytes"].as_u64().unwrap() > 0);
    assert_eq!(committed["expiry"], expires);

    let expired = owner.report_where(Mono::now() + Duration::from_secs(10), |r| {
        r["counts"]["window"] == 0
    });
    assert!(
        expired.is_some(),
        "no report of the expiry within ten seconds"
    );

    owner.close();
    let mut last = None;
    while let Ok(frame) = owner.frames.recv_timeout(WAIT) {
        last = Some(is_report(&frame).expect("a frame after the end of the channel is no report"));
    }
    let last = last.expect("no last report after the end of the channel");
    assert_eq!(last["counts"]["window"], 0);
    assert_eq!(owner.exit_code(WAIT), Some(exit::END));
}

/// Ruling 1 of the position: the first report of an owner over an empty directory ends with `position` null.
#[test]
fn a_new_store_reports_no_position() {
    let scratch = Scratch::new("owner-position-new");
    let mut owner = start(&scratch.0, KEY, 1, false, &[]);
    let (opened, text) = owner
        .report_where(Mono::now() + WAIT, |_| true)
        .expect("the owner reports its open");
    assert_eq!(opened.get("position"), Some(&Value::Null), "{text}");
    assert!(text.ends_with(r#","position":null}}"#), "{text}");
    owner.close();
    assert_eq!(owner.exit_code(WAIT), Some(exit::END));
}

/// Ruling 2 of the position: the report that an owner writes before its first request holds the store id and the
/// position that an earlier owner committed over the same directory.
#[test]
fn a_second_owner_reports_the_position_at_its_start() {
    let scratch = Scratch::new("owner-position-start");
    let mut first = start(&scratch.0, KEY, 1, true, &[]);
    first.write(&analysis_write(
        vec![episode(1, two_days_ago(), FAR)],
        Position { seg: 2, idx: 0 },
    ));
    assert!(matches!(
        first.worker_reply().outcome,
        Ok(ReplyResult::Null)
    ));
    first.close();
    assert_eq!(first.exit_code(WAIT), Some(exit::END));

    let mut second = start(&scratch.0, KEY, 1, false, &[]);
    let (opened, text) = second
        .report_where(Mono::now() + WAIT, |_| true)
        .expect("the second owner reports its open");
    assert_eq!(
        opened.get("position"),
        Some(&serde_json::json!({"store": analysis_store(), "seg": 2, "idx": 0})),
        "{text}"
    );
    second.close();
    assert_eq!(second.exit_code(WAIT), Some(exit::END));
}

/// Ruling 3 of the position: the report of a committed batch ends with the store id of that batch and its commit
/// position.
#[test]
fn the_report_of_a_commit_ends_with_its_position() {
    let scratch = Scratch::new("owner-position-commit");
    let mut owner = start(&scratch.0, KEY, 1, true, &[]);
    owner.write(&analysis_write(
        vec![episode(1, two_days_ago(), FAR)],
        Position { seg: 2, idx: 0 },
    ));
    assert!(matches!(
        owner.worker_reply().outcome,
        Ok(ReplyResult::Null)
    ));
    let (_, text) = owner
        .report_where(Mono::now() + WAIT, |r| r["counts"]["window"] == 1)
        .expect("no report shows the committed window row");
    let tail = format!(
        r#","position":{{"store":"{}","seg":2,"idx":0}}}}}}"#,
        analysis_store()
    );
    assert!(text.ends_with(&tail), "{text}");
    owner.close();
    assert_eq!(owner.exit_code(WAIT), Some(exit::END));
}

/// The ruling on a malformed row of `meta`: an owner commits a batch whose three summaries make one workflow, and the
/// test then makes the row of `meta` malformed through a connection of its own with the owner's key, in two forms one
/// after the other: a store id that is no store id, then a store text that is not UTF-8 (the byte FF, the form of
/// verifier-67). Over each form an owner stays up: its first report holds `position` null, it answers the read of the
/// workflows with the one workflow, it exits 0 at the end of its channel, and its standard error holds one line for
/// the row and no value of it.
#[test]
fn a_malformed_meta_row_keeps_the_owner_up() {
    let scratch = Scratch::new("owner-position-malformed");
    let day = two_days_ago();
    let mut first = start(&scratch.0, KEY, 1, true, &[]);
    first.write(&analysis_write(
        vec![
            episode(1, day, FAR),
            episode(2, day, FAR),
            episode(3, day, FAR),
        ],
        Position { seg: 4, idx: 0 },
    ));
    assert!(matches!(
        first.worker_reply().outcome,
        Ok(ReplyResult::Null)
    ));
    first.send(&read_workflows());
    let found = workflows(first.reply());
    assert_eq!(found.len(), 1);
    first.close();
    assert_eq!(first.exit_code(WAIT), Some(exit::END));

    halvo_sqlcipher::initialize().unwrap();
    let hex: String = KEY.iter().map(|b| format!("{b:02x}")).collect();
    for plant in [
        "UPDATE meta SET store = 'no-store-id-424242', seg = 424243, idx = 424244 WHERE one = 1",
        "UPDATE meta SET store = CAST(x'ff' AS TEXT) WHERE one = 1",
    ] {
        let conn = Connection::open(scratch.0.join("derived").join("derived.db")).unwrap();
        conn.execute_batch(&format!("PRAGMA key = \"x'{hex}'\";"))
            .unwrap();
        assert_eq!(conn.execute(plant, []).unwrap(), 1, "{plant}");
        drop(conn);

        let mut owner = start_with(pipes(), &scratch.0, KEY, 1, false, &[], Stdio::piped());
        let mut stderr = owner.child.stderr.take().unwrap();
        let (opened, text) = owner
            .report_where(Mono::now() + WAIT, |_| true)
            .unwrap_or_else(|| panic!("after {plant} the owner reports no open"));
        assert_eq!(
            opened.get("position"),
            Some(&Value::Null),
            "{plant}: {text}"
        );
        owner.send(&read_workflows());
        assert_eq!(workflows(owner.reply()), found, "{plant}");
        owner.close();
        assert_eq!(owner.exit_code(WAIT), Some(exit::END), "{plant}");
        let mut said = Vec::new();
        stderr.read_to_end(&mut said).unwrap();
        assert!(!said.contains(&0xff), "{plant}: {said:?}");
        let said = String::from_utf8(said).unwrap();
        let lines: Vec<&str> = said.lines().collect();
        assert_eq!(lines.len(), 1, "{plant}: {said}");
        assert!(
            lines[0].contains("table meta is malformed"),
            "{plant}: {said}"
        );
        for value in ["no-store-id-424242", "424243", "424244"] {
            assert!(!said.contains(value), "{plant}: {said}");
        }
    }
}

/// Rulings 1 and 2 of the owner stream: the owner refuses a frame `stream` beside which no end came, takes four
/// streams, answers a fifth `full` and closes the end that came with it, and gives a place again within two seconds
/// of the end-of-file of a dropped end.
#[test]
fn a_fifth_stream_is_answered_full_and_a_dropped_end_frees_its_place() {
    let scratch = Scratch::new("owner-stream-full");
    let mut owner = start_on(stream_channel(), &scratch.0, KEY, 1, false, &[]);

    owner.send(STREAM_BESIDE);
    assert_eq!(owner.stream_answer(), REFUSED);

    let mut held = Vec::new();
    for _ in 0..4 {
        let (stream, answer) = hand_over(&mut owner);
        assert_eq!(answer, TAKEN);
        held.push(stream);
    }
    let (mut fifth, answer) = hand_over(&mut owner);
    assert_eq!(answer, FULL);
    assert!(
        fifth.ends(),
        "the owner did not close the end that came with the fifth frame"
    );

    drop(held.pop());
    let deadline = Mono::now() + Duration::from_secs(2);
    loop {
        let (stream, answer) = hand_over(&mut owner);
        if answer == TAKEN {
            held.push(stream);
            break;
        }
        assert_eq!(answer, FULL);
        assert!(
            Mono::now() < deadline,
            "no place is free two seconds after an end was dropped"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Rulings 3 and 4 of the owner stream: on a stream a `runner` request reaches the store, whose handler of `runner`
/// requests answers `malformed` to a body with no `op`, and an interface read answers `role`; on the starter's
/// channel the bytes of the same `runner` request answer `role`.
#[test]
fn on_a_stream_a_runner_request_reaches_the_store_and_an_interface_read_answers_role() {
    let scratch = Scratch::new("owner-stream-roles");
    let mut owner = start_on(stream_channel(), &scratch.0, KEY, 1, false, &[]);
    let (mut stream, answer) = hand_over(&mut owner);
    assert_eq!(answer, TAKEN);

    let runner = runner_request();
    stream.send(&runner);
    assert_eq!(stream.reply().outcome.unwrap_err(), ErrorName::Malformed);
    stream.send(&read_workflows());
    assert_eq!(stream.reply().outcome.unwrap_err(), ErrorName::Role);

    owner.send(&runner);
    assert_eq!(owner.reply().outcome.unwrap_err(), ErrorName::Role);
}

/// The test's `Ends` in the product's link: the one end that the hand-over returned, once; after it `internal`.
impl Ends for Once<End> {
    fn end(&self) -> Result<End, Refusal> {
        self.0.lock().unwrap().take().ok_or(Refusal::Internal)
    }
}

/// A runner `load` of an agent that the store does not hold: the runner handler answers it `binding`, which no code of
/// the owner but a handler gives.
fn load(id: &RequestId) -> String {
    format!(
        r#"{{"v":1,"id":{},"request":"runner","body":{{"op":"load","agent":"{}","run":"{}"}}}}"#,
        serde_json::to_string(id).unwrap(),
        "40000000000000000000000000000001",
        "50000000000000000000000000000001",
    )
}

/// U9.2 point 1: the product's link over an end that the hand-over made. A runner `load` gets the runner handler's
/// answer `binding`.
#[test]
fn a_runner_request_through_the_link_is_answered_by_the_handler() {
    let scratch = Scratch::new("owner-stream-link");
    let mut owner = start_on(stream_channel(), &scratch.0, KEY, 1, false, &[]);
    let (end, answer) = handed(&mut owner);
    assert_eq!(answer, TAKEN);
    #[cfg(unix)]
    let end = End::new(end.into());
    #[cfg(windows)]
    let end = End::new(end.0.into(), end.1.into());
    let link = StreamLink::new(Once(Mutex::new(Some(end))));

    let id = request_id();
    let reply = decode_reply(&link.request(load(&id).as_bytes()).unwrap()).unwrap();
    assert_eq!(reply.id, Some(id));
    assert_eq!(
        reply.outcome.unwrap_err(),
        ErrorName::read("binding").unwrap()
    );
}

/// U9.2 (second) ruling 1: the product's `Ends` over the client call. `serve_one` stands in for the daemon's server and
/// calls `dispatch_unknown` in front of the owner binary; a runner `load` through `StreamLink::new(ClientEnds::new(..))`
/// over `Client::owner_stream` gets the runner handler's answer `binding`.
#[cfg(any(target_os = "macos", windows))]
#[test]
fn a_runner_request_through_the_product_ends_is_answered_by_the_handler() {
    use halvo_agent_client::link::product::ClientEnds;
    let scratch = Scratch::new("owner-stream-product-ends");
    let mut owner = start_on(stream_channel(), &scratch.0, KEY, 1, false, &[]);
    let (client_end, server) = control_connection();
    let client = halvo_control::client::Client::new(Once(std::sync::Mutex::new(Some(client_end))));
    let link = StreamLink::new(ClientEnds::new(move || client.owner_stream()));

    let id = request_id();
    let (served, reply) = std::thread::scope(|scope| {
        let served = scope.spawn(|| serve_one(&server, &mut owner));
        let reply = link.request(load(&id).as_bytes());
        (served.join().unwrap(), reply)
    });
    assert!(served, "dispatch_unknown wrote no reply");
    let reply = decode_reply(&reply.unwrap()).unwrap();
    assert_eq!(reply.id, Some(id));
    assert_eq!(
        reply.outcome.unwrap_err(),
        ErrorName::read("binding").unwrap()
    );
}

/// Ruling 5 of the owner stream: a length above 65,536 ends its stream, whose far end reads end-of-file, and the
/// owner goes on: a read on the starter's channel still gets its reply.
#[test]
fn a_length_above_the_bound_ends_its_stream_alone() {
    let scratch = Scratch::new("owner-stream-length");
    let mut owner = start_on(stream_channel(), &scratch.0, KEY, 1, false, &[]);
    let (mut stream, answer) = hand_over(&mut owner);
    assert_eq!(answer, TAKEN);

    let above = u32::try_from(CLIENT_FRAME_MAX + 1).unwrap();
    stream.writes.write_all(&above.to_be_bytes()).unwrap();
    assert!(
        stream.ends(),
        "the stream did not read end-of-file after a length of 65,537"
    );

    owner.send(&read_workflows());
    assert_eq!(workflows(owner.reply()), []);
}

/// Ends the starter's channel of an owner that `start_on` started over `stream_channel`. On macOS the test's end of
/// that channel is one socket, of which the reader thread holds a copy, so the socket is shut down for writes and the
/// owner reads its end; on Windows the pipe that the test writes is closed.
fn end_stream_channel(owner: &mut Owner) {
    let requests = owner.requests.take().unwrap();
    #[cfg(unix)]
    std::os::unix::net::UnixStream::from(std::os::fd::OwnedFd::from(requests))
        .shutdown(std::net::Shutdown::Write)
        .unwrap();
    #[cfg(windows)]
    drop(requests);
}

/// The request `name` with `body`, sent on the stream, and the text of its result; a refusal fails the test with its
/// error name. No reply of the bodies that the test sends holds a piece of a kept state.
fn agent_body(stream: &mut Stream, name: &str, body: &str) -> String {
    let id = serde_json::to_string(&request_id()).unwrap();
    let request = format!(r#"{{"v":1,"id":{id},"request":"{name}","body":{body}}}"#);
    stream.send(request.as_bytes());
    match stream.reply().outcome {
        Ok(ReplyResult::Raw(raw)) => raw.get().to_owned(),
        other => panic!("the request {name} answers {other:?}"),
    }
}

/// The `coordinator_write` body `op` of the workflow and the agent with the keys `more`, and the text of its result.
fn coordinator(stream: &mut Stream, workflow: &str, agent: &str, op: &str, more: &str) -> String {
    let body = format!(r#"{{"op":"{op}","workflow":"{workflow}","agent":"{agent}"{more}}}"#);
    agent_body(stream, "coordinator_write", &body)
}

/// The key `session` of the agent's entry in the roster of `list_agents`.
fn session_of(stream: &mut Stream, agent: &str) -> String {
    let body = r#"{"op":"list_agents","workflow":null,"agent":null,"after":null}"#;
    let roster: Value =
        serde_json::from_str(&agent_body(stream, "coordinator_write", body)).unwrap();
    let entry = roster["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["config"]["agent"] == agent)
        .unwrap_or_else(|| panic!("the roster holds no entry of the agent {agent}"));
    entry["session"].as_str().unwrap().to_owned()
}

/// r3 U9.12 (the coordinator's rulings of 2026-11-22, log lines 15547 and 15663): the body `drop_sessions` removes the
/// agent's kept sign-in from the owner's store by the removal call, through the owner's adapter. The owner's program
/// runs on a scratch store with two approved workflows, one of each app id of the analysis write, the agent `A` of the
/// first and `B` of the second; each ran once and ended, `A` kept a sign-in of two parts and `B` one of one part, whose
/// pieces are text that the test makes. After `drop_sessions` of `A`, read by SQL with the store's key and with no
/// condition of the store's reads: no `agent_session` row stands under the first workflow and the row of the second
/// stands. The write-ahead log, read right after the reply while the owner runs, is empty behind the checkpoint; the
/// roster reads `session` `no` for `A` and `yes` for `B`; and the reply says `{"stored":0}`.
#[test]
fn drop_sessions_leaves_no_session_row_of_the_agent_at_rest() {
    const A: &str = "40000000000000000000000000000001";
    const B: &str = "40000000000000000000000000000002";
    const RUN_A: &str = "50000000000000000000000000000001";
    const RUN_B: &str = "50000000000000000000000000000002";
    const ORIGIN: &str = "https://pay.test:443";
    let scratch = Scratch::new("owner-drop-at-rest");
    let mut owner = start_on(
        stream_channel(),
        &scratch.0,
        KEY,
        1,
        true,
        &["--check-ms=60000"],
    );
    let day = two_days_ago();
    let mut episodes: Vec<Episode> = (1..=3).map(|n| episode(n, day, FAR)).collect();
    episodes.extend((4..=6).map(|n| Episode {
        app: AppId::parse("com.example.other").unwrap(),
        ..episode(n, day, FAR)
    }));
    owner.write(&analysis_write(episodes, Position { seg: 7, idx: 0 }));
    assert!(matches!(
        owner.worker_reply().outcome,
        Ok(ReplyResult::Null)
    ));
    owner.send(&read_workflows());
    let ids = workflows(owner.reply());
    assert_eq!(ids.len(), 2, "the two app ids make two workflows");
    for id in &ids {
        let approve = halvo_schema_core::request::InterfaceDecision::approve(
            &InterfaceRole::claim(),
            request_id(),
            id.clone(),
        );
        owner.send(&approve.to_bytes().unwrap());
        assert!(matches!(owner.reply().outcome, Ok(ReplyResult::Null)));
    }
    let (ours, other) = (ids[0].as_str(), ids[1].as_str());

    let (mut stream, answer) = hand_over(&mut owner);
    assert_eq!(answer, TAKEN);
    let s = &mut stream;
    for (workflow, agent, run) in [(ours, A, RUN_A), (other, B, RUN_B)] {
        let create = coordinator(s, workflow, agent, "create_agent", "");
        assert_eq!(create, r#"{"rev":1}"#);
        let start = format!(r#","run":"{run}","mode":"compile""#);
        let started = coordinator(s, workflow, agent, "start_run", &start);
        assert_eq!(started, r#"{"ok":"yes"}"#);
    }
    let full: String = "U912TestKeptPart".chars().cycle().take(1024).collect();
    for (agent, run, part, of, data) in [
        (A, RUN_A, 1, 2, vec![full; 48]),
        (A, RUN_A, 2, 2, vec!["U912TestKeptLast".to_owned()]),
        (B, RUN_B, 1, 1, vec!["U912TestKeptOther".to_owned()]),
    ] {
        let body = serde_json::json!({
            "op": "put_session", "agent": agent, "run": run, "origin": ORIGIN,
            "rev": 1, "part": part, "of": of, "data": data,
        });
        let stored = agent_body(s, "runner", &body.to_string());
        assert_eq!(stored, r#"{"stored":1}"#);
    }
    for (workflow, agent, run) in [(ours, A, RUN_A), (other, B, RUN_B)] {
        let end = format!(r#","run":"{run}""#);
        assert_eq!(
            coordinator(s, workflow, agent, "end_run", &end),
            r#"{"ok":"yes"}"#
        );
    }
    assert_eq!(session_of(s, A), "yes", "the kept sign-in is listed");

    let dropped = coordinator(s, ours, A, "drop_sessions", "");
    let wal =
        std::fs::metadata(scratch.0.join("derived").join("derived.db-wal")).map_or(0, |m| m.len());
    let sessions = (session_of(s, A), session_of(s, B));
    end_stream_channel(&mut owner);
    assert_eq!(owner.exit_code(WAIT), Some(exit::END));

    halvo_sqlcipher::initialize().unwrap();
    let hex: String = KEY.iter().map(|b| format!("{b:02x}")).collect();
    let conn = Connection::open(scratch.0.join("derived").join("derived.db")).unwrap();
    conn.execute_batch(&format!("PRAGMA key = \"x'{hex}'\";"))
        .unwrap();
    let rows_under = |workflow: &str| -> i64 {
        conn.query_row(
            "SELECT count(*) FROM rows WHERE kind = 'agent_session' AND workflow = ?1",
            [workflow],
            |row| row.get(0),
        )
        .unwrap()
    };
    assert_eq!(
        rows_under(ours),
        0,
        "agent_session rows stand under the workflow of drop_sessions"
    );
    assert_eq!(
        rows_under(other),
        1,
        "the kept sign-in of the other workflow"
    );
    assert_eq!(wal, 0, "bytes of the write-ahead log right after the reply");
    assert_eq!(sessions, ("no".to_owned(), "yes".to_owned()));
    assert_eq!(dropped, r#"{"stored":0}"#);
}

/// The coordinator body `op` of the person's key for a model's host, with `workflow` and `agent` null and the keys
/// `more`, sent on the stream, and the text of its result.
fn key_body(stream: &mut Stream, op: &str, more: &str) -> String {
    let body = format!(r#"{{"op":"{op}","workflow":null,"agent":null{more}}}"#);
    agent_body(stream, "coordinator_write", &body)
}

/// The keys of `put_model_key` that write `key`.
fn key_of(key: &str) -> String {
    format!(r#","key":{}"#, serde_json::to_string(key).unwrap())
}

/// The bodies of the rows of the kind `model_key` in the store under `dir`, read by SQL with the store's key and with
/// no condition of the store's reads, once its owner has ended.
fn model_key_rows(dir: &Path) -> Vec<String> {
    halvo_sqlcipher::initialize().unwrap();
    let hex: String = KEY.iter().map(|b| format!("{b:02x}")).collect();
    let conn = Connection::open(dir.join("derived").join("derived.db")).unwrap();
    conn.execute_batch(&format!("PRAGMA key = \"x'{hex}'\";"))
        .unwrap();
    let mut rows = conn
        .prepare("SELECT body FROM rows WHERE kind = 'model_key'")
        .unwrap();
    rows.query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

/// r3 U7.6, sentence (3) (the coordinator's log line 16422, point 4): a dropped key is gone from the store's files. The
/// owner's program runs on a scratch store with no workflow, and the person's key for a model's host goes through the
/// owner's adapter in two coordinator bodies; each key is a made-up text that names itself as a test value. The first
/// owner writes the first key and then the second over it; read by SQL with the store's key and with no condition of
/// the store's reads once that owner has ended, ONE `model_key` row stands, it holds the second key, and no row holds
/// the first. A second owner on the same store drops the key, and then no `model_key` row stands. The write-ahead log,
/// read right after the reply of each write and of the drop while the owner runs, is empty behind the checkpoint.
#[test]
fn a_dropped_model_key_is_gone_from_the_stores_files() {
    const FIRST: &str = "test-value-not-a-model-key-first";
    const SECOND: &str = "test-value-not-a-model-key-second-of-another-length";
    let scratch = Scratch::new("owner-model-key");
    let wal = || {
        std::fs::metadata(scratch.0.join("derived").join("derived.db-wal")).map_or(0, |m| m.len())
    };
    let started = || start_on(stream_channel(), &scratch.0, KEY, 1, false, &[]);

    let mut owner = started();
    let (mut stream, answer) = hand_over(&mut owner);
    assert_eq!(answer, TAKEN);
    let written: Vec<(String, u64)> = [FIRST, SECOND]
        .into_iter()
        .map(|key| (key_body(&mut stream, "put_model_key", &key_of(key)), wal()))
        .collect();
    end_stream_channel(&mut owner);
    assert_eq!(owner.exit_code(WAIT), Some(exit::END));
    let after_write = model_key_rows(&scratch.0);

    let mut owner = started();
    let (mut stream, answer) = hand_over(&mut owner);
    assert_eq!(answer, TAKEN);
    let dropped = (key_body(&mut stream, "drop_model_key", ""), wal());
    end_stream_channel(&mut owner);
    assert_eq!(owner.exit_code(WAIT), Some(exit::END));
    let after_drop = model_key_rows(&scratch.0);

    let stored = |n: u32| (format!(r#"{{"stored":{n}}}"#), 0);
    assert_eq!(
        written,
        vec![stored(1), stored(1)],
        "(the reply, the bytes of the write-ahead log) of each write"
    );
    assert_eq!(
        (
            after_write.len(),
            after_write.iter().filter(|row| row.contains(SECOND)).count(),
            after_write.iter().filter(|row| row.contains(FIRST)).count(),
        ),
        (1, 1, 0),
        "(model_key rows, rows that hold the second key, rows that hold the first) after the write over"
    );
    assert_eq!(
        dropped,
        stored(0),
        "(the reply, the bytes of the write-ahead log) of the drop"
    );
    assert_eq!(
        after_drop,
        Vec::<String>::new(),
        "model_key rows after the drop"
    );
}

/// r3 U7.6, sentence (2) (the coordinator's log lines 16422, point 4, and 16823, its condition (v)): after Delete
/// everything a read of the key answers none, on the harness of `a_delete_during_a_batch_leaves_nothing` of crash.rs. A
/// capture store is made in the data directory under the root file's custody, whose root wraps the derived store's
/// key in derived/master.key; the owner's program on that key writes the person's key, a made-up text that names
/// itself as a test value, and ends; the product's offline delete runs on the data directory and destroys that root;
/// a capture store made again gives the derived store's key of the next start, and the owner started on it answers
/// `read_model_key` with no key.
#[cfg(all(target_os = "macos", debug_assertions))]
#[test]
fn a_model_key_row_is_gone_after_delete_everything() {
    use halvo_capture::daemon::custody_policy::Custody;
    use halvo_capture::daemon::delete_all::offline;
    use halvo_capture::daemon::port::{DataDirStore, Opened, StoreHost};
    const BEFORE: &str = "test-value-not-a-model-key-before-the-delete";
    let scratch = Scratch::new("owner-model-key-delete-all");
    let derived_key = || -> [u8; 32] {
        let host = DataDirStore::at(&scratch.0);
        let Opened::ReadWrite { store, .. } = host.create(Custody::RootFile) else {
            panic!("no capture store was made in {}", scratch.0.display());
        };
        drop(store);
        let key = host
            .derived_store_key()
            .unwrap_or_else(|fault| panic!("no key of the derived store: {fault:?}"));
        *key.expose()
    };

    let mut owner = start_on(stream_channel(), &scratch.0, derived_key(), 1, false, &[]);
    let (mut stream, answer) = hand_over(&mut owner);
    assert_eq!(answer, TAKEN);
    let written = key_body(&mut stream, "put_model_key", &key_of(BEFORE));
    let before = key_body(&mut stream, "model_key_state", "");
    end_stream_channel(&mut owner);
    assert_eq!(owner.exit_code(WAIT), Some(exit::END));
    assert_eq!(
        (written.as_str(), before.as_str()),
        (r#"{"stored":1}"#, r#"{"stands":"yes"}"#)
    );

    let data_dir = halvo_control::paths::DataDir::from_flag(scratch.0.clone()).unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let status = offline(&data_dir, &mut out, &mut err);
    let printed = format!(
        "status {status}, out: {}, err: {}",
        String::from_utf8_lossy(&out),
        String::from_utf8_lossy(&err)
    );
    assert_eq!(status, 0, "{printed}");

    let mut owner = start_on(stream_channel(), &scratch.0, derived_key(), 1, false, &[]);
    let (mut stream, answer) = hand_over(&mut owner);
    assert_eq!(answer, TAKEN);
    assert_eq!(
        key_body(&mut stream, "read_model_key", ""),
        r#"{"key":null}"#,
        "{printed}"
    );
}

/// r3 U7.7: the agent of the tests of the person's yes to a hosted model.
const AGREEING: &str = "40000000000000000000000000000077";

/// The keys of `set_model` of a yes up to `frontier`, with a made-up triple for each hosted tier.
const YES: &str = r#","tier":"frontier","agreement":[{"tier":"low_cost_hosted","origin":"https://hosted-model.test","model":"test-model-small"},{"tier":"frontier","origin":"https://hosted-model.test","model":"test-model-large"}]"#;

/// The keys of `set_model` of the way back, the person's "Use this computer only".
const WAY_BACK: &str = r#","tier":"on_device","agreement":null"#;

/// An owner on `dir` with `key` and a worker, one approved workflow of the analysis write (of the category
/// `utility_bill_payment`, whose runtime is `web`) and its agent `AGREEING`, which `create_agent` on the stream set
/// up: the owner, the stream and the workflow's id.
fn one_agent(dir: &Path, key: [u8; 32]) -> (Owner, Stream, String) {
    let mut owner = start_on(stream_channel(), dir, key, 1, true, &["--check-ms=60000"]);
    let day = two_days_ago();
    let episodes: Vec<Episode> = (1..=3).map(|n| episode(n, day, FAR)).collect();
    owner.write(&analysis_write(episodes, Position { seg: 4, idx: 0 }));
    assert!(matches!(
        owner.worker_reply().outcome,
        Ok(ReplyResult::Null)
    ));
    owner.send(&read_workflows());
    let ids = workflows(owner.reply());
    assert_eq!(ids.len(), 1, "the three episodes make one workflow");
    let approve = halvo_schema_core::request::InterfaceDecision::approve(
        &InterfaceRole::claim(),
        request_id(),
        ids[0].clone(),
    );
    owner.send(&approve.to_bytes().unwrap());
    assert!(matches!(owner.reply().outcome, Ok(ReplyResult::Null)));
    let (mut stream, answer) = hand_over(&mut owner);
    assert_eq!(answer, TAKEN);
    let workflow = ids[0].as_str().to_owned();
    let created = coordinator(&mut stream, &workflow, AGREEING, "create_agent", "");
    assert_eq!(created, r#"{"rev":1}"#);
    (owner, stream, workflow)
}

/// The key `agreement` of the agent's entry in the roster of `list_agents`, which the entry must hold.
fn agreement_of(stream: &mut Stream, agent: &str) -> Value {
    let body = r#"{"op":"list_agents","workflow":null,"agent":null,"after":null}"#;
    let roster: Value =
        serde_json::from_str(&agent_body(stream, "coordinator_write", body)).unwrap();
    let entry = roster["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["config"]["agent"] == agent)
        .unwrap_or_else(|| panic!("the roster holds no entry of the agent {agent}"));
    entry
        .as_object()
        .unwrap()
        .get("agreement")
        .cloned()
        .unwrap_or_else(|| panic!("the entry holds no key agreement: {entry}"))
}

/// The bodies of the rows of the kind in the store under `dir`, read by SQL with `key` and with no condition of the
/// store's reads, once its owner has ended.
fn bodies_of(dir: &Path, key: [u8; 32], kind: &str) -> Vec<String> {
    halvo_sqlcipher::initialize().unwrap();
    let hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
    let conn = Connection::open(dir.join("derived").join("derived.db")).unwrap();
    conn.execute_batch(&format!("PRAGMA key = \"x'{hex}'\";"))
        .unwrap();
    let mut rows = conn
        .prepare("SELECT body FROM rows WHERE kind = ?1")
        .unwrap();
    rows.query_map([kind], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

/// r3 U7.7, sentence (2) (the coordinator's log line 17342, point 3): the way back writes `on_device` and leaves no
/// agreement, on the real store through the owner's adapter. The agent of an approved workflow says yes up to
/// `frontier` (revision 2), and the roster hands that agreement; then the way back answers revision 3, the write-ahead
/// log is empty behind the checkpoint right after its reply, and the roster hands none. Read by SQL with the store's
/// key and with no condition of the store's reads once the owner has ended, no `agent_agreement` row stands and the
/// agent's newest set-up holds `on_device`.
#[test]
fn the_way_back_writes_on_device_and_leaves_no_agreement() {
    let scratch = Scratch::new("owner-agreement-way-back");
    let (mut owner, mut stream, workflow) = one_agent(&scratch.0, KEY);
    let s = &mut stream;
    let yes = coordinator(s, &workflow, AGREEING, "set_model", YES);
    let agreed = agreement_of(s, AGREEING);
    let back = coordinator(s, &workflow, AGREEING, "set_model", WAY_BACK);
    let wal =
        std::fs::metadata(scratch.0.join("derived").join("derived.db-wal")).map_or(0, |m| m.len());
    let after = agreement_of(s, AGREEING);
    end_stream_channel(&mut owner);
    assert_eq!(owner.exit_code(WAIT), Some(exit::END));
    let agreements = bodies_of(&scratch.0, KEY, "agent_agreement");
    let newest = bodies_of(&scratch.0, KEY, "agent_config")
        .iter()
        .map(|body| serde_json::from_str::<Value>(body).unwrap())
        .filter(|config| config["agent"] == AGREEING)
        .max_by_key(|config| config["rev"].as_u64())
        .unwrap();

    assert_eq!(
        (yes.as_str(), back.as_str()),
        (r#"{"rev":2}"#, r#"{"rev":3}"#)
    );
    assert_eq!(agreed["tier"], "frontier", "the roster after the yes");
    assert_eq!(after, Value::Null, "the roster after the way back");
    assert_eq!(
        wal, 0,
        "bytes of the write-ahead log right after the way back"
    );
    assert_eq!(agreements, Vec::<String>::new(), "agent_agreement rows");
    assert_eq!(
        (&newest["rev"], &newest["tier"]),
        (&Value::from(3), &Value::from("on_device"))
    );
}

/// r3 U7.7, sentence (7) (the coordinator's log line 17342, point 4): the delete of the agent's workflow takes its
/// agreement with the agent's other rows. The agent of an approved workflow says yes up to `frontier`, and the roster
/// hands that agreement; the interface's decision `delete` of the workflow is taken. Read by SQL with the store's key
/// once the owner has ended, no `agent_agreement` row and no `agent_config` row stands.
#[test]
fn the_delete_of_a_workflow_takes_its_agreement() {
    let scratch = Scratch::new("owner-agreement-delete");
    let (mut owner, mut stream, workflow) = one_agent(&scratch.0, KEY);
    let yes = coordinator(&mut stream, &workflow, AGREEING, "set_model", YES);
    let agreed = agreement_of(&mut stream, AGREEING);
    let delete = halvo_schema_core::request::InterfaceDecision::delete(
        &InterfaceRole::claim(),
        request_id(),
        WorkflowId::parse(&workflow).unwrap(),
    );
    owner.send(&delete.to_bytes().unwrap());
    let deleted = owner.reply().outcome;
    end_stream_channel(&mut owner);
    assert_eq!(owner.exit_code(WAIT), Some(exit::END));

    assert_eq!(yes, r#"{"rev":2}"#);
    assert_eq!(agreed["tier"], "frontier", "the roster after the yes");
    assert!(matches!(deleted, Ok(ReplyResult::Null)), "{deleted:?}");
    assert_eq!(
        (
            bodies_of(&scratch.0, KEY, "agent_agreement"),
            bodies_of(&scratch.0, KEY, "agent_config")
        ),
        (Vec::new(), Vec::new()),
        "(agent_agreement rows, agent_config rows) after the delete"
    );
}

/// r3 U7.7, sentence (6) (the coordinator's log line 17342, point 4): Delete everything takes the agreement with the
/// store, on the harness of `a_model_key_row_is_gone_after_delete_everything`. A capture store is made in the data
/// directory under the root file's custody; the owner on its derived key sets up an agent of an approved workflow,
/// which says yes up to `frontier`, and the roster hands that agreement; the product's offline delete runs on the
/// data directory and destroys that root; the owner started on the key of the next start lists no agent, and read by
/// SQL with that key once it has ended, no `agent_agreement` row stands.
#[cfg(all(target_os = "macos", debug_assertions))]
#[test]
fn the_agreement_row_is_gone_after_delete_everything() {
    use halvo_capture::daemon::custody_policy::Custody;
    use halvo_capture::daemon::delete_all::offline;
    use halvo_capture::daemon::port::{DataDirStore, Opened, StoreHost};
    let scratch = Scratch::new("owner-agreement-delete-all");
    let derived_key = || -> [u8; 32] {
        let host = DataDirStore::at(&scratch.0);
        let Opened::ReadWrite { store, .. } = host.create(Custody::RootFile) else {
            panic!("no capture store was made in {}", scratch.0.display());
        };
        drop(store);
        let key = host
            .derived_store_key()
            .unwrap_or_else(|fault| panic!("no key of the derived store: {fault:?}"));
        *key.expose()
    };

    let (mut owner, mut stream, workflow) = one_agent(&scratch.0, derived_key());
    let yes = coordinator(&mut stream, &workflow, AGREEING, "set_model", YES);
    let agreed = agreement_of(&mut stream, AGREEING);
    end_stream_channel(&mut owner);
    assert_eq!(owner.exit_code(WAIT), Some(exit::END));
    assert_eq!(yes, r#"{"rev":2}"#);
    assert_eq!(agreed["tier"], "frontier", "the roster after the yes");

    let data_dir = halvo_control::paths::DataDir::from_flag(scratch.0.clone()).unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let status = offline(&data_dir, &mut out, &mut err);
    let printed = format!(
        "status {status}, out: {}, err: {}",
        String::from_utf8_lossy(&out),
        String::from_utf8_lossy(&err)
    );
    assert_eq!(status, 0, "{printed}");

    let next = derived_key();
    let mut owner = start_on(stream_channel(), &scratch.0, next, 1, false, &[]);
    let (mut stream, answer) = hand_over(&mut owner);
    assert_eq!(answer, TAKEN);
    let body = r#"{"op":"list_agents","workflow":null,"agent":null,"after":null}"#;
    let listed = agent_body(&mut stream, "coordinator_write", body);
    end_stream_channel(&mut owner);
    assert_eq!(owner.exit_code(WAIT), Some(exit::END));
    assert_eq!(listed, r#"{"agents":[],"next":null}"#, "{printed}");
    assert_eq!(
        bodies_of(&scratch.0, next, "agent_agreement"),
        Vec::<String>::new(),
        "{printed}"
    );
}
