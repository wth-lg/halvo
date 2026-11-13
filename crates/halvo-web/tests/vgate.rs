//! Verifier 40: the six rules of the start-to-stop check (A11.1) read on a session of a real
//! browser that the product's executor starts, headless, at the start, while the browser still
//! starts its helpers, and after the stop. It prints counts and yes or no alone, on the process's
//! own standard output. With no browser at a known place it prints `vgate browser=none`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use halvo_web::cdp::{Executor, StartOptions};
use halvo_web::checks::{check_named, read_holds, Subject};
use halvo_web::guard::GuardProgram;
use halvo_web::relay::{Names, Relay};

fn say(line: String) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

fn browsers() -> Vec<(&'static str, PathBuf)> {
    let places: [(&str, &str); 5] = [
        (
            "chrome",
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        ),
        (
            "chrome",
            "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
        ),
        (
            "chrome",
            "C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe",
        ),
        (
            "edge",
            "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe",
        ),
        (
            "edge",
            "C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe",
        ),
    ];
    let mut found: Vec<(&'static str, PathBuf)> = Vec::new();
    for (name, place) in places {
        let p = PathBuf::from(place);
        if p.is_file() && !found.iter().any(|f| f.0 == name) {
            found.push((name, p));
        }
    }
    found
}

fn line_of(counts: &[halvo_web::checks::Count]) -> String {
    counts
        .iter()
        .map(|c| format!("{}={}", c.rule, c.count))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The verifier's own reads beside the unit's check: the arguments of the tree that name a
/// debugging switch, by the switch's name, and on macOS the descriptors of the browser's first
/// process that are ends of the executor's pipe.
fn own_reads(
    browser: &str,
    read: usize,
    root: &halvo_web::procs::Proc,
    ids: &halvo_web::start::PipeIds,
) {
    let table = halvo_web::procs::table().unwrap_or_default();
    let tree = halvo_web::checks::tree_of(&table, root);
    let mut pipe = 0;
    let mut port = 0;
    let mut other = 0;
    let mut unread = 0;
    for p in &tree {
        match halvo_web::checks::command_args(p.pid) {
            None => unread += 1,
            Some(args) => {
                for a in args.iter().flat_map(|a| a.split(' ')) {
                    let low = a.to_ascii_lowercase();
                    if !low.contains("remote-debugging") {
                        continue;
                    }
                    if low.starts_with("--remote-debugging-pipe")
                        || low.starts_with("--remote-debugging-io-pipes")
                    {
                        pipe += 1;
                    } else if low.contains("remote-debugging-port") {
                        port += 1;
                    } else {
                        other += 1;
                    }
                }
            }
        }
    }
    let holders = halvo_web::checks::pipe_holder_procs(&table, root, ids).len();
    say(format!(
        "vgate own browser={browser} read={read} tree={} args_unread={unread} switch_pipe={pipe} switch_port={port} switch_other={other} holders_beside_first={holders}",
        tree.len()
    ));
    #[cfg(target_os = "macos")]
    {
        let ends: Vec<String> = halvo_web::peer::pipe_handles_of(root.pid)
            .unwrap_or_default()
            .into_iter()
            .filter(|(_, h)| *h != 0 && ids.handles.contains(h))
            .map(|(fd, h)| {
                let which = ids.handles.iter().position(|x| *x == h).unwrap_or(9);
                format!("{fd}:{}", halvo_web::checks::ENDS[which.min(3)])
            })
            .collect();
        let fds = halvo_web::peer::fds_of(root.pid).unwrap_or_default();
        let low: Vec<String> = fds
            .iter()
            .filter(|(fd, _)| *fd <= 4)
            .map(|(fd, t)| format!("{fd}:type{t}"))
            .collect();
        say(format!(
            "vgate own browser={browser} read={read} first_process_ends={} descriptors_0_to_4={}",
            if ends.is_empty() {
                "-".to_string()
            } else {
                ends.join(",")
            },
            low.join(",")
        ));
    }
}

async fn session(name: &str, browser: &Path) {
    let dir =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("vgate-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch");
    let relay = Relay::start(Names::Fixture).await.expect("relay");
    let opts = StartOptions {
        browser: browser.to_path_buf(),
        profile: dir.join("p"),
        headless: true,
        relay: relay.handle(),
        guard: GuardProgram {
            program: PathBuf::from(env!("CARGO_BIN_EXE_web-probe")),
            prefix: Vec::new(),
        },
    };
    let args = halvo_web::cdp::launch_args(&opts);
    say(format!(
        "vgate start browser={name} flags={} port_flags={} pipe_flags={} proxy_flags={}",
        args.len(),
        args.iter()
            .filter(
                |a| a.contains("remote-debugging-port") || a.contains("remote-debugging-address")
            )
            .count(),
        args.iter()
            .filter(|a| *a == "--remote-debugging-pipe")
            .count(),
        args.iter()
            .filter(|a| a.starts_with("--proxy-server="))
            .count(),
    ));
    let at = Instant::now();
    let ex = match Executor::start(&opts).await {
        Ok(ex) => ex,
        Err(f) => {
            say(format!(
                "vgate start browser={name} result=failed cause={} removed={}",
                f.cause, f.removed
            ));
            relay.stop();
            let _ = std::fs::remove_dir(&dir);
            return;
        }
    };
    let root = ex.root().clone();
    let ids = ex.pipe_ids().clone();
    let profile = dir.join("p");
    say(format!(
        "vgate start browser={name} result=ok first_reply_ms={} start_ms={}",
        ex.first_reply().as_millis(),
        at.elapsed().as_millis()
    ));
    let mut all = true;
    for read in 1..=6 {
        let subject = Subject {
            root: &root,
            profile: &profile,
            pipe: Some(&ids),
            relay_port: Some(relay.port),
        };
        let began = at.elapsed().as_millis();
        let (counts, named) = tokio::task::block_in_place(|| check_named(&subject));
        let holds = read_holds(&counts);
        all &= holds;
        say(format!(
            "vgate check browser={name} read={read} at_ms={began} took_ms={} {} named={} holds={}",
            at.elapsed().as_millis() - began,
            line_of(&counts),
            named.len(),
            if holds { "yes" } else { "no" }
        ));
        tokio::task::block_in_place(|| own_reads(name, read, &root, &ids));
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let keep = ex.keep_pipe();
    let stopped = ex.stop_and_remove().await;
    let subject = Subject {
        root: &root,
        profile: &profile,
        pipe: Some(&ids),
        relay_port: Some(relay.port),
    };
    let (counts, _) = tokio::task::block_in_place(|| check_named(&subject));
    let holds = read_holds(&counts);
    all &= holds;
    say(format!(
        "vgate check browser={name} read=after_stop {} holds={}",
        line_of(&counts),
        if holds { "yes" } else { "no" }
    ));
    drop(keep);
    say(format!(
        "vgate stop browser={name} tree={} adopted={} left={} removed={} every_read_holds={}",
        stopped.pids.len(),
        stopped.adopted.len(),
        stopped.left.len(),
        if stopped.removed { "yes" } else { "no" },
        if all { "yes" } else { "no" }
    ));
    relay.stop();
    let _ = std::fs::remove_dir(&dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_six_rules_on_a_session_of_a_real_browser() {
    let mut missed = 0;
    for _ in 0..200 {
        match halvo_web::procs::own() {
            Some(p) if p.started_us != 0 => {}
            _ => missed += 1,
        }
    }
    say(format!(
        "vgate own_row reads=200 missed_or_without_start_time={missed}"
    ));
    let found = browsers();
    if found.is_empty() {
        say("vgate browser=none".to_string());
        return;
    }
    for (name, path) in found {
        session(name, &path).await;
    }
}
