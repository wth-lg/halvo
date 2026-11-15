use std::num::NonZeroU32;

use halvo_schema::learning::record::LearnRoutine;
use halvo_schema::{AgentId, RowRead, Summary, SummaryRead, WorkflowId, WriterStep};

use super::fixtures::*;
use crate::routine::LearnStep;
use crate::rows::newest_routine;

fn key(w: u32, a: u32) -> (WorkflowId, AgentId) {
    (
        WorkflowId::parse(&wf(w)).expect("a workflow id"),
        AgentId::parse(&agent(a)).expect("an agent id"),
    )
}

fn row(rev: u32, day: u32, period: u32) -> LearnRoutine {
    let first = fresh(1, DAY);
    LearnRoutine {
        period: NonZeroU32::new(period),
        ..with_figures(&first, rev, day, &figures(&first))
    }
}

fn newest(rows: &[LearnRoutine]) -> Option<LearnRoutine> {
    let (w, a) = key(1, 1);
    newest_routine(rows, &w, &a).cloned()
}

fn read_order(store: &Store) -> Vec<LearnRoutine> {
    let mut rows = Vec::new();
    RowRead::for_each(store, "learn_routine", &mut |json| {
        rows.push(LearnRoutine::parse(json.as_bytes()).expect("a learn_routine row"));
    });
    rows
}

#[test]
fn the_newest_row_is_the_one_of_the_highest_rev_wherever_it_stands() {
    let high = row(7, DAY - 9, 1);
    let low = row(6, DAY, 2);
    assert_eq!(newest(&[high.clone(), low.clone()]), Some(high.clone()));
    assert_eq!(newest(&[low.clone(), high.clone()]), Some(high));
}

#[test]
fn of_two_rows_of_one_rev_the_newest_is_the_one_of_the_later_day_wherever_it_stands() {
    let later = row(7, DAY, 1);
    let earlier = row(7, DAY - 1, 2);
    assert_eq!(
        newest(&[later.clone(), earlier.clone()]),
        Some(later.clone())
    );
    assert_eq!(newest(&[earlier, later.clone()]), Some(later));
}

#[test]
fn of_two_rows_of_one_rev_and_one_day_the_newest_is_the_one_that_comes_last() {
    let one = row(7, DAY, 1);
    let two = row(7, DAY, 2);
    assert_eq!(newest(&[one.clone(), two.clone()]), Some(two.clone()));
    assert_eq!(newest(&[two, one.clone()]), Some(one));
}

#[test]
fn the_newest_row_is_of_the_workflow_and_agent_asked_and_none_where_it_has_no_row() {
    let mine = row(2, DAY - 5, 1);
    let other_workflow = LearnRoutine {
        workflow: key(2, 1).0,
        ..row(9, DAY, 2)
    };
    let other_agent = LearnRoutine {
        agent: key(1, 2).1,
        ..row(9, DAY, 3)
    };
    let rows = [other_workflow.clone(), mine.clone(), other_agent.clone()];
    assert_eq!(newest(&rows), Some(mine));
    assert_eq!(newest(&[other_workflow, other_agent]), None);
    assert_eq!(newest(&[]), None);
}

#[test]
fn of_two_rows_of_one_rev_the_step_and_the_function_go_on_from_the_same_row() {
    let today = DAY + 30;
    for (a, b, day, period) in [
        (row(5, DAY, 4), row(5, DAY - 1, 5), DAY, 4),
        (row(5, DAY, 4), row(5, DAY, 5), DAY, 5),
        (row(5, DAY - 1, 9), row(5, DAY, 2), DAY, 2),
    ] {
        let mut store = Store::default();
        store.put_routine(&a);
        store.put_routine(&b);
        let chosen = newest(&read_order(&store)).expect("a newest row");
        assert_eq!(
            (chosen.day.get(), chosen.period.map(|p| p.get())),
            (day, Some(period))
        );
        let records = store.step(today);
        assert_eq!(records.len(), 1);
        let written = LearnRoutine::parse(records[0].json.as_bytes()).expect("a learn_routine row");
        assert_eq!(written.rev.get(), 6);
        assert_eq!(written.period.map(|p| p.get()), Some(period), "day {day}");
    }
}

struct LaterDayFirst(Vec<String>);

impl RowRead for LaterDayFirst {
    fn for_each(&self, kind: &str, visit: &mut dyn FnMut(&str)) {
        if kind == "learn_routine" {
            for row in &self.0 {
                visit(row);
            }
        }
    }
}

impl SummaryRead for LaterDayFirst {
    fn for_each(&self, _visit: &mut dyn FnMut(&Summary)) {}
}

#[test]
fn of_two_rows_of_one_rev_read_with_the_later_day_first_the_step_and_the_function_go_on_from_the_later_day(
) {
    let today = DAY + 30;
    let later = row(5, DAY, 9);
    let earlier = row(5, DAY - 1, 4);
    let read = LaterDayFirst(vec![
        later.to_json().expect("a writable row"),
        earlier.to_json().expect("a writable row"),
    ]);
    let mut order = Vec::new();
    RowRead::for_each(&read, "learn_routine", &mut |json| {
        order.push(LearnRoutine::parse(json.as_bytes()).expect("a learn_routine row"));
    });
    assert_eq!(order, vec![later.clone(), earlier]);
    assert_eq!(newest(&order), Some(later.clone()));
    let records = LearnStep::new(Fixed(today)).after_commit(&read, &read);
    assert_eq!(records.len(), 1);
    let written = LearnRoutine::parse(records[0].json.as_bytes()).expect("a learn_routine row");
    assert_eq!(
        (written.rev.get(), written.period.map(|p| p.get())),
        (6, Some(9))
    );
}
