use std::fmt;

use serde::de::{Deserialize, Deserializer, Error, MapAccess, SeqAccess, Visitor};

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Node {
    Null,
    Bool(bool),
    Count(u64),
    Text(String),
    List(Vec<Node>),
    Map(Vec<(String, Node)>),
}

impl Node {
    pub fn parse(text: &str) -> Option<Node> {
        let mut reader = serde_json::Deserializer::from_str(text);
        let node = Node::deserialize(&mut reader).ok()?;
        reader.end().ok()?;
        Some(node)
    }

    pub fn get(&self, key: &str) -> Option<&Node> {
        match self {
            Node::Map(entries) => entries
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    pub fn count(&self) -> Option<u64> {
        match self {
            Node::Count(value) => Some(*value),
            _ => None,
        }
    }
}

pub fn compact(line: &str) -> bool {
    let mut in_string = false;
    let mut escaped = false;
    for byte in line.bytes() {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
        } else if byte == b'"' {
            in_string = true;
        } else if matches!(byte, b' ' | b'\t' | b'\n' | b'\r') {
            return false;
        }
    }
    true
}

impl<'de> Deserialize<'de> for Node {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Node, D::Error> {
        deserializer.deserialize_any(NodeVisitor)
    }
}

struct NodeVisitor;

impl<'de> Visitor<'de> for NodeVisitor {
    type Value = Node;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("JSON with no fraction, no sign and no repeated key")
    }

    fn visit_unit<E: Error>(self) -> Result<Node, E> {
        Ok(Node::Null)
    }

    fn visit_bool<E: Error>(self, value: bool) -> Result<Node, E> {
        Ok(Node::Bool(value))
    }

    fn visit_u64<E: Error>(self, value: u64) -> Result<Node, E> {
        Ok(Node::Count(value))
    }

    fn visit_str<E: Error>(self, value: &str) -> Result<Node, E> {
        Ok(Node::Text(value.to_string()))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Node, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = seq.next_element()? {
            items.push(item);
        }
        Ok(Node::List(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Node, A::Error> {
        let mut entries: Vec<(String, Node)> = Vec::new();
        while let Some(key) = map.next_key::<String>()? {
            if entries.iter().any(|(name, _)| *name == key) {
                return Err(A::Error::custom("a repeated key"));
            }
            let value = map.next_value()?;
            entries.push((key, value));
        }
        Ok(Node::Map(entries))
    }
}
