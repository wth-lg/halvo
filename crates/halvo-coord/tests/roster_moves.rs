#[path = "coordinator_support.rs"]
mod support;

use halvo_coord::mem::Call;
use halvo_coord::types::{
    Action, BindAsk, BindResult, HandbackAsk, HandbackReason, HandbackResult, ItemKind, Limits,
    Origin, RunEvent, RunId, What,
};
use halvo_coord::{words, Coordinator};
use support::{agent, calls_since, coordinator, mark, started, Both, MANUAL};

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
