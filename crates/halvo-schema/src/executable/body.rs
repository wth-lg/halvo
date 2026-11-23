use std::num::NonZeroU32;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::value::RawValue;

use crate::ids::{AgentId, WorkflowId};
use crate::record::Instant;
use crate::request::{CoordinatorBody, RunnerBody, REPLY_MAX};

use super::bound::{check_kept, KeptField};
use super::config::AgentConfig;
use super::frame::FRAME_JSON_MAX;
use super::json::{self, each_once, from_json, list, object_type, opt};
use super::names::{
    ActionClass, AgentState, Cause, CheckpointState, EffectState, LeftReason, ListedState,
    MarkState, Mode, ModelTier, ReplyState, RoomState, Way, YesNo,
};
use super::record::{
    checkpoint_copy, hosts_up_to, origins_and_slots, part_of, slot_names_once, AgentCheckpoint,
    AgentEffect, AgentLog, AgentReply, AgentRun, AgentStart, Hosted, Slot, SESSION_PARTS_MAX,
};
use super::step::{ExecutableStep, PLAYBOOK_STEPS_MAX};
use super::text::{
    Digest, EffectId, ModelRequestId, Name, Null, OwnText, OwnValue, ProviderId, RunId, SecretText,
    SessionData, WebOrigin, OWN_TEXT_MAX,
};
use super::{is_sender, same_address, Refusal};

/// The most steps of one `put_playbook` body and of one `load_steps` reply.
pub const STEPS_PER_REQUEST_MAX: usize = 16;

/// The most bytes of the reply of `list_checkpoints` and of every reply of a runner or coordinator body, as they
/// arrive: the bound of a reply less the 53 bytes of its envelope beside a request id.
pub const CHECKPOINTS_REPLY_MAX: usize = REPLY_MAX - 53;

/// The most bytes of the value of a runner reply that one `store_reply` frame carries beside a request id: the bound
/// of a frame less the 28 bytes of the frame's own text and the 53 of the reply's envelope. The reply of
/// `load_replies` holds it as its bytes arrive and at its writer.
pub const RUNNER_REPLY_VALUE_MAX: usize = FRAME_JSON_MAX - 28 - 53;

/// The most message ids of one `load_replies` body, those of one page of the run's list.
pub const REPLY_IDS_MAX: usize = 25;

/// The most entries of one reply of `list_overlaps`.
pub const OVERLAPS_MAX: usize = 16;

macro_rules! op_enum {
    (
        $(#[$meta:meta])*
        pub enum $name:ident { $( $variant:ident($ty:ident) = $op:literal ),+ $(,)? }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub enum $name {
            $( $variant($ty), )+
        }

        impl $name {
            /// The name under the key `op`.
            pub fn op(&self) -> &'static str {
                match self {
                    $( $name::$variant(_) => $op, )+
                }
            }

            pub(in crate::executable) fn from_raw(raw: &RawValue) -> Result<Self, Refusal> {
                let obj = json::object(stringify!($name), raw)?;
                let op = obj.read("op", |raw| json::string(stringify!($name), raw))?;
                match op.as_str() {
                    $( $op => $ty::from_raw(raw).map($name::$variant), )+
                    _ => Err(Refusal::malformed(stringify!($name), "unknown_op").at("op")),
                }
            }

            /// Reads one body and refuses every other input.
            pub fn parse(bytes: &[u8]) -> Result<Self, Refusal> {
                Self::from_raw(json::whole(stringify!($name), bytes)?)
            }

            /// The compact JSON of the body, or the refusal of a rule it breaks.
            pub fn to_json(&self) -> Result<String, Refusal> {
                match self {
                    $( $name::$variant(b) => b.to_json(), )+
                }
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                match self {
                    $( $name::$variant(b) => b.serialize(serializer), )+
                }
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let raw = json::deserialize_raw(stringify!($name), deserializer)?;
                Self::from_raw(&raw).map_err(<D::Error as serde::de::Error>::custom)
            }
        }
    };
}

macro_rules! reply_enum {
    (
        $(#[$meta:meta])*
        pub enum $name:ident { $( $variant:ident($ty:ident) = $key:literal ),+ $(,)? }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq, Eq)]
        #[allow(clippy::large_enum_variant)]
        pub enum $name {
            $( $variant($ty), )+
        }

        const _: () = {
            $( assert!(<$ty as json::Arrives>::MAX <= CHECKPOINTS_REPLY_MAX); )+
        };

        impl $name {
            pub(in crate::executable) fn from_raw(raw: &RawValue) -> Result<Self, Refusal> {
                Self::read(raw, raw.get().len())
            }

            fn read(raw: &RawValue, given: usize) -> Result<Self, Refusal> {
                reply_bytes(stringify!($name), given)?;
                let obj = json::object(stringify!($name), raw)?;
                $(
                    if obj.has($key) {
                        if given > <$ty as json::Arrives>::MAX {
                            return Err(Refusal::length(stringify!($name), "reply_bytes"));
                        }
                        return $ty::from_raw(raw).map($name::$variant);
                    }
                )+
                Err(Refusal::malformed(stringify!($name), "reply_form"))
            }

            /// Reads one reply as it arrives and refuses every other input. An input of more than
            /// `CHECKPOINTS_REPLY_MAX` bytes, counted on the bytes given before anything is read, is refused with the
            /// cause `length` and the rule `reply_bytes`; so is an input of more bytes than the bound of the reply
            /// type that its keys name, once they name it.
            pub fn parse(bytes: &[u8]) -> Result<Self, Refusal> {
                reply_bytes(stringify!($name), bytes.len())?;
                Self::read(json::whole(stringify!($name), bytes)?, bytes.len())
            }

            /// The compact JSON of the reply.
            pub fn to_json(&self) -> Result<String, Refusal> {
                match self {
                    $( $name::$variant(r) => r.to_json(), )+
                }
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                match self {
                    $( $name::$variant(r) => r.serialize(serializer), )+
                }
            }
        }

        /// The serde reader counts the text of the value that serde gives it, white space and escapes inside the
        /// value included, against `CHECKPOINTS_REPLY_MAX` and the bound of the reply type that its keys name. Serde
        /// gives it no byte outside the value: those bytes are counted by `parse` or by the reader of the text around
        /// the value.
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let raw = json::deserialize_raw(stringify!($name), deserializer)?;
                Self::from_raw(&raw).map_err(<D::Error as serde::de::Error>::custom)
            }
        }
    };
}

/// A count of the person's model requests and of their count (r7 8.1L), a JSON integer of digits alone from 0 to
/// `u64::MAX`.
fn count64(raw: &RawValue) -> Result<u64, Refusal> {
    let text = raw.get();
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(Refusal::malformed("count", "not_a_count"));
    }
    text.parse::<u64>()
        .map_err(|_| Refusal::malformed("count", "range"))
}

fn reply_bytes(type_name: &'static str, len: usize) -> Result<(), Refusal> {
    if len > CHECKPOINTS_REPLY_MAX {
        return Err(Refusal::length(type_name, "reply_bytes"));
    }
    Ok(())
}

fn same_run(
    type_name: &'static str,
    body: (&AgentId, &RunId),
    record: (&AgentId, &RunId),
) -> Result<(), Refusal> {
    if body != record {
        return Err(Refusal::malformed(type_name, "record_of_another_run").at("record"));
    }
    Ok(())
}

fn steps_in_order(type_name: &'static str, steps: &[ExecutableStep]) -> Result<(), Refusal> {
    if steps.len() > STEPS_PER_REQUEST_MAX {
        return Err(Refusal::malformed(type_name, "steps_per_request").at("steps"));
    }
    if steps
        .windows(2)
        .any(|w| w[1].n.get() != w[0].n.get().wrapping_add(1))
    {
        return Err(Refusal::malformed(type_name, "number_order").at("steps"));
    }
    Ok(())
}

object_type! {
    /// `{"op":"load","agent":..,"run":..}`: the newest configuration, playbook, bindings and sessions.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Load [op = "load"] {
        pub agent: AgentId = "agent" => json::agent_id,
        pub run: RunId = "run" => from_json,
    }
    rules(_b) { Ok(()) }
}

object_type! {
    /// `load_steps`: at most 16 steps of playbook `number` from the step numbered `from`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct LoadSteps [op = "load_steps"] {
        pub agent: AgentId = "agent" => json::agent_id,
        pub run: RunId = "run" => from_json,
        pub number: NonZeroU32 = "number" => from_json,
        pub from: NonZeroU32 = "from" => from_json,
    }
    rules(b) {
        if usize::try_from(b.from.get()).map_or(true, |n| n > PLAYBOOK_STEPS_MAX) {
            return Err(Refusal::length("LoadSteps", "step_number").at("from"));
        }
        Ok(())
    }
}

object_type! {
    /// `load_session`: one part of a stored session revision of one origin.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct LoadSession [op = "load_session"] {
        pub agent: AgentId = "agent" => json::agent_id,
        pub run: RunId = "run" => from_json,
        pub origin: WebOrigin = "origin" => from_json,
        pub rev: NonZeroU32 = "rev" => from_json,
        pub part: NonZeroU32 = "part" => from_json,
    }
    rules(b) {
        if b.part.get() > SESSION_PARTS_MAX {
            return Err(Refusal::malformed("LoadSession", "parts_max").at("part"));
        }
        Ok(())
    }
}

object_type! {
    /// `put_playbook`: 1 to 16 steps of playbook `number`, numbered in order.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct PutPlaybook [op = "put_playbook"] {
        pub agent: AgentId = "agent" => json::agent_id,
        pub run: RunId = "run" => from_json,
        pub number: NonZeroU32 = "number" => from_json,
        pub steps: Vec<ExecutableStep> = "steps" => list(ExecutableStep::from_raw),
    }
    rules(b) {
        if b.steps.is_empty() {
            return Err(Refusal::malformed("PutPlaybook", "no_step").at("steps"));
        }
        steps_in_order("PutPlaybook", &b.steps)
    }
}

object_type! {
    /// `put_session`: one part of a new session revision of one origin.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct PutSession [op = "put_session"] {
        pub agent: AgentId = "agent" => json::agent_id,
        pub run: RunId = "run" => from_json,
        pub origin: WebOrigin = "origin" => from_json,
        pub rev: NonZeroU32 = "rev" => from_json,
        pub part: NonZeroU32 = "part" => from_json,
        pub of: NonZeroU32 = "of" => from_json,
        pub data: SessionData = "data" => from_json,
    }
    rules(b) { part_of("PutSession", b.part, b.of, Some(&b.data)) }
}

object_type! {
    /// `drop_session`: the end of the use of the kept sign-in of one origin. The store keeps one more revision of
    /// that origin, of one part in the drop form (`SessionData::dropped`), which `load` lists as that origin's newest
    /// revision and from which a runner imports nothing.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct DropSession [op = "drop_session"] {
        pub agent: AgentId = "agent" => json::agent_id,
        pub run: RunId = "run" => from_json,
        pub origin: WebOrigin = "origin" => from_json,
    }
    rules(_b) { Ok(()) }
}

object_type! {
    /// `effect`: one row of an effect of the body's agent and run.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct PutEffect [op = "effect"] {
        pub agent: AgentId = "agent" => json::agent_id,
        pub run: RunId = "run" => from_json,
        pub record: AgentEffect = "record" => AgentEffect::from_raw,
    }
    rules(b) { same_run("PutEffect", (&b.agent, &b.run), (&b.record.agent, &b.record.run)) }
}

object_type! {
    /// `log`: one or more lines of the log of the body's agent and run.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct PutLog [op = "log"] {
        pub agent: AgentId = "agent" => json::agent_id,
        pub run: RunId = "run" => from_json,
        pub records: Vec<AgentLog> = "records" => list(AgentLog::from_raw),
    }
    rules(b) {
        if b.records.is_empty() {
            return Err(Refusal::malformed("PutLog", "no_record").at("records"));
        }
        b.records
            .iter()
            .try_for_each(|r| same_run("PutLog", (&b.agent, &b.run), (&r.agent, &r.run)))
    }
}

object_type! {
    /// `finish`: the record of the body's run.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Finish [op = "finish"] {
        pub agent: AgentId = "agent" => json::agent_id,
        pub run: RunId = "run" => from_json,
        pub record: AgentRun = "record" => AgentRun::from_raw,
    }
    rules(b) { same_run("Finish", (&b.agent, &b.run), (&b.record.agent, &b.record.run)) }
}

object_type! {
    /// `checkpoint`: one row of the runner's checkpoint of a workbook, `planned` before `prepare` or `made` once the
    /// copy is made; the rows `gone` and `left` are the supervisor library's, through `mark_checkpoint`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct PutCheckpoint [op = "checkpoint"] {
        pub agent: AgentId = "agent" => json::agent_id,
        pub run: RunId = "run" => from_json,
        pub record: AgentCheckpoint = "record" => AgentCheckpoint::from_raw,
    }
    rules(b) {
        if !matches!(b.record.state, CheckpointState::Planned | CheckpointState::Made) {
            return Err(Refusal::malformed("PutCheckpoint", "state_of_runner").at("record"));
        }
        same_run("PutCheckpoint", (&b.agent, &b.run), (&b.record.agent, &b.record.run))
    }
}

object_type! {
    /// `reply_row`: one row of the reply to one message, of the body's agent and run. Which row may follow which is
    /// the handler's to hold.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct PutReplyRow [op = "reply_row"] {
        pub agent: AgentId = "agent" => json::agent_id,
        pub run: RunId = "run" => from_json,
        pub record: AgentReply = "record" => AgentReply::from_raw,
    }
    rules(b) { same_run("PutReplyRow", (&b.agent, &b.run), (&b.record.agent, &b.record.run)) }
}

object_type! {
    /// `load_replies`: `message_ids` holds the provider's ids of 1 to 25 mail messages, each once, those of one page of
    /// the run's list, whose rows the run reads, and never a message's text; each id is the value that `agent_reply`
    /// holds under `message`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct LoadReplies [op = "load_replies"] {
        pub agent: AgentId = "agent" => json::agent_id,
        pub run: RunId = "run" => from_json,
        pub message_ids: Vec<ProviderId> = "message_ids" => list(from_json),
    }
    rules(b) {
        let fail = |rule| Err(Refusal::malformed("LoadReplies", rule).at("message_ids"));
        if b.message_ids.is_empty() {
            return fail("no_message");
        }
        if b.message_ids.len() > REPLY_IDS_MAX {
            return fail("messages_max");
        }
        each_once("LoadReplies", &b.message_ids).map_err(|e| e.at("message_ids"))
    }
}

object_type! {
    /// `model_hold`: the hold of the cost of one model request against the person's count, before the request goes to
    /// the model's host (r7 8.1L): the id that the runner minted for the request, the hosted tier, the bytes of the
    /// request's body and its limit of output tokens, which the store prices by the build's prices. It carries no
    /// figure of the count: no amount, no cap, nothing spent or held and no end of a period or of a hold's life. The
    /// agent and the run name the run for the store's checks of a runner body alone; the count's row stores neither.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct ModelHold [op = "model_hold"] {
        pub agent: AgentId = "agent" => json::agent_id,
        pub run: RunId = "run" => from_json,
        pub request: ModelRequestId = "request" => from_json,
        pub tier: ModelTier = "tier" => from_json,
        pub body_bytes: u64 = "body_bytes" => count64,
        pub output_limit: u64 = "output_limit" => count64,
    }
    rules(b) {
        if b.tier == ModelTier::OnDevice {
            return Err(Refusal::malformed("ModelHold", "hosted_tier").at("tier"));
        }
        Ok(())
    }
}

object_type! {
    /// The usage that the host's answer reported for one model request: the tokens in and the tokens out.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct ModelUsage {
        pub tokens_in: u64 = "tokens_in" => count64,
        pub tokens_out: u64 = "tokens_out" => count64,
    }
    rules(_u) { Ok(()) }
}

object_type! {
    /// `model_settle`: the settle of the hold of one model request (r7 8.1L): the request's id and the usage that the
    /// host's answer reported, or null under `usage` for a request of which nothing was sent. It carries no figure of
    /// the count.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct ModelSettle [op = "model_settle"] {
        pub agent: AgentId = "agent" => json::agent_id,
        pub run: RunId = "run" => from_json,
        pub request: ModelRequestId = "request" => from_json,
        pub usage: Option<ModelUsage> = "usage" => opt(ModelUsage::from_raw),
    }
    rules(_b) { Ok(()) }
}

op_enum! {
    /// The runner bodies (`RunnerCall`): each holds `op` first, then `agent` and `run`, then its keys. A body that
    /// holds a record refuses a record of another agent or run.
    pub enum RunnerCall {
        Load(Load) = "load",
        LoadSteps(LoadSteps) = "load_steps",
        LoadSession(LoadSession) = "load_session",
        PutPlaybook(PutPlaybook) = "put_playbook",
        PutSession(PutSession) = "put_session",
        Effect(PutEffect) = "effect",
        Log(PutLog) = "log",
        Finish(Finish) = "finish",
        Checkpoint(PutCheckpoint) = "checkpoint",
        ReplyRow(PutReplyRow) = "reply_row",
        LoadReplies(LoadReplies) = "load_replies",
        DropSession(DropSession) = "drop_session",
        ModelHold(ModelHold) = "model_hold",
        ModelSettle(ModelSettle) = "model_settle",
    }
}

impl RunnerCall {
    /// The agent that the body names.
    pub fn agent(&self) -> &AgentId {
        match self {
            RunnerCall::Load(b) => &b.agent,
            RunnerCall::LoadSteps(b) => &b.agent,
            RunnerCall::LoadSession(b) => &b.agent,
            RunnerCall::PutPlaybook(b) => &b.agent,
            RunnerCall::PutSession(b) => &b.agent,
            RunnerCall::Effect(b) => &b.agent,
            RunnerCall::Log(b) => &b.agent,
            RunnerCall::Finish(b) => &b.agent,
            RunnerCall::Checkpoint(b) => &b.agent,
            RunnerCall::ReplyRow(b) => &b.agent,
            RunnerCall::LoadReplies(b) => &b.agent,
            RunnerCall::DropSession(b) => &b.agent,
            RunnerCall::ModelHold(b) => &b.agent,
            RunnerCall::ModelSettle(b) => &b.agent,
        }
    }

    /// The run that the body names.
    pub fn run(&self) -> &RunId {
        match self {
            RunnerCall::Load(b) => &b.run,
            RunnerCall::LoadSteps(b) => &b.run,
            RunnerCall::LoadSession(b) => &b.run,
            RunnerCall::PutPlaybook(b) => &b.run,
            RunnerCall::PutSession(b) => &b.run,
            RunnerCall::Effect(b) => &b.run,
            RunnerCall::Log(b) => &b.run,
            RunnerCall::Finish(b) => &b.run,
            RunnerCall::Checkpoint(b) => &b.run,
            RunnerCall::ReplyRow(b) => &b.run,
            RunnerCall::LoadReplies(b) => &b.run,
            RunnerCall::DropSession(b) => &b.run,
            RunnerCall::ModelHold(b) => &b.run,
            RunnerCall::ModelSettle(b) => &b.run,
        }
    }
}

impl RunnerBody for RunnerCall {
    type Reply = RunnerReply;
}

object_type! {
    /// The newest playbook of `load`: its number and its count of steps, 1 to 64.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct LoadedPlaybook {
        pub number: NonZeroU32 = "number" => from_json,
        pub steps: NonZeroU32 = "steps" => from_json,
    }
    rules(p) {
        if usize::try_from(p.steps.get()).map_or(true, |n| n > PLAYBOOK_STEPS_MAX) {
            return Err(Refusal::length("LoadedPlaybook", "steps").at("steps"));
        }
        Ok(())
    }
}

object_type! {
    /// The newest bindings of `load`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct LoadedBindings {
        pub rev: NonZeroU32 = "rev" => from_json,
        pub origins: Vec<WebOrigin> = "origins" => list(from_json),
        pub slots: Vec<Slot> = "slots" => list(Slot::from_raw),
    }
    rules(b) { origins_and_slots("LoadedBindings", &b.origins, &b.slots) }
}

object_type! {
    /// The newest session revision of one origin in `load`: its revision and its count of parts, 1 to 64.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct LoadedSession {
        pub origin: WebOrigin = "origin" => from_json,
        pub rev: NonZeroU32 = "rev" => from_json,
        pub parts: NonZeroU32 = "parts" => from_json,
    }
    rules(s) {
        if s.parts.get() > SESSION_PARTS_MAX {
            return Err(Refusal::malformed("LoadedSession", "parts_max").at("parts"));
        }
        Ok(())
    }
}

object_type! {
    /// The reply of `load`: the agent's newest finished playbook or null; the number that a new playbook is stored
    /// under, the newest stored number plus 1, above the finished number (CP-182); one session entry per origin, and
    /// the agent's one start row or null when none stands.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct LoadReply {
        pub config: AgentConfig = "config" => AgentConfig::from_raw,
        pub playbook: Option<LoadedPlaybook> = "playbook" => opt(LoadedPlaybook::from_raw),
        pub next_playbook: NonZeroU32 = "next_playbook" => from_json,
        pub bindings: Option<LoadedBindings> = "bindings" => opt(LoadedBindings::from_raw),
        pub sessions: Vec<LoadedSession> = "sessions" => list(LoadedSession::from_raw),
        pub start: Option<AgentStart> = "start" => opt(AgentStart::from_raw),
    }
    rules(r) {
        if r.playbook.as_ref().is_some_and(|p| p.number >= r.next_playbook) {
            return Err(Refusal::malformed("LoadReply", "next_above_playbook").at("next_playbook"));
        }
        let origins: Vec<&WebOrigin> = r.sessions.iter().map(|s| &s.origin).collect();
        each_once("LoadReply", &origins).map_err(|e| e.at("sessions"))?;
        if r
            .start
            .as_ref()
            .is_some_and(|s| (&s.workflow, &s.agent) != (&r.config.workflow, &r.config.agent))
        {
            return Err(Refusal::malformed("LoadReply", "start_of_another_agent").at("start"));
        }
        Ok(())
    }
    arrives(CHECKPOINTS_REPLY_MAX, "reply_bytes")
}

object_type! {
    /// The reply of `load_steps`: the agent of the step rows that the store read, and at most 16 steps, numbered in
    /// order.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct StepsReply {
        pub agent: AgentId = "agent" => json::agent_id,
        pub steps: Vec<ExecutableStep> = "steps" => list(ExecutableStep::from_raw),
    }
    rules(r) { steps_in_order("StepsReply", &r.steps) }
    arrives(CHECKPOINTS_REPLY_MAX, "reply_bytes")
}

object_type! {
    /// The reply of `load_session`: the agent of the session part that the store read, and part `part` of `of`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct SessionReply {
        pub agent: AgentId = "agent" => json::agent_id,
        pub origin: WebOrigin = "origin" => from_json,
        pub rev: NonZeroU32 = "rev" => from_json,
        pub part: NonZeroU32 = "part" => from_json,
        pub of: NonZeroU32 = "of" => from_json,
        pub data: SessionData = "data" => from_json,
    }
    rules(r) { part_of("SessionReply", r.part, r.of, Some(&r.data)) }
    arrives(CHECKPOINTS_REPLY_MAX, "reply_bytes")
}

object_type! {
    /// The reply of the runner bodies `put_playbook`, `put_session`, `effect`, `log`, `finish`, `checkpoint`,
    /// `reply_row`, `drop_session`, `model_hold` and `model_settle`, and of the coordinator bodies `mark_checkpoint` and
    /// `drop_sessions`: the count stored.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Stored {
        pub stored: u32 = "stored" => from_json,
    }
    rules(_r) { Ok(()) }
    arrives(CHECKPOINTS_REPLY_MAX, "reply_bytes")
}

object_type! {
    /// The reply of `load_replies`: for each id asked that has a row of the asking agent its newest row, in the order
    /// asked, and beside them two lists of bare ids asked, each id once and in one list alone: in `blocked` those for
    /// which another agent holds `begun`, `committed` or `unknown`, and in `blocked_withheld` those for which another
    /// agent holds `withheld` and none of the three. Its rows are of one agent and one workflow, and it names at most
    /// 25 ids in all. Its bound, `RUNNER_REPLY_VALUE_MAX`, counts the bytes given to `parse` and to
    /// `RunnerReply::parse`, and the compact text that the writer writes; a reply over it is refused whole. Through
    /// `RunnerReply::parse` the text is read as JSON before this bound refuses it and no row is read before it does,
    /// so a text over the bound that is no JSON is refused as malformed.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct LoadRepliesReply {
        pub replies: Vec<AgentReply> = "replies" => list(AgentReply::from_raw),
        pub blocked: Vec<ProviderId> = "blocked" => list(from_json),
        pub blocked_withheld: Vec<ProviderId> = "blocked_withheld" => list(from_json),
    }
    rules(r) {
        let fail = |key, rule| Err(Refusal::malformed("LoadRepliesReply", rule).at(key));
        if let Some(first) = r.replies.first() {
            if r
                .replies
                .iter()
                .any(|row| (&row.workflow, &row.agent) != (&first.workflow, &first.agent))
            {
                return fail("replies", "row_of_another_agent");
            }
        }
        let messages: Vec<&ProviderId> = r.replies.iter().map(|row| &row.message).collect();
        each_once("LoadRepliesReply", &messages).map_err(|e| e.at("replies"))?;
        each_once("LoadRepliesReply", &r.blocked).map_err(|e| e.at("blocked"))?;
        each_once("LoadRepliesReply", &r.blocked_withheld).map_err(|e| e.at("blocked_withheld"))?;
        if r.blocked_withheld.iter().any(|id| r.blocked.contains(id)) {
            return fail("blocked_withheld", "blocked_twice");
        }
        let mut all: Vec<&ProviderId> = messages;
        all.extend(&r.blocked);
        all.extend(&r.blocked_withheld);
        all.sort_unstable_by(|a, b| a.as_str().cmp(b.as_str()));
        all.dedup();
        if all.len() > REPLY_IDS_MAX {
            return fail("replies", "ids_max");
        }
        Ok(())
    }
    max_len(RUNNER_REPLY_VALUE_MAX)
    arrives(RUNNER_REPLY_VALUE_MAX, "reply_bytes")
}

impl LoadRepliesReply {
    /// Whether the reply answers the body: every row is of the body's agent and of an id that it asked, the rows
    /// stand in the order asked, and each id of the two lists is one that it asked; else the refusal of the first
    /// rule that the reply breaks.
    pub fn answers(&self, body: &LoadReplies) -> Result<(), Refusal> {
        let fail = |key, rule| Err(Refusal::malformed("LoadRepliesReply", rule).at(key));
        if self.replies.iter().any(|row| row.agent != body.agent) {
            return fail("replies", "row_of_another_agent");
        }
        let mut last = None;
        for row in &self.replies {
            let Some(at) = body.message_ids.iter().position(|id| *id == row.message) else {
                return fail("replies", "not_asked");
            };
            if last.is_some_and(|last| at <= last) {
                return fail("replies", "order_asked");
            }
            last = Some(at);
        }
        for (key, ids) in [
            ("blocked", &self.blocked),
            ("blocked_withheld", &self.blocked_withheld),
        ] {
            if ids.iter().any(|id| !body.message_ids.contains(id)) {
                return fail(key, "not_asked");
            }
        }
        Ok(())
    }
}

reply_enum! {
    /// The reply of a runner body, told apart by its keys. Each variant's type has a bound of at most
    /// `CHECKPOINTS_REPLY_MAX` bytes as they arrive in its own `parse`, or the crate does not compile: the reply of
    /// `load_replies` has `RUNNER_REPLY_VALUE_MAX`, and every other `CHECKPOINTS_REPLY_MAX`.
    pub enum RunnerReply {
        Load(LoadReply) = "config",
        Steps(StepsReply) = "steps",
        Session(SessionReply) = "data",
        Stored(Stored) = "stored",
        Replies(LoadRepliesReply) = "replies",
    }
}

impl RunnerReply {
    /// Whether the reply has the form that the body's call answers with; a reply of `load_replies` answers its call
    /// only as `LoadRepliesReply::answers` says, a reply of `load` only when its config, and its start row if it
    /// holds one, are of the body's agent, and a reply of `load_steps` or `load_session` only when it names the
    /// body's agent.
    pub fn answers(&self, call: &RunnerCall) -> bool {
        match (self, call) {
            (RunnerReply::Replies(r), RunnerCall::LoadReplies(b)) => r.answers(b).is_ok(),
            (RunnerReply::Load(r), RunnerCall::Load(b)) => {
                r.config.agent == b.agent && r.start.as_ref().is_none_or(|s| s.agent == b.agent)
            }
            (RunnerReply::Steps(r), RunnerCall::LoadSteps(b)) => r.agent == b.agent,
            (RunnerReply::Session(r), RunnerCall::LoadSession(b)) => r.agent == b.agent,
            _ => matches!(
                (self, call),
                (
                    RunnerReply::Stored(_),
                    RunnerCall::PutPlaybook(_)
                        | RunnerCall::PutSession(_)
                        | RunnerCall::Effect(_)
                        | RunnerCall::Log(_)
                        | RunnerCall::Finish(_)
                        | RunnerCall::Checkpoint(_)
                        | RunnerCall::ReplyRow(_)
                        | RunnerCall::DropSession(_)
                        | RunnerCall::ModelHold(_)
                        | RunnerCall::ModelSettle(_)
                )
            ),
        }
    }
}

object_type! {
    /// `create_agent`: stores the preset of the workflow's category.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct CreateAgent [op = "create_agent"] {
        pub workflow: WorkflowId = "workflow" => json::workflow_id,
        pub agent: AgentId = "agent" => json::agent_id,
    }
    rules(_b) { Ok(()) }
}

object_type! {
    /// `set_bindings`: slots alone, each name once, which the handler merges by name into the newest revision of
    /// the bindings; the body names no origin, so a body that holds the key `origins` is refused.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct SetBindings [op = "set_bindings"] {
        pub workflow: WorkflowId = "workflow" => json::workflow_id,
        pub agent: AgentId = "agent" => json::agent_id,
        pub slots: Vec<Slot> = "slots" => list(Slot::from_raw),
    }
    rules(b) { slot_names_once("SetBindings", &b.slots) }
}

object_type! {
    /// `set_state`: the state call with the agent's id.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct SetState [op = "set_state"] {
        pub workflow: WorkflowId = "workflow" => json::workflow_id,
        pub agent: AgentId = "agent" => json::agent_id,
        pub state: AgentState = "state" => from_json,
    }
    rules(_b) { Ok(()) }
}

object_type! {
    /// `start_run`: the check before a run of the mode.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct StartRun [op = "start_run"] {
        pub workflow: WorkflowId = "workflow" => json::workflow_id,
        pub agent: AgentId = "agent" => json::agent_id,
        pub run: RunId = "run" => from_json,
        pub mode: Mode = "mode" => from_json,
    }
    rules(_b) { Ok(()) }
}

object_type! {
    /// `end_run`: the moves of the run's end for a run whose runner ended without a stored `finish`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct EndRun [op = "end_run"] {
        pub workflow: WorkflowId = "workflow" => json::workflow_id,
        pub agent: AgentId = "agent" => json::agent_id,
        pub run: RunId = "run" => from_json,
    }
    rules(_b) { Ok(()) }
}

object_type! {
    /// `add_origin`: the one origin that the bindings gain, by one of three ways, after the act of the run `run`
    /// with the person's answer; the handler stores the next revision with it, or nothing when it is there.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct AddOrigin [op = "add_origin"] {
        pub workflow: WorkflowId = "workflow" => json::workflow_id,
        pub agent: AgentId = "agent" => json::agent_id,
        pub run: RunId = "run" => from_json,
        pub origin: WebOrigin = "origin" => from_json,
        pub way: Way = "way" => from_json,
    }
    rules(_b) { Ok(()) }
}

object_type! {
    /// `drop_sessions`: the end of the use of every kept sign-in of the agent, with no run of the agent open. The
    /// store keeps, for each origin of a kept sign-in of the agent, one more revision in the drop form, as
    /// `drop_session` keeps it for one origin, and answers the count stored; Halvo stops using those sign-ins, and
    /// what was kept stays under the store's key until its own day.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct DropSessions [op = "drop_sessions"] {
        pub workflow: WorkflowId = "workflow" => json::workflow_id,
        pub agent: AgentId = "agent" => json::agent_id,
    }
    rules(_b) { Ok(()) }
}

object_type! {
    /// `list_replies`: the newest state of each reply of the agent, one entry for each message that has an
    /// `agent_reply` row of the agent, for the messages after the message id `after` in ascending order, or from the
    /// first. It stores nothing and changes nothing.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct ListReplies [op = "list_replies"] {
        pub workflow: WorkflowId = "workflow" => json::workflow_id,
        pub agent: AgentId = "agent" => json::agent_id,
        pub after: Option<ProviderId> = "after" => opt(from_json),
    }
    rules(_b) { Ok(()) }
}

object_type! {
    /// `put_model_key`: the person's key for a model's host, for the person and for no agent, with `workflow` and
    /// `agent` null (r3 U7.6). The store keeps ONE row of the kind `model_key`: in one write it removes the row that
    /// stands, at rest, and stores the key, and answers `{"stored":1}`. The key is at least one byte and at most
    /// `OWN_TEXT_MAX` bytes, taken as given and never trimmed; `Debug` prints no part of it.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct PutModelKey [op = "put_model_key"] {
        pub workflow: Null = "workflow" => from_json,
        pub agent: Null = "agent" => from_json,
        pub key: SecretText = "key" => from_json,
    }
    rules(b) {
        if b.key.expose().len() > OWN_TEXT_MAX {
            return Err(Refusal::length("PutModelKey", "key_length").at("key"));
        }
        Ok(())
    }
}

object_type! {
    /// `drop_model_key`: the end of the person's key, with `workflow` and `agent` null (r3 U7.6). The store removes the
    /// row of the kind `model_key` at rest and answers `{"stored":0}`, also where none stands.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct DropModelKey [op = "drop_model_key"] {
        pub workflow: Null = "workflow" => from_json,
        pub agent: Null = "agent" => from_json,
    }
    rules(_b) { Ok(()) }
}

object_type! {
    /// `model_key_state`: whether the person's key stands, with `workflow` and `agent` null (r3 U7.6): what the window
    /// is told. It stores nothing.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct ModelKeyState [op = "model_key_state"] {
        pub workflow: Null = "workflow" => from_json,
        pub agent: Null = "agent" => from_json,
    }
    rules(_b) { Ok(()) }
}

object_type! {
    /// `read_model_key`: the person's key, with `workflow` and `agent` null (r3 U7.6), which the app's own answer of
    /// the supervisor's `Secrets` to the reserved name `model_key` reads. It stores nothing.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct ReadModelKey [op = "read_model_key"] {
        pub workflow: Null = "workflow" => from_json,
        pub agent: Null = "agent" => from_json,
    }
    rules(_b) { Ok(()) }
}

object_type! {
    /// `set_model`: the person's yes for the agent to a hosted model, or the way back (r3 U7.7). `tier` is the agent's
    /// ceiling; `agreement` is null with `on_device`, the way back, and with a hosted tier a list of `Hosted` triples,
    /// as the caller read them from the router's table: one of the ceiling, at most one of each tier, and no rule for
    /// a missing lower tier. The parser refuses a list with a
    /// tier twice or with `on_device` in a triple, an empty origin or model word (`malformed`) and a longer one than
    /// `OWN_TEXT_MAX` bytes (`length`), and with a hosted tier a list with no triple of that tier or with a tier above
    /// it (`malformed`). A hosted tier with null and `on_device` with a list are `validate`'s cause `tier`. In one write
    /// the store keeps the next revision of the agent's set-up with the tier, and the agent's one `agent_agreement` row
    /// at a yes or none at the way back, and answers that revision.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct SetModel [op = "set_model"] {
        pub workflow: WorkflowId = "workflow" => json::workflow_id,
        pub agent: AgentId = "agent" => json::agent_id,
        pub tier: ModelTier = "tier" => from_json,
        pub agreement: Option<Vec<Hosted>> = "agreement" => opt(list(Hosted::from_raw)),
    }
    rules(b) {
        match (b.tier, &b.agreement) {
            (_, None) => Ok(()),
            (ModelTier::OnDevice, Some(hosts)) => {
                let tiers: Vec<ModelTier> = hosts.iter().map(|h| h.tier).collect();
                each_once("SetModel", &tiers).map_err(|r| r.at("agreement"))
            }
            (ceiling, Some(hosts)) => {
                hosts_up_to("SetModel", ceiling, hosts).map_err(|r| r.at("agreement"))
            }
        }
    }
}

object_type! {
    /// `model_room`: what the person's count is at the request's instant, with `workflow` and `agent` null (r7 8.1L). It
    /// stores nothing.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct ModelRoom [op = "model_room"] {
        pub workflow: Null = "workflow" => from_json,
        pub agent: Null = "agent" => from_json,
    }
    rules(_b) { Ok(()) }
}

object_type! {
    /// `set_model_cap`: the person's set of the cap of the count, `cap_minor` minor units of a currency for each cap
    /// period, with `workflow` and `agent` null (r7 8.1L). The store sets it on the count that stands; where no row
    /// stands or the row does not read, it starts a new count at nothing with that cap.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct SetModelCap [op = "set_model_cap"] {
        pub workflow: Null = "workflow" => from_json,
        pub agent: Null = "agent" => from_json,
        pub cap_minor: u32 = "cap_minor" => from_json,
    }
    rules(_b) { Ok(()) }
}

object_type! {
    /// `new_model_count`: a new count of the person at nothing with the cap of `cap_minor` minor units, with `workflow`
    /// and `agent` null (r7 8.1L; the coordinator's log line 17696 (b)). The store reads no row: it replaces every row
    /// of the person's count by ONE count at nothing with that cap, anchored at the request's instant, and answers as
    /// `set_model_cap` answers a new count. It lifts what is counted, so the app sends it in ONE place alone: where
    /// `model_room` met the fault of the store's read, which no other body can mend.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct NewModelCount [op = "new_model_count"] {
        pub workflow: Null = "workflow" => from_json,
        pub agent: Null = "agent" => from_json,
        pub cap_minor: u32 = "cap_minor" => from_json,
    }
    rules(_b) { Ok(()) }
}

op_enum! {
    /// The coordinator bodies of this run (`CoordinatorCall`): each holds `op` first, then `workflow` and `agent`,
    /// then its keys.
    pub enum CoordinatorCall {
        CreateAgent(CreateAgent) = "create_agent",
        SetBindings(SetBindings) = "set_bindings",
        SetState(SetState) = "set_state",
        StartRun(StartRun) = "start_run",
        EndRun(EndRun) = "end_run",
        AddOrigin(AddOrigin) = "add_origin",
        ListCheckpoints(ListCheckpoints) = "list_checkpoints",
        MarkCheckpoint(MarkCheckpoint) = "mark_checkpoint",
        ListOverlaps(ListOverlaps) = "list_overlaps",
        ListAgents(ListAgents) = "list_agents",
        AgentHistory(AgentHistory) = "agent_history",
        DropSessions(DropSessions) = "drop_sessions",
        ListReplies(ListReplies) = "list_replies",
        PutModelKey(PutModelKey) = "put_model_key",
        DropModelKey(DropModelKey) = "drop_model_key",
        ModelKeyState(ModelKeyState) = "model_key_state",
        ReadModelKey(ReadModelKey) = "read_model_key",
        SetModel(SetModel) = "set_model",
        ModelRoom(ModelRoom) = "model_room",
        SetModelCap(SetModelCap) = "set_model_cap",
        NewModelCount(NewModelCount) = "new_model_count",
    }
}

impl CoordinatorBody for CoordinatorCall {
    type Reply = CoordinatorReply;
}

object_type! {
    /// `list_checkpoints`: one body over all agents, with `workflow` and `agent` null, for the checkpoints after the
    /// run `after`, or from the first.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct ListCheckpoints [op = "list_checkpoints"] {
        pub workflow: Null = "workflow" => from_json,
        pub agent: Null = "agent" => from_json,
        pub after: Option<RunId> = "after" => opt(from_json),
    }
    rules(_b) { Ok(()) }
}

object_type! {
    /// `mark_checkpoint`: the next row of the checkpoint of the run `run` and the workbook `workbook`, `gone` or
    /// `left` with the reason, which stands exactly in a `left` body.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct MarkCheckpoint [op = "mark_checkpoint"] {
        pub workflow: WorkflowId = "workflow" => json::workflow_id,
        pub agent: AgentId = "agent" => json::agent_id,
        pub run: RunId = "run" => from_json,
        pub workbook: OwnValue = "workbook" => from_json,
        pub state: MarkState = "state" => from_json,
        pub left: Option<LeftReason> = "left" => opt(from_json),
    }
    rules(b) {
        let fail = |key, rule| Err(Refusal::malformed("MarkCheckpoint", rule).at(key));
        if b.workbook.is_empty() {
            return fail("workbook", "empty_path");
        }
        if b.left.is_some() != (b.state == MarkState::Left) {
            return fail("left", "left_of_state");
        }
        Ok(())
    }
}

object_type! {
    /// `list_overlaps`: the read of the other live agents that overlap the agent's slots as they would stand with
    /// `slots` merged by name into its newest revision of the bindings, as `set_bindings` merges them, or as they
    /// stand when `slots` is null; each slot name stands once. It stores nothing and changes nothing.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct ListOverlaps [op = "list_overlaps"] {
        pub workflow: WorkflowId = "workflow" => json::workflow_id,
        pub agent: AgentId = "agent" => json::agent_id,
        pub slots: Option<Vec<Slot>> = "slots" => opt(list(Slot::from_raw)),
    }
    rules(b) {
        b.slots
            .as_deref()
            .map_or(Ok(()), |slots| slot_names_once("ListOverlaps", slots))
    }
}

object_type! {
    /// `list_agents`: one body over all agents, with `workflow` and `agent` null, as `list_checkpoints` has them, for
    /// the agents after the agent `after`, or from the first (the interface run's I6.1).
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct ListAgents [op = "list_agents"] {
        pub workflow: Null = "workflow" => from_json,
        pub agent: Null = "agent" => from_json,
        pub after: Option<AgentId> = "after" => opt(json::agent_id),
    }
    rules(_b) { Ok(()) }
}

object_type! {
    /// `agent_history`: the agent's last `runs` runs that have a stored record, newest first, each with the effects
    /// of its run (the interface run's I6.1).
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct AgentHistory [op = "agent_history"] {
        pub workflow: WorkflowId = "workflow" => json::workflow_id,
        pub agent: AgentId = "agent" => json::agent_id,
        pub runs: u32 = "runs" => from_json,
    }
    rules(_b) { Ok(()) }
}

object_type! {
    /// One checkpoint of the reply of `list_checkpoints`, from its newest row, `planned` or `made`: `ended` says
    /// whether the run's `agent_run` record is stored, `effect` is the state of the run's `local_edit` effect on the
    /// workbook or null, and `superseded` whether a later run of the agent has a committed checkpoint of the same
    /// path. An effect of a run whose end is stored is never `releasing`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct ListedCheckpoint {
        pub workflow: WorkflowId = "workflow" => json::workflow_id,
        pub agent: AgentId = "agent" => json::agent_id,
        pub run: RunId = "run" => from_json,
        pub folder: OwnValue = "folder" => from_json,
        pub workbook: OwnValue = "workbook" => from_json,
        pub copy: OwnValue = "copy" => from_json,
        pub state: ListedState = "state" => from_json,
        pub sha256: Option<Digest> = "sha256" => opt(from_json),
        pub bytes: Option<u32> = "bytes" => opt(from_json),
        pub at: Instant = "at" => json::instant,
        pub ended: YesNo = "ended" => from_json,
        pub effect: Option<EffectState> = "effect" => opt(from_json),
        pub superseded: YesNo = "superseded" => from_json,
    }
    rules(c) {
        let state = match c.state {
            ListedState::Planned => CheckpointState::Planned,
            ListedState::Made => CheckpointState::Made,
        };
        checkpoint_copy(
            "ListedCheckpoint",
            [("folder", &c.folder), ("workbook", &c.workbook), ("copy", &c.copy)],
            state,
            c.sha256.as_ref(),
            c.bytes,
        )?;
        if c.ended == YesNo::Yes && c.effect == Some(EffectState::Releasing) {
            return Err(Refusal::malformed("ListedCheckpoint", "releasing_after_end").at("effect"));
        }
        Ok(())
    }
}

object_type! {
    /// The reply of `list_checkpoints`: the checkpoints in ascending order of run id, each run and workbook once,
    /// and the run id to ask after for the next page, or null. Its bound, `CHECKPOINTS_REPLY_MAX`, counts the bytes
    /// given to `parse` and to `CoordinatorReply::parse`, and the compact text that the writer writes.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct CheckpointsReply {
        pub checkpoints: Vec<ListedCheckpoint> = "checkpoints" => list(ListedCheckpoint::from_raw),
        pub next: Option<RunId> = "next" => opt(from_json),
    }
    rules(r) {
        if r.checkpoints.windows(2).any(|w| w[1].run < w[0].run) {
            return Err(Refusal::malformed("CheckpointsReply", "run_order").at("checkpoints"));
        }
        let keys: Vec<(&RunId, &OwnValue)> =
            r.checkpoints.iter().map(|c| (&c.run, &c.workbook)).collect();
        each_once("CheckpointsReply", &keys).map_err(|e| e.at("checkpoints"))
    }
    max_len(CHECKPOINTS_REPLY_MAX)
    arrives(CHECKPOINTS_REPLY_MAX, "reply_bytes")
}

object_type! {
    /// One other live agent of the reply of `list_overlaps`: its workflow, its agent and the senders that it shares
    /// with the asking agent, at least one, in the spelling of the asking agent's list. Each sender is a piece that
    /// the reader of the slot `reply_senders` takes, and no two are one address by `same_address`. Which senders an
    /// entry names, their spelling and their order are those of the asking agent's list, and the store's handler of
    /// `list_overlaps` holds them for every body; this type checks the form of each sender alone (CP-232 (1)).
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Overlap {
        pub workflow: WorkflowId = "workflow" => json::workflow_id,
        pub agent: AgentId = "agent" => json::agent_id,
        pub senders: Vec<OwnText> = "senders" => list(from_json),
    }
    rules(o) {
        let fail = |rule| Err(Refusal::malformed("Overlap", rule).at("senders"));
        if o.senders.is_empty() {
            return fail("no_sender");
        }
        if o.senders.iter().any(|s| !is_sender(s.as_str())) {
            return fail("not_a_sender");
        }
        for (n, sender) in o.senders.iter().enumerate() {
            if o.senders[..n]
                .iter()
                .any(|earlier| same_address(earlier.as_str(), sender.as_str()))
            {
                return fail("sender_twice");
            }
        }
        Ok(())
    }
}

object_type! {
    /// The reply of `list_overlaps`: the other live agents that overlap the asking agent, at most 16, in ascending
    /// order of workflow id, each workflow and each agent once, and `more`, `yes` when more agents overlap than the
    /// 16 that it names, which it holds only beside 16 entries. Its bound, `CHECKPOINTS_REPLY_MAX`, counts the bytes
    /// given to `parse` and to `CoordinatorReply::parse`, and the compact text that the writer writes, so a list that
    /// would not fit is refused whole and never written cut.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct OverlapsReply {
        pub overlaps: Vec<Overlap> = "overlaps" => list(Overlap::from_raw),
        pub more: YesNo = "more" => from_json,
    }
    rules(r) {
        let fail = |key, rule| Err(Refusal::malformed("OverlapsReply", rule).at(key));
        if r.overlaps.len() > OVERLAPS_MAX {
            return fail("overlaps", "overlaps_max");
        }
        if r.overlaps.windows(2).any(|w| w[1].workflow <= w[0].workflow) {
            return fail("overlaps", "workflow_order");
        }
        let agents: Vec<&AgentId> = r.overlaps.iter().map(|o| &o.agent).collect();
        each_once("OverlapsReply", &agents).map_err(|e| e.at("overlaps"))?;
        if r.more == YesNo::Yes && r.overlaps.len() < OVERLAPS_MAX {
            return fail("more", "more_short");
        }
        Ok(())
    }
    max_len(CHECKPOINTS_REPLY_MAX)
    arrives(CHECKPOINTS_REPLY_MAX, "reply_bytes")
}

impl OverlapsReply {
    /// Whether the reply answers the body: it names other agents alone, so no entry names the body's workflow or its
    /// agent; else the refusal of that rule.
    pub fn answers(&self, body: &ListOverlaps) -> Result<(), Refusal> {
        if self
            .overlaps
            .iter()
            .any(|o| o.workflow == body.workflow || o.agent == body.agent)
        {
            return Err(Refusal::malformed("OverlapsReply", "own_agent").at("overlaps"));
        }
        Ok(())
    }
}

object_type! {
    /// One effect of the key `unknown` of an entry of `list_agents`: an effect of the agent whose newest row is
    /// `unknown`, with the run, the effect id, the day, the class and the kept fields of that row.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct UnknownEffect {
        pub run: RunId = "run" => from_json,
        pub effect: EffectId = "effect" => from_json,
        pub day: u32 = "day" => from_json,
        pub class: ActionClass = "class" => from_json,
        pub kept: Vec<KeptField> = "kept" => list(KeptField::from_raw),
    }
    rules(u) { check_kept(u.class, &u.kept).map_err(|r| r.at("kept")) }
}

object_type! {
    /// The agreement of an entry of `list_agents` (r3 U7.7): the ceiling tier of the person's yes that stands for the
    /// agent and its triples, as the agent's `agent_agreement` row holds them, under the rules of that row.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct ListedAgreement {
        pub tier: ModelTier = "tier" => from_json,
        pub hosts: Vec<Hosted> = "hosts" => list(Hosted::from_raw),
    }
    rules(g) { hosts_up_to("ListedAgreement", g.tier, &g.hosts).map_err(|r| r.at("hosts")) }
}

object_type! {
    /// One agent of the reply of `list_agents`: its configuration, the state of its workflow, the slot names of its
    /// newest bindings, `session` (`yes` when a session of the agent is stored whole), its newest run record or
    /// null, its effects whose newest row is `unknown`, `refused_run` (the id of the agent's newest started run
    /// where that run's log holds `step_failed` with the cause `unauthorized`, or null), `reply_unknown` (`yes` when
    /// the run of `last` left a reply in the state `unknown`), `refused_slots` (the names of the slots of those
    /// bindings that their one reader refuses), `last_cause` (the cause that the `stopped` row of the agent's
    /// newest started run holds, or null where that run has not ended, ended with no cause or holds a cause that is no
    /// name of the list) and `window` (`yes` where the agent's next run needs a browser window, `no` where it needs
    /// none, read from the agent's own rows). The five keys after `unknown` hold a run id, a yes or no, slot names, a
    /// name of the closed list of causes and a yes or no alone: never a slot's value, no site, no count, no secret and
    /// no text, of a page, of a site or of an error. `agreement` (r3 U7.7) is the person's yes that stands for the
    /// agent, its ceiling tier and its triples, only where its tier is the tier of `config`, else null. An entry that
    /// lacks a key is refused, `window` and `agreement` among them. A run record of another agent is refused,
    /// `reply_unknown` `yes` beside `last` null, and an agreement whose tier is not the tier of `config`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct ListedAgent {
        pub config: AgentConfig = "config" => AgentConfig::from_raw,
        pub state: AgentState = "state" => from_json,
        pub slots: Vec<Name> = "slots" => list(from_json),
        pub session: YesNo = "session" => from_json,
        pub last: Option<AgentRun> = "last" => opt(AgentRun::from_raw),
        pub unknown: Vec<UnknownEffect> = "unknown" => list(UnknownEffect::from_raw),
        pub refused_run: Option<RunId> = "refused_run" => opt(from_json),
        pub reply_unknown: YesNo = "reply_unknown" => from_json,
        pub refused_slots: Vec<Name> = "refused_slots" => list(from_json),
        pub last_cause: Option<Cause> = "last_cause" => opt(from_json),
        pub window: YesNo = "window" => from_json,
        pub agreement: Option<ListedAgreement> = "agreement" => opt(ListedAgreement::from_raw),
    }
    rules(a) {
        if a.agreement.as_ref().is_some_and(|g| g.tier != a.config.tier) {
            return Err(Refusal::malformed("ListedAgent", "agreement_of_another_tier").at("agreement"));
        }
        if a
            .last
            .as_ref()
            .is_some_and(|r| (&r.workflow, &r.agent) != (&a.config.workflow, &a.config.agent))
        {
            return Err(Refusal::malformed("ListedAgent", "record_of_another_agent").at("last"));
        }
        if a.reply_unknown == YesNo::Yes && a.last.is_none() {
            return Err(
                Refusal::malformed("ListedAgent", "reply_unknown_without_last").at("reply_unknown"),
            );
        }
        Ok(())
    }
}

object_type! {
    /// The reply of `list_agents`: the agents in ascending order of agent id, each once, and the agent id to ask
    /// after for the next page, or null. Its bound, `CHECKPOINTS_REPLY_MAX`, counts the bytes given to `parse` and to
    /// `CoordinatorReply::parse`, and the compact text that the writer writes, so that no reply passes `REPLY_MAX`
    /// with its envelope.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct AgentsReply {
        pub agents: Vec<ListedAgent> = "agents" => list(ListedAgent::from_raw),
        pub next: Option<AgentId> = "next" => opt(json::agent_id),
    }
    rules(r) {
        if r.agents.windows(2).any(|w| w[1].config.agent <= w[0].config.agent) {
            return Err(Refusal::malformed("AgentsReply", "agent_order").at("agents"));
        }
        Ok(())
    }
    max_len(CHECKPOINTS_REPLY_MAX)
    arrives(CHECKPOINTS_REPLY_MAX, "reply_bytes")
}

object_type! {
    /// The checkpoint of a `local_edit` effect in the reply of `agent_history`: the state of the newest
    /// `agent_checkpoint` row of the effect's run for the workbook of its kept `file`, the instant of that row and
    /// the copy's file name as that row holds it. A newest row `planned` gives no checkpoint, since no copy was made,
    /// so the state `planned` is refused.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct PastCheckpoint {
        pub state: CheckpointState = "state" => from_json,
        pub at: Instant = "at" => json::instant,
        pub name: OwnText = "name" => from_json,
    }
    rules(c) {
        if c.state == CheckpointState::Planned {
            return Err(Refusal::malformed("PastCheckpoint", "planned").at("state"));
        }
        Ok(())
    }
}

/// One effect of a run in the reply of `agent_history`, as its last row holds it: the effect id, the digest of that
/// row, the class, the state, the cause of that row or null, and the kept fields of the class. An effect of the class
/// `local_edit` also holds the key `checkpoint`, its checkpoint or null; an effect of another class holds no such
/// key, and its `checkpoint` is none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PastEffect {
    pub effect: EffectId,
    pub digest: Digest,
    pub class: ActionClass,
    pub state: EffectState,
    pub cause: Option<Cause>,
    pub kept: Vec<KeptField>,
    pub checkpoint: Option<PastCheckpoint>,
}

impl PastEffect {
    const KEYS: &'static [&'static str] = &[
        "effect",
        "digest",
        "class",
        "state",
        "cause",
        "kept",
        "checkpoint",
    ];

    /// The keys of an effect of the class: all seven for `local_edit`, the first six for any other.
    fn keys(class: ActionClass) -> &'static [&'static str] {
        if class == ActionClass::LocalEdit {
            Self::KEYS
        } else {
            &Self::KEYS[..6]
        }
    }

    /// Checks the kept fields against the class, and that a checkpoint stands on a `local_edit` effect alone.
    pub fn rules(&self) -> Result<(), Refusal> {
        check_kept(self.class, &self.kept).map_err(|r| r.at("kept"))?;
        if self.class != ActionClass::LocalEdit && self.checkpoint.is_some() {
            return Err(Refusal::malformed("PastEffect", "checkpoint_of_class").at("checkpoint"));
        }
        Ok(())
    }

    pub(in crate::executable) fn from_raw(raw: &RawValue) -> Result<Self, Refusal> {
        let obj = json::object("PastEffect", raw)?;
        let class: ActionClass = obj.read("class", from_json)?;
        obj.exactly(Self::keys(class))?;
        let value = PastEffect {
            effect: obj.read("effect", from_json)?,
            digest: obj.read("digest", from_json)?,
            class,
            state: obj.read("state", from_json)?,
            cause: obj.read("cause", opt(from_json))?,
            kept: obj.read("kept", list(KeptField::from_raw))?,
            checkpoint: if class == ActionClass::LocalEdit {
                obj.read("checkpoint", opt(PastCheckpoint::from_raw))?
            } else {
                None
            },
        };
        value.rules()?;
        Ok(value)
    }
}

impl Serialize for PastEffect {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        self.rules().map_err(json::ser_err)?;
        let keys = Self::keys(self.class);
        let mut map = serializer.serialize_map(Some(keys.len()))?;
        map.serialize_entry("effect", &self.effect)?;
        map.serialize_entry("digest", &self.digest)?;
        map.serialize_entry("class", &self.class)?;
        map.serialize_entry("state", &self.state)?;
        map.serialize_entry("cause", &self.cause)?;
        map.serialize_entry("kept", &self.kept)?;
        if keys.len() == Self::KEYS.len() {
            map.serialize_entry("checkpoint", &self.checkpoint)?;
        }
        map.end()
    }
}

object_type! {
    /// One run in the reply of `agent_history`: its stored record and the effects of the run, each as its last row
    /// holds it.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct HistoryRun {
        pub run: AgentRun = "run" => AgentRun::from_raw,
        pub effects: Vec<PastEffect> = "effects" => list(PastEffect::from_raw),
    }
    rules(_r) { Ok(()) }
}

object_type! {
    /// The reply of `agent_history`: the agent's runs that have a stored record, at most the count asked for, newest
    /// first. Its bound, `CHECKPOINTS_REPLY_MAX`, counts the bytes given to `parse` and to `CoordinatorReply::parse`,
    /// and the compact text that the writer writes, as for every reply of a coordinator body, so that no reply passes
    /// `REPLY_MAX` with its envelope: a reply that would not fit is refused whole and never written cut.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct HistoryReply {
        pub runs: Vec<HistoryRun> = "runs" => list(HistoryRun::from_raw),
    }
    rules(_r) { Ok(()) }
    max_len(CHECKPOINTS_REPLY_MAX)
    arrives(CHECKPOINTS_REPLY_MAX, "reply_bytes")
}

object_type! {
    /// One entry of the reply of `list_replies`, from the agent's newest `agent_reply` row of one message: the
    /// provider's id of the message, the run and the state of that row alone. It holds no draft, effect, seq, day or
    /// instant, and no row holds an address, a subject or a text.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct ListedReply {
        pub message: ProviderId = "message" => from_json,
        pub run: RunId = "run" => from_json,
        pub state: ReplyState = "state" => from_json,
    }
    rules(_r) { Ok(()) }
}

object_type! {
    /// The reply of `list_replies`: the entries in ascending order of message id, by the bytes of the id, each
    /// message once, and the message id to ask after for the next page, or null. Its bound, `CHECKPOINTS_REPLY_MAX`,
    /// counts the bytes given to `parse` and to `CoordinatorReply::parse`, and the compact text that the writer
    /// writes, as for every reply of a coordinator body.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct ListRepliesReply {
        pub replies: Vec<ListedReply> = "replies" => list(ListedReply::from_raw),
        pub next: Option<ProviderId> = "next" => opt(from_json),
    }
    rules(r) {
        if r
            .replies
            .windows(2)
            .any(|w| w[1].message.as_str() <= w[0].message.as_str())
        {
            return Err(Refusal::malformed("ListRepliesReply", "message_order").at("replies"));
        }
        Ok(())
    }
    max_len(CHECKPOINTS_REPLY_MAX)
    arrives(CHECKPOINTS_REPLY_MAX, "reply_bytes")
}

object_type! {
    /// The reply of `create_agent`, `set_bindings`, `add_origin` and `set_model`: the revision stored, or for an
    /// `add_origin` that stored nothing the newest.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct RevReply {
        pub rev: NonZeroU32 = "rev" => from_json,
    }
    rules(_r) { Ok(()) }
    arrives(CHECKPOINTS_REPLY_MAX, "reply_bytes")
}

object_type! {
    /// The reply of `set_state`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct StateReply {
        pub state: AgentState = "state" => from_json,
    }
    rules(_r) { Ok(()) }
    arrives(CHECKPOINTS_REPLY_MAX, "reply_bytes")
}

object_type! {
    /// The reply of `start_run` and `end_run`, `{"ok":"yes"}`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Accepted [ok = "yes"] {}
    rules(_r) { Ok(()) }
    arrives(CHECKPOINTS_REPLY_MAX, "reply_bytes")
}

object_type! {
    /// The reply of `model_key_state`: `yes` while the person's key stands, else `no`, and no part, length or sign of
    /// the key.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct StandsReply {
        pub stands: YesNo = "stands" => from_json,
    }
    rules(_r) { Ok(()) }
    arrives(CHECKPOINTS_REPLY_MAX, "reply_bytes")
}

object_type! {
    /// The reply of `read_model_key`: the person's key, or null where none stands. `Debug` prints no part of it.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct KeyReply {
        pub key: Option<SecretText> = "key" => opt(from_json),
    }
    rules(_r) { Ok(()) }
    arrives(CHECKPOINTS_REPLY_MAX, "reply_bytes")
}

object_type! {
    /// The reply of `model_room` (r7 8.1L): `room` says what the store found of the person's count at the request's
    /// instant. `none`, no count is stored: `cap_minor` is the cap that a new count would hold, in minor units, or null
    /// where the build has none, `spent` and `held` are 0 and `period_end_ms` is null. `reads`, the four values of the
    /// count's room as halvo-route's `Count::room` gives them: `cap_minor` in minor units, null where the count has no
    /// cap; `spent` and `held` in micro-units; and `period_end_ms`, the end of the cap period in milliseconds since
    /// 1970-01-01T00:00:00Z. `unread`, a row stands that does not read: every figure is null.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct RoomReply {
        pub room: RoomState = "room" => from_json,
        pub cap_minor: Option<u64> = "cap_minor" => opt(count64),
        pub spent: Option<u64> = "spent" => opt(count64),
        pub held: Option<u64> = "held" => opt(count64),
        pub period_end_ms: Option<u64> = "period_end_ms" => opt(count64),
    }
    rules(r) {
        let figures = [r.spent, r.held, r.period_end_ms];
        let holds = match r.room {
            RoomState::None => {
                r.spent == Some(0) && r.held == Some(0) && r.period_end_ms.is_none()
            }
            RoomState::Reads => figures.iter().all(Option::is_some),
            RoomState::Unread => r.cap_minor.is_none() && figures.iter().all(Option::is_none),
        };
        if !holds {
            return Err(Refusal::malformed("RoomReply", "room_figures").at("room"));
        }
        Ok(())
    }
    arrives(CHECKPOINTS_REPLY_MAX, "reply_bytes")
}

object_type! {
    /// The reply of `set_model_cap` (r7 8.1L): the cap that the count now holds, in minor units, and `new_count`, `yes`
    /// where the set started a new count at nothing because no row stood or the row did not read, else `no`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct CapReply {
        pub cap_minor: u32 = "cap_minor" => from_json,
        pub new_count: YesNo = "new_count" => from_json,
    }
    rules(_r) { Ok(()) }
    arrives(CHECKPOINTS_REPLY_MAX, "reply_bytes")
}

reply_enum! {
    /// The reply of a coordinator body, told apart by its keys. Each variant's type has a bound of
    /// `CHECKPOINTS_REPLY_MAX` bytes as they arrive in its own `parse`, or the crate does not compile.
    pub enum CoordinatorReply {
        Rev(RevReply) = "rev",
        State(StateReply) = "state",
        Accepted(Accepted) = "ok",
        Checkpoints(CheckpointsReply) = "checkpoints",
        Stored(Stored) = "stored",
        Overlaps(OverlapsReply) = "overlaps",
        Agents(AgentsReply) = "agents",
        History(HistoryReply) = "runs",
        Replies(ListRepliesReply) = "replies",
        Stands(StandsReply) = "stands",
        Key(KeyReply) = "key",
        Room(RoomReply) = "room",
        Cap(CapReply) = "new_count",
    }
}

impl CoordinatorReply {
    /// Whether the reply has the form that the body's call answers with: `create_agent` is answered by the
    /// revision 1 alone, `set_bindings`, `add_origin` and `set_model` by any revision, `list_checkpoints`, `list_agents`,
    /// `agent_history` and `list_replies` by their lists, `mark_checkpoint` by `stored` 0 or 1, `drop_sessions` by any
    /// count stored, `put_model_key` by `stored` 1, `drop_model_key` by `stored` 0, `model_key_state` by `stands`,
    /// `read_model_key` by `key`, `model_room` by `room`, `set_model_cap` by `new_count` with the body's cap,
    /// `new_model_count` by `new_count` `yes` with the body's cap, and `list_overlaps` by its list only as
    /// `OverlapsReply::answers` says.
    pub fn answers(&self, call: &CoordinatorCall) -> bool {
        match (self, call) {
            (CoordinatorReply::Overlaps(r), CoordinatorCall::ListOverlaps(b)) => {
                r.answers(b).is_ok()
            }
            (CoordinatorReply::Rev(r), CoordinatorCall::CreateAgent(_)) => r.rev.get() == 1,
            (CoordinatorReply::Stored(r), CoordinatorCall::MarkCheckpoint(_)) => r.stored <= 1,
            (CoordinatorReply::Stored(r), CoordinatorCall::PutModelKey(_)) => r.stored == 1,
            (CoordinatorReply::Stored(r), CoordinatorCall::DropModelKey(_)) => r.stored == 0,
            (CoordinatorReply::Cap(r), CoordinatorCall::SetModelCap(b)) => {
                r.cap_minor == b.cap_minor
            }
            (CoordinatorReply::Cap(r), CoordinatorCall::NewModelCount(b)) => {
                r.cap_minor == b.cap_minor && r.new_count == YesNo::Yes
            }
            (
                CoordinatorReply::Rev(_),
                CoordinatorCall::SetBindings(_)
                | CoordinatorCall::AddOrigin(_)
                | CoordinatorCall::SetModel(_),
            )
            | (CoordinatorReply::State(_), CoordinatorCall::SetState(_))
            | (CoordinatorReply::Checkpoints(_), CoordinatorCall::ListCheckpoints(_))
            | (CoordinatorReply::Agents(_), CoordinatorCall::ListAgents(_))
            | (CoordinatorReply::History(_), CoordinatorCall::AgentHistory(_))
            | (CoordinatorReply::Stored(_), CoordinatorCall::DropSessions(_))
            | (CoordinatorReply::Replies(_), CoordinatorCall::ListReplies(_))
            | (CoordinatorReply::Stands(_), CoordinatorCall::ModelKeyState(_))
            | (CoordinatorReply::Key(_), CoordinatorCall::ReadModelKey(_))
            | (CoordinatorReply::Room(_), CoordinatorCall::ModelRoom(_))
            | (
                CoordinatorReply::Accepted(_),
                CoordinatorCall::StartRun(_) | CoordinatorCall::EndRun(_),
            ) => true,
            _ => false,
        }
    }
}
