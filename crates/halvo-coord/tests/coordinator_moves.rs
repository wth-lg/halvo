#[path = "coordinator_support.rs"]
mod support;

use halvo_coord::items::{choices, ConfirmAnswer};
use halvo_coord::mem::{Call, Method};
use halvo_coord::ports::PortError;
use halvo_coord::sync::Next;
use halvo_coord::types::{
    Action, AgentId, Body, Category, Cause, Choice, Class, ConfirmAsk, HandbackAsk, HandbackReason,
    HoldSetCause, ItemId, ItemKind, LapseCause, Lapsed, Limits, Name, OtherLapse, Outcome, Param,
    PastRun, RunEnd, RunEvent, RunId, Snapshot, Status, What, Workflow, WorkflowId, WorkflowState,
    WorkflowTrigger,
};
use halvo_coord::{words, Coordinator};
use support::{
    agent, calls_since, coordinator, mark, owner, started, Both, DAY, MANUAL, PERSON, T0,
};

/// What a life that saw the whole run writes of its defers: the defer of its launch acknowledged
/// and no tick at which a defer failed (ruling RD-86, RD-120).
const SEEN_WHOLE: Option<halvo_coord::types::Defers> = Some(halvo_coord::types::Defers {
    launch: true,
    ticks_failed: 0,
});

fn text(key: &str) -> String {
    words::text(key, &[]).unwrap()
}

fn row_line(s: &Snapshot, n: usize) -> String {
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

fn events(c: &Coordinator<Both>) -> Vec<What> {
    c.ports()
        .ports
        .events
        .borrow()
        .iter()
        .map(|e| e.what.clone())
        .collect()
}

fn ask_confirm(run: &RunId, n: usize) -> RunEvent {
    RunEvent::Confirm(
        ConfirmAsk::new(
            run.clone(),
            agent(n),
            vec![halvo_coord::types::AskedEffect {
                effect: halvo_coord::types::EffectId::parse(&format!("{:032x}", 0xe1)).unwrap(),
                class: Class::Send,
                digest: halvo_coord::types::Digest::parse(&"ab".repeat(32)).unwrap(),
                digest_ok: true,
                fields: vec![
                    halvo_coord::types::BoundField {
                        name: Name::parse("to").unwrap(),
                        from: halvo_coord::types::BoundFrom::Extract,
                        value: vec!["Ann Lee".into()],
                    },
                    halvo_coord::types::BoundField {
                        name: Name::parse("subject").unwrap(),
                        from: halvo_coord::types::BoundFrom::Extract,
                        value: vec!["Rent".into()],
                    },
                    halvo_coord::types::BoundField {
                        name: Name::parse("body").unwrap(),
                        from: halvo_coord::types::BoundFrom::Model,
                        value: vec!["Thanks".into()],
                    },
                ],
                new_origin: None,
            }],
        )
        .unwrap(),
    )
}

fn handback(run: &RunId, n: usize, reason: HandbackReason) -> RunEvent {
    RunEvent::Handback(HandbackAsk {
        run: run.clone(),
        agent: agent(n),
        reason,
        origin: (reason == HandbackReason::AccountLink)
            .then(|| halvo_coord::types::Origin::parse("https://mail.site1.example:443").unwrap()),
    })
}

fn shown_step_mark_or_zero(c: &Coordinator<Both>, item: &ItemId) -> u32 {
    c.snapshot()
        .items
        .iter()
        .find(|i| i.item() == item)
        .map_or(0, |i| i.step())
}

fn open_item(c: &Coordinator<Both>) -> (ItemId, ItemKind) {
    let s = c.snapshot();
    assert_eq!(s.items.len(), 1, "{s:?}");
    (s.items[0].item().clone(), s.items[0].kind())
}

#[test]
fn the_creation_move_comes_before_each_tick() {
    let both = owner(&[MANUAL], Limits::PRODUCT);
    let fresh = WorkflowId::parse(&format!("{:032x}", 0xf9)).unwrap();
    both.ports
        .workflows
        .borrow_mut()
        .push(Workflow::Full(halvo_coord::types::FullWorkflow {
            id: fresh.clone(),
            state: WorkflowState::Approved,
            category: Category::Email,
            site: None,
            trigger: WorkflowTrigger::Manual,
            period: None,
            last: DAY,
            agent: None,
        }));
    let c = Coordinator::new(both, Limits::PRODUCT);
    let mut made: Vec<AgentId> = Vec::new();
    for k in 0..3 {
        let m = c.ports().ports.tried.borrow().len();
        c.tick();
        let calls: Vec<Call> = c.ports().ports.tried.borrow()[m..]
            .iter()
            .map(|t| t.call.clone())
            .collect();
        let create = calls
            .iter()
            .position(|call| matches!(call, Call::Send(Body::CreateAgent { workflow, .. }) if *workflow == fresh))
            .unwrap_or_else(|| panic!("tick {k}: {calls:?}"));
        let reads: Vec<usize> = calls
            .iter()
            .enumerate()
            .filter(|(_, call)| matches!(call, Call::Workflows))
            .map(|(i, _)| i)
            .collect();
        assert!(
            reads.iter().any(|i| *i < create) && reads.iter().any(|i| *i > create),
            "tick {k}: the creation reads the list, then the tick reads it: {calls:?}"
        );
        if let Call::Send(Body::CreateAgent { agent, .. }) = &calls[create] {
            assert!(!made.contains(agent), "a new agent id each time");
            made.push(agent.clone());
        }
        assert!(!calls
            .iter()
            .any(|call| matches!(call, Call::Send(Body::SetState { .. }))));
    }
}

#[test]
fn the_coordinator_sends_no_body_and_logs_no_event_of_its_own() {
    let source =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/coordinator.rs"))
            .unwrap();
    for word in [
        "Body::",
        "What::",
        "Event {",
        ".send(",
        ".log(",
        ".cancel(",
        ".kill(",
        ".start(&",
        ".answer_confirm(",
        ".answer_handback(",
        ".defer(",
        ".reap(",
    ] {
        assert!(!source.contains(word), "src/coordinator.rs holds {word}");
    }
    assert!(source.contains("schedule::drive(") && source.contains("run::drive("));
}

#[test]
fn the_persons_stop_of_a_waiting_run_ends_it_and_closes_its_item_for_every_kind() {
    type Lapse = fn(OtherLapse) -> Lapsed;
    let cases: Vec<(Option<HandbackReason>, Lapse)> = vec![
        (None, |_| Lapsed::Confirm(LapseCause::Cancelled)),
        (Some(HandbackReason::SignIn), Lapsed::SignIn),
        (Some(HandbackReason::AccountLink), Lapsed::LinkAccount),
        (Some(HandbackReason::StepFailed), Lapsed::Stuck),
        (Some(HandbackReason::EffectUnknown), Lapsed::Check),
    ];
    for (reason, lapsed) in cases {
        let c = coordinator(&[MANUAL], Limits::PRODUCT);
        let run = start(&c, 0);
        let event = match reason {
            None => ask_confirm(&run, 0),
            Some(r) => handback(&run, 0, r),
        };
        assert_eq!(c.on_run(&run, event), Ok(()), "{reason:?}");
        let (item, _) = open_item(&c);
        assert_eq!(c.snapshot().rows[0].status, Status::NeedsYou);
        let m = mark(&c);
        assert_eq!(c.control(&agent(0), Action::Stop), Ok(()), "{reason:?}");
        let cancels = calls_since(&c, m)
            .iter()
            .filter(|call| matches!(call, Call::Cancel(r) if *r == run))
            .count();
        assert_eq!(cancels, 1, "{reason:?}");
        assert!(events(&c).contains(&What::ItemLapsed {
            item,
            lapsed: lapsed(OtherLapse::Cancelled)
        }));
        let s = c.snapshot();
        assert!(s.items.is_empty());
        assert_eq!(row_line(&s, 0), text("line.working.stopping"));
        c.ports().ports.end_runner(&run);
        assert_eq!(c.on_run(&run, RunEvent::Ended(None)), Ok(()));
        assert!(events(&c).contains(&What::RunEnded {
            run: run.clone(),
            cause: RunEnd::Cancelled,
            defers: SEEN_WHOLE
        }));
        assert_eq!(c.live(), 0);
    }
}

#[test]
fn the_persons_stop_of_a_run_whose_confirmation_is_recorded_works() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    c.on_run(&run, ask_confirm(&run, 0)).unwrap();
    let (item, _) = open_item(&c);
    c.on_screen(&item);
    c.ports().ports.set_clock(T0 + 1_000, DAY);
    c.tick();
    c.ports().drop_decision.set(true);
    assert_eq!(
        c.answer(
            &item,
            Choice::Allow,
            None,
            shown_step_mark_or_zero(&c, &item)
        ),
        Err(Cause::NoService)
    );
    assert_eq!(c.snapshot().rows[0].status, Status::Working);
    let m = mark(&c);
    assert_eq!(c.control(&agent(0), Action::Stop), Ok(()));
    let calls = calls_since(&c, m);
    assert_eq!(
        calls
            .iter()
            .filter(|call| matches!(call, Call::Cancel(_)))
            .count(),
        1
    );
    assert_eq!(row_line(&c.snapshot(), 0), text("line.working.stopping"));
    assert_eq!(c.control(&agent(0), Action::Stop), Ok(()), "again");
}

#[test]
fn the_quit_cancels_every_live_run_refuses_every_start_and_kills_what_lives_at_its_second_step() {
    let limits = Limits {
        max_live_runs: 2,
        ..Limits::PRODUCT
    };
    let c = coordinator(&[MANUAL, MANUAL, MANUAL], limits);
    let first = start(&c, 0);
    let second = start(&c, 1);
    assert_eq!(c.control(&agent(2), Action::RunNow), Ok(()));
    assert_eq!(started(&c).len(), 2, "the third waits in the queue");
    c.on_run(&second, handback(&second, 1, HandbackReason::StepFailed))
        .unwrap();
    let (item, _) = open_item(&c);
    let m = mark(&c);
    let wait = c.quit_stop();
    assert_eq!(wait.until, T0 + limits.cancel_grace_s * 1000);
    let mut cancelled: Vec<RunId> = calls_since(&c, m)
        .iter()
        .filter_map(|call| match call {
            Call::Cancel(r) => Some(r.clone()),
            _ => None,
        })
        .collect();
    cancelled.sort();
    let mut both = vec![first.clone(), second.clone()];
    both.sort();
    assert_eq!(cancelled, both, "one cancel for each live run");
    assert!(events(&c).contains(&What::ItemLapsed {
        item,
        lapsed: Lapsed::Stuck(OtherLapse::AppEnded)
    }));
    for n in 0..3 {
        assert_eq!(c.control(&agent(n), Action::RunNow), Err(Cause::State));
    }
    let m = mark(&c);
    c.ports().ports.set_clock(T0 + 2_000, DAY);
    c.tick();
    c.ports().ports.end_runner(&first);
    assert_eq!(
        c.on_run(&first, RunEvent::Ended(Some(Outcome::Completed))),
        Ok(())
    );
    let between = calls_since(&c, m);
    assert!(
        !between
            .iter()
            .any(|call| matches!(call, Call::Start(_) | Call::Send(Body::StartRun { .. }))),
        "{between:?}"
    );
    assert!(events(&c).contains(&What::RunEnded {
        run: first.clone(),
        cause: RunEnd::Cancelled,
        defers: SEEN_WHOLE
    }));
    assert_eq!(c.live(), 1);
    let ended_before = events(&c)
        .iter()
        .filter(|w| matches!(w, What::RunEnded { .. }))
        .count();
    let m = mark(&c);
    c.quit_kill();
    let killed: Vec<RunId> = calls_since(&c, m)
        .iter()
        .filter_map(|call| match call {
            Call::Kill(r) => Some(r.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(killed, [first, second]);
    assert_eq!(c.live(), 0);
    assert_eq!(
        events(&c)
            .iter()
            .filter(|w| matches!(w, What::RunEnded { .. }))
            .count(),
        ended_before,
        "the kill at the quit stores no run_ended"
    );
}

#[test]
fn a_move_of_the_person_after_a_tick_saw_the_workflow_gone_is_unknown_agent_with_nothing_stored() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    c.ports()
        .ports
        .workflows
        .borrow_mut()
        .retain(|w| w.id() != &support::flow(0));
    assert_eq!(c.control(&agent(0), Action::RunNow), Err(Cause::State));
    c.tick();
    let m = mark(&c);
    let tried = c.ports().ports.tried.borrow().len();
    assert_eq!(
        c.control(&agent(0), Action::RunNow),
        Err(Cause::UnknownAgent)
    );
    assert_eq!(
        c.control(&agent(0), Action::SwitchOff),
        Err(Cause::UnknownAgent)
    );
    for action in [Action::SwitchOn, Action::Stop] {
        assert_eq!(c.control(&agent(0), action), Err(Cause::UnknownAgent));
    }
    assert!(calls_since(&c, m)
        .iter()
        .all(|call| matches!(call, Call::Changed | Call::Attention(_))));
    assert_eq!(
        c.ports().ports.tried.borrow().len(),
        tried + calls_since(&c, m).len()
    );
    assert!(
        c.snapshot().rows.is_empty(),
        "the row of an agent whose workflow is gone goes"
    );
    assert_eq!(c.card(&agent(0)), Err(Cause::UnknownAgent));
}

#[test]
fn the_runners_end_of_a_waiting_run_lapses_its_item_with_run_ended_and_ends_the_run() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    c.on_run(&run, handback(&run, 0, HandbackReason::StepFailed))
        .unwrap();
    let (item, _) = open_item(&c);
    c.ports().ports.end_runner(&run);
    assert_eq!(
        c.on_run(&run, RunEvent::Ended(Some(Outcome::HandedBack))),
        Ok(())
    );
    assert!(events(&c).contains(&What::ItemLapsed {
        item,
        lapsed: Lapsed::Stuck(OtherLapse::RunEnded)
    }));
    assert!(events(&c).contains(&What::RunEnded {
        run,
        cause: RunEnd::HandedBack,
        defers: SEEN_WHOLE
    }));
    assert!(c.snapshot().items.is_empty());
    assert_eq!(c.live(), 0);
}

/// A coordinator whose creation move made one agent of a utility bill with no slot, so that it
/// holds an open setup item, and the agent with that item.
fn setup_agent() -> (Coordinator<Both>, AgentId, ItemId) {
    let both = owner(&[], Limits::PRODUCT);
    let flow = WorkflowId::parse(&format!("{:032x}", 0xfa)).unwrap();
    let made = AgentId::parse(&format!("{:032x}", 1)).unwrap();
    both.ports
        .workflows
        .borrow_mut()
        .push(Workflow::Full(halvo_coord::types::FullWorkflow {
            id: flow.clone(),
            state: WorkflowState::Approved,
            category: Category::UtilityBillPayment,
            site: Some("site1.example".into()),
            trigger: WorkflowTrigger::Manual,
            period: None,
            last: DAY,
            agent: None,
        }));
    let mut entry = both.ports.agents.borrow()[..].to_vec();
    entry.push(halvo_coord::types::Agent {
        workflow: flow.clone(),
        agent: made.clone(),
        config: halvo_coord::mem::Config::parse(&format!(
            "{{\"v\":1,\"workflow\":\"{flow}\",\"agent\":\"{made}\",\"rev\":1,\"category\":\"utility_bill_payment\",\"tier\":\"on_device\",\"reasoning\":\"medium\",\"autonomy\":\"draft_and_confirm\",\"allow\":[],\"caps\":{{\"untrusted\":\"yes\",\"private\":\"yes\",\"external\":\"yes\"}},\"limits\":{{\"steps\":20,\"wall_s\":120,\"model_tokens\":0}},\"budget\":null,\"permissions\":[],\"runtime\":\"web\",\"playbook\":null}}"
        ))
        .unwrap(),
        state: halvo_coord::types::AgentState::Approved,
        slots: vec![],
        session: false,
        last: None,
        unknown: vec![],
    });
    let late = both.ports.workflows.borrow_mut().pop().unwrap();
    let c = Coordinator::new(both, Limits::PRODUCT);
    c.tick();
    c.ports().ports.workflows.borrow_mut().push(late);
    *c.ports().ports.agents.borrow_mut() = entry;
    c.tick();
    let (item, kind) = open_item(&c);
    assert_eq!(kind, ItemKind::Setup);
    (c, made, item)
}

#[test]
fn an_agent_whose_setup_body_the_owner_ended_is_unknown_agent_to_the_persons_moves() {
    let (c, made, item) = setup_agent();
    c.answer(
        &item,
        Choice::Choose,
        Some((Param::Start, 1)),
        shown_step_mark_or_zero(&c, &item),
    )
    .unwrap();
    c.answer(
        &item,
        Choice::Choose,
        Some((Param::Payee, 1)),
        shown_step_mark_or_zero(&c, &item),
    )
    .unwrap();
    c.ports().ports.refuse(
        Method::Send,
        PortError::Refused(Name::parse("no_workflow").unwrap()),
    );
    assert_eq!(
        c.setup_typed(
            &item,
            Param::Instrument,
            "Card 4242",
            shown_step_mark_or_zero(&c, &item)
        ),
        Ok(())
    );
    c.ports().ports.clear_refusals();
    let m = mark(&c);
    assert_eq!(c.control(&made, Action::RunNow), Err(Cause::UnknownAgent));
    assert_eq!(
        c.control(&made, Action::SwitchOff),
        Err(Cause::UnknownAgent)
    );
    assert!(calls_since(&c, m)
        .iter()
        .all(|call| matches!(call, Call::Changed | Call::Attention(_))));
}

#[test]
fn an_agent_whose_newest_run_the_store_has_not_ended_shows_waiting_turn_store() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    c.ports().ports.end_runner(&run);
    c.on_run(&run, RunEvent::Ended(Some(Outcome::Completed)))
        .unwrap();
    c.tick();
    assert_eq!(row_line(&c.snapshot(), 0), text("line.waiting_turn.store"));
    c.ports().ports.store_end(
        &agent(0),
        PastRun {
            run,
            day: DAY,
            outcome: Outcome::Completed,
            unknown: 0,
        },
    );
    c.tick();
    assert_eq!(row_line(&c.snapshot(), 0), text("line.all_set.done"));
}

#[test]
fn the_page_holds_the_state_of_a_new_snapshot_after_each_move_and_hears_each_change() {
    let c = coordinator(&[MANUAL, PERSON], Limits::PRODUCT);
    let mut held = c.snapshot();
    let check = |c: &Coordinator<Both>, held: &mut Snapshot| {
        let changed = c.ports().ports.calls.borrow().len();
        let _ = changed;
        if let Next::Changed(s) = c.next(held.seq) {
            *held = s;
        }
        assert_eq!(*held, c.snapshot());
    };
    check(&c, &mut held);
    let rows = held.rows.len();
    assert_eq!(rows, 2);
    let heard = |c: &Coordinator<Both>| {
        c.ports()
            .ports
            .calls
            .borrow()
            .iter()
            .filter(|t| matches!(t.call, Call::Changed))
            .count()
    };
    let run = start(&c, 0);
    let before = heard(&c);
    c.on_run(&run, handback(&run, 0, HandbackReason::SignIn))
        .unwrap();
    assert_eq!(heard(&c), before + 1);
    let attention: Vec<u32> = c
        .ports()
        .ports
        .calls
        .borrow()
        .iter()
        .filter_map(|t| match t.call {
            Call::Attention(n) => Some(n),
            _ => None,
        })
        .collect();
    assert_eq!(attention.last(), Some(&1));
    check(&c, &mut held);
    let (item, _) = open_item(&c);
    c.answer(
        &item,
        Choice::Done,
        None,
        shown_step_mark_or_zero(&c, &item),
    )
    .unwrap();
    check(&c, &mut held);
    let quiet = heard(&c);
    c.tick();
    assert_eq!(
        heard(&c),
        quiet,
        "a tick that changes nothing tells the page nothing"
    );
    assert!(matches!(c.next(held.seq), Next::Same));
    c.control(&agent(0), Action::Stop).unwrap();
    check(&c, &mut held);
    c.ports().ports.end_runner(&run);
    c.on_run(&run, RunEvent::Ended(None)).unwrap();
    check(&c, &mut held);
}

/// A confirmation of a run of agent 0 whose decision the port records and whose answer the
/// support port then fails with `no_service` (`Both::drop_decision`), so that the item stays
/// recorded: the `no_service` comes from that stand-in, not from the memory ports.
fn recorded_with_the_answer_failed_by_the_stand_in(c: &Coordinator<Both>) -> (RunId, ItemId) {
    let run = start(c, 0);
    c.on_run(&run, ask_confirm(&run, 0)).unwrap();
    let item = c
        .snapshot()
        .items
        .iter()
        .find(|i| i.kind() == ItemKind::Confirm)
        .map(|i| i.item().clone())
        .unwrap();
    c.on_screen(&item);
    c.ports().ports.set_clock(T0 + 1_000, DAY);
    c.tick();
    c.ports().drop_decision.set(true);
    assert_eq!(
        c.answer(
            &item,
            Choice::Allow,
            None,
            shown_step_mark_or_zero(c, &item)
        ),
        Err(Cause::NoService)
    );
    (run, item)
}

#[test]
fn a_recorded_confirmation_of_a_stopped_run_lapses_with_run_ended_at_the_runners_end() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let (run, item) = recorded_with_the_answer_failed_by_the_stand_in(&c);
    assert_eq!(c.control(&agent(0), Action::Stop), Ok(()));
    c.ports().ports.end_runner(&run);
    assert_eq!(c.on_run(&run, RunEvent::Ended(None)), Ok(()));
    assert!(events(&c).contains(&What::ItemLapsed {
        item,
        lapsed: Lapsed::Confirm(LapseCause::RunEnded)
    }));
    assert!(events(&c).contains(&What::RunEnded {
        run,
        cause: RunEnd::Cancelled,
        defers: SEEN_WHOLE
    }));
}

#[test]
fn the_quit_lapses_every_open_item_with_app_ended_in_the_life_that_ends() {
    let limits = Limits {
        max_live_runs: 3,
        ..Limits::PRODUCT
    };
    let slotted = support::A {
        person: true,
        slotted: true,
    };
    let c = coordinator(&[MANUAL, MANUAL, slotted], limits);
    let (_, recorded_item) = recorded_with_the_answer_failed_by_the_stand_in(&c);
    let run = start(&c, 1);
    c.on_run(&run, ask_confirm(&run, 1)).unwrap();
    let ready = c
        .snapshot()
        .items
        .iter()
        .find(|i| i.kind() == ItemKind::Ready)
        .map(|i| i.item().clone())
        .expect("the agent with a slot holds a ready item");
    let shown = c
        .snapshot()
        .items
        .iter()
        .find(|i| i.kind() == ItemKind::Confirm)
        .map(|i| i.item().clone())
        .unwrap();
    let wait = c.quit_stop();
    for (item, lapsed) in [
        (recorded_item, Lapsed::Confirm(LapseCause::AppEnded)),
        (shown, Lapsed::Confirm(LapseCause::AppEnded)),
        (ready, Lapsed::Ready(OtherLapse::AppEnded)),
    ] {
        assert!(
            events(&c).contains(&What::ItemLapsed { item, lapsed }),
            "{lapsed:?}"
        );
    }
    assert!(c.snapshot().items.is_empty());
    assert_eq!(wait.until, T0 + 1_000 + limits.cancel_grace_s * 1000);
    let cancels = c
        .ports()
        .ports
        .calls
        .borrow()
        .iter()
        .filter(|t| matches!(t.call, Call::Cancel(_)))
        .count();
    assert_eq!(cancels, 2, "one cancel for each of the two live runs");
}

#[test]
fn a_tick_after_the_first_step_of_the_quit_makes_no_move() {
    let due = support::A {
        person: false,
        slotted: true,
    };
    let c = Coordinator::new(owner(&[due], Limits::PRODUCT), Limits::PRODUCT);
    c.quit_stop();
    let m = mark(&c);
    c.tick();
    let calls = calls_since(&c, m);
    assert!(
        calls
            .iter()
            .all(|call| matches!(call, Call::Changed | Call::Attention(_))),
        "{calls:?}"
    );
    let other = Coordinator::new(owner(&[due], Limits::PRODUCT), Limits::PRODUCT);
    other.tick();
    assert_eq!(
        started(&other).len(),
        1,
        "without the quit the tick starts the due slot"
    );
}

#[test]
fn a_setup_answer_that_is_refused_leaves_the_item_open_and_its_owed_body_owed() {
    let (c, made, item) = setup_agent();
    assert_eq!(
        c.answer(
            &item,
            Choice::Choose,
            Some((Param::Start, 99)),
            shown_step_mark_or_zero(&c, &item)
        ),
        Err(Cause::Malformed)
    );
    let s = c.snapshot();
    assert_eq!(
        s.rows.iter().find(|r| r.agent == made).unwrap().status,
        Status::NeedsYou
    );
    assert_eq!(s.items.len(), 1);
    c.answer(
        &item,
        Choice::Choose,
        Some((Param::Start, 1)),
        shown_step_mark_or_zero(&c, &item),
    )
    .unwrap();
    c.answer(
        &item,
        Choice::Choose,
        Some((Param::Payee, 1)),
        shown_step_mark_or_zero(&c, &item),
    )
    .unwrap();
    c.ports().ports.refuse(Method::Send, PortError::Busy);
    assert_eq!(
        c.setup_typed(
            &item,
            Param::Instrument,
            "Card 4242",
            shown_step_mark_or_zero(&c, &item)
        ),
        Ok(())
    );
    assert_eq!(
        c.answer(
            &item,
            Choice::None,
            None,
            shown_step_mark_or_zero(&c, &item)
        ),
        Err(Cause::State)
    );
    c.ports().ports.clear_refusals();
    let m = mark(&c);
    c.tick();
    assert!(calls_since(&c, m)
        .iter()
        .any(|call| matches!(call, Call::Send(Body::SetBindings { .. }))));
}

#[test]
fn inside_needs_you_the_row_whose_item_was_raised_first_stands_first() {
    let limits = Limits {
        max_live_runs: 2,
        ..Limits::PRODUCT
    };
    let c = coordinator(&[MANUAL, MANUAL], limits);
    let second = start(&c, 1);
    c.on_run(&second, handback(&second, 1, HandbackReason::StepFailed))
        .unwrap();
    c.ports().ports.set_clock(T0 + 1_000, DAY);
    let first = start(&c, 0);
    c.on_run(&first, handback(&first, 0, HandbackReason::StepFailed))
        .unwrap();
    let s = c.snapshot();
    let order: Vec<AgentId> = s.rows.iter().map(|r| r.agent.clone()).collect();
    assert_eq!(order, [agent(1), agent(0)], "{s:?}");
    assert!(
        s.rows[0].name > s.rows[1].name,
        "the older item, not the name"
    );
}

#[test]
fn a_runs_check_item_is_stored_as_the_check_of_that_run() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    c.on_run(&run, handback(&run, 0, HandbackReason::EffectUnknown))
        .unwrap();
    let (item, kind) = open_item(&c);
    assert_eq!(kind, ItemKind::Check);
    assert!(events(&c).contains(&What::ItemRaised {
        item,
        raised: halvo_coord::types::Raised::Check(Some(run)),
    }));
}

#[test]
fn rd61e_pin_an_answered_runs_check_then_an_unknown_effect_at_the_end_brings_the_holds_check() {
    use halvo_coord::types::{HandbackResult, PastRun};
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    c.on_run(&run, handback(&run, 0, HandbackReason::EffectUnknown))
        .unwrap();
    let (item, kind) = open_item(&c);
    assert_eq!(kind, ItemKind::Check);
    let m = mark(&c);
    assert_eq!(
        c.answer(
            &item,
            Choice::TryAgain,
            None,
            shown_step_mark_or_zero(&c, &item)
        ),
        Ok(())
    );
    assert_eq!(handed_back(&c, m), [HandbackResult::Done]);
    assert!(c.snapshot().items.is_empty(), "the run's check is answered");
    c.ports().ports.store_end(
        &agent(0),
        PastRun {
            run: run.clone(),
            day: DAY,
            outcome: Outcome::EffectUnknown,
            unknown: 1,
        },
    );
    c.ports().ports.end_runner(&run);
    assert_eq!(
        c.on_run(&run, RunEvent::Ended(Some(Outcome::EffectUnknown))),
        Ok(())
    );
    c.ports().ports.set_clock(T0 + 1_000, DAY);
    c.tick();
    let s = c.snapshot();
    assert_eq!(s.items.len(), 1, "one card follows: {s:?}");
    assert_eq!(s.items[0].kind(), ItemKind::Check, "the hold's check");
    assert_ne!(s.items[0].item(), &item, "a new item, not the run's");
}

fn bind(run: &RunId, n: usize) -> RunEvent {
    RunEvent::Bind(halvo_coord::types::BindAsk {
        run: run.clone(),
        agent: agent(n),
        origin: halvo_coord::types::Origin::parse("https://pay.site1.example:443").unwrap(),
    })
}

fn bind_answers(c: &Coordinator<Both>) -> Vec<(RunId, halvo_coord::types::BindResult)> {
    c.ports()
        .ports
        .calls
        .borrow()
        .iter()
        .filter_map(|t| match &t.call {
            Call::AnswerBind(run, result) => Some((run.clone(), *result)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_second_step_is_raised_answered_and_handed_to_the_runner_through_answer_bind() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    assert_eq!(c.on_run(&run, bind(&run, 0)), Ok(()));
    let (item, kind) = open_item(&c);
    assert_eq!(kind, ItemKind::SignIn);
    let choices: Vec<Choice> = c.snapshot().items[0]
        .choices()
        .iter()
        .map(|t| t.choice)
        .collect();
    assert_eq!(choices, [Choice::Decline, Choice::Allow]);
    assert_eq!(
        c.answer(
            &item,
            Choice::Allow,
            None,
            shown_step_mark_or_zero(&c, &item)
        ),
        Ok(())
    );
    assert_eq!(
        bind_answers(&c),
        [(run.clone(), halvo_coord::types::BindResult::Allow)]
    );
    assert!(c.snapshot().items.is_empty());
    assert_eq!(row_line(&c.snapshot(), 0), text("line.working.working"));
    assert_eq!(
        c.answer(
            &item,
            Choice::Allow,
            None,
            shown_step_mark_or_zero(&c, &item)
        ),
        Err(Cause::UnknownItem)
    );
}

#[test]
fn the_persons_stop_of_a_second_step_answers_decline_lapses_it_and_stops_the_run() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    c.on_run(&run, bind(&run, 0)).unwrap();
    let (item, _) = open_item(&c);
    let m = mark(&c);
    assert_eq!(c.control(&agent(0), Action::Stop), Ok(()));
    let calls = calls_since(&c, m);
    let decline = calls
        .iter()
        .position(|call| {
            matches!(
                call,
                Call::AnswerBind(_, halvo_coord::types::BindResult::Decline)
            )
        })
        .expect("decline first");
    let cancel = calls
        .iter()
        .position(|call| matches!(call, Call::Cancel(r) if *r == run))
        .expect("one cancel");
    assert!(decline < cancel);
    assert_eq!(
        calls
            .iter()
            .filter(|call| matches!(call, Call::Cancel(_)))
            .count(),
        1
    );
    assert!(events(&c).contains(&What::ItemLapsed {
        item,
        lapsed: Lapsed::SignIn(OtherLapse::Cancelled)
    }));
    assert_eq!(row_line(&c.snapshot(), 0), text("line.working.stopping"));
}

#[test]
fn the_persons_stop_of_a_second_step_whose_row_is_stored_answers_decline_and_lapses_it_cancelled() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    c.on_run(&run, bind(&run, 0)).unwrap();
    let (item, _) = open_item(&c);
    c.ports()
        .ports
        .refuse(Method::AnswerBind, PortError::NoService);
    assert_eq!(
        c.answer(
            &item,
            Choice::Allow,
            None,
            shown_step_mark_or_zero(&c, &item)
        ),
        Err(Cause::NoService)
    );
    c.ports().ports.clear_refusals();
    let m = mark(&c);
    assert_eq!(c.control(&agent(0), Action::Stop), Ok(()));
    let calls = calls_since(&c, m);
    let decline = calls
        .iter()
        .position(|call| {
            matches!(
                call,
                Call::AnswerBind(_, halvo_coord::types::BindResult::Decline)
            )
        })
        .expect("decline first");
    let cancel = calls
        .iter()
        .position(|call| matches!(call, Call::Cancel(r) if *r == run))
        .expect("one cancel");
    assert!(decline < cancel);
    assert!(!calls.iter().any(|call| matches!(
        call,
        Call::AnswerBind(_, halvo_coord::types::BindResult::Allow)
    )));
    assert_eq!(
        calls
            .iter()
            .filter(|call| matches!(call, Call::Cancel(_)))
            .count(),
        1
    );
    assert_eq!(row_line(&c.snapshot(), 0), text("line.working.stopping"));
    assert!(events(&c).contains(&What::ItemLapsed {
        item: item.clone(),
        lapsed: Lapsed::SignIn(OtherLapse::Cancelled)
    }));
    c.ports().ports.end_runner(&run);
    assert_eq!(c.on_run(&run, RunEvent::Ended(None)), Ok(()));
    let lapses = events(&c)
        .iter()
        .filter(|w| matches!(w, What::ItemLapsed { item: i, .. } if *i == item))
        .count();
    assert_eq!(lapses, 1, "the end lapses it no more");
}

#[test]
fn a_second_step_that_waited_wait_limit_lapses_at_a_tick_and_its_run_stops() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    c.on_run(&run, bind(&run, 0)).unwrap();
    let (item, _) = open_item(&c);
    c.ports()
        .ports
        .set_clock(T0 + Limits::PRODUCT.wait_limit_s * 1000, DAY);
    c.tick();
    assert_eq!(
        bind_answers(&c),
        [(run.clone(), halvo_coord::types::BindResult::Decline)]
    );
    assert!(events(&c).contains(&What::ItemLapsed {
        item,
        lapsed: Lapsed::SignIn(OtherLapse::WaitLimit)
    }));
    assert_eq!(row_line(&c.snapshot(), 0), text("line.working.stopping"));
}

#[test]
fn the_quit_lapses_a_second_step_with_app_ended_once() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    c.on_run(&run, bind(&run, 0)).unwrap();
    let (item, _) = open_item(&c);
    c.quit_stop();
    let lapses = events(&c)
        .iter()
        .filter(|w| matches!(w, What::ItemLapsed { item: i, .. } if *i == item))
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        lapses,
        [What::ItemLapsed {
            item,
            lapsed: Lapsed::SignIn(OtherLapse::AppEnded)
        }]
    );
}

#[test]
fn a_link_account_item_shows_its_origin_and_its_answer_stores_it() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    c.on_run(&run, handback(&run, 0, HandbackReason::AccountLink))
        .unwrap();
    let s = c.snapshot();
    assert_eq!(s.items.len(), 1);
    assert_eq!(s.items[0].kind(), ItemKind::LinkAccount);
    assert_eq!(s.items[0].fields().len(), 1, "the site of the account");
    let item = s.items[0].item().clone();
    assert_eq!(
        c.answer(
            &item,
            Choice::Done,
            None,
            shown_step_mark_or_zero(&c, &item)
        ),
        Ok(())
    );
    assert!(events(&c).iter().any(|w| matches!(
        w,
        What::ItemAnswered { item: i, answer: halvo_coord::types::Answered::LinkAccount(a) }
            if *i == item && a.origin.as_str() == "https://mail.site1.example:443"
    )));
}

/// The run ends with `end` as every stop: one `cancel` since `m`, the row says that it stops, and
/// at the first tick after `cancel_grace_s` it is killed and its `run_ended` holds `end`.
fn ends_with(c: &Coordinator<Both>, m: usize, run: &RunId, end: RunEnd, now: u64) {
    let cancels = calls_since(c, m)
        .iter()
        .filter(|call| matches!(call, Call::Cancel(r) if r == run))
        .count();
    assert_eq!(cancels, 1, "one cancel");
    assert_eq!(row_line(&c.snapshot(), 0), text("line.working.stopping"));
    c.ports().ports.set_clock(now + 4_999, DAY);
    c.tick();
    assert!(
        !calls_since(c, m)
            .iter()
            .any(|call| matches!(call, Call::Kill(r) if r == run)),
        "no kill before the grace"
    );
    c.ports()
        .ports
        .set_clock(now + Limits::PRODUCT.cancel_grace_s * 1000, DAY);
    c.tick();
    assert!(calls_since(c, m)
        .iter()
        .any(|call| matches!(call, Call::Kill(r) if r == run)));
    assert!(events(c).contains(&What::RunEnded {
        run: run.clone(),
        cause: end,
        defers: SEEN_WHOLE
    }));
    assert_eq!(c.live(), 0);
}

fn items_raised(c: &Coordinator<Both>) -> usize {
    events(c)
        .iter()
        .filter(|w| matches!(w, What::ItemRaised { .. }))
        .count()
}

fn allowed(c: &Coordinator<Both>) -> (RunId, ItemId) {
    let run = start(c, 0);
    c.on_run(&run, ask_confirm(&run, 0)).unwrap();
    let (item, _) = open_item(c);
    c.on_screen(&item);
    c.ports().ports.set_clock(T0 + 1_000, DAY);
    c.tick();
    (run, item)
}

#[test]
fn a_confirm_request_for_an_effect_whose_newest_release_rejected_it_gets_no_card_and_ends_its_run()
{
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let (run, item) = allowed(&c);
    assert_eq!(
        c.answer(
            &item,
            Choice::Reject,
            None,
            shown_step_mark_or_zero(&c, &item)
        ),
        Ok(())
    );
    let before = items_raised(&c);
    let m = mark(&c);
    assert_eq!(c.on_run(&run, ask_confirm(&run, 0)), Err(Cause::State));
    assert_eq!(items_raised(&c), before, "no card for the late request");
    assert!(c.snapshot().items.is_empty());
    ends_with(&c, m, &run, RunEnd::Kind, T0 + 1_000);
}

#[test]
fn a_confirm_request_for_an_effect_that_has_begun_gets_no_card_and_ends_its_run() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let (run, item) = allowed(&c);
    assert_eq!(
        c.answer(
            &item,
            Choice::Allow,
            None,
            shown_step_mark_or_zero(&c, &item)
        ),
        Ok(())
    );
    c.stored(&halvo_coord::guard::EffectRecord {
        run: run.clone(),
        agent: agent(0),
        effect: halvo_coord::types::EffectId::parse(&format!("{:032x}", 0xe1)).unwrap(),
        state: halvo_coord::guard::EffectState::Committed,
        class: Class::Send,
        digest: halvo_coord::types::Digest::parse(&"ab".repeat(32)).unwrap(),
        approval: None,
    });
    let before = items_raised(&c);
    let m = mark(&c);
    assert_eq!(c.on_run(&run, ask_confirm(&run, 0)), Err(Cause::State));
    assert_eq!(items_raised(&c), before, "no card for the late request");
    ends_with(&c, m, &run, RunEnd::Kind, T0 + 1_000);
}

#[test]
fn a_request_after_items_per_run_items_ends_its_run_with_budget() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    for _ in 0..Limits::PRODUCT.items_per_run {
        c.on_run(&run, handback(&run, 0, HandbackReason::StepFailed))
            .unwrap();
        let (item, _) = open_item(&c);
        c.answer(
            &item,
            Choice::Done,
            None,
            shown_step_mark_or_zero(&c, &item),
        )
        .unwrap();
    }
    let before = items_raised(&c);
    let m = mark(&c);
    assert_eq!(
        c.on_run(&run, handback(&run, 0, HandbackReason::StepFailed)),
        Err(Cause::State)
    );
    assert_eq!(items_raised(&c), before);
    ends_with(&c, m, &run, RunEnd::Budget, T0);
}

#[test]
fn an_agent_whose_setup_body_the_owner_refused_with_binding_is_unknown_agent() {
    let (c, made, item) = setup_agent();
    c.answer(
        &item,
        Choice::Choose,
        Some((Param::Start, 1)),
        shown_step_mark_or_zero(&c, &item),
    )
    .unwrap();
    c.answer(
        &item,
        Choice::Choose,
        Some((Param::Payee, 1)),
        shown_step_mark_or_zero(&c, &item),
    )
    .unwrap();
    c.ports().ports.refuse(
        Method::Send,
        PortError::Refused(Name::parse("binding").unwrap()),
    );
    assert_eq!(
        c.setup_typed(
            &item,
            Param::Instrument,
            "Card 4242",
            shown_step_mark_or_zero(&c, &item)
        ),
        Ok(())
    );
    c.ports().ports.clear_refusals();
    let m = mark(&c);
    for action in [
        Action::RunNow,
        Action::SwitchOff,
        Action::SwitchOn,
        Action::Stop,
    ] {
        assert_eq!(c.control(&made, action), Err(Cause::UnknownAgent));
    }
    c.tick();
    assert!(
        !calls_since(&c, m)
            .iter()
            .any(|call| matches!(call, Call::Send(Body::SetBindings { .. }) | Call::Log(_))),
        "nothing more is sent or stored for that agent"
    );
    assert!(c.snapshot().items.is_empty());
}

#[test]
fn a_second_request_while_the_run_waits_lapses_its_item_and_ends_the_run_with_kind() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    c.on_run(&run, handback(&run, 0, HandbackReason::StepFailed))
        .unwrap();
    let (item, _) = open_item(&c);
    let m = mark(&c);
    assert_eq!(
        c.on_run(&run, handback(&run, 0, HandbackReason::StepFailed)),
        Err(Cause::State)
    );
    assert!(events(&c).contains(&What::ItemLapsed {
        item,
        lapsed: Lapsed::Stuck(OtherLapse::RunEnded)
    }));
    assert!(c.snapshot().items.is_empty());
    ends_with(&c, m, &run, RunEnd::Kind, T0);
}

#[test]
fn a_setup_body_refused_with_config_asks_every_parameter_again_on_the_same_item() {
    let (c, made, item) = setup_agent();
    c.answer(
        &item,
        Choice::Choose,
        Some((Param::Start, 1)),
        shown_step_mark_or_zero(&c, &item),
    )
    .unwrap();
    c.answer(
        &item,
        Choice::Choose,
        Some((Param::Payee, 1)),
        shown_step_mark_or_zero(&c, &item),
    )
    .unwrap();
    c.ports().ports.refuse(
        Method::Send,
        PortError::Refused(Name::parse("config").unwrap()),
    );
    assert_eq!(
        c.setup_typed(
            &item,
            Param::Instrument,
            "Card 4242",
            shown_step_mark_or_zero(&c, &item)
        ),
        Ok(())
    );
    c.ports().ports.clear_refusals();
    let s = c.snapshot();
    assert_eq!(
        s.rows.iter().find(|r| r.agent == made).unwrap().status,
        Status::NeedsYou
    );
    assert_eq!(s.items.len(), 1);
    assert_eq!(s.items[0].item(), &item);
    assert_eq!(s.items[0].params().len(), 3, "every parameter again");
    assert_eq!(
        c.answer(
            &item,
            Choice::Choose,
            Some((Param::Start, 1)),
            shown_step_mark_or_zero(&c, &item)
        ),
        Ok(())
    );
}

#[test]
fn switching_off_an_agent_whose_run_waits_lapses_its_item_with_cancelled_and_stops_the_run() {
    let cases = [
        (None, Lapsed::Confirm(LapseCause::Cancelled)),
        (
            Some(HandbackReason::StepFailed),
            Lapsed::Stuck(OtherLapse::Cancelled),
        ),
    ];
    for (reason, lapsed) in cases {
        let c = coordinator(&[MANUAL], Limits::PRODUCT);
        let run = start(&c, 0);
        let event = match reason {
            None => ask_confirm(&run, 0),
            Some(r) => handback(&run, 0, r),
        };
        assert_eq!(c.on_run(&run, event), Ok(()), "{reason:?}");
        let (item, _) = open_item(&c);
        let m = mark(&c);
        assert_eq!(
            c.control(&agent(0), Action::SwitchOff),
            Ok(()),
            "{reason:?}"
        );
        let cancels = calls_since(&c, m)
            .iter()
            .filter(|call| matches!(call, Call::Cancel(r) if *r == run))
            .count();
        assert_eq!(cancels, 1, "{reason:?}");
        let stored = events(&c);
        assert!(
            stored.contains(&What::ItemLapsed { item, lapsed }),
            "{reason:?}"
        );
        assert!(stored.contains(&What::HoldSet {
            cause: HoldSetCause::Person
        }));
        assert!(c.snapshot().items.is_empty(), "{reason:?}");
        c.ports().ports.end_runner(&run);
        assert_eq!(c.on_run(&run, RunEvent::Ended(None)), Ok(()));
        assert!(events(&c).contains(&What::RunEnded {
            run: run.clone(),
            cause: RunEnd::Cancelled,
            defers: SEEN_WHOLE
        }));
        assert_eq!(c.live(), 0);
    }
}

#[test]
fn the_second_step_of_the_quit_waits_for_the_latest_stop_of_two_live_runs() {
    let limits = Limits {
        max_live_runs: 2,
        ..Limits::PRODUCT
    };
    let c = coordinator(&[MANUAL, MANUAL], limits);
    start(&c, 0);
    start(&c, 1);
    assert_eq!(c.control(&agent(0), Action::Stop), Ok(()));
    c.ports().ports.set_clock(T0 + 2_000, DAY);
    let wait = c.quit_stop();
    assert_eq!(wait.until, T0 + 2_000 + limits.cancel_grace_s * 1000);
}

#[test]
fn the_card_of_an_agent_whose_setup_body_the_owner_ended_is_unknown_agent() {
    let (c, made, item) = setup_agent();
    c.answer(
        &item,
        Choice::Choose,
        Some((Param::Start, 1)),
        shown_step_mark_or_zero(&c, &item),
    )
    .unwrap();
    c.answer(
        &item,
        Choice::Choose,
        Some((Param::Payee, 1)),
        shown_step_mark_or_zero(&c, &item),
    )
    .unwrap();
    assert!(
        c.card(&made).is_ok(),
        "a card before the owner ends the body"
    );
    c.ports().ports.refuse(
        Method::Send,
        PortError::Refused(Name::parse("no_workflow").unwrap()),
    );
    assert_eq!(
        c.setup_typed(
            &item,
            Param::Instrument,
            "Card 4242",
            shown_step_mark_or_zero(&c, &item)
        ),
        Ok(())
    );
    c.ports().ports.clear_refusals();
    for action in [
        Action::RunNow,
        Action::SwitchOn,
        Action::SwitchOff,
        Action::Stop,
    ] {
        assert_eq!(c.control(&made, action), Err(Cause::UnknownAgent));
    }
    assert_eq!(c.card(&made), Err(Cause::UnknownAgent));
}

#[test]
fn a_lapse_at_the_runners_end_that_met_a_busy_owner_is_stored_at_the_next_tick_before_the_run_ended(
) {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    c.on_run(&run, handback(&run, 0, HandbackReason::StepFailed))
        .unwrap();
    let (item, _) = open_item(&c);
    c.ports().ports.refuse(Method::Log, PortError::Busy);
    c.ports().ports.end_runner(&run);
    let _ = c.on_run(&run, RunEvent::Ended(Some(Outcome::HandedBack)));
    c.ports().ports.clear_refusals();
    let lapse = What::ItemLapsed {
        item,
        lapsed: Lapsed::Stuck(OtherLapse::RunEnded),
    };
    assert!(
        !events(&c).contains(&lapse),
        "the busy owner stored nothing"
    );
    let from = c.ports().ports.tried.borrow().len();
    c.tick();
    let refused = c.ports().ports.tried.borrow()[from..]
        .iter()
        .filter(|t| matches!(t.call, Call::Log(_)) && t.refused.is_some())
        .count();
    assert_eq!(
        refused, 0,
        "the board reads the lapse back, so that the owed run_ended follows it at once"
    );
    let stored = events(&c);
    assert_eq!(stored.iter().filter(|w| **w == lapse).count(), 1);
    let at = |what: &What| stored.iter().position(|w| w == what);
    let ended = What::RunEnded {
        run,
        cause: RunEnd::HandedBack,
        defers: SEEN_WHOLE,
    };
    assert!(at(&lapse) < at(&ended), "{stored:?}");
    assert!(c.snapshot().items.is_empty());
    assert_eq!(c.live(), 0);
}

#[test]
fn a_lapse_at_the_quit_that_met_a_busy_owner_is_stored_by_a_tick_or_by_the_second_step() {
    for by_tick in [true, false] {
        let c = coordinator(&[MANUAL], Limits::PRODUCT);
        let run = start(&c, 0);
        c.on_run(&run, handback(&run, 0, HandbackReason::StepFailed))
            .unwrap();
        let (item, _) = open_item(&c);
        c.ports().ports.refuse(Method::Log, PortError::Busy);
        let _ = c.quit_stop();
        c.ports().ports.clear_refusals();
        let lapse = What::ItemLapsed {
            item,
            lapsed: Lapsed::Stuck(OtherLapse::AppEnded),
        };
        assert!(
            !events(&c).contains(&lapse),
            "{by_tick}: nothing stored yet"
        );
        if by_tick {
            c.tick();
        } else {
            c.quit_kill();
        }
        assert_eq!(
            events(&c).iter().filter(|w| **w == lapse).count(),
            1,
            "{by_tick}"
        );
    }
    let (c, _made, item) = setup_agent();
    c.ports().ports.refuse(Method::Log, PortError::Busy);
    let _ = c.quit_stop();
    c.ports().ports.clear_refusals();
    c.tick();
    assert!(events(&c).contains(&What::ItemLapsed {
        item,
        lapsed: Lapsed::Setup(OtherLapse::AppEnded)
    }));
}

#[test]
fn the_second_step_of_the_quit_comes_at_the_latest_grace_after_the_first_whatever_the_clock_did() {
    let limits = Limits::PRODUCT;
    let grace = limits.cancel_grace_s * 1000;
    let c = coordinator(&[MANUAL], limits);
    start(&c, 0);
    assert_eq!(c.control(&agent(0), Action::Stop), Ok(()));
    c.ports().ports.set_clock(T0 + 2_000, DAY);
    assert_eq!(
        c.quit_stop().until,
        T0 + grace,
        "a run stopped before the quit counts the grace from its stop"
    );
    let c = coordinator(&[MANUAL], limits);
    start(&c, 0);
    c.ports().ports.set_clock(T0 + 2_000, DAY);
    assert_eq!(c.control(&agent(0), Action::Stop), Ok(()));
    c.ports().ports.set_clock(T0 + 1_000, DAY);
    assert_eq!(
        c.quit_stop().until,
        T0 + 1_000 + grace,
        "a stop that reads later than the first step counts from the first step"
    );
    let c = coordinator(&[MANUAL], limits);
    c.ports().ports.set_clock(T0 + 3_000, DAY);
    assert_eq!(c.quit_stop().until, T0 + 3_000, "no live run");
}

#[test]
fn two_agents_of_one_name_get_their_ordinal_among_the_shown_agents_in_the_row_and_in_the_card() {
    let c = coordinator(&[MANUAL, MANUAL, MANUAL], Limits::PRODUCT);
    let named = |c: &Coordinator<Both>, n: usize| {
        c.snapshot()
            .rows
            .iter()
            .find(|r| r.agent == agent(n))
            .map(|r| r.name.clone())
            .unwrap()
    };
    let first = named(&c, 0);
    let third = named(&c, 2);
    assert!(third.ends_with(" (3)"), "{third}");
    assert_eq!(c.card(&agent(2)).unwrap().name, third);
    c.ports()
        .ports
        .workflows
        .borrow_mut()
        .retain(|w| w.id() != &support::flow(1));
    c.tick();
    assert_eq!(c.snapshot().rows.len(), 2);
    assert_eq!(named(&c, 0), first, "the first of the name has no ordinal");
    let second = format!("{first} (2)");
    assert_eq!(
        named(&c, 2),
        second,
        "the third agent is the second shown of its name"
    );
    for n in [0, 2] {
        assert_eq!(c.card(&agent(n)).unwrap().name, named(&c, n), "{n}");
    }
    assert_eq!(c.card(&agent(1)), Err(Cause::UnknownAgent));
}

/// The next life of the app over the owner of the life of `c` (its workflows, agents and
/// events, which `change` may change), with the random source moved past the ids that the
/// life of `c` drew, after its first tick.
fn next_life(c: &Coordinator<Both>, change: impl FnOnce(&Both)) -> Coordinator<Both> {
    use halvo_coord::ports::Rng;
    let old = c.ports();
    let next = owner(&[], Limits::PRODUCT);
    *next.ports.workflows.borrow_mut() = old.ports.workflows.borrow().clone();
    *next.ports.agents.borrow_mut() = old.ports.agents.borrow().clone();
    *next.ports.events.borrow_mut() = old.ports.events.borrow().clone();
    for _ in 0..64 {
        let _ = next.ports.bytes16();
    }
    change(&next);
    let life = Coordinator::new(next, Limits::PRODUCT);
    life.tick();
    life
}

fn setups_raised(c: &Coordinator<Both>) -> usize {
    events(c)
        .iter()
        .filter(|w| {
            matches!(
                w,
                What::ItemRaised {
                    raised: halvo_coord::types::Raised::Setup,
                    ..
                }
            )
        })
        .count()
}

fn workflow_of(c: &Coordinator<Both>, made: &AgentId) -> WorkflowId {
    c.ports()
        .ports
        .agents
        .borrow()
        .iter()
        .find(|a| &a.agent == made)
        .map(|a| a.workflow.clone())
        .unwrap()
}

#[test]
fn a_setup_that_the_quit_or_recovery_lapsed_is_raised_again_by_the_next_lifes_first_creation_move()
{
    for (how, quit, named) in [
        ("quit", true, false),
        ("crash", false, false),
        ("quit, the workflow names its agent", true, true),
    ] {
        let (c, made, old) = setup_agent();
        if quit {
            let _ = c.quit_stop();
            c.quit_kill();
        }
        let flow = workflow_of(&c, &made);
        let life = next_life(&c, |o| {
            for w in o.ports.workflows.borrow_mut().iter_mut() {
                if let Workflow::Full(f) = w {
                    if named && f.id == flow {
                        f.agent = Some(made.clone());
                    }
                }
            }
        });
        assert!(
            events(&life).contains(&What::ItemLapsed {
                item: old.clone(),
                lapsed: Lapsed::Setup(OtherLapse::AppEnded)
            }),
            "{how}"
        );
        let s = life.snapshot();
        let row = s.rows.iter().find(|r| r.agent == made).unwrap();
        assert_eq!(row.status, Status::NeedsYou, "{how}");
        let setups: Vec<_> = s
            .items
            .iter()
            .filter(|i| i.kind() == ItemKind::Setup)
            .collect();
        assert_eq!(setups.len(), 1, "{how}");
        assert_ne!(setups[0].item(), &old, "{how}: a new item");
        assert_eq!(setups[0].params().len(), 3, "{how}: every parameter");
        life.tick();
        assert_eq!(setups_raised(&life), 2, "{how}: one raise in each life");
    }
}

#[test]
fn a_setup_answered_in_this_life_is_not_raised_again_by_a_later_creation_move() {
    let (c, made, item) = setup_agent();
    c.answer(
        &item,
        Choice::Choose,
        Some((Param::Start, 1)),
        shown_step_mark_or_zero(&c, &item),
    )
    .unwrap();
    c.answer(
        &item,
        Choice::Choose,
        Some((Param::Payee, 1)),
        shown_step_mark_or_zero(&c, &item),
    )
    .unwrap();
    assert_eq!(
        c.setup_typed(
            &item,
            Param::Instrument,
            "Card 4242",
            shown_step_mark_or_zero(&c, &item)
        ),
        Ok(())
    );
    assert!(c.snapshot().items.is_empty());
    for _ in 0..2 {
        c.tick();
    }
    let s = c.snapshot();
    assert!(s.items.iter().all(|i| i.kind() != ItemKind::Setup), "{s:?}");
    assert!(s.rows.iter().any(|r| r.agent == made));
    assert_eq!(setups_raised(&c), 1);
}

#[test]
fn the_next_life_raises_no_setup_for_an_agent_of_a_dismissed_workflow_or_one_with_an_open_check() {
    let (c, made, _) = setup_agent();
    let _ = c.quit_stop();
    c.quit_kill();
    let flow = workflow_of(&c, &made);
    let life = next_life(&c, |o| {
        for w in o.ports.workflows.borrow_mut().iter_mut() {
            if w.id() == &flow {
                *w = Workflow::Dismissed(halvo_coord::types::DismissedWorkflow {
                    id: flow.clone(),
                    category: Category::UtilityBillPayment,
                    site: Some("site1.example".into()),
                });
            }
        }
    });
    assert_eq!(
        setups_raised(&life),
        1,
        "dismissed: the first life's raise only"
    );
    assert!(life
        .snapshot()
        .items
        .iter()
        .all(|i| i.kind() != ItemKind::Setup));
    let (c, made, _) = setup_agent();
    let _ = c.quit_stop();
    c.quit_kill();
    let flow = workflow_of(&c, &made);
    let life = next_life(&c, |o| {
        let run = RunId::parse(&format!("{:032x}", 0x0700)).unwrap();
        let last = o
            .ports
            .events
            .borrow()
            .iter()
            .filter(|e| e.agent == made)
            .map(|e| e.seq.get())
            .max()
            .unwrap_or(0);
        let event = |k: u32, what: What| halvo_coord::types::Event {
            workflow: flow.clone(),
            agent: made.clone(),
            seq: std::num::NonZeroU32::new(last + k).unwrap(),
            day: DAY,
            at: 0,
            what,
        };
        o.ports.events.borrow_mut().extend([
            event(
                1,
                What::RunStarted {
                    run: run.clone(),
                    trigger: halvo_coord::types::Trigger::Person,
                    slot: None,
                },
            ),
            event(
                2,
                What::RunEnded {
                    run: run.clone(),
                    cause: RunEnd::Completed,
                    defers: None,
                },
            ),
        ]);
        for a in o.ports.agents.borrow_mut().iter_mut() {
            if a.agent == made {
                a.last = Some(PastRun {
                    run: run.clone(),
                    day: DAY,
                    outcome: Outcome::Completed,
                    unknown: 1,
                });
            }
        }
    });
    let s = life.snapshot();
    assert!(
        s.items.iter().any(|i| i.kind() == ItemKind::Check),
        "recovery raised the check: {s:?}"
    );
    assert!(s.items.iter().all(|i| i.kind() != ItemKind::Setup));
    assert_eq!(
        setups_raised(&life),
        1,
        "open check: the first life's raise only"
    );
}

fn cancels_of(c: &Coordinator<Both>, m: usize, run: &RunId) -> usize {
    calls_since(c, m)
        .iter()
        .filter(|call| matches!(call, Call::Cancel(r) if r == run))
        .count()
}

fn kills_of(c: &Coordinator<Both>, m: usize, run: &RunId) -> usize {
    calls_since(c, m)
        .iter()
        .filter(|call| matches!(call, Call::Kill(r) if r == run))
        .count()
}

#[test]
fn a_confirmation_that_waited_wait_limit_lapses_and_its_run_stops_and_ends_lapsed() {
    let limits = Limits::PRODUCT;
    let c = coordinator(&[MANUAL], limits);
    let run = start(&c, 0);
    c.on_run(&run, ask_confirm(&run, 0)).unwrap();
    let (item, _) = open_item(&c);
    let limit = T0 + limits.wait_limit_s * 1000;
    c.ports().ports.set_clock(limit - 1, DAY);
    c.tick();
    assert_eq!(
        c.snapshot().items.len(),
        1,
        "not lapsed before wait_limit_s"
    );
    let m = mark(&c);
    c.ports().ports.set_clock(limit, DAY);
    c.tick();
    assert!(events(&c).contains(&What::ItemLapsed {
        item,
        lapsed: Lapsed::Confirm(LapseCause::WaitLimit)
    }));
    assert_eq!(cancels_of(&c, m, &run), 1);
    let s = c.snapshot();
    assert!(s.items.is_empty());
    assert_eq!(row_line(&s, 0), text("line.working.stopping"));
    let m = mark(&c);
    c.ports()
        .ports
        .set_clock(limit + limits.cancel_grace_s * 1000, DAY);
    c.tick();
    assert_eq!(
        kills_of(&c, m, &run),
        1,
        "the grace tick kills the stopped run"
    );
    assert!(events(&c).contains(&What::RunEnded {
        run,
        cause: RunEnd::Lapsed,
        defers: SEEN_WHOLE
    }));
    assert_eq!(c.live(), 0);
}

#[test]
fn a_confirmation_answered_and_not_yet_released_shows_no_card() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let (_, item) = recorded_with_the_answer_failed_by_the_stand_in(&c);
    let s = c.snapshot();
    assert_eq!(s.rows[0].status, Status::Working);
    assert!(
        s.items.iter().all(|i| i.item() != &item),
        "no card for a recorded confirmation: {s:?}"
    );
}

#[test]
fn the_coordinator_calls_neither_log_nor_send_itself() {
    let stack = std::backtrace::Backtrace::force_capture().to_string();
    if !stack.contains("the_coordinator_calls_neither_log_nor_send_itself") {
        eprintln!("this target's backtraces name no function, so the check cannot judge here");
        return;
    }
    let c = coordinator(&[MANUAL, MANUAL], Limits::PRODUCT);
    c.ports().trace.set(true);
    let first = start(&c, 0);
    c.on_run(&first, ask_confirm(&first, 0)).unwrap();
    let (item, _) = open_item(&c);
    c.on_screen(&item);
    c.ports().ports.set_clock(T0 + 1_000, DAY);
    c.tick();
    let _ = c.answer(
        &item,
        Choice::Allow,
        None,
        shown_step_mark_or_zero(&c, &item),
    );
    c.ports().ports.end_runner(&first);
    let _ = c.on_run(&first, RunEvent::Ended(Some(Outcome::Completed)));
    let second = start(&c, 1);
    c.on_run(&second, handback(&second, 1, HandbackReason::StepFailed))
        .unwrap();
    assert_eq!(c.control(&agent(1), Action::Stop), Ok(()));
    assert_eq!(c.control(&agent(0), Action::SwitchOff), Ok(()));
    assert_eq!(c.control(&agent(0), Action::SwitchOn), Ok(()));
    c.ports().ports.set_clock(T0 + 30_000, DAY);
    c.tick();
    let quit = coordinator(&[MANUAL], Limits::PRODUCT);
    quit.ports().trace.set(true);
    let third = start(&quit, 0);
    quit.on_run(&third, handback(&third, 0, HandbackReason::StepFailed))
        .unwrap();
    let _ = quit.quit_stop();
    quit.tick();
    quit.quit_kill();
    let (setup, made, open) = setup_agent();
    setup.ports().trace.set(true);
    setup
        .answer(
            &open,
            Choice::Choose,
            Some((Param::Start, 1)),
            shown_step_mark_or_zero(&setup, &open),
        )
        .unwrap();
    setup
        .answer(
            &open,
            Choice::Choose,
            Some((Param::Payee, 1)),
            shown_step_mark_or_zero(&setup, &open),
        )
        .unwrap();
    assert_eq!(
        setup.setup_typed(
            &open,
            Param::Instrument,
            "Card 4242",
            shown_step_mark_or_zero(&setup, &open)
        ),
        Ok(())
    );
    let _ = setup.control(&made, Action::RunNow);
    let mut callers = c.ports().callers.borrow().clone();
    callers.extend(quit.ports().callers.borrow().iter().cloned());
    callers.extend(setup.ports().callers.borrow().iter().cloned());
    assert!(
        callers.iter().any(|c| c.starts_with("::log <- "))
            && callers.iter().any(|c| c.starts_with("::send <- ")),
        "the check saw calls of both: {callers:?}"
    );
    let own: Vec<&String> = callers
        .iter()
        .filter(|c| c.contains("<- halvo_coord::coordinator::"))
        .collect();
    assert!(
        own.is_empty(),
        "the coordinator called them itself: {own:?}"
    );
}

#[test]
fn a_stop_while_every_log_answers_no_service_stores_its_lapse_before_the_run_ended_once_the_owner_is_back(
) {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    c.on_run(&run, handback(&run, 0, HandbackReason::StepFailed))
        .unwrap();
    let (item, _) = open_item(&c);
    c.ports().ports.refuse(Method::Log, PortError::NoService);
    assert_eq!(c.control(&agent(0), Action::Stop), Ok(()));
    c.ports().ports.end_runner(&run);
    let _ = c.on_run(&run, RunEvent::Ended(None));
    let lapse = What::ItemLapsed {
        item,
        lapsed: Lapsed::Stuck(OtherLapse::Cancelled),
    };
    assert!(!events(&c).contains(&lapse));
    c.ports().ports.clear_refusals();
    c.ports().ports.set_clock(T0 + 1_000, DAY);
    c.tick();
    let stored = events(&c);
    assert_eq!(stored.iter().filter(|w| **w == lapse).count(), 1);
    let ended = What::RunEnded {
        run: run.clone(),
        cause: RunEnd::Cancelled,
        defers: SEEN_WHOLE,
    };
    assert_eq!(stored.iter().filter(|w| **w == ended).count(), 1);
    let at = |what: &What| stored.iter().position(|w| w == what);
    assert!(at(&lapse) < at(&ended), "{stored:?}");
}

/// A coordinator whose agent 0 has a run that waits on the item of the handback `reason`,
/// raised at `T0`, and that waited `wait_limit_s` until the tick that timed it out.
fn timed_out(reason: HandbackReason) -> (Coordinator<Both>, RunId, ItemId, usize) {
    let limits = Limits::PRODUCT;
    let c = coordinator(&[MANUAL], limits);
    let run = start(&c, 0);
    c.on_run(&run, handback(&run, 0, reason)).unwrap();
    let (item, _) = open_item(&c);
    let limit = T0 + limits.wait_limit_s * 1000;
    c.ports().ports.set_clock(limit - 1, DAY);
    c.tick();
    assert_eq!(
        c.snapshot().items.len(),
        1,
        "{reason:?}: not lapsed before wait_limit_s"
    );
    let m = mark(&c);
    c.ports().ports.set_clock(limit, DAY);
    c.tick();
    (c, run, item, m)
}

#[test]
fn an_item_that_a_run_waits_on_lapses_at_wait_limit_and_its_run_ends_lapsed() {
    let limits = Limits::PRODUCT;
    type Lapse = fn(OtherLapse) -> Lapsed;
    let kinds: [(HandbackReason, Lapse); 4] = [
        (HandbackReason::SignIn, Lapsed::SignIn),
        (HandbackReason::AccountLink, Lapsed::LinkAccount),
        (HandbackReason::StepFailed, Lapsed::Stuck),
        (HandbackReason::EffectUnknown, Lapsed::Check),
    ];
    for (reason, lapsed) in kinds {
        let (c, run, item, m) = timed_out(reason);
        let calls = calls_since(&c, m);
        assert!(
            calls.contains(&Call::AnswerHandback(
                run.clone(),
                halvo_coord::types::HandbackResult::Timeout
            )),
            "{reason:?}: the handback is answered with timeout: {calls:?}"
        );
        assert!(
            events(&c).contains(&What::ItemLapsed {
                item,
                lapsed: lapsed(OtherLapse::WaitLimit)
            }),
            "{reason:?}"
        );
        assert_eq!(cancels_of(&c, m, &run), 1, "{reason:?}");
        let s = c.snapshot();
        assert!(s.items.is_empty(), "{reason:?}");
        assert_eq!(row_line(&s, 0), text("line.working.stopping"), "{reason:?}");
        let m = mark(&c);
        c.ports().ports.set_clock(
            T0 + limits.wait_limit_s * 1000 + limits.cancel_grace_s * 1000,
            DAY,
        );
        c.tick();
        assert_eq!(kills_of(&c, m, &run), 1, "{reason:?}");
        assert!(
            events(&c).contains(&What::RunEnded {
                run: run.clone(),
                cause: RunEnd::Lapsed,
                defers: SEEN_WHOLE
            }),
            "{reason:?}"
        );
        assert_eq!(c.live(), 0, "{reason:?}");
    }
}

#[test]
fn the_holds_check_item_never_lapses_by_time() {
    let limits = Limits::PRODUCT;
    let c = coordinator(&[MANUAL], limits);
    let run = start(&c, 0);
    c.ports().ports.end_runner(&run);
    c.on_run(&run, RunEvent::Ended(Some(Outcome::Completed)))
        .unwrap();
    c.ports().ports.store_end(
        &agent(0),
        PastRun {
            run: run.clone(),
            day: DAY,
            outcome: Outcome::Completed,
            unknown: 1,
        },
    );
    c.ports().ports.set_clock(T0 + 1_000, DAY);
    c.tick();
    let (item, kind) = open_item(&c);
    assert_eq!(kind, ItemKind::Check);
    for k in 1..4u64 {
        c.ports()
            .ports
            .set_clock(T0 + k * limits.wait_limit_s * 1000, DAY);
        c.tick();
        let (still, _) = open_item(&c);
        assert_eq!(still, item, "the hold's check stands after {k} wait limits");
    }
    assert!(!events(&c)
        .iter()
        .any(|w| matches!(w, What::ItemLapsed { item: i, .. } if *i == item)));
}

#[test]
fn after_a_lapse_at_wait_limit_the_person_finds_no_card_the_row_stopped_early_and_a_start_passes() {
    let limits = Limits::PRODUCT;
    let (c, run, _, _) = timed_out(HandbackReason::StepFailed);
    let grace = T0 + limits.wait_limit_s * 1000 + limits.cancel_grace_s * 1000;
    c.ports().ports.set_clock(grace, DAY);
    c.tick();
    c.ports().ports.store_end(
        &agent(0),
        PastRun {
            run: run.clone(),
            day: DAY,
            outcome: Outcome::HandedBack,
            unknown: 0,
        },
    );
    c.ports().ports.set_clock(grace + 1_000, DAY);
    c.tick();
    let s = c.snapshot();
    assert!(s.items.is_empty());
    assert_eq!(s.rows[0].status, Status::AllSet);
    assert_eq!(row_line(&s, 0), text("line.all_set.stopped_early"));
    assert!(events(&c).contains(&What::RunEnded {
        run: run.clone(),
        cause: RunEnd::Lapsed,
        defers: SEEN_WHOLE
    }));
    let next = start(&c, 0);
    assert_ne!(next, run);
}

#[test]
fn a_stop_marks_the_guards_table_and_the_board_before_a_cancel_that_panics() {
    use halvo_coord::run::{drive, recover, Act, StopBy};
    let limits = Limits::PRODUCT;
    let both = owner(&[MANUAL], limits);
    let mut board = recover(&both, &limits).unwrap().board;
    let mut table = halvo_coord::guard::Table::new();
    let started = drive(
        &both,
        &mut board,
        &mut table,
        &limits,
        Act::Start { agent: agent(0) },
    );
    let run = started.report.run.unwrap();
    drive(
        &both,
        &mut board,
        &mut table,
        &limits,
        Act::Hello { run: run.clone() },
    );
    both.panics.set(Some("cancel"));
    let stop = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        drive(
            &both,
            &mut board,
            &mut table,
            &limits,
            Act::Stop {
                run: run.clone(),
                by: StopBy::Person,
            },
        )
    }));
    both.panics.set(None);
    assert!(stop.is_err(), "the port's cancel panicked");
    assert!(
        table.is_stopped(&run),
        "the guard refuses the run's records"
    );
    let live = board.runs.iter().find(|r| r.run == run).unwrap();
    assert!(
        live.stop.is_some(),
        "the board holds the stop, so the grace tick kills the run"
    );
    both.ports.set_clock(T0 + limits.cancel_grace_s * 1000, DAY);
    let tick = drive(&both, &mut board, &mut table, &limits, Act::Tick);
    assert!(
        both.ports
            .calls
            .borrow()
            .iter()
            .any(|t| t.call == Call::Kill(run.clone())),
        "{tick:?}"
    );
}

#[test]
fn a_start_puts_the_run_on_the_board_only_after_a_start_that_returned() {
    use halvo_coord::run::{drive, recover, Act};
    let limits = Limits::PRODUCT;
    let both = owner(&[MANUAL], limits);
    let mut board = recover(&both, &limits).unwrap().board;
    let mut table = halvo_coord::guard::Table::new();
    both.panics.set(Some("start"));
    let start = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        drive(
            &both,
            &mut board,
            &mut table,
            &limits,
            Act::Start { agent: agent(0) },
        )
    }));
    both.panics.set(None);
    assert!(start.is_err(), "the port's start panicked");
    assert!(
        board.runs.is_empty(),
        "no run on the board: {:?}",
        board.runs
    );
    assert!(
        both.ports
            .tried
            .borrow()
            .iter()
            .any(|t| matches!(t.call, Call::Send(Body::StartRun { .. }))),
        "the start got as far as its body"
    );
}

fn run_ends(c: &Coordinator<Both>) -> usize {
    events(c)
        .iter()
        .filter(|w| matches!(w, What::RunEnded { .. }))
        .count()
}

#[test]
fn after_the_quits_first_step_a_tick_kills_each_stopping_run_at_its_own_grace_with_no_run_ended() {
    let limits = Limits {
        max_live_runs: 2,
        ..Limits::PRODUCT
    };
    let grace = limits.cancel_grace_s * 1000;
    let c = coordinator(&[MANUAL, MANUAL], limits);
    let first = start(&c, 0);
    let second = start(&c, 1);
    assert_eq!(c.control(&agent(0), Action::Stop), Ok(()));
    c.ports().ports.set_clock(T0 + 2_000, DAY);
    let _ = c.quit_stop();
    let ends = run_ends(&c);
    let m = mark(&c);
    c.ports().ports.set_clock(T0 + grace, DAY);
    c.tick();
    assert_eq!(kills_of(&c, m, &first), 1, "the first run's own grace");
    assert_eq!(kills_of(&c, m, &second), 0, "not the second's yet");
    let m = mark(&c);
    c.ports().ports.set_clock(T0 + 2_000 + grace - 1, DAY);
    c.tick();
    assert_eq!(kills_of(&c, m, &second), 0);
    c.ports().ports.set_clock(T0 + 2_000 + grace, DAY);
    c.tick();
    assert_eq!(kills_of(&c, m, &second), 1);
    assert_eq!(c.live(), 0);
    assert_eq!(run_ends(&c), ends, "the kill stores no run_ended");
}

#[test]
fn after_the_quits_first_step_a_tick_kills_a_dying_tree_at_its_grace() {
    let limits = Limits::PRODUCT;
    let c = coordinator(&[MANUAL], limits);
    let run = start(&c, 0);
    assert_eq!(c.control(&agent(0), Action::Stop), Ok(()));
    c.ports().ports.end_runner(&run);
    assert_eq!(c.on_run(&run, RunEvent::Ended(None)), Ok(()));
    assert_eq!(c.live(), 0, "the run ended; its tree waits for the kill");
    c.ports().ports.set_clock(T0 + 1_000, DAY);
    let _ = c.quit_stop();
    let m = mark(&c);
    c.ports()
        .ports
        .set_clock(T0 + limits.cancel_grace_s * 1000 - 1, DAY);
    c.tick();
    assert_eq!(kills_of(&c, m, &run), 0, "not before its grace");
    c.ports()
        .ports
        .set_clock(T0 + limits.cancel_grace_s * 1000, DAY);
    c.tick();
    assert_eq!(kills_of(&c, m, &run), 1);
}

#[test]
fn after_the_quits_first_step_a_tick_sends_what_the_board_owes() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    c.ports().ports.end_runner(&run);
    c.ports().ports.refuse(Method::Log, PortError::NoService);
    let _ = c.on_run(&run, RunEvent::Ended(Some(Outcome::Completed)));
    c.ports().ports.clear_refusals();
    let ended = What::RunEnded {
        run: run.clone(),
        cause: RunEnd::Completed,
        defers: SEEN_WHOLE,
    };
    assert!(!events(&c).contains(&ended), "the end is owed");
    let _ = c.quit_stop();
    assert!(!events(&c).contains(&ended));
    c.ports().ports.set_clock(T0 + 1_000, DAY);
    c.tick();
    assert_eq!(events(&c).iter().filter(|w| **w == ended).count(), 1);
}

#[test]
fn after_the_quits_first_step_a_tick_serves_no_queue_starts_nothing_and_raises_nothing() {
    let limits = Limits {
        max_live_runs: 1,
        ..Limits::PRODUCT
    };
    let c = coordinator(&[MANUAL, MANUAL], limits);
    let first = start(&c, 0);
    assert_eq!(c.control(&agent(1), Action::RunNow), Ok(()));
    assert_eq!(started(&c).len(), 1, "the second waits in the queue");
    c.ports().ports.store_end(
        &agent(0),
        PastRun {
            run: first.clone(),
            day: DAY,
            outcome: Outcome::Completed,
            unknown: 1,
        },
    );
    let _ = c.quit_stop();
    let raised = items_raised(&c);
    let m = mark(&c);
    c.ports()
        .ports
        .set_clock(T0 + limits.cancel_grace_s * 1000, DAY);
    c.tick();
    c.ports()
        .ports
        .set_clock(T0 + 2 * limits.cancel_grace_s * 1000, DAY);
    c.tick();
    let calls = calls_since(&c, m);
    assert_eq!(kills_of(&c, m, &first), 1);
    assert!(
        !calls
            .iter()
            .any(|call| matches!(call, Call::Start(_) | Call::Send(Body::StartRun { .. }))),
        "{calls:?}"
    );
    assert_eq!(started(&c).len(), 1);
    assert_eq!(items_raised(&c), raised, "no check and no ready item");
    assert!(c.snapshot().items.is_empty());
}

/// The owner stored the body that it refused last, and only its acknowledgement was lost.
fn lose_the_acknowledgement(c: &Coordinator<Both>) {
    let tried = c.ports().ports.tried.borrow().clone();
    let events = tried
        .iter()
        .rev()
        .find_map(|t| match (&t.call, &t.refused) {
            (Call::Log(events), Some(_)) => Some(events.clone()),
            _ => None,
        })
        .expect("a refused log");
    c.ports().ports.events.borrow_mut().extend(events);
    c.ports().ports.clear_refusals();
}

fn closing_rows(c: &Coordinator<Both>, item: &ItemId) -> usize {
    events(c)
        .iter()
        .filter(|w| {
            matches!(w, What::ItemAnswered { item: i, .. } | What::ItemLapsed { item: i, .. } if i == item)
        })
        .count()
}

#[test]
fn a_run_started_whose_acknowledgement_was_lost_is_closed_in_this_life_and_counts_as_a_start_of_the_day(
) {
    let limits = Limits {
        starts_per_day: 2,
        ..Limits::PRODUCT
    };
    let c = coordinator(&[MANUAL], limits);
    c.ports().ports.refuse(Method::Log, PortError::NoService);
    assert_eq!(c.control(&agent(0), Action::RunNow), Err(Cause::NoService));
    lose_the_acknowledgement(&c);
    let lost = started(&c).last().cloned().unwrap_or_else(|| {
        events(&c)
            .iter()
            .rev()
            .find_map(|w| match w {
                What::RunStarted { run, .. } => Some(run.clone()),
                _ => None,
            })
            .unwrap()
    });
    let starts = |c: &Coordinator<Both>| {
        events(c)
            .iter()
            .filter(|w| matches!(w, What::RunStarted { .. }))
            .count()
    };
    let before = starts(&c);
    c.ports().ports.set_clock(T0 + 1_000, DAY);
    c.tick();
    let m = mark(&c);
    assert_eq!(c.control(&agent(0), Action::RunNow), Err(Cause::State));
    assert_eq!(
        starts(&c),
        before,
        "no second run_started while the first has no end"
    );
    assert!(!calls_since(&c, m)
        .iter()
        .any(|call| matches!(call, Call::Start(_))));
    assert_eq!(row_line(&c.snapshot(), 0), text("line.waiting_turn.store"));
    assert!(events(&c).contains(&What::RunEnded {
        run: lost.clone(),
        cause: RunEnd::Interrupted,
        defers: None
    }));
    assert!(calls_since(&c, m).contains(&Call::Send(Body::EndRun { run: lost.clone() })));
    c.ports().ports.set_clock(T0 + 2_000, DAY);
    c.tick();
    let next = start(&c, 0);
    assert_ne!(next, lost);
    c.ports().ports.end_runner(&next);
    c.on_run(&next, RunEvent::Ended(Some(Outcome::Completed)))
        .unwrap();
    c.ports().ports.store_end(
        &agent(0),
        PastRun {
            run: next,
            day: DAY,
            outcome: Outcome::Completed,
            unknown: 0,
        },
    );
    c.ports().ports.set_clock(T0 + 3_000, DAY);
    c.tick();
    assert_eq!(
        row_line(&c.snapshot(), 0),
        text(&halvo_coord::types::StatusLine::TriedToday.key()),
        "the lost row and the next start make starts_per_day"
    );
}

#[test]
fn a_lapse_of_an_item_whose_answer_was_stored_with_a_lost_acknowledgement_stores_no_second_closing_row(
) {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    c.on_run(&run, handback(&run, 0, HandbackReason::StepFailed))
        .unwrap();
    let (item, _) = open_item(&c);
    c.ports().ports.refuse(Method::Log, PortError::NoService);
    assert_eq!(
        c.answer(
            &item,
            Choice::Done,
            None,
            shown_step_mark_or_zero(&c, &item)
        ),
        Err(Cause::NoService)
    );
    lose_the_acknowledgement(&c);
    let m = mark(&c);
    assert_eq!(c.control(&agent(0), Action::Stop), Ok(()));
    assert_eq!(closing_rows(&c, &item), 1, "{:?}", events(&c));
    assert_eq!(cancels_of(&c, m, &run), 1);
}

#[test]
fn a_lapse_of_an_own_item_whose_answer_was_stored_with_a_lost_acknowledgement_stores_no_second_closing_row(
) {
    let slotted = support::A {
        person: true,
        slotted: true,
    };
    let c = coordinator(&[slotted], Limits::PRODUCT);
    let (item, kind) = open_item(&c);
    assert_eq!(kind, ItemKind::Ready);
    c.ports().ports.refuse(Method::Log, PortError::NoService);
    assert_eq!(
        c.answer(
            &item,
            Choice::Start,
            None,
            shown_step_mark_or_zero(&c, &item)
        ),
        Err(Cause::NoService)
    );
    lose_the_acknowledgement(&c);
    let _ = c.quit_stop();
    assert_eq!(closing_rows(&c, &item), 1, "{:?}", events(&c));
}

#[test]
fn a_second_answer_after_a_lost_acknowledgement_counts_the_same_choice_and_stores_a_later_decline()
{
    use halvo_coord::types::HandbackResult;
    for order in NO_SERVICE_ROW_UNSEEN_ORDERS {
        for (second, result, rows) in [
            (Choice::Done, HandbackResult::Done, 1),
            (Choice::Decline, HandbackResult::Declined, 2),
        ] {
            let case = format!("{order:?} {second:?}");
            let (c, run, item) = answered_with_a_lost_reply(order);
            let m = mark(&c);
            assert_eq!(
                c.answer(&item, second, None, shown_step_mark_or_zero(&c, &item)),
                Ok(()),
                "{case}"
            );
            assert_eq!(closing_rows(&c, &item), rows, "{case}: {:?}", events(&c));
            let handed: Vec<Call> = calls_since(&c, m)
                .into_iter()
                .filter(|call| matches!(call, Call::AnswerHandback(..)))
                .collect();
            assert_eq!(
                handed,
                [Call::AnswerHandback(run.clone(), result)],
                "{case}: the press's own result, once"
            );
            let s = c.snapshot();
            assert!(s.items.is_empty(), "{case}");
            assert_eq!(row_line(&s, 0), text("line.working.working"), "{case}");
        }
    }
}

#[test]
fn an_item_answered_before_wait_limit_does_not_lapse_at_it() {
    let limits = Limits::PRODUCT;
    let c = coordinator(&[MANUAL], limits);
    let run = start(&c, 0);
    c.on_run(&run, handback(&run, 0, HandbackReason::StepFailed))
        .unwrap();
    let (item, _) = open_item(&c);
    c.ports().ports.set_clock(T0 + 1_000, DAY);
    assert_eq!(
        c.answer(
            &item,
            Choice::Done,
            None,
            shown_step_mark_or_zero(&c, &item)
        ),
        Ok(())
    );
    let m = mark(&c);
    c.ports()
        .ports
        .set_clock(T0 + limits.wait_limit_s * 1000 + 1_000, DAY);
    c.tick();
    assert_eq!(closing_rows(&c, &item), 1, "the answer alone");
    assert_eq!(cancels_of(&c, m, &run), 0);
    assert_eq!(row_line(&c.snapshot(), 0), text("line.working.working"));
}

#[test]
fn after_the_quits_first_step_a_tick_whose_clock_reads_before_a_stop_kills_that_run() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    c.ports().ports.set_clock(T0 + 10_000, DAY);
    assert_eq!(c.control(&agent(0), Action::Stop), Ok(()));
    let _ = c.quit_stop();
    let m = mark(&c);
    c.ports().ports.set_clock(T0 + 1_000, DAY);
    c.tick();
    assert_eq!(
        kills_of(&c, m, &run),
        1,
        "one step of the clock back (RD-7)"
    );
}

#[test]
fn a_switch_off_marks_the_guards_table_and_the_board_before_a_cancel_that_panics() {
    use halvo_coord::run::{drive, recover, Act};
    let limits = Limits::PRODUCT;
    let both = owner(&[MANUAL], limits);
    let mut board = recover(&both, &limits).unwrap().board;
    let mut table = halvo_coord::guard::Table::new();
    let started = drive(
        &both,
        &mut board,
        &mut table,
        &limits,
        Act::Start { agent: agent(0) },
    );
    let run = started.report.run.unwrap();
    both.panics.set(Some("cancel"));
    let off = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        drive(
            &both,
            &mut board,
            &mut table,
            &limits,
            Act::SwitchOff { agent: agent(0) },
        )
    }));
    both.panics.set(None);
    assert!(off.is_err(), "the port's cancel panicked");
    assert!(table.is_stopped(&run));
    let live = board.runs.iter().find(|r| r.run == run).unwrap();
    assert!(live.stop.is_some());
}

/// The two orders in which the coordinator learns that the owner stored an answer whose call
/// answered `no_service`: by its own read right after the call, or by a later refusal `seq`.
const ORDERS: [support::Loss; 2] = [support::Loss::Reply, support::Loss::Later];

const NO_SERVICE_ROW_UNSEEN_ORDERS: [support::Loss; 2] =
    [support::Loss::Later, support::Loss::Unread];

/// A run of agent 0 that waits on a stuck item, which the person answered `done`: the owner
/// stored the row and the call answered `no_service`, in the order `order`.
fn answered_with_a_lost_reply(order: support::Loss) -> (Coordinator<Both>, RunId, ItemId) {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    c.on_run(&run, handback(&run, 0, HandbackReason::StepFailed))
        .unwrap();
    let (item, _) = open_item(&c);
    c.ports().loss.set(Some(order));
    assert_eq!(
        c.answer(
            &item,
            Choice::Done,
            None,
            shown_step_mark_or_zero(&c, &item)
        ),
        Err(Cause::NoService),
        "{order:?}"
    );
    let lost: Vec<halvo_coord::types::Event> = c.ports().lost.borrow_mut().drain(..).collect();
    c.ports().ports.events.borrow_mut().extend(lost);
    assert_eq!(
        closing_rows(&c, &item),
        1,
        "{order:?}: the owner holds the answer"
    );
    (c, run, item)
}

fn handed_back(c: &Coordinator<Both>, m: usize) -> Vec<halvo_coord::types::HandbackResult> {
    calls_since(c, m)
        .into_iter()
        .filter_map(|call| match call {
            Call::AnswerHandback(_, result) => Some(result),
            _ => None,
        })
        .collect()
}

#[test]
fn a_stored_answer_whose_reply_was_lost_counts_for_the_same_answer_again_in_both_orders() {
    use halvo_coord::types::HandbackResult;
    for order in NO_SERVICE_ROW_UNSEEN_ORDERS {
        let (c, _run, item) = answered_with_a_lost_reply(order);
        let m = mark(&c);
        let tried = c.ports().ports.tried.borrow().len();
        assert_eq!(
            c.answer(
                &item,
                Choice::Done,
                None,
                shown_step_mark_or_zero(&c, &item)
            ),
            Ok(()),
            "{order:?}"
        );
        assert_eq!(closing_rows(&c, &item), 1, "{order:?}: one item_answered");
        if order == support::Loss::Reply {
            let refused = c.ports().ports.tried.borrow()[tried..]
                .iter()
                .filter(|t| matches!(t.call, Call::Log(_)) && t.refused.is_some())
                .count();
            assert_eq!(
                refused, 0,
                "the ledger took the row that the read right after the call brought"
            );
        }
        assert_eq!(handed_back(&c, m), [HandbackResult::Done], "{order:?}");
        assert_eq!(
            row_line(&c.snapshot(), 0),
            text("line.working.working"),
            "{order:?}"
        );
    }
}

#[test]
fn a_stored_done_whose_reply_was_lost_then_decline_stores_the_decline_and_sends_declined_in_both_orders(
) {
    use halvo_coord::types::HandbackResult;
    for order in NO_SERVICE_ROW_UNSEEN_ORDERS {
        let (c, _run, item) = answered_with_a_lost_reply(order);
        let m = mark(&c);
        assert_eq!(
            c.answer(
                &item,
                Choice::Decline,
                None,
                shown_step_mark_or_zero(&c, &item)
            ),
            Ok(()),
            "{order:?}"
        );
        assert_eq!(
            closing_rows(&c, &item),
            2,
            "{order:?}: the stored done and the decline"
        );
        assert_eq!(
            handed_back(&c, m),
            [HandbackResult::Declined],
            "{order:?}: the press's own choice"
        );
        assert!(c.snapshot().items.is_empty(), "{order:?}");
    }
}

#[test]
fn a_stop_after_a_stored_answer_whose_reply_was_lost_stores_no_lapse_in_both_orders() {
    for order in NO_SERVICE_ROW_UNSEEN_ORDERS {
        let (c, run, item) = answered_with_a_lost_reply(order);
        let m = mark(&c);
        assert_eq!(c.control(&agent(0), Action::Stop), Ok(()), "{order:?}");
        assert_eq!(
            closing_rows(&c, &item),
            1,
            "{order:?}: no item_lapsed beside the answer"
        );
        assert_eq!(cancels_of(&c, m, &run), 1, "{order:?}");
        assert_eq!(
            row_line(&c.snapshot(), 0),
            text("line.working.stopping"),
            "{order:?}"
        );
        c.ports().ports.end_runner(&run);
        assert_eq!(c.on_run(&run, RunEvent::Ended(None)), Ok(()), "{order:?}");
        assert!(
            events(&c).contains(&What::RunEnded {
                run: run.clone(),
                cause: RunEnd::Cancelled,
                defers: SEEN_WHOLE
            }),
            "{order:?}"
        );
    }
}

#[test]
fn the_runners_end_after_a_stored_answer_whose_reply_was_lost_stores_no_lapse_in_both_orders() {
    for order in NO_SERVICE_ROW_UNSEEN_ORDERS {
        let (c, run, item) = answered_with_a_lost_reply(order);
        c.ports().ports.end_runner(&run);
        assert_eq!(
            c.on_run(&run, RunEvent::Ended(Some(Outcome::HandedBack))),
            Ok(()),
            "{order:?}"
        );
        assert_eq!(
            closing_rows(&c, &item),
            1,
            "{order:?}: no item_lapsed beside the answer"
        );
        assert!(
            events(&c).contains(&What::RunEnded {
                run: run.clone(),
                cause: RunEnd::HandedBack,
                defers: SEEN_WHOLE
            }),
            "{order:?}: the runner's own cause"
        );
        assert_eq!(c.live(), 0, "{order:?}");
    }
}

#[test]
fn the_wait_limit_after_a_stored_answer_whose_reply_was_lost_stores_no_lapse_in_both_orders() {
    use halvo_coord::types::HandbackResult;
    let limits = Limits::PRODUCT;
    for order in NO_SERVICE_ROW_UNSEEN_ORDERS {
        let (c, run, item) = answered_with_a_lost_reply(order);
        let m = mark(&c);
        c.ports()
            .ports
            .set_clock(T0 + limits.wait_limit_s * 1000, DAY);
        c.tick();
        assert_eq!(
            closing_rows(&c, &item),
            1,
            "{order:?}: no item_lapsed beside the answer"
        );
        assert_eq!(
            handed_back(&c, m),
            [HandbackResult::Timeout],
            "{order:?}: the run is timed out"
        );
        assert_eq!(cancels_of(&c, m, &run), 1, "{order:?}");
        let m = mark(&c);
        c.ports().ports.set_clock(
            T0 + limits.wait_limit_s * 1000 + limits.cancel_grace_s * 1000,
            DAY,
        );
        c.tick();
        assert_eq!(kills_of(&c, m, &run), 1, "{order:?}");
        assert!(
            events(&c).contains(&What::RunEnded {
                run: run.clone(),
                cause: RunEnd::Lapsed,
                defers: SEEN_WHOLE
            }),
            "{order:?}"
        );
    }
}

/// What the lossy sequences found: by the kind of the item, the items that end with more than one
/// closing row; the confirmations whose `item_answered` rows stand beside their lapse, which I4.5
/// allows; the second steps, told by their answer row, with two answer rows or with an answer and
/// a lapse; the items of a kind whose choices are decline and done with the rows `done` and then
/// one `decline`, which rulings RD-46 B4 and RD-61 (b) and (d) allow, and the items of a handback
/// with a second row of the same choice, which the clause of RD-61 (c) allows; the Bind moves made
/// and the second steps that they raised; each `answer_bind` with `allow` that did its work, and
/// the ones that broke a property of RD-46 (5) T19; the moves that drew a refused
/// `Runs::answer_handback`, each handback result that did its work inside a press, and the ones
/// that broke RD-61 (h) 4.
#[derive(Debug, Default)]
struct Lossy {
    twice: Vec<ItemKind>,
    confirm_rows: usize,
    second_twice: usize,
    second_lapsed: usize,
    done_decline: usize,
    same_again: usize,
    bind_moves: usize,
    binds_raised: usize,
    second_steps: usize,
    allows: usize,
    handbacks_refused: usize,
    moves: usize,
    handed: usize,
    broken: Vec<String>,
    c4_seeds: Vec<u64>,
    queued_moves: usize,
    off_in_queue: Vec<String>,
}

impl Lossy {
    fn add(&mut self, o: Lossy) {
        self.twice.extend(o.twice);
        self.confirm_rows += o.confirm_rows;
        self.second_twice += o.second_twice;
        self.second_lapsed += o.second_lapsed;
        self.done_decline += o.done_decline;
        self.same_again += o.same_again;
        self.handbacks_refused += o.handbacks_refused;
        self.moves += o.moves;
        self.handed += o.handed;
        self.bind_moves += o.bind_moves;
        self.binds_raised += o.binds_raised;
        self.second_steps += o.second_steps;
        self.allows += o.allows;
        self.broken.extend(o.broken);
        self.c4_seeds.extend(o.c4_seeds);
        self.queued_moves += o.queued_moves;
        self.off_in_queue.extend(o.off_in_queue);
    }
}

/// C4 of I5.12 for the two stored holds (ruling RD-52 (a)): by the owner's events of each agent in
/// the order of `seq`, no `run_started` follows a `hold_set` with the cause `person` or
/// `failures` unless a `hold_cleared` with the same cause stands between them.
fn c4_holds(owners: &[halvo_coord::types::Event]) -> bool {
    use halvo_coord::types::{Cleared, HoldSetCause};
    let mut agents: Vec<&AgentId> = owners.iter().map(|e| &e.agent).collect();
    agents.sort();
    agents.dedup();
    agents.into_iter().all(|a| {
        let mut mine: Vec<&halvo_coord::types::Event> =
            owners.iter().filter(|e| &e.agent == a).collect();
        mine.sort_by_key(|e| e.seq);
        let (mut person, mut failures) = (false, false);
        mine.into_iter().all(|e| {
            match &e.what {
                What::HoldSet {
                    cause: HoldSetCause::Person,
                } => person = true,
                What::HoldSet {
                    cause: HoldSetCause::Failures,
                } => failures = true,
                What::HoldCleared {
                    cause: Cleared::Person,
                } => person = false,
                What::HoldCleared {
                    cause: Cleared::Failures,
                } => failures = false,
                What::RunStarted { .. } => return !(person || failures),
                _ => {}
            }
            true
        })
    })
}

/// The answer rows of the item among `rows`, in their order.
fn answers_of<'a>(rows: &'a [What], item: &ItemId) -> Vec<&'a halvo_coord::types::Answered> {
    rows.iter()
        .filter_map(|w| match w {
            What::ItemAnswered { item: i, answer } if i == item => Some(answer),
            _ => None,
        })
        .collect()
}

fn handback_result_for_choice(
    kind: ItemKind,
    choice: Choice,
) -> Option<halvo_coord::types::HandbackResult> {
    use halvo_coord::types::HandbackResult;
    match (kind, choice) {
        (ItemKind::Check, Choice::TryAgain) | (_, Choice::Done) => Some(HandbackResult::Done),
        (ItemKind::Check, Choice::WentThrough) | (_, Choice::Decline) => {
            Some(HandbackResult::Declined)
        }
        _ => None,
    }
}

fn handed_by_the_newest_row(
    c: &Coordinator<Both>,
    from: usize,
    mv: u64,
    pressed: Option<&(ItemId, Choice, ItemKind)>,
    case: &str,
    out: &mut Lossy,
) {
    use halvo_coord::types::HandbackResult;
    let calls = c.ports().handbacks_at.borrow()[from..].to_vec();
    let all: Vec<What> = c
        .ports()
        .ports
        .events
        .borrow()
        .iter()
        .map(|e| e.what.clone())
        .collect();
    for (_, result, ok, at) in calls {
        if !ok || result == HandbackResult::Timeout {
            continue;
        }
        let Some((item, choice, kind)) = pressed else {
            if !matches!(mv, 2 | 3) {
                out.broken.push(format!(
                    "{case}: {result:?} handed outside a press, move {mv}"
                ));
            }
            continue;
        };
        out.handed += 1;
        let newest = answers_of(&all[..at], item)
            .last()
            .and_then(|a| a.choice())
            .and_then(|ch| handback_result_for_choice(*kind, ch));
        if newest != Some(result) {
            out.broken.push(format!(
                "{case}: {result:?} handed with the newest row giving {newest:?}"
            ));
        }
        let decline_done = choices(*kind) == [Choice::Decline, Choice::Done];
        if decline_done && result == HandbackResult::Done && *choice != Choice::Done {
            out.broken
                .push(format!("{case}: done handed in a press of {choice:?}"));
        }
    }
}

/// One sequence of drawn moves over two agents in which each `log` loses its acknowledgement
/// with chance one in four, in the form `form`; the rows of a `Later` loss land in the owner's
/// events after the move where their `seq` is still free. A drawn move can bring a run's request
/// `Bind` for one of two origins, and an answer draws among the item's own choices. After each
/// move every `answer_bind` with `allow` that did its work is checked: it was made inside a move
/// that answers that item with Allow, the item's newest `item_answered` row was `allow` at the
/// call, and it came after no Stop or switch-off of its run, no lapse of the item and no Don't
/// allow on the item answered ok (ruling RD-46 (5) T19). After each move, too, the queue holds no
/// start of an agent whose hold `off` the board knows (ruling RD-52 (d)); with `max_live` 1 the
/// second agent's start queues while the first runs.
fn lossy_sequence(seed: u64, form: support::Loss, max_live: u32) -> Lossy {
    use halvo_coord::types::{Answered, BindResult, Event, Origin, Raised, SignInAnswer};
    let limits = Limits {
        max_live_runs: max_live,
        ..Limits::PRODUCT
    };
    let mut state = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
    let mut draw = |n: u64| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (state >> 33) % n
    };
    let c = coordinator(&[MANUAL, MANUAL], limits);
    let mut now = T0;
    let mut runs: [Option<RunId>; 2] = [None, None];
    let show = std::env::var("HALVO_LOSSY_SHOW").is_ok();
    let mut moves: Vec<String> = Vec::new();
    let mut out = Lossy::default();
    let mut stopped: Vec<RunId> = Vec::new();
    let mut declined: Vec<ItemId> = Vec::new();
    let run_of = |rows: &[What], item: &ItemId| {
        rows.iter().find_map(|w| match w {
            What::ItemRaised {
                item: i,
                raised: Raised::SignIn(run),
            } if i == item => Some(run.clone()),
            _ => None,
        })
    };
    let check = |c: &Coordinator<Both>,
                 from: usize,
                 pressed: Option<(&ItemId, Choice)>,
                 stopped: &[RunId],
                 declined: &[ItemId],
                 before: &[What],
                 out: &mut Lossy| {
        let calls: Vec<(RunId, BindResult, bool, usize)> =
            c.ports().binds_at.borrow()[from..].to_vec();
        let all: Vec<halvo_coord::types::Event> = c.ports().ports.events.borrow().clone();
        for (run, result, ok, at) in calls {
            if !ok || result != BindResult::Allow {
                continue;
            }
            out.allows += 1;
            let rows: Vec<What> = all[..at].iter().map(|e| e.what.clone()).collect();
            let Some((item, Choice::Allow)) = pressed else {
                out.broken.push(format!(
                    "seed {seed} {form:?}: allow outside a press of Allow"
                ));
                continue;
            };
            if run_of(&rows, item).as_ref() != Some(&run) {
                out.broken.push(format!(
                    "seed {seed} {form:?}: allow to another run than the item's"
                ));
            }
            let newest = answers_of(&rows, item).last().copied().cloned();
            if !matches!(
                &newest,
                Some(Answered::SignIn(SignInAnswer::Second(b))) if b.answer == BindResult::Allow
            ) {
                out.broken.push(format!(
                    "seed {seed} {form:?}: allow with the newest row {newest:?}"
                ));
            }
            let lapsed = before
                .iter()
                .any(|w| matches!(w, What::ItemLapsed { item: i, .. } if i == item));
            if stopped.contains(&run) || declined.contains(item) || lapsed {
                out.broken.push(format!(
                    "seed {seed} {form:?}: allow after a stop, a lapse or a decline"
                ));
            }
        }
    };
    let origins = [
        "https://pay.site1.example:443",
        "https://card.site1.example:443",
    ];
    let mut again: Option<ItemId> = None;
    let mut side = seed.wrapping_mul(0xd1b5_4a32_d192_ed03) | 1;
    let mut refuse_handback = || {
        side = side
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (side >> 33).is_multiple_of(4)
    };
    for _ in 0..18 {
        let lose = draw(4) == 0;
        if lose {
            c.ports().loss.set(Some(form));
        }
        let n = draw(2) as usize;
        let retry = again.take().filter(|_| draw(2) == 0);
        let mv = if retry.is_some() { 5 } else { draw(12) };
        let before = events(&c);
        let from = c.ports().binds_at.borrow().len();
        let handbacks_from = c.ports().handbacks_at.borrow().len();
        let mut pressed: Option<(ItemId, Choice)> = None;
        let mut pressed_handback: Option<(ItemId, Choice, ItemKind)> = None;
        match mv {
            0 | 1 => {
                if c.control(&agent(n), Action::RunNow).is_ok() {
                    runs[n] = started(&c).last().cloned();
                }
            }
            2 | 3 => {
                if let Some(run) = &runs[n] {
                    let reason = [
                        HandbackReason::SignIn,
                        HandbackReason::AccountLink,
                        HandbackReason::StepFailed,
                        HandbackReason::EffectUnknown,
                    ][draw(4) as usize];
                    let _ = c.on_run(run, RunEvent::Hello);
                    let _ = c.on_run(run, handback(run, n, reason));
                }
            }
            4 => {
                if let Some(run) = &runs[n] {
                    let _ = c.on_run(run, RunEvent::Hello);
                    let _ = c.on_run(run, ask_confirm(run, n));
                }
            }
            5 | 6 => {
                let items = c.snapshot().items;
                let pick = match items.iter().position(|i| Some(i.item()) == retry.as_ref()) {
                    Some(k) => k,
                    None => draw(items.len().max(1) as u64) as usize,
                };
                if let Some(item) = items.get(pick) {
                    let id = item.item().clone();
                    let choices: Vec<Choice> = item.choices().iter().map(|t| t.choice).collect();
                    let choice = match item.kind() {
                        ItemKind::Confirm => {
                            c.on_screen(&id);
                            now += limits.arm_ms + 1;
                            c.ports().ports.set_clock(now, DAY);
                            c.tick();
                            [Choice::Allow, Choice::Reject][draw(2) as usize]
                        }
                        ItemKind::Setup => Choice::None,
                        _ => choices[draw(choices.len() as u64) as usize],
                    };
                    let second =
                        choices.contains(&Choice::Allow) && item.kind() == ItemKind::SignIn;
                    let handback_kind = !second
                        && !matches!(
                            item.kind(),
                            ItemKind::Confirm | ItemKind::Setup | ItemKind::Ready
                        );
                    if handback_kind {
                        pressed_handback = Some((id.clone(), choice, item.kind()));
                        if refuse_handback() {
                            out.handbacks_refused += 1;
                            c.ports()
                                .ports
                                .refuse(Method::AnswerHandback, PortError::Busy);
                        }
                    }
                    let answered = c.answer(&id, choice, None, shown_step_mark_or_zero(&c, &id));
                    c.ports().ports.clear_refusals();
                    if second {
                        pressed = Some((id.clone(), choice));
                        match answered {
                            Ok(()) if choice == Choice::Decline => declined.push(id),
                            Ok(()) => {}
                            Err(_) => again = Some(id),
                        }
                    }
                }
            }
            7 => {
                if c.control(&agent(n), Action::Stop).is_ok() {
                    stopped.extend(runs[n].clone());
                }
            }
            8 => {
                if let Some(run) = &runs[n] {
                    c.ports().ports.end_runner(run);
                    let outcome = [Outcome::Completed, Outcome::HandedBack, Outcome::Failed]
                        [draw(3) as usize];
                    let _ = c.on_run(run, RunEvent::Ended(Some(outcome)));
                }
            }
            9 => {
                now += [
                    1_000,
                    limits.cancel_grace_s * 1000,
                    limits.wait_limit_s * 1000,
                ][draw(3) as usize];
                c.ports().ports.set_clock(now, DAY);
                c.tick();
            }
            10 => {
                if let Some(run) = &runs[n] {
                    out.bind_moves += 1;
                    let origin = Origin::parse(origins[draw(2) as usize]).unwrap();
                    let _ = c.on_run(run, RunEvent::Hello);
                    let ask = RunEvent::Bind(halvo_coord::types::BindAsk {
                        run: run.clone(),
                        agent: agent(n),
                        origin,
                    });
                    if c.on_run(run, ask).is_ok() {
                        out.binds_raised += 1;
                    }
                }
            }
            _ => {
                if draw(2) == 0 {
                    if c.control(&agent(n), Action::SwitchOff).is_ok() {
                        stopped.extend(runs[n].clone());
                    }
                } else {
                    let _ = c.control(&agent(n), Action::SwitchOn);
                }
            }
        }
        check(
            &c,
            from,
            pressed.as_ref().map(|(i, ch)| (i, *ch)),
            &stopped,
            &declined,
            &before,
            &mut out,
        );
        let off = c.off_beside_live();
        if !off.is_empty() {
            out.broken.push(format!(
                "seed {seed} {form:?}: move {mv}: off beside a live run that no stop reached"
            ));
        }
        let queued = c.queued();
        if !queued.is_empty() {
            out.queued_moves += 1;
        }
        if queued.iter().any(|(_, off)| *off) {
            out.off_in_queue.push(format!(
                "seed {seed} {form:?} max_live {max_live}: move {mv}: {queued:?}"
            ));
        }
        out.moves += 1;
        handed_by_the_newest_row(
            &c,
            handbacks_from,
            mv,
            pressed_handback.as_ref(),
            &format!("seed {seed} {form:?}"),
            &mut out,
        );
        if show {
            moves.push(format!(
                "move {mv} agent {n} lose {lose}: {:?}",
                &events(&c)[before.len()..]
            ));
        }
        c.ports().loss.set(None);
        let lost: Vec<Event> = c.ports().lost.borrow_mut().drain(..).collect();
        for e in lost {
            let last = c
                .ports()
                .ports
                .events
                .borrow()
                .iter()
                .filter(|o| o.agent == e.agent)
                .map(|o| o.seq.get())
                .max()
                .unwrap_or(0);
            if e.seq.get() == last + 1 {
                c.ports().ports.events.borrow_mut().push(e);
            }
        }
    }
    if draw(3) == 0 {
        let before = events(&c);
        let from = c.ports().binds_at.borrow().len();
        let _ = c.quit_stop();
        c.tick();
        c.quit_kill();
        check(&c, from, None, &stopped, &declined, &before, &mut out);
    }
    let owners: Vec<Event> = c.ports().ports.events.borrow().clone();
    if !c4_holds(&owners) {
        out.c4_seeds.push(seed);
    }
    let stored = events(&c);
    for w in &stored {
        let What::ItemRaised { item, raised } = w else {
            continue;
        };
        let answers = answers_of(&stored, item);
        let answered = answers.len();
        let ended = stored
            .iter()
            .filter(|w| {
                matches!(w, What::ItemLapsed { item: i, .. } | What::ItemReleased { item: i, .. } if i == item)
            })
            .count();
        let second = answers
            .iter()
            .any(|a| matches!(a, Answered::SignIn(SignInAnswer::Second(_))));
        let picks: Vec<Option<Choice>> = answers.iter().map(|a| a.choice()).collect();
        let decline_done = choices(raised.kind()) == [Choice::Decline, Choice::Done];
        let done_decline = decline_done
            && ended == 0
            && picks.len() > 1
            && picks.last() == Some(&Some(Choice::Decline))
            && picks[..picks.len() - 1]
                .iter()
                .all(|p| *p == Some(Choice::Done));
        let same_again = !second
            && !matches!(raised, Raised::Confirm(_))
            && raised.run().is_some()
            && ended == 0
            && picks.len() > 1
            && picks.windows(2).all(|w| w[0] == w[1]);
        let confirm = matches!(raised, Raised::Confirm(_));
        let closing = if confirm || second {
            ended
        } else {
            answered + ended
        };
        if second {
            out.second_steps += 1;
            if answered > 1 {
                out.second_twice += 1;
            }
            if ended > 0 {
                out.second_lapsed += 1;
            }
        }
        if done_decline {
            out.done_decline += 1;
        } else if same_again {
            out.same_again += 1;
        } else if closing > 1 {
            out.twice.push(raised.kind());
            if show {
                println!("seed {seed} {form:?}: {moves:#?}");
            }
        }
        if confirm && answered > 0 && ended > 0 {
            out.confirm_rows += 1;
        }
    }
    out
}

#[test]
fn lossy_sequences_leave_no_item_with_two_closing_rows_and_send_allow_only_by_the_newest_row_in_both_forms(
) {
    let count: u64 = std::env::var("HALVO_LOSSY_SEQUENCES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1_280);
    let mut c4 = Vec::new();
    for (form, max_live) in [
        support::Loss::Reply,
        support::Loss::Later,
        support::Loss::Unread,
    ]
    .into_iter()
    .flat_map(|form| [(form, 2), (form, 1)])
    {
        let mut all = Lossy::default();
        for seed in 0..count {
            all.add(lossy_sequence(seed, form, max_live));
        }
        let mut by_kind: Vec<(ItemKind, usize)> = Vec::new();
        for k in &all.twice {
            match by_kind.iter_mut().find(|(kind, _)| kind == k) {
                Some((_, n)) => *n += 1,
                None => by_kind.push((*k, 1)),
            }
        }
        println!(
            "lossy form={form:?} max_live={max_live} sequences={count} moves_with_a_queued_start={} off_in_queue={}",
            all.queued_moves,
            all.off_in_queue.len()
        );
        assert!(
            all.off_in_queue.is_empty(),
            "{form:?} max_live {max_live}: {:?}",
            &all.off_in_queue[..all.off_in_queue.len().min(8)]
        );
        if max_live == 1 {
            assert!(all.queued_moves > 0, "{form:?}: no start queued");
        }
        println!(
            "lossy form={form:?} sequences={count} items_with_two_closing_rows={} by_kind={by_kind:?} confirmations_answered_and_lapsed={} bind_moves={} second_steps_raised={} second_steps_answered={} second_steps_with_two_answer_rows={} second_steps_answered_and_lapsed={} done_then_decline={} same_choice_again={} allows_sent={} handback_presses_refused={} handback_results_checked={} moves_checked_for_off_beside_live={} broken={}",
            all.twice.len(),
            all.confirm_rows,
            all.bind_moves,
            all.binds_raised,
            all.second_steps,
            all.second_twice,
            all.second_lapsed,
            all.done_decline,
            all.same_again,
            all.allows,
            all.handbacks_refused,
            all.handed,
            all.moves,
            all.broken.len()
        );
        println!(
            "lossy form={form:?} c4_broken={} seeds={:?}",
            all.c4_seeds.len(),
            all.c4_seeds
        );
        assert!(all.twice.is_empty(), "{form:?}: {by_kind:?}");
        c4.push((form, all.c4_seeds.clone()));
        assert!(
            all.broken.is_empty(),
            "{form:?}: {:?}",
            &all.broken[..all.broken.len().min(8)]
        );
        assert!(
            all.binds_raised > 0 && all.allows > 0 && all.second_twice > 0,
            "{form:?}: {all:?}"
        );
        assert!(
            all.handbacks_refused > 0 && all.handed > 0 && all.done_decline > 0,
            "{form:?}: {all:?}"
        );
    }
    assert!(
        c4.iter().all(|(_, seeds)| seeds.is_empty()),
        "C4 broken: {c4:?}"
    );
}

#[test]
fn a_switch_off_whose_lapse_lost_its_reply_leaves_one_closing_row_at_the_runners_end() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    c.on_run(&run, handback(&run, 0, HandbackReason::EffectUnknown))
        .unwrap();
    let (item, _) = open_item(&c);
    c.ports().loss.set(Some(support::Loss::Reply));
    assert_eq!(c.control(&agent(0), Action::SwitchOff), Ok(()));
    c.ports().ports.end_runner(&run);
    assert_eq!(
        c.on_run(&run, RunEvent::Ended(Some(Outcome::HandedBack))),
        Ok(())
    );
    assert_eq!(closing_rows(&c, &item), 1);
}

#[test]
fn a_stop_after_an_answer_whose_row_was_stored_and_whose_handback_failed_stores_no_lapse() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    c.on_run(&run, handback(&run, 0, HandbackReason::StepFailed))
        .unwrap();
    let (item, _) = open_item(&c);
    c.ports()
        .ports
        .refuse(Method::AnswerHandback, PortError::NoService);
    assert_eq!(
        c.answer(
            &item,
            Choice::Done,
            None,
            shown_step_mark_or_zero(&c, &item)
        ),
        Err(Cause::NoService)
    );
    c.ports().ports.clear_refusals();
    assert_eq!(
        closing_rows(&c, &item),
        1,
        "the answer is stored and acknowledged"
    );
    let m = mark(&c);
    assert_eq!(c.control(&agent(0), Action::Stop), Ok(()));
    assert_eq!(
        closing_rows(&c, &item),
        1,
        "no item_lapsed beside the stored answer"
    );
    assert_eq!(cancels_of(&c, m, &run), 1);
}

/// The person's Stop for `who`, which has no live run: answers ok, makes the calls `calls` alone,
/// stores no event, and leaves the rows and the items of the snapshot as they were (ruling
/// RD-43 (a) to (c)). The hold's check card takes the unknown effects that it shows from the
/// facts of the move's own read of the roster and makes no read of its own (ruling RD-81 8.3).
fn a_stop_changes_nothing(c: &Coordinator<Both>, who: &AgentId, calls: &[Call], case: &str) {
    let before = c.snapshot();
    let stored = events(c).len();
    let m = mark(c);
    assert_eq!(c.control(who, Action::Stop), Ok(()), "{case}");
    assert_eq!(calls_since(c, m), calls, "{case}: the calls of the control");
    assert_eq!(events(c).len(), stored, "{case}: no event stored");
    let after = c.snapshot();
    let ids = |s: &Snapshot| s.items.iter().map(|i| i.item().clone()).collect::<Vec<_>>();
    assert_eq!(ids(&after), ids(&before), "{case}: the same item");
    assert_eq!(after.rows, before.rows, "{case}: the same rows");
}

#[test]
fn the_persons_stop_of_an_agent_with_no_live_run_or_a_stopping_one_lapses_nothing_and_stores_nothing(
) {
    let limits = Limits::PRODUCT;
    let c = coordinator(&[MANUAL], limits);
    let run = start(&c, 0);
    c.ports().ports.end_runner(&run);
    c.on_run(&run, RunEvent::Ended(Some(Outcome::Completed)))
        .unwrap();
    c.ports().ports.store_end(
        &agent(0),
        PastRun {
            run,
            day: DAY,
            outcome: Outcome::Completed,
            unknown: 1,
        },
    );
    c.ports().ports.set_clock(T0 + 1_000, DAY);
    c.tick();
    assert_eq!(open_item(&c).1, ItemKind::Check, "the hold's check");
    a_stop_changes_nothing(&c, &agent(0), &[], "the hold's check");

    let (c, made, _) = setup_agent();
    assert_eq!(open_item(&c).1, ItemKind::Setup);
    a_stop_changes_nothing(&c, &made, &[], "a setup item");

    let slotted = support::A {
        person: true,
        slotted: true,
    };
    let c = coordinator(&[slotted], limits);
    assert_eq!(open_item(&c).1, ItemKind::Ready);
    a_stop_changes_nothing(&c, &agent(0), &[], "a ready item");

    let one = Limits {
        max_live_runs: 1,
        ..limits
    };
    let c = coordinator(&[MANUAL, slotted], one);
    let (ready, kind) = open_item(&c);
    assert_eq!(kind, ItemKind::Ready);
    let first = start(&c, 0);
    c.ports().ports.refuse(Method::Log, PortError::NoService);
    assert_eq!(
        c.answer(
            &ready,
            Choice::Start,
            None,
            shown_step_mark_or_zero(&c, &ready)
        ),
        Ok(())
    );
    c.ports().ports.clear_refusals();
    assert_eq!(started(&c).len(), 1, "agent 1's start waits in the queue");
    assert_eq!(
        c.snapshot()
            .items
            .iter()
            .map(|i| i.item().clone())
            .collect::<Vec<_>>(),
        std::slice::from_ref(&ready),
        "the queued agent holds its ready item"
    );
    // Turned by ruling RD-133, Addition 1 item 5: the Stop takes the queued start out of the
    // queue, stores nothing and keeps the ready item; the next end takes no start of it.
    let stored = events(&c).len();
    assert_eq!(c.control(&agent(1), Action::Stop), Ok(()));
    assert_eq!(events(&c).len(), stored, "a queued agent: nothing stored");
    assert_eq!(c.queued(), vec![], "a queued agent: out of the queue");
    assert_eq!(
        c.snapshot()
            .items
            .iter()
            .map(|i| i.item().clone())
            .collect::<Vec<_>>(),
        std::slice::from_ref(&ready),
        "the ready item stays"
    );
    c.ports().ports.end_runner(&first);
    c.on_run(&first, RunEvent::Ended(Some(Outcome::Completed)))
        .unwrap();
    assert_eq!(started(&c).len(), 1, "the next end takes no start of it");

    let c = coordinator(&[MANUAL], limits);
    let run = start(&c, 0);
    assert_eq!(c.control(&agent(0), Action::Stop), Ok(()));
    let stored = events(&c).len();
    let m = mark(&c);
    assert_eq!(c.control(&agent(0), Action::Stop), Ok(()), "a stopping run");
    assert_eq!(cancels_of(&c, m, &run), 0, "no second cancel");
    assert_eq!(calls_since(&c, m), Vec::<Call>::new());
    assert_eq!(events(&c).len(), stored, "nothing stored");
}

/// How the person's first press of a second step went: lost in one of RD-40's two forms, or
/// stored and acknowledged with `Runs::answer_bind` failing `busy` after it (RD-46's S).
#[derive(Clone, Copy, Debug)]
enum First {
    Lost(support::Loss),
    Taken,
}

/// A run of agent 0 that waits on a second step for the origin of `bind`.
fn second_step() -> (Coordinator<Both>, RunId, ItemId) {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    c.on_run(&run, bind(&run, 0)).unwrap();
    let (item, kind) = open_item(&c);
    assert_eq!(kind, ItemKind::SignIn);
    (c, run, item)
}

fn first_press(c: &Coordinator<Both>, item: &ItemId, choice: Choice, how: First) {
    match how {
        First::Lost(order) => {
            c.ports().loss.set(Some(order));
            assert_eq!(
                c.answer(item, choice, None, shown_step_mark_or_zero(c, item)),
                Err(Cause::NoService),
                "{how:?}"
            );
            let lost: Vec<halvo_coord::types::Event> =
                c.ports().lost.borrow_mut().drain(..).collect();
            c.ports().ports.events.borrow_mut().extend(lost);
        }
        First::Taken => {
            c.ports().ports.refuse(Method::AnswerBind, PortError::Busy);
            assert_eq!(
                c.answer(item, choice, None, shown_step_mark_or_zero(c, item)),
                Err(Cause::Busy),
                "{how:?}"
            );
            c.ports().ports.clear_refusals();
        }
    }
}

/// The owner's rows for the item after its `item_raised`, as words: the answer of a second step
/// or of an account link, or `lapsed:<cause>`.
fn step_rows(c: &Coordinator<Both>, item: &ItemId) -> Vec<String> {
    use halvo_coord::types::{Answered, SignInAnswer};
    events(c)
        .iter()
        .filter_map(|w| match w {
            What::ItemAnswered { item: i, answer } if i == item => Some(match answer {
                Answered::SignIn(SignInAnswer::Second(b)) => b.answer.name().to_owned(),
                Answered::LinkAccount(l) => l.answer.name().to_owned(),
                other => format!("{other:?}"),
            }),
            What::ItemLapsed { item: i, lapsed } if i == item => {
                Some(format!("lapsed:{}", lapsed.cause().name()))
            }
            _ => None,
        })
        .collect()
}

fn allows(c: &Coordinator<Both>) -> usize {
    bind_answers(c)
        .iter()
        .filter(|(_, r)| *r == halvo_coord::types::BindResult::Allow)
        .count()
}

fn binds_since(c: &Coordinator<Both>, m: usize) -> Vec<halvo_coord::types::BindResult> {
    calls_since(c, m)
        .into_iter()
        .filter_map(|call| match call {
            Call::AnswerBind(_, r) => Some(r),
            _ => None,
        })
        .collect()
}

fn decline_before_one_cancel(c: &Coordinator<Both>, m: usize, run: &RunId, case: &str) {
    use halvo_coord::types::BindResult;
    let calls = calls_since(c, m);
    let decline = calls
        .iter()
        .position(|call| matches!(call, Call::AnswerBind(r, BindResult::Decline) if r == run));
    let cancel = calls
        .iter()
        .position(|call| matches!(call, Call::Cancel(r) if r == run));
    assert!(
        decline.is_some() && decline < cancel,
        "{case}: decline before the cancel: {calls:?}"
    );
    assert_eq!(cancels_of(c, m, run), 1, "{case}");
}

#[test]
fn a_lost_allow_then_allow_sends_allow_once_in_the_second_move_in_both_forms() {
    use halvo_coord::types::BindResult;
    for order in ORDERS {
        let (c, _run, item) = second_step();
        first_press(&c, &item, Choice::Allow, First::Lost(order));
        assert_eq!(allows(&c), 0, "{order:?}");
        let m = mark(&c);
        assert_eq!(
            c.answer(
                &item,
                Choice::Allow,
                None,
                shown_step_mark_or_zero(&c, &item)
            ),
            Ok(()),
            "{order:?}"
        );
        assert_eq!(step_rows(&c, &item), ["allow", "allow"], "{order:?}");
        assert_eq!(binds_since(&c, m), [BindResult::Allow], "{order:?}");
        assert_eq!(allows(&c), 1, "{order:?}");
    }
}

#[test]
fn a_lost_allow_then_dont_allow_sends_decline_and_never_allow_in_both_forms() {
    use halvo_coord::types::BindResult;
    for order in ORDERS {
        let (c, run, item) = second_step();
        first_press(&c, &item, Choice::Allow, First::Lost(order));
        assert_eq!(
            c.answer(
                &item,
                Choice::Decline,
                None,
                shown_step_mark_or_zero(&c, &item)
            ),
            Ok(()),
            "{order:?}"
        );
        assert_eq!(step_rows(&c, &item), ["allow", "decline"], "{order:?}");
        assert_eq!(
            bind_answers(&c).iter().map(|(_, r)| *r).collect::<Vec<_>>(),
            [BindResult::Decline],
            "{order:?}"
        );
        c.ports().ports.end_runner(&run);
        assert_eq!(
            c.on_run(&run, RunEvent::Ended(Some(Outcome::Declined))),
            Ok(()),
            "{order:?}"
        );
        assert_eq!(run_ends_of(&c, &run), [RunEnd::Declined], "{order:?}");
        assert_eq!(
            step_rows(&c, &item),
            ["allow", "decline"],
            "{order:?}: no lapse at the end"
        );
    }
}

#[test]
fn a_lost_decline_then_the_persons_last_press_decides_in_both_forms() {
    use halvo_coord::types::BindResult;
    for order in ORDERS {
        let (c, _run, item) = second_step();
        first_press(&c, &item, Choice::Decline, First::Lost(order));
        assert_eq!(
            c.answer(
                &item,
                Choice::Allow,
                None,
                shown_step_mark_or_zero(&c, &item)
            ),
            Ok(()),
            "{order:?}"
        );
        assert_eq!(step_rows(&c, &item), ["decline", "allow"], "{order:?}");
        assert_eq!(allows(&c), 1, "{order:?}: by the person's last press");
        let (c, _run, item) = second_step();
        first_press(&c, &item, Choice::Decline, First::Lost(order));
        assert_eq!(
            c.answer(
                &item,
                Choice::Decline,
                None,
                shown_step_mark_or_zero(&c, &item)
            ),
            Ok(()),
            "{order:?}"
        );
        assert_eq!(step_rows(&c, &item), ["decline", "decline"], "{order:?}");
        assert_eq!(
            bind_answers(&c).iter().map(|(_, r)| *r).collect::<Vec<_>>(),
            [BindResult::Decline],
            "{order:?}"
        );
    }
}

#[test]
fn a_lost_allow_then_the_stop_sends_decline_before_one_cancel_and_lapses_cancelled_in_both_forms() {
    for order in ORDERS {
        let (c, run, item) = second_step();
        first_press(&c, &item, Choice::Allow, First::Lost(order));
        let m = mark(&c);
        assert_eq!(c.control(&agent(0), Action::Stop), Ok(()), "{order:?}");
        decline_before_one_cancel(&c, m, &run, &format!("{order:?}"));
        assert_eq!(
            step_rows(&c, &item),
            ["allow", "lapsed:cancelled"],
            "{order:?}"
        );
        c.ports().ports.end_runner(&run);
        assert_eq!(c.on_run(&run, RunEvent::Ended(None)), Ok(()), "{order:?}");
        assert!(events(&c).contains(&What::RunEnded {
            run: run.clone(),
            cause: RunEnd::Cancelled,
            defers: SEEN_WHOLE
        }));
        assert_eq!(allows(&c), 0, "{order:?}");
    }
}

#[test]
fn a_lost_allow_then_the_wait_limit_sends_decline_and_the_run_ends_lapsed_in_both_forms() {
    use halvo_coord::types::BindResult;
    let limits = Limits::PRODUCT;
    for order in ORDERS {
        let (c, run, item) = second_step();
        first_press(&c, &item, Choice::Allow, First::Lost(order));
        let m = mark(&c);
        c.ports()
            .ports
            .set_clock(T0 + limits.wait_limit_s * 1000, DAY);
        c.tick();
        assert_eq!(binds_since(&c, m), [BindResult::Decline], "{order:?}");
        assert_eq!(
            step_rows(&c, &item),
            ["allow", "lapsed:wait_limit"],
            "{order:?}"
        );
        c.ports().ports.set_clock(
            T0 + limits.wait_limit_s * 1000 + limits.cancel_grace_s * 1000,
            DAY,
        );
        c.tick();
        assert!(events(&c).contains(&What::RunEnded {
            run: run.clone(),
            cause: RunEnd::Lapsed,
            defers: SEEN_WHOLE
        }));
        assert_eq!(allows(&c), 0, "{order:?}");
    }
}

fn run_ends_of(c: &Coordinator<Both>, run: &RunId) -> Vec<RunEnd> {
    events(c)
        .iter()
        .filter_map(|w| match w {
            What::RunEnded {
                run: r,
                cause,
                defers: _,
            } if r == run => Some(*cause),
            _ => None,
        })
        .collect()
}

#[test]
fn a_lost_allow_then_the_quit_lapses_app_ended_and_the_next_life_adds_interrupted_alone_in_both_forms(
) {
    for order in ORDERS {
        let (c, run, item) = second_step();
        first_press(&c, &item, Choice::Allow, First::Lost(order));
        let m = mark(&c);
        let _ = c.quit_stop();
        assert_eq!(
            step_rows(&c, &item),
            ["allow", "lapsed:app_ended"],
            "{order:?}"
        );
        assert!(binds_since(&c, m).is_empty(), "{order:?}");
        c.quit_kill();
        let life = next_life(&c, |_| {});
        assert_eq!(
            step_rows(&life, &item),
            ["allow", "lapsed:app_ended"],
            "{order:?}"
        );
        assert_eq!(run_ends_of(&life, &run), [RunEnd::Interrupted], "{order:?}");
        assert!(bind_answers(&life).is_empty(), "{order:?}");
    }
}

#[test]
fn a_kill_after_a_lost_or_taken_allow_binds_nothing_lapses_nothing_and_a_new_bind_raises_a_new_item(
) {
    let forms = [
        First::Lost(support::Loss::Reply),
        First::Lost(support::Loss::Later),
        First::Taken,
    ];
    for how in forms {
        let (c, run, item) = second_step();
        first_press(&c, &item, Choice::Allow, how);
        let life = next_life(&c, |_| {});
        assert!(life.snapshot().items.is_empty(), "{how:?}: no item");
        assert!(
            bind_answers(&life).is_empty(),
            "{how:?}: no answer_bind call"
        );
        assert_eq!(
            step_rows(&life, &item),
            ["allow"],
            "{how:?}: no item_lapsed"
        );
        assert_eq!(run_ends_of(&life, &run), [RunEnd::Interrupted], "{how:?}");
        life.ports().ports.set_clock(T0 + 1_000, DAY);
        life.tick();
        let next = start(&life, 0);
        life.on_run(&next, bind(&next, 0)).unwrap();
        let (fresh, kind) = open_item(&life);
        assert_eq!(kind, ItemKind::SignIn, "{how:?}");
        assert_ne!(fresh, item, "{how:?}: a new item id");
    }
}

#[test]
fn a_stored_allow_that_the_run_did_not_take_then_allow_sends_allow_once_with_one_row() {
    use halvo_coord::types::BindResult;
    let (c, _run, item) = second_step();
    first_press(&c, &item, Choice::Allow, First::Taken);
    let m = mark(&c);
    assert_eq!(
        c.answer(
            &item,
            Choice::Allow,
            None,
            shown_step_mark_or_zero(&c, &item)
        ),
        Ok(())
    );
    assert_eq!(step_rows(&c, &item), ["allow"]);
    assert_eq!(binds_since(&c, m), [BindResult::Allow]);
    assert_eq!(allows(&c), 1);
}

#[test]
fn a_stored_allow_that_the_run_did_not_take_then_dont_allow_stores_and_sends_decline() {
    use halvo_coord::types::BindResult;
    let (c, run, item) = second_step();
    first_press(&c, &item, Choice::Allow, First::Taken);
    let m = mark(&c);
    assert_eq!(
        c.answer(
            &item,
            Choice::Decline,
            None,
            shown_step_mark_or_zero(&c, &item)
        ),
        Ok(())
    );
    assert_eq!(step_rows(&c, &item), ["allow", "decline"]);
    assert_eq!(binds_since(&c, m), [BindResult::Decline]);
    assert_eq!(allows(&c), 0);
    c.ports().ports.end_runner(&run);
    assert_eq!(
        c.on_run(&run, RunEvent::Ended(Some(Outcome::Declined))),
        Ok(())
    );
    assert_eq!(run_ends_of(&c, &run), [RunEnd::Declined]);
    assert_eq!(
        step_rows(&c, &item),
        ["allow", "decline"],
        "no lapse at the end"
    );
    let (c, _run, item) = second_step();
    first_press(&c, &item, Choice::Allow, First::Taken);
    c.ports().ports.refuse(Method::Log, PortError::NoService);
    let m = mark(&c);
    assert_eq!(
        c.answer(
            &item,
            Choice::Decline,
            None,
            shown_step_mark_or_zero(&c, &item)
        ),
        Err(Cause::NoService)
    );
    c.ports().ports.clear_refusals();
    assert!(binds_since(&c, m).is_empty(), "nothing is sent");
    assert_eq!(step_rows(&c, &item), ["allow"]);
    let m = mark(&c);
    assert_eq!(
        c.answer(
            &item,
            Choice::Allow,
            None,
            shown_step_mark_or_zero(&c, &item)
        ),
        Ok(())
    );
    assert_eq!(
        step_rows(&c, &item),
        ["allow", "allow"],
        "the step is open again: the press stores a row of its own"
    );
    assert_eq!(binds_since(&c, m), [BindResult::Allow]);
}

#[test]
fn a_stored_decline_that_the_run_did_not_take_refuses_allow_and_sends_decline_again() {
    use halvo_coord::types::BindResult;
    let (c, _run, item) = second_step();
    first_press(&c, &item, Choice::Decline, First::Taken);
    let m = mark(&c);
    assert_eq!(
        c.answer(
            &item,
            Choice::Allow,
            None,
            shown_step_mark_or_zero(&c, &item)
        ),
        Err(Cause::State)
    );
    assert!(binds_since(&c, m).is_empty());
    assert_eq!(step_rows(&c, &item), ["decline"]);
    assert_eq!(
        c.answer(
            &item,
            Choice::Decline,
            None,
            shown_step_mark_or_zero(&c, &item)
        ),
        Ok(())
    );
    assert_eq!(step_rows(&c, &item), ["decline"]);
    assert_eq!(binds_since(&c, m), [BindResult::Decline]);
}

#[test]
fn a_stored_allow_that_the_run_did_not_take_is_sent_by_no_tick_or_read_and_lapses_at_the_wait_limit_with_decline(
) {
    use halvo_coord::types::BindResult;
    let limits = Limits::PRODUCT;
    let (c, run, item) = second_step();
    first_press(&c, &item, Choice::Allow, First::Taken);
    let m = mark(&c);
    let stored = events(&c).len();
    for at in [1_000, limits.wait_limit_s * 1000 - 1] {
        c.ports().ports.set_clock(T0 + at, DAY);
        c.tick();
        let _ = c.snapshot();
        let _ = c.same();
        let _ = c.next(0);
        let _ = c.card(&agent(0));
    }
    assert!(
        binds_since(&c, m).is_empty(),
        "no tick and no read before the limit sends anything"
    );
    assert_eq!(events(&c).len(), stored, "and stores nothing");
    c.ports()
        .ports
        .set_clock(T0 + limits.wait_limit_s * 1000, DAY);
    c.tick();
    assert_eq!(binds_since(&c, m), [BindResult::Decline]);
    assert_eq!(step_rows(&c, &item), ["allow", "lapsed:wait_limit"]);
    c.ports().ports.set_clock(
        T0 + limits.wait_limit_s * 1000 + limits.cancel_grace_s * 1000,
        DAY,
    );
    c.tick();
    assert_eq!(run_ends_of(&c, &run), [RunEnd::Lapsed]);
    assert_eq!(allows(&c), 0);
}

#[test]
fn the_stop_of_a_stored_allow_sends_decline_before_one_cancel_and_holds_no_step_after_the_end_the_kill_or_a_lost_run(
) {
    let limits = Limits::PRODUCT;
    for (way, end) in [
        ("end", RunEnd::Cancelled),
        ("grace", RunEnd::Cancelled),
        ("lost", RunEnd::Interrupted),
    ] {
        let (c, run, item) = second_step();
        first_press(&c, &item, Choice::Allow, First::Taken);
        let m = mark(&c);
        assert_eq!(c.control(&agent(0), Action::Stop), Ok(()), "{way}");
        decline_before_one_cancel(&c, m, &run, way);
        assert_eq!(step_rows(&c, &item), ["allow", "lapsed:cancelled"], "{way}");
        match way {
            "end" => {
                c.ports().ports.end_runner(&run);
                assert_eq!(c.on_run(&run, RunEvent::Ended(None)), Ok(()));
            }
            _ => {
                if way == "lost" {
                    c.ports().ports.new_life();
                }
                c.ports()
                    .ports
                    .set_clock(T0 + limits.cancel_grace_s * 1000, DAY);
                c.tick();
            }
        }
        assert_eq!(run_ends_of(&c, &run), [end], "{way}");
        assert_eq!(c.live(), 0, "{way}");
        assert!(c.snapshot().items.is_empty(), "{way}");
        assert_eq!(
            c.answer(
                &item,
                Choice::Allow,
                None,
                shown_step_mark_or_zero(&c, &item)
            ),
            Err(Cause::UnknownItem),
            "{way}: no step held"
        );
        assert_eq!(step_rows(&c, &item), ["allow", "lapsed:cancelled"], "{way}");
        assert_eq!(allows(&c), 0, "{way}");
    }
}

#[test]
fn a_stored_allow_lapses_with_run_ended_at_the_runners_end_and_with_app_ended_at_the_quit() {
    let (c, run, item) = second_step();
    first_press(&c, &item, Choice::Allow, First::Taken);
    c.ports().ports.end_runner(&run);
    assert_eq!(
        c.on_run(&run, RunEvent::Ended(Some(Outcome::Completed))),
        Ok(())
    );
    assert_eq!(step_rows(&c, &item), ["allow", "lapsed:run_ended"]);
    assert_eq!(run_ends_of(&c, &run), [RunEnd::Completed]);
    let (c, _run, item) = second_step();
    first_press(&c, &item, Choice::Allow, First::Taken);
    let m = mark(&c);
    let _ = c.quit_stop();
    assert_eq!(step_rows(&c, &item), ["allow", "lapsed:app_ended"]);
    assert!(binds_since(&c, m).is_empty());
}

#[test]
fn a_quit_whose_lapse_got_no_acknowledgement_leaves_the_rows_of_a_kill() {
    let (c, run, item) = second_step();
    first_press(&c, &item, Choice::Allow, First::Taken);
    c.ports().ports.refuse(Method::Log, PortError::NoService);
    let _ = c.quit_stop();
    c.quit_kill();
    c.ports().ports.clear_refusals();
    let life = next_life(&c, |_| {});
    assert_eq!(step_rows(&life, &item), ["allow"]);
    assert_eq!(run_ends_of(&life, &run), [RunEnd::Interrupted]);
    assert!(bind_answers(&life).is_empty());
}

#[test]
fn an_allow_that_answer_bind_refuses_with_ended_answers_ok_and_stores_no_lapse() {
    let (c, run, item) = second_step();
    c.ports().ports.end_runner(&run);
    assert_eq!(
        c.answer(
            &item,
            Choice::Allow,
            None,
            shown_step_mark_or_zero(&c, &item)
        ),
        Ok(())
    );
    assert_eq!(step_rows(&c, &item), ["allow"]);
    assert_eq!(
        c.on_run(&run, RunEvent::Ended(Some(Outcome::Completed))),
        Ok(())
    );
    assert_eq!(step_rows(&c, &item), ["allow"]);
    assert_eq!(run_ends_of(&c, &run), [RunEnd::Completed]);
    assert_eq!(allows(&c), 0);
}

#[test]
fn a_second_step_answered_after_the_runs_end_is_unknown_item() {
    let (c, run, item) = second_step();
    c.ports().ports.end_runner(&run);
    assert_eq!(c.on_run(&run, RunEvent::Ended(None)), Ok(()));
    assert_eq!(
        c.answer(
            &item,
            Choice::Allow,
            None,
            shown_step_mark_or_zero(&c, &item)
        ),
        Err(Cause::UnknownItem)
    );
    assert_eq!(step_rows(&c, &item), ["lapsed:run_ended"]);
}

#[test]
fn two_second_steps_of_one_run_keep_their_own_rows_and_a_second_bind_ends_the_run_with_kind() {
    use halvo_coord::types::BindResult;
    let (c, run, a) = second_step();
    first_press(&c, &a, Choice::Allow, First::Lost(support::Loss::Reply));
    assert_eq!(
        c.answer(&a, Choice::Allow, None, shown_step_mark_or_zero(&c, &a)),
        Ok(())
    );
    let other = RunEvent::Bind(halvo_coord::types::BindAsk {
        run: run.clone(),
        agent: agent(0),
        origin: halvo_coord::types::Origin::parse("https://card.site1.example:443").unwrap(),
    });
    c.on_run(&run, other).unwrap();
    let (b, _) = open_item(&c);
    assert_ne!(a, b);
    assert_eq!(
        c.answer(&b, Choice::Decline, None, shown_step_mark_or_zero(&c, &b)),
        Ok(())
    );
    assert_eq!(step_rows(&c, &a), ["allow", "allow"]);
    assert_eq!(step_rows(&c, &b), ["decline"]);
    assert_eq!(
        bind_answers(&c).iter().map(|(_, r)| *r).collect::<Vec<_>>(),
        [BindResult::Allow, BindResult::Decline]
    );
    let (c, run, a) = second_step();
    let m = mark(&c);
    assert_eq!(c.on_run(&run, bind(&run, 0)), Err(Cause::State));
    assert_eq!(step_rows(&c, &a), ["lapsed:run_ended"]);
    assert!(binds_since(&c, m).is_empty());
    assert!(c.snapshot().items.is_empty());
    c.ports().ports.end_runner(&run);
    assert_eq!(c.on_run(&run, RunEvent::Ended(None)), Ok(()));
    assert_eq!(run_ends_of(&c, &run), [RunEnd::Kind]);
    assert_eq!(step_rows(&c, &a), ["lapsed:run_ended"]);
}

/// A second step whose allow the run took, then the way `way`: the run's end, the person's Stop,
/// or the quit; the item gets no `item_lapsed` (ruling RD-46 (5) T18).
fn a_taken_allow_then(way: &str) {
    let (c, run, item) = second_step();
    assert_eq!(
        c.answer(
            &item,
            Choice::Allow,
            None,
            shown_step_mark_or_zero(&c, &item)
        ),
        Ok(()),
        "{way}"
    );
    match way {
        "end" => {
            c.ports().ports.end_runner(&run);
            let _ = c.on_run(&run, RunEvent::Ended(Some(Outcome::Completed)));
        }
        "stop" => {
            let _ = c.control(&agent(0), Action::Stop);
            c.ports().ports.end_runner(&run);
            let _ = c.on_run(&run, RunEvent::Ended(None));
        }
        _ => {
            let _ = c.quit_stop();
            c.quit_kill();
        }
    }
    assert_eq!(step_rows(&c, &item), ["allow"], "{way}");
}

#[test]
fn a_taken_allow_gets_no_lapse_at_the_runs_end() {
    a_taken_allow_then("end");
}

#[test]
fn a_taken_allow_gets_no_lapse_at_the_persons_stop() {
    a_taken_allow_then("stop");
}

#[test]
fn a_taken_allow_gets_no_lapse_at_the_quit() {
    a_taken_allow_then("quit");
}

#[test]
fn a_link_accounts_lost_done_then_the_declining_choice_sends_declined_and_never_done_in_both_forms()
{
    use halvo_coord::types::HandbackResult;
    for order in NO_SERVICE_ROW_UNSEEN_ORDERS {
        let c = coordinator(&[MANUAL], Limits::PRODUCT);
        let run = start(&c, 0);
        c.on_run(&run, handback(&run, 0, HandbackReason::AccountLink))
            .unwrap();
        let (item, _) = open_item(&c);
        c.ports().loss.set(Some(order));
        assert_eq!(
            c.answer(
                &item,
                Choice::Done,
                None,
                shown_step_mark_or_zero(&c, &item)
            ),
            Err(Cause::NoService),
            "{order:?}"
        );
        let lost: Vec<halvo_coord::types::Event> = c.ports().lost.borrow_mut().drain(..).collect();
        c.ports().ports.events.borrow_mut().extend(lost);
        let m = mark(&c);
        assert_eq!(
            c.answer(
                &item,
                Choice::Decline,
                None,
                shown_step_mark_or_zero(&c, &item)
            ),
            Ok(()),
            "{order:?}"
        );
        assert_eq!(handed_back(&c, m), [HandbackResult::Declined], "{order:?}");
        assert_eq!(step_rows(&c, &item), ["done", "decline"], "{order:?}");
        let (c2, item2) = {
            let c = coordinator(&[MANUAL], Limits::PRODUCT);
            let run = start(&c, 0);
            c.on_run(&run, handback(&run, 0, HandbackReason::AccountLink))
                .unwrap();
            let (item, _) = open_item(&c);
            (c, item)
        };
        c2.ports().loss.set(Some(order));
        assert_eq!(
            c2.answer(
                &item2,
                Choice::Done,
                None,
                shown_step_mark_or_zero(&c2, &item2)
            ),
            Err(Cause::NoService)
        );
        let lost: Vec<halvo_coord::types::Event> = c2.ports().lost.borrow_mut().drain(..).collect();
        c2.ports().ports.events.borrow_mut().extend(lost);
        let m = mark(&c2);
        assert_eq!(
            c2.answer(
                &item2,
                Choice::Done,
                None,
                shown_step_mark_or_zero(&c2, &item2)
            ),
            Ok(()),
            "{order:?}"
        );
        assert_eq!(handed_back(&c2, m), [HandbackResult::Done], "{order:?}");
        assert_eq!(step_rows(&c2, &item2), ["done"], "{order:?}");
        let c3 = coordinator(&[MANUAL], Limits::PRODUCT);
        let run = start(&c3, 0);
        c3.on_run(&run, handback(&run, 0, HandbackReason::AccountLink))
            .unwrap();
        let (item3, _) = open_item(&c3);
        c3.ports().loss.set(Some(order));
        assert_eq!(
            c3.answer(
                &item3,
                Choice::Done,
                None,
                shown_step_mark_or_zero(&c3, &item3)
            ),
            Err(Cause::NoService)
        );
        let lost: Vec<halvo_coord::types::Event> = c3.ports().lost.borrow_mut().drain(..).collect();
        c3.ports().ports.events.borrow_mut().extend(lost);
        c3.ports()
            .ports
            .refuse(Method::AnswerHandback, PortError::Busy);
        assert_eq!(
            c3.answer(
                &item3,
                Choice::Decline,
                None,
                shown_step_mark_or_zero(&c3, &item3)
            ),
            Err(Cause::Busy),
            "{order:?}"
        );
        c3.ports().ports.clear_refusals();
        let m = mark(&c3);
        assert_eq!(
            c3.answer(
                &item3,
                Choice::Decline,
                None,
                shown_step_mark_or_zero(&c3, &item3)
            ),
            Ok(()),
            "{order:?}"
        );
        assert_eq!(handed_back(&c3, m), [HandbackResult::Declined], "{order:?}");
        assert_eq!(
            step_rows(&c3, &item3),
            ["done", "decline"],
            "{order:?}: the newest row decides, and the same choice stores no second decline"
        );
    }
}

/// The memory `Runs::answer_bind` and `Runs::answer_handback` keep the port's duty (ruling RD-46
/// (2) g): each error other than the refusal `ended` comes with no call that did its work, so
/// that the run's waiting call got nothing, and a success is one call that did its work.
#[test]
fn the_memory_answer_bind_and_answer_handback_fail_with_an_error_other_than_ended_only_when_the_call_got_nothing(
) {
    use halvo_coord::mem::{Config, Ports};
    use halvo_coord::ports::Runs;
    use halvo_coord::types::{BindResult, HandbackResult};
    let run = RunId::parse(&format!("{:032x}", 7)).unwrap();
    let ended = PortError::Refused(Name::parse("ended").unwrap());
    let refusals = [
        PortError::Busy,
        PortError::NoService,
        PortError::Refused(Name::parse("internal").unwrap()),
    ];
    for method in [Method::AnswerBind, Method::AnswerHandback] {
        let call = |p: &Ports<Config>| match method {
            Method::AnswerBind => p.answer_bind(&run, BindResult::Allow),
            _ => p.answer_handback(&run, HandbackResult::Done),
        };
        let got = |p: &Ports<Config>| {
            p.calls
                .borrow()
                .iter()
                .filter(|t| matches!(t.call, Call::AnswerBind(..) | Call::AnswerHandback(..)))
                .count()
        };
        let p: Ports<Config> = Ports::new(T0, DAY);
        let unknown = call(&p);
        assert!(
            unknown.is_err() && unknown != Err(ended.clone()),
            "{method:?}"
        );
        assert_eq!(got(&p), 0, "{method:?}: unknown_run");
        p.runner_of(&run);
        for e in &refusals {
            p.refuse(method, e.clone());
            assert_eq!(call(&p), Err(e.clone()), "{method:?}");
            assert_eq!(got(&p), 0, "{method:?}: {e:?}");
            p.clear_refusals();
        }
        assert_eq!(call(&p), Ok(()), "{method:?}");
        assert_eq!(got(&p), 1, "{method:?}: a success did its work");
        p.end_runner(&run);
        assert_eq!(call(&p), Err(ended.clone()), "{method:?}");
        assert_eq!(got(&p), 1, "{method:?}");
    }
}

/// A switch-off of agent `n` whose `hold_set` the owner stored and whose acknowledgement was lost,
/// in the form `order`: the coordinator does not know the hold (ruling RD-52 (a), the builder's
/// seed 297).
fn switched_off_with_a_lost_acknowledgement(c: &Coordinator<Both>, n: usize, order: support::Loss) {
    c.ports().loss.set(Some(order));
    let _ = c.control(&agent(n), Action::SwitchOff);
    c.ports().loss.set(None);
    let lost: Vec<halvo_coord::types::Event> = c.ports().lost.borrow_mut().drain(..).collect();
    c.ports().ports.events.borrow_mut().extend(lost);
    let held = c.ports().ports.events.borrow().iter().any(|e| {
        e.agent == agent(n)
            && matches!(
                e.what,
                What::HoldSet {
                    cause: halvo_coord::types::HoldSetCause::Person
                }
            )
    });
    assert!(held, "{order:?}: the owner holds the hold_set");
}

fn run_starts_of(c: &Coordinator<Both>, n: usize) -> usize {
    c.ports()
        .ports
        .events
        .borrow()
        .iter()
        .filter(|e| e.agent == agent(n) && matches!(e.what, What::RunStarted { .. }))
        .count()
}

fn states_sent(c: &Coordinator<Both>, m: usize) -> Vec<halvo_coord::types::AgentState> {
    calls_since(c, m)
        .into_iter()
        .filter_map(|call| match call {
            Call::Send(Body::SetState { state, .. }) => Some(state),
            _ => None,
        })
        .collect()
}

/// What every test of RD-52 (a) asserts after the start whose read brought the hold: no
/// `Runs::start`, no `run_started` row of that start, `set_state` `paused` once and no `active`
/// or `approved`, and the row offers `switch_on` and no `run_now`, with the status `off` where no
/// item is open.
fn refused_after_its_read(c: &Coordinator<Both>, m: usize, n: usize, starts: usize, case: &str) {
    use halvo_coord::types::{AgentState, Control};
    assert!(
        !calls_since(c, m)
            .iter()
            .any(|call| matches!(call, Call::Start(_))),
        "{case}: no Runs::start"
    );
    assert_eq!(run_starts_of(c, n), starts, "{case}: no run_started row");
    assert_eq!(
        states_sent(c, m),
        [AgentState::Paused],
        "{case}: paused once, and no active or approved"
    );
    let s = c.snapshot();
    let row = s.rows.iter().find(|r| r.agent == agent(n)).unwrap();
    let controls: Vec<Control> = row.controls.iter().map(|t| t.control).collect();
    assert!(
        controls.contains(&Control::SwitchOn),
        "{case}: {controls:?}"
    );
    assert!(!controls.contains(&Control::RunNow), "{case}: {controls:?}");
    let open = s.items.iter().any(|i| i.agent() == &agent(n));
    if !open {
        assert_eq!(row.status, Status::Off, "{case}");
    }
}

#[test]
fn a_run_now_whose_read_brings_a_lost_switch_off_is_refused_with_hold_in_both_forms() {
    for order in NO_SERVICE_ROW_UNSEEN_ORDERS {
        let c = coordinator(&[MANUAL], Limits::PRODUCT);
        switched_off_with_a_lost_acknowledgement(&c, 0, order);
        let starts = run_starts_of(&c, 0);
        let m = mark(&c);
        assert_eq!(
            c.control(&agent(0), Action::RunNow),
            Err(Cause::Hold),
            "{order:?}"
        );
        refused_after_its_read(&c, m, 0, starts, &format!("{order:?}"));
    }
}

/// An agent of `spec` whose events hold the hold `failures`, known to the coordinator.
fn failures_held(spec: &[support::A]) -> Coordinator<Both> {
    let both = owner(spec, Limits::PRODUCT);
    {
        let mut events = both.ports.events.borrow_mut();
        let last = events
            .iter()
            .rfind(|e| e.agent == agent(0))
            .cloned()
            .unwrap();
        events.push(halvo_coord::types::Event {
            seq: std::num::NonZeroU32::new(last.seq.get() + 1).unwrap(),
            what: What::HoldSet {
                cause: halvo_coord::types::HoldSetCause::Failures,
            },
            ..last
        });
    }
    let c = Coordinator::new(both, Limits::PRODUCT);
    c.tick();
    c
}

#[test]
fn a_run_now_under_failures_whose_hold_cleared_read_brings_a_lost_switch_off_stores_the_clear_alone_in_both_forms(
) {
    use halvo_coord::types::Cleared;
    for order in NO_SERVICE_ROW_UNSEEN_ORDERS {
        let c = failures_held(&[MANUAL]);
        switched_off_with_a_lost_acknowledgement(&c, 0, order);
        let starts = run_starts_of(&c, 0);
        let before = events(&c).len();
        let m = mark(&c);
        assert_eq!(
            c.control(&agent(0), Action::RunNow),
            Err(Cause::Hold),
            "{order:?}"
        );
        assert_eq!(
            events(&c)[before..],
            [What::HoldCleared {
                cause: Cleared::Failures
            }],
            "{order:?}: one hold_cleared and nothing else"
        );
        refused_after_its_read(&c, m, 0, starts, &format!("{order:?}"));
    }
}

#[test]
fn a_queued_start_of_a_ready_item_whose_read_brings_a_lost_switch_off_is_refused_with_hold_and_leaves_the_item_open_in_both_forms(
) {
    let slotted = support::A {
        person: true,
        slotted: true,
    };
    let one = Limits {
        max_live_runs: 1,
        ..Limits::PRODUCT
    };
    for order in NO_SERVICE_ROW_UNSEEN_ORDERS {
        let c = coordinator(&[MANUAL, slotted, MANUAL], one);
        let (ready, kind) = open_item(&c);
        assert_eq!(kind, ItemKind::Ready);
        let first = start(&c, 0);
        switched_off_with_a_lost_acknowledgement(&c, 1, order);
        let starts = run_starts_of(&c, 1);
        let m = mark(&c);
        assert_eq!(
            c.answer(
                &ready,
                Choice::Start,
                None,
                shown_step_mark_or_zero(&c, &ready)
            ),
            Err(Cause::Hold),
            "{order:?}"
        );
        assert!(
            !events(&c)
                .iter()
                .any(|w| matches!(w, What::ItemAnswered { item, .. } if *item == ready)),
            "{order:?}: no item_answered for the item"
        );
        assert!(
            c.snapshot().items.iter().any(|i| i.item() == &ready),
            "{order:?}: the item is open"
        );
        refused_after_its_read(&c, m, 1, starts, &format!("{order:?}"));
        assert_eq!(c.control(&agent(2), Action::RunNow), Ok(()), "{order:?}");
        assert_eq!(started(&c).len(), 1, "{order:?}: agent 2 waits");
        c.ports().ports.end_runner(&first);
        c.on_run(&first, RunEvent::Ended(Some(Outcome::Completed)))
            .unwrap();
        let specs: Vec<AgentId> = c
            .ports()
            .ports
            .calls
            .borrow()
            .iter()
            .filter_map(|t| match &t.call {
                Call::Start(spec) => Some(spec.agent.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            specs,
            [agent(0), agent(2)],
            "{order:?}: the queue holds no start of agent 1, so the end takes agent 2's"
        );
        assert_eq!(run_starts_of(&c, 1), starts, "{order:?}");
    }
}

#[test]
fn a_ticks_start_whose_read_brings_a_lost_switch_off_stores_nothing_and_the_next_tick_tries_none_in_both_forms(
) {
    let auto = support::A {
        person: false,
        slotted: true,
    };
    for order in NO_SERVICE_ROW_UNSEEN_ORDERS {
        let c = coordinator(&[auto], Limits::PRODUCT);
        let first = started(&c).last().cloned().expect("the first slot's start");
        assert_eq!(c.on_run(&first, RunEvent::Hello), Ok(()));
        c.ports().ports.end_runner(&first);
        c.on_run(&first, RunEvent::Ended(Some(Outcome::Completed)))
            .unwrap();
        c.ports().ports.store_end(
            &agent(0),
            PastRun {
                run: first,
                day: DAY,
                outcome: Outcome::Completed,
                unknown: 0,
            },
        );
        c.ports().ports.set_clock(T0 + 1_000, DAY);
        c.tick();
        switched_off_with_a_lost_acknowledgement(&c, 0, order);
        let starts = run_starts_of(&c, 0);
        let before = events(&c).len();
        let m = mark(&c);
        c.ports().ports.set_clock(T0 + 2 * 86_400_000, DAY + 2);
        c.tick();
        assert_eq!(events(&c).len(), before, "{order:?}: nothing is stored");
        refused_after_its_read(&c, m, 0, starts, &format!("{order:?}"));
        let m = mark(&c);
        let tried = c.ports().ports.tried.borrow().len();
        c.ports()
            .ports
            .set_clock(T0 + 2 * 86_400_000 + 1_000, DAY + 2);
        c.tick();
        assert!(
            !calls_since(&c, m)
                .iter()
                .any(|call| matches!(call, Call::Start(_))),
            "{order:?}"
        );
        assert!(
            !c.ports().ports.tried.borrow()[tried..].iter().any(|t| matches!(
                &t.call,
                Call::Log(rows) if rows.iter().any(|e| matches!(e.what, What::RunStarted { .. }))
            )),
            "{order:?}: the next tick tries no start of the agent"
        );
    }
}

#[test]
fn a_start_that_leaves_the_queue_after_a_lost_switch_off_is_dropped_with_hold_and_stores_nothing_in_both_forms(
) {
    let one = Limits {
        max_live_runs: 1,
        ..Limits::PRODUCT
    };
    for order in NO_SERVICE_ROW_UNSEEN_ORDERS {
        let c = coordinator(&[MANUAL, MANUAL], one);
        let first = start(&c, 0);
        assert_eq!(c.control(&agent(1), Action::RunNow), Ok(()), "{order:?}");
        assert_eq!(started(&c).len(), 1, "{order:?}: agent 1 waits");
        switched_off_with_a_lost_acknowledgement(&c, 1, order);
        let starts = run_starts_of(&c, 1);
        let before = c
            .ports()
            .ports
            .events
            .borrow()
            .iter()
            .filter(|e| e.agent == agent(1))
            .count();
        let m = mark(&c);
        c.ports().ports.end_runner(&first);
        c.on_run(&first, RunEvent::Ended(Some(Outcome::Completed)))
            .unwrap();
        let after = c
            .ports()
            .ports
            .events
            .borrow()
            .iter()
            .filter(|e| e.agent == agent(1))
            .count();
        assert_eq!(after, before, "{order:?}: nothing is stored for agent 1");
        refused_after_its_read(&c, m, 1, starts, &format!("{order:?}"));
        let m = mark(&c);
        c.ports().ports.set_clock(T0 + 1_000, DAY);
        c.tick();
        assert!(
            !calls_since(&c, m)
                .iter()
                .any(|call| matches!(call, Call::Start(_))),
            "{order:?}: the start was dropped"
        );
    }
}

fn stored_not_taken_with_port_busy(
    reason: HandbackReason,
    first: Choice,
) -> (Coordinator<Both>, RunId, ItemId) {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    c.on_run(&run, handback(&run, 0, reason)).unwrap();
    let (item, _) = open_item(&c);
    c.ports()
        .ports
        .refuse(Method::AnswerHandback, PortError::Busy);
    assert_eq!(
        c.answer(&item, first, None, shown_step_mark_or_zero(&c, &item)),
        Err(Cause::Busy)
    );
    c.ports().ports.clear_refusals();
    (c, run, item)
}

fn press_landing_later(c: &Coordinator<Both>, item: &ItemId, choice: Choice) {
    c.ports().loss.set(Some(support::Loss::Later));
    assert_eq!(
        c.answer(item, choice, None, shown_step_mark_or_zero(c, item)),
        Err(Cause::NoService)
    );
    c.ports().loss.set(None);
    let lost: Vec<halvo_coord::types::Event> = c.ports().lost.borrow_mut().drain(..).collect();
    c.ports().ports.events.borrow_mut().extend(lost);
}

fn press_stored_unread(c: &Coordinator<Both>, item: &ItemId, choice: Choice) {
    c.ports().loss.set(Some(support::Loss::Unread));
    assert_eq!(
        c.answer(item, choice, None, shown_step_mark_or_zero(c, item)),
        Err(Cause::NoService)
    );
    c.ports().loss.set(None);
}

#[test]
fn rd61_t_a_link_done_then_a_decline_landing_unread_then_done_gives_declined_once() {
    use halvo_coord::types::HandbackResult;
    let (c, _run, item) =
        stored_not_taken_with_port_busy(HandbackReason::AccountLink, Choice::Done);
    let m = mark(&c);
    press_stored_unread(&c, &item, Choice::Decline);
    assert_eq!(
        c.answer(
            &item,
            Choice::Done,
            None,
            shown_step_mark_or_zero(&c, &item)
        ),
        Err(Cause::UnknownItem)
    );
    assert_eq!(handed_back(&c, m), [HandbackResult::Declined]);
    assert_eq!(step_rows(&c, &item), ["done", "decline"]);
    assert!(c.snapshot().items.is_empty());
}

#[test]
fn rd61_t_b_link_done_then_a_decline_landing_unread_then_decline_stores_no_third_row() {
    use halvo_coord::types::HandbackResult;
    let (c, _run, item) =
        stored_not_taken_with_port_busy(HandbackReason::AccountLink, Choice::Done);
    let m = mark(&c);
    press_stored_unread(&c, &item, Choice::Decline);
    assert_eq!(
        c.answer(
            &item,
            Choice::Decline,
            None,
            shown_step_mark_or_zero(&c, &item)
        ),
        Ok(())
    );
    assert_eq!(handed_back(&c, m), [HandbackResult::Declined]);
    assert_eq!(step_rows(&c, &item), ["done", "decline"]);
}

#[test]
fn rd61_t_c_link_done_then_a_decline_that_never_landed_then_done_stores_done_again() {
    use halvo_coord::types::HandbackResult;
    let (c, _run, item) =
        stored_not_taken_with_port_busy(HandbackReason::AccountLink, Choice::Done);
    let m = mark(&c);
    c.ports().ports.refuse(Method::Log, PortError::NoService);
    assert_eq!(
        c.answer(
            &item,
            Choice::Decline,
            None,
            shown_step_mark_or_zero(&c, &item)
        ),
        Err(Cause::NoService)
    );
    c.ports().ports.clear_refusals();
    assert_eq!(
        c.answer(
            &item,
            Choice::Done,
            None,
            shown_step_mark_or_zero(&c, &item)
        ),
        Ok(())
    );
    assert_eq!(handed_back(&c, m), [HandbackResult::Done]);
    assert_eq!(step_rows(&c, &item), ["done", "done"]);
}

#[test]
fn rd61_t_d_pin_link_done_then_a_known_decline_whose_call_was_busy_then_done_gives_declined() {
    use halvo_coord::types::HandbackResult;
    let (c, _run, item) =
        stored_not_taken_with_port_busy(HandbackReason::AccountLink, Choice::Done);
    c.ports()
        .ports
        .refuse(Method::AnswerHandback, PortError::Busy);
    assert_eq!(
        c.answer(
            &item,
            Choice::Decline,
            None,
            shown_step_mark_or_zero(&c, &item)
        ),
        Err(Cause::Busy)
    );
    c.ports().ports.clear_refusals();
    let m = mark(&c);
    assert_eq!(
        c.answer(
            &item,
            Choice::Done,
            None,
            shown_step_mark_or_zero(&c, &item)
        ),
        Err(Cause::UnknownItem)
    );
    assert_eq!(handed_back(&c, m), [HandbackResult::Declined]);
    assert_eq!(step_rows(&c, &item), ["done", "decline"]);
}

#[test]
fn rd61_t_e_a_stored_done_not_taken_gets_no_lapse_and_no_done_at_any_end() {
    use halvo_coord::types::HandbackResult;
    let limits = Limits::PRODUCT;
    for way in ["stop", "wait_limit", "runners_end", "quit"] {
        for landed in [false, true] {
            let case = format!("{way} landed={landed}");
            let (c, run, item) = if landed {
                let c = coordinator(&[MANUAL], limits);
                let run = start(&c, 0);
                c.on_run(&run, handback(&run, 0, HandbackReason::AccountLink))
                    .unwrap();
                let (item, _) = open_item(&c);
                press_landing_later(&c, &item, Choice::Done);
                (c, run, item)
            } else {
                stored_not_taken_with_port_busy(HandbackReason::AccountLink, Choice::Done)
            };
            let m = mark(&c);
            match way {
                "stop" => assert_eq!(c.control(&agent(0), Action::Stop), Ok(()), "{case}"),
                "wait_limit" => {
                    c.ports()
                        .ports
                        .set_clock(T0 + limits.wait_limit_s * 1000, DAY);
                    c.tick();
                }
                "runners_end" => {
                    c.ports().ports.end_runner(&run);
                    let _ = c.on_run(&run, RunEvent::Ended(Some(Outcome::HandedBack)));
                }
                _ => {
                    c.quit_stop();
                    c.quit_kill();
                }
            }
            c.ports()
                .ports
                .set_clock(T0 + 2 * limits.wait_limit_s * 1000, DAY);
            c.tick();
            assert_eq!(step_rows(&c, &item), ["done"], "{case}: no item_lapsed");
            assert!(
                !handed_back(&c, m).contains(&HandbackResult::Done),
                "{case}: the run never gets done"
            );
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HeldNotTakenWay {
    Unread,
    Later,
    Busy,
}

const HELD: [HeldNotTakenWay; 3] = [
    HeldNotTakenWay::Unread,
    HeldNotTakenWay::Later,
    HeldNotTakenWay::Busy,
];

fn held_not_taken(
    reason: HandbackReason,
    first: Choice,
    how: HeldNotTakenWay,
) -> (Coordinator<Both>, RunId, ItemId) {
    let (c, run, item) = match how {
        HeldNotTakenWay::Busy => stored_not_taken_with_port_busy(reason, first),
        HeldNotTakenWay::Later | HeldNotTakenWay::Unread => {
            let c = coordinator(&[MANUAL], Limits::PRODUCT);
            let run = start(&c, 0);
            c.on_run(&run, handback(&run, 0, reason)).unwrap();
            let (item, _) = open_item(&c);
            if how == HeldNotTakenWay::Later {
                press_landing_later(&c, &item, first);
            } else {
                c.ports().loss.set(Some(support::Loss::Unread));
                assert_eq!(
                    c.answer(&item, first, None, shown_step_mark_or_zero(&c, &item)),
                    Err(Cause::NoService)
                );
                c.ports().loss.set(None);
            }
            (c, run, item)
        }
    };
    assert_eq!(
        closing_rows(&c, &item),
        1,
        "{how:?}: the owner holds the row"
    );
    (c, run, item)
}

fn answer_choices(c: &Coordinator<Both>, item: &ItemId) -> Vec<Choice> {
    events(c)
        .iter()
        .filter_map(|w| match w {
            What::ItemAnswered { item: i, answer } if i == item => answer.choice(),
            _ => None,
        })
        .collect()
}

const DECLINE_DONE: [HandbackReason; 2] = [HandbackReason::StepFailed, HandbackReason::SignIn];

#[test]
fn rd61_t_f_a_stored_done_not_taken_then_decline_stores_two_rows_and_sends_declined_once() {
    use halvo_coord::types::HandbackResult;
    for reason in DECLINE_DONE {
        for how in HELD {
            let case = format!("{reason:?} {how:?}");
            let (c, _run, item) = held_not_taken(reason, Choice::Done, how);
            let m = mark(&c);
            assert_eq!(
                c.answer(
                    &item,
                    Choice::Decline,
                    None,
                    shown_step_mark_or_zero(&c, &item)
                ),
                Ok(()),
                "{case}"
            );
            assert_eq!(
                answer_choices(&c, &item),
                [Choice::Done, Choice::Decline],
                "{case}"
            );
            assert_eq!(handed_back(&c, m), [HandbackResult::Declined], "{case}");
            assert!(c.snapshot().items.is_empty(), "{case}: no card");
        }
    }
}

#[test]
fn rd61_t_g_pin_a_stored_decline_not_taken_then_done_sends_declined_and_answers_unknown_item() {
    use halvo_coord::types::HandbackResult;
    for reason in DECLINE_DONE {
        for how in HELD {
            let case = format!("{reason:?} {how:?}");
            let (c, _run, item) = held_not_taken(reason, Choice::Decline, how);
            let m = mark(&c);
            assert_eq!(
                c.answer(
                    &item,
                    Choice::Done,
                    None,
                    shown_step_mark_or_zero(&c, &item)
                ),
                Err(Cause::UnknownItem),
                "{case}"
            );
            assert_eq!(answer_choices(&c, &item), [Choice::Decline], "{case}");
            assert_eq!(handed_back(&c, m), [HandbackResult::Declined], "{case}");
        }
    }
}

#[test]
fn rd61_t_h_the_clause_holds_for_stuck_and_a_sign_ins_first_step() {
    use halvo_coord::types::HandbackResult;
    for reason in DECLINE_DONE {
        for (landed, then) in [
            (true, Choice::Done),
            (false, Choice::Done),
            (true, Choice::Decline),
        ] {
            let case = format!("{reason:?} landed={landed} then={then:?}");
            let (c, _run, item) = stored_not_taken_with_port_busy(reason, Choice::Done);
            let m = mark(&c);
            if landed {
                press_stored_unread(&c, &item, Choice::Decline);
            } else {
                c.ports().ports.refuse(Method::Log, PortError::NoService);
                assert_eq!(
                    c.answer(
                        &item,
                        Choice::Decline,
                        None,
                        shown_step_mark_or_zero(&c, &item)
                    ),
                    Err(Cause::NoService),
                    "{case}"
                );
                c.ports().ports.clear_refusals();
            }
            let out = c.answer(&item, then, None, shown_step_mark_or_zero(&c, &item));
            let (answer, rows, result) = match (landed, then) {
                (true, Choice::Done) => (
                    Err(Cause::UnknownItem),
                    vec![Choice::Done, Choice::Decline],
                    HandbackResult::Declined,
                ),
                (false, _) => (
                    Ok(()),
                    vec![Choice::Done, Choice::Done],
                    HandbackResult::Done,
                ),
                _ => (
                    Ok(()),
                    vec![Choice::Done, Choice::Decline],
                    HandbackResult::Declined,
                ),
            };
            assert_eq!(out, answer, "{case}");
            assert_eq!(answer_choices(&c, &item), rows, "{case}");
            assert_eq!(handed_back(&c, m), [result], "{case}");
        }
    }
}

const HANDBACK_KINDS_WITH_DONE_PRESS: [(HandbackReason, Choice); 4] = [
    (HandbackReason::SignIn, Choice::Done),
    (HandbackReason::AccountLink, Choice::Done),
    (HandbackReason::StepFailed, Choice::Done),
    (HandbackReason::EffectUnknown, Choice::TryAgain),
];

fn waiting_on(reason: HandbackReason) -> (Coordinator<Both>, RunId, ItemId) {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    c.on_run(&run, handback(&run, 0, reason)).unwrap();
    let (item, _) = open_item(&c);
    (c, run, item)
}

#[test]
fn rd52c_a_handback_answer_whose_row_the_read_shows_is_handed_in_that_move_for_each_kind() {
    use halvo_coord::types::HandbackResult;
    for (reason, choice) in HANDBACK_KINDS_WITH_DONE_PRESS {
        let (c, _run, item) = waiting_on(reason);
        let m = mark(&c);
        c.ports().loss.set(Some(support::Loss::Reply));
        assert_eq!(
            c.answer(&item, choice, None, shown_step_mark_or_zero(&c, &item)),
            Ok(()),
            "{reason:?}"
        );
        assert_eq!(closing_rows(&c, &item), 1, "{reason:?}: one row");
        assert_eq!(handed_back(&c, m), [HandbackResult::Done], "{reason:?}");
        let s = c.snapshot();
        assert!(s.items.is_empty(), "{reason:?}: no card");
        assert_eq!(row_line(&s, 0), text("line.working.working"), "{reason:?}");
    }
}

#[test]
fn rd52c_a_handback_answer_whose_read_fails_or_shows_nothing_answers_the_stores_cause() {
    for (reason, choice) in HANDBACK_KINDS_WITH_DONE_PRESS {
        for loss in [support::Loss::Unread, support::Loss::Nothing] {
            let case = format!("{reason:?} {loss:?}");
            let (c, _run, item) = waiting_on(reason);
            let m = mark(&c);
            let tried = c.ports().ports.tried.borrow().len();
            c.ports().loss.set(Some(loss));
            assert_eq!(
                c.answer(&item, choice, None, shown_step_mark_or_zero(&c, &item)),
                Err(Cause::NoService),
                "{case}"
            );
            assert!(handed_back(&c, m).is_empty(), "{case}: nothing sent");
            let logs = c.ports().ports.tried.borrow()[tried..]
                .iter()
                .filter(|t| matches!(t.call, Call::Log(_)))
                .count();
            let lost = usize::from(loss == support::Loss::Unread);
            assert_eq!(logs, lost, "{case}: no log after the lost one");
            assert_eq!(open_item(&c).0, item, "{case}: the card stays");
        }
    }
}

#[test]
fn rd52c_a_read_that_shows_another_choice_as_the_newest_decides_by_rule_3() {
    use halvo_coord::types::HandbackResult;
    let cases = [
        (
            HandbackReason::StepFailed,
            Choice::Done,
            Choice::Decline,
            Ok(()),
            HandbackResult::Declined,
            2,
        ),
        (
            HandbackReason::AccountLink,
            Choice::Done,
            Choice::Decline,
            Ok(()),
            HandbackResult::Declined,
            2,
        ),
        (
            HandbackReason::SignIn,
            Choice::Decline,
            Choice::Done,
            Err(Cause::UnknownItem),
            HandbackResult::Declined,
            1,
        ),
        (
            HandbackReason::EffectUnknown,
            Choice::TryAgain,
            Choice::WentThrough,
            Err(Cause::UnknownItem),
            HandbackResult::Done,
            1,
        ),
    ];
    for (reason, first, then, answer, result, rows) in cases {
        let case = format!("{reason:?} {first:?} then {then:?}");
        let (c, _run, item) = waiting_on(reason);
        c.ports().loss.set(Some(support::Loss::Unread));
        assert_eq!(
            c.answer(&item, first, None, shown_step_mark_or_zero(&c, &item)),
            Err(Cause::NoService),
            "{case}"
        );
        let m = mark(&c);
        c.ports().loss.set(Some(support::Loss::Nothing));
        assert_eq!(
            c.answer(&item, then, None, shown_step_mark_or_zero(&c, &item)),
            answer,
            "{case}"
        );
        assert_eq!(handed_back(&c, m), [result], "{case}");
        assert_eq!(closing_rows(&c, &item), rows, "{case}");
        assert!(c.snapshot().items.is_empty(), "{case}: no card");
    }
}

#[test]
fn rd52c_a_read_that_fails_after_a_lost_store_logs_nothing_more() {
    for reason in DECLINE_DONE {
        let (c, _run, item) = stored_not_taken_with_port_busy(reason, Choice::Done);
        let m = mark(&c);
        let tried = c.ports().ports.tried.borrow().len();
        c.ports().loss.set(Some(support::Loss::Nothing));
        c.ports()
            .ports
            .refuse(Method::History, PortError::NoService);
        assert_eq!(
            c.answer(
                &item,
                Choice::Decline,
                None,
                shown_step_mark_or_zero(&c, &item)
            ),
            Err(Cause::NoService),
            "{reason:?}"
        );
        c.ports().ports.clear_refusals();
        let logs = c.ports().ports.tried.borrow()[tried..]
            .iter()
            .filter(|t| matches!(t.call, Call::Log(_)))
            .count();
        assert_eq!(logs, 0, "{reason:?}: no log after the lost one");
        assert!(handed_back(&c, m).is_empty(), "{reason:?}: nothing sent");
        assert_eq!(answer_choices(&c, &item), [Choice::Done], "{reason:?}");
    }
}

#[test]
fn rd52c_a_stored_done_of_an_account_link_then_a_lost_decline_sends_declined() {
    use halvo_coord::types::HandbackResult;
    for how in [HeldNotTakenWay::Busy, HeldNotTakenWay::Unread] {
        let (c, _run, item) = held_not_taken(HandbackReason::AccountLink, Choice::Done, how);
        let m = mark(&c);
        c.ports().loss.set(Some(support::Loss::Reply));
        assert_eq!(
            c.answer(
                &item,
                Choice::Decline,
                None,
                shown_step_mark_or_zero(&c, &item)
            ),
            Ok(()),
            "{how:?}"
        );
        assert_eq!(handed_back(&c, m), [HandbackResult::Declined], "{how:?}");
        assert_eq!(step_rows(&c, &item), ["done", "decline"], "{how:?}");
    }
}

#[test]
fn rd52c_after_a_busy_port_the_same_press_sends_again_with_no_second_row() {
    use halvo_coord::types::HandbackResult;
    for (reason, choice) in HANDBACK_KINDS_WITH_DONE_PRESS {
        let (c, _run, item) = waiting_on(reason);
        c.ports().loss.set(Some(support::Loss::Reply));
        c.ports()
            .ports
            .refuse(Method::AnswerHandback, PortError::Busy);
        let tried = c.ports().ports.tried.borrow().len();
        assert_eq!(
            c.answer(&item, choice, None, shown_step_mark_or_zero(&c, &item)),
            Err(Cause::Busy),
            "{reason:?}"
        );
        let calls = c.ports().ports.tried.borrow()[tried..]
            .iter()
            .filter(|t| matches!(t.call, Call::AnswerHandback(..)))
            .count();
        assert_eq!(calls, 1, "{reason:?}: the port once");
        c.ports().ports.clear_refusals();
        assert_eq!(open_item(&c).0, item, "{reason:?}: the card stays");
        let m = mark(&c);
        assert_eq!(
            c.answer(&item, choice, None, shown_step_mark_or_zero(&c, &item)),
            Ok(()),
            "{reason:?}"
        );
        assert_eq!(handed_back(&c, m), [HandbackResult::Done], "{reason:?}");
        assert_eq!(closing_rows(&c, &item), 1, "{reason:?}: no second row");
    }
}

const WAIT_WAYS_BY_LAPSE: [&str; 3] = ["stuck", "confirm", "second_step"];

fn waiting(way: &str) -> (Coordinator<Both>, RunId, ItemId) {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    let event = match way {
        "stuck" => handback(&run, 0, HandbackReason::StepFailed),
        "confirm" => ask_confirm(&run, 0),
        _ => bind(&run, 0),
    };
    c.on_run(&run, event).unwrap();
    let (item, _) = open_item(&c);
    (c, run, item)
}

#[test]
fn rd48a_a_stop_whose_cancel_panics_leaves_the_board_holding_it_at_each_lapse() {
    let limits = Limits::PRODUCT;
    for way in WAIT_WAYS_BY_LAPSE {
        for by in ["stop", "switch_off", "wait_limit"] {
            let case = format!("{way} {by}");
            let (c, run, _item) = waiting(way);
            let at = if by == "wait_limit" {
                T0 + limits.wait_limit_s * 1000
            } else {
                T0
            };
            c.ports().ports.set_clock(at, DAY);
            c.ports().panics.set(Some("cancel"));
            let stop = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match by {
                "stop" => {
                    let _ = c.control(&agent(0), Action::Stop);
                }
                "switch_off" => {
                    let _ = c.control(&agent(0), Action::SwitchOff);
                }
                _ => c.tick(),
            }));
            c.ports().panics.set(None);
            assert!(stop.is_err(), "{case}: the port's cancel panicked");
            c.tick();
            let s = c.snapshot();
            assert!(s.items.is_empty(), "{case}: no card of the lapsed item");
            let line = if by == "switch_off" {
                "line.off.person"
            } else {
                "line.working.stopping"
            };
            assert_eq!(
                row_line(&s, 0),
                text(line),
                "{case}: the board holds the stop"
            );
            let m = mark(&c);
            c.ports()
                .ports
                .set_clock(at + limits.cancel_grace_s * 1000, DAY);
            c.tick();
            assert_eq!(kills_of(&c, m, &run), 1, "{case}: the grace tick kills");
        }
    }
}

#[test]
fn rd83_a_cancel_or_a_decision_that_panics_at_a_confirmations_stop_leaves_its_row_owed() {
    let limits = Limits::PRODUCT;
    for by in ["stop", "switch_off", "wait_limit"] {
        for panics in ["cancel", "answer_confirm"] {
            let case = format!("{by} {panics}");
            let (c, run, item) = run_waiting_on_a_confirmation(false);
            let at = if by == "wait_limit" {
                T0 + limits.wait_limit_s * 1000
            } else {
                T0
            };
            c.ports().ports.set_clock(at, DAY);
            let m = mark(&c);
            c.ports().panics.set(Some(panics));
            let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match by {
                "stop" => {
                    let _ = c.control(&agent(0), Action::Stop);
                }
                "switch_off" => {
                    let _ = c.control(&agent(0), Action::SwitchOff);
                }
                _ => c.tick(),
            }));
            c.ports().panics.set(None);
            assert!(out.is_err(), "{case}: the port panicked");
            let cancels = usize::from(panics == "answer_confirm");
            assert_eq!(cancels_of(&c, m, &run), cancels, "{case}: the cancel first");
            let decided = calls_since(&c, m)
                .iter()
                .any(|call| matches!(call, Call::AnswerConfirm(..)));
            assert!(!decided, "{case}: no decision");
            assert_eq!(closing_rows(&c, &item), 0, "{case}: the row is owed");
            let record = halvo_coord::guard::EffectRecord {
                run: run.clone(),
                agent: agent(0),
                effect: halvo_coord::types::EffectId::parse(&format!("{:032x}", 0xe1)).unwrap(),
                state: halvo_coord::guard::EffectState::Approved,
                class: Class::Send,
                digest: halvo_coord::types::Digest::parse(&"ab".repeat(32)).unwrap(),
                approval: None,
            };
            assert_eq!(
                c.check_record(&record),
                Err(halvo_coord::guard::Refusal::Approval),
                "{case}: the table holds the stop"
            );
            c.tick();
            assert_eq!(closing_rows(&c, &item), 1, "{case}: the next tick sends it");
            let s = c.snapshot();
            assert!(s.items.is_empty(), "{case}: no card of the lapsed item");
            let line = if by == "switch_off" {
                "line.off.person"
            } else {
                "line.working.stopping"
            };
            assert_eq!(
                row_line(&s, 0),
                text(line),
                "{case}: the board holds the stop"
            );
            c.tick();
            assert_eq!(closing_rows(&c, &item), 1, "{case}: once");
        }
    }
}

fn is_hold(what: &What) -> bool {
    matches!(what, What::HoldSet { .. } | What::HoldCleared { .. })
}

fn lose_the_next_holds_reply(c: &Coordinator<Both>, order: support::Loss) {
    c.ports().loss_waits_for_row.set(Some(is_hold));
    c.ports().loss.set(Some(order));
}

fn hold_sets(c: &Coordinator<Both>, n: usize) -> usize {
    c.ports()
        .ports
        .events
        .borrow()
        .iter()
        .filter(|e| e.agent == agent(n) && matches!(e.what, What::HoldSet { .. }))
        .count()
}

fn places_of_calls_since(
    c: &Coordinator<Both>,
    m: usize,
    pick: impl Fn(&Call) -> bool,
) -> Vec<usize> {
    calls_since(c, m)
        .iter()
        .enumerate()
        .filter(|(_, call)| pick(call))
        .map(|(k, _)| k)
        .collect()
}

#[test]
fn rd52b_a_switch_off_whose_hold_the_read_shows_stops_and_pauses_in_the_order_of_each_lapse() {
    use halvo_coord::types::AgentState;
    for way in ["stuck", "confirm"] {
        let (c, run, item) = waiting(way);
        let m = mark(&c);
        lose_the_next_holds_reply(&c, support::Loss::Reply);
        assert_eq!(c.control(&agent(0), Action::SwitchOff), Ok(()), "{way}");
        assert_eq!(hold_sets(&c, 0), 1, "{way}");
        assert_eq!(closing_rows(&c, &item), 1, "{way}: the lapse");
        assert!(
            events(&c).iter().any(|w| matches!(w, What::ItemLapsed { item: i, lapsed } if *i == item && lapsed.cause().name() == "cancelled")),
            "{way}"
        );
        let lapse = places_of_calls_since(
            &c,
            m,
            |call| matches!(call, Call::Log(rows) if rows.iter().any(|e| matches!(e.what, What::ItemLapsed { .. }))),
        );
        let cancel =
            places_of_calls_since(&c, m, |call| matches!(call, Call::Cancel(r) if *r == run));
        let paused = places_of_calls_since(&c, m, |call| {
            matches!(
                call,
                Call::Send(Body::SetState {
                    state: AgentState::Paused,
                    ..
                })
            )
        });
        assert_eq!(cancel.len(), 1, "{way}: one cancel");
        assert_eq!(paused.len(), 1, "{way}: paused once");
        let (first, second) = if way == "confirm" {
            (cancel[0], lapse[0])
        } else {
            (lapse[0], cancel[0])
        };
        assert!(
            first < second && second < paused[0],
            "{way}: {lapse:?} {cancel:?} {paused:?}"
        );
        assert!(c.snapshot().items.is_empty(), "{way}");
        c.ports().ports.end_runner(&run);
        assert_eq!(c.on_run(&run, RunEvent::Ended(None)), Ok(()), "{way}");
        assert!(events(&c).contains(&What::RunEnded {
            run: run.clone(),
            cause: RunEnd::Cancelled,
            defers: SEEN_WHOLE
        }));
    }
}

fn assert_an_unseen_switch_off_does_nothing(
    c: &Coordinator<Both>,
    item: &ItemId,
    order: support::Loss,
    case: &str,
) {
    let m = mark(c);
    lose_the_next_holds_reply(c, order);
    assert_eq!(
        c.control(&agent(0), Action::SwitchOff),
        Err(Cause::NoService),
        "{case}"
    );
    c.ports().loss.set(None);
    let lost: Vec<halvo_coord::types::Event> = c.ports().lost.borrow_mut().drain(..).collect();
    c.ports().ports.events.borrow_mut().extend(lost);
    assert_eq!(closing_rows(c, item), 0, "{case}: no item_lapsed");
    assert!(
        !calls_since(c, m)
            .iter()
            .any(|call| matches!(call, Call::Cancel(_) | Call::Send(Body::SetState { .. }))),
        "{case}: no cancel and no set_state"
    );
    let s = c.snapshot();
    assert_eq!(open_item(c).0, *item, "{case}: the card stays");
    assert_ne!(row_line(&s, 0), text("line.working.stopping"), "{case}");
    assert_eq!(hold_sets(c, 0), 1, "{case}: the owner holds the hold");
}

#[test]
fn rd52b_a_switch_off_whose_hold_the_move_does_not_see_does_nothing_and_a_second_one_works() {
    use halvo_coord::types::{AgentState, Control};
    for way in ["stuck", "confirm"] {
        for order in NO_SERVICE_ROW_UNSEEN_ORDERS {
            let case = format!("{way} {order:?}");
            let (c, run, item) = waiting(way);
            assert_an_unseen_switch_off_does_nothing(&c, &item, order, &case);
            let row = c.snapshot().rows[0].clone();
            assert!(
                row.controls.iter().any(|t| t.control == Control::SwitchOff),
                "{case}: the row offers Switch off"
            );
            let m = mark(&c);
            assert_eq!(c.control(&agent(0), Action::SwitchOff), Ok(()), "{case}");
            assert_eq!(hold_sets(&c, 0), 1, "{case}: no second hold_set");
            assert_eq!(closing_rows(&c, &item), 1, "{case}: the lapse");
            assert_eq!(cancels_of(&c, m, &run), 1, "{case}");
            assert_eq!(states_sent(&c, m), [AgentState::Paused], "{case}");
        }
    }
}

#[test]
fn rd52b_t_i_an_answer_whose_read_shows_a_stored_switch_off_stops_the_run_first() {
    use halvo_coord::types::AgentState;
    for reason in DECLINE_DONE {
        for order in NO_SERVICE_ROW_UNSEEN_ORDERS {
            let case = format!("{reason:?} {order:?}");
            let (c, run, item) = waiting_on(reason);
            assert_an_unseen_switch_off_does_nothing(&c, &item, order, &case);
            let m = mark(&c);
            assert_eq!(
                c.answer(
                    &item,
                    Choice::Done,
                    None,
                    shown_step_mark_or_zero(&c, &item)
                ),
                Err(Cause::UnknownItem),
                "{case}"
            );
            assert!(
                answer_choices(&c, &item).is_empty(),
                "{case}: no answer row"
            );
            assert!(
                handed_back(&c, m).is_empty(),
                "{case}: no answer at the run"
            );
            assert_eq!(step_rows(&c, &item), ["lapsed:cancelled"], "{case}");
            assert_eq!(cancels_of(&c, m, &run), 1, "{case}");
            assert_eq!(states_sent(&c, m), [AgentState::Paused], "{case}");
            assert!(c.snapshot().items.is_empty(), "{case}: no card");
            assert_eq!(c.live(), 1, "{case}: the run is stopping until its end");
        }
    }
}

#[test]
fn rd52b_a_stop_after_an_unseen_switch_off_stops_the_run_once() {
    for order in NO_SERVICE_ROW_UNSEEN_ORDERS {
        let (c, run, item) = waiting("stuck");
        assert_an_unseen_switch_off_does_nothing(&c, &item, order, &format!("{order:?}"));
        let m = mark(&c);
        assert_eq!(c.control(&agent(0), Action::Stop), Ok(()), "{order:?}");
        assert_eq!(cancels_of(&c, m, &run), 1, "{order:?}");
        assert_eq!(step_rows(&c, &item), ["lapsed:cancelled"], "{order:?}");
        assert_eq!(hold_sets(&c, 0), 1, "{order:?}");
    }
}

#[test]
fn rd52b_a_switch_off_of_a_working_run_in_both_forms() {
    use halvo_coord::types::AgentState;
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    let m = mark(&c);
    lose_the_next_holds_reply(&c, support::Loss::Reply);
    assert_eq!(c.control(&agent(0), Action::SwitchOff), Ok(()));
    assert_eq!(cancels_of(&c, m, &run), 1);
    assert_eq!(states_sent(&c, m), [AgentState::Paused]);
    for order in NO_SERVICE_ROW_UNSEEN_ORDERS {
        let c = coordinator(&[MANUAL], Limits::PRODUCT);
        let run = start(&c, 0);
        let m = mark(&c);
        lose_the_next_holds_reply(&c, order);
        assert_eq!(
            c.control(&agent(0), Action::SwitchOff),
            Err(Cause::NoService),
            "{order:?}"
        );
        c.ports().loss.set(None);
        let lost: Vec<halvo_coord::types::Event> = c.ports().lost.borrow_mut().drain(..).collect();
        c.ports().ports.events.borrow_mut().extend(lost);
        assert_eq!(cancels_of(&c, m, &run), 0, "{order:?}");
        let _ = c.on_run(&run, handback(&run, 0, HandbackReason::StepFailed));
        assert_eq!(
            cancels_of(&c, m, &run),
            1,
            "{order:?}: the request's move stops"
        );
        assert_eq!(states_sent(&c, m), [AgentState::Paused], "{order:?}");
        let s = c.snapshot();
        assert!(s.items.is_empty(), "{order:?}: no card");
        assert_eq!(row_line(&s, 0), text("line.off.person"), "{order:?}");
    }
}

#[test]
fn rd52b_a_switch_off_while_the_owner_is_away_does_nothing() {
    for way in ["stuck", "confirm"] {
        let (c, _run, item) = waiting(way);
        let m = mark(&c);
        c.ports().ports.refuse(Method::Log, PortError::NoService);
        assert_eq!(
            c.control(&agent(0), Action::SwitchOff),
            Err(Cause::NoService),
            "{way}"
        );
        c.ports().ports.clear_refusals();
        assert_eq!(hold_sets(&c, 0), 0, "{way}");
        assert_eq!(closing_rows(&c, &item), 0, "{way}: no item_lapsed");
        assert!(
            !calls_since(&c, m)
                .iter()
                .any(|call| matches!(call, Call::Cancel(_) | Call::Send(_))),
            "{way}"
        );
        assert_eq!(open_item(&c).0, item, "{way}: the card stays");
    }
}

fn store_an_unseen_failures_hold(c: &Coordinator<Both>, n: usize) {
    let mut events = c.ports().ports.events.borrow_mut();
    let last = events
        .iter()
        .rfind(|e| e.agent == agent(n))
        .cloned()
        .unwrap();
    events.push(halvo_coord::types::Event {
        seq: std::num::NonZeroU32::new(last.seq.get() + 1).unwrap(),
        what: What::HoldSet {
            cause: HoldSetCause::Failures,
        },
        ..last
    });
}

#[test]
fn rd62a_pin_a_start_whose_read_brings_a_hold_failures_answers_hold_and_the_next_press_starts() {
    use halvo_coord::types::{Cleared, Control};
    let slotted = support::A {
        person: true,
        slotted: true,
    };
    for by in ["run_now", "ready"] {
        let c = if by == "ready" {
            coordinator(&[slotted], Limits::PRODUCT)
        } else {
            coordinator(&[MANUAL], Limits::PRODUCT)
        };
        let ready = c.snapshot().items.first().map(|i| i.item().clone());
        store_an_unseen_failures_hold(&c, 0);
        let press = |c: &Coordinator<Both>| match &ready {
            None => c.control(&agent(0), Action::RunNow),
            Some(item) => c.answer(item, Choice::Start, None, shown_step_mark_or_zero(c, item)),
        };
        let before = events(&c).len();
        let m = mark(&c);
        assert_eq!(press(&c), Err(Cause::Hold), "{by}");
        assert_eq!(
            events(&c).len(),
            before,
            "{by}: the owner's rows gain nothing"
        );
        assert!(started(&c).is_empty(), "{by}: no Runs::start");
        assert!(calls_since(&c, m)
            .iter()
            .all(|call| !matches!(call, Call::Start(_))));
        let s = c.snapshot();
        let controls: Vec<Control> = s.rows[0].controls.iter().map(|t| t.control).collect();
        assert!(
            controls.contains(&Control::SwitchOn),
            "{by}: the row shows the hold: {controls:?}"
        );
        if ready.is_none() {
            assert_eq!(row_line(&s, 0), text("line.problem.failures"), "{by}");
        } else {
            assert_eq!(s.items.len(), 1, "{by}: the card stays open");
        }
        assert_eq!(press(&c), Ok(()), "{by}");
        let new: Vec<What> = events(&c)[before..].to_vec();
        assert_eq!(
            new.iter()
                .filter(|w| matches!(
                    w,
                    What::HoldCleared {
                        cause: Cleared::Failures
                    }
                ))
                .count(),
            1,
            "{by}: {new:?}"
        );
        assert!(
            new.iter().any(|w| matches!(w, What::RunStarted { .. })),
            "{by}: {new:?}"
        );
        assert_eq!(started(&c).len(), 1, "{by}: it starts");
    }
}

#[test]
fn rd62a_question_no_move_sends_paused_for_a_hold_failures_whose_acknowledgement_was_lost() {
    use halvo_coord::types::AgentState;
    let limits = Limits {
        fails_to_pause: 1,
        ..Limits::PRODUCT
    };
    for order in [support::Loss::Reply, support::Loss::Later] {
        let c = coordinator(&[MANUAL], limits);
        let run = start(&c, 0);
        let m = mark(&c);
        c.ports().ports.end_runner(&run);
        c.ports().loss.set(Some(order));
        let _ = c.on_run(&run, RunEvent::Ended(Some(Outcome::Failed)));
        c.ports().loss.set(None);
        let lost: Vec<halvo_coord::types::Event> = c.ports().lost.borrow_mut().drain(..).collect();
        c.ports().ports.events.borrow_mut().extend(lost);
        assert!(
            events(&c).iter().any(|w| matches!(
                w,
                What::HoldSet {
                    cause: HoldSetCause::Failures
                }
            )),
            "{order:?}: the owner holds the hold"
        );
        for k in 1..=3 {
            c.ports().ports.set_clock(T0 + k * 1_000, DAY);
            c.tick();
        }
        let _ = c.control(&agent(0), Action::RunNow);
        assert!(
            !states_sent(&c, m).contains(&AgentState::Paused),
            "{order:?}: {:?}",
            states_sent(&c, m)
        );
    }
}

fn controls_of(c: &Coordinator<Both>, n: usize) -> Vec<halvo_coord::types::Control> {
    let s = c.snapshot();
    let row = s.rows.iter().find(|r| r.agent == agent(n)).unwrap();
    row.controls.iter().map(|t| t.control).collect()
}

#[test]
/// Turned by ruling RD-133, Addition 1 item 5: a Stop of a queued agent takes its plain queued
/// start out of the queue (it stayed through the Stop by RD-48 (h)), stores nothing, and the next
/// end takes no start of it; a second Stop after a new start stops the new run.
fn rd48h_turned_a_stop_takes_the_plain_queued_start_out_of_the_queue() {
    use halvo_coord::types::Control;
    let one = Limits {
        max_live_runs: 1,
        ..Limits::PRODUCT
    };
    for second_stop in [false, true] {
        let c = coordinator(&[MANUAL, MANUAL], one);
        let first = start(&c, 0);
        assert_eq!(c.control(&agent(1), Action::RunNow), Ok(()));
        assert_eq!(started(&c).len(), 1, "agent 1 waits");
        let stored = events(&c).len();
        let controls = controls_of(&c, 1);
        assert!(controls.contains(&Control::SwitchOff), "{controls:?}");
        assert!(!controls.contains(&Control::Stop), "{controls:?}");
        assert_eq!(c.control(&agent(1), Action::Stop), Ok(()));
        assert_eq!(events(&c).len(), stored, "the Stop stores nothing");
        assert_eq!(c.queued(), vec![], "out of the queue");
        assert_ne!(row_line(&c.snapshot(), 1), text("line.waiting_turn.queued"));
        c.ports().ports.end_runner(&first);
        c.on_run(&first, RunEvent::Ended(Some(Outcome::Completed)))
            .unwrap();
        assert_eq!(started(&c).len(), 1, "the next end takes no start of it");
        if second_stop {
            assert_eq!(c.control(&agent(1), Action::RunNow), Ok(()));
            let new = started(&c).last().cloned().unwrap();
            let m = mark(&c);
            assert_eq!(c.control(&agent(1), Action::Stop), Ok(()));
            assert_eq!(
                cancels_of(&c, m, &new),
                1,
                "the second Stop stops the new run"
            );
        }
    }
}

#[test]
fn rd52e_a_stop_while_the_run_of_a_gone_workflow_is_stopping_is_unknown_agent() {
    let limits = Limits::PRODUCT;
    let c = coordinator(&[MANUAL], limits);
    let run = start(&c, 0);
    c.ports()
        .ports
        .workflows
        .borrow_mut()
        .retain(|w| w.id() != &support::flow(0));
    let m = mark(&c);
    c.tick();
    assert_eq!(cancels_of(&c, m, &run), 1, "the tick stops the run");
    assert_eq!(c.live(), 1, "stopping");
    let stored = events(&c).len();
    let m = mark(&c);
    assert_eq!(c.control(&agent(0), Action::Stop), Err(Cause::UnknownAgent));
    assert_eq!(events(&c).len(), stored, "nothing stored");
    assert_eq!(cancels_of(&c, m, &run), 0, "no second cancel");
    c.ports()
        .ports
        .set_clock(T0 + limits.cancel_grace_s * 1000, DAY);
    c.tick();
    assert_eq!(kills_of(&c, m, &run), 1, "the grace tick ends the run");
    assert_eq!(c.live(), 0);
    assert!(
        !events(&c)
            .iter()
            .any(|w| matches!(w, What::RunEnded { run: r, .. } if *r == run)),
        "no run_ended"
    );
}

#[test]
fn rd52a_the_start_of_a_ready_item_answers_budget_and_queue_full_as_its_row_says() {
    use halvo_coord::types::commands::LIST;
    let slotted = support::A {
        person: true,
        slotted: true,
    };
    let row = LIST.iter().find(|c| c.name == "agents_answer").unwrap();
    for (limits, cause) in [
        (
            Limits {
                starts_per_day: 0,
                ..Limits::PRODUCT
            },
            Cause::Budget,
        ),
        (
            Limits {
                max_live_runs: 1,
                queue_max: 0,
                ..Limits::PRODUCT
            },
            Cause::QueueFull,
        ),
    ] {
        let c = coordinator(&[MANUAL, slotted], limits);
        let (ready, kind) = open_item(&c);
        assert_eq!(kind, ItemKind::Ready);
        if cause == Cause::QueueFull {
            start(&c, 0);
        }
        assert_eq!(
            c.answer(
                &ready,
                Choice::Start,
                None,
                shown_step_mark_or_zero(&c, &ready)
            ),
            Err(cause)
        );
        assert!(row.errors.contains(&cause), "{cause:?} in the row");
        assert!(row.errors.contains(&Cause::Hold), "hold in the row");
    }
}

fn ask_confirm_with_send_effects(run: &RunId, n: usize, count: usize) -> RunEvent {
    let RunEvent::Confirm(one) = ask_confirm(run, n) else {
        unreachable!()
    };
    let effects = (0..count)
        .map(|k| halvo_coord::types::AskedEffect {
            effect: halvo_coord::types::EffectId::parse(&format!("{:032x}", 0x100 + k)).unwrap(),
            ..one.effects()[0].clone()
        })
        .collect();
    RunEvent::Confirm(ConfirmAsk::new(run.clone(), agent(n), effects).unwrap())
}

#[test]
fn rd73_a_confirm_request_of_more_effects_than_request_records_ends_its_run_with_kind() {
    let limits = Limits::PRODUCT;
    let most = limits.request_records as usize;
    let c = coordinator(&[MANUAL], limits);
    let run = start(&c, 0);
    let before = items_raised(&c);
    let m = mark(&c);
    assert_eq!(
        c.on_run(&run, ask_confirm_with_send_effects(&run, 0, most + 1)),
        Err(Cause::State)
    );
    assert_eq!(items_raised(&c), before, "no item_raised");
    assert!(c.snapshot().items.is_empty(), "no card");
    ends_with(&c, m, &run, RunEnd::Kind, T0);
    let c = coordinator(&[MANUAL], limits);
    let run = start(&c, 0);
    assert_eq!(
        c.on_run(&run, ask_confirm_with_send_effects(&run, 0, most)),
        Ok(())
    );
    assert_eq!(open_item(&c).1, ItemKind::Confirm, "the card of 8 effects");
}

#[test]
fn rd52b_a_switch_on_whose_hold_cleared_reply_is_lost_in_both_forms() {
    for order in [
        support::Loss::Reply,
        support::Loss::Later,
        support::Loss::Unread,
    ] {
        let c = coordinator(&[MANUAL], Limits::PRODUCT);
        assert_eq!(c.control(&agent(0), Action::SwitchOff), Ok(()), "{order:?}");
        let m = mark(&c);
        lose_the_next_holds_reply(&c, order);
        let out = c.control(&agent(0), Action::SwitchOn);
        c.ports().loss.set(None);
        if order == support::Loss::Reply {
            assert_eq!(out, Ok(()), "{order:?}");
            assert_eq!(states_sent(&c, m).len(), 1, "{order:?}");
        } else {
            assert_eq!(out, Err(Cause::NoService), "{order:?}");
            assert!(states_sent(&c, m).is_empty(), "{order:?}");
        }
    }
}

fn answer_every_setup_parameter(
    c: &Coordinator<Both>,
    item: &ItemId,
    refusal: Option<&str>,
) -> Result<(), Cause> {
    c.answer(
        item,
        Choice::Choose,
        Some((Param::Start, 1)),
        shown_step_mark_or_zero(c, item),
    )
    .unwrap();
    c.answer(
        item,
        Choice::Choose,
        Some((Param::Payee, 1)),
        shown_step_mark_or_zero(c, item),
    )
    .unwrap();
    if let Some(name) = refusal {
        c.ports()
            .ports
            .refuse(Method::Send, PortError::Refused(Name::parse(name).unwrap()));
    }
    let out = c.setup_typed(
        item,
        Param::Instrument,
        "Card 4242",
        shown_step_mark_or_zero(c, item),
    );
    c.ports().ports.clear_refusals();
    out
}

#[test]
fn rd87_two_refusals_in_a_row_give_three_marks_for_one_id() {
    let (c, _made, item) = setup_agent();
    let mut marks = vec![shown_step_mark_or_zero(&c, &item)];
    for _ in 0..2 {
        assert_eq!(
            answer_every_setup_parameter(&c, &item, Some("config")),
            Ok(())
        );
        assert_eq!(open_item(&c).0, item, "the same id asks again");
        marks.push(shown_step_mark_or_zero(&c, &item));
    }
    assert_eq!(marks, [0, 1, 2]);
}

#[test]
fn rd87_each_refusal_by_name_changes_the_mark_and_an_owed_body_does_not() {
    let by_name = [
        "config",
        "malformed",
        "too_large",
        "length",
        "overlap",
        "not_a_name_of_the_list",
    ];
    for name in by_name
        .into_iter()
        .chain(["busy", "store_unavailable", "internal", "state"])
    {
        let (c, _made, item) = setup_agent();
        let before = shown_step_mark_or_zero(&c, &item);
        let _ = answer_every_setup_parameter(&c, &item, Some(name));
        let after = shown_step_mark_or_zero(&c, &item);
        if by_name.contains(&name) {
            assert_eq!(after, before + 1, "{name}: asked again");
        } else {
            assert_eq!(after, before, "{name}: the body is owed");
        }
    }
}

#[test]
fn rd87_the_moves_that_ask_nothing_new_leave_the_mark() {
    let (c, made, item) = setup_agent();
    let before = shown_step_mark_or_zero(&c, &item);
    c.answer(&item, Choice::Choose, Some((Param::Start, 1)), before)
        .unwrap();
    assert_eq!(shown_step_mark_or_zero(&c, &item), before, "a taken answer");
    let name = |c: &Coordinator<Both>| {
        c.snapshot()
            .rows
            .iter()
            .find(|r| r.agent == made)
            .unwrap()
            .name
            .clone()
    };
    let old = name(&c);
    for w in c.ports().ports.workflows.borrow_mut().iter_mut() {
        if let Workflow::Full(f) = w {
            f.site = Some("site2.example".into());
        }
    }
    c.tick();
    assert_ne!(name(&c), old, "the agent's name changed");
    assert_eq!(shown_step_mark_or_zero(&c, &item), before, "a name");
    c.ports().ports.refuse(Method::Send, PortError::Busy);
    c.answer(&item, Choice::Choose, Some((Param::Payee, 1)), before)
        .unwrap();
    let _ = c.setup_typed(&item, Param::Instrument, "Card 4242", before);
    c.ports().ports.clear_refusals();
    assert_eq!(shown_step_mark_or_zero(&c, &item), before, "an owed body");
    let limits = Limits {
        max_live_runs: 2,
        ..Limits::PRODUCT
    };
    let c = coordinator(&[MANUAL, MANUAL], limits);
    let first = start(&c, 0);
    let second = start(&c, 1);
    c.on_run(&first, ask_confirm(&first, 0)).unwrap();
    c.on_run(&second, ask_confirm(&second, 1)).unwrap();
    let marks = |c: &Coordinator<Both>| -> Vec<u32> {
        c.snapshot().items.iter().map(|i| i.step()).collect()
    };
    let before = marks(&c);
    for i in c.snapshot().items {
        c.on_screen(i.item());
    }
    c.ports().ports.set_clock(T0 + limits.arm_ms + 1, DAY);
    c.tick();
    assert!(c
        .snapshot()
        .items
        .iter()
        .all(|i| i.armed() == halvo_coord::types::YesNo::Yes));
    assert_eq!(marks(&c), before, "armed");
    let one = c.snapshot().items[0].item().clone();
    assert_eq!(c.answer(&one, Choice::Reject, None, 0), Ok(()));
    assert_eq!(marks(&c), [before[1]], "a place among confirmations");
}

#[test]
fn rd87_an_answer_of_a_step_that_is_no_longer_current_is_refused_with_state() {
    let (c, _made, item) = setup_agent();
    assert_eq!(
        answer_every_setup_parameter(&c, &item, Some("config")),
        Ok(())
    );
    assert_eq!(shown_step_mark_or_zero(&c, &item), 1);
    let stored = events(&c).len();
    let m = mark(&c);
    assert_eq!(
        c.answer(&item, Choice::Choose, Some((Param::Start, 1)), 0),
        Err(Cause::State),
        "the answer of the step before the refusal"
    );
    assert_eq!(
        c.setup_typed(&item, Param::Instrument, "Card 4242", 0),
        Err(Cause::State)
    );
    assert_eq!(events(&c).len(), stored, "nothing stored");
    assert!(calls_since(&c, m)
        .iter()
        .all(|call| matches!(call, Call::Changed | Call::Attention(_))));
    assert_eq!(
        c.answer(&item, Choice::Choose, Some((Param::Start, 1)), 1),
        Ok(())
    );
    let (c, _run, item) = waiting("stuck");
    let m = mark(&c);
    assert_eq!(
        c.answer(&item, Choice::Decline, None, 1),
        Err(Cause::State),
        "a decline of a step that is not the item's"
    );
    assert!(handed_back(&c, m).is_empty());
    assert_eq!(c.answer(&item, Choice::Decline, None, 0), Ok(()));
}

fn ask_confirm_native_signature(run: &RunId, n: usize) -> RunEvent {
    let RunEvent::Confirm(one) = ask_confirm(run, n) else {
        unreachable!()
    };
    let slot = |name: &str| halvo_coord::types::BoundField {
        name: Name::parse(name).unwrap(),
        from: halvo_coord::types::BoundFrom::Slot,
        value: vec![format!("value of {name}")],
    };
    let sign = halvo_coord::types::AskedEffect {
        class: Class::Sign,
        fields: vec![slot("document"), slot("party")],
        ..one.effects()[0].clone()
    };
    RunEvent::Confirm(ConfirmAsk::new(run.clone(), agent(n), vec![sign]).unwrap())
}

fn run_waiting_on_a_confirmation(native: bool) -> (Coordinator<Both>, RunId, ItemId) {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    let ask = if native {
        ask_confirm_native_signature(&run, 0)
    } else {
        ask_confirm(&run, 0)
    };
    c.on_run(&run, ask).unwrap();
    let (item, kind) = open_item(&c);
    assert_eq!(kind, ItemKind::Confirm);
    (c, run, item)
}

#[test]
fn a_confirmations_say_new_and_say_gone_name_its_agent() {
    for native in [false, true] {
        let (c, _run, _item) = run_waiting_on_a_confirmation(native);
        let s = c.snapshot();
        let name = s.rows[0].name.clone();
        let item = &s.items[0];
        let new = words::text("say.item.new", &[("name", &name)]).unwrap();
        let gone = words::text("say.item.gone", &[("name", &name)]).unwrap();
        assert_eq!(item.say_new(), new, "native={native}");
        assert_eq!(item.say_gone(), gone, "native={native}");
    }
}

#[test]
fn rd80_a_page_answer_arms_in_its_own_move_from_the_report() {
    let arm = Limits::PRODUCT.arm_ms;
    let (c, _run, item) = run_waiting_on_a_confirmation(false);
    assert_eq!(c.shown_on_page(&item, 0), Ok(()));
    for (after, out) in [
        (100, Err(Cause::Early)),
        (arm - 1, Err(Cause::Early)),
        (arm, Ok(())),
    ] {
        c.ports().ports.set_clock(T0 + after, DAY);
        assert_eq!(c.answer(&item, Choice::Allow, None, 0), out, "{after} ms");
    }
    assert!(c.snapshot().items.is_empty(), "taken");
}

#[test]
fn rd80_an_answer_with_no_report_or_read_before_arm_ms_is_early() {
    let arm = Limits::PRODUCT.arm_ms;
    let (c, _run, item) = run_waiting_on_a_confirmation(false);
    c.ports().ports.set_clock(T0 + 10 * arm, DAY);
    assert_eq!(
        c.answer(&item, Choice::Allow, None, 0),
        Err(Cause::Early),
        "no report"
    );
    let (c, _run, item) = run_waiting_on_a_confirmation(false);
    assert_eq!(c.shown_on_page(&item, 0), Ok(()));
    c.ports().ports.set_clock(T0 + arm - 100, DAY);
    c.ports().ports.step_reads(200);
    assert_eq!(
        c.answer(&item, Choice::Allow, None, 0),
        Err(Cause::Early),
        "the clock read before the lock"
    );
}

#[test]
fn rd80_the_reports_causes_and_two_reports_keep_the_first() {
    let arm = Limits::PRODUCT.arm_ms;
    let (c, _run, item) = run_waiting_on_a_confirmation(false);
    let other = ItemId::parse(&format!("{:032x}", 0xbad)).unwrap();
    assert_eq!(c.shown_on_page(&other, 0), Err(Cause::UnknownItem));
    assert_eq!(c.shown_on_page(&item, 1), Err(Cause::State));
    c.ports().clock_fails.set(true);
    assert_eq!(c.shown_on_page(&item, 0), Err(Cause::NoService));
    c.ports().clock_fails.set(false);
    assert_eq!(c.shown_on_page(&item, 0), Ok(()), "the first report");
    c.ports().ports.set_clock(T0 + arm - 100, DAY);
    assert_eq!(c.shown_on_page(&item, 0), Ok(()), "a second report");
    c.ports().ports.set_clock(T0 + arm, DAY);
    assert_eq!(
        c.answer(&item, Choice::Allow, None, 0),
        Ok(()),
        "armed from the first report"
    );
    assert_eq!(c.shown_on_page(&item, 0), Err(Cause::Stale), "answered");
    let (c, _run, native) = run_waiting_on_a_confirmation(true);
    assert_eq!(c.shown_on_page(&native, 0), Err(Cause::Presenter));
    c.ports().ports.set_clock(T0 + arm, DAY);
    c.tick();
    assert_eq!(
        c.answer_native(&native, ConfirmAnswer::Allow),
        Err(Cause::Early),
        "no time noted"
    );
    let (c, _run, item) = run_waiting_on_a_confirmation(false);
    c.ports().drop_decision.set(true);
    c.shown_on_page(&item, 0).unwrap();
    c.ports().ports.set_clock(T0 + arm, DAY);
    assert_eq!(
        c.answer(&item, Choice::Allow, None, 0),
        Err(Cause::NoService)
    );
    assert_eq!(c.shown_on_page(&item, 0), Err(Cause::Stale), "recorded");
}

#[test]
fn rd80_a_hide_forgets_the_report_of_a_shown_item_and_keeps_an_armed_one() {
    let arm = Limits::PRODUCT.arm_ms;
    let (c, _run, item) = run_waiting_on_a_confirmation(false);
    c.shown_on_page(&item, 0).unwrap();
    c.page_hidden();
    c.ports().ports.set_clock(T0 + arm, DAY);
    assert_eq!(
        c.answer(&item, Choice::Allow, None, 0),
        Err(Cause::Early),
        "the report was forgotten"
    );
    c.shown_on_page(&item, 0).unwrap();
    c.ports().ports.set_clock(T0 + 2 * arm, DAY);
    assert_eq!(
        c.answer(&item, Choice::Allow, None, 0),
        Ok(()),
        "the new show"
    );
    let (c, _run, item) = run_waiting_on_a_confirmation(false);
    c.shown_on_page(&item, 0).unwrap();
    c.ports().ports.set_clock(T0 + arm, DAY);
    c.tick();
    assert_eq!(
        c.snapshot().items[0].armed(),
        halvo_coord::types::YesNo::Yes
    );
    c.page_hidden();
    assert_eq!(
        c.answer(&item, Choice::Allow, None, 0),
        Ok(()),
        "an armed item stays armed"
    );
}

#[test]
fn rd52b_rule3_a_request_whose_seq_read_brings_a_stored_switch_off_stores_no_item_of_it() {
    use halvo_coord::types::AgentState;
    for way in ["stuck", "confirm", "second_step"] {
        for order in NO_SERVICE_ROW_UNSEEN_ORDERS {
            let case = format!("{way} {order:?}");
            let c = coordinator(&[MANUAL], Limits::PRODUCT);
            let run = start(&c, 0);
            lose_the_next_holds_reply(&c, order);
            assert_eq!(
                c.control(&agent(0), Action::SwitchOff),
                Err(Cause::NoService),
                "{case}: the switch-off does not see its hold"
            );
            c.ports().loss.set(None);
            let lost: Vec<halvo_coord::types::Event> =
                c.ports().lost.borrow_mut().drain(..).collect();
            c.ports().ports.events.borrow_mut().extend(lost);
            assert_eq!(hold_sets(&c, 0), 1, "{case}: the owner holds the hold");
            let raised_before = events(&c)
                .iter()
                .filter(|w| matches!(w, What::ItemRaised { .. }))
                .count();
            let m = mark(&c);
            let event = match way {
                "stuck" => handback(&run, 0, HandbackReason::StepFailed),
                "confirm" => ask_confirm(&run, 0),
                _ => bind(&run, 0),
            };
            let _ = c.on_run(&run, event);
            let raised_after = events(&c)
                .iter()
                .filter(|w| matches!(w, What::ItemRaised { .. }))
                .count();
            assert_eq!(
                raised_after, raised_before,
                "{case}: no item_raised row is stored after the read that showed the hold"
            );
            assert_eq!(cancels_of(&c, m, &run), 1, "{case}: one cancel");
            assert_eq!(
                states_sent(&c, m),
                [AgentState::Paused],
                "{case}: paused once"
            );
            assert!(c.snapshot().items.is_empty(), "{case}: no card");
            assert_eq!(hold_sets(&c, 0), 1, "{case}: no second hold_set");
            c.tick();
            c.ports().ports.set_clock(T0 + 1_000, DAY);
            c.tick();
            assert_eq!(
                items_raised(&c),
                raised_before,
                "{case}: no item_raised at the next ticks"
            );
            let lapsed = |c: &Coordinator<Both>| {
                events(c)
                    .iter()
                    .filter(|w| matches!(w, What::ItemLapsed { .. }))
                    .count()
            };
            let lapsed_before = lapsed(&c);
            let life = next_life(&c, |_| {});
            assert_eq!(
                lapsed(&life),
                lapsed_before,
                "{case}: the next life lapses no item of it"
            );
        }
    }
}

#[test]
fn rd52b_rule3_a_tick_with_no_log_of_its_own_does_not_stop_the_run_of_an_unseen_switch_off() {
    for way in ["stuck", "confirm", "second_step"] {
        for order in NO_SERVICE_ROW_UNSEEN_ORDERS {
            let case = format!("{way} {order:?}");
            let (c, run, item) = waiting(way);
            assert_an_unseen_switch_off_does_nothing(&c, &item, order, &case);
            let m = mark(&c);
            c.tick();
            assert_eq!(cancels_of(&c, m, &run), 0, "{case}: no cancel");
            assert!(states_sent(&c, m).is_empty(), "{case}: no set_state");
            assert_eq!(closing_rows(&c, &item), 0, "{case}: no lapse");
            assert_eq!(open_item(&c).0, item, "{case}: the card stays");
            assert_ne!(
                row_line(&c.snapshot(), 0),
                text("line.working.stopping"),
                "{case}: the run is not stopping"
            );
        }
    }
}

#[test]
fn rd52b_rule3_a_ticks_flush_read_that_brings_a_switch_off_stops_before_the_wait_limit() {
    use halvo_coord::types::AgentState;
    for order in NO_SERVICE_ROW_UNSEEN_ORDERS {
        let case = format!("{order:?}");
        let c = coordinator(&[MANUAL], Limits::PRODUCT);
        let run = start(&c, 0);
        c.ports().ports.refuse(Method::Log, PortError::NoService);
        let _ = c.on_run(&run, handback(&run, 0, HandbackReason::StepFailed));
        c.ports().ports.clear_refusals();
        let (item, _) = open_item(&c);
        assert_eq!(
            closing_rows(&c, &item),
            0,
            "{case}: setup: the item is open and its row waits in the ledger"
        );
        assert_an_unseen_switch_off_does_nothing(&c, &item, order, &case);
        c.ports()
            .ports
            .set_clock(T0 + Limits::PRODUCT.wait_limit_s * 1000, DAY);
        let m = mark(&c);
        c.tick();
        assert_eq!(
            step_rows(&c, &item),
            ["lapsed:cancelled"],
            "{case}: the switch-off's lapse, not the wait limit's"
        );
        assert_eq!(cancels_of(&c, m, &run), 1, "{case}: one cancel");
        assert_eq!(
            states_sent(&c, m),
            [AgentState::Paused],
            "{case}: paused once"
        );
        assert_eq!(hold_sets(&c, 0), 1, "{case}: no second hold_set");
    }
}

#[test]
fn rd52d_a_switch_off_drops_a_queued_start_and_a_switch_on_brings_none_back() {
    let one = Limits {
        max_live_runs: 1,
        ..Limits::PRODUCT
    };
    let auto = support::A {
        person: false,
        slotted: true,
    };
    for by in ["person", "scheduler"] {
        let c = if by == "person" {
            let c = coordinator(&[MANUAL, MANUAL], one);
            start(&c, 0);
            assert_eq!(c.control(&agent(1), Action::RunNow), Ok(()), "{by}");
            c
        } else {
            coordinator(&[auto, auto], one)
        };
        let first = started(&c)[0].clone();
        assert_eq!(started(&c).len(), 1, "{by}: agent 1 waits");
        assert_eq!(
            row_line(&c.snapshot(), 1),
            text("line.waiting_turn.queued"),
            "{by}"
        );
        let before: Vec<What> = c
            .ports()
            .ports
            .events
            .borrow()
            .iter()
            .filter(|e| e.agent == agent(1))
            .map(|e| e.what.clone())
            .collect();
        assert_eq!(c.control(&agent(1), Action::SwitchOff), Ok(()), "{by}");
        let after: Vec<What> = c
            .ports()
            .ports
            .events
            .borrow()
            .iter()
            .filter(|e| e.agent == agent(1))
            .map(|e| e.what.clone())
            .collect();
        assert_eq!(
            after[before.len()..],
            [What::HoldSet {
                cause: HoldSetCause::Person
            }],
            "{by}: the hold alone, nothing for the start"
        );
        assert_ne!(
            row_line(&c.snapshot(), 1),
            text("line.waiting_turn.queued"),
            "{by}: the start left the queue"
        );
        assert_eq!(c.control(&agent(1), Action::SwitchOn), Ok(()), "{by}");
        c.ports().ports.end_runner(&first);
        c.on_run(&first, RunEvent::Ended(Some(Outcome::Completed)))
            .unwrap();
        assert_eq!(started(&c).len(), 1, "{by}: no start at that end");
    }
}

#[test]
fn rd52d_after_the_drop_and_a_switch_on_the_persons_start_is_taken_again() {
    let one = Limits {
        max_live_runs: 1,
        ..Limits::PRODUCT
    };
    let c = coordinator(&[MANUAL, MANUAL], one);
    let first = start(&c, 0);
    assert_eq!(c.control(&agent(1), Action::RunNow), Ok(()));
    assert_eq!(c.queued(), vec![(agent(1), false)]);
    assert_eq!(c.control(&agent(1), Action::SwitchOff), Ok(()));
    assert_eq!(c.queued(), vec![]);
    assert_eq!(c.control(&agent(1), Action::SwitchOn), Ok(()));
    assert_eq!(
        c.control(&agent(1), Action::RunNow),
        Ok(()),
        "taken again, not state"
    );
    assert_eq!(c.queued(), vec![(agent(1), false)]);
    c.ports().ports.end_runner(&first);
    c.on_run(&first, RunEvent::Ended(Some(Outcome::Completed)))
        .unwrap();
    assert_eq!(started(&c).len(), 2, "the queued start goes at that end");
}

#[test]
fn rd52d_a_read_that_first_shows_the_hold_of_a_queued_agent_drops_its_start() {
    let one = Limits {
        max_live_runs: 1,
        ..Limits::PRODUCT
    };
    let slotted = support::A {
        person: true,
        slotted: true,
    };
    let c = coordinator(&[MANUAL, slotted], one);
    start(&c, 0);
    c.tick();
    let (item, kind) = open_item(&c);
    assert_eq!(kind, ItemKind::Ready);
    c.ports().ports.refuse(Method::Log, PortError::NoService);
    let _ = c.answer(&item, Choice::Start, None, 0);
    c.ports().ports.clear_refusals();
    assert_eq!(c.queued(), vec![(agent(1), false)], "queued with its item");
    let last = c
        .ports()
        .ports
        .events
        .borrow()
        .iter()
        .filter(|e| e.agent == agent(1))
        .map(|e| e.seq.get())
        .max()
        .unwrap_or(0);
    c.ports()
        .ports
        .events
        .borrow_mut()
        .push(halvo_coord::types::Event {
            workflow: support::flow(1),
            agent: agent(1),
            seq: std::num::NonZeroU32::new(last + 1).unwrap(),
            day: DAY,
            at: 0,
            what: What::HoldSet {
                cause: HoldSetCause::Person,
            },
        });
    let _ = c.answer(
        &item,
        Choice::Skip,
        None,
        shown_step_mark_or_zero(&c, &item),
    );
    assert_eq!(c.queued(), vec![], "the start left the queue");
    assert_ne!(row_line(&c.snapshot(), 1), text("line.waiting_turn.queued"));
}

fn queued_with_its_ready_item(c: &Coordinator<Both>) -> ItemId {
    start(c, 0);
    c.tick();
    let (item, kind) = open_item(c);
    assert_eq!(kind, ItemKind::Ready);
    c.ports().ports.refuse(Method::Log, PortError::NoService);
    let _ = c.answer(&item, Choice::Start, None, 0);
    c.ports().ports.clear_refusals();
    assert_eq!(c.queued(), vec![(agent(1), false)], "queued with its item");
    item
}

#[test]
fn rd102_a_skip_on_the_card_of_a_queued_agent_drops_its_start() {
    let one = Limits {
        max_live_runs: 1,
        ..Limits::PRODUCT
    };
    let slotted = support::A {
        person: true,
        slotted: true,
    };
    let c = coordinator(&[MANUAL, slotted], one);
    let item = queued_with_its_ready_item(&c);
    let skipped = c.answer(
        &item,
        Choice::Skip,
        None,
        shown_step_mark_or_zero(&c, &item),
    );
    assert_eq!(skipped, Ok(()), "the skip is stored");
    assert_eq!(c.queued(), vec![], "the skip drops the queued start");
    assert_eq!(
        row_line(&c.snapshot(), 1),
        text("line.all_set.done"),
        "the row leaves queued in the same reply: all set, with no item and no start"
    );
}

#[test]
fn rd99_an_end_whose_read_first_shows_a_stored_switch_off_stops_nothing_and_sends_no_state() {
    use halvo_coord::types::Control;
    for way in ["stuck", "working"] {
        for order in NO_SERVICE_ROW_UNSEEN_ORDERS {
            let case = format!("{way} {order:?}");
            let (c, run, item) = if way == "stuck" {
                let (c, run, item) = waiting("stuck");
                assert_an_unseen_switch_off_does_nothing(&c, &item, order, &case);
                (c, run, Some(item))
            } else {
                let c = coordinator(&[MANUAL], Limits::PRODUCT);
                let run = start(&c, 0);
                lose_the_next_holds_reply(&c, order);
                assert_eq!(
                    c.control(&agent(0), Action::SwitchOff),
                    Err(Cause::NoService),
                    "{case}"
                );
                c.ports().loss.set(None);
                let lost: Vec<halvo_coord::types::Event> =
                    c.ports().lost.borrow_mut().drain(..).collect();
                c.ports().ports.events.borrow_mut().extend(lost);
                assert_eq!(hold_sets(&c, 0), 1, "{case}: the owner holds the hold");
                assert_ne!(
                    row_line(&c.snapshot(), 0),
                    text("line.working.stopping"),
                    "{case}: the board does not hold it"
                );
                (c, run, None)
            };
            let m = mark(&c);
            c.ports().ports.end_runner(&run);
            let _ = c.on_run(&run, RunEvent::Ended(Some(Outcome::Completed)));
            assert!(states_sent(&c, m).is_empty(), "{case}: no set_state");
            assert_eq!(cancels_of(&c, m, &run), 0, "{case}: no cancel");
            if let Some(item) = &item {
                assert_eq!(
                    step_rows(&c, item),
                    ["lapsed:run_ended"],
                    "{case}: the item lapses with run_ended once"
                );
            }
            let ends: Vec<RunEnd> = events(&c)
                .iter()
                .filter_map(|w| match w {
                    What::RunEnded {
                        run: r,
                        cause,
                        defers: _,
                    } if *r == run => Some(*cause),
                    _ => None,
                })
                .collect();
            assert_eq!(ends, [RunEnd::Completed], "{case}: the run's own end");
            let s = c.snapshot();
            assert_eq!(row_line(&s, 0), text("line.off.person"), "{case}: Off");
            let controls = controls_of(&c, 0);
            assert!(
                controls.contains(&Control::SwitchOn) && controls.contains(&Control::Delete),
                "{case}: {controls:?}"
            );
            assert!(!controls.contains(&Control::RunNow), "{case}: {controls:?}");
            let started_before = started(&c).len();
            let raised_before = events(&c)
                .iter()
                .filter(|w| matches!(w, What::ItemRaised { .. }))
                .count();
            c.ports().ports.set_clock(T0 + 1_000, DAY);
            c.tick();
            assert_eq!(
                started(&c).len(),
                started_before,
                "{case}: the tick starts nothing"
            );
            let raised_after = events(&c)
                .iter()
                .filter(|w| matches!(w, What::ItemRaised { .. }))
                .count();
            assert_eq!(
                raised_after, raised_before,
                "{case}: the tick raises nothing"
            );
            assert!(c.snapshot().items.is_empty(), "{case}: no card");
            assert_eq!(
                c.control(&agent(0), Action::RunNow),
                Err(Cause::Hold),
                "{case}: Start now answers hold"
            );
        }
    }
}

#[test]
fn a_wait_limit_lapse_between_two_clock_reads_of_a_tick_holds_the_boards_stop() {
    let limits = Limits::PRODUCT;
    let limit = T0 + limits.wait_limit_s * 1000;
    let mut window = Vec::new();
    for k in 1..400u64 {
        let c = coordinator(&[MANUAL], limits);
        let run = start(&c, 0);
        c.on_run(&run, ask_confirm(&run, 0)).unwrap();
        let (item, _) = open_item(&c);
        let m = mark(&c);
        c.ports().ports.set_clock(limit - k, DAY);
        c.ports().ports.step_reads(1);
        c.tick();
        c.ports().ports.step_reads(0);
        let lapsed = events(&c).contains(&What::ItemLapsed {
            item: item.clone(),
            lapsed: Lapsed::Confirm(LapseCause::WaitLimit),
        });
        if !lapsed {
            continue;
        }
        let line = row_line(&c.snapshot(), 0);
        let cancels = cancels_of(&c, m, &run);
        if line != text("line.working.stopping") {
            let m2 = mark(&c);
            c.ports()
                .ports
                .set_clock(limit + limits.cancel_grace_s * 1000 + 1_000, DAY);
            c.tick();
            window.push(format!(
                "k={k}: lapsed with wait_limit, cancels={cancels}, row line {line:?}, kills at grace {}, live after grace {}",
                kills_of(&c, m2, &run),
                c.live()
            ));
        }
    }
    assert!(
        window.is_empty(),
        "a lapse at wait_limit that cancelled its run with no stop on the board: {window:#?}"
    );
}

fn tick_with_one_failed_clock_read(at: u64, p: u32) -> (Coordinator<Both>, RunId, ItemId, usize) {
    let (c, run, item) = run_waiting_on_a_confirmation(false);
    c.ports().ports.set_clock(at, DAY);
    let m = mark(&c);
    c.ports().fail_read_at.set(Some(p));
    c.tick();
    c.ports().fail_read_at.set(None);
    (c, run, item, m)
}

fn confirmation_tick_outcome(
    c: &Coordinator<Both>,
    run: &RunId,
    item: &ItemId,
    m: usize,
) -> (bool, bool, usize, Vec<String>) {
    let s = c.snapshot();
    let stopping = row_line(&s, 0) == text("line.working.stopping");
    let card = s.items.iter().any(|i| i.item() == item);
    (stopping, card, cancels_of(c, m, run), step_rows(c, item))
}

#[test]
fn a_tick_at_the_wait_limit_with_one_failed_clock_read_holds_no_stop_without_its_lapse() {
    let at = T0 + Limits::PRODUCT.wait_limit_s * 1000;
    for p in 0..40u32 {
        let (c, run, item, m) = tick_with_one_failed_clock_read(at, p);
        let outcome = confirmation_tick_outcome(&c, &run, &item, m);
        let whole = outcome == (true, false, 1, vec!["lapsed:wait_limit".to_string()]);
        let none = outcome == (false, true, 0, Vec::<String>::new());
        assert!(whole || none, "read {p} failed: {outcome:?}");
    }
}

#[test]
fn a_tick_past_the_wait_limit_whose_reading_fails_makes_no_move_and_the_next_tick_lapses() {
    let at = T0 + Limits::PRODUCT.wait_limit_s * 1000 + 5_000;
    let mut no_move = 0;
    for p in 0..40u32 {
        let (c, run, item, m) = tick_with_one_failed_clock_read(at, p);
        let outcome = confirmation_tick_outcome(&c, &run, &item, m);
        if outcome == (false, true, 0, Vec::<String>::new()) {
            no_move += 1;
            c.ports().ports.set_clock(at + 1_000, DAY);
            c.tick();
            assert_eq!(
                confirmation_tick_outcome(&c, &run, &item, m),
                (true, false, 1, vec!["lapsed:wait_limit".to_string()]),
                "read {p} failed, then the next tick"
            );
        } else {
            assert_eq!(
                outcome,
                (true, false, 1, vec!["lapsed:wait_limit".to_string()]),
                "read {p} failed"
            );
        }
    }
    assert!(no_move > 0, "no failed read made the tick hold back");
}

#[test]
fn a_second_mark_of_a_stopping_run_keeps_the_time_of_its_first_stop() {
    let limits = Limits::PRODUCT;
    let (c, run, _item) = waiting("second_step");
    let at = T0 + limits.wait_limit_s * 1000;
    c.ports().ports.set_clock(at, DAY);
    c.ports().panics.set(Some("answer_bind"));
    let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| c.tick()));
    c.ports().panics.set(None);
    assert!(out.is_err(), "the port's answer_bind panicked");
    c.ports().ports.set_clock(at + 1_000, DAY);
    c.tick();
    let m = mark(&c);
    c.ports()
        .ports
        .set_clock(at + limits.cancel_grace_s * 1000, DAY);
    c.tick();
    assert_eq!(
        kills_of(&c, m, &run),
        1,
        "the grace counts from the first stop"
    );
}

fn allowed_records(
    c: &Coordinator<Both>,
    run: &RunId,
    item: &ItemId,
) -> [halvo_coord::guard::EffectRecord; 2] {
    let entry = events(c)
        .iter()
        .find_map(|w| match w {
            What::ItemAnswered {
                item: i,
                answer: halvo_coord::types::Answered::Confirm(e),
            } if i == item => Some(e.clone()),
            _ => None,
        })
        .expect("the allow is stored");
    let record = |state| halvo_coord::guard::EffectRecord {
        run: run.clone(),
        agent: agent(0),
        effect: entry.effect.clone(),
        state,
        class: Class::Send,
        digest: entry.digest.clone(),
        approval: Some(entry.approval.clone()),
    };
    [
        record(halvo_coord::guard::EffectState::Approved),
        record(halvo_coord::guard::EffectState::Releasing),
    ]
}

#[test]
fn a_switch_off_of_a_working_run_makes_the_guard_refuse_its_allowed_and_its_running_effects() {
    let limits = Limits::PRODUCT;
    let mut both = owner(&[MANUAL], limits);
    both.policy = halvo_coord::mem::policy::Policy::new(&[(
        halvo_coord::types::Autonomy::DraftAndConfirm,
        Class::Prepare,
        true,
    )]);
    let c = Coordinator::new(both, limits);
    c.tick();
    let (run, item) = allowed(&c);
    assert_eq!(
        c.answer(
            &item,
            Choice::Allow,
            None,
            shown_step_mark_or_zero(&c, &item)
        ),
        Ok(())
    );
    let [approved, releasing] = allowed_records(&c, &run, &item);
    let running = halvo_coord::guard::EffectRecord {
        run: run.clone(),
        agent: agent(0),
        effect: halvo_coord::types::EffectId::parse(&format!("{:032x}", 0xe9)).unwrap(),
        state: halvo_coord::guard::EffectState::Releasing,
        class: Class::Prepare,
        digest: halvo_coord::types::Digest::parse(&"cd".repeat(32)).unwrap(),
        approval: None,
    };
    for record in [&approved, &releasing, &running] {
        assert_eq!(
            c.check_record(record),
            Ok(()),
            "{:?} while the run works",
            record.state
        );
    }
    assert!(c.snapshot().items.is_empty(), "the run works on, no card");
    assert_eq!(c.control(&agent(0), Action::SwitchOff), Ok(()));
    for record in [&approved, &releasing, &running] {
        assert_eq!(
            c.check_record(record),
            Err(halvo_coord::guard::Refusal::Approval),
            "{:?} {:?} after the switch-off",
            record.class,
            record.state
        );
    }
}

#[test]
fn a_switch_off_whose_stuck_items_lapse_row_panics_leaves_the_guard_refusing_an_earlier_allow() {
    let limits = Limits::PRODUCT;
    let c = coordinator(&[MANUAL], limits);
    let (run, item) = allowed(&c);
    assert_eq!(
        c.answer(
            &item,
            Choice::Allow,
            None,
            shown_step_mark_or_zero(&c, &item)
        ),
        Ok(())
    );
    let records = allowed_records(&c, &run, &item);
    c.on_run(&run, handback(&run, 0, HandbackReason::StepFailed))
        .unwrap();
    assert_eq!(open_item(&c).1, ItemKind::Stuck);
    c.ports().panics.set(Some("log_lapsed"));
    let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = c.control(&agent(0), Action::SwitchOff);
    }));
    c.ports().panics.set(None);
    assert!(out.is_err(), "the log of item_lapsed panicked");
    for record in &records {
        assert_eq!(
            c.check_record(record),
            Err(halvo_coord::guard::Refusal::Approval),
            "{:?} after the switch-off",
            record.state
        );
    }
    let m = mark(&c);
    c.ports()
        .ports
        .set_clock(T0 + 1_000 + limits.cancel_grace_s * 1000, DAY);
    c.tick();
    assert_eq!(kills_of(&c, m, &run), 1, "the grace tick kills the run");
}

#[test]
fn a_switch_off_that_drops_a_queued_start_keeps_its_ready_card_whose_start_answers_hold() {
    use halvo_coord::types::Control;
    let one = Limits {
        max_live_runs: 1,
        ..Limits::PRODUCT
    };
    let slotted = support::A {
        person: true,
        slotted: true,
    };
    let c = coordinator(&[MANUAL, slotted], one);
    start(&c, 0);
    c.tick();
    let (item, kind) = open_item(&c);
    assert_eq!(kind, ItemKind::Ready);
    c.ports().ports.refuse(Method::Log, PortError::NoService);
    let _ = c.answer(&item, Choice::Start, None, 0);
    c.ports().ports.clear_refusals();
    assert_eq!(c.queued(), vec![(agent(1), false)], "queued with its item");
    assert_eq!(c.control(&agent(1), Action::SwitchOff), Ok(()));
    assert_eq!(c.queued(), vec![], "the queued start is dropped");
    assert_eq!(open_item(&c).0, item, "the ready card stays");
    let controls = controls_of(&c, 1);
    assert!(
        controls.contains(&Control::SwitchOn) && controls.contains(&Control::Delete),
        "{controls:?}"
    );
    assert!(!controls.contains(&Control::RunNow), "{controls:?}");
    assert_eq!(
        c.answer(
            &item,
            Choice::Start,
            None,
            shown_step_mark_or_zero(&c, &item)
        ),
        Err(Cause::Hold)
    );
    assert_eq!(closing_rows(&c, &item), 0, "no closing row of the item");
    assert_eq!(c.control(&agent(1), Action::SwitchOn), Ok(()));
    c.ports().ports.set_clock(T0 + 1_000, DAY);
    c.tick();
    let raised = c
        .ports()
        .ports
        .events
        .borrow()
        .iter()
        .filter(|e| {
            e.agent == agent(1)
                && matches!(
                    e.what,
                    What::ItemRaised {
                        raised: halvo_coord::types::Raised::Ready,
                        ..
                    }
                )
        })
        .count();
    assert_eq!(raised, 1, "one ready item, not a second beside it");
}

#[test]
fn rd52b_an_allow_on_a_second_step_whose_read_shows_a_stored_switch_off_stops_the_run_first() {
    use halvo_coord::types::AgentState;
    for order in NO_SERVICE_ROW_UNSEEN_ORDERS {
        let case = format!("{order:?}");
        let (c, run, item) = waiting("second_step");
        assert_an_unseen_switch_off_does_nothing(&c, &item, order, &case);
        let m = mark(&c);
        let answered = c.answer(
            &item,
            Choice::Allow,
            None,
            shown_step_mark_or_zero(&c, &item),
        );
        assert_eq!(
            answered,
            Err(Cause::UnknownItem),
            "{case}: as for an item no longer open"
        );
        assert_eq!(allows(&c), 0, "{case}: no allow goes out");
        assert!(
            !step_rows(&c, &item).iter().any(|r| r == "allow"),
            "{case}: no answer row: {:?}",
            step_rows(&c, &item)
        );
        assert_eq!(
            step_rows(&c, &item),
            ["lapsed:cancelled"],
            "{case}: the step lapses with the switch-off"
        );
        assert_eq!(cancels_of(&c, m, &run), 1, "{case}: one cancel");
        assert_eq!(
            states_sent(&c, m),
            [AgentState::Paused],
            "{case}: paused once"
        );
        assert!(c.off_beside_live().is_empty(), "{case}");
        assert!(c.snapshot().items.is_empty(), "{case}: no card");
    }
}

#[test]
fn rd101_5b_an_answer_on_a_confirmation_whose_read_shows_a_stored_switch_off_stops_the_run_first() {
    for native in [false, true] {
        for choice in [Choice::Allow, Choice::Reject] {
            for order in NO_SERVICE_ROW_UNSEEN_ORDERS {
                let case = format!("native={native} {choice:?} {order:?}");
                let (c, run, item) = run_waiting_on_a_confirmation(native);
                assert_an_unseen_switch_off_does_nothing(&c, &item, order, &case);
                if native {
                    c.on_screen(&item);
                } else {
                    assert_eq!(c.shown_on_page(&item, 0), Ok(()), "{case}");
                }
                c.ports().ports.set_clock(T0 + Limits::PRODUCT.arm_ms, DAY);
                if native {
                    c.tick();
                }
                let (m, from) = (mark(&c), c.ports().ports.events.borrow().len());
                let out = if native {
                    let answer = if choice == Choice::Allow {
                        ConfirmAnswer::Allow
                    } else {
                        ConfirmAnswer::Reject
                    };
                    c.answer_native(&item, answer)
                } else {
                    c.answer(&item, choice, None, shown_step_mark_or_zero(&c, &item))
                };
                assert_eq!(
                    out,
                    Err(Cause::UnknownItem),
                    "{case}: as for an item no longer open"
                );
                no_answer_of_the_press_before_the_cancel(&c, m, &run, &case);
                assert_eq!(
                    item_rows_since(&c, from, &item),
                    ["lapsed:cancelled"],
                    "{case}: no row of the press, and the item lapses with the switch-off"
                );
                assert_the_stop_came_first(&c, m, &run, &case);
            }
        }
    }
}

/// The runner gets no decision before the run's one cancel, and every decision after it is the
/// lapse's, which rejects each effect (rulings RD-52 (b) rule 3, RD-83, RD-101 item 5 (b)).
fn no_answer_of_the_press_before_the_cancel(
    c: &Coordinator<Both>,
    m: usize,
    run: &RunId,
    case: &str,
) {
    let answered = places_of_calls_since(c, m, |call| matches!(call, Call::AnswerConfirm(..)));
    let cancels = places_of_calls_since(c, m, |call| matches!(call, Call::Cancel(r) if r == run));
    assert_eq!(cancels.len(), 1, "{case}: one cancel");
    assert!(
        answered.iter().all(|k| *k > cancels[0]),
        "{case}: no decision before the cancel: {answered:?} {cancels:?}"
    );
    for call in calls_since(c, m) {
        if let Call::AnswerConfirm(_, _, d) = call {
            assert!(
                d.approvals()
                    .iter()
                    .all(|e| e.answer == halvo_coord::types::EffectAnswer::RejectOnce),
                "{case}: the lapse's decision allows nothing: {d:?}"
            );
        }
    }
}

#[test]
fn rd101_5b_a_confirmation_whose_read_before_the_answer_fails_sends_nothing_and_the_read_back_stops_the_run_first(
) {
    for native in [false, true] {
        let case = format!("native={native}");
        let (c, run, item) = run_waiting_on_a_confirmation(native);
        switch_off_stored_unseen(&c, support::Loss::Later, &case);
        if native {
            c.on_screen(&item);
        } else {
            assert_eq!(c.shown_on_page(&item, 0), Ok(()), "{case}");
        }
        c.ports().ports.set_clock(T0 + Limits::PRODUCT.arm_ms, DAY);
        if native {
            c.tick();
        }
        let press = |c: &Coordinator<Both>| {
            if native {
                c.answer_native(&item, ConfirmAnswer::Allow)
            } else {
                c.answer(
                    &item,
                    Choice::Allow,
                    None,
                    shown_step_mark_or_zero(c, &item),
                )
            }
        };
        let (m, from) = (mark(&c), c.ports().ports.events.borrow().len());
        c.ports().next_history_fails.set(true);
        assert_eq!(press(&c), Err(Cause::NoService), "{case}: the read's cause");
        c.ports().next_history_fails.set(false);
        no_answer_of_the_press_before_the_cancel(&c, m, &run, &case);
        assert_eq!(
            item_rows_since(&c, from, &item),
            ["lapsed:cancelled"],
            "{case}: no row of the press, and the item lapses with the switch-off"
        );
        assert_the_stop_came_first(&c, m, &run, &case);
    }
}

#[test]
fn a_stop_or_a_wait_limit_tick_whose_read_shows_a_stored_switch_off_lapses_with_its_own_cause_and_sends_no_state(
) {
    use halvo_coord::types::Control;
    let limits = Limits::PRODUCT;
    for way in WAIT_WAYS_BY_LAPSE {
        for by in ["stop", "wait_limit"] {
            for order in NO_SERVICE_ROW_UNSEEN_ORDERS {
                let case = format!("{way} {by} {order:?}");
                let (c, run, item) = waiting(way);
                assert_an_unseen_switch_off_does_nothing(&c, &item, order, &case);
                let m = mark(&c);
                if by == "stop" {
                    assert_eq!(c.control(&agent(0), Action::Stop), Ok(()), "{case}");
                } else {
                    c.ports()
                        .ports
                        .set_clock(T0 + limits.wait_limit_s * 1000, DAY);
                    c.tick();
                }
                assert!(states_sent(&c, m).is_empty(), "{case}: no set_state");
                assert_eq!(cancels_of(&c, m, &run), 1, "{case}: one cancel");
                let cause = if by == "stop" {
                    "cancelled"
                } else {
                    "wait_limit"
                };
                assert_eq!(
                    step_rows(&c, &item),
                    [format!("lapsed:{cause}")],
                    "{case}: the move's own cause, once"
                );
                assert_eq!(hold_sets(&c, 0), 1, "{case}");
                let s = c.snapshot();
                assert_eq!(row_line(&s, 0), text("line.off.person"), "{case}");
                let controls = controls_of(&c, 0);
                assert!(
                    controls.contains(&Control::Stop)
                        && controls.contains(&Control::SwitchOn)
                        && controls.contains(&Control::Delete)
                        && !controls.contains(&Control::RunNow),
                    "{case}: {controls:?}"
                );
            }
        }
    }
}

#[test]
fn a_stored_went_through_whose_reply_was_lost_then_try_again_stores_no_second_row_and_hands_went_through(
) {
    use halvo_coord::types::HandbackResult;
    for order in NO_SERVICE_ROW_UNSEEN_ORDERS {
        let c = coordinator(&[MANUAL], Limits::PRODUCT);
        let run = start(&c, 0);
        c.on_run(&run, handback(&run, 0, HandbackReason::EffectUnknown))
            .unwrap();
        let (item, kind) = open_item(&c);
        assert_eq!(kind, ItemKind::Check, "{order:?}");
        c.ports().loss.set(Some(order));
        assert_eq!(
            c.answer(
                &item,
                Choice::WentThrough,
                None,
                shown_step_mark_or_zero(&c, &item)
            ),
            Err(Cause::NoService),
            "{order:?}"
        );
        let lost: Vec<halvo_coord::types::Event> = c.ports().lost.borrow_mut().drain(..).collect();
        c.ports().ports.events.borrow_mut().extend(lost);
        assert_eq!(closing_rows(&c, &item), 1, "{order:?}: the owner holds it");
        let m = mark(&c);
        assert_eq!(
            c.answer(
                &item,
                Choice::TryAgain,
                None,
                shown_step_mark_or_zero(&c, &item)
            ),
            Err(Cause::UnknownItem),
            "{order:?}"
        );
        assert_eq!(closing_rows(&c, &item), 1, "{order:?}: no second row");
        assert_eq!(
            handed_back(&c, m),
            [HandbackResult::Declined],
            "{order:?}: the stored went_through"
        );
    }
}

#[test]
fn a_handback_answer_stored_with_no_read_lets_the_next_switch_off_store_its_hold_at_its_first_log()
{
    for reason in DECLINE_DONE {
        let c = coordinator(&[MANUAL], Limits::PRODUCT);
        let run = start(&c, 0);
        c.on_run(&run, handback(&run, 0, reason)).unwrap();
        let (item, _) = open_item(&c);
        let tried = c.ports().ports.tried.borrow().len();
        assert_eq!(
            c.answer(
                &item,
                Choice::Done,
                None,
                shown_step_mark_or_zero(&c, &item)
            ),
            Ok(()),
            "{reason:?}"
        );
        assert!(
            !c.ports().ports.tried.borrow()[tried..]
                .iter()
                .any(|t| t.refused.is_some()),
            "{reason:?}: the answer's log was acknowledged with no read"
        );
        let tried = c.ports().ports.tried.borrow().len();
        assert_eq!(
            c.control(&agent(0), Action::SwitchOff),
            Ok(()),
            "{reason:?}"
        );
        let after: Vec<(Call, bool)> = c.ports().ports.tried.borrow()[tried..]
            .iter()
            .map(|t| (t.call.clone(), t.refused.is_some()))
            .collect();
        let hold = after
            .iter()
            .position(|(call, _)| {
                matches!(call, Call::Log(rows) if rows.iter().any(|e| matches!(e.what, What::HoldSet { .. })))
            })
            .expect("hold_set is logged");
        assert!(!after[hold].1, "{reason:?}: acknowledged at its first log");
        assert!(
            !after[..hold]
                .iter()
                .any(|(call, _)| matches!(call, Call::History(..))),
            "{reason:?}: no read before it"
        );
    }
}

#[test]
fn a_tick_whose_confirmation_flush_reads_a_stored_switch_off_stops_the_run_at_its_end() {
    use halvo_coord::types::AgentState;
    for order in NO_SERVICE_ROW_UNSEEN_ORDERS {
        let case = format!("{order:?}");
        let c = coordinator(&[MANUAL], Limits::PRODUCT);
        let run = start(&c, 0);
        c.ports().loss.set(Some(support::Loss::Nothing));
        c.on_run(&run, ask_confirm(&run, 0)).unwrap();
        c.ports().loss.set(None);
        let (item, kind) = open_item(&c);
        assert_eq!(kind, ItemKind::Confirm, "{case}");
        assert_eq!(items_raised(&c), 0, "{case}: the raise waits in the ledger");
        assert_an_unseen_switch_off_does_nothing(&c, &item, order, &case);
        let m = mark(&c);
        c.ports().loss.set(Some(support::Loss::Nothing));
        c.ports().next_history_fails.set(true);
        c.tick();
        c.ports().loss.set(None);
        c.ports().next_history_fails.set(false);
        assert!(
            c.off_beside_live().is_empty(),
            "{case}: no run beside the hold"
        );
        assert_eq!(cancels_of(&c, m, &run), 1, "{case}: one cancel");
        assert_eq!(
            states_sent(&c, m),
            [AgentState::Paused],
            "{case}: paused once"
        );
    }
}

#[test]
fn a_press_whose_read_shows_a_known_switch_off_beside_a_live_run_stops_that_run() {
    use halvo_coord::types::AgentState;
    for order in NO_SERVICE_ROW_UNSEEN_ORDERS {
        let case = format!("{order:?}");
        let (c, run, item) = waiting("stuck");
        assert_an_unseen_switch_off_does_nothing(&c, &item, order, &case);
        c.ports().panics.set(Some("agents"));
        let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = c.answer(
                &item,
                Choice::Done,
                None,
                shown_step_mark_or_zero(&c, &item),
            );
        }));
        c.ports().panics.set(None);
        assert!(
            out.is_err(),
            "{case}: the rest of the switch-off panicked at its read"
        );
        assert_eq!(
            c.off_beside_live(),
            [agent(0)],
            "{case}: the hold is known beside the run"
        );
        assert_eq!(open_item(&c).0, item, "{case}: the card stays");
        let m = mark(&c);
        c.ports().loss.set(Some(support::Loss::Nothing));
        let _ = c.answer(
            &item,
            Choice::Done,
            None,
            shown_step_mark_or_zero(&c, &item),
        );
        c.ports().loss.set(None);
        assert!(
            calls_since(&c, m)
                .iter()
                .any(|call| matches!(call, Call::History(..))),
            "{case}: the press reads"
        );
        assert!(
            c.off_beside_live().is_empty(),
            "{case}: the read stops the run"
        );
        assert_eq!(cancels_of(&c, m, &run), 1, "{case}");
        assert_eq!(states_sent(&c, m), [AgentState::Paused], "{case}");
        assert!(
            handed_back(&c, m).is_empty(),
            "{case}: no done at the runner"
        );
    }
}

fn raised_with_no_acknowledgement(way: &str) -> (Coordinator<Both>, RunId, ItemId) {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    let event = match way {
        "stuck" => handback(&run, 0, HandbackReason::StepFailed),
        "sign_in" => handback(&run, 0, HandbackReason::SignIn),
        "confirm" => ask_confirm(&run, 0),
        _ => bind(&run, 0),
    };
    c.ports().loss.set(Some(support::Loss::Nothing));
    c.on_run(&run, event).unwrap();
    c.ports().loss.set(None);
    assert_eq!(items_raised(&c), 0, "{way}: the raise waits in the ledger");
    let (item, _) = open_item(&c);
    (c, run, item)
}

fn item_rows_since(c: &Coordinator<Both>, from: usize, item: &ItemId) -> Vec<String> {
    c.ports().ports.events.borrow()[from..]
        .iter()
        .filter_map(|e| match &e.what {
            What::ItemRaised { item: i, .. } if i == item => Some("raised".to_owned()),
            What::ItemAnswered { item: i, .. } if i == item => Some("answered".to_owned()),
            What::ItemReleased { item: i, .. } if i == item => Some("released".to_owned()),
            What::ItemLapsed { item: i, lapsed } if i == item => {
                Some(format!("lapsed:{}", lapsed.cause().name()))
            }
            _ => None,
        })
        .collect()
}

fn runner_answers_tried_since(c: &Coordinator<Both>, from: usize) -> Vec<Call> {
    c.ports().ports.tried.borrow()[from..]
        .iter()
        .filter(|t| matches!(t.call, Call::AnswerHandback(..) | Call::AnswerBind(..)))
        .map(|t| t.call.clone())
        .collect()
}

fn assert_the_stop_came_first(c: &Coordinator<Both>, m: usize, run: &RunId, case: &str) {
    use halvo_coord::types::AgentState;
    assert_eq!(cancels_of(c, m, run), 1, "{case}: one cancel");
    assert_eq!(
        states_sent(c, m),
        [AgentState::Paused],
        "{case}: paused once"
    );
    assert!(
        c.off_beside_live().is_empty(),
        "{case}: no run beside the hold"
    );
    assert!(c.snapshot().items.is_empty(), "{case}: no card");
    assert_eq!(
        row_line(&c.snapshot(), 0),
        text("line.off.person"),
        "{case}"
    );
}

fn press_under_a_cancel_that_panics(
    c: &Coordinator<Both>,
    item: &ItemId,
    choice: Choice,
    case: &str,
) {
    c.ports().panics.set(Some("cancel"));
    let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = c.answer(item, choice, None, shown_step_mark_or_zero(c, item));
    }));
    c.ports().panics.set(None);
    assert!(out.is_err(), "{case}: the cancel panicked");
    assert!(
        c.off_beside_live().is_empty(),
        "{case}: the board's stop came before the cancel"
    );
}

#[test]
fn a_done_whose_flush_of_its_waiting_raise_reads_a_stored_switch_off_hands_nothing_and_lapses_the_item_after_its_raise(
) {
    for way in ["stuck", "sign_in"] {
        for order in NO_SERVICE_ROW_UNSEEN_ORDERS {
            let case = format!("{way} {order:?}");
            let (c, run, item) = raised_with_no_acknowledgement(way);
            assert_an_unseen_switch_off_does_nothing(&c, &item, order, &case);
            let (m, t, from) = (
                mark(&c),
                c.ports().ports.tried.borrow().len(),
                c.ports().ports.events.borrow().len(),
            );
            assert_eq!(
                c.answer(
                    &item,
                    Choice::Done,
                    None,
                    shown_step_mark_or_zero(&c, &item)
                ),
                Err(Cause::UnknownItem),
                "{case}: as for an item no longer open"
            );
            assert_eq!(
                runner_answers_tried_since(&c, t),
                [],
                "{case}: no answer at the runner"
            );
            assert_eq!(
                item_rows_since(&c, from, &item),
                ["raised", "lapsed:cancelled"],
                "{case}: the raise, then its lapse, and no row of the press"
            );
            assert_the_stop_came_first(&c, m, &run, &case);
            let (c, _run, item) = raised_with_no_acknowledgement(way);
            assert_an_unseen_switch_off_does_nothing(&c, &item, order, &case);
            let t = c.ports().ports.tried.borrow().len();
            press_under_a_cancel_that_panics(&c, &item, Choice::Done, &case);
            assert_eq!(
                runner_answers_tried_since(&c, t),
                [],
                "{case}: no answer at the runner"
            );
        }
    }
}

#[test]
fn an_allow_on_a_second_step_whose_flush_of_its_waiting_raise_reads_a_stored_switch_off_sends_decline_alone_and_lapses_it_after_its_raise(
) {
    use halvo_coord::types::BindResult;
    for order in NO_SERVICE_ROW_UNSEEN_ORDERS {
        let case = format!("{order:?}");
        let (c, run, item) = raised_with_no_acknowledgement("second_step");
        assert_an_unseen_switch_off_does_nothing(&c, &item, order, &case);
        let (m, t, from) = (
            mark(&c),
            c.ports().ports.tried.borrow().len(),
            c.ports().ports.events.borrow().len(),
        );
        assert_eq!(
            c.answer(
                &item,
                Choice::Allow,
                None,
                shown_step_mark_or_zero(&c, &item)
            ),
            Err(Cause::UnknownItem),
            "{case}: as for a step no longer open"
        );
        assert_eq!(
            runner_answers_tried_since(&c, t),
            [Call::AnswerBind(run.clone(), BindResult::Decline)],
            "{case}: the lapse's decline alone"
        );
        decline_before_one_cancel(&c, m, &run, &case);
        assert_eq!(
            item_rows_since(&c, from, &item),
            ["raised", "lapsed:cancelled"],
            "{case}: the raise, then its lapse, and no row of the press"
        );
        assert_the_stop_came_first(&c, m, &run, &case);
        let (c, run, item) = raised_with_no_acknowledgement("second_step");
        assert_an_unseen_switch_off_does_nothing(&c, &item, order, &case);
        let t = c.ports().ports.tried.borrow().len();
        press_under_a_cancel_that_panics(&c, &item, Choice::Allow, &case);
        assert_eq!(
            runner_answers_tried_since(&c, t),
            [Call::AnswerBind(run.clone(), BindResult::Decline)],
            "{case}: the lapse's decline alone"
        );
    }
}

fn switch_off_stored_unseen(c: &Coordinator<Both>, order: support::Loss, case: &str) {
    let m = mark(c);
    lose_the_next_holds_reply(c, order);
    assert_eq!(
        c.control(&agent(0), Action::SwitchOff),
        Err(Cause::NoService),
        "{case}"
    );
    c.ports().loss.set(None);
    let lost: Vec<halvo_coord::types::Event> = c.ports().lost.borrow_mut().drain(..).collect();
    c.ports().ports.events.borrow_mut().extend(lost);
    assert!(
        !calls_since(c, m)
            .iter()
            .any(|call| matches!(call, Call::Cancel(_) | Call::Send(Body::SetState { .. }))),
        "{case}: no cancel and no set_state"
    );
    assert_eq!(hold_sets(c, 0), 1, "{case}: the owner holds the hold");
}

#[test]
fn a_done_again_after_a_stored_done_that_the_run_did_not_take_reads_first_and_hands_nothing_beside_a_stored_switch_off(
) {
    for way in ["stuck", "sign_in"] {
        for order in NO_SERVICE_ROW_UNSEEN_ORDERS {
            let case = format!("{way} {order:?}");
            let reason = if way == "stuck" {
                HandbackReason::StepFailed
            } else {
                HandbackReason::SignIn
            };
            let c = coordinator(&[MANUAL], Limits::PRODUCT);
            let run = start(&c, 0);
            c.on_run(&run, handback(&run, 0, reason)).unwrap();
            let (item, _) = open_item(&c);
            c.ports()
                .ports
                .refuse(Method::AnswerHandback, PortError::NoService);
            assert_eq!(
                c.answer(
                    &item,
                    Choice::Done,
                    None,
                    shown_step_mark_or_zero(&c, &item)
                ),
                Err(Cause::NoService),
                "{case}"
            );
            c.ports().ports.clear_refusals();
            assert_eq!(closing_rows(&c, &item), 1, "{case}: the done is stored");
            switch_off_stored_unseen(&c, order, &case);
            let (m, t, from) = (
                mark(&c),
                c.ports().ports.tried.borrow().len(),
                c.ports().ports.events.borrow().len(),
            );
            assert_eq!(
                c.answer(
                    &item,
                    Choice::Done,
                    None,
                    shown_step_mark_or_zero(&c, &item)
                ),
                Err(Cause::UnknownItem),
                "{case}: as for an item no longer open"
            );
            assert_eq!(
                runner_answers_tried_since(&c, t),
                [],
                "{case}: no answer at the runner"
            );
            assert!(
                !item_rows_since(&c, from, &item).contains(&"answered".to_owned()),
                "{case}: no second row"
            );
            assert_eq!(closing_rows(&c, &item), 1, "{case}: the stored done alone");
            assert_the_stop_came_first(&c, m, &run, &case);
        }
    }
}

#[test]
fn an_allow_again_after_a_stored_allow_that_the_run_did_not_take_reads_first_and_sends_decline_alone_beside_a_stored_switch_off(
) {
    use halvo_coord::types::BindResult;
    for order in NO_SERVICE_ROW_UNSEEN_ORDERS {
        let case = format!("{order:?}");
        let (c, run, item) = second_step();
        first_press(&c, &item, Choice::Allow, First::Taken);
        assert_eq!(
            step_rows(&c, &item),
            ["allow"],
            "{case}: the allow is stored"
        );
        switch_off_stored_unseen(&c, order, &case);
        let (m, t) = (mark(&c), c.ports().ports.tried.borrow().len());
        assert_eq!(
            c.answer(
                &item,
                Choice::Allow,
                None,
                shown_step_mark_or_zero(&c, &item)
            ),
            Err(Cause::UnknownItem),
            "{case}: as for a step no longer open"
        );
        assert_eq!(
            runner_answers_tried_since(&c, t),
            [Call::AnswerBind(run.clone(), BindResult::Decline)],
            "{case}: the lapse's decline alone, never the stored allow"
        );
        decline_before_one_cancel(&c, m, &run, &case);
        assert_eq!(
            step_rows(&c, &item),
            ["allow", "lapsed:cancelled"],
            "{case}: no second allow row, and the step lapses"
        );
        assert_the_stop_came_first(&c, m, &run, &case);
    }
}

#[test]
fn rd101_5b_an_allow_on_a_confirmation_whose_waiting_raise_meets_a_stored_switch_off_stops_the_run_first_and_lapses_it_after_its_raise(
) {
    for order in NO_SERVICE_ROW_UNSEEN_ORDERS {
        let case = format!("{order:?}");
        let (c, run, item) = raised_with_no_acknowledgement("confirm");
        assert_an_unseen_switch_off_does_nothing(&c, &item, order, &case);
        assert_eq!(c.shown_on_page(&item, 0), Ok(()), "{case}");
        c.ports().ports.set_clock(T0 + Limits::PRODUCT.arm_ms, DAY);
        let (m, from) = (mark(&c), c.ports().ports.events.borrow().len());
        assert_eq!(
            c.answer(
                &item,
                Choice::Allow,
                None,
                shown_step_mark_or_zero(&c, &item)
            ),
            Err(Cause::UnknownItem),
            "{case}: as for an item no longer open"
        );
        no_answer_of_the_press_before_the_cancel(&c, m, &run, &case);
        assert_eq!(
            item_rows_since(&c, from, &item),
            ["raised", "lapsed:cancelled"],
            "{case}: the waiting raise, then its lapse, and no row of the press"
        );
        assert_the_stop_came_first(&c, m, &run, &case);
    }
}
#[test]
fn a_ticks_lapse_of_a_confirmation_cancels_right_after_the_log_of_a_waiting_row_with_one_clock_read_between_that_stamps_the_row_and_the_stop(
) {
    let limits = Limits::PRODUCT;
    let (c, run, item) = raised_with_no_acknowledgement("confirm");
    let waiting = c.ports().ports.events.borrow().len();
    let past = T0 + limits.wait_limit_s * 1000 + 10_000;
    c.ports().ports.set_clock(past, DAY);
    c.ports().loss.set(Some(support::Loss::Nothing));
    c.ports().next_history_fails.set(true);
    c.ports().ports.step_reads(1);
    let t = c.ports().ports.tried.borrow().len();
    c.tick();
    c.ports().ports.step_reads(0);
    c.ports().loss.set(None);
    c.ports().next_history_fails.set(false);
    let tried: Vec<halvo_coord::mem::Tried> = c.ports().ports.tried.borrow()[t..].to_vec();
    let cancel = tried
        .iter()
        .position(|x| x.call == Call::Cancel(run.clone()))
        .expect("the tick cancels the run");
    let before = &tried[cancel - 1];
    assert!(
        matches!(&before.call, Call::Log(rows) if rows.iter().all(|e| !matches!(e.what, What::ItemLapsed { .. })))
            && before.refused.is_none(),
        "the call right before the cancel is the acknowledged log of the waiting row: {:?}",
        before.call
    );
    let reading = before.at;
    assert_eq!(
        tried[cancel].at - reading,
        1,
        "one read of the clock lies between that log and the cancel"
    );
    let lapsed_at = c.ports().ports.events.borrow()[waiting..]
        .iter()
        .find_map(|e| match &e.what {
            What::ItemLapsed { item: i, .. } if *i == item => Some(e.at),
            _ => None,
        })
        .expect("the item lapses");
    assert_eq!(
        Some(lapsed_at),
        i64::try_from(reading).ok().map(|ms| ms * 1_000_000),
        "the row's instant, in nanoseconds, is that reading"
    );
    let m = mark(&c);
    c.ports()
        .ports
        .set_clock(reading + limits.cancel_grace_s * 1000 - 1, DAY);
    c.tick();
    assert_eq!(
        kills_of(&c, m, &run),
        0,
        "no kill before the grace of that reading"
    );
    c.ports()
        .ports
        .set_clock(reading + limits.cancel_grace_s * 1000, DAY);
    c.tick();
    assert_eq!(
        kills_of(&c, m, &run),
        1,
        "the board's stop is at that reading"
    );
}

/// A run of agent 0 that waits on a stuck item whose `choice` the owner stored and acknowledged
/// and whose `Runs::answer_handback` failed `busy`, so that the run did not take it.
fn stuck_with_a_stored_answer_not_taken(choice: Choice) -> (Coordinator<Both>, RunId, ItemId) {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    c.on_run(&run, handback(&run, 0, HandbackReason::StepFailed))
        .unwrap();
    let (item, _) = open_item(&c);
    c.ports()
        .ports
        .refuse(Method::AnswerHandback, PortError::Busy);
    assert_eq!(
        c.answer(&item, choice, None, shown_step_mark_or_zero(&c, &item)),
        Err(Cause::Busy)
    );
    c.ports().ports.clear_refusals();
    assert_eq!(closing_rows(&c, &item), 1, "the answer is stored");
    (c, run, item)
}

#[test]
fn rd108_2_a_done_again_whose_read_before_its_kept_row_fails_answers_no_service_and_hands_nothing()
{
    for with_hold in [false, true] {
        let case = format!("hold stored unseen={with_hold}");
        let (c, _run, item) = stuck_with_a_stored_answer_not_taken(Choice::Done);
        if with_hold {
            switch_off_stored_unseen(&c, support::Loss::Later, &case);
        }
        let (m, t) = (mark(&c), c.ports().ports.tried.borrow().len());
        c.ports().next_history_fails.set(true);
        let out = c.answer(
            &item,
            Choice::Done,
            None,
            shown_step_mark_or_zero(&c, &item),
        );
        let read_made = !c.ports().next_history_fails.take();
        assert!(
            read_made,
            "{case}: the press reads before the kept row's result"
        );
        assert_eq!(out, Err(Cause::NoService), "{case}: the read's cause");
        assert_eq!(
            runner_answers_tried_since(&c, t),
            [],
            "{case}: no answer at the runner"
        );
        assert_eq!(
            cancels_of(&c, m, &_run),
            0,
            "{case}: nothing stops in this move"
        );
        assert_eq!(open_item(&c).0, item, "{case}: the card stays");
        assert_eq!(closing_rows(&c, &item), 1, "{case}: no second row");
    }
}

#[test]
fn rd108_2_an_allow_again_whose_read_before_its_recorded_allow_fails_answers_no_service_and_sends_no_allow(
) {
    use halvo_coord::types::BindResult;
    for with_hold in [false, true] {
        let case = format!("hold stored unseen={with_hold}");
        let (c, run, item) = second_step();
        first_press(&c, &item, Choice::Allow, First::Taken);
        assert_eq!(
            step_rows(&c, &item),
            ["allow"],
            "{case}: the allow is stored"
        );
        if with_hold {
            switch_off_stored_unseen(&c, support::Loss::Later, &case);
        }
        let (m, t) = (mark(&c), c.ports().ports.tried.borrow().len());
        c.ports().next_history_fails.set(true);
        let out = c.answer(
            &item,
            Choice::Allow,
            None,
            shown_step_mark_or_zero(&c, &item),
        );
        let read_made = !c.ports().next_history_fails.take();
        assert!(
            read_made,
            "{case}: the press reads before its recorded allow"
        );
        assert_eq!(out, Err(Cause::NoService), "{case}: the read's cause");
        assert!(
            !runner_answers_tried_since(&c, t)
                .contains(&Call::AnswerBind(run.clone(), BindResult::Allow)),
            "{case}: no allow at the runner"
        );
        if with_hold {
            decline_before_one_cancel(&c, m, &run, &case);
            assert_eq!(
                step_rows(&c, &item),
                ["allow", "lapsed:cancelled"],
                "{case}: the read back after the press shows the hold and the step lapses"
            );
        } else {
            assert_eq!(
                runner_answers_tried_since(&c, t),
                [],
                "{case}: nothing at the runner"
            );
            assert_eq!(open_item(&c).0, item, "{case}: the card stays");
        }
    }
}

#[test]
fn rd108_2_a_done_after_a_stored_decline_that_the_run_did_not_take_reads_first_and_hands_nothing_beside_a_stored_switch_off(
) {
    for order in NO_SERVICE_ROW_UNSEEN_ORDERS {
        let case = format!("{order:?}");
        let (c, run, item) = stuck_with_a_stored_answer_not_taken(Choice::Decline);
        switch_off_stored_unseen(&c, order, &case);
        let (m, t, from) = (
            mark(&c),
            c.ports().ports.tried.borrow().len(),
            c.ports().ports.events.borrow().len(),
        );
        assert_eq!(
            c.answer(
                &item,
                Choice::Done,
                None,
                shown_step_mark_or_zero(&c, &item)
            ),
            Err(Cause::UnknownItem),
            "{case}: as for an item no longer open"
        );
        assert_eq!(
            runner_answers_tried_since(&c, t),
            [],
            "{case}: the stored decline is not handed past the switch-off"
        );
        assert_eq!(
            item_rows_since(&c, from, &item),
            Vec::<String>::new(),
            "{case}: no row of the press; the stored decline closed the item"
        );
        assert_the_stop_came_first(&c, m, &run, &case);
    }
}

#[test]
fn no_writer_stores_an_item_answered_of_a_setup_with_the_answer_none() {
    use halvo_coord::types::{Answered, SetupAnswer};
    let (c, _made, item) = setup_agent();
    let press = |choice: Choice, pick: Option<(Param, u32)>| {
        c.answer(&item, choice, pick, shown_step_mark_or_zero(&c, &item))
    };
    let _ = press(Choice::None, None);
    press(Choice::Choose, Some((Param::Start, 1))).unwrap();
    let _ = press(Choice::None, None);
    press(Choice::Choose, Some((Param::Payee, 1))).unwrap();
    let _ = press(Choice::None, None);
    assert_eq!(
        c.setup_typed(
            &item,
            Param::Instrument,
            "Card 4242",
            shown_step_mark_or_zero(&c, &item)
        ),
        Ok(())
    );
    let _ = press(Choice::None, None);
    let answers: Vec<SetupAnswer> = events(&c)
        .iter()
        .filter_map(|w| match w {
            What::ItemAnswered {
                item: i,
                answer: Answered::Setup(a),
            } if *i == item => Some(*a),
            _ => None,
        })
        .collect();
    assert_eq!(
        answers,
        [SetupAnswer::Choose],
        "the setup's one closing row holds choose, never none"
    );
}

#[test]
fn rd121_the_next_lifes_first_creation_move_raises_no_setup_for_an_agent_set_aside() {
    let (c, made, _old) = setup_agent();
    let life = next_life(&c, |o| {
        o.ports.refuse_agent(
            Method::History,
            &made,
            PortError::Refused(halvo_coord::types::Name::parse("store").unwrap()),
        )
    });
    let tried_for_it = life
        .ports()
        .ports
        .tried
        .borrow()
        .iter()
        .filter(|t| matches!(&t.call, Call::Log(rows) if rows.iter().any(|e| e.agent == made)))
        .count();
    assert_eq!(
        tried_for_it, 0,
        "nothing is logged for an agent whose history was not read"
    );
    assert_eq!(setups_raised(&life), 1, "the earlier life's raise alone");
    assert!(
        !events(&life)
            .iter()
            .any(|w| matches!(w, What::ItemLapsed { .. })),
        "and no lapse of its earlier item before its history is read"
    );
}

fn is_item_raise(e: &halvo_coord::types::Event) -> bool {
    matches!(e.what, What::ItemRaised { .. })
}

#[test]
fn rd122_a_held_place_of_the_items_list_stops_the_ticks_start_at_the_coordinator() {
    let due = support::A {
        person: false,
        slotted: true,
    };
    let c = Coordinator::new(owner(&[due], Limits::PRODUCT), Limits::PRODUCT);
    c.tick();
    assert_eq!(started(&c).len(), 1, "the first slot starts");
    let run = started(&c)[0].clone();
    assert_eq!(c.on_run(&run, RunEvent::Hello), Ok(()));
    c.ports().ports.refuse_record(
        is_item_raise,
        PortError::Refused(halvo_coord::types::Name::parse("store").unwrap()),
    );
    let _ = c.on_run(&run, handback(&run, 0, HandbackReason::StepFailed));
    let _ = c.on_run(&run, RunEvent::Ended(Some(Outcome::Completed)));
    c.ports().ports.store_end(
        &agent(0),
        PastRun {
            run,
            day: DAY,
            outcome: Outcome::Completed,
            unknown: 0,
        },
    );
    for k in 1..=3u64 {
        c.ports()
            .ports
            .set_clock(T0 + k * 86_400_000, DAY + k as u32);
        c.tick();
    }
    assert_eq!(
        started(&c).len(),
        1,
        "a place that the items' list holds under class D stops the tick's start (RD-64 2.5)"
    );
    c.ports().ports.clear_refusals();
    c.ports().ports.set_clock(T0 + 4 * 86_400_000, DAY + 4);
    c.tick();
    assert_eq!(
        started(&c).len(),
        2,
        "once the store takes the row the place is free and the tick starts the agent"
    );
}

fn refused_name(name: &str) -> PortError {
    PortError::Refused(halvo_coord::types::Name::parse(name).unwrap())
}

#[test]
fn rd122_a_refused_items_list_makes_the_record_stuck_and_an_acknowledged_flush_ends_it() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    c.ports()
        .ports
        .refuse_record(is_item_raise, refused_name("store"));
    let _ = c.on_run(&run, handback(&run, 0, HandbackReason::StepFailed));
    for k in 1..=3u64 {
        c.ports().ports.set_clock(T0 + k * 1_000, DAY);
        c.tick();
    }
    assert!(
        c.record_stuck(&agent(0)),
        "three ticks in a row at which the items' list was refused make the record stuck"
    );
    c.ports().ports.clear_refusals();
    c.ports().ports.set_clock(T0 + 4_000, DAY);
    c.tick();
    assert!(
        !c.record_stuck(&agent(0)),
        "a tick whose flush the owner acknowledged ends the count"
    );
}

#[test]
fn u24r_a_stuck_record_shows_the_record_line_on_its_row_with_stop_and_delete_alone() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    c.ports()
        .ports
        .refuse_record(is_item_raise, refused_name("store"));
    let _ = c.on_run(&run, handback(&run, 0, HandbackReason::StepFailed));
    for k in 1..=2u64 {
        c.ports().ports.set_clock(T0 + k * 1_000, DAY);
        c.tick();
        assert!(!c.record_stuck(&agent(0)), "tick {k}");
        assert_ne!(
            row_line(&c.snapshot(), 0),
            text("line.problem.record"),
            "tick {k}"
        );
    }
    c.ports().ports.set_clock(T0 + 3_000, DAY);
    c.tick();
    assert!(c.record_stuck(&agent(0)));
    let s = c.snapshot();
    let row = s.rows.iter().find(|r| r.agent == agent(0)).unwrap();
    assert_eq!(row.line, text("line.problem.record"));
    assert_eq!(row.status, Status::Problem);
    let controls: Vec<halvo_coord::types::Control> =
        row.controls.iter().map(|c| c.control).collect();
    assert_eq!(
        controls,
        [
            halvo_coord::types::Control::Stop,
            halvo_coord::types::Control::Delete
        ]
    );
    assert!(
        s.items.iter().all(|i| i.agent() != &agent(0)),
        "no item of the agent invites a press whose answer cannot be saved"
    );
    c.ports().ports.clear_refusals();
    c.ports().ports.set_clock(T0 + 4_000, DAY);
    c.tick();
    assert!(!c.record_stuck(&agent(0)));
    assert_ne!(row_line(&c.snapshot(), 0), text("line.problem.record"));
}

fn is_run_start(e: &halvo_coord::types::Event) -> bool {
    matches!(e.what, What::RunStarted { .. })
}

#[test]
fn u24r_g_after_an_acknowledged_stop_the_record_stays_stuck_until_the_next_tick_ends() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    c.ports()
        .ports
        .refuse_record(is_item_raise, refused_name("store"));
    let _ = c.on_run(&run, handback(&run, 0, HandbackReason::StepFailed));
    for k in 1..=3u64 {
        c.ports().ports.set_clock(T0 + k * 1_000, DAY);
        c.tick();
    }
    assert!(c.record_stuck(&agent(0)));
    c.ports().ports.clear_refusals();
    assert_eq!(c.control(&agent(0), Action::Stop), Ok(()));
    assert!(
        events(&c)
            .iter()
            .any(|w| matches!(w, What::ItemLapsed { .. })),
        "the press's own store was acknowledged"
    );
    assert!(
        c.record_stuck(&agent(0)),
        "an acknowledged press does not end the count before the tick ends"
    );
    assert_eq!(row_line(&c.snapshot(), 0), text("line.problem.record"));
    c.ports().ports.set_clock(T0 + 4_000, DAY);
    c.tick();
    assert!(!c.record_stuck(&agent(0)), "the next tick ends the count");
    assert_ne!(row_line(&c.snapshot(), 0), text("line.problem.record"));
}

#[test]
fn u24r_g_a_persons_run_now_is_not_held_by_the_stuck_count() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    c.ports()
        .ports
        .refuse_record(is_run_start, refused_name("store"));
    for k in 1..=3u64 {
        assert!(
            c.control(&agent(0), Action::RunNow).is_err(),
            "press {k}: the refused start starts nothing"
        );
        c.ports().ports.set_clock(T0 + k * 1_000, DAY);
        c.tick();
    }
    assert!(started(&c).is_empty());
    assert!(c.record_stuck(&agent(0)));
    assert_eq!(row_line(&c.snapshot(), 0), text("line.problem.record"));
    c.ports().ports.clear_refusals();
    assert_eq!(
        c.control(&agent(0), Action::RunNow),
        Ok(()),
        "the stuck count does not hold a person's Run now"
    );
    assert_eq!(started(&c).len(), 1);
    assert!(
        c.record_stuck(&agent(0)),
        "the start's own acknowledged store does not end the count before the tick ends"
    );
    c.ports().ports.set_clock(T0 + 4_000, DAY);
    c.tick();
    assert!(!c.record_stuck(&agent(0)));
}

fn is_run_end(e: &halvo_coord::types::Event) -> bool {
    matches!(e.what, What::RunEnded { .. })
}

#[test]
fn u24r_each_record_that_the_board_counts_as_dropped_is_of_a_move_already_made() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    c.ports()
        .ports
        .refuse_record(is_item_raise, refused_name("too_large"));
    let _ = c.on_run(&run, handback(&run, 0, HandbackReason::StepFailed));
    assert!(
        c.snapshot().items.iter().any(|i| i.agent() == &agent(0)),
        "the coordinator raised the item and the page shows it, though its row was refused"
    );
    assert_eq!(c.records_dropped(&agent(0)), None);
    c.ports().ports.set_clock(T0 + 3 * 86_400_000, DAY + 3);
    c.tick();
    assert_eq!(
        c.records_dropped(&agent(0)).map(|(n, event, _)| (n, event)),
        Some((1, "item_raised".to_string()))
    );
    assert!(
        !events(&c)
            .iter()
            .any(|w| matches!(w, What::ItemRaised { .. })),
        "the owner never stored the raise that happened"
    );

    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    c.ports()
        .ports
        .refuse_record(is_run_end, refused_name("too_large"));
    let _ = c.on_run(&run, RunEvent::Ended(Some(Outcome::Completed)));
    c.ports().ports.set_clock(T0 + 1_000, DAY);
    c.tick();
    assert_eq!(c.live(), 0, "the run ended, though its row was refused");
    assert_eq!(c.records_dropped(&agent(0)), None);
    c.ports().ports.set_clock(T0 + 95 * 86_400_000, DAY + 95);
    c.tick();
    assert_eq!(
        c.records_dropped(&agent(0)).map(|(n, event, _)| (n, event)),
        Some((1, "run_ended".to_string()))
    );
    assert!(
        !events(&c)
            .iter()
            .any(|w| matches!(w, What::RunEnded { run: r, .. } if *r == run)),
        "the owner never stored the end that happened"
    );
}

fn dropped_lines(c: &Coordinator<Both>, n: usize) -> Vec<String> {
    c.card(&agent(n))
        .unwrap()
        .lines
        .iter()
        .filter(|l| l.key == halvo_coord::types::LineKey::RecordsDropped)
        .map(|l| l.text.clone())
        .collect()
}

#[test]
fn u24r_the_card_shows_the_boards_dropped_count_first_and_nothing_without_one() {
    let c = coordinator(&[MANUAL, MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    assert!(dropped_lines(&c, 0).is_empty(), "nothing taken off yet");
    c.ports()
        .ports
        .refuse_record(is_item_raise, refused_name("too_large"));
    let _ = c.on_run(&run, handback(&run, 0, HandbackReason::StepFailed));
    c.ports().ports.set_clock(T0 + 3 * 86_400_000, DAY + 3);
    c.tick();
    let (count, _, _) = c.records_dropped(&agent(0)).unwrap();
    assert_eq!(count, 1);
    let card = c.card(&agent(0)).unwrap();
    assert_eq!(
        card.lines[0].key,
        halvo_coord::types::LineKey::RecordsDropped
    );
    assert_eq!(
        card.lines[0].text,
        words::text("card.records_dropped", &[("count", &count.to_string())]).unwrap()
    );
    assert_eq!(dropped_lines(&c, 0).len(), 1, "one line");
    assert_eq!(c.records_dropped(&agent(1)), None);
    assert!(dropped_lines(&c, 1).is_empty(), "an agent with none");
}

/// The words key of `working.kill_stuck` on the system that runs the test.
const HOST_KILL_STUCK: &str = if cfg!(windows) {
    "line.working.kill_stuck_windows"
} else {
    "line.working.kill_stuck_mac"
};

fn row_of(c: &Coordinator<Both>, n: usize) -> halvo_coord::types::Row {
    c.snapshot()
        .rows
        .into_iter()
        .find(|r| r.agent == agent(n))
        .unwrap()
}

fn row_controls(row: &halvo_coord::types::Row) -> Vec<halvo_coord::types::Control> {
    row.controls.iter().map(|c| c.control).collect()
}

#[test]
fn u24r_b1_the_card_shows_the_boards_count_of_two_dropped_records() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    c.ports()
        .ports
        .refuse_record(is_item_raise, refused_name("too_large"));
    let _ = c.on_run(&run, handback(&run, 0, HandbackReason::StepFailed));
    c.ports().ports.set_clock(T0 + 3 * 86_400_000, DAY + 3);
    c.tick();
    assert_eq!(c.records_dropped(&agent(0)).map(|(n, _, _)| n), Some(1));
    c.ports().ports.clear_refusals();
    c.ports()
        .ports
        .refuse_record(is_run_end, refused_name("too_large"));
    let _ = c.on_run(&run, RunEvent::Ended(Some(Outcome::Completed)));
    c.ports()
        .ports
        .set_clock(T0 + 3 * 86_400_000 + 1_000, DAY + 3);
    c.tick();
    c.ports().ports.set_clock(T0 + 99 * 86_400_000, DAY + 99);
    c.tick();
    assert_eq!(
        c.records_dropped(&agent(0)).map(|(n, _, _)| n),
        Some(2),
        "a raise and an end taken off"
    );
    let card = c.card(&agent(0)).unwrap();
    assert_eq!(
        card.lines[0].key,
        halvo_coord::types::LineKey::RecordsDropped
    );
    assert_eq!(
        card.lines[0].text,
        words::text("card.records_dropped", &[("count", "2")]).unwrap()
    );
}

#[test]
fn u24r_b2_one_agents_stuck_record_leaves_the_other_agents_row_as_it_was() {
    let c = coordinator(&[MANUAL, MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    let before = row_of(&c, 1);
    c.ports()
        .ports
        .refuse_record(is_item_raise, refused_name("store"));
    let _ = c.on_run(&run, handback(&run, 0, HandbackReason::StepFailed));
    for k in 1..=3u64 {
        c.ports().ports.set_clock(T0 + k * 1_000, DAY);
        c.tick();
    }
    assert!(c.record_stuck(&agent(0)));
    assert!(!c.record_stuck(&agent(1)));
    assert_eq!(row_of(&c, 0).line, text("line.problem.record"));
    let after = row_of(&c, 1);
    assert_ne!(
        after.line,
        text("line.problem.record"),
        "a healthy agent's row does not take another agent's stuck line"
    );
    assert_eq!(after.line, before.line);
    assert_eq!(row_controls(&after), row_controls(&before));
}

#[test]
fn u24r_b3_an_agent_set_aside_at_recovery_shows_the_record_line_with_delete_alone() {
    let o = owner(&[MANUAL, MANUAL], Limits::PRODUCT);
    o.ports
        .refuse_agent(Method::History, &agent(0), refused_name("store"));
    let c = Coordinator::new(o, Limits::PRODUCT);
    c.tick();
    assert!(c.record_stuck(&agent(0)), "recovery set agent 0 aside");
    let row = row_of(&c, 0);
    assert_eq!(row.line, text("line.problem.record"));
    assert_eq!(
        row_controls(&row),
        [halvo_coord::types::Control::Delete],
        "the delete alone"
    );
    assert!(!c.record_stuck(&agent(1)));
    assert_ne!(row_of(&c, 1).line, text("line.problem.record"));
}

#[test]
fn u24r_b4_a_gone_workflows_live_run_keeps_its_gone_row_while_its_record_is_stuck() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    c.ports()
        .ports
        .refuse_record(is_item_raise, refused_name("store"));
    let _ = c.on_run(&run, handback(&run, 0, HandbackReason::StepFailed));
    for k in 1..=3u64 {
        c.ports().ports.set_clock(T0 + k * 1_000, DAY);
        c.tick();
    }
    assert!(c.record_stuck(&agent(0)));
    c.ports()
        .ports
        .workflows
        .borrow_mut()
        .retain(|w| w.id() != &support::flow(0));
    c.ports().ports.set_clock(T0 + 4_000, DAY);
    c.tick();
    assert_eq!(c.live(), 1, "the run lives on after its workflow went");
    assert!(c.record_stuck(&agent(0)), "the record is still stuck");
    let row = row_of(&c, 0);
    assert_eq!(row.line, text("line.working.stopping"));
    assert!(row.controls.is_empty(), "{:?}", row.controls);
    c.ports().ports.refuse(Method::Kill, PortError::Busy);
    c.ports()
        .ports
        .set_clock(T0 + 4_000 + Limits::PRODUCT.cancel_grace_s * 1000, DAY);
    c.tick();
    assert_eq!(c.live(), 1, "the kill did not end the run");
    assert!(c.record_stuck(&agent(0)));
    let row = row_of(&c, 0);
    assert_eq!(row.line, text(HOST_KILL_STUCK));
    assert!(row.controls.is_empty(), "{:?}", row.controls);
}

#[test]
fn rd122_the_board_counts_a_record_that_the_items_list_took_off_by_its_event_and_refusal() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    c.ports()
        .ports
        .refuse_record(is_item_raise, refused_name("too_large"));
    let _ = c.on_run(&run, handback(&run, 0, HandbackReason::StepFailed));
    let past = T0 + 3 * 86_400_000;
    c.ports().ports.set_clock(past, DAY + 3);
    c.tick();
    assert_eq!(
        c.records_dropped(&agent(0)),
        Some((1, "item_raised".to_string(), "too_large".to_string())),
        "a record more than two days old is taken off and counted by its event and its refusal"
    );
}

#[test]
fn rd122_no_sender_meets_seq_after_another_sender_of_the_agent_stored() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    c.on_run(&run, handback(&run, 0, HandbackReason::StepFailed))
        .unwrap();
    let (item, _) = open_item(&c);
    let _ = c.answer(
        &item,
        Choice::Done,
        None,
        shown_step_mark_or_zero(&c, &item),
    );
    let _ = c.on_run(&run, RunEvent::Ended(Some(Outcome::Completed)));
    c.ports().ports.set_clock(T0 + 1_000, DAY);
    c.tick();
    let seq_refusals = c
        .ports()
        .ports
        .tried
        .borrow()
        .iter()
        .filter(|t| {
            matches!(t.call, Call::Log(_))
                && matches!(&t.refused, Some(PortError::Refused(n)) if n.as_str() == "seq")
        })
        .count();
    assert_eq!(
        seq_refusals, 0,
        "the ledger, the press and the schedule's log share one stored last, so no first try meets seq (RD-64 3.2)"
    );
}

#[test]
fn rd124_t2d_where_the_owners_last_fell_between_a_raise_and_a_press_the_ledger_follows_the_read_and_the_press_answers_ok(
) {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    c.on_run(&run, handback(&run, 0, HandbackReason::StepFailed))
        .unwrap();
    let (item, _) = open_item(&c);
    c.ports().ports.events.borrow_mut().clear();
    let a = c.answer(
        &item,
        Choice::Done,
        None,
        shown_step_mark_or_zero(&c, &item),
    );
    let b = c.on_run(&run, RunEvent::Ended(Some(Outcome::Completed)));
    c.ports().ports.set_clock(T0 + 1_000, DAY);
    c.tick();
    c.ports().ports.set_clock(T0 + 2_000, DAY);
    c.tick();
    let tried = c.ports().ports.tried.borrow();
    let seq = tried
        .iter()
        .filter(|t| {
            matches!(t.call, Call::Log(_))
                && matches!(&t.refused, Some(PortError::Refused(n)) if n.as_str() == "seq")
        })
        .count();
    let seqs: Vec<u32> = c
        .ports()
        .ports
        .events
        .borrow()
        .iter()
        .map(|e| e.seq.get())
        .collect();
    assert!(a.is_ok(), "the press answers Ok: {a:?}");
    assert!(b.is_ok(), "{b:?}");
    assert_eq!(
        seq, 1,
        "the press's first try meets seq once, then the ledger follows the coordinator's read (RD-64 3.2)"
    );
    assert_eq!(seqs, vec![1, 2], "the press and the end are stored");
}

fn panicking<R>(c: &Coordinator<Both>, port: &'static str, call: impl FnOnce() -> R) {
    c.ports().panics.set(Some(port));
    let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(call));
    c.ports().panics.set(None);
    assert!(out.is_err(), "the port {port} panicked");
}

fn tick_that_panics(c: &Coordinator<Both>) {
    panicking(c, "agents", || c.tick());
}

fn armed_confirmation() -> (Coordinator<Both>, RunId, ItemId) {
    let (c, run, item) = run_waiting_on_a_confirmation(false);
    assert_eq!(c.shown_on_page(&item, 0), Ok(()));
    c.ports().ports.set_clock(T0 + Limits::PRODUCT.arm_ms, DAY);
    c.tick();
    assert_eq!(
        c.snapshot().items[0].armed(),
        halvo_coord::types::YesNo::Yes,
        "armed at the tick"
    );
    (c, run, item)
}

fn ended_in_store(c: &Coordinator<Both>, run: &RunId, outcome: Outcome) {
    c.ports().ports.store_end(
        &agent(0),
        PastRun {
            run: run.clone(),
            day: DAY,
            outcome,
            unknown: 0,
        },
    );
    c.tick();
}

fn same_state(a: &Snapshot, b: &Snapshot) -> bool {
    a.rows == b.rows && a.items == b.items && a.note == b.note
}

#[test]
fn rd80_a_move_that_did_not_return_raises_seq_at_the_next_publish_though_nothing_differs() {
    let (c, _run, item) = armed_confirmation();
    let before = c.snapshot();
    panicking(&c, "history", || c.answer(&item, Choice::Allow, None, 0));
    c.tick();
    let after = c.snapshot();
    assert!(
        same_state(&before, &after),
        "rows, items and note are equal"
    );
    assert_eq!(
        after.seq,
        before.seq + 1,
        "the next tick's publish raised seq"
    );
    c.tick();
    assert_eq!(c.answer(&item, Choice::Allow, None, 7), Err(Cause::State));
    let later = c.snapshot();
    assert_eq!(later.seq, after.seq, "a move that returned raises nothing");
    assert!(same_state(&after, &later));
}

#[test]
fn rd80_after_a_tick_that_panics_the_bell_rings_and_the_first_call_takes_the_note() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let before = c.snapshot();
    let m = mark(&c);
    tick_that_panics(&c);
    assert!(
        calls_since(&c, m).contains(&Call::Changed),
        "the bell rang as the tick unwound"
    );
    let Next::Changed(s) = c.next(before.seq) else {
        panic!("the waiting call takes a state");
    };
    assert_eq!(s.note, Some(text("note.out_of_date")));
    assert_eq!(s.rows, before.rows);
    assert_eq!(s.seq, before.seq + 1);
}

#[test]
fn rd80_after_a_tick_that_panics_every_publish_carries_the_note_until_a_tick_returns() {
    let note = Some(text("note.out_of_date"));
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    tick_that_panics(&c);
    c.on_run(&run, ask_confirm(&run, 0)).unwrap();
    let s = c.snapshot();
    assert_eq!(s.items.len(), 1);
    assert_eq!(s.note, note, "a runner's event");
    let item = s.items[0].item().clone();
    assert_eq!(c.shown_on_page(&item, 0), Ok(()));
    c.ports().ports.set_clock(T0 + Limits::PRODUCT.arm_ms, DAY);
    assert_eq!(c.answer(&item, Choice::Reject, None, 0), Ok(()));
    let s = c.snapshot();
    assert!(s.items.is_empty());
    assert_eq!(s.note, note, "an answer");
    assert_eq!(c.control(&agent(0), Action::Stop), Ok(()));
    let s = c.snapshot();
    assert_eq!(row_line(&s, 0), text("line.working.stopping"));
    assert_eq!(s.note, note, "a stop");
    c.tick();
    assert_eq!(c.snapshot().note, None, "the first tick that returns");
}

#[test]
fn rd80_with_a_full_roster_the_note_of_the_mark_stands_and_roster_full_comes_back() {
    let limits = Limits {
        roster_max: 1,
        ..Limits::PRODUCT
    };
    let c = coordinator(&[MANUAL], limits);
    c.ports()
        .ports
        .workflows
        .borrow_mut()
        .push(Workflow::Full(halvo_coord::types::FullWorkflow {
            id: WorkflowId::parse(&format!("{:032x}", 0xf9)).unwrap(),
            state: WorkflowState::Approved,
            category: Category::Email,
            site: None,
            trigger: WorkflowTrigger::Manual,
            period: None,
            last: DAY,
            agent: None,
        }));
    c.tick();
    assert_eq!(c.snapshot().note, Some(text("note.roster_full")));
    tick_that_panics(&c);
    assert_eq!(c.snapshot().note, Some(text("note.out_of_date")));
    c.tick();
    assert_eq!(c.snapshot().note, Some(text("note.roster_full")));
}

#[test]
fn rd80_while_the_mark_stands_each_kind_of_start_is_no_service_and_a_stop_is_taken() {
    let c = coordinator(&[MANUAL, MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    assert_eq!(c.control(&agent(1), Action::SwitchOff), Ok(()));
    let slotted = coordinator(
        &[support::A {
            person: true,
            slotted: true,
        }],
        Limits::PRODUCT,
    );
    slotted.tick();
    let (ready, kind) = open_item(&slotted);
    assert_eq!(kind, ItemKind::Ready);
    tick_that_panics(&c);
    tick_that_panics(&slotted);
    let m = mark(&c);
    assert_eq!(
        c.control(&agent(1), Action::SwitchOn),
        Err(Cause::NoService),
        "switch_on"
    );
    assert_eq!(
        c.control(&agent(0), Action::RunNow),
        Err(Cause::NoService),
        "run_now"
    );
    let step = shown_step_mark_or_zero(&slotted, &ready);
    assert_eq!(
        slotted.answer(&ready, Choice::Start, None, step),
        Err(Cause::NoService),
        "the start of a ready item"
    );
    assert_eq!(started(&slotted).len(), 0);
    assert!(
        !calls_since(&c, m)
            .iter()
            .any(|call| matches!(call, Call::Start(_) | Call::Log(_))),
        "nothing started or stored"
    );
    assert_eq!(
        c.control(&agent(0), Action::Stop),
        Ok(()),
        "a stop is taken"
    );
    assert_eq!(cancels_of(&c, m, &run), 1);
    c.tick();
    slotted.tick();
    assert_eq!(c.control(&agent(1), Action::SwitchOn), Ok(()));
    let step = shown_step_mark_or_zero(&slotted, &ready);
    assert_eq!(slotted.answer(&ready, Choice::Start, None, step), Ok(()));
}

#[test]
fn rd80_a_first_tick_that_panics_leaves_the_stand_in() {
    let c = Coordinator::new(owner(&[MANUAL], Limits::PRODUCT), Limits::PRODUCT);
    tick_that_panics(&c);
    let s = c.snapshot();
    assert!(s.rows.is_empty() && s.items.is_empty());
    assert_eq!(s.note, Some(text("note.no_service")));
    assert_eq!(c.control(&agent(0), Action::RunNow), Err(Cause::NoService));
}

#[test]
fn rd122_a_run_that_stood_through_a_tick_that_did_not_return_ends_with_its_defers_not_known() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    c.tick();
    tick_that_panics(&c);
    c.tick();
    c.ports().ports.end_runner(&run);
    c.on_run(&run, RunEvent::Ended(Some(Outcome::Completed)))
        .unwrap();
    assert!(events(&c).contains(&What::RunEnded {
        run: run.clone(),
        cause: RunEnd::Completed,
        defers: None
    }));
    ended_in_store(&c, &run, Outcome::Completed);
    let next = start(&c, 0);
    c.tick();
    c.ports().ports.end_runner(&next);
    c.on_run(&next, RunEvent::Ended(Some(Outcome::Completed)))
        .unwrap();
    assert!(
        events(&c).contains(&What::RunEnded {
            run: next,
            cause: RunEnd::Completed,
            defers: SEEN_WHOLE
        }),
        "a run that started after the mark went is seen whole"
    );
}

const LOST_WORLDS: [&str; 4] = [
    "answer_confirm",
    "answer_confirm_after_record",
    "answer_confirm_after_send",
    "log_answered",
];

fn approved_under(
    run: &RunId,
    approval: Option<halvo_coord::types::ApprovalId>,
) -> halvo_coord::guard::EffectRecord {
    halvo_coord::guard::EffectRecord {
        run: run.clone(),
        agent: agent(0),
        effect: halvo_coord::types::EffectId::parse(&format!("{:032x}", 0xe1)).unwrap(),
        state: halvo_coord::guard::EffectState::Approved,
        class: Class::Send,
        digest: halvo_coord::types::Digest::parse(&"ab".repeat(32)).unwrap(),
        approval,
    }
}

fn lost_lapses(c: &Coordinator<Both>, item: &ItemId) -> usize {
    events(c)
        .iter()
        .filter(|w| {
            matches!(w, What::ItemLapsed { item: i, lapsed } if i == item && lapsed.cause().name() == "answer_lost")
        })
        .count()
}

fn decisions_since(c: &Coordinator<Both>, m: usize) -> Vec<halvo_coord::types::Decision> {
    calls_since(c, m)
        .into_iter()
        .filter_map(|call| match call {
            Call::AnswerConfirm(_, _, d) => Some(d),
            _ => None,
        })
        .collect()
}

#[test]
fn rd84_an_answer_whose_move_did_not_return_is_settled_by_the_next_move_with_a_stop() {
    for world in LOST_WORLDS {
        for next in ["tick", "confirm", "stop", "press"] {
            let case = format!("{world}, then {next}");
            let (c, run, item) = armed_confirmation();
            let m = mark(&c);
            panicking(&c, world, || c.answer(&item, Choice::Allow, None, 0));
            let sent = decisions_since(&c, m);
            assert_eq!(
                sent.len(),
                usize::from(world == "answer_confirm_after_send"),
                "{case}"
            );
            let approval = sent.first().map(|d| d.approvals()[0].approval.clone());
            if approval.is_some() {
                assert_eq!(
                    c.check_record(&approved_under(&run, approval.clone())),
                    Err(halvo_coord::guard::Refusal::Approval),
                    "{case}: no row of the sent allow before the settle"
                );
            }
            match next {
                "tick" => c.tick(),
                "confirm" => {
                    let _ = c.on_run(&run, ask_confirm(&run, 0));
                }
                "stop" => assert_eq!(c.control(&agent(0), Action::Stop), Ok(()), "{case}"),
                _ => {
                    let _ = c.answer(&item, Choice::Allow, None, 0);
                }
            }
            assert_eq!(lost_lapses(&c, &item), 1, "{case}: one answer_lost");
            assert_eq!(cancels_of(&c, m, &run), 1, "{case}: one cancel");
            assert_eq!(
                decisions_since(&c, m).len(),
                sent.len(),
                "{case}: no decision"
            );
            assert!(
                !events(&c)
                    .iter()
                    .any(|w| matches!(w, What::ItemReleased { item: i, .. } if i == &item)),
                "{case}: no item_released"
            );
            let s = c.snapshot();
            assert!(s.items.is_empty(), "{case}: no card");
            assert_eq!(
                row_line(&s, 0),
                text("line.working.stopping"),
                "{case}: the board holds the stop"
            );
            assert_eq!(
                c.check_record(&approved_under(&run, approval.clone())),
                Err(halvo_coord::guard::Refusal::Approval),
                "{case}: the table holds the stop"
            );
            c.tick();
            assert_eq!(lost_lapses(&c, &item), 1, "{case}: and no second");
            assert_eq!(cancels_of(&c, m, &run), 1, "{case}: once");
            c.ports().ports.end_runner(&run);
            assert_eq!(c.on_run(&run, RunEvent::Ended(None)), Ok(()), "{case}");
            assert!(
                events(&c)
                    .iter()
                    .any(|w| matches!(w, What::RunEnded { run: r, cause: RunEnd::Failed, .. } if r == &run)),
                "{case}: the run ended failed"
            );
            let s = c.snapshot();
            assert_ne!(
                s.rows[0].status,
                Status::Working,
                "{case}: the row not working"
            );
            ended_in_store(&c, &run, Outcome::Failed);
            assert_eq!(c.control(&agent(0), Action::RunNow), Ok(()), "{case}");
            assert_eq!(started(&c).len(), 2, "{case}: a second run starts");
        }
    }
}

#[test]
fn rd84_a_move_that_returned_leaves_an_answer_to_its_own_rules() {
    let (c, run, item) = armed_confirmation();
    c.ports().drop_decision.set(true);
    assert_eq!(
        c.answer(&item, Choice::Allow, None, 0),
        Err(Cause::NoService),
        "the release failed after the record"
    );
    let m = mark(&c);
    c.tick();
    c.ports().ports.end_runner(&run);
    assert_eq!(c.on_run(&run, RunEvent::Ended(None)), Ok(()));
    assert_eq!(lost_lapses(&c, &item), 0);
    assert_eq!(cancels_of(&c, m, &run), 0);
}

#[test]
fn rd84_a_stop_stored_failed_reads_back_as_the_stop_answer_lost_and_ends_failed() {
    use halvo_coord::run::{run_step, Ending, Next, Phase, RunMove, StopCause};
    let stop = halvo_coord::schedule::Stop {
        at: T0,
        cause: RunEnd::Failed,
    };
    let phase = Phase::of(&halvo_coord::types::Live::Stopping, Some(stop));
    assert_eq!(phase, Phase::Stopping(StopCause::AnswerLost));
    for ending in [Ending::Kill, Ending::Frame(None)] {
        assert_eq!(
            run_step(Some(&phase), RunMove::End(ending)),
            Ok(Next::Ended(RunEnd::Failed)),
            "run_step's end and the stored cause are one (ruling RD-9)"
        );
    }
}

fn due_life() -> Coordinator<Both> {
    let due = support::A {
        person: false,
        slotted: true,
    };
    Coordinator::new(owner(&[due], Limits::PRODUCT), Limits::PRODUCT)
}

#[test]
fn rd80_a_first_tick_that_panics_after_recovery_leaves_the_stand_in_and_no_card() {
    let c = due_life();
    let held = c.snapshot();
    panicking(&c, "start", || c.tick());
    let Next::Changed(s) = c.next(held.seq) else {
        panic!("the waiting call takes a state");
    };
    assert!(s.rows.is_empty() && s.items.is_empty(), "{s:?}");
    assert_eq!(s.note, Some(text("note.no_service")));
    assert_eq!(c.card(&agent(0)), Err(Cause::NoService));
    assert_eq!(c.control(&agent(0), Action::RunNow), Err(Cause::NoService));
    c.tick();
    let s = c.snapshot();
    assert_eq!(
        s.rows.len(),
        1,
        "the first tick that returns shows the rows"
    );
    assert_eq!(s.note, None);
}

#[test]
fn rd80_a_tick_that_returns_while_another_panic_unwinds_sets_no_mark() {
    struct TickOnDrop<'a>(&'a Coordinator<Both>);
    impl Drop for TickOnDrop<'_> {
        fn drop(&mut self) {
            self.0.tick();
        }
    }
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let held = c.snapshot();
    let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _tick = TickOnDrop(&c);
        panic!("a panic of the caller, outside the coordinator");
    }));
    assert!(out.is_err());
    let s = c.snapshot();
    assert_eq!(s.note, None, "the tick returned");
    assert_eq!(s.seq, held.seq, "no move was lost and nothing differs");
    assert_eq!(c.control(&agent(0), Action::RunNow), Ok(()));
}

#[test]
fn rd80_same_as_the_first_call_after_a_lost_tick_answers_the_state_with_the_note() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let held = c.snapshot();
    tick_that_panics(&c);
    let same = c.same();
    assert_eq!(same.seq, held.seq + 1);
    assert_eq!(same.note, Some(text("note.out_of_date")));
    assert_eq!(
        same.rows, held.rows,
        "the whole state, not the empty lists of no change"
    );
    assert!(matches!(c.next(same.seq), Next::Same));
}

#[test]
fn rd84_a_settle_whose_clock_fails_holds_the_moves_own_work_until_it_settles() {
    for next in ["confirm", "press", "native", "typed", "tick"] {
        let (c, run, item) = armed_confirmation();
        let m = mark(&c);
        panicking(&c, "answer_confirm_after_send", || {
            c.answer(&item, Choice::Allow, None, 0)
        });
        let sent = decisions_since(&c, m).len();
        c.ports()
            .fail_read_at
            .set(Some(u32::from(matches!(next, "press" | "native"))));
        let at = mark(&c);
        let out = match next {
            "confirm" => c.on_run(&run, ask_confirm(&run, 0)),
            "press" => c.answer(&item, Choice::Reject, None, 0),
            "native" => c.answer_native(&item, ConfirmAnswer::Reject),
            "typed" => c.setup_typed(&item, Param::Start, "Flat 2", 0),
            _ => {
                c.tick();
                let worked = calls_since(&c, at)
                    .iter()
                    .any(|call| matches!(call, Call::Agents | Call::Workflows));
                if worked {
                    Ok(())
                } else {
                    Err(Cause::NoService)
                }
            }
        };
        c.ports().fail_read_at.set(None);
        assert_eq!(
            out,
            Err(Cause::NoService),
            "{next}: the move waits for the settle"
        );
        assert_eq!(lost_lapses(&c, &item), 0, "{next}");
        assert_eq!(cancels_of(&c, m, &run), 0, "{next}");
        c.tick();
        assert!(
            !events(&c).iter().any(|w| matches!(
                w,
                What::ItemLapsed { item: i, lapsed: Lapsed::Confirm(LapseCause::Cancelled) } if i == &item
            )),
            "{next}: no lapse of the move's own"
        );
        assert_eq!(lost_lapses(&c, &item), 1, "{next}: one answer_lost");
        assert_eq!(cancels_of(&c, m, &run), 1, "{next}: one cancel");
        assert_eq!(decisions_since(&c, m).len(), sent, "{next}: no decision");
        assert!(c.snapshot().items.is_empty(), "{next}: no second item");
        c.ports().ports.end_runner(&run);
        assert_eq!(c.on_run(&run, RunEvent::Ended(None)), Ok(()), "{next}");
        assert!(
            events(&c).iter().any(
                |w| matches!(w, What::RunEnded { run: r, cause: RunEnd::Failed, .. } if r == &run)
            ),
            "{next}: the run ended failed"
        );
    }
}

#[test]
fn rd80_while_the_tick_mark_stands_switch_off_and_the_cards_open_are_answered_as_always() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    tick_that_panics(&c);
    assert_eq!(
        c.snapshot().note,
        Some(text("note.out_of_date")),
        "the mark stands"
    );
    let m = mark(&c);
    assert!(c.card(&agent(0)).is_ok(), "the card's open");
    assert_eq!(
        c.control(&agent(0), Action::SwitchOff),
        Ok(()),
        "Switch off is never withheld"
    );
    assert_eq!(cancels_of(&c, m, &run), 1, "the switch-off stops the run");
    assert_eq!(c.snapshot().note, Some(text("note.out_of_date")));
}

#[test]
fn rd84_after_a_returned_answer_a_tick_whose_clock_read_fails_still_does_its_work() {
    let (c, _run, item) = armed_confirmation();
    assert_eq!(c.answer(&item, Choice::Allow, None, 0), Ok(()));
    c.ports().fail_read_at.set(Some(0));
    let at = mark(&c);
    c.tick();
    c.ports().fail_read_at.set(None);
    assert!(
        calls_since(&c, at)
            .iter()
            .any(|call| matches!(call, Call::Agents | Call::Workflows)),
        "a returned answer leaves no note, so no settle holds the tick's work"
    );
}

#[test]
fn rd84_the_settle_of_the_second_agents_item_logs_under_that_agent() {
    let limits = Limits {
        max_live_runs: 2,
        ..Limits::PRODUCT
    };
    let c = coordinator(&[MANUAL, MANUAL], limits);
    let _run0 = start(&c, 0);
    let run1 = start(&c, 1);
    c.on_run(&run1, ask_confirm(&run1, 1)).unwrap();
    let (item, _) = open_item(&c);
    c.on_screen(&item);
    c.ports().ports.set_clock(T0 + 1_000, DAY);
    c.tick();
    let step = shown_step_mark_or_zero(&c, &item);
    panicking(&c, "answer_confirm", || {
        c.answer(&item, Choice::Allow, None, step)
    });
    c.tick();
    let lapsed: Vec<AgentId> = c
        .ports()
        .ports
        .events
        .borrow()
        .iter()
        .filter(|e| {
            matches!(&e.what, What::ItemLapsed { item: i, lapsed } if i == &item && lapsed.cause().name() == "answer_lost")
        })
        .map(|e| e.agent.clone())
        .collect();
    assert_eq!(
        lapsed,
        vec![agent(1)],
        "one answer_lost row, under the item's own agent"
    );
}

#[test]
fn rd84_the_settles_row_takes_the_moves_one_clock_read() {
    let (c, _run, item) = armed_confirmation();
    panicking(&c, "answer_confirm_after_send", || {
        c.answer(&item, Choice::Allow, None, 0)
    });
    c.ports().fail_read_at.set(Some(1));
    c.tick();
    c.ports().fail_read_at.set(None);
    c.tick();
    assert_eq!(
        lost_lapses(&c, &item),
        1,
        "the item_lapsed row is stamped at the settle's one read (RD-84 item 2, RD-101 item 1)"
    );
}

#[test]
fn rd84_a_settle_whose_cancel_panics_still_ends_the_run_failed_at_the_grace() {
    let (c, run, item) = armed_confirmation();
    panicking(&c, "answer_confirm_after_send", || {
        c.answer(&item, Choice::Allow, None, 0)
    });
    // Turned by ruling RD-138 Addition 1 (M5): the settle's unwind is contained to its own
    // agent, so the move that took it up returns.
    c.ports().panics.set(Some("cancel"));
    let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        c.control(&agent(0), Action::Stop)
    }));
    c.ports().panics.set(None);
    assert!(
        out.is_ok(),
        "the Stop returns though the settle's cancel panicked"
    );
    assert_eq!(lost_lapses(&c, &item), 0, "the row waits in the ledger");
    let later = T0 + Limits::PRODUCT.arm_ms + 1_000 + Limits::PRODUCT.cancel_grace_s * 1000;
    c.ports().ports.set_clock(later, DAY);
    c.tick();
    c.tick();
    assert!(
        events(&c)
            .iter()
            .any(|w| matches!(w, What::RunEnded { run: r, cause: RunEnd::Failed, .. } if r == &run)),
        "the run is stopped on the board before the cancel, so the kill at the grace ends it failed"
    );
    assert_eq!(lost_lapses(&c, &item), 1, "one answer_lost row");
}

#[test]
fn rd122_a_lost_move_that_is_no_tick_keeps_the_defers_of_a_run_seen_whole() {
    let limits = Limits {
        max_live_runs: 2,
        ..Limits::PRODUCT
    };
    let c = coordinator(&[MANUAL, MANUAL], limits);
    let run0 = start(&c, 0);
    c.tick();
    panicking(&c, "start", || c.control(&agent(1), Action::RunNow));
    c.tick();
    c.ports().ports.end_runner(&run0);
    assert_eq!(
        c.on_run(&run0, RunEvent::Ended(Some(Outcome::Completed))),
        Ok(())
    );
    let defers = events(&c).iter().find_map(|w| match w {
        What::RunEnded { run: r, defers, .. } if r == &run0 => Some(*defers),
        _ => None,
    });
    assert!(
        matches!(defers, Some(Some(_))),
        "only a tick that did not return makes the defers not known: {defers:?}"
    );
}

#[test]
fn rd122_an_owed_run_end_keeps_its_defers_through_a_lost_tick() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    c.tick();
    c.ports().ports.end_runner(&run);
    c.ports().loss.set(Some(support::Loss::Nothing));
    let _ = c.on_run(&run, RunEvent::Ended(Some(Outcome::Completed)));
    c.ports().loss.set(None);
    let owed_first = !events(&c)
        .iter()
        .any(|w| matches!(w, What::RunEnded { run: r, .. } if r == &run));
    tick_that_panics(&c);
    c.tick();
    c.tick();
    let defers = events(&c).iter().find_map(|w| match w {
        What::RunEnded { run: r, defers, .. } if r == &run => Some(*defers),
        _ => None,
    });
    assert_eq!(
        (owed_first, matches!(defers, Some(Some(_)))),
        (true, true),
        "a run that left the board before the lost tick, its end owed, was seen whole: {defers:?}"
    );
}

#[test]
fn rd80_the_publish_after_a_lost_move_rings_the_bell_and_sets_the_trays_count() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    c.on_run(&run, ask_confirm(&run, 0)).unwrap();
    tick_that_panics(&c);
    let m = mark(&c);
    let s = c.snapshot();
    assert_eq!(s.note, Some(text("note.out_of_date")));
    let calls = calls_since(&c, m);
    assert!(calls.contains(&Call::Changed), "the bell: {calls:?}");
    assert!(
        calls.contains(&Call::Attention(1)),
        "the tray's count of the agents that need the person: {calls:?}"
    );
}

#[test]
fn rd84_the_quits_first_step_settles_a_lost_answer_first() {
    let (c, run, item) = armed_confirmation();
    let m = mark(&c);
    panicking(&c, "answer_confirm_after_send", || {
        c.answer(&item, Choice::Allow, None, 0)
    });
    let _ = c.quit_stop();
    assert_eq!(
        lost_lapses(&c, &item),
        1,
        "the settle comes before the quit's lapses"
    );
    assert!(
        !events(&c).iter().any(|w| matches!(
            w,
            What::ItemLapsed { item: i, lapsed } if i == &item && lapsed.cause().name() == "app_ended"
        )),
        "the quit does not lapse the settled item again"
    );
    assert_eq!(cancels_of(&c, m, &run), 1, "one cancel");
}

#[test]
fn rd84_the_quits_second_step_settles_a_lost_answer_first() {
    let (c, run, item) = armed_confirmation();
    let m = mark(&c);
    panicking(&c, "answer_confirm_after_send", || {
        c.answer(&item, Choice::Allow, None, 0)
    });
    c.quit_kill();
    assert_eq!(
        lost_lapses(&c, &item),
        1,
        "the settle comes before the quit's kill"
    );
    assert_eq!(cancels_of(&c, m, &run), 1, "the settle's one cancel");
}

fn lost_answer_beside_other_runs(others: usize) -> (Coordinator<Both>, RunId, ItemId, Vec<RunId>) {
    lost_answer_beside(others, 0)
}

fn lost_answer_beside(
    others: usize,
    idle: usize,
) -> (Coordinator<Both>, RunId, ItemId, Vec<RunId>) {
    let limits = Limits {
        max_live_runs: (1 + others + idle) as u32,
        ..Limits::PRODUCT
    };
    let spec: Vec<support::A> = std::iter::repeat_n(MANUAL, 1 + others + idle).collect();
    let c = coordinator(&spec, limits);
    let run0 = start(&c, 0);
    c.on_run(&run0, ask_confirm(&run0, 0)).unwrap();
    let (item, _) = open_item(&c);
    assert_eq!(c.shown_on_page(&item, 0), Ok(()));
    c.ports().ports.set_clock(T0 + Limits::PRODUCT.arm_ms, DAY);
    c.tick();
    let runs: Vec<RunId> = (1..=others).map(|n| start(&c, n)).collect();
    panicking(&c, "answer_confirm_after_send", || {
        c.answer(&item, Choice::Allow, None, 0)
    });
    (c, run0, item, runs)
}

fn clock_fails(c: &Coordinator<Both>) {
    c.ports().clock_fails.set(true);
}

fn clock_answers(c: &Coordinator<Both>) {
    c.ports().clock_fails.set(false);
    c.ports().fail_read_at.set(None);
    c.ports().fail_reads_after.set(None);
    c.ports().day_fails.set(false);
}

fn lapses_of(c: &Coordinator<Both>, item: &ItemId, cause: &str) -> usize {
    events(c)
        .iter()
        .filter(|w| {
            matches!(w, What::ItemLapsed { item: i, lapsed } if i == item && lapsed.cause().name() == cause)
        })
        .count()
}

fn ends_of(c: &Coordinator<Both>, run: &RunId) -> Vec<RunEnd> {
    events(c)
        .iter()
        .filter_map(|w| match w {
            What::RunEnded { run: r, cause, .. } if r == run => Some(*cause),
            _ => None,
        })
        .collect()
}

fn item_of_agent(c: &Coordinator<Both>, n: usize) -> ItemId {
    c.snapshot()
        .items
        .iter()
        .find(|i| i.agent() == &agent(n))
        .map(|i| i.item().clone())
        .unwrap()
}

fn after_the_grace(c: &Coordinator<Both>) {
    let later = T0 + Limits::PRODUCT.arm_ms + 1_000 + Limits::PRODUCT.cancel_grace_s * 1000;
    c.ports().ports.set_clock(later, DAY);
    c.tick();
}

#[test]
fn rd133_another_agents_stop_goes_on_with_no_clock_while_a_settle_waits() {
    let (c, _run0, _item, runs) = lost_answer_beside_other_runs(1);
    let run1 = runs[0].clone();
    c.on_run(&run1, ask_confirm(&run1, 1)).unwrap();
    let item1 = item_of_agent(&c, 1);
    clock_fails(&c);
    let m = mark(&c);
    let out = c.control(&agent(1), Action::Stop);
    assert_eq!(out, Ok(()), "Stop is refused by no clock read (RD-133 1)");
    assert_eq!(
        cancels_of(&c, m, &run1),
        1,
        "the cancel reaches the runner in that move"
    );
    assert_eq!(
        lapses_of(&c, &item1, "cancelled"),
        0,
        "its row waits for a stamp"
    );
    clock_answers(&c);
    c.tick();
    assert_eq!(
        lapses_of(&c, &item1, "cancelled"),
        1,
        "the row follows at the next clock read"
    );
    c.ports().ports.end_runner(&run1);
    assert_eq!(c.on_run(&run1, RunEvent::Ended(None)), Ok(()));
    assert_eq!(ends_of(&c, &run1), vec![RunEnd::Cancelled]);
}

#[test]
fn rd133_a_working_runs_stop_goes_on_with_no_clock_and_the_grace_counts_from_the_stamp() {
    let (c, _run0, _item, runs) = lost_answer_beside_other_runs(1);
    let run1 = runs[0].clone();
    clock_fails(&c);
    let m = mark(&c);
    assert_eq!(c.control(&agent(1), Action::Stop), Ok(()));
    assert_eq!(cancels_of(&c, m, &run1), 1, "the cancel in that move");
    clock_answers(&c);
    c.tick();
    assert_eq!(
        kills_of(&c, m, &run1),
        0,
        "no kill before cancel_grace_s from the read that stamped the stop"
    );
    after_the_grace(&c);
    assert_eq!(
        kills_of(&c, m, &run1),
        1,
        "killed once the grace from that read passed"
    );
    assert_eq!(ends_of(&c, &run1), vec![RunEnd::Cancelled]);
}

#[test]
fn rd133_another_agents_switch_off_goes_on_with_no_clock_and_its_hold_follows() {
    let (c, _run0, _item, runs) = lost_answer_beside_other_runs(1);
    let run1 = runs[0].clone();
    clock_fails(&c);
    let m = mark(&c);
    let out = c.control(&agent(1), Action::SwitchOff);
    assert_eq!(
        out,
        Ok(()),
        "Switch off is refused by no clock read (RD-133 1)"
    );
    assert_eq!(
        cancels_of(&c, m, &run1),
        1,
        "the cancel reaches the runner in that move"
    );
    clock_answers(&c);
    c.tick();
    let holds = c
        .ports()
        .ports
        .events
        .borrow()
        .iter()
        .filter(|e| {
            e.agent == agent(1)
                && matches!(
                    e.what,
                    What::HoldSet {
                        cause: HoldSetCause::Person
                    }
                )
        })
        .count();
    assert_eq!(holds, 1, "the hold_set follows at the next clock read");
    assert_eq!(row_line(&c.snapshot(), 1), text("line.off.person"));
    assert_eq!(c.control(&agent(1), Action::RunNow), Err(Cause::Hold));
}

#[test]
fn rd133_while_a_settle_waits_only_the_moves_on_its_item_and_its_run_wait() {
    let (c, run0, item, runs) = lost_answer_beside_other_runs(1);
    let run1 = runs[0].clone();
    clock_fails(&c);
    assert_eq!(
        c.answer(&item, Choice::Reject, None, 0),
        Err(Cause::NoService),
        "page answer"
    );
    assert_eq!(
        c.answer_native(&item, ConfirmAnswer::Reject),
        Err(Cause::NoService),
        "native answer"
    );
    assert_eq!(
        c.setup_typed(&item, Param::Start, "Flat 2", 0),
        Err(Cause::NoService),
        "typed value on the item's id"
    );
    assert_eq!(
        c.on_run(&run0, ask_confirm(&run0, 0)),
        Err(Cause::NoService),
        "an event of the waiting item's run"
    );
    assert_eq!(
        c.on_run(&run1, RunEvent::Hello),
        Err(Cause::State),
        "an event of another run goes on to its own answer"
    );
    clock_answers(&c);
    assert_eq!(
        lost_lapses(&c, &item),
        0,
        "nothing settled while the clock failed"
    );
    c.tick();
    assert_eq!(
        lost_lapses(&c, &item),
        1,
        "the next move that reads the clock settles first"
    );
}

#[test]
fn rd133_the_waiting_agents_own_stop_goes_first_and_the_run_ends_stopped() {
    let (c, run0, item, _runs) = lost_answer_beside_other_runs(0);
    let m = mark(&c);
    let sent = decisions_since(&c, m).len();
    clock_fails(&c);
    let at = mark(&c);
    assert_eq!(
        c.control(&agent(0), Action::Stop),
        Ok(()),
        "the own Stop goes on"
    );
    assert_eq!(
        cancels_of(&c, at, &run0),
        1,
        "the cancel reaches the runner in that move"
    );
    clock_answers(&c);
    c.tick();
    c.tick();
    assert_eq!(
        decisions_since(&c, m).len(),
        sent,
        "no decision sent or recorded"
    );
    assert!(
        !events(&c)
            .iter()
            .any(|w| matches!(w, What::ItemReleased { item: i, .. } if i == &item)),
        "no release"
    );
    assert_eq!(
        lapses_of(&c, &item, "answer_lost"),
        1,
        "the item lapses answer_lost once"
    );
    assert_eq!(
        lapses_of(&c, &item, "cancelled"),
        0,
        "and with no other cause"
    );
    assert_eq!(
        cancels_of(&c, at, &run0),
        1,
        "the later settle touches nothing"
    );
    c.ports().ports.end_runner(&run0);
    assert_eq!(c.on_run(&run0, RunEvent::Ended(None)), Ok(()));
    assert_eq!(
        ends_of(&c, &run0),
        vec![RunEnd::Cancelled],
        "run_ended once, the person's stop"
    );
}

#[test]
fn rd133_the_guard_refuses_at_once_in_the_window_before_the_stamp() {
    let (c, _run0, _item, runs) = lost_answer_beside_other_runs(1);
    let run1 = runs[0].clone();
    clock_fails(&c);
    assert_eq!(c.control(&agent(1), Action::Stop), Ok(()));
    let record = halvo_coord::guard::EffectRecord {
        agent: agent(1),
        ..approved_under(&run1, None)
    };
    assert_eq!(
        c.check_record(&record),
        Err(halvo_coord::guard::Refusal::Approval),
        "the guard's table holds the stop before any stamp"
    );
    clock_answers(&c);
}

#[test]
fn rd133_a_clock_that_never_answers_leaves_the_run_stopped_with_its_rows_unstamped() {
    let (c, _run0, _item, runs) = lost_answer_beside_other_runs(1);
    let run1 = runs[0].clone();
    c.on_run(&run1, ask_confirm(&run1, 1)).unwrap();
    let item1 = item_of_agent(&c, 1);
    clock_fails(&c);
    let m = mark(&c);
    assert_eq!(c.control(&agent(1), Action::Stop), Ok(()));
    for _ in 0..3 {
        c.tick();
    }
    assert_eq!(cancels_of(&c, m, &run1), 1, "stopped and cancelled once");
    assert_eq!(kills_of(&c, m, &run1), 0, "no kill with no stamp");
    assert_eq!(
        lapses_of(&c, &item1, "cancelled"),
        0,
        "the row stays unstamped"
    );
    assert_eq!(
        row_line(&c.snapshot(), 1),
        text("line.working.stopping"),
        "the board holds the stop"
    );
    clock_answers(&c);
}

#[test]
fn rd133_rows_that_waited_for_a_stamp_are_read_lost_by_the_next_lives_recovery() {
    let (c, _run0, _item, runs) = lost_answer_beside_other_runs(1);
    let run1 = runs[0].clone();
    c.on_run(&run1, ask_confirm(&run1, 1)).unwrap();
    let item1 = item_of_agent(&c, 1);
    clock_fails(&c);
    assert_eq!(c.control(&agent(1), Action::Stop), Ok(()));
    assert_eq!(
        lapses_of(&c, &item1, "cancelled"),
        0,
        "the row never left this life"
    );
    let life = next_life(&c, |_| {});
    let closed: Vec<String> = life
        .ports()
        .ports
        .events
        .borrow()
        .iter()
        .filter_map(|e| match &e.what {
            What::ItemLapsed { item: i, lapsed } if i == &item1 => {
                Some(lapsed.cause().name().to_string())
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        closed,
        vec!["app_ended".to_string()],
        "recovery lapses the open item"
    );
    let ended: Vec<RunEnd> = life
        .ports()
        .ports
        .events
        .borrow()
        .iter()
        .filter_map(|e| match &e.what {
            What::RunEnded { run: r, cause, .. } if r == &run1 => Some(*cause),
            _ => None,
        })
        .collect();
    assert_eq!(
        ended,
        vec![RunEnd::Interrupted],
        "and ends the run interrupted"
    );
    let s = life.snapshot();
    assert_eq!(
        row_line(&s, 1),
        text("line.all_set.stopped_early"),
        "the row reads the run stopped early (RD-133, Addition 1 item 9)"
    );
    assert!(
        !s.items.iter().any(|i| i.agent() == &agent(1)),
        "and no card of the lost item stands"
    );
    life.ports().ports.histories.borrow_mut().push((
        agent(1),
        halvo_coord::types::RunHistory {
            run: PastRun {
                run: run1.clone(),
                day: DAY,
                outcome: Outcome::Failed,
                unknown: 0,
            },
            effects: Vec::new(),
        },
    ));
    assert_eq!(
        life.card(&agent(1)).unwrap().runs[0].text,
        text("line.run.interrupted"),
        "the person reads that it stopped because Halvo closed, not that they stopped it \
         (RD-133, Addition 2 C)"
    );
}

#[test]
fn rd133_a_start_of_another_agent_is_not_refused_because_a_settle_waits() {
    let (c, _run0, item, _runs) = lost_answer_beside(0, 1);
    c.ports().fail_read_at.set(Some(0));
    let m = mark(&c);
    assert_eq!(c.control(&agent(1), Action::RunNow), Ok(()), "H2 (ii)");
    assert!(
        calls_since(&c, m)
            .iter()
            .any(|call| matches!(call, Call::Start(_))),
        "the start goes out"
    );
    assert_eq!(
        lost_lapses(&c, &item),
        0,
        "the settle still waits for its own read"
    );
    clock_answers(&c);
}

#[test]
fn rd133_each_tick_held_by_a_waiting_settle_counts_a_failed_defer_for_each_run() {
    let (c, run0, item, runs) = lost_answer_beside_other_runs(1);
    let run1 = runs[0].clone();
    clock_fails(&c);
    for _ in 0..4 {
        c.tick();
    }
    clock_answers(&c);
    c.tick();
    assert_eq!(
        lost_lapses(&c, &item),
        1,
        "settled at the first tick that reads the clock"
    );
    for run in [&run0, &run1] {
        c.ports().ports.end_runner(run);
        let _ = c.on_run(run, RunEvent::Ended(Some(Outcome::Completed)));
    }
    for run in [&run0, &run1] {
        let defers = events(&c).iter().find_map(|w| match w {
            What::RunEnded { run: r, defers, .. } if r == run => Some(*defers),
            _ => None,
        });
        assert_eq!(
            defers.flatten().map(|d| d.ticks_failed),
            Some(4),
            "four held ticks, each known to send no defer (RD-133 4)"
        );
    }
}

#[test]
fn rd133_a_queued_start_stays_queued_at_a_runs_end_while_the_mark_stands() {
    let c = coordinator(&[MANUAL, MANUAL], Limits::PRODUCT);
    let run0 = start(&c, 0);
    assert_eq!(
        c.control(&agent(1), Action::RunNow),
        Ok(()),
        "queued behind run 0"
    );
    assert_eq!(c.queued(), vec![(agent(1), false)]);
    let before = started(&c).len();
    tick_that_panics(&c);
    assert_eq!(c.control(&agent(1), Action::RunNow), Err(Cause::NoService));
    c.ports().ports.end_runner(&run0);
    let _ = c.on_run(&run0, RunEvent::Ended(Some(Outcome::Completed)));
    assert_eq!(started(&c).len(), before, "no start while the mark stands");
    assert_eq!(c.queued(), vec![(agent(1), false)], "the queue keeps it");
    assert_eq!(c.snapshot().note, Some(text("note.out_of_date")));
    c.tick();
    assert_eq!(
        started(&c).len(),
        before + 1,
        "the run drive of a tick that returned starts it"
    );
}

#[test]
fn rd133_a_queued_start_waits_for_a_tick_that_returns_with_one_live_run() {
    let one = Limits {
        max_live_runs: 1,
        ..Limits::PRODUCT
    };
    let c = coordinator(&[MANUAL, MANUAL], one);
    let first = start(&c, 0);
    assert_eq!(c.control(&agent(1), Action::RunNow), Ok(()));
    assert_eq!(started(&c).len(), 1, "agent 1 waits in the queue");
    tick_that_panics(&c);
    let m = mark(&c);
    c.ports().ports.end_runner(&first);
    c.on_run(&first, RunEvent::Ended(Some(Outcome::Completed)))
        .unwrap();
    assert!(
        !calls_since(&c, m)
            .iter()
            .any(|call| matches!(call, Call::Start(_))),
        "no start went out while the mark of the tick stood"
    );
    assert_eq!(c.snapshot().note, Some(text("note.out_of_date")));
    c.tick();
    assert!(
        calls_since(&c, m)
            .iter()
            .any(|call| matches!(call, Call::Start(_))),
        "the tick that returned starts it"
    );
}

#[test]
fn rd133_a_switch_off_of_a_queued_agent_under_the_mark_takes_it_out_of_the_queue() {
    let c = coordinator(&[MANUAL, MANUAL], Limits::PRODUCT);
    let run0 = start(&c, 0);
    assert_eq!(c.control(&agent(1), Action::RunNow), Ok(()));
    let before = started(&c).len();
    tick_that_panics(&c);
    assert_eq!(c.control(&agent(1), Action::SwitchOff), Ok(()));
    assert_eq!(c.queued(), vec![], "out of the queue");
    c.ports().ports.end_runner(&run0);
    let _ = c.on_run(&run0, RunEvent::Ended(Some(Outcome::Completed)));
    c.tick();
    assert_eq!(
        started(&c).len(),
        before,
        "it does not start when the tick returns"
    );
}

#[test]
fn rd133_own01_another_agents_stop_is_not_withheld_while_a_settle_waits_for_the_clock() {
    let limits = Limits {
        max_live_runs: 2,
        ..Limits::PRODUCT
    };
    let c = coordinator(&[MANUAL, MANUAL], limits);
    let (_run0, item) = allowed(&c);
    let run1 = start(&c, 1);
    let step = shown_step_mark_or_zero(&c, &item);
    panicking(&c, "answer_confirm", || {
        c.answer(&item, Choice::Allow, None, step)
    });
    c.ports().fail_read_at.set(Some(0));
    let m = mark(&c);
    let out = c.control(&agent(1), Action::Stop);
    c.ports().fail_read_at.set(None);
    assert_eq!(
        out,
        Ok(()),
        "RD-80 4.5: the Stop of an agent whose item no settle holds is answered as always"
    );
    assert!(
        calls_since(&c, m)
            .iter()
            .any(|call| matches!(call, Call::Cancel(r) if r == &run1)),
        "the Stop reaches the other agent's run"
    );
}

/// own-01 again with every read of the move failing (`clock_fails`), once with agent 1 holding
/// no open item, once with its run waiting on an open item: the Stop goes on and the cancel
/// goes out in the same move (ruling RD-133, Addition 1 item 1).
#[test]
fn rd133_own01_with_the_clock_failing_for_the_whole_move_with_and_without_an_open_item() {
    for open in [false, true] {
        let limits = Limits {
            max_live_runs: 2,
            ..Limits::PRODUCT
        };
        let c = coordinator(&[MANUAL, MANUAL], limits);
        let (_run0, item) = allowed(&c);
        let run1 = start(&c, 1);
        if open {
            c.on_run(&run1, ask_confirm(&run1, 1)).unwrap();
        }
        let step = shown_step_mark_or_zero(&c, &item);
        panicking(&c, "answer_confirm", || {
            c.answer(&item, Choice::Allow, None, step)
        });
        clock_fails(&c);
        let m = mark(&c);
        let out = c.control(&agent(1), Action::Stop);
        assert_eq!(out, Ok(()), "open {open}: the Stop goes on");
        assert_eq!(
            cancels_of(&c, m, &run1),
            1,
            "open {open}: the cancel in that move"
        );
        assert_eq!(c.live(), 2, "open {open}: run 1 stopping, run 0 waiting");
        clock_answers(&c);
    }
}

/// The last time that this life read the clock, as the board's events store it.
fn at_of(ms: u64) -> i64 {
    (ms as i64) * 1_000_000
}

fn person_holds(c: &Coordinator<Both>, n: usize) -> Vec<halvo_coord::types::Event> {
    c.ports()
        .ports
        .events
        .borrow()
        .iter()
        .filter(|e| {
            e.agent == agent(n)
                && matches!(
                    e.what,
                    What::HoldSet {
                        cause: HoldSetCause::Person
                    }
                )
        })
        .cloned()
        .collect()
}

#[test]
fn rd133_the_waiting_agents_own_switch_off_lapses_answer_lost_once_and_ends_cancelled() {
    let (c, run0, item, _runs) = lost_answer_beside_other_runs(0);
    let last = T0 + Limits::PRODUCT.arm_ms;
    let m = mark(&c);
    let sent = decisions_since(&c, m).len();
    c.ports().ports.set_clock(last + 5_000, DAY);
    clock_fails(&c);
    let at = mark(&c);
    assert_eq!(c.control(&agent(0), Action::SwitchOff), Ok(()));
    assert_eq!(cancels_of(&c, at, &run0), 1, "the cancel in that move");
    let holds = person_holds(&c, 0);
    assert_eq!(
        holds.len(),
        1,
        "the hold_set is stored at once (RD-133, answer (b))"
    );
    assert_eq!(
        holds[0].at,
        at_of(last),
        "at the last time that this life read the clock, before the press"
    );
    assert_eq!(row_line(&c.snapshot(), 0), text("line.off.person"));
    clock_answers(&c);
    c.tick();
    c.tick();
    assert_eq!(decisions_since(&c, m).len(), sent, "no decision");
    assert_eq!(lapses_of(&c, &item, "answer_lost"), 1, "answer_lost once");
    assert_eq!(lapses_of(&c, &item, "cancelled"), 0, "no other lapse");
    assert_eq!(
        cancels_of(&c, at, &run0),
        1,
        "the later settle touches nothing"
    );
    assert_eq!(person_holds(&c, 0).len(), 1, "one hold_set");
    c.ports().ports.end_runner(&run0);
    assert_eq!(c.on_run(&run0, RunEvent::Ended(None)), Ok(()));
    assert_eq!(
        ends_of(&c, &run0),
        vec![RunEnd::Cancelled],
        "run_ended once, cancelled"
    );
    assert!(
        !events(&c).iter().any(|w| matches!(
            w,
            What::HoldSet {
                cause: HoldSetCause::Failures
            }
        )),
        "the cancelled end does not count toward the failures hold"
    );
}

#[test]
fn rd133_in_the_window_a_request_from_the_stopped_run_raises_no_card() {
    let (c, _run0, _item, runs) = lost_answer_beside_other_runs(1);
    let run1 = runs[0].clone();
    clock_fails(&c);
    assert_eq!(c.control(&agent(1), Action::Stop), Ok(()));
    let _ = c.on_run(&run1, ask_confirm(&run1, 1));
    let cards = |c: &Coordinator<Both>| {
        c.snapshot()
            .items
            .iter()
            .filter(|i| i.agent() == &agent(1))
            .count()
    };
    assert_eq!(cards(&c), 0, "no card in the window");
    clock_answers(&c);
    c.tick();
    assert_eq!(cards(&c), 0, "and none after the stamp");
}

#[test]
fn rd133_in_the_window_the_card_leaves_in_the_move_and_an_answer_to_it_sends_no_decision() {
    let (c, _run0, _item, runs) = lost_answer_beside_other_runs(1);
    let run1 = runs[0].clone();
    c.on_run(&run1, ask_confirm(&run1, 1)).unwrap();
    let item1 = item_of_agent(&c, 1);
    clock_fails(&c);
    let at = mark(&c);
    assert_eq!(c.control(&agent(1), Action::Stop), Ok(()));
    assert_eq!(
        decisions_since(&c, at).len(),
        1,
        "the Stop's own reject to the runner, as with a clock that answers"
    );
    let m = mark(&c);
    assert!(
        !c.snapshot().items.iter().any(|i| i.item() == &item1),
        "the card leaves the page in the move"
    );
    assert!(c.answer(&item1, Choice::Reject, None, 0).is_err());
    assert!(c.answer(&item1, Choice::Allow, None, 0).is_err());
    assert_eq!(decisions_since(&c, m), vec![], "no decision in the window");
    clock_answers(&c);
    c.tick();
    assert!(c.answer(&item1, Choice::Reject, None, 0).is_err());
    assert_eq!(decisions_since(&c, m), vec![], "nor after the stamp");
    assert_eq!(lapses_of(&c, &item1, "cancelled"), 1, "the lapse is stored");
}

/// A Switch off under a failing clock takes a queued agent off the queue and holds it off on
/// the board in the move; the clock answers at a tick or at a run's end, and the agent does not
/// start (ruling RD-133, Addition 1 item 4).
#[test]
fn rd133_a_switch_off_under_a_failing_clock_holds_a_queued_agent_off_until_and_after_the_stamp() {
    let one = Limits {
        max_live_runs: 1,
        ..Limits::PRODUCT
    };
    for at_end in [false, true] {
        let c = coordinator(&[MANUAL, MANUAL], one);
        let first = start(&c, 0);
        assert_eq!(c.control(&agent(1), Action::RunNow), Ok(()));
        assert_eq!(c.queued(), vec![(agent(1), false)]);
        clock_fails(&c);
        assert_eq!(c.control(&agent(1), Action::SwitchOff), Ok(()), "{at_end}");
        assert_eq!(c.queued(), vec![], "{at_end}: off the queue in the move");
        assert_eq!(
            c.control(&agent(1), Action::RunNow),
            Err(Cause::Hold),
            "{at_end}: held off on the board in the move"
        );
        assert_eq!(row_line(&c.snapshot(), 1), text("line.off.person"));
        clock_answers(&c);
        if !at_end {
            c.tick();
        }
        c.ports().ports.end_runner(&first);
        c.on_run(&first, RunEvent::Ended(Some(Outcome::Completed)))
            .unwrap();
        c.tick();
        assert_eq!(started(&c).len(), 1, "{at_end}: agent 1 does not start");
        assert_eq!(person_holds(&c, 1).len(), 1, "{at_end}: one hold_set");
        assert_eq!(c.control(&agent(1), Action::RunNow), Err(Cause::Hold));
    }
}

/// A Stop of a queued agent takes it out of the queue at any time, with no read of the clock,
/// with the mark of a lost tick standing and without (ruling RD-133, Addition 1 item 5).
#[test]
fn rd133_a_stop_of_a_queued_agent_takes_it_out_of_the_queue_with_and_without_the_mark() {
    let one = Limits {
        max_live_runs: 1,
        ..Limits::PRODUCT
    };
    for marked in [false, true] {
        let c = coordinator(&[MANUAL, MANUAL], one);
        let first = start(&c, 0);
        assert_eq!(c.control(&agent(1), Action::RunNow), Ok(()));
        if marked {
            tick_that_panics(&c);
        }
        let stored = events(&c).len();
        clock_fails(&c);
        assert_eq!(c.control(&agent(1), Action::Stop), Ok(()), "{marked}");
        assert_eq!(c.queued(), vec![], "{marked}: out of the queue");
        assert_eq!(events(&c).len(), stored, "{marked}: nothing stored");
        clock_answers(&c);
        c.ports().ports.end_runner(&first);
        c.on_run(&first, RunEvent::Ended(Some(Outcome::Completed)))
            .unwrap();
        c.tick();
        assert_eq!(started(&c).len(), 1, "{marked}: agent 1 does not start");
    }
}

/// A run that a Stop reached while a settle waited counts the held ticks only up to its stop
/// (ruling RD-133, Addition 1 item 7).
#[test]
fn rd133_a_run_stopped_while_a_settle_waits_counts_the_held_ticks_up_to_its_stop() {
    let (c, run0, item, runs) = lost_answer_beside_other_runs(1);
    let run1 = runs[0].clone();
    clock_fails(&c);
    c.tick();
    c.tick();
    assert_eq!(c.control(&agent(1), Action::Stop), Ok(()));
    c.tick();
    c.tick();
    clock_answers(&c);
    c.tick();
    assert_eq!(lost_lapses(&c, &item), 1, "settled");
    for run in [&run0, &run1] {
        c.ports().ports.end_runner(run);
        let _ = c.on_run(run, RunEvent::Ended(None));
    }
    let failed = |run: &RunId| {
        events(&c).iter().find_map(|w| match w {
            What::RunEnded { run: r, defers, .. } if r == run => {
                Some(defers.map(|d| d.ticks_failed))
            }
            _ => None,
        })
    };
    assert_eq!(failed(&run0), Some(Some(4)), "run 0: four held ticks");
    assert_eq!(failed(&run1), Some(Some(2)), "run 1: two, up to its stop");
}

/// A tick that did not return makes the defer keys null, and held ticks after it do not make
/// them a count again: null wins (ruling RD-133, Addition 1 item 7; RD-122).
#[test]
fn rd133_after_a_tick_that_did_not_return_the_held_ticks_leave_the_defer_keys_null() {
    let limits = Limits {
        max_live_runs: 2,
        ..Limits::PRODUCT
    };
    let c = coordinator(&[MANUAL, MANUAL], limits);
    let run0 = start(&c, 0);
    c.on_run(&run0, ask_confirm(&run0, 0)).unwrap();
    let (item, _) = open_item(&c);
    assert_eq!(c.shown_on_page(&item, 0), Ok(()));
    c.ports().ports.set_clock(T0 + Limits::PRODUCT.arm_ms, DAY);
    c.tick();
    let run1 = start(&c, 1);
    tick_that_panics(&c);
    panicking(&c, "answer_confirm_after_send", || {
        c.answer(&item, Choice::Allow, None, 0)
    });
    clock_fails(&c);
    c.tick();
    c.tick();
    clock_answers(&c);
    c.tick();
    for run in [&run0, &run1] {
        c.ports().ports.end_runner(run);
        let _ = c.on_run(run, RunEvent::Ended(None));
    }
    for run in [&run0, &run1] {
        let defers = events(&c).iter().find_map(|w| match w {
            What::RunEnded { run: r, defers, .. } if r == run => Some(*defers),
            _ => None,
        });
        assert_eq!(defers, Some(None), "null wins over the held ticks");
    }
}

/// An `Ended` that no read of the clock let through keeps the person's cause: it is made again
/// at the next read that answers, before that move's work, so the kill of a later tick, which
/// the library refuses with `unknown_run`, never rewrites it to `interrupted` (ruling RD-133,
/// Addition 1 item 8). Once with the Stop itself under a failing clock, once with the Stop
/// stamped and only the end under a failing clock.
#[test]
fn rd133_an_end_refused_in_the_window_keeps_the_persons_cause() {
    for stop_unstamped in [true, false] {
        let (c, _run0, _item, runs) = lost_answer_beside_other_runs(1);
        let run1 = runs[0].clone();
        if stop_unstamped {
            clock_fails(&c);
            assert_eq!(c.control(&agent(1), Action::Stop), Ok(()));
        } else {
            assert_eq!(c.control(&agent(1), Action::Stop), Ok(()));
            clock_fails(&c);
        }
        c.ports().ports.end_runner(&run1);
        let _ = c.on_run(&run1, RunEvent::Ended(None));
        c.tick();
        clock_answers(&c);
        after_the_grace(&c);
        after_the_grace(&c);
        assert_eq!(
            ends_of(&c, &run1),
            vec![RunEnd::Cancelled],
            "stop_unstamped {stop_unstamped}: the person's cause, once"
        );
    }
}

/// A Switch off under a failing clock, then a new life: the `hold_set` was stored at once at
/// the last time that this life read the clock, never later than the press, so the agent is
/// still off, takes no start and its row reads switched off (ruling RD-133, answer (b)).
#[test]
fn rd133_a_switch_off_under_a_failing_clock_holds_the_agent_off_across_a_new_life() {
    let c = coordinator(&[MANUAL, MANUAL], Limits::PRODUCT);
    let run1 = start(&c, 1);
    let last = T0;
    let press = T0 + 7_000;
    c.ports().ports.set_clock(press, DAY);
    clock_fails(&c);
    let m = mark(&c);
    assert_eq!(c.control(&agent(1), Action::SwitchOff), Ok(()));
    assert_eq!(cancels_of(&c, m, &run1), 1, "the cancel in the move");
    let holds = person_holds(&c, 1);
    assert_eq!(holds.len(), 1, "stored at once");
    assert_eq!(holds[0].at, at_of(last), "at the last read");
    assert!(holds[0].at <= at_of(press), "never later than the press");
    let life = next_life(&c, |_| {});
    assert_eq!(row_line(&life.snapshot(), 1), text("line.off.person"));
    assert_eq!(life.control(&agent(1), Action::RunNow), Err(Cause::Hold));
    let before = started(&life).len();
    life.tick();
    assert_eq!(started(&life).len(), before, "no tick's start");
}

/// With no read of the clock in this life after the board was made, a Switch off under a
/// failing clock has nothing to stamp its `hold_set` with: it waits in the state, as in
/// reading (a), and a new life before the next read does not hold the agent off (a named
/// limit of ruling RD-133, answer (b)).
#[test]
fn rd133_with_no_read_in_this_life_a_switch_off_under_a_failing_clock_waits_as_in_a() {
    let c = Coordinator::new(owner(&[MANUAL, MANUAL], Limits::PRODUCT), Limits::PRODUCT);
    c.ports().fail_reads_after.set(Some(1));
    c.tick();
    assert_eq!(c.snapshot().rows.len(), 2, "recovery's read made the board");
    clock_fails(&c);
    let out = c.control(&agent(1), Action::SwitchOff);
    clock_answers(&c);
    assert_eq!(out, Ok(()), "the Switch off goes on");
    assert_eq!(person_holds(&c, 1).len(), 0, "no read to stamp it with");
    let life = next_life(&c, |_| {});
    assert_ne!(
        row_line(&life.snapshot(), 1),
        text("line.off.person"),
        "a new life before the next read loses it"
    );
    c.ports().ports.set_clock(T0 + 4_000, DAY);
    c.tick();
    let holds = person_holds(&c, 1);
    assert_eq!(holds.len(), 1, "the next read that answers stores it");
    assert_eq!(holds[0].at, at_of(T0 + 4_000), "at that read");
    assert_eq!(row_line(&c.snapshot(), 1), text("line.off.person"));
}

const DUE: support::A = support::A {
    person: false,
    slotted: true,
};

/// Two days on: the next slot of a `DUE` agent, which a new life's first tick starts.
const NEXT_SLOT: (u64, u32) = (T0 + 2 * 86_400_000, DAY + 2);

/// A Switch off of a slotted agent under a failing clock, then a new life at its next slot
/// (ruling RD-133, Addition 2 A): the `hold_set` was stored at once at the last read, so the
/// agent is still off, the new life starts no run of it, its first tick included, and its row
/// reads switched off.
#[test]
fn rd133_a_switch_off_under_a_failing_clock_holds_a_slotted_agent_off_across_a_new_life() {
    let c = coordinator(&[MANUAL, DUE], Limits::PRODUCT);
    let run1 = started(&c)[0].clone();
    let press = T0 + 7_000;
    c.ports().ports.set_clock(press, DAY);
    clock_fails(&c);
    let m = mark(&c);
    assert_eq!(c.control(&agent(1), Action::SwitchOff), Ok(()));
    assert_eq!(cancels_of(&c, m, &run1), 1, "the cancel in the move");
    let holds = person_holds(&c, 1);
    assert_eq!(holds.len(), 1, "stored at once");
    assert_eq!(holds[0].at, at_of(T0), "at the last read");
    assert!(holds[0].at <= at_of(press), "never later than the press");
    let life = next_life(&c, |b| b.ports.set_clock(NEXT_SLOT.0, NEXT_SLOT.1));
    assert_eq!(started(&life), vec![], "its first tick starts no run of it");
    life.ports()
        .ports
        .set_clock(NEXT_SLOT.0 + 1_000, NEXT_SLOT.1);
    life.tick();
    assert_eq!(started(&life), vec![], "nor a later tick");
    assert_eq!(row_line(&life.snapshot(), 1), text("line.off.person"));
}

/// The limits of the stored `hold_set` (ruling RD-133, Addition 2 A): where the day of the last
/// read cannot be read, or the owner does not acknowledge the `hold_set`, the hold is kept only in
/// this life's memory; after a new life the agent is on and its due slot starts it.
#[test]
fn rd133_a_hold_with_no_day_or_no_acknowledgement_is_lost_at_a_new_life() {
    for case in ["day", "owner"] {
        let c = coordinator(&[MANUAL, DUE], Limits::PRODUCT);
        c.ports().ports.set_clock(T0 + 7_000, DAY);
        clock_fails(&c);
        if case == "day" {
            c.ports().day_fails.set(true);
        } else {
            c.ports().ports.refuse(Method::Log, PortError::NoService);
        }
        assert_eq!(c.control(&agent(1), Action::SwitchOff), Ok(()), "{case}");
        assert_eq!(person_holds(&c, 1).len(), 0, "{case}: no hold stored");
        assert_eq!(
            c.control(&agent(1), Action::RunNow),
            Err(Cause::Hold),
            "{case}: held off in this life"
        );
        let life = next_life(&c, |b| b.ports.set_clock(NEXT_SLOT.0, NEXT_SLOT.1));
        assert_eq!(started(&life).len(), 1, "{case}: the due slot starts it");
        assert_ne!(
            row_line(&life.snapshot(), 1),
            text("line.off.person"),
            "{case}"
        );
        c.ports().ports.clear_refusals();
        clock_answers(&c);
    }
}

/// A Switch off after a tick, or after a start whose only read is the drive's stamp, stores its
/// `hold_set` at that move's read, not at an older one, and never later than the press (ruling
/// RD-133, answer (b); the coordinator's word on `Report::read`).
#[test]
fn rd133_a_switch_off_after_a_tick_stores_its_hold_at_that_ticks_read() {
    for last in ["tick", "start"] {
        let c = coordinator(&[MANUAL, MANUAL], Limits::PRODUCT);
        let read_at = T0 + 3_000;
        c.ports().ports.set_clock(read_at, DAY);
        if last == "tick" {
            c.tick();
        } else {
            let _ = start(&c, 0);
        }
        let press = T0 + 9_000;
        c.ports().ports.set_clock(press, DAY);
        clock_fails(&c);
        assert_eq!(c.control(&agent(1), Action::SwitchOff), Ok(()), "{last}");
        let holds = person_holds(&c, 1);
        assert_eq!(holds.len(), 1, "{last}: stored at once");
        assert_eq!(holds[0].at, at_of(read_at), "{last}: at that move's read");
        assert!(
            holds[0].at <= at_of(press),
            "{last}: never later than the press"
        );
        clock_answers(&c);
    }
}

/// Whether the tick since `m` did its work: a held tick reads no list of agents.
fn tick_worked(c: &Coordinator<Both>, m: usize) -> bool {
    calls_since(c, m)
        .iter()
        .any(|call| matches!(call, Call::Agents))
}

/// A Switch off in the window of an agent that recovery set aside is refused as a stamped one
/// is, and holds no tick once the clock answers (the review of round 2, finding 1).
#[test]
fn rd133_a_clockless_switch_off_of_an_agent_set_aside_is_refused_and_holds_no_tick() {
    let (c, made, _) = setup_agent();
    let life = next_life(&c, |o| {
        o.ports.refuse_agent(
            Method::History,
            &made,
            PortError::Refused(halvo_coord::types::Name::parse("store").unwrap()),
        )
    });
    clock_fails(&life);
    assert_eq!(
        life.control(&made, Action::SwitchOff),
        Err(Cause::NoService),
        "refused in the window as a stamped Switch off of an agent set aside is"
    );
    clock_answers(&life);
    life.ports().ports.clear_refusals();
    life.ports().ports.set_clock(T0 + 1_000, DAY);
    let m = mark(&life);
    life.tick();
    assert!(tick_worked(&life, m), "the tick does its work");
}

/// A Switch off in the window of an agent whose hold `off` already stands, with no run to stop,
/// is refused `state` as a stamped one is and stores no second `hold_set` (finding 2).
#[test]
fn rd133_a_clockless_switch_off_of_an_agent_already_off_stores_no_second_hold() {
    let c = coordinator(&[MANUAL, MANUAL], Limits::PRODUCT);
    assert_eq!(c.control(&agent(1), Action::SwitchOff), Ok(()));
    assert_eq!(person_holds(&c, 1).len(), 1);
    clock_fails(&c);
    assert_eq!(c.control(&agent(1), Action::SwitchOff), Err(Cause::State));
    clock_answers(&c);
    c.ports().ports.set_clock(T0 + 1_000, DAY);
    let m = mark(&c);
    c.tick();
    assert!(tick_worked(&c, m), "no tick held");
    assert_eq!(person_holds(&c, 1).len(), 1, "one hold_set");
}

/// A `hold_set` that the owner did not acknowledge at a Switch off in the window is sent again
/// and stored once when the clock answers, not stored a second time (finding 3).
#[test]
fn rd133_an_unacknowledged_hold_is_stored_once_when_the_clock_answers() {
    let c = coordinator(&[MANUAL, MANUAL], Limits::PRODUCT);
    let run1 = start(&c, 1);
    clock_fails(&c);
    c.ports().ports.refuse(Method::Log, PortError::Busy);
    let m = mark(&c);
    assert_eq!(c.control(&agent(1), Action::SwitchOff), Ok(()));
    assert_eq!(cancels_of(&c, m, &run1), 1, "the cancel in the move");
    assert_eq!(person_holds(&c, 1).len(), 0, "not acknowledged");
    c.ports().ports.clear_refusals();
    clock_answers(&c);
    c.ports().ports.set_clock(T0 + 1_000, DAY);
    c.tick();
    c.tick();
    assert_eq!(person_holds(&c, 1).len(), 1, "stored once");
    assert_eq!(row_line(&c.snapshot(), 1), text("line.off.person"));
    assert_eq!(c.control(&agent(1), Action::RunNow), Err(Cause::Hold));
}

/// The quit's first step under a failing clock stops each live run with no time; the time is
/// taken at the next read that answers, so the quit's tick kills no run before the grace from
/// that read (finding 4).
#[test]
fn rd133_the_quits_stop_all_under_a_failing_clock_keeps_the_grace() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    clock_fails(&c);
    let m = mark(&c);
    let _ = c.quit_stop();
    assert_eq!(cancels_of(&c, m, &run), 1, "the quit's cancel");
    clock_answers(&c);
    let read = T0 + 60_000;
    c.ports().ports.set_clock(read, DAY);
    c.tick();
    assert_eq!(
        kills_of(&c, m, &run),
        0,
        "no kill before the grace from the stamp"
    );
    c.ports()
        .ports
        .set_clock(read + Limits::PRODUCT.cancel_grace_s * 1000, DAY);
    c.tick();
    assert_eq!(kills_of(&c, m, &run), 1, "killed once the grace passed");
}

/// A run's end that the window kept and that a later move makes again while the mark of a lost
/// tick stands takes no queued start: only a tick that returns starts it (finding 5).
#[test]
fn rd133_a_replayed_end_under_the_mark_takes_no_queued_start() {
    let one = Limits {
        max_live_runs: 1,
        ..Limits::PRODUCT
    };
    let c = coordinator(&[MANUAL, MANUAL], one);
    let first = start(&c, 0);
    assert_eq!(c.control(&agent(1), Action::RunNow), Ok(()));
    tick_that_panics(&c);
    clock_fails(&c);
    c.ports().ports.end_runner(&first);
    let _ = c.on_run(&first, RunEvent::Ended(Some(Outcome::Completed)));
    c.tick();
    clock_answers(&c);
    let m = mark(&c);
    let _ = c.control(&agent(0), Action::Stop);
    assert_eq!(
        ends_of(&c, &first),
        vec![RunEnd::Completed],
        "the end is made"
    );
    assert!(
        !calls_since(&c, m)
            .iter()
            .any(|call| matches!(call, Call::Start(_))),
        "no start while the mark stands"
    );
    assert_eq!(c.queued(), vec![(agent(1), false)], "the queue keeps it");
    c.tick();
    assert_eq!(started(&c).len(), 2, "the tick that returned starts it");
}

/// The guard's table holds the stop in the window: a record that the guard takes while the run
/// works is refused once the Stop went on with no clock (the review's test finding 1).
#[test]
fn rd133_in_the_window_the_guard_refuses_a_record_that_it_took_before_the_stop() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let (run, item) = allowed(&c);
    assert_eq!(
        c.answer(
            &item,
            Choice::Allow,
            None,
            shown_step_mark_or_zero(&c, &item)
        ),
        Ok(())
    );
    let records = allowed_records(&c, &run, &item);
    for record in &records {
        assert_eq!(
            c.check_record(record),
            Ok(()),
            "{:?} while the run works",
            record.state
        );
    }
    clock_fails(&c);
    let m = mark(&c);
    assert_eq!(c.control(&agent(0), Action::Stop), Ok(()));
    assert_eq!(cancels_of(&c, m, &run), 1, "the cancel in the move");
    for record in &records {
        assert_eq!(
            c.check_record(record),
            Err(halvo_coord::guard::Refusal::Approval),
            "{:?} refused in the window",
            record.state
        );
    }
    clock_answers(&c);
}

/// The grace counts from the read that stamps the stop, not from the last read before the
/// press: the clock moves between the two (the review's test finding 3).
#[test]
fn rd133_the_grace_counts_from_the_stamping_read_not_the_last_read_before_the_press() {
    let (c, _run0, _item, runs) = lost_answer_beside_other_runs(1);
    let run1 = runs[0].clone();
    let before = T0 + Limits::PRODUCT.arm_ms;
    let grace = Limits::PRODUCT.cancel_grace_s * 1000;
    clock_fails(&c);
    let m = mark(&c);
    assert_eq!(c.control(&agent(1), Action::Stop), Ok(()));
    clock_answers(&c);
    let stamp = before + 5_000;
    c.ports().ports.set_clock(stamp, DAY);
    c.tick();
    c.ports().ports.set_clock(before + grace + 1_000, DAY);
    c.tick();
    assert_eq!(
        kills_of(&c, m, &run1),
        0,
        "the grace from the last read before the press has passed, the grace from the stamp has not"
    );
    c.ports().ports.set_clock(stamp + grace, DAY);
    c.tick();
    assert_eq!(
        kills_of(&c, m, &run1),
        1,
        "killed at the grace from the stamp"
    );
}

/// A second Switch off in the window, while the first one's `hold_set` waits for the owner, is
/// refused `state` and queues no second `hold_set` (the second review, finding 1).
#[test]
fn rd133_a_second_clockless_switch_off_with_the_first_hold_unacknowledged_stores_one_hold() {
    let c = coordinator(&[MANUAL, MANUAL], Limits::PRODUCT);
    let _run1 = start(&c, 1);
    clock_fails(&c);
    c.ports().ports.refuse(Method::Log, PortError::Busy);
    assert_eq!(c.control(&agent(1), Action::SwitchOff), Ok(()));
    assert_eq!(person_holds(&c, 1).len(), 0, "not acknowledged");
    assert_eq!(c.control(&agent(1), Action::SwitchOff), Err(Cause::State));
    c.ports().ports.clear_refusals();
    clock_answers(&c);
    c.ports().ports.set_clock(T0 + 1_000, DAY);
    c.tick();
    c.tick();
    assert_eq!(person_holds(&c, 1).len(), 1, "one hold_set for one hold");
    assert_eq!(row_line(&c.snapshot(), 1), text("line.off.person"));
}

/// After the clock steps back, a Switch off in the window stores its `hold_set` at the last read
/// of the life, not at a higher earlier one, and never later than the press (finding 2).
#[test]
fn rd133_a_switch_off_after_the_clock_steps_back_stores_its_hold_at_the_last_read() {
    let c = coordinator(&[MANUAL, MANUAL], Limits::PRODUCT);
    let high = T0 + 1_000_000;
    c.ports().ports.set_clock(high, DAY);
    c.tick();
    let low = T0 + 400_000;
    c.ports().ports.set_clock(low, DAY);
    c.tick();
    let press = low + 1_000;
    c.ports().ports.set_clock(press, DAY);
    clock_fails(&c);
    assert_eq!(c.control(&agent(1), Action::SwitchOff), Ok(()));
    let holds = person_holds(&c, 1);
    assert_eq!(holds.len(), 1);
    assert_eq!(holds[0].at, at_of(low), "at the last read");
    assert!(holds[0].at <= at_of(press), "never later than the press");
    clock_answers(&c);
}

/// A `hold_set` that the owner refuses with a name that lasts the life does not hold the tick
/// for the rest of the life once the clock answers (finding 3).
#[test]
fn rd133_a_hold_refused_for_the_life_holds_no_later_tick() {
    let c = coordinator(&[MANUAL, MANUAL], Limits::PRODUCT);
    clock_fails(&c);
    c.ports().ports.refuse(
        Method::Log,
        PortError::Refused(halvo_coord::types::Name::parse("role").unwrap()),
    );
    assert_eq!(c.control(&agent(1), Action::SwitchOff), Ok(()));
    clock_answers(&c);
    c.ports().ports.set_clock(T0 + 1_000, DAY);
    c.tick();
    c.ports().ports.set_clock(T0 + 2_000, DAY);
    let m = mark(&c);
    c.tick();
    assert!(tick_worked(&c, m), "the tick does its work");
    c.ports().ports.clear_refusals();
}

/// The quit's first step under a failing clock counts the grace of a stop with no time from the
/// last read of the life, not from time 0 (finding 4).
#[test]
fn rd133_the_quits_wait_under_a_failing_clock_counts_from_the_last_read() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let _run = start(&c, 0);
    clock_fails(&c);
    let wait = c.quit_stop();
    assert_eq!(
        wait.until,
        T0 + Limits::PRODUCT.cancel_grace_s * 1000,
        "the grace from the last read"
    );
    clock_answers(&c);
}

/// A Stop or Switch off after the window kept the runner's end neither stops the ended run nor
/// replaces its outcome: the end is made with the runner's outcome (finding 5).
#[test]
fn rd133_a_stop_after_a_kept_end_keeps_the_runners_outcome() {
    for action in [Action::Stop, Action::SwitchOff] {
        let c = coordinator(&[MANUAL, MANUAL], Limits::PRODUCT);
        let run = start(&c, 1);
        clock_fails(&c);
        c.ports().ports.end_runner(&run);
        let _ = c.on_run(&run, RunEvent::Ended(Some(Outcome::Failed)));
        let m = mark(&c);
        let _ = c.control(&agent(1), action);
        assert_eq!(
            cancels_of(&c, m, &run),
            0,
            "{action:?}: no cancel to an ended run"
        );
        clock_answers(&c);
        c.ports().ports.set_clock(T0 + 1_000, DAY);
        c.tick();
        assert_eq!(
            ends_of(&c, &run),
            vec![RunEnd::Failed],
            "{action:?}: the runner's outcome"
        );
    }
}

/// A `hold_set` that the owner acknowledged at a Switch off in the window, whose read-back
/// failed, is not stored a second time when the clock answers (finding 6).
#[test]
fn rd133_an_acknowledged_hold_whose_read_back_failed_is_stored_once() {
    let c = coordinator(&[MANUAL, MANUAL], Limits::PRODUCT);
    clock_fails(&c);
    c.ports().next_history_fails.set(true);
    assert_eq!(c.control(&agent(1), Action::SwitchOff), Ok(()));
    assert_eq!(person_holds(&c, 1).len(), 1, "acknowledged");
    let _ = c.control(&agent(0), Action::SwitchOn);
    clock_answers(&c);
    c.ports().ports.set_clock(T0 + 1_000, DAY);
    c.tick();
    c.tick();
    assert_eq!(person_holds(&c, 1).len(), 1, "stored once");
    assert_eq!(row_line(&c.snapshot(), 1), text("line.off.person"));
}

/// The quit's stop-all after the window kept a runner's end neither cancels the ended run nor
/// replaces its outcome (the third review, finding 1).
#[test]
fn rd133_the_quits_stop_all_after_a_kept_end_keeps_the_runners_outcome() {
    let c = coordinator(&[MANUAL, MANUAL], Limits::PRODUCT);
    let run = start(&c, 1);
    clock_fails(&c);
    c.ports().ports.end_runner(&run);
    let _ = c.on_run(&run, RunEvent::Ended(Some(Outcome::Failed)));
    let m = mark(&c);
    let _ = c.quit_stop();
    assert_eq!(cancels_of(&c, m, &run), 0, "no cancel to an ended run");
    clock_answers(&c);
    c.ports().ports.set_clock(T0 + 1_000, DAY);
    c.quit_kill();
    assert_eq!(
        ends_of(&c, &run),
        vec![RunEnd::Failed],
        "the runner's outcome"
    );
}

/// The rest of a switch-off at a read that answers, while the kept end of the agent's run met a
/// failing read again, does not stop the ended run (the third review, finding 2).
#[test]
fn rd133_the_rest_of_a_switch_off_does_not_stop_a_run_whose_end_is_kept_again() {
    let c = coordinator(&[MANUAL, MANUAL], Limits::PRODUCT);
    let run = start(&c, 1);
    clock_fails(&c);
    c.ports().ports.end_runner(&run);
    let _ = c.on_run(&run, RunEvent::Ended(Some(Outcome::Failed)));
    let m = mark(&c);
    assert_eq!(c.control(&agent(1), Action::SwitchOff), Ok(()));
    clock_answers(&c);
    c.ports().fail_read_at.set(Some(1));
    c.ports().ports.set_clock(T0 + 1_000, DAY);
    c.tick();
    clock_answers(&c);
    c.tick();
    assert_eq!(cancels_of(&c, m, &run), 0, "no cancel to an ended run");
    assert_eq!(
        ends_of(&c, &run),
        vec![RunEnd::Failed],
        "the runner's outcome"
    );
    assert_eq!(person_holds(&c, 1).len(), 1, "one hold_set");
}

/// A runner's end made in a move whose first read failed and whose drive's read answered
/// leaves a dying tree whose stop has no time; the next read stamps it, so its kill waits for
/// the grace from that read (the third review, finding 4).
#[test]
fn rd133_a_dying_tree_of_a_stop_with_no_time_is_stamped_before_its_kill() {
    let c = coordinator(&[MANUAL, MANUAL], Limits::PRODUCT);
    let run = start(&c, 1);
    clock_fails(&c);
    let m = mark(&c);
    assert_eq!(c.control(&agent(1), Action::Stop), Ok(()));
    clock_answers(&c);
    c.ports().fail_read_at.set(Some(0));
    c.ports().ports.end_runner(&run);
    let _ = c.on_run(&run, RunEvent::Ended(None));
    clock_answers(&c);
    assert_eq!(
        ends_of(&c, &run),
        vec![RunEnd::Cancelled],
        "the person's cause"
    );
    c.ports().ports.set_clock(T0 + 1_000, DAY);
    c.tick();
    assert_eq!(kills_of(&c, m, &run), 0, "no kill before the grace");
}

/// A second press of Switch off while the first, made in the window, is still in hand is
/// refused `state` and stores no second `hold_set`, also once the clock answers (the third
/// review, finding 3).
#[test]
fn rd133_a_stamped_second_switch_off_while_the_first_is_in_hand_stores_one_hold() {
    let c = coordinator(&[MANUAL, MANUAL], Limits::PRODUCT);
    let _run1 = start(&c, 1);
    clock_fails(&c);
    c.ports().ports.refuse(Method::Log, PortError::Busy);
    assert_eq!(c.control(&agent(1), Action::SwitchOff), Ok(()));
    clock_answers(&c);
    assert_eq!(c.control(&agent(1), Action::SwitchOff), Err(Cause::State));
    c.ports().ports.clear_refusals();
    c.ports().ports.set_clock(T0 + 1_000, DAY);
    c.tick();
    c.tick();
    assert_eq!(person_holds(&c, 1).len(), 1, "one hold_set");
    assert_eq!(row_line(&c.snapshot(), 1), text("line.off.person"));
}

/// A runner's end that the window kept, with no stop before it, is made at the next read as
/// the same end with a clock that answers is: the runner's outcome, and no kill then or at the
/// quit (the fourth review, finding 1).
#[test]
fn rd133_a_kept_natural_end_sends_no_kill_after_the_clock_answers() {
    let limits = Limits::PRODUCT;
    let c = coordinator(&[MANUAL, MANUAL], limits);
    let run = start(&c, 1);
    clock_fails(&c);
    c.ports().ports.end_runner(&run);
    let _ = c.on_run(&run, RunEvent::Ended(Some(Outcome::Failed)));
    let m = mark(&c);
    clock_answers(&c);
    c.ports().ports.set_clock(T0 + 1_000, DAY);
    c.tick();
    assert_eq!(
        ends_of(&c, &run),
        vec![RunEnd::Failed],
        "the runner's outcome"
    );
    c.ports()
        .ports
        .set_clock(T0 + 1_000 + limits.cancel_grace_s * 1000, DAY);
    c.tick();
    c.tick();
    let _ = c.quit_stop();
    c.quit_kill();
    assert_eq!(
        kills_of(&c, m, &run),
        0,
        "no kill to a run that ended on its own"
    );
}

/// A switch-off that waited for the clock does not hold every later tick when the owner
/// refuses the read of the agent's events with a name that lasts the life (the fourth review,
/// finding 2).
#[test]
fn rd133_a_waiting_switch_off_whose_read_is_refused_for_the_life_holds_no_later_tick() {
    let c = coordinator(&[MANUAL, MANUAL], Limits::PRODUCT);
    clock_fails(&c);
    assert_eq!(c.control(&agent(1), Action::SwitchOff), Ok(()));
    c.ports().ports.refuse_agent(
        Method::History,
        &agent(1),
        PortError::Refused(halvo_coord::types::Name::parse("no_workflow").unwrap()),
    );
    clock_answers(&c);
    c.ports().ports.set_clock(T0 + 1_000, DAY);
    c.tick();
    c.ports().ports.set_clock(T0 + 2_000, DAY);
    let m = mark(&c);
    c.tick();
    assert!(tick_worked(&c, m), "the tick does its work");
    c.ports().ports.clear_refusals();
}

/// A runner's end that the window kept and that comes again, in a move whose first read fails
/// and whose run drive's read answers, is made with the runner's outcome, no dying tree and no
/// kill (the fifth review, finding 1).
#[test]
fn rd133_a_kept_natural_end_sent_again_when_only_the_first_read_fails_sends_no_kill() {
    let limits = Limits::PRODUCT;
    let c = coordinator(&[MANUAL, MANUAL], limits);
    let run = start(&c, 1);
    clock_fails(&c);
    c.ports().ports.end_runner(&run);
    assert_eq!(
        c.on_run(&run, RunEvent::Ended(Some(Outcome::Failed))),
        Err(Cause::NoService)
    );
    let m = mark(&c);
    clock_answers(&c);
    c.ports().ports.set_clock(T0 + 1_000, DAY);
    c.ports().fail_read_at.set(Some(0));
    let _ = c.on_run(&run, RunEvent::Ended(Some(Outcome::Failed)));
    clock_answers(&c);
    c.ports().ports.set_clock(T0 + 2_000, DAY);
    c.tick();
    assert_eq!(
        ends_of(&c, &run),
        vec![RunEnd::Failed],
        "the runner's outcome"
    );
    c.ports()
        .ports
        .set_clock(T0 + 2_000 + limits.cancel_grace_s * 1000, DAY);
    c.tick();
    c.tick();
    let _ = c.quit_stop();
    c.quit_kill();
    assert_eq!(
        kills_of(&c, m, &run),
        0,
        "no kill to a run that ended on its own"
    );
}

/// An answer to another agent's confirmation in the window does not take the place of the
/// waiting settle: the first item still settles at the next read that answers (the fifth
/// review, finding 2).
#[test]
fn rd133_an_answer_to_another_item_in_the_window_keeps_the_waiting_settle() {
    let limits = Limits {
        max_live_runs: 2,
        ..Limits::PRODUCT
    };
    let c = coordinator(&[MANUAL, MANUAL], limits);
    let run0 = start(&c, 0);
    c.on_run(&run0, ask_confirm(&run0, 0)).unwrap();
    let item0 = item_of_agent(&c, 0);
    let run1 = start(&c, 1);
    c.on_run(&run1, ask_confirm(&run1, 1)).unwrap();
    let item1 = item_of_agent(&c, 1);
    assert_eq!(c.shown_on_page(&item0, 0), Ok(()));
    assert_eq!(c.shown_on_page(&item1, 0), Ok(()));
    c.ports().ports.set_clock(T0 + Limits::PRODUCT.arm_ms, DAY);
    c.tick();
    let m = mark(&c);
    panicking(&c, "answer_confirm_after_send", || {
        c.answer(&item0, Choice::Allow, None, 0)
    });
    clock_fails(&c);
    let _ = c.answer(&item1, Choice::Allow, None, 0);
    clock_answers(&c);
    c.ports()
        .ports
        .set_clock(T0 + Limits::PRODUCT.arm_ms + 1_000, DAY);
    c.tick();
    assert_eq!(lost_lapses(&c, &item0), 1, "the first item settles");
    assert_eq!(cancels_of(&c, m, &run0), 1, "its run's one cancel");
}

/// A `hold_set` that the owner refuses alone with a name that does not last the life holds no
/// later tick; the agent stays held off, and once the owner takes the hold its row reads
/// switched off (the sixth review, finding 1).
#[test]
fn rd133_a_hold_refused_malformed_alone_holds_no_later_tick_and_keeps_the_agent_off() {
    let c = coordinator(&[MANUAL, MANUAL], Limits::PRODUCT);
    clock_fails(&c);
    c.ports().ports.refuse_agent(
        Method::Log,
        &agent(1),
        PortError::Refused(halvo_coord::types::Name::parse("malformed").unwrap()),
    );
    assert_eq!(c.control(&agent(1), Action::SwitchOff), Ok(()));
    clock_answers(&c);
    c.ports().ports.set_clock(T0 + 1_000, DAY);
    c.tick();
    c.ports().ports.set_clock(T0 + 2_000, DAY);
    let m = mark(&c);
    c.tick();
    assert!(tick_worked(&c, m), "the tick does its work");
    assert_eq!(
        c.control(&agent(1), Action::RunNow),
        Err(Cause::Hold),
        "held off"
    );
    c.ports().ports.clear_refusals();
    c.ports().ports.set_clock(T0 + 3_000, DAY);
    c.tick();
    c.tick();
    assert_eq!(person_holds(&c, 1).len(), 1, "the hold is taken once");
    assert_eq!(row_line(&c.snapshot(), 1), text("line.off.person"));
}

/// Where the owner refuses the read of a switched-off agent's events for the life, the agent
/// is held off on the board, so its next slot does not start it (the sixth review, finding 2).
#[test]
fn rd133_a_switch_off_whose_read_is_refused_for_the_life_keeps_the_agent_from_its_slot() {
    let c = coordinator(&[MANUAL, DUE], Limits::PRODUCT);
    let run1 = started(&c)[0].clone();
    clock_fails(&c);
    assert_eq!(c.control(&agent(1), Action::SwitchOff), Ok(()));
    c.ports().ports.refuse_agent(
        Method::History,
        &agent(1),
        PortError::Refused(halvo_coord::types::Name::parse("unsupported").unwrap()),
    );
    clock_answers(&c);
    c.ports().ports.end_runner(&run1);
    let _ = c.on_run(&run1, RunEvent::Ended(None));
    c.ports().ports.set_clock(NEXT_SLOT.0, NEXT_SLOT.1);
    c.tick();
    c.ports().ports.set_clock(NEXT_SLOT.0 + 1_000, NEXT_SLOT.1);
    c.tick();
    assert_eq!(started(&c).len(), 1, "its next slot does not start it");
    assert_eq!(c.control(&agent(1), Action::RunNow), Err(Cause::Hold));
    c.ports().ports.clear_refusals();
}

// ---- RD-138 (round 3): the adversaries' cases of interrogate-u24m, each renamed with its item ----

/// A person's Switch off of a slotted agent with a working run, whose `Runs::cancel` does not
/// return, with the clock answering and with every read failing. The intent: the hold is stored
/// at once (with the last read's time where the clock fails), and the agent stays off.
#[test]
fn rd138_m1_adv1_own_1_a_switch_off_whose_cancel_does_not_return_keeps_the_agent_off() {
    let mut got = Vec::new();
    for case in ["clock", "no clock"] {
        let c = coordinator(&[MANUAL, DUE], Limits::PRODUCT);
        let run1 = started(&c)[0].clone();
        c.ports().ports.set_clock(T0 + 7_000, DAY);
        if case == "no clock" {
            clock_fails(&c);
        }
        panicking(&c, "cancel", || c.control(&agent(1), Action::SwitchOff));
        clock_answers(&c);
        let holds = person_holds(&c, 1).len();
        c.ports().ports.set_clock(T0 + 8_000, DAY);
        c.tick();
        c.ports().ports.end_runner(&run1);
        let _ = c.on_run(&run1, RunEvent::Ended(None));
        let before = started(&c).len();
        c.ports().ports.set_clock(NEXT_SLOT.0, NEXT_SLOT.1);
        c.tick();
        let starts_after = started(&c).len() - before;
        let off_row = row_line(&c.snapshot(), 1) == text("line.off.person");
        eprintln!("{case}: hold_set rows {holds}, starts at the next slot {starts_after}, row reads switched off {off_row}");
        got.push((case, holds, starts_after, off_row));
    }
    assert_eq!(
        got,
        vec![("clock", 1, 0, true), ("no clock", 1, 0, true)],
        "(case, hold_set rows stored, starts of agent 1 at its next slot, row reads switched off)"
    );
}

/// The same with an open confirmation: the cancel goes out in the lapse of the item.
#[test]
fn rd138_m1_adv1_own_1b_a_waiting_runs_switch_off_whose_cancel_does_not_return_keeps_the_agent_off()
{
    let mut got = Vec::new();
    for case in ["clock", "no clock"] {
        let c = coordinator(&[MANUAL, MANUAL], Limits::PRODUCT);
        let run1 = start(&c, 1);
        c.on_run(&run1, ask_confirm(&run1, 1)).unwrap();
        c.ports().ports.set_clock(T0 + 7_000, DAY);
        if case == "no clock" {
            clock_fails(&c);
        }
        panicking(&c, "cancel", || c.control(&agent(1), Action::SwitchOff));
        clock_answers(&c);
        let holds = person_holds(&c, 1).len();
        c.ports().ports.set_clock(T0 + 8_000, DAY);
        c.tick();
        c.ports().ports.end_runner(&run1);
        let _ = c.on_run(&run1, RunEvent::Ended(None));
        c.tick();
        let run_now = c.control(&agent(1), Action::RunNow);
        eprintln!(
            "{case}: hold_set rows {holds}, Run now after the Switch off answers {run_now:?}"
        );
        got.push((case, holds, run_now));
    }
    assert_eq!(
        got,
        vec![
            ("clock", 1, Err(Cause::Hold)),
            ("no clock", 1, Err(Cause::Hold))
        ],
        "(case, hold_set rows stored, Run now of agent 1 after its Switch off)"
    );
}

/// A person's Stop of a working run whose `Runs::cancel` does not return: the stop's time is
/// taken at the next read that answers, so the kill waits `cancel_grace_s` from it.
#[test]
fn rd138_m1_adv1_own_2_a_stop_whose_cancel_does_not_return_keeps_its_grace() {
    let mut got = Vec::new();
    for case in ["clock", "no clock"] {
        let c = coordinator(&[MANUAL, MANUAL], Limits::PRODUCT);
        let run1 = start(&c, 1);
        let press = T0 + 7_000;
        c.ports().ports.set_clock(press, DAY);
        if case == "no clock" {
            clock_fails(&c);
        }
        let m = mark(&c);
        panicking(&c, "cancel", || c.control(&agent(1), Action::Stop));
        clock_answers(&c);
        c.ports().ports.set_clock(press + 1_000, DAY);
        c.tick();
        let kills = kills_of(&c, m, &run1);
        eprintln!(
            "{case}: kills one second after the press {kills} (grace {} s)",
            Limits::PRODUCT.cancel_grace_s
        );
        got.push((case, kills));
    }
    assert_eq!(
        got,
        vec![("clock", 0), ("no clock", 0)],
        "(case, kills of the stopped run one second after the press)"
    );
}

/// Probe 1 across a new life: the Switch off of a slotted agent whose cancel does not return,
/// then a clock that answers at a tick, then a new life at the agent's next slot. The intent:
/// the agent stays off across a restart (its hold was stored at once), so the new life starts
/// no run of it.
#[test]
fn rd138_m1_adv1_own_1c_a_switch_off_whose_cancel_does_not_return_holds_across_a_new_life() {
    let mut got = Vec::new();
    for case in ["clock", "no clock"] {
        let c = coordinator(&[MANUAL, DUE], Limits::PRODUCT);
        c.ports().ports.set_clock(T0 + 7_000, DAY);
        if case == "no clock" {
            clock_fails(&c);
        }
        panicking(&c, "cancel", || c.control(&agent(1), Action::SwitchOff));
        clock_answers(&c);
        c.ports().ports.set_clock(T0 + 8_000, DAY);
        c.tick();
        let row = row_line(&c.snapshot(), 1) == text("line.off.person");
        let life = next_life(&c, |b| b.ports.set_clock(NEXT_SLOT.0, NEXT_SLOT.1));
        let starts = started(&life).len();
        let life_row = row_line(&life.snapshot(), 1) == text("line.off.person");
        eprintln!("{case}: row reads switched off {row}; new life: starts {starts}, row reads switched off {life_row}");
        got.push((case, row, starts, life_row));
    }
    assert_eq!(
        got,
        vec![("clock", true, 0, true), ("no clock", true, 0, true)],
        "(case, row switched off after the stamp, starts in the new life at the slot, row switched off in the new life)"
    );
}

/// The agent store ends agent 1's run `run`, as it does after the kill, and a tick reads it.
fn adv3_store_ends(c: &Coordinator<Both>, run: &RunId) {
    c.ports().ports.store_end(
        &agent(1),
        halvo_coord::types::PastRun {
            run: run.clone(),
            day: DAY,
            outcome: halvo_coord::types::Outcome::Completed,
            unknown: 0,
        },
    );
    c.tick();
}

/// Presses `action` for agent 1 while `Runs::cancel` panics, with the clock failing or not;
/// whether the call panicked.
fn adv3_press_with_cancel_panicking(
    c: &Coordinator<Both>,
    action: Action,
    clock_fails: bool,
) -> bool {
    c.ports().clock_fails.set(clock_fails);
    c.ports().panics.set(Some("cancel"));
    let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        c.control(&agent(1), action)
    }));
    c.ports().panics.set(None);
    c.ports().clock_fails.set(false);
    out.is_err()
}

/// F1, manual agent: the person's Switch off of a working run while the clock fails and the
/// runner's cancel panics. Intent: the Switch off always goes on and the agent stays off until the
/// person switches it on again (its hold_set is stored at once at the last read).
fn adv3_switch_off_manual(clock_fails: bool) -> (bool, usize, String, Result<(), Cause>) {
    let c = coordinator(&[MANUAL, MANUAL], Limits::PRODUCT);
    let run1 = start(&c, 1);
    c.ports().ports.set_clock(T0 + 7_000, DAY);
    let panicked = adv3_press_with_cancel_panicking(&c, Action::SwitchOff, clock_fails);
    c.ports().ports.set_clock(T0 + 8_000, DAY);
    c.tick();
    let later = T0 + 8_000 + Limits::PRODUCT.cancel_grace_s * 1000 + 1_000;
    c.ports().ports.set_clock(later, DAY);
    c.tick();
    adv3_store_ends(&c, &run1);
    let holds = person_holds(&c, 1).len();
    let line = row_line(&c.snapshot(), 1);
    let run_now = c.control(&agent(1), Action::RunNow);
    (panicked, holds, line, run_now)
}

#[test]
fn rd138_m1_adv3_f1_control_stamped_switch_off_whose_cancel_panics_keeps_the_agent_off() {
    let (panicked, holds, line, run_now) = adv3_switch_off_manual(false);
    println!("stamped: panicked={panicked} holds={holds} line={line:?} run_now={run_now:?}");
    assert!(panicked);
    assert_eq!(holds, 1, "the stamped Switch off stores its hold_set");
    assert_eq!(line, text("line.off.person"));
    assert_eq!(run_now, Err(Cause::Hold));
}

#[test]
fn rd138_m1_adv3_f1_clockless_switch_off_whose_cancel_panics_keeps_the_agent_off() {
    let (panicked, holds, line, run_now) = adv3_switch_off_manual(true);
    println!("clockless: panicked={panicked} holds={holds} line={line:?} run_now={run_now:?}");
    assert!(panicked);
    assert_eq!(
        (holds, line, run_now),
        (1, text("line.off.person"), Err(Cause::Hold)),
        "the agent must stay off after the person's Switch off"
    );
}

/// F1, slotted agent: the same press, then the agent's next slot in the same life.
#[test]
fn rd138_m1_adv3_f1_clockless_switch_off_whose_cancel_panics_then_the_next_slot() {
    for clock_fails in [false, true] {
        let c = coordinator(&[MANUAL, DUE], Limits::PRODUCT);
        let first = started(&c);
        assert_eq!(
            first.len(),
            1,
            "the due agent's run started at the first tick"
        );
        let run1 = first[0].clone();
        assert_eq!(c.on_run(&run1, RunEvent::Hello), Ok(()));
        c.ports().ports.set_clock(T0 + 7_000, DAY);
        let panicked = adv3_press_with_cancel_panicking(&c, Action::SwitchOff, clock_fails);
        c.ports().ports.set_clock(T0 + 8_000, DAY);
        c.tick();
        let later = T0 + 8_000 + Limits::PRODUCT.cancel_grace_s * 1000 + 1_000;
        c.ports().ports.set_clock(later, DAY);
        c.tick();
        adv3_store_ends(&c, &run1);
        let before = started(&c).len();
        c.ports().ports.set_clock(T0 + 2 * 86_400_000, DAY + 2);
        c.tick();
        c.ports()
            .ports
            .set_clock(T0 + 2 * 86_400_000 + 1_000, DAY + 2);
        c.tick();
        let after = started(&c).len();
        println!(
            "clock_fails={clock_fails}: panicked={panicked} holds={} starts at the next slot={} line={:?}",
            person_holds(&c, 1).len(),
            after - before,
            row_line(&c.snapshot(), 1)
        );
        assert_eq!(
            after - before,
            0,
            "clock_fails={clock_fails}: no run of a switched-off agent starts at its next slot"
        );
    }
}

/// F2: the person's Stop of a working run while the clock fails and the runner's cancel panics.
/// Intent: the stop's time is stamped at the next clock read that answers, so the kill counts
/// cancel_grace_s from that read (as rd133_a_working_runs_stop_goes_on_with_no_clock_and_the_grace_counts_from_the_stamp holds).
#[test]
fn rd138_m1_adv3_f2_stop_whose_cancel_panics_is_killed_before_its_grace() {
    for clock_fails in [false, true] {
        let c = coordinator(&[MANUAL, MANUAL], Limits::PRODUCT);
        let run1 = start(&c, 1);
        c.ports().ports.set_clock(T0 + 7_000, DAY);
        let m = mark(&c);
        let panicked = adv3_press_with_cancel_panicking(&c, Action::Stop, clock_fails);
        c.ports().ports.set_clock(T0 + 8_000, DAY);
        c.tick();
        let kills = kills_of(&c, m, &run1);
        println!("clock_fails={clock_fails}: panicked={panicked} kills one second after the press={kills}");
        assert_eq!(
            kills, 0,
            "clock_fails={clock_fails}: no kill before cancel_grace_s from the stamping read"
        );
    }
}

/// D1's own case with one more fault: the waiting agent's own Switch off while the settle waits
/// on a failing clock, and the runner's cancel panics. The intent: the Switch off goes on, and the
/// agent stays off until the person switches it on again.
#[test]
fn rd138_m1_own02_the_waiting_agents_own_switch_off_whose_cancel_panics_keeps_it_off() {
    let (c, run0, item0, _runs) = lost_answer_beside_other_runs(0);
    clock_fails(&c);
    panicking(&c, "cancel", || c.control(&agent(0), Action::SwitchOff));
    clock_answers(&c);
    c.tick();
    after_the_grace(&c);
    ended_in_store(&c, &run0, Outcome::Completed);
    let holds = person_holds(&c, 0).len();
    let line = row_line(&c.snapshot(), 0);
    let m = mark(&c);
    let run_now = c.control(&agent(0), Action::RunNow);
    println!(
        "OBSERVED holds {holds}; row {line:?}; answer_lost lapses {}; run0 ends {:?}; RunNow {run_now:?}; starts {}",
        lapses_of(&c, &item0, "answer_lost"),
        ends_of(&c, &run0),
        calls_since(&c, m).iter().filter(|x| matches!(x, Call::Start(_))).count()
    );
    assert_eq!(
        (holds, line, run_now),
        (1, text("line.off.person"), Err(Cause::Hold)),
        "the agent stays off after the person's Switch off"
    );
}

#[test]
fn rd138_m2_b02_own04_a_tick_that_read_no_clock_does_not_free_the_queue_under_the_mark() {
    let c = coordinator(&[MANUAL, MANUAL], Limits::PRODUCT);
    let run0 = start(&c, 0);
    assert_eq!(
        c.control(&agent(1), Action::RunNow),
        Ok(()),
        "queued behind run 0"
    );
    assert_eq!(c.queued(), vec![(agent(1), false)]);
    let before = started(&c).len();
    tick_that_panics(&c);
    clock_fails(&c);
    let m = mark(&c);
    c.tick();
    let worked = tick_worked(&c, m);
    let note_after_tick = c.snapshot().note;
    clock_answers(&c);
    c.ports().ports.end_runner(&run0);
    let _ = c.on_run(&run0, RunEvent::Ended(Some(Outcome::Completed)));
    let n = started(&c).len();
    println!(
        "OBSERVED tick_worked={worked} note_after_tick={note_after_tick:?} started_before={before} started_after_end={n} queued={:?}",
        c.queued()
    );
    assert_eq!(
        n, before,
        "no queued start on a state that no tick read since the mark"
    );
}

// ---- finder mark: adv1_mark_1: a tick that take_up settles and that is still held returns without a publish, so the page keeps showing the settled card and row ----
#[test]
fn rd138_m3_adv1_mark_1() {
    let (c, run0, item0, runs) = lost_answer_beside_other_runs(1);
    let run1 = runs[0].clone();
    // Agent 1's runner ends while no read of the clock answers: the end is kept.
    clock_fails(&c);
    c.ports().ports.end_runner(&run1);
    let _ = c.on_run(&run1, RunEvent::Ended(Some(Outcome::Completed)));
    // The next tick's first read answers (take_up stamps and settles), every later read fails,
    // so the kept end is kept again and the tick stays held.
    c.ports().clock_fails.set(false);
    c.ports().fail_reads_after.set(Some(1));
    let m = mark(&c);
    c.tick();
    assert_eq!(
        lost_lapses(&c, &item0),
        1,
        "the held tick's take_up settled the lost answer"
    );
    let shown = c.snapshot();
    let bell = calls_since(&c, m)
        .iter()
        .any(|call| matches!(call, Call::Changed));
    // Any later call publishes what is true now; a Stop of agent 1, whose run is already marked,
    // changes nothing.
    let _ = c.control(&agent(1), Action::Stop);
    let fresh = c.snapshot();
    assert_eq!(
        row_line(&shown, 0),
        row_line(&fresh, 0),
        "after the tick the page must not go on showing agent 0 as it was before the settle"
    );
    let ids = |s: &Snapshot| {
        s.items
            .iter()
            .map(|i| i.item().clone())
            .collect::<Vec<ItemId>>()
    };
    assert_eq!(
        ids(&shown),
        ids(&fresh),
        "the settled card must leave the page"
    );
    assert!(bell, "the page is told that the settle changed the state");
    let _ = run0;
}

// ---- interrogate-u24m-adversary-1: the finder's adv1_mark_1, recording every symptom ----

/// A settle made in a tick's take-up, by a tick that then stays held because a kept end is kept
/// again, publishes nothing: the page goes on showing the state before the settle, here across
/// three more held ticks, until some other move publishes.
#[test]
fn rd138_m3_adv1_mark_1b_a_held_tick_that_settled_publishes_nothing() {
    let (c, _run0, item0, runs) = lost_answer_beside_other_runs(1);
    let run1 = runs[0].clone();
    clock_fails(&c);
    c.ports().ports.end_runner(&run1);
    let kept = c.on_run(&run1, RunEvent::Ended(Some(Outcome::Completed)));
    c.ports().clock_fails.set(false);
    c.ports().fail_reads_after.set(Some(1));
    let before = c.snapshot();
    let m = mark(&c);
    c.tick();
    let settled = lost_lapses(&c, &item0);
    let cancels = cancels_of(&c, m, &_run0);
    let shown = c.snapshot();
    let bell = calls_since(&c, m)
        .iter()
        .any(|call| matches!(call, Call::Changed));
    for _ in 0..3 {
        c.tick();
    }
    let later = c.snapshot();
    let _ = c.control(&agent(1), Action::Stop);
    let fresh = c.snapshot();
    let ids = |s: &Snapshot| {
        s.items
            .iter()
            .map(|i| i.item().clone())
            .collect::<Vec<ItemId>>()
    };
    eprintln!("kept end answered {kept:?}; settled at the tick: answer_lost rows {settled}, cancels {cancels}");
    eprintln!(
        "seq before/after tick/after 3 held ticks/after a Stop: {}/{}/{}/{}",
        before.seq, shown.seq, later.seq, fresh.seq
    );
    eprintln!("agent 0 row after the tick: {:?}; after 3 held ticks: {:?}; after the Stop's publish: {:?}", row_line(&shown, 0), row_line(&later, 0), row_line(&fresh, 0));
    eprintln!(
        "cards after the tick: {}; after the Stop's publish: {}; bell at the tick: {bell}",
        ids(&shown).len(),
        ids(&fresh).len()
    );
    assert_eq!(
        (row_line(&later, 0), ids(&later), bell),
        (row_line(&fresh, 0), ids(&fresh), true),
        "(agent 0's row, the cards shown, the bell rung at the tick that settled)"
    );
}

/// A tick whose take-up read answers settles agent 0's lost answer and stamps agent 1's clockless
/// Switch off, then a later read of the same tick fails, so the tick is held and returns: does the
/// page get the changed state?
#[test]
fn rd138_m3_own01_a_held_tick_that_changed_the_state_in_its_take_up_publishes_it() {
    let (c, run0, item0, runs) = lost_answer_beside_other_runs(1);
    let _run1 = runs[0].clone();
    clock_fails(&c);
    assert_eq!(c.control(&agent(1), Action::SwitchOff), Ok(()));
    clock_answers(&c);
    let before = c.snapshot();
    let line0_before = row_line(&before, 0);
    // The first read of the tick (take_up's) answers; the second (the switch-off's rest) fails once.
    c.ports().fail_read_at.set(Some(1));
    c.tick();
    let after_tick = c.snapshot();
    let lapsed = lapses_of(&c, &item0, "answer_lost");
    println!(
        "OBSERVED seq {} -> {}; agent 0 row {:?} -> {:?}; answer_lost lapses stored {}; ends of run0 {:?}",
        before.seq,
        after_tick.seq,
        line0_before,
        row_line(&after_tick, 0),
        lapsed,
        ends_of(&c, &run0)
    );
    // What a move that publishes shows right after, with nothing else changed by it.
    let _ = c.card(&agent(0));
    let truth = {
        let m = c.control(&agent(0), Action::SwitchOn);
        let s = c.snapshot();
        println!(
            "OBSERVED next move {m:?}; seq {}; agent 0 row {:?}",
            s.seq,
            row_line(&s, 0)
        );
        s
    };
    assert_eq!(lapsed, 1, "the take-up settled agent 0's item");
    assert!(
        same_state(&after_tick, &truth) || after_tick.seq > before.seq,
        "the page must not go on showing a state that is no longer true: published {:?} after the tick, the state then {:?}",
        row_line(&after_tick, 0),
        row_line(&truth, 0)
    );
}

// adversary 2, probe batch m7: the quit's first step before the settle's stamp, in each world
// of RD-84 item 8 (a port that panics before its call back, after the record, after the send,
// and the owner's log of item_answered)

fn adv2_m7_world(port: &'static str, fail_all: bool) -> (usize, usize, Vec<RunEnd>, usize, usize) {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run0 = start(&c, 0);
    c.on_run(&run0, ask_confirm(&run0, 0)).unwrap();
    let (item, _) = open_item(&c);
    assert_eq!(c.shown_on_page(&item, 0), Ok(()));
    c.ports().ports.set_clock(T0 + Limits::PRODUCT.arm_ms, DAY);
    c.tick();
    panicking(&c, port, || c.answer(&item, Choice::Allow, None, 0));
    let m = mark(&c);
    let sent = decisions_since(&c, m).len();
    if fail_all {
        clock_fails(&c);
    }
    let _ = c.quit_stop();
    clock_answers(&c);
    c.tick();
    c.ports().ports.end_runner(&run0);
    let _ = c.on_run(&run0, RunEvent::Ended(None));
    let out = (
        lost_lapses(&c, &item),
        lapses_of(&c, &item, "app_ended"),
        ends_of(&c, &run0),
        cancels_of(&c, m, &run0),
        decisions_since(&c, m).len() - sent,
    );
    eprintln!("m7 {port} clock fails through the quit: {fail_all}: answer_lost {} app_ended {} ends {:?} cancels {} decisions {}", out.0, out.1, out.2, out.3, out.4);
    out
}

#[test]
fn rd138_m4_adv2_m7_quit_before_the_stamp_in_each_world_of_a_lost_answer() {
    let mut bad = Vec::new();
    for port in [
        "answer_confirm",
        "answer_confirm_after_record",
        "answer_confirm_after_send",
        "log_answered",
    ] {
        let control = adv2_m7_world(port, false);
        let failing = adv2_m7_world(port, true);
        if failing.0 != control.0 || failing.2 != control.2 {
            bad.push(format!(
                "{port}: with the clock {control:?}, clock failing at the quit {failing:?}"
            ));
        }
    }
    assert!(
        bad.is_empty(),
        "worlds whose settle the quit drops: {bad:#?}"
    );
}

#[test]
fn rd138_m5_c02_a_settle_that_does_not_return_does_not_swallow_another_agents_stop() {
    let (c, run0, item, runs) = lost_answer_beside_other_runs(1);
    let run1 = runs[0].clone();
    clock_fails(&c);
    c.tick();
    clock_answers(&c);
    let m = mark(&c);
    c.ports().panics.set(Some("log_lapsed"));
    let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        c.control(&agent(1), Action::Stop)
    }));
    c.ports().panics.set(None);
    let stop_cancels = cancels_of(&c, m, &run1);
    let settle_cancels = cancels_of(&c, m, &run0);
    let panicked = out.is_err();
    c.tick();
    let after_tick = cancels_of(&c, m, &run1);
    let lost = lapses_of(&c, &item, "answer_lost");
    println!(
        "OBSERVED stop_panicked={panicked} run1_cancels_in_move={stop_cancels} run0_cancels={settle_cancels} run1_cancels_after_tick={after_tick} answer_lost={lost} row1={:?}",
        row_line(&c.snapshot(), 1)
    );
    assert!(
        !panicked,
        "the person's Stop of agent 1 returns; the waiting settle of agent 0 is not its work"
    );
    assert_eq!(
        stop_cancels, 1,
        "the Stop of agent 1 cancels its run in that move"
    );
}

/// Lane c case 02 with a second press: agent 0's settle waits (a held tick read no clock), the
/// clock answers, and the store panics at item_lapsed inside the settle that agent 1's Stop takes
/// up first. Does the first Stop go on, and does a second press go on?
#[test]
fn rd138_m5_own03_a_settle_that_panics_inside_another_agents_stop() {
    let (c, _run0, item, runs) = lost_answer_beside_other_runs(1);
    let run1 = runs[0].clone();
    clock_fails(&c);
    c.tick();
    clock_answers(&c);
    let m = mark(&c);
    c.ports().panics.set(Some("log_lapsed"));
    let first = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        c.control(&agent(1), Action::Stop)
    }));
    c.ports().panics.set(None);
    let after_first = cancels_of(&c, m, &run1);
    let line_after_first = row_line(&c.snapshot(), 1);
    let second = c.control(&agent(1), Action::Stop);
    let after_second = cancels_of(&c, m, &run1);
    println!(
        "OBSERVED first_panicked={} run1_cancels_after_first={after_first} row1_after_first={line_after_first:?} second={second:?} run1_cancels_after_second={after_second} answer_lost={}",
        first.is_err(),
        lapses_of(&c, &item, "answer_lost")
    );
    assert!(
        !first.is_err(),
        "the first Stop of agent 1 goes on in its move"
    );
    assert_eq!(after_first, 1, "its cancel in that move");
}

// ---- RD-138 (round 3): the brief's own cases, each with an open item and without one ----

/// A Switch off under a failing clock whose `Runs::cancel` panics, which stands for one that
/// does not return: the record is written before the cancel, so the hold is stored, the row reads
/// switched off, Run now is refused, and the agent does not start at its next slot in this life
/// or in a new life (ruling RD-138 (1), M1).
#[test]
fn rd138_m1_a_clockless_switch_off_whose_cancel_panics_keeps_the_agent_off_in_both_lives() {
    for open in [false, true] {
        let c = coordinator(&[MANUAL, DUE], Limits::PRODUCT);
        let run1 = started(&c)[0].clone();
        assert_eq!(c.on_run(&run1, RunEvent::Hello), Ok(()));
        if open {
            c.on_run(&run1, ask_confirm(&run1, 1)).unwrap();
        }
        c.ports().ports.set_clock(T0 + 7_000, DAY);
        clock_fails(&c);
        panicking(&c, "cancel", || c.control(&agent(1), Action::SwitchOff));
        clock_answers(&c);
        assert_eq!(
            person_holds(&c, 1).len(),
            1,
            "open {open}: the hold is stored"
        );
        assert_eq!(
            row_line(&c.snapshot(), 1),
            text("line.off.person"),
            "open {open}"
        );
        assert_eq!(
            c.control(&agent(1), Action::RunNow),
            Err(Cause::Hold),
            "open {open}"
        );
        let life = next_life(&c, |b| b.ports.set_clock(NEXT_SLOT.0, NEXT_SLOT.1));
        assert_eq!(
            started(&life),
            vec![],
            "open {open}: no start in a new life"
        );
        assert_eq!(
            row_line(&life.snapshot(), 1),
            text("line.off.person"),
            "open {open}"
        );
        c.ports().ports.set_clock(T0 + 8_000, DAY);
        c.tick();
        c.ports().ports.end_runner(&run1);
        let _ = c.on_run(&run1, RunEvent::Ended(None));
        let before = started(&c).len();
        c.ports().ports.set_clock(NEXT_SLOT.0, NEXT_SLOT.1);
        c.tick();
        assert_eq!(
            started(&c).len(),
            before,
            "open {open}: no start at the next slot"
        );
    }
}

/// A Stop under a failing clock whose `Runs::cancel` panics keeps its grace: no kill 1 s after
/// the press, the kill at the grace from the read that stamped the stop, also for a run with an
/// open item, whose cancel goes out in the lapse of that item (ruling RD-138 (1), M1).
#[test]
fn rd138_m1_a_clockless_stop_whose_cancel_panics_keeps_its_grace() {
    for open in [false, true] {
        let c = coordinator(&[MANUAL, MANUAL], Limits::PRODUCT);
        let run1 = start(&c, 1);
        if open {
            c.on_run(&run1, ask_confirm(&run1, 1)).unwrap();
        }
        let press = T0 + 7_000;
        c.ports().ports.set_clock(press, DAY);
        clock_fails(&c);
        let m = mark(&c);
        panicking(&c, "cancel", || c.control(&agent(1), Action::Stop));
        clock_answers(&c);
        let stamp = press + 1_000;
        c.ports().ports.set_clock(stamp, DAY);
        c.tick();
        assert_eq!(
            kills_of(&c, m, &run1),
            0,
            "open {open}: no kill 1 s after the press"
        );
        c.ports()
            .ports
            .set_clock(stamp + Limits::PRODUCT.cancel_grace_s * 1000, DAY);
        c.tick();
        assert_eq!(
            kills_of(&c, m, &run1),
            1,
            "open {open}: the kill at the grace"
        );
    }
}

/// A Switch off under a failing clock whose `hold_set` store panics: the cancel still goes out
/// in that move, the store's panic goes on after it, and Run now is refused (ruling RD-138 (1),
/// M1).
#[test]
fn rd138_m1_a_clockless_switch_off_whose_hold_store_panics_still_cancels_in_the_move() {
    for open in [false, true] {
        let c = coordinator(&[MANUAL, MANUAL], Limits::PRODUCT);
        let run1 = start(&c, 1);
        if open {
            c.on_run(&run1, ask_confirm(&run1, 1)).unwrap();
        }
        clock_fails(&c);
        let m = mark(&c);
        panicking(&c, "log_hold_set", || {
            c.control(&agent(1), Action::SwitchOff)
        });
        assert_eq!(
            cancels_of(&c, m, &run1),
            1,
            "open {open}: one cancel in that move"
        );
        clock_answers(&c);
        assert_eq!(
            c.control(&agent(1), Action::RunNow),
            Err(Cause::Hold),
            "open {open}: Run now is refused"
        );
    }
}

/// After a tick that did not return, a tick under a failing clock reads nothing and keeps the
/// mark: the note stays, a queued start is not served at a run's end, and in that window Stop
/// and Switch off answer Ok and send the cancel (ruling RD-138 (2), M2).
#[test]
fn rd138_m2_a_tick_that_reads_no_clock_keeps_the_mark_and_stop_goes_on() {
    for action in [Action::Stop, Action::SwitchOff] {
        let c = coordinator(&[MANUAL, MANUAL], Limits::PRODUCT);
        let run0 = start(&c, 0);
        assert_eq!(
            c.control(&agent(1), Action::RunNow),
            Ok(()),
            "queued behind run 0"
        );
        let before = started(&c).len();
        tick_that_panics(&c);
        clock_fails(&c);
        c.tick();
        assert_eq!(
            c.snapshot().note,
            Some(text("note.out_of_date")),
            "{action:?}: the note stays"
        );
        let m = mark(&c);
        assert_eq!(
            c.control(&agent(0), action),
            Ok(()),
            "{action:?} answers Ok"
        );
        assert_eq!(cancels_of(&c, m, &run0), 1, "{action:?}: the cancel");
        clock_answers(&c);
        c.ports().ports.end_runner(&run0);
        let _ = c.on_run(&run0, RunEvent::Ended(None));
        assert_eq!(
            started(&c).len(),
            before,
            "{action:?}: the queue is not served"
        );
        assert_eq!(
            c.snapshot().note,
            Some(text("note.out_of_date")),
            "{action:?}"
        );
    }
}

// ---- RD-140 (round 4) ----

/// pr-review-38 Addition 2 case 8 (A2-B1): agent 0's clockless Stop left an unstamped
/// `item_lapsed`; agent 1's Stop is the next move, and the store panics at that row in the
/// move's take-up. Agent 1's Stop still goes on with its one cancel (ruling RD-140 R1).
#[test]
fn rd140_r1_c08_another_agents_waiting_row_panics_in_the_same_move() {
    let limits = Limits {
        max_live_runs: 2,
        ..Limits::PRODUCT
    };
    let c = coordinator(&[MANUAL, MANUAL, MANUAL], limits);
    let (_run0, _item) = allowed(&c);
    let run1 = start(&c, 1);
    c.ports().clock_fails.set(true);
    assert_eq!(c.control(&agent(0), Action::Stop), Ok(()));
    c.ports().clock_fails.set(false);
    let m = mark(&c);
    c.ports().panics.set(Some("log_lapsed"));
    let pressed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        c.control(&agent(1), Action::Stop)
    }));
    c.ports().panics.set(None);
    let in_move = cancels_of(&c, m, &run1);
    c.tick();
    let after_tick = cancels_of(&c, m, &run1);
    assert_eq!(
        (pressed.is_ok(), in_move, after_tick),
        (true, 1, 1),
        "agent 1's Stop in the move whose take-up flushed agent 0's waiting row"
    );
}

/// A panic in the rest of another agent's switch-off, which the move's take-up makes at the
/// first read that answers, is held to that agent: this move's Stop goes on with its cancel,
/// the other agent stays held off, and its hold is stored at a later move (ruling RD-140 R1).
#[test]
fn rd140_r1_a_panic_in_another_agents_switch_off_rest_does_not_lose_this_stop() {
    let limits = Limits {
        max_live_runs: 2,
        ..Limits::PRODUCT
    };
    let c = coordinator(&[MANUAL, MANUAL], limits);
    let run0 = start(&c, 0);
    let _run1 = start(&c, 1);
    clock_fails(&c);
    c.ports().ports.refuse(Method::Log, PortError::Busy);
    assert_eq!(c.control(&agent(1), Action::SwitchOff), Ok(()));
    c.ports().ports.clear_refusals();
    clock_answers(&c);
    let m = mark(&c);
    c.ports().panics.set(Some("log_hold_set"));
    let pressed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        c.control(&agent(0), Action::Stop)
    }));
    c.ports().panics.set(None);
    assert_eq!(pressed.ok(), Some(Ok(())), "this move's Stop returns Ok");
    assert_eq!(cancels_of(&c, m, &run0), 1, "with its one cancel");
    assert_eq!(
        c.control(&agent(1), Action::RunNow),
        Err(Cause::Hold),
        "agent 1 held off"
    );
    c.ports().ports.set_clock(T0 + 1_000, DAY);
    c.tick();
    assert_eq!(
        person_holds(&c, 1).len(),
        1,
        "the hold is stored at a later move"
    );
    assert_eq!(row_line(&c.snapshot(), 1), text("line.off.person"));
}

/// A panic in another agent's kept end, which the move's take-up makes at the first read that
/// answers, is held to that end: this move's Stop goes on with its cancel, and the end is kept
/// and made once at a later move with the runner's outcome (ruling RD-140 R1).
#[test]
fn rd140_r1_a_panic_in_another_agents_kept_end_does_not_lose_this_stop() {
    let limits = Limits {
        max_live_runs: 2,
        ..Limits::PRODUCT
    };
    let c = coordinator(&[MANUAL, MANUAL], limits);
    let run0 = start(&c, 0);
    let run1 = start(&c, 1);
    clock_fails(&c);
    c.ports().ports.end_runner(&run1);
    let _ = c.on_run(&run1, RunEvent::Ended(Some(Outcome::Completed)));
    clock_answers(&c);
    let m = mark(&c);
    c.ports().panics.set(Some("log_ended"));
    let pressed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        c.control(&agent(0), Action::Stop)
    }));
    c.ports().panics.set(None);
    assert_eq!(pressed.ok(), Some(Ok(())), "this move's Stop returns Ok");
    assert_eq!(cancels_of(&c, m, &run0), 1, "with its one cancel");
    c.ports().ports.set_clock(T0 + 1_000, DAY);
    c.tick();
    assert_eq!(
        ends_of(&c, &run1),
        vec![RunEnd::Completed],
        "made once, as the runner said"
    );
    assert_eq!(cancels_of(&c, m, &run1), 0, "no cancel to the ended run");
}

/// pr-review-38 Addition 2's kill-s20 (A2-B2): while a switch-off waits for its stamp, a Switch
/// on is refused, so the stamp does not undo a Switch on that answered Ok (ruling RD-140 R2).
#[test]
fn rd140_r2_a_switch_on_in_the_window_is_not_undone_by_the_stamp() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    c.ports().clock_fails.set(true);
    assert_eq!(c.control(&agent(0), Action::SwitchOff), Ok(()));
    c.ports().clock_fails.set(false);
    c.ports().fail_read_at.set(Some(0));
    let on = c.control(&agent(0), Action::SwitchOn);
    c.ports().fail_read_at.set(None);
    c.tick();
    c.tick();
    let line = row_line(&c.snapshot(), 0);
    assert!(
        !(on == Ok(()) && line == text("line.off.person")),
        "a Switch on answered Ok must not be undone by the switch-off's stamp: on={on:?} line={line}"
    );
}

/// Verifier-54 Addition 3 case 8a in its Switch off form: agent 0's clockless Stop left an
/// unstamped `item_lapsed`; agent 1's Switch off is the next move, and the store panics at that
/// row in the move's take-up. Agent 1's Switch off still goes on: Ok, one cancel in that move,
/// its hold stored, Run now refused (ruling RD-140 R1).
#[test]
fn rd140_r1_c08_a_switch_off_beside_another_agents_waiting_row_that_panics() {
    let limits = Limits {
        max_live_runs: 2,
        ..Limits::PRODUCT
    };
    let c = coordinator(&[MANUAL, MANUAL, MANUAL], limits);
    let (_run0, _item) = allowed(&c);
    let run1 = start(&c, 1);
    c.ports().clock_fails.set(true);
    assert_eq!(c.control(&agent(0), Action::Stop), Ok(()));
    c.ports().clock_fails.set(false);
    let m = mark(&c);
    c.ports().panics.set(Some("log_lapsed"));
    let pressed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        c.control(&agent(1), Action::SwitchOff)
    }));
    c.ports().panics.set(None);
    assert_eq!(
        (
            pressed.ok(),
            cancels_of(&c, m, &run1),
            person_holds(&c, 1).len(),
            c.control(&agent(1), Action::RunNow)
        ),
        (Some(Ok(())), 1, 1, Err(Cause::Hold)),
        "(the press, cancels in the move, hold_set rows, Run now)"
    );
}

fn one_at_a_time() -> Limits {
    Limits {
        max_live_runs: 1,
        ..Limits::PRODUCT
    }
}

/// The row offers the take-back exactly while a start of the agent waits in the queue, whoever
/// queued it (I2.5, I8.1; ruling RD-54 (h)).
#[test]
fn u24e_the_row_offers_take_back_exactly_while_a_start_of_the_agent_is_queued() {
    use halvo_coord::types::Control;
    let c = coordinator(&[MANUAL, MANUAL], one_at_a_time());
    assert!(!controls_of(&c, 1).contains(&Control::TakeBack), "idle");
    let first = start(&c, 0);
    assert!(!controls_of(&c, 0).contains(&Control::TakeBack), "live");
    assert_eq!(c.control(&agent(1), Action::RunNow), Ok(()));
    assert_eq!(
        controls_of(&c, 1),
        [Control::TakeBack, Control::SwitchOff, Control::Delete],
        "queued"
    );
    assert_eq!(row_line(&c.snapshot(), 1), text("line.waiting_turn.queued"));
    assert_eq!(c.control(&agent(1), Action::SwitchOff), Ok(()));
    assert!(!controls_of(&c, 1).contains(&Control::TakeBack), "off");
    assert_eq!(c.control(&agent(1), Action::SwitchOn), Ok(()));
    c.ports().ports.end_runner(&first);
    c.on_run(&first, RunEvent::Ended(Some(Outcome::Completed)))
        .unwrap();
    assert!(!controls_of(&c, 0).contains(&Control::TakeBack), "ended");
}

/// A take-back drops the queued start with nothing stored or sent and no hold; the ready item
/// that the start answered stays closed, the next end takes no start of it, and a later tick
/// asks again (I5.3; rulings RD-54 (h), RD-55 (b)).
#[test]
fn u24e_a_take_back_drops_the_queued_start_stores_nothing_and_a_later_tick_asks_again() {
    use halvo_coord::types::Control;
    let slotted = support::A {
        person: true,
        slotted: true,
    };
    let c = coordinator(&[MANUAL, slotted], one_at_a_time());
    let first = start(&c, 0);
    c.tick();
    let (asked, kind) = open_item(&c);
    assert_eq!(kind, ItemKind::Ready);
    assert_eq!(c.answer(&asked, Choice::Start, None, 0), Ok(()));
    assert_eq!(c.queued(), vec![(agent(1), false)], "queued by its card");
    let stored = events(&c).len();
    let m = mark(&c);
    assert_eq!(c.control(&agent(1), Action::TakeBack), Ok(()));
    assert_eq!(c.queued(), vec![], "the start left the queue");
    assert_eq!(events(&c).len(), stored, "nothing stored");
    assert_eq!(
        calls_since(&c, m),
        [Call::Changed, Call::Attention(0)],
        "no call of the owner or of the runs, and the page hears the change"
    );
    let s = c.snapshot();
    assert!(
        s.items.is_empty(),
        "the answered card does not come back at the press"
    );
    assert_eq!(row_line(&s, 1), text("line.all_set.done"));
    assert_eq!(
        controls_of(&c, 1),
        [Control::RunNow, Control::SwitchOff, Control::Delete],
        "the agent stays on"
    );
    c.ports().ports.end_runner(&first);
    c.on_run(&first, RunEvent::Ended(Some(Outcome::Completed)))
        .unwrap();
    assert_eq!(started(&c).len(), 1, "the next end takes no start of it");
    c.tick();
    let (again, kind) = open_item(&c);
    assert_eq!(kind, ItemKind::Ready, "a later tick asks again");
    assert_ne!(again, asked, "a new item");
}

/// A take-back that arrives after the run started answers ok and changes nothing, as a Stop from
/// a page that is behind (ruling RD-52 (d)): no cancel, no row, and the row offers Stop.
#[test]
fn u24e_a_take_back_after_the_run_started_answers_ok_and_stops_nothing() {
    use halvo_coord::types::Control;
    let c = coordinator(&[MANUAL, MANUAL], one_at_a_time());
    let first = start(&c, 0);
    assert_eq!(c.control(&agent(1), Action::RunNow), Ok(()));
    c.ports().ports.end_runner(&first);
    c.on_run(&first, RunEvent::Ended(Some(Outcome::Completed)))
        .unwrap();
    let new = started(&c).last().cloned().unwrap();
    assert_ne!(new, first, "the end started agent 1");
    let stored = events(&c).len();
    let m = mark(&c);
    assert_eq!(c.control(&agent(1), Action::TakeBack), Ok(()));
    assert_eq!(cancels_of(&c, m, &new), 0, "no cancel");
    assert!(calls_since(&c, m).is_empty(), "no call of any port");
    assert_eq!(events(&c).len(), stored, "nothing stored");
    assert_eq!(c.live(), 1, "the run goes on");
    let controls = controls_of(&c, 1);
    assert!(
        controls.contains(&Control::Stop) && !controls.contains(&Control::TakeBack),
        "{controls:?}"
    );
}

/// The take-back reads no clock, so it goes on while the clock fails and under the mark of a tick
/// that did not return, and it is `unknown_agent` with nothing stored once a tick saw the
/// workflow gone (I5.7, ruling RD-52 (e)).
#[test]
fn u24e_a_take_back_goes_on_without_the_clock_and_is_unknown_agent_for_a_gone_workflow() {
    for marked in [false, true] {
        let c = coordinator(&[MANUAL, MANUAL], one_at_a_time());
        start(&c, 0);
        assert_eq!(c.control(&agent(1), Action::RunNow), Ok(()));
        if marked {
            tick_that_panics(&c);
        }
        let stored = events(&c).len();
        clock_fails(&c);
        assert_eq!(c.control(&agent(1), Action::TakeBack), Ok(()), "{marked}");
        assert_eq!(c.queued(), vec![], "{marked}: out of the queue");
        assert_eq!(events(&c).len(), stored, "{marked}: nothing stored");
        clock_answers(&c);
    }
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    c.ports()
        .ports
        .workflows
        .borrow_mut()
        .retain(|w| w.id() != &support::flow(0));
    c.tick();
    let stored = events(&c).len();
    assert_eq!(
        c.control(&agent(0), Action::TakeBack),
        Err(Cause::UnknownAgent)
    );
    assert_eq!(events(&c).len(), stored, "nothing stored");
}
