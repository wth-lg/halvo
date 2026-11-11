use std::collections::BTreeSet;

use halvo_route_wire::{Body, BodyError, MAX_BODY_BYTES, MAX_MESSAGES, MAX_OUTPUT_TOKENS};
use serde_json::Value;

mod common;

fn read_lines(name: &str) -> Vec<Value> {
    let path = format!(
        "{}/../../docs/r6/gateway/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"));
    assert!(text.ends_with('\n') && !text.contains('\r'), "{name}");
    text.lines()
        .map(|line| serde_json::from_str(line).unwrap_or_else(|error| panic!("{name}: {error}")))
        .collect()
}

fn word(value: &Value) -> &str {
    value.as_str().unwrap_or("")
}

#[test]
fn vectors_hold() {
    let vectors = read_lines("vectors.jsonl");
    assert!(!vectors.is_empty());
    let mut misses = Vec::new();
    for (at, vector) in vectors.iter().enumerate() {
        let name = word(&vector["name"]);
        assert_eq!(vector["n"].as_u64(), Some(at as u64 + 1), "{name}");
        let bytes = common::bytes_of(vector);
        let bytes = bytes.as_slice();
        let got = match Body::parse(bytes) {
            Ok(read) => match read.to_bytes() {
                Ok(written) if Body::parse(&written).as_ref() == Ok(&read) => (
                    "yes",
                    if written == bytes { "yes" } else { "no" },
                    Value::Null,
                ),
                _ => ("miss", "miss", Value::Null),
            },
            Err(error) => ("no", "no", Value::from(error.as_str())),
        };
        let stated = (
            word(&vector["valid"]),
            word(&vector["canon"]),
            vector["error"].clone(),
        );
        if got != stated {
            misses.push(format!("{name}: stated {stated:?}, got {got:?}"));
        }
    }
    assert!(misses.is_empty(), "{misses:#?}");
}

#[test]
fn vectors_cover_every_error_each_edge_and_each_form_rule() {
    let vectors = read_lines("vectors.jsonl");
    let stated = |name: &str| {
        let vector = vectors
            .iter()
            .find(|vector| vector["name"] == name)
            .unwrap_or_else(|| panic!("no vector {name}"));
        (
            word(&vector["valid"]).to_string(),
            word(&vector["canon"]).to_string(),
            vector["error"].as_str().map(str::to_string),
        )
    };
    let valid_canon = [
        "least",
        "largest",
        "three_roles",
        "unicode_text",
        "control_characters",
        "limit_largest",
        "sixty_four_messages",
        "largest_two_byte_text",
        "least_as_bytes",
    ];
    for name in valid_canon {
        assert_eq!(stated(name), ("yes".into(), "yes".into(), None), "{name}");
    }
    let valid_not_canon = [
        "space_between_tokens",
        "keys_in_other_order",
        "space_around_object",
        "white_space_of_each_kind",
        "escaped_key_name",
        "escaped_quote_long_form",
        "escaped_backslash_long_form",
        "long_form_of_short_escape",
        "upper_case_hex_escape",
        "escaped_solidus",
        "escaped_non_ascii",
        "escaped_surrogate_pair",
        "escaped_del",
        "escaped_line_separator",
    ];
    for name in valid_not_canon {
        assert_eq!(stated(name), ("yes".into(), "no".into(), None), "{name}");
    }
    let invalid = [
        ("on_device_tier", "tier"),
        ("unknown_key", "key"),
        ("key_twice", "key"),
        ("no_messages", "limit"),
        ("too_many_messages", "limit"),
        ("image_part", "malformed"),
        ("choices_key", "key"),
        ("limit_zero", "limit"),
        ("limit_above", "limit"),
        ("limit_negative", "malformed"),
        ("limit_fraction", "malformed"),
        ("limit_exponent", "malformed"),
        ("limit_one_point_zero", "malformed"),
        ("limit_minus_zero", "malformed"),
        ("one_byte_too_large", "size"),
        ("workflow_key", "key"),
        ("unknown_effort", "effort"),
        ("tool_role", "role"),
        ("limit_u32_max", "limit"),
        ("limit_above_u32", "malformed"),
        ("malformed_before_key", "malformed"),
        ("key_before_tier", "key"),
        ("tier_before_effort", "tier"),
        ("effort_before_role", "effort"),
        ("role_before_limit", "role"),
        ("size_before_limit", "size"),
        ("size_before_malformed", "size"),
        ("space_after_largest", "size"),
        ("message_key_before_role", "key"),
        ("missing_key_before_effort", "key"),
        ("missing_key_before_limit", "key"),
        ("key_twice_wrong_type", "malformed"),
        ("unknown_key_fraction", "key"),
        ("effort_number", "malformed"),
        ("role_number", "malformed"),
        ("two_byte_text_too_large", "size"),
        ("byte_order_mark_bytes", "malformed"),
        ("invalid_byte", "malformed"),
        ("overlong_form", "malformed"),
        ("encoded_surrogate", "malformed"),
        ("cut_sequence", "malformed"),
    ];
    for (name, error) in invalid {
        assert_eq!(
            stated(name),
            ("no".into(), "no".into(), Some(error.to_string())),
            "{name}"
        );
    }
    let errors: BTreeSet<&str> = vectors
        .iter()
        .filter_map(|vector| vector["error"].as_str())
        .collect();
    let all: BTreeSet<&str> = BodyError::ALL.iter().map(|error| error.as_str()).collect();
    assert_eq!(errors, all);
}

enum Edge {
    Bytes(usize),
    FewerCharacters(usize),
    Tokens(u64),
    Messages(usize),
}

fn edges() -> Vec<(&'static str, Edge)> {
    let tokens = u64::from(MAX_OUTPUT_TOKENS);
    let u32_max = u64::from(u32::MAX);
    vec![
        ("largest", Edge::Bytes(MAX_BODY_BYTES)),
        ("largest_two_byte_text", Edge::Bytes(MAX_BODY_BYTES)),
        ("one_byte_too_large", Edge::Bytes(MAX_BODY_BYTES + 1)),
        ("space_after_largest", Edge::Bytes(MAX_BODY_BYTES + 1)),
        ("two_byte_text_too_large", Edge::Bytes(MAX_BODY_BYTES + 1)),
        (
            "two_byte_text_too_large",
            Edge::FewerCharacters(MAX_BODY_BYTES),
        ),
        ("least", Edge::Tokens(1)),
        ("limit_zero", Edge::Tokens(0)),
        ("limit_largest", Edge::Tokens(tokens)),
        ("limit_above", Edge::Tokens(tokens + 1)),
        ("limit_u32_max", Edge::Tokens(u32_max)),
        ("limit_above_u32", Edge::Tokens(u32_max + 1)),
        ("least", Edge::Messages(1)),
        ("no_messages", Edge::Messages(0)),
        ("sixty_four_messages", Edge::Messages(MAX_MESSAGES)),
        ("too_many_messages", Edge::Messages(MAX_MESSAGES + 1)),
    ]
}

fn spelled_after(bytes: &[u8], key: &str) -> String {
    let text = String::from_utf8_lossy(bytes);
    let at = text.find(key).map_or(text.len(), |at| at + key.len());
    text[at..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect()
}

#[test]
fn each_edge_vector_holds_its_edge() {
    let vectors = read_lines("vectors.jsonl");
    let mut misses = Vec::new();
    for (name, edge) in edges() {
        let vector = vectors
            .iter()
            .find(|vector| vector["name"] == name)
            .unwrap_or_else(|| panic!("no vector {name}"));
        let bytes = common::bytes_of(vector);
        let holds = match edge {
            Edge::Bytes(count) => bytes.len() == count,
            Edge::FewerCharacters(count) => String::from_utf8_lossy(&bytes).chars().count() < count,
            Edge::Tokens(count) => {
                spelled_after(&bytes, "\"max_output_tokens\":") == count.to_string()
            }
            Edge::Messages(count) => {
                let body: Value = serde_json::from_slice(&bytes).unwrap();
                body["messages"].as_array().map(Vec::len) == Some(count)
            }
        };
        if !holds {
            misses.push(name);
        }
    }
    assert!(misses.is_empty(), "{misses:?}");
}
