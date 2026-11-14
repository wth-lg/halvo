use halvo_learn::{routine_step, Mark as RuleMark, RoutineState};
use halvo_schema::learning::names::Mark;
use halvo_schema::learning::record::LearnRoutine;
use halvo_schema::TypedRecord;

use super::fixtures::*;

fn only_row(records: &[TypedRecord]) -> LearnRoutine {
    assert_eq!(records.len(), 1, "one record");
    assert_eq!(records[0].kind, "learn_routine");
    LearnRoutine::parse(records[0].json.as_bytes()).expect("a learn_routine row")
}

fn due(prev: &LearnRoutine, next: &RoutineState, today: u32) -> bool {
    let before = figures(prev);
    next.mark != before.mark
        || next.period != before.period
        || next.last != before.last
        || today.saturating_sub(prev.day.get()) >= 30
}

fn settled(rev: u32, day: u32, period: u32, first: u32, last: u32) -> LearnRoutine {
    let state = RoutineState {
        period: Some(period),
        seen: 3,
        first: Some(first),
        last: Some(last),
        next: Some(last + period),
        missed: 0,
        mark: RuleMark::None,
        since: None,
    };
    with_figures(&fresh(1, DAY - 200), rev, day, &state)
}

fn home(n: u32, day: u32) -> halvo_schema::Summary {
    summary(n, BILL, domain(HOME, "address_field"), BROWSER, day)
}

#[test]
fn on_scripted_days_the_step_returns_the_row_that_routine_step_gives_and_no_other() {
    let start = DAY;
    let user: Vec<u32> = (1..=8).map(|k| start + 14 * k).collect();
    let agent: Vec<u32> = (9..=14).map(|k| start + 14 * k).collect();
    let mut store = Store::default();
    store.put_routine(&fresh(1, start));
    let mut days = Vec::new();
    let mut stale_on = None;
    let mut written = 0;
    for today in start..=start + 300 {
        if user.contains(&today) {
            store.summaries.push(home(today, today));
            days.push(today);
        }
        if agent.contains(&today) {
            store.put_run(&run(1, today, today, "replay", "started"));
            store.put_run(&run(1, today, today, "replay", "verified"));
            days.push(today);
        }
        if today % 10 == 0 {
            let n = today + 1_000_000;
            store.summaries.push(summary(
                n,
                BILL,
                domain(OTHER, "address_field"),
                BROWSER,
                today,
            ));
            store.summaries.push(summary(
                n + 1,
                ORDER,
                domain(HOME, "address_field"),
                BROWSER,
                today,
            ));
            store.put_run(&run(1, n, today, "replay", "unverified"));
            store.put_run(&run(1, n + 1, today, "compile", "verified"));
            store.put_run(&run(2, n + 2, today, "replay", "verified"));
        }
        let prev = store.newest(1);
        let expected = routine_step(Some(&figures(&prev)), &days, today);
        let records = store.step(today);
        if due(&prev, &expected, today) {
            let row = only_row(&records);
            assert_eq!(
                row,
                with_figures(&prev, prev.rev.get() + 1, today, &expected),
                "day {today}"
            );
            written += 1;
            if expected.mark == RuleMark::Stale && stale_on.is_none() {
                stale_on = Some(today);
            }
        } else {
            assert!(records.is_empty(), "day {today}");
        }
        store.commit(&records);
    }
    assert_eq!(stale_on, Some(start + 196 + 60));
    for row in store.routines() {
        if row.day.get() < start + 256 {
            assert_eq!(row.mark, Mark::None, "rev {}", row.rev);
        }
    }
    let last = store.newest(1);
    assert_eq!(last.period.map(|p| p.get()), Some(14));
    assert_eq!(last.mark, Mark::Stale);
    assert_eq!(last.since.map(|d| d.get()), Some(start + 256));
    assert_eq!(store.routines().len(), written + 1);
}

#[test]
fn an_unchanged_row_is_written_again_on_its_thirtieth_day_and_not_before() {
    let mut store = Store::default();
    let first = fresh(1, DAY);
    store.put_routine(&first);
    for today in DAY..DAY + 30 {
        assert!(store.step(today).is_empty(), "day {today}");
    }
    let records = store.step(DAY + 30);
    assert_eq!(
        only_row(&records),
        with_figures(&first, 2, DAY + 30, &figures(&first))
    );
    store.commit(&records);
    for today in DAY + 30..DAY + 60 {
        assert!(store.step(today).is_empty(), "day {today}");
    }
    assert_eq!(only_row(&store.step(DAY + 60)).rev.get(), 3);
}

#[test]
fn a_change_of_the_mark_alone_writes_a_row() {
    let today = DAY + 60;
    let prev = settled(5, today - 1, 7, DAY - 14, DAY);
    let mut store = Store::default();
    store.put_routine(&prev);
    let row = only_row(&store.step(today));
    let expected = routine_step(Some(&figures(&prev)), &[], today);
    assert_eq!(row, with_figures(&prev, 6, today, &expected));
    assert_eq!(row.mark, Mark::Stale);
    assert_eq!(row.since.map(|d| d.get()), Some(today));
    assert_eq!((row.period, row.last), (prev.period, prev.last));
}

#[test]
fn a_change_of_the_period_alone_writes_a_row() {
    let today = DAY + 1;
    let prev = settled(5, today - 1, 10, DAY - 14, DAY);
    let mut store = Store::default();
    store.put_routine(&prev);
    store.summaries = vec![home(1, DAY - 14), home(2, DAY - 7), home(3, DAY)];
    let row = only_row(&store.step(today));
    let expected = routine_step(Some(&figures(&prev)), &[DAY - 14, DAY - 7, DAY], today);
    assert_eq!(row, with_figures(&prev, 6, today, &expected));
    assert_eq!(row.period.map(|p| p.get()), Some(7));
    assert_eq!((row.mark, row.last), (prev.mark, prev.last));
}

#[test]
fn a_change_of_the_last_day_alone_writes_a_row() {
    let today = DAY + 7;
    let prev = settled(5, today - 1, 7, DAY - 14, DAY);
    let mut store = Store::default();
    store.put_routine(&prev);
    store.summaries = vec![
        home(1, DAY - 14),
        home(2, DAY - 7),
        home(3, DAY),
        home(4, DAY + 7),
    ];
    let row = only_row(&store.step(today));
    let expected = routine_step(
        Some(&figures(&prev)),
        &[DAY - 14, DAY - 7, DAY, DAY + 7],
        today,
    );
    assert_eq!(row, with_figures(&prev, 6, today, &expected));
    assert_eq!(row.last.map(|d| d.get()), Some(DAY + 7));
    assert_eq!((row.mark, row.period), (prev.mark, prev.period));
}

#[test]
fn a_change_of_seen_first_or_missed_alone_writes_no_row_until_the_thirtieth_day() {
    let today = DAY + 13;
    let days = [DAY - 21, DAY - 14, DAY - 7, DAY];
    let summaries: Vec<_> = (0u32..).zip(days).map(|(n, d)| home(n, d)).collect();
    for age in 1..30 {
        let prev = settled(5, today - age, 7, DAY - 14, DAY);
        let expected = routine_step(Some(&figures(&prev)), &days, today);
        assert_eq!((expected.seen, expected.missed), (4, 1));
        assert_eq!(expected.first, Some(DAY - 21));
        let mut store = Store::default();
        store.put_routine(&prev);
        store.summaries = summaries.clone();
        assert!(store.step(today).is_empty(), "age {age}");
    }
    let prev = settled(5, today - 30, 7, DAY - 14, DAY);
    let mut store = Store::default();
    store.put_routine(&prev);
    store.summaries = summaries;
    let expected = routine_step(Some(&figures(&prev)), &days, today);
    assert_eq!(
        only_row(&store.step(today)),
        with_figures(&prev, 6, today, &expected)
    );
}

#[test]
fn the_step_returns_no_learn_edit_and_no_learn_run_and_no_row_of_a_workflow_without_one() {
    let mut store = Store::default();
    store.put_routine(&fresh(1, DAY));
    store.rows.push(("learn_edit", EDIT_ROW.to_owned()));
    let mut records = Vec::new();
    for today in DAY..DAY + 120 {
        if today % 7 == 0 {
            store.put_run(&run(1, today, today, "replay", "started"));
            store.put_run(&run(1, today, today, "replay", "verified"));
        }
        if today % 3 == 0 {
            store.put_run(&run(2, today + 1_000_000, today, "replay", "verified"));
            store.summaries.push(summary(
                today,
                BILL,
                domain(OTHER, "address_field"),
                BROWSER,
                today,
            ));
        }
        let out = store.step(today);
        store.commit(&out);
        records.extend(out);
    }
    assert!(records.len() > 3);
    for record in &records {
        assert_eq!(record.kind, "learn_routine");
        let row = LearnRoutine::parse(record.json.as_bytes()).expect("a learn_routine row");
        assert_eq!(row.workflow.as_str(), wf(1));
    }
}

#[test]
fn the_newest_row_is_the_one_with_the_highest_rev_whatever_the_order_of_the_rows() {
    let today = DAY + 7;
    let older = fresh(1, DAY - 50);
    let older = with_figures(&older, 9, DAY - 50, &figures(&older));
    let newer = settled(10, today - 1, 7, DAY - 14, DAY);
    let mut store = Store::default();
    store.put_routine(&older);
    store.put_routine(&newer);
    let mut order = Vec::new();
    halvo_schema::RowRead::for_each(&store, "learn_routine", &mut |r| order.push(r.to_owned()));
    assert!(order[0].contains("\"rev\":10,"));
    store.summaries = vec![home(1, DAY + 7)];
    let row = only_row(&store.step(today));
    let expected = routine_step(Some(&figures(&newer)), &[DAY + 7], today);
    assert_eq!(row, with_figures(&newer, 11, today, &expected));
    assert_eq!(row.period.map(|p| p.get()), Some(7));
}

#[test]
fn of_two_rows_of_one_rev_the_step_goes_on_from_the_one_of_the_later_day() {
    let today = DAY + 30;
    let earlier = settled(5, DAY - 2, 7, DAY - 14, DAY);
    let later = settled(5, DAY, 9, DAY - 18, DAY);
    let mut store = Store::default();
    store.put_routine(&later);
    store.put_routine(&earlier);
    let mut order = Vec::new();
    halvo_schema::RowRead::for_each(&store, "learn_routine", &mut |r| order.push(r.to_owned()));
    assert!(order[0].contains(&format!("\"day\":{},", DAY - 2)));
    let expected = routine_step(Some(&figures(&later)), &[], today);
    assert_eq!(
        only_row(&store.step(today)),
        with_figures(&later, 6, today, &expected)
    );
}

#[test]
fn of_two_rows_of_one_rev_and_one_day_the_step_goes_on_from_the_one_visited_last() {
    let today = DAY + 30;
    let seven = settled(5, DAY, 7, DAY - 14, DAY);
    let nine = settled(5, DAY, 9, DAY - 18, DAY);
    let mut store = Store::default();
    store.put_routine(&nine);
    store.put_routine(&seven);
    let mut order = Vec::new();
    halvo_schema::RowRead::for_each(&store, "learn_routine", &mut |r| order.push(r.to_owned()));
    assert!(order[0].contains("\"period\":7,"));
    assert!(order[1].contains("\"period\":9,"));
    let expected = routine_step(Some(&figures(&nine)), &[], today);
    assert_eq!(
        only_row(&store.step(today)),
        with_figures(&nine, 6, today, &expected)
    );
}

#[test]
fn each_routine_reads_the_days_of_its_own_key() {
    let today = DAY + 10;
    let one = fresh(1, DAY);
    let two = fresh_at(2, DAY, domain(OTHER, "address_field"), BROWSER);
    let mut store = Store::default();
    store.put_routine(&one);
    store.put_routine(&two);
    store.summaries = vec![
        home(1, DAY + 1),
        home(2, DAY + 4),
        home(3, DAY + 7),
        summary(4, BILL, domain(OTHER, "address_field"), BROWSER, DAY + 9),
    ];
    let records = store.step(today);
    assert_eq!(records.len(), 2);
    let rows: Vec<LearnRoutine> = records
        .iter()
        .map(|r| LearnRoutine::parse(r.json.as_bytes()).expect("a learn_routine row"))
        .collect();
    let of = |w: u32| {
        rows.iter()
            .find(|r| r.workflow.as_str() == wf(w))
            .expect("a row")
    };
    let expected_one = routine_step(Some(&figures(&one)), &[DAY + 1, DAY + 4, DAY + 7], today);
    let expected_two = routine_step(Some(&figures(&two)), &[DAY + 9], today);
    assert_eq!(of(1), &with_figures(&one, 2, today, &expected_one));
    assert_eq!(of(2), &with_figures(&two, 2, today, &expected_two));
}

#[test]
fn an_unreadable_learn_run_or_learn_routine_row_makes_the_step_return_nothing() {
    let mut store = Store::default();
    store.put_routine(&fresh(1, DAY));
    assert_eq!(store.step(DAY + 30).len(), 1);
    for bad in [("learn_run", "{}"), ("learn_routine", "{\"v\":1}")] {
        let mut broken = Store::default();
        broken.put_routine(&fresh(1, DAY));
        broken.rows.push((bad.0, bad.1.to_owned()));
        assert!(broken.step(DAY + 30).is_empty(), "{}", bad.0);
    }
}

#[test]
fn an_unreadable_learn_edit_row_does_not_stop_the_step() {
    let first = fresh(1, DAY);
    let mut store = Store::default();
    store.put_routine(&first);
    store.rows.push(("learn_edit", "{}".to_owned()));
    assert_eq!(
        only_row(&store.step(DAY + 30)),
        with_figures(&first, 2, DAY + 30, &figures(&first))
    );
}

#[test]
fn a_summary_of_the_other_source_of_one_domain_moves_the_other_workflow_alone() {
    let gone = RoutineState {
        period: Some(30),
        seen: 3,
        first: Some(DAY - 180),
        last: Some(DAY - 120),
        next: Some(DAY - 90),
        missed: 0,
        mark: RuleMark::None,
        since: None,
    };
    let address = with_figures(&fresh(1, DAY - 200), 2, DAY - 1, &gone);
    let sign_in = fresh_at(2, DAY - 1, domain(HOME, "sign_in"), BROWSER);
    let mut store = Store::default();
    store.put_routine(&address);
    store.put_routine(&sign_in);
    store
        .summaries
        .push(summary(1, BILL, domain(HOME, "sign_in"), BROWSER, DAY - 3));
    let records = store.step(DAY);
    let rows: Vec<LearnRoutine> = records
        .iter()
        .map(|r| LearnRoutine::parse(r.json.as_bytes()).expect("a learn_routine row"))
        .collect();
    let of = |w: u32| {
        rows.iter()
            .find(|r| r.workflow.as_str() == wf(w))
            .expect("a row")
    };
    let expected_address = routine_step(Some(&figures(&address)), &[], DAY);
    let expected_sign_in = routine_step(Some(&figures(&sign_in)), &[DAY - 3], DAY);
    assert_eq!(of(1), &with_figures(&address, 3, DAY, &expected_address));
    assert_eq!(of(1).mark, Mark::Stale);
    assert_eq!(of(1).last.map(|d| d.get()), Some(DAY - 120));
    assert_eq!(of(2), &with_figures(&sign_in, 2, DAY, &expected_sign_in));
    assert_eq!(of(2).last.map(|d| d.get()), Some(DAY - 3));
}

#[test]
fn two_workflows_of_one_agent_keep_their_own_rows_and_marks() {
    let today = DAY + 60;
    let one = settled(5, today - 1, 7, DAY - 14, DAY);
    let live = RoutineState {
        period: Some(7),
        seen: 3,
        first: Some(DAY + 39),
        last: Some(DAY + 53),
        next: Some(DAY + 60),
        missed: 0,
        mark: RuleMark::None,
        since: None,
    };
    let two = LearnRoutine {
        agent: one.agent.clone(),
        ..with_figures(
            &fresh_at(2, DAY, domain(OTHER, "address_field"), BROWSER),
            9,
            today - 1,
            &live,
        )
    };
    let mut store = Store::default();
    store.put_routine(&one);
    store.put_routine(&two);
    let other = |n: u32, day: u32| summary(n, BILL, domain(OTHER, "address_field"), BROWSER, day);
    store.summaries = vec![other(1, DAY + 46), other(2, DAY + 53), other(3, DAY + 60)];
    let records = store.step(today);
    let rows: Vec<LearnRoutine> = records
        .iter()
        .map(|r| LearnRoutine::parse(r.json.as_bytes()).expect("a learn_routine row"))
        .collect();
    let expected_one = routine_step(Some(&figures(&one)), &[], today);
    let expected_two = routine_step(Some(&figures(&two)), &[DAY + 46, DAY + 53, DAY + 60], today);
    assert_eq!(
        rows,
        vec![
            with_figures(&one, 6, today, &expected_one),
            with_figures(&two, 10, today, &expected_two),
        ]
    );
    assert_eq!((rows[0].mark, rows[1].mark), (Mark::Stale, Mark::None));
}

#[test]
fn a_row_at_the_highest_rev_gives_no_next_row() {
    let first = fresh(1, DAY);
    for (rev, rows) in [(u32::MAX - 1, 1), (u32::MAX, 0)] {
        let mut store = Store::default();
        store.put_routine(&with_figures(&first, rev, DAY, &figures(&first)));
        let records = store.step(DAY + 30);
        assert_eq!(records.len(), rows, "rev {rev}");
        if rows == 1 {
            assert_eq!(only_row(&records).rev.get(), u32::MAX);
        }
    }
}

#[test]
fn a_clock_more_than_two_days_behind_the_newest_row_gives_the_row_of_routine_step() {
    let back = DAY - 10;
    let prev = settled(5, DAY, 7, DAY - 14, DAY);
    let mut store = Store::default();
    store.put_routine(&prev);
    store.summaries = vec![
        home(1, DAY - 30),
        home(2, DAY - 23),
        home(3, DAY - 16),
        home(4, DAY),
    ];
    let row = only_row(&store.step(back));
    let expected = routine_step(
        Some(&figures(&prev)),
        &[DAY - 30, DAY - 23, DAY - 16, DAY],
        back,
    );
    assert_eq!(row, with_figures(&prev, 6, back, &expected));
    assert_eq!((row.missed, row.mark), (0, Mark::None));

    let mut quiet = Store::default();
    quiet.put_routine(&prev);
    assert!(quiet.step(DAY - 5).is_empty());

    let stale = RoutineState {
        missed: 4,
        mark: RuleMark::Stale,
        since: Some(DAY + 60),
        ..figures(&prev)
    };
    let stale = with_figures(&prev, 6, DAY + 60, &stale);
    let mut store = Store::default();
    store.put_routine(&stale);
    let row = only_row(&store.step(DAY - 5));
    let expected = routine_step(Some(&figures(&stale)), &[], DAY - 5);
    assert_eq!(row, with_figures(&stale, 7, DAY - 5, &expected));
    assert_eq!((row.missed, row.mark), (0, Mark::None));
}

#[test]
fn a_clock_at_day_zero_gives_nothing() {
    let mut store = Store::default();
    store.put_routine(&fresh(1, DAY));
    assert!(store.step(0).is_empty());

    let prev = settled(5, DAY, 7, DAY - 14, DAY);
    let stale = RoutineState {
        missed: 4,
        mark: RuleMark::Stale,
        since: Some(DAY + 60),
        ..figures(&prev)
    };
    let stale = with_figures(&prev, 6, DAY + 60, &stale);
    let mut store = Store::default();
    store.put_routine(&stale);
    assert_eq!(store.step(1).len(), 1);
    assert!(store.step(0).is_empty());
}

#[test]
fn each_agent_of_one_workflow_keeps_its_own_row() {
    let one = fresh(1, DAY);
    let first_two = LearnRoutine {
        agent: halvo_schema::AgentId::parse(&agent(2)).expect("an agent id"),
        ..fresh(1, DAY + 5)
    };
    let two = with_figures(&first_two, 4, DAY + 5, &figures(&first_two));
    let mut store = Store::default();
    store.put_routine(&one);
    store.put_routine(&two);
    let today = DAY + 35;
    let records = store.step(today);
    let rows: Vec<LearnRoutine> = records
        .iter()
        .map(|r| LearnRoutine::parse(r.json.as_bytes()).expect("a learn_routine row"))
        .collect();
    assert_eq!(
        rows,
        vec![
            with_figures(&one, 2, today, &figures(&one)),
            with_figures(&two, 5, today, &figures(&two)),
        ]
    );
}
