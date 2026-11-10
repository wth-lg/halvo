use crate::pages::{self, Server};
use crate::sys::{self, Browser};
use crate::text::{error_rate, join_pieces};
use crate::trial::{append, How, Record};
use serde_json::{json, Value};
use std::cell::Cell;
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub const WINDOW_MS: u64 = 2000;
pub const DISPLAY: (usize, usize) = (1920, 1080);
pub const SCALES: [&str; 2] = ["1.0", "0.5"];
pub const ONE_ENGINE: &str = "pp-ocrv6-tiny";
const FIREFOX_PREFS: &str = "user_pref(\"browser.shell.checkDefaultBrowser\", false);\nuser_pref(\"browser.startup.homepage_override.mstone\", \"ignore\");\nuser_pref(\"startup.homepage_welcome_url\", \"about:blank\");\nuser_pref(\"startup.homepage_welcome_url.additional\", \"\");\nuser_pref(\"termsofuse.bypassNotification\", true);\nuser_pref(\"datareporting.policy.dataSubmissionPolicyBypassNotification\", true);\nuser_pref(\"browser.sessionstore.resume_from_crash\", false);\nuser_pref(\"browser.warnOnQuit\", false);\nuser_pref(\"app.update.disabledForTesting\", true);\nuser_pref(\"browser.aboutwelcome.enabled\", false);\n";

#[derive(Default)]
pub struct Options {
    pub out: PathBuf,
    pub only: Vec<String>,
    pub runs: Option<u32>,
    pub soffice: Option<PathBuf>,
    pub soffice_version: String,
    pub ocr_bin: Option<PathBuf>,
    pub ocr_args: Vec<String>,
    pub ocr_version: String,
    pub node: Option<PathBuf>,
    pub input_dir: Option<PathBuf>,
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Normal,
    Kiosk,
    Activated,
}

struct Running {
    child: Option<Child>,
    pid: u32,
    profile: PathBuf,
    webkit: bool,
}

impl Running {
    fn stop(mut self, title: &str) {
        sys::kill_tree(sys::resolve_pid(self.pid, title));
        sys::kill_tree(self.pid);
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn pause(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

fn ms(since: Instant) -> f64 {
    since.elapsed().as_secs_f64() * 1000.0
}

fn file_url(path: &Path) -> String {
    let text = path.display().to_string().replace('\\', "/");
    if text.starts_with('/') {
        format!("file://{text}")
    } else {
        format!("file:///{text}")
    }
}

fn quiet(program: &Path) -> Command {
    let mut command = Command::new(program);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

fn collect(mut command: Command, timeout: Duration) -> Result<(bool, String), String> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("the process did not start: {e}"))?;
    let mut stdout = child.stdout.take().ok_or("the process has no output")?;
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stdout.read_to_string(&mut text);
        text
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(e) => return Err(format!("wait: {e}")),
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        pause(50);
    };
    let text = reader.join().unwrap_or_default();
    match status {
        None => Err(format!("no answer in {} s", timeout.as_secs())),
        Some(status) => Ok((status.success(), text)),
    }
}

fn answer(text: &str) -> Option<Result<Record, String>> {
    let line = text.lines().rev().find(|line| !line.trim().is_empty())?;
    let value: Value = serde_json::from_str(line).ok()?;
    if let Some(error) = value.get("error").and_then(Value::as_str) {
        return Some(Err(error.to_string()));
    }
    serde_json::from_value(value.get("ok")?.clone())
        .ok()
        .map(Ok)
}

struct Suite {
    options: Options,
    server: Server,
    exe: PathBuf,
    records: PathBuf,
    scratch: PathBuf,
    tokens: u32,
    crashes: Cell<u32>,
    pooled: BTreeMap<String, u32>,
}

impl Suite {
    fn wanted(&self, cell: &str) -> bool {
        self.options.only.is_empty()
            || self
                .options
                .only
                .iter()
                .any(|prefix| cell.starts_with(prefix.as_str()))
    }

    fn runs(&self, planned: u32) -> u32 {
        self.options.runs.unwrap_or(planned)
    }

    fn token(&mut self) -> String {
        self.tokens += 1;
        format!("t{}", self.tokens)
    }

    fn save(&self, record: &Record) {
        if let Err(error) = append(&self.records, record) {
            println!("records: {error}");
        }
    }

    fn snap(&self, cell: &str, run: u32) {
        if run == 1 {
            let path = self.options.out.join("frames").join(format!("{cell}.png"));
            let _ = self.child(&["frame", "--out", &path.display().to_string()], 30);
        }
    }

    fn missing(&self, cell: &str, reason: &str) {
        self.save(&Record::missing(cell, reason));
        println!("cell {cell}: not measurable");
    }

    fn collect_record(
        &self,
        command: impl Fn() -> Command,
        seconds: u64,
    ) -> Result<Record, String> {
        let mut last = String::new();
        for _ in 0..3 {
            let (success, text) = collect(command(), Duration::from_secs(seconds))?;
            match answer(&text) {
                Some(result) => return result,
                None => {
                    self.crashes.set(self.crashes.get() + 1);
                    last = format!(
                        "the process ended {} and printed no record",
                        if success {
                            "with status 0"
                        } else {
                            "with a failure status"
                        }
                    );
                }
            }
        }
        Err(last)
    }

    fn child(&self, args: &[&str], seconds: u64) -> Result<Record, String> {
        self.collect_record(
            || {
                let mut command = Command::new(&self.exe);
                command.arg("child").args(args);
                command
            },
            seconds,
        )
    }

    fn run_cell<F>(&mut self, cell: &str, planned: u32, mut one: F) -> bool
    where
        F: FnMut(&mut Suite, u32) -> Result<Record, String>,
    {
        if !self.wanted(cell) {
            return true;
        }
        let runs = self.runs(planned);
        let mut done = 0;
        let mut first_error: Option<String> = None;
        for run in 1..=runs {
            self.crashes.set(0);
            match one(self, run) {
                Ok(mut record) => {
                    record.cell = cell.to_string();
                    record.run = run;
                    let record = record.number(
                        "crash_reruns_count",
                        How::Sum,
                        f64::from(self.crashes.get()),
                    );
                    self.save(&record);
                    done += 1;
                }
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
        println!("cell {cell}: {done} of {runs} runs");
        if done < runs {
            let reason = format!(
                "{done} of {runs} runs answered: {}",
                first_error.unwrap_or_default()
            );
            self.save(&Record::missing(cell, &reason));
            return false;
        }
        true
    }

    fn browser_args(browser: &Browser, profile: &Path, url: &str, mode: Mode) -> Vec<String> {
        if browser.name == "firefox" {
            return vec![
                "-new-instance".to_string(),
                "-profile".to_string(),
                profile.display().to_string(),
                url.to_string(),
            ];
        }
        let mut args = vec![
            format!("--user-data-dir={}", profile.display()),
            "--no-first-run".to_string(),
            "--no-default-browser-check".to_string(),
            "--disable-search-engine-choice-screen".to_string(),
        ];
        if cfg!(target_os = "macos") {
            args.push("--use-mock-keychain".to_string());
        }
        match mode {
            Mode::Kiosk => args.push("--kiosk".to_string()),
            Mode::Activated if cfg!(windows) => {
                args.push("--force-renderer-accessibility".to_string());
                args.push("--new-window".to_string());
            }
            _ => args.push("--new-window".to_string()),
        }
        args.push(url.to_string());
        args
    }

    fn start(&mut self, browser: &Browser, page: &str, mode: Mode) -> Result<Running, String> {
        let token = self.token();
        let url = self.server.url(page, &token);
        let profile = self.scratch.join(format!("profile-{token}"));
        std::fs::create_dir_all(&profile).map_err(|e| format!("profile: {e}"))?;
        let webkit = browser.name == "safari";
        let child = if webkit {
            if let Some(old) = sys::pid_named("Safari") {
                sys::kill_tree(old);
                pause(1500);
            }
            let _ = quiet(Path::new("open"))
                .args(["-a", "Safari", &url])
                .status();
            None
        } else {
            if browser.name == "firefox" {
                std::fs::write(profile.join("user.js"), FIREFOX_PREFS)
                    .map_err(|e| format!("user.js: {e}"))?;
            }
            Some(
                quiet(Path::new(&browser.path))
                    .args(Self::browser_args(browser, &profile, &url, mode))
                    .spawn()
                    .map_err(|e| format!("the browser did not start: {e}"))?,
            )
        };
        let mut running = Running {
            pid: child.as_ref().map(Child::id).unwrap_or(0),
            child,
            profile,
            webkit,
        };
        if !self.server.wait_for(&token, Duration::from_secs(40)) {
            running.stop(&pages::title(page));
            return Err("the browser did not request the page in 40 s".to_string());
        }
        if webkit {
            match sys::pid_named("Safari") {
                Some(pid) => running.pid = pid,
                None => return Err("no Safari process was found".to_string()),
            }
        }
        pause(2500);
        Ok(running)
    }

    fn start_office(&mut self, target: &str) -> Result<(Running, String), String> {
        let soffice = self
            .options
            .soffice
            .clone()
            .ok_or("no LibreOffice path was given")?;
        let (file, content) = pages::document(target).ok_or("no such document")?;
        let token = self.token();
        let dir = self.scratch.join(format!("office-{token}"));
        let user = dir.join("profile").join("user");
        std::fs::create_dir_all(&user).map_err(|e| format!("profile: {e}"))?;
        std::fs::write(
            user.join("registrymodifications.xcu"),
            pages::OFFICE_PROFILE,
        )
        .map_err(|e| format!("profile: {e}"))?;
        std::fs::write(dir.join(file), content).map_err(|e| format!("document: {e}"))?;
        let child = quiet(&soffice)
            .args([
                "--norestore",
                "--nologo",
                "--nofirststartwizard",
                "--nolockcheck",
            ])
            .arg(format!(
                "-env:UserInstallation={}",
                file_url(&dir.join("profile"))
            ))
            .arg(dir.join(file))
            .spawn()
            .map_err(|e| format!("LibreOffice did not start: {e}"))?;
        pause(20_000);
        let running = Running {
            pid: sys::resolve_pid(child.id(), file),
            child: Some(child),
            profile: dir,
            webkit: false,
        };
        Ok((running, file.to_string()))
    }

    fn address_cells(&mut self) {
        let page = "text-1";
        let title = pages::title(page);
        for browser in sys::browsers() {
            let cell = format!("address.{}.{}", sys::RUNNER, browser.name);
            self.run_cell(&cell, 5, |suite, run| {
                let running = suite.start(&browser, page, Mode::Normal)?;
                let pid = running.pid.to_string();
                let mut args = vec![
                    "address",
                    "--pid",
                    &pid,
                    "--needle",
                    "127.0.0.1",
                    "--title",
                    &title,
                ];
                if running.webkit {
                    args.push("--webkit");
                }
                let result = suite.child(&args, 60);
                suite.snap(&cell, run);
                running.stop(&title);
                Ok(result?.text("version", &browser.version))
            });
        }
    }

    fn accessibility_cells(&mut self) {
        let chrome = sys::browsers().into_iter().find(|b| b.name == "chrome");
        for page in ["text-1", "text-2", "text-3", pages::LIST_PAGE] {
            let title = pages::title(page);
            for (variant, mode) in [("plain", Mode::Normal), ("activated", Mode::Activated)] {
                let cell = format!("accessibility.{}.{page}.{variant}", sys::RUNNER);
                self.run_cell(&cell, 5, |suite, run| {
                    let chrome = chrome.clone().ok_or("the runner has no Chrome")?;
                    let running = suite.start(&chrome, page, mode)?;
                    let pid = running.pid.to_string();
                    let mut args = vec!["accessibility", "--pid", &pid, "--title", &title];
                    if mode == Mode::Activated {
                        if cfg!(windows) {
                            args.extend(["--step", "force-renderer-accessibility"]);
                        } else {
                            args.push("--activate");
                        }
                    }
                    let result = suite.child(&args, 60);
                    suite.snap(&cell, run);
                    running.stop(&title);
                    Ok(result?
                        .text("process", "chrome")
                        .text("version", &chrome.version))
                });
            }
        }
        for target in ["calc", "writer"] {
            let cell = format!("accessibility.{}.{target}", sys::RUNNER);
            if !self.wanted(&cell) {
                continue;
            }
            match self.start_office(target) {
                Err(error) => self.missing(&cell, &error),
                Ok((running, file)) => {
                    let pid = running.pid.to_string();
                    let version = self.options.soffice_version.clone();
                    self.run_cell(&cell, 5, |suite, run| {
                        suite.snap(&cell, run);
                        Ok(suite
                            .child(&["accessibility", "--pid", &pid, "--title", &file], 60)?
                            .text("process", "soffice")
                            .text("version", &version))
                    });
                    running.stop(&file);
                }
            }
        }
    }

    fn pool(&mut self, cell: &str, record: &Record) {
        let run = self.pooled.entry(cell.to_string()).or_insert(0);
        *run += 1;
        let copy = record.renamed(cell, *run);
        self.save(&copy);
    }

    fn extract_cells(&mut self) {
        let chrome = sys::browsers().into_iter().find(|b| b.name == "chrome");
        let frames = self.options.out.join("frames");
        let mut engines: Vec<String> = sys::OS_ENGINES.iter().map(|e| e.to_string()).collect();
        if self.options.ocr_bin.is_some() {
            engines.push(ONE_ENGINE.to_string());
        }
        let system_version = sys::system_version();
        for target in ["text-1", "text-2", "text-3", "calc", "writer"] {
            let is_page = target.starts_with("text-");
            let cells: Vec<(String, String, String)> = engines
                .iter()
                .flat_map(|engine| {
                    SCALES.iter().map(move |scale| {
                        (
                            engine.clone(),
                            scale.to_string(),
                            format!("extract.{}.{engine}.{target}.x{scale}", sys::RUNNER),
                        )
                    })
                })
                .filter(|(_, _, cell)| self.wanted(cell))
                .collect();
            if cells.is_empty() {
                continue;
            }
            let started = if is_page {
                match &chrome {
                    Some(chrome) => self
                        .start(chrome, target, Mode::Kiosk)
                        .map(|running| (running, pages::title(target))),
                    None => Err("the runner has no Chrome".to_string()),
                }
            } else {
                self.start_office(target)
            };
            let (running, title) = match started {
                Ok(started) => started,
                Err(error) => {
                    for (engine, scale, cell) in &cells {
                        self.missing(cell, &error);
                        if is_page {
                            self.missing(
                                &format!("engine.{}.{engine}.x{scale}", sys::RUNNER),
                                &error,
                            );
                        }
                    }
                    continue;
                }
            };
            for (engine, scale, cell) in &cells {
                let pooled = format!("engine.{}.{engine}.x{scale}", sys::RUNNER);
                let version = if engine == ONE_ENGINE {
                    self.options.ocr_version.clone()
                } else {
                    system_version.clone()
                };
                let complete = self.run_cell(cell, 5, |suite, run| {
                    let frame = frames.join(format!("{target}.run{run}.png"));
                    let frame_text = frame.display().to_string();
                    let record = if engine == ONE_ENGINE {
                        if !frame.exists() {
                            return Err("the run has no saved frame".to_string());
                        }
                        let bin = suite.options.ocr_bin.clone().ok_or("no engine binary")?;
                        let extra = suite.options.ocr_args.clone();
                        suite.collect_record(
                            || {
                                let mut command = Command::new(&bin);
                                command
                                    .args([
                                        "--frame",
                                        &frame_text,
                                        "--scale",
                                        scale,
                                        "--target",
                                        target,
                                    ])
                                    .args(&extra);
                                command
                            },
                            120,
                        )?
                    } else {
                        let mut args = vec![
                            "extract", "--engine", engine, "--scale", scale, "--target", target,
                        ];
                        if engine == sys::OS_ENGINES[0] && scale == "1.0" {
                            args.extend(["--frame-out", &frame_text]);
                        }
                        suite.child(&args, 60)?
                    };
                    let record = record.text("version", &version);
                    if is_page {
                        suite.pool(&pooled, &record);
                    }
                    Ok(record)
                });
                if !complete && is_page && self.wanted(cell) {
                    self.missing(&pooled, &format!("a run of {cell} is missing"));
                }
            }
            running.stop(&title);
        }
    }

    fn private_run(&mut self, browser: &Browser, cell: &str, run: u32) -> Result<Record, String> {
        let normal = pages::title("page-one");
        let private = pages::title("page-two");
        let running = self.start(browser, "page-one", Mode::Normal)?;
        let pid = running.pid.to_string();
        let token = self.token();
        let url = self.server.url("page-two", &token);
        let mut second: Option<Child> = None;
        let opened: Result<(), String> = match browser.name.as_str() {
            "safari" => self
                .child(
                    &[
                        "press-menu",
                        "--pid",
                        &pid,
                        "--path",
                        "File",
                        "--path",
                        "New Private Window",
                    ],
                    30,
                )
                .map(|_| {
                    pause(2000);
                    let _ = quiet(Path::new("open"))
                        .args(["-a", "Safari", &url])
                        .status();
                }),
            "firefox" => quiet(Path::new(&browser.path))
                .args([
                    "-profile",
                    &running.profile.display().to_string(),
                    "-private-window",
                    &url,
                ])
                .spawn()
                .map(|child| second = Some(child))
                .map_err(|e| format!("the second start failed: {e}")),
            name => quiet(Path::new(&browser.path))
                .arg(format!("--user-data-dir={}", running.profile.display()))
                .arg(if name == "edge" {
                    "--inprivate"
                } else {
                    "--incognito"
                })
                .arg(&url)
                .spawn()
                .map(|child| second = Some(child))
                .map_err(|e| format!("the second start failed: {e}")),
        };
        let result = opened.and_then(|()| {
            let requested = self.server.wait_for(&token, Duration::from_secs(30));
            if !requested && !running.webkit {
                return Err("the private window did not request its page in 30 s".to_string());
            }
            pause(2500);
            self.snap(cell, run);
            self.child(
                &[
                    "private",
                    "--pid",
                    &pid,
                    "--normal",
                    &normal,
                    "--private",
                    &private,
                ],
                60,
            )
        });
        if let Some(mut child) = second {
            let _ = child.kill();
            let _ = child.wait();
        }
        running.stop(&normal);
        if let Some(pid) = sys::pid_named("Safari").filter(|_| browser.name == "safari") {
            sys::kill_tree(pid);
        }
        sys::kill_tree(sys::resolve_pid(0, &private));
        result
    }

    fn private_cells(&mut self) {
        for browser in sys::browsers() {
            let cell = format!("private.{}.{}", sys::RUNNER, browser.name);
            if !self.wanted(&cell) {
                continue;
            }
            let runs = self.runs(3);
            let mut reads = Vec::new();
            let mut failure = String::new();
            for run in 1..=runs {
                match self.private_run(&browser, &cell, run) {
                    Ok(read) => reads.push(read),
                    Err(error) => {
                        failure = error;
                        break;
                    }
                }
            }
            println!("cell {cell}: {} of {runs} runs", reads.len());
            if (reads.len() as u32) < runs {
                self.missing(
                    &cell,
                    &format!("{} of {runs} runs answered: {failure}", reads.len()),
                );
                continue;
            }
            let answered = |read: &Record, signal: &str| {
                read.detail.get(signal).and_then(Value::as_bool) == Some(true)
            };
            let signal = ["attribute", "class", "title"]
                .into_iter()
                .find(|signal| reads.iter().all(|read| answered(read, signal)))
                .unwrap_or("none");
            for (index, read) in reads.iter().enumerate() {
                let mut record = Record::new(&cell, index as u32 + 1)
                    .text("signal", signal)
                    .text("version", &browser.version);
                for name in ["attribute", "class", "title"] {
                    record = record.number(
                        &format!("{name}_runs"),
                        How::Sum,
                        f64::from(u8::from(answered(read, name))),
                    );
                }
                self.save(&record.with_detail(read.detail.clone()));
            }
        }
    }

    fn input_run(&mut self, driver: &str) -> Result<Record, String> {
        let node = self.options.node.clone().ok_or("no node path was given")?;
        let dir = self
            .options
            .input_dir
            .clone()
            .ok_or("no input directory was given")?;
        let token = self.token();
        let url = self.server.url("input", &token);
        let ready = self.scratch.join(format!("ready-{token}"));
        let stop = self.scratch.join(format!("stop-{token}"));
        let mut counter = Command::new(&self.exe)
            .args(["child", "count-input", "--ms", "120000"])
            .arg("--ready")
            .arg(&ready)
            .arg("--stop")
            .arg(&stop)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| format!("the counter did not start: {e}"))?;
        let deadline = Instant::now() + Duration::from_secs(15);
        while !ready.exists() && Instant::now() < deadline {
            pause(50);
        }
        let driven = if ready.exists() {
            self.drive(&node, &dir, &url, driver)
        } else {
            Err("the counter did not register in 15 s".to_string())
        };
        pause(500);
        let _ = std::fs::write(&stop, b"stop\n");
        let mut text = String::new();
        if let Some(mut out) = counter.stdout.take() {
            let _ = out.read_to_string(&mut text);
        }
        let _ = counter.wait();
        let (version, page) = driven?;
        let counted = answer(&text).ok_or("the counter printed no record")??;
        let count = |key: &str| {
            counted
                .detail
                .get(key)
                .and_then(Value::as_f64)
                .unwrap_or(0.0)
        };
        let on_page = |key: &str| page.get(key).and_then(Value::as_f64).unwrap_or(0.0);
        Ok(Record::new("", 0)
            .number("keys", How::Min, count("keys"))
            .number("clicks", How::Min, count("clicks"))
            .text("version", &version)
            .number("page_keys_count", How::Min, on_page("keys"))
            .number("page_clicks_count", How::Min, on_page("clicks"))
            .number("no_device_keys_count", How::Min, count("no_device_keys"))
            .number(
                "no_device_clicks_count",
                How::Min,
                count("no_device_clicks"),
            )
            .with_detail(json!({ "page": page })))
    }

    fn drive(
        &self,
        node: &Path,
        dir: &Path,
        url: &str,
        driver: &str,
    ) -> Result<(String, Value), String> {
        let mode = if driver == "playwright" {
            "playwright"
        } else {
            "hold"
        };
        let mut drive = Command::new(node)
            .arg(dir.join("drive.mjs"))
            .args([url, mode])
            .current_dir(dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| format!("node did not start: {e}"))?;
        let pid = drive.id();
        std::thread::spawn(move || {
            pause(90_000);
            sys::kill_tree(pid);
        });
        let mut lines = BufReader::new(drive.stdout.take().ok_or("node has no output")?).lines();
        let mut next = |what: &str| -> Result<Value, String> {
            let line = lines
                .next()
                .and_then(Result::ok)
                .ok_or_else(|| format!("the page driver ended before its {what} line"))?;
            serde_json::from_str(&line).map_err(|e| format!("the {what} line: {e}"))
        };
        let geometry = next("geometry")?;
        let mut version = geometry
            .get("version")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if driver == "enigo" {
            let at = |key: &str| {
                geometry
                    .get("button")
                    .and_then(|button| button.get(key))
                    .and_then(Value::as_i64)
                    .unwrap_or(0) as i32
            };
            pause(500);
            sys::send_input(at("screenX"), at("screenY"))?;
            pause(500);
            version = sys::input_driver_version();
            if let Some(stdin) = drive.stdin.as_mut() {
                let _ = writeln!(stdin, "done");
            }
        }
        let counts = next("counts")?;
        let _ = drive.wait();
        Ok((version, counts))
    }

    fn input_cells(&mut self) {
        if let Some(preflight) = sys::listen_preflight() {
            let cell = format!("listen-event-access.{}", sys::RUNNER);
            if self.wanted(&cell) {
                self.save(&Record::new(&cell, 1).text("preflight", &preflight.to_string()));
                println!("cell {cell}: 1 of 1 runs");
            }
            return;
        }
        for driver in ["playwright", "enigo"] {
            let cell = format!("input.{}.{driver}", sys::RUNNER);
            self.run_cell(&cell, 3, |suite, _| suite.input_run(driver));
        }
    }
}

pub fn run(options: Options) -> Result<(), String> {
    std::fs::create_dir_all(&options.out).map_err(|e| format!("{}: {e}", options.out.display()))?;
    let name = options
        .out
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    let scratch = options.out.with_file_name(format!("{name}-scratch"));
    std::fs::create_dir_all(&scratch).map_err(|e| format!("scratch: {e}"))?;
    std::fs::create_dir_all(options.out.join("frames")).map_err(|e| format!("frames: {e}"))?;
    let before = sys::display_size();
    let mut note = String::new();
    if before != DISPLAY {
        if let Err(error) = sys::set_display_size(DISPLAY.0, DISPLAY.1) {
            note = error;
        }
        pause(3000);
    }
    let after = sys::display_size();
    println!(
        "display {}x{} then {}x{}",
        before.0, before.1, after.0, after.1
    );
    let env = |name: &str| std::env::var(name).unwrap_or_default();
    let meta = json!({
        "runner": sys::RUNNER,
        "system_version": sys::system_version(),
        "display_before": [before.0, before.1],
        "display_after": [after.0, after.1],
        "display_note": note,
        "hosted": {
            "branch": env("GITHUB_REF_NAME"),
            "commit": env("GITHUB_SHA"),
            "run": env("GITHUB_RUN_ID"),
            "attempt": env("GITHUB_RUN_ATTEMPT"),
        },
        "browsers": sys::browsers().iter().map(|b| json!({"name": b.name, "version": b.version})).collect::<Vec<Value>>(),
    });
    std::fs::write(options.out.join("meta.json"), format!("{meta}\n"))
        .map_err(|e| format!("meta: {e}"))?;
    let mut suite = Suite {
        records: options.out.join("records.jsonl"),
        exe: std::env::current_exe().map_err(|e| format!("current exe: {e}"))?,
        server: Server::start()?,
        options,
        scratch,
        tokens: 0,
        crashes: Cell::new(0),
        pooled: BTreeMap::new(),
    };
    suite.address_cells();
    suite.accessibility_cells();
    suite.extract_cells();
    suite.private_cells();
    suite.input_cells();
    Ok(())
}

fn value(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == name)
        .and_then(|index| args.get(index + 1))
        .cloned()
}

fn values(args: &[String], name: &str) -> Vec<String> {
    args.windows(2)
        .filter(|pair| pair[0] == name)
        .map(|pair| pair[1].clone())
        .collect()
}

fn need(args: &[String], name: &str) -> Result<String, String> {
    value(args, name).ok_or_else(|| format!("the argument {name} is missing"))
}

fn pid_of(args: &[String]) -> Result<u32, String> {
    need(args, "--pid")?
        .parse()
        .map_err(|_| "the pid is not a number".to_string())
}

fn sampled(
    record: Record,
    before: (sys::Usage, sys::Usage),
    after: sys::Usage,
    start: Instant,
) -> Record {
    record
        .number(
            "peak_footprint_bytes",
            How::Max,
            sys::own_usage().peak_bytes as f64,
        )
        .number(
            "cpu_before_ms",
            How::Median,
            before.1.cpu_ms - before.0.cpu_ms,
        )
        .number("cpu_after_ms", How::Median, after.cpu_ms - before.1.cpu_ms)
        .number("mem_before_bytes", How::Median, before.1.mem_bytes as f64)
        .number("mem_after_bytes", How::Median, after.mem_bytes as f64)
        .number("before_window_ms", How::Median, WINDOW_MS as f64)
        .number("after_window_ms", How::Median, ms(start))
        .text("mem_kind", sys::MEM_KIND)
        .text("peak_kind", sys::PEAK_KIND)
}

fn baseline(pid: u32, webkit: bool) -> (sys::Usage, sys::Usage) {
    let first = sys::tree_usage(pid, webkit);
    pause(WINDOW_MS);
    (first, sys::tree_usage(pid, webkit))
}

fn rest(start: Instant, until_ms: u64) {
    let passed = start.elapsed().as_millis() as u64;
    if passed < until_ms {
        pause(until_ms - passed);
    }
}

fn child_address(args: &[String]) -> Result<Record, String> {
    sys::granted()?;
    let title = need(args, "--title")?;
    let needle = need(args, "--needle")?;
    let webkit = args.iter().any(|arg| arg == "--webkit");
    let pid = sys::resolve_pid(pid_of(args)?, &title);
    let before = baseline(pid, webkit);
    let start = Instant::now();
    let read = sys::address_read(pid, &needle, &title);
    let wall_ms = ms(start);
    rest(start, WINDOW_MS);
    let after = sys::tree_usage(pid, webkit);
    let readable = read.value.is_some() && read.activation != "set";
    let record = Record::new("", 0)
        .text("activation", &read.activation)
        .number("readable_runs", How::Sum, f64::from(u8::from(readable)))
        .number("wall_ms", How::Median, wall_ms)
        .number(
            "value_bytes",
            How::Max,
            read.value.as_ref().map(String::len).unwrap_or(0) as f64,
        )
        .number("attempts_count", How::Max, f64::from(read.attempts))
        .number("visited_nodes", How::Max, f64::from(read.nodes))
        .text(
            "role",
            if read.role.is_empty() {
                "none"
            } else {
                &read.role
            },
        );
    Ok(sampled(record, before, after, start))
}

fn child_accessibility(args: &[String]) -> Result<Record, String> {
    sys::granted()?;
    let title = need(args, "--title")?;
    let pid = sys::resolve_pid(pid_of(args)?, &title);
    let step = if args.iter().any(|arg| arg == "--activate") {
        let step = sys::activate(pid)?;
        pause(3000);
        step
    } else {
        value(args, "--step").unwrap_or_else(|| "none".to_string())
    };
    let before = baseline(pid, false);
    let start = Instant::now();
    let focused = sys::focused_read(pid);
    let focused_ms = ms(start);
    let timer = Instant::now();
    let first = sys::subtree_read(pid, &title)?;
    let first_ms = ms(timer);
    rest(start, 1500);
    let timer = Instant::now();
    let second = sys::subtree_read(pid, &title)?;
    let second_ms = ms(timer);
    let first_bytes = first.text.len();
    let (best, best_ms) = if second.text.len() >= first_bytes {
        (second, second_ms)
    } else {
        (first, first_ms)
    };
    rest(start, WINDOW_MS);
    let after = sys::tree_usage(pid, false);
    let record = Record::new("", 0)
        .number("focused_ms", How::Median, focused_ms)
        .number("focused_bytes", How::Max, focused.bytes as f64)
        .number("subtree_ms", How::Median, best_ms)
        .number("first_bytes", How::Max, first_bytes as f64)
        .number("stored_bytes", How::Max, best.text.len() as f64)
        .number("text_lines", How::Max, best.lines as f64)
        .number("visited_nodes", How::Max, best.visited as f64)
        .text("activation_step", &step)
        .text("name_attr", sys::NAME_ATTR)
        .text("value_attr", sys::VALUE_ATTR)
        .text("view", sys::TREE_VIEW)
        .with_detail(json!({ "text": best.text, "focused_found": focused.found }));
    Ok(sampled(record, before, after, start))
}

fn child_extract(args: &[String]) -> Result<Record, String> {
    let engine = need(args, "--engine")?;
    let scale = need(args, "--scale")?;
    let target = need(args, "--target")?;
    let (frame, device) = sys::capture()?;
    if (frame.width, frame.height) != DISPLAY {
        return Err(format!(
            "the frame is {}x{} and not {}x{}",
            frame.width, frame.height, DISPLAY.0, DISPLAY.1
        ));
    }
    if let Some(path) = value(args, "--frame-out") {
        frame.save_png(Path::new(&path))?;
    }
    let image = if scale == "0.5" { frame.halve() } else { frame };
    let (_, first_ms) = sys::recognize(&engine, &image)?;
    let cpu_start = sys::own_usage().cpu_ms;
    let (pieces, elapsed_ms) = sys::recognize(&engine, &image)?;
    let cpu_ms = sys::own_usage().cpu_ms - cpu_start;
    let mut record = extraction(
        &target,
        &pieces,
        elapsed_ms,
        cpu_ms,
        first_ms,
        (image.width, image.height),
    )
    .text("filter", if scale == "0.5" { "box-2x2" } else { "none" })
    .text("capture", &device);
    for (key, setting) in sys::engine_settings(&engine) {
        record = record.text(key, &setting);
    }
    Ok(record)
}

pub fn extraction(
    target: &str,
    pieces: &[String],
    elapsed_ms: f64,
    cpu_ms: f64,
    first_ms: f64,
    size: (usize, usize),
) -> Record {
    let text = join_pieces(pieces);
    let mut record = Record::new("", 0)
        .number("call_ms", How::Median, elapsed_ms.max(cpu_ms))
        .number("elapsed_ms", How::Median, elapsed_ms)
        .number("cpu_ms", How::Median, cpu_ms)
        .number("first_call_ms", How::Median, first_ms);
    if target.starts_with("text-") {
        if let Some(rate) = pages::known_text(target).and_then(|known| error_rate(&text, &known)) {
            record = record
                .number("cer_pct", How::Median, rate.percent())
                .number("known_chars", How::Max, rate.known_chars as f64);
        }
    }
    record
        .number("stored_bytes", How::Max, text.len() as f64)
        .number(
            "peak_footprint_bytes",
            How::Max,
            sys::own_usage().peak_bytes as f64,
        )
        .number("frame_width_px", How::Max, size.0 as f64)
        .number("frame_height_px", How::Max, size.1 as f64)
        .text("peak_kind", sys::PEAK_KIND)
        .with_detail(json!({ "text": text }))
}

fn child_private(args: &[String]) -> Result<Record, String> {
    sys::granted()?;
    let (normal, private, loaded) = sys::private_read(
        pid_of(args)?,
        &need(args, "--normal")?,
        &need(args, "--private")?,
    )?;
    let attribute = private.hits.iter().any(|hit| !normal.hits.contains(hit));
    let class = !private.class.is_empty() && private.class != normal.class;
    let title = sys::marked(&private.title) && !sys::marked(&normal.title);
    Ok(Record::new("", 0).with_detail(json!({
        "attribute": attribute,
        "class": class,
        "title": title,
        "private_page_loaded": loaded,
        "normal": normal,
        "private": private,
    })))
}

fn child_count(args: &[String]) -> Result<Record, String> {
    let ms: u64 = need(args, "--ms")?
        .parse()
        .map_err(|_| "the time is not a number".to_string())?;
    let counted = sys::count_input(
        ms,
        Path::new(&need(args, "--ready")?),
        Path::new(&need(args, "--stop")?),
    )?;
    Ok(Record::new("", 0).with_detail(json!({
        "keys": counted[0],
        "clicks": counted[1],
        "no_device_keys": counted[2],
        "no_device_clicks": counted[3],
    })))
}

pub fn child(args: &[String]) -> i32 {
    let result = match args.first().map(String::as_str) {
        Some("address") => child_address(args),
        Some("accessibility") => child_accessibility(args),
        Some("extract") => child_extract(args),
        Some("private") => child_private(args),
        Some("count-input") => child_count(args),
        Some("frame") => need(args, "--out").and_then(|path| {
            let (frame, _) = sys::capture()?;
            frame.save_png(Path::new(&path))?;
            Ok(Record::new("", 0))
        }),
        Some("press-menu") => pid_of(args)
            .and_then(|pid| {
                sys::granted()?;
                sys::press_menu(pid, &values(args, "--path"))
            })
            .map(|()| Record::new("", 0)),
        _ => Err("no such child".to_string()),
    };
    print_answer(result);
    0
}

pub fn print_answer(result: Result<Record, String>) {
    let line = match result {
        Ok(record) => json!({ "ok": record }),
        Err(error) => json!({ "error": error }),
    };
    println!("{line}");
}
