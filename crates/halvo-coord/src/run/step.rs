use crate::schedule::Stop;
use crate::types::{Cause, Live, Outcome, RunEnd, RunItem};

/// The cause of a stop (I5.6, I5.7): the person's stop and the switch-off control give
/// `cancelled`, a lapse with `wait_limit` gives `lapsed`, and a workflow that is gone gives
/// `deleted`; a request that the coordinator answers with the end of its run gives `kind` (I3.1,
/// ruling IG-32), and one beyond `items_per_run` gives `budget` (I3.3). A stopped run ends with
/// the cause of its stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopCause {
    /// `cancelled`.
    Cancelled,
    /// `lapsed`.
    Lapsed,
    /// `deleted`.
    Deleted,
    /// `kind`.
    Kind,
    /// `budget`.
    Budget,
}

impl StopCause {
    /// The end of a run with this stop.
    pub const fn end(self) -> RunEnd {
        match self {
            StopCause::Cancelled => RunEnd::Cancelled,
            StopCause::Lapsed => RunEnd::Lapsed,
            StopCause::Deleted => RunEnd::Deleted,
            StopCause::Kind => RunEnd::Kind,
            StopCause::Budget => RunEnd::Budget,
        }
    }

    /// The stop that the stored cause `end` of a stop names, the coordinator's own ends `kind`
    /// and `budget` among them, or none for an end that no stop has; `Phase::of` reads it, so
    /// that `run_step`'s end and the stored cause are one (ruling RD-9).
    pub const fn of_stop(end: RunEnd) -> Option<StopCause> {
        match end {
            RunEnd::Kind => Some(StopCause::Kind),
            RunEnd::Budget => Some(StopCause::Budget),
            _ => StopCause::of(end),
        }
    }

    /// The stop of the end `end`, or none for an end that is no stop.
    pub const fn of(end: RunEnd) -> Option<StopCause> {
        match end {
            RunEnd::Cancelled => Some(StopCause::Cancelled),
            RunEnd::Lapsed => Some(StopCause::Lapsed),
            RunEnd::Deleted => Some(StopCause::Deleted),
            _ => None,
        }
    }
}

/// A live run as `run_step` reads it (I5.6): its phase, with the item of a `waiting` run and
/// the cause of the stop of a `stopping` one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    /// `starting`.
    Starting,
    /// `working`.
    Working,
    /// `waiting`, with the item that the run raised.
    Waiting(RunItem),
    /// `stopping`, with the cause of its stop.
    Stopping(StopCause),
}

impl Phase {
    /// The phase of a live run of the board: the member's `Live` and the run's stop; a
    /// `stopping` run whose stop holds no cause of a stop is `cancelled`.
    pub fn of(live: &Live, stop: Option<Stop>) -> Phase {
        match live {
            Live::Starting => Phase::Starting,
            Live::Working => Phase::Working,
            Live::Waiting(item) => Phase::Waiting(item.clone()),
            Live::Stopping => Phase::Stopping(
                stop.and_then(|s| StopCause::of_stop(s.cause))
                    .unwrap_or(StopCause::Cancelled),
            ),
        }
    }
}

/// How a live run ends (I5.6): the runner's last frame with its outcome, or none when the
/// runner was gone with no last frame; the kill of a stopped run; a restart of the app; or a
/// kill that the supervisor library refused with `unknown_run` (I5.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ending {
    /// The runner's end, with the outcome of its last frame or none.
    Frame(Option<Outcome>),
    /// The kill of a stopped run.
    Kill,
    /// A restart of the app that finds the run live.
    Restart,
    /// A kill that the library refused with `unknown_run`.
    UnknownRun,
}

/// A move of a run (I5.6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunMove {
    /// Step 8 of I5.5 started the runner.
    Spawn,
    /// The runner's first frame.
    Hello,
    /// An item of the run opened.
    Wait(RunItem),
    /// The run's item was closed.
    Resume,
    /// A stop.
    Stop(StopCause),
    /// The end.
    End(Ending),
}

/// What a move makes of a run: a live run in a phase, or an ended run with the cause of
/// I5.6.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Next {
    /// The run is live in this phase.
    Live(Phase),
    /// The run ended with this cause.
    Ended(RunEnd),
}

/// The moves of a run (I5.6), from no run or a live run in a phase; every other pair is
/// refused with `state`. `spawn` makes a `starting` run, `hello` a `working` one from
/// `starting`, `wait` a `waiting` one from `working` and `resume` a `working` one from
/// `waiting`; a stop makes a run of the three first phases `stopping` with its cause, and a
/// `stopping` run takes the cause `deleted` when its workflow goes and no other stop. An end
/// gives a `stopping` run the cause of its stop at the runner's end or its kill, and
/// `interrupted` at a restart or at a kill refused with `unknown_run`; it gives any other live
/// run the outcome of its last frame, and `interrupted` with no frame or at a restart; a kill
/// ends only a stopped run.
pub fn run_step(run: Option<&Phase>, mv: RunMove) -> Result<Next, Cause> {
    use Phase::{Starting, Stopping, Waiting, Working};
    let next = match (run, mv) {
        (None, RunMove::Spawn) => Next::Live(Starting),
        (Some(Starting), RunMove::Hello) => Next::Live(Working),
        (Some(Working), RunMove::Wait(item)) => Next::Live(Waiting(item)),
        (Some(Waiting(_)), RunMove::Resume) => Next::Live(Working),
        (Some(Starting | Working | Waiting(_)), RunMove::Stop(cause)) => {
            Next::Live(Stopping(cause))
        }
        (Some(Stopping(_)), RunMove::Stop(StopCause::Deleted)) => {
            Next::Live(Stopping(StopCause::Deleted))
        }
        (Some(Stopping(cause)), RunMove::End(Ending::Frame(_) | Ending::Kill)) => {
            Next::Ended(cause.end())
        }
        (Some(Stopping(_)), RunMove::End(Ending::Restart | Ending::UnknownRun)) => {
            Next::Ended(RunEnd::Interrupted)
        }
        (Some(Starting | Working | Waiting(_)), RunMove::End(Ending::Frame(Some(outcome)))) => {
            Next::Ended(RunEnd::from(outcome))
        }
        (
            Some(Starting | Working | Waiting(_)),
            RunMove::End(Ending::Frame(None) | Ending::Restart),
        ) => Next::Ended(RunEnd::Interrupted),
        _ => return Err(Cause::State),
    };
    Ok(next)
}
