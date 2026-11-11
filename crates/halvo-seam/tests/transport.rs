use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use halvo_schema::{Instant, Position, StoreId};
use halvo_seam::fixture::{FixtureTransport, OpenError, ServeError};
use halvo_seam::frame::{read_frame, write_frame, CLIENT_FRAME_MAX, SERVER_FRAME_MAX};
use halvo_seam::message::{
    decode_server, Batch, Cause, ClientMessage, KindFilter, OpenCursor, ServerMessage, TimeRange,
};
use halvo_seam::record::{Kind, ServedKind};
use sha2::{Digest, Sha256};

const EXAMPLE_TWO: &str = concat!(
    r#"{"v":1,"id":"019b78c9-0a80-7a1b-8f21-3b5d6c7e8f90","at":1767258000000000000,"seq":0,"kind":"foreground_app","app":"org.mozilla.firefox","p":{}}"#,
    "\n",
    r#"{"v":1,"id":"019b78c9-0c1c-7c3d-9a4e-1f20718293a4","at":1767258000412000000,"seq":1,"kind":"window_title","app":"org.mozilla.firefox","p":{"text":"Pay bill - Northgrid","cut":false,"why":"app_change","site":{"domain":"northgrid.test","source":"address_field"}}}"#,
    "\n"
);

const DAY: i64 = 1_767_258_000_000_000_000;
const FOURTEEN_DAYS: i64 = 1_209_600_000_000_000;

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

fn files(test: &str, texts: &[String]) -> Vec<PathBuf> {
    let dir = new_dir(test);
    texts
        .iter()
        .enumerate()
        .map(|(i, text)| {
            let path = dir.join(format!("day-{:03}.jsonl", i + 1));
            fs::write(&path, text).unwrap();
            path
        })
        .collect()
}

fn id(i: u64) -> String {
    format!("019b78c9-0000-7000-8000-{i:012x}")
}

fn line(i: u64, at: i64, kind: &str, p: &str) -> String {
    let app = if kind == "run_mark" {
        "null".to_owned()
    } else {
        format!("\"com.example.app{}\"", i % 3)
    };
    format!(
        "{{\"v\":1,\"id\":\"{}\",\"at\":{at},\"seq\":{i},\"kind\":\"{kind}\",\"app\":{app},\"p\":{p}}}\n",
        id(i)
    )
}

fn key(i: u64, at: i64) -> String {
    line(i, at, "keystroke", r#"{"class":"printable"}"#)
}

fn mixed(i: u64, at: i64) -> String {
    match i % 7 {
        0 => line(i, at, "foreground_app", "{}"),
        1 => line(
            i,
            at,
            "window_title",
            r#"{"text":"Title text of some length","cut":false,"why":"title_change"}"#,
        ),
        2 => key(i, at),
        3 => line(i, at, "pointer", r#"{"class":"click","button":"primary"}"#),
        4 => line(i, at, "screen_frame", "{}"),
        5 => line(i, at, "pointer", r#"{"class":"move","count":9}"#),
        _ => line(i, at, "run_mark", r#"{"edge":"start"}"#),
    }
}

fn open(paths: &[PathBuf], first_live: u64) -> FixtureTransport {
    FixtureTransport::open(paths, first_live, None).unwrap()
}

fn fresh() -> ClientMessage {
    ClientMessage::OpenCursor(OpenCursor::fresh(None, None))
}

fn at(t: &FixtureTransport, position: Position) -> ClientMessage {
    ClientMessage::OpenCursor(OpenCursor::at(t.store().clone(), position, None, None).unwrap())
}

fn batch(reply: ServerMessage) -> Batch {
    match reply {
        ServerMessage::Batch(b) => b,
        other => panic!("not a batch: {other:?}"),
    }
}

fn error(reply: ServerMessage) -> (Cause, String) {
    match reply {
        ServerMessage::Error(e) => (e.cause().clone(), e.detail().to_owned()),
        other => panic!("not an error: {other:?}"),
    }
}

fn read_all(t: &mut FixtureTransport, open: ClientMessage) -> Vec<Batch> {
    let opened = batch(t.handle(&open));
    assert!(opened.records.is_empty() && opened.expiry.is_empty());
    let mut out = Vec::new();
    loop {
        let b = batch(t.handle(&ClientMessage::ReadBatch));
        let done = b.next == b.watermark;
        out.push(b);
        if done {
            break;
        }
        assert!(out.len() < 100_000);
    }
    assert!(matches!(
        t.handle(&ClientMessage::Close),
        ServerMessage::Closed
    ));
    out
}

fn frames(messages: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    for m in messages {
        write_frame(&mut out, m, CLIENT_FRAME_MAX).unwrap();
    }
    out
}

fn serve(t: &mut FixtureTransport, input: Vec<u8>) -> (Result<(), ServeError>, Vec<Vec<u8>>) {
    let mut output = Vec::new();
    let result = t.serve(Cursor::new(input), &mut output);
    let mut replies = Vec::new();
    let mut reader = Cursor::new(output);
    while let Some(p) = read_frame(&mut reader, SERVER_FRAME_MAX).unwrap() {
        replies.push(p);
    }
    (result, replies)
}

fn served(batches: &[Batch]) -> Vec<Position> {
    batches
        .iter()
        .flat_map(|b| b.records.iter().map(|e| e.pos))
        .collect()
}

#[test]
fn the_example_of_c5_14_travels_byte_for_byte() {
    let paths = files(
        "the_example_of_c5_14_travels_byte_for_byte",
        &[EXAMPLE_TWO.to_owned()],
    );
    let mut t = open(&paths, 1);
    assert_eq!(t.store().as_str(), "4a20c017c73d6c2ba45ee3b27fa724f4");
    let input = frames(&[
        br#"{"msg":"open_cursor","store":null,"position":{"seg":0,"idx":0},"kinds":null,"range":null}"#.to_vec(),
        br#"{"msg":"read_batch"}"#.to_vec(),
        br#"{"msg":"close"}"#.to_vec(),
    ]);
    let (result, replies) = serve(&mut t, input);
    result.unwrap();
    let want = [
        r#"{"msg":"batch","store":"4a20c017c73d6c2ba45ee3b27fa724f4","watermark":{"seg":2,"idx":0},"records":[],"expiry":[],"next":{"seg":0,"idx":0}}"#,
        r#"{"msg":"batch","store":"4a20c017c73d6c2ba45ee3b27fa724f4","watermark":{"seg":2,"idx":0},"records":[{"pos":{"seg":1,"idx":0},"record":{"v":1,"id":"019b78c9-0a80-7a1b-8f21-3b5d6c7e8f90","at":1767258000000000000,"seq":0,"kind":"foreground_app","app":"org.mozilla.firefox","p":{}}},{"pos":{"seg":1,"idx":1},"record":{"v":1,"id":"019b78c9-0c1c-7c3d-9a4e-1f20718293a4","at":1767258000412000000,"seq":1,"kind":"window_title","app":"org.mozilla.firefox","p":{"text":"Pay bill - Northgrid","cut":false,"why":"app_change","site":{"domain":"northgrid.test","source":"address_field"}}}}],"expiry":[{"seg":1,"at":1768467600000000000}],"next":{"seg":2,"idx":0}}"#,
        r#"{"msg":"closed"}"#,
    ];
    assert_eq!(replies.len(), 3);
    for (got, want) in replies.iter().zip(want) {
        assert_eq!(String::from_utf8_lossy(got), want);
    }
    let other = frames(&[br#"{"msg":"open_cursor","store":"00000000000000000000000000000000","position":{"seg":1,"idx":0},"kinds":null,"range":null}"#.to_vec()]);
    let (result, replies) = serve(&mut t, other);
    result.unwrap();
    assert_eq!(
        String::from_utf8_lossy(&replies[0]),
        r#"{"msg":"error","cause":"store-changed","store":"4a20c017c73d6c2ba45ee3b27fa724f4","detail":"store id differs"}"#
    );
}

#[test]
fn the_store_id_is_the_hash_of_every_file_in_list_order() {
    let texts = [key(1, DAY), String::new(), key(2, DAY + 5)];
    let paths = files(
        "the_store_id_is_the_hash_of_every_file_in_list_order",
        &texts,
    );
    let mut hasher = Sha256::new();
    for text in &texts {
        hasher.update(text.as_bytes());
    }
    let hex: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let t = open(&paths, 1);
    assert_eq!(t.store().as_str(), &hex[..32]);
    assert_eq!(open(&paths, 3).store(), t.store());
    assert_eq!(t.watermark(), Position { seg: 4, idx: 0 });
    let reversed: Vec<PathBuf> = paths.iter().rev().cloned().collect();
    assert_ne!(open(&reversed, 1).store(), t.store());
    let none: [PathBuf; 0] = [];
    let empty = open(&none, 1);
    assert_eq!(empty.store().as_str(), "e3b0c44298fc1c149afbf4c8996fb924");
    assert_eq!(empty.watermark(), Position { seg: 1, idx: 0 });
}

#[test]
fn open_fails_on_a_missing_file_a_first_live_of_zero_and_a_refused_set() {
    let paths = files("open_fails_on_a_missing_file", &[key(1, DAY)]);
    let missing = paths[0].with_file_name("day-009.jsonl");
    assert!(matches!(
        FixtureTransport::open(&[paths[0].clone(), missing], 1, None),
        Err(OpenError::File { segment: 2, .. })
    ));
    assert!(matches!(
        FixtureTransport::open(&paths, 0, None),
        Err(OpenError::FirstLive)
    ));
    let set = paths[0].with_file_name("set.toml");
    fs::write(&set, "envelope = [\"app\"]\n").unwrap();
    assert!(matches!(
        FixtureTransport::open(&paths, 1, Some(&set)),
        Err(OpenError::InputSet(_))
    ));
    assert!(matches!(
        FixtureTransport::open(&paths, 1, Some(&paths[0].with_file_name("none.toml"))),
        Err(OpenError::InputSet(_))
    ));
    assert!(FixtureTransport::open(&[paths[0].with_file_name("")], 1, None).is_err());
}

#[test]
fn a_read_that_the_byte_bound_stops_serves_every_line_once_and_both_entries_agree() {
    let big: String = (0..4000)
        .map(|i| key(i, DAY + i as i64 * 1_000_000_000))
        .collect();
    let small: String = (4000..4600)
        .map(|i| mixed(i, DAY + i as i64 * 1_000_000_000))
        .collect();
    let texts = [big.clone(), String::new(), small.clone(), String::new()];
    let paths = files("a_read_that_the_byte_bound_stops", &texts);
    let mut t = open(&paths, 1);
    let batches = read_all(&mut t, fresh());
    assert!(batches.len() > 2, "{} batches", batches.len());
    let positions = served(&batches);
    let mut want = Vec::new();
    for idx in 0..4000u64 {
        want.push(Position { seg: 1, idx });
    }
    for idx in 0..600u64 {
        if (4000 + idx) % 7 != 4 {
            want.push(Position { seg: 3, idx });
        }
    }
    assert_eq!(positions, want);
    let mut before = Position { seg: 0, idx: 0 };
    for b in &batches {
        let payload = ServerMessage::Batch(b.clone()).to_payload().unwrap();
        assert!(payload.len() <= SERVER_FRAME_MAX);
        assert!(b.next > before || b.next == b.watermark);
        assert!(b.records.iter().all(|e| e.pos >= before && e.pos < b.next));
        let segs: Vec<u64> = b.expiry.iter().map(|e| e.seg).collect();
        let mut want_segs: Vec<u64> = b.records.iter().map(|e| e.pos.seg).collect();
        want_segs.dedup();
        assert_eq!(segs, want_segs);
        assert!(b.next.seg != 2 && b.next.seg != 4 || b.next == b.watermark);
        before = b.next;
    }
    assert_eq!(batches.last().unwrap().next, Position { seg: 5, idx: 0 });
    let mut checked = 0;
    for pair in batches.windows(2) {
        let (b, following) = (&pair[0], &pair[1]);
        if b.next.seg != 1 || b.next.idx >= 3999 {
            continue;
        }
        let mut grown = b.clone();
        grown.records.push(following.records[0].clone());
        assert_eq!(following.records[0].pos, b.next);
        grown.next = Position {
            seg: 1,
            idx: b.next.idx + 1,
        };
        let len = ServerMessage::Batch(grown).to_payload().unwrap().len();
        assert!(
            len > SERVER_FRAME_MAX,
            "batch not full: one more record gives {len}"
        );
        checked += 1;
    }
    assert!(checked >= 2);
    let n = batches.len();
    let mut requests = vec![fresh().to_payload()];
    requests.extend(std::iter::repeat_n(
        ClientMessage::ReadBatch.to_payload(),
        n,
    ));
    requests.push(ClientMessage::Close.to_payload());
    let mut again = open(&paths, 1);
    let (result, replies) = serve(&mut again, frames(&requests));
    result.unwrap();
    assert_eq!(replies.len(), n + 2);
    for (reply, want) in replies[1..=n].iter().zip(&batches) {
        assert!(reply.len() <= SERVER_FRAME_MAX);
        let ServerMessage::Batch(got) = decode_server(reply).unwrap() else {
            panic!("not a batch")
        };
        assert_eq!(&got, want);
    }
}

#[test]
fn a_filter_that_leaves_every_line_out_still_reads_at_most_the_line_bound() {
    let big: String = (1000..7000)
        .map(|i| key(i, DAY + i as i64 * 1_000_000_000))
        .collect();
    let bytes = big.len();
    assert_eq!(bytes % 6000, 0);
    let paths = files("a_filter_that_leaves_every_line_out", &[big]);
    let mut t = open(&paths, 1);
    let marks = KindFilter::new(&[ServedKind::RunMark]);
    let batches = read_all(
        &mut t,
        ClientMessage::OpenCursor(OpenCursor::fresh(marks, None)),
    );
    assert!(batches
        .iter()
        .all(|b| b.records.is_empty() && b.expiry.is_empty()));
    let line_len = bytes / 6000;
    let mut before = 0u64;
    for b in &batches[..batches.len() - 1] {
        let lines = b.next.idx - before;
        assert!(lines as usize * line_len <= 262_144);
        assert!((lines as usize + 1) * line_len > 262_144);
        before = b.next.idx;
    }
    assert_eq!(batches.len(), bytes.div_ceil(262_144 / line_len * line_len));
}

#[test]
fn an_empty_segment_is_passed_in_the_same_read_and_named_in_no_expiry() {
    let texts = [
        key(1, DAY),
        String::new(),
        key(2, DAY + 9_000_000_000),
        String::new(),
    ];
    let paths = files("an_empty_segment_is_passed_in_the_same_read", &texts);
    let mut t = open(&paths, 1);
    let batches = read_all(&mut t, fresh());
    assert_eq!(batches.len(), 1);
    let b = &batches[0];
    assert_eq!(
        served(&batches),
        [Position { seg: 1, idx: 0 }, Position { seg: 3, idx: 0 }]
    );
    assert_eq!(b.expiry.iter().map(|e| e.seg).collect::<Vec<_>>(), [1, 3]);
    assert_eq!(b.next, Position { seg: 5, idx: 0 });
    assert_eq!(b.watermark, Position { seg: 5, idx: 0 });
    let all_empty = files(
        "an_empty_segment_all_empty",
        &[String::new(), String::new()],
    );
    let mut e = open(&all_empty, 1);
    let batches = read_all(&mut e, fresh());
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].next, Position { seg: 3, idx: 0 });
}

#[test]
fn positions_after_the_watermark_and_beyond_a_file_are_handled_by_c5_9() {
    let texts = [
        key(1, DAY) + &key(2, DAY + 1_000_000_000),
        key(3, DAY + 2_000_000_000),
    ];
    let paths = files("positions_after_the_watermark", &texts);
    let mut t = open(&paths, 1);
    for p in [Position { seg: 3, idx: 1 }, Position { seg: 4, idx: 0 }] {
        assert_eq!(error(t.handle(&at(&t, p))).0, Cause::BadPosition);
        assert_eq!(
            error(t.handle(&ClientMessage::ReadBatch)).0,
            Cause::BadRequest
        );
    }
    let at_w = {
        let open = at(&t, Position { seg: 3, idx: 0 });
        read_all(&mut t, open)
    };
    assert_eq!(at_w.len(), 1);
    assert!(at_w[0].records.is_empty());
    assert_eq!(at_w[0].next, Position { seg: 3, idx: 0 });
    let beyond = {
        let open = at(&t, Position { seg: 1, idx: 99 });
        read_all(&mut t, open)
    };
    assert_eq!(served(&beyond), [Position { seg: 2, idx: 0 }]);
    let inside = {
        let open = at(&t, Position { seg: 1, idx: 1 });
        read_all(&mut t, open)
    };
    assert_eq!(
        served(&inside),
        [Position { seg: 1, idx: 1 }, Position { seg: 2, idx: 0 }]
    );
    let opened = batch(t.handle(&at(&t, Position { seg: 1, idx: 1 })));
    assert_eq!(opened.next, Position { seg: 1, idx: 1 });
    assert!(opened.records.is_empty() && opened.expiry.is_empty());
}

#[test]
fn a_shredded_segment_serves_no_record_and_is_no_error() {
    let texts = [
        key(1, DAY) + &key(2, DAY + 1),
        key(3, DAY + 86_400_000_000_000),
        key(4, DAY + 2 * 86_400_000_000_000),
    ];
    let paths = files("a_shredded_segment_serves_no_record", &texts);
    let mut t = open(&paths, 2);
    let all = read_all(&mut t, fresh());
    assert_eq!(
        served(&all),
        [Position { seg: 2, idx: 0 }, Position { seg: 3, idx: 0 }]
    );
    assert!(all.iter().all(|b| b.expiry.iter().all(|e| e.seg != 1)));
    let inside = {
        let open = at(&t, Position { seg: 1, idx: 1 });
        read_all(&mut t, open)
    };
    assert_eq!(
        served(&inside),
        [Position { seg: 2, idx: 0 }, Position { seg: 3, idx: 0 }]
    );
    let mut gone = open(&paths, 4);
    let none = read_all(&mut gone, fresh());
    assert!(served(&none).is_empty());
    assert_eq!(none[0].next, Position { seg: 4, idx: 0 });
    let mut past = open(&paths, 9);
    assert!(served(&read_all(&mut past, fresh())).is_empty());
    let broken = [String::from("not json\n"), key(5, DAY)];
    let broken_paths = files("a_shredded_segment_passes_no_reader", &broken);
    let mut skip = open(&broken_paths, 2);
    assert_eq!(
        served(&read_all(&mut skip, fresh())),
        [Position { seg: 2, idx: 0 }]
    );
}

#[test]
fn a_store_id_of_another_store_gets_store_changed_and_opens_no_cursor() {
    let paths = files("a_store_id_of_another_store", &[key(1, DAY)]);
    let mut t = open(&paths, 1);
    let other = StoreId::parse("0123456789abcdef0123456789abcdef").unwrap();
    let wrong = ClientMessage::OpenCursor(
        OpenCursor::at(other, Position { seg: 1, idx: 0 }, None, None).unwrap(),
    );
    let (cause, detail) = error(t.handle(&wrong));
    assert_eq!(cause, Cause::StoreChanged(t.store().clone()));
    assert_eq!(detail, "store id differs");
    assert_eq!(
        error(t.handle(&ClientMessage::ReadBatch)).0,
        Cause::BadRequest
    );
    let wrong_after_w = ClientMessage::OpenCursor(
        OpenCursor::at(
            StoreId::parse("0123456789abcdef0123456789abcdef").unwrap(),
            Position { seg: 9, idx: 0 },
            None,
            None,
        )
        .unwrap(),
    );
    assert!(matches!(
        error(t.handle(&wrong_after_w)).0,
        Cause::StoreChanged(_)
    ));
}

#[test]
fn a_malformed_line_gets_store_error_in_place_of_its_batch_and_closes_the_cursor() {
    let good: String = (0..5).map(|i| key(i, DAY + i as i64)).collect();
    let bad_lines = [
        key(5, DAY).replace("\"v\":1", "\"v\":2"),
        "\n".to_owned(),
        key(5, DAY).replace('\n', "\r\n"),
        format!("\u{feff}{}", key(5, DAY)),
        key(5, DAY).replace("\"class\"", "\"class\":\"tab\",\"class\""),
        format!("{}{}\n", " ".repeat(16_384), key(5, DAY).trim_end()),
        line(5, DAY, "camera", "{}"),
        key(5, DAY).replace("{\"class\":\"printable\"}", "[\"printable\"]"),
    ];
    for (n, bad) in bad_lines.iter().enumerate() {
        let text = format!("{good}{bad}{}", key(6, DAY + 6));
        let paths = files(&format!("a_malformed_line_{n}"), &[text]);
        let mut t = open(&paths, 1);
        assert!(matches!(t.handle(&fresh()), ServerMessage::Batch(_)));
        let (cause, detail) = error(t.handle(&ClientMessage::ReadBatch));
        assert_eq!(
            (cause, detail.as_str()),
            (Cause::StoreError, "segment 1 line 5"),
            "{n}"
        );
        assert_eq!(
            error(t.handle(&ClientMessage::ReadBatch)).0,
            Cause::BadRequest
        );
        assert!(matches!(t.handle(&fresh()), ServerMessage::Batch(_)));
        assert_eq!(
            error(t.handle(&ClientMessage::ReadBatch)).1,
            "segment 1 line 5"
        );
        let marks = KindFilter::new(&[ServedKind::RunMark]);
        let filtered = ClientMessage::OpenCursor(OpenCursor::fresh(marks, None));
        assert!(matches!(t.handle(&filtered), ServerMessage::Batch(_)));
        assert_eq!(
            error(t.handle(&ClientMessage::ReadBatch)).1,
            "segment 1 line 5"
        );
        let after = {
            let open = at(&t, Position { seg: 1, idx: 6 });
            read_all(&mut t, open)
        };
        assert_eq!(served(&after), [Position { seg: 1, idx: 6 }]);
    }
    let tail = format!("{good}{}", key(5, DAY).trim_end());
    let paths = files("a_malformed_line_tail", &[tail, key(9, DAY)]);
    let mut t = open(&paths, 1);
    t.handle(&fresh());
    assert_eq!(
        error(t.handle(&ClientMessage::ReadBatch)).1,
        "segment 1 line 5"
    );
}

#[test]
fn a_malformed_line_after_the_bound_is_met_by_the_batch_that_would_hold_it() {
    let mut text: String = (0..4000).map(|i| key(i, DAY + i as i64)).collect();
    text.push_str("{\"v\":1}\n");
    let paths = files("a_malformed_line_after_the_bound", &[text]);
    let mut t = open(&paths, 1);
    assert!(matches!(t.handle(&fresh()), ServerMessage::Batch(_)));
    let mut last_next = Position { seg: 0, idx: 0 };
    let detail = loop {
        match t.handle(&ClientMessage::ReadBatch) {
            ServerMessage::Batch(b) => {
                assert!(!b.records.is_empty());
                last_next = b.next;
            }
            ServerMessage::Error(e) => {
                assert_eq!(e.cause(), &Cause::StoreError);
                break e.detail().to_owned();
            }
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(detail, "segment 1 line 4000");
    assert!(last_next.idx > 0 && last_next.idx < 4000);
    let resumed = batch(t.handle(&at(&t, last_next)));
    assert_eq!(resumed.next, last_next);
    loop {
        match t.handle(&ClientMessage::ReadBatch) {
            ServerMessage::Batch(_) => {}
            ServerMessage::Error(e) => {
                assert_eq!(e.detail(), "segment 1 line 4000");
                break;
            }
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn the_first_line_is_checked_when_a_read_starts_inside_its_file() {
    let rest: String = (1..5).map(|i| key(i, DAY + i as i64)).collect();
    let firsts = [
        "not json\n".to_owned(),
        format!("{}\n", "x".repeat(20_000)),
        key(0, DAY).replace('\n', ""),
    ];
    for (n, first) in firsts.iter().enumerate() {
        let paths = files(
            &format!("the_first_line_is_checked_{n}"),
            &[format!("{first}{rest}")],
        );
        let mut t = open(&paths, 1);
        t.handle(&at(&t, Position { seg: 1, idx: 2 }));
        assert_eq!(
            error(t.handle(&ClientMessage::ReadBatch)).1,
            "segment 1 line 0",
            "{n}"
        );
    }
    let dropped = format!(
        "{}not json\n{}\n{}",
        key(0, DAY),
        "y".repeat(30_000),
        key(3, DAY + 3)
    );
    let paths = files("the_dropped_lines_are_not_checked", &[dropped]);
    let mut t = open(&paths, 1);
    let got = {
        let open = at(&t, Position { seg: 1, idx: 3 });
        read_all(&mut t, open)
    };
    assert_eq!(served(&got), [Position { seg: 1, idx: 3 }]);
    assert_eq!(got[0].expiry[0].at, Instant(DAY + FOURTEEN_DAYS));
}

#[test]
fn the_expiry_instant_is_the_first_line_floored_to_a_second_plus_fourteen_days() {
    let cases = [
        (DAY + 412_000_000, DAY + FOURTEEN_DAYS),
        (-1, -1_000_000_000 + FOURTEEN_DAYS),
        (-1_000_000_000, -1_000_000_000 + FOURTEEN_DAYS),
        (i64::MIN, i64::MIN + (FOURTEEN_DAYS - 145_224_192)),
        (i64::MAX - 5, i64::MAX),
        (
            i64::MAX - FOURTEEN_DAYS - 2_000_000_000,
            i64::MAX
                - FOURTEEN_DAYS
                - 2_000_000_000
                - (i64::MAX - FOURTEEN_DAYS - 2_000_000_000).rem_euclid(1_000_000_000)
                + FOURTEEN_DAYS,
        ),
    ];
    for (n, (first_at, want)) in cases.into_iter().enumerate() {
        let text = line(0, first_at, "screen_frame", "{}") + &key(1, 7);
        let paths = files(&format!("the_expiry_instant_{n}"), &[text]);
        let mut t = open(&paths, 1);
        let got = read_all(&mut t, fresh());
        assert_eq!(served(&got), [Position { seg: 1, idx: 1 }]);
        assert_eq!(got[0].expiry[0].at, Instant(want), "{n}");
    }
}

#[test]
fn the_filters_change_no_position_and_serve_marks_and_never_screen_frames() {
    let text: String = (0..21)
        .map(|i| mixed(i, DAY + i as i64 * 1_000_000_000))
        .collect();
    let paths = files("the_filters_change_no_position", &[text]);
    let mut t = open(&paths, 1);
    let all = read_all(&mut t, fresh());
    let kinds: Vec<Kind> = all[0].records.iter().map(|e| e.record.kind()).collect();
    assert!(!kinds.contains(&Kind::ScreenFrame));
    assert_eq!(all[0].records.len(), 18);
    for kind in ServedKind::ALL {
        let filter = KindFilter::new(&[*kind]);
        let got = read_all(
            &mut t,
            ClientMessage::OpenCursor(OpenCursor::fresh(filter, None)),
        );
        let want: Vec<Position> = all[0]
            .records
            .iter()
            .filter(|e| e.record.kind() == kind.kind())
            .map(|e| e.pos)
            .collect();
        assert_eq!(served(&got), want, "{kind:?}");
        assert!(!want.is_empty());
    }
    let range = TimeRange::new(Instant(DAY + 3_000_000_000), Instant(DAY + 10_000_000_000));
    let got = read_all(
        &mut t,
        ClientMessage::OpenCursor(OpenCursor::fresh(None, range)),
    );
    let want: Vec<Position> = all[0]
        .records
        .iter()
        .filter(|e| {
            e.record.at() >= Instant(DAY + 3_000_000_000)
                && e.record.at() < Instant(DAY + 10_000_000_000)
        })
        .map(|e| e.pos)
        .collect();
    assert_eq!(served(&got), want);
    assert_eq!(want.first(), Some(&Position { seg: 1, idx: 3 }));
    assert_eq!(want.last(), Some(&Position { seg: 1, idx: 9 }));
}

#[test]
fn messages_out_of_order_and_the_analysis_write_get_bad_request() {
    let paths = files("messages_out_of_order", &[key(1, DAY) + &key(2, DAY + 1)]);
    let mut t = open(&paths, 1);
    assert_eq!(
        error(t.handle(&ClientMessage::ReadBatch)).0,
        Cause::BadRequest
    );
    assert_eq!(error(t.handle(&ClientMessage::Close)).0, Cause::BadRequest);
    let write = ClientMessage::AnalysisWrite(
        serde_json::value::RawValue::from_string("{}".to_owned()).unwrap(),
    );
    assert_eq!(error(t.handle(&write)).0, Cause::BadRequest);
    assert!(matches!(
        t.handle(&at(&t, Position { seg: 1, idx: 1 })),
        ServerMessage::Batch(_)
    ));
    assert_eq!(error(t.handle(&fresh())).0, Cause::BadRequest);
    assert_eq!(error(t.handle(&write)).0, Cause::BadRequest);
    let b = batch(t.handle(&ClientMessage::ReadBatch));
    assert_eq!(
        b.records.iter().map(|e| e.pos).collect::<Vec<_>>(),
        [Position { seg: 1, idx: 1 }]
    );
}

#[test]
fn the_stream_entry_ends_at_end_of_file_and_after_a_bad_frame() {
    let paths = files("the_stream_entry_ends", &[key(1, DAY)]);
    let mut t = open(&paths, 1);
    let (result, replies) = serve(&mut t, frames(&[fresh().to_payload()]));
    result.unwrap();
    assert_eq!(replies.len(), 1);
    let (result, replies) = serve(&mut t, frames(&[ClientMessage::ReadBatch.to_payload()]));
    result.unwrap();
    assert!(String::from_utf8_lossy(&replies[0]).contains(r#""cause":"bad-request""#));
    let mut zero = 0u32.to_be_bytes().to_vec();
    zero.extend(frames(&[fresh().to_payload()]));
    let (result, replies) = serve(&mut t, zero);
    result.unwrap();
    assert_eq!(replies.len(), 1);
    assert!(String::from_utf8_lossy(&replies[0]).contains(r#""cause":"bad-frame""#));
    let mut over = ((CLIENT_FRAME_MAX + 1) as u32).to_be_bytes().to_vec();
    over.extend(vec![b' '; CLIENT_FRAME_MAX + 1]);
    let (result, replies) = serve(&mut t, over);
    result.unwrap();
    assert_eq!(replies.len(), 1);
    assert!(String::from_utf8_lossy(&replies[0]).contains(r#""cause":"bad-frame""#));
    let mut at_bound = br#"{"msg":"read_batch"}"#.to_vec();
    at_bound.resize(CLIENT_FRAME_MAX, b' ');
    let (result, replies) = serve(
        &mut t,
        frames(&[
            at_bound,
            br#"["read_batch"]"#.to_vec(),
            fresh().to_payload(),
        ]),
    );
    result.unwrap();
    assert_eq!(replies.len(), 2);
    assert!(String::from_utf8_lossy(&replies[0]).contains(r#""cause":"bad-request""#));
    assert!(String::from_utf8_lossy(&replies[1]).contains(r#""cause":"bad-frame""#));
    let bad_kinds = br#"{"msg":"open_cursor","store":null,"position":{"seg":0,"idx":0},"kinds":[],"range":null}"#.to_vec();
    let (result, replies) = serve(
        &mut t,
        frames(&[
            bad_kinds,
            fresh().to_payload(),
            ClientMessage::ReadBatch.to_payload(),
        ]),
    );
    result.unwrap();
    assert_eq!(replies.len(), 3);
    assert!(String::from_utf8_lossy(&replies[0]).contains(r#""cause":"bad-request""#));
    assert!(String::from_utf8_lossy(&replies[2]).contains(r#""records":[{"#));
    let mut cut = frames(&[fresh().to_payload()]);
    cut.extend_from_slice(&20u32.to_be_bytes());
    cut.extend_from_slice(b"{\"msg\"");
    let (result, replies) = serve(&mut t, cut);
    assert!(matches!(result, Err(ServeError::End)));
    assert_eq!(replies.len(), 1);
    let (result, replies) = serve(&mut t, vec![0, 0]);
    assert!(matches!(result, Err(ServeError::End)));
    assert!(replies.is_empty());
}

#[test]
fn an_input_set_reduces_records_and_changes_no_position_expiry_or_next() {
    let text: String = (0..14)
        .map(|i| mixed(i, DAY + i as i64 * 1_000_000_000))
        .collect();
    let keys: String = (20..24)
        .map(|i| key(i, DAY + i as i64 * 1_000_000_000))
        .collect();
    let paths = files("an_input_set_reduces_records", &[text, keys]);
    let set = paths[0].with_file_name("app-title.toml");
    fs::write(&set, "envelope = [\"app\"]\nlayers = []\n[kinds]\nforeground_app = []\nwindow_title = [\"text\"]\n").unwrap();
    let mut whole = open(&paths, 1);
    let mut reduced = FixtureTransport::open(&paths, 1, Some(&set)).unwrap();
    let a = read_all(&mut whole, fresh());
    let b = read_all(&mut reduced, fresh());
    assert_eq!(a.len(), b.len());
    for (x, y) in a.iter().zip(&b) {
        assert_eq!(x.next, y.next);
        assert_eq!(x.watermark, y.watermark);
        assert_eq!(x.expiry, y.expiry);
        assert_eq!(x.store, y.store);
    }
    assert_eq!(
        b[0].expiry.iter().map(|e| e.seg).collect::<Vec<_>>(),
        [1, 2]
    );
    let kinds: Vec<Kind> = b
        .iter()
        .flat_map(|x| x.records.iter().map(|e| e.record.kind()))
        .collect();
    assert!(kinds
        .iter()
        .all(|k| matches!(k, Kind::ForegroundApp | Kind::WindowTitle | Kind::RunMark)));
    assert_eq!(kinds.len(), 6);
    let titles: Vec<String> = b
        .iter()
        .flat_map(|x| x.records.iter())
        .filter(|e| e.record.kind() == Kind::WindowTitle)
        .map(|e| String::from_utf8(e.record.to_json()).unwrap())
        .collect();
    assert!(titles
        .iter()
        .all(|t| t.ends_with(r#""p":{"text":"Title text of some length"}}"#)));
}
