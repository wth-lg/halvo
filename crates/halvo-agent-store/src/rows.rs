use halvo_schema::executable::{
    AgentBindings, AgentConfig, AgentEffect, AgentLog, AgentPlaybook, AgentRun, AgentSession,
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

fn parsed<T>(
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

pub(crate) fn newest_config(
    rows: &dyn RowRead,
    agent: &AgentId,
) -> Result<Option<AgentConfig>, ErrorName> {
    Ok(configs(rows)?
        .into_iter()
        .filter(|c| &c.agent == agent)
        .max_by_key(|c| c.rev))
}
