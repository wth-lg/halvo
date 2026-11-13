use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use halvo_connect::{Graph, Route};
use halvo_schema::executable::WebOrigin;
use serde_json::Value;

pub fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

const END_WITH_INPUT: &str = "data:text/javascript,const e=()=>process.exit(0);process.stdin.on('end',e).on('error',e).resume();";

pub struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub struct Fixture {
    _server: Server,
    port: u16,
}

impl Fixture {
    pub fn start() -> Fixture {
        Fixture::start_then(|_| {})
    }

    pub fn start_then(after_spawn: impl FnOnce(u32)) -> Fixture {
        let server = repo()
            .join("tools")
            .join("fixture-server")
            .join("variants")
            .join("server.mjs");
        let child = Command::new("node")
            .args(["--import", END_WITH_INPUT])
            .arg(server)
            .args(["--port", "0"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("node starts the fixture server");
        let mut server = Server(child);
        after_spawn(server.0.id());
        let stdout = server.0.stdout.take().expect("its standard output");
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut line = String::new();
            let _ = BufReader::new(stdout).read_line(&mut line);
            let _ = tx.send(line);
        });
        let line = rx.recv_timeout(Duration::from_secs(60)).unwrap_or_default();
        let port = line
            .trim_end()
            .strip_prefix("listening 127.0.0.1:")
            .and_then(|port| port.parse::<u16>().ok());
        let Some(port) = port else {
            panic!("the fixture server printed no line listening 127.0.0.1:<port>");
        };
        Fixture {
            _server: server,
            port,
        }
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn origin(&self, host: &str) -> WebOrigin {
        WebOrigin::parse(&format!("http://{host}.agent.test:{}", self.port)).expect("an origin")
    }

    pub fn graph(&self, host: &str) -> Graph {
        self.graph_within(host, Duration::from_secs(10))
    }

    pub fn graph_within(&self, host: &str, timeout: Duration) -> Graph {
        Graph::new(&self.origin(host), Route::Loopback, timeout).expect("the client builds")
    }

    pub fn reset(&self, seed: u8, variants: &[&str]) {
        let mut path = format!("/reset?seed={seed}");
        for variant in variants {
            path.push_str("&variant=");
            path.push_str(variant);
        }
        self.control("POST", &path);
    }

    pub fn advance(&self) {
        self.control("POST", "/advance");
    }

    pub fn state(&self) -> Value {
        self.control("GET", "/state")
    }

    fn control(&self, method: &str, path: &str) -> Value {
        let mut stream =
            TcpStream::connect(loopback(self.port)).expect("the fixture server takes a connection");
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .expect("a read timeout");
        write!(
            stream,
            "{method} {path} HTTP/1.0\r\nHost: control.agent.test:{}\r\nContent-Length: 0\r\n\r\n",
            self.port
        )
        .expect("the request is written");
        let mut answer = Vec::new();
        stream.read_to_end(&mut answer).expect("the answer is read");
        let answer = String::from_utf8(answer).expect("a text answer");
        let (head, body) = answer.split_once("\r\n\r\n").expect("a head and a body");
        assert!(
            head.lines()
                .next()
                .is_some_and(|line| line.contains(" 200 ")),
            "the control host takes the call"
        );
        serde_json::from_str(body).expect("a JSON answer")
    }
}

pub fn loopback(port: u16) -> SocketAddr {
    SocketAddr::from((Ipv4Addr::LOCALHOST, port))
}
