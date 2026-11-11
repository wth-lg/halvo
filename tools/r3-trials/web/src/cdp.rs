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
use chromiumoxide::cdp::browser_protocol::fetch::{
    ContinueRequestParams, EnableParams as FetchEnableParams, EventRequestPaused,
    FailRequestParams, RequestPattern, RequestStage,
};
use chromiumoxide::cdp::browser_protocol::input::{
    DispatchKeyEventParams, DispatchKeyEventType, DispatchMouseEventParams, DispatchMouseEventType,
    InsertTextParams, MouseButton,
};
use chromiumoxide::cdp::browser_protocol::network::{ErrorReason, LoaderId};
use chromiumoxide::cdp::browser_protocol::page::{
    AddScriptToEvaluateOnNewDocumentParams, CreateIsolatedWorldParams, FrameId, FrameTree,
    GetFrameTreeParams, NavigateParams,
};
use chromiumoxide::cdp::browser_protocol::target::TargetId;
use chromiumoxide::cdp::js_protocol::runtime::{
    AddBindingParams, CallArgument, CallFunctionOnParams, EvaluateParams, EventBindingCalled,
    ExecutionContextId, RemoteObjectId,
};
use chromiumoxide::{Browser, Page};
use futures::StreamExt;
use tokio::task::JoinHandle;

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

#[derive(Clone, Debug)]
struct Target {
    frame: FrameId,
    node: BackendNodeId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Continued,
    Failed,
    Refused,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PausedRequest {
    pub method: String,
    pub origin: String,
    pub path: String,
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

fn body_of(request: &chromiumoxide::cdp::browser_protocol::network::Request) -> (Vec<u8>, bool) {
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
    paused: Arc<Mutex<Vec<PausedRequest>>>,
    rule: Option<JoinHandle<()>>,
    recorded: Arc<Mutex<Vec<String>>>,
    recorder: Option<JoinHandle<()>>,
    owns_process: bool,
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
        let used = std::fs::read_dir(&opts.profile)
            .map(|mut entries| entries.next().is_some())
            .unwrap_or(false);
        if used {
            return Err(Fault::new("start", "profile directory is not empty"));
        }
        let mut builder = BrowserConfig::builder()
            .chrome_executable(&opts.browser)
            .disable_default_args()
            .user_data_dir(&opts.profile)
            .port(opts.debug_port.unwrap_or(0))
            .launch_timeout(Duration::from_secs(30))
            .request_timeout(STEP_TIMEOUT);
        builder = if opts.headless {
            builder.new_headless_mode()
        } else {
            builder.with_head()
        };
        for flag in launch_args(opts) {
            let bare = flag.trim_start_matches('-');
            builder = match bare.split_once('=') {
                Some((k, v)) => builder.arg((k, v)),
                None => builder.arg(bare),
            };
        }
        let config = builder.build().map_err(|e| Fault::new("start", e))?;
        let (mut browser, mut handler) = Browser::launch(config).await.map_err(cdp("start"))?;
        let pid = browser.get_mut_child().and_then(|c| c.inner.id());
        let handler = tokio::spawn(async move { while handler.next().await.is_some() {} });
        let page = match browser.new_page("about:blank").await {
            Ok(p) => p,
            Err(e) => {
                let _ = browser.kill().await;
                return Err(Fault::new("start", e.to_string()));
            }
        };
        Ok(Executor::wrap(browser, page, handler, pid, true))
    }

    pub async fn connect(address: &str, url_prefix: &str) -> Res<Executor> {
        let (mut browser, mut handler) = Browser::connect(format!("http://{address}"))
            .await
            .map_err(cdp("attach"))?;
        let handler = tokio::spawn(async move { while handler.next().await.is_some() {} });
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
        let page = loop {
            match browser.get_page(target.clone()).await {
                Ok(p) => break p,
                Err(e) if Instant::now() > deadline => {
                    return Err(Fault::new("attach", e.to_string()))
                }
                Err(_) => tokio::time::sleep(POLL).await,
            }
        };
        Ok(Executor::wrap(browser, page, handler, None, false))
    }

    fn wrap(
        browser: Browser,
        page: Page,
        handler: JoinHandle<()>,
        pid: Option<u32>,
        owns_process: bool,
    ) -> Executor {
        Executor {
            browser,
            page,
            handler,
            pid,
            refs: Vec::new(),
            paused: Arc::new(Mutex::new(Vec::new())),
            rule: None,
            recorded: Arc::new(Mutex::new(Vec::new())),
            recorder: None,
            owns_process,
        }
    }

    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    pub async fn version(&self) -> Res<String> {
        let v = self.browser.version().await.map_err(cdp("version"))?;
        Ok(v.product)
    }

    pub async fn stop(mut self) -> Vec<u32> {
        if let Some(t) = self.rule.take() {
            t.abort();
        }
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
        self.handler.abort();
        self.pid.into_iter().collect()
    }

    async fn frame_tree(&self) -> Res<FrameTree> {
        let r = self
            .page
            .execute(GetFrameTreeParams::default())
            .await
            .map_err(cdp("frame"))?;
        Ok(r.result.frame_tree)
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
        let tree = self.frame_tree().await?;
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
            .page
            .execute(CreateIsolatedWorldParams::new(frame.clone()))
            .await
            .map_err(cdp("frame"))?;
        Ok(r.result.execution_context_id)
    }

    async fn eval_value(&self, frame: &FrameId, expression: &str) -> Res<serde_json::Value> {
        let ctx = self.world(frame).await?;
        let params = EvaluateParams::builder()
            .expression(expression)
            .context_id(ctx)
            .return_by_value(true)
            .build()
            .map_err(|e| Fault::new("eval", e))?;
        let r = self.page.execute(params).await.map_err(cdp("eval"))?;
        if r.result.exception_details.is_some() {
            return Err(Fault::new("eval", "exception"));
        }
        Ok(r.result.result.value.unwrap_or(serde_json::Value::Null))
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
            .page
            .execute(
                GetFullAxTreeParams::builder()
                    .frame_id(frame.clone())
                    .build(),
            )
            .await
            .map_err(cdp("snapshot"))?;
        Ok(r.result.nodes)
    }

    async fn object_of(&self, node: &BackendNodeId) -> Res<RemoteObjectId> {
        let r = self
            .page
            .execute(ResolveNodeParams::builder().backend_node_id(*node).build())
            .await
            .map_err(cdp("node"))?;
        r.result
            .object
            .object_id
            .ok_or_else(|| Fault::new("node", "no object"))
    }

    async fn call_on(
        &self,
        node: &BackendNodeId,
        function: &str,
        args: Vec<serde_json::Value>,
    ) -> Res<serde_json::Value> {
        let object = self.object_of(node).await?;
        let mut builder = CallFunctionOnParams::builder()
            .function_declaration(function)
            .object_id(object)
            .return_by_value(true);
        for a in args {
            builder = builder.argument(CallArgument::builder().value(a).build());
        }
        let params = builder.build().map_err(|e| Fault::new("call", e))?;
        let r = self.page.execute(params).await.map_err(cdp("call"))?;
        if r.result.exception_details.is_some() {
            return Err(Fault::new("call", "exception"));
        }
        Ok(r.result.result.value.unwrap_or(serde_json::Value::Null))
    }

    async fn is_credential(&self, node: &BackendNodeId) -> Res<bool> {
        let v = self.call_on(node, CREDENTIAL_FN, Vec::new()).await?;
        Ok(v == serde_json::Value::Bool(true))
    }

    async fn child_frame_of(&self, owner: &BackendNodeId) -> Res<FrameId> {
        let r = self
            .page
            .execute(
                DescribeNodeParams::builder()
                    .backend_node_id(*owner)
                    .build(),
            )
            .await
            .map_err(cdp("frame"))?;
        r.result
            .node
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
        let index: HashMap<String, usize> = nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.node_id.inner().clone(), i))
            .collect();
        let root = nodes
            .iter()
            .position(|n| n.parent_id.is_none())
            .ok_or_else(|| Fault::new("snapshot", "no root"))?;
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
                        self.is_credential(&b).await?
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
                        let child = self.child_frame_of(&b).await?;
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
            frame = self.child_frame_of(&owners[0]).await?;
            if !self.ready(&frame).await {
                return Ok(Err(Resolved::None));
            }
        }
        Ok(Ok(frame))
    }

    async fn by_test_id(&self, frame: &FrameId, id: &str) -> Res<Vec<BackendNodeId>> {
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
        let r = self.page.execute(params).await.map_err(cdp("eval"))?;
        let object = r
            .result
            .result
            .object_id
            .ok_or_else(|| Fault::new("eval", "no element"))?;
        let d = self
            .page
            .execute(DescribeNodeParams::builder().object_id(object).build())
            .await
            .map_err(cdp("node"))?;
        Ok(vec![d.result.node.backend_node_id])
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

    pub async fn click(&self, reference: &str) -> Res<()> {
        let t = self.target(reference)?;
        self.page
            .execute(
                ScrollIntoViewIfNeededParams::builder()
                    .backend_node_id(t.node)
                    .build(),
            )
            .await
            .map_err(cdp("click"))?;
        let quads = self
            .page
            .execute(
                GetContentQuadsParams::builder()
                    .backend_node_id(t.node)
                    .build(),
            )
            .await
            .map_err(cdp("click"))?;
        let q = quads
            .result
            .quads
            .first()
            .map(|q| q.inner().clone())
            .filter(|q| q.len() == 8)
            .ok_or_else(|| Fault::new("click", "no box"))?;
        let x = (q[0] + q[2] + q[4] + q[6]) / 4.0;
        let y = (q[1] + q[3] + q[5] + q[7]) / 4.0;
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
        self.page
            .execute(FocusParams::builder().backend_node_id(t.node).build())
            .await
            .map_err(cdp("focus"))?;
        Ok(())
    }

    pub async fn fill(&self, reference: &str, text: &str) -> Res<()> {
        let t = self.target(reference)?;
        self.focus(&t).await?;
        self.call_on(
            &t.node,
            "function () { if (this.select) this.select(); }",
            Vec::new(),
        )
        .await?;
        self.page
            .execute(InsertTextParams::new(text))
            .await
            .map_err(cdp("fill"))?;
        Ok(())
    }

    pub async fn select(&self, reference: &str, label: &str) -> Res<()> {
        let t = self.target(reference)?;
        let v = self
            .call_on(
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
            self.page.execute(params).await.map_err(cdp("press"))?;
        }
        Ok(())
    }

    pub async fn upload(&self, reference: &str, file: &Path) -> Res<()> {
        let t = self.target(reference)?;
        self.page
            .execute(
                SetFileInputFilesParams::builder()
                    .file(file.display().to_string())
                    .backend_node_id(t.node)
                    .build()
                    .map_err(|e| Fault::new("upload", e))?,
            )
            .await
            .map_err(cdp("upload"))?;
        Ok(())
    }

    pub async fn read(&self, reference: &str) -> Res<String> {
        let t = self.target(reference)?;
        let v = self.call_on(&t.node, READ_FN, Vec::new()).await?;
        v.as_str()
            .map(squeeze)
            .ok_or_else(|| Fault::new("read", "not text"))
    }

    pub async fn file_sizes(&self, reference: &str) -> Res<Vec<u64>> {
        let t = self.target(reference)?;
        let v = self
            .call_on(
                &t.node,
                "function () { return Array.from(this.files || []).map((f) => f.size); }",
                Vec::new(),
            )
            .await?;
        Ok(v.as_array()
            .map(|a| a.iter().filter_map(|n| n.as_u64()).collect())
            .unwrap_or_default())
    }

    pub async fn arm_rule(
        &mut self,
        allowed_origins: Vec<String>,
        state_changing: bool,
    ) -> Res<()> {
        let mut events = self
            .page
            .event_listener::<EventRequestPaused>()
            .await
            .map_err(cdp("rule"))?;
        let page = self.page.clone();
        let paused = self.paused.clone();
        if let Ok(mut p) = paused.lock() {
            p.clear();
        }
        if let Some(t) = self.rule.take() {
            t.abort();
        }
        self.rule = Some(tokio::spawn(async move {
            while let Some(ev) = events.next().await {
                let method = ev.request.method.clone();
                let (origin, path) = split_url(&ev.request.url);
                let (body, body_complete) = body_of(&ev.request);
                let changes = !matches!(method.as_str(), "GET" | "HEAD" | "OPTIONS");
                let let_through = allowed_origins.contains(&origin) && (state_changing || !changes);
                let id = ev.request_id.clone();
                let answered = if let_through {
                    page.execute(ContinueRequestParams::new(id))
                        .await
                        .map(|_| Outcome::Continued)
                } else {
                    page.execute(FailRequestParams::new(id, ErrorReason::BlockedByClient))
                        .await
                        .map(|_| Outcome::Failed)
                };
                if let Ok(mut p) = paused.lock() {
                    p.push(PausedRequest {
                        method,
                        origin,
                        path,
                        let_through,
                        outcome: answered.unwrap_or(Outcome::Refused),
                        body,
                        body_complete,
                    });
                }
            }
        }));
        let pattern = RequestPattern::builder()
            .url_pattern("*")
            .request_stage(RequestStage::Request)
            .build();
        self.page
            .execute(FetchEnableParams::builder().pattern(pattern).build())
            .await
            .map_err(cdp("rule"))?;
        Ok(())
    }

    pub fn paused(&self) -> Vec<PausedRequest> {
        self.paused.lock().map(|p| p.clone()).unwrap_or_default()
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
            Some("profile directory is not empty".to_string())
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
}
