use super::{
    active_state, current_slot, finished, gives_active, holds, slot_open, start_step, tick_plan,
    tick_step, wants_active, Around, Ask, Board, Dying, Go, Hold, LiveRun, Member, Owed, Owing,
    StartStep, Stop, Verdict, Waiting,
};
use crate::ports::{Analysis, Clock, Owner, PortError, Rng, Runs, Stamp};
use crate::types::{
    hex, Agent, AgentId, AgentState, Answered, Body, Cause, CheckAnswer, Cleared, ConfigView,
    Doing, Event, Facts, HoldSetCause, ItemId, Lapsed, Live, OtherLapse, Outcome, OwnItem,
    OwnItemKind, Raised, ReadyAnswer, RunEnd, RunId, RunSpec, Trigger, What, Workflow, WorkflowId,
    WorkflowState,
};
use std::num::NonZeroU32;

/// A move that `drive` carries out on the ports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Move {
    /// One tick, in the order of I5.9.
    Tick,
    /// A start by the person: the control `run_now`, or the choice `start` of a `ready` item.
    Start {
        /// The agent.
        agent: AgentId,
    },
    /// The choice `skip` of the agent's `ready` item (I5.3).
    Skip {
        /// The agent.
        agent: AgentId,
    },
    /// The person's answer to the hold `check` of the agent (I5.4).
    Check {
        /// The agent.
        agent: AgentId,
        /// `try_again` or `went_through`.
        answer: CheckAnswer,
    },
    /// The control `switch_on` (I5.4).
    SwitchOn {
        /// The agent.
        agent: AgentId,
    },
    /// The switch-off control (I5.4, I5.7).
    SwitchOff {
        /// The agent.
        agent: AgentId,
    },
    /// The end of a live run by its runner: the outcome of its last frame, or none when the
    /// runner was gone with no last frame (I5.6).
    End {
        /// The run.
        run: RunId,
        /// The outcome, or none.
        outcome: Option<Outcome>,
    },
    /// The creation of an agent for each approved workflow with none (I5.5).
    Create,
    /// What the board owes and nothing else, for a tick after the first step of the app's quit
    /// (ruling RD-34 (d)).
    Owed,
}

/// The answer of a move that was not refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    /// Done.
    Ok,
    /// The start joined the queue, or, for a start leaving it, stays at its head.
    Queued,
}

/// A start that a move made for an agent, with its answer and its run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Served {
    /// The agent.
    pub agent: AgentId,
    /// The answer of `start_step`, or `busy` or `no_service` from steps 6 to 8.
    pub answer: Result<Answer, Cause>,
    /// The run whose `run_started` event was stored, or none.
    pub run: Option<RunId>,
}

/// A run that a move ended, with its cause and the start that its end took from the queue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ended {
    /// The run.
    pub run: RunId,
    /// The cause of I5.6.
    pub cause: RunEnd,
    /// The start that the end took from the queue: none or one.
    pub next: Vec<Served>,
}

/// What a move did, in the keys of a trace line of the schedule (I5.13).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    /// The answer.
    pub answer: Result<Answer, Cause>,
    /// For a start, the run whose `run_started` event was stored, or none.
    pub run: Option<RunId>,
    /// The runs that the move stopped.
    pub stopped: Vec<RunId>,
    /// The runs that the move ended.
    pub ended: Vec<Ended>,
    /// For a tick, the start that it took from the queue: none or one.
    pub next: Vec<Served>,
    /// For a tick, the agents that got a `ready` item.
    pub raised: Vec<AgentId>,
    /// For a tick, the starts by the scheduler, in the order of the agents.
    pub started: Vec<Served>,
    /// For a creation, the agents that it created.
    pub created: Vec<AgentId>,
    /// For a tick, the agents of the workflows that are gone whose queued start or open item it
    /// dropped.
    pub dropped: Vec<AgentId>,
}

impl Default for Report {
    fn default() -> Report {
        Report {
            answer: Ok(Answer::Ok),
            run: None,
            stopped: Vec::new(),
            ended: Vec::new(),
            next: Vec::new(),
            raised: Vec::new(),
            started: Vec::new(),
            created: Vec::new(),
            dropped: Vec::new(),
        }
    }
}

fn refused(cause: Cause) -> Report {
    Report {
        answer: Err(cause),
        ..Report::default()
    }
}

fn port_cause(error: &PortError) -> Cause {
    match error {
        PortError::Busy => Cause::Busy,
        PortError::NoService | PortError::Refused(_) => Cause::NoService,
    }
}

/// What a move works with: the ports, the limits, and the one read of the clock that the move
/// made first, whose instant and day stamp every row of the move (I7.1).
struct Cx<'a, P> {
    ports: &'a P,
    limits: &'a crate::types::Limits,
    stamp: Stamp,
}

fn ids(workflows: &[Workflow]) -> Vec<WorkflowId> {
    workflows.iter().map(|w| w.id().clone()).collect()
}

fn known<C>(board: &Board<C>) -> Vec<WorkflowId> {
    board
        .members
        .iter()
        .map(|m| m.facts.workflow.id.clone())
        .chain(board.runs.iter().map(|r| r.workflow.clone()))
        .collect()
}

fn newest_started<C>(facts: &Facts<C>) -> Option<&RunId> {
    facts.events.iter().rev().find_map(|e| match &e.what {
        What::RunStarted { run, .. } => Some(run),
        _ => None,
    })
}

fn refresh<C: Clone>(member: &mut Member<C>, entry: Option<&Agent<C>>, history: usize) {
    if let Some(a) = entry {
        member.facts.config = a.config.clone();
        member.facts.slots = a.slots.clone();
        member.session = a.session;
    }
    let facts = &mut member.facts;
    let last = entry.and_then(|a| a.last.as_ref());
    facts.store_ended = match newest_started(facts) {
        None => true,
        Some(run) => last.is_some_and(|l| &l.run == run),
    };
    if let Some(l) = last {
        facts.runs.retain(|r| r.run != l.run);
        facts.runs.insert(0, l.clone());
        facts.runs.truncate(history);
    }
}

fn fresh<P: Owner<Config = C>, C: Clone>(
    cx: &Cx<'_, P>,
    board: &mut Board<C>,
    only: Option<usize>,
) -> Result<(), PortError> {
    let agents = cx.ports.agents()?;
    let history = cx.limits.history_runs as usize;
    for (k, m) in board.members.iter_mut().enumerate() {
        if only.is_none_or(|i| i == k) {
            let entry = agents.iter().find(|a| a.agent == m.facts.agent);
            refresh(m, entry, history);
        }
    }
    Ok(())
}

fn listing<P: Owner, C>(
    cx: &Cx<'_, P>,
    board: &mut Board<C>,
    only: Option<usize>,
) -> Result<Vec<WorkflowId>, PortError> {
    let list = cx.ports.workflows()?;
    for (k, m) in board.members.iter_mut().enumerate() {
        if only.is_none_or(|i| i == k) {
            let found = list.iter().find_map(|w| match w {
                Workflow::Full(f) if f.id == m.facts.workflow.id => Some(f.clone()),
                _ => None,
            });
            if let Some(f) = found {
                m.facts.workflow = f;
            }
        }
    }
    Ok(ids(&list))
}

fn store<P: Owner, C: ConfigView>(
    cx: &Cx<'_, P>,
    board: &mut Board<C>,
    i: usize,
    whats: Vec<What>,
) -> Result<(), PortError> {
    store_at(cx, board, i, whats, cx.stamp.day(), cx.stamp.at())
}

fn store_at<P: Owner, C: ConfigView>(
    cx: &Cx<'_, P>,
    board: &mut Board<C>,
    i: usize,
    whats: Vec<What>,
    day: u32,
    at: i64,
) -> Result<(), PortError> {
    store_read(cx, board, i, whats, day, at, None).map(|_| ())
}

/// What a store did: it logged the events or found them in the read after a refusal `seq`; or
/// the read brought a lost start, which it closed, and it logged nothing of a `run_started`; or
/// a start decided its steps 0 to 4 again from the facts as read and they fail with this cause,
/// so that it logged nothing more (ruling RD-52 (a)).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stored {
    Logged,
    LostStart,
    Decided(Cause),
}

/// A start's decision of its steps 0 to 4 again, from the agent's facts as a read left them: the
/// cause of the step that now fails, or none where the start goes on (ruling RD-52 (a)).
type Decide<'a, C> = &'a dyn Fn(&Facts<C>) -> Option<Cause>;

/// Logs the events; after a refusal `seq` it reads the owner's events once, keeps the events
/// that the read does not cover, closes the lost starts that it brought, and, for a start that
/// gives `decide`, decides that start's steps 0 to 4 again from the facts as read before it logs
/// the rest once more (I7.2, rulings XB-31, RD-34 (b) and RD-52 (a)).
fn store_read<P: Owner, C: ConfigView>(
    cx: &Cx<'_, P>,
    board: &mut Board<C>,
    i: usize,
    whats: Vec<What>,
    day: u32,
    at: i64,
    decide: Option<Decide<'_, C>>,
) -> Result<Stored, PortError> {
    let refusal = match log_at(cx, board, i, &whats, day, at) {
        Err(e) if seq_refusal(&e) => e,
        other => return other.map(|()| Stored::Logged),
    };
    let facts = &mut board.members[i].facts;
    let before = facts.events.len();
    if resync(cx.ports, &facts.agent, &mut facts.events).is_err() {
        return Err(refusal);
    }
    let rest: Vec<What> = whats
        .into_iter()
        .filter(|w| !covered(&facts.events, before, w))
        .collect();
    settle(facts, before);
    let starts = rest.iter().any(|w| matches!(w, What::RunStarted { .. }));
    if close_lost_starts(cx, board, i, before, day, at) && starts {
        return Ok(Stored::LostStart);
    }
    if let Some(cause) = decide.and_then(|d| d(&board.members[i].facts)) {
        return Ok(Stored::Decided(cause));
    }
    if rest.is_empty() {
        return Ok(Stored::Logged);
    }
    log_at(cx, board, i, &rest, day, at).map(|()| Stored::Logged)
}

/// Closes each `run_started` of the agent among its events from `from` on whose
/// acknowledgement was lost (ruling RD-34 (b) (5)): the events hold no `run_ended` of the run,
/// the board holds no live or dying run of it, and no `run_ended` or `end_run` of it is owed in
/// this life. As recovery would in the next life (I5.8), its `run_ended` with `interrupted` and
/// its `end_run` are sent, each owed until the owner acknowledges it, and the agent's newest run
/// reads as not ended by the store, so that step 3 of I5.5 holds the agent's next start until
/// the store has ended it. Such a row was never taken up: the start that logged it answered
/// `no_service` and launched no runner. Whether it closed one.
fn close_lost_starts<P: Owner, C: ConfigView>(
    cx: &Cx<'_, P>,
    board: &mut Board<C>,
    i: usize,
    from: usize,
    day: u32,
    at: i64,
) -> bool {
    let facts = &board.members[i].facts;
    let lost: Vec<RunId> = facts.events[from.min(facts.events.len())..]
        .iter()
        .filter_map(|e| match &e.what {
            What::RunStarted { run, .. } => Some(run.clone()),
            _ => None,
        })
        .filter(|run| !holds_end(&facts.events, run))
        .filter(|run| {
            !board.runs.iter().any(|r| &r.run == run) && !board.dying.iter().any(|d| &d.run == run)
        })
        .filter(|run| {
            !board.owed.iter().any(|o| {
                o.agent == facts.agent
                    && matches!(
                        &o.what,
                        Owing::RunEnded { run: r, .. } | Owing::EndRun { run: r } if r == run
                    )
            })
        })
        .collect();
    if lost.is_empty() {
        return false;
    }
    let agent = facts.agent.clone();
    let workflow = facts.workflow.id.clone();
    board.members[i].facts.store_ended = false;
    for run in lost {
        let debts = [
            Owing::RunEnded {
                run: run.clone(),
                cause: RunEnd::Interrupted,
                day,
                at,
            },
            Owing::EndRun { run },
        ];
        for what in debts {
            let owed = Owed {
                agent: agent.clone(),
                workflow: workflow.clone(),
                what,
            };
            if !send_owed(cx, board, &owed) {
                board.owed.push(owed);
            }
        }
    }
    true
}

fn log_at<P: Owner, C>(
    cx: &Cx<'_, P>,
    board: &mut Board<C>,
    i: usize,
    whats: &[What],
    day: u32,
    at: i64,
) -> Result<(), PortError> {
    let facts = &mut board.members[i].facts;
    let last = facts.events.last().map_or(0, |e| e.seq.get());
    let events: Vec<Event> = whats
        .iter()
        .zip(0u32..)
        .map(|(what, k)| Event {
            workflow: facts.workflow.id.clone(),
            agent: facts.agent.clone(),
            seq: NonZeroU32::MIN.saturating_add(last.saturating_add(k)),
            day,
            at,
            what: what.clone(),
        })
        .collect();
    cx.ports.log(&events)?;
    facts.events.extend(events);
    Ok(())
}

/// Whether the owner's events, of which those from `before` on came with the read after a
/// refusal `seq`, already hold what `what` would store (I7.2, ruling XB-31): an open item raised
/// as `what` raises it, an item that `what` would close and that they close, an end of the run
/// whose `run_ended` `what` is, whatever its cause (ruling RD-34), or the same event.
fn covered(events: &[Event], before: usize, what: &What) -> bool {
    let read = &events[before.min(events.len())..];
    match what {
        What::ItemRaised { raised, .. } => read.iter().any(|e| {
            matches!(&e.what, What::ItemRaised { item, raised: r } if r == raised && !item_closed(events, item))
        }),
        What::ItemAnswered { item, .. } | What::ItemLapsed { item, .. } => {
            item_closed(events, item)
        }
        What::RunEnded { run, .. } => holds_end(events, run),
        other => read.iter().any(|e| &e.what == other),
    }
}

/// After a read of the owner's events: the agent's own item leaves its facts when those events
/// close it, and an agent that then holds no item takes the newest own item that the read brought
/// and that is still open, so that the board holds what the owner's ledger holds: a check that a
/// lost acknowledgement hid, with the lapse of the `ready` item before it, replaces that item
/// (ruling RD-5).
fn settle<C>(facts: &mut Facts<C>, before: usize) {
    let open = match &facts.doing {
        Doing::Idle(Some(o)) | Doing::Queued(Some(o)) => Some(o.item.clone()),
        Doing::Idle(None) | Doing::Queued(None) => None,
        Doing::Live(_) => return,
    };
    if let Some(item) = open {
        if !item_closed(&facts.events, &item) {
            return;
        }
        facts.doing = without_item(&facts.doing);
    }
    let revealed = facts.events[before.min(facts.events.len())..]
        .iter()
        .rev()
        .find_map(|e| match &e.what {
            What::ItemRaised { item, raised } if !item_closed(&facts.events, item) => {
                let kind = match raised {
                    Raised::Check(None) => OwnItemKind::Check,
                    Raised::Ready => OwnItemKind::Ready,
                    Raised::Setup => OwnItemKind::Setup,
                    _ => return None,
                };
                Some(OwnItem {
                    item: item.clone(),
                    kind,
                })
            }
            _ => None,
        });
    if let Some(own) = revealed {
        facts.doing = match facts.doing {
            Doing::Queued(_) => Doing::Queued(Some(own)),
            _ => Doing::Idle(Some(own)),
        };
    }
}

fn send_state<P: Owner, C>(cx: &Cx<'_, P>, facts: &Facts<C>, state: AgentState) {
    let _ = cx.ports.send(&Body::SetState {
        workflow: facts.workflow.id.clone(),
        agent: facts.agent.clone(),
        state,
    });
}

fn minted<P: Rng>(cx: &Cx<'_, P>) -> Result<String, PortError> {
    Ok(hex(cx.ports.bytes16()?))
}

fn own(doing: &Doing, kind: OwnItemKind) -> Option<ItemId> {
    match doing {
        Doing::Idle(Some(o)) | Doing::Queued(Some(o)) if o.kind == kind => Some(o.item.clone()),
        _ => None,
    }
}

fn without_item(doing: &Doing) -> Doing {
    match doing {
        Doing::Idle(_) => Doing::Idle(None),
        Doing::Queued(_) => Doing::Queued(None),
        Doing::Live(live) => Doing::Live(live.clone()),
    }
}

fn finish<P: Owner, C>(cx: &Cx<'_, P>, board: &mut Board<C>, i: usize, run: &RunId, cause: RunEnd)
where
    C: ConfigView,
{
    let (day, at) = (cx.stamp.day(), cx.stamp.at());
    match log_end(cx, board, i, run, cause, (day, at), false) {
        Ok(()) => {}
        Err(e) if gone_refusal(&e) => {}
        Err(_) => {
            let facts = &board.members[i].facts;
            let owed = Owed {
                agent: facts.agent.clone(),
                workflow: facts.workflow.id.clone(),
                what: Owing::RunEnded {
                    run: run.clone(),
                    cause,
                    day,
                    at,
                },
            };
            board.owed.push(owed);
        }
    }
}

/// Logs the `run_ended` of a run, with `hold_set` of `failures` where the end makes the streak.
/// A debt logs through `log_at`, so that a refusal `seq` comes back to `send_owed`, whose read
/// pays it when the owner's events hold the run's end; the end of a move logs through `store_at`
/// (ruling RD-34).
fn log_end<P: Owner, C>(
    cx: &Cx<'_, P>,
    board: &mut Board<C>,
    i: usize,
    run: &RunId,
    cause: RunEnd,
    (day, at): (u32, i64),
    debt: bool,
) -> Result<(), PortError>
where
    C: ConfigView,
{
    let facts = &board.members[i].facts;
    let before = facts
        .events
        .iter()
        .rev()
        .filter_map(|e| match e.what {
            What::RunEnded { cause, .. } => Some(cause),
            _ => None,
        })
        .take_while(|c| c.failure().is_some())
        .count();
    let streak = if cause.failure().is_some() {
        before + 1
    } else {
        0
    };
    let pause = cause.failure().is_some() && streak >= cx.limits.fails_to_pause as usize;
    let mut whats = vec![What::RunEnded {
        run: run.clone(),
        cause,
    }];
    if pause {
        whats.push(What::HoldSet {
            cause: HoldSetCause::Failures,
        });
    }
    if debt {
        log_at(cx, board, i, &whats, day, at)?;
    } else {
        store_at(cx, board, i, whats, day, at)?;
    }
    if pause {
        send_state(cx, &board.members[i].facts, AgentState::Paused);
    }
    Ok(())
}

/// Steps 5 to 8 of a start that passed steps 0 to 4 (I5.5): the request to defer, the store of
/// the open `ready` item's answer `start` and of `run_started`, the body `start_run` and
/// `Runs::start`. Where the log meets `seq`, the start decides its steps 0 to 4 again from the
/// facts as read (`decide`, ruling RD-52 (a)); where one fails it answers that step's cause and
/// writes nothing of the body, so that no slot and no start of the day is counted and no runner
/// starts. Such a start has sent the request to defer of step 5, as I5.5 says of a start that
/// fails at step 6.
fn launch<P, C>(
    cx: &Cx<'_, P>,
    board: &mut Board<C>,
    i: usize,
    trigger: Trigger,
    go: Go,
    decide: Decide<'_, C>,
) -> Served
where
    P: Owner + Runs + Analysis + Rng,
    C: ConfigView,
{
    let agent = board.members[i].facts.agent.clone();
    let served = |answer, run| Served {
        agent: agent.clone(),
        answer,
        run,
    };
    let _ = cx.ports.defer(cx.stamp.now().saturating_add(go.defer_ms));
    let run = match minted(cx).map(|h| RunId::parse(&h)) {
        Ok(Ok(run)) => run,
        Ok(Err(_)) => return served(Err(Cause::NoService), None),
        Err(e) => return served(Err(port_cause(&e)), None),
    };
    let mut whats = Vec::new();
    if let Some(item) = own(&board.members[i].facts.doing, OwnItemKind::Ready) {
        whats.push(What::ItemAnswered {
            item,
            answer: Answered::Ready(ReadyAnswer::Start),
        });
    }
    whats.push(What::RunStarted {
        run: run.clone(),
        trigger,
        slot: go.slot,
    });
    let (day, at) = (cx.stamp.day(), cx.stamp.at());
    match store_read(cx, board, i, whats, day, at, Some(decide)) {
        Ok(Stored::Logged) => {}
        Ok(Stored::LostStart) => return served(Err(Cause::State), None),
        Ok(Stored::Decided(cause)) => return served(Err(cause), None),
        Err(e) => return served(Err(port_cause(&e)), None),
    }
    board.members[i].facts.store_ended = false;
    let workflow = board.members[i].facts.workflow.id.clone();
    let body = Body::StartRun {
        workflow: workflow.clone(),
        agent: agent.clone(),
        run: run.clone(),
        mode: go.mode,
    };
    let spec = RunSpec {
        run: run.clone(),
        agent: agent.clone(),
        workflow: workflow.clone(),
        mode: go.mode,
        headed: go.headed,
    };
    if cx.ports.send(&body).is_err() || cx.ports.start(&spec).is_err() {
        board.members[i].facts.doing = Doing::Idle(None);
        finish(cx, board, i, &run, RunEnd::Failed);
        let owed = Owed {
            agent: agent.clone(),
            workflow,
            what: Owing::EndRun { run: run.clone() },
        };
        if !send_owed(cx, board, &owed) {
            board.owed.push(owed);
        }
        return served(Err(Cause::NoService), Some(run));
    }
    let rank = board.rank();
    board.runs.push(LiveRun {
        run: run.clone(),
        agent: agent.clone(),
        workflow,
        holds_model: go.holds_model,
        defer_ms: go.defer_ms,
        rank,
        stop: None,
    });
    board.members[i].facts.doing = Doing::Live(Live::Starting);
    served(Ok(Answer::Ok), Some(run))
}

/// A start of the agent `i` (I5.5). A start acts on no verdict that is older than its newest
/// read of the owner's events (ruling RD-52 (a)): after the store of `hold_cleared` that ends the
/// hold `failures`, and after a store of its path whose refusal `seq` caused a read, it decides
/// steps 0 to 4 again. Where its read brought a `hold_set` with the cause `person` that the
/// board's events did not hold, it sends `set_state` `paused` once, after its answer is decided;
/// a send that fails is not made again.
fn start<P, C>(
    cx: &Cx<'_, P>,
    board: &mut Board<C>,
    i: usize,
    trigger: Trigger,
    listed: &[WorkflowId],
    from_queue: bool,
    unread: Option<Cause>,
) -> Served
where
    P: Owner + Runs + Analysis + Rng,
    C: ConfigView,
{
    let off = holds(&board.members[i].facts).contains(&Hold::Off);
    let served = start_steps(cx, board, i, trigger, listed, from_queue, unread);
    if !off && holds(&board.members[i].facts).contains(&Hold::Off) {
        send_state(cx, &board.members[i].facts, AgentState::Paused);
    }
    served
}

fn start_steps<P, C>(
    cx: &Cx<'_, P>,
    board: &mut Board<C>,
    i: usize,
    trigger: Trigger,
    listed: &[WorkflowId],
    from_queue: bool,
    unread: Option<Cause>,
) -> Served
where
    P: Owner + Runs + Analysis + Rng,
    C: ConfigView,
{
    let agent = board.members[i].facts.agent.clone();
    let (day, at) = (cx.stamp.day(), cx.stamp.at());
    close_lost_starts(cx, board, i, 0, day, at);
    let around = Around {
        listed: listed.contains(&board.members[i].facts.workflow.id),
        live: u32::try_from(board.runs.len()).unwrap_or(u32::MAX),
        model_held: board.runs.iter().any(|r| r.holds_model),
        queued: board.queue.len(),
        session: board.members[i].session,
        from_queue,
        owed: board
            .owed
            .iter()
            .any(|o| o.agent == agent && matches!(o.what, Owing::RunEnded { .. })),
    };
    let steps = |facts: &mut Facts<C>| {
        let kept = facts.store_ended;
        if unread.is_some() {
            facts.store_ended = true;
        }
        let step = start_step(facts, trigger, cx.limits, &around);
        facts.store_ended = kept;
        step
    };
    let decide = |facts: &Facts<C>| match start_step(facts, trigger, cx.limits, &around) {
        StartStep {
            verdict: Verdict::Refused(cause),
            ..
        } => Some(cause),
        StartStep { clear: Some(_), .. } => Some(Cause::Hold),
        StartStep { .. } => None,
    };
    let mut step = steps(&mut board.members[i].facts);
    let served = |answer| Served {
        agent: agent.clone(),
        answer,
        run: None,
    };
    if step.clear.is_some() {
        let cleared = What::HoldCleared {
            cause: Cleared::Failures,
        };
        if let Err(e) = store(cx, board, i, vec![cleared]) {
            return served(Err(port_cause(&e)));
        }
        step = steps(&mut board.members[i].facts);
        let facts = &board.members[i].facts;
        if !holds(facts).contains(&Hold::Off) {
            send_state(cx, facts, active_state(&facts.config));
        }
    }
    if let Some(cause) = unread {
        let off = holds(&board.members[i].facts).contains(&Hold::Off);
        match step.verdict {
            Verdict::Refused(Cause::QueueFull) | Verdict::Queued | Verdict::Go(_) => {
                return served(Err(cause));
            }
            Verdict::Refused(Cause::Hold) if !off => return served(Err(cause)),
            Verdict::Refused(_) => {}
        }
    }
    match step.verdict {
        Verdict::Refused(cause) => served(Err(cause)),
        Verdict::Queued if from_queue => served(Ok(Answer::Queued)),
        Verdict::Queued => {
            let mut item = match &board.members[i].facts.doing {
                Doing::Idle(item) => item.clone(),
                Doing::Queued(_) | Doing::Live(_) => None,
            };
            if let Some(ready) = own(&board.members[i].facts.doing, OwnItemKind::Ready) {
                let answered = What::ItemAnswered {
                    item: ready,
                    answer: Answered::Ready(ReadyAnswer::Start),
                };
                match store_read(cx, board, i, vec![answered], day, at, Some(&decide)) {
                    Ok(Stored::Decided(cause)) => return served(Err(cause)),
                    Ok(_) => item = None,
                    Err(_) => {}
                }
            }
            board.queue.push(Waiting {
                agent: agent.clone(),
                trigger,
            });
            board.members[i].facts.doing = Doing::Queued(item);
            served(Ok(Answer::Queued))
        }
        Verdict::Go(go) => launch(cx, board, i, trigger, go, &decide),
    }
}

fn serve<P, C>(cx: &Cx<'_, P>, board: &mut Board<C>, listed: &[WorkflowId]) -> Option<Served>
where
    P: Owner + Runs + Analysis + Rng,
    C: ConfigView,
{
    let head = board.queue.head()?.clone();
    let Some(i) = board.index(&head.agent) else {
        board.queue.pop();
        return Some(Served {
            agent: head.agent,
            answer: Err(Cause::State),
            run: None,
        });
    };
    let served = start(cx, board, i, head.trigger, listed, true, None);
    if served.answer != Ok(Answer::Queued) {
        board.queue.pop();
        if let Doing::Queued(item) = &board.members[i].facts.doing {
            board.members[i].facts.doing = Doing::Idle(item.clone());
        }
    }
    Some(served)
}

/// Makes the live run `run` `stopping`, its stop at `now` with `cause`, and its member's phase
/// `stopping` (I5.7); nothing for a run that is not live.
pub fn stop_run<C>(board: &mut Board<C>, run: &RunId, now: u64, cause: RunEnd) {
    let Some(r) = board.runs.iter_mut().find(|r| &r.run == run) else {
        return;
    };
    r.stop = Some(Stop { at: now, cause });
    let agent = r.agent.clone();
    if let Some(i) = board.index(&agent) {
        board.members[i].facts.doing = Doing::Live(Live::Stopping);
    }
}

fn end_run<P, C>(
    cx: &Cx<'_, P>,
    board: &mut Board<C>,
    run: &RunId,
    cause: RunEnd,
    listed: &[WorkflowId],
    killed: bool,
    serving: bool,
) -> Ended
where
    P: Owner + Runs + Analysis + Rng,
    C: ConfigView,
{
    if let Some(k) = board.runs.iter().position(|r| &r.run == run) {
        let live = board.runs.remove(k);
        if let (Some(stop), false) = (live.stop, killed) {
            board.dying.push(Dying {
                run: live.run.clone(),
                rank: live.rank,
                stopped: stop.at,
            });
        }
        if let Some(i) = board.index(&live.agent) {
            board.members[i].facts.doing = Doing::Idle(None);
            if listed.contains(&live.workflow) {
                finish(cx, board, i, &live.run, cause);
            }
        }
    }
    Ended {
        run: run.clone(),
        cause,
        next: if serving {
            serve(cx, board, listed).into_iter().collect()
        } else {
            Vec::new()
        },
    }
}

fn gone_refusal(error: &PortError) -> bool {
    matches!(error, PortError::Refused(name) if name.as_str() == "no_workflow")
}

fn send_owed<P: Owner, C: ConfigView>(cx: &Cx<'_, P>, board: &mut Board<C>, owed: &Owed) -> bool {
    match &owed.what {
        Owing::EndRun { run } => match cx.ports.send(&Body::EndRun { run: run.clone() }) {
            Ok(_) => true,
            Err(e) => gone_refusal(&e),
        },
        Owing::RunEnded {
            run,
            cause,
            day,
            at,
        } => {
            let Some(i) = board.index(&owed.agent) else {
                return true;
            };
            if holds_end(&board.members[i].facts.events, run) {
                return true;
            }
            match log_end(cx, board, i, run, *cause, (*day, *at), true) {
                Ok(()) => true,
                Err(e) if seq_refusal(&e) => resync_end(cx, board, i, run),
                Err(e) => gone_refusal(&e),
            }
        }
        Owing::ItemLapsed {
            item,
            lapsed,
            day,
            at,
        } => {
            let Some(i) = board.index(&owed.agent) else {
                return true;
            };
            if item_closed(&board.members[i].facts.events, item) {
                return true;
            }
            let what = What::ItemLapsed {
                item: item.clone(),
                lapsed: *lapsed,
            };
            match log_at(cx, board, i, &[what], *day, *at) {
                Ok(()) => true,
                Err(e) if seq_refusal(&e) => {
                    let facts = &mut board.members[i].facts;
                    read_after_seq(cx.ports, facts).is_ok() && item_closed(&facts.events, item)
                }
                Err(e) => gone_refusal(&e),
            }
        }
    }
}

/// Whether the owner refused a log with `seq`: the record's `seq` is not the owner's last of the
/// agent plus 1, because the owner stored an event whose acknowledgement was lost, or one of
/// another writer (I7.2).
pub(crate) fn seq_refusal(error: &PortError) -> bool {
    matches!(error, PortError::Refused(name) if name.as_str() == "seq")
}

/// The events take the owner's events of the agent after the last one that they hold, page by
/// page of at most 64, as `coord_history` gives them (I7.2, rulings XB-17 and XB-31).
pub(crate) fn resync<P: Owner>(
    ports: &P,
    agent: &AgentId,
    events: &mut Vec<Event>,
) -> Result<(), PortError> {
    loop {
        let after = events.last().map_or(0, |e| e.seq.get());
        let page = ports.history(agent, after)?;
        let full = page.len() >= 64;
        events.extend(page.into_iter().filter(|e| e.seq.get() > after));
        if !full {
            return Ok(());
        }
    }
}

/// A read of the owner's events after a refusal `seq` (I7.2, XB-31, rulings RD-12 (b) and RD-5):
/// the facts take the owner's events after their last one, and `settle` makes the board hold the
/// agent's item that the owner's ledger holds, whichever log was refused: a skip, a start or a
/// lapse that the owner stored with a lost acknowledgement takes the agent's item off the board,
/// and an agent that then holds no item takes the newest item of the coordinator's own kinds that
/// those events raised and left open.
pub(crate) fn read_after_seq<P: Owner, C>(
    ports: &P,
    facts: &mut Facts<C>,
) -> Result<(), PortError> {
    let before = facts.events.len();
    resync(ports, &facts.agent, &mut facts.events)?;
    settle(facts, before);
    Ok(())
}

/// The read after a refusal `seq` of the raise of a check or of the person's answer to it (I7.2,
/// rulings RD-12 (b) and RD-34 (b) (5)): `read_after_seq`, and then the close of each start that
/// the read brought and that no runner took up, as `store_read` does for the other moves.
pub(crate) fn read_back<P: Owner, C: ConfigView>(
    ports: &P,
    limits: &crate::types::Limits,
    stamp: Stamp,
    board: &mut Board<C>,
    i: usize,
) -> Result<(), PortError> {
    let before = board.members[i].facts.events.len();
    read_after_seq(ports, &mut board.members[i].facts)?;
    let cx = Cx {
        ports,
        limits,
        stamp,
    };
    close_lost_starts(&cx, board, i, before, stamp.day(), stamp.at());
    Ok(())
}

/// Whether the events close the item: they hold its `item_released` or its `item_lapsed`, or,
/// for a kind other than `confirm`, its `item_answered` (I5.8, I7.1).
pub(crate) fn item_closed(events: &[Event], item: &ItemId) -> bool {
    let confirm = events.iter().any(|e| {
        matches!(&e.what, What::ItemRaised { item: i, raised: Raised::Confirm(_) } if i == item)
    });
    events.iter().any(|e| match &e.what {
        What::ItemReleased { item: i, .. } | What::ItemLapsed { item: i, .. } => i == item,
        What::ItemAnswered { item: i, .. } => i == item && !confirm,
        _ => false,
    })
}

/// After the owner refused an owed `run_ended` with `seq` (I7.2): `read_after_seq` reads the
/// owner's events after the last one that the facts hold, and the debt is paid when those hold
/// the run's `run_ended`, which the owner stored with an acknowledgement that was lost; else the
/// next tick logs it after them.
fn resync_end<P: Owner, C>(cx: &Cx<'_, P>, board: &mut Board<C>, i: usize, run: &RunId) -> bool {
    let facts = &mut board.members[i].facts;
    read_after_seq(cx.ports, facts).is_ok() && holds_end(&facts.events, run)
}

/// Whether the events hold a `run_ended` of the run.
fn holds_end(events: &[Event], run: &RunId) -> bool {
    events
        .iter()
        .any(|e| matches!(&e.what, What::RunEnded { run: r, .. } if r == run))
}

/// Whether the agent store's last record of the agent, as the move's read of `Owner::agents`
/// gave it, shows the run of an owed `end_run` ended: it names the run, or a run whose
/// `run_started` comes after that of the run, which the store could take only once it had ended
/// it (I5.5 step 3, I5.8).
fn store_took<C>(board: &Board<C>, agent: &AgentId, run: &RunId) -> bool {
    let Some(i) = board.index(agent) else {
        return false;
    };
    let facts = &board.members[i].facts;
    let at = |run: &RunId| {
        facts
            .events
            .iter()
            .position(|e| matches!(&e.what, What::RunStarted { run: r, .. } if r == run))
    };
    facts.runs.first().is_some_and(|last| {
        &last.run == run || matches!((at(run), at(&last.run)), (Some(a), Some(b)) if b > a)
    })
}

fn pay_owed<P: Owner, C: ConfigView>(cx: &Cx<'_, P>, board: &mut Board<C>) {
    let owed = std::mem::take(&mut board.owed);
    for o in owed {
        if matches!(&o.what, Owing::EndRun { run } if store_took(board, &o.agent, run)) {
            continue;
        }
        if !send_owed(cx, board, &o) {
            board.owed.push(o);
        }
    }
}

fn raise<P: Owner + Rng, C: ConfigView>(
    cx: &Cx<'_, P>,
    board: &mut Board<C>,
    i: usize,
    raised: Raised,
    kind: OwnItemKind,
) -> Result<(), PortError> {
    let item = ItemId::parse(&minted(cx)?).map_err(|_| PortError::NoService)?;
    store(
        cx,
        board,
        i,
        vec![What::ItemRaised {
            item: item.clone(),
            raised,
        }],
    )?;
    let facts = &mut board.members[i].facts;
    let stored = facts
        .events
        .iter()
        .any(|e| matches!(&e.what, What::ItemRaised { item: r, .. } if *r == item));
    if stored {
        facts.doing = Doing::Idle(Some(OwnItem { item, kind }));
    }
    Ok(())
}

/// Takes the day of the move's read of the clock as the coordinator's day (I5.9).
fn read_day<P, C>(cx: &Cx<'_, P>, board: &mut Board<C>) {
    let day = cx.stamp.day();
    board.day = day;
    for m in &mut board.members {
        m.facts.today = day;
    }
}

fn tick<P, C>(cx: &Cx<'_, P>, board: &mut Board<C>) -> Report
where
    P: Owner<Config = C> + Runs + Analysis + Clock + Rng,
    C: ConfigView + Clone,
{
    read_day(cx, board);
    let listed = listing(cx, board, None).ok();
    let starts = listed.is_some() && fresh(cx, board, None).is_ok();
    let seen = listed.clone().unwrap_or_else(|| known(board));
    let plan = tick_plan(board, &seen, cx.stamp.now(), cx.limits);
    let mut report = Report::default();
    if let Some(ms) = plan.defer_ms {
        let _ = cx.ports.defer(cx.stamp.now().saturating_add(ms));
    }
    for run in &plan.stop {
        stop_run(board, run, cx.stamp.now(), RunEnd::Deleted);
        let _ = cx.ports.cancel(run);
        report.stopped.push(run.clone());
    }
    for run in &plan.recause {
        if let Some(stop) = board
            .runs
            .iter_mut()
            .find(|r| &r.run == run)
            .and_then(|r| r.stop.as_mut())
        {
            stop.cause = RunEnd::Deleted;
        }
    }
    report.dropped.clone_from(&plan.dropped);
    for agent in &plan.dropped {
        board.queue.remove(agent);
        if let Some(i) = board.index(agent) {
            if !matches!(board.members[i].facts.doing, Doing::Live(_)) {
                board.members[i].facts.doing = Doing::Idle(None);
            }
        }
    }
    board.gone.extend(plan.seen.iter().cloned());
    for run in &plan.kill {
        if cx.ports.kill(run).is_err() {
            continue;
        }
        let cause = board
            .runs
            .iter()
            .find(|r| &r.run == run)
            .map(|r| r.stop.map_or(RunEnd::Interrupted, |s| s.cause));
        match cause {
            Some(cause) => report
                .ended
                .push(end_run(cx, board, run, cause, &seen, true, starts)),
            None => board.dying.retain(|d| &d.run != run),
        }
    }
    let Some(listed) = listed else {
        return report;
    };
    pay_owed(cx, board);
    for m in &board.members {
        if starts && wants_active(&m.facts) {
            send_state(cx, &m.facts, AgentState::Active);
        }
    }
    if plan.serve && starts {
        report.next.extend(serve(cx, board, &listed));
    }
    for i in 0..board.members.len() {
        let facts = &board.members[i].facts;
        let agent = facts.agent.clone();
        match tick_step(facts, listed.contains(&facts.workflow.id), cx.limits) {
            Ask::Ready if starts => {
                if raise(cx, board, i, Raised::Ready, OwnItemKind::Ready).is_ok() {
                    report.raised.push(agent);
                }
            }
            Ask::Ready => {}
            Ask::Start if starts => {
                report
                    .started
                    .push(start(cx, board, i, Trigger::Schedule, &listed, false, None));
            }
            Ask::Start => {}
            Ask::Nothing => {}
        }
    }
    report
}

/// The new life's raise of `setup` for every agent of the list whose hold `setup` stands and that
/// holds no open item, one whose item recovery lapsed with `app_ended` among them (I5.8, K7). No
/// run is live and the queue is empty before the life's first creation move, so that an agent
/// with no open item is `idle`.
fn setups_again<P, C>(cx: &Cx<'_, P>, board: &mut Board<C>, list: &[Workflow])
where
    P: Owner + Rng,
    C: ConfigView,
{
    for i in 0..board.members.len() {
        let facts = &board.members[i].facts;
        let listed = list
            .iter()
            .any(|w| matches!(w, Workflow::Full(f) if f.id == facts.workflow.id));
        if listed && facts.doing == Doing::Idle(None) && holds(facts).contains(&Hold::Setup) {
            let _ = raise(cx, board, i, Raised::Setup, OwnItemKind::Setup);
        }
    }
}

fn create<P, C>(cx: &Cx<'_, P>, board: &mut Board<C>) -> Report
where
    P: Owner<Config = C> + Rng,
    C: ConfigView + Clone,
{
    let workflows = match cx.ports.workflows() {
        Ok(w) => w,
        Err(e) => return refused(port_cause(&e)),
    };
    if board.first_listing() {
        setups_again(cx, board, &workflows);
    }
    let mut report = Report::default();
    board.roster_full = false;
    let mut made = Vec::new();
    for w in workflows {
        let Workflow::Full(f) = w else { continue };
        let wanted = f.state == WorkflowState::Approved && f.agent.is_none();
        if !wanted {
            continue;
        }
        if board.members.len() + made.len() >= cx.limits.roster_max as usize {
            board.roster_full = true;
            continue;
        }
        let Ok(Ok(agent)) = minted(cx).map(|h| AgentId::parse(&h)) else {
            continue;
        };
        let body = Body::CreateAgent {
            workflow: f.id.clone(),
            agent: agent.clone(),
        };
        if cx.ports.send(&body).is_ok() {
            report.created.push(agent.clone());
            made.push((f, agent));
        }
    }
    if made.is_empty() {
        return report;
    }
    let Ok(agents) = cx.ports.agents() else {
        return report;
    };
    for (workflow, id) in made {
        let Some(a) = agents
            .iter()
            .find(|a| a.agent == id && a.workflow == workflow.id)
        else {
            continue;
        };
        if board
            .members
            .iter()
            .any(|m| m.facts.workflow.id == workflow.id)
        {
            continue;
        }
        let member = Member {
            facts: Facts {
                agent: id.clone(),
                workflow,
                config: a.config.clone(),
                slots: a.slots.clone(),
                runs: a.last.clone().into_iter().collect(),
                events: Vec::new(),
                store_ended: true,
                today: board.day,
                doing: Doing::Idle(None),
            },
            session: a.session,
        };
        let at = board
            .members
            .partition_point(|m| m.facts.agent < member.facts.agent);
        board.members.insert(at, member);
        if holds(&board.members[at].facts).contains(&Hold::Setup) {
            let _ = raise(cx, board, at, Raised::Setup, OwnItemKind::Setup);
        }
    }
    report
}

/// The body of the person's answer to the check of `run`, from the agent's facts (I5.4, rulings
/// RD-5 and RD-38): with an open item `check`, its `item_answered`; with an open `ready` item and
/// no item `check`, the lapse of that item with the cause `check`, the `item_raised` of a new item
/// `check` and its `item_answered`, in that order; then `hold_cleared` with the run and the
/// answer, and for `went_through` the `slot_skipped` of the slot that the run's `run_started`
/// names, or of none.
fn check_body<P: Rng, C>(
    cx: &Cx<'_, P>,
    facts: &Facts<C>,
    run: &RunId,
    answer: CheckAnswer,
) -> Result<Vec<What>, PortError> {
    let mut whats = Vec::new();
    if let Some(item) = own(&facts.doing, OwnItemKind::Check) {
        whats.push(What::ItemAnswered {
            item,
            answer: Answered::Check(answer),
        });
    } else if let Some(ready) = own(&facts.doing, OwnItemKind::Ready) {
        let check = ItemId::parse(&minted(cx)?).map_err(|_| PortError::NoService)?;
        whats.push(What::ItemLapsed {
            item: ready,
            lapsed: Lapsed::Ready(OtherLapse::Check),
        });
        whats.push(What::ItemRaised {
            item: check.clone(),
            raised: Raised::Check(None),
        });
        whats.push(What::ItemAnswered {
            item: check,
            answer: Answered::Check(answer),
        });
    }
    whats.push(What::HoldCleared {
        cause: Cleared::Check {
            run: run.clone(),
            answer,
        },
    });
    if answer == CheckAnswer::WentThrough {
        let slot = facts.events.iter().find_map(|e| match &e.what {
            What::RunStarted { run: r, slot, .. } if r == run => *slot,
            _ => None,
        });
        whats.push(What::SlotSkipped { slot });
    }
    Ok(whats)
}

/// The person's answer to the check of `run` (I5.4, I7.2, rulings XB-31, RD-5 and RD-38): it logs
/// `check_body` of the agent's facts, and the agent then holds no item `check` or `ready`. After
/// a refusal `seq` it reads the owner's events once and closes the lost starts that they bring
/// (`read_back`); where the hold `check` of that run still stands it logs once more the body that
/// the facts as read give. Where it no longer stands, a `hold_cleared` of that run with the same
/// answer is this answer, stored with a lost acknowledgement, and is taken with nothing logged; one
/// with the other answer makes this answer `stale`; and with none the answer is `state`.
fn answer_check<P: Owner + Rng, C: ConfigView>(
    cx: &Cx<'_, P>,
    board: &mut Board<C>,
    i: usize,
    run: &RunId,
    answer: CheckAnswer,
) -> Report {
    let (day, at) = (cx.stamp.day(), cx.stamp.at());
    let whats = match check_body(cx, &board.members[i].facts, run, answer) {
        Ok(whats) => whats,
        Err(e) => return refused(port_cause(&e)),
    };
    let logged = match log_at(cx, board, i, &whats, day, at) {
        Err(e) if seq_refusal(&e) => {
            if read_back(cx.ports, cx.limits, cx.stamp, board, i).is_err() {
                return refused(port_cause(&e));
            }
            let facts = &board.members[i].facts;
            let stands = holds(facts).contains(&Hold::Check)
                && facts.runs.first().is_some_and(|r| &r.run == run);
            if !stands {
                let given = facts.events.iter().rev().find_map(|e| match &e.what {
                    What::HoldCleared {
                        cause: Cleared::Check { run: r, answer: a },
                    } if r == run => Some(*a),
                    _ => None,
                });
                return match given {
                    Some(a) if a == answer => Report::default(),
                    Some(_) => refused(Cause::Stale),
                    None => refused(Cause::State),
                };
            }
            match check_body(cx, facts, run, answer) {
                Ok(again) => log_at(cx, board, i, &again, day, at),
                Err(e) => Err(e),
            }
        }
        other => other,
    };
    if let Err(e) = logged {
        return refused(port_cause(&e));
    }
    let doing = &board.members[i].facts.doing;
    if own(doing, OwnItemKind::Check).is_some() || own(doing, OwnItemKind::Ready).is_some() {
        board.members[i].facts.doing = without_item(doing);
    }
    Report::default()
}

fn member_move<P, C>(cx: &Cx<'_, P>, board: &mut Board<C>, mv: Move) -> Report
where
    P: Owner<Config = C> + Runs + Analysis + Rng,
    C: ConfigView + Clone,
{
    let agent = match &mv {
        Move::Start { agent }
        | Move::Skip { agent }
        | Move::Check { agent, .. }
        | Move::SwitchOn { agent }
        | Move::SwitchOff { agent } => agent.clone(),
        Move::Tick | Move::End { .. } | Move::Create | Move::Owed => return refused(Cause::State),
    };
    let Some(i) = board.index(&agent) else {
        return refused(Cause::UnknownAgent);
    };
    let list = listing(cx, board, Some(i));
    let read = fresh(cx, board, Some(i));
    let facts = &board.members[i].facts;
    let standing = holds(facts);
    match mv {
        Move::Start { .. } => {
            let listed = match list {
                Ok(listed) => listed,
                Err(e) => return refused(port_cause(&e)),
            };
            let unread = read.err().map(|e| port_cause(&e));
            let s = start(cx, board, i, Trigger::Person, &listed, false, unread);
            Report {
                answer: s.answer,
                run: s.run,
                ..Report::default()
            }
        }
        Move::Skip { .. } => {
            if let Err(e) = list {
                return refused(port_cause(&e));
            }
            let Some(item) = own(&facts.doing, OwnItemKind::Ready) else {
                return refused(Cause::State);
            };
            let slot = current_slot(&facts.workflow, facts.today)
                .filter(|s| finished(facts) && slot_open(facts, *s));
            let whats = vec![
                What::ItemAnswered {
                    item: item.clone(),
                    answer: Answered::Ready(ReadyAnswer::Skip),
                },
                What::SlotSkipped { slot },
            ];
            let pressed = |facts: &Facts<C>| {
                let open = own(&facts.doing, OwnItemKind::Ready).as_ref() == Some(&item);
                let skipped = facts.events.iter().any(|e| {
                    matches!(&e.what, What::ItemAnswered { item: i, answer: Answered::Ready(ReadyAnswer::Skip) } if *i == item)
                });
                (!open && !skipped).then_some(Cause::Stale)
            };
            let (day, at) = (cx.stamp.day(), cx.stamp.at());
            match store_read(cx, board, i, whats, day, at, Some(&pressed)) {
                Ok(Stored::Decided(c)) => refused(c),
                Ok(_) => {
                    let doing = without_item(&board.members[i].facts.doing);
                    board.members[i].facts.doing = doing;
                    Report::default()
                }
                Err(e) => refused(port_cause(&e)),
            }
        }
        Move::Check { answer, .. } => {
            if list
                .as_ref()
                .is_ok_and(|listed| !listed.contains(&facts.workflow.id))
            {
                return refused(Cause::State);
            }
            if let Err(e) = read {
                return refused(port_cause(&e));
            }
            let Some(newest) = facts
                .runs
                .first()
                .filter(|_| standing.contains(&Hold::Check))
            else {
                return refused(Cause::State);
            };
            let run = newest.run.clone();
            answer_check(cx, board, i, &run, answer)
        }
        Move::SwitchOn { .. } => {
            let mut whats = Vec::new();
            if standing.contains(&Hold::Off) {
                whats.push(What::HoldCleared {
                    cause: Cleared::Person,
                });
            }
            if standing.contains(&Hold::Failures) {
                whats.push(What::HoldCleared {
                    cause: Cleared::Failures,
                });
            }
            if whats.is_empty() {
                return refused(Cause::State);
            }
            if let Err(e) = store(cx, board, i, whats) {
                return refused(port_cause(&e));
            }
            let facts = &board.members[i].facts;
            let state = if gives_active(facts) {
                AgentState::Active
            } else {
                AgentState::Approved
            };
            send_state(cx, facts, state);
            Report::default()
        }
        Move::SwitchOff { .. } => {
            if standing.contains(&Hold::Off) {
                return refused(Cause::State);
            }
            let set = What::HoldSet {
                cause: HoldSetCause::Person,
            };
            if let Err(e) = store(cx, board, i, vec![set]) {
                return refused(port_cause(&e));
            }
            let mut report = Report::default();
            let live = board
                .runs
                .iter()
                .find(|r| r.agent == agent && r.stop.is_none())
                .map(|r| r.run.clone());
            if let Some(run) = live {
                stop_run(board, &run, cx.stamp.now(), RunEnd::Cancelled);
                let _ = cx.ports.cancel(&run);
                report.stopped.push(run);
            }
            send_state(cx, &board.members[i].facts, AgentState::Paused);
            report
        }
        Move::Tick | Move::End { .. } | Move::Create | Move::Owed => refused(Cause::State),
    }
}

/// Carries out a move on the ports and the board, and is the one function of the schedule
/// that calls a port (I5.10). A tick, a start by the person and an end read the day first
/// (ruling XB-10). A tick works in the order of I5.9: it reads the day; sends the
/// request to defer again when a run is live before it; stops, by `Runs::cancel` and with the
/// cause `deleted`, each run of a workflow that the owner's list no longer holds and drops that
/// agent's queued start and open item with nothing stored (I5.7); kills by `Runs::kill`, in the
/// order of their starts, every run whose stop came at least `cancel_grace_s` before it, live
/// or ended in between, and ends each live one with the cause of its stop; serves the queue
/// only when no run was live before it; and then raises the `ready` items and makes the starts
/// that `tick_step` gives, agent by agent; it serves and starts only when its reads of the
/// owner's list and of `Owner::agents` answered, and raises a `ready` item only then. Each
/// move decides from what the owner answers in that move (ruling XB-9): the workflow's state,
/// trigger, period and `last` from the owner's list that it reads, and from the agent's entry
/// of `Owner::agents` its configuration, its slot names, its session, whether the agent store
/// has ended its newest run and that run's record (ruling XB-2). A start of the person whose
/// read of the agents fails keeps the cause of a step that the owner's list and the stored
/// events decide (step 0, the hold `off`, step 2, a live or queued run), and is answered with
/// the read's cause, `busy` or `no_service`, wherever the entry or the store would decide. A
/// start goes through `start_step` and then steps 5 to 8 of I5.5: the request to defer, the `run_started` event, which closes an open `ready`
/// item in the same body, the body `start_run` and `Runs::start`; a failed step 7 or 8 stores
/// `run_ended` with `failed` and answers `no_service`. The end of a run stores `run_ended`
/// unless its workflow is gone, with `hold_set` of `failures` and `set_state` `paused` when the
/// last `fails_to_pause` ends each have a cause of the failure class, which no end can find
/// while that hold stands, and then takes one start from the queue; a failed start takes none. After what is owed
/// and before any start, a tick sends `set_state` `active` for each agent that `wants_active`
/// names (ruling IG-6a), again at each tick until the owner's list shows it `active`. Once a
/// start has stored `run_started`, every later step reads the agent's newest run as not ended
/// by the agent store until the agent's entry of `Owner::agents` is read again (ruling XB-24).
/// What is owed is sent at each tick until the owner acknowledges it, also for a workflow that
/// the owner's list no longer holds, whose debt the owner's refusal `no_workflow` ends (ruling
/// XB-26); an owed `end_run` is acknowledged too once the tick's read of the agent store's last
/// record names its run or a later run of the agent (I5.8, ruling XB-30), and an owed
/// `run_ended` or `item_lapsed` that the owner refuses with `seq` is paid when the owner's
/// events, read again, hold it (I7.2). An owed `run_ended` or `item_lapsed` is paid with no log
/// when the agent's events already hold the end of that run or a close of that item, which a
/// read after a refusal `seq` brought in (ruling RD-11). Every log of a move that the owner
/// refuses with `seq` reads the owner's events of the agent after the last one that the board
/// holds, logs nothing of what they already hold (the same event, an open item of the same kind,
/// an item that they close, an end of the run), takes the agent's open item from them, and logs
/// the rest once more; a second refusal waits for the next move (I7.2, ruling XB-31).
pub fn drive<P>(
    ports: &P,
    board: &mut Board<P::Config>,
    limits: &crate::types::Limits,
    mv: Move,
) -> Report
where
    P: Owner + Runs + Analysis + Clock + Rng,
    P::Config: ConfigView + Clone,
{
    let stamp = match Stamp::read(ports) {
        Ok(stamp) => stamp,
        Err(e) => return refused(port_cause(&e)),
    };
    let cx = Cx {
        ports,
        limits,
        stamp,
    };
    if matches!(mv, Move::Start { .. } | Move::End { .. }) {
        read_day(&cx, board);
    }
    match mv {
        Move::Tick => tick(&cx, board),
        Move::Create => create(&cx, board),
        Move::Owed => {
            pay_owed(&cx, board);
            Report::default()
        }
        Move::End { run, outcome } => {
            let Some(live) = board.runs.iter().find(|r| r.run == run) else {
                return refused(Cause::State);
            };
            let cause = match (live.stop, outcome) {
                (Some(stop), _) => stop.cause,
                (None, Some(outcome)) => RunEnd::from(outcome),
                (None, None) => RunEnd::Interrupted,
            };
            let listed = listing(&cx, board, None).ok();
            let serving = listed.is_some() && fresh(&cx, board, None).is_ok();
            let listed = listed.unwrap_or_else(|| known(board));
            Report {
                ended: vec![end_run(&cx, board, &run, cause, &listed, false, serving)],
                ..Report::default()
            }
        }
        other => member_move(&cx, board, other),
    }
}
