//! `web-standin`: the stand-in browser of the tests of a failed start (`tests/failed_start.rs`,
//! CP-170 E15 (2), CP-186 N1), on macOS and on Windows, and the fenced guard of those tests.
//!
//! As a browser it takes its mode from `HALVO_WEB_STAND_IN` when another stand-in started it, and
//! otherwise from the file `stand-in.mode` beside its own program, because the executor gives a
//! browser a fixed environment. It reads the executor's messages from descriptor 3 and writes its
//! own to descriptor 4 (macOS), or uses the two handles that `--remote-debugging-io-pipes=R,W`
//! names (Windows); it keeps both ends from the processes that it starts, and passes the
//! arguments that it got on to them, so that each names the run's profile as a browser's helpers
//! do.
//!
//! - `browser_slow_probe`: answers a start and a stop as far as they need, asks for the start's
//!   probe through the relay 500 ms after it answered the navigation, and when it is asked to
//!   close starts `late` and ends.
//! - `late`: waits 300 ms, starts `helper` and ends, so the helper appears after the browser ended.
//! - `helper`: runs for up to 20 s.
//! - `hang`: starts `long_helper` with no argument of its own, then holds the pipe and answers
//!   nothing for up to 60 s.
//! - `long_helper`: runs for up to 90 s.
//! - `leave`: starts `helper` and ends before any reply.
//!
//! With `--fence-below DIR` before `--relay-guard` it is the guard of those tests: the guard's loop
//! with the system's table and its watch of the executor, whose ask and forced end reach only a
//! process whose program lies below DIR; any other call ends it with exit 9 and sends nothing.

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use halvo_web::start::Start;

const VAR: &str = "HALVO_WEB_STAND_IN";
const MODE_FILE: &str = "stand-in.mode";

fn mode(me: &Path) -> String {
    std::env::var(VAR)
        .ok()
        .filter(|m| !m.is_empty())
        .or_else(|| {
            std::fs::read_to_string(me.with_file_name(MODE_FILE))
                .ok()
                .map(|m| m.trim().to_string())
        })
        .unwrap_or_default()
}

fn run(me: &Path, mode: &str, args: &[String]) {
    let _ = Command::new(me)
        .args(args)
        .env(VAR, mode)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .start();
}

fn hold(limit: Duration) {
    let until = Instant::now() + limit;
    while Instant::now() < until {
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(target_os = "macos")]
fn ends(_args: &[String]) -> Option<(File, File)> {
    use std::os::fd::FromRawFd;
    for fd in [3, 4] {
        // SAFETY: fcntl on the two descriptors that the start placed in this process.
        if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } != 0 {
            return None;
        }
    }
    // SAFETY: the start placed the executor's ends at 3 and 4, nothing else in this process owns
    // them, and each is wrapped once.
    Some(unsafe { (File::from_raw_fd(3), File::from_raw_fd(4)) })
}

#[cfg(windows)]
fn ends(args: &[String]) -> Option<(File, File)> {
    use std::os::windows::io::FromRawHandle;
    use windows_sys::Win32::Foundation::{SetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT};
    let value = args
        .iter()
        .find_map(|a| a.strip_prefix("--remote-debugging-io-pipes="))?;
    let (r, w) = value.split_once(',')?;
    let (r, w) = (r.parse::<usize>().ok()?, w.parse::<usize>().ok()?);
    for h in [r, w] {
        // SAFETY: the start gave this process the two handles that the argument names.
        if unsafe { SetHandleInformation(h as HANDLE, HANDLE_FLAG_INHERIT, 0) } == 0 {
            return None;
        }
    }
    // SAFETY: as above; nothing else in this process owns them, and each is wrapped once.
    Some(unsafe { (File::from_raw_handle(r as _), File::from_raw_handle(w as _)) })
}

#[cfg(not(any(target_os = "macos", windows)))]
fn ends(_args: &[String]) -> Option<(File, File)> {
    None
}

fn send(out: &mut File, v: &serde_json::Value) {
    let mut bytes = serde_json::to_vec(v).unwrap_or_default();
    bytes.push(0);
    let _ = out.write_all(&bytes);
}

fn target_info(id: &str) -> serde_json::Value {
    serde_json::json!({
        "targetId": id, "type": "page", "title": "", "url": "about:blank",
        "attached": true, "canAccessOpener": false
    })
}

fn probe(args: &[String], url: &str) {
    let Some(proxy) = args
        .iter()
        .find_map(|a| a.strip_prefix("--proxy-server=socks5://"))
    else {
        return;
    };
    let host = url
        .split("://")
        .nth(1)
        .and_then(|r| r.split(['/', ':']).next())
        .unwrap_or("");
    let Ok(mut s) = std::net::TcpStream::connect(proxy) else {
        return;
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    let mut r = [0u8; 10];
    if s.write_all(&[5, 1, 0]).is_err() || s.read_exact(&mut r[..2]).is_err() {
        return;
    }
    let mut req = vec![5, 1, 0, 3, host.len() as u8];
    req.extend_from_slice(host.as_bytes());
    req.extend_from_slice(&80u16.to_be_bytes());
    let _ = s.write_all(&req);
    let _ = s.read(&mut r);
}

fn browser(me: &Path, args: &[String], input: File, mut out: File) {
    let mut reader = BufReader::new(input);
    let mut targets: Vec<String> = Vec::new();
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match reader.read_until(0, &mut buf) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        if buf.last() == Some(&0) {
            buf.pop();
        }
        let Ok(m) = serde_json::from_slice::<serde_json::Value>(&buf) else {
            continue;
        };
        let method = m["method"].as_str().unwrap_or("").to_string();
        let result = match method.as_str() {
            "Browser.getVersion" => serde_json::json!({
                "protocolVersion": "1.3", "product": "StandIn/1.0", "revision": "0",
                "userAgent": "StandIn", "jsVersion": "0"
            }),
            "Target.createTarget" => {
                let id = format!("T{}", targets.len() + 1);
                targets.push(id.clone());
                send(
                    &mut out,
                    &serde_json::json!({
                        "method": "Target.attachedToTarget",
                        "params": {
                            "sessionId": format!("S-{id}"),
                            "targetInfo": target_info(&id),
                            "waitingForDebugger": false
                        }
                    }),
                );
                serde_json::json!({ "targetId": id })
            }
            "Page.navigate" => {
                let url = m["params"]["url"].as_str().unwrap_or("");
                if url.contains(halvo_web::cdp::PROBE_HOST) {
                    let (args, url) = (args.to_vec(), url.to_string());
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(500));
                        probe(&args, &url);
                    });
                }
                serde_json::json!({ "frameId": "F1", "loaderId": "L1" })
            }
            "Target.getTargets" => serde_json::json!({
                "targetInfos": targets.iter().map(|t| target_info(t)).collect::<Vec<_>>()
            }),
            "Target.attachToTarget" => serde_json::json!({
                "sessionId": format!("S-{}", m["params"]["targetId"].as_str().unwrap_or(""))
            }),
            _ => serde_json::json!({}),
        };
        let mut reply = serde_json::json!({ "id": m["id"].clone(), "result": result });
        if let Some(s) = m.get("sessionId") {
            reply["sessionId"] = s.clone();
        }
        send(&mut out, &reply);
        if method == "Browser.close" {
            run(me, "late", args);
            return;
        }
    }
}

/// Whether `exe` lies below `dir`: by path components on macOS, and on Windows with `/` and `\`
/// as one and the letter case ignored.
fn below(exe: &Path, dir: &Path) -> bool {
    if cfg!(windows) {
        let norm = |p: &Path| p.to_string_lossy().replace('\\', "/").to_lowercase();
        norm(exe).starts_with(&format!("{}/", norm(dir).trim_end_matches('/')))
    } else {
        exe.starts_with(dir)
    }
}

fn guard(args: &[String]) -> i32 {
    let (Some(at), Some(fence)) = (
        args.iter().position(|a| a == halvo_web::guard::GUARD_FLAG),
        args.iter()
            .position(|a| a == "--fence-below")
            .and_then(|i| args.get(i + 1)),
    ) else {
        return 2;
    };
    let fence = if cfg!(windows) {
        PathBuf::from(fence)
    } else {
        std::fs::canonicalize(fence).unwrap_or_else(|_| PathBuf::from(fence))
    };
    let g = &args[at + 1..];
    let num = |i: usize| g.get(i).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
    let (Some(profile), Some(program)) = (g.get(5), g.get(6)) else {
        return 2;
    };
    let mark = halvo_web::checks::Mark::new(Path::new(profile), Path::new(program), num(1));
    let watch = halvo_web::procs::ExecutorWatch::open(num(2) as u32, num(3));
    let start = Instant::now();
    halvo_web::guard::guard_loop(
        num(0) as u32,
        num(1),
        halvo_web::guard::Trigger::ExecutorGone {
            pid: num(2) as u32,
            started_us: num(3),
        },
        halvo_web::guard::GuardIo {
            read: &mut || halvo_web::procs::table().ok(),
            args: &halvo_web::checks::command_args,
            mark: Some(&mark),
            executor: &mut |_, _| watch.runs(),
            now_ms: &mut || start.elapsed().as_millis(),
            pause: &mut |_| std::thread::sleep(halvo_web::guard::POLL),
            end: &mut |p, force| {
                if !p.exe.as_deref().is_some_and(|e| below(e, &fence)) {
                    std::process::exit(9);
                }
                if force {
                    halvo_web::procs::kill(p)
                } else {
                    halvo_web::procs::ask_end(p)
                }
            },
            record: &mut |_| {},
        },
    )
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == halvo_web::guard::GUARD_FLAG) {
        return ExitCode::from(guard(&args).clamp(0, 255) as u8);
    }
    let Ok(me) = std::env::current_exe() else {
        return ExitCode::from(2);
    };
    match mode(&me).as_str() {
        "browser_slow_probe" => {
            if let Some((input, out)) = ends(&args) {
                browser(&me, &args, input, out);
            }
        }
        "late" => {
            std::thread::sleep(Duration::from_millis(300));
            run(&me, "helper", &args);
        }
        "leave" => {
            if ends(&args).is_some() {
                run(&me, "helper", &args);
            }
        }
        "hang" => {
            if let Some(held) = ends(&args) {
                run(&me, "long_helper", &[]);
                hold(Duration::from_secs(60));
                drop(held);
            }
        }
        "helper" => hold(Duration::from_secs(20)),
        "long_helper" => hold(Duration::from_secs(90)),
        _ => return ExitCode::from(2),
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    /// CP-186 N8: the fence is on in this test target from its start.
    #[test]
    fn the_fence_is_on_in_this_test_target() {
        assert!(
            std::hint::black_box(cfg!(feature = "test-fence")),
            "the fence is off in this test target"
        );
    }
}
