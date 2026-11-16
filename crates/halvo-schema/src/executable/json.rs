use std::collections::HashSet;
use std::fmt;
use std::hash::Hash;
use std::marker::PhantomData;
use std::num::NonZeroU32;

use serde::de::{Deserialize, Deserializer, MapAccess, Visitor};
use serde_json::value::RawValue;

use crate::category::SeedCategory;
use crate::ids::{AgentId, RequestId, WorkflowId};
use crate::record::Instant;

/// The cause of a refusal of this run's parsers: `malformed`, an input that is not the form of its contract, or
/// `length`, a playbook of more than 64 steps, a step whose compact JSON passes 2,048 bytes, a frame whose JSON
/// passes 65,536 bytes or a reply whose bytes as they arrive pass the bound of a reply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefusalCause {
    Malformed,
    Length,
}

impl RefusalCause {
    /// `malformed` or `length`.
    pub fn as_str(self) -> &'static str {
        match self {
            RefusalCause::Malformed => "malformed",
            RefusalCause::Length => "length",
        }
    }
}

/// The error of every parser of the agent-setup run: the cause, the Rust name of the type whose rule refused the
/// input, the key of the contract under which the refused value stood where there is one, and the token of the
/// rule. It never holds the input, an unknown key or a value. `Display` prints `<cause>: <type>: <key>: <rule>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refusal {
    cause: RefusalCause,
    type_name: &'static str,
    key: Option<&'static str>,
    rule: &'static str,
}

impl Refusal {
    pub(in crate::executable) fn malformed(type_name: &'static str, rule: &'static str) -> Self {
        Refusal {
            cause: RefusalCause::Malformed,
            type_name,
            key: None,
            rule,
        }
    }

    pub(in crate::executable) fn length(type_name: &'static str, rule: &'static str) -> Self {
        Refusal {
            cause: RefusalCause::Length,
            type_name,
            key: None,
            rule,
        }
    }

    pub(in crate::executable) fn at(mut self, key: &'static str) -> Self {
        if self.key.is_none() {
            self.key = Some(key);
        }
        self
    }

    /// `malformed` or `length`.
    pub fn cause(&self) -> RefusalCause {
        self.cause
    }

    /// The Rust name of the type whose rule refused the input.
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
            "{}: {}: {}: {}",
            self.cause.as_str(),
            self.type_name,
            self.key.unwrap_or("-"),
            self.rule
        )
    }
}

impl std::error::Error for Refusal {}

pub(in crate::executable) trait Arrives {
    const MAX: usize;
}

pub(in crate::executable) trait FromJson: Sized {
    fn from_json(raw: &RawValue) -> Result<Self, Refusal>;
}

pub(in crate::executable) fn whole<'a>(
    type_name: &'static str,
    bytes: &'a [u8],
) -> Result<&'a RawValue, Refusal> {
    serde_json::from_slice::<&RawValue>(bytes).map_err(|_| Refusal::malformed(type_name, "json"))
}

pub(in crate::executable) fn deserialize_raw<'de, D: Deserializer<'de>>(
    type_name: &'static str,
    deserializer: D,
) -> Result<Box<RawValue>, D::Error> {
    Box::<RawValue>::deserialize(deserializer)
        .map_err(|_| <D::Error as serde::de::Error>::custom(Refusal::malformed(type_name, "json")))
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

pub(in crate::executable) struct Obj<'a> {
    type_name: &'static str,
    entries: Vec<(String, &'a RawValue)>,
}

pub(in crate::executable) fn object<'a>(
    type_name: &'static str,
    raw: &'a RawValue,
) -> Result<Obj<'a>, Refusal> {
    if !raw.get().starts_with('{') {
        return Err(Refusal::malformed(type_name, "not_an_object"));
    }
    let entries = serde_json::from_str::<Entries<'a>>(raw.get())
        .map_err(|_| Refusal::malformed(type_name, "not_an_object"))?
        .0;
    let mut seen = HashSet::with_capacity(entries.len());
    for (key, _) in &entries {
        if !seen.insert(key.as_str()) {
            return Err(Refusal::malformed(type_name, "repeated_key"));
        }
    }
    Ok(Obj { type_name, entries })
}

impl<'a> Obj<'a> {
    pub(in crate::executable) fn known(&self, keys: &[&'static str]) -> Result<(), Refusal> {
        for (key, _) in &self.entries {
            if !keys.contains(&key.as_str()) {
                return Err(Refusal::malformed(self.type_name, "unknown_key"));
            }
        }
        Ok(())
    }

    pub(in crate::executable) fn exactly(&self, keys: &[&'static str]) -> Result<(), Refusal> {
        self.known(keys)?;
        for key in keys {
            if !self.has(key) {
                return Err(Refusal::malformed(self.type_name, "missing_key").at(key));
            }
        }
        Ok(())
    }

    pub(in crate::executable) fn has(&self, key: &str) -> bool {
        self.entries.iter().any(|(k, _)| k == key)
    }

    pub(in crate::executable) fn get(&self, key: &'static str) -> Result<&'a RawValue, Refusal> {
        self.entries
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| *v)
            .ok_or_else(|| Refusal::malformed(self.type_name, "missing_key").at(key))
    }

    pub(in crate::executable) fn read<T>(
        &self,
        key: &'static str,
        read: impl Fn(&RawValue) -> Result<T, Refusal>,
    ) -> Result<T, Refusal> {
        read(self.get(key)?).map_err(|r| r.at(key))
    }
}

pub(in crate::executable) fn string(
    type_name: &'static str,
    raw: &RawValue,
) -> Result<String, Refusal> {
    if !raw.get().starts_with('"') {
        return Err(Refusal::malformed(type_name, "not_a_string"));
    }
    serde_json::from_str::<String>(raw.get())
        .map_err(|_| Refusal::malformed(type_name, "not_a_string"))
}

pub(in crate::executable) fn opt<T>(
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

pub(in crate::executable) fn list<T>(
    read: impl Fn(&RawValue) -> Result<T, Refusal>,
) -> impl Fn(&RawValue) -> Result<Vec<T>, Refusal> {
    move |raw| {
        if !raw.get().starts_with('[') {
            return Err(Refusal::malformed("list", "not_a_list"));
        }
        let items: Vec<&RawValue> = serde_json::from_str(raw.get())
            .map_err(|_| Refusal::malformed("list", "not_a_list"))?;
        items.into_iter().map(&read).collect()
    }
}

pub(in crate::executable) fn each_once<T: Eq + Hash>(
    type_name: &'static str,
    items: &[T],
) -> Result<(), Refusal> {
    let mut seen = HashSet::with_capacity(items.len());
    for item in items {
        if !seen.insert(item) {
            return Err(Refusal::malformed(type_name, "repeated_name"));
        }
    }
    Ok(())
}

pub(in crate::executable) fn from_json<T: FromJson>(raw: &RawValue) -> Result<T, Refusal> {
    T::from_json(raw)
}

fn digits(type_name: &'static str, text: &str) -> Result<(), Refusal> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(Refusal::malformed(type_name, "not_a_count"));
    }
    Ok(())
}

impl FromJson for u32 {
    fn from_json(raw: &RawValue) -> Result<Self, Refusal> {
        let text = raw.get();
        digits("count", text)?;
        text.parse::<u32>()
            .map_err(|_| Refusal::malformed("count", "range"))
    }
}

impl FromJson for NonZeroU32 {
    fn from_json(raw: &RawValue) -> Result<Self, Refusal> {
        NonZeroU32::new(u32::from_json(raw).map_err(|r| Refusal {
            type_name: "number",
            ..r
        })?)
        .ok_or_else(|| Refusal::malformed("number", "zero"))
    }
}

pub(in crate::executable) fn instant(raw: &RawValue) -> Result<Instant, Refusal> {
    let text = raw.get();
    digits("Instant", text.strip_prefix('-').unwrap_or(text))?;
    if text == "-0" {
        return Err(Refusal::malformed("Instant", "minus_zero"));
    }
    text.parse::<i64>()
        .map(Instant)
        .map_err(|_| Refusal::malformed("Instant", "range"))
}

pub(in crate::executable) fn agent_id(raw: &RawValue) -> Result<AgentId, Refusal> {
    AgentId::parse(&string("AgentId", raw)?).map_err(|e| Refusal::malformed("AgentId", e.rule()))
}

pub(in crate::executable) fn workflow_id(raw: &RawValue) -> Result<WorkflowId, Refusal> {
    WorkflowId::parse(&string("WorkflowId", raw)?)
        .map_err(|e| Refusal::malformed("WorkflowId", e.rule()))
}

pub(in crate::executable) fn request_id(raw: &RawValue) -> Result<RequestId, Refusal> {
    RequestId::parse(&string("RequestId", raw)?)
        .map_err(|e| Refusal::malformed("RequestId", e.rule()))
}

pub(in crate::executable) fn seed_category(raw: &RawValue) -> Result<SeedCategory, Refusal> {
    SeedCategory::parse(&string("SeedCategory", raw)?)
        .map_err(|_| Refusal::malformed("SeedCategory", "unknown_name"))
}

pub(in crate::executable) trait WriteFields {
    fn write_fields<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error>;
}

pub(in crate::executable) struct Plain<'a, T>(pub(in crate::executable) &'a T);

impl<T: WriteFields> serde::Serialize for Plain<'_, T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.write_fields(serializer)
    }
}

thread_local! {
    static FIRST: std::cell::Cell<Option<Refusal>> = const { std::cell::Cell::new(None) };
}

pub(in crate::executable) fn ser_err<E: serde::ser::Error>(refusal: Refusal) -> E {
    FIRST.with(|first| {
        if first.get().is_none() {
            first.set(Some(refusal));
        }
    });
    E::custom(refusal)
}

pub(in crate::executable) fn write_json<T: serde::Serialize>(
    type_name: &'static str,
    value: &T,
) -> Result<String, Refusal> {
    FIRST.with(|first| first.set(None));
    serde_json::to_string(value).map_err(|_| {
        FIRST
            .with(|first| first.take())
            .unwrap_or(Refusal::malformed(type_name, "write"))
    })
}

macro_rules! object_type {
    (
        $(#[$meta:meta])*
        pub struct $name:ident $([$tkey:ident = $tval:literal])? {
            $( $(#[$fmeta:meta])* pub $field:ident : $ty:ty = $key:literal => $read:expr ),* $(,)?
        }
        rules($this:pat_param) $rules:block
        $(max_len($max:expr))?
        $(arrives($amax:expr, $arule:literal))?
    ) => {
        $(#[$meta])*
        pub struct $name {
            $( $(#[$fmeta])* pub $field: $ty, )*
        }

        impl $name {
            const KEYS: &'static [&'static str] = &[$(stringify!($tkey),)? $($key),*];

            /// Checks the rules of the contract that join keys of this object; a parser and `Serialize` call it.
            pub fn rules(&self) -> Result<(), $crate::executable::Refusal> {
                let $this = self;
                $rules
            }

            pub(in crate::executable) fn from_raw(
                raw: &serde_json::value::RawValue,
            ) -> Result<Self, $crate::executable::Refusal> {
                let obj = $crate::executable::json::object(stringify!($name), raw)?;
                obj.exactly(Self::KEYS)?;
                $(
                    let tag = obj.read(stringify!($tkey), |raw| {
                        $crate::executable::json::string(stringify!($name), raw)
                    })?;
                    if tag != $tval {
                        return Err($crate::executable::Refusal::malformed(
                            stringify!($name),
                            "tag",
                        )
                        .at(stringify!($tkey)));
                    }
                )?
                let value = $name {
                    $( $field: obj.read($key, $read)?, )*
                };
                value.rules()?;
                value.check_len()?;
                Ok(value)
            }

            fn check_len(&self) -> Result<(), $crate::executable::Refusal> {
                $(
                    let len = serde_json::to_vec(&$crate::executable::json::Plain(self))
                        .map_or(usize::MAX, |bytes| bytes.len());
                    if len > $max {
                        return Err($crate::executable::Refusal::length(stringify!($name), "json_length"));
                    }
                )?
                Ok(())
            }

            /// Reads one JSON value of this type and refuses every other input.
            $(
                #[doc = concat!(
                    "An input of more than `",
                    stringify!($amax),
                    "` bytes, counted on the bytes given before anything is read, white space and escapes ",
                    "included, is refused with the cause `length` and the rule `",
                    $arule,
                    "`."
                )]
            )?
            pub fn parse(bytes: &[u8]) -> Result<Self, $crate::executable::Refusal> {
                $(
                    if bytes.len() > $amax {
                        return Err($crate::executable::Refusal::length(stringify!($name), $arule));
                    }
                )?
                Self::from_raw($crate::executable::json::whole(stringify!($name), bytes)?)
            }

            /// The compact JSON of the value, or the refusal of a rule it breaks.
            pub fn to_json(&self) -> Result<String, $crate::executable::Refusal> {
                $crate::executable::json::write_json(stringify!($name), self)
            }
        }

        $(
            impl $crate::executable::json::Arrives for $name {
                const MAX: usize = $amax;
            }
        )?

        impl $crate::executable::json::WriteFields for $name {
            fn write_fields<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                use serde::ser::SerializeMap;
                let mut map = serializer.serialize_map(Some(Self::KEYS.len()))?;
                $( map.serialize_entry(stringify!($tkey), $tval)?; )?
                $( map.serialize_entry($key, &self.$field)?; )*
                map.end()
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                self.rules().map_err($crate::executable::json::ser_err)?;
                self.check_len().map_err($crate::executable::json::ser_err)?;
                $crate::executable::json::WriteFields::write_fields(self, serializer)
            }
        }
    };
}

pub(in crate::executable) use object_type;

macro_rules! names {
    (
        $(#[$meta:meta])*
        pub enum $name:ident {
            $( $(#[$vmeta:meta])* $variant:ident = $text:literal ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name {
            $( $(#[$vmeta])* $variant, )+
        }

        impl $name {
            /// Every name of the closed list, in the order of the contract.
            pub const ALL: &'static [$name] = &[ $( $name::$variant, )+ ];

            /// The name as it is written.
            pub fn as_str(self) -> &'static str {
                match self {
                    $( $name::$variant => $text, )+
                }
            }

            /// The value whose name is `s`; any other string is refused with `malformed`.
            pub fn parse(s: &str) -> Result<Self, $crate::executable::Refusal> {
                match s {
                    $( $text => Ok($name::$variant), )+
                    _ => Err($crate::executable::Refusal::malformed(stringify!($name), "unknown_name")),
                }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_str())
            }
        }

        impl $crate::executable::json::FromJson for $name {
            fn from_json(
                raw: &serde_json::value::RawValue,
            ) -> Result<Self, $crate::executable::Refusal> {
                $name::parse(&$crate::executable::json::string(stringify!($name), raw)?)
            }
        }
    };
}

pub(in crate::executable) use names;
