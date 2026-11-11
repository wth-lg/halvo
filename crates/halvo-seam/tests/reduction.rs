use std::fs;
use std::path::{Path, PathBuf};

use halvo_seam::client::Client;
use halvo_seam::fixture::FixtureTransport;
use halvo_seam::input_set::{InputSet, Layer, SetError};
use halvo_seam::message::{Entry, OpenCursor};
use halvo_seam::record::{Button, CaptureRecord, Kind, Motion, Payload, Pointer, PointerDetail};
use sha2::{Digest, Sha256};

const SET_FILE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/r2/input-sets/app-title.toml"
);
const DATA_FILE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/reduction.jsonl");
const SET_HASH: &str = "31118e5fab51c18e77f82c529b9f65cfb34dda1db55f9fe59bf2595c0c81513e";

const APP_TITLE: &str =
    "envelope = [\"app\"]\nlayers = []\n[kinds]\nforeground_app = []\nwindow_title = [\"text\"]\n";

const EXTRACTED_TEXT: &str = "envelope = [\"app\"]\nlayers = [\"accessibility_text\", \"extracted_text\"]\n[kinds]\nforeground_app = []\nwindow_title = [\"text\", \"site\"]\nkeystroke = [\"class\"]\npointer = [\"class\", \"button\", \"count\"]\n";

fn new_dir(test: &str) -> PathBuf {
    let base = Path::new(env!("CARGO_TARGET_TMPDIR"));
    let mut n = 0;
    loop {
        let dir = base.join(format!("halvo-seam-{test}-{n}"));
        if fs::create_dir(&dir).is_ok() {
            return dir;
        }
        n += 1;
    }
}

fn set(text: &str) -> InputSet {
    InputSet::parse(text.as_bytes()).unwrap_or_else(|line| panic!("line {line} of {text}"))
}

fn rec(kind: &str, p: &str) -> CaptureRecord {
    let text = format!(
        r#"{{"v":1,"id":"019b78c9-0a80-7a1b-8f21-3b5d6c7e8f90","at":1767258000000000000,"seq":3,"kind":"{kind}","app":{},"p":{p}}}"#,
        if kind == "run_mark" {
            "null"
        } else {
            r#""org.mozilla.firefox""#
        }
    );
    CaptureRecord::read(text.as_bytes()).unwrap()
}

fn p_of(record: &CaptureRecord) -> String {
    let json = String::from_utf8(record.to_json()).unwrap();
    let start = json.find(r#""p":"#).unwrap();
    json[start + 4..json.len() - 1].to_owned()
}

#[test]
fn set_reader_refusals_report_the_path_and_the_line() {
    let dir = new_dir("set_reader_refusals_report_the_path_and_the_line");
    let missing = dir.join("missing.toml");
    match InputSet::read(&missing) {
        Err(SetError::Unreadable { path, .. }) => assert_eq!(path, missing),
        other => panic!("missing file: {other:?}"),
    }
    let cases = [
        ("empty", String::new(), 1),
        (
            "comment",
            APP_TITLE.replace("[kinds]\n", "# kinds\n[kinds]\n"),
            3,
        ),
        ("comment-first", format!("# set\n{APP_TITLE}"), 1),
        ("unknown-kind", format!("{APP_TITLE}screen_frame = []\n"), 6),
        (
            "unknown-kind-run-mark",
            format!("{APP_TITLE}run_mark = []\n"),
            6,
        ),
        (
            "unknown-key",
            APP_TITLE.replace(r#"["text"]"#, r#"["text", "title"]"#),
            5,
        ),
        (
            "unknown-layer",
            APP_TITLE.replace("layers = []", r#"layers = ["screen"]"#),
            2,
        ),
        (
            "name-twice",
            APP_TITLE.replace(r#"["text"]"#, r#"["text", "text"]"#),
            5,
        ),
        (
            "out-of-order",
            APP_TITLE.replace(r#"["text"]"#, r#"["text", "why", "cut"]"#),
            5,
        ),
        (
            "kinds-out-of-order",
            format!("{APP_TITLE}pointer = []\nkeystroke = []\n"),
            7,
        ),
        ("no-text", APP_TITLE.replace(r#"["text"]"#, r#"["cut"]"#), 5),
        (
            "envelope",
            APP_TITLE.replace(r#"["app"]"#, r#"["app", "at"]"#),
            1,
        ),
        ("spacing", APP_TITLE.replace("layers = []", "layers= []"), 2),
        (
            "comma-space",
            EXTRACTED_TEXT.replace(r#""class", "button""#, r#""class","button""#),
            7,
        ),
        ("blank-line", format!("{APP_TITLE}\n"), 6),
        ("cr", APP_TITLE.replace("[kinds]\n", "[kinds]\r\n"), 3),
        (
            "ends-early",
            "envelope = [\"app\"]\nlayers = []\n[kinds]\n".to_owned(),
            4,
        ),
        ("ends-after-one", "envelope = [\"app\"]\n".to_owned(), 2),
        ("no-lf", APP_TITLE.trim_end_matches('\n').to_owned(), 5),
        (
            "no-lf-early",
            "envelope = [\"app\"]\nlayers = []".to_owned(),
            2,
        ),
        ("no-lf-optional", format!("{APP_TITLE}keystroke = []"), 6),
    ];
    for (name, text, line) in cases {
        let path = dir.join(format!("{name}.toml"));
        fs::write(&path, &text).unwrap();
        match InputSet::read(&path) {
            Err(SetError::Form {
                path: at,
                line: got,
            }) => {
                assert_eq!(at, path, "{name}");
                assert_eq!(got, line, "{name}");
            }
            other => panic!("{name}: {other:?}"),
        }
        assert_eq!(InputSet::parse(text.as_bytes()).err(), Some(line), "{name}");
    }
}

#[test]
fn the_set_files_of_c6_parse() {
    let app_title = set(APP_TITLE);
    assert!(app_title.layers().is_empty());
    assert!(app_title.keystroke().is_none() && app_title.pointer().is_none());
    let all = set(EXTRACTED_TEXT);
    assert_eq!(
        all.layers(),
        &[Layer::AccessibilityText, Layer::ExtractedText]
    );
    assert!(all.title().site && !all.title().cut);
    assert!(all.keystroke().unwrap().class);
    let p = all.pointer().unwrap();
    assert!(p.class && p.button && p.count);
    let classes = format!(
        "{APP_TITLE}keystroke = [\"class\"]\npointer = [\"class\", \"button\", \"count\"]\n"
    );
    assert!(set(&classes).pointer().is_some());
    let only_pointer = format!("{APP_TITLE}pointer = []\n");
    assert!(set(&only_pointer).keystroke().is_none());
    let only_key = format!("{APP_TITLE}keystroke = []\n");
    assert!(set(&only_key).pointer().is_none());
    let title_all = APP_TITLE.replace(r#"["text"]"#, r#"["text", "cut", "why", "site"]"#);
    let t = set(&title_all).title();
    assert!(t.cut && t.why && t.site);
    let one_layer = APP_TITLE.replace("layers = []", r#"layers = ["extracted_text"]"#);
    assert_eq!(set(&one_layer).layers(), &[Layer::ExtractedText]);
}

#[test]
fn the_reduction_serves_the_listed_keys_and_the_marks_and_nothing_else() {
    let app_title = set(APP_TITLE);
    let fg = rec("foreground_app", "{}");
    assert_eq!(app_title.reduce(&fg), Some(fg.clone()));
    let title = rec(
        "window_title",
        r#"{"text":"Pay bill","cut":false,"why":"app_change","site":{"domain":"northgrid.test","source":"address_field"}}"#,
    );
    let reduced = app_title.reduce(&title).unwrap();
    assert_eq!(p_of(&reduced), r#"{"text":"Pay bill"}"#);
    assert_eq!(reduced.id(), title.id());
    assert_eq!(reduced.at(), title.at());
    assert_eq!(reduced.seq(), title.seq());
    assert_eq!(reduced.app(), title.app());
    let mark = rec("run_mark", r#"{"edge":"end"}"#);
    assert_eq!(app_title.reduce(&mark), Some(mark.clone()));
    for out in [
        rec("keystroke", r#"{"class":"printable"}"#),
        rec("pointer", r#"{"class":"click","button":"primary"}"#),
        rec("screen_frame", "{}"),
    ] {
        assert_eq!(app_title.reduce(&out), None);
    }
    let all = set(EXTRACTED_TEXT);
    let full_title = rec(
        "window_title",
        r#"{"text":"Pay bill","cut":true,"why":"title_change","site":{"domain":"northgrid.test","source":"address_field"}}"#,
    );
    assert_eq!(
        p_of(&all.reduce(&full_title).unwrap()),
        r#"{"text":"Pay bill","site":{"domain":"northgrid.test","source":"address_field"}}"#
    );
    let click = rec("pointer", r#"{"class":"click","button":"secondary"}"#);
    let scroll = rec("pointer", r#"{"class":"scroll","count":14}"#);
    assert_eq!(all.reduce(&click), Some(click.clone()));
    assert_eq!(all.reduce(&scroll), Some(scroll.clone()));
    assert_eq!(all.reduce(&mark), Some(mark.clone()));
    assert_eq!(all.reduce(&rec("screen_frame", "{}")), None);
    let buttons = set(&format!("{APP_TITLE}pointer = [\"button\"]\n"));
    assert_eq!(
        buttons.reduce(&click).unwrap().payload(),
        &Payload::Pointer(Pointer::Unclassed(Some(PointerDetail::Button(
            Button::Secondary
        ))))
    );
    assert_eq!(
        buttons.reduce(&scroll).unwrap().payload(),
        &Payload::Pointer(Pointer::Unclassed(None))
    );
    let classes = set(&format!(
        "{APP_TITLE}keystroke = []\npointer = [\"class\"]\n"
    ));
    assert_eq!(
        classes.reduce(&scroll).unwrap().payload(),
        &Payload::Pointer(Pointer::Motion(Motion::Scroll, None))
    );
    assert_eq!(
        classes.reduce(&click).unwrap().payload(),
        &Payload::Pointer(Pointer::Click(None))
    );
    assert_eq!(
        classes
            .reduce(&rec("keystroke", r#"{"class":"tab"}"#))
            .unwrap()
            .payload(),
        &Payload::Keystroke(None)
    );
    let counts = set(&format!("{APP_TITLE}pointer = [\"count\"]\n"));
    assert_eq!(p_of(&counts.reduce(&scroll).unwrap()), r#"{"count":14}"#);
    assert_eq!(p_of(&counts.reduce(&click).unwrap()), "{}");
    for s in [&app_title, &all, &buttons, &classes, &counts] {
        for r in [&fg, &title, &mark, &click, &scroll, &full_title] {
            if let Some(once) = s.reduce(r) {
                assert_eq!(s.reduce(&once), Some(once.clone()));
            }
        }
    }
}

fn read_through(set: Option<&Path>) -> Vec<Entry> {
    let mut transport = FixtureTransport::open(&[DATA_FILE], 1, set).unwrap();
    let mut client = Client::new(&mut transport);
    client.open(OpenCursor::fresh(None, None)).unwrap();
    let mut entries = Vec::new();
    loop {
        let batch = client.read().unwrap();
        let done = batch.next == batch.watermark;
        entries.extend(batch.records);
        if done {
            client.close().unwrap();
            return entries;
        }
    }
}

#[test]
fn reduction_app_title() {
    let mut passed = 0;

    let set_bytes = fs::read(SET_FILE).unwrap();
    let hash: String = Sha256::digest(&set_bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(set_bytes, APP_TITLE.as_bytes());
    assert_eq!((set_bytes.len(), hash.as_str()), (83, SET_HASH));
    passed += 1;

    let data = fs::read(DATA_FILE).unwrap();
    let lines: Vec<&[u8]> = data.split_inclusive(|b| *b == b'\n').collect();
    assert_eq!(lines.len(), 26);
    let records: Vec<CaptureRecord> = lines
        .iter()
        .map(|l| CaptureRecord::read_line(l).unwrap())
        .collect();
    let count = |kind: Kind| records.iter().filter(|r| r.kind() == kind).count();
    assert_eq!(
        [
            Kind::ForegroundApp,
            Kind::WindowTitle,
            Kind::Keystroke,
            Kind::Pointer,
            Kind::RunMark,
            Kind::ScreenFrame
        ]
        .map(count),
        [4, 6, 7, 6, 2, 1]
    );
    let served = read_through(Some(Path::new(SET_FILE)));
    assert_eq!(served.len(), 12);
    passed += 1;

    let outside = served
        .iter()
        .filter(|e| {
            matches!(
                e.record.kind(),
                Kind::Keystroke | Kind::Pointer | Kind::ScreenFrame
            )
        })
        .count();
    assert_eq!(outside, 0);
    passed += 1;

    let mut keys_outside = 0;
    for e in &served {
        match e.record.payload() {
            Payload::WindowTitle(t) => {
                keys_outside += usize::from(t.cut.is_some())
                    + usize::from(t.why.is_some())
                    + usize::from(t.site.is_some());
            }
            Payload::ForegroundApp | Payload::RunMark(_) => {}
            _ => keys_outside += 1,
        }
        let json = String::from_utf8(e.record.to_json()).unwrap();
        match e.record.kind() {
            Kind::WindowTitle => assert!(
                json.contains(r#""p":{"text":"#)
                    && !json.contains(r#""cut""#)
                    && !json.contains(r#""site""#)
                    && !json.contains(r#""why""#)
            ),
            Kind::ForegroundApp => assert!(json.ends_with(r#""p":{}}"#)),
            _ => {}
        }
    }
    assert_eq!(keys_outside, 0);
    passed += 1;

    let mut seen = [0usize; 3];
    for (idx, record) in records.iter().enumerate() {
        let slot = match record.kind() {
            Kind::ForegroundApp => 0,
            Kind::WindowTitle => 1,
            Kind::RunMark => 2,
            _ => continue,
        };
        let entry = served
            .iter()
            .find(|e| e.pos.seg == 1 && e.pos.idx == idx as u64)
            .unwrap_or_else(|| panic!("line {idx} was not served"));
        let got = &entry.record;
        assert_eq!(
            (got.id(), got.at(), got.seq(), got.kind(), got.app()),
            (
                record.id(),
                record.at(),
                record.seq(),
                record.kind(),
                record.app()
            )
        );
        match (got.payload(), record.payload()) {
            (Payload::WindowTitle(a), Payload::WindowTitle(b)) => assert_eq!(a.text, b.text),
            _ => assert_eq!(got.to_line(), lines[idx]),
        }
        seen[slot] += 1;
    }
    assert_eq!(seen, [4, 6, 2]);
    passed += 1;

    let whole = read_through(None);
    assert_eq!(whole.len(), 25);
    let mut lines_served = 0;
    for (idx, line) in lines.iter().enumerate() {
        match whole
            .iter()
            .find(|e| e.pos.seg == 1 && e.pos.idx == idx as u64)
        {
            Some(e) => {
                assert_eq!(e.record.to_line(), *line, "line {idx}");
                lines_served += 1;
            }
            None => assert_eq!(records[idx].kind(), Kind::ScreenFrame, "line {idx}"),
        }
    }
    assert_eq!(lines_served, 25);
    passed += 1;

    assert_eq!(passed, 6);
}
