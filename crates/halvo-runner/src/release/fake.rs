//! Tests of the confirm path with a fake paused request that reads its bound values from a list, one per read, and
//! counts what it lets leave; no fixture runs.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use halvo_policy::Facts;
use halvo_schema::executable::names::{Answer, FieldName, HandbackReason, LogEvent, Source};
use halvo_schema::executable::record::Slot;
use halvo_schema::executable::text::OwnValue;
use halvo_schema::executable::{
    ActionClass, Bound, BoundField, Cause, EffectState, FromRunner, Outcome, RunnerCall,
};
use halvo_schema::Category;

use super::suite::{run_many_path, run_path, Plan, Record};
use super::{Left, Paused, Replied};

#[derive(Default)]
struct Fake {
    reads: Vec<Bound>,
    at: usize,
    released: Arc<AtomicU32>,
    from_slot: bool,
    clock: Option<(Arc<AtomicU64>, u64)>,
    generation: u32,
    last: Option<Bound>,
    left: Arc<Mutex<Vec<(u32, Bound)>>>,
}

struct Answers;

impl Left for Answers {
    fn reply(self: Box<Self>) -> Replied {
        Replied::Reply
    }
}

impl Paused for Fake {
    fn class(&self) -> ActionClass {
        ActionClass::Delete
    }

    fn bound(&mut self, slots: &[Slot]) -> Result<Bound, Cause> {
        if self.from_slot {
            let item = slots
                .iter()
                .find(|s| s.name.as_str() == "item")
                .map(|s| s.value.joined())
                .ok_or(Cause::StepFailed)?;
            let read = delete(&item, Source::Extract);
            self.last = Some(read.clone());
            return Ok(read);
        }
        let read = self.reads[self.at.min(self.reads.len() - 1)].clone();
        self.at += 1;
        self.last = Some(read.clone());
        Ok(read)
    }

    fn facts(&self) -> Facts {
        Facts {
            ids: ["a", "b", "c", "d"].map(String::from).to_vec(),
            ..Facts::default()
        }
    }

    fn release(self: Box<Self>) -> Box<dyn Left> {
        self.released.fetch_add(1, Ordering::SeqCst);
        if let Some(last) = &self.last {
            self.left
                .lock()
                .expect("the record of what left")
                .push((self.generation, last.clone()));
        }
        Box::new(Answers)
    }

    fn renew(mut self: Box<Self>, _slots: &[Slot]) -> Result<Box<dyn Paused>, Cause> {
        if let Some((clock, ms)) = &self.clock {
            clock.fetch_add(*ms, Ordering::SeqCst);
        }
        self.generation += 1;
        Ok(self)
    }
}

fn delete(id: &str, from: Source) -> Bound {
    Bound::new(
        ActionClass::Delete,
        vec![BoundField {
            name: FieldName::Items,
            from,
            origin: None,
            value: OwnValue::new(id),
        }],
    )
    .expect("a bound object")
}

/// Runs the confirm path on a fake request whose reads give `reads` in turn; the record and the count of requests
/// that left.
fn run(reads: Vec<Bound>, plan: Plan) -> (Record, u32) {
    let released = Arc::new(AtomicU32::new(0));
    let fake = Fake {
        reads,
        at: 0,
        released: Arc::clone(&released),
        from_slot: false,
        clock: None,
        ..Fake::default()
    };
    let record = run_path(
        Category::CalendarCleanup,
        (Vec::new(), Vec::new()),
        Box::new(fake),
        plan,
    );
    (record, released.load(Ordering::SeqCst))
}

fn stopped(record: &Record) -> Option<(LogEvent, Option<Cause>)> {
    record.logs().last().map(|l| (l.event, l.cause))
}

/// A5.2, A6.3: values read again that equal the row's let the request leave once, and `commit` follows its reply.
#[test]
fn values_read_again_that_equal_the_rows_let_the_request_leave_once() {
    let (record, released) = run(vec![delete("a", Source::Extract)], Plan::plain());
    assert_eq!(released, 1);
    assert_eq!(
        record.states(),
        [
            EffectState::Held,
            EffectState::Approved,
            EffectState::Releasing,
            EffectState::Committed
        ]
    );
    assert_eq!(
        record.effects()[2].digest,
        halvo_policy::digest(&delete("a", Source::Extract)).expect("a digest")
    );
    assert_eq!(record.done(), Some(Outcome::Completed));
}

/// A5.2: a value that changes at every read takes two rebinds, each asked again, and at the third change the
/// effect is ended and the run ends `failed` with `changed`; nothing leaves.
#[test]
fn values_that_change_at_every_read_end_the_effect_after_two_rebinds() {
    let reads = ["a", "b", "c", "d"]
        .map(|id| delete(id, Source::Extract))
        .to_vec();
    let (record, released) = run(reads, Plan::plain());
    assert_eq!(released, 0, "nothing leaves");
    assert_eq!(record.confirms().len(), 3, "asked again after each rebind");
    assert_eq!(
        record.states(),
        [
            EffectState::Held,
            EffectState::Approved,
            EffectState::Held,
            EffectState::Approved,
            EffectState::Held,
            EffectState::Approved,
            EffectState::Rejected
        ]
    );
    assert_eq!(record.effects()[6].cause, Some(Cause::Ended));
    assert_eq!(record.done(), Some(Outcome::Failed));
    assert_eq!(
        stopped(&record),
        Some((LogEvent::Stopped, Some(Cause::Changed)))
    );
}

/// A6.2, CP-135 (4), reading 1: a cancel that the link holds when the answer to `begin` is taken lets nothing
/// leave; the effect is `releasing` at the `finish` and the run ends `effect_unknown`.
#[test]
fn a_cancel_held_at_the_answer_to_begin_lets_nothing_leave() {
    let plan = Plan {
        cancel_before_begin_answer: true,
        ..Plan::plain()
    };
    let (record, released) = run(vec![delete("a", Source::Extract)], plan);
    assert_eq!(released, 0, "nothing leaves after the cancel");
    assert_eq!(
        record.states(),
        [
            EffectState::Held,
            EffectState::Approved,
            EffectState::Releasing
        ]
    );
    assert_eq!(record.handbacks(), [HandbackReason::EffectUnknown]);
    assert_eq!(record.done(), Some(Outcome::EffectUnknown));
}

/// A5.1, CP-139: a later load whose `decide` gives `refuse` ends the effect, and the run ends `refused` with the
/// cause `autonomy`; nothing leaves.
#[test]
fn a_later_load_that_refuses_ends_the_effect() {
    let plan = Plan {
        observe_from_load: Some(2),
        ..Plan::plain()
    };
    let (record, released) = run(vec![delete("a", Source::Extract)], plan);
    assert_eq!(released, 0);
    assert_eq!(
        record.states(),
        [
            EffectState::Held,
            EffectState::Approved,
            EffectState::Rejected
        ]
    );
    assert_eq!(record.done(), Some(Outcome::Refused));
    assert_eq!(
        stopped(&record),
        Some((LogEvent::Stopped, Some(Cause::Autonomy)))
    );
}

/// A9.3: values read again that `admit` refuses take no `rebind`; the effect is ended and the run ends `failed`
/// with `provenance`.
#[test]
fn values_read_again_that_admit_refuses_end_the_effect() {
    let reads = vec![delete("a", Source::Extract), delete("b", Source::Model)];
    let (record, released) = run(reads, Plan::plain());
    assert_eq!(released, 0);
    assert_eq!(
        record.states(),
        [
            EffectState::Held,
            EffectState::Approved,
            EffectState::Rejected
        ]
    );
    assert_eq!(
        stopped(&record),
        Some((LogEvent::Stopped, Some(Cause::Provenance)))
    );
}

/// A5.2: the person's `reject_once` rejects the effect; nothing leaves and the run ends `declined`.
#[test]
fn reject_once_lets_nothing_leave() {
    let plan = Plan {
        answer: Answer::RejectOnce,
        ..Plan::plain()
    };
    let (record, released) = run(vec![delete("a", Source::Extract)], plan);
    assert_eq!(released, 0);
    assert_eq!(record.states(), [EffectState::Held, EffectState::Rejected]);
    assert_eq!(record.done(), Some(Outcome::Declined));
}

/// A9.3: a first read that `admit` refuses makes no `propose`; the run ends `failed` with `provenance`.
#[test]
fn a_first_read_that_admit_refuses_makes_no_propose() {
    let (record, released) = run(vec![delete("a", Source::Model)], Plan::plain());
    assert_eq!(released, 0);
    assert!(record.states().is_empty(), "no propose row");
    assert_eq!(
        stopped(&record),
        Some((LogEvent::Stopped, Some(Cause::Provenance)))
    );
}

/// A5.1: a `propose` that `decide` refuses makes the run end `refused`; nothing is asked and nothing leaves.
#[test]
fn a_refused_propose_ends_the_run_refused() {
    let plan = Plan {
        observe_from_load: Some(1),
        ..Plan::plain()
    };
    let (record, released) = run(vec![delete("a", Source::Extract)], plan);
    assert_eq!(released, 0);
    assert_eq!(record.states(), [EffectState::Refused]);
    assert!(record.confirms().is_empty());
    assert_eq!(record.done(), Some(Outcome::Refused));
}

/// A6.5: the `wall_s` of the load before the `begin`, lowered to 0, ends the run with `limit` and `wall` before
/// the `begin`; nothing leaves.
#[test]
fn a_wall_s_lowered_at_the_load_before_the_begin_ends_the_run() {
    let plan = Plan {
        wall_zero_from_load: Some(2),
        ..Plan::plain()
    };
    let (record, released) = run(vec![delete("a", Source::Extract)], plan);
    assert_eq!(released, 0);
    assert_eq!(record.states(), [EffectState::Held, EffectState::Approved]);
    assert_eq!(record.done(), Some(Outcome::Limit));
    assert_eq!(
        stopped(&record),
        Some((LogEvent::Stopped, Some(Cause::Wall)))
    );
}

/// A5.2: an approval that a row of the run held before is used, also after a `rebind`; the runner refuses it and
/// nothing leaves.
#[test]
fn an_approval_id_given_twice_is_refused_as_used() {
    let plan = Plan {
        one_approval_id: true,
        ..Plan::plain()
    };
    let reads = vec![delete("a", Source::Extract), delete("b", Source::Extract)];
    let (record, released) = run(reads, plan);
    assert_eq!(released, 0);
    assert_eq!(record.confirms().len(), 2);
    assert_eq!(
        record.states(),
        [EffectState::Held, EffectState::Approved, EffectState::Held]
    );
    assert_eq!(record.done(), Some(Outcome::Failed));
}

/// A6.3: a decision that does not answer the request's effects with their digests is a frame refusal: status 2,
/// no `approve` and nothing leaves.
#[test]
fn a_decision_with_another_digest_is_a_frame_refusal() {
    let plan = Plan {
        other_digest: true,
        ..Plan::plain()
    };
    let (record, released) = run(vec![delete("a", Source::Extract)], plan);
    assert_eq!(released, 0);
    assert_eq!(record.states(), [EffectState::Held]);
    assert_eq!(record.status, 2);
    assert!(record.finish().is_none());
}

/// CP-143 (4) (iv), the review's case confirm-1: a later load that gives `confirm` for a `cleared` effect asks more
/// than the load under which it was cleared; the effect is ended and proposed again as `held`, and its request
/// leaves only after the person's answer.
#[test]
fn a_later_load_that_asks_a_confirmation_for_a_cleared_effect_is_answered_before_anything_leaves() {
    let plan = Plan {
        runs_at: Some(|load| load == 1),
        ..Plan::plain()
    };
    let (record, released) = run(vec![delete("a", Source::Extract)], plan);
    assert_eq!(released, 1);
    assert_eq!(
        record.states(),
        [
            EffectState::Cleared,
            EffectState::Rejected,
            EffectState::Held,
            EffectState::Approved,
            EffectState::Releasing,
            EffectState::Committed
        ]
    );
    assert_eq!(record.effects()[1].cause, Some(Cause::Ended));
    assert_eq!(record.confirms().len(), 1);
    let confirm = record
        .frames
        .iter()
        .position(|f| matches!(f, halvo_schema::executable::FromRunner::Confirm(_)));
    let releasing = record.frames.iter().position(|f| {
        matches!(f, halvo_schema::executable::FromRunner::Store(s)
            if matches!(s.request.body(), halvo_schema::executable::RunnerCall::Effect(b)
                if b.record.state == EffectState::Releasing))
    });
    assert!(
        confirm.is_some_and(|c| releasing.is_some_and(|r| c < r)),
        "the confirmation comes before the begin"
    );
    assert_eq!(record.done(), Some(Outcome::Completed));
}

/// A5.1, A5.2, CP-143 (4) (iv), CP-157 E4: the times that one step proposes effects again are at most
/// `LIMITS.rebinds`, whether a `cleared` effect's values changed or a later load asks a confirmation; two value
/// changes and then a load that asks more end the run `failed` with `changed`, with nothing let through.
#[test]
fn the_times_that_one_step_proposes_again_are_at_most_two() {
    let plan = Plan {
        runs_at: Some(|load| load <= 3),
        ..Plan::plain()
    };
    let reads = ["a", "b", "c"]
        .map(|id| delete(id, Source::Extract))
        .to_vec();
    let (record, released) = run(reads, plan);
    assert_eq!(released, 0);
    assert!(record.confirms().is_empty());
    assert_eq!(
        record.states(),
        [
            EffectState::Cleared,
            EffectState::Rejected,
            EffectState::Cleared,
            EffectState::Rejected,
            EffectState::Cleared,
            EffectState::Rejected
        ]
    );
    assert_eq!(
        stopped(&record),
        Some((LogEvent::Stopped, Some(Cause::Changed)))
    );
}

/// A5.1, A5.2, the ruling CP-157 E1: a `cleared` effect whose slot changes at the load before its `begin` is
/// ended and proposed again from that load's slot, and its request leaves with the new value.
#[test]
fn a_cleared_effect_whose_slot_changes_at_the_load_before_the_begin_is_proposed_again() {
    let released = Arc::new(AtomicU32::new(0));
    let fake = Fake {
        reads: Vec::new(),
        at: 0,
        released: Arc::clone(&released),
        from_slot: true,
        clock: None,
        ..Fake::default()
    };
    let plan = Plan {
        runs_at: Some(|_| true),
        slots_at: Some(Box::new(|load, slots| {
            let value = if load >= 2 { "b" } else { "a" };
            slots.push(super::suite::slot("item", value));
        })),
        ..Plan::plain()
    };
    let record = run_path(
        Category::CalendarCleanup,
        (Vec::new(), Vec::new()),
        Box::new(fake),
        plan,
    );
    assert_eq!(released.load(Ordering::SeqCst), 1);
    assert_eq!(
        record.states(),
        [
            EffectState::Cleared,
            EffectState::Rejected,
            EffectState::Cleared,
            EffectState::Releasing,
            EffectState::Committed
        ]
    );
    let digest_b = halvo_policy::digest(&delete("b", Source::Extract)).expect("a digest");
    assert_eq!(
        record.effects()[2].digest,
        digest_b,
        "proposed again with the new slot"
    );
    assert_eq!(record.effects()[3].digest, digest_b);
    assert_eq!(record.done(), Some(Outcome::Completed));
}

/// Runs the confirm path on several fake requests in one call, each with its reads, with `spend_ms` added to the
/// run's clock at each `renew`; the record and the count of requests that left.
fn run_many(reads: Vec<Vec<Bound>>, spend_ms: u64, plan: Plan) -> (Record, u32) {
    let released = Arc::new(AtomicU32::new(0));
    let requests: Vec<Box<dyn Paused>> = reads
        .into_iter()
        .map(|reads| {
            Box::new(Fake {
                reads,
                at: 0,
                released: Arc::clone(&released),
                from_slot: false,
                clock: Some((Arc::clone(&plan.clock), spend_ms)),
                ..Fake::default()
            }) as Box<dyn Paused>
        })
        .collect();
    let record = run_many_path(
        Category::CalendarCleanup,
        (Vec::new(), Vec::new()),
        requests,
        plan,
    );
    (record, released.load(Ordering::SeqCst))
}

fn is_ended_row(frame: &FromRunner) -> bool {
    matches!(frame, FromRunner::Store(s)
        if matches!(s.request.body(), RunnerCall::Effect(b) if b.record.state == EffectState::Rejected))
}

/// Every effect of the record is ended with the cause `ended` or still as its first row stood, and none is `held`.
fn ended_or_as_it_stood(record: &Record) {
    let effects = record.effects();
    assert!(
        effects.iter().all(|e| e.state != EffectState::Held),
        "no held row: {:?}",
        record.states()
    );
    let ids: std::collections::BTreeSet<_> = effects.iter().map(|e| e.effect.clone()).collect();
    for id in ids {
        let rows: Vec<_> = effects.iter().filter(|e| e.effect == id).collect();
        let last = rows.last().expect("a row");
        let ended = last.state == EffectState::Rejected && last.cause == Some(Cause::Ended);
        assert!(
            ended || rows.len() == 1,
            "ended or as it stood: {:?}",
            record.states()
        );
    }
}

/// A6.2, CP-157 E3: a `cancel` that the link holds before the `propose` of a batch that case (iv) proposes again
/// is read before it: no `held` row and no confirmation; the run ends `declined`.
#[test]
fn a_cancel_held_before_the_batch_propose_is_read_before_it() {
    let reads = vec![
        vec![delete("a", Source::Extract)],
        vec![delete("b", Source::Extract)],
    ];
    let plan = Plan {
        runs_at: Some(|load| load == 1),
        cancel_before: Some(is_ended_row),
        ..Plan::plain()
    };
    let (record, released) = run_many(reads, 0, plan);
    assert_eq!(released, 0);
    assert!(record.confirms().is_empty(), "no confirmation request");
    ended_or_as_it_stood(&record);
    assert_eq!(record.done(), Some(Outcome::Declined));
}

/// A6.2, CP-157 E3: a `cancel` that the link holds before the `propose` of a new effect for a `cleared` effect whose
/// values changed is read before it: no second `cleared` row; the run ends `declined`.
#[test]
fn a_cancel_held_before_the_renewed_propose_is_read_before_it() {
    let reads = vec![vec![
        delete("a", Source::Extract),
        delete("b", Source::Extract),
    ]];
    let plan = Plan {
        runs_at: Some(|_| true),
        cancel_before: Some(is_ended_row),
        ..Plan::plain()
    };
    let (record, released) = run_many(reads, 0, plan);
    assert_eq!(released, 0);
    assert_eq!(
        record.states(),
        [EffectState::Cleared, EffectState::Rejected],
        "no propose after the cancel"
    );
    assert_eq!(record.done(), Some(Outcome::Declined));
}

/// A6.5, CP-157 E3: the run's time reaches the newest load's `wall_s` while the batch of case (iv) is made again
/// (the clock of the test's own, moved by each `renew`); the wall is read before each `propose` of the batch, so no
/// `held` row and no confirmation follow, and the run ends `limit` with `wall`.
#[test]
fn the_wall_is_read_before_each_propose_of_the_batch() {
    let reads = vec![
        vec![delete("a", Source::Extract)],
        vec![delete("b", Source::Extract)],
    ];
    let plan = Plan {
        runs_at: Some(|load| load == 1),
        wall_s: Some(5),
        ..Plan::plain()
    };
    let (record, released) = run_many(reads, 6000, plan);
    assert_eq!(released, 0);
    assert!(record.confirms().is_empty(), "no confirmation request");
    ended_or_as_it_stood(&record);
    assert_eq!(record.done(), Some(Outcome::Limit));
    assert_eq!(
        stopped(&record),
        Some((LogEvent::Stopped, Some(Cause::Wall)))
    );
}

fn count_of(record: &Record, state: EffectState) -> usize {
    record.states().iter().filter(|s| **s == state).count()
}

/// CP-157 E4 (a), adversary 1's case adv_c1_change_then_confirm: one change of a `cleared` effect's value, then a
/// load that gives `confirm` for the three `cleared` effects. That is two times: one new effect, then the batch of
/// three proposed again all together in one confirmation request; the run goes on and all three leave.
#[test]
fn one_change_and_a_batch_of_three_are_two_times_and_the_run_goes_on() {
    let reads = vec![
        vec![delete("a", Source::Extract), delete("d", Source::Extract)],
        vec![delete("b", Source::Extract)],
        vec![delete("c", Source::Extract)],
    ];
    let plan = Plan {
        runs_at: Some(|load| load <= 2),
        ..Plan::plain()
    };
    let (record, released) = run_many(reads, 0, plan);
    let confirms = record.confirms();
    assert_eq!(confirms.len(), 1, "one confirmation request");
    assert_eq!(confirms[0].effects.len(), 3, "with all three effects");
    let ids: std::collections::BTreeSet<_> =
        record.effects().iter().map(|e| e.effect.clone()).collect();
    assert_eq!(
        ids.len(),
        7,
        "three effects, one new effect, then three new effects"
    );
    assert_eq!(
        count_of(&record, EffectState::Cleared),
        4,
        "the first three and the one new effect"
    );
    assert_eq!(
        count_of(&record, EffectState::Held),
        3,
        "the batch, proposed once"
    );
    assert_eq!(released, 3);
    assert_eq!(record.done(), Some(Outcome::Completed));
}

/// CP-157 E4 (b), adversary 1's case adv_c1_three_cleared_then_confirm widened: two changes of a `cleared`
/// effect's value and then a load that gives `confirm` would be a third time; every open effect of the step is
/// ended with `ended`, and the run ends `failed` with `changed`.
#[test]
fn a_third_time_ends_every_open_effect_and_the_run_fails_changed() {
    let reads = vec![
        vec![
            delete("a", Source::Extract),
            delete("d", Source::Extract),
            delete("b", Source::Extract),
        ],
        vec![delete("c", Source::Extract)],
        vec![delete("d", Source::Extract)],
    ];
    let plan = Plan {
        runs_at: Some(|load| load <= 3),
        ..Plan::plain()
    };
    let (record, released) = run_many(reads, 0, plan);
    assert_eq!(released, 0);
    assert!(record.confirms().is_empty());
    let effects = record.effects();
    let ids: std::collections::BTreeSet<_> = effects.iter().map(|e| e.effect.clone()).collect();
    for id in &ids {
        let last = effects
            .iter()
            .rev()
            .find(|e| &e.effect == id)
            .expect("a row");
        assert_eq!(
            (last.state, last.cause),
            (EffectState::Rejected, Some(Cause::Ended)),
            "every effect ended: {:?}",
            record.states()
        );
    }
    assert_eq!(ids.len(), 5, "three effects and two new ones");
    assert_eq!(record.done(), Some(Outcome::Failed));
    assert_eq!(
        stopped(&record),
        Some((LogEvent::Stopped, Some(Cause::Changed)))
    );
}

/// Runs the confirm path on one fake request whose reads give `reads` in turn; the record and what left, each with
/// the generation of the request (raised by each `renew`) and the values of its last read.
fn run_recorded(reads: Vec<Bound>, plan: Plan) -> (Record, Vec<(u32, Bound)>) {
    let left = Arc::new(Mutex::new(Vec::new()));
    let fake = Fake {
        reads,
        left: Arc::clone(&left),
        ..Fake::default()
    };
    let record = run_path(
        Category::CalendarCleanup,
        (Vec::new(), Vec::new()),
        Box::new(fake),
        plan,
    );
    let left = left.lock().expect("the record of what left").clone();
    (record, left)
}

/// CP-157 E5 (a), adversary 3's k1: two paused requests in one call of `release`, both `cleared` and then asked
/// again by case (iv), stand in one confirmation request, and both leave.
#[test]
fn two_cleared_effects_are_asked_again_in_one_request() {
    let reads = vec![
        vec![delete("a", Source::Extract)],
        vec![delete("b", Source::Extract)],
    ];
    let plan = Plan {
        runs_at: Some(|load| load == 1),
        ..Plan::plain()
    };
    let (record, released) = run_many(reads, 0, plan);
    let confirms = record.confirms();
    assert_eq!(confirms.len(), 1, "one confirmation request");
    assert_eq!(confirms[0].effects.len(), 2, "with both effects");
    assert_eq!(released, 2, "both leave");
    assert_eq!(record.done(), Some(Outcome::Completed));
}

/// CP-157 E5 (b), adversary 3's k2: after a `rebind` the request that leaves is the one that `renew` made again,
/// with the values that the person was asked about the second time.
#[test]
fn after_a_rebind_the_request_is_renewed_before_it_leaves() {
    let reads = vec![delete("a", Source::Extract), delete("b", Source::Extract)];
    let (record, left) = run_recorded(reads, Plan::plain());
    assert_eq!(record.confirms().len(), 2);
    assert_eq!(
        left,
        [(1, delete("b", Source::Extract))],
        "the renewed request leaves"
    );
    assert_eq!(record.done(), Some(Outcome::Completed));
}

/// CP-157 E5 (b), adversary 3's k5: a `cleared` effect whose values change twice is proposed again each time and
/// leaves with the last values, made again twice.
#[test]
fn a_cleared_effect_whose_values_change_twice_leaves_with_the_last_values() {
    let reads = ["a", "b", "c"]
        .map(|id| delete(id, Source::Extract))
        .to_vec();
    let plan = Plan {
        runs_at: Some(|_| true),
        ..Plan::plain()
    };
    let (record, left) = run_recorded(reads, plan);
    assert_eq!(
        left,
        [(2, delete("c", Source::Extract))],
        "the last values leave"
    );
    let last_propose = record
        .effects()
        .into_iter()
        .rfind(|e| e.state == EffectState::Cleared)
        .expect("a propose row");
    assert_eq!(
        last_propose.digest,
        halvo_policy::digest(&delete("c", Source::Extract)).expect("a digest")
    );
    assert_eq!(record.done(), Some(Outcome::Completed));
}

/// CP-157 E5 (c), adversary 3's k4: the person's answer comes after the run's `wall_s` on the clock of the test's
/// own, and the wait for it is no time of `wall_s` (A6.5): the run still ends `completed`.
#[test]
fn the_wait_for_a_confirmation_is_no_time_of_wall_s() {
    let clock = Arc::new(AtomicU64::new(0));
    let during = Arc::clone(&clock);
    let plan = Plan {
        wall_s: Some(5),
        clock,
        on_confirm: Box::new(move |_| {
            during.fetch_add(10_000, Ordering::SeqCst);
        }),
        ..Plan::plain()
    };
    let (record, left) = run_recorded(vec![delete("a", Source::Extract)], plan);
    assert_eq!(left.len(), 1);
    assert_eq!(record.done(), Some(Outcome::Completed));
}

/// A6.5, CP-157 E3: the run's time reaches `wall_s` while a `cleared` effect whose values changed is made again
/// (the clock of the test's own, moved by `renew`); the wall is read before the new effect's `propose`, so no second
/// `cleared` row follows, and the run ends `limit` with `wall`.
#[test]
fn the_wall_is_read_before_the_renewed_propose() {
    let reads = vec![vec![
        delete("a", Source::Extract),
        delete("b", Source::Extract),
    ]];
    let plan = Plan {
        runs_at: Some(|_| true),
        wall_s: Some(5),
        ..Plan::plain()
    };
    let (record, released) = run_many(reads, 6000, plan);
    assert_eq!(released, 0);
    assert_eq!(
        record.states(),
        [EffectState::Cleared, EffectState::Rejected],
        "no propose after the wall"
    );
    assert_eq!(record.done(), Some(Outcome::Limit));
    assert_eq!(
        stopped(&record),
        Some((LogEvent::Stopped, Some(Cause::Wall)))
    );
}

/// CP-143 (4) (iv), CP-157 E1: an effect of the batch that case (iv) proposes again is proposed with the values read
/// again from its renewed request, and the person is shown those values in the one confirmation request.
#[test]
fn the_batch_is_proposed_with_the_values_read_again() {
    let reads = vec![
        vec![delete("a", Source::Extract)],
        vec![delete("b", Source::Extract), delete("c", Source::Extract)],
    ];
    let plan = Plan {
        runs_at: Some(|load| load == 1),
        ..Plan::plain()
    };
    let (record, released) = run_many(reads, 0, plan);
    let confirms = record.confirms();
    assert_eq!(confirms.len(), 1, "one confirmation request");
    let shown: Vec<String> = confirms[0]
        .effects
        .iter()
        .map(|e| e.bound.fields()[0].value.joined())
        .collect();
    assert_eq!(shown, ["a", "c"], "the batch shows the values read again");
    assert_eq!(released, 2);
    assert_eq!(record.done(), Some(Outcome::Completed));
}
