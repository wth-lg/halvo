use halvo_schema::executable::{Bound, Digest, Refusal};
use sha2::{Digest as _, Sha256};

const HEX: &[u8; 16] = b"0123456789abcdef";

/// The digest of a bound object (A5.2): the lower-case hex SHA-256 of its compact JSON, the form that the
/// confirmation shows and the effect row carries.
pub fn digest(bound: &Bound) -> Result<Digest, Refusal> {
    let json = bound.to_json()?;
    let hash = Sha256::digest(json.as_bytes());
    let mut hex = String::with_capacity(64);
    for byte in hash {
        hex.push(char::from(HEX[usize::from(byte >> 4)]));
        hex.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    Digest::parse(&hex)
}
