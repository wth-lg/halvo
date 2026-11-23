use halvo_schema::executable::{
    AgentAgreement, AgentBindings, AgentCheckpoint, AgentConfig, AgentEffect, AgentFinished,
    AgentLog, AgentPlaybook, AgentReply, AgentRun, AgentSession, AgentStart, ModelKey, ModelTier,
    Refusal,
};
use halvo_schema::{AgentId, ErrorName, RowRead, TypedRecord};

use crate::errors::refused;

pub(crate) const AGENT_CONFIG: &str = "agent_config";
pub(crate) const AGENT_RUN: &str = "agent_run";
pub(crate) const AGENT_PLAYBOOK: &str = "agent_playbook";
pub(crate) const AGENT_BINDINGS: &str = "agent_bindings";
pub(crate) const AGENT_EFFECT: &str = "agent_effect";
pub(crate) const AGENT_LOG: &str = "agent_log";
pub(crate) const AGENT_SESSION: &str = "agent_session";
pub(crate) const AGENT_CHECKPOINT: &str = "agent_checkpoint";
pub(crate) const AGENT_REPLY: &str = "agent_reply";
pub(crate) const AGENT_START: &str = "agent_start";
pub(crate) const AGENT_FINISHED: &str = "agent_finished";
pub(crate) const MODEL_KEY: &str = "model_key";
pub(crate) const AGENT_AGREEMENT: &str = "agent_agreement";
pub(crate) const MODEL_COUNT: &str = "model_count";

pub(crate) fn parsed<T>(
    rows: &dyn RowRead,
    kind: &str,
    parse: fn(&[u8]) -> Result<T, Refusal>,
) -> Result<Vec<T>, ErrorName> {
    let mut out = Vec::new();
    let mut unread = false;
    rows.for_each(kind, &mut |row| match parse(row.as_bytes()) {
        Ok(value) => out.push(value),
        Err(_) => unread = true,
    });
    if unread {
        return Err(ErrorName::Store);
    }
    Ok(out)
}

pub(crate) fn record(
    kind: &'static str,
    json: Result<String, Refusal>,
) -> Result<TypedRecord, ErrorName> {
    Ok(TypedRecord {
        kind,
        json: json.map_err(refused)?,
    })
}

pub(crate) fn configs(rows: &dyn RowRead) -> Result<Vec<AgentConfig>, ErrorName> {
    parsed(rows, AGENT_CONFIG, AgentConfig::parse)
}

pub(crate) fn runs(rows: &dyn RowRead) -> Result<Vec<AgentRun>, ErrorName> {
    parsed(rows, AGENT_RUN, AgentRun::parse)
}

pub(crate) fn playbooks(rows: &dyn RowRead) -> Result<Vec<AgentPlaybook>, ErrorName> {
    parsed(rows, AGENT_PLAYBOOK, AgentPlaybook::parse)
}

pub(crate) fn bindings(rows: &dyn RowRead) -> Result<Vec<AgentBindings>, ErrorName> {
    parsed(rows, AGENT_BINDINGS, AgentBindings::parse)
}

pub(crate) fn effects(rows: &dyn RowRead) -> Result<Vec<AgentEffect>, ErrorName> {
    parsed(rows, AGENT_EFFECT, AgentEffect::parse)
}

pub(crate) fn logs(rows: &dyn RowRead) -> Result<Vec<AgentLog>, ErrorName> {
    parsed(rows, AGENT_LOG, AgentLog::parse)
}

pub(crate) fn sessions(rows: &dyn RowRead) -> Result<Vec<AgentSession>, ErrorName> {
    parsed(rows, AGENT_SESSION, AgentSession::parse)
}

pub(crate) fn checkpoints(rows: &dyn RowRead) -> Result<Vec<AgentCheckpoint>, ErrorName> {
    parsed(rows, AGENT_CHECKPOINT, AgentCheckpoint::parse)
}

pub(crate) fn replies(rows: &dyn RowRead) -> Result<Vec<AgentReply>, ErrorName> {
    parsed(rows, AGENT_REPLY, AgentReply::parse)
}

pub(crate) fn finished(rows: &dyn RowRead) -> Result<Vec<AgentFinished>, ErrorName> {
    parsed(rows, AGENT_FINISHED, AgentFinished::parse)
}

/// The rows of the person's key, of which the store keeps ONE (r3 U7.6).
pub(crate) fn model_keys(rows: &dyn RowRead) -> Result<Vec<ModelKey>, ErrorName> {
    parsed(rows, MODEL_KEY, ModelKey::parse)
}

pub(crate) fn start_of(
    rows: &dyn RowRead,
    agent: &AgentId,
) -> Result<Option<AgentStart>, ErrorName> {
    Ok(parsed(rows, AGENT_START, AgentStart::parse)?
        .into_iter()
        .filter(|s| &s.agent == agent)
        .min_by_key(|s| s.at))
}

pub(crate) fn newest_config(
    rows: &dyn RowRead,
    agent: &AgentId,
) -> Result<Option<AgentConfig>, ErrorName> {
    Ok(configs(rows)?
        .into_iter()
        .filter(|c| &c.agent == agent)
        .max_by_key(|c| c.rev))
}

/// The rows of the persons' yes to a hosted model, at most one for an agent (r3 U7.7).
pub(crate) fn agreements(rows: &dyn RowRead) -> Result<Vec<AgentAgreement>, ErrorName> {
    parsed(rows, AGENT_AGREEMENT, AgentAgreement::parse)
}

/// The agent's standing ceiling, the one function that answers it (r3 U7.7): the agent's `agent_agreement` row under
/// the workflow of its newest set-up where that row's tier is the tier of that set-up, a hosted tier; none for an
/// agent with no set-up, for `on_device`, and for a hosted tier with no row of that tier. The roster reads it; the
/// check of a `model_hold` will. For an agent whose newest set-up holds a hosted tier it reads every agreement row,
/// and a row that the typed reader refuses, of any agent, answers `store`; for an agent with no set-up or at
/// `on_device` it reads no agreement row.
pub(crate) fn standing_agreement(
    rows: &dyn RowRead,
    agent: &AgentId,
) -> Result<Option<AgentAgreement>, ErrorName> {
    let Some(newest) = newest_config(rows, agent)? else {
        return Ok(None);
    };
    if newest.tier == ModelTier::OnDevice {
        return Ok(None);
    }
    Ok(agreements(rows)?
        .into_iter()
        .rfind(|a| &a.agent == agent && a.workflow == newest.workflow && a.tier == newest.tier))
}
