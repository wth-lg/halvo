//! The adapter to a model's host (the agents run's unit U7.5). Its one operation,
//! `send_under_grant`, takes the router's grant by value, the sender's form of the routing
//! contract's T9.2: it writes the host's own form of one request from the closed body of T11.3, in
//! the form that the adapter word of the grant's row names, sends it ONCE to the origin of the
//! grant's row and to no other, and reads the text and the token counts of the answer. It takes no
//! origin and no model word from its caller: both come from the grant's row. A rule of the tree
//! (crates/halvo-model/tests/door.rs) holds that the operation's name stands in this file and the
//! door's files alone, and that no file under crates/halvo-runner names it, the router's crate or a
//! grant.
//!
//! What leaves: the texts of the body's messages, the model word of the row, the body's output
//! limit, and the key in the one header `x-api-key`, with the closed list `HEADERS` of headers. The
//! body's tier and effort do not leave. The path is the adapter's constant: no address and no path
//! is made from the model word or the adapter word.

use std::fmt;
use std::io::{self, Read};
use std::sync::Arc;
use std::time::{Duration, Instant};

use halvo_route::leave::LeaveGrant;
use halvo_route_wire::{Body, Role};
use reqwest::blocking::Client;
use reqwest::header::{HeaderValue, ACCEPT, CONTENT_TYPE};
use reqwest::{redirect, retry, Url};
use serde_json::Value;

/// The forms of a host's interface that this crate writes, each named by the adapter word of a row
/// of the router's table of hosts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Adapter {
    /// `POST /v1/messages` of Anthropic's API with the header `anthropic-version: 2026-10-21`: a
    /// body of `model`, `max_tokens`, `system` and `messages`, and an answer whose `content` holds
    /// text blocks and whose `usage` holds `input_tokens` and `output_tokens`.
    AnthropicMessages,
}

impl Adapter {
    /// Every adapter of this crate.
    pub const ALL: [Adapter; 1] = [Adapter::AnthropicMessages];

    /// The adapter word that a row of the table of hosts names.
    pub const fn word(self) -> &'static str {
        match self {
            Adapter::AnthropicMessages => "anthropic-messages-2026-10-21",
        }
    }

    /// The adapter whose word is `word`, or none.
    pub fn parse(word: &str) -> Option<Adapter> {
        Adapter::ALL
            .into_iter()
            .find(|adapter| adapter.word() == word)
    }
}

/// The one time limit of a request, from the start of its connect to the end of its answer's body.
/// After it the adapter gives `NoAnswer` and sends nothing again.
pub const TIME_LIMIT: Duration = Duration::from_secs(90);

/// The headers of a request, a closed list: the client adds no other.
pub const HEADERS: [&str; 6] = [
    "host",
    "content-type",
    "content-length",
    "accept",
    "x-api-key",
    "anthropic-version",
];

const PATH: &str = "/v1/messages";
const VERSION: &str = "2026-10-21";
const ANSWER_BOUND: usize = 4_194_304;

/// Why `send_under_grant` gave no answer. No variant holds text of the request, of the answer or
/// of the key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendError {
    /// The grant's row names an adapter word that this crate does not have: nothing was sent.
    Adapter,
    /// The origin of the grant's row is not `https://` and a host name alone (or, in a build with
    /// the feature `loopback`, `http://127.0.0.1:<port>`): nothing was sent.
    Origin,
    /// The TLS configuration or the HTTP client could not be built on this system: nothing was
    /// sent.
    Client,
    /// The body is outside the limits of T11.3, or its messages have no form of the adapter: the
    /// request was not sent.
    Form,
    /// No connection or no TLS session to the origin could be made: the request did not leave.
    NotSent,
    /// The request left and no whole answer came within `TIME_LIMIT`.
    NoAnswer,
    /// The host answered this status, which is not 200.
    Status(u16),
    /// The answer of status 200 is not of the adapter's form.
    Answer,
}

/// What a host answered: the text of its text blocks, in order, the token counts that its usage
/// reports, and the milliseconds from the start of the request to the answer's head and to the end
/// of its body. Its `Debug` writes the text as its number of bytes.
#[derive(Clone, PartialEq, Eq)]
pub struct Answered {
    pub text: String,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub head_ms: u64,
    pub ms: u64,
}

impl fmt::Debug for Answered {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Answered")
            .field("text", &format_args!("{} bytes", self.text.len()))
            .field("tokens_in", &self.tokens_in)
            .field("tokens_out", &self.tokens_out)
            .field("head_ms", &self.head_ms)
            .field("ms", &self.ms)
            .finish()
    }
}

/// The adapter's ONE operation: sends ONE request, made from `body` in the form of the adapter
/// that the grant's row names, with the row's model word and with `key` in the header `x-api-key`,
/// to the origin of the grant's row and to no other, and reads its answer. It takes the grant by
/// value, so one grant serves one request, and it sends nothing again, whatever the answer.
pub fn send_under_grant(grant: LeaveGrant, key: &str, body: &Body) -> Result<Answered, SendError> {
    let row = grant.row();
    let adapter = Adapter::parse(row.adapter()).ok_or(SendError::Adapter)?;
    Host::new(adapter, row.origin())?.send(key, row.model(), body)
}

/// One origin of a model's host with the adapter of its row. The client is reqwest with rustls on
/// the ring provider and the verifier of the OS trust store, HTTP/1.1, no proxy, no redirect, no
/// retry, no cookie and no user agent.
struct Host {
    client: Client,
    url: Url,
    adapter: Adapter,
}

impl Host {
    /// The host at `origin` for `adapter`. `origin` must be `https://` and a host name alone; a
    /// build with the feature `loopback` also takes `http://127.0.0.1:<port>`.
    fn new(adapter: Adapter, origin: &str) -> Result<Host, SendError> {
        let parsed = Url::parse(origin).map_err(|_| SendError::Origin)?;
        let named = parsed.scheme() == "https"
            && parsed.domain().map(|name| format!("https://{name}")) == Some(origin.to_owned());
        if !named && !loopback(&parsed, origin) {
            return Err(SendError::Origin);
        }
        let url = Url::parse(&format!("{origin}{PATH}")).map_err(|_| SendError::Origin)?;
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verifier = rustls_platform_verifier::Verifier::new(provider.clone())
            .map_err(|_| SendError::Client)?;
        let tls = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|_| SendError::Client)?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(verifier))
            .with_no_client_auth();
        let client = Client::builder()
            .tls_backend_preconfigured(tls)
            .no_proxy()
            .redirect(redirect::Policy::none())
            .retry(retry::never())
            .connect_timeout(TIME_LIMIT)
            .timeout(TIME_LIMIT)
            .build()
            .map_err(|_| SendError::Client)?;
        Ok(Host {
            client,
            url,
            adapter,
        })
    }

    /// Sends ONE request of the adapter's form, made from `body` with `model` as its model word,
    /// with `key` in the header `x-api-key`, and reads its answer.
    fn send(&self, key: &str, model: &str, body: &Body) -> Result<Answered, SendError> {
        let form = match self.adapter {
            Adapter::AnthropicMessages => messages_form(model, body)?,
        };
        let mut key = HeaderValue::from_str(key).map_err(|_| SendError::NotSent)?;
        key.set_sensitive(true);
        let start = Instant::now();
        let response = self
            .client
            .post(self.url.clone())
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json")
            .header("x-api-key", key)
            .header("anthropic-version", VERSION)
            .body(form)
            .send()
            .map_err(|error| {
                if error.is_connect() || error.is_builder() {
                    SendError::NotSent
                } else {
                    SendError::NoAnswer
                }
            })?;
        let head_ms = millis(start);
        let status = response.status().as_u16();
        if status != 200 {
            return Err(SendError::Status(status));
        }
        let bytes = read_bounded(response)?;
        let ms = millis(start);
        let (text, tokens_in, tokens_out) = messages_answer(&bytes).ok_or(SendError::Answer)?;
        Ok(Answered {
            text,
            tokens_in,
            tokens_out,
            head_ms,
            ms,
        })
    }
}

impl fmt::Debug for Host {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Host")
            .field("url", &self.url.as_str())
            .field("adapter", &self.adapter)
            .finish_non_exhaustive()
    }
}

#[cfg(feature = "loopback")]
fn loopback(parsed: &Url, origin: &str) -> bool {
    std::hint::black_box(crate::graph::LOOPBACK_MARKER.as_ptr());
    parsed.scheme() == "http"
        && parsed.host_str() == Some("127.0.0.1")
        && parsed
            .port_or_known_default()
            .is_some_and(|port| format!("http://127.0.0.1:{port}") == origin)
}

#[cfg(not(feature = "loopback"))]
fn loopback(_parsed: &Url, _origin: &str) -> bool {
    false
}

/// The body of `POST /v1/messages`: `model`, `max_tokens`, `system` when the first message of the
/// closed body is a system message, and `messages` with the other messages, which are of the roles
/// user and assistant. A system message after the first, a body with no other message, and a body
/// outside the limits of T11.3 have no form.
fn messages_form(model: &str, body: &Body) -> Result<Vec<u8>, SendError> {
    body.to_bytes().map_err(|_| SendError::Form)?;
    let (system, rest) = match body.messages.split_first() {
        Some((first, rest)) if first.role == Role::System => (Some(&first.content), rest),
        _ => (None, &body.messages[..]),
    };
    if rest.is_empty() || rest.iter().any(|message| message.role == Role::System) {
        return Err(SendError::Form);
    }
    let text = |value: &str| serde_json::to_string(value).map_err(|_| SendError::Form);
    let mut out = format!(
        "{{\"model\":{},\"max_tokens\":{}",
        text(model)?,
        body.max_output_tokens
    );
    if let Some(system) = system {
        out.push_str(",\"system\":");
        out.push_str(&text(system)?);
    }
    out.push_str(",\"messages\":[");
    for (at, message) in rest.iter().enumerate() {
        if at > 0 {
            out.push(',');
        }
        out.push_str("{\"role\":");
        out.push_str(&text(message.role.as_str())?);
        out.push_str(",\"content\":");
        out.push_str(&text(&message.content)?);
        out.push('}');
    }
    out.push_str("]}");
    Ok(out.into_bytes())
}

/// The text of the text blocks of `content`, in order, and the counts `input_tokens` and
/// `output_tokens` of `usage`; none when the answer is not one JSON object with an array `content`
/// whose text blocks hold a string `text` and a `usage` with the two counts.
fn messages_answer(bytes: &[u8]) -> Option<(String, u64, u64)> {
    let answer: Value = serde_json::from_slice(bytes).ok()?;
    let mut text = String::new();
    for block in answer.get("content")?.as_array()? {
        if block.get("type")?.as_str()? == "text" {
            text.push_str(block.get("text")?.as_str()?);
        }
    }
    let usage = answer.get("usage")?;
    let tokens_in = usage.get("input_tokens")?.as_u64()?;
    let tokens_out = usage.get("output_tokens")?.as_u64()?;
    Some((text, tokens_in, tokens_out))
}

fn read_bounded(mut response: reqwest::blocking::Response) -> Result<Vec<u8>, SendError> {
    if response
        .content_length()
        .is_some_and(|length| length > ANSWER_BOUND as u64)
    {
        return Err(SendError::Answer);
    }
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 65_536];
    loop {
        match response.read(&mut chunk) {
            Ok(0) => return Ok(bytes),
            Ok(n) if bytes.len() + n > ANSWER_BOUND => return Err(SendError::Answer),
            Ok(n) => bytes.extend_from_slice(&chunk[..n]),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return Err(SendError::NoAnswer),
        }
    }
}

fn millis(start: Instant) -> u64 {
    u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX)
}
