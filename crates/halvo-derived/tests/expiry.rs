mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{
    bodies, episode, open_entries, open_host, position, store_id, summary_id, Grouping,
    NoProposals, Scratch, Seq, TestClock, DAY_NS, NOTE, TRACE,
};
use halvo_derived::{Counts, Host, Report};
use halvo_schema_core::kinds::{FieldType, KindEntry, Origin, RetentionClass};
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
    assert_eq!(
        first.revisions,
        vec![("workflow", 0), ("agent", 0), ("learn", 0)]
    );
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
