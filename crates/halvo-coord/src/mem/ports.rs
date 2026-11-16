use crate::items::drive::log_bytes;
use crate::ports::{Analysis, Clock, Owner, Page, PortError, Reaped, Rng, Runs, Tray};
use crate::types::{
    hex, Agent, AgentId, AgentState, Answer, ApprovalId, BindResult, Body, Category, ConfigView,
    Decision, DecisionEntry, EffectAnswer, Event, HandbackResult, Limits, Mode, Name, Outcome,
    PastRun, Reply, RunHistory, RunId, RunSpec, Runtime, Slot, What, Workflow, WorkflowId,
    WorkflowState,
};
use std::cell::{Cell, RefCell};

const HISTORY_MAX: usize = 64;
const DAY_MS: i64 = 86_400_000;

fn refusal(name: &str) -> PortError {
    PortError::Refused(Name::parse(name).expect("an error name of I0.6's form"))
}

/// The bindings that `set_bindings` stored for one agent of one workflow: their slots and the
/// number of the newest revision, 1 for the first body (ruling IG-20).
#[derive(Clone, Debug)]
struct Bound {
    workflow: WorkflowId,
    agent: AgentId,
    slots: Vec<Slot>,
    rev: u32,
}

/// A port method, by which a test makes it refuse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    /// `Owner::log`.
    Log,
    /// `Owner::log` with a body that holds a `run_started` event, and no other body (ruling
    /// XB-12).
    LogRunStarted,
    /// `Owner::history`.
    History,
    /// `Owner::workflows`.
    Workflows,
    /// `Owner::agents`.
    Agents,
    /// `Owner::send`.
    Send,
    /// `Owner::send` with the body `end_run` alone, of a run whose workflow the owner's list
    /// holds: for a run of a removed workflow the owner answers `no_workflow` first.
    EndRun,
    /// `Owner::send` with the body `start_run` alone.
    StartRun,
    /// `Runs::start`.
    Start,
    /// `Runs::answer_confirm`.
    AnswerConfirm,
    /// `Runs::answer_handback`.
    AnswerHandback,
    /// `Runs::answer_bind`.
    AnswerBind,
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
    /// `Owner::agent_history`.
    AgentHistory,
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
    /// `Runs::answer_bind` with its run and answer.
    AnswerBind(RunId, BindResult),
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
            Call::AnswerBind(..) => "answer_bind",
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

/// A call that the coordinator made, with the refusal that answered it or none: the port
/// column of a trace line, in which a refused call stands as it was made (rulings XB-12,
/// XB-18).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tried {
    /// `Clock::now()` at the call.
    pub at: u64,
    /// The call.
    pub call: Call,
    /// The refusal, or none for a call that did its work.
    pub refused: Option<PortError>,
}

/// Every port in memory, one value for `Coordinator<P>` and the `drive` functions (I5.10). The
/// clock gives the instant that the test set and the day of an instant in its zone, a whole
/// number of days east of UTC that `new` and `set_clock` choose so that the day of `now` is
/// `today`, until `set_zone` sets another; with `step_reads` each read of `now` moves it on.
/// The random bytes count up from 1; the owner lists what the
/// test put into `workflows` and `agents` and answers each body as A-A7.2 says; `Runs` mints
/// approval ids from a counter of its own, starts no process, and `reap` answers the marks
/// that the test left with `leave_marks`; a method that the test named in `refuse` answers
/// that error and does nothing else. `tried` records every call as it was made, a refused one
/// among them with its refusal, and `calls` the calls that did their work.
///
/// `Runs` answers as the supervisor library does (I5.10, A-A6.6): `started` for a `start` of a
/// run id that this life of the library already started, and no second runner is made;
/// `unknown_run` for `cancel`, `kill`, `answer_handback` and `answer_bind` of a run id that this
/// life never started; `ended` for `cancel`, `answer_handback` and `answer_bind` of a run whose
/// runner has ended, by
/// `end_runner` or by a kill, while `kill` of such a run succeeds. `new_life` is a new process
/// of the library: it knows no runner of the earlier one, and the marks stay. `reap` answers
/// every mark once with its answer and keeps only the marks that it answered `alive`.
///
/// The owner keeps the refusals that the real one gives (I7.2, A-A7.2), each as
/// `PortError::Refused` with the error name, and a refusal stores nothing, records no call and
/// changes no state of the port. `log` refuses a body of more than `request_records` records,
/// or whose text (`items::drive::log_text`) has more than `request_bytes` bytes, with
/// `malformed`, and then, record by record, a workflow that is not in `workflows` with
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
/// nothing, as A-A7.2 gives it no refusal, and merges its slots by name into the slots stored
/// for its workflow and agent, as the agent-setup run's handler does (its CP-36): a slot of the
/// body replaces the stored slot of its name or is added, and a slot that the body does not name
/// stays; the agent's entry of `agents` then lists each name, and the answer is the revision
/// that it stored: 1 for the first body of the agent, whatever `create_agent` answered, and the
/// next number for each later one (ruling IG-20). `start_run` is refused with `no_workflow` for a
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
    /// Every call as it was made, in order, a refused one with its refusal.
    pub tried: RefCell<Vec<Tried>>,
    /// The runs that `Owner::agent_history` gives, each with its agent, newest first.
    pub histories: RefCell<Vec<(AgentId, RunHistory)>>,
    now: Cell<u64>,
    zone: Cell<i64>,
    step: Cell<u64>,
    bytes: Cell<u128>,
    approvals: Cell<u128>,
    bound_slots: RefCell<Vec<Bound>>,
    refusals: RefCell<Vec<(Method, PortError)>>,
    limits: Limits,
    created: RefCell<Vec<(WorkflowId, AgentId)>>,
    marks: RefCell<Vec<(RunId, Reaped)>>,
    runners: RefCell<Vec<RunId>>,
    ended: RefCell<Vec<RunId>>,
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
            tried: RefCell::new(Vec::new()),
            histories: RefCell::new(Vec::new()),
            now: Cell::new(now),
            zone: Cell::new(zone_of(now, today)),
            step: Cell::new(0),
            bytes: Cell::new(0),
            approvals: Cell::new(0),
            bound_slots: RefCell::new(Vec::new()),
            refusals: RefCell::new(Vec::new()),
            limits,
            created: RefCell::new(Vec::new()),
            marks: RefCell::new(Vec::new()),
            runners: RefCell::new(Vec::new()),
            ended: RefCell::new(Vec::new()),
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

    /// Whether `run` is a run of a workflow that the owner's list no longer holds: the owner
    /// removed every row that names the workflow and refuses a body of it with `no_workflow`
    /// (I5.7, I7.2), before an answer of the test's own refusal of `end_run`.
    fn removed_run(&self, run: &RunId) -> bool {
        let workflow = self.events.borrow().iter().find_map(|e| match &e.what {
            What::RunStarted { run: r, .. } if r == run => Some(e.workflow.clone()),
            _ => None,
        });
        workflow.is_some_and(|w| !self.workflows.borrow().iter().any(|x| x.id() == &w))
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

    /// The slots that `set_bindings` stored for `agent` of `workflow`, in the order of their
    /// first binding.
    pub fn slots(&self, workflow: &WorkflowId, agent: &AgentId) -> Vec<Slot> {
        self.bound_slots
            .borrow()
            .iter()
            .find(|b| &b.workflow == workflow && &b.agent == agent)
            .map(|b| b.slots.clone())
            .unwrap_or_default()
    }

    fn merge_slots(&self, workflow: &WorkflowId, agent: &AgentId, slots: &[Slot]) -> u32 {
        let mut bound = self.bound_slots.borrow_mut();
        let at = match bound
            .iter()
            .position(|b| &b.workflow == workflow && &b.agent == agent)
        {
            Some(at) => at,
            None => {
                bound.push(Bound {
                    workflow: workflow.clone(),
                    agent: agent.clone(),
                    slots: Vec::new(),
                    rev: 0,
                });
                bound.len() - 1
            }
        };
        bound[at].rev = bound[at].rev.saturating_add(1);
        let rev = bound[at].rev;
        let stored = &mut bound[at].slots;
        for slot in slots {
            match stored.iter_mut().find(|s| s.name == slot.name) {
                Some(s) => s.value.clone_from(&slot.value),
                None => stored.push(slot.clone()),
            }
        }
        for entry in self
            .agents
            .borrow_mut()
            .iter_mut()
            .filter(|e| &e.workflow == workflow && &e.agent == agent)
        {
            for slot in slots {
                if !entry.slots.contains(&slot.name) {
                    entry.slots.push(slot.name.clone());
                }
            }
        }
        rev
    }

    /// Leaves marks of the supervisor library, each with the answer that `Runs::reap` gives
    /// for it (I5.10); a mark of a run that the library holds already takes the new answer.
    pub fn leave_marks(&self, marks: &[(RunId, Reaped)]) {
        let mut held = self.marks.borrow_mut();
        for (run, answer) in marks {
            match held.iter_mut().find(|(r, _)| r == run) {
                Some(mark) => mark.1 = *answer,
                None => held.push((run.clone(), *answer)),
            }
        }
    }

    /// A runner of `run` stands in this life of the supervisor library, as after its
    /// `Runs::start`, with no call recorded.
    pub fn runner_of(&self, run: &RunId) {
        let mut runners = self.runners.borrow_mut();
        if !runners.contains(run) {
            runners.push(run.clone());
        }
    }

    /// The runner of `run` ended by itself: with its last frame, or gone with none.
    pub fn end_runner(&self, run: &RunId) {
        let mut ended = self.ended.borrow_mut();
        if !ended.contains(run) {
            ended.push(run.clone());
        }
    }

    /// A new process of the supervisor library: it knows no runner of the earlier one, and the
    /// marks stay.
    pub fn new_life(&self) {
        self.runners.borrow_mut().clear();
        self.ended.borrow_mut().clear();
    }

    /// Sets the clock to `now` in the zone in which the day of `now` is `today`.
    pub fn set_clock(&self, now: u64, today: u32) {
        self.now.set(now);
        self.zone.set(zone_of(now, today));
    }

    /// Moves the clock to `now` and keeps its zone, so that its day follows the instant.
    pub fn set_now(&self, now: u64) {
        self.now.set(now);
    }

    /// Sets the clock's zone, in milliseconds east of UTC, and keeps its instant.
    pub fn set_zone(&self, zone_ms: i64) {
        self.zone.set(zone_ms);
    }

    /// Makes each later read of `now` move the clock on by `ms` after it answers, as a clock
    /// that runs between two reads; 0 stops it.
    pub fn step_reads(&self, ms: u64) {
        self.step.set(ms);
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
        let at = self.now.get();
        self.tried.borrow_mut().push(Tried {
            at,
            call: call.clone(),
            refused: None,
        });
        self.calls.borrow_mut().push(Timed { at, call });
    }

    fn note(&self, call: Call, error: PortError) -> PortError {
        self.tried.borrow_mut().push(Tried {
            at: self.now.get(),
            call,
            refused: Some(error.clone()),
        });
        error
    }

    fn checked(&self, method: Method, call: &Call) -> Result<(), PortError> {
        self.refused(method).map_err(|e| self.note(call.clone(), e))
    }

    fn runner(&self, run: &RunId, call: &Call, ended_too: bool) -> Result<(), PortError> {
        if !self.runners.borrow().contains(run) {
            return Err(self.note(call.clone(), refusal("unknown_run")));
        }
        if ended_too && self.ended.borrow().contains(run) {
            return Err(self.note(call.clone(), refusal("ended")));
        }
        Ok(())
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
        if events.len() > self.limits.request_records as usize
            || log_bytes(events) > self.limits.request_bytes as usize
        {
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

    fn another_agent(&self, workflow: &WorkflowId, agent: &AgentId) -> bool {
        let listed = self.workflows.borrow().iter().any(|w| match w {
            Workflow::Full(f) => &f.id == workflow && f.agent.as_ref().is_some_and(|a| a != agent),
            Workflow::Dismissed(_) => false,
        });
        listed
            || self
                .agents
                .borrow()
                .iter()
                .any(|a| &a.workflow == workflow && &a.agent != agent)
            || self
                .created
                .borrow()
                .iter()
                .any(|(w, a)| w == workflow && a != agent)
    }

    fn send_refusal(&self, body: &Body) -> Option<&'static str> {
        match body {
            Body::CreateAgent { workflow, agent } => self
                .state_call_refusal(workflow)
                .or_else(|| self.another_agent(workflow, agent).then_some("binding")),
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
        let call = Call::Log(events.to_vec());
        self.checked(Method::Log, &call)?;
        if events
            .iter()
            .any(|e| matches!(e.what, What::RunStarted { .. }))
        {
            self.checked(Method::LogRunStarted, &call)?;
        }
        if let Some(name) = self.log_refusal(events) {
            return Err(self.note(call, refusal(name)));
        }
        self.record(call);
        self.events.borrow_mut().extend_from_slice(events);
        Ok(())
    }

    fn history(&self, agent: &AgentId, after: u32) -> Result<Vec<Event>, PortError> {
        self.checked(Method::History, &Call::History(agent.clone(), after))?;
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
        self.checked(Method::Workflows, &Call::Workflows)?;
        self.record(Call::Workflows);
        Ok(self.workflows.borrow().clone())
    }

    fn agents(&self) -> Result<Vec<Agent<C>>, PortError> {
        self.checked(Method::Agents, &Call::Agents)?;
        self.record(Call::Agents);
        Ok(self.agents.borrow().clone())
    }

    fn send(&self, body: &Body) -> Result<Reply, PortError> {
        let call = Call::Send(body.clone());
        self.checked(Method::Send, &call)?;
        match body {
            Body::EndRun { run } => {
                if self.removed_run(run) {
                    return Err(self.note(call, refusal("no_workflow")));
                }
                self.checked(Method::EndRun, &call)?
            }
            Body::StartRun { .. } => self.checked(Method::StartRun, &call)?,
            _ => {}
        }
        if let Some(name) = self.send_refusal(body) {
            return Err(self.note(call, refusal(name)));
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
            Body::SetBindings {
                workflow,
                agent,
                slots,
            } => {
                let rev = self.merge_slots(workflow, agent, slots);
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

    fn agent_history(
        &self,
        workflow: &WorkflowId,
        agent: &AgentId,
        runs: u32,
    ) -> Result<Vec<RunHistory>, PortError> {
        self.refused(Method::AgentHistory)?;
        if !self.workflows.borrow().iter().any(|w| w.id() == workflow) {
            return Err(refusal("no_workflow"));
        }
        if !self.bound(workflow, agent) {
            return Err(refusal("binding"));
        }
        Ok(self
            .histories
            .borrow()
            .iter()
            .filter(|(a, _)| a == agent)
            .map(|(_, h)| h.clone())
            .take(runs as usize)
            .collect())
    }
}

impl<C> Runs for Ports<C> {
    fn start(&self, spec: &RunSpec) -> Result<(), PortError> {
        let call = Call::Start(spec.clone());
        self.checked(Method::Start, &call)?;
        if self.runners.borrow().contains(&spec.run) {
            return Err(self.note(call, refusal("started")));
        }
        self.record(call);
        self.runners.borrow_mut().push(spec.run.clone());
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
        let call = Call::AnswerHandback(run.clone(), result);
        self.checked(Method::AnswerHandback, &call)?;
        self.runner(run, &call, true)?;
        self.record(call);
        Ok(())
    }

    fn answer_bind(&self, run: &RunId, result: BindResult) -> Result<(), PortError> {
        let call = Call::AnswerBind(run.clone(), result);
        self.checked(Method::AnswerBind, &call)?;
        self.runner(run, &call, true)?;
        self.record(call);
        Ok(())
    }

    fn cancel(&self, run: &RunId) -> Result<(), PortError> {
        let call = Call::Cancel(run.clone());
        self.checked(Method::Cancel, &call)?;
        self.runner(run, &call, true)?;
        self.record(call);
        Ok(())
    }

    fn kill(&self, run: &RunId) -> Result<(), PortError> {
        let call = Call::Kill(run.clone());
        self.checked(Method::Kill, &call)?;
        self.runner(run, &call, false)?;
        self.record(call);
        self.end_runner(run);
        Ok(())
    }

    fn reap(&self) -> Result<Vec<(RunId, Reaped)>, PortError> {
        self.checked(Method::Reap, &Call::Reap)?;
        self.record(Call::Reap);
        let marks = self.marks.borrow().clone();
        self.marks
            .borrow_mut()
            .retain(|(_, answer)| *answer == Reaped::Alive);
        Ok(marks)
    }
}

impl<C> Analysis for Ports<C> {
    fn defer(&self, until: u64) -> Result<(), PortError> {
        self.checked(Method::Defer, &Call::Defer(until))?;
        self.record(Call::Defer(until));
        Ok(())
    }
}

impl<C> Clock for Ports<C> {
    fn now(&self) -> Result<u64, PortError> {
        let now = self.now.get();
        self.now.set(now.saturating_add(self.step.get()));
        Ok(now)
    }

    fn day(&self, at: u64) -> Result<u32, PortError> {
        i64::try_from(at)
            .ok()
            .and_then(|at| at.checked_add(self.zone.get()))
            .and_then(|local| u32::try_from(local.div_euclid(DAY_MS)).ok())
            .ok_or(PortError::NoService)
    }
}

/// The zone, a whole number of days east of UTC, in which the day of `now` is `today`.
fn zone_of(now: u64, today: u32) -> i64 {
    let now = i64::try_from(now).unwrap_or(i64::MAX);
    (i64::from(today) - now.div_euclid(DAY_MS)).saturating_mul(DAY_MS)
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
