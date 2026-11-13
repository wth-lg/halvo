use super::check::raise_checks;
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

fn history<P: Owner>(ports: &P, agent: &AgentId) -> Result<Vec<Event>, PortError> {
    let mut events: Vec<Event> = Vec::new();
    resync(ports, agent, &mut events)?;
    Ok(events)
}

/// Stores recovery's body for one agent: `run_ended` with `interrupted` for each run that ends
/// and `item_lapsed` with `app_ended` for each open item, after the agent's last event, which
/// the agent's events take once the owner acknowledged them. An empty body calls no port.
fn log_body<P: Owner, C>(ports: &P, f: &mut Found<C>, day: u32, at: i64) -> Result<(), PortError> {
    let mut whats: Vec<What> = f
        .step
        .ended
        .iter()
        .map(|run| What::RunEnded {
            run: run.clone(),
            cause: RunEnd::Interrupted,
        })
        .collect();
    whats.extend(f.step.lapsed.iter().map(|(item, lapsed)| What::ItemLapsed {
        item: item.clone(),
        lapsed: *lapsed,
    }));
    if whats.is_empty() {
        return Ok(());
    }
    let last = f.events.last().map_or(0, |e| e.seq.get());
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
    ports.log(&events)?;
    f.events.extend(events);
    Ok(())
}

/// The workflow of an agent of a new life as the owner's list holds it (ruling XB-28 (a)): the
/// full shape as it is, and the dismissed shape with its id, its category and its site, the
/// state `paused`, the trigger `manual` and no period, so that no tick starts its agent or
/// sends `active` for it.
fn known(workflow: &Workflow, agent: &AgentId, day: u32) -> FullWorkflow {
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
/// calls `Runs::reap` before anything else, so that no `Runs::start` comes before it, and
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
/// with no item, an empty queue and no live run; then recovery raises the item `check` where
/// `check_due` holds, since it read the store's records.
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
    for agent in agents {
        let Some(workflow) = workflows
            .iter()
            .find(|w| w.id() == &agent.workflow)
            .map(|w| known(w, &agent.agent, day))
        else {
            continue;
        };
        let events = history(ports, &agent.agent).map_err(|e| cause(&e))?;
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
        let mut logged = log_body(ports, f, day, at);
        if matches!(&logged, Err(e) if seq_refusal(e))
            && resync(ports, &f.agent.agent, &mut f.events).is_ok()
        {
            f.step = f.step(&reaped);
            logged = log_body(ports, f, day, at);
        }
        ended.extend(f.step.ended.iter().map(|run| Ended {
            run: run.clone(),
            cause: RunEnd::Interrupted,
            next: Vec::new(),
        }));
        if logged.is_err() {
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
            if ports.send(&Body::EndRun { run: run.clone() }).is_err() {
                owed.push(Owed {
                    agent: f.agent.agent.clone(),
                    workflow: f.workflow.id.clone(),
                    what: Owing::EndRun { run: run.clone() },
                });
            }
        }
    }
    let history_runs = limits.history_runs as usize;
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
            Member {
                facts: Facts {
                    agent: f.agent.agent.clone(),
                    workflow: f.workflow,
                    config: f.agent.config,
                    slots: f.agent.slots,
                    runs,
                    events: f.events,
                    store_ended,
                    today: day,
                    doing: Doing::Idle(None),
                },
                session: f.agent.session,
            }
        })
        .collect();
    let mut board = Board::new(day, members);
    board.owed = owed;
    let checked = raise_checks(ports, &mut board, None);
    Ok(Life {
        board,
        ended,
        checked,
    })
}
