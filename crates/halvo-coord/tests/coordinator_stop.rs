#[path = "items_support.rs"]
mod items_support;
#[path = "run_support.rs"]
mod support;

use halvo_coord::items::drive::{issue, lapse_item, stop_confirm, Ledger};
use halvo_coord::items::ConfirmState;
use halvo_coord::run::{Act, StopBy};
use halvo_coord::schedule::Answer;
use halvo_coord::types::{
    Cause, Class, ConfirmAsk, Doing, ItemId, LapseCause, Lapsed, Live, OtherLapse, RunId, RunItem,
    RunItemKind, What,
};
use support::{agent, flow, world, World, MANUAL};

fn waiting(w: &mut World, kind: RunItemKind, item: &ItemId) -> RunId {
    let run = w.act(Act::Start { agent: agent(0) }).report.run.unwrap();
    assert_eq!(
        w.act(Act::Hello { run: run.clone() }).report.answer,
        Ok(Answer::Ok)
    );
    let wait = w.act(Act::Wait {
        run: run.clone(),
        item: RunItem {
            item: item.clone(),
            kind,
        },
    });
    assert_eq!(wait.report.answer, Ok(Answer::Ok));
    w.forget();
    run
}

fn ledger(w: &World) -> Ledger {
    let last = w.facts(0).events.last().map_or(0, |e| e.seq.get());
    Ledger::new(flow(0), agent(0), last)
}

fn item(n: u32) -> ItemId {
    ItemId::parse(&format!("{:032x}", 0x5100 + n)).unwrap()
}

fn cancels(calls: &[String]) -> usize {
    calls.iter().filter(|c| c.starts_with("cancel")).count()
}

#[test]
fn a_stop_of_a_waiting_run_whose_item_has_not_lapsed_is_state_and_calls_nothing() {
    for (n, kind) in RunItemKind::ALL.iter().enumerate() {
        let mut w = world(&[MANUAL]);
        let run = waiting(&mut w, *kind, &item(n as u32));
        for by in [StopBy::Person, StopBy::WaitLimit] {
            let stop = w.act(Act::Stop {
                run: run.clone(),
                by,
            });
            assert_eq!(stop.report.answer, Err(Cause::State), "{kind:?}");
        }
        assert!(w.calls().is_empty());
        assert!(matches!(w.facts(0).doing, Doing::Live(Live::Waiting(_))));
        assert!(!w.table.is_stopped(&run));
    }
}

#[test]
fn lapse_item_with_cancelled_from_the_test_stops_the_run_of_an_item_of_another_kind_in_the_table() {
    let kinds = [
        (RunItemKind::SignIn, Lapsed::SignIn(OtherLapse::Cancelled)),
        (
            RunItemKind::LinkAccount,
            Lapsed::LinkAccount(OtherLapse::Cancelled),
        ),
        (RunItemKind::Stuck, Lapsed::Stuck(OtherLapse::Cancelled)),
        (RunItemKind::Check, Lapsed::Check(OtherLapse::Cancelled)),
        (RunItemKind::Show, Lapsed::Show(OtherLapse::Cancelled)),
    ];
    for (n, (kind, lapsed)) in kinds.into_iter().enumerate() {
        let mut w = world(&[MANUAL]);
        let it = item(n as u32);
        let run = waiting(&mut w, kind, &it);
        let mut ledger = ledger(&w);
        lapse_item(
            &w.ports,
            &mut ledger,
            &mut w.table,
            &run,
            &RunItem {
                item: it.clone(),
                kind,
            },
            OtherLapse::Cancelled,
            &w.limits,
        )
        .unwrap();
        assert!(w.ports.events.borrow().iter().any(|e| e.what
            == What::ItemLapsed {
                item: it.clone(),
                lapsed
            }));
        assert!(w.table.is_stopped(&run));
        let stop = w.act(Act::Stop {
            run: run.clone(),
            by: StopBy::Person,
        });
        assert_eq!(stop.report.answer, Ok(Answer::Ok), "{kind:?}");
        assert_eq!(stop.report.stopped, std::slice::from_ref(&run));
        assert_eq!(w.facts(0).doing, Doing::Live(Live::Stopping));
        assert_eq!(cancels(&w.calls()), 1, "one cancel, the lapse's");
    }
}

#[test]
fn the_lapse_of_an_item_with_run_ended_or_app_ended_stops_no_run() {
    for cause in [OtherLapse::RunEnded, OtherLapse::AppEnded] {
        let mut w = world(&[MANUAL]);
        let it = item(9);
        let run = waiting(&mut w, RunItemKind::Stuck, &it);
        let mut ledger = ledger(&w);
        let table = &mut w.table;
        lapse_item(
            &w.ports,
            &mut ledger,
            table,
            &run,
            &RunItem {
                item: it.clone(),
                kind: RunItemKind::Stuck,
            },
            cause,
            &w.limits,
        )
        .unwrap();
        assert!(!w.table.is_stopped(&run));
        assert_eq!(cancels(&w.calls()), 0);
    }
}

#[test]
fn lapse_item_refuses_an_item_of_the_kind_confirm_with_malformed_and_calls_nothing() {
    let mut w = world(&[MANUAL]);
    let it = item(10);
    let run = waiting(&mut w, RunItemKind::Confirm, &it);
    let mut ledger = ledger(&w);
    let refused = lapse_item(
        &w.ports,
        &mut ledger,
        &mut w.table,
        &run,
        &RunItem {
            item: it.clone(),
            kind: RunItemKind::Confirm,
        },
        OtherLapse::Cancelled,
        &w.limits,
    );
    assert_eq!(refused, Err(Cause::Malformed));
    assert!(w.calls().is_empty());
}

#[test]
fn stop_confirm_of_a_held_confirmation_lapses_it_with_cancelled_and_stops_its_run_in_the_table() {
    let mut w = world(&[MANUAL]);
    let run = w.act(Act::Start { agent: agent(0) }).report.run.unwrap();
    w.act(Act::Hello { run: run.clone() });
    let ask = ConfirmAsk::new(
        run.clone(),
        agent(0),
        vec![items_support::asked(1, Class::Send, 1, true)],
    )
    .unwrap();
    let mut ledger = ledger(&w);
    let mut held = issue(&w.ports, &mut ledger, &w.table, &w.limits, &ask).unwrap();
    w.act(Act::Wait {
        run: run.clone(),
        item: RunItem {
            item: held.item().clone(),
            kind: RunItemKind::Confirm,
        },
    });
    w.forget();
    stop_confirm(&w.ports, &mut ledger, &mut w.table, &mut held, &w.limits).unwrap();
    assert_eq!(
        *held.confirmation().state(),
        ConfirmState::Lapsed(LapseCause::Cancelled)
    );
    assert!(w.table.is_stopped(&run));
    let stop = w.act(Act::Stop {
        run: run.clone(),
        by: StopBy::Person,
    });
    assert_eq!(stop.report.answer, Ok(Answer::Ok));
    assert_eq!(cancels(&w.calls()), 1);
}

#[test]
fn a_stop_with_kind_or_budget_ends_with_its_cause_in_run_step_and_in_the_stored_run_ended() {
    use halvo_coord::run::{run_step, Ending, Next, Phase, RunMove};
    use halvo_coord::types::{Outcome, RunEnd};
    for (by, end) in [
        (StopBy::Kind, RunEnd::Kind),
        (StopBy::Budget, RunEnd::Budget),
    ] {
        let mut w = world(&[MANUAL]);
        let run = w.act(Act::Start { agent: agent(0) }).report.run.unwrap();
        w.act(Act::Hello { run: run.clone() });
        let stop = w.act(Act::Stop {
            run: run.clone(),
            by,
        });
        assert_eq!(stop.report.answer, Ok(Answer::Ok));
        let live = w.board.runs.iter().find(|r| r.run == run).unwrap();
        let phase = Phase::of(&Live::Stopping, live.stop);
        assert_eq!(
            run_step(
                Some(&phase),
                RunMove::End(Ending::Frame(Some(Outcome::Completed)))
            ),
            Ok(Next::Ended(end))
        );
        w.ports.end_runner(&run);
        let ended = w.act(Act::End {
            run: run.clone(),
            outcome: Some(Outcome::Completed),
        });
        assert_eq!(ended.report.ended[0].cause, end);
        assert!(w.ports.events.borrow().iter().any(|e| e.what
            == What::RunEnded {
                run: run.clone(),
                cause: end
            }));
    }
}
