use halvo_schema::executable::CoordinatorReply;
use halvo_schema::learning::names::Mark;
use halvo_schema::learning::record::{Day, LearnRoutine};
use halvo_schema::TypedRecord;
use serde_json::{json, Value};

use super::host::{active_world, agent, run, wf, world_with_no_routine_row, World, DAY};
use crate::{CoordinatorAnswer, LearnHost};

fn routines(w: &World) -> Vec<LearnRoutine> {
    let mut rows: Vec<LearnRoutine> = w
        .host
        .stored("learn_routine")
        .iter()
        .filter_map(|row| LearnRoutine::parse(row.as_bytes()).ok())
        .filter(|row| row.workflow.as_str() == wf(1))
        .collect();
    rows.sort_by_key(|row| (row.rev, row.day));
    rows
}

fn day(d: u32) -> Day {
    Day::new(d).expect("a day")
}

fn planted(w: &mut World, rev: u32, on: u32, mark: &str) {
    let stale = mark == "stale";
    let last = if stale { on - 70 } else { on };
    let row = json!({
        "v": 1, "workflow": wf(1), "agent": agent(1), "rev": rev, "day": on,
        "category": "utility_bill_payment",
        "site": {"key": "northwind-power.test", "source": "address_field"},
        "app": "com.example.browser", "period": 30, "seen": 3, "first": last - 60, "last": last,
        "next": last + 30, "missed": if stale { 2 } else { 0 }, "mark": mark,
        "since": if stale { Value::from(on) } else { Value::Null },
    });
    w.host
        .commit(vec![TypedRecord {
            kind: "learn_routine",
            json: row.to_string(),
        }])
        .expect("the row is stored");
}

fn first_of_workflow_1(row: &LearnRoutine, on: u32) {
    assert_eq!(row.rev.get(), 1);
    assert_eq!(row.day, day(on));
    assert_eq!(row.agent.as_str(), agent(1));
    assert_eq!(row.category.as_str(), "utility_bill_payment");
    assert_eq!(
        serde_json::to_value(&row.site).expect("a site value"),
        json!({"key": "northwind-power.test", "source": "address_field"})
    );
    assert_eq!(row.app.as_str(), "com.example.browser");
    assert_eq!(
        (row.period, row.seen, row.first, row.last, row.next),
        (None, 0, None, None, None)
    );
    assert_eq!((row.missed, row.mark, row.since), (0, Mark::None, None));
}

fn run_revs(w: &World, r: u32) -> usize {
    w.host
        .runs()
        .iter()
        .filter(|row| row.run.as_str() == run(r))
        .count()
}

#[test]
fn a_start_after_a_stop_behind_the_state_call_of_create_agent_stores_the_routine_row_with_the_runs_row(
) {
    let mut w = world_with_no_routine_row();
    assert!(routines(&w).is_empty());
    let commits = w.host.commits.get();
    w.start(1, "compile");
    assert_eq!(w.host.commits.get(), commits + 1);
    assert_eq!(run_revs(&w, 1), 1);
    let rows = routines(&w);
    assert_eq!(rows.len(), 1);
    first_of_workflow_1(&rows[0], DAY);
}

#[test]
fn a_start_after_a_refused_commit_of_create_agent_stores_the_routine_row() {
    let mut w = super::host::bare_world();
    w.host.put_workflow(super::host::workflow(
        &wf(1),
        "utility_bill_payment",
        "approved",
        None,
    ));
    w.host.refuse = true;
    let reply = w.coordinator(json!({"op": "create_agent", "workflow": wf(1), "agent": agent(1)}));
    assert!(matches!(
        reply,
        Ok(CoordinatorAnswer::Agent(CoordinatorReply::Rev(_)))
    ));
    w.host.refuse = false;
    assert!(routines(&w).is_empty());
    w.start(1, "compile");
    let rows = routines(&w);
    assert_eq!(rows.len(), 1);
    first_of_workflow_1(&rows[0], DAY);
}

#[test]
fn a_pair_whose_rows_left_by_their_bound_gets_rev_1_at_its_next_start() {
    let mut w = active_world();
    w.host.day = DAY + 93;
    assert!(w.host.learn_rows("learn_routine").is_empty());
    w.start(2, "replay");
    let rows = routines(&w);
    assert_eq!(rows.len(), 2);
    first_of_workflow_1(&rows[0], DAY);
    first_of_workflow_1(&rows[1], DAY + 93);
}

#[test]
fn under_a_clock_set_back_so_that_the_old_rows_are_read_again_the_newest_row_by_the_one_rule_is_written_again(
) {
    let mut w = active_world();
    planted(&mut w, 2, DAY + 1, "none");
    w.host.day = DAY + 94;
    w.start(2, "replay");
    let rows = routines(&w);
    assert_eq!(rows.len(), 3);
    first_of_workflow_1(&rows[1], DAY + 94);
    w.host.day = DAY + 92;
    assert_eq!(w.host.learn_rows("learn_routine").len(), 3);
    w.start(3, "replay");
    let rows = routines(&w);
    assert_eq!(rows.len(), 4);
    let written = &rows[3];
    assert_eq!((written.rev.get(), written.day), (3, day(DAY + 92)));
    let before = &rows[2];
    assert_eq!((before.rev.get(), before.day), (2, day(DAY + 1)));
    assert_eq!(
        LearnRoutine {
            rev: before.rev,
            day: before.day,
            ..written.clone()
        },
        *before
    );
}

#[test]
fn the_newest_row_is_written_again_at_30_and_92_days_with_the_mark_none_and_not_at_29_days_or_with_stale(
) {
    for (age, written) in [(29, false), (30, true), (92, true)] {
        let mut w = active_world();
        w.host.day = DAY + age;
        let commits = w.host.commits.get();
        w.start(2, "replay");
        assert_eq!(
            w.host.commits.get(),
            commits + 1,
            "one commit at {age} days"
        );
        let rows = routines(&w);
        assert_eq!(
            rows.len(),
            if written { 2 } else { 1 },
            "rows at {age} days"
        );
        if written {
            assert_eq!((rows[1].rev.get(), rows[1].day), (2, day(DAY + age)));
            assert_eq!(
                LearnRoutine {
                    rev: rows[0].rev,
                    day: rows[0].day,
                    ..rows[1].clone()
                },
                rows[0]
            );
        }
    }
    let mut w = active_world();
    planted(&mut w, 2, DAY, "stale");
    assert_eq!(routines(&w).len(), 2);
    w.host.day = DAY + 30;
    w.start(2, "replay");
    assert_eq!(routines(&w).len(), 2);
    assert_eq!(run_revs(&w, 2), 1);
}

#[test]
fn a_routine_read_that_fails_leaves_the_agent_stores_reply_and_the_runs_row_and_writes_no_routine_row(
) {
    let mut w = world_with_no_routine_row();
    let refused = format!(
        r#"{{"v":1,"workflow":"{}","agent":"{}","rev":1,"day":{DAY},"category":"utility_bill_payment","site":null,"app":"com.example.browser","period":null,"seen":0,"first":null,"last":null,"next":null,"missed":0,"mark":"gone","since":null}}"#,
        wf(1),
        agent(1)
    );
    w.host.put_raw("learn_routine", &refused);
    let reply = w.coordinator(json!({
        "op": "start_run", "workflow": wf(1), "agent": agent(1), "run": run(1), "mode": "compile",
    }));
    assert!(matches!(
        reply,
        Ok(CoordinatorAnswer::Agent(CoordinatorReply::Accepted(_)))
    ));
    assert_eq!(run_revs(&w, 1), 1);
    assert_eq!(w.host.stored("learn_routine"), [refused]);
}

#[test]
fn a_start_run_that_arrives_again_where_the_runs_row_stands_stores_the_missing_routine_row_alone() {
    let mut w = world_with_no_routine_row();
    let row = json!({
        "v": 1, "workflow": wf(1), "agent": agent(1), "run": run(1), "rev": 1, "day": DAY,
        "mode": "compile", "state": "started", "cause": null, "base": null, "assisted": 0,
        "kept": 0, "failed_step": null,
    });
    w.host
        .commit(vec![TypedRecord {
            kind: "learn_run",
            json: row.to_string(),
        }])
        .expect("the row is stored");
    let (count, commits) = (w.host.count(), w.host.commits.get());
    w.start(1, "compile");
    assert_eq!(
        (w.host.count(), w.host.commits.get()),
        (count + 1, commits + 1)
    );
    assert_eq!(run_revs(&w, 1), 1);
    let rows = routines(&w);
    assert_eq!(rows.len(), 1);
    first_of_workflow_1(&rows[0], DAY);
    assert!(w.handlers.starts.is_empty());
}

#[test]
fn a_start_run_that_arrives_again_after_both_rows_stand_changes_nothing() {
    let mut w = world_with_no_routine_row();
    w.start(1, "compile");
    assert_eq!(routines(&w).len(), 1);
    assert_eq!(run_revs(&w, 1), 1);
    let (count, commits) = (w.host.count(), w.host.commits.get());
    let starts = w.handlers.starts.clone();
    w.start(1, "compile");
    assert_eq!((w.host.count(), w.host.commits.get()), (count, commits));
    assert_eq!(w.handlers.starts, starts);
}

fn rev_2_on(w: &mut World, on: u32, stale: bool) {
    let (last, period) = if stale { (on - 70, 31) } else { (on, 30) };
    let row = json!({
        "v": 1, "workflow": wf(1), "agent": agent(1), "rev": 2, "day": on,
        "category": "utility_bill_payment",
        "site": {"key": "northwind-power.test", "source": "address_field"},
        "app": "com.example.browser", "period": period, "seen": 3, "first": last - 2 * period,
        "last": last, "next": last + period, "missed": if stale { 2 } else { 0 },
        "mark": if stale { "stale" } else { "none" },
        "since": if stale { Value::from(on) } else { Value::Null },
    });
    w.host
        .commit(vec![TypedRecord {
            kind: "learn_routine",
            json: row.to_string(),
        }])
        .expect("the row is stored");
}

fn the_later_of_two_rows_of_one_rev_is_written_again(later: u32, later_visited_first: bool) {
    let mut w = active_world();
    rev_2_on(&mut w, later, false);
    rev_2_on(&mut w, later - 1, true);
    w.host.day = later + 30;
    let visited: Vec<u32> = w
        .host
        .learn_rows("learn_routine")
        .iter()
        .map(|row| LearnRoutine::parse(row.as_bytes()).expect("a routine row"))
        .filter(|row| row.rev.get() == 2)
        .map(|row| row.day.get())
        .collect();
    let order = if later_visited_first {
        [later, later - 1]
    } else {
        [later - 1, later]
    };
    assert_eq!(visited, order);
    w.start(2, "replay");
    let rows = routines(&w);
    let written = rows.last().expect("a routine row");
    assert_eq!((written.rev.get(), written.day), (3, day(later + 30)));
    assert_eq!(
        (written.mark, written.period.map(|p| p.get()), written.last),
        (Mark::None, Some(30), Some(day(later)))
    );
}

#[test]
fn the_start_writes_again_the_later_of_two_rows_of_one_rev_when_the_read_visits_it_last() {
    the_later_of_two_rows_of_one_rev_is_written_again(DAY + 1, false);
}

#[test]
fn the_start_writes_again_the_later_of_two_rows_of_one_rev_when_the_read_visits_it_first() {
    the_later_of_two_rows_of_one_rev_is_written_again(100_000, true);
}
