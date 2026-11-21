use std::collections::BTreeMap;
use std::num::NonZeroU32;

use halvo_policy::{effect_step, Effect, EffectMove};
use halvo_schema::executable::body::{Accepted, EndRun, Finish, Stored};
use halvo_schema::executable::names::{Cause, EffectState, Mode, Outcome};
use halvo_schema::executable::text::V1;
use halvo_schema::executable::{
    AgentEffect, AgentFinished, AgentPlaybook, AgentRun, CoordinatorReply, EffectId, RunId,
    RunnerReply,
};
use halvo_schema::{ErrorName, TypedRecord};

use crate::effect::rebinds;
use crate::errors::Own;
use crate::host::{Host, Now};
use crate::rows::{self, record};
use crate::{bind, day, reply, runner};

pub(crate) fn finish(host: &mut dyn Host, now: Now, b: &Finish) -> Result<RunnerReply, ErrorName> {
    let (_, workflow) = bind::runner_agent(&*host, &b.agent)?;
    if b.record.workflow != workflow.id {
        return Err(Own::Binding.into());
    }
    bind::own_run(&*host, now, &b.run, &b.agent)?;
    match rows::runs(host.rows())?
        .into_iter()
        .find(|r| r.run == b.run)
    {
        Some(stored) if stored == b.record => {}
        Some(_) => return Err(Own::Effect.into()),
        None => {
            agrees(&*host, &b.record)?;
            let fact = finished(&*host, b)?;
            let mut records = vec![record(rows::AGENT_RUN, b.record.to_json())?];
            records.extend(fact);
            records.extend(moves(&*host, &b.run, now)?);
            records.extend(reply::ended(&*host, &b.run, now)?);
            day::commit(host, now, records)?;
        }
    }
    Ok(RunnerReply::Stored(Stored { stored: 1 }))
}

/// The row `agent_finished` that the record of a compile with the outcome `completed` brings into its commit
/// (CP-182 (2)): the record's playbook number with its count of stored steps and the run's id. `playbook` where that
/// number is not the agent's newest stored playbook number; none for any other record, and none where a row of the
/// number stands, so that each finished number has one row.
fn finished(host: &dyn Host, b: &Finish) -> Result<Option<TypedRecord>, ErrorName> {
    let r = &b.record;
    if r.mode != Some(Mode::Compile) || r.outcome != Outcome::Completed {
        return Ok(None);
    }
    let own: Vec<AgentPlaybook> = rows::playbooks(host.rows())?
        .into_iter()
        .filter(|p| p.agent == b.agent)
        .collect();
    let newest = own.iter().map(|p| p.number).max();
    let number = r
        .playbook
        .filter(|n| Some(*n) == newest)
        .ok_or(Own::Playbook)?;
    if runner::finished(host.rows(), &b.agent)?.contains_key(&number) {
        return Ok(None);
    }
    let count = own.iter().filter(|p| p.number == number).count();
    let row = AgentFinished {
        v: V1,
        workflow: r.workflow.clone(),
        agent: b.agent.clone(),
        number,
        steps: u32::try_from(count)
            .ok()
            .and_then(NonZeroU32::new)
            .ok_or(Own::Playbook)?,
        run: b.run.clone(),
    };
    Ok(Some(record(rows::AGENT_FINISHED, row.to_json())?))
}

fn agrees(host: &dyn Host, record: &AgentRun) -> Result<(), ErrorName> {
    let all = rows::effects(host.rows())?;
    let lasts = last_states(&all, &record.run);
    let open = lasts.iter().any(|s| {
        matches!(
            s,
            EffectState::Held
                | EffectState::Cleared
                | EffectState::Approved
                | EffectState::Releasing
                | EffectState::NotMade
        )
    });
    if record.outcome == Outcome::Completed && open {
        return Err(Own::Effect.into());
    }
    let unknown = lasts
        .iter()
        .filter(|s| matches!(s, EffectState::Unknown | EffectState::Releasing))
        .count();
    if usize::try_from(record.unknown).unwrap_or(usize::MAX) < unknown {
        return Err(Own::Effect.into());
    }
    Ok(())
}

fn last_states(all: &[AgentEffect], run: &RunId) -> Vec<EffectState> {
    let mut last: BTreeMap<&EffectId, &AgentEffect> = BTreeMap::new();
    for row in all.iter().filter(|r| &r.run == run) {
        let newer = last.get(&row.effect).is_none_or(|held| held.seq < row.seq);
        if newer {
            last.insert(&row.effect, row);
        }
    }
    last.into_values().map(|r| r.state).collect()
}

pub(crate) fn end_run(
    host: &mut dyn Host,
    now: Now,
    b: &EndRun,
) -> Result<CoordinatorReply, ErrorName> {
    bind::of_agent(&*host, &b.workflow, &b.agent)?;
    bind::no_other_agent(&*host, &b.run, &b.agent)?;
    if !rows::runs(host.rows())?.iter().any(|r| r.run == b.run) {
        let run = AgentRun {
            v: V1,
            workflow: b.workflow.clone(),
            agent: b.agent.clone(),
            run: b.run.clone(),
            day: day::of(&*host, now),
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
            before_start: None,
            cut: None,
            no_key: None,
            not_to_account: None,
            incomplete: None,
            reply_to_other: None,
            more_pages: None,
            held_recipient: None,
            organized: None,
            other_task: None,
            held_earlier: None,
            earlier_page: None,
        };
        let mut records = vec![record(rows::AGENT_RUN, run.to_json())?];
        records.extend(moves(&*host, &b.run, now)?);
        records.extend(reply::ended(&*host, &b.run, now)?);
        day::commit(host, now, records)?;
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
            day: day::of(host, now),
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
