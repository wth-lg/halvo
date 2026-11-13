//! `web-rows ROW ...`: the rows of U2.1's gate that run the product's executor with a real browser
//! and that no test holds. Each prints one line per run, `row <name> run=<i> ... result=<ok|miss>`,
//! and `row <name> ok=<n> of=<runs>`; exit 0 when every run is ok. Counts, times and words alone.
//!
//! - `first_start --browser PATH --runs N`: N first starts of a new profile each, timed from the
//!   start's call to the browser's first reply and to the start's return (CP-158 (2)).
//! - `killed_start --browser PATH --runs N --seed S --scratch DIR`: N children of this program
//!   that each start the browser, each ended by the system's hard kill at a moment drawn from
//!   0 to 3,000 ms after its start; a run is ok when every process that names its profile and the
//!   guard are gone within `LEFT_WAIT` (A11.1, CP-133 (i)).
//! - `relay_foreign --browser PATH [--as USER --copy-dir DIR]`: a connection to the relay from a
//!   process outside the browser's tree, of this account and, with `--as`, of another account
//!   (macOS, `sudo -n -u USER`; Windows, with `--password-file FILE`, a start with the account's
//!   credentials); ok when the relay refused it unread and recorded it apart.
//!
//! - `crash --browser PATH --kind chrome|edge --port N`: the crash row (CP-165 (2), CP-167): with
//!   the fixture on port N, a start and stop with no crash, then the browser crashed by the
//!   driver's command `Browser.crash` with the product's means, with each candidate of
//!   `CRASH_CANDIDATES` and with the means taken out (the control); every file that changed under
//!   the places of `crash_row::places` is listed by name, size and time and compared. Hosted
//!   runners alone: no browser is made to crash on the development Mac.
//!
//! - `checks --browser PATH --kind chrome|edge --port N`: the six rules of the start-to-stop check
//!   with the product's flags, read after the start, during a session and after the stop.
//!
//! `start_only` and `foreign_connect` are the children of those rows.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use halvo_web::cdp::{CrashMeans, Executor, StartOptions};
use halvo_web::crash_row;
use halvo_web::guard::GuardProgram;
use halvo_web::procs;
use halvo_web::relay::{Names, Relay};
use halvo_web::start::Start;

/// How long `killed_start` waits for the run's processes to be gone after the kill.
const LEFT_WAIT: Duration = Duration::from_secs(30);

fn arg(argv: &[String], name: &str) -> Option<String> {
    argv.iter()
        .position(|a| a == name)
        .and_then(|i| argv.get(i + 1))
        .cloned()
}

fn options(browser: &Path, profile: PathBuf, relay: &Relay) -> Option<StartOptions> {
    options_with(browser, profile, relay, CrashMeans::Product)
}

fn options_with(
    browser: &Path,
    profile: PathBuf,
    relay: &Relay,
    crash_means: CrashMeans,
) -> Option<StartOptions> {
    Some(StartOptions {
        browser: browser.to_path_buf(),
        profile,
        headless: true,
        relay: relay.handle(),
        guard: GuardProgram::this_program().ok()?,
        crash_means,
    })
}

/// How long a crash reading waits for the crash handlers that began during it to be gone: the
/// stop's `STOP_WAIT` of U0.3's library.
const STOP_WAIT: Duration = Duration::from_secs(15);

/// What one reading of the crash row found: what it left outside the profile and the temporary
/// directory, the dumps inside them, whether the crash handlers were gone or `STOP_WAIT` passed,
/// what reached the relay after the crash, whether the stop removed both directories, and the
/// browser's major version.
struct Reading {
    places: Vec<crash_row::Place>,
    left: Vec<crash_row::File>,
    inside_dumps: usize,
    handler: &'static str,
    sent: usize,
    removed: bool,
    major: String,
}

fn epoch_us() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_micros() as u64)
}

/// Whether a crash handler that began at or after `since_us` still runs.
fn handler_runs(since_us: u64) -> bool {
    procs::table().unwrap_or_default().iter().any(|p| {
        p.started_us >= since_us
            && (p
                .exe
                .as_deref()
                .and_then(|e| e.file_name())
                .is_some_and(|n| n.to_string_lossy().contains("crashpad_handler"))
                || halvo_web::checks::command_args(p.pid)
                    .is_some_and(|a| a.iter().any(|x| x.contains("--type=crashpad-handler"))))
    })
}

async fn crash_reading(
    browser: &Path,
    port: u16,
    dir: &Path,
    name: &str,
    means: CrashMeans,
    crash: bool,
) -> Result<Reading, String> {
    let profile = dir.join(format!("p-{name}"));
    let mut temp = profile.clone().into_os_string();
    temp.push("-tmp");
    let places = crash_row::places(&profile, Path::new(&temp));
    let before = crash_row::snapshot(&places);
    let since_us = epoch_us().saturating_sub(1_000_000);
    let relay = Relay::start(Names::Fixture)
        .await
        .map_err(|_| "relay".to_string())?;
    relay.handle().allow("utility.agent.test", port);
    let opts = options_with(browser, profile, &relay, means).ok_or("guard")?;
    let mut ex = Executor::start(&opts)
        .await
        .map_err(|f| f.cause.to_string())?;
    let major = ex
        .version()
        .await
        .ok()
        .and_then(|v| {
            v.rsplit('/')
                .next()
                .and_then(|x| x.split('.').next())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "-".into());
    let _ = ex
        .open(&format!("http://utility.agent.test:{port}/signin"))
        .await;
    let (refused_before, tunnels_before) = (relay.handle().refusals().len(), relay.tunnels());
    if crash {
        ex.crash().await;
    }
    let waited = Instant::now();
    tokio::time::sleep(Duration::from_millis(500)).await;
    let handler = loop {
        if !handler_runs(since_us) {
            break "gone";
        }
        if waited.elapsed() > STOP_WAIT {
            break "stop_wait";
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    };
    let first = crash_row::snapshot(&places);
    tokio::time::sleep(Duration::from_secs(3)).await;
    let second = crash_row::snapshot(&places);
    let later_changes = crash_row::changed(&first, &second).len();
    let sent = relay.tunnels().saturating_sub(tunnels_before) as usize;
    let refused = relay
        .handle()
        .refusals()
        .len()
        .saturating_sub(refused_before);
    let changed = crash_row::changed(&before, &second);
    let inside_dumps = changed
        .iter()
        .filter(|f| {
            places.iter().any(|p| p.name == f.place && p.inside_profile) && crash_row::is_dump(f)
        })
        .count();
    let stopped = ex.stop_and_remove().await;
    relay.stop();
    let left = crash_row::left_outside(&changed, &places);
    println!(
        "row crash reading={name} handler={handler} later_changes={later_changes} tunnels_after_crash={sent} refused_after_crash={refused} left_count={} left_bytes={} left_places={} dumps_left={} dumps_inside_profile={inside_dumps} removed={}",
        left.len(),
        left.iter().map(|f| f.size).sum::<u64>(),
        by_place(&left),
        left.iter().filter(|f| crash_row::is_dump(f)).count(),
        if stopped.removed { "yes" } else { "no" }
    );
    Ok(Reading {
        places,
        left,
        inside_dumps,
        handler,
        sent,
        removed: stopped.removed,
        major,
    })
}

/// The names of the places of `files` with the count of each: `name:n,...`, or `-`.
fn by_place(files: &[crash_row::File]) -> String {
    let mut counts: std::collections::BTreeMap<&str, usize> = Default::default();
    for f in files {
        *counts.entry(f.place).or_default() += 1;
    }
    if counts.is_empty() {
        return "-".into();
    }
    counts
        .iter()
        .map(|(k, v)| format!("{k}:{v}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn check_line(machine: &str, kind: &str, read: &str, s: &halvo_web::checks::Subject<'_>) -> bool {
    let (counts, _) = halvo_web::checks::check_named(s);
    let holds = halvo_web::checks::read_holds(&counts);
    let rules: Vec<String> = counts
        .iter()
        .map(|c| format!("{}={}", c.rule, c.count))
        .collect();
    println!(
        "row checks machine={machine} browser={kind} read={read} {} holds={}",
        rules.join(" "),
        if holds { "yes" } else { "no" }
    );
    holds
}

/// The six rules of the start-to-stop check (A11.1, CP-89 (c)) with the product's flags, its means
/// against a crash's dump among them (CP-167 E6): read after the start, during a session on a
/// fixture page, and after the stop with the pipe kept.
async fn checks_row(argv: &[String]) -> (usize, usize) {
    let (Some(browser), Some(kind), Some(port), Some(dir)) = (
        arg(argv, "--browser"),
        arg(argv, "--kind"),
        arg(argv, "--port").and_then(|p| p.parse::<u16>().ok()),
        scratch_of(argv, "checks"),
    ) else {
        return (0, 1);
    };
    let browser = std::path::absolute(&browser).unwrap_or(PathBuf::from(&browser));
    let machine = if cfg!(windows) { "windows" } else { "macos" };
    let Ok(relay) = Relay::start(Names::Fixture).await else {
        return (0, 3);
    };
    relay.handle().allow("utility.agent.test", port);
    let profile = dir.join("p");
    let Some(opts) = options(&browser, profile.clone(), &relay) else {
        return (0, 3);
    };
    let mut ex = match Executor::start(&opts).await {
        Ok(ex) => ex,
        Err(f) => {
            println!(
                "row checks machine={machine} browser={kind} result=notrun cause={}",
                f.cause
            );
            return (0, 3);
        }
    };
    let root = ex.root().clone();
    let ids = ex.pipe_ids().clone();
    let subject = halvo_web::checks::Subject {
        root: &root,
        profile: &profile,
        pipe: Some(&ids),
        relay_port: Some(relay.port),
    };
    let mut ok = usize::from(check_line(machine, &kind, "start", &subject));
    let _ = ex
        .open(&format!("http://utility.agent.test:{port}/signin"))
        .await;
    ok += usize::from(check_line(machine, &kind, "session", &subject));
    let kept = ex.keep_pipe();
    let stopped = ex.stop_and_remove().await;
    ok += usize::from(check_line(machine, &kind, "end", &subject));
    drop(kept);
    relay.stop();
    println!(
        "row checks machine={machine} browser={kind} removed={}",
        if stopped.removed { "yes" } else { "no" }
    );
    let _ = std::fs::remove_dir(&dir);
    (ok, 3)
}

/// The crash row (CP-165 (2), CP-167 E1 to E4) for one browser: a start and stop with no crash,
/// the crash with the product's means, the crash with each candidate of the closed list, and the
/// crash with the means taken out (the control). One line per reading and one line per means
/// with its result; the product's line is the cell.
async fn crash(argv: &[String]) -> (usize, usize) {
    let (Some(browser), Some(kind), Some(port), Some(dir)) = (
        arg(argv, "--browser"),
        arg(argv, "--kind"),
        arg(argv, "--port").and_then(|p| p.parse::<u16>().ok()),
        scratch_of(argv, "crash"),
    ) else {
        return (0, 1);
    };
    let browser = std::path::absolute(&browser).unwrap_or(PathBuf::from(&browser));
    let machine = if cfg!(windows) { "windows" } else { "macos" };
    let place = crash_row::crash_place(&kind);
    println!(
        "row crash machine={machine} browser={kind} crash=Browser.crash crash_place={kind}_crashpad crash_place_is_own_browser_s=yes send_setting={}",
        match place.as_deref().and_then(crash_row::send_setting) {
            Some(true) => "yes",
            Some(false) => "no",
            None => "unread",
        }
    );
    let baseline =
        match crash_reading(&browser, port, &dir, "baseline", CrashMeans::Product, false).await {
            Ok(r) => r,
            Err(cause) => {
                println!("row crash machine={machine} browser={kind} result=notrun cause={cause}");
                return (0, 1);
            }
        };
    let control = crash_reading(&browser, port, &dir, "control", CrashMeans::Without, true).await;
    let mut means: Vec<(&str, CrashMeans)> = vec![("product", CrashMeans::Product)];
    for (i, (name, _)) in halvo_web::CRASH_CANDIDATES.iter().enumerate() {
        means.push((name, CrashMeans::Candidate(i)));
    }
    let mut product_ok = false;
    for (name, m) in means {
        let with = crash_reading(&browser, port, &dir, &format!("means-{name}"), m, true).await;
        let line = match (&with, &control) {
            (Ok(w), Ok(c)) => {
                let beyond = crash_row::beyond(&w.left, &baseline.left);
                let control_dumps: Vec<&crash_row::File> =
                    c.left.iter().filter(|f| crash_row::is_dump(f)).collect();
                let result = crash_row::result(&w.places, &baseline.left, &w.left, &c.left, w.sent);
                if name == "product" {
                    product_ok = result == "ok";
                }
                format!(
                    "row crash machine={machine} browser={kind} major={} means={name} beyond_count={} beyond_bytes={} beyond_places={} control_dumps={} control_dump_bytes={} control_places={} sent_after_crash={} dump_inside_profile={} handler={} removed={} result={result}",
                    w.major,
                    beyond.len(),
                    beyond.iter().map(|f| f.size).sum::<u64>(),
                    by_place(&beyond),
                    control_dumps.len(),
                    control_dumps.iter().map(|f| f.size).sum::<u64>(),
                    by_place(&c.left.iter().filter(|f| crash_row::is_dump(f)).cloned().collect::<Vec<_>>()),
                    w.sent,
                    if w.inside_dumps > 0 { "yes" } else { "no" },
                    w.handler,
                    if w.removed && baseline.removed { "yes" } else { "no" },
                )
            }
            (Err(cause), _) | (_, Err(cause)) => {
                format!("row crash machine={machine} browser={kind} means={name} result=notrun cause={cause}")
            }
        };
        println!("{line}");
    }
    let _ = std::fs::remove_dir(&dir);
    (usize::from(product_ok), 1)
}

fn scratch_of(argv: &[String], name: &str) -> Option<PathBuf> {
    let base = arg(argv, "--scratch")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = std::path::absolute(base)
        .ok()?
        .join(format!("halvo-web-rows-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

async fn first_start(argv: &[String]) -> (usize, usize) {
    let (Some(browser), Some(dir)) = (arg(argv, "--browser"), scratch_of(argv, "first")) else {
        return (0, 1);
    };
    let runs: usize = arg(argv, "--runs")
        .and_then(|r| r.parse().ok())
        .unwrap_or(5);
    let Ok(relay) = Relay::start(Names::Fixture).await else {
        return (0, runs);
    };
    let mut ok = 0;
    let mut most = 0u128;
    for i in 1..=runs {
        let Some(opts) = options(Path::new(&browser), dir.join(format!("p{i}")), &relay) else {
            break;
        };
        let at = Instant::now();
        match Executor::start(&opts).await {
            Ok(ex) => {
                let start_ms = at.elapsed().as_millis();
                let reply_ms = ex.first_reply().as_millis();
                most = most.max(reply_ms);
                let stopped = ex.stop_and_remove().await;
                let fine = stopped.removed && stopped.left.is_empty();
                ok += usize::from(fine);
                println!(
                    "row first_start run={i} first_reply_ms={reply_ms} start_ms={start_ms} bound_ms={} result={}",
                    halvo_web::cdp::FIRST_REPLY_WAIT.as_millis(),
                    if fine { "ok" } else { "miss" }
                );
            }
            Err(f) => println!("row first_start run={i} result=miss cause={}", f.cause),
        }
    }
    println!("row first_start most_first_reply_ms={most}");
    relay.stop();
    let _ = std::fs::remove_dir(&dir);
    (ok, runs)
}

/// The child of `killed_start`: starts the browser and holds it until it is killed.
async fn start_only(argv: &[String]) -> ExitCode {
    let (Some(browser), Some(profile)) = (arg(argv, "--browser"), arg(argv, "--profile")) else {
        return ExitCode::from(64);
    };
    let Ok(relay) = Relay::start(Names::Fixture).await else {
        return ExitCode::from(2);
    };
    let Some(opts) = options(Path::new(&browser), PathBuf::from(profile), &relay) else {
        return ExitCode::from(2);
    };
    let started = Executor::start(&opts).await;
    tokio::time::sleep(Duration::from_secs(60)).await;
    drop(started);
    ExitCode::SUCCESS
}

/// A draw from 0 to 3,000 ms by a linear congruential step of `state`.
fn draw(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    (*state >> 33) % 3_001
}

/// Whether two paths name one program: on Windows with `\\?\` taken off, `/` and `\` taken as one
/// and the letter case ignored, as the system compares them there.
fn same_path(a: &Path, b: &Path) -> bool {
    if cfg!(windows) {
        let norm = |p: &Path| {
            let t = p.to_string_lossy().replace('/', "\\").to_lowercase();
            t.strip_prefix("\\\\?\\").map(str::to_string).unwrap_or(t)
        };
        norm(a) == norm(b)
    } else {
        a == b
    }
}

/// The processes that the run of `profile` left: those that name its profile as a whole path and
/// run a program inside the browser's bundle or install directory (`checks::Mark`), and its guard,
/// this program run with the guard's flag and the profile's temporary directory.
fn left_of(mark: &halvo_web::checks::Mark, temp: &str, me: &Path) -> (Vec<procs::Proc>, usize) {
    let table = procs::table().unwrap_or_default();
    let own: Vec<procs::Proc> = table
        .iter()
        .filter(|p| mark.owns(p, &halvo_web::checks::command_args))
        .cloned()
        .collect();
    let guards = table
        .iter()
        .filter(|p| p.exe.as_deref().is_some_and(|e| same_path(e, me)))
        .filter(|p| {
            halvo_web::checks::command_args(p.pid).is_some_and(|a| {
                a.iter().any(|x| x == halvo_web::guard::GUARD_FLAG)
                    && a.iter().any(|x| x.contains(temp))
            })
        })
        .count();
    (own, guards)
}

async fn killed_start(argv: &[String]) -> (usize, usize) {
    let (Some(browser), Some(dir)) = (arg(argv, "--browser"), scratch_of(argv, "killed")) else {
        return (0, 1);
    };
    let runs: usize = arg(argv, "--runs")
        .and_then(|r| r.parse().ok())
        .unwrap_or(10);
    let mut state: u64 = arg(argv, "--seed")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    let Ok(me) = std::env::current_exe() else {
        return (0, runs);
    };
    let me_real = std::fs::canonicalize(&me).unwrap_or(me.clone());
    let browser = std::path::absolute(&browser).unwrap_or(PathBuf::from(&browser));
    let mut ok = 0;
    for i in 1..=runs {
        let at_ms = draw(&mut state);
        let profile = dir.join(format!("p{i}"));
        let mut temp = profile.clone().into_os_string();
        temp.push("-tmp");
        let temp = temp.to_string_lossy().into_owned();
        let since_us = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_micros() as u64)
            .saturating_sub(1_000_000);
        let mark = halvo_web::checks::Mark::new(&profile, &browser, since_us);
        let mut child = match std::process::Command::new(&me)
            .arg("start_only")
            .arg("--browser")
            .arg(&browser)
            .arg("--profile")
            .arg(&profile)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .start()
        {
            Ok(c) => c,
            Err(_) => {
                println!("row killed_start run={i} result=miss cause=child");
                continue;
            }
        };
        tokio::time::sleep(Duration::from_millis(at_ms)).await;
        let _ = child.kill();
        let _ = child.wait();
        let killed = Instant::now();
        let (gone_ms, left, guards) = loop {
            let (own, guards) = left_of(&mark, &temp, &me_real);
            if own.is_empty() && guards == 0 {
                break (Some(killed.elapsed().as_millis()), 0, 0);
            }
            if killed.elapsed() > LEFT_WAIT {
                break (None, own.len(), guards);
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        let ended = if left > 0 {
            halvo_web::checks::end_runs_own(&mark).ended
        } else {
            0
        };
        let fine = gone_ms.is_some();
        ok += usize::from(fine);
        println!(
            "row killed_start run={i} at_ms={at_ms} gone_ms={} left={left} guards_left={guards} ended_after={ended} result={}",
            gone_ms.map_or("-".to_string(), |m| m.to_string()),
            if fine { "ok" } else { "miss" }
        );
    }
    (ok, runs)
}

/// The child of `relay_foreign`: one SOCKS5 CONNECT to `utility.agent.test` through the relay at
/// 127.0.0.1:PORT; prints `connect=tunnel` when the relay answered with a tunnel, else
/// `connect=refused`.
fn foreign_connect(argv: &[String]) -> ExitCode {
    use std::io::{Read, Write};
    let (Some(port), Some(target)) = (
        argv.get(2).and_then(|p| p.parse::<u16>().ok()),
        argv.get(3).and_then(|p| p.parse::<u16>().ok()),
    ) else {
        return ExitCode::from(64);
    };
    let word = (|| {
        let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).ok()?;
        s.set_read_timeout(Some(Duration::from_secs(10))).ok()?;
        s.write_all(&[5, 1, 0]).ok()?;
        let mut r = [0u8; 10];
        s.read_exact(&mut r[..2]).ok()?;
        let host = b"utility.agent.test";
        let mut req = vec![5, 1, 0, 3, host.len() as u8];
        req.extend_from_slice(host);
        req.extend_from_slice(&target.to_be_bytes());
        s.write_all(&req).ok()?;
        s.read_exact(&mut r).ok()?;
        (r[1] == 0).then_some("tunnel")
    })()
    .unwrap_or("refused");
    println!("connect={word}");
    ExitCode::SUCCESS
}

/// The start of `foreign_connect` under the account `user`, with no window and no prompt: on macOS
/// `sudo -n -u USER`; on Windows `System.Diagnostics.Process` with the account's name and the
/// password that `--password-file` holds (CreateProcessWithLogonW), through PowerShell, which
/// prints the child's line.
#[cfg(unix)]
fn as_other(
    user: &str,
    copy: &Path,
    port: u16,
    echo_port: u16,
    _argv: &[String],
) -> Option<std::process::Command> {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(copy, std::fs::Permissions::from_mode(0o755));
    let mut other = std::process::Command::new("/usr/bin/sudo");
    other
        .args(["-n", "-u", user])
        .arg(copy)
        .arg("foreign_connect")
        .arg(port.to_string())
        .arg(echo_port.to_string());
    Some(other)
}

#[cfg(windows)]
fn as_other(
    user: &str,
    copy: &Path,
    port: u16,
    echo_port: u16,
    argv: &[String],
) -> Option<std::process::Command> {
    let password_file = arg(argv, "--password-file")?;
    let quote = |s: &str| s.replace('\'', "''");
    let script = format!(
        "$ErrorActionPreference = 'Stop'; \
         $i = New-Object System.Diagnostics.ProcessStartInfo; \
         $i.FileName = '{exe}'; $i.Arguments = 'foreign_connect {port} {echo_port}'; \
         $i.UserName = '{user}'; \
         $i.Password = ConvertTo-SecureString ((Get-Content -Raw '{pw}').Trim()) -AsPlainText -Force; \
         $i.UseShellExecute = $false; $i.CreateNoWindow = $true; $i.LoadUserProfile = $false; \
         $i.WorkingDirectory = '{dir}'; $i.RedirectStandardOutput = $true; \
         $p = [System.Diagnostics.Process]::Start($i); $o = $p.StandardOutput.ReadToEnd(); \
         $p.WaitForExit(); Write-Output $o.Trim()",
        exe = quote(&copy.to_string_lossy()),
        user = quote(user),
        pw = quote(&password_file),
        dir = quote(&copy.parent()?.to_string_lossy()),
    );
    let mut other = std::process::Command::new("powershell.exe");
    other.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
    Some(other)
}

async fn relay_foreign(argv: &[String]) -> (usize, usize) {
    let (Some(browser), Some(dir)) = (arg(argv, "--browser"), scratch_of(argv, "foreign")) else {
        return (0, 1);
    };
    let Ok(relay) = Relay::start(Names::Fixture).await else {
        return (0, 1);
    };
    let Ok(echo) = std::net::TcpListener::bind(("127.0.0.1", 0)) else {
        return (0, 1);
    };
    let echo_port = echo.local_addr().map(|a| a.port()).unwrap_or(0);
    echo.set_nonblocking(true).ok();
    relay.handle().allow("utility.agent.test", echo_port);
    let Some(opts) = options(Path::new(&browser), dir.join("p"), &relay) else {
        return (0, 1);
    };
    let ex = match Executor::start(&opts).await {
        Ok(ex) => ex,
        Err(f) => {
            println!("row relay_foreign result=miss cause={}", f.cause);
            return (0, 1);
        }
    };
    let Ok(me) = std::env::current_exe() else {
        return (0, 1);
    };
    let mut cells: Vec<(&str, std::process::Command)> = Vec::new();
    let mut same = std::process::Command::new(&me);
    same.arg("foreign_connect")
        .arg(relay.port.to_string())
        .arg(echo_port.to_string());
    cells.push(("same", same));
    if let (Some(user), Some(copy_dir)) = (arg(argv, "--as"), arg(argv, "--copy-dir")) {
        let copy = Path::new(&copy_dir).join(if cfg!(windows) {
            "web-rows.exe"
        } else {
            "web-rows"
        });
        if std::fs::copy(&me, &copy).is_ok() {
            if let Some(other) = as_other(&user, &copy, relay.port, echo_port, argv) {
                cells.push(("other", other));
            }
        }
    }
    let mut ok = 0;
    let runs = cells.len();
    for (who, mut cmd) in cells {
        let before = relay.handle().foreign_count();
        let out = cmd.stdin(std::process::Stdio::null()).start_output();
        let said = out
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();
        let foreign = relay.handle().foreign();
        let cause = foreign.last().map_or("-", |f| f.cause);
        let recorded = relay.handle().foreign_count() == before + 1;
        let accepted = usize::from(echo.accept().is_ok());
        let fine = recorded && said == "connect=refused" && accepted == 0;
        ok += usize::from(fine);
        println!(
            "row relay_foreign as={who} cause={cause} recorded={} {said} accepted={accepted} result={}",
            if recorded { "yes" } else { "no" },
            if fine { "ok" } else { "miss" }
        );
    }
    let stopped = ex.stop_and_remove().await;
    println!(
        "row relay_foreign stop removed={} left={}",
        if stopped.removed { "yes" } else { "no" },
        stopped.left.len()
    );
    relay.stop();
    let _ = std::fs::remove_dir(&dir);
    (ok, runs)
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().collect();
    if let Some(code) = halvo_web::guard::run_if_guard(&argv) {
        return ExitCode::from(code.clamp(0, 255) as u8);
    }
    let row = argv.get(1).cloned().unwrap_or_default();
    if row == "foreign_connect" {
        return foreign_connect(&argv);
    }
    let Ok(runtime) = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    else {
        return ExitCode::from(2);
    };
    let (ok, runs) = match row.as_str() {
        "start_only" => return runtime.block_on(start_only(&argv)),
        "first_start" => runtime.block_on(first_start(&argv)),
        "killed_start" => runtime.block_on(killed_start(&argv)),
        "relay_foreign" => runtime.block_on(relay_foreign(&argv)),
        "crash" => runtime.block_on(crash(&argv)),
        "checks" => runtime.block_on(checks_row(&argv)),
        _ => {
            eprintln!("usage: web-rows first_start|killed_start|relay_foreign --browser PATH ...");
            return ExitCode::from(64);
        }
    };
    println!("row {row} ok={ok} of={runs}");
    if ok == runs && runs > 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
