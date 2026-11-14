//! Tests of the confirm path with a fake paused request that reads its bound values from a list, one per read, and
//! counts what it lets leave; no fixture runs.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use halvo_schema::executable::names::{
    AllowedClass, Answer, FieldName, HandbackReason, LogEvent, Source,
};
use halvo_schema::executable::record::Slot;
use halvo_schema::executable::text::OwnValue;
use halvo_schema::executable::{
    ActionClass, Bound, BoundField, Cause, EffectState, FromRunner, Outcome, RunnerCall, WebOrigin,
};
use halvo_schema::Category;

use super::suite::{run_many_path, run_path, Plan, Record};
use super::{Left, MakerFacts, Paused, Replied};

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
    submit: bool,
    renew_fails: Option<Cause>,
    slots_seen: Arc<Mutex<Vec<Vec<String>>>>,
    fixed: Option<Bound>,
    fixed_reads: Vec<Bound>,
    no_reply: bool,
}

struct Answers;

impl Left for Answers {
    fn reply(self: Box<Self>) -> Replied {
        Replied::Made
    }
}

/// A request that left and got no reply that reads as made.
struct Silent;

impl Left for Silent {
    fn reply(self: Box<Self>) -> Replied {
        Replied::NoReply
    }
}

impl Paused for Fake {
    fn class(&self) -> ActionClass {
        if let Some(fixed) = self.fixed.as_ref().or(self.fixed_reads.first()) {
            return fixed.class();
        }
        if self.submit {
            ActionClass::Submit
        } else {
            ActionClass::Delete
        }
    }

    fn bound(&mut self, slots: &[Slot]) -> Result<Bound, Cause> {
        if let Some(fixed) = &self.fixed {
            return Ok(fixed.clone());
        }
        if !self.fixed_reads.is_empty() {
            let read = self.fixed_reads[self.at.min(self.fixed_reads.len() - 1)].clone();
            self.at += 1;
            self.last = Some(read.clone());
            return Ok(read);
        }
        self.slots_seen
            .lock()
            .expect("the record of slots")
            .push(slots.iter().map(|s| s.name.as_str().to_string()).collect());
        if self.submit {
            let value = slots
                .iter()
                .find(|s| s.name.as_str() == "fields")
                .map(|s| s.value.joined())
                .ok_or(Cause::Changed)?;
            let read = submit(&value);
            self.last = Some(read.clone());
            return Ok(read);
        }
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

    fn facts(&self) -> MakerFacts {
        if self.submit || self.fixed.is_some() || !self.fixed_reads.is_empty() {
            return MakerFacts {
                request_origin: Some(form_origin()),
                ..MakerFacts::default()
            };
        }
        MakerFacts {
            ids: ["a", "b", "c", "d"].map(String::from).to_vec(),
            ..MakerFacts::default()
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
        if self.no_reply {
            return Box::new(Silent);
        }
        Box::new(Answers)
    }

    fn renew(mut self: Box<Self>, _slots: &[Slot]) -> Result<Box<dyn Paused>, Cause> {
        if let Some(cause) = self.renew_fails {
            return Err(cause);
        }
        if let Some((clock, ms)) = &self.clock {
            clock.fetch_add(*ms, Ordering::SeqCst);
        }
        self.generation += 1;
        Ok(self)
    }
}

/// The origin of the fake's form.
fn form_origin() -> WebOrigin {
    WebOrigin::parse("http://form.agent.test:8080").expect("an origin")
}

/// The bound object of a `submit` of the form with `fields` from its slot.
fn submit(fields: &str) -> Bound {
    let origin = form_origin();
    Bound::new(
        ActionClass::Submit,
        vec![
            BoundField {
                name: FieldName::Origin,
                from: Source::Const,
                origin: None,
                value: OwnValue::new(origin.as_str()),
            },
            BoundField {
                name: FieldName::Fields,
                from: Source::Slot,
                origin: None,
                value: OwnValue::new(fields),
            },
        ],
    )
    .expect("a bound object")
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
/// The confirmation request that reaches the confirmer holds `cut` as null: this runner reads no list of
/// messages, so it cuts none (CP-171 J5).
#[test]
fn values_read_again_that_equal_the_rows_let_the_request_leave_once() {
    let (record, released) = run(vec![delete("a", Source::Extract)], Plan::plain());
    assert_eq!(
        record.confirms().iter().map(|c| c.cut).collect::<Vec<_>>(),
        [None],
        "the one confirmation request holds cut as null"
    );
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
fn reject_once_lets_no_request_of_its_effect_leave() {
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

/// A5.1, A5.2, CP-157 E1: a `cleared` `submit` effect whose slot `fields` changes at the load before its `begin` is
/// ended and proposed again from that load's slot, and its request leaves with the new value.
#[test]
fn a_cleared_effect_whose_slot_changes_at_the_load_before_the_begin_is_proposed_again() {
    let left = Arc::new(Mutex::new(Vec::new()));
    let fake = Fake {
        submit: true,
        left: Arc::clone(&left),
        ..Fake::default()
    };
    let plan = Plan {
        runs_at: Some(|_| true),
        slots_at: Some(Box::new(|load, slots| {
            let value = if load >= 2 { "b" } else { "a" };
            slots.push(super::suite::slot("fields", value));
        })),
        ..Plan::plain()
    };
    let record = run_path(
        Category::CalendarCleanup,
        (vec![form_origin()], Vec::new()),
        Box::new(fake),
        plan,
    );
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
    let digest_b = halvo_policy::digest(&submit("b")).expect("a digest");
    assert_eq!(
        record.effects()[2].digest,
        digest_b,
        "proposed again with the new slot"
    );
    let left = left.lock().expect("the record of what left").clone();
    assert_eq!(
        left,
        [(1, submit("b"))],
        "the renewed request leaves with the new slot"
    );
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
        cancel_before: Some(Box::new(is_ended_row)),
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
        cancel_before: Some(Box::new(is_ended_row)),
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

/// The correction of CP-157 E1, case 3: a `renew` that fails with another cause than `changed` after an approval
/// ends the effect with its `ended` row and writes no `rebind` row, so that no effect is left `held` with a card
/// that is never shown.
#[test]
fn a_renew_that_fails_after_an_approval_ends_the_effect_without_a_rebind_row() {
    let fake = Fake {
        reads: vec![delete("a", Source::Extract), delete("b", Source::Extract)],
        renew_fails: Some(Cause::StepFailed),
        ..Fake::default()
    };
    let record = run_path(
        Category::CalendarCleanup,
        (Vec::new(), Vec::new()),
        Box::new(fake),
        Plan::plain(),
    );
    assert_eq!(
        record.states(),
        [
            EffectState::Held,
            EffectState::Approved,
            EffectState::Rejected
        ]
    );
    assert_eq!(record.effects()[2].cause, Some(Cause::Ended));
    assert_eq!(record.confirms().len(), 1);
    assert_eq!(record.done(), Some(Outcome::Failed));
    assert_eq!(
        stopped(&record),
        Some((LogEvent::Stopped, Some(Cause::StepFailed)))
    );
}

/// The correction of CP-157 E1, (a): the port hands a maker the slots of its request's class alone: a `submit`
/// gets `fields` and no other slot of the load, at its first read and at its `begin`, and a `delete` gets none.
#[test]
fn the_port_hands_a_maker_the_slots_of_its_class_alone() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let fake = Fake {
        submit: true,
        slots_seen: Arc::clone(&seen),
        ..Fake::default()
    };
    let plan = Plan {
        runs_at: Some(|_| true),
        slots_at: Some(Box::new(|_, slots| {
            slots.push(super::suite::slot("fields", "a"));
            slots.push(super::suite::slot("payee", "Someone Else"));
            slots.push(super::suite::slot("reply_senders", "a@agent.test"));
        })),
        ..Plan::plain()
    };
    let record = run_path(
        Category::CalendarCleanup,
        (vec![form_origin()], Vec::new()),
        Box::new(fake),
        plan,
    );
    assert_eq!(record.done(), Some(Outcome::Completed));
    let seen = seen.lock().expect("the record of slots").clone();
    assert_eq!(
        seen,
        [vec!["fields".to_string()], vec!["fields".to_string()]]
    );

    let seen = Arc::new(Mutex::new(Vec::new()));
    let fake = Fake {
        reads: vec![delete("a", Source::Extract)],
        slots_seen: Arc::clone(&seen),
        ..Fake::default()
    };
    let plan = Plan {
        slots_at: Some(Box::new(|_, slots| {
            slots.push(super::suite::slot("payee", "Someone Else"));
        })),
        ..Plan::plain()
    };
    let record = run_path(
        Category::CalendarCleanup,
        (Vec::new(), Vec::new()),
        Box::new(fake),
        plan,
    );
    assert_eq!(record.done(), Some(Outcome::Completed));
    let seen = seen.lock().expect("the record of slots").clone();
    assert!(
        seen.iter().all(|s| s.is_empty()),
        "a delete gets no slot: {seen:?}"
    );
}

/// The correction of CP-157 E1, (b): a slot of the class that the load before the `begin` no longer holds does not
/// end as a failed step; the effect is ended and the run ends `failed` with `changed`.
#[test]
fn a_slot_of_the_class_that_the_later_load_no_longer_holds_ends_the_run_changed() {
    let fake = Fake {
        submit: true,
        ..Fake::default()
    };
    let plan = Plan {
        slots_at: Some(Box::new(|load, slots| {
            if load == 1 {
                slots.push(super::suite::slot("fields", "a"));
            }
        })),
        ..Plan::plain()
    };
    let record = run_path(
        Category::CalendarCleanup,
        (vec![form_origin()], Vec::new()),
        Box::new(fake),
        plan,
    );
    assert_eq!(
        record.states(),
        [
            EffectState::Held,
            EffectState::Approved,
            EffectState::Rejected
        ]
    );
    assert_eq!(record.done(), Some(Outcome::Failed));
    assert_eq!(
        stopped(&record),
        Some((LogEvent::Stopped, Some(Cause::Changed)))
    );
}

/// The origin of the page from which the fake's amounts and items are read.
fn page_origin() -> WebOrigin {
    WebOrigin::parse("http://page.agent.test:8080").expect("an origin")
}

fn field(name: FieldName, from: Source, origin: Option<WebOrigin>, value: &str) -> BoundField {
    BoundField {
        name,
        from,
        origin,
        value: OwnValue::new(value),
    }
}

/// A `payment`, a `purchase` and a `submit` whose request goes to the form's origin, with the amounts and items
/// read from the page's origin.
fn to_the_form() -> Vec<Bound> {
    let form = form_origin();
    let page = Some(page_origin());
    vec![
        Bound::new(
            ActionClass::Payment,
            vec![
                field(FieldName::Payee, Source::Slot, None, "Larkspur"),
                field(FieldName::Amount, Source::Extract, page.clone(), "84.37"),
                field(
                    FieldName::Instrument,
                    Source::Slot,
                    None,
                    "Card ending 4417",
                ),
                field(FieldName::Origin, Source::Const, None, form.as_str()),
            ],
        )
        .expect("a payment"),
        Bound::new(
            ActionClass::Purchase,
            vec![
                field(FieldName::Seller, Source::Slot, None, "Bramble"),
                field(FieldName::Items, Source::Extract, page.clone(), "tea"),
                field(FieldName::Amount, Source::Extract, page, "12.00"),
                field(
                    FieldName::Instrument,
                    Source::Slot,
                    None,
                    "Card ending 4417",
                ),
                field(FieldName::Origin, Source::Const, None, form.as_str()),
            ],
        )
        .expect("a purchase"),
        submit("a"),
    ]
}

/// CP-159 part 2, CP-166 E1: a request of `class` to an origin that no binding holds gets no card and no request;
/// the new origin of the facts is the runner's, none at this head, so no maker can let it pass.
fn no_card_for_an_origin_that_no_binding_holds(class: ActionClass) {
    let bound = to_the_form()
        .into_iter()
        .find(|b| b.class() == class)
        .expect("a bound object of the class");
    let released = Arc::new(AtomicU32::new(0));
    let fake = Fake {
        fixed: Some(bound),
        released: Arc::clone(&released),
        ..Fake::default()
    };
    let record = run_path(
        Category::UtilityBillPayment,
        (vec![page_origin()], Vec::new()),
        Box::new(fake),
        Plan::plain(),
    );
    assert_eq!(released.load(Ordering::SeqCst), 0, "{class:?}: no request");
    assert!(record.confirms().is_empty(), "{class:?}: no card");
    assert!(record.states().is_empty(), "{class:?}: no propose row");
    assert_eq!(
        stopped(&record),
        Some((LogEvent::Stopped, Some(Cause::Provenance))),
        "{class:?}"
    );
}

#[test]
fn a_payment_to_an_origin_that_no_binding_holds_gets_no_card() {
    no_card_for_an_origin_that_no_binding_holds(ActionClass::Payment);
}

#[test]
fn a_purchase_to_an_origin_that_no_binding_holds_gets_no_card() {
    no_card_for_an_origin_that_no_binding_holds(ActionClass::Purchase);
}

#[test]
fn a_submit_to_an_origin_that_no_binding_holds_gets_no_card() {
    no_card_for_an_origin_that_no_binding_holds(ActionClass::Submit);
}

/// CP-166 E6, CP-174 Part 1: on one card for two effects the person rejects the first and allows the second. The
/// rejected effect's request never leaves, the allowed one's leaves once, and the run then ends `declined`.
#[test]
fn reject_once_for_one_effect_and_allow_once_for_the_other_lets_the_allowed_one_leave_once() {
    let left = Arc::new(Mutex::new(Vec::new()));
    let requests: Vec<Box<dyn Paused>> = ["a", "b"]
        .iter()
        .map(|id| {
            Box::new(Fake {
                reads: vec![delete(id, Source::Extract)],
                left: Arc::clone(&left),
                ..Fake::default()
            }) as Box<dyn Paused>
        })
        .collect();
    let plan = Plan {
        answer_for: Some(|_, index| {
            if index == 0 {
                Answer::RejectOnce
            } else {
                Answer::AllowOnce
            }
        }),
        ..Plan::plain()
    };
    let record = run_many_path(
        Category::CalendarCleanup,
        (Vec::new(), Vec::new()),
        requests,
        plan,
    );
    assert_eq!(record.confirms().len(), 1, "one card for both");
    let left = left.lock().expect("the record of what left").clone();
    assert_eq!(
        left,
        [(0, delete("b", Source::Extract))],
        "the allowed request leaves once"
    );
    assert_eq!(
        record.done(),
        Some(Outcome::Declined),
        "a reject_once ends its effect alone, and with no effect open the run ends declined (CP-174 Part 1 (c))"
    );
}

/// Runs the confirm path on one fake request for each list of reads, in one call of `release`, all writing into one
/// record of what left; the request of the index `no_reply_at` gets no reply that reads as made. The record and
/// what left, each with the generation of its request and the values of its last read.
fn run_each(
    reads: Vec<Vec<Bound>>,
    no_reply_at: Option<usize>,
    plan: Plan,
) -> (Record, Vec<(u32, Bound)>) {
    let left = Arc::new(Mutex::new(Vec::new()));
    let requests: Vec<Box<dyn Paused>> = reads
        .into_iter()
        .enumerate()
        .map(|(i, reads)| {
            Box::new(Fake {
                reads,
                left: Arc::clone(&left),
                no_reply: no_reply_at == Some(i),
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
    let left = left.lock().expect("the record of what left").clone();
    (record, left)
}

/// The count of `load` frames that the runner wrote.
fn loads(record: &Record) -> usize {
    record
        .frames
        .iter()
        .filter(|f| matches!(f, FromRunner::Store(s) if matches!(s.request.body(), RunnerCall::Load(_))))
        .count()
}

/// The ids of the effects in the order of their first rows.
fn effect_ids(record: &Record) -> Vec<halvo_schema::executable::EffectId> {
    let mut ids = Vec::new();
    for e in record.effects() {
        if !ids.contains(&e.effect) {
            ids.push(e.effect);
        }
    }
    ids
}

/// The state and the cause of the last row of each effect, in the order of the effects' first rows.
fn last_rows(record: &Record) -> Vec<(EffectState, Option<Cause>)> {
    let effects = record.effects();
    effect_ids(record)
        .iter()
        .map(|id| {
            let last = effects
                .iter()
                .rev()
                .find(|e| &e.effect == id)
                .expect("a row");
            (last.state, last.cause)
        })
        .collect()
}

/// The first card allows both effects; every later card rejects.
fn allow_then_reject(card: usize, _: usize) -> Answer {
    if card == 1 {
        Answer::AllowOnce
    } else {
        Answer::RejectOnce
    }
}

fn a_then_c() -> Vec<Bound> {
    vec![delete("a", Source::Extract), delete("c", Source::Extract)]
}

/// CP-174 Part 1 (a), F1 (a): [A, B] are allowed on the first card. A's values change at the load before its
/// `begin`, so A takes a `rebind`, and its second card is answered `reject_once`. The reject ends A alone: B behind
/// it goes to its `begin` after its own load, leaves once and is `committed`, and the run then ends `declined`.
#[test]
fn a_reject_on_the_card_after_a_rebind_ends_that_effect_alone_and_the_effect_behind_leaves() {
    let plan = Plan {
        answer_for: Some(allow_then_reject),
        ..Plan::plain()
    };
    let (record, left) = run_each(
        vec![a_then_c(), vec![delete("b", Source::Extract)]],
        None,
        plan,
    );
    assert_eq!(
        left,
        [(0, delete("b", Source::Extract))],
        "B leaves once; A's request never leaves"
    );
    assert_eq!(record.confirms().len(), 2, "the first card and A's second");
    assert_eq!(
        last_rows(&record),
        [
            (EffectState::Rejected, None),
            (EffectState::Committed, None)
        ]
    );
    assert_eq!(
        loads(&record),
        3,
        "the first load, the load before A's begin and the load before B's"
    );
    assert_eq!(record.done(), Some(Outcome::Declined));
}

/// CP-174 Part 1 (a), F1 (b) as Addition 1 of CP-174 rules it: A is a `cleared` `delete` and B a `held` `submit`
/// that the first card allows. The load before A's `begin` gives `confirm` for A, so A is ended and proposed again
/// as `held`, and the batch's card, which holds A's new effect alone, is answered `reject_once`. The reject ends A's
/// new effect alone: B behind it leaves once and is `committed`, and the run then ends `declined`.
#[test]
fn a_reject_on_the_card_of_a_batch_ends_that_effect_alone_and_the_effect_behind_leaves() {
    let left = Arc::new(Mutex::new(Vec::new()));
    let requests: Vec<Box<dyn Paused>> = vec![
        Box::new(Fake {
            reads: vec![delete("a", Source::Extract)],
            left: Arc::clone(&left),
            ..Fake::default()
        }),
        Box::new(Fake {
            submit: true,
            left: Arc::clone(&left),
            ..Fake::default()
        }),
    ];
    let plan = Plan {
        answer_for: Some(allow_then_reject),
        allow_at: Some(|load| {
            if load == 1 {
                vec![AllowedClass::Delete]
            } else {
                Vec::new()
            }
        }),
        ..Plan::plain()
    };
    let record = run_many_path(
        Category::CalendarCleanup,
        (vec![form_origin()], vec![super::suite::slot("fields", "x")]),
        requests,
        plan,
    );
    let left = left.lock().expect("the record of what left").clone();
    assert_eq!(
        left,
        [(0, submit("x"))],
        "B leaves once; A's request never leaves"
    );
    assert_eq!(
        last_rows(&record),
        [
            (EffectState::Rejected, Some(Cause::Ended)),
            (EffectState::Committed, None),
            (EffectState::Rejected, None)
        ],
        "A ended for the batch, B committed, A's new effect rejected"
    );
    let confirms = record.confirms();
    assert_eq!(confirms.len(), 2, "the first card and the batch's");
    let new_effect = effect_ids(&record)[2].clone();
    assert_eq!(
        confirms[1]
            .effects
            .iter()
            .map(|e| e.effect.clone())
            .collect::<Vec<_>>(),
        [new_effect],
        "the batch's card holds A's new effect alone"
    );
    assert_eq!(
        loads(&record),
        3,
        "the first load, the load before A's begin and the load before B's"
    );
    assert_eq!(record.done(), Some(Outcome::Declined));
}

/// CP-174 Part 1 (c), F1 (c): B, before A in the queue, has left once already when A's card after its `rebind` is
/// answered `reject_once`; no effect is open then, and the run ends `declined`, although B is `committed`.
#[test]
fn a_reject_after_an_effect_has_left_ends_the_run_declined() {
    let plan = Plan {
        answer_for: Some(allow_then_reject),
        ..Plan::plain()
    };
    let (record, left) = run_each(
        vec![vec![delete("b", Source::Extract)], a_then_c()],
        None,
        plan,
    );
    assert_eq!(
        left,
        [(0, delete("b", Source::Extract))],
        "B left once; A's request never leaves"
    );
    assert_eq!(
        last_rows(&record),
        [
            (EffectState::Committed, None),
            (EffectState::Rejected, None)
        ]
    );
    assert_eq!(
        loads(&record),
        3,
        "the first load, the load before B's begin and the load before A's"
    );
    assert_eq!(record.done(), Some(Outcome::Declined));
}

/// CP-174 Part 1 (c), F1 (d): A's card after its `rebind` is answered `reject_once`, and then B's request gets no
/// reply that reads as made. The end that B's own path gives is the run's end: `effect_unknown`, not `declined`.
#[test]
fn after_a_reject_an_effect_whose_request_gets_no_reply_ends_the_run_effect_unknown() {
    let plan = Plan {
        answer_for: Some(allow_then_reject),
        ..Plan::plain()
    };
    let (record, left) = run_each(
        vec![a_then_c(), vec![delete("b", Source::Extract)]],
        Some(1),
        plan,
    );
    assert_eq!(
        left,
        [(0, delete("b", Source::Extract))],
        "B leaves once; A's request never leaves"
    );
    assert_eq!(
        last_rows(&record),
        [
            (EffectState::Rejected, None),
            (EffectState::Releasing, None)
        ]
    );
    assert_eq!(loads(&record), 3);
    assert_eq!(record.done(), Some(Outcome::EffectUnknown));
}

/// CP-174 Part 1 (c): A's card after its `rebind` is answered `reject_once`, and then the load before B's `begin`
/// gives `refuse` (autonomy `observe`). The end that B's own path gives is the run's end: `refused`, not `declined`.
/// The runner's close makes any run with a `releasing` effect `effect_unknown`, so (d) cannot show this; this test
/// fails when the flag of the reject takes the place of that end.
#[test]
fn after_a_reject_a_later_load_that_refuses_ends_the_run_refused() {
    let plan = Plan {
        answer_for: Some(allow_then_reject),
        observe_from_load: Some(3),
        ..Plan::plain()
    };
    let (record, left) = run_each(
        vec![a_then_c(), vec![delete("b", Source::Extract)]],
        None,
        plan,
    );
    assert!(left.is_empty(), "nothing leaves");
    assert_eq!(
        last_rows(&record),
        [
            (EffectState::Rejected, None),
            (EffectState::Rejected, Some(Cause::Ended))
        ],
        "A rejected by the person, B ended by the runner"
    );
    assert_eq!(loads(&record), 3);
    assert_eq!(record.done(), Some(Outcome::Refused));
}

/// CP-174 Part 1, F1 (e): a `cleared` effect whose value changed is ended by the runner, which leaves a `rejected`
/// row with the cause `ended`, proposed again as `cleared` and `committed`. No `reject_once` came, so the run does
/// not end `declined`: the flag of a reject is not read from the state `rejected`.
#[test]
fn an_effect_that_the_runner_ended_and_proposed_again_does_not_end_the_run_declined() {
    let plan = Plan {
        runs_at: Some(|_| true),
        ..Plan::plain()
    };
    let (record, left) = run_each(
        vec![vec![
            delete("a", Source::Extract),
            delete("b", Source::Extract),
        ]],
        None,
        plan,
    );
    assert_eq!(
        left,
        [(1, delete("b", Source::Extract))],
        "the request leaves once, made again for the new effect"
    );
    assert_eq!(
        last_rows(&record),
        [
            (EffectState::Rejected, Some(Cause::Ended)),
            (EffectState::Committed, None)
        ]
    );
    assert_eq!(loads(&record), 3);
    assert_eq!(record.done(), Some(Outcome::Completed));
}

fn is_confirm(frame: &FromRunner) -> bool {
    matches!(frame, FromRunner::Confirm(_))
}

/// Holds for the `n`th confirmation request, counted from 1.
fn the_card(n: usize) -> super::suite::CancelBefore {
    let mut cards = 0;
    Box::new(move |frame| {
        let card = is_confirm(frame);
        if card {
            cards += 1;
        }
        card && cards == n
    })
}

fn is_rejected_row(frame: &FromRunner) -> bool {
    matches!(frame, FromRunner::Store(s)
        if matches!(s.request.body(), RunnerCall::Effect(b) if b.record.state == EffectState::Rejected))
}

/// The states of the effect rows and the count of `load` frames that the runner wrote after its last confirmation
/// request.
fn after_the_card(record: &Record) -> (Vec<EffectState>, usize) {
    let at = record
        .frames
        .iter()
        .rposition(is_confirm)
        .expect("a confirmation request");
    let rest = &record.frames[at + 1..];
    let states = rest
        .iter()
        .filter_map(|f| match f {
            FromRunner::Store(s) => match s.request.body() {
                RunnerCall::Effect(b) => Some(b.record.state),
                _ => None,
            },
            _ => None,
        })
        .collect();
    let loads = rest
        .iter()
        .filter(|f| matches!(f, FromRunner::Store(s) if matches!(s.request.body(), RunnerCall::Load(_))))
        .count();
    (states, loads)
}

/// CP-174 Part 2 (a), F2 T1, the pin of CP-166 E7 turned: a cancel written before the person's `reject_once` reply
/// is read when the decision has come, after the `rejected` row is stored. The effect is `rejected`, no load follows
/// the card, nothing leaves, and the run ends `declined`.
#[test]
fn a_cancel_before_a_reject_once_reply_is_read_after_the_rejected_row() {
    let plan = Plan {
        answer: Answer::RejectOnce,
        cancel_before: Some(Box::new(is_confirm)),
        ..Plan::plain()
    };
    let (record, released) = run(vec![delete("a", Source::Extract)], plan);
    assert_eq!(released, 0);
    assert_eq!(
        record.states(),
        [EffectState::Held, EffectState::Rejected],
        "the reject is stored before the cancel is read"
    );
    assert_eq!(
        after_the_card(&record),
        (vec![EffectState::Rejected], 0),
        "after the card: the rejected row and no load"
    );
    assert_eq!(record.done(), Some(Outcome::Declined));
}

/// CP-174 Part 2 (a) and (b), F2 T2: a cancel written before the person's `allow_once` reply is read before any
/// `approved` row: the effect stays `held`, no row and no load follow the card, nothing leaves, and the run ends
/// `declined`.
#[test]
fn a_cancel_before_an_allow_once_reply_is_read_before_the_approved_row() {
    let plan = Plan {
        cancel_before: Some(Box::new(is_confirm)),
        ..Plan::plain()
    };
    let (record, released) = run(vec![delete("a", Source::Extract)], plan);
    assert_eq!(released, 0);
    assert_eq!(record.states(), [EffectState::Held], "no approved row");
    assert_eq!(
        after_the_card(&record),
        (Vec::new(), 0),
        "after the card: no row and no load"
    );
    assert_eq!(record.done(), Some(Outcome::Declined));
}

/// The first effect of a card is rejected and every other one allowed.
fn reject_the_first(_: usize, index: usize) -> Answer {
    if index == 0 {
        Answer::RejectOnce
    } else {
        Answer::AllowOnce
    }
}

/// CP-174 Part 2 (a), F2 T3: one card for [A, B] that rejects A and allows B, with a cancel written before the reply:
/// A's `rejected` row is stored, then the cancel is read, so B stays `held`; no load follows, nothing leaves, and the
/// run ends `declined`.
#[test]
fn a_cancel_before_a_reply_that_rejects_one_effect_and_allows_the_other_stores_the_reject_alone() {
    let plan = Plan {
        answer_for: Some(reject_the_first),
        cancel_before: Some(Box::new(is_confirm)),
        ..Plan::plain()
    };
    let (record, left) = run_each(
        vec![
            vec![delete("a", Source::Extract)],
            vec![delete("b", Source::Extract)],
        ],
        None,
        plan,
    );
    assert!(left.is_empty(), "nothing leaves");
    assert_eq!(
        last_rows(&record),
        [(EffectState::Rejected, None), (EffectState::Held, None)]
    );
    assert_eq!(after_the_card(&record), (vec![EffectState::Rejected], 0));
    assert_eq!(record.done(), Some(Outcome::Declined));
}

/// CP-174 Part 2 (a), F2 T4: the same card, with the cancel written before the reply to A's `rejected` row. The
/// cancel is read after the rejects and before the allows, so the rows and the end are T3's.
#[test]
fn a_cancel_before_the_answer_to_the_rejected_row_is_read_before_the_approved_row() {
    let plan = Plan {
        answer_for: Some(reject_the_first),
        cancel_before: Some(Box::new(is_rejected_row)),
        ..Plan::plain()
    };
    let (record, left) = run_each(
        vec![
            vec![delete("a", Source::Extract)],
            vec![delete("b", Source::Extract)],
        ],
        None,
        plan,
    );
    assert!(left.is_empty(), "nothing leaves");
    assert_eq!(
        last_rows(&record),
        [(EffectState::Rejected, None), (EffectState::Held, None)]
    );
    assert_eq!(after_the_card(&record), (vec![EffectState::Rejected], 0));
    assert_eq!(record.done(), Some(Outcome::Declined));
}

/// CP-174 Part 2 (a), F2 T5: [A, B] are allowed on the first card, A takes a `rebind`, and a cancel is written
/// before the reply to A's second card, which rejects. A is `rejected`, no load follows the card, B gets no `begin`
/// row and nothing leaves, and the run ends `declined`.
#[test]
fn a_cancel_before_a_reject_on_the_card_after_a_rebind_ends_the_run_before_the_effect_behind() {
    let plan = Plan {
        answer_for: Some(allow_then_reject),
        cancel_before: Some(the_card(2)),
        ..Plan::plain()
    };
    let (record, left) = run_each(
        vec![a_then_c(), vec![delete("b", Source::Extract)]],
        None,
        plan,
    );
    assert!(left.is_empty(), "nothing leaves");
    assert_eq!(
        last_rows(&record),
        [(EffectState::Rejected, None), (EffectState::Approved, None)],
        "A rejected; B as the first card left it, with no begin row"
    );
    assert_eq!(after_the_card(&record), (vec![EffectState::Rejected], 0));
    assert_eq!(record.done(), Some(Outcome::Declined));
}

/// The frames of the `load` of the count `n`, counted from 1.
fn the_load(n: usize) -> super::suite::CancelBefore {
    let mut loads = 0;
    Box::new(move |frame| {
        let load = matches!(frame, FromRunner::Store(s) if matches!(s.request.body(), RunnerCall::Load(_)));
        if load {
            loads += 1;
        }
        load && loads == n
    })
}

/// CP-166 E2 (A6.2): a cancel that comes with the answer to the load before the first `propose` is read before it:
/// no `propose` row and no card; the run ends `declined`.
#[test]
fn a_cancel_with_the_first_load_is_read_before_the_first_propose() {
    let plan = Plan {
        cancel_before: Some(the_load(1)),
        ..Plan::plain()
    };
    let (record, released) = run(vec![delete("a", Source::Extract)], plan);
    assert_eq!(released, 0);
    assert!(record.states().is_empty(), "no propose row");
    assert!(record.confirms().is_empty(), "no card");
    assert_eq!(record.done(), Some(Outcome::Declined));
}

/// CP-166 E2 (A6.5): a `wall_s` that the load before the first `propose` lowers to the run's time is read before
/// it: no `held` row and no card; the run ends `limit` with `wall`.
#[test]
fn a_wall_s_of_the_first_load_is_read_before_the_first_propose() {
    let plan = Plan {
        wall_zero_from_load: Some(1),
        ..Plan::plain()
    };
    let (record, released) = run(vec![delete("a", Source::Extract)], plan);
    assert_eq!(released, 0);
    assert!(record.states().is_empty(), "no held row");
    assert!(record.confirms().is_empty(), "no card");
    assert_eq!(record.done(), Some(Outcome::Limit));
    assert_eq!(
        stopped(&record),
        Some((LogEvent::Stopped, Some(Cause::Wall)))
    );
}

/// CP-166 E2 (A6.2): a cancel that comes with the load before the `begin` is read after that load: no `begin`
/// row, and the run ends `declined` with no effect `releasing`.
#[test]
fn a_cancel_with_the_load_before_the_begin_is_read_before_the_begin() {
    let plan = Plan {
        cancel_before: Some(the_load(2)),
        ..Plan::plain()
    };
    let (record, released) = run(vec![delete("a", Source::Extract)], plan);
    assert_eq!(released, 0);
    assert_eq!(
        record.states(),
        [EffectState::Held, EffectState::Approved],
        "no begin row"
    );
    assert!(record.handbacks().is_empty(), "no effect is releasing");
    assert_eq!(record.done(), Some(Outcome::Declined));
}

/// CP-166 E3 (the verifier's changes 7 and 57): a `delete` and a `submit`, both `cleared`, turn to `confirm` one class
/// at a time. The delete's turn is the first time (the batch of both); the submit's value change is the second;
/// the submit's own turn would be a third, so every open effect of the step is ended and the run ends `failed`
/// with `changed`, after five effects in all and one request let through.
#[test]
fn a_further_time_after_a_batch_ends_every_open_effect_changed() {
    let released = Arc::new(AtomicU32::new(0));
    let requests: Vec<Box<dyn Paused>> = vec![
        Box::new(Fake {
            reads: vec![delete("d", Source::Extract)],
            released: Arc::clone(&released),
            ..Fake::default()
        }),
        Box::new(Fake {
            fixed_reads: vec![submit("a"), submit("a"), submit("b")],
            released: Arc::clone(&released),
            ..Fake::default()
        }),
    ];
    let plan = Plan {
        allow_at: Some(|load| match load {
            1 => vec![AllowedClass::Delete, AllowedClass::Submit],
            2..=4 => vec![AllowedClass::Submit],
            _ => Vec::new(),
        }),
        ..Plan::plain()
    };
    let record = run_many_path(
        Category::CalendarCleanup,
        (vec![form_origin()], Vec::new()),
        requests,
        plan,
    );
    let effects = record.effects();
    let ids: std::collections::BTreeSet<_> = effects.iter().map(|e| e.effect.clone()).collect();
    assert_eq!(
        ids.len(),
        5,
        "the first two, the batch of two, and one new submit: {:?}",
        record.states()
    );
    assert_eq!(
        released.load(Ordering::SeqCst),
        1,
        "the allowed delete leaves"
    );
    assert_eq!(record.done(), Some(Outcome::Failed));
    assert_eq!(
        stopped(&record),
        Some((LogEvent::Stopped, Some(Cause::Changed)))
    );
    let last = effects.last().expect("a row");
    assert_eq!(
        (last.state, last.cause),
        (EffectState::Rejected, Some(Cause::Ended))
    );
}

/// CP-166 E3 (the verifier's change 10): a `cleared` effect whose value changes at every read is proposed again
/// twice and then ended, and the run ends `failed` with `changed`; nothing leaves.
#[test]
fn a_cleared_effect_whose_value_changes_at_every_read_ends_changed_after_two_times() {
    let reads = ["a", "b", "c", "d", "e"]
        .map(|id| delete(id, Source::Extract))
        .to_vec();
    let plan = Plan {
        runs_at: Some(|_| true),
        ..Plan::plain()
    };
    let (record, released) = run(reads, plan);
    assert_eq!(released, 0);
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
