use crate::ports::{Clock, Owner, Rng};
use crate::schedule::{holds, Board, Hold};
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
/// event, and the agent holds the item once the owner acknowledged it. The agents whose item
/// was raised, in ascending agent id.
pub(super) fn raise_checks<P, C>(
    ports: &P,
    board: &mut Board<C>,
    only: Option<&AgentId>,
) -> Vec<AgentId>
where
    P: Owner + Rng + Clock,
    C: ConfigView,
{
    let Ok(now) = ports.now() else {
        return Vec::new();
    };
    let at = i64::try_from(now)
        .unwrap_or(i64::MAX)
        .saturating_mul(1_000_000);
    let day = board.day;
    let mut raised = Vec::new();
    for i in 0..board.members.len() {
        let facts = &board.members[i].facts;
        let gone = board.gone.contains(&facts.workflow.id);
        if only.is_some_and(|a| a != &facts.agent) || !check_due(facts, gone) {
            continue;
        }
        let Ok(Ok(item)) = ports.bytes16().map(|b| ItemId::parse(&hex(b))) else {
            continue;
        };
        let last = facts.events.last().map_or(0, |e| e.seq.get());
        let event = Event {
            workflow: facts.workflow.id.clone(),
            agent: facts.agent.clone(),
            seq: NonZeroU32::MIN.saturating_add(last),
            day,
            at,
            what: What::ItemRaised {
                item: item.clone(),
                raised: Raised::Check(None),
            },
        };
        if ports.log(std::slice::from_ref(&event)).is_ok() {
            let facts = &mut board.members[i].facts;
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
