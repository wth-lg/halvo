//! `web-probe --browser PATH --port N [--headed]`: the nine capability steps of A13.2 against the
//! fixture server that the caller started on port N, each in a browser of its own started by the
//! executor on its pipe through the relay, which lets through the fixture's two sites alone. Prints
//! `probe step=<name> result=<ok|miss>` for each step and `probe ok=<n> of=9`; exit 0 when 9.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use halvo_web::cdp::{
    split_url, Executor, Fault, Locator, Mode, Outcome, PausedRequest, Res, Resolved, SnapshotLine,
    StartOptions,
};
use halvo_web::relay::{Names, Relay, RelayHandle};
use serde_json::Value;

const STEPS: [&str; 9] = [
    "signin_form",
    "table_read",
    "form_fill",
    "payment_frame",
    "file_upload",
    "session_expiry",
    "changed_page_1",
    "block_request",
    "credential_mark",
];

const UTILITY: &str = "utility.agent.test";
const PAY: &str = "pay.agent.test";
const CONTROL: &str = "control.agent.test";
const SEED: &str = include_str!("../../../../tools/fixture-server/variants/data/seed-1.json");

fn variants_of(step: &str) -> &'static [&'static str] {
    match step {
        "session_expiry" => &["session_expiry"],
        "changed_page_1" => &["changed_page_1"],
        "credential_mark" => &["second_factor", "captcha"],
        _ => &[],
    }
}

fn origin(host: &str, port: u16) -> String {
    format!("http://{host}:{port}")
}

struct Control {
    port: u16,
}

impl Control {
    fn json(&self, method: &str, target: &str) -> Res<Value> {
        let fault = |e: String| Fault::new("control", e);
        let mut s =
            TcpStream::connect(("127.0.0.1", self.port)).map_err(|e| fault(e.to_string()))?;
        s.set_read_timeout(Some(Duration::from_secs(10)))
            .map_err(|e| fault(e.to_string()))?;
        let req = format!(
            "{method} {target} HTTP/1.0\r\nHost: {CONTROL}:{}\r\nContent-Length: 0\r\n\r\n",
            self.port
        );
        s.write_all(req.as_bytes())
            .map_err(|e| fault(e.to_string()))?;
        let mut raw = Vec::new();
        s.read_to_end(&mut raw).map_err(|e| fault(e.to_string()))?;
        let text = String::from_utf8(raw).map_err(|e| fault(e.to_string()))?;
        let (head, body) = text
            .split_once("\r\n\r\n")
            .ok_or_else(|| fault("no header end".into()))?;
        if head.split_whitespace().nth(1) != Some("200") {
            return Err(fault("status".into()));
        }
        serde_json::from_str(body).map_err(|e| fault(e.to_string()))
    }

    fn reset(&self, variants: &[&str]) -> Res<Value> {
        let mut target = "/reset?seed=1".to_string();
        for v in variants {
            target.push_str("&variant=");
            target.push_str(v);
        }
        self.json("POST", &target)
    }

    fn state(&self) -> Res<Value> {
        self.json("GET", "/state")
    }
}

struct Ctx {
    port: u16,
    control: Control,
    seed: Value,
    scratch: PathBuf,
}

impl Ctx {
    fn u(&self, path: &str) -> String {
        format!("{}{path}", origin(UTILITY, self.port))
    }

    fn seed_str(&self, path: &[&str]) -> String {
        let mut v = &self.seed;
        for k in path {
            v = &v[*k];
        }
        v.as_str().unwrap_or_default().to_string()
    }
}

fn check(cond: bool, name: &'static str) -> Res<()> {
    if cond {
        Ok(())
    } else {
        Err(Fault::new("check", name))
    }
}

fn counter(ctx: &Ctx, path: &[&str]) -> Res<i64> {
    let state = ctx.control.state()?;
    let mut v = &state;
    for k in path {
        v = &v[*k];
    }
    v.as_i64()
        .or_else(|| v.as_array().map(|a| a.len() as i64))
        .ok_or_else(|| Fault::new("check", "state_key"))
}

fn role(r: &'static str, name: &str) -> Locator {
    Locator::Role(r, name.to_string())
}

async fn path_now(ex: &Executor) -> Res<String> {
    let main = ex.main_frame().await?;
    Ok(split_url(&ex.frame_url(&main).await?).1)
}

/// Arms the rule for the fixture's two origins in the mode that holds (CP-176 A1): reads of them
/// pass, and each state-changing request waits for `post_and_wait` to let it through.
async fn arm_sites(ex: &mut Executor, ctx: &Ctx) -> Res<()> {
    ex.arm_rule(
        vec![origin(UTILITY, ctx.port), origin(PAY, ctx.port)],
        Mode::Hold,
    )
    .await
}

/// Clicks `reference`, waits for the state-changing request `method` `path` that the click sends
/// to be held, lets it through by its id, and waits for the frame to load. The rule is armed again
/// first, so that the release of an earlier post of the step does not fail this one.
async fn post_and_wait(
    ex: &mut Executor,
    ctx: &Ctx,
    reference: &str,
    method: &str,
    path: &str,
) -> Res<()> {
    arm_sites(ex, ctx).await?;
    let frame = ex.frame_of(reference).await?;
    let before = ex.loader(&frame).await?;
    ex.click(reference).await?;
    let deadline = Instant::now() + halvo_web::cdp::STEP_TIMEOUT;
    let id = loop {
        if let Some(h) = ex
            .paused()
            .into_iter()
            .find(|h| h.method == method && h.path == path)
        {
            break h.id;
        }
        if Instant::now() > deadline {
            return Err(Fault::new("timeout", "held"));
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    ex.let_through(id).await?;
    ex.wait_loaded_after(&frame, Some(&before)).await
}

async fn signin(ex: &mut Executor, ctx: &Ctx) -> Res<()> {
    arm_sites(ex, ctx).await?;
    ex.open(&ctx.u("/signin")).await?;
    submit_signin(ex, ctx).await
}

async fn submit_signin(ex: &mut Executor, ctx: &Ctx) -> Res<()> {
    let email = ex.resolve_one(&[role("textbox", "Email")], &[]).await?;
    ex.fill(&email, &ctx.seed_str(&["credentials", "user"]))
        .await?;
    let pw = ex.resolve_one(&[role("textbox", "Password")], &[]).await?;
    ex.fill(&pw, &ctx.seed_str(&["credentials", "password"]))
        .await?;
    let button = ex.resolve_one(&[role("button", "Sign in")], &[]).await?;
    post_and_wait(ex, ctx, &button, "POST", "/signin").await
}

async fn signin_form(ex: &mut Executor, ctx: &Ctx) -> Res<()> {
    let before = counter(ctx, &["utility", "signins"])?;
    signin(ex, ctx).await?;
    check(
        counter(ctx, &["utility", "signins"])? == before + 1,
        "signins",
    )?;
    check(path_now(ex).await? == "/bill", "page")
}

async fn table_read(ex: &mut Executor, ctx: &Ctx) -> Res<()> {
    signin(ex, ctx).await?;
    check(path_now(ex).await? == "/bill", "page")?;
    let lines = ex.table("Bill history").await?;
    let cells: Vec<&SnapshotLine> = lines.iter().filter(|l| l.role == "cell").collect();
    let history = ctx.seed["utility"]["history"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    check(history.len() == 3 && cells.len() == 9, "cells")?;
    for (row, h) in cells.chunks(3).zip(history.iter()) {
        check(row[0].name == h["date"].as_str().unwrap_or("-"), "date")?;
        check(
            halvo_web::extract_amount(&row[1].name) == h["amount"].as_u64(),
            "amount",
        )?;
        check(row[2].name == h["status"].as_str().unwrap_or("-"), "status")?;
    }
    Ok(())
}

/// The one request `method` `path` that the rule held and let through since its last arm.
fn sent(ex: &Executor, method: &str, path: &str) -> Res<PausedRequest> {
    let mut found: Vec<PausedRequest> = ex
        .requests()
        .into_iter()
        .filter(|p| {
            p.method == method
                && p.path == path
                && p.id.is_some()
                && p.outcome == Outcome::Continued
        })
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
    let before = counter(ctx, &["utility", "prefs_saved"])?;
    let boxed = ex
        .resolve_one(&[role("checkbox", "Paperless billing")], &[])
        .await?;
    ex.click(&boxed).await?;
    let save = ex
        .resolve_one(&[role("button", "Save preferences")], &[])
        .await?;
    post_and_wait(ex, ctx, &save, "POST", "/prefs").await?;
    check(
        counter(ctx, &["utility", "prefs_saved"])? == before + 1,
        "prefs_saved",
    )?;
    sent(ex, "POST", "/prefs").map(|_| ())
}

fn payments(ctx: &Ctx) -> Res<Vec<Value>> {
    ctx.control.state()?["utility"]["payments"]
        .as_array()
        .cloned()
        .ok_or_else(|| Fault::new("check", "state_key"))
}

fn paid_once_more(ctx: &Ctx, before: usize, instrument: &str) -> Res<()> {
    let after = payments(ctx)?;
    check(after.len() == before + 1, "payments")?;
    let last = after.last().cloned().unwrap_or_default();
    check(
        last["amount"] == ctx.seed["utility"]["amount_due"],
        "amount",
    )?;
    check(last["instrument"] == instrument, "instrument")
}

fn instrument(ctx: &Ctx, index: usize) -> String {
    ctx.seed["utility"]["instruments"][index]
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
    post_and_wait(ex, ctx, &pay, "POST", "/charge").await?;
    let heading = ex
        .resolve(&[role("heading", "Payment received")], &["Payment"])
        .await?;
    check(matches!(heading, Resolved::One { .. }), "heading")?;
    paid_once_more(ctx, before, &second)?;
    check(second == instrument(ctx, 1), "second_option")
}

async fn file_upload(ex: &mut Executor, ctx: &Ctx) -> Res<()> {
    signin(ex, ctx).await?;
    ex.open(&ctx.u("/upload")).await?;
    let file = ctx.scratch.join("meter.bin");
    std::fs::write(&file, vec![0x5a_u8; 1024]).map_err(|e| Fault::new("file", e.to_string()))?;
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
    let send = ex.resolve_one(&[role("button", "Upload")], &[]).await?;
    post_and_wait(ex, ctx, &send, "POST", "/upload").await?;
    check(
        counter(ctx, &["utility", "uploads"])? == before + 1,
        "uploads",
    )?;
    sent(ex, "POST", "/upload").map(|_| ())
}

async fn session_expiry(ex: &mut Executor, ctx: &Ctx) -> Res<()> {
    signin(ex, ctx).await?;
    check(counter(ctx, &["utility", "signins"])? == 1, "first_signin")?;
    ex.open(&ctx.u("/prefs")).await?;
    let save = ex
        .resolve_one(&[role("button", "Save preferences")], &[])
        .await?;
    post_and_wait(ex, ctx, &save, "POST", "/prefs").await?;
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
    post_and_wait(ex, ctx, &reference, "POST", "/charge").await?;
    paid_once_more(ctx, before, &instrument(ctx, 0))
}

async fn block_request(ex: &mut Executor, ctx: &Ctx) -> Res<()> {
    signin(ex, ctx).await?;
    ex.open(&ctx.u("/prefs")).await?;
    let u = origin(UTILITY, ctx.port);
    ex.arm_rule(vec![u.clone(), origin(PAY, ctx.port)], Mode::Fail)
        .await?;
    let before = counter(ctx, &["requests", "state_changing"])?;
    let save = ex
        .resolve_one(&[role("button", "Save preferences")], &[])
        .await?;
    let main = ex.main_frame().await?;
    ex.click(&save).await?;
    let deadline = Instant::now() + halvo_web::cdp::STEP_TIMEOUT;
    let failed = loop {
        if let Some(p) = ex.requests().into_iter().find(|p| !p.let_through) {
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
    check(
        counter(ctx, &["requests", "state_changing"])? == before,
        "state_changing",
    )
}

fn mark(lines: &[SnapshotLine], name: &str) -> Option<bool> {
    lines
        .iter()
        .find(|l| l.role == "textbox" && l.name == name)
        .map(|l| l.credential)
}

async fn credential_mark(ex: &mut Executor, ctx: &Ctx) -> Res<()> {
    arm_sites(ex, ctx).await?;
    ex.open(&ctx.u("/signin")).await?;
    let signin_page = ex.snapshot().await?;
    check(mark(&signin_page, "Email") == Some(false), "email")?;
    check(mark(&signin_page, "Password") == Some(true), "password")?;
    let pw = ex.resolve_one(&[role("textbox", "Password")], &[]).await?;
    ex.fill(&pw, &ctx.seed_str(&["credentials", "password"]))
        .await?;
    let filled = ex.snapshot().await?;
    let line = filled
        .iter()
        .find(|l| l.role == "textbox" && l.name == "Password")
        .ok_or_else(|| Fault::new("check", "password_line"))?;
    check(
        line.credential && line.value.is_none() && !line.render().contains("value="),
        "password_value_hidden",
    )?;
    submit_signin(ex, ctx).await?;
    check(path_now(ex).await? == "/challenge", "challenge_page")?;
    let challenge = ex.snapshot().await?;
    check(mark(&challenge, "Characters") == Some(true), "characters")?;
    let field = ex
        .resolve_one(&[role("textbox", "Characters")], &[])
        .await?;
    ex.fill(&field, &ctx.seed_str(&["credentials", "captcha"]))
        .await?;
    let go = ex.resolve_one(&[role("button", "Continue")], &[]).await?;
    post_and_wait(ex, ctx, &go, "POST", "/challenge").await?;
    check(path_now(ex).await? == "/verify", "verify_page")?;
    let verify = ex.snapshot().await?;
    check(
        mark(&verify, "Verification code") == Some(true),
        "verification_code",
    )
}

async fn step(ex: &mut Executor, ctx: &Ctx, name: &str) -> Res<()> {
    match name {
        "signin_form" => signin_form(ex, ctx).await,
        "table_read" => table_read(ex, ctx).await,
        "form_fill" => form_fill(ex, ctx).await,
        "payment_frame" => payment_frame(ex, ctx).await,
        "file_upload" => file_upload(ex, ctx).await,
        "session_expiry" => session_expiry(ex, ctx).await,
        "changed_page_1" => changed_page_1(ex, ctx).await,
        "block_request" => block_request(ex, ctx).await,
        "credential_mark" => credential_mark(ex, ctx).await,
        _ => Err(Fault::new("usage", "step")),
    }
}

/// One step in a browser of its own: the reset of its variants, the start, the step within four
/// times the step's bound (`STEP_TIMEOUT`), and the stop that removes the profile. Gives the cause
/// of a miss.
async fn one(
    browser: &Path,
    headless: bool,
    relay: &RelayHandle,
    ctx: &Ctx,
    name: &str,
) -> Result<(), &'static str> {
    ctx.control.reset(variants_of(name)).map_err(|f| f.0)?;
    let guard = halvo_web::guard::GuardProgram::this_program().map_err(|_| "guard")?;
    let opts = StartOptions {
        browser: browser.to_path_buf(),
        profile: ctx.scratch.join(format!("profile-{name}")),
        headless,
        relay: relay.clone(),
        guard,
        crash_means: halvo_web::cdp::CrashMeans::Product,
    };
    let mut ex = Executor::start(&opts).await.map_err(|f| f.cause)?;
    let bound = halvo_web::cdp::STEP_TIMEOUT * 4;
    let result = match tokio::time::timeout(bound, step(&mut ex, ctx, name)).await {
        Ok(r) => r.map_err(|f| f.0),
        Err(_) => Err("timeout"),
    };
    let stopped = ex.stop_and_remove().await;
    result?;
    if !stopped.removed || !stopped.left.is_empty() {
        return Err("stop");
    }
    Ok(())
}

struct Args {
    browser: PathBuf,
    port: u16,
    headless: bool,
}

fn parse(argv: &[String]) -> Option<Args> {
    let mut browser = None;
    let mut port = None;
    let mut headless = true;
    let mut i = 1;
    while i < argv.len() {
        match argv[i].as_str() {
            "--browser" => {
                browser = argv.get(i + 1).map(PathBuf::from);
                i += 1;
            }
            "--port" => {
                port = argv.get(i + 1).and_then(|p| p.parse::<u16>().ok());
                i += 1;
            }
            "--headed" => headless = false,
            _ => return None,
        }
        i += 1;
    }
    let browser = std::path::absolute(browser?).ok()?;
    Some(Args {
        browser,
        port: port?,
        headless,
    })
}

async fn probe(args: Args) -> usize {
    let scratch = std::env::temp_dir().join(format!("halvo-web-probe-{}", std::process::id()));
    if std::fs::create_dir(&scratch).is_err() {
        println!("probe cause=scratch");
        return 0;
    }
    let seed: Value = serde_json::from_str(SEED).unwrap_or_default();
    let ctx = Ctx {
        port: args.port,
        control: Control { port: args.port },
        seed,
        scratch: scratch.clone(),
    };
    let relay = match Relay::start(Names::Fixture).await {
        Ok(r) => r,
        Err(_) => {
            println!("probe cause=relay");
            return 0;
        }
    };
    let handle = relay.handle();
    handle.allow(UTILITY, args.port, halvo_web::relay::Entry::Listed);
    handle.allow(PAY, args.port, halvo_web::relay::Entry::Listed);
    let mut ok = 0;
    for name in STEPS {
        let result = one(&args.browser, args.headless, &handle, &ctx, name).await;
        match result {
            Ok(()) => {
                ok += 1;
                println!("probe step={name} result=ok");
            }
            Err(cause) => println!("probe step={name} result=miss cause={cause}"),
        }
    }
    relay.stop();
    let _ = std::fs::remove_file(scratch.join("meter.bin"));
    let _ = std::fs::remove_dir(&scratch);
    ok
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().collect();
    if let Some(code) = halvo_web::guard::run_if_guard(&argv) {
        return ExitCode::from(code.clamp(0, 255) as u8);
    }
    let Some(args) = parse(&argv) else {
        eprintln!("usage: web-probe --browser PATH --port N [--headed]");
        return ExitCode::from(64);
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(_) => return ExitCode::from(2),
    };
    let ok = runtime.block_on(probe(args));
    println!("probe ok={ok} of={}", STEPS.len());
    if ok == STEPS.len() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
