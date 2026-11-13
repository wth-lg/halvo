mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{
    episode, key, listed, open_host, own_note_body, position, raw, raw_request, registry,
    store_id, summary_id, token, Grouping, NoProposals, OwnNote, Scratch, Seq, TestClock, DAY_NS,
    FAR, OWN, TRACE,
};
use halvo_derived::test_host::open_keyless;
use halvo_derived::{Committed, Host, Report, Store, Takes};
use halvo_schema_core::category::SeedCategory;
use halvo_schema_core::ids::RequestId;
use halvo_schema_core::kinds::KindEntry;
use halvo_schema_core::record::{
    Episode, FloorLayer, Instant, Ordinal, RowRead, SummaryRead, Tagged, Text, TypedRecord,
    WriterStep,
};
use halvo_schema_core::request::{InterfaceDecision, InterfaceRole};
use halvo_schema_core::site::AppId;
use halvo_sqlcipher::rusqlite::Connection;
use sha2::{Digest, Sha256};

const DAY: u32 = 20_767;

fn ord(n: u64) -> Ordinal {
    Ordinal::new(n).unwrap()
}

/// The episode `n` of `day` in the app `app`, whose one observed step holds `title`.
fn titled(n: u64, day: u32, app: &str, title: &str) -> Episode {
    let mut episode = episode(n, day, None);
    episode.app = AppId::parse(app).unwrap();
    episode.steps[0].title = Some(Tagged {
        value: Text::parse(title).unwrap(),
        layer: FloorLayer::Floor,
    });
    episode
}

/// The forms in which a marker can stand in a file: UTF-8, UTF-16LE and UTF-16BE.
fn forms(marker: &str) -> [Vec<u8>; 3] {
    [
        marker.as_bytes().to_vec(),
        marker.encode_utf16().flat_map(u16::to_le_bytes).collect(),
        marker.encode_utf16().flat_map(u16::to_be_bytes).collect(),
    ]
}

/// The files of the store under `scratch`, of the database, its write-ahead log and its shared-memory file, whose
/// bytes hold `marker` in one of its forms.
fn holding(scratch: &Scratch, marker: &str) -> Vec<&'static str> {
    let dir = scratch.path().join("derived");
    let mut found = Vec::new();
    for name in ["derived.db", "derived.db-wal", "derived.db-shm"] {
        let bytes = match std::fs::read(dir.join(name)) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => panic!("{name} cannot be read: {e}"),
        };
        let held = forms(marker)
            .iter()
            .any(|form| bytes.windows(form.len()).any(|w| w == form.as_slice()));
        if held {
            found.push(name);
        }
    }
    found
}

fn with(extra: &[&'static KindEntry]) -> Vec<&'static KindEntry> {
    let mut entries = registry();
    entries.extend_from_slice(extra);
    entries
}

fn keyless(scratch: &Scratch, entries: &[&'static KindEntry], host: Host) -> Store {
    open_keyless(scratch.path(), entries, host, &mut Seq(1 << 40)).unwrap()
}

const SCRUB_EXPIRY: &str = "qzscrube5501";
const SCRUB_PURGE: &str = "qzscrubp5502";
const SCRUB_DELETE: &str = "qzscrubd5503";
const SCRUB_OWN: &str = "qzscrubo5504";

/// privacy_scrub: in the keyless test build, which keeps every setting of the product and leaves the key out, a
/// marker that the test planted in a row that an expiry, a purge or the delete of a workflow removes is in no byte of
/// the database, its write-ahead log or its shared-memory file after them, while every marker stood in those files
/// before them. The rows are an evidence row past its expiry, an evidence row whose segment is purged, the evidence of
/// a deleted workflow's summaries and a row of a kind of origin own that names it.
#[test]
fn privacy_scrub() {
    let scratch = Scratch::new("privacy-scrub");
    let clock = TestClock::at_day(DAY);
    let host = Host::new(Box::new(clock.clone()), Box::new(Grouping { min: 2 })).handler(
        "test",
        Takes::Runner,
        Box::new(OwnNote),
    );
    let mut store = keyless(&scratch, &with(&[&OWN]), host);
    let mut expiring = titled(1, DAY, "com.example.expiry", &format!("Pay {SCRUB_EXPIRY}"));
    expiring.expires = Instant(i64::from(DAY + 1) * DAY_NS);
    let purged = titled(2, DAY, "com.example.purge", &format!("Pay {SCRUB_PURGE}"));
    let mut deleted: Vec<Episode> = (3..=4)
        .map(|n| titled(n, DAY, "com.example.delete", &format!("Pay {SCRUB_DELETE}")))
        .collect();
    for episode in &mut deleted {
        episode.category = SeedCategory::ShopOrder;
        episode.expires = Instant(FAR);
    }
    let mut batch = vec![expiring, purged];
    batch.extend(deleted);
    let outcome = store
        .commit_batch(&store_id(1), &batch, position(1, 0), &mut Seq(100))
        .unwrap();
    assert_eq!(
        (outcome.summaries, outcome.evidence, outcome.proposals.stored),
        (4, 4, 1)
    );
    let items = listed(&store);
    assert_eq!(items.len(), 1, "one workflow, of the two deleted episodes");
    let workflow = items[0].workflow.id().clone();
    store
        .handle(&raw_request(
            "runner",
            &own_note_body(workflow.as_str(), &format!("Note {SCRUB_OWN}")),
        ))
        .unwrap();
    let markers = [SCRUB_EXPIRY, SCRUB_PURGE, SCRUB_DELETE, SCRUB_OWN];
    for marker in markers {
        assert!(
            !holding(&scratch, marker).is_empty(),
            "the search finds {marker} while its row is stored"
        );
    }
    clock.set_day(DAY + 2);
    assert_eq!(store.expire().unwrap(), 1, "the expiry removes one row");
    assert_eq!(store.purge(&[ord(2)]).unwrap(), 1, "the purge removes one row");
    store
        .decide(&InterfaceDecision::delete(
            &InterfaceRole::claim(),
            RequestId::from_random([2; 16]),
            workflow,
        ))
        .unwrap();
    for marker in markers {
        assert_eq!(
            holding(&scratch, marker),
            Vec::<&str>::new(),
            "after the expiry, the purge and the delete, the files that hold {marker}"
        );
    }
    drop(store);
    for marker in markers {
        assert_eq!(
            holding(&scratch, marker),
            Vec::<&str>::new(),
            "after the store is closed, the files that hold {marker}"
        );
    }
}

/// A connection of the test's own to the database of a keyless store.
fn plain(scratch: &Scratch) -> Connection {
    let conn = Connection::open(scratch.db()).unwrap();
    conn.busy_timeout(std::time::Duration::ZERO).unwrap();
    conn
}

/// A connection of the test's own to the database, with the store's key or with none.
fn own(scratch: &Scratch, keyed: bool) -> Connection {
    if keyed {
        raw(scratch)
    } else {
        plain(scratch)
    }
}

fn open_store(scratch: &Scratch, keyed: bool, entries: &[&'static KindEntry], host: Host) -> Store {
    if keyed {
        Store::open(scratch.path(), &key(), entries, host, &mut Seq(1 << 40)).unwrap()
    } else {
        keyless(scratch, entries, host)
    }
}

fn count(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

fn hmac(key: &[u8], message: &[u8]) -> String {
    let pad = |byte: u8| -> Vec<u8> { key.iter().map(|b| b ^ byte).chain(vec![byte; 64 - key.len()]).collect() };
    let mut inner = Sha256::new();
    inner.update(pad(0x36));
    inner.update(message);
    let mut outer = Sha256::new();
    outer.update(pad(0x5c));
    outer.update(inner.finalize());
    outer.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Marker A of privacy_window: the title's site token, 64 lower-case hex digits, which the site token and the site
/// hash both take, so that a store that kept the token unhashed would hold it as the site key of a summary.
const MARKER_A: &str = "a11ce0de5ca1ab1ea11ce0de5ca1ab1ea11ce0de5ca1ab1ea11ce0de5ca1ab1e";
/// Marker B of privacy_window: a word of the title that is not its site token.
const MARKER_B: &str = "qzwindowb40417";

/// The second test step of privacy_window: after each commit it returns one record of the window kind `test_trace`
/// for the workflow and the segment that the test put in its cell, and nothing while the cell is empty.
struct TraceStep(Rc<RefCell<Option<(String, u64)>>>);

impl WriterStep for TraceStep {
    fn after_commit(&self, _summaries: &dyn SummaryRead, _rows: &dyn RowRead) -> Vec<TypedRecord> {
        match &*self.0.borrow() {
            None => Vec::new(),
            Some((workflow, segment)) => vec![TypedRecord {
                kind: "test_trace",
                json: format!(
                    r#"{{"v":1,"workflow":"{workflow}","segments":[{segment}],"expires":{FAR}}}"#
                ),
            }],
        }
    }
}

fn window_case(keyed: bool) {
    let scratch = Scratch::new(if keyed { "window-keyed" } else { "window-keyless" });
    let clock = TestClock::at_day(DAY + 2);
    let trace = Rc::new(RefCell::new(None));
    let host = Host::new(Box::new(clock.clone()), Box::new(Grouping { min: 3 }))
        .step("test", Box::new(TraceStep(Rc::clone(&trace))));
    let mut store = open_store(&scratch, keyed, &with(&[&TRACE]), host);
    let routine: Vec<Episode> = (0..3u32)
        .map(|i| {
            let mut e = episode(101 + u64::from(i), DAY + i, token(MARKER_A));
            e.expires = Instant(i64::from(DAY + i + 14) * DAY_NS);
            e.steps[0].title = Some(Tagged {
                value: Text::parse(&format!("Pay bill {MARKER_A} {MARKER_B}")).unwrap(),
                layer: FloorLayer::Floor,
            });
            e
        })
        .collect();
    let first = store
        .commit_batch(&store_id(1), &routine, position(1, 0), &mut Seq(100))
        .unwrap();
    assert_eq!(
        (first.summaries, first.evidence, first.proposals.stored),
        (3, 3, 1)
    );
    for marker in [MARKER_A, MARKER_B] {
        let found = holding(&scratch, marker);
        if keyed {
            assert_eq!(found, Vec::<&str>::new(), "the encrypted files hold {marker}");
        } else {
            assert!(
                !found.is_empty(),
                "the search finds {marker} while its evidence is stored"
            );
        }
    }
    clock.set_day(DAY + 2 + 15);
    assert_eq!(
        store.expire().unwrap(),
        3,
        "the expiry, with no request, removes the three evidence rows"
    );
    assert_eq!(store.purge(&[ord(101), ord(102), ord(103)]).unwrap(), 0);
    for marker in [MARKER_A, MARKER_B] {
        assert_eq!(
            holding(&scratch, marker),
            Vec::<&str>::new(),
            "after the window, the files that hold {marker}"
        );
    }
    let workflow = listed(&store)[0].workflow.id().as_str().to_owned();
    store.purge(&[ord(200)]).unwrap();
    *trace.borrow_mut() = Some((workflow, 200));
    let mut late = episode(104, DAY + 17, None);
    late.segments = vec![ord(200)];
    let other_order = store
        .commit_batch(&store_id(1), &[late], position(2, 0), &mut Seq(200))
        .unwrap();
    assert_eq!(
        (
            other_order.summaries,
            other_order.evidence,
            other_order.dropped
        ),
        (1, 0, 1),
        "(summaries, evidence rows, dropped) of an episode that names a purged ordinal"
    );
    assert_eq!(
        other_order.steps,
        vec![Ok(Committed {
            stored: 0,
            dropped: 1
        })],
        "the window record of the second step that names the purged ordinal"
    );
    drop(store);
    let conn = own(&scratch, keyed);
    assert_eq!(
        count(
            &conn,
            "SELECT count(*) FROM rows WHERE kind IN ('evidence', 'test_trace') OR class = 0"
        ),
        0,
        "window rows"
    );
    assert_eq!(count(&conn, "SELECT count(*) FROM sources"), 0, "rows that name an ordinal");
    let site_key: Vec<u8> = conn
        .query_row("SELECT site_key FROM meta", [], |r| r.get(0))
        .unwrap();
    let hash_b = hmac(&site_key, MARKER_B.as_bytes());
    let naming_b: i64 = conn
        .query_row(
            "SELECT count(*) FROM rows WHERE instr(body, ?1) > 0",
            [&hash_b],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(naming_b, 0, "rows that hold the keyed hash of marker B");
    assert_eq!(
        count(
            &conn,
            "SELECT count(*) FROM rows WHERE kind = 'summary' AND summary IN \
             ('00000000000000000000000000000065', '00000000000000000000000000000066', '00000000000000000000000000000067')"
        ),
        3,
        "the summaries of the routine"
    );
    assert_eq!(
        count(&conn, "SELECT count(*) FROM rows WHERE kind = 'workflow'"),
        1,
        "the workflow of the routine"
    );
}

/// privacy_window: a routine of three episodes holds marker A as the site token of its title and marker B elsewhere
/// in the title. After its observation window has passed on the injected clock and the expiry has run with no request,
/// neither marker is in any byte of the store's files, the encrypted ones and those of the keyless test build, where
/// the search reads the rows and both markers stood before; no row names a shredded ordinal, the keyed hash of B is in
/// no row, and the routine's summaries and its workflow stand. In the other order, an ordinal purged and then a commit
/// that names it, neither the evidence nor a window row of a registered kind is stored, and the summary is.
#[test]
fn privacy_window() {
    window_case(true);
    window_case(false);
}

fn summary_ids(conn: &Connection) -> Vec<String> {
    let mut statement = conn
        .prepare("SELECT summary FROM rows WHERE kind = 'summary' ORDER BY summary")
        .unwrap();
    statement
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// A host whose report sink keeps the last report in `last`.
fn reporting(clock: &TestClock, last: &Rc<RefCell<Option<Report>>>) -> Host {
    let last = Rc::clone(last);
    Host::new(Box::new(clock.clone()), Box::new(NoProposals))
        .on_report(Box::new(move |report: &Report| *last.borrow_mut() = Some(report.clone())))
}

/// privacy_bounded: the store, reopened on an injected clock, keeps a summary on the last day inside its bound of 93
/// days and removes one a day past it and one stamped 400 days after the clock. Each is read as kept or gone by the
/// stored rows that the test's own connection counts and by the counts of the report that the open hands its host,
/// and never through a read of the store.
#[test]
fn privacy_bounded() {
    let scratch = Scratch::new("privacy-bounded");
    let last = Rc::new(RefCell::new(None));
    let clock = TestClock::at_day(DAY - 1);
    let mut store = open_host(&scratch, reporting(&clock, &last)).unwrap();
    let inside = episode(1, DAY - 92, None);
    let past = episode(2, DAY - 93, None);
    let stored = store
        .commit_batch(&store_id(1), &[inside, past], position(1, 0), &mut Seq(100))
        .unwrap();
    clock.set_day(DAY + 400);
    let ahead = store
        .commit_batch(
            &store_id(1),
            &[episode(3, DAY + 400, None)],
            position(2, 0),
            &mut Seq(200),
        )
        .unwrap();
    assert_eq!((stored.summaries, ahead.summaries), (2, 1));
    assert_eq!(
        last.borrow().as_ref().map(|r| r.counts.bounded),
        Some(3),
        "the report's count of bounded rows before the reopen"
    );
    drop(store);
    assert_eq!(summary_ids(&raw(&scratch)).len(), 3, "summaries stored before the reopen");
    let clock = TestClock::at_day(DAY);
    let store = open_host(&scratch, reporting(&clock, &last)).unwrap();
    assert_eq!(
        last.borrow().as_ref().map(|r| r.counts.bounded),
        Some(1),
        "the report's count of bounded rows after the open on the day"
    );
    drop(store);
    assert_eq!(
        summary_ids(&raw(&scratch)),
        vec![summary_id(1).as_str().to_owned()],
        "the summaries that the test's own connection counts after the reopen"
    );
}
