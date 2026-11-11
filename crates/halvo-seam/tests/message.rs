use halvo_schema::{AnalysisRole, AnalysisWrite, Instant, Position, RequestId, StoreId};
use halvo_seam::message::{
    decode_client, decode_server, Cause, ClientMessage, KindFilter, OpenCursor, Refusal,
    ServerError, ServerMessage, TimeRange, DETAIL_MAX,
};
use halvo_seam::record::ServedKind;

const STORE: &str = "4a20c017c73d6c2ba45ee3b27fa724f4";

const EXAMPLE_BATCH: &str = r#"{"msg":"batch","store":"4a20c017c73d6c2ba45ee3b27fa724f4","watermark":{"seg":2,"idx":0},"records":[{"pos":{"seg":1,"idx":0},"record":{"v":1,"id":"019b78c9-0a80-7a1b-8f21-3b5d6c7e8f90","at":1767258000000000000,"seq":0,"kind":"foreground_app","app":"org.mozilla.firefox","p":{}}},{"pos":{"seg":1,"idx":1},"record":{"v":1,"id":"019b78c9-0c1c-7c3d-9a4e-1f20718293a4","at":1767258000412000000,"seq":1,"kind":"window_title","app":"org.mozilla.firefox","p":{"text":"Pay bill - Northgrid","cut":false,"why":"app_change","site":{"domain":"northgrid.test","source":"address_field"}}}}],"expiry":[{"seg":1,"at":1768467600000000000}],"next":{"seg":2,"idx":0}}"#;

fn client(text: &str) -> Result<ClientMessage, Refusal> {
    decode_client(text.as_bytes())
}

fn open(store: &str, position: &str, kinds: &str, range: &str) -> String {
    format!(
        r#"{{"msg":"open_cursor","store":{store},"position":{position},"kinds":{kinds},"range":{range}}}"#
    )
}

fn zero_open() -> String {
    open("null", r#"{"seg":0,"idx":0}"#, "null", "null")
}

fn server(text: &str) -> bool {
    decode_server(text.as_bytes()).is_ok()
}

#[test]
fn the_client_messages_decode_and_write_back_byte_for_byte() {
    let texts = [
        zero_open(),
        open(
            &format!("\"{STORE}\""),
            r#"{"seg":3,"idx":9}"#,
            r#"["keystroke","foreground_app"]"#,
            r#"{"start":-5,"end":10}"#,
        ),
        r#"{"msg":"read_batch"}"#.to_owned(),
        r#"{"msg":"close"}"#.to_owned(),
        r#"{"msg":"analysis_write","request":{"v":1,"id":"x"}}"#.to_owned(),
    ];
    for text in texts {
        let message = client(&text).unwrap_or_else(|e| panic!("{text}: {e:?}"));
        assert_eq!(message.to_payload(), text.as_bytes());
    }
}

#[test]
fn keys_stand_in_any_order_with_json_white_space_and_escaped_keys() {
    let text = "\r\n {\t\"range\" :null,\n\"kinds\":[ \"run_mark\" ],\"position\":{\"idx\":0,\r\"seg\":0},\"store\":null , \"\\u006dsg\":\"open_cursor\"}\n ";
    let message = client(text).unwrap();
    assert_eq!(
        message.to_payload(),
        open("null", r#"{"seg":0,"idx":0}"#, r#"["run_mark"]"#, "null").as_bytes()
    );
    assert!(matches!(
        client(r#"{"\u006dsg":"read_batch"}"#),
        Ok(ClientMessage::ReadBatch)
    ));
}

#[test]
fn a_key_twice_is_a_bad_frame_in_the_message_the_position_and_the_range() {
    let cases = [
        r#"{"msg":"read_batch","msg":"read_batch"}"#.to_owned(),
        r#"{"msg":"read_batch","\u006dsg":"close"}"#.to_owned(),
        zero_open().replace(r#""kinds":null"#, r#""kinds":null,"kinds":null"#),
        open("null", r#"{"seg":0,"idx":0,"idx":0}"#, "null", "null"),
        open("null", r#"{"seg":0,"seg":0,"idx":0}"#, "null", "null"),
        open(
            "null",
            r#"{"seg":0,"idx":0}"#,
            "null",
            r#"{"start":1,"end":5,"end":6}"#,
        ),
    ];
    for text in cases {
        assert_eq!(client(&text).err(), Some(Refusal::BadFrame), "{text}");
    }
}

#[test]
fn an_array_in_place_of_an_object_is_a_bad_frame() {
    let cases = [
        r#"["read_batch"]"#.to_owned(),
        r#"["open_cursor",null,{"seg":0,"idx":0},null,null]"#.to_owned(),
        open("null", "[0,0]", "null", "null"),
        open("null", r#"{"seg":0,"idx":0}"#, "null", "[1,5]"),
        r#"{"msg":"analysis_write","request":[1,"x"]}"#.to_owned(),
        r#"{"msg":"analysis_write","request":null}"#.to_owned(),
        r#"{"msg":"analysis_write","request":"{}"}"#.to_owned(),
        "[]".to_owned(),
        "null".to_owned(),
        "7".to_owned(),
    ];
    for text in cases {
        assert_eq!(client(&text).err(), Some(Refusal::BadFrame), "{text}");
    }
}

#[test]
fn a_number_outside_its_form_or_range_is_a_bad_frame() {
    let positions = [
        r#"{"seg":-0,"idx":0}"#,
        r#"{"seg":1e2,"idx":0}"#,
        r#"{"seg":1.0,"idx":0}"#,
        r#"{"seg":-1,"idx":0}"#,
        r#"{"seg":0,"idx":9223372036854775808}"#,
        r#"{"seg":18446744073709551616,"idx":0}"#,
        r#"{"seg":"0","idx":0}"#,
    ];
    let store = format!("\"{STORE}\"");
    for position in positions {
        let text = open(&store, position, "null", "null");
        assert_eq!(client(&text).err(), Some(Refusal::BadFrame), "{text}");
    }
    let ranges = [
        r#"{"start":-0,"end":5}"#,
        r#"{"start":1e2,"end":500}"#,
        r#"{"start":0,"end":9223372036854775808}"#,
        r#"{"start":-9223372036854775809,"end":0}"#,
        r#"{"start":0.5,"end":2}"#,
    ];
    for range in ranges {
        let text = open("null", r#"{"seg":0,"idx":0}"#, "null", range);
        assert_eq!(client(&text).err(), Some(Refusal::BadFrame), "{text}");
    }
    let edge = open(
        &store,
        r#"{"seg":9223372036854775807,"idx":9223372036854775807}"#,
        "null",
        r#"{"start":-9223372036854775808,"end":9223372036854775807}"#,
    );
    assert_eq!(client(&edge).unwrap().to_payload(), edge.as_bytes());
}

#[test]
fn a_missing_or_unknown_key_or_a_value_of_another_type_is_a_bad_frame() {
    let cases = [
        r#"{}"#.to_owned(),
        r#"{"msg":"open_cursor"}"#.to_owned(),
        zero_open().replace(r#","range":null"#, ""),
        zero_open().replace(r#""store":null,"#, ""),
        zero_open().replace(r#"}"#, r#","note":1}"#),
        open("null", r#"{"seg":0}"#, "null", "null"),
        open("null", r#"{"seg":0,"idx":0,"x":1}"#, "null", "null"),
        open("null", r#"{"seg":0,"idx":0}"#, "null", r#"{"start":1}"#),
        open(
            "null",
            r#"{"seg":0,"idx":0}"#,
            "null",
            r#"{"start":1,"end":2,"x":3}"#,
        ),
        open("null", "null", "null", "null"),
        open("7", r#"{"seg":0,"idx":0}"#, "null", "null"),
        open("null", r#"{"seg":0,"idx":0}"#, r#""keystroke""#, "null"),
        open("null", r#"{"seg":0,"idx":0}"#, "[7]", "null"),
        r#"{"msg":"read_batch","store":null}"#.to_owned(),
        r#"{"msg":"close","position":{"seg":0,"idx":0}}"#.to_owned(),
        r#"{"msg":"analysis_write"}"#.to_owned(),
        r#"{"msg":"analysis_write","request":{},"store":null}"#.to_owned(),
        r#"{"msg":"read_more"}"#.to_owned(),
        r#"{"msg":7}"#.to_owned(),
        r#"{"msg":"read_batch"} {"msg":"close"}"#.to_owned(),
        r#"{"msg":"read_batch"}x"#.to_owned(),
        r#"{"msg":"read_batch""#.to_owned(),
        String::new(),
    ];
    for text in cases {
        assert_eq!(client(&text).err(), Some(Refusal::BadFrame), "{text}");
    }
    assert_eq!(decode_client(&[0xff, 0xfe]).err(), Some(Refusal::BadFrame));
}

#[test]
fn a_message_of_c5_4_with_an_illegal_value_is_a_bad_request() {
    let nonzero = r#"{"seg":1,"idx":0}"#;
    let zero = r#"{"seg":0,"idx":0}"#;
    let cases = [
        open("null", nonzero, "null", "null"),
        open("null", r#"{"seg":0,"idx":1}"#, "null", "null"),
        open(
            &format!("\"{}\"", STORE.to_uppercase()),
            zero,
            "null",
            "null",
        ),
        open(&format!("\"{}\"", &STORE[1..]), zero, "null", "null"),
        open(r#""""#, zero, "null", "null"),
        open("null", zero, "[]", "null"),
        open("null", zero, r#"["keystroke","keystroke"]"#, "null"),
        open("null", zero, r#"["screen_frame"]"#, "null"),
        open("null", zero, r#"["system_audio"]"#, "null"),
        open(
            "null",
            zero,
            r#"["foreground_app","window_title","keystroke","pointer","run_mark","run_mark"]"#,
            "null",
        ),
        open("null", zero, "null", r#"{"start":5,"end":5}"#),
        open("null", zero, "null", r#"{"start":5,"end":4}"#),
    ];
    for text in cases {
        assert_eq!(client(&text).err(), Some(Refusal::BadRequest), "{text}");
    }
    let all_five = open(
        "null",
        zero,
        r#"["run_mark","pointer","keystroke","window_title","foreground_app"]"#,
        "null",
    );
    assert_eq!(client(&all_five).unwrap().to_payload(), all_five.as_bytes());
}

#[test]
fn the_typed_open_cursor_keeps_its_rules() {
    assert!(KindFilter::new(&[]).is_none());
    assert!(KindFilter::new(&[ServedKind::Pointer, ServedKind::Pointer]).is_none());
    let filter = KindFilter::new(&[ServedKind::Pointer, ServedKind::RunMark]).unwrap();
    assert!(filter.admits(halvo_seam::record::Kind::RunMark));
    assert!(!filter.admits(halvo_seam::record::Kind::ScreenFrame));
    assert!(TimeRange::new(Instant(5), Instant(5)).is_none());
    let range = TimeRange::new(Instant(5), Instant(9)).unwrap();
    assert!(range.contains(Instant(5)) && range.contains(Instant(8)));
    assert!(!range.contains(Instant(9)) && !range.contains(Instant(4)));
    let fresh = ClientMessage::OpenCursor(OpenCursor::fresh(None, None));
    assert_eq!(fresh.to_payload(), zero_open().as_bytes());
    let at = OpenCursor::at(
        StoreId::parse(STORE).unwrap(),
        Position { seg: 2, idx: 1 },
        Some(filter),
        Some(range),
    )
    .unwrap();
    assert_eq!(
        ClientMessage::OpenCursor(at).to_payload(),
        open(
            &format!("\"{STORE}\""),
            r#"{"seg":2,"idx":1}"#,
            r#"["pointer","run_mark"]"#,
            r#"{"start":5,"end":9}"#
        )
        .as_bytes()
    );
}

#[test]
fn the_analysis_write_carries_the_bytes_of_to_bytes_unchanged() {
    let write = AnalysisWrite::new(
        &AnalysisRole::claim(),
        RequestId::from_random([7; 16]),
        StoreId::parse(STORE).unwrap(),
    );
    let bytes = write.to_bytes().unwrap();
    let message = ClientMessage::analysis_write(&write).unwrap();
    let payload = message.to_payload();
    assert_eq!(payload.len(), bytes.len() + 35);
    let mut want = br#"{"msg":"analysis_write","request":"#.to_vec();
    want.extend_from_slice(&bytes);
    want.push(b'}');
    assert_eq!(payload, want);
    let ClientMessage::AnalysisWrite(request) = decode_client(&payload).unwrap() else {
        panic!("not an analysis write")
    };
    assert_eq!(request.get().as_bytes(), &bytes[..]);
    assert!(halvo_schema::request::decode(request.get().as_bytes()).is_ok());
}

#[test]
fn the_server_messages_of_c5_14_decode_and_write_back_byte_for_byte() {
    let texts = [
        EXAMPLE_BATCH.to_owned(),
        format!(
            r#"{{"msg":"batch","store":"{STORE}","watermark":{{"seg":2,"idx":0}},"records":[],"expiry":[],"next":{{"seg":0,"idx":0}}}}"#
        ),
        r#"{"msg":"closed"}"#.to_owned(),
        format!(
            r#"{{"msg":"error","cause":"store-changed","store":"{STORE}","detail":"store id differs"}}"#
        ),
        r#"{"msg":"error","cause":"bad-frame","store":null,"detail":""}"#.to_owned(),
        r#"{"msg":"error","cause":"bad-request","store":null,"detail":"x"}"#.to_owned(),
        r#"{"msg":"error","cause":"bad-position","store":null,"detail":"x"}"#.to_owned(),
        r#"{"msg":"error","cause":"store-error","store":null,"detail":"segment 3 line 0"}"#
            .to_owned(),
        r#"{"msg":"write_ack","reply":{"v":1,"id":null,"error":"unsupported"}}"#.to_owned(),
    ];
    for text in texts {
        let message = decode_server(text.as_bytes()).unwrap_or_else(|_| panic!("{text}"));
        assert_eq!(message.to_payload().unwrap(), text.as_bytes());
    }
    let ServerMessage::Batch(batch) = decode_server(EXAMPLE_BATCH.as_bytes()).unwrap() else {
        panic!("not a batch")
    };
    assert_eq!(batch.records.len(), 2);
    assert_eq!(batch.records[1].pos, Position { seg: 1, idx: 1 });
    assert_eq!(batch.expiry[0].at, Instant(1768467600000000000));
}

#[test]
fn a_server_message_outside_c5_5_is_refused() {
    let entry = r#"{"pos":{"seg":1,"idx":0},"record":{"v":1,"id":"019b78c9-0a80-7a1b-8f21-3b5d6c7e8f90","at":1,"seq":0,"kind":"foreground_app","app":null,"p":{}}}"#;
    let batch = |records: &str, expiry: &str| {
        format!(
            r#"{{"msg":"batch","store":"{STORE}","watermark":{{"seg":2,"idx":0}},"records":[{records}],"expiry":[{expiry}],"next":{{"seg":2,"idx":0}}}}"#
        )
    };
    assert!(server(&batch(entry, r#"{"seg":1,"at":5}"#)));
    let positional_entry = r#"[{"seg":1,"idx":0},{"v":1,"id":"019b78c9-0a80-7a1b-8f21-3b5d6c7e8f90","at":1,"seq":0,"kind":"foreground_app","app":null,"p":{}}]"#;
    let cases = [
        batch(positional_entry, ""),
        batch(&entry.replace(r#"{"seg":1,"idx":0}"#, "[1,0]"), ""),
        batch(&entry.replace(r#""p":{}"#, r#""p":[]"#), ""),
        batch(&entry.replace(r#""record":{"v":1,"id":"019b78c9-0a80-7a1b-8f21-3b5d6c7e8f90","at":1,"seq":0,"kind":"foreground_app","app":null,"p":{}}"#, r#""record":[1,"019b78c9-0a80-7a1b-8f21-3b5d6c7e8f90",1,0,"foreground_app",null,{}]"#), ""),
        batch(&entry.replace(r#""kind":"foreground_app""#, r#""kind":"camera""#), ""),
        batch(&entry.replacen(r#"{"pos":"#, r#"{"pos":{"seg":1,"idx":0},"pos":"#, 1), ""),
        batch(&entry.replacen(r#"{"pos":"#, r#"{"note":1,"pos":"#, 1), ""),
        batch(entry, "[1,5]"),
        batch(entry, r#"{"seg":1,"at":5,"at":5}"#),
        batch(entry, r#"{"seg":1,"at":5,"x":0}"#),
        batch(entry, r#"{"seg":-0,"at":5}"#),
        batch(entry, r#"{"seg":1,"at":1e2}"#),
        batch(entry, r#"{"seg":9223372036854775808,"at":5}"#),
        batch(entry, r#"{"seg":1}"#),
        batch("", "").replace(r#""watermark":{"seg":2,"idx":0}"#, r#""watermark":[2,0]"#),
        batch("", "").replace(r#""next":{"seg":2,"idx":0}"#, r#""next":null"#),
        batch("", "").replace(r#""records":[]"#, r#""records":{}"#),
        batch("", "").replace(r#""expiry":[],"#, ""),
        batch("", "").replace(r#""msg":"batch","#, r#""msg":"batch","msg":"batch","#),
        batch("", "").replace(&format!("\"{STORE}\""), "null"),
        batch("", "").replace(STORE, &STORE[1..]),
        r#"["batch"]"#.to_owned(),
        r#"{"msg":"closed","store":null}"#.to_owned(),
        r#"{"msg":"write_ack","reply":[1]}"#.to_owned(),
        r#"{"msg":"write_ack"}"#.to_owned(),
        r#"{"msg":"error","cause":"store-changed","store":null,"detail":"x"}"#.to_owned(),
        format!(r#"{{"msg":"error","cause":"bad-request","store":"{STORE}","detail":"x"}}"#),
        r#"{"msg":"error","cause":"too-late","store":null,"detail":"x"}"#.to_owned(),
        r#"{"msg":"error","cause":"bad-frame","detail":"x"}"#.to_owned(),
        r#"{"msg":"error","cause":"bad-frame","store":null}"#.to_owned(),
        format!(
            r#"{{"msg":"error","cause":"bad-frame","store":null,"detail":"{}"}}"#,
            "d".repeat(DETAIL_MAX + 1)
        ),
        r#"{"msg":"hello"}"#.to_owned(),
    ];
    for text in cases {
        assert!(!server(&text), "{text}");
    }
    let at_max = format!(
        r#"{{"msg":"error","cause":"bad-frame","store":null,"detail":"{}"}}"#,
        "d".repeat(DETAIL_MAX)
    );
    assert!(server(&at_max));
}

#[test]
fn a_server_error_keeps_its_rules() {
    assert!(ServerError::new(Cause::BadFrame, &"d".repeat(DETAIL_MAX + 1)).is_none());
    let changed = ServerError::new(
        Cause::StoreChanged(StoreId::parse(STORE).unwrap()),
        "store id differs",
    )
    .unwrap();
    assert_eq!(changed.cause().name(), "store-changed");
    assert_eq!(
        ServerMessage::Error(changed).to_payload().unwrap(),
        format!(r#"{{"msg":"error","cause":"store-changed","store":"{STORE}","detail":"store id differs"}}"#).as_bytes()
    );
    let names: Vec<&str> = [
        Cause::BadFrame,
        Cause::BadRequest,
        Cause::BadPosition,
        Cause::StoreError,
    ]
    .iter()
    .map(Cause::name)
    .collect();
    assert_eq!(
        names,
        ["bad-frame", "bad-request", "bad-position", "store-error"]
    );
}

#[test]
fn a_key_of_another_message_is_a_bad_frame_also_when_its_value_is_null() {
    let open_null_request =
        zero_open().replace(r#","range":null"#, r#","range":null,"request":null"#);
    let mut client_cases: Vec<String> = ["store", "position", "kinds", "range", "request"]
        .iter()
        .flat_map(|key| {
            [
                format!(r#"{{"msg":"read_batch","{key}":null}}"#),
                format!(r#"{{"msg":"close","{key}":null}}"#),
            ]
        })
        .collect();
    for key in ["store", "position", "kinds", "range"] {
        client_cases.push(format!(
            r#"{{"msg":"analysis_write","request":{{}},"{key}":null}}"#
        ));
    }
    client_cases.push(open_null_request);
    client_cases.push(r#"{"msg":"analysis_write","request":{},"request":null}"#.to_owned());
    for text in client_cases {
        assert_eq!(client(&text).err(), Some(Refusal::BadFrame), "{text}");
    }
    let keys = [
        "store",
        "watermark",
        "records",
        "expiry",
        "next",
        "reply",
        "cause",
        "detail",
    ];
    let empty_batch = format!(
        r#"{{"msg":"batch","store":"{STORE}","watermark":{{"seg":2,"idx":0}},"records":[],"expiry":[],"next":{{"seg":2,"idx":0}}"#
    );
    let mut server_cases: Vec<String> = keys
        .iter()
        .map(|key| format!(r#"{{"msg":"closed","{key}":null}}"#))
        .collect();
    for key in ["reply", "cause", "detail"] {
        server_cases.push(format!(r#"{empty_batch},"{key}":null}}"#));
    }
    for key in [
        "store",
        "watermark",
        "records",
        "expiry",
        "next",
        "cause",
        "detail",
    ] {
        server_cases.push(format!(
            r#"{{"msg":"write_ack","reply":{{"v":1}},"{key}":null}}"#
        ));
    }
    for key in ["watermark", "records", "expiry", "next", "reply"] {
        server_cases.push(format!(
            r#"{{"msg":"error","cause":"bad-frame","store":null,"detail":"x","{key}":null}}"#
        ));
    }
    server_cases.push(r#"{"msg":"error","cause":null,"store":null,"detail":"x"}"#.to_owned());
    server_cases
        .push(r#"{"msg":"error","cause":"bad-frame","store":null,"detail":null}"#.to_owned());
    server_cases.push(r#"{"msg":"write_ack","reply":null}"#.to_owned());
    for text in server_cases {
        assert!(!server(&text), "{text}");
    }
    assert!(server(&format!("{empty_batch}}}")));
}

#[test]
fn a_watermark_or_a_segment_ordinal_outside_the_form_of_c5_3_is_refused() {
    let entry = |seg: u64| {
        format!(
            r#"{{"pos":{{"seg":{seg},"idx":0}},"record":{{"v":1,"id":"019b78c9-0a80-7a1b-8f21-3b5d6c7e8f90","at":1,"seq":0,"kind":"foreground_app","app":null,"p":{{}}}}}}"#
        )
    };
    let batch = |watermark: &str, records: &str, expiry: &str, next: &str| {
        format!(
            r#"{{"msg":"batch","store":"{STORE}","watermark":{watermark},"records":[{records}],"expiry":[{expiry}],"next":{next}}}"#
        )
    };
    let w = r#"{"seg":2,"idx":0}"#;
    let refused = [
        batch(r#"{"seg":2,"idx":3}"#, "", "", r#"{"seg":2,"idx":3}"#),
        batch(r#"{"seg":0,"idx":0}"#, "", "", r#"{"seg":0,"idx":0}"#),
        batch(w, &entry(0), r#"{"seg":0,"at":5}"#, r#"{"seg":0,"idx":1}"#),
        batch(w, "", r#"{"seg":0,"at":5}"#, w),
    ];
    for text in refused {
        assert!(!server(&text), "{text}");
    }
    assert!(server(&batch(
        r#"{"seg":1,"idx":0}"#,
        "",
        "",
        r#"{"seg":0,"idx":0}"#
    )));
    assert!(server(&batch(w, &entry(1), r#"{"seg":1,"at":5}"#, w)));
}
