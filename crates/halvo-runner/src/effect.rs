use std::num::NonZeroU32;

use halvo_policy::{digest, effect_step, Decided, Effect, EffectMove};
use halvo_schema::executable::body::{PutEffect, Stored};
use halvo_schema::executable::text::V1;
use halvo_schema::executable::{
    ActionClass, AgentEffect, Bound, Cause, Digest, EffectId, EffectState, KeptField, Outcome,
    RunnerCall, RunnerReply,
};
use halvo_schema::{RequestId, RunnerRequest};

use crate::clock::{day, now, random16};
use crate::run::{cause_of, Runner, Stop};

/// How many times a row of an effect is sent when the store answers it with a fault (`store` or `internal`), each
/// time with the same request id, so that the store answers a repeat of its last row as the first (A7.5). A fault
/// may come after the row was stored, so a row that met a fault and was not stored in the end counts as stored
/// in the direction that tells the person more: a `propose` counts as an effect, a `begin` as `releasing`, for
/// which the runner lets no request leave and which it hands back as `effect_unknown` before its `finish`, and a
/// `commit` as not stored, so that the effect stays `releasing`.
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
    pub(crate) state: EffectState,
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

impl Runner {
    /// Makes an effect of `bound` with the `Decided` that `decide` gave for it, and stores its first row: the
    /// index of the effect, whose state is `refused`, `cleared` or `held`.
    pub(crate) fn propose(&mut self, decided: Decided, bound: &Bound) -> Result<usize, Stop> {
        let class = decided.class();
        let digest = digest(bound).map_err(|_| failed(Cause::StepFailed))?;
        let state = effect_step(None, EffectMove::Propose(decided))
            .map_err(|cause| failed(cause.cause()))?;
        self.effects.push(Tracked {
            id: EffectId::from_random(random16()),
            class,
            digest,
            kept: kept_of(bound),
            fields: bound.fields().len() as u32,
            seq: 0,
            state,
        });
        let at = self.effects.len() - 1;
        match self.store_row(at, state) {
            Ok(()) => Ok(at),
            Err(stop) => {
                if self.effects[at].seq == 0 {
                    self.effects.pop();
                }
                Err(stop)
            }
        }
    }

    /// One move of the effect at `at` through the step relation and, when it takes it, the row that the move
    /// stores: the next state.
    pub(crate) fn advance(&mut self, at: usize, step: EffectMove) -> Result<EffectState, Stop> {
        let e = &self.effects[at];
        let now = Effect {
            id: e.id.clone(),
            state: e.state,
            class: e.class,
            digest: e.digest.clone(),
            rebinds: 0,
        };
        let next = effect_step(Some(&now), step).map_err(|cause| failed(cause.cause()))?;
        self.store_row(at, next)?;
        Ok(next)
    }

    fn store_row(&mut self, at: usize, state: EffectState) -> Result<(), Stop> {
        let e = &self.effects[at];
        let when = now();
        let record = AgentEffect {
            v: V1,
            workflow: self.args.workflow.clone(),
            agent: self.args.agent.clone(),
            run: self.args.run.clone(),
            effect: e.id.clone(),
            seq: NonZeroU32::new(e.seq + 1).unwrap_or(NonZeroU32::MAX),
            day: day(when),
            at: when,
            state,
            class: e.class,
            digest: e.digest.clone(),
            kept: e.kept.clone(),
            fields: e.fields,
            approval: None,
            cause: None,
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
            if faulted && state != EffectState::Committed {
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
