use halvo_wire::request::Route;
use halvo_wire::types::{
    CapGetAnswer, CapSetAnswer, CheckoutAnswer, DeleteAnswer, ManageAnswer, Rejected, StatusAnswer,
};
use serde::de::{DeserializeOwned, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::fmt;

const SCHEMA: &str = include_str!("../../../schema/schema.v1.json");
const ROUTES: &str = include_str!("../../../schema/routes.v1.json");
const KEYWORDS: [&str; 8] = [
    "type",
    "properties",
    "required",
    "additionalProperties",
    "enum",
    "minimum",
    "maximum",
    "maxLength",
];
const DEPTH: usize = 16;

/// The status and the body of one answer of the backend, as the transport received them.
pub struct Reply {
    pub status: u16,
    pub body: Vec<u8>,
}

/// An answer that was read: the route's own answer on status 200, or `Rejected` on a 4xx.
#[derive(Debug)]
pub enum Reading<T> {
    Answer(T),
    Rejected,
}

/// Why no answer was read, which makes the call a failed call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unread {
    /// The status was neither 200 nor from 400 to 499, so the body was not parsed.
    Status(u16),
    /// The body is not an answer that the vendored schema and `results` admit.
    Malformed(Fault),
}

/// What makes a body malformed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// The body is not one JSON text in UTF-8.
    Json,
    /// A value has another type than the schema gives it (a null included), a key is
    /// missing, repeated or not in the schema, or a string is outside its enum.
    Shape,
    /// An integer lies outside its minimum and maximum, or a string is longer than its
    /// `maxLength` in characters.
    Bound,
    /// A number where the schema has an integer is written otherwise than as digits, with a
    /// minus sign in front where it is negative: with a fraction, an exponent or as `-0`.
    IntegerForm,
    /// The generated type refuses a body that the schema admits.
    Type,
    /// The keys are not exactly `result` and the keys that `results` lists for its value.
    Results,
    /// The answer type is not the one that the route names, or a vendored file cannot be read.
    Mapping,
}

/// A generated answer type and the name of its definition under `$defs` of the vendored schema.
pub trait RouteAnswer: DeserializeOwned {
    const DEF: &'static str;
}

impl RouteAnswer for CheckoutAnswer {
    const DEF: &'static str = "CheckoutAnswer";
}
impl RouteAnswer for StatusAnswer {
    const DEF: &'static str = "StatusAnswer";
}
impl RouteAnswer for CapGetAnswer {
    const DEF: &'static str = "CapGetAnswer";
}
impl RouteAnswer for CapSetAnswer {
    const DEF: &'static str = "CapSetAnswer";
}
impl RouteAnswer for ManageAnswer {
    const DEF: &'static str = "ManageAnswer";
}
impl RouteAnswer for DeleteAnswer {
    const DEF: &'static str = "DeleteAnswer";
}

/// Reads the answer of `route`. On status 200 the raw JSON is checked against the route's
/// answer schema in the vendored `schema.v1.json`, as that schema reads it and with the
/// integer form above, then parsed with the generated type `T`, then checked against
/// `results` of the vendored `routes.v1.json`. On a status from 400 to 499 the body must be
/// `Rejected` by the same two steps. On any other status nothing is parsed.
pub fn read<T: RouteAnswer>(route: &Route, reply: &Reply) -> Result<Reading<T>, Unread> {
    if T::DEF != route.answer {
        return Err(Unread::Malformed(Fault::Mapping));
    }
    match reply.status {
        200 => {
            let pairs = checked(&reply.body, route.answer)?;
            let answer = typed::<T>(&reply.body)?;
            results(route.name, &pairs).map_err(Unread::Malformed)?;
            Ok(Reading::Answer(answer))
        }
        400..=499 => {
            checked(&reply.body, "Rejected")?;
            typed::<Rejected>(&reply.body)?;
            Ok(Reading::Rejected)
        }
        other => Err(Unread::Status(other)),
    }
}

fn typed<T: DeserializeOwned>(body: &[u8]) -> Result<T, Unread> {
    serde_json::from_slice(body).map_err(|_| Unread::Malformed(Fault::Type))
}

enum Raw {
    Null,
    Bool,
    Digits(u64),
    Negative(i64),
    OtherNumber,
    Text(String),
    List,
    Object(Vec<(String, Raw)>),
}

impl<'de> Deserialize<'de> for Raw {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Raw, D::Error> {
        deserializer.deserialize_any(RawVisitor)
    }
}

struct RawVisitor;

impl<'de> Visitor<'de> for RawVisitor {
    type Value = Raw;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a JSON value")
    }

    fn visit_unit<E>(self) -> Result<Raw, E> {
        Ok(Raw::Null)
    }

    fn visit_bool<E>(self, _: bool) -> Result<Raw, E> {
        Ok(Raw::Bool)
    }

    fn visit_u64<E>(self, value: u64) -> Result<Raw, E> {
        Ok(Raw::Digits(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Raw, E> {
        Ok(u64::try_from(value).map_or(Raw::Negative(value), Raw::Digits))
    }

    fn visit_f64<E>(self, _: f64) -> Result<Raw, E> {
        Ok(Raw::OtherNumber)
    }

    fn visit_str<E>(self, value: &str) -> Result<Raw, E> {
        Ok(Raw::Text(value.to_owned()))
    }

    fn visit_string<E>(self, value: String) -> Result<Raw, E> {
        Ok(Raw::Text(value))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Raw, A::Error> {
        while seq.next_element::<Raw>()?.is_some() {}
        Ok(Raw::List)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Raw, A::Error> {
        let mut pairs = Vec::new();
        while let Some(pair) = map.next_entry::<String, Raw>()? {
            pairs.push(pair);
        }
        Ok(Raw::Object(pairs))
    }
}

fn checked(body: &[u8], def: &str) -> Result<Vec<(String, Raw)>, Unread> {
    let raw: Raw = serde_json::from_slice(body).map_err(|_| Unread::Malformed(Fault::Json))?;
    let schema: Value =
        serde_json::from_str(SCHEMA).map_err(|_| Unread::Malformed(Fault::Mapping))?;
    let defs = schema
        .get("$defs")
        .and_then(Value::as_object)
        .ok_or(Unread::Malformed(Fault::Mapping))?;
    let node = defs.get(def).ok_or(Unread::Malformed(Fault::Mapping))?;
    check(defs, node, &raw, 0).map_err(Unread::Malformed)?;
    match raw {
        Raw::Object(pairs) => Ok(pairs),
        _ => Err(Unread::Malformed(Fault::Shape)),
    }
}

fn check(defs: &Map<String, Value>, node: &Value, raw: &Raw, depth: usize) -> Result<(), Fault> {
    let node = node.as_object().ok_or(Fault::Mapping)?;
    if depth > DEPTH {
        return Err(Fault::Mapping);
    }
    if let Some(reference) = node.get("$ref") {
        let name = reference
            .as_str()
            .and_then(|text| text.strip_prefix("#/$defs/"))
            .ok_or(Fault::Mapping)?;
        if node.len() != 1 {
            return Err(Fault::Mapping);
        }
        let target = defs.get(name).ok_or(Fault::Mapping)?;
        return check(defs, target, raw, depth + 1);
    }
    if node.keys().any(|key| !KEYWORDS.contains(&key.as_str())) {
        return Err(Fault::Mapping);
    }
    match node.get("type").and_then(Value::as_str) {
        Some("object") => object(defs, node, raw, depth),
        Some("string") => string(node, raw),
        Some("integer") => integer(node, raw),
        _ => Err(Fault::Mapping),
    }
}

fn object(
    defs: &Map<String, Value>,
    node: &Map<String, Value>,
    raw: &Raw,
    depth: usize,
) -> Result<(), Fault> {
    if node.get("additionalProperties") != Some(&Value::Bool(false)) {
        return Err(Fault::Mapping);
    }
    let properties = node
        .get("properties")
        .and_then(Value::as_object)
        .ok_or(Fault::Mapping)?;
    let Raw::Object(pairs) = raw else {
        return Err(Fault::Shape);
    };
    let mut seen: Vec<&str> = Vec::with_capacity(pairs.len());
    for (key, value) in pairs {
        if seen.contains(&key.as_str()) {
            return Err(Fault::Shape);
        }
        seen.push(key);
        let property = properties.get(key).ok_or(Fault::Shape)?;
        check(defs, property, value, depth + 1)?;
    }
    let required = match node.get("required") {
        Some(list) => list.as_array().ok_or(Fault::Mapping)?.as_slice(),
        None => &[],
    };
    for name in required {
        let name = name.as_str().ok_or(Fault::Mapping)?;
        if !seen.contains(&name) {
            return Err(Fault::Shape);
        }
    }
    Ok(())
}

fn string(node: &Map<String, Value>, raw: &Raw) -> Result<(), Fault> {
    let Raw::Text(text) = raw else {
        return Err(Fault::Shape);
    };
    if let Some(choices) = node.get("enum") {
        let choices = choices.as_array().ok_or(Fault::Mapping)?;
        if !choices
            .iter()
            .any(|choice| choice.as_str() == Some(text.as_str()))
        {
            return Err(Fault::Shape);
        }
    }
    if let Some(most) = node.get("maxLength") {
        let most = most.as_u64().ok_or(Fault::Mapping)?;
        if text.chars().count() as u64 > most {
            return Err(Fault::Bound);
        }
    }
    Ok(())
}

fn integer(node: &Map<String, Value>, raw: &Raw) -> Result<(), Fault> {
    let value = match raw {
        Raw::Digits(value) => i128::from(*value),
        Raw::Negative(value) => i128::from(*value),
        Raw::OtherNumber => return Err(Fault::IntegerForm),
        _ => return Err(Fault::Shape),
    };
    if let Some(least) = node.get("minimum") {
        if value < bound(least)? {
            return Err(Fault::Bound);
        }
    }
    if let Some(most) = node.get("maximum") {
        if value > bound(most)? {
            return Err(Fault::Bound);
        }
    }
    Ok(())
}

fn bound(value: &Value) -> Result<i128, Fault> {
    if let Some(value) = value.as_u64() {
        return Ok(i128::from(value));
    }
    value.as_i64().map(i128::from).ok_or(Fault::Mapping)
}

fn results(route: &str, pairs: &[(String, Raw)]) -> Result<(), Fault> {
    let routes: Value = serde_json::from_str(ROUTES).map_err(|_| Fault::Mapping)?;
    let listed = routes
        .get("routes")
        .and_then(Value::as_array)
        .and_then(|routes| {
            routes
                .iter()
                .find(|entry| entry.get("name").and_then(Value::as_str) == Some(route))
        })
        .and_then(|entry| entry.get("results"))
        .and_then(Value::as_object)
        .ok_or(Fault::Mapping)?;
    let result = pairs
        .iter()
        .find_map(|(key, value)| match (key.as_str(), value) {
            ("result", Raw::Text(result)) => Some(result.as_str()),
            _ => None,
        })
        .ok_or(Fault::Results)?;
    let keys = listed
        .get(result)
        .and_then(Value::as_array)
        .ok_or(Fault::Results)?;
    let others: Vec<&str> = pairs
        .iter()
        .map(|(key, _)| key.as_str())
        .filter(|key| *key != "result")
        .collect();
    let same = others.len() == keys.len()
        && keys
            .iter()
            .all(|key| key.as_str().is_some_and(|key| others.contains(&key)));
    if same {
        Ok(())
    } else {
        Err(Fault::Results)
    }
}

#[cfg(test)]
mod tests;
