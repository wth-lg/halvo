use std::fmt;
use std::io::{self, Read};
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use halvo_schema::executable::WebOrigin;
use reqwest::blocking::{Client, Response};
use reqwest::header::CONTENT_TYPE;
use reqwest::{redirect, retry, Method, Url};
use serde_json::Value;

use crate::answer::FORM;
use crate::error::{AnswerFault, BuildError, CallError};
use crate::token::Token;

/// How the client of the adapter finds the address of the origin's host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// The system's resolver. The origin must be `https`.
    System,
    /// The test configuration of A11.2: the client maps the origin's host name to 127.0.0.1 at the origin's port
    /// and looks no name up. Only this route takes an `http` origin, and it takes no host that the URL parser reads
    /// as an IP address.
    Loopback,
}

/// The Graph-shaped adapter of A11.2 for one origin, the faces of mail and calendar under `/v1.0/me/`. It holds
/// its HTTP client and the origin, and no token. The client is reqwest with rustls on the ring provider and the
/// verifier of the OS trust store, TLS 1.2 or 1.3, HTTP/1.1, no proxy, no redirect, no retry and no cookie. Each
/// request has one time limit, which holds its connect, its head and its body together, and the body of an answer
/// that an operation reads is bounded at 33,554,432 bytes.
///
/// It offers no request beyond the operations of `Mail` and `Calendar`: its own request calls cannot be reached from
/// outside this crate. Each call below fails to compile, and the same text with a public operation in its place
/// compiles:
///
/// ```compile_fail
/// use halvo_connect::{Calendar, Graph, RunDay, Token};
/// fn call(graph: &Graph, token: &Token, method: reqwest::Method, path: &str) -> bool {
///     let _ = RunDay::from_unix_seconds(0);
///     graph.effect(token, method, path, 204).is_ok()
/// }
/// ```
///
/// ```compile_fail
/// use halvo_connect::{Calendar, Graph, RunDay, Token};
/// fn call(graph: &Graph, token: &Token, method: reqwest::Method, path: &str) -> bool {
///     let _ = RunDay::from_unix_seconds(0);
///     graph.json(token, method, path, None, 200).is_ok()
/// }
/// ```
///
/// ```compile_fail
/// use halvo_connect::{Calendar, Graph, RunDay, Token};
/// fn call(graph: &Graph, token: &Token, method: reqwest::Method, path: &str) -> bool {
///     let _ = RunDay::from_unix_seconds(0);
///     graph.send(token, method, path, None, 202).is_ok()
/// }
/// ```
///
/// ```
/// use halvo_connect::{Calendar, Graph, RunDay, Token};
/// fn call(graph: &Graph, token: &Token, method: reqwest::Method, path: &str) -> bool {
///     let _ = (method, path);
///     graph.calendar_list(token, RunDay::from_unix_seconds(0)).is_ok()
/// }
/// ```
pub struct Graph {
    client: Client,
    origin: String,
    timeout: Duration,
}

impl Graph {
    /// The adapter for `origin`, whose requests each end within `timeout`, from the start of the request to the
    /// end of its answer's body. Refuses an `http` origin on the route
    /// `System` with `Cleartext` and an address host on the route `Loopback` with `Address`, and answers `Client`
    /// when the TLS configuration or the client cannot be built.
    pub fn new(origin: &WebOrigin, route: Route, timeout: Duration) -> Result<Graph, BuildError> {
        let url = Url::parse(origin.as_str()).map_err(|_| BuildError::Client)?;
        if url.scheme() != "https" && route != Route::Loopback {
            return Err(BuildError::Cleartext);
        }
        let mapped = match route {
            Route::System => None,
            Route::Loopback => {
                let host = url.domain().ok_or(BuildError::Address)?;
                let port = url.port_or_known_default().ok_or(BuildError::Client)?;
                Some((host, SocketAddr::from((Ipv4Addr::LOCALHOST, port))))
            }
        };
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verifier = rustls_platform_verifier::Verifier::new(provider.clone())
            .map_err(|_| BuildError::Client)?;
        let tls = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|_| BuildError::Client)?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(verifier))
            .with_no_client_auth();
        let mut builder = Client::builder()
            .tls_backend_preconfigured(tls)
            .no_proxy()
            .redirect(redirect::Policy::none())
            .retry(retry::never())
            .connect_timeout(timeout)
            .timeout(timeout);
        if let Some((host, address)) = mapped {
            builder = builder.resolve(host, address);
        }
        let client = builder.build().map_err(|_| BuildError::Client)?;
        Ok(Graph {
            client,
            origin: origin.as_str().to_owned(),
            timeout,
        })
    }

    pub(crate) fn json(
        &self,
        token: &Token,
        method: Method,
        path: &str,
        body: Option<&Value>,
        success: u16,
    ) -> Result<Value, CallError> {
        let response = self.send(token, method, path, body, success)?;
        let bytes = read_bounded(response)?;
        serde_json::from_slice(&bytes).map_err(|_| FORM)
    }

    pub(crate) fn effect(
        &self,
        token: &Token,
        method: Method,
        path: &str,
        success: u16,
    ) -> Result<(), CallError> {
        self.send(token, method, path, None, success).map(drop)
    }

    fn send(
        &self,
        token: &Token,
        method: Method,
        path: &str,
        body: Option<&Value>,
        success: u16,
    ) -> Result<Response, CallError> {
        let mut request = self
            .client
            .request(method, format!("{}{path}", self.origin))
            .bearer_auth(token.value())
            .timeout(self.timeout);
        if let Some(body) = body {
            request = request
                .header(CONTENT_TYPE, "application/json")
                .body(body.to_string());
        }
        let response = request.send().map_err(|error| {
            if error.is_connect() || error.is_builder() {
                CallError::NotSent
            } else {
                CallError::NoAnswer
            }
        })?;
        match response.status().as_u16() {
            status if status == success => Ok(response),
            401 => Err(CallError::Unauthorized),
            status => Err(CallError::Status(status)),
        }
    }
}

impl fmt::Debug for Graph {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Graph")
            .field("origin", &self.origin)
            .finish_non_exhaustive()
    }
}

const BOUND: usize = 33_554_432;

fn read_bounded(mut response: Response) -> Result<Vec<u8>, CallError> {
    if response
        .content_length()
        .is_some_and(|length| length > BOUND as u64)
    {
        return Err(CallError::Answer(AnswerFault::Size));
    }
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 65_536];
    loop {
        match response.read(&mut chunk) {
            Ok(0) => return Ok(bytes),
            Ok(n) if bytes.len() + n > BOUND => return Err(CallError::Answer(AnswerFault::Size)),
            Ok(n) => bytes.extend_from_slice(&chunk[..n]),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return Err(CallError::NoAnswer),
        }
    }
}

pub(crate) fn segment(id: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(id.len());
    for b in id.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            out.push('%');
            out.push(char::from(HEX[usize::from(b >> 4)]));
            out.push(char::from(HEX[usize::from(b & 0x0f)]));
        }
    }
    out
}

#[cfg(test)]
mod tests;
