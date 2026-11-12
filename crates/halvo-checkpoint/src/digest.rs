use std::fmt;

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

impl fmt::Debug for Sha256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Sha256({})", self.hex())
    }
}
