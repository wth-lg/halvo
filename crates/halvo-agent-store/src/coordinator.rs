use std::num::NonZeroU32;

use halvo_policy::{preset, validate};
use halvo_schema::executable::body::{
    Accepted, CreateAgent, RevReply, SetState, StartRun, StateReply,
};
use halvo_schema::executable::names::{AgentState, LogEvent, Mode};
use halvo_schema::executable::text::V1;
use halvo_schema::executable::{AgentLog, AgentStart, CoordinatorCall, CoordinatorReply, Runtime};
use halvo_schema::{Category, ErrorName, Workflow, WorkflowState};

use crate::errors::{refused, Own};
use crate::host::{Host, Now};
use crate::rows::{self, record};
use crate::{bind, bindings, checkpoint, day, end};

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
///   unless the workflow's agent is this agent, and answers `{"rev":1}`.
/// - `set_state`: `no_workflow`; `binding` unless the workflow's agent is the body's, and for `active` where no
///   configuration of the agent is stored; `playbook` for `active` while
///   the agent's runtime is `web` and no playbook step of the agent is stored; then the state call and its own
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
///   system's form. It stores the next revision of the agent's bindings, revision 1 where none is stored: each slot
///   of the body replaces the stored slot of its name in its place or is added at the end, and the other slots and
///   the origins stay. It answers the revision stored.
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
/// - `list_overlaps`: not served yet. It is answered `malformed`, as the store answered the body before the schema
///   read it, and it stores nothing and changes no state.
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
        CoordinatorCall::ListOverlaps(_) => Err(ErrorName::Malformed),
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
        host.set_state(&b.workflow, AgentState::Approved, Some(&b.agent))?;
    }
    Ok(CoordinatorReply::Rev(RevReply {
        rev: NonZeroU32::MIN,
    }))
}

fn set_state(host: &mut dyn Host, b: &SetState) -> Result<CoordinatorReply, ErrorName> {
    bind::of_agent(&*host, &b.workflow, &b.agent)?;
    if b.state == AgentState::Active {
        let config = rows::newest_config(host.rows(), &b.agent)?.ok_or(Own::Binding)?;
        let playbook = rows::playbooks(host.rows())?
            .iter()
            .any(|p| p.agent == b.agent);
        if config.runtime == Runtime::Web && !playbook {
            return Err(Own::Playbook.into());
        }
    }
    host.set_state(&b.workflow, b.state, Some(&b.agent))?;
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
