use halvo_schema::executable::names::LogEvent;
use halvo_schema::executable::{AgentConfig, AgentLog, RunId};
use halvo_schema::{AgentId, ErrorName, FullWorkflow, Workflow, WorkflowId};

use crate::errors::Own;
use crate::host::{Host, Now};
use crate::{day, rows};

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

pub(crate) fn runner_agent(
    host: &dyn Host,
    agent: &AgentId,
) -> Result<(AgentConfig, FullWorkflow), ErrorName> {
    let config = rows::newest_config(host.rows(), agent)?.ok_or(Own::Binding)?;
    let workflow = of_agent(host, &config.workflow, agent)?;
    Ok((config, workflow))
}

pub(crate) fn open_run(host: &dyn Host, run: &RunId) -> Result<(), ErrorName> {
    if rows::runs(host.rows())?.iter().any(|r| &r.run == run) {
        return Err(Own::Effect.into());
    }
    Ok(())
}

pub(crate) fn no_other_agent(
    host: &dyn Host,
    run: &RunId,
    agent: &AgentId,
) -> Result<(), ErrorName> {
    let rows = host.rows();
    let another = rows::effects(rows)?
        .iter()
        .any(|r| &r.run == run && &r.agent != agent)
        || rows::runs(rows)?
            .iter()
            .any(|r| &r.run == run && &r.agent != agent)
        || rows::logs(rows)?
            .iter()
            .any(|r| &r.run == run && &r.agent != agent);
    if another {
        return Err(Own::Binding.into());
    }
    Ok(())
}

pub(crate) fn start_row(
    host: &dyn Host,
    now: Now,
    run: &RunId,
    agent: &AgentId,
) -> Result<Option<AgentLog>, ErrorName> {
    let today = day::of(host, now);
    let bound = day::bound(rows::AGENT_LOG);
    Ok(rows::logs(host.rows())?.into_iter().find(|l| {
        l.event == LogEvent::StartAccepted
            && &l.run == run
            && &l.agent == agent
            && bound.is_none_or(|days| day::within(today, days, l.day))
    }))
}

pub(crate) fn own_run(
    host: &dyn Host,
    now: Now,
    run: &RunId,
    agent: &AgentId,
) -> Result<(), ErrorName> {
    no_other_agent(host, run, agent)?;
    if start_row(host, now, run, agent)?.is_none() {
        return Err(Own::Binding.into());
    }
    Ok(())
}
