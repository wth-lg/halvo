use super::{Failure, ItemKind, Status};

/// The status of an agent with the case of its line (I1.3), each with its words key
/// `line.<status key>.<case>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StatusLine {
    /// `needs_you` with the kind of the open item.
    NeedsYou(ItemKind),
    /// `not_yet.category`: the hold `not_yet`.
    NotYet,
    /// `off.person`: the hold `off`.
    Off,
    /// `problem.failures`: the hold `failures`.
    Failures,
    /// `working.starting`: a live run in the phase `starting`.
    Starting,
    /// `working.working`: a live run in the phase `working`.
    Working,
    /// `working.stopping`: a live run in the phase `stopping`.
    Stopping,
    /// `waiting_turn.queued`: a queued start.
    Queued,
    /// `waiting_turn.store`: the agent store has not ended the agent's newest run (ruling
    /// XB-28 (b)).
    Store,
    /// `problem.<cause>`: a last run that ended with a cause of the failure class.
    Problem(Failure),
    /// `all_set.tried_today`: a day with `starts_per_day` starts.
    TriedToday,
    /// `all_set.new`: no run of the agent has ended.
    New,
    /// `all_set.done`: the last run ended `completed`.
    Done,
    /// `all_set.nothing`: the last run ended `nothing_to_do`.
    Nothing,
    /// `all_set.stopped_early`: the last run had any other end.
    StoppedEarly,
}

impl StatusLine {
    /// Every status line, in the order of I1.3.
    pub fn all() -> Vec<StatusLine> {
        let mut all: Vec<StatusLine> = ItemKind::ALL
            .iter()
            .map(|k| StatusLine::NeedsYou(*k))
            .collect();
        all.extend([
            StatusLine::NotYet,
            StatusLine::Off,
            StatusLine::Failures,
            StatusLine::Starting,
            StatusLine::Working,
            StatusLine::Stopping,
            StatusLine::Queued,
            StatusLine::Store,
        ]);
        all.extend(Failure::ALL.iter().map(|f| StatusLine::Problem(*f)));
        all.extend([
            StatusLine::TriedToday,
            StatusLine::New,
            StatusLine::Done,
            StatusLine::Nothing,
            StatusLine::StoppedEarly,
        ]);
        all
    }

    /// The status.
    pub fn status(self) -> Status {
        match self {
            StatusLine::NeedsYou(_) => Status::NeedsYou,
            StatusLine::NotYet => Status::NotYet,
            StatusLine::Off => Status::Off,
            StatusLine::Failures | StatusLine::Problem(_) => Status::Problem,
            StatusLine::Starting | StatusLine::Working | StatusLine::Stopping => Status::Working,
            StatusLine::Queued | StatusLine::Store => Status::WaitingTurn,
            StatusLine::TriedToday
            | StatusLine::New
            | StatusLine::Done
            | StatusLine::Nothing
            | StatusLine::StoppedEarly => Status::AllSet,
        }
    }

    /// The words key of the line.
    pub fn key(self) -> String {
        let case = match self {
            StatusLine::NeedsYou(kind) => kind.name(),
            StatusLine::NotYet => "category",
            StatusLine::Off => "person",
            StatusLine::Failures => "failures",
            StatusLine::Starting => "starting",
            StatusLine::Working => "working",
            StatusLine::Stopping => "stopping",
            StatusLine::Queued => "queued",
            StatusLine::Store => "store",
            StatusLine::Problem(f) => f.name(),
            StatusLine::TriedToday => "tried_today",
            StatusLine::New => "new",
            StatusLine::Done => "done",
            StatusLine::Nothing => "nothing",
            StatusLine::StoppedEarly => "stopped_early",
        };
        format!("line.{}.{}", self.status().name(), case)
    }
}
