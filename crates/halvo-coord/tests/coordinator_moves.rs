#[path = "coordinator_support.rs"]
mod support;

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
    assert_eq!(c.answer(&item, Choice::Allow, None), Err(Cause::NoService));
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
fn the_persons_stop_of_a_second_step_whose_row_is_stored_answers_decline_and_lapses_it_cancelled() {
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
            cause: RunEnd::Cancelled
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
    c.answer(&item, Choice::Choose, Some((Param::Start, 1)))
        .unwrap();
    c.answer(&item, Choice::Choose, Some((Param::Payee, 1)))
        .unwrap();
    assert!(
        c.card(&made).is_ok(),
        "a card before the owner ends the body"
    );
    c.ports().ports.refuse(
        Method::Send,
        PortError::Refused(Name::parse("no_workflow").unwrap()),
    );
    assert_eq!(c.setup_typed(&item, Param::Instrument, "Card 4242"), Ok(()));
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
    c.answer(&item, Choice::Choose, Some((Param::Start, 1)))
        .unwrap();
    c.answer(&item, Choice::Choose, Some((Param::Payee, 1)))
        .unwrap();
    assert_eq!(c.setup_typed(&item, Param::Instrument, "Card 4242"), Ok(()));
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
        cause: RunEnd::Lapsed
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
    let _ = c.answer(&item, Choice::Allow, None);
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
        .answer(&open, Choice::Choose, Some((Param::Start, 1)))
        .unwrap();
    setup
        .answer(&open, Choice::Choose, Some((Param::Payee, 1)))
        .unwrap();
    assert_eq!(
        setup.setup_typed(&open, Param::Instrument, "Card 4242"),
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
                cause: RunEnd::Lapsed
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
        cause: RunEnd::Lapsed
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
        cause: RunEnd::Interrupted
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
    assert_eq!(c.answer(&item, Choice::Done, None), Err(Cause::NoService));
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
    assert_eq!(c.answer(&item, Choice::Start, None), Err(Cause::NoService));
    lose_the_acknowledgement(&c);
    let _ = c.quit_stop();
    assert_eq!(closing_rows(&c, &item), 1, "{:?}", events(&c));
}

#[test]
fn a_second_answer_after_a_lost_acknowledgement_takes_the_stored_answer_which_decides() {
    for (second, decided) in [(Choice::Done, false), (Choice::Decline, true)] {
        let c = coordinator(&[MANUAL], Limits::PRODUCT);
        let run = start(&c, 0);
        c.on_run(&run, handback(&run, 0, HandbackReason::StepFailed))
            .unwrap();
        let (item, _) = open_item(&c);
        c.ports().ports.refuse(Method::Log, PortError::NoService);
        assert_eq!(c.answer(&item, Choice::Done, None), Err(Cause::NoService));
        lose_the_acknowledgement(&c);
        let m = mark(&c);
        let out = c.answer(&item, second, None);
        if decided {
            assert_eq!(out, Err(Cause::UnknownItem), "{second:?}");
        } else {
            assert_eq!(out, Ok(()), "{second:?}");
        }
        assert_eq!(closing_rows(&c, &item), 1, "{second:?}: {:?}", events(&c));
        let handed: Vec<Call> = calls_since(&c, m)
            .into_iter()
            .filter(|call| matches!(call, Call::AnswerHandback(..)))
            .collect();
        assert_eq!(
            handed,
            [Call::AnswerHandback(
                run.clone(),
                halvo_coord::types::HandbackResult::Done
            )],
            "{second:?}: the stored answer's result, once"
        );
        let s = c.snapshot();
        assert!(s.items.is_empty(), "{second:?}");
        assert_eq!(row_line(&s, 0), text("line.working.working"), "{second:?}");
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
    assert_eq!(c.answer(&item, Choice::Done, None), Ok(()));
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
        c.answer(&item, Choice::Done, None),
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
    for order in ORDERS {
        let (c, _run, item) = answered_with_a_lost_reply(order);
        let m = mark(&c);
        let tried = c.ports().ports.tried.borrow().len();
        assert_eq!(c.answer(&item, Choice::Done, None), Ok(()), "{order:?}");
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
fn a_stored_answer_whose_reply_was_lost_decides_another_choice_in_both_orders() {
    use halvo_coord::types::HandbackResult;
    for order in ORDERS {
        let (c, _run, item) = answered_with_a_lost_reply(order);
        let m = mark(&c);
        assert_eq!(
            c.answer(&item, Choice::Decline, None),
            Err(Cause::UnknownItem),
            "{order:?}"
        );
        assert_eq!(
            closing_rows(&c, &item),
            1,
            "{order:?}: one row, the stored done"
        );
        assert_eq!(
            handed_back(&c, m),
            [HandbackResult::Done],
            "{order:?}: the stored choice"
        );
        assert!(c.snapshot().items.is_empty(), "{order:?}");
    }
}

#[test]
fn a_stop_after_a_stored_answer_whose_reply_was_lost_stores_no_lapse_in_both_orders() {
    for order in ORDERS {
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
                cause: RunEnd::Cancelled
            }),
            "{order:?}"
        );
    }
}

#[test]
fn the_runners_end_after_a_stored_answer_whose_reply_was_lost_stores_no_lapse_in_both_orders() {
    for order in ORDERS {
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
                cause: RunEnd::HandedBack
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
    for order in ORDERS {
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
                cause: RunEnd::Lapsed
            }),
            "{order:?}"
        );
    }
}

/// What the lossy sequences found: by the kind of the item, the items that end with more than one
/// closing row; the confirmations whose `item_answered` rows stand beside their lapse, which I4.5
/// allows; the second steps, told by their answer row, with two answer rows or with an answer and
/// a lapse, and the `link_account` items with `done` and then `decline`, which ruling RD-46
/// allows; the Bind moves made and the second steps that they raised; each `answer_bind` with
/// `allow` that did its work, and the ones that broke a property of RD-46 (5) T19.
#[derive(Debug, Default)]
struct Lossy {
    twice: Vec<ItemKind>,
    confirm_rows: usize,
    second_twice: usize,
    second_lapsed: usize,
    link_done_decline: usize,
    bind_moves: usize,
    binds_raised: usize,
    second_steps: usize,
    allows: usize,
    broken: Vec<String>,
    c4_seeds: Vec<u64>,
}

impl Lossy {
    fn add(&mut self, o: Lossy) {
        self.twice.extend(o.twice);
        self.confirm_rows += o.confirm_rows;
        self.second_twice += o.second_twice;
        self.second_lapsed += o.second_lapsed;
        self.link_done_decline += o.link_done_decline;
        self.bind_moves += o.bind_moves;
        self.binds_raised += o.binds_raised;
        self.second_steps += o.second_steps;
        self.allows += o.allows;
        self.broken.extend(o.broken);
        self.c4_seeds.extend(o.c4_seeds);
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

/// One sequence of drawn moves over two agents in which each `log` loses its acknowledgement
/// with chance one in four, in the form `form`; the rows of a `Later` loss land in the owner's
/// events after the move where their `seq` is still free. A drawn move can bring a run's request
/// `Bind` for one of two origins, and an answer draws among the item's own choices. After each
/// move every `answer_bind` with `allow` that did its work is checked: it was made inside a move
/// that answers that item with Allow, the item's newest `item_answered` row was `allow` at the
/// call, and it came after no Stop or switch-off of its run, no lapse of the item and no Don't
/// allow on the item answered ok (ruling RD-46 (5) T19).
fn lossy_sequence(seed: u64, form: support::Loss) -> Lossy {
    use halvo_coord::types::{
        Answered, BindResult, DoneAnswer, Event, LinkAnswer, Origin, Raised, SignInAnswer,
    };
    let limits = Limits {
        max_live_runs: 2,
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
        let mut pressed: Option<(ItemId, Choice)> = None;
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
                    let answered = c.answer(&id, choice, None);
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
        let link_done_decline = matches!(
            &answers[..],
            [
                Answered::LinkAccount(LinkAnswer {
                    answer: DoneAnswer::Done,
                    ..
                }),
                Answered::LinkAccount(LinkAnswer {
                    answer: DoneAnswer::Decline,
                    ..
                }),
            ]
        ) && ended == 0;
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
        if link_done_decline {
            out.link_done_decline += 1;
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
    for form in ORDERS {
        let mut all = Lossy::default();
        for seed in 0..count {
            all.add(lossy_sequence(seed, form));
        }
        let mut by_kind: Vec<(ItemKind, usize)> = Vec::new();
        for k in &all.twice {
            match by_kind.iter_mut().find(|(kind, _)| kind == k) {
                Some((_, n)) => *n += 1,
                None => by_kind.push((*k, 1)),
            }
        }
        println!(
            "lossy form={form:?} sequences={count} items_with_two_closing_rows={} by_kind={by_kind:?} confirmations_answered_and_lapsed={} bind_moves={} second_steps_raised={} second_steps_answered={} second_steps_with_two_answer_rows={} second_steps_answered_and_lapsed={} link_done_then_decline={} allows_sent={} broken={}",
            all.twice.len(),
            all.confirm_rows,
            all.bind_moves,
            all.binds_raised,
            all.second_steps,
            all.second_twice,
            all.second_lapsed,
            all.link_done_decline,
            all.allows,
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
    assert_eq!(c.answer(&item, Choice::Done, None), Err(Cause::NoService));
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
/// RD-43 (a) to (c)). The snapshot of the hold's check card reads `Owner::agents` for the
/// unknown effects that the card shows, after every move of the person while it stands.
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
    a_stop_changes_nothing(&c, &agent(0), &[Call::Agents], "the hold's check");

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
    assert_eq!(c.answer(&ready, Choice::Start, None), Ok(()));
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
    a_stop_changes_nothing(&c, &agent(1), &[], "a queued agent with a ready item");
    c.ports().ports.end_runner(&first);
    c.on_run(&first, RunEvent::Ended(Some(Outcome::Completed)))
        .unwrap();
    let starts = started(&c);
    assert_eq!(
        starts.len(),
        2,
        "the queued start is taken when its turn comes"
    );
    let second = starts.last().cloned().unwrap();
    assert!(events(&c).iter().any(|w| matches!(
        w,
        What::RunStarted { run, .. } if *run == second
    )));

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
                c.answer(item, choice, None),
                Err(Cause::NoService),
                "{how:?}"
            );
            let lost: Vec<halvo_coord::types::Event> =
                c.ports().lost.borrow_mut().drain(..).collect();
            c.ports().ports.events.borrow_mut().extend(lost);
        }
        First::Taken => {
            c.ports().ports.refuse(Method::AnswerBind, PortError::Busy);
            assert_eq!(c.answer(item, choice, None), Err(Cause::Busy), "{how:?}");
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
        assert_eq!(c.answer(&item, Choice::Allow, None), Ok(()), "{order:?}");
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
        assert_eq!(c.answer(&item, Choice::Decline, None), Ok(()), "{order:?}");
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
        assert_eq!(c.answer(&item, Choice::Allow, None), Ok(()), "{order:?}");
        assert_eq!(step_rows(&c, &item), ["decline", "allow"], "{order:?}");
        assert_eq!(allows(&c), 1, "{order:?}: by the person's last press");
        let (c, _run, item) = second_step();
        first_press(&c, &item, Choice::Decline, First::Lost(order));
        assert_eq!(c.answer(&item, Choice::Decline, None), Ok(()), "{order:?}");
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
            cause: RunEnd::Cancelled
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
            cause: RunEnd::Lapsed
        }));
        assert_eq!(allows(&c), 0, "{order:?}");
    }
}

fn run_ends_of(c: &Coordinator<Both>, run: &RunId) -> Vec<RunEnd> {
    events(c)
        .iter()
        .filter_map(|w| match w {
            What::RunEnded { run: r, cause } if r == run => Some(*cause),
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
    assert_eq!(c.answer(&item, Choice::Allow, None), Ok(()));
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
    assert_eq!(c.answer(&item, Choice::Decline, None), Ok(()));
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
        c.answer(&item, Choice::Decline, None),
        Err(Cause::NoService)
    );
    c.ports().ports.clear_refusals();
    assert!(binds_since(&c, m).is_empty(), "nothing is sent");
    assert_eq!(step_rows(&c, &item), ["allow"]);
    let m = mark(&c);
    assert_eq!(c.answer(&item, Choice::Allow, None), Ok(()));
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
    assert_eq!(c.answer(&item, Choice::Allow, None), Err(Cause::State));
    assert!(binds_since(&c, m).is_empty());
    assert_eq!(step_rows(&c, &item), ["decline"]);
    assert_eq!(c.answer(&item, Choice::Decline, None), Ok(()));
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
            c.answer(&item, Choice::Allow, None),
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
    assert_eq!(c.answer(&item, Choice::Allow, None), Ok(()));
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
        c.answer(&item, Choice::Allow, None),
        Err(Cause::UnknownItem)
    );
    assert_eq!(step_rows(&c, &item), ["lapsed:run_ended"]);
}

#[test]
fn two_second_steps_of_one_run_keep_their_own_rows_and_a_second_bind_ends_the_run_with_kind() {
    use halvo_coord::types::BindResult;
    let (c, run, a) = second_step();
    first_press(&c, &a, Choice::Allow, First::Lost(support::Loss::Reply));
    assert_eq!(c.answer(&a, Choice::Allow, None), Ok(()));
    let other = RunEvent::Bind(halvo_coord::types::BindAsk {
        run: run.clone(),
        agent: agent(0),
        origin: halvo_coord::types::Origin::parse("https://card.site1.example:443").unwrap(),
    });
    c.on_run(&run, other).unwrap();
    let (b, _) = open_item(&c);
    assert_ne!(a, b);
    assert_eq!(c.answer(&b, Choice::Decline, None), Ok(()));
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
    assert_eq!(c.answer(&item, Choice::Allow, None), Ok(()), "{way}");
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
    for order in ORDERS {
        let c = coordinator(&[MANUAL], Limits::PRODUCT);
        let run = start(&c, 0);
        c.on_run(&run, handback(&run, 0, HandbackReason::AccountLink))
            .unwrap();
        let (item, _) = open_item(&c);
        c.ports().loss.set(Some(order));
        assert_eq!(
            c.answer(&item, Choice::Done, None),
            Err(Cause::NoService),
            "{order:?}"
        );
        let lost: Vec<halvo_coord::types::Event> = c.ports().lost.borrow_mut().drain(..).collect();
        c.ports().ports.events.borrow_mut().extend(lost);
        let m = mark(&c);
        assert_eq!(c.answer(&item, Choice::Decline, None), Ok(()), "{order:?}");
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
        assert_eq!(c2.answer(&item2, Choice::Done, None), Err(Cause::NoService));
        let lost: Vec<halvo_coord::types::Event> = c2.ports().lost.borrow_mut().drain(..).collect();
        c2.ports().ports.events.borrow_mut().extend(lost);
        let m = mark(&c2);
        assert_eq!(c2.answer(&item2, Choice::Done, None), Ok(()), "{order:?}");
        assert_eq!(handed_back(&c2, m), [HandbackResult::Done], "{order:?}");
        assert_eq!(step_rows(&c2, &item2), ["done"], "{order:?}");
        let c3 = coordinator(&[MANUAL], Limits::PRODUCT);
        let run = start(&c3, 0);
        c3.on_run(&run, handback(&run, 0, HandbackReason::AccountLink))
            .unwrap();
        let (item3, _) = open_item(&c3);
        c3.ports().loss.set(Some(order));
        assert_eq!(c3.answer(&item3, Choice::Done, None), Err(Cause::NoService));
        let lost: Vec<halvo_coord::types::Event> = c3.ports().lost.borrow_mut().drain(..).collect();
        c3.ports().ports.events.borrow_mut().extend(lost);
        c3.ports()
            .ports
            .refuse(Method::AnswerHandback, PortError::Busy);
        assert_eq!(
            c3.answer(&item3, Choice::Decline, None),
            Err(Cause::Busy),
            "{order:?}"
        );
        c3.ports().ports.clear_refusals();
        let m = mark(&c3);
        assert_eq!(
            c3.answer(&item3, Choice::Decline, None),
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
    for order in ORDERS {
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
    for order in ORDERS {
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
    for order in ORDERS {
        let c = coordinator(&[MANUAL, slotted, MANUAL], one);
        let (ready, kind) = open_item(&c);
        assert_eq!(kind, ItemKind::Ready);
        let first = start(&c, 0);
        switched_off_with_a_lost_acknowledgement(&c, 1, order);
        let starts = run_starts_of(&c, 1);
        let m = mark(&c);
        assert_eq!(
            c.answer(&ready, Choice::Start, None),
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
    for order in ORDERS {
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
    for order in ORDERS {
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
