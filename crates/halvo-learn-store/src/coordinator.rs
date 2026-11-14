use std::num::NonZeroU32;

use halvo_learn::RunCause;
use halvo_schema::executable::body::{CreateAgent, EndRun, StartRun};
use halvo_schema::executable::{CoordinatorCall, CoordinatorReply, RunId};
use halvo_schema::learning::body::{LearnedRead, Refinement, Refinements};
use halvo_schema::learning::names::{Mark, RefinementKind};
use halvo_schema::learning::record::{Day, EditState, LearnRoutine, LearnRun, RunStatus};
use halvo_schema::{AgentId, ErrorName, Workflow, WorkflowId};

use crate::agent;
use crate::handlers::{CoordinatorAnswer, Fault, LearnHandlers, Refused};
use crate::host::LearnHost;
use crate::rows::{self, Edits, Read};

pub(crate) fn today(host: &dyn LearnHost) -> Result<Day, ErrorName> {
    Day::new(host.today()).ok_or(ErrorName::Internal)
}

fn op_of(body: &[u8]) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(body).ok()?;
    Some(value.as_object()?.get("op")?.as_str()?.to_owned())
}

impl LearnHandlers {
    pub(crate) fn coordinator_write(
        &mut self,
        host: &mut dyn LearnHost,
        body: &[u8],
    ) -> Result<CoordinatorAnswer, Refused> {
        if op_of(body).as_deref() == Some(LearnedRead::OP) {
            let read = LearnedRead::parse(body).map_err(|_| ErrorName::Malformed)?;
            return Ok(CoordinatorAnswer::Refinements(
                self.list_refinements(host, &read)?,
            ));
        }
        let call = CoordinatorCall::parse(body).ok();
        if let Some(CoordinatorCall::EndRun(b)) = &call {
            return self.end_run(host, body, b);
        }
        let now = host.agent_now();
        let reply = halvo_agent_store::coordinator(host.agent_host(), now, body)?;
        match (&call, &reply) {
            (Some(CoordinatorCall::StartRun(b)), CoordinatorReply::Accepted(_)) => {
                self.sweep(host)?;
                let outcome = self.started(host, b);
                self.leave(outcome);
            }
            (Some(CoordinatorCall::CreateAgent(b)), CoordinatorReply::Rev(_)) => {
                let outcome = created(host, b);
                self.leave(outcome);
            }
            _ => {}
        }
        Ok(CoordinatorAnswer::Agent(reply))
    }

    /// `end_run` (contracts.md L4.3; the coordinator's ruling LM-61): for a run with facts under the body's agent the
    /// handlers first read the agent's newest compile number by their own `load` of the run, then pass the body on;
    /// where the agent store accepted, they make the commit that the `finish` of the run with the outcome `failed`
    /// would make. The run's facts and remembered start go whatever the agent store answers, and the reply is the
    /// agent store's. A refused own `load` drops the run before the body is passed on, and nothing is committed.
    fn end_run(
        &mut self,
        host: &mut dyn LearnHost,
        body: &[u8],
        b: &EndRun,
    ) -> Result<CoordinatorAnswer, Refused> {
        let held = self.facts.get(&b.run).is_some_and(|f| f.agent == b.agent);
        let newest = if held {
            self.newest_of(host, &b.agent, &b.run)
        } else {
            Ok(None)
        };
        let now = host.agent_now();
        let reply = halvo_agent_store::coordinator(host.agent_host(), now, body);
        let of_agent = self.facts.get(&b.run).is_some_and(|f| f.agent == b.agent)
            || self.starts.get(&b.run).is_some_and(|s| s.agent == b.agent);
        let mut facts = None;
        if of_agent {
            facts = self.facts.remove(&b.run);
            self.starts.remove(&b.run);
        }
        let reply = reply?;
        if let (CoordinatorReply::Accepted(_), Some(facts), Ok(newest)) = (&reply, facts, newest) {
            let outcome =
                self.commit_ended(host, &b.run, &facts, newest, &[], Err(RunCause::Failed));
            self.leave(outcome);
        }
        Ok(CoordinatorAnswer::Agent(reply))
    }

    fn list_refinements(
        &mut self,
        host: &mut dyn LearnHost,
        read: &LearnedRead,
    ) -> Result<Refinements, ErrorName> {
        let probe = RunId::from_random(self.ids.bytes());
        #[cfg(test)]
        self.probes.push(probe.clone());
        let Some(loaded) = agent::load(host, &read.agent, &probe)? else {
            return Ok(Refinements {
                refinements: Vec::new(),
            });
        };
        let newest = loaded.playbook.map(|p| p.number);
        let Read::Rows(edits) = Edits::read(host.rows(), &read.workflow) else {
            return Err(ErrorName::Store);
        };
        let Read::Rows(routines) = rows::routines(host.rows(), &read.workflow) else {
            return Err(ErrorName::Store);
        };
        let mut entries: Vec<(Refinement, String)> = Vec::new();
        for edit in edits.of_agent(&read.agent) {
            if edit.workflow != read.workflow || Some(edit.base) != newest {
                continue;
            }
            let what = match edit.state {
                EditState::Provisional => RefinementKind::TargetLearned,
                EditState::Active => RefinementKind::TargetKept,
                EditState::Reverted(_) => RefinementKind::TargetRevoked,
                EditState::Withdrawn => continue,
            };
            let entry = Refinement {
                day: edit.last,
                what,
            };
            entries.push((entry, edit.edit.as_str().to_owned()));
        }
        if let Some(&LearnRoutine {
            mark: Mark::Stale,
            since: Some(since),
            ..
        }) = rows::newest_routine(&routines, &read.workflow, &read.agent)
        {
            let entry = Refinement {
                day: since,
                what: RefinementKind::RoutineStale,
            };
            entries.push((entry, String::new()));
        }
        entries.sort_by(|(a, a_id), (b, b_id)| {
            (a.day, a.what.as_str().as_bytes(), a_id).cmp(&(
                b.day,
                b.what.as_str().as_bytes(),
                b_id,
            ))
        });
        Ok(Refinements {
            refinements: entries.into_iter().map(|(entry, _)| entry).collect(),
        })
    }
}

impl LearnHandlers {
    /// At a `start_run` that the agent store accepted (contracts.md L4.3): stores the run's `learn_run` rev 1 and
    /// remembers the start; in the same typed commit, or alone where the run's row stands already, the `learn_routine`
    /// row that `routine_due` gives. A run whose row stands already changes nothing else. Where the run rows of the
    /// workflow cannot be read, nothing is stored and the start is remembered with no row; where the commit is
    /// refused, the start is remembered with no row too, and the run is a lost run.
    fn started(&mut self, host: &mut dyn LearnHost, b: &StartRun) -> Result<(), Fault> {
        let Read::Rows(runs) = rows::runs(host.rows(), &b.workflow) else {
            self.remember_start(b, false);
            return Ok(());
        };
        let stands = rows::newest_run(&runs, b.run.as_str()).is_some();
        let stored = start_rows(host, b, stands);
        if !stands {
            self.remember_start(b, stored.is_ok());
        }
        Ok(stored?)
    }
}

fn start_rows(host: &mut dyn LearnHost, b: &StartRun, stands: bool) -> Result<(), ErrorName> {
    let day = today(host)?;
    let mut records = Vec::new();
    if !stands {
        let row = LearnRun {
            workflow: b.workflow.clone(),
            agent: b.agent.clone(),
            run: b.run.clone(),
            day,
            mode: b.mode,
            status: RunStatus::Started,
        };
        records.push(rows::run_record(&row)?);
    }
    if let Some(row) = routine_due(host, &b.workflow, &b.agent, day) {
        records.push(rows::routine_record(&row)?);
    }
    if records.is_empty() {
        return Ok(());
    }
    host.commit(records)
}

/// The `learn_routine` row that an accepted `start_run` stores for the body's workflow and agent (contracts.md L4.3;
/// the workflow run's ruling S-69; the coordinator's ruling LM-57): where a read of the routine rows that succeeded
/// holds no row of the pair, rev 1 as at `create_agent`; where the pair's newest row by `rows::newest_routine` is at
/// least `REWRITE_DAYS` days old and holds the mark `none`, that row again with `rev` + 1 and `day` today; else none,
/// also where the read stops the workflow. A row that holds `stale` is not written again.
fn routine_due(
    host: &dyn LearnHost,
    workflow: &WorkflowId,
    agent: &AgentId,
    day: Day,
) -> Option<LearnRoutine> {
    let Read::Rows(routines) = rows::routines(host.rows(), workflow) else {
        return None;
    };
    match rows::newest_routine(&routines, workflow, agent) {
        None => first_routine(host, workflow, agent, day),
        Some(row)
            if row.mark == Mark::None
                && day.get().saturating_sub(row.day.get()) >= REWRITE_DAYS =>
        {
            Some(LearnRoutine {
                rev: row.rev.checked_add(1)?,
                day,
                ..row.clone()
            })
        }
        Some(_) => None,
    }
}

/// The age in days at which an accepted `start_run` writes the pair's newest routine row again.
const REWRITE_DAYS: u32 = 30;

/// Rev 1 of the pair's routine row from the stored workflow's key, or none where the workflow is not stored whole.
fn first_routine(
    host: &dyn LearnHost,
    workflow: &WorkflowId,
    agent: &AgentId,
    day: Day,
) -> Option<LearnRoutine> {
    let Some(Workflow::Full(w)) = host.workflow(workflow).map(|item| item.workflow) else {
        return None;
    };
    Some(LearnRoutine {
        workflow: workflow.clone(),
        agent: agent.clone(),
        rev: NonZeroU32::MIN,
        day,
        category: w.category,
        site: w.site,
        app: w.app,
        period: None,
        seen: 0,
        first: None,
        last: None,
        next: None,
        missed: 0,
        mark: Mark::None,
        since: None,
    })
}

fn created(host: &mut dyn LearnHost, b: &CreateAgent) -> Result<(), Fault> {
    let Read::Rows(routines) = rows::routines(host.rows(), &b.workflow) else {
        return Ok(());
    };
    if rows::newest_routine(&routines, &b.workflow, &b.agent).is_some() {
        return Ok(());
    }
    let Some(row) = first_routine(host, &b.workflow, &b.agent, today(host)?) else {
        return Ok(());
    };
    Ok(host.commit(vec![rows::routine_record(&row)?])?)
}
