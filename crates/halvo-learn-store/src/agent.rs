use std::num::NonZeroU32;

use halvo_schema::executable::body::{Load, LoadReply, LoadSteps, STEPS_PER_REQUEST_MAX};
use halvo_schema::executable::{ExecutableStep, RunId, RunnerCall, RunnerReply};
use halvo_schema::{AgentId, ErrorName};

use crate::host::LearnHost;

/// Passes a runner body on to the agent store's handler, with the calls of the family `agent`.
pub(crate) fn runner(host: &mut dyn LearnHost, body: &[u8]) -> Result<RunnerReply, ErrorName> {
    let now = host.agent_now();
    halvo_agent_store::runner(host.agent_host(), now, body)
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
/// configuration and its newest compile number. None where the agent store refuses the body.
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
