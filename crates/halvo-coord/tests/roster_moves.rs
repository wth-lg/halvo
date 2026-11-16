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

const DECLINED: [&str; 5] = [
    "http://pay.site1.example:80",
    "http://127.example.com:80",
    "http://127.0.0.1.example.com:80",
    "http://localhost:8080",
    "http://10.0.0.1:80",
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
    RunEvent::Confirm(
        ConfirmAsk::new(
            run.clone(),
            agent(0),
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
