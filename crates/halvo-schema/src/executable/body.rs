use std::num::NonZeroU32;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::value::RawValue;

use crate::ids::{AgentId, WorkflowId};
use crate::request::{CoordinatorBody, RunnerBody};

use super::config::AgentConfig;
use super::json::{self, each_once, from_json, list, object_type, opt};
use super::names::{AgentState, Mode};
use super::record::{
    origins_and_slots, part_of, slot_names_once, AgentEffect, AgentLog, AgentRun, Slot,
    SESSION_PARTS_MAX,
};
use super::step::{ExecutableStep, PLAYBOOK_STEPS_MAX};
use super::text::{RunId, SessionData, WebOrigin};
use super::Refusal;

/// The most steps of one `put_playbook` body and of one `load_steps` reply.
pub const STEPS_PER_REQUEST_MAX: usize = 16;

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
    /// The reply of `load`: one session entry per origin.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct LoadReply {
        pub config: AgentConfig = "config" => AgentConfig::from_raw,
        pub playbook: Option<LoadedPlaybook> = "playbook" => opt(LoadedPlaybook::from_raw),
        pub bindings: Option<LoadedBindings> = "bindings" => opt(LoadedBindings::from_raw),
        pub sessions: Vec<LoadedSession> = "sessions" => list(LoadedSession::from_raw),
    }
    rules(r) {
        let origins: Vec<&WebOrigin> = r.sessions.iter().map(|s| &s.origin).collect();
        each_once("LoadReply", &origins).map_err(|e| e.at("sessions"))
    }
}

object_type! {
    /// The reply of `load_steps`: at most 16 steps, numbered in order.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct StepsReply {
        pub steps: Vec<ExecutableStep> = "steps" => list(ExecutableStep::from_raw),
    }
    rules(r) { steps_in_order("StepsReply", &r.steps) }
}

object_type! {
    /// The reply of `load_session`: part `part` of `of`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct SessionReply {
        pub origin: WebOrigin = "origin" => from_json,
        pub rev: NonZeroU32 = "rev" => from_json,
        pub part: NonZeroU32 = "part" => from_json,
        pub of: NonZeroU32 = "of" => from_json,
        pub data: SessionData = "data" => from_json,
    }
    rules(r) { part_of("SessionReply", r.part, r.of, Some(&r.data)) }
}

object_type! {
    /// The reply of `put_playbook`, `put_session`, `effect`, `log` and `finish`: the count stored.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Stored {
        pub stored: u32 = "stored" => from_json,
    }
    rules(_r) { Ok(()) }
}

/// The reply of a runner body, told apart by its keys.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunnerReply {
    Load(LoadReply),
    Steps(StepsReply),
    Session(SessionReply),
    Stored(Stored),
}

impl RunnerReply {
    pub(in crate::executable) fn from_raw(raw: &RawValue) -> Result<Self, Refusal> {
        let obj = json::object("RunnerReply", raw)?;
        if obj.has("config") {
            LoadReply::from_raw(raw).map(RunnerReply::Load)
        } else if obj.has("steps") {
            StepsReply::from_raw(raw).map(RunnerReply::Steps)
        } else if obj.has("data") {
            SessionReply::from_raw(raw).map(RunnerReply::Session)
        } else if obj.has("stored") {
            Stored::from_raw(raw).map(RunnerReply::Stored)
        } else {
            Err(Refusal::malformed("RunnerReply", "reply_form"))
        }
    }

    /// Reads one reply and refuses every other input.
    pub fn parse(bytes: &[u8]) -> Result<Self, Refusal> {
        Self::from_raw(json::whole("RunnerReply", bytes)?)
    }

    /// The compact JSON of the reply.
    pub fn to_json(&self) -> Result<String, Refusal> {
        match self {
            RunnerReply::Load(r) => r.to_json(),
            RunnerReply::Steps(r) => r.to_json(),
            RunnerReply::Session(r) => r.to_json(),
            RunnerReply::Stored(r) => r.to_json(),
        }
    }

    /// Whether the reply has the form that the body's call answers with.
    pub fn answers(&self, call: &RunnerCall) -> bool {
        matches!(
            (self, call),
            (RunnerReply::Load(_), RunnerCall::Load(_))
                | (RunnerReply::Steps(_), RunnerCall::LoadSteps(_))
                | (RunnerReply::Session(_), RunnerCall::LoadSession(_))
                | (
                    RunnerReply::Stored(_),
                    RunnerCall::PutPlaybook(_)
                        | RunnerCall::PutSession(_)
                        | RunnerCall::Effect(_)
                        | RunnerCall::Log(_)
                        | RunnerCall::Finish(_)
                )
        )
    }
}

impl Serialize for RunnerReply {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            RunnerReply::Load(r) => r.serialize(serializer),
            RunnerReply::Steps(r) => r.serialize(serializer),
            RunnerReply::Session(r) => r.serialize(serializer),
            RunnerReply::Stored(r) => r.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for RunnerReply {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = json::deserialize_raw("RunnerReply", deserializer)?;
        Self::from_raw(&raw).map_err(<D::Error as serde::de::Error>::custom)
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

op_enum! {
    /// The coordinator bodies of this run (`CoordinatorCall`): each holds `op` first, then `workflow` and `agent`,
    /// then its keys. The `op` name `list_agents` is reserved and is no body yet.
    pub enum CoordinatorCall {
        CreateAgent(CreateAgent) = "create_agent",
        SetBindings(SetBindings) = "set_bindings",
        SetState(SetState) = "set_state",
        StartRun(StartRun) = "start_run",
        EndRun(EndRun) = "end_run",
    }
}

impl CoordinatorBody for CoordinatorCall {
    type Reply = CoordinatorReply;
}

object_type! {
    /// The reply of `create_agent` and `set_bindings`: the revision stored.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct RevReply {
        pub rev: NonZeroU32 = "rev" => from_json,
    }
    rules(_r) { Ok(()) }
}

object_type! {
    /// The reply of `set_state`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct StateReply {
        pub state: AgentState = "state" => from_json,
    }
    rules(_r) { Ok(()) }
}

object_type! {
    /// The reply of `start_run` and `end_run`, `{"ok":"yes"}`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Accepted [ok = "yes"] {}
    rules(_r) { Ok(()) }
}

/// The reply of a coordinator body, told apart by its keys.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CoordinatorReply {
    Rev(RevReply),
    State(StateReply),
    Accepted(Accepted),
}

impl CoordinatorReply {
    pub(in crate::executable) fn from_raw(raw: &RawValue) -> Result<Self, Refusal> {
        let obj = json::object("CoordinatorReply", raw)?;
        if obj.has("rev") {
            RevReply::from_raw(raw).map(CoordinatorReply::Rev)
        } else if obj.has("state") {
            StateReply::from_raw(raw).map(CoordinatorReply::State)
        } else if obj.has("ok") {
            Accepted::from_raw(raw).map(CoordinatorReply::Accepted)
        } else {
            Err(Refusal::malformed("CoordinatorReply", "reply_form"))
        }
    }

    /// Reads one reply and refuses every other input.
    pub fn parse(bytes: &[u8]) -> Result<Self, Refusal> {
        Self::from_raw(json::whole("CoordinatorReply", bytes)?)
    }

    /// The compact JSON of the reply.
    pub fn to_json(&self) -> Result<String, Refusal> {
        match self {
            CoordinatorReply::Rev(r) => r.to_json(),
            CoordinatorReply::State(r) => r.to_json(),
            CoordinatorReply::Accepted(r) => r.to_json(),
        }
    }

    /// Whether the reply has the form that the body's call answers with.
    pub fn answers(&self, call: &CoordinatorCall) -> bool {
        matches!(
            (self, call),
            (
                CoordinatorReply::Rev(_),
                CoordinatorCall::CreateAgent(_) | CoordinatorCall::SetBindings(_)
            ) | (CoordinatorReply::State(_), CoordinatorCall::SetState(_))
                | (
                    CoordinatorReply::Accepted(_),
                    CoordinatorCall::StartRun(_) | CoordinatorCall::EndRun(_)
                )
        )
    }
}

impl Serialize for CoordinatorReply {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            CoordinatorReply::Rev(r) => r.serialize(serializer),
            CoordinatorReply::State(r) => r.serialize(serializer),
            CoordinatorReply::Accepted(r) => r.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for CoordinatorReply {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = json::deserialize_raw("CoordinatorReply", deserializer)?;
        Self::from_raw(&raw).map_err(<D::Error as serde::de::Error>::custom)
    }
}
