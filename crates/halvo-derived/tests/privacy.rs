mod common;

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::time::Duration;

use common::{
    domain, episode, key, listed, open_host, own_note_body, position, raw, raw_request, registry,
    store_id, summary_id, token, Grouping, NoProposals, OwnNote, Scratch, Seq, TestClock, DAY_NS,
    FAR, NOTE, OWN, TRACE,
};
use halvo_derived::test_host::{delete_all, open_keyless};
use halvo_derived::{Committed, Counts, Host, OpenError, Report, Store, Takes};
use halvo_schema_core::category::SeedCategory;
use halvo_schema_core::ids::{RequestId, WorkflowId};
use halvo_schema_core::kinds::{self, KindEntry};
use halvo_schema_core::record::{
    Episode, FloorLayer, Instant, Ordinal, RowRead, SummaryRead, Tagged, Text, TypedRecord,
    Workflow, WriterStep,
};
use halvo_schema_core::request::{self, Decoded, ErrorName, InterfaceDecision, InterfaceRole};
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

/// The names of the entries of `derived/` under `scratch` whose bytes hold `marker` in one of its forms, in the order
/// of their names. It lists the directory and reads every entry of it, the database, its write-ahead log, its
/// shared-memory file and any file that a later store makes there, with no list of names of its own; an empty file
/// holds nothing and is not read. An entry that is no regular file, or that cannot be read, fails the test with its
/// name. Its limit: a marker that crosses the end of a page is not found whole, which is why every test first asserts
/// that each of its markers is found while its row is stored.
fn holding(scratch: &Scratch, marker: &str) -> Vec<String> {
    let dir = scratch.path().join("derived");
    let mut entries: Vec<std::fs::DirEntry> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("derived/ cannot be listed: {e}"))
        .map(|entry| entry.unwrap_or_else(|e| panic!("an entry of derived/ cannot be read: {e}")))
        .collect();
    entries.sort_by_key(|entry| entry.file_name());
    let mut found = Vec::new();
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let metadata = entry
            .metadata()
            .unwrap_or_else(|e| panic!("{name} cannot be read: {e}"));
        assert!(metadata.is_file(), "{name} is no regular file");
        if metadata.len() == 0 {
            continue;
        }
        let bytes =
            std::fs::read(entry.path()).unwrap_or_else(|e| panic!("{name} cannot be read: {e}"));
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
const SCRUB_BELOW: &str = "qzscrubb5505";

/// privacy_scrub: in the keyless test build, which keeps every setting of the product and leaves the key out, the test
/// plants a marker in each of five rows: an evidence row past its expiry, an evidence row whose segment is purged, the
/// evidence of a deleted workflow's two summaries, a row of a kind of origin own that names that workflow, and an
/// evidence row below the first live ordinal. Every marker is found in the files while its row is stored. Right after
/// each of the four removals (the expiry, the purge, the delete of the workflow, the purge below the first live
/// ordinal), before the next one runs, the marker of each row that the removal took is in no entry of `derived/`, and
/// every marker whose row is still stored is still found there; after the last removal and after the store is closed
/// no marker is in any entry.
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
    let below = titled(5, DAY, "com.example.below", &format!("Pay {SCRUB_BELOW}"));
    let mut batch = vec![expiring, purged, below];
    batch.extend(deleted);
    let outcome = store
        .commit_batch(&store_id(1), &batch, position(1, 0), &mut Seq(100))
        .unwrap();
    assert_eq!(
        (
            outcome.summaries,
            outcome.evidence,
            outcome.proposals.stored
        ),
        (5, 5, 1)
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
    let markers = [
        SCRUB_EXPIRY,
        SCRUB_PURGE,
        SCRUB_DELETE,
        SCRUB_OWN,
        SCRUB_BELOW,
    ];
    for marker in markers {
        assert!(
            !holding(&scratch, marker).is_empty(),
            "the search finds {marker} while its row is stored"
        );
    }
    let after = |removal: &str, gone: &[&str]| {
        for marker in markers {
            let found = holding(&scratch, marker);
            if gone.contains(&marker) {
                assert_eq!(
                    found,
                    Vec::<&str>::new(),
                    "right after {removal}, the entries that hold {marker}, whose row is removed"
                );
            } else {
                assert!(
                    !found.is_empty(),
                    "right after {removal}, the search finds {marker}, whose row is stored"
                );
            }
        }
    };
    clock.set_day(DAY + 2);
    assert_eq!(store.expire().unwrap(), 1, "the expiry removes one row");
    after("the expiry", &[SCRUB_EXPIRY]);
    assert_eq!(
        store.purge(&[ord(2)]).unwrap(),
        1,
        "the purge removes one row"
    );
    after("the purge", &[SCRUB_EXPIRY, SCRUB_PURGE]);
    store
        .decide(&InterfaceDecision::delete(
            &InterfaceRole::claim(),
            RequestId::from_random([2; 16]),
            workflow,
        ))
        .unwrap();
    after(
        "the delete of the workflow",
        &[SCRUB_EXPIRY, SCRUB_PURGE, SCRUB_DELETE, SCRUB_OWN],
    );
    assert_eq!(
        store.purge_below(ord(6)).unwrap(),
        1,
        "the purge below the first live ordinal removes one row"
    );
    after("the purge below the first live ordinal", &markers);
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
    halvo_derived::test_host::start_sqlite();
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
    let pad = |byte: u8| -> Vec<u8> {
        key.iter()
            .map(|b| b ^ byte)
            .chain(vec![byte; 64 - key.len()])
            .collect()
    };
    let mut inner = Sha256::new();
    inner.update(pad(0x36));
    inner.update(message);
    let mut outer = Sha256::new();
    outer.update(pad(0x5c));
    outer.update(inner.finalize());
    outer
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
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
    let scratch = Scratch::new(if keyed {
        "window-keyed"
    } else {
        "window-keyless"
    });
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
            assert_eq!(
                found,
                Vec::<&str>::new(),
                "the encrypted files hold {marker}"
            );
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
    for marker in [MARKER_A, MARKER_B] {
        assert_eq!(
            holding(&scratch, marker),
            Vec::<&str>::new(),
            "right after the expiry, the entries that hold {marker}"
        );
    }
    assert_eq!(
        store.purge(&[ord(101), ord(102), ord(103)]).unwrap(),
        0,
        "the purge of the window's ordinals after the expiry removes nothing"
    );
    let workflow = listed(&store)[0].workflow.id().as_str().to_owned();
    store.purge(&[ord(200)]).unwrap();
    *trace.borrow_mut() = Some((workflow.clone(), 200));
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
    *trace.borrow_mut() = Some((workflow, 300));
    let stored = store
        .commit_batch(&store_id(1), &[], position(3, 0), &mut Seq(300))
        .unwrap();
    assert_eq!(
        stored.steps,
        vec![Ok(Committed {
            stored: 1,
            dropped: 0
        })],
        "the window record of the second step that names a live ordinal"
    );
    *trace.borrow_mut() = None;
    assert_eq!(
        store.purge_below(ord(301)).unwrap(),
        1,
        "the purge below the first live ordinal removes the window row of the test kind"
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
    assert_eq!(
        count(&conn, "SELECT count(*) FROM sources"),
        0,
        "rows that name an ordinal"
    );
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
    Host::new(Box::new(clock.clone()), Box::new(NoProposals)).on_report(Box::new(
        move |report: &Report| *last.borrow_mut() = Some(report.clone()),
    ))
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
    assert_eq!(
        summary_ids(&raw(&scratch)).len(),
        3,
        "summaries stored before the reopen"
    );
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

/// The second test step of privacy_kept: called after each commit, it counts its calls and returns one record of the
/// test-only kind `test_note` for each workflow id that the test put in its list.
struct NoteStep {
    calls: Rc<Cell<u32>>,
    named: Rc<RefCell<Vec<String>>>,
}

impl WriterStep for NoteStep {
    fn after_commit(&self, _summaries: &dyn SummaryRead, _rows: &dyn RowRead) -> Vec<TypedRecord> {
        self.calls.set(self.calls.get() + 1);
        let n = self.calls.get();
        self.named
            .borrow()
            .iter()
            .flat_map(|workflow| {
                [
                    TypedRecord {
                        kind: "test_note",
                        json: format!(r#"{{"v":1,"workflow":"{workflow}","day":{DAY},"n":{n}}}"#),
                    },
                    TypedRecord {
                        kind: "test_trace",
                        json: format!(
                            r#"{{"v":1,"workflow":"{workflow}","segments":[{}],"expires":{FAR}}}"#,
                            500 + n
                        ),
                    },
                ]
            })
            .collect()
    }
}

/// An analysis write with one episode, as its sender writes it.
fn analysis_write(episode: &serde_json::Value) -> Vec<u8> {
    format!(
        r#"{{"v":1,"id":"{}","request":"analysis_write","body":{{"store":"{}","episodes":[{episode}],"commit":null}}}}"#,
        "5".repeat(32),
        "1".repeat(32)
    )
    .into_bytes()
}

fn decoded(bytes: &[u8]) -> Result<(), ErrorName> {
    match request::decode(bytes) {
        Ok(Decoded::AnalysisWrite(_)) => Ok(()),
        Ok(_) => panic!("the analysis write decodes as another request"),
        Err(e) => Err(e.error),
    }
}

/// privacy_kept: the registry passes `kinds::check_registry`; the owner's reader refuses an analysis write whose app
/// id is a path or whose domain is a sentence; with one test-only kind of origin capture whose entry gives a workflow
/// key, and a second test step after the test proposer that is called after each commit and whose records of that kind,
/// with records of a test-only window kind with a workflow key beside them, are stored by the typed commit, the delete of one workflow leaves no row of any registered kind that names it, no
/// summary of its key and no evidence or support row of those summaries, while the other workflow keeps its rows, and
/// one more batch with no new matching episode proposes nothing with its key.
#[test]
fn privacy_kept() {
    assert_eq!(kinds::check_registry(), Vec::<String>::new());
    let good = serde_json::to_value(episode(1, DAY, domain("northwind-power.test"))).unwrap();
    assert_eq!(decoded(&analysis_write(&good)), Ok(()), "the control write");
    let mut path_app = good.clone();
    path_app["app"] = serde_json::json!("/Applications/Browser.app/Contents/MacOS/browser");
    assert_eq!(
        decoded(&analysis_write(&path_app)),
        Err(ErrorName::Malformed)
    );
    let mut sentence = good.clone();
    sentence["site"]["domain"] = serde_json::json!("pay the bill at northwind power");
    assert_eq!(
        decoded(&analysis_write(&sentence)),
        Err(ErrorName::Malformed)
    );

    let scratch = Scratch::new("privacy-kept");
    let clock = TestClock::at_day(DAY);
    let calls = Rc::new(Cell::new(0));
    let named = Rc::new(RefCell::new(Vec::new()));
    let host = Host::new(Box::new(clock.clone()), Box::new(Grouping { min: 2 })).step(
        "test",
        Box::new(NoteStep {
            calls: Rc::clone(&calls),
            named: Rc::clone(&named),
        }),
    );
    let mut store = open_store(&scratch, true, &with(&[&NOTE, &TRACE]), host);
    let batch = [
        episode(1, DAY, None),
        episode(2, DAY, None),
        episode(3, DAY, domain("northwind-power.test")),
        episode(4, DAY, domain("northwind-power.test")),
    ];
    let first = store
        .commit_batch(&store_id(1), &batch, position(1, 0), &mut Seq(100))
        .unwrap();
    assert_eq!(first.proposals.stored, 2);
    assert_eq!(first.steps, vec![Ok(Committed::default())]);
    let items = listed(&store);
    let of_site = |with_site: bool| -> WorkflowId {
        let item = items
            .iter()
            .find(|item| match &item.workflow {
                Workflow::Full(full) => full.site.is_some() == with_site,
                Workflow::Dismissed(_) => false,
            })
            .unwrap();
        item.workflow.id().clone()
    };
    let (deleted, kept) = (of_site(false), of_site(true));
    *named.borrow_mut() = vec![deleted.as_str().to_owned(), kept.as_str().to_owned()];
    for at in 2..=3 {
        let outcome = store
            .commit_batch(&store_id(1), &[], position(at, 0), &mut Seq(100 * at))
            .unwrap();
        assert_eq!(
            outcome.steps,
            vec![Ok(Committed {
                stored: 4,
                dropped: 0
            })]
        );
    }
    store
        .decide(&InterfaceDecision::delete(
            &InterfaceRole::claim(),
            RequestId::from_random([2; 16]),
            deleted.clone(),
        ))
        .unwrap();
    *named.borrow_mut() = vec![kept.as_str().to_owned()];
    let after = store
        .commit_batch(&store_id(1), &[], position(4, 0), &mut Seq(400))
        .unwrap();
    assert_eq!(
        (after.proposals.stored, after.proposals.updated),
        (0, 0),
        "one more batch with no new episode proposes nothing with the deleted key"
    );
    assert_eq!(
        calls.get(),
        4,
        "the second step is called after each commit"
    );
    drop(store);
    let conn = raw(&scratch);
    let naming: i64 = conn
        .query_row(
            "SELECT count(*) FROM rows WHERE workflow = ?1 OR instr(body, ?1) > 0",
            [deleted.as_str()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(naming, 0, "rows of any kind that name the deleted workflow");
    let support: Vec<String> = [1, 2]
        .iter()
        .map(|n| summary_id(*n).as_str().to_owned())
        .collect();
    let of_key: i64 = conn
        .query_row(
            "SELECT count(*) FROM rows WHERE summary IN (?1, ?2)",
            [&support[0], &support[1]],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        of_key, 0,
        "summaries of the deleted key, their evidence and their support rows"
    );
    let kept_notes: i64 = conn
        .query_row(
            "SELECT count(*) FROM rows WHERE kind = 'test_note' AND workflow = ?1",
            [kept.as_str()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        kept_notes, 3,
        "the rows of the test-only kind of the other workflow"
    );
    let kept_traces: i64 = conn
        .query_row(
            "SELECT count(*) FROM rows WHERE kind = 'test_trace' AND workflow = ?1",
            [kept.as_str()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        kept_traces, 3,
        "the window rows of the test-only kind of the other workflow"
    );
    assert_eq!(
        count(&conn, "SELECT count(*) FROM rows WHERE kind = 'workflow'"),
        1
    );
}

const CARD: &str = "4111 1111 1111 1111";
const IBAN: &str = "GB82 WEST 1234 5698 7654 32";

/// The test worker of privacy_redact: one episode per title, each title masked by halvo-redact before it enters the
/// analysis write, as the worker of the product masks it.
fn worker(titles: &[String]) -> Vec<Episode> {
    titles
        .iter()
        .zip(1..)
        .map(|(title, n)| titled(n, DAY, "com.example.browser", &halvo_redact::mask(title)))
        .collect()
}

/// privacy_redact: titles that hold a Luhn-valid card number written with spaces and a valid IBAN go through one
/// batch of the test worker, which masks them with halvo-redact; no stored row holds either number, written with its
/// spaces or without them, and the stored titles hold the tokens of the two rules.
#[test]
fn privacy_redact() {
    let scratch = Scratch::new("privacy-redact");
    let clock = TestClock::at_day(DAY);
    let mut store = open_host(
        &scratch,
        Host::new(Box::new(clock.clone()), Box::new(Grouping { min: 2 })),
    )
    .unwrap();
    let titles = [
        format!("Pay {CARD} - Northwind Power"),
        format!("Transfer from {IBAN} - Bank"),
        format!("Pay {CARD} from {IBAN}"),
    ];
    let outcome = store
        .commit_batch(
            &store_id(1),
            &worker(&titles),
            position(1, 0),
            &mut Seq(100),
        )
        .unwrap();
    assert_eq!(
        (
            outcome.summaries,
            outcome.evidence,
            outcome.proposals.stored
        ),
        (3, 3, 1)
    );
    drop(store);
    let conn = raw(&scratch);
    let holding = |text: &str| -> i64 {
        conn.query_row(
            "SELECT count(*) FROM rows WHERE instr(body, ?1) > 0",
            [text],
            |r| r.get(0),
        )
        .unwrap()
    };
    for number in [CARD, IBAN] {
        for form in [number.to_owned(), number.replace(' ', "")] {
            assert_eq!(holding(&form), 0, "rows that hold a planted number");
        }
    }
    assert_eq!(
        (holding("[card]"), holding("[iban]")),
        (2, 2),
        "(rows that hold the card token, rows that hold the IBAN token)"
    );
}

/// The test-only count of privacy_listed, through the test's own connection and by no statement of the store: the
/// id of every stored workflow, read from its record, in ascending order; the stored rows per retention class; and
/// the summary ids of the support rows of each workflow, in ascending order.
struct Counted {
    workflows: Vec<String>,
    counts: Counts,
    support: Vec<(String, String)>,
}

fn counted(conn: &Connection) -> Counted {
    let mut statement = conn
        .prepare("SELECT body FROM rows WHERE kind = 'workflow'")
        .unwrap();
    let mut workflows: Vec<String> = statement
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .map(|body| {
            let record: serde_json::Value = serde_json::from_str(&body.unwrap()).unwrap();
            record["id"].as_str().unwrap().to_owned()
        })
        .collect();
    workflows.sort();
    let class = |code: i64| -> u64 {
        let n: i64 = conn
            .query_row("SELECT count(*) FROM rows WHERE class = ?1", [code], |r| {
                r.get(0)
            })
            .unwrap();
        u64::try_from(n).unwrap()
    };
    let mut statement = conn
        .prepare("SELECT workflow, summary FROM rows WHERE kind = 'support'")
        .unwrap();
    let mut support: Vec<(String, String)> = statement
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    support.sort();
    Counted {
        workflows,
        counts: Counts {
            window: class(0),
            bounded: class(1),
            kept: class(2),
        },
        support,
    }
}

/// privacy_listed: after an expiry, the stored-workflows section, which is the interface read `workflows` page by
/// page, and the Inventory's derived section, which is the report's count per class, show the same workflow ids, the
/// same support sets and the same counts as the test-only count through the test's own connection, over workflows in
/// the states proposed, approved and dismissed and rows that the expiry removed.
#[test]
fn privacy_listed() {
    let scratch = Scratch::new("privacy-listed");
    let clock = TestClock::at_day(DAY);
    let mut store = open_host(
        &scratch,
        Host::new(Box::new(clock.clone()), Box::new(Grouping { min: 2 })),
    )
    .unwrap();
    let apps = [
        "com.example.one",
        "com.example.two",
        "com.example.three",
        "com.example.four",
    ];
    let mut batch = Vec::new();
    for (app, n) in apps.iter().zip((1..).step_by(2)) {
        let early = if n == 7 { DAY - 92 } else { DAY - 1 };
        batch.push(titled(n, early, app, "Pay bill"));
        batch.push(titled(n + 1, DAY, app, "Pay bill"));
    }
    for episode in &mut batch[..2] {
        episode.expires = Instant(i64::from(DAY + 1) * DAY_NS);
    }
    let outcome = store
        .commit_batch(&store_id(1), &batch, position(1, 0), &mut Seq(100))
        .unwrap();
    assert_eq!(outcome.proposals.stored, 4);
    let id_of = |app: &str| -> WorkflowId {
        listed(&store)
            .into_iter()
            .find(|item| matches!(&item.workflow, Workflow::Full(full) if full.app.as_str() == app))
            .map(|item| item.workflow.id().clone())
            .unwrap()
    };
    let (approved, dismissed) = (id_of(apps[1]), id_of(apps[2]));
    let role = InterfaceRole::claim();
    store
        .decide(&InterfaceDecision::approve(
            &role,
            RequestId::from_random([4; 16]),
            approved,
        ))
        .unwrap();
    store
        .decide(&InterfaceDecision::dismiss(
            &role,
            RequestId::from_random([5; 16]),
            dismissed,
        ))
        .unwrap();
    assert_eq!(
        std::fs::metadata(scratch.path().join("derived").join("derived.db-wal"))
            .map(|m| m.len())
            .unwrap_or(0),
        0,
        "the bytes of the write-ahead log after the dismissal, which the truncating checkpoint follows"
    );
    clock.set_day(DAY + 1);
    assert_eq!(
        store.expire().unwrap(),
        5,
        "two evidence rows past their expiry, and a summary past its bound with its evidence and its support row"
    );
    let items = listed(&store);
    let report = store.report().unwrap();
    drop(store);
    let own = counted(&raw(&scratch));
    let read: Vec<String> = items
        .iter()
        .map(|item| item.workflow.id().as_str().to_owned())
        .collect();
    assert_eq!(
        read, own.workflows,
        "the workflow ids of the read and of the count"
    );
    let read_support: Vec<(String, String)> = items
        .iter()
        .flat_map(|item| {
            let id = item.workflow.id().as_str().to_owned();
            item.summaries
                .iter()
                .map(move |s| (id.clone(), s.as_str().to_owned()))
        })
        .collect();
    assert_eq!(
        read_support, own.support,
        "the support sets of the read and of the count"
    );
    assert_eq!(
        report.counts, own.counts,
        "the report's counts and the count"
    );
    assert_eq!(
        (own.counts.kept, own.support.len()),
        (4, 5),
        "(workflows, support rows) that the count finds"
    );
}

/// The delete of everything, through the test host: with a stored workflow and a key file of the test's own making
/// in `derived/`, it leaves no database file, no log, no shared-memory file, no writer lock and no key file, and no
/// directory `derived/`; a file outside `derived/` stays; and an open after it makes an empty store.
#[test]
fn delete_all_leaves_no_database_file_and_no_key_file() {
    let scratch = Scratch::new("delete-all");
    let clock = TestClock::at_day(DAY);
    let mut store = open_host(
        &scratch,
        Host::new(Box::new(clock.clone()), Box::new(Grouping { min: 2 })),
    )
    .unwrap();
    let outcome = store
        .commit_batch(
            &store_id(1),
            &[episode(1, DAY, None), episode(2, DAY, None)],
            position(1, 0),
            &mut Seq(100),
        )
        .unwrap();
    assert_eq!(outcome.proposals.stored, 1);
    let derived = scratch.path().join("derived");
    std::fs::write(derived.join("master.key"), [9u8; 48]).unwrap();
    let outside = scratch.path().join("store");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("segment"), b"segment").unwrap();
    let files = [
        "derived.db",
        "derived.db-wal",
        "derived.db-shm",
        "writer.lock",
        "master.key",
    ];
    for name in ["derived.db", "writer.lock", "master.key"] {
        assert!(
            derived.join(name).exists(),
            "{name} before the delete of everything"
        );
    }
    delete_all(store).unwrap();
    for name in files {
        assert!(
            !derived.join(name).exists(),
            "{name} after the delete of everything"
        );
    }
    assert!(!derived.exists(), "derived/ after the delete of everything");
    assert!(outside.join("segment").exists(), "a file outside derived/");
    let store = open_host(
        &scratch,
        Host::new(Box::new(clock.clone()), Box::new(NoProposals)),
    )
    .unwrap();
    assert_eq!(listed(&store).len(), 0);
    assert_eq!(store.report().unwrap().counts, Counts::default());
}

const CIPHER_MARKER: &str = "qzcipher77001";

/// The files of a store, keyed or keyless, in which a search finds the marker that the test planted as a routine's
/// title token and in its titles, searched while the store is open and again after it is closed.
fn planted(scratch: &Scratch, keyed: bool) -> Vec<String> {
    let clock = TestClock::at_day(DAY);
    let host = Host::new(Box::new(clock.clone()), Box::new(Grouping { min: 2 }));
    let mut store = open_store(scratch, keyed, &registry(), host);
    let batch: Vec<Episode> = (1..=2)
        .map(|n| {
            let mut e = titled(
                n,
                DAY,
                "com.example.browser",
                &format!("Pay {CIPHER_MARKER}"),
            );
            e.site = token(CIPHER_MARKER);
            e
        })
        .collect();
    let outcome = store
        .commit_batch(&store_id(1), &batch, position(1, 0), &mut Seq(100))
        .unwrap();
    assert_eq!((outcome.evidence, outcome.proposals.stored), (2, 1));
    let mut found = holding(scratch, CIPHER_MARKER);
    drop(store);
    found.extend(holding(scratch, CIPHER_MARKER));
    found
}

/// The ciphertext check: a marker that the test planted in stored rows, as a title token and in the titles, is in no
/// byte of the raw files of the store, while the store is open and after it is closed; the control, the same rows in
/// the keyless test build, shows that the search finds it where the bytes are not encrypted.
#[test]
fn the_ciphertext_check_finds_no_planted_marker_in_the_raw_file_bytes() {
    assert_eq!(
        planted(&Scratch::new("cipher-keyed"), true),
        Vec::<&str>::new(),
        "the encrypted files that hold the marker"
    );
    assert!(
        !planted(&Scratch::new("cipher-keyless"), false).is_empty(),
        "the control: the keyless files hold the marker"
    );
}

const SECOND: &str = "HALVO_DERIVED_SECOND_PROCESS_DIR";
const SECOND_TEST: &str = "a_second_process_is_refused_also_with_the_lock_file_unlinked";
const SECOND_TOKEN: &str = "HALVO_DERIVED_SECOND_PROCESS_TOKEN";
/// The file in the parent's scratch directory that holds the token of the second-process test. The child branch runs
/// only where the token in the child's environment equals the one that the parent wrote into this file, so that a
/// variable set from outside cannot turn a run of the binary into the child: a run with a token that the parent did not
/// write runs the test as a parent, which skips the test's last step.
const TOKEN_FILE: &str = "second-process-token";
/// How long the test waits for a child that runs as the second process.
const CHILD_BOUND: Duration = Duration::from_secs(120);
/// How long the test waits for the child that runs the whole test as a parent. Such a run starts three children of its
/// own one after another and waits up to `CHILD_BOUND` for each, so this bound is longer than the three together, and
/// the outer wait never ends while the inner run may still pass.
const PARENT_RUN_BOUND: Duration = Duration::from_secs(480);
const LOCKED: i32 = 40;
const OPENED: i32 = 41;
const OTHER: i32 = 42;

/// The second process of the test: this test binary run again for the one test of the second process, with the data
/// directory in its environment. The child answers by its own exit code and ends itself. Dropped on any path of the
/// test, a panic among them, the guard asks through the handle that `spawn` returned whether that one process still
/// runs, and only then ends it through the same handle and waits for it; it looks up no process by number or by name,
/// signals no other process, and ends no process that the child itself started.
struct Second(std::process::Child);

impl Second {
    fn start(dir: &Path, token: &str) -> Second {
        let child = Command::new(std::env::current_exe().unwrap())
            .args([SECOND_TEST, "--exact", "--test-threads=1"])
            .env(SECOND, dir)
            .env(SECOND_TOKEN, token)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        Second(child)
    }

    /// The exit code of the second process, waited for at most `bound`.
    fn code(mut self, bound: Duration) -> Option<i32> {
        let start = std::time::Instant::now();
        while start.elapsed() < bound {
            if let Some(status) = self.0.try_wait().unwrap() {
                return status.code();
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        None
    }
}

impl Drop for Second {
    fn drop(&mut self) {
        if let Ok(None) = self.0.try_wait() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

/// A second process is refused while a store holds the derived database: with the writer lock file in place, and
/// again after the lock file is unlinked, when the second process takes a new lock file and the database's exclusive
/// lock refuses it. The control: after the store is closed, a second process opens it.
#[test]
fn a_second_process_is_refused_also_with_the_lock_file_unlinked() {
    let outside = std::env::var_os(SECOND);
    let child = match (&outside, std::env::var_os(SECOND_TOKEN)) {
        (Some(dir), Some(token)) => std::fs::read(Path::new(dir).join(TOKEN_FILE))
            .ok()
            .filter(|written| written.as_slice() == token.as_encoded_bytes())
            .map(|_| dir.clone()),
        _ => None,
    };
    if let Some(dir) = child {
        let host = Host::new(Box::new(TestClock::at_day(DAY)), Box::new(NoProposals));
        let opened = Store::open(Path::new(&dir), &key(), &registry(), host, &mut Seq(1));
        std::process::exit(match opened {
            Err(OpenError::Locked) => LOCKED,
            Ok(_) => OPENED,
            Err(_) => OTHER,
        });
    }
    assert!(
        PARENT_RUN_BOUND > CHILD_BOUND * 3,
        "the bound of the child that runs as a parent exceeds the bounds of its three children"
    );
    let scratch = Scratch::new("second-process");
    let token = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    std::fs::write(scratch.path().join(TOKEN_FILE), &token).unwrap();
    let clock = TestClock::at_day(DAY);
    let store = open_host(
        &scratch,
        Host::new(Box::new(clock.clone()), Box::new(NoProposals)),
    )
    .unwrap();
    let lock = scratch.path().join("derived").join("writer.lock");
    assert!(lock.exists(), "the lock file of the open store");
    assert_eq!(
        Second::start(scratch.path(), &token).code(CHILD_BOUND),
        Some(LOCKED),
        "a second process with the lock file in place"
    );
    std::fs::remove_file(&lock).unwrap();
    assert!(!lock.exists(), "the lock file after it is unlinked");
    assert_eq!(
        Second::start(scratch.path(), &token).code(CHILD_BOUND),
        Some(LOCKED),
        "a second process with the lock file unlinked"
    );
    drop(store);
    assert_eq!(
        Second::start(scratch.path(), &token).code(CHILD_BOUND),
        Some(OPENED),
        "the control: a second process after the store is closed"
    );
    if outside.is_none() {
        assert_eq!(
            Second::start(scratch.path(), "a token that the parent did not write").code(PARENT_RUN_BOUND),
            Some(0),
            "a run with the directory set from outside and no matching token runs the test as a parent and passes"
        );
    }
}

/// The writer lock refuses an open on its own: while another holder has the lock of `derived/writer.lock` and no
/// store holds the database, an open is refused as `Locked`, and after the holder lets the lock go the open succeeds.
#[test]
fn an_open_is_refused_while_another_holds_the_writer_lock_and_the_database_is_free() {
    let scratch = Scratch::new("writer-lock");
    let clock = TestClock::at_day(DAY);
    let host = || Host::new(Box::new(clock.clone()), Box::new(NoProposals));
    drop(open_host(&scratch, host()).unwrap());
    let holder = std::fs::File::options()
        .write(true)
        .open(scratch.path().join("derived").join("writer.lock"))
        .unwrap();
    holder.try_lock().unwrap();
    assert!(
        matches!(open_host(&scratch, host()), Err(OpenError::Locked)),
        "an open while another holds the writer lock"
    );
    holder.unlock().unwrap();
    drop(holder);
    assert!(
        open_host(&scratch, host()).is_ok(),
        "the control: an open after the lock is let go"
    );
}

fn support_summaries(scratch: &Scratch) -> Vec<String> {
    let conn = raw(scratch);
    let mut statement = conn
        .prepare("SELECT summary FROM rows WHERE kind = 'support' ORDER BY summary")
        .unwrap();
    statement
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// The cost that DS-5 (a) names, pinned so that a change to its reading B is made on purpose: with the clock set back
/// by one day over a stored summary, a commit whose test proposer still returns the key gives the stored workflow the
/// proposal without the summary ahead, its recurrence and its support rows, while the summary stays stored and counted
/// in the report; after the clock has reached that day, the next commit gives the row and its support rows as they
/// were before.
#[test]
fn with_the_clock_set_back_a_day_a_workflow_takes_its_proposal_without_the_summary_ahead_until_that_day(
) {
    let scratch = Scratch::new("clock-set-back");
    let last = Rc::new(RefCell::new(None));
    let host = |clock: &TestClock| {
        let last = Rc::clone(&last);
        Host::new(Box::new(clock.clone()), Box::new(Grouping { min: 2 })).on_report(Box::new(
            move |report: &Report| *last.borrow_mut() = Some(report.clone()),
        ))
    };
    let clock = TestClock::at_day(DAY);
    let mut store = open_host(&scratch, host(&clock)).unwrap();
    let batch: Vec<Episode> = (0..3u32)
        .map(|i| episode(u64::from(i) + 1, DAY - 2 + i, None))
        .collect();
    let first = store
        .commit_batch(&store_id(1), &batch, position(1, 0), &mut Seq(100))
        .unwrap();
    assert_eq!(first.proposals.stored, 1);
    drop(store);
    let before = (
        common::bodies(&scratch, "workflow"),
        support_summaries(&scratch),
    );
    let all: Vec<String> = (1..=3).map(|n| summary_id(n).as_str().to_owned()).collect();
    assert_eq!(before.1, all);

    clock.set_day(DAY - 1);
    let mut store = open_host(&scratch, host(&clock)).unwrap();
    let back = store
        .commit_batch(&store_id(1), &[], position(2, 0), &mut Seq(200))
        .unwrap();
    assert_eq!(
        (back.proposals.stored, back.proposals.updated),
        (0, 1),
        "(stored, updated) with the clock set back"
    );
    assert_eq!(
        last.borrow().as_ref().map(|r| r.counts.bounded),
        Some(5),
        "three summaries and two support rows in the report"
    );
    drop(store);
    let set_back: serde_json::Value =
        serde_json::from_str(&common::bodies(&scratch, "workflow")[0]).unwrap();
    assert_eq!(
        (
            &set_back["recurrence"]["count"],
            &set_back["recurrence"]["last"]
        ),
        (&serde_json::json!(2), &serde_json::json!(DAY - 1)),
        "the recurrence of the proposal without the summary ahead"
    );
    assert_eq!(support_summaries(&scratch), all[..2].to_vec());
    assert_eq!(summary_ids(&raw(&scratch)), all, "the summaries stored");

    clock.set_day(DAY);
    let mut store = open_host(&scratch, host(&clock)).unwrap();
    let again = store
        .commit_batch(&store_id(1), &[], position(3, 0), &mut Seq(300))
        .unwrap();
    assert_eq!(again.proposals.updated, 1);
    drop(store);
    assert_eq!(
        (
            common::bodies(&scratch, "workflow"),
            support_summaries(&scratch)
        ),
        before,
        "the workflow and its support rows after the clock has reached the day"
    );
}

/// A support row goes with its summary whatever its own day: in a database where a support row's day and day of going
/// lie 50 days after those of its summary, as only another build or a fault leaves them, the expiry that removes the
/// summary at its bound removes the support row with it, by the sweep of rows that name a summary that is not stored.
/// The row is read as gone by the stored rows of the test's own connection.
#[test]
fn a_support_row_goes_with_its_summary_whatever_its_own_day() {
    let scratch = Scratch::new("support-sweep");
    let clock = TestClock::at_day(DAY);
    let host = || Host::new(Box::new(clock.clone()), Box::new(Grouping { min: 1 }));
    let mut store = open_host(&scratch, host()).unwrap();
    let outcome = store
        .commit_batch(
            &store_id(1),
            &[episode(1, DAY, None)],
            position(1, 0),
            &mut Seq(100),
        )
        .unwrap();
    assert_eq!(outcome.proposals.stored, 1);
    drop(store);
    let moved = raw(&scratch)
        .execute(
            "UPDATE rows SET day = day + 50, gone = gone + 50 WHERE kind = 'support'",
            [],
        )
        .unwrap();
    assert_eq!(moved, 1, "support rows moved");
    clock.set_day(DAY + 93);
    drop(open_host(&scratch, host()).unwrap());
    let conn = raw(&scratch);
    assert_eq!(
        (
            count(&conn, "SELECT count(*) FROM rows WHERE kind = 'summary'"),
            count(&conn, "SELECT count(*) FROM rows WHERE kind = 'support'")
        ),
        (0, 0),
        "(summaries, support rows) after the expiry at the summary's bound"
    );
}

const STORED_MARKER: &str = "qzstored60601";

/// A keyless store under `scratch` with one committed episode whose observed title holds `marker`, and no checkpoint
/// after its commit.
fn one_stored(scratch: &Scratch, marker: &str) -> Store {
    let host = Host::new(Box::new(TestClock::at_day(DAY)), Box::new(NoProposals));
    let mut store = keyless(scratch, &registry(), host);
    let episode = titled(1, DAY, "com.example.browser", &format!("Pay {marker}"));
    let outcome = store
        .commit_batch(&store_id(1), &[episode], position(1, 0), &mut Seq(100))
        .unwrap();
    assert_eq!(outcome.evidence, 1);
    store
}

/// The control of the search in the database file: a marker whose row is stored is found in `derived.db` after a
/// truncating checkpoint, and the search names that entry alone.
#[test]
fn the_search_finds_a_stored_marker_in_the_database_file_after_a_checkpoint() {
    let scratch = Scratch::new("search-db");
    let mut store = one_stored(&scratch, STORED_MARKER);
    assert_eq!(
        store.expire().unwrap(),
        0,
        "an expiry that removes nothing and runs the truncating checkpoint"
    );
    assert_eq!(holding(&scratch, STORED_MARKER), vec!["derived.db"]);
}

/// The control of the search in the write-ahead log: a marker whose row is stored and not yet checkpointed is found
/// in `derived.db-wal`, and the search names that entry alone, while the store is open.
#[test]
fn the_search_finds_a_stored_marker_in_the_log_before_a_checkpoint() {
    let scratch = Scratch::new("search-wal");
    let store = one_stored(&scratch, STORED_MARKER);
    assert_eq!(holding(&scratch, STORED_MARKER), vec!["derived.db-wal"]);
    drop(store);
}

/// The name, the length and the sha256 of every entry of `derived/` under `scratch`, in the order of their names.
fn fingerprint(scratch: &Scratch) -> Vec<(String, u64, String)> {
    let mut entries: Vec<(String, u64, String)> = std::fs::read_dir(scratch.path().join("derived"))
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            let bytes = std::fs::read(entry.path()).unwrap();
            let hash: String = Sha256::digest(&bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            (
                entry.file_name().to_string_lossy().into_owned(),
                bytes.len() as u64,
                hash,
            )
        })
        .collect();
    entries.sort();
    entries
}

/// S-68 (3), both ways: a database that a key made is refused by the keyless open, and a database that the keyless
/// open made is refused by the keyed open, each as `OpenError::Sealed`, and the name, the length and the sha256 of
/// every entry of `derived/` are the same after the refused open as before it.
#[test]
fn the_keyless_open_refuses_a_keyed_database_and_the_keyed_open_a_keyless_one() {
    let host = || Host::new(Box::new(TestClock::at_day(DAY)), Box::new(NoProposals));
    let keyed = Scratch::new("keyed-refused");
    drop(open_store(&keyed, true, &registry(), host()));
    let before = fingerprint(&keyed);
    assert!(before
        .iter()
        .any(|(name, len, _)| name == "derived.db" && *len > 0));
    assert_eq!(
        open_keyless(keyed.path(), &registry(), host(), &mut Seq(1 << 40)).err(),
        Some(OpenError::Sealed),
        "the keyless open of a keyed database"
    );
    assert_eq!(
        fingerprint(&keyed),
        before,
        "the entries of a keyed database after the refused keyless open"
    );
    let plain_made = Scratch::new("keyless-refused");
    drop(open_store(&plain_made, false, &registry(), host()));
    let before = fingerprint(&plain_made);
    assert!(before
        .iter()
        .any(|(name, len, _)| name == "derived.db" && *len > 0));
    assert_eq!(
        Store::open(
            plain_made.path(),
            &key(),
            &registry(),
            host(),
            &mut Seq(1 << 40)
        )
        .err(),
        Some(OpenError::Sealed),
        "the keyed open of a keyless database"
    );
    assert_eq!(
        fingerprint(&plain_made),
        before,
        "the entries of a keyless database after the refused keyed open"
    );
}

const OPEN_BY_BOUND: &str = "qzopenexp70701";
const OPEN_BY_SWEEP: &str = "qzopenswp70702";
const OPEN_KEPT: &str = "qzopenkpt70703";

/// The expiry of an open, searched right after it (ruling S-81, F1): a keyless store holds three evidence rows with a
/// marker each in their observed titles: one whose window ends the next day, which the open's expiry removes by its own
/// bound; one whose summary passes its bound of 93 days at the open, which the open removes with that summary by the
/// sweep of every evidence row whose summary is not stored; and one that stays. Every marker is found while its row is
/// stored, also after the store is closed. Two days later by the host's clock the store is opened again, and right
/// after that open, with the store open and before any other call, the two removed markers are in no entry of
/// `derived/` and the staying one is found. The test's own connection then shows which path each row went.
#[test]
fn the_expiry_of_an_open_leaves_no_marker_of_the_rows_that_it_removed() {
    let scratch = Scratch::new("open-expiry");
    let clock = TestClock::at_day(DAY);
    let host = || Host::new(Box::new(clock.clone()), Box::new(NoProposals));
    let mut store = keyless(&scratch, &registry(), host());
    let mut by_bound = titled(
        1,
        DAY,
        "com.example.window",
        &format!("Pay {OPEN_BY_BOUND}"),
    );
    by_bound.expires = Instant(i64::from(DAY + 1) * DAY_NS);
    let by_sweep = titled(
        2,
        DAY - 92,
        "com.example.swept",
        &format!("Pay {OPEN_BY_SWEEP}"),
    );
    let kept = titled(3, DAY, "com.example.kept", &format!("Pay {OPEN_KEPT}"));
    let outcome = store
        .commit_batch(
            &store_id(1),
            &[by_bound, by_sweep, kept],
            position(1, 0),
            &mut Seq(100),
        )
        .unwrap();
    assert_eq!((outcome.summaries, outcome.evidence), (3, 3));
    drop(store);
    for marker in [OPEN_BY_BOUND, OPEN_BY_SWEEP, OPEN_KEPT] {
        assert!(
            !holding(&scratch, marker).is_empty(),
            "after the close, the search finds {marker}, whose row is stored"
        );
    }
    clock.set_day(DAY + 2);
    let store = keyless(&scratch, &registry(), host());
    for (marker, path) in [
        (OPEN_BY_BOUND, "its own bound"),
        (OPEN_BY_SWEEP, "the sweep"),
    ] {
        assert_eq!(
            holding(&scratch, marker),
            Vec::<&str>::new(),
            "right after the open's expiry, the entries that hold {marker}, whose row the open removed by {path}"
        );
    }
    assert!(
        !holding(&scratch, OPEN_KEPT).is_empty(),
        "right after the open's expiry, the search finds {OPEN_KEPT}, whose row is stored"
    );
    drop(store);
    let conn = plain(&scratch);
    let ids = |kind: &str| -> Vec<String> {
        let mut statement = conn
            .prepare("SELECT summary FROM rows WHERE kind = ?1 ORDER BY summary")
            .unwrap();
        statement
            .query_map([kind], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    let (first, third) = (
        summary_id(1).as_str().to_owned(),
        summary_id(3).as_str().to_owned(),
    );
    assert_eq!(
        (ids("summary"), ids("evidence")),
        (vec![first, third.clone()], vec![third]),
        "(summaries, evidence rows) after the open: the window row went with its summary stored, by its own bound, and \
         the other went with its summary, by the sweep"
    );
}

/// The files under `src/` that hold test code, which the lists of the next test do not read as the store's code.
const TEST_CODE: &[&str] = &["src/tests.rs", "src/test_host.rs"];

/// Every call of the checkpointed write in the store's code: the file, the function that holds the call, and the test
/// of this file that searches every entry of `derived/` right after that call. In the order of the files' names and of
/// the lines.
const CHECKPOINTED_WRITES: &[(&str, &str, &str)] = &[
    (
        "src/db.rs",
        "reconcile",
        "the_expiry_of_an_open_leaves_no_marker_of_the_rows_that_it_removed",
    ),
    ("src/decide.rs", "decide", "privacy_listed"),
    ("src/decide.rs", "decide", "privacy_scrub"),
    ("src/purge.rs", "purge", "privacy_scrub"),
    ("src/purge.rs", "purge_below", "privacy_scrub"),
    ("src/report.rs", "expire", "privacy_scrub"),
];

/// Every place of the store's code that removes a row: a call of the transaction's `remove`, or a run of a statement
/// of `src/sql.rs` whose text begins with `DELETE`. The file, the function that holds it, what it calls, and the
/// checkpointed write under which it runs, or the sentence of the contract that allows it outside one. In the order of
/// the files' names and of the lines.
const REMOVALS: &[(&str, &str, &str, &str)] = &[
    ("src/db.rs", "remove", "sql::ROW_DELETE", "under the write of each remove below"),
    ("src/db.rs", "raise_mark", "sql::PURGED_BELOW_DELETE", "purge and purge_below; the purged ordinals hold no text (C2.7)"),
    ("src/db.rs", "raise_mark", "sql::PURGED_BELOW_DELETE", "purge and purge_below; the purged ordinals hold no text (C2.7)"),
    ("src/db.rs", "expire", "remove", "the expiry: src/report.rs expire and src/db.rs reconcile"),
    ("src/db.rs", "expire", "remove", "the expiry: src/report.rs expire and src/db.rs reconcile"),
    ("src/db.rs", "expire", "remove", "the expiry: src/report.rs expire and src/db.rs reconcile"),
    ("src/db.rs", "expire", "remove", "the expiry: src/report.rs expire and src/db.rs reconcile"),
    ("src/db.rs", "sweep", "remove", "the expiry and the open: src/report.rs expire and src/db.rs reconcile"),
    ("src/db.rs", "sweep", "remove", "the expiry and the open: src/report.rs expire and src/db.rs reconcile"),
    ("src/decide.rs", "dismiss", "remove", "src/decide.rs decide, the dismissal"),
    ("src/decide.rs", "delete", "remove", "src/decide.rs decide, the delete"),
    ("src/decide.rs", "delete", "remove", "src/decide.rs decide, the delete"),
    (
        "src/propose.rs",
        "set_support",
        "sql::ROW_DELETE",
        "outside a checkpointed write: C2.4, ruling S-80, a support row that a new proposal drops lies in the files \
         until the next checkpointed call",
    ),
    ("src/purge.rs", "purge", "remove", "src/purge.rs purge"),
    ("src/purge.rs", "purge_below", "remove", "src/purge.rs purge_below"),
];

/// The name of the function that a line of Rust declares, if it declares one.
fn declared(line: &str) -> Option<String> {
    let mut rest = line.trim_start();
    for prefix in ["pub(crate) ", "pub ", "const ", "unsafe ", "async "] {
        rest = rest.strip_prefix(prefix).unwrap_or(rest);
    }
    let rest = rest.strip_prefix("fn ")?;
    let end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))?;
    Some(rest[..end].to_owned())
}

/// The places of the store's code, read from its source at run time: every call of the checkpointed write, and every
/// removal, each with the function that holds it.
/// A call of the checkpointed write: the file and the function that holds it.
type WritePlace = (String, String);
/// A removal: the file, the function that holds it and what it calls.
type RemovalPlace = (String, String, String);

fn places() -> (Vec<WritePlace>, Vec<RemovalPlace>) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let sql = std::fs::read_to_string(root.join("src").join("sql.rs")).unwrap();
    let deleting: Vec<String> = sql
        .lines()
        .filter_map(|line| {
            let (name, text) = line.trim_start().split_once(" = ")?;
            text.starts_with("\"DELETE").then(|| format!("sql::{name}"))
        })
        .collect();
    assert!(
        !deleting.is_empty(),
        "the removing statements of src/sql.rs"
    );
    let mut files: Vec<String> = std::fs::read_dir(root.join("src"))
        .unwrap()
        .map(|entry| format!("src/{}", entry.unwrap().file_name().to_string_lossy()))
        .filter(|name| !TEST_CODE.contains(&name.as_str()))
        .collect();
    files.sort();
    let (mut writes, mut removals) = (Vec::new(), Vec::new());
    for file in files {
        let text = std::fs::read_to_string(root.join(&file))
            .unwrap_or_else(|e| panic!("{file} cannot be read: {e}"));
        let mut function = String::new();
        for line in text.lines() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            if let Some(name) = declared(line) {
                function = name;
            }
            if !line.contains("fn write_checkpointed") {
                for _ in line.matches("write_checkpointed(") {
                    writes.push((file.clone(), function.clone()));
                }
            }
            for pattern in ["tx.remove(", "self.remove("] {
                for _ in line.matches(pattern) {
                    removals.push((file.clone(), function.clone(), "remove".to_owned()));
                }
            }
            for statement in &deleting {
                for (at, _) in line.match_indices(statement.as_str()) {
                    let after = line[at + statement.len()..].chars().next();
                    if !after.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
                        removals.push((file.clone(), function.clone(), statement.clone()));
                    }
                }
            }
        }
    }
    (writes, removals)
}

/// The places of the store's code that remove what a person wrote, read from the crate's source at run time and held
/// as two closed lists (ruling S-81, F2). Every call of the checkpointed write: `Store::expire` (src/report.rs), the
/// reconcile step of every open (src/db.rs), `Store::purge` and `Store::purge_below` (src/purge.rs), and the dismissal
/// and the delete of `Store::decide` (src/decide.rs), each beside the test of this file that searches every entry of
/// `derived/` right after it. Every removal: the transaction's `remove` in the expiry and the sweep (src/db.rs), the
/// dismissal and the delete (src/decide.rs) and the two purges (src/purge.rs), the statement that `remove` runs, the
/// purged ordinals of `raise_mark`, and the support row that a new proposal drops in `set_support` (src/propose.rs),
/// each beside the checkpointed write under which it runs or the sentence of the contract that allows it outside one.
/// The test fails when a place appears that a list does not hold, when a listed place is gone, and when a listed test
/// is no test of this file: a new place needs its search right after it before a list may gain it. Its limits: it
/// reads text, so a removal written as a statement that does not begin with `DELETE`, or behind a macro or a built
/// string, is not seen; nor are these forms, named by the readers of ruling S-82 (item 4 (c)): a new call written over
/// two lines, a call through another name of the transaction, a listed removal run from a new place under a plain
/// write, a remove call on another receiver name, and an alias inside braces.
#[test]
fn every_place_that_removes_or_checkpoints_stands_on_a_list_with_its_search() {
    let (writes, removals) = places();
    let listed_writes: Vec<WritePlace> = CHECKPOINTED_WRITES
        .iter()
        .map(|(file, function, _)| (file.to_string(), function.to_string()))
        .collect();
    assert_eq!(writes, listed_writes, "the calls of the checkpointed write");
    let listed_removals: Vec<RemovalPlace> = REMOVALS
        .iter()
        .map(|(file, function, what, _)| (file.to_string(), function.to_string(), what.to_string()))
        .collect();
    assert_eq!(removals, listed_removals, "the removals");
    let own = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("privacy.rs"),
    )
    .unwrap()
    .replace("\r\n", "\n");
    for (_, _, test) in CHECKPOINTED_WRITES {
        assert!(
            own.contains(&format!("#[test]\nfn {test}(")),
            "{test} is a test of tests/privacy.rs"
        );
    }
}

const REBUILT_EXPIRY: &str = "qzrebuiltexp81";
const REBUILT_PURGE: &str = "qzrebuiltpur82";
const REBUILT_DELETE: &str = "qzrebuiltdel83";
const REBUILT_OPEN: &str = "qzrebuiltopn84";
const REBUILT_ALL: &str = "qzrebuiltall85";

/// The byte offsets in `derived.db` under `scratch` at which `marker` stands in UTF-8.
fn offsets(scratch: &Scratch, marker: &str) -> Vec<usize> {
    let bytes = std::fs::read(scratch.path().join("derived").join("derived.db")).unwrap();
    bytes
        .windows(marker.len())
        .enumerate()
        .filter(|(_, w)| *w == marker.as_bytes())
        .map(|(at, _)| at)
        .collect()
}

/// The id of the stored workflow whose app is `app`.
fn workflow_of(store: &Store, app: &str) -> String {
    listed(store)
        .into_iter()
        .find(|item| matches!(&item.workflow, Workflow::Full(full) if full.app.as_str() == app))
        .map(|item| item.workflow.id().as_str().to_owned())
        .unwrap()
}

/// A removed row on a page that the database built again (rulings S-80, item 5, and S-81, F3). In a keyless store,
/// five rows that hold a marker each (an evidence row past its window at the next expiry, an evidence row whose
/// segment is purged, a row of origin own of a workflow that is deleted, an evidence row past its window at the next
/// open, and an evidence row that stays until the delete of everything) stand among about a hundred rows of another
/// workflow, of origin own, with made-up text of mixed lengths. The delete of that workflow removes those rows, and the
/// database rebalances the pages that held them and builds them again: the byte offset of every marker in
/// `derived.db` moves, which shows that its row's page was built again. Then each removal runs in turn (the expiry, the
/// purge, the delete of a workflow, the expiry of an open, the delete of everything), and right after each the marker
/// of the row that it took is in no entry of `derived/`, while every marker whose row stands is still found.
#[test]
fn a_removed_row_on_a_page_that_the_database_built_again_leaves_no_marker() {
    let scratch = Scratch::new("rebuilt-page");
    let clock = TestClock::at_day(DAY);
    let host = || {
        Host::new(Box::new(clock.clone()), Box::new(Grouping { min: 1 })).handler(
            "test",
            Takes::Runner,
            Box::new(OwnNote),
        )
    };
    let entries = with(&[&OWN]);
    let mut store = keyless(&scratch, &entries, host());
    let mut at = 1;
    let mut batch = |store: &mut Store, episodes: &[Episode]| {
        at += 1;
        store
            .commit_batch(&store_id(1), episodes, position(at, 0), &mut Seq(100 * at))
            .unwrap()
    };
    batch(
        &mut store,
        &[
            titled(1, DAY, "com.example.fill", "Fill"),
            titled(2, DAY, "com.example.gone", "Gone"),
        ],
    );
    let (fill, gone) = (
        workflow_of(&store, "com.example.fill"),
        workflow_of(&store, "com.example.gone"),
    );
    let mark = |n: u64, marker: &str, expires_day: Option<u32>| {
        let mut episode = titled(n, DAY, "com.example.mark", &format!("Pay {marker}"));
        if let Some(day) = expires_day {
            episode.expires = Instant(i64::from(day) * DAY_NS);
        }
        episode
    };
    for i in 0..48u64 {
        for k in 0..2u64 {
            let len = 40 + ((i * 2 + k) * 137 % 700) as usize;
            let note = format!("fill {i:03} {k} {}", "x".repeat(len));
            store
                .handle(&raw_request("runner", &own_note_body(&fill, &note)))
                .unwrap();
        }
        match i {
            12 => {
                batch(&mut store, &[mark(10, REBUILT_EXPIRY, Some(DAY + 1))]);
            }
            20 => {
                batch(&mut store, &[mark(11, REBUILT_PURGE, None)]);
            }
            28 => {
                store
                    .handle(&raw_request(
                        "runner",
                        &own_note_body(&gone, &format!("Note {REBUILT_DELETE}")),
                    ))
                    .unwrap();
            }
            36 => {
                batch(&mut store, &[mark(12, REBUILT_OPEN, Some(DAY + 3))]);
            }
            44 => {
                batch(&mut store, &[mark(13, REBUILT_ALL, None)]);
            }
            _ => {}
        }
    }
    let markers = [
        REBUILT_EXPIRY,
        REBUILT_PURGE,
        REBUILT_DELETE,
        REBUILT_OPEN,
        REBUILT_ALL,
    ];
    assert_eq!(
        store.expire().unwrap(),
        0,
        "a checkpoint that removes nothing"
    );
    let before: Vec<Vec<usize>> = markers.iter().map(|m| offsets(&scratch, m)).collect();
    let pages_before = std::fs::metadata(scratch.db()).unwrap().len() / 4096;
    store
        .decide(&InterfaceDecision::delete(
            &InterfaceRole::claim(),
            RequestId::from_random([5; 16]),
            WorkflowId::parse(&fill).unwrap(),
        ))
        .unwrap();
    let after: Vec<Vec<usize>> = markers.iter().map(|m| offsets(&scratch, m)).collect();
    let pages_after = std::fs::metadata(scratch.db()).unwrap().len() / 4096;
    println!("rebuilt-page: pages of derived.db before {pages_before}, after {pages_after}");
    for (marker, (b, a)) in markers.iter().zip(before.iter().zip(after.iter())) {
        println!(
            "rebuilt-page: {marker}: offsets before {b:?} (pages {:?}), after {a:?} (pages {:?})",
            b.iter().map(|o| o / 4096).collect::<Vec<_>>(),
            a.iter().map(|o| o / 4096).collect::<Vec<_>>()
        );
        assert!(
            !a.is_empty(),
            "{marker} stands in derived.db while its row is stored"
        );
        assert_ne!(
            b, a,
            "the offsets of {marker} move when the database builds its page again"
        );
    }
    let after_each = |removal: &str, gone_markers: &[&str]| {
        for marker in markers {
            let found = holding(&scratch, marker);
            if gone_markers.contains(&marker) {
                assert_eq!(
                    found,
                    Vec::<&str>::new(),
                    "right after {removal} on a page built again, the entries that hold {marker}, whose row is removed"
                );
            } else {
                assert!(
                    !found.is_empty(),
                    "right after {removal} on a page built again, the search finds {marker}, whose row is stored"
                );
            }
        }
    };
    clock.set_day(DAY + 2);
    assert!(
        store.expire().unwrap() >= 1,
        "the expiry removes the window row"
    );
    after_each("the expiry", &[REBUILT_EXPIRY]);
    assert_eq!(
        store.purge(&[ord(11)]).unwrap(),
        1,
        "the purge removes one row"
    );
    after_each("the purge", &[REBUILT_EXPIRY, REBUILT_PURGE]);
    store
        .decide(&InterfaceDecision::delete(
            &InterfaceRole::claim(),
            RequestId::from_random([6; 16]),
            WorkflowId::parse(&gone).unwrap(),
        ))
        .unwrap();
    after_each(
        "the delete of a workflow",
        &[REBUILT_EXPIRY, REBUILT_PURGE, REBUILT_DELETE],
    );
    drop(store);
    clock.set_day(DAY + 4);
    let store = keyless(&scratch, &entries, host());
    after_each(
        "the expiry of an open",
        &[REBUILT_EXPIRY, REBUILT_PURGE, REBUILT_DELETE, REBUILT_OPEN],
    );
    delete_all(store).unwrap();
    assert!(
        !scratch.path().join("derived").exists(),
        "right after the delete of everything, derived/ with every entry that could hold {REBUILT_ALL}"
    );
}

/// A drawn sequence of numbers, the same for the same seed (xorshift).
struct Draw(u64);

impl Draw {
    fn new(seed: u64) -> Draw {
        Draw(0x9e37_79b9_7f4a_7c15 ^ seed.wrapping_mul(0x1_0001_0001))
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    /// A length of made-up text from 20 to 950 bytes, short, middling and long in a drawn mix.
    fn length(&mut self) -> usize {
        (match self.below(6) {
            0 => 20 + self.below(60),
            1 => 700 + self.below(250),
            _ => 80 + self.below(500),
        }) as usize
    }
}

/// A test handler that commits, in one typed commit, one record of `test_own` for each note and one record of the
/// window kind `test_trace` for each trace of the body
/// `{"workflow":<id>,"notes":[<text>,...],"traces":[[<segment>,<expires>],...]}` of its request.
struct OwnNotes;

impl halvo_derived::Handler for OwnNotes {
    fn handle(
        &self,
        request: &halvo_schema_core::request::RawRequest,
        cx: &mut halvo_derived::HandlerCx<'_>,
    ) -> Result<Box<serde_json::value::RawValue>, ErrorName> {
        let body: serde_json::Value =
            serde_json::from_str(request.body().get()).map_err(|_| ErrorName::Malformed)?;
        let workflow = &body["workflow"];
        let mut records: Vec<TypedRecord> = Vec::new();
        for note in body["notes"].as_array().ok_or(ErrorName::Malformed)? {
            records.push(TypedRecord {
                kind: "test_own",
                json: format!(
                    r#"{{"v":1,"workflow":{workflow},"note":{note},"at":1794339000000000000}}"#
                ),
            });
        }
        for trace in body["traces"].as_array().ok_or(ErrorName::Malformed)? {
            records.push(TypedRecord {
                kind: "test_trace",
                json: format!(
                    r#"{{"v":1,"workflow":{workflow},"segments":[{}],"expires":{}}}"#,
                    trace[0], trace[1]
                ),
            });
        }
        cx.commit(&records).map_err(|_| ErrorName::State)?;
        Ok(serde_json::value::RawValue::from_string("null".to_owned()).unwrap())
    }
}

/// The six calls of the checkpointed write whose removals the page-gap test drives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Removal {
    Delete,
    Dismissal,
    Open,
    Expiry,
    Purge,
    PurgeBelow,
}

impl Removal {
    fn letter(self) -> char {
        match self {
            Removal::Delete => 'd',
            Removal::Dismissal => 's',
            Removal::Open => 'o',
            Removal::Expiry => 'e',
            Removal::Purge => 'p',
            Removal::PurgeBelow => 'b',
        }
    }

    fn name(self) -> &'static str {
        match self {
            Removal::Delete => "delete-of-a-workflow",
            Removal::Dismissal => "dismissal",
            Removal::Open => "expiry-of-an-open",
            Removal::Expiry => "expiry-call",
            Removal::Purge => "purge",
            Removal::PurgeBelow => "purge-below",
        }
    }
}

/// What one drawn store showed: the removals made, the taken markers found after their removal, the stored markers
/// found in more places than their row and its index entry, and the rows in which the store differs from the model.
#[derive(Default)]
struct GapCount {
    removals: u64,
    left: u64,
    copies: u64,
    model: u64,
    unseen: u64,
}

/// The name and the bytes of every entry of `derived/` under `scratch`, read once.
fn entries_of(scratch: &Scratch) -> Vec<(String, Vec<u8>)> {
    std::fs::read_dir(scratch.path().join("derived"))
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.file_name().to_string_lossy().into_owned(),
                std::fs::read(entry.path()).unwrap_or_default(),
            )
        })
        .collect()
}

/// The whole made-up markers `vxpg<letter><seed>r<serial>q` in `files`, with the number of places of each.
fn drawn_markers(files: &[(String, Vec<u8>)], letter: char, seed: u64) -> BTreeMap<String, u64> {
    let stem = format!("vxpg{letter}{seed:02}r");
    let mut found = BTreeMap::new();
    for (_, bytes) in files {
        let mut at = 0;
        while at + 14 <= bytes.len() {
            let piece = &bytes[at..at + 14];
            if piece.starts_with(stem.as_bytes())
                && piece[8..13].iter().all(u8::is_ascii_digit)
                && piece[13] == b'q'
            {
                *found
                    .entry(String::from_utf8_lossy(piece).into_owned())
                    .or_insert(0) += 1;
                at += 14;
            } else {
                at += 1;
            }
        }
    }
    found
}

/// What a taken text of the page-gap test is, read from its form.
fn what_it_is(text: &str) -> &'static str {
    if text.starts_with("vxpg") {
        "a drawn marker of a row of origin own or of an evidence row"
    } else if text.starts_with(r#"{"v":1,"id":"#) {
        "the full shape of the workflow's row"
    } else if text.contains(r#""segments":"#) {
        "the body of a row of the window kind test_trace"
    } else if text.starts_with(r#"{"v":1,"workflow":"#) {
        "the body of a support row"
    } else if text.ends_with("support") {
        "a support row's entry (summary, kind) in the index rows_summary"
    } else if text.ends_with("evidence") {
        "an evidence row's entry (summary, kind) in the index rows_summary"
    } else if text.ends_with(".test") {
        "the made-up domain of the workflow's site"
    } else {
        "a support row's entry (kind, workflow, summary) in the index rows_kind_workflow"
    }
}

/// The names of the entries of `files` in which `text` stands whole.
fn standing_in(files: &[(String, Vec<u8>)], text: &str) -> Vec<String> {
    files
        .iter()
        .filter(|(_, bytes)| bytes.windows(text.len()).any(|w| w == text.as_bytes()))
        .map(|(name, _)| name.clone())
        .collect()
}

/// Whether `text` stands whole in one of `files`.
fn stands_in(files: &[(String, Vec<u8>)], text: &str) -> bool {
    !standing_in(files, text).is_empty()
}

/// The model of a drawn store: the stored rows of origin own by marker, the stored evidence by marker with its
/// summary's serial and the day of its window, and the stored rows of `test_trace` by their exact body, each with its
/// workflow, its segment and the day of its window.
#[derive(Default)]
struct Model {
    own: BTreeMap<String, u64>,
    evidence: BTreeMap<String, (u64, u64, u32)>,
    traces: BTreeMap<String, (u64, u64, u32)>,
}

/// One drawn store for one removal. Twelve workflows: eight that the removals take from, four of them with a made-up
/// domain as their site, and four fillers whose rows stand among theirs. Rows of origin own and evidence with made-up
/// text of 20 to 950 bytes, and rows of the window kind `test_trace`, are written in a drawn order, 240 at the start
/// and 30 between the removals. Before each removal of a kind other than the delete of a workflow one filler is deleted,
/// so that pages are merged and built again while the rows that the removal will take live on them. After each
/// removal, with the store open and again after the close, every entry of `derived/` is read: the whole markers and
/// exact bodies that the removal took (for the dismissal and the delete, the bodies of the support rows and of the full
/// shape of the workflow; for the delete, its site's domain too) are counted where they stand, each stored marker is
/// counted against its places, and the stored rows are compared with the model through the test's own connection.
fn page_gap_store(removal: Removal, seed: u64) -> GapCount {
    let letter = removal.letter();
    let mut draw = Draw::new(seed * 8 + u64::from(letter as u8));
    let scratch = Scratch::new("page-gap");
    let clock = TestClock::at_day(DAY);
    let host = || {
        Host::new(Box::new(clock.clone()), Box::new(Grouping { min: 1 })).handler(
            "test",
            Takes::Runner,
            Box::new(OwnNotes),
        )
    };
    let entries = with(&[&OWN, &TRACE]);
    let mut store = keyless(&scratch, &entries, host());
    let sites: Vec<Option<String>> = (0..12u64)
        .map(|k| (k % 2 == 0 && k < 8).then(|| format!("vxpgsite{letter}{seed:02}k{k}.test")))
        .collect();
    let windowed = matches!(removal, Removal::Open | Removal::Expiry);
    let own_share = if removal == Removal::Dismissal { 4 } else { 7 };
    let mut serial = 0u64;
    let mut at = 0u64;
    let episode_of =
        |k: u64, serial: u64, marker: Option<&str>, expires_day: Option<u32>, draw: &mut Draw| {
            let fill = ["ab", "cd", "ef", "gh"][draw.below(4) as usize].repeat(draw.length() / 2);
            let title = format!("{} {fill}", marker.unwrap_or("Pay"));
            let mut e = titled(serial, DAY, &format!("com.example.pg{k}"), &title);
            e.site = sites[k as usize].as_deref().map(|s| domain(s).unwrap());
            if let Some(day) = expires_day {
                e.expires = Instant(i64::from(day) * DAY_NS);
            }
            e
        };
    let mut batch = |store: &mut Store, episodes: Vec<Episode>| {
        at += 1;
        store
            .commit_batch(
                &store_id(1),
                &episodes,
                position(at, 0),
                &mut Seq(1000 * at),
            )
            .unwrap();
    };
    let first: Vec<Episode> = (0..12u64)
        .map(|k| {
            serial += 1;
            episode_of(k, serial, None, None, &mut draw)
        })
        .collect();
    batch(&mut store, first);
    let mut ids: BTreeMap<u64, String> = BTreeMap::new();
    for item in listed(&store) {
        if let Workflow::Full(full) = &item.workflow {
            let k: u64 = full
                .app
                .as_str()
                .trim_start_matches("com.example.pg")
                .parse()
                .unwrap();
            ids.insert(k, item.workflow.id().as_str().to_owned());
        }
    }
    assert_eq!(ids.len(), 12);
    let mut live: Vec<u64> = (0..12).collect();
    let mut takeable: Vec<u64> = (0..8).collect();
    let mut fillers: Vec<u64> = (8..12).collect();
    let mut model = Model::default();
    let mut today = DAY;
    let mut count = GapCount::default();

    let mut write = |store: &mut Store,
                     draw: &mut Draw,
                     live: &[u64],
                     model: &mut Model,
                     today: u32,
                     rows: u64| {
        let mut written = 0;
        while written < rows {
            let k = live[draw.below(live.len() as u64) as usize];
            if draw.below(10) < own_share {
                let (mut notes, mut traces) = (Vec::new(), Vec::new());
                for _ in 0..1 + draw.below(4) {
                    serial += 1;
                    let marker = format!("vxpg{letter}{seed:02}r{serial:05}q");
                    let fill =
                        ["ij", "kl", "mn", "op"][draw.below(4) as usize].repeat(draw.length() / 2);
                    notes.push(format!("{marker} {fill}"));
                    model.own.insert(marker, k);
                }
                for _ in 0..draw.below(3) {
                    serial += 1;
                    let day = if windowed {
                        today + 1 + draw.below(5) as u32
                    } else {
                        DAY + 400
                    };
                    let expires = i64::from(day) * DAY_NS + 7_000 + serial as i64;
                    traces.push(serde_json::json!([serial, expires]));
                    let body = format!(
                        r#"{{"v":1,"workflow":"{}","segments":[{serial}],"expires":{expires}}}"#,
                        ids[&k]
                    );
                    model.traces.insert(body, (k, serial, day));
                }
                written += (notes.len() + traces.len()) as u64;
                let body =
                    serde_json::json!({"workflow": ids[&k], "notes": notes, "traces": traces})
                        .to_string();
                store.handle(&raw_request("runner", &body)).unwrap();
            } else {
                serial += 1;
                let marker = format!("vxpg{letter}{seed:02}r{serial:05}q");
                let expires = windowed.then(|| today + 1 + draw.below(5) as u32);
                let e = episode_of(k, serial, Some(&marker), expires, draw);
                model
                    .evidence
                    .insert(marker, (k, serial, expires.unwrap_or(u32::MAX)));
                batch(store, vec![e]);
                written += 1;
            }
        }
    };

    write(&mut store, &mut draw, &live, &mut model, today, 240);
    for _ in 0..4 {
        if removal != Removal::Delete && !fillers.is_empty() {
            let filler = fillers.remove(0);
            store
                .decide(&InterfaceDecision::delete(
                    &InterfaceRole::claim(),
                    RequestId::from_random([8; 16]),
                    WorkflowId::parse(&ids[&filler]).unwrap(),
                ))
                .unwrap();
            live.retain(|l| *l != filler);
            model.own.retain(|_, of| *of != filler);
            model.evidence.retain(|_, (of, _, _)| *of != filler);
            model.traces.retain(|_, (of, _, _)| *of != filler);
        }
        let mut taken_markers: Vec<String> = Vec::new();
        let mut taken_texts: Vec<String> = Vec::new();
        let evidence_entry = |n: u64| format!("{}evidence", summary_id(n).as_str());
        let act: Box<dyn FnOnce(&mut Store)> = match removal {
            Removal::Delete | Removal::Dismissal => {
                if takeable.len() < 3 {
                    break;
                }
                let k = takeable.remove(draw.below(takeable.len() as u64) as usize);
                let item = listed(&store)
                    .into_iter()
                    .find(|item| item.workflow.id().as_str() == ids[&k])
                    .unwrap();
                let workflow = item.workflow.id().clone();
                taken_texts.push(serde_json::to_string(&item.workflow).unwrap());
                for summary in &item.summaries {
                    let support = halvo_schema_core::record::Support {
                        workflow: workflow.clone(),
                        summary: summary.clone(),
                        day: DAY,
                    };
                    taken_texts.push(serde_json::to_string(&support).unwrap());
                    taken_texts.push(format!("{}{}", workflow.as_str(), summary.as_str()));
                    taken_texts.push(format!("{}support", summary.as_str()));
                }
                if removal == Removal::Delete {
                    live.retain(|l| *l != k);
                    taken_texts.extend(sites[k as usize].clone());
                    taken_markers.extend(
                        model
                            .own
                            .iter()
                            .filter(|(_, of)| **of == k)
                            .map(|(m, _)| m.clone()),
                    );
                    for (marker, (of, n, _)) in &model.evidence {
                        if *of == k {
                            taken_markers.push(marker.clone());
                            taken_texts.push(evidence_entry(*n));
                        }
                    }
                    taken_texts.extend(
                        model
                            .traces
                            .iter()
                            .filter(|(_, (of, _, _))| *of == k)
                            .map(|(b, _)| b.clone()),
                    );
                }
                let decision = if removal == Removal::Delete {
                    InterfaceDecision::delete
                } else {
                    InterfaceDecision::dismiss
                };
                Box::new(move |store: &mut Store| {
                    store
                        .decide(&decision(
                            &InterfaceRole::claim(),
                            RequestId::from_random([7; 16]),
                            workflow,
                        ))
                        .unwrap();
                })
            }
            Removal::Expiry | Removal::Open => {
                today += 1;
                for (marker, (_, n, d)) in &model.evidence {
                    if *d <= today {
                        taken_markers.push(marker.clone());
                        taken_texts.push(evidence_entry(*n));
                    }
                }
                taken_texts.extend(
                    model
                        .traces
                        .iter()
                        .filter(|(_, (_, _, d))| *d <= today)
                        .map(|(b, _)| b.clone()),
                );
                Box::new(|store: &mut Store| {
                    store.expire().unwrap();
                })
            }
            Removal::Purge => {
                let mut ordinals = Vec::new();
                for (marker, (_, n, _)) in &model.evidence {
                    if draw.below(3) == 0 {
                        ordinals.push(ord(*n));
                        taken_markers.push(marker.clone());
                        taken_texts.push(evidence_entry(*n));
                    }
                }
                for (body, (_, n, _)) in &model.traces {
                    if draw.below(3) == 0 {
                        ordinals.push(ord(*n));
                        taken_texts.push(body.clone());
                    }
                }
                Box::new(move |store: &mut Store| {
                    store.purge(&ordinals).unwrap();
                })
            }
            Removal::PurgeBelow => {
                let mut segments: Vec<u64> = model
                    .evidence
                    .values()
                    .chain(model.traces.values())
                    .map(|(_, n, _)| *n)
                    .collect();
                segments.sort();
                let first_live = segments[segments.len() / 3] + 1;
                for (marker, (_, n, _)) in &model.evidence {
                    if *n < first_live {
                        taken_markers.push(marker.clone());
                        taken_texts.push(evidence_entry(*n));
                    }
                }
                taken_texts.extend(
                    model
                        .traces
                        .iter()
                        .filter(|(_, (_, n, _))| *n < first_live)
                        .map(|(b, _)| b.clone()),
                );
                Box::new(move |store: &mut Store| {
                    store.purge_below(ord(first_live)).unwrap();
                })
            }
        };
        let before = entries_of(&scratch);
        let seen = drawn_markers(&before, letter, seed);
        count.unseen += taken_markers
            .iter()
            .filter(|m| !seen.contains_key(*m))
            .count() as u64;
        count.unseen += taken_texts
            .iter()
            .filter(|t| !stands_in(&before, t))
            .count() as u64;
        drop(before);
        if removal == Removal::Open {
            clock.set_day(today);
            drop(store);
            store = keyless(&scratch, &entries, host());
        } else {
            if removal == Removal::Expiry {
                clock.set_day(today);
            }
            act(&mut store);
        }
        count.removals += 1;
        for marker in &taken_markers {
            model.own.remove(marker);
            model.evidence.remove(marker);
        }
        for body in &taken_texts {
            model.traces.remove(body);
        }
        let mut left: BTreeSet<String> = BTreeSet::new();
        let scan = |left: &mut BTreeSet<String>, count: &mut GapCount, model: &Model| {
            let files = entries_of(&scratch);
            let found = drawn_markers(&files, letter, seed);
            for text in taken_markers.iter().chain(taken_texts.iter()) {
                let entries = standing_in(&files, text);
                if !entries.is_empty() && left.insert(text.clone()) {
                    println!(
                        "PAGE-GAP {} seed {seed}: left after its removal: {} in {}",
                        removal.name(),
                        what_it_is(text),
                        entries.join(" and ")
                    );
                }
            }
            for (marker, places) in &found {
                let allowed = if model.own.contains_key(marker) {
                    2
                } else if model.evidence.contains_key(marker) {
                    1
                } else {
                    continue;
                };
                if *places > allowed {
                    count.copies += 1;
                }
            }
        };
        scan(&mut left, &mut count, &model);
        drop(store);
        scan(&mut left, &mut count, &model);
        let conn = plain(&scratch);
        let mut statement = conn
            .prepare(
                "SELECT kind, body FROM rows WHERE kind IN ('test_own', 'evidence', 'test_trace')",
            )
            .unwrap();
        let rows: Vec<(String, String)> = statement
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        drop(statement);
        drop(conn);
        let stem = format!("vxpg{letter}{seed:02}r");
        let mut stored: BTreeSet<String> = BTreeSet::new();
        for (kind, body) in &rows {
            if kind == "test_trace" {
                stored.insert(body.clone());
            } else if let Some(at) = body.find(&stem) {
                stored.insert(body[at..(at + 14).min(body.len())].to_owned());
            }
        }
        let expected: BTreeSet<String> = model
            .own
            .keys()
            .chain(model.evidence.keys())
            .chain(model.traces.keys())
            .cloned()
            .collect();
        count.model += stored.symmetric_difference(&expected).count() as u64;
        count.left += left.len() as u64;
        store = keyless(&scratch, &entries, host());
        write(&mut store, &mut draw, &live, &mut model, today, 30);
    }
    drop(store);
    count
}

/// The seeds of the page-gap test, for each removal: chosen so that, at a head whose build of the database leaves the
/// gap of a rebuilt page as it was, each removal leaves at least one marker (9087f2d7 on this Mac: a drawn search over
/// seeds 1 to 24 for the dismissal and 1 to 16 for the others).
const PAGE_GAP_SEEDS: &[(Removal, &[u64])] = &[
    (Removal::Delete, &[1, 2, 3, 4, 5, 6, 7, 8]),
    (Removal::Dismissal, &[1, 2, 3, 4, 5, 6, 7, 8]),
    (Removal::Open, &[2, 12, 1, 3]),
    (Removal::Expiry, &[4, 5, 1, 2]),
    (Removal::Purge, &[15, 1, 2, 3]),
    (Removal::PurgeBelow, &[1, 6, 2, 3]),
];

/// Removed rows leave no text in the gap of a page that the database built again (rulings S-80, item 5, S-81, F3, and
/// S-82, item 4, written from the verifier's probe evidence/verifier-37/v37r.rs). For each of the six calls of the
/// checkpointed write that `every_place_that_removes_or_checkpoints_stands_on_a_list_with_its_search` lists (the
/// delete of a workflow, the dismissal, the expiry of an open, the expiry call, the purge and the purge below the first
/// live ordinal), drawn keyless stores with fixed seeds, 8 for the delete and the dismissal and 4 for each of the
/// others, each as `page_gap_store` builds it, four stores at a time on threads of their own. The test prints one line per removal with its counts: the stores, the
/// removals, the taken markers and bodies found in an entry of `derived/` after their removal, the stored markers found
/// in more places than their row and its index entry, and the stored rows that differ from the model. It fails when one
/// count but the first two is above 0. At a head whose build of the database leaves the gap of a rebuilt page as it
/// was, it fails: that is the finding of ruling S-82, which the mend of the database's build closes.
#[test]
fn removed_rows_leave_no_text_in_the_gap_of_a_page_built_again() {
    let mut lines = Vec::new();
    let mut failed = false;
    for (removal, seeds) in PAGE_GAP_SEEDS {
        let mut total = GapCount::default();
        let mut counts = Vec::new();
        for four in seeds.chunks(4) {
            counts.extend(std::thread::scope(|scope| {
                let runs: Vec<_> = four
                    .iter()
                    .map(|seed| scope.spawn(move || page_gap_store(*removal, *seed)))
                    .collect();
                runs.into_iter()
                    .map(|run| run.join().unwrap())
                    .collect::<Vec<GapCount>>()
            }));
        }
        for count in counts {
            total.removals += count.removals;
            total.left += count.left;
            total.copies += count.copies;
            total.model += count.model;
            total.unseen += count.unseen;
        }
        let line = format!(
            "PAGE-GAP {}: stores {}, removals {}, markers-left {}, extra-copies {}, model-differences {}, \
             unseen-before-removal {}",
            removal.name(),
            seeds.len(),
            total.removals,
            total.left,
            total.copies,
            total.model,
            total.unseen
        );
        println!("{line}");
        failed |= total.left > 0 || total.copies > 0 || total.model > 0 || total.unseen > 0;
        lines.push(line);
    }
    assert!(
        !failed,
        "removed rows left text in the files:\n{}",
        lines.join("\n")
    );
}
