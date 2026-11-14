use crate::start::Start;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::Value;

pub const CONTROL: &str = "control.agent.test";
pub const UTILITY: &str = "utility.agent.test";
pub const PAY: &str = "pay.agent.test";

pub fn origin(host: &str, port: u16) -> String {
    format!("http://{host}:{port}")
}

pub struct Control {
    pub port: u16,
}

impl Control {
    pub fn request(&self, method: &str, target: &str) -> Result<(u16, String), String> {
        let mut s = TcpStream::connect(("127.0.0.1", self.port)).map_err(|e| e.to_string())?;
        s.set_read_timeout(Some(Duration::from_secs(10)))
            .map_err(|e| e.to_string())?;
        let req = format!(
            "{method} {target} HTTP/1.0\r\nHost: {CONTROL}:{}\r\nContent-Length: 0\r\n\r\n",
            self.port
        );
        s.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
        let mut raw = Vec::new();
        s.read_to_end(&mut raw).map_err(|e| e.to_string())?;
        let text = String::from_utf8(raw).map_err(|e| e.to_string())?;
        let (head, body) = text
            .split_once("\r\n\r\n")
            .ok_or_else(|| "no header end".to_string())?;
        let status = head
            .split_whitespace()
            .nth(1)
            .and_then(|c| c.parse::<u16>().ok())
            .ok_or_else(|| "no status".to_string())?;
        Ok((status, body.to_string()))
    }

    pub fn reset(&self, variants: &[&str]) -> Result<Value, String> {
        let mut target = "/reset?seed=1".to_string();
        for v in variants {
            target.push_str("&variant=");
            target.push_str(v);
        }
        self.json("POST", &target)
    }

    pub fn state(&self) -> Result<Value, String> {
        self.json("GET", "/state")
    }

    fn json(&self, method: &str, target: &str) -> Result<Value, String> {
        let (status, body) = self.request(method, target)?;
        if status != 200 {
            return Err(format!("status {status}"));
        }
        serde_json::from_str(&body).map_err(|e| e.to_string())
    }
}

pub fn count(state: &Value, path: &[&str]) -> Option<i64> {
    let mut v = state;
    for k in path {
        v = &v[*k];
    }
    v.as_i64().or_else(|| v.as_array().map(|a| a.len() as i64))
}

pub struct Seed(pub Value);

impl Seed {
    pub fn load(repo: &Path, n: u8) -> Result<Seed, String> {
        let path = repo.join(format!("tools/fixture-server/variants/data/seed-{n}.json"));
        let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        serde_json::from_str(&text)
            .map(Seed)
            .map_err(|e| e.to_string())
    }

    pub fn str(&self, path: &[&str]) -> String {
        let mut v = &self.0;
        for k in path {
            v = &v[*k];
        }
        v.as_str().unwrap_or_default().to_string()
    }
}

pub struct Server {
    child: Child,
    pub port: u16,
}

impl Server {
    pub fn start(program: &Path, args: &[String]) -> Result<Server, String> {
        let mut child = Command::new("node")
            .arg(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .start()
            .map_err(|e| format!("node: {e}"))?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            let _ = BufReader::new(stdout).read_line(&mut line);
            let _ = tx.send(line);
        });
        let line = rx.recv_timeout(Duration::from_secs(15)).unwrap_or_default();
        let port = line
            .trim()
            .strip_prefix("listening 127.0.0.1:")
            .and_then(|p| p.parse::<u16>().ok());
        match port {
            Some(port) => Ok(Server { child, port }),
            None => {
                let _ = child.kill();
                let mut err = String::new();
                if let Some(mut e) = child.stderr.take() {
                    let _ = e.read_to_string(&mut err);
                }
                let _ = child.wait();
                Err(format!("no listening line: {}", err.trim()))
            }
        }
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    pub fn stop(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn fixture_program(repo: &Path) -> PathBuf {
    repo.join("tools/fixture-server/variants/server.mjs")
}
