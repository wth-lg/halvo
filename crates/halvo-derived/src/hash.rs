use halvo_schema_core::site::{SiteHash, SiteToken};
use sha2::{Digest, Sha256};

use crate::error::Fault;

const BLOCK: usize = 64;

/// The keyed hash of a title-derived site token: HMAC-SHA256 under the key of the site hash, which lives in the
/// derived database alone, written as 64 lower-case hex digits.
pub(crate) fn site_hash(key: &[u8; 32], token: &SiteToken) -> Result<SiteHash, Fault> {
    let mut block = [0u8; BLOCK];
    block[..key.len()].copy_from_slice(key);
    let mut inner = Sha256::new();
    inner.update(block.map(|b| b ^ 0x36));
    inner.update(token.as_str().as_bytes());
    let mut outer = Sha256::new();
    outer.update(block.map(|b| b ^ 0x5c));
    outer.update(inner.finalize());
    block.fill(0);
    std::hint::black_box(&block);
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut hex = String::with_capacity(64);
    for b in outer.finalize() {
        hex.push(char::from(DIGITS[usize::from(b >> 4)]));
        hex.push(char::from(DIGITS[usize::from(b & 0x0f)]));
    }
    SiteHash::parse(&hex).map_err(|_| Fault::Row)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_site_hash_is_hmac_sha256() {
        let mut key = [0u8; 32];
        key[..20].copy_from_slice(&[0x0b; 20]);
        let token = SiteToken::parse("northwind-power").unwrap();
        assert_eq!(
            site_hash(&key, &token).unwrap().as_str(),
            "d8f21dc639cba05e1aebe491f10d0522e43af30ccf75e9ace19b3f1c4aaeac14"
        );
    }
}
