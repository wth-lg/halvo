use crate::{is_rejected, Locator, EDITS_MAX};

tokens! {
    /// The outcome of a run, the `outcome` of its run record (AG-A6.5).
    pub enum Outcome {
        Completed = "completed",
        NothingToDo = "nothing_to_do",
        Declined = "declined",
        HandedBack = "handed_back",
        ClassMismatch = "class_mismatch",
        Refused = "refused",
        Limit = "limit",
        EffectUnknown = "effect_unknown",
        Failed = "failed",
    }
}

tokens! {
    /// The run causes, the closed list of `learn_run`'s `cause` (L3.4): the outcomes other
    /// than `completed`, and `unknown`.
    pub enum RunCause {
        NothingToDo = "nothing_to_do",
        Declined = "declined",
        HandedBack = "handed_back",
        ClassMismatch = "class_mismatch",
        Refused = "refused",
        Limit = "limit",
        EffectUnknown = "effect_unknown",
        Failed = "failed",
        Unknown = "unknown",
    }
}

tokens! {
    /// The cause of a line of a run's log, the one closed list of AG-A7.4.
    pub enum LogCause {
        Approval = "approval",
        Digest = "digest",
        Used = "used",
        Unknown = "unknown",
        Autonomy = "autonomy",
        Checkpoint = "checkpoint",
        Class = "class",
        ClassMismatch = "class_mismatch",
        Origin = "origin",
        Provenance = "provenance",
        Declined = "declined",
        Timeout = "timeout",
        SessionSize = "session_size",
        StepFailed = "step_failed",
        Steps = "steps",
        Wall = "wall",
        Tokens = "tokens",
        Sheet = "sheet",
        Workbook = "workbook",
        Adapter = "adapter",
        Effect = "effect",
        Ended = "ended",
        Changed = "changed",
    }
}

tokens! {
    /// The edit causes, the closed list of `learn_edit`'s `cause` (L3.5): the counted names.
    pub enum EditCause {
        StepFailed = "step_failed",
        ClassMismatch = "class_mismatch",
        Origin = "origin",
    }
}

/// What [`verify_run`] reads of a run record (L3.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunFacts {
    /// The run's outcome.
    pub outcome: Outcome,
    /// The run's count of effects in the state `unknown`.
    pub unknown: u32,
}

/// `Ok` when the run is verified: its outcome is `completed` and `unknown` is 0. Otherwise the
/// run cause: the outcome's own name, or `unknown` for `completed` with `unknown` above 0.
pub fn verify_run(facts: &RunFacts) -> Result<(), RunCause> {
    let cause = match facts.outcome {
        Outcome::Completed if facts.unknown == 0 => return Ok(()),
        Outcome::Completed => RunCause::Unknown,
        Outcome::NothingToDo => RunCause::NothingToDo,
        Outcome::Declined => RunCause::Declined,
        Outcome::HandedBack => RunCause::HandedBack,
        Outcome::ClassMismatch => RunCause::ClassMismatch,
        Outcome::Refused => RunCause::Refused,
        Outcome::Limit => RunCause::Limit,
        Outcome::EffectUnknown => RunCause::EffectUnknown,
        Outcome::Failed => RunCause::Failed,
    };
    Err(cause)
}

/// Whether a `step_failed` line with this cause counts against the `provisional` edit that the
/// run was served on its step (L3.5): yes, with the edit cause that a `reverted` row stores,
/// for `step_failed`, `class_mismatch` and `origin`; no for every other cause and for a line
/// with no cause.
pub fn counted(cause: Option<LogCause>) -> Option<EditCause> {
    match cause? {
        LogCause::StepFailed => Some(EditCause::StepFailed),
        LogCause::ClassMismatch => Some(EditCause::ClassMismatch),
        LogCause::Origin => Some(EditCause::Origin),
        LogCause::Approval
        | LogCause::Digest
        | LogCause::Used
        | LogCause::Unknown
        | LogCause::Autonomy
        | LogCause::Checkpoint
        | LogCause::Class
        | LogCause::Provenance
        | LogCause::Declined
        | LogCause::Timeout
        | LogCause::SessionSize
        | LogCause::Steps
        | LogCause::Wall
        | LogCause::Tokens
        | LogCause::Sheet
        | LogCause::Workbook
        | LogCause::Adapter
        | LogCause::Effect
        | LogCause::Ended
        | LogCause::Changed => None,
    }
}

/// What [`candidate_move`] reads at a run's `finish` (L3.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CandidateFacts {
    /// The run is verified ([`verify_run`]).
    pub verified: bool,
    /// The run's log holds a `recovered` line for the candidate's step with no `step_failed`
    /// line for that step after it.
    pub recovered: bool,
    /// An edit of the step and compile number is `provisional` once the moves of this run
    /// are made.
    pub pending: bool,
    /// The agent's compile number at `finish` is still the one that the run loaded.
    pub current: bool,
    /// The edits of the step and compile number as the rows stand at `finish`.
    pub edits: u32,
    /// The candidate's locators, those that were new when it was admitted.
    pub locators: Vec<Locator>,
    /// The list of the base edit, empty when there is none.
    pub base_list: Vec<Locator>,
    /// What the `reverted` rows of the same agent, `base` and `n` brought, as the rows stand
    /// at `finish`.
    pub barred: Vec<Locator>,
}

/// The answer of [`candidate_move`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CandidateMove {
    /// The candidate is kept with the locators that are left, in their order, which the
    /// caller hands to `build_list` with the base edit's list.
    Keep(Vec<Locator>),
    /// The candidate is dropped and never stored.
    Drop,
}

/// Keeps a candidate only when the run is verified, recovered the step with no failure of it
/// after, no edit of the step is `provisional`, the compile number is current, at least one of
/// its locators is left when those of the base edit are taken away, none of those left is
/// barred, and the step has fewer than [`EDITS_MAX`] edits (L3.4).
pub fn candidate_move(facts: &CandidateFacts) -> CandidateMove {
    if !facts.verified
        || !facts.recovered
        || facts.pending
        || !facts.current
        || facts.edits >= EDITS_MAX
    {
        return CandidateMove::Drop;
    }
    let left: Vec<Locator> = facts
        .locators
        .iter()
        .filter(|locator| !facts.base_list.contains(locator))
        .cloned()
        .collect();
    if left.is_empty() || is_rejected(&left, &facts.barred) {
        return CandidateMove::Drop;
    }
    CandidateMove::Keep(left)
}
