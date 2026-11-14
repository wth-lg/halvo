//! Captured text: the title text, the site and both string forms of the app
//! identity. A captured string enters only through [`CapturedText::enter`] and
//! leaves only through the record encoder, which hands it to the
//! [`TextFilterPoint`] that it is given and writes what the point returns.

use std::fmt;

use zeroize::Zeroize;

/// The most bytes that the text entry keeps of one captured string.
pub const TEXT_MAX_BYTES: usize = 1024;

/// The field that a captured string is stored in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextField {
    /// The app identity in the form of a bundle id.
    BundleId,
    /// The app identity in the form of an executable path; a cut keeps its tail.
    ExecutablePath,
    /// The title text of a window.
    Title,
    /// The registrable domain of a site.
    Site,
}

impl TextField {
    /// Whether a cut of this field keeps the tail of the string.
    pub const fn keeps_tail(self) -> bool {
        matches!(self, TextField::ExecutablePath)
    }
}

/// The text filter point of the record encoder (contracts.md C13.6): every
/// captured string passes through it once, and the encoder writes what it
/// returns and no other text. It has one implementor, under daemon/, and what
/// that implementor returns is what bounds stored text; the cap of
/// [`CapturedText::enter`] bounds memory in the queue alone.
pub trait TextFilterPoint {
    /// The stored text that the point returns.
    type Stored: AsRef<str>;

    /// The stored form of `text`, captured for `field`.
    fn filter(&self, field: TextField, text: &str) -> Self::Stored;
}

/// One captured string, at most [`TEXT_MAX_BYTES`] bytes, cut on a character
/// boundary. It has no accessor and no `Display`; its `Debug` prints a fixed
/// placeholder, and its buffer is wiped when it drops.
pub struct CapturedText {
    bytes: Box<str>,
    cut: bool,
}

impl CapturedText {
    /// The floor's text entry: keeps at most [`TEXT_MAX_BYTES`] bytes of `raw`
    /// on a character boundary, the tail for a field that keeps its tail and the
    /// head for every other field, records whether it cut, and wipes `raw`.
    pub fn enter(field: TextField, mut raw: String) -> CapturedText {
        let (start, end) = kept_range(&raw, TEXT_MAX_BYTES, field.keeps_tail());
        let cut = start != 0 || end != raw.len();
        let bytes = Box::<str>::from(&raw[start..end]);
        raw.zeroize();
        CapturedText { bytes, cut }
    }

    /// Whether the entry cut the string.
    pub fn was_cut(&self) -> bool {
        self.cut
    }

    #[cfg(test)]
    pub(crate) fn heap_bytes(&self) -> usize {
        self.bytes.len()
    }

    /// Whether the string, or with `file_name` its part after the last `/` or
    /// `\`, equals one of `names`, ignoring ASCII case: the one comparison of
    /// captured text, for the static exclusion list.
    pub(crate) fn names_one_of(&self, names: &[&str], file_name: bool) -> bool {
        let text: &str = &self.bytes;
        let compared = if file_name {
            text.rsplit(['/', '\\']).next().unwrap_or(text)
        } else {
            text
        };
        names.iter().any(|name| name.eq_ignore_ascii_case(compared))
    }
}

/// The string writer of the record encoder and the one reader of captured
/// text: it hands `text` to `point` for `field`, appends what the point returns
/// to `out` as a JSON string, and says whether the point changed the text.
pub(super) fn write_through<P: TextFilterPoint>(
    text: &CapturedText,
    field: TextField,
    point: &P,
    out: &mut String,
) -> bool {
    let stored = point.filter(field, &text.bytes);
    let stored = stored.as_ref();
    push_json_string(out, stored);
    stored != &*text.bytes
}

/// Appends `text` to `out` as a JSON string with the escapes of serde_json. It
/// writes into `out` alone and makes no copy of its own; it does not wipe the
/// point's `Stored` value, and a buffer that grows past its capacity leaves its
/// old bytes in freed memory.
pub(super) fn push_json_string(out: &mut String, text: &str) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => {
                let byte = u32::from(c) as usize;
                out.push_str("\\u00");
                out.push(char::from(HEX[byte >> 4]));
                out.push(char::from(HEX[byte & 0xf]));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn kept_range(raw: &str, max: usize, tail: bool) -> (usize, usize) {
    if raw.len() <= max {
        return (0, raw.len());
    }
    if tail {
        let mut start = raw.len() - max;
        while !raw.is_char_boundary(start) {
            start += 1;
        }
        (start, raw.len())
    } else {
        let mut end = max;
        while !raw.is_char_boundary(end) {
            end -= 1;
        }
        (0, end)
    }
}

impl fmt::Debug for CapturedText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CapturedText(..)")
    }
}

impl Drop for CapturedText {
    fn drop(&mut self) {
        self.bytes.zeroize();
    }
}
