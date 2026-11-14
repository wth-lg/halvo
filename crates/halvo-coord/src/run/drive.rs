use super::check::raise_checks;
use super::{delete_step, run_step, Ending, Next, Phase, RunMove, StopCause};
use crate::guard::Table;
use crate::ports::{Analysis, Clock, Owner, PortError, Reaped, Rng, Runs};
use crate::schedule::{self, stop_run, Board, Move, Report, Stop};
use crate::types::{
    Agent, AgentId, Answer, BindResult, Body, Cause, ConfigView, Decision, Doing, Event,
    HandbackResult, Limits, Live, Outcome, Reply, RunEnd, RunId, RunItem, RunSpec, Workflow,
    WorkflowId,
};
use std::cell::RefCell;

/// Who stops a live run with the move `stop` of the run drive: the person's stop control,
/// which gives the cause `cancelled`, a lapse of the run's item with `wait_limit`, which gives
/// `lapsed` (I5.6, I5.7), or the coordinator's answer to a request that ends the run with
/// `kind` or `budget` (I3.1, I3.3, ruling IG-32). The switch-off control and a workflow that is
/// gone stop a run through `Act::SwitchOff` and `Act::Tick`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopBy {
    /// The person's stop control.
    Person,
    /// A lapse of the run's item with `wait_limit`.
    WaitLimit,
    /// The coordinator's answer to a request of the run that ends it with `kind`: a request that
    /// A-A6.2 does not list, a class outside the six, a request while the run waits, or a confirm
    /// request for an effect that has begun or whose newest released decision is `reject_once`
    /// (I3.1, ruling IG-32).
    Kind,
    /// A request after the run raised `items_per_run` items, which ends it with `budget` (I3.3).
    Budget,
}

impl StopBy {
    /// The cause of the stop.
    pub const fn cause(self) -> StopCause {
        match self {
            StopBy::Person => StopCause::Cancelled,
            StopBy::WaitLimit => StopCause::Lapsed,
            StopBy::Kind => StopCause::Kind,
            StopBy::Budget => StopCause::Budget,
        }
    }
}

/// A move that the run drive carries out on the ports, the board of the schedule and the
/// guard's table of this life.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Act {
    /// One tick: the schedule's tick of I5.9 with the parts of the run around it.
    Tick,
    /// A start by the person (I5.5).
    Start {
        /// The agent.
        agent: AgentId,
    },
    /// The switch-off control (I5.4, I5.7).
    SwitchOff {
        /// The agent.
        agent: AgentId,
    },
    /// The runner's first frame.
    Hello {
        /// The run.
        run: RunId,
    },
    /// An item of the run opened.
    Wait {
        /// The run.
        run: RunId,
        /// The item.
        item: RunItem,
    },
    /// The run's item was closed: a confirmation at its release or at a lapse that stops no
    /// run, an item of another kind at its answer or its lapse.
    Resume {
        /// The run.
        run: RunId,
    },
    /// A stop of a live run that is not `stopping`.
    Stop {
        /// The run.
        run: RunId,
        /// Who stops it.
        by: StopBy,
    },
    /// The runner's end, with the outcome of its last frame, or none when the runner was gone
    /// with no last frame.
    End {
        /// The run.
        run: RunId,
        /// The outcome, or none.
        outcome: Option<Outcome>,
    },
    /// The first step of the app's quit (I5.7, ruling RD-8): every live run that no stop has
    /// reached is stopped with `cancelled` and gets `Runs::cancel` once, a `waiting` run among
    /// them, whose item the caller has lapsed with `app_ended` (I4.5), and the queue is emptied,
    /// so that no start leaves it.
    StopAll,
    /// The second step of the app's quit (I5.7, ruling RD-8): `Runs::kill` for every live run
    /// and every dying tree, in the order of their starts, through the board; a run whose kill
    /// succeeded leaves the board, and nothing is stored or sent.
    KillAll,
    /// A tick after the first step of the app's quit (ruling RD-34 (d)): the stages of I5.9 that
    /// start nothing. `Runs::kill` for each stopping run and each dying tree once
    /// `cancel_grace_s` have passed since that run's own stop, or where the clock reads earlier
    /// than the stop (ruling RD-7), with nothing stored at the kill, as at the second step, so
    /// that the next life's recovery ends those runs `interrupted`; then what the board owes. It
    /// serves no queue, starts nothing and raises nothing.
    QuitTick,
}

/// What a move of the run drive did: the schedule's report of the move, and the runs that a
/// tick ended `interrupted` because the library refused their kill with `unknown_run`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Done {
    /// The report of the move, in the keys of a trace line.
    pub report: Report,
    /// The runs that a tick ended `interrupted` after a kill refused with `unknown_run`.
    pub lost: Vec<RunId>,
    /// The workflows that a tick saw gone for the first time: the coordinator drops the items
    /// of their runs with nothing stored and empties their agents' ledgers with `forgotten`, so
    /// that an event that waits there is not sent again (I5.7).
    pub gone: Vec<WorkflowId>,
    /// The agents for which the move raised the item `check` (ruling XB-18).
    pub checked: Vec<AgentId>,
    /// The runs and dying trees that the second step of the quit killed.
    pub killed: Vec<RunId>,
}

fn refused_with(error: &PortError, name: &str) -> bool {
    matches!(error, PortError::Refused(n) if n.as_str() == name)
}

fn answer(cause: Option<Cause>) -> Done {
    Done {
        report: Report {
            answer: cause.map_or(Ok(schedule::Answer::Ok), Err),
            ..Report::default()
        },
        lost: Vec::new(),
        gone: Vec::new(),
        checked: Vec::new(),
        killed: Vec::new(),
    }
}

fn of_report(report: Report) -> Done {
    Done {
        report,
        lost: Vec::new(),
        gone: Vec::new(),
        checked: Vec::new(),
        killed: Vec::new(),
    }
}

/// The live run `run` of the board with the index of its member and its phase.
fn live<C>(board: &Board<C>, run: &RunId) -> Option<(usize, Phase)> {
    let r = board.runs.iter().find(|r| &r.run == run)?;
    let i = board
        .members
        .iter()
        .position(|m| m.facts.agent == r.agent)?;
    match &board.members[i].facts.doing {
        Doing::Live(l) => Some((i, Phase::of(l, r.stop))),
        Doing::Idle(_) | Doing::Queued(_) => None,
    }
}

fn phase_of(phase: &Phase) -> Live {
    match phase {
        Phase::Starting => Live::Starting,
        Phase::Working => Live::Working,
        Phase::Waiting(item) => Live::Waiting(item.clone()),
        Phase::Stopping(_) => Live::Stopping,
    }
}

/// The ports as the schedule's tick and switch-off see them, with the runs whose kill the
/// supervisor library refused with `unknown_run` noted, and no `cancel` for a run in `sent`,
/// whose cancel the lapse of its item sent when it stopped the run in the guard's table (I5.7).
/// Every other `cancel` marks the run stopped in the guard's table before the port's call, so
/// that a panic in the call leaves the run's effect records refused (ruling RD-34 (e)).
struct Watch<'a, P> {
    ports: &'a P,
    unknown: RefCell<Vec<RunId>>,
    listed: RefCell<Option<Vec<WorkflowId>>>,
    sent: Vec<RunId>,
    table: RefCell<&'a mut Table>,
}

impl<P: Owner> Owner for Watch<'_, P> {
    type Config = P::Config;

    fn log(&self, events: &[Event]) -> Result<(), PortError> {
        self.ports.log(events)
    }

    fn history(&self, agent: &AgentId, after: u32) -> Result<Vec<Event>, PortError> {
        self.ports.history(agent, after)
    }

    fn workflows(&self) -> Result<Vec<Workflow>, PortError> {
        let list = self.ports.workflows();
        if let Ok(l) = &list {
            *self.listed.borrow_mut() = Some(l.iter().map(|w| w.id().clone()).collect());
        }
        list
    }

    fn agents(&self) -> Result<Vec<Agent<P::Config>>, PortError> {
        self.ports.agents()
    }

    fn send(&self, body: &Body) -> Result<Reply, PortError> {
        self.ports.send(body)
    }
}

impl<P: Runs> Runs for Watch<'_, P> {
    fn start(&self, spec: &RunSpec) -> Result<(), PortError> {
        self.ports.start(spec)
    }

    fn answer_confirm(
        &self,
        run: &RunId,
        answers: &[Answer],
        record: &mut dyn FnMut(&Decision) -> Result<(), PortError>,
    ) -> Result<(), PortError> {
        self.ports.answer_confirm(run, answers, record)
    }

    fn answer_handback(&self, run: &RunId, result: HandbackResult) -> Result<(), PortError> {
        self.ports.answer_handback(run, result)
    }

    fn answer_bind(&self, run: &RunId, result: BindResult) -> Result<(), PortError> {
        self.ports.answer_bind(run, result)
    }

    fn cancel(&self, run: &RunId) -> Result<(), PortError> {
        if self.sent.contains(run) {
            return Ok(());
        }
        {
            let mut table = self.table.borrow_mut();
            **table = table.stop(run);
        }
        self.ports.cancel(run)
    }

    fn kill(&self, run: &RunId) -> Result<(), PortError> {
        let killed = self.ports.kill(run);
        if let Err(e) = &killed {
            if refused_with(e, "unknown_run") {
                self.unknown.borrow_mut().push(run.clone());
            }
        }
        killed
    }

    fn reap(&self) -> Result<Vec<(RunId, Reaped)>, PortError> {
        self.ports.reap()
    }
}

impl<P: Analysis> Analysis for Watch<'_, P> {
    fn defer(&self, until: u64) -> Result<(), PortError> {
        self.ports.defer(until)
    }
}

impl<P: Clock> Clock for Watch<'_, P> {
    fn now(&self) -> Result<u64, PortError> {
        self.ports.now()
    }

    fn today(&self) -> Result<u32, PortError> {
        self.ports.today()
    }

    fn day(&self, at: u64) -> Result<u32, PortError> {
        self.ports.day(at)
    }
}

impl<P: Rng> Rng for Watch<'_, P> {
    fn bytes16(&self) -> Result<[u8; 16], PortError> {
        self.ports.bytes16()
    }
}

/// Ends the `stopping` run whose kill the library refused with `unknown_run`, in the tick of
/// that kill and with the cause `interrupted` (I5.7, I5.8): its `run_ended` is stored, no
/// `end_run` is sent for it, and its end takes one start from the queue.
fn lose<P>(ports: &P, board: &mut Board<P::Config>, limits: &Limits, run: &RunId) -> Option<Report>
where
    P: Owner + Runs + Analysis + Clock + Rng,
    P::Config: ConfigView + Clone,
{
    board.dying.retain(|d| &d.run != run);
    let (_, phase) = live(board, run)?;
    if run_step(Some(&phase), RunMove::End(Ending::UnknownRun))
        != Ok(Next::Ended(RunEnd::Interrupted))
    {
        return None;
    }
    let r = board.runs.iter_mut().find(|r| &r.run == run)?;
    let at = r.stop.map_or(0, |s| s.at);
    r.stop = Some(Stop {
        at,
        cause: RunEnd::Interrupted,
    });
    let report = schedule::drive(
        ports,
        board,
        limits,
        Move::End {
            run: run.clone(),
            outcome: None,
        },
    );
    board.dying.retain(|d| &d.run != run);
    Some(report)
}

fn tick<P>(ports: &P, board: &mut Board<P::Config>, table: &mut Table, limits: &Limits) -> Done
where
    P: Owner + Runs + Analysis + Clock + Rng,
    P::Config: ConfigView + Clone,
{
    let watch = Watch {
        ports,
        unknown: RefCell::new(Vec::new()),
        listed: RefCell::new(None),
        sent: Vec::new(),
        table: RefCell::new(table),
    };
    let before = board.clone();
    let mut report = schedule::drive(&watch, board, limits, Move::Tick);
    let Watch {
        unknown,
        listed,
        table,
        ..
    } = watch;
    let table = table.into_inner();
    let gone = listed
        .into_inner()
        .as_ref()
        .map(|listed| delete_step(&before, listed));
    for run in gone.iter().flat_map(|g| g.stop.iter()) {
        *table = table.stop(run);
    }
    let mut lost = Vec::new();
    for run in unknown.into_inner() {
        if let Some(end) = lose(ports, board, limits, &run) {
            report.ended.extend(end.ended);
            lost.push(run);
        }
    }
    Done {
        report,
        lost,
        gone: gone.map(|g| g.seen).unwrap_or_default(),
        checked: raise_checks(ports, board, None),
        killed: Vec::new(),
    }
}

/// Carries out a move of a run on the ports, the board and the guard's table (I5.6, I5.7,
/// I5.10). `hello`, `wait` and `resume` move the phase of the run's member by `run_step` and
/// call no port; any other pair, and a run that is not live, is `state`. A stop by the person
/// or by `wait_limit` of a `waiting` run whose item has not lapsed is `state` and calls no port:
/// the lapse of an item of every kind that stops its run stops it in the guard's table first,
/// which is the fact that this reads (ruling RD-6); a stop with `kind` or `budget` asks no such
/// fact, because the caller lapses the run's item with `run_ended` before it. A stop of a live
/// run that is not `stopping` stops the run in the guard's table and makes it `stopping` with
/// the cause of the stop, from the clock's time on, so that the first tick at which
/// `cancel_grace_s` have passed kills it, and then sends `Runs::cancel`, unless the table held
/// the run as stopped already, where the lapse of its item that stopped it there sent the
/// cancel (I4.5): the mark comes before the port's call, so that a panic in the call leaves the
/// run's effect records refused and its kill due (ruling RD-34 (e)). The runner's end of a `waiting` run is `state` until its item is closed,
/// so that its item is closed before the end sets the member idle; any other end is the
/// schedule's end of the run. A start by the person is the schedule's start. The switch-off
/// control is the schedule's, and each run that it stops is stopped in the guard's table as
/// well; it sends no `cancel` to a run that the table holds as stopped, whose lapse sent it. A tick is the schedule's tick, which kills each run whose stop came `cancel_grace_s`
/// before it, again at each tick while the kill fails; each run that the tick stops for a
/// workflow that is gone is stopped in the guard's table; and a `stopping` run whose kill the
/// library refused with `unknown_run` ends `interrupted` in that tick, with no `end_run`
/// sent for it, so that its agent takes no start until the agent store has ended it. The
/// first step of the app's quit stops every live run that no stop has reached, as the person's
/// stop does, and empties the queue; the second kills every live run and every dying tree and
/// stores no `run_ended`, so that recovery ends those runs `interrupted` in the next life
/// (ruling RD-8).
pub fn drive<P>(
    ports: &P,
    board: &mut Board<P::Config>,
    table: &mut Table,
    limits: &Limits,
    act: Act,
) -> Done
where
    P: Owner + Runs + Analysis + Clock + Rng,
    P::Config: ConfigView + Clone,
{
    match act {
        Act::Tick => tick(ports, board, table, limits),
        Act::Start { agent } => {
            let mut done = of_report(schedule::drive(
                ports,
                board,
                limits,
                Move::Start {
                    agent: agent.clone(),
                },
            ));
            done.checked = raise_checks(ports, board, Some(&agent));
            done
        }
        Act::SwitchOff { agent } => {
            let sent = board
                .runs
                .iter()
                .filter(|r| r.agent == agent && r.stop.is_none() && table.is_stopped(&r.run))
                .map(|r| r.run.clone())
                .collect();
            let watch = Watch {
                ports,
                unknown: RefCell::new(Vec::new()),
                listed: RefCell::new(None),
                sent,
                table: RefCell::new(table),
            };
            let report = schedule::drive(&watch, board, limits, Move::SwitchOff { agent });
            let table = watch.table.into_inner();
            for run in &report.stopped {
                *table = table.stop(run);
            }
            of_report(report)
        }
        Act::Hello { run } => phase(board, &run, RunMove::Hello),
        Act::Wait { run, item } => phase(board, &run, RunMove::Wait(item)),
        Act::Resume { run } => phase(board, &run, RunMove::Resume),
        Act::Stop { run, by } => stop(ports, board, table, &run, by),
        Act::End { run, outcome } => {
            let Some((_, phase)) = live(board, &run) else {
                return answer(Some(Cause::State));
            };
            if matches!(phase, Phase::Waiting(_))
                || run_step(Some(&phase), RunMove::End(Ending::Frame(outcome))).is_err()
            {
                return answer(Some(Cause::State));
            }
            let mut done = of_report(schedule::drive(
                ports,
                board,
                limits,
                Move::End { run, outcome },
            ));
            done.checked = raise_checks(ports, board, None);
            done
        }
        Act::StopAll => stop_all(ports, board, table),
        Act::KillAll => kill_all(ports, board),
        Act::QuitTick => quit_tick(ports, board, limits),
    }
}

fn stop_all<P>(ports: &P, board: &mut Board<P::Config>, table: &mut Table) -> Done
where
    P: Runs + Clock + Owner,
{
    let mut done = answer(None);
    let runs: Vec<RunId> = board
        .runs
        .iter()
        .filter(|r| r.stop.is_none())
        .map(|r| r.run.clone())
        .collect();
    for run in runs {
        let stopped = stop_as(ports, board, table, &run, StopBy::Person, false);
        done.report.stopped.extend(stopped.report.stopped);
    }
    while let Some(waiting) = board.queue.pop() {
        if let Some(m) = board
            .members
            .iter_mut()
            .find(|m| m.facts.agent == waiting.agent)
        {
            if let Doing::Queued(item) = &m.facts.doing {
                m.facts.doing = Doing::Idle(item.clone());
            }
        }
    }
    done
}

fn kill_all<P, C>(ports: &P, board: &mut Board<C>) -> Done
where
    P: Runs,
{
    let mut trees: Vec<(u64, RunId)> = board
        .runs
        .iter()
        .map(|r| (r.rank, r.run.clone()))
        .chain(board.dying.iter().map(|d| (d.rank, d.run.clone())))
        .collect();
    trees.sort();
    let mut done = answer(None);
    for (_, run) in trees {
        if kill_tree(ports, board, &run) {
            done.killed.push(run);
        }
    }
    done
}

/// `Runs::kill` for the tree of `run`, which leaves the board when the kill succeeded, with
/// nothing stored or sent: the quit's kill.
fn kill_tree<P, C>(ports: &P, board: &mut Board<C>, run: &RunId) -> bool
where
    P: Runs,
{
    if ports.kill(run).is_err() {
        return false;
    }
    board.dying.retain(|d| &d.run != run);
    if let Some(k) = board.runs.iter().position(|r| &r.run == run) {
        let live = board.runs.remove(k);
        if let Some(m) = board
            .members
            .iter_mut()
            .find(|m| m.facts.agent == live.agent)
        {
            m.facts.doing = Doing::Idle(None);
        }
    }
    true
}

fn quit_tick<P>(ports: &P, board: &mut Board<P::Config>, limits: &Limits) -> Done
where
    P: Owner + Runs + Analysis + Clock + Rng,
    P::Config: ConfigView + Clone,
{
    let now = match ports.now() {
        Ok(now) => now,
        Err(PortError::Busy) => return answer(Some(Cause::Busy)),
        Err(_) => return answer(Some(Cause::NoService)),
    };
    let grace = limits.cancel_grace_s.saturating_mul(1000);
    let due = |at: u64| now < at || now - at >= grace;
    let mut trees: Vec<(u64, RunId)> = board
        .runs
        .iter()
        .filter(|r| r.stop.is_some_and(|s| due(s.at)))
        .map(|r| (r.rank, r.run.clone()))
        .chain(
            board
                .dying
                .iter()
                .filter(|d| due(d.stopped))
                .map(|d| (d.rank, d.run.clone())),
        )
        .collect();
    trees.sort();
    let mut done = answer(None);
    for (_, run) in trees {
        if kill_tree(ports, board, &run) {
            done.killed.push(run);
        }
    }
    schedule::drive(ports, board, limits, Move::Owed);
    done
}

fn phase<C>(board: &mut Board<C>, run: &RunId, mv: RunMove) -> Done {
    let Some((i, now)) = live(board, run) else {
        return answer(Some(Cause::State));
    };
    match run_step(Some(&now), mv) {
        Ok(Next::Live(next)) => {
            board.members[i].facts.doing = Doing::Live(phase_of(&next));
            answer(None)
        }
        Ok(Next::Ended(_)) | Err(_) => answer(Some(Cause::State)),
    }
}

fn stop<P>(
    ports: &P,
    board: &mut Board<P::Config>,
    table: &mut Table,
    run: &RunId,
    by: StopBy,
) -> Done
where
    P: Runs + Clock + Owner,
{
    let lapsed_first = matches!(by, StopBy::Person | StopBy::WaitLimit);
    stop_as(ports, board, table, run, by, lapsed_first)
}

/// The stop of a live run; `lapsed_first` makes a `waiting` run whose item has not lapsed in
/// the guard's table `state`, which the quit's stop-all does not ask, because the quit lapses
/// every open item with `app_ended` first (I4.5).
fn stop_as<P>(
    ports: &P,
    board: &mut Board<P::Config>,
    table: &mut Table,
    run: &RunId,
    by: StopBy,
    lapsed_first: bool,
) -> Done
where
    P: Runs + Clock + Owner,
{
    let now = match ports.now() {
        Ok(now) => now,
        Err(PortError::Busy) => return answer(Some(Cause::Busy)),
        Err(_) => return answer(Some(Cause::NoService)),
    };
    let Some((_, phase)) = live(board, run) else {
        return answer(Some(Cause::State));
    };
    if run_step(Some(&phase), RunMove::Stop(by.cause())).is_err() {
        return answer(Some(Cause::State));
    }
    if lapsed_first && matches!(phase, Phase::Waiting(_)) && !table.is_stopped(run) {
        return answer(Some(Cause::State));
    }
    let send = !table.is_stopped(run);
    *table = table.stop(run);
    stop_run(board, run, now, by.cause().end());
    if send {
        let _ = ports.cancel(run);
    }
    let mut done = answer(None);
    done.report.stopped.push(run.clone());
    done
}
