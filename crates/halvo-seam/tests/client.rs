use std::fs;
use std::io::{self, Cursor};
use std::path::{Path, PathBuf};

use halvo_schema::{AnalysisRole, AnalysisWrite, ErrorName, Instant, Position, RequestId, StoreId};
use halvo_seam::client::{Client, ClientError, Failure, StreamLink};
use halvo_seam::fixture::FixtureTransport;
use halvo_seam::frame::{read_frame, write_frame, CLIENT_FRAME_MAX, SERVER_FRAME_MAX};
use halvo_seam::message::{Cause, KindFilter, OpenCursor, TimeRange};
use halvo_seam::record::ServedKind;

const STORE: &str = "4a20c017c73d6c2ba45ee3b27fa724f4";
const OTHER: &str = "0123456789abcdef0123456789abcdef";
const DAY: i64 = 1_767_258_000_000_000_000;

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

fn record(i: u64, kind: &str, at: i64) -> String {
    let (app, p) = match kind {
        "keystroke" => (r#""com.example.app""#, r#"{"class":"printable"}"#),
        "window_title" => (
            r#""com.example.app""#,
            r#"{"text":"t","cut":false,"why":"app_change"}"#,
        ),
        "run_mark" => ("null", r#"{"edge":"start"}"#),
        "screen_frame" => (r#""com.example.app""#, "{}"),
        _ => (r#""com.example.app""#, "{}"),
    };
    format!(
        r#"{{"v":1,"id":"019b78c9-0000-7000-8000-{i:012x}","at":{at},"seq":{i},"kind":"{kind}","app":{app},"p":{p}}}"#
    )
}

fn store_files(test: &str, days: usize, per_day: u64) -> Vec<PathBuf> {
    let dir = new_dir(test);
    (0..days)
        .map(|d| {
            let text: String = (0..per_day)
                .map(|i| {
                    let n = d as u64 * per_day + i;
                    let kind =
                        ["keystroke", "window_title", "run_mark", "screen_frame"][(n % 4) as usize];
                    record(n, kind, DAY + n as i64 * 1_000_000_000) + "\n"
                })
                .collect();
            let path = dir.join(format!("day-{:03}.jsonl", d + 1));
            fs::write(&path, text).unwrap();
            path
        })
        .collect()
}

fn pos(seg: u64, idx: u64) -> Position {
    Position { seg, idx }
}

fn pos_json(p: Position) -> String {
    format!(r#"{{"seg":{},"idx":{}}}"#, p.seg, p.idx)
}

fn batch_json(store: &str, w: Position, entries: &[(Position, String)], next: Position) -> String {
    let records: Vec<String> = entries
        .iter()
        .map(|(p, r)| format!(r#"{{"pos":{},"record":{r}}}"#, pos_json(*p)))
        .collect();
    let mut segs: Vec<u64> = entries.iter().map(|(p, _)| p.seg).collect();
    segs.dedup();
    let expiry: Vec<String> = segs
        .iter()
        .map(|s| format!(r#"{{"seg":{s},"at":{}}}"#, DAY + 2_000_000_000_000_000))
        .collect();
    format!(
        r#"{{"msg":"batch","store":"{store}","watermark":{},"records":[{}],"expiry":[{}],"next":{}}}"#,
        pos_json(w),
        records.join(","),
        expiry.join(","),
        pos_json(next)
    )
}

fn opened(store: &str, w: Position, next: Position) -> String {
    batch_json(store, w, &[], next)
}

fn scripted(replies: &[String]) -> Client<StreamLink<Cursor<Vec<u8>>, Vec<u8>>> {
    let mut bytes = Vec::new();
    for r in replies {
        write_frame(&mut bytes, r.as_bytes(), SERVER_FRAME_MAX).unwrap();
    }
    Client::new(StreamLink::new(Cursor::new(bytes), Vec::new()))
}

fn sent(client: Client<StreamLink<Cursor<Vec<u8>>, Vec<u8>>>) -> Vec<String> {
    let (_, written) = client.into_link().into_parts();
    let mut reader = Cursor::new(written);
    let mut out = Vec::new();
    while let Some(p) = read_frame(&mut reader, CLIENT_FRAME_MAX).unwrap() {
        out.push(String::from_utf8(p).unwrap());
    }
    out
}

fn store() -> StoreId {
    StoreId::parse(STORE).unwrap()
}

fn read_to_end<L: halvo_seam::client::Link>(
    client: &mut Client<L>,
) -> Vec<halvo_seam::message::Batch> {
    let mut out = Vec::new();
    loop {
        let b = client.read().unwrap();
        let done = b.next == b.watermark;
        out.push(b);
        if done {
            return out;
        }
    }
}

#[test]
fn the_client_reads_a_store_through_both_entries_alike() {
    let files = store_files(
        "the_client_reads_a_store_through_both_entries_alike",
        3,
        3000,
    );
    let mut transport = FixtureTransport::open(&files, 1, None).unwrap();
    let mut client = Client::new(&mut transport);
    let opened = client.open(OpenCursor::fresh(None, None)).unwrap();
    assert_eq!(opened.watermark, pos(4, 0));
    assert_eq!(client.position(), Some(pos(0, 0)));
    let in_process = read_to_end(&mut client);
    assert!(in_process.len() > 3);
    assert_eq!(client.position(), Some(pos(4, 0)));
    client.close().unwrap();
    assert_eq!(client.position(), None);
    let served: usize = in_process.iter().map(|b| b.records.len()).sum();
    assert_eq!(served, 9000 - 2250);

    let (c2s_r, c2s_w) = io::pipe().unwrap();
    let (s2c_r, s2c_w) = io::pipe().unwrap();
    let mut server = FixtureTransport::open(&files, 1, None).unwrap();
    let thread = std::thread::spawn(move || {
        let result = server.serve(c2s_r, s2c_w);
        (server, result)
    });
    let mut client = Client::new(StreamLink::new(s2c_r, c2s_w));
    client
        .open(OpenCursor::at(opened.store.clone(), pos(0, 0), None, None).unwrap())
        .unwrap();
    let streamed = read_to_end(&mut client);
    client.close().unwrap();
    assert_eq!(streamed, in_process);
    drop(client);
    let (_, result) = thread.join().unwrap();
    result.unwrap();
}

#[test]
fn a_reader_that_resumes_at_its_position_skips_and_repeats_no_record() {
    let files = store_files("a_reader_that_resumes_at_its_position", 2, 4000);
    let mut whole = FixtureTransport::open(&files, 1, None).unwrap();
    let mut client = Client::new(&mut whole);
    let opened = client.open(OpenCursor::fresh(None, None)).unwrap();
    let all: Vec<Position> = read_to_end(&mut client)
        .iter()
        .flat_map(|b| b.records.iter().map(|e| e.pos))
        .collect();
    let mut got = Vec::new();
    let mut position = pos(0, 0);
    loop {
        let mut transport = FixtureTransport::open(&files, 1, None).unwrap();
        let mut client = Client::new(&mut transport);
        client
            .open(OpenCursor::at(opened.store.clone(), position, None, None).unwrap())
            .unwrap();
        let b = client.read().unwrap();
        let take = (b.records.len() / 2).max(1).min(b.records.len());
        got.extend(b.records[..take].iter().map(|e| e.pos));
        position = if take < b.records.len() {
            b.records[take].pos
        } else {
            b.next
        };
        if position == b.watermark {
            break;
        }
    }
    assert_eq!(got, all);
}

#[test]
fn a_frame_or_a_message_outside_the_protocol_stops_the_client_and_it_sends_nothing_more() {
    let over = format!("{{\"msg\":\"closed\"{}}}", " ".repeat(SERVER_FRAME_MAX));
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&((SERVER_FRAME_MAX + 1) as u32).to_be_bytes());
    bytes.extend_from_slice(&over.as_bytes()[..SERVER_FRAME_MAX + 1]);
    let mut client = Client::new(StreamLink::new(Cursor::new(bytes), Vec::new()));
    assert!(matches!(
        client.open(OpenCursor::fresh(None, None)),
        Err(ClientError::Failed(Failure::Frame))
    ));
    assert!(matches!(
        client.open(OpenCursor::fresh(None, None)),
        Err(ClientError::Failed(Failure::Stopped))
    ));
    assert!(matches!(
        client.read(),
        Err(ClientError::Failed(Failure::Stopped))
    ));
    let (_, written) = client.into_link().into_parts();
    let mut reader = Cursor::new(written);
    assert!(read_frame(&mut reader, CLIENT_FRAME_MAX).unwrap().is_some());
    assert!(read_frame(&mut reader, CLIENT_FRAME_MAX).unwrap().is_none());

    let w = pos(3, 0);
    let array_record = batch_json(STORE, w, &[], pos(0, 0)).replace(
        r#""records":[]"#,
        r#""records":[{"pos":{"seg":1,"idx":0},"record":[1,"019b78c9-0000-7000-8000-000000000001",1,0,"keystroke","a",{"class":"tab"}]}]"#,
    );
    let mut client = scripted(&[opened(STORE, w, pos(0, 0)), array_record]);
    client.open(OpenCursor::fresh(None, None)).unwrap();
    assert!(matches!(
        client.read(),
        Err(ClientError::Failed(Failure::Message))
    ));
    assert!(matches!(
        client.close(),
        Err(ClientError::Failed(Failure::Stopped))
    ));
    assert_eq!(sent(client).len(), 2);

    let mut client = scripted(&[]);
    assert!(matches!(
        client.open(OpenCursor::fresh(None, None)),
        Err(ClientError::Failed(Failure::End))
    ));
    let mut client = scripted(&[r#"{"msg":"closed"}"#.to_owned()]);
    assert!(matches!(
        client.open(OpenCursor::fresh(None, None)),
        Err(ClientError::Failed(Failure::Order(_)))
    ));
    let mut client =
        scripted(&[r#"{"msg":"error","cause":"bad-frame","store":null,"detail":"x"}"#.to_owned()]);
    assert!(matches!(
        client.open(OpenCursor::fresh(None, None)),
        Err(ClientError::Failed(Failure::BadFrame))
    ));
    assert!(matches!(
        client.open(OpenCursor::fresh(None, None)),
        Err(ClientError::Failed(Failure::Stopped))
    ));
    assert_eq!(sent(client).len(), 1);
}

#[test]
fn a_reply_to_open_cursor_outside_c5_6_stops_the_client() {
    let w = pos(3, 0);
    let rec = record(1, "keystroke", DAY);
    let cases = [
        batch_json(STORE, w, &[(pos(1, 0), rec.clone())], pos(0, 0)),
        opened(STORE, w, pos(1, 0)),
        opened(OTHER, w, pos(1, 2)),
        opened(STORE, pos(1, 0), pos(1, 2)),
    ];
    for (n, reply) in cases.iter().enumerate() {
        let mut client = scripted(std::slice::from_ref(reply));
        let open = if n >= 2 {
            OpenCursor::at(store(), pos(1, 2), None, None).unwrap()
        } else {
            OpenCursor::fresh(None, None)
        };
        assert!(
            matches!(
                client.open(open),
                Err(ClientError::Failed(Failure::Order(_)))
            ),
            "{n}"
        );
    }
}

#[test]
fn a_batch_that_breaks_a_rule_of_c5_7_stops_the_client() {
    let w = pos(3, 0);
    let p = pos(1, 2);
    let r = |i| record(i, "keystroke", DAY + i as i64);
    let cases: Vec<(&str, String, Option<KindFilter>, Option<TimeRange>)> = vec![
        (
            "store",
            batch_json(OTHER, w, &[(pos(1, 2), r(2))], pos(1, 3)),
            None,
            None,
        ),
        (
            "watermark falls",
            batch_json(STORE, pos(2, 0), &[(pos(1, 2), r(2))], pos(1, 3)),
            None,
            None,
        ),
        (
            "next below the position",
            batch_json(STORE, w, &[], pos(1, 1)),
            None,
            None,
        ),
        (
            "next above the watermark",
            batch_json(STORE, w, &[], pos(3, 1)),
            None,
            None,
        ),
        (
            "a record before the position",
            batch_json(STORE, w, &[(pos(1, 1), r(1))], pos(1, 3)),
            None,
            None,
        ),
        (
            "a record at next",
            batch_json(STORE, w, &[(pos(1, 3), r(3))], pos(1, 3)),
            None,
            None,
        ),
        (
            "positions that do not rise",
            batch_json(STORE, w, &[(pos(1, 3), r(3)), (pos(1, 3), r(3))], pos(1, 5)),
            None,
            None,
        ),
        (
            "positions that fall",
            batch_json(STORE, w, &[(pos(1, 4), r(4)), (pos(1, 3), r(3))], pos(1, 5)),
            None,
            None,
        ),
        (
            "a screen frame",
            batch_json(
                STORE,
                w,
                &[(pos(1, 2), record(2, "screen_frame", DAY))],
                pos(1, 3),
            ),
            None,
            None,
        ),
        (
            "a record outside the kinds",
            batch_json(STORE, w, &[(pos(1, 2), r(2))], pos(1, 3)),
            KindFilter::new(&[ServedKind::RunMark]),
            None,
        ),
        (
            "a record outside the range",
            batch_json(
                STORE,
                w,
                &[(pos(1, 2), record(2, "keystroke", DAY + 50))],
                pos(1, 3),
            ),
            None,
            TimeRange::new(Instant(DAY), Instant(DAY + 50)),
        ),
        (
            "an empty batch that does not move",
            batch_json(STORE, w, &[], p),
            None,
            None,
        ),
    ];
    for (name, reply, kinds, range) in cases {
        let mut client = scripted(&[opened(STORE, w, p), reply]);
        client
            .open(OpenCursor::at(store(), p, kinds, range).unwrap())
            .unwrap();
        assert!(
            matches!(client.read(), Err(ClientError::Failed(Failure::Order(_)))),
            "{name}"
        );
        assert!(
            matches!(client.read(), Err(ClientError::Failed(Failure::Stopped))),
            "{name}"
        );
    }
    let good = [
        batch_json(STORE, w, &[], pos(1, 9)),
        batch_json(
            STORE,
            w,
            &[(pos(1, 9), r(9)), (pos(2, 0), r(10))],
            pos(2, 4),
        ),
        batch_json(STORE, pos(4, 0), &[], pos(4, 0)),
        batch_json(STORE, pos(4, 0), &[], pos(4, 0)),
    ];
    let mut replies = vec![opened(STORE, w, p)];
    replies.extend(good.iter().cloned());
    let mut client = scripted(&replies);
    client
        .open(OpenCursor::at(store(), p, None, None).unwrap())
        .unwrap();
    for _ in 0..4 {
        client.read().unwrap();
    }
    assert_eq!(client.position(), Some(pos(4, 0)));
}

#[test]
fn the_errors_of_c5_10_leave_the_state_that_their_table_gives() {
    let w = pos(3, 0);
    let changed = format!(
        r#"{{"msg":"error","cause":"store-changed","store":"{OTHER}","detail":"store id differs"}}"#
    );
    let bad_request =
        r#"{"msg":"error","cause":"bad-request","store":null,"detail":"x"}"#.to_owned();
    let store_error =
        r#"{"msg":"error","cause":"store-error","store":null,"detail":"segment 1 line 4"}"#
            .to_owned();
    let bad_position =
        r#"{"msg":"error","cause":"bad-position","store":null,"detail":"x"}"#.to_owned();
    let mut client = scripted(&[
        changed.clone(),
        bad_position,
        bad_request.clone(),
        opened(STORE, w, pos(0, 0)),
        batch_json(STORE, w, &[], pos(1, 0)),
        store_error,
        opened(STORE, w, pos(1, 0)),
        changed,
        opened(OTHER, w, pos(0, 0)),
        bad_request,
        r#"{"msg":"closed"}"#.to_owned(),
    ]);
    let first = client.open(OpenCursor::at(store(), pos(1, 0), None, None).unwrap());
    let Err(ClientError::Refused(Cause::StoreChanged(id))) = first else {
        panic!("{first:?}")
    };
    assert_eq!(id.as_str(), OTHER);
    assert!(matches!(client.read(), Err(ClientError::NotNow)));
    assert!(matches!(
        client.open(OpenCursor::at(store(), pos(9, 0), None, None).unwrap()),
        Err(ClientError::Refused(Cause::BadPosition))
    ));
    assert!(matches!(
        client.open(OpenCursor::fresh(None, None)),
        Err(ClientError::Refused(Cause::BadRequest))
    ));
    client.open(OpenCursor::fresh(None, None)).unwrap();
    assert!(matches!(
        client.open(OpenCursor::fresh(None, None)),
        Err(ClientError::NotNow)
    ));
    client.read().unwrap();
    assert!(matches!(
        client.read(),
        Err(ClientError::Refused(Cause::StoreError))
    ));
    assert_eq!(client.position(), Some(pos(1, 0)));
    assert!(matches!(client.close(), Err(ClientError::NotNow)));
    client
        .open(OpenCursor::at(store(), pos(1, 0), None, None).unwrap())
        .unwrap();
    assert!(matches!(
        client.read(),
        Err(ClientError::Refused(Cause::StoreChanged(_)))
    ));
    assert!(matches!(client.read(), Err(ClientError::NotNow)));
    client.open(OpenCursor::fresh(None, None)).unwrap();
    assert!(matches!(
        client.analysis_write(&a_write()),
        Err(ClientError::Refused(Cause::BadRequest))
    ));
    client.close().unwrap();
    let messages = sent(client);
    assert_eq!(messages.len(), 11);
    assert!(messages.iter().filter(|m| m.contains("read_batch")).count() == 3);
}

#[test]
fn the_analysis_write_goes_out_as_to_bytes_wrote_it_and_its_reply_is_decoded() {
    let write = AnalysisWrite::new(
        &AnalysisRole::claim(),
        RequestId::from_random([3; 16]),
        store(),
    );
    let bytes = write.to_bytes().unwrap();
    let reply = format!(
        r#"{{"msg":"write_ack","reply":{{"v":1,"id":"{}","error":"unsupported"}}}}"#,
        RequestId::from_random([3; 16]).as_str()
    );
    let mut client = scripted(&[reply, r#"{"msg":"write_ack","reply":{"v":2}}"#.to_owned()]);
    let got = client.analysis_write(&write).unwrap();
    assert_eq!(got.outcome.err(), Some(ErrorName::Unsupported));
    assert!(matches!(
        client.analysis_write(&write),
        Err(ClientError::Failed(Failure::Message))
    ));
    let messages = sent(client);
    let mut want = br#"{"msg":"analysis_write","request":"#.to_vec();
    want.extend_from_slice(&bytes);
    want.push(b'}');
    assert_eq!(messages[0].as_bytes(), &want[..]);

    let files = store_files("the_analysis_write_goes_out", 1, 4);
    let mut transport = FixtureTransport::open(&files, 1, None).unwrap();
    let mut client = Client::new(&mut transport);
    assert!(matches!(
        client.analysis_write(&write),
        Err(ClientError::Refused(Cause::BadRequest))
    ));
    client.open(OpenCursor::fresh(None, None)).unwrap();
    assert!(matches!(
        client.analysis_write(&write),
        Err(ClientError::Refused(Cause::BadRequest))
    ));
    assert_eq!(client.read().unwrap().records.len(), 3);
}

struct Typed(Vec<halvo_seam::message::ServerMessage>);

impl halvo_seam::client::Link for Typed {
    fn exchange(
        &mut self,
        _: &halvo_seam::message::ClientMessage,
    ) -> Result<halvo_seam::message::ServerMessage, Failure> {
        Ok(self.0.remove(0))
    }
}

#[test]
fn a_batch_outside_the_form_of_c5_3_stops_the_client_on_any_link() {
    let batch = |w: Position, entries: Vec<halvo_seam::message::Entry>, next: Position| {
        halvo_seam::message::ServerMessage::Batch(halvo_seam::message::Batch {
            store: store(),
            watermark: w,
            records: entries,
            expiry: Vec::new(),
            next,
        })
    };
    let rec =
        halvo_seam::record::CaptureRecord::read(record(1, "keystroke", DAY).as_bytes()).unwrap();
    let mut open_bad = Client::new(Typed(vec![batch(pos(2, 3), vec![], pos(0, 0))]));
    assert!(matches!(
        open_bad.open(OpenCursor::fresh(None, None)),
        Err(ClientError::Failed(Failure::Order(_)))
    ));
    let mut open_zero = Client::new(Typed(vec![batch(pos(0, 0), vec![], pos(0, 0))]));
    assert!(matches!(
        open_zero.open(OpenCursor::fresh(None, None)),
        Err(ClientError::Failed(Failure::Order(_)))
    ));
    let seg_zero = halvo_seam::message::Entry {
        pos: pos(0, 0),
        record: rec.clone(),
    };
    let mut read_bad = Client::new(Typed(vec![
        batch(pos(2, 0), vec![], pos(0, 0)),
        batch(pos(2, 0), vec![seg_zero], pos(0, 1)),
    ]));
    read_bad.open(OpenCursor::fresh(None, None)).unwrap();
    assert!(matches!(
        read_bad.read(),
        Err(ClientError::Failed(Failure::Order(_)))
    ));
    let seg_one = halvo_seam::message::Entry {
        pos: pos(1, 0),
        record: rec,
    };
    let mut good = Client::new(Typed(vec![
        batch(pos(2, 0), vec![], pos(0, 0)),
        batch(pos(2, 0), vec![seg_one], pos(2, 0)),
    ]));
    good.open(OpenCursor::fresh(None, None)).unwrap();
    assert_eq!(good.read().unwrap().records.len(), 1);
}

const OUT_OF_PLACE: ClientError = ClientError::Failed(Failure::Order(
    "an error that cannot answer the message sent",
));

fn error_json(cause: &str, store: Option<&str>) -> String {
    let store = store.map_or("null".to_owned(), |s| format!("\"{s}\""));
    format!(r#"{{"msg":"error","cause":"{cause}","store":{store},"detail":"x"}}"#)
}

fn a_write() -> AnalysisWrite {
    AnalysisWrite::new(
        &AnalysisRole::claim(),
        RequestId::from_random([3; 16]),
        store(),
    )
}

fn open_then(reply: String) -> Client<StreamLink<Cursor<Vec<u8>>, Vec<u8>>> {
    let mut client = scripted(&[opened(STORE, pos(3, 0), pos(0, 0)), reply]);
    client.open(OpenCursor::fresh(None, None)).unwrap();
    client
}

fn stopped_after_one_failure(
    mut client: Client<StreamLink<Cursor<Vec<u8>>, Vec<u8>>>,
    sent_before: usize,
) {
    assert_eq!(client.read(), Err(ClientError::Failed(Failure::Stopped)));
    assert_eq!(
        client.open(OpenCursor::fresh(None, None)),
        Err(ClientError::Failed(Failure::Stopped))
    );
    assert_eq!(sent(client).len(), sent_before);
}

#[test]
fn store_changed_that_names_the_store_its_open_named_is_a_failed_read() {
    let mut client = scripted(&[error_json("store-changed", Some(STORE))]);
    let got = client.open(OpenCursor::at(store(), pos(1, 0), None, None).unwrap());
    assert_eq!(got, Err(OUT_OF_PLACE));
    stopped_after_one_failure(client, 1);
}

#[test]
fn store_changed_that_answers_an_open_with_a_null_store_is_a_failed_read() {
    let mut client = scripted(&[error_json("store-changed", Some(OTHER))]);
    assert_eq!(
        client.open(OpenCursor::fresh(None, None)),
        Err(OUT_OF_PLACE)
    );
    stopped_after_one_failure(client, 1);
}

#[test]
fn store_changed_that_answers_a_close_is_a_failed_read() {
    let mut client = open_then(error_json("store-changed", Some(OTHER)));
    assert_eq!(client.close(), Err(OUT_OF_PLACE));
    stopped_after_one_failure(client, 2);
}

#[test]
fn store_changed_that_names_the_store_of_the_open_cursor_to_a_read_is_a_failed_read() {
    let mut client = open_then(error_json("store-changed", Some(STORE)));
    assert_eq!(client.read(), Err(OUT_OF_PLACE));
    stopped_after_one_failure(client, 2);
}

#[test]
fn store_changed_or_bad_position_that_answers_an_analysis_write_is_a_failed_read() {
    for reply in [
        error_json("store-changed", Some(OTHER)),
        error_json("bad-position", None),
    ] {
        let mut client = scripted(&[reply]);
        assert_eq!(client.analysis_write(&a_write()).err(), Some(OUT_OF_PLACE));
        stopped_after_one_failure(client, 1);
    }
}

#[test]
fn bad_position_that_answers_a_read_is_a_failed_read() {
    let mut client = open_then(error_json("bad-position", None));
    assert_eq!(client.read(), Err(OUT_OF_PLACE));
    stopped_after_one_failure(client, 2);
}

#[test]
fn bad_position_that_answers_an_open_at_the_zero_position_is_a_failed_read() {
    for open in [
        OpenCursor::fresh(None, None),
        OpenCursor::at(store(), pos(0, 0), None, None).unwrap(),
    ] {
        let mut client = scripted(&[error_json("bad-position", None)]);
        assert_eq!(client.open(open), Err(OUT_OF_PLACE));
        stopped_after_one_failure(client, 1);
    }
}

#[test]
fn bad_position_that_answers_a_close_is_a_failed_read() {
    let mut client = open_then(error_json("bad-position", None));
    assert_eq!(client.close(), Err(OUT_OF_PLACE));
    stopped_after_one_failure(client, 2);
}

#[test]
fn bad_request_that_answers_a_read_of_an_open_cursor_is_a_failed_read() {
    let mut client = open_then(error_json("bad-request", None));
    assert_eq!(client.read(), Err(OUT_OF_PLACE));
    stopped_after_one_failure(client, 2);
}

#[test]
fn bad_request_that_answers_a_close_of_an_open_cursor_is_a_failed_read() {
    let mut client = open_then(error_json("bad-request", None));
    assert_eq!(client.close(), Err(OUT_OF_PLACE));
    stopped_after_one_failure(client, 2);
}

#[test]
fn a_refusal_that_c5_9_allows_for_the_message_sent_stays_a_refusal() {
    let w = pos(3, 0);
    let mut client = scripted(&[
        error_json("store-changed", Some(OTHER)),
        error_json("bad-position", None),
        error_json("bad-request", None),
        error_json("store-error", None),
        opened(STORE, w, pos(0, 0)),
        error_json("store-changed", Some(OTHER)),
        opened(STORE, w, pos(0, 0)),
        error_json("store-error", None),
        error_json("bad-request", None),
    ]);
    let at = |p| OpenCursor::at(store(), p, None, None).unwrap();
    assert_eq!(
        client.open(at(pos(1, 0))),
        Err(ClientError::Refused(Cause::StoreChanged(
            StoreId::parse(OTHER).unwrap()
        )))
    );
    assert_eq!(
        client.open(at(pos(9, 0))),
        Err(ClientError::Refused(Cause::BadPosition))
    );
    assert_eq!(
        client.open(OpenCursor::fresh(None, None)),
        Err(ClientError::Refused(Cause::BadRequest))
    );
    assert_eq!(
        client.open(OpenCursor::fresh(None, None)),
        Err(ClientError::Refused(Cause::StoreError))
    );
    client.open(OpenCursor::fresh(None, None)).unwrap();
    assert_eq!(
        client.read(),
        Err(ClientError::Refused(Cause::StoreChanged(
            StoreId::parse(OTHER).unwrap()
        )))
    );
    client.open(OpenCursor::fresh(None, None)).unwrap();
    assert_eq!(client.read(), Err(ClientError::Refused(Cause::StoreError)));
    assert_eq!(
        client.analysis_write(&a_write()).err(),
        Some(ClientError::Refused(Cause::BadRequest))
    );
    assert_eq!(sent(client).len(), 9);
}

#[test]
fn after_store_error_the_position_of_the_last_batch_taken_stays_readable() {
    let w = pos(3, 0);
    let store_error = error_json("store-error", None);
    let mut client = scripted(&[
        opened(STORE, w, pos(1, 2)),
        store_error.clone(),
        opened(STORE, w, pos(1, 2)),
        batch_json(STORE, w, &[], pos(2, 0)),
        store_error,
    ]);
    let open = || OpenCursor::at(store(), pos(1, 2), None, None).unwrap();
    client.open(open()).unwrap();
    assert_eq!(client.read(), Err(ClientError::Refused(Cause::StoreError)));
    assert_eq!(client.position(), Some(pos(1, 2)));
    client.open(open()).unwrap();
    client.read().unwrap();
    assert_eq!(client.read(), Err(ClientError::Refused(Cause::StoreError)));
    assert_eq!(client.position(), Some(pos(2, 0)));
}

#[test]
fn after_store_changed_to_a_read_the_position_of_the_last_batch_taken_stays_readable() {
    let w = pos(3, 0);
    let mut client = scripted(&[
        opened(STORE, w, pos(0, 0)),
        batch_json(STORE, w, &[], pos(2, 0)),
        error_json("store-changed", Some(OTHER)),
    ]);
    client.open(OpenCursor::fresh(None, None)).unwrap();
    client.read().unwrap();
    assert!(matches!(
        client.read(),
        Err(ClientError::Refused(Cause::StoreChanged(_)))
    ));
    assert_eq!(client.position(), Some(pos(2, 0)));

    let files = store_files("after_store_changed_to_a_read", 2, 4);
    let mut transport = FixtureTransport::open(&files, 1, None).unwrap();
    let replace = transport.replace_handle();
    let mut client = Client::new(&mut transport);
    client.open(OpenCursor::fresh(None, None)).unwrap();
    let b = client.read().unwrap();
    replace.replace();
    assert!(matches!(
        client.read(),
        Err(ClientError::Refused(Cause::StoreChanged(_)))
    ));
    assert_eq!(client.position(), Some(b.next));
    let opened = client.open(OpenCursor::fresh(None, None)).unwrap();
    assert_eq!(opened.watermark, pos(1, 0));
    assert!(client.read().unwrap().records.is_empty());
}

#[test]
fn after_bad_position_the_position_of_the_open_stays_readable() {
    let mut client = scripted(&[error_json("bad-position", None)]);
    let got = client.open(OpenCursor::at(store(), pos(9, 0), None, None).unwrap());
    assert_eq!(got, Err(ClientError::Refused(Cause::BadPosition)));
    assert_eq!(client.position(), Some(pos(9, 0)));
}

#[test]
fn a_reply_to_open_cursor_with_an_expiry_entry_stops_the_client() {
    let reply = format!(
        r#"{{"msg":"batch","store":"{STORE}","watermark":{{"seg":3,"idx":0}},"records":[],"expiry":[{{"seg":1,"at":{DAY}}}],"next":{{"seg":0,"idx":0}}}}"#
    );
    let mut client = scripted(&[reply]);
    assert_eq!(
        client.open(OpenCursor::fresh(None, None)),
        Err(ClientError::Failed(Failure::Order(
            "the reply to open_cursor"
        )))
    );
    stopped_after_one_failure(client, 1);
}

#[test]
fn an_expiry_out_of_rising_seg_order_or_with_a_seg_twice_stops_the_client() {
    let rec = |i: u64| record(i, "keystroke", DAY + i as i64);
    let records = format!(
        r#"[{{"pos":{{"seg":1,"idx":0}},"record":{}}},{{"pos":{{"seg":2,"idx":0}},"record":{}}}]"#,
        rec(0),
        rec(1)
    );
    let later = DAY + 2_000_000_000_000_000;
    for expiry in [
        format!(r#"[{{"seg":2,"at":{later}}},{{"seg":1,"at":{later}}}]"#),
        format!(r#"[{{"seg":1,"at":{later}}},{{"seg":1,"at":{later}}},{{"seg":2,"at":{later}}}]"#),
    ] {
        let reply = format!(
            r#"{{"msg":"batch","store":"{STORE}","watermark":{{"seg":3,"idx":0}},"records":{records},"expiry":{expiry},"next":{{"seg":3,"idx":0}}}}"#
        );
        let mut client = open_then(reply);
        assert_eq!(
            client.read(),
            Err(ClientError::Failed(Failure::Order(
                "expiry in rising seg order with no seg twice"
            ))),
            "{expiry}"
        );
        stopped_after_one_failure(client, 2);
    }
}
