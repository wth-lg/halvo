use crate::ports::{Clock, Owner, Rng, Stamp};
use crate::schedule::{holds, read_after_seq, seq_refusal, Board, Hold};
use crate::types::{
    hex, AgentId, ConfigView, Doing, Event, Facts, ItemId, OwnItem, OwnItemKind, Raised, What,
};
use std::num::NonZeroU32;

/// Whether the coordinator raises the item `check` for the agent of `facts` in a move that has
/// read the agent store's record (I5.4, ruling XB-18): the agent is idle with no open item,
/// its workflow is not gone, and the hold `check` stands, which it does once the store has
/// ended the agent's newest run with `unknown` above 0 and no answer to a `check` names that
/// run.
pub fn check_due<C: ConfigView>(facts: &Facts<C>, gone: bool) -> bool {
    !gone && facts.doing == Doing::Idle(None) && holds(facts).contains(&Hold::Check)
}

/// Raises the item `check` for each agent of the board, or for `only`, for which `check_due`
/// holds: it stores `item_raised` of the kind `check` with no run, after the agent's last
/// event, and the agent holds the item once the owner acknowledged it. When the owner refuses
/// the log with `seq`, the facts take the owner's events after their last one and the move goes
/// on from there, once: the agent holds an item of the coordinator's own kinds that those events
/// raised and left open, and else the item `check` is logged once more after them while
/// `check_due` holds (ruling XB-31). The agents that hold the item `check` so, in ascending
/// agent id.
pub(super) fn raise_checks<P, C>(
    ports: &P,
    board: &mut Board<C>,
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
        let gone = board.gone.contains(&board.members[i].facts.workflow.id);
        let facts = &mut board.members[i].facts;
        if only.is_some_and(|a| a != &facts.agent) || !check_due(facts, gone) {
            continue;
        }
        let Ok(Ok(item)) = ports.bytes16().map(|b| ItemId::parse(&hex(b))) else {
            continue;
        };
        let check = |facts: &Facts<C>| Event {
            workflow: facts.workflow.id.clone(),
            agent: facts.agent.clone(),
            seq: NonZeroU32::MIN.saturating_add(facts.events.last().map_or(0, |e| e.seq.get())),
            day,
            at,
            what: What::ItemRaised {
                item: item.clone(),
                raised: Raised::Check(None),
            },
        };
        let mut event = check(facts);
        let mut held = None;
        match ports.log(std::slice::from_ref(&event)) {
            Ok(()) => held = Some(item.clone()),
            Err(e) if seq_refusal(&e) => {
                if read_after_seq(ports, facts).is_err() {
                    continue;
                }
                if let Doing::Idle(Some(own)) = &facts.doing {
                    if own.kind == OwnItemKind::Check {
                        raised.push(facts.agent.clone());
                    }
                    continue;
                }
                if !check_due(facts, gone) {
                    continue;
                }
                event = check(facts);
                if ports.log(std::slice::from_ref(&event)).is_ok() {
                    held = Some(item.clone());
                }
            }
            Err(_) => {}
        }
        if let Some(item) = held {
            facts.events.push(event);
            facts.doing = Doing::Idle(Some(OwnItem {
                item,
                kind: OwnItemKind::Check,
            }));
            raised.push(facts.agent.clone());
        }
    }
    raised
}
