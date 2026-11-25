use super::{Failure, ItemKind, LastCause, PaidEnd, PayClass, Status};

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
    /// `problem.site_declined`: the hold `site_declined`, after the person's no at the question
    /// that allows a site (r4 U2.4a). `all` does not list it until its fixture state of I8.3 is
    /// written.
    SiteDeclined,
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
    /// `last_cause`, I6.1), in place of the failed run's line; for `no_access` the case is
    /// `no_access_mac` or `no_access_windows` (`key_on`, r4 U3.22d). `all` does not list it until
    /// its fixture state of I8.3 is written.
    LastCause(LastCause),
    /// `needs_you.amounts.<class>`: the open item `amounts`, or the hold `amounts` before its item
    /// is up, of an agent of the class of its category (r4 U3.29). `all` does not list it until
    /// its fixture state of I8.3 is written.
    Amounts(PayClass),
    /// `problem.<end>.<class>`: the row of `Problem` for an agent whose newest started run paid or
    /// ordered and ended with a last cause of `PaidEnd`, with the class of the agent's category
    /// (r4 U3.29). `all` does not list it until its fixture state of I8.3 is written.
    Paid(PaidEnd, PayClass),
    /// `problem.answers_saved`: the row of `Problem` and of `LastCause` for an agent whose rows
    /// hold a set-up's `item_answered` behind the end of its newest run, which ended with a cause
    /// of the failure class (r4 U3.22a, the coordinator's fifth ruling on its words). `all` does
    /// not list it until its fixture state of I8.3 is written.
    AnswersSaved,
    /// `problem.answers_saved_today`: the row of `AnswersSaved` on a day with `starts_per_day`
    /// starts, on which step 2 of a start refuses the person's Start now (r4 U3.23, the
    /// coordinator's ruling of log line 19955). `all` does not list it until its fixture state of
    /// I8.3 is written.
    AnswersSavedToday,
    /// `problem.tried_today`: the row of `Problem`, and of `LastCause` for a cause of
    /// `roster::START_AGAIN_CAUSES`, on a day with `starts_per_day` starts (r4 U3.23, the
    /// coordinator's ruling of log line 19955). `all` does not list it until its fixture state of
    /// I8.3 is written.
    FailedToday,
    /// `all_set.tried_today`: a day with `starts_per_day` starts behind a last run that did not
    /// fail.
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
        all.extend(
            ItemKind::ALL
                .iter()
                .filter(|k| **k != ItemKind::Amounts)
                .map(|k| StatusLine::NeedsYou(*k)),
        );
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
            StatusLine::NeedsYou(_) | StatusLine::Amounts(_) => Status::NeedsYou,
            StatusLine::NotYet => Status::NotYet,
            StatusLine::Off => Status::Off,
            StatusLine::Kept => Status::Off,
            StatusLine::SignInRefused
            | StatusLine::SiteDeclined
            | StatusLine::Failures
            | StatusLine::Problem(_)
            | StatusLine::LastCause(_)
            | StatusLine::Paid(..)
            | StatusLine::AnswersSaved
            | StatusLine::AnswersSavedToday
            | StatusLine::FailedToday
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

    /// The words key of the line in this build (`key_on` of `System::HOST`).
    pub fn key(self) -> String {
        self.key_on(System::HOST)
    }

    /// The words key of the line on `system`: `line.problem.no_access_mac` or
    /// `line.problem.no_access_windows` for the last cause `no_access`, whose sentence says what to
    /// do on that system (r4 U3.22d), and the same key on every system for every other line; for
    /// a run that paid or ordered `line.problem.<end>.<class>` (r4 U3.29).
    pub fn key_on(self, system: System) -> String {
        match self {
            StatusLine::Paid(end, class) => {
                return format!("line.problem.{}.{}", end.name(), class.name())
            }
            StatusLine::Amounts(class) => {
                return format!("line.needs_you.amounts.{}", class.name())
            }
            _ => {}
        }
        let case = match self {
            StatusLine::NeedsYou(kind) => kind.name(),
            StatusLine::NotYet => "category",
            StatusLine::Off => "person",
            StatusLine::Kept => "kept",
            StatusLine::SignInRefused => "sign_in_refused",
            StatusLine::SiteDeclined => "site_declined",
            StatusLine::Failures => "failures",
            StatusLine::Starting => "starting",
            StatusLine::Working => "working",
            StatusLine::Stopping => "stopping",
            StatusLine::KillStuck(System::Mac) => "kill_stuck_mac",
            StatusLine::KillStuck(System::Windows) => "kill_stuck_windows",
            StatusLine::Queued => "queued",
            StatusLine::Store => "store",
            StatusLine::Problem(f) => f.name(),
            StatusLine::LastCause(LastCause::NoAccess) => match system {
                System::Mac => "no_access_mac",
                System::Windows => "no_access_windows",
            },
            StatusLine::LastCause(c) => c.name(),
            StatusLine::Paid(end, _) => end.name(),
            StatusLine::Amounts(_) => "amounts",
            StatusLine::AnswersSaved => "answers_saved",
            StatusLine::AnswersSavedToday => "answers_saved_today",
            StatusLine::FailedToday | StatusLine::TriedToday => "tried_today",
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
