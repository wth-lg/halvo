mod common;

use common::*;
use halvo_coord_store::{coordinator, Clock, CommitRefused, DayFault, Host, MemoryStore};
use halvo_schema::{ErrorName, RowRead, TypedRecord, WorkflowId, WorkflowItem};

struct StandIn {
    store: MemoryStore,
    extra: Vec<String>,
    refusal: Option<CommitRefused>,
}

impl RowRead for StandIn {
    fn for_each(&self, kind: &str, visit: &mut dyn FnMut(&str)) {
        self.store.for_each(kind, visit);
        for row in &self.extra {
            visit(row);
        }
    }
}

impl Host for StandIn {
    fn commit(&mut self, records: Vec<TypedRecord>) -> Result<(), CommitRefused> {
        match self.refusal {
            Some(refused) => Err(refused),
            None => Host::commit(&mut self.store, records),
        }
    }

    fn rows(&self) -> &dyn RowRead {
        self
    }

    fn workflow(&self, id: &WorkflowId) -> Option<WorkflowItem> {
        Host::workflow(&self.store, id)
    }

    fn clock(&self) -> &dyn Clock {
        Host::clock(&self.store)
    }
}

fn stand_in(extra: Vec<String>, refusal: Option<CommitRefused>) -> StandIn {
    StandIn {
        store: store(),
        extra,
        refusal,
    }
}

#[test]
fn each_refusal_of_the_commit_has_its_error_name() {
    for (refusal, error) in [
        (
            CommitRefused::Day(DayFault::AfterToday),
            ErrorName::Malformed,
        ),
        (
            CommitRefused::Day(DayFault::OutsideBound),
            ErrorName::Malformed,
        ),
        (CommitRefused::Record, ErrorName::Malformed),
        (CommitRefused::Workflow, ErrorName::NoWorkflow),
        (CommitRefused::Kind, ErrorName::Store),
        (CommitRefused::OwnFromStep, ErrorName::Store),
    ] {
        let mut host = stand_in(Vec::new(), Some(refusal));
        assert_eq!(
            coordinator(&mut host, &log(&[row(1, TODAY)])),
            Err(error),
            "{refusal:?}"
        );
        assert!(rows(&host.store).is_empty(), "{refusal:?}");
    }
}

#[test]
fn a_stored_row_that_the_reader_refuses_is_store() {
    let unread = row(1, TODAY).replacen(r#""v":1"#, r#""v":2"#, 1);
    let mut host = stand_in(vec![unread.clone()], None);
    assert_eq!(
        coordinator(&mut host, &log(&[row(1, TODAY)])),
        Err(ErrorName::Store)
    );
    assert!(rows(&host.store).is_empty());
    let mut host = stand_in(vec![unread], None);
    assert_eq!(
        coordinator(&mut host, &history(None)),
        Err(ErrorName::Store)
    );
}

#[test]
fn one_unreadable_row_of_another_agent_is_store_for_every_agent() {
    let mut readable = store();
    assert_eq!(
        Host::commit(
            &mut readable,
            vec![typed(row(1, TODAY)), typed(row(2, TODAY))]
        ),
        Ok(())
    );
    let theirs = hold_set(2, 2, 1, TODAY, noon(TODAY)).replacen(r#""v":1"#, r#""v":2"#, 1);
    let mut host = StandIn {
        store: readable,
        extra: vec![theirs],
        refusal: None,
    };
    assert_eq!(
        coordinator(&mut host, &log(&[row(3, TODAY)])),
        Err(ErrorName::Store)
    );
    assert_eq!(
        coordinator(&mut host, &history(None)),
        Err(ErrorName::Store)
    );
    assert_eq!(rows(&host.store).len(), 2);
}

#[test]
fn two_stored_rows_of_one_seq_are_store() {
    let mut host = stand_in(vec![row(1, TODAY), row(1, TODAY)], None);
    assert_eq!(
        coordinator(&mut host, &history(None)),
        Err(ErrorName::Store)
    );
}

#[test]
fn rows_of_another_workflow_or_agent_count_for_neither() {
    let mut host = stand_in(
        vec![
            hold_set(2, 1, 1, TODAY, noon(TODAY)),
            hold_set(2, 1, 2, TODAY, noon(TODAY)),
            hold_set(1, 2, 1, TODAY, noon(TODAY)),
            hold_set(1, 2, 2, TODAY, noon(TODAY)),
            hold_set(1, 2, 3, TODAY, noon(TODAY)),
        ],
        None,
    );
    let Ok(halvo_coord_store::CoordReply::History(h)) = coordinator(&mut host, &history(None))
    else {
        panic!("no history");
    };
    assert!(h.records.is_empty());
    assert_eq!(h.last, 0);
    assert_eq!(coordinator(&mut host, &log(&[row(1, TODAY)])), stored(1));
}
