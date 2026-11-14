use std::collections::HashSet;
use std::fmt;
use std::marker::PhantomData;
use std::num::NonZeroU32;

use serde::de::{Deserialize, Deserializer, MapAccess, Visitor};
use serde_json::value::RawValue;

use crate::ids::ParseError;
use crate::record::Instant;

/// The error of every parser and writer of the interface run: the Rust name of the type whose rule refused the
/// value, the key of the contract under which the value stood where there is one, and the token of the rule. It
/// never holds the input, an unknown key or a value, and a handler answers it with the error `malformed`.
/// `Display` prints `<type>: <key>: <rule>`, with `-` where there is no key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refusal {
    type_name: &'static str,
    key: Option<&'static str>,
    rule: &'static str,
}

impl Refusal {
    pub(in crate::coordinator) fn new(type_name: &'static str, rule: &'static str) -> Self {
        Refusal {
            type_name,
            key: None,
            rule,
        }
    }

    pub(in crate::coordinator) fn at(mut self, key: &'static str) -> Self {
        if self.key.is_none() {
            self.key = Some(key);
        }
        self
    }

    /// The Rust name of the type whose rule refused the value.
    pub fn type_name(&self) -> &'static str {
        self.type_name
    }

    /// The key of the contract under which the refused value stood, or none for a whole input.
    pub fn key(&self) -> Option<&'static str> {
        self.key
    }

    /// The token of the rule: lower-case words joined by `_`.
    pub fn rule(&self) -> &'static str {
        self.rule
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {}: {}",
            self.type_name,
            self.key.unwrap_or("-"),
            self.rule
        )
    }
}

impl std::error::Error for Refusal {}

pub(in crate::coordinator) fn core(error: ParseError) -> Refusal {
    Refusal::new(error.type_name(), error.rule())
}

pub(in crate::coordinator) fn agent_setup(refusal: crate::executable::Refusal) -> Refusal {
    Refusal {
        type_name: refusal.type_name(),
        key: refusal.key(),
        rule: refusal.rule(),
    }
}

pub(in crate::coordinator) fn whole<'a>(
    type_name: &'static str,
    bytes: &'a [u8],
) -> Result<&'a RawValue, Refusal> {
    serde_json::from_slice::<&RawValue>(bytes).map_err(|_| Refusal::new(type_name, "json"))
}

pub(in crate::coordinator) fn deserialize<'de, D: Deserializer<'de>, T>(
    type_name: &'static str,
    deserializer: D,
    from_raw: fn(&RawValue) -> Result<T, Refusal>,
) -> Result<T, D::Error> {
    let raw = Box::<RawValue>::deserialize(deserializer)
        .map_err(|_| <D::Error as serde::de::Error>::custom(Refusal::new(type_name, "json")))?;
    from_raw(&raw).map_err(<D::Error as serde::de::Error>::custom)
}

pub(in crate::coordinator) fn write<T: serde::Serialize>(
    type_name: &'static str,
    value: &T,
) -> Result<String, Refusal> {
    serde_json::to_string(value).map_err(|_| Refusal::new(type_name, "write"))
}

struct Entries<'a>(Vec<(String, &'a RawValue)>);

impl<'de: 'a, 'a> Deserialize<'de> for Entries<'a> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V<'a>(PhantomData<&'a ()>);
        impl<'de: 'a, 'a> Visitor<'de> for V<'a> {
            type Value = Entries<'a>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a JSON object")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Entries<'a>, A::Error> {
                let mut entries = Vec::new();
                while let Some(entry) = map.next_entry::<String, &'de RawValue>()? {
                    entries.push(entry);
                }
                Ok(Entries(entries))
            }
        }
        deserializer.deserialize_map(V(PhantomData))
    }
}

pub(in crate::coordinator) struct Obj<'a> {
    type_name: &'static str,
    entries: Vec<(String, &'a RawValue)>,
}

pub(in crate::coordinator) fn object<'a>(
    type_name: &'static str,
    raw: &'a RawValue,
) -> Result<Obj<'a>, Refusal> {
    if !raw.get().starts_with('{') {
        return Err(Refusal::new(type_name, "not_an_object"));
    }
    let entries = serde_json::from_str::<Entries<'a>>(raw.get())
        .map_err(|_| Refusal::new(type_name, "not_an_object"))?
        .0;
    let mut seen = HashSet::with_capacity(entries.len());
    for (key, _) in &entries {
        if !seen.insert(key.as_str()) {
            return Err(Refusal::new(type_name, "repeated_key"));
        }
    }
    Ok(Obj { type_name, entries })
}

impl<'a> Obj<'a> {
    pub(in crate::coordinator) fn exactly(&self, keys: &[&'static str]) -> Result<(), Refusal> {
        for (key, _) in &self.entries {
            if !keys.contains(&key.as_str()) {
                return Err(Refusal::new(self.type_name, "unknown_key"));
            }
        }
        for key in keys {
            if !self.entries.iter().any(|(k, _)| k == key) {
                return Err(Refusal::new(self.type_name, "missing_key").at(key));
            }
        }
        Ok(())
    }

    pub(in crate::coordinator) fn read<T>(
        &self,
        key: &'static str,
        read: impl Fn(&RawValue) -> Result<T, Refusal>,
    ) -> Result<T, Refusal> {
        let raw = self
            .entries
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| *v)
            .ok_or_else(|| Refusal::new(self.type_name, "missing_key").at(key))?;
        read(raw).map_err(|r| r.at(key))
    }

    pub(in crate::coordinator) fn null_but(
        &self,
        among: &[&'static str],
        set: &[&'static str],
    ) -> Result<(), Refusal> {
        for key in among.iter().filter(|key| !set.contains(key)) {
            let null = self
                .entries
                .iter()
                .find(|(k, _)| k == key)
                .is_some_and(|(_, v)| v.get() == "null");
            if !null {
                return Err(Refusal::new(self.type_name, "not_null").at(key));
            }
        }
        Ok(())
    }
}

pub(in crate::coordinator) fn string(
    type_name: &'static str,
    raw: &RawValue,
) -> Result<String, Refusal> {
    if !raw.get().starts_with('"') {
        return Err(Refusal::new(type_name, "not_a_string"));
    }
    serde_json::from_str::<String>(raw.get()).map_err(|_| Refusal::new(type_name, "not_a_string"))
}

fn digits(type_name: &'static str, raw: &RawValue) -> Result<u32, Refusal> {
    let text = raw.get();
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(Refusal::new(type_name, "not_a_count"));
    }
    text.parse::<u32>()
        .map_err(|_| Refusal::new(type_name, "range"))
}

pub(in crate::coordinator) fn count(raw: &RawValue) -> Result<u32, Refusal> {
    digits("u32", raw)
}

pub(in crate::coordinator) fn number(raw: &RawValue) -> Result<NonZeroU32, Refusal> {
    NonZeroU32::new(digits("NonZeroU32", raw)?).ok_or(Refusal::new("NonZeroU32", "zero"))
}

pub(in crate::coordinator) fn instant(raw: &RawValue) -> Result<Instant, Refusal> {
    let text = raw.get();
    let magnitude = text.strip_prefix('-').unwrap_or(text);
    if magnitude.is_empty() || !magnitude.bytes().all(|b| b.is_ascii_digit()) {
        return Err(Refusal::new("Instant", "not_an_instant"));
    }
    if text == "-0" {
        return Err(Refusal::new("Instant", "minus_zero"));
    }
    text.parse::<i64>()
        .map(Instant)
        .map_err(|_| Refusal::new("Instant", "range"))
}

pub(in crate::coordinator) fn version(
    type_name: &'static str,
) -> impl Fn(&RawValue) -> Result<(), Refusal> {
    move |raw| {
        if raw.get() == "1" {
            Ok(())
        } else {
            Err(Refusal::new(type_name, "version"))
        }
    }
}

pub(in crate::coordinator) fn opt<T>(
    read: impl Fn(&RawValue) -> Result<T, Refusal>,
) -> impl Fn(&RawValue) -> Result<Option<T>, Refusal> {
    move |raw| {
        if raw.get() == "null" {
            Ok(None)
        } else {
            read(raw).map(Some)
        }
    }
}

pub(in crate::coordinator) fn list<T>(
    read: impl Fn(&RawValue) -> Result<T, Refusal>,
) -> impl Fn(&RawValue) -> Result<Vec<T>, Refusal> {
    move |raw| {
        if !raw.get().starts_with('[') {
            return Err(Refusal::new("list", "not_a_list"));
        }
        let items: Vec<&RawValue> =
            serde_json::from_str(raw.get()).map_err(|_| Refusal::new("list", "not_a_list"))?;
        items.into_iter().map(&read).collect()
    }
}
