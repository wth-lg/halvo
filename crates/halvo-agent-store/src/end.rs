use std::collections::BTreeMap;

use halvo_policy::{effect_step, Effect, EffectMove};
use halvo_schema::executable::body::{Accepted, EndRun, Finish, Stored};
use halvo_schema::executable::names::{Cause, EffectState, Outcome};
use halvo_schema::executable::text::V1;
use halvo_schema::executable::{
    AgentEffect, AgentRun, CoordinatorReply, EffectId, RunId, RunnerReply,
};
use halvo_schema::{ErrorName, TypedRecord};

use crate::bind;
use crate::effect::rebinds;
use crate::errors::Own;
use crate::host::{Host, Now};
use crate::rows::{self, record};

pub(crate) fn finish(host: &mut dyn Host, now: Now, b: &Finish) -> Result<RunnerReply, ErrorName> {
    let (_, workflow) = bind::runner_agent(&*host, &b.agent)?;
    if b.record.workflow != workflow.id {
        return Err(Own::Binding.into());
    }
    bind::own_run(&*host, &b.run, &b.agent)?;
    match rows::runs(host.rows())?
        .into_iter()
        .find(|r| r.run == b.run)
    {
        Some(stored) if stored == b.record => {}
        Some(_) => return Err(Own::Effect.into()),
        None => {
            let mut records = vec![record(rows::AGENT_RUN, b.record.to_json())?];
            records.extend(moves(&*host, &b.run, now)?);
            host.commit(records)?;
        }
    }
    Ok(RunnerReply::Stored(Stored { stored: 1 }))
}

pub(crate) fn end_run(
    host: &mut dyn Host,
    now: Now,
    b: &EndRun,
) -> Result<CoordinatorReply, ErrorName> {
    bind::of_agent(&*host, &b.workflow, &b.agent)?;
    bind::own_run(&*host, &b.run, &b.agent)?;
    if !rows::runs(host.rows())?.iter().any(|r| r.run == b.run) {
        let run = AgentRun {
            v: V1,
            workflow: b.workflow.clone(),
            agent: b.agent.clone(),
            run: b.run.clone(),
            day: now.day,
            mode: None,
            outcome: Outcome::Failed,
            playbook: None,
            steps: 0,
            prompts: 0,
            handbacks: 0,
            confirmations: 0,
            effects: 0,
            unknown: 0,
            model_calls: 0,
            tokens_in: 0,
            tokens_out: 0,
            wall_s: 0,
            model: None,
        };
        let mut records = vec![record(rows::AGENT_RUN, run.to_json())?];
        records.extend(moves(&*host, &b.run, now)?);
        host.commit(records)?;
    }
    Ok(CoordinatorReply::Accepted(Accepted {}))
}

fn moves(host: &dyn Host, run: &RunId, now: Now) -> Result<Vec<TypedRecord>, ErrorName> {
    let all = rows::effects(host.rows())?;
    let mut of_run: BTreeMap<&EffectId, Vec<&AgentEffect>> = BTreeMap::new();
    for row in all.iter().filter(|r| &r.run == run) {
        of_run.entry(&row.effect).or_default().push(row);
    }
    let mut out = Vec::new();
    for mut rows_of in of_run.into_values() {
        rows_of.sort_by_key(|r| r.seq);
        let Some(last) = rows_of.last() else {
            continue;
        };
        let (step, approval, cause) = match last.state {
            EffectState::Held | EffectState::Cleared | EffectState::Approved => {
                (EffectMove::End, None, Some(Cause::Ended))
            }
            EffectState::Releasing => (EffectMove::Lose, last.approval.clone(), None),
            _ => continue,
        };
        let effect = Effect {
            id: last.effect.clone(),
            state: last.state,
            class: last.class,
            digest: last.digest.clone(),
            rebinds: rebinds(&rows_of),
        };
        let state = effect_step(Some(&effect), step).map_err(|_| Own::Effect)?;
        let next = AgentEffect {
            seq: last.seq.checked_add(1).ok_or(Own::Length)?,
            day: now.day,
            at: now.at,
            state,
            approval,
            cause,
            ..(*last).clone()
        };
        out.push(record(rows::AGENT_EFFECT, next.to_json())?);
    }
    Ok(out)
}
