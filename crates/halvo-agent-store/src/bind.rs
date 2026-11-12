use halvo_schema::{AgentId, ErrorName, FullWorkflow, Workflow, WorkflowId};

use crate::errors::Own;
use crate::host::Host;

pub(crate) fn stored(host: &dyn Host, workflow: &WorkflowId) -> Result<Workflow, ErrorName> {
    host.workflow(workflow)
        .map(|item| item.workflow)
        .ok_or(ErrorName::NoWorkflow)
}

pub(crate) fn of_agent(
    host: &dyn Host,
    workflow: &WorkflowId,
    agent: &AgentId,
) -> Result<FullWorkflow, ErrorName> {
    match stored(host, workflow)? {
        Workflow::Full(w) if w.agent.as_ref() == Some(agent) => Ok(w),
        _ => Err(Own::Binding.into()),
    }
}
