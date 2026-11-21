use halvo_schema::executable::body::{LoadReplies, LoadRepliesReply, PutReplyRow, Stored};
use halvo_schema::executable::names::{EffectState, ReplyState};
use halvo_schema::executable::{AgentReply, ProviderId, RunId, RunnerReply};
use halvo_schema::{AgentId, ErrorName, TypedRecord};

use crate::errors::Own;
use crate::host::{Host, Now};
use crate::rows::{self, record};
use crate::{bind, day};

pub(crate) fn of_message<'a>(
    all: &'a [AgentReply],
    agent: &AgentId,
    message: &ProviderId,
) -> Vec<&'a AgentReply> {
    let mut mine: Vec<&AgentReply> = all
        .iter()
        .filter(|r| &r.agent == agent && &r.message == message)
        .collect();
    mine.sort_by_key(|r| r.seq);
    mine
}

pub(crate) fn put(
    host: &mut dyn Host,
    now: Now,
    b: &PutReplyRow,
) -> Result<RunnerReply, ErrorName> {
    let (_, workflow) = bind::runner_agent(&*host, &b.agent)?;
    let row = &b.record;
    if row.workflow != workflow.id {
        return Err(Own::Binding.into());
    }
    bind::own_run(&*host, now, &b.run, &b.agent)?;
    let all = rows::replies(host.rows())?;
    let mine = of_message(&all, &b.agent, &row.message);
    if mine.last().is_some_and(|last| repeats(last, row)) {
        return Ok(stored());
    }
    bind::open_run(&*host, &b.run)?;
    if !follows(&*host, &mine, row)? {
        return Err(Own::Effect.into());
    }
    if row.state == ReplyState::Begun && others(&all, &b.agent, &row.message) == Some(Held::Sent) {
        return Err(Own::Effect.into());
    }
    day::commit(host, now, vec![record(rows::AGENT_REPLY, row.to_json())?])?;
    Ok(stored())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Held {
    Sent,
    Withheld,
}

pub(crate) fn others(all: &[AgentReply], agent: &AgentId, message: &ProviderId) -> Option<Held> {
    let mut newest: Vec<&AgentReply> = Vec::new();
    for row in all
        .iter()
        .filter(|r| &r.agent != agent && &r.message == message)
    {
        match newest.iter_mut().find(|n| n.agent == row.agent) {
            Some(n) if n.seq < row.seq => *n = row,
            Some(_) => {}
            None => newest.push(row),
        }
    }
    let holds = |states: &[ReplyState]| newest.iter().any(|r| states.contains(&r.state));
    if holds(&[
        ReplyState::Begun,
        ReplyState::Committed,
        ReplyState::Unknown,
    ]) {
        Some(Held::Sent)
    } else if holds(&[ReplyState::Withheld]) {
        Some(Held::Withheld)
    } else {
        None
    }
}

pub(crate) fn load(host: &dyn Host, now: Now, b: &LoadReplies) -> Result<RunnerReply, ErrorName> {
    bind::runner_agent(host, &b.agent)?;
    bind::own_run(host, now, &b.run, &b.agent)?;
    bind::open_run(host, &b.run)?;
    let all = rows::replies(host.rows())?;
    let mut reply = LoadRepliesReply {
        replies: Vec::new(),
        blocked: Vec::new(),
        blocked_withheld: Vec::new(),
    };
    for id in &b.message_ids {
        if let Some(newest) = of_message(&all, &b.agent, id).last() {
            reply.replies.push((*newest).clone());
        }
        match others(&all, &b.agent, id) {
            Some(Held::Sent) => reply.blocked.push(id.clone()),
            Some(Held::Withheld) => reply.blocked_withheld.push(id.clone()),
            None => {}
        }
    }
    if reply.answers(b).is_err() || reply.to_json().is_err() {
        return Err(ErrorName::Internal);
    }
    Ok(RunnerReply::Replies(reply))
}

fn stored() -> RunnerReply {
    RunnerReply::Stored(Stored { stored: 1 })
}

fn repeats(last: &AgentReply, row: &AgentReply) -> bool {
    let dated = AgentReply {
        day: row.day,
        at: row.at,
        ..last.clone()
    };
    dated == *row
}

fn follows(host: &dyn Host, mine: &[&AgentReply], row: &AgentReply) -> Result<bool, ErrorName> {
    let Some((last, earlier)) = mine.split_last() else {
        return Ok(row.seq.get() == 1 && row.state == ReplyState::Drafted && row.effect.is_none());
    };
    if Some(row.seq.get()) != last.seq.get().checked_add(1) {
        return Ok(false);
    }
    let same_send = row.draft == last.draft && row.run == last.run;
    Ok(match (last.state, row.state) {
        (ReplyState::Drafted, ReplyState::Begun) => row.draft == last.draft,
        (ReplyState::Drafted, ReplyState::Lost) => true,
        (ReplyState::Drafted, ReplyState::Withheld) => made_by_create_reply(earlier) && same_send,
        (ReplyState::Lost, ReplyState::Drafted) => {
            row.effect.is_none() && mine.iter().all(|r| r.draft != row.draft)
        }
        (ReplyState::Begun, ReplyState::Committed | ReplyState::Unknown) => {
            same_send && row.effect == last.effect
        }
        (ReplyState::Begun, ReplyState::Drafted) => {
            row.draft == last.draft && row.effect == last.effect && not_made_of_run(host, row)?
        }
        _ => false,
    })
}

fn made_by_create_reply(earlier: &[&AgentReply]) -> bool {
    earlier.last().is_none_or(|r| r.state == ReplyState::Lost)
}

fn not_made_of_run(host: &dyn Host, row: &AgentReply) -> Result<bool, ErrorName> {
    let Some(effect) = &row.effect else {
        return Ok(false);
    };
    let newest = rows::effects(host.rows())?
        .into_iter()
        .filter(|e| &e.effect == effect && e.run == row.run)
        .max_by_key(|e| e.seq);
    Ok(newest.is_some_and(|e| not_made(e.state)))
}

/// Whether a reply row may go back from `begun` to `drafted`: only when its effect's newest row is `not_made`, which
/// the runner alone writes for an effect that it knows was not made (A7.4, CP-141 (2)).
fn not_made(state: EffectState) -> bool {
    match state {
        EffectState::NotMade => true,
        EffectState::Refused
        | EffectState::Cleared
        | EffectState::Held
        | EffectState::Approved
        | EffectState::Rejected
        | EffectState::Releasing
        | EffectState::Committed
        | EffectState::Unknown => false,
    }
}

pub(crate) fn ended(host: &dyn Host, run: &RunId, now: Now) -> Result<Vec<TypedRecord>, ErrorName> {
    let all = rows::replies(host.rows())?;
    let mut out = Vec::new();
    for row in all
        .iter()
        .filter(|r| &r.run == run && r.state == ReplyState::Begun)
    {
        let newest = of_message(&all, &row.agent, &row.message);
        if newest.last() != Some(&row) {
            continue;
        }
        let unknown = AgentReply {
            seq: row.seq.checked_add(1).ok_or(Own::Length)?,
            day: day::of(host, now),
            at: now.at,
            state: ReplyState::Unknown,
            ..row.clone()
        };
        out.push(record(rows::AGENT_REPLY, unknown.to_json())?);
    }
    Ok(out)
}
