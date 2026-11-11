use std::fmt;
use std::mem;

use serde::de::{DeserializeSeed, Deserializer, Error, MapAccess, Visitor};

use crate::json::{self, Count, Keys};

/// The reason of a refusal, one of the eight of B-10, in this crate's form; the bytes of an answer
/// write it in the billing run's form, with hyphens (`over-cap`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reason {
    OverCap,
    NoCap,
    NotActive,
    Pending,
    Closing,
    NoSuchAccount,
    CredentialMismatch,
    OtherArguments,
}

impl Reason {
    /// Every value, in the order of T11.5.
    pub const ALL: [Reason; 8] = [
        Reason::OverCap,
        Reason::NoCap,
        Reason::NotActive,
        Reason::Pending,
        Reason::Closing,
        Reason::NoSuchAccount,
        Reason::CredentialMismatch,
        Reason::OtherArguments,
    ];

    /// The word of the value in this crate's form, with underscores: `over_cap` and so on.
    pub const fn as_str(self) -> &'static str {
        match self {
            Reason::OverCap => "over_cap",
            Reason::NoCap => "no_cap",
            Reason::NotActive => "not_active",
            Reason::Pending => "pending",
            Reason::Closing => "closing",
            Reason::NoSuchAccount => "no_such_account",
            Reason::CredentialMismatch => "credential_mismatch",
            Reason::OtherArguments => "other_arguments",
        }
    }

    const fn wire(self) -> &'static str {
        match self {
            Reason::OverCap => "over-cap",
            Reason::NoCap => "no-cap",
            Reason::NotActive => "not-active",
            Reason::Pending => "pending",
            Reason::Closing => "closing",
            Reason::NoSuchAccount => "no-such-account",
            Reason::CredentialMismatch => "credential-mismatch",
            Reason::OtherArguments => "other-arguments",
        }
    }
}

/// Why an answer is not read. `Status`: the status is neither 200 nor 403. `Malformed`: any other
/// fault, among them an unknown reason and a `result` that does not fit the status.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnswerError {
    Status,
    Malformed,
}

impl AnswerError {
    /// Every value, in the order of T11.5.
    pub const ALL: [AnswerError; 2] = [AnswerError::Status, AnswerError::Malformed];

    /// The word of the error: `status` or `malformed`.
    pub const fn as_str(self) -> &'static str {
        match self {
            AnswerError::Status => "status",
            AnswerError::Malformed => "malformed",
        }
    }
}

/// One complete answer of the gateway once the upstream call has ended (T11.5): a success with the
/// text, the input and output tokens and the settled cost in micro-units, or a refusal.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Answer {
    Ok {
        content: String,
        tokens_in: u64,
        tokens_out: u64,
        cost: u64,
    },
    Refused(Reason),
}

impl Answer {
    /// Reads status 200 as `{"result":"ok","content":<text>,"usage":{"input":<count>,"output":<count>},"cost":<count>}`
    /// and status 403 as `{"result":"refused","reason":<reason>}`. Both objects are closed, their
    /// keys stand in any order with JSON white space between tokens, a repeated key is `Malformed`,
    /// and a count is digits alone that fit a `u64`. Any other status is `Status`, whatever the bytes.
    pub fn parse(status: u16, bytes: &[u8]) -> Result<Answer, AnswerError> {
        let shape = match status {
            200 => Shape::Ok,
            403 => Shape::Refused,
            _ => return Err(AnswerError::Status),
        };
        json::text(bytes)
            .and_then(|text| json::read(text, shape))
            .ok_or(AnswerError::Malformed)
    }
}

const OK_KEYS: &[&str] = &["result", "content", "usage", "cost"];
const USAGE_KEYS: &[&str] = &["input", "output"];
const REFUSED_KEYS: &[&str] = &["result", "reason"];

fn next_slot<'de, A: MapAccess<'de>>(
    map: &mut A,
    keys: &'static [&'static str],
    seen: &mut [bool],
) -> Result<Option<usize>, A::Error> {
    match map.next_key_seed(Keys(keys))? {
        None if seen.contains(&false) => Err(A::Error::custom("a key is missing")),
        None => Ok(None),
        Some(None) => Err(A::Error::custom("an unknown key")),
        Some(Some(slot)) if mem::replace(&mut seen[slot], true) => {
            Err(A::Error::custom("a repeated key"))
        }
        Some(Some(slot)) => Ok(Some(slot)),
    }
}

fn word<'de, A: MapAccess<'de>>(map: &mut A, want: &str) -> Result<(), A::Error> {
    if map.next_value::<String>()? == want {
        Ok(())
    } else {
        Err(A::Error::custom("a result that does not fit the status"))
    }
}

#[derive(Clone, Copy)]
enum Shape {
    Ok,
    Refused,
}

impl<'de> DeserializeSeed<'de> for Shape {
    type Value = Answer;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Answer, D::Error> {
        deserializer.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for Shape {
    type Value = Answer;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("an answer object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Answer, A::Error> {
        match self {
            Shape::Ok => read_ok(&mut map),
            Shape::Refused => read_refused(&mut map),
        }
    }
}

fn read_ok<'de, A: MapAccess<'de>>(map: &mut A) -> Result<Answer, A::Error> {
    let mut seen = [false; 4];
    let mut content = None;
    let mut usage = None;
    let mut cost = None;
    while let Some(slot) = next_slot(map, OK_KEYS, &mut seen)? {
        match slot {
            0 => word(map, "ok")?,
            1 => content = Some(map.next_value::<String>()?),
            2 => usage = Some(map.next_value_seed(UsageSeed)?),
            _ => cost = Some(map.next_value_seed(Count(u64::MAX))?),
        }
    }
    match (content, usage, cost) {
        (Some(content), Some((tokens_in, tokens_out)), Some(cost)) => Ok(Answer::Ok {
            content,
            tokens_in,
            tokens_out,
            cost,
        }),
        _ => Err(A::Error::custom("a key is missing")),
    }
}

fn read_refused<'de, A: MapAccess<'de>>(map: &mut A) -> Result<Answer, A::Error> {
    let mut seen = [false; 2];
    let mut reason = None;
    while let Some(slot) = next_slot(map, REFUSED_KEYS, &mut seen)? {
        if slot == 0 {
            word(map, "refused")?;
        } else {
            let text = map.next_value::<String>()?;
            let found = Reason::ALL.into_iter().find(|known| known.wire() == text);
            reason = Some(found.ok_or_else(|| A::Error::custom("an unknown reason"))?);
        }
    }
    reason
        .map(Answer::Refused)
        .ok_or_else(|| A::Error::custom("a key is missing"))
}

struct UsageSeed;

impl<'de> DeserializeSeed<'de> for UsageSeed {
    type Value = (u64, u64);

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(u64, u64), D::Error> {
        deserializer.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for UsageSeed {
    type Value = (u64, u64);

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a usage object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(u64, u64), A::Error> {
        let mut seen = [false; 2];
        let mut counts = [0; 2];
        while let Some(slot) = next_slot(&mut map, USAGE_KEYS, &mut seen)? {
            counts[slot] = map.next_value_seed(Count(u64::MAX))?;
        }
        Ok((counts[0], counts[1]))
    }
}
