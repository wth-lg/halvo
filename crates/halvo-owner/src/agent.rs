//! The handlers of the agent-setup run in the owner's host (contract A7.5, the workflow run's C25.7): an adapter that
//! gives `halvo_agent_store::runner` and `halvo_agent_store::coordinator` the seven calls of `halvo_agent_store::Host`
//! from the `HandlerCx` of the request and from the host's clock.

use std::cell::Cell;
use std::rc::Rc;

use halvo_agent_store::{Host, Now};
use halvo_derived::{Clock, Error, Handler, HandlerCx, MoveTo, RemoveRefusal};
use halvo_schema::executable::names::AgentState;
use halvo_schema::{
    AgentId, ErrorName, Instant, RawRequest, RowRead, TypedRecord, WorkflowId, WorkflowItem,
};
use serde_json::value::RawValue;

/// The family of the rows that the two handlers read and store.
pub(crate) const FAMILY: &str = "agent";

/// The `op` names of `CoordinatorCall`, all twenty-two, which the handler of `coordinator_write` takes. A
/// twenty-third `op` is answered `unsupported` by the store until it is named here.
pub(crate) const OPS: &[&str] = &[
    "create_agent",
    "set_bindings",
    "set_state",
    "start_run",
    "end_run",
    "add_origin",
    "list_checkpoints",
    "mark_checkpoint",
    "list_overlaps",
    "list_agents",
    "agent_history",
    "drop_sessions",
    "list_replies",
    "put_model_key",
    "drop_model_key",
    "model_key_state",
    "read_model_key",
    "set_model",
    "model_room",
    "set_model_cap",
    "new_model_count",
    "sheet_answers",
];

/// The body that a handler answers.
#[derive(Clone, Copy)]
enum Body {
    Runner,
    Coordinator,
}

/// The handler of halvo-agent-store for one kind of body, with the host's clock.
struct Agent {
    body: Body,
    clock: Rc<dyn Clock>,
}

/// The handler of every `runner` request, over the host's clock.
pub(crate) fn runner(clock: Rc<dyn Clock>) -> Box<dyn Handler> {
    Box::new(Agent {
        body: Body::Runner,
        clock,
    })
}

/// The handler of the `coordinator_write` bodies of `OPS`, over the host's clock.
pub(crate) fn coordinator(clock: Rc<dyn Clock>) -> Box<dyn Handler> {
    Box::new(Agent {
        body: Body::Coordinator,
        clock,
    })
}

impl Handler for Agent {
    /// Reads the clock once for the request: its instant is the request's `Now`, and `Host::day` gives the day of an
    /// instant by the same clock. A fault of the store in the read of a workflow is answered `store`, whatever the
    /// handler answered after it.
    fn handle(
        &self,
        request: &RawRequest,
        cx: &mut HandlerCx<'_>,
    ) -> Result<Box<RawValue>, ErrorName> {
        let now = Now {
            at: self.clock.now(),
        };
        let mut host = Cx {
            cx,
            clock: &*self.clock,
            fault: Cell::new(false),
        };
        let body = request.body().get().as_bytes();
        let json = match self.body {
            Body::Runner => halvo_agent_store::runner(&mut host, now, body)
                .and_then(|reply| reply.to_json().map_err(|_| ErrorName::Internal)),
            Body::Coordinator => halvo_agent_store::coordinator(&mut host, now, body)
                .and_then(|reply| reply.to_json().map_err(|_| ErrorName::Internal)),
        };
        if host.fault.get() {
            return Err(ErrorName::Store);
        }
        RawValue::from_string(json?).map_err(|_| ErrorName::Internal)
    }
}

/// The seven calls of `halvo_agent_store::Host` for one request.
struct Cx<'c, 'x> {
    cx: &'c mut HandlerCx<'x>,
    clock: &'c dyn Clock,
    fault: Cell<bool>,
}

impl Host for Cx<'_, '_> {
    /// `HandlerCx::commit`: a list that the store refuses, for a rule or for a fault, is answered `store`, as the
    /// memory store of halvo-agent-store answers it.
    fn commit(&mut self, records: Vec<TypedRecord>) -> Result<(), ErrorName> {
        self.cx
            .commit(&records)
            .map(|_| ())
            .map_err(|_| ErrorName::Store)
    }

    /// `HandlerCx::remove_under`: `role` under a request that is no `coordinator_write`, and every other refusal, a
    /// fault among them, as `store`, as `commit` answers it.
    fn remove_under(
        &mut self,
        kind: &str,
        workflow: &WorkflowId,
        records: Vec<TypedRecord>,
    ) -> Result<u64, ErrorName> {
        match self.cx.remove_under(kind, workflow, &records) {
            Ok(done) => Ok(done.removed),
            Err(RemoveRefusal::Role) => Err(ErrorName::Role),
            Err(_) => Err(ErrorName::Store),
        }
    }

    /// `HandlerCx::remove_person`: `role` under a request that is no `coordinator_write`, but for a runner's request of
    /// a kind of `kinds::RUNNER_PERSON_KINDS` (r7 8.1L), and every other refusal, a fault among them, as `store`, as
    /// `remove_under` answers it.
    fn remove_person(&mut self, kind: &str, records: Vec<TypedRecord>) -> Result<u64, ErrorName> {
        match self.cx.remove_person(kind, &records) {
            Ok(done) => Ok(done.removed),
            Err(RemoveRefusal::Role) => Err(ErrorName::Role),
            Err(_) => Err(ErrorName::Store),
        }
    }

    /// `HandlerCx::state`: its refusal by name, and a fault as `store`.
    fn set_state(
        &mut self,
        workflow: &WorkflowId,
        state: AgentState,
        agent: Option<&AgentId>,
    ) -> Result<(), ErrorName> {
        let to = match state {
            AgentState::Approved => MoveTo::Approved,
            AgentState::Active => MoveTo::Active,
            AgentState::Paused => MoveTo::Paused,
        };
        self.cx
            .state(workflow, to, agent.cloned())
            .map_err(|e| match e {
                Error::Refused(name) => name,
                Error::Fault(_) => ErrorName::Store,
            })
    }

    /// `HandlerCx::rows`: the store answers a fault of this read as the fault, after the handler.
    fn rows(&self) -> &dyn RowRead {
        self
    }

    /// `HandlerCx::workflow`: a fault reads as no workflow here and makes the request's answer `store`.
    fn workflow(&self, id: &WorkflowId) -> Option<WorkflowItem> {
        self.cx.workflow(id).unwrap_or_else(|_| {
            self.fault.set(true);
            None
        })
    }

    /// The day of the host's clock.
    fn day(&self, at: Instant) -> u32 {
        self.clock.day(at)
    }
}

impl RowRead for Cx<'_, '_> {
    fn for_each(&self, kind: &str, visit: &mut dyn FnMut(&str)) {
        self.cx.rows().for_each(kind, visit);
    }
}
