use halvo_schema::executable::{AgentBindings, AgentConfig, AgentPlaybook, Refusal};
use halvo_schema::{AgentId, ErrorName, RowRead};

pub(crate) const AGENT_CONFIG: &str = "agent_config";
pub(crate) const AGENT_PLAYBOOK: &str = "agent_playbook";
pub(crate) const AGENT_BINDINGS: &str = "agent_bindings";

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

pub(crate) fn configs(rows: &dyn RowRead) -> Result<Vec<AgentConfig>, ErrorName> {
    parsed(rows, AGENT_CONFIG, AgentConfig::parse)
}

pub(crate) fn playbooks(rows: &dyn RowRead) -> Result<Vec<AgentPlaybook>, ErrorName> {
    parsed(rows, AGENT_PLAYBOOK, AgentPlaybook::parse)
}

pub(crate) fn bindings(rows: &dyn RowRead) -> Result<Vec<AgentBindings>, ErrorName> {
    parsed(rows, AGENT_BINDINGS, AgentBindings::parse)
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
