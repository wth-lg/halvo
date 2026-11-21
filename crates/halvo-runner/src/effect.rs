use std::num::NonZeroU32;

use halvo_policy::{digest, effect_step, Decided, Effect, EffectMove};
use halvo_schema::executable::body::{PutEffect, Stored};
use halvo_schema::executable::day::day_of;
use halvo_schema::executable::frame::Approval;
use halvo_schema::executable::text::V1;
use halvo_schema::executable::{
    ActionClass, AgentEffect, ApprovalId, Bound, Cause, Digest, EffectId, EffectState, KeptField,
    Outcome, RunnerCall, RunnerReply, WebOrigin,
};
use halvo_schema::{RequestId, RunnerRequest};

use crate::clock::{now, random16};
use crate::run::{cause_of, Runner, Stop};

/// How many times a row of an effect is sent when the store answers it with a fault (`store` or `internal`), each
/// time with the same request id, so that the store answers a repeat of its last row as the first (A7.5). A fault
/// may come after the row was stored, so a row that met a fault and was not stored in the end counts as stored
/// in the direction that tells the person more: a `propose` counts as an effect, a `begin` as `releasing`, for
/// which the runner lets no request leave and which it hands back as `effect_unknown` before its `finish`, and a
/// `commit` or a `not_made` as not stored, so that the effect stays `releasing`.
const ROW_SENDS: u32 = 3;

/// An effect of this run as its last stored row has it.
#[derive(Debug)]
pub(crate) struct Tracked {
    id: EffectId,
    class: ActionClass,
    digest: Digest,
    kept: Vec<KeptField>,
    fields: u32,
    seq: u32,
    rebinds: u32,
    approval: Option<ApprovalId>,
    allowed_origin: Option<WebOrigin>,
    pub(crate) state: EffectState,
}

impl Tracked {
    /// The id of the effect.
    #[cfg_attr(
        not(any(test, feature = "loopback")),
        expect(
            dead_code,
            reason = "the confirm path has no request maker in a build without the feature loopback until the adapters and the browser are linked (CP-142)"
        )
    )]
    pub(crate) fn id(&self) -> &EffectId {
        &self.id
    }

    /// The digest of the effect's last row.
    pub(crate) fn digest(&self) -> &Digest {
        &self.digest
    }

    /// The `new_origin` of the confirmation whose `allow_once` approved the effect last; none before an approval
    /// and after a `rebind`.
    #[cfg_attr(
        not(any(test, feature = "loopback")),
        expect(
            dead_code,
            reason = "the confirm path has no request maker in a build without the feature loopback until the adapters and the browser are linked (CP-142)"
        )
    )]
    pub(crate) fn allowed_origin(&self) -> Option<&WebOrigin> {
        self.allowed_origin.as_ref()
    }

    /// The count of the effect's `rebind` rows.
    #[cfg_attr(
        not(any(test, feature = "loopback")),
        expect(
            dead_code,
            reason = "the confirm path has no request maker in a build without the feature loopback until the adapters and the browser are linked (CP-142)"
        )
    )]
    pub(crate) fn rebinds(&self) -> u32 {
        self.rebinds
    }
}

fn failed(cause: Cause) -> Stop {
    Stop::End(Outcome::Failed, Some(cause))
}

fn kept_of(bound: &Bound) -> Vec<KeptField> {
    let kept = bound.class().kept_fields();
    bound
        .fields()
        .iter()
        .filter(|f| kept.iter().any(|(name, _)| *name == f.name))
        .map(|f| KeptField {
            name: f.name,
            value: f.value.clone(),
        })
        .collect()
}

/// The effect that `bound` makes with the `Decided` that `decide` gave for it, before its first row is stored: the
/// runner's own part of a `propose`.
fn proposal(decided: Decided, bound: &Bound) -> Result<Tracked, Stop> {
    let class = decided.class();
    let digest = digest(bound).map_err(|_| failed(Cause::StepFailed))?;
    let state =
        effect_step(None, EffectMove::Propose(decided)).map_err(|cause| failed(cause.cause()))?;
    Ok(Tracked {
        id: EffectId::from_random(random16()),
        class,
        digest,
        kept: kept_of(bound),
        fields: bound.fields().len() as u32,
        seq: 0,
        rebinds: 0,
        approval: None,
        allowed_origin: None,
        state,
    })
}

impl Runner {
    /// Makes an effect of `bound` with the `Decided` that `decide` gave for it, and stores its first row: the
    /// index of the effect, whose state is `refused`, `cleared` or `held`.
    pub(crate) fn propose(&mut self, decided: Decided, bound: &Bound) -> Result<usize, Stop> {
        let made = proposal(decided, bound)?;
        self.first_row(made)
    }

    /// Ends the `cleared` effect at `ended` with the cause `reproposed` and makes an effect of `bound` in its place,
    /// as `propose` makes one (A5.2, CP-181). The caller has passed every check of its own that can end the run
    /// before the new proposal: the renewal of the values, the bound object, `admit`, the read of a `cancel` and the
    /// wall bound. Here the new effect is made before the `reproposed` row, and a failure of that part ends the old
    /// effect with the cause `ended`, so that nothing but the store's answer to the new `propose` stands between the
    /// two rows.
    pub(crate) fn propose_again(
        &mut self,
        ended: usize,
        decided: Decided,
        bound: &Bound,
    ) -> Result<usize, Stop> {
        let made = proposal(decided, bound);
        let made = self.or_ended(ended, made)?;
        self.end_reproposed(ended)?;
        self.first_row(made)
    }

    /// Stores the first row of the effect `made` and gives its index; an effect whose row the store did not take
    /// and that met no fault is no effect of the run.
    fn first_row(&mut self, made: Tracked) -> Result<usize, Stop> {
        let state = made.state;
        self.effects.push(made);
        let at = self.effects.len() - 1;
        match self.store_row(at, state, None) {
            Ok(()) => Ok(at),
            Err(stop) => {
                if self.effects[at].seq == 0 {
                    self.effects.pop();
                }
                Err(stop)
            }
        }
    }

    /// `checked` when it holds a value; otherwise the move `end` of the effect at `at`, with the cause `ended`, and
    /// then the error of `checked`, unless the row of that move fails first. It ends an effect that the runner was
    /// about to propose again and does not: every exit before the new `propose` leaves the effect `ended`.
    pub(crate) fn or_ended<T, E: From<Stop>>(
        &mut self,
        at: usize,
        checked: Result<T, E>,
    ) -> Result<T, E> {
        match checked {
            Ok(value) => Ok(value),
            Err(error) => {
                self.advance(at, EffectMove::End)?;
                Err(error)
            }
        }
    }

    /// One move of the effect at `at` through the step relation and, when it takes it, the row that the move
    /// stores, with the cause `ended` for `end` and the move's own cause for `not_made`: the next state.
    pub(crate) fn advance(&mut self, at: usize, step: EffectMove) -> Result<EffectState, Stop> {
        let cause = match &step {
            EffectMove::End => Some(Cause::Ended),
            EffectMove::NotMade { cause } => Some(*cause),
            _ => None,
        };
        self.moved(at, step, cause)
    }

    /// The move `end` of the `cleared` effect at `at`, whose row holds the cause `reproposed` and a null approval
    /// (CP-181): the runner proposes the effect again at once, so the store's count of what a person allowed and
    /// Halvo did not do leaves it out. `propose_again` alone writes it.
    fn end_reproposed(&mut self, at: usize) -> Result<EffectState, Stop> {
        self.moved(at, EffectMove::End, Some(Cause::Reproposed))
    }

    /// The move `step` of the effect at `at` and, when the step relation takes it, its row with the cause `cause`.
    fn moved(
        &mut self,
        at: usize,
        step: EffectMove,
        cause: Option<Cause>,
    ) -> Result<EffectState, Stop> {
        let e = &self.effects[at];
        let now = Effect {
            id: e.id.clone(),
            state: e.state,
            class: e.class,
            digest: e.digest.clone(),
            rebinds: e.rebinds,
        };
        let next = effect_step(Some(&now), step).map_err(|cause| failed(cause.cause()))?;
        self.store_row(at, next, cause)?;
        Ok(next)
    }

    /// The move `rebind` of the effect at `at` to the bound object `bound`: its row and every later row carry the
    /// digest of `bound` and its kept fields (A5.2, A7.4, CP-157 E2). The approval is void and the effect holds none
    /// until the person answers again, so a later `rejected` row holds none (CP-162 (5) (d), CP-166 E8).
    pub(crate) fn rebind(&mut self, at: usize, bound: &Bound) -> Result<EffectState, Stop> {
        let new = digest(bound).map_err(|_| failed(Cause::StepFailed))?;
        let e = &self.effects[at];
        let now = Effect {
            id: e.id.clone(),
            state: e.state,
            class: e.class,
            digest: e.digest.clone(),
            rebinds: e.rebinds,
        };
        let step = EffectMove::Rebind {
            digest: new.clone(),
            class: bound.class(),
        };
        let next = effect_step(Some(&now), step).map_err(|cause| failed(cause.cause()))?;
        let e = &mut self.effects[at];
        e.digest = new;
        e.kept = kept_of(bound);
        e.rebinds += 1;
        e.approval = None;
        e.allowed_origin = None;
        self.store_row(at, next, None)?;
        Ok(next)
    }

    /// The move `approve` of the effect at `at` with the person's `allow_once`: its row holds the approval, which
    /// counts as used when any row of this run held it before (A5.2; the store's handler decides `used` from every
    /// row of the family).
    #[cfg_attr(
        not(any(test, feature = "loopback")),
        expect(
            dead_code,
            reason = "the confirm path has no request maker in a build without the feature loopback until the adapters and the browser are linked (CP-142)"
        )
    )]
    pub(crate) fn approve(
        &mut self,
        at: usize,
        approval: &Approval,
        shown: Option<WebOrigin>,
    ) -> Result<(), Stop> {
        let used = self.approvals.contains(&approval.approval);
        let step = EffectMove::Approve {
            entry: approval.effect.clone(),
            digest: approval.digest.clone(),
            used,
        };
        let e = &self.effects[at];
        let now = Effect {
            id: e.id.clone(),
            state: e.state,
            class: e.class,
            digest: e.digest.clone(),
            rebinds: e.rebinds,
        };
        let next = effect_step(Some(&now), step).map_err(|cause| failed(cause.cause()))?;
        self.approvals.push(approval.approval.clone());
        self.effects[at].approval = Some(approval.approval.clone());
        self.effects[at].allowed_origin = shown;
        self.store_row(at, next, None)
    }

    fn store_row(
        &mut self,
        at: usize,
        state: EffectState,
        row_cause: Option<Cause>,
    ) -> Result<(), Stop> {
        let e = &self.effects[at];
        let when = now();
        let record = AgentEffect {
            v: V1,
            workflow: self.args.workflow.clone(),
            agent: self.args.agent.clone(),
            run: self.args.run.clone(),
            effect: e.id.clone(),
            seq: NonZeroU32::new(e.seq + 1).unwrap_or(NonZeroU32::MAX),
            day: day_of(when, self.args.utc_offset_s),
            at: when,
            state,
            class: e.class,
            digest: e.digest.clone(),
            kept: e.kept.clone(),
            fields: e.fields,
            approval: match state {
                EffectState::Held | EffectState::Cleared | EffectState::Refused => None,
                EffectState::Rejected
                    if matches!(row_cause, Some(Cause::Ended | Cause::Reproposed)) =>
                {
                    None
                }
                _ => e.approval.clone(),
            },
            cause: row_cause,
        };
        let call = RunnerCall::Effect(PutEffect {
            agent: self.args.agent.clone(),
            run: self.args.run.clone(),
            record,
        });
        let request = RunnerRequest::new(self.role(), RequestId::from_random(random16()), call);
        let mut sends = 0;
        let mut faulted = false;
        let cause = loop {
            sends += 1;
            match self.send_store(&request)? {
                Ok(RunnerReply::Stored(Stored { stored: 1 })) => break None,
                Ok(_) => break Some(Cause::Effect),
                Err(error) => {
                    let fault = matches!(error.as_str(), "store" | "internal");
                    faulted |= fault;
                    if !fault || sends >= ROW_SENDS {
                        break Some(cause_of(error));
                    }
                }
            }
        };
        if let Some(cause) = cause {
            if faulted && !matches!(state, EffectState::Committed | EffectState::NotMade) {
                let e = &mut self.effects[at];
                e.seq += 1;
                e.state = state;
            }
            return Err(failed(cause));
        }
        let e = &mut self.effects[at];
        e.seq += 1;
        e.state = state;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use halvo_schema::executable::names::{FieldName, Source};
    use halvo_schema::executable::text::OwnValue;
    use halvo_schema::executable::BoundField;

    fn field(name: FieldName, value: &str) -> BoundField {
        BoundField {
            name,
            from: Source::Slot,
            origin: None,
            value: OwnValue::new(value),
        }
    }

    #[test]
    fn a_row_keeps_the_kept_fields_of_its_class_alone() {
        let edit = Bound::new(
            ActionClass::LocalEdit,
            vec![
                field(FieldName::File, "/f.xlsx"),
                field(FieldName::Range, "B2:D6"),
            ],
        )
        .expect("a bound object");
        let names: Vec<FieldName> = kept_of(&edit).iter().map(|k| k.name).collect();
        assert_eq!(names, [FieldName::File, FieldName::Range]);
        let send = Bound::new(
            ActionClass::Send,
            vec![
                field(FieldName::To, "a@b.test"),
                field(FieldName::Subject, "s"),
                field(FieldName::Body, "b"),
            ],
        )
        .expect("a bound object");
        let names: Vec<FieldName> = kept_of(&send).iter().map(|k| k.name).collect();
        assert_eq!(names, [FieldName::To]);
    }
}
