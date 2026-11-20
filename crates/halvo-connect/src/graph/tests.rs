use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{mpsc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use halvo_schema::executable::WebOrigin;
use reqwest::Method;
use serde_json::json;

use super::{segment, Graph, Route};
use crate::error::{AnswerFault, BuildError, CallError, Effect, NotMade};
use crate::token::Token;

const LIMIT: Duration = Duration::from_secs(5);
const QUIET: Duration = Duration::from_millis(400);

enum Reply {
    Bytes(String),
    Hold,
    Close,
}

fn answer(status: &str, body: &str) -> Reply {
    Reply::Bytes(format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    ))
}

fn origin(scheme: &str, port: u16) -> WebOrigin {
    WebOrigin::parse(&format!("{scheme}://mail.agent.test:{port}")).expect("an origin")
}

fn token() -> Token {
    Token::new("unit-token-1".to_owned()).expect("a token")
}

fn graph(port: u16, timeout: Duration) -> Graph {
    Graph::new(&origin("http", port), Route::Loopback, timeout).expect("the client builds")
}

static PORTS: Mutex<()> = Mutex::new(());

fn ports() -> MutexGuard<'static, ()> {
    PORTS.lock().unwrap_or_else(PoisonError::into_inner)
}

fn bind_any(_ports: &MutexGuard<'static, ()>) -> TcpListener {
    TcpListener::bind("127.0.0.1:0").expect("a port")
}

fn closed_port() -> (MutexGuard<'static, ()>, u16) {
    let held = ports();
    let port = bind_any(&held).local_addr().expect("its address").port();
    (held, port)
}

fn listen(reply: Reply) -> (u16, mpsc::Receiver<Vec<u8>>) {
    let listener = bind_any(&ports());
    let port = listener.local_addr().expect("its address").port();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            if tx.send(request(&mut stream)).is_err() {
                return;
            }
            match &reply {
                Reply::Bytes(bytes) => {
                    let _ = stream.write_all(bytes.as_bytes());
                }
                Reply::Hold => thread::sleep(Duration::from_secs(3)),
                Reply::Close => {}
            }
        }
    });
    (port, rx)
}

fn request(stream: &mut TcpStream) -> Vec<u8> {
    let _ = stream.set_read_timeout(Some(QUIET));
    let mut seen = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        if let Some(end) = seen.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&seen[..end]).to_ascii_lowercase();
            let length = head
                .lines()
                .find_map(|line| line.strip_prefix("content-length: "))
                .and_then(|n| n.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if seen.len() >= end + 4 + length {
                return seen;
            }
        }
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => return seen,
            Ok(n) => seen.extend_from_slice(&buf[..n]),
        }
    }
}

fn text(seen: &mpsc::Receiver<Vec<u8>>) -> String {
    String::from_utf8(seen.recv_timeout(LIMIT).expect("a request")).expect("text")
}

#[test]
fn segment_keeps_unreserved_bytes_and_encodes_every_other() {
    assert_eq!(segment("AZaz09-._~"), "AZaz09-._~");
    assert_eq!(segment("a/b"), "a%2Fb");
    assert_eq!(segment("%2e%2e"), "%252e%252e");
    assert_eq!(segment("a b?#="), "a%20b%3F%23%3D");
    assert_eq!(segment("é"), "%C3%A9");
    assert_eq!(segment("+\u{0}\u{7f}"), "%2B%00%7F");
}

#[test]
fn the_loopback_route_stands_under_the_feature_that_the_tests_name_with_its_marker() {
    const { assert!(cfg!(feature = "loopback")) };
    assert_eq!(
        super::LOOPBACK_MARKER,
        "halvo-connect-loopback-route-of-tests"
    );
    let manifest = include_str!("../../Cargo.toml").replace("\r\n", "\n");
    assert!(manifest.contains("[features]\nloopback = []\n"));
    assert!(manifest.contains("halvo-connect = { path = \".\", features = [\"loopback\"] }"));
}

#[test]
fn http_needs_the_loopback_route() {
    let http = origin("http", 18090);
    assert_eq!(
        Graph::new(&http, Route::System, LIMIT).err(),
        Some(BuildError::Cleartext)
    );
    assert!(Graph::new(&http, Route::Loopback, LIMIT).is_ok());
}

#[test]
fn the_loopback_route_maps_host_names_only() {
    for host in ["3221225985", "0xc0000201", "192.0.513", "2130706433"] {
        for scheme in ["http", "https"] {
            assert_eq!(
                WebOrigin::parse(&format!("{scheme}://{host}:18090"))
                    .map_err(|refusal| refusal.rule()),
                Err("origin_ipv4"),
                "{scheme}://{host}"
            );
        }
    }
    for host in ["192.0.2.1", "127.0.0.1"] {
        for scheme in ["http", "https"] {
            let origin = WebOrigin::parse(&format!("{scheme}://{host}:18090")).expect("an origin");
            assert_eq!(
                Graph::new(&origin, Route::Loopback, LIMIT).err(),
                Some(BuildError::Address),
                "{scheme}://{host}"
            );
        }
    }
    let https = WebOrigin::parse("https://192.0.2.1:443").expect("an origin");
    assert!(Graph::new(&https, Route::System, LIMIT).is_ok());
}

#[test]
fn https_builds_on_the_system_route_with_the_os_trust_store() {
    let origin = WebOrigin::parse("https://graph.microsoft.com:443").expect("an origin");
    let graph = Graph::new(&origin, Route::System, LIMIT).expect("the client builds");
    assert_eq!(
        format!("{graph:?}"),
        "Graph { origin: \"https://graph.microsoft.com:443\", .. }"
    );
}

#[test]
fn the_token_goes_in_the_authorization_header_alone() {
    let (port, seen) = listen(answer("200 OK", "{\"value\":[]}"));
    let got = graph(port, LIMIT)
        .json(&token(), Method::GET, "/v1.0/me/x", None, 200)
        .expect("an answer");
    assert_eq!(got, json!({"value": []}));
    let request = text(&seen).to_ascii_lowercase();
    assert!(request.starts_with("get /v1.0/me/x http/1.1\r\n"));
    assert!(request.contains("\r\nauthorization: bearer unit-token-1\r\n"));
    assert!(request.contains(&format!("\r\nhost: mail.agent.test:{port}\r\n")));
    assert_eq!(request.matches("unit-token-1").count(), 1);
}

#[test]
fn a_body_goes_as_json() {
    let (port, seen) = listen(answer("200 OK", "{\"id\":\"d1\"}"));
    let body = json!({"body": {"content": "Noted."}});
    let got = graph(port, LIMIT)
        .json(
            &token(),
            Method::PATCH,
            "/v1.0/me/messages/d1",
            Some(&body),
            200,
        )
        .expect("an answer");
    assert_eq!(got, json!({"id": "d1"}));
    let request = text(&seen);
    let (head, sent) = request.split_once("\r\n\r\n").expect("a head");
    let head = head.to_ascii_lowercase();
    assert!(head.starts_with("patch /v1.0/me/messages/d1 http/1.1\r\n"));
    assert!(head.contains("\r\ncontent-type: application/json"));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(sent).ok(),
        Some(body)
    );
}

#[test]
fn status_401_is_unauthorized_and_makes_nothing() {
    let (port, seen) = listen(answer("401 Unauthorized", "{\"error\":{}}"));
    let error = graph(port, LIMIT)
        .effect(&token(), Method::POST, "/v1.0/me/messages/d1/send", 202)
        .expect_err("a 401");
    assert_eq!(error, CallError::Unauthorized);
    assert_eq!(error.effect(), Effect::NotMade(NotMade::Unauthorized));
    assert!(seen.recv_timeout(LIMIT).is_ok());
    assert!(seen.recv_timeout(QUIET).is_err(), "one request");
}

#[test]
fn another_status_outside_2xx_is_named() {
    for (status, line) in [(404, "404 Not Found"), (500, "500 Internal Server Error")] {
        let (port, _seen) = listen(answer(line, "{}"));
        let client = graph(port, LIMIT);
        assert_eq!(
            client.effect(&token(), Method::DELETE, "/v1.0/me/events/e1", 204),
            Err(CallError::Status(status, None))
        );
        assert_eq!(
            client.json(&token(), Method::GET, "/v1.0/me/x", None, 200),
            Err(CallError::Status(status, None))
        );
    }
}

fn answer_with(status: &str, header: &str, body: &str) -> Reply {
    Reply::Bytes(format!(
        "HTTP/1.1 {status}\r\n{header}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    ))
}

#[test]
fn an_answer_429_or_5xx_carries_the_seconds_of_retry_after_and_no_other_does() {
    let rows = [
        ("429 Too Many Requests", 429, "Retry-After: 30", Some(30)),
        ("429 Too Many Requests", 429, "retry-after: 0", Some(0)),
        (
            "500 Internal Server Error",
            500,
            "Retry-After: 4294967295",
            Some(u32::MAX),
        ),
        (
            "503 Service Unavailable",
            503,
            "Retry-After: 120",
            Some(120),
        ),
        ("599 Other", 599, "Retry-After: 7", Some(7)),
        ("429 Too Many Requests", 429, "X-Other: 30", None),
        (
            "429 Too Many Requests",
            429,
            "Retry-After: Wed, 21 Oct 2026 07:28:00 GMT",
            None,
        ),
        (
            "429 Too Many Requests",
            429,
            "Retry-After: 4294967296",
            None,
        ),
        ("429 Too Many Requests", 429, "Retry-After: -1", None),
        ("429 Too Many Requests", 429, "Retry-After: +5", None),
        ("429 Too Many Requests", 429, "Retry-After: 1.5", None),
        ("429 Too Many Requests", 429, "Retry-After: ", None),
        ("404 Not Found", 404, "Retry-After: 30", None),
        ("400 Bad Request", 400, "Retry-After: 30", None),
        ("499 Other", 499, "Retry-After: 30", None),
        ("600 Other", 600, "Retry-After: 30", None),
        ("302 Found", 302, "Retry-After: 30", None),
        ("201 Created", 201, "Retry-After: 30", None),
    ];
    let mut answered = Vec::new();
    for (line, status, header, seconds) in rows {
        let (port, seen) = listen(answer_with(line, header, "{}"));
        let client = graph(port, LIMIT);
        assert_eq!(
            client.effect(&token(), Method::POST, "/v1.0/me/messages/d1/send", 202),
            Err(CallError::Status(status, seconds)),
            "{line} {header}"
        );
        assert_eq!(
            client.json(&token(), Method::GET, "/v1.0/me/x", None, 200),
            Err(CallError::Status(status, seconds)),
            "{line} {header}"
        );
        assert!(seen.recv_timeout(LIMIT).is_ok());
        assert!(seen.recv_timeout(LIMIT).is_ok());
        answered.push(seen);
    }
    thread::sleep(QUIET);
    assert!(
        answered.iter().all(|seen| seen.try_recv().is_err()),
        "no request is sent again"
    );
}

#[test]
fn an_answer_with_two_retry_after_lines_carries_no_seconds() {
    for header in [
        "Retry-After: 5\r\nRetry-After: 600",
        "Retry-After: 5\r\nRetry-After: 5",
    ] {
        let (port, seen) = listen(answer_with("429 Too Many Requests", header, "{}"));
        let error = graph(port, LIMIT)
            .effect(&token(), Method::POST, "/v1.0/me/messages/d1/send", 202)
            .expect_err("a 429");
        assert!(seen.recv_timeout(LIMIT).is_ok());
        assert_eq!(error, CallError::Status(429, None), "{header}");
    }
}

#[test]
fn a_send_answered_429_is_throttled_with_its_seconds_and_unknown_after_a_5xx() {
    let (port, _seen) = listen(answer_with("429 Too Many Requests", "Retry-After: 9", "{}"));
    let error = graph(port, LIMIT)
        .effect(&token(), Method::POST, "/v1.0/me/messages/d1/send", 202)
        .expect_err("a 429");
    assert_eq!(error, CallError::Status(429, Some(9)));
    assert_eq!(error.effect(), Effect::NotMade(NotMade::Throttled));
    let (port, _seen) = listen(answer_with(
        "503 Service Unavailable",
        "Retry-After: 9",
        "{}",
    ));
    let error = graph(port, LIMIT)
        .effect(&token(), Method::DELETE, "/v1.0/me/events/e1", 204)
        .expect_err("a 503");
    assert_eq!(error, CallError::Status(503, Some(9)));
    assert_eq!(error.effect(), Effect::Unknown);
}

#[test]
fn a_redirect_is_not_followed() {
    let (port, seen) = listen(Reply::Bytes(
        "HTTP/1.1 302 Found\r\nLocation: /v1.0/me/elsewhere\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            .to_owned(),
    ));
    let error = graph(port, LIMIT)
        .effect(&token(), Method::DELETE, "/v1.0/me/events/e1", 204)
        .expect_err("a 302");
    assert_eq!(error, CallError::Status(302, None));
    assert_eq!(error.effect(), Effect::Unknown);
    assert!(seen.recv_timeout(LIMIT).is_ok());
    assert!(seen.recv_timeout(QUIET).is_err(), "one request");
}

#[test]
fn the_success_status_alone_makes_the_effect_and_its_body_is_not_read() {
    for (line, success) in [("202 Accepted", 202), ("204 No Content", 204)] {
        let (port, _seen) = listen(answer(line, "not json"));
        assert_eq!(
            graph(port, LIMIT).effect(&token(), Method::POST, "/v1.0/me/x", success),
            Ok(()),
            "{line}"
        );
    }
}

#[test]
fn every_other_status_is_named_and_its_effect_is_unknown() {
    let others = [
        ("200 OK", 200),
        ("201 Created", 201),
        ("203 Other", 203),
        ("204 No Content", 204),
        ("299 Other", 299),
        ("300 Multiple Choices", 300),
        ("301 Moved Permanently", 301),
    ];
    for (line, status) in others {
        let (port, _seen) = listen(answer(line, "{}"));
        let error = graph(port, LIMIT)
            .effect(&token(), Method::POST, "/v1.0/me/messages/d1/send", 202)
            .expect_err("not the success status of send");
        assert_eq!(error, CallError::Status(status, None), "{line}");
        assert_eq!(error.effect(), Effect::Unknown, "{line}");
    }
    for (line, status) in others.into_iter().skip(1) {
        let (port, _seen) = listen(answer(line, "{\"value\":[]}"));
        assert_eq!(
            graph(port, LIMIT).json(&token(), Method::GET, "/v1.0/me/x", None, 200),
            Err(CallError::Status(status, None)),
            "{line}"
        );
    }
}

#[test]
fn an_answer_that_is_not_json_is_refused() {
    let (port, _seen) = listen(answer("200 OK", "not json"));
    assert_eq!(
        graph(port, LIMIT).json(&token(), Method::GET, "/v1.0/me/x", None, 200),
        Err(CallError::Answer(AnswerFault::Form))
    );
}

#[test]
fn a_closed_port_is_not_sent() {
    let (_ports, port) = closed_port();
    let error = graph(port, LIMIT)
        .effect(&token(), Method::POST, "/v1.0/me/messages/d1/send", 202)
        .expect_err("no listener");
    assert_eq!(error, CallError::NotSent);
    assert_eq!(error.effect(), Effect::NotMade(NotMade::NotSent));
}

#[test]
fn listen_binds_no_port_while_a_closed_port_is_held() {
    let (held, _port) = closed_port();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(listen(Reply::Close).0);
    });
    assert_eq!(
        rx.recv_timeout(Duration::from_millis(300)),
        Err(mpsc::RecvTimeoutError::Timeout),
        "listen bound a port while a closed port was held"
    );
    drop(held);
    assert!(
        rx.recv_timeout(LIMIT).is_ok(),
        "listen binds once the port is let go"
    );
}

#[test]
fn a_panic_while_the_ports_are_held_leaves_them_usable() {
    let panicked = thread::spawn(|| {
        let _held = ports();
        panic!("a test failed while it held the ports");
    })
    .join();
    assert!(panicked.is_err());
    assert!(
        PORTS.is_poisoned(),
        "the panic left the ports lock poisoned"
    );
    let (held, _port) = closed_port();
    drop(held);
}

#[test]
fn a_held_answer_is_no_answer_and_its_effect_unknown() {
    let (port, seen) = listen(Reply::Hold);
    let error = graph(port, Duration::from_millis(300))
        .effect(&token(), Method::POST, "/v1.0/me/messages/d1/send", 202)
        .expect_err("no answer");
    assert_eq!(error, CallError::NoAnswer);
    assert_eq!(error.effect(), Effect::Unknown);
    assert!(text(&seen).starts_with("POST /v1.0/me/messages/d1/send HTTP/1.1\r\n"));
}

#[test]
fn an_https_origin_speaks_tls_first_and_a_failed_handshake_sends_nothing() {
    let (port, seen) = listen(Reply::Close);
    let client =
        Graph::new(&origin("https", port), Route::Loopback, LIMIT).expect("the client builds");
    assert_eq!(
        client.json(&token(), Method::GET, "/v1.0/me/x", None, 200),
        Err(CallError::NotSent)
    );
    let first = seen.recv_timeout(LIMIT).expect("a connection");
    assert_eq!(first.first(), Some(&0x16), "a TLS handshake record");
    let lossy = String::from_utf8_lossy(&first);
    assert!(!lossy.contains("unit-token-1"));
    assert!(!lossy.contains("/v1.0/"));
}
