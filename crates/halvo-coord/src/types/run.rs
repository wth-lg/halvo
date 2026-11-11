use super::{AgentId, ApprovalId, BoundFrom, Class, Digest, EffectAnswer, EffectId, Name, Origin};
use super::{HandbackReason, Mode, Outcome, RunId, WorkflowId};

/// What `Runs::start` starts (I5.10).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunSpec {
    /// The run.
    pub run: RunId,
    /// The agent.
    pub agent: AgentId,
    /// The agent's workflow.
    pub workflow: WorkflowId,
    /// `compile` or `replay`.
    pub mode: Mode,
    /// Whether the runner's browser is headed.
    pub headed: bool,
}

/// An event of a run, handed to `Coordinator::on_run` (I5.10). `Ended` holds the outcome of
/// A-A6.5 that the runner's last frame gave, or none when the runner was gone with no last
/// frame; the end that the coordinator stores is the end of I5.6, which a stop overrides.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunEvent {
    /// The runner's first frame.
    Hello,
    /// A confirm request.
    Confirm(ConfirmAsk),
    /// A handback request.
    Handback(HandbackAsk),
    /// A progress frame: the step and the count of steps.
    Progress {
        /// `step`.
        step: u32,
        /// `of`.
        of: u32,
    },
    /// A request of a type that A-A6.2 does not list.
    Other,
    /// The runner's end.
    Ended(Option<Outcome>),
}

/// A confirm request of A-A6.3 with the comparison of I4.4 added (I5.10).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfirmAsk {
    /// `run`.
    pub run: RunId,
    /// `agent`.
    pub agent: AgentId,
    /// `effects`, in the request's order.
    pub effects: Vec<AskedEffect>,
}

/// One effect of a confirm request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AskedEffect {
    /// `effect`.
    pub effect: EffectId,
    /// `class`.
    pub class: Class,
    /// `digest`.
    pub digest: Digest,
    /// Whether the digest of the bound object equals `digest` (I4.4).
    pub digest_ok: bool,
    /// The fields of the bound object, in the order of A-A9.3.
    pub fields: Vec<BoundField>,
    /// `new_origin`, or none.
    pub new_origin: Option<Origin>,
}

/// A field of a bound object (A-A9.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoundField {
    /// `name`.
    pub name: Name,
    /// `from`: `const`, `slot`, `extract` or `model`.
    pub from: BoundFrom,
    /// `value`, in pieces.
    pub value: Vec<String>,
}

/// The answer for one effect that the coordinator hands `Runs::answer_confirm`; the port mints
/// the approval id (I4.5, I4.6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answer {
    /// The effect.
    pub effect: EffectId,
    /// The digest of the request for that effect.
    pub digest: Digest,
    /// `allow_once` or `reject_once`.
    pub answer: EffectAnswer,
}

/// A decision of A-A6.3: one entry per effect of the request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decision {
    /// `approvals`.
    pub approvals: Vec<DecisionEntry>,
}

/// One entry of a decision (A-A6.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecisionEntry {
    /// `approval`.
    pub approval: ApprovalId,
    /// `effect`.
    pub effect: EffectId,
    /// `digest`.
    pub digest: Digest,
    /// `answer`.
    pub answer: EffectAnswer,
}

/// A handback request of A-A6.4.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandbackAsk {
    /// `run`.
    pub run: RunId,
    /// `agent`.
    pub agent: AgentId,
    /// `reason`.
    pub reason: HandbackReason,
    /// `origin`, or none.
    pub origin: Option<Origin>,
}
