use std::fmt;
use std::mem;

use serde::de::{DeserializeSeed, Deserializer, Error, IgnoredAny, MapAccess, SeqAccess, Visitor};

use crate::json::{self, Count, Keys};
use crate::{MAX_BODY_BYTES, MAX_MESSAGES, MAX_OUTPUT_TOKENS};

/// A tier as a request names it, the ceiling that the gateway serves at and prices the hold at. It
/// holds the two hosted tiers alone, so `on_device` cannot stand in a request.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WireTier {
    LowCostHosted,
    Frontier,
}

impl WireTier {
    /// Every value, in the order of T11.3.
    pub const ALL: [WireTier; 2] = [WireTier::LowCostHosted, WireTier::Frontier];

    /// The word of the value in a body: `low_cost_hosted` or `frontier`.
    pub const fn as_str(self) -> &'static str {
        match self {
            WireTier::LowCostHosted => "low_cost_hosted",
            WireTier::Frontier => "frontier",
        }
    }

    /// The value whose word is `text`; any other text is `BodyError::Tier`.
    pub fn parse(text: &str) -> Result<WireTier, BodyError> {
        WireTier::ALL
            .into_iter()
            .find(|tier| tier.as_str() == text)
            .ok_or(BodyError::Tier)
    }
}

/// The effort level of a request (T1.3), which the gateway maps to the host's setting.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Effort {
    Low,
    Medium,
    High,
}

impl Effort {
    /// Every value, in the order of T11.3.
    pub const ALL: [Effort; 3] = [Effort::Low, Effort::Medium, Effort::High];

    /// The word of the value in a body: `low`, `medium` or `high`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Effort::Low => "low",
            Effort::Medium => "medium",
            Effort::High => "high",
        }
    }

    /// The value whose word is `text`; any other text is `BodyError::Effort`.
    pub fn parse(text: &str) -> Result<Effort, BodyError> {
        Effort::ALL
            .into_iter()
            .find(|effort| effort.as_str() == text)
            .ok_or(BodyError::Effort)
    }
}

/// The author of a message.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    System,
    User,
    Assistant,
}

impl Role {
    /// Every value, in the order of T11.3.
    pub const ALL: [Role; 3] = [Role::System, Role::User, Role::Assistant];

    /// The word of the value in a body: `system`, `user` or `assistant`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
        }
    }

    /// The value whose word is `text`; any other text is `BodyError::Role`.
    pub fn parse(text: &str) -> Result<Role, BodyError> {
        Role::ALL
            .into_iter()
            .find(|role| role.as_str() == text)
            .ok_or(BodyError::Role)
    }
}

/// Why a body is refused (T11.3). `Malformed`: not one JSON object, a value of the wrong JSON type,
/// a count not written with digits alone or above 4,294,967,295, or a string that is no Unicode
/// text. `Key`: an unknown, missing or repeated key. `Tier`, `Effort`, `Role`: a word off its list.
/// `Limit`: a count of messages or an output limit outside its range. `Size`: more than
/// `MAX_BODY_BYTES` bytes. Where two fit, the one that stands first in `ALL` is returned.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BodyError {
    Malformed,
    Key,
    Tier,
    Effort,
    Role,
    Limit,
    Size,
}

impl BodyError {
    /// Every value, in the order of T11.3, which is the order in which one error wins over another.
    pub const ALL: [BodyError; 7] = [
        BodyError::Malformed,
        BodyError::Key,
        BodyError::Tier,
        BodyError::Effort,
        BodyError::Role,
        BodyError::Limit,
        BodyError::Size,
    ];

    /// The word of the error: `malformed`, `key`, `tier`, `effort`, `role`, `limit` or `size`.
    pub const fn as_str(self) -> &'static str {
        match self {
            BodyError::Malformed => "malformed",
            BodyError::Key => "key",
            BodyError::Tier => "tier",
            BodyError::Effort => "effort",
            BodyError::Role => "role",
            BodyError::Limit => "limit",
            BodyError::Size => "size",
        }
    }
}

/// One message of a request: its role and its text. A message holds text and nothing else.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

/// The body of `POST /v1/chat` (T11.3): the tier as a ceiling, the effort level, the output limit
/// of the one choice that a request asks for, and 1 to `MAX_MESSAGES` messages.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Body {
    pub tier: WireTier,
    pub effort: Effort,
    pub max_output_tokens: u32,
    pub messages: Vec<Message>,
}

impl Body {
    /// Reads a body: one JSON object of UTF-8 text with the four keys of T11.3 in any order, JSON
    /// white space between tokens, and messages that hold the keys `role` and `content` in any order.
    /// A repeated key is refused, also when an escape spells it. Returns the first error of the order
    /// of `BodyError::ALL` that fits the whole text.
    pub fn parse(bytes: &[u8]) -> Result<Body, BodyError> {
        let read = json::text(bytes)
            .and_then(|text| json::read(text, BodySeed))
            .ok_or(BodyError::Malformed)?;
        read.finish(bytes.len())
    }

    /// Writes the body in its one form: the keys in the order of T11.3 with no space, the output
    /// limit in digits, and each string as `serde_json` writes one (`\"`, `\\`, the short escapes,
    /// `\u00xx` with lower-case digits for the other characters under 0x20, every other character as
    /// UTF-8). Refuses a body outside its limits with `Limit`, and then a longer text with `Size`.
    pub fn to_bytes(&self) -> Result<Vec<u8>, BodyError> {
        if !(1..=MAX_MESSAGES).contains(&self.messages.len())
            || !(1..=MAX_OUTPUT_TOKENS).contains(&self.max_output_tokens)
        {
            return Err(BodyError::Limit);
        }
        let mut out = Vec::new();
        out.extend_from_slice(b"{\"tier\":\"");
        out.extend_from_slice(self.tier.as_str().as_bytes());
        out.extend_from_slice(b"\",\"effort\":\"");
        out.extend_from_slice(self.effort.as_str().as_bytes());
        out.extend_from_slice(b"\",\"max_output_tokens\":");
        out.extend_from_slice(self.max_output_tokens.to_string().as_bytes());
        out.extend_from_slice(b",\"messages\":[");
        for (at, message) in self.messages.iter().enumerate() {
            if at > 0 {
                out.push(b',');
            }
            out.extend_from_slice(b"{\"role\":\"");
            out.extend_from_slice(message.role.as_str().as_bytes());
            out.extend_from_slice(b"\",\"content\":");
            json::write_string(&mut out, &message.content);
            out.push(b'}');
        }
        out.extend_from_slice(b"]}");
        if out.len() > MAX_BODY_BYTES {
            return Err(BodyError::Size);
        }
        Ok(out)
    }
}

const BODY_KEYS: &[&str] = &["tier", "effort", "max_output_tokens", "messages"];
const MESSAGE_KEYS: &[&str] = &["role", "content"];

#[derive(Default)]
struct Fault(Option<BodyError>);

impl Fault {
    fn add(&mut self, error: BodyError) {
        match self.0 {
            Some(held) if (held as u8) <= (error as u8) => {}
            _ => self.0 = Some(error),
        }
    }
}

#[derive(Default)]
struct Read {
    tier: Option<WireTier>,
    effort: Option<Effort>,
    max_output_tokens: Option<u32>,
    messages: Option<Vec<Message>>,
    fault: Fault,
}

impl Read {
    fn finish(self, size: usize) -> Result<Body, BodyError> {
        let mut fault = self.fault;
        if size > MAX_BODY_BYTES {
            fault.add(BodyError::Size);
        }
        if let Some(error) = fault.0 {
            return Err(error);
        }
        match (
            self.tier,
            self.effort,
            self.max_output_tokens,
            self.messages,
        ) {
            (Some(tier), Some(effort), Some(max_output_tokens), Some(messages)) => Ok(Body {
                tier,
                effort,
                max_output_tokens,
                messages,
            }),
            _ => Err(BodyError::Key),
        }
    }
}

struct BodySeed;

impl<'de> DeserializeSeed<'de> for BodySeed {
    type Value = Read;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Read, D::Error> {
        deserializer.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for BodySeed {
    type Value = Read;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a body object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Read, A::Error> {
        let mut read = Read::default();
        let mut seen = [false; 4];
        while let Some(key) = map.next_key_seed(Keys(BODY_KEYS))? {
            let Some(slot) = key else {
                map.next_value::<IgnoredAny>()?;
                read.fault.add(BodyError::Key);
                continue;
            };
            if mem::replace(&mut seen[slot], true) {
                read.fault.add(BodyError::Key);
            }
            match slot {
                0 => match WireTier::parse(&map.next_value::<String>()?) {
                    Ok(tier) => read.tier = Some(tier),
                    Err(error) => read.fault.add(error),
                },
                1 => match Effort::parse(&map.next_value::<String>()?) {
                    Ok(effort) => read.effort = Some(effort),
                    Err(error) => read.fault.add(error),
                },
                2 => {
                    let count = map.next_value_seed(Count(u64::from(u32::MAX)))?;
                    let count = u32::try_from(count).map_err(A::Error::custom)?;
                    if !(1..=MAX_OUTPUT_TOKENS).contains(&count) {
                        read.fault.add(BodyError::Limit);
                    }
                    read.max_output_tokens = Some(count);
                }
                _ => {
                    let list = map.next_value_seed(MessagesSeed)?;
                    if !(1..=MAX_MESSAGES).contains(&list.count) {
                        read.fault.add(BodyError::Limit);
                    }
                    if let Some(error) = list.fault.0 {
                        read.fault.add(error);
                    }
                    read.messages = Some(list.messages);
                }
            }
        }
        if seen.contains(&false) {
            read.fault.add(BodyError::Key);
        }
        Ok(read)
    }
}

#[derive(Default)]
struct MessageList {
    messages: Vec<Message>,
    count: usize,
    fault: Fault,
}

struct MessagesSeed;

impl<'de> DeserializeSeed<'de> for MessagesSeed {
    type Value = MessageList;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<MessageList, D::Error> {
        deserializer.deserialize_seq(self)
    }
}

impl<'de> Visitor<'de> for MessagesSeed {
    type Value = MessageList;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("an array of messages")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<MessageList, A::Error> {
        let mut list = MessageList::default();
        while let Some(read) = seq.next_element_seed(MessageSeed)? {
            list.count += 1;
            match read {
                Ok(message) => list.messages.push(message),
                Err(error) => list.fault.add(error),
            }
        }
        Ok(list)
    }
}

struct MessageSeed;

impl<'de> DeserializeSeed<'de> for MessageSeed {
    type Value = Result<Message, BodyError>;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for MessageSeed {
    type Value = Result<Message, BodyError>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a message object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut role = None;
        let mut content = None;
        let mut seen = [false; 2];
        let mut fault = Fault::default();
        while let Some(key) = map.next_key_seed(Keys(MESSAGE_KEYS))? {
            let Some(slot) = key else {
                map.next_value::<IgnoredAny>()?;
                fault.add(BodyError::Key);
                continue;
            };
            if mem::replace(&mut seen[slot], true) {
                fault.add(BodyError::Key);
            }
            if slot == 0 {
                match Role::parse(&map.next_value::<String>()?) {
                    Ok(read) => role = Some(read),
                    Err(error) => fault.add(error),
                }
            } else {
                content = Some(map.next_value::<String>()?);
            }
        }
        if seen.contains(&false) {
            fault.add(BodyError::Key);
        }
        Ok(match (fault.0, role, content) {
            (None, Some(role), Some(content)) => Ok(Message { role, content }),
            (Some(error), _, _) => Err(error),
            _ => Err(BodyError::Key),
        })
    }
}
