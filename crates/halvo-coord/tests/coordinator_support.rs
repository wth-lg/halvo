#![allow(dead_code)]

#[path = "run_support.rs"]
mod run_support;

use halvo_coord::mem::policy::Policy;
use halvo_coord::mem::{Call, Config, Ports};
use halvo_coord::ports::{Analysis, Clock, Owner, Page, PortError, Reaped, Rng, Runs, Tray};
use halvo_coord::types::{
    Agent, AgentId, AgentState, Answer, BindResult, Body, Class, Decision, Event, HandbackResult,
    Limits, Reply, RunHistory, RunId, RunSpec, Workflow, WorkflowId,
};
use halvo_coord::Coordinator;
use std::cell::Cell;

pub use run_support::{agent, config, finished_run, flow, workflow, A, DAY, MANUAL, T0};

pub const PERSON: A = A {
    person: true,
    slotted: false,
};

/// The memory ports and the memory policy as one value, which `Coordinator` needs; a test can
/// make the next `answer_confirm` fail after its record returned, so that an item stays
/// `recorded`, can have each call of `log` and `send` name the function that made it, and can
/// make `start`, `cancel` or `answer_confirm` panic.
pub struct Both {
    pub ports: Ports<Config>,
    pub policy: Policy<Config>,
    pub drop_decision: Cell<bool>,
    pub trace: Cell<bool>,
    pub callers: std::cell::RefCell<Vec<String>>,
    pub panics: Cell<Option<&'static str>>,
    pub loss: Cell<Option<Loss>>,
    pub lost: std::cell::RefCell<Vec<Event>>,
    pub next_history_fails: Cell<bool>,
    pub loss_waits_for_row: Cell<Option<LostAckRow>>,
    pub clock_fails: Cell<bool>,
    /// Each `Runs::answer_bind`: its run, its answer, whether it did its work, and how many
    /// events the owner held at the call.
    pub binds_at: std::cell::RefCell<Vec<(RunId, BindResult, bool, usize)>>,
    pub handbacks_at: std::cell::RefCell<Vec<(RunId, HandbackResult, bool, usize)>>,
}

pub type LostAckRow = fn(&halvo_coord::types::What) -> bool;

/// A lost acknowledgement of the next `log`, in one of two forms: the owner stores the rows and
/// the reply is lost, so that the coordinator's read right after the call brings them; or the
/// call is refused and the rows land in the owner's events after the move (`lost`), so that the
/// coordinator learns of them through a later refusal `seq`.
/// Two more forms: `Unread`, the rows stored and the reply lost as `Reply`, with the next read of
/// the owner's events failing too; and `Nothing`, the call answering `no_service` with nothing
/// stored, as a refusal whose reply was lost.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Loss {
    Reply,
    Later,
    Unread,
    Nothing,
}

impl Both {
    /// Notes the function that called `log` or `send`: the first frame of the stack below this
    /// wrapper's own method, which a target names `<..::support::Both as ..>::log` or, with
    /// MSVC, `..::support::impl$1::log`.
    fn note(&self, method: &str) {
        if !self.trace.get() {
            return;
        }
        let stack = std::backtrace::Backtrace::force_capture().to_string();
        let mut below = false;
        for line in stack.lines().map(str::trim_start) {
            if line.starts_with("at ") {
                continue;
            }
            let name = line
                .split_once(": ")
                .map_or(line, |(_, n)| n)
                .trim_start_matches('<');
            if name.contains("support::") && name.ends_with(method) {
                below = true;
                continue;
            }
            if below {
                self.callers
                    .borrow_mut()
                    .push(format!("{method} <- {name}"));
                return;
            }
        }
    }
}

impl Owner for Both {
    type Config = Config;

    fn log(&self, events: &[Event]) -> Result<(), PortError> {
        self.note("::log");
        let waits = self
            .loss_waits_for_row
            .get()
            .is_some_and(|names| !events.iter().any(|e| names(&e.what)));
        let loss = if waits {
            None
        } else {
            self.loss_waits_for_row.set(None);
            self.loss.take()
        };
        match loss {
            Some(Loss::Reply) => {
                self.ports.log(events)?;
                Err(PortError::NoService)
            }
            Some(Loss::Later) => {
                self.lost.borrow_mut().extend(events.iter().cloned());
                Err(PortError::NoService)
            }
            Some(Loss::Unread) => {
                self.ports.log(events)?;
                self.next_history_fails.set(true);
                Err(PortError::NoService)
            }
            Some(Loss::Nothing) => Err(PortError::NoService),
            None => self.ports.log(events),
        }
    }

    fn history(&self, agent: &AgentId, after: u32) -> Result<Vec<Event>, PortError> {
        if self.next_history_fails.take() {
            return Err(PortError::NoService);
        }
        self.ports.history(agent, after)
    }

    fn workflows(&self) -> Result<Vec<Workflow>, PortError> {
        self.ports.workflows()
    }

    fn agents(&self) -> Result<Vec<Agent<Config>>, PortError> {
        self.ports.agents()
    }

    fn send(&self, body: &Body) -> Result<Reply, PortError> {
        self.note("::send");
        self.ports.send(body)
    }

    fn agent_history(
        &self,
        workflow: &WorkflowId,
        agent: &AgentId,
        runs: u32,
    ) -> Result<Vec<RunHistory>, PortError> {
        self.ports.agent_history(workflow, agent, runs)
    }
}

impl Runs for Both {
    fn start(&self, spec: &RunSpec) -> Result<(), PortError> {
        if self.panics.get() == Some("start") {
            panic!("the port start panics");
        }
        self.ports.start(spec)
    }

    fn answer_confirm(
        &self,
        run: &RunId,
        answers: &[Answer],
        record: &mut dyn FnMut(&Decision) -> Result<(), PortError>,
    ) -> Result<(), PortError> {
        if self.panics.get() == Some("answer_confirm") {
            panic!("the port answer_confirm panics");
        }
        if self.drop_decision.replace(false) {
            let mut kept = Ok(());
            let _ = self.ports.answer_confirm(run, answers, &mut |d| {
                kept = record(d);
                kept.clone()
            });
            return kept.and(Err(PortError::NoService));
        }
        self.ports.answer_confirm(run, answers, record)
    }

    fn answer_handback(&self, run: &RunId, result: HandbackResult) -> Result<(), PortError> {
        let at = self.ports.events.borrow().len();
        let out = self.ports.answer_handback(run, result);
        self.handbacks_at
            .borrow_mut()
            .push((run.clone(), result, out.is_ok(), at));
        out
    }

    fn answer_bind(&self, run: &RunId, result: BindResult) -> Result<(), PortError> {
        let at = self.ports.events.borrow().len();
        let out = self.ports.answer_bind(run, result);
        self.binds_at
            .borrow_mut()
            .push((run.clone(), result, out.is_ok(), at));
        out
    }

    fn cancel(&self, run: &RunId) -> Result<(), PortError> {
        if self.panics.get() == Some("cancel") {
            panic!("the port cancel panics");
        }
        self.ports.cancel(run)
    }

    fn kill(&self, run: &RunId) -> Result<(), PortError> {
        self.ports.kill(run)
    }

    fn reap(&self) -> Result<Vec<(RunId, Reaped)>, PortError> {
        self.ports.reap()
    }
}

impl Analysis for Both {
    fn defer(&self, until: u64) -> Result<(), PortError> {
        self.ports.defer(until)
    }
}

impl Clock for Both {
    fn now(&self) -> Result<u64, PortError> {
        if self.clock_fails.get() {
            return Err(PortError::NoService);
        }
        self.ports.now()
    }

    fn today(&self) -> Result<u32, PortError> {
        self.ports.today()
    }

    fn day(&self, at: u64) -> Result<u32, PortError> {
        self.ports.day(at)
    }
}

impl Rng for Both {
    fn bytes16(&self) -> Result<[u8; 16], PortError> {
        self.ports.bytes16()
    }
}

impl Tray for Both {
    fn attention(&self, count: u32) -> Result<(), PortError> {
        self.ports.attention(count)
    }
}

impl Page for Both {
    fn changed(&self) -> Result<(), PortError> {
        self.ports.changed()
    }
}

impl halvo_coord::guard::Policy for Both {
    type Config = Config;

    fn gives_run(&self, config: &Config, class: Class) -> Result<bool, PortError> {
        self.policy.gives_run(config, class)
    }
}

/// The owner of a world: one workflow and one agent per entry of `spec`, each with one run
/// that completed before the first slot, as `run_support` makes them.
pub fn owner(spec: &[A], limits: Limits) -> Both {
    let ports: Ports<Config> = Ports::with_limits(T0, DAY, limits);
    for (n, a) in spec.iter().enumerate() {
        let (record, events) = finished_run(n);
        ports.events.borrow_mut().extend(events);
        ports
            .workflows
            .borrow_mut()
            .push(Workflow::Full(workflow(n, *a)));
        ports.agents.borrow_mut().push(Agent {
            workflow: flow(n),
            agent: agent(n),
            config: config(n, *a),
            state: AgentState::Active,
            slots: Vec::new(),
            session: true,
            last: Some(record),
            unknown: Vec::new(),
        });
    }
    Both {
        ports,
        policy: Policy::new(&[]),
        drop_decision: Cell::new(false),
        trace: Cell::new(false),
        callers: std::cell::RefCell::new(Vec::new()),
        panics: Cell::new(None),
        loss: Cell::new(None),
        lost: std::cell::RefCell::new(Vec::new()),
        next_history_fails: Cell::new(false),
        loss_waits_for_row: Cell::new(None),
        clock_fails: Cell::new(false),
        binds_at: std::cell::RefCell::new(Vec::new()),
        handbacks_at: std::cell::RefCell::new(Vec::new()),
    }
}

/// A coordinator over the owner of `spec` after its first tick, which recovers its life.
pub fn coordinator(spec: &[A], limits: Limits) -> Coordinator<Both> {
    let c = Coordinator::new(owner(spec, limits), limits);
    c.tick();
    c
}

/// The calls that did their work since `from`, with the clock read left out.
pub fn calls_since(c: &Coordinator<Both>, from: usize) -> Vec<Call> {
    c.ports().ports.calls.borrow()[from..]
        .iter()
        .map(|t| t.call.clone())
        .collect()
}

pub fn mark(c: &Coordinator<Both>) -> usize {
    c.ports().ports.calls.borrow().len()
}

/// The runs that the coordinator started, in order.
pub fn started(c: &Coordinator<Both>) -> Vec<RunId> {
    c.ports()
        .ports
        .calls
        .borrow()
        .iter()
        .filter_map(|t| match &t.call {
            Call::Start(spec) => Some(spec.run.clone()),
            _ => None,
        })
        .collect()
}
