#![allow(dead_code)]

use halvo_account::sign_in::SignIn;
use halvo_secrets::{Directory, Name, Store, Write};
use std::ffi::OsString;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write as _};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub const CLEARED: [&str; 12] = [
    "HALVO_ACCOUNT_BACKEND",
    "HALVO_ACCOUNT_STORE_DIR",
    "HALVO_ACCOUNT_PAGE_ORIGINS",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "NO_PROXY",
    "http_proxy",
    "https_proxy",
    "all_proxy",
    "no_proxy",
    "SSLKEYLOGFILE",
];

static NEXT: AtomicUsize = AtomicUsize::new(0);

pub fn scratch(label: &str) -> PathBuf {
    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    let path = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("halvo-account-{}-{label}-{n}", std::process::id()));
    fs::create_dir_all(&path).expect("a scratch directory");
    path
}

pub fn store_at(dir: &Path) -> Store {
    Store::in_directory(
        Directory::new(dir.to_path_buf()).expect("an absolute directory"),
        Name::new("account").expect("a name"),
    )
}

pub fn keep_sign_in(dir: &Path, sign_in: &SignIn) {
    assert_eq!(store_at(dir).write(&sign_in.to_secret()), Write::Stored);
}

#[derive(Debug)]
pub struct Seen {
    pub method: String,
    pub target: String,
    pub version: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Seen {
    pub fn header_names(&self) -> Vec<String> {
        self.headers.iter().map(|(name, _)| name.clone()).collect()
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(own, _)| own == name)
            .map(|(_, value)| value.as_str())
    }
}

pub struct Listener {
    listener: TcpListener,
    pub port: u16,
}

impl Listener {
    pub fn new() -> Listener {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback listener");
        let port = listener.local_addr().expect("an address").port();
        Listener { listener, port }
    }

    pub fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    pub fn answer(self, status: u16, extra: &str, body: &str) -> Answering {
        let origin = self.origin();
        let extra = extra.to_string();
        let body = body.to_string();
        let thread = thread::spawn(move || {
            let stream = self.accept_within(Duration::from_secs(20))?;
            Some(serve(stream, status, &extra, &body))
        });
        Answering { origin, thread }
    }

    pub fn answer_each(self, status: u16, extra: &str, body: &str) -> Counting {
        let origin = self.origin();
        let extra = extra.to_string();
        let body = body.to_string();
        let thread = thread::spawn(move || {
            let mut seen = Vec::new();
            let mut limit = Duration::from_secs(20);
            while let Some(stream) = self.accept_within(limit) {
                seen.push(serve(stream, status, &extra, &body));
                limit = Duration::from_secs(2);
            }
            seen
        });
        Counting { origin, thread }
    }

    pub fn first_record(self) -> JoinHandle<Option<Vec<u8>>> {
        thread::spawn(move || {
            let mut stream = self.accept_within(Duration::from_secs(20))?;
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .expect("a read timeout");
            let mut head = [0; 5];
            stream.read_exact(&mut head).ok()?;
            let length = usize::from(u16::from_be_bytes([head[3], head[4]]));
            let mut record = vec![0; length];
            stream.read_exact(&mut record).ok()?;
            Some([head.to_vec(), record].concat())
        })
    }

    fn accept_within(&self, limit: Duration) -> Option<TcpStream> {
        self.listener
            .set_nonblocking(true)
            .expect("a nonblocking listener");
        let start = Instant::now();
        loop {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    stream.set_nonblocking(false).expect("a blocking stream");
                    return Some(stream);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if start.elapsed() > limit {
                        return None;
                    }
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("accept failed: {error}"),
            }
        }
    }

    pub fn was_reached(&self) -> bool {
        self.accept_within(Duration::from_millis(200)).is_some()
    }
}

pub struct Answering {
    pub origin: String,
    thread: JoinHandle<Option<Seen>>,
}

impl Answering {
    pub fn seen(self) -> Option<Seen> {
        self.thread.join().expect("the listener thread")
    }
}

pub struct Counting {
    pub origin: String,
    thread: JoinHandle<Vec<Seen>>,
}

impl Counting {
    pub fn seen_all(self) -> Vec<Seen> {
        self.thread.join().expect("the listener thread")
    }
}

fn serve(stream: TcpStream, status: u16, extra: &str, body: &str) -> Seen {
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("a read timeout");
    let mut reader = BufReader::new(stream.try_clone().expect("a second handle"));
    let mut line = String::new();
    reader.read_line(&mut line).expect("a request line");
    let mut parts = line.trim_end_matches("\r\n").splitn(3, ' ');
    let method = parts.next().unwrap_or_default().to_string();
    let target = parts.next().unwrap_or_default().to_string();
    let version = parts.next().unwrap_or_default().to_string();
    let mut headers = Vec::new();
    loop {
        let mut field = String::new();
        reader.read_line(&mut field).expect("a header line");
        let field = field.trim_end_matches("\r\n");
        if field.is_empty() {
            break;
        }
        let (name, value) = field.split_once(':').expect("a header field");
        headers.push((name.to_string(), value.trim().to_string()));
    }
    let length: usize = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .map_or(0, |(_, value)| value.parse().expect("a length"));
    let mut body_read = vec![0; length];
    reader.read_exact(&mut body_read).expect("the body");
    let mut stream = stream;
    let answer = format!(
        "HTTP/1.1 {status} Answer\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n{extra}\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(answer.as_bytes());
    let _ = stream.flush();
    Seen {
        method,
        target,
        version,
        headers,
        body: body_read,
    }
}

pub struct Ran {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Starts the built binary with `args`, the variables of `CLEARED` removed, `set` set and
/// `stdin` as its standard input: the one function of the tests that starts it. It refuses
/// words that name `subscribe` or `manage` unless `--print-url` comes first, so that no test
/// run can open a page in the browser of the machine that runs the tests: no test hands a
/// page to the built binary.
pub fn helper(args: &[&str], set: &[(&str, OsString)], stdin: Stdio) -> Ran {
    let opens = args
        .iter()
        .any(|word| *word == "subscribe" || *word == "manage");
    assert!(
        !opens || args.first() == Some(&"--print-url"),
        "a test run of the built binary names subscribe or manage without --print-url first: {args:?}"
    );
    let mut command = Command::new(env!("CARGO_BIN_EXE_halvo-account"));
    command.args(args).stdin(stdin);
    for name in CLEARED {
        command.env_remove(name);
    }
    for (name, value) in set {
        command.env(name, value);
    }
    let output = command.output().expect("the helper runs");
    Ran {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}
