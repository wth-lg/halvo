use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::num::NonZeroU32;

use halvo_policy::{preset, validate};
use halvo_schema::executable::body::{
    Accepted, AgentHistory, AgentsReply, CreateAgent, DropSessions, HistoryReply, HistoryRun,
    KeyReply, ListAgents, ListedAgent, PastCheckpoint, PastEffect, PutModelKey, RevReply, SetState,
    StandsReply, StartRun, StateReply, Stored, UnknownEffect, CHECKPOINTS_REPLY_MAX,
};
use halvo_schema::executable::names::{
    ActionClass, AgentState, Cause, CheckpointState, EffectState, FieldName, LogEvent, Mode,
    ReplyState, YesNo,
};
use halvo_schema::executable::record::Slot;
use halvo_schema::executable::text::{Name, V1};
use halvo_schema::executable::{
    reply_senders, AgentCheckpoint, AgentConfig, AgentEffect, AgentLog, AgentReply, AgentRun,
    AgentSession, AgentStart, CoordinatorCall, CoordinatorReply, EffectId, ModelKey, Outcome,
    OwnText, Refusal, RunId, Runtime,
};
use halvo_schema::{AgentId, Category, ErrorName, Instant, Workflow, WorkflowState};
use serde_json::Value;

use crate::errors::{refused, Own};
use crate::host::{Host, Now};
use crate::rows::{self, record};
use crate::workbook::{workbook_path, PathForm};
use crate::{bind, bindings, checkpoint, day, end, reply, runner};

/// Answers one coordinator body of A7.2, read from its bytes with the typed parser: the reply, or the error name of
/// the first check that refuses it. A refused body stores nothing, but for `create_agent`, whose state call comes
/// after its commit: when the state call is refused, the stored configuration stays, and a repeat of the body makes
/// the state call and stores no second configuration. A body that is not the form is `malformed`. The checks, first
/// that fits:
///
/// - `create_agent`: `no_workflow`; `state` for a workflow that is `proposed` or `dismissed`; `binding` when the
///   workflow's agent is another agent or the agent's configuration is stored for another workflow. In one commit
///   it stores the preset of the workflow's category as revision 1 unless the agent's configuration is stored, and
///   the agent's start row (`agent_start`) with the request's instant and day unless one of the agent is stored; a
///   later configuration never moves that row (CP-143 (2)). Then it makes the state call (`approved`, the agent)
///   unless the workflow's agent is this agent, and answers `{"rev":1}`. Its state call goes through the function
///   of the overlap, as that of `set_state` does: `overlap` where the agent's stored bindings overlap another agent
///   that is live, as `list_overlaps` reads them (CP-150 (1) (b)).
/// - `set_state`: `no_workflow`; `binding` unless the workflow's agent is the body's, and for `active` where no
///   configuration of the agent is stored; `playbook` for `active` while the agent's runtime is `web` and no
///   playbook of the agent is finished, by the store's one function `finished` (CP-182): a stored step of a
///   number with no row `agent_finished`, a run record with another outcome and an `end_run` are no proof;
///   `overlap` for `approved` and `active`, never for `paused`, where the agent's stored bindings overlap another
///   agent that is live, as
///   `list_overlaps` reads them, and then no state changes (CP-150 (1) (b)); then the state call and its own
///   refusal.
/// - `start_run`: `no_workflow`; `binding`; `state` unless the workflow is `approved` or `active` for `compile`
///   and `active` for `replay`; `binding` for a run of which a stored effect row, log line or `agent_run` record
///   names another agent; `effect` for a run whose `agent_run` record is stored. Where a start row of the run and
///   the agent is among the rows that the store hands out, also one past the bound of `agent_log` that the runner
///   bodies no longer count, it stores nothing, so it does not open such a run again on a host whose read still
///   hands out that start row (CP-205 (b)); else it stores the start row, one `agent_log` row with the event
///   `start_accepted`, `seq` 1, no step, cause, model or figure, and the request's instant and day. It answers
///   `{"ok":"yes"}` (CP-135 (5)). A named limit, of the class of the effects' clock limit: on a machine whose clock
///   moves by days, a read that no longer hands out the start row (a clock read three days back or 93 days on) lets
///   `start_run` store a new start row and open the run again (CP-227).
/// - `set_bindings`: `no_workflow`; `binding`; `binding` for a slot `workbook` that `workbook_path` refuses in the
///   system's form; `overlap` where the revision that it would store overlaps another agent that is live, as
///   `list_overlaps` reads it, whichever slots the body names and whatever the state of the agent's own workflow
///   (CP-150 (1) (b)). It stores the next revision of the agent's bindings, revision 1 where none is stored: each
///   slot of the body replaces the stored slot of its name in its place or is added at the end, and the other slots
///   and the origins stay. It answers the revision stored.
/// - `add_origin`: `no_workflow`; `binding`. Where the newest revision holds the origin it stores nothing and
///   answers that revision; else it stores the next revision with the origin added at the end and answers it.
/// - `list_checkpoints`: over every agent, the newest row of each checkpoint (one run and one workbook path) whose
///   newest row is `planned` or `made`, of the runs after `after`, in ascending order of run id and then of path:
///   `ended` says whether the run's `agent_run` record is stored; `effect` is the state of the run's `local_edit`
///   effect whose kept `file` is the path, by its last row, the first of `unknown`, `releasing`, `committed`,
///   `approved`, `held`, `cleared`, `rejected`, `refused` where two such effects stand, or none; `superseded` says
///   whether a checkpoint of the same agent and path in another run, whose first row is later, has such an effect
///   `committed`. A page holds whole runs while the writer takes it, and `next` names its last run where runs are
///   left.
/// - `mark_checkpoint`: `no_workflow`; `binding` where a row of the checkpoint names another workflow or agent.
///   Where the checkpoint's newest row is `planned` or `made` it stores the next row, `gone` or `left` with the
///   reason, with the digest and length of that row and the request's instant and day, and answers 1; else it
///   stores nothing and answers 0.
/// - `end_run`: `no_workflow`; `binding`; `binding` for a run of which a stored effect row, log line or
///   `agent_run` record names another agent. It needs no start row. Where the run's `agent_run` record is stored it
///   changes nothing.
///   Else, in one commit, the run's record with the outcome `failed`, the mode null, every count 0, no playbook and
///   no model, on the request's day, and the moves of the run's end as `runner` makes them for `finish`. It answers
///   `{"ok":"yes"}`.
/// - `list_overlaps`: `no_workflow`; `binding`. It stores nothing and changes no state, and answers every other agent
///   that is live and overlaps the agent's slots as they would stand with the body's slots merged into its newest
///   revision as `set_bindings` merges them, or as they stand where `slots` is null: each with its workflow, its
///   agent and the senders that it shares, in the spelling and the order of the asking agent's list, in ascending
///   order of workflow id; the first 16, and `more` `yes` where more overlap. Two agents overlap when their
///   bindings hold one account address under `account_address` and their lists under `reply_senders`, each read
///   by `reply_senders`, hold one sender in common, two addresses being one by `same_address`; a list that its
///   reader refuses lists nobody, and bindings with no `account_address` overlap nobody. The other agent's bindings
///   are its newest revision, and it is live where the stored workflow of those bindings names it as its agent and
///   is `approved` or `active` (CP-150 (1)).
/// - `list_agents`: it stores nothing and changes no state. Over every agent after `after`, in ascending order of
///   agent id, it takes the agent's newest configuration and holds the agent only where the stored workflow of that
///   configuration is of the full shape and names the agent as its agent, so no agent of a workflow that is not
///   stored, names another agent, or is stored in the dismissed shape (S-69 (d)). Each entry holds: the
///   configuration with `playbook` the newest number of `finished`, or null (CP-182); `state`, the stored state of
///   that workflow; `slots`, the slot names of the agent's newest bindings of that workflow, in their order, or none;
///   `session`, `yes` where a session revision of the agent is stored whole, as `load` lists it, and no origin's
///   revision that `load` lists is in the drop form of `drop_session`, so that the interface starts a run with a
///   window while a dropped sign-in is not kept again; `last`, the agent's
///   run record of the greatest day, then of the latest start row, then of the greatest run id; `unknown`, each
///   effect of the agent whose newest row by seq is `unknown`, in ascending order of effect id, with the run, the
///   day, the class and the kept fields of that row, whatever a record's count says; `refused_run`, the run of the
///   agent's newest start row (`start_accepted`, by `at` and then run id), ended or not and with or without a record,
///   where a log row of that run and the agent holds `step_failed` with the cause `unauthorized`, else null (CP-183);
///   `reply_unknown`, `yes` where `last` is a record and an `agent_reply` row of the agent names its run in the state
///   `unknown`, else `no`; `refused_slots`, of the bindings that `slots` reads, the names in their order of the slots
///   that their one reader refuses: `workbook` by `workbook_path` in the form of `set_bindings` and `reply_senders` by
///   `reply_senders`, a slot with no reader never; `last_cause`, the cause of the end of the agent's newest started
///   run, as `refused_run` finds that run, else null: null until the run's `agent_run` record stands, then the cause
///   of the run's `stopped` row with no step, else of its `stopped` row with a step, each whose cause is not in
///   `GOES_ON_AFTER` (`not_selected` and `amount_unread`, steps after which the run goes on, and `session_size`, a
///   sign-in over the limit that the run does not keep, at a handback's opening or before its end), else null; a
///   name of the closed list of causes alone, never a text of a page, of a site or of an error. A `stopped` row
///   whose stored cause is no name of the list reads with the cause null, so it gives `last_cause` null and does not
///   refuse the roster; every other `agent_log` row that the typed reader refuses refuses it with `store`. `window` is
///   `no` where the agent's next run needs no browser window: the run of the agent's newest start row, as `refused_run`
///   finds it, has its `agent_run` record with the outcome `completed`, and either `session` reads `yes` and that run
///   does not hold both a `handback` row and a `stopped` row with the cause `session_size`, or `session` reads `no` and
///   no run of the agent among the rows that the read hands out opened a hand-back of a sign-in: a `handback` row that
///   is neither a failed step's item, after its `step_failed` row, nor an unknown effect's, the last of a run that
///   ended `effect_unknown`; else `yes`, also where no start row stands; a yes or a no alone, never a site or a count
///   (r3 U5.15). Each key reads the agent's own rows alone and holds no slot's value. Named limits of the contract: a
///   newer start of the agent ends the sign of `refused_run`, also a run that ended before any request to the service,
///   and the sign leaves with its log row after 93 days; a newer start of the agent ends `last_cause` too, also where
///   that run will meet the same cause, and the cause leaves with its `stopped` row after 93 days; with nothing kept
///   `window` reads `no` where no run in the read's reach signed in at a hand-back, so the next run meets a credential
///   field with no window where a site begins to ask for a sign-in, and where the agent's last sign-in at a hand-back
///   lies more than 93 days back when its kept sign-in goes, by `drop_sessions` or by its 35 days; with a sign-in kept
///   it reads `no` where a site no longer takes it, and after a run that signed in at a site whose state it could not
///   store while another site's sign-in is kept; a hand-back whose own row the store refused reads as none, a failed
///   step's item whose `step_failed` row the store refused reads as a sign-in's, and a sign-in's row that is the last
///   hand-back row of a run that ended `effect_unknown`, because the store refused that run's item row, reads as the
///   item; it reads `yes` after every run that did not complete, also with a sign-in kept, and once the agent's start
///   rows leave the read after 93 days. A page holds whole entries
///   while the writer takes it, and `next` names its last agent where agents are left; `length` where the first
///   entry of the page alone does not fit.
/// - `agent_history`: `no_workflow`; `binding`. It stores nothing and changes no state. Of the agent's runs, only
///   those whose `agent_run` record is stored count, in the order of `last` of `list_agents`, newest first, and it
///   gives the first `runs` of them: each with its record and the effects whose rows name the run, in ascending order
///   of effect id, each as its newest row by seq holds it: its id, digest, class, state, cause and kept fields. An
///   effect of the class `local_edit` also gives `checkpoint`: the newest row by seq of the run's checkpoint of the
///   workbook path that is the effect's kept `file`, with its state, its instant and the copy's name, or null where
///   no such row stands or that row is `planned`; `store` where that name is longer than one `own_text`. The reply
///   is bounded as every reply: one that does not fit is refused by the writer, never cut.
/// - `drop_sessions`: `no_workflow`; `binding` unless the workflow's agent is the body's; `state` while a run of the
///   agent is open: a start row of the agent (`start_accepted`) stands in the store's read as the runner bodies read
///   one, from 93 days back to two days ahead of the request's day, and no `agent_run` record of its run is stored;
///   after `end_run` of that run the body is taken. Else it makes the host's removal call, `Host::remove_under`, for
///   the kind `agent_session` under the body's workflow with no record: in one write the store removes every row of
///   that kind that names the workflow, whatever its agent and its day, the rows that no read hands out any more
///   among them, each under the store's secure delete with the truncating checkpoint behind the commit (W-C25.5,
///   r3 U9.12a). It stores no row and answers `{"stored":0}`, also at a repeat. So `load` lists no session of the
///   agent, `list_agents` reads `session` `no`, `put_session` takes revision 1 of a site again, and no row of
///   another kind or of another workflow changes. Where the call refuses, the body answers its refusal: `store`
///   where the store refuses the call or faults. A refusal and a fault before the commit leave every row. After a
///   fault behind the commit (the checkpoint or the host's report) `store` comes with the rows gone from every read,
///   and their bytes may lie in the store's files, under the store's key, until the next checkpointed write or the
///   next open; a repeat of the body then removes nothing and checkpoints again. `unsupported` comes from a host
///   that does not hand the call on, which removes nothing. Named limits: a run that never gets its record keeps
///   the body refused until its `end_run`, or until its start row leaves the store's read; a copy or a backup of the
///   store's files from before the body keeps what it held.
/// - `list_replies`: `no_workflow`; `binding`. It stores nothing and changes no state. For each message after `after`
///   that has an `agent_reply` row of the agent, in ascending order of message id by its bytes, it gives the agent's
///   newest row of that message, the last by seq and then by the order stored, as the runner's `load_replies` reads
///   it: the message id, the run and the state of that row alone, no draft, effect, seq, day or instant, and no row
///   of another agent. A page holds whole entries while the writer takes it, and `next` names its last message where
///   entries are left; `length` where the first entry of the page alone does not fit, which an id of at most 1,024
///   bytes cannot reach. Named limits: a row that a later row of its message follows is not listed; a stored row
///   that the typed reader refuses fails the whole read with `store`.
/// - `put_model_key`, `drop_model_key`, `model_key_state` and `read_model_key` (r3 U7.6): the person's key for a
///   model's host, ONE row of the kind `model_key` for the person and for no agent, which names no workflow; each body
///   has `workflow` and `agent` null and needs no stored workflow. `put_model_key` makes the host's removal call of a
///   person's kind, `Host::remove_person`, with the new row: in one write the store removes the row that stands, each
///   row under the store's secure delete with the truncating checkpoint behind the commit, and stores the new one, so
///   ONE row stands; it answers `{"stored":1}`. `drop_model_key` makes the same call with no record and answers
///   `{"stored":0}`, also where no key stands. Where the call refuses, the body answers its refusal, `store` where the
///   store refuses the call or faults; a refusal and a fault before the commit leave the row that stood. After a fault
///   behind the commit (the checkpoint or the host's report) `store` comes with the old row gone from every read and
///   the new one stored, and the old row's bytes may lie in the store's files, under the store's key, until the next
///   checkpointed write or the next open. `model_key_state` stores nothing and answers `{"stands":"yes"}` while a row
///   stands, else `{"stands":"no"}`: what the window is told, and no part, length or sign of the key. `read_model_key`
///   stores nothing and answers the key of the row, `{"key":null}` where none stands; the app's own answer of the
///   supervisor's `Secrets` to the reserved name `model_key` is its one reader. A stored row that the typed reader
///   refuses fails the two reads with `store`. No runner body reads or writes the kind. Named limits: a copy or a
///   backup of the store's files from before a write or a drop keeps the key that it held; the app's own process can
///   read the key back by `read_model_key`, a request of its role.
pub fn coordinator(
    host: &mut dyn Host,
    now: Now,
    body: &[u8],
) -> Result<CoordinatorReply, ErrorName> {
    match CoordinatorCall::parse(body).map_err(refused)? {
        CoordinatorCall::CreateAgent(b) => create_agent(host, now, &b),
        CoordinatorCall::SetBindings(b) => bindings::set_bindings(host, &b),
        CoordinatorCall::SetState(b) => set_state(host, &b),
        CoordinatorCall::StartRun(b) => start_run(host, now, &b),
        CoordinatorCall::EndRun(b) => end::end_run(host, now, &b),
        CoordinatorCall::AddOrigin(b) => bindings::add_origin(host, &b),
        CoordinatorCall::ListCheckpoints(b) => checkpoint::list(&*host, &b),
        CoordinatorCall::MarkCheckpoint(b) => checkpoint::mark(host, now, &b),
        CoordinatorCall::ListOverlaps(b) => bindings::list_overlaps(&*host, &b),
        CoordinatorCall::ListAgents(b) => list_agents(&*host, &b),
        CoordinatorCall::AgentHistory(b) => agent_history(&*host, &b),
        CoordinatorCall::DropSessions(b) => drop_sessions(host, now, &b),
        CoordinatorCall::ListReplies(b) => reply::list(&*host, &b),
        CoordinatorCall::PutModelKey(b) => put_model_key(host, &b),
        CoordinatorCall::DropModelKey(_) => drop_model_key(host),
        CoordinatorCall::ModelKeyState(_) => model_key_state(&*host),
        CoordinatorCall::ReadModelKey(_) => read_model_key(&*host),
    }
}

/// The person's key written over the one that stands, in one write of the host's removal call of a person's kind.
fn put_model_key(host: &mut dyn Host, b: &PutModelKey) -> Result<CoordinatorReply, ErrorName> {
    let row = ModelKey {
        v: V1,
        key: b.key.clone(),
    };
    let records = vec![record(rows::MODEL_KEY, row.to_json())?];
    host.remove_person(rows::MODEL_KEY, records)?;
    Ok(CoordinatorReply::Stored(Stored { stored: 1 }))
}

/// The person's key removed at rest by the host's removal call of a person's kind.
fn drop_model_key(host: &mut dyn Host) -> Result<CoordinatorReply, ErrorName> {
    host.remove_person(rows::MODEL_KEY, Vec::new())?;
    Ok(CoordinatorReply::Stored(Stored { stored: 0 }))
}

/// Whether the person's key stands, and nothing of the key.
fn model_key_state(host: &dyn Host) -> Result<CoordinatorReply, ErrorName> {
    let stands = if rows::model_keys(host.rows())?.is_empty() {
        YesNo::No
    } else {
        YesNo::Yes
    };
    Ok(CoordinatorReply::Stands(StandsReply { stands }))
}

/// The person's key, of the last row in the store's order where more than one stood, or none.
fn read_model_key(host: &dyn Host) -> Result<CoordinatorReply, ErrorName> {
    let key = rows::model_keys(host.rows())?.pop().map(|row| row.key);
    Ok(CoordinatorReply::Key(KeyReply { key }))
}

fn drop_sessions(
    host: &mut dyn Host,
    now: Now,
    b: &DropSessions,
) -> Result<CoordinatorReply, ErrorName> {
    bind::of_agent(&*host, &b.workflow, &b.agent)?;
    if open_run_of(&*host, now, &b.agent)? {
        return Err(ErrorName::State);
    }
    host.remove_under(rows::AGENT_SESSION, &b.workflow, Vec::new())?;
    Ok(CoordinatorReply::Stored(Stored { stored: 0 }))
}

/// Whether a run of the agent is open: a start row of the agent (`start_accepted`) stands in the store's read as
/// `bind::start_row` reads one, and no `agent_run` record of its run is stored, as `bind::open_run` reads one.
fn open_run_of(host: &dyn Host, now: Now, agent: &AgentId) -> Result<bool, ErrorName> {
    let rows = host.rows();
    let today = day::of(host, now);
    let bound = day::bound(rows::AGENT_LOG);
    let records = rows::runs(rows)?;
    Ok(rows::logs(rows)?.iter().any(|l| {
        l.event == LogEvent::StartAccepted
            && &l.agent == agent
            && bound.is_none_or(|days| day::stands(today, days, l.day))
            && !records.iter().any(|r| r.run == l.run)
    }))
}

fn list_agents(host: &dyn Host, b: &ListAgents) -> Result<CoordinatorReply, ErrorName> {
    let rows = host.rows();
    let mut newest: BTreeMap<AgentId, AgentConfig> = BTreeMap::new();
    for config in rows::configs(rows)? {
        if b.after.as_ref().is_some_and(|after| &config.agent <= after) {
            continue;
        }
        if newest
            .get(&config.agent)
            .is_none_or(|held| held.rev < config.rev)
        {
            newest.insert(config.agent.clone(), config);
        }
    }
    let bindings = rows::bindings(rows)?;
    let sessions = rows::sessions(rows)?;
    let runs = rows::runs(rows)?;
    let effects = rows::effects(rows)?;
    let logs = rows::parsed(rows, rows::AGENT_LOG, roster_log)?;
    let starts = start_rows(&logs);
    let replies = rows::replies(rows)?;
    let mut entries = Vec::new();
    for (agent, mut config) in newest {
        let Some(state) = named_state(host, &config) else {
            continue;
        };
        config.playbook = runner::finished(rows, &agent)?
            .pop_last()
            .map(|(number, _)| number);
        let newest_bindings = bindings
            .iter()
            .filter(|r| r.agent == agent && r.workflow == config.workflow)
            .max_by_key(|r| r.rev);
        let slots = newest_bindings.map_or_else(Vec::new, |r| {
            r.slots.iter().map(|s| s.name.clone()).collect()
        });
        let refused_slots = newest_bindings.map_or_else(Vec::new, |r| refused_slots(&r.slots));
        let own: Vec<AgentSession> = sessions
            .iter()
            .filter(|s| s.agent == agent)
            .cloned()
            .collect();
        let listed = runner::newest_sessions(&own);
        let session = if listed.is_empty() || listed.iter().any(|s| runner::dropped(&own, s)) {
            YesNo::No
        } else {
            YesNo::Yes
        };
        let last = runs
            .iter()
            .filter(|r| r.agent == agent)
            .max_by_key(|r| age(&starts, r))
            .cloned();
        let unknown = unknown_of(&effects, &agent);
        let refused_run = refused_run(&logs, &starts, &agent);
        let reply_unknown = reply_unknown(&replies, last.as_ref());
        let last_cause = last_cause(&logs, &starts, &runs, &agent);
        let window = window(&logs, &starts, &runs, &agent, session);
        entries.push(ListedAgent {
            config,
            state,
            slots,
            session,
            last,
            unknown,
            refused_run,
            reply_unknown,
            refused_slots,
            last_cause,
            window,
        });
    }
    Ok(CoordinatorReply::Agents(page(entries)?))
}

/// The state of the stored workflow of the configuration where it is of the full shape and names the agent, else
/// none.
fn named_state(host: &dyn Host, config: &AgentConfig) -> Option<AgentState> {
    match host.workflow(&config.workflow)?.workflow {
        Workflow::Full(w) if w.agent.as_ref() == Some(&config.agent) => match w.state {
            WorkflowState::Approved => Some(AgentState::Approved),
            WorkflowState::Active => Some(AgentState::Active),
            WorkflowState::Paused => Some(AgentState::Paused),
            WorkflowState::Proposed => None,
        },
        _ => None,
    }
}

fn start_rows(logs: &[AgentLog]) -> Vec<AgentLog> {
    logs.iter()
        .filter(|l| l.event == LogEvent::StartAccepted)
        .cloned()
        .collect()
}

/// One `agent_log` row as `list_agents` reads it: as `AgentLog::parse` reads it, but a `stopped` row whose cause is
/// no name of the closed list reads with the cause null. Every other row that the parser refuses stays refused.
fn roster_log(bytes: &[u8]) -> Result<AgentLog, Refusal> {
    let refusal = match AgentLog::parse(bytes) {
        Ok(line) => return Ok(line),
        Err(refusal) => refusal,
    };
    if (refusal.key(), refusal.rule()) != (Some("cause"), "unknown_name") {
        return Err(refusal);
    }
    let Ok(Value::Object(mut line)) = serde_json::from_slice::<Value>(bytes) else {
        return Err(refusal);
    };
    if line.get("event").and_then(Value::as_str) != Some(LogEvent::Stopped.as_str()) {
        return Err(refusal);
    }
    line.insert("cause".to_owned(), Value::Null);
    AgentLog::parse(Value::Object(line).to_string().as_bytes()).map_err(|_| refusal)
}

/// The agent's newest start row: of the greatest `at`, then of the greatest run id.
fn newest_start<'a>(starts: &'a [AgentLog], agent: &AgentId) -> Option<&'a AgentLog> {
    starts
        .iter()
        .filter(|l| &l.agent == agent)
        .max_by_key(|l| (l.at, &l.run))
}

/// The causes of a step's `stopped` row after which the runner goes on with its run: `not_selected`, the row of a
/// send whose message the reply rule no longer selects; `session_size`, the row with no step that the runner writes
/// once a run for a sign-in over the limit that it does not keep, at the opening of a handback or at its end before
/// its own end row (r3 U5.2, U5.2f); and
/// `amount_unread`, the row of a committed payment or purchase whose page after the reply names no amount (r3 U4.1w-2).
/// `last_cause` of `list_agents` never reads such a row as the row of a run's end. The list is the runner's knowledge
/// held by name: a second such cause of the runner reads as the cause of an end until it stands here.
pub const GOES_ON_AFTER: &[Cause] = &[Cause::NotSelected, Cause::SessionSize, Cause::AmountUnread];

/// The cause of the end of the run of the agent's newest start row, none where that run has not ended or ended with
/// no cause. The run has ended where its `agent_run` record of the agent stands. The cause of its end is that of the
/// run's `stopped` row with no step, which the runner writes just before the record where its end has a cause; else
/// that of its `stopped` row with a step, the row of a step that ended the run; each with a cause not in
/// `GOES_ON_AFTER`, and each the row of the greatest seq where two stand. It reads no row of another agent.
fn last_cause(
    logs: &[AgentLog],
    starts: &[AgentLog],
    runs: &[AgentRun],
    agent: &AgentId,
) -> Option<Cause> {
    let newest = newest_start(starts, agent)?;
    if !runs
        .iter()
        .any(|r| &r.agent == agent && r.run == newest.run)
    {
        return None;
    }
    let stopped = || {
        logs.iter()
            .filter(|l| &l.agent == agent && l.run == newest.run && l.event == LogEvent::Stopped)
    };
    let ends = || stopped().filter(|l| !l.cause.is_some_and(|c| GOES_ON_AFTER.contains(&c)));
    let end = ends().filter(|l| l.step.is_none()).max_by_key(|l| l.seq);
    let step = ends().filter(|l| l.step.is_some()).max_by_key(|l| l.seq);
    end.or(step)?.cause
}

/// The run of the agent's newest start row, of the greatest `at` and then run id, where a log row of that run and the
/// agent holds `step_failed` with the cause `unauthorized`; else none. It reads no run record and no row of another
/// agent.
fn refused_run(logs: &[AgentLog], starts: &[AgentLog], agent: &AgentId) -> Option<RunId> {
    let newest = newest_start(starts, agent)?;
    logs.iter()
        .any(|l| {
            &l.agent == agent
                && l.run == newest.run
                && l.event == LogEvent::StepFailed
                && l.cause == Some(Cause::Unauthorized)
        })
        .then(|| newest.run.clone())
}

/// `window` of `list_agents` (the coordinator's rulings of its log lines 16446, 16516 and 16557): `no` where the run of
/// the agent's newest start row, of the greatest `at` and then run id, has its `agent_run` record of the agent with
/// the outcome `completed`, and either `session` reads `yes` and that run's rows of the agent do not hold both a
/// `handback` row and a `stopped` row with the cause `session_size`, or `session` reads `no` and no run of the agent
/// among the rows that the read hands out opened a hand-back of a sign-in (`signed_in_at_a_handback`). Else `yes`, also
/// where no start row of the agent stands. It reads no row of another agent.
fn window(
    logs: &[AgentLog],
    starts: &[AgentLog],
    runs: &[AgentRun],
    agent: &AgentId,
    session: YesNo,
) -> YesNo {
    let Some(newest) = newest_start(starts, agent) else {
        return YesNo::Yes;
    };
    let completed = runs
        .iter()
        .any(|r| &r.agent == agent && r.run == newest.run && r.outcome == Outcome::Completed);
    let of_the_run = |l: &&AgentLog| &l.agent == agent && l.run == newest.run;
    let handed_back = logs
        .iter()
        .filter(of_the_run)
        .any(|l| l.event == LogEvent::Handback);
    let over_limit = logs
        .iter()
        .filter(of_the_run)
        .any(|l| l.event == LogEvent::Stopped && l.cause == Some(Cause::SessionSize));
    let needs_none = completed
        && match session {
            YesNo::Yes => !(handed_back && over_limit),
            YesNo::No => !signed_in_at_a_handback(logs, runs, agent),
        };
    if needs_none {
        YesNo::No
    } else {
        YesNo::Yes
    }
}

/// Whether a run of the agent among `logs` opened a hand-back at which a person was asked to sign in: a `handback` row
/// of the agent that is neither the item of a failed step, whose run holds a `step_failed` row of the seq just before
/// it (the runner's `failed_step` writes the two in a row), nor the item of an unknown effect, the last `handback` row
/// of a run whose `agent_run` record of the agent has the outcome `effect_unknown` (the runner's `close`). Where the
/// row before it is missing, because the store refused it, the row counts as a sign-in's.
fn signed_in_at_a_handback(logs: &[AgentLog], runs: &[AgentRun], agent: &AgentId) -> bool {
    let own: Vec<&AgentLog> = logs.iter().filter(|l| &l.agent == agent).collect();
    own.iter()
        .filter(|h| h.event == LogEvent::Handback)
        .any(|h| {
            let of_run = || own.iter().filter(|l| l.run == h.run);
            let after_a_failed_step = of_run().any(|l| {
                l.event == LogEvent::StepFailed && l.seq.get().checked_add(1) == Some(h.seq.get())
            });
            let last = !of_run().any(|l| l.event == LogEvent::Handback && l.seq > h.seq);
            let unknown_effect = last
                && runs.iter().any(|r| {
                    &r.agent == agent && r.run == h.run && r.outcome == Outcome::EffectUnknown
                });
            !after_a_failed_step && !unknown_effect
        })
}

/// `yes` where `last` is a record and a reply row of its agent and run is `unknown`, a state that no row follows.
fn reply_unknown(replies: &[AgentReply], last: Option<&AgentRun>) -> YesNo {
    let left = last.is_some_and(|record| {
        replies.iter().any(|r| {
            r.agent == record.agent && r.run == record.run && r.state == ReplyState::Unknown
        })
    });
    if left {
        YesNo::Yes
    } else {
        YesNo::No
    }
}

/// The names, in their order, of the slots that their one reader refuses: `workbook` by `workbook_path` in the form
/// of `set_bindings`, `reply_senders` by `reply_senders`. A slot of another name has no reader and is never named.
fn refused_slots(slots: &[Slot]) -> Vec<Name> {
    slots
        .iter()
        .filter(|slot| {
            let value = slot.value.joined();
            match slot.name.as_str() {
                "workbook" => workbook_path(&value, PathForm::NATIVE).is_err(),
                "reply_senders" => reply_senders(&value).is_err(),
                _ => false,
            }
        })
        .map(|slot| slot.name.clone())
        .collect()
}

fn started(starts: &[AgentLog], record: &AgentRun) -> Option<Instant> {
    starts
        .iter()
        .filter(|l| l.run == record.run && l.agent == record.agent)
        .map(|l| l.at)
        .min()
}

/// The order of an agent's run records, the newest last: the day, then the start row, then the run id.
fn age(starts: &[AgentLog], record: &AgentRun) -> (u32, Option<Instant>, RunId) {
    (record.day, started(starts, record), record.run.clone())
}

/// The newest row by seq of each effect of the rows, in ascending order of effect id.
fn last_rows<'a>(
    rows: impl Iterator<Item = &'a AgentEffect>,
) -> BTreeMap<&'a EffectId, &'a AgentEffect> {
    let mut last: BTreeMap<&EffectId, &AgentEffect> = BTreeMap::new();
    for row in rows {
        if last.get(&row.effect).is_none_or(|held| held.seq < row.seq) {
            last.insert(&row.effect, row);
        }
    }
    last
}

fn unknown_of(effects: &[AgentEffect], agent: &AgentId) -> Vec<UnknownEffect> {
    last_rows(effects.iter().filter(|r| &r.agent == agent))
        .into_values()
        .filter(|row| row.state == EffectState::Unknown)
        .map(|row| UnknownEffect {
            run: row.run.clone(),
            effect: row.effect.clone(),
            day: row.day,
            class: row.class,
            kept: row.kept.clone(),
        })
        .collect()
}

/// The first entries that fit one reply as the writer writes it, with `next` the last of them where entries are
/// left: the compact text of a page is that of the empty list with the agent id under `next`, and each entry's own
/// text with a comma before every entry but the first.
fn page(entries: Vec<ListedAgent>) -> Result<AgentsReply, ErrorName> {
    let Some(newest) = entries.last() else {
        return Ok(AgentsReply {
            agents: Vec::new(),
            next: None,
        });
    };
    let frame = AgentsReply {
        agents: Vec::new(),
        next: Some(newest.config.agent.clone()),
    };
    let mut used = frame.to_json().map_err(refused)?.len();
    let mut agents: Vec<ListedAgent> = Vec::new();
    for entry in entries {
        let len = entry.to_json().map_err(refused)?.len() + usize::from(!agents.is_empty());
        if used + len > CHECKPOINTS_REPLY_MAX {
            let Some(next) = agents.last().map(|a| a.config.agent.clone()) else {
                return Err(Own::Length.into());
            };
            return Ok(AgentsReply {
                agents,
                next: Some(next),
            });
        }
        used += len;
        agents.push(entry);
    }
    Ok(AgentsReply { agents, next: None })
}

fn agent_history(host: &dyn Host, b: &AgentHistory) -> Result<CoordinatorReply, ErrorName> {
    bind::of_agent(host, &b.workflow, &b.agent)?;
    let rows = host.rows();
    let starts = start_rows(&rows::logs(rows)?);
    let mut records: Vec<AgentRun> = rows::runs(rows)?
        .into_iter()
        .filter(|r| r.agent == b.agent)
        .collect();
    records.sort_by_cached_key(|r| Reverse(age(&starts, r)));
    records.truncate(usize::try_from(b.runs).unwrap_or(usize::MAX));
    let effects = rows::effects(rows)?;
    let checkpoints = rows::checkpoints(rows)?;
    let mut runs = Vec::with_capacity(records.len());
    for record in records {
        let mut past = Vec::new();
        for row in last_rows(effects.iter().filter(|r| r.run == record.run)).into_values() {
            let checkpoint = if row.class == ActionClass::LocalEdit {
                checkpoint_of(&checkpoints, row)?
            } else {
                None
            };
            past.push(PastEffect {
                effect: row.effect.clone(),
                digest: row.digest.clone(),
                class: row.class,
                state: row.state,
                cause: row.cause,
                kept: row.kept.clone(),
                checkpoint,
            });
        }
        runs.push(HistoryRun {
            run: record,
            effects: past,
        });
    }
    Ok(CoordinatorReply::History(HistoryReply { runs }))
}

/// The checkpoint of a `local_edit` effect: the newest row by seq of its run's checkpoint of the workbook path that
/// is its kept `file`, or none where no such row stands or that row is `planned`.
fn checkpoint_of(
    checkpoints: &[AgentCheckpoint],
    edit: &AgentEffect,
) -> Result<Option<PastCheckpoint>, ErrorName> {
    let Some(file) = edit
        .kept
        .iter()
        .find(|k| k.name == FieldName::File)
        .map(|k| k.value.joined())
    else {
        return Ok(None);
    };
    let newest = checkpoints
        .iter()
        .filter(|c| c.run == edit.run && c.workbook.joined() == file)
        .max_by_key(|c| c.seq);
    match newest {
        Some(row) if row.state != CheckpointState::Planned => Ok(Some(PastCheckpoint {
            state: row.state,
            at: row.at,
            name: OwnText::parse(&row.copy.joined()).map_err(|_| ErrorName::Store)?,
        })),
        _ => Ok(None),
    }
}

fn create_agent(
    host: &mut dyn Host,
    now: Now,
    b: &CreateAgent,
) -> Result<CoordinatorReply, ErrorName> {
    let w = match bind::stored(&*host, &b.workflow)? {
        Workflow::Full(w) if w.state != WorkflowState::Proposed => w,
        _ => return Err(ErrorName::State),
    };
    if w.agent.as_ref().is_some_and(|a| a != &b.agent) {
        return Err(Own::Binding.into());
    }
    let configs = rows::configs(host.rows())?;
    let mut mine = configs.iter().filter(|c| c.agent == b.agent).peekable();
    if mine.clone().any(|c| c.workflow != b.workflow) {
        return Err(Own::Binding.into());
    }
    let mut records = Vec::new();
    if mine.peek().is_none() {
        let config = preset(
            Category::from(w.category),
            b.workflow.clone(),
            b.agent.clone(),
        )
        .ok_or(Own::Config)?;
        validate(&config).map_err(|_| Own::Config)?;
        records.push(record(rows::AGENT_CONFIG, config.to_json())?);
    }
    if rows::start_of(host.rows(), &b.agent)?.is_none() {
        let start = AgentStart {
            v: V1,
            workflow: b.workflow.clone(),
            agent: b.agent.clone(),
            day: day::of(&*host, now),
            at: now.at,
        };
        records.push(record(rows::AGENT_START, start.to_json())?);
    }
    if !records.is_empty() {
        day::commit(host, now, records)?;
    }
    if w.agent.is_none() {
        bindings::state_call(host, &b.workflow, AgentState::Approved, &b.agent)?;
    }
    Ok(CoordinatorReply::Rev(RevReply {
        rev: NonZeroU32::MIN,
    }))
}

fn set_state(host: &mut dyn Host, b: &SetState) -> Result<CoordinatorReply, ErrorName> {
    bind::of_agent(&*host, &b.workflow, &b.agent)?;
    if b.state == AgentState::Active {
        let config = rows::newest_config(host.rows(), &b.agent)?.ok_or(Own::Binding)?;
        if config.runtime == Runtime::Web && runner::finished(host.rows(), &b.agent)?.is_empty() {
            return Err(Own::Playbook.into());
        }
    }
    bindings::state_call(host, &b.workflow, b.state, &b.agent)?;
    Ok(CoordinatorReply::State(StateReply { state: b.state }))
}

fn start_run(host: &mut dyn Host, now: Now, b: &StartRun) -> Result<CoordinatorReply, ErrorName> {
    let w = bind::of_agent(&*host, &b.workflow, &b.agent)?;
    let open = match b.mode {
        Mode::Compile => matches!(w.state, WorkflowState::Approved | WorkflowState::Active),
        Mode::Replay => w.state == WorkflowState::Active,
    };
    if !open {
        return Err(ErrorName::State);
    }
    bind::no_other_agent(&*host, &b.run, &b.agent)?;
    bind::open_run(&*host, &b.run)?;
    if !bind::held_start_row(&*host, &b.run, &b.agent)? {
        let row = AgentLog {
            v: V1,
            workflow: b.workflow.clone(),
            agent: b.agent.clone(),
            run: b.run.clone(),
            seq: NonZeroU32::MIN,
            day: day::of(&*host, now),
            at: now.at,
            event: LogEvent::StartAccepted,
            step: None,
            cause: None,
            model: None,
            ms: None,
            tokens_in: None,
            tokens_out: None,
            page_amount: None,
        };
        day::commit(host, now, vec![record(rows::AGENT_LOG, row.to_json())?])?;
    }
    Ok(CoordinatorReply::Accepted(Accepted {}))
}
