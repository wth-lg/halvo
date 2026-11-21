use std::collections::BTreeMap;
use std::num::NonZeroU32;

use halvo_policy::check_playbook;
use halvo_schema::executable::body::{
    Load, LoadReply, LoadSession, LoadSteps, LoadedBindings, LoadedPlaybook, LoadedSession, PutLog,
    PutPlaybook, PutSession, SessionReply, StepsReply, Stored, STEPS_PER_REQUEST_MAX,
};
use halvo_schema::executable::names::{Cause, LogEvent};
use halvo_schema::executable::text::V1;
use halvo_schema::executable::{
    AgentPlaybook, AgentSession, ExecutableStep, RunnerCall, RunnerReply, WebOrigin,
};
use halvo_schema::{AgentId, ErrorName, RowRead};

use crate::errors::{refused, Own};
use crate::host::{Host, Now};
use crate::rows::{self, record};
use crate::{bind, checkpoint, day, effect, end, reply};

/// Answers one runner body of A7.1, read from its bytes with the typed parser: the reply, or the error name of the
/// first check that refuses it, with nothing stored. A body that is not the form is `malformed`, and one past a
/// bound of A7.1 `length`. Every body is refused with `binding` unless the agent's configuration is stored and the
/// agent is its workflow's, and a body whose record names another workflow is refused with `binding`. A run belongs
/// to the agent whose start row `start_run` stored (CP-135 (5)): every body is refused with `binding` for a run of
/// which a stored effect row, log line or `agent_run` record names another agent, and for a run of which no start
/// row of the body's agent stands, from 93 days back to two days ahead of the request's day, the rows that an expiry
/// at that day leaves (CP-122 (g)). Once the run's `agent_run` record is
/// stored, by `finish` or `end_run`, every body that names the run is refused with `effect`, but for the repeat
/// of the run's own `finish` and an `effect` row equal to the effect's last row, which are answered as the first.
/// Then:
///
/// - `load`: the stored configuration; the newest finished playbook number with the count of steps that its compile
///   finished, by `finished`, or null; `next_playbook`, the number that a new playbook is stored under: the newest
///   number of a stored step or a finished playbook plus 1, or 1 where there is none (CP-182); the newest revision
///   of the bindings; for each origin, in byte order, the newest session revision whose parts 1 to `of` are all
///   stored, with `of` as its count of parts; the agent's start row of the configuration's workflow, the earliest by
///   `at` where more than one is stored, or null where none is.
/// - `load_steps`: `playbook` where the number is not finished, by `finished`, or no step of it is stored; else the
///   agent of the stored steps of that number, read before the cut by `from`, and at most 16 stored steps from the
///   one numbered `from`, in order and with no gap, none past the count of steps that the compile finished.
/// - `load_session`: `binding` where the part is not stored; else the part with the agent of its row.
/// - `put_playbook`: `playbook` unless the number is the newest stored number or the next one (1 for the first),
///   for a first step after a gap in the steps of that number, and for a step whose number and `n` hold another
///   stored step; where `check_playbook` refuses the steps of that number that would be held, `playbook` for its
///   cause `workbook`, a write to a file that is not the slot `workbook` (CP-218 (c)), and `class` for any other
///   cause. It stores each step that is not stored and answers the count of the body's steps.
/// - `put_session`: `binding` where the place of the part (origin, revision, part) holds a part with another `of`
///   or other data; else it stores the part, with the request's day, unless it is stored, and answers 1.
/// - `log`: `effect` for a body with a record of the event `start_accepted` or of `seq` 1, the start row's place;
///   else it stores each record that is not stored and answers the count of the body's records.
/// - `effect`: `binding` where a stored row of the effect id names another workflow, agent or run. A row equal to
///   the effect's last row by seq in every key but `day` and `at` is a repeat, answered as the first,
///   `{"stored":1}`, and stores nothing (CP-132 (e)). Else `effect` for every row of an ended run, and for every row
///   that A5.2 does not allow. A first row has seq 1 and the state that
///   `effect_step` gives for `propose` with `decide` for the agent's stored configuration and the row's class, with
///   the checkpoint `yes` for a `cleared` row and `no` for any other. A later row has the next seq and the effect's
///   class, and its state names the move: `held` a `rebind` with the row's digest and class; `approved` an
///   `approve` whose entry is the row's effect, with the row's digest and `used` when any stored row of any
///   effect holds the row's approval; `rejected` with the cause `ended` an `end`, with the cause `reproposed` an
///   `end` after a `cleared` row alone, else a `reject`; `releasing` a `begin` with the row's digest; `committed` a
///   `commit`; `not_made` a `not_made` with the row's cause; `refused`, `cleared` and `unknown` no move. A row of a
///   move other than `rebind` keeps the digest and the kept fields of the last row. `effect_step` takes the effect
///   as its last row stands, with its count of `held` rows after the first as its count of rebinds, and must give
///   the row's state. A `releasing`, `committed` or `not_made` row holds the approval of the effect's last
///   `approved` row, or none where the effect has no `approved` row, else `effect` (CP-132 (a)). A `releasing` row
///   is refused with `effect` when `decide` for the agent's newest stored configuration and the effect's class,
///   with the checkpoint of its first row, gives `refuse` (CP-143 (4) (ii)).
/// - `checkpoint`: a row equal to the checkpoint's newest row (its run and workbook) in every key but `day` and
///   `at` is answered as the first and stores nothing; else `effect` unless the row is a `planned` first row of the
///   checkpoint or a `made` row after its `planned` row with the same workflow, agent, folder and copy and an `at`
///   not before that row's (CP-205 (b)); `length` for the `planned` first row of a new path when the run's
///   checkpoints with it, each in its widest listed form, would not fit one page of `list_checkpoints`, so that one
///   run's checkpoints always fit a page (CP-205 C2). The store's crate reads the row with the schema's strict
///   reader and stores the text of its writer.
/// - Every row that a body stores is held to the day rule (CP-122): a row that holds `at` (`effect`, `log`) is
///   refused with `day` unless its `day` is `Host::day` of its `at`, and every row of a bounded kind, the record of
///   `finish` among them, is refused with `day` when its day is after the day of the request's instant or already
///   outside its kind's bound; a refused body stores nothing. A row that the handler builds itself takes the
///   request's instant and its day. A repeat that stores nothing is answered before the day is read.
/// - `finish`: where the run's record is stored, `{"stored":1}` for the same record and `effect` for another.
///   Else `effect` for a record with the outcome `completed` while an effect of the run is `held`, `cleared`,
///   `approved`, `releasing` or `not_made`, and for one whose `unknown` is below the count of the run's effects
///   that are `unknown` after the moves (CP-132 (b)); the keys of the run's list are stored as given, with no
///   relation over them. Else `playbook` for a record of the mode `compile` with the outcome `completed` whose
///   `playbook` is not the agent's newest stored playbook number (CP-182 (2)). Else, in one commit, the record; for
///   such a compile the row `agent_finished` of that number with its count of stored steps and the run's id, unless
///   a row of the number stands; and the moves of the run's end: for each effect of the run whose last row is
///   `held`, `cleared` or `approved` a row `rejected` with the cause `ended` and no approval, and for each one that
///   is `releasing` a row `unknown` that keeps its approval, each with the next seq and the request's day and
///   instant; and for each message whose newest reply row of its agent names the run and is `begun`, a reply row
///   `unknown` with the next seq, the request's day and instant, and the rest of that row.
/// - `reply_row`: a row equal to the newest row of the message of the body's agent (by `seq`) in every key but
///   `day` and `at` is a repeat, answered as the first, `{"stored":1}`, before the run's end is read. Else `effect`
///   for a row of an ended run and for a row that the closed order of A7.4 does not allow: a first row is
///   `drafted` with seq 1 and no effect; a later row has the next seq and follows the newest row as `begun` with
///   its draft or as `lost` after `drafted`; as `withheld` with its draft and run after a `drafted` row that is
///   the first row or follows `lost`; as `drafted` with a draft that no row of the message holds and no effect
///   after `lost`; as `committed` or `unknown` with its draft, run and effect after `begun`; as `drafted` after
///   `begun` with its draft and effect only when that effect's newest row of the row's run is `not_made`, which the
///   runner alone writes; nothing follows `committed`, `unknown` or `withheld`. Then
///   `effect` for a `begun` row while another agent's newest row of the message is `begun`, `committed` or
///   `unknown`, by the same function that gives the list `blocked` of `load_replies`. It stores the row. A named
///   limit: a clock set back more than two days past the day of a run's end record leaves that record out of the
///   read, so a `begun` row of the ended run is stored and stays `begun` (CP-227).
/// - `load_replies`: for each id asked, in the order asked, the newest row of the body's agent; in `blocked` each
///   id for which another agent's newest row, over every agent's rows, is `begun`, `committed` or `unknown`, and
///   in `blocked_withheld` each other id for which another agent's newest row is `withheld`; no row and no key of
///   another agent. `internal` for a reply that does not answer the body or that its writer refuses, so that no
///   reply holds fewer rows than the store has for the ids asked.
pub fn runner(host: &mut dyn Host, now: Now, body: &[u8]) -> Result<RunnerReply, ErrorName> {
    match RunnerCall::parse(body).map_err(refused)? {
        RunnerCall::Load(b) => load(&*host, now, &b),
        RunnerCall::LoadSteps(b) => load_steps(&*host, now, &b),
        RunnerCall::LoadSession(b) => load_session(&*host, now, &b),
        RunnerCall::PutPlaybook(b) => put_playbook(host, now, &b),
        RunnerCall::PutSession(b) => put_session(host, now, &b),
        RunnerCall::Effect(b) => effect::put(host, now, &b),
        RunnerCall::Log(b) => put_log(host, now, &b),
        RunnerCall::Finish(b) => end::finish(host, now, &b),
        RunnerCall::Checkpoint(b) => checkpoint::put(host, now, &b),
        RunnerCall::ReplyRow(b) => reply::put(host, now, &b),
        RunnerCall::LoadReplies(b) => reply::load(&*host, now, &b),
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

/// The agent's newest finished playbook number with the count of steps that its compile finished, or none where no
/// compile of the agent finished: the key `playbook` of the reply of `load`, by the same function. It refuses with
/// `binding`, as `load` does, unless the agent's configuration is stored and the agent is its workflow's; it stores
/// nothing, writes no log line and names no run, so that a party that is no runner reads the number without a
/// runner body, which CP-135 (5) refuses for a run with no start row. `store` where a stored row of the family is
/// not the form.
pub fn newest_playbook(
    host: &dyn Host,
    agent: &AgentId,
) -> Result<Option<LoadedPlaybook>, ErrorName> {
    bind::runner_agent(host, agent)?;
    newest_finished(host.rows(), agent)
}

/// The agent's finished playbooks (CP-182): each number for which a row `agent_finished` of the agent stands, with
/// the count of steps of that row, the first row by the store's order where two rows name one number. It is the one
/// function of A7.1 that `load`, `newest_playbook`, `load_steps`, the test of `set_state` for `active` and the
/// roster of `list_agents` read: a stored step of a number with no such row, or past the count of that row, is no
/// step of a finished playbook.
pub(crate) fn finished(
    rows: &dyn RowRead,
    agent: &AgentId,
) -> Result<BTreeMap<NonZeroU32, NonZeroU32>, ErrorName> {
    let mut out = BTreeMap::new();
    for row in rows::finished(rows)?
        .into_iter()
        .filter(|f| &f.agent == agent)
    {
        out.entry(row.number).or_insert(row.steps);
    }
    Ok(out)
}

fn newest_finished(
    rows: &dyn RowRead,
    agent: &AgentId,
) -> Result<Option<LoadedPlaybook>, ErrorName> {
    Ok(finished(rows, agent)?
        .pop_last()
        .map(|(number, steps)| LoadedPlaybook { number, steps }))
}

fn own_sessions(host: &dyn Host, agent: &AgentId) -> Result<Vec<AgentSession>, ErrorName> {
    Ok(rows::sessions(host.rows())?
        .into_iter()
        .filter(|s| &s.agent == agent)
        .collect())
}

fn load(host: &dyn Host, now: Now, b: &Load) -> Result<RunnerReply, ErrorName> {
    let (config, workflow) = bind::runner_agent(host, &b.agent)?;
    bind::own_run(host, now, &b.run, &b.agent)?;
    bind::open_run(host, &b.run)?;
    let playbook = newest_finished(host.rows(), &b.agent)?;
    let stored = own_steps(host, &b.agent)?.into_iter().map(|p| p.number);
    let newest = stored.chain(playbook.as_ref().map(|p| p.number)).max();
    let next_playbook = match newest {
        Some(number) => number.checked_add(1).ok_or(Own::Length)?,
        None => NonZeroU32::MIN,
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
        next_playbook,
        bindings,
        sessions,
        start: rows::start_of(host.rows(), &b.agent)?.filter(|s| s.workflow == workflow.id),
    }))
}

/// For each origin of the rows, in byte order, the newest session revision whose parts 1 to `of` are all stored,
/// with `of` as its count of parts: the key `sessions` of `load`, and `session` of the roster, `yes` where it holds
/// one.
pub(crate) fn newest_sessions(rows: &[AgentSession]) -> Vec<LoadedSession> {
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

fn load_steps(host: &dyn Host, now: Now, b: &LoadSteps) -> Result<RunnerReply, ErrorName> {
    bind::runner_agent(host, &b.agent)?;
    bind::own_run(host, now, &b.run, &b.agent)?;
    bind::open_run(host, &b.run)?;
    let finished = finished(host.rows(), &b.agent)?;
    let count = finished.get(&b.number).ok_or(Own::Playbook)?;
    let mut steps: Vec<AgentPlaybook> = own_steps(host, &b.agent)?
        .into_iter()
        .filter(|p| p.number == b.number && p.n <= *count)
        .collect();
    let Some(agent) = steps.first().map(|p| p.agent.clone()) else {
        return Err(Own::Playbook.into());
    };
    steps.sort_by_key(|p| p.n);
    let steps = steps
        .into_iter()
        .filter(|p| p.n >= b.from)
        .zip(b.from.get()..)
        .take_while(|(p, want)| p.n.get() == *want)
        .map(|(p, _)| p.step)
        .take(STEPS_PER_REQUEST_MAX)
        .collect();
    Ok(RunnerReply::Steps(StepsReply { agent, steps }))
}

fn load_session(host: &dyn Host, now: Now, b: &LoadSession) -> Result<RunnerReply, ErrorName> {
    bind::runner_agent(host, &b.agent)?;
    bind::own_run(host, now, &b.run, &b.agent)?;
    bind::open_run(host, &b.run)?;
    let row = own_sessions(host, &b.agent)?
        .into_iter()
        .find(|s| s.origin == b.origin && s.rev == b.rev && s.part == b.part)
        .ok_or(Own::Binding)?;
    Ok(RunnerReply::Session(SessionReply {
        agent: row.agent,
        origin: row.origin,
        rev: row.rev,
        part: row.part,
        of: row.of,
        data: row.data,
    }))
}

fn put_playbook(host: &mut dyn Host, now: Now, b: &PutPlaybook) -> Result<RunnerReply, ErrorName> {
    let (config, workflow) = bind::runner_agent(&*host, &b.agent)?;
    bind::own_run(&*host, now, &b.run, &b.agent)?;
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
    check_playbook(config.category, &steps).map_err(|refusal| match refusal.cause() {
        Cause::Workbook => Own::Playbook,
        _ => Own::Class,
    })?;
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
    bind::own_run(&*host, now, &b.run, &b.agent)?;
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
                day: day::of(&*host, now),
                data: b.data.clone(),
            };
            day::commit(host, now, vec![record(rows::AGENT_SESSION, row.to_json())?])?;
        }
    }
    Ok(stored(1))
}

fn put_log(host: &mut dyn Host, now: Now, b: &PutLog) -> Result<RunnerReply, ErrorName> {
    let (_, workflow) = bind::runner_agent(&*host, &b.agent)?;
    if b.records.iter().any(|r| r.workflow != workflow.id) {
        return Err(Own::Binding.into());
    }
    bind::own_run(&*host, now, &b.run, &b.agent)?;
    if b.records
        .iter()
        .any(|r| r.event == LogEvent::StartAccepted || r.seq.get() == 1)
    {
        return Err(Own::Effect.into());
    }
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
        day::commit(host, now, new)?;
    }
    Ok(stored(b.records.len()))
}
