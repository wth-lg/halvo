//! The door's case under the egress check: the built program of this test build (with the stand-in
//! form), started by sh scripts/egress/check.sh with the list scripts/egress/lists/loopback.txt,
//! whose one entry, 127.0.0.1:47201, is the stand-in host that this test starts. It runs outside
//! the merge gate, in one daily slice, and speaks to nothing but that listener.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

const PORT: u16 = 47201;

const ANSWER: &str = r#"{"id":"msg_stand_in","type":"message","role":"assistant","model":"stand-in-model","content":[{"type":"text","text":"done"}],"stop_reason":"end_turn","usage":{"input_tokens":12,"output_tokens":3}}"#;

/// Stops the stand-in host's thread when the test ends, also when it panics.
struct Stop(Arc<AtomicBool>);

impl Drop for Stop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", PORT));
    }
}

fn answer_one(stream: &mut TcpStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(20)));
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 4096];
    let end = loop {
        if let Some(at) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            break at;
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => bytes.extend_from_slice(&chunk[..n]),
        }
    };
    let head = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
    let length: usize = head
        .lines()
        .find_map(|line| line.strip_prefix("content-length:"))
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(0);
    let mut body = bytes.len() - end - 4;
    while body < length {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => body += n,
        }
    }
    let _ = write!(
        stream,
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{ANSWER}",
        ANSWER.len()
    );
}

#[test]
#[ignore = "the case of the door under the egress check: run by its check in one daily slice, with --ignored --exact"]
fn the_built_door_under_the_egress_check_reaches_the_granted_loopback_origin_alone() {
    let listener = TcpListener::bind(("127.0.0.1", PORT)).expect("the list's port on the loopback");
    let accepted = Arc::new(AtomicUsize::new(0));
    let stop = Stop(Arc::new(AtomicBool::new(false)));
    let host = {
        let (accepted, stop) = (accepted.clone(), stop.0.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(mut stream) = stream else { continue };
                accepted.fetch_add(1, Ordering::SeqCst);
                answer_one(&mut stream);
            }
        })
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..");
    let started = std::time::Instant::now();
    let mut child = Command::new("sh")
        .args([
            "scripts/egress/check.sh",
            "scripts/egress/lists/loopback.txt",
            "--",
            env!("CARGO_BIN_EXE_halvo-model"),
            "--tier",
            "frontier",
            "--ceiling",
            "frontier",
            "--consent",
            "recorded",
            "--loopback-port",
            &PORT.to_string(),
        ])
        .current_dir(&root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the egress check starts");
    let key = "stand-in-key-u75-case5";
    let input = format!(
        "{}\n{key}\n{}\n",
        key.len(),
        r#"{"id":1,"system":"Be brief.","prompt":"Pay the bill.","snapshot":null,"grammar":"text","level":"low"}"#
    );
    child
        .stdin
        .take()
        .expect("its input")
        .write_all(input.as_bytes())
        .expect("the key and one request");
    let output = child.wait_with_output().expect("the egress check ends");
    let out = String::from_utf8_lossy(&output.stdout).into_owned();
    let err = String::from_utf8_lossy(&output.stderr).into_owned();
    println!(
        "the egress check's run of the built door took {} ms",
        started.elapsed().as_millis()
    );
    let egress: Vec<&str> = err
        .lines()
        .filter(|line| line.starts_with("egress mode="))
        .collect();
    let reply: serde_json::Value =
        serde_json::from_str(out.lines().next().unwrap_or("")).unwrap_or_default();
    println!("{}", egress.join("\n"));
    let served = accepted.load(Ordering::SeqCst);
    drop(stop);
    let _ = host.join();
    assert_eq!(
        (
            output.status.code(),
            out.lines().count(),
            reply["id"].as_u64(),
            reply["text"].as_str(),
            egress.len(),
            egress
                .first()
                .is_some_and(|line| line.contains(" command=0 denied=0 ")),
            served,
            out.contains(key) || err.contains(key),
        ),
        (Some(0), 1, Some(1), Some("done"), 1, true, 1, false),
        "standard output:\n{out}\nstandard error:\n{err}"
    );
}
