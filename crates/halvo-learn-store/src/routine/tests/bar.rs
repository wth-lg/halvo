use halvo_learn::{routine_step, Mark as RuleMark, RoutineState};
use halvo_schema::learning::names::Mark;
use halvo_schema::learning::record::LearnRoutine;
use halvo_schema::TypedRecord;

use super::fixtures::*;

fn rows_of(records: &[TypedRecord]) -> Vec<LearnRoutine> {
    records
        .iter()
        .map(|r| {
            assert_eq!(r.kind, "learn_routine");
            LearnRoutine::parse(r.json.as_bytes()).expect("a learn_routine row")
        })
        .collect()
}

fn only_row(records: &[TypedRecord]) -> LearnRoutine {
    let rows = rows_of(records);
    assert_eq!(rows.len(), 1, "one record");
    rows.into_iter().next().expect("one row")
}

fn weekly(rev: u32, day: u32, first: u32, last: u32) -> LearnRoutine {
    let state = RoutineState {
        period: Some(7),
        seen: 3,
        first: Some(first),
        last: Some(last),
        next: Some(last + 7),
        missed: 0,
        mark: RuleMark::None,
        since: None,
    };
    with_figures(&fresh(1, DAY - 200), rev, day, &state)
}

fn home(store: &mut Store, n: u32, days: &[u32]) {
    for (k, &day) in (n..).zip(days) {
        store.summaries.push(summary(
            k,
            BILL,
            domain(HOME, "address_field"),
            BROWSER,
            day,
        ));
    }
}

fn last(row: &LearnRoutine) -> Option<u32> {
    row.last.map(|d| d.get())
}

fn since(row: &LearnRoutine) -> Option<u32> {
    row.since.map(|d| d.get())
}

#[test]
fn a_clock_one_or_two_days_behind_the_newest_row_rests_its_pair_and_steps_the_other_pairs() {
    let ahead = weekly(5, DAY, DAY - 14, DAY);
    let other = fresh_at(2, DAY - 40, domain(OTHER, "address_field"), BROWSER);
    let mut store = Store::default();
    store.put_routine(&ahead);
    store.put_routine(&other);
    home(&mut store, 1, &[DAY - 14, DAY - 7, DAY]);
    for back in [DAY - 1, DAY - 2] {
        let moved = routine_step(Some(&figures(&ahead)), &[DAY - 14, DAY - 7], back);
        assert_eq!(
            moved.last,
            Some(DAY - 7),
            "routine_step would move last back"
        );
        let expected = routine_step(Some(&figures(&other)), &[], back);
        assert_eq!(
            only_row(&store.step(back)),
            with_figures(&other, 2, back, &expected),
            "clock {back}"
        );
    }
}

#[test]
fn the_first_commit_on_the_day_of_the_newest_row_takes_its_pair_up_again() {
    let ahead = weekly(5, DAY, DAY - 21, DAY - 7);
    let mut store = Store::default();
    store.put_routine(&ahead);
    home(&mut store, 1, &[DAY - 21, DAY - 14, DAY - 7, DAY - 1]);
    assert!(store.step(DAY - 2).is_empty());
    assert!(store.step(DAY - 1).is_empty());
    let expected = routine_step(
        Some(&figures(&ahead)),
        &[DAY - 21, DAY - 14, DAY - 7, DAY - 1],
        DAY,
    );
    let row = only_row(&store.step(DAY));
    assert_eq!(row, with_figures(&ahead, 6, DAY, &expected));
    assert_eq!(last(&row), Some(DAY - 1));
}

#[test]
fn review_c1_1_a_clock_set_back_one_day_moves_last_neither_back_nor_forward() {
    let mut store = Store::default();
    store.put_routine(&fresh(1, DAY - 200));
    let days: Vec<u32> = (0..12).map(|i| DAY - 77 + 7 * i).collect();
    home(&mut store, 100, &days);
    let records = store.step(DAY);
    assert_eq!(records.len(), 1);
    store.commit(&records);
    assert_eq!(last(&store.newest(1)), Some(DAY));
    assert!(
        store.step(DAY - 1).is_empty(),
        "no row on the day the clock is back"
    );
    assert!(
        store.step(DAY).is_empty(),
        "no row when the clock is forward again"
    );
    assert_eq!(last(&store.newest(1)), Some(DAY));
    let on_day = store
        .routines()
        .into_iter()
        .filter(|r| r.day.get() == DAY)
        .count();
    assert_eq!(on_day, 1, "one row of the pair dated DAY");
}

#[test]
fn review_c1_2_a_clock_set_back_one_day_neither_clears_a_stale_mark_nor_moves_its_since() {
    let mut store = Store::default();
    store.put_routine(&fresh(1, DAY - 200));
    home(&mut store, 200, &[DAY - 81, DAY - 74, DAY - 67, DAY - 60]);
    let records = store.step(DAY);
    store.commit(&records);
    let at_day = store.newest(1);
    assert_eq!((at_day.mark, since(&at_day)), (Mark::Stale, Some(DAY)));
    assert!(
        store.step(DAY - 1).is_empty(),
        "the clock alone clears nothing"
    );
    assert!(store.step(DAY + 1).is_empty(), "nothing to raise again");
    let after = store.newest(1);
    assert_eq!(after, at_day);
    assert_eq!((after.mark, since(&after)), (Mark::Stale, Some(DAY)));
}

#[test]
fn review_c1_3_a_clock_set_back_one_day_does_not_mark_stale_a_routine_that_the_person_resumed() {
    let mut store = Store::default();
    store.put_routine(&fresh(1, DAY - 200));
    home(&mut store, 300, &[DAY - 90, DAY - 83, DAY - 76, DAY - 69]);
    let records = store.step(DAY - 5);
    store.commit(&records);
    assert_eq!(store.newest(1).mark, Mark::Stale);
    home(&mut store, 400, &[DAY]);
    let records = store.step(DAY);
    assert_eq!(records.len(), 1);
    store.commit(&records);
    let at_day = store.newest(1);
    assert_eq!((at_day.mark, last(&at_day)), (Mark::None, Some(DAY)));
    assert!(
        store.step(DAY - 1).is_empty(),
        "the clock alone marks nothing"
    );
    assert!(store.step(DAY).is_empty());
    let after = store.newest(1);
    assert_eq!(after, at_day);
    assert_eq!((after.mark, last(&after)), (Mark::None, Some(DAY)));
}

#[test]
fn a_clock_three_days_behind_a_row_that_the_read_still_hands_out_gives_todays_row() {
    let ahead = weekly(5, DAY, DAY - 14, DAY);
    let mut store = Store::default();
    store.put_routine(&ahead);
    home(&mut store, 1, &[DAY - 14, DAY - 7, DAY]);
    let back = DAY - 3;
    let expected = routine_step(Some(&figures(&ahead)), &[DAY - 14, DAY - 7], back);
    let row = only_row(&store.step(back));
    assert_eq!(row, with_figures(&ahead, 6, back, &expected));
    assert_eq!((row.day.get(), last(&row)), (back, Some(DAY - 7)));
}

#[test]
fn a_stale_mark_written_under_a_clock_ahead_stands_while_its_pair_rests_and_goes_at_the_first_commit_on_its_day(
) {
    let mut store = Store::default();
    store.put_routine(&fresh(1, DAY - 200));
    home(&mut store, 1, &[DAY - 81, DAY - 74, DAY - 67, DAY - 60]);
    let records = store.step(DAY);
    store.commit(&records);
    let marked = store.newest(1);
    assert_eq!((marked.mark, since(&marked)), (Mark::Stale, Some(DAY)));
    home(&mut store, 5, &[DAY - 2]);
    assert!(store.step(DAY - 2).is_empty(), "the task done at DAY - 2");
    assert!(store.step(DAY - 1).is_empty());
    assert_eq!(
        store.newest(1),
        marked,
        "mark and since stand while it rests"
    );
    let expected = routine_step(
        Some(&figures(&marked)),
        &[DAY - 81, DAY - 74, DAY - 67, DAY - 60, DAY - 2],
        DAY,
    );
    let row = only_row(&store.step(DAY));
    assert_eq!(
        row,
        with_figures(&marked, marked.rev.get() + 1, DAY, &expected)
    );
    assert_eq!(
        (row.mark, last(&row), since(&row)),
        (Mark::None, Some(DAY - 2), Some(DAY))
    );
}

#[test]
fn the_same_store_stepped_twice_on_one_day_returns_nothing_the_second_time_and_one_row_after_a_new_occurrence_day(
) {
    let mut store = Store::default();
    store.put_routine(&fresh(1, DAY - 10));
    home(&mut store, 1, &[DAY - 21, DAY - 14, DAY - 7]);
    let first = store.step(DAY);
    assert_eq!(first.len(), 1);
    store.commit(&first);
    assert!(store.step(DAY).is_empty(), "the second step of the day");
    home(&mut store, 4, &[DAY]);
    let row = only_row(&store.step(DAY));
    assert_eq!(row.rev.get(), 3);
    assert_eq!(last(&row), Some(DAY));
    store.commit(&[TypedRecord {
        kind: "learn_routine",
        json: row.to_json().expect("a writable row"),
    }]);
    assert!(store.step(DAY).is_empty(), "the fourth step of the day");
}
