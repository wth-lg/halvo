//! The scripted host of the hosted form's stand-in (unit U0.8): a listener on 127.0.0.1 that answers
//! each `POST /v1/messages` of the door's one adapter in that adapter's form (one text block and the
//! usage counts), by the walk that a right driver makes, as the control's mode `right` of
//! `r3-standin` does: in a compile the next action of the task, in a recovery the action that the
//! page and this run's own lines call for, and for a reply a body of a few words. It is no model and
//! it speaks to nothing: it reads the request, records its body, and writes one answer. It takes no
//! word of a stored step, because the hosted request holds none (A12.7).

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use serde_json::Value;

/// The model word of the stand-in rows of the door's loopback form (halvo-model's `MODEL`): a
/// request with another word is answered 400.
pub const MODEL: &str = "stand-in-model";

/// The adapter's version header (halvo-connect's `VERSION`): a request with another is answered 400.
pub const VERSION: &str = "2026-10-21";

/// How the host walks: as a right driver, or as one that answers the first recovery ask at each
/// step that found no control with a `handback`, which the harness refuses, and the next one right.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Walk {
    Right,
    FirstRecoveryRefused,
}

/// One request that the host received: its head and its body.
#[derive(Clone, Debug)]
pub struct Received {
    pub head: String,
    pub body: Vec<u8>,
}

/// The scripted host on 127.0.0.1. Its thread stops when the value drops, also when a test panics.
pub struct Host {
    port: u16,
    received: Arc<Mutex<Vec<Received>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Host {
    /// Starts the host on 127.0.0.1:`port` (0: a free port), walking by `walk`. With `log`, it
    /// appends each request's body to that file, behind a line `--- request <n> bytes=<n>`; it
    /// writes no head, so no key.
    pub fn start(port: u16, log: Option<PathBuf>, walk: Walk) -> std::io::Result<Host> {
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        let port = listener.local_addr()?.port();
        let received = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let (received, stop) = (received.clone(), stop.clone());
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    let Ok(mut stream) = stream else { continue };
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(20)));
                    let Some(request) = read_request(&mut stream) else {
                        continue;
                    };
                    let (status, answer) = answer(&request, walk);
                    let n = {
                        let mut all = received.lock().unwrap_or_else(|e| e.into_inner());
                        all.push(request.clone());
                        all.len()
                    };
                    if let Some(path) = &log {
                        if let Ok(mut f) = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(path)
                        {
                            let _ = writeln!(f, "--- request {n} bytes={}", request.body.len());
                            let _ = f.write_all(&request.body);
                            let _ = writeln!(f);
                        }
                    }
                    let reason = if status == 200 { "OK" } else { "Bad Request" };
                    let _ = write!(
                        stream,
                        "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{answer}",
                        answer.len()
                    );
                    let _ = stream.flush();
                }
            })
        };
        Ok(Host {
            port,
            received,
            stop,
            thread: Some(thread),
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Every request received so far, in order.
    pub fn received(&self) -> Vec<Received> {
        self.received
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn read_request(stream: &mut TcpStream) -> Option<Received> {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 4096];
    let end = loop {
        if let Some(at) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            break at;
        }
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        bytes.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8(bytes[..end].to_vec()).ok()?;
    let length: usize = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().ok())?
        })
        .unwrap_or(0);
    let mut body = bytes[end + 4..].to_vec();
    while body.len() < length {
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    Some(Received { head, body })
}

fn header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().find_map(|line| {
        let (n, v) = line.split_once(':')?;
        n.eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

/// The status and the body of the answer to one request: 400 when it is not of the adapter's form
/// with the stand-in's model word; else 200 with one text block and the usage.
fn answer(request: &Received, walk: Walk) -> (u16, String) {
    let refuse = |why: &str| {
        (
            400,
            format!("{{\"type\":\"error\",\"error\":{{\"type\":\"invalid_request_error\",\"message\":{}}}}}",
                serde_json::to_string(why).unwrap_or_default()),
        )
    };
    if !request.head.starts_with("POST /v1/messages HTTP/1.1") {
        return refuse("not POST /v1/messages");
    }
    if header(&request.head, "anthropic-version") != Some(VERSION)
        || header(&request.head, "x-api-key").is_none_or(str::is_empty)
    {
        return refuse("headers");
    }
    let Ok(body) = serde_json::from_slice::<Value>(&request.body) else {
        return refuse("body");
    };
    let messages = body["messages"].as_array().cloned().unwrap_or_default();
    let (Some(system), Some(content), 1) = (
        body["system"].as_str(),
        messages
            .first()
            .filter(|m| m["role"] == "user")
            .and_then(|m| m["content"].as_str()),
        messages.len(),
    ) else {
        return refuse("messages");
    };
    if body["model"] != MODEL || !body["max_tokens"].is_u64() {
        return refuse("model");
    }
    let (prompt, snapshot) = split(content);
    let text = if prompt.contains("\nDone so far:") {
        let ask = Ask {
            prompt,
            lines: snapshot.lines().filter_map(parse_line).collect(),
        };
        let first_recovery =
            prompt.contains(crate::words::REPAIR_HOSTED) && !ask.last_line_refused();
        if walk == Walk::FirstRecoveryRefused && first_recovery {
            action("handback", None, None, None)
        } else {
            next(&ask)
        }
    } else {
        "Thank you for the message, I will reply soon".to_string()
    };
    let tokens = |bytes: usize| bytes.div_ceil(4).max(1);
    (
        200,
        format!(
            "{{\"id\":\"msg_r3_host\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"{MODEL}\",\"content\":[{{\"type\":\"text\",\"text\":{}}}],\"stop_reason\":\"end_turn\",\"usage\":{{\"input_tokens\":{},\"output_tokens\":{}}}}}",
            serde_json::to_string(&text).unwrap_or_default(),
            tokens(system.len() + content.len()),
            tokens(text.len())
        ),
    )
}

/// The prompt and the snapshot of the user's text: the text before the marker line `<<<DATA` and the
/// lines between it and `DATA>>>` (A9.5).
fn split(content: &str) -> (&str, &str) {
    match content.split_once("\n<<<DATA\n") {
        Some((prompt, rest)) => (prompt, rest.strip_suffix("\nDATA>>>").unwrap_or(rest)),
        None => (content, ""),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Line {
    reference: String,
    role: String,
    name: String,
}

fn parse_line(text: &str) -> Option<Line> {
    let (reference, rest) = text.split_once(' ')?;
    let (role, rest) = rest.split_once(' ')?;
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(Line {
        reference: reference.to_string(),
        role: role.to_string(),
        name: rest[..end].replace("''", "\""),
    })
}

/// The fixture's names of the link to the payment and of the effect buttons, as its pages and its
/// variant `changed_page_2` show them (tools/fixture-server/variants/pages.mjs).
const TO_PAYMENT: [&str; 2] = ["Pay bill", "Make a payment"];
const EFFECT_BUTTONS: [&str; 4] = [
    "Pay now",
    "Place order",
    "Submit payment",
    "Complete purchase",
];
const SELECT_NAME: &str = "Payment method";
const DONE_HEADINGS: [&str; 2] = ["Payment received", "Order placed"];

struct Ask<'a> {
    prompt: &'a str,
    lines: Vec<Line>,
}

impl Ask<'_> {
    fn find(&self, role: &str, names: &[&str]) -> Option<&Line> {
        self.lines
            .iter()
            .find(|l| l.role == role && names.contains(&l.name.as_str()))
    }

    fn first(&self, role: &str) -> Option<&Line> {
        self.lines.iter().find(|l| l.role == role)
    }

    fn after_text(&self, label: &str) -> Option<&Line> {
        let i = self
            .lines
            .iter()
            .position(|l| l.role == "text" && l.name == label)?;
        self.lines[i + 1..].iter().find(|l| l.role == "text")
    }

    fn kept(&self) -> Vec<&str> {
        self.prompt
            .lines()
            .find_map(|l| l.strip_prefix("Kept: "))
            .map(|k| {
                if k == "none" {
                    Vec::new()
                } else {
                    k.split(", ").collect()
                }
            })
            .unwrap_or_default()
    }

    /// Whether the last numbered line of the list is a refusal: in a recovery, an ask after the
    /// first at the same step.
    fn last_line_refused(&self) -> bool {
        self.prompt
            .lines()
            .skip_while(|l| !l.starts_with("Done so far:"))
            .skip(1)
            .filter(|l| {
                l.split_once(". ")
                    .is_some_and(|(n, _)| n.bytes().all(|b| b.is_ascii_digit()))
            })
            .last()
            .is_some_and(|l| l.contains("(refused"))
    }

    /// The actions of the list that were carried out: each numbered line without a refusal.
    fn carried(&self) -> Vec<&str> {
        self.prompt
            .lines()
            .skip_while(|l| !l.starts_with("Done so far:"))
            .skip(1)
            .filter_map(|l| {
                let (n, a) = l.split_once(". ")?;
                (n.bytes().all(|b| b.is_ascii_digit()) && !a.contains("(refused")).then_some(a)
            })
            .collect()
    }
}

fn action(kind: &str, reference: Option<&str>, slot: Option<&str>, name: Option<&str>) -> String {
    let q = |v: Option<&str>| {
        v.map_or("null".to_string(), |s| {
            serde_json::to_string(s).unwrap_or_default()
        })
    };
    format!(
        "{{\"action\":\"{kind}\",\"ref\":{},\"slot\":{},\"key\":null,\"name\":{}}}",
        q(reference),
        q(slot),
        q(name)
    )
}

/// The right walk: done at the page that shows the end, handback at a sign-in form, then the
/// extracts that the effect needs, the select of the payment method, the effect button, and the link
/// to the payment.
fn next(ask: &Ask) -> String {
    let click = |l: &Line| action("click", Some(&l.reference), None, None);
    if ask.find("heading", &DONE_HEADINGS).is_some() {
        return action("done", None, None, None);
    }
    if ask.first("textbox").is_some() && ask.find("button", &["Sign in"]).is_some() {
        return action("handback", None, None, None);
    }
    let kept = ask.kept();
    let order = ask.find("text", &["Total"]).is_some();
    let needed: &[&str] = if order {
        &["items", "total"]
    } else {
        &["amount_due"]
    };
    for name in needed {
        if kept.contains(name) {
            continue;
        }
        let line = match *name {
            "items" => ask.first("listitem"),
            "total" => ask.after_text("Total"),
            _ => ask.after_text("Amount due"),
        };
        if let Some(l) = line {
            return action("extract", Some(&l.reference), None, Some(name));
        }
    }
    if let Some(combo) = ask.find("combobox", &[SELECT_NAME]) {
        let selected = ask.carried().iter().any(|a| a.starts_with("select "));
        if !selected {
            return action("select", Some(&combo.reference), Some("instrument"), None);
        }
        if let Some(b) = ask.find("button", &EFFECT_BUTTONS) {
            return click(b);
        }
    }
    if let Some(l) = ask.find("link", &TO_PAYMENT) {
        return click(l);
    }
    action("done", None, None, None)
}
