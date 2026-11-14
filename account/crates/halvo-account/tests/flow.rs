use halvo_account::answer::Reply;
use halvo_account::flow::{
    run, Keep, NoAnswer, NoRandom, NotOpened, Opener, Random, Reach, Transport,
};
use halvo_account::grammar::{parse, Build};
use halvo_account::origin::{PageUrl, Pages};
use halvo_account::outcome::{Failure, Outcome};
use halvo_account::sign_in::SignIn;
use halvo_secrets::{Delete, Fault, Locked, Read, Secret, Write};
use halvo_wire::request::Request;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::ffi::OsString;

const SAMPLES: &str = include_str!("../../../schema/samples.v1.json");
const SESSION_URL: &str = "http://127.0.0.1:1/x";

type Row = (
    &'static [&'static str],
    &'static str,
    &'static str,
    Option<&'static str>,
    &'static str,
    &'static str,
);

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Normal,
    Occupied,
    Locked,
    Fault,
}

/// What the store does once the request is sent: what a second run's delete, or a third run's
/// subscribe, does while the answer is on its way.
#[derive(Clone)]
enum After {
    /// The store holds these bytes.
    Holds(Vec<u8>),
    /// The store holds nothing.
    Empty,
    /// Each read answers in this mode.
    Reads(Mode),
}

#[derive(Debug, PartialEq)]
struct Sent {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
}

struct World {
    log: RefCell<Vec<&'static str>>,
    stored: RefCell<Option<Vec<u8>>>,
    read_mode: Mode,
    write_mode: Mode,
    delete_mode: Mode,
    after_send: Option<After>,
    replies: RefCell<VecDeque<Result<Reply, NoAnswer>>>,
    sent: RefCell<Vec<Sent>>,
    opened: RefCell<Vec<String>>,
    open_fails: bool,
    random: Option<[u8; 48]>,
}

impl World {
    fn new() -> World {
        World {
            log: RefCell::new(Vec::new()),
            stored: RefCell::new(None),
            read_mode: Mode::Normal,
            write_mode: Mode::Normal,
            delete_mode: Mode::Normal,
            after_send: None,
            replies: RefCell::new(VecDeque::new()),
            sent: RefCell::new(Vec::new()),
            opened: RefCell::new(Vec::new()),
            open_fails: false,
            random: Some([3; 48]),
        }
    }

    fn holding(sign_in: &SignIn) -> World {
        let world = World::new();
        world.store_bytes(sign_in.to_secret().expose());
        world
    }

    fn store_bytes(&self, bytes: &[u8]) {
        *self.stored.borrow_mut() = Some(bytes.to_vec());
    }

    fn answering(self, status: u16, body: &str) -> World {
        self.replies.borrow_mut().push_back(Ok(Reply {
            status,
            body: body.as_bytes().to_vec(),
        }));
        self
    }

    fn silent(self) -> World {
        self.replies.borrow_mut().push_back(Err(NoAnswer));
        self
    }

    fn log(&self) -> Vec<&'static str> {
        self.log.borrow().clone()
    }

    fn stored(&self) -> Option<Vec<u8>> {
        self.stored.borrow().clone()
    }

    fn run(&self, words: &[&str]) -> Outcome {
        self.run_with(words, &Pages::Loopback)
    }

    fn run_with(&self, words: &[&str], pages: &Pages) -> Outcome {
        let args: Vec<OsString> = words.iter().map(OsString::from).collect();
        let invocation = parse(&args, Build::Debug).expect("words of the grammar");
        run(
            &invocation,
            &Reach {
                keep: self,
                transport: self,
                opener: self,
                random: self,
                pages,
            },
        )
    }
}

impl Keep for World {
    fn read(&self) -> Read {
        let mode = match &self.after_send {
            Some(After::Reads(mode)) if self.log.borrow().contains(&"send") => *mode,
            _ => self.read_mode,
        };
        self.log.borrow_mut().push("read");
        match mode {
            Mode::Locked => Read::Locked(Locked::Mode),
            Mode::Fault => Read::Fault(Fault { code: 5 }),
            _ => match self.stored.borrow().as_ref() {
                None => Read::NotStored,
                Some(bytes) => Read::Found(Secret::new(bytes.clone()).expect("a secret")),
            },
        }
    }

    fn write(&self, secret: &Secret) -> Write {
        self.log.borrow_mut().push("write");
        match self.write_mode {
            Mode::Occupied => Write::Occupied,
            Mode::Locked => Write::Locked(Locked::Owner),
            Mode::Fault => Write::Fault(Fault { code: 28 }),
            Mode::Normal => {
                let mut stored = self.stored.borrow_mut();
                if stored.is_some() {
                    return Write::Occupied;
                }
                *stored = Some(secret.expose().to_vec());
                Write::Stored
            }
        }
    }

    fn delete(&self) -> Delete {
        self.log.borrow_mut().push("delete");
        match self.delete_mode {
            Mode::Locked => Delete::Locked(Locked::Acl),
            Mode::Fault => Delete::Fault(Fault { code: 5 }),
            _ => match self.stored.borrow_mut().take() {
                Some(_) => Delete::Deleted,
                None => Delete::NotStored,
            },
        }
    }
}

impl Transport for World {
    fn send(&self, request: &Request) -> Result<Reply, NoAnswer> {
        self.log.borrow_mut().push("send");
        self.sent.borrow_mut().push(Sent {
            method: request.method().to_string(),
            path: request.path().to_string(),
            headers: request
                .headers()
                .iter()
                .map(|(name, value)| (name.to_string(), value.clone()))
                .collect(),
            body: request.body().map(<[u8]>::to_vec),
        });
        match &self.after_send {
            Some(After::Holds(bytes)) => *self.stored.borrow_mut() = Some(bytes.clone()),
            Some(After::Empty) => *self.stored.borrow_mut() = None,
            Some(After::Reads(_)) | None => {}
        }
        self.replies
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(|| panic!("a request that the test did not answer"))
    }
}

impl Opener for World {
    fn open(&self, page: &PageUrl) -> Result<(), NotOpened> {
        self.log.borrow_mut().push("open");
        self.opened.borrow_mut().push(page.as_str().to_string());
        if self.open_fails {
            Err(NotOpened)
        } else {
            Ok(())
        }
    }
}

impl Random for World {
    fn fill(&self, bytes: &mut [u8; 48]) -> Result<(), NoRandom> {
        let source = self.random.ok_or(NoRandom)?;
        bytes.copy_from_slice(&source);
        Ok(())
    }
}

fn sign_in(seed: u8) -> SignIn {
    let mut bytes = [0; 48];
    for (at, byte) in bytes.iter_mut().enumerate() {
        *byte = seed.wrapping_mul(31).wrapping_add(at as u8);
    }
    SignIn::from_random(&bytes)
}

fn account_headers(sign_in: &SignIn) -> Vec<(String, String)> {
    vec![
        (
            "halvo-account".to_string(),
            sign_in.account().as_str().to_string(),
        ),
        (
            "authorization".to_string(),
            format!("Bearer {}", sign_in.credential().as_str()),
        ),
    ]
}

fn with_body(sign_in: &SignIn) -> Vec<(String, String)> {
    let mut headers = account_headers(sign_in);
    headers.push(("content-type".to_string(), "application/json".to_string()));
    headers
}

fn samples() -> serde_json::Value {
    serde_json::from_str(SAMPLES).expect("the samples")
}

fn sample(route: &str, result: &str) -> String {
    samples()["answers"][route][result].to_string()
}

fn results_of(route: &str) -> Vec<String> {
    samples()["answers"][route]
        .as_object()
        .expect("a route of the samples")
        .keys()
        .cloned()
        .collect()
}

const ALL: [&[&str]; 6] = [
    &["status"],
    &["subscribe", "5", "week"],
    &["set-cap", "5", "week"],
    &["cap"],
    &["manage"],
    &["delete"],
];

const ALL_BUT_SUBSCRIBE: [&[&str]; 5] = [
    &["status"],
    &["set-cap", "5", "week"],
    &["cap"],
    &["manage"],
    &["delete"],
];

fn printing<'a>(words: &[&'a str]) -> Vec<&'a str> {
    let mut all = vec!["--print-url"];
    all.extend_from_slice(words);
    all
}

#[test]
fn with_an_empty_store_every_command_but_subscribe_sends_nothing() {
    for words in ALL_BUT_SUBSCRIBE {
        for words in [words.to_vec(), printing(words)] {
            let world = World::new();
            let outcome = world.run(&words);
            assert_eq!(
                (outcome.code(), outcome.line().as_str()),
                (3, "error=no-credential"),
                "{words:?}"
            );
            assert_eq!(world.log(), ["read"], "{words:?}");
            assert_eq!(world.stored(), None);
        }
    }
}

#[test]
fn a_store_that_answers_locked_or_fault_makes_no_credential_and_sends_nothing() {
    for (mode, line) in [
        (Mode::Locked, "error=store-locked"),
        (Mode::Fault, "error=store-fault"),
    ] {
        for words in ALL {
            let world = World::new();
            let world = World {
                read_mode: mode,
                ..world
            };
            let outcome = world.run(words);
            assert_eq!(
                (outcome.code(), outcome.line().as_str()),
                (4, line),
                "{words:?}"
            );
            assert_eq!(world.log(), ["read"], "{words:?}");
            assert_eq!(world.stored(), None);
        }
    }
}

#[test]
fn a_stored_item_that_is_no_sign_in_is_kept_and_nothing_is_sent() {
    let good = sign_in(1).to_secret().expose().to_vec();
    let mut upper = good.clone();
    upper[40] = b'A';
    let mut newline = good.clone();
    newline[36] = b'\n';
    let mut longer = good.clone();
    longer.push(b'\n');
    let mut wrong_version = good.clone();
    wrong_version[14] = b'5';
    let items: Vec<Vec<u8>> = vec![
        b"x".to_vec(),
        vec![b'a'; 101],
        upper,
        newline,
        longer,
        wrong_version,
        good[..100].to_vec(),
        vec![0xff; 101],
    ];
    for item in items {
        for words in ALL {
            let world = World::new();
            world.store_bytes(&item);
            let outcome = world.run(words);
            assert_eq!(
                (outcome.code(), outcome.line().as_str()),
                (4, "error=store-unreadable"),
                "{words:?}"
            );
            assert_eq!(world.log(), ["read"], "{words:?}");
            assert_eq!(world.stored(), Some(item.clone()));
        }
    }
}

#[test]
fn subscribe_with_an_empty_store_stores_a_new_sign_in_before_it_sends() {
    let world = World::new().answering(200, &sample("checkout-start", "session"));
    let outcome = world.run(&["--print-url", "subscribe", "7", "month"]);
    assert_eq!(
        (outcome.code(), outcome.line()),
        (0, format!("result=session url={SESSION_URL}"))
    );
    assert_eq!(world.log(), ["read", "write", "send"]);
    let made = SignIn::from_random(&[3; 48]);
    assert_eq!(world.stored(), Some(made.to_secret().expose().to_vec()));
    let sent = world.sent.borrow();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].headers, with_body(&made));
    assert_eq!(
        sent[0].body.as_deref(),
        Some(&br#"{"cap":{"amount":7,"period":"month"}}"#[..])
    );
}

#[test]
fn a_sign_in_that_was_stored_before_a_failed_call_is_the_one_that_the_next_subscribe_sends() {
    let world = World::new().silent();
    let outcome = world.run(&["--print-url", "subscribe", "3", "week"]);
    assert_eq!(
        (outcome.code(), outcome.line().as_str()),
        (1, "error=failed-call")
    );
    let first = world.stored().expect("the sign-in stays stored");
    let world = World::new().answering(200, &sample("checkout-start", "account-exists"));
    world.store_bytes(&first);
    let outcome = world.run(&["--print-url", "subscribe", "3", "week"]);
    assert_eq!(outcome.line(), "result=account-exists");
    assert_eq!(world.log(), ["read", "send"]);
    assert_eq!(world.stored(), Some(first.clone()));
    let made = SignIn::from_random(&[3; 48]);
    assert_eq!(world.sent.borrow()[0].headers, with_body(&made));
}

#[test]
fn subscribe_sends_nothing_when_the_write_of_a_new_sign_in_fails() {
    for (mode, line) in [
        (Mode::Occupied, "error=store-changed"),
        (Mode::Locked, "error=store-locked"),
        (Mode::Fault, "error=store-fault"),
    ] {
        let world = World {
            write_mode: mode,
            ..World::new()
        };
        let outcome = world.run(&["subscribe", "5", "week"]);
        assert_eq!((outcome.code(), outcome.line().as_str()), (4, line));
        assert_eq!(world.log(), ["read", "write"]);
        assert!(world.sent.borrow().is_empty());
    }
}

#[test]
fn subscribe_without_random_bytes_stores_nothing_and_sends_nothing() {
    let world = World {
        random: None,
        ..World::new()
    };
    let outcome = world.run(&["subscribe", "5", "week"]);
    assert_eq!(
        (outcome.code(), outcome.line().as_str()),
        (4, "error=no-random")
    );
    assert_eq!(world.log(), ["read"]);
    assert_eq!(world.stored(), None);
}

#[test]
fn subscribe_with_a_stored_sign_in_writes_nothing_and_sends_that_sign_in() {
    let kept = sign_in(4);
    let world = World::holding(&kept).answering(200, &sample("checkout-start", "unavailable"));
    let outcome = world.run(&["subscribe", "9", "week"]);
    assert_eq!(
        (outcome.code(), outcome.line().as_str()),
        (0, "result=unavailable")
    );
    assert_eq!(world.log(), ["read", "send"]);
    assert_eq!(world.sent.borrow()[0].headers, with_body(&kept));
}

#[test]
fn each_request_carries_the_values_of_its_call_and_of_the_stored_sign_in() {
    let table: [Row; 10] = [
        (
            &["status"],
            "GET",
            "/v1/subscription",
            None,
            "status",
            "active",
        ),
        (&["cap"], "GET", "/v1/cap", None, "cap-get", "no-cap"),
        (&["manage"], "POST", "/v1/manage", None, "manage", "pending"),
        (
            &["delete"],
            "DELETE",
            "/v1/account",
            None,
            "account-delete",
            "pending",
        ),
        (
            &["set-cap", "1", "week"],
            "PUT",
            "/v1/cap",
            Some(r#"{"cap":{"amount":1,"period":"week"}}"#),
            "cap-set",
            "pending",
        ),
        (
            &["set-cap", "100000000", "month"],
            "PUT",
            "/v1/cap",
            Some(r#"{"cap":{"amount":100000000,"period":"month"}}"#),
            "cap-set",
            "pending",
        ),
        (
            &["set-cap", "42", "month"],
            "PUT",
            "/v1/cap",
            Some(r#"{"cap":{"amount":42,"period":"month"}}"#),
            "cap-set",
            "pending",
        ),
        (
            &["subscribe", "1", "week"],
            "POST",
            "/v1/checkout",
            Some(r#"{"cap":{"amount":1,"period":"week"}}"#),
            "checkout-start",
            "unavailable",
        ),
        (
            &["subscribe", "99", "month"],
            "POST",
            "/v1/checkout",
            Some(r#"{"cap":{"amount":99,"period":"month"}}"#),
            "checkout-start",
            "unavailable",
        ),
        (
            &["subscribe", "100000000", "week"],
            "POST",
            "/v1/checkout",
            Some(r#"{"cap":{"amount":100000000,"period":"week"}}"#),
            "checkout-start",
            "unavailable",
        ),
    ];
    for seed in [11, 12] {
        let kept = sign_in(seed);
        for (words, method, path, body, route, result) in table {
            let world = World::holding(&kept).answering(200, &sample(route, result));
            world.run(words);
            let headers = match body {
                Some(_) => with_body(&kept),
                None => account_headers(&kept),
            };
            let expected = Sent {
                method: method.to_string(),
                path: path.to_string(),
                headers,
                body: body.map(|body| body.as_bytes().to_vec()),
            };
            assert_eq!(*world.sent.borrow(), vec![expected], "{words:?}");
        }
    }
    assert_ne!(
        sign_in(11).account().as_str(),
        sign_in(12).account().as_str()
    );
}

#[test]
fn delete_removes_the_credential_after_deleted_or_no_such_account_alone() {
    for result in ["deleted", "no-such-account"] {
        let world = World::holding(&sign_in(5)).answering(200, &sample("account-delete", result));
        let outcome = world.run(&["delete"]);
        assert_eq!(
            (outcome.code(), outcome.line()),
            (0, format!("result={result} credential=removed"))
        );
        assert_eq!(world.log(), ["read", "send", "read", "delete"]);
        assert_eq!(world.stored(), None);
    }
    for result in ["pending", "not-finished", "credential-mismatch"] {
        let kept = sign_in(5);
        let world = World::holding(&kept).answering(200, &sample("account-delete", result));
        let outcome = world.run(&["delete"]);
        assert_eq!(
            (outcome.code(), outcome.line()),
            (0, format!("result={result} credential=kept"))
        );
        assert_eq!(world.log(), ["read", "send"]);
        assert_eq!(world.stored(), Some(kept.to_secret().expose().to_vec()));
    }
}

#[test]
fn delete_keeps_the_credential_when_the_call_fails_or_is_rejected() {
    let kept = sign_in(6);
    let cases: Vec<(World, u8, &str)> = vec![
        (World::holding(&kept).silent(), 1, "error=failed-call"),
        (
            World::holding(&kept).answering(500, r#"{"result":"deleted"}"#),
            1,
            "error=failed-call",
        ),
        (
            World::holding(&kept).answering(302, r#"{"result":"deleted"}"#),
            1,
            "error=failed-call",
        ),
        (
            World::holding(&kept).answering(200, r#"{"result":"deleted","x":1}"#),
            1,
            "error=failed-call",
        ),
        (
            World::holding(&kept).answering(200, r#"["deleted"]"#),
            1,
            "error=failed-call",
        ),
        (
            World::holding(&kept).answering(200, r#"{"result":"deleted"} x"#),
            1,
            "error=failed-call",
        ),
        (
            World::holding(&kept).answering(400, r#"{"result":"rejected"}"#),
            2,
            "error=rejected",
        ),
    ];
    for (world, code, line) in cases {
        let outcome = world.run(&["delete"]);
        assert_eq!((outcome.code(), outcome.line().as_str()), (code, line));
        assert_eq!(world.log(), ["read", "send"]);
        assert_eq!(world.stored(), Some(kept.to_secret().expose().to_vec()));
    }
}

#[test]
fn a_delete_that_the_store_refuses_after_the_backend_deleted_says_that_the_credential_stays() {
    for (mode, line) in [
        (
            Mode::Locked,
            "result=deleted credential=kept error=store-locked",
        ),
        (
            Mode::Fault,
            "result=deleted credential=kept error=store-fault",
        ),
    ] {
        let world = World {
            delete_mode: mode,
            ..World::holding(&sign_in(7))
        }
        .answering(200, &sample("account-delete", "deleted"));
        let outcome = world.run(&["delete"]);
        assert_eq!((outcome.code(), outcome.line().as_str()), (4, line));
        assert_eq!(world.log(), ["read", "send", "read", "delete"]);
    }
}

#[test]
fn after_deleted_or_no_such_account_delete_removes_only_the_sign_in_that_it_sent() {
    let sent = sign_in(5);
    let bytes = sent.to_secret().expose().to_vec();
    let other = sign_in(6).to_secret().expose().to_vec();
    let mut credential_one_byte_off = bytes.clone();
    let last = credential_one_byte_off.len() - 1;
    credential_one_byte_off[last] = if bytes[last] == b'0' { b'1' } else { b'0' };
    let credential_under_another_id = format!(
        "{} {}",
        sign_in(6).account().as_str(),
        sent.credential().as_str()
    )
    .into_bytes();
    let another_form = b"no sign-in of this helper".to_vec();
    for (name, held) in [
        ("another sign-in", &other),
        (
            "the same id, one byte of the credential",
            &credential_one_byte_off,
        ),
        (
            "the same credential, another id",
            &credential_under_another_id,
        ),
    ] {
        assert!(
            SignIn::from_secret(&Secret::new(held.clone()).expect("a secret")).is_some(),
            "{name} is a sign-in"
        );
        assert_ne!(held, &bytes, "{name} is another sign-in");
    }
    // (case, what the store does once the request is sent, the mode of the delete,
    //  code, line after `result=R`, delete calls, what the store holds at the end)
    type Case<'a> = (
        &'a str,
        Option<After>,
        Mode,
        u8,
        &'a str,
        usize,
        Option<Vec<u8>>,
    );
    let cases: Vec<Case> = vec![
        (
            "the same sign-in",
            None,
            Mode::Normal,
            0,
            "credential=removed",
            1,
            None,
        ),
        (
            "the same sign-in, the delete answers locked",
            None,
            Mode::Locked,
            4,
            "credential=kept error=store-locked",
            1,
            Some(bytes.clone()),
        ),
        (
            "the same sign-in, the delete fails",
            None,
            Mode::Fault,
            4,
            "credential=kept error=store-fault",
            1,
            Some(bytes.clone()),
        ),
        (
            "nothing stored",
            Some(After::Empty),
            Mode::Normal,
            0,
            "credential=removed",
            0,
            None,
        ),
        (
            "another sign-in",
            Some(After::Holds(other.clone())),
            Mode::Normal,
            4,
            "credential=kept error=store-changed",
            0,
            Some(other.clone()),
        ),
        (
            "the same id, one byte of the credential",
            Some(After::Holds(credential_one_byte_off.clone())),
            Mode::Normal,
            4,
            "credential=kept error=store-changed",
            0,
            Some(credential_one_byte_off.clone()),
        ),
        (
            "the same credential, another id",
            Some(After::Holds(credential_under_another_id.clone())),
            Mode::Normal,
            4,
            "credential=kept error=store-changed",
            0,
            Some(credential_under_another_id.clone()),
        ),
        (
            "an item of another form",
            Some(After::Holds(another_form.clone())),
            Mode::Normal,
            4,
            "credential=kept error=store-unreadable",
            0,
            Some(another_form.clone()),
        ),
        (
            "the second read answers locked",
            Some(After::Reads(Mode::Locked)),
            Mode::Normal,
            4,
            "credential=kept error=store-locked",
            0,
            Some(bytes.clone()),
        ),
        (
            "the second read fails",
            Some(After::Reads(Mode::Fault)),
            Mode::Normal,
            4,
            "credential=kept error=store-fault",
            0,
            Some(bytes.clone()),
        ),
    ];
    for result in ["deleted", "no-such-account"] {
        for (name, after, delete_mode, code, line, deletes, held) in &cases {
            let world = World {
                after_send: after.clone(),
                delete_mode: *delete_mode,
                ..World::holding(&sent)
            }
            .answering(200, &sample("account-delete", result));
            let outcome = world.run(&["delete"]);
            let log = world.log();
            assert_eq!(
                (outcome.code(), outcome.line()),
                (*code, format!("result={result} {line}")),
                "{result}, {name}"
            );
            assert_eq!(log[..3], ["read", "send", "read"], "{result}, {name}");
            assert_eq!(
                log.iter().filter(|call| **call == "delete").count(),
                *deletes,
                "{result}, {name}: delete calls in {log:?}"
            );
            assert_eq!(world.stored(), *held, "{result}, {name}");
        }
    }
}

#[test]
fn a_url_of_2048_code_points_is_read_and_the_page_check_refuses_it() {
    let body = serde_json::json!({
        "result": "session",
        "url": "\u{1F600}".repeat(2048),
    })
    .to_string();
    for words in [
        &["--print-url", "subscribe", "5", "week"][..],
        &["subscribe", "5", "week"],
    ] {
        let world = World::holding(&sign_in(9)).answering(200, &body);
        let outcome = world.run(words);
        assert!(
            matches!(outcome, Outcome::Failed(Failure::PageRefused)),
            "{words:?}: {outcome:?}"
        );
        assert_eq!(
            (outcome.code(), outcome.line().as_str()),
            (1, "error=failed-call")
        );
        assert_eq!(world.log(), ["read", "send"], "{words:?}");
        assert!(world.opened.borrow().is_empty(), "{words:?}");
    }
}

#[test]
fn no_command_but_delete_removes_the_credential_whatever_it_is_answered() {
    let commands: [(&[&str], &str); 5] = [
        (&["status"], "status"),
        (&["--print-url", "subscribe", "5", "week"], "checkout-start"),
        (&["set-cap", "5", "week"], "cap-set"),
        (&["cap"], "cap-get"),
        (&["--print-url", "manage"], "manage"),
    ];
    let kept = sign_in(8);
    for (words, route) in commands {
        let mut worlds: Vec<World> = results_of(route)
            .iter()
            .map(|result| World::holding(&kept).answering(200, &sample(route, result)))
            .collect();
        worlds.push(World::holding(&kept).answering(404, r#"{"result":"rejected"}"#));
        worlds.push(World::holding(&kept).answering(503, ""));
        worlds.push(World::holding(&kept).answering(200, r#"{"result":"deleted"}"#));
        worlds.push(World::holding(&kept).silent());
        for world in worlds {
            world.run(words);
            assert!(!world.log().contains(&"delete"), "{words:?}");
            assert_eq!(
                world.log().iter().filter(|call| **call == "send").count(),
                1
            );
            assert_eq!(world.stored(), Some(kept.to_secret().expose().to_vec()));
        }
    }
}

#[test]
fn print_url_prints_the_page_and_opens_nothing() {
    for (words, route, result) in [
        (
            &["--print-url", "subscribe", "5", "week"][..],
            "checkout-start",
            "session",
        ),
        (&["--print-url", "manage"][..], "manage", "page"),
    ] {
        let world = World::holding(&sign_in(9)).answering(200, &sample(route, result));
        let outcome = world.run(words);
        assert_eq!(
            (outcome.code(), outcome.line()),
            (0, format!("result={result} url={SESSION_URL}"))
        );
        assert!(world.opened.borrow().is_empty());
        assert!(!world.log().contains(&"open"));
    }
}

#[test]
fn without_print_url_the_page_is_opened_and_not_printed() {
    for (words, route, result) in [
        (&["subscribe", "5", "week"][..], "checkout-start", "session"),
        (&["manage"][..], "manage", "page"),
    ] {
        let world = World::holding(&sign_in(9)).answering(200, &sample(route, result));
        let outcome = world.run(words);
        assert_eq!(
            (outcome.code(), outcome.line()),
            (0, format!("result={result}"))
        );
        assert_eq!(*world.opened.borrow(), vec![SESSION_URL.to_string()]);
        assert_eq!(world.log(), ["read", "send", "open"]);
    }
}

#[test]
fn a_page_that_could_not_be_opened_is_said_so() {
    let world = World {
        open_fails: true,
        ..World::holding(&sign_in(9))
    }
    .answering(200, &sample("manage", "page"));
    let outcome = world.run(&["manage"]);
    assert_eq!(
        (outcome.code(), outcome.line().as_str()),
        (6, "result=page error=not-opened")
    );
}

#[test]
fn a_page_outside_the_list_is_neither_opened_nor_printed() {
    for url in [
        "http://evil.example/x",
        "https://127.0.0.1/x",
        "http://127.0.0.1@evil.example/",
        "javascript:alert(1)",
    ] {
        for words in [
            &["--print-url", "subscribe", "5", "week"][..],
            &["subscribe", "5", "week"],
        ] {
            let body = format!(r#"{{"result":"session","url":"{url}"}}"#);
            let world = World::holding(&sign_in(9)).answering(200, &body);
            let outcome = world.run(words);
            assert_eq!(
                (outcome.code(), outcome.line().as_str()),
                (1, "error=failed-call")
            );
            assert!(world.opened.borrow().is_empty());
        }
        for words in [&["--print-url", "manage"][..], &["manage"]] {
            let body = format!(r#"{{"result":"page","url":"{url}"}}"#);
            let world = World::holding(&sign_in(9)).answering(200, &body);
            let outcome = world.run(words);
            assert_eq!(
                (outcome.code(), outcome.line().as_str()),
                (1, "error=failed-call")
            );
            assert!(world.opened.borrow().is_empty());
        }
    }
    let hosted = Pages::Hosted("https://checkout.example");
    let world = World::holding(&sign_in(9)).answering(200, &sample("manage", "page"));
    let outcome = world.run_with(&["manage"], &hosted);
    assert_eq!(outcome.code(), 1);
    assert!(world.opened.borrow().is_empty());
}

#[test]
fn results_without_a_page_open_nothing_and_print_no_url() {
    for (route, words) in [
        ("checkout-start", &["subscribe", "5", "week"][..]),
        ("manage", &["manage"][..]),
    ] {
        for result in results_of(route) {
            if result == "session" || result == "page" {
                continue;
            }
            for words in [words.to_vec(), printing(words)] {
                let world = World::holding(&sign_in(10)).answering(200, &sample(route, &result));
                let outcome = world.run(&words);
                assert_eq!(
                    (outcome.code(), outcome.line()),
                    (0, format!("result={result}"))
                );
                assert!(world.opened.borrow().is_empty());
            }
        }
    }
}

#[test]
fn print_url_changes_nothing_for_the_other_commands() {
    for (words, route, result) in [
        (&["status"][..], "status", "payment-failed"),
        (&["cap"][..], "cap-get", "cap"),
        (&["set-cap", "5", "week"][..], "cap-set", "cap"),
        (&["delete"][..], "account-delete", "not-finished"),
    ] {
        let plain = World::holding(&sign_in(2)).answering(200, &sample(route, result));
        let printed = World::holding(&sign_in(2)).answering(200, &sample(route, result));
        let one = plain.run(words);
        let other = printed.run(&printing(words));
        assert_eq!((one.code(), one.line()), (other.code(), other.line()));
        assert_eq!(*plain.sent.borrow(), *printed.sent.borrow());
    }
}

#[test]
fn status_reports_each_result_of_the_route() {
    for result in results_of("status") {
        let world = World::holding(&sign_in(3)).answering(200, &sample("status", &result));
        let outcome = world.run(&["status"]);
        assert_eq!(
            (outcome.code(), outcome.line()),
            (0, format!("result={result}"))
        );
    }
}

#[test]
fn cap_and_set_cap_report_the_cap_with_what_is_spent_and_held_and_the_period_end() {
    let body = r#"{"result":"cap","cap":{"amount":5,"period":"month"},"spend":{"spent":1,"held":2,"period_end":1767225600000}}"#;
    for words in [&["cap"][..], &["set-cap", "5", "month"]] {
        let world = World::holding(&sign_in(3)).answering(200, body);
        let outcome = world.run(words);
        assert_eq!(
            (outcome.code(), outcome.line().as_str()),
            (
                0,
                "result=cap amount=5 period=month spent=1 held=2 period_end=1767225600000"
            )
        );
    }
    for (words, route) in [
        (&["cap"][..], "cap-get"),
        (&["set-cap", "5", "week"], "cap-set"),
    ] {
        for result in results_of(route) {
            if result == "cap" {
                continue;
            }
            let world = World::holding(&sign_in(3)).answering(200, &sample(route, &result));
            let outcome = world.run(words);
            assert_eq!(
                (outcome.code(), outcome.line()),
                (0, format!("result={result}"))
            );
        }
    }
}

#[test]
fn a_rejection_and_each_failed_call_are_reported_and_nothing_more_is_sent() {
    for words in ALL {
        let words = printing(words);
        let cases: Vec<(World, u8, &str)> = vec![
            (
                World::holding(&sign_in(1)).answering(400, r#"{"result":"rejected"}"#),
                2,
                "error=rejected",
            ),
            (World::holding(&sign_in(1)).silent(), 1, "error=failed-call"),
            (
                World::holding(&sign_in(1)).answering(500, ""),
                1,
                "error=failed-call",
            ),
            (
                World::holding(&sign_in(1)).answering(200, "{"),
                1,
                "error=failed-call",
            ),
            (
                World::holding(&sign_in(1)).answering(200, r#"{"result":"rejected"}"#),
                1,
                "error=failed-call",
            ),
        ];
        for (world, code, line) in cases {
            let outcome = world.run(&words);
            assert_eq!(
                (outcome.code(), outcome.line().as_str()),
                (code, line),
                "{words:?}"
            );
            assert_eq!(world.log(), ["read", "send"], "{words:?}");
        }
    }
}

#[test]
fn an_integer_written_with_a_fraction_fails_the_call() {
    let body = r#"{"result":"cap","cap":{"amount":5.0,"period":"week"},"spend":{"spent":0,"held":0,"period_end":0}}"#;
    let world = World::holding(&sign_in(3)).answering(200, body);
    let outcome = world.run(&["cap"]);
    assert_eq!(
        (outcome.code(), outcome.line().as_str()),
        (1, "error=failed-call")
    );
    assert!(outcome.note().contains("integer"));
}

fn answers_of(route: &str) -> Vec<(u16, String)> {
    let mut answers: Vec<(u16, String)> = results_of(route)
        .iter()
        .map(|result| (200, sample(route, result)))
        .collect();
    answers.push((400, r#"{"result":"rejected"}"#.to_string()));
    answers.push((404, r#"{"result":"rejected"}"#.to_string()));
    answers.push((500, String::new()));
    answers.push((200, "{".to_string()));
    answers
}

#[test]
fn a_sign_in_that_subscribe_made_stays_whatever_the_backend_answers() {
    let made = SignIn::from_random(&[3; 48]).to_secret().expose().to_vec();
    for (status, body) in answers_of("checkout-start") {
        let world = World::new().answering(status, &body);
        world.run(&["--print-url", "subscribe", "5", "week"]);
        assert_eq!(world.log(), ["read", "write", "send"], "{status} {body}");
        assert_eq!(world.stored(), Some(made.clone()), "{status} {body}");
    }
    let world = World::new().silent();
    world.run(&["--print-url", "subscribe", "5", "week"]);
    assert_eq!(world.stored(), Some(made));
}

#[test]
fn a_page_that_was_not_opened_or_was_refused_removes_nothing() {
    let kept = sign_in(12);
    let refused = [
        r#"{"result":"session","url":"http://evil.example/x"}"#,
        r#"{"result":"page","url":"http://evil.example/x"}"#,
    ];
    for (words, route, result, off_list) in [
        (
            &["subscribe", "5", "week"][..],
            "checkout-start",
            "session",
            refused[0],
        ),
        (&["manage"][..], "manage", "page", refused[1]),
    ] {
        for open_fails in [false, true] {
            let world = World {
                open_fails,
                ..World::holding(&kept)
            }
            .answering(200, &sample(route, result));
            world.run(words);
            assert_eq!(world.log(), ["read", "send", "open"], "{words:?}");
            assert_eq!(world.stored(), Some(kept.to_secret().expose().to_vec()));
        }
        for words in [words.to_vec(), printing(words)] {
            let world = World::holding(&kept).answering(200, off_list);
            let outcome = world.run(&words);
            assert_eq!(outcome.code(), 1, "{words:?}");
            assert_eq!(world.log(), ["read", "send"], "{words:?}");
            assert_eq!(world.stored(), Some(kept.to_secret().expose().to_vec()));
        }
    }
    let world = World::new().answering(200, refused[0]);
    world.run(&["subscribe", "5", "week"]);
    assert_eq!(world.log(), ["read", "write", "send"]);
    assert!(world.stored().is_some());
}

struct FinderA1 {
    stored: RefCell<Option<Vec<u8>>>,
    other: Vec<u8>,
    body: String,
}

impl Keep for FinderA1 {
    fn read(&self) -> Read {
        match self.stored.borrow().as_ref() {
            None => Read::NotStored,
            Some(bytes) => Read::Found(Secret::new(bytes.clone()).expect("a secret")),
        }
    }

    fn write(&self, secret: &Secret) -> Write {
        let mut stored = self.stored.borrow_mut();
        if stored.is_some() {
            return Write::Occupied;
        }
        *stored = Some(secret.expose().to_vec());
        Write::Stored
    }

    fn delete(&self) -> Delete {
        match self.stored.borrow_mut().take() {
            Some(_) => Delete::Deleted,
            None => Delete::NotStored,
        }
    }
}

impl Transport for FinderA1 {
    fn send(&self, _request: &Request) -> Result<Reply, NoAnswer> {
        // While this request is in flight, another run's delete removed the first sign-in
        // and a subscribe of a third run stored a new one.
        *self.stored.borrow_mut() = Some(self.other.clone());
        Ok(Reply {
            status: 200,
            body: self.body.as_bytes().to_vec(),
        })
    }
}

impl Opener for FinderA1 {
    fn open(&self, _page: &PageUrl) -> Result<(), NotOpened> {
        Err(NotOpened)
    }
}

impl Random for FinderA1 {
    fn fill(&self, _bytes: &mut [u8; 48]) -> Result<(), NoRandom> {
        Err(NoRandom)
    }
}

#[test]
fn finder_a1_delete_removes_only_the_sign_in_that_it_sent() {
    let first = sign_in(1).to_secret().expose().to_vec();
    let second = sign_in(2).to_secret().expose().to_vec();
    let world = FinderA1 {
        stored: RefCell::new(Some(first)),
        other: second.clone(),
        body: sample("account-delete", "deleted"),
    };
    let args = vec![OsString::from("delete")];
    let invocation = parse(&args, Build::Debug).expect("delete");
    let outcome = run(
        &invocation,
        &Reach {
            keep: &world,
            transport: &world,
            opener: &world,
            random: &world,
            pages: &Pages::Loopback,
        },
    );
    let kept = world.stored.borrow().clone();
    let second_kept = kept.as_deref() == Some(second.as_slice());
    println!(
        "FINDER_A1 line={} second_kept={second_kept}",
        outcome.line()
    );
    assert!(
        second_kept,
        "the second sign-in, for which the backend answered nothing, was removed"
    );
}
