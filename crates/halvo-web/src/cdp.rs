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
/// after its executor was gone (CP-148): `GuardAct::line`.
pub const GUARD_RECORD: &str = "guard_ended";
const POLL: Duration = Duration::from_millis(50);

/// How long the executor waits for the browser's first reply on its debugging channel before it
/// ends the browser's tree by the guard's rule (A11.1, CP-158): an organisation's setting
/// `RemoteDebuggingAllowed` off turns the pipe off, and the browser then runs and gives no reply
/// (CP-93 (4)). The trial's bound, taken from no measurement of a first start (CP-158 (2)).
pub const FIRST_REPLY_WAIT: Duration = Duration::from_secs(30);

/// The origin that the start asks for, which no run binds, so that the relay refuses it and records
/// the refusal before the agent acts (A11.1): `.invalid` resolves nowhere (RFC 6761), so a browser
/// whose route bypasses the relay sends nothing for it.
pub const PROBE_HOST: &str = "halvo-start-probe.invalid";

/// The port of `PROBE_HOST`'s origin.
pub const PROBE_PORT: u16 = 80;

/// How long the start waits for the relay to record the probe's refusal.
pub const PROBE_WAIT: Duration = Duration::from_secs(10);

/// A start that failed (A11.1, CP-165 (9) and (10)): the cause, a detail of the step for the
/// record, whether both directories that the start made are gone, and what the end of the run's
/// own processes found before it ended any and left after. The cause carries no words for a
/// person: the interface run words them. `no_channel` is a browser that lived and gave no first
/// reply within `FIRST_REPLY_WAIT`; `browser_start` one that could not start or ended before it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartFailure {
    pub cause: &'static str,
    pub detail: String,
    pub removed: bool,
    pub left_before: usize,
    pub left_after: usize,
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
    /// The browser's program, an absolute path.
    pub browser: PathBuf,
    /// The profile directory that the start makes, an absolute path that does not exist; its
    /// temporary directory is made beside it.
    pub profile: PathBuf,
    pub headless: bool,
    /// The relay that is the browser's only route: the start names the browser's first process to
    /// it, starts its guard, and asks for `PROBE_HOST` before the agent acts.
    pub relay: crate::relay::RelayHandle,
    /// The program that runs the relay's guard.
    pub guard: crate::guard::GuardProgram,
}

impl StartOptions {
    /// The mark of a run of these options that begins now (`checks::Mark`), with the margin of the
    /// clock's reading against a process's creation time.
    pub fn mark_now(&self) -> crate::checks::Mark {
        let since_us = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_micros() as u64)
            .saturating_sub(LEFTOVER_SLACK_US);
        crate::checks::Mark::new(&self.profile, &self.browser, since_us)
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

/// An executor that is dropped without `stop` ends its browser through the child that its start
/// made and closes its channel, as the end of the executor's process would.
impl Drop for Executor {
    fn drop(&mut self) {
        if !self.child.exited() {
            self.child.kill();
        }
        self.wire_task.abort();
        self.relay.trust(None);
        if self.ended.is_none() {
            self.ended = Some(crate::checks::end_runs_own(&self.mark));
        }
    }
}

/// Copies of the executor's two ends of the pipe, which keep the pipe from being freed while a
/// check after the stop reads who holds it; dropping it closes them.
pub struct PipeKeep(#[allow(dead_code)] std::sync::Arc<Vec<std::fs::File>>);

pub struct Executor {
    guard: start::Started,
    kept: std::sync::Arc<Vec<std::fs::File>>,
    relay: crate::relay::RelayHandle,
    root: crate::procs::Proc,
    child: start::Started,
    home: String,
    pipe_ids: start::PipeIds,
    first_reply: Duration,
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

/// The browser's command line after its path, every flag the executor's own: the product's flags
/// (`crate::PRODUCT_FLAGS`), the relay's flags, headless when asked, the profile, the debugging pipe
/// and the first page. A caller adds no flag, so none can name a port or a route beside the relay.
/// No flag names a bound site, and the product carries no mock keychain (A8.3).
pub fn launch_args(opts: &StartOptions) -> Vec<String> {
    let mut args: Vec<String> = crate::PRODUCT_FLAGS.iter().map(|f| f.to_string()).collect();
    args.extend(crate::relay::relay_flags(opts.relay.port()));
    if opts.headless {
        args.push("--headless=new".into());
    }
    args.push(format!("--user-data-dir={}", opts.profile.display()));
    args.push("--remote-debugging-pipe".into());
    args.push("about:blank".into());
    args
}

impl Executor {
    /// Starts the browser on its pipe (A11.1) with the agent's settings of U0.7b written into the
    /// new profile before the start (`crate::AGENT_LOCAL_STATE`, `crate::AGENT_PREFERENCES`) and
    /// every download refused after it, names its first process to the relay, starts the relay's
    /// guard, waits up to `FIRST_REPLY_WAIT` for its first reply, and asks for `PROBE_HOST` until
    /// the relay records the refusal. A start that fails ends every process that names the
    /// profile, removes the two directories that it made and reports both (`StartFailure`).
    /// Causes: `browser_start` (the browser could not be started or ended before its first
    /// reply), `guard` (no guard could be started), `no_channel` (the browser lived and gave no
    /// first reply in time; its tree is ended by the guard's rule), `no_route` (no refusal
    /// recorded), `start` (any other step).
    pub async fn start(opts: &StartOptions) -> Result<Executor, StartFailure> {
        let refuse = |f: Fault| StartFailure {
            cause: f.0,
            detail: f.1,
            removed: true,
            left_before: 0,
            left_after: 0,
        };
        if !opts.browser.is_absolute() || !opts.profile.is_absolute() {
            return Err(refuse(Fault::new("start", "relative_path")));
        }
        let (profile, temp) = session::make_dirs(&opts.profile).map_err(refuse)?;
        let made = session::RemoveOnDrop::new(&profile, &temp);
        let launched = async {
            write_agent_settings(&profile).map_err(|f| (f, Default::default()))?;
            let ex = Executor::launch(opts, &temp).await?;
            let deny = SetDownloadBehaviorParams::new(SetDownloadBehaviorBehavior::Deny);
            if let Err(f) = ex.wire.call(None, deny).await {
                let (_, ended) = ex.stop().await;
                return Err((Fault::new("start", f.1), ended));
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
            Err((f, ended)) => {
                let removed = session::remove_dirs(&profile, &temp).await;
                made.disarm();
                Err(StartFailure {
                    cause: f.0,
                    detail: f.1,
                    removed,
                    left_before: ended.before,
                    left_after: ended.after,
                })
            }
        }
    }

    async fn launch(
        opts: &StartOptions,
        temp: &Path,
    ) -> Result<Executor, (Fault, crate::checks::Ended)> {
        let early = |f: Fault| (f, crate::checks::Ended::default());
        let args: Vec<std::ffi::OsString> = launch_args(opts).into_iter().map(Into::into).collect();
        let env =
            start::browser_env(temp).ok_or(early(Fault::new("browser_start", "environment")))?;
        let dir = start::system_dir().ok_or(early(Fault::new("browser_start", "system_dir")))?;
        let spec = start::Spec {
            program: &opts.browser,
            args,
            env,
            dir: &dir,
        };
        let mark = opts.mark_now();
        let started_at = Instant::now();
        let on = {
            let held = start::lock();
            start::start_on_pipe(&held, &spec)
        };
        let on = on.map_err(|f| early(Fault::new("browser_start", f.to_string())))?;
        let mut copies = on.kept;
        if let (Ok(a), Ok(b)) = (on.from_browser.try_clone(), on.to_browser.try_clone()) {
            copies.push(a);
            copies.push(b);
        }
        let kept = std::sync::Arc::new(copies);
        let link = crate::pipe::PipeLink::new(on.from_browser, on.to_browser);
        let pipe_ids = on.ids;
        let mut child = on.child;
        let root = crate::procs::table()
            .ok()
            .and_then(|t| t.into_iter().find(|p| p.pid == child.pid()));
        let fail_early = |child: &mut start::Started, f: Fault| {
            child.kill();
            (f, crate::checks::end_runs_own(&mark))
        };
        let Some(root) = root else {
            return Err(fail_early(&mut child, Fault::new("browser_start", "table")));
        };
        opts.relay.trust(Some(root.clone()));
        let mut guard = match opts
            .relay
            .start_guard(&opts.guard, &root, &temp.join(GUARD_RECORD))
        {
            Ok(g) => g,
            Err(step) => {
                opts.relay.trust(None);
                return Err(fail_early(&mut child, Fault::new("guard", step)));
            }
        };
        let (wire, wire_task) = wire::Wire::open(link);
        let mut fail = |child: &mut start::Started, task: &JoinHandle<()>, f: Fault| {
            opts.relay.trust(None);
            guard.kill();
            child.kill();
            task.abort();
            (f, crate::checks::end_runs_own(&mark))
        };
        if wire
            .call_within(None, GetVersionParams::default(), FIRST_REPLY_WAIT)
            .await
            .is_err()
        {
            if child.exited() {
                let fault = Fault::new("browser_start", "the browser ended before its first reply");
                return Err(fail(&mut child, &wire_task, fault));
            }
            let tree = root.clone();
            let _ = tokio::task::spawn_blocking(move || crate::guard::end_tree_now(&tree)).await;
            return Err(fail(
                &mut child,
                &wire_task,
                Fault::new("no_channel", "no first reply within the bound"),
            ));
        }
        let first_reply = started_at.elapsed();
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
        let before = opts.relay.refusals_of(PROBE_HOST, PROBE_PORT);
        let _ = wire
            .call(
                Some(&home),
                NavigateParams::new(format!("http://{PROBE_HOST}:{PROBE_PORT}/")),
            )
            .await;
        let deadline = Instant::now() + PROBE_WAIT;
        while opts.relay.refusals_of(PROBE_HOST, PROBE_PORT) == before {
            if Instant::now() > deadline {
                return Err(fail(
                    &mut child,
                    &wire_task,
                    Fault::new("no_route", "no refusal of the probe recorded"),
                ));
            }
            tokio::time::sleep(POLL).await;
        }
        Ok(Executor {
            guard,
            kept,
            relay: opts.relay.clone(),
            root,
            child,
            home,
            pipe_ids,
            first_reply,
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

    /// Keeps the pipe from being freed until the value is dropped, for a check after the stop: a
    /// freed pipe's handles may name a new pipe of another process (macOS).
    pub fn keep_pipe(&self) -> PipeKeep {
        PipeKeep(self.kept.clone())
    }

    /// The browser's first process as the process table read it after the start: its number,
    /// creation time and group, which the runner's frame `browser` carries (A6.2).
    pub fn root(&self) -> &crate::procs::Proc {
        &self.root
    }

    /// The identity of the browser's pipe, which the checks of a session read (CP-89 (c)).
    pub fn pipe_ids(&self) -> &start::PipeIds {
        &self.pipe_ids
    }

    /// The time from the start of the browser's process to its first reply on the pipe (CP-158 (2)).
    pub fn first_reply(&self) -> Duration {
        self.first_reply
    }

    /// Whether the transport of the debugging channel has ended.
    pub fn channel_closed(&self) -> bool {
        self.wire.closed()
    }

    /// The process number of the browser's first process.
    pub fn pid(&self) -> u32 {
        self.root.pid
    }

    pub async fn version(&self) -> Res<String> {
        let v = self
            .wire
            .call(None, GetVersionParams::default())
            .await
            .map_err(|f| Fault::new("version", f.1))?;
        Ok(v.product)
    }

    /// The faults of the executor's own connection, which attaches to every target of the browser
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
        self.relay.trust(None);
        let deadline = Instant::now() + Duration::from_secs(20);
        while !self.guard.exited() && Instant::now() < deadline {
            tokio::time::sleep(POLL).await;
        }
        self.guard.kill();
        (vec![self.root.pid], ended)
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
pub(crate) mod tests {
    use super::*;

    /// The guard's program of the tests: this test binary run as `guard_entry`.
    pub(crate) fn test_guard() -> crate::guard::GuardProgram {
        crate::guard::GuardProgram {
            program: std::env::current_exe().unwrap(),
            prefix: vec![
                "cdp::tests::guard_entry".into(),
                "--exact".into(),
                "--quiet".into(),
                "--test-threads=1".into(),
                "--".into(),
            ],
        }
    }

    /// An absolute path on this system to a program that does not exist.
    pub(crate) fn nowhere(name: &str) -> PathBuf {
        crate::start::system_dir()
            .unwrap()
            .join("halvo-web-nonexistent")
            .join(name)
    }

    pub(crate) fn options_with(
        browser: PathBuf,
        profile: PathBuf,
        relay: crate::relay::RelayHandle,
    ) -> StartOptions {
        StartOptions {
            browser,
            profile,
            headless: true,
            relay,
            guard: test_guard(),
        }
    }

    /// The guard of the tests' starts, fenced (CP-161 F1): it runs the guard's loop with the real
    /// table, and its ask and its forced end reach only a process whose program lies under the
    /// target directory's `tmp`, where the tests put their stand-ins; any other call ends this
    /// guard with exit 9 and sends nothing.
    #[test]
    fn guard_entry() {
        let args: Vec<String> = std::env::args().skip_while(|a| a != "--").skip(1).collect();
        if args.first().map(String::as_str) != Some(crate::guard::GUARD_FLAG) {
            return;
        }
        let exe = std::env::current_exe().unwrap();
        let fence = std::fs::canonicalize(exe.ancestors().nth(3).unwrap().join("tmp")).unwrap();
        let num = |i: usize| args.get(i).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
        let start = Instant::now();
        let code = crate::guard::guard_loop(
            num(1) as u32,
            num(2),
            crate::guard::Trigger::ExecutorGone {
                pid: num(3) as u32,
                started_us: num(4),
            },
            crate::guard::GuardIo {
                read: &mut || crate::procs::table().ok(),
                now_ms: &mut || start.elapsed().as_millis(),
                pause: &mut |_| std::thread::sleep(crate::guard::POLL),
                end: &mut |p, force| {
                    if !p.exe.as_deref().is_some_and(|e| e.starts_with(&fence)) {
                        std::process::exit(9);
                    }
                    if force {
                        crate::procs::kill(p)
                    } else {
                        crate::procs::ask_end(p)
                    }
                },
                record: &mut |_| {},
            },
        );
        std::process::exit(code);
    }

    #[tokio::test]
    async fn start_refuses_a_used_profile_directory_and_a_relative_path() {
        let dir = crate::child::scratch("used-profile");
        let relay = crate::relay::Relay::start(crate::relay::Names::Fixture)
            .await
            .unwrap();
        let profile = dir.join("p");
        std::fs::create_dir_all(&profile).unwrap();
        let used = Executor::start(&options_with(
            crate::cdp::tests::nowhere("chrome"),
            profile,
            relay.handle(),
        ))
        .await
        .err()
        .map(|f| f.cause);
        assert_eq!(used, Some("start"));
        let relative = Executor::start(&options_with(
            PathBuf::from("chrome"),
            dir.join("q"),
            relay.handle(),
        ))
        .await
        .err()
        .map(|f| (f.cause, f.detail));
        assert_eq!(relative, Some(("start", "relative_path".to_string())));
        assert!(!dir.join("q").exists());
    }

    /// The review of 85b6d770 (finder a, cases 1 and 2; finder b, case 1): the command line is the
    /// executor's alone, in a closed list, so no flag of a caller names a port or a route beside
    /// the relay, and the relay's proxy is the one value of its switches.
    #[test]
    fn the_command_line_is_the_executor_s_closed_list() {
        let relay = crate::relay::RelayHandle::for_tests(18999);
        let opts = options_with(
            PathBuf::from("/b/chrome"),
            PathBuf::from("/p/profile"),
            relay,
        );
        let mut want: Vec<String> = crate::PRODUCT_FLAGS.iter().map(|f| f.to_string()).collect();
        want.extend(crate::relay::relay_flags(18999));
        want.extend(
            [
                "--headless=new",
                "--user-data-dir=/p/profile",
                "--remote-debugging-pipe",
                "about:blank",
            ]
            .map(String::from),
        );
        assert_eq!(launch_args(&opts), want);
        let args = launch_args(&opts);
        for switch in ["--proxy-server", "--proxy-bypass-list", "--no-proxy-server"] {
            let n = args
                .iter()
                .filter(|a| a.split('=').next() == Some(switch))
                .count();
            assert_eq!(n, usize::from(switch != "--no-proxy-server"), "{switch}");
        }
        assert!(!args.iter().any(|a| crate::checks::names_a_port(a)));
        assert!(!args.iter().any(|a| a.contains("mock-keychain")));
        assert!(!args.iter().any(|a| a.contains("host-resolver-rules")));
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

    /// This process's creation time, the floor of what the tests below count as a run's own.
    #[cfg(target_os = "macos")]
    fn own_start_us() -> u64 {
        crate::procs::own().unwrap().started_us
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

    /// Ends what a test left that names its profile, each a stand-in of the test by its program.
    #[cfg(target_os = "macos")]
    fn end_left(left: &[crate::procs::Proc], dir: &Path) {
        let dir = std::fs::canonicalize(dir).unwrap();
        for p in left {
            if p.exe.as_deref().is_some_and(|e| e.starts_with(&dir)) {
                crate::procs::kill(p);
            }
        }
    }

    #[cfg(target_os = "macos")]
    async fn stand_in_start(
        name: &str,
        mode: &str,
        guard: Option<crate::guard::GuardProgram>,
    ) -> (
        Result<Executor, StartFailure>,
        PathBuf,
        PathBuf,
        crate::relay::Relay,
    ) {
        let dir = crate::child::scratch(name);
        let stand_in = crate::standin::bundle(&dir);
        let browser = crate::standin::launcher(stand_in.binary.parent().unwrap(), mode);
        let relay = crate::relay::Relay::start(crate::relay::Names::Fixture)
            .await
            .unwrap();
        let profile = dir.join("p");
        let mut opts = options_with(browser, profile.clone(), relay.handle());
        if let Some(g) = guard {
            opts.guard = g;
        }
        (Executor::start(&opts).await, profile, dir, relay)
    }

    /// CP-126 E1 and point 42: a failed start ends every process that names the profile: the
    /// stand-in leaves a helper and ends before its first reply, which is `browser_start`, kept
    /// apart from `no_channel` (CP-165 (9)); the failure reports both directories removed.
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn a_failed_start_ends_a_process_that_the_browser_left_with_its_profile() {
        let since_us = own_start_us();
        let (started, profile, dir, _relay) = stand_in_start("leftover", "leave", None).await;
        let left = naming(&profile, since_us);
        end_left(&left, &dir);
        let failed = started.err().expect("the start failed");
        assert_eq!(failed.cause, "browser_start", "{failed:?}");
        assert!(failed.removed, "{failed:?}");
        assert_eq!(left.len(), 0, "processes left that name the profile");
    }

    /// The review's point 3, finding (3): a start for which no guard could be started fails with
    /// the cause `guard`, ends its browser and leaves no process that names the profile.
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn a_start_with_no_guard_fails_and_leaves_no_browser() {
        let since_us = own_start_us();
        let none = crate::guard::GuardProgram {
            program: crate::cdp::tests::nowhere("guard"),
            prefix: Vec::new(),
        };
        let (started, profile, dir, _relay) =
            stand_in_start("no-guard", "browser", Some(none)).await;
        let left = named_within(&profile, since_us, Duration::from_millis(500)).await;
        end_left(&left, &dir);
        let failed = started.err().expect("the start went on with no guard");
        assert_eq!(failed.cause, "guard", "{failed:?}");
        assert!(failed.removed);
        assert_eq!(left.len(), 0);
    }

    /// CP-158 (1), CP-165 (9): a browser that lives and gives no first reply within the bound is
    /// `no_channel`, and its tree is ended by the guard's rule; nothing that names the profile is
    /// left, nor its helper, which names no profile. The stand-in holds the pipe and answers
    /// nothing.
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn a_start_with_no_first_reply_ends_its_tree_and_is_no_channel() {
        let since_us = own_start_us();
        let (started, profile, dir, _relay) = stand_in_start("no-reply", "hang", None).await;
        let left = named_within(&profile, since_us, Duration::from_millis(500)).await;
        end_left(&left, &dir);
        let bundle = std::fs::canonicalize(&dir).unwrap();
        let helpers: Vec<crate::procs::Proc> = crate::procs::table()
            .unwrap()
            .into_iter()
            .filter(|p| p.exe.as_deref().is_some_and(|e| e.starts_with(&bundle)))
            .collect();
        end_left(&helpers, &dir);
        let failed = started.err().expect("the start failed");
        assert_eq!(failed.cause, "no_channel", "{failed:?}");
        assert!(failed.removed);
        assert_eq!(left.len(), 0);
        assert_eq!(helpers.len(), 0, "a helper of the tree was left");
    }

    /// CP-126 E1 (1): a stand-in browser that answers the start and the stop on its pipe starts,
    /// when it is asked to close, a process that names the profile and starts another one 300 ms
    /// later, after the browser has ended. After the stop none of them is left. The stand-in asks
    /// for the start's probe 500 ms after the navigation, and the start returned only after the
    /// relay recorded its refusal.
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn a_normal_stop_ends_a_process_that_appears_after_the_browser_ended() {
        let since_us = own_start_us();
        let (started, profile, dir, relay) =
            stand_in_start("late", "browser_slow_probe", None).await;
        let ex = started.expect("the stand-in starts");
        assert_eq!(relay.handle().refusals_of(PROBE_HOST, PROBE_PORT), 1);
        let stopped = ex.stop_and_remove().await;
        let left = named_within(&profile, since_us, Duration::from_secs(2)).await;
        end_left(&left, &dir);
        assert!(stopped.removed);
        assert_eq!(
            left.len(),
            0,
            "processes left after the stop that name the profile"
        );
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

    #[test]
    fn an_amount_is_digits_one_point_and_two_decimals() {
        assert_eq!(crate::extract_amount("12.34"), Some(1234));
        assert_eq!(crate::extract_amount("0.05"), Some(5));
        for bad in ["12.3", "12", ".34", "1.234", "1,00", "-1.00", "1.0a", ""] {
            assert_eq!(crate::extract_amount(bad), None, "{bad}");
        }
    }
}
