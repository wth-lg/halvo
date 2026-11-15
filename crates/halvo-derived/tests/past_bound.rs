//! DS-4: a read hands out exactly the rows that an expiry at that read's clock would leave, and no read writes.

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{
    episode, open_entries, position, raw, raw_request, store_id, summary_id, Fixed, Scratch, Seq,
    TestClock, DAY_NS,
};
use halvo_derived::{Handler, HandlerCx, Host, Report, Store, Takes};
use halvo_schema_core::category::SeedCategory;
use halvo_schema_core::ids::{RequestId, WorkflowId};
use halvo_schema_core::kinds::{FieldType, KindEntry, Origin, RetentionClass};
use halvo_schema_core::record::{
    Confidence, Instant, Proposal, Recurrence, RowRead, SummaryRead, Trigger, TypedRecord,
    WriterStep,
};
use halvo_schema_core::request::{
    ErrorName, InterfaceRead, InterfaceRole, RawRequest, ReplyResult,
};
use halvo_schema_core::site::AppId;
use serde_json::value::RawValue;

const DAY: u32 = 20_767;
const NOON: i64 = 43_200_000_000_000;
const HOUR: i64 = 3_600_000_000_000;
const FAR: i64 = 9_000_000_000_000_000_000;

static PB_LONG: KindEntry = KindEntry {
    name: "test_pb_long",
    class: RetentionClass::Bounded { days: 93 },
    origin: Origin::Capture,
    family: "test",
    workflow_key: None,
    fields: &[("v", FieldType::Version), ("day", FieldType::Day)],
    example: r#"{"v":1,"day":20767}"#,
};

/// Stored under 35 days; the variants open it under 93 (lengthened) and under 10 (shortened).
static PB_35: KindEntry = KindEntry {
    name: "test_pb_35",
    class: RetentionClass::Bounded { days: 35 },
    ..PB_LONG
};

static PB_35_AS_93: KindEntry = KindEntry {
    class: RetentionClass::Bounded { days: 93 },
    ..PB_35
};

static PB_35_AS_10: KindEntry = KindEntry {
    class: RetentionClass::Bounded { days: 10 },
    ..PB_35
};

static PB_W: KindEntry = KindEntry {
    name: "test_pb_w",
    class: RetentionClass::Window,
    origin: Origin::Capture,
    family: "test",
    workflow_key: None,
    fields: &[
        ("v", FieldType::Version),
        ("segments", FieldType::Ordinal),
        ("expires", FieldType::Instant),
    ],
    example: r#"{"v":1,"segments":[4410],"expires":1795478400000000000}"#,
};

static PB_KEPT: KindEntry = KindEntry {
    name: "test_pb_kept",
    class: RetentionClass::Kept,
    origin: Origin::Capture,
    family: "test",
    workflow_key: Some("workflow"),
    fields: &[
        ("v", FieldType::Version),
        ("workflow", FieldType::Id),
        ("seen", FieldType::Count),
    ],
    example: r#"{"v":1,"workflow":"10000000000000000000000000000001","seen":1}"#,
};

const KINDS: [&str; 4] = ["test_pb_long", "test_pb_35", "test_pb_w", "test_pb_kept"];

/// How the store is opened after its rows were stored under `PB_35`: the bound of `test_pb_35` unchanged,
/// lengthened to 93 days or shortened to 10.
#[derive(Clone, Copy, Debug)]
enum Bound {
    Unchanged,
    Lengthened,
    Shortened,
}

impl Bound {
    fn entries(self) -> [&'static KindEntry; 4] {
        let pb35 = match self {
            Bound::Unchanged => &PB_35,
            Bound::Lengthened => &PB_35_AS_93,
            Bound::Shortened => &PB_35_AS_10,
        };
        [&PB_LONG, pb35, &PB_W, &PB_KEPT]
    }
}

fn record(kind: &'static str, json: String) -> TypedRecord {
    TypedRecord { kind, json }
}

fn bounded(kind: &'static str, day: u32) -> TypedRecord {
    record(kind, format!(r#"{{"v":1,"day":{day}}}"#))
}

fn window(segment: u64, expires: i64) -> TypedRecord {
    record(
        "test_pb_w",
        format!(r#"{{"v":1,"segments":[{segment}],"expires":{expires}}}"#),
    )
}

/// The proposal of the episodes' key (the category of `episode`, no site, the browser app) over `ids`.
fn over(ids: &[u64]) -> Proposal {
    Proposal {
        category: SeedCategory::UtilityBillPayment,
        site: None,
        app: AppId::parse("com.example.browser").unwrap(),
        trigger: Trigger::Manual,
        recurrence: Recurrence {
            count: ids.len() as u32,
            period: None,
            first: DAY - 92,
            last: DAY,
        },
        confidence: Confidence(50),
        summaries: ids.iter().map(|n| summary_id(*n)).collect(),
    }
}

/// A step of the family `test` that reads, in each `after_commit`, the summaries and the rows of every test kind
/// into `seen`, and returns the records that the test put in `emit`.
struct Probe {
    emit: Rc<RefCell<Vec<TypedRecord>>>,
    seen: Rc<RefCell<Vec<String>>>,
    echo: Rc<Cell<bool>>,
}

impl WriterStep for Probe {
    fn after_commit(&self, summaries: &dyn SummaryRead, rows: &dyn RowRead) -> Vec<TypedRecord> {
        let mut seen = self.seen.borrow_mut();
        summaries.for_each(&mut |s| seen.push(format!("step summary {}", s.id.as_str())));
        for kind in KINDS {
            rows.for_each(kind, &mut |row| seen.push(format!("step {kind} {row}")));
        }
        let mut out: Vec<TypedRecord> = self.emit.borrow_mut().drain(..).collect();
        if self.echo.get() {
            let mut n = 300;
            rows.for_each("test_pb_long", &mut |_| {
                n += 1;
                out.push(window(n, FAR));
            });
        }
        out
    }
}

/// A runner handler of the family `test` that reads every test kind, the workflow's item and its first evidence
/// page into `seen`.
struct Reader {
    workflow: Rc<RefCell<Option<WorkflowId>>>,
    seen: Rc<RefCell<Vec<String>>>,
}

impl Handler for Reader {
    fn handle(
        &self,
        _request: &RawRequest,
        cx: &mut HandlerCx<'_>,
    ) -> Result<Box<RawValue>, ErrorName> {
        let mut seen = self.seen.borrow_mut();
        for kind in KINDS {
            cx.rows()
                .for_each(kind, &mut |row| seen.push(format!("handler {kind} {row}")));
        }
        if let Some(w) = self.workflow.borrow().as_ref() {
            seen.push(format!("handler workflow {:?}", cx.workflow(w)));
            seen.push(format!("handler evidence {:?}", cx.evidence(w, None)));
        }
        Ok(RawValue::from_string("null".to_owned()).unwrap())
    }
}

/// One store with rows of every class and of the workflow run, held open at its clock.
struct Fixture {
    scratch: Scratch,
    clock: TestClock,
    store: Store,
    workflow: WorkflowId,
    seen: Rc<RefCell<Vec<String>>>,
    reports: Rc<RefCell<Vec<Report>>>,
    echo: Rc<Cell<bool>>,
    fixed: Rc<RefCell<Vec<Proposal>>>,
    at: u64,
}

impl Fixture {
    /// Stores, under the clock at noon of `DAY`:
    /// - summaries 1 (day DAY - 92, going on DAY + 1, its evidence expiring far later), 2 (day DAY, its evidence
    ///   expiring at noon plus an hour) and 3 (day DAY - 50), and one workflow supported by the three;
    /// - `test_pb_long` rows of days DAY - 92 and DAY, `test_pb_35` rows of days DAY - 34 and DAY, window rows that
    ///   expire at noon plus an hour and far later, and one kept row;
    ///
    /// then, through the test's own connection, gives the support row of summary 1 a later day of going than its
    /// summary's, and opens the store again under `bound` at noon of `DAY`.
    fn new(name: &str, bound: Bound) -> Fixture {
        let scratch = Scratch::new(name);
        let clock = TestClock::at_day(DAY);
        let fixed = Rc::new(RefCell::new(vec![over(&[1, 2, 3])]));
        let emit = Rc::new(RefCell::new(Vec::new()));
        let seen = Rc::new(RefCell::new(Vec::new()));
        let reports = Rc::new(RefCell::new(Vec::new()));
        let workflow = Rc::new(RefCell::new(None));
        let echo = Rc::new(Cell::new(false));
        let host = |fixed: &Rc<RefCell<Vec<Proposal>>>| {
            let sink = Rc::clone(&reports);
            Host::new(Box::new(clock.clone()), Box::new(Fixed(Rc::clone(fixed))))
                .step(
                    "test",
                    Box::new(Probe {
                        emit: Rc::clone(&emit),
                        seen: Rc::clone(&seen),
                        echo: Rc::clone(&echo),
                    }),
                )
                .handler(
                    "test",
                    Takes::Runner,
                    Box::new(Reader {
                        workflow: Rc::clone(&workflow),
                        seen: Rc::clone(&seen),
                    }),
                )
                .on_report(Box::new(move |report: &Report| {
                    sink.borrow_mut().push(report.clone())
                }))
        };
        let soon = i64::from(DAY) * DAY_NS + NOON + HOUR;
        let stored = Bound::Unchanged.entries();
        let mut store = open_entries(&scratch, host(&fixed), &stored).unwrap();
        let mut e1 = episode(1, DAY - 92, None);
        e1.expires = Instant(FAR);
        let mut e2 = episode(2, DAY, None);
        e2.expires = Instant(soon);
        let e3 = episode(3, DAY - 50, None);
        store
            .commit_batch(&store_id(1), &[e1, e2, e3], position(1, 0), &mut Seq(100))
            .unwrap();
        let id = match store
            .interface_read(&InterfaceRead::workflows(
                &InterfaceRole::claim(),
                RequestId::from_random([1; 16]),
                None,
            ))
            .unwrap()
        {
            ReplyResult::Workflows(page) => page.workflows[0].workflow.id().clone(),
            other => panic!("no workflows page: {other:?}"),
        };
        fixed.borrow_mut().clear();
        *emit.borrow_mut() = vec![
            bounded("test_pb_long", DAY - 92),
            bounded("test_pb_long", DAY),
            bounded("test_pb_35", DAY - 34),
            bounded("test_pb_35", DAY),
            window(101, soon),
            window(102, FAR),
            record(
                "test_pb_kept",
                format!(r#"{{"v":1,"workflow":"{}","seen":1}}"#, id.as_str()),
            ),
        ];
        let outcome = store
            .commit_batch(&store_id(1), &[], position(2, 0), &mut Seq(200))
            .unwrap();
        assert_eq!(
            format!("{:?}", outcome.steps),
            "[Ok(Committed { stored: 7, dropped: 0 })]"
        );
        drop(store);
        raw(&scratch)
            .execute(
                "UPDATE rows SET day = ?1, gone = ?2 WHERE kind = 'support' AND summary = ?3",
                (DAY - 50, DAY + 43, summary_id(1).as_str()),
            )
            .unwrap();
        *workflow.borrow_mut() = Some(id.clone());
        seen.borrow_mut().clear();
        let entries = bound.entries();
        let store = open_entries(&scratch, host(&fixed), &entries).unwrap();
        reports.borrow_mut().clear();
        Fixture {
            scratch,
            clock,
            store,
            workflow: id,
            seen,
            reports,
            echo,
            fixed,
            at: 3,
        }
    }

    /// A `commit_batch` with no episode at the next position, which runs the recurrence step and the probe step.
    fn batch(&mut self) -> halvo_derived::CommitOutcome {
        self.at += 1;
        self.store
            .commit_batch(
                &store_id(1),
                &[],
                position(self.at, 0),
                &mut Seq(self.at * 100),
            )
            .unwrap()
    }

    fn set(&self, instant: i64) {
        self.clock.set(instant);
    }

    /// What the reads hand out at the clock as it stands, as text: a handler's reads, the interface read's pages,
    /// and, when `step` is set, a step's summary read and row read through a `commit_batch` with no episode.
    fn reads(&mut self, step: bool) -> Vec<String> {
        self.seen.borrow_mut().clear();
        self.store.handle(&raw_request("runner", "{}")).unwrap();
        let role = InterfaceRole::claim();
        let workflows = self
            .store
            .interface_read(&InterfaceRead::workflows(
                &role,
                RequestId::from_random([2; 16]),
                None,
            ))
            .unwrap();
        let evidence = self.store.interface_read(&InterfaceRead::evidence(
            &role,
            RequestId::from_random([3; 16]),
            self.workflow.clone(),
            None,
        ));
        let mut out = self.seen.borrow().clone();
        out.push(format!("interface workflows {workflows:?}"));
        out.push(format!("interface evidence {evidence:?}"));
        if step {
            self.seen.borrow_mut().clear();
            self.at += 1;
            self.store
                .commit_batch(
                    &store_id(1),
                    &[],
                    position(self.at, 0),
                    &mut Seq(self.at * 100),
                )
                .unwrap();
            out.extend(self.seen.borrow().iter().cloned());
        }
        out
    }
}

/// The clocks of the property: before, at and after each edge of the fixture's rows, and one, two and three days
/// back.
fn clocks() -> Vec<(&'static str, i64)> {
    let day = |d: u32| i64::from(d) * DAY_NS;
    let soon = day(DAY) + NOON + HOUR;
    vec![
        ("expiry - 1", soon - 1),
        ("expiry", soon),
        ("expiry + 1", soon + 1),
        ("going on the next day", day(DAY + 1) - 1),
        ("going on the clock's day", day(DAY + 1)),
        ("day + 10", day(DAY + 10)),
        ("day + 35", day(DAY + 35)),
        ("day + 43", day(DAY + 43)),
        ("one day ahead", day(DAY - 1) + NOON),
        ("two days ahead", day(DAY - 2) + NOON),
        ("three days ahead", day(DAY - 3) + NOON),
    ]
}

/// H5 (a): at every clock, each read hands out the same bytes before `Store::expire` and after it at the same
/// clock, under a bound that is unchanged, lengthened and shortened; and the expiry removes rows at the clocks where
/// a row is due.
#[test]
fn every_read_hands_out_what_an_expiry_at_its_clock_would_leave() {
    let mut removed = Vec::new();
    for bound in [Bound::Unchanged, Bound::Lengthened, Bound::Shortened] {
        for (label, instant) in clocks() {
            let mut fx = Fixture::new("reads-expiry", bound);
            fx.set(instant);
            let before = fx.reads(true);
            let gone = fx.store.expire().unwrap();
            let after = fx.reads(true);
            assert_eq!(
                before, after,
                "{bound:?} at {label}: the reads before and after the expiry"
            );
            removed.push((format!("{bound:?} {label}"), gone > 0));
        }
    }
    let due: Vec<&String> = removed.iter().filter(|(_, g)| *g).map(|(l, _)| l).collect();
    assert_eq!(
        due.len(),
        removed.len() - 3 * 3,
        "the expiry removes rows at every clock but the three before the first edge and the two days back: {due:?}"
    );
}

/// H5 (b): walking the report's deadlines, one nanosecond before each every read hands out what it did, and at
/// it, with no expiry run, the due row is in no read: the reads equal those after the expiry at that clock.
#[test]
fn at_the_report_s_deadline_the_due_row_leaves_every_read_before_any_expiry() {
    for bound in [Bound::Unchanged, Bound::Lengthened, Bound::Shortened] {
        let mut fx = Fixture::new("reads-deadline", bound);
        let mut walked = 0;
        while let Some(deadline) = fx.store.report().unwrap().expiry {
            if deadline.0 >= i64::from(DAY + 100) * DAY_NS {
                break;
            }
            fx.set(deadline.0 - 1);
            let before = fx.reads(false);
            fx.set(deadline.0);
            let at = fx.reads(false);
            assert_ne!(
                before, at,
                "{bound:?}: a row leaves the reads at {deadline:?}"
            );
            assert!(fx.store.expire().unwrap() >= 1, "{bound:?} at {deadline:?}");
            assert_eq!(
                at,
                fx.reads(false),
                "{bound:?}: the reads at {deadline:?} equal those after its expiry"
            );
            walked += 1;
        }
        assert!(walked >= 3, "{bound:?}: deadlines walked {walked}");
    }
}

/// The bytes of the store's database file and its write-ahead log.
fn files(scratch: &Scratch) -> Vec<Vec<u8>> {
    let db = scratch.db();
    let wal = db.with_extension("db-wal");
    [db, wal]
        .iter()
        .map(|p| std::fs::read(p).unwrap_or_default())
        .collect()
}

/// H5 (d): past a deadline, no read writes: the files of the store are byte for byte what they were, the host's
/// sink got no report, the revisions have not risen, and the report still counts the due rows.
#[test]
fn no_read_writes_past_a_deadline() {
    let mut fx = Fixture::new("reads-write-nothing", Bound::Unchanged);
    fx.set(i64::from(DAY + 1) * DAY_NS + NOON);
    let report = fx.store.report().unwrap();
    let bytes = files(&fx.scratch);
    let sink = fx.reports.borrow().len();
    for _ in 0..3 {
        fx.reads(false);
    }
    assert_eq!(files(&fx.scratch), bytes, "the files after the reads");
    assert_eq!(
        fx.reports.borrow().len(),
        sink,
        "reports that the reads gave the host"
    );
    let later = fx.store.report().unwrap();
    assert_eq!(later.revisions, report.revisions);
    assert_eq!(later.counts, report.counts);
    assert!(
        report.counts.bounded >= 3,
        "the due rows are still counted: {:?}",
        report.counts
    );
}

fn has(reads: &[String], what: &str) -> bool {
    reads.iter().any(|line| line.contains(what))
}

const LONG_OF_DAY: &str = r#"handler test_pb_long {"v":1,"day":20767}"#;
const LONG_PAST: &str = r#"handler test_pb_long {"v":1,"day":20675}"#;

/// H5 (c): a row stored today is read by a handler and by the interface read with the clock one and two days back,
/// by neither three days back, where no row is removed, and by both again with the clock forward, the tables
/// unchanged; 400 days on, the kept row and the far window row are visited and the bounded rows beside them are not.
#[test]
fn a_row_one_or_two_days_ahead_is_read_three_days_ahead_is_not_and_the_classes_side_by_side() {
    let mut fx = Fixture::new("reads-ahead", Bound::Unchanged);
    let s2 = summary_id(2).as_str().to_owned();
    let bytes = files(&fx.scratch);
    let counts = fx.store.report().unwrap().counts;
    let mut seen = Vec::new();
    for back in [1u32, 2, 3, 0] {
        fx.set(i64::from(DAY - back) * DAY_NS + NOON);
        let reads = fx.reads(false);
        let interface = reads
            .iter()
            .find(|l| l.starts_with("interface workflows"))
            .unwrap();
        seen.push((back, has(&reads, LONG_OF_DAY), interface.contains(&s2)));
    }
    assert_eq!(
        seen,
        vec![
            (1, true, true),
            (2, true, true),
            (3, false, false),
            (0, true, true)
        ],
        "(days back, the handler reads the row of the day, the interface read names its summary)"
    );
    assert_eq!(
        files(&fx.scratch),
        bytes,
        "no row was removed and nothing written"
    );
    assert_eq!(fx.store.report().unwrap().counts, counts);
    fx.set(i64::from(DAY + 400) * DAY_NS + NOON);
    let reads = fx.reads(false);
    assert!(
        has(&reads, "handler test_pb_kept"),
        "the kept row 400 days on"
    );
    assert!(
        has(&reads, r#""segments":[102]"#),
        "the far window row 400 days on"
    );
    assert!(
        !has(&reads, "handler test_pb_long"),
        "the bounded rows 400 days on"
    );
    assert!(
        !has(&reads, "handler test_pb_35"),
        "the bounded rows 400 days on"
    );
}

/// H5 (e): the write path keeps its statement: a proposal of the key that names summaries 2 and 3, with summary 1
/// past its bound and not expired, leaves the support rows of 2 and 3 alone, counted through the test's connection.
#[test]
fn an_update_removes_the_support_row_of_a_summary_past_its_bound_that_no_read_hands_out() {
    let mut fx = Fixture::new("reads-set-support", Bound::Unchanged);
    fx.set(i64::from(DAY + 1) * DAY_NS + NOON);
    *fx.fixed.borrow_mut() = vec![over(&[2, 3])];
    let outcome = fx.batch();
    assert_eq!(outcome.proposals.updated, 1);
    let Fixture { scratch, store, .. } = fx;
    drop(store);
    let supports: Vec<String> = raw(&scratch)
        .prepare("SELECT summary FROM rows WHERE kind = 'support' ORDER BY summary")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        supports,
        vec![
            summary_id(2).as_str().to_owned(),
            summary_id(3).as_str().to_owned()
        ]
    );
}

/// H5 (f): with rows past their bound and no expiry, the report counts them, a purge of a segment removes and
/// counts its row, the delete of their workflow removes its rows in one transaction, and after the delete, with the
/// clock set back, no read finds one of them.
#[test]
fn a_delete_a_purge_and_the_counts_still_find_rows_that_the_reads_leave_out() {
    let mut fx = Fixture::new("reads-delete-purge", Bound::Unchanged);
    fx.set(i64::from(DAY + 1) * DAY_NS + NOON);
    let reads = fx.reads(false);
    assert!(!has(&reads, LONG_PAST) && !has(&reads, r#""segments":[101]"#));
    let counts = fx.store.report().unwrap().counts;
    assert_eq!(
        (counts.window, counts.bounded, counts.kept),
        (5, 10, 2),
        "every stored row is counted, past its bound or not"
    );
    let ord = halvo_schema_core::record::Ordinal::new(101).unwrap();
    assert_eq!(
        fx.store.purge(&[ord]).unwrap(),
        1,
        "the purge removes the window row past its expiry"
    );
    fx.store
        .decide(&halvo_schema_core::request::InterfaceDecision::delete(
            &InterfaceRole::claim(),
            RequestId::from_random([4; 16]),
            fx.workflow.clone(),
        ))
        .unwrap();
    let counts = fx.store.report().unwrap().counts;
    assert_eq!(
        (counts.window, counts.bounded, counts.kept),
        (1, 4, 0),
        "the delete removed the workflow, its kept row, its support rows, its summaries and their evidence"
    );
    fx.set(i64::from(DAY) * DAY_NS + NOON);
    let reads = fx.reads(true);
    for gone in [
        "step summary",
        "handler test_pb_kept",
        r#""segments":[101]"#,
    ] {
        assert!(
            !has(&reads, gone),
            "after the delete with the clock set back: {gone}"
        );
    }
    assert!(has(
        &reads,
        "interface workflows Workflows(WorkflowsPage { workflows: [], next: None })"
    ));
}

/// H5 (j): a step reads its rows by the batch's clock: with no expiry before the batch, a row past its bound is not
/// visited, and the step returns no record built from it.
#[test]
fn a_step_builds_no_record_from_a_row_past_its_bound() {
    let mut fx = Fixture::new("reads-step", Bound::Unchanged);
    fx.set(i64::from(DAY + 1) * DAY_NS + NOON);
    fx.echo.set(true);
    fx.seen.borrow_mut().clear();
    let outcome = fx.batch();
    let seen = fx.seen.borrow().clone();
    assert!(!seen
        .iter()
        .any(|l| l.contains(r#"step test_pb_long {"v":1,"day":20675}"#)));
    assert_eq!(
        format!("{:?}", outcome.steps),
        "[Ok(Committed { stored: 1, dropped: 0 })]",
        "one record, built from the one row of the long kind inside its bound"
    );
}

/// H5 (k), pinned: the clock set back over a deadline before the host's expiry has run hands the row out again,
/// since it is stored; the host's expiry at the deadline is what closes it, and after it the row is gone for good.
#[test]
fn a_clock_set_back_over_a_deadline_hands_a_row_out_again_until_the_host_s_expiry_has_run() {
    let mut fx = Fixture::new("reads-set-back", Bound::Unchanged);
    let deadline = i64::from(DAY + 1) * DAY_NS;
    let mut seen = Vec::new();
    for at in [deadline, deadline - NOON] {
        fx.set(at);
        seen.push(has(&fx.reads(false), LONG_PAST));
    }
    fx.set(deadline);
    assert!(fx.store.expire().unwrap() >= 1);
    fx.set(deadline - NOON);
    seen.push(has(&fx.reads(false), LONG_PAST));
    assert_eq!(
        seen,
        vec![false, true, false],
        "(at the deadline, set back before it, set back after the expiry)"
    );
}

/// A host clock that counts its readings and, while `step` is not 0, moves on by `step` after each reading.
#[derive(Clone)]
struct SteppingClock {
    at: Rc<Cell<i64>>,
    step: Rc<Cell<i64>>,
    readings: Rc<Cell<u32>>,
}

impl halvo_derived::Clock for SteppingClock {
    fn now(&self) -> Instant {
        self.readings.set(self.readings.get() + 1);
        let at = self.at.get();
        self.at.set(at + self.step.get());
        Instant(at)
    }

    fn day(&self, at: Instant) -> u32 {
        u32::try_from(at.0.div_euclid(DAY_NS)).unwrap()
    }

    fn day_start(&self, day: u32) -> Instant {
        Instant(i64::from(day) * DAY_NS)
    }
}

/// A runner handler in front that reads the long kind, passes the request on, and reads it again.
struct Front(Rc<RefCell<Vec<usize>>>);

impl Handler for Front {
    fn handle(&self, _r: &RawRequest, cx: &mut HandlerCx<'_>) -> Result<Box<RawValue>, ErrorName> {
        let count = |cx: &HandlerCx<'_>| {
            let mut n = 0;
            cx.rows().for_each("test_pb_long", &mut |_| n += 1);
            n
        };
        let first = count(cx);
        self.0.borrow_mut().push(first);
        cx.pass_on()?;
        let again = count(cx);
        self.0.borrow_mut().push(again);
        Ok(RawValue::from_string("null".to_owned()).unwrap())
    }
}

/// The handler behind `Front`: it reads the long kind once.
struct Behind(Rc<RefCell<Vec<usize>>>);

impl Handler for Behind {
    fn handle(&self, _r: &RawRequest, cx: &mut HandlerCx<'_>) -> Result<Box<RawValue>, ErrorName> {
        let mut n = 0;
        cx.rows().for_each("test_pb_long", &mut |_| n += 1);
        self.0.borrow_mut().push(n);
        Ok(RawValue::from_string("null".to_owned()).unwrap())
    }
}

/// H5 (i): one reading of the host's clock for each call: with a clock that steps over a deadline at each reading,
/// one handled request reads its rows by one reading in front and behind, one interface read reads the clock once
/// and its item's `summaries` agree with that reading, and a step's summary read and row read agree on one day.
#[test]
fn every_read_of_one_call_reads_by_one_reading_of_the_host_s_clock() {
    let scratch = Scratch::new("reads-one-reading");
    let clock = SteppingClock {
        at: Rc::new(Cell::new(i64::from(DAY) * DAY_NS + NOON)),
        step: Rc::new(Cell::new(0)),
        readings: Rc::new(Cell::new(0)),
    };
    let counts = Rc::new(RefCell::new(Vec::new()));
    let fixed = Rc::new(RefCell::new(vec![over(&[1])]));
    let emit = Rc::new(RefCell::new(vec![bounded("test_pb_long", DAY - 92)]));
    let seen = Rc::new(RefCell::new(Vec::new()));
    let host = Host::new(Box::new(clock.clone()), Box::new(Fixed(Rc::clone(&fixed))))
        .step(
            "test",
            Box::new(Probe {
                emit: Rc::clone(&emit),
                seen: Rc::clone(&seen),
                echo: Rc::new(Cell::new(false)),
            }),
        )
        .handler("test", Takes::Runner, Box::new(Front(Rc::clone(&counts))))
        .handler("test", Takes::Runner, Box::new(Behind(Rc::clone(&counts))));
    let mut store = open_entries(&scratch, host, &Bound::Unchanged.entries()).unwrap();
    store
        .commit_batch(
            &store_id(1),
            &[episode(1, DAY - 92, None)],
            position(1, 0),
            &mut Seq(1),
        )
        .unwrap();
    fixed.borrow_mut().clear();
    let deadline = i64::from(DAY + 1) * DAY_NS;
    clock.step.set(1);
    clock.at.set(deadline - 1);
    clock.readings.set(0);
    store.handle(&raw_request("runner", "{}")).unwrap();
    assert_eq!(
        (counts.borrow().clone(), clock.readings.get()),
        (vec![1, 1, 1], 1),
        "(rows read in front, behind, in front again; readings of the clock)"
    );
    clock.at.set(deadline - 1);
    clock.readings.set(0);
    let page = store
        .interface_read(&InterfaceRead::workflows(
            &InterfaceRole::claim(),
            RequestId::from_random([5; 16]),
            None,
        ))
        .unwrap();
    let ReplyResult::Workflows(page) = page else {
        panic!("no workflows page");
    };
    assert_eq!(
        (page.workflows[0].summaries.clone(), clock.readings.get()),
        (vec![summary_id(1)], 1),
        "(the item's summaries by the one reading before the deadline, readings of the clock)"
    );
    clock.at.set(deadline - 1);
    seen.borrow_mut().clear();
    store
        .commit_batch(&store_id(1), &[], position(2, 0), &mut Seq(2))
        .unwrap();
    let seen = seen.borrow().clone();
    let summary = seen.iter().any(|l| l.starts_with("step summary"));
    let row = seen.iter().any(|l| l.starts_with("step test_pb_long"));
    assert_eq!(
        (summary, row),
        (true, true),
        "the step's summary read and row read agree on one day"
    );
}

/// The summary ids of the evidence page that the interface read gives after `after`, and its `next`.
fn evidence_ids(fx: &Fixture, after: Option<u64>) -> (Vec<String>, bool) {
    match fx
        .store
        .interface_read(&InterfaceRead::evidence(
            &InterfaceRole::claim(),
            RequestId::from_random([6; 16]),
            fx.workflow.clone(),
            after.map(summary_id),
        ))
        .unwrap()
    {
        ReplyResult::Evidence(page) => (
            page.evidence
                .iter()
                .map(|e| e.summary.as_str().to_owned())
                .collect(),
            page.next.is_some(),
        ),
        other => panic!("no evidence page: {other:?}"),
    }
}

/// The ids of the workflow's support set that the interface read gives.
fn summary_ids(fx: &Fixture) -> Vec<String> {
    match fx
        .store
        .interface_read(&InterfaceRead::workflows(
            &InterfaceRole::claim(),
            RequestId::from_random([7; 16]),
            None,
        ))
        .unwrap()
    {
        ReplyResult::Workflows(page) => page.workflows[0]
            .summaries
            .iter()
            .map(|s| s.as_str().to_owned())
            .collect(),
        other => panic!("no workflows page: {other:?}"),
    }
}

/// H5 (h): the parts of the interface read: `summaries` leaves out the id of a support row whose summary is past
/// its bound, a page asked with `after` after a deadline passed holds no item past its bound, and when no evidence
/// row is left the page is empty with a null `next`.
#[test]
fn the_interface_read_leaves_rows_past_their_bound_out_of_summaries_and_evidence_pages() {
    let fx = Fixture::new("reads-interface-parts", Bound::Unchanged);
    let id = |n: u64| summary_id(n).as_str().to_owned();
    let mut seen = Vec::new();
    for (at, after) in [
        (i64::from(DAY) * DAY_NS + NOON, None),
        (i64::from(DAY + 1) * DAY_NS + NOON, Some(1)),
        (i64::from(DAY + 43) * DAY_NS, None),
    ] {
        fx.set(at);
        seen.push((summary_ids(&fx), evidence_ids(&fx, after)));
    }
    assert_eq!(
        seen,
        vec![
            (vec![id(1), id(2), id(3)], (vec![id(1), id(2), id(3)], false)),
            (vec![id(2), id(3)], (vec![id(3)], false)),
            (vec![id(2)], (Vec::new(), false)),
        ],
        "(support ids, (evidence page, a next)) on DAY, on DAY + 1 after summary 1, and on DAY + 43"
    );
}

/// What a handler of `Items` read: the support ids of the workflow's item and the evidence page's summary ids, in
/// front before it passes the request on, behind, and in front again.
type ItemReads = Rc<RefCell<Vec<(&'static str, usize, usize)>>>;

/// A runner handler that reads the item of the workflow in `workflow` and its first evidence page, and, where
/// `front` is set, passes the request on and reads both again.
struct Items {
    front: bool,
    workflow: Rc<RefCell<Option<WorkflowId>>>,
    seen: ItemReads,
}

impl Items {
    fn read(&self, cx: &HandlerCx<'_>, at: &'static str) {
        let w = self.workflow.borrow().clone().unwrap();
        let summaries = cx.workflow(&w).unwrap().unwrap().summaries.len();
        let evidence = cx.evidence(&w, None).unwrap().evidence.len();
        self.seen.borrow_mut().push((at, summaries, evidence));
    }
}

impl Handler for Items {
    fn handle(&self, _r: &RawRequest, cx: &mut HandlerCx<'_>) -> Result<Box<RawValue>, ErrorName> {
        if self.front {
            self.read(cx, "front");
            cx.pass_on()?;
            self.read(cx, "front again");
        } else {
            self.read(cx, "behind");
        }
        Ok(RawValue::from_string("null".to_owned()).unwrap())
    }
}

/// H3, C25.5: a handler's workflow read and evidence read take the one reading of the host's clock that the handled
/// request took at its start. With a clock that steps over the day of going of the workflow's one summary at each
/// reading, every `workflow` and `evidence` call of one request, in front and behind, still hands out the summary
/// and its evidence row, and the request reads the clock once.
#[test]
fn a_handler_s_workflow_read_and_evidence_read_take_the_one_reading_of_its_request() {
    let scratch = Scratch::new("handler-item-one-reading");
    let clock = SteppingClock {
        at: Rc::new(Cell::new(i64::from(DAY) * DAY_NS + NOON)),
        step: Rc::new(Cell::new(0)),
        readings: Rc::new(Cell::new(0)),
    };
    let fixed = Rc::new(RefCell::new(vec![over(&[1])]));
    let workflow = Rc::new(RefCell::new(None));
    let seen: ItemReads = Rc::new(RefCell::new(Vec::new()));
    let items = |front| {
        Box::new(Items {
            front,
            workflow: Rc::clone(&workflow),
            seen: Rc::clone(&seen),
        })
    };
    let host = Host::new(Box::new(clock.clone()), Box::new(Fixed(Rc::clone(&fixed))))
        .handler("test", Takes::Runner, items(true))
        .handler("test", Takes::Runner, items(false));
    let mut store = open_entries(&scratch, host, &Bound::Unchanged.entries()).unwrap();
    let mut first = episode(1, DAY - 92, None);
    first.expires = Instant(FAR);
    store
        .commit_batch(&store_id(1), &[first], position(1, 0), &mut Seq(1))
        .unwrap();
    fixed.borrow_mut().clear();
    let id = match store
        .interface_read(&InterfaceRead::workflows(
            &InterfaceRole::claim(),
            RequestId::from_random([8; 16]),
            None,
        ))
        .unwrap()
    {
        ReplyResult::Workflows(page) => page.workflows[0].workflow.id().clone(),
        other => panic!("no workflows page: {other:?}"),
    };
    *workflow.borrow_mut() = Some(id);
    let deadline = i64::from(DAY + 1) * DAY_NS;
    clock.step.set(1);
    clock.at.set(deadline - 1);
    clock.readings.set(0);
    store.handle(&raw_request("runner", "{}")).unwrap();
    let one_request = (seen.borrow().clone(), clock.readings.get());
    seen.borrow_mut().clear();
    store.handle(&raw_request("runner", "{}")).unwrap();
    drop(store);
    assert_eq!(
        one_request,
        (
            vec![("front", 1, 1), ("behind", 1, 1), ("front again", 1, 1)],
            1
        ),
        "(support ids and evidence rows read in front, behind and in front again; readings of the clock) in a \
         request that started one nanosecond before the day of going"
    );
    assert_eq!(
        *seen.borrow(),
        vec![("front", 0, 0), ("behind", 0, 0), ("front again", 0, 0)],
        "the next request starts after the day of going"
    );
}
