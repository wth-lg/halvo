use halvo_route_wire::{
    Body, BodyError, Effort, Message, Role, WireTier, MAX_BODY_BYTES, MAX_MESSAGES,
    MAX_OUTPUT_TOKENS,
};

const LEAST: &str = r#"{"tier":"low_cost_hosted","effort":"low","max_output_tokens":1,"messages":[{"role":"user","content":"a"}]}"#;

fn parse(text: &str) -> Result<Body, BodyError> {
    Body::parse(text.as_bytes())
}

fn least() -> Body {
    Body {
        tier: WireTier::LowCostHosted,
        effort: Effort::Low,
        max_output_tokens: 1,
        messages: vec![Message {
            role: Role::User,
            content: "a".to_string(),
        }],
    }
}

fn with_count(spelling: &str) -> String {
    LEAST.replace(
        "\"max_output_tokens\":1",
        &format!("\"max_output_tokens\":{spelling}"),
    )
}

fn with_messages(n: usize) -> String {
    let list = vec![r#"{"role":"user","content":"a"}"#; n].join(",");
    LEAST.replace(r#"[{"role":"user","content":"a"}]"#, &format!("[{list}]"))
}

fn with_content_of(total: usize) -> String {
    let fill = total - LEAST.len() + 1;
    LEAST.replace(
        r#""content":"a""#,
        &format!("\"content\":\"{}\"", "a".repeat(fill)),
    )
}

#[test]
fn limits_are_the_constants_of_t11_4() {
    assert_eq!(MAX_BODY_BYTES, 131_072);
    assert_eq!(MAX_OUTPUT_TOKENS, 2_048);
    assert_eq!(MAX_MESSAGES, 64);
}

#[test]
fn closed_lists_have_their_words_and_parse_them() {
    assert_eq!(
        WireTier::ALL.map(WireTier::as_str),
        ["low_cost_hosted", "frontier"]
    );
    assert_eq!(Effort::ALL.map(Effort::as_str), ["low", "medium", "high"]);
    assert_eq!(Role::ALL.map(Role::as_str), ["system", "user", "assistant"]);
    assert_eq!(
        BodyError::ALL.map(BodyError::as_str),
        [
            "size",
            "malformed",
            "key",
            "tier",
            "effort",
            "role",
            "limit"
        ]
    );
    for tier in WireTier::ALL {
        assert_eq!(WireTier::parse(tier.as_str()), Ok(tier));
    }
    for effort in Effort::ALL {
        assert_eq!(Effort::parse(effort.as_str()), Ok(effort));
    }
    for role in Role::ALL {
        assert_eq!(Role::parse(role.as_str()), Ok(role));
    }
    for word in ["on_device", "Frontier", "frontier ", "", "low-cost-hosted"] {
        assert_eq!(WireTier::parse(word), Err(BodyError::Tier), "{word:?}");
    }
    for word in ["max", "Low", "none", "reasoning_level"] {
        assert_eq!(Effort::parse(word), Err(BodyError::Effort), "{word:?}");
    }
    for word in ["tool", "developer", "User", "function"] {
        assert_eq!(Role::parse(word), Err(BodyError::Role), "{word:?}");
    }
}

#[test]
fn least_body_is_read() {
    assert_eq!(parse(LEAST), Ok(least()));
}

#[test]
fn keys_in_any_order_and_white_space_between_tokens_are_read() {
    let other = r#"{"messages":[{"content":"a","role":"user"}],"max_output_tokens":1,"effort":"low","tier":"low_cost_hosted"}"#;
    assert_eq!(parse(other), Ok(least()));
    let spaced = " \t\r\n{ \"tier\" :\t\"low_cost_hosted\" ,\r\n\"effort\": \"low\", \"max_output_tokens\" : 1 , \"messages\" : [ { \"role\" : \"user\" , \"content\" : \"a\" } ] } \n";
    assert_eq!(parse(spaced), Ok(least()));
}

#[test]
fn escaped_names_and_words_are_read_after_decoding() {
    let escaped = LEAST
        .replace("\"tier\"", "\"\\u0074ier\"")
        .replace("\"low_cost_hosted\"", "\"low_cost_h\\u006fsted\"");
    assert_eq!(parse(&escaped), Ok(least()));
}

#[test]
fn unknown_missing_or_repeated_key_is_key() {
    let cases = [
        LEAST.replace("{\"tier\"", "{\"n\":1,\"tier\""),
        LEAST.replace("{\"tier\"", "{\"workflow\":\"w\",\"tier\""),
        LEAST.replace("{\"tier\"", "{\"stream\":false,\"tier\""),
        LEAST.replace("{\"tier\"", "{\"__proto__\":{},\"tier\""),
        LEAST.replace("\"effort\"", "\"reasoning_level\""),
        LEAST.replace("\"max_output_tokens\"", "\"max_tokens\""),
        LEAST.replace("\"tier\":\"low_cost_hosted\",", ""),
        LEAST.replace(",\"messages\":[{\"role\":\"user\",\"content\":\"a\"}]", ""),
        LEAST.replace("{\"tier\"", "{\"tier\":\"low_cost_hosted\",\"tier\""),
        LEAST.replace("{\"tier\"", "{\"t\\u0069er\":\"low_cost_hosted\",\"tier\""),
        LEAST.replace("{\"role\"", "{\"name\":\"x\",\"role\""),
        LEAST.replace(",\"content\":\"a\"", ""),
        LEAST.replace("{\"role\":\"user\"", "{\"role\":\"user\",\"role\":\"user\""),
        LEAST.replace("{\"role\":\"user\"", "{\"role\":\"user\",\"x\":[[[[1]]]]"),
    ];
    for case in &cases {
        assert_eq!(parse(case), Err(BodyError::Key), "{case}");
    }
}

#[test]
fn deep_value_of_an_unknown_key_is_key() {
    let deep = format!("{}1{}", "[".repeat(5000), "]".repeat(5000));
    let case = LEAST.replace("{\"tier\"", &format!("{{\"x\":{deep},\"tier\""));
    assert_eq!(parse(&case), Err(BodyError::Key));
}

#[test]
fn a_word_off_its_list_is_its_error() {
    assert_eq!(
        parse(&LEAST.replace("low_cost_hosted", "on_device")),
        Err(BodyError::Tier)
    );
    assert_eq!(
        parse(&LEAST.replace("\"low\"", "\"max\"")),
        Err(BodyError::Effort)
    );
    assert_eq!(
        parse(&LEAST.replace("\"user\"", "\"tool\"")),
        Err(BodyError::Role)
    );
}

#[test]
fn a_value_of_the_wrong_json_type_is_malformed() {
    let cases = [
        LEAST.replace("\"low_cost_hosted\"", "1"),
        LEAST.replace("\"low_cost_hosted\"", "null"),
        LEAST.replace("\"low\"", "[\"low\"]"),
        with_count("\"1\""),
        with_count("true"),
        LEAST.replace("[{\"role\":\"user\",\"content\":\"a\"}]", "{}"),
        LEAST.replace("[{\"role\":\"user\",\"content\":\"a\"}]", "[\"a\"]"),
        LEAST.replace("\"content\":\"a\"", "\"content\":{\"type\":\"image\"}"),
        LEAST.replace(
            "\"content\":\"a\"",
            "\"content\":[{\"type\":\"text\",\"text\":\"a\"}]",
        ),
        LEAST.replace("\"content\":\"a\"", "\"content\":null"),
        LEAST.replace("\"role\":\"user\"", "\"role\":1"),
    ];
    for case in &cases {
        assert_eq!(parse(case), Err(BodyError::Malformed), "{case}");
    }
}

#[test]
fn text_that_is_not_one_json_object_is_malformed() {
    let cases = [
        String::new(),
        " ".to_string(),
        "null".to_string(),
        format!("[{LEAST}]"),
        format!("{LEAST}{LEAST}"),
        format!("{LEAST},"),
        format!("\u{feff}{LEAST}"),
        LEAST.replace("}]}", "},]}"),
        LEAST.replace('"', "'"),
        format!("{}/**/}}", &LEAST[..LEAST.len() - 1]),
        LEAST.replace("\"a\"", "\"a\tb\""),
        LEAST.replace("\"a\"", "\"\\x\""),
    ];
    for case in &cases {
        assert_eq!(parse(case), Err(BodyError::Malformed), "{case:?}");
    }
}

#[test]
fn a_count_not_written_with_digits_alone_is_malformed() {
    for spelling in [
        "-1",
        "1.5",
        "1e3",
        "1E0",
        "1.0",
        "-0",
        "+1",
        "01",
        "4294967296",
        "18446744073709551616",
        "0x10",
    ] {
        assert_eq!(
            parse(&with_count(spelling)),
            Err(BodyError::Malformed),
            "{spelling}"
        );
    }
}

#[test]
fn output_limit_is_read_from_1_to_its_maximum() {
    assert_eq!(parse(&with_count("0")), Err(BodyError::Limit));
    assert!(parse(&with_count("1")).is_ok());
    assert_eq!(
        parse(&with_count("2048")).map(|body| body.max_output_tokens),
        Ok(2048)
    );
    assert_eq!(parse(&with_count("2049")), Err(BodyError::Limit));
    assert_eq!(parse(&with_count("4294967295")), Err(BodyError::Limit));
}

#[test]
fn messages_are_read_from_1_to_their_maximum() {
    assert_eq!(parse(&with_messages(0)), Err(BodyError::Limit));
    assert_eq!(parse(&with_messages(1)), Ok(least()));
    assert_eq!(
        parse(&with_messages(64)).map(|body| body.messages.len()),
        Ok(64)
    );
    assert_eq!(parse(&with_messages(65)), Err(BodyError::Limit));
}

#[test]
fn body_bytes_are_read_up_to_their_maximum() {
    let largest = with_content_of(MAX_BODY_BYTES);
    assert_eq!(largest.len(), MAX_BODY_BYTES);
    assert!(parse(&largest).is_ok());
    let over = with_content_of(MAX_BODY_BYTES + 1);
    assert_eq!(over.len(), MAX_BODY_BYTES + 1);
    assert_eq!(parse(&over), Err(BodyError::Size));
    let spaced = format!("{} ", with_content_of(MAX_BODY_BYTES));
    assert_eq!(parse(&spaced), Err(BodyError::Size));
}

#[test]
fn where_two_errors_fit_the_first_of_the_order_is_returned() {
    let over = with_content_of(MAX_BODY_BYTES + 1);
    let cases = [
        (
            LEAST.replace("{\"tier\":\"low_cost_hosted\"", "{\"x\":1,\"tier\":2"),
            BodyError::Malformed,
        ),
        (
            LEAST.replace(
                "{\"tier\":\"low_cost_hosted\"",
                "{\"tier\":1,\"tier\":\"x\"",
            ),
            BodyError::Malformed,
        ),
        (
            LEAST.replace(
                "{\"tier\":\"low_cost_hosted\"",
                "{\"x\":1,\"tier\":\"on_device\"",
            ),
            BodyError::Key,
        ),
        (
            LEAST
                .replace("low_cost_hosted", "on_device")
                .replace("\"low\"", "\"max\""),
            BodyError::Tier,
        ),
        (
            LEAST
                .replace("\"low\"", "\"max\"")
                .replace("\"user\"", "\"tool\""),
            BodyError::Effort,
        ),
        (
            with_count("0").replace("\"user\"", "\"tool\""),
            BodyError::Role,
        ),
        (
            over.replace("\"max_output_tokens\":1,", "\"max_output_tokens\":0,"),
            BodyError::Size,
        ),
        (over.replace("\"low\"", "\"low\"]"), BodyError::Size),
        (
            over.replace("{\"tier\"", "{\"x\":0,\"tier\""),
            BodyError::Size,
        ),
        (
            with_messages(65).replace("\"tier\":\"low_cost_hosted\",", ""),
            BodyError::Key,
        ),
    ];
    for (case, want) in &cases {
        assert_eq!(parse(case), Err(*want), "{}", &case[..case.len().min(120)]);
    }
}

#[test]
fn size_comes_first_and_needs_no_byte_past_the_limit() {
    let base = LEAST.replace("low_cost_hosted", "x");
    let fill = 1_048_576 - base.len() + 1;
    let big = base.replace(
        r#""content":"a""#,
        &format!("\"content\":\"{}\"", "a".repeat(fill)),
    );
    assert_eq!(big.len(), 1_048_576);
    assert_eq!(parse(&big), Err(BodyError::Size));
    for byte in [b'x', b'{', 0xff] {
        let bytes = vec![byte; MAX_BODY_BYTES + 1];
        assert_eq!(Body::parse(&bytes), Err(BodyError::Size), "{byte}");
    }
}

#[test]
fn strings_that_are_not_text_are_malformed() {
    let mut bad_content = LEAST.as_bytes().to_vec();
    let at = LEAST.find("\"a\"").unwrap() + 1;
    bad_content[at] = 0xff;
    assert_eq!(Body::parse(&bad_content), Err(BodyError::Malformed));
    let mut bad_unknown = LEAST
        .replace("{\"tier\"", "{\"x\":\"a\",\"tier\"")
        .into_bytes();
    bad_unknown[6] = 0xff;
    assert_eq!(Body::parse(&bad_unknown), Err(BodyError::Malformed));
    let cases = [
        LEAST.replace("\"a\"", "\"\\ud800\""),
        LEAST.replace("\"a\"", "\"\\udc00\""),
        LEAST.replace("\"a\"", "\"\\ud83d\\u0041\""),
        LEAST.replace("{\"tier\"", "{\"x\":\"\\ud800\",\"tier\""),
        LEAST.replace("{\"tier\"", "{\"x\":[\"\\udfff\"],\"tier\""),
        LEAST.replace("{\"tier\"", "{\"\\ud800\":1,\"tier\""),
    ];
    for case in &cases {
        assert_eq!(parse(case), Err(BodyError::Malformed), "{case}");
    }
    let pair = LEAST.replace("\"a\"", "\"\\ud83d\\ude00\"");
    assert_eq!(
        parse(&pair).map(|body| body.messages[0].content.clone()),
        Ok("\u{1f600}".to_string())
    );
}
