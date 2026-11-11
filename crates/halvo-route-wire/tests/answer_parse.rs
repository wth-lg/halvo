use halvo_route_wire::{Answer, AnswerError, Reason};

const OK: &str = r#"{"result":"ok","content":"done","usage":{"input":10,"output":5},"cost":42}"#;

fn read(status: u16, text: &str) -> Result<Answer, AnswerError> {
    Answer::parse(status, text.as_bytes())
}

fn done() -> Answer {
    Answer::Ok {
        content: "done".to_string(),
        tokens_in: 10,
        tokens_out: 5,
        cost: 42,
    }
}

const WIRE: [&str; 8] = [
    "over-cap",
    "no-cap",
    "not-active",
    "pending",
    "closing",
    "no-such-account",
    "credential-mismatch",
    "other-arguments",
];

#[test]
fn closed_lists_have_their_words() {
    assert_eq!(
        Reason::ALL.map(Reason::as_str),
        [
            "over_cap",
            "no_cap",
            "not_active",
            "pending",
            "closing",
            "no_such_account",
            "credential_mismatch",
            "other_arguments"
        ]
    );
    assert_eq!(
        AnswerError::ALL.map(AnswerError::as_str),
        ["status", "malformed"]
    );
}

#[test]
fn a_success_is_read_with_its_four_values() {
    assert_eq!(read(200, OK), Ok(done()));
}

#[test]
fn a_success_is_read_in_any_key_order_with_white_space() {
    let other = " {\"cost\" : 42,\r\n\"usage\":{ \"output\":5 ,\"input\":10},\t\"content\":\"done\", \"result\":\"ok\"}\n";
    assert_eq!(read(200, other), Ok(done()));
}

#[test]
fn each_refusal_is_read_from_its_hyphen_form() {
    for (wire, reason) in WIRE.iter().zip(Reason::ALL) {
        let text = format!("{{\"result\":\"refused\",\"reason\":\"{wire}\"}}");
        assert_eq!(read(403, &text), Ok(Answer::Refused(reason)), "{wire}");
        let spaced = format!(" {{ \"reason\" : \"{wire}\" , \"result\" : \"refused\" }} ");
        assert_eq!(read(403, &spaced), Ok(Answer::Refused(reason)), "{wire}");
    }
}

#[test]
fn a_reason_off_the_list_or_in_this_files_form_is_malformed() {
    for reason in Reason::ALL {
        let text = format!(
            "{{\"result\":\"refused\",\"reason\":\"{}\"}}",
            reason.as_str()
        );
        let want = if reason.as_str().contains('_') {
            Err(AnswerError::Malformed)
        } else {
            Ok(Answer::Refused(reason))
        };
        assert_eq!(read(403, &text), want, "{}", reason.as_str());
    }
    for word in ["over-budget", "Over-cap", "", "over-cap ", "rate-limited"] {
        let text = format!("{{\"result\":\"refused\",\"reason\":\"{word}\"}}");
        assert_eq!(read(403, &text), Err(AnswerError::Malformed), "{word:?}");
    }
    assert_eq!(
        read(403, r#"{"result":"refused","reason":1}"#),
        Err(AnswerError::Malformed)
    );
}

#[test]
fn any_status_but_200_and_403_is_status() {
    for status in [
        0,
        100,
        201,
        204,
        400,
        401,
        404,
        409,
        429,
        500,
        502,
        503,
        u16::MAX,
    ] {
        assert_eq!(read(status, OK), Err(AnswerError::Status), "{status}");
        assert_eq!(
            read(status, "not json"),
            Err(AnswerError::Status),
            "{status}"
        );
    }
}

#[test]
fn a_result_that_does_not_fit_its_status_is_malformed() {
    assert_eq!(
        read(200, r#"{"result":"refused","reason":"over-cap"}"#),
        Err(AnswerError::Malformed)
    );
    assert_eq!(read(403, OK), Err(AnswerError::Malformed));
    assert_eq!(
        read(200, &OK.replace("\"ok\"", "\"OK\"")),
        Err(AnswerError::Malformed)
    );
}

#[test]
fn both_objects_are_closed_and_a_repeated_key_is_malformed() {
    let cases = [
        OK.replace("{\"result\"", "{\"model\":\"m\",\"result\""),
        OK.replace("\"output\":5", "\"output\":5,\"cached\":0"),
        OK.replace(",\"cost\":42", ""),
        OK.replace(",\"output\":5", ""),
        OK.replace("\"content\":\"done\",", ""),
        OK.replace("{\"result\"", "{\"result\":\"ok\",\"result\""),
        OK.replace("\"cost\":42", "\"cost\":42,\"cost\":42"),
        OK.replace("\"input\":10", "\"input\":10,\"input\":10"),
        OK.replace("\"input\":10", "\"\\u0069nput\":10,\"input\":10"),
        OK.replace("{\"input\":10,\"output\":5}", "[10,5]"),
        OK.replace("\"done\"", "{\"text\":\"done\"}"),
        OK.replace("\"done\"", "null"),
    ];
    for case in &cases {
        assert_eq!(read(200, case), Err(AnswerError::Malformed), "{case}");
    }
    let refusals = [
        r#"{"result":"refused","reason":"over-cap","detail":"x"}"#,
        r#"{"result":"refused","reason":"over-cap","reason":"over-cap"}"#,
        r#"{"result":"refused"}"#,
        r#"{"reason":"over-cap"}"#,
        r#"{"result":"refused","reason":"over-cap"} {}"#,
        r#"[{"result":"refused","reason":"over-cap"}]"#,
        "",
    ];
    for case in refusals {
        assert_eq!(read(403, case), Err(AnswerError::Malformed), "{case}");
    }
}

#[test]
fn a_count_not_written_with_digits_alone_is_malformed() {
    for spelling in [
        "-1",
        "1.5",
        "4e1",
        "42.0",
        "-0",
        "+4",
        "042",
        "18446744073709551616",
        "\"42\"",
    ] {
        let text = OK.replace("\"cost\":42", &format!("\"cost\":{spelling}"));
        assert_eq!(read(200, &text), Err(AnswerError::Malformed), "{spelling}");
    }
    let largest = OK
        .replace("\"cost\":42", "\"cost\":18446744073709551615")
        .replace("\"input\":10", "\"input\":0");
    assert_eq!(
        read(200, &largest),
        Ok(Answer::Ok {
            content: "done".to_string(),
            tokens_in: 0,
            tokens_out: 5,
            cost: u64::MAX
        })
    );
}

#[test]
fn content_is_read_after_decoding_and_text_alone_is_read() {
    let escaped = OK.replace("\"done\"", "\"d\\u00f6ne\\n\\ud83d\\ude00\"");
    assert_eq!(
        read(200, &escaped),
        Ok(Answer::Ok {
            content: "d\u{f6}ne\n\u{1f600}".to_string(),
            tokens_in: 10,
            tokens_out: 5,
            cost: 42
        })
    );
    assert_eq!(
        read(200, &OK.replace("\"done\"", "\"\\ud800\"")),
        Err(AnswerError::Malformed)
    );
    let mut bytes = OK.as_bytes().to_vec();
    bytes[27] = 0xff;
    assert_eq!(Answer::parse(200, &bytes), Err(AnswerError::Malformed));
}
