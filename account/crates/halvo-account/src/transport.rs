use crate::answer::Reply;
use crate::flow::{NoAnswer, Transport};
use crate::origin::Backend;
use halvo_wire::request::Request;
use std::io::Read;
use std::time::Duration;

/// The most bytes of an answer's body that the helper reads; a longer body is no answer.
pub const BODY_LIMIT: u64 = 65_536;

/// How long the helper waits for its connection to the backend.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// How long the helper waits for the whole answer.
pub const TIMEOUT: Duration = Duration::from_secs(30);

/// The one HTTP client of the app: reqwest over HTTP/1.1 with rustls, ring and the system's
/// trust store, TLS 1.3 at the least, no proxy, no redirect, no cookie, no compression, and
/// https alone in a release build. It sends the method, the path, the headers and the body
/// of a `Request` and nothing of its own but the transport headers that
/// `transport-headers.txt` of this crate lists.
pub struct Client {
    inner: reqwest::blocking::Client,
    backend: Backend,
}

impl Client {
    /// Builds the client for `backend`, or answers `None` when reqwest cannot build it.
    pub fn new(backend: Backend) -> Option<Client> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let inner = reqwest::blocking::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .referer(false)
            .tls_version_min(reqwest::tls::Version::TLS_1_3)
            .https_only(cfg!(not(debug_assertions)))
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(TIMEOUT)
            .build()
            .ok()?;
        Some(Client { inner, backend })
    }
}

impl Transport for Client {
    fn send(&self, request: &Request) -> Result<Reply, NoAnswer> {
        let method =
            reqwest::Method::from_bytes(request.method().as_bytes()).map_err(|_| NoAnswer)?;
        let mut call = self
            .inner
            .request(method, self.backend.url_of(request.path()));
        for (name, value) in request.headers() {
            call = call.header(*name, value.as_str());
        }
        if let Some(body) = request.body() {
            call = call.body(body.to_vec());
        }
        let response = call.send().map_err(|_| NoAnswer)?;
        let status = response.status().as_u16();
        let mut body = Vec::new();
        response
            .take(BODY_LIMIT + 1)
            .read_to_end(&mut body)
            .map_err(|_| NoAnswer)?;
        if body.len() as u64 > BODY_LIMIT {
            return Err(NoAnswer);
        }
        Ok(Reply { status, body })
    }
}
