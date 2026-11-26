//! THE ONE TEST OF THE TREE THAT STARTS THE BUILT DOOR FROM THE RUNNER'S OWN START (r6 U5.4). A whole run of a runner
//! in this process, with the stand-in supervisor of crate::model::tests, asks two requests through `ask`. The door's
//! place is a script of this test, which starts the built program halvo-model of this build directory with
//! `--loopback-port <port>` before the arguments that the runner's start wrote, and hands the door's standard input
//! through `tee` into a record. The port is that of a stand-in host that this test binds on 127.0.0.1; a door of the
//! stand-in form makes its ruling from that port alone, so its one origin is that host, and a door built without
//! that form refuses the argument and sends nothing. The test is ignored in a plain run, whose build does not make
//! the door: scripts/r3/ci.d/64-model-egress.sh runs it by its exact name with `--ignored`, in the macos job, under
//! the egress check, behind its call that builds the door. Every wait of the test has a bound (`RUN_BOUND`,
//! `HOST_READ`).

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use halvo_schema::executable::body::ModelUsage;

use super::{asked, count, recorded, request, run_at, scratch, Answers};
use crate::model::ask::{Grammar, Level, Place, OUTPUT_LIMIT};

/// The made-up key of this test: visible ASCII, held by no file but this one.
const KEY: &str = "made-up-key-r6u54-Jx7Qm2Vt-9b4e";

/// The most time of the whole run, from its start to its end. The runner reads the door's line with no time limit of
/// its own, so a door that never answers ends the test here.
const RUN_BOUND: Duration = Duration::from_secs(60);

/// The most time that the stand-in host waits for the bytes of one request.
const HOST_READ: Duration = Duration::from_secs(20);

/// The answer of the stand-in host to the first request, in the form of the adapter's host: 12 tokens in and 3 out,
/// two numbers that stand in this answer alone.
const ANSWER: &str = r#"{"id":"msg_u54","type":"message","role":"assistant","model":"stand-in-model","content":[{"type":"thinking","thinking":"","signature":"s"},{"type":"text","text":"done"}],"stop_reason":"end_turn","stop_details":null,"usage":{"input_tokens":12,"output_tokens":3}}"#;

/// The request lines that the runner writes for the two requests of the test (A12.2).
const LINES: [&str; 2] = [
    r#"{"id":1,"system":"Be brief.","prompt":"Pay the bill.","snapshot":null,"grammar":"action","level":"medium"}"#,
    r#"{"id":2,"system":"Be brief.","prompt":"Write the reply.","snapshot":null,"grammar":"text","level":"low"}"#,
];

/// One request as the stand-in host read it: its head as text and its body.
struct Got {
    head: String,
    body: Vec<u8>,
}

/// Reads one request whole, its head and as many bytes of body as its content-length names, each read within
/// `HOST_READ`; none where the stream ends or a read passes that time.
fn read_request(stream: &mut TcpStream) -> Option<Got> {
    stream.set_nonblocking(false).ok()?;
    stream.set_read_timeout(Some(HOST_READ)).ok()?;
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 4096];
    let end = loop {
        if let Some(at) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            break at;
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return None,
            Ok(n) => bytes.extend_from_slice(&chunk[..n]),
        }
    };
    let head = String::from_utf8_lossy(&bytes[..end]).into_owned();
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
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return None,
            Ok(n) => body.extend_from_slice(&chunk[..n]),
        }
    }
    Some(Got { head, body })
}

/// The stand-in host of the test, on 127.0.0.1 at a port of its own. It records each request and answers the first
/// with status 200 and `ANSWER`, each later one with status 500. Its thread ends when the value drops or when
/// `RUN_BOUND` has passed, whichever comes first.
struct Host {
    port: u16,
    got: Arc<Mutex<Vec<Got>>>,
    stop: Arc<AtomicBool>,
}

impl Drop for Host {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

impl Host {
    fn start() -> Host {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a listener on the loopback");
        listener
            .set_nonblocking(true)
            .expect("a listener that does not wait");
        let port = listener.local_addr().expect("its address").port();
        let got = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let until = Instant::now() + RUN_BOUND;
        let (kept, stopped) = (got.clone(), stop.clone());
        std::thread::spawn(move || {
            while !stopped.load(Ordering::SeqCst) && Instant::now() < until {
                let Ok((mut stream, _)) = listener.accept() else {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                };
                let Some(one) = read_request(&mut stream) else {
                    continue;
                };
                let first = {
                    let mut all = kept.lock().expect("the record");
                    all.push(one);
                    all.len() == 1
                };
                let (status, body) = if first {
                    ("200 OK", ANSWER)
                } else {
                    ("500 Internal Server Error", "{}")
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.flush();
            }
        });
        Host { port, got, stop }
    }
}

/// `path` as one word of sh, in single quotes.
fn word(path: &Path) -> String {
    let text = path.to_str().expect("a path of UTF-8");
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// The script that stands at the door's place. It records its arguments and its environment into `record`, starts the
/// built door `door` with the port of the stand-in host before the arguments that it got, its own standard input
/// handed through `tee` into the record `to-door.bin`, and behind the door's end records the door's exit status and
/// the mark `ended`, which the stand-in supervisor reads at the run's `finish`.
fn script(record: &Path, door: &Path, port: u16) -> String {
    format!(
        "#!/bin/sh\nprintf '%s\\n' \"$@\" >{args}\n/usr/bin/env >{env}\n/usr/bin/tee -a {input} | {door} --loopback-port {port} \"$@\"\nprintf '%s' \"$?\" >{exit}\n: >{ended}\n",
        args = word(&record.join("args.txt")),
        env = word(&record.join("env.txt")),
        input = word(&record.join("to-door.bin")),
        door = word(door),
        exit = word(&record.join("door-exit.txt")),
        ended = word(&record.join("ended")),
    )
}

/// A compile that the person allowed to use a hosted model, whose drive asks two requests, starts the built door
/// through the runner's own start, and the two programs read each other. (1) The door's standard input holds the
/// key's head and then the runner's two request lines, by equal bytes. (2) The door sends one request for each line
/// to the stand-in host: `POST /v1/messages`, the key in the one header `x-api-key`, the stand-in ruling's model
/// word, the runner's output limit, the line's level as the effort, its system text and its prompt. (3) The runner
/// reads the door's lines: the first hold settles at the tokens that the host reported, and the second, whose
/// request the host answers with status 500, at nothing (the door's refusal `status`). (4) Each request has a hold of
/// its line's bytes before it and a settle behind it, and the key is asked at the door's start alone. (5) The key
/// stands in the door's standard input and in the head of each request at the host, and in no argument of the door,
/// no variable of its environment, no body at the host, no frame that the runner wrote and no line of its standard
/// error. (6) The door ends by the end of its input, with the exit status 0, before the run's `finish`.
#[test]
#[ignore = "starts the built door halvo-model, which the build of a plain run does not make: scripts/r3/ci.d/64-model-egress.sh runs it by its exact name with --ignored, in the macos job, under the egress check, behind its call that builds the door"]
fn a_run_asks_the_built_door_through_the_runners_own_start() {
    let exe = std::env::current_exe().expect("the test binary");
    let door = exe
        .ancestors()
        .nth(2)
        .expect("the directory above deps")
        .join("halvo-model");
    assert!(
        door.is_file(),
        "no built door at {}: `cargo test --locked -p halvo-model --no-run` builds it, as the first call of scripts/r3/ci.d/64-model-egress.sh does",
        door.display()
    );
    let dir = scratch("real-door");
    let record = halvo_test_dir::make(&dir, "record").expect("the record directory");
    let data = halvo_test_dir::make(&dir, "data").expect("the data directory");
    let host = Host::start();
    let place = record.join("halvo-model");
    std::fs::write(&place, script(&record, &door, host.port)).expect("the script");
    std::fs::set_permissions(&place, std::fs::Permissions::from_mode(0o755)).expect("its mode");
    let drive = vec![
        request("Pay the bill."),
        asked("Write the reply.", Grammar::Text, Level::Low),
    ];
    let (done, wait) = std::sync::mpsc::channel();
    let (at, kept, run_data) = (
        Place {
            program: place,
            before: Vec::new(),
        },
        record.clone(),
        data.clone(),
    );
    std::thread::spawn(move || {
        let answers = Answers {
            key: Some(KEY),
            hold: None,
        };
        let _ = done.send(run_at(at, true, answers, drive, &kept, &run_data));
    });
    let seen = match wait.recv_timeout(RUN_BOUND) {
        Ok(seen) => seen,
        Err(RecvTimeoutError::Timeout) => panic!(
            "the run did not end within {} s: the door never answered, or the run never ended",
            RUN_BOUND.as_secs()
        ),
        Err(RecvTimeoutError::Disconnected) => panic!("the run's thread ended with no result"),
    };
    let got = host.got.lock().expect("the record");

    // (1) What the runner wrote to the door.
    let to_door =
        std::fs::read(record.join("to-door.bin")).expect("the record of the door's input");
    assert_eq!(
        String::from_utf8_lossy(&to_door),
        format!("{}\n{KEY}\n{}\n{}\n", KEY.len(), LINES[0], LINES[1]),
        "the door's standard input: the key's head, then one line a request"
    );

    // (2) What the door sent.
    assert_eq!(
        got.len(),
        2,
        "one request at the stand-in host for each request line"
    );
    let header = format!("x-api-key: {KEY}");
    for (one, (prompt, effort)) in got
        .iter()
        .zip([("Pay the bill.", "medium"), ("Write the reply.", "low")])
    {
        let mut lines = one.head.lines();
        assert_eq!(lines.next(), Some("POST /v1/messages HTTP/1.1"), "{prompt}");
        let with_key: Vec<&str> = lines.filter(|line| line.contains(KEY)).collect();
        assert_eq!(
            with_key,
            [header.as_str()],
            "{prompt}: the key's one header"
        );
        assert_eq!(
            count(&one.body, KEY.as_bytes()),
            0,
            "{prompt}: no key in a body"
        );
        let body: serde_json::Value = serde_json::from_slice(&one.body).expect("a body of JSON");
        assert_eq!(
            (
                body["model"].as_str(),
                body["max_tokens"].as_u64(),
                body["output_config"]["effort"].as_str(),
                body["system"].as_str(),
                body["messages"].as_array().map(Vec::len),
                body["messages"][0]["role"].as_str(),
                body["messages"][0]["content"].as_str(),
            ),
            (
                Some("stand-in-model"),
                Some(OUTPUT_LIMIT),
                Some(effort),
                Some("Be brief."),
                Some(1),
                Some("user"),
                Some(prompt),
            ),
            "{prompt}: the request at the stand-in host"
        );
    }

    // (3) and (4) The holds and the settles around the door's lines.
    assert_eq!(
        seen.model_frames(),
        ["hold", "secret", "settle", "hold", "settle"],
        "a hold before each request, the key asked at the door's start alone, a settle behind each"
    );
    assert_eq!(
        seen.holds(),
        [LINES[0].len() as u64, LINES[1].len() as u64],
        "each hold names the bytes of its request line"
    );
    assert_eq!(
        seen.settles(),
        [
            Some(ModelUsage {
                tokens_in: 12,
                tokens_out: 3
            }),
            None
        ],
        "the host's tokens for the reply, nothing for the refusal status"
    );

    // (5) The key.
    assert_eq!(
        count(&to_door, KEY.as_bytes()),
        1,
        "the key once in the door's input"
    );
    assert_eq!(
        recorded(&record, "args.txt"),
        "--tier\nfrontier\n--ceiling\nfrontier\n--consent\nrecorded\n",
        "the door's arguments as the runner's start wrote them"
    );
    let env =
        std::fs::read_to_string(record.join("env.txt")).expect("the record of the environment");
    assert!(!env.contains(KEY), "no variable of the door holds the key");
    let in_frames: usize = seen
        .from_runner
        .iter()
        .map(|frame| count(frame, KEY.as_bytes()))
        .sum();
    assert_eq!(in_frames, 0, "no frame that the runner wrote holds the key");
    assert!(
        seen.lines.iter().all(|line| !line.contains(KEY)),
        "no line of standard error holds the key"
    );

    // (6) The end.
    assert_eq!(
        seen.ended_at_finish,
        Some(true),
        "the door ended before the run's finish"
    );
    assert_eq!(
        recorded(&record, "door-exit.txt"),
        "0",
        "the door ended by the end of its input"
    );
    assert!(
        seen.lines
            .iter()
            .any(|l| l == "run door=ended by=input_end"),
        "{:?}",
        seen.lines
    );
    assert_eq!(seen.status, 0, "the finish was stored and done written");
}
