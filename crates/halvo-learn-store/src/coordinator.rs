use std::num::NonZeroU32;

use halvo_schema::executable::body::{CreateAgent, StartRun};
use halvo_schema::executable::{CoordinatorCall, CoordinatorReply, RunId};
use halvo_schema::learning::body::{LearnedRead, Refinement, Refinements};
use halvo_schema::learning::names::{Mark, RefinementKind};
use halvo_schema::learning::record::{Day, EditState, LearnRoutine, LearnRun, RunStatus};
use halvo_schema::{ErrorName, Workflow};

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
        let now = host.agent_now();
        let reply = halvo_agent_store::coordinator(host.agent_host(), now, body);
        if let Some(CoordinatorCall::EndRun(b)) = &call {
            let of_agent = self.facts.get(&b.run).is_some_and(|f| f.agent == b.agent)
                || self.starts.get(&b.run).is_some_and(|s| s.agent == b.agent);
            if of_agent {
                self.drop_run(&b.run);
            }
        }
        let reply = reply?;
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
        let routine = routines
            .into_iter()
            .filter(|r| r.workflow == read.workflow && r.agent == read.agent)
            .max_by_key(|r| r.rev);
        if let Some(LearnRoutine {
            mark: Mark::Stale,
            since: Some(since),
            ..
        }) = routine
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
    /// At a `start_run` that the agent store accepted: stores the run's `learn_run` rev 1 and remembers the start
    /// (contracts.md L4.3). A run whose row stands already changes nothing. Where the run rows of the workflow cannot
    /// be read, or the commit is refused, the start is remembered with no row and the run is a lost run.
    fn started(&mut self, host: &mut dyn LearnHost, b: &StartRun) -> Result<(), Fault> {
        let Read::Rows(runs) = rows::runs(host.rows(), &b.workflow) else {
            self.remember_start(b, false);
            return Ok(());
        };
        if rows::newest_run(&runs, b.run.as_str()).is_some() {
            return Ok(());
        }
        let stored = today(host).and_then(|day| {
            let row = LearnRun {
                workflow: b.workflow.clone(),
                agent: b.agent.clone(),
                run: b.run.clone(),
                day,
                mode: b.mode,
                status: RunStatus::Started,
            };
            host.commit(vec![rows::run_record(&row)?])
        });
        self.remember_start(b, stored.is_ok());
        Ok(stored?)
    }
}

fn created(host: &mut dyn LearnHost, b: &CreateAgent) -> Result<(), Fault> {
    let Read::Rows(routines) = rows::routines(host.rows(), &b.workflow) else {
        return Ok(());
    };
    let stored = routines
        .iter()
        .any(|r| r.workflow == b.workflow && r.agent == b.agent);
    if stored {
        return Ok(());
    }
    let Some(Workflow::Full(w)) = host.workflow(&b.workflow).map(|item| item.workflow) else {
        return Ok(());
    };
    let row = LearnRoutine {
        workflow: b.workflow.clone(),
        agent: b.agent.clone(),
        rev: NonZeroU32::MIN,
        day: today(host)?,
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
    };
    Ok(host.commit(vec![rows::routine_record(&row)?])?)
}
