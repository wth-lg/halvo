use halvo_schema::learning::names::{Mark, RefinementKind};
use halvo_schema::learning::record::LearnRoutine;
use halvo_schema::{AgentId, Summary, SummaryRead, TypedRecord, WorkflowId, WriterStep};
use serde_json::json;

use super::host::{active_world, agent, wf, World};
use crate::rows::{self, Read};
use crate::{Clock, CoordinatorAnswer, LearnHost, LearnStep};

const LATER: u32 = 100_000;

/// A `learn_routine` row of rev 2 of workflow 1 and agent 1 on `on`: stale since that day after a last occurrence
/// 70 days before it, or not stale with its last occurrence on that day.
fn rev_2(w: &mut World, on: u32, stale: bool) {
    let last = if stale { on - 70 } else { on };
    let row = json!({
        "v": 1, "workflow": wf(1), "agent": agent(1), "rev": 2, "day": on,
        "category": "utility_bill_payment",
        "site": {"key": "northwind-power.test", "source": "address_field"},
        "app": "com.example.browser", "period": 30, "seen": 3, "first": last - 60, "last": last,
        "next": last + 30, "missed": if stale { 2 } else { 0 },
        "mark": if stale { "stale" } else { "none" },
        "since": if stale { json!(on) } else { json!(null) },
    });
    w.host
        .commit(vec![TypedRecord {
            kind: "learn_routine",
            json: row.to_string(),
        }])
        .expect("the row is stored");
}

#[test]
fn of_two_routine_rows_of_one_rev_list_refinements_takes_the_row_of_the_later_day_as_the_step_does()
{
    let mut w = active_world();
    w.host.day = LATER;
    rev_2(&mut w, LATER, true);
    rev_2(&mut w, LATER - 1, false);
    let visited: Vec<u32> = w
        .host
        .learn_rows("learn_routine")
        .iter()
        .map(|row| LearnRoutine::parse(row.as_bytes()).expect("a routine row"))
        .map(|row| row.day.get())
        .collect();
    assert_eq!(visited, [LATER, LATER - 1]);
    let workflow = WorkflowId::parse(&wf(1)).expect("a workflow id");
    let agent_id = AgentId::parse(&agent(1)).expect("an agent id");
    let Read::Rows(routines) = rows::routines(w.host.rows(), &workflow) else {
        panic!("the routine rows are read");
    };
    let newest = rows::newest_routine(&routines, &workflow, &agent_id).expect("a newest row");
    assert_eq!((newest.day.get(), newest.mark), (LATER, Mark::Stale));
    let read =
        w.coordinator(json!({"op": "list_refinements", "workflow": wf(1), "agent": agent(1)}));
    let Ok(CoordinatorAnswer::Refinements(read)) = read else {
        panic!("the read is answered: {read:?}");
    };
    assert_eq!(read.refinements.len(), 1);
    assert_eq!(read.refinements[0].what, RefinementKind::RoutineStale);
    assert_eq!(read.refinements[0].day.get(), LATER);
}

struct NoSummaries;

impl SummaryRead for NoSummaries {
    fn for_each(&self, _visit: &mut dyn FnMut(&Summary)) {}
}

struct Today(u32);

impl Clock for Today {
    fn today(&self) -> u32 {
        self.0
    }
}

#[test]
fn of_two_routine_rows_of_one_rev_and_different_days_the_step_and_list_refinements_take_the_same_row(
) {
    let mut w = active_world();
    w.host.day = LATER + 30;
    rev_2(&mut w, LATER, true);
    rev_2(&mut w, LATER - 1, false);
    let written: Vec<LearnRoutine> = LearnStep::new(Today(w.host.day))
        .after_commit(&NoSummaries, &w.host)
        .into_iter()
        .map(|r| LearnRoutine::parse(r.json.as_bytes()).expect("a routine row"))
        .filter(|r| r.workflow.as_str() == wf(1))
        .collect();
    assert_eq!(written.len(), 1);
    assert_eq!(written[0].rev.get(), 3);
    assert_eq!(
        written[0].first.map(|d| d.get()),
        Some(LATER - 70 - 60),
        "the step went on from the row of the later day"
    );
    let read =
        w.coordinator(json!({"op": "list_refinements", "workflow": wf(1), "agent": agent(1)}));
    let Ok(CoordinatorAnswer::Refinements(read)) = read else {
        panic!("the read is answered: {read:?}");
    };
    assert_eq!(read.refinements.len(), 1);
    assert_eq!(read.refinements[0].what, RefinementKind::RoutineStale);
    assert_eq!(read.refinements[0].day.get(), LATER);
}
