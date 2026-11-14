use crate::ports::{Clock, Owner, Rng, Stamp};
use crate::schedule::{holds, read_back, seq_refusal, Board, Hold};
use crate::types::{
    hex, AgentId, ConfigView, Doing, Event, Facts, ItemId, Lapsed, Limits, OtherLapse, OwnItem,
    OwnItemKind, Raised, What,
};
use std::num::NonZeroU32;

/// Whether the coordinator raises the item `check` for the agent of `facts` in a move that has
/// read the agent store's record (I5.4, rulings XB-18 and RD-5): the agent is idle with no open
/// item or with an open `ready` item, which the raise lapses, its workflow is not gone, and the
/// hold `check` stands, which it does once the store has ended the agent's newest run with
/// `unknown` above 0 and no answer to a `check` names that run. An open `setup` or `check` item
/// holds the raise back.
pub fn check_due<C: ConfigView>(facts: &Facts<C>, gone: bool) -> bool {
    let idle = match &facts.doing {
        Doing::Idle(None) => true,
        Doing::Idle(Some(own)) => own.kind == OwnItemKind::Ready,
        Doing::Queued(_) | Doing::Live(_) => false,
    };
    !gone && idle && holds(facts).contains(&Hold::Check)
}

/// Raises the item `check` for each agent of the board, or for `only`, for which `check_due`
/// holds: after the agent's last event it stores, in one body, `item_lapsed` of the agent's open
/// `ready` item with the cause `check` where it holds one, and then `item_raised` of the kind
/// `check` with no run; the agent holds the item `check` once the owner acknowledged the body,
/// and the lapse stores no `slot_skipped` (I5.4, ruling RD-5). When the owner refuses the body
/// with `seq`, the move reads the owner's events and closes the lost starts that they bring
/// (`read_back`), and goes on from there, once: the agent holds an item `check` that those events
/// raised and left open, or else the body is made again from the item that the agent holds after
/// the read and logged once more while `check_due` holds (rulings XB-31, RD-34 (b), RD-5).
/// The agents that hold the item `check` so, in ascending agent id.
pub(super) fn raise_checks<P, C>(
    ports: &P,
    board: &mut Board<C>,
    limits: &Limits,
    only: Option<&AgentId>,
) -> Vec<AgentId>
where
    P: Owner + Rng + Clock,
    C: ConfigView,
{
    let Ok(stamp) = Stamp::read(ports) else {
        return Vec::new();
    };
    let (day, at) = (stamp.day(), stamp.at());
    let mut raised = Vec::new();
    for i in 0..board.members.len() {
        let agent = board.members[i].facts.agent.clone();
        let gone = board.gone.contains(&board.members[i].facts.workflow.id);
        if only.is_some_and(|a| a != &agent) || !check_due(&board.members[i].facts, gone) {
            continue;
        }
        let Ok(Ok(item)) = ports.bytes16().map(|b| ItemId::parse(&hex(b))) else {
            continue;
        };
        let body = |facts: &Facts<C>| -> Vec<Event> {
            let last = facts.events.last().map_or(0, |e| e.seq.get());
            let lapse = match &facts.doing {
                Doing::Idle(Some(own)) if own.kind == OwnItemKind::Ready => {
                    Some(What::ItemLapsed {
                        item: own.item.clone(),
                        lapsed: Lapsed::Ready(OtherLapse::Check),
                    })
                }
                _ => None,
            };
            let raise = What::ItemRaised {
                item: item.clone(),
                raised: Raised::Check(None),
            };
            lapse
                .into_iter()
                .chain(std::iter::once(raise))
                .zip(0u32..)
                .map(|(what, k)| Event {
                    workflow: facts.workflow.id.clone(),
                    agent: facts.agent.clone(),
                    seq: NonZeroU32::MIN.saturating_add(last.saturating_add(k)),
                    day,
                    at,
                    what,
                })
                .collect()
        };
        let mut events = body(&board.members[i].facts);
        let mut held = None;
        match ports.log(&events) {
            Ok(()) => held = Some(item.clone()),
            Err(e) if seq_refusal(&e) => {
                if read_back(ports, limits, stamp, board, i).is_err() {
                    continue;
                }
                let doing = &board.members[i].facts.doing;
                if matches!(doing, Doing::Idle(Some(own)) if own.kind == OwnItemKind::Check) {
                    raised.push(agent);
                    continue;
                }
                if !check_due(&board.members[i].facts, gone) {
                    continue;
                }
                events = body(&board.members[i].facts);
                if ports.log(&events).is_ok() {
                    held = Some(item.clone());
                }
            }
            Err(_) => {}
        }
        if let Some(item) = held {
            let facts = &mut board.members[i].facts;
            facts.events.extend(events);
            facts.doing = Doing::Idle(Some(OwnItem {
                item,
                kind: OwnItemKind::Check,
            }));
            raised.push(agent);
        }
    }
    raised
}
