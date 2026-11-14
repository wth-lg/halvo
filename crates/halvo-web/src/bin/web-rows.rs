//! `web-rows ROW ...`: the rows of U2.1's gate that run the product's executor with a real browser
//! and that no test holds. Each prints one line per run, `row <name> run=<i> ... result=<ok|miss>`,
//! and `row <name> ok=<n> of=<runs>`; exit 0 when every run is ok. Counts, times and words alone.
//!
//! - `first_start --browser PATH --runs N`: N first starts of a new profile each, timed from the
//!   start's call to the browser's first reply and to the start's return (CP-158 (2)).
//! - `killed_start --browser PATH --runs N --seed S --scratch DIR`: N children of this program
//!   that each start the browser, each ended by the system's hard kill at a moment drawn from
//!   0 to 3,000 ms after its start; one read rule (`rows::read_left`) from the child's start to the
//!   run's verdict: `seen` is the most of the run's own processes that one read found, a run with
//!   `seen=0` is notrun, ok when every process of the run and its guard are gone within
//!   `LEFT_WAIT` with nothing open, miss otherwise; it draws until N runs read ok or miss, at most
//!   3 N draws, and is ok only with N ok and no miss (A11.1, CP-133 (i), CP-170 E8 (a), (b)).
//! - `relay_foreign --browser PATH [--as USER --copy-dir DIR]`: a connection to the relay from a
//!   process outside the browser's tree, of this account and, with `--as`, of another account
//!   (macOS, `sudo -n -u USER`; Windows, with `--password-file FILE`, a start with the account's
//!   credentials); after a control in which the row's own browser loads the listed echo port
//!   through the relay (notrun when it does not); a cell is ok when the relay refused it unread
//!   and recorded it apart with the cause that the cell is for, and a cell that cannot be prepared
//!   prints its own miss.
//!
//! - `crash --browser PATH --kind chrome|edge --port N`: the crash row (CP-165 (2), CP-167): with
//!   the fixture on port N, a start and stop with no crash, then the browser crashed by the
//!   driver's command `Browser.crash` with the product's means, with each candidate of
//!   `CRASH_CANDIDATES` and with the means taken out (the control); every file that changed under
//!   the places of `crash_row::places` is listed by name, size and time and compared. Hosted
//!   runners alone: no browser is made to crash on the development Mac.
//!
//! - `checks --browser PATH --kind chrome|edge --port N`: the six rules of the start-to-stop check
//!   with the product's flags, read after the start, during a session and after the stop, each line
//!   naming the six rules and the five keys with `holds=`; a control read while the row's own
//!   process holds a second listener must read `holds=no`; ok when the three hold, the control
//!   reads no and the stop removed both directories (CP-170 E4 (a), (b)).
//!
//! - `block_frame_post --browser PATH --kind chrome|edge --port N`: the row of A13.7 through the
//!   product's executor: a state-changing request of the payment frame is paused and failed by
//!   the request rule, and the fixture counts nothing.
//! - `ws_allowed --browser PATH --kind chrome|edge --ws-port N`: the row of CP-176 A2 (c) through
//!   the product's executor, against `rows/ws-listener.mjs` on port N: no WebSocket handshake
//!   reaches the listener from a document, a frame or a worker, each of which tried and was
//!   refused; the row's own handshake outside the browser is counted (the control); and the
//!   executor failed no document while it added the policy line.
//! - `hold_release --browser PATH --kind chrome|edge --port N [--hop-port M]`: the row of CP-176
//!   A1 (f) through the product's executor: a state-changing request stays held while a snapshot
//!   of the page is read, is let through once by its id, and a second one that the page sent
//!   meanwhile is failed, with no byte of it at the fixture. With `--hop-port`, its hop case
//!   (CP-186 T4 (b)) against `rows/hop-listener.mjs` on port M: the answer to the released request
//!   is a 307 to the same address, and the hop is failed, so the listener counts one arrival.
//!
//! - `crash_one --browser PATH --port N --relay-port P`: one crash reading with the product's means
//!   on a relay at port P, for the egress check of A13.3's third item to wrap (hosted runners
//!   alone).
//!
//! `start_only` and `foreign_connect` are the children of those rows.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use halvo_web::cdp::{CrashMeans, Executor, Mode, StartOptions};
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
    changed: Vec<crash_row::File>,
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

/// The word of the crash handlers that began at or after `since_us` of the browser `program`:
/// `runs`, `unread` or `gone` (`crash_row::handler_state`, CP-189 G8).
fn handler_word(since_us: u64, program: &Path) -> &'static str {
    let table = procs::table().ok();
    crash_row::handler_state(
        table.as_deref(),
        since_us,
        program,
        &halvo_web::checks::command_args,
    )
}

#[allow(clippy::too_many_arguments)]
async fn crash_reading(
    browser: &Path,
    port: u16,
    dir: &Path,
    name: &str,
    means: CrashMeans,
    crash: bool,
    origin: &crash_row::Listing,
) -> Result<Reading, String> {
    crash_reading_at(browser, port, dir, name, means, crash, 0, origin).await
}

/// The listing of the places outside a profile, taken before a row's first reading (CP-186 T3).
fn origin_of(dir: &Path) -> crash_row::Listing {
    let profile = dir.join("p-origin");
    let mut temp = profile.clone().into_os_string();
    temp.push("-tmp");
    crash_row::snapshot(&crash_row::places(&profile, Path::new(&temp)))
}

#[allow(clippy::too_many_arguments)]
async fn crash_reading_at(
    browser: &Path,
    port: u16,
    dir: &Path,
    name: &str,
    means: CrashMeans,
    crash: bool,
    relay_port: u16,
    origin: &crash_row::Listing,
) -> Result<Reading, String> {
    let profile = dir.join(format!("p-{name}"));
    let mut temp = profile.clone().into_os_string();
    temp.push("-tmp");
    let places = crash_row::places(&profile, Path::new(&temp));
    let before = crash_row::snapshot(&places);
    let since_us = epoch_us().saturating_sub(1_000_000);
    let relay = Relay::start_at(Names::Fixture, relay_port)
        .await
        .map_err(|_| "relay".to_string())?;
    relay
        .handle()
        .allow("utility.agent.test", port, halvo_web::relay::Entry::Listed);
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
        .arm_rule(
            vec![format!("http://utility.agent.test:{port}")],
            Mode::Fail,
        )
        .await;
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
        let word = handler_word(since_us, browser);
        if word == "gone" {
            break "gone";
        }
        if waited.elapsed() > STOP_WAIT {
            break if word == "unread" {
                "unread"
            } else {
                "stop_wait"
            };
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
    let removals = crash_row::removals(&changed, &places, origin);
    println!(
        "row crash reading={name} handler={handler} later_changes={later_changes} tunnels_after_crash={sent} refused_after_crash={refused} left_count={} left_bytes={} left_places={} dumps_left={} dumps_inside_profile={inside_dumps} removed={} removed_files_deciding={} removed_files_own={} removed_files_temp={}",
        left.len(),
        left.iter().map(|f| f.size).sum::<u64>(),
        by_place(&left),
        left.iter().filter(|f| crash_row::is_dump(f)).count(),
        if stopped.removed { "yes" } else { "no" },
        removals.deciding,
        removals.own,
        removals.temp
    );
    Ok(Reading {
        places,
        left,
        changed: changed.clone(),
        inside_dumps,
        handler,
        sent,
        removed: stopped.removed,
        major,
    })
}

/// The facts of a reading that `crash_row::result` reads (CP-186 T2).
fn facts_of(r: &Reading) -> crash_row::Facts {
    crash_row::Facts {
        changed: r.changed.clone(),
        dumps_inside_profile: r.inside_dumps,
        handler: r.handler.to_string(),
        removed: r.removed,
    }
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

/// The members of a run as its guard reads them (`guard::members`, with the members known from
/// earlier reads), kept across the reads of the row `checks`.
struct MemberWatch {
    known: Vec<procs::Proc>,
    group_held: bool,
}

/// One line of the row `checks` at its read `read` (CP-185 (4)): the run's members as the guard
/// would read them now, those that do not bear the run's `mark` at this read, and the file names
/// of their programs; `members=notrun` when the table could not be read, which a reader takes as
/// not run and never as 0. Gives whether the line holds counts.
fn member_line(
    machine: &str,
    kind: &str,
    read: &str,
    root: &procs::Proc,
    mark: &halvo_web::checks::Mark,
    watch: &mut MemberWatch,
) -> bool {
    let head = format!("row checks machine={machine} browser={kind} read={read}");
    let Ok(table) = procs::table() else {
        println!("{head} members=notrun cause=table_unread");
        return false;
    };
    if let Some(r) = table.iter().find(|t| procs::same(t, root)) {
        if !watch.known.iter().any(|k| procs::same(k, r)) {
            watch.known.push(r.clone());
        }
    }
    let args = halvo_web::checks::command_args;
    let running = halvo_web::guard::members(
        &mut watch.known,
        &table,
        root,
        std::process::id(),
        watch.group_held,
        Some(mark),
        &args,
    );
    watch.group_held = root.pgid == root.pid
        && table
            .iter()
            .any(|t| t.pgid == root.pgid && t.started_us >= root.started_us);
    let unmarked: Vec<&procs::Proc> = running.iter().filter(|p| !mark.owns(p, &args)).collect();
    let mut names: Vec<String> = unmarked
        .iter()
        .map(|p| {
            p.exe
                .as_ref()
                .and_then(|e| e.file_name())
                .map_or("-".to_string(), |n| n.to_string_lossy().replace(' ', "_"))
        })
        .collect();
    names.sort();
    names.dedup();
    // CP-189 G1: the adopted processes of this read (parent 1, inside the browser's bundle, begun
    // at or after the first process, outside its tree), and those of them that bear the mark.
    let tree = procs::descendants(&table, std::slice::from_ref(root));
    let roots: Vec<PathBuf> = root
        .exe
        .as_deref()
        .map(procs::bundle_root)
        .into_iter()
        .collect();
    let adopted = procs::adopted(&table, &tree, root.started_us, &roots);
    let adopted_marked = adopted.iter().filter(|p| mark.owns(p, &args)).count();
    println!(
        "{head} members={} unmarked={} unmarked_programs={} adopted={} adopted_marked={adopted_marked}",
        running.len(),
        unmarked.len(),
        if names.is_empty() {
            "-".to_string()
        } else {
            names.join(",")
        },
        adopted.len()
    );
    // CP-189 G1, the coordinator's answer (a): for each adopted process and each member without the
    // mark, its program's file name and the forms of its two profile arguments, never their values.
    let form_of = |p: &procs::Proc| {
        let program = p
            .exe
            .as_ref()
            .and_then(|e| e.file_name())
            .map_or("-".to_string(), |n| n.to_string_lossy().replace(' ', "_"));
        let (database, user_data_dir) = match args(p.pid) {
            Some(a) => halvo_web::checks::profile_forms(&a, &mark.profile, cfg!(windows)),
            None => ("unread", "unread"),
        };
        format!("{program}:database={database}:user_data_dir={user_data_dir}")
    };
    let list = |v: Vec<String>| {
        if v.is_empty() {
            "-".to_string()
        } else {
            v.join(",")
        }
    };
    println!(
        "{head} unmarked_forms={} adopted_forms={}",
        list(unmarked.iter().map(|p| form_of(p)).collect()),
        list(adopted.iter().map(form_of).collect())
    );
    true
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
        return (0, 1);
    };
    relay
        .handle()
        .allow("utility.agent.test", port, halvo_web::relay::Entry::Listed);
    let profile = dir.join("p");
    let Some(opts) = options(&browser, profile.clone(), &relay) else {
        return (0, 1);
    };
    let mut ex = match Executor::start(&opts).await {
        Ok(ex) => ex,
        Err(f) => {
            println!(
                "row checks machine={machine} browser={kind} result=notrun cause={}",
                f.cause
            );
            return (0, 1);
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
    let start = check_line(machine, &kind, "start", &subject);
    let mark = halvo_web::checks::Mark::new(&profile, &browser, root.started_us);
    let mut watch = MemberWatch {
        known: Vec::new(),
        group_held: false,
    };
    let mut member_reads = usize::from(member_line(
        machine, &kind, "start", &root, &mark, &mut watch,
    ));
    let _ = ex
        .arm_rule(
            vec![format!("http://utility.agent.test:{port}")],
            Mode::Fail,
        )
        .await;
    let _ = ex
        .open(&format!("http://utility.agent.test:{port}/signin"))
        .await;
    let session = check_line(machine, &kind, "session", &subject);
    member_reads += usize::from(member_line(
        machine, &kind, "session", &root, &mark, &mut watch,
    ));
    let second = {
        let _held = halvo_web::start::lock();
        std::net::TcpListener::bind(("127.0.0.1", 0))
    };
    let control_read = check_line(machine, &kind, "control", &subject);
    let control = second.is_ok() && !control_read;
    println!(
        "row checks machine={machine} browser={kind} control=executor_listener result={}",
        if control { "ok" } else { "miss" }
    );
    drop(second);
    let kept = ex.keep_pipe();
    let stopped = ex.stop_and_remove().await;
    let end = check_line(machine, &kind, "end", &subject);
    member_reads += usize::from(member_line(machine, &kind, "end", &root, &mark, &mut watch));
    drop(kept);
    relay.stop();
    // CP-189 G7: a row that did not print its three member lines reads as not run, never as ok.
    let read_all = member_reads == 3;
    let fine = read_all && start && session && end && control && stopped.removed;
    println!("row checks machine={machine} browser={kind} member_reads={member_reads} of=3");
    println!(
        "row checks machine={machine} browser={kind} removed={} result={}{}",
        if stopped.removed { "yes" } else { "no" },
        if fine {
            "ok"
        } else if read_all {
            "miss"
        } else {
            "notrun"
        },
        if read_all { "" } else { " cause=member_reads" }
    );
    let _ = std::fs::remove_dir(&dir);
    (usize::from(fine), 1)
}

/// A GET of the variants fixture's control on 127.0.0.1:`port`, as JSON.
fn fixture_json(port: u16, method: &str, target: &str) -> Option<serde_json::Value> {
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(10))).ok()?;
    let req = format!(
        "{method} {target} HTTP/1.0\r\nHost: control.agent.test:{port}\r\nContent-Length: 0\r\n\r\n"
    );
    s.write_all(req.as_bytes()).ok()?;
    let mut raw = String::new();
    s.read_to_string(&mut raw).ok()?;
    let (_, body) = raw.split_once("\r\n\r\n")?;
    serde_json::from_str(body).ok()
}

/// The fixture's counts of state-changing requests and of payments.
fn counts_of(port: u16) -> Option<(i64, usize)> {
    let state = fixture_json(port, "GET", "/state")?;
    let changing = state["requests"]["state_changing"].as_i64()?;
    let payments = state["utility"]["payments"].as_array()?.len();
    Some((changing, payments))
}

const SEED: &str = include_str!("../../../../tools/fixture-server/variants/data/seed-1.json");

/// The id of the held request `method` `path`, read until `STEP_TIMEOUT`.
async fn held(ex: &Executor, method: &str, path: &str) -> Result<u64, &'static str> {
    let deadline = Instant::now() + halvo_web::cdp::STEP_TIMEOUT;
    loop {
        if let Some(h) = ex
            .paused()
            .into_iter()
            .find(|h| h.method == method && h.path == path)
        {
            return Ok(h.id);
        }
        if Instant::now() > deadline {
            return Err("held_none");
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn frame_post_steps(ex: &mut Executor, port: u16) -> Result<(bool, bool), &'static str> {
    use halvo_web::cdp::Locator;
    let seed: serde_json::Value = serde_json::from_str(SEED).map_err(|_| "seed")?;
    let utility = format!("http://utility.agent.test:{port}");
    let pay = format!("http://pay.agent.test:{port}");
    ex.arm_rule(vec![utility.clone(), pay.clone()], Mode::Hold)
        .await
        .map_err(|f| f.0)?;
    ex.open(&format!("{utility}/signin"))
        .await
        .map_err(|f| f.0)?;
    for (name, key) in [("Email", "user"), ("Password", "password")] {
        let r = ex
            .resolve_one(&[Locator::Role("textbox", name.into())], &[])
            .await
            .map_err(|f| f.0)?;
        ex.fill(&r, seed["credentials"][key].as_str().unwrap_or(""))
            .await
            .map_err(|f| f.0)?;
    }
    let sign_in = ex
        .resolve_one(&[Locator::Role("button", "Sign in".into())], &[])
        .await
        .map_err(|f| f.0)?;
    ex.click(&sign_in).await.map_err(|f| f.0)?;
    let id = held(ex, "POST", "/signin").await?;
    ex.let_through(id).await.map_err(|f| f.0)?;
    tokio::time::sleep(Duration::from_millis(1_000)).await;
    ex.arm_rule(vec![utility.clone(), pay.clone()], Mode::Fail)
        .await
        .map_err(|f| f.0)?;
    ex.open(&format!("{utility}/pay")).await.map_err(|f| f.0)?;
    let before = counts_of(port).ok_or("state")?;
    let pay_now = ex
        .resolve_one(&[Locator::Role("button", "Pay now".into())], &["Payment"])
        .await
        .map_err(|f| f.0)?;
    ex.click(&pay_now).await.map_err(|f| f.0)?;
    let deadline = Instant::now() + halvo_web::cdp::STEP_TIMEOUT;
    let failed = loop {
        if let Some(p) = ex
            .requests()
            .into_iter()
            .find(|p| p.method == "POST" && p.origin == pay && p.path == "/charge")
        {
            break !p.let_through && p.outcome == halvo_web::cdp::Outcome::Failed;
        }
        if Instant::now() > deadline {
            return Err("paused_none");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    tokio::time::sleep(Duration::from_millis(2_000)).await;
    let after = counts_of(port).ok_or("state")?;
    Ok((failed, after == before))
}

/// The row `block_frame_post` of A13.7 through the product's executor (CP-170 E15): with the
/// request rule armed to fail every state-changing request, a click of `Pay now` in the payment
/// frame of `/pay` sends a POST to the payment site, which the rule pauses and fails, and the
/// fixture counts no state-changing request and no payment.
async fn block_frame_post(argv: &[String]) -> (usize, usize) {
    let (Some(browser), Some(kind), Some(port), Some(dir)) = (
        arg(argv, "--browser"),
        arg(argv, "--kind"),
        arg(argv, "--port").and_then(|p| p.parse::<u16>().ok()),
        scratch_of(argv, "frame-post"),
    ) else {
        return (0, 1);
    };
    let browser = std::path::absolute(&browser).unwrap_or(PathBuf::from(&browser));
    let machine = if cfg!(windows) { "windows" } else { "macos" };
    if fixture_json(port, "POST", "/reset?seed=1").is_none() {
        println!(
            "row block_frame_post machine={machine} browser={kind} result=notrun cause=fixture"
        );
        return (0, 1);
    }
    let Ok(relay) = Relay::start(Names::Fixture).await else {
        return (0, 1);
    };
    relay
        .handle()
        .allow("utility.agent.test", port, halvo_web::relay::Entry::Listed);
    relay
        .handle()
        .allow("pay.agent.test", port, halvo_web::relay::Entry::Listed);
    let Some(opts) = options(&browser, dir.join("p"), &relay) else {
        return (0, 1);
    };
    let mut ex = match Executor::start(&opts).await {
        Ok(ex) => ex,
        Err(f) => {
            println!(
                "row block_frame_post machine={machine} browser={kind} result=notrun cause={}",
                f.cause
            );
            return (0, 1);
        }
    };
    let steps = frame_post_steps(&mut ex, port).await;
    let stopped = ex.stop_and_remove().await;
    relay.stop();
    let _ = std::fs::remove_dir(&dir);
    let (line, fine) = match steps {
        Ok((failed, unchanged)) => (
            format!(
                "paused=yes failed={} nothing_counted={} removed={}",
                if failed { "yes" } else { "no" },
                if unchanged { "yes" } else { "no" },
                if stopped.removed { "yes" } else { "no" }
            ),
            failed && unchanged && stopped.removed,
        ),
        Err(cause) => (format!("cause={cause}"), false),
    };
    println!(
        "row block_frame_post machine={machine} browser={kind} {line} result={}",
        if fine { "ok" } else { "miss" }
    );
    (usize::from(fine), 1)
}

/// The fixture's counts of state-changing requests and of requests of every kind.
fn requests_of(port: u16) -> Option<(i64, i64)> {
    let state = fixture_json(port, "GET", "/state")?;
    Some((
        state["requests"]["state_changing"].as_i64()?,
        state["requests"]["total"].as_i64()?,
    ))
}

/// What `hold_release` read: whether the first request was still held after the snapshot,
/// whether the release went through, the fixture's new state-changing requests, and the outcomes
/// of the two requests.
struct HoldRead {
    held_through_snapshot: bool,
    released: bool,
    counted: i64,
    held_after: usize,
    first: Option<halvo_web::cdp::Outcome>,
    second: Option<halvo_web::cdp::Outcome>,
}

async fn hold_release_steps(ex: &mut Executor, port: u16) -> Result<HoldRead, &'static str> {
    use halvo_web::cdp::Locator;
    let seed: serde_json::Value = serde_json::from_str(SEED).map_err(|_| "seed")?;
    let utility = format!("http://utility.agent.test:{port}");
    let pay = format!("http://pay.agent.test:{port}");
    ex.arm_rule(vec![utility.clone(), pay.clone()], Mode::Hold)
        .await
        .map_err(|f| f.0)?;
    ex.open(&format!("{utility}/signin"))
        .await
        .map_err(|f| f.0)?;
    for (name, key) in [("Email", "user"), ("Password", "password")] {
        let r = ex
            .resolve_one(&[Locator::Role("textbox", name.into())], &[])
            .await
            .map_err(|f| f.0)?;
        ex.fill(&r, seed["credentials"][key].as_str().unwrap_or(""))
            .await
            .map_err(|f| f.0)?;
    }
    let sign_in = ex
        .resolve_one(&[Locator::Role("button", "Sign in".into())], &[])
        .await
        .map_err(|f| f.0)?;
    ex.click(&sign_in).await.map_err(|f| f.0)?;
    let id = held(ex, "POST", "/signin").await?;
    ex.let_through(id).await.map_err(|f| f.0)?;
    tokio::time::sleep(Duration::from_millis(1_000)).await;
    ex.arm_rule(vec![utility.clone(), pay.clone()], Mode::Hold)
        .await
        .map_err(|f| f.0)?;
    ex.open(&format!("{utility}/bill")).await.map_err(|f| f.0)?;
    let (before, _) = requests_of(port).ok_or("state")?;
    let link = ex
        .resolve_one(&[Locator::Role("link", "Pay bill".into())], &[])
        .await
        .map_err(|f| f.0)?;
    ex.click(&link).await.map_err(|f| f.0)?;
    let first = held(ex, "POST", "/track").await?;
    let lines = ex.snapshot().await.map_err(|f| f.0)?;
    let held_through_snapshot = !lines.is_empty() && ex.paused().iter().any(|h| h.id == first);
    ex.open(&format!("{utility}/bill")).await.map_err(|f| f.0)?;
    let link = ex
        .resolve_one(&[Locator::Role("link", "Pay bill".into())], &[])
        .await
        .map_err(|f| f.0)?;
    ex.click(&link).await.map_err(|f| f.0)?;
    let deadline = Instant::now() + halvo_web::cdp::STEP_TIMEOUT;
    let second = loop {
        if let Some(h) = ex
            .paused()
            .into_iter()
            .find(|h| h.id != first && h.method == "POST" && h.path == "/track")
        {
            break h.id;
        }
        if Instant::now() > deadline {
            return Err("second_held_none");
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    let released = ex.let_through(first).await.is_ok();
    tokio::time::sleep(Duration::from_millis(2_000)).await;
    let (after, _) = requests_of(port).ok_or("state")?;
    let held_after = ex.paused().len();
    let outcome = |id: u64| {
        ex.requests()
            .into_iter()
            .find(|r| r.id == Some(id))
            .map(|r| r.outcome)
    };
    Ok(HoldRead {
        held_through_snapshot,
        released,
        counted: after - before,
        held_after,
        first: outcome(first),
        second: outcome(second),
    })
}

/// What the hop case read: the listener's arrivals of the state-changing request, whether the
/// record holds the released request as let through and the hop as a failed request of its own,
/// and the arrivals that the row's own client made when it followed the 307 once (the control).
struct HopRead {
    arrivals: i64,
    released: bool,
    hop_failed: bool,
    control: i64,
}

/// The row's own client, outside the browser: a POST of `path` to the listener on 127.0.0.1:`port`;
/// gives the answer's status and Location line.
fn plain_post(port: u16, path: &str) -> Option<(u16, Option<String>)> {
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(10))).ok()?;
    let req = format!(
        "POST {path} HTTP/1.0\r\nHost: utility.agent.test:{port}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: 3\r\n\r\nn=1"
    );
    s.write_all(req.as_bytes()).ok()?;
    let mut raw = String::new();
    s.read_to_string(&mut raw).ok()?;
    let status = raw.split_whitespace().nth(1)?.parse().ok()?;
    let location = raw.lines().take_while(|l| !l.is_empty()).find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim()
            .eq_ignore_ascii_case("location")
            .then(|| v.trim().to_string())
    });
    Some((status, location))
}

/// The listener's count of arrivals of POST /act.
fn hop_arrivals(port: u16) -> Option<i64> {
    fixture_json(port, "GET", "/count")?["act"].as_i64()
}

async fn hop_case(ex: &mut Executor, port: u16) -> Result<HopRead, &'static str> {
    use halvo_web::cdp::{Locator, Outcome};
    fixture_json(port, "GET", "/reset").ok_or("listener")?;
    let origin = format!("http://utility.agent.test:{port}");
    ex.arm_rule(vec![origin.clone()], Mode::Hold)
        .await
        .map_err(|f| f.0)?;
    ex.open(&format!("{origin}/page")).await.map_err(|f| f.0)?;
    let send = ex
        .resolve_one(&[Locator::Role("button", "Send".into())], &[])
        .await
        .map_err(|f| f.0)?;
    ex.click(&send).await.map_err(|f| f.0)?;
    let id = held(ex, "POST", "/act").await?;
    ex.let_through(id).await.map_err(|f| f.0)?;
    tokio::time::sleep(Duration::from_millis(3_000)).await;
    let arrivals = hop_arrivals(port).ok_or("count")?;
    let records = ex.requests();
    let released = records
        .iter()
        .any(|r| r.id == Some(id) && r.outcome == Outcome::Continued);
    let hop_failed = records.iter().any(|r| {
        r.method == "POST" && r.path == "/act" && r.id != Some(id) && r.outcome == Outcome::Failed
    });
    fixture_json(port, "GET", "/reset").ok_or("listener")?;
    if let Some((307, Some(next))) = plain_post(port, "/act") {
        let _ = plain_post(port, &next);
    }
    let control = hop_arrivals(port).ok_or("count")?;
    Ok(HopRead {
        arrivals,
        released,
        hop_failed,
        control,
    })
}

/// The row of CP-176 A1 (f) through the product's executor, on the fixture's bill page with the
/// variant `get_effect`, whose link `Pay bill` sends a state-changing `POST /track` by the page's
/// own script: the first request stays held while a snapshot of the page is read; the page sends a
/// second one meanwhile; the first is let through once by its id and the second is failed. ok when
/// the fixture counts exactly one more state-changing request, the first went on, the second was
/// failed and nothing is held after the release.
async fn hold_release(argv: &[String]) -> (usize, usize) {
    let (Some(browser), Some(kind), Some(port), Some(dir)) = (
        arg(argv, "--browser"),
        arg(argv, "--kind"),
        arg(argv, "--port").and_then(|p| p.parse::<u16>().ok()),
        scratch_of(argv, "hold-release"),
    ) else {
        return (0, 1);
    };
    let browser = std::path::absolute(&browser).unwrap_or(PathBuf::from(&browser));
    let machine = if cfg!(windows) { "windows" } else { "macos" };
    if fixture_json(port, "POST", "/reset?seed=1&variant=get_effect").is_none() {
        println!("row hold_release machine={machine} browser={kind} result=notrun cause=fixture");
        return (0, 1);
    }
    let Ok(relay) = Relay::start(Names::Fixture).await else {
        return (0, 1);
    };
    for host in ["utility.agent.test", "pay.agent.test"] {
        relay
            .handle()
            .allow(host, port, halvo_web::relay::Entry::Listed);
    }
    let Some(opts) = options(&browser, dir.join("p"), &relay) else {
        return (0, 1);
    };
    let mut ex = match Executor::start(&opts).await {
        Ok(ex) => ex,
        Err(f) => {
            println!(
                "row hold_release machine={machine} browser={kind} result=notrun cause={}",
                f.cause
            );
            return (0, 1);
        }
    };
    let steps = hold_release_steps(&mut ex, port).await;
    let hop_port = arg(argv, "--hop-port").and_then(|p| p.parse::<u16>().ok());
    let hop = match hop_port {
        Some(hp) => {
            relay
                .handle()
                .allow("utility.agent.test", hp, halvo_web::relay::Entry::Listed);
            Some(hop_case(&mut ex, hp).await)
        }
        None => None,
    };
    let stopped = ex.stop_and_remove().await;
    relay.stop();
    let _ = fixture_json(port, "POST", "/reset?seed=1");
    let _ = std::fs::remove_dir(&dir);
    use halvo_web::cdp::Outcome;
    let name = |o: Option<Outcome>| match o {
        Some(Outcome::Continued) => "continued",
        Some(Outcome::Failed) => "failed",
        Some(Outcome::Held) => "held",
        Some(Outcome::Refused) => "refused",
        None => "-",
    };
    let (line, fine) = match steps {
        Ok(r) => (
            format!(
                "held_through_snapshot={} released={} first={} second={} counted={} held_after={} removed={}",
                if r.held_through_snapshot { "yes" } else { "no" },
                if r.released { "yes" } else { "no" },
                name(r.first),
                name(r.second),
                r.counted,
                r.held_after,
                if stopped.removed { "yes" } else { "no" }
            ),
            r.held_through_snapshot
                && r.released
                && r.first == Some(Outcome::Continued)
                && r.second == Some(Outcome::Failed)
                && r.counted == 1
                && r.held_after == 0
                && stopped.removed,
        ),
        Err(cause) => (format!("cause={cause}"), false),
    };
    println!(
        "row hold_release machine={machine} browser={kind} {line} result={}",
        if fine { "ok" } else { "miss" }
    );
    let hop_fine = match hop {
        None => true,
        Some(Ok(h)) => {
            let fine = h.arrivals == 1 && h.released && h.hop_failed && h.control == 2;
            println!(
                "row hold_release machine={machine} browser={kind} case=hop307 arrivals={} released={} hop_failed={} control_arrivals={} result={}{}",
                h.arrivals,
                if h.released { "yes" } else { "no" },
                if h.hop_failed { "yes" } else { "no" },
                h.control,
                if fine { "ok" } else { "miss" },
                if h.arrivals >= 2 {
                    " cause=hop_let_through"
                } else {
                    ""
                }
            );
            fine
        }
        Some(Err(cause)) => {
            println!(
                "row hold_release machine={machine} browser={kind} case=hop307 result=notrun cause={cause}"
            );
            false
        }
    };
    (usize::from(fine && hop_fine), 1)
}

/// The contexts of the listener's page that the row requires: a document, a frame and a worker.
const WS_CONTEXTS: [&str; 3] = ["page", "frame", "worker"];

/// The row's own WebSocket handshake to the listener, outside the browser: the control that the
/// listener counts a handshake at all.
fn own_handshake(port: u16) -> bool {
    use std::io::{Read, Write};
    let Ok(mut s) = std::net::TcpStream::connect(("127.0.0.1", port)) else {
        return false;
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    let req = format!(
        "GET /ws?ctx=control HTTP/1.1\r\nHost: utility.agent.test:{port}\r\nConnection: Upgrade\r\n\
         Upgrade: websocket\r\nSec-WebSocket-Version: 13\r\n\
         Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n"
    );
    if s.write_all(req.as_bytes()).is_err() {
        return false;
    }
    let mut sink = Vec::new();
    let _ = s.read_to_end(&mut sink);
    true
}

/// The contexts of `map` in the listener's counts with a count above 0.
fn contexts(counts: &serde_json::Value, map: &str) -> Vec<String> {
    counts[map]
        .as_object()
        .map(|o| {
            o.iter()
                .filter(|(_, v)| v.as_i64().unwrap_or(0) > 0)
                .map(|(k, _)| k.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// The row of CP-176 A2 (c): the listener's page through the product's executor on the pipe, in
/// the mode that fails, which lets the page's reads through. ok when the row's own handshake was
/// counted, no handshake came from the browser, the page, its frame and its worker each fetched
/// and reported a failed WebSocket, and the executor failed no document.
async fn ws_allowed(argv: &[String]) -> (usize, usize) {
    let (Some(browser), Some(kind), Some(port), Some(dir)) = (
        arg(argv, "--browser"),
        arg(argv, "--kind"),
        arg(argv, "--ws-port").and_then(|p| p.parse::<u16>().ok()),
        scratch_of(argv, "ws-allowed"),
    ) else {
        return (0, 1);
    };
    let browser = std::path::absolute(&browser).unwrap_or(PathBuf::from(&browser));
    let machine = if cfg!(windows) { "windows" } else { "macos" };
    // The listener's counts start at none for each browser (run 43480096367: Edge read the
    // control that Chrome's run had counted already).
    if fixture_json(port, "GET", "/reset").is_none() {
        println!("row ws_allowed machine={machine} browser={kind} result=notrun cause=listener");
        return (0, 1);
    }
    let control = own_handshake(port)
        && fixture_json(port, "GET", "/count")
            .is_some_and(|c| c["ws"]["control"].as_i64() == Some(1));
    let Ok(relay) = Relay::start(Names::Fixture).await else {
        return (0, 1);
    };
    relay
        .handle()
        .allow("utility.agent.test", port, halvo_web::relay::Entry::Listed);
    let Some(opts) = options(&browser, dir.join("p"), &relay) else {
        return (0, 1);
    };
    let mut ex = match Executor::start(&opts).await {
        Ok(ex) => ex,
        Err(f) => {
            println!(
                "row ws_allowed machine={machine} browser={kind} result=notrun cause={}",
                f.cause
            );
            return (0, 1);
        }
    };
    let origin = format!("http://utility.agent.test:{port}");
    let opened = match ex.arm_rule(vec![origin.clone()], Mode::Fail).await {
        Ok(()) => ex.open(&format!("{origin}/page")).await.is_ok(),
        Err(_) => false,
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut counts = serde_json::Value::Null;
    while Instant::now() < deadline {
        counts = fixture_json(port, "GET", "/count").unwrap_or(serde_json::Value::Null);
        let refused = contexts(&counts, "e");
        if WS_CONTEXTS.iter().all(|c| refused.iter().any(|r| r == c)) {
            tokio::time::sleep(Duration::from_millis(1_000)).await;
            counts = fixture_json(port, "GET", "/count").unwrap_or(counts);
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let policy = ex.policy_counts();
    let stopped = ex.stop_and_remove().await;
    relay.stop();
    let _ = std::fs::remove_dir(&dir);
    let handshakes: i64 = counts["ws"]
        .as_object()
        .map(|o| {
            o.iter()
                .filter(|(k, _)| k.as_str() != "control")
                .map(|(_, v)| v.as_i64().unwrap_or(0))
                .sum()
        })
        .unwrap_or(-1);
    let (tried, refused, data_blob) = (
        contexts(&counts, "f"),
        contexts(&counts, "e"),
        contexts(&counts, "r"),
    );
    let all = |got: &[String]| WS_CONTEXTS.iter().all(|c| got.iter().any(|g| g == c));
    let count = |k: &str| policy.get(k).copied().unwrap_or(0);
    let fine = control
        && opened
        && handshakes == 0
        && all(&tried)
        && all(&refused)
        && count("document_failed") == 0
        && count("fulfilled") >= 1
        && stopped.removed;
    let join = |v: &[String]| {
        let mut v = v.to_vec();
        v.sort();
        if v.is_empty() {
            "-".to_string()
        } else {
            v.join(",")
        }
    };
    println!(
        "row ws_allowed machine={machine} browser={kind} control={} handshakes={handshakes} \
         tried={} refused={} data_blob={} fulfilled={} line_added={} passed={} \
         document_failed={} script_failed={} removed={} result={}",
        if control { "yes" } else { "no" },
        join(&tried),
        join(&refused),
        join(&data_blob),
        count("fulfilled"),
        count("line_added"),
        count("passed"),
        count("document_failed"),
        count("script_failed"),
        if stopped.removed { "yes" } else { "no" },
        if fine { "ok" } else { "miss" }
    );
    (usize::from(fine), 1)
}

/// One crash reading with the product's means on a relay at `--relay-port`, which the egress check
/// of A13.3's third item wraps in the crash row's hosted step, so that its denials count what the
/// run tried to send (CP-165 (2) (b)); exit 0 when the reading ran.
async fn crash_one(argv: &[String]) -> (usize, usize) {
    let (Some(browser), Some(port), Some(relay_port), Some(dir)) = (
        arg(argv, "--browser"),
        arg(argv, "--port").and_then(|p| p.parse::<u16>().ok()),
        arg(argv, "--relay-port").and_then(|p| p.parse::<u16>().ok()),
        scratch_of(argv, "crash-one"),
    ) else {
        return (0, 1);
    };
    let browser = std::path::absolute(&browser).unwrap_or(PathBuf::from(&browser));
    let origin = origin_of(&dir);
    let got = crash_reading_at(
        &browser,
        port,
        &dir,
        "egress",
        CrashMeans::Product,
        true,
        relay_port,
        &origin,
    )
    .await;
    let _ = std::fs::remove_dir(&dir);
    match got {
        Ok(_) => {
            println!("row crash_one result=ok");
            (1, 1)
        }
        Err(cause) => {
            println!("row crash_one result=notrun cause={cause}");
            (0, 1)
        }
    }
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
    let probe_places = crash_row::places(&dir.join("p-origin"), &dir.join("p-origin-tmp"));
    if let Some(under) = crash_row::scratch_under_place(&dir, &probe_places) {
        println!(
            "row crash machine={machine} browser={kind} result=notrun cause=scratch_under_place place={under}"
        );
        let _ = std::fs::remove_dir(&dir);
        return (0, 1);
    }
    let origin = origin_of(&dir);
    let baseline = match crash_reading(
        &browser,
        port,
        &dir,
        "baseline",
        CrashMeans::Product,
        false,
        &origin,
    )
    .await
    {
        Ok(r) => r,
        Err(cause) => {
            println!("row crash machine={machine} browser={kind} result=notrun cause={cause}");
            return (0, 1);
        }
    };
    let control = crash_reading(
        &browser,
        port,
        &dir,
        "control",
        CrashMeans::Without,
        true,
        &origin,
    )
    .await;
    let mut means: Vec<(&str, CrashMeans)> = vec![("product", CrashMeans::Product)];
    for (i, (name, _)) in halvo_web::CRASH_CANDIDATES.iter().enumerate() {
        means.push((name, CrashMeans::Candidate(i)));
    }
    let mut product_ok = false;
    for (name, m) in means {
        let with = crash_reading(
            &browser,
            port,
            &dir,
            &format!("means-{name}"),
            m,
            true,
            &origin,
        )
        .await;
        let line = match (&with, &control) {
            (Ok(w), Ok(c)) => {
                let beyond = crash_row::beyond(&w.left, &baseline.left);
                let control_dumps: Vec<&crash_row::File> =
                    c.changed.iter().filter(|f| crash_row::is_dump(f)).collect();
                let verdict = crash_row::result(
                    &w.places,
                    &origin,
                    &facts_of(&baseline),
                    &facts_of(w),
                    &c.changed,
                    w.sent,
                );
                let result = verdict.result;
                let cause = if result == "ok" {
                    String::new()
                } else {
                    format!(" cause={}", verdict.cause)
                };
                if name == "product" {
                    product_ok = result == "ok";
                }
                format!(
                    "row crash machine={machine} browser={kind} major={} means={name} beyond_count={} beyond_links={} beyond_bytes={} beyond_places={} control_dumps={} control_dump_bytes={} control_places={} sent_after_crash={} dump_inside_profile={} handler={} removed={} result={result}",
                    w.major,
                    beyond.len(),
                    beyond.iter().filter(|f| f.link).count(),
                    beyond.iter().map(|f| f.size).sum::<u64>(),
                    by_place(&beyond),
                    control_dumps.len(),
                    control_dumps.iter().map(|f| f.size).sum::<u64>(),
                    by_place(&c.changed.iter().filter(|f| crash_row::is_dump(f)).cloned().collect::<Vec<_>>()),
                    w.sent,
                    if w.inside_dumps > 0 { "yes" } else { "no" },
                    w.handler,
                    if w.removed && baseline.removed { "yes" } else { "no" },
                ) + &format!(
                    " removed_files_deciding={}",
                    crash_row::removals(&w.changed, &w.places, &origin).deciding
                ) + &cause
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
    let Ok(started) = Executor::start(&opts).await else {
        return ExitCode::from(1);
    };
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

/// One read of what the run of `mark` has running (`rows::read_left`): its own processes, and its
/// guard, this program run with the guard's flag and the run's temporary path; the row's own child
/// `child`, which names the profile in its arguments, is no process of the run.
fn left_of(
    mark: &halvo_web::checks::Mark,
    temp: &str,
    me: &Path,
    child: u32,
) -> halvo_web::rows::Left {
    let table: Option<Vec<procs::Proc>> = procs::table()
        .ok()
        .map(|t| t.into_iter().filter(|p| p.pid != child).collect());
    halvo_web::rows::read_left(
        table.as_deref(),
        mark,
        &halvo_web::checks::command_args,
        &|p| {
            p.exe.as_deref().is_some_and(|e| same_path(e, me))
                && halvo_web::checks::command_args(p.pid).is_some_and(|a| {
                    a.iter().any(|x| x == halvo_web::guard::GUARD_FLAG)
                        && a.iter().any(|x| x.contains(temp))
                })
        },
    )
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
    let (mut ok, mut miss, mut notrun, mut draws) = (0usize, 0usize, 0usize, 0usize);
    while ok + miss < runs && draws < 3 * runs {
        draws += 1;
        let i = draws;
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
                notrun += 1;
                println!("row killed_start run={i} result=notrun cause=child");
                continue;
            }
        };
        let started = Instant::now();
        let mut seen = 0usize;
        while started.elapsed() < Duration::from_millis(at_ms) {
            seen = seen.max(left_of(&mark, &temp, &me_real, child.id()).own);
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let _ = child.kill();
        let _ = child.wait();
        let killed = Instant::now();
        let (gone_ms, last) = loop {
            let left = left_of(&mark, &temp, &me_real, child.id());
            seen = seen.max(left.own);
            if left.gone() {
                break (Some(killed.elapsed().as_millis()), left);
            }
            if killed.elapsed() > LEFT_WAIT {
                break (None, left);
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        let ended = if last.own > 0 {
            halvo_web::checks::end_runs_own(&mark).ended
        } else {
            0
        };
        let result = match (seen, gone_ms) {
            (0, _) => "notrun",
            (_, Some(_)) => "ok",
            (_, None) => "miss",
        };
        match result {
            "ok" => ok += 1,
            "miss" => miss += 1,
            _ => notrun += 1,
        }
        println!(
            "row killed_start run={i} at_ms={at_ms} seen={seen} gone_ms={} left={} guards_left={} cause={} ended_after={ended} result={result}",
            gone_ms.map_or("-".to_string(), |m| m.to_string()),
            last.own,
            last.guards,
            last.open.unwrap_or("-"),
        );
    }
    println!("row killed_start ok={ok} miss={miss} notrun={notrun} draws={draws}");
    if miss > 0 {
        return (0, runs);
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

/// Whether the relay's cause of refusal is the one that a cell of `relay_foreign` is for (CP-170
/// E8 (c)): `outside_tree` for a process of this account, which the walk reads; for a process of
/// another account also `no_owner`, because the walk cannot read its descriptors on macOS.
fn cell_cause_fits(who: &str, cause: &str) -> bool {
    cause == "outside_tree" || (who == "other" && cause == "no_owner")
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
    relay.handle().allow(
        "utility.agent.test",
        echo_port,
        halvo_web::relay::Entry::Listed,
    );
    let Some(opts) = options(Path::new(&browser), dir.join("p"), &relay) else {
        return (0, 1);
    };
    let mut ex = match Executor::start(&opts).await {
        Ok(ex) => ex,
        Err(f) => {
            println!("row relay_foreign result=miss cause={}", f.cause);
            return (0, 1);
        }
    };
    let tunnels = relay.tunnels();
    let _ = ex
        .arm_rule(
            vec![format!("http://utility.agent.test:{echo_port}")],
            Mode::Fail,
        )
        .await;
    let _ = ex
        .open(&format!("http://utility.agent.test:{echo_port}/"))
        .await;
    let mut control = false;
    for _ in 0..50 {
        if echo.accept().is_ok() {
            control = relay.tunnels() > tunnels;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    // After the control the browser sends nothing more: its page's new connections to the listener,
    // which never answers, would otherwise reach it during the cells (run 43474813792, Windows).
    let _ = ex.arm_rule(Vec::new(), Mode::Fail).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    while echo.accept().is_ok() {}
    println!(
        "row relay_foreign control=browser_through_relay result={}",
        if control { "ok" } else { "notrun" }
    );
    let Ok(me) = std::env::current_exe() else {
        return (0, 1);
    };
    let mut cells: Vec<(&str, Result<std::process::Command, &str>)> = Vec::new();
    let mut same = std::process::Command::new(&me);
    same.arg("foreign_connect")
        .arg(relay.port.to_string())
        .arg(echo_port.to_string());
    cells.push(("same", Ok(same)));
    if let (Some(user), Some(copy_dir)) = (arg(argv, "--as"), arg(argv, "--copy-dir")) {
        let copy = Path::new(&copy_dir).join(if cfg!(windows) {
            "web-rows.exe"
        } else {
            "web-rows"
        });
        let other = if std::fs::copy(&me, &copy).is_err() {
            Err("copy")
        } else {
            as_other(&user, &copy, relay.port, echo_port, argv).ok_or("password_file")
        };
        cells.push(("other", other));
    }
    let mut ok = 0;
    let runs = cells.len();
    for (who, cmd) in cells {
        let mut cmd = match cmd {
            Ok(c) => c,
            Err(cause) => {
                println!("row relay_foreign as={who} cause={cause} result=miss");
                continue;
            }
        };
        let before = relay.handle().foreign_count();
        let tunnels_before = relay.tunnels();
        let out = cmd.stdin(std::process::Stdio::null()).start_output();
        let said = out
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();
        let foreign = relay.handle().foreign();
        let cause = foreign.last().map_or("-", |f| f.cause);
        let recorded = relay.handle().foreign_count() == before + 1;
        let accepted = usize::from(echo.accept().is_ok());
        let tunnelled = relay.tunnels() - tunnels_before;
        let fine = control
            && recorded
            && cell_cause_fits(who, cause)
            && said == "connect=refused"
            && accepted == 0
            && tunnelled == 0;
        ok += usize::from(fine);
        println!(
            "row relay_foreign as={who} cause={cause} recorded={} {said} accepted={accepted} tunnelled={tunnelled} result={}",
            if recorded { "yes" } else { "no" },
            if fine { "ok" } else if control { "miss" } else { "notrun" }
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
        "block_frame_post" => runtime.block_on(block_frame_post(&argv)),
        "hold_release" => runtime.block_on(hold_release(&argv)),
        "ws_allowed" => runtime.block_on(ws_allowed(&argv)),
        "crash_one" => runtime.block_on(crash_one(&argv)),
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
