use halvo_schema::request::{
    decode_reply, write_reply, DecodeError, ErrorName, ANALYSIS_WRITE_MAX, INTERFACE_REQUEST_MAX,
    REPLY_MAX,
};
use halvo_schema::{EvidencePage, ReplyResult, RequestId, WorkflowsPage};
use serde_json::value::RawValue;

fn string_reply(id: Option<&RequestId>, len: usize) -> (Vec<u8>, String) {
    let envelope = write_reply(id, Ok(&ReplyResult::Null)).unwrap().len() - "null".len();
    let value = format!("\"{}\"", "a".repeat(len - envelope - 2));
    let raw = RawValue::from_string(value.clone()).unwrap();
    let bytes = write_reply(id, Ok(&ReplyResult::Raw(raw))).unwrap();
    assert_eq!(bytes.len(), len);
    (bytes, value)
}

fn refusal(bytes: &[u8]) -> Option<DecodeError> {
    decode_reply(bytes).err()
}

fn padded(head: &str, tail: &str, len: usize) -> Vec<u8> {
    let white = b" \t\n\r";
    let gap = len - head.len() - tail.len();
    let mut bytes = head.as_bytes().to_vec();
    bytes.extend((0..gap).map(|i| white[i % white.len()]));
    bytes.extend_from_slice(tail.as_bytes());
    assert_eq!(bytes.len(), len);
    bytes
}

fn accented(head: &str, len: usize) -> Vec<u8> {
    let room = len - head.len() - "\"\"}".len();
    let value = "é".repeat(room / 2) + &"a".repeat(room % 2);
    let text = format!("{head}\"{value}\"}}");
    assert_eq!(text.len(), len);
    assert!(text.chars().count() < REPLY_MAX);
    text.into_bytes()
}

#[test]
fn decode_reply_reads_a_reply_of_reply_max_bytes_and_refuses_one_byte_more_before_it_reads_anything(
) {
    let id = RequestId::from_random([0x40; 16]);
    let quoted = format!("\"{}\"", id.as_str());
    let too_large = DecodeError {
        id: None,
        error: ErrorName::TooLarge,
    };

    for at in [Some(&id), None] {
        let (bytes, value) = string_reply(at, REPLY_MAX);
        let reply = decode_reply(&bytes).expect("a compact reply of REPLY_MAX bytes is read");
        assert_eq!(reply.id.as_ref(), at);
        match reply.outcome {
            Ok(ReplyResult::Raw(raw)) => assert_eq!(raw.get(), value),
            other => panic!("a compact reply of REPLY_MAX bytes read as {other:?}"),
        }
        let longer = RawValue::from_string(format!("\"{}\"", "a".repeat(value.len() - 1))).unwrap();
        let written = write_reply(at, Ok(&ReplyResult::Raw(longer)));
        assert_eq!(
            written.as_ref().map(Vec::len).ok(),
            Some(REPLY_MAX + 1),
            "the writer applies no length bound: it writes a reply of REPLY_MAX + 1 bytes"
        );
        assert_eq!(
            refusal(&written.unwrap()),
            Some(too_large.clone()),
            "the decoder refuses the reply of REPLY_MAX + 1 bytes that the writer wrote"
        );
    }

    for (label, bytes) in [
        ("an empty input", &b""[..]),
        ("an input of one byte", &b"{"[..]),
    ] {
        assert_eq!(
            refusal(bytes),
            Some(DecodeError {
                id: None,
                error: ErrorName::Malformed,
            }),
            "{label} is malformed, not too large"
        );
    }

    let head = format!("{{\"v\":1,\"id\":{quoted},\"ok\":");
    let exact = padded(&head, "null}", REPLY_MAX);
    let reply = decode_reply(&exact).expect("a padded reply of REPLY_MAX bytes is read");
    assert_eq!(reply.id.as_ref(), Some(&id));
    assert!(matches!(reply.outcome, Ok(ReplyResult::Null)));
    let exact = padded(&format!("{head}null,\"x\":"), "1}", REPLY_MAX);
    assert_eq!(
        refusal(&exact),
        Some(DecodeError {
            id: Some(id.clone()),
            error: ErrorName::Malformed,
        }),
        "a reply of REPLY_MAX bytes with a key too many is read, and its id with it"
    );
    let exact = accented(&head, REPLY_MAX);
    let reply =
        decode_reply(&exact).expect("a reply of REPLY_MAX bytes in two-byte characters is read");
    assert_eq!(reply.id.as_ref(), Some(&id));
    assert!(matches!(reply.outcome, Ok(ReplyResult::Raw(_))));

    let above = REPLY_MAX + 1;
    let refused: Vec<(&str, Vec<u8>)> = vec![
        ("white space after ok", padded(&head, "null}", above)),
        (
            "white space before the object",
            padded("", &format!("{head}null}}"), above),
        ),
        (
            "white space after the object",
            padded(&format!("{head}null}}"), "", above),
        ),
        (
            "white space in an error reply",
            padded(
                &format!("{{\"v\":1,\"id\":{quoted},\"error\":"),
                "\"too_large\"}",
                above,
            ),
        ),
        (
            "a version that is not 1",
            padded(
                &format!("{{\"v\":2,\"id\":{quoted},\"ok\":"),
                "null}",
                above,
            ),
        ),
        (
            "fewer characters than the bound in more bytes",
            accented(&head, above),
        ),
        ("a compact reply with one byte set to NUL", {
            let mut bytes =
                format!("{head}\"{}\"}}", "a".repeat(above - head.len() - 3)).into_bytes();
            assert_eq!(bytes.len(), above);
            bytes[above / 2] = 0;
            bytes
        }),
        ("bytes that are no JSON", vec![b'x'; above]),
        ("bytes that are no UTF-8", vec![0xff; above]),
        (
            "four times the bound",
            padded(&head, "null}", 4 * REPLY_MAX),
        ),
    ];
    for (label, bytes) in refused {
        assert_eq!(refusal(&bytes), Some(too_large.clone()), "{label}");
    }
}

#[test]
fn the_reply_bound_is_255_kib() {
    assert_eq!(REPLY_MAX, 261_120);
    assert_eq!(REPLY_MAX, 255 * 1024);
    assert_eq!(ANALYSIS_WRITE_MAX, 65_500);
    assert_eq!(INTERFACE_REQUEST_MAX, 4_096);
}

#[test]
fn a_page_beside_a_request_id_holds_at_most_261067_bytes() {
    let workflows: WorkflowsPage = serde_json::from_str(r#"{"workflows":[],"next":null}"#).unwrap();
    let evidence: EvidencePage = serde_json::from_str(r#"{"evidence":[],"next":null}"#).unwrap();
    let id = RequestId::from_random([0x40; 16]);
    for (page, result) in [
        (
            serde_json::to_vec(&workflows).unwrap(),
            ReplyResult::Workflows(workflows),
        ),
        (
            serde_json::to_vec(&evidence).unwrap(),
            ReplyResult::Evidence(evidence),
        ),
    ] {
        let with_id = write_reply(Some(&id), Ok(&result)).unwrap();
        assert_eq!(with_id.len(), page.len() + 53);
        let null_id = write_reply(None, Ok(&result)).unwrap();
        assert_eq!(null_id.len(), page.len() + 23);
    }
    assert_eq!(REPLY_MAX - 53, 261_067);
}
