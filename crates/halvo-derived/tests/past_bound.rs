//! DS-4: a read hands out exactly the rows that an expiry at that read's clock would leave, and no read writes.

mod common;

use std::cell::RefCell;
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
}

impl WriterStep for Probe {
    fn after_commit(&self, summaries: &dyn SummaryRead, rows: &dyn RowRead) -> Vec<TypedRecord> {
        let mut seen = self.seen.borrow_mut();
        summaries.for_each(&mut |s| seen.push(format!("step summary {}", s.id.as_str())));
        for kind in KINDS {
            rows.for_each(kind, &mut |row| seen.push(format!("step {kind} {row}")));
        }
        self.emit.borrow_mut().drain(..).collect()
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
        let host = |fixed: &Rc<RefCell<Vec<Proposal>>>| {
            let sink = Rc::clone(&reports);
            Host::new(Box::new(clock.clone()), Box::new(Fixed(Rc::clone(fixed))))
                .step(
                    "test",
                    Box::new(Probe {
                        emit: Rc::clone(&emit),
                        seen: Rc::clone(&seen),
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
            at: 3,
        }
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
        let evidence = self
            .store
            .interface_read(&InterfaceRead::evidence(
                &role,
                RequestId::from_random([3; 16]),
                self.workflow.clone(),
                None,
            ))
            .unwrap();
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
