use std::io::Write;

use r3_web::start::Start;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::ExitCode;

use r3_web::{
    browser_kind, CANDIDATE, DRIVER_STEPS, EGRESS_STEPS, EGRESS_WRAP_FLAG, ISOLATION_STEPS,
    ORG_STEPS, PORT_FREE_STEP, REACH_STEPS, RELAY_STEPS, ROUND3_STEPS, STEPS, WAY_STEPS,
};

#[derive(Default)]
struct Args {
    browser: Option<String>,
    port: Option<u16>,
    runs: Option<u32>,
    step: Option<String>,
    isolation: bool,
    driver: bool,
    relay_rows: bool,
    way_rows: bool,
    egress_rows: bool,
    reach_rows: bool,
    net_log: Option<PathBuf>,
    round2: bool,
    add_flags: Vec<String>,
    machine: Option<String>,
    repo: Option<PathBuf>,
    scratch: Option<PathBuf>,
    fault: Option<String>,
    mode: Mode,
    added_bytes: Option<u64>,
    out: Option<PathBuf>,
    headed: bool,
    relay: bool,
    routes: bool,
    round3: bool,
    port_channel: bool,
    round3_rows: bool,
    org_rows: bool,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum Mode {
    #[default]
    Cells,
    Extras,
    Resolver,
    SnapshotPay,
}

fn refuse(cause: &str, detail: &str) -> ExitCode {
    eprintln!("REFUSED {cause}: {detail}");
    ExitCode::from(2)
}

fn parse(argv: &[String]) -> Result<Args, String> {
    let mut a = Args::default();
    let mut i = 0;
    while i < argv.len() {
        let flag = argv[i].as_str();
        let value = argv.get(i + 1).cloned();
        let mut take = || {
            i += 1;
            value.clone().ok_or_else(|| format!("{flag} needs a value"))
        };
        match flag {
            "--candidate" => {
                if take()? != CANDIDATE {
                    return Err("--candidate takes cdp; no other adapter is built".into());
                }
            }
            "--browser" => a.browser = Some(take()?),
            "--port" => {
                a.port = Some(
                    take()?
                        .parse()
                        .ok()
                        .filter(|p| *p > 0 && *p < 65533)
                        .ok_or("--port takes 1 to 65532")?,
                )
            }
            "--runs" => {
                a.runs = Some(
                    take()?
                        .parse()
                        .ok()
                        .filter(|r| *r > 0)
                        .ok_or("--runs takes a count from 1")?,
                )
            }
            "--step" => a.step = Some(take()?),
            "--rows" => {
                match take()?.as_str() {
                    "isolation" => a.isolation = true,
                    "driver" => a.driver = true,
                    "relay" => a.relay_rows = true,
                    "ways" => a.way_rows = true,
                    "egress" => a.egress_rows = true,
                    "reach" => a.reach_rows = true,
                    "round3" => a.round3_rows = true,
                    "org" => a.org_rows = true,
                    _ => return Err(
                        "--rows takes isolation, driver, relay, ways, egress, reach, round3 or org"
                            .into(),
                    ),
                }
            }
            "--add-flag" => a.add_flags.push(take()?),
            "--machine" => {
                let m = take()?;
                if !["mac", "hosted-mac", "hosted-win"].contains(&m.as_str()) {
                    return Err("--machine takes mac, hosted-mac or hosted-win".into());
                }
                a.machine = Some(m);
            }
            "--repo" => a.repo = Some(PathBuf::from(take()?)),
            "--scratch" => a.scratch = Some(PathBuf::from(take()?)),
            "--fault" => a.fault = Some(take()?),
            "--added-bytes" => {
                a.added_bytes = Some(take()?.parse().map_err(|_| "--added-bytes takes a count")?)
            }
            "--out" => a.out = Some(PathBuf::from(take()?)),
            "--extras" => a.mode = Mode::Extras,
            "--resolver-probe" => a.mode = Mode::Resolver,
            "--snapshot-pay" => a.mode = Mode::SnapshotPay,
            "--headed" => a.headed = true,
            "--relay" => a.relay = true,
            "--net-log" => a.net_log = Some(PathBuf::from(take()?)),
            "--round" => match take()?.as_str() {
                "2" => a.round2 = true,
                "3" => {
                    a.round2 = true;
                    a.round3 = true;
                }
                _ => return Err("--round takes 2 or 3".into()),
            },
            "--channel" => match take()?.as_str() {
                "pipe" => a.port_channel = false,
                "port" => a.port_channel = true,
                _ => return Err("--channel takes pipe or port".into()),
            },
            "--routes" => a.routes = true,
            _ => return Err(format!("unknown argument {flag}")),
        }
        i += 1;
    }
    if a.browser.is_none()
        || a.port.is_none()
        || a.machine.is_none()
        || a.repo.is_none()
        || a.scratch.is_none()
    {
        return Err("--browser, --port, --machine, --repo and --scratch are required".into());
    }
    if a.mode == Mode::Cells && a.runs.is_none() {
        return Err("--runs is required".into());
    }
    if a.mode == Mode::Extras && a.added_bytes.is_none() {
        return Err("--extras needs --added-bytes".into());
    }
    if a.mode == Mode::SnapshotPay && a.out.is_none() {
        return Err("--snapshot-pay needs --out".into());
    }
    if a.fault.is_some() && a.mode != Mode::Cells {
        return Err("--fault runs cells only".into());
    }
    let driver_rows: Vec<&str> = DRIVER_STEPS
        .iter()
        .copied()
        .chain(a.round3.then_some(PORT_FREE_STEP))
        .collect();
    let rows: &[&str] = if a.round3_rows {
        &ROUND3_STEPS
    } else if a.org_rows {
        &ORG_STEPS
    } else if a.reach_rows {
        &REACH_STEPS
    } else if a.egress_rows {
        &EGRESS_STEPS
    } else if a.way_rows {
        &WAY_STEPS
    } else if a.relay_rows {
        &RELAY_STEPS
    } else if a.driver {
        &driver_rows
    } else if a.isolation {
        &ISOLATION_STEPS
    } else {
        &STEPS
    };
    if let Some(s) = &a.step {
        if !rows.contains(&s.as_str()) {
            return Err(format!("--step takes one of {}", rows.join(",")));
        }
    }
    if (a.isolation || a.driver) && (a.mode != Mode::Cells || a.fault.is_some()) {
        return Err("--rows isolation and --rows driver run cells with no fault".into());
    }
    if (a.round2 || a.relay_rows || a.way_rows || a.egress_rows || a.reach_rows)
        && !a.relay
        && !a.driver
    {
        return Err("--round 2 and --rows relay need --relay".into());
    }
    if (a.relay_rows || a.way_rows || a.egress_rows || a.reach_rows) && !a.round2 {
        return Err("--rows relay belongs to --round 2".into());
    }
    if (a.round3_rows || a.org_rows) && !(a.round3 && a.relay) {
        return Err("--rows round3 and --rows org belong to --round 3 with --relay".into());
    }
    if !a.isolation && !a.add_flags.is_empty() {
        return Err("--add-flag belongs to --rows isolation".into());
    }
    Ok(a)
}

fn egress_wrap(spec: &str, argv: &[String]) -> ExitCode {
    let parts: Option<Vec<String>> = (0..spec.len() / 2)
        .map(|i| u8::from_str_radix(spec.get(2 * i..2 * i + 2)?, 16).ok())
        .collect::<Option<Vec<u8>>>()
        .and_then(|b| serde_json::from_slice(&b).ok());
    let Some([repo, list, browser, outfile]) = parts.as_deref() else {
        eprintln!("REFUSED usage: the egress wrap flag holds no four paths");
        return ExitCode::from(2);
    };
    let rest = argv.iter().filter(|a| !a.starts_with(EGRESS_WRAP_FLAG));
    let err_file = match std::fs::File::create(outfile) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("REFUSED start: {} {}", outfile, e.kind());
            return ExitCode::from(2);
        }
    };
    let spawned = std::process::Command::new("sh")
        .current_dir(repo)
        .arg("scripts/egress/check.sh")
        .arg(list)
        .arg("--")
        .arg(browser)
        .args(rest)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::from(err_file))
        .start();
    let mut child = match spawned {
        Ok(c) => c,
        Err(e) => {
            eprintln!("REFUSED start: sh {}", e.kind());
            return ExitCode::from(2);
        }
    };
    let mut reader = std::fs::File::open(outfile).ok();
    let mut buf = vec![0u8; 65536];
    loop {
        let done = !matches!(child.try_wait(), Ok(None));
        let mut read_any = false;
        if let Some(r) = reader.as_mut() {
            while let Ok(n) = std::io::Read::read(r, &mut buf) {
                if n == 0 {
                    break;
                }
                read_any = true;
                let _ = std::io::stderr().write_all(&buf[..n]);
            }
        }
        if done && !read_any {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    match child.wait() {
        Ok(s) => ExitCode::from(s.code().unwrap_or(1).clamp(0, 255) as u8),
        Err(_) => ExitCode::from(2),
    }
}

fn foreign_probe(argv: &[String]) -> ExitCode {
    use std::io::Read;
    let (Some(relay), Some(host), Some(port)) = (
        argv.first().and_then(|p| p.parse::<u16>().ok()),
        argv.get(1),
        argv.get(2).and_then(|p| p.parse::<u16>().ok()),
    ) else {
        eprintln!("REFUSED usage: --foreign-probe RELAY_PORT HOST PORT");
        return ExitCode::from(2);
    };
    let Ok(mut c) = std::net::TcpStream::connect(("127.0.0.1", relay)) else {
        println!("foreign connected=no bytes=0");
        return ExitCode::SUCCESS;
    };
    let _ = c.set_read_timeout(Some(std::time::Duration::from_secs(5)));
    let mut req = vec![5u8, 1, 0, 5, 1, 0, 3, host.len() as u8];
    req.extend_from_slice(host.as_bytes());
    req.extend_from_slice(&port.to_be_bytes());
    req.extend_from_slice(b"GET /foreign HTTP/1.1\r\nHost: x\r\n\r\n");
    let _ = c.write_all(&req);
    let mut got = Vec::new();
    let mut buf = [0u8; 512];
    let end = loop {
        match c.read(&mut buf) {
            Ok(0) => break "eof",
            Ok(n) => got.extend_from_slice(&buf[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => break "reset",
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                break "timeout"
            }
            Err(_) => break "error",
        }
    };
    println!("foreign connected=yes bytes={} end={end}", got.len());
    ExitCode::SUCCESS
}

fn port_free(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_ok()
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if let Some(spec) = argv.iter().find_map(|a| a.strip_prefix(EGRESS_WRAP_FLAG)) {
        return egress_wrap(spec, &argv);
    }
    if argv.first().map(String::as_str) == Some("--driver-child") {
        return driver_child(&argv[1..]);
    }
    #[cfg(feature = "driver")]
    if argv.first().map(String::as_str) == Some("--org-control") {
        return driver::org_control(&argv[1..]);
    }
    if argv.first().map(String::as_str) == Some("--policy-named") {
        let (Some(name), Some(file)) = (argv.get(1), argv.get(2)) else {
            eprintln!("REFUSED usage: --policy-named NAME FILE [VALUE_READ]");
            return ExitCode::from(2);
        };
        let Ok(text) = std::fs::read_to_string(file) else {
            out("policy_named named=no how=unread");
            return ExitCode::from(1);
        };
        let printed = argv
            .get(3)
            .and_then(|v| r3_web::policy::printed_value(name, v));
        let n = r3_web::policy::named_with(&text, name, printed);
        out(&format!(
            "policy_named named={} how={}",
            if n.named() { "yes" } else { "no" },
            n.word()
        ));
        return ExitCode::from(if n.named() { 0 } else { 1 });
    }
    #[cfg(feature = "driver")]
    if argv.first().map(String::as_str) == Some(r3_web::relay::GUARD_FLAG) {
        return ExitCode::from(r3_web::relay::guard_main(&argv[1..]).clamp(0, 255) as u8);
    }
    if argv.first().map(String::as_str) == Some("--hold-child") {
        std::thread::sleep(std::time::Duration::from_secs(60));
        return ExitCode::SUCCESS;
    }
    if argv.first().map(String::as_str) == Some("--foreign-probe") {
        return foreign_probe(&argv[1..]);
    }
    let args = match parse(&argv) {
        Ok(a) => a,
        Err(e) => return refuse("usage", &e),
    };
    let browser = args.browser.clone().unwrap_or_default();
    let Some(kind) = browser_kind(&browser) else {
        return refuse("browser", "the file name is no Chrome or Edge binary");
    };
    if !std::path::Path::new(&browser).is_file() {
        return refuse("browser", "no file at the path");
    }
    let port = args.port.unwrap_or_default();
    let needed: Vec<u16> = match (&args.fault, args.mode) {
        (Some(_), _) => vec![port, port + 2],
        (None, Mode::Extras) => vec![port, port + 1],
        (None, _) => vec![port],
    };
    if let Some(p) = needed.iter().find(|p| !port_free(**p)) {
        return refuse("port", &format!("127.0.0.1:{p} is taken"));
    }
    run(args, kind)
}

#[cfg(not(feature = "driver"))]
fn driver_child(_argv: &[String]) -> ExitCode {
    ExitCode::from(1)
}

#[cfg(feature = "driver")]
fn driver_child(argv: &[String]) -> ExitCode {
    driver::child_main(argv)
}

#[cfg(not(feature = "driver"))]
fn run(args: Args, _kind: &'static str) -> ExitCode {
    let repo = args.repo.unwrap_or_default();
    let _ = r3_web::fixture::Seed::load(&repo, 1);
    out(&format!("notrun candidate={CANDIDATE} cause=no_driver"));
    ExitCode::from(1)
}

#[cfg(feature = "driver")]
fn run(args: Args, kind: &'static str) -> ExitCode {
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            println!("notrun candidate={CANDIDATE} cause=runtime");
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };
    rt.block_on(driver::main(args, kind))
}

#[cfg(feature = "driver")]
mod driver {
    use r3_web::start::Start;
    use std::path::{Path, PathBuf};
    use std::process::ExitCode;
    use std::time::{Duration, Instant};

    use r3_web::cdp::host_of;
    use r3_web::cdp::{
        Channel, Executor, Fault, Locator, Outcome, PausedRequest, Res, Resolved, SnapshotLine,
        StartOptions,
    };
    use r3_web::cdp::{NO_CHANNEL_MESSAGE, NO_ROUTE_MESSAGE, PROBE_HOST, PROBE_PORT};
    use r3_web::checks;
    use r3_web::fixture::{count, fixture_program, origin, Control, Seed, Server, PAY, UTILITY};
    use r3_web::relay::{relay_flags, HostPort, Relay, RelayHandle};
    use r3_web::tls::{make_certificate, openssl_runs, CertFault, Listener};
    use r3_web::{
        extract_amount, major_of, median, same_site_local_state, variants_of, CANDIDATE,
        EGRESS_STEPS, EGRESS_WRAP_FLAG, FIRST_FLAGS, ISOLATION_STEPS, ORG_STEPS, PORT_FREE_STEP,
        REACH_STEPS, RELAY_STEPS, RESOLVER_RULE, ROUND2_FLAGS, ROUND3_STEPS, STEPS, WAY_STEPS,
        WS_POLICY,
    };

    use super::{out, Args, Mode};

    const NO_SANDBOX_INSIDE_THE_EGRESS_CHECK: &str = "--no-sandbox";

    #[derive(Clone)]
    pub struct Stop(tokio::sync::watch::Receiver<bool>);

    impl Stop {
        fn is_set(&self) -> bool {
            *self.0.borrow()
        }

        async fn wait(&self) {
            let mut rx = self.0.clone();
            if rx.wait_for(|stopped| *stopped).await.is_err() {
                std::future::pending::<()>().await;
            }
        }

        async fn or<T>(&self, work: impl std::future::Future<Output = Res<T>>) -> Res<T> {
            tokio::select! {
                biased;
                _ = self.wait() => Err(Fault::new("signal", "stopped")),
                r = work => r,
            }
        }
    }

    fn watch_signals() -> std::io::Result<Stop> {
        let (tx, rx) = tokio::sync::watch::channel(false);
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            let mut term = signal(SignalKind::terminate())?;
            let mut int = signal(SignalKind::interrupt())?;
            tokio::spawn(async move {
                tokio::select! {
                    _ = term.recv() => {}
                    _ = int.recv() => {}
                }
                let _ = tx.send(true);
            });
        }
        #[cfg(not(unix))]
        {
            let mut c = tokio::signal::windows::ctrl_c()?;
            let mut b = tokio::signal::windows::ctrl_break()?;
            tokio::spawn(async move {
                tokio::select! {
                    _ = c.recv() => {}
                    _ = b.recv() => {}
                }
                let _ = tx.send(true);
            });
        }
        Ok(Stop(rx))
    }

    pub struct Ctx {
        browser: PathBuf,
        kind: &'static str,
        machine: String,
        port: u16,
        control: Control,
        seed: Seed,
        scratch: PathBuf,
        headless: bool,
        stop: Stop,
        added_flags: Vec<String>,
        routes: bool,
        relayed: bool,
        round2: bool,
        relay: Option<RelayHandle>,
        relay_port: u16,
        net_log: Option<PathBuf>,
        repo: PathBuf,
        channel: Channel,
        round3: bool,
        probe_before: std::cell::Cell<usize>,
        siblings: std::cell::RefCell<Vec<std::process::Child>>,
    }

    impl Ctx {
        fn flags(&self) -> Vec<String> {
            let mut f: Vec<String> = FIRST_FLAGS.iter().map(|s| s.to_string()).collect();
            f.push(RESOLVER_RULE.to_string());
            f.extend(self.added_flags.iter().cloned());
            f
        }

        fn options(&self, profile: &str) -> StartOptions {
            StartOptions {
                browser: self.browser.clone(),
                profile: self
                    .scratch
                    .join(format!("{}-{profile}", std::process::id())),
                headless: self.headless,
                flags: self.flags(),
                channel: self.channel,
                relay: self.relay.clone(),
                local_state: None,
                program: None,
            }
        }

        fn u(&self, path: &str) -> String {
            format!("{}{path}", origin(UTILITY, self.port))
        }

        fn form(&self) -> &'static str {
            match (self.channel, self.relayed) {
                (Channel::Pipe, true) => "pipe_relay",
                (Channel::Pipe, false) => "pipe",
                (Channel::Port, true) => "port_relay",
                (Channel::Port, false) => "port",
            }
        }
    }

    fn check(cond: bool, name: &'static str) -> Res<()> {
        if cond {
            Ok(())
        } else {
            Err(Fault::new("check", name))
        }
    }

    fn state(ctx: &Ctx) -> Res<serde_json::Value> {
        ctx.control.state().map_err(|e| Fault::new("control", e))
    }

    fn counter(ctx: &Ctx, path: &[&str]) -> Res<i64> {
        count(&state(ctx)?, path).ok_or_else(|| Fault::new("check", "state_key"))
    }

    fn role(r: &'static str, name: &str) -> Locator {
        Locator::Role(r, name.to_string())
    }

    async fn path_now(ex: &Executor) -> Res<String> {
        let main = ex.main_frame().await?;
        Ok(r3_web::cdp::split_url(&ex.frame_url(&main).await?).1)
    }

    async fn click_and_wait(ex: &mut Executor, reference: &str) -> Res<()> {
        let frame = ex.frame_of(reference).await?;
        let before = ex.loader(&frame).await?;
        ex.click(reference).await?;
        ex.wait_loaded_after(&frame, Some(&before)).await
    }

    async fn signin(ex: &mut Executor, ctx: &Ctx) -> Res<()> {
        ex.open(&ctx.u("/signin")).await?;
        submit_signin(ex, ctx).await
    }

    async fn submit_signin(ex: &mut Executor, ctx: &Ctx) -> Res<()> {
        let email = ex.resolve_one(&[role("textbox", "Email")], &[]).await?;
        ex.fill(&email, &ctx.seed.str(&["credentials", "user"]))
            .await?;
        let pw = ex.resolve_one(&[role("textbox", "Password")], &[]).await?;
        ex.fill(&pw, &ctx.seed.str(&["credentials", "password"]))
            .await?;
        let button = ex.resolve_one(&[role("button", "Sign in")], &[]).await?;
        click_and_wait(ex, &button).await
    }

    async fn signin_form(ex: &mut Executor, ctx: &Ctx) -> Res<()> {
        let before = counter(ctx, &["utility", "signins"])?;
        signin(ex, ctx).await?;
        let after = counter(ctx, &["utility", "signins"])?;
        check(after == before + 1, "signins")?;
        check(path_now(ex).await? == "/bill", "page")
    }

    async fn table_read(ex: &mut Executor, ctx: &Ctx) -> Res<()> {
        signin(ex, ctx).await?;
        check(path_now(ex).await? == "/bill", "page")?;
        let lines = ex.table("Bill history").await?;
        let cells: Vec<&SnapshotLine> = lines.iter().filter(|l| l.role == "cell").collect();
        let history = ctx.seed.0["utility"]["history"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        check(history.len() == 3 && cells.len() == 9, "cells")?;
        for (row, h) in cells.chunks(3).zip(history.iter()) {
            check(row[0].name == h["date"].as_str().unwrap_or("-"), "date")?;
            check(
                extract_amount(&row[1].name) == h["amount"].as_u64(),
                "amount",
            )?;
            check(row[2].name == h["status"].as_str().unwrap_or("-"), "status")?;
        }
        Ok(())
    }

    async fn watch_requests(ex: &mut Executor, ctx: &Ctx) -> Res<()> {
        ex.arm_rule(vec![origin(UTILITY, ctx.port), origin(PAY, ctx.port)], true)
            .await
    }

    fn sent(ex: &Executor, method: &str, path: &str) -> Res<PausedRequest> {
        let mut found: Vec<PausedRequest> = ex
            .paused()
            .into_iter()
            .filter(|p| p.method == method && p.path == path && p.outcome == Outcome::Continued)
            .collect();
        match found.len() {
            1 => Ok(found.remove(0)),
            0 => Err(Fault::new("check", "request_not_sent")),
            _ => Err(Fault::new("check", "request_sent_twice")),
        }
    }

    async fn form_fill(ex: &mut Executor, ctx: &Ctx) -> Res<()> {
        signin(ex, ctx).await?;
        ex.open(&ctx.u("/prefs")).await?;
        watch_requests(ex, ctx).await?;
        let before = counter(ctx, &["utility", "prefs_saved"])?;
        let boxed = ex
            .resolve_one(&[role("checkbox", "Paperless billing")], &[])
            .await?;
        ex.click(&boxed).await?;
        let save = ex
            .resolve_one(&[role("button", "Save preferences")], &[])
            .await?;
        click_and_wait(ex, &save).await?;
        let after = counter(ctx, &["utility", "prefs_saved"])?;
        check(after == before + 1, "prefs_saved")?;
        let post = sent(ex, "POST", "/prefs")?;
        check(post.body_holds(b"paperless=yes"), "box_set_in_request")
    }

    fn payments(ctx: &Ctx) -> Res<Vec<serde_json::Value>> {
        state(ctx)?["utility"]["payments"]
            .as_array()
            .cloned()
            .ok_or_else(|| Fault::new("check", "state_key"))
    }

    fn paid_once_more(ctx: &Ctx, before: usize, instrument: &str) -> Res<()> {
        let after = payments(ctx)?;
        check(after.len() == before + 1, "payments")?;
        let last = after.last().cloned().unwrap_or_default();
        check(
            last["amount"] == ctx.seed.0["utility"]["amount_due"],
            "amount",
        )?;
        check(last["instrument"] == instrument, "instrument")
    }

    fn instrument(ctx: &Ctx, index: usize) -> String {
        ctx.seed.0["utility"]["instruments"][index]
            .as_str()
            .unwrap_or("-")
            .to_string()
    }

    fn method_line(lines: &[SnapshotLine]) -> Res<usize> {
        lines
            .iter()
            .position(|l| l.role == "combobox" && l.name == "Payment method")
            .ok_or_else(|| Fault::new("resolve", "combobox"))
    }

    async fn payment_frame(ex: &mut Executor, ctx: &Ctx) -> Res<()> {
        signin(ex, ctx).await?;
        ex.open(&ctx.u("/pay")).await?;
        let before = payments(ctx)?.len();
        let lines = ex.snapshot().await?;
        let combo = method_line(&lines)?;
        let second = lines[combo + 1..]
            .iter()
            .take_while(|l| l.role == "option")
            .nth(1)
            .map(|l| l.name.clone())
            .ok_or_else(|| Fault::new("resolve", "option"))?;
        check(
            lines[combo].value.as_deref() != Some(second.as_str()),
            "second_not_chosen_yet",
        )?;
        let method = ex
            .resolve_one(&[role("combobox", "Payment method")], &["Payment"])
            .await?;
        ex.select(&method, &second).await?;
        let after = ex.snapshot().await?;
        check(
            after[method_line(&after)?].value.as_deref() == Some(second.as_str()),
            "selected",
        )?;
        let pay = ex
            .resolve_one(&[role("button", "Pay now")], &["Payment"])
            .await?;
        click_and_wait(ex, &pay).await?;
        let heading = ex
            .resolve(&[role("heading", "Payment received")], &["Payment"])
            .await?;
        check(matches!(heading, Resolved::One { .. }), "heading")?;
        paid_once_more(ctx, before, &second)?;
        check(second == instrument(ctx, 1), "second_option")
    }

    async fn file_upload(ex: &mut Executor, ctx: &Ctx, i: u32) -> Res<()> {
        signin(ex, ctx).await?;
        ex.open(&ctx.u("/upload")).await?;
        let file = ctx.scratch.join(format!("meter-{i}.bin"));
        std::fs::write(&file, vec![0x5a_u8; 1024])
            .map_err(|e| Fault::new("file", e.to_string()))?;
        let before = counter(ctx, &["utility", "uploads"])?;
        let input = ex
            .resolve_one(
                &[
                    Locator::TestId("meter-photo".into()),
                    role("button", "Meter photo"),
                ],
                &[],
            )
            .await?;
        ex.upload(&input, &file).await?;
        check(ex.file_sizes(&input).await? == vec![1024], "file_in_input")?;
        watch_requests(ex, ctx).await?;
        let send = ex.resolve_one(&[role("button", "Upload")], &[]).await?;
        click_and_wait(ex, &send).await?;
        let after = counter(ctx, &["utility", "uploads"])?;
        check(after == before + 1, "uploads")?;
        sent(ex, "POST", "/upload").map(|_| ())
    }

    async fn session_expiry(ex: &mut Executor, ctx: &Ctx) -> Res<()> {
        signin(ex, ctx).await?;
        let signins = counter(ctx, &["utility", "signins"])?;
        check(signins == 1, "first_signin")?;
        ex.open(&ctx.u("/prefs")).await?;
        let save = ex
            .resolve_one(&[role("button", "Save preferences")], &[])
            .await?;
        click_and_wait(ex, &save).await?;
        check(
            counter(ctx, &["utility", "prefs_saved"])? == 1,
            "prefs_saved",
        )?;
        ex.open(&ctx.u("/bill")).await?;
        let lines = ex.snapshot().await?;
        check(lines.iter().any(|l| l.credential), "credential_field")?;
        check(counter(ctx, &["utility", "signins"])? == 1, "signins")
    }

    async fn changed_page_1(ex: &mut Executor, ctx: &Ctx) -> Res<()> {
        signin(ex, ctx).await?;
        ex.open(&ctx.u("/pay")).await?;
        let before = payments(ctx)?.len();
        let found = ex
            .resolve(
                &[Locator::TestId("pay-now".into()), role("button", "Pay now")],
                &["Payment"],
            )
            .await?;
        let Resolved::One { reference, locator } = found else {
            return Err(Fault::new("resolve", "none"));
        };
        check(locator == 1, "second_locator")?;
        click_and_wait(ex, &reference).await?;
        paid_once_more(ctx, before, &instrument(ctx, 0))
    }

    async fn block_request(ex: &mut Executor, ctx: &Ctx) -> Res<()> {
        signin(ex, ctx).await?;
        ex.open(&ctx.u("/prefs")).await?;
        let u = origin(UTILITY, ctx.port);
        ex.arm_rule(vec![u.clone(), origin(PAY, ctx.port)], false)
            .await?;
        let before = counter(ctx, &["requests", "state_changing"])?;
        let save = ex
            .resolve_one(&[role("button", "Save preferences")], &[])
            .await?;
        let main = ex.main_frame().await?;
        ex.click(&save).await?;
        let deadline = Instant::now() + r3_web::cdp::STEP_TIMEOUT;
        let failed = loop {
            if let Some(p) = ex.paused().into_iter().find(|p| !p.let_through) {
                break p;
            }
            if Instant::now() > deadline {
                return Err(Fault::new("timeout", "paused"));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        };
        check(failed.method == "POST", "method")?;
        check(failed.origin == u, "origin")?;
        check(failed.path == "/prefs", "path")?;
        check(failed.outcome == Outcome::Failed, "failed_by_browser")?;
        loop {
            if ex.frame_url(&main).await?.starts_with("chrome-error://") {
                break;
            }
            if Instant::now() > deadline {
                return Err(Fault::new("timeout", "error_page"));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let after = counter(ctx, &["requests", "state_changing"])?;
        check(after == before, "state_changing")
    }

    fn mark(lines: &[SnapshotLine], name: &str) -> Option<bool> {
        lines
            .iter()
            .find(|l| l.role == "textbox" && l.name == name)
            .map(|l| l.credential)
    }

    async fn credential_mark(ex: &mut Executor, ctx: &Ctx) -> Res<()> {
        ex.open(&ctx.u("/signin")).await?;
        let signin_page = ex.snapshot().await?;
        check(mark(&signin_page, "Email") == Some(false), "email")?;
        check(mark(&signin_page, "Password") == Some(true), "password")?;
        submit_signin(ex, ctx).await?;
        check(path_now(ex).await? == "/challenge", "challenge_page")?;
        let challenge = ex.snapshot().await?;
        check(mark(&challenge, "Characters") == Some(true), "characters")?;
        let field = ex
            .resolve_one(&[role("textbox", "Characters")], &[])
            .await?;
        ex.fill(&field, &ctx.seed.str(&["credentials", "captcha"]))
            .await?;
        let go = ex.resolve_one(&[role("button", "Continue")], &[]).await?;
        click_and_wait(ex, &go).await?;
        check(path_now(ex).await? == "/verify", "verify_page")?;
        let verify = ex.snapshot().await?;
        check(
            mark(&verify, "Verification code") == Some(true),
            "verification_code",
        )
    }

    async fn pay_frame_target(ex: &Executor, ctx: &Ctx) -> Res<String> {
        let frame = format!("{}/", origin(PAY, ctx.port));
        let mut targets: Vec<String> = ex
            .frame_targets()
            .await?
            .into_iter()
            .filter(|(_, url)| url.starts_with(&frame))
            .map(|(id, _)| id)
            .collect();
        match targets.len() {
            1 => Ok(targets.remove(0)),
            _ => Err(Fault::new("check", "frame_not_own_target")),
        }
    }

    async fn frame_isolated(ex: &Executor, ctx: &Ctx) -> Res<()> {
        pay_frame_target(ex, ctx).await.map(|_| ())
    }

    async fn payment_frame_isolated(ex: &mut Executor, ctx: &Ctx) -> Res<()> {
        payment_frame(ex, ctx).await?;
        frame_isolated(ex, ctx).await
    }

    async fn changed_page_1_isolated(ex: &mut Executor, ctx: &Ctx) -> Res<()> {
        changed_page_1(ex, ctx).await?;
        frame_isolated(ex, ctx).await
    }

    type Key = (&'static str, &'static [&'static str]);

    const STATE_CHANGING: Key = ("state_changing", &["requests", "state_changing"]);
    const PREFS_SAVED: Key = ("prefs_saved", &["utility", "prefs_saved"]);
    const PAYMENTS: Key = ("payments", &["utility", "payments"]);
    const EVIL: Key = ("evil", &["requests", "evil"]);

    fn read_counters(ctx: &Ctx, keys: &[Key]) -> Res<Vec<i64>> {
        let s = state(ctx)?;
        keys.iter()
            .map(|(name, path)| {
                count(&s, path).ok_or_else(|| Fault::new("check", format!("state_key_{name}")))
            })
            .collect()
    }

    async fn arm_isolation_rule(ex: &mut Executor, ctx: &Ctx) -> Res<()> {
        ex.arm_rule(
            vec![origin(UTILITY, ctx.port), origin(PAY, ctx.port)],
            false,
        )
        .await
    }

    async fn paused_after(
        ex: &Executor,
        t0: Instant,
        want: impl Fn(&PausedRequest) -> bool,
    ) -> Option<(PausedRequest, u128)> {
        let deadline = t0 + r3_web::cdp::STEP_TIMEOUT;
        loop {
            if let Some(p) = ex.paused().into_iter().find(|p| want(p)) {
                return Some((p, t0.elapsed().as_millis()));
            }
            if Instant::now() > deadline {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    async fn held(
        ctx: &Ctx,
        keys: &[Key],
        before: &[i64],
        seen: Option<(PausedRequest, u128)>,
    ) -> Res<Option<u128>> {
        tokio::time::sleep(Duration::from_millis(2_000)).await;
        let second = read_counters(ctx, keys)?;
        let third = read_counters(ctx, keys)?;
        let mut misses: Vec<&str> = Vec::new();
        match &seen {
            None => misses.push("paused_none"),
            Some((p, _)) => {
                if p.let_through {
                    misses.push("let_through");
                }
                if p.outcome != Outcome::Failed {
                    misses.push("not_failed");
                }
            }
        }
        for (i, (name, _)) in keys.iter().enumerate() {
            if second[i] != before[i] || third[i] != before[i] {
                misses.push(name);
            }
        }
        if misses.is_empty() {
            Ok(seen.map(|(_, ms)| ms))
        } else {
            Err(Fault::new("check", misses.join(".")))
        }
    }

    const POPUP_SCRIPT: &str = r#"(() => { const f = document.querySelector('form[action="/prefs"]'); if (!f) return false; f.target = '_blank'; return true; })()"#;

    async fn block_popup(ex: &mut Executor, ctx: &Ctx) -> Res<Option<u128>> {
        signin(ex, ctx).await?;
        arm_isolation_rule(ex, ctx).await?;
        ex.open(&ctx.u("/prefs")).await?;
        let changed = ex.run_page_script(POPUP_SCRIPT).await?;
        check(changed == serde_json::Value::Bool(true), "script")?;
        let keys = [STATE_CHANGING, PREFS_SAVED];
        let before = read_counters(ctx, &keys)?;
        let boxed = ex
            .resolve_one(&[role("checkbox", "Paperless billing")], &[])
            .await?;
        ex.click(&boxed).await?;
        let save = ex
            .resolve_one(&[role("button", "Save preferences")], &[])
            .await?;
        let u = origin(UTILITY, ctx.port);
        let main = ex.main_frame().await?;
        let t0 = Instant::now();
        ex.click(&save).await?;
        let seen = paused_after(ex, t0, |p| {
            p.method == "POST" && p.origin == u && p.path == "/prefs" && p.frame != *main.inner()
        })
        .await;
        held(ctx, &keys, &before, seen).await
    }

    async fn block_frame_post(ex: &mut Executor, ctx: &Ctx) -> Res<Option<u128>> {
        signin(ex, ctx).await?;
        arm_isolation_rule(ex, ctx).await?;
        ex.open(&ctx.u("/pay")).await?;
        let pay_frame = pay_frame_target(ex, ctx).await?;
        let keys = [STATE_CHANGING, PAYMENTS];
        let before = read_counters(ctx, &keys)?;
        let pay = ex
            .resolve_one(&[role("button", "Pay now")], &["Payment"])
            .await?;
        let p_origin = origin(PAY, ctx.port);
        let t0 = Instant::now();
        ex.click(&pay).await?;
        let seen = paused_after(ex, t0, |p| {
            p.method == "POST"
                && p.origin == p_origin
                && p.path == "/charge"
                && p.frame == pay_frame
        })
        .await;
        held(ctx, &keys, &before, seen).await
    }

    fn socket_script(port: u16) -> String {
        format!(
            "(() => {{ globalThis.halvoTrialSocket = new WebSocket(\"ws://evil.agent.test:{port}/\"); return true; }})()"
        )
    }

    async fn block_websocket(ex: &mut Executor, ctx: &Ctx) -> Res<Option<u128>> {
        if ctx.round2 {
            return block_websocket_at_relay(ex, ctx).await;
        }
        signin(ex, ctx).await?;
        arm_isolation_rule(ex, ctx).await?;
        ex.open(&ctx.u("/bill")).await?;
        let keys = [EVIL];
        let before = read_counters(ctx, &keys)?;
        let t0 = Instant::now();
        let ran = ex.run_page_script(&socket_script(ctx.port)).await?;
        check(ran == serde_json::Value::Bool(true), "script")?;
        let seen = paused_after(ex, t0, |p| {
            host_of(&p.origin) == "evil.agent.test" && p.path == "/"
        })
        .await;
        held(ctx, &keys, &before, seen).await
    }

    async fn cert_error(ex: &mut Executor, ctx: &Ctx, i: u32) -> Res<Option<u128>> {
        let dir = ctx.scratch.join(format!("{}-cert-{i}", std::process::id()));
        let (cert, key) = make_certificate(&dir).map_err(|e| match e {
            CertFault::NoOpenssl => Fault::new("openssl", "no openssl"),
            CertFault::Failed(d) => Fault::new("certificate", d),
        })?;
        let program = ctx.repo.join("tools/r3-trials/web/tls-listener.mjs");
        let mut listener =
            Listener::start_tls(&program, &cert, &key).map_err(|e| Fault::new("listener", e))?;
        if let Some(r) = &ctx.relay {
            r.allow(UTILITY, listener.port);
        }
        let address = format!("https://{UTILITY}:{}/", listener.port);
        let opened = ex.navigate_error(&address).await;
        let counts = listener.counts().map_err(|e| Fault::new("listener", e));
        listener.stop();
        let error = opened?;
        let counts = counts?;
        let mut misses: Vec<&str> = Vec::new();
        if !error.as_deref().unwrap_or("").starts_with("net::ERR_CERT_") {
            misses.push("no_certificate_error");
        }
        if counts.requests != 0 {
            misses.push("request_counted");
        }
        if misses.is_empty() {
            Ok(None)
        } else {
            Err(Fault::new("check", misses.join(".")))
        }
    }

    fn fixture_list(ctx: &Ctx) -> Vec<HostPort> {
        vec![(UTILITY.to_string(), ctx.port), (PAY.to_string(), ctx.port)]
    }

    fn relay_of(ctx: &Ctx) -> Res<RelayHandle> {
        ctx.relay
            .clone()
            .ok_or_else(|| Fault::new("relay", "no relay"))
    }

    async fn refused_after(relay: &RelayHandle, host: &str, port: u16, before: usize) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if relay.refusals_of(host, port) > before {
                return true;
            }
            if Instant::now() > deadline {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    async fn alive(ex: &Executor) -> bool {
        matches!(
            ex.run_page_script("document.readyState").await,
            Ok(serde_json::Value::String(_))
        )
    }

    fn outcome(misses: Vec<String>, ms: Option<u128>) -> Res<Option<u128>> {
        if misses.is_empty() {
            Ok(ms)
        } else {
            Err(Fault::new("check", misses.join(".")))
        }
    }

    async fn block_websocket_at_relay(ex: &mut Executor, ctx: &Ctx) -> Res<Option<u128>> {
        let relay = relay_of(ctx)?;
        signin(ex, ctx).await?;
        arm_isolation_rule(ex, ctx).await?;
        ex.open(&ctx.u("/bill")).await?;
        let keys = [EVIL];
        let before = read_counters(ctx, &keys)?;
        let refused = relay.refusals_of("evil.agent.test", ctx.port);
        let t0 = Instant::now();
        let ran = ex.run_page_script(&socket_script(ctx.port)).await?;
        check(ran == serde_json::Value::Bool(true), "script")?;
        let mut misses = Vec::new();
        let seen = refused_after(&relay, "evil.agent.test", ctx.port, refused).await;
        let ms = t0.elapsed().as_millis();
        if !seen {
            misses.push("refusal_none".to_string());
        }
        tokio::time::sleep(Duration::from_millis(2_000)).await;
        let second = read_counters(ctx, &keys)?;
        let third = read_counters(ctx, &keys)?;
        if second != before || third != before {
            misses.push("evil".to_string());
        }
        if !alive(ex).await {
            misses.push("ended".to_string());
        }
        outcome(misses, Some(ms))
    }

    async fn list_refuse(ex: &mut Executor, ctx: &Ctx) -> Res<Option<u128>> {
        let relay = relay_of(ctx)?;
        ex.open(&ctx.u("/bill")).await?;
        let keys = [EVIL];
        let before = read_counters(ctx, &keys)?;
        let e = format!("evil.agent.test:{}", ctx.port);
        let kinds = [
            ("document", "(() => { const f = document.createElement('iframe'); f.src = 'http://EVIL_HOST/doc'; document.body.appendChild(f); return true; })()"),
            ("fetch", "(() => { fetch('http://EVIL_HOST/fetch', { mode: 'no-cors' }).catch(() => {}); return true; })()"),
            ("image", "(() => { new Image().src = 'http://EVIL_HOST/img'; return true; })()"),
            ("form", "(() => { const i = document.createElement('iframe'); i.name = 'halvoSink'; document.body.appendChild(i); const f = document.createElement('form'); f.method = 'post'; f.action = 'http://EVIL_HOST/form'; f.target = 'halvoSink'; document.body.appendChild(f); f.submit(); return true; })()"),
            ("ws", "(() => { new WebSocket('ws://EVIL_HOST/'); return true; })()"),
            ("wss", "(() => { new WebSocket('wss://EVIL_HOST/'); return true; })()"),
        ];
        let mut misses = Vec::new();
        for (kind, script) in kinds {
            let n0 = relay.refusals_of("evil.agent.test", ctx.port);
            let ran = ex.run_page_script(&script.replace("EVIL_HOST", &e)).await;
            if ran.is_err() || !refused_after(&relay, "evil.agent.test", ctx.port, n0).await {
                misses.push(format!("refusal_{kind}"));
            }
        }
        tokio::time::sleep(Duration::from_millis(2_000)).await;
        if read_counters(ctx, &keys)? != before {
            misses.push("evil".to_string());
        }
        if !alive(ex).await {
            misses.push("ended".to_string());
        }
        outcome(misses, None)
    }

    fn chain_listener(ctx: &Ctx) -> Res<Listener> {
        Listener::start_chain(&ctx.repo.join("tools/r3-trials/web/chain-listener.mjs"))
            .map_err(|e| Fault::new("listener", e))
    }

    async fn list_hops(ex: &mut Executor, ctx: &Ctx) -> Res<Option<u128>> {
        let relay = relay_of(ctx)?;
        let mut listener = chain_listener(ctx)?;
        let l = listener.port;
        let hop = |n: u8| format!("hop{n}.agent.test");
        relay.set_list(Some(vec![(hop(1), l)]));
        let adding = relay.clone();
        ex.on_main_document(Some(std::sync::Arc::new(move |h: &str, p: u16| {
            adding.allow(h, p)
        })));
        ex.arm_rule(
            (1..=3).map(|n| format!("http://{}:{l}", hop(n))).collect(),
            false,
        )
        .await?;
        let mut misses = Vec::new();
        let opened = ex.open(&format!("http://{}:{l}/start", hop(1))).await;
        if opened.is_err() {
            misses.push("chain".to_string());
        }
        ex.on_main_document(None);
        let here = ex
            .frame_url(&ex.main_frame().await?)
            .await
            .unwrap_or_default();
        if !here.starts_with(&format!("http://{}:{l}/end", hop(3))) {
            misses.push("not_on_hop3".to_string());
        }
        relay.set_list(Some(vec![(hop(3), l)]));
        let n0 = relay.refusals_of(&hop(1), l);
        let before = listener
            .host_counts()
            .map_err(|e| Fault::new("listener", e))?;
        ex.run_page_script(&format!(
            "(() => {{ fetch('http://{}:{l}/f', {{ mode: 'no-cors' }}).catch(() => {{}}); return true; }})()",
            hop(1)
        ))
        .await?;
        if !refused_after(&relay, &hop(1), l, n0).await {
            misses.push("earlier_hop_not_refused".to_string());
        }
        tokio::time::sleep(Duration::from_millis(1_000)).await;
        let after = listener
            .host_counts()
            .map_err(|e| Fault::new("listener", e))?;
        listener.stop();
        for n in 1..=2 {
            if before.get(&hop(n)).copied().unwrap_or(0) != 1 {
                misses.push(format!("hop{n}_count"));
            }
        }
        if before.get(&hop(3)).copied().unwrap_or(0) == 0 {
            misses.push("hop3_count".to_string());
        }
        if after.get(&hop(1)) != before.get(&hop(1)) {
            misses.push("hop1_reached_after".to_string());
        }
        if !alive(ex).await {
            misses.push("ended".to_string());
        }
        outcome(misses, None)
    }

    async fn list_close(ex: &mut Executor, ctx: &Ctx) -> Res<Option<u128>> {
        let relay = relay_of(ctx)?;
        let listener = chain_listener(ctx)?;
        let l = listener.port;
        let moments = ["chain", "handback", "new-origin", "finish"];
        let host = |m: &str| format!("{m}.close.agent.test");
        let mut list = fixture_list(ctx);
        list.extend(moments.iter().map(|m| (host(m), l)));
        relay.set_list(Some(list.clone()));
        ex.open(&ctx.u("/bill")).await?;
        let mut misses = Vec::new();
        for m in moments {
            let h = host(m);
            let fetch = format!(
                "(() => {{ fetch('http://{h}:{l}/f', {{ mode: 'no-cors' }}).catch(() => {{}}); return true; }})()"
            );
            ex.run_page_script(&fetch).await?;
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut open = false;
            while Instant::now() < deadline {
                if relay
                    .open_tunnels()
                    .iter()
                    .any(|(oh, op)| *oh == h && *op == l)
                {
                    open = true;
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            if !open {
                misses.push(format!("open_{m}"));
                continue;
            }
            if m == "finish" {
                relay.set_list(Some(Vec::new()));
            } else {
                list.retain(|(lh, _)| *lh != h);
                relay.set_list(Some(list.clone()));
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
            let closed = relay.closed().iter().any(|(ch, cp)| *ch == h && *cp == l)
                && !relay
                    .open_tunnels()
                    .iter()
                    .any(|(oh, op)| *oh == h && *op == l);
            if !closed {
                misses.push(format!("closed_{m}"));
            }
            let n0 = relay.refusals_of(&h, l);
            ex.run_page_script(&fetch).await?;
            if !refused_after(&relay, &h, l, n0).await {
                misses.push(format!("refused_{m}"));
            }
        }
        listener.stop();
        outcome(misses, None)
    }

    async fn own_traffic(ex: &mut Executor, ctx: &Ctx) -> Res<Option<u128>> {
        let relay = relay_of(ctx)?;
        tokio::time::sleep(Duration::from_secs(10)).await;
        let refusals = relay.refusals();
        let mut distinct: Vec<&HostPort> = refusals.iter().collect();
        distinct.sort();
        distinct.dedup();
        eprintln!(
            "own_traffic machine={} browser={} refused={} distinct={} tunnels={}",
            ctx.machine,
            ctx.kind,
            refusals.len(),
            distinct.len(),
            relay.tunnels()
        );
        let mut misses = Vec::new();
        if !alive(ex).await {
            misses.push("ended".to_string());
        }
        outcome(misses, None)
    }

    struct Egress {
        dir: PathBuf,
        net_log: PathBuf,
        before: u64,
    }

    fn refused_udp_connects(path: &Path) -> Option<Vec<String>> {
        let text = std::fs::read_to_string(path).ok()?;
        let d: serde_json::Value = serde_json::Deserializer::from_str(&text)
            .into_iter()
            .next()?
            .ok()?;
        let connect = d["constants"]["logEventTypes"]["UDP_CONNECT"].as_u64()?;
        let mut addr = std::collections::HashMap::new();
        let mut out = Vec::new();
        for e in d["events"].as_array()? {
            if e["type"].as_u64() != Some(connect) {
                continue;
            }
            let id = e["source"]["id"].as_u64().unwrap_or(0);
            if let Some(a) = e["params"]["address"].as_str() {
                addr.insert(id, a.to_string());
            }
            if e["params"]["net_error"].as_i64() == Some(-10) {
                out.push(addr.get(&id).cloned().unwrap_or_else(|| "-".into()));
            }
        }
        Some(out)
    }

    pub fn org_control(argv: &[String]) -> ExitCode {
        let (Some(browser), Some(dir)) = (argv.first(), argv.get(1)) else {
            eprintln!("REFUSED usage: --org-control BROWSER DIR");
            return ExitCode::from(2);
        };
        let Ok(rt) = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        else {
            out("org_control reply=no cause=runtime");
            return ExitCode::from(1);
        };
        let opts = StartOptions {
            browser: PathBuf::from(browser),
            profile: Path::new(dir).join(format!("{}-org-control", std::process::id())),
            headless: true,
            flags: FIRST_FLAGS.iter().map(|f| f.to_string()).collect(),
            channel: Channel::Pipe,
            relay: None,
            local_state: None,
            program: None,
        };
        rt.block_on(async {
            match Executor::start(&opts).await {
                Ok(ex) => {
                    let stopped = ex.stop_and_remove().await;
                    out(&format!(
                        "org_control reply=yes left_before={} left_after={}",
                        stopped.ended.before, stopped.ended.after
                    ));
                    ExitCode::SUCCESS
                }
                Err(f) => {
                    out(&format!("org_control reply=no cause={}", f.0));
                    ExitCode::from(1)
                }
            }
        })
    }

    #[cfg(all(test, target_os = "macos"))]
    mod leftover_tests {
        #[test]
        fn a_process_that_names_a_longer_profile_is_left_alone() {
            use r3_web::start::Start;
            let dir = std::env::temp_dir().join(format!("r3-web-p10-{}", std::process::id()));
            let mut other = std::process::Command::new("/usr/bin/perl")
                .args(["-e", "sleep 30", "--"])
                .arg(format!("--user-data-dir={}", dir.join("p-10").display()))
                .start()
                .unwrap();
            let mut own = std::process::Command::new("/usr/bin/perl")
                .args(["-e", "sleep 30", "--"])
                .arg(format!("--user-data-dir={}", dir.join("p-1").display()))
                .start()
                .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(300));
            let since = r3_web::procs::table()
                .unwrap()
                .into_iter()
                .find(|p| p.pid == std::process::id())
                .map(|p| p.started_us)
                .unwrap();
            let mark = r3_web::checks::Mark::new(
                &dir.join("p-1"),
                std::path::Path::new("/usr/bin/perl"),
                since,
            );
            let ended = r3_web::checks::end_runs_own(&mark);
            let alive = matches!(other.try_wait(), Ok(None));
            let own_alive = matches!(own.try_wait(), Ok(None));
            for c in [&mut other, &mut own] {
                let _ = c.kill();
                let _ = c.wait();
            }
            assert!(
                alive,
                "a process that names p-10 was ended for the run of p-1"
            );
            assert!(
                !own_alive,
                "the control: the process that names p-1 was left ({ended:?})"
            );
        }

        #[test]
        fn an_org_effect_counts_only_with_its_control_value_and_page() {
            use super::OrgFacts;
            let f = |c: Option<&str>, r: Option<&str>, w: Option<&str>, p: Option<&str>| OrgFacts {
                control_before: c.map(Into::into),
                value_read: r.map(Into::into),
                value_want: w.map(Into::into),
                page_named: p.map(Into::into),
                page_cause: None,
            };
            assert_eq!(f(None, None, None, None).lacks(), Some("no_control_reply"));
            assert_eq!(
                f(Some("reply"), None, Some("0"), Some("yes")).lacks(),
                Some("no_value_read")
            );
            assert_eq!(
                f(Some("reply"), Some("1"), Some("0"), Some("yes")).lacks(),
                Some("no_value_read")
            );
            assert_eq!(
                f(Some("reply"), Some("0"), Some("0"), Some("no")).lacks(),
                Some("page_not_named")
            );
            assert_eq!(
                f(Some("reply"), Some("0"), Some("0"), Some("yes")).lacks(),
                None
            );
            assert_eq!(
                f(
                    Some("reply"),
                    Some("direct"),
                    Some("direct"),
                    Some("unread")
                )
                .lacks(),
                None
            );
        }

        #[test]
        fn of_the_run_counts_a_member_born_after_the_kill() {
            use r3_web::start::Start;
            let dir = std::env::temp_dir().join(format!("r3-web-born-{}", std::process::id()));
            let killed_at = r3_web::procs::table()
                .unwrap()
                .into_iter()
                .find(|p| p.pid == std::process::id())
                .map(|p| p.started_us)
                .unwrap();
            let mark = r3_web::checks::Mark::new(
                &dir.join("p-1"),
                std::path::Path::new("/usr/bin/perl"),
                killed_at,
            );
            let mut born = std::process::Command::new("/usr/bin/perl")
                .args(["-e", "sleep 30", "--"])
                .arg(format!("--user-data-dir={}", dir.join("p-1").display()))
                .start()
                .unwrap();
            let mut other = std::process::Command::new("/usr/bin/perl")
                .args(["-e", "sleep 30", "--"])
                .arg(format!("--user-data-dir={}", dir.join("p-2").display()))
                .start()
                .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(300));
            let found = super::of_the_run(&[], &mark).unwrap_or_default();
            let pids: Vec<u32> = found.iter().map(|p| p.pid).collect();
            for c in [&mut born, &mut other] {
                let _ = c.kill();
                let _ = c.wait();
            }
            assert!(
                pids.contains(&born.id()),
                "a process of the run born after the kill was not counted: {pids:?}"
            );
            assert!(
                !pids.contains(&other.id()),
                "a process of another profile was counted: {pids:?}"
            );
        }
    }

    #[cfg(test)]
    mod org_tests {
        use super::*;

        fn ctx() -> (Ctx, tokio::sync::watch::Sender<bool>) {
            let (tx, rx) = tokio::sync::watch::channel(false);
            let ctx = Ctx {
                browser: PathBuf::from("browser"),
                kind: "chrome",
                machine: "mac".into(),
                port: 9,
                control: Control { port: 9 },
                seed: Seed(serde_json::Value::Null),
                scratch: std::env::temp_dir(),
                headless: true,
                stop: Stop(rx),
                added_flags: Vec::new(),
                routes: false,
                relayed: true,
                round2: true,
                relay: None,
                relay_port: 0,
                net_log: None,
                repo: PathBuf::from("."),
                channel: Channel::Pipe,
                round3: true,
                probe_before: std::cell::Cell::new(0),
                siblings: std::cell::RefCell::new(Vec::new()),
            };
            (ctx, tx)
        }

        #[test]
        fn a_page_read_without_the_setting_counts_neither_as_the_effect_nor_as_a_miss() {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let (ctx, _tx) = ctx();
            let fault = || Err(Fault::new("no_channel", NO_CHANNEL_MESSAGE));
            std::env::set_var("R3_ORG_PAGE_NAMED", "no");
            let not_seen = rt.block_on(org_outcome(&ctx, "org_debugging", fault()));
            std::env::set_var("R3_ORG_PAGE_NAMED", "yes");
            let seen = rt.block_on(org_outcome(&ctx, "org_debugging", fault()));
            std::env::remove_var("R3_ORG_PAGE_NAMED");
            match not_seen {
                Err(n) => assert_eq!(n.cause, "policy_not_seen reason=page_not_named"),
                Ok(r) => panic!("a page without the setting gave a run, ok={}", r.ok),
            }
            match seen {
                Ok(r) => assert!(!r.ok, "the control without its facts counted as the effect"),
                Err(n) => panic!("the control with the page named gave no run: {}", n.cause),
            }
        }
    }

    fn check_processes() -> String {
        let names: Vec<String> = processes()
            .into_iter()
            .filter(|p| is_check_command(&p.3))
            .map(|p| {
                p.2.rsplit(['/', '\\'])
                    .next()
                    .unwrap_or("-")
                    .replace(' ', "_")
            })
            .collect();
        if names.is_empty() {
            "-".into()
        } else {
            names.join(",")
        }
    }

    fn is_check_command(command: &str) -> bool {
        command.contains("scripts/egress/check.sh")
            || command.contains("egress-spawn")
            || command.contains("log stream --style ndjson")
    }

    fn port_of(text: &str) -> &str {
        text.rsplit(':').next().unwrap_or("-")
    }

    enum EgressSeen {
        Clean,
        Denied(usize),
        Unread(String),
    }

    fn egress_runs(repo: &Path) -> u64 {
        std::fs::read_dir(repo.join("target/egress"))
            .map(|d| {
                d.filter_map(|e| e.ok()?.file_name().to_str()?.parse::<u64>().ok())
                    .max()
                    .unwrap_or(0)
            })
            .unwrap_or(0)
    }

    fn sh_path(p: &Path) -> String {
        let s = p.to_string_lossy().replace('\\', "/");
        let b = s.as_bytes();
        if cfg!(windows) && b.len() > 2 && b[1] == b':' && b[2] == b'/' {
            format!("/{}{}", (b[0] as char).to_ascii_lowercase(), &s[2..])
        } else {
            s
        }
    }

    fn egress_setup(
        ctx: &Ctx,
        opts: &mut StartOptions,
        name: &str,
        i: u32,
    ) -> Result<Egress, String> {
        if ctx.relay_port == 0 {
            return Err("no relay".into());
        }
        let dir = ctx
            .scratch
            .join(format!("{}-egress-{name}-{i}", std::process::id()));
        std::fs::create_dir_all(&dir).map_err(|e| format!("dir {}", e.kind()))?;
        let list = dir.join("list.txt");
        std::fs::write(
            &list,
            format!("[fixed]\n127.0.0.1:{}\n[agent]\n", ctx.relay_port),
        )
        .map_err(|e| format!("list {}", e.kind()))?;
        let spec = serde_json::to_vec(&[
            ctx.repo.to_string_lossy().into_owned(),
            sh_path(&list),
            sh_path(&ctx.browser),
            dir.join("check.err").to_string_lossy().into_owned(),
        ])
        .map_err(|_| "spec".to_string())?;
        let hex: String = spec.iter().map(|b| format!("{b:02x}")).collect();
        opts.flags.push(format!("{EGRESS_WRAP_FLAG}{hex}"));
        opts.program = Some(ctx.browser.clone());
        if cfg!(target_os = "macos") {
            opts.flags.push(NO_SANDBOX_INSIDE_THE_EGRESS_CHECK.into());
        }
        let net_log = match &ctx.net_log {
            Some(d) => d.join(format!("{name}-{i}.json")),
            None => dir.join("net.json"),
        };
        opts.flags
            .push(format!("--log-net-log={}", net_log.display()));
        opts.browser = std::env::current_exe().map_err(|e| format!("exe {}", e.kind()))?;
        Ok(Egress {
            dir,
            net_log,
            before: egress_runs(&ctx.repo),
        })
    }

    async fn egress_read(ctx: &Ctx, e: &Egress, name: &str, i: u32) -> EgressSeen {
        let mut found = None;
        let t0 = Instant::now();
        for _ in 0..360 {
            let last = egress_runs(&ctx.repo);
            for n in e.before + 1..=last {
                let d = ctx.repo.join(format!("target/egress/{n}"));
                let state = std::fs::read_to_string(d.join("state")).unwrap_or_default();
                if state.lines().any(|l| l == "done") {
                    found = Some((n, d));
                    break;
                }
            }
            if found.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        let line = std::fs::read_to_string(e.dir.join("check.err"))
            .unwrap_or_default()
            .lines()
            .find(|l| l.starts_with("egress mode="))
            .map(|l| l.to_string());
        let waited = t0.elapsed().as_secs();
        let Some((n, d)) = found else {
            let err = std::fs::read_to_string(e.dir.join("check.err")).unwrap_or_default();
            let own: Vec<&str> = err
                .lines()
                .filter(|l| l.starts_with("REFUSED") || l.starts_with("egress "))
                .collect();
            let last = egress_runs(&ctx.repo);
            let state = if last > e.before {
                std::fs::read_to_string(ctx.repo.join(format!("target/egress/{last}/state")))
                    .unwrap_or_default()
                    .lines()
                    .filter_map(|l| l.split('=').next())
                    .collect::<Vec<_>>()
                    .join(",")
            } else {
                "-".into()
            };
            eprintln!(
                "egress step={name} i={i} run=- waited_s={waited} newest_dir={} state_keys={} check_lines={} check_alive={}",
                if last > e.before { last.to_string() } else { "-".into() },
                if state.is_empty() { "-" } else { &state },
                if own.is_empty() { "-".to_string() } else { own.join(" | ").replace(' ', "_") },
                check_processes()
            );
            return EgressSeen::Unread("no_done_run".into());
        };
        let denied = match std::fs::read_to_string(d.join("denied.log")) {
            Ok(t) => t,
            Err(_) => return EgressSeen::Unread("no_denied_log".into()),
        };
        let lines: Vec<&str> = denied.lines().filter(|l| !l.trim().is_empty()).collect();
        let refused = refused_udp_connects(&e.net_log);
        let mut pool: Vec<String> = refused.clone().unwrap_or_default();
        let mut packets = 0;
        for l in &lines {
            let dest = l
                .split_whitespace()
                .find_map(|f| f.strip_prefix("dest="))
                .unwrap_or("-");
            let at = pool.iter().position(|a| port_of(a) == port_of(dest));
            match at {
                Some(k) if l.contains("proto=udp") => {
                    pool.remove(k);
                }
                _ => packets += 1,
            }
        }
        let mut seen: Vec<String> = refused.clone().unwrap_or_default();
        seen.sort();
        seen.dedup();
        eprintln!(
            "egress step={name} i={i} run={n} waited_s={waited} denied={} connect_only={} packets={packets} net_log={} refused_connects={} line={}",
            lines.len(),
            lines.len() - packets,
            if refused.is_some() { "read" } else { "unread" },
            if seen.is_empty() { "-".to_string() } else { seen.join(",") },
            line.as_deref().unwrap_or("-")
        );
        for l in &lines {
            let fields: Vec<&str> = l
                .split_whitespace()
                .filter(|f| !f.starts_with("at=") && *f != "denied")
                .collect();
            eprintln!("egress_denied step={name} i={i} {}", fields.join(" "));
        }
        if packets == 0 {
            EgressSeen::Clean
        } else {
            EgressSeen::Denied(packets)
        }
    }

    const WAY_EVIL: &str = "evil.agent.test";

    async fn way_open(ex: &mut Executor, ctx: &Ctx, row: &str) -> Res<Listener> {
        way_open_on(ex, ctx, row, UTILITY).await
    }

    async fn way_open_on(ex: &mut Executor, ctx: &Ctx, row: &str, host: &str) -> Res<Listener> {
        let relay = relay_of(ctx)?;
        let program = ctx.repo.join("tools/r3-trials/web/ways-listener.mjs");
        let listener = Listener::start_ways(&program).map_err(|e| Fault::new("listener", e))?;
        relay.allow(host, listener.port);
        ex.arm_rule(vec![format!("http://{host}:{}", listener.port)], false)
            .await?;
        if let Err(f) = ex
            .open(&format!("http://{host}:{}/way/{row}", listener.port))
            .await
        {
            listener.stop();
            return Err(f);
        }
        Ok(listener)
    }

    fn way_counts(listener: &mut Listener) -> Res<serde_json::Value> {
        listener
            .json_counts()
            .map_err(|e| Fault::new("listener", e))
    }

    fn path_count(c: &serde_json::Value, path: &str) -> u64 {
        c["paths"][path].as_u64().unwrap_or(0)
    }

    async fn way_plain(ex: &mut Executor, ctx: &Ctx, row: &str) -> Res<Option<u128>> {
        let mut listener = way_open(ex, ctx, row).await?;
        let wait = if row == "updater_wake" { 75 } else { 4 };
        tokio::time::sleep(Duration::from_secs(wait)).await;
        let c = way_counts(&mut listener);
        listener.stop();
        let c = c?;
        eprintln!("way_counts step={row} {}", c["paths"]);
        let page = format!("/way/{row}");
        let misses: Vec<String> = c["paths"]
            .as_object()
            .map(|m| {
                m.keys()
                    .filter(|k| **k != page && k.as_str() != "/favicon.ico")
                    .map(|k| format!("f{}", k.replace('/', "_")))
                    .collect()
            })
            .unwrap_or_default();
        outcome(misses, None)
    }

    async fn no_worker_paths(ex: &mut Executor, ctx: &Ctx) -> Res<Option<u128>> {
        let mut listener = way_open_on(ex, ctx, "no_worker_paths", "127.0.0.1").await?;
        tokio::time::sleep(Duration::from_secs(4)).await;
        let _ = ex
            .open(&format!("http://127.0.0.1:{}/sxg", listener.port))
            .await;
        tokio::time::sleep(Duration::from_secs(1)).await;
        let c = way_counts(&mut listener);
        listener.stop();
        let c = c?;
        eprintln!(
            "way_counts step=no_worker_paths {} sxg_accept={}",
            c["paths"], c["sxg_accept"]
        );
        let mut misses = Vec::new();
        if path_count(&c, "/sw.js") > 0 {
            misses.push("worker_script".to_string());
        }
        if path_count(&c, "/pre") > 0 {
            misses.push("prerender".to_string());
        }
        if c["sxg_accept"].as_u64().unwrap_or(0) > 0 {
            misses.push("signed_exchange_accept".to_string());
        }
        outcome(misses, None)
    }

    async fn after_unload(ex: &mut Executor, ctx: &Ctx) -> Res<Option<u128>> {
        let relay = relay_of(ctx)?;
        let mut listener = way_open(ex, ctx, "after_unload").await?;
        tokio::time::sleep(Duration::from_secs(1)).await;
        let next = ex
            .open(&format!("http://{UTILITY}:{}/way/next", listener.port))
            .await;
        tokio::time::sleep(Duration::from_secs(3)).await;
        let c = way_counts(&mut listener);
        let port = listener.port;
        listener.stop();
        next?;
        let c = c?;
        let refused = relay.refusals_of(WAY_EVIL, port);
        eprintln!(
            "way_counts step=after_unload {} relay_refused_evil={refused}",
            c["paths"]
        );
        let misses: Vec<String> = ["/beacon", "/keepalive", "/later", "/ping"]
            .iter()
            .filter(|p| path_count(&c, p) > 0)
            .map(|p| format!("f{}", p.replace('/', "_")))
            .collect();
        outcome(misses, None)
    }

    async fn way_reports(ex: &mut Executor, ctx: &Ctx) -> Res<Option<u128>> {
        let mut listener = way_open(ex, ctx, "reports").await?;
        tokio::time::sleep(Duration::from_secs(65)).await;
        let c = way_counts(&mut listener);
        listener.stop();
        let c = c?;
        eprintln!("way_counts step=reports {}", c["paths"]);
        let misses: Vec<String> = ["/report", "/report-to", "/report-uri"]
            .iter()
            .filter(|p| path_count(&c, p) > 0)
            .map(|p| format!("f{}", p.replace('/', "_")))
            .collect();
        outcome(misses, None)
    }

    fn processes() -> Vec<(u32, u32, String, String)> {
        let text = if cfg!(windows) {
            std::process::Command::new("powershell.exe")
                .args([
                    "-NoLogo",
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    "Get-CimInstance Win32_Process | ForEach-Object { \"$($_.ProcessId)`t$($_.ParentProcessId)`t$($_.Name)`t$($_.CommandLine)\" }",
                ])
                .start_output()
        } else {
            std::process::Command::new("ps")
                .args(["-axww", "-o", "pid=,ppid=,comm=,command="])
                .start_output()
        };
        let Ok(o) = text else { return Vec::new() };
        String::from_utf8_lossy(&o.stdout)
            .lines()
            .filter_map(|l| {
                if cfg!(windows) {
                    let mut w = l.splitn(4, '\t');
                    let pid = w.next()?.trim().parse().ok()?;
                    let ppid = w.next()?.trim().parse().ok()?;
                    let name = w.next()?.trim().to_string();
                    Some((pid, ppid, name, w.next().unwrap_or("").to_string()))
                } else {
                    let mut w = l.split_whitespace();
                    let pid = w.next()?.parse().ok()?;
                    let ppid = w.next()?.parse().ok()?;
                    let rest: Vec<&str> = w.collect();
                    let cut = rest
                        .iter()
                        .skip(1)
                        .position(|x| x.starts_with('/'))
                        .map(|k| k + 1)
                        .unwrap_or(rest.len());
                    Some((pid, ppid, rest[..cut].join(" "), rest[cut..].join(" ")))
                }
            })
            .collect()
    }

    async fn handler_launch(ex: &mut Executor, ctx: &Ctx) -> Res<Option<u128>> {
        let before: std::collections::BTreeSet<u32> =
            processes().into_iter().map(|p| p.0).collect();
        let listener = way_open(ex, ctx, "handler_launch").await?;
        tokio::time::sleep(Duration::from_secs(4)).await;
        listener.stop();
        let after = processes();
        let parent: std::collections::BTreeMap<u32, u32> =
            after.iter().map(|p| (p.0, p.1)).collect();
        let checks: std::collections::BTreeSet<u32> = after
            .iter()
            .filter(|p| is_check_command(&p.3))
            .map(|p| p.0)
            .collect();
        let own = std::process::id();
        let below_own = |mut p: u32| {
            for _ in 0..64 {
                if p == own || checks.contains(&p) {
                    return true;
                }
                match parent.get(&p) {
                    Some(&q) if q != p && q != 0 => p = q,
                    _ => return false,
                }
            }
            false
        };
        let new: Vec<String> = after
            .iter()
            .filter(|p| !before.contains(&p.0) && !below_own(p.0))
            .map(|p| {
                p.2.rsplit(['/', '\\'])
                    .next()
                    .unwrap_or("-")
                    .replace(' ', "_")
            })
            .collect();
        eprintln!(
            "handler_launch new_outside_tree={} names={}",
            new.len(),
            if new.is_empty() {
                "-".to_string()
            } else {
                new.join(",")
            }
        );
        let misses = if new.is_empty() {
            Vec::new()
        } else {
            vec![format!("new_processes_{}", new.len())]
        };
        outcome(misses, None)
    }

    async fn reach(ex: &mut Executor, ctx: &Ctx, context: &str) -> Res<Option<u128>> {
        let relay = relay_of(ctx)?;
        let program = ctx.repo.join("tools/r3-trials/web/ways-listener.mjs");
        let mut listener = Listener::start_ways(&program).map_err(|e| Fault::new("listener", e))?;
        let p = listener.port;
        for h in ["127.0.0.1", UTILITY, WAY_EVIL] {
            relay.allow(h, p);
        }
        let armed = ex
            .arm_rule(
                vec![
                    format!("http://127.0.0.1:{p}"),
                    format!("http://{UTILITY}:{p}"),
                ],
                false,
            )
            .await;
        let page = if context == "opened" {
            "opener"
        } else {
            context
        };
        let mut done = match armed {
            Ok(()) => ex.open(&format!("http://127.0.0.1:{p}/reach/{page}")).await,
            Err(f) => Err(f),
        };
        if done.is_ok() && context == "opened" {
            done = match ex.resolve_one(&[role("button", "Open")], &[]).await {
                Ok(b) => ex.click(&b).await,
                Err(f) => Err(f),
            };
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
        let c = way_counts(&mut listener);
        listener.stop();
        done?;
        let c = c?;
        let evil: u64 = c["evil"]
            .as_object()
            .map(|m| m.values().filter_map(|v| v.as_u64()).sum())
            .unwrap_or(0);
        let ran = c["did"][context].as_u64().unwrap_or(0);
        let paused_reads = ex
            .paused()
            .iter()
            .filter(|q| q.path.starts_with("/read") && q.origin.contains(WAY_EVIL))
            .count();
        eprintln!(
            "reach context={context} evil_requests={evil} evil_bytes={} ran={ran} did={} paused_reads={paused_reads} relay_refused_evil={}",
            c["evil_bytes"],
            c["did"],
            relay.refusals_of(WAY_EVIL, p)
        );
        let mut misses = Vec::new();
        if evil > 0 {
            misses.push("bytes_reached".to_string());
        }
        if ran == 0 && context != "service" {
            misses.push("context_did_not_run".to_string());
        }
        outcome(misses, None)
    }

    async fn ws_allowed_run(
        ex: &mut Executor,
        ctx: &Ctx,
        i: u32,
        policy: bool,
    ) -> Res<serde_json::Value> {
        let relay = relay_of(ctx)?;
        let dir = ctx.scratch.join(format!(
            "{}-ws-{}-{i}",
            std::process::id(),
            if policy { "policy" } else { "control" }
        ));
        let (cert, key) = make_certificate(&dir).map_err(|e| match e {
            CertFault::NoOpenssl => Fault::new("openssl", "no openssl"),
            CertFault::Failed(d) => Fault::new("certificate", d),
        })?;
        let program = ctx.repo.join("tools/r3-trials/web/ws-listener.mjs");
        let mut listener =
            Listener::start_ws(&program, &cert, &key).map_err(|e| Fault::new("listener", e))?;
        let first = listener
            .json_counts()
            .map_err(|e| Fault::new("listener", e))?;
        let port = |k: &str| first["ports"][k].as_u64().unwrap_or(0) as u16;
        let (p, q, r) = (port("http"), port("wss"), port("https"));
        for pt in [p, q, r] {
            relay.allow(UTILITY, pt);
        }
        let allowed = vec![
            format!("http://{UTILITY}:{p}"),
            format!("https://{UTILITY}:{r}"),
        ];
        if policy {
            ex.arm_rule_with_policy(allowed, false, WS_POLICY).await?;
        } else {
            ex.arm_rule(allowed, false).await?;
        }
        let opened = ex.open(&format!("http://{UTILITY}:{p}/page")).await;
        tokio::time::sleep(Duration::from_secs(5)).await;
        let counts = listener
            .json_counts()
            .map_err(|e| Fault::new("listener", e));
        listener.stop();
        opened?;
        let mut c = counts?;
        c["policy_counts"] = serde_json::to_value(ex.policy_counts()).unwrap_or_default();
        Ok(c)
    }

    const WS_CONTEXTS: [&str; 7] = [
        "page",
        "frame",
        "worker",
        "shared",
        "blank",
        "dataframe",
        "blobworker",
    ];

    fn ctx_count(c: &serde_json::Value, map: &str, ctx: &str) -> u64 {
        c[map][ctx].as_u64().unwrap_or(0)
    }

    async fn ws_allowed(ex: &mut Executor, ctx: &Ctx, i: u32) -> Res<Option<u128>> {
        let c = ws_allowed_run(ex, ctx, i, true).await?;
        let mut misses = Vec::new();
        for k in WS_CONTEXTS {
            if ctx_count(&c, "ws", k) > 0 {
                misses.push(format!("ws_{k}"));
            }
            if ctx_count(&c, "f", k) == 0 {
                misses.push(format!("fetch_{k}"));
            }
            if ctx_count(&c, "r", k) == 0 {
                misses.push(format!("data_blob_{k}"));
            }
        }
        let wss = c["wss_connections"].as_u64().unwrap_or(0);
        let https = c["https_connections"].as_u64().unwrap_or(0);
        if wss > 0 {
            misses.push("wss".to_string());
        }
        if https < WS_CONTEXTS.len() as u64 {
            misses.push("https_fetch".to_string());
        }
        eprintln!(
            "ws_allowed i={i} ws={} fetch={} data_blob={} wss_connections={wss} https_connections={https} policy={}",
            c["ws"],
            c["f"],
            c["r"],
            c["policy_counts"]
        );
        outcome(misses, None)
    }

    async fn ws_allowed_control(ex: &mut Executor, ctx: &Ctx, i: u32) -> Res<Option<u128>> {
        let c = ws_allowed_run(ex, ctx, i, false).await?;
        let arrived = WS_CONTEXTS
            .iter()
            .filter(|k| ctx_count(&c, "ws", k) > 0)
            .count();
        let wss = c["wss_connections"].as_u64().unwrap_or(0);
        eprintln!(
            "ws_allowed_control i={i} ws={} fetch={} wss_connections={wss}",
            c["ws"], c["f"]
        );
        if arrived == WS_CONTEXTS.len() && wss > 0 {
            Ok(None)
        } else {
            Err(Fault::new("check", format!("arrived_{arrived}_wss_{wss}")))
        }
    }

    async fn frame_doc(ex: &mut Executor, ctx: &Ctx) -> Res<Option<u128>> {
        signin(ex, ctx).await?;
        let u = origin(UTILITY, ctx.port);
        ex.arm_rule(vec![u.clone(), origin(PAY, ctx.port)], false)
            .await?;
        ex.open(&ctx.u("/pay")).await?;
        let doc = ex
            .paused()
            .into_iter()
            .find(|p| p.path == "/frame" && p.resource == "Document");
        let Some(doc) = doc else {
            return Err(Fault::new("check", "no_frame_document"));
        };
        let main = ex.main_frame().await?;
        let mut misses = Vec::new();
        if doc.frame == *main.inner() {
            misses.push("frame_is_main".to_string());
        }
        match ex.parent_origin_of(&doc.frame).await? {
            Some(o) if o == u => {}
            _ => misses.push("page_origin".to_string()),
        }
        if doc.origin != origin(PAY, ctx.port) {
            misses.push("frame_origin".to_string());
        }
        outcome(misses, None)
    }

    struct Siblings {
        go: std::sync::Arc<std::sync::atomic::AtomicBool>,
        thread: std::thread::JoinHandle<Vec<std::process::Child>>,
    }

    impl Siblings {
        fn start(_ctx: &Ctx) -> Siblings {
            let go = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
            let running = go.clone();
            let ready = std::sync::Arc::new(std::sync::Barrier::new(2));
            let started = ready.clone();
            let thread = std::thread::spawn(move || {
                let mut kids = Vec::new();
                started.wait();
                let Ok(exe) = std::env::current_exe() else {
                    return kids;
                };
                while running.load(std::sync::atomic::Ordering::SeqCst) && kids.len() < 80 {
                    let mut cmd = std::process::Command::new(&exe);
                    cmd.arg("--hold-child")
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null());
                    if let Ok(c) = cmd.start() {
                        kids.push(c);
                    }
                    std::thread::sleep(Duration::from_millis(25));
                }
                kids
            });
            ready.wait();
            Siblings { go, thread }
        }

        fn stop(self) -> Vec<std::process::Child> {
            self.go.store(false, std::sync::atomic::Ordering::SeqCst);
            self.thread.join().unwrap_or_default()
        }
    }

    async fn pipe_inherit(ex: &mut Executor, ctx: &Ctx) -> Res<Option<u128>> {
        let ids = ex
            .pipe_ids()
            .cloned()
            .ok_or_else(|| Fault::new("check", "no_pipe"))?;
        let pids: Vec<u32> = ctx.siblings.borrow().iter().map(|c| c.id()).collect();
        let holders = pids
            .iter()
            .filter(|p| checks::holds_an_end(**p, &ids))
            .count();
        let root = ex
            .root()
            .cloned()
            .ok_or_else(|| Fault::new("check", "no_root"))?;
        let table = r3_web::procs::table().map_err(|e| Fault::new("check", e.to_string()))?;
        let after = table
            .iter()
            .filter(|p| pids.contains(&p.pid) && p.started_us >= root.started_us)
            .count();
        eprintln!(
            "pipe_inherit siblings={} after_the_browser={after} holders={holders}",
            pids.len()
        );
        check(after >= 1, "no_sibling_after_the_pipe")?;
        check(holders == 0, "sibling_holds_an_end")?;
        Ok(None)
    }

    async fn relay_foreign(ex: &mut Executor, ctx: &Ctx) -> Res<Option<u128>> {
        let relay = relay_of(ctx)?;
        signin(ex, ctx).await?;
        let program = ctx.repo.join("tools/r3-trials/web/record-listener.mjs");
        let mut listener =
            Listener::start_record(&program, "/foreign").map_err(|e| Fault::new("listener", e))?;
        relay.allow(UTILITY, listener.port);
        let foreign_before = relay.foreign_count();
        let listener_port = listener.port;
        let refusals_before = relay.refusals_of(UTILITY, listener_port);
        let exe = std::env::current_exe().map_err(|e| Fault::new("probe", e.to_string()))?;
        let t0 = Instant::now();
        let mut cmd = std::process::Command::new(exe);
        cmd.arg("--foreign-probe")
            .arg(ctx.relay_port.to_string())
            .arg(UTILITY)
            .arg(listener.port.to_string())
            .stdin(std::process::Stdio::null());
        let out = tokio::task::spawn_blocking(move || r3_web::start::output(&mut cmd))
            .await
            .map_err(|_| Fault::new("probe", "join"))?
            .map_err(|e| Fault::new("probe", e.kind().to_string()))?;
        let deadline = Instant::now() + Duration::from_secs(5);
        while relay.foreign_count() == foreign_before && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let ms = t0.elapsed().as_millis();
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        let bytes: Option<u64> = text
            .split_whitespace()
            .find_map(|w| w.strip_prefix("bytes="))
            .and_then(|v| v.parse().ok());
        let counts = listener.record_counts();
        listener.stop();
        let c = counts.map_err(|e| Fault::new("listener", e))?;
        let foreign = relay.foreign_count() - foreign_before;
        let refusals = relay.refusals_of(UTILITY, listener_port) - refusals_before;
        eprintln!(
            "relay_foreign answer_bytes={} end={} foreign={foreign} agent_refusals={refusals} listener_connections={} last={:?}",
            bytes.map(|b| b.to_string()).unwrap_or("-".into()),
            text.split_whitespace()
                .find_map(|w| w.strip_prefix("end="))
                .unwrap_or("-"),
            c.connections,
            relay.foreign().last()
        );
        check(text.contains("connected=yes"), "probe_not_connected")?;
        check(
            text.contains("end=reset"),
            "request_read_before_the_refusal",
        )?;
        check(bytes == Some(0), "relay_answered")?;
        check(foreign == 1, "not_recorded_apart")?;
        check(refusals == 0, "recorded_as_the_agent_s")?;
        check(c.connections == 0 && c.bytes == 0, "listener_reached")?;
        let back = ex.resolve_one(&[role("link", "Pay bill")], &[]).await;
        check(back.is_ok(), "agent_route_lost")?;
        Ok(Some(ms))
    }

    async fn start_probe(ex: &mut Executor, ctx: &Ctx) -> Res<Option<u128>> {
        let relay = relay_of(ctx)?;
        let probed = relay.refusals_of(PROBE_HOST, PROBE_PORT) - ctx.probe_before.get();
        let total = counter(ctx, &["requests", "total"])?;
        eprintln!("start_probe refusals={probed} fixture_requests_before_act={total}");
        check(probed == 1, "no_probe_refusal")?;
        check(total == 0, "request_before_the_probe")?;
        ex.open(&ctx.u("/signin")).await?;
        check(
            counter(ctx, &["requests", "total"])? > 0,
            "agent_did_not_act",
        )?;
        Ok(None)
    }

    async fn tree_lines_hold(ex: &Executor, needle: &str) -> Res<usize> {
        let root = ex
            .root()
            .cloned()
            .ok_or_else(|| Fault::new("check", "no_root"))?;
        let table = r3_web::procs::table().map_err(|e| Fault::new("check", e.to_string()))?;
        Ok(checks::tree_of(&table, &root)
            .iter()
            .filter(|p| {
                checks::command_line(p.pid)
                    .map(|l| l.contains(needle))
                    .unwrap_or(false)
            })
            .count())
    }

    async fn same_site_pref(ex: &mut Executor, ctx: &Ctx) -> Res<Option<u128>> {
        signin(ex, ctx).await?;
        ex.open(&ctx.u("/pay")).await?;
        let apart = pay_frame_target(ex, ctx).await.is_ok();
        let named = tree_lines_hold(ex, "agent.test").await?;
        eprintln!("same_site_pref frame_apart={apart} lines_naming_the_site={named}");
        check(apart, "frame_in_page_process")?;
        check(named == 0, "site_on_a_command_line")?;
        Ok(None)
    }

    async fn same_site_rule(ex: &mut Executor, ctx: &Ctx) -> Res<Option<u128>> {
        signin(ex, ctx).await?;
        ex.open(&ctx.u("/pay")).await?;
        let apart = pay_frame_target(ex, ctx).await.is_ok();
        ex.arm_rule_on_page_only(
            vec![origin(UTILITY, ctx.port), origin(PAY, ctx.port)],
            false,
        )
        .await?;
        let keys = [STATE_CHANGING, PAYMENTS];
        let before = read_counters(ctx, &keys)?;
        let pay = origin(PAY, ctx.port);
        let t0 = Instant::now();
        let sent = ex
            .run_frame_script(
                &format!("{pay}/"),
                "(() => { fetch('/charge', {method: 'POST', body: 'halvo-same-site', mode: 'no-cors'}).catch(() => {}); return true; })()",
            )
            .await?;
        check(sent == serde_json::Value::Bool(true), "script")?;
        let seen = paused_after(ex, t0, |p| {
            p.method == "POST" && p.origin == pay && p.path == "/charge"
        })
        .await;
        eprintln!(
            "same_site_rule frame_apart={apart} paused={}",
            seen.as_ref()
                .map(|(p, _)| format!("{:?}", p.outcome))
                .unwrap_or("none".into())
        );
        if let Some((p, _)) = &seen {
            check(p.outcome == Outcome::Failed, "not_failed")?;
        }
        held(ctx, &keys, &before, seen).await
    }

    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub(crate) struct OrgFacts {
        control_before: Option<String>,
        value_read: Option<String>,
        value_want: Option<String>,
        page_named: Option<String>,
        page_cause: Option<String>,
    }

    impl OrgFacts {
        fn from_env() -> OrgFacts {
            let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
            OrgFacts {
                control_before: var("R3_ORG_CONTROL_BEFORE"),
                value_read: var("R3_ORG_VALUE_READ"),
                value_want: var("R3_ORG_VALUE_WANT"),
                page_named: var("R3_ORG_PAGE_NAMED"),
                page_cause: var("R3_ORG_PAGE_CAUSE"),
            }
        }

        pub(crate) fn lacks(&self) -> Option<&'static str> {
            if self.control_before.as_deref() != Some("reply") {
                return Some("no_control_reply");
            }
            match (&self.value_read, &self.value_want) {
                (Some(r), Some(w)) if r == w => {}
                _ => return Some("no_value_read"),
            }
            match self.page_named.as_deref() {
                Some("yes" | "unread") => None,
                _ => Some("page_not_named"),
            }
        }

        fn keys(&self) -> String {
            let v = |o: &Option<String>| o.clone().unwrap_or_else(|| "-".into());
            let cause = if self.page_named.as_deref() == Some("unread") {
                format!(" page_cause={}", v(&self.page_cause))
            } else {
                String::new()
            };
            format!(
                " control_before={} value_read={} page_named={}{cause}",
                v(&self.control_before),
                v(&self.value_read),
                v(&self.page_named)
            )
        }
    }

    async fn org_outcome(ctx: &Ctx, name: &str, started: Res<Executor>) -> Result<Run, NotRun> {
        let (want_cause, want_message) = if name == "org_debugging" {
            ("no_channel", NO_CHANNEL_MESSAGE)
        } else {
            ("no_route", NO_ROUTE_MESSAGE)
        };
        let not_seen = OrgFacts::from_env().page_named.as_deref() == Some("no");
        let (ok, cause, version, left) = match started {
            Ok(ex) => {
                let v = ex.version().await.unwrap_or_else(|_| "-".into());
                let stopped = ex.stop_and_remove().await;
                if not_seen {
                    return Err(NotRun::new(
                        "policy_not_seen reason=page_not_named",
                        format!("start=started left_after={}", stopped.ended.after),
                    ));
                }
                (false, "started".to_string(), v, Some(stopped.ended))
            }
            Err(f) if f.0 == "signal" => return Err(NotRun::new("signal", "")),
            Err(f) if not_seen => {
                return Err(NotRun::new(
                    "policy_not_seen reason=page_not_named",
                    format!(
                        "start={} left_after={}",
                        f.0,
                        left_of_fault(&f.1).map_or(0, |e| e.after)
                    ),
                ))
            }
            Err(f) => {
                let message = f.1.split(" stderr_kept_bytes_").next().unwrap_or("");
                let total = counter(ctx, &["requests", "total"]).unwrap_or(-1);
                let entries = std::fs::read_dir(&ctx.scratch)
                    .map(|d| d.count())
                    .unwrap_or(0);
                eprintln!(
                    "org step={name} cause={} message_is_plain={} fixture_requests={total} scratch_entries={entries}",
                    f.0,
                    message == want_message
                );
                let effect = f.0 == want_cause && message == want_message && total == 0;
                let lacks = OrgFacts::from_env().lacks();
                let ok = effect && lacks.is_none();
                let cause = match (effect, lacks) {
                    (true, Some(l)) => l.to_string(),
                    _ => f.0.to_string(),
                };
                (ok, cause, "-".to_string(), left_of_fault(&f.1))
            }
        };
        let left_after = left.map_or(0, |e| e.after);
        let (ok, cause) = if ok && left_after > 0 {
            (false, "left_after".to_string())
        } else {
            (ok, cause)
        };
        Ok(Run {
            left,
            procs: None,
            ok,
            cause: if ok { "-".into() } else { cause },
            ms: 0,
            version: if version == "-" {
                format!("{}/0", ctx.kind)
            } else {
                version
            },
            peers: None,
            checks: None,
        })
    }

    async fn step(ex: &mut Executor, ctx: &Ctx, name: &str, i: u32) -> Res<Option<u128>> {
        let whole = |r: Res<()>| r.map(|_| None);
        match name {
            "signin_form" => whole(signin_form(ex, ctx).await),
            "table_read" => whole(table_read(ex, ctx).await),
            "form_fill" => whole(form_fill(ex, ctx).await),
            "payment_frame" => whole(payment_frame(ex, ctx).await),
            "file_upload" => whole(file_upload(ex, ctx, i).await),
            "session_expiry" => whole(session_expiry(ex, ctx).await),
            "changed_page_1" => whole(changed_page_1(ex, ctx).await),
            "block_request" => whole(block_request(ex, ctx).await),
            "credential_mark" => whole(credential_mark(ex, ctx).await),
            "payment_frame_isolated" => whole(payment_frame_isolated(ex, ctx).await),
            "changed_page_1_isolated" => whole(changed_page_1_isolated(ex, ctx).await),
            "block_popup" => block_popup(ex, ctx).await,
            "block_frame_post" => block_frame_post(ex, ctx).await,
            "block_websocket" => block_websocket(ex, ctx).await,
            "cert_error" => cert_error(ex, ctx, i).await,
            "list_refuse" => list_refuse(ex, ctx).await,
            "list_hops" => list_hops(ex, ctx).await,
            "list_close" => list_close(ex, ctx).await,
            "own_traffic" => own_traffic(ex, ctx).await,
            "ws_allowed" => ws_allowed(ex, ctx, i).await,
            "ws_allowed_control" => ws_allowed_control(ex, ctx, i).await,
            "frame_doc" => frame_doc(ex, ctx).await,
            "reach_page" | "reach_opened" | "reach_frame" | "reach_worker" | "reach_shared"
            | "reach_service" => reach(ex, ctx, &name["reach_".len()..]).await,
            "pipe_inherit" => pipe_inherit(ex, ctx).await,
            "relay_foreign" => relay_foreign(ex, ctx).await,
            "start_probe" => start_probe(ex, ctx).await,
            "same_site_pref" => same_site_pref(ex, ctx).await,
            "same_site_rule" => same_site_rule(ex, ctx).await,
            "after_unload" => after_unload(ex, ctx).await,
            "handler_launch" => handler_launch(ex, ctx).await,
            "no_worker_paths" => no_worker_paths(ex, ctx).await,
            "reports" => way_reports(ex, ctx).await,
            "udp_quic" | "webtransport" | "peer_connection" | "secure_dns" | "lan_discovery"
            | "link_lookup" | "updater_wake" => way_plain(ex, ctx, name).await,
            _ => Err(Fault::new("usage", name.to_string())),
        }
    }

    struct Run {
        ok: bool,
        cause: String,
        ms: u128,
        version: String,
        peers: Option<(u32, u32)>,
        checks: Option<Vec<checks::Count>>,
        left: Option<checks::Ended>,
        procs: Option<usize>,
    }

    fn left_of_fault(detail: &str) -> Option<checks::Ended> {
        let num = |key: &str| {
            detail
                .split_whitespace()
                .find_map(|w| w.strip_prefix(key))
                .and_then(|n| n.parse::<usize>().ok())
        };
        Some(checks::Ended {
            before: num("left_before_")?,
            after: num("left_after_")?,
            ..Default::default()
        })
    }

    #[cfg(unix)]
    fn socket_owners(port: u16) -> Option<Vec<u32>> {
        let out = std::process::Command::new("lsof")
            .args(["-nP", &format!("-iTCP:{port}"), "-sTCP:ESTABLISHED", "-Fpn"])
            .start_output()
            .ok()?;
        let suffix = format!("->127.0.0.1:{port}");
        let mut pid = 0;
        let mut owners = Vec::new();
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            if let Some(p) = line.strip_prefix('p') {
                pid = p.parse().unwrap_or(0);
            } else if let Some(n) = line.strip_prefix('n') {
                if n.ends_with(&suffix) {
                    owners.push(pid);
                }
            }
        }
        Some(owners)
    }

    #[cfg(windows)]
    fn socket_owners(port: u16) -> Option<Vec<u32>> {
        let out = std::process::Command::new("netstat")
            .args(["-ano", "-p", "TCP"])
            .start_output()
            .ok()?;
        let suffix = format!(":{port}");
        let mut owners = Vec::new();
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            let cols: Vec<&str> = line.split_whitespace().collect();
            if cols.len() == 5 && cols[3] == "ESTABLISHED" && cols[2].ends_with(&suffix) {
                if let Ok(pid) = cols[4].parse() {
                    owners.push(pid);
                }
            }
        }
        Some(owners)
    }

    fn fixture_peers(port: u16) -> Option<(u32, u32)> {
        let me = std::process::id();
        let owners = socket_owners(port)?;
        let own = owners.iter().filter(|p| **p == me).count() as u32;
        Some((owners.len() as u32, own))
    }

    struct NotRun {
        cause: &'static str,
        detail: String,
    }

    impl NotRun {
        fn new(cause: &'static str, detail: impl Into<String>) -> Self {
            NotRun {
                cause,
                detail: detail.into(),
            }
        }
    }

    async fn run_once(ctx: &Ctx, name: &str, i: u32) -> Result<Run, NotRun> {
        if ctx.stop.is_set() {
            return Err(NotRun::new("signal", ""));
        }
        ctx.control
            .reset(variants_of(name))
            .map_err(|e| NotRun::new("fixture", e))?;
        if ctx.round2 {
            if let Some(r) = &ctx.relay {
                r.set_list(Some(fixture_list(ctx)));
            }
        }
        let mut opts = ctx.options(&format!("p-{name}-{i}"));
        let egress = if EGRESS_STEPS.contains(&name) {
            Some(egress_setup(ctx, &mut opts, name, i).map_err(|e| NotRun::new("egress", e))?)
        } else {
            None
        };
        if let Some(r) = &ctx.relay {
            ctx.probe_before.set(r.refusals_of(PROBE_HOST, PROBE_PORT));
        }
        if name == "same_site_pref" {
            opts.local_state = Some(same_site_local_state(ctx.port));
        }
        let siblings = (name == "pipe_inherit").then(|| Siblings::start(ctx));
        let mark = opts.mark_now();
        let started = if ctx.round2 {
            ctx.stop.or(Executor::start_settled(&opts)).await
        } else {
            ctx.stop.or(Executor::start(&opts)).await
        };
        if matches!(&started, Err(f) if f.0 == "signal") {
            let e = checks::end_runs_own(&mark);
            eprintln!(
                "signal step={name} i={i} left_before={} left_after={}",
                e.before, e.after
            );
        }
        if let Some(s) = siblings {
            ctx.siblings.replace(s.stop());
        }
        if ORG_STEPS.contains(&name) {
            return org_outcome(ctx, name, started).await;
        }
        let mut ex = match started {
            Ok(ex) => ex,
            Err(f) if f.0 == "signal" => return Err(NotRun::new("signal", "")),
            Err(f) => {
                eprintln!(
                    "start step={name} i={i} cause={} detail={}",
                    f.0,
                    printable(&f.1)
                );
                return Err(NotRun::new("browser_start", ""));
            }
        };
        let version = ex.version().await.unwrap_or_else(|_| "-".into());
        let subject = match (ctx.round3, ex.root().cloned()) {
            (true, Some(root)) => Some((root, ex.pipe_ids().cloned())),
            _ => None,
        };
        let read =
            |at: &str, subject: &Option<(r3_web::procs::Proc, Option<r3_web::start::PipeIds>)>| {
                subject.as_ref().map(|(root, pipe)| {
                    let (counts, named) = checks::check_named(&checks::Subject {
                        root,
                        profile: &opts.profile,
                        pipe: pipe.as_ref(),
                        relay_port: ctx.relay.as_ref().map(|_| ctx.relay_port),
                    });
                    for n in &named {
                        out(&format!(
                            "check_named machine={} browser={} step={name} i={i} at={at} {}",
                            ctx.machine,
                            ctx.kind,
                            n.keys()
                        ));
                    }
                    eprintln!(
                        "check step={name} i={i} at={at} {}",
                        counts
                            .iter()
                            .map(|c| format!("{}={}", c.rule, c.count))
                            .collect::<Vec<_>>()
                            .join(" ")
                    );
                    counts
                })
            };
        let at_start = read("start", &subject);
        let t0 = Instant::now();
        let outcome = ctx.stop.or(step(&mut ex, ctx, name, i)).await;
        let at_session = read("session", &subject);
        let ms = match &outcome {
            Ok(Some(from_action)) => *from_action,
            _ => t0.elapsed().as_millis(),
        };
        let peers = if ctx.routes {
            fixture_peers(ctx.port)
        } else {
            None
        };
        let faults = ex.attach_faults();
        if !faults.is_empty() {
            eprintln!(
                "attach_faults step={name} i={i} n={} first={}",
                faults.len(),
                printable(&faults[0])
            );
        }
        let keep = ex.keep_pipe();
        let stopped = ex.stop_and_remove().await;
        let at_end = read("end", &subject);
        drop(keep);
        let checked = match (at_start, at_session, at_end) {
            (Some(a), Some(b), Some(c)) => Some(
                a.iter()
                    .zip(b.iter())
                    .zip(c.iter())
                    .map(|((x, y), z)| checks::Count {
                        rule: x.rule,
                        count: x.count + y.count + z.count,
                    })
                    .collect(),
            ),
            _ => None,
        };
        for mut child in ctx.siblings.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let outcome = match (&egress, outcome) {
            (Some(e), Ok(v)) => match egress_read(ctx, e, name, i).await {
                EgressSeen::Clean => Ok(v),
                EgressSeen::Denied(n) => Err(Fault::new("egress", format!("denied_{n}"))),
                EgressSeen::Unread(d) => Err(Fault::new("egress_open", d)),
            },
            (Some(e), Err(f)) => {
                let _ = egress_read(ctx, e, name, i).await;
                Err(f)
            }
            (None, o) => o,
        };
        if let Err(f) = &outcome {
            if f.0 == "signal" {
                return Err(NotRun::new("signal", ""));
            }
            eprintln!(
                "miss step={name} i={i} cause={} detail={}",
                f.0,
                printable(&f.1)
            );
            if f.0 == "control" {
                return Err(NotRun::new(
                    "fixture",
                    format!("control during the run: {}", printable(&f.1)),
                ));
            }
        }
        let left_after = stopped.ended.after;
        Ok(Run {
            ok: outcome.is_ok() && left_after == 0,
            cause: match outcome.err() {
                Some(f) => f.0.to_string(),
                None if left_after > 0 => "left_after".into(),
                None => "-".into(),
            },
            ms,
            version,
            peers,
            checks: checked,
            left: Some(stopped.ended),
            procs: Some(stopped.pids.len() + stopped.adopted.len()),
        })
    }

    async fn cells(ctx: &Ctx, steps: &[&str], runs: u32) -> bool {
        let mut all_ran = true;
        for name in steps {
            if *name == "cert_error" && !openssl_runs() {
                out(&format!(
                    "notrun candidate={CANDIDATE} machine={} browser={} step={name} cause=openssl",
                    ctx.machine, ctx.kind
                ));
                all_ran = false;
                continue;
            }
            if *name == "handler_launch" && ctx.machine == "mac" {
                out(&format!(
                    "notrun candidate={CANDIDATE} machine={} browser={} step={name} cause=no_window_rule",
                    ctx.machine, ctx.kind
                ));
                continue;
            }
            if ORG_STEPS.contains(name) && ctx.machine == "mac" {
                out(&format!(
                    "notrun candidate={CANDIDATE} machine={} browser={} step={name} cause=no_policy_place",
                    ctx.machine, ctx.kind
                ));
                continue;
            }
            let mut ok = 0;
            let mut open_runs = 0;
            let mut checked_runs = 0;
            let mut check_ok = 0;
            let mut check_sums: Vec<(&'static str, usize)> = checks::RULES
                .iter()
                .chain(checks::KEYS.iter())
                .map(|r| (*r, 0))
                .collect();
            let mut peers = (0u32, 0u32);
            let mut times = Vec::new();
            let mut majors = Vec::new();
            let mut ran = true;
            for i in 1..=runs {
                match run_once(ctx, name, i).await {
                    Ok(r) => {
                        out(&format!(
                            "run candidate={CANDIDATE} machine={} browser={} step={name} i={i} result={} ms={} version={} cause={} procs={} left_before={} left_after={}",
                            ctx.machine,
                            ctx.kind,
                            if r.ok { "ok" } else { "miss" },
                            r.ms,
                            r.version.replace(' ', "_"),
                            r.cause,
                            r.procs.map_or("-".to_string(), |n| n.to_string()),
                            r.left.map_or("-".to_string(), |e| e.before.to_string()),
                            r.left.map_or("-".to_string(), |e| e.after.to_string())
                        ));
                        if let Some((all, own)) = r.peers {
                            peers.0 += all;
                            peers.1 += own;
                        }
                        if let Some(counts) = &r.checks {
                            checked_runs += 1;
                            if checks::read_holds(counts) {
                                check_ok += 1;
                            }
                            for c in counts {
                                if let Some(slot) =
                                    check_sums.iter_mut().find(|(r, _)| *r == c.rule)
                                {
                                    slot.1 += c.count;
                                }
                            }
                        }
                        if r.ok {
                            ok += 1;
                            times.push(r.ms);
                        }
                        if r.cause == "egress_open" {
                            open_runs += 1;
                        }
                        if let Some(m) = major_of(&r.version) {
                            majors.push(m);
                        }
                    }
                    Err(e) => {
                        if !e.detail.is_empty() {
                            eprintln!("control: {}", printable(&e.detail));
                        }
                        out(&format!(
                            "notrun candidate={CANDIDATE} machine={} browser={} step={name} cause={}",
                            ctx.machine, ctx.kind, e.cause
                        ));
                        ran = false;
                        break;
                    }
                }
            }
            if !ran {
                all_ran = false;
                if ctx.stop.is_set() {
                    break;
                }
                continue;
            }
            majors.sort_unstable();
            let majors_key = r3_web::majors_key(&majors);
            majors.dedup();
            let relay_key = if name.ends_with("_relay") {
                format!(" relay={}", name.trim_start_matches("driver_killed_"))
            } else if ctx.relayed {
                format!(" relay={}", ctx.form())
            } else {
                String::new()
            };
            let Some(major) = majors.first() else {
                out(&format!(
                    "notrun candidate={CANDIDATE} machine={} browser={} step={name} cause=version",
                    ctx.machine, ctx.kind
                ));
                all_ran = false;
                continue;
            };
            if majors.len() > 1 {
                out(&format!(
                    "note step={name} majors={}",
                    majors
                        .iter()
                        .map(|m| m.to_string())
                        .collect::<Vec<_>>()
                        .join(",")
                ));
            }
            if checked_runs > 0 {
                out(&format!(
                    "check machine={} browser={} step={name} ok={check_ok} of={checked_runs} {}",
                    ctx.machine,
                    ctx.kind,
                    check_sums
                        .iter()
                        .map(|(r, n)| format!("{r}={n}"))
                        .collect::<Vec<_>>()
                        .join(" ")
                ));
            }
            if EGRESS_STEPS.contains(name) {
                out(&format!(
                    "way row={name} machine={} browser={} closed={} runs={runs}",
                    ctx.machine,
                    ctx.kind,
                    if open_runs > 0 {
                        "open"
                    } else if ok == runs {
                        "yes"
                    } else {
                        "no"
                    }
                ));
                continue;
            }
            if *name == "own_traffic" || *name == "ws_allowed" {
                out(&format!(
                    "way row={name} machine={} browser={} closed={} runs={runs}",
                    ctx.machine,
                    ctx.kind,
                    if ok == runs { "yes" } else { "no" }
                ));
                continue;
            }
            if *name == "frame_doc" {
                out(&format!(
                    "way row=frame_doc form={} machine={} browser={} closed={} runs={runs} told={ok}",
                    ctx.form(),
                    ctx.machine,
                    ctx.kind,
                    if ok == runs { "yes" } else { "no" }
                ));
                continue;
            }
            if *name == "ws_allowed_control" {
                out(&format!(
                    "control row=ws_allowed machine={} browser={} arrived_runs={ok} runs={runs}",
                    ctx.machine, ctx.kind
                ));
                continue;
            }
            let org_keys = if ORG_STEPS.contains(name) {
                OrgFacts::from_env().keys()
            } else {
                String::new()
            };
            out(&format!(
                "cell candidate={CANDIDATE} machine={} browser={} major={major}{majors_key} step={name} ok={ok} of={runs} median_ms={}{relay_key}{org_keys}",
                ctx.machine,
                ctx.kind,
                median(&mut times)
            ));
            if ctx.round2 && ctx.relayed && STEPS.contains(name) {
                out(&format!(
                    "relay_cell machine={} browser={} form={} step={name} ok={ok} of={runs}",
                    ctx.machine,
                    ctx.kind,
                    ctx.form()
                ));
            }
            if ctx.routes {
                out(&format!(
                    "route candidate={CANDIDATE} machine={} browser={} step={name} relay={} fixture_conns={} from_driver={} from_other={}",
                    ctx.machine,
                    ctx.kind,
                    if ctx.relayed { "yes" } else { "no" },
                    peers.0,
                    peers.1,
                    peers.0 - peers.1
                ));
            }
        }
        all_ran
    }

    async fn cold_start(ctx: &Ctx, i: u32) -> Res<u128> {
        ctx.control
            .reset(&[])
            .map_err(|e| Fault::new("control", e))?;
        let t0 = Instant::now();
        let mut ex = ctx
            .stop
            .or(start_for(ctx, &ctx.options(&format!("p-cold-{i}"))))
            .await?;
        let opened = ctx.stop.or(ex.open(&ctx.u("/signin"))).await;
        let ms = t0.elapsed().as_millis();
        ex.stop_and_remove().await;
        opened.map(|_| ms)
    }

    async fn start_for(ctx: &Ctx, opts: &StartOptions) -> Res<Executor> {
        if ctx.round2 {
            Executor::start_settled(opts).await
        } else {
            Executor::start(opts).await
        }
    }

    async fn record(ctx: &Ctx) -> Res<()> {
        ctx.control
            .reset(&[])
            .map_err(|e| Fault::new("control", e))?;
        let mut ex = ctx
            .stop
            .or(start_for(ctx, &ctx.options("p-record")))
            .await?;
        let outcome = ctx
            .stop
            .or(async {
                ex.add_recorder().await?;
                signin(&mut ex, ctx).await?;
                let link = ex.resolve_one(&[role("link", "Pay bill")], &[]).await?;
                click_and_wait(&mut ex, &link).await?;
                let deadline = Instant::now() + Duration::from_secs(5);
                loop {
                    let events: Vec<serde_json::Value> = ex
                        .recorded()
                        .iter()
                        .filter_map(|p| serde_json::from_str(p).ok())
                        .collect();
                    let typed = events.iter().any(|e| {
                        e["kind"] == "input"
                            && e["role"] == "textbox"
                            && e["name"] == "Email"
                            && e["testid"] == "email"
                    });
                    let clicked = events.iter().any(|e| {
                        e["kind"] == "click"
                            && e["role"] == "link"
                            && e["name"] == "Pay bill"
                            && e["testid"] == "pay-bill"
                    });
                    if typed && clicked {
                        return Ok(());
                    }
                    if Instant::now() > deadline {
                        return Err(Fault::new("check", "recorded"));
                    }
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            })
            .await;
        ex.stop_and_remove().await;
        outcome
    }

    async fn extras(ctx: &Ctx, added_bytes: u64) -> bool {
        let mut colds = Vec::new();
        let mut faults = Vec::new();
        for i in 1..=10 {
            match cold_start(ctx, i).await {
                Ok(ms) => colds.push(ms),
                Err(f) => faults.push(("cold", f)),
            }
        }
        let rec = record(ctx).await;
        if let Err(f) = &rec {
            faults.push(("record", f.clone()));
        }
        for (part, f) in &faults {
            eprintln!("{part} cause={} detail={}", f.0, printable(&f.1));
        }
        let stopped = ctx.stop.is_set();
        let unmeasured =
            stopped || colds.len() < 10 || faults.iter().any(|(_, f)| f.0 == "control");
        if unmeasured {
            out(&format!(
                "notrun candidate={CANDIDATE} machine={} browser={} cell=extras cause={}",
                ctx.machine,
                ctx.kind,
                if stopped {
                    "signal"
                } else if colds.len() < 10 {
                    "cold_start"
                } else {
                    "fixture"
                }
            ));
            return false;
        }
        out(&format!(
            "extras candidate={CANDIDATE} form={} added_bytes={added_bytes} cold_ms={} record={}",
            if ctx.channel == Channel::Pipe {
                "pipe"
            } else {
                "port"
            },
            median(&mut colds),
            if rec.is_ok() { "ok" } else { "miss" }
        ));
        true
    }

    fn printable(detail: &str) -> String {
        let token = detail.replace(' ', "_");
        let safe = token.len() <= 64
            && token
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_:.-".contains(&b));
        if safe {
            token
        } else {
            format!("text_of_{}_bytes", detail.len())
        }
    }

    async fn resolver(ctx: &Ctx) -> Res<()> {
        ctx.control
            .reset(&[])
            .map_err(|e| Fault::new("control", e))?;
        let mut ex = ctx
            .stop
            .or(Executor::start(&ctx.options("p-resolver")))
            .await?;
        let outcome = ctx
            .stop
            .or(async {
                let t0 = counter(ctx, &["requests", "total"])?;
                let other = ex
                    .navigate_error(&format!(
                        "http://halvo-resolver-probe.localhost:{}/",
                        ctx.port
                    ))
                    .await?;
                let t1 = counter(ctx, &["requests", "total"])?;
                ex.open(&ctx.u("/signin")).await?;
                let t2 = counter(ctx, &["requests", "total"])?;
                out(&format!(
                    "resolver other_name_error={} other_name_requests={} test_name_requests={}",
                    other.unwrap_or("none".into()).replace(' ', "_"),
                    t1 - t0,
                    t2 - t1
                ));
                Ok(())
            })
            .await;
        ex.stop_and_remove().await;
        outcome
    }

    async fn snapshot_pay(ctx: &Ctx, path: &Path) -> Res<()> {
        ctx.control
            .reset(&[])
            .map_err(|e| Fault::new("control", e))?;
        let mut ex = ctx
            .stop
            .or(Executor::start(&ctx.options("p-snapshot")))
            .await?;
        let outcome = ctx
            .stop
            .or(async {
                signin(&mut ex, ctx).await?;
                ex.open(&ctx.u("/pay")).await?;
                let lines = ex.snapshot().await?;
                let mut text = String::new();
                for l in &lines {
                    text.push_str(&l.render());
                    text.push('\n');
                }
                std::fs::write(path, text).map_err(|e| Fault::new("file", e.to_string()))?;
                out(&format!("snapshot lines={}", lines.len()));
                Ok(())
            })
            .await;
        ex.stop_and_remove().await;
        outcome
    }

    const HELD_PATH: &str = "/held";
    const KILL_WATCH: Duration = Duration::from_secs(10);
    const CHILD_WAIT: Duration = Duration::from_secs(60);
    const CHILD_LIFE: Duration = Duration::from_secs(120);
    const CONTROL_WAIT: Duration = Duration::from_secs(15);

    struct Form {
        step: &'static str,
        channel: Channel,
        relay: bool,
    }

    const PORT_FREE_FORM: Form = Form {
        step: PORT_FREE_STEP,
        channel: Channel::Pipe,
        relay: true,
    };

    const FORMS: [Form; 4] = [
        Form {
            step: "driver_killed_pipe",
            channel: Channel::Pipe,
            relay: false,
        },
        Form {
            step: "driver_killed_port",
            channel: Channel::Port,
            relay: false,
        },
        Form {
            step: "driver_killed_pipe_relay",
            channel: Channel::Pipe,
            relay: true,
        },
        Form {
            step: "driver_killed_port_relay",
            channel: Channel::Port,
            relay: true,
        },
    ];

    struct ChildArgs {
        browser: PathBuf,
        profile: PathBuf,
        page: String,
        target: String,
        allow: Vec<String>,
        let_through: bool,
        channel: Channel,
        relay: bool,
        round2: bool,
    }

    fn child_args(argv: &[String]) -> Result<ChildArgs, String> {
        let mut browser = None;
        let mut profile = None;
        let mut page = None;
        let mut target = None;
        let mut allow = Vec::new();
        let mut let_through = None;
        let mut channel = None;
        let mut relay = None;
        let mut round2 = false;
        let mut i = 0;
        while i < argv.len() {
            let value = argv
                .get(i + 1)
                .cloned()
                .ok_or_else(|| format!("{} needs a value", argv[i]))?;
            match argv[i].as_str() {
                "--browser" => browser = Some(PathBuf::from(value)),
                "--profile" => profile = Some(PathBuf::from(value)),
                "--page" => page = Some(value),
                "--target" => target = Some(value),
                "--allow" => allow.push(value),
                "--let-through" => let_through = Some(value == "yes"),
                "--relay" => relay = Some(value == "yes"),
                "--round2" => round2 = value == "yes",
                "--channel" => {
                    channel = Some(match value.as_str() {
                        "pipe" => Channel::Pipe,
                        "port" => Channel::Port,
                        _ => return Err("--channel takes pipe or port".into()),
                    })
                }
                other => return Err(format!("unknown argument {other}")),
            }
            i += 2;
        }
        Ok(ChildArgs {
            browser: browser.ok_or("--browser is required")?,
            profile: profile.ok_or("--profile is required")?,
            page: page.ok_or("--page is required")?,
            target: target.ok_or("--target is required")?,
            allow,
            let_through: let_through.ok_or("--let-through is required")?,
            channel: channel.ok_or("--channel is required")?,
            relay: relay.ok_or("--relay is required")?,
            round2,
        })
    }

    fn held_script(target: &str) -> String {
        format!(
            "(() => {{ fetch({}, {{method: 'POST', body: 'halvo-held-request', mode: 'no-cors'}}).catch(() => {{}}); return true; }})()",
            serde_json::Value::String(target.to_string())
        )
    }

    pub fn child_main(argv: &[String]) -> ExitCode {
        let a = match child_args(argv) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("REFUSED usage: {e}");
                return ExitCode::from(2);
            }
        };
        let rt = match tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(_) => return ExitCode::from(1),
        };
        let mut flags: Vec<String> = FIRST_FLAGS.iter().map(|s| s.to_string()).collect();
        flags.push(RESOLVER_RULE.to_string());
        let relay = if a.relay {
            match rt.block_on(Relay::start()) {
                Ok(r) => {
                    out(&format!("relay_port {}", r.port));
                    flags.extend(relay_flags(r.port));
                    if a.round2 {
                        let list: Vec<HostPort> = [&a.page, &a.target]
                            .iter()
                            .filter_map(|u| r3_web::cdp::host_port(&r3_web::cdp::split_url(u).0))
                            .collect();
                        r.handle().set_list(Some(list));
                    }
                    Some(r)
                }
                Err(e) => {
                    eprintln!("driver relay {}", e.kind());
                    return ExitCode::from(1);
                }
            }
        } else {
            None
        };
        if a.round2 {
            flags.extend(ROUND2_FLAGS.iter().map(|f| f.to_string()));
        }
        let code = rt.block_on(port_child(&a, flags, relay.as_ref().map(Relay::handle)));
        drop(relay);
        code
    }

    async fn port_child(a: &ChildArgs, flags: Vec<String>, relay: Option<RelayHandle>) -> ExitCode {
        let opts = StartOptions {
            browser: a.browser.clone(),
            profile: a.profile.clone(),
            headless: true,
            flags,
            channel: a.channel,
            relay,
            local_state: None,
            program: None,
        };
        let started = if a.round2 {
            Executor::start_settled(&opts).await
        } else {
            Executor::start(&opts).await
        };
        let mut ex = match started {
            Ok(ex) => ex,
            Err(f) => {
                eprintln!("driver start cause={} detail={}", f.0, printable(&f.1));
                return ExitCode::from(1);
            }
        };
        out(&format!(
            "started browser_pid={} version={}",
            ex.pid().unwrap_or(0),
            ex.version()
                .await
                .unwrap_or_else(|_| "-".into())
                .replace(' ', "_")
        ));
        let held = async {
            ex.open(&a.page).await?;
            if a.let_through {
                ex.arm_rule(a.allow.clone(), true).await?;
            } else {
                ex.hold_requests(a.allow.clone()).await?;
            }
            ex.run_page_script(&held_script(&a.target)).await?;
            let deadline = Instant::now() + r3_web::cdp::STEP_TIMEOUT;
            loop {
                if let Some(p) = ex.paused().into_iter().find(|p| p.path == HELD_PATH) {
                    return Ok(p.outcome);
                }
                if Instant::now() > deadline {
                    return Err(Fault::new("timeout", "paused"));
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
        .await;
        match held {
            Ok(outcome) => {
                out(&format!("paused outcome={outcome:?}"));
                tokio::time::sleep(CHILD_LIFE).await;
                ex.stop_and_remove().await;
                ExitCode::from(3)
            }
            Err(f) => {
                eprintln!("driver step cause={} detail={}", f.0, printable(&f.1));
                ex.stop_and_remove().await;
                ExitCode::from(1)
            }
        }
    }

    struct Killed {
        bytes: u64,
        connections: u64,
        path_requests: u64,
        gone_ms: Option<u128>,
        free_ms: Option<u128>,
        watched: bool,
        version: String,
        left: bool,
        ended: checks::Ended,
        watch: Option<r3_web::killwatch::Watched>,
        ended_by: String,
    }

    fn of_the_run(
        tree: &[r3_web::procs::Proc],
        mark: &checks::Mark,
    ) -> Option<Vec<r3_web::procs::Proc>> {
        let table = r3_web::procs::table().ok()?;
        let mut out: Vec<r3_web::procs::Proc> = tree
            .iter()
            .filter(|p| r3_web::procs::running(p))
            .cloned()
            .collect();
        for p in &table {
            if !out.iter().any(|o| r3_web::procs::same(o, p))
                && mark.owns(p, &checks::command_args)
                && r3_web::procs::running(p)
            {
                out.push(p.clone());
            }
        }
        Some(out)
    }

    fn port_bindable(port: u16) -> bool {
        if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return true;
        }
        reuse_bind(port)
    }

    #[cfg(windows)]
    fn reuse_bind(port: u16) -> bool {
        use windows_sys::Win32::Networking::WinSock::{
            bind, closesocket, setsockopt, socket, AF_INET, INADDR_LOOPBACK, INVALID_SOCKET,
            IN_ADDR, IN_ADDR_0, IPPROTO_TCP, SOCKADDR, SOCKADDR_IN, SOCK_STREAM, SOL_SOCKET,
            SO_REUSEADDR,
        };
        // SAFETY: socket makes a new socket or returns INVALID_SOCKET; std's own bind above has
        // started the socket layer.
        let s = unsafe { socket(AF_INET as i32, SOCK_STREAM, IPPROTO_TCP) };
        if s == INVALID_SOCKET {
            return false;
        }
        let on: i32 = 1;
        let addr = SOCKADDR_IN {
            sin_family: AF_INET,
            sin_port: port.to_be(),
            sin_addr: IN_ADDR {
                S_un: IN_ADDR_0 {
                    S_addr: INADDR_LOOPBACK.to_be(),
                },
            },
            sin_zero: [0; 8],
        };
        // SAFETY: the option and the address are live values of the sizes given; the socket is
        // owned here and closed once.
        unsafe {
            setsockopt(
                s,
                SOL_SOCKET,
                SO_REUSEADDR,
                &on as *const i32 as *const u8,
                4,
            );
            let bound = bind(
                s,
                &addr as *const SOCKADDR_IN as *const SOCKADDR,
                std::mem::size_of::<SOCKADDR_IN>() as i32,
            ) == 0;
            closesocket(s);
            bound
        }
    }

    #[cfg(not(windows))]
    fn reuse_bind(_port: u16) -> bool {
        false
    }

    fn remove_made_dir(dir: &Path) -> bool {
        for _ in 0..50 {
            match std::fs::symlink_metadata(dir) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return true,
                Ok(m) if m.is_dir() => {
                    let _ = std::fs::remove_dir_all(dir);
                }
                _ => return false,
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    }

    async fn close_browser(root: &r3_web::procs::Proc) -> bool {
        r3_web::procs::kill(root);
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if !r3_web::procs::running(root) {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        false
    }

    async fn kill_once(
        ctx: &Ctx,
        form: &Form,
        i: u32,
        let_through: bool,
    ) -> Result<Killed, NotRun> {
        if ctx.stop.is_set() {
            return Err(NotRun::new("signal", ""));
        }
        ctx.control
            .reset(&[])
            .map_err(|e| NotRun::new("fixture", e))?;
        let program = ctx.repo.join("tools/r3-trials/web/record-listener.mjs");
        let mut listener =
            Listener::start_record(&program, HELD_PATH).map_err(|e| NotRun::new("listener", e))?;
        let target = format!("http://{UTILITY}:{}{HELD_PATH}", listener.port);
        let mut allow = vec![origin(UTILITY, ctx.port)];
        if let_through {
            allow.push(format!("http://{UTILITY}:{}", listener.port));
        }
        let profile = ctx.scratch.join(format!(
            "{}-p-{}-{}-{i}",
            std::process::id(),
            form.step,
            if let_through { "control" } else { "kill" }
        ));
        let mark = StartOptions {
            browser: ctx.browser.clone(),
            profile: profile.clone(),
            headless: true,
            flags: Vec::new(),
            channel: Channel::Pipe,
            relay: None,
            local_state: None,
            program: None,
        }
        .mark_now();
        let exe = std::env::current_exe().map_err(|e| NotRun::new("driver", e.to_string()))?;
        let mut cmd = std::process::Command::new(exe);
        cmd.arg("--driver-child")
            .arg("--browser")
            .arg(&ctx.browser)
            .arg("--profile")
            .arg(&profile)
            .arg("--page")
            .arg(ctx.u("/signin"))
            .arg("--target")
            .arg(&target)
            .arg("--let-through")
            .arg(if let_through { "yes" } else { "no" })
            .arg("--channel")
            .arg(if form.channel == Channel::Pipe {
                "pipe"
            } else {
                "port"
            })
            .arg("--relay")
            .arg(if form.relay { "yes" } else { "no" })
            .arg("--round2")
            .arg(if ctx.round2 { "yes" } else { "no" });
        for a in &allow {
            cmd.arg("--allow").arg(a);
        }
        let mut child = cmd
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .start()
            .map_err(|e| NotRun::new("driver", e.to_string()))?;
        let Some(stdout) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(NotRun::new("driver", "no stdout"));
        };
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        std::thread::spawn(move || {
            use std::io::BufRead;
            for line in std::io::BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let mut pid = None;
        let mut version = "-".to_string();
        let mut paused = false;
        let mut relay_port = None;
        let deadline = Instant::now() + CHILD_WAIT;
        while Instant::now() < deadline && !ctx.stop.is_set() {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(line) => {
                    if let Some(rest) = line.strip_prefix("started ") {
                        for pair in rest.split(' ') {
                            match pair.split_once('=') {
                                Some(("browser_pid", v)) => pid = v.parse::<u32>().ok(),
                                Some(("version", v)) => version = v.to_string(),
                                _ => {}
                            }
                        }
                    } else if let Some(p) = line.strip_prefix("relay_port ") {
                        relay_port = p.trim().parse::<u16>().ok();
                    } else if line.starts_with("paused ") {
                        paused = true;
                        break;
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        let root = pid.and_then(|p| {
            r3_web::procs::table()
                .ok()?
                .into_iter()
                .find(|q| q.pid == p)
        });
        if paused && let_through {
            let waited = Instant::now();
            let mut arrived = false;
            while waited.elapsed() < CONTROL_WAIT && !ctx.stop.is_set() {
                if matches!(listener.record_counts(), Ok(c) if c.path_requests > 0) {
                    arrived = true;
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            eprintln!(
                "control_wait step={} arrived_before_kill={} ms={}",
                form.step,
                if arrived { "yes" } else { "no" },
                waited.elapsed().as_millis()
            );
        }
        let tree: Vec<r3_web::procs::Proc> = match (&root, r3_web::procs::table()) {
            (Some(r), Ok(t)) => r3_web::checks::tree_of(&t, r),
            _ => Vec::new(),
        };
        let watch_port = if form.step == PORT_FREE_STEP {
            relay_port
        } else {
            None
        };
        let t_kill = Instant::now();
        let _ = child.kill();
        let _ = child.wait();
        let mut gone_ms = None;
        let mut free_ms = None;
        let mut watch = None;
        if let (true, Some(port)) = (paused, watch_port) {
            use r3_web::killwatch;
            let first = root.clone();
            let w = killwatch::watch(
                killwatch::Readers {
                    running: &mut || of_the_run(&tree, &mark).map(|r| !r.is_empty()),
                    free: &mut || port_bindable(port),
                    now_ms: &mut || t_kill.elapsed().as_millis(),
                    pause: &mut || std::thread::sleep(killwatch::PASS_WAIT),
                    report: &mut |at| {
                        let now = of_the_run(&tree, &mark);
                        let names: Vec<String> = now
                            .iter()
                            .flatten()
                            .filter_map(|p| {
                                p.exe
                                    .as_ref()?
                                    .file_name()
                                    .map(|n| n.to_string_lossy().replace(' ', "_"))
                            })
                            .collect();
                        eprintln!(
                            "watch_at step={} i={i} ms={at} running={} programs={} first={}",
                            form.step,
                            now.as_ref()
                                .map_or("unread".to_string(), |r| r.len().to_string()),
                            if names.is_empty() {
                                "-".into()
                            } else {
                                names.join(",")
                            },
                            match (&now, &first) {
                                (Some(r), Some(f))
                                    if r.iter().any(|p| r3_web::procs::same(p, f)) =>
                                    "yes",
                                (None, _) => "unread",
                                _ => "no",
                            }
                        );
                    },
                },
                killwatch::CAP,
                killwatch::REPORT_AT,
            );
            gone_ms = w.gone_ms;
            free_ms = w.free_ms;
            watch = Some(w);
        } else if paused {
            while t_kill.elapsed() < KILL_WATCH {
                if gone_ms.is_none() {
                    if let Some(r) = &root {
                        if !r3_web::procs::running(r) {
                            gone_ms = Some(t_kill.elapsed().as_millis());
                        }
                    }
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }
        let counts = listener.record_counts();
        listener.stop();
        let mut left = false;
        if let Some(r) = &root {
            if r3_web::procs::running(r) {
                left = !close_browser(r).await;
            }
        }
        let ended = checks::end_runs_own(&mark);
        let mut temp = profile.clone().into_os_string();
        temp.push("-tmp");
        let ended_by =
            match std::fs::read_to_string(Path::new(&temp).join(r3_web::cdp::GUARD_RECORD)) {
                Ok(r) => {
                    let key = |k: &str| {
                        r.split_whitespace()
                            .find_map(|w| w.strip_prefix(k))
                            .unwrap_or("-")
                            .to_string()
                    };
                    if key("first=") == "yes" {
                        format!("guard ended_ms={}", key("ms="))
                    } else {
                        format!("self guard_ended={}", key("programs="))
                    }
                }
                Err(_) if gone_ms.is_some() => "self".to_string(),
                Err(_) => "row".to_string(),
            };
        let removed = remove_made_dir(&profile) && remove_made_dir(Path::new(&temp));
        if !removed {
            eprintln!("driver step={} i={i} profile_removed=no", form.step);
        }
        if !paused {
            return Err(NotRun::new("driver", "no paused line"));
        }
        let c = counts.map_err(|e| NotRun::new("listener", e))?;
        Ok(Killed {
            bytes: c.bytes,
            connections: c.connections,
            path_requests: c.path_requests,
            gone_ms,
            free_ms,
            watched: watch_port.is_some(),
            version,
            left,
            ended,
            watch,
            ended_by,
        })
    }

    async fn port_free_form(ctx: &Ctx, form: &Form, runs: u32) -> bool {
        let head = format!(
            "candidate={CANDIDATE} machine={} browser={}",
            ctx.machine, ctx.kind
        );
        let name = form.step;
        let mut ok = 0;
        let mut misses = 0;
        let mut opens = 0;
        let mut gone = Vec::new();
        let mut majors = Vec::new();
        for i in 1..=runs {
            let k = match kill_once(ctx, form, i, false).await {
                Ok(k) => k,
                Err(e) => {
                    out(&format!("notrun {head} step={name} cause={}", e.cause));
                    return false;
                }
            };
            let outcome = match &k.watch {
                Some(w) => r3_web::killwatch::decide(&w.passes, k.ended.before, k.ended.after),
                None => r3_web::killwatch::Outcome::Miss("unwatched"),
            };
            match outcome {
                r3_web::killwatch::Outcome::Ok => ok += 1,
                r3_web::killwatch::Outcome::Miss(_) => misses += 1,
                r3_web::killwatch::Outcome::Open(_) => opens += 1,
            }
            if let Some(g) = k.gone_ms {
                gone.push(g);
            }
            if let Some(m) = major_of(&k.version) {
                majors.push(m);
            }
            out(&format!(
                "run {head} step={name} i={i} result={} cause={} watched={} gone_ms={} free_ms={} max_gap_ms={} version={} left={} left_before={} left_after={} ended_by={}",
                outcome.result(),
                outcome.cause(),
                if k.watched { "yes" } else { "no" },
                k.gone_ms.map(|v| v.to_string()).unwrap_or("-".into()),
                k.free_ms.map(|v| v.to_string()).unwrap_or("-".into()),
                k.watch.as_ref().map_or("-".into(), |w| w.max_gap_ms.to_string()),
                k.version,
                if k.left { "yes" } else { "no" },
                k.ended.before,
                k.ended.after,
                k.ended_by
            ));
        }
        majors.sort_unstable();
        let majors_key = r3_web::majors_key(&majors);
        majors.dedup();
        let Some(major) = majors.first() else {
            out(&format!("notrun {head} step={name} cause=version"));
            return false;
        };
        if let Some(cause) = r3_web::killwatch::call_notrun(misses, opens) {
            out(&format!(
                "notrun {head} step={name} cause={cause} ok={ok} open={opens} of={runs}"
            ));
            return false;
        }
        out(&format!(
            "cell {head} major={major}{majors_key} step={name} ok={ok} of={runs} median_ms={} relay=pipe_relay",
            median(&mut gone)
        ));
        true
    }

    async fn driver_form(ctx: &Ctx, form: &Form, runs: u32) -> bool {
        let name = form.step;
        let head = format!(
            "candidate={CANDIDATE} machine={} browser={}",
            ctx.machine, ctx.kind
        );
        let control = match kill_once(ctx, form, 0, true).await {
            Ok(k) => k,
            Err(e) => {
                out(&format!("notrun {head} step={name} cause={}", e.cause));
                return false;
            }
        };
        let seen = control.bytes > 0 && control.path_requests > 0;
        out(&format!(
            "control {head} step={name} let_through=yes browser_after={} connections={} bytes={} seen={}",
            if control.gone_ms.is_some() { "gone" } else { "alive" },
            control.connections,
            control.bytes,
            if seen { "yes" } else { "no" }
        ));
        if !seen {
            out(&format!("notrun {head} step={name} cause=control"));
            return false;
        }
        let mut ok = 0;
        let mut bytes = 0;
        let mut gone = Vec::new();
        let mut majors = Vec::new();
        for i in 1..=runs {
            let k = match kill_once(ctx, form, i, false).await {
                Ok(k) => k,
                Err(e) => {
                    out(&format!("notrun {head} step={name} cause={}", e.cause));
                    return false;
                }
            };
            let clean = k.bytes == 0 && k.ended.after == 0;
            if clean {
                ok += 1;
            }
            bytes += k.bytes;
            if let Some(ms) = k.gone_ms {
                gone.push(ms);
            }
            if let Some(m) = major_of(&k.version) {
                majors.push(m);
            }
            out(&format!(
                "run {head} step={name} i={i} result={} ms={} version={} browser_after={} connections={} bytes={} left={} left_before={} left_after={}",
                if clean { "ok" } else { "miss" },
                k.gone_ms.unwrap_or(0),
                k.version,
                if k.gone_ms.is_some() { "gone" } else { "alive" },
                k.connections,
                k.bytes,
                if k.left { "yes" } else { "no" },
                k.ended.before,
                k.ended.after
            ));
        }
        majors.sort_unstable();
        let majors_key = r3_web::majors_key(&majors);
        majors.dedup();
        let relay_key = if name.ends_with("_relay") {
            format!(" relay={}", name.trim_start_matches("driver_killed_"))
        } else {
            String::new()
        };
        let Some(major) = majors.first() else {
            out(&format!("notrun {head} step={name} cause=version"));
            return false;
        };
        let miss_bytes = if ok < runs {
            format!(" bytes={bytes}")
        } else {
            String::new()
        };
        out(&r3_web::kill_line(
            &ctx.machine,
            ctx.kind,
            name.trim_start_matches("driver_killed_"),
            runs,
            runs - ok,
        ));
        out(&format!(
            "cell {head} major={major}{majors_key} step={name} ok={ok} of={runs} median_ms={}{miss_bytes}{relay_key}",
            median(&mut gone)
        ));
        true
    }

    async fn driver_cells(ctx: &Ctx, step: Option<&str>, runs: u32) -> bool {
        let mut all = true;
        for form in FORMS.iter().filter(|f| step.is_none_or(|s| s == f.step)) {
            if ctx.stop.is_set() {
                return false;
            }
            all &= driver_form(ctx, form, runs).await;
        }
        if ctx.round3 && step.is_none_or(|s| s == PORT_FREE_STEP) && !ctx.stop.is_set() {
            all &= port_free_form(ctx, &PORT_FREE_FORM, runs.min(10)).await;
        }
        all
    }

    pub async fn main(args: Args, kind: &'static str) -> ExitCode {
        let repo = args.repo.clone().unwrap_or_default();
        let port = args.port.unwrap_or_default();
        let machine = args.machine.clone().unwrap_or_default();
        let stop = match watch_signals() {
            Ok(s) => s,
            Err(e) => {
                eprintln!("signals: {e}");
                out(&format!(
                    "notrun candidate={CANDIDATE} machine={machine} browser={kind} cause=runtime"
                ));
                return ExitCode::from(1);
            }
        };
        let seed = match Seed::load(&repo, 1) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("seed: {e}");
                out(&format!(
                    "notrun candidate={CANDIDATE} machine={machine} browser={kind} cause=seed"
                ));
                return ExitCode::from(1);
            }
        };
        let upstream = if args.fault.is_some() { port + 2 } else { port };
        let server = match Server::start(
            &fixture_program(&repo),
            &["--port".into(), upstream.to_string()],
        ) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("fixture: {e}");
                out(&format!(
                    "notrun candidate={CANDIDATE} machine={machine} browser={kind} cause=fixture"
                ));
                return ExitCode::from(1);
            }
        };
        let proxy = match &args.fault {
            None => None,
            Some(fault) => {
                let program = repo.join("tools/r3-trials/web/faults.mjs");
                let a = [
                    "--listen".to_string(),
                    port.to_string(),
                    "--upstream".into(),
                    upstream.to_string(),
                    "--fault".into(),
                    fault.clone(),
                ];
                match Server::start(&program, &a) {
                    Ok(p) => Some(p),
                    Err(e) => {
                        eprintln!("faults: {e}");
                        server.stop();
                        out(&format!("notrun candidate={CANDIDATE} machine={machine} browser={kind} cause=faults"));
                        return ExitCode::from(1);
                    }
                }
            }
        };
        let mut ctx = Ctx {
            browser: PathBuf::from(args.browser.clone().unwrap_or_default()),
            kind,
            machine: machine.clone(),
            port,
            control: Control { port: upstream },
            seed,
            scratch: args.scratch.clone().unwrap_or_default(),
            headless: !args.headed,
            stop,
            added_flags: args.add_flags.clone(),
            routes: args.routes,
            relayed: args.relay,
            round2: args.round2,
            relay: None,
            relay_port: 0,
            net_log: args.net_log.clone(),
            repo: repo.clone(),
            round3: args.round3,
            probe_before: std::cell::Cell::new(0),
            siblings: std::cell::RefCell::new(Vec::new()),
            channel: if args.round3 && !args.port_channel {
                Channel::Pipe
            } else {
                Channel::Port
            },
        };
        let relay = if args.relay {
            match Relay::start().await {
                Ok(r) => {
                    ctx.added_flags.extend(relay_flags(r.port));
                    ctx.relay = Some(r.handle());
                    ctx.relay_port = r.port;
                    Some(r)
                }
                Err(e) => {
                    eprintln!("relay {}", e.kind());
                    out(&format!(
                        "notrun candidate={CANDIDATE} machine={machine} browser={kind} cause=relay"
                    ));
                    server.stop();
                    return ExitCode::from(1);
                }
            }
        } else {
            None
        };
        if args.round2 {
            ctx.added_flags
                .extend(ROUND2_FLAGS.iter().map(|f| f.to_string()));
        }
        out(&format!(
            "start candidate={CANDIDATE} machine={machine} browser={kind} fixture_pid={} flags={} added_flags={} rows={} round={} headless={} channel={}",
            server.pid(),
            FIRST_FLAGS.len(),
            ctx.added_flags.len(),
            if args.round3_rows {
                "round3"
            } else if args.org_rows {
                "org"
            } else if args.reach_rows {
                "reach"
            } else if args.egress_rows {
                "egress"
            } else if args.way_rows {
                "ways"
            } else if args.relay_rows {
                "relay"
            } else if args.driver {
                "driver"
            } else if args.isolation {
                "isolation"
            } else {
                "nine"
            },
            if args.round3 {
                3
            } else if args.round2 {
                2
            } else {
                1
            },
            if ctx.headless { "yes" } else { "no" },
            if ctx.channel == Channel::Pipe { "pipe" } else { "port" }
        ));
        let code = match args.mode {
            Mode::Cells if args.driver => {
                if driver_cells(&ctx, args.step.as_deref(), args.runs.unwrap_or(20)).await {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::from(1)
                }
            }
            Mode::Cells => {
                let steps: Vec<&str> = match &args.step {
                    Some(s) => vec![s.as_str()],
                    None if args.round3_rows => ROUND3_STEPS.to_vec(),
                    None if args.org_rows => ORG_STEPS.to_vec(),
                    None if args.reach_rows => REACH_STEPS.to_vec(),
                    None if args.egress_rows => EGRESS_STEPS.to_vec(),
                    None if args.way_rows => WAY_STEPS.to_vec(),
                    None if args.relay_rows => RELAY_STEPS.to_vec(),
                    None if args.isolation => ISOLATION_STEPS.to_vec(),
                    None => STEPS.to_vec(),
                };
                if cells(&ctx, &steps, args.runs.unwrap_or(10)).await {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::from(1)
                }
            }
            Mode::Extras => {
                if extras(&ctx, args.added_bytes.unwrap_or_default()).await {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::from(1)
                }
            }
            Mode::Resolver => match resolver(&ctx).await {
                Ok(()) => ExitCode::SUCCESS,
                Err(f) => {
                    eprintln!("resolver cause={} detail={}", f.0, f.1);
                    let cause = if f.0 == "signal" {
                        "signal"
                    } else {
                        "resolver"
                    };
                    out(&format!("notrun candidate={CANDIDATE} machine={machine} browser={kind} cause={cause}"));
                    ExitCode::from(1)
                }
            },
            Mode::SnapshotPay => {
                match snapshot_pay(&ctx, args.out.as_deref().unwrap_or(Path::new("-"))).await {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(f) => {
                        eprintln!("snapshot cause={} detail={}", f.0, f.1);
                        let cause = if f.0 == "signal" {
                            "signal"
                        } else {
                            "snapshot"
                        };
                        out(&format!("notrun candidate={CANDIDATE} machine={machine} browser={kind} cause={cause}"));
                        ExitCode::from(1)
                    }
                }
            }
        };
        if let Some(p) = proxy {
            p.stop();
        }
        if let Some(r) = relay {
            out(&format!(
                "relay tunnels={} refused={}",
                r.tunnels(),
                r.refused()
            ));
            r.stop();
        }
        server.stop();
        out("stop fixture=stopped");
        code
    }
}

fn out(line: &str) {
    let mut stdout = std::io::stdout().lock();
    let _ = writeln!(stdout, "{line}");
    let _ = stdout.flush();
}
