use std::collections::BTreeMap;

use halvo_schema::executable::body::{
    CheckpointsReply, ListCheckpoints, ListedCheckpoint, MarkCheckpoint, PutCheckpoint, Stored,
};
use halvo_schema::executable::names::{
    ActionClass, CheckpointState, EffectState, FieldName, ListedState, MarkState, YesNo,
};
use halvo_schema::executable::{
    AgentCheckpoint, AgentEffect, CoordinatorReply, Digest, OwnValue, RunId, RunnerReply,
};
use halvo_schema::{ErrorName, Instant};

use crate::errors::Own;
use crate::host::{Host, Now};
use crate::rows::{self, record};
use crate::{bind, day};

type Key = (RunId, String);

fn key(run: &RunId, workbook: &OwnValue) -> Key {
    (run.clone(), workbook.joined())
}

fn by_checkpoint(all: Vec<AgentCheckpoint>) -> BTreeMap<Key, Vec<AgentCheckpoint>> {
    let mut out: BTreeMap<Key, Vec<AgentCheckpoint>> = BTreeMap::new();
    for row in all {
        out.entry(key(&row.run, &row.workbook))
            .or_default()
            .push(row);
    }
    for rows_of in out.values_mut() {
        rows_of.sort_by_key(|r| r.seq);
    }
    out
}

fn repeats(last: &AgentCheckpoint, row: &AgentCheckpoint) -> bool {
    let dated = AgentCheckpoint {
        day: row.day,
        at: row.at,
        ..last.clone()
    };
    dated == *row
}

fn follows(last: &AgentCheckpoint, row: &AgentCheckpoint) -> bool {
    last.state == CheckpointState::Planned
        && row.state == CheckpointState::Made
        && last.workflow == row.workflow
        && last.agent == row.agent
        && last.folder == row.folder
        && last.copy == row.copy
        && last.at <= row.at
}

pub(crate) fn put(
    host: &mut dyn Host,
    now: Now,
    b: &PutCheckpoint,
) -> Result<RunnerReply, ErrorName> {
    let (_, workflow) = bind::runner_agent(&*host, &b.agent)?;
    let row = &b.record;
    if row.workflow != workflow.id {
        return Err(Own::Binding.into());
    }
    bind::own_run(&*host, now, &b.run, &b.agent)?;
    bind::open_run(&*host, &b.run)?;
    let held = by_checkpoint(rows::checkpoints(host.rows())?);
    let last = held
        .get(&key(&row.run, &row.workbook))
        .and_then(|rows_of| rows_of.last());
    match last {
        Some(last) if repeats(last, row) => return Ok(stored_runner()),
        None if row.state == CheckpointState::Planned => fits_one_page(&held, row)?,
        Some(last) if follows(last, row) => {}
        _ => return Err(Own::Effect.into()),
    }
    day::commit(
        host,
        now,
        vec![record(rows::AGENT_CHECKPOINT, row.to_json())?],
    )?;
    Ok(stored_runner())
}

fn stored_runner() -> RunnerReply {
    RunnerReply::Stored(Stored { stored: 1 })
}

fn widest(row: &AgentCheckpoint) -> Result<ListedCheckpoint, ErrorName> {
    Ok(ListedCheckpoint {
        workflow: row.workflow.clone(),
        agent: row.agent.clone(),
        run: row.run.clone(),
        folder: row.folder.clone(),
        workbook: row.workbook.clone(),
        copy: row.copy.clone(),
        state: ListedState::Made,
        sha256: Some(Digest::parse(&"f".repeat(64)).map_err(|_| ErrorName::Internal)?),
        bytes: Some(u32::MAX),
        at: Instant(i64::MIN),
        ended: YesNo::Yes,
        effect: Some(EffectState::Committed),
        superseded: YesNo::Yes,
    })
}

fn fits_one_page(
    held: &BTreeMap<Key, Vec<AgentCheckpoint>>,
    row: &AgentCheckpoint,
) -> Result<(), ErrorName> {
    let mut checkpoints = Vec::new();
    for ((run, _), rows_of) in held {
        if run == &row.run {
            if let Some(first) = rows_of.first() {
                checkpoints.push(widest(first)?);
            }
        }
    }
    checkpoints.push(widest(row)?);
    let page = CheckpointsReply {
        checkpoints,
        next: Some(row.run.clone()),
    };
    page.to_json().map(|_| ()).map_err(|_| Own::Length.into())
}

fn file_of(row: &AgentEffect) -> Option<String> {
    row.kept
        .iter()
        .find(|k| k.name == FieldName::File)
        .map(|k| k.value.joined())
}

fn caution(state: EffectState) -> u8 {
    match state {
        EffectState::Unknown => 7,
        EffectState::Releasing => 6,
        EffectState::Committed => 5,
        EffectState::Approved => 4,
        EffectState::Held => 3,
        EffectState::Cleared => 2,
        EffectState::Rejected | EffectState::NotMade => 1,
        EffectState::Refused => 0,
    }
}

fn edit_states(effects: &[AgentEffect]) -> BTreeMap<Key, EffectState> {
    let mut last: BTreeMap<_, &AgentEffect> = BTreeMap::new();
    for row in effects.iter().filter(|r| r.class == ActionClass::LocalEdit) {
        let newer = last.get(&row.effect).is_none_or(|held| held.seq < row.seq);
        if newer {
            last.insert(&row.effect, row);
        }
    }
    let mut out: BTreeMap<Key, EffectState> = BTreeMap::new();
    for row in last.into_values() {
        let Some(file) = file_of(row) else {
            continue;
        };
        let slot = out.entry((row.run.clone(), file)).or_insert(row.state);
        if caution(row.state) > caution(*slot) {
            *slot = row.state;
        }
    }
    out
}

fn yes_no(yes: bool) -> YesNo {
    if yes {
        YesNo::Yes
    } else {
        YesNo::No
    }
}

pub(crate) fn list(host: &dyn Host, b: &ListCheckpoints) -> Result<CoordinatorReply, ErrorName> {
    let held = by_checkpoint(rows::checkpoints(host.rows())?);
    let edits = edit_states(&rows::effects(host.rows())?);
    let ended: Vec<RunId> = rows::runs(host.rows())?
        .into_iter()
        .map(|r| r.run)
        .collect();
    let mut runs: BTreeMap<RunId, Vec<ListedCheckpoint>> = BTreeMap::new();
    for (k, rows_of) in &held {
        let (Some(first), Some(last)) = (rows_of.first(), rows_of.last()) else {
            continue;
        };
        let state = match last.state {
            CheckpointState::Planned => ListedState::Planned,
            CheckpointState::Made => ListedState::Made,
            CheckpointState::Gone | CheckpointState::Left => continue,
        };
        if b.after.as_ref().is_some_and(|after| &last.run <= after) {
            continue;
        }
        let superseded = held.iter().any(|(other, other_rows)| {
            other_rows.first().is_some_and(|o| {
                o.run != last.run
                    && o.agent == last.agent
                    && other.1 == k.1
                    && o.at > first.at
                    && edits.get(other) == Some(&EffectState::Committed)
            })
        });
        runs.entry(last.run.clone())
            .or_default()
            .push(ListedCheckpoint {
                workflow: last.workflow.clone(),
                agent: last.agent.clone(),
                run: last.run.clone(),
                folder: last.folder.clone(),
                workbook: last.workbook.clone(),
                copy: last.copy.clone(),
                state,
                sha256: last.sha256.clone(),
                bytes: last.bytes,
                at: last.at,
                ended: yes_no(ended.contains(&last.run)),
                effect: edits.get(k).copied(),
                superseded: yes_no(superseded),
            });
    }
    Ok(CoordinatorReply::Checkpoints(page(runs)))
}

fn page(runs: BTreeMap<RunId, Vec<ListedCheckpoint>>) -> CheckpointsReply {
    let count = runs.len();
    let mut reply = CheckpointsReply {
        checkpoints: Vec::new(),
        next: None,
    };
    for (i, (run, listed)) in runs.into_iter().enumerate() {
        let mut checkpoints = reply.checkpoints.clone();
        checkpoints.extend(listed);
        let trial = CheckpointsReply {
            checkpoints,
            next: (i + 1 < count).then(|| run.clone()),
        };
        if trial.to_json().is_err() && !reply.checkpoints.is_empty() {
            return reply;
        }
        reply = trial;
    }
    reply
}

pub(crate) fn mark(
    host: &mut dyn Host,
    now: Now,
    b: &MarkCheckpoint,
) -> Result<CoordinatorReply, ErrorName> {
    bind::stored(&*host, &b.workflow)?;
    let held = by_checkpoint(rows::checkpoints(host.rows())?);
    let Some(rows_of) = held.get(&key(&b.run, &b.workbook)) else {
        return Ok(stored_coordinator(0));
    };
    if rows_of
        .iter()
        .any(|r| r.workflow != b.workflow || r.agent != b.agent)
    {
        return Err(Own::Binding.into());
    }
    let Some(last) = rows_of.last() else {
        return Ok(stored_coordinator(0));
    };
    if !matches!(last.state, CheckpointState::Planned | CheckpointState::Made) {
        return Ok(stored_coordinator(0));
    }
    let next = AgentCheckpoint {
        seq: last.seq.checked_add(1).ok_or(Own::Length)?,
        day: day::of(&*host, now),
        at: now.at,
        state: match b.state {
            MarkState::Gone => CheckpointState::Gone,
            MarkState::Left => CheckpointState::Left,
        },
        left: b.left,
        ..last.clone()
    };
    day::commit(
        host,
        now,
        vec![record(rows::AGENT_CHECKPOINT, next.to_json())?],
    )?;
    Ok(stored_coordinator(1))
}

fn stored_coordinator(n: u32) -> CoordinatorReply {
    CoordinatorReply::Stored(Stored { stored: n })
}
