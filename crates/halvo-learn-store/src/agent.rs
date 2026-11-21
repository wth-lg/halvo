use std::num::NonZeroU32;

use halvo_schema::executable::body::{Load, LoadReply, LoadSteps, STEPS_PER_REQUEST_MAX};
use halvo_schema::executable::{ExecutableStep, RunId, RunnerCall, RunnerReply};
use halvo_schema::{AgentId, ErrorName};

use crate::host::LearnHost;

/// Passes a runner body on to the agent store's handler, with the calls of the family `agent`; a call in which a read
/// met a fault of the store is refused `store` (`LearnHost::agent_faulted`).
pub(crate) fn runner(host: &mut dyn LearnHost, body: &[u8]) -> Result<RunnerReply, ErrorName> {
    let now = host.agent_now();
    let reply = halvo_agent_store::runner(host.agent_host(), now, body);
    if host.agent_faulted() {
        return Err(ErrorName::Store);
    }
    reply
}

fn ask(host: &mut dyn LearnHost, call: RunnerCall) -> Result<Option<RunnerReply>, ErrorName> {
    let body = call.to_json().map_err(|_| ErrorName::Internal)?;
    match runner(host, body.as_bytes()) {
        Ok(reply) => Ok(Some(reply)),
        Err(e @ (ErrorName::Store | ErrorName::Internal)) => Err(e),
        Err(_) => Ok(None),
    }
}

/// What the agent store answers a `load` of the agent in the run, which stores nothing: the agent's
/// configuration and its newest finished compile number (CP-182). None where the agent store refuses the body.
pub(crate) fn load(
    host: &mut dyn LearnHost,
    agent: &AgentId,
    run: &RunId,
) -> Result<Option<LoadReply>, ErrorName> {
    let call = RunnerCall::Load(Load {
        agent: agent.clone(),
        run: run.clone(),
    });
    match ask(host, call)? {
        Some(RunnerReply::Load(reply)) => Ok(Some(reply)),
        Some(_) => Err(ErrorName::Internal),
        None => Ok(None),
    }
}

/// The agent's newest finished compile number as the agent store's `load` would give it, by the agent store's read
/// that needs no runner body (CP-135 (5) refuses a runner body of a run with no start row): `Some(None)` for an
/// agent with no finished compile number, and none where the agent store refuses the agent; `store` where a read of
/// the call met a fault of the store.
pub(crate) fn newest_number(
    host: &mut dyn LearnHost,
    agent: &AgentId,
) -> Result<Option<Option<NonZeroU32>>, ErrorName> {
    let read = halvo_agent_store::newest_playbook(&*host.agent_host(), agent);
    if host.agent_faulted() {
        return Err(ErrorName::Store);
    }
    match read {
        Ok(playbook) => Ok(Some(playbook.map(|p| p.number))),
        Err(e @ (ErrorName::Store | ErrorName::Internal)) => Err(e),
        Err(_) => Ok(None),
    }
}

/// The stored steps of the compile number, read page by page through `load_steps`, which stores nothing.
pub(crate) fn compiled(
    host: &mut dyn LearnHost,
    agent: &AgentId,
    run: &RunId,
    number: NonZeroU32,
) -> Result<Vec<ExecutableStep>, ErrorName> {
    let mut steps: Vec<ExecutableStep> = Vec::new();
    let mut from = NonZeroU32::MIN;
    loop {
        let call = RunnerCall::LoadSteps(LoadSteps {
            agent: agent.clone(),
            run: run.clone(),
            number,
            from,
        });
        let page = match ask(host, call)? {
            Some(RunnerReply::Steps(reply)) => reply.steps,
            Some(_) => return Err(ErrorName::Internal),
            None => return Ok(steps),
        };
        let full = page.len() == STEPS_PER_REQUEST_MAX;
        let Some(last) = page.last().map(|s| s.n) else {
            return Ok(steps);
        };
        steps.extend(page);
        match last.checked_add(1) {
            Some(next) if full => from = next,
            _ => return Ok(steps),
        }
    }
}
