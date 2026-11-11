use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chromiumoxide::cdp::browser_protocol::target::{SessionId, SetAutoAttachParams};
use chromiumoxide::cdp::js_protocol::runtime::RunIfWaitingForDebuggerParams;
use chromiumoxide::cdp::{CdpEvent, CdpEventMessage};
use chromiumoxide::error::CdpError;
use chromiumoxide::types::{CallId, Command, Message, MethodId};
use chromiumoxide::Connection;
use futures::StreamExt;
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use super::{Fault, Res, STEP_TIMEOUT};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Attached {
    pub target: String,
    pub kind: String,
    pub ready: bool,
}

#[derive(Default)]
pub(crate) struct Book {
    pub sessions: HashMap<String, Attached>,
    pub faults: Vec<String>,
}

struct Order {
    method: MethodId,
    session: Option<String>,
    params: Value,
    reply: oneshot::Sender<Result<Value, String>>,
}

#[derive(Clone)]
pub(crate) struct Wire {
    tx: mpsc::UnboundedSender<Order>,
    pub book: Arc<Mutex<Book>>,
    gate: Arc<tokio::sync::Mutex<()>>,
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

impl Wire {
    pub(crate) async fn open(address: &str) -> Res<(Wire, JoinHandle<()>)> {
        let conn = Connection::<CdpEventMessage>::connect(address)
            .await
            .map_err(|e| Fault::new("wire", e.to_string()))?;
        let (tx, rx) = mpsc::unbounded_channel();
        let wire = Wire {
            tx,
            book: Arc::new(Mutex::new(Book::default())),
            gate: Arc::new(tokio::sync::Mutex::new(())),
        };
        let task = tokio::spawn(run(conn, rx, wire.clone()));
        if let Err(f) = wire.call(None, auto_attach()?).await {
            task.abort();
            return Err(f);
        }
        Ok((wire, task))
    }

    pub(crate) async fn call<C: Command>(&self, session: Option<&str>, cmd: C) -> Res<C::Response> {
        let method = cmd.identifier();
        let params = serde_json::to_value(&cmd).map_err(|e| Fault::new("wire", e.to_string()))?;
        let (reply, answer) = oneshot::channel();
        self.tx
            .send(Order {
                method: method.clone(),
                session: session.map(str::to_string),
                params,
                reply,
            })
            .map_err(|_| Fault::new("wire", "closed"))?;
        let value = match tokio::time::timeout(STEP_TIMEOUT, answer).await {
            Ok(Ok(Ok(v))) => v,
            Ok(Ok(Err(e))) => return Err(Fault::new("wire", format!("{method}: {e}"))),
            Ok(Err(_)) => return Err(Fault::new("wire", "closed")),
            Err(_) => return Err(Fault::new("wire", format!("{method}: timeout"))),
        };
        C::response_from_value(value).map_err(|e| Fault::new("wire", e.to_string()))
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

    fn fault(&self, text: String) {
        if let Ok(mut b) = self.book.lock() {
            b.faults.push(text);
        }
    }

    fn on_event(&self, ev: CdpEventMessage) {
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
                tokio::spawn(async move { wire.prepare(session, kind, waiting).await });
            }
            CdpEvent::TargetDetachedFromTarget(d) => {
                if let Ok(mut b) = self.book.lock() {
                    b.sessions.remove(d.session_id.inner());
                }
            }
            _ => {}
        }
    }

    async fn prepare(&self, session: String, kind: String, waiting: bool) {
        let _held = self.gate.lock().await;
        if nests(&kind) {
            let nested = match auto_attach() {
                Ok(params) => self.call(Some(&session), params).await.map(|_| ()),
                Err(f) => Err(f),
            };
            if let Err(f) = nested {
                self.fault(format!("attach kind={kind} cause={}", f.0));
                return;
            }
        }
        if let Ok(mut b) = self.book.lock() {
            if let Some(a) = b.sessions.get_mut(&session) {
                a.ready = true;
            }
        }
        if waiting {
            if let Err(f) = self
                .call(Some(&session), RunIfWaitingForDebuggerParams::default())
                .await
            {
                self.fault(format!("resume kind={kind} cause={}", f.0));
            }
        }
    }
}

async fn run(
    mut conn: Connection<CdpEventMessage>,
    mut orders: mpsc::UnboundedReceiver<Order>,
    wire: Wire,
) {
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
                Some(Ok(Message::Response(r))) => {
                    if let Some(reply) = pending.remove(&r.id) {
                        let answer = match (r.result, r.error) {
                            (_, Some(e)) => Err(e.message),
                            (Some(v), None) => Ok(v),
                            (None, None) => Ok(Value::Null),
                        };
                        let _ = reply.send(answer);
                    }
                }
                Some(Ok(Message::Event(ev))) => wire.on_event(ev),
                Some(Err(CdpError::Ws(_))) => break,
                Some(Err(_)) => {}
                None => break,
            },
        }
    }
}
