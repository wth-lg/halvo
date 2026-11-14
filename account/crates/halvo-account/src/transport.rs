use crate::answer::Reply;
use crate::flow::{NoAnswer, Transport};
use crate::origin::Backend;
use halvo_wire::request::Request;
use std::io::Read;
use std::time::Duration;

/// The most bytes of an answer's body that the helper reads; a longer body is no answer.
pub const BODY_LIMIT: u64 = 65_536;

/// The two limits of a call: how long the client waits for its connection to the backend, and
/// how long it waits for the whole call, the answer read to its end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Limits {
    connect: Duration,
    call: Duration,
}

/// The limits of the product, and the one place that states them: 10 s to connect and 30 s
/// for the whole call. `Client::new` builds with these, and nothing at run time changes them.
const LIMITS: Limits = Limits {
    connect: Duration::from_secs(10),
    call: Duration::from_secs(30),
};

/// The one HTTP client of the app: reqwest over HTTP/1.1 with rustls, ring and the system's
/// trust store, TLS 1.3 at the least, no proxy, no redirect, no cookie, no compression, and
/// https alone in a release build. It sends the method, the path, the headers and the body
/// of a `Request` and nothing of its own but the transport headers that
/// `transport-headers.txt` of this crate lists. It waits at most 10 s for its connection and
/// 30 s for the whole call; a call that passes either limit is a call with no answer. It takes
/// no certificate file: no argument, variable or setting of this crate adds a root, and the
/// system's own verifier decides on macOS and on Windows.
pub struct Client {
    inner: reqwest::blocking::Client,
    backend: Backend,
    #[cfg(test)]
    limits: Limits,
}

impl Client {
    /// Builds the client for `backend` with the limits of the product, or answers `None` when
    /// reqwest cannot build it.
    pub fn new(backend: Backend) -> Option<Client> {
        Client::with_limits(backend, LIMITS)
    }

    /// The one builder of the client. Only the tests of this module call it with other limits,
    /// short ones of their own.
    fn with_limits(backend: Backend, limits: Limits) -> Option<Client> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let inner = reqwest::blocking::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .referer(false)
            .tls_version_min(reqwest::tls::Version::TLS_1_3)
            .https_only(cfg!(not(debug_assertions)))
            .connect_timeout(limits.connect)
            .timeout(limits.call)
            .build()
            .ok()?;
        Some(Client {
            inner,
            backend,
            #[cfg(test)]
            limits,
        })
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

#[cfg(test)]
mod tests;
