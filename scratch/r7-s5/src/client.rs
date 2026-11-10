//! The helper's one HTTPS request: reqwest over rustls, with the server certificate
//! verified by the operating system (rustls-platform-verifier, reqwest's `rustls` feature).

use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct Ping {
    pub status: u16,
    pub client_build: Duration,
    pub request: Duration,
}

/// `extra_root_pem` is for this crate's own tests, which must not change the trust
/// store of the machine they run on. The helper always passes `None`.
pub fn ping(
    url: &str,
    run_id: &str,
    bearer: &str,
    extra_root_pem: Option<&str>,
) -> Result<Ping, reqwest::Error> {
    let started = Instant::now();
    let mut builder = reqwest::blocking::Client::builder().timeout(Duration::from_secs(20));
    if let Some(pem) = extra_root_pem {
        builder = builder.tls_certs_merge([reqwest::Certificate::from_pem(pem.as_bytes())?]);
    }
    let client = builder.build()?;
    let client_build = started.elapsed();

    let response = client
        .get(url)
        .header("x-run-id", run_id)
        .bearer_auth(bearer)
        .send()?;
    let status = response.status().as_u16();
    response.bytes()?;
    Ok(Ping {
        status,
        client_build,
        request: started.elapsed() - client_build,
    })
}

/// An error with every cause beneath it, so a certificate failure is visible in a log.
pub fn error_chain(error: &dyn std::error::Error) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}
