//! A loopback HTTPS stub with its own test certificate authority, and the record of
//! what reached it.

use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{ServerConfig, ServerConnection, StreamOwned};
use std::fs::File;
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

pub const CA_NAME: &str = "Halvo R7 S6 spike test CA (synthetic)";

const IO_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_HEAD: usize = 16 * 1024;
const RESPONSE: &[u8] =
    b"HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok";

pub struct TestPki {
    pub ca_pem: String,
    pub leaf_pem: String,
    leaf: CertificateDer<'static>,
    leaf_key: Vec<u8>,
}

/// A fresh CA and a leaf for `localhost`. The CA's private key is dropped on return,
/// so nothing can issue another certificate under a CA that a runner was told to trust.
pub fn generate_pki() -> Result<TestPki, rcgen::Error> {
    let not_before = time::OffsetDateTime::now_utc() - time::Duration::days(1);
    let not_after = not_before + time::Duration::days(8);

    let mut ca = CertificateParams::new(Vec::<String>::new())?;
    ca.distinguished_name.push(DnType::CommonName, CA_NAME);
    ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    ca.not_before = not_before;
    ca.not_after = not_after;
    let ca_key = KeyPair::generate()?;
    let ca_cert = ca.self_signed(&ca_key)?;
    let issuer = Issuer::new(ca, ca_key);

    let names = ["localhost", "127.0.0.1", "::1"].map(String::from);
    let mut leaf = CertificateParams::new(names)?;
    leaf.distinguished_name
        .push(DnType::CommonName, "localhost");
    leaf.use_authority_key_identifier_extension = true;
    leaf.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    leaf.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    leaf.not_before = not_before;
    leaf.not_after = not_after;
    let leaf_key = KeyPair::generate()?;
    let leaf_cert = leaf.signed_by(&leaf_key, &issuer)?;

    Ok(TestPki {
        ca_pem: ca_cert.pem(),
        leaf_pem: leaf_cert.pem(),
        leaf: leaf_cert.der().clone(),
        leaf_key: leaf_key.serialize_der(),
    })
}

#[derive(Clone, Debug, PartialEq)]
pub enum Entry {
    /// A complete HTTP request head arrived over TLS.
    Request {
        run_id: String,
        bearer: String,
        line: String,
    },
    /// A connection that ended before a request, with the reason the stub saw.
    NoRequest(String),
}

#[derive(Clone)]
struct Record {
    entries: Arc<Mutex<Vec<Entry>>>,
    file: Option<Arc<Mutex<File>>>,
}

impl Record {
    fn push(&self, entry: Entry) {
        if let Some(file) = &self.file {
            let text = match &entry {
                Entry::Request {
                    run_id,
                    bearer,
                    line,
                } => format!("request x-run-id={run_id} bearer={bearer} line={line:?}\n"),
                Entry::NoRequest(reason) => format!("no request: {reason}\n"),
            };
            let mut file = file.lock().unwrap_or_else(PoisonError::into_inner);
            let _ = file.write_all(text.as_bytes());
        }
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(entry);
    }
}

pub struct Stub {
    port: u16,
    pub listening: Vec<SocketAddr>,
    record: Record,
}

impl Stub {
    /// Serves until the process ends. Each connection gets one request and one response.
    pub fn start(pki: &TestPki, record_file: Option<&Path>) -> io::Result<Stub> {
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(pki.leaf_key.clone()));
        let config = ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(io::Error::other)?
            .with_no_client_auth()
            .with_single_cert(vec![pki.leaf.clone()], key)
            .map_err(io::Error::other)?;
        let config = Arc::new(config);

        let file = match record_file {
            Some(path) => Some(Arc::new(Mutex::new(File::create(path)?))),
            None => None,
        };
        let record = Record {
            entries: Arc::default(),
            file,
        };

        let v4 = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let port = v4.local_addr()?.port();
        let mut listeners = vec![v4];
        // `localhost` resolves to both loopback addresses. Listening on both keeps a
        // client from waiting on a refused first address, which would be a cost of
        // this stub and not of the helper.
        if let Ok(v6) = TcpListener::bind((Ipv6Addr::LOCALHOST, port)) {
            listeners.push(v6);
        }

        let mut listening = Vec::new();
        for listener in listeners {
            listening.push(listener.local_addr()?);
            let config = config.clone();
            let record = record.clone();
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(stream) = stream else { continue };
                    exchange(stream, config.clone(), &record);
                }
            });
        }
        Ok(Stub {
            port,
            listening,
            record,
        })
    }

    pub fn url(&self, path: &str) -> String {
        format!("https://localhost:{}{path}", self.port)
    }

    pub fn entries(&self) -> Vec<Entry> {
        self.record
            .entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The bearer value of the recorded request with this run id, if the record holds one.
    pub fn bearer_of(&self, wanted: &str) -> Option<String> {
        self.entries().into_iter().find_map(|entry| match entry {
            Entry::Request { run_id, bearer, .. } if run_id == wanted => Some(bearer),
            _ => None,
        })
    }
}

fn exchange(stream: TcpStream, config: Arc<ServerConfig>, record: &Record) {
    let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
    let connection = match ServerConnection::new(config) {
        Ok(connection) => connection,
        Err(error) => return record.push(Entry::NoRequest(format!("tls setup: {error}"))),
    };
    let mut tls = StreamOwned::new(connection, stream);

    let mut head = Vec::new();
    let mut chunk = [0u8; 2048];
    while !head.windows(4).any(|window| window == b"\r\n\r\n") {
        match tls.read(&mut chunk) {
            Ok(0) => return record.push(Entry::NoRequest("closed before a request".to_string())),
            Ok(count) => head.extend_from_slice(&chunk[..count]),
            Err(error) => return record.push(Entry::NoRequest(error.to_string())),
        }
        if head.len() > MAX_HEAD {
            return record.push(Entry::NoRequest("request head too large".to_string()));
        }
    }

    // The record is written before the response, so it holds the request by the time
    // the client sees an answer.
    record.push(parse(&head));
    let _ = tls.write_all(RESPONSE);
    tls.conn.send_close_notify();
    let _ = tls.flush();
}

fn parse(head: &[u8]) -> Entry {
    let text = String::from_utf8_lossy(head);
    let mut lines = text.split("\r\n");
    let line = lines.next().unwrap_or_default().to_string();
    let mut run_id = String::new();
    let mut bearer = String::new();
    for header in lines {
        let Some((name, value)) = header.split_once(':') else {
            continue;
        };
        match name.trim().to_ascii_lowercase().as_str() {
            "x-run-id" => run_id = value.trim().to_string(),
            "authorization" => {
                bearer = value
                    .trim()
                    .strip_prefix("Bearer ")
                    .unwrap_or_default()
                    .to_string();
            }
            _ => {}
        }
    }
    Entry::Request {
        run_id,
        bearer,
        line,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_run_id_and_the_bearer() {
        let head = b"GET /v1/ping HTTP/1.1\r\nHost: localhost\r\nX-Run-Id: run-07\r\nauthorization: Bearer synthetic\r\n\r\n";
        assert_eq!(
            parse(head),
            Entry::Request {
                run_id: "run-07".to_string(),
                bearer: "synthetic".to_string(),
                line: "GET /v1/ping HTTP/1.1".to_string(),
            }
        );
    }
}
