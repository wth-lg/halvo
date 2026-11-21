//! The handlers and the step of the learning run in the owner's host (the learning run's contracts L11.5, the workflow
//! run's C25.7): `LearnHandlers` of halvo-learn-store in front of the agent store's handlers, and `LearnStep` after
//! the recurrence step, all on the host's clock.
//!
//! `LearnHandlers` run the agent store's code in front, with the calls of the family `agent` that it makes. A handler
//! gets the calls of its own family alone (`HandlerCx`), so each such call goes behind by one `HandlerCx::pass_on` to
//! the handler `Behind` of the family `agent`, which makes it with its own `HandlerCx` (the form of the learning run's
//! gate host, crates/halvo-learn-gate/src/derived.rs). One value of `LearnHandlers` serves every request of the
//! host's life (L4.1, L11.5).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use halvo_agent_store::{Host as AgentHost, Now};
use halvo_derived::{Clock, Error, Handler, HandlerCx, MoveTo, Takes};
use halvo_learn_store::{Clock as Today, LearnHandlers, LearnHost, LearnStep};
use halvo_schema::executable::names::AgentState;
use halvo_schema::{
    AgentId, ErrorName, Instant, RawRequest, RowRead, TypedRecord, WorkflowId, WorkflowItem,
};
use serde_json::value::RawValue;

use crate::agent;
use crate::steps::Line;

/// The family of the rows that `LearnHandlers` and `LearnStep` read and store.
pub(crate) const FAMILY: &str = "learn";

/// The `op` names that the handlers in front take: the nine of the agent store's handler (`agent::OPS`), which they
/// pass on, and `list_refinements`, which they answer themselves (L2.2, L4.3).
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
    "list_refinements",
];

/// The lines of the learning run, made with the host's clock: `LearnStep`, then `LearnHandlers` in front of the
/// handlers of `runner` and of `OPS`, then `Behind` for each, which the agent store's handlers of the list follow.
pub(crate) fn lines(clock: Rc<dyn Clock>) -> Vec<Line> {
    let handlers = Rc::new(RefCell::new(LearnHandlers::new()));
    let channel = Rc::new(Channel::default());
    let front = || -> Box<dyn Handler> {
        Box::new(Front {
            clock: Rc::clone(&clock),
            handlers: Rc::clone(&handlers),
            channel: Rc::clone(&channel),
        })
    };
    let behind = || -> Box<dyn Handler> {
        Box::new(Behind {
            channel: Rc::clone(&channel),
        })
    };
    vec![
        Line::Step(FAMILY, Box::new(LearnStep::new(Day(Rc::clone(&clock))))),
        Line::Handler(FAMILY, Takes::Runner, front()),
        Line::Handler(FAMILY, Takes::Coordinator(OPS), front()),
        Line::Handler(agent::FAMILY, Takes::Runner, behind()),
        Line::Handler(agent::FAMILY, Takes::Coordinator(OPS), behind()),
    ]
}

/// Today of the host's clock: the day of its present instant (L0.6), which the step reads.
struct Day(Rc<dyn Clock>);

impl Today for Day {
    fn today(&self) -> u32 {
        self.0.day(self.0.now())
    }
}

/// A call of the family `agent` that the agent store's code in front asks of `Behind`.
enum Call {
    Commit(Vec<TypedRecord>),
    State(WorkflowId, MoveTo, Option<AgentId>),
    Rows(String),
    Workflow(WorkflowId),
}

/// What `Behind` answers a call.
enum Answer {
    Done(Result<(), ErrorName>),
    Rows(Vec<String>),
    Workflow(Option<WorkflowItem>),
    /// The read of the workflow met a fault of the store.
    Failed,
}

/// The call in flight between the handlers in front and `Behind`, and whether a call went unanswered in the request.
#[derive(Default)]
struct Channel {
    call: RefCell<Option<Call>>,
    answer: RefCell<Option<Answer>>,
    broken: Cell<bool>,
}

fn move_to(state: AgentState) -> MoveTo {
    match state {
        AgentState::Approved => MoveTo::Approved,
        AgentState::Active => MoveTo::Active,
        AgentState::Paused => MoveTo::Paused,
    }
}

/// The handler of the family `agent` behind `LearnHandlers`: it makes the call that the code in front put into the
/// channel with its own `HandlerCx`, as the agent store's handler of the list makes it (`agent`), and passes a
/// request with no call on to that handler.
struct Behind {
    channel: Rc<Channel>,
}

impl Handler for Behind {
    fn handle(
        &self,
        _request: &RawRequest,
        cx: &mut HandlerCx<'_>,
    ) -> Result<Box<RawValue>, ErrorName> {
        let Some(call) = self.channel.call.borrow_mut().take() else {
            return cx.pass_on();
        };
        let answer = match call {
            Call::Commit(records) => Answer::Done(
                cx.commit(&records)
                    .map(|_| ())
                    .map_err(|_| ErrorName::Store),
            ),
            Call::State(workflow, to, agent) => {
                Answer::Done(cx.state(&workflow, to, agent).map_err(|e| match e {
                    Error::Refused(name) => name,
                    Error::Fault(_) => ErrorName::Store,
                }))
            }
            Call::Rows(kind) => {
                let mut rows = Vec::new();
                cx.rows()
                    .for_each(&kind, &mut |row| rows.push(row.to_owned()));
                Answer::Rows(rows)
            }
            Call::Workflow(id) => match cx.workflow(&id) {
                Ok(item) => Answer::Workflow(item),
                Err(_) => Answer::Failed,
            },
        };
        *self.channel.answer.borrow_mut() = Some(answer);
        RawValue::from_string("null".to_owned()).map_err(|_| ErrorName::Internal)
    }
}

/// The calls of `halvo_agent_store::Host` for the agent store's code in front, each made behind by one `pass_on`.
struct Bridge<'r, 'c, 'x> {
    cx: &'r RefCell<&'c mut HandlerCx<'x>>,
    channel: &'r Channel,
    clock: &'r dyn Clock,
    /// A fault of the store at the read of a workflow, which makes the request's answer `store`, as the agent store's
    /// handler of the list answers it.
    fault: Cell<bool>,
}

impl Bridge<'_, '_, '_> {
    fn ask(&self, call: Call) -> Option<Answer> {
        *self.channel.call.borrow_mut() = Some(call);
        let passed = self.cx.borrow_mut().pass_on();
        let unanswered = self.channel.call.borrow_mut().take().is_some();
        let answer = self.channel.answer.borrow_mut().take();
        match (passed, unanswered, answer) {
            (Ok(_), false, Some(answer)) => Some(answer),
            _ => {
                self.channel.broken.set(true);
                None
            }
        }
    }
}

impl AgentHost for Bridge<'_, '_, '_> {
    fn commit(&mut self, records: Vec<TypedRecord>) -> Result<(), ErrorName> {
        match self.ask(Call::Commit(records)) {
            Some(Answer::Done(done)) => done,
            _ => Err(ErrorName::Internal),
        }
    }

    fn set_state(
        &mut self,
        workflow: &WorkflowId,
        state: AgentState,
        agent: Option<&AgentId>,
    ) -> Result<(), ErrorName> {
        let call = Call::State(workflow.clone(), move_to(state), agent.cloned());
        match self.ask(call) {
            Some(Answer::Done(done)) => done,
            _ => Err(ErrorName::Internal),
        }
    }

    fn rows(&self) -> &dyn RowRead {
        self
    }

    fn workflow(&self, id: &WorkflowId) -> Option<WorkflowItem> {
        match self.ask(Call::Workflow(id.clone())) {
            Some(Answer::Workflow(item)) => item,
            Some(Answer::Failed) => {
                self.fault.set(true);
                None
            }
            _ => None,
        }
    }

    fn day(&self, at: Instant) -> u32 {
        self.clock.day(at)
    }
}

impl RowRead for Bridge<'_, '_, '_> {
    fn for_each(&self, kind: &str, visit: &mut dyn FnMut(&str)) {
        if let Some(Answer::Rows(rows)) = self.ask(Call::Rows(kind.to_owned())) {
            for row in &rows {
                visit(row);
            }
        }
    }
}

/// What `LearnHandlers` get from the owner's host for one request: the calls of the family `learn` on the request's
/// `HandlerCx`, the agent store's calls through `Bridge`, and one reading of the host's clock.
struct Host<'r, 'c, 'x> {
    cx: &'r RefCell<&'c mut HandlerCx<'x>>,
    agent: Bridge<'r, 'c, 'x>,
    at: Instant,
    day: u32,
}

impl Today for Host<'_, '_, '_> {
    fn today(&self) -> u32 {
        self.day
    }
}

impl LearnHost for Host<'_, '_, '_> {
    fn agent_host(&mut self) -> &mut dyn AgentHost {
        &mut self.agent
    }

    fn agent_now(&self) -> Now {
        Now { at: self.at }
    }

    fn commit(&mut self, records: Vec<TypedRecord>) -> Result<(), ErrorName> {
        self.cx
            .borrow_mut()
            .commit(&records)
            .map(|_| ())
            .map_err(|_| ErrorName::Store)
    }

    fn rows(&self) -> &dyn RowRead {
        self
    }

    fn for_each_under(&self, kind: &str, workflow: &WorkflowId, visit: &mut dyn FnMut(&str)) {
        let mut rows = Vec::new();
        self.cx
            .borrow()
            .rows()
            .for_each_under(kind, workflow, &mut |row| rows.push(row.to_owned()));
        for row in &rows {
            visit(row);
        }
    }

    /// A fault of the store reads as no workflow: the reply stays the agent store's (L4.2, LM-58 (b)).
    fn workflow(&self, id: &WorkflowId) -> Option<WorkflowItem> {
        let item = self.cx.borrow().workflow(id);
        item.unwrap_or(None)
    }
}

impl RowRead for Host<'_, '_, '_> {
    fn for_each(&self, kind: &str, visit: &mut dyn FnMut(&str)) {
        let mut rows = Vec::new();
        self.cx
            .borrow()
            .rows()
            .for_each(kind, &mut |row| rows.push(row.to_owned()));
        for row in &rows {
            visit(row);
        }
    }
}

/// `LearnHandlers` as a handler of the owner's store, in front of `Behind` and of the agent store's handlers.
struct Front {
    clock: Rc<dyn Clock>,
    handlers: Rc<RefCell<LearnHandlers>>,
    channel: Rc<Channel>,
}

impl Handler for Front {
    /// Reads the clock once for the request. A call that goes unanswered behind is answered `internal`, and a fault
    /// of the store at the agent store's read of a workflow `store`.
    fn handle(
        &self,
        request: &RawRequest,
        cx: &mut HandlerCx<'_>,
    ) -> Result<Box<RawValue>, ErrorName> {
        let at = self.clock.now();
        let body = request.body().get().as_bytes();
        let cx = RefCell::new(cx);
        let mut host = Host {
            cx: &cx,
            agent: Bridge {
                cx: &cx,
                channel: &self.channel,
                clock: &*self.clock,
                fault: Cell::new(false),
            },
            at,
            day: self.clock.day(at),
        };
        self.channel.broken.set(false);
        let mut handlers = self.handlers.borrow_mut();
        let json = match request.name() {
            "runner" => handlers
                .runner(&mut host, body)
                .map_err(|e| e.error_name())?
                .to_json()
                .map_err(|_| ErrorName::Internal)?,
            "coordinator_write" => handlers
                .coordinator(&mut host, body)
                .map_err(|e| e.error_name())?
                .to_json()?,
            _ => return Err(ErrorName::Unsupported),
        };
        if self.channel.broken.get() {
            return Err(ErrorName::Internal);
        }
        if host.agent.fault.get() {
            return Err(ErrorName::Store);
        }
        RawValue::from_string(json).map_err(|_| ErrorName::Internal)
    }
}
