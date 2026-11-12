use std::fmt;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use halvo_schema::executable::WebOrigin;
use reqwest::blocking::{Client, Response};
use reqwest::header::CONTENT_TYPE;
use reqwest::{redirect, retry, Method};
use serde_json::Value;

use crate::answer::FORM;
use crate::error::{BuildError, CallError};
use crate::token::Token;

/// How the client of the adapter finds the address of the origin's host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// The system's resolver. The origin must be `https`.
    System,
    /// The test configuration of A11.2: the client maps the origin's host to 127.0.0.1 at the origin's port and
    /// looks no name up. Only this route takes an `http` origin.
    Loopback,
}

/// The Graph-shaped adapter of A11.2 for one origin, the faces of mail and calendar under `/v1.0/me/`. It holds
/// its HTTP client and the origin, and no token. The client is reqwest with rustls on the ring provider and the
/// verifier of the OS trust store, TLS 1.2 or 1.3, HTTP/1.1, no proxy, no redirect, no retry, no cookie, and one
/// time limit for each request.
pub struct Graph {
    client: Client,
    origin: String,
}

impl Graph {
    /// The adapter for `origin`, whose requests each end within `timeout`. Refuses an `http` origin on the route
    /// `System` with `Cleartext`, and answers `Client` when the TLS configuration or the client cannot be built.
    pub fn new(origin: &WebOrigin, route: Route, timeout: Duration) -> Result<Graph, BuildError> {
        let (scheme, authority) = origin
            .as_str()
            .split_once("://")
            .ok_or(BuildError::Client)?;
        let (host, port) = authority.rsplit_once(':').ok_or(BuildError::Client)?;
        let port: u16 = port.parse().map_err(|_| BuildError::Client)?;
        if scheme != "https" && route != Route::Loopback {
            return Err(BuildError::Cleartext);
        }
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
        if route == Route::Loopback {
            builder = builder.resolve(host, SocketAddr::from((Ipv4Addr::LOCALHOST, port)));
        }
        let client = builder.build().map_err(|_| BuildError::Client)?;
        Ok(Graph {
            client,
            origin: origin.as_str().to_owned(),
        })
    }

    pub(crate) fn json(
        &self,
        token: &Token,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Value, CallError> {
        let response = self.send(token, method, path, body)?;
        let bytes = response.bytes().map_err(|_| CallError::NoAnswer)?;
        serde_json::from_slice(&bytes).map_err(|_| FORM)
    }

    pub(crate) fn effect(
        &self,
        token: &Token,
        method: Method,
        path: &str,
    ) -> Result<(), CallError> {
        self.send(token, method, path, None).map(drop)
    }

    fn send(
        &self,
        token: &Token,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Response, CallError> {
        let mut request = self
            .client
            .request(method, format!("{}{path}", self.origin))
            .bearer_auth(token.value());
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
            200..=299 => Ok(response),
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
