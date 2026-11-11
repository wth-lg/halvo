use super::request_presenter;
use crate::types::{
    Answer, Cause, Choice, ConfirmAsk, Decision, Digest, EffectAnswer, EffectId, LapseCause,
    Presenter,
};

/// The person's answer to a confirmation, the safe one first (I3.1). One answer covers the
/// whole request (I4.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmAnswer {
    /// `reject`: `reject_once` for every effect.
    Reject,
    /// `allow`: `allow_once` for every effect.
    Allow,
}

impl ConfirmAnswer {
    /// The answer of the choice `reject` or `allow`; `malformed` for any other choice.
    pub fn from_choice(choice: Choice) -> Result<ConfirmAnswer, Cause> {
        match choice {
            Choice::Reject => Ok(ConfirmAnswer::Reject),
            Choice::Allow => Ok(ConfirmAnswer::Allow),
            _ => Err(Cause::Malformed),
        }
    }

    /// The answer of a decision for every effect: `allow_once` or `reject_once` (A-A6.3).
    pub const fn once(self) -> EffectAnswer {
        match self {
            ConfirmAnswer::Reject => EffectAnswer::RejectOnce,
            ConfirmAnswer::Allow => EffectAnswer::AllowOnce,
        }
    }
}

/// A move of the table of I4.2.
#[derive(Clone, Copy, Debug)]
pub enum ConfirmMove<'a> {
    /// `issue` with a confirm request, whose `digest_ok` of each effect is the comparison of
    /// I4.4.
    Issue(&'a ConfirmAsk),
    /// `arm`, made `arm_ms` after the presenter reported the item on screen.
    Arm,
    /// `answer` from a presenter: `page` for `agents_answer`, `native` for the dialog's result.
    Answer {
        /// `allow` or `reject`.
        answer: ConfirmAnswer,
        /// Where the answer came from.
        from: Presenter,
    },
    /// `record`: the `item_answered` rows of this decision are acknowledged; the port minted
    /// its approval ids (I4.6).
    Record(&'a Decision),
    /// `release`: `Runs::answer_confirm` returned success, so the decision went to the runner.
    Release,
    /// `lapse` with a cause.
    Lapse(LapseCause),
}

/// The state of a confirmation item (I4.2) with what it holds: the answer from `answered`, the
/// decision from `recorded`, and the cause of a lapse. `released` and `lapsed` are final.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfirmState {
    /// `shown`.
    Shown,
    /// `armed`.
    Armed,
    /// `answered`.
    Answered(ConfirmAnswer),
    /// `recorded`, with the decision whose rows were acknowledged.
    Recorded(Decision),
    /// `released`, with the decision that went to the runner.
    Released(Decision),
    /// `lapsed`, with its cause.
    Lapsed(LapseCause),
}

impl ConfirmState {
    /// Whether the item is open: neither `released` nor `lapsed`.
    pub fn is_open(&self) -> bool {
        !matches!(self, ConfirmState::Released(_) | ConfirmState::Lapsed(_))
    }

    /// Whether the item waits for the person: `shown` or `armed`, which `needs_you` counts.
    pub fn waits_for_person(&self) -> bool {
        matches!(self, ConfirmState::Shown | ConfirmState::Armed)
    }
}

/// A confirmation item: the effects of its request with their digests, its presenter and its
/// state. It is made only by `confirm_step`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Confirmation {
    asked: Vec<(EffectId, Digest)>,
    presenter: Presenter,
    state: ConfirmState,
}

impl Confirmation {
    /// The effects of the request with their digests, in the request's order.
    pub fn asked(&self) -> &[(EffectId, Digest)] {
        &self.asked
    }

    /// The presenter of the request (I4.1).
    pub fn presenter(&self) -> Presenter {
        self.presenter
    }

    /// The state.
    pub fn state(&self) -> &ConfirmState {
        &self.state
    }

    /// The answers that `Runs::answer_confirm` takes for `answer`: one per effect of the
    /// request, with its digest.
    pub fn answers(&self, answer: ConfirmAnswer) -> Vec<Answer> {
        self.asked
            .iter()
            .map(|(effect, digest)| Answer {
                effect: effect.clone(),
                digest: digest.clone(),
                answer: answer.once(),
            })
            .collect()
    }

    fn to(&self, state: ConfirmState) -> Confirmation {
        Confirmation {
            state,
            ..self.clone()
        }
    }

    fn covers(&self, decision: &Decision, answer: ConfirmAnswer) -> bool {
        decision.approvals().len() == self.asked.len()
            && decision.approvals().iter().all(|entry| {
                entry.answer == answer.once()
                    && self
                        .asked
                        .iter()
                        .any(|(effect, digest)| *effect == entry.effect && *digest == entry.digest)
            })
    }
}

/// One move of the table of I4.2 on `item`, none before `issue`: the item after the move, or
/// the cause of its refusal. An `answer`, first that fits: on an item that is `answered`,
/// `recorded`, `released` or `lapsed`, `stale`; from a source that is not the item's
/// presenter, `presenter`; on an item that is `shown`, `early`. A `record` takes only a
/// decision with one entry per effect of the request, with its digest and the answer of the
/// item. A lapse takes any of the six causes from `shown` or `armed`, `run_ended`,
/// `cancelled`, `no_record` or `app_ended` from `answered`, and `run_ended` or `app_ended`
/// from `recorded`. Every other pair gives `state`.
pub fn confirm_step(
    item: Option<&Confirmation>,
    mv: ConfirmMove<'_>,
) -> Result<Confirmation, Cause> {
    use ConfirmState::*;
    use LapseCause::*;
    let Some(item) = item else {
        let ConfirmMove::Issue(ask) = mv else {
            return Err(Cause::State);
        };
        let holds = ask.effects().iter().all(|e| e.digest_ok);
        return Ok(Confirmation {
            asked: ask
                .effects()
                .iter()
                .map(|e| (e.effect.clone(), e.digest.clone()))
                .collect(),
            presenter: request_presenter(ask),
            state: if holds { Shown } else { Lapsed(Digest) },
        });
    };
    match (&item.state, mv) {
        (Shown, ConfirmMove::Arm) => Ok(item.to(Armed)),
        (state, ConfirmMove::Answer { answer, from }) => match state {
            Answered(_) | Recorded(_) | Released(_) | Lapsed(_) => Err(Cause::Stale),
            _ if from != item.presenter => Err(Cause::Presenter),
            Shown => Err(Cause::Early),
            Armed => Ok(item.to(Answered(answer))),
        },
        (Answered(answer), ConfirmMove::Record(decision)) if item.covers(decision, *answer) => {
            Ok(item.to(Recorded(decision.clone())))
        }
        (Recorded(decision), ConfirmMove::Release) => Ok(item.to(Released(decision.clone()))),
        (Shown | Armed, ConfirmMove::Lapse(cause))
        | (Answered(_), ConfirmMove::Lapse(cause @ (RunEnded | Cancelled | NoRecord | AppEnded)))
        | (Recorded(_), ConfirmMove::Lapse(cause @ (RunEnded | AppEnded))) => {
            Ok(item.to(Lapsed(cause)))
        }
        _ => Err(Cause::State),
    }
}
