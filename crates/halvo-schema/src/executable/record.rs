use std::num::NonZeroU32;

use crate::ids::{AgentId, WorkflowId};
use crate::record::Instant;

use super::bound::{check_kept, KeptField};
use super::json::{self, each_once, from_json, list, object_type, opt};
use super::names::{
    ActionClass, Cause, CheckpointState, EffectState, LeftReason, LogEvent, Mode, Outcome,
};
use super::step::ExecutableStep;
use super::text::{
    ApprovalId, Digest, EffectId, Name, OwnValue, RunId, SessionData, WebOrigin, V1,
};
use super::Refusal;

/// The most parts of the stored session of one origin.
pub const SESSION_PARTS_MAX: u32 = 64;

object_type! {
    /// One finished run, kind `agent_run`. `prompts` is the sum of `handbacks` and `confirmations`, `unknown` is at
    /// most `effects`, and a run that is `completed` has no unknown effect. The key `mode` always stands: `compile`,
    /// `replay`, or null in the record that the handler of `end_run` stores, whose outcome is `failed`; a null mode
    /// beside the outcome of a `finish` is refused.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct AgentRun {
        pub v: V1 = "v" => from_json,
        pub workflow: WorkflowId = "workflow" => json::workflow_id,
        pub agent: AgentId = "agent" => json::agent_id,
        pub run: RunId = "run" => from_json,
        pub day: u32 = "day" => from_json,
        pub mode: Option<Mode> = "mode" => opt(from_json),
        pub outcome: Outcome = "outcome" => from_json,
        pub playbook: Option<NonZeroU32> = "playbook" => opt(from_json),
        pub steps: u32 = "steps" => from_json,
        pub prompts: u32 = "prompts" => from_json,
        pub handbacks: u32 = "handbacks" => from_json,
        pub confirmations: u32 = "confirmations" => from_json,
        pub effects: u32 = "effects" => from_json,
        pub unknown: u32 = "unknown" => from_json,
        pub model_calls: u32 = "model_calls" => from_json,
        pub tokens_in: u32 = "tokens_in" => from_json,
        pub tokens_out: u32 = "tokens_out" => from_json,
        pub wall_s: u32 = "wall_s" => from_json,
        pub model: Option<Name> = "model" => opt(from_json),
    }
    rules(r) {
        let fail = |key, rule| Err(Refusal::malformed("AgentRun", rule).at(key));
        if r.mode.is_none() && r.outcome != Outcome::Failed {
            return fail("mode", "mode_null_after_finish");
        }
        if u64::from(r.prompts) != u64::from(r.handbacks) + u64::from(r.confirmations) {
            return fail("prompts", "prompts_sum");
        }
        if r.unknown > r.effects {
            return fail("unknown", "unknown_above_effects");
        }
        if r.outcome == Outcome::Completed && r.unknown > 0 {
            return fail("unknown", "completed_with_unknown");
        }
        Ok(())
    }
}

object_type! {
    /// One step of one playbook, kind `agent_playbook`: the playbook's number, the step's number and the step,
    /// whose `n` is the row's `n`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct AgentPlaybook {
        pub v: V1 = "v" => from_json,
        pub workflow: WorkflowId = "workflow" => json::workflow_id,
        pub agent: AgentId = "agent" => json::agent_id,
        pub number: NonZeroU32 = "number" => from_json,
        pub n: NonZeroU32 = "n" => from_json,
        pub step: ExecutableStep = "step" => ExecutableStep::from_raw,
    }
    rules(p) {
        if p.step.n != p.n {
            return Err(Refusal::malformed("AgentPlaybook", "n_of_step").at("n"));
        }
        Ok(())
    }
}

object_type! {
    /// A slot of the bindings: its name and the value that the user typed, chose or confirmed at activation.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Slot {
        pub name: Name = "name" => from_json,
        pub value: OwnValue = "value" => from_json,
    }
    rules(_s) { Ok(()) }
}

pub(in crate::executable) fn slot_names_once(
    type_name: &'static str,
    slots: &[Slot],
) -> Result<(), Refusal> {
    let names: Vec<&Name> = slots.iter().map(|s| &s.name).collect();
    each_once(type_name, &names).map_err(|r| r.at("slots"))
}

pub(in crate::executable) fn origins_and_slots(
    type_name: &'static str,
    origins: &[WebOrigin],
    slots: &[Slot],
) -> Result<(), Refusal> {
    each_once(type_name, origins).map_err(|r| r.at("origins"))?;
    slot_names_once(type_name, slots)
}

object_type! {
    /// One revision of an agent's bindings, kind `agent_bindings`: its origins, each once, and its slots, each name
    /// once.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct AgentBindings {
        pub v: V1 = "v" => from_json,
        pub workflow: WorkflowId = "workflow" => json::workflow_id,
        pub agent: AgentId = "agent" => json::agent_id,
        pub rev: NonZeroU32 = "rev" => from_json,
        pub origins: Vec<WebOrigin> = "origins" => list(from_json),
        pub slots: Vec<Slot> = "slots" => list(Slot::from_raw),
    }
    rules(b) { origins_and_slots("AgentBindings", &b.origins, &b.slots) }
}

object_type! {
    /// One state of one effect, kind `agent_effect`. Its class makes an effect; `kept` holds the kept fields of the
    /// class in their order, an amount of at most 32 bytes and an address of at most 320; `fields` is the number of
    /// the bound fields of the class. A `held`, `cleared` or `refused` row holds no approval and an `approved` row
    /// one; a row with the cause `ended` is `rejected`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct AgentEffect {
        pub v: V1 = "v" => from_json,
        pub workflow: WorkflowId = "workflow" => json::workflow_id,
        pub agent: AgentId = "agent" => json::agent_id,
        pub run: RunId = "run" => from_json,
        pub effect: EffectId = "effect" => from_json,
        pub seq: NonZeroU32 = "seq" => from_json,
        pub day: u32 = "day" => from_json,
        pub at: Instant = "at" => json::instant,
        pub state: EffectState = "state" => from_json,
        pub class: ActionClass = "class" => from_json,
        pub digest: Digest = "digest" => from_json,
        pub kept: Vec<KeptField> = "kept" => list(KeptField::from_raw),
        pub fields: u32 = "fields" => from_json,
        pub approval: Option<ApprovalId> = "approval" => opt(from_json),
        pub cause: Option<Cause> = "cause" => opt(from_json),
    }
    rules(e) {
        let fail = |key, rule| Err(Refusal::malformed("AgentEffect", rule).at(key));
        let count = e.class.bound_fields().len();
        if count == 0 {
            return fail("class", "class_without_effect");
        }
        check_kept(e.class, &e.kept).map_err(|r| r.at("kept"))?;
        if usize::try_from(e.fields).ok() != Some(count) {
            return fail("fields", "fields_of_class");
        }
        let approval = match e.state {
            EffectState::Held | EffectState::Cleared | EffectState::Refused => Some(false),
            EffectState::Approved => Some(true),
            _ => None,
        };
        if approval.is_some_and(|holds| holds != e.approval.is_some()) {
            return fail("approval", "approval_of_state");
        }
        if e.cause == Some(Cause::Ended) && e.state != EffectState::Rejected {
            return fail("cause", "ended_not_rejected");
        }
        Ok(())
    }
}

object_type! {
    /// One line of a run's log, kind `agent_log`: ids, an event, a step number, a cause, the model token and
    /// figures, and no text.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct AgentLog {
        pub v: V1 = "v" => from_json,
        pub workflow: WorkflowId = "workflow" => json::workflow_id,
        pub agent: AgentId = "agent" => json::agent_id,
        pub run: RunId = "run" => from_json,
        pub seq: NonZeroU32 = "seq" => from_json,
        pub day: u32 = "day" => from_json,
        pub at: Instant = "at" => json::instant,
        pub event: LogEvent = "event" => from_json,
        pub step: Option<NonZeroU32> = "step" => opt(from_json),
        pub cause: Option<Cause> = "cause" => opt(from_json),
        pub model: Option<Name> = "model" => opt(from_json),
        pub ms: Option<u32> = "ms" => opt(from_json),
        pub tokens_in: Option<u32> = "tokens_in" => opt(from_json),
        pub tokens_out: Option<u32> = "tokens_out" => opt(from_json),
    }
    rules(_l) { Ok(()) }
}

pub(in crate::executable) fn part_of(
    type_name: &'static str,
    part: NonZeroU32,
    of: NonZeroU32,
    data: Option<&SessionData>,
) -> Result<(), Refusal> {
    if of.get() > SESSION_PARTS_MAX {
        return Err(Refusal::malformed(type_name, "parts_max").at("of"));
    }
    if part > of {
        return Err(Refusal::malformed(type_name, "part_above_of").at("part"));
    }
    if data.is_some_and(|d| part < of && !d.is_full()) {
        return Err(Refusal::malformed(type_name, "part_not_full").at("data"));
    }
    Ok(())
}

object_type! {
    /// One part of the stored session of one origin, kind `agent_session`: part `part` of `of`, at most 64, whose
    /// data is base64 text in pieces of 1,024 bytes, at most 48; every part but the last holds 48 full pieces.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct AgentSession {
        pub v: V1 = "v" => from_json,
        pub workflow: WorkflowId = "workflow" => json::workflow_id,
        pub agent: AgentId = "agent" => json::agent_id,
        pub origin: WebOrigin = "origin" => from_json,
        pub rev: NonZeroU32 = "rev" => from_json,
        pub part: NonZeroU32 = "part" => from_json,
        pub of: NonZeroU32 = "of" => from_json,
        pub day: u32 = "day" => from_json,
        pub data: SessionData = "data" => from_json,
    }
    rules(s) { part_of("AgentSession", s.part, s.of, Some(&s.data)) }
}

pub(in crate::executable) fn checkpoint_copy(
    type_name: &'static str,
    paths: [(&'static str, &OwnValue); 3],
    state: CheckpointState,
    sha256: Option<&Digest>,
    bytes: Option<u32>,
) -> Result<(), Refusal> {
    let fail = |key, rule| Err(Refusal::malformed(type_name, rule).at(key));
    if let Some((key, _)) = paths.iter().find(|(_, value)| value.is_empty()) {
        return fail(key, "empty_path");
    }
    if sha256.is_some() != bytes.is_some() {
        return fail("bytes", "digest_and_bytes");
    }
    match (state, sha256) {
        (CheckpointState::Planned, Some(_)) => fail("sha256", "planned_with_digest"),
        (CheckpointState::Made, None) => fail("sha256", "made_without_digest"),
        _ => Ok(()),
    }
}

object_type! {
    /// One state of one checkpoint that `prepare` made beside a workbook, kind `agent_checkpoint`: the folder that
    /// `prepare` was given, the workbook's path as given to it and the copy's file name, each a value of at least one
    /// byte in pieces of at most 1,024 bytes, the copy's SHA-256 and length, both or neither, and the state. The row
    /// with `seq` 1 is `planned` and holds no digest, and a `planned` row has `seq` 1; a `made` row has `seq` 2 and a
    /// digest; a `gone` or `left` row has `seq` 2 or 3, since a checkpoint has at most three rows (which of the two
    /// follows which newest row is the store's to hold); `left` names the reason in a `left` row and is null in every
    /// other.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct AgentCheckpoint {
        pub v: V1 = "v" => from_json,
        pub workflow: WorkflowId = "workflow" => json::workflow_id,
        pub agent: AgentId = "agent" => json::agent_id,
        pub run: RunId = "run" => from_json,
        pub seq: NonZeroU32 = "seq" => from_json,
        pub day: u32 = "day" => from_json,
        pub at: Instant = "at" => json::instant,
        pub folder: OwnValue = "folder" => from_json,
        pub workbook: OwnValue = "workbook" => from_json,
        pub copy: OwnValue = "copy" => from_json,
        pub sha256: Option<Digest> = "sha256" => opt(from_json),
        pub bytes: Option<u32> = "bytes" => opt(from_json),
        pub state: CheckpointState = "state" => from_json,
        pub left: Option<LeftReason> = "left" => opt(from_json),
    }
    rules(c) {
        let fail = |key, rule| Err(Refusal::malformed("AgentCheckpoint", rule).at(key));
        let first = c.seq.get() == 1;
        if first != (c.state == CheckpointState::Planned)
            || (c.state == CheckpointState::Made && c.seq.get() != 2)
            || c.seq.get() > 3
        {
            return fail("seq", "seq_of_state");
        }
        checkpoint_copy(
            "AgentCheckpoint",
            [("folder", &c.folder), ("workbook", &c.workbook), ("copy", &c.copy)],
            c.state,
            c.sha256.as_ref(),
            c.bytes,
        )?;
        if c.left.is_some() != (c.state == CheckpointState::Left) {
            return fail("left", "left_of_state");
        }
        Ok(())
    }
}
