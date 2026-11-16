#[path = "coordinator_support.rs"]
mod support;

use halvo_coord::guard::EffectState;
use halvo_coord::mem::Call;
use halvo_coord::types::{
    Action, AskedEffect, BindAsk, BindResult, BoundField, BoundFrom, Choice, Class, ConfirmAsk,
    Digest, EffectId, HandbackAsk, HandbackReason, HandbackResult, ItemId, ItemKind, Kept, Limits,
    Name, Origin, Outcome, PastEffect, PastRun, RunEnd, RunEvent, RunHistory, RunId, What,
};
use halvo_coord::{words, Coordinator};
use support::{agent, calls_since, coordinator, mark, started, Both, DAY, MANUAL, T0};

fn start(c: &Coordinator<Both>, n: usize) -> RunId {
    assert_eq!(c.control(&agent(n), Action::RunNow), Ok(()));
    let run = started(c).last().cloned().unwrap();
    assert_eq!(c.on_run(&run, RunEvent::Hello), Ok(()));
    run
}

fn raised(c: &Coordinator<Both>) -> usize {
    c.ports()
        .ports
        .events
        .borrow()
        .iter()
        .filter(|e| matches!(e.what, What::ItemRaised { .. }))
        .count()
}

fn bind(run: &RunId, origin: &str) -> RunEvent {
    RunEvent::Bind(BindAsk {
        run: run.clone(),
        agent: agent(0),
        origin: Origin::parse(origin).unwrap(),
    })
}

fn link(run: &RunId, origin: &str) -> RunEvent {
    RunEvent::Handback(HandbackAsk {
        run: run.clone(),
        agent: agent(0),
        reason: HandbackReason::AccountLink,
        origin: Some(Origin::parse(origin).unwrap()),
    })
}

const DECLINED: [&str; 7] = [
    "http://pay.site1.example:80",
    "http://127.example.com:80",
    "http://127.0.0.1.example.com:80",
    "http://localhost:8080",
    "http://10.0.0.1:80",
    "http://127.a.b.c:80",
    "http://128.0.0.1:80",
];

#[test]
fn a_second_step_for_an_http_site_outside_127_is_declined_and_raises_no_item() {
    for origin in DECLINED {
        let c = coordinator(&[MANUAL], Limits::PRODUCT);
        let run = start(&c, 0);
        let (m, before) = (mark(&c), raised(&c));
        assert_eq!(c.on_run(&run, bind(&run, origin)), Ok(()), "{origin}");
        assert_eq!(
            calls_since(&c, m)
                .into_iter()
                .filter(|call| matches!(call, Call::AnswerBind(..)))
                .collect::<Vec<_>>(),
            [Call::AnswerBind(run.clone(), BindResult::Decline)],
            "{origin}"
        );
        assert_eq!(raised(&c), before, "no item_raised for {origin}");
        assert!(c.snapshot().items.is_empty(), "{origin}");
        let row = c.snapshot().rows[0].line.clone();
        assert_eq!(row, words::text("line.working.working", &[]).unwrap());
    }
}

#[test]
fn a_second_step_for_127_0_0_1_or_an_https_site_raises_its_item() {
    for origin in [
        "http://127.0.0.1:8080",
        "http://127.255.0.9:80",
        "https://pay.site1.example:443",
    ] {
        let c = coordinator(&[MANUAL], Limits::PRODUCT);
        let run = start(&c, 0);
        let m = mark(&c);
        assert_eq!(c.on_run(&run, bind(&run, origin)), Ok(()), "{origin}");
        assert!(!calls_since(&c, m)
            .iter()
            .any(|call| matches!(call, Call::AnswerBind(..))));
        let s = c.snapshot();
        assert_eq!(s.items.len(), 1, "{origin}: {s:?}");
        assert_eq!(s.items[0].kind(), ItemKind::SignIn);
    }
}

#[test]
fn an_account_link_handback_for_an_http_site_outside_127_is_declined_and_raises_no_item() {
    for origin in DECLINED {
        let c = coordinator(&[MANUAL], Limits::PRODUCT);
        let run = start(&c, 0);
        let (m, before) = (mark(&c), raised(&c));
        assert_eq!(c.on_run(&run, link(&run, origin)), Ok(()), "{origin}");
        assert_eq!(
            calls_since(&c, m)
                .into_iter()
                .filter(|call| matches!(call, Call::AnswerHandback(..)))
                .collect::<Vec<_>>(),
            [Call::AnswerHandback(run.clone(), HandbackResult::Declined)],
            "{origin}"
        );
        assert_eq!(raised(&c), before, "no item_raised for {origin}");
        assert!(c.snapshot().items.is_empty(), "{origin}");
    }
}

fn ask(run: &RunId, effects: &[(u32, &str)]) -> RunEvent {
    ask_of(run, 0, effects)
}

fn ask_of(run: &RunId, n: usize, effects: &[(u32, &str)]) -> RunEvent {
    RunEvent::Confirm(
        ConfirmAsk::new(
            run.clone(),
            agent(n),
            effects
                .iter()
                .map(|(n, d)| AskedEffect {
                    effect: EffectId::parse(&format!("{n:032x}")).unwrap(),
                    class: Class::Send,
                    digest: Digest::parse(&d.repeat(32)).unwrap(),
                    digest_ok: true,
                    fields: ["to", "subject", "body"]
                        .iter()
                        .map(|name| BoundField {
                            name: Name::parse(name).unwrap(),
                            from: BoundFrom::Extract,
                            value: vec!["Ann Lee".into()],
                        })
                        .collect(),
                    new_origin: None,
                })
                .collect(),
        )
        .unwrap(),
    )
}

/// Raises a page confirmation of `effects` for `run`, shows it and arms it at `T0 + 1_000`.
fn armed(c: &Coordinator<Both>, run: &RunId, effects: &[(u32, &str)]) -> ItemId {
    assert_eq!(c.on_run(run, ask(run, effects)), Ok(()));
    let item = c.snapshot().items[0].item().clone();
    c.on_screen(&item);
    c.ports().ports.set_clock(T0 + 1_000, DAY);
    c.tick();
    item
}

fn press(c: &Coordinator<Both>, item: &ItemId, choice: Choice) {
    let step = c
        .snapshot()
        .items
        .iter()
        .find(|i| i.item() == item)
        .map_or(0, |i| i.step());
    assert_eq!(c.answer(item, choice, None, step), Ok(()));
}

fn end(c: &Coordinator<Both>, run: &RunId, outcome: Outcome) {
    c.ports().ports.end_runner(run);
    let _ = c.on_run(run, RunEvent::Ended(Some(outcome)));
    c.tick();
}

fn lapse_at_wait_limit(c: &Coordinator<Both>) {
    let limits = Limits::PRODUCT;
    c.ports()
        .ports
        .set_clock(T0 + 1_000 + limits.wait_limit_s * 1000, DAY);
    c.tick();
}

fn send(n: u32, d: &str) -> PastEffect {
    PastEffect {
        effect: EffectId::parse(&format!("{n:032x}")).unwrap(),
        digest: Digest::parse(&d.repeat(32)).unwrap(),
        class: Class::Send,
        state: EffectState::Rejected,
        cause: None,
        kept: vec![Kept {
            name: Name::parse("to").unwrap(),
            value: vec!["Ann Lee".into()],
        }],
    }
}

/// The card of agent 0 after the store gives `run` with `outcome` and `effects`, with the
/// run's line and the keys of its entries.
fn card_of(
    c: &Coordinator<Both>,
    run: &RunId,
    outcome: Outcome,
    effects: Vec<PastEffect>,
) -> (String, Vec<String>) {
    c.ports().ports.histories.borrow_mut().push((
        agent(0),
        RunHistory {
            run: PastRun {
                run: run.clone(),
                day: DAY,
                outcome,
                unknown: 0,
            },
            effects,
        },
    ));
    let card = c.card(&agent(0)).unwrap();
    let keys = card.runs[0].effects.iter().map(|e| e.key()).collect();
    (card.runs[0].text.clone(), keys)
}

fn line(key: &str) -> String {
    words::text(key, &[]).unwrap()
}

fn own_end(c: &Coordinator<Both>, run: &RunId) -> Option<RunEnd> {
    c.ports()
        .ports
        .events
        .borrow()
        .iter()
        .rev()
        .find_map(|e| match &e.what {
            What::RunEnded { run: r, cause } if r == run => Some(*cause),
            _ => None,
        })
}

#[test]
fn rd85_a_no_on_the_card_shows_the_refused_entry_and_the_line_that_you_said_no() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    let item = armed(&c, &run, &[(0xe1, "ab")]);
    press(&c, &item, Choice::Reject);
    end(&c, &run, Outcome::Declined);
    let (text, keys) = card_of(&c, &run, Outcome::Declined, vec![send(0xe1, "ab")]);
    assert_eq!(keys, ["run.effect.send.refused"]);
    assert_eq!(text, line("line.run.declined"));
}

#[test]
fn rd85_two_cards_a_no_on_the_first_and_a_lapse_of_the_second_at_the_coordinator() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    let first = armed(&c, &run, &[(0xe1, "ab")]);
    press(&c, &first, Choice::Reject);
    assert_eq!(c.on_run(&run, ask(&run, &[(0xe3, "ab")])), Ok(()));
    assert_eq!(c.snapshot().items.len(), 1, "the second card");
    lapse_at_wait_limit(&c);
    end(&c, &run, Outcome::Declined);
    assert_eq!(own_end(&c, &run), Some(RunEnd::Lapsed));
    let (text, keys) = card_of(
        &c,
        &run,
        Outcome::Declined,
        vec![send(0xe1, "ab"), send(0xe3, "ab")],
    );
    assert_eq!(
        keys,
        ["run.effect.send.refused", "run.effect.send.not_done"]
    );
    assert_eq!(text, line("line.run.lapsed"));
}

#[test]
fn rd85_a_stop_on_an_open_card_is_no_refusal_and_the_line_says_that_you_stopped_it() {
    for store in [Outcome::Declined, Outcome::Completed, Outcome::Failed] {
        let c = coordinator(&[MANUAL], Limits::PRODUCT);
        let run = start(&c, 0);
        armed(&c, &run, &[(0xe1, "ab")]);
        assert_eq!(c.control(&agent(0), Action::Stop), Ok(()));
        end(&c, &run, store);
        assert_eq!(own_end(&c, &run), Some(RunEnd::Cancelled), "{store:?}");
        let (text, keys) = card_of(&c, &run, store, vec![send(0xe1, "ab")]);
        assert_eq!(keys, ["run.effect.send.not_done"], "{store:?}");
        assert_eq!(text, line("line.run.cancelled"), "{store:?}");
    }
}

#[test]
fn rd85_a_stop_after_an_allow_is_no_refusal() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    let item = armed(&c, &run, &[(0xe1, "ab")]);
    press(&c, &item, Choice::Allow);
    assert_eq!(c.control(&agent(0), Action::Stop), Ok(()));
    end(&c, &run, Outcome::Declined);
    let (text, keys) = card_of(&c, &run, Outcome::Declined, vec![send(0xe1, "ab")]);
    assert_eq!(keys, ["run.effect.send.not_done"]);
    assert_eq!(text, line("line.run.cancelled"));
}

#[test]
fn rd48c_a_run_that_lapsed_at_wait_limit_shows_the_lapsed_line_whatever_the_store_says() {
    for store in [Outcome::Declined, Outcome::Completed, Outcome::Failed] {
        let c = coordinator(&[MANUAL], Limits::PRODUCT);
        let run = start(&c, 0);
        armed(&c, &run, &[(0xe1, "ab")]);
        lapse_at_wait_limit(&c);
        end(&c, &run, store);
        assert_eq!(own_end(&c, &run), Some(RunEnd::Lapsed), "{store:?}");
        let (text, keys) = card_of(&c, &run, store, vec![send(0xe1, "ab")]);
        assert_eq!(keys, ["run.effect.send.not_done"], "{store:?}");
        assert_eq!(text, line("line.run.lapsed"), "{store:?}");
    }
}

#[test]
fn rd85_a_run_that_failed_after_a_no_keeps_its_refused_entry_and_its_failed_line() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    let item = armed(&c, &run, &[(0xe1, "ab")]);
    press(&c, &item, Choice::Reject);
    end(&c, &run, Outcome::Failed);
    let (text, keys) = card_of(&c, &run, Outcome::Failed, vec![send(0xe1, "ab")]);
    assert_eq!(keys, ["run.effect.send.refused"]);
    assert_eq!(text, line("line.run.failed"));
}

#[test]
fn rd85_a_no_whose_release_never_came_is_a_refusal() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    let item = armed(&c, &run, &[(0xe1, "ab")]);
    c.ports().drop_decision.set(true);
    let step = c.snapshot().items[0].step();
    let _ = c.answer(&item, Choice::Reject, None, step);
    let released = c
        .ports()
        .ports
        .events
        .borrow()
        .iter()
        .any(|e| matches!(e.what, What::ItemReleased { .. }));
    assert!(!released, "no item_released row");
    let (_, keys) = card_of(&c, &run, Outcome::Declined, vec![send(0xe1, "ab")]);
    assert_eq!(keys, ["run.effect.send.refused"]);
}

#[test]
fn an_account_link_handback_for_127_0_0_1_or_an_https_site_raises_its_item() {
    for origin in ["http://127.0.0.1:8080", "https://mail.site1.example:443"] {
        let c = coordinator(&[MANUAL], Limits::PRODUCT);
        let run = start(&c, 0);
        let m = mark(&c);
        assert_eq!(c.on_run(&run, link(&run, origin)), Ok(()), "{origin}");
        assert!(!calls_since(&c, m)
            .iter()
            .any(|call| matches!(call, Call::AnswerHandback(..))));
        let s = c.snapshot();
        assert_eq!(s.items.len(), 1, "{origin}: {s:?}");
        assert_eq!(s.items[0].kind(), ItemKind::LinkAccount);
    }
}

fn name_of(s: &halvo_coord::types::Snapshot, agent: &halvo_coord::types::AgentId) -> String {
    s.rows
        .iter()
        .find(|r| &r.agent == agent)
        .map(|r| r.name.clone())
        .unwrap()
}

#[test]
fn i34_one_confirmation_names_the_agent_that_asks() {
    let c = coordinator(&[MANUAL, MANUAL], Limits::PRODUCT);
    let run = start(&c, 1);
    assert_eq!(c.on_run(&run, ask_of(&run, 1, &[(0xe1, "ab")])), Ok(()));
    let s = c.snapshot();
    assert_eq!(s.items.len(), 1);
    let name = name_of(&s, &agent(1));
    assert_eq!(
        s.items[0].text(),
        words::text("confirm.asked_by", &[("name", &name)]).unwrap()
    );
}

#[test]
fn i34_two_page_confirmations_name_their_agent_and_their_place_in_the_replys_order() {
    let limits = Limits {
        max_live_runs: 2,
        ..Limits::PRODUCT
    };
    let c = coordinator(&[MANUAL, MANUAL], limits);
    let first = start(&c, 0);
    let second = start(&c, 1);
    assert_eq!(
        c.on_run(&second, ask_of(&second, 1, &[(0xe2, "ab")])),
        Ok(())
    );
    assert_eq!(c.on_run(&first, ask_of(&first, 0, &[(0xe1, "ab")])), Ok(()));
    let s = c.snapshot();
    assert_eq!(s.items.len(), 2);
    let mut texts = Vec::new();
    for (k, item) in s.items.iter().enumerate() {
        assert_eq!(item.presenter(), Some(halvo_coord::types::Presenter::Page));
        let name = name_of(&s, item.agent());
        let (n, count) = ((k + 1).to_string(), "2".to_owned());
        assert_eq!(
            item.text(),
            words::text(
                "confirm.asked_by_place",
                &[("name", &name), ("n", &n), ("count", &count)]
            )
            .unwrap(),
            "item {k}"
        );
        texts.push(item.text().to_owned());
    }
    assert_ne!(texts[0], texts[1], "two confirmations do not read the same");
}

/// A coordinator whose one agent's run lives while a tick has seen its workflow gone.
fn gone_while_live() -> (Coordinator<Both>, RunId) {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    c.ports()
        .ports
        .workflows
        .borrow_mut()
        .retain(|w| w.id() != &support::flow(0));
    c.tick();
    assert_eq!(c.live(), 1, "the run is stopping");
    (c, run)
}

fn row_of(c: &Coordinator<Both>) -> Option<halvo_coord::types::Row> {
    c.snapshot().rows.into_iter().find(|r| r.agent == agent(0))
}

#[test]
fn rd54g_a_gone_workflows_live_run_keeps_its_row_stopping_with_no_control_until_it_ends() {
    let (c, _) = gone_while_live();
    let row = row_of(&c).expect("the row stays while the run lives");
    assert_eq!(row.line, line("line.working.stopping"));
    assert_eq!(row.status, halvo_coord::types::Status::Working);
    assert!(row.controls.is_empty(), "{:?}", row.controls);
    assert_eq!(
        c.control(&agent(0), Action::Stop),
        Err(halvo_coord::types::Cause::UnknownAgent)
    );
    assert_eq!(
        c.card(&agent(0)).map(|_| ()),
        Err(halvo_coord::types::Cause::UnknownAgent)
    );
    c.ports()
        .ports
        .set_clock(T0 + Limits::PRODUCT.cancel_grace_s * 1000, DAY);
    c.tick();
    assert_eq!(c.live(), 0, "the kill ended the run");
    assert!(row_of(&c).is_none(), "no row once the run has ended");
}

#[test]
fn rd54g_a_gone_workflows_run_whose_kill_does_not_end_it_shows_kill_stuck() {
    let (c, _) = gone_while_live();
    c.ports().ports.refuse(
        halvo_coord::mem::Method::Kill,
        halvo_coord::ports::PortError::Busy,
    );
    c.ports()
        .ports
        .set_clock(T0 + Limits::PRODUCT.cancel_grace_s * 1000, DAY);
    c.tick();
    assert_eq!(c.live(), 1, "the kill did not end the run");
    let row = row_of(&c).expect("the row stays");
    assert_eq!(row.line, line("line.working.kill_stuck"));
    assert!(row.controls.is_empty());
    c.ports().ports.clear_refusals();
    c.ports()
        .ports
        .set_clock(T0 + Limits::PRODUCT.cancel_grace_s * 1000 + 1_000, DAY);
    c.tick();
    assert_eq!(c.live(), 0);
    assert!(row_of(&c).is_none());
}

#[test]
fn rd54g_a_gone_workflows_row_names_no_next_day() {
    let slotted = support::A {
        person: false,
        slotted: true,
    };
    let c = coordinator(&[slotted], Limits::PRODUCT);
    if started(&c).is_empty() {
        start(&c, 0);
    } else {
        let run = started(&c).last().cloned().unwrap();
        assert_eq!(c.on_run(&run, RunEvent::Hello), Ok(()));
    }
    c.ports()
        .ports
        .workflows
        .borrow_mut()
        .retain(|w| w.id() != &support::flow(0));
    c.tick();
    assert_eq!(c.live(), 1, "the run is stopping");
    let row = row_of(&c).expect("the row stays while the run lives");
    assert_eq!(row.line, line("line.working.stopping"));
    assert_eq!(row.next, None, "a gone workflow has no next slot");
}

/// A run whose one item of `items_per_run` 1 was raised and answered.
fn items_spent() -> (Coordinator<Both>, RunId) {
    let c = coordinator(
        &[MANUAL],
        Limits {
            items_per_run: 1,
            ..Limits::PRODUCT
        },
    );
    let run = start(&c, 0);
    assert_eq!(
        c.on_run(&run, link(&run, "https://pay.site1.example:443")),
        Ok(())
    );
    let item = c.snapshot().items[0].item().clone();
    press(&c, &item, Choice::Done);
    assert!(c.snapshot().items.is_empty());
    (c, run)
}

#[test]
fn i14_an_insecure_second_step_is_declined_also_after_the_runs_items_were_spent() {
    let (c, run) = items_spent();
    let (m, before) = (mark(&c), raised(&c));
    let _ = c.on_run(&run, bind(&run, "http://pay.site1.example:80"));
    assert_eq!(
        calls_since(&c, m)
            .into_iter()
            .filter(|call| matches!(call, Call::AnswerBind(..) | Call::Cancel(..)))
            .collect::<Vec<_>>(),
        [Call::AnswerBind(run.clone(), BindResult::Decline)]
    );
    assert_eq!(raised(&c), before);
}

#[test]
fn i14_an_insecure_account_link_is_declined_also_after_the_runs_items_were_spent() {
    let (c, run) = items_spent();
    let (m, before) = (mark(&c), raised(&c));
    let _ = c.on_run(&run, link(&run, "http://10.0.0.1:80"));
    assert_eq!(
        calls_since(&c, m)
            .into_iter()
            .filter(|call| matches!(call, Call::AnswerHandback(..) | Call::Cancel(..)))
            .collect::<Vec<_>>(),
        [Call::AnswerHandback(run.clone(), HandbackResult::Declined)]
    );
    assert_eq!(raised(&c), before);
}

const EVERY_ORIGIN: [&str; 3] = [
    "http://pay.site1.example:80",
    "http://127.0.0.1:8080",
    "https://pay.site1.example:443",
];

fn answers_and_cancels(c: &Coordinator<Both>, m: usize) -> Vec<Call> {
    calls_since(c, m)
        .into_iter()
        .filter(|call| {
            matches!(
                call,
                Call::AnswerBind(..) | Call::AnswerHandback(..) | Call::Cancel(..)
            )
        })
        .collect()
}

#[test]
fn rd116_a_second_request_while_a_card_is_open_ends_the_run_with_kind_for_every_origin() {
    for origin in EVERY_ORIGIN {
        for second in [bind as fn(&RunId, &str) -> RunEvent, link] {
            let c = coordinator(&[MANUAL], Limits::PRODUCT);
            let run = start(&c, 0);
            armed(&c, &run, &[(0xe1, "ab")]);
            let (m, before) = (mark(&c), raised(&c));
            assert_eq!(
                c.on_run(&run, second(&run, origin)),
                Err(halvo_coord::types::Cause::State),
                "{origin}"
            );
            assert_eq!(
                answers_and_cancels(&c, m),
                [Call::Cancel(run.clone())],
                "the kind stop and no answer: {origin}"
            );
            assert_eq!(raised(&c), before, "nothing raised: {origin}");
            assert!(c.snapshot().items.is_empty(), "{origin}");
        }
    }
}

#[test]
fn rd116_a_second_request_of_a_stopping_run_is_refused_with_state_for_every_origin() {
    for origin in EVERY_ORIGIN {
        for second in [bind as fn(&RunId, &str) -> RunEvent, link] {
            let c = coordinator(&[MANUAL], Limits::PRODUCT);
            let run = start(&c, 0);
            assert_eq!(c.control(&agent(0), Action::Stop), Ok(()));
            assert_eq!(
                c.snapshot().rows[0].line,
                line("line.working.stopping"),
                "{origin}"
            );
            let (m, before) = (mark(&c), raised(&c));
            assert_eq!(
                c.on_run(&run, second(&run, origin)),
                Err(halvo_coord::types::Cause::State),
                "{origin}"
            );
            assert!(
                answers_and_cancels(&c, m).is_empty(),
                "nothing sent: {origin}"
            );
            assert_eq!(raised(&c), before, "{origin}");
        }
    }
}

fn kills_tried(c: &Coordinator<Both>) -> usize {
    c.ports()
        .ports
        .tried
        .borrow()
        .iter()
        .filter(|t| matches!(t.call, Call::Kill(_)))
        .count()
}

#[test]
fn rd116_no_kill_stuck_line_before_any_kill_even_at_the_due_instant() {
    let (c, _) = gone_while_live();
    c.ports()
        .ports
        .set_clock(T0 + Limits::PRODUCT.cancel_grace_s * 1000, DAY);
    assert_eq!(
        c.control(&agent(0), Action::Stop),
        Err(halvo_coord::types::Cause::UnknownAgent),
        "a move that publishes and is no tick"
    );
    assert_eq!(kills_tried(&c), 0, "no kill was sent yet");
    let row = row_of(&c).expect("the row stays");
    assert_eq!(row.line, line("line.working.stopping"));
}

#[test]
fn rd116_kill_stuck_after_a_refused_kill_when_the_clock_stepped_back() {
    let (c, _) = gone_while_live();
    c.ports().ports.refuse(
        halvo_coord::mem::Method::Kill,
        halvo_coord::ports::PortError::Busy,
    );
    c.ports().ports.set_clock(T0 - 1_000, DAY);
    c.tick();
    assert_eq!(kills_tried(&c), 1, "the tick tried the kill");
    assert_eq!(c.live(), 1, "the kill did not end the run");
    let row = row_of(&c).expect("the row stays");
    assert_eq!(row.line, line("line.working.kill_stuck"));
    assert!(row.controls.is_empty());
}

fn ask_class(run: &RunId, n: usize, effect: u32, class: Class, fields: &[&str]) -> RunEvent {
    RunEvent::Confirm(
        ConfirmAsk::new(
            run.clone(),
            agent(n),
            vec![AskedEffect {
                effect: EffectId::parse(&format!("{effect:032x}")).unwrap(),
                class,
                digest: Digest::parse(&"ab".repeat(32)).unwrap(),
                digest_ok: true,
                fields: fields
                    .iter()
                    .map(|name| BoundField {
                        name: Name::parse(name).unwrap(),
                        from: BoundFrom::Slot,
                        value: vec!["Lease".into()],
                    })
                    .collect(),
                new_origin: None,
            }],
        )
        .unwrap(),
    )
}

fn live(n: u32) -> Limits {
    Limits {
        max_live_runs: n,
        ..Limits::PRODUCT
    }
}

#[test]
fn rd116_r4_a_sign_in_handback_for_an_http_site_raises_its_item() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    let m = mark(&c);
    let ask = RunEvent::Handback(HandbackAsk {
        run: run.clone(),
        agent: agent(0),
        reason: HandbackReason::SignIn,
        origin: Some(Origin::parse("http://pay.site1.example:80").unwrap()),
    });
    assert_eq!(c.on_run(&run, ask), Ok(()));
    assert!(answers_and_cancels(&c, m).is_empty(), "nothing declined");
    let s = c.snapshot();
    assert_eq!(s.items.len(), 1);
    assert_eq!(s.items[0].kind(), ItemKind::SignIn);
}

#[test]
fn rd116_r4_a_native_confirmation_takes_no_place_among_page_confirmations() {
    let c = coordinator(&[MANUAL, MANUAL], live(2));
    let native = start(&c, 0);
    let page = start(&c, 1);
    assert_eq!(
        c.on_run(
            &native,
            ask_class(&native, 0, 0xe5, Class::Sign, &["document", "party"])
        ),
        Ok(())
    );
    assert_eq!(c.on_run(&page, ask_of(&page, 1, &[(0xe1, "ab")])), Ok(()));
    let s = c.snapshot();
    assert_eq!(s.items.len(), 2, "{s:?}");
    let page_item = s
        .items
        .iter()
        .find(|i| i.presenter() == Some(halvo_coord::types::Presenter::Page))
        .unwrap();
    let native_item = s
        .items
        .iter()
        .find(|i| i.presenter() == Some(halvo_coord::types::Presenter::Native))
        .unwrap();
    let name = name_of(&s, page_item.agent());
    assert_eq!(
        page_item.text(),
        words::text("confirm.asked_by", &[("name", &name)]).unwrap(),
        "one page confirmation has no place"
    );
    let name = name_of(&s, native_item.agent());
    assert_eq!(
        native_item.text(),
        words::text("confirm.asked_by", &[("name", &name)]).unwrap()
    );
}

#[test]
fn rd116_r4_another_item_before_two_page_confirmations_takes_no_place() {
    let c = coordinator(&[MANUAL, MANUAL, MANUAL], live(3));
    let first = start(&c, 0);
    let a = start(&c, 1);
    let b = start(&c, 2);
    assert_eq!(
        c.on_run(&first, link(&first, "https://mail.site1.example:443")),
        Ok(())
    );
    c.ports().ports.set_clock(T0 + 1_000, DAY);
    assert_eq!(c.on_run(&a, ask_of(&a, 1, &[(0xe1, "ab")])), Ok(()));
    assert_eq!(c.on_run(&b, ask_of(&b, 2, &[(0xe2, "ab")])), Ok(()));
    let s = c.snapshot();
    assert_eq!(s.items.len(), 3, "{s:?}");
    assert_eq!(
        s.items[0].kind(),
        ItemKind::LinkAccount,
        "the older item first"
    );
    let places: Vec<String> = s.items[1..]
        .iter()
        .enumerate()
        .map(|(k, item)| {
            let name = name_of(&s, item.agent());
            let n = (k + 1).to_string();
            words::text(
                "confirm.asked_by_place",
                &[("name", &name), ("n", &n), ("count", "2")],
            )
            .unwrap()
        })
        .collect();
    let got: Vec<&str> = s.items[1..].iter().map(|i| i.text()).collect();
    assert_eq!(got, places);
}

#[test]
fn rd116_r4_a_gone_workflows_live_run_has_one_row() {
    let (c, _) = gone_while_live();
    let s = c.snapshot();
    assert_eq!(s.rows.iter().filter(|r| r.agent == agent(0)).count(), 1);
}

#[test]
fn rd116_r4_a_refused_kill_of_one_gone_agent_shows_kill_stuck_on_its_row_alone() {
    let c = coordinator(&[MANUAL, MANUAL], live(2));
    start(&c, 0);
    start(&c, 1);
    c.ports()
        .ports
        .workflows
        .borrow_mut()
        .retain(|w| w.id() != &support::flow(0));
    c.tick();
    c.ports().ports.refuse(
        halvo_coord::mem::Method::Kill,
        halvo_coord::ports::PortError::Busy,
    );
    c.ports()
        .ports
        .set_clock(T0 + Limits::PRODUCT.cancel_grace_s * 1000, DAY);
    c.ports()
        .ports
        .workflows
        .borrow_mut()
        .retain(|w| w.id() != &support::flow(1));
    c.tick();
    assert_eq!(c.live(), 2);
    let s = c.snapshot();
    let line_of = |n: usize| {
        s.rows
            .iter()
            .find(|r| r.agent == agent(n))
            .map(|r| r.line.clone())
            .unwrap()
    };
    assert_eq!(line_of(0), line("line.working.kill_stuck"));
    assert_eq!(
        line_of(1),
        line("line.working.stopping"),
        "no kill of its own yet"
    );
}
