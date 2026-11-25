use crate::guard::{self, EffectRecord, Policy, Refusal, Table};
use crate::items::drive::{self as items, Held, HeldBind, Ledger, SetupDone};
use crate::items::{
    admits, compose_amounts, compose_check, compose_link_account, compose_plain, compose_sign_in,
    native_card, params, raise_confirm, raise_step, ConfirmAnswer, ConfirmMove, ConfirmState,
    Differs, NativeCard, Plain, Raise, Setup, SetupInput, SetupParam, Source,
};
use crate::ports::{
    Analysis, Chosen, Clock, Files, Owner, Page, PortError, Rng, Runs, Senders, Tray,
};
use crate::roster::{self, Placed};
use crate::run::{self, Act, Done, StopBy};
use crate::schedule::{self, Board, Move, Report};
use crate::sync::{Next, Sync};
use crate::types::{
    Action, AgentId, AmountsAnswer, BusyHistory, CandidateFrom, Card, Cause, CheckAnswer, Choice,
    Class, ConfigView, Doing, Facts, FullWorkflow, HandbackAsk, Item, ItemId, ItemKind, LapseCause,
    Limits, Live, OtherLapse, OwnItemKind, PageCause, Param, PayClass, Presenter, Row, RunEnd,
    RunEvent, RunId, RunItem, RunItemKind, Snapshot, Status, StatusLine, System, Unknown,
    WorkflowId,
};
use crate::words;
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// What the app's quit hook waits for after the first step of the quit (I5.7, ruling RD-8): the
/// latest time, in milliseconds of `Clock::now`, at which it calls the second step,
/// `cancel_grace_s` after the stop of each run that is still live, where the stop of a run that
/// was stopped before the quit counts from that stop; the hook calls it sooner once no run is
/// live (`Coordinator::live`). The hook waits for the distance from `at`, the first step's own
/// read of the clock, to `until`, never longer than `cancel_grace_s`, and counts it from the
/// return of the first step on its own monotonic time, not from `at`; it calls no clock port.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuitWait {
    /// The time of the second step at the latest.
    pub until: u64,
    /// The time that the first step read on the clock, or the last time that this life read
    /// when that read failed.
    pub at: u64,
}

type Cfg<P> = <P as Owner>::Config;

/// The coordinator of one life of the app (I5.10): one type over a value `P` that implements
/// every port and `Policy`. It holds the board of the schedule, the guard's table, one ledger
/// per agent, the held confirmations and setups, and the state that the page receives, behind
/// one lock that each method takes for its move and gives back when it returns, so that no
/// move is held open between two calls. Each move calls the pure functions and the one `drive`
/// function of each part, and the coordinator sends no body and logs no event of its own
/// (rulings IG-6c, IG-9). Before each tick it makes the creation move (I5.9); its first tick is
/// the recovery of I5.8. After every move the rows, the items and the note are composed again,
/// and the page and the tray hear of a change (I8.2).
pub struct Coordinator<P: Owner> {
    ports: P,
    limits: Limits,
    label: fn(&FullWorkflow) -> Option<String>,
    state: Mutex<State<Cfg<P>>>,
    /// The latest read of the clock that answered in this life, in milliseconds, `NO_READ` before
    /// the first (ruling RD-133, answer (b)): the coordinator's own reads and the read that
    /// stamped each drive's move. Named limits: a read inside the items drive is not noted, so
    /// the noted read can be older than the last read of the life, never later than it.
    last_read: AtomicU64,
    /// How many reads of the clock answered in this life, so that a tick knows whether its own
    /// work read one (ruling RD-138 (2)).
    reads: AtomicU64,
    unstopped: Arc<Mutex<Vec<(AgentId, RunId)>>>,
    live_runs: AtomicUsize,
    quit_mark: Arc<AtomicBool>,
    /// The mark of the hold of a delete of everything (`hold_stop`): read where the quit's mark
    /// is read, and lifted by `lift_hold`, which never lifts the quit's.
    hold_mark: Arc<AtomicBool>,
    /// The workflow of each member as the last publish found it, which `quit_write` reads
    /// without the coordinator's lock.
    flows: Mutex<Vec<(AgentId, crate::types::WorkflowId)>>,
    /// The runs, each with its agent, whose `run_not_stopped` the owner acknowledged in this
    /// life (`quit_write`).
    written: Mutex<Vec<(AgentId, RunId)>>,
}

const NO_READ: u64 = 0;

/// Where the rest of a switch-off that waited for the clock stands after a read that answered.
#[derive(Clone, Copy, PartialEq, Eq)]
enum OffRest {
    /// The hold shows on the board and the move `off` made the rest.
    Done,
    /// A read of the clock failed again: the switch-off waits for the next one.
    Clock,
    /// The owner has not taken the `hold_set`, or its events could not be read: the agent is
    /// held off on the board (`hold_place`) and the next ticks try again, with no tick held.
    Owner,
}

/// A clock that answers one instant, the last read of this life, with the day that the
/// app's clock gives it, so that a row is stamped at that read (ruling RD-133, answer (b)).
struct LastRead<'a, P>(&'a P, u64);

impl<P: Clock> Clock for LastRead<'_, P> {
    fn now(&self) -> Result<u64, PortError> {
        Ok(self.1)
    }

    fn day(&self, at: u64) -> Result<u32, PortError> {
        self.0.day(at)
    }
}

/// An open item that a live run waits on, of a kind other than `confirm` and other than the
/// second step of a sign-in, with the time of its raise, which the first tick that reads the
/// clock sets where the raise could not read it (RD-34 (a)).
struct Waited {
    run: RunId,
    agent: AgentId,
    item: RunItem,
    raised_at: Option<u64>,
}

/// A delete that the person confirmed while a run of the agent was on the board, held in
/// memory for this life alone and stored nowhere, so that a new life sends no delete from an
/// earlier yes (I5.7; rulings RD-30 (1), RD-31 (4), RD-64 1.6 (b)).
struct Deleting {
    agent: AgentId,
    workflow: WorkflowId,
    run: RunId,
    /// Whether the delete waits for the run to leave the board alone and not for its stored
    /// `run_ended`: the agent's record was stuck at the yes (ruling RD-64 1.6 (b)).
    board_end: bool,
}

#[derive(Clone, Copy)]
struct AnsweredBy {
    step: Option<u32>,
    from: Presenter,
    now: Option<u64>,
    /// Whether the call says that more calls of its press follow (I8.1 `more`; r4 U3.30).
    more: bool,
}

struct State<C> {
    board: Option<Board<C>>,
    table: Table,
    ledgers: Vec<(AgentId, Ledger)>,
    held: Vec<Held>,
    binds: Vec<HeldBind>,
    handbacks: Vec<(ItemId, HandbackAsk)>,
    waits: Vec<Waited>,
    setups: Vec<Setup>,
    forgotten: Vec<AgentId>,
    quitting: bool,
    sync: Sync,
    mark: Mark,
    answering: Vec<ItemId>,
    returned: bool,
    offs: Vec<AgentId>,
    ended: Vec<(RunId, Option<crate::types::Outcome>)>,
    marked: Vec<RunId>,
    held_off: Vec<AgentId>,
    refused_on: Vec<(AgentId, crate::ports::Shared)>,
    /// Set by a `stop_sign_in` whose body `drop_sessions` the owner refused with `store`, and
    /// taken by the `control_reply` of that same call, which answers it with
    /// `Refusal::SignInNotFinished` (r4 U3.16).
    sign_in_unfinished: bool,
    /// Set by a `use_model` whose body the owner refused or that got no reply, and taken by the
    /// `control_reply` of that same call, which answers it with the refusal of that action that
    /// says that Halvo could not finish this (r4 U3.17b).
    model_unfinished: Option<Action>,
    deleting: Vec<Deleting>,
    /// The agents that ask first whose queued start the person took back, each with the `ready`
    /// item that a later tick raised for it since, or none until then: that item's row says
    /// that the start was cancelled and that the agent asks again, in this life alone (the
    /// gates of U2.4e, ruling RD-152 (4)).
    taken_back: Vec<(AgentId, Option<ItemId>)>,
    checks: Vec<(ItemId, Vec<Unknown>)>,
    /// For each open item `amounts`, the payment or order of its run as the first read of the
    /// agent's history that answered gave it, or none where that read holds none (r4 U3.29); an
    /// item that no read answered yet is no entry. The fields live in this life's state alone.
    amounts: Vec<(ItemId, Option<Differs>)>,
    /// The runs of an earlier life that this life's `Runs::reap` answered `alive` and whose kill
    /// at the second step of the quit this life's library answered Ok.
    killed_kept: Vec<RunId>,
}

/// What a move or a tick that did not return left (rulings RD-80 3.4, 4.1, 4.2): `unpublished`
/// until the first publish after such a move, which raises `seq` also when nothing differs;
/// `tick` from a tick that did not return until a tick returns, while every publish carries the
/// note `note.out_of_date`; `defers` from such a tick until the next move forgets the defers of
/// the runs on the board (ruling RD-122).
#[derive(Clone, Copy, Default)]
struct Mark {
    unpublished: bool,
    tick: bool,
    defers: bool,
}

/// How a call takes the coordinator's lock: a tick, another move, or a call that reads or
/// notes and moves nothing.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Entry {
    Tick,
    Move,
    Read,
}

/// The coordinator's state while a call holds its lock. A call that does not return leaves the
/// mark and rings the bell as it unwinds, and runs nothing else there (ruling RD-80 4.2); a call
/// made while another panic already unwinds its thread, and that returns, leaves none.
struct Entered<'a, P: Owner + Page> {
    st: MutexGuard<'a, State<Cfg<P>>>,
    ports: &'a P,
    entry: Entry,
    unwinding: bool,
    mark_before: Option<QuitLine>,
}

/// What the thread of a call that holds the coordinator's lock reads of the app's quit: the
/// mark, the mark of the hold of a delete of everything, and the list of the runs that did not
/// stop, inside whose lock a start reads the marks for the last time.
type QuitLine = (
    Arc<AtomicBool>,
    Arc<AtomicBool>,
    Arc<Mutex<Vec<(AgentId, RunId)>>>,
);

thread_local! {
    static QUIT_MARK: RefCell<Option<QuitLine>> = const { RefCell::new(None) };
}

fn mark_this_thread(line: Option<QuitLine>) -> Option<QuitLine> {
    QUIT_MARK.with(|m| m.replace(line))
}

pub(crate) fn quit_marked() -> bool {
    QUIT_MARK.with(|m| {
        m.borrow().as_ref().is_some_and(|(mark, hold, _)| {
            mark.load(Ordering::SeqCst) || hold.load(Ordering::SeqCst)
        })
    })
}

/// The last reading of the app's quit mark by a start, directly before `Runs::start` (ruling
/// RD-147 (5)(i)): inside the lock of the list of the runs that did not stop, false where the
/// quit is marked; otherwise it writes `run` with `agent` at the end of that list and is true.
/// The quit hook marks the quit before it reads that list inside the same lock, so either the
/// start finds the mark or the hook's list names the run. True on a thread that holds no
/// coordinator's lock.
pub(crate) fn past_the_mark(agent: &AgentId, run: &RunId) -> bool {
    QUIT_MARK.with(|m| {
        let line = m.borrow();
        let Some((mark, hold, unstopped)) = line.as_ref() else {
            return true;
        };
        let mut listed = unstopped.lock().unwrap_or_else(PoisonError::into_inner);
        if mark.load(Ordering::SeqCst) || hold.load(Ordering::SeqCst) {
            return false;
        }
        listed.push((agent.clone(), run.clone()));
        true
    })
}

impl<P: Owner + Page> std::ops::Deref for Entered<'_, P> {
    type Target = State<Cfg<P>>;

    fn deref(&self) -> &State<Cfg<P>> {
        &self.st
    }
}

impl<P: Owner + Page> std::ops::DerefMut for Entered<'_, P> {
    fn deref_mut(&mut self) -> &mut State<Cfg<P>> {
        &mut self.st
    }
}

impl<P: Owner + Page> Drop for Entered<'_, P> {
    fn drop(&mut self) {
        mark_this_thread(self.mark_before.take());
        if self.unwinding || !std::thread::panicking() {
            return;
        }
        self.st.mark.unpublished = true;
        if self.entry == Entry::Tick {
            self.st.mark.tick = true;
            self.st.mark.defers = true;
        }
        let _ = self.ports.changed();
    }
}

fn plain_label(workflow: &FullWorkflow) -> Option<String> {
    workflow.site.as_deref().and_then(roster::plain_site)
}

fn cause_of(error: &PortError) -> Cause {
    match error {
        PortError::Busy => Cause::Busy,
        PortError::NoService | PortError::Refused(_) => Cause::NoService,
    }
}

fn answered(report: &Report) -> Result<(), Cause> {
    report.answer.map(|_| ())
}

fn index<C>(board: &Board<C>, agent: &AgentId) -> Option<usize> {
    board.members.iter().position(|m| &m.facts.agent == agent)
}

fn set_aside<C>(board: &mut Board<C>, agent: &AgentId) -> Option<Vec<schedule::Waiting>> {
    let order = board.queue.entries().to_vec();
    board.queue.remove(agent).then_some(order)
}

fn put_back<C>(board: &mut Board<C>, agent: &AgentId, order: &[schedule::Waiting]) {
    let now = board.queue.entries().to_vec();
    for w in &now {
        board.queue.remove(&w.agent);
    }
    let kept = |w: &&schedule::Waiting| &w.agent == agent || now.iter().any(|n| n.agent == w.agent);
    for w in order.iter().filter(kept) {
        board.queue.push(w.clone());
    }
    for w in now
        .iter()
        .filter(|n| !order.iter().any(|o| o.agent == n.agent))
    {
        board.queue.push(w.clone());
    }
}

/// Whether the owner has acknowledged the `paused` of `agent`, the task that the person switched
/// off, for the kept body that its switch-off freed (I3.5 (e); ruling RD-160 (2)): its workflow is
/// listed `paused`, has left the list or is the stand-in of a dismissed one, or the board holds
/// no member of it.
fn off_paused<C>(board: &Board<C>, agent: &AgentId) -> bool {
    match index(board, agent) {
        None => true,
        Some(i) => {
            let facts = &board.members[i].facts;
            facts.stand_in
                || facts.workflow.state == crate::types::WorkflowState::Paused
                || board.gone.contains(&facts.workflow.id)
        }
    }
}

/// The items ledger of the agent of member `i`, which takes the owner's events that the board
/// read after its last acknowledged one, the coordinator's read right after a call that answered
/// no acknowledgement among them, as its own read after a refusal `seq` takes them (ruling
/// RD-40), and shares the pair's one stored last with the board: what the ledger learnt by its
/// own store or read since becomes the board's, and the ledger then numbers after the board's
/// (ruling RD-64 3.2).
fn ledger<'a, C>(
    ledgers: &'a mut Vec<(AgentId, Ledger)>,
    board: &mut Board<C>,
    i: usize,
) -> &'a mut Ledger {
    let facts = &board.members[i].facts;
    let k = match ledgers.iter().position(|(a, _)| a == &facts.agent) {
        Some(k) => k,
        None => {
            let fresh = Ledger::new(
                facts.workflow.id.clone(),
                facts.agent.clone(),
                board.stored_last(i),
            );
            ledgers.push((facts.agent.clone(), fresh));
            ledgers.len() - 1
        }
    };
    let l = &mut ledgers[k].1;
    l.set_dismissed(board.workflow_dismissed(&board.members[i].facts.workflow.id));
    if let Some(last) = l.learnt() {
        board.set_last(i, last);
    }
    l.take(&board.members[i].facts.events);
    l.follow(board.stored_last(i));
    l
}

/// The stored last that each ledger learnt by its own store or read becomes the board's, before
/// a sender of the board numbers (ruling RD-64 3.2).
fn share_lasts<C>(board: &mut Board<C>, ledgers: &mut [(AgentId, Ledger)]) {
    for (agent, l) in ledgers.iter_mut() {
        if let Some(i) = index(board, agent) {
            if let Some(last) = l.learnt() {
                board.set_last(i, last);
            }
        }
    }
}

fn hold_stop<C>(board: &mut Board<C>, table: &mut Table, run: &RunId, now: u64, by: StopBy) {
    *table = table.stop(run);
    if !board.runs.iter().any(|r| &r.run == run && r.stop.is_some()) {
        schedule::stop_run(board, run, now, by.cause().end());
    }
}

fn waiting_on(doing: &Doing) -> Option<RunItem> {
    match doing {
        Doing::Live(Live::Waiting(open)) => Some(open.clone()),
        _ => None,
    }
}

/// Whether a stop on the board, or the stop of a dying tree, has no time yet: the record of a
/// stop that no read of the clock answered, written before its cancel, whose time the next read
/// that answers gives (rulings RD-133 1, RD-138 (1)).
fn unstamped_stops<C>(board: &Board<C>) -> bool {
    board.runs.iter().any(|r| r.stop.is_some_and(|s| s.at == 0))
        || board.dying.iter().any(|d| d.stopped == 0)
}

/// Runs one agent's part of a move's take-up and holds its unwind to it (rulings RD-138
/// Addition 1, RD-140 R1): none where it panicked, so that the caller puts what it took back
/// and marks the move, and the move's own Stop or Switch off still goes on.
fn contain<R>(work: impl FnOnce() -> R) -> Option<R> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)).ok()
}

fn live_unstopped_run<C>(board: &Board<C>, agent: &AgentId) -> Option<RunId> {
    board
        .runs
        .iter()
        .find(|r| &r.agent == agent && r.stop.is_none())
        .map(|r| r.run.clone())
}

struct Reading<'a, P, C> {
    ports: &'a P,
    board: &'a mut Board<C>,
    i: usize,
    made: bool,
    stopped: bool,
}

impl<'a, P: Owner, C: ConfigView> Reading<'a, P, C> {
    fn new(ports: &'a P, board: &'a mut Board<C>, i: usize) -> Reading<'a, P, C> {
        Reading {
            ports,
            board,
            i,
            made: false,
            stopped: false,
        }
    }

    fn read(&mut self, l: &mut Ledger) -> items::Read {
        self.made = true;
        let facts = &mut self.board.members[self.i].facts;
        let read = schedule::resync(self.ports, &facts.agent, &mut facts.events);
        l.take(&facts.events);
        if let Ok(last) = read {
            self.board.set_last(self.i, last);
            l.follow(last);
        }
        let facts = &self.board.members[self.i].facts;
        let agent = facts.agent.clone();
        let off = schedule::holds(facts).contains(&schedule::Hold::Off);
        self.stopped = off && live_unstopped_run(self.board, &agent).is_some();
        match (read, self.stopped) {
            (Err(_), _) => items::Read::Failed,
            (Ok(_), true) => items::Read::Off,
            (Ok(_), false) => items::Read::Taken,
        }
    }
}

struct LapseParts<'a> {
    table: &'a mut Table,
    ledgers: &'a mut Vec<(AgentId, Ledger)>,
    held: &'a mut Vec<Held>,
    binds: &'a mut Vec<HeldBind>,
    handbacks: &'a mut Vec<(ItemId, HandbackAsk)>,
    answering: &'a mut Vec<ItemId>,
}

fn candidates(param: Param, workflow: &FullWorkflow) -> Vec<(Vec<String>, CandidateFrom)> {
    match (param, &workflow.site) {
        (Param::Start | Param::Payee | Param::Seller, Some(site)) => {
            vec![(vec![site.clone()], CandidateFrom::Records)]
        }
        _ => Vec::new(),
    }
}

impl<P> Coordinator<P>
where
    P: Owner
        + Runs
        + Analysis
        + Clock
        + Rng
        + Tray
        + Page
        + Policy<Config = Cfg<P>>
        + Senders
        + Files,
    Cfg<P>: ConfigView + Clone,
{
    /// A coordinator of a new life over `ports` with `limits`, which recovers at its first
    /// tick; the label of a site is `roster::plain_site` until `with_label` hands the app's.
    pub fn new(ports: P, limits: Limits) -> Coordinator<P> {
        Coordinator {
            ports,
            limits,
            label: plain_label,
            state: Mutex::new(State {
                board: None,
                table: Table::new(),
                ledgers: Vec::new(),
                held: Vec::new(),
                binds: Vec::new(),
                handbacks: Vec::new(),
                waits: Vec::new(),
                setups: Vec::new(),
                forgotten: Vec::new(),
                quitting: false,
                sync: Sync::new(),
                mark: Mark::default(),
                answering: Vec::new(),
                returned: false,
                offs: Vec::new(),
                ended: Vec::new(),
                marked: Vec::new(),
                held_off: Vec::new(),
                refused_on: Vec::new(),
                sign_in_unfinished: false,
                model_unfinished: None,
                deleting: Vec::new(),
                taken_back: Vec::new(),
                checks: Vec::new(),
                amounts: Vec::new(),
                killed_kept: Vec::new(),
            }),
            last_read: AtomicU64::new(NO_READ),
            reads: AtomicU64::new(0),
            unstopped: Arc::new(Mutex::new(Vec::new())),
            live_runs: AtomicUsize::new(0),
            quit_mark: Arc::new(AtomicBool::new(false)),
            hold_mark: Arc::new(AtomicBool::new(false)),
            flows: Mutex::new(Vec::new()),
            written: Mutex::new(Vec::new()),
        }
    }

    /// The coordinator with the app's function that gives a workflow's site label (I2.3).
    pub fn with_label(mut self, label: fn(&FullWorkflow) -> Option<String>) -> Coordinator<P> {
        self.label = label;
        self
    }

    /// The ports.
    pub fn ports(&self) -> &P {
        &self.ports
    }

    /// Takes the lock for a call. A move first takes up what a move that did not return left;
    /// then the first call after such a move publishes, before its own work (ruling RD-80 3.4,
    /// 4.2).
    fn enter(&self, entry: Entry) -> Entered<'_, P> {
        let mut st = Entered {
            st: self.state.lock().unwrap_or_else(PoisonError::into_inner),
            ports: &self.ports,
            entry,
            unwinding: std::thread::panicking(),
            mark_before: mark_this_thread(Some((
                Arc::clone(&self.quit_mark),
                Arc::clone(&self.hold_mark),
                Arc::clone(&self.unstopped),
            ))),
        };
        if entry != Entry::Read {
            let tick = st.mark.tick;
            if let Some(board) = st.board.as_mut() {
                board.hold_queue(tick);
            }
            self.take_up(&mut st);
            let held = st.mark.tick && entry == Entry::Move;
            if let Some(board) = st.board.as_mut() {
                board.hold_queue(held);
            }
        }
        if st.mark.unpublished {
            self.publish(&mut st);
        }
        st
    }

    fn enter_control(&self, agent: &AgentId, action: Option<Action>) -> Entered<'_, P> {
        let mut st = Entered {
            st: self.state.lock().unwrap_or_else(PoisonError::into_inner),
            ports: &self.ports,
            entry: Entry::Move,
            unwinding: std::thread::panicking(),
            mark_before: mark_this_thread(Some((
                Arc::clone(&self.quit_mark),
                Arc::clone(&self.hold_mark),
                Arc::clone(&self.unstopped),
            ))),
        };
        let tick = st.mark.tick;
        if let Some(board) = st.board.as_mut() {
            board.hold_queue(tick);
        }
        let aside = if Self::takes_its_start_away(&st, agent, action) {
            st.board.as_mut().and_then(|board| set_aside(board, agent))
        } else {
            None
        };
        let taken = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.take_up(&mut st);
        }));
        if let (Some(order), Some(board)) = (aside, st.board.as_mut()) {
            put_back(board, agent, &order);
        }
        if let Err(unwind) = taken {
            std::panic::resume_unwind(unwind);
        }
        let held = st.mark.tick;
        if let Some(board) = st.board.as_mut() {
            board.hold_queue(held);
        }
        if st.mark.unpublished {
            self.publish(&mut st);
        }
        st
    }

    fn takes_its_start_away(st: &State<Cfg<P>>, agent: &AgentId, action: Option<Action>) -> bool {
        let named = st
            .board
            .as_ref()
            .is_some_and(|board| index(board, agent).is_some_and(|i| !Self::gone(st, board, i)));
        let off = st.offs.contains(agent) || st.held_off.contains(agent);
        named
            && match action {
                None | Some(Action::TakeBack | Action::Stop) => true,
                Some(Action::SwitchOff) => !off,
                Some(
                    Action::RunNow
                    | Action::SwitchOn
                    | Action::StopSignIn
                    | Action::UseModel
                    | Action::ThisComputerOnly
                    | Action::ChangeAnswers,
                ) => false,
            }
    }

    fn lock(&self) -> Entered<'_, P> {
        self.enter(Entry::Read)
    }

    /// Whether an answer for `item` waits for the settle of RD-84, which could not read the
    /// clock: only a move that would take the waiting item waits and answers `no_service`, and
    /// every other move goes on (ruling RD-133 1).
    fn waits_on_item(st: &State<Cfg<P>>, item: &ItemId) -> bool {
        st.answering.contains(item)
    }

    /// Whether an event of `run` waits for the settle of RD-84: the run of the waiting item
    /// (ruling RD-133 1).
    fn waits_on_run(st: &State<Cfg<P>>, run: &RunId) -> bool {
        st.answering.iter().any(|item| {
            st.held
                .iter()
                .any(|h| h.item() == item && h.ask().run() == run)
        })
    }

    /// Whether a tick is held: a settle of RD-84 or a stop or switch-off of RD-133 still waits
    /// for a read of the clock that answers, so the tick does none of its work. A stop waits
    /// while its time on the board is 0 (`unstamped_stops`), the record that it wrote before its
    /// cancel (ruling RD-138 (1)). A held tick publishes, so that what its take-up changed shows
    /// (RD-138 (3)), and only a tick whose own work read the clock clears the mark (RD-138 (2)).
    fn holds_tick(st: &State<Cfg<P>>) -> bool {
        !st.answering.is_empty()
            || st.board.as_ref().is_some_and(unstamped_stops)
            || !st.offs.is_empty()
            || !st.ended.is_empty()
            || st.ledgers.iter().any(|(_, l)| l.has_unstamped())
    }

    /// What a move takes up first: after a tick that did not return, the defers of the runs on
    /// the board are not known (ruling RD-122); then, at one read of the clock that answers,
    /// what waited for a stamp (`stamp_waiting`) and the settle of RD-84.
    fn take_up(&self, st: &mut State<Cfg<P>>) {
        if st.mark.defers {
            st.mark.defers = false;
            if let Some(board) = st.board.as_mut() {
                board.defers_not_known();
            }
        }
        if !Self::holds_tick(st) {
            return;
        }
        let Ok(stamp) = crate::ports::Stamp::read(&self.ports) else {
            return;
        };
        self.noted(Some(stamp.now()));
        self.stamp_waiting(st, stamp);
        for (run, outcome) in std::mem::take(&mut st.ended) {
            let owing = Self::owing_end(st, &run, outcome, stamp);
            if contain(|| self.end_of_run(st, &run, outcome)).is_none() {
                match (owing, st.board.as_mut()) {
                    (Some(owed), Some(board)) if !board.runs.iter().any(|r| r.run == run) => {
                        board.owed.push(owed);
                    }
                    _ => self.keep_end(st, &run, outcome),
                }
                self.held_panic(st);
            }
        }
        self.offs_at_stamp(st, stamp);
        self.settle(st, stamp);
    }

    /// What a stop or a switch-off made with no read of the clock left, at the first read that
    /// answers and before the move's own work (ruling RD-133 1): each stop on the board takes
    /// that read as its time, so that the kill counts `cancel_grace_s` from it; the rows that
    /// waited are stamped with it and sent. The runner's ends that the window kept and the rest
    /// of each switch-off follow in `take_up` (`end_of_run`, `offs_at_stamp`).
    fn stamp_waiting(&self, st: &mut State<Cfg<P>>, stamp: crate::ports::Stamp) {
        let State { board, ledgers, .. } = st;
        let Some(board) = board.as_mut() else { return };
        for stop in board.runs.iter_mut().filter_map(|r| r.stop.as_mut()) {
            if stop.at == 0 {
                stop.at = stamp.now();
            }
        }
        for d in board.dying.iter_mut().filter(|d| d.stopped == 0) {
            d.stopped = stamp.now();
        }
        let mut panicked = false;
        for (agent, l) in ledgers.iter_mut() {
            if !l.has_unstamped() {
                continue;
            }
            l.stamp_unstamped(stamp);
            let sent = contain(|| {
                let _ = items::flush(&self.ports, l, &self.limits);
                if let Some(i) = index(board, agent) {
                    self.read_back(board, i);
                }
            });
            panicked |= sent.is_none();
        }
        if panicked {
            self.held_panic(st);
        }
    }

    /// What a panic held to one agent's work in a move leaves: the move is marked, so that the
    /// next call publishes, and the bell rings, as for a move that did not return.
    fn held_panic(&self, st: &mut State<Cfg<P>>) {
        st.mark.unpublished = true;
        let _ = self.ports.changed();
    }

    /// The switch-offs that waited for a read of the clock, after the runner's ends that the
    /// window kept are made, so that the rest of a switch-off does not stop a run that ended.
    fn offs_at_stamp(&self, st: &mut State<Cfg<P>>, stamp: crate::ports::Stamp) {
        let State {
            board,
            ledgers,
            offs,
            held_off,
            ..
        } = st;
        let Some(board) = board.as_mut() else { return };
        let waiting: Vec<AgentId> = std::mem::take(offs);
        let mut panicked = false;
        for agent in waiting {
            let rest = contain(|| self.off_at_stamp(board, ledgers, &agent, stamp));
            panicked |= rest.is_none();
            match rest.unwrap_or(OffRest::Owner) {
                OffRest::Done => {}
                OffRest::Clock => offs.push(agent),
                OffRest::Owner => {
                    board.hold_place(&agent, true);
                    if !held_off.contains(&agent) {
                        held_off.push(agent);
                    }
                }
            }
        }
        if panicked {
            self.held_panic(st);
        }
    }

    /// The tick's turn at the switch-offs whose `hold_set` the owner has not taken: each is
    /// tried again at one read of the clock, and the agent stays held off on the board until its
    /// hold shows; then the board holds its place as its ledger says. Each agent's try is held
    /// to that agent as the take-up's are (ruling RD-140 R1, RD-144 (1)): one that panics leaves
    /// its agent held off and the others tried, and the answer says that a try panicked.
    fn held_off_at_tick(
        &self,
        board: &mut Board<Cfg<P>>,
        st_ledgers: &mut Vec<(AgentId, Ledger)>,
        held_off: &mut Vec<AgentId>,
    ) -> bool {
        if held_off.is_empty() {
            return false;
        }
        let stamp = crate::ports::Stamp::read(&self.ports).ok();
        if let Some(stamp) = stamp {
            self.noted(Some(stamp.now()));
        }
        let mut panicked = false;
        for agent in std::mem::take(held_off) {
            let rest = match stamp {
                Some(stamp) => contain(|| self.off_at_stamp(board, st_ledgers, &agent, stamp)),
                None => Some(OffRest::Clock),
            };
            panicked |= rest.is_none();
            if rest == Some(OffRest::Done) {
                let keep = st_ledgers
                    .iter()
                    .any(|(a, l)| a == &agent && l.holds_place());
                board.hold_place(&agent, keep);
            } else if index(board, &agent).is_some() {
                board.hold_place(&agent, true);
                held_off.push(agent);
            }
        }
        panicked
    }

    /// The rest of a switch-off that no read of the clock stamped, at `stamp`, the first read
    /// that answered: whether the agent still waits. The agent's events are read first, so
    /// that a `hold_set` that the owner acknowledged is seen whatever an earlier read-back
    /// gave; a `hold_set` that still waits for the owner goes again and is never queued a
    /// second time; with no `hold_set` stored, the ledger stores one at `stamp`; once the
    /// board shows the hold, the move `off` makes the rest. Where the owner does not take the
    /// `hold_set`, or the agent's events cannot be read, whatever the name of the refusal, the
    /// agent leaves the switch-offs that hold the tick and is held off on the board until a
    /// later tick sees its hold (`held_off_at_tick`), so that no refusal holds every tick and no
    /// tick starts the agent.
    fn off_at_stamp(
        &self,
        board: &mut Board<Cfg<P>>,
        ledgers: &mut Vec<(AgentId, Ledger)>,
        agent: &AgentId,
        stamp: crate::ports::Stamp,
    ) -> OffRest {
        let Some(i) = index(board, agent) else {
            return OffRest::Done;
        };
        let l = ledger(ledgers, board, i);
        if !l.waiting().is_empty() {
            let _ = items::flush(&self.ports, l, &self.limits);
        }
        if l.waits_with_a_persons_hold() {
            return OffRest::Owner;
        }
        if self.read_agent(board, i).is_err() {
            return OffRest::Owner;
        }
        if !schedule::holds(&board.members[i].facts).contains(&schedule::Hold::Off) {
            let l = ledger(ledgers, board, i);
            if items::store_held_off(&self.ports, l, &self.limits, stamp).is_err()
                || self.read_agent(board, i).is_err()
                || !schedule::holds(&board.members[i].facts).contains(&schedule::Hold::Off)
            {
                return OffRest::Owner;
            }
        }
        let report = schedule::drive(
            &self.ports,
            board,
            &self.limits,
            Move::Off {
                agent: agent.clone(),
            },
        );
        if report.unstamped {
            OffRest::Clock
        } else if matches!(report.answer, Err(Cause::Busy)) {
            OffRest::Owner
        } else {
            OffRest::Done
        }
    }

    /// Reads the agent's events back onto the board.
    fn read_agent(&self, board: &mut Board<Cfg<P>>, i: usize) -> Result<(), PortError> {
        let facts = &mut board.members[i].facts;
        let last = schedule::resync(&self.ports, &facts.agent, &mut facts.events)?;
        board.set_last(i, last);
        Ok(())
    }

    /// The settle of a confirmation that an answer's move left `answered` and did not return
    /// (ruling RD-84 items 1 and 2), keyed on the item that the move noted, never on the state
    /// alone: at one read of the clock, the run stopped on the board with `answer_lost`, then the
    /// lapse with `answer_lost`, which stops the run in the guard's table, queues its row,
    /// sends `Runs::cancel` once and no decision, and then the row. Where the clock cannot be
    /// read the moves on the item and its run wait (`waits_on_item`, `waits_on_run`) and the
    /// next move that reads the clock settles it; an item that is no longer `answered`, as after
    /// the person's Stop of its run, is not touched.
    fn settle(&self, st: &mut State<Cfg<P>>, stamp: crate::ports::Stamp) {
        for item in std::mem::take(&mut st.answering) {
            if contain(|| self.settle_item(st, stamp, item)).is_none() {
                self.held_panic(st);
            }
        }
    }

    /// The quit's first step while a settle waits for the clock (ruling RD-138 (4)): the run of
    /// each waiting item is stopped on the guard's table and the board with `answer_lost` and no
    /// time, so that the quit's stop-all does not stop it `cancelled` and its lapse at the quit
    /// leaves the item to the settle, which lapses it `answer_lost` and sends the one cancel at
    /// the next read that answers.
    fn stop_for_settles(&self, st: &mut State<Cfg<P>>) {
        let State {
            board,
            table,
            held,
            answering,
            ..
        } = st;
        let Some(board) = board.as_mut() else { return };
        for h in held.iter().filter(|h| answering.contains(h.item())) {
            if matches!(h.confirmation().state(), ConfirmState::Answered(_)) {
                hold_stop(board, table, h.ask().run(), 0, StopBy::AnswerLost);
            }
        }
    }

    /// The settle of one item that `answering` noted. `settle` holds the unwind of each item's
    /// settle to that item, marking the move and ringing the bell, so that a panic there does not
    /// undo another agent's Stop or Switch off that the same move takes (RD-138 Addition 1).
    fn settle_item(&self, st: &mut State<Cfg<P>>, stamp: crate::ports::Stamp, item: ItemId) {
        let State {
            board,
            table,
            ledgers,
            held,
            ..
        } = st;
        let Some(board) = board.as_mut() else { return };
        let Some(h) = held.iter_mut().find(|h| h.item() == &item) else {
            return;
        };
        if !matches!(h.confirmation().state(), ConfirmState::Answered(_)) {
            return;
        }
        let Some(i) = index(board, h.ask().agent()) else {
            return;
        };
        let run = h.ask().run().clone();
        hold_stop(board, table, &run, stamp.now(), StopBy::AnswerLost);
        let l = ledger(ledgers, board, i);
        let _ = items::lapse_at(
            &self.ports,
            l,
            table,
            h,
            LapseCause::AnswerLost,
            &self.limits,
            stamp,
        );
        held.retain(|h| h.item() != &item || h.confirmation().state().is_open());
        self.read_back(board, i);
    }

    /// One tick (I5.9): the recovery of I5.8 at the first, the creation move of I5.5, the run
    /// drive's tick, then each setup's owed body (`tick_setup`, dropped at `Closed` or `Gone`),
    /// each held confirmation's time (`items::drive::tick`), whose lapse with `wait_limit` stops
    /// its run, each second step's and each other item's wait limit. After the first step of the
    /// quit a tick does only the stages that start nothing (ruling RD-34 (d)): it sends the rows
    /// of the items' ledgers and makes the run drive's `QuitTick`, which kills each stopping run
    /// and dying tree at its grace with nothing stored and sends what the board owes. While the
    /// hold of a delete of everything stands a tick makes the `QuitTick` alone and sends no row
    /// of the ledgers, since the store is being deleted or cannot be written.
    pub fn tick(&self) {
        let mut st = self.enter(Entry::Tick);
        if Self::holds_tick(&st) {
            if let Some(board) = st.board.as_mut() {
                board.held_tick();
            }
            self.publish(&mut st);
            return;
        }
        let reads = self.reads.load(Ordering::Relaxed);
        if st.quitting || self.quit_mark.load(Ordering::SeqCst) {
            let State { board, ledgers, .. } = &mut *st;
            if let Some(board) = board.as_mut() {
                self.flush_ledgers(board, ledgers);
            }
            let _ = self.act(&mut st, Act::QuitTick);
        } else if self.hold_mark.load(Ordering::SeqCst) {
            let _ = self.act(&mut st, Act::QuitTick);
        } else {
            let before = Self::agents_held_off(&st);
            self.tick_in(&mut st, &before);
            self.finish_switch_offs_after_reads(&mut st, &before);
        }
        if self.reads.load(Ordering::Relaxed) != reads {
            st.mark.tick = false;
        }
        st.returned = true;
        self.deletes_due(&mut st);
        self.publish(&mut st);
    }

    /// The items' part of a tick's stage of what is owed (I7.2): sends the rows that wait in the
    /// ledger of each agent, such as a lapse whose store met a busy owner at the person's stop,
    /// at the runner's end or at the quit, so that the lapse is stored in this life, and reads
    /// back the events of each agent whose rows it sent. A tick sends them after its creation
    /// move and before the run drive's tick, so that a lapse goes before the `run_ended` that
    /// came to be owed after it.
    fn flush_ledgers(&self, board: &mut Board<Cfg<P>>, ledgers: &mut [(AgentId, Ledger)]) {
        for (agent, l) in ledgers.iter_mut() {
            if l.waiting().is_empty() {
                continue;
            }
            if let Some(i) = index(board, agent) {
                l.set_dismissed(board.workflow_dismissed(&board.members[i].facts.workflow.id));
            }
            let _ = items::flush(&self.ports, l, &self.limits);
            if l.refused() {
                board.refused(agent);
            }
            if l.take_acked() {
                board.acknowledged(agent);
            }
            board.hold_place(agent, l.holds_place());
            if let Some(i) = index(board, agent) {
                self.read_back(board, i);
            }
        }
    }

    fn tick_in(&self, st: &mut State<Cfg<P>>, before: &[AgentId]) {
        if st.board.is_none() {
            match run::recover(&self.ports, &self.limits) {
                Ok(life) => st.board = Some(life.board),
                Err(_) => return,
            }
        }
        {
            let State { board, ledgers, .. } = &mut *st;
            let Some(board) = board.as_mut() else { return };
            share_lasts(board, ledgers);
            let created = schedule::drive(&self.ports, board, &self.limits, Move::Create);
            self.noted(created.read);
            self.flush_ledgers(board, ledgers);
        }
        let panicked = {
            let State {
                board,
                ledgers,
                held_off,
                ..
            } = &mut *st;
            board
                .as_mut()
                .is_some_and(|board| self.held_off_at_tick(board, ledgers, held_off))
        };
        if panicked {
            self.held_panic(st);
        }
        self.finish_switch_offs_after_reads(st, before);
        let State {
            board,
            table,
            forgotten,
            ledgers,
            held,
            binds,
            handbacks,
            waits,
            setups,
            ..
        } = st;
        let Some(board) = board.as_mut() else { return };
        share_lasts(board, ledgers);
        let done = run::drive(&self.ports, board, table, &self.limits, Act::Tick);
        self.noted(done.report.read);
        for workflow in &done.gone {
            for m in board
                .members
                .iter()
                .filter(|m| &m.facts.workflow.id == workflow)
            {
                let agent = &m.facts.agent;
                forgotten.push(agent.clone());
                held.retain(|h| h.ask().agent() != agent);
                binds.retain(|b| &b.ask().agent != agent);
                handbacks.retain(|(_, a)| &a.agent != agent);
                waits.retain(|w| &w.agent != agent);
                setups.retain(|s| s.agent() != agent);
                for (a, l) in ledgers.iter_mut() {
                    if a == agent {
                        *l = run::forgotten(workflow.clone(), agent.clone(), l);
                    }
                }
            }
        }
        self.open_setups(st);
        self.tick_waited_setups(st);
        self.tick_setups(st);
        self.tick_confirms(st);
        self.tick_binds(st);
        self.tick_waits(st);
    }

    fn open_setups(&self, st: &mut State<Cfg<P>>) {
        let Some(board) = st.board.as_ref() else {
            return;
        };
        for m in &board.members {
            let f = &m.facts;
            let Some(open) = f.doing.item() else { continue };
            if open.kind != ItemKind::Setup || st.setups.iter().any(|s| s.item() == &open.item) {
                continue;
            }
            let asked: Vec<SetupParam> = params(f.workflow.category)
                .iter()
                .filter(|p| !f.slots.iter().any(|s| s.as_str() == p.name()))
                .map(|p| SetupParam {
                    param: *p,
                    candidates: candidates(*p, &f.workflow),
                })
                .collect();
            if let Ok(setup) = items::open_setup(
                &self.ports,
                open.item.clone(),
                f.workflow.id.clone(),
                f.agent.clone(),
                f.workflow.category,
                asked,
            ) {
                st.setups.push(setup);
            }
        }
    }

    fn tick_setups(&self, st: &mut State<Cfg<P>>) {
        let mut k = 0;
        while k < st.setups.len() {
            let State {
                board,
                ledgers,
                setups,
                ..
            } = &mut *st;
            let done = match board.as_mut() {
                Some(board) => match index(board, setups[k].agent()) {
                    Some(i) => {
                        let l = ledger(ledgers, board, i);
                        let facts = &board.members[i].facts;
                        items::tick_setup(&self.ports, l, &self.limits, &mut setups[k], facts)
                    }
                    None => Err(Cause::State),
                },
                None => return,
            };
            if self.setup_done(st, k, done) {
                k += 1;
            }
        }
    }

    /// Takes what a move of the setup `k` answered; whether the setup stays: never where the move
    /// closed it, as Not now closes an item asked again (r4 U3.22a). Where an item asked again
    /// closed at the owner's acknowledgement, the board reads the agent's rows back, so that the
    /// row holds the `item_answered` that the close stored (r4 U3.22a, the fifth ruling).
    fn setup_done(&self, st: &mut State<Cfg<P>>, k: usize, done: Result<SetupDone, Cause>) -> bool {
        if matches!(done, Ok(SetupDone::Overlap)) {
            let again = self.overlap_once_more(st, k);
            return self.setup_done(st, k, again);
        }
        if matches!(done, Ok(SetupDone::ActiveOverlap)) {
            self.active_overlap(st, k);
        }
        let agent = st.setups[k].agent().clone();
        let answered_again = st.setups[k].is_asked_again()
            && matches!(
                done,
                Ok(SetupDone::Closed { .. } | SetupDone::ActiveOverlap)
            );
        let closed = st.setups[k].state() == &crate::items::SetupState::Closed;
        let stays = !closed
            && (matches!(
                done,
                Ok(SetupDone::Open | SetupDone::Owed | SetupDone::NotTaken)
            ) || matches!(done, Err(cause) if cause != Cause::Stale));
        if stays {
            return true;
        }
        st.setups.remove(k);
        if let Some(board) = st.board.as_mut() {
            if let Some(i) = index(board, &agent) {
                let facts = &mut board.members[i].facts;
                if matches!(facts.doing.item(), Some(o) if o.kind == ItemKind::Setup) {
                    facts.doing = match &facts.doing {
                        Doing::Queued(_) => Doing::Queued(None),
                        _ => Doing::Idle(None),
                    };
                }
                if answered_again {
                    self.read_back(board, i);
                }
            }
        }
        if matches!(done, Ok(SetupDone::Gone | SetupDone::AgentGone)) {
            st.forgotten.push(agent);
        }
        false
    }

    /// What follows a first refusal `overlap` of the body of the setup `k` (I3.5 (b); ruling
    /// RD-67 3.2 (b)): every `paused` that the board owes, then the body once more; the answer of
    /// that send, which is never `Overlap`.
    fn overlap_once_more(&self, st: &mut State<Cfg<P>>, k: usize) -> Result<SetupDone, Cause> {
        let State {
            board,
            ledgers,
            setups,
            ..
        } = &mut *st;
        let board = board.as_mut().ok_or(Cause::State)?;
        schedule::pause_owed(&self.ports, board);
        let i = index(board, setups[k].agent()).ok_or(Cause::State)?;
        let l = ledger(ledgers, board, i);
        let facts = &board.members[i].facts;
        items::again_setup(&self.ports, l, &self.limits, &mut setups[k], facts)
    }

    /// The `active` that the rule of `active` sent after the acknowledged body of the setup `k`
    /// was refused with `overlap`: its agent is marked for the life and gets `paused` (ruling
    /// RD-67 4.3).
    fn active_overlap(&self, st: &mut State<Cfg<P>>, k: usize) {
        let agent = st.setups[k].agent().clone();
        if let Some(board) = st.board.as_mut() {
            if let Some(i) = index(board, &agent) {
                schedule::refused_active(&self.ports, board, i);
            }
        }
    }

    /// After a switch-off of the agent `off` that the person made, the kept body of each setup
    /// whose sentence names that agent, or names nobody, goes once more (I3.5 (e); ruling RD-67
    /// 3.2 (e)), in this move where the owner has acknowledged the `paused` of `off`, else at the
    /// first tick that finds it acknowledged (`tick_waited_setups`; ruling RD-160 (2)).
    fn setups_freed(&self, st: &mut State<Cfg<P>>, off: &AgentId) {
        let paused = st.board.as_ref().is_some_and(|b| off_paused(b, off));
        let mut k = 0;
        while k < st.setups.len() {
            let State {
                board,
                ledgers,
                setups,
                ..
            } = &mut *st;
            let done = match board.as_mut() {
                Some(board) => match index(board, setups[k].agent()) {
                    Some(i) => {
                        let l = ledger(ledgers, board, i);
                        let facts = &board.members[i].facts;
                        items::freed_setup(
                            &self.ports,
                            l,
                            &self.limits,
                            &mut setups[k],
                            facts,
                            off,
                            paused,
                        )
                    }
                    None => Err(Cause::State),
                },
                None => return,
            };
            if self.setup_done(st, k, done) {
                k += 1;
            }
        }
    }

    /// At a tick, the kept body of each setup that waits for the `paused` of the task that the
    /// person switched off goes once more, as `again_setup` sends it, once the owner has
    /// acknowledged that `paused` (`off_paused`); a setup that still waits sends nothing (I3.5
    /// (e); ruling RD-160 (2)).
    fn tick_waited_setups(&self, st: &mut State<Cfg<P>>) {
        let mut k = 0;
        while k < st.setups.len() {
            let State {
                board,
                ledgers,
                setups,
                ..
            } = &mut *st;
            let Some(board) = board.as_mut() else { return };
            if !setups[k].waits().is_some_and(|off| off_paused(board, off)) {
                k += 1;
                continue;
            }
            let done = match index(board, setups[k].agent()) {
                Some(i) => {
                    let l = ledger(ledgers, board, i);
                    let facts = &board.members[i].facts;
                    items::again_setup(&self.ports, l, &self.limits, &mut setups[k], facts)
                }
                None => Err(Cause::State),
            };
            if self.setup_done(st, k, done) {
                k += 1;
            }
        }
    }

    fn tick_confirms(&self, st: &mut State<Cfg<P>>) {
        let State {
            board,
            table,
            ledgers,
            held,
            ..
        } = st;
        let Some(board) = board.as_mut() else { return };
        for h in held.iter_mut() {
            let Some(i) = index(board, h.ask().agent()) else {
                continue;
            };
            let l = ledger(ledgers, board, i);
            let before = (l.last(), l.waiting().len());
            let _ = items::flush(&self.ports, l, &self.limits);
            if let Ok(stamp) = crate::ports::Stamp::read(&self.ports) {
                self.noted(Some(stamp.now()));
                let lapses = matches!(
                    items::due(h, stamp.now(), &self.limits),
                    Some(ConfirmMove::Lapse(LapseCause::WaitLimit))
                );
                if lapses {
                    hold_stop(board, table, h.ask().run(), stamp.now(), StopBy::WaitLimit);
                }
                let l = ledger(ledgers, board, i);
                let _ = items::tick_at(&self.ports, l, table, h, stamp, &self.limits);
            }
            let l = ledger(ledgers, board, i);
            if (l.last(), l.waiting().len()) != before {
                self.read_back(board, i);
            }
        }
        held.retain(|h| h.confirmation().state().is_open());
    }

    /// The wait limit of the items that live runs wait on, other than a confirmation and a second
    /// step (I4.2, I5.7, ruling RD-34 (a)): `wait_limit_s` after its raise an item is timed out by
    /// `items::time_out` and its run is stopped with `wait_limit`, so that it ends `lapsed`. An
    /// item that its run no longer waits on is forgotten.
    fn tick_waits(&self, st: &mut State<Cfg<P>>) {
        let Ok(now) = self.clock_now() else { return };
        let State {
            board,
            table,
            ledgers,
            waits,
            handbacks,
            ..
        } = st;
        let Some(board) = board.as_mut() else { return };
        let limit = self.limits.wait_limit_s.saturating_mul(1000);
        waits.retain_mut(|w| {
            let Some(i) = index(board, &w.agent) else {
                return false;
            };
            let waits_on = matches!(
                &board.members[i].facts.doing,
                Doing::Live(Live::Waiting(open)) if *open == w.item
            );
            if !waits_on {
                return false;
            }
            let at = *w.raised_at.get_or_insert(now);
            if now < at.saturating_add(limit) {
                return true;
            }
            hold_stop(board, table, &w.run, now, StopBy::WaitLimit);
            let l = ledger(ledgers, board, i);
            let _ = items::time_out(&self.ports, l, table, &w.run, &w.item, &self.limits);
            self.read_back(board, i);
            handbacks.retain(|(item, _)| item != &w.item.item);
            false
        });
    }

    fn tick_binds(&self, st: &mut State<Cfg<P>>) {
        let Ok(now) = self.clock_now() else { return };
        let State {
            board,
            table,
            ledgers,
            binds,
            ..
        } = st;
        let Some(board) = board.as_mut() else { return };
        for b in binds.iter_mut() {
            if !items::bind_due(b, now, &self.limits) {
                continue;
            }
            let Some(i) = index(board, &b.ask().agent) else {
                continue;
            };
            hold_stop(board, table, &b.ask().run, now, StopBy::WaitLimit);
            let l = ledger(ledgers, board, i);
            let _ = items::lapse_bind(
                &self.ports,
                l,
                table,
                b,
                OtherLapse::WaitLimit,
                &self.limits,
            );
            self.read_back(board, i);
        }
        binds.retain(|b| *b.state() != items::BindState::Closed);
    }

    fn read_back(&self, board: &mut Board<Cfg<P>>, i: usize) {
        let facts = &mut board.members[i].facts;
        if let Ok(last) = schedule::resync(&self.ports, &facts.agent, &mut facts.events) {
            board.set_last(i, last);
        }
    }

    /// A run's event (I5.10): `hello`; a request, which raises its item through `raise_confirm`
    /// with the guard's table of this life for a confirm request (ruling IG-32), which also ends
    /// the run with `kind` where `items::admits` refuses the request (ruling RD-90 item 6), and
    /// through `raise_step` for any other, and moves the run to `waiting`; the second step of a `sign_in`
    /// item (`Bind`); and the runner's end, before which the run's open item lapses with
    /// `run_ended`. A request that ends the run (`kind`, `budget`) shows no card, stops the run
    /// with that cause and is answered `state`. A second step or a handback `account_link` whose
    /// origin is `http` outside 127.0.0.0/8 is declined to the runner, with no item raised, only
    /// where `raise_step` would raise an item or end the run with `budget`: after its state rule
    /// (a run that is not `working` is refused with `state`) and its kind rule (a second request
    /// while an item of the run is open ends the run with `kind` and raises nothing), and before
    /// its budget rule; it never declines quietly (I1.4, ruling RD-116 R2).
    pub fn on_run(&self, run: &RunId, event: RunEvent) -> Result<(), Cause> {
        let mut st = self.enter(Entry::Move);
        if Self::waits_on_run(&st, run) {
            return Err(Cause::NoService);
        }
        let before = Self::agents_held_off(&st);
        let out = self.on_run_in(&mut st, run, &event);
        self.finish_switch_offs_after_reads(&mut st, &before);
        self.deletes_due(&mut st);
        self.publish(&mut st);
        out
    }

    fn on_run_in(
        &self,
        st: &mut State<Cfg<P>>,
        run: &RunId,
        event: &RunEvent,
    ) -> Result<(), Cause> {
        let board = st.board.as_ref().ok_or(Cause::State)?;
        let agent = board
            .runs
            .iter()
            .find(|r| &r.run == run)
            .map(|r| r.agent.clone())
            .ok_or(Cause::State)?;
        let i = index(board, &agent).ok_or(Cause::State)?;
        match event {
            RunEvent::Hello => self.act(st, Act::Hello { run: run.clone() }),
            RunEvent::Progress(_) => Ok(()),
            RunEvent::Ended(outcome) => {
                let waiting = match &board.members[i].facts.doing {
                    Doing::Live(Live::Waiting(open)) => Some(open.clone()),
                    _ => None,
                };
                if let Some(open) = waiting {
                    self.close_run_item(st, i, run, &open, false);
                    self.act(st, Act::Resume { run: run.clone() })?;
                }
                self.lapse_held(st, i, run, LapseCause::RunEnded);
                st.handbacks.retain(|(_, a)| &a.run != run);
                st.waits.retain(|w| &w.run != run);
                self.end_of_run(st, run, *outcome)
            }
            RunEvent::Confirm(_) | RunEvent::Handback(_) | RunEvent::Bind(_) | RunEvent::Other => {
                let facts = &board.members[i].facts;
                let raise = match event {
                    RunEvent::Confirm(ask) => {
                        match raise_confirm(facts, &st.table, ask, &self.limits) {
                            Raise::Run(_) | Raise::End(RunEnd::Budget)
                                if !admits(ask, &st.table) =>
                            {
                                Raise::End(RunEnd::Kind)
                            }
                            other => other,
                        }
                    }
                    _ => raise_step(facts, Source::of(event).ok_or(Cause::State)?, &self.limits),
                };
                if matches!(raise, Raise::Run(_) | Raise::End(RunEnd::Budget)) {
                    match event {
                        RunEvent::Bind(ask) if ask.origin.insecure() => {
                            return self
                                .ports
                                .answer_bind(run, crate::types::BindResult::Decline)
                                .map_err(|e| cause_of(&e));
                        }
                        RunEvent::Handback(HandbackAsk {
                            reason: crate::types::HandbackReason::AccountLink,
                            origin: Some(origin),
                            ..
                        }) if origin.insecure() => {
                            return items::answer_from_check(
                                &self.ports,
                                run,
                                crate::types::HandbackResult::Declined,
                            );
                        }
                        _ => {}
                    }
                }
                match raise {
                    Raise::Run(RunItemKind::Confirm) => self.issue(st, i, run, event),
                    Raise::Run(RunItemKind::SignIn) if matches!(event, RunEvent::Bind(_)) => {
                        self.raise_bind(st, i, run, event)
                    }
                    Raise::Run(kind) => {
                        let State { board, ledgers, .. } = st;
                        let Some(board) = board.as_mut() else {
                            return Err(Cause::State);
                        };
                        let l = ledger(ledgers, board, i);
                        let mut reading = Reading::new(&self.ports, board, i);
                        let raised = items::raise_run_item_reading(
                            &self.ports,
                            l,
                            &self.limits,
                            run,
                            kind,
                            &mut |l| reading.read(l),
                        );
                        let (made, stopped) = (reading.made, reading.stopped);
                        if stopped {
                            return self.stopped_at_raise(st, i);
                        }
                        let open = raised?;
                        if !made {
                            self.read_back(board, i);
                        }
                        st.waits.push(Waited {
                            run: run.clone(),
                            agent: agent.clone(),
                            item: open.clone(),
                            raised_at: self.clock_now().ok(),
                        });
                        if let RunEvent::Handback(ask) = event {
                            st.handbacks.push((open.item.clone(), ask.clone()));
                        }
                        self.act(
                            st,
                            Act::Wait {
                                run: run.clone(),
                                item: open,
                            },
                        )
                    }
                    Raise::Answer(result) => items::answer_from_check(&self.ports, run, result),
                    Raise::Nothing => Ok(()),
                    Raise::Refused(cause) => Err(cause),
                    Raise::End(end) => self.end_for_request(st, i, run, end),
                    Raise::Own(_) => Err(Cause::State),
                }
            }
        }
    }

    fn issue(
        &self,
        st: &mut State<Cfg<P>>,
        i: usize,
        run: &RunId,
        event: &RunEvent,
    ) -> Result<(), Cause> {
        let RunEvent::Confirm(ask) = event else {
            return Err(Cause::State);
        };
        let State {
            board,
            table,
            ledgers,
            held,
            ..
        } = st;
        let Some(board) = board.as_mut() else {
            return Err(Cause::State);
        };
        let l = ledger(ledgers, board, i);
        let mut reading = Reading::new(&self.ports, board, i);
        let issued = items::issue_reading(&self.ports, l, table, &self.limits, ask, &mut |l| {
            reading.read(l)
        });
        let (made, stopped) = (reading.made, reading.stopped);
        if stopped {
            return self.stopped_at_raise(st, i);
        }
        let h = issued?;
        if !made {
            self.read_back(board, i);
        }
        if !h.confirmation().state().is_open() {
            return Ok(());
        }
        let item = h.item().clone();
        held.push(h);
        self.act(
            st,
            Act::Wait {
                run: run.clone(),
                item: RunItem {
                    item,
                    kind: RunItemKind::Confirm,
                },
            },
        )
    }

    fn raise_bind(
        &self,
        st: &mut State<Cfg<P>>,
        i: usize,
        run: &RunId,
        event: &RunEvent,
    ) -> Result<(), Cause> {
        let RunEvent::Bind(ask) = event else {
            return Err(Cause::State);
        };
        let State {
            board,
            table,
            ledgers,
            binds,
            ..
        } = st;
        let Some(board) = board.as_mut() else {
            return Err(Cause::State);
        };
        let l = ledger(ledgers, board, i);
        let mut reading = Reading::new(&self.ports, board, i);
        let raised =
            items::raise_bind_reading(&self.ports, l, table, &self.limits, ask, &mut |l| {
                reading.read(l)
            });
        let (made, stopped) = (reading.made, reading.stopped);
        if stopped {
            return self.stopped_at_raise(st, i);
        }
        let b = raised?;
        if !made {
            self.read_back(board, i);
        }
        let item = b.item().clone();
        binds.push(b);
        self.act(
            st,
            Act::Wait {
                run: run.clone(),
                item: RunItem {
                    item,
                    kind: RunItemKind::SignIn,
                },
            },
        )
    }

    /// Ends the run of a request that `raise_confirm` or `raise_step` answered with the end `end`
    /// (I3.1, I3.3, ruling IG-32): the run's open item and every item of the run that the
    /// coordinator holds lapse with `run_ended`, and the run drive stops the run with the cause
    /// `kind` or `budget`, which sends `cancel` once and kills the run at the first tick after
    /// `cancel_grace_s`, as every stop; the request shows no card and is answered `state`.
    fn end_for_request(
        &self,
        st: &mut State<Cfg<P>>,
        i: usize,
        run: &RunId,
        end: RunEnd,
    ) -> Result<(), Cause> {
        let by = match end {
            RunEnd::Kind => StopBy::Kind,
            RunEnd::Budget => StopBy::Budget,
            _ => return Err(Cause::State),
        };
        let waiting = st
            .board
            .as_ref()
            .and_then(|b| match &b.members[i].facts.doing {
                Doing::Live(Live::Waiting(open)) => Some(open.clone()),
                _ => None,
            });
        if let Some(open) = waiting {
            self.close_run_item(st, i, run, &open, false);
        }
        self.lapse_held(st, i, run, LapseCause::RunEnded);
        self.lapse_binds(st, i, run, OtherLapse::RunEnded);
        st.handbacks.retain(|(_, a)| &a.run != run);
        st.waits.retain(|w| &w.run != run);
        self.act(
            st,
            Act::Stop {
                run: run.clone(),
                by,
            },
        )?;
        Err(Cause::State)
    }

    fn act(&self, st: &mut State<Cfg<P>>, act: Act) -> Result<(), Cause> {
        let State {
            board,
            table,
            ledgers,
            ..
        } = st;
        let start = match &act {
            Act::Start { agent } => Some(agent.clone()),
            _ => None,
        };
        let board = board.as_mut().ok_or(Cause::State)?;
        share_lasts(board, ledgers);
        let done: Done = run::drive(&self.ports, board, table, &self.limits, act);
        self.noted(done.report.read);
        if let (Some(agent), Some(shared)) = (start, done.report.shared.clone()) {
            st.refused_on.retain(|(a, _)| a != &agent);
            st.refused_on.push((agent, shared));
        }
        answered(&done.report)
    }

    /// Takes off the stop that a kept end put on the board for its run before the end is made,
    /// so that the end is made as one that no stop preceded: with the runner's outcome, no
    /// dying tree and no kill.
    fn unmark(&self, st: &mut State<Cfg<P>>, run: &RunId) {
        let before = st.marked.len();
        st.marked.retain(|r| r != run);
        if st.marked.len() == before {
            return;
        }
        if let Some(r) = st
            .board
            .as_mut()
            .and_then(|b| b.runs.iter_mut().find(|r| &r.run == run))
        {
            r.stop = None;
        }
    }

    /// Keeps the read of the clock that stamped a move as the last time that this life read
    /// (ruling RD-133 1, (b)).
    fn noted(&self, read: Option<u64>) {
        if let Some(now) = read {
            self.last_read.store(now, Ordering::Relaxed);
            self.reads.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// One read of the app's clock, noted as the last read of this life where it answers.
    fn clock_now(&self) -> Result<u64, PortError> {
        let now = self.ports.now();
        self.noted(now.as_ref().ok().copied());
        now
    }

    /// The last read of the clock of this life as a stamp, where there was one.
    fn last_stamp(&self) -> Option<crate::ports::Stamp> {
        let at = self.last_read.load(Ordering::Relaxed);
        if at == NO_READ {
            return None;
        }
        crate::ports::Stamp::read(&LastRead(&self.ports, at)).ok()
    }

    /// The runner's end through the run drive. Where no read of the clock answers, the end is
    /// kept and made again at the next read that answers, so that a run that a person stopped
    /// ends with the person's cause and no later kill ends it `interrupted` (ruling RD-133 1).
    /// A kept end of a run that no stop reached puts a stop with the runner's own cause on the
    /// board at once, stamped at that read, so that no Stop, switch-off or quit stops a run
    /// that ended and its end keeps the runner's outcome.
    fn end_of_run(
        &self,
        st: &mut State<Cfg<P>>,
        run: &RunId,
        outcome: Option<crate::types::Outcome>,
    ) -> Result<(), Cause> {
        self.unmark(st, run);
        let State {
            board,
            table,
            ledgers,
            ..
        } = st;
        let board = board.as_mut().ok_or(Cause::State)?;
        share_lasts(board, ledgers);
        let act = Act::End {
            run: run.clone(),
            outcome,
        };
        let done: Done = run::drive(&self.ports, board, table, &self.limits, act);
        self.noted(done.report.read);
        if !done.report.unstamped {
            st.ended.retain(|(r, _)| r != run);
        }
        if done.report.unstamped {
            self.keep_end(st, run, outcome);
        }
        answered(&done.report)
    }

    /// The `run_ended` that a kept end makes at `stamp`, as a debt: where its store panics after
    /// the run left the board, the board owes it and a later tick pays it, as for a store that
    /// refused it (ruling RD-140 R1).
    fn owing_end(
        st: &State<Cfg<P>>,
        run: &RunId,
        outcome: Option<crate::types::Outcome>,
        stamp: crate::ports::Stamp,
    ) -> Option<schedule::Owed> {
        let r = st.board.as_ref()?.runs.iter().find(|r| &r.run == run)?;
        let ended = outcome.map_or(crate::types::RunEnd::Interrupted, Into::into);
        let cause = match r.stop {
            Some(stop) if !st.marked.contains(run) => stop.cause,
            _ => ended,
        };
        Some(schedule::Owed {
            agent: r.agent.clone(),
            workflow: r.workflow.clone(),
            what: schedule::Owing::RunEnded {
                run: run.clone(),
                cause,
                day: stamp.day(),
                at: stamp.at(),
            },
        })
    }

    /// Keeps a runner's end for the next read that answers, and, for a run that no stop reached,
    /// puts a stop with the runner's own cause on the board, so that no Stop, switch-off or quit
    /// stops a run that ended.
    fn keep_end(
        &self,
        st: &mut State<Cfg<P>>,
        run: &RunId,
        outcome: Option<crate::types::Outcome>,
    ) {
        if !st.ended.iter().any(|(r, _)| r == run) {
            st.ended.push((run.clone(), outcome));
        }
        let marked = st.board.as_mut().is_some_and(|board| {
            board
                .runs
                .iter_mut()
                .find(|r| &r.run == run && r.stop.is_none())
                .is_some_and(|r| {
                    let cause = outcome.map_or(crate::types::RunEnd::Interrupted, Into::into);
                    r.stop = Some(schedule::Stop { at: 0, cause });
                    true
                })
        });
        if marked && !st.marked.contains(run) {
            st.marked.push(run.clone());
        }
    }

    fn close_run_item(
        &self,
        st: &mut State<Cfg<P>>,
        i: usize,
        run: &RunId,
        open: &RunItem,
        stop: bool,
    ) {
        let State {
            board,
            table,
            ledgers,
            held,
            binds,
            handbacks,
            answering,
            ..
        } = st;
        let Some(board) = board.as_mut() else { return };
        let mut parts = LapseParts {
            table,
            ledgers,
            held,
            binds,
            handbacks,
            answering,
        };
        self.lapse_open(board, &mut parts, i, run, open, stop);
    }

    fn lapse_open(
        &self,
        board: &mut Board<Cfg<P>>,
        parts: &mut LapseParts<'_>,
        i: usize,
        run: &RunId,
        open: &RunItem,
        stop: bool,
    ) {
        let LapseParts {
            table,
            ledgers,
            held,
            binds,
            handbacks,
            answering,
        } = parts;
        let l = ledger(ledgers, board, i);
        let cause = if stop {
            OtherLapse::Cancelled
        } else {
            OtherLapse::RunEnded
        };
        if let Some(b) = binds.iter_mut().find(|b| b.item() == &open.item) {
            let _ = items::lapse_bind(&self.ports, l, table, b, cause, &self.limits);
            binds.retain(|b| *b.state() != items::BindState::Closed);
        } else if open.kind == RunItemKind::Confirm {
            if let Some(h) = held.iter_mut().find(|h| h.item() == &open.item) {
                let lost = answering.contains(&open.item)
                    && matches!(h.confirmation().state(), ConfirmState::Answered(_));
                if stop && lost {
                    answering.retain(|a| a != &open.item);
                    let _ = items::lapse(
                        &self.ports,
                        l,
                        table,
                        h,
                        LapseCause::AnswerLost,
                        &self.limits,
                    );
                } else if stop {
                    let _ = items::stop_confirm(&self.ports, l, table, h, &self.limits);
                } else {
                    let _ =
                        items::lapse(&self.ports, l, table, h, LapseCause::RunEnded, &self.limits);
                }
            }
            held.retain(|h| h.item() != &open.item || h.confirmation().state().is_open());
        } else {
            let _ = items::lapse_item(&self.ports, l, table, run, open, cause, &self.limits);
            handbacks.retain(|(item, _)| item != &open.item);
        }
        self.read_back(board, i);
    }

    fn switch_off(&self, st: &mut State<Cfg<P>>, i: usize, mv: Move) -> Result<(), Cause> {
        let State {
            board,
            table,
            ledgers,
            held,
            binds,
            handbacks,
            waits,
            answering,
            ..
        } = st;
        let board = board.as_mut().ok_or(Cause::NoService)?;
        share_lasts(board, ledgers);
        let open = waiting_on(&board.members[i].facts.doing);
        let mut parts = LapseParts {
            table,
            ledgers,
            held,
            binds,
            handbacks,
            answering,
        };
        let mut halt = |board: &mut Board<Cfg<P>>, i: usize, run: &RunId| {
            *parts.table = parts.table.stop(run);
            let Some(open) = &open else { return false };
            self.lapse_open(board, &mut parts, i, run, open, true);
            true
        };
        let report = schedule::drive_with(&self.ports, board, &self.limits, mv, Some(&mut halt));
        self.noted(report.read);
        if report.unstamped {
            return self.switch_off_unstamped(st, i);
        }
        waits.retain(|w| !report.stopped.contains(&w.run));
        answered(&report)
    }

    /// The person's switch-off when no read of the clock answers (ruling RD-133 1), and so the
    /// rest of a switch-off that a move's own read of the owner's events showed beside a live run
    /// (RD-52 (b) rule 3) when that move's `Move::Off` finds no read (adversary 3's a/02): the
    /// agent's live run is stopped on the guard's table and the board at once and its open item
    /// lapses with `cancelled`, `Runs::cancel` goes out in this move, and its queued start
    /// leaves the queue; the run's stop time, the rows and the rest of the switch-off wait for
    /// the next read that answers (`stamp_waiting`), and meanwhile the agent takes no start
    /// (`hold`) and no switch-on (`no_service`). The `hold_set` is stored at once at the last
    /// read of the clock of this life (`last_stamp`), so that the agent stays off across a new
    /// life; its time and day may be older than the press by as long as the clock failed
    /// (answer (b)). Named limits: with no read of a move noted yet in this life (recovery's own
    /// read does not count), where the day of the last read cannot be read, or where the owner
    /// does not acknowledge the `hold_set`, the hold is kept only in this life's state until the
    /// next read that answers, and a new life before it loses the hold. In the window it refuses
    /// what a stamped switch-off refuses for a reason other than the clock: an agent set aside
    /// (`no_service`), one whose dismissed workflow's events were not read, or one already off
    /// with no run to stop (`state`). The order is the root (ruling RD-138 (1)): first what
    /// cannot fail, the stop on the guard's table and the board at time 0, the queue and the list
    /// of switch-offs; then the `hold_set`'s store, whose panic is held until the cancel went out;
    /// then `Runs::cancel`, sent here or by the lapse of the run's open item. A cancel that does
    /// not return so loses neither the hold nor the stop's grace, and a refused store does not
    /// refuse the press.
    fn switch_off_unstamped(&self, st: &mut State<Cfg<P>>, i: usize) -> Result<(), Cause> {
        let Some(board) = st.board.as_mut() else {
            return Err(Cause::NoService);
        };
        let agent = board.members[i].facts.agent.clone();
        if board.aside(&agent) {
            return Err(Cause::NoService);
        }
        if board.dismissed_unread(&agent) {
            return Err(Cause::State);
        }
        let pending = st
            .ledgers
            .iter()
            .any(|(a, l)| a == &agent && l.waits_with_a_persons_hold());
        let standing = pending
            || st.offs.contains(&agent)
            || schedule::holds(&board.members[i].facts).contains(&schedule::Hold::Off);
        let live =
            live_unstopped_run(board, &agent).filter(|run| !st.ended.iter().any(|(r, _)| r == run));
        if standing && live.is_none() {
            return Err(Cause::State);
        }
        let open = waiting_on(&board.members[i].facts.doing);
        if let Some(run) = &live {
            run::stop_unstamped(board, &mut st.table, run, StopBy::Person);
        }
        if board.queue.remove(&agent) {
            let facts = &mut board.members[i].facts;
            if let Doing::Queued(item) = &facts.doing {
                facts.doing = Doing::Idle(item.clone());
            }
        }
        if !st.offs.contains(&agent) {
            st.offs.push(agent);
        }
        let store = if standing { None } else { self.last_stamp() };
        let stored = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if let (Some(at), Some(board)) = (store, st.board.as_mut()) {
                share_lasts(board, &mut st.ledgers);
                let l = ledger(&mut st.ledgers, board, i);
                let _ = items::store_held_off(&self.ports, l, &self.limits, at);
                self.read_back(board, i);
            }
        }));
        if let Some(run) = live {
            match &open {
                Some(open) => self.close_run_item(st, i, &run, open, true),
                None => run::send_cancel(&self.ports, &run),
            }
            st.waits.retain(|w| w.run != run);
        }
        if let Err(unwind) = stored {
            std::panic::resume_unwind(unwind);
        }
        Ok(())
    }

    fn stopped_at_raise(&self, st: &mut State<Cfg<P>>, i: usize) -> Result<(), Cause> {
        let agent = st
            .board
            .as_ref()
            .map(|b| b.members[i].facts.agent.clone())
            .ok_or(Cause::State)?;
        self.switch_off(st, i, Move::Off { agent })?;
        Err(Cause::State)
    }

    fn agents_held_off(st: &State<Cfg<P>>) -> Vec<AgentId> {
        st.board.as_ref().map_or_else(Vec::new, |b| {
            b.members
                .iter()
                .filter(|m| schedule::holds(&m.facts).contains(&schedule::Hold::Off))
                .map(|m| m.facts.agent.clone())
                .collect()
        })
    }

    fn finish_switch_offs_after_reads(&self, st: &mut State<Cfg<P>>, before: &[AgentId]) {
        for agent in Self::agents_held_off(st) {
            let Some(board) = st.board.as_ref() else {
                return;
            };
            let (Some(i), false) = (index(board, &agent), before.contains(&agent)) else {
                continue;
            };
            if live_unstopped_run(board, &agent).is_some() || board.queue.holds(&agent) {
                let _ = self.switch_off(st, i, Move::Off { agent });
            }
        }
    }

    /// Lapses with `cause` every second step of `run` that the coordinator still holds, one whose
    /// row is stored among them, and forgets them.
    fn lapse_binds(&self, st: &mut State<Cfg<P>>, i: usize, run: &RunId, cause: OtherLapse) {
        let State {
            board,
            table,
            ledgers,
            binds,
            ..
        } = st;
        let Some(board) = board.as_mut() else { return };
        let l = ledger(ledgers, board, i);
        for b in binds.iter_mut().filter(|b| &b.ask().run == run) {
            let _ = items::lapse_bind(&self.ports, l, table, b, cause, &self.limits);
        }
        binds.retain(|b| &b.ask().run != run);
        self.read_back(board, i);
    }

    /// Lapses with `cause` every confirmation of `run` that the coordinator still holds open,
    /// a recorded one of a stopped run among them, and forgets them.
    fn lapse_held(&self, st: &mut State<Cfg<P>>, i: usize, run: &RunId, cause: LapseCause) {
        let State {
            board,
            table,
            ledgers,
            held,
            answering,
            ..
        } = st;
        let Some(board) = board.as_mut() else { return };
        let l = ledger(ledgers, board, i);
        let settles = |h: &Held| answering.contains(h.item());
        for h in held
            .iter_mut()
            .filter(|h| h.ask().run() == run && !settles(h))
        {
            if h.confirmation().state().is_open() {
                let _ = items::lapse(&self.ports, l, table, h, cause, &self.limits);
            }
        }
        held.retain(|h| h.ask().run() != run || settles(h));
        self.read_back(board, i);
    }

    /// The lapse at the app's quit of every open item of the agent `i` with `app_ended`, in the
    /// life that ends (I4.5): its held confirmations, the item of its waiting run, and an item
    /// that no run raised, which then leaves its facts.
    fn lapse_at_quit(&self, st: &mut State<Cfg<P>>, i: usize) {
        let Some(board) = st.board.as_ref() else {
            return;
        };
        let facts = &board.members[i].facts;
        let doing = facts.doing.clone();
        let runs: Vec<RunId> = board
            .runs
            .iter()
            .filter(|r| r.agent == facts.agent)
            .map(|r| r.run.clone())
            .collect();
        let bind_items: Vec<ItemId> = st
            .binds
            .iter()
            .filter(|b| runs.contains(&b.ask().run))
            .map(|b| b.item().clone())
            .collect();
        for run in &runs {
            self.lapse_held(st, i, run, LapseCause::AppEnded);
            self.lapse_binds(st, i, run, OtherLapse::AppEnded);
        }
        let State {
            board,
            table,
            ledgers,
            setups,
            ..
        } = st;
        let Some(board) = board.as_mut() else { return };
        let l = ledger(ledgers, board, i);
        match &doing {
            Doing::Live(Live::Waiting(open))
                if open.kind != RunItemKind::Confirm && !bind_items.contains(&open.item) =>
            {
                if let Some(run) = runs.first() {
                    let _ = items::lapse_item(
                        &self.ports,
                        l,
                        table,
                        run,
                        open,
                        OtherLapse::AppEnded,
                        &self.limits,
                    );
                }
            }
            Doing::Idle(Some(own)) | Doing::Queued(Some(own)) => {
                items::lapse_own(&self.ports, l, &self.limits, own, OtherLapse::AppEnded);
                setups.retain(|s| s.item() != &own.item);
                let facts = &mut board.members[i].facts;
                facts.doing = match facts.doing {
                    Doing::Queued(_) => Doing::Queued(None),
                    _ => Doing::Idle(None),
                };
            }
            _ => {}
        }
        self.read_back(board, i);
    }

    /// The person's answer to an item (I8.1 `agents_answer`): a confirmation's `allow` or
    /// `reject` from the page, a handback's choice, after which the run resumes, and the choices
    /// of the items that no run raises, through the schedule's moves and the setup flow. A
    /// `choose` names its parameter and its candidate. `step` is the step mark of the item that
    /// the page showed: an answer whose step is no longer the item's current one is refused with
    /// `state`, a decline among them, and nothing is stored or sent (I3.4, ruling RD-28 (k)).
    /// While the board holds stuck the record of the agent whose row shows an item that a run
    /// raised, the agent of that run, a press of any choice on that item, from a page that drew
    /// it before, answers `no_service` and is not taken: nothing of it is armed, stored or sent
    /// to the run, and the item stays as it was, with its own time, and shows again once the
    /// record can be saved. That holds for a confirmation, the second step of a sign-in and every
    /// handback alike; the items that no run raises are answered as before. Such a refused press
    /// ends as every other refusal of this move ends: what the entry of the move took up is
    /// published (rulings RD-147 (5)(ii), RD-159, RD-164).
    pub fn answer(
        &self,
        item: &ItemId,
        choice: Choice,
        pick: Option<(Param, u32)>,
        step: u32,
    ) -> Result<(), Cause> {
        self.answer_of_page(item, choice, pick, step, false)
    }

    /// The choice `choose` of `agents_answer` whose call says that more calls of its press follow
    /// (I8.1 `more`; r4 U3.30): `answer` with that choice and the candidate `n` of `param`, but
    /// the set-up item takes the candidate and makes no body, also where every question then
    /// holds an answer.
    pub fn choose_more(&self, item: &ItemId, param: Param, n: u32, step: u32) -> Result<(), Cause> {
        self.answer_of_page(item, Choice::Choose, Some((param, n)), step, true)
    }

    fn answer_of_page(
        &self,
        item: &ItemId,
        choice: Choice,
        pick: Option<(Param, u32)>,
        step: u32,
        more: bool,
    ) -> Result<(), Cause> {
        let now = self.clock_now().ok();
        let mut st = self.enter(Entry::Move);
        if Self::waits_on_item(&st, item) {
            return Err(Cause::NoService);
        }
        let before = Self::agents_held_off(&st);
        let out = if self.withdrawn(&st, item) {
            Err(Cause::NoService)
        } else {
            let by = AnsweredBy {
                step: Some(step),
                from: Presenter::Page,
                now,
                more,
            };
            self.answer_in(&mut st, item, choice, pick, by)
        };
        self.finish_switch_offs_after_reads(&mut st, &before);
        self.publish(&mut st);
        out
    }

    /// The answer of the native dialog of a confirmation (I4.5), whose item keeps its one step.
    /// While the board holds stuck the record of the agent whose row shows the item, the agent of
    /// the run that raised it, a press of either control answers `no_service` and is not taken:
    /// nothing of it is armed, stored or sent to the run, and the item stays held with its own
    /// time, which `present` offers again once the record can be saved. Such a refused press ends
    /// as every other refusal of this move ends: what the entry of the move took up is published
    /// (rulings RD-147 (5)(ii), RD-164).
    pub fn answer_native(&self, item: &ItemId, answer: ConfirmAnswer) -> Result<(), Cause> {
        let choice = match answer {
            ConfirmAnswer::Allow => Choice::Allow,
            ConfirmAnswer::Reject => Choice::Reject,
        };
        let now = self.clock_now().ok();
        let mut st = self.enter(Entry::Move);
        if Self::waits_on_item(&st, item) {
            return Err(Cause::NoService);
        }
        let before = Self::agents_held_off(&st);
        let out = if st
            .held
            .iter()
            .find(|h| h.item() == item)
            .is_some_and(|h| self.withdrawn(&st, h.item()))
        {
            Err(Cause::NoService)
        } else {
            let by = AnsweredBy {
                step: None,
                from: Presenter::Native,
                now,
                more: false,
            };
            self.answer_in(&mut st, item, choice, None, by)
        };
        self.finish_switch_offs_after_reads(&mut st, &before);
        self.publish(&mut st);
        out
    }

    fn step_of(st: &State<Cfg<P>>, item: &ItemId) -> u32 {
        st.setups
            .iter()
            .find(|s| s.item() == item)
            .map_or(0, Setup::step)
    }

    fn owner_of(&self, st: &State<Cfg<P>>, item: &ItemId) -> Result<(usize, Doing), Cause> {
        let board = st.board.as_ref().ok_or(Cause::UnknownItem)?;
        board
            .members
            .iter()
            .position(|m| m.facts.doing.item().is_some_and(|o| &o.item == item))
            .map(|i| (i, board.members[i].facts.doing.clone()))
            .ok_or(Cause::UnknownItem)
    }

    fn answer_in(
        &self,
        st: &mut State<Cfg<P>>,
        item: &ItemId,
        choice: Choice,
        pick: Option<(Param, u32)>,
        by: AnsweredBy,
    ) -> Result<(), Cause> {
        let (i, doing) = self.owner_of(st, item)?;
        let step = by.step.unwrap_or_else(|| Self::step_of(st, item));
        if step != Self::step_of(st, item) {
            return Err(Cause::State);
        }
        let agent = st.board.as_ref().map(|b| b.members[i].facts.agent.clone());
        let agent = agent.ok_or(Cause::UnknownItem)?;
        match doing {
            Doing::Live(Live::Waiting(open)) => {
                let run = st
                    .board
                    .as_ref()
                    .and_then(|b| b.runs.iter().find(|r| r.agent == agent))
                    .map(|r| r.run.clone())
                    .ok_or(Cause::State)?;
                if open.kind == RunItemKind::Confirm {
                    self.confirm_answer(st, i, &run, item, choice, by)
                } else if st.binds.iter().any(|b| b.item() == item) {
                    self.bind_answer(st, i, &run, item, choice)
                } else {
                    let State {
                        board,
                        ledgers,
                        handbacks,
                        ..
                    } = st;
                    let board = board.as_mut().ok_or(Cause::State)?;
                    let ask = handbacks
                        .iter()
                        .find(|(i, _)| i == item)
                        .map(|(_, a)| a.clone())
                        .ok_or(Cause::State)?;
                    let l = ledger(ledgers, board, i);
                    let mut reading = Reading::new(&self.ports, board, i);
                    let out = items::answer_handback_reading(
                        &self.ports,
                        l,
                        &self.limits,
                        item,
                        &ask,
                        choice,
                        &mut |l| reading.read(l),
                    );
                    let (made, stopped) = (reading.made, reading.stopped);
                    if !made {
                        self.read_back(board, i);
                    }
                    if stopped {
                        self.switch_off(st, i, Move::Off { agent })?;
                        return Err(Cause::UnknownItem);
                    }
                    match out {
                        Ok(()) => {}
                        Err(Cause::UnknownItem) => {
                            handbacks.retain(|(i, _)| i != item);
                            self.act(st, Act::Resume { run })?;
                            return Err(Cause::UnknownItem);
                        }
                        Err(e) => return Err(e),
                    }
                    handbacks.retain(|(i, _)| i != item);
                    self.act(st, Act::Resume { run })
                }
            }
            Doing::Idle(Some(own)) | Doing::Queued(Some(own)) => match (own.kind, choice) {
                (OwnItemKind::Ready, Choice::Start) => self.start_in(st, &agent),
                (OwnItemKind::Ready, Choice::Skip) => self.schedule(st, Move::Skip { agent }),
                (OwnItemKind::Check, Choice::TryAgain | Choice::WentThrough) => {
                    let answer = CheckAnswer::parse(choice.name())?;
                    self.schedule(st, Move::Check { agent, answer })
                }
                (OwnItemKind::Amounts, Choice::SwitchOff | Choice::Seen) => {
                    let answer = AmountsAnswer::parse(choice.name())?;
                    self.schedule(st, Move::Amounts { agent, answer })
                }
                (OwnItemKind::Setup, Choice::None) => {
                    self.setup(st, item, SetupInput::None, step, false)
                }
                (OwnItemKind::Setup, Choice::Choose) => {
                    let (param, n) = pick.ok_or(Cause::Malformed)?;
                    self.setup(st, item, SetupInput::Choose { param, n }, step, by.more)
                }
                _ => Err(Cause::Malformed),
            },
            _ => Err(Cause::UnknownItem),
        }
    }

    fn confirm_answer(
        &self,
        st: &mut State<Cfg<P>>,
        i: usize,
        run: &RunId,
        item: &ItemId,
        choice: Choice,
        by: AnsweredBy,
    ) -> Result<(), Cause> {
        let answer = match choice {
            Choice::Allow => ConfirmAnswer::Allow,
            Choice::Reject => ConfirmAnswer::Reject,
            _ => return Err(Cause::Malformed),
        };
        let State {
            board,
            table,
            ledgers,
            held,
            answering,
            ..
        } = st;
        let board = board.as_mut().ok_or(Cause::State)?;
        let h = held
            .iter_mut()
            .find(|h| h.item() == item)
            .ok_or(Cause::Stale)?;
        if let (Some(now), true) = (by.now, h.confirmation().presenter() == by.from) {
            items::arm_at(h, now, &self.limits);
        }
        let agent = board.members[i].facts.agent.clone();
        let l = ledger(ledgers, board, i);
        let mut reading = Reading::new(&self.ports, board, i);
        if !answering.contains(item) {
            answering.push(item.clone());
        }
        let out = items::answer_reading(
            &self.ports,
            l,
            table,
            h,
            answer,
            by.from,
            &self.limits,
            &mut |l| reading.read(l),
        );
        answering.retain(|a| a != item);
        let stopped = reading.stopped;
        let closed = !h.confirmation().state().is_open();
        self.read_back(board, i);
        if stopped {
            self.switch_off(st, i, Move::Off { agent })?;
            return Err(Cause::UnknownItem);
        }
        if closed {
            held.retain(|h| h.item() != item);
            self.act(st, Act::Resume { run: run.clone() })?;
        }
        out
    }

    fn bind_answer(
        &self,
        st: &mut State<Cfg<P>>,
        i: usize,
        run: &RunId,
        item: &ItemId,
        choice: Choice,
    ) -> Result<(), Cause> {
        let State {
            board,
            table,
            ledgers,
            binds,
            ..
        } = st;
        let board = board.as_mut().ok_or(Cause::State)?;
        let b = binds
            .iter_mut()
            .find(|b| b.item() == item)
            .ok_or(Cause::Stale)?;
        let agent = board.members[i].facts.agent.clone();
        let l = ledger(ledgers, board, i);
        let mut reading = Reading::new(&self.ports, board, i);
        let out =
            items::answer_bind_reading(&self.ports, l, table, &self.limits, b, choice, &mut |l| {
                reading.read(l)
            });
        let stopped = reading.stopped;
        let closed = *b.state() == items::BindState::Closed;
        self.read_back(board, i);
        if stopped {
            self.switch_off(st, i, Move::Off { agent })?;
            return Err(Cause::UnknownItem);
        }
        if closed {
            binds.retain(|b| b.item() != item);
            self.act(st, Act::Resume { run: run.clone() })?;
        }
        out
    }

    fn setup(
        &self,
        st: &mut State<Cfg<P>>,
        item: &ItemId,
        input: SetupInput<'_>,
        step: u32,
        more: bool,
    ) -> Result<(), Cause> {
        let k = st
            .setups
            .iter()
            .position(|s| s.item() == item)
            .ok_or(Cause::Stale)?;
        if st.setups[k].step() != step {
            return Err(Cause::State);
        }
        let State {
            board,
            ledgers,
            setups,
            ..
        } = &mut *st;
        let board = board.as_mut().ok_or(Cause::State)?;
        let i = index(board, setups[k].agent()).ok_or(Cause::State)?;
        let l = ledger(ledgers, board, i);
        let facts = &board.members[i].facts;
        let setup = &mut setups[k];
        let done = if more {
            items::answer_setup_more(&self.ports, l, &self.limits, setup, facts, input)
        } else {
            items::answer_setup(&self.ports, l, &self.limits, setup, facts, input)
        };
        let out = done.map(|_| ());
        self.setup_done(st, k, done);
        out
    }

    /// The text of a parameter of a setup item that has no candidate (I8.1
    /// `agents_setup_typed`), at the step mark `step` that the page showed, which is refused with
    /// `state` where it is no longer the item's (I3.4). Where the text is a line of Which
    /// spreadsheet file that an open item asking Which sheet takes and that differs from the line
    /// that it holds (`Setup::sheets_due`), the names of that workbook's worksheets are read first
    /// through `Files::sheets`, with the coordinator's lock released as `setup_choose` releases
    /// it, and the move then takes the line with the names that the read answered, none where it
    /// failed (`SetupInput::Book`; r4 U3.22c). The move finds the item again under the lock, as
    /// every move does: another line, a lapse or another step mark may stand by then. This is the
    /// call that does not say `more`: where the line leaves no question without an answer, the
    /// one body is made in this move.
    pub fn setup_typed(
        &self,
        item: &ItemId,
        param: Param,
        text: &str,
        step: u32,
    ) -> Result<(), Cause> {
        self.typed_line(item, param, text, step, false)
    }

    /// `setup_typed` of a call that says that more calls of its press follow (I8.1 `more`; r4
    /// U3.30): the line is taken, not taken or refused as `setup_typed` does it, the names of a
    /// workbook's worksheets are read as there, and NO body is made, also where every question
    /// then holds a line. So the body of a set-up holds the answers of ONE press: the call that
    /// does not say `more` makes it, behind every other call of that press.
    pub fn setup_typed_more(
        &self,
        item: &ItemId,
        param: Param,
        text: &str,
        step: u32,
    ) -> Result<(), Cause> {
        self.typed_line(item, param, text, step, true)
    }

    fn typed_line(
        &self,
        item: &ItemId,
        param: Param,
        text: &str,
        step: u32,
        more: bool,
    ) -> Result<(), Cause> {
        let due = {
            let st = self.lock();
            st.setups
                .iter()
                .find(|s| s.item() == item && s.step() == step)
                .and_then(|s| s.sheets_due(param, text))
        };
        let read = due.map(|line| self.ports.sheets(&line).ok());
        let input = match &read {
            Some(sheets) => SetupInput::Book {
                text,
                sheets: sheets.as_deref(),
            },
            None => SetupInput::Typed { param, text },
        };
        let mut st = self.enter(Entry::Move);
        if Self::waits_on_item(&st, item) {
            return Err(Cause::NoService);
        }
        let before = Self::agents_held_off(&st);
        let out = self.setup(&mut st, item, input, step, more);
        self.finish_switch_offs_after_reads(&mut st, &before);
        self.publish(&mut st);
        out
    }

    /// `agents_choose_file` (I8.1; r4 U3.22b): the system's own chooser of ONE file for the
    /// question `param` of the open set-up item `item`, at the step mark `step` that the page
    /// showed. The call is refused where `setup_typed` refuses a typed line of that question,
    /// with the same cause (`no_service` while the item waits for the settle of RD-84, `stale`
    /// for no such set-up item, `state` for another step mark or no board), and for every
    /// question but Which spreadsheet file and Where the data comes from (`crate::items::choose_in`).
    /// The chooser opens in the folder that `choose_in` gives, with the coordinator's lock
    /// released, and the call answers what `Files::choose` answers, `no_service` or `busy` where
    /// the port fails. It stores nothing, sends nothing to the owner and changes no item: a
    /// picked path reaches the item only as a typed line, through `setup_typed`.
    pub fn setup_choose(&self, item: &ItemId, param: Param, step: u32) -> Result<Chosen, Cause> {
        let folder = {
            let st = self.lock();
            if Self::waits_on_item(&st, item) {
                return Err(Cause::NoService);
            }
            let setup = st
                .setups
                .iter()
                .find(|s| s.item() == item)
                .ok_or(Cause::Stale)?;
            if setup.step() != step {
                return Err(Cause::State);
            }
            let board = st.board.as_ref().ok_or(Cause::State)?;
            index(board, setup.agent()).ok_or(Cause::State)?;
            crate::items::choose_in(setup, param, System::HOST)?
        };
        self.ports
            .choose(folder.as_deref())
            .map_err(|error| cause_of(&error))
    }

    /// `agents_present` (I8.1): a confirmation whose presenter is native; `presenter` for the
    /// page's; `no_service` for a native one while the board holds stuck the record of the agent
    /// whose row shows it, the agent of the run that raised it, so that nobody is asked to allow
    /// what Halvo cannot save (rulings RD-147 (5)(ii), RD-164).
    pub fn present(&self, item: &ItemId) -> Result<(), Cause> {
        let st = self.lock();
        let h = st
            .held
            .iter()
            .find(|h| h.item() == item)
            .ok_or(Cause::UnknownItem)?;
        match h.confirmation().presenter() {
            Presenter::Native if self.withdrawn(&st, item) => Err(Cause::NoService),
            Presenter::Native => Ok(()),
            Presenter::Page => Err(Cause::Presenter),
        }
    }

    /// The card of the native dialog of the confirmation `item` (I8.1 `agents_present`, ruling
    /// RD-77 12.1): `items::native_card` of the confirmation that the coordinator holds for that
    /// id, read under the lock. As the first call after a move or a tick that did not return it
    /// publishes before it reads, as every call does then (rulings RD-80 3.4, 4.2): the snapshot's
    /// `seq` rises, the page's bell rings (`Page::changed`) and the tray's count is set
    /// (`Tray::attention`); any other call of it calls no port. It logs no row and sends no body.
    /// `unknown_item` for an id that is no held confirmation, an item of another kind and no item
    /// among them, the causes of `items::native_card` otherwise, and then `no_service` while the
    /// board holds stuck the record of the agent whose row shows the item, the agent of the run
    /// that raised it (rulings RD-147 (5)(ii), RD-164).
    pub fn native_card(&self, item: &ItemId) -> Result<NativeCard, Cause> {
        let st = self.lock();
        let h = st
            .held
            .iter()
            .find(|h| h.item() == item)
            .ok_or(Cause::UnknownItem)?;
        let card = native_card(h)?;
        if self.withdrawn(&st, item) {
            return Err(Cause::NoService);
        }
        Ok(card)
    }

    fn withdrawn(&self, st: &State<Cfg<P>>, item: &ItemId) -> bool {
        st.board.as_ref().is_some_and(|b| {
            b.members.iter().any(|m| {
                matches!(&m.facts.doing, Doing::Live(Live::Waiting(open)) if &open.item == item)
                    && b.record_stuck(&m.facts.agent, &self.limits)
            })
        })
    }

    /// The page's report that it opened the page confirmation `item` at the step mark `step`
    /// (I8.1 `agents_shown`, ruling RD-80 5.1, 5.2), from which an answer arms `arm_ms` later:
    /// the clock is read first, before the lock, and a clock that cannot be read is its cause, so
    /// that the page reports again; an id that is no item is `unknown_item`; an item of another
    /// presenter, a native confirmation or an item of another kind, is `presenter`, with no time
    /// noted; a confirmation that is answered or later is `stale`; a step that is not the item's
    /// is `state`. The first report counts, and a later one answers ok and changes nothing.
    pub fn shown_on_page(&self, item: &ItemId, step: u32) -> Result<(), Cause> {
        let now = self.clock_now().map_err(|e| cause_of(&e));
        let mut st = self.lock();
        let held = st.held.iter().position(|h| h.item() == item);
        let closed = st.board.as_ref().is_some_and(|b| {
            b.members
                .iter()
                .any(|m| schedule::item_closed(&m.facts.events, item))
        });
        let open = self.owner_of(&st, item).is_ok();
        let Some(k) = held else {
            return Err(match (closed, open) {
                (true, _) => Cause::Stale,
                (false, true) => Cause::Presenter,
                (false, false) => Cause::UnknownItem,
            });
        };
        let h = &mut st.held[k];
        if h.confirmation().presenter() != Presenter::Page {
            return Err(Cause::Presenter);
        }
        if !h.confirmation().state().waits_for_person() {
            return Err(Cause::Stale);
        }
        if step != 0 {
            return Err(Cause::State);
        }
        items::on_screen(h, now?);
        Ok(())
    }

    /// The page is hidden or its window is gone (RD-80 5.6): the report of each page
    /// confirmation that is still `shown` is forgotten, so that its next show counts anew, and an
    /// armed one stays armed.
    pub fn page_hidden(&self) {
        let mut st = self.lock();
        for h in st
            .held
            .iter_mut()
            .filter(|h| h.confirmation().presenter() == Presenter::Page)
        {
            items::forget_report(h);
        }
    }

    /// The presenter's first report that a confirmation is on screen, from which it arms (I4.2).
    pub fn on_screen(&self, item: &ItemId) {
        let mut st = self.lock();
        let Ok(now) = self.clock_now() else { return };
        if let Some(h) = st.held.iter_mut().find(|h| h.item() == item) {
            items::on_screen(h, now);
        }
    }

    fn schedule(&self, st: &mut State<Cfg<P>>, mv: Move) -> Result<(), Cause> {
        let on = match &mv {
            Move::SwitchOn { agent } => Some(agent.clone()),
            _ => None,
        };
        let State { board, ledgers, .. } = st;
        let board = board.as_mut().ok_or(Cause::NoService)?;
        share_lasts(board, ledgers);
        let report = schedule::drive(&self.ports, board, &self.limits, mv);
        self.noted(report.read);
        if let (Some(agent), Some(stamp)) = (&on, report.stamp) {
            run::raise_amounts(&self.ports, board, &self.limits, Some(agent), stamp);
        }
        if let Some(agent) = on {
            st.refused_on.retain(|(a, _)| a != &agent);
            if let Some(shared) = report.shared.clone() {
                st.refused_on.push((agent, shared));
            }
        }
        answered(&report)
    }

    /// What the open setup item `item` holds to show of the newest refusal `overlap` of its
    /// body: the entries of the read that followed it, or that the read did not answer; none
    /// otherwise (I3.5 (c), (d); ruling RD-67 3.2). The sentence's words are U2.4d's.
    pub fn setup_overlap(&self, item: &ItemId) -> Option<crate::ports::Shared> {
        let st = self.lock();
        st.setups
            .iter()
            .find(|s| s.item() == item)
            .and_then(|s| s.shared().cloned())
    }

    /// What the newest switch-on or start by the person of `agent` holds to show where the owner
    /// refused its state with `overlap`: the entries of the read that followed, or the half of the
    /// sentence that is known where that read did not answer; none after a later control of the
    /// person on that agent that the owner did not refuse so, such as a switch-on or a Start now
    /// that goes, a switch-off or a stop (I3.5 (c), (f); rulings RD-67 3.2 (f), 4.3, RD-160 (3)).
    /// It keeps its name for the page that reads it.
    pub fn switch_on_overlap(&self, agent: &AgentId) -> Option<crate::ports::Shared> {
        Self::switch_on_shared(&self.lock(), agent)
    }

    /// A start by the person, `run_now` or the `start` of a `ready` item: `state` after the first
    /// step of the quit, and `no_service` while the hold of a delete of everything stands and
    /// while the mark of a tick that did not return stands (ruling RD-80 4.5).
    fn start_in(&self, st: &mut State<Cfg<P>>, agent: &AgentId) -> Result<(), Cause> {
        if st.quitting || self.quit_mark.load(Ordering::SeqCst) {
            return Err(Cause::State);
        }
        if self.hold_mark.load(Ordering::SeqCst) || st.mark.tick {
            return Err(Cause::NoService);
        }
        if st.offs.contains(agent) || st.held_off.contains(agent) {
            return Err(Cause::Hold);
        }
        self.act(
            st,
            Act::Start {
                agent: agent.clone(),
            },
        )
    }

    fn gone(st: &State<Cfg<P>>, board: &Board<Cfg<P>>, i: usize) -> bool {
        let facts = &board.members[i].facts;
        st.forgotten.contains(&facts.agent) || board.gone.contains(&facts.workflow.id)
    }

    /// The facts of the agents whose rows the page shows, in ascending agent id: every agent but
    /// one that `gone` names.
    fn shown<'a>(st: &State<Cfg<P>>, board: &'a Board<Cfg<P>>) -> Vec<&'a Facts<Cfg<P>>> {
        board
            .members
            .iter()
            .enumerate()
            .filter(|(i, _)| !Self::gone(st, board, *i))
            .map(|(_, m)| &m.facts)
            .collect()
    }

    /// A control of a row (I8.1 `agents_control`). Every control of an agent whose workflow a
    /// tick saw gone, or whose setup body the owner ended, is answered `unknown_agent`, and the
    /// control itself stores nothing; the take-up that the call makes first, below, can still
    /// store and send before that answer (I5.7, rulings IG-33, IG-36); after the first
    /// step of the quit every start is `state`; while the mark of a tick that did not return
    /// stands, `run_now` and `switch_on` are `no_service` (ruling RD-80 4.5). The person's
    /// `stop` lapses the open item of a
    /// waiting run with `cancelled` and stops the run in one command, for every kind of item,
    /// and answers success for an agent whose run is stopping or not live (ruling RD-6); the
    /// switch-off control lapses that item in the same way before its own move. Like every move,
    /// the call first takes up what an earlier stop, switch-off or runner's end left waiting for
    /// a read of the clock, which can read the clock, store and send for agents and their runs,
    /// the named agent among them where its own earlier switch-off waited for the clock; for
    /// `take_back`, `switch_off` and `stop`, after the refusals above, the named agent's
    /// start that waits in the queue is set aside during that take-up and put back in its place
    /// after it, so that no end that the take-up makes starts the agent that the control names
    /// (RD-149, verifier-59 issue 1). The take-back's own move then takes that start out of the
    /// queue and keeps the agent's open item; it reads no port and no clock, stores and sends
    /// nothing, adds no hold and leaves every hold that stood, and it answers success also where
    /// no start of the agent waits, a run that started before the call going on (rulings RD-54
    /// (h), RD-52 (d)). The call ends, as every control does, by publishing the state, which
    /// tells the page and the tray when it changed.
    /// A switch-off
    /// whose `hold_set` the owner refuses with a name of class B stops the agent's live run as
    /// the stop does, stores no hold and answers the store's cause; the agent stays on, and the
    /// answer of `control_reply` says both, that the task was stopped and that the switch-off
    /// could not be saved (ruling RD-155 (1); term (5) of RD-161 R1).
    pub fn control(&self, agent: &AgentId, action: Action) -> Result<(), Cause> {
        self.control_reply(agent, action).map_err(|r| r.cause())
    }

    /// `control`, with its refusal as the reply of `agents_control` writes it: a switch-off whose
    /// `hold_set` the owner refused under class B after it stopped the agent's live run is
    /// `Refusal::SwitchNotSaved`, whose sentence the page shows beside the control, while the
    /// owner's list does not show the agent's workflow dismissed (ruling RD-168); a switch-on
    /// whose state the owner refused with `overlap` is `Refusal::Overlap`, the cause `overlap`
    /// and not `no_service`, the cause of a lost call, with the sentence of what
    /// `switch_on_overlap` holds and the names of the rows of the state that the call publishes
    /// (ruling RD-161 R4, WAY A; I3.5 (f)); a `stop_sign_in` whose body the owner refused with
    /// `store` is `Refusal::SignInNotFinished`, the cause `no_service` with the sentence that
    /// Halvo could not finish this (r4 U3.16); every other refusal is its cause alone. A start of
    /// the person that the owner refuses so keeps its cause.
    pub fn control_reply(
        &self,
        agent: &AgentId,
        action: Action,
    ) -> Result<(), crate::types::Refusal> {
        let mut st = self.enter_control(agent, Some(action));
        let before = Self::agents_held_off(&st);
        let off = matches!(action, Action::SwitchOff);
        st.refused_on.retain(|(a, _)| a != agent);
        let kept = Self::still_on(&st, agent);
        let out = self.control_in(&mut st, agent, action);
        let unfinished = std::mem::take(&mut st.sign_in_unfinished);
        let model = st.model_unfinished.take();
        let stopped = off && out.is_err() && !kept && Self::still_on(&st, agent);
        let shared = (matches!(action, Action::SwitchOn) && out.is_err())
            .then(|| Self::switch_on_shared(&st, agent))
            .flatten();
        if off && out.is_ok() {
            // Every owed `paused` before a freed body, where a set-up keeps one (ruling RD-160
            // (2)); a switch-off that waits for a read of the clock sends its own with the rest
            // of it (ruling RD-133 1).
            let freed = !st.offs.contains(agent) && st.setups.iter().any(|s| s.kept().is_some());
            if let (Some(board), true) = (st.board.as_mut(), freed) {
                schedule::pause_owed(&self.ports, board);
            }
            self.setups_freed(&mut st, agent);
        }
        self.finish_switch_offs_after_reads(&mut st, &before);
        self.deletes_due(&mut st);
        self.publish(&mut st);
        let overlap = shared.map(|shared| {
            let rows = st.sync.snapshot().rows;
            crate::types::Overlapping::of(&shared, |a| {
                rows.iter().find(|r| &r.agent == a).map(|r| r.name.clone())
            })
        });
        out.map_err(|cause| match overlap {
            _ if stopped => crate::types::Refusal::SwitchNotSaved(cause),
            _ if unfinished => crate::types::Refusal::SignInNotFinished(cause),
            _ if model == Some(Action::UseModel) => {
                crate::types::Refusal::UseModelNotFinished(cause)
            }
            _ if model == Some(Action::ThisComputerOnly) => {
                crate::types::Refusal::ThisComputerNotFinished(cause)
            }
            Some(say) => crate::types::Refusal::Overlap(say),
            None => crate::types::Refusal::Cause(cause),
        })
    }

    /// What `switch_on_overlap` holds for `agent`, read under a lock already held.
    fn switch_on_shared(st: &State<Cfg<P>>, agent: &AgentId) -> Option<crate::ports::Shared> {
        st.refused_on
            .iter()
            .find(|(a, _)| a == agent)
            .map(|(_, s)| s.clone())
    }

    /// The name of the row of `agent` among `listed`, the agents of the rows with their names.
    fn row_name(listed: &[(&AgentId, &str)], agent: &AgentId) -> Option<String> {
        listed
            .iter()
            .find(|(a, _)| *a == agent)
            .map(|(_, name)| (*name).to_owned())
    }

    /// Whether the board marks the agent kept on after a switch-off that could not be saved
    /// (`Board::kept_on`), while the owner's list does not show its workflow dismissed.
    fn still_on(st: &State<Cfg<P>>, agent: &AgentId) -> bool {
        st.board.as_ref().is_some_and(|board| {
            board.kept_on(agent)
                && board.members.iter().any(|m| {
                    &m.facts.agent == agent && !board.workflow_dismissed(&m.facts.workflow.id)
                })
        })
    }

    /// The person's yes to the question of a row's Delete (I8.1 `agents_delete`; ruling RD-19
    /// (c) rule 7), which removes the agent with its workflow through `Owner::delete`, the one
    /// delete of W-C3.10, at that yes and in this life or not at all (ruling RD-30 (1)). Like a
    /// Stop, the call first takes up what an earlier move left, with the agent's queued start
    /// set aside during it. An agent with no run on the board, or one that a new life set
    /// aside, is deleted at once, and a start that only waits its turn leaves the queue with the
    /// agent, as I5.7 drops it; the row then leaves the state and every control of the agent answers
    /// `unknown_agent`. With a run on the board the call stores the hold `off` and stops the run
    /// as the switch-off does, so that the agent stays switched off after a quit, keeps the
    /// intent in memory alone, and sends the delete once that run's `run_ended` is stored, at
    /// the end of the move that stores it or of a later tick; meanwhile the row shows
    /// `working.deleting` with no control. An agent whose record is stuck is stopped as Stop
    /// stops it, with no row of the hold, and its delete is sent once the run has left the
    /// board (ruling RD-64 1.6 (b)). After the first step of the quit no intent is sent and the
    /// end of the next move forgets each, so nothing is deleted and the agent stands stopped and switched off, with Delete on its row
    /// in the next life (I5.7). Causes: `no_service` with no board; `unknown_agent` for an agent
    /// that has no row or whose workflow is gone; `state` after the quit's first step, while a
    /// switch-off of the agent waits for a read, or where the owner refuses the delete, its
    /// guard among them, which the page meets by reading the state anew (ruling RD-19 (d)
    /// (1)); `busy` and `no_service` where the owner or the store of the hold answers so, and
    /// then nothing is deleted and no run is stopped. A second yes while an intent stands
    /// answers success and changes nothing. A refusal `no_workflow` of the delete counts as
    /// done, since the owner holds no row of the workflow.
    pub fn delete(&self, agent: &AgentId) -> Result<(), Cause> {
        let mut st = self.enter_control(agent, None);
        let before = Self::agents_held_off(&st);
        let out = self.delete_in(&mut st, agent);
        self.finish_switch_offs_after_reads(&mut st, &before);
        self.deletes_due(&mut st);
        self.publish(&mut st);
        out
    }

    fn delete_in(&self, st: &mut State<Cfg<P>>, agent: &AgentId) -> Result<(), Cause> {
        if st.quitting {
            return Err(Cause::State);
        }
        let board = st.board.as_ref().ok_or(Cause::NoService)?;
        let i = index(board, agent).ok_or(Cause::UnknownAgent)?;
        if Self::gone(st, board, i) {
            return Err(Cause::UnknownAgent);
        }
        if st.deleting.iter().any(|d| &d.agent == agent) {
            return Ok(());
        }
        let workflow = board.members[i].facts.workflow.id.clone();
        let on_board = board
            .runs
            .iter()
            .find(|r| &r.agent == agent)
            .map(|r| r.run.clone());
        let Some(run) = on_board.filter(|_| !board.aside(agent)) else {
            return self.send_delete(st, agent, &workflow);
        };
        if st.offs.contains(agent) || st.held_off.contains(agent) {
            return Err(Cause::State);
        }
        let stuck = board.record_stuck(agent, &self.limits);
        let unstopped =
            live_unstopped_run(board, agent).filter(|r| !st.ended.iter().any(|(e, _)| e == r));
        let off = schedule::holds(&board.members[i].facts).contains(&schedule::Hold::Off);
        if stuck {
            if unstopped.is_some() {
                self.stop_in(st, i)?;
            }
        } else if unstopped.is_some() || !off {
            self.switch_off(
                st,
                i,
                Move::SwitchOff {
                    agent: agent.clone(),
                },
            )?;
        }
        st.deleting.push(Deleting {
            agent: agent.clone(),
            workflow,
            run,
            board_end: stuck,
        });
        Ok(())
    }

    /// Sends the delete of `workflow` and, once the owner took it, forgets the agent as a tick
    /// forgets one whose workflow it saw gone: its row, its items and its queued start go.
    fn send_delete(
        &self,
        st: &mut State<Cfg<P>>,
        agent: &AgentId,
        workflow: &WorkflowId,
    ) -> Result<(), Cause> {
        match self.ports.delete(workflow) {
            Ok(()) => {}
            Err(PortError::Refused(name)) if name.as_str() == "no_workflow" => {}
            Err(PortError::Refused(_)) => return Err(Cause::State),
            Err(e) => return Err(cause_of(&e)),
        }
        st.deleting.retain(|d| &d.agent != agent);
        if !st.forgotten.contains(agent) {
            st.forgotten.push(agent.clone());
        }
        st.held.retain(|h| h.ask().agent() != agent);
        st.binds.retain(|b| &b.ask().agent != agent);
        st.handbacks.retain(|(_, a)| &a.agent != agent);
        st.waits.retain(|w| &w.agent != agent);
        st.setups.retain(|s| s.agent() != agent);
        for (a, l) in st.ledgers.iter_mut() {
            if a == agent {
                *l = run::forgotten(workflow.clone(), agent.clone(), l);
            }
        }
        if let Some(board) = st.board.as_mut() {
            board.queue.remove(agent);
        }
        Ok(())
    }

    /// Sends each delete whose run has ended: its `run_ended` stored, which is so once the run
    /// has left the board and the board owes no `run_ended` of it, or, for a delete of a stuck
    /// record, once the run has left the board. A delete that the owner answers `busy` or
    /// `no_service` is tried again at the end of a later move; one that it refuses is dropped,
    /// so the agent stands stopped and switched off with Delete on its row. After the first
    /// step of the quit every intent is forgotten and nothing is sent.
    fn deletes_due(&self, st: &mut State<Cfg<P>>) {
        if st.quitting {
            st.deleting.clear();
            return;
        }
        let Some(board) = st.board.as_ref() else {
            return;
        };
        let due: Vec<(AgentId, WorkflowId)> = st
            .deleting
            .iter()
            .filter(|d| {
                let on_board = board.runs.iter().any(|r| r.run == d.run);
                let owed = board.owed.iter().any(
                    |o| matches!(&o.what, schedule::Owing::RunEnded { run, .. } if run == &d.run),
                );
                let kept = st.ended.iter().any(|(r, _)| r == &d.run);
                !on_board && (d.board_end || (!owed && !kept))
            })
            .map(|d| (d.agent.clone(), d.workflow.clone()))
            .collect();
        for (agent, workflow) in due {
            match self.send_delete(st, &agent, &workflow) {
                Ok(()) | Err(Cause::Busy | Cause::NoService) => {}
                Err(_) => st.deleting.retain(|d| d.agent != agent),
            }
        }
    }

    fn control_in(
        &self,
        st: &mut State<Cfg<P>>,
        agent: &AgentId,
        action: Action,
    ) -> Result<(), Cause> {
        let board = st.board.as_ref().ok_or(Cause::NoService)?;
        let i = index(board, agent).ok_or(Cause::UnknownAgent)?;
        let gone = Self::gone(st, board, i);
        match action {
            _ if gone => Err(Cause::UnknownAgent),
            Action::RunNow => self.start_in(st, agent),
            Action::SwitchOn
                if st.mark.tick || st.offs.contains(agent) || st.held_off.contains(agent) =>
            {
                Err(Cause::NoService)
            }
            Action::SwitchOff if st.offs.contains(agent) || st.held_off.contains(agent) => {
                Err(Cause::State)
            }
            Action::SwitchOn => self.schedule(
                st,
                Move::SwitchOn {
                    agent: agent.clone(),
                },
            ),
            Action::SwitchOff => self.switch_off(
                st,
                i,
                Move::SwitchOff {
                    agent: agent.clone(),
                },
            ),
            Action::Stop => self.stop_in(st, i),
            Action::StopSignIn => self.stop_sign_in(st, i),
            Action::UseModel => self.use_model(st, i),
            Action::ThisComputerOnly => self.this_computer_only(st, i),
            Action::ChangeAnswers => self.change_answers(st, i),
            Action::TakeBack => {
                self.schedule(
                    st,
                    Move::TakeBack {
                        agent: agent.clone(),
                    },
                )?;
                let asks = st.board.as_ref().is_some_and(|b| {
                    index(b, agent)
                        .is_some_and(|i| schedule::needs_person(&b.members[i].facts.config))
                });
                st.taken_back.retain(|(a, _)| a != agent);
                if asks {
                    st.taken_back.push((agent.clone(), None));
                }
                Ok(())
            }
        }
    }

    /// The person's `stop_sign_in` (r4 U2.4y): after the quit's first step, as the delete, and
    /// while the agent is not idle on the board, it answers `state` and sends nothing; else the
    /// schedule's `drop_sessions` sends ONE body `drop_sessions`, after whose success the agent's
    /// member holds no session, so the row that the call publishes offers the control no more.
    /// The store's refusal `state` (a run of the agent is open there) and its refusal `day` (a row
    /// of a later day stands, after this computer's clock was set back) answer those causes, so
    /// that the person reads their sentences beside the control; its refusal `store` (a fault of
    /// the store, behind which the agent's kept sign-ins may be gone from every read or may all
    /// stand) answers `no_service` and marks the call, so that its reply says that Halvo could
    /// not finish this (r4 U3.16); every other refusal answers what `cause_of` makes of it.
    fn stop_sign_in(&self, st: &mut State<Cfg<P>>, i: usize) -> Result<(), Cause> {
        if st.quitting {
            return Err(Cause::State);
        }
        let board = st.board.as_mut().ok_or(Cause::NoService)?;
        if !matches!(board.members[i].facts.doing, Doing::Idle(_)) {
            return Err(Cause::State);
        }
        match schedule::drop_sessions(&self.ports, board, i) {
            Ok(()) => Ok(()),
            Err(PortError::Refused(name)) if name.as_str() == "state" => Err(Cause::State),
            Err(PortError::Refused(name)) if name.as_str() == "day" => Err(Cause::Day),
            Err(PortError::Refused(name)) if name.as_str() == "store" => {
                st.sign_in_unfinished = true;
                Err(Cause::NoService)
            }
            Err(e) => Err(cause_of(&e)),
        }
    }

    /// The person's `use_model`, the answer Allow for this agent of the question `use_model` (r4
    /// U3.17b): after the quit's first step, and where the agent's row offers no `use_model` with
    /// its question (no host, a runtime outside the ONE list `roster::MODEL_RUNTIMES`, a yes that
    /// stands for a hosted tier, no key saved, no limit set, or a state that could not be read),
    /// it answers `state` and sends nothing; else the schedule's `set_model` sends ONE body, the
    /// yes `agree_hosted`. Where the owner refuses it or no reply comes, it marks the call, so that
    /// its reply says that Halvo could not finish this, and the agent's member stays as it was.
    fn use_model(&self, st: &mut State<Cfg<P>>, i: usize) -> Result<(), Cause> {
        if st.quitting {
            return Err(Cause::State);
        }
        let board = st.board.as_mut().ok_or(Cause::NoService)?;
        let config = &board.members[i].facts.config;
        let asks = roster::model_controls(config).contains(&crate::types::Control::UseModel)
            && config
                .model()
                .is_some_and(|model| matches!(roster::use_model_ask(&model), Ok((Some(_), _))));
        if !asks {
            return Err(Cause::State);
        }
        match schedule::set_model(&self.ports, board, i, true) {
            Ok(()) => Ok(()),
            Err(e) => {
                st.model_unfinished = Some(Action::UseModel);
                Err(cause_of(&e))
            }
        }
    }

    /// The person's `this_computer_only`, which asks nothing (r4 U3.17b): after the quit's first
    /// step, and where the agent's row does not offer it (no host, or the stored tier is
    /// `on_device`), it answers `state` and sends nothing; else the schedule's `set_model` sends
    /// ONE body, the way back `withdraw_hosted`. Where the owner refuses it or no reply comes, it
    /// marks the call, so that its reply says that Halvo could not finish this, and the agent's
    /// member stays as it was.
    fn this_computer_only(&self, st: &mut State<Cfg<P>>, i: usize) -> Result<(), Cause> {
        if st.quitting {
            return Err(Cause::State);
        }
        let board = st.board.as_mut().ok_or(Cause::NoService)?;
        let back = crate::types::Control::ThisComputerOnly;
        if !roster::model_controls(&board.members[i].facts.config).contains(&back) {
            return Err(Cause::State);
        }
        match schedule::set_model(&self.ports, board, i, false) {
            Ok(()) => Ok(()),
            Err(e) => {
                st.model_unfinished = Some(Action::ThisComputerOnly);
                Err(cause_of(&e))
            }
        }
    }

    /// The person's `change_answers` (r4 U3.22a): after the quit's first step, and where the
    /// agent's row does not offer Change its answers (`roster::offers_change_answers`), it answers
    /// `state`. Else it asks the owner ONCE, with the lock held, for the agent's stored answers
    /// (`Owner::sheet_answers`, r3 U11.7; r4 U3.22g), sends no body and stores nothing: the agent
    /// holds a new set-up item open on the board, which no row raised, and the coordinator puts it
    /// up asking every question of the agent's category, as asked again (`Setup::asked_again`),
    /// each line holding its stored answer where the core takes it as that question's typed line
    /// (`Setup::holding`) and empty otherwise, every line empty where the read fails; so that Not
    /// now closes it with the stored answers standing and Use these answers sends ONE body
    /// `set_bindings` with a slot for every question. Named limit: such an item is gone when Halvo
    /// starts again, and the stored answers stand.
    fn change_answers(&self, st: &mut State<Cfg<P>>, i: usize) -> Result<(), Cause> {
        if st.quitting {
            return Err(Cause::State);
        }
        let board = st.board.as_mut().ok_or(Cause::NoService)?;
        let facts = &board.members[i].facts;
        if !roster::offers_change_answers(facts) {
            return Err(Cause::State);
        }
        let stored = self
            .ports
            .sheet_answers(&facts.workflow.id, &facts.agent)
            .unwrap_or_default();
        let minted = self.ports.bytes16().map_err(|e| cause_of(&e))?;
        let item = ItemId::parse(&crate::types::hex(minted)).map_err(|_| Cause::NoService)?;
        let asked = params(facts.workflow.category)
            .iter()
            .map(|p| SetupParam {
                param: *p,
                candidates: candidates(*p, &facts.workflow),
            })
            .collect();
        let setup = items::open_setup(
            &self.ports,
            item.clone(),
            facts.workflow.id.clone(),
            facts.agent.clone(),
            facts.workflow.category,
            asked,
        )?
        .asked_again()
        .holding(&stored);
        board.members[i].facts.doing = Doing::Idle(Some(crate::types::OwnItem {
            item,
            kind: OwnItemKind::Setup,
        }));
        st.setups.push(setup);
        Ok(())
    }

    /// Follows each agent whose start the person took back: the first `ready` item that the
    /// agent holds after it is that mark's item; the mark ends once the agent holds no item of
    /// its own any more after that item, holds another one, runs or waits in the queue.
    fn follow_taken_back(st: &mut State<Cfg<P>>) {
        let Some(board) = st.board.as_ref() else {
            return;
        };
        let mut kept = Vec::new();
        for (agent, item) in std::mem::take(&mut st.taken_back) {
            let Some(i) = index(board, &agent) else {
                continue;
            };
            let facts = &board.members[i].facts;
            let open = facts
                .doing
                .item()
                .filter(|o| o.kind == ItemKind::Ready && matches!(facts.doing, Doing::Idle(_)));
            match (open, item) {
                (Some(o), None) => kept.push((agent, Some(o.item))),
                (Some(o), Some(was)) if o.item == was => kept.push((agent, Some(was))),
                (None, None) if matches!(facts.doing, Doing::Idle(_)) => kept.push((agent, None)),
                _ => {}
            }
        }
        st.taken_back = kept;
    }

    /// The person's Stop (I5.7). It is refused by no read of the clock (ruling RD-133 1): where
    /// none answers, the stop goes on the guard's table and the board at once, with its time and
    /// its rows waiting for the next read that answers (`stamp_waiting`), and `Runs::cancel` goes
    /// out in this move, for a run with an open item here and for one with none in the run
    /// drive's stop. A new life before that read shows the waiting rows lost in the way that
    /// recovery reads them: it ends the run `interrupted` and lapses its item `app_ended`, so the
    /// person reads that it stopped because Halvo closed, not that they stopped it. A Stop of an agent with no live run takes its queued start out of the
    /// queue, at any time and with no read of the clock (Addition 1 item 5).
    fn stop_in(&self, st: &mut State<Cfg<P>>, i: usize) -> Result<(), Cause> {
        let now = self.clock_now().ok();
        let State {
            board,
            table,
            ended,
            ..
        } = st;
        let board = board.as_mut().ok_or(Cause::NoService)?;
        let agent = board.members[i].facts.agent.clone();
        let live =
            live_unstopped_run(board, &agent).filter(|run| !ended.iter().any(|(r, _)| r == run));
        let Some(run) = live else {
            if board.queue.remove(&agent) {
                let facts = &mut board.members[i].facts;
                if let Doing::Queued(item) = &facts.doing {
                    facts.doing = Doing::Idle(item.clone());
                }
            }
            return Ok(());
        };
        let Some(open) = waiting_on(&board.members[i].facts.doing) else {
            return self.act(
                st,
                Act::Stop {
                    run,
                    by: StopBy::Person,
                },
            );
        };
        match now {
            Some(now) => hold_stop(board, table, &run, now, StopBy::Person),
            None => {
                run::stop_unstamped(board, table, &run, StopBy::Person);
            }
        }
        self.close_run_item(st, i, &run, &open, true);
        Ok(())
    }

    /// Marks the app's quit (I5.7): from this call on every start that has not passed its last
    /// reading of the mark is refused, the person's, the queue's at the end of a run and a
    /// tick's, also before the first step of the quit has taken the coordinator's lock and also
    /// in a move that holds that lock now. A start reads the mark at its start of a run and last
    /// directly before `Runs::start`, inside the lock of the list of `not_stopped`: a start that
    /// passed that reading before this call stands in that list from its reading, and a later
    /// one is refused there and ends its run `interrupted`. A tick after the mark makes only the
    /// stages that start nothing, as after the first step. This call takes no lock and calls no port, so the app's
    /// quit hook makes it first, before anything that can wait.
    pub fn mark_quit(&self) {
        self.quit_mark.store(true, Ordering::SeqCst);
    }

    /// The first step of the app's quit (I5.7, ruling RD-8): it marks the quit (`mark_quit`)
    /// before it takes the coordinator's lock; then every open item lapses with `app_ended` in
    /// the life that ends (I4.5), every live run that no stop has reached gets `cancel` through
    /// the run drive's stop-all, and the queue is emptied before any port call of the step.
    /// From the mark on every start that has not passed its last reading of the mark is refused,
    /// the person's, the queue's and a tick's, also where a port panics in the step before its
    /// own work. It returns at once, with the time at which the hook calls the second step at
    /// the latest: `cancel_grace_s` after the latest stop of a live run, where a stop that reads
    /// later than the first step counts from the first step (ruling RD-28 (e)), and the first
    /// step's time when no run is live; and with the time that the step read on the clock, or
    /// the last read of the life where that read fails: the hook waits for the distance from
    /// that time to the first, counted from the return of this step, without a read of its own.
    pub fn quit_stop(&self) -> QuitWait {
        self.mark_quit();
        let mut st = self.enter(Entry::Move);
        st.quitting = true;
        self.stop_every_run(&mut st)
    }

    /// Marks the hold of a delete of everything: from this call on every start that has not
    /// passed its last reading of the marks is refused, as after `mark_quit`, until `lift_hold`.
    /// This call takes no lock and calls no port.
    pub fn mark_hold(&self) {
        self.hold_mark.store(true, Ordering::SeqCst);
    }

    /// Lifts the hold of a delete of everything and nothing else: the next tick is a whole tick.
    /// It never lifts the quit's mark, so after `mark_quit` a lift starts nothing.
    pub fn lift_hold(&self) {
        self.hold_mark.store(false, Ordering::SeqCst);
    }

    /// The first step of the hold of a delete of everything, which the app makes before its
    /// delete goes out (the capture run's app-3b4): `mark_hold`, and then the quit's first step
    /// after its mark, the same code: the queue emptied, every open item lapsed, every live run
    /// that no stop has reached cancelled, and the time of the wait, with no mark of the quit for
    /// the life.
    pub fn hold_stop(&self) -> QuitWait {
        self.mark_hold();
        let mut st = self.enter(Entry::Move);
        self.stop_every_run(&mut st)
    }

    fn stop_every_run(&self, st: &mut State<Cfg<P>>) -> QuitWait {
        if let Some(board) = st.board.as_mut() {
            for m in &mut board.members {
                if board.queue.remove(&m.facts.agent) {
                    if let Doing::Queued(item) = &m.facts.doing {
                        m.facts.doing = Doing::Idle(item.clone());
                    }
                }
            }
        }
        self.stop_for_settles(st);
        let last = self.last_read.load(Ordering::Relaxed);
        let now = self.clock_now().unwrap_or(last);
        let members = st.board.as_ref().map_or(0, |b| b.members.len());
        for i in 0..members {
            self.lapse_at_quit(st, i);
        }
        let _ = self.act(st, Act::StopAll);
        let grace = self.limits.cancel_grace_s.saturating_mul(1000);
        let until = st
            .board
            .as_ref()
            .and_then(|b| {
                b.runs
                    .iter()
                    .filter_map(|r| r.stop.map(|s| if s.at == 0 { now } else { s.at.min(now) }))
                    .max()
            })
            .map_or(now, |at| at.saturating_add(grace));
        self.publish(st);
        QuitWait { until, at: now }
    }

    /// The second step of the app's quit (I5.7, ruling RD-8), which the app's hook calls when no
    /// run is live any more or at `QuitWait::until`: sends the rows that still wait in the
    /// agents' ledgers, the lapses of the first step among them, kills every live run and every
    /// dying tree through the run drive's kill-all and stores no `run_ended`, so that recovery
    /// ends those runs `interrupted` in the next life. Then it sends `Runs::kill` for each run
    /// of an earlier life that the board keeps because this life's `Runs::reap` answered it
    /// `alive` (`Board::alive_at_reap`, and the kept row of a gone workflow), so that the Quit
    /// that such a row names reaches its run (ruling RD-151 (c)); a kill that this life's
    /// library answers Ok takes the run out of `not_stopped`, and any other answer, such as the
    /// `unknown_run` of a library that never started the run, leaves it named.
    pub fn quit_kill(&self) {
        self.kill_every_run(true);
    }

    /// The second step of the hold of a delete of everything, which the app's hold calls as the
    /// quit's hook calls `quit_kill`, the same code: it kills every live run and every dying
    /// tree, and each run of an earlier life that the board keeps alive, and stores nothing: no
    /// row of the ledgers, no `run_ended`.
    pub fn hold_kill(&self) {
        self.kill_every_run(false);
    }

    fn kill_every_run(&self, send_ledgers: bool) {
        let mut st = self.enter(Entry::Move);
        let State { board, ledgers, .. } = &mut *st;
        if let (true, Some(board)) = (send_ledgers, board.as_mut()) {
            self.flush_ledgers(board, ledgers);
        }
        let _ = self.act(&mut st, Act::KillAll);
        let killed = st.board.as_ref().map_or_else(Vec::new, |b| {
            run::kill_kept(&self.ports, b, &st.killed_kept)
        });
        st.killed_kept.extend(killed);
        self.publish(&mut st);
    }

    /// The write of the list of the runs that did not stop (ruling RD-150; RD-154 point 2), which
    /// the app's quit hook calls on a thread of its own once the second step has returned or half
    /// of `cancel_grace_s` has passed since it called that step: for each run that `not_stopped`
    /// names at this call, of an agent that the last publish found as a member, one
    /// `run_not_stopped` event in that agent's ledger of the owner, where the next life's
    /// recovery reads it. The rows of one agent go in one body, numbered after the owner's stored
    /// last, which this call reads (`Owner::history`), and stamped by one read of the clock, or by
    /// the last read of this life where that read fails; with neither, nothing is written. A
    /// refusal `seq` reads the stored last once more and sends once more; any other failure
    /// writes nothing for that agent. This call takes no lock that a move holds, so it writes
    /// while a kill of the second step holds the coordinator's lock, and then the list is the one
    /// from before that move. A run whose row the owner acknowledged in this life is not written
    /// again. Not written: a run of an agent that is no member, such as the kept row of a gone
    /// workflow, whose rows the owner keeps no more; and nothing at all while another port call
    /// holds every port, since the owner then does not answer. The rows go past the agent's
    /// ledger, so a later row of that ledger in this life meets the refusal `seq` and is numbered
    /// again after a read (I7.2).
    pub fn quit_write(&self) {
        let fresh: Vec<(AgentId, RunId)> = {
            let written = self.written.lock().unwrap_or_else(PoisonError::into_inner);
            self.not_stopped()
                .into_iter()
                .filter(|pair| !written.contains(pair))
                .collect()
        };
        if fresh.is_empty() {
            return;
        }
        let flows = self
            .flows
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let acknowledged =
            run::write_not_stopped(&self.ports, &fresh, &flows, || self.last_stamp());
        self.written
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend(acknowledged);
    }

    /// The count of live runs as the last publish found them, for which the app's quit hook
    /// waits; a dying tree is no live run. This read takes no lock, so it answers while a kill
    /// holds the coordinator's lock.
    pub fn live(&self) -> usize {
        self.live_runs.load(Ordering::Relaxed)
    }

    /// The runs whose processes the board does not know to be gone, each with its agent, in
    /// the order of their starts, as the last publish found them (ruling RD-147 (5)(i), RD-151):
    /// first each run of an earlier life that the board keeps because this life's `Runs::reap`
    /// answered it `alive` and whose kill this life has not seen answered Ok (`quit_kill`;
    /// RD-151 (c)), then each live run, and each dying tree that the board still holds, a run
    /// that ended after its stop and whose tree no kill has ended yet, named with the agent that
    /// the list gave its run while it lived. The publish at the end of a move writes the list,
    /// before the quit as after it, the first step, the end of a run and the second step among
    /// them, and it
    /// writes it before it calls the notification ports: a port's panic before the write
    /// leaves the list of the move before, a panic in a notification after the write leaves the
    /// list as that move wrote it, and a move that returns before any change, such as a move
    /// refused while a settle waits, writes none. So a run or a tree whose kill was refused stays
    /// in the list, and a tree leaves it with the move whose kill of it was answered Ok. After a
    /// move that did not return, the next call of the coordinator publishes at its entry and
    /// writes the list from the board as that move left it. The run of a start stands at the end
    /// of the list from the start's last reading of the quit's mark, directly before
    /// `Runs::start` (`mark_quit`), until the publish of its move writes the list again, which
    /// drops it where `Runs::start` failed. While a
    /// kill of a tick or of the second step holds the coordinator's lock the list is the one from
    /// before that move: a run whose kill returned earlier in that move stays named until the
    /// move returns. This read takes no lock that a port call holds, so it answers while a kill
    /// holds the coordinator's lock.
    pub fn not_stopped(&self) -> Vec<(AgentId, RunId)> {
        self.unstopped
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The agents whose hold `off` stands in the owner's events as the coordinator has read
    /// them beside a live run that no stop has reached: none after any move (I5.7, ruling RD-52
    /// (b)).
    pub fn off_beside_live(&self) -> Vec<AgentId> {
        let st = self.lock();
        let Some(board) = st.board.as_ref() else {
            return Vec::new();
        };
        Self::agents_held_off(&st)
            .into_iter()
            .filter(|a| live_unstopped_run(board, a).is_some())
            .collect()
    }

    /// The agents whose start waits in the queue, in its order, each with whether its hold `off`
    /// stands in the owner's events as the coordinator has read them: never after any move (I5.7,
    /// ruling RD-52 (d)).
    pub fn queued(&self) -> Vec<(AgentId, bool)> {
        let st = self.lock();
        let Some(board) = st.board.as_ref() else {
            return Vec::new();
        };
        let offs = Self::agents_held_off(&st);
        board
            .queue
            .entries()
            .iter()
            .map(|w| (w.agent.clone(), offs.contains(&w.agent)))
            .collect()
    }

    /// The guard for an effect record of a run (I4.7): `Policy` for the record's class and the
    /// configuration of its agent, and `guard_step` on the guard's table.
    pub fn check_record(&self, record: &EffectRecord) -> Result<(), Refusal> {
        let mut st = self.lock();
        let config = st
            .board
            .as_ref()
            .and_then(|b| index(b, &record.agent).map(|i| b.members[i].facts.config.clone()));
        let table = guard::check(&self.ports, config.as_ref(), &st.table, record)?;
        st.table = table;
        Ok(())
    }

    /// The owner's acknowledgement of the store of an effect record, which the link reports
    /// (I4.7, ruling IG-32): the guard's table notes the effect as begun where the record's
    /// state begins it, so that a later confirm request of its run for that effect shows no card
    /// and ends the run with `kind`.
    pub fn stored(&self, record: &EffectRecord) {
        let mut st = self.lock();
        st.table = st.table.stored(record);
    }

    /// Whether the agent's record is stuck on the board, which gives its row the line
    /// `line.problem.record` (I5.7, I1.3; rulings RD-64 2.5, RD-122 T1); false where there is no
    /// board or no such agent.
    pub fn record_stuck(&self, agent: &AgentId) -> bool {
        let st = self.lock();
        st.board
            .as_ref()
            .is_some_and(|b| b.record_stuck(agent, &self.limits))
    }

    /// The count of the agent's records taken off in this life, with the event name and the
    /// refusal name of the last one, as the board holds them (I7.2; rulings RD-64 2.6, RD-122 T1).
    pub fn records_dropped(&self, agent: &AgentId) -> Option<(u32, String, String)> {
        let st = self.lock();
        st.board
            .as_ref()
            .and_then(|b| b.dropped(agent))
            .map(|(n, event, refusal)| (n, event.to_string(), refusal.to_string()))
    }

    /// The card of an agent (I8.1 `agents_card`, I2.6), read through `Owner::agent_history`,
    /// with the name of its row: two agents of one name get their ordinal among the agents whose
    /// rows the page shows (I2.3). Its first line is `card.records_dropped` with the count of
    /// `records_dropped` where the board took off a record of the agent in this life. Where the
    /// history read answers busy, the card holds that line alone, where the count is at least
    /// 1, no run, the key `history` with `busy`, so that the page asks again and never shows it
    /// as a card with no history, and the key `unread` with the sentence and the control text
    /// that the page shows once it asks no more (RD-148 R6, way C); any other refusal of the
    /// read answers its cause (the coordinator's word on R6 of U2.4t). An agent that has no
    /// row, whose workflow a tick saw gone or whose setup body the owner ended, is
    /// `unknown_agent`, as for every control.
    /// A run that this life's `Runs::reap` answered `alive` and that a record of the store names
    /// takes its line from that record's end, as its row does (`Board::ended_after_reap`).
    pub fn card(&self, agent: &AgentId) -> Result<Card, Cause> {
        let st = self.lock();
        let board = st
            .board
            .as_ref()
            .filter(|_| st.returned)
            .ok_or(Cause::NoService)?;
        let shown = Self::shown(&st, board);
        let k = shown
            .iter()
            .position(|f| &f.agent == agent)
            .ok_or(Cause::UnknownAgent)?;
        let facts = shown[k];
        let name = roster::names(&shown, self.label)?.swap_remove(k);
        let count = board.dropped(agent).map_or(0, |(n, _, _)| n);
        let dropped = roster::dropped_line(count)?;
        let history =
            match self
                .ports
                .agent_history(&facts.workflow.id, agent, self.limits.history_runs)
            {
                Ok(history) => history,
                Err(PortError::Busy) => {
                    return Ok(Card {
                        agent: agent.clone(),
                        name,
                        lines: dropped.into_iter().collect(),
                        runs: Vec::new(),
                        history: Some(BusyHistory::new()?),
                    })
                }
                Err(e) => return Err(cause_of(&e)),
            };
        let records: Vec<_> = history.iter().map(|h| h.run.clone()).collect();
        let after = board.ended_after_reap(facts, &records);
        let mut card = roster::card(
            after.as_ref().unwrap_or(facts),
            &name,
            &history,
            &self.limits,
        )?;
        if let Some(line) = dropped {
            card.lines.insert(0, line);
        }
        Ok(card)
    }

    /// The reply of `agents_snapshot` (I8.1).
    pub fn snapshot(&self) -> Snapshot {
        self.lock().sync.snapshot()
    }

    /// The reply of `agents_next` for a page that holds the state of `after` (I8.2).
    pub fn next(&self, after: u32) -> Next {
        self.lock().sync.next(after)
    }

    /// The reply of `agents_next` after `next_wait_s` with no change (I8.1); the whole state
    /// where this call is the one that publishes after a move that did not return (ruling
    /// RD-80 4.2), so that the page takes the raised `seq` with its state.
    pub fn same(&self) -> Snapshot {
        let mut st = Entered {
            st: self.state.lock().unwrap_or_else(PoisonError::into_inner),
            ports: &self.ports,
            entry: Entry::Read,
            unwinding: std::thread::panicking(),
            mark_before: mark_this_thread(Some((
                Arc::clone(&self.quit_mark),
                Arc::clone(&self.hold_mark),
                Arc::clone(&self.unstopped),
            ))),
        };
        if st.mark.unpublished {
            self.publish(&mut st);
            return st.sync.snapshot();
        }
        st.sync.same()
    }

    fn publish(&self, st: &mut State<Cfg<P>>) {
        Self::follow_taken_back(st);
        let State { board, ledgers, .. } = &mut *st;
        if let Some(board) = board.as_mut() {
            for (agent, l) in ledgers.iter_mut() {
                for (event, refusal) in l.taken_off() {
                    board.count_dropped(agent, &event, &refusal);
                }
                if l.take_acked() {
                    board.acknowledged(agent);
                }
            }
        }
        if let Some(b) = st.board.as_ref() {
            let mut listed = self
                .unstopped
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let mut named: Vec<(u64, AgentId, RunId)> = b
                .runs
                .iter()
                .map(|r| (r.rank, r.agent.clone(), r.run.clone()))
                .collect();
            for d in &b.dying {
                if let Some((agent, _)) = listed.iter().find(|(_, run)| run == &d.run) {
                    named.push((d.rank, agent.clone(), d.run.clone()));
                }
            }
            named.sort_by_key(|(rank, _, _)| *rank);
            *listed = run::kept_runs(b, &st.killed_kept)
                .into_iter()
                .chain(named.into_iter().map(|(_, agent, run)| (agent, run)))
                .collect();
            self.live_runs.store(b.runs.len(), Ordering::Relaxed);
            *self.flows.lock().unwrap_or_else(PoisonError::into_inner) = b
                .members
                .iter()
                .map(|m| (m.facts.agent.clone(), m.facts.workflow.id.clone()))
                .collect();
        }
        Self::keep_check_fields(st);
        self.keep_amounts_fields(st);
        let (rows, items, note) = self.compose(st);
        let needs = rows.iter().filter(|r| r.status == Status::NeedsYou).count();
        let changed = if st.mark.unpublished {
            st.mark.unpublished = false;
            st.sync.publish_again(rows, items, note);
            true
        } else {
            st.sync.publish(rows, items, note)
        };
        if changed {
            let _ = self.ports.changed();
            let _ = self
                .ports
                .attention(u32::try_from(needs).unwrap_or(u32::MAX));
        }
    }

    /// Keeps, for each open check card, the entries of the roster's key `unknown` that the
    /// newest read naming them gave, and forgets those of a card that is no longer open: so the
    /// card shows the kept fields of the entry that made its hold stand also in a move whose
    /// roster read did not answer, or whose read no longer names that entry (rulings RD-81 8.3,
    /// RD-135 item 4). The fields live in this life's state alone.
    fn keep_check_fields(st: &mut State<Cfg<P>>) {
        let State { board, checks, .. } = st;
        let open: Vec<(ItemId, &Vec<Unknown>)> = board.as_ref().map_or_else(Vec::new, |b| {
            b.members
                .iter()
                .filter_map(|m| {
                    let item = m.facts.doing.item()?;
                    (item.kind == ItemKind::Check).then_some((item.item, &m.facts.unknown))
                })
                .collect()
        });
        checks.retain(|(item, _)| open.iter().any(|(i, _)| i == item));
        for (item, unknown) in open {
            if unknown.is_empty() {
                continue;
            }
            match checks.iter_mut().find(|(i, _)| *i == item) {
                Some((_, kept)) => kept.clone_from(unknown),
                None => checks.push((item, unknown.clone())),
            }
        }
    }

    /// Keeps, for each open item `amounts`, the committed payment or order of the agent's newest
    /// run whose page afterwards could not be matched, as one read of the agent's history gives
    /// it, and forgets the entry of an item that is no longer open (r4 U3.29). A read that does
    /// not answer keeps nothing, and the next publish reads again.
    fn keep_amounts_fields(&self, st: &mut State<Cfg<P>>) {
        let State { board, amounts, .. } = st;
        let open: Vec<(ItemId, &Facts<Cfg<P>>)> = board.as_ref().map_or_else(Vec::new, |b| {
            b.members
                .iter()
                .filter_map(|m| {
                    let item = m.facts.doing.item()?;
                    (item.kind == ItemKind::Amounts).then_some((item.item, &m.facts))
                })
                .collect()
        });
        amounts.retain(|(item, _)| open.iter().any(|(i, _)| i == item));
        for (item, facts) in open {
            if amounts.iter().any(|(i, _)| *i == item) {
                continue;
            }
            let Some(newest) = facts.runs.first() else {
                continue;
            };
            let Ok(history) = self
                .ports
                .agent_history(&facts.workflow.id, &facts.agent, 1)
            else {
                continue;
            };
            let differs = history
                .iter()
                .filter(|h| h.run.run == newest.run)
                .flat_map(|h| &h.effects)
                .find_map(|e| match e.page {
                    Some(PageCause::AmountDiffers { amount })
                        if e.state == guard::EffectState::Committed
                            && matches!(e.class, Class::Payment | Class::Purchase) =>
                    {
                        Some(Differs {
                            class: e.class,
                            kept: e.kept.clone(),
                            amount,
                        })
                    }
                    _ => None,
                });
            amounts.push((item, differs));
        }
    }

    fn compose(&self, st: &State<Cfg<P>>) -> (Vec<Row>, Vec<Item>, Option<String>) {
        let Some(board) = st.board.as_ref().filter(|_| st.returned) else {
            return (
                Vec::new(),
                Vec::new(),
                words::text("note.no_service", &[]).ok(),
            );
        };
        let mut shown = Self::shown(st, board);
        let still: Vec<&Facts<Cfg<P>>> = board
            .members
            .iter()
            .map(|m| &m.facts)
            .filter(|f| board.gone.contains(&f.workflow.id) && matches!(f.doing, Doing::Live(_)))
            .chain(board.members.iter().map(|m| &m.facts).filter(|f| {
                board.gone.contains(&f.workflow.id)
                    && !matches!(f.doing, Doing::Live(_))
                    && board.alive_at_reap(&f.agent).is_some()
            }))
            .chain(board.kept_gone())
            .collect();
        shown.extend(still.iter().copied());
        shown.sort_by(|a, b| a.agent.cmp(&b.agent));
        let Ok(names) = roster::names(&shown, self.label) else {
            let note = st
                .mark
                .tick
                .then(|| words::text("note.out_of_date", &[]).ok())
                .flatten();
            return (Vec::new(), Vec::new(), note);
        };
        let listed: Vec<(&AgentId, &str)> = shown
            .iter()
            .map(|f| &f.agent)
            .zip(names.iter().map(String::as_str))
            .collect();
        let mut placed: Vec<(Placed, Option<Item>)> = Vec::new();
        for (facts, name) in shown.iter().zip(&names) {
            if still.iter().any(|f| f.agent == facts.agent) {
                let stuck = board
                    .runs
                    .iter()
                    .any(|r| r.agent == facts.agent && r.kill_refused)
                    || board.alive_at_reap(&facts.agent).is_some()
                    || board.kept_gone().iter().any(|f| f.agent == facts.agent);
                let quit = roster::not_stopped_at_quit(facts);
                let recorded =
                    quit.and_then(|r| roster::recorded_end(facts, r, board.answered_alive(r)));
                let row = match (quit, recorded) {
                    (Some(run), Some(end)) => {
                        let read = roster::with_end(facts, run, end);
                        let line = roster::status_of(&read, &self.limits, None, false);
                        roster::gone_ended_row(&read, line, name)
                    }
                    (Some(_), None) => {
                        roster::quit_row(*facts, name, Vec::new()).map(|r| Row { next: None, ..r })
                    }
                    (None, _) => roster::gone_row(*facts, stuck, System::HOST, name),
                };
                if let Ok(row) = row {
                    placed.push((Placed { row, raised: None }, None));
                }
                continue;
            }
            let open = facts.doing.item();
            let held = open
                .as_ref()
                .and_then(|o| st.held.iter().find(|h| h.item() == &o.item));
            let quit = roster::not_stopped_at_quit(facts);
            let recorded =
                quit.and_then(|r| roster::recorded_end(facts, r, board.answered_alive(r)));
            let mut after = board.ended_after_reap(facts, &facts.runs);
            if let (Some(run), Some(end)) = (quit, recorded) {
                after = Some(roster::with_end(after.as_ref().unwrap_or(facts), run, end));
            }
            if let Some(read) = after.as_mut() {
                if Self::ready_only_by_interrupted(facts, read) {
                    read.doing = Doing::Idle(None);
                }
            }
            let mut line = match roster::status_of(
                after.as_ref().unwrap_or(*facts),
                &self.limits,
                held.map(|h| h.confirmation().state()),
                board.record_stuck(&facts.agent, &self.limits),
            ) {
                line @ crate::types::StatusLine::Record => line,
                _ if board.alive_at_reap(&facts.agent).is_some() => {
                    crate::types::StatusLine::KillStuck(System::HOST)
                }
                line => line,
            };
            let owed = board.setup_owed(&facts.agent)
                && !board.workflow_dismissed(&facts.workflow.id)
                && line.status() == Status::AllSet;
            if owed {
                line = crate::types::StatusLine::NeedsYou(ItemKind::Setup);
            }
            let waiting = board
                .queue
                .entries()
                .iter()
                .find(|w| w.agent == facts.agent)
                .map(|w| w.trigger);
            let quit_line = quit.is_some()
                && recorded.is_none()
                && !matches!(
                    line,
                    crate::types::StatusLine::Record
                        | crate::types::StatusLine::NeedsYou(ItemKind::Check)
                );
            let session = board
                .members
                .iter()
                .any(|m| m.facts.agent == facts.agent && m.session);
            let row = if st.deleting.iter().any(|d| d.agent == facts.agent) {
                roster::deleting_row(*facts, name)
            } else if quit_line {
                let offered = roster::controls(*facts, waiting, &self.limits);
                let offered = roster::with_stop_sign_in(*facts, session, offered);
                roster::quit_row(*facts, name, roster::with_model(*facts, offered))
            } else {
                roster::row_with_session(*facts, session, line, name, waiting, &self.limits)
            };
            let Ok(mut row) = row else {
                continue;
            };
            let asks_again = st.taken_back.iter().any(|(a, item)| {
                a == &facts.agent
                    && item.is_some()
                    && open.as_ref().map(|o| &o.item) == item.as_ref()
            });
            if asks_again && !quit_line && line == StatusLine::NeedsYou(ItemKind::Ready) {
                if let Ok(text) = words::text("line.needs_you.asks_again", &[]) {
                    row.line = text;
                }
            }
            // While the raise of its setup is owed the row has no card, so its line says that the
            // questions could not be shown yet (U3.12 (B); RD-155 (2), way B), unless the row is
            // the quit row, whose line says to check outside Halvo first (r4 U3.13).
            if owed && !quit_line && !st.deleting.iter().any(|d| d.agent == facts.agent) {
                if let Ok(text) = words::text("line.needs_you.setup_owed", &[]) {
                    row.line = text;
                }
            }
            let item = match (&open, line.status()) {
                (Some(o), Status::NeedsYou) if !quit_line => {
                    self.item_of(st, facts, held, (o.kind, &o.item), name, &listed)
                }
                _ => None,
            };
            placed.push((
                Placed {
                    row,
                    raised: roster::raised_at(facts),
                },
                item,
            ));
        }
        let mut rows: Vec<Placed> = placed.iter().map(|(p, _)| p.clone()).collect();
        roster::order(&mut rows);
        let named: Vec<(&str, Item)> = rows
            .iter()
            .filter_map(|p| {
                placed
                    .iter()
                    .find(|(q, _)| q.row.agent == p.row.agent)
                    .and_then(|(_, item)| item.clone())
                    .map(|item| (p.row.name.as_str(), item))
            })
            .collect();
        let pages = named
            .iter()
            .filter(|(_, i)| {
                i.kind() == ItemKind::Confirm && i.presenter() == Some(Presenter::Page)
            })
            .count();
        let mut n = 0;
        let items = named
            .into_iter()
            .map(|(name, item)| {
                if pages < 2 || item.presenter() != Some(Presenter::Page) {
                    return item;
                }
                n += 1;
                item.clone().placed(name, n, pages).unwrap_or(item)
            })
            .collect();
        let note = if st.mark.tick {
            words::text("note.out_of_date", &[]).ok()
        } else if board.roster_full {
            words::text("note.roster_full", &[]).ok()
        } else {
            None
        };
        (rows.into_iter().map(|p| p.row).collect(), items, note)
    }

    /// Whether the open `ready` item of `facts` is the stored raise of a run that a record of the
    /// store names `completed` or `nothing_to_do` (`Board::raise_of_a_finished_run`, with `read`,
    /// the facts with the ends that the row reads, from `Board::ended_after_reap`) while no new
    /// slot has opened for it or while a hold of the agent stands (`schedule::holds` of `facts`).
    /// Under any hold the stored item stays hidden, at a new slot too, whatever came first. A new
    /// slot has opened when the agent's current slot is open by `slot_open` on `read`, where that
    /// run's own end serves the slot in which it started; at such a slot with no hold standing,
    /// the item stands for the ask of I5.3 for the next run. A named difference: where the slot
    /// opened first and a hold came after, the ordinary way keeps its ready card under the hold,
    /// and this rule hides the item until the hold ends. Such an item is not shown, not counted
    /// and not given to a page, so that a task that the record names finished is never offered as
    /// if it had not run while it is hidden. The rule asks no tick. Nothing is stored, and the item
    /// stays open until a start of the person or the next life's recovery ends it (RD-156 (1),
    /// RD-162 (1), RD-165).
    fn ready_only_by_interrupted(facts: &Facts<Cfg<P>>, read: &Facts<Cfg<P>>) -> bool {
        let new_slot = schedule::current_slot(&read.workflow, read.today)
            .is_some_and(|s| schedule::slot_open(read, s));
        Board::raise_of_a_finished_run(facts, read)
            && (!new_slot || !schedule::holds(facts).is_empty())
    }

    /// The open item `item` of the kind `kind` of the agent of `facts`, whose row has the name
    /// `name`, as the page gets it; `listed` holds the name of every row, which the sentence of a
    /// set-up refused with `overlap` reads for the other agent (I3.5 (c)).
    fn item_of(
        &self,
        st: &State<Cfg<P>>,
        facts: &Facts<Cfg<P>>,
        held: Option<&Held>,
        (kind, item): (ItemKind, &ItemId),
        name: &str,
        listed: &[(&AgentId, &str)],
    ) -> Option<Item> {
        let agent = &facts.agent;
        let plain = |p: Plain| compose_plain(item, agent, p, name).ok();
        match kind {
            ItemKind::Confirm => held
                .and_then(|h| h.page_item().ok())
                .and_then(|i| i.said_by(name).ok()),
            ItemKind::SignIn => match st.binds.iter().find(|b| b.item() == item) {
                Some(b) => b.page_item(name).ok(),
                None => {
                    let ask = st.handbacks.iter().find(|(i, _)| i == item).map(|(_, a)| a);
                    compose_sign_in(item, agent, name, ask).ok()
                }
            },
            ItemKind::LinkAccount => st
                .handbacks
                .iter()
                .find(|(i, _)| i == item)
                .and_then(|(_, a)| a.origin.as_ref())
                .and_then(|origin| compose_link_account(item, agent, name, origin).ok()),
            ItemKind::Stuck => plain(Plain::Stuck),
            ItemKind::Ready => plain(Plain::Ready),
            ItemKind::Show => plain(Plain::Show),
            ItemKind::Setup => st.setups.iter().find(|s| s.item() == item).and_then(|s| {
                s.page_item(name, |a| Self::row_name(listed, a), &self.limits)
                    .ok()
            }),
            ItemKind::Check => {
                let kept = st
                    .checks
                    .iter()
                    .find(|(i, _)| i == item)
                    .map_or(&[][..], |(_, kept)| kept);
                let unknown: &[_] = match &facts.doing {
                    Doing::Idle(_) | Doing::Queued(_) if facts.unknown.is_empty() => kept,
                    Doing::Idle(_) | Doing::Queued(_) => &facts.unknown,
                    Doing::Live(_) => &[],
                };
                compose_check(item, agent, name, unknown).ok()
            }
            ItemKind::Amounts => {
                let differs = st
                    .amounts
                    .iter()
                    .find(|(i, _)| i == item)
                    .and_then(|(_, d)| d.as_ref());
                let category = PayClass::of(facts.workflow.category);
                compose_amounts(item, agent, name, category, differs).ok()
            }
        }
    }
}

impl<P: Owner> std::fmt::Debug for Coordinator<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Coordinator")
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod setup_tests;
#[cfg(test)]
mod tests;
