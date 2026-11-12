//! Test-only parts of the floor that the crate's tests share: the text filter
//! points of the tests.

use super::text::{write_through, CapturedText, TextField, TextFilterPoint};

/// A point that stores every text unchanged.
pub(crate) struct IdentityPoint;

impl TextFilterPoint for IdentityPoint {
    type Stored = String;

    fn filter(&self, _field: TextField, text: &str) -> String {
        text.to_owned()
    }
}

/// The text that the encoder writes for `text` in `field` through
/// [`IdentityPoint`].
pub(crate) fn stored(text: &CapturedText, field: TextField) -> String {
    let mut out = String::new();
    let changed = write_through(text, field, &IdentityPoint, &mut out);
    assert!(!changed, "the identity point changed a text");
    serde_json::from_str(&out).expect("the writer wrote a JSON string")
}
