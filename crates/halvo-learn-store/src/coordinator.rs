use std::num::NonZeroU32;

use halvo_schema::executable::body::{CreateAgent, StartRun};
use halvo_schema::executable::{CoordinatorCall, CoordinatorReply, RunId};
use halvo_schema::learning::body::{LearnedRead, Refinement, Refinements};
use halvo_schema::learning::names::{Mark, RefinementKind};
use halvo_schema::learning::record::{Day, EditState, LearnRoutine, LearnRun, RunStatus};
use halvo_schema::{ErrorName, Workflow};

use crate::agent;
use crate::handlers::{CoordinatorAnswer, LearnHandlers, Refused};
use crate::host::LearnHost;
use crate::rows::{self, Edits};

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
                if started(host, b)? {
                    self.remember_start(&b.run, &b.agent, b.mode);
                }
            }
            (Some(CoordinatorCall::CreateAgent(b)), CoordinatorReply::Rev(_)) => {
                created(host, b)?;
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
        let edits = Edits::read(host.rows())?;
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
        let routine = rows::routines(host.rows())?
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

/// Stores the run's `learn_run` rev 1 and answers whether this call stored it.
fn started(host: &mut dyn LearnHost, b: &StartRun) -> Result<bool, ErrorName> {
    let runs = rows::runs(host.rows())?;
    if rows::newest_run(&runs, b.run.as_str()).is_some() {
        return Ok(false);
    }
    let row = LearnRun {
        workflow: b.workflow.clone(),
        agent: b.agent.clone(),
        run: b.run.clone(),
        day: today(host)?,
        mode: b.mode,
        status: RunStatus::Started,
    };
    host.commit(vec![rows::run_record(&row)?])?;
    Ok(true)
}

fn created(host: &mut dyn LearnHost, b: &CreateAgent) -> Result<(), ErrorName> {
    let stored = rows::routines(host.rows())?
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
    host.commit(vec![rows::routine_record(&row)?])
}
