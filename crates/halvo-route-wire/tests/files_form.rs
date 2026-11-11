use halvo_route_wire::{
    Effort, Reason, Role, WireTier, MAX_BODY_BYTES, MAX_MESSAGES, MAX_OUTPUT_TOKENS,
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
}
