mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{
    bodies, episode, families, open_entries, open_host, position, raw, registry, store_id,
    summary_id, Grouping, NoProposals, Scratch, Seq, TestClock, DAY_NS, NOTE, OWN, TRACE,
};
use halvo_derived::{Committed, Counts, Host, Report};
use halvo_schema_core::kinds::{self, FieldType, KindEntry, Origin, RetentionClass};
use halvo_schema_core::record::{Instant, RowRead, Summary, SummaryRead, TypedRecord, WriterStep};

const DAY: u32 = 20_767;

static SHORT: KindEntry = KindEntry {
    name: "test_short",
    class: RetentionClass::Bounded { days: 35 },
    origin: Origin::Capture,
    family: "test",
    workflow_key: None,
    fields: &[("v", FieldType::Version), ("day", FieldType::Day)],
    example: r#"{"v":1,"day":20767}"#,
};

struct Emit(Rc<RefCell<Vec<TypedRecord>>>);

impl WriterStep for Emit {
    fn after_commit(&self, _summaries: &dyn SummaryRead, _rows: &dyn RowRead) -> Vec<TypedRecord> {
        self.0.borrow_mut().drain(..).collect()
    }
}

fn summary_ids(scratch: &Scratch) -> Vec<String> {
    let mut ids: Vec<String> = bodies(scratch, "summary")
        .iter()
        .map(|b| {
            serde_json::from_str::<Summary>(b)
                .unwrap()
                .id
                .as_str()
                .to_owned()
        })
        .collect();
    ids.sort();
    ids
}

fn id(n: u64) -> String {
    summary_id(n).as_str().to_owned()
}

fn plain(clock: &TestClock) -> Host {
    Host::new(Box::new(clock.clone()), Box::new(NoProposals))
}

#[test]
fn expiry_at_open_removes_each_bounded_row_past_its_bound_or_stamped_after_the_clock() {
    let scratch = Scratch::new("expiry-bounded");
    let clock = TestClock::at_day(DAY);
    let emitted = Rc::new(RefCell::new(Vec::new()));
    let host = |clock: &TestClock| {
        Host::new(Box::new(clock.clone()), Box::new(Grouping { min: 1 }))
            .step("test", Box::new(Emit(Rc::clone(&emitted))))
    };
    let mut store = open_entries(&scratch, host(&clock), &[&SHORT]).unwrap();
    for (n, day) in [(1, DAY), (2, DAY - 1)] {
        emitted.borrow_mut().push(TypedRecord {
            kind: "test_short",
            json: format!(r#"{{"v":1,"day":{day}}}"#),
        });
        store
            .commit_batch(
                &store_id(1),
                &[episode(n, day, None)],
                position(n, 0),
                &mut Seq(n * 100),
            )
            .unwrap();
    }
    clock.set_day(DAY + 400);
    let mut ahead = episode(3, DAY + 400, None);
    ahead.app = halvo_schema_core::site::AppId::parse("com.example.other").unwrap();
    store
        .commit_batch(&store_id(1), &[ahead], position(3, 0), &mut Seq(300))
        .unwrap();
    drop(store);
    assert_eq!(bodies(&scratch, "workflow").len(), 2);
    assert_eq!(bodies(&scratch, "support").len(), 3);
    assert_eq!(summary_ids(&scratch), vec![id(1), id(2), id(3)]);
    clock.set_day(DAY + 34);
    drop(open_entries(&scratch, plain(&clock), &[&SHORT]).unwrap());
    assert_eq!(summary_ids(&scratch), vec![id(1), id(2)]);
    assert_eq!(bodies(&scratch, "test_short").len(), 1);
    clock.set_day(DAY + 92);
    drop(open_entries(&scratch, plain(&clock), &[&SHORT]).unwrap());
    assert_eq!(summary_ids(&scratch), vec![id(1)]);
    assert_eq!(bodies(&scratch, "test_short").len(), 0);
    assert_eq!(bodies(&scratch, "evidence").len(), 1);
    let support = bodies(&scratch, "support");
    assert_eq!(support.len(), 1);
    assert!(support[0].contains(&id(1)));
    assert_eq!(bodies(&scratch, "workflow").len(), 2);
    clock.set_day(DAY + 93);
    drop(open_entries(&scratch, plain(&clock), &[&SHORT]).unwrap());
    assert_eq!(summary_ids(&scratch), Vec::<String>::new());
    assert_eq!(bodies(&scratch, "support").len(), 0);
    assert_eq!(bodies(&scratch, "evidence").len(), 0);
    assert_eq!(bodies(&scratch, "workflow").len(), 2);
}

#[test]
fn expiry_at_open_removes_each_window_row_whose_expiry_has_passed() {
    let scratch = Scratch::new("expiry-window");
    let clock = TestClock::at_day(DAY);
    let mut store = open_entries(&scratch, plain(&clock), &[&TRACE, &NOTE]).unwrap();
    let mut soon = episode(1, DAY, None);
    soon.expires = Instant(clock.now() + 1_000);
    let mut later = episode(2, DAY, None);
    later.expires = Instant(clock.now() + 2_000);
    store
        .commit_batch(&store_id(1), &[soon, later], position(1, 0), &mut Seq(1))
        .unwrap();
    drop(store);
    clock.set(clock.now() + 1_000);
    drop(open_entries(&scratch, plain(&clock), &[&TRACE, &NOTE]).unwrap());
    let left = bodies(&scratch, "evidence");
    assert_eq!(left.len(), 1);
    assert!(left[0].contains(&id(2)));
    assert_eq!(summary_ids(&scratch), vec![id(1), id(2)]);
    assert_eq!(common::count(&scratch, "SELECT count(*) FROM sources"), 1);
}

#[test]
fn expire_runs_the_expiry_of_an_open_on_the_clock_of_the_store() {
    let scratch = Scratch::new("expire-call");
    let clock = TestClock::at_day(DAY);
    let mut store = open_host(&scratch, plain(&clock)).unwrap();
    let mut evidence_soon = episode(2, DAY, None);
    evidence_soon.expires = Instant(clock.now() + 10);
    store
        .commit_batch(
            &store_id(1),
            &[episode(1, DAY - 92, None), evidence_soon],
            position(1, 0),
            &mut Seq(1),
        )
        .unwrap();
    assert_eq!(store.expire().unwrap(), 0);
    clock.set_day(DAY + 1);
    assert_eq!(store.expire().unwrap(), 3);
    assert_eq!(store.expire().unwrap(), 0);
    drop(store);
    assert_eq!(summary_ids(&scratch), vec![id(2)]);
    assert_eq!(bodies(&scratch, "evidence").len(), 0);
}

#[test]
fn the_store_gives_its_host_the_report_after_each_committed_transaction() {
    let scratch = Scratch::new("report");
    let clock = TestClock::at_day(DAY);
    let reports: Rc<RefCell<Vec<Report>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&reports);
    let host = Host::new(Box::new(clock.clone()), Box::new(Grouping { min: 2 })).on_report(
        Box::new(move |report: &Report| sink.borrow_mut().push(report.clone())),
    );
    let mut store = open_host(&scratch, host).unwrap();
    assert_eq!(reports.borrow().len(), 1);
    let first = reports.borrow()[0].clone();
    let expected: Vec<(&str, u64)> = families(&registry()).into_iter().map(|f| (f, 0)).collect();
    assert_eq!(
        first.revisions, expected,
        "one revision per family of the registry, in the order of its first entry, each 0"
    );
    assert_eq!(first.revisions[0].0, "workflow");
    let distinct: std::collections::BTreeSet<&str> = kinds::registry().map(|e| e.family).collect();
    assert_eq!(first.revisions.len(), distinct.len());
    assert_eq!(
        first.counts,
        Counts {
            window: 0,
            bounded: 0,
            kept: 0
        }
    );
    assert_eq!(first.expiry, None);
    assert!(first.disk_bytes > 0);
    let mut soon = episode(1, DAY - 10, None);
    soon.expires = Instant(clock.now() + 5_000);
    store
        .commit_batch(
            &store_id(1),
            &[soon, episode(2, DAY, None)],
            position(1, 0),
            &mut Seq(1),
        )
        .unwrap();
    let seen = reports.borrow().clone();
    assert_eq!(seen.len(), 3);
    assert_eq!(seen[1].revisions[0], ("workflow", 1));
    assert_eq!(
        seen[1].counts,
        Counts {
            window: 2,
            bounded: 2,
            kept: 0
        }
    );
    assert_eq!(seen[2].revisions[0], ("workflow", 2));
    assert_eq!(
        seen[2].counts,
        Counts {
            window: 2,
            bounded: 4,
            kept: 1
        }
    );
    assert_eq!(seen[2].expiry, Some(Instant(clock.now() + 5_000)));
    clock.set(clock.now() + 5_000);
    assert_eq!(store.expire().unwrap(), 1);
    let last = reports.borrow().last().cloned().unwrap();
    assert_eq!(
        last.counts,
        Counts {
            window: 1,
            bounded: 4,
            kept: 1
        }
    );
    assert_eq!(
        last.expiry,
        Some(Instant(i64::from(DAY - 10 + 93) * DAY_NS))
    );
    assert_eq!(store.report().unwrap(), last);
}

static BOUND_93: KindEntry = KindEntry {
    name: "test_bound",
    class: RetentionClass::Bounded { days: 93 },
    origin: Origin::Capture,
    family: "test",
    workflow_key: None,
    fields: &[("v", FieldType::Version), ("day", FieldType::Day)],
    example: r#"{"v":1,"day":20767}"#,
};

static BOUND_10: KindEntry = KindEntry {
    class: RetentionClass::Bounded { days: 10 },
    ..BOUND_93
};

#[test]
fn expiry_at_open_reads_the_bound_of_the_entry_that_names_the_kind() {
    let scratch = Scratch::new("expiry-entry-bound");
    let clock = TestClock::at_day(DAY);
    let emitted = Rc::new(RefCell::new(vec![TypedRecord {
        kind: "test_bound",
        json: format!(r#"{{"v":1,"day":{DAY}}}"#),
    }]));
    let host = Host::new(Box::new(clock.clone()), Box::new(NoProposals))
        .step("test", Box::new(Emit(Rc::clone(&emitted))));
    let mut store = open_entries(&scratch, host, &[&BOUND_93]).unwrap();
    store
        .commit_batch(&store_id(1), &[], position(1, 0), &mut Seq(1))
        .unwrap();
    drop(store);
    assert_eq!(bodies(&scratch, "test_bound").len(), 1);
    clock.set_day(DAY + 20);
    drop(open_entries(&scratch, plain(&clock), &[&BOUND_93]).unwrap());
    assert_eq!(bodies(&scratch, "test_bound").len(), 1);
    clock.set_day(DAY + 9);
    drop(open_entries(&scratch, plain(&clock), &[&BOUND_10]).unwrap());
    assert_eq!(bodies(&scratch, "test_bound").len(), 1);
    clock.set_day(DAY + 10);
    drop(open_entries(&scratch, plain(&clock), &[&BOUND_10]).unwrap());
    assert_eq!(bodies(&scratch, "test_bound").len(), 0);
}

#[test]
fn expiry_at_open_removes_a_row_past_its_bound_of_a_kind_that_the_open_does_not_name() {
    let scratch = Scratch::new("expiry-unnamed-kind");
    let clock = TestClock::at_day(DAY);
    let emitted = Rc::new(RefCell::new(vec![TypedRecord {
        kind: "test_bound",
        json: format!(r#"{{"v":1,"day":{DAY}}}"#),
    }]));
    let host = Host::new(Box::new(clock.clone()), Box::new(NoProposals))
        .step("test", Box::new(Emit(Rc::clone(&emitted))));
    let mut store = open_entries(&scratch, host, &[&BOUND_10]).unwrap();
    store
        .commit_batch(&store_id(1), &[], position(1, 0), &mut Seq(1))
        .unwrap();
    drop(store);
    clock.set_day(DAY + 9);
    drop(open_host(&scratch, plain(&clock)).unwrap());
    assert_eq!(bodies(&scratch, "test_bound").len(), 1);
    clock.set_day(DAY + 10);
    drop(open_host(&scratch, plain(&clock)).unwrap());
    assert_eq!(bodies(&scratch, "test_bound").len(), 0);
}

static NOTE_10: KindEntry = KindEntry {
    class: RetentionClass::Bounded { days: 10 },
    ..NOTE
};

#[test]
fn the_report_gives_the_deadline_of_the_bound_that_the_store_expires_by() {
    let scratch = Scratch::new("deadline-shortened");
    let clock = TestClock::at_day(DAY);
    let emitted = Rc::new(RefCell::new(Vec::new()));
    let host = || {
        Host::new(Box::new(clock.clone()), Box::new(Grouping { min: 1 }))
            .step("test", Box::new(Emit(Rc::clone(&emitted))))
    };
    let mut store = open_entries(&scratch, host(), &[&NOTE]).unwrap();
    store
        .commit_batch(
            &store_id(1),
            &[episode(1, DAY, None)],
            position(1, 0),
            &mut Seq(100),
        )
        .unwrap();
    drop(store);
    let stored: halvo_schema_core::record::Workflow =
        serde_json::from_str(&bodies(&scratch, "workflow")[0]).unwrap();
    emitted.borrow_mut().push(TypedRecord {
        kind: "test_note",
        json: format!(
            r#"{{"v":1,"workflow":"{}","day":{DAY},"n":1}}"#,
            stored.id().as_str()
        ),
    });
    let mut store = open_entries(&scratch, host(), &[&NOTE]).unwrap();
    store
        .commit_batch(&store_id(1), &[], position(2, 0), &mut Seq(200))
        .unwrap();
    drop(store);
    let store = open_entries(&scratch, plain(&clock), &[&NOTE_10]).unwrap();
    let report = store.report().unwrap();
    drop(store);
    assert_eq!(report.expiry, Some(Instant(1_795_132_800_000_000_000)));
    assert_eq!(report.expiry, Some(Instant(i64::from(DAY + 10) * DAY_NS)));
}

static B1_30: KindEntry = KindEntry {
    name: "test_b1",
    class: RetentionClass::Bounded { days: 30 },
    origin: Origin::Capture,
    family: "test",
    workflow_key: None,
    fields: &[("v", FieldType::Version), ("day", FieldType::Day)],
    example: r#"{"v":1,"day":20767}"#,
};

static B2_40: KindEntry = KindEntry {
    name: "test_b2",
    class: RetentionClass::Bounded { days: 40 },
    ..B1_30
};

static B2_20: KindEntry = KindEntry {
    class: RetentionClass::Bounded { days: 20 },
    ..B2_40
};

static B3_10: KindEntry = KindEntry {
    name: "test_b3",
    class: RetentionClass::Bounded { days: 10 },
    ..B1_30
};

static B3_50: KindEntry = KindEntry {
    class: RetentionClass::Bounded { days: 50 },
    ..B3_10
};

static W_ONE: KindEntry = KindEntry {
    name: "test_w",
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

static A1_5: KindEntry = KindEntry {
    name: "test_a1",
    class: RetentionClass::Bounded { days: 5 },
    ..B1_30
};

static A2_15: KindEntry = KindEntry {
    name: "test_a2",
    class: RetentionClass::Bounded { days: 15 },
    ..B1_30
};

#[test]
fn expire_at_the_reported_deadline_removes_a_row_and_one_nanosecond_before_it_none() {
    let scratch = Scratch::new("deadline-rule");
    let clock = TestClock::at_day(DAY + 2);
    let window_ends = i64::from(DAY + 25) * DAY_NS + 3_600_000_000_000;
    let emitted = Rc::new(RefCell::new(
        ["test_b1", "test_b2", "test_b3"]
            .iter()
            .map(|kind| TypedRecord {
                kind,
                json: format!(r#"{{"v":1,"day":{DAY}}}"#),
            })
            .chain([TypedRecord {
                kind: "test_w",
                json: format!(r#"{{"v":1,"segments":[5],"expires":{window_ends}}}"#),
            }])
            .chain(
                [("test_a1", DAY + 1), ("test_a2", DAY + 2)].map(|(kind, day)| TypedRecord {
                    kind,
                    json: format!(r#"{{"v":1,"day":{day}}}"#),
                }),
            )
            .collect::<Vec<_>>(),
    ));
    let host = Host::new(Box::new(clock.clone()), Box::new(NoProposals))
        .step("test", Box::new(Emit(Rc::clone(&emitted))));
    let mut store = open_entries(
        &scratch,
        host,
        &[&B1_30, &B2_40, &B3_10, &W_ONE, &A1_5, &A2_15],
    )
    .unwrap();
    store
        .commit_batch(&store_id(1), &[], position(1, 0), &mut Seq(1))
        .unwrap();
    drop(store);
    assert_eq!(bodies(&scratch, "test_b1").len(), 1);
    clock.set_day(DAY);
    let mut store = open_entries(
        &scratch,
        plain(&clock),
        &[&B1_30, &B2_20, &B3_50, &W_ONE, &A1_5, &A2_15],
    )
    .unwrap();
    let mut deadlines = Vec::new();
    while let Some(deadline) = store.report().unwrap().expiry {
        clock.set(deadline.0 - 1);
        assert_eq!(
            store.expire().unwrap(),
            0,
            "one nanosecond before {deadline:?}"
        );
        clock.set(deadline.0);
        assert!(store.expire().unwrap() >= 1, "at {deadline:?}");
        deadlines.push(deadline.0);
    }
    drop(store);
    assert_eq!(
        deadlines,
        vec![
            i64::from(DAY + 6) * DAY_NS,
            i64::from(DAY + 10) * DAY_NS,
            i64::from(DAY + 17) * DAY_NS,
            i64::from(DAY + 20) * DAY_NS,
            window_ends,
            i64::from(DAY + 30) * DAY_NS,
        ],
        "the row stored under 10 days goes on its day plus 10 although its entry now gives 50, and the rows one and \
         two days after the clock go on their own day of going"
    );
}

fn b1(day: u32) -> TypedRecord {
    TypedRecord {
        kind: "test_b1",
        json: format!(r#"{{"v":1,"day":{day}}}"#),
    }
}

#[test]
fn a_row_one_or_two_days_after_the_clock_stays_and_a_row_three_days_after_it_goes_at_the_next_expiry(
) {
    let scratch = Scratch::new("ahead-two-days");
    let clock = TestClock::at_day(DAY);
    let emitted = Rc::new(RefCell::new(vec![b1(DAY), b1(DAY), b1(DAY - 1)]));
    let host = Host::new(Box::new(clock.clone()), Box::new(NoProposals))
        .step("test", Box::new(Emit(Rc::clone(&emitted))));
    let mut store = open_entries(&scratch, host, &[&B1_30]).unwrap();
    store
        .commit_batch(&store_id(1), &[], position(1, 0), &mut Seq(1))
        .unwrap();
    drop(store);
    let stored = bodies(&scratch, "test_b1");
    assert_eq!(stored.len(), 3);
    let first_gone = Some(Instant(i64::from(DAY - 1 + 30) * DAY_NS));
    let mut store = open_entries(&scratch, plain(&clock), &[&B1_30]).unwrap();
    let mut seen = Vec::new();
    for back in [1, 2] {
        clock.set_day(DAY - back);
        let report = store.report().unwrap();
        seen.push((
            back,
            store.expire().unwrap(),
            report.counts.bounded,
            report.expiry,
        ));
    }
    drop(store);
    assert_eq!(
        seen,
        vec![(1, 0, 3, first_gone), (2, 0, 3, first_gone)],
        "(days back, rows removed, bounded rows, deadline)"
    );
    assert_eq!(bodies(&scratch, "test_b1"), stored);
    let mut store = open_entries(&scratch, plain(&clock), &[&B1_30]).unwrap();
    clock.set_day(DAY - 3);
    let report = store.report().unwrap();
    assert_eq!(
        (report.counts.bounded, report.expiry),
        (3, Some(Instant(clock.now()))),
        "a row more than two days ahead is due at the clock's instant"
    );
    assert_eq!(store.expire().unwrap(), 2);
    let report = store.report().unwrap();
    assert_eq!((report.counts.bounded, report.expiry), (1, first_gone));
    drop(store);
    assert_eq!(bodies(&scratch, "test_b1"), vec![b1(DAY - 1).json]);
}

fn kinds_stored(scratch: &Scratch) -> Vec<String> {
    raw(scratch)
        .prepare("SELECT kind FROM rows ORDER BY kind")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn a_bounded_row_with_no_day_goes_at_the_next_expiry_and_a_kept_or_window_row_with_none_stays() {
    let scratch = Scratch::new("bounded-no-day");
    let clock = TestClock::at_day(DAY);
    let entries: &[&'static KindEntry] = &[&B1_30, &OWN, &W_ONE];
    drop(open_entries(&scratch, plain(&clock), entries).unwrap());
    let far: i64 = 9_000_000_000_000_000_000;
    raw(&scratch)
        .execute_batch(&format!(
            "INSERT INTO rows (kind, family, class, body) VALUES ('test_b1', 'test', 1, '{{\"v\":1}}'), \
             ('test_own', 'test', 2, '{{\"v\":1}}'); \
             INSERT INTO rows (kind, family, class, expires, body) VALUES ('test_w', 'test', 0, {far}, \
             '{{\"v\":1}}');"
        ))
        .unwrap();
    assert_eq!(kinds_stored(&scratch), ["test_b1", "test_own", "test_w"]);
    let mut store = open_entries(&scratch, plain(&clock), entries).unwrap();
    let report = store.report().unwrap();
    let removed = store.expire().unwrap();
    drop(store);
    assert_eq!(
        (
            kinds_stored(&scratch),
            report.counts,
            report.expiry,
            removed
        ),
        (
            vec!["test_own".to_owned(), "test_w".to_owned()],
            Counts {
                window: 1,
                bounded: 0,
                kept: 1
            },
            Some(Instant(far)),
            0
        ),
        "(kinds left after the open, counts, deadline, rows that a second expiry removes)"
    );
    clock.set_day(DAY + 400);
    drop(open_entries(&scratch, plain(&clock), entries).unwrap());
    assert_eq!(kinds_stored(&scratch), ["test_own", "test_w"]);
}

/// A kind of a family that no file of this crate names, as a later run's crate registers one beside the registry.
static LATER: KindEntry = KindEntry {
    name: "zq_later_note",
    class: RetentionClass::Bounded { days: 93 },
    origin: Origin::Capture,
    family: "zq_later",
    workflow_key: None,
    fields: &[("v", FieldType::Version), ("day", FieldType::Day)],
    example: r#"{"v":1,"day":20767}"#,
};

#[test]
fn a_kind_of_a_new_family_beside_the_registry_gets_its_revision_its_place_in_the_report_and_its_count(
) {
    let scratch = Scratch::new("later-family");
    let clock = TestClock::at_day(DAY);
    let emitted = Rc::new(RefCell::new(vec![TypedRecord {
        kind: "zq_later_note",
        json: format!(r#"{{"v":1,"day":{DAY}}}"#),
    }]));
    let reports: Rc<RefCell<Vec<Report>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&reports);
    let host = Host::new(Box::new(clock.clone()), Box::new(NoProposals))
        .step("zq_later", Box::new(Emit(Rc::clone(&emitted))))
        .on_report(Box::new(move |report: &Report| {
            sink.borrow_mut().push(report.clone())
        }));
    let mut store = open_entries(&scratch, host, &[&LATER]).unwrap();
    let mut want: Vec<(&str, u64)> = families(&registry()).into_iter().map(|f| (f, 0)).collect();
    want.push(("zq_later", 0));
    assert_eq!(
        reports.borrow()[0].revisions,
        want,
        "the new family stands after the registry's families, with 0"
    );
    let outcome = store
        .commit_batch(&store_id(1), &[], position(1, 0), &mut Seq(1))
        .unwrap();
    assert_eq!(
        outcome.steps,
        vec![Ok(Committed {
            stored: 1,
            dropped: 0
        })]
    );
    let last = store.report().unwrap();
    drop(store);
    let names = |revisions: &[(&'static str, u64)]| -> Vec<&'static str> {
        revisions.iter().map(|(family, _)| *family).collect()
    };
    assert_eq!(names(&last.revisions), names(&want));
    assert_eq!(last.revisions.last(), Some(&("zq_later", 1)));
    assert_eq!(
        last.counts,
        Counts {
            window: 0,
            bounded: 1,
            kept: 0
        }
    );
    let stored: i64 = raw(&scratch)
        .query_row(
            "SELECT n FROM revisions WHERE family = 'zq_later'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(stored, 1);
}
