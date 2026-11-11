use std::io::Write;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::ExitCode;

use r3_web::{browser_kind, CANDIDATE, ISOLATION_STEPS, STEPS};

#[derive(Default)]
struct Args {
    browser: Option<String>,
    port: Option<u16>,
    runs: Option<u32>,
    step: Option<String>,
    isolation: bool,
    add_flags: Vec<String>,
    machine: Option<String>,
    repo: Option<PathBuf>,
    scratch: Option<PathBuf>,
    fault: Option<String>,
    mode: Mode,
    added_bytes: Option<u64>,
    out: Option<PathBuf>,
    headed: bool,
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
                if take()? != "isolation" {
                    return Err("--rows takes isolation".into());
                }
                a.isolation = true;
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
    let rows: &[&str] = if a.isolation {
        &ISOLATION_STEPS
    } else {
        &STEPS
    };
    if let Some(s) = &a.step {
        if !rows.contains(&s.as_str()) {
            return Err(format!("--step takes one of {}", rows.join(",")));
        }
    }
    if a.isolation && (a.mode != Mode::Cells || a.fault.is_some()) {
        return Err("--rows isolation runs cells with no fault".into());
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

    use r3_web::cdp::host_of;
    use r3_web::cdp::{
        Executor, Fault, Locator, Outcome, PausedRequest, Res, Resolved, SnapshotLine, StartOptions,
    };
    use r3_web::fixture::{count, fixture_program, origin, Control, Seed, Server, PAY, UTILITY};
    use r3_web::tls::{make_certificate, openssl_runs, CertFault, TlsListener};
    use r3_web::{
        extract_amount, major_of, median, variants_of, CANDIDATE, FIRST_FLAGS, ISOLATION_STEPS,
        RESOLVER_RULE, STEPS,
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
            TlsListener::start(&program, &cert, &key).map_err(|e| Fault::new("listener", e))?;
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
            _ => Err(Fault::new("usage", name.to_string())),
        }
    }

    struct Run {
        ok: bool,
        cause: String,
        ms: u128,
        version: String,
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
        let started = ctx
            .stop
            .or(Executor::start(
                &ctx.options(&format!("p-{name}-{i}"), None),
            ))
            .await;
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
            out(&format!(
                "cell candidate={CANDIDATE} machine={} browser={} major={major} step={name} ok={ok} of={runs} median_ms={}",
                ctx.machine,
                ctx.kind,
                median(&mut times)
            ));
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
        let ctx = Ctx {
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
            repo: repo.clone(),
        };
        out(&format!(
            "start candidate={CANDIDATE} machine={machine} browser={kind} fixture_pid={} flags={} added_flags={} rows={} headless={}",
            server.pid(),
            FIRST_FLAGS.len(),
            ctx.added_flags.len(),
            if args.isolation { "isolation" } else { "nine" },
            if ctx.headless { "yes" } else { "no" }
        ));
        let code = match args.mode {
            Mode::Cells => {
                let steps: Vec<&str> = match &args.step {
                    Some(s) => vec![s.as_str()],
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
