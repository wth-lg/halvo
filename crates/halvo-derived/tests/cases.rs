mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{
    episode, key, listed, own_note_body, position, raw, raw_request, registry, store_id,
    Grouping, OwnNote, Scratch, Seq, TestClock, OWN,
};
use halvo_derived::{Cause, Error, Host, Refusal, Store, Takes};
use halvo_schema_core::ids::RequestId;
use halvo_schema_core::record::{RowRead, SummaryRead, TypedRecord, WriterStep};
use halvo_schema_core::request::{ErrorName, InterfaceDecision, InterfaceRole};

const DAY: u32 = 20_767;

fn count(scratch: &Scratch, sql: &str) -> i64 {
    raw(scratch).query_row(sql, [], |r| r.get(0)).unwrap()
}

/// The second test step of own_origin_refused: after a commit it returns one record of the kind of origin own
/// `test_own` for the workflow that the test put in its cell, and nothing while the cell is empty.
struct OwnStep(Rc<RefCell<Option<String>>>);

impl WriterStep for OwnStep {
    fn after_commit(&self, _summaries: &dyn SummaryRead, _rows: &dyn RowRead) -> Vec<TypedRecord> {
        match &*self.0.borrow() {
            None => Vec::new(),
            Some(workflow) => vec![TypedRecord {
                kind: "test_own",
                json: format!(
                    r#"{{"v":1,"workflow":"{workflow}","note":"Call the landlord","at":1794339000000000000}}"#
                ),
            }],
        }
    }
}

fn own_host(clock: &TestClock, step: &Rc<RefCell<Option<String>>>) -> Host {
    Host::new(Box::new(clock.clone()), Box::new(Grouping { min: 1 }))
        .step("test", Box::new(OwnStep(Rc::clone(step))))
        .handler("test", Takes::Runner, Box::new(OwnNote))
        .handler("test", Takes::Coordinator(&["own_note"]), Box::new(OwnNote))
        .handler("test", Takes::Role("test_role"), Box::new(OwnNote))
}

fn open_own(scratch: &Scratch, clock: &TestClock, step: &Rc<RefCell<Option<String>>>) -> Store {
    let mut entries = registry();
    entries.push(&OWN);
    Store::open(scratch.path(), &key(), &entries, own_host(clock, step), &mut Seq(1 << 40)).unwrap()
}

const OWN_ROWS: &str = "SELECT count(*) FROM rows WHERE kind = 'test_own'";

/// own_origin_refused: with a second test-only kind, of origin own, with a workflow key and a field of the type
/// own_text, the store refuses one record of that kind, and stores no row of it, when the second test step returns it
/// after a commit and when a test handler commits it for a request of a further role; it stores the record when a test
/// handler commits it for a `runner` request and for a `coordinator_write`; and after the delete of the workflow that
/// the records name no row of that kind is left (contracts.md C25.8).
#[test]
fn own_origin_refused() {
    let scratch = Scratch::new("own-origin");
    let clock = TestClock::at_day(DAY);
    let step = Rc::new(RefCell::new(None));
    let mut store = open_own(&scratch, &clock, &step);
    let first = store
        .commit_batch(&store_id(1), &[episode(1, DAY, None)], position(1, 0), &mut Seq(100))
        .unwrap();
    assert_eq!(first.proposals.stored, 1);
    let workflow = listed(&store)[0].workflow.id().clone();
    *step.borrow_mut() = Some(workflow.as_str().to_owned());
    let by_step = store
        .commit_batch(&store_id(1), &[], position(2, 0), &mut Seq(200))
        .unwrap();
    assert_eq!(
        by_step.steps,
        vec![Err(Refusal {
            index: 0,
            cause: Cause::OwnOrigin
        })],
        "the record of origin own that the second step returns"
    );
    *step.borrow_mut() = None;
    let body = own_note_body(workflow.as_str(), "Call the landlord");
    assert_eq!(
        store.handle(&raw_request("test_role", &body)).err(),
        Some(Error::Refused(ErrorName::Role)),
        "the record of origin own that a handler of a further role commits"
    );
    drop(store);
    assert_eq!(count(&scratch, OWN_ROWS), 0, "rows of origin own after the two refusals");
    let mut store = open_own(&scratch, &clock, &step);
    store.handle(&raw_request("runner", &body)).unwrap();
    store
        .handle(&raw_request("coordinator_write", &body))
        .unwrap();
    drop(store);
    assert_eq!(count(&scratch, OWN_ROWS), 2, "rows of origin own for a runner request and a coordinator_write");
    let mut store = open_own(&scratch, &clock, &step);
    store
        .decide(&InterfaceDecision::delete(
            &InterfaceRole::claim(),
            RequestId::from_random([2; 16]),
            workflow,
        ))
        .unwrap();
    drop(store);
    assert_eq!(count(&scratch, OWN_ROWS), 0, "rows of origin own after the delete of their workflow");
}
