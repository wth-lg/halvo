use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use halvo_connect::{Graph, Route};
use halvo_schema::executable::WebOrigin;

pub struct Plan {
    pub head_after: Duration,
    pub head: Vec<u8>,
    pub body_after: Duration,
    pub body: Vec<u8>,
    pub hold: Duration,
}

pub struct StandIn {
    port: u16,
    done: mpsc::Receiver<bool>,
}

impl StandIn {
    pub fn serve(plan: Plan) -> StandIn {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
        let port = listener.local_addr().expect("its address").port();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
            let mut seen = Vec::new();
            let mut buf = [0u8; 4096];
            while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
                match stream.read(&mut buf) {
                    Ok(0) | Err(_) => return,
                    Ok(n) => seen.extend_from_slice(&buf[..n]),
                }
            }
            thread::sleep(plan.head_after);
            let mut wrote_all = stream.write_all(&plan.head).is_ok() && stream.flush().is_ok();
            thread::sleep(plan.body_after);
            wrote_all = wrote_all && stream.write_all(&plan.body).is_ok() && stream.flush().is_ok();
            thread::sleep(plan.hold);
            let _ = tx.send(wrote_all);
        });
        StandIn { port, done: rx }
    }

    pub fn graph(&self, timeout: Duration) -> Graph {
        let origin =
            WebOrigin::parse(&format!("http://mail.agent.test:{}", self.port)).expect("an origin");
        Graph::new(&origin, Route::Loopback, timeout).expect("the client builds")
    }

    pub fn wrote_all(&self, within: Duration) -> Option<bool> {
        self.done.recv_timeout(within).ok()
    }
}

pub fn head(length: Option<usize>) -> Vec<u8> {
    match length {
        Some(n) => format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {n}\r\nConnection: close\r\n\r\n"
        ),
        None => "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n"
            .to_owned(),
    }
    .into_bytes()
}

pub fn inbox_of(size: usize) -> Vec<u8> {
    let open = br#"{"value": [], "pad": ""#;
    let close = br#""}"#;
    let mut body = Vec::with_capacity(size);
    body.extend_from_slice(open);
    body.resize(size - close.len(), b'a');
    body.extend_from_slice(close);
    body
}
