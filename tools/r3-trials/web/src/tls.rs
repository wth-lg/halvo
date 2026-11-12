use crate::start::Start;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

pub const ANSWER_WAIT: Duration = Duration::from_secs(10);

const OPENSSL_CONFIG: &str = "[req]
distinguished_name = dn
prompt = no
x509_extensions = ext

[dn]
CN = utility.agent.test

[ext]
subjectAltName = DNS:utility.agent.test
basicConstraints = critical,CA:FALSE
";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CertFault {
    NoOpenssl,
    Failed(String),
}

/// Whether the system's `openssl` runs: `openssl version` exits 0.
pub fn openssl_runs() -> bool {
    Command::new("openssl")
        .arg("version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .start_status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Makes a self-signed certificate for `utility.agent.test` and its key with the system's
/// `openssl` in the directory `dir`, which must not exist yet, and returns the paths of the two
/// PEM files. Nothing of them is printed. `NoOpenssl` when `openssl` does not run.
pub fn make_certificate(dir: &Path) -> Result<(PathBuf, PathBuf), CertFault> {
    if !openssl_runs() {
        return Err(CertFault::NoOpenssl);
    }
    std::fs::create_dir(dir).map_err(|e| CertFault::Failed(format!("dir {}", e.kind())))?;
    let config = dir.join("openssl.cnf");
    let cert = dir.join("cert.pem");
    let key = dir.join("key.pem");
    std::fs::write(&config, OPENSSL_CONFIG)
        .map_err(|e| CertFault::Failed(format!("config {}", e.kind())))?;
    let status = Command::new("openssl")
        .args([
            "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "2",
        ])
        .arg("-config")
        .arg(&config)
        .arg("-keyout")
        .arg(&key)
        .arg("-out")
        .arg(&cert)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .start_status()
        .map_err(|e| CertFault::Failed(format!("spawn {}", e.kind())))?;
    if !status.success() || !cert.is_file() || !key.is_file() {
        return Err(CertFault::Failed(format!(
            "openssl exit {}",
            status.code().unwrap_or(-1)
        )));
    }
    Ok((cert, key))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    pub requests: u64,
    pub handshakes: u64,
    pub tls_errors: u64,
}

fn parse_keys<const N: usize>(line: &str, keys: [&str; N]) -> Option<[u64; N]> {
    let rest = line.trim().strip_prefix("count ")?;
    let mut values = [None; N];
    for pair in rest.split(' ') {
        let (k, v) = pair.split_once('=')?;
        let slot = keys.iter().position(|key| *key == k)?;
        if values[slot].is_some() {
            return None;
        }
        values[slot] = Some(v.parse::<u64>().ok()?);
    }
    let mut out = [0; N];
    for (o, v) in out.iter_mut().zip(values) {
        *o = v?;
    }
    Some(out)
}

/// Reads the line `count <host>=<n> ...` or `count -` of the chain listener.
pub fn parse_host_counts(line: &str) -> Option<std::collections::BTreeMap<String, u64>> {
    let rest = line.trim().strip_prefix("count ")?;
    let mut out = std::collections::BTreeMap::new();
    if rest == "-" {
        return Some(out);
    }
    for pair in rest.split(' ') {
        let (k, v) = pair.split_once('=')?;
        if out.insert(k.to_string(), v.parse::<u64>().ok()?).is_some() {
            return None;
        }
    }
    Some(out)
}

/// Reads the line `count requests=<n> handshakes=<n> tls_errors=<n>` of the TLS listener.
pub fn parse_counts(line: &str) -> Option<Counts> {
    let [requests, handshakes, tls_errors] =
        parse_keys(line, ["requests", "handshakes", "tls_errors"])?;
    Some(Counts {
        requests,
        handshakes,
        tls_errors,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordCounts {
    pub connections: u64,
    pub bytes: u64,
    pub path_requests: u64,
}

/// Reads the line `count connections=<n> bytes=<n> path_requests=<n>` of the recording listener.
pub fn parse_record_counts(line: &str) -> Option<RecordCounts> {
    let [connections, bytes, path_requests] =
        parse_keys(line, ["connections", "bytes", "path_requests"])?;
    Some(RecordCounts {
        connections,
        bytes,
        path_requests,
    })
}

/// A listener of the trial in Node, a child process on a loopback port that the system picks:
/// the TLS listener of `cert_error` (`tls-listener.mjs`) or the recording listener of
/// `driver_killed_port` (`record-listener.mjs`).
pub struct Listener {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: mpsc::Receiver<String>,
    pub port: u16,
}

impl Listener {
    /// Starts `tls-listener.mjs` with the certificate and the key.
    pub fn start_tls(program: &Path, cert: &Path, key: &Path) -> Result<Listener, String> {
        Listener::spawn(
            program,
            &[
                "--cert".as_ref(),
                cert.as_os_str(),
                "--key".as_ref(),
                key.as_os_str(),
            ],
        )
    }

    /// Starts `ways-listener.mjs`, the pages of the rows under the egress check.
    pub fn start_ways(program: &Path) -> Result<Listener, String> {
        Listener::spawn(program, &[])
    }

    /// Starts `chain-listener.mjs`, the hosts of the rows `list_hops` and `list_close`.
    pub fn start_chain(program: &Path) -> Result<Listener, String> {
        Listener::spawn(program, &[])
    }

    /// Asks the chain listener for its count of each host and waits at most `ANSWER_WAIT`.
    pub fn host_counts(&mut self) -> Result<std::collections::BTreeMap<String, u64>, String> {
        let line = self.count_line()?;
        parse_host_counts(&line).ok_or_else(|| "answer form".to_string())
    }

    /// Starts `ws-listener.mjs`, the listener of the row `ws_allowed`, with the certificate and
    /// the key of its two TLS listeners.
    pub fn start_ws(program: &Path, cert: &Path, key: &Path) -> Result<Listener, String> {
        Listener::spawn(
            program,
            &[
                "--cert".as_ref(),
                cert.as_os_str(),
                "--key".as_ref(),
                key.as_os_str(),
            ],
        )
    }

    /// Asks a listener whose count line is `count <json>` for it and waits at most `ANSWER_WAIT`.
    pub fn json_counts(&mut self) -> Result<serde_json::Value, String> {
        let line = self.count_line()?;
        let rest = line
            .trim()
            .strip_prefix("count ")
            .ok_or_else(|| "answer form".to_string())?;
        serde_json::from_str(rest).map_err(|_| "answer form".to_string())
    }

    /// Starts `record-listener.mjs` for the request path `path`.
    pub fn start_record(program: &Path, path: &str) -> Result<Listener, String> {
        Listener::spawn(program, &["--path".as_ref(), path.as_ref()])
    }

    fn spawn(program: &Path, args: &[&std::ffi::OsStr]) -> Result<Listener, String> {
        let mut child = Command::new("node")
            .arg(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .start()
            .map_err(|e| format!("node {}", e.kind()))?;
        let stdin = child.stdin.take();
        let Some(stdout) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err("no stdout".into());
        };
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let first = lines.recv_timeout(ANSWER_WAIT).unwrap_or_default();
        let port = first
            .trim()
            .strip_prefix("listening 127.0.0.1:")
            .and_then(|p| p.parse::<u16>().ok());
        let mut listener = Listener {
            child,
            stdin,
            lines,
            port: 0,
        };
        match port {
            Some(port) => {
                listener.port = port;
                Ok(listener)
            }
            None => {
                listener.stop();
                Err("no listening line".into())
            }
        }
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Asks the TLS listener for its counts and waits for the answer at most `ANSWER_WAIT`.
    pub fn counts(&mut self) -> Result<Counts, String> {
        let line = self.count_line()?;
        parse_counts(&line).ok_or_else(|| "answer form".to_string())
    }

    /// Asks the recording listener for its counts and waits for the answer at most `ANSWER_WAIT`.
    pub fn record_counts(&mut self) -> Result<RecordCounts, String> {
        let line = self.count_line()?;
        parse_record_counts(&line).ok_or_else(|| "answer form".to_string())
    }

    fn count_line(&mut self) -> Result<String, String> {
        let stdin = self.stdin.as_mut().ok_or("stdin closed")?;
        stdin
            .write_all(b"count\n")
            .and_then(|_| stdin.flush())
            .map_err(|e| format!("write {}", e.kind()))?;
        self.lines
            .recv_timeout(ANSWER_WAIT)
            .map_err(|_| "no answer".to_string())
    }

    /// Ends the listener: closes its standard input, waits for it at most `ANSWER_WAIT`, then kills
    /// it, and reaps it. A listener that is dropped is ended the same way.
    pub fn stop(mut self) {
        self.end();
    }

    fn end(&mut self) {
        self.stdin = None;
        let deadline = std::time::Instant::now() + ANSWER_WAIT;
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(50))
                }
                _ => break,
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.end();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpStream;

    #[test]
    fn parse_counts_takes_the_three_counts_once_each() {
        assert_eq!(
            parse_counts("count requests=0 handshakes=2 tls_errors=1"),
            Some(Counts {
                requests: 0,
                handshakes: 2,
                tls_errors: 1
            })
        );
        for bad in [
            "count requests=0 handshakes=2",
            "count requests=0 requests=1 handshakes=2 tls_errors=1",
            "count requests=x handshakes=2 tls_errors=1",
            "listening 127.0.0.1:1",
            "count requests=0 handshakes=2 tls_errors=1 more=1",
        ] {
            assert_eq!(parse_counts(bad), None, "{bad}");
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let base = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-tmp");
        std::fs::create_dir_all(&base).expect("test-tmp");
        base.join(format!(
            "tls-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ))
    }

    #[test]
    fn the_listener_counts_a_failed_handshake_and_no_request() {
        if !openssl_runs() {
            eprintln!("openssl does not run here; the test checks nothing");
            return;
        }
        let dir = scratch("count");
        let (cert, key) = make_certificate(&dir).expect("certificate");
        let program = Path::new(env!("CARGO_MANIFEST_DIR")).join("tls-listener.mjs");
        let mut listener = Listener::start_tls(&program, &cert, &key).expect("listener");
        assert_eq!(
            listener.counts(),
            Ok(Counts {
                requests: 0,
                handshakes: 0,
                tls_errors: 0
            })
        );
        let mut plain = TcpStream::connect(("127.0.0.1", listener.port)).expect("connect");
        plain
            .write_all(b"GET / HTTP/1.0\r\nHost: utility.agent.test\r\n\r\n")
            .expect("write");
        let deadline = std::time::Instant::now() + ANSWER_WAIT;
        let counts = loop {
            let c = listener.counts().expect("counts");
            if c.tls_errors > 0 || std::time::Instant::now() > deadline {
                break c;
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        listener.stop();
        assert_eq!(counts.tls_errors, 1);
        assert_eq!(counts.requests, 0);
        assert_eq!(counts.handshakes, 0);
    }

    #[test]
    fn parse_host_counts_takes_each_host_once() {
        let c = parse_host_counts("count hop1.agent.test=1 hop2.agent.test=2").expect("form");
        assert_eq!(c.get("hop2.agent.test"), Some(&2));
        assert_eq!(parse_host_counts("count -").map(|c| c.len()), Some(0));
        assert_eq!(parse_host_counts("count a=1 a=2"), None);
        assert_eq!(parse_host_counts("listening 127.0.0.1:1"), None);
    }

    #[test]
    fn the_chain_listener_redirects_hop1_to_hop2_and_counts_by_host() {
        let program = Path::new(env!("CARGO_MANIFEST_DIR")).join("chain-listener.mjs");
        let mut listener = Listener::start_chain(&program).expect("listener");
        let mut plain = TcpStream::connect(("127.0.0.1", listener.port)).expect("connect");
        plain
            .write_all(b"GET /start HTTP/1.0\r\nHost: hop1.agent.test\r\n\r\n")
            .expect("write");
        let mut answer = String::new();
        use std::io::Read;
        plain.read_to_string(&mut answer).expect("read");
        let counts = listener.host_counts().expect("counts");
        listener.stop();
        assert!(answer.starts_with("HTTP/1.1 302"), "{answer}");
        assert!(
            answer.contains("Location: http://hop2.agent.test:"),
            "{answer}"
        );
        assert_eq!(counts.get("hop1.agent.test"), Some(&1));
    }

    #[test]
    fn parse_record_counts_takes_the_three_counts_once_each() {
        assert_eq!(
            parse_record_counts("count connections=1 bytes=187 path_requests=1"),
            Some(RecordCounts {
                connections: 1,
                bytes: 187,
                path_requests: 1
            })
        );
        for bad in [
            "count connections=1 bytes=187",
            "count connections=1 connections=1 bytes=1 path_requests=0",
            "count requests=0 handshakes=2 tls_errors=1",
        ] {
            assert_eq!(parse_record_counts(bad), None, "{bad}");
        }
    }

    #[test]
    fn the_recording_listener_counts_the_bytes_and_the_requests_of_its_path() {
        let program = Path::new(env!("CARGO_MANIFEST_DIR")).join("record-listener.mjs");
        let mut listener = Listener::start_record(&program, "/held").expect("listener");
        assert_eq!(
            listener.record_counts(),
            Ok(RecordCounts {
                connections: 0,
                bytes: 0,
                path_requests: 0
            })
        );
        let request =
            b"POST /held HTTP/1.1\r\nHost: utility.agent.test\r\nContent-Length: 1\r\n\r\nx";
        let mut plain = TcpStream::connect(("127.0.0.1", listener.port)).expect("connect");
        plain.write_all(request).expect("write");
        let deadline = std::time::Instant::now() + ANSWER_WAIT;
        let counts = loop {
            let c = listener.record_counts().expect("counts");
            if c.bytes == request.len() as u64 || std::time::Instant::now() > deadline {
                break c;
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        listener.stop();
        assert_eq!(counts.connections, 1);
        assert_eq!(counts.bytes, request.len() as u64);
        assert_eq!(counts.path_requests, 1);
    }

    #[test]
    fn make_certificate_refuses_a_directory_that_exists() {
        if !openssl_runs() {
            eprintln!("openssl does not run here; the test checks nothing");
            return;
        }
        let dir = scratch("exists");
        std::fs::create_dir(&dir).expect("dir");
        assert!(matches!(make_certificate(&dir), Err(CertFault::Failed(_))));
    }
}
