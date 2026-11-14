use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use chromiumoxide::cdp::browser_protocol::fetch::{
    ContinueRequestParams, ContinueResponseParams, EnableParams as FetchEnableParams,
    EventRequestPaused, FailRequestParams, FulfillRequestParams, GetResponseBodyParams,
    HeaderEntry, RequestPattern, RequestStage,
};
use chromiumoxide::cdp::browser_protocol::network::{
    EnableParams as NetworkEnable, ErrorReason, ResourceType,
};
use chromiumoxide::cdp::browser_protocol::target::{
    ActivateTargetParams, SetAutoAttachParams, TargetId,
};
use chromiumoxide::cdp::js_protocol::runtime::RunIfWaitingForDebuggerParams;
use chromiumoxide::cdp::{CdpEvent, CdpEventMessage};
use chromiumoxide::types::{CallId, Command, Message, MethodId};
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use super::{body_of, split_url, Fault, Outcome, PausedRequest, Res, STEP_TIMEOUT};
use crate::pipe::PipeLink;

/// The transport of the wire: the pipe of the product's form, or, for the trial's port-form
/// control alone, the library's connection to a debugging port.
pub(crate) enum Link {
    Pipe(PipeLink),
    #[cfg(feature = "port-control")]
    Socket(Box<chromiumoxide::Connection<CdpEventMessage>>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Attached {
    pub target: String,
    pub kind: String,
    pub ready: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Rule {
    pub allowed: Vec<String>,
    pub state_changing: bool,
    pub hold: bool,
    pub policy: Option<String>,
}

impl Rule {
    pub(crate) fn lets_through(&self, method: &str, origin: &str) -> bool {
        self.allowed.iter().any(|a| a == origin) && (self.state_changing || !changes_state(method))
    }
}

pub(crate) fn changes_state(method: &str) -> bool {
    !matches!(method, "GET" | "HEAD" | "OPTIONS")
}

#[derive(Default)]
pub(crate) struct Book {
    pub sessions: HashMap<String, Attached>,
    pub faults: Vec<String>,
    pub rule: Option<Rule>,
    pub version: u64,
    pub home: Option<String>,
    pub paused: Vec<PausedRequest>,
    pub answered: HashSet<String>,
    pub on_main_document: Option<Hook>,
    pub policy_counts: std::collections::BTreeMap<String, u64>,
    pub storage_pages: HashSet<String>,
    pub only: Option<String>,
    pub bindings: Vec<(String, String)>,
    pub closed: bool,
}

struct Order {
    method: MethodId,
    session: Option<String>,
    params: Value,
    reply: oneshot::Sender<Result<Value, String>>,
}

/// A call of the runner's that the executor makes for a request that the rule lets through.
pub type Hook = std::sync::Arc<dyn Fn(&str, u16) + Send + Sync>;

/// The host and the port of an origin `scheme://host[:port]`, with the port of the scheme where it
/// has none.
pub fn host_port(origin: &str) -> Option<(String, u16)> {
    let (scheme, rest) = origin.split_once("://")?;
    let default = match scheme {
        "http" | "ws" => 80,
        "https" | "wss" => 443,
        _ => return None,
    };
    match rest.rsplit_once(':') {
        Some((h, p)) if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) => {
            Some((h.to_string(), p.parse().ok()?))
        }
        _ => Some((rest.to_string(), default)),
    }
}

#[derive(Clone)]
pub(crate) struct Wire {
    tx: mpsc::UnboundedSender<Order>,
    pub book: Arc<Mutex<Book>>,
}

pub(crate) fn auto_attach() -> Res<SetAutoAttachParams> {
    SetAutoAttachParams::builder()
        .auto_attach(true)
        .wait_for_debugger_on_start(true)
        .flatten(true)
        .build()
        .map_err(|e| Fault::new("wire", e))
}

fn nests(kind: &str) -> bool {
    matches!(kind, "page" | "iframe")
}

fn must_arm(kind: &str) -> bool {
    matches!(kind, "page" | "iframe")
}

fn intercept_all(policy: bool) -> FetchEnableParams {
    let mut b = FetchEnableParams::builder().pattern(
        RequestPattern::builder()
            .url_pattern("*")
            .request_stage(RequestStage::Request)
            .build(),
    );
    if policy {
        for kind in [
            ResourceType::Document,
            ResourceType::Script,
            ResourceType::Other,
        ] {
            b = b.pattern(
                RequestPattern::builder()
                    .url_pattern("*")
                    .resource_type(kind)
                    .request_stage(RequestStage::Response)
                    .build(),
            );
        }
    }
    b.build()
}

impl Wire {
    /// Starts the wire's task on `link`; nothing is sent yet.
    pub(crate) fn open(link: Link) -> (Wire, JoinHandle<()>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let wire = Wire {
            tx,
            book: Arc::new(Mutex::new(Book::default())),
        };
        let task = tokio::spawn(run(link, rx, wire.clone()));
        (wire, task)
    }

    /// Asks every target of the browser to wait for the wire before it runs, nested.
    pub(crate) async fn attach_all(&self) -> Res<()> {
        self.call(None, auto_attach()?).await.map(|_| ())
    }

    pub(crate) async fn call<C: Command>(&self, session: Option<&str>, cmd: C) -> Res<C::Response> {
        self.call_within(session, cmd, STEP_TIMEOUT).await
    }

    pub(crate) async fn call_within<C: Command>(
        &self,
        session: Option<&str>,
        cmd: C,
        wait: std::time::Duration,
    ) -> Res<C::Response> {
        let method = cmd.identifier();
        let params = serde_json::to_value(&cmd).map_err(|e| Fault::new("wire", e.to_string()))?;
        let value = self
            .raw_within(session, method.clone(), params, wait)
            .await?;
        C::response_from_value(value).map_err(|e| Fault::new("wire", e.to_string()))
    }

    pub(crate) async fn raw_within(
        &self,
        session: Option<&str>,
        method: MethodId,
        params: Value,
        wait: std::time::Duration,
    ) -> Res<Value> {
        let (reply, answer) = oneshot::channel();
        self.tx
            .send(Order {
                method: method.clone(),
                session: session.map(str::to_string),
                params,
                reply,
            })
            .map_err(|_| Fault::new("wire", "closed"))?;
        match tokio::time::timeout(wait, answer).await {
            Ok(Ok(Ok(v))) => Ok(v),
            Ok(Ok(Err(e))) => Err(Fault::new("wire", format!("{method}: {e}"))),
            Ok(Err(_)) => Err(Fault::new("wire", "closed")),
            Err(_) => Err(Fault::new("wire", format!("{method}: timeout"))),
        }
    }

    /// Whether the transport has ended.
    pub(crate) fn closed(&self) -> bool {
        self.book.lock().map(|b| b.closed).unwrap_or(true)
    }

    /// Answers every request for `url` with the executor's own storage page from now on.
    pub(crate) fn serve_storage_page(&self, url: &str, on: bool) {
        if let Ok(mut b) = self.book.lock() {
            if on {
                b.storage_pages.insert(url.to_string());
            } else {
                b.storage_pages.remove(url);
            }
        }
    }

    /// The payloads of every call of a binding named `name` so far.
    pub(crate) fn binding_calls(&self, name: &str) -> Vec<String> {
        self.book
            .lock()
            .map(|b| {
                b.bindings
                    .iter()
                    .filter(|(n, _)| n == name)
                    .map(|(_, p)| p.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(crate) fn session_of(&self, target: &str) -> Option<(String, String)> {
        let book = self.book.lock().ok()?;
        book.sessions
            .iter()
            .find(|(_, a)| a.target == target && a.ready)
            .map(|(s, a)| (s.clone(), a.kind.clone()))
    }

    pub(crate) fn faults(&self) -> Vec<String> {
        self.book
            .lock()
            .map(|b| b.faults.clone())
            .unwrap_or_default()
    }

    pub(crate) fn paused(&self) -> Vec<PausedRequest> {
        self.book
            .lock()
            .map(|b| b.paused.clone())
            .unwrap_or_default()
    }

    pub(crate) fn set_main_document_hook(&self, hook: Option<Hook>) {
        if let Ok(mut b) = self.book.lock() {
            b.on_main_document = hook;
        }
    }

    pub(crate) fn set_home(&self, target: &str) {
        if let Ok(mut b) = self.book.lock() {
            b.home = Some(target.to_string());
        }
    }

    fn fault(&self, step: &str, session: &str, kind: &str, f: &Fault) {
        if let Ok(mut b) = self.book.lock() {
            let held = if b.sessions.contains_key(session) {
                "attached"
            } else {
                "gone"
            };
            b.faults.push(format!("{step}:{kind}:{}:{held}", f.0));
        }
    }

    fn rule(&self) -> Option<Rule> {
        self.book.lock().ok().and_then(|b| b.rule.clone())
    }

    async fn arm(&self, session: &str, kind: &str) -> Res<()> {
        let policy = self.rule().map(|r| r.policy.is_some()).unwrap_or(false);
        match self.call(Some(session), intercept_all(policy)).await {
            Ok(_) => Ok(()),
            Err(f) if must_arm(kind) => Err(f),
            Err(f) => {
                self.fault("arm_other", session, kind, &f);
                Ok(())
            }
        }
    }

    /// Arms the rule on the one target of `session` and on no other target and not at the browser
    /// level: a control of the trial, which no operation of A11.1 does.
    pub(crate) async fn arm_one(&self, session: &str, rule: Rule) -> Res<()> {
        let policy = rule.policy.is_some();
        {
            let mut b = self.book.lock().map_err(|_| Fault::new("rule", "book"))?;
            b.rule = Some(rule);
            b.only = Some(session.to_string());
            b.version += 1;
            b.paused.clear();
            b.answered.clear();
        }
        self.call(Some(session), intercept_all(policy))
            .await
            .map(|_| ())
            .map_err(|f| Fault::new("rule", f.1))
    }

    /// Arms the rule at the browser level and on every target that the connection holds, and from
    /// then on on every new target before it runs. Neither level alone sees every request: the
    /// browser level misses keepalive requests and beacons, and a target's own level misses the
    /// first requests of a frame that runs before the target is prepared. The paused requests of
    /// an earlier rule are dropped.
    pub(crate) async fn arm_rule(&self, rule: Rule) -> Res<()> {
        let sessions: Vec<(String, String)> = {
            let mut b = self.book.lock().map_err(|_| Fault::new("rule", "book"))?;
            b.rule = Some(rule);
            b.only = None;
            b.version += 1;
            b.paused.clear();
            b.answered.clear();
            b.sessions
                .iter()
                .filter(|(_, a)| a.ready)
                .map(|(s, a)| (s.clone(), a.kind.clone()))
                .collect()
        };
        let policy = self.rule().map(|r| r.policy.is_some()).unwrap_or(false);
        self.call(None, intercept_all(policy))
            .await
            .map_err(|f| Fault::new("rule", format!("kind=browser {}", f.1)))?;
        for (session, kind) in sessions {
            if let Err(f) = self.arm(&session, &kind).await {
                let attached = self
                    .book
                    .lock()
                    .map(|b| b.sessions.contains_key(&session))
                    .unwrap_or(true);
                if attached {
                    return Err(Fault::new("rule", format!("kind={kind} {}", f.1)));
                }
            }
        }
        Ok(())
    }

    fn on_event(&self, ev: CdpEventMessage) {
        let from = ev.session_id.clone();
        match ev.params {
            CdpEvent::TargetAttachedToTarget(a) => {
                let session = a.session_id.inner().clone();
                let kind = a.target_info.r#type.clone();
                if let Ok(mut b) = self.book.lock() {
                    b.sessions.insert(
                        session.clone(),
                        Attached {
                            target: a.target_info.target_id.inner().clone(),
                            kind: kind.clone(),
                            ready: false,
                        },
                    );
                }
                let wire = self.clone();
                let waiting = a.waiting_for_debugger;
                let opener = a.target_info.opener_id.as_ref().map(|o| o.inner().clone());
                tokio::spawn(async move { wire.prepare(session, kind, waiting, opener).await });
            }
            CdpEvent::TargetDetachedFromTarget(d) => {
                if let Ok(mut b) = self.book.lock() {
                    b.sessions.remove(d.session_id.inner());
                }
            }
            CdpEvent::FetchRequestPaused(p) => {
                let wire = self.clone();
                tokio::spawn(async move { wire.decide(from, *p).await });
            }
            CdpEvent::RuntimeBindingCalled(c) => {
                if let Ok(mut b) = self.book.lock() {
                    b.bindings.push((c.name.clone(), c.payload.clone()));
                }
            }
            _ => {}
        }
    }

    async fn respond(&self, session: Option<String>, ev: EventRequestPaused, code: i64) {
        let policy = self.rule().and_then(|r| r.policy);
        let mut headers = ev.response_headers.clone().unwrap_or_default();
        if let Some(p) = &policy {
            headers.push(HeaderEntry::new("Content-Security-Policy", p.clone()));
        }
        let id = ev.request_id.clone();
        if matches!(ev.resource_type, ResourceType::Document) {
            if let Ok(b) = self
                .call(session.as_deref(), GetResponseBodyParams::new(id.clone()))
                .await
            {
                use base64::Engine;
                let body = if b.base64_encoded {
                    b.body
                } else {
                    base64::engine::general_purpose::STANDARD.encode(b.body.as_bytes())
                };
                let fulfil = FulfillRequestParams::builder()
                    .request_id(id.clone())
                    .response_code(code)
                    .response_headers(headers.clone())
                    .body(body)
                    .build();
                if let Ok(f) = fulfil {
                    if self.call(session.as_deref(), f).await.is_ok() {
                        self.count_policy("fulfilled");
                        return;
                    }
                }
            }
            self.count_policy("document_not_fulfilled");
        }
        let mut cont = ContinueResponseParams::new(id);
        cont.response_code = Some(code);
        cont.response_headers = Some(headers);
        if self.call(session.as_deref(), cont).await.is_ok() {
            self.count_policy("continued");
        } else {
            self.count_policy("not_continued");
        }
    }

    async fn serve(&self, session: Option<String>, ev: EventRequestPaused) {
        use base64::Engine;
        let body = base64::engine::general_purpose::STANDARD.encode(super::session::STORAGE_PAGE);
        let params = FulfillRequestParams::builder()
            .request_id(ev.request_id.clone())
            .response_code(200)
            .response_header(HeaderEntry::new("Content-Type", "text/html; charset=utf-8"))
            .body(body)
            .build();
        if let Ok(p) = params {
            let _ = self.call(session.as_deref(), p).await;
        }
    }

    fn count_policy(&self, what: &str) {
        if let Ok(mut b) = self.book.lock() {
            *b.policy_counts.entry(what.to_string()).or_insert(0) += 1;
        }
    }

    pub(crate) fn policy_counts(&self) -> std::collections::BTreeMap<String, u64> {
        self.book
            .lock()
            .map(|b| b.policy_counts.clone())
            .unwrap_or_default()
    }

    async fn decide(&self, session: Option<String>, ev: EventRequestPaused) {
        let storage = self
            .book
            .lock()
            .map(|b| b.storage_pages.contains(&ev.request.url))
            .unwrap_or(false);
        if storage {
            self.serve(session, ev).await;
            return;
        }
        if let Some(code) = ev.response_status_code {
            self.respond(session, ev, code).await;
            return;
        }
        let method = ev.request.method.clone();
        let (origin, path) = split_url(&ev.request.url);
        let (body, body_complete) = body_of(&ev.request);
        let worker_script = ev
            .request
            .headers
            .inner()
            .as_object()
            .map(|h| {
                h.iter()
                    .any(|(k, v)| k.eq_ignore_ascii_case("service-worker") && v == "script")
            })
            .unwrap_or(false);
        let let_through = !worker_script
            && self
                .rule()
                .map(|r| r.lets_through(&method, &origin))
                .unwrap_or(false);
        let id = ev.request_id.clone();
        let hold = self.rule().map(|r| r.hold).unwrap_or(false);
        let document = matches!(ev.resource_type, ResourceType::Document);
        if let_through && document {
            let (home, hook) = match self.book.lock() {
                Ok(b) => (b.home.clone(), b.on_main_document.clone()),
                Err(_) => (None, None),
            };
            if let (Some(home), Some(hook)) = (home, hook) {
                if home == *ev.frame_id.inner() {
                    if let Some((host, port)) = host_port(&origin) {
                        hook(&host, port);
                    }
                }
            }
        }
        let answered = if !let_through && hold {
            Ok(Outcome::Held)
        } else if let_through {
            self.call(session.as_deref(), ContinueRequestParams::new(id))
                .await
                .map(|_| Outcome::Continued)
        } else {
            self.call(
                session.as_deref(),
                FailRequestParams::new(id, ErrorReason::BlockedByClient),
            )
            .await
            .map(|_| Outcome::Failed)
        };
        if let Ok(mut b) = self.book.lock() {
            if let Some(network) = &ev.network_id {
                if !b
                    .answered
                    .insert(format!("{} {}", network.inner(), ev.request.url))
                {
                    return;
                }
            }
            b.paused.push(PausedRequest {
                method,
                origin,
                path,
                frame: ev.frame_id.inner().clone(),
                resource: format!("{:?}", ev.resource_type),
                let_through,
                outcome: answered.unwrap_or(Outcome::Refused),
                body,
                body_complete,
            });
        }
    }

    async fn prepare(&self, session: String, kind: String, waiting: bool, opener: Option<String>) {
        let mut nested = !nests(&kind);
        let home = loop {
            let (version, armed) = match self.book.lock() {
                Ok(b) => (b.version, b.rule.is_some() && b.only.is_none()),
                Err(_) => return,
            };
            if armed {
                if let Err(f) = self.arm(&session, &kind).await {
                    self.fault("arm", &session, &kind, &f);
                    return;
                }
            }
            if !nested {
                if must_arm(&kind) && opener.is_none() {
                    if let Err(f) = self.call(Some(&session), NetworkEnable::default()).await {
                        self.fault("network", &session, &kind, &f);
                        return;
                    }
                }
                let attached = match auto_attach() {
                    Ok(params) => self.call(Some(&session), params).await.map(|_| ()),
                    Err(f) => Err(f),
                };
                if let Err(f) = attached {
                    self.fault("attach", &session, &kind, &f);
                    return;
                }
                nested = true;
            }
            let Ok(mut b) = self.book.lock() else {
                return;
            };
            if b.version == version {
                if let Some(a) = b.sessions.get_mut(&session) {
                    a.ready = true;
                }
                break b.home.clone();
            }
        };
        if waiting {
            if let Err(f) = self
                .call(Some(&session), RunIfWaitingForDebuggerParams::default())
                .await
            {
                self.fault("resume", &session, &kind, &f);
            }
        }
        if let (Some(home), Some(opener)) = (home, opener) {
            if kind == "page" && home == opener {
                if let Err(f) = self
                    .call(None, ActivateTargetParams::new(TargetId::from(home)))
                    .await
                {
                    self.fault("front", &session, &kind, &f);
                }
            }
        }
    }
}

fn answer(
    pending: &mut HashMap<CallId, oneshot::Sender<Result<Value, String>>>,
    m: Message<CdpEventMessage>,
    wire: &Wire,
) {
    match m {
        Message::Response(r) => {
            if let Some(reply) = pending.remove(&r.id) {
                let answer = match (r.result, r.error) {
                    (_, Some(e)) => Err(e.message),
                    (Some(v), None) => Ok(v),
                    (None, None) => Ok(Value::Null),
                };
                let _ = reply.send(answer);
            }
        }
        Message::Event(ev) => wire.on_event(ev),
    }
}

async fn run(link: Link, orders: mpsc::UnboundedReceiver<Order>, wire: Wire) {
    match link {
        Link::Pipe(p) => run_pipe(p, orders, &wire).await,
        #[cfg(feature = "port-control")]
        Link::Socket(c) => run_socket(*c, orders, &wire).await,
    }
    if let Ok(mut b) = wire.book.lock() {
        b.closed = true;
    }
}

async fn run_pipe(mut link: PipeLink, mut orders: mpsc::UnboundedReceiver<Order>, wire: &Wire) {
    let mut pending: HashMap<CallId, oneshot::Sender<Result<Value, String>>> = HashMap::new();
    loop {
        tokio::select! {
            order = orders.recv() => match order {
                Some(o) => match link.submit(o.method, o.session, o.params) {
                    Ok(id) => {
                        pending.insert(id, o.reply);
                    }
                    Err(e) => {
                        let _ = o.reply.send(Err(e));
                    }
                },
                None => break,
            },
            message = link.reads.recv() => match message {
                Some(m) => answer(&mut pending, m, wire),
                None => break,
            },
        }
    }
}

#[cfg(feature = "port-control")]
async fn run_socket(
    mut conn: chromiumoxide::Connection<CdpEventMessage>,
    mut orders: mpsc::UnboundedReceiver<Order>,
    wire: &Wire,
) {
    use chromiumoxide::cdp::browser_protocol::target::SessionId;
    use futures::StreamExt;
    let mut pending: HashMap<CallId, oneshot::Sender<Result<Value, String>>> = HashMap::new();
    loop {
        tokio::select! {
            order = orders.recv() => match order {
                Some(o) => {
                    let session = o.session.map(SessionId::from);
                    match conn.submit_command(o.method, session, o.params) {
                        Ok(id) => {
                            pending.insert(id, o.reply);
                        }
                        Err(e) => {
                            let _ = o.reply.send(Err(e.to_string()));
                        }
                    }
                }
                None => break,
            },
            message = conn.next() => match message {
                Some(Ok(m)) => answer(&mut pending, m, wire),
                Some(Err(chromiumoxide::error::CdpError::Ws(_))) => break,
                Some(Err(_)) => {}
                None => break,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_port_reads_the_port_or_the_scheme_default() {
        assert_eq!(
            host_port("http://hop1.agent.test:18190"),
            Some(("hop1.agent.test".to_string(), 18190))
        );
        assert_eq!(
            host_port("https://utility.agent.test"),
            Some(("utility.agent.test".to_string(), 443))
        );
        assert_eq!(host_port("data:text/html,x"), None);
    }

    #[test]
    fn the_rule_passes_a_read_of_an_allowed_origin_and_nothing_else_when_it_holds_state() {
        let rule = Rule {
            allowed: vec!["http://utility.agent.test:18190".into()],
            state_changing: false,
            hold: false,
            policy: None,
        };
        assert!(rule.lets_through("GET", "http://utility.agent.test:18190"));
        assert!(rule.lets_through("HEAD", "http://utility.agent.test:18190"));
        assert!(!rule.lets_through("POST", "http://utility.agent.test:18190"));
        assert!(!rule.lets_through("GET", "http://evil.agent.test:18190"));
        assert!(!rule.lets_through("GET", "http://utility.agent.test:18191"));
        let open = Rule {
            state_changing: true,
            ..rule
        };
        assert!(open.lets_through("POST", "http://utility.agent.test:18190"));
        assert!(!open.lets_through("POST", "http://evil.agent.test:18190"));
    }

    #[test]
    fn a_page_and_a_frame_take_nested_attach_and_must_be_armed_and_a_worker_need_not() {
        for kind in ["page", "iframe"] {
            assert!(nests(kind) && must_arm(kind), "{kind}");
        }
        for kind in ["worker", "shared_worker", "service_worker", "browser_ui"] {
            assert!(!nests(kind) && !must_arm(kind), "{kind}");
        }
    }
}
