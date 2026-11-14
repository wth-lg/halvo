use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::Path;

use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};

/// A refusal of a command (contracts.md 0.4): a cause token of the command's closed list and a detail, which is
/// the path or the argument at fault and never holds a title, an app id or a site name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    pub cause: &'static str,
    pub detail: String,
}

impl Refusal {
    /// A refusal with this cause and detail.
    pub fn new(cause: &'static str, detail: impl Into<String>) -> Refusal {
        Refusal {
            cause,
            detail: detail.into(),
        }
    }
}

/// The nanoseconds of one scripted day.
pub const DAY: i64 = 86_400_000_000_000;

/// The nanoseconds of one second.
pub const SECOND: i64 = 1_000_000_000;

/// The nanoseconds of one millisecond.
pub const MILLI: i64 = 1_000_000;

/// The last scripted day.
pub const LAST_DAY: u32 = 93;

/// The longest title in bytes (C4, C10.9).
pub const TITLE_MAX: usize = 1_024;

/// True when `s` is an id of contracts.md 0.7: 1 to 48 bytes of lower-case letters, digits and hyphens.
pub fn is_id(s: &str) -> bool {
    (1..=48).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The 64 lower-case hex digits of the SHA-256 of `bytes` (contracts.md 0.3).
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// The bytes in lower-case hex.
pub fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// The hash of a file's bytes, read in pieces.
pub fn file_sha256(path: &Path) -> io::Result<String> {
    use std::io::Read;
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 65_536];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex(&hasher.finalize()))
}

/// A JSON string as a compact JSON writer writes it (C4.8 item 3).
pub fn json_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_default()
}

/// A list of strings as compact JSON.
pub fn json_strs<S: AsRef<str>>(list: &[S]) -> String {
    let inner: Vec<String> = list.iter().map(|s| json_str(s.as_ref())).collect();
    format!("[{}]", inner.join(","))
}

/// A string or null as compact JSON.
pub fn json_opt(s: Option<&str>) -> String {
    s.map_or_else(|| "null".to_owned(), json_str)
}

/// Reads one JSON object into `T` and takes it only when `write` gives back the same bytes: the compact form
/// with the keys in the contract's order, with no white space, no escape that the writer does not write and
/// no key twice.
pub fn read_exact<T: DeserializeOwned>(bytes: &[u8], write: impl Fn(&T) -> String) -> Option<T> {
    let value: T = serde_json::from_slice(bytes).ok()?;
    (write(&value).as_bytes() == bytes).then_some(value)
}

/// The lines of a text file by 0.2: each ended by LF, the last included. None when the bytes are no such file
/// (a last line with no LF, or bytes that are not UTF-8). An empty file has no line.
pub fn text_lines(bytes: &[u8]) -> Option<Vec<&str>> {
    let text = std::str::from_utf8(bytes).ok()?;
    if text.is_empty() {
        return Some(Vec::new());
    }
    let body = text.strip_suffix('\n')?;
    Some(body.split('\n').collect())
}

/// The instant of a scripted day and offset: day1 + (day - 1) x DAY + offset, or none past the range of i64.
pub fn instant(day1: i64, day: u32, offset: i64) -> Option<i64> {
    let base = i64::from(day).checked_sub(1)?.checked_mul(DAY)?;
    day1.checked_add(base)?.checked_add(offset)
}

/// The number of whole seconds that lie after `start` and before `end`.
pub fn whole_seconds_between(start: i64, end: i64) -> i64 {
    let first = start.div_euclid(SECOND) + 1;
    let last = (end - 1).div_euclid(SECOND);
    (last - first + 1).max(0)
}

/// Writes `bytes` to a new file at `path`.
pub fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.flush()
}

/// Creates the directory `dir`, its parents where they are missing, and refuses with `exists` when `dir` is
/// already there and with `output` when it cannot be made.
pub fn create_out_dir(dir: &Path) -> Result<(), Refusal> {
    if fs::symlink_metadata(dir).is_ok() {
        return Err(Refusal::new("exists", dir.display().to_string()));
    }
    if let Some(parent) = dir.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|_| Refusal::new("output", dir.display().to_string()))?;
        }
    }
    match fs::create_dir(dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            Err(Refusal::new("exists", dir.display().to_string()))
        }
        Err(_) => Err(Refusal::new("output", dir.display().to_string())),
    }
}

/// The value of a decimal argument from 0 to 2^63 - 1, written with no sign and no leading zero.
pub fn parse_seed(s: &str) -> Option<u64> {
    let ok =
        !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && (s == "0" || !s.starts_with('0'));
    if !ok {
        return None;
    }
    s.parse::<u64>().ok().filter(|n| *n <= i64::MAX as u64)
}
