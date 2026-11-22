use super::{Failure, ItemKind, LastCause, Status};

/// The system that Halvo runs on, which chooses the words of `working.kill_stuck`, because the
/// way to quit Halvo differs on each (I5.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum System {
    /// macOS.
    Mac,
    /// Windows.
    Windows,
}

impl System {
    /// Every system, in this order.
    pub const ALL: [System; 2] = [System::Mac, System::Windows];

    /// The system of this build: Windows where it is built for Windows, and macOS otherwise,
    /// since Halvo is built for those two alone.
    pub const HOST: System = if cfg!(windows) {
        System::Windows
    } else {
        System::Mac
    };
}

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
    /// `off.kept`: the derived hold `off` of I5.4, which `status` tells from `off.person` by the
    /// events (ruling RD-67 1.3 (c)). `all` does not list it until its fixture state of I8.3 is
    /// written.
    Kept,
    /// `problem.sign_in_refused`: the hold `sign_in_refused` (ruling RD-73 4.3). `all` does not
    /// list it until its fixture state of I8.3 is written.
    SignInRefused,
    /// `problem.failures`: the hold `failures`.
    Failures,
    /// `working.starting`: a live run in the phase `starting`.
    Starting,
    /// `working.working`: a live run in the phase `working`.
    Working,
    /// `working.stopping`: a live run in the phase `stopping`.
    Stopping,
    /// `working.kill_stuck_<system>`: the live run of an agent whose workflow a tick saw gone,
    /// whose kill was refused, so that the run outlived it (I5.7, ruling RD-54 (g)); its words
    /// say how to quit Halvo on that system.
    KillStuck(System),
    /// `waiting_turn.queued`: a queued start.
    Queued,
    /// `waiting_turn.store`: the agent store has not ended the agent's newest run (ruling
    /// XB-28 (b)).
    Store,
    /// `problem.<cause>`: a last run that ended with a cause of the failure class.
    Problem(Failure),
    /// `problem.<last cause>`: the row of `Problem` for an agent whose newest started run the
    /// agent store ended with a cause that the roster has a sentence for (the roster's key
    /// `last_cause`, I6.1), in place of the failed run's line. `all` does not list it until its
    /// fixture state of I8.3 is written.
    LastCause(LastCause),
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
    /// `problem.record`: the agent's record cannot be read or saved right now (I5.7, I5.8;
    /// ruling RD-64 1.5, 2.5), first of every row (I1.3).
    Record,
    /// `working.deleting`: the agent whose delete the person confirmed while its run was live,
    /// which Halvo stops and then removes (I5.7; ruling RD-19 (c) rule 7), the line that the plan
    /// names `line.deleting`.
    Deleting,
}

impl StatusLine {
    /// Every status line, in the order of I1.3.
    pub fn all() -> Vec<StatusLine> {
        let mut all = vec![StatusLine::Record];
        all.extend(ItemKind::ALL.iter().map(|k| StatusLine::NeedsYou(*k)));
        all.extend([
            StatusLine::NotYet,
            StatusLine::Off,
            StatusLine::Failures,
            StatusLine::Starting,
            StatusLine::Working,
            StatusLine::Stopping,
            StatusLine::KillStuck(System::Mac),
            StatusLine::KillStuck(System::Windows),
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
        all.push(StatusLine::Deleting);
        all
    }

    /// The status.
    pub fn status(self) -> Status {
        match self {
            StatusLine::NeedsYou(_) => Status::NeedsYou,
            StatusLine::NotYet => Status::NotYet,
            StatusLine::Off => Status::Off,
            StatusLine::Kept => Status::Off,
            StatusLine::SignInRefused
            | StatusLine::Failures
            | StatusLine::Problem(_)
            | StatusLine::LastCause(_)
            | StatusLine::Record => Status::Problem,
            StatusLine::Starting
            | StatusLine::Working
            | StatusLine::Stopping
            | StatusLine::KillStuck(_)
            | StatusLine::Deleting => Status::Working,
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
            StatusLine::Kept => "kept",
            StatusLine::SignInRefused => "sign_in_refused",
            StatusLine::Failures => "failures",
            StatusLine::Starting => "starting",
            StatusLine::Working => "working",
            StatusLine::Stopping => "stopping",
            StatusLine::KillStuck(System::Mac) => "kill_stuck_mac",
            StatusLine::KillStuck(System::Windows) => "kill_stuck_windows",
            StatusLine::Queued => "queued",
            StatusLine::Store => "store",
            StatusLine::Problem(f) => f.name(),
            StatusLine::LastCause(c) => c.name(),
            StatusLine::TriedToday => "tried_today",
            StatusLine::New => "new",
            StatusLine::Done => "done",
            StatusLine::Nothing => "nothing",
            StatusLine::StoppedEarly => "stopped_early",
            StatusLine::Record => "record",
            StatusLine::Deleting => "deleting",
        };
        format!("line.{}.{}", self.status().name(), case)
    }
}
