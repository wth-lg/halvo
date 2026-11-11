use super::Cause;
use serde::de::{DeserializeSeed, Deserializer, Error, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};
use std::fmt;

struct Strict;

impl<'de> DeserializeSeed<'de> for Strict {
    type Value = Value;

    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Value, D::Error> {
        d.deserialize_any(Strict)
    }
}

impl<'de> Visitor<'de> for Strict {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("JSON in which no object holds a key twice")
    }

    fn visit_unit<E: Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_bool<E: Error>(self, b: bool) -> Result<Value, E> {
        Ok(Value::Bool(b))
    }

    fn visit_i64<E: Error>(self, n: i64) -> Result<Value, E> {
        Ok(Value::Number(n.into()))
    }

    fn visit_u64<E: Error>(self, n: u64) -> Result<Value, E> {
        Ok(Value::Number(n.into()))
    }

    fn visit_f64<E: Error>(self, n: f64) -> Result<Value, E> {
        Number::from_f64(n)
            .map(Value::Number)
            .ok_or_else(|| E::custom("a number that JSON cannot hold"))
    }

    fn visit_str<E: Error>(self, s: &str) -> Result<Value, E> {
        Ok(Value::String(s.to_owned()))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut list = Vec::new();
        while let Some(v) = seq.next_element_seed(Strict)? {
            list.push(v);
        }
        Ok(Value::Array(list))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut object = Map::new();
        while let Some(key) = map.next_key::<String>()? {
            if object.contains_key(&key) {
                return Err(A::Error::custom("a key stands twice"));
            }
            let value = map.next_value_seed(Strict)?;
            object.insert(key, value);
        }
        Ok(Value::Object(object))
    }
}

pub(crate) fn parse(text: &str) -> Result<Value, Cause> {
    let mut d = serde_json::Deserializer::from_str(text);
    let value = Strict.deserialize(&mut d).map_err(|_| Cause::Malformed)?;
    d.end().map_err(|_| Cause::Malformed)?;
    Ok(value)
}
