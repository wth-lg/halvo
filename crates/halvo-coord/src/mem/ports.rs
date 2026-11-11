use crate::ports::{Analysis, Clock, Owner, Page, PortError, Rng, Runs, Tray};
use crate::types::{
    hex, Agent, AgentId, Answer, ApprovalId, Body, ConfigView, Decision, DecisionEntry,
    EffectAnswer, Event, HandbackResult, Reply, RunId, RunSpec, Workflow,
};
use std::cell::{Cell, RefCell};

/// A port method, by which a test makes it refuse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    /// `Owner::log`.
    Log,
    /// `Owner::history`.
    History,
    /// `Owner::workflows`.
    Workflows,
    /// `Owner::agents`.
    Agents,
    /// `Owner::send`.
    Send,
    /// `Runs::start`.
    Start,
    /// `Runs::answer_confirm`.
    AnswerConfirm,
    /// `Runs::answer_handback`.
    AnswerHandback,
    /// `Runs::cancel`.
    Cancel,
    /// `Runs::kill`.
    Kill,
    /// `Runs::reap`.
    Reap,
    /// `Analysis::defer`.
    Defer,
    /// `Tray::attention`.
    Attention,
    /// `Page::changed`.
    Changed,
}

/// A call of a port that changes or reads the world outside the coordinator; the reads of the
/// clock and of the random bytes are not recorded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Call {
    /// `Owner::log` with its events.
    Log(Vec<Event>),
    /// `Owner::history` with its agent and `after`.
    History(AgentId, u32),
    /// `Owner::workflows`.
    Workflows,
    /// `Owner::agents`.
    Agents,
    /// `Owner::send` with its body.
    Send(Body),
    /// `Runs::start` with its spec.
    Start(RunSpec),
    /// `Runs::answer_confirm` with its run, its answers and the decision that it sent.
    AnswerConfirm(RunId, Vec<Answer>, Decision),
    /// `Runs::answer_handback` with its run and result.
    AnswerHandback(RunId, HandbackResult),
    /// `Runs::cancel` with its run.
    Cancel(RunId),
    /// `Runs::kill` with its run.
    Kill(RunId),
    /// `Runs::reap`.
    Reap,
    /// `Analysis::defer` with its time.
    Defer(u64),
    /// `Tray::attention` with its count.
    Attention(u32),
    /// `Page::changed`.
    Changed,
}

impl Call {
    /// The name of the port method of the call.
    pub fn method(&self) -> &'static str {
        match self {
            Call::Log(_) => "log",
            Call::History(..) => "history",
            Call::Workflows => "workflows",
            Call::Agents => "agents",
            Call::Send(_) => "send",
            Call::Start(_) => "start",
            Call::AnswerConfirm(..) => "answer_confirm",
            Call::AnswerHandback(..) => "answer_handback",
            Call::Cancel(_) => "cancel",
            Call::Kill(_) => "kill",
            Call::Reap => "reap",
            Call::Defer(_) => "defer",
            Call::Attention(_) => "attention",
            Call::Changed => "changed",
        }
    }
}

/// A call with the clock's time when it was made, in milliseconds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Timed {
    /// `Clock::now()` at the call.
    pub at: u64,
    /// The call.
    pub call: Call,
}

/// Every port in memory, one value for `Coordinator<P>` and the `drive` functions (I5.10). The
/// clock gives what the test set; the random bytes count up from 1; the owner stores what
/// `log` hands it, lists what the test put into `workflows` and `agents`, and answers each body
/// as A-A7.2 says; `Runs` mints approval ids from a counter of its own and starts no process;
/// a method that the test named in `refuse` answers that error and does nothing else.
pub struct Ports<C> {
    /// The owner's list of workflows.
    pub workflows: RefCell<Vec<Workflow>>,
    /// The agents that `Owner::agents` lists.
    pub agents: RefCell<Vec<Agent<C>>>,
    /// The events that `Owner::log` acknowledged, in the order of the calls.
    pub events: RefCell<Vec<Event>>,
    /// Every recorded call, in order.
    pub calls: RefCell<Vec<Timed>>,
    now: Cell<u64>,
    today: Cell<u32>,
    bytes: Cell<u128>,
    approvals: Cell<u128>,
    bindings_rev: Cell<u32>,
    refusals: RefCell<Vec<(Method, PortError)>>,
}

impl<C> Ports<C> {
    /// Memory ports with the clock at `now` milliseconds and on the day `today`.
    pub fn new(now: u64, today: u32) -> Ports<C> {
        Ports {
            workflows: RefCell::new(Vec::new()),
            agents: RefCell::new(Vec::new()),
            events: RefCell::new(Vec::new()),
            calls: RefCell::new(Vec::new()),
            now: Cell::new(now),
            today: Cell::new(today),
            bytes: Cell::new(0),
            approvals: Cell::new(0),
            bindings_rev: Cell::new(1),
            refusals: RefCell::new(Vec::new()),
        }
    }

    /// Sets the clock.
    pub fn set_clock(&self, now: u64, today: u32) {
        self.now.set(now);
        self.today.set(today);
    }

    /// Makes `method` answer `error` until `clear_refusals`.
    pub fn refuse(&self, method: Method, error: PortError) {
        let mut r = self.refusals.borrow_mut();
        r.retain(|(m, _)| *m != method);
        r.push((method, error));
    }

    /// Makes every method do its work again.
    pub fn clear_refusals(&self) {
        self.refusals.borrow_mut().clear();
    }

    /// The decisions that `Runs::answer_confirm` sent, in order.
    pub fn sent_decisions(&self) -> Vec<Decision> {
        self.calls
            .borrow()
            .iter()
            .filter_map(|t| match &t.call {
                Call::AnswerConfirm(_, _, d) => Some(d.clone()),
                _ => None,
            })
            .collect()
    }

    fn refused(&self, method: Method) -> Result<(), PortError> {
        match self.refusals.borrow().iter().find(|(m, _)| *m == method) {
            Some((_, e)) => Err(e.clone()),
            None => Ok(()),
        }
    }

    fn record(&self, call: Call) {
        self.calls.borrow_mut().push(Timed {
            at: self.now.get(),
            call,
        });
    }

    fn approval(&self) -> ApprovalId {
        let n = self.approvals.get() + 1;
        self.approvals.set(n);
        ApprovalId::parse(&hex(n.to_be_bytes())).expect("32 hex digits")
    }

    fn decision(&self, answers: &[Answer], answer: Option<EffectAnswer>) -> Decision {
        Decision {
            approvals: answers
                .iter()
                .map(|a| DecisionEntry {
                    approval: self.approval(),
                    effect: a.effect.clone(),
                    digest: a.digest.clone(),
                    answer: answer.unwrap_or(a.answer),
                })
                .collect(),
        }
    }
}

impl<C: ConfigView + Clone> Owner for Ports<C> {
    type Config = C;

    fn log(&self, events: &[Event]) -> Result<(), PortError> {
        self.refused(Method::Log)?;
        self.record(Call::Log(events.to_vec()));
        self.events.borrow_mut().extend_from_slice(events);
        Ok(())
    }

    fn history(&self, agent: &AgentId, after: u32) -> Result<Vec<Event>, PortError> {
        self.refused(Method::History)?;
        self.record(Call::History(agent.clone(), after));
        let mut found: Vec<Event> = self
            .events
            .borrow()
            .iter()
            .filter(|e| &e.agent == agent && e.seq > after)
            .cloned()
            .collect();
        found.sort_by_key(|e| e.seq);
        Ok(found)
    }

    fn workflows(&self) -> Result<Vec<Workflow>, PortError> {
        self.refused(Method::Workflows)?;
        self.record(Call::Workflows);
        Ok(self.workflows.borrow().clone())
    }

    fn agents(&self) -> Result<Vec<Agent<C>>, PortError> {
        self.refused(Method::Agents)?;
        self.record(Call::Agents);
        Ok(self.agents.borrow().clone())
    }

    fn send(&self, body: &Body) -> Result<Reply, PortError> {
        self.refused(Method::Send)?;
        self.record(Call::Send(body.clone()));
        Ok(match body {
            Body::CreateAgent { .. } => serde_json::json!({ "rev": 1 }),
            Body::SetBindings { .. } => {
                let rev = self.bindings_rev.get() + 1;
                self.bindings_rev.set(rev);
                serde_json::json!({ "rev": rev })
            }
            Body::SetState { state, .. } => serde_json::json!({ "state": state.name() }),
            Body::StartRun { .. } => serde_json::json!({ "ok": "yes" }),
        })
    }
}

impl<C> Runs for Ports<C> {
    fn start(&self, spec: &RunSpec) -> Result<(), PortError> {
        self.refused(Method::Start)?;
        self.record(Call::Start(spec.clone()));
        Ok(())
    }

    fn answer_confirm(
        &self,
        run: &RunId,
        answers: &[Answer],
        record: &mut dyn FnMut(&Decision) -> Result<(), PortError>,
    ) -> Result<(), PortError> {
        self.refused(Method::AnswerConfirm)?;
        let decision = self.decision(answers, None);
        let sent = match record(&decision) {
            Ok(()) => decision,
            Err(_) => self.decision(answers, Some(EffectAnswer::RejectOnce)),
        };
        self.record(Call::AnswerConfirm(run.clone(), answers.to_vec(), sent));
        Ok(())
    }

    fn answer_handback(&self, run: &RunId, result: HandbackResult) -> Result<(), PortError> {
        self.refused(Method::AnswerHandback)?;
        self.record(Call::AnswerHandback(run.clone(), result));
        Ok(())
    }

    fn cancel(&self, run: &RunId) -> Result<(), PortError> {
        self.refused(Method::Cancel)?;
        self.record(Call::Cancel(run.clone()));
        Ok(())
    }

    fn kill(&self, run: &RunId) -> Result<(), PortError> {
        self.refused(Method::Kill)?;
        self.record(Call::Kill(run.clone()));
        Ok(())
    }

    fn reap(&self) -> Result<u32, PortError> {
        self.refused(Method::Reap)?;
        self.record(Call::Reap);
        Ok(0)
    }
}

impl<C> Analysis for Ports<C> {
    fn defer(&self, until: u64) -> Result<(), PortError> {
        self.refused(Method::Defer)?;
        self.record(Call::Defer(until));
        Ok(())
    }
}

impl<C> Clock for Ports<C> {
    fn now(&self) -> Result<u64, PortError> {
        Ok(self.now.get())
    }

    fn today(&self) -> Result<u32, PortError> {
        Ok(self.today.get())
    }
}

impl<C> Rng for Ports<C> {
    fn bytes16(&self) -> Result<[u8; 16], PortError> {
        let n = self.bytes.get() + 1;
        self.bytes.set(n);
        Ok(n.to_be_bytes())
    }
}

impl<C> Tray for Ports<C> {
    fn attention(&self, count: u32) -> Result<(), PortError> {
        self.refused(Method::Attention)?;
        self.record(Call::Attention(count));
        Ok(())
    }
}

impl<C> Page for Ports<C> {
    fn changed(&self) -> Result<(), PortError> {
        self.refused(Method::Changed)?;
        self.record(Call::Changed);
        Ok(())
    }
}
