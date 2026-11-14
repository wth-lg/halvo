use crate::guard::{self, EffectRecord, Policy, Refusal, Table};
use crate::items::drive::{self as items, Held, HeldBind, Ledger, SetupDone};
use crate::items::{
    compose_check, compose_link_account, compose_plain, params, raise_confirm, raise_step,
    ConfirmAnswer, ConfirmMove, Plain, Raise, Setup, SetupInput, SetupParam, Source,
};
use crate::ports::{Analysis, Clock, Owner, Page, PortError, Rng, Runs, Tray};
use crate::roster::{self, Placed};
use crate::run::{self, Act, Done, StopBy};
use crate::schedule::{self, Board, Move, Report};
use crate::sync::{Next, Sync};
use crate::types::{
    Action, AgentId, CandidateFrom, Card, Cause, CheckAnswer, Choice, ConfigView, Doing, Facts,
    FullWorkflow, HandbackAsk, Item, ItemId, ItemKind, LapseCause, Limits, Live, OtherLapse,
    OwnItemKind, Param, Presenter, Row, RunEnd, RunEvent, RunId, RunItem, RunItemKind, Snapshot,
    Status,
};
use crate::words;
use std::sync::{Mutex, MutexGuard, PoisonError};

/// What the app's quit hook waits for after the first step of the quit (I5.7, ruling RD-8): the
/// latest time, in milliseconds of `Clock::now`, at which it calls the second step,
/// `cancel_grace_s` after the stop of each run that is still live, where the stop of a run that
/// was stopped before the quit counts from that stop; the hook calls it sooner once no run is
/// live (`Coordinator::live`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuitWait {
    /// The time of the second step at the latest.
    pub until: u64,
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

/// Who answers an item, and when: the step mark that the page showed, or none for the native
/// dialog, whose item keeps its one step (I3.4); the presenter; and the clock as the answer read
/// it before the coordinator's lock was taken (ruling RD-80 5.3).
#[derive(Clone, Copy)]
struct By {
    step: Option<u32>,
    from: Presenter,
    now: Option<u64>,
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

/// The items ledger of the agent of `facts`, which takes the owner's events that the board read
/// after its last acknowledged one, the coordinator's read right after a call that answered no
/// acknowledgement among them, as its own read after a refusal `seq` takes them (ruling RD-40).
fn ledger<'a, C>(ledgers: &'a mut Vec<(AgentId, Ledger)>, facts: &Facts<C>) -> &'a mut Ledger {
    let k = match ledgers.iter().position(|(a, _)| a == &facts.agent) {
        Some(k) => k,
        None => {
            let last = facts.events.last().map_or(0, |e| e.seq.get());
            let fresh = Ledger::new(facts.workflow.id.clone(), facts.agent.clone(), last);
            ledgers.push((facts.agent.clone(), fresh));
            ledgers.len() - 1
        }
    };
    ledgers[k].1.take(&facts.events);
    &mut ledgers[k].1
}

/// Holds the stop of the live run `run` in the guard's table and on the board, from `now` and
/// with the cause of `by`, before any port call of the stop (rulings RD-34 (e), RD-48 (a)).
fn mark<C>(board: &mut Board<C>, table: &mut Table, run: &RunId, now: u64, by: StopBy) {
    *table = table.stop(run);
    schedule::stop_run(board, run, now, by.cause().end());
}

/// The open item that a waiting run of the agent waits on, or none.
fn waiting_on(doing: &Doing) -> Option<RunItem> {
    match doing {
        Doing::Live(Live::Waiting(open)) => Some(open.clone()),
        _ => None,
    }
}

/// The live run of the agent that no stop has reached, or none.
fn reachable<C>(board: &Board<C>, agent: &AgentId) -> Option<RunId> {
    board
        .runs
        .iter()
        .find(|r| &r.agent == agent && r.stop.is_none())
        .map(|r| r.run.clone())
}

/// The coordinator's read of an agent's events inside an items move, which the items drive calls
/// after a refusal `seq` or a log with no acknowledgement (rulings RD-52 (b) rule 3, (c) rule
/// 1): it takes the owner's events into the board's facts and the ledger, and gives `Off` where
/// they first show the agent's hold `off` beside its live run that no stop has reached, so that
/// the move stops the run first and stores nothing more for that agent.
struct Reading<'a, P, C> {
    ports: &'a P,
    board: &'a mut Board<C>,
    i: usize,
    off_before: bool,
    made: bool,
    stopped: bool,
}

impl<'a, P: Owner, C: ConfigView> Reading<'a, P, C> {
    fn new(ports: &'a P, board: &'a mut Board<C>, i: usize) -> Reading<'a, P, C> {
        let off_before = schedule::holds(&board.members[i].facts).contains(&schedule::Hold::Off);
        Reading {
            ports,
            board,
            i,
            off_before,
            made: false,
            stopped: false,
        }
    }

    fn read(&mut self, l: &mut Ledger) -> items::Read {
        self.made = true;
        let facts = &mut self.board.members[self.i].facts;
        let read = schedule::resync(self.ports, &facts.agent, &mut facts.events);
        l.take(&facts.events);
        let agent = facts.agent.clone();
        let off = schedule::holds(facts).contains(&schedule::Hold::Off);
        self.stopped = !self.off_before && off && reachable(self.board, &agent).is_some();
        match (read, self.stopped) {
            (Err(_), _) => items::Read::Failed,
            (Ok(()), true) => items::Read::Off,
            (Ok(()), false) => items::Read::Taken,
        }
    }
}

/// The parts of the state that the lapse of a run's open item writes, beside the board.
struct Parts<'a> {
    table: &'a mut Table,
    ledgers: &'a mut Vec<(AgentId, Ledger)>,
    held: &'a mut Vec<Held>,
    binds: &'a mut Vec<HeldBind>,
    handbacks: &'a mut Vec<(ItemId, HandbackAsk)>,
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
    P: Owner + Runs + Analysis + Clock + Rng + Tray + Page + Policy<Config = Cfg<P>>,
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
            }),
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

    fn lock(&self) -> MutexGuard<'_, State<Cfg<P>>> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// One tick (I5.9): the recovery of I5.8 at the first, the creation move of I5.5, the run
    /// drive's tick, then each setup's owed body (`tick_setup`, dropped at `Closed` or `Gone`),
    /// each held confirmation's time (`items::drive::tick`), whose lapse with `wait_limit` stops
    /// its run, each second step's and each other item's wait limit. After the first step of the
    /// quit a tick does only the stages that start nothing (ruling RD-34 (d)): it sends the rows
    /// of the items' ledgers and makes the run drive's `QuitTick`, which kills each stopping run
    /// and dying tree at its grace with nothing stored and sends what the board owes.
    pub fn tick(&self) {
        let mut st = self.lock();
        if st.quitting {
            let State { board, ledgers, .. } = &mut *st;
            if let Some(board) = board.as_mut() {
                self.flush_ledgers(board, ledgers);
            }
            let _ = self.act(&mut st, Act::QuitTick);
        } else {
            let before = Self::offs(&st);
            self.tick_in(&mut st, &before);
            self.after_reads(&mut st, &before);
        }
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
            let _ = items::flush(&self.ports, l, &self.limits);
            if let Some(i) = index(board, agent) {
                self.read_back(board, i);
            }
        }
    }

    /// The stages of a tick (I5.9). A switch-off that the read after the tick's own flush first
    /// shows is done right after that read, before the tick's other stages store or send
    /// anything for that agent (ruling RD-52 (b) rule 3); `before` are the agents that stood off
    /// before the tick.
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
            schedule::drive(&self.ports, board, &self.limits, Move::Create);
            self.flush_ledgers(board, ledgers);
        }
        self.after_reads(st, before);
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
        let done = run::drive(&self.ports, board, table, &self.limits, Act::Tick);
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
            if let Ok(setup) = Setup::new(
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
            let done = match board.as_ref() {
                Some(board) => match index(board, setups[k].agent()) {
                    Some(i) => {
                        let facts = &board.members[i].facts;
                        let l = ledger(ledgers, facts);
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

    /// Takes what a move of the setup `k` answered; whether the setup stays.
    fn setup_done(&self, st: &mut State<Cfg<P>>, k: usize, done: Result<SetupDone, Cause>) -> bool {
        let agent = st.setups[k].agent().clone();
        let stays = matches!(
            done,
            Ok(SetupDone::Open | SetupDone::Owed | SetupDone::NotTaken)
        ) || matches!(done, Err(cause) if cause != Cause::Stale);
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
            }
        }
        if matches!(done, Ok(SetupDone::Gone | SetupDone::AgentGone)) {
            st.forgotten.push(agent);
        }
        false
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
            if let Ok(now) = self.ports.now() {
                let lapses = matches!(
                    items::due(h, now, &self.limits),
                    Some(ConfirmMove::Lapse(LapseCause::WaitLimit))
                );
                if lapses {
                    mark(board, table, h.ask().run(), now, StopBy::WaitLimit);
                }
            }
            let l = ledger(ledgers, &board.members[i].facts);
            let before = (l.last(), l.waiting().len());
            let _ = items::tick(&self.ports, l, table, h, &self.limits);
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
        let Ok(now) = self.ports.now() else { return };
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
            mark(board, table, &w.run, now, StopBy::WaitLimit);
            let l = ledger(ledgers, &board.members[i].facts);
            let _ = items::time_out(&self.ports, l, table, &w.run, &w.item, &self.limits);
            self.read_back(board, i);
            handbacks.retain(|(item, _)| item != &w.item.item);
            false
        });
    }

    fn tick_binds(&self, st: &mut State<Cfg<P>>) {
        let Ok(now) = self.ports.now() else { return };
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
            mark(board, table, &b.ask().run, now, StopBy::WaitLimit);
            let l = ledger(ledgers, &board.members[i].facts);
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
        let _ = schedule::resync(&self.ports, &facts.agent, &mut facts.events);
    }

    /// A run's event (I5.10): `hello`; a request, which raises its item through `raise_confirm`
    /// with the guard's table of this life for a confirm request (ruling IG-32), and through
    /// `raise_step` for any other, and moves the run to `waiting`; the second step of a `sign_in`
    /// item (`Bind`); and the runner's end, before which the run's open item lapses with
    /// `run_ended`. A request that ends the run (`kind`, `budget`) shows no card, stops the run
    /// with that cause and is answered `state`.
    pub fn on_run(&self, run: &RunId, event: RunEvent) -> Result<(), Cause> {
        let mut st = self.lock();
        let before = Self::offs(&st);
        let out = self.on_run_in(&mut st, run, &event);
        self.after_reads(&mut st, &before);
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
                self.lapse_binds(st, i, run, OtherLapse::RunEnded);
                st.handbacks.retain(|(_, a)| &a.run != run);
                st.waits.retain(|w| &w.run != run);
                self.act(
                    st,
                    Act::End {
                        run: run.clone(),
                        outcome: *outcome,
                    },
                )
            }
            RunEvent::Confirm(_) | RunEvent::Handback(_) | RunEvent::Bind(_) | RunEvent::Other => {
                let facts = &board.members[i].facts;
                let raise = match event {
                    RunEvent::Confirm(ask) => raise_confirm(facts, &st.table, ask, &self.limits),
                    _ => raise_step(facts, Source::of(event).ok_or(Cause::State)?, &self.limits),
                };
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
                        let l = ledger(ledgers, &board.members[i].facts);
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
                            raised_at: self.ports.now().ok(),
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
        let l = ledger(ledgers, &board.members[i].facts);
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
        let l = ledger(ledgers, &board.members[i].facts);
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
        let State { board, table, .. } = st;
        let board = board.as_mut().ok_or(Cause::State)?;
        let done: Done = run::drive(&self.ports, board, table, &self.limits, act);
        answered(&done.report)
    }

    /// The lapse of the open item of the waiting run `run`: with `run_ended` at the runner's end;
    /// with `cancelled` for a stop, whose run the caller holds stopped in the guard's table and
    /// on the board first, and whose lapse sends `Runs::cancel` (ruling RD-48 (a)).
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
            ..
        } = st;
        let Some(board) = board.as_mut() else { return };
        let mut parts = Parts {
            table,
            ledgers,
            held,
            binds,
            handbacks,
        };
        self.lapse_open(board, &mut parts, i, run, open, stop);
    }

    fn lapse_open(
        &self,
        board: &mut Board<Cfg<P>>,
        parts: &mut Parts<'_>,
        i: usize,
        run: &RunId,
        open: &RunItem,
        stop: bool,
    ) {
        let Parts {
            table,
            ledgers,
            held,
            binds,
            handbacks,
        } = parts;
        let l = ledger(ledgers, &board.members[i].facts);
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
                if stop {
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

    /// The switch-off of the agent `i`, or with `Move::Off` its rest after a read in this move
    /// showed the hold (I5.7, ruling RD-52 (b)): the schedule stores the hold first and stops
    /// nothing until it is known stored, then holds the run's stop on the board; the halt then
    /// holds it in the guard's table and lapses the run's open item with `cancelled`, whose lapse
    /// sends `Runs::cancel`, which the schedule sends for a run that waits on no item; then
    /// `set_state` `paused` (ruling RD-48 (a)).
    fn off(&self, st: &mut State<Cfg<P>>, i: usize, mv: Move) -> Result<(), Cause> {
        let State {
            board,
            table,
            ledgers,
            held,
            binds,
            handbacks,
            waits,
            ..
        } = st;
        let board = board.as_mut().ok_or(Cause::NoService)?;
        let open = waiting_on(&board.members[i].facts.doing);
        let mut parts = Parts {
            table,
            ledgers,
            held,
            binds,
            handbacks,
        };
        let mut halt = |board: &mut Board<Cfg<P>>, i: usize, run: &RunId| {
            *parts.table = parts.table.stop(run);
            let Some(open) = &open else { return false };
            self.lapse_open(board, &mut parts, i, run, open, true);
            true
        };
        let report = schedule::drive_with(&self.ports, board, &self.limits, mv, Some(&mut halt));
        waits.retain(|w| !report.stopped.contains(&w.run));
        answered(&report)
    }

    /// A run's request whose raise met a read that first shows the agent switched off beside the
    /// run (RD-52 (b) rule 3): no item is raised, the rest of the switch-off stops the run in this
    /// move, and the request is answered `state`, as a request that ends its run.
    fn stopped_at_raise(&self, st: &mut State<Cfg<P>>, i: usize) -> Result<(), Cause> {
        let agent = st
            .board
            .as_ref()
            .map(|b| b.members[i].facts.agent.clone())
            .ok_or(Cause::State)?;
        self.off(st, i, Move::Off { agent })?;
        Err(Cause::State)
    }

    /// The agents whose hold `off` stands in the board's events.
    fn offs(st: &State<Cfg<P>>) -> Vec<AgentId> {
        st.board.as_ref().map_or_else(Vec::new, |b| {
            b.members
                .iter()
                .filter(|m| schedule::holds(&m.facts).contains(&schedule::Hold::Off))
                .map(|m| m.facts.agent.clone())
                .collect()
        })
    }

    /// The rest of the switch-off for each agent whose hold `off` a read of this move brought in
    /// beside a live run that no stop has reached, an agent of `before` having stood off before
    /// the move (I5.7, ruling RD-52 (b) rule 3).
    fn after_reads(&self, st: &mut State<Cfg<P>>, before: &[AgentId]) {
        for agent in Self::offs(st) {
            let Some(board) = st.board.as_ref() else {
                return;
            };
            let (Some(i), false) = (index(board, &agent), before.contains(&agent)) else {
                continue;
            };
            if reachable(board, &agent).is_some() {
                let _ = self.off(st, i, Move::Off { agent });
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
        let l = ledger(ledgers, &board.members[i].facts);
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
            ..
        } = st;
        let Some(board) = board.as_mut() else { return };
        let l = ledger(ledgers, &board.members[i].facts);
        for h in held.iter_mut().filter(|h| h.ask().run() == run) {
            if h.confirmation().state().is_open() {
                let _ = items::lapse(&self.ports, l, table, h, cause, &self.limits);
            }
        }
        held.retain(|h| h.ask().run() != run);
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
        let l = ledger(ledgers, &board.members[i].facts);
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
    pub fn answer(
        &self,
        item: &ItemId,
        choice: Choice,
        pick: Option<(Param, u32)>,
        step: u32,
    ) -> Result<(), Cause> {
        let now = self.ports.now().ok();
        let mut st = self.lock();
        let before = Self::offs(&st);
        let by = By {
            step: Some(step),
            from: Presenter::Page,
            now,
        };
        let out = self.answer_in(&mut st, item, choice, pick, by);
        self.after_reads(&mut st, &before);
        self.publish(&mut st);
        out
    }

    /// The answer of the native dialog of a confirmation (I4.5), whose item keeps its one step.
    pub fn answer_native(&self, item: &ItemId, answer: ConfirmAnswer) -> Result<(), Cause> {
        let choice = match answer {
            ConfirmAnswer::Allow => Choice::Allow,
            ConfirmAnswer::Reject => Choice::Reject,
        };
        let now = self.ports.now().ok();
        let mut st = self.lock();
        let before = Self::offs(&st);
        let by = By {
            step: None,
            from: Presenter::Native,
            now,
        };
        let out = self.answer_in(&mut st, item, choice, None, by);
        self.after_reads(&mut st, &before);
        self.publish(&mut st);
        out
    }

    /// The step mark of the open item `item`: the setup's own, and 0 for every other kind, whose
    /// item never asks again under its id (I3.4, ruling RD-87 1).
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
        by: By,
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
                    let l = ledger(ledgers, &board.members[i].facts);
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
                        self.off(st, i, Move::Off { agent })?;
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
                (OwnItemKind::Setup, Choice::None) => self.setup(st, item, SetupInput::None, step),
                (OwnItemKind::Setup, Choice::Choose) => {
                    let (param, n) = pick.ok_or(Cause::Malformed)?;
                    self.setup(st, item, SetupInput::Choose { param, n }, step)
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
        by: By,
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
        let l = ledger(ledgers, &board.members[i].facts);
        let out = items::answer(&self.ports, l, table, h, answer, by.from, &self.limits);
        let closed = !h.confirmation().state().is_open();
        self.read_back(board, i);
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
        let l = ledger(ledgers, &board.members[i].facts);
        let out = items::answer_bind(&self.ports, l, table, &self.limits, b, choice);
        let closed = *b.state() == items::BindState::Closed;
        self.read_back(board, i);
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
        let board = board.as_ref().ok_or(Cause::State)?;
        let i = index(board, setups[k].agent()).ok_or(Cause::State)?;
        let facts = &board.members[i].facts;
        let l = ledger(ledgers, facts);
        let done = items::answer_setup(&self.ports, l, &self.limits, &mut setups[k], facts, input);
        let out = done.map(|_| ());
        self.setup_done(st, k, done);
        out
    }

    /// The text of a parameter of a setup item that has no candidate (I8.1
    /// `agents_setup_typed`), at the step mark `step` that the page showed, which is refused with
    /// `state` where it is no longer the item's (I3.4).
    pub fn setup_typed(
        &self,
        item: &ItemId,
        param: Param,
        text: &str,
        step: u32,
    ) -> Result<(), Cause> {
        let mut st = self.lock();
        let before = Self::offs(&st);
        let out = self.setup(&mut st, item, SetupInput::Typed { param, text }, step);
        self.after_reads(&mut st, &before);
        self.publish(&mut st);
        out
    }

    /// `agents_present` (I8.1): a confirmation whose presenter is native; `presenter` for the
    /// page's.
    pub fn present(&self, item: &ItemId) -> Result<(), Cause> {
        let st = self.lock();
        let h = st
            .held
            .iter()
            .find(|h| h.item() == item)
            .ok_or(Cause::UnknownItem)?;
        match h.confirmation().presenter() {
            Presenter::Native => Ok(()),
            Presenter::Page => Err(Cause::Presenter),
        }
    }

    /// The page's report that it opened the page confirmation `item` at the step mark `step`
    /// (I8.1 `agents_shown`, ruling RD-80 5.1, 5.2), from which an answer arms `arm_ms` later:
    /// the clock is read first, before the lock, and a clock that cannot be read is its cause, so
    /// that the page reports again; an id that is no item is `unknown_item`; an item of another
    /// presenter, a native confirmation or an item of another kind, is `presenter`, with no time
    /// noted; a confirmation that is answered or later is `stale`; a step that is not the item's
    /// is `state`. The first report counts, and a later one answers ok and changes nothing.
    pub fn shown_on_page(&self, item: &ItemId, step: u32) -> Result<(), Cause> {
        let now = self.ports.now().map_err(|e| cause_of(&e));
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
        let Ok(now) = self.ports.now() else { return };
        if let Some(h) = st.held.iter_mut().find(|h| h.item() == item) {
            items::on_screen(h, now);
        }
    }

    fn schedule(&self, st: &mut State<Cfg<P>>, mv: Move) -> Result<(), Cause> {
        let board = st.board.as_mut().ok_or(Cause::NoService)?;
        answered(&schedule::drive(&self.ports, board, &self.limits, mv))
    }

    fn start_in(&self, st: &mut State<Cfg<P>>, agent: &AgentId) -> Result<(), Cause> {
        if st.quitting {
            return Err(Cause::State);
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
    /// tick saw gone, or whose setup body the owner ended, is answered `unknown_agent` at once
    /// with nothing stored (I5.7, rulings IG-33, IG-36); after the first
    /// step of the quit every start is `state`. The person's `stop` lapses the open item of a
    /// waiting run with `cancelled` and stops the run in one command, for every kind of item,
    /// and answers success for an agent whose run is stopping or not live (ruling RD-6); the
    /// switch-off control lapses that item in the same way before its own move.
    pub fn control(&self, agent: &AgentId, action: Action) -> Result<(), Cause> {
        let mut st = self.lock();
        let before = Self::offs(&st);
        let out = self.control_in(&mut st, agent, action);
        self.after_reads(&mut st, &before);
        self.publish(&mut st);
        out
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
            Action::SwitchOn => self.schedule(
                st,
                Move::SwitchOn {
                    agent: agent.clone(),
                },
            ),
            Action::SwitchOff => self.off(
                st,
                i,
                Move::SwitchOff {
                    agent: agent.clone(),
                },
            ),
            Action::Stop => self.stop_in(st, i),
        }
    }

    /// The person's stop of the agent `i` (I5.7, rulings RD-6, RD-43, RD-48 (a)): nothing for an
    /// agent with no live run or a stopping one; for a waiting run the guard's table and the board
    /// hold the stop before its open item lapses with `cancelled`, whose lapse sends
    /// `Runs::cancel`; any other live run is stopped by the run drive, which holds the stop before
    /// the cancel.
    fn stop_in(&self, st: &mut State<Cfg<P>>, i: usize) -> Result<(), Cause> {
        let State { board, table, .. } = st;
        let board = board.as_mut().ok_or(Cause::NoService)?;
        let agent = board.members[i].facts.agent.clone();
        let Some(run) = reachable(board, &agent) else {
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
        let now = self.ports.now().map_err(|e| cause_of(&e))?;
        mark(board, table, &run, now, StopBy::Person);
        self.close_run_item(st, i, &run, &open, true);
        Ok(())
    }

    /// The first step of the app's quit (I5.7, ruling RD-8): every open item lapses with
    /// `app_ended` in the life that ends (I4.5), every live run that no stop has reached gets
    /// `cancel` through the run drive's stop-all, the queue is emptied, and from then on every
    /// start is refused, the person's, the queue's and a tick's. It returns at once, with the
    /// time at which the hook calls the second step at the latest: `cancel_grace_s` after the
    /// latest stop of a live run, where a stop that reads later than the first step counts from
    /// the first step (ruling RD-28 (e)), and the first step's time when no run is live.
    pub fn quit_stop(&self) -> QuitWait {
        let mut st = self.lock();
        st.quitting = true;
        let now = self.ports.now().unwrap_or(0);
        let members = st.board.as_ref().map_or(0, |b| b.members.len());
        for i in 0..members {
            self.lapse_at_quit(&mut st, i);
        }
        let _ = self.act(&mut st, Act::StopAll);
        let grace = self.limits.cancel_grace_s.saturating_mul(1000);
        let until = st
            .board
            .as_ref()
            .and_then(|b| {
                b.runs
                    .iter()
                    .filter_map(|r| r.stop.map(|s| s.at.min(now)))
                    .max()
            })
            .map_or(now, |at| at.saturating_add(grace));
        self.publish(&mut st);
        QuitWait { until }
    }

    /// The second step of the app's quit (I5.7, ruling RD-8), which the app's hook calls when no
    /// run is live any more or at `QuitWait::until`: sends the rows that still wait in the
    /// agents' ledgers, the lapses of the first step among them, kills every live run and every
    /// dying tree through the run drive's kill-all and stores no `run_ended`, so that recovery
    /// ends those runs `interrupted` in the next life.
    pub fn quit_kill(&self) {
        let mut st = self.lock();
        let State { board, ledgers, .. } = &mut *st;
        if let Some(board) = board.as_mut() {
            self.flush_ledgers(board, ledgers);
        }
        let _ = self.act(&mut st, Act::KillAll);
        self.publish(&mut st);
    }

    /// The count of live runs, for which the app's quit hook waits.
    pub fn live(&self) -> usize {
        self.lock().board.as_ref().map_or(0, |b| b.runs.len())
    }

    /// The agents whose hold `off` stands in the owner's events as the coordinator has read
    /// them beside a live run that no stop has reached: none after any move (I5.7, ruling RD-52
    /// (b)).
    pub fn off_beside_live(&self) -> Vec<AgentId> {
        let st = self.lock();
        let Some(board) = st.board.as_ref() else {
            return Vec::new();
        };
        Self::offs(&st)
            .into_iter()
            .filter(|a| reachable(board, a).is_some())
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

    /// The card of an agent (I8.1 `agents_card`, I2.6), read through `Owner::agent_history`,
    /// with the name of its row: two agents of one name get their ordinal among the agents whose
    /// rows the page shows (I2.3). An agent that has no row, whose workflow a tick saw gone or
    /// whose setup body the owner ended, is `unknown_agent`, as for every control.
    pub fn card(&self, agent: &AgentId) -> Result<Card, Cause> {
        let st = self.lock();
        let board = st.board.as_ref().ok_or(Cause::NoService)?;
        let shown = Self::shown(&st, board);
        let k = shown
            .iter()
            .position(|f| &f.agent == agent)
            .ok_or(Cause::UnknownAgent)?;
        let facts = shown[k];
        let name = roster::names(&shown, self.label)?.swap_remove(k);
        let history = self
            .ports
            .agent_history(&facts.workflow.id, agent, self.limits.history_runs)
            .map_err(|e| cause_of(&e))?;
        roster::card(agent, &facts.config, &name, &history, &self.limits)
    }

    /// The reply of `agents_snapshot` (I8.1).
    pub fn snapshot(&self) -> Snapshot {
        self.lock().sync.snapshot()
    }

    /// The reply of `agents_next` for a page that holds the state of `after` (I8.2).
    pub fn next(&self, after: u32) -> Next {
        self.lock().sync.next(after)
    }

    /// The reply of `agents_next` after `next_wait_s` with no change (I8.1).
    pub fn same(&self) -> Snapshot {
        self.lock().sync.same()
    }

    fn publish(&self, st: &mut State<Cfg<P>>) {
        let (rows, items, note) = self.compose(st);
        let needs = rows.iter().filter(|r| r.status == Status::NeedsYou).count();
        if st.sync.publish(rows, items, note) {
            let _ = self.ports.changed();
            let _ = self
                .ports
                .attention(u32::try_from(needs).unwrap_or(u32::MAX));
        }
    }

    fn compose(&self, st: &State<Cfg<P>>) -> (Vec<Row>, Vec<Item>, Option<String>) {
        let Some(board) = st.board.as_ref() else {
            return (
                Vec::new(),
                Vec::new(),
                words::text("note.no_service", &[]).ok(),
            );
        };
        let shown = Self::shown(st, board);
        let Ok(names) = roster::names(&shown, self.label) else {
            return (Vec::new(), Vec::new(), None);
        };
        let mut placed: Vec<(Placed, Option<Item>)> = Vec::new();
        for (facts, name) in shown.iter().zip(&names) {
            let open = facts.doing.item();
            let held = open
                .as_ref()
                .and_then(|o| st.held.iter().find(|h| h.item() == &o.item));
            let line = roster::status(*facts, &self.limits, held.map(|h| h.confirmation().state()));
            let Ok(row) = roster::row(*facts, line, name) else {
                continue;
            };
            let item = match (&open, line.status()) {
                (Some(o), Status::NeedsYou) => self.item_of(st, facts, held, o.kind, &o.item, name),
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
        let items = rows
            .iter()
            .filter_map(|p| {
                placed
                    .iter()
                    .find(|(q, _)| q.row.agent == p.row.agent)
                    .and_then(|(_, item)| item.clone())
            })
            .collect();
        let note = board
            .roster_full
            .then(|| words::text("note.roster_full", &[]).ok())
            .flatten();
        (rows.into_iter().map(|p| p.row).collect(), items, note)
    }

    fn item_of(
        &self,
        st: &State<Cfg<P>>,
        facts: &Facts<Cfg<P>>,
        held: Option<&Held>,
        kind: ItemKind,
        item: &ItemId,
        name: &str,
    ) -> Option<Item> {
        let agent = &facts.agent;
        let plain = |p: Plain| compose_plain(item, agent, p, name).ok();
        match kind {
            ItemKind::Confirm => held
                .and_then(|h| h.page_item().ok())
                .and_then(|i| i.said_by(name).ok()),
            ItemKind::SignIn => match st.binds.iter().find(|b| b.item() == item) {
                Some(b) => b.page_item(name).ok(),
                None => plain(Plain::SignIn),
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
            ItemKind::Setup => st
                .setups
                .iter()
                .find(|s| s.item() == item)
                .and_then(|s| s.page_item(name, &self.limits).ok()),
            ItemKind::Check => {
                let newest = facts.runs.first().map(|r| r.run.clone());
                let unknown = match (&facts.doing, newest) {
                    (Doing::Idle(_) | Doing::Queued(_), Some(run)) => self
                        .ports
                        .agents()
                        .ok()
                        .and_then(|all| all.into_iter().find(|a| &a.agent == agent))
                        .map(|a| a.unknown.into_iter().filter(|u| u.run == run).collect())
                        .unwrap_or_default(),
                    _ => Vec::new(),
                };
                compose_check(item, agent, name, &unknown).ok()
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
