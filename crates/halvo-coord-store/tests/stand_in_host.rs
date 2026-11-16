mod common;

use common::*;
use halvo_coord_store::{coordinator, CommitRefused, DayFault, Host};
use halvo_schema::ErrorName;

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
