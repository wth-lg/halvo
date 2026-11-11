use std::fmt;

use serde::de::{DeserializeSeed, Deserializer, Error, Unexpected, Visitor};

pub(crate) fn text(bytes: &[u8]) -> Option<&str> {
    let text = std::str::from_utf8(bytes).ok()?;
    if lone_surrogate(text.as_bytes()) {
        return None;
    }
    Some(text)
}

fn lone_surrogate(bytes: &[u8]) -> bool {
    let mut at = 0;
    let mut in_string = false;
    let mut high = false;
    while at < bytes.len() {
        let byte = bytes[at];
        if !in_string {
            in_string = byte == b'"';
            at += 1;
            continue;
        }
        let unit = match byte {
            b'\\' if bytes.get(at + 1) == Some(&b'u') => {
                match bytes.get(at + 2..at + 6).and_then(hex4) {
                    Some(unit) => unit,
                    None => return false,
                }
            }
            _ => {
                if high {
                    return true;
                }
                in_string = byte != b'"';
                at += if byte == b'\\' { 2 } else { 1 };
                continue;
            }
        };
        let is_low = (0xdc00..=0xdfff).contains(&unit);
        if is_low != high {
            return true;
        }
        high = (0xd800..=0xdbff).contains(&unit);
        at += 6;
    }
    high
}

fn hex4(digits: &[u8]) -> Option<u16> {
    let text = std::str::from_utf8(digits).ok()?;
    if !text.bytes().all(|digit| digit.is_ascii_hexdigit()) {
        return None;
    }
    u16::from_str_radix(text, 16).ok()
}

pub(crate) fn read<'a, S>(text: &'a str, seed: S) -> Option<S::Value>
where
    S: DeserializeSeed<'a>,
{
    let mut reader = serde_json::Deserializer::from_str(text);
    let value = seed.deserialize(&mut reader).ok()?;
    reader.end().ok()?;
    Some(value)
}

pub(crate) struct Count(pub(crate) u64);

impl<'de> DeserializeSeed<'de> for Count {
    type Value = u64;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<u64, D::Error> {
        deserializer.deserialize_u64(self)
    }
}

impl Visitor<'_> for Count {
    type Value = u64;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(formatter, "digits alone, at most {}", self.0)
    }

    fn visit_u64<E: Error>(self, value: u64) -> Result<u64, E> {
        if value <= self.0 {
            Ok(value)
        } else {
            Err(E::invalid_value(Unexpected::Unsigned(value), &self))
        }
    }
}

pub(crate) struct Keys(pub(crate) &'static [&'static str]);

impl<'de> DeserializeSeed<'de> for Keys {
    type Value = Option<usize>;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Option<usize>, D::Error> {
        deserializer.deserialize_identifier(self)
    }
}

impl Visitor<'_> for Keys {
    type Value = Option<usize>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a key")
    }

    fn visit_str<E: Error>(self, key: &str) -> Result<Option<usize>, E> {
        Ok(self.0.iter().position(|name| *name == key))
    }
}

const HEX: &[u8; 16] = b"0123456789abcdef";

pub(crate) fn write_string(out: &mut Vec<u8>, text: &str) {
    out.push(b'"');
    for character in text.chars() {
        match character {
            '"' => out.extend_from_slice(b"\\\""),
            '\\' => out.extend_from_slice(b"\\\\"),
            '\u{8}' => out.extend_from_slice(b"\\b"),
            '\u{c}' => out.extend_from_slice(b"\\f"),
            '\n' => out.extend_from_slice(b"\\n"),
            '\r' => out.extend_from_slice(b"\\r"),
            '\t' => out.extend_from_slice(b"\\t"),
            '\u{0}'..='\u{1f}' => {
                let code = character as usize;
                out.extend_from_slice(b"\\u00");
                out.push(HEX[code >> 4]);
                out.push(HEX[code & 0xf]);
            }
            _ => {
                let mut buffer = [0; 4];
                out.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
            }
        }
    }
    out.push(b'"');
}
