use std::collections::BTreeMap;
use std::num::NonZeroU32;

use halvo_policy::{preset, validate};
use halvo_schema::executable::body::{
    Accepted, AgentsReply, CreateAgent, ListAgents, ListedAgent, RevReply, SetState, StartRun,
    StateReply, UnknownEffect, CHECKPOINTS_REPLY_MAX,
};
use halvo_schema::executable::names::{AgentState, EffectState, LogEvent, Mode, YesNo};
use halvo_schema::executable::text::V1;
use halvo_schema::executable::{
    AgentConfig, AgentEffect, AgentLog, AgentRun, AgentSession, AgentStart, CoordinatorCall,
    CoordinatorReply, EffectId, Runtime,
};
use halvo_schema::{AgentId, Category, ErrorName, Instant, Workflow, WorkflowState};

use crate::errors::{refused, Own};
use crate::host::{Host, Now};
use crate::rows::{self, record};
use crate::{bind, bindings, checkpoint, day, end, runner};

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
///   `session`, `yes` where a session revision of the agent is stored whole, as `load` lists it; `last`, the agent's
///   run record of the greatest day, then of the latest start row, then of the greatest run id; `unknown`, each
///   effect of the agent whose newest row by seq is `unknown`, in ascending order of effect id, with the run, the
///   day, the class and the kept fields of that row, whatever a record's count says. A page holds whole entries while
///   the writer takes it, and `next` names its last agent where agents are left; `length` where the first entry of
///   the page alone does not fit.
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
    }
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
    let starts: Vec<AgentLog> = rows::logs(rows)?
        .into_iter()
        .filter(|l| l.event == LogEvent::StartAccepted)
        .collect();
    let mut entries = Vec::new();
    for (agent, mut config) in newest {
        let Some(state) = named_state(host, &config) else {
            continue;
        };
        config.playbook = runner::finished(rows, &agent)?
            .pop_last()
            .map(|(number, _)| number);
        let slots = bindings
            .iter()
            .filter(|r| r.agent == agent && r.workflow == config.workflow)
            .max_by_key(|r| r.rev)
            .map_or_else(Vec::new, |r| {
                r.slots.iter().map(|s| s.name.clone()).collect()
            });
        let own: Vec<AgentSession> = sessions
            .iter()
            .filter(|s| s.agent == agent)
            .cloned()
            .collect();
        let session = if runner::newest_sessions(&own).is_empty() {
            YesNo::No
        } else {
            YesNo::Yes
        };
        let last = runs
            .iter()
            .filter(|r| r.agent == agent)
            .max_by_key(|r| (r.day, started(&starts, r), r.run.clone()))
            .cloned();
        let unknown = unknown_of(&effects, &agent);
        entries.push(ListedAgent {
            config,
            state,
            slots,
            session,
            last,
            unknown,
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

fn started(starts: &[AgentLog], record: &AgentRun) -> Option<Instant> {
    starts
        .iter()
        .filter(|l| l.run == record.run && l.agent == record.agent)
        .map(|l| l.at)
        .min()
}

fn unknown_of(effects: &[AgentEffect], agent: &AgentId) -> Vec<UnknownEffect> {
    let mut last: BTreeMap<&EffectId, &AgentEffect> = BTreeMap::new();
    for row in effects.iter().filter(|r| &r.agent == agent) {
        if last.get(&row.effect).is_none_or(|held| held.seq < row.seq) {
            last.insert(&row.effect, row);
        }
    }
    last.into_values()
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
        };
        day::commit(host, now, vec![record(rows::AGENT_LOG, row.to_json())?])?;
    }
    Ok(CoordinatorReply::Accepted(Accepted {}))
}
