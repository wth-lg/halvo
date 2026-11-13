mod common;

use common::*;
use halvo_coord_store::{CommitRefused, DayFault, Host};
use halvo_schema::{RowRead, SummaryRead, TypedRecord, WriterStep};

struct Returns(Vec<TypedRecord>);

impl WriterStep for Returns {
    fn after_commit(&self, _: &dyn SummaryRead, _: &dyn RowRead) -> Vec<TypedRecord> {
        self.0.clone()
    }
}

#[test]
fn commit_stores_a_coord_event_as_given() {
    let mut store = store();
    let json = row(1, TODAY);
    assert_eq!(Host::commit(&mut store, vec![typed(json.clone())]), Ok(()));
    assert_eq!(rows(&store), [json]);
}

#[test]
fn commit_refuses_a_kind_outside_the_family() {
    let mut store = store();
    let other = TypedRecord {
        kind: "agent_log",
        json: row(1, TODAY),
    };
    assert_eq!(
        Host::commit(&mut store, vec![typed(row(1, TODAY)), other]),
        Err(CommitRefused::Kind)
    );
    assert!(rows(&store).is_empty());
}

#[test]
fn commit_refuses_the_whole_list_for_one_record() {
    let mut store = store();
    let unknown_workflow = hold_set(9, 1, 2, TODAY, noon(TODAY));
    assert_eq!(
        Host::commit(
            &mut store,
            vec![typed(row(1, TODAY)), typed(unknown_workflow)]
        ),
        Err(CommitRefused::Workflow)
    );
    let version_two = row(2, TODAY).replacen(r#""v":1"#, r#""v":2"#, 1);
    assert_eq!(
        Host::commit(&mut store, vec![typed(row(1, TODAY)), typed(version_two)]),
        Err(CommitRefused::Record)
    );
    let not_coord = row(2, TODAY).replacen(r#""event":"hold_set""#, r#""event":"other""#, 1);
    assert_eq!(
        Host::commit(&mut store, vec![typed(row(1, TODAY)), typed(not_coord)]),
        Err(CommitRefused::Record)
    );
    assert!(rows(&store).is_empty());
}

#[test]
fn commit_refuses_a_day_after_the_clock_or_outside_its_bound() {
    let mut store = store();
    for (day, refusal) in [
        (TODAY + 1, Some(DayFault::AfterToday)),
        (TODAY + 400, Some(DayFault::AfterToday)),
        (TODAY - 93, Some(DayFault::OutsideBound)),
        (0, Some(DayFault::OutsideBound)),
        (TODAY, None),
        (TODAY - 92, None),
    ] {
        let before = rows(&store).len();
        let answer = Host::commit(&mut store, vec![typed(row(1, day))]);
        match refusal {
            Some(fault) => {
                assert_eq!(answer, Err(CommitRefused::Day(fault)), "day {day}");
                assert_eq!(rows(&store).len(), before, "day {day}");
            }
            None => {
                assert_eq!(answer, Ok(()), "day {day}");
                assert_eq!(rows(&store).len(), before + 1, "day {day}");
            }
        }
    }
    assert_eq!(
        Host::commit(
            &mut store,
            vec![typed(row(1, TODAY)), typed(row(2, TODAY + 1))]
        ),
        Err(CommitRefused::Day(DayFault::AfterToday))
    );
    assert_eq!(rows(&store).len(), 2);
}

#[test]
fn the_clock_day_is_local() {
    let late = at(TODAY, 23 * 3_600 + 1_800, WEST);
    let (mut store, _) = store_at(late, WEST);
    assert_eq!(
        Host::commit(&mut store, vec![typed(hold_set(1, 1, 1, TODAY, late))]),
        Ok(())
    );
    assert_eq!(
        Host::commit(&mut store, vec![typed(hold_set(1, 1, 2, TODAY + 1, late))]),
        Err(CommitRefused::Day(DayFault::AfterToday))
    );
}

#[test]
fn a_step_that_returns_a_coord_event_is_refused() {
    let mut store = store().with_step(Box::new(Returns(vec![typed(row(1, TODAY))])));
    assert_eq!(store.after_batch(), Err(CommitRefused::OwnFromStep));
    assert!(rows(&store).is_empty());

    let mut other = store_at(noon(TODAY), 0)
        .0
        .with_step(Box::new(Returns(vec![TypedRecord {
            kind: "agent_run",
            json: row(1, TODAY),
        }])));
    assert_eq!(other.after_batch(), Err(CommitRefused::Kind));
    assert!(rows(&other).is_empty());

    let mut none = store_at(noon(TODAY), 0)
        .0
        .with_step(Box::new(Returns(Vec::new())));
    assert_eq!(none.after_batch(), Ok(()));
}

#[test]
fn every_step_runs_after_a_batch() {
    let mut store = store()
        .with_step(Box::new(Returns(Vec::new())))
        .with_step(Box::new(Returns(vec![typed(row(1, TODAY))])));
    assert_eq!(store.after_batch(), Err(CommitRefused::OwnFromStep));
    assert!(rows(&store).is_empty());
}

#[test]
fn the_read_hands_out_a_row_two_days_ahead_and_not_three() {
    let (mut store, clock) = store_at(noon(TODAY), 0);
    assert_eq!(Host::commit(&mut store, vec![typed(row(1, TODAY))]), Ok(()));
    clock.set(noon(TODAY - 2));
    assert_eq!(rows(&store), [row(1, TODAY)]);
    clock.set(noon(TODAY - 3));
    assert!(rows(&store).is_empty());
    clock.set(noon(TODAY));
    assert_eq!(rows(&store), [row(1, TODAY)]);
}

#[test]
fn the_read_hands_out_no_row_past_its_bound() {
    let (mut store, clock) = store_at(noon(TODAY), 0);
    assert_eq!(
        Host::commit(
            &mut store,
            vec![typed(row(1, TODAY - 92)), typed(row(2, TODAY))]
        ),
        Ok(())
    );
    clock.set(noon(TODAY + 1));
    assert_eq!(rows(&store), [row(2, TODAY)]);
    clock.set(noon(TODAY + 93));
    assert!(rows(&store).is_empty());
}

#[test]
fn expiry_keeps_92_days_back() {
    let (mut store, clock) = store_at(noon(TODAY), 0);
    assert_eq!(
        Host::commit(
            &mut store,
            vec![typed(row(1, TODAY - 92)), typed(row(2, TODAY))]
        ),
        Ok(())
    );
    assert_eq!(store.expire(), 0);
    assert_eq!(rows(&store).len(), 2);
    clock.set(noon(TODAY + 1));
    assert_eq!(store.expire(), 1);
    assert_eq!(rows(&store), [row(2, TODAY)]);
    clock.set(noon(TODAY + 92));
    assert_eq!(store.expire(), 0);
    clock.set(noon(TODAY + 93));
    assert_eq!(store.expire(), 1);
    assert!(rows(&store).is_empty());
}

#[test]
fn expiry_keeps_a_row_two_days_ahead_of_the_clock() {
    let (mut store, clock) = store_at(noon(TODAY), 0);
    assert_eq!(
        Host::commit(
            &mut store,
            vec![typed(row(1, TODAY - 1)), typed(row(2, TODAY))]
        ),
        Ok(())
    );
    clock.set(noon(TODAY - 2));
    assert_eq!(store.expire(), 0);
    assert_eq!(rows(&store).len(), 2);
    clock.set(noon(TODAY - 3));
    assert_eq!(store.expire(), 1);
    assert_eq!(rows(&store), [row(1, TODAY - 1)]);
    clock.set(noon(TODAY - 4));
    assert_eq!(store.expire(), 1);
    assert!(rows(&store).is_empty());
}

#[test]
fn the_store_reads_the_workflow_it_holds() {
    let store = store();
    let item = Host::workflow(&store, &halvo_schema::WorkflowId::parse(&wf(1)).unwrap())
        .expect("workflow 1");
    assert_eq!(item.workflow, workflow(1, Some(1)));
    assert!(Host::workflow(&store, &halvo_schema::WorkflowId::parse(&wf(3)).unwrap()).is_none());
    assert_eq!(
        Host::clock(&store).day(halvo_schema::Instant(noon(TODAY))),
        TODAY
    );
}
