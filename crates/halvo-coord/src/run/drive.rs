use super::{run_step, Ending, Next, Phase, RunMove, StopCause};
use crate::guard::Table;
use crate::ports::{Analysis, Clock, Owner, PortError, Reaped, Rng, Runs};
use crate::schedule::{self, stop_run, Board, Move, Owing, Report, Stop};
use crate::types::{
    Agent, AgentId, Answer, Body, Cause, ConfigView, Decision, Doing, Event, HandbackResult,
    Limits, Live, Outcome, Reply, RunEnd, RunId, RunItem, RunSpec, Workflow,
};
use std::cell::RefCell;

/// Who stops a live run with the move `stop` of the run drive: the person's stop control,
/// which gives the cause `cancelled`, or a lapse of the run's item with `wait_limit`, which
/// gives `lapsed` (I5.6, I5.7). The switch-off control and a workflow that is gone stop a
/// run through `Act::SwitchOff` and `Act::Tick`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopBy {
    /// The person's stop control.
    Person,
    /// A lapse of the run's item with `wait_limit`.
    WaitLimit,
}

impl StopBy {
    /// The cause of the stop.
    pub const fn cause(self) -> StopCause {
        match self {
            StopBy::Person => StopCause::Cancelled,
            StopBy::WaitLimit => StopCause::Lapsed,
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
}

/// What a move of the run drive did: the schedule's report of the move, and the runs that a
/// tick ended `interrupted` because the library refused their kill with `unknown_run`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Done {
    /// The report of the move, in the keys of a trace line.
    pub report: Report,
    /// The runs that a tick ended `interrupted` after a kill refused with `unknown_run`.
    pub lost: Vec<RunId>,
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
    }
}

fn of_report(report: Report) -> Done {
    Done {
        report,
        lost: Vec::new(),
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

/// The ports as the schedule's tick sees them, with the runs whose kill the supervisor
/// library refused with `unknown_run` noted.
struct Watch<'a, P> {
    ports: &'a P,
    unknown: RefCell<Vec<RunId>>,
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
        self.ports.workflows()
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

    fn cancel(&self, run: &RunId) -> Result<(), PortError> {
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
    if let Ok(agents) = ports.agents() {
        board.owed.retain(|o| {
            o.what != Owing::EndRun
                || !agents
                    .iter()
                    .any(|a| a.agent == o.agent && a.last.as_ref().is_some_and(|l| l.run == o.run))
        });
    }
    let watch = Watch {
        ports,
        unknown: RefCell::new(Vec::new()),
    };
    let mut report = schedule::drive(&watch, board, limits, Move::Tick);
    for run in &report.stopped {
        *table = table.stop(run);
    }
    let mut lost = Vec::new();
    for run in watch.unknown.into_inner() {
        if let Some(end) = lose(ports, board, limits, &run) {
            report.ended.extend(end.ended);
            lost.push(run);
        }
    }
    Done { report, lost }
}

/// Carries out a move of a run on the ports, the board and the guard's table (I5.6, I5.7,
/// I5.10). `hello`, `wait` and `resume` move the phase of the run's member by `run_step` and
/// call no port; any other pair, and a run that is not live, is `state`. A stop of a live run
/// that is not `stopping` sends `Runs::cancel` and stops the run in the guard's table, unless
/// the table holds the run as stopped already, where the lapse of its item that stopped it
/// there sent the cancel (I4.5), and then makes the run `stopping` with the cause of the
/// stop, from the clock's time on, so that the first tick at which `cancel_grace_s` have
/// passed kills it. The runner's end of a `waiting` run is `state` until its item is closed,
/// so that its item is closed before the end sets the member idle; any other end is the
/// schedule's end of the run. A start by the person is the schedule's start. The switch-off
/// control is the schedule's, and each run that it stops is stopped in the guard's table as
/// well. A tick is the schedule's tick, which kills each run whose stop came `cancel_grace_s`
/// before it, again at each tick while the kill fails; each run that the tick stops for a
/// workflow that is gone is stopped in the guard's table; and a `stopping` run whose kill the
/// library refused with `unknown_run` ends `interrupted` in that tick, with no `end_run`
/// sent for it, so that its agent takes no start until the agent store has ended it.
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
            of_report(schedule::drive(ports, board, limits, Move::Start { agent }))
        }
        Act::SwitchOff { agent } => {
            let report = schedule::drive(ports, board, limits, Move::SwitchOff { agent });
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
            of_report(schedule::drive(
                ports,
                board,
                limits,
                Move::End { run, outcome },
            ))
        }
    }
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
    let now = match ports.now() {
        Ok(now) => now,
        Err(PortError::Busy) => return answer(Some(Cause::Busy)),
        Err(_) => return answer(Some(Cause::NoService)),
    };
    let Some((_, phase)) = live(board, run) else {
        return answer(Some(Cause::State));
    };
    if !matches!(
        run_step(Some(&phase), RunMove::Stop(by.cause())),
        Ok(Next::Live(Phase::Stopping(_)))
    ) || matches!(phase, Phase::Stopping(_))
    {
        return answer(Some(Cause::State));
    }
    if !table.is_stopped(run) {
        let _ = ports.cancel(run);
        *table = table.stop(run);
    }
    stop_run(board, run, now, by.cause().end());
    let mut done = answer(None);
    done.report.stopped.push(run.clone());
    done
}
