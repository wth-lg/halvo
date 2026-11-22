//! What the tests of the run's browser share: the hosted runner's gate of a test that starts a real browser, a
//! fixture server on 127.0.0.1 that records every connection, the supervisor's loop with the browser of a test's
//! own, the web steps, and the processes that bear a run's profile.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use halvo_schema::executable::{ExecutableStep, FromRunner, RunId, WebOrigin};

use super::{Act, Runner, Seen, Supervisor};

/// One line on standard error, named by the test.
pub fn say(test: &str, line: &str) {
    let _ = std::io::stderr().write_all(format!("{test}: {line}\n").as_bytes());
}

/// The browser of a test of the real browser, which runs on a hosted runner alone and is read the way the test
/// `the_built_app_starts_its_daemon_and_the_daemon_ends_with_it` reads it: off a hosted runner none, after one line
/// that says so; on a hosted runner the program that `HALVO_BROWSER` names, after one line that names it, and a
/// panic that names `HALVO_BROWSER` where it is not set. The product never reads that variable.
pub fn hosted(test: &str) -> Option<PathBuf> {
    if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true") {
        say(
            test,
            "not run: it starts a real browser, on a hosted runner alone",
        );
        return None;
    }
    let browser = std::env::var_os("HALVO_BROWSER").unwrap_or_else(|| {
        panic!(
            "{test}: HALVO_BROWSER is not set: the hosted job sets it to the runner's own Chrome"
        )
    });
    let browser = PathBuf::from(browser);
    say(test, &format!("browser {}", browser.display()));
    Some(browser)
}

/// Whether the test of the browser that the runner finds by itself runs here: on a hosted macOS runner alone. Off one,
/// and on Windows, false after one line that says so.
pub fn hosted_mac(test: &str) -> bool {
    if !cfg!(target_os = "macos") || std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true") {
        say(
            test,
            "not run: it starts the browser that the runner finds, on a hosted macOS runner alone",
        );
        return false;
    }
    true
}

/// One connection that the fixture accepted: the request's method and path, empty where none came, and every byte
/// that it read.
#[derive(Clone, Debug)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub bytes: Vec<u8>,
}

/// A fixture server on a loopback port of its own that answers each path of its pages with that page and every
/// other path with 404, and records every connection. Dropping it stops it.
pub struct Fixture {
    port: u16,
    seen: Arc<Mutex<Vec<Request>>>,
    stop: Arc<AtomicBool>,
}

impl Fixture {
    pub fn start(pages: &[(&str, &str)]) -> Fixture {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind the fixture");
        let port = listener.local_addr().expect("the fixture's port").port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let pages: Vec<(String, String)> = pages
            .iter()
            .map(|(path, page)| (path.to_string(), page.to_string()))
            .collect();
        let (record, stopped) = (Arc::clone(&seen), Arc::clone(&stop));
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if stopped.load(Ordering::SeqCst) {
                    return;
                }
                if let Ok(stream) = stream {
                    let request = answer(stream, &pages);
                    record.lock().expect("the record").push(request);
                }
            }
        });
        Fixture { port, seen, stop }
    }

    pub fn origin(&self) -> WebOrigin {
        WebOrigin::parse(&format!("http://127.0.0.1:{}", self.port)).expect("an origin")
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    pub fn requests(&self) -> Vec<Request> {
        self.seen.lock().expect("the record").clone()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect((Ipv4Addr::LOCALHOST, self.port));
    }
}

/// Reads one request, its head and the body of its `Content-Length`, answers it and closes the connection.
fn answer(mut stream: TcpStream, pages: &[(String, String)]) -> Request {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(at) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            break Some(at + 4);
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break None,
            Ok(n) => bytes.extend_from_slice(&chunk[..n]),
        }
    };
    let head = String::from_utf8_lossy(&bytes[..head_end.unwrap_or(bytes.len())]).into_owned();
    let mut words = head.split_whitespace();
    let method = words.next().unwrap_or_default().to_string();
    let path = words.next().unwrap_or_default().to_string();
    let length = head
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())?
        })
        .unwrap_or(0);
    if let Some(end) = head_end {
        while bytes.len() < end + length {
            match stream.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => bytes.extend_from_slice(&chunk[..n]),
            }
        }
        let page = pages.iter().find(|(p, _)| *p == path).map(|(_, page)| page);
        let (status, body) = match page {
            Some(page) => ("200 OK", page.as_str()),
            None => ("404 Not Found", ""),
        };
        let reply = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(reply.as_bytes());
    }
    Request {
        method,
        path,
        bytes,
    }
}

/// A step `navigate` to the const address `url`, number `n`.
pub fn navigate(n: u32, url: &str) -> ExecutableStep {
    let json = format!(
        r#"{{"v":1,"n":{n},"kind":"web","verb":"navigate","target":null,"args":[{{"name":"url","from":"const","value":"{url}","key":null}}],"pre":[],"post":[],"effect":"none","class":"read"}}"#
    );
    ExecutableStep::parse(json.as_bytes()).expect("a navigate step")
}

/// A step `extract` of the control with the test id `id`, parsed as an amount and kept as `amount`, number `n`.
pub fn extract_amount(n: u32, id: &str) -> ExecutableStep {
    let json = format!(
        r#"{{"v":1,"n":{n},"kind":"web","verb":"extract","target":{{"desc":"the amount","frame":[],"credential":"no","locators":[{{"by":"test_id","value":"{id}","name":null}}]}},"args":[{{"name":"as","from":"const","value":"amount","key":null}},{{"name":"parse","from":"const","value":"amount","key":null}}],"pre":[],"post":[],"effect":"none","class":"read"}}"#
    );
    ExecutableStep::parse(json.as_bytes()).expect("an extract step")
}

/// The JSON of a web target of the top document found by the test id `id`.
pub fn target(id: &str) -> String {
    format!(
        r#"{{"desc":"the control {id}","frame":[],"credential":"no","locators":[{{"by":"test_id","value":"{id}","name":null}}]}}"#
    )
}

/// The JSON of an arg `name` with the const `value`.
pub fn konst(name: &str, value: &str) -> String {
    format!(r#"{{"name":"{name}","from":"const","value":"{value}","key":null}}"#)
}

/// The JSON of an arg `name` from the slot `key`.
pub fn from_slot(name: &str, key: &str) -> String {
    format!(r#"{{"name":"{name}","from":"slot","value":null,"key":"{key}"}}"#)
}

/// The JSON of a condition `check` on the control with the test id `id`, with the const `arg`.
pub fn cond(check: &str, id: Option<&str>, arg: Option<&str>) -> String {
    let target = id.map_or("null".to_string(), target);
    let arg = arg.map_or("null".to_string(), |a| konst("arg", a));
    format!(r#"{{"check":"{check}","target":{target},"arg":{arg}}}"#)
}

/// A web step `n` of `verb` with the effect `none`, on the control with the test id `id` (none for a step with no
/// target), with its args and its conditions; of the class `read` for `navigate`, `extract` and `wait` and
/// `prepare` for every other verb.
pub fn step(
    n: u32,
    verb: &str,
    id: Option<&str>,
    args: &[String],
    pre: &[String],
    post: &[String],
) -> ExecutableStep {
    let class = match verb {
        "navigate" | "extract" | "wait" => "read",
        _ => "prepare",
    };
    let json = format!(
        r#"{{"v":1,"n":{n},"kind":"web","verb":"{verb}","target":{},"args":[{}],"pre":[{}],"post":[{}],"effect":"none","class":"{class}"}}"#,
        id.map_or("null".to_string(), target),
        args.join(","),
        pre.join(","),
        post.join(",")
    );
    ExecutableStep::parse(json.as_bytes()).expect("a web step")
}

/// `Supervisor::serve` with `--browser` set to `browser`: starts the runner and serves it until its end, with `rule`
/// choosing the act for each frame, at most `limit` for the whole run.
pub fn serve_with(
    sup: &Supervisor,
    browser: &Path,
    rule: impl FnMut(&FromRunner, &[FromRunner]) -> Act,
    limit: Duration,
) -> Seen {
    let mut args = sup.args();
    let at = args
        .iter()
        .position(|a| a == "--browser")
        .expect("the key --browser")
        + 1;
    args[at] = browser.to_string_lossy().into_owned();
    serve_runner(sup, Runner::start(&args), rule, limit)
}

/// `Supervisor::serve` with no `--browser`, for the test `test`, which `Runner::start_finding_its_browser` lets
/// start on a hosted macOS runner alone: the runner finds the browser of the machine by itself.
pub fn serve_finding(
    sup: &Supervisor,
    test: &str,
    rule: impl FnMut(&FromRunner, &[FromRunner]) -> Act,
    limit: Duration,
) -> Seen {
    let mut args = sup.args();
    let at = args
        .iter()
        .position(|a| a == "--browser")
        .expect("the key --browser");
    args.drain(at..at + 2);
    serve_runner(
        sup,
        Runner::start_finding_its_browser(test, &args),
        rule,
        limit,
    )
}

/// Serves `runner` until its end, as `Supervisor::serve` does.
fn serve_runner(
    sup: &Supervisor,
    mut runner: Runner,
    mut rule: impl FnMut(&FromRunner, &[FromRunner]) -> Act,
    limit: Duration,
) -> Seen {
    let mut frames: Vec<FromRunner> = Vec::new();
    let mut closed_at: Option<Instant> = None;
    let start = Instant::now();
    while let Some(frame) = runner.recv() {
        let act = rule(&frame, &frames);
        frames.push(frame.clone());
        if !sup.act(&mut runner, &frame, act, &mut closed_at) || start.elapsed() > limit {
            break;
        }
    }
    let status = runner.wait_within(limit.saturating_sub(start.elapsed()));
    let close_to_end = match (closed_at, status) {
        (Some(at), Some(_)) => Some(at.elapsed()),
        _ => None,
    };
    if status.is_none() {
        runner.kill();
    }
    let stderr = runner.stderr();
    for line in stderr
        .lines()
        .filter(|l| l.starts_with("run request=failed"))
    {
        say("runner", line);
    }
    Seen {
        data_dir: runner.data_dir.clone(),
        line: runner.line.clone(),
        frames,
        status,
        stderr,
        close_to_end,
        group_gone: None,
    }
}

/// The directory `<data directory>/agents/profiles/<run id>` of the run's profile and the profile in it, in the
/// form that the executor writes into the browser's flags (`halvo_web::cdp::resolve_profile`): on macOS with every
/// link of the data directory resolved, on Windows as given.
pub fn profile_of(seen: &Seen, run: &RunId) -> (PathBuf, PathBuf) {
    let data = if cfg!(windows) {
        seen.data_dir.clone()
    } else {
        std::fs::canonicalize(&seen.data_dir).expect("the data directory")
    };
    let run_dir = data.join("agents").join("profiles").join(run.as_str());
    let profile = run_dir.join("profile");
    (run_dir, profile)
}

/// The processes that began after `since_us` and bear `profile` among their arguments, as the web crate's mark reads
/// a profile.
pub fn bearing(profile: &Path, since_us: u64) -> Vec<halvo_web::procs::Proc> {
    let profile = profile.to_string_lossy().into_owned();
    halvo_web::procs::table()
        .expect("the process table")
        .into_iter()
        .filter(|p| p.started_us >= since_us && p.pid != std::process::id())
        .filter(|p| {
            halvo_web::checks::command_args(p.pid)
                .is_some_and(|a| halvo_web::checks::bears_profile(&a, &profile))
        })
        .collect()
}

/// This process's creation time, from which `bearing` counts.
pub fn own_start_us() -> u64 {
    halvo_web::procs::own()
        .expect("this process's row")
        .started_us
}
