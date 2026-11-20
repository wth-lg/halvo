use super::check::{raise_checks, read_replies};
use crate::items::drive::{class_of, Class};
use crate::ports::{Analysis, Clock, Owner, PortError, Reaped, Rng, Runs, Stamp};
use crate::schedule::{item_closed, resync, seq_refusal, Board, Ended, Member, Owed, Owing};
use crate::types::{
    Agent, AgentId, Body, Cause, ConfigView, Doing, Event, Facts, FullWorkflow, ItemId, LapseCause,
    Lapsed, Limits, OtherLapse, PastRun, Raised, RunEnd, RunId, What, Workflow, WorkflowState,
    WorkflowTrigger,
};
use std::num::NonZeroU32;

/// What recovery reads of one agent of an earlier life: its ledger events, every one, in
/// ascending `seq`, and the agent store's last run record of it, or none (I5.8).
#[derive(Clone, Copy, Debug)]
pub struct Earlier<'a> {
    /// The agent.
    pub agent: &'a AgentId,
    /// Its ledger events.
    pub events: &'a [Event],
    /// The agent store's last run record of it.
    pub last: Option<&'a PastRun>,
}

/// What recovery does for one agent (I5.8): the runs whose `run_started` has no `run_ended`,
/// which end `interrupted`; the items of the earlier life that are still open, each with its
/// `item_lapsed` of the cause `app_ended`; and the run whose `end_run` is owed, or none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recovery {
    /// The runs that end `interrupted`, in the order of their `run_started` events.
    pub ended: Vec<RunId>,
    /// The open items with their lapse.
    pub lapsed: Vec<(ItemId, Lapsed)>,
    /// The run whose `end_run` is owed.
    pub owed: Option<RunId>,
}

fn lapse_of(raised: &Raised) -> Lapsed {
    let other = OtherLapse::AppEnded;
    match raised {
        Raised::Confirm(_) => Lapsed::Confirm(LapseCause::AppEnded),
        Raised::SignIn(_) => Lapsed::SignIn(other),
        Raised::LinkAccount(_) => Lapsed::LinkAccount(other),
        Raised::Stuck(_) => Lapsed::Stuck(other),
        Raised::Check(_) => Lapsed::Check(other),
        Raised::Setup => Lapsed::Setup(other),
        Raised::Ready => Lapsed::Ready(other),
        Raised::Show(_) => Lapsed::Show(other),
    }
}

/// Recovery of one agent from what an earlier life left (I5.8, rulings XB-3, XB-16, XB-20,
/// H-4): every run whose `run_started` event has no `run_ended` event ends `interrupted`,
/// whatever `reap` answered; every item with an `item_raised` event and neither
/// `item_released` nor `item_lapsed`, and for a kind other than `confirm` no `item_answered`,
/// lapses with `app_ended`; and `end_run` is owed for the newest run whose `run_started` the
/// events hold when the store's last record does not name it, unless `reap` answered `alive`
/// for it. What is owed is read from the events and the store's record alone; a run with no
/// mark, which `reap` does not answer, is owed as one answered `gone`.
pub fn recover_step(earlier: &Earlier<'_>, reaped: &[(RunId, Reaped)]) -> Recovery {
    let started: Vec<&RunId> = earlier
        .events
        .iter()
        .filter_map(|e| match &e.what {
            What::RunStarted { run, .. } => Some(run),
            _ => None,
        })
        .collect();
    let has_end = |run: &RunId| {
        earlier
            .events
            .iter()
            .any(|e| matches!(&e.what, What::RunEnded { run: r, .. } if r == run))
    };
    let ended = started
        .iter()
        .filter(|run| !has_end(run))
        .map(|run| (*run).clone())
        .collect();
    let mut lapsed: Vec<(ItemId, Lapsed)> = Vec::new();
    for e in earlier.events {
        if let What::ItemRaised { item, raised } = &e.what {
            if !item_closed(earlier.events, item) && !lapsed.iter().any(|(i, _)| i == item) {
                lapsed.push((item.clone(), lapse_of(raised)));
            }
        }
    }
    let owed = started.last().and_then(|newest| {
        let named = earlier.last.is_some_and(|l| &l.run == *newest);
        let alive = reaped
            .iter()
            .any(|(run, answer)| run == *newest && *answer == Reaped::Alive);
        (!named && !alive).then(|| (*newest).clone())
    });
    Recovery {
        ended,
        lapsed,
        owed,
    }
}

/// A new life of the coordinator, as `recover` builds it: the board, and the runs that
/// recovery ended `interrupted`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Life<C> {
    /// The board of the new life.
    pub board: Board<C>,
    /// The runs that recovery ended, each with the cause `interrupted` and no start taken.
    pub ended: Vec<Ended>,
    /// The agents for which recovery raised the item `check` (ruling XB-18).
    pub checked: Vec<AgentId>,
}

fn cause(error: &PortError) -> Cause {
    match error {
        PortError::Busy => Cause::Busy,
        PortError::NoService | PortError::Refused(_) => Cause::NoService,
    }
}

fn history<P: Owner>(ports: &P, agent: &AgentId) -> Result<(Vec<Event>, u32), PortError> {
    let mut events: Vec<Event> = Vec::new();
    let last = resync(ports, agent, &mut events)?;
    Ok((events, last))
}

/// Stores recovery's body for one agent: `run_ended` with `interrupted` for each run that ends
/// and `item_lapsed` with `app_ended` for each open item, after the agent's last event, which
/// the agent's events take once the owner acknowledged them. The rows go in bodies of at most
/// `request_records` records and `request_bytes` bytes, each row a debt of its own, so that a
/// body is cut between the rows of two debts and never inside one (I7.2, ruling RD-64 2.2). An
/// empty body calls no port.
fn log_recovery_body<P: Owner, C>(
    ports: &P,
    f: &mut Found<C>,
    (day, at): (u32, i64),
    limits: &Limits,
) -> Result<(), PortError> {
    let mut whats: Vec<What> = f
        .step
        .ended
        .iter()
        .map(|run| What::RunEnded {
            run: run.clone(),
            cause: RunEnd::Interrupted,
            defers: None,
        })
        .collect();
    whats.extend(f.step.lapsed.iter().map(|(item, lapsed)| What::ItemLapsed {
        item: item.clone(),
        lapsed: *lapsed,
    }));
    if whats.is_empty() {
        return Ok(());
    }
    let last = f.last;
    let events: Vec<Event> = whats
        .into_iter()
        .zip(0u32..)
        .map(|(what, k)| Event {
            workflow: f.workflow.id.clone(),
            agent: f.agent.agent.clone(),
            seq: NonZeroU32::MIN.saturating_add(last.saturating_add(k)),
            day,
            at,
            what,
        })
        .collect();
    let mut sent = 0;
    while sent < events.len() {
        let n = crate::items::drive::body_len(&events[sent..], limits);
        ports.log(&events[sent..sent + n])?;
        f.events.extend_from_slice(&events[sent..sent + n]);
        sent += n;
        f.last = events[sent - 1].seq.get();
    }
    Ok(())
}

/// The workflow of an agent of a new life as the owner's list holds it (ruling XB-28 (a)): the
/// full shape as it is, and the dismissed shape with its id, its category and its site, the
/// state `paused`, the trigger `manual` and no period, so that no tick starts its agent or
/// sends `active` for it.
fn full_or_paused_manual(workflow: &Workflow, agent: &AgentId, day: u32) -> FullWorkflow {
    match workflow {
        Workflow::Full(f) => f.clone(),
        Workflow::Dismissed(d) => FullWorkflow {
            id: d.id.clone(),
            state: WorkflowState::Paused,
            category: d.category,
            site: d.site.clone(),
            trigger: WorkflowTrigger::Manual,
            period: None,
            last: day,
            agent: Some(agent.clone()),
        },
    }
}

fn dismissed_in(list: &[Workflow], id: &crate::types::WorkflowId) -> bool {
    list.iter()
        .any(|w| matches!(w, Workflow::Dismissed(d) if &d.id == id))
}

fn started_at(events: &[Event], run: &RunId) -> i64 {
    events
        .iter()
        .find_map(|e| match &e.what {
            What::RunStarted { run: r, .. } if r == run => Some(e.at),
            _ => None,
        })
        .unwrap_or(i64::MAX)
}

struct Found<C> {
    agent: Agent<C>,
    workflow: FullWorkflow,
    events: Vec<Event>,
    last: u32,
    step: Recovery,
}

impl<C> Found<C> {
    fn step(&self, reaped: &[(RunId, Reaped)]) -> Recovery {
        recover_step(
            &Earlier {
                agent: &self.agent.agent,
                events: &self.events,
                last: self.agent.last.as_ref(),
            },
            reaped,
        )
    }
}

/// The recovery of a new life of the app (I5.8, I5.10), the coordinator's first move: it
/// calls `Runs::reap` so that no `Runs::start` comes before it, and
/// answers that call's error with no board, so that no start can come before `reap` has
/// answered. It reads the owner's list of workflows and every agent of `Owner::agents` whose
/// workflow the list holds, with every ledger event of the agent, page by page of at most 64,
/// and the store's last record of it; then, by `recover_step` and in the order in which the
/// runs started, it stores for each agent `run_ended` with `interrupted` for each run that
/// ends and `item_lapsed` with `app_ended` for each open item, in one body; and sends
/// `end_run` for each owed run. When the owner refuses a body with `seq`, the agent's events
/// take the owner's events after their last one, and the body that `recover_step` reads from
/// them is logged once more (ruling XB-31). A body that the owner does not acknowledge, with its
/// `run_ended` and its `item_lapsed` entries, and an `end_run` that it does not acknowledge, are
/// owed on the board, which the schedule's tick sends again (XB-32). The board takes the facts so read, with the day of the clock, an idle agent
/// with no item, an empty queue and no live run, and keeps the runs for which `reap` answered
/// `alive` (`Board::alive_at_reap`; ruling RD-147 (5)(i)); then recovery raises the item `check` where
/// `check_due` holds, since it read the store's records, and that raise lapses with the cause
/// `check` each `ready` item whose `app_ended` lapse is owed (ruling RD-96).
/// Where `reap` answered `alive` for a run, recovery also reads the history of each agent whose
/// workflow the list lacks: `Busy` and `NoService` fail the recovery, as for a listed agent;
/// `no_workflow` leaves the agent out; any other refusal keeps it aside for
/// `Board::reread_gone`; and where its newest run is one that `reap` answered `alive` and that
/// no record of the store names, the board keeps its row (`Board::keep_gone`; ruling RD-150).
/// A runner that ended before this life's `reap`, as one does within `LINK_END` (20 s) of the
/// link's close, is answered `gone`, and its run gets no line (a named limit, RD-150).
pub fn recover<P>(ports: &P, limits: &Limits) -> Result<Life<P::Config>, Cause>
where
    P: Owner + Runs + Analysis + Clock + Rng,
    P::Config: ConfigView + Clone,
{
    let stamp = Stamp::read(ports).map_err(|e| cause(&e))?;
    let day = stamp.day();
    let reaped = ports.reap().map_err(|e| cause(&e))?;
    let workflows = ports.workflows().map_err(|e| cause(&e))?;
    let agents = ports.agents().map_err(|e| cause(&e))?;
    let mut found: Vec<Found<P::Config>> = Vec::new();
    let mut aside: Vec<AgentId> = Vec::new();
    let mut unread: Vec<AgentId> = Vec::new();
    let mut kept: Vec<Facts<P::Config>> = Vec::new();
    let mut gone_aside: Vec<Agent<P::Config>> = Vec::new();
    for agent in agents {
        let Some(listed) = workflows.iter().find(|w| w.id() == &agent.workflow) else {
            match Board::gone_facts(ports, agent.clone(), &reaped, day) {
                Ok(read) => kept.extend(read),
                Err(PortError::Refused(name)) if name.as_str() == "no_workflow" => {}
                Err(PortError::Refused(_)) => gone_aside.push(agent),
                Err(e) => return Err(cause(&e)),
            }
            continue;
        };
        let workflow = full_or_paused_manual(listed, &agent.agent, day);
        let (events, last) = match history(ports, &agent.agent) {
            Ok(read) => read,
            Err(PortError::Refused(name)) => match name.as_str() {
                "no_workflow" => continue,
                "binding" if matches!(listed, Workflow::Dismissed(_)) => {
                    unread.push(agent.agent.clone());
                    (Vec::new(), 0)
                }
                _ => {
                    aside.push(agent.agent.clone());
                    (Vec::new(), 0)
                }
            },
            Err(e) => return Err(cause(&e)),
        };
        let step = recover_step(
            &Earlier {
                agent: &agent.agent,
                events: &events,
                last: agent.last.as_ref(),
            },
            &reaped,
        );
        found.push(Found {
            agent,
            workflow,
            events,
            last,
            step,
        });
    }
    found.sort_by(|a, b| {
        let first = |f: &Found<P::Config>| {
            f.step
                .ended
                .first()
                .or(f.step.owed.as_ref())
                .map_or(i64::MAX, |r| started_at(&f.events, r))
        };
        first(a)
            .cmp(&first(b))
            .then_with(|| a.agent.agent.cmp(&b.agent.agent))
    });
    let at = stamp.at();
    let mut owed: Vec<Owed> = Vec::new();
    let mut ended: Vec<Ended> = Vec::new();
    for f in &mut found {
        let mut logged = log_recovery_body(ports, f, (day, at), limits);
        let reread = matches!(&logged, Err(e) if seq_refusal(e))
            .then(|| resync(ports, &f.agent.agent, &mut f.events));
        if let Some(Ok(last)) = reread {
            f.last = last;
            f.step = f.step(&reaped);
            logged = log_recovery_body(ports, f, (day, at), limits);
        }
        let dismissed = dismissed_in(&workflows, &f.workflow.id);
        let gone = matches!(&logged, Err(e) if class_of(e, dismissed) == Some(Class::Gone));
        ended.extend(f.step.ended.iter().map(|run| Ended {
            run: run.clone(),
            cause: RunEnd::Interrupted,
            next: Vec::new(),
        }));
        if logged.is_err() && !gone {
            let runs = f.step.ended.iter().map(|run| Owing::RunEnded {
                run: run.clone(),
                cause: RunEnd::Interrupted,
                day,
                at,
            });
            let lapses = f
                .step
                .lapsed
                .iter()
                .map(|(item, lapsed)| Owing::ItemLapsed {
                    item: item.clone(),
                    lapsed: *lapsed,
                    day,
                    at,
                });
            owed.extend(runs.chain(lapses).map(|what| Owed {
                agent: f.agent.agent.clone(),
                workflow: f.workflow.id.clone(),
                what,
            }));
        }
    }
    for f in &found {
        if let Some(run) = &f.step.owed {
            let sent = ports.send(&Body::EndRun { run: run.clone() });
            let dismissed = dismissed_in(&workflows, &f.workflow.id);
            if matches!(&sent, Err(e) if class_of(e, dismissed) != Some(Class::Gone)) {
                owed.push(Owed {
                    agent: f.agent.agent.clone(),
                    workflow: f.workflow.id.clone(),
                    what: Owing::EndRun { run: run.clone() },
                });
            }
        }
    }
    let history_runs = limits.history_runs as usize;
    let lasts: Vec<(AgentId, u32)> = found
        .iter()
        .filter(|f| !aside.contains(&f.agent.agent))
        .map(|f| (f.agent.agent.clone(), f.last))
        .collect();
    let members = found
        .into_iter()
        .map(|f| {
            let newest = f.events.iter().rev().find_map(|e| match &e.what {
                What::RunStarted { run, .. } => Some(run.clone()),
                _ => None,
            });
            let store_ended = match &newest {
                None => true,
                Some(run) => f.agent.last.as_ref().is_some_and(|l| &l.run == run),
            };
            let mut runs: Vec<PastRun> = f.agent.last.clone().into_iter().collect();
            runs.truncate(history_runs);
            let mut facts = Facts {
                agent: f.agent.agent.clone(),
                workflow: f.workflow,
                config: f.agent.config,
                slots: f.agent.slots,
                runs,
                unknown: Vec::new(),
                events: f.events,
                store_ended,
                replies: crate::types::Replies::NotAsked,
                today: day,
                doing: Doing::Idle(None),
            };
            facts.take_unknown(&f.agent.unknown);
            read_replies(ports, &mut facts);
            Member {
                facts,
                session: f.agent.session,
            }
        })
        .collect();
    let mut board = Board::new(day, members);
    for (agent, last) in lasts {
        if let Some(i) = board.members.iter().position(|m| m.facts.agent == agent) {
            board.set_last(i, last);
        }
    }
    board.owed = owed;
    board.keep_alive(&reaped);
    board.note_records();
    for facts in kept {
        board.keep_gone(facts);
    }
    for agent in gone_aside {
        board.set_gone_aside(agent, reaped.clone());
    }
    for agent in &aside {
        board.set_aside(agent, reaped.clone());
    }
    for agent in &unread {
        board.keep_unread(agent);
    }
    board.note_list(&workflows);
    let checked = raise_checks(ports, &mut board, limits, None);
    Ok(Life {
        board,
        ended,
        checked,
    })
}
