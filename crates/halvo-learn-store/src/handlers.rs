use std::collections::BTreeMap;

use halvo_policy::class_of;
use halvo_schema::executable::{
    ActionClass, CoordinatorReply, ExecutableStep, RunId, RunnerCall, RunnerReply,
};
use halvo_schema::learning::body::Refinements;
use halvo_schema::{ErrorName, SeedCategory};

use crate::agent;
use crate::facts::{RunFacts, Start};
use crate::host::LearnHost;
use crate::ids::Ids;

/// The most runs whose facts or remembered start one value holds at one time; when one more would be made, the
/// one that was made first goes first and reads as a lost run (contracts.md L4.2, ruling LM-43 F4).
pub const RUNS_HELD_MAX: usize = 8;

/// The most `step_ok`, `step_failed` and `recovered` lines that one value holds for one run (contracts.md L11.2,
/// L4.2; the coordinator's ruling LM-51). A `log` body that would take a run past it drops what the value holds of
/// that run, which then reads as a lost run. The bound counts distinct pairs of a sequence number and a held line, so
/// a line that the run held already does not count again, and it holds across all the bodies of the run.
pub const LINES_HELD_MAX: usize = 4_096;

/// The class rule that admission compares with: `class_of` of halvo-policy (AG-A4.3).
pub(crate) type ClassRule = fn(SeedCategory, &ExecutableStep) -> ActionClass;

/// Why the handlers refused a request. A fault of the family `learn` behind a body that the agent store answered is
/// no refusal: the reply is the agent store's, and the learning commit is left out (contracts.md L4.2; the
/// coordinator's ruling LM-58 (b)).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// The error of the reply form (W-C3.2), the agent store's own or the handlers'.
    Error(ErrorName),
}

impl Refused {
    /// The error name of the reply.
    pub fn error_name(self) -> ErrorName {
        match self {
            Refused::Error(name) => name,
        }
    }
}

/// Why the learning part of a request left its commit out: an error of a read, a write or the commit, or
/// `check_sources` answered `source` for a row of the commit (contracts.md L1.5, L3.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fault {
    Error(ErrorName),
    Source,
}

impl From<ErrorName> for Fault {
    fn from(name: ErrorName) -> Fault {
        Fault::Error(name)
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
/// for a request, after the agent store's own. A run holds at most one admitted proposal for each step: a second
/// admitted proposal for a step of a run replaces the first, and a refused one leaves the first in place.
///
/// A host gives one value every `runner` and every `coordinator_write` request for its life; a new value is a
/// restart. A value makes facts at a `load` only for a run whose `learn_run` rev 1 it stored itself at the run's
/// `start_run`; every other run is a lost run: its `load_steps` get the compiled steps unchanged, each of its
/// proposals is answered `base`, its `finish` makes no commit and its `learn_run` stays at rev 1. A run with no
/// `load` before its `finish` is lost the same way. The number that a run loaded (L) is that of the reply to its
/// first `load`; a later `load` adds the slot values of its reply and changes nothing else, and slot values are kept
/// for a run of the mode `replay` alone. A replay `put_playbook` names L or L plus 1, and every read for a step is by
/// L; any other number, or L no longer the agent's newest, answers each differing step `base`, holds nothing and
/// answers the count 0. A step for which no `load_steps` reply to the run, under L, held a step of its `n` counts as
/// differing and is answered `base`. The facts and the remembered start of a run go at its answered `finish`, at an
/// `end_run` of the run under its agent whatever the agent store answers, and when the handlers' own `load` of the
/// run is refused with a name other than `store` and `internal`: they send that `load` after the agent store
/// refused another runner body of a run with facts, and for every run with facts at each `start_run` that the agent
/// store accepts. At most `RUNS_HELD_MAX` runs are held; when one more would be made, the one made first goes first.
/// A run whose `log` bodies would take the lines held for it past `LINES_HELD_MAX` is dropped and reads as a lost run.
/// An `end_run` that the agent store accepts for a run with facts under its agent makes the commit that the `finish`
/// of that run with the outcome `failed` would make, with the newest compile number that the handlers read by their
/// own `load` of the run before they pass the body on (contracts.md L4.3; the coordinator's ruling LM-61).
///
/// A row of the family `learn` that its parser refuses stops what rests on its kind in the workflow that it names, or
/// in every workflow where that name cannot be read, and nothing else (contracts.md L4.2; the coordinator's ruling
/// LM-58 (a), (d)): a `learn_edit` row the edits of that workflow, whose runs are served the compiled steps, get no
/// candidate and no move and store a rev 2 with `kept` 0; a `learn_run` row its run rows, its runs being lost runs whose
/// mode comes from the remembered start; a `learn_routine` row its routine rows. A body that the agent store answered
/// gets the agent store's reply whatever the learning part could read or store, and a replay `put_playbook` its count;
/// `list_refinements` alone answers `store` (LM-58 (b)). An accepted `start_run` also stores the pair's `learn_routine` row
/// where a read that succeeded holds no row of the pair, and its newest row again at 30 days with the mark `none` (L4.3;
/// S-69, LM-57). A step whose
/// `load_steps` replies to a run, under the number that it loaded, carried different edits gets no `use_ok` in that run,
/// and a counted failure of it is laid on the edit of the last reply (L3.7; LM-60).
pub struct LearnHandlers {
    pub(crate) ids: Ids,
    pub(crate) facts: BTreeMap<RunId, RunFacts>,
    pub(crate) starts: BTreeMap<RunId, Start>,
    pub(crate) made: u64,
    pub(crate) class_rule: ClassRule,
    #[cfg(test)]
    pub(crate) probes: Vec<RunId>,
    #[cfg(test)]
    pub(crate) faults: Vec<Fault>,
}

impl Default for LearnHandlers {
    fn default() -> LearnHandlers {
        LearnHandlers {
            ids: Ids::default(),
            facts: BTreeMap::new(),
            starts: BTreeMap::new(),
            made: 0,
            class_rule: class_of,
            #[cfg(test)]
            probes: Vec::new(),
            #[cfg(test)]
            faults: Vec::new(),
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
        let call = RunnerCall::parse(body);
        let answer = match &call {
            Ok(RunnerCall::Load(b)) => self.load(host, body, b),
            Ok(RunnerCall::LoadSteps(b)) => self.load_steps(host, body, b),
            Ok(RunnerCall::PutPlaybook(b)) => self.put_playbook(host, body, b),
            Ok(RunnerCall::Log(b)) => self.log(host, body, b),
            Ok(RunnerCall::Finish(b)) => self.finish(host, body, b),
            _ => Ok(agent::runner(host, body)?),
        };
        if let (Ok(call), Err(Refused::Error(name))) = (&call, &answer) {
            let was_load = matches!(call, RunnerCall::Load(_));
            self.after_refusal(host, *name, call.agent(), call.run(), was_load);
        }
        answer
    }

    /// Leaves out the learning commit whose part ended with a fault; a test keeps the fault.
    pub(crate) fn leave(&mut self, outcome: Result<(), Fault>) {
        #[cfg(test)]
        if let Err(fault) = outcome {
            self.faults.push(fault);
        }
        #[cfg(not(test))]
        let _ = outcome;
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
