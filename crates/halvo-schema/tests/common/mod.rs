#![allow(dead_code)]

use std::fmt::Debug;

use serde::de::{DeserializeOwned, Deserializer, MapAccess, Visitor};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

pub const HEX_A: &str = "0123456789abcdef0123456789abcdef";
pub const HEX_B: &str = "fedcba9876543210fedcba9876543210";
pub const MAX63: u64 = (1 << 63) - 1;

pub struct Entries(pub Vec<(String, Box<RawValue>)>);

impl<'de> Deserialize<'de> for Entries {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Entries;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a JSON object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Entries, A::Error> {
                let mut out = Vec::new();
                while let Some(entry) = map.next_entry::<String, Box<RawValue>>()? {
                    out.push(entry);
                }
                Ok(Entries(out))
            }
        }
        d.deserialize_map(V)
    }
}

pub fn entries(line: &str) -> Vec<(String, Box<RawValue>)> {
    serde_json::from_str::<Entries>(line).expect(line).0
}

pub fn raw(text: &str) -> Box<RawValue> {
    RawValue::from_string(text.to_owned()).expect(text)
}

pub fn object(entries: &[(String, Box<RawValue>)]) -> String {
    let parts: Vec<String> = entries
        .iter()
        .map(|(k, v)| format!("{}:{}", serde_json::to_string(k).unwrap(), v.get()))
        .collect();
    format!("{{{}}}", parts.join(","))
}

pub fn with(line: &str, key: &str, value: &str) -> String {
    let mut e = entries(line);
    let slot = e.iter_mut().find(|(k, _)| k == key).expect(key);
    slot.1 = raw(value);
    object(&e)
}

pub fn refused<T: DeserializeOwned + Debug>(line: &str) {
    let read = serde_json::from_str::<T>(line);
    assert!(
        read.is_err(),
        "read although it must be refused: {line} -> {read:?}"
    );
}

pub fn read<T: DeserializeOwned>(line: &str) -> T {
    match serde_json::from_str::<T>(line) {
        Ok(value) => value,
        Err(e) => panic!("refused {line}: {e}"),
    }
}

pub fn round_trip<T: Serialize + DeserializeOwned>(line: &str) -> T {
    let value: T = read(line);
    assert_eq!(serde_json::to_string(&value).unwrap(), line);
    value
}

/// Reads the line, writes it back to the same bytes, and shows each key as required, unique and closed:
/// a missing key, a key that stands twice and an unknown key are refused, and the keys in reverse order are read.
pub fn assert_strict<T: Serialize + DeserializeOwned + Debug + PartialEq>(line: &str) {
    let value: T = round_trip(line);
    let e = entries(line);
    for i in 0..e.len() {
        let mut dropped = e.clone();
        dropped.remove(i);
        refused::<T>(&object(&dropped));
        let mut doubled = e.clone();
        doubled.insert(i + 1, e[i].clone());
        refused::<T>(&object(&doubled));
    }
    let mut extra = e.clone();
    extra.push(("unknown".to_owned(), raw("1")));
    refused::<T>(&object(&extra));
    let mut reversed = e.clone();
    reversed.reverse();
    let again: T = read(&object(&reversed));
    assert_eq!(again, value);
    assert_eq!(serde_json::to_string(&again).unwrap(), line);
}
