use crate::types::Cause;
use crate::words;

/// A key of the words table (I1.1), made only from a key that the table holds, so that every
/// text of an item is the table's sentence of a key with typed values in its placeholders
/// (I3.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key(&'static str);

impl Key {
    /// The key `text`; `malformed` for a string that is no key of the table.
    pub fn parse(text: &str) -> Result<Key, Cause> {
        words::KEYS
            .iter()
            .find(|(k, _)| *k == text)
            .map(|(k, _)| Key(k))
            .ok_or(Cause::Malformed)
    }

    /// The key's name.
    pub const fn name(self) -> &'static str {
        self.0
    }

    /// The sentence of the key with its placeholders filled (`words::text`).
    pub fn text(self, pairs: &[(&str, &str)]) -> Result<String, Cause> {
        words::text(self.0, pairs)
    }
}

/// The sentence of the key `text`, which has no placeholder.
pub(crate) fn say(text: &str) -> Result<String, Cause> {
    Key::parse(text)?.text(&[])
}
