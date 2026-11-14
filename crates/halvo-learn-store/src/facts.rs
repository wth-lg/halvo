use std::collections::BTreeMap;
use std::num::NonZeroU32;

use halvo_schema::executable::body::LoadReply;
use halvo_schema::executable::names::{Cause, LogEvent, Mode};
use halvo_schema::executable::step::Locator;
use halvo_schema::executable::{ExecutableStep, OwnText};
use halvo_schema::learning::record::EditId;
use halvo_schema::{AgentId, SeedCategory, WorkflowId};

/// A run whose `learn_run` rev 1 this value stored at its `start_run`: its agent, its mode and the order in which
/// the value made it, by which the first made goes first at the bound `RUNS_HELD_MAX`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Start {
    pub(crate) agent: AgentId,
    pub(crate) mode: Mode,
    pub(crate) made: u64,
}

/// A step that a `load_steps` reply gave the run, and the edit that it carried.
pub(crate) struct Served {
    pub(crate) step: ExecutableStep,
    pub(crate) edit: Option<EditId>,
}

/// A proposal of the run that `admit` took (contracts.md L3.7, the state `candidate`): the proposed description
/// and the locators that were new against the served list.
pub(crate) struct Candidate {
    pub(crate) desc: OwnText,
    pub(crate) locators: Vec<Locator>,
}

/// A `step_ok`, `step_failed` or `recovered` line of the run's log.
#[derive(PartialEq, Eq)]
pub(crate) struct Line {
    pub(crate) event: LogEvent,
    pub(crate) step: NonZeroU32,
    pub(crate) cause: Option<Cause>,
}

/// What the handlers hold in memory for one run from its `load` to its `finish` (contracts.md L4.2). A host that
/// restarts has lost them, and the run then reads as a lost run.
pub(crate) struct RunFacts {
    pub(crate) workflow: WorkflowId,
    pub(crate) agent: AgentId,
    pub(crate) category: SeedCategory,
    pub(crate) loaded: Option<NonZeroU32>,
    pub(crate) slots: Vec<String>,
    pub(crate) served: BTreeMap<NonZeroU32, Served>,
    pub(crate) candidates: BTreeMap<NonZeroU32, Candidate>,
    pub(crate) lines: Vec<(NonZeroU32, Line)>,
}

/// The slot values of a `load` reply's bindings, kept for a run of the mode `replay` alone.
pub(crate) fn slot_values(reply: &LoadReply, mode: Mode) -> Vec<String> {
    if mode != Mode::Replay {
        return Vec::new();
    }
    reply
        .bindings
        .iter()
        .flat_map(|b| b.slots.iter())
        .map(|slot| slot.value.joined())
        .collect()
}

impl RunFacts {
    pub(crate) fn new(agent: &AgentId, reply: &LoadReply, mode: Mode) -> RunFacts {
        RunFacts {
            workflow: reply.config.workflow.clone(),
            agent: agent.clone(),
            category: reply.config.category,
            loaded: reply.playbook.as_ref().map(|p| p.number),
            slots: slot_values(reply, mode),
            served: BTreeMap::new(),
            candidates: BTreeMap::new(),
            lines: Vec::new(),
        }
    }
}
