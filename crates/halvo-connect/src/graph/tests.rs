use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use halvo_schema::executable::WebOrigin;
use reqwest::Method;
use serde_json::json;

use super::{segment, Graph, Route};
use crate::error::{AnswerFault, BuildError, CallError, Effect};
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

fn listen(reply: Reply) -> (u16, mpsc::Receiver<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
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
    assert_eq!(error.effect(), Effect::NotMade);
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
            Err(CallError::Status(status))
        );
        assert_eq!(
            client.json(&token(), Method::GET, "/v1.0/me/x", None, 200),
            Err(CallError::Status(status))
        );
    }
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
    assert_eq!(error, CallError::Status(302));
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
        assert_eq!(error, CallError::Status(status), "{line}");
        assert_eq!(error.effect(), Effect::Unknown, "{line}");
    }
    for (line, status) in others.into_iter().skip(1) {
        let (port, _seen) = listen(answer(line, "{\"value\":[]}"));
        assert_eq!(
            graph(port, LIMIT).json(&token(), Method::GET, "/v1.0/me/x", None, 200),
            Err(CallError::Status(status)),
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
    let port = {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
        listener.local_addr().expect("its address").port()
    };
    let error = graph(port, LIMIT)
        .effect(&token(), Method::POST, "/v1.0/me/messages/d1/send", 202)
        .expect_err("no listener");
    assert_eq!(error, CallError::NotSent);
    assert_eq!(error.effect(), Effect::NotMade);
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
