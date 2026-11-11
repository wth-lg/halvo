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
        opened(STORE, w, pos(0, 0)),
        bad_request.clone(),
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
    client.open(OpenCursor::fresh(None, None)).unwrap();
    assert!(matches!(
        client.open(OpenCursor::fresh(None, None)),
        Err(ClientError::NotNow)
    ));
    assert!(matches!(
        client.read(),
        Err(ClientError::Refused(Cause::BadRequest))
    ));
    client.read().unwrap();
    assert!(matches!(
        client.read(),
        Err(ClientError::Refused(Cause::StoreError))
    ));
    assert_eq!(client.position(), None);
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
        client.close(),
        Err(ClientError::Refused(Cause::BadRequest))
    ));
    client.close().unwrap();
    let messages = sent(client);
    assert_eq!(messages.len(), 11);
    assert!(messages.iter().filter(|m| m.contains("read_batch")).count() == 4);
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
