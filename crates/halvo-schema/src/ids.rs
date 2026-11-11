use std::fmt;
use std::marker::PhantomData;

use serde::de::{self, Deserialize, Deserializer, Visitor};
use serde::ser::Serializer;

/// The version of every serialized record, request and reply, written first under the key `v`.
/// A reader refuses any other number.
pub const VERSION: u32 = 1;

/// The one error of every parser of this crate. It holds the Rust name of the type that refused the input
/// and the token of the rule that refused it, and never the refused input.
/// `Display` prints `<type name>: <rule token>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParseError {
    type_name: &'static str,
    rule: &'static str,
}

impl ParseError {
    pub(crate) const fn new(type_name: &'static str, rule: &'static str) -> Self {
        ParseError { type_name, rule }
    }

    /// The Rust name of the type that refused the input, such as `AppId`.
    pub fn type_name(&self) -> &'static str {
        self.type_name
    }

    /// The token of the rule that refused the input: lower-case words joined by `_`.
    pub fn rule(&self) -> &'static str {
        self.rule
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.type_name, self.rule)
    }
}

impl std::error::Error for ParseError {}

pub(crate) struct ParsedStr<T> {
    parse: fn(&str) -> Result<T, ParseError>,
    expecting: &'static str,
    _value: PhantomData<T>,
}

impl<'de, T> Visitor<'de> for ParsedStr<T> {
    type Value = T;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.expecting)
    }

    fn visit_str<E: de::Error>(self, s: &str) -> Result<T, E> {
        (self.parse)(s).map_err(E::custom)
    }
}

pub(crate) fn parse_str<'de, D, T>(
    deserializer: D,
    expecting: &'static str,
    parse: fn(&str) -> Result<T, ParseError>,
) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_str(ParsedStr {
        parse,
        expecting,
        _value: PhantomData,
    })
}

pub(crate) fn required<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer)
}

pub(crate) trait Strict {
    fn rules(&self) -> Result<(), ParseError>;
    fn write_fields<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error>;
}

macro_rules! strict_struct {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident $([$v:ident])? {
            $( $(#[$fmeta:meta])* pub $field:ident : $ty:ty ),+ $(,)?
        }
        rules($this:ident) $rules:block
    ) => {
        $(#[$meta])*
        $vis struct $name {
            $( $(#[$fmeta])* pub $field: $ty, )+
        }

        impl $crate::ids::Strict for $name {
            fn rules(&self) -> Result<(), $crate::ids::ParseError> {
                let $this = self;
                $rules
            }

            fn write_fields<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                #[derive(serde::Serialize)]
                struct Out<'a> {
                    $( $v: u32, )?
                    $( $field: &'a $ty, )+
                }
                serde::Serialize::serialize(
                    &Out { $( $v: $crate::ids::VERSION, )? $( $field: &self.$field, )+ },
                    serializer,
                )
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                $crate::ids::Strict::rules(self).map_err(<S::Error as serde::ser::Error>::custom)?;
                $crate::ids::Strict::write_fields(self, serializer)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                #[derive(serde::Deserialize)]
                #[serde(deny_unknown_fields)]
                #[allow(dead_code)]
                struct In {
                    $( $v: $crate::ids::Version, )?
                    $( #[serde(deserialize_with = "crate::ids::required")] $field: $ty, )+
                }
                let input = <In as serde::Deserialize>::deserialize(deserializer)?;
                let value = $name { $( $field: input.$field, )+ };
                $crate::ids::Strict::rules(&value).map_err(<D::Error as serde::de::Error>::custom)?;
                Ok(value)
            }
        }
    };
}

macro_rules! string_type {
    (
        $(#[$meta:meta])*
        $name:ident [$($derive:ident),*] = $check:path
    ) => {
        $(#[$meta])*
        #[derive(Clone, PartialEq, Eq $(, $derive)*)]
        pub struct $name(String);

        impl $name {
            /// Returns the value when the whole string has the form of this type, kept as given;
            /// else the error that names the first rule it breaks.
            pub fn parse(s: &str) -> Result<Self, $crate::ids::ParseError> {
                match $check(s) {
                    Ok(()) => Ok($name(s.to_owned())),
                    Err(rule) => Err($crate::ids::ParseError::new(stringify!($name), rule)),
                }
            }

            /// The value as it was parsed.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(&self.0)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                $crate::ids::parse_str(deserializer, concat!("a string of the form of ", stringify!($name)), $name::parse)
            }
        }
    };
}

fn hex(s: &str, len: usize) -> Result<(), &'static str> {
    if s.len() != len {
        return Err("length");
    }
    if !s
        .bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("hex_digit");
    }
    Ok(())
}

pub(crate) fn hex32(s: &str) -> Result<(), &'static str> {
    hex(s, 32)
}

fn episode(s: &str) -> Result<(), &'static str> {
    if s.is_empty() || s.len() > 48 {
        return Err("length");
    }
    if !s
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err("character");
    }
    Ok(())
}

fn random_hex(bytes: [u8; 16]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(32);
    for b in bytes {
        out.push(char::from(DIGITS[usize::from(b >> 4)]));
        out.push(char::from(DIGITS[usize::from(b & 0x0f)]));
    }
    out
}

macro_rules! random_id {
    ($(#[$meta:meta])* $name:ident) => {
        string_type! {
            $(#[$meta])*
            $name [Debug, PartialOrd, Ord, Hash] = hex32
        }

        impl $name {
            /// Writes the 16 bytes as 32 lower-case hex digits, in order. The caller supplies the bytes:
            /// the operating system's generator in the product, a seeded generator in `propose` and `replay`.
            pub fn from_random(bytes: [u8; 16]) -> Self {
                $name(random_hex(bytes))
            }
        }
    };
}

random_id! {
    /// The id of a stored workflow: 32 lower-case hex digits from 128 random bits, minted at commit.
    WorkflowId
}

random_id! {
    /// The id of a summary: 32 lower-case hex digits from 128 random bits, minted by the sender of the analysis write.
    SummaryId
}

random_id! {
    /// The id of an agent: 32 lower-case hex digits from 128 random bits, minted by the agent-setup run.
    AgentId
}

random_id! {
    /// The id of a request: 32 lower-case hex digits from 128 random bits, minted by its sender.
    /// A sender that repeats a request repeats its id.
    RequestId
}

string_type! {
    /// The id of an episode of a label row: 1 to 48 bytes of lower-case letters, digits and hyphens.
    EpisodeId [Debug, PartialOrd, Ord, Hash] = episode
}

string_type! {
    /// The store id of the seam: 32 lower-case hex digits. It has no random constructor.
    StoreId [Debug] = hex32
}

strict_struct! {
    /// A read position of the seam that is never reused: the segment ordinal and the record index inside that
    /// segment, each from 0 to 2^63 - 1. Positions order by `seg`, then by `idx`. Written
    /// `{"seg":<n>,"idx":<n>}`; a reader refuses a number outside the range, and `Serialize` refuses to write one.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct Position {
        pub seg: u64,
        pub idx: u64,
    }
    rules(p) {
        if p.seg > MAX_ORDINAL || p.idx > MAX_ORDINAL {
            return Err(ParseError::new("Position", "range"));
        }
        Ok(())
    }
}

pub(crate) const MAX_ORDINAL: u64 = i64::MAX as u64;
