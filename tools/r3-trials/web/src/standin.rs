// Stand-in programs for the tests of a run's own processes (CP-126 E1). The stand-in is this test
// binary, copied into a fake app bundle and run with `R3_STAND_IN` naming its mode; each passes the
// arguments that it got on, so each names the run's profile as the browser's processes do.
//
// - `browser`: answers the executor on its debugging pipe as far as a start and a stop need; when
//   it is asked to close it starts `late` and ends.
// - `browser_late`: as `browser`, but it starts `late` when its first page is made, so a helper
//   runs beside the browser while a row waits; it asks for the start's probe through the relay,
//   and after the probe it answers nothing but a close, so a row waits on its first call.
// - `late`: waits 300 ms, starts `helper` and ends, so the helper appears after the browser ended.
// - `helper`: waits up to 20 s and ends.
// - `hang`: holds the pipe and answers nothing for up to 60 s, as a browser that stalls in its start.
// - `env`: writes its environment as the start gave it, one `NAME=VALUE` a line and every entry
//   (none dropped as a shell or a table would), into the file that its first argument names.
// - `leave`: a first process that ends and leaves a process of its own: it starts `helper` with
//   the pipe's ends closed to it (by `posix_spawn`, so the helper never runs this program's image
//   with other arguments), then ends, which closes the pipe and fails the start.
//
// Another test of this binary runs as usual: with no `R3_STAND_IN`, `entry` returns at once.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

// `super::start` is the trial library's start module: `crate::start` inside the library, and
// `r3_web::start` brought in by an integration test that includes this file.
use super::start::Start;

/// The variable that names a stand-in's mode.
pub const VAR: &str = "R3_STAND_IN";
/// The test that a stand-in runs.
pub const ENTRY: &str = "standin::entry";
/// The arguments of the test harness before the stand-in's own: `--` ends the harness's options.
const HARNESS: [&str; 4] = ["--exact", "--quiet", "--test-threads=1", "--"];

/// A fake app bundle with the stand-in in it.
pub struct Bundle {
    /// `<dir>/Stand-in.app/Contents/MacOS/stand-in`, a copy of this test binary.
    pub binary: PathBuf,
    /// A launcher beside it that runs the stand-in as `browser` with the start's arguments.
    pub script: PathBuf,
}

/// Makes the bundle under `dir`.
pub fn bundle(dir: &Path) -> Bundle {
    let macos = dir.join("Stand-in.app").join("Contents").join("MacOS");
    std::fs::create_dir_all(&macos).unwrap();
    let binary = macos.join("stand-in");
    std::fs::copy(std::env::current_exe().unwrap(), &binary).unwrap();
    let script = launcher(&macos, "browser");
    Bundle { binary, script }
}

/// A launcher in the bundle's `dir` that runs the stand-in as `mode` with the start's arguments.
pub fn launcher(dir: &Path, mode: &str) -> PathBuf {
    launcher_as(dir, mode, &format!("{mode}.sh"))
}

/// `launcher` under the file name `name`, such as `Google Chrome`, which the trial program takes as a
/// browser's.
pub fn launcher_as(dir: &Path, mode: &str, name: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let script = dir.join(name);
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\n{VAR}={mode} exec \"$(dirname \"$0\")/stand-in\" {ENTRY} {} \"$@\"\n",
            HARNESS.join(" ")
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script
}

/// Starts `binary` as the stand-in `mode` with `args` as its own arguments.
pub fn run(binary: &Path, mode: &str, args: &[String]) -> std::io::Result<Child> {
    Command::new(binary)
        .arg(ENTRY)
        .args(HARNESS)
        .args(args)
        .env(VAR, mode)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .start()
}

#[test]
fn entry() {
    let Ok(mode) = std::env::var(VAR) else {
        return;
    };
    let args: Vec<String> = std::env::args().skip_while(|a| a != "--").skip(1).collect();
    let me = std::env::current_exe().unwrap();
    match mode.as_str() {
        "browser" => browser(&me, &args, false),
        "browser_late" => browser(&me, &args, true),
        "late" => {
            std::thread::sleep(Duration::from_millis(300));
            let _ = run(&me, "helper", &args);
        }
        "leave" => {
            for fd in [3, 4] {
                // SAFETY: fcntl on descriptors that the start placed in this process.
                unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
            }
            let _ = run(&me, "helper", &args);
        }
        "env" => {
            if let Some(out) = args.first() {
                let mut text = String::new();
                for (k, v) in std::env::vars_os() {
                    text.push_str(&format!(
                        "{}={}\n",
                        k.to_string_lossy(),
                        v.to_string_lossy()
                    ));
                }
                let _ = std::fs::write(out, text);
            }
        }
        "hang" => {
            let until = Instant::now() + Duration::from_secs(60);
            while Instant::now() < until {
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        "helper" => {
            let until = Instant::now() + Duration::from_secs(20);
            while Instant::now() < until {
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        _ => {}
    }
    std::process::exit(0);
}

fn send(out: &mut std::fs::File, v: &serde_json::Value) {
    let mut bytes = serde_json::to_vec(v).unwrap();
    bytes.push(0);
    let _ = out.write_all(&bytes);
}

fn target_info(id: &str) -> serde_json::Value {
    serde_json::json!({
        "targetId": id, "type": "page", "title": "", "url": "about:blank",
        "attached": true, "canAccessOpener": false
    })
}

/// Asks for the start probe's origin through the relay that `--proxy-server=socks5://HOST:PORT`
/// names, as a browser does with its proxy: the relay refuses it and records the refusal.
fn probe(args: &[String], url: &str) {
    use std::io::Read;
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

/// Answers the executor's messages on descriptors 3 (from it) and 4 (to it).
fn browser(me: &Path, args: &[String], early: bool) {
    use std::os::fd::FromRawFd;
    // SAFETY: the start placed the executor's ends at 3 and 4 and nothing else in this process
    // owns them; each is wrapped once.
    let input = unsafe { std::fs::File::from_raw_fd(3) };
    // SAFETY: as above, for descriptor 4.
    let mut out = unsafe { std::fs::File::from_raw_fd(4) };
    let mut reader = BufReader::new(input);
    let mut targets: Vec<String> = Vec::new();
    let mut probed = false;
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
        if early && probed && method != "Browser.close" {
            continue;
        }
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
                if early && targets.len() == 1 {
                    let _ = run(me, "late", args);
                }
                serde_json::json!({ "targetId": id })
            }
            "Page.navigate" => {
                let url = m["params"]["url"].as_str().unwrap_or("");
                if url.contains("halvo-start-probe.invalid") {
                    probe(args, url);
                    probed = true;
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
            let _ = run(me, "late", args);
            return;
        }
    }
}
