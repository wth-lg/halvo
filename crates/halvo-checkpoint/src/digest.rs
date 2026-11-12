use std::fmt;
use std::io::{self, Read};

use sha2::Digest;

/// A SHA-256 digest.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Sha256([u8; 32]);

impl Sha256 {
    /// The digest of `bytes`.
    pub fn of(bytes: &[u8]) -> Sha256 {
        let mut out = [0u8; 32];
        out.copy_from_slice(&sha2::Sha256::digest(bytes));
        Sha256(out)
    }

    /// The digest as the list's row writes it: 64 lower-case hex digits.
    pub fn hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Takes 64 lower-case hex digits; `None` for any other text.
    pub fn parse(text: &str) -> Option<Sha256> {
        let digit = |b: u8| match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            _ => None,
        };
        let bytes = text.as_bytes();
        if bytes.len() != 64 {
            return None;
        }
        let mut out = [0u8; 32];
        for (at, pair) in bytes.chunks(2).enumerate() {
            out[at] = digit(pair[0])? * 16 + digit(pair[1])?;
        }
        Some(Sha256(out))
    }
}

impl Sha256 {
    /// The digest and the length of what `reader` holds, read to its end but never past
    /// `bound` bytes and one more: a length over `bound` says that it holds more.
    pub fn of_reader(reader: impl Read, bound: u64) -> io::Result<(Sha256, u64)> {
        read(reader, bound)
    }
}

impl fmt::Debug for Sha256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Sha256({})", self.hex())
    }
}

pub(crate) fn read(reader: impl Read, bound: u64) -> io::Result<(Sha256, u64)> {
    let mut hasher = sha2::Sha256::new();
    let mut limited = reader.take(bound.saturating_add(1));
    let mut buf = vec![0u8; 64 * 1024];
    let mut count = 0u64;
    loop {
        match limited.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                hasher.update(&buf[..n]);
                count += n as u64;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&hasher.finalize());
    Ok((Sha256(out), count))
}
