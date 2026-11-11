use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use r3_profile::screen::{self, Window};
use r3_profile::{
    bound_bytes, encode, entry_names, landing, marker_needles, new_entries, outside_roots, search,
    search_with, LEAVE_OUT_ALL,
};
use r3_web::cdp::{Executor, Fault, Locator, Res, SessionState, StartOptions, Stopped};
use r3_web::fixture::{fixture_program, origin, Control, Seed, Server, PAY, UTILITY};
use r3_web::{browser_kind, procs, FIRST_FLAGS, RESOLVER_RULE};

#[derive(Default)]
struct Args {
    item: u8,
    browser: String,
    port: u16,
    machine: String,
    repo: PathBuf,
    scratch: PathBuf,
    leave_out: Vec<String>,
    plant: Option<(String, String)>,
    tree_peak: Option<PathBuf>,
    web_trial: Option<PathBuf>,
    runs: u32,
}

fn out(line: &str) {
    let mut s = std::io::stdout().lock();
    let _ = writeln!(s, "{line}");
    let _ = s.flush();
}

fn refuse(cause: &str, detail: &str) -> ExitCode {
    eprintln!("REFUSED {cause}: {detail}");
    ExitCode::from(2)
}

fn parse(argv: &[String]) -> Result<Args, String> {
    let mut a = Args {
        runs: 5,
        ..Args::default()
    };
    let mut i = 0;
    while i < argv.len() {
        let flag = argv[i].as_str();
        let value = argv
            .get(i + 1)
            .cloned()
            .ok_or_else(|| format!("{flag} needs a value"))?;
        match flag {
            "--item" => {
                a.item = match value.as_str() {
                    "1" => 1,
                    "2" => 2,
                    "4" => 4,
                    _ => return Err("--item takes 1, 2 or 4".into()),
                }
            }
            "--browser" => a.browser = value,
            "--port" => {
                a.port = value
                    .parse()
                    .ok()
                    .filter(|p| *p > 0 && *p < 65533)
                    .ok_or("--port takes 1 to 65532")?
            }
            "--machine" => {
                if !["mac", "hosted-mac", "hosted-win"].contains(&value.as_str()) {
                    return Err("--machine takes mac, hosted-mac or hosted-win".into());
                }
                a.machine = value;
            }
            "--repo" => a.repo = PathBuf::from(value),
            "--scratch" => a.scratch = PathBuf::from(value),
            "--leave-out" => a.leave_out.push(value),
            "--plant" => {
                let (encoding, place) = value
                    .split_once(':')
                    .ok_or("--plant takes ENCODING:PLACE")?;
                if !r3_profile::ENCODINGS.contains(&encoding)
                    || !["parent", "temp"].contains(&place)
                {
                    return Err("--plant takes utf8|utf16le|utf16be:parent|temp".into());
                }
                a.plant = Some((encoding.to_string(), place.to_string()));
            }
            "--tree-peak" => a.tree_peak = Some(PathBuf::from(value)),
            "--web-trial" => a.web_trial = Some(PathBuf::from(value)),
            "--runs" => {
                a.runs = value
                    .parse()
                    .ok()
                    .filter(|r| *r > 0)
                    .ok_or("--runs takes a count from 1")?
            }
            _ => return Err(format!("unknown argument {flag}")),
        }
        i += 2;
    }
    if a.item == 0 || a.browser.is_empty() || a.port == 0 || a.machine.is_empty() {
        return Err("--item, --browser, --port and --machine are required".into());
    }
    if a.repo.as_os_str().is_empty() || a.scratch.as_os_str().is_empty() {
        return Err("--repo and --scratch are required".into());
    }
    if a.item == 4 && (a.tree_peak.is_none() || a.web_trial.is_none()) {
        return Err("--item 4 needs --tree-peak and --web-trial".into());
    }
    if a.item != 2 && a.plant.is_some() {
        return Err("--plant belongs to --item 2".into());
    }
    if a.item == 4 && !a.leave_out.is_empty() {
        return Err("--leave-out belongs to --item 1 and 2".into());
    }
    Ok(a)
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse(&argv) {
        Ok(a) => a,
        Err(e) => return refuse("usage", &e),
    };
    let Some(kind) = browser_kind(&args.browser) else {
        return refuse("browser", "the file name is no Chrome or Edge binary");
    };
    if !Path::new(&args.browser).is_file() {
        return refuse("browser", "no file at the path");
    }
    if std::net::TcpListener::bind(("127.0.0.1", args.port)).is_err() {
        return refuse("port", &format!("127.0.0.1:{} is taken", args.port));
    }
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(_) => {
            out(&format!(
                "notrun item={} machine={} browser={kind} cause=runtime",
                args.item, args.machine
            ));
            return ExitCode::from(1);
        }
    };
    if args.item == 4 {
        return rt.block_on(item4(&args, kind));
    }
    rt.block_on(items12(&args, kind))
}

#[derive(Clone)]
struct Stop(tokio::sync::watch::Receiver<bool>);

impl Stop {
    fn is_set(&self) -> bool {
        *self.0.borrow()
    }

    async fn or<T>(&self, work: impl std::future::Future<Output = Res<T>>) -> Res<T> {
        let mut rx = self.0.clone();
        tokio::select! {
            biased;
            _ = rx.wait_for(|s| *s) => Err(Fault::new("signal", "stopped")),
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

#[derive(Default)]
struct Seen {
    front: Option<String>,
    windows: Option<Vec<Window>>,
    changed_front: bool,
    raised: Vec<Window>,
    reads: u32,
    failed_reads: u32,
    roots: Vec<u32>,
}

impl Seen {
    fn prompt(&self) -> bool {
        self.changed_front || !self.raised.is_empty()
    }
}

struct Watch {
    seen: Arc<Mutex<Seen>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

const WATCH_PERIOD: Duration = Duration::from_millis(250);
const WATCH_TAIL: Duration = Duration::from_secs(10);

impl Watch {
    fn start() -> Watch {
        let front = screen::front();
        let windows = screen::windows();
        let failed = u32::from(front.is_none()) + u32::from(windows.is_none());
        let seen = Arc::new(Mutex::new(Seen {
            front,
            windows,
            reads: 1,
            failed_reads: failed,
            ..Seen::default()
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let (shared, stopping) = (seen.clone(), stop.clone());
        let thread = std::thread::spawn(move || {
            while !stopping.load(Ordering::SeqCst) {
                sample(&shared);
                std::thread::sleep(WATCH_PERIOD);
            }
            sample(&shared);
        });
        Watch {
            seen,
            stop,
            thread: Some(thread),
        }
    }

    fn add_browser(&self, root: Option<u32>) {
        if let (Some(r), Ok(mut s)) = (root, self.seen.lock()) {
            s.roots.push(r);
        }
    }

    async fn finish(mut self) -> Seen {
        tokio::time::sleep(WATCH_TAIL).await;
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        self.seen
            .lock()
            .map(|mut s| std::mem::take(&mut *s))
            .unwrap_or_default()
    }
}

fn sample(seen: &Mutex<Seen>) {
    let roots = seen.lock().map(|s| s.roots.clone()).unwrap_or_default();
    let pids = tree_pids(&roots);
    let front = screen::front();
    let windows = screen::windows();
    let Ok(mut s) = seen.lock() else {
        return;
    };
    s.reads += 1;
    match front {
        Some(f) => {
            if s.front.as_ref().is_some_and(|b| *b != f) {
                s.changed_front = true;
            }
        }
        None => s.failed_reads += 1,
    }
    let raised = match (&s.windows, windows) {
        (Some(before), Some(now)) => screen::raised(before, &now, &pids),
        _ => {
            s.failed_reads += 1;
            Vec::new()
        }
    };
    for w in raised {
        if !s.raised.contains(&w) {
            s.raised.push(w);
        }
    }
}

fn tree_pids(roots: &[u32]) -> Vec<u32> {
    let table = procs::table().unwrap_or_default();
    let mut pids = Vec::new();
    for root in roots {
        let Some(r) = table.iter().find(|p| p.pid == *root) else {
            continue;
        };
        let tree = procs::descendants(&table, std::slice::from_ref(r));
        let bundles: Vec<PathBuf> = r
            .exe
            .as_deref()
            .map(procs::bundle_root)
            .into_iter()
            .collect();
        let adopted = procs::adopted(&table, &tree, r.started_us, &bundles);
        pids.extend(tree.iter().chain(adopted.iter()).map(|p| p.pid));
    }
    pids
}

struct Ctx {
    browser: PathBuf,
    kind: &'static str,
    machine: String,
    port: u16,
    control: Control,
    seed: Seed,
    scratch: PathBuf,
    stop: Stop,
}

impl Ctx {
    fn u(&self, path: &str) -> String {
        format!("{}{path}", origin(UTILITY, self.port))
    }

    fn origins(&self) -> Vec<String> {
        vec![origin(UTILITY, self.port), origin(PAY, self.port)]
    }

    fn options(&self, profile: PathBuf) -> StartOptions {
        let mut flags: Vec<String> = FIRST_FLAGS.iter().map(|s| s.to_string()).collect();
        flags.push(RESOLVER_RULE.to_string());
        StartOptions {
            browser: self.browser.clone(),
            profile,
            headless: true,
            flags,
            debug_port: None,
        }
    }

    fn cell(&self) -> String {
        format!("machine={} browser={}", self.machine, self.kind)
    }
}

fn printable(text: &str) -> String {
    let safe = text.len() <= 64
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_:.-".contains(&b));
    if safe {
        text.to_string()
    } else {
        format!("text_of_{}_bytes", text.len())
    }
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

async fn fill(ex: &mut Executor, name: &str, value: &str) -> Res<()> {
    let r = ex.resolve_one(&[role("textbox", name)], &[]).await?;
    ex.fill(&r, value).await
}

async fn sign_in_with_second_factor(ex: &mut Executor, ctx: &Ctx) -> Res<()> {
    ex.open(&ctx.u("/signin")).await?;
    fill(ex, "Email", &ctx.seed.str(&["credentials", "user"])).await?;
    fill(ex, "Password", &ctx.seed.str(&["credentials", "password"])).await?;
    let go = ex.resolve_one(&[role("button", "Sign in")], &[]).await?;
    click_and_wait(ex, &go).await?;
    if path_now(ex).await? != "/verify" {
        return Err(Fault::new("signin", "no_verify_page"));
    }
    fill(
        ex,
        "Verification code",
        &ctx.seed.str(&["credentials", "code"]),
    )
    .await?;
    let verify = ex.resolve_one(&[role("button", "Verify")], &[]).await?;
    click_and_wait(ex, &verify).await?;
    if path_now(ex).await? != "/bill" {
        return Err(Fault::new("signin", "no_bill_page"));
    }
    Ok(())
}

fn leave_out(state: &SessionState, names: &[String]) -> SessionState {
    let mut cut = state.clone();
    for n in names {
        if n == LEAVE_OUT_ALL {
            cut.cookies.clear();
            cut.storage.clear();
        } else {
            cut = cut.without_cookie(n);
        }
    }
    cut
}

fn stop_line(ctx: &Ctx, which: &str, s: &Stopped) {
    out(&format!(
        "stop {} profile={which} pids={} adopted={} left={} removed={}",
        ctx.cell(),
        s.pids.len(),
        s.adopted.len(),
        s.left.len(),
        if s.removed { "yes" } else { "no" }
    ));
}

struct Session {
    landing: String,
    stopped_a: Stopped,
    stopped_b: Option<Stopped>,
    b_was_empty: bool,
    marker_cookies: Vec<String>,
}

async fn session(ctx: &Ctx, leave: &[String], watch: &Watch) -> Res<Session> {
    let pid = std::process::id();
    let a_dir = ctx.scratch.join(format!("{pid}-profile-a"));
    let b_dir = ctx.scratch.join(format!("{pid}-profile-b"));
    let mut a = ctx
        .stop
        .or(Executor::start(&ctx.options(a_dir.clone())))
        .await?;
    watch.add_browser(a.pid());
    let signed = ctx.stop.or(sign_in_with_second_factor(&mut a, ctx)).await;
    let exported = match signed {
        Ok(()) => ctx.stop.or(a.export_state(&ctx.origins())).await,
        Err(f) => Err(f),
    };
    let stopped_a = a.stop_and_remove().await;
    let exported = exported?;
    let names: Vec<&str> = exported.cookies.iter().map(|c| c.name.as_str()).collect();
    out(&format!(
        "state {} cookies={} names={} storage_items={} bytes={}",
        ctx.cell(),
        exported.cookies.len(),
        if names.is_empty() {
            "-".to_string()
        } else {
            names.join(",")
        },
        exported.item_count(),
        exported.to_json().len()
    ));
    let marker = ctx.seed.str(&["page_marker"]);
    let marker_cookies: Vec<String> = exported
        .cookies
        .iter()
        .filter(|c| !marker.is_empty() && c.value == marker)
        .map(|c| c.name.clone())
        .collect();
    let state = leave_out(&exported, leave);
    drop(exported);
    if !leave.is_empty() {
        out(&format!(
            "leave_out {} names={} cookies_left={}",
            ctx.cell(),
            leave.join(","),
            state.cookies.len()
        ));
    }
    std::fs::create_dir(&b_dir).map_err(|e| Fault::new("profile", e.to_string()))?;
    let b_was_empty = std::fs::read_dir(&b_dir)
        .map(|mut e| e.next().is_none())
        .unwrap_or(false);
    let mut b = ctx.stop.or(Executor::start(&ctx.options(b_dir))).await?;
    watch.add_browser(b.pid());
    let landed = ctx
        .stop
        .or(async {
            b.import_state(&state).await?;
            b.open(&ctx.u("/bill")).await?;
            path_now(&b).await
        })
        .await;
    let stopped_b = b.stop_and_remove().await;
    Ok(Session {
        landing: landed?,
        stopped_a,
        stopped_b: Some(stopped_b),
        b_was_empty,
        marker_cookies,
    })
}

fn plant(ctx: &Ctx, marker: &str, encoding: &str, place: &str) -> Res<PathBuf> {
    let mut bytes = b"planted:".to_vec();
    bytes.extend(encode(marker, encoding).ok_or_else(|| Fault::new("plant", encoding))?);
    let dir = match place {
        "parent" => ctx.scratch.clone(),
        _ => {
            let d = std::env::temp_dir().join(format!(
                "r3-profile-plant-{}-{encoding}",
                std::process::id()
            ));
            std::fs::create_dir_all(&d).map_err(|e| Fault::new("plant", e.to_string()))?;
            d
        }
    };
    let file = dir.join(format!("planted-{encoding}.bin"));
    std::fs::write(&file, bytes).map_err(|e| Fault::new("plant", e.to_string()))?;
    Ok(file)
}

async fn items12(args: &Args, kind: &'static str) -> ExitCode {
    let cell = format!("machine={} browser={kind}", args.machine);
    let temp = std::env::temp_dir();
    let temp_before: BTreeSet<String> = entry_names(&temp);
    let started = std::time::SystemTime::now();
    let stop = match watch_signals() {
        Ok(s) => s,
        Err(_) => {
            out(&format!("notrun item={} {cell} cause=runtime", args.item));
            return ExitCode::from(1);
        }
    };
    let seed = match Seed::load(&args.repo, 1) {
        Ok(s) => s,
        Err(_) => {
            out(&format!("notrun item={} {cell} cause=seed", args.item));
            return ExitCode::from(1);
        }
    };
    let server = match Server::start(
        &fixture_program(&args.repo),
        &["--port".into(), args.port.to_string()],
    ) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("fixture: {e}");
            out(&format!("notrun item={} {cell} cause=fixture", args.item));
            return ExitCode::from(1);
        }
    };
    let ctx = Ctx {
        browser: PathBuf::from(&args.browser),
        kind,
        machine: args.machine.clone(),
        port: args.port,
        control: Control { port: args.port },
        seed,
        scratch: args.scratch.clone(),
        stop,
    };
    let code = run_items12(args, &ctx, &temp, &temp_before, started).await;
    server.stop();
    out("stop fixture=stopped");
    code
}

async fn run_items12(
    args: &Args,
    ctx: &Ctx,
    temp: &Path,
    temp_before: &BTreeSet<String>,
    started: std::time::SystemTime,
) -> ExitCode {
    let cell = ctx.cell();
    if ctx.control.reset(&["second_factor"]).is_err() {
        out(&format!("notrun item={} {cell} cause=fixture", args.item));
        return ExitCode::from(1);
    }
    let watcher = Watch::start();
    let outcome = session(ctx, &args.leave_out, &watcher).await;
    let watch = watcher.finish().await;
    let s = match outcome {
        Ok(s) => s,
        Err(f) => {
            let cause = match f.0 {
                "signal" => "signal",
                "start" | "browser_start" => "browser_start",
                "signin" => "signin",
                _ => "session",
            };
            eprintln!("session cause={} detail={}", f.0, f.1.len());
            out(&format!("notrun item={} {cell} cause={cause}", args.item));
            return ExitCode::from(1);
        }
    };
    stop_line(ctx, "a", &s.stopped_a);
    let gone = |st: &Stopped| st.profile.as_ref().is_some_and(|p| !p.exists());
    let a_gone = gone(&s.stopped_a);
    let b_gone = s.stopped_b.as_ref().is_some_and(gone);
    if let Some(b) = &s.stopped_b {
        stop_line(ctx, "b", b);
    }
    let yes_no = |v: bool| if v { "yes" } else { "no" };
    out(&format!(
        "profile {cell} a_gone={} b_gone={} b_other_name=yes b_empty_at_start={}",
        yes_no(a_gone),
        yes_no(b_gone),
        yes_no(s.b_was_empty)
    ));
    if s.stopped_b.is_none() {
        out(&format!(
            "notrun item={} {cell} cause=profile_kept",
            args.item
        ));
        return ExitCode::from(1);
    }
    let (import, asked) = landing(&s.landing);
    out(&format!(
        "session {cell} import={import} second_factor_asked_again={asked}"
    ));
    out(&format!(
        "watch {cell} reads={} failed_reads={} front_changed={} windows_raised={}",
        watch.reads,
        watch.failed_reads,
        if watch.changed_front { "yes" } else { "no" },
        watch.raised.len()
    ));
    if watch.failed_reads > 0 {
        out(&format!(
            "notrun item={} {cell} cause=window_list",
            args.item
        ));
        return ExitCode::from(1);
    }
    out(&format!(
        "keychain {cell} prompt={}",
        if watch.prompt() { "yes" } else { "no" }
    ));
    if args.item == 1 {
        return ExitCode::SUCCESS;
    }
    let marker = ctx.seed.str(&["page_marker"]);
    if marker.is_empty() {
        out(&format!("notrun item=2 {cell} cause=seed"));
        return ExitCode::from(1);
    }
    if let Some((encoding, place)) = &args.plant {
        match plant(ctx, &marker, encoding, place) {
            Ok(_) => out(&format!("plant {cell} encoding={encoding} place={place}")),
            Err(_) => {
                out(&format!("notrun item=2 {cell} cause=plant"));
                return ExitCode::from(1);
            }
        }
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let temp_after = entry_names(temp);
    let fresh = new_entries(temp, temp_before, &temp_after, &ctx.scratch);
    for entry in &fresh {
        let name = entry
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        eprintln!("temp_entry name={}", printable(&name));
    }
    let mut roots = vec![ctx.scratch.clone()];
    roots.extend(fresh.iter().cloned());
    let mut needles = marker_needles(&marker);
    for name in &s.marker_cookies {
        needles.push(name.as_bytes().to_vec());
    }
    let mut found = search_with(&roots, &needles, None);
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let outside = search_with(&outside_roots(&home, temp), &needles, Some(started));
    found.add(&outside);
    let host = search(&roots, UTILITY);
    let cookie_store: u64 = found.hits.iter().skip(3).sum();
    out(&format!(
        "searched {cell} roots={} temp_entries={} outside_files={} files={} bytes={} unreadable={} skipped={} utf8={} utf16le={} utf16be={} marker_cookies={} cookie_store={}",
        roots.len(),
        fresh.len(),
        outside.files,
        found.files,
        found.bytes,
        found.unreadable,
        found.skipped,
        found.hits[0],
        found.hits[1],
        found.hits[2],
        s.marker_cookies.len(),
        cookie_store
    ));
    out(&format!("trace {cell} host_hits={}", host.total()));
    out(&format!("disk {cell} marker_hits={}", found.total()));
    if s.marker_cookies.is_empty() && args.leave_out.is_empty() {
        out(&format!("notrun item=2 {cell} cause=marker_cookie"));
        return ExitCode::from(1);
    }
    if found.unreadable > 0 {
        out(&format!("notrun item=2 {cell} cause=unreadable"));
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

async fn item4(args: &Args, kind: &'static str) -> ExitCode {
    let cell = format!("machine={} browser={kind}", args.machine);
    let stop = match watch_signals() {
        Ok(s) => s,
        Err(_) => {
            out(&format!("notrun item=4 {cell} cause=runtime"));
            return ExitCode::from(1);
        }
    };
    let (Some(tp), Some(wt)) = (&args.tree_peak, &args.web_trial) else {
        return ExitCode::from(2);
    };
    let mut cmd = Command::new(tp);
    cmd.arg("--runs")
        .arg(args.runs.to_string())
        .arg("--")
        .arg(wt)
        .args(["--candidate", "cdp", "--browser", &args.browser, "--port"])
        .arg(args.port.to_string())
        .args(["--machine", &args.machine, "--repo"])
        .arg(&args.repo)
        .arg("--scratch")
        .arg(&args.scratch)
        .args(["--runs", "1"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(_) => {
            out(&format!("notrun item=4 {cell} cause=tree_peak"));
            return ExitCode::from(1);
        }
    };
    let Some(stdout) = child.stdout.take() else {
        return ExitCode::from(1);
    };
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let mut run_ok: u32 = 0;
    let mut runs_done: u32 = 0;
    let mut steps_ok: u32 = 0;
    let mut median: Option<u64> = None;
    let mut signalled = false;
    let mut stop_rx = stop.0.clone();
    loop {
        tokio::select! {
            line = rx.recv() => {
                let Some(line) = line else { break };
                out(&line);
                if line.starts_with("cell candidate=") && line.contains(" ok=1 of=1 ") {
                    steps_ok += 1;
                }
                if let Some(rest) = line.strip_prefix("tree run=") {
                    let fields: Vec<&str> = rest.split(' ').collect();
                    let run = fields.first().copied().unwrap_or("0");
                    let exit = fields.iter().find_map(|f| f.strip_prefix("exit=")).unwrap_or("-");
                    let peak = fields.iter().find_map(|f| f.strip_prefix("peak_bytes=")).unwrap_or("0");
                    runs_done += 1;
                    out(&format!("steps {cell} run={run} ok={steps_ok} of=9"));
                    if exit == "0" && steps_ok == 9 {
                        run_ok += 1;
                    }
                    steps_ok = 0;
                    out(&format!("browser {cell} run={run} peak_bytes={peak}"));
                }
                if let Some(m) = line.strip_prefix("tree median_bytes=") {
                    median = m.parse().ok();
                }
            }
            _ = stop_rx.wait_for(|s| *s), if !signalled => {
                signalled = true;
                #[cfg(unix)]
                unsafe { libc_kill(child.id() as i32, 15) };
            }
        }
    }
    let status = child.wait();
    let done = status.map(|s| s.success()).unwrap_or(false);
    if stop.is_set() {
        out(&format!("notrun item=4 {cell} cause=signal"));
        return ExitCode::from(1);
    }
    match median {
        Some(m) if done && runs_done == args.runs && run_ok == args.runs => {
            out(&format!(
                "browser machine={} bound_bytes={}",
                args.machine,
                bound_bytes(m)
            ));
            ExitCode::SUCCESS
        }
        _ => {
            out(&format!(
                "notrun item=4 {cell} runs_ok={run_ok} of={} cause=runs",
                args.runs
            ));
            ExitCode::from(1)
        }
    }
}

#[cfg(unix)]
extern "C" {
    #[link_name = "kill"]
    fn libc_kill(pid: i32, sig: i32) -> i32;
}
