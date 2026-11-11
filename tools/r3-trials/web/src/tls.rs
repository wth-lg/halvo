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
        .status()
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
        .status()
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

/// Reads the line `count requests=<n> handshakes=<n> tls_errors=<n>` of the listener.
pub fn parse_counts(line: &str) -> Option<Counts> {
    let rest = line.trim().strip_prefix("count ")?;
    let mut values = [None; 3];
    for pair in rest.split(' ') {
        let (k, v) = pair.split_once('=')?;
        let slot = match k {
            "requests" => 0,
            "handshakes" => 1,
            "tls_errors" => 2,
            _ => return None,
        };
        if values[slot].is_some() {
            return None;
        }
        values[slot] = Some(v.parse::<u64>().ok()?);
    }
    Some(Counts {
        requests: values[0]?,
        handshakes: values[1]?,
        tls_errors: values[2]?,
    })
}

/// The TLS listener of `cert_error`, `tools/r3-trials/web/tls-listener.mjs`, a child process on a
/// loopback port that the system picks.
pub struct TlsListener {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: mpsc::Receiver<String>,
    pub port: u16,
}

impl TlsListener {
    pub fn start(program: &Path, cert: &Path, key: &Path) -> Result<TlsListener, String> {
        let mut child = Command::new("node")
            .arg(program)
            .arg("--cert")
            .arg(cert)
            .arg("--key")
            .arg(key)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
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
        let mut listener = TlsListener {
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

    /// Asks the listener for its counts and waits for the answer at most `ANSWER_WAIT`.
    pub fn counts(&mut self) -> Result<Counts, String> {
        let stdin = self.stdin.as_mut().ok_or("stdin closed")?;
        stdin
            .write_all(b"count\n")
            .and_then(|_| stdin.flush())
            .map_err(|e| format!("write {}", e.kind()))?;
        let line = self
            .lines
            .recv_timeout(ANSWER_WAIT)
            .map_err(|_| "no answer".to_string())?;
        parse_counts(&line).ok_or_else(|| "answer form".to_string())
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

impl Drop for TlsListener {
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
        let mut listener = TlsListener::start(&program, &cert, &key).expect("listener");
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
