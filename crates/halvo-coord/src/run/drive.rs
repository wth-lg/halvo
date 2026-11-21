use super::check::raise_checks;
use super::{delete_step, run_step, Ending, Next, Phase, RunMove, StopCause};
use crate::guard::Table;
use crate::ports::{Analysis, Clock, Owner, PortError, Reaped, Rng, Runs};
use crate::schedule::{self, stop_run, Board, Move, Report, Stop};
use crate::types::{
    Agent, AgentId, Answer, BindResult, Body, Cause, ConfigView, Decision, Doing, Event,
    HandbackResult, Limits, Live, Outcome, Reply, ReplyRow, RunEnd, RunId, RunItem, RunSpec,
    Workflow, WorkflowId,
};
use std::cell::RefCell;

/// Who stops a live run with the move `stop` of the run drive: the person's stop control,
/// which gives the cause `cancelled`, a lapse of the run's item with `wait_limit`, which gives
/// `lapsed` (I5.6, I5.7), or the coordinator's answer to a request that ends the run with
/// `kind` or `budget` (I3.1, I3.3, ruling IG-32), or the settle of a confirmation that an
/// answer's move left `answered`, which gives `answer_lost` (ruling RD-84). The switch-off
/// control and a workflow that is
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
    /// The settle of a confirmation that an answer's move left `answered` (ruling RD-84).
    AnswerLost,
}

impl StopBy {
    /// The cause of the stop.
    pub const fn cause(self) -> StopCause {
        match self {
            StopBy::Person => StopCause::Cancelled,
            StopBy::WaitLimit => StopCause::Lapsed,
            StopBy::Kind => StopCause::Kind,
            StopBy::Budget => StopCause::Budget,
            StopBy::AnswerLost => StopCause::AnswerLost,
        }
    }
}

/// A stop that no read of the clock answered (ruling RD-133 1): the run is stopped on the guard's
/// table and the board at once, with the time 0 on the board, which is the record that the
/// coordinator stamps at the next read that answers. It sends nothing: the coordinator writes the
/// rest of the record and then sends the cancel (`send_cancel`), or the lapse of the run's open
/// item sends it (ruling RD-138 (1)). Whether the board took a new stop.
pub fn stop_unstamped<C>(board: &mut Board<C>, table: &mut Table, run: &RunId, by: StopBy) -> bool {
    *table = table.stop(run);
    if board.runs.iter().any(|r| &r.run == run && r.stop.is_some()) {
        return false;
    }
    stop_run(board, run, 0, by.cause().end());
    true
}

/// Sends `Runs::cancel` for a run whose stop is already on the guard's table and the board, and
/// whose record (its board stop at time 0 where no read of the clock answered, the switch-off's
/// list and `hold_set`) the coordinator wrote first, so that a cancel that does not return loses
/// none of it (ruling RD-138 (1)).
pub fn send_cancel<P: Runs>(ports: &P, run: &RunId) {
    let _ = ports.cancel(run);
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
/// supervisor library refused with `unknown_run` noted, and no `cancel` for a run in
/// `cancel_already_sent`, whose cancel the lapse of its item sent when it stopped the run in the
/// guard's table (I5.7). Every other `cancel` marks the run stopped in the guard's table before
/// the port's call, so that a panic in the call leaves the run's effect records refused (ruling
/// RD-34 (e)).
struct Watch<'a, P> {
    ports: &'a P,
    kill_refused_unknown_run: RefCell<Vec<RunId>>,
    listed: RefCell<Option<Vec<WorkflowId>>>,
    cancel_already_sent: Vec<RunId>,
    table: RefCell<&'a mut Table>,
}

impl<P: Runs> Watch<'_, P> {
    fn mark_stopped_then_cancel(&self, run: &RunId) -> Result<(), PortError> {
        {
            let mut table = self.table.borrow_mut();
            **table = table.stop(run);
        }
        self.ports.cancel(run)
    }
}

impl<P: Owner> Owner for Watch<'_, P> {
    type Config = P::Config;

    fn log(&self, events: &[Event]) -> Result<(), PortError> {
        self.ports.log(events)
    }

    fn history(&self, agent: &AgentId, after: u32) -> Result<(Vec<Event>, u32), PortError> {
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

    fn replies(
        &self,
        workflow: &WorkflowId,
        agent: &AgentId,
        run: &RunId,
    ) -> Result<Vec<ReplyRow>, PortError> {
        self.ports.replies(workflow, agent, run)
    }

    fn overlaps(
        &self,
        workflow: &WorkflowId,
        agent: &AgentId,
        slots: Option<&[crate::types::Slot]>,
    ) -> Result<crate::ports::Overlaps, PortError> {
        self.ports.overlaps(workflow, agent, slots)
    }

    fn delete(&self, workflow: &WorkflowId) -> Result<(), PortError> {
        self.ports.delete(workflow)
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
        if self.cancel_already_sent.contains(run) {
            return Ok(());
        }
        self.mark_stopped_then_cancel(run)
    }

    fn kill(&self, run: &RunId) -> Result<(), PortError> {
        let killed = self.ports.kill(run);
        if let Err(e) = &killed {
            if refused_with(e, "unknown_run") {
                self.kill_refused_unknown_run.borrow_mut().push(run.clone());
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
fn end_interrupted_after_unknown_run_kill<P>(
    ports: &P,
    board: &mut Board<P::Config>,
    limits: &Limits,
    run: &RunId,
) -> Option<Report>
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
        kill_refused_unknown_run: RefCell::new(Vec::new()),
        listed: RefCell::new(None),
        cancel_already_sent: Vec::new(),
        table: RefCell::new(table),
    };
    let before = board.clone();
    let mut report = schedule::drive(&watch, board, limits, Move::Tick);
    let Watch {
        kill_refused_unknown_run,
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
    for run in kill_refused_unknown_run.into_inner() {
        if let Some(end) = end_interrupted_after_unknown_run_kill(ports, board, limits, &run) {
            report.ended.extend(end.ended);
            lost.push(run);
        }
    }
    let checked = checks_of_the_move(ports, board, limits, None, &report);
    Done {
        report,
        lost,
        gone: gone.map(|g| g.seen).unwrap_or_default(),
        checked,
        killed: Vec::new(),
    }
}

/// The checks that an act raises after its move of the schedule, each stamped by that move's one
/// read of the clock, which its report carries; none where no read of the clock answered (ruling
/// RD-77 part 6).
fn checks_of_the_move<P>(
    ports: &P,
    board: &mut Board<P::Config>,
    limits: &Limits,
    only: Option<&AgentId>,
    report: &Report,
) -> Vec<AgentId>
where
    P: Owner + Rng,
    P::Config: ConfigView,
{
    report.stamp.map_or_else(Vec::new, |stamp| {
        raise_checks(ports, board, limits, only, stamp)
    })
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
            done.checked = checks_of_the_move(ports, board, limits, Some(&agent), &done.report);
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
                kill_refused_unknown_run: RefCell::new(Vec::new()),
                listed: RefCell::new(None),
                cancel_already_sent: sent,
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
            done.checked = checks_of_the_move(ports, board, limits, None, &done.report);
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
        let stopped = stop_as(
            ports,
            board,
            table,
            &run,
            StopBy::Person,
            ItemLapse::LapsedByQuit,
        );
        done.report.stopped.extend(stopped.report.stopped);
        if stopped.report.read.is_some() {
            done.report.read = stopped.report.read;
        }
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
        if quit_kill_tree(ports, board, &run) {
            done.killed.push(run);
        }
    }
    done
}

/// `Runs::kill` for the tree of `run`, which leaves the board when the kill succeeded, with
/// nothing stored or sent: the quit's kill.
fn quit_kill_tree<P, C>(ports: &P, board: &mut Board<C>, run: &RunId) -> bool
where
    P: Runs,
{
    if ports.kill(run).is_err() {
        if let Some(r) = board.runs.iter_mut().find(|r| &r.run == run) {
            r.kill_refused = true;
        }
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
        if quit_kill_tree(ports, board, &run) {
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
    let lapse = match by {
        StopBy::Person | StopBy::WaitLimit | StopBy::AnswerLost => ItemLapse::MustHaveLapsed,
        StopBy::Kind | StopBy::Budget => ItemLapse::NoOpenItem,
    };
    stop_as(ports, board, table, run, by, lapse)
}

/// Where the lapse of a stopped run's open item stands at its stop: a stop by the person or at
/// the wait limit answers `state` for a `waiting` run whose item has not lapsed in the guard's
/// table; the quit lapses every open item with `app_ended` before its stop-all (I4.5); a stop for
/// a request's kind or budget finds the run working, with no open item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ItemLapse {
    MustHaveLapsed,
    LapsedByQuit,
    NoOpenItem,
}

fn stop_as<P>(
    ports: &P,
    board: &mut Board<P::Config>,
    table: &mut Table,
    run: &RunId,
    by: StopBy,
    lapse: ItemLapse,
) -> Done
where
    P: Runs + Clock + Owner,
{
    let now = ports.now().ok();
    let Some((_, phase)) = live(board, run) else {
        return answer(Some(Cause::State));
    };
    if run_step(Some(&phase), RunMove::Stop(by.cause())).is_err() {
        return answer(Some(Cause::State));
    }
    if lapse == ItemLapse::MustHaveLapsed
        && matches!(phase, Phase::Waiting(_))
        && !table.is_stopped(run)
    {
        return answer(Some(Cause::State));
    }
    let send = !table.is_stopped(run);
    *table = table.stop(run);
    stop_run(board, run, now.unwrap_or(0), by.cause().end());
    if send {
        let _ = ports.cancel(run);
    }
    let mut done = answer(None);
    done.report.stopped.push(run.clone());
    done.report.read = now;
    done
}

#[cfg(test)]
mod watch_tests {
    use super::Watch;
    use crate::guard::Table;
    use crate::mem::{Config, Method, Ports};
    use crate::ports::{Owner, PortError};
    use crate::types::{AgentId, WorkflowId};
    use std::cell::RefCell;

    /// The row of the coordinator's message msg_273bd1c3262d (round RD-160): the tick's view of
    /// the ports hands the read `Owner::overlaps` on to the owner, so that a read inside a tick
    /// meets the owner's answer (here `busy`) and not the trait's default `no_service`.
    #[test]
    fn the_ticks_watch_hands_the_read_of_overlaps_on_to_the_owner() {
        let p: Ports<Config> = Ports::new(0, 0);
        p.refuse(Method::Overlaps, PortError::Busy);
        let mut table = Table::new();
        let watch = Watch {
            ports: &p,
            kill_refused_unknown_run: RefCell::new(Vec::new()),
            listed: RefCell::new(None),
            cancel_already_sent: Vec::new(),
            table: RefCell::new(&mut table),
        };
        let workflow = WorkflowId::parse("000000000000000000000000000000e0").unwrap();
        let agent = AgentId::parse("000000000000000000000000000000b0").unwrap();
        assert_eq!(
            watch.overlaps(&workflow, &agent, None),
            Err(PortError::Busy)
        );
        assert_eq!(p.overlap_reads.borrow().clone(), vec![(agent, None)]);
    }
}

#[cfg(test)]
mod watch {
    use super::Watch;
    use crate::guard::Table;
    use crate::mem::Config;
    use crate::ports::{Owner, PortError};
    use crate::types::{Agent, AgentId, Body, Event, Reply, ReplyRow, RunId, Workflow, WorkflowId};
    use std::cell::RefCell;

    #[derive(Default)]
    struct Recording {
        deletes: RefCell<Vec<WorkflowId>>,
    }

    impl Owner for Recording {
        type Config = Config;

        fn log(&self, _: &[Event]) -> Result<(), PortError> {
            Err(PortError::Busy)
        }

        fn history(&self, _: &AgentId, _: u32) -> Result<(Vec<Event>, u32), PortError> {
            Err(PortError::Busy)
        }

        fn workflows(&self) -> Result<Vec<Workflow>, PortError> {
            Err(PortError::Busy)
        }

        fn agents(&self) -> Result<Vec<Agent<Config>>, PortError> {
            Err(PortError::Busy)
        }

        fn send(&self, _: &Body) -> Result<Reply, PortError> {
            Err(PortError::Busy)
        }

        fn replies(
            &self,
            _: &WorkflowId,
            _: &AgentId,
            _: &RunId,
        ) -> Result<Vec<ReplyRow>, PortError> {
            Err(PortError::Busy)
        }

        fn delete(&self, workflow: &WorkflowId) -> Result<(), PortError> {
            self.deletes.borrow_mut().push(workflow.clone());
            Ok(())
        }
    }

    #[test]
    fn a_delete_through_the_watch_reaches_the_owner_that_it_wraps() {
        let owner = Recording::default();
        let mut table = Table::new();
        let watch = Watch {
            ports: &owner,
            kill_refused_unknown_run: RefCell::new(Vec::new()),
            listed: RefCell::new(None),
            cancel_already_sent: Vec::new(),
            table: RefCell::new(&mut table),
        };
        let workflow = WorkflowId::parse(&format!("{:032x}", 0x51)).unwrap();
        assert_eq!(watch.delete(&workflow), Ok(()));
        assert_eq!(*owner.deletes.borrow(), [workflow]);
    }
}
