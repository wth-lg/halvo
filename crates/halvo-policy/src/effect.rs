use halvo_schema::executable::names::Cause;
use halvo_schema::executable::{ActionClass, Digest, EffectId, EffectState};

use crate::decide::{Decided, Decision};
use crate::limits::LIMITS;

/// An effect as it stands (A5.2): its id, the state, class and digest of its last row, and the count of its
/// `rebind` rows, which the store's handler computes from the rows in the call that stores the next row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Effect {
    pub id: EffectId,
    pub state: EffectState,
    pub class: ActionClass,
    pub digest: Digest,
    pub rebinds: u32,
}

/// One move of an effect (A5.2). It is not `Clone`, since its `Propose` holds a `Decided`, which is used once.
#[derive(Debug, PartialEq, Eq)]
pub enum EffectMove {
    /// Makes the effect, with the `Decided` that `decide` returned for its class.
    Propose(Decided),
    /// A bound value changed: the digest of the new bound object and its class, after `admit` passed for it.
    Rebind { digest: Digest, class: ActionClass },
    /// The person's approval: the effect id of its entry, the digest that the entry was given for, and whether any
    /// row of any effect ever held that approval.
    Approve {
        entry: EffectId,
        digest: Digest,
        used: bool,
    },
    /// The person's no.
    Reject,
    /// The end of the run before the request left; the row holds the cause `ended`.
    End,
    /// The runner is about to let the request leave: the digest of the bound object computed again.
    Begin { digest: Digest },
    /// The reply to the request came.
    Commit,
    /// The run ended while the effect was `releasing`.
    Lose,
}

/// The cause of a refused move: one of the seven causes of the step relation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepCause {
    Effect,
    Class,
    Changed,
    Approval,
    Digest,
    Used,
    Unknown,
}

impl StepCause {
    /// The cause as the record writes it.
    pub fn cause(self) -> Cause {
        match self {
            StepCause::Effect => Cause::Effect,
            StepCause::Class => Cause::Class,
            StepCause::Changed => Cause::Changed,
            StepCause::Approval => Cause::Approval,
            StepCause::Digest => Cause::Digest,
            StepCause::Used => Cause::Used,
            StepCause::Unknown => Cause::Unknown,
        }
    }
}

/// The step relation of an effect (A5.2, the model's `Step`): the next state for `effect` as it stands, or none
/// before its `propose`, and one move, or the cause of the refusal, the first that fits in the order of A5.2. It
/// takes the move by value, so a `propose` and its `Decided` serve one call.
pub fn effect_step(effect: Option<&Effect>, step: EffectMove) -> Result<EffectState, StepCause> {
    match &step {
        EffectMove::Propose(decided) => propose(effect, decided),
        EffectMove::Rebind { digest, class } => {
            let e = effect.ok_or(StepCause::Effect)?;
            if !matches!(e.state, EffectState::Held | EffectState::Approved) {
                return Err(StepCause::Effect);
            }
            if *class != e.class {
                return Err(StepCause::Class);
            }
            if *digest == e.digest {
                return Err(StepCause::Effect);
            }
            if e.rebinds >= LIMITS.rebinds {
                return Err(StepCause::Changed);
            }
            Ok(EffectState::Held)
        }
        EffectMove::Approve {
            entry,
            digest,
            used,
        } => {
            let e = effect.ok_or(StepCause::Effect)?;
            if e.state != EffectState::Held {
                return Err(StepCause::Effect);
            }
            if *entry != e.id {
                return Err(StepCause::Approval);
            }
            if *digest != e.digest {
                return Err(StepCause::Digest);
            }
            if *used {
                return Err(StepCause::Used);
            }
            Ok(EffectState::Approved)
        }
        EffectMove::Reject => match effect.map(|e| e.state) {
            Some(EffectState::Held) => Ok(EffectState::Rejected),
            _ => Err(StepCause::Effect),
        },
        EffectMove::End => match effect.map(|e| e.state) {
            Some(EffectState::Held | EffectState::Cleared | EffectState::Approved) => {
                Ok(EffectState::Rejected)
            }
            _ => Err(StepCause::Effect),
        },
        EffectMove::Begin { digest } => begin(effect, digest),
        EffectMove::Commit => match effect.map(|e| e.state) {
            Some(EffectState::Releasing) => Ok(EffectState::Committed),
            _ => Err(StepCause::Effect),
        },
        EffectMove::Lose => match effect.map(|e| e.state) {
            Some(EffectState::Releasing) => Ok(EffectState::Unknown),
            _ => Err(StepCause::Effect),
        },
    }
}

fn propose(effect: Option<&Effect>, decided: &Decided) -> Result<EffectState, StepCause> {
    if effect.is_some() {
        return Err(StepCause::Effect);
    }
    if matches!(
        decided.class(),
        ActionClass::Read | ActionClass::Prepare | ActionClass::Credential
    ) {
        return Err(StepCause::Effect);
    }
    match decided.decision() {
        Decision::Refuse => Ok(EffectState::Refused),
        Decision::Run => Ok(EffectState::Cleared),
        Decision::Confirm => Ok(EffectState::Held),
        Decision::Handback => Err(StepCause::Effect),
    }
}

fn begin(effect: Option<&Effect>, digest: &Digest) -> Result<EffectState, StepCause> {
    let e = effect.ok_or(StepCause::Effect)?;
    let needs_approval = matches!(
        e.class,
        ActionClass::Purchase | ActionClass::Payment | ActionClass::Sign
    );
    match e.state {
        EffectState::Refused
        | EffectState::Rejected
        | EffectState::Committed
        | EffectState::Releasing => return Err(StepCause::Effect),
        EffectState::Unknown => return Err(StepCause::Unknown),
        EffectState::Held => return Err(StepCause::Approval),
        EffectState::Cleared if needs_approval => return Err(StepCause::Approval),
        EffectState::Cleared | EffectState::Approved => {}
    }
    if *digest != e.digest {
        return Err(StepCause::Digest);
    }
    Ok(EffectState::Releasing)
}
