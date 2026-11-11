use std::io::Write;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::ExitCode;

use r3_web::{browser_kind, CANDIDATE, STEPS};

#[derive(Default)]
struct Args {
    browser: Option<String>,
    port: Option<u16>,
    runs: Option<u32>,
    step: Option<String>,
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
            "--step" => {
                let s = take()?;
                if !STEPS.contains(&s.as_str()) {
                    return Err(format!("--step takes one of {}", STEPS.join(",")));
                }
                a.step = Some(s);
            }
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
        _ => vec![port],
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

    use r3_web::cdp::{Executor, Fault, Locator, Res, Resolved, SnapshotLine, StartOptions};
    use r3_web::fixture::{count, fixture_program, origin, Control, Seed, Server, PAY, UTILITY};
    use r3_web::{
        extract_amount, major_of, median, variants_of, CANDIDATE, FIRST_FLAGS, RESOLVER_RULE, STEPS,
    };

    use super::{out, Args, Mode};

    pub struct Ctx {
        browser: PathBuf,
        kind: &'static str,
        machine: String,
        port: u16,
        control: Control,
        seed: Seed,
        scratch: PathBuf,
        headless: bool,
    }

    fn flags() -> Vec<String> {
        let mut f: Vec<String> = FIRST_FLAGS.iter().map(|s| s.to_string()).collect();
        f.push(RESOLVER_RULE.to_string());
        f
    }

    impl Ctx {
        fn options(&self, profile: &str, debug_port: Option<u16>) -> StartOptions {
            StartOptions {
                browser: self.browser.clone(),
                profile: self.scratch.join(profile),
                headless: self.headless,
                flags: flags(),
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
        let before = count(&state(ctx)?, &["utility", "signins"]);
        signin(ex, ctx).await?;
        let after = count(&state(ctx)?, &["utility", "signins"]);
        check(after == before + 1, "signins")?;
        check(path_now(ex).await? == "/bill", "page")
    }

    async fn table_read(ex: &mut Executor, ctx: &Ctx) -> Res<()> {
        signin(ex, ctx).await?;
        check(path_now(ex).await? == "/bill", "page")?;
        let lines = ex.snapshot().await?;
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

    async fn form_fill(ex: &mut Executor, ctx: &Ctx) -> Res<()> {
        signin(ex, ctx).await?;
        ex.open(&ctx.u("/prefs")).await?;
        let before = count(&state(ctx)?, &["utility", "prefs_saved"]);
        let boxed = ex
            .resolve_one(&[role("checkbox", "Paperless billing")], &[])
            .await?;
        ex.click(&boxed).await?;
        let save = ex
            .resolve_one(&[role("button", "Save preferences")], &[])
            .await?;
        click_and_wait(ex, &save).await?;
        let after = count(&state(ctx)?, &["utility", "prefs_saved"]);
        check(after == before + 1, "prefs_saved")
    }

    fn payments(ctx: &Ctx) -> Res<Vec<serde_json::Value>> {
        Ok(state(ctx)?["utility"]["payments"]
            .as_array()
            .cloned()
            .unwrap_or_default())
    }

    fn paid_once_more(ctx: &Ctx, before: usize) -> Res<()> {
        let after = payments(ctx)?;
        check(after.len() == before + 1, "payments")?;
        let amount = after.last().map(|p| p["amount"].clone());
        check(
            amount == Some(ctx.seed.0["utility"]["amount_due"].clone()),
            "amount",
        )
    }

    async fn payment_frame(ex: &mut Executor, ctx: &Ctx) -> Res<()> {
        signin(ex, ctx).await?;
        ex.open(&ctx.u("/pay")).await?;
        let before = payments(ctx)?.len();
        let lines = ex.snapshot().await?;
        let combo = lines
            .iter()
            .position(|l| l.role == "combobox" && l.name == "Payment method")
            .ok_or_else(|| Fault::new("resolve", "combobox"))?;
        let first = lines[combo + 1..]
            .iter()
            .find(|l| l.role == "option")
            .map(|l| l.name.clone())
            .ok_or_else(|| Fault::new("resolve", "option"))?;
        let method = ex
            .resolve_one(&[role("combobox", "Payment method")], &["Payment"])
            .await?;
        ex.select(&method, &first).await?;
        let pay = ex
            .resolve_one(&[role("button", "Pay now")], &["Payment"])
            .await?;
        click_and_wait(ex, &pay).await?;
        let heading = ex
            .resolve(&[role("heading", "Payment received")], &["Payment"])
            .await?;
        check(matches!(heading, Resolved::One { .. }), "heading")?;
        paid_once_more(ctx, before)
    }

    async fn file_upload(ex: &mut Executor, ctx: &Ctx, i: u32) -> Res<()> {
        signin(ex, ctx).await?;
        ex.open(&ctx.u("/upload")).await?;
        let file = ctx.scratch.join(format!("meter-{i}.bin"));
        std::fs::write(&file, vec![0x5a_u8; 1024])
            .map_err(|e| Fault::new("file", e.to_string()))?;
        let before = count(&state(ctx)?, &["utility", "uploads"]);
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
        let send = ex.resolve_one(&[role("button", "Upload")], &[]).await?;
        click_and_wait(ex, &send).await?;
        let after = count(&state(ctx)?, &["utility", "uploads"]);
        check(after == before + 1, "uploads")
    }

    async fn session_expiry(ex: &mut Executor, ctx: &Ctx) -> Res<()> {
        signin(ex, ctx).await?;
        let signins = count(&state(ctx)?, &["utility", "signins"]);
        check(signins == 1, "first_signin")?;
        ex.open(&ctx.u("/prefs")).await?;
        let save = ex
            .resolve_one(&[role("button", "Save preferences")], &[])
            .await?;
        click_and_wait(ex, &save).await?;
        check(
            count(&state(ctx)?, &["utility", "prefs_saved"]) == 1,
            "prefs_saved",
        )?;
        ex.open(&ctx.u("/bill")).await?;
        let lines = ex.snapshot().await?;
        check(lines.iter().any(|l| l.credential), "credential_field")?;
        check(count(&state(ctx)?, &["utility", "signins"]) == 1, "signins")
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
        paid_once_more(ctx, before)
    }

    async fn block_request(ex: &mut Executor, ctx: &Ctx) -> Res<()> {
        signin(ex, ctx).await?;
        ex.open(&ctx.u("/prefs")).await?;
        let u = origin(UTILITY, ctx.port);
        ex.arm_rule(vec![u.clone(), origin(PAY, ctx.port)], false)
            .await?;
        let before = count(&state(ctx)?, &["requests", "state_changing"]);
        let save = ex
            .resolve_one(&[role("button", "Save preferences")], &[])
            .await?;
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
        tokio::time::sleep(Duration::from_millis(300)).await;
        let after = count(&state(ctx)?, &["requests", "state_changing"]);
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

    async fn step(ex: &mut Executor, ctx: &Ctx, name: &str, i: u32) -> Res<()> {
        match name {
            "signin_form" => signin_form(ex, ctx).await,
            "table_read" => table_read(ex, ctx).await,
            "form_fill" => form_fill(ex, ctx).await,
            "payment_frame" => payment_frame(ex, ctx).await,
            "file_upload" => file_upload(ex, ctx, i).await,
            "session_expiry" => session_expiry(ex, ctx).await,
            "changed_page_1" => changed_page_1(ex, ctx).await,
            "block_request" => block_request(ex, ctx).await,
            "credential_mark" => credential_mark(ex, ctx).await,
            _ => Err(Fault::new("usage", name.to_string())),
        }
    }

    struct Run {
        ok: bool,
        cause: String,
        ms: u128,
        version: String,
    }

    async fn run_once(ctx: &Ctx, name: &str, i: u32) -> Result<Run, String> {
        ctx.control.reset(variants_of(name))?;
        let mut ex = match Executor::start(&ctx.options(&format!("p-{name}-{i}"), None)).await {
            Ok(ex) => ex,
            Err(f) => {
                return Ok(Run {
                    ok: false,
                    cause: f.0.to_string(),
                    ms: 0,
                    version: "-".into(),
                })
            }
        };
        let version = ex.version().await.unwrap_or_else(|_| "-".into());
        let t0 = Instant::now();
        let outcome = step(&mut ex, ctx, name, i).await;
        let ms = t0.elapsed().as_millis();
        if let Err(f) = &outcome {
            eprintln!("miss step={name} i={i} cause={} detail={}", f.0, f.1);
        }
        ex.stop().await;
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
                        }
                        times.push(r.ms);
                        if let Some(m) = major_of(&r.version) {
                            majors.push(m);
                        }
                    }
                    Err(e) => {
                        eprintln!("control: {e}");
                        out(&format!(
                            "notrun candidate={CANDIDATE} machine={} browser={} step={name} cause=fixture",
                            ctx.machine, ctx.kind
                        ));
                        ran = false;
                        break;
                    }
                }
            }
            if !ran {
                all_ran = false;
                continue;
            }
            majors.sort_unstable();
            majors.dedup();
            let major = majors.first().map(|m| m.to_string()).unwrap_or("-".into());
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
        let mut ex = Executor::start(&ctx.options(&format!("p-cold-{i}"), None)).await?;
        let opened = ex.open(&ctx.u("/signin")).await;
        let ms = t0.elapsed().as_millis();
        ex.stop().await;
        opened.map(|_| ms)
    }

    async fn record(ctx: &Ctx) -> Res<()> {
        ctx.control
            .reset(&[])
            .map_err(|e| Fault::new("control", e))?;
        let mut ex = Executor::start(&ctx.options("p-record", None)).await?;
        let outcome = async {
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
        }
        .await;
        ex.stop().await;
        outcome
    }

    async fn attach(ctx: &Ctx) -> Res<()> {
        ctx.control
            .reset(&[])
            .map_err(|e| Fault::new("control", e))?;
        let debug = ctx.port + 1;
        let mut ex = Executor::start(&ctx.options("p-attach", Some(debug))).await?;
        let outcome = async {
            signin(&mut ex, ctx).await?;
            let mut second =
                Executor::connect(&format!("127.0.0.1:{debug}"), &ctx.u("/bill")).await?;
            let heading = second.resolve(&[role("heading", "Your bill")], &[]).await;
            second.stop().await;
            check(matches!(heading?, Resolved::One { .. }), "heading")
        }
        .await;
        ex.stop().await;
        outcome
    }

    async fn extras(ctx: &Ctx, added_bytes: u64) {
        let mut colds = Vec::new();
        for i in 1..=10 {
            match cold_start(ctx, i).await {
                Ok(ms) => colds.push(ms),
                Err(f) => eprintln!("cold i={i} cause={} detail={}", f.0, f.1),
            }
        }
        let rec = record(ctx).await;
        if let Err(f) = &rec {
            eprintln!("record cause={} detail={}", f.0, f.1);
        }
        let att = attach(ctx).await;
        if let Err(f) = &att {
            eprintln!("attach cause={} detail={}", f.0, f.1);
        }
        out(&format!(
            "extras candidate={CANDIDATE} added_bytes={added_bytes} cold_ms={} record={} attach={} pipe=no",
            if colds.len() == 10 { median(&mut colds).to_string() } else { "-".into() },
            if rec.is_ok() { "ok" } else { "miss" },
            if att.is_ok() { "ok" } else { "miss" }
        ));
    }

    async fn resolver(ctx: &Ctx) -> Res<()> {
        ctx.control
            .reset(&[])
            .map_err(|e| Fault::new("control", e))?;
        let mut ex = Executor::start(&ctx.options("p-resolver", None)).await?;
        let outcome = async {
            let t0 = count(&state(ctx)?, &["requests", "total"]);
            let other = ex
                .navigate_error(&format!(
                    "http://halvo-resolver-probe.localhost:{}/",
                    ctx.port
                ))
                .await?;
            let t1 = count(&state(ctx)?, &["requests", "total"]);
            ex.open(&ctx.u("/signin")).await?;
            let t2 = count(&state(ctx)?, &["requests", "total"]);
            out(&format!(
                "resolver other_name_error={} other_name_requests={} test_name_requests={}",
                other.unwrap_or("none".into()).replace(' ', "_"),
                t1 - t0,
                t2 - t1
            ));
            Ok(())
        }
        .await;
        ex.stop().await;
        outcome
    }

    async fn snapshot_pay(ctx: &Ctx, path: &Path) -> Res<()> {
        ctx.control
            .reset(&[])
            .map_err(|e| Fault::new("control", e))?;
        let mut ex = Executor::start(&ctx.options("p-snapshot", None)).await?;
        let outcome = async {
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
        }
        .await;
        ex.stop().await;
        outcome
    }

    pub async fn main(args: Args, kind: &'static str) -> ExitCode {
        let repo = args.repo.clone().unwrap_or_default();
        let port = args.port.unwrap_or_default();
        let machine = args.machine.clone().unwrap_or_default();
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
        };
        out(&format!(
            "start candidate={CANDIDATE} machine={machine} browser={kind} fixture_pid={} flags={} headless={}",
            server.pid(),
            FIRST_FLAGS.len(),
            if ctx.headless { "yes" } else { "no" }
        ));
        let code = match args.mode {
            Mode::Cells => {
                let steps: Vec<&str> = match &args.step {
                    Some(s) => vec![s.as_str()],
                    None => STEPS.to_vec(),
                };
                if cells(&ctx, &steps, args.runs.unwrap_or(10)).await {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::from(1)
                }
            }
            Mode::Extras => {
                extras(&ctx, args.added_bytes.unwrap_or_default()).await;
                ExitCode::SUCCESS
            }
            Mode::Resolver => match resolver(&ctx).await {
                Ok(()) => ExitCode::SUCCESS,
                Err(f) => {
                    eprintln!("resolver cause={} detail={}", f.0, f.1);
                    out(&format!("notrun candidate={CANDIDATE} machine={machine} browser={kind} cause=resolver"));
                    ExitCode::from(1)
                }
            },
            Mode::SnapshotPay => {
                match snapshot_pay(&ctx, args.out.as_deref().unwrap_or(Path::new("-"))).await {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(f) => {
                        eprintln!("snapshot cause={} detail={}", f.0, f.1);
                        out(&format!("notrun candidate={CANDIDATE} machine={machine} browser={kind} cause=snapshot"));
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
