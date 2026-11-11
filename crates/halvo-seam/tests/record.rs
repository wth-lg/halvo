use std::io::Cursor;

use halvo_schema::{AppId, Domain, Instant, Text};
use halvo_seam::record::{
    Button, CaptureRecord, Edge, Kind, Line, Lines, Motion, Payload, Pointer, PointerDetail,
    RecordError, RecordId, Seq, Title, LINE_MAX,
};

const ID: &str = "019b78c9-0a80-7a1b-8f21-3b5d6c7e8f90";

const EXAMPLE: [&str; 6] = [
    r#"{"v":1,"id":"019b78c9-0a80-7a1b-8f21-3b5d6c7e8f90","at":1767258000000000000,"seq":0,"kind":"foreground_app","app":"org.mozilla.firefox","p":{}}"#,
    r#"{"v":1,"id":"019b78c9-0c1c-7c3d-9a4e-1f20718293a4","at":1767258000412000000,"seq":1,"kind":"window_title","app":"org.mozilla.firefox","p":{"text":"Pay bill - Northgrid","cut":false,"why":"app_change","site":{"domain":"northgrid.test","source":"address_field"}}}"#,
    r#"{"v":1,"id":"019b78c9-1638-70e4-b7d2-c5a6f8091b2c","at":1767258003000000000,"seq":2,"kind":"keystroke","app":"org.mozilla.firefox","p":{"class":"printable"}}"#,
    r#"{"v":1,"id":"019b78c9-1a20-75f6-a1c0-d9e8b7a6c5d4","at":1767258004000000000,"seq":3,"kind":"pointer","app":"org.mozilla.firefox","p":{"class":"click","button":"primary"}}"#,
    r#"{"v":1,"id":"019b78c9-1e08-729a-b3c4-d5e6f7081920","at":1767258005000000000,"seq":4,"kind":"pointer","app":"org.mozilla.firefox","p":{"class":"scroll","count":14}}"#,
    r#"{"v":1,"id":"019b78c9-3190-76d0-a1f2-a3b4c5d6e7f8","at":1767258010000000000,"seq":5,"kind":"run_mark","app":null,"p":{"edge":"start"}}"#,
];

fn record(kind: &str, app: &str, p: &str) -> String {
    format!(
        r#"{{"v":1,"id":"{ID}","at":1767258000000000000,"seq":0,"kind":"{kind}","app":{app},"p":{p}}}"#
    )
}

fn read(json: &str) -> Result<CaptureRecord, RecordError> {
    CaptureRecord::read(json.as_bytes())
}

fn refused(json: &str) -> (&'static str, &'static str) {
    match read(json) {
        Ok(_) => panic!("read took a malformed record: {json}"),
        Err(e) => (e.key(), e.rule()),
    }
}

fn line(json: &str) -> Vec<u8> {
    let mut out = json.as_bytes().to_vec();
    out.push(b'\n');
    out
}

#[test]
fn example_lines_of_c4_8_read_and_write_back_byte_for_byte() {
    for text in EXAMPLE {
        let rec = CaptureRecord::read_line(&line(text)).unwrap();
        assert_eq!(rec.to_line(), line(text));
        assert_eq!(rec.to_json(), text.as_bytes());
    }
    let title = CaptureRecord::read(EXAMPLE[1].as_bytes()).unwrap();
    assert_eq!(title.kind(), Kind::WindowTitle);
    assert_eq!(title.at(), Instant(1767258000412000000));
    assert_eq!(title.seq().get(), 1);
    assert_eq!(title.app().map(AppId::as_str), Some("org.mozilla.firefox"));
    let Payload::WindowTitle(t) = title.payload() else {
        panic!("not a title")
    };
    assert_eq!(t.text.as_str(), "Pay bill - Northgrid");
    assert_eq!(t.cut, Some(false));
    assert_eq!(t.site.as_ref().map(Domain::as_str), Some("northgrid.test"));
    let mark = CaptureRecord::read(EXAMPLE[5].as_bytes()).unwrap();
    assert_eq!(mark.payload(), &Payload::RunMark(Edge::Start));
    assert_eq!(mark.app(), None);
}

#[test]
fn every_payload_form_of_c4_3_is_read_and_written_back() {
    let forms = [
        ("foreground_app", r#""a.b""#, "{}"),
        ("foreground_app", "null", "{}"),
        (
            "window_title",
            r#""a.b""#,
            r#"{"text":"","cut":true,"why":"title_change"}"#,
        ),
        (
            "window_title",
            r#""a.b""#,
            r#"{"text":"x","cut":false,"why":"app_change","site":{"domain":"example.com","source":"address_field"}}"#,
        ),
        ("keystroke", r#""a.b""#, r#"{"class":"printable"}"#),
        ("keystroke", r#""a.b""#, r#"{"class":"editing"}"#),
        ("keystroke", r#""a.b""#, r#"{"class":"navigation"}"#),
        ("keystroke", r#""a.b""#, r#"{"class":"enter"}"#),
        ("keystroke", r#""a.b""#, r#"{"class":"tab"}"#),
        ("keystroke", r#""a.b""#, r#"{"class":"shortcut"}"#),
        ("keystroke", "null", r#"{"class":"other"}"#),
        (
            "pointer",
            r#""a.b""#,
            r#"{"class":"click","button":"primary"}"#,
        ),
        (
            "pointer",
            r#""a.b""#,
            r#"{"class":"click","button":"secondary"}"#,
        ),
        (
            "pointer",
            r#""a.b""#,
            r#"{"class":"click","button":"other"}"#,
        ),
        ("pointer", r#""a.b""#, r#"{"class":"scroll","count":0}"#),
        (
            "pointer",
            r#""a.b""#,
            r#"{"class":"drag","count":9223372036854775807}"#,
        ),
        ("pointer", r#""a.b""#, r#"{"class":"move","count":3}"#),
        ("run_mark", "null", r#"{"edge":"start"}"#),
        ("run_mark", "null", r#"{"edge":"end"}"#),
        ("screen_frame", r#""a.b""#, "{}"),
    ];
    for (kind, app, p) in forms {
        let text = record(kind, app, p);
        let rec = read(&text).unwrap_or_else(|e| panic!("{text}: {e}"));
        assert_eq!(rec.kind().as_str(), kind);
        assert_eq!(rec.to_json(), text.as_bytes(), "{text}");
    }
}

#[test]
fn reduced_records_are_read_and_written_back() {
    let forms = [
        ("window_title", r#"{"text":"x"}"#),
        (
            "window_title",
            r#"{"text":"x","site":{"domain":"example.com","source":"address_field"}}"#,
        ),
        ("keystroke", "{}"),
        ("pointer", "{}"),
        ("pointer", r#"{"class":"click"}"#),
        ("pointer", r#"{"class":"move"}"#),
        ("pointer", r#"{"button":"primary"}"#),
        ("pointer", r#"{"count":3}"#),
    ];
    for (kind, p) in forms {
        let text = record(kind, r#""a.b""#, p);
        let rec = read(&text).unwrap_or_else(|e| panic!("{text}: {e}"));
        assert_eq!(rec.to_json(), text.as_bytes(), "{text}");
    }
    let rec = read(&record("pointer", r#""a.b""#, r#"{"count":3}"#)).unwrap();
    let Payload::Pointer(Pointer::Unclassed(Some(PointerDetail::Count(n)))) = rec.payload() else {
        panic!("not an unclassed count")
    };
    assert_eq!(n.get(), 3);
}

#[test]
fn keys_stand_in_any_order_with_spaces_and_tabs_between_tokens() {
    let shuffled = " \t{ \"p\" :\t{\"why\":\"app_change\" , \"site\":{\"source\":\"address_field\",\"domain\":\"northgrid.test\"},\"cut\":false,\"text\":\"Pay bill - Northgrid\"}, \"app\":\"org.mozilla.firefox\",\"kind\":\"window_title\",\"seq\":1,\"at\":1767258000412000000,\"id\":\"019b78c9-0c1c-7c3d-9a4e-1f20718293a4\",\"v\":1 }\t ";
    let rec = CaptureRecord::read_line(&line(shuffled)).unwrap();
    assert_eq!(rec.to_json(), EXAMPLE[1].as_bytes());
}

#[test]
fn a_key_written_with_an_escape_is_that_key() {
    let escaped = EXAMPLE[0].replace(r#""at":"#, r#""\u0061t":"#);
    let rec = read(&escaped).unwrap();
    assert_eq!(rec.to_json(), EXAMPLE[0].as_bytes());
    let twin = EXAMPLE[0].replace(r#""seq":0"#, r#""seq":0,"\u0061t":5"#);
    assert_eq!(refused(&twin), ("at", "twice"));
    let in_p = record("keystroke", "null", r#"{"class":"tab","\u0063lass":"tab"}"#);
    assert_eq!(refused(&in_p), ("class", "twice"));
}

#[test]
fn a_key_twice_is_refused_in_the_record_in_p_and_in_site() {
    let top = EXAMPLE[0].replace(r#""seq":0"#, r#""seq":0,"kind":"foreground_app""#);
    assert_eq!(refused(&top), ("kind", "twice"));
    let top_p = EXAMPLE[0].replace(r#""p":{}"#, r#""p":{},"p":{}"#);
    assert_eq!(refused(&top_p), ("p", "twice"));
    let p = record("window_title", "null", r#"{"text":"a","text":"b"}"#);
    assert_eq!(refused(&p), ("text", "twice"));
    let site = record(
        "window_title",
        "null",
        r#"{"text":"a","site":{"domain":"example.com","domain":"example.com","source":"address_field"}}"#,
    );
    assert_eq!(refused(&site), ("domain", "twice"));
    let v = EXAMPLE[0].replace(r#"{"v":1,"#, r#"{"v":1,"v":1,"#);
    assert_eq!(refused(&v), ("v", "twice"));
}

#[test]
fn an_unknown_key_is_refused_and_named_by_the_object_that_holds_it() {
    let top = EXAMPLE[0].replace(r#""p":{}"#, r#""p":{},"note":1"#);
    assert_eq!(refused(&top), ("record", "unknown_key"));
    let p = record("keystroke", "null", r#"{"class":"tab","code":4}"#);
    assert_eq!(refused(&p), ("p", "unknown_key"));
    let site = record(
        "window_title",
        "null",
        r#"{"text":"a","site":{"domain":"example.com","source":"address_field","url":"x"}}"#,
    );
    assert_eq!(refused(&site), ("site", "unknown_key"));
}

#[test]
fn each_missing_key_is_named() {
    let keys = [
        ("v", r#""v":1,"#),
        ("id", r#""id":"019b78c9-0a80-7a1b-8f21-3b5d6c7e8f90","#),
        ("at", r#""at":1767258000000000000,"#),
        ("seq", r#""seq":0,"#),
        ("kind", r#""kind":"foreground_app","#),
        ("app", r#""app":"org.mozilla.firefox","#),
    ];
    for (key, text) in keys {
        assert_eq!(refused(&EXAMPLE[0].replace(text, "")), (key, "missing"));
    }
    let no_p = EXAMPLE[0].replace(r#","p":{}"#, "");
    assert_eq!(refused(&no_p), ("p", "missing"));
    let no_source = record(
        "window_title",
        "null",
        r#"{"text":"a","site":{"domain":"example.com"}}"#,
    );
    assert_eq!(refused(&no_source), ("source", "missing"));
    let no_domain = record(
        "window_title",
        "null",
        r#"{"text":"a","site":{"source":"address_field"}}"#,
    );
    assert_eq!(refused(&no_domain), ("domain", "missing"));
}

#[test]
fn an_array_in_place_of_an_object_is_refused() {
    let positional = r#"[1,"019b78c9-0a80-7a1b-8f21-3b5d6c7e8f90",1767258000000000000,0,"foreground_app","org.mozilla.firefox",{}]"#;
    assert_eq!(refused(positional).0, "record");
    let tag_first = r#"["foreground_app",1,"019b78c9-0a80-7a1b-8f21-3b5d6c7e8f90",1767258000000000000,0,"org.mozilla.firefox",{}]"#;
    assert_eq!(refused(tag_first).0, "record");
    assert!(CaptureRecord::read_line(&line(positional)).is_err());
    assert_eq!(
        refused(&record("keystroke", "null", r#"["printable"]"#)),
        ("p", "form")
    );
    assert_eq!(refused(&record("keystroke", "null", "[]")), ("p", "form"));
    assert_eq!(
        refused(&record("run_mark", "null", r#"["run_mark","start"]"#)),
        ("p", "form")
    );
    let site = record(
        "window_title",
        "null",
        r#"{"text":"a","site":["example.com","address_field"]}"#,
    );
    assert_eq!(refused(&site), ("site", "form"));
    assert_eq!(
        refused(&record("keystroke", "null", r#""printable""#)),
        ("p", "form")
    );
    assert_eq!(refused(&record("keystroke", "null", "null")), ("p", "form"));
    assert_eq!(refused("1").0, "record");
    assert_eq!(refused("null").0, "record");
}

#[test]
fn a_number_outside_the_decimal_integer_form_is_refused() {
    let cases = [
        ("v", r#""v":1,"#, r#""v":2,"#),
        ("v", r#""v":1,"#, r#""v":1.0,"#),
        ("v", r#""v":1,"#, r#""v":-0,"#),
        ("v", r#""v":1,"#, r#""v":1e0,"#),
        ("v", r#""v":1,"#, r#""v":"1","#),
        ("at", r#""at":1767258000000000000,"#, r#""at":1e2,"#),
        ("at", r#""at":1767258000000000000,"#, r#""at":-0,"#),
        ("at", r#""at":1767258000000000000,"#, r#""at":1.5,"#),
        (
            "at",
            r#""at":1767258000000000000,"#,
            r#""at":1767258000000000000.0,"#,
        ),
        (
            "at",
            r#""at":1767258000000000000,"#,
            r#""at":9223372036854775808,"#,
        ),
        (
            "at",
            r#""at":1767258000000000000,"#,
            r#""at":-9223372036854775809,"#,
        ),
        ("at", r#""at":1767258000000000000,"#, r#""at":"1","#),
        ("at", r#""at":1767258000000000000,"#, r#""at":null,"#),
        ("seq", r#""seq":0,"#, r#""seq":-1,"#),
        ("seq", r#""seq":0,"#, r#""seq":-0,"#),
        ("seq", r#""seq":0,"#, r#""seq":1e2,"#),
        ("seq", r#""seq":0,"#, r#""seq":9223372036854775808,"#),
    ];
    for (key, from, to) in cases {
        let text = EXAMPLE[0].replace(from, to);
        assert_eq!(refused(&text), (key, "form"), "{text}");
    }
    for count in ["-0", "1e2", "-1", "9223372036854775808", "2.0"] {
        let text = record(
            "pointer",
            "null",
            &format!(r#"{{"class":"move","count":{count}}}"#),
        );
        assert_eq!(refused(&text), ("count", "form"), "{text}");
    }
    let low = EXAMPLE[0].replace("1767258000000000000", "-9223372036854775808");
    assert_eq!(read(&low).unwrap().at(), Instant(i64::MIN));
    let high = EXAMPLE[0].replace(r#""seq":0"#, r#""seq":9223372036854775807"#);
    assert_eq!(read(&high).unwrap().seq().get(), 9223372036854775807);
}

#[test]
fn the_id_has_the_form_of_a_uuid_v7() {
    let bad = [
        "019B78C9-0A80-7A1B-8F21-3B5D6C7E8F90",
        "019b78c9-0a80-4a1b-8f21-3b5d6c7e8f90",
        "019b78c9-0a80-7a1b-8f21-3b5d6c7e8f9",
        "019b78c9-0a80-7a1b-8f21-3b5d6c7e8f900",
        "019b78c90a80-7a1b-8f21-3b5d6c7e8f90a",
        "019b78c9-0a80-7a1b-8f21_3b5d6c7e8f90",
        "019b78c9-0a80-7a1b-8f21-3b5d6c7e8g90",
        "",
    ];
    for id in bad {
        let text = EXAMPLE[0].replace(ID, id);
        assert_eq!(refused(&text), ("id", "form"), "{id}");
        assert!(RecordId::parse(id).is_none());
    }
    assert_eq!(
        refused(&EXAMPLE[0].replace(&format!("\"{ID}\""), "7")),
        ("id", "form")
    );
    assert_eq!(
        RecordId::parse(ID).map(|i| i.as_str().to_owned()),
        Some(ID.to_owned())
    );
}

#[test]
fn the_app_is_cut_to_its_file_name_and_kept_as_an_app_id() {
    let cases = [
        (
            r#""/Applications/Mail.app/Contents/MacOS/Mail""#,
            Some("Mail"),
        ),
        (
            r#""C:\\Program Files\\Office\\EXCEL.EXE""#,
            Some("EXCEL.EXE"),
        ),
        (r#""C:/Tools\\mixed/run.exe""#, Some("run.exe")),
        (r#""com.apple.mail""#, Some("com.apple.mail")),
        (r#""dir/""#, None),
        (r#"".""#, None),
        (r#""a/..""#, None),
        (r#""a\u0001b""#, None),
        ("null", None),
    ];
    for (app, want) in cases {
        let text = record("foreground_app", app, "{}");
        let rec = read(&text).unwrap_or_else(|e| panic!("{text}: {e}"));
        assert_eq!(rec.app().map(AppId::as_str), want, "{app}");
    }
    let long_name = format!("\"{}\"", "a".repeat(1024));
    assert_eq!(
        read(&record("foreground_app", &long_name, "{}"))
            .unwrap()
            .app(),
        None
    );
    let long_path = format!("\"{}/x.exe\"", "d".repeat(1018));
    assert_eq!(
        read(&record("foreground_app", &long_path, "{}"))
            .unwrap()
            .app()
            .map(AppId::as_str),
        Some("x.exe")
    );
    let cut = read(&record("foreground_app", r#""/opt/x/tool""#, "{}")).unwrap();
    assert!(String::from_utf8(cut.to_json())
        .unwrap()
        .contains(r#""app":"tool""#));
    let over = format!("\"{}\"", "a".repeat(1025));
    assert_eq!(
        refused(&record("foreground_app", &over, "{}")),
        ("app", "form")
    );
    assert_eq!(
        refused(&record("foreground_app", r#""""#, "{}")),
        ("app", "form")
    );
    assert_eq!(
        refused(&record("foreground_app", "7", "{}")),
        ("app", "form")
    );
    assert_eq!(
        refused(&record(
            "run_mark",
            r#""com.apple.mail""#,
            r#"{"edge":"start"}"#
        )),
        ("app", "run_mark")
    );
    assert_eq!(
        refused(&record("run_mark", r#""/""#, r#"{"edge":"end"}"#)),
        ("app", "run_mark")
    );
}

#[test]
fn a_kind_outside_c4_4_is_refused() {
    for kind in [
        "system_audio",
        "microphone",
        "camera",
        "Keystroke",
        "",
        "screen",
    ] {
        assert_eq!(
            refused(&record(kind, "null", "{}")),
            ("kind", "form"),
            "{kind}"
        );
    }
    assert_eq!(
        refused(&EXAMPLE[0].replace(r#""kind":"foreground_app""#, r#""kind":null"#)),
        ("kind", "form")
    );
}

#[test]
fn payload_keys_follow_the_kind() {
    let cases = [
        (
            "foreground_app",
            r#"{"class":"printable"}"#,
            ("class", "kind_key"),
        ),
        ("screen_frame", r#"{"edge":"start"}"#, ("edge", "kind_key")),
        ("keystroke", r#"{"text":"a"}"#, ("text", "kind_key")),
        ("keystroke", r#"{"class":"click"}"#, ("class", "form")),
        (
            "keystroke",
            r#"{"class":"printable","button":"primary"}"#,
            ("button", "kind_key"),
        ),
        ("pointer", r#"{"class":"printable"}"#, ("class", "form")),
        (
            "pointer",
            r#"{"class":"click","count":1}"#,
            ("count", "pairing"),
        ),
        (
            "pointer",
            r#"{"class":"scroll","button":"primary"}"#,
            ("button", "pairing"),
        ),
        (
            "pointer",
            r#"{"button":"primary","count":3}"#,
            ("count", "pairing"),
        ),
        (
            "pointer",
            r#"{"class":"click","button":"middle"}"#,
            ("button", "form"),
        ),
        (
            "pointer",
            r#"{"class":"click","text":"a"}"#,
            ("text", "kind_key"),
        ),
        ("run_mark", "{}", ("edge", "missing")),
        ("run_mark", r#"{"edge":"middle"}"#, ("edge", "form")),
        (
            "run_mark",
            r#"{"edge":"start","class":"tab"}"#,
            ("class", "kind_key"),
        ),
        ("run_mark", r#"{"edge":null}"#, ("edge", "form")),
        ("window_title", "{}", ("text", "missing")),
        (
            "window_title",
            r#"{"cut":true,"why":"app_change"}"#,
            ("text", "missing"),
        ),
        (
            "window_title",
            r#"{"text":"a","why":"other"}"#,
            ("why", "form"),
        ),
        (
            "window_title",
            r#"{"text":"a","cut":"true"}"#,
            ("cut", "form"),
        ),
        ("window_title", r#"{"text":"a","cut":1}"#, ("cut", "form")),
        ("window_title", r#"{"text":7}"#, ("text", "form")),
        (
            "window_title",
            r#"{"text":"a","edge":"start"}"#,
            ("edge", "kind_key"),
        ),
        (
            "window_title",
            r#"{"text":"a","site":{"domain":"example.com","source":"url"}}"#,
            ("source", "form"),
        ),
        (
            "window_title",
            r#"{"text":"a","site":{"domain":"com","source":"address_field"}}"#,
            ("domain", "form"),
        ),
        (
            "window_title",
            r#"{"text":"a","site":{"domain":"Example.com","source":"address_field"}}"#,
            ("domain", "form"),
        ),
        (
            "window_title",
            r#"{"text":"a","site":{"domain":"www.example.com","source":"address_field"}}"#,
            ("domain", "form"),
        ),
        (
            "window_title",
            r#"{"text":"a","site":null}"#,
            ("site", "form"),
        ),
    ];
    for (kind, p, want) in cases {
        let text = record(kind, "null", p);
        assert_eq!(refused(&text), want, "{text}");
    }
    let text_1024 = format!(r#"{{"text":"{}"}}"#, "é".repeat(512));
    assert!(read(&record("window_title", "null", &text_1024)).is_ok());
    let text_1025 = format!(r#"{{"text":"{}x"}}"#, "é".repeat(512));
    assert_eq!(
        refused(&record("window_title", "null", &text_1025)),
        ("text", "form")
    );
    let domain_254 = format!("{}comx", "a.".repeat(125));
    assert_eq!(domain_254.len(), 254);
    let site =
        format!(r#"{{"text":"a","site":{{"domain":"{domain_254}","source":"address_field"}}}}"#);
    assert_eq!(
        refused(&record("window_title", "null", &site)),
        ("domain", "form")
    );
}

#[test]
fn the_line_reader_keeps_the_rules_of_c4_8() {
    let ok = line(EXAMPLE[0]);
    assert!(CaptureRecord::read_line(&ok).is_ok());
    let line_err = |bytes: &[u8]| {
        let e = CaptureRecord::read_line(bytes).unwrap_err();
        (e.key(), e.rule())
    };
    assert_eq!(line_err(b"\n"), ("line", "empty"));
    assert_eq!(line_err(b""), ("line", "no_lf"));
    assert_eq!(line_err(EXAMPLE[0].as_bytes()), ("line", "no_lf"));
    let mut cr = EXAMPLE[0].as_bytes().to_vec();
    cr.extend_from_slice(b"\r\n");
    assert_eq!(line_err(&cr), ("line", "cr"));
    let cr_between = format!("{}\n", EXAMPLE[0].replace(r#","seq""#, "\r,\"seq\""));
    assert_eq!(line_err(cr_between.as_bytes()), ("line", "cr"));
    let mut bom = vec![0xEF, 0xBB, 0xBF];
    bom.extend_from_slice(&ok);
    assert_eq!(line_err(&bom), ("line", "bom"));
    let feff_inside = record("window_title", "null", "{\"text\":\"\u{feff}a\"}");
    assert!(CaptureRecord::read_line(&line(&feff_inside)).is_ok());
    let escaped_cr = record("window_title", "null", r#"{"text":"a\r\nb"}"#);
    let rec = CaptureRecord::read_line(&line(&escaped_cr)).unwrap();
    assert_eq!(rec.to_json(), escaped_cr.as_bytes());
    let trailing = format!("{} x\n", EXAMPLE[0]);
    assert_eq!(line_err(trailing.as_bytes()), ("record", "json"));
    let two = format!("{}{}\n", EXAMPLE[0], EXAMPLE[0]);
    assert_eq!(line_err(two.as_bytes()), ("record", "json"));
    let leading = format!(" \t {}\t \n", EXAMPLE[0]);
    assert!(CaptureRecord::read_line(leading.as_bytes()).is_ok());
    let mut at_bound = " ".repeat(LINE_MAX - 1 - EXAMPLE[0].len());
    at_bound.push_str(EXAMPLE[0]);
    at_bound.push('\n');
    assert_eq!(at_bound.len(), LINE_MAX);
    assert!(CaptureRecord::read_line(at_bound.as_bytes()).is_ok());
    let over = format!(" {at_bound}");
    assert_eq!(line_err(over.as_bytes()), ("line", "length"));
    let mut lf_inside = ok.clone();
    lf_inside.extend_from_slice(&ok);
    assert_eq!(line_err(&lf_inside), ("line", "lf"));
}

#[test]
fn an_error_names_the_key_and_holds_no_value_of_the_record() {
    let texts = [
        record(
            "window_title",
            r#""SECRET.APP""#,
            r#"{"text":"SECRET TITLE","why":"SECRET-WHY"}"#,
        ),
        record(
            "window_title",
            r#""SECRET.APP""#,
            r#"{"text":"SECRET","SECRETKEY":1}"#,
        ),
        record("SECRETKIND", r#""SECRET.APP""#, "{}"),
        EXAMPLE[0].replace(ID, "SECRET-ID"),
        EXAMPLE[0].replace(r#""p":{}"#, r#""p":{},"SECRETKEY":"SECRET""#),
        EXAMPLE[0].replace("1767258000000000000", "\"SECRET\""),
    ];
    for text in texts {
        let e = read(&text).unwrap_err();
        let shown = format!("{e} {e:?}");
        assert!(!shown.contains("SECRET"), "{shown}");
    }
}

#[test]
fn new_keeps_the_rules_of_the_reader() {
    let id = RecordId::parse(ID).unwrap();
    let app = AppId::parse("com.apple.mail").unwrap();
    let mark = CaptureRecord::new(
        id.clone(),
        Instant(5),
        Seq::new(0).unwrap(),
        Some(app.clone()),
        Payload::RunMark(Edge::End),
    );
    assert_eq!(mark.unwrap_err().key(), "app");
    assert!(Seq::new(1 << 63).is_none());
    let title = CaptureRecord::new(
        id,
        Instant(-1),
        Seq::new(7).unwrap(),
        Some(app),
        Payload::WindowTitle(Title {
            text: Text::parse("a\u{1}b\"c\\").unwrap(),
            cut: None,
            why: None,
            site: None,
        }),
    )
    .unwrap();
    let written = String::from_utf8(title.to_json()).unwrap();
    assert_eq!(
        written,
        r#"{"v":1,"id":"019b78c9-0a80-7a1b-8f21-3b5d6c7e8f90","at":-1,"seq":7,"kind":"window_title","app":"com.apple.mail","p":{"text":"a\u0001b\"c\\"}}"#
    );
    assert_eq!(CaptureRecord::read(written.as_bytes()).unwrap(), title);
    let motion = Payload::Pointer(Pointer::Motion(Motion::Drag, None));
    assert_eq!(motion.kind(), Kind::Pointer);
    let click = Payload::Pointer(Pointer::Click(Some(Button::Secondary)));
    assert_eq!(click.kind(), Kind::Pointer);
}

#[test]
fn lines_hold_one_line_and_count_a_long_one_without_holding_it() {
    let mut data = Vec::new();
    data.extend_from_slice(b"one\n");
    data.extend(std::iter::repeat_n(b'x', 20_000));
    data.push(b'\n');
    data.extend_from_slice(b"\n");
    data.extend_from_slice(b"tail");
    let mut lines = Lines::new(Cursor::new(data));
    let mut buf = Vec::new();
    assert_eq!(lines.next_line(&mut buf).unwrap(), Some(Line::Bytes));
    assert_eq!(buf, b"one\n");
    assert!(!lines.at_end().unwrap());
    assert_eq!(lines.next_line(&mut buf).unwrap(), Some(Line::Long(20_001)));
    assert!(buf.len() <= LINE_MAX + 1);
    assert_eq!(lines.next_line(&mut buf).unwrap(), Some(Line::Bytes));
    assert_eq!(buf, b"\n");
    assert_eq!(lines.next_line(&mut buf).unwrap(), Some(Line::Bytes));
    assert_eq!(buf, b"tail");
    assert!(lines.at_end().unwrap());
    assert_eq!(lines.next_line(&mut buf).unwrap(), None);
    let mut skip = Lines::new(Cursor::new(b"a\nbb\nccc".to_vec()));
    assert!(skip.skip_line().unwrap());
    assert!(skip.skip_line().unwrap());
    assert_eq!(skip.next_line(&mut buf).unwrap(), Some(Line::Bytes));
    assert_eq!(buf, b"ccc");
    assert!(!skip.skip_line().unwrap());
    let mut exact = vec![b'y'; LINE_MAX - 1];
    exact.push(b'\n');
    let mut at_max = Lines::new(Cursor::new(exact));
    assert_eq!(at_max.next_line(&mut buf).unwrap(), Some(Line::Bytes));
    assert_eq!(buf.len(), LINE_MAX);
}

#[test]
fn lines_keep_a_line_of_16384_bytes_and_pass_over_one_of_16385() {
    let mut data = vec![b'y'; LINE_MAX - 1];
    data.push(b'\n');
    data.extend(std::iter::repeat_n(b'z', LINE_MAX));
    data.push(b'\n');
    let mut lines = Lines::new(std::io::BufReader::with_capacity(4_096, Cursor::new(data)));
    let mut buf = Vec::new();
    assert_eq!(lines.next_line(&mut buf).unwrap(), Some(Line::Bytes));
    assert_eq!(buf.len(), 16_384);
    assert_eq!(lines.next_line(&mut buf).unwrap(), Some(Line::Long(16_385)));
    assert!(buf.is_empty());
    assert_eq!(lines.next_line(&mut buf).unwrap(), None);
}

#[test]
fn an_app_with_a_control_character_of_the_category_cc_is_taken_as_null() {
    let app_of = |app: &str| {
        let json = record("foreground_app", &format!("\"{app}\""), "{}");
        read(&json).unwrap().app().map(|a| a.as_str().to_owned())
    };
    for c in ['\u{80}', '\u{85}', '\u{9b}', '\u{9f}'] {
        assert_eq!(app_of(&format!("com.example{c}reader")), None, "{c:?}");
        assert_eq!(app_of(&format!("/Applications/Odd{c}.app")), None, "{c:?}");
    }
    for c in ['\u{a0}', '\u{ff}'] {
        let app = format!("com.example{c}reader");
        assert_eq!(app_of(&app), Some(app.clone()), "{c:?}");
    }
}
