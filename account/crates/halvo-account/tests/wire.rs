mod support;

use halvo_account::sign_in::SignIn;
use halvo_secrets::Read;
use halvo_wire::request::{AccountId, Credential, HEADER_NAMES};
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::io::{Seek, Write};
use std::process::Stdio;
use support::{helper, keep_sign_in, scratch, store_at, Listener, Ran, Seen};

const TRANSPORT: &str = include_str!("../transport-headers.txt");

type Row = (
    &'static [&'static str],
    &'static str,
    &'static str,
    &'static str,
    Option<&'static str>,
    &'static str,
);

fn transport_names() -> Vec<String> {
    TRANSPORT
        .lines()
        .map(|line| {
            let (name, reason) = line.split_once('\t').expect("a name, a tab and a reason");
            assert!(reason.len() > 20, "{name} has no reason");
            name.to_string()
        })
        .collect()
}

fn backend_and_store(origin: &str, dir: &std::path::Path) -> Vec<(&'static str, OsString)> {
    vec![
        ("HALVO_ACCOUNT_BACKEND", OsString::from(origin)),
        ("HALVO_ACCOUNT_STORE_DIR", dir.as_os_str().to_owned()),
    ]
}

fn exchange(
    words: &[&str],
    sign_in: Option<&SignIn>,
    status: u16,
    body: &str,
) -> (Ran, Seen, String) {
    let dir = scratch("wire").join("store");
    if let Some(sign_in) = sign_in {
        keep_sign_in(&dir, sign_in);
    }
    let answering = Listener::new().answer(status, "", body);
    let origin = answering.origin.clone();
    let ran = helper(
        words,
        &backend_and_store(&answering.origin, &dir),
        Stdio::null(),
    );
    let seen = answering
        .seen()
        .unwrap_or_else(|| panic!("{words:?} sent nothing: {}", ran.stderr));
    (ran, seen, origin)
}

fn kept() -> SignIn {
    SignIn::from_random(&[0x5a; 48])
}

#[test]
fn the_transport_header_list_names_lower_case_headers_outside_the_account_headers() {
    let names = transport_names();
    assert!(!names.is_empty());
    let unique: BTreeSet<&String> = names.iter().collect();
    assert_eq!(unique.len(), names.len());
    for name in &names {
        assert!(
            name.bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte == b'-'),
            "{name}"
        );
        assert!(!HEADER_NAMES.contains(&name.as_str()), "{name}");
    }
}

#[test]
fn each_command_sends_its_route_with_the_stored_sign_in_and_only_the_listed_headers() {
    let table: [Row; 6] = [
        (
            &["status"],
            r#"{"result":"active"}"#,
            "GET",
            "/v1/subscription",
            None,
            "result=active",
        ),
        (
            &["cap"],
            r#"{"result":"no-cap"}"#,
            "GET",
            "/v1/cap",
            None,
            "result=no-cap",
        ),
        (
            &["set-cap", "5", "month"],
            r#"{"result":"pending"}"#,
            "PUT",
            "/v1/cap",
            Some(r#"{"cap":{"amount":5,"period":"month"}}"#),
            "result=pending",
        ),
        (
            &["--print-url", "manage"],
            r#"{"result":"unavailable"}"#,
            "POST",
            "/v1/manage",
            None,
            "result=unavailable",
        ),
        (
            &["--print-url", "subscribe", "7", "week"],
            r#"{"result":"account-exists"}"#,
            "POST",
            "/v1/checkout",
            Some(r#"{"cap":{"amount":7,"period":"week"}}"#),
            "result=account-exists",
        ),
        (
            &["delete"],
            r#"{"result":"not-finished"}"#,
            "DELETE",
            "/v1/account",
            None,
            "result=not-finished credential=kept",
        ),
    ];
    let allowed: BTreeSet<String> = HEADER_NAMES
        .iter()
        .map(|name| name.to_string())
        .chain(transport_names())
        .collect();
    let mut every_name = BTreeSet::new();
    let sign_in = kept();
    for (words, answer, method, target, body, line) in table {
        let (ran, seen, origin) = exchange(words, Some(&sign_in), 200, answer);
        assert_eq!(
            (ran.code, ran.stdout.as_str()),
            (0, &*format!("{line}\n")),
            "{words:?}"
        );
        assert_eq!(
            (seen.method.as_str(), seen.target.as_str()),
            (method, target)
        );
        assert_eq!(seen.version, "HTTP/1.1");
        assert_eq!(seen.header("host"), origin.strip_prefix("http://"));
        assert_eq!(
            seen.header("halvo-account"),
            Some(sign_in.account().as_str())
        );
        let bearer = format!("Bearer {}", sign_in.credential().as_str());
        assert_eq!(seen.header("authorization"), Some(bearer.as_str()));
        assert_eq!(
            seen.header("content-type"),
            body.map(|_| "application/json"),
            "{words:?}"
        );
        assert_eq!(seen.body, body.unwrap_or("").as_bytes(), "{words:?}");
        let names = seen.header_names();
        let unique: BTreeSet<&String> = names.iter().collect();
        assert_eq!(
            unique.len(),
            names.len(),
            "{words:?} repeats a header: {names:?}"
        );
        for name in &names {
            assert!(allowed.contains(name), "{words:?} sent the header {name}");
            every_name.insert(name.clone());
        }
    }
    for name in transport_names() {
        assert!(every_name.contains(&name), "no request carried {name}");
    }
}

#[test]
fn delete_removes_the_stored_sign_in_after_the_backend_deleted_the_account() {
    let dir = scratch("delete").join("store");
    keep_sign_in(&dir, &kept());
    let answering = Listener::new().answer(200, "", r#"{"result":"deleted"}"#);
    let ran = helper(
        &["delete"],
        &backend_and_store(&answering.origin, &dir),
        Stdio::null(),
    );
    assert!(answering.seen().is_some());
    assert_eq!(
        (ran.code, ran.stdout.as_str()),
        (0, "result=deleted credential=removed\n")
    );
    assert!(matches!(store_at(&dir).read(), Read::NotStored));
}

#[test]
fn subscribe_on_an_empty_store_stores_the_sign_in_that_it_sends() {
    let mut accounts = BTreeSet::new();
    for _ in 0..2 {
        let dir = scratch("subscribe").join("store");
        let answering = Listener::new().answer(200, "", r#"{"result":"unavailable"}"#);
        let ran = helper(
            &["--print-url", "subscribe", "3", "week"],
            &backend_and_store(&answering.origin, &dir),
            Stdio::null(),
        );
        let seen = answering.seen().expect("checkout start");
        assert_eq!((ran.code, ran.stdout.as_str()), (0, "result=unavailable\n"));
        let stored = match store_at(&dir).read() {
            Read::Found(secret) => SignIn::from_secret(&secret).expect("a sign-in"),
            other => panic!("the store answered {other:?}"),
        };
        assert!(AccountId::parse(stored.account().as_str()).is_some());
        assert!(Credential::parse(stored.credential().as_str()).is_some());
        assert_eq!(
            seen.header("halvo-account"),
            Some(stored.account().as_str())
        );
        let bearer = format!("Bearer {}", stored.credential().as_str());
        assert_eq!(seen.header("authorization"), Some(bearer.as_str()));
        accounts.insert(stored.account().as_str().to_string());
    }
    assert_eq!(accounts.len(), 2);
}

#[test]
fn an_argument_outside_the_grammar_sends_nothing_and_stores_nothing() {
    for words in [
        &["status", "x"][..],
        &["--help"],
        &["--print-url", "subscribe", "0", "week"],
        &["--print-url", "subscribe", "5", "fortnight"],
        &["canary-1f2e3d4c"],
        &["--print-url", "--print-url", "status"],
        &["delete", "--print-url"],
        &[],
    ] {
        let dir = scratch("usage").join("store");
        let listener = Listener::new();
        let ran = helper(
            words,
            &backend_and_store(&listener.origin(), &dir),
            Stdio::null(),
        );
        assert_eq!(
            (ran.code, ran.stdout.as_str()),
            (64, "error=usage\n"),
            "{words:?}"
        );
        assert!(!listener.was_reached(), "{words:?}");
        assert!(!dir.exists(), "{words:?}");
    }
}

#[test]
fn without_the_store_directory_nothing_is_stored_read_or_sent() {
    let home = scratch("home");
    let listener = Listener::new();
    for words in [
        &["--print-url", "subscribe", "5", "week"][..],
        &["status"],
        &["delete"],
    ] {
        let ran = helper(
            words,
            &[
                ("HALVO_ACCOUNT_BACKEND", OsString::from(listener.origin())),
                ("HOME", home.as_os_str().to_owned()),
            ],
            Stdio::null(),
        );
        assert_eq!(
            (ran.code, ran.stdout.as_str()),
            (5, "error=no-store-directory\n"),
            "{words:?}"
        );
    }
    assert!(!listener.was_reached());
    assert_eq!(fs::read_dir(&home).expect("the home").count(), 0);
}

#[test]
fn without_a_backend_by_address_nothing_is_sent() {
    let dir = scratch("backend").join("store");
    keep_sign_in(&dir, &kept());
    let listener = Listener::new();
    let ran = helper(
        &["status"],
        &[("HALVO_ACCOUNT_STORE_DIR", dir.as_os_str().to_owned())],
        Stdio::null(),
    );
    assert_eq!((ran.code, ran.stdout.as_str()), (5, "error=no-backend\n"));
    for origin in [
        format!("http://localhost:{}", listener.port),
        format!("http://127.0.0.1:{}/", listener.port),
        format!("http://127.0.0.1:{}/v1", listener.port),
    ] {
        let ran = helper(
            &["status"],
            &backend_and_store(&origin, &dir),
            Stdio::null(),
        );
        assert_eq!(
            (ran.code, ran.stdout.as_str()),
            (5, "error=bad-backend\n"),
            "{origin}"
        );
    }
    assert!(!listener.was_reached());
}

#[test]
fn with_an_empty_store_the_binary_sends_nothing() {
    let dir = scratch("empty").join("store");
    let listener = Listener::new();
    for words in [
        &["status"][..],
        &["cap"],
        &["set-cap", "1", "week"],
        &["--print-url", "manage"],
        &["delete"],
    ] {
        let ran = helper(
            words,
            &backend_and_store(&listener.origin(), &dir),
            Stdio::null(),
        );
        assert_eq!(
            (ran.code, ran.stdout.as_str()),
            (3, "error=no-credential\n"),
            "{words:?}"
        );
    }
    assert!(!listener.was_reached());
}

#[test]
fn standard_input_is_not_read() {
    let dir = scratch("stdin").join("store");
    keep_sign_in(&dir, &kept());
    let input_path = scratch("stdin-file").join("input.txt");
    fs::File::create(&input_path)
        .and_then(|mut file| file.write_all(b"canary-standard-input\nstatus\n"))
        .expect("an input file");
    let mut input = fs::File::open(&input_path).expect("the input file");
    let answering = Listener::new().answer(200, "", r#"{"result":"active"}"#);
    let ran = helper(
        &["status"],
        &backend_and_store(&answering.origin, &dir),
        Stdio::from(input.try_clone().expect("a second handle")),
    );
    let seen = answering.seen().expect("status");
    assert_eq!((ran.code, ran.stdout.as_str()), (0, "result=active\n"));
    assert!(!String::from_utf8_lossy(&seen.body).contains("canary"));
    assert_eq!(input.stream_position().expect("a position"), 0);
}

#[test]
fn the_proxy_variables_are_not_read() {
    let dir = scratch("proxy").join("store");
    keep_sign_in(&dir, &kept());
    let proxy = Listener::new();
    let answering = Listener::new().answer(200, "", r#"{"result":"active"}"#);
    let mut set = backend_and_store(&answering.origin, &dir);
    for name in [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "all_proxy",
    ] {
        set.push((name, OsString::from(proxy.origin())));
    }
    let ran = helper(&["status"], &set, Stdio::null());
    assert!(answering.seen().is_some());
    assert_eq!((ran.code, ran.stdout.as_str()), (0, "result=active\n"));
    assert!(!proxy.was_reached());
}

#[test]
fn a_redirect_is_not_followed() {
    let dir = scratch("redirect").join("store");
    keep_sign_in(&dir, &kept());
    let elsewhere = Listener::new();
    let location = format!("location: {}/v1/subscription\r\n", elsewhere.origin());
    let answering = Listener::new().answer(302, &location, r#"{"result":"active"}"#);
    let ran = helper(
        &["status"],
        &backend_and_store(&answering.origin, &dir),
        Stdio::null(),
    );
    assert!(answering.seen().is_some());
    assert_eq!((ran.code, ran.stdout.as_str()), (1, "error=failed-call\n"));
    assert!(!elsewhere.was_reached());
}

#[test]
fn a_body_longer_than_the_limit_is_no_answer() {
    let answer = r#"{"result":"active"}"#;
    for (length, code, line) in [
        (65_536, 0, "result=active\n"),
        (65_537, 1, "error=failed-call\n"),
    ] {
        let dir = scratch("long").join("store");
        keep_sign_in(&dir, &kept());
        let body = format!("{answer}{}", " ".repeat(length - answer.len()));
        let answering = Listener::new().answer(200, "", &body);
        let ran = helper(
            &["status"],
            &backend_and_store(&answering.origin, &dir),
            Stdio::null(),
        );
        assert!(answering.seen().is_some());
        assert_eq!((ran.code, ran.stdout.as_str()), (code, line), "{length}");
    }
}

#[test]
fn a_failed_call_says_so_on_standard_error_and_keeps_the_sign_in() {
    let dir = scratch("failed").join("store");
    keep_sign_in(&dir, &kept());
    let answering = Listener::new().answer(500, "", "");
    let ran = helper(
        &["delete"],
        &backend_and_store(&answering.origin, &dir),
        Stdio::null(),
    );
    assert!(answering.seen().is_some());
    assert_eq!((ran.code, ran.stdout.as_str()), (1, "error=failed-call\n"));
    assert!(ran.stderr.starts_with("halvo-account: the call failed"));
    assert!(matches!(store_at(&dir).read(), Read::Found(_)));
}

#[test]
fn a_run_sends_one_request_also_after_a_redirect_to_the_same_origin_or_a_server_error() {
    for (status, path) in [
        (302, Some("/v1/cap")),
        (307, Some("/v1/subscription")),
        (500, None),
        (503, None),
    ] {
        let dir = scratch("one-request").join("store");
        keep_sign_in(&dir, &kept());
        let listener = Listener::new();
        let extra = path
            .map(|path| format!("location: {}{path}\r\n", listener.origin()))
            .unwrap_or_default();
        let counting = listener.answer_each(status, &extra, r#"{"result":"active"}"#);
        let ran = helper(
            &["status"],
            &backend_and_store(&counting.origin, &dir),
            Stdio::null(),
        );
        let seen = counting.seen_all();
        assert_eq!(
            (ran.code, ran.stdout.as_str()),
            (1, "error=failed-call\n"),
            "{status}"
        );
        assert_eq!(seen.len(), 1, "{status} brought {} requests", seen.len());
    }
}

fn supported_versions(record: &[u8]) -> Vec<u16> {
    assert_eq!(record[0], 0x16, "a handshake record");
    let hello = &record[5..];
    assert_eq!(hello[0], 0x01, "a ClientHello");
    let pair = |at: usize| usize::from(u16::from_be_bytes([hello[at], hello[at + 1]]));
    let mut at = 4 + 2 + 32;
    at += 1 + usize::from(hello[at]);
    at += 2 + pair(at);
    at += 1 + usize::from(hello[at]);
    let end = at + 2 + pair(at);
    at += 2;
    while at + 4 <= end {
        let kind = pair(at);
        let length = pair(at + 2);
        let data = &hello[at + 4..at + 4 + length];
        if kind == 0x002b {
            return data[1..]
                .chunks(2)
                .map(|two| u16::from_be_bytes([two[0], two[1]]))
                .collect();
        }
        at += 4 + length;
    }
    Vec::new()
}

#[test]
fn the_client_offers_tls_1_3_alone() {
    let dir = scratch("tls").join("store");
    keep_sign_in(&dir, &kept());
    let listener = Listener::new();
    let origin = format!("https://127.0.0.1:{}", listener.port);
    let hello = listener.first_record();
    let ran = helper(
        &["status"],
        &backend_and_store(&origin, &dir),
        Stdio::null(),
    );
    let record = hello
        .join()
        .expect("the listener thread")
        .expect("a ClientHello");
    assert_eq!((ran.code, ran.stdout.as_str()), (1, "error=failed-call\n"));
    assert_eq!(supported_versions(&record), [0x0304]);
}
