use serde_json::Value;

/// The bytes of a line of `vectors.jsonl` or `answers.jsonl`: its `body` as UTF-8, or its `hex`
/// decoded, where the line holds exactly one of the two keys and `hex` is an even number of the
/// digits `0` to `9` and `a` to `f`.
pub fn bytes_of(line: &Value) -> Vec<u8> {
    let name = &line["name"];
    match (line.get("body"), line.get("hex")) {
        (Some(Value::String(body)), None) => body.as_bytes().to_vec(),
        (None, Some(Value::String(digits))) => {
            assert!(
                digits.len().is_multiple_of(2)
                    && digits
                        .bytes()
                        .all(|digit| matches!(digit, b'0'..=b'9' | b'a'..=b'f')),
                "{name}"
            );
            (0..digits.len())
                .step_by(2)
                .map(|at| u8::from_str_radix(&digits[at..at + 2], 16).unwrap())
                .collect()
        }
        _ => panic!("{name}: a line holds body or hex"),
    }
}
