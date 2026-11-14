use std::fmt;
use std::num::NonZeroU64;

use serde::{Serialize, Serializer};
use serde_json::value::RawValue;

use super::json::{self, FromJson};
use super::Refusal;

/// The most bytes of one `own_text`.
pub const OWN_TEXT_MAX: usize = 1024;

/// A text that capture did not supply (field type `own_text`): a string of at most 1,024 bytes, never masked and
/// never shortened. `Debug` prints the type name and the length alone.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct OwnText(String);

impl OwnText {
    /// Returns the text when it has at most 1,024 bytes, else the refusal with the rule `own_text_length`.
    pub fn parse(s: &str) -> Result<OwnText, Refusal> {
        if s.len() > OWN_TEXT_MAX {
            return Err(Refusal::malformed("OwnText", "own_text_length"));
        }
        Ok(OwnText(s.to_owned()))
    }

    /// The text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for OwnText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "OwnText({} bytes)", self.0.len())
    }
}

impl Serialize for OwnText {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl FromJson for OwnText {
    fn from_json(raw: &RawValue) -> Result<Self, Refusal> {
        OwnText::parse(&json::string("OwnText", raw)?)
    }
}

/// The id that a provider gave a message or a draft, written as one `own_text`: 1 to 1,024 bytes, never masked and
/// never shortened. `Debug` prints the type name and the length alone.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ProviderId(String);

impl ProviderId {
    /// Returns the id when it has 1 to 1,024 bytes, else the refusal with the rule `empty_id` or `own_text_length`.
    pub fn parse(s: &str) -> Result<ProviderId, Refusal> {
        if s.is_empty() {
            return Err(Refusal::malformed("ProviderId", "empty_id"));
        }
        if s.len() > OWN_TEXT_MAX {
            return Err(Refusal::malformed("ProviderId", "own_text_length"));
        }
        Ok(ProviderId(s.to_owned()))
    }

    /// The id.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ProviderId({} bytes)", self.0.len())
    }
}

impl Serialize for ProviderId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl FromJson for ProviderId {
    fn from_json(raw: &RawValue) -> Result<Self, Refusal> {
        ProviderId::parse(&json::string("ProviderId", raw)?)
    }
}

fn cut(s: &str) -> Vec<String> {
    let mut pieces = Vec::new();
    let mut rest = s;
    while !rest.is_empty() {
        let mut end = rest.len().min(OWN_TEXT_MAX);
        while !rest.is_char_boundary(end) {
            end -= 1;
        }
        pieces.push(rest[..end].to_owned());
        rest = &rest[end..];
    }
    pieces
}

/// A value written as a list of `own_text` pieces, `["<own_text>"...]`: the value cut at character bounds into the
/// longest pieces of at most 1,024 bytes, in order, which a reader joins. The empty value is the empty list. A list
/// that is not this cut of its joined value is refused, so one value has one form. `Debug` prints the length alone.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct OwnValue(Vec<String>);

impl OwnValue {
    /// The cut of `s`.
    pub fn new(s: &str) -> OwnValue {
        OwnValue(cut(s))
    }

    /// The pieces in order.
    pub fn pieces(&self) -> &[String] {
        &self.0
    }

    /// The joined value.
    pub fn joined(&self) -> String {
        self.0.concat()
    }

    /// The number of bytes of the joined value.
    pub fn len(&self) -> usize {
        self.0.iter().map(String::len).sum()
    }

    /// Whether the value is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for OwnValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "OwnValue({} bytes)", self.len())
    }
}

impl Serialize for OwnValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl FromJson for OwnValue {
    fn from_json(raw: &RawValue) -> Result<Self, Refusal> {
        let pieces: Vec<OwnText> = json::list(OwnText::from_json)(raw)
            .map_err(|r| r.at("value"))?
            .into_iter()
            .collect();
        let pieces: Vec<String> = pieces.into_iter().map(|p| p.0).collect();
        if cut(&pieces.concat()) != pieces {
            return Err(Refusal::malformed("OwnValue", "not_the_cut"));
        }
        Ok(OwnValue(pieces))
    }
}

fn name_form(s: &str) -> bool {
    (1..=32).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// A name (field type `name`): 1 to 32 bytes of `a` to `z`, `0` to `9` and `_`. A slot name, an arg name, the name
/// under which a value is kept, a model token and a secret name have this form.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Name(String);

impl Name {
    /// Returns the name when `s` has the form, else the refusal with the rule `name_form`.
    pub fn parse(s: &str) -> Result<Name, Refusal> {
        if !name_form(s) {
            return Err(Refusal::malformed("Name", "name_form"));
        }
        Ok(Name(s.to_owned()))
    }

    /// The name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for Name {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl FromJson for Name {
    fn from_json(raw: &RawValue) -> Result<Self, Refusal> {
        Name::parse(&json::string("Name", raw)?)
    }
}

pub(in crate::executable) fn is_port(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('0')
        && s.len() <= 5
        && s.bytes().all(|b| b.is_ascii_digit())
        && s.parse::<u32>().is_ok_and(|n| (1..=65_535).contains(&n))
}

fn host(s: &str) -> bool {
    (1..=253).contains(&s.len())
        && s.split('.').all(|label| {
            (1..=63).contains(&label.len())
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
}

fn octet(label: &str) -> bool {
    (1..=3).contains(&label.len())
        && label.bytes().all(|b| b.is_ascii_digit())
        && (label == "0" || !label.starts_with('0'))
        && label.parse::<u8>().is_ok()
}

pub(in crate::executable) fn number_form(host: &str) -> bool {
    let last = host.rsplit('.').next().unwrap_or_default();
    let numeric = last.bytes().all(|b| b.is_ascii_digit())
        || last
            .strip_prefix("0x")
            .is_some_and(|hex| hex.bytes().all(|b| b.is_ascii_hexdigit()));
    numeric && !(host.split('.').count() == 4 && host.split('.').all(octet))
}

fn web_origin(s: &str) -> Option<&str> {
    let (scheme, rest) = s.split_once("://")?;
    let (h, p) = rest.rsplit_once(':')?;
    ((scheme == "http" || scheme == "https") && host(h) && is_port(p)).then_some(h)
}

/// A web origin, `<scheme>://<host>:<port>` in lower case with the port always written: the scheme `http` or
/// `https`, the host a name of dot-separated labels of `a` to `z`, `0` to `9` and `-` of at most 253 bytes, the port
/// a decimal from 1 to 65,535 with no leading zero. A host whose last label is a number, all digits or `0x` and hex
/// digits, is an IPv4 address only as four decimal labels of 0 to 255 with no leading zero; in any other form
/// (`127.1`, `0x7f000001`, `2130706433`, `0177.0.0.1`, `1.2.3.04`) it is no origin, as `WebOrigin::of_address`
/// refuses it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WebOrigin(String);

impl WebOrigin {
    /// Returns the origin when `s` has the form, else the refusal with the rule `origin_form`, or `origin_ipv4` for a
    /// host of that form whose last label is a number and that is no four decimal octets.
    pub fn parse(s: &str) -> Result<WebOrigin, Refusal> {
        let host = web_origin(s).ok_or(Refusal::malformed("WebOrigin", "origin_form"))?;
        if number_form(host) {
            return Err(Refusal::malformed("WebOrigin", "origin_ipv4"));
        }
        Ok(WebOrigin(s.to_owned()))
    }

    /// The origin.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for WebOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for WebOrigin {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl FromJson for WebOrigin {
    fn from_json(raw: &RawValue) -> Result<Self, Refusal> {
        WebOrigin::parse(&json::string("WebOrigin", raw)?)
    }
}

fn hex(s: &str, len: usize) -> bool {
    s.len() == len
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A digest: the lower-case hex SHA-256 of the compact JSON of a bound object, 64 digits.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Digest(String);

impl Digest {
    /// Returns the digest when `s` is 64 lower-case hex digits, else the refusal with the rule `hex64`.
    pub fn parse(s: &str) -> Result<Digest, Refusal> {
        if !hex(s, 64) {
            return Err(Refusal::malformed("Digest", "hex64"));
        }
        Ok(Digest(s.to_owned()))
    }

    /// The 64 digits.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for Digest {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl FromJson for Digest {
    fn from_json(raw: &RawValue) -> Result<Self, Refusal> {
        Digest::parse(&json::string("Digest", raw)?)
    }
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
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            /// Returns the id when `s` is 32 lower-case hex digits, else the refusal with the rule `hex32`.
            pub fn parse(s: &str) -> Result<Self, Refusal> {
                if !hex(s, 32) {
                    return Err(Refusal::malformed(stringify!($name), "hex32"));
                }
                Ok($name(s.to_owned()))
            }

            /// Writes the 16 bytes as 32 lower-case hex digits, in order: the one constructor from outside a
            /// parser. The sender that first names the id supplies 16 random bytes.
            pub fn from_random(bytes: [u8; 16]) -> Self {
                $name(random_hex(bytes))
            }

            /// The 32 digits.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(&self.0)
            }
        }

        impl FromJson for $name {
            fn from_json(raw: &RawValue) -> Result<Self, Refusal> {
                $name::parse(&json::string(stringify!($name), raw)?)
            }
        }
    };
}

random_id! {
    /// The id of a run: 32 lower-case hex digits from 16 random bytes.
    RunId
}

random_id! {
    /// The id of an effect: 32 lower-case hex digits from 16 random bytes.
    EffectId
}

random_id! {
    /// The id of an approval, which the supervisor mints: 32 lower-case hex digits from 16 random bytes.
    ApprovalId
}

/// The debugging address of the stand-in's browser, `127.0.0.1:<port>`, the port from 1 to 65,535.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Endpoint(String);

impl Endpoint {
    /// Returns the address when `s` is `127.0.0.1:` and a port, else the refusal with the rule `endpoint_form`.
    pub fn parse(s: &str) -> Result<Endpoint, Refusal> {
        match s.strip_prefix("127.0.0.1:") {
            Some(p) if is_port(p) => Ok(Endpoint(s.to_owned())),
            _ => Err(Refusal::malformed("Endpoint", "endpoint_form")),
        }
    }

    /// The address.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for Endpoint {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl FromJson for Endpoint {
    fn from_json(raw: &RawValue) -> Result<Self, Refusal> {
        Endpoint::parse(&json::string("Endpoint", raw)?)
    }
}

/// The creation time of a process as the process table records it on macOS and on Windows, in microseconds since
/// the Unix epoch, written as a string of 1 to 20 decimal digits with no leading zero: 1 to 18,446,744,073,709,551,615.
/// 0, which the process table gives a process whose creation time it could not read, is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CreationTime(NonZeroU64);

impl CreationTime {
    /// Returns the creation time when `s` has the form, else the refusal with the rule `creation_time`.
    pub fn parse(s: &str) -> Result<CreationTime, Refusal> {
        let fail = || Refusal::malformed("CreationTime", "creation_time");
        if s.starts_with('0') || !s.bytes().all(|b| b.is_ascii_digit()) {
            return Err(fail());
        }
        s.parse::<NonZeroU64>()
            .map(CreationTime)
            .map_err(|_| fail())
    }

    /// The creation time of the microseconds that the process table records.
    pub fn from_micros(micros: NonZeroU64) -> CreationTime {
        CreationTime(micros)
    }

    /// The microseconds.
    pub fn micros(self) -> NonZeroU64 {
        self.0
    }
}

impl fmt::Display for CreationTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Serialize for CreationTime {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0)
    }
}

impl FromJson for CreationTime {
    fn from_json(raw: &RawValue) -> Result<Self, Refusal> {
        CreationTime::parse(&json::string("CreationTime", raw)?)
    }
}

/// The value of an entry of the supervisor's secret store, which only a `secret_reply` frame carries: at least one
/// byte. `Debug` prints the type name alone.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretText(String);

impl SecretText {
    /// The value `s`, or the refusal with the rule `empty_secret` when it is empty.
    pub fn parse(s: &str) -> Result<SecretText, Refusal> {
        if s.is_empty() {
            return Err(Refusal::malformed("SecretText", "empty_secret"));
        }
        Ok(SecretText(s.to_owned()))
    }

    /// The value.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretText")
    }
}

impl Serialize for SecretText {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl FromJson for SecretText {
    fn from_json(raw: &RawValue) -> Result<Self, Refusal> {
        SecretText::parse(&json::string("SecretText", raw)?)
    }
}

/// The key `v` of a record, a frame or a request of this run: the integer 1, read from the text `1` alone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct V1;

impl Serialize for V1 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u32(crate::ids::VERSION)
    }
}

impl FromJson for V1 {
    fn from_json(raw: &RawValue) -> Result<Self, Refusal> {
        if raw.get() != "1" {
            return Err(Refusal::malformed("V1", "version"));
        }
        Ok(V1)
    }
}

/// The JSON `null` of a key that a body always holds as null, as `workflow` and `agent` of `list_checkpoints`, a
/// body over all agents: read from the text `null` alone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Null;

impl Serialize for Null {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_unit()
    }
}

impl FromJson for Null {
    fn from_json(raw: &RawValue) -> Result<Self, Refusal> {
        if raw.get() != "null" {
            return Err(Refusal::malformed("Null", "not_null"));
        }
        Ok(Null)
    }
}

/// The base64 text of one part of a stored session, `["<own_text>"...]`: 1 to 48 pieces of the base64 alphabet
/// (`A` to `Z`, `a` to `z`, `0` to `9`, `+`, `/` and `=`), each 1,024 bytes but the last, which holds 1 to 1,024.
/// `Debug` prints the number of pieces alone.
#[derive(Clone, PartialEq, Eq)]
pub struct SessionData(Vec<String>);

/// The most pieces of one part of a stored session.
pub const SESSION_PIECES_MAX: usize = 48;

impl SessionData {
    /// Returns the data when the pieces have the form, else the refusal with the rule that they break.
    pub fn new(pieces: Vec<String>) -> Result<SessionData, Refusal> {
        let fail = |rule| Err(Refusal::malformed("SessionData", rule));
        if pieces.is_empty() || pieces.len() > SESSION_PIECES_MAX {
            return fail("piece_count");
        }
        for (i, piece) in pieces.iter().enumerate() {
            if !piece
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'=')
            {
                return fail("base64");
            }
            let last = i + 1 == pieces.len();
            if piece.is_empty()
                || piece.len() > OWN_TEXT_MAX
                || (!last && piece.len() != OWN_TEXT_MAX)
            {
                return fail("piece_length");
            }
        }
        Ok(SessionData(pieces))
    }

    /// The pieces in order.
    pub fn pieces(&self) -> &[String] {
        &self.0
    }

    /// Whether the part is full: 48 pieces of 1,024 bytes.
    pub fn is_full(&self) -> bool {
        self.0.len() == SESSION_PIECES_MAX && self.0.iter().all(|p| p.len() == OWN_TEXT_MAX)
    }
}

impl fmt::Debug for SessionData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SessionData({} pieces)", self.0.len())
    }
}

impl Serialize for SessionData {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl FromJson for SessionData {
    fn from_json(raw: &RawValue) -> Result<Self, Refusal> {
        let pieces = json::list(OwnText::from_json)(raw)?;
        SessionData::new(pieces.into_iter().map(|p| p.0).collect())
    }
}
