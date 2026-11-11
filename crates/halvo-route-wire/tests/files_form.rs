use halvo_route_wire::{
    BodyError, Effort, Reason, Role, WireTier, MAX_BODY_BYTES, MAX_MESSAGES, MAX_OUTPUT_TOKENS,
};

fn read(name: &str) -> String {
    let path = format!(
        "{}/../../docs/r6/gateway/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn words<const N: usize>(list: [&str; N]) -> String {
    list.map(|word| format!("\"{word}\"")).join(",")
}

#[test]
fn the_limits_file_holds_the_text_of_t11_4() {
    let row = |tier: WireTier| {
        format!(
            "{}\t{MAX_BODY_BYTES}\t{MAX_OUTPUT_TOKENS}\t120000\t90000\tprovisional\n",
            tier.as_str()
        )
    };
    let want = format!(
        "tier\tmax_body_bytes\tmax_output_tokens\thold_life_ms\tupstream_deadline_ms\tstatus\n{}{}",
        row(WireTier::LowCostHosted),
        row(WireTier::Frontier)
    );
    assert_eq!(read("tier-limits.tsv"), want);
    assert_eq!(
        want,
        "tier\tmax_body_bytes\tmax_output_tokens\thold_life_ms\tupstream_deadline_ms\tstatus\nlow_cost_hosted\t131072\t2048\t120000\t90000\tprovisional\nfrontier\t131072\t2048\t120000\t90000\tprovisional\n"
    );
}

#[test]
fn the_schema_is_the_body_of_t11_3_in_the_form_of_t11_6() {
    let want = format!(
        concat!(
            "{{\"$schema\":\"https://json-schema.org/draft/2020-12/schema\",\"type\":\"object\",\"properties\":{{",
            "\"tier\":{{\"type\":\"string\",\"enum\":[{}]}},",
            "\"effort\":{{\"type\":\"string\",\"enum\":[{}]}},",
            "\"max_output_tokens\":{{\"type\":\"integer\",\"minimum\":1,\"maximum\":{}}},",
            "\"messages\":{{\"type\":\"array\",\"items\":{{\"type\":\"object\",\"properties\":{{",
            "\"role\":{{\"type\":\"string\",\"enum\":[{}]}},",
            "\"content\":{{\"type\":\"string\",\"maxLength\":{}}}}},",
            "\"required\":[\"role\",\"content\"],\"additionalProperties\":false}}}}}},",
            "\"required\":[\"tier\",\"effort\",\"max_output_tokens\",\"messages\"],\"additionalProperties\":false}}\n"
        ),
        words(WireTier::ALL.map(WireTier::as_str)),
        words(Effort::ALL.map(Effort::as_str)),
        MAX_OUTPUT_TOKENS,
        words(Role::ALL.map(Role::as_str)),
        MAX_BODY_BYTES,
    );
    let text = read("body.schema.json");
    assert_eq!(text, want);
    for absent in [
        "$defs", "$ref", "minItems", "maxItems", "pattern", "oneOf", "anyOf", "allOf", "const",
    ] {
        assert!(!text.contains(absent), "{absent}");
    }
}

#[test]
fn no_line_of_the_vector_files_holds_a_unicode_line_break_raw() {
    for name in ["vectors.jsonl", "answers.jsonl"] {
        for (at, line) in read(name).split('\n').enumerate() {
            for raw in ['\u{85}', '\u{2028}', '\u{2029}'] {
                assert!(!line.contains(raw), "{name}:{} {raw:?}", at + 1);
            }
        }
    }
}

#[test]
fn request_md_states_the_names_and_the_limits() {
    let text = read("request.md");
    assert!(!text.contains('\r') && text.ends_with('\n'));
    let mut names = vec![
        "POST /v1/chat",
        "x-request-id",
        "tier",
        "effort",
        "max_output_tokens",
        "messages",
        "role",
        "content",
        "tier-limits.tsv",
        "vectors.jsonl",
        "answers.jsonl",
        "body.schema.json",
        "gateway-check",
    ];
    let reasons: Vec<String> = Reason::ALL
        .iter()
        .map(|reason| reason.as_str().replace('_', "-"))
        .collect();
    names.extend(reasons.iter().map(String::as_str));
    for name in names {
        assert!(text.contains(name), "{name}");
    }
    for number in [
        MAX_BODY_BYTES.to_string(),
        MAX_OUTPUT_TOKENS.to_string(),
        MAX_MESSAGES.to_string(),
    ] {
        assert!(text.contains(&number), "{number}");
    }
    for words in [
        "`size_before_limit`",
        "`size_before_malformed`",
        "without reading it",
        "bytes as received",
        "an overlong form and an encoded surrogate",
        "fatal: true, ignoreBOM: true",
        "does not stop at the first fault",
        "binds `max_output_tokens` and nothing else",
        "may have settled",
        "no `minItems` or `maxItems`",
        "no keyword bounds the bytes of the whole body",
        "never both and never neither",
        "ends at LF alone",
        "scripts/r6/ci.d/20-r6-gateway.sh",
    ] {
        assert!(text.contains(words), "{words}");
    }
    assert_eq!(text.matches("an allowance that the branch sets").count(), 2);
    assert!(!text.contains("two places"));
    let row = |error: &str| text.find(&format!("| `{error}` |")).unwrap();
    let rows: Vec<usize> = BodyError::ALL
        .iter()
        .map(|error| row(error.as_str()))
        .collect();
    assert!(rows.windows(2).all(|pair| pair[0] < pair[1]), "{rows:?}");
}
