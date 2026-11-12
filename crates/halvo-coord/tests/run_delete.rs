#[path = "run_support.rs"]
mod support;

#[path = "items_support.rs"]
mod items_support;

use halvo_coord::items::drive::{answer, flush, issue, on_screen, tick as items_tick, Ledger};
use halvo_coord::items::ConfirmAnswer;
use halvo_coord::mem::{Call, Method};
use halvo_coord::ports::PortError;
use halvo_coord::run::{delete_step, forgotten, Act};
use halvo_coord::schedule::Answer;
use halvo_coord::types::{
    Class, ConfirmAsk, Doing, ItemId, Limits, Live, Presenter, RunEnd, RunId, RunItem, RunItemKind,
    What, Workflow,
};
use support::{agent, flow, world, world_with, World, A, DAY, MANUAL, T0};

const GRACE_MS: u64 = Limits::PRODUCT.cancel_grace_s * 1000;

fn started(w: &mut World, n: usize) -> RunId {
    let done = w.act(Act::Start { agent: agent(n) });
    assert_eq!(done.report.answer, Ok(Answer::Ok), "{done:?}");
    done.report.run.unwrap()
}

fn delete(w: &World, n: usize) {
    w.ports
        .workflows
        .borrow_mut()
        .retain(|wf| wf.id() != &flow(n));
}

fn lapse_sends(w: &World, item: &ItemId) -> Vec<Option<PortError>> {
    w.ports
        .tried
        .borrow()
        .iter()
        .filter(|t| match &t.call {
            Call::Log(events) => events
                .iter()
                .any(|e| matches!(&e.what, What::ItemLapsed { item: i, .. } if i == item)),
            _ => false,
        })
        .map(|t| t.refused.clone())
        .collect()
}

#[test]
fn refusal_deleted_workflow() {
    let mut w = world(&[MANUAL]);
    let run = started(&mut w, 0);
    w.act(Act::Hello { run: run.clone() });
    let last = w.ports.events.borrow().len() as u32;
    let mut ledger = Ledger::new(flow(0), agent(0), last);
    let ask = ConfirmAsk::new(
        run.clone(),
        agent(0),
        vec![items_support::asked(1, Class::Send, 1, true)],
    )
    .unwrap();
    let mut held = issue(&w.ports, &mut ledger, &w.table, &w.limits, &ask).unwrap();
    let item = held.item().clone();
    w.act(Act::Wait {
        run: run.clone(),
        item: RunItem {
            item: item.clone(),
            kind: RunItemKind::Confirm,
        },
    });
    on_screen(&mut held, T0);
    w.set_clock(T0 + w.limits.arm_ms, DAY);
    items_tick(&w.ports, &mut ledger, &mut w.table, &mut held, &w.limits).unwrap();
    w.ports.refuse(Method::Log, PortError::NoService);
    let sent = answer(
        &w.ports,
        &mut ledger,
        &mut w.table,
        &mut held,
        ConfirmAnswer::Allow,
        Presenter::Page,
        &w.limits,
    );
    assert!(sent.is_err());
    assert_eq!(
        ledger.waiting().len(),
        1,
        "the item_lapsed with no_record waits"
    );
    w.ports.clear_refusals();
    delete(&w, 0);
    assert!(flush(&w.ports, &mut ledger, &w.limits).is_err());
    let tick = w.act(Act::Tick);
    assert_eq!(
        tick.report.stopped,
        vec![run.clone()],
        "stopped at the next tick"
    );
    assert_eq!(tick.gone, vec![flow(0)]);
    assert!(w.table.is_stopped(&run));
    assert_eq!(w.facts(0).doing, Doing::Live(Live::Stopping));
    ledger = forgotten(flow(0), agent(0), &ledger);
    let _ = flush(&w.ports, &mut ledger, &w.limits);
    w.set_clock(T0 + w.limits.arm_ms + GRACE_MS, DAY);
    let kill = w.act(Act::Tick);
    assert_eq!(kill.report.ended[0].cause, RunEnd::Deleted);
    let _ = flush(&w.ports, &mut ledger, &w.limits);
    let refused = PortError::Refused(halvo_coord::types::Name::parse("no_workflow").unwrap());
    assert_eq!(
        lapse_sends(&w, &item),
        vec![Some(PortError::NoService), Some(refused)],
        "refused with no_workflow once and not sent again"
    );
    let events = w.ports.events.borrow();
    assert!(!events
        .iter()
        .any(|e| matches!(&e.what, What::RunEnded { run: r, .. } if *r == run)));
    assert!(!events
        .iter()
        .any(|e| matches!(&e.what, What::ItemLapsed { .. })));
}

fn attempts(w: &World, what: &str) -> Vec<Option<PortError>> {
    w.ports
        .tried
        .borrow()
        .iter()
        .filter(|t| match (&t.call, what) {
            (Call::Send(halvo_coord::types::Body::EndRun { .. }), "end_run") => true,
            (Call::Log(events), "run_ended") => events
                .iter()
                .any(|e| matches!(e.what, What::RunEnded { .. })),
            _ => false,
        })
        .map(|t| t.refused.clone())
        .collect()
}

#[test]
fn an_owed_end_run_of_a_gone_workflow_is_sent_once_more_and_ends_at_no_workflow() {
    let mut w = world(&[MANUAL]);
    w.ports.refuse(Method::StartRun, PortError::NoService);
    w.ports.refuse(Method::EndRun, PortError::Busy);
    w.act(Act::Start { agent: agent(0) });
    assert_eq!(w.board.owed.len(), 1);
    delete(&w, 0);
    let gone = PortError::Refused(halvo_coord::types::Name::parse("no_workflow").unwrap());
    w.ports.clear_refusals();
    w.ports.refuse(Method::EndRun, gone.clone());
    w.set_clock(T0 + 1, DAY);
    w.act(Act::Tick);
    w.set_clock(T0 + 2, DAY);
    w.act(Act::Tick);
    assert_eq!(
        attempts(&w, "end_run"),
        vec![Some(PortError::Busy), Some(gone)],
        "sent once more, and the refusal no_workflow ends the debt"
    );
    assert!(w.board.owed.is_empty());
}

#[test]
fn an_owed_run_ended_of_a_gone_workflow_is_logged_once_more_and_ends_at_no_workflow() {
    let mut w = world(&[MANUAL]);
    let run = started(&mut w, 0);
    w.ports.refuse(Method::Log, PortError::NoService);
    w.act(Act::End {
        run,
        outcome: Some(halvo_coord::types::Outcome::Completed),
    });
    w.ports.clear_refusals();
    delete(&w, 0);
    w.set_clock(T0 + 1, DAY);
    w.act(Act::Tick);
    w.set_clock(T0 + 2, DAY);
    w.act(Act::Tick);
    let gone = PortError::Refused(halvo_coord::types::Name::parse("no_workflow").unwrap());
    assert_eq!(
        attempts(&w, "run_ended"),
        vec![Some(PortError::NoService), Some(gone)]
    );
    assert!(w.board.owed.is_empty());
}

#[test]
fn delete_step_stops_the_runs_of_a_workflow_that_is_gone_and_drops_its_queued_start() {
    let limits = Limits {
        max_live_runs: 1,
        ..Limits::PRODUCT
    };
    let mut w = world_with(&[MANUAL, MANUAL, MANUAL], limits);
    let live = started(&mut w, 0);
    assert_eq!(
        w.act(Act::Start { agent: agent(1) }).report.answer,
        Ok(Answer::Queued)
    );
    let listed: Vec<_> = w
        .ports
        .workflows
        .borrow()
        .iter()
        .map(|wf: &Workflow| wf.id().clone())
        .filter(|id| *id != flow(0) && *id != flow(1))
        .collect();
    let gone = delete_step(&w.board, &listed);
    assert_eq!(gone.stop, vec![live.clone()]);
    assert_eq!(gone.dropped, vec![agent(1)]);
    assert_eq!(gone.seen, vec![flow(0), flow(1)]);
    assert!(gone.recause.is_empty());
    w.act(Act::Stop {
        run: live.clone(),
        by: halvo_coord::run::StopBy::Person,
    });
    let again = delete_step(&w.board, &listed);
    assert!(again.stop.is_empty());
    assert_eq!(again.recause, vec![live], "a stopping run takes deleted");
    let _ = A {
        person: false,
        slotted: false,
    };
}
