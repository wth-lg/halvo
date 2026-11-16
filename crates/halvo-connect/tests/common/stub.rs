use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use halvo_connect::{Graph, Route};
use halvo_schema::executable::WebOrigin;
use serde_json::Value;

pub const HOLD: u16 = 0;

#[derive(Debug, PartialEq)]
pub struct Seen {
    pub method: String,
    pub path: String,
    pub authorization: Option<String>,
    pub body: Option<Value>,
}

pub struct Stub {
    port: u16,
    seen: mpsc::Receiver<Seen>,
}

impl Stub {
    pub fn start(routes: Vec<(String, u16, String)>) -> Stub {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
        let port = listener.local_addr().expect("its address").port();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let Some(seen) = read(&mut stream) else {
                    continue;
                };
                let key = format!("{} {}", seen.method, seen.path);
                let (status, body) = routes
                    .iter()
                    .find(|(route, _, _)| *route == key)
                    .map_or((404, "{}".to_owned()), |(_, status, body)| {
                        (*status, body.clone())
                    });
                if tx.send(seen).is_err() {
                    return;
                }
                if status == HOLD {
                    thread::spawn(move || {
                        thread::sleep(Duration::from_secs(3));
                        drop(stream);
                    });
                    continue;
                }
                let _ = stream.write_all(
                    format!(
                        "HTTP/1.1 {status} Stub\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                );
            }
        });
        Stub { port, seen: rx }
    }

    pub fn graph(&self, host: &str) -> Graph {
        self.graph_within(host, Duration::from_secs(10))
    }

    pub fn graph_within(&self, host: &str, timeout: Duration) -> Graph {
        let origin = WebOrigin::parse(&format!("http://{host}.agent.test:{}", self.port))
            .expect("an origin");
        Graph::new(&origin, Route::Loopback, timeout).expect("the client builds")
    }

    pub fn seen(&self) -> Vec<Seen> {
        let mut all = Vec::new();
        while let Ok(seen) = self.seen.recv_timeout(Duration::from_millis(400)) {
            all.push(seen);
        }
        all
    }
}

fn read(stream: &mut TcpStream) -> Option<Seen> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut bytes = Vec::new();
    let mut buf = [0u8; 4096];
    let end = loop {
        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            break end;
        }
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => return None,
            Ok(n) => bytes.extend_from_slice(&buf[..n]),
        }
    };
    let head = String::from_utf8_lossy(&bytes[..end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let method = first.next()?.to_owned();
    let path = first.next()?.to_owned();
    let mut authorization = None;
    let mut length = 0;
    for line in lines {
        let (name, value) = line.split_once(':')?;
        match name.to_ascii_lowercase().as_str() {
            "authorization" => authorization = Some(value.trim().to_owned()),
            "content-length" => length = value.trim().parse().ok()?,
            _ => {}
        }
    }
    while bytes.len() < end + 4 + length {
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => return None,
            Ok(n) => bytes.extend_from_slice(&buf[..n]),
        }
    }
    let body = &bytes[end + 4..end + 4 + length];
    let body = if body.is_empty() {
        None
    } else {
        Some(
            serde_json::from_slice(body)
                .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(body).into_owned())),
        )
    };
    Some(Seen {
        method,
        path,
        authorization,
        body,
    })
}
