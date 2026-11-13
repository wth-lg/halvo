use std::num::NonZeroU32;

use halvo_schema::executable::body::LoadReply;
use halvo_schema::executable::names::Mode;
use halvo_schema::executable::RunId;
use halvo_schema::{AgentId, ErrorName};

use crate::agent;
use crate::facts::Start;
use crate::handlers::{LearnHandlers, RUNS_HELD_MAX};
use crate::host::LearnHost;

/// The error names of a refused body that say nothing of the run: a store fault, or the handlers' own.
pub(crate) fn of_the_store(name: ErrorName) -> bool {
    matches!(name, ErrorName::Store | ErrorName::Internal)
}

impl LearnHandlers {
    /// Remembers the start of a run whose `learn_run` rev 1 this value stored; at `RUNS_HELD_MAX` runs held, the
    /// one made first goes first.
    pub(crate) fn remember_start(&mut self, run: &RunId, agent: &AgentId, mode: Mode) {
        while self.starts.len() >= RUNS_HELD_MAX {
            let Some(first) = self
                .starts
                .iter()
                .min_by_key(|(_, start)| start.made)
                .map(|(run, _)| run.clone())
            else {
                break;
            };
            self.drop_run(&first);
        }
        self.made += 1;
        let start = Start {
            agent: agent.clone(),
            mode,
            made: self.made,
        };
        self.starts.insert(run.clone(), start);
    }

    /// Drops the run's facts and its remembered start; the run then reads as a lost run.
    pub(crate) fn drop_run(&mut self, run: &RunId) {
        self.facts.remove(run);
        self.starts.remove(run);
    }

    /// The handlers' own `load` of a run, which stores nothing. A refusal with a name other than `store` and
    /// `internal` is the sign of a run that can store nothing more, and drops the run.
    pub(crate) fn load_of(
        &mut self,
        host: &mut dyn LearnHost,
        agent: &AgentId,
        run: &RunId,
    ) -> Result<Option<LoadReply>, ErrorName> {
        let reply = agent::load(host, agent, run)?;
        if reply.is_none() {
            self.drop_run(run);
        }
        Ok(reply)
    }

    /// The agent's newest compile number by the handlers' own `load` of the run, or none.
    pub(crate) fn newest_of(
        &mut self,
        host: &mut dyn LearnHost,
        agent: &AgentId,
        run: &RunId,
    ) -> Result<Option<NonZeroU32>, ErrorName> {
        Ok(self
            .load_of(host, agent, run)?
            .and_then(|reply| reply.playbook.map(|p| p.number)))
    }

    /// The handlers' own `load` of every run that has facts, which drops those that the agent store refuses.
    pub(crate) fn sweep(&mut self, host: &mut dyn LearnHost) -> Result<(), ErrorName> {
        let held: Vec<(RunId, AgentId)> = self
            .facts
            .iter()
            .map(|(run, facts)| (run.clone(), facts.agent.clone()))
            .collect();
        for (run, agent) in held {
            self.load_of(host, &agent, &run)?;
        }
        Ok(())
    }

    /// After the agent store refused a runner body of a run: a refused `load` under the run's own agent drops the
    /// run, and for any other body of a run with facts the handlers send their own `load` of it to find out. A body
    /// under another agent never drops a run.
    pub(crate) fn after_refusal(
        &mut self,
        host: &mut dyn LearnHost,
        name: ErrorName,
        agent: &AgentId,
        run: &RunId,
        was_load: bool,
    ) {
        if of_the_store(name) {
            return;
        }
        if was_load {
            let of_agent = self.facts.get(run).is_some_and(|f| &f.agent == agent)
                || self.starts.get(run).is_some_and(|s| &s.agent == agent);
            if of_agent {
                self.drop_run(run);
            }
            return;
        }
        if let Some(agent) = self.facts.get(run).map(|f| f.agent.clone()) {
            let _ = self.load_of(host, &agent, run);
        }
    }
}
