//! The stand-in model door of the tests of the whole runner (r3 U7.10; the coordinator's rulings of 2026-11-26, P6
//! and M5), built with the feature `loopback` alone (`required-features` of its entry in Cargo.toml), which no shipped
//! build turns on. A test copies it as `halvo-model` beside a copy of the built runner, where the runner starts its
//! door (crate::model::ask, `Place::beside_the_runner`). It reads the head of the key that the runner writes first and
//! keeps no byte of it; then it answers each request line with the next line of the file `answers.txt` beside it (the
//! last once they run out), as a reply of 100 and 10 tokens, where `@ref:<word>@` in that line stands for the
//! reference of the first line of the request's snapshot that holds `<word>`. It writes the FIRST request line, whole,
//! into the file `first-request.json` beside it, for the test to print. It opens no socket and reaches nothing.

use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};

fn main() {
    let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    else {
        std::process::exit(2);
    };
    std::process::exit(serve(&dir));
}

/// Serves the runner on standard input and output until the input ends.
fn serve(dir: &Path) -> i32 {
    let answers: Vec<String> = std::fs::read_to_string(dir.join("answers.txt"))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect();
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut count = Vec::new();
    if input.read_until(b'\n', &mut count).is_err() {
        return 1;
    }
    let n: usize = String::from_utf8_lossy(&count)
        .trim_end()
        .parse()
        .unwrap_or(0);
    let mut key = vec![0u8; n + 1];
    if input.read_exact(&mut key).is_err() {
        return 1;
    }
    key.fill(0);
    let first: PathBuf = dir.join("first-request.json");
    let mut asked = 0usize;
    loop {
        let mut line = Vec::new();
        match input.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => return 0,
            Ok(_) => {}
        }
        if asked == 0 {
            let _ = std::fs::write(&first, &line);
        }
        let request: serde_json::Value = serde_json::from_slice(&line).unwrap_or_default();
        let id = request["id"].as_u64().unwrap_or(0);
        let snapshot = request["snapshot"].as_str().unwrap_or_default();
        let answer = answers
            .get(asked)
            .or(answers.last())
            .map(|answer| with_refs(answer, snapshot))
            .unwrap_or_default();
        asked += 1;
        let reply = format!(
            "{{\"id\":{id},\"text\":{},\"tokens_in\":100,\"tokens_out\":10,\"first_token_ms\":1,\"ms\":2}}",
            serde_json::Value::String(answer)
        );
        if writeln!(out, "{reply}").and_then(|()| out.flush()).is_err() {
            return 0;
        }
    }
}

/// `answer` with each `@ref:<word>@` replaced by the reference of the first line of `snapshot` that holds `<word>`,
/// the first word of that line; by the empty text where no line holds it.
fn with_refs(answer: &str, snapshot: &str) -> String {
    let mut out = String::new();
    let mut rest = answer;
    while let Some(at) = rest.find("@ref:") {
        out.push_str(&rest[..at]);
        let after = &rest[at + "@ref:".len()..];
        let Some(end) = after.find('@') else {
            out.push_str(&rest[at..]);
            return out;
        };
        let word = &after[..end];
        let reference = snapshot
            .lines()
            .find(|line| line.contains(word))
            .and_then(|line| line.split(' ').next())
            .unwrap_or_default();
        out.push_str(reference);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}
