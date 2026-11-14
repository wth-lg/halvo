use std::num::NonZeroU32;

use halvo_policy::{preset, validate};
use halvo_schema::executable::body::{
    Accepted, CreateAgent, RevReply, SetState, StartRun, StateReply,
};
use halvo_schema::executable::names::{AgentState, Mode};
use halvo_schema::executable::{CoordinatorCall, CoordinatorReply, Runtime};
use halvo_schema::{Category, ErrorName, TypedRecord, Workflow, WorkflowState};

use crate::errors::{refused, Own};
use crate::host::{Host, Now};
use crate::rows;
use crate::{bind, bindings, end};

/// Answers one coordinator body of A7.2, read from its bytes with the typed parser: the reply, or the error name of
/// the first check that refuses it. A refused body stores nothing, but for `create_agent`, whose state call comes
/// after its commit: when the state call is refused, the stored configuration stays, and a repeat of the body makes
/// the state call and stores no second configuration. A body that is not the form is `malformed`. The checks, first
/// that fits:
///
/// - `create_agent`: `no_workflow`; `state` for a workflow that is `proposed` or `dismissed`; `binding` when the
///   workflow's agent is another agent or the agent's configuration is stored for another workflow. It stores the
///   preset of the workflow's category as revision 1 unless the agent's configuration is stored, makes the state
///   call (`approved`, the agent) unless the workflow's agent is this agent, and answers `{"rev":1}`.
/// - `set_state`: `no_workflow`; `binding` unless the workflow's agent is the body's, and for `active` where no
///   configuration of the agent is stored; `playbook` for `active` while
///   the agent's runtime is `web` and no playbook step of the agent is stored; then the state call and its own
///   refusal.
/// - `start_run`: `no_workflow`; `binding`; `state` unless the workflow is `approved` or `active` for `compile`
///   and `active` for `replay`. It stores nothing.
/// - `set_bindings`: `no_workflow`; `binding`; `binding` for a slot `workbook` that `workbook_path` refuses in the
///   system's form. It stores the next revision of the agent's bindings, revision 1 where none is stored: each slot
///   of the body replaces the stored slot of its name in its place or is added at the end, and the other slots and
///   the origins stay. It answers the revision stored.
/// - `add_origin`: `no_workflow`; `binding`. Where the newest revision holds the origin it stores nothing and
///   answers that revision; else it stores the next revision with the origin added at the end and answers it.
/// - `end_run`: `no_workflow`; `binding`; `binding` for a run of which a stored effect row, log line or
///   `agent_run` record names another agent. Where the run's `agent_run` record is stored it changes nothing.
///   Else, in one commit, the run's record with the outcome `failed`, the mode null, every count 0, no playbook and
///   no model, on the request's day, and the moves of the run's end as `runner` makes them for `finish`. It answers
///   `{"ok":"yes"}`.
pub fn coordinator(
    host: &mut dyn Host,
    now: Now,
    body: &[u8],
) -> Result<CoordinatorReply, ErrorName> {
    match CoordinatorCall::parse(body).map_err(refused)? {
        CoordinatorCall::CreateAgent(b) => create_agent(host, &b),
        CoordinatorCall::SetBindings(b) => bindings::set_bindings(host, &b),
        CoordinatorCall::SetState(b) => set_state(host, &b),
        CoordinatorCall::StartRun(b) => start_run(&*host, &b),
        CoordinatorCall::EndRun(b) => end::end_run(host, now, &b),
        CoordinatorCall::AddOrigin(b) => bindings::add_origin(host, &b),
        CoordinatorCall::ListCheckpoints(_) | CoordinatorCall::MarkCheckpoint(_) => {
            Err(ErrorName::Malformed)
        }
    }
}

fn create_agent(host: &mut dyn Host, b: &CreateAgent) -> Result<CoordinatorReply, ErrorName> {
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
    if mine.peek().is_none() {
        let config = preset(
            Category::from(w.category),
            b.workflow.clone(),
            b.agent.clone(),
        )
        .ok_or(Own::Config)?;
        validate(&config).map_err(|_| Own::Config)?;
        let json = config.to_json().map_err(refused)?;
        host.commit(vec![TypedRecord {
            kind: rows::AGENT_CONFIG,
            json,
        }])?;
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

fn start_run(host: &dyn Host, b: &StartRun) -> Result<CoordinatorReply, ErrorName> {
    let w = bind::of_agent(host, &b.workflow, &b.agent)?;
    let open = match b.mode {
        Mode::Compile => matches!(w.state, WorkflowState::Approved | WorkflowState::Active),
        Mode::Replay => w.state == WorkflowState::Active,
    };
    if !open {
        return Err(ErrorName::State);
    }
    Ok(CoordinatorReply::Accepted(Accepted {}))
}
