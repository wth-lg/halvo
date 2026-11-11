use crate::ports::{Analysis, Clock, Owner, Page, PortError, Rng, Runs, Tray};
use crate::types::{
    hex, Agent, AgentId, AgentState, Answer, ApprovalId, Body, Category, ConfigView, Decision,
    DecisionEntry, EffectAnswer, Event, HandbackResult, Limits, Mode, Name, Outcome, PastRun,
    Reply, RunId, RunSpec, Runtime, What, Workflow, WorkflowId, WorkflowState,
};
use std::cell::{Cell, RefCell};

const HISTORY_MAX: usize = 64;

fn refusal(name: &str) -> PortError {
    PortError::Refused(Name::parse(name).expect("an error name of I0.6's form"))
}

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
    /// `Owner::send` with the body `end_run` alone.
    EndRun,
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
/// clock gives what the test set; the random bytes count up from 1; the owner lists what the
/// test put into `workflows` and `agents` and answers each body as A-A7.2 says; `Runs` mints
/// approval ids from a counter of its own, starts no process, and `reap` ends the trees that
/// the test left with `leave_trees`; a method that the test named in `refuse` answers that
/// error and does nothing else.
///
/// The owner keeps the refusals that the real one gives (I7.2, A-A7.2), each as
/// `PortError::Refused` with the error name, and a refusal stores nothing, records no call and
/// changes no state of the port. `log` refuses a body of more than `request_records` records
/// with `malformed`, and then, record by record, a workflow that is not in `workflows` with
/// `no_workflow`, an agent that is not the workflow's with `binding`, and a `seq` that is not
/// the last stored `seq` of its agent plus 1, the records before it in the same body counted,
/// with `seq`. An agent is the workflow's when the workflow of `workflows` names it, when an
/// entry of `agents` pairs the two, or once `send` acknowledged `create_agent` for the pair.
/// `send` answers `create_agent` and `set_state` as the state call of W-C25.7 that each makes:
/// `no_workflow` for a workflow that is not in `workflows`, and `state` for one that is
/// `proposed` or dismissed. `set_state` is refused with `no_workflow` first, then with
/// `playbook` for `active` and an agent whose runtime is `web` and which has no playbook (an
/// agent of `agents` by its configuration, and an agent that only a `create_agent` made by the
/// preset of its workflow's category, A-A1.3), then with `state`. `set_bindings` refuses
/// nothing, as A-A7.2 gives it no refusal. `start_run` is refused with `no_workflow` for a
/// workflow that is not in `workflows`, with `binding` for an agent that is not the workflow's,
/// and with `state` unless the workflow is `active`, or `approved` for the mode `compile`
/// (A-A7.2). `history` answers at most 64 events for one call.
/// `Runs::answer_confirm` refuses answers that make no `Decision` (none, or two for one
/// effect) with `malformed`, before it mints an id or calls `record`.
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
    limits: Limits,
    created: RefCell<Vec<(WorkflowId, AgentId)>>,
    trees: Cell<u32>,
}

impl<C> Ports<C> {
    /// Memory ports with the clock at `now` milliseconds and on the day `today`, and the
    /// product's limits.
    pub fn new(now: u64, today: u32) -> Ports<C> {
        Ports::with_limits(now, today, Limits::PRODUCT)
    }

    /// Memory ports as `new` makes them, whose owner keeps the `request_records` of `limits`.
    pub fn with_limits(now: u64, today: u32, limits: Limits) -> Ports<C> {
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
            limits,
            created: RefCell::new(Vec::new()),
            trees: Cell::new(0),
        }
    }

    /// Ends a run in the agent store as the store's handler of its end does: the agent's entry
    /// in `agents` gets the run's record as its `last` (I6.1, A-A7.4), which the coordinator
    /// reads to know that the store has ended the agent's newest run (ruling XB-2).
    pub fn store_end(&self, agent: &AgentId, run: PastRun) {
        if let Some(a) = self
            .agents
            .borrow_mut()
            .iter_mut()
            .find(|a| &a.agent == agent)
        {
            a.last = Some(run);
        }
    }

    fn end_in_store(&self, run: &RunId) {
        let started = self.events.borrow().iter().find_map(|e| match &e.what {
            What::RunStarted { run: r, .. } if r == run => Some((e.agent.clone(), e.day)),
            _ => None,
        });
        let Some((agent, day)) = started else {
            return;
        };
        let ended = self
            .agents
            .borrow()
            .iter()
            .any(|a| a.agent == agent && a.last.as_ref().is_some_and(|l| &l.run == run));
        if !ended {
            self.store_end(
                &agent,
                PastRun {
                    run: run.clone(),
                    day,
                    outcome: Outcome::Failed,
                    unknown: 0,
                },
            );
        }
    }

    /// Leaves `count` runner trees of an earlier life of the app, which the next `Runs::reap`
    /// ends and counts.
    pub fn leave_trees(&self, count: u32) {
        self.trees.set(self.trees.get().saturating_add(count));
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

    fn bound(&self, workflow: &WorkflowId, agent: &AgentId) -> bool {
        let named = self.workflows.borrow().iter().any(|w| match w {
            Workflow::Full(f) => &f.id == workflow && f.agent.as_ref() == Some(agent),
            Workflow::Dismissed(_) => false,
        });
        named
            || self
                .agents
                .borrow()
                .iter()
                .any(|a| &a.workflow == workflow && &a.agent == agent)
            || self
                .created
                .borrow()
                .iter()
                .any(|(w, a)| w == workflow && a == agent)
    }

    fn log_refusal(&self, events: &[Event]) -> Option<&'static str> {
        if events.len() > self.limits.request_records as usize {
            return Some("malformed");
        }
        let stored = self.events.borrow();
        let mut last: Vec<(&AgentId, u32)> = Vec::new();
        for e in events {
            if !self
                .workflows
                .borrow()
                .iter()
                .any(|w| w.id() == &e.workflow)
            {
                return Some("no_workflow");
            }
            if !self.bound(&e.workflow, &e.agent) {
                return Some("binding");
            }
            let before = match last.iter().rev().find(|(a, _)| *a == &e.agent) {
                Some((_, seq)) => *seq,
                None => stored
                    .iter()
                    .filter(|s| s.agent == e.agent)
                    .map(|s| s.seq.get())
                    .max()
                    .unwrap_or(0),
            };
            if before.checked_add(1) != Some(e.seq.get()) {
                return Some("seq");
            }
            last.push((&e.agent, e.seq.get()));
        }
        None
    }

    fn approval(&self) -> ApprovalId {
        let n = self.approvals.get() + 1;
        self.approvals.set(n);
        ApprovalId::parse(&hex(n.to_be_bytes())).expect("32 hex digits")
    }

    fn decision(
        &self,
        answers: &[Answer],
        answer: Option<EffectAnswer>,
    ) -> Result<Decision, PortError> {
        let minted = self.approvals.get();
        let entries = answers
            .iter()
            .map(|a| DecisionEntry {
                approval: self.approval(),
                effect: a.effect.clone(),
                digest: a.digest.clone(),
                answer: answer.unwrap_or(a.answer),
            })
            .collect();
        Decision::new(entries).map_err(|_| {
            self.approvals.set(minted);
            refusal("malformed")
        })
    }
}

fn preset_runtime(category: Category) -> Runtime {
    match category {
        Category::UtilityBillPayment | Category::ShopOrder => Runtime::Web,
        Category::Email => Runtime::GraphMail,
        Category::CalendarCleanup => Runtime::GraphCalendar,
        Category::SpreadsheetFill => Runtime::File,
        Category::TenantRent
        | Category::LeaseRenewalOrChange
        | Category::TenantApplicationReview
        | Category::RideOrder
        | Category::DailyChartCheck => Runtime::None,
    }
}

impl<C: ConfigView> Ports<C> {
    fn web_with_no_playbook(&self, workflow: &WorkflowId, agent: &AgentId) -> bool {
        if let Some(a) = self.agents.borrow().iter().find(|a| &a.agent == agent) {
            return a.config.runtime() == Runtime::Web && a.config.playbook().is_none();
        }
        let created = self
            .created
            .borrow()
            .iter()
            .any(|(w, a)| w == workflow && a == agent);
        let category = self.workflows.borrow().iter().find_map(|w| match w {
            Workflow::Full(f) if &f.id == workflow => Some(f.category),
            Workflow::Dismissed(d) if &d.id == workflow => Some(d.category),
            _ => None,
        });
        created && category.is_some_and(|c| preset_runtime(c) == Runtime::Web)
    }

    fn state_call_refusal(&self, workflow: &WorkflowId) -> Option<&'static str> {
        match self.workflows.borrow().iter().find(|w| w.id() == workflow) {
            None => Some("no_workflow"),
            Some(Workflow::Dismissed(_)) => Some("state"),
            Some(Workflow::Full(f)) => (f.state == WorkflowState::Proposed).then_some("state"),
        }
    }

    fn send_refusal(&self, body: &Body) -> Option<&'static str> {
        match body {
            Body::CreateAgent { workflow, .. } => self.state_call_refusal(workflow),
            Body::SetState {
                workflow,
                agent,
                state,
            } => {
                if !self.workflows.borrow().iter().any(|w| w.id() == workflow) {
                    return Some("no_workflow");
                }
                if *state == AgentState::Active && self.web_with_no_playbook(workflow, agent) {
                    return Some("playbook");
                }
                self.state_call_refusal(workflow)
            }
            Body::StartRun {
                workflow,
                agent,
                mode,
                ..
            } => {
                let state = match self.workflows.borrow().iter().find(|w| w.id() == workflow) {
                    None => return Some("no_workflow"),
                    Some(Workflow::Full(f)) => Some(f.state),
                    Some(Workflow::Dismissed(_)) => None,
                };
                if !self.bound(workflow, agent) {
                    return Some("binding");
                }
                let startable = matches!(
                    (state, mode),
                    (Some(WorkflowState::Active), _)
                        | (Some(WorkflowState::Approved), Mode::Compile)
                );
                (!startable).then_some("state")
            }
            _ => None,
        }
    }
}

impl<C: ConfigView + Clone> Owner for Ports<C> {
    type Config = C;

    fn log(&self, events: &[Event]) -> Result<(), PortError> {
        self.refused(Method::Log)?;
        if let Some(name) = self.log_refusal(events) {
            return Err(refusal(name));
        }
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
            .filter(|e| &e.agent == agent && e.seq.get() > after)
            .cloned()
            .collect();
        found.sort_by_key(|e| e.seq);
        found.truncate(HISTORY_MAX);
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
        if matches!(body, Body::EndRun { .. }) {
            self.refused(Method::EndRun)?;
        }
        if let Some(name) = self.send_refusal(body) {
            return Err(refusal(name));
        }
        self.record(Call::Send(body.clone()));
        Ok(match body {
            Body::CreateAgent { workflow, agent } => {
                let pair = (workflow.clone(), agent.clone());
                let mut created = self.created.borrow_mut();
                if !created.contains(&pair) {
                    created.push(pair);
                }
                serde_json::json!({ "rev": 1 })
            }
            Body::SetBindings { .. } => {
                let rev = self.bindings_rev.get() + 1;
                self.bindings_rev.set(rev);
                serde_json::json!({ "rev": rev })
            }
            Body::SetState { state, .. } => serde_json::json!({ "state": state.name() }),
            Body::StartRun { .. } => serde_json::json!({ "ok": "yes" }),
            Body::EndRun { run } => {
                self.end_in_store(run);
                serde_json::json!({ "ok": "yes" })
            }
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
        let decision = self.decision(answers, None)?;
        match record(&decision) {
            Ok(()) => {
                self.record(Call::AnswerConfirm(run.clone(), answers.to_vec(), decision));
                Ok(())
            }
            Err(e) => {
                let sent = self.decision(answers, Some(EffectAnswer::RejectOnce))?;
                self.record(Call::AnswerConfirm(run.clone(), answers.to_vec(), sent));
                Err(e)
            }
        }
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
        Ok(self.trees.replace(0))
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
