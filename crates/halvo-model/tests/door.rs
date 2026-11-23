//! The door's sentences, each held by one test. The tests of the door run it in this process
//! against a stand-in host: a listener thread on 127.0.0.1 that the test starts, which records each
//! request and answers what the adapter's reference says that host answers (a JSON object with
//! `content` blocks and `usage`). No test speaks to anything but such a listener. The last test is
//! the rule of the tree on the adapter's one operation and on the runner's files.

use std::io::{Cursor, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

const KEY: &str = "stand-in-key-u75-7f3a9c";

const ANSWER: &str = r#"{"id":"msg_stand_in","type":"message","role":"assistant","model":"stand-in-model","content":[{"type":"thinking","thinking":"","signature":"s"},{"type":"text","text":"done"}],"stop_reason":"end_turn","stop_details":null,"usage":{"input_tokens":12,"output_tokens":3}}"#;

/// One request as the stand-in host read it: its head as text and its body.
struct Recorded {
    head: String,
    body: Vec<u8>,
}

/// A stand-in host on 127.0.0.1 at a port of its own. It counts the connections that it accepts,
/// records each request, and answers each with `status` and `answer`. Its thread stops when the
/// value drops, also when a test panics.
struct StandIn {
    port: u16,
    accepted: Arc<AtomicUsize>,
    recorded: Arc<Mutex<Vec<Recorded>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl StandIn {
    fn start(status: u16, answer: &str) -> StandIn {
        StandIn::start_in_parts(status, answer, 1, Duration::ZERO)
    }

    /// A stand-in host that writes the head of its answer at once and its body in `parts` parts,
    /// `gap` apart.
    fn start_in_parts(status: u16, answer: &str, parts: usize, gap: Duration) -> StandIn {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a listener on the loopback");
        let port = listener.local_addr().expect("its address").port();
        let accepted = Arc::new(AtomicUsize::new(0));
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let answer = answer.to_owned();
        let thread = {
            let (accepted, recorded, stop) = (accepted.clone(), recorded.clone(), stop.clone());
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    let Ok(mut stream) = stream else { continue };
                    accepted.fetch_add(1, Ordering::SeqCst);
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(20)));
                    if let Some(request) = read_request(&mut stream) {
                        recorded.lock().expect("the record").push(request);
                        let reason = if status == 200 { "OK" } else { "Refused" };
                        let _ = write!(
                            stream,
                            "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                            answer.len()
                        );
                        let _ = stream.flush();
                        let size = answer.len().div_ceil(parts);
                        for (at, part) in answer.as_bytes().chunks(size).enumerate() {
                            if at > 0 {
                                std::thread::sleep(gap);
                            }
                            let _ = stream.write_all(part);
                            let _ = stream.flush();
                        }
                    }
                }
            })
        };
        StandIn {
            port,
            accepted,
            recorded,
            stop,
            thread: Some(thread),
        }
    }

    fn accepted(&self) -> usize {
        self.accepted.load(Ordering::SeqCst)
    }
}

impl Drop for StandIn {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn read_request(stream: &mut TcpStream) -> Option<Recorded> {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 4096];
    let end = loop {
        if let Some(at) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            break at;
        }
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        bytes.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8(bytes[..end].to_vec()).ok()?;
    let length: usize = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().ok())?
        })
        .unwrap_or(0);
    let mut body = bytes[end + 4..].to_vec();
    while body.len() < length {
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    Some(Recorded { head, body })
}

/// Runs the door in this process with `args` and `input`, and gives its exit status, its standard
/// output and its standard error.
fn door(args: &[String], input: &str) -> (i32, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = halvo_model::run(args, Cursor::new(input.as_bytes()), &mut out, &mut err);
    (
        code,
        String::from_utf8(out).expect("UTF-8 output"),
        String::from_utf8(err).expect("UTF-8 errors"),
    )
}

fn args(tier: &str, ceiling: &str, consent: &str, port: Option<u16>) -> Vec<String> {
    let mut args: Vec<String> = ["--tier", tier, "--ceiling", ceiling, "--consent", consent]
        .iter()
        .map(|arg| (*arg).to_owned())
        .collect();
    if let Some(port) = port {
        args.push("--loopback-port".to_owned());
        args.push(port.to_string());
    }
    args
}

/// The input of the door: the key in the form that its starter writes (the count of its bytes and
/// one LF, the key, one LF), then `rest`.
fn keyed(key: &str, rest: &str) -> String {
    format!("{}\n{key}\n{rest}", key.len())
}

fn request(id: u64) -> String {
    format!(
        r#"{{"id":{id},"system":"Be brief.","prompt":"Pay the bill.","snapshot":"e1 button \"Pay\"\ne2 link","grammar":"action","level":"high"}}"#
    )
}

#[test]
fn with_no_grant_the_door_opens_no_connection() {
    let host = StandIn::start(200, ANSWER);
    let port = Some(host.port);
    let mut got = Vec::new();
    for (args, cause) in [
        (args("frontier", "frontier", "recorded", None), "ruling"),
        (args("frontier", "frontier", "missing", port), "consent"),
        (
            args("frontier", "low_cost_hosted", "recorded", port),
            "ceiling",
        ),
        (args("on_device", "frontier", "recorded", port), "local"),
    ] {
        let (code, out, err) = door(&args, &keyed(KEY, &format!("{}\n", request(1))));
        got.push((code, out, err));
        assert_eq!(
            got.last().expect("a run"),
            &(
                0,
                format!("{{\"id\":1,\"refused\":\"{cause}\"}}\n"),
                String::new()
            ),
            "{args:?}"
        );
    }
    assert_eq!(
        host.accepted(),
        0,
        "connections that the stand-in host accepted"
    );
}

#[test]
fn with_a_grant_one_request_reaches_the_granted_origin_alone() {
    let granted = StandIn::start(200, ANSWER);
    let other = StandIn::start(200, ANSWER);
    let input = keyed(KEY, &format!("{}\n", request(7)));
    let refused = door(
        &args("frontier", "frontier", "missing", Some(granted.port)),
        &input,
    );
    let refused_seen = (refused, granted.accepted(), other.accepted());
    let (code, out, err) = door(
        &args("frontier", "frontier", "recorded", Some(granted.port)),
        &input,
    );
    let reply: serde_json::Value = serde_json::from_str(out.trim_end()).expect("one JSON line");
    let keys: Vec<&String> = reply.as_object().expect("an object").keys().collect();
    assert_eq!(
        (
            refused_seen,
            code,
            err,
            out.lines().count(),
            reply["id"].as_u64(),
            reply["text"].as_str(),
            reply["tokens_in"].as_u64(),
            reply["tokens_out"].as_u64(),
            keys.len(),
            granted.accepted(),
            granted.recorded.lock().expect("the record").len(),
            other.accepted(),
        ),
        (
            (
                (
                    0,
                    "{\"id\":7,\"refused\":\"consent\"}\n".to_owned(),
                    String::new()
                ),
                0,
                0
            ),
            0,
            String::new(),
            1,
            Some(7),
            Some("done"),
            Some(12),
            Some(3),
            6,
            1,
            1,
            0,
        )
    );
}

#[test]
fn the_request_that_leaves_holds_the_closed_fields_alone() {
    let host = StandIn::start(200, ANSWER);
    let (code, out, err) = door(
        &args("low_cost_hosted", "frontier", "recorded", Some(host.port)),
        &keyed(KEY, &format!("{}\n", request(3))),
    );
    assert_eq!((code, err.as_str()), (0, ""), "{out}");
    let recorded = host.recorded.lock().expect("the record");
    assert_eq!(recorded.len(), 1);
    let body = r#"{"model":"stand-in-model","max_tokens":2048,"system":"Be brief.","messages":[{"role":"user","content":"Pay the bill.\n<<<DATA\ne1 button \"Pay\"\ne2 link\nDATA>>>"}]}"#;
    let mut lines = recorded[0].head.split("\r\n");
    let request_line = lines.next().expect("a request line");
    let mut headers: Vec<(String, String)> = lines
        .map(|line| {
            let (name, value) = line.split_once(':').expect("a header line");
            (name.to_ascii_lowercase(), value.trim().to_owned())
        })
        .collect();
    headers.sort();
    let mut want: Vec<(String, String)> = [
        ("accept", "application/json".to_owned()),
        ("anthropic-version", "2026-10-21".to_owned()),
        ("content-length", body.len().to_string()),
        ("content-type", "application/json".to_owned()),
        ("host", format!("127.0.0.1:{}", host.port)),
        ("x-api-key", KEY.to_owned()),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_owned(), value))
    .collect();
    want.sort();
    let mut closed: Vec<&str> = halvo_connect::model::HEADERS.to_vec();
    closed.sort_unstable();
    assert_eq!(
        (
            request_line,
            headers,
            String::from_utf8_lossy(&recorded[0].body).into_owned()
        ),
        ("POST /v1/messages HTTP/1.1", want.clone(), body.to_owned())
    );
    assert_eq!(
        want.iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        closed
    );
}

#[test]
fn the_key_stands_in_no_line_that_the_door_writes() {
    let ok = StandIn::start(200, ANSWER);
    let unauthorized = StandIn::start(
        401,
        r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#,
    );
    let unreadable = StandIn::start(200, "not the form of the adapter");
    let lines = keyed(
        KEY,
        &format!(
            "{}\nnot a request {KEY}\n{{\"id\":2}}\n{}\n",
            request(1),
            request(4)
        ),
    );
    let mut written = String::new();
    let mut seen = Vec::new();
    for (args, input) in [
        (
            args("frontier", "frontier", "recorded", Some(ok.port)),
            lines.clone(),
        ),
        (
            args("frontier", "frontier", "recorded", Some(unauthorized.port)),
            lines.clone(),
        ),
        (
            args("frontier", "frontier", "recorded", Some(unreadable.port)),
            lines.clone(),
        ),
        (
            args("frontier", "frontier", "recorded", None),
            lines.clone(),
        ),
        (
            args("frontier", "frontier", KEY, Some(ok.port)),
            lines.clone(),
        ),
        (
            args("frontier", "frontier", "recorded", Some(ok.port)),
            keyed(&format!("{KEY} {KEY}"), &format!("{}\n", request(1))),
        ),
    ] {
        let (code, out, err) = door(&args, &input);
        seen.push(code);
        written.push_str(&out);
        written.push_str(&err);
    }
    let debug = format!(
        "{:?}",
        halvo_model::Key::read(&mut Cursor::new(keyed(KEY, "").into_bytes()))
    );
    assert_eq!(seen, [0, 0, 0, 0, 2, 2], "{written}");
    for refusal in ["status", "answer", "ruling", "request"] {
        assert!(
            written.contains(&format!("\"refused\":\"{refusal}\"")),
            "{refusal}: {written}"
        );
    }
    assert_eq!(
        (written.contains(KEY), debug.contains(KEY)),
        (false, false),
        "{written}\n{debug}"
    );
    assert_eq!(debug, format!("Some(Key({} bytes))", KEY.len()));
}

#[test]
fn no_line_that_the_door_writes_holds_a_byte_of_a_key_or_of_a_refused_request() {
    let host = StandIn::start(200, ANSWER);
    let halves = ["first-half-a91f", "second-half-b72e"];
    let with_line_feed = halves.join("\n");
    let line = "zq-91xw-lpq7-vv03 ~~ {nope";
    let (system, prompt, snapshot) = ("sys-q8z1x-kk", "prompt-w7v2k-jj", "snap-j4m9t-hh");
    let refused = format!(
        r#"{{"id":5,"system":"{system}","prompt":"{prompt}","snapshot":"{snapshot}","grammar":"text","level":"low"}}"#
    );
    let large = "pz".repeat(70_000);
    let too_large = format!(
        r#"{{"id":6,"system":"s","prompt":"{large}","snapshot":null,"grammar":"text","level":"low"}}"#
    );
    let requests = format!("{line}\n{refused}\n{too_large}\n");
    let mut written = String::new();
    let mut seen = Vec::new();
    for (consent, input) in [
        ("recorded", keyed(&with_line_feed, &requests)),
        ("recorded", keyed("cr-key-d81\r", &requests)),
        ("recorded", keyed("nul-key\0-e72", &requests)),
        ("recorded", format!("0\n\n{requests}")),
        ("recorded", format!("40\nshort-key-f63\n{requests}")),
        ("recorded", format!("4\nlong-key-g54\n{requests}")),
        ("missing", keyed(KEY, &requests)),
        ("recorded", keyed(KEY, &format!("{line}\n{too_large}\n"))),
    ] {
        let (code, out, err) = door(
            &args("frontier", "frontier", consent, Some(host.port)),
            &input,
        );
        seen.push(code);
        written.push_str(&out);
        written.push_str(&err);
    }
    let given = [
        halves[0],
        halves[1],
        "cr-key-d81",
        "nul-key",
        "short-key-f63",
        "long-key-g54",
        KEY,
        line,
        system,
        prompt,
        snapshot,
        &large[..6],
    ];
    let found: Vec<&str> = given
        .iter()
        .flat_map(|text| {
            (0..=text.len().saturating_sub(6)).map(|at| &text[at..(at + 6).min(text.len())])
        })
        .filter(|part| written.contains(part))
        .collect();
    assert_eq!(
        (seen, found, host.accepted()),
        (vec![2, 2, 2, 2, 2, 2, 0, 0], Vec::<&str>::new(), 0),
        "{written}"
    );
    assert_eq!(
        written,
        format!(
            "{}{}",
            "REFUSED key\n".repeat(6),
            "{\"id\":0,\"refused\":\"request\"}\n{\"id\":5,\"refused\":\"consent\"}\n{\"id\":6,\"refused\":\"consent\"}\n{\"id\":0,\"refused\":\"request\"}\n{\"id\":6,\"refused\":\"size\"}\n"
        )
    );
}

#[test]
fn an_answer_that_ends_past_the_time_limit_is_no_answer() {
    assert_eq!(
        halvo_connect::model::TIME_LIMIT,
        Duration::from_secs(90),
        "the one time limit that the program halvo-model passes"
    );
    let limit = Duration::from_millis(1_000);
    let host = StandIn::start_in_parts(200, ANSWER, 5, Duration::from_millis(400));
    let started = std::time::Instant::now();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = halvo_model::run_with_limit(
        &args("frontier", "frontier", "recorded", Some(host.port)),
        limit,
        Cursor::new(keyed(KEY, &format!("{}\n", request(9))).into_bytes()),
        &mut out,
        &mut err,
    );
    let answered_ms = started.elapsed().as_millis();
    std::thread::sleep(Duration::from_millis(1_500));
    assert_eq!(
        (
            code,
            String::from_utf8_lossy(&out).into_owned(),
            String::from_utf8_lossy(&err).into_owned(),
            answered_ms < 1_500,
            host.accepted(),
            host.recorded.lock().expect("the record").len(),
        ),
        (
            0,
            "{\"id\":9,\"refused\":\"no_answer\"}\n".to_owned(),
            String::new(),
            true,
            1,
            1,
        ),
        "the door answered after {answered_ms} ms"
    );
}

#[test]
fn every_row_of_the_release_table_names_an_adapter_of_the_door() {
    let words = halvo_model::adapter_words();
    let ruling = halvo_route::release::ruling();
    let table = include_str!("../../halvo-route/data/hosts.tsv");
    let mut named: Vec<String> = ruling
        .rows()
        .iter()
        .map(|row| row.adapter().to_owned())
        .collect();
    named.extend(
        table
            .lines()
            .skip(1)
            .map(|line| line.split('\t').nth(3).unwrap_or("").to_owned()),
    );
    println!(
        "rows of the release ruling: {}, rows of the table's text: {}, adapter words of the door: {words:?}",
        ruling.rows().len(),
        table.lines().skip(1).count()
    );
    let without: Vec<&String> = named
        .iter()
        .filter(|word| !words.contains(&word.as_str()))
        .collect();
    assert_eq!(without, Vec::<&String>::new());
    assert_eq!(words, ["anthropic-messages-2026-10-21"]);
}

/// The name of the adapter's one operation, written in parts so that this text is no use of it.
const OPERATION: &str = concat!("send", "_under", "_grant");

/// The Rust files that may name the adapter's one operation, each with the number of times that
/// the name stands in it as a word, in code, comments and strings alike, and why: the operation's
/// own file and the door's files alone (the coordinator's log line 16959).
const OPERATION_USES: [(&str, usize, &str); 2] = [
    (
        "crates/halvo-connect/src/model.rs",
        3,
        "the operation's own file: its definition, the module's comment and the comment of SendError",
    ),
    (
        "crates/halvo-model/src/lib.rs",
        2,
        "the door: its import and its one call",
    ),
];

/// The names that no file under crates/halvo-runner holds, as a word of ASCII letters, digits,
/// underscores and hyphens: the adapter's one operation, the router's grant type and the router's
/// crate by its path name and its package name.
const RUNNER_NAMES: [&str; 4] = [
    OPERATION,
    concat!("Leave", "Grant"),
    concat!("halvo", "_route"),
    concat!("halvo", "-route"),
];

/// Every file under the directory `dir` of the tree at `base` that `keep` takes by its name, by its
/// path from `base`, in order: the walk of `rust_files` of crates/halvo-route/tests/
/// no_grant_in_release.rs, copied, with the name test as an argument. It enters no directory whose
/// name begins with a dot, none named `node_modules` or `target`, and none that holds a file
/// `CACHEDIR.TAG`.
fn files(
    base: &std::path::Path,
    dir: &str,
    keep: &dyn Fn(&str) -> bool,
    out: &mut Vec<String>,
    faults: &mut Vec<String>,
) {
    let entries = match std::fs::read_dir(base.join(dir)) {
        Ok(entries) => entries,
        Err(e) => {
            faults.push(format!("the directory '{dir}' cannot be listed: {e}"));
            return;
        }
    };
    let mut found = Vec::new();
    for entry in entries {
        let read = entry.and_then(|entry| {
            let kind = entry.file_type()?;
            Ok((entry.file_name().to_string_lossy().into_owned(), kind))
        });
        match read {
            Ok(one) => found.push(one),
            Err(e) => faults.push(format!("an entry of '{dir}' cannot be read: {e}")),
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, kind) in found {
        let path = if dir.is_empty() {
            name.clone()
        } else {
            format!("{dir}/{name}")
        };
        if kind.is_dir() {
            let skipped = name.starts_with('.')
                || name == "node_modules"
                || name == "target"
                || base.join(&path).join("CACHEDIR.TAG").exists();
            if !skipped {
                files(base, &path, keep, out, faults);
            }
        } else if keep(&name) {
            out.push(path);
        }
    }
}

/// How many times `name` stands in `text` as a word whose signs are of `sign`.
fn words(text: &str, name: &str, sign: fn(char) -> bool) -> usize {
    text.split(|c: char| !sign(c))
        .filter(|word| *word == name)
        .count()
}

fn rust_sign(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn name_sign(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

/// The refusals of the rule over the tree at `base`: each Rust file whose count of the operation's
/// name differs from its entry of `listed` (a file not listed is allowed none, a listed file that is
/// gone holds none), each file under crates/halvo-runner that names one of `RUNNER_NAMES`, and
/// each fault of the walk.
fn operation_refusals(base: &std::path::Path, listed: &[(&str, usize, &str)]) -> Vec<String> {
    let (mut rust, mut faults) = (Vec::new(), Vec::new());
    let is_rust = |name: &str| name.to_ascii_lowercase().ends_with(".rs");
    files(base, "", &is_rust, &mut rust, &mut faults);
    let mut counts: Vec<(String, usize)> = Vec::new();
    for file in &rust {
        match std::fs::read_to_string(base.join(file)) {
            Ok(text) => match words(&text, OPERATION, rust_sign) {
                0 => {}
                n => counts.push((file.clone(), n)),
            },
            Err(e) => faults.push(format!("{file} cannot be read as UTF-8 text: {e}")),
        }
    }
    let mut refusals = faults;
    let mut named: Vec<&str> = counts
        .iter()
        .map(|(file, _)| file.as_str())
        .chain(listed.iter().map(|(file, _, _)| *file))
        .collect();
    named.sort_unstable();
    named.dedup();
    for file in named {
        let found = counts
            .iter()
            .find(|(f, _)| f == file)
            .map_or(0, |(_, n)| *n);
        let (allowed, why) = listed
            .iter()
            .find(|(f, _, _)| *f == file)
            .map_or((0, "the file is not on the list"), |(_, n, why)| (*n, *why));
        if found != allowed {
            refusals.push(format!(
                "{file}: uses of the name of the adapter's one operation ({OPERATION}) as a word: {found}, where OPERATION_USES allows {allowed} ({why}); only the door sends a model request, with a grant that it holds"
            ));
        }
    }
    let (mut runner, mut faults) = (Vec::new(), Vec::new());
    if base.join("crates/halvo-runner").is_dir() {
        files(
            base,
            "crates/halvo-runner",
            &|_| true,
            &mut runner,
            &mut faults,
        );
    }
    refusals.extend(faults);
    for file in runner {
        match std::fs::read(base.join(&file)) {
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes);
                for name in RUNNER_NAMES {
                    let n = words(&text, name, name_sign);
                    if n > 0 {
                        refusals.push(format!(
                            "{file}: names {name} {n} times; the runner's program holds no way to a grant of the router and no call of the adapter's one operation"
                        ));
                    }
                }
            }
            Err(e) => refusals.push(format!("{file} cannot be read: {e}")),
        }
    }
    refusals
}

#[test]
fn the_adapter_operation_stands_in_the_door_alone_and_the_runner_names_no_grant() {
    let tree = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "operation-rule-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));
    let plant = |path: &str, text: &str| {
        let file = tree.join(path);
        std::fs::create_dir_all(file.parent().expect("a parent")).expect("a planted directory");
        std::fs::write(file, text).expect("a planted file");
    };
    let call = format!("fn f() {{ {OPERATION}(g, k, b); }}\n");
    plant("crates/halvo-connect/src/model.rs", &call);
    plant(
        "crates/other/src/lib.rs",
        &format!("// {OPERATION}\n{call}"),
    );
    plant("target/debug/held.rs", &call);
    plant(
        "crates/halvo-runner/src/planted.rs",
        &format!("use {}::leave::{};\n", RUNNER_NAMES[2], RUNNER_NAMES[1]),
    );
    plant(
        "crates/halvo-runner/Cargo.toml",
        &format!(
            "[dependencies]\nrouter = {{ package = \"{}\" }}\n",
            RUNNER_NAMES[3]
        ),
    );
    plant(
        "crates/halvo-runner/src/near.rs",
        "use halvo_route_wire::Body; // halvo-route-wire, a granted\n",
    );
    let planted = operation_refusals(
        &tree,
        &[("crates/halvo-connect/src/model.rs", 1, "the planted file")],
    );
    let files: Vec<&str> = planted
        .iter()
        .map(|refusal| refusal.split(':').next().expect("a file"))
        .collect();
    assert_eq!(
        files,
        [
            "crates/other/src/lib.rs",
            "crates/halvo-runner/Cargo.toml",
            "crates/halvo-runner/src/planted.rs",
            "crates/halvo-runner/src/planted.rs",
        ],
        "{planted:#?}"
    );

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..");
    let refusals = operation_refusals(&root, &OPERATION_USES);
    println!(
        "files that may name the adapter's one operation: {}, names that no file of the runner holds: {}",
        OPERATION_USES.len(),
        RUNNER_NAMES.len()
    );
    assert_eq!(refusals, Vec::<String>::new());
}
