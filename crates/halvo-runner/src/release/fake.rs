//! Tests of the confirm path with a fake paused request that reads its bound values from a list, one per read, and
//! counts what it lets leave; no fixture runs.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use halvo_policy::Facts;
use halvo_schema::executable::names::{Answer, FieldName, HandbackReason, LogEvent, Source};
use halvo_schema::executable::text::OwnValue;
use halvo_schema::executable::{ActionClass, Bound, BoundField, Cause, EffectState, Outcome};
use halvo_schema::Category;

use super::suite::{run_path, Plan, Record};
use super::{Left, Paused, Replied};

struct Fake {
    reads: Vec<Bound>,
    at: usize,
    released: Arc<AtomicU32>,
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

    fn bound(&mut self) -> Result<Bound, Cause> {
        let read = self.reads[self.at.min(self.reads.len() - 1)].clone();
        self.at += 1;
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
        Box::new(Answers)
    }

    fn renew(self: Box<Self>) -> Result<Box<dyn Paused>, Cause> {
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
