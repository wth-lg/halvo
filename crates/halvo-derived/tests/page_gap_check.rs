mod common;

#[path = "../../halvo-sqlcipher/tests/page_gap/rules.rs"]
mod rules;

use std::cell::RefCell;
use std::rc::Rc;

use common::{
    domain, episode, listed, own_note_body, position, raw_request, registry, store_id, summary_id,
    Fixed, Grouping, OwnNote, Scratch, Seq, TestClock, DAY_NS, FAR, NOTE, OWN, TRACE,
};
use halvo_derived::test_host::open_keyless;
use halvo_derived::{Handler, HandlerCx, Host, Store, Takes};
use halvo_schema_core::category::SeedCategory;
use halvo_schema_core::ids::{RequestId, SummaryId, WorkflowId};
use halvo_schema_core::kinds::KindEntry;
use halvo_schema_core::record::{
    Confidence, Episode, FloorLayer, Ordinal, Proposal, Recurrence, Tagged, Text, Trigger,
    TypedRecord, Workflow,
};
use halvo_schema_core::request::{ErrorName, InterfaceDecision, InterfaceRole, RawRequest};
use halvo_schema_core::site::{AppId, Domain, DomainSource, SiteDomain, SiteValue};
use halvo_sqlcipher::rusqlite::Connection;
use serde_json::value::RawValue;

const DAY: u32 = 20_767;

/// The evidence rows of the draw, each naming six segments, so that `sources` holds six small cells for each.
const ROWS: u64 = 120;

/// The single removals of the draw.
const STEPS: usize = 100;

/// A drawn sequence of numbers, the same for the same seed (xorshift).
struct Draw(u64);

impl Draw {
    fn order(&mut self, n: usize) -> Vec<usize> {
        let mut order: Vec<usize> = (0..n).collect();
        for i in (1..n).rev() {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            order.swap(i, (self.0 % (i as u64 + 1)) as usize);
        }
        order
    }
}

fn keyless(scratch: &Scratch) -> Store {
    let host = Host::new(
        Box::new(TestClock::at_day(DAY)),
        Box::new(Grouping { min: 1_000 }),
    );
    open_keyless(scratch.path(), &registry(), host, &mut Seq(1 << 40)).unwrap()
}

/// The marker of the title of evidence row `i` of a store of `seed`.
fn marker(seed: u64, i: u64) -> String {
    format!("qzgap{seed}x{i:04}q")
}

fn titled(mut e: Episode, title: String) -> Episode {
    e.steps[0].title = Some(Tagged {
        value: Text::parse(&title).unwrap(),
        layer: FloorLayer::Floor,
    });
    e
}

/// A keyless store of `seed`: `ROWS` evidence rows of six segments each, so that the table `sources`, the smallest of
/// the store, holds many small cells, and twelve evidence rows among them of about 3,500 to 4,400 bytes, of stepped
/// sizes, whose cells spill onto overflow pages, so that some of them keep the least local part and leave room in the
/// tail of their last overflow page, which `plants` needs; all written in one batch; the store is closed.
fn drawn_store(seed: u64) -> Scratch {
    let scratch = Scratch::new(&format!("page-gap-check-{seed}"));
    let mut store = keyless(&scratch);
    let mut episodes: Vec<Episode> = (0..ROWS)
        .map(|i| {
            let mut e = episode(1_000 + i, DAY, None);
            e.segments = (0..6)
                .map(|k| Ordinal::new(10_000 + i * 10 + k).unwrap())
                .collect();
            titled(e, marker(seed, i))
        })
        .collect();
    for b in 0..12u64 {
        let mut e = episode(3_000 + b, DAY, None);
        let step = e.steps[0].clone();
        let each = (3_500 + b as usize * 80) / 5;
        e.steps = (0..5)
            .map(|k| {
                let mut s = step.clone();
                s.title = Some(Tagged {
                    value: Text::parse(&format!("big{b:02}{k}{}", "z".repeat(each))).unwrap(),
                    layer: FloorLayer::Floor,
                });
                s
            })
            .collect();
        episodes.insert(10 + 9 * b as usize, e);
    }
    store
        .commit_batch(&store_id(1), &episodes, position(1, 0), &mut Seq(100))
        .unwrap();
    drop(store);
    scratch
}

/// A keyless connection of the test's own on the database of the closed store, through which dbstat reads, and the
/// bytes of the main file.
fn image(scratch: &Scratch) -> (Connection, Vec<u8>) {
    halvo_derived::test_host::start_sqlite();
    let conn = Connection::open(scratch.db()).unwrap();
    conn.busy_timeout(std::time::Duration::ZERO).unwrap();
    let bytes = std::fs::read(scratch.db()).unwrap();
    (conn, bytes)
}

/// The file's own page count: the header's count at byte 28.
fn header_pages(bytes: &[u8]) -> usize {
    u32::from_be_bytes([bytes[28], bytes[29], bytes[30], bytes[31]]) as usize
}

/// The entries of `derived/` under `scratch` whose bytes hold `marker` whole.
fn holding(scratch: &Scratch, marker: &str) -> Vec<String> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(scratch.path().join("derived")).unwrap() {
        let entry = entry.unwrap();
        let bytes = std::fs::read(entry.path()).unwrap();
        if bytes.windows(marker.len()).any(|w| w == marker.as_bytes()) {
            found.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    found
}

/// The check's reading of the image of the closed store under `scratch`: the problems, and whether the four kinds of
/// pages add up to the pages read, which equal the file's own page count.
fn checked(scratch: &Scratch) -> (rules::Checked, bool) {
    let (conn, bytes) = image(scratch);
    let read = rules::check(&conn, &bytes);
    let sums = read.btree + read.overflow + read.trunks + read.leaves == read.pages
        && read.pages == header_pages(&bytes);
    (read, sums)
}

/// S-83 7 (b), S-85 item 4 and S-94 P2: the structural check of halvo-sqlcipher's tests, included here by a path
/// attribute, on the files of a keyless derived store. For each seed, a store as `drawn_store` writes it; then
/// `STEPS` single removals in a drawn order, each the purge of one ordinal of one evidence row, a checkpointed call
/// that ends with the truncating checkpoint, the store closed after each. After each removal the removed row's marker
/// is in no entry of `derived/`, the check names no problem, and its four kinds of pages add up to the pages that it
/// read, which equal the file's own page count. The last image holds a freelist trunk with a leaf and an overflow
/// tail, and `plants` names every byte that it plants in it: the control that the check sees what it is to see on
/// this store's own pages. On a build of the database without the statement of editPage (the row
/// edited-page-gap-not-zeroed of crates/halvo-sqlcipher/mutants.rows) this test fails by the check's own lines.
#[test]
fn small_rows_removed_one_at_a_time_leave_no_line_of_the_structural_check() {
    for seed in [1u64, 2, 3] {
        let mut lines = Vec::new();
        let scratch = drawn_store(seed);
        let order = Draw(0x9e37_79b9_7f4a_7c15 ^ seed).order(ROWS as usize);
        for (step, &i) in order.iter().take(STEPS).enumerate() {
            let mut store = keyless(&scratch);
            let removed = store
                .purge(&[Ordinal::new(10_000 + i as u64 * 10).unwrap()])
                .unwrap();
            assert_eq!(removed, 1, "seed {seed}, step {step}");
            drop(store);
            let marker = marker(seed, i as u64);
            assert_eq!(
                holding(&scratch, &marker),
                Vec::<String>::new(),
                "seed {seed}, step {step}: {marker}"
            );
            let (read, sums) = checked(&scratch);
            assert!(
                sums,
                "seed {seed}, step {step}: the kinds of pages do not add up"
            );
            lines.extend(
                read.problems
                    .iter()
                    .map(|p| format!("seed {seed}, step {step}: {p}")),
            );
        }
        assert_eq!(
            lines,
            Vec::<String>::new(),
            "seed {seed}: the lines of the structural check"
        );
        let (conn, bytes) = image(&scratch);
        let read = rules::check(&conn, &bytes);
        assert!(
            read.trunks >= 1 && read.leaves >= 1 && read.overflow_tails >= 1,
            "seed {seed}: the last image lacks a page kind that plants needs"
        );
        assert_eq!(
            rules::plants(&conn, &bytes),
            Vec::<String>::new(),
            "seed {seed}: the plants that the check does not name"
        );
    }
}

/// The draw of the test above, with secure delete off: the includer's own control (rules.rs, the caller's duties),
/// which must produce lines. The store of a seed is written and closed as `drawn_store` writes it; then a connection
/// of the test's own with secure delete off deletes the same evidence rows one at a time, each with its cells of
/// `sources`, and checkpoints the log with TRUNCATE after each; the check names a line after some of the steps.
#[test]
fn the_same_draw_with_secure_delete_off_leaves_lines_of_the_structural_check() {
    let scratch = drawn_store(1);
    let order = Draw(0x9e37_79b9_7f4a_7c15 ^ 1).order(ROWS as usize);
    let (conn, _) = image(&scratch);
    conn.execute_batch("PRAGMA secure_delete = OFF; PRAGMA foreign_keys = ON;")
        .unwrap();
    let mut with_lines = 0;
    for &i in order.iter().take(STEPS) {
        let row: i64 = conn
            .query_row(
                "SELECT row FROM sources WHERE ordinal = ?1",
                [10_000 + i as i64 * 10],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            conn.execute("DELETE FROM rows WHERE id = ?1", [row])
                .unwrap(),
            1
        );
        let busy: i64 = conn
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0))
            .unwrap();
        assert_eq!(busy, 0);
        let bytes = std::fs::read(scratch.db()).unwrap();
        if !rules::check(&conn, &bytes).problems.is_empty() {
            with_lines += 1;
        }
    }
    assert!(with_lines > 0, "no step with secure delete off left a line");
    println!("steps with lines of the check, secure delete off: {with_lines} of {STEPS}");
}

/// The three workflows of the draw of `removals`, each by its site and its app: the one that the delete takes, the one
/// that the dismissal takes, and the one whose app the batch replaces. Workflow `w` stands on the summaries 5,000 +
/// 10 `w` + 0, 1 and 2.
const WORKFLOWS: [(&str, &str); 3] = [
    ("gapdel.test", "com.example.gapdel"),
    ("gapdis.test", "com.example.gapdis"),
    ("gaprep.test", "com.example.gaprep"),
];

/// The ordinal that the purge of the draw shreds: the segment of the second summary of the dismissed workflow, which a
/// row of the window kind `test_trace` names as well.
const PURGED: u64 = 5_011;

/// The kinds whose records `Records` commits.
const RECORD_KINDS: [&str; 3] = ["test_note", "test_trace", "learn_run"];

/// A test handler that commits, in one typed commit, the records of the body
/// `{"op":..,"records":[[<kind>,<record text>],...]}` of its `coordinator_write`, each of a kind of `RECORD_KINDS` of
/// the family that the host gives the handler.
struct Records;

impl Handler for Records {
    fn handle(
        &self,
        request: &RawRequest,
        cx: &mut HandlerCx<'_>,
    ) -> Result<Box<RawValue>, ErrorName> {
        let body: serde_json::Value =
            serde_json::from_str(request.body().get()).map_err(|_| ErrorName::Malformed)?;
        let mut records = Vec::new();
        for pair in body["records"].as_array().ok_or(ErrorName::Malformed)? {
            let kind = RECORD_KINDS
                .into_iter()
                .find(|kind| pair[0] == *kind)
                .ok_or(ErrorName::Malformed)?;
            let json = pair[1].as_str().ok_or(ErrorName::Malformed)?.to_owned();
            records.push(TypedRecord { kind, json });
        }
        cx.commit(&records).map_err(|_| ErrorName::State)?;
        Ok(RawValue::from_string("null".to_owned()).unwrap())
    }
}

/// The keyless store of the draw of `removals` under `scratch`, with the entries `OWN`, `NOTE` and `TRACE` beside the
/// registry, whose kinds include those of the learning run (family `learn`), and a host of the recurrence step that
/// returns the proposals in `fixed`, `OwnNote` for `runner` requests, and `Records` of the family `test` and of the
/// family `learn` for the `coordinator_write` ops `test_rows` and `learn_rows`.
fn removal_store(
    scratch: &Scratch,
    clock: &TestClock,
    fixed: &Rc<RefCell<Vec<Proposal>>>,
) -> Store {
    let mut entries: Vec<&'static KindEntry> = registry();
    entries.extend_from_slice(&[&OWN, &NOTE, &TRACE]);
    let host = Host::new(Box::new(clock.clone()), Box::new(Fixed(Rc::clone(fixed))))
        .handler("test", Takes::Runner, Box::new(OwnNote))
        .handler(
            "test",
            Takes::Coordinator(&["test_rows"]),
            Box::new(Records),
        )
        .handler(
            "learn",
            Takes::Coordinator(&["learn_rows"]),
            Box::new(Records),
        );
    open_keyless(scratch.path(), &entries, host, &mut Seq(1 << 40)).unwrap()
}

/// A proposal of the key of `site` with the app `app` over the summaries `ids`.
fn proposal(site: &str, app: &str, ids: &[SummaryId]) -> Proposal {
    Proposal {
        category: SeedCategory::UtilityBillPayment,
        site: Some(SiteValue::Domain {
            key: SiteDomain::from(Domain::parse(site).unwrap()),
            source: DomainSource::AddressField,
        }),
        app: AppId::parse(app).unwrap(),
        trigger: Trigger::Manual,
        recurrence: Recurrence {
            count: ids.len() as u32,
            period: None,
            first: DAY,
            last: DAY,
        },
        confidence: Confidence(60),
        summaries: ids.to_vec(),
    }
}

/// Commits `records` through `Records` by a `coordinator_write` of `op`.
fn commit_records(store: &mut Store, op: &str, records: &[(&str, String)]) {
    let pairs: Vec<serde_json::Value> = records
        .iter()
        .map(|(kind, json)| serde_json::json!([kind, json]))
        .collect();
    let body = serde_json::json!({"op": op, "records": pairs}).to_string();
    store
        .handle(&raw_request("coordinator_write", &body))
        .unwrap();
}

/// Counts of rows by kind, in the order of the kinds' names.
type ByKind = Vec<(String, i64)>;

/// The rows of a family other than `workflow`, by kind, in the database of the closed store under `scratch`, through a
/// keyless connection of the test's own: the rows that stand under the indexes rows_read and rows_read_workflow, whose
/// condition is `family <> 'workflow'`.
fn under_both(scratch: &Scratch) -> ByKind {
    let (conn, _) = image(scratch);
    let mut statement = conn
        .prepare("SELECT kind, count(*) FROM rows WHERE family <> 'workflow' GROUP BY kind ORDER BY kind")
        .unwrap();
    statement
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// The store of `removals`, closed after its rows are written, with what the removals need of it.
struct Drawn {
    scratch: Scratch,
    clock: TestClock,
    fixed: Rc<RefCell<Vec<Proposal>>>,
    ids: Vec<WorkflowId>,
}

/// One keyless store on the day `DAY`: the three workflows of `WORKFLOWS`, stored by one batch of nine episodes whose
/// titles are 200 to 800 bytes long and the proposals of the recurrence step; for each workflow four rows of origin
/// own of 160 to 950 bytes by `OwnNote`, each in a request of its own, so that their entries in rows_read and
/// rows_read_workflow spill onto overflow pages, one row of `test_note`, and one row of `test_trace` that expires at
/// the start of the next day; one row of `test_trace` that names `PURGED` under the third workflow; and ONE row of the
/// learning run's kind `learn_run` under the first workflow. The test's own connection reads the rows of a family
/// other than `workflow` by kind.
fn removals_drawn() -> Drawn {
    let scratch = Scratch::new("page-gap-removals");
    let clock = TestClock::at_day(DAY);
    let fixed = Rc::new(RefCell::new(Vec::new()));
    let mut store = removal_store(&scratch, &clock, &fixed);
    let mut episodes = Vec::new();
    for (w, (site, app)) in WORKFLOWS.iter().enumerate() {
        let numbers: Vec<u64> = (0..3).map(|j| 5_000 + 10 * w as u64 + j).collect();
        for (j, n) in numbers.iter().enumerate() {
            let mut e = episode(*n, DAY, domain(site));
            e.app = AppId::parse(app).unwrap();
            episodes.push(titled(e, format!("gap{n} {}", "t".repeat(200 + 300 * j))));
        }
        let ids: Vec<SummaryId> = numbers.iter().map(|n| summary_id(*n)).collect();
        fixed.borrow_mut().push(proposal(site, app, &ids));
    }
    let out = store
        .commit_batch(&store_id(1), &episodes, position(1, 0), &mut Seq(100))
        .unwrap();
    assert_eq!(out.proposals.stored, 3, "the workflows of the draw");
    let items = listed(&store);
    let ids: Vec<WorkflowId> = WORKFLOWS
        .iter()
        .map(|(_, app)| {
            items
                .iter()
                .find(|item| matches!(&item.workflow, Workflow::Full(full) if full.app.as_str() == *app))
                .unwrap()
                .workflow
                .id()
                .clone()
        })
        .collect();
    for (w, id) in ids.iter().enumerate() {
        for k in 0..4 {
            let note = format!("gapown{w}{k} {}", "n".repeat(150 + 260 * k));
            store
                .handle(&raw_request("runner", &own_note_body(id.as_str(), &note)))
                .unwrap();
        }
    }
    let expires = (i64::from(DAY) + 1) * DAY_NS;
    let mut rows = Vec::new();
    for (w, id) in ids.iter().enumerate() {
        let id = id.as_str();
        rows.push((
            "test_note",
            format!(r#"{{"v":1,"workflow":"{id}","day":{DAY},"n":{w}}}"#),
        ));
        rows.push((
            "test_trace",
            format!(
                r#"{{"v":1,"workflow":"{id}","segments":[{}],"expires":{expires}}}"#,
                6_000 + w
            ),
        ));
    }
    rows.push((
        "test_trace",
        format!(
            r#"{{"v":1,"workflow":"{}","segments":[{PURGED}],"expires":{FAR}}}"#,
            ids[2].as_str()
        ),
    ));
    commit_records(&mut store, "test_rows", &rows);
    let learned = format!(
        r#"{{"v":1,"workflow":"{}","agent":"1a2b3c4d5e6f708192a3b4c5d6e7f809","run":"9f8e7d6c5b4a39281706f5e4d3c2b1a0","rev":2,"day":{DAY},"mode":"replay","state":"verified","cause":null,"base":1,"assisted":1,"kept":1,"failed_step":null}}"#,
        ids[0].as_str()
    );
    commit_records(&mut store, "learn_rows", &[("learn_run", learned)]);
    drop(store);
    let kinds = |pairs: &[(&str, i64)]| -> ByKind {
        pairs.iter().map(|(k, n)| ((*k).to_owned(), *n)).collect()
    };
    assert_eq!(
        under_both(&scratch),
        kinds(&[
            ("learn_run", 1),
            ("test_note", 3),
            ("test_own", 12),
            ("test_trace", 4)
        ]),
        "the rows of a family other than workflow that the draw stored"
    );
    Drawn {
        scratch,
        clock,
        fixed,
        ids,
    }
}

/// The five removals of the draw, in the order of `removals`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Removal {
    Expiry,
    Dismissal,
    Delete,
    Purge,
    Replace,
}

/// Applies the interface decision that `decision` makes for the workflow `id`.
fn decide(
    store: &mut Store,
    decision: fn(&InterfaceRole, RequestId, WorkflowId) -> InterfaceDecision,
    id: &WorkflowId,
) {
    store
        .decide(&decision(
            &InterfaceRole::claim(),
            RequestId::from_random([7; 16]),
            id.clone(),
        ))
        .unwrap();
}

/// The draw of `each_removal_leaves_no_line_of_the_structural_check`: the store of `removals_drawn`, then its five
/// removals in turn, each on the store opened again and closed after it. The expiry call (`Store::expire`) with the
/// clock two days on takes the three rows of `test_trace` past their expiry; the dismissal of the second workflow
/// (`Store::decide`) takes its support rows; the delete of the first takes every row that names it, the learned row and
/// its rows of origin own among them, and its summaries with their evidence; the purge of `PURGED` (`Store::purge`)
/// takes an evidence row and a row of `test_trace`; and a batch whose proposal gives the third workflow another app and
/// drops one of its support rows is followed by the expiry call, the next checkpointed call. `seen` gets each removal
/// with the store closed, before it (`false`) and after it (`true`).
fn removals(seen: &mut dyn FnMut(Removal, bool, &Scratch)) {
    let drawn = removals_drawn();
    for removal in [
        Removal::Expiry,
        Removal::Dismissal,
        Removal::Delete,
        Removal::Purge,
        Removal::Replace,
    ] {
        seen(removal, false, &drawn.scratch);
        let mut store = removal_store(&drawn.scratch, &drawn.clock, &drawn.fixed);
        match removal {
            Removal::Expiry => {
                drawn.clock.set_day(DAY + 2);
                assert_eq!(store.expire().unwrap(), 3, "the expiry call");
            }
            Removal::Dismissal => decide(&mut store, InterfaceDecision::dismiss, &drawn.ids[1]),
            Removal::Delete => decide(&mut store, InterfaceDecision::delete, &drawn.ids[0]),
            Removal::Purge => assert_eq!(
                store.purge(&[Ordinal::new(PURGED).unwrap()]).unwrap(),
                2,
                "the purge"
            ),
            Removal::Replace => {
                let kept = [summary_id(5_020), summary_id(5_021)];
                *drawn.fixed.borrow_mut() =
                    vec![proposal(WORKFLOWS[2].0, "com.example.gapnew", &kept)];
                let out = store
                    .commit_batch(&store_id(1), &[], position(2, 0), &mut Seq(200))
                    .unwrap();
                assert_eq!(out.proposals.updated, 1, "the batch's replace");
                store.expire().unwrap();
            }
        }
        drop(store);
        seen(removal, true, &drawn.scratch);
    }
}

/// pr-review-27 N2, C2.4 (contracts.md line 215) and ruling S-100 item 10: the structural check after each removal of
/// the crate. One keyless store, as `removals` draws it, holds rows of origin own (`OwnNote`), of the bounded kind
/// `test_note`, of the window kind `test_trace` and ONE row of the learning run's kind `learn_run`, whose family is not
/// `workflow`, beside the rows of the workflow run; then the expiry call, a dismissal, a delete, a purge, and a batch's
/// replace with the expiry call after it, in turn. After each, with the store closed, the check names no problem, and
/// its four kinds of pages add up to the pages that it read, which equal the file's own page count. The learned row is
/// one of the rows that the delete takes: the learned rows of the learning run are rows of this one database, and this
/// check holds their scrub.
#[test]
fn each_removal_leaves_no_line_of_the_structural_check() {
    let mut lines = Vec::new();
    removals(&mut |removal, after, scratch| {
        if after {
            let (read, sums) = checked(scratch);
            assert!(sums, "{removal:?}: the kinds of pages do not add up");
            lines.extend(read.problems.iter().map(|p| format!("{removal:?}: {p}")));
        }
    });
    assert_eq!(
        lines,
        Vec::<String>::new(),
        "the lines of the structural check"
    );
}

/// Ruling S-100 item 10: the delete, the purge and the expiry of the draw of `removals` take rows of a family other
/// than `workflow`, which stand under both indexes over the body, rows_read and rows_read_workflow. Around each of the
/// three, with the store closed, the test's own connection counts those rows: each takes at least one, and the delete
/// takes the learned row.
#[test]
fn the_delete_the_purge_and_the_expiry_take_rows_under_both_indexes() {
    let mut counts: Vec<(Removal, bool, ByKind)> = Vec::new();
    removals(&mut |removal, after, scratch| {
        if matches!(removal, Removal::Expiry | Removal::Delete | Removal::Purge) {
            counts.push((removal, after, under_both(scratch)));
        }
    });
    let total = |rows: &[(String, i64)]| rows.iter().map(|(_, n)| n).sum::<i64>();
    let learned = |rows: &[(String, i64)]| {
        rows.iter()
            .filter(|(kind, _)| kind == "learn_run")
            .map(|(_, n)| n)
            .sum::<i64>()
    };
    assert_eq!(
        counts.len(),
        6,
        "a count before and after each of the three"
    );
    for pair in counts.chunks(2) {
        let ((removal, false, before), (_, true, after)) = (&pair[0], &pair[1]) else {
            panic!("the counts come in pairs, before and after");
        };
        assert!(
            total(after) < total(before),
            "{removal:?}: the rows of a family other than workflow, {before:?} before and {after:?} after"
        );
        if *removal == Removal::Delete {
            assert_eq!(
                (learned(before), learned(after)),
                (1, 0),
                "the learned row, before and after the delete"
            );
        }
    }
}
