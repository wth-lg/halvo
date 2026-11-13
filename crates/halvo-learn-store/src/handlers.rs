use std::collections::BTreeMap;

use halvo_policy::class_of;
use halvo_schema::executable::{
    ActionClass, CoordinatorReply, ExecutableStep, RunId, RunnerCall, RunnerReply,
};
use halvo_schema::learning::body::Refinements;
use halvo_schema::{ErrorName, SeedCategory};

use crate::agent;
use crate::facts::RunFacts;
use crate::host::LearnHost;
use crate::ids::Ids;

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
pub struct LearnHandlers {
    pub(crate) ids: Ids,
    pub(crate) facts: BTreeMap<RunId, RunFacts>,
    pub(crate) class_rule: fn(SeedCategory, &ExecutableStep) -> ActionClass,
}

impl Default for LearnHandlers {
    fn default() -> LearnHandlers {
        LearnHandlers {
            ids: Ids::default(),
            facts: BTreeMap::new(),
            class_rule: class_of,
        }
    }
}

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
        match RunnerCall::parse(body) {
            Ok(RunnerCall::Load(b)) => self.load(host, body, &b),
            Ok(RunnerCall::LoadSteps(b)) => self.load_steps(host, body, &b),
            Ok(RunnerCall::PutPlaybook(b)) => self.put_playbook(host, body, &b),
            _ => Ok(agent::runner(host, body)?),
        }
    }

    /// Answers one `coordinator_write` body (AG-A7.2, contracts.md L2.2), read from its bytes.
    pub fn coordinator(
        &mut self,
        host: &mut dyn LearnHost,
        body: &[u8],
    ) -> Result<CoordinatorAnswer, Refused> {
        self.coordinator_write(host, body)
    }
}
