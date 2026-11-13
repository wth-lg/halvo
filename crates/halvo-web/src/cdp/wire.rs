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

use super::{split_url, Fault, HeldRequest, Mode, Outcome, PausedRequest, Res, STEP_TIMEOUT};
use crate::pipe::PipeLink;

/// The transport of the wire: the browser's debugging pipe, its one client.
pub(crate) type Link = PipeLink;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Attached {
    pub target: String,
    pub kind: String,
    pub ready: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Rule {
    pub allowed: Vec<String>,
    pub mode: Mode,
    pub policy: Option<String>,
}

/// The bound of the held requests of one arm (CP-176 A1 (b)): a new state-changing request beyond
/// it is failed.
pub const MAX_HELD: usize = 16;

/// How many state-changing requests of one arm the rule keeps by their number once failed, so
/// that the second pause of each follows the first; beyond it a failed request is not kept and its
/// second pause is failed on its own.
const KEPT_FAILED: usize = 1024;

/// One pause of a request as the browser gave it: the session that paused it (`None`: the
/// browser's level) and the interception's id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Pause {
    pub session: Option<String>,
    pub id: String,
}

/// The facts of one paused request that the rule reads.
#[derive(Clone, Debug, Default)]
pub(crate) struct Facts {
    /// The browser's number of the request (`networkId`), the same at both levels.
    pub network: Option<String>,
    /// Whether the pause is a hop of a redirect (`redirectedRequestId`).
    pub redirected: bool,
    pub method: String,
    pub url: String,
    pub origin: String,
    pub path: String,
    pub frame: String,
    pub worker_script: bool,
    /// The host and port that the runner's hook of the main document gets when the request is let
    /// through.
    pub hook: Option<(String, u16)>,
}

/// What the rule answers one pause.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Act {
    Continue,
    Fail,
    /// Held with no answer under the executor's id.
    Hold(u64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HoldState {
    Waiting,
    Released,
    Failed,
}

/// One state-changing request of an arm, by the browser's number, its address, its method and
/// whether it is a redirect's hop, with every pause of it.
struct Held {
    request: HeldRequest,
    key: (String, String, String, bool),
    state: HoldState,
    pauses: Vec<Pause>,
    hook: Option<(String, u16)>,
}

/// The state-changing requests of one arm (CP-176 A1): what is held, whether a release was made
/// since the arm, and the executor's next id, which is never given twice in its life.
#[derive(Default)]
pub(crate) struct Holds {
    held: Vec<Held>,
    released: bool,
    next: u64,
}

/// What a release answers: the pauses to let go on, the pauses to fail, the ids of the requests
/// that it failed, and the hook's host and port of a document that it let through.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Released {
    pub go: Vec<Pause>,
    pub fail: Vec<Pause>,
    pub failed: Vec<u64>,
    pub hook: Option<(String, u16)>,
}

impl Holds {
    /// The answer to one pause of `f` under `rule`, and whether the request is new (a record of its
    /// own). A request that changes no state passes to an allowed origin and is failed elsewhere.
    /// A state-changing request is failed outside the list and in `Mode::Fail`; in `Mode::Hold` it
    /// is held under a new id, unless the browser gave it no number, a release was made since the
    /// arm, or `MAX_HELD` requests wait, which fail it. A second pause of one request (the same
    /// number, address, method and hop) follows the answer of the first.
    pub(crate) fn on_pause(&mut self, rule: Option<&Rule>, f: &Facts, pause: Pause) -> (Act, bool) {
        let allowed = !f.worker_script && rule.is_some_and(|r| r.allowed.contains(&f.origin));
        if !changes_state(&f.method) {
            return (if allowed { Act::Continue } else { Act::Fail }, true);
        }
        let Some(network) = &f.network else {
            return (Act::Fail, true);
        };
        let key = (
            network.clone(),
            f.url.clone(),
            f.method.clone(),
            f.redirected,
        );
        if let Some(h) = self.held.iter_mut().find(|h| h.key == key) {
            let act = match h.state {
                HoldState::Waiting => {
                    h.pauses.push(pause);
                    Act::Hold(h.request.id)
                }
                HoldState::Released => Act::Continue,
                HoldState::Failed => Act::Fail,
            };
            return (act, false);
        }
        let waiting = self
            .held
            .iter()
            .filter(|h| h.state == HoldState::Waiting)
            .count();
        let holds = allowed
            && rule.is_some_and(|r| r.mode == Mode::Hold)
            && !self.released
            && waiting < MAX_HELD;
        self.next += 1;
        let request = HeldRequest {
            id: self.next,
            method: f.method.clone(),
            origin: f.origin.clone(),
            path: f.path.clone(),
            frame: f.frame.clone(),
        };
        let (state, act) = if holds {
            (HoldState::Waiting, Act::Hold(self.next))
        } else {
            (HoldState::Failed, Act::Fail)
        };
        if !holds && self.held.len() >= KEPT_FAILED {
            return (act, true);
        }
        self.held.push(Held {
            request,
            key,
            state,
            pauses: if holds { vec![pause] } else { Vec::new() },
            hook: f.hook.clone(),
        });
        (act, true)
    }

    /// Lets the held request `id` through, once: its pauses go on, and every other waiting request
    /// is failed, as is every state-changing request after it until the next arm. The test and the
    /// mark happen in one call under the caller's lock.
    pub(crate) fn release(&mut self, id: u64) -> Result<Released, &'static str> {
        if self.released {
            return Err("released");
        }
        let Some(at) = self
            .held
            .iter()
            .position(|h| h.request.id == id && h.state == HoldState::Waiting)
        else {
            return Err("not_held");
        };
        self.released = true;
        self.held[at].state = HoldState::Released;
        let go = std::mem::take(&mut self.held[at].pauses);
        let hook = self.held[at].hook.clone();
        let (mut fail, mut failed) = (Vec::new(), Vec::new());
        for h in self
            .held
            .iter_mut()
            .filter(|h| h.state == HoldState::Waiting)
        {
            h.state = HoldState::Failed;
            fail.append(&mut h.pauses);
            failed.push(h.request.id);
        }
        Ok(Released {
            go,
            fail,
            failed,
            hook,
        })
    }

    /// Fails the held request `id`.
    pub(crate) fn fail(&mut self, id: u64) -> Result<Vec<Pause>, &'static str> {
        let h = self
            .held
            .iter_mut()
            .find(|h| h.request.id == id && h.state == HoldState::Waiting)
            .ok_or("not_held")?;
        h.state = HoldState::Failed;
        Ok(std::mem::take(&mut h.pauses))
    }

    /// Ends the arm: every waiting request is failed and the release is over. Gives the pauses to
    /// fail.
    pub(crate) fn end_arm(&mut self) -> Vec<Pause> {
        let mut fail = Vec::new();
        for h in self
            .held
            .iter_mut()
            .filter(|h| h.state == HoldState::Waiting)
        {
            fail.append(&mut h.pauses);
        }
        self.held.clear();
        self.released = false;
        fail
    }

    /// The requests that wait, each once.
    pub(crate) fn waiting(&self) -> Vec<HeldRequest> {
        self.held
            .iter()
            .filter(|h| h.state == HoldState::Waiting)
            .map(|h| h.request.clone())
            .collect()
    }
}

/// Whether a request is a service worker's script by its headers, which the rule never lets
/// through (A4.2: no service worker runs in the agent's browser).
pub(crate) fn is_worker_script(headers: &serde_json::Value) -> bool {
    headers.as_object().is_some_and(|h| {
        h.iter()
            .any(|(k, v)| k.eq_ignore_ascii_case("service-worker") && v == "script")
    })
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
    pub requests: Vec<PausedRequest>,
    pub holds: Holds,
    pub answered: HashSet<String>,
    pub on_main_document: Option<Hook>,
    pub policy_counts: std::collections::BTreeMap<String, u64>,
    pub storage_pages: HashSet<String>,
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

    /// Every request that the rule paused, once each, with its answer.
    pub(crate) fn requests(&self) -> Vec<PausedRequest> {
        self.book
            .lock()
            .map(|b| b.requests.clone())
            .unwrap_or_default()
    }

    /// The state-changing requests that are held now.
    pub(crate) fn held(&self) -> Vec<HeldRequest> {
        self.book
            .lock()
            .map(|b| b.holds.waiting())
            .unwrap_or_default()
    }

    /// Sets the outcome of the records of the requests `ids`.
    fn mark(&self, ids: &[(u64, Outcome)]) {
        if let Ok(mut b) = self.book.lock() {
            for r in b.requests.iter_mut() {
                if let Some((_, o)) = ids.iter().find(|(id, _)| r.id == Some(*id)) {
                    r.outcome = *o;
                    r.let_through = *o == Outcome::Continued;
                }
            }
        }
    }

    /// Answers `pauses`: each goes on (`go`) or is failed.
    async fn answer_pauses(&self, pauses: Vec<Pause>, go: bool) -> bool {
        let mut all = true;
        for p in pauses {
            let id = chromiumoxide::cdp::browser_protocol::fetch::RequestId::from(p.id);
            let done = if go {
                self.call(p.session.as_deref(), ContinueRequestParams::new(id))
                    .await
                    .is_ok()
            } else {
                self.call(
                    p.session.as_deref(),
                    FailRequestParams::new(id, ErrorReason::BlockedByClient),
                )
                .await
                .is_ok()
            };
            all &= done;
        }
        all
    }

    /// Lets the held request `id` through once and fails every other held request (CP-176 A1 (b)).
    pub(crate) async fn release(&self, id: u64) -> Res<()> {
        let (
            Released {
                go,
                fail,
                failed: others,
                hook,
            },
            runner_hook,
        ) = {
            let mut b = self.book.lock().map_err(|_| Fault::new("rule", "book"))?;
            let released = b
                .holds
                .release(id)
                .map_err(|cause| Fault::new("let_through", cause))?;
            (released, b.on_main_document.clone())
        };
        if let (Some((host, port)), Some(h)) = (hook, runner_hook) {
            h(&host, port);
        }
        let went = self.answer_pauses(go, true).await;
        self.answer_pauses(fail, false).await;
        let mut marks: Vec<(u64, Outcome)> =
            others.into_iter().map(|o| (o, Outcome::Failed)).collect();
        marks.push((
            id,
            if went {
                Outcome::Continued
            } else {
                Outcome::Refused
            },
        ));
        self.mark(&marks);
        if went {
            Ok(())
        } else {
            Err(Fault::new("let_through", "browser"))
        }
    }

    /// Fails the held request `id`.
    pub(crate) async fn fail_held(&self, id: u64) -> Res<()> {
        let fail = {
            let mut b = self.book.lock().map_err(|_| Fault::new("rule", "book"))?;
            b.holds
                .fail(id)
                .map_err(|cause| Fault::new("fail", cause))?
        };
        self.answer_pauses(fail, false).await;
        self.mark(&[(id, Outcome::Failed)]);
        Ok(())
    }

    /// Fails every held request at once with no wait for the browser's answer, for the stop and the
    /// drop of the executor (CP-176 A1 (b)).
    pub(crate) fn fail_all_held_now(&self) {
        let (fail, ids) = match self.book.lock() {
            Ok(mut b) => {
                let ids: Vec<u64> = b.holds.waiting().iter().map(|h| h.id).collect();
                (b.holds.end_arm(), ids)
            }
            Err(_) => return,
        };
        for p in fail {
            let id = chromiumoxide::cdp::browser_protocol::fetch::RequestId::from(p.id);
            let params = FailRequestParams::new(id, ErrorReason::BlockedByClient);
            if let Ok(value) = serde_json::to_value(&params) {
                let (reply, _) = oneshot::channel();
                let _ = self.tx.send(Order {
                    method: chromiumoxide::types::Method::identifier(&params),
                    session: p.session,
                    params: value,
                    reply,
                });
            }
        }
        self.mark(
            &ids.into_iter()
                .map(|i| (i, Outcome::Failed))
                .collect::<Vec<_>>(),
        );
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

    /// Arms the rule at the browser level and on every target that the connection holds, and from
    /// then on on every new target before it runs. Neither level alone sees every request: the
    /// browser level misses keepalive requests and beacons, and a target's own level misses the
    /// first requests of a frame that runs before the target is prepared. The paused requests of
    /// an earlier rule are dropped from the record, and every request that it held is failed
    /// (CP-176 A1 (b)).
    pub(crate) async fn arm_rule(&self, rule: Rule) -> Res<()> {
        let (sessions, fail): (Vec<(String, String)>, Vec<Pause>) = {
            let mut b = self.book.lock().map_err(|_| Fault::new("rule", "book"))?;
            b.rule = Some(rule);
            b.version += 1;
            b.requests.clear();
            b.answered.clear();
            let fail = b.holds.end_arm();
            (
                b.sessions
                    .iter()
                    .filter(|(_, a)| a.ready)
                    .map(|(s, a)| (s.clone(), a.kind.clone()))
                    .collect(),
                fail,
            )
        };
        self.answer_pauses(fail, false).await;
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
        let id = ev.request_id.clone();
        let document = matches!(ev.resource_type, ResourceType::Document);
        let worker_script = is_worker_script(ev.request.headers.inner());
        let (act, new, runner_hook, home) = {
            let Ok(mut b) = self.book.lock() else {
                return;
            };
            let home = b.home.clone();
            let facts = Facts {
                network: ev.network_id.as_ref().map(|n| n.inner().clone()),
                redirected: ev.redirected_request_id.is_some(),
                method: method.clone(),
                url: ev.request.url.clone(),
                origin: origin.clone(),
                path: path.clone(),
                frame: ev.frame_id.inner().clone(),
                worker_script,
                hook: (document && home.as_deref() == Some(ev.frame_id.inner().as_str()))
                    .then(|| host_port(&origin))
                    .flatten(),
            };
            let rule = b.rule.clone();
            let pause = Pause {
                session: session.clone(),
                id: id.inner().clone(),
            };
            let (act, new) = b.holds.on_pause(rule.as_ref(), &facts, pause);
            if let (Act::Hold(h), true) = (act, new) {
                b.requests.push(PausedRequest {
                    id: Some(h),
                    method: method.clone(),
                    origin: origin.clone(),
                    path: path.clone(),
                    frame: facts.frame.clone(),
                    resource: format!("{:?}", ev.resource_type),
                    let_through: false,
                    outcome: Outcome::Held,
                });
            }
            (act, new, b.on_main_document.clone(), facts.hook)
        };
        if act == Act::Continue {
            if let (Some((host, port)), Some(hook)) = (home, runner_hook) {
                hook(&host, port);
            }
        }
        let answered = match act {
            Act::Hold(_) => Ok(Outcome::Held),
            Act::Continue => self
                .call(session.as_deref(), ContinueRequestParams::new(id))
                .await
                .map(|_| Outcome::Continued),
            Act::Fail => self
                .call(
                    session.as_deref(),
                    FailRequestParams::new(id, ErrorReason::BlockedByClient),
                )
                .await
                .map(|_| Outcome::Failed),
        };
        if !new || matches!(act, Act::Hold(_)) {
            return;
        }
        if let Ok(mut b) = self.book.lock() {
            if !changes_state(&method) {
                if let Some(network) = &ev.network_id {
                    if !b
                        .answered
                        .insert(format!("{} {}", network.inner(), ev.request.url))
                    {
                        return;
                    }
                }
            }
            b.requests.push(PausedRequest {
                id: None,
                method,
                origin,
                path,
                frame: ev.frame_id.inner().clone(),
                resource: format!("{:?}", ev.resource_type),
                let_through: act == Act::Continue,
                outcome: answered.unwrap_or(Outcome::Refused),
            });
        }
    }

    async fn prepare(&self, session: String, kind: String, waiting: bool, opener: Option<String>) {
        let mut nested = !nests(&kind);
        let home = loop {
            let (version, armed) = match self.book.lock() {
                Ok(b) => (b.version, b.rule.is_some()),
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
    run_pipe(link, orders, &wire).await;
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

#[cfg(test)]
mod tests {
    use super::*;

    const U: &str = "http://utility.agent.test:18090";

    fn rule(allowed: &[&str], mode: Mode) -> Rule {
        Rule {
            allowed: allowed.iter().map(|a| a.to_string()).collect(),
            mode,
            policy: None,
        }
    }

    /// A pause of a request of `method` to `origin` + `path` with the browser's number `network`,
    /// in the frame `F1`.
    fn facts(method: &str, origin: &str, path: &str, network: Option<&str>) -> Facts {
        Facts {
            network: network.map(str::to_string),
            redirected: false,
            method: method.to_string(),
            url: format!("{origin}{path}"),
            origin: origin.to_string(),
            path: path.to_string(),
            frame: "F1".to_string(),
            worker_script: false,
            hook: None,
        }
    }

    fn pause(id: &str) -> Pause {
        Pause {
            session: Some("S".to_string()),
            id: id.to_string(),
        }
    }

    /// CP-170 E7 (d), m04: an origin that an allowed origin begins is no allowed origin.
    #[test]
    fn an_origin_that_an_allowed_origin_begins_is_refused() {
        let r = rule(
            &["http://pay.agent.test", "http://pay.agent.test:80"],
            Mode::Fail,
        );
        let mut h = Holds::default();
        let get = |o: &str| facts("GET", o, "/", Some("n"));
        assert_eq!(
            h.on_pause(Some(&r), &get("http://pay.agent.test"), pause("a"))
                .0,
            Act::Continue
        );
        for other in [
            "http://pay.agent.test.evil.example",
            "http://pay.agent.test:8080",
            "http://pay.agent.test:80.evil.example",
        ] {
            assert_eq!(
                h.on_pause(Some(&r), &get(other), pause("b")).0,
                Act::Fail,
                "{other}"
            );
        }
        assert_eq!(
            h.on_pause(None, &get("http://pay.agent.test"), pause("c"))
                .0,
            Act::Fail
        );
    }

    /// CP-170 E7 (e), m07: a service worker's script is not let through, also from an allowed
    /// origin; the product reads it from the request's headers with `is_worker_script`.
    #[test]
    fn a_service_worker_s_script_is_not_let_through() {
        let r = rule(&[U], Mode::Hold);
        let mut h = Holds::default();
        assert!(is_worker_script(
            &serde_json::json!({"Service-Worker": "script"})
        ));
        assert!(!is_worker_script(&serde_json::json!({"Accept": "*/*"})));
        let script = Facts {
            worker_script: true,
            ..facts("GET", U, "/sw.js", Some("n1"))
        };
        assert_eq!(h.on_pause(Some(&r), &script, pause("a")).0, Act::Fail);
        let plain = facts("GET", U, "/app.js", Some("n2"));
        assert_eq!(h.on_pause(Some(&r), &plain, pause("b")).0, Act::Continue);
    }

    /// CP-176 A1 (a): in the mode that fails, a state-changing request to an allowed origin is
    /// failed at once; a read passes; nothing is held.
    #[test]
    fn a1_the_mode_that_fails_fails_every_state_changing_request() {
        let r = rule(&[U], Mode::Fail);
        let mut h = Holds::default();
        let post = facts("POST", U, "/prefs", Some("n1"));
        assert_eq!(h.on_pause(Some(&r), &post, pause("a")), (Act::Fail, true));
        assert_eq!(
            h.on_pause(Some(&r), &facts("GET", U, "/bill", Some("n2")), pause("b"))
                .0,
            Act::Continue
        );
        assert!(h.waiting().is_empty());
        assert_eq!(h.release(1), Err("not_held"));
    }

    /// CP-176 A1 (b) and (f): two requests of one method, origin and path are held under two ids;
    /// only the one let through by its id goes on, and the other is failed.
    #[test]
    fn a1_of_two_like_requests_only_the_one_let_through_by_its_id_leaves() {
        let r = rule(&[U], Mode::Hold);
        let mut h = Holds::default();
        let (a, _) = h.on_pause(
            Some(&r),
            &facts("POST", U, "/prefs", Some("n1")),
            pause("p1"),
        );
        let (b, _) = h.on_pause(
            Some(&r),
            &facts("POST", U, "/prefs", Some("n2")),
            pause("p2"),
        );
        let (Act::Hold(ia), Act::Hold(ib)) = (a, b) else {
            panic!("{a:?} {b:?}");
        };
        assert_ne!(ia, ib);
        let waiting = h.waiting();
        assert_eq!(waiting.len(), 2);
        assert_eq!(
            (waiting[1].method.as_str(), waiting[1].path.as_str()),
            ("POST", "/prefs")
        );
        let r = h.release(ib).unwrap();
        assert_eq!(r.go, vec![pause("p2")]);
        assert_eq!(r.fail, vec![pause("p1")]);
        assert_eq!(r.failed, vec![ia]);
        assert!(h.waiting().is_empty());
        assert_eq!(h.release(ia), Err("released"));
    }

    /// CP-176 A1 (c) and (f): one request paused at both levels is one held request; both pauses
    /// go on at its release, and a pause of it after the release follows the release.
    #[test]
    fn a1_one_request_paused_twice_is_held_once_and_follows_its_answer() {
        let r = rule(&[U], Mode::Hold);
        let mut h = Holds::default();
        let post = facts("POST", U, "/prefs", Some("n1"));
        let (first, new) = h.on_pause(Some(&r), &post, pause("browser"));
        let Act::Hold(id) = first else {
            panic!("{first:?}")
        };
        assert!(new);
        assert_eq!(
            h.on_pause(Some(&r), &post, pause("target")),
            (Act::Hold(id), false)
        );
        assert_eq!(h.waiting().len(), 1);
        let done = h.release(id).unwrap();
        assert_eq!(done.go, vec![pause("browser"), pause("target")]);
        assert!(done.fail.is_empty() && done.failed.is_empty());
        assert_eq!(
            h.on_pause(Some(&r), &post, pause("late")),
            (Act::Continue, false)
        );
    }

    /// CP-176 A1 (c) and (f): a pause that the browser gives no number cannot be let through and
    /// is failed.
    #[test]
    fn a1_a_pause_with_no_number_is_failed() {
        let r = rule(&[U], Mode::Hold);
        let mut h = Holds::default();
        assert_eq!(
            h.on_pause(Some(&r), &facts("POST", U, "/prefs", None), pause("a")),
            (Act::Fail, true)
        );
        assert!(h.waiting().is_empty());
    }

    /// CP-176 A1 (c) and (f): a hop of 307 after a release, to the same address, is another
    /// state-changing request by its mark of a hop alone: it
    /// is failed, as a record of its own; a hop that became a GET passes by the list.
    #[test]
    fn a1_a_307_hop_after_a_release_is_failed_and_a_get_hop_passes() {
        let r = rule(&[U], Mode::Hold);
        let mut h = Holds::default();
        let (Act::Hold(id), _) = h.on_pause(
            Some(&r),
            &facts("POST", U, "/prefs", Some("n1")),
            pause("a"),
        ) else {
            panic!()
        };
        h.release(id).unwrap();
        let hop = Facts {
            redirected: true,
            ..facts("POST", U, "/prefs", Some("n1"))
        };
        assert_eq!(h.on_pause(Some(&r), &hop, pause("b")), (Act::Fail, true));
        assert_eq!(h.on_pause(Some(&r), &hop, pause("c")), (Act::Fail, false));
        let get = Facts {
            redirected: true,
            ..facts("GET", U, "/saved", Some("n1"))
        };
        assert_eq!(h.on_pause(Some(&r), &get, pause("d")).0, Act::Continue);
    }

    /// CP-176 A1 (a) and (f): an OPTIONS before a request passes to an allowed origin and uses
    /// nothing up: the release that follows still lets its request through.
    #[test]
    fn a1_an_options_passes_and_uses_nothing_up() {
        let r = rule(&[U], Mode::Hold);
        let mut h = Holds::default();
        assert_eq!(
            h.on_pause(
                Some(&r),
                &facts("OPTIONS", U, "/api", Some("n0")),
                pause("o")
            )
            .0,
            Act::Continue
        );
        let (Act::Hold(id), _) =
            h.on_pause(Some(&r), &facts("PUT", U, "/api", Some("n1")), pause("a"))
        else {
            panic!()
        };
        assert_eq!(h.release(id).unwrap().go, vec![pause("a")]);
    }

    /// CP-176 A1 (a) and (f): a request of another frame is held with its frame, and the release
    /// of one id fails it.
    #[test]
    fn a1_a_request_of_another_frame_is_held_with_its_frame_and_failed_at_a_release() {
        let r = rule(&[U, "http://pay.agent.test:18090"], Mode::Hold);
        let mut h = Holds::default();
        let (Act::Hold(main), _) = h.on_pause(
            Some(&r),
            &facts("POST", U, "/prefs", Some("n1")),
            pause("a"),
        ) else {
            panic!()
        };
        let other = Facts {
            frame: "F2".to_string(),
            ..facts("POST", "http://pay.agent.test:18090", "/charge", Some("n2"))
        };
        assert!(matches!(
            h.on_pause(Some(&r), &other, pause("b")).0,
            Act::Hold(_)
        ));
        assert_eq!(h.waiting()[1].frame, "F2");
        let r = h.release(main).unwrap();
        assert_eq!((r.go, r.fail), (vec![pause("a")], vec![pause("b")]));
        assert_eq!(r.failed.len(), 1);
    }

    /// CP-176 A1 (b) and (f): a held request beyond `MAX_HELD` is failed at once.
    #[test]
    fn a1_a_held_request_beyond_the_bound_is_failed() {
        let r = rule(&[U], Mode::Hold);
        let mut h = Holds::default();
        for n in 0..MAX_HELD {
            let net = format!("n{n}");
            assert!(matches!(
                h.on_pause(
                    Some(&r),
                    &facts("POST", U, "/prefs", Some(&net)),
                    pause(&net)
                )
                .0,
                Act::Hold(_)
            ));
        }
        assert_eq!(
            h.on_pause(
                Some(&r),
                &facts("POST", U, "/prefs", Some("over")),
                pause("over")
            ),
            (Act::Fail, true)
        );
        assert_eq!(h.waiting().len(), MAX_HELD);
    }

    /// CP-176 A1 (b) and (f): a new arm fails every held request and ends the release; a request
    /// failed by `fail` goes nowhere.
    #[test]
    fn a1_held_requests_are_failed_at_a_new_arm() {
        let r = rule(&[U], Mode::Hold);
        let mut h = Holds::default();
        for n in ["n1", "n2", "n3"] {
            h.on_pause(Some(&r), &facts("POST", U, "/prefs", Some(n)), pause(n));
        }
        assert_eq!(h.fail(3), Ok(vec![pause("n3")]));
        assert_eq!(h.fail(3), Err("not_held"));
        assert_eq!(h.end_arm(), vec![pause("n1"), pause("n2")]);
        assert!(h.waiting().is_empty());
        let (Act::Hold(id), _) = h.on_pause(
            Some(&r),
            &facts("POST", U, "/prefs", Some("n4")),
            pause("n4"),
        ) else {
            panic!()
        };
        assert_eq!(id, 4, "an id is never given twice");
        h.release(id).unwrap();
        h.end_arm();
        assert!(matches!(
            h.on_pause(
                Some(&r),
                &facts("POST", U, "/prefs", Some("n5")),
                pause("n5")
            )
            .0,
            Act::Hold(_)
        ));
    }

    #[test]
    fn every_method_but_get_head_and_options_changes_state() {
        for m in [
            "POST", "PUT", "PATCH", "DELETE", "CONNECT", "TRACE", "PROPFIND",
        ] {
            assert!(changes_state(m), "{m}");
        }
        for m in ["GET", "HEAD", "OPTIONS"] {
            assert!(!changes_state(m), "{m}");
        }
    }

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
    fn a_page_and_a_frame_take_nested_attach_and_must_be_armed_and_a_worker_need_not() {
        for kind in ["page", "iframe"] {
            assert!(nests(kind) && must_arm(kind), "{kind}");
        }
        for kind in ["worker", "shared_worker", "service_worker", "browser_ui"] {
            assert!(!nests(kind) && !must_arm(kind), "{kind}");
        }
    }
}
