use halvo_route_wire::{Body, BodyError, Effort, Message, Role, WireTier, MAX_BODY_BYTES};

fn body(messages: Vec<Message>, max_output_tokens: u32) -> Body {
    Body {
        tier: WireTier::Frontier,
        effort: Effort::High,
        max_output_tokens,
        messages,
    }
}

fn message(role: Role, content: &str) -> Message {
    Message {
        role,
        content: content.to_string(),
    }
}

#[test]
fn writes_the_keys_in_their_order_with_no_space() {
    let written = body(
        vec![
            message(Role::System, "s"),
            message(Role::User, "u"),
            message(Role::Assistant, "a b"),
        ],
        2048,
    )
    .to_bytes()
    .unwrap();
    assert_eq!(
        String::from_utf8(written).unwrap(),
        r#"{"tier":"frontier","effort":"high","max_output_tokens":2048,"messages":[{"role":"system","content":"s"},{"role":"user","content":"u"},{"role":"assistant","content":"a b"}]}"#
    );
}

#[test]
fn writes_a_string_as_serde_json_writes_one() {
    let every: Vec<char> = (0..=0x10_ffff_u32).filter_map(char::from_u32).collect();
    for chunk in every.chunks(16_000) {
        let text: String = chunk.iter().collect();
        let written = body(vec![message(Role::User, &text)], 1)
            .to_bytes()
            .unwrap();
        let want = format!(
            "{{\"tier\":\"frontier\",\"effort\":\"high\",\"max_output_tokens\":1,\"messages\":[{{\"role\":\"user\",\"content\":{}}}]}}",
            serde_json::to_string(&text).unwrap()
        );
        assert_eq!(written, want.into_bytes());
    }
}

#[test]
fn writes_the_short_escapes_and_lower_case_hex() {
    let written = body(
        vec![message(
            Role::User,
            "\"\\\u{8}\u{c}\n\r\t\u{0}\u{1f}\u{7f}/\u{2028}é",
        )],
        1,
    )
    .to_bytes()
    .unwrap();
    let text = String::from_utf8(written).unwrap();
    assert!(
        text.contains(r#""content":"\"\\\b\f\n\r\t\u0000\u001f"#),
        "{text}"
    );
    assert!(text.ends_with("\u{7f}/\u{2028}é\"}]}"), "{text}");
}

#[test]
fn checks_the_limits_before_it_writes() {
    let one = || vec![message(Role::User, "a")];
    assert_eq!(body(Vec::new(), 1).to_bytes(), Err(BodyError::Limit));
    assert_eq!(
        body(vec![message(Role::User, "a"); 65], 1).to_bytes(),
        Err(BodyError::Limit)
    );
    assert!(body(vec![message(Role::User, "a"); 64], 1)
        .to_bytes()
        .is_ok());
    assert_eq!(body(one(), 0).to_bytes(), Err(BodyError::Limit));
    assert_eq!(body(one(), 2049).to_bytes(), Err(BodyError::Limit));
    assert_eq!(body(one(), u32::MAX).to_bytes(), Err(BodyError::Limit));
    let frame = body(vec![message(Role::User, "")], 1)
        .to_bytes()
        .unwrap()
        .len();
    let fits = "a".repeat(MAX_BODY_BYTES - frame);
    assert_eq!(
        body(vec![message(Role::User, &fits)], 1)
            .to_bytes()
            .map(|bytes| bytes.len()),
        Ok(MAX_BODY_BYTES)
    );
    let over = "a".repeat(MAX_BODY_BYTES - frame + 1);
    assert_eq!(
        body(vec![message(Role::User, &over)], 1).to_bytes(),
        Err(BodyError::Size)
    );
    assert_eq!(
        body(vec![message(Role::User, &over)], 0).to_bytes(),
        Err(BodyError::Size)
    );

    let escapes = "\u{1}".repeat((MAX_BODY_BYTES - frame) / 6 + 1);
    assert_eq!(
        body(vec![message(Role::User, &escapes)], 1).to_bytes(),
        Err(BodyError::Size)
    );
}

#[test]
fn size_comes_first_in_to_bytes() {
    let messages = vec![message(Role::User, &"a".repeat(2_100)); 65];
    assert_eq!(body(messages, 1).to_bytes(), Err(BodyError::Size));
}

#[test]
fn a_written_body_is_read_back_as_the_same_body() {
    let original = body(
        vec![
            message(Role::System, "\u{0}\u{1f}\"\\ \u{10ffff}"),
            message(Role::User, ""),
            message(Role::Assistant, "\u{feff}x"),
        ],
        7,
    );
    let written = original.to_bytes().unwrap();
    assert_eq!(Body::parse(&written), Ok(original.clone()));
    assert_eq!(Body::parse(&written).unwrap().to_bytes(), Ok(written));
}

#[test]
fn the_written_form_is_never_longer_than_a_body_it_reads() {
    let readable = [
        r#"{ "tier":"frontier","effort":"high","max_output_tokens":1,"messages":[{"role":"user","content":"a"}]}"#,
        r#"{"tier":"frontier","effort":"high","max_output_tokens":1,"messages":[{"role":"user","content":"\u00e9\/\u001F\u000a\u0022\u005c\ud83d\ude00"}]}"#,
        r#"{"messages":[{"content":"a","role":"user"}],"max_output_tokens":1,"effort":"high","tier":"frontier"}"#,
    ];
    for text in readable {
        let read = Body::parse(text.as_bytes()).unwrap_or_else(|error| panic!("{error:?} {text}"));
        let written = read.to_bytes().unwrap();
        assert!(written.len() <= text.len(), "{text}");
        assert_ne!(written, text.as_bytes());
        assert_eq!(Body::parse(&written), Ok(read));
    }
}
