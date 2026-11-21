use crate::args::Args;
use crate::bell::Bell;
use crate::wired::Wired;
use halvo_coord::guard::Policy;
use halvo_coord::items::ConfirmAnswer;
use halvo_coord::ports::{Analysis, Clock, Owner, Rng, Runs, Senders};
use halvo_coord::sync::Next;
use halvo_coord::types::commands::LIST;
use halvo_coord::types::{
    Action, AgentId, Card, Cause, Choice, ClosedReply, ConfigView, ErrorReply, ItemId, Limits,
    OkReply, Param, RunId, Snapshot, YesNo,
};
use halvo_coord::{words, Coordinator};
use serde::{Serialize, Serializer};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex, PoisonError, Weak};
use std::time::{Duration, Instant};

/// A reply of a command (I8.1), written as the page's fixtures write it (I8.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    /// The state: `agents_snapshot`, and `agents_next` at a change or after its wait.
    Snapshot(Snapshot),
    /// `agents_card`.
    Card(Card),
    /// `{"ok":"yes"}`.
    Ok,
    /// `{"closed":"yes"}` of `agents_next`.
    Closed,
    /// `{"error":"<cause>"}`.
    Error(Cause),
    /// A refused control of a row whose sentence says more than its cause
    /// (`Refusal::SwitchNotSaved`); a refusal with the sentence of its cause alone is `Error`.
    Refused(halvo_coord::types::Refusal),
}

impl Serialize for Reply {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Reply::Snapshot(state) => state.serialize(s),
            Reply::Card(card) => card.serialize(s),
            Reply::Ok => OkReply.serialize(s),
            Reply::Closed => ClosedReply { closed: YesNo::Yes }.serialize(s),
            Reply::Error(cause) => ErrorReply { error: *cause }.serialize(s),
            Reply::Refused(refusal) => refusal.serialize(s),
        }
    }
}

impl From<Result<(), Cause>> for Reply {
    fn from(out: Result<(), Cause>) -> Reply {
        match out {
            Ok(()) => Reply::Ok,
            Err(cause) => Reply::Error(cause),
        }
    }
}

/// The waits of each waiting call of `agents_next` (I8.2, ruling RD-74 point 1). A call whose
/// `after` is not the state's `seq` when it comes is answered at once, but no sooner than
/// `coalesce` after the last reply of `agents_next` with a state that the hub had sent when the
/// call looked. A call that waits is answered `coalesce` after the first change that it sees,
/// with the state that the coordinator holds then. After `next` with no change it answers its
/// own `after` with empty lists and no note, and reads the state no second time. Two calls that
/// wait at once each get their reply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Waits {
    /// `next_wait_s`.
    pub next: Duration,
    /// `coalesce_ms`.
    pub coalesce: Duration,
}

impl Waits {
    /// The waits of `limits`.
    pub fn of(limits: &Limits) -> Waits {
        Waits {
            next: Duration::from_secs(limits.next_wait_s),
            coalesce: Duration::from_millis(limits.coalesce_ms),
        }
    }
}

/// The hub's side that the app, the shell and the Tauri handler call, with no type of the
/// ports in it.
pub trait Core: Send + Sync {
    /// Runs the command of `LIST` named `command` with the arguments of its call, which are
    /// none when the call's body is no JSON; the reply of every refusal is
    /// `{"error":"<cause>"}`, so the call never fails. A name outside `LIST` answers
    /// `{"error":"malformed"}`. `agents_snapshot` and `agents_next`, to which `LIST` gives no
    /// error cause, are never refused (ruling RD-74 point 2): `agents_snapshot` answers the
    /// state whatever its arguments, and a call of `agents_next` whose arguments do not fit
    /// (an `after` that is no count, no `after`, an extra key or a body that is no JSON) is
    /// answered as a call with `after` 0, so that it is closed while the window is hidden and
    /// bound by `Waits` while it is shown. Until the first `tick` has returned (ruling RD-74
    /// point 3), `agents_snapshot` answers the stand-in state, `seq` 0 with empty lists and the
    /// note `note.no_service`, without taking the coordinator's lock, and `agents_next` waits
    /// for that return within its wait and under the hide rule, and else answers the stand-in;
    /// every other command is handed to the coordinator, which has no board yet and answers as
    /// it does with no service.
    fn call(&self, command: &str, args: Option<&Value>) -> Reply;

    /// The hook of the window `main` shown: from now on `agents_next` waits again.
    fn shown(&self);

    /// The hook of the window `main` hidden: every waiting `agents_next` answers
    /// `{"closed":"yes"}`, and so does each one until the window is shown again. No window
    /// event stands behind it, so the app calls it where it hides the window.
    fn hidden(&self);

    /// The hook of the window `main` destroyed, which answers as `hidden` does.
    fn destroyed(&self);

    /// The hook of the app's start and of each pass of the status poller: one tick of the
    /// coordinator (I5.9), whose first tick recovers the life (I5.8). After its first return it
    /// rings the bell, so that a waiting `agents_next` answers the first state. A tick made on
    /// the coordinator itself is not the hub's first tick, so the app ticks through this hook
    /// alone. A first tick that panics in a port leaves the stand-in until a tick returns.
    fn tick(&self);

    /// The hook before the capture run's quit function sends its pause (I5.7, I5.9, ruling
    /// RD-8). It first marks the quit (`Coordinator::mark_quit`), with no lock and no port call,
    /// so that from its call on every start that has not passed its last reading of the mark,
    /// directly before `Runs::start`, is refused, the person's, the queue's and a tick's, also in
    /// a move that holds the coordinator's lock then; a start that passed that reading before the
    /// call has written its run in the list that the hook returns. Then the first step of the
    /// coordinator's quit, then a wait in which no move of the coordinator is held open, until
    /// no run is live, at most until `cancel_grace_s` after the stop of each live run, counted
    /// from its stop for a run that was stopped before the quit, never longer than
    /// `cancel_grace_s` whatever the clock reads (RD-28 (e)), and at most `budget`, the time that
    /// the system gives the quit at a logout or a shutdown; then the second step. In the window
    /// of the second step the hook also writes the list of the runs that did not stop to the
    /// owner (`Coordinator::quit_write`), on a thread of its own, once the second step has
    /// returned and at the latest at half of that window, so that the next life reads the list
    /// also where a kill still holds when the hook returns (rulings RD-150, RD-154 point 2); a
    /// call whose first step has not returned within `cancel_grace_s` writes no list. The grace
    /// counts from the stop that the first step reads on the clock once it holds the
    /// coordinator's lock, and for a run stopped before the quit from its own stop; the hook
    /// takes the length of its wait from the first step, from the time that the step read
    /// (`QuitWait::at`) to `QuitWait::until`, and counts it from the return of that step on its
    /// own monotonic time, not from the step's read, so that a clock that fails after the first
    /// step, or at the step's own read, where the step takes the last read of the life, leaves
    /// the whole grace and a step of the clock back adds nothing; the budget counts from the
    /// hook's start. Each step runs on a thread of its own
    /// and the hook waits for it at most `cancel_grace_s` after it called it; the wait reads
    /// `Coordinator::live`, which takes no lock, and the hook's own thread calls no port, the
    /// clock included. So the hook stands behind the coordinator's lock and behind a port
    /// nowhere, and a kill that holds or never returns, of a tick or of either step, does not
    /// hold Quit (ruling RD-147 (5)(i)): the hook returns at the latest `cancel_grace_s` after
    /// its call when its first step has not returned by then, and otherwise at the latest
    /// `cancel_grace_s` after the end of its wait, which ends at the latest `cancel_grace_s`
    /// after the first step returned. When the first step has not returned within
    /// `cancel_grace_s`, that call sends no kill to all; and every run that got no kill stands in
    /// the list that it returns, so the answer stays true, the run of a start that the move
    /// holding the lock took past its last reading of the mark among them, which that start
    /// writes in the list before `Runs::start`. At a `cancel_grace_s` of 0, which the product does not set (`Limits::PRODUCT`
    /// holds 5), neither step is waited for. It returns the live runs and the dying trees that
    /// the board still holds, each with its agent, in the order of their starts, as the last
    /// write of the list left them (`Coordinator::not_stopped`): a run or a tree whose kill was
    /// refused, every run and tree that the board held before a move whose kill has not
    /// returned, and, where the first step could not take the lock in time, every one that the
    /// board held when the move that holds the lock began, with the run of each start that
    /// passed its last reading of the mark in that move; after a step that did not return, the
    /// next call of the coordinator writes the list again at its entry. A second call, during the wait or
    /// after a kill that holds, takes its own steps under the same bounds, returns at its own
    /// end and does not hold the first call. A step that has not returned goes on in its thread
    /// until the app's process ends, and closing the link ends the runner with the processes of
    /// its mark within the agent-setup run's `LINK_END` of 20 s; neither a step nor a kill starts
    /// that clock. A port's panic in a step ends that step's thread, and the hook then returns
    /// at once; where no thread can be started for a step, the hook takes that step no further
    /// and returns. The next life's reap is then the kill (RD-34 (e)).
    fn quit(&self, budget: Option<Duration>) -> Vec<(AgentId, RunId)>;

    /// The native dialog's report that the item is on screen (I4.2).
    fn on_screen(&self, item: &ItemId);

    /// The native dialog's answer to the item (I4.5).
    fn answer_native(&self, item: &ItemId, answer: ConfirmAnswer) -> Result<(), Cause>;
}

/// The presenter of the native dialog (I4.9), which U4.2 builds: it gets each confirmation of
/// the presenter `native` that the page asks to show through `agents_present`, never a second
/// time while the dialog of that item is open, and returns at once, the dialog running on a
/// thread of its own. It is called with no lock held, so a presenter may call
/// `Native::on_screen` and `Native::answer`, which take the coordinator's lock.
pub trait Present: Send + Sync {
    /// Shows the dialog of `dialog`'s item.
    fn present(&self, dialog: Native);
}

/// A native confirmation handed to the presenter: its dialog counts as open until this value
/// is dropped, and its answer reaches the coordinator as the native one (I4.5), which the page
/// cannot give.
pub struct Native {
    core: Arc<dyn Core>,
    open: Arc<Mutex<Vec<ItemId>>>,
    item: ItemId,
}

impl Native {
    /// The item.
    pub fn item(&self) -> &ItemId {
        &self.item
    }

    /// The dialog's first report that it is on screen, from which the item arms (I4.2).
    pub fn on_screen(&self) {
        self.core.on_screen(&self.item);
    }

    /// The person's answer in the dialog: `Err(Early)` until the first tick after `arm_ms`
    /// from `on_screen`, after which I4.9 has the dialog shown once more, and
    /// `Err(UnknownItem)` after an answer. While the board holds the record of the item's agent
    /// stuck, `Err(NoService)`: the press is not taken, and the item stays held with its own
    /// time, so that a later `agents_present` shows it again once the record can be saved.
    pub fn answer(&self, answer: ConfirmAnswer) -> Result<(), Cause> {
        self.core.answer_native(&self.item, answer)
    }
}

impl Drop for Native {
    fn drop(&mut self) {
        let mut open = self.open.lock().unwrap_or_else(PoisonError::into_inner);
        open.retain(|i| i != &self.item);
    }
}

/// The value that the app manages for the handler: the hub behind `Core`.
#[derive(Clone)]
pub struct Agents(pub Arc<dyn Core>);

type Cfg<P> = <P as Owner>::Config;

pub(crate) const LOOK: Duration = Duration::from_millis(100);

pub(crate) fn stand_in() -> Snapshot {
    Snapshot {
        seq: 0,
        rows: Vec::new(),
        items: Vec::new(),
        note: words::text("note.no_service", &[]).ok(),
    }
}

/// The coordinator of the app over its ports `P`, the bell of its page and tray, and the waits
/// of the waiting call.
pub struct Hub<P: Owner> {
    coord: Arc<Coordinator<Wired<P>>>,
    bell: Arc<Bell>,
    waits: Waits,
    replied: Mutex<Option<Instant>>,
    present: Box<dyn Present>,
    open: Arc<Mutex<Vec<ItemId>>>,
    me: Weak<Hub<P>>,
    grace: Duration,
    ticked: AtomicBool,
}

impl<P> Hub<P>
where
    P: Owner
        + Runs
        + Analysis
        + Clock
        + Rng
        + Policy<Config = Cfg<P>>
        + Senders
        + Send
        + Sync
        + 'static,
    Cfg<P>: ConfigView + Clone + Send + 'static,
{
    /// A hub of a new life over `ports` and `limits`, whose count of agents under `needs_you`
    /// goes to `tray` at each change of the state, also when the count is the same, and whose
    /// native confirmations go to `present`, with the waits of `limits`. The tray's sink and
    /// every port of `P` are called with the coordinator's lock held; the quit hook's own thread
    /// calls none of them. None calls back into the hub, the `Core` or the coordinator, and none
    /// waits for a thread that can wait for that lock (a sink that waits for the main thread
    /// while the main thread calls a hook stops the app for good).
    pub fn new(
        ports: P,
        limits: Limits,
        tray: Box<dyn Fn(u32) + Send + Sync>,
        present: Box<dyn Present>,
    ) -> Arc<Hub<P>> {
        Hub::with_waits(ports, limits, tray, present, Waits::of(&limits))
    }

    /// The same with other waits, under the same rule for the sink and the ports.
    pub fn with_waits(
        ports: P,
        limits: Limits,
        tray: Box<dyn Fn(u32) + Send + Sync>,
        present: Box<dyn Present>,
        waits: Waits,
    ) -> Arc<Hub<P>> {
        let bell = Arc::new(Bell::new(tray));
        Arc::new_cyclic(|me| Hub {
            coord: Arc::new(Coordinator::new(Wired::new(ports, bell.clone()), limits)),
            bell,
            waits,
            replied: Mutex::new(None),
            present,
            open: Arc::new(Mutex::new(Vec::new())),
            me: me.clone(),
            grace: Duration::from_secs(limits.cancel_grace_s),
            ticked: AtomicBool::new(false),
        })
    }

    /// The coordinator. A tick made on it is not the hub's first tick: a hub that is never
    /// ticked through `Core::tick` answers the stand-in for good.
    pub fn coordinator(&self) -> &Coordinator<Wired<P>> {
        &self.coord
    }

    fn step<T: Send + 'static>(&self, step: fn(&Coordinator<Wired<P>>) -> T) -> Option<T> {
        let (done, answer) = mpsc::channel();
        let coord = Arc::clone(&self.coord);
        std::thread::Builder::new()
            .spawn(move || {
                let _ = done.send(step(&coord));
            })
            .ok()?;
        answer.recv_timeout(self.grace).ok()
    }

    /// The second step of the quit and the write of its list, in one window of `cancel_grace_s`
    /// from the call of the second step (ruling RD-154 point 2): the second step on a thread of
    /// its own, and on another the write (`Coordinator::quit_write`), which starts once that step
    /// has returned or ended by a panic, and at the latest at half of the window; the hook waits
    /// for the second step and then for the write, both until the end of the window. Where no
    /// thread can be started for the second step, the hook takes it no further, and writes
    /// nothing; where none can be started for the write, it waits for the second step alone.
    fn kill_and_write(&self) {
        let (to_writer, kill_seen_by_writer) = mpsc::channel::<()>();
        let (to_hook, kill_seen_by_hook) = mpsc::channel::<()>();
        let (wrote, written) = mpsc::channel::<()>();
        let end = Instant::now() + self.grace;
        let coord = Arc::clone(&self.coord);
        let kill = std::thread::Builder::new().spawn(move || {
            coord.quit_kill();
            let _ = to_writer.send(());
            let _ = to_hook.send(());
        });
        if kill.is_err() {
            return;
        }
        let coord = Arc::clone(&self.coord);
        let half = self.grace / 2;
        let write = std::thread::Builder::new().spawn(move || {
            let _ = kill_seen_by_writer.recv_timeout(half);
            coord.quit_write();
            let _ = wrote.send(());
        });
        let _ = kill_seen_by_hook.recv_timeout(end.saturating_duration_since(Instant::now()));
        if write.is_ok() {
            let _ = written.recv_timeout(end.saturating_duration_since(Instant::now()));
        }
    }

    fn run(&self, command: &str, args: &Args<'_>) -> Option<Reply> {
        let item = || ItemId::parse(args.text("item"));
        let agent = || AgentId::parse(args.text("agent"));
        Some(match command {
            "agents_snapshot" => self.snapshot(),
            "agents_next" => self.next(args.count("after")?),
            "agents_card" => match agent().and_then(|a| self.coord.card(&a)) {
                Ok(card) => Reply::Card(card),
                Err(cause) => Reply::Error(cause),
            },
            "agents_answer" => self.answer(item, args).into(),
            "agents_setup_typed" => item()
                .and_then(|i| {
                    let param = Param::parse(args.text("param"))?;
                    let step = args.count("step").ok_or(Cause::Malformed)?;
                    self.coord.setup_typed(&i, param, args.text("text"), step)
                })
                .into(),
            "agents_present" => item().and_then(|i| self.present(i)).into(),
            "agents_shown" => item()
                .and_then(|i| {
                    let step = args.count("step").ok_or(Cause::Malformed)?;
                    self.coord.shown_on_page(&i, step)
                })
                .into(),
            "agents_control" => {
                match agent().and_then(|a| Ok((a, Action::parse(args.text("action"))?))) {
                    Ok((a, action)) => match self.coord.control_reply(&a, action) {
                        Ok(()) => Reply::Ok,
                        Err(halvo_coord::types::Refusal::Cause(cause)) => Reply::Error(cause),
                        Err(refusal) => Reply::Refused(refusal),
                    },
                    Err(cause) => Reply::Error(cause),
                }
            }
            "agents_delete" => agent().and_then(|a| self.coord.delete(&a)).into(),
            _ => return None,
        })
    }

    fn answer(
        &self,
        item: impl Fn() -> Result<ItemId, Cause>,
        args: &Args<'_>,
    ) -> Result<(), Cause> {
        let item = item()?;
        let choice = Choice::parse(args.text("choice"))?;
        let pick = match choice {
            Choice::Choose => {
                let n = args.count("n").ok_or(Cause::Malformed)?;
                Some((Param::parse(args.text("param"))?, n))
            }
            _ => None,
        };
        let step = args.count("step").ok_or(Cause::Malformed)?;
        self.coord.answer(&item, choice, pick, step)
    }

    fn present(&self, item: ItemId) -> Result<(), Cause> {
        self.coord.present(&item)?;
        let mut open = self.open.lock().unwrap_or_else(PoisonError::into_inner);
        if open.contains(&item) {
            return Ok(());
        }
        open.push(item.clone());
        drop(open);
        let Some(core) = self.me.upgrade() else {
            return Err(Cause::NoService);
        };
        self.present.present(Native {
            core,
            open: self.open.clone(),
            item,
        });
        Ok(())
    }

    fn snapshot(&self) -> Reply {
        if self.ticked.load(Ordering::SeqCst) {
            Reply::Snapshot(self.coord.snapshot())
        } else {
            Reply::Snapshot(stand_in())
        }
    }

    fn next(&self, after: u32) -> Reply {
        let entry = self.bell.mark();
        let deadline = Instant::now() + self.waits.next;
        let mut seen = entry;
        let mut due: Option<Instant> = None;
        let mut first = true;
        loop {
            if !seen.shown || seen.closes != entry.closes {
                return Reply::Closed;
            }
            let now = Instant::now();
            if !self.ticked.load(Ordering::SeqCst) {
                if now >= deadline {
                    return Reply::Snapshot(stand_in());
                }
                seen = self.bell.wait(seen, deadline);
                continue;
            }
            if due.is_none() && self.coord.next(after) != Next::Same {
                let from = if first {
                    now
                } else {
                    now + self.waits.coalesce
                };
                let rest = self.last_reply().map(|t| t + self.waits.coalesce);
                due = Some(rest.map_or(from, |r| r.max(from)));
            }
            first = false;
            match due {
                Some(at) if now >= at => {
                    if let Next::Changed(state) = self.coord.next(after) {
                        *self.replied.lock().unwrap_or_else(PoisonError::into_inner) = Some(now);
                        return Reply::Snapshot(state);
                    }
                    due = None;
                }
                None if now >= deadline => {
                    return Reply::Snapshot(Snapshot {
                        seq: after,
                        rows: Vec::new(),
                        items: Vec::new(),
                        note: None,
                    })
                }
                _ => {}
            }
            seen = self.bell.wait(seen, due.unwrap_or(deadline));
        }
    }

    fn last_reply(&self) -> Option<Instant> {
        *self.replied.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl<P> Core for Hub<P>
where
    P: Owner
        + Runs
        + Analysis
        + Clock
        + Rng
        + Policy<Config = Cfg<P>>
        + Senders
        + Send
        + Sync
        + 'static,
    Cfg<P>: ConfigView + Clone + Send + 'static,
{
    fn call(&self, command: &str, args: Option<&Value>) -> Reply {
        let Some(found) = LIST.iter().find(|c| c.name == command) else {
            return Reply::Error(Cause::Malformed);
        };
        match Args::read(found, args) {
            Ok(args) => self
                .run(found.name, &args)
                .unwrap_or(Reply::Error(Cause::Malformed)),
            Err(_) if found.name == "agents_next" => self.next(0),
            Err(_) if found.errors.is_empty() => self.snapshot(),
            Err(cause) => Reply::Error(cause),
        }
    }

    fn shown(&self) {
        self.bell.show();
    }

    fn hidden(&self) {
        self.bell.close();
    }

    fn destroyed(&self) {
        self.bell.close();
    }

    fn tick(&self) {
        self.coord.tick();
        if !self.ticked.swap(true, Ordering::SeqCst) {
            self.bell.ring();
        }
    }

    fn quit(&self, budget: Option<Duration>) -> Vec<(AgentId, RunId)> {
        let start = Instant::now();
        self.coord.mark_quit();
        let Some(wait) = self.step(Coordinator::quit_stop) else {
            return self.coord.not_stopped();
        };
        let stopped = Instant::now();
        let grace = Duration::from_millis(wait.until.saturating_sub(wait.at)).min(self.grace);
        let end = budget.map_or(stopped + grace, |b| (start + b).min(stopped + grace));
        let mut seen = self.bell.mark();
        while self.coord.live() > 0 {
            let now = Instant::now();
            if now >= end {
                break;
            }
            seen = self.bell.wait(seen, end.min(now + LOOK));
        }
        self.kill_and_write();
        self.coord.not_stopped()
    }

    fn on_screen(&self, item: &ItemId) {
        self.coord.on_screen(item);
    }

    fn answer_native(&self, item: &ItemId, answer: ConfirmAnswer) -> Result<(), Cause> {
        self.coord.answer_native(item, answer)
    }
}
