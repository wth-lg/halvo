use std::collections::BTreeMap;
use std::num::NonZeroU32;

use halvo_policy::check_playbook;
use halvo_schema::executable::body::{
    Load, LoadReply, LoadSession, LoadSteps, LoadedBindings, LoadedPlaybook, LoadedSession, PutLog,
    PutPlaybook, PutSession, SessionReply, StepsReply, Stored, STEPS_PER_REQUEST_MAX,
};
use halvo_schema::executable::text::V1;
use halvo_schema::executable::{
    AgentPlaybook, AgentSession, ExecutableStep, RunnerCall, RunnerReply, WebOrigin,
};
use halvo_schema::{AgentId, ErrorName};

use crate::errors::{refused, Own};
use crate::host::{Host, Now};
use crate::rows::{self, record};
use crate::{bind, effect, end};

/// Answers one runner body of A7.1, read from its bytes with the typed parser: the reply, or the error name of the
/// first check that refuses it, with nothing stored. A body that is not the form is `malformed`, and one past a
/// bound of A7.1 `length`. Every body is refused with `binding` unless the agent's configuration is stored and the
/// agent is its workflow's, and a body whose record names another workflow is refused with `binding`. A run belongs
/// to the agent whose effect row, log line or `agent_run` record first names it: `effect`, `log` and `finish` are
/// refused with `binding` for a run of which a stored row names another agent. Once the run's `agent_run` record is
/// stored, by `finish` or `end_run`, every body that names the run is refused with `effect`, but for the repeat
/// of the run's own `finish` and an `effect` row equal to the effect's last row, which are answered as the first.
/// Then:
///
/// - `load`: the stored configuration; the newest playbook number with its count of stored steps; the newest
///   revision of the bindings; for each origin, in byte order, the newest session revision whose parts 1 to `of`
///   are all stored, with `of` as its count of parts.
/// - `load_steps`: `playbook` where no step of that playbook number is stored; else at most 16 stored steps from
///   the one numbered `from`, in order and with no gap.
/// - `load_session`: `binding` where the part is not stored.
/// - `put_playbook`: `playbook` unless the number is the newest stored number or the next one (1 for the first),
///   for a first step after a gap in the steps of that number, and for a step whose number and `n` hold another
///   stored step; `class` where `check_playbook` refuses the steps of that number that would be held. It stores
///   each step that is not stored and answers the count of the body's steps.
/// - `put_session`: `binding` where the place of the part (origin, revision, part) holds a part with another `of`
///   or other data; else it stores the part, with the request's day, unless it is stored, and answers 1.
/// - `log`: stores each record that is not stored and answers the count of the body's records.
/// - `effect`: `binding` where a stored row of the effect id names another workflow, agent or run. A row equal to
///   the effect's last row by seq in its state, digest and approval is answered as the first, `{"stored":1}`, and
///   stores nothing. Else `effect` for every row of an ended run, and for every row that A5.2 does not allow. A first row has seq 1 and the state that
///   `effect_step` gives for `propose` with `decide` for the agent's stored configuration and the row's class, with
///   the checkpoint `yes` for a `cleared` row and `no` for any other. A later row has the next seq and the effect's
///   class, and its state names the move: `held` a `rebind` with the row's digest and class; `approved` an
///   `approve` whose entry is the row's effect, with the row's digest and `used` when any stored row of any
///   effect holds the row's approval; `rejected` with the cause `ended` an `end`, else a `reject`; `releasing` a
///   `begin` with the row's digest; `committed` a `commit`; `refused`, `cleared` and `unknown` no move. A row of a
///   move other than `rebind` keeps the digest and the kept fields of the last row. `effect_step` takes the effect
///   as its last row stands, with its count of `held` rows after the first as its count of rebinds, and must give
///   the row's state.
/// - `finish`: where the run's record is stored, `{"stored":1}` for the same record and `effect` for another.
///   Else, in one commit, the record and the moves of the run's end: for each effect of the run whose last row is
///   `held`, `cleared` or `approved` a row `rejected` with the cause `ended` and no approval, and for each one that
///   is `releasing` a row `unknown` that keeps its approval, each with the next seq and the request's day and
///   instant.
pub fn runner(host: &mut dyn Host, now: Now, body: &[u8]) -> Result<RunnerReply, ErrorName> {
    match RunnerCall::parse(body).map_err(refused)? {
        RunnerCall::Load(b) => load(&*host, &b),
        RunnerCall::LoadSteps(b) => load_steps(&*host, &b),
        RunnerCall::LoadSession(b) => load_session(&*host, &b),
        RunnerCall::PutPlaybook(b) => put_playbook(host, &b),
        RunnerCall::PutSession(b) => put_session(host, now, &b),
        RunnerCall::Effect(b) => effect::put(host, &b),
        RunnerCall::Log(b) => put_log(host, &b),
        RunnerCall::Finish(b) => end::finish(host, now, &b),
        RunnerCall::Checkpoint(_) => Err(ErrorName::Malformed),
    }
}

fn stored(count: usize) -> RunnerReply {
    RunnerReply::Stored(Stored {
        stored: u32::try_from(count).unwrap_or(u32::MAX),
    })
}

fn own_steps(host: &dyn Host, agent: &AgentId) -> Result<Vec<AgentPlaybook>, ErrorName> {
    Ok(rows::playbooks(host.rows())?
        .into_iter()
        .filter(|p| &p.agent == agent)
        .collect())
}

fn own_sessions(host: &dyn Host, agent: &AgentId) -> Result<Vec<AgentSession>, ErrorName> {
    Ok(rows::sessions(host.rows())?
        .into_iter()
        .filter(|s| &s.agent == agent)
        .collect())
}

fn load(host: &dyn Host, b: &Load) -> Result<RunnerReply, ErrorName> {
    let (config, workflow) = bind::runner_agent(host, &b.agent)?;
    bind::open_run(host, &b.run)?;
    let steps = own_steps(host, &b.agent)?;
    let playbook = match steps.iter().map(|p| p.number).max() {
        None => None,
        Some(number) => {
            let count = steps.iter().filter(|p| p.number == number).count();
            let count = u32::try_from(count).ok().and_then(NonZeroU32::new);
            Some(LoadedPlaybook {
                number,
                steps: count.ok_or(ErrorName::Store)?,
            })
        }
    };
    let bindings = rows::bindings(host.rows())?
        .into_iter()
        .filter(|r| r.agent == b.agent && r.workflow == workflow.id)
        .max_by_key(|r| r.rev)
        .map(|r| LoadedBindings {
            rev: r.rev,
            origins: r.origins,
            slots: r.slots,
        });
    let sessions = newest_sessions(&own_sessions(host, &b.agent)?);
    Ok(RunnerReply::Load(LoadReply {
        config,
        playbook,
        bindings,
        sessions,
    }))
}

fn newest_sessions(rows: &[AgentSession]) -> Vec<LoadedSession> {
    let mut revisions: BTreeMap<(&WebOrigin, NonZeroU32), Vec<&AgentSession>> = BTreeMap::new();
    for row in rows {
        revisions
            .entry((&row.origin, row.rev))
            .or_default()
            .push(row);
    }
    let mut newest: BTreeMap<&WebOrigin, (NonZeroU32, NonZeroU32)> = BTreeMap::new();
    for ((origin, rev), parts) in revisions {
        if let Some(of) = complete(&parts) {
            newest.insert(origin, (rev, of));
        }
    }
    newest
        .into_iter()
        .map(|(origin, (rev, parts))| LoadedSession {
            origin: origin.clone(),
            rev,
            parts,
        })
        .collect()
}

fn complete(parts: &[&AgentSession]) -> Option<NonZeroU32> {
    let of = parts.first()?.of;
    let all = parts.iter().all(|p| p.of == of)
        && (1..=of.get()).all(|n| parts.iter().any(|p| p.part.get() == n));
    all.then_some(of)
}

fn load_steps(host: &dyn Host, b: &LoadSteps) -> Result<RunnerReply, ErrorName> {
    bind::runner_agent(host, &b.agent)?;
    bind::open_run(host, &b.run)?;
    let mut steps: Vec<AgentPlaybook> = own_steps(host, &b.agent)?
        .into_iter()
        .filter(|p| p.number == b.number)
        .collect();
    if steps.is_empty() {
        return Err(Own::Playbook.into());
    }
    steps.sort_by_key(|p| p.n);
    let steps = steps
        .into_iter()
        .filter(|p| p.n >= b.from)
        .zip(b.from.get()..)
        .take_while(|(p, want)| p.n.get() == *want)
        .map(|(p, _)| p.step)
        .take(STEPS_PER_REQUEST_MAX)
        .collect();
    Ok(RunnerReply::Steps(StepsReply { steps }))
}

fn load_session(host: &dyn Host, b: &LoadSession) -> Result<RunnerReply, ErrorName> {
    bind::runner_agent(host, &b.agent)?;
    bind::open_run(host, &b.run)?;
    let row = own_sessions(host, &b.agent)?
        .into_iter()
        .find(|s| s.origin == b.origin && s.rev == b.rev && s.part == b.part)
        .ok_or(Own::Binding)?;
    Ok(RunnerReply::Session(SessionReply {
        origin: row.origin,
        rev: row.rev,
        part: row.part,
        of: row.of,
        data: row.data,
    }))
}

fn put_playbook(host: &mut dyn Host, b: &PutPlaybook) -> Result<RunnerReply, ErrorName> {
    let (config, workflow) = bind::runner_agent(&*host, &b.agent)?;
    bind::open_run(&*host, &b.run)?;
    let held = own_steps(&*host, &b.agent)?;
    let newest = held.iter().map(|p| p.number.get()).max().unwrap_or(0);
    if b.number.get() != newest && Some(b.number.get()) != newest.checked_add(1) {
        return Err(Own::Playbook.into());
    }
    let mut steps: Vec<ExecutableStep> = held
        .into_iter()
        .filter(|p| p.number == b.number)
        .map(|p| p.step)
        .collect();
    let top = steps.iter().map(|s| s.n.get()).max().unwrap_or(0);
    if b.steps.first().is_some_and(|s| s.n.get() > top + 1) {
        return Err(Own::Playbook.into());
    }
    let mut new = Vec::new();
    for step in &b.steps {
        match steps.iter().find(|s| s.n == step.n) {
            Some(stored) if stored == step => {}
            Some(_) => return Err(Own::Playbook.into()),
            None => {
                steps.push(step.clone());
                new.push(step.clone());
            }
        }
    }
    steps.sort_by_key(|s| s.n);
    check_playbook(config.category, &steps).map_err(|_| Own::Class)?;
    let records = new
        .into_iter()
        .map(|step| {
            let row = AgentPlaybook {
                v: V1,
                workflow: workflow.id.clone(),
                agent: b.agent.clone(),
                number: b.number,
                n: step.n,
                step,
            };
            record(rows::AGENT_PLAYBOOK, row.to_json())
        })
        .collect::<Result<Vec<_>, _>>()?;
    if !records.is_empty() {
        host.commit(records)?;
    }
    Ok(stored(b.steps.len()))
}

fn put_session(host: &mut dyn Host, now: Now, b: &PutSession) -> Result<RunnerReply, ErrorName> {
    let (_, workflow) = bind::runner_agent(&*host, &b.agent)?;
    bind::open_run(&*host, &b.run)?;
    let held = own_sessions(&*host, &b.agent)?;
    match held
        .iter()
        .find(|s| s.origin == b.origin && s.rev == b.rev && s.part == b.part)
    {
        Some(s) if s.of == b.of && s.data == b.data => {}
        Some(_) => return Err(Own::Binding.into()),
        None => {
            let row = AgentSession {
                v: V1,
                workflow: workflow.id,
                agent: b.agent.clone(),
                origin: b.origin.clone(),
                rev: b.rev,
                part: b.part,
                of: b.of,
                day: now.day,
                data: b.data.clone(),
            };
            host.commit(vec![record(rows::AGENT_SESSION, row.to_json())?])?;
        }
    }
    Ok(stored(1))
}

fn put_log(host: &mut dyn Host, b: &PutLog) -> Result<RunnerReply, ErrorName> {
    let (_, workflow) = bind::runner_agent(&*host, &b.agent)?;
    if b.records.iter().any(|r| r.workflow != workflow.id) {
        return Err(Own::Binding.into());
    }
    bind::own_run(&*host, &b.run, &b.agent)?;
    bind::open_run(&*host, &b.run)?;
    let mut held = rows::logs(host.rows())?;
    let mut new = Vec::new();
    for line in &b.records {
        if !held.contains(line) {
            held.push(line.clone());
            new.push(record(rows::AGENT_LOG, line.to_json())?);
        }
    }
    if !new.is_empty() {
        host.commit(new)?;
    }
    Ok(stored(b.records.len()))
}
