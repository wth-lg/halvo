use halvo_schema::executable::{CoordinatorReply, RunnerReply};
use halvo_schema::learning::body::Refinements;
use halvo_schema::ErrorName;

use crate::host::LearnHost;

/// Why the handlers refused a request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// The error of the reply form (W-C3.2), the agent store's own or the handlers'.
    Error(ErrorName),
    /// `check_sources` answered `source` for a row of the commit of a `finish` (contracts.md L1.5, L3.7): the
    /// agent store's rows of the run are stored, the handlers made no commit, and the run reads as a lost run.
    Source,
}

impl Refused {
    /// The error name of the reply: the error itself, or `internal` for `Source`.
    pub fn error_name(self) -> ErrorName {
        match self {
            Refused::Error(name) => name,
            Refused::Source => ErrorName::Internal,
        }
    }
}

impl From<ErrorName> for Refused {
    fn from(name: ErrorName) -> Refused {
        Refused::Error(name)
    }
}

/// The answer to a `coordinator_write` request: the agent store's reply, or the reply of `list_refinements`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CoordinatorAnswer {
    Agent(CoordinatorReply),
    Refinements(Refinements),
}

impl CoordinatorAnswer {
    /// The compact JSON of the reply, or `internal` where its writer refuses it.
    pub fn to_json(&self) -> Result<String, ErrorName> {
        match self {
            CoordinatorAnswer::Agent(reply) => reply.to_json().map_err(|_| ErrorName::Internal),
            CoordinatorAnswer::Refinements(reply) => {
                reply.to_json().map_err(|_| ErrorName::Internal)
            }
        }
    }
}

/// The learning handlers (contracts.md L4): the handler of the request types `runner` and `coordinator_write`
/// that a host puts in front of the agent store's handler. They pass every request on to the agent store's
/// handler (`halvo_agent_store::runner` and `halvo_agent_store::coordinator`) but a `put_playbook` of a replay
/// run and the read `list_refinements`, which they answer themselves; they hold the facts of each run in memory,
/// serve learned targets in the reply of `load_steps`, and make at most one typed commit of the family `learn`
/// for a request, after the agent store's own.
#[derive(Default)]
pub struct LearnHandlers {}

impl LearnHandlers {
    /// Handlers that hold no facts of any run.
    pub fn new() -> LearnHandlers {
        LearnHandlers::default()
    }

    /// Answers one `runner` body (AG-A7.1), read from its bytes.
    pub fn runner(
        &mut self,
        host: &mut dyn LearnHost,
        body: &[u8],
    ) -> Result<RunnerReply, Refused> {
        let now = host.agent_now();
        Ok(halvo_agent_store::runner(host.agent_host(), now, body)?)
    }

    /// Answers one `coordinator_write` body (AG-A7.2, contracts.md L2.2), read from its bytes.
    pub fn coordinator(
        &mut self,
        host: &mut dyn LearnHost,
        body: &[u8],
    ) -> Result<CoordinatorAnswer, Refused> {
        let now = host.agent_now();
        let reply = halvo_agent_store::coordinator(host.agent_host(), now, body)?;
        Ok(CoordinatorAnswer::Agent(reply))
    }
}
