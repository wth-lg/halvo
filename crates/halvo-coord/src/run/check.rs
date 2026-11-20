use crate::ports::{Clock, Owner, Rng, Stamp};
use crate::schedule::{holds, owed_of_kind, owed_ready, read_back, seq_refusal, Board, Hold};
use crate::types::{
    hex, AgentId, Category, Cleared, ConfigView, Doing, Event, Facts, ItemId, ItemKind, Lapsed,
    Limits, OtherLapse, OwnItem, OwnItemKind, Raised, Replies, What,
};
use std::num::NonZeroU32;

/// Whether the coordinator raises the item `check` for the agent of `facts` in a move that has
/// read the agent store's record (I5.4, rulings XB-18, RD-5 and RD-96): the agent is idle with no
/// open item or with an open `ready` item, which the raise lapses together with each `ready` item
/// whose lapse the board owes (`owed_ready`), its workflow is not gone, and the
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

/// The read of the reply rows of an agent of the category `email` (rulings RD-67 3.3, 3.4, RD-81
/// 8.4), which each move that computes the agent's holds makes once its facts hold the owner's
/// answer of that move: the move that raises the item `check` and each tick while it is open
/// among them. It asks `Owner::replies` for the newest run where the agent store has ended that
/// run and no `hold_cleared` of the cause `check` names it, and for no other agent or category;
/// the facts take the rows, or `Replies::NoAnswer` for a read that did not answer. While such a
/// read leaves the hold `check` not standing, the run counts as not ended by the store in that move,
/// so that no start passes and the row says that the agent waits for its last run to be closed
/// (I5.5 step 3); a read that did not answer ends no hold that stands.
pub(crate) fn read_replies<P, C>(ports: &P, facts: &mut Facts<C>)
where
    P: Owner + ?Sized,
    C: ConfigView,
{
    facts.replies = Replies::NotAsked;
    if facts.workflow.category != Category::Email || !facts.store_ended {
        return;
    }
    let Some(newest) = facts.runs.first().map(|r| r.run.clone()) else {
        return;
    };
    let answered = facts.events.iter().any(|e| {
        matches!(&e.what, What::HoldCleared { cause: Cleared::Check { run, .. } } if *run == newest)
    });
    if answered {
        return;
    }
    facts.replies = match ports.replies(&facts.workflow.id, &facts.agent, &newest) {
        Ok(rows) => Replies::Rows(rows),
        Err(_) => Replies::NoAnswer,
    };
    if facts.replies == Replies::NoAnswer && !holds(facts).contains(&Hold::Check) {
        facts.store_ended = false;
    }
}

/// Raises the item `check` for each agent of the board, or for `only`, for which `check_due`
/// holds: after the agent's last event it stores, in one body, `item_lapsed` with the cause `check`
/// of the agent's open `ready` item where it holds one and of each `ready` item whose lapse the
/// board owes and the events do not close (`owed_ready`), then the owed `item_lapsed` of each
/// `check` item of an earlier life that the events do not close (`owed_of_kind`, ruling RD-96
/// item 5 (b)), and then `item_raised` of the kind
/// `check` with no run; the agent holds the item `check` once the owner acknowledged the body,
/// the lapse stores no `slot_skipped`, and the debt stays on the board until a tick finds the item
/// closed (I5.4, I5.8, rulings RD-5 and RD-96). When the owner refuses the body
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
    let mut raised = Vec::new();
    for i in 0..board.members.len() {
        let agent = board.members[i].facts.agent.clone();
        let gone = board.gone.contains(&board.members[i].facts.workflow.id);
        if only.is_some_and(|a| a != &agent)
            || board.aside(&agent)
            || board.dismissed_unread(&agent)
            || !check_due(&board.members[i].facts, gone)
        {
            continue;
        }
        let Ok(Ok(item)) = ports.bytes16().map(|b| ItemId::parse(&hex(b))) else {
            continue;
        };
        let events = check_raise_body(
            &board.members[i].facts,
            owed_ready(board, i),
            owed_of_kind(board, i, ItemKind::Check),
            &item,
            stamp,
        );
        let logged = match ports.log(&events) {
            Ok(()) => Retried::Logged(events),
            Err(e) if seq_refusal(&e) => {
                retry_after_read_back(ports, limits, stamp, board, i, gone, &item)
            }
            Err(_) => Retried::Nothing,
        };
        match logged {
            Retried::Logged(events) => {
                let facts = &mut board.members[i].facts;
                facts.events.extend(events);
                facts.doing = Doing::Idle(Some(OwnItem {
                    item,
                    kind: OwnItemKind::Check,
                }));
                raised.push(agent);
            }
            Retried::HeldAlready => raised.push(agent),
            Retried::Nothing => {}
        }
    }
    raised
}

enum Retried {
    Logged(Vec<Event>),
    HeldAlready,
    Nothing,
}

fn check_raise_body<C>(
    facts: &Facts<C>,
    owed: Vec<ItemId>,
    owed_checks: Vec<What>,
    item: &ItemId,
    stamp: Stamp,
) -> Vec<Event> {
    let last = facts.events.last().map_or(0, |e| e.seq.get());
    let open = match &facts.doing {
        Doing::Idle(Some(own)) if own.kind == OwnItemKind::Ready => Some(own.item.clone()),
        _ => None,
    };
    let lapses = open.into_iter().chain(owed).map(|item| What::ItemLapsed {
        item,
        lapsed: Lapsed::Ready(OtherLapse::Check),
    });
    let raise = What::ItemRaised {
        item: item.clone(),
        raised: Raised::Check(None),
    };
    lapses
        .chain(owed_checks)
        .chain(std::iter::once(raise))
        .zip(0u32..)
        .map(|(what, k)| Event {
            workflow: facts.workflow.id.clone(),
            agent: facts.agent.clone(),
            seq: NonZeroU32::MIN.saturating_add(last.saturating_add(k)),
            day: stamp.day(),
            at: stamp.at(),
            what,
        })
        .collect()
}

fn retry_after_read_back<P, C>(
    ports: &P,
    limits: &Limits,
    stamp: Stamp,
    board: &mut Board<C>,
    i: usize,
    gone: bool,
    item: &ItemId,
) -> Retried
where
    P: Owner + Rng + Clock,
    C: ConfigView,
{
    if read_back(ports, limits, stamp, board, i).is_err() {
        return Retried::Nothing;
    }
    let doing = &board.members[i].facts.doing;
    if matches!(doing, Doing::Idle(Some(own)) if own.kind == OwnItemKind::Check) {
        return Retried::HeldAlready;
    }
    if !check_due(&board.members[i].facts, gone) {
        return Retried::Nothing;
    }
    let events = check_raise_body(
        &board.members[i].facts,
        owed_ready(board, i),
        owed_of_kind(board, i, ItemKind::Check),
        item,
        stamp,
    );
    match ports.log(&events) {
        Ok(()) => Retried::Logged(events),
        Err(_) => Retried::Nothing,
    }
}
