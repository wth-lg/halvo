use std::collections::BTreeSet;

use halvo_route_wire::{Answer, AnswerError, Reason};
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

fn got(answer: Result<Answer, AnswerError>) -> [Value; 5] {
    match answer {
        Ok(Answer::Ok {
            content,
            tokens_in,
            tokens_out,
            cost,
        }) => [
            Value::from("ok"),
            Value::from(content),
            Value::from(tokens_in),
            Value::from(tokens_out),
            Value::from(cost),
        ],
        Ok(Answer::Refused(reason)) => [
            Value::from(reason.as_str()),
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
        ],
        Err(error) => [
            Value::from(error.as_str()),
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
        ],
    }
}

#[test]
fn answers_hold() {
    let answers = read_lines("answers.jsonl");
    assert!(!answers.is_empty());
    let mut misses = Vec::new();
    for (at, answer) in answers.iter().enumerate() {
        let name = answer["name"].as_str().unwrap_or("");
        assert_eq!(answer["n"].as_u64(), Some(at as u64 + 1), "{name}");
        let status = u16::try_from(answer["status"].as_u64().unwrap()).unwrap();
        let got = got(Answer::parse(status, &common::bytes_of(answer)));
        let stated =
            ["expect", "content", "tokens_in", "tokens_out", "cost"].map(|key| answer[key].clone());
        if got != stated {
            misses.push(format!("{name}: stated {stated:?}, got {got:?}"));
        }
    }
    assert!(misses.is_empty(), "{misses:#?}");
}

#[test]
fn answers_cover_each_case_of_t11_6() {
    let answers = read_lines("answers.jsonl");
    let expect = |name: &str| {
        answers
            .iter()
            .find(|answer| answer["name"] == name)
            .unwrap_or_else(|| panic!("no answer {name}"))["expect"]
            .as_str()
            .unwrap()
            .to_string()
    };
    assert_eq!(expect("ok"), "ok");
    assert_eq!(expect("ok_space_and_other_order"), "ok");
    assert_eq!(expect("status_500"), "status");
    assert_eq!(expect("unknown_reason"), "malformed");
    assert_eq!(expect("key_twice"), "malformed");
    assert_eq!(expect("refusal_under_200"), "malformed");
    assert_eq!(expect("usage_largest"), "ok");
    assert_eq!(expect("tokens_in_above_u64"), "malformed");
    assert_eq!(expect("tokens_out_above_u64"), "malformed");
    assert_eq!(expect("ok_as_bytes"), "ok");
    assert_eq!(expect("byte_order_mark_bytes"), "malformed");
    assert_eq!(expect("invalid_byte_content"), "malformed");
    assert_eq!(expect("usage_missing_key"), "malformed");
    let refusals: BTreeSet<&str> = answers
        .iter()
        .filter(|answer| answer["status"] == 403)
        .filter_map(|answer| answer["expect"].as_str())
        .collect();
    for reason in Reason::ALL {
        assert!(refusals.contains(reason.as_str()), "{}", reason.as_str());
    }
    let outcomes: BTreeSet<&str> = answers
        .iter()
        .filter_map(|answer| answer["expect"].as_str())
        .collect();
    for error in AnswerError::ALL {
        assert!(outcomes.contains(error.as_str()), "{}", error.as_str());
    }
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
fn each_edge_answer_holds_its_edge() {
    let largest = u64::MAX.to_string();
    let above = (u128::from(u64::MAX) + 1).to_string();
    let edges = [
        ("usage_largest", "\"input\":", &largest),
        ("usage_largest", "\"output\":", &largest),
        ("cost_largest", "\"cost\":", &largest),
        ("tokens_in_above_u64", "\"input\":", &above),
        ("tokens_out_above_u64", "\"output\":", &above),
        ("cost_above_u64", "\"cost\":", &above),
    ];
    let answers = read_lines("answers.jsonl");
    let mut misses = Vec::new();
    for (name, key, digits) in edges {
        let answer = answers
            .iter()
            .find(|answer| answer["name"] == name)
            .unwrap_or_else(|| panic!("no answer {name}"));
        if spelled_after(&common::bytes_of(answer), key) != *digits {
            misses.push(name);
        }
    }
    assert!(misses.is_empty(), "{misses:?}");
}
