use halvo_schema_core::ids::{AgentId, WorkflowId};
use halvo_schema_core::record::{Workflow, WorkflowState};
use halvo_schema_core::request::ErrorName;

use crate::decide::{set_body, workflow_by_id};
use crate::error::Error;
use crate::handler::HandlerCx;
use crate::typed::Writer;

/// The state to which the state call moves a stored workflow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveTo {
    Approved,
    Active,
    Paused,
}

impl MoveTo {
    fn state(self) -> WorkflowState {
        match self {
            MoveTo::Approved => WorkflowState::Approved,
            MoveTo::Active => WorkflowState::Active,
            MoveTo::Paused => WorkflowState::Paused,
        }
    }
}

impl HandlerCx<'_> {
    /// The state call: in one transaction it moves a stored workflow among `approved`, `active` and `paused`, sets its
    /// agent to `agent` or clears it, and raises the revision of the family `workflow` when the row changes. It
    /// answers `no_workflow` for a workflow that is not stored, before any rule of the move, then `state` for one that
    /// is `proposed` or `dismissed` and for a call that would leave an `active` or a `paused` workflow with no agent,
    /// and changes nothing for a call that asks for what the row holds. It takes nothing else, so no free text reaches
    /// the workflow. The store offers it while a handler answers a `coordinator_write`, the request under which the
    /// state of a workflow changes, and answers `role` to it while a handler answers a request of another role.
    pub fn state(
        &mut self,
        workflow: &WorkflowId,
        to: MoveTo,
        agent: Option<AgentId>,
    ) -> Result<(), Error> {
        self.db.arm();
        if self.writer != Writer::Coordinator {
            return Err(Error::Refused(ErrorName::Role));
        }
        self.db.write(|tx| match workflow_by_id(tx, workflow)? {
            None => Err(Error::Refused(ErrorName::NoWorkflow)),
            Some((_, Workflow::Dismissed(_))) => Err(Error::Refused(ErrorName::State)),
            Some((_, Workflow::Full(full))) if full.state == WorkflowState::Proposed => {
                Err(Error::Refused(ErrorName::State))
            }
            Some(_) if to != MoveTo::Approved && agent.is_none() => {
                Err(Error::Refused(ErrorName::State))
            }
            Some((row, Workflow::Full(mut full))) => {
                if full.state == to.state() && full.agent == agent {
                    return Ok(());
                }
                full.state = to.state();
                full.agent = agent;
                Ok(set_body(tx, row, &Workflow::Full(full))?)
            }
        })
    }
}
