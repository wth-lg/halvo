#[path = "coordinator_support.rs"]
mod support;

use halvo_coord::mem::{Call, Method};
use halvo_coord::ports::PortError;
use halvo_coord::sync::Next;
use halvo_coord::types::{
    Action, AgentId, Body, Category, Cause, Choice, Class, ConfirmAsk, HandbackAsk, HandbackReason,
    ItemId, ItemKind, LapseCause, Lapsed, Limits, Name, OtherLapse, Outcome, Param, PastRun,
    RunEnd, RunEvent, RunId, Snapshot, Status, What, Workflow, WorkflowId, WorkflowState,
    WorkflowTrigger,
};
use halvo_coord::{words, Coordinator};
use support::{
    agent, calls_since, coordinator, mark, owner, started, Both, DAY, MANUAL, PERSON, T0,
};

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
            cause: RunEnd::Cancelled
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
    assert_eq!(c.answer(&item, Choice::Allow, None), Err(Cause::NoService));
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
        cause: RunEnd::Cancelled
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
        cause: RunEnd::HandedBack
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
    c.answer(&item, Choice::Choose, Some((Param::Start, 1)))
        .unwrap();
    c.answer(&item, Choice::Choose, Some((Param::Payee, 1)))
        .unwrap();
    c.ports().ports.refuse(
        Method::Send,
        PortError::Refused(Name::parse("no_workflow").unwrap()),
    );
    assert_eq!(c.setup_typed(&item, Param::Instrument, "Card 4242"), Ok(()));
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
    c.answer(&item, Choice::Done, None).unwrap();
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

fn recorded(c: &Coordinator<Both>) -> (RunId, ItemId) {
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
    assert_eq!(c.answer(&item, Choice::Allow, None), Err(Cause::NoService));
    (run, item)
}

#[test]
fn a_recorded_confirmation_of_a_stopped_run_lapses_with_run_ended_at_the_runners_end() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let (run, item) = recorded(&c);
    assert_eq!(c.control(&agent(0), Action::Stop), Ok(()));
    c.ports().ports.end_runner(&run);
    assert_eq!(c.on_run(&run, RunEvent::Ended(None)), Ok(()));
    assert!(events(&c).contains(&What::ItemLapsed {
        item,
        lapsed: Lapsed::Confirm(LapseCause::RunEnded)
    }));
    assert!(events(&c).contains(&What::RunEnded {
        run,
        cause: RunEnd::Cancelled
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
    let (_, recorded_item) = recorded(&c);
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
        c.answer(&item, Choice::Choose, Some((Param::Start, 99))),
        Err(Cause::Malformed)
    );
    let s = c.snapshot();
    assert_eq!(
        s.rows.iter().find(|r| r.agent == made).unwrap().status,
        Status::NeedsYou
    );
    assert_eq!(s.items.len(), 1);
    c.answer(&item, Choice::Choose, Some((Param::Start, 1)))
        .unwrap();
    c.answer(&item, Choice::Choose, Some((Param::Payee, 1)))
        .unwrap();
    c.ports().ports.refuse(Method::Send, PortError::Busy);
    assert_eq!(c.setup_typed(&item, Param::Instrument, "Card 4242"), Ok(()));
    assert_eq!(c.answer(&item, Choice::None, None), Err(Cause::State));
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
    assert_eq!(c.answer(&item, Choice::Allow, None), Ok(()));
    assert_eq!(
        bind_answers(&c),
        [(run.clone(), halvo_coord::types::BindResult::Allow)]
    );
    assert!(c.snapshot().items.is_empty());
    assert_eq!(row_line(&c.snapshot(), 0), text("line.working.working"));
    assert_eq!(
        c.answer(&item, Choice::Allow, None),
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
fn the_persons_stop_of_a_second_step_whose_row_is_stored_works_and_its_end_lapses_it() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    c.on_run(&run, bind(&run, 0)).unwrap();
    let (item, _) = open_item(&c);
    c.ports()
        .ports
        .refuse(Method::AnswerBind, PortError::NoService);
    assert_eq!(c.answer(&item, Choice::Allow, None), Err(Cause::NoService));
    c.ports().ports.clear_refusals();
    let m = mark(&c);
    assert_eq!(c.control(&agent(0), Action::Stop), Ok(()));
    let calls = calls_since(&c, m);
    assert!(!calls
        .iter()
        .any(|call| matches!(call, Call::AnswerBind(..))));
    assert_eq!(
        calls
            .iter()
            .filter(|call| matches!(call, Call::Cancel(_)))
            .count(),
        1
    );
    assert_eq!(row_line(&c.snapshot(), 0), text("line.working.stopping"));
    c.ports().ports.end_runner(&run);
    assert_eq!(c.on_run(&run, RunEvent::Ended(None)), Ok(()));
    assert!(events(&c).contains(&What::ItemLapsed {
        item,
        lapsed: Lapsed::SignIn(OtherLapse::RunEnded)
    }));
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
    assert_eq!(c.answer(&item, Choice::Done, None), Ok(()));
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
        cause: end
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
    assert_eq!(c.answer(&item, Choice::Reject, None), Ok(()));
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
    assert_eq!(c.answer(&item, Choice::Allow, None), Ok(()));
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
        c.answer(&item, Choice::Done, None).unwrap();
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
    c.answer(&item, Choice::Choose, Some((Param::Start, 1)))
        .unwrap();
    c.answer(&item, Choice::Choose, Some((Param::Payee, 1)))
        .unwrap();
    c.ports().ports.refuse(
        Method::Send,
        PortError::Refused(Name::parse("binding").unwrap()),
    );
    assert_eq!(c.setup_typed(&item, Param::Instrument, "Card 4242"), Ok(()));
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
    c.answer(&item, Choice::Choose, Some((Param::Start, 1)))
        .unwrap();
    c.answer(&item, Choice::Choose, Some((Param::Payee, 1)))
        .unwrap();
    c.ports().ports.refuse(
        Method::Send,
        PortError::Refused(Name::parse("config").unwrap()),
    );
    assert_eq!(c.setup_typed(&item, Param::Instrument, "Card 4242"), Ok(()));
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
        c.answer(&item, Choice::Choose, Some((Param::Start, 1))),
        Ok(())
    );
}
