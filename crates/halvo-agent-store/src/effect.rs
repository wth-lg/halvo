use halvo_policy::{decide, effect_step, Checkpoint, Effect, EffectMove};
use halvo_schema::executable::body::{PutEffect, Stored};
use halvo_schema::executable::names::{Cause, EffectState};
use halvo_schema::executable::{AgentConfig, AgentEffect, RunnerReply};
use halvo_schema::ErrorName;

use crate::bind;
use crate::errors::Own;
use crate::host::Host;
use crate::rows::{self, record};

pub(crate) fn put(host: &mut dyn Host, b: &PutEffect) -> Result<RunnerReply, ErrorName> {
    let (config, workflow) = bind::runner_agent(&*host, &b.agent)?;
    let row = &b.record;
    if row.workflow != workflow.id {
        return Err(Own::Binding.into());
    }
    bind::own_run(&*host, &row.run, &row.agent)?;
    let all = rows::effects(host.rows())?;
    let mut mine: Vec<&AgentEffect> = all.iter().filter(|r| r.effect == row.effect).collect();
    if mine
        .iter()
        .any(|r| r.workflow != row.workflow || r.agent != row.agent || r.run != row.run)
    {
        return Err(Own::Binding.into());
    }
    mine.sort_by_key(|r| r.seq);
    if mine.last().is_some_and(|last| {
        last.state == row.state && last.digest == row.digest && last.approval == row.approval
    }) {
        return Ok(stored());
    }
    bind::open_run(&*host, &row.run)?;
    match mine.split_first() {
        None => first(&config, row)?,
        Some((first_row, _)) => later(&all, &mine, first_row, row)?,
    }
    host.commit(vec![record(rows::AGENT_EFFECT, row.to_json())?])?;
    Ok(stored())
}

fn stored() -> RunnerReply {
    RunnerReply::Stored(Stored { stored: 1 })
}

fn first(config: &AgentConfig, row: &AgentEffect) -> Result<(), ErrorName> {
    if row.seq.get() != 1 {
        return Err(Own::Effect.into());
    }
    let checkpoint = if row.state == EffectState::Cleared {
        Checkpoint::Yes
    } else {
        Checkpoint::No
    };
    let decided = decide(config, row.class, checkpoint);
    match effect_step(None, EffectMove::Propose(decided)) {
        Ok(state) if state == row.state => Ok(()),
        _ => Err(Own::Effect.into()),
    }
}

fn later(
    all: &[AgentEffect],
    mine: &[&AgentEffect],
    first_row: &AgentEffect,
    row: &AgentEffect,
) -> Result<(), ErrorName> {
    let Some(last) = mine.last() else {
        return Err(Own::Effect.into());
    };
    if Some(row.seq.get()) != last.seq.get().checked_add(1) || row.class != first_row.class {
        return Err(Own::Effect.into());
    }
    let effect = Effect {
        id: row.effect.clone(),
        state: last.state,
        class: first_row.class,
        digest: last.digest.clone(),
        rebinds: rebinds(mine),
    };
    let step = match (row.state, row.cause) {
        (EffectState::Held, _) => EffectMove::Rebind {
            digest: row.digest.clone(),
            class: row.class,
        },
        _ if row.digest != last.digest || row.kept != last.kept => {
            return Err(Own::Effect.into());
        }
        (EffectState::Approved, _) => EffectMove::Approve {
            entry: row.effect.clone(),
            digest: row.digest.clone(),
            used: all
                .iter()
                .any(|r| r.approval.is_some() && r.approval == row.approval),
        },
        (EffectState::Rejected, Some(Cause::Ended)) => EffectMove::End,
        (EffectState::Rejected, _) => EffectMove::Reject,
        (EffectState::Releasing, _) => EffectMove::Begin {
            digest: row.digest.clone(),
        },
        (EffectState::Committed, _) => EffectMove::Commit,
        (EffectState::Refused | EffectState::Cleared | EffectState::Unknown, _) => {
            return Err(Own::Effect.into());
        }
    };
    match effect_step(Some(&effect), step) {
        Ok(state) if state == row.state => Ok(()),
        _ => Err(Own::Effect.into()),
    }
}

pub(crate) fn rebinds(mine: &[&AgentEffect]) -> u32 {
    let count = mine
        .iter()
        .skip(1)
        .filter(|r| r.state == EffectState::Held)
        .count();
    u32::try_from(count).unwrap_or(u32::MAX)
}
