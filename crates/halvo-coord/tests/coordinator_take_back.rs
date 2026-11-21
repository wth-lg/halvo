#[path = "coordinator_support.rs"]
mod support;

use halvo_coord::types::{Action, Choice, ItemKind, Limits, Outcome, RunEvent, RunId, Snapshot};
use halvo_coord::{words, Coordinator};
use support::{agent, coordinator, started, Both, MANUAL};

fn one_at_a_time() -> Limits {
    Limits {
        max_live_runs: 1,
        ..Limits::PRODUCT
    }
}

fn line(s: &Snapshot, n: usize) -> String {
    s.rows
        .iter()
        .find(|r| r.agent == agent(n))
        .map(|r| r.line.clone())
        .unwrap()
}

fn start(c: &Coordinator<Both>, n: usize) -> RunId {
    assert_eq!(c.control(&agent(n), Action::RunNow), Ok(()));
    let run = started(c).last().cloned().unwrap();
    assert_eq!(c.on_run(&run, RunEvent::Hello), Ok(()));
    run
}

fn ready(c: &Coordinator<Both>) -> halvo_coord::types::ItemId {
    let s = c.snapshot();
    assert_eq!(s.items.len(), 1, "{s:?}");
    assert_eq!(s.items[0].kind(), ItemKind::Ready);
    s.items[0].item().clone()
}

fn asks_again() -> Option<String> {
    words::text("line.needs_you.asks_again", &[]).ok()
}

/// Behind a take-back of an agent that asks first, its card comes back at the next tick; the
/// row's line then says that the person cancelled the start and that the agent asks again, so
/// that the card that comes back reads as no failed press (the gates of U2.4e, the inbox line
/// 845; ruling RD-152 (4)). Once that card is answered the line is the plain one again.
#[test]
fn u34_after_a_take_back_the_card_that_comes_back_says_that_the_start_was_cancelled() {
    let asks = support::A {
        person: true,
        slotted: true,
    };
    let c = coordinator(&[MANUAL, asks], one_at_a_time());
    let first = start(&c, 0);
    c.tick();
    let asked = ready(&c);
    assert_eq!(
        line(&c.snapshot(), 1),
        words::text("line.needs_you.ready", &[]).unwrap(),
        "the first ask reads as the plain line"
    );
    assert_eq!(c.answer(&asked, Choice::Start, None, 0), Ok(()));
    assert_eq!(c.control(&agent(1), Action::TakeBack), Ok(()));
    c.ports().ports.end_runner(&first);
    c.on_run(&first, RunEvent::Ended(Some(Outcome::Completed)))
        .unwrap();
    c.tick();
    let again = ready(&c);
    assert_ne!(again, asked);
    assert_eq!(
        Some(line(&c.snapshot(), 1)),
        asks_again(),
        "the card that came back says why"
    );
    assert_eq!(c.answer(&again, Choice::Skip, None, 0), Ok(()));
    c.tick();
    let s = c.snapshot();
    if let Some(item) = s.items.first() {
        assert_ne!(item.item(), &again);
        assert_ne!(
            Some(line(&s, 1)),
            asks_again(),
            "a later ask is plain again"
        );
    }
}

/// A take-back of an agent that does not ask first leaves no such line: no card comes back.
#[test]
fn u34_a_take_back_of_an_agent_that_does_not_ask_first_leaves_the_plain_line() {
    let c = coordinator(&[MANUAL, MANUAL], one_at_a_time());
    let first = start(&c, 0);
    assert_eq!(c.control(&agent(1), Action::RunNow), Ok(()));
    assert_eq!(c.control(&agent(1), Action::TakeBack), Ok(()));
    c.ports().ports.end_runner(&first);
    c.on_run(&first, RunEvent::Ended(Some(Outcome::Completed)))
        .unwrap();
    c.tick();
    let s = c.snapshot();
    assert_ne!(Some(line(&s, 1)), asks_again());
    assert!(s.items.is_empty(), "{s:?}");
}
