mod common;

use common::*;
use halvo_agent_store::MemoryStore;
use halvo_schema::executable::names::EffectState;
use halvo_schema::ErrorName;
use serde_json::{json, Value};

fn payment() -> MemoryStore {
    store_with("utility_bill_payment")
}

fn ok(store: &mut MemoryStore, row: Value) {
    assert_eq!(put(store, row.clone()), Ok(stored(1)), "{row}");
}

fn refused(store: &mut MemoryStore, row: Value) {
    let before = effects(store).len();
    assert_eq!(put(store, row.clone()), Err(declared("effect")), "{row}");
    assert_eq!(effects(store).len(), before);
}

#[test]
fn a_second_approval_of_an_approved_effect_is_refused() {
    let mut store = payment();
    ok(&mut store, row(1, 1, 1, "held", "payment", 1, None, None));
    ok(
        &mut store,
        row(1, 1, 2, "approved", "payment", 1, Some(1), None),
    );
    refused(
        &mut store,
        row(1, 1, 3, "approved", "payment", 1, Some(2), None),
    );
    assert_eq!(
        last_of(&store, &effect(1))
            .approval
            .map(|a| a.as_str().to_owned()),
        Some(approval(1))
    );
}

#[test]
fn the_runner_ends_a_cleared_or_approved_effect_and_rejects_a_held_one_alone() {
    let mut store = unattended_sender();
    ok(&mut store, row(1, 1, 1, "cleared", "send", 1, None, None));
    refused(&mut store, row(1, 1, 2, "rejected", "send", 1, None, None));
    ok(
        &mut store,
        row(1, 1, 2, "rejected", "send", 1, None, Some("ended")),
    );
    ok(&mut store, row(1, 2, 1, "held", "delete", 1, None, None));
    ok(
        &mut store,
        row(1, 2, 2, "approved", "delete", 1, Some(1), None),
    );
    refused(
        &mut store,
        row(1, 2, 3, "rejected", "delete", 1, None, None),
    );
    ok(
        &mut store,
        row(1, 2, 3, "rejected", "delete", 1, None, Some("ended")),
    );
}

#[test]
fn a_confirmed_payment_moves_through_its_rows() {
    let mut store = payment();
    ok(&mut store, row(1, 1, 1, "held", "payment", 1, None, None));
    ok(&mut store, row(1, 1, 2, "held", "payment", 2, None, None));
    ok(
        &mut store,
        row(1, 1, 3, "approved", "payment", 2, Some(1), None),
    );
    ok(
        &mut store,
        row(1, 1, 4, "releasing", "payment", 2, Some(1), None),
    );
    ok(
        &mut store,
        row(1, 1, 5, "committed", "payment", 2, Some(1), None),
    );
    assert_eq!(effects(&store).len(), 5);
    assert_eq!(last_of(&store, &effect(1)).state, EffectState::Committed);
}

#[test]
fn a_cleared_edit_moves_to_committed_and_a_held_one_to_rejected() {
    let mut store = store_with("spreadsheet_fill");
    ok(
        &mut store,
        row(1, 1, 1, "cleared", "local_edit", 1, None, None),
    );
    ok(
        &mut store,
        row(1, 1, 2, "releasing", "local_edit", 1, None, None),
    );
    ok(
        &mut store,
        row(1, 1, 3, "committed", "local_edit", 1, None, None),
    );

    let mut store = payment();
    ok(&mut store, row(1, 1, 1, "held", "payment", 1, None, None));
    ok(
        &mut store,
        row(1, 1, 2, "rejected", "payment", 1, None, None),
    );
    ok(&mut store, row(1, 2, 1, "held", "payment", 1, None, None));
    ok(
        &mut store,
        row(1, 2, 2, "rejected", "payment", 1, None, Some("ended")),
    );
    refused(
        &mut store,
        row(1, 1, 3, "releasing", "payment", 1, None, None),
    );
}

#[test]
fn every_move_repeated_while_its_row_is_last_is_answered_as_the_first() {
    let mut paths: Vec<Vec<Value>> = vec![
        vec![
            row(1, 1, 1, "held", "payment", 1, None, None),
            row(1, 1, 2, "held", "payment", 2, None, None),
            row(1, 1, 3, "approved", "payment", 2, Some(1), None),
            row(1, 1, 4, "releasing", "payment", 2, Some(1), None),
            row(1, 1, 5, "committed", "payment", 2, Some(1), None),
        ],
        vec![
            row(1, 1, 1, "held", "payment", 1, None, None),
            row(1, 1, 2, "rejected", "payment", 1, None, None),
        ],
        vec![
            row(1, 1, 1, "held", "payment", 1, None, None),
            row(1, 1, 2, "approved", "payment", 1, Some(1), None),
            row(1, 1, 3, "held", "payment", 2, None, None),
            row(1, 1, 4, "rejected", "payment", 2, None, Some("ended")),
        ],
    ];
    paths.push(vec![row(1, 1, 1, "refused", "payment", 1, None, None)]);
    for (i, path) in paths.into_iter().enumerate() {
        let category = if i == 3 {
            "tenant_rent"
        } else {
            "utility_bill_payment"
        };
        let mut store = store_with(category);
        for (n, move_row) in path.iter().enumerate() {
            ok(&mut store, move_row.clone());
            let count = effects(&store).len();
            assert_eq!(count, n + 1);
            ok(&mut store, move_row.clone());
            ok(&mut store, move_row.clone());
            assert_eq!(effects(&store).len(), count, "{move_row}");
        }
    }
}

#[test]
fn a_repeat_of_a_row_that_is_no_longer_last_is_refused() {
    let mut store = payment();
    let propose = row(1, 1, 1, "held", "payment", 1, None, None);
    let approve = row(1, 1, 2, "approved", "payment", 1, Some(1), None);
    let begin = row(1, 1, 3, "releasing", "payment", 1, Some(1), None);
    ok(&mut store, propose.clone());
    ok(&mut store, approve.clone());
    refused(&mut store, propose);
    ok(&mut store, begin.clone());
    refused(&mut store, approve);
    ok(
        &mut store,
        row(1, 1, 4, "committed", "payment", 1, Some(1), None),
    );
    refused(&mut store, begin);
}

#[test]
fn an_approval_held_by_any_row_is_used() {
    let mut store = payment();
    ok(&mut store, row(1, 1, 1, "held", "payment", 1, None, None));
    ok(
        &mut store,
        row(1, 1, 2, "approved", "payment", 1, Some(1), None),
    );
    ok(&mut store, row(1, 2, 1, "held", "payment", 1, None, None));
    refused(
        &mut store,
        row(1, 2, 2, "approved", "payment", 1, Some(1), None),
    );
    ok(
        &mut store,
        row(1, 2, 2, "approved", "payment", 1, Some(2), None),
    );

    ok(&mut store, row(1, 1, 3, "held", "payment", 2, None, None));
    ok(&mut store, row(1, 1, 4, "held", "payment", 1, None, None));
    refused(
        &mut store,
        row(1, 1, 5, "approved", "payment", 1, Some(1), None),
    );
    ok(
        &mut store,
        row(1, 1, 5, "approved", "payment", 1, Some(3), None),
    );

    start(&mut store, 2);
    ok(&mut store, row(2, 3, 1, "held", "payment", 1, None, None));
    refused(
        &mut store,
        row(2, 3, 2, "approved", "payment", 1, Some(3), None),
    );
    ok(
        &mut store,
        row(2, 3, 2, "approved", "payment", 1, Some(4), None),
    );
}

#[test]
fn an_approval_is_for_the_effect_s_digest() {
    let mut store = payment();
    ok(&mut store, row(1, 1, 1, "held", "payment", 1, None, None));
    refused(
        &mut store,
        row(1, 1, 2, "approved", "payment", 2, Some(1), None),
    );
    ok(
        &mut store,
        row(1, 1, 2, "approved", "payment", 1, Some(1), None),
    );
    refused(
        &mut store,
        row(1, 1, 3, "releasing", "payment", 2, Some(1), None),
    );
    ok(
        &mut store,
        row(1, 1, 3, "releasing", "payment", 1, Some(1), None),
    );
}

#[test]
fn an_effect_takes_two_rebinds() {
    let mut store = payment();
    ok(&mut store, row(1, 1, 1, "held", "payment", 1, None, None));
    ok(&mut store, row(1, 1, 2, "held", "payment", 2, None, None));
    ok(
        &mut store,
        row(1, 1, 3, "approved", "payment", 2, Some(1), None),
    );
    ok(&mut store, row(1, 1, 4, "held", "payment", 3, None, None));
    refused(&mut store, row(1, 1, 5, "held", "payment", 4, None, None));
    refused(&mut store, row(1, 1, 5, "held", "payment", 3, None, None));
    assert_eq!(effects(&store).len(), 4);
    ok(
        &mut store,
        row(1, 1, 5, "approved", "payment", 3, Some(2), None),
    );
    refused(&mut store, row(1, 1, 6, "held", "payment", 1, None, None));
}

#[test]
fn a_cleared_effect_is_not_rebound() {
    let mut store = store_with("spreadsheet_fill");
    ok(
        &mut store,
        row(1, 1, 1, "cleared", "local_edit", 1, None, None),
    );
    refused(
        &mut store,
        row(1, 1, 2, "held", "local_edit", 2, None, None),
    );
}

#[test]
fn begin_needs_an_approval_for_a_held_effect() {
    let mut store = payment();
    ok(&mut store, row(1, 1, 1, "held", "payment", 1, None, None));
    refused(
        &mut store,
        row(1, 1, 2, "releasing", "payment", 1, None, None),
    );
    refused(
        &mut store,
        row(1, 1, 2, "committed", "payment", 1, None, None),
    );
}

#[test]
fn no_runner_row_makes_unknown_cleared_or_refused_after_the_first() {
    let mut store = payment();
    ok(&mut store, row(1, 1, 1, "held", "payment", 1, None, None));
    ok(
        &mut store,
        row(1, 1, 2, "approved", "payment", 1, Some(1), None),
    );
    ok(
        &mut store,
        row(1, 1, 3, "releasing", "payment", 1, Some(1), None),
    );
    for state in ["unknown", "cleared", "refused"] {
        refused(&mut store, row(1, 1, 4, state, "payment", 1, None, None));
    }
    refused(
        &mut store,
        row(1, 1, 4, "rejected", "payment", 1, None, Some("ended")),
    );
    ok(
        &mut store,
        row(1, 1, 4, "committed", "payment", 1, Some(1), None),
    );
}

#[test]
fn a_row_keeps_the_seq_class_digest_and_kept_fields_of_its_effect() {
    let mut store = payment();
    refused(&mut store, row(1, 1, 2, "held", "payment", 1, None, None));
    ok(&mut store, row(1, 1, 1, "held", "payment", 1, None, None));
    refused(
        &mut store,
        row(1, 1, 3, "approved", "payment", 1, Some(1), None),
    );
    refused(
        &mut store,
        row(1, 1, 1, "approved", "payment", 1, Some(1), None),
    );
    refused(
        &mut store,
        row(1, 1, 2, "approved", "submit", 1, Some(1), None),
    );
    refused(&mut store, row(1, 1, 2, "held", "submit", 2, None, None));
    let mut other_kept = row(1, 1, 2, "approved", "payment", 1, Some(1), None);
    other_kept["kept"][0]["value"] = json!(["w"]);
    refused(&mut store, other_kept);
    let mut rebound = row(1, 1, 2, "held", "payment", 2, None, None);
    rebound["kept"][0]["value"] = json!(["w"]);
    ok(&mut store, rebound);
    let mut approved = row(1, 1, 3, "approved", "payment", 2, Some(1), None);
    approved["kept"][0]["value"] = json!(["w"]);
    ok(&mut store, approved);
    refused(
        &mut store,
        row(1, 1, 4, "rejected", "payment", 3, None, None),
    );
}

#[test]
fn a_row_of_another_workflow_or_run_is_refused() {
    let mut store = payment();
    let mut other = row(1, 1, 1, "held", "payment", 1, None, None);
    other["workflow"] = json!(wf(2));
    assert_eq!(put(&mut store, other), Err(declared("binding")));
    ok(&mut store, row(1, 1, 1, "held", "payment", 1, None, None));
    assert_eq!(
        put(
            &mut store,
            row(2, 1, 2, "approved", "payment", 1, Some(1), None)
        ),
        Err(declared("binding"))
    );
    assert_eq!(
        put(&mut store, row(2, 1, 1, "held", "payment", 1, None, None)),
        Err(declared("binding"))
    );
    let mut stranger = row(1, 2, 1, "held", "payment", 1, None, None);
    stranger["agent"] = json!(agent(2));
    assert_eq!(put(&mut store, stranger), Err(declared("binding")));
    assert_eq!(effects(&store).len(), 1);
}

#[test]
fn an_effect_body_whose_record_is_not_the_form_is_malformed() {
    let mut store = payment();
    let mut no_kept = row(1, 1, 1, "held", "payment", 1, None, None);
    no_kept["kept"] = json!([]);
    assert_eq!(put(&mut store, no_kept), Err(ErrorName::Malformed));
    let read = row(1, 1, 1, "held", "read", 1, None, None);
    assert_eq!(put(&mut store, read), Err(ErrorName::Malformed));
    assert_eq!(
        put(
            &mut store,
            row(1, 1, 1, "held", "payment", 1, Some(1), None)
        ),
        Err(ErrorName::Malformed)
    );
    assert!(effects(&store).is_empty());
}

#[test]
fn a_repeat_after_the_run_s_end_is_answered_as_the_first() {
    let mut store = payment();
    let commit = row(1, 1, 4, "committed", "payment", 1, Some(1), None);
    ok(&mut store, row(1, 1, 1, "held", "payment", 1, None, None));
    ok(
        &mut store,
        row(1, 1, 2, "approved", "payment", 1, Some(1), None),
    );
    ok(
        &mut store,
        row(1, 1, 3, "releasing", "payment", 1, Some(1), None),
    );
    ok(&mut store, commit.clone());
    assert_eq!(
        finish(&mut store, run_record(1, Some("replay"), "completed")),
        Ok(stored(1))
    );
    let count = effects(&store).len();
    assert_eq!(put(&mut store, commit), Ok(stored(1)));
    assert_eq!(effects(&store).len(), count);
    refused(&mut store, row(1, 2, 1, "held", "payment", 1, None, None));
}

#[test]
fn a_commit_a_reject_or_an_end_row_with_another_digest_is_refused() {
    let mut store = payment();
    ok(&mut store, row(1, 1, 1, "held", "payment", 1, None, None));
    ok(
        &mut store,
        row(1, 1, 2, "approved", "payment", 1, Some(1), None),
    );
    ok(
        &mut store,
        row(1, 1, 3, "releasing", "payment", 1, Some(1), None),
    );
    refused(
        &mut store,
        row(1, 1, 4, "committed", "payment", 2, Some(1), None),
    );
    ok(
        &mut store,
        row(1, 1, 4, "committed", "payment", 1, Some(1), None),
    );

    ok(&mut store, row(1, 2, 1, "held", "payment", 1, None, None));
    refused(
        &mut store,
        row(1, 2, 2, "rejected", "payment", 2, None, None),
    );
    ok(
        &mut store,
        row(1, 2, 2, "rejected", "payment", 1, None, None),
    );

    ok(&mut store, row(1, 3, 1, "held", "payment", 1, None, None));
    refused(
        &mut store,
        row(1, 3, 2, "rejected", "payment", 2, None, Some("ended")),
    );
    ok(
        &mut store,
        row(1, 3, 2, "rejected", "payment", 1, None, Some("ended")),
    );
}

fn day_refused(store: &mut MemoryStore, now: halvo_agent_store::Now, row: Value) {
    let before = effects(store).len();
    assert_eq!(
        put_at(store, now, row.clone()),
        Err(declared("day")),
        "{row}"
    );
    assert_eq!(effects(store).len(), before, "{row}");
}

#[test]
fn a_row_of_the_local_day_of_its_at_is_stored_and_the_utc_day_of_the_same_instant_is_refused() {
    let mut store = payment().with_utc_offset(WEST);
    let evening = local(ROW_DAY, 23 * 3600 + 30 * 60, WEST);
    assert_eq!(evening.div_euclid(NANOS_PER_DAY), i64::from(ROW_DAY) + 1);
    let held = |e| {
        dated(
            row(1, e, 1, "held", "payment", 1, None, None),
            ROW_DAY,
            evening,
        )
    };
    day_refused(&mut store, NOW, dated(held(2), ROW_DAY + 1, evening));
    assert_eq!(put_at(&mut store, NOW, held(1)), Ok(stored(1)));
    assert_eq!(last_of(&store, &effect(1)).day, ROW_DAY);
}

#[test]
fn a_row_whose_day_is_a_day_off_the_day_of_its_at_is_refused() {
    let mut store = payment();
    let held = row(1, 1, 1, "held", "payment", 1, None, None);
    day_refused(&mut store, NOW, dated(held.clone(), ROW_DAY + 1, ROW_AT));
    day_refused(&mut store, NOW, dated(held.clone(), ROW_DAY - 1, ROW_AT));
    assert_eq!(put_at(&mut store, NOW, held), Ok(stored(1)));
}

#[test]
fn a_row_after_the_day_of_the_clock_or_outside_its_bound_is_refused() {
    let mut store = payment();
    let held = |e| row(1, e, 1, "held", "payment", 1, None, None);
    let ahead = NOW.at.0 + NANOS_PER_DAY;
    day_refused(&mut store, NOW, dated(held(1), TODAY + 1, ahead));
    let gone = NOW.at.0 - 93 * NANOS_PER_DAY;
    day_refused(&mut store, NOW, dated(held(2), TODAY - 93, gone));
    let last = NOW.at.0 - 92 * NANOS_PER_DAY;
    assert_eq!(
        put_at(&mut store, NOW, dated(held(3), TODAY - 92, last)),
        Ok(stored(1))
    );
}

#[test]
fn a_row_built_before_midnight_is_stored_and_repeated_just_after_it() {
    let mut store = payment().with_utc_offset(WEST);
    let before = local(ROW_DAY, 86_399, WEST);
    let after = at(local(ROW_DAY + 1, 1, WEST));
    let held = dated(
        row(1, 1, 1, "held", "payment", 1, None, None),
        ROW_DAY,
        before,
    );
    assert_eq!(put_at(&mut store, after, held.clone()), Ok(stored(1)));
    assert_eq!(put_at(&mut store, after, held), Ok(stored(1)));
    assert_eq!(effects(&store).len(), 1);
}

#[test]
fn a_repeat_of_the_last_row_is_answered_as_the_first_under_a_clock_that_would_refuse_it() {
    let mut store = payment();
    let held = row(1, 1, 1, "held", "payment", 1, None, None);
    assert_eq!(put_at(&mut store, NOW, held.clone()), Ok(stored(1)));
    let late = at(ROW_AT + 93 * NANOS_PER_DAY);
    assert_eq!(put_at(&mut store, late, held.clone()), Ok(stored(1)));
    assert_eq!(effects(&store).len(), 1);
    day_refused(
        &mut store,
        late,
        row(1, 1, 2, "rejected", "payment", 1, None, None),
    );
}

#[test]
fn a_releasing_or_committed_row_holds_the_approval_of_the_effect_s_last_approved_row() {
    let mut store = payment();
    ok(&mut store, row(1, 1, 1, "held", "payment", 1, None, None));
    ok(
        &mut store,
        row(1, 1, 2, "approved", "payment", 1, Some(1), None),
    );
    ok(&mut store, row(1, 1, 3, "held", "payment", 2, None, None));
    ok(
        &mut store,
        row(1, 1, 4, "approved", "payment", 2, Some(2), None),
    );
    for other in [None, Some(1), Some(3)] {
        refused(
            &mut store,
            row(1, 1, 5, "releasing", "payment", 2, other, None),
        );
    }
    ok(
        &mut store,
        row(1, 1, 5, "releasing", "payment", 2, Some(2), None),
    );
    for other in [None, Some(1), Some(3)] {
        refused(
            &mut store,
            row(1, 1, 6, "committed", "payment", 2, other, None),
        );
    }
    ok(
        &mut store,
        row(1, 1, 6, "committed", "payment", 2, Some(2), None),
    );
}

#[test]
fn a_releasing_or_committed_row_after_cleared_holds_no_approval() {
    let mut store = unattended_sender();
    ok(&mut store, row(1, 1, 1, "cleared", "send", 1, None, None));
    refused(
        &mut store,
        row(1, 1, 2, "releasing", "send", 1, Some(1), None),
    );
    ok(&mut store, row(1, 1, 2, "releasing", "send", 1, None, None));
    refused(
        &mut store,
        row(1, 1, 3, "committed", "send", 1, Some(1), None),
    );
    ok(&mut store, row(1, 1, 3, "committed", "send", 1, None, None));
}

fn rows_of(store: &MemoryStore, e: u32) -> Vec<(u32, EffectState)> {
    let mut out: Vec<(u32, EffectState)> = effects(store)
        .into_iter()
        .filter(|r| r.effect.as_str() == effect(e))
        .map(|r| (r.seq.get(), r.state))
        .collect();
    out.sort_unstable_by_key(|(seq, _)| *seq);
    out
}

#[test]
fn a_row_that_differs_from_the_last_row_in_a_key_but_day_and_at_is_no_repeat() {
    let mut store = unattended_sender();
    ok(&mut store, row(1, 1, 1, "held", "payment", 1, None, None));
    ok(
        &mut store,
        row(1, 1, 2, "rejected", "payment", 1, None, None),
    );
    refused(
        &mut store,
        row(1, 1, 3, "rejected", "payment", 1, None, Some("ended")),
    );

    ok(&mut store, row(1, 2, 1, "held", "payment", 1, None, None));
    ok(
        &mut store,
        row(1, 2, 2, "rejected", "payment", 1, None, Some("ended")),
    );
    refused(
        &mut store,
        row(1, 2, 3, "rejected", "payment", 1, None, None),
    );

    ok(&mut store, row(1, 3, 1, "cleared", "send", 1, None, None));
    refused(
        &mut store,
        row(1, 3, 2, "cleared", "payment", 1, None, None),
    );
    refused(
        &mut store,
        row(1, 3, 1, "cleared", "payment", 1, None, None),
    );

    ok(&mut store, row(1, 4, 1, "held", "payment", 1, None, None));
    refused(&mut store, row(1, 4, 2, "held", "payment", 1, None, None));

    ok(&mut store, row(1, 5, 1, "cleared", "send", 1, None, None));
    ok(&mut store, row(1, 5, 2, "releasing", "send", 1, None, None));
    refused(&mut store, row(1, 5, 3, "releasing", "send", 1, None, None));
    let mut other_kept = row(1, 5, 2, "releasing", "send", 1, None, None);
    other_kept["kept"][0]["value"] = json!(["w"]);
    refused(&mut store, other_kept);
    ok(&mut store, row(1, 5, 3, "committed", "send", 1, None, None));
    refused(&mut store, row(1, 5, 4, "committed", "send", 1, None, None));

    assert_eq!(
        rows_of(&store, 1),
        [(1, EffectState::Held), (2, EffectState::Rejected)]
    );
    assert_eq!(rows_of(&store, 4), [(1, EffectState::Held)]);
    assert_eq!(rows_of(&store, 5).len(), 3);
}

#[test]
fn a_row_equal_to_the_last_row_but_for_day_and_at_is_a_repeat() {
    let mut store = unattended_sender();
    ok(&mut store, row(1, 1, 1, "cleared", "send", 1, None, None));
    let begin = row(1, 1, 2, "releasing", "send", 1, None, None);
    ok(&mut store, begin.clone());
    let later = at(NOW.at.0 + NANOS_PER_DAY);
    let moved = dated(begin, TODAY, NOW.at.0);
    assert_eq!(put_at(&mut store, later, moved), Ok(stored(1)));
    assert_eq!(end_run(&mut store, 1), Ok(accepted()));
    let lost = row(1, 1, 3, "unknown", "send", 1, None, None);
    assert_eq!(put(&mut store, lost), Ok(stored(1)));
    assert_eq!(
        rows_of(&store, 1),
        [
            (1, EffectState::Cleared),
            (2, EffectState::Releasing),
            (3, EffectState::Unknown)
        ]
    );
}
