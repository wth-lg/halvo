#[path = "coordinator_support.rs"]
mod support;

use halvo_coord::mem::{Call, Method};
use halvo_coord::ports::PortError;
use halvo_coord::types::commands::LIST;
use halvo_coord::types::{
    Action, Cause, HandbackAsk, HandbackReason, HoldSetCause, Limits, Name, RunEvent, RunId,
    Snapshot, Status, What,
};
use halvo_coord::{words, Coordinator};
use support::{agent, calls_since, coordinator, flow, mark, started, Both, DAY, MANUAL, T0};

fn text(key: &str) -> String {
    words::text(key, &[]).unwrap()
}

fn start(c: &Coordinator<Both>, n: usize) -> RunId {
    assert_eq!(c.control(&agent(n), Action::RunNow), Ok(()));
    let run = started(c).last().cloned().unwrap();
    assert_eq!(c.on_run(&run, RunEvent::Hello), Ok(()));
    run
}

fn deletes(c: &Coordinator<Both>) -> Vec<halvo_coord::types::WorkflowId> {
    c.ports().ports.deletes.borrow().clone()
}

fn row(s: &Snapshot, n: usize) -> Option<&halvo_coord::types::Row> {
    s.rows.iter().find(|r| r.agent == agent(n))
}

fn holds_off(c: &Coordinator<Both>, n: usize) -> bool {
    c.ports().ports.events.borrow().iter().any(|e| {
        e.agent == agent(n)
            && matches!(
                e.what,
                What::HoldSet {
                    cause: HoldSetCause::Person
                }
            )
    })
}

fn cancels(c: &Coordinator<Both>, from: usize) -> usize {
    calls_since(c, from)
        .iter()
        .filter(|call| matches!(call, Call::Cancel(_)))
        .count()
}

fn refused(name: &str) -> PortError {
    PortError::Refused(Name::parse(name).unwrap())
}

#[test]
fn agents_delete_is_a_row_of_the_command_list_with_the_agent_and_five_causes() {
    let row = LIST
        .iter()
        .find(|c| c.name == "agents_delete")
        .expect("agents_delete is a command of I8.1");
    let args: Vec<&str> = row.args.iter().map(|a| a.name).collect();
    assert_eq!(args, ["agent"]);
    assert_eq!(row.replies, ["reply-ok"]);
    assert_eq!(
        row.errors,
        [
            Cause::State,
            Cause::UnknownAgent,
            Cause::NoService,
            Cause::Busy,
            Cause::Malformed
        ]
    );
}

#[test]
fn a_delete_with_no_live_run_sends_the_decision_at_once_and_the_row_leaves() {
    let c = coordinator(&[MANUAL, MANUAL], Limits::PRODUCT);
    let seq = c.snapshot().seq;
    assert_eq!(c.delete(&agent(0)), Ok(()));
    assert_eq!(deletes(&c), [flow(0)]);
    let s = c.snapshot();
    assert!(row(&s, 0).is_none(), "the deleted agent has no row");
    assert!(row(&s, 1).is_some(), "the other agent keeps its row");
    assert!(s.seq > seq, "the state changed, so the page reads it anew");
    assert_eq!(
        c.control(&agent(0), Action::RunNow),
        Err(Cause::UnknownAgent)
    );
    assert_eq!(c.card(&agent(0)).map(|_| ()), Err(Cause::UnknownAgent));
    assert_eq!(c.delete(&agent(0)), Err(Cause::UnknownAgent));
    c.ports().ports.set_clock(T0 + 1_000, DAY);
    c.tick();
    assert!(row(&c.snapshot(), 0).is_none(), "a later tick shows no row");
    assert_eq!(deletes(&c), [flow(0)], "one decision");
}

#[test]
fn a_start_that_only_waits_its_turn_leaves_the_queue_and_does_not_withhold_the_delete() {
    let c = coordinator(&[MANUAL, MANUAL], Limits::PRODUCT);
    let first = start(&c, 0);
    assert_eq!(c.control(&agent(1), Action::RunNow), Ok(()));
    assert_eq!(c.queued().len(), 1, "agent 1 waits its turn");
    assert_eq!(c.delete(&agent(1)), Ok(()));
    assert_eq!(deletes(&c), [flow(1)]);
    assert!(c.queued().is_empty(), "the delete dropped the queued start");
    assert_eq!(c.on_run(&first, RunEvent::Ended(None)), Ok(()));
    c.ports().ports.set_clock(T0 + 1_000, DAY);
    c.tick();
    assert_eq!(started(&c).len(), 1, "nothing starts the deleted agent");
}

#[test]
fn a_delete_with_a_live_run_stores_the_hold_stops_the_run_and_sends_the_decision_at_its_stored_end()
{
    let c = coordinator(&[MANUAL, MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    let m = mark(&c);
    assert_eq!(c.delete(&agent(0)), Ok(()));
    assert!(holds_off(&c, 0), "the hold off is stored first");
    assert_eq!(
        cancels(&c, m),
        1,
        "the run is stopped as the switch-off stops it"
    );
    assert!(
        deletes(&c).is_empty(),
        "nothing is deleted while the run lives"
    );
    let s = c.snapshot();
    let r = row(&s, 0).expect("the row stays until the run has ended");
    assert_eq!(r.line, text("line.working.deleting"));
    assert_eq!(r.status, Status::Working);
    assert!(r.controls.is_empty(), "no control while Halvo deletes it");
    assert_eq!(r.next, None);
    assert_eq!(c.delete(&agent(0)), Ok(()), "a second yes changes nothing");
    assert_eq!(c.on_run(&run, RunEvent::Ended(None)), Ok(()));
    assert_eq!(
        deletes(&c),
        [flow(0)],
        "the decision goes once the end is stored"
    );
    assert!(row(&c.snapshot(), 0).is_none());
}

#[test]
fn a_delete_whose_end_store_is_refused_waits_until_a_tick_has_stored_it() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    assert_eq!(c.delete(&agent(0)), Ok(()));
    c.ports()
        .ports
        .refuse_record(|e| matches!(e.what, What::RunEnded { .. }), PortError::Busy);
    assert_eq!(c.on_run(&run, RunEvent::Ended(None)), Ok(()));
    assert!(
        deletes(&c).is_empty(),
        "with its run_ended owed the delete waits"
    );
    assert_eq!(
        row(&c.snapshot(), 0).map(|r| r.line.clone()),
        Some(text("line.working.deleting"))
    );
    c.ports().ports.clear_refusals();
    c.ports().ports.set_clock(T0 + 1_000, DAY);
    c.tick();
    assert_eq!(
        deletes(&c),
        [flow(0)],
        "the tick that pays the end sends it"
    );
    assert!(row(&c.snapshot(), 0).is_none());
}

#[test]
fn after_a_quit_in_between_nothing_is_deleted_and_the_agent_stands_switched_off() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    assert_eq!(c.delete(&agent(0)), Ok(()));
    let _ = c.quit_stop();
    assert_eq!(c.on_run(&run, RunEvent::Ended(None)), Ok(()));
    c.quit_kill();
    assert!(deletes(&c).is_empty(), "a quit forgets the intent");
    assert!(holds_off(&c, 0), "the hold off stands for the next life");
    let next = Coordinator::new(support::owner(&[], Limits::PRODUCT), Limits::PRODUCT);
    {
        let old = c.ports();
        let new = next.ports();
        *new.ports.workflows.borrow_mut() = old.ports.workflows.borrow().clone();
        *new.ports.agents.borrow_mut() = old.ports.agents.borrow().clone();
        *new.ports.events.borrow_mut() = old.ports.events.borrow().clone();
    }
    next.tick();
    let s = next.snapshot();
    let r = row(&s, 0).expect("the agent is still there");
    assert_eq!(r.status, Status::Off);
    let controls: Vec<&str> = r.controls.iter().map(|c| c.control.name()).collect();
    assert!(controls.contains(&"switch_on") && controls.contains(&"delete"));
    assert!(next.ports().ports.deletes.borrow().is_empty());
}

#[test]
fn a_delete_after_the_quits_first_step_answers_state_and_deletes_nothing() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let _ = c.quit_stop();
    assert_eq!(c.delete(&agent(0)), Err(Cause::State));
    assert!(deletes(&c).is_empty());
}

#[test]
fn a_delete_whose_hold_is_not_stored_does_nothing_and_answers_the_cause() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let _run = start(&c, 0);
    c.ports().ports.refuse(Method::Log, PortError::Busy);
    let m = mark(&c);
    assert_eq!(c.delete(&agent(0)), Err(Cause::Busy));
    assert_eq!(cancels(&c, m), 0, "no stop without the stored hold");
    c.ports().ports.clear_refusals();
    let s = c.snapshot();
    let r = row(&s, 0).unwrap();
    assert_ne!(r.line, text("line.working.deleting"));
    assert!(r.controls.iter().any(|c| c.control.name() == "delete"));
    assert!(deletes(&c).is_empty());
}

#[test]
fn the_owners_refusal_of_a_delete_answers_state_and_the_row_stays() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    c.ports().ports.refuse(Method::Delete, refused("state"));
    assert_eq!(c.delete(&agent(0)), Err(Cause::State));
    assert!(row(&c.snapshot(), 0).is_some());
    assert!(deletes(&c).is_empty());
}

#[test]
fn a_busy_or_unserved_owner_answers_busy_or_no_service_and_the_row_stays() {
    for (error, cause) in [
        (PortError::Busy, Cause::Busy),
        (PortError::NoService, Cause::NoService),
    ] {
        let c = coordinator(&[MANUAL], Limits::PRODUCT);
        c.ports().ports.refuse(Method::Delete, error);
        assert_eq!(c.delete(&agent(0)), Err(cause));
        assert!(row(&c.snapshot(), 0).is_some());
    }
}

#[test]
fn a_workflow_that_the_owner_no_longer_holds_counts_as_deleted() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    c.ports()
        .ports
        .refuse(Method::Delete, refused("no_workflow"));
    assert_eq!(c.delete(&agent(0)), Ok(()));
    assert!(row(&c.snapshot(), 0).is_none());
}

#[test]
fn an_agent_with_no_row_answers_unknown_agent_and_no_board_answers_no_service() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    assert_eq!(c.delete(&agent(5)), Err(Cause::UnknownAgent));
    let fresh = Coordinator::new(support::owner(&[MANUAL], Limits::PRODUCT), Limits::PRODUCT);
    assert_eq!(fresh.delete(&agent(0)), Err(Cause::NoService));
}

#[test]
fn the_owner_of_the_memory_ports_refuses_a_delete_while_a_run_of_the_agent_is_live() {
    use halvo_coord::ports::Owner;
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    assert_eq!(c.ports().ports.delete(&flow(0)), Err(refused("state")));
    assert_eq!(c.control(&agent(0), Action::Stop), Ok(()));
    assert_eq!(c.on_run(&run, RunEvent::Ended(None)), Ok(()));
    assert_eq!(c.ports().ports.delete(&flow(0)), Ok(()));
    assert_eq!(
        c.ports().ports.delete(&flow(0)),
        Err(refused("no_workflow"))
    );
}

fn handback(run: &RunId, n: usize) -> RunEvent {
    RunEvent::Handback(HandbackAsk {
        run: run.clone(),
        agent: agent(n),
        reason: HandbackReason::StepFailed,
        origin: None,
    })
}

fn is_item_raise(e: &halvo_coord::types::Event) -> bool {
    matches!(e.what, What::ItemRaised { .. })
}

#[test]
fn rd64_a_delete_of_a_stuck_record_stops_the_run_as_stop_does_and_is_sent_when_it_left_the_board() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    c.ports()
        .ports
        .refuse_record(is_item_raise, refused("store"));
    let _ = c.on_run(&run, handback(&run, 0));
    for k in 1..=3u64 {
        c.ports().ports.set_clock(T0 + k * 1_000, DAY);
        c.tick();
    }
    assert!(c.record_stuck(&agent(0)), "the record is stuck");
    let m = mark(&c);
    assert_eq!(c.delete(&agent(0)), Ok(()));
    assert!(!holds_off(&c, 0), "no row of the hold for a stuck record");
    assert_eq!(cancels(&c, m), 1, "the run is stopped as Stop stops it");
    assert!(deletes(&c).is_empty());
    assert_eq!(
        row(&c.snapshot(), 0).map(|r| r.line.clone()),
        Some(text("line.working.deleting"))
    );
    assert_eq!(c.on_run(&run, RunEvent::Ended(None)), Ok(()));
    assert_eq!(deletes(&c), [flow(0)]);
    assert!(row(&c.snapshot(), 0).is_none());
}

#[test]
fn a_delete_whose_run_a_tick_kills_after_the_grace_meets_the_owners_answer_at_that_tick() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let _run = start(&c, 0);
    assert_eq!(c.delete(&agent(0)), Ok(()));
    c.ports().ports.refuse(Method::Delete, PortError::Busy);
    let grace = Limits::PRODUCT.cancel_grace_s * 1_000;
    c.ports().ports.set_clock(T0 + grace + 1_000, DAY);
    c.tick();
    assert_eq!(c.live(), 0, "the tick killed the run after its grace");
    assert!(
        deletes(&c).is_empty(),
        "the owner answered busy, so nothing is deleted"
    );
    assert_eq!(
        row(&c.snapshot(), 0).map(|r| r.line.clone()),
        Some(text("line.working.deleting")),
        "the intent stays for the next move"
    );
    c.ports().ports.clear_refusals();
    c.ports().ports.set_clock(T0 + grace + 2_000, DAY);
    c.tick();
    assert_eq!(
        deletes(&c),
        [flow(0)],
        "the next tick meets the owner, not the default"
    );
    assert!(row(&c.snapshot(), 0).is_none());
}

/// The named limit of the guard (the coordinator's word of 2026-11-21, its point (4)): a start
/// whose acknowledgement was lost and that this life has not read makes the owner refuse the
/// delete with `state`; the yes changes nothing and the row reads as before, with Delete.
#[test]
fn a_delete_that_a_lost_start_blocks_answers_state_and_the_row_reads_as_before() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let before = c.snapshot();
    let mine = row(&before, 0).cloned().unwrap();
    {
        let mut events = c.ports().ports.events.borrow_mut();
        let seq = events.iter().filter(|e| e.agent == agent(0)).count() as u32 + 1;
        let lost = halvo_coord::types::Event {
            workflow: flow(0),
            agent: agent(0),
            seq: std::num::NonZeroU32::new(seq).unwrap(),
            day: DAY,
            at: i64::try_from(T0).unwrap() * 1_000_000,
            what: What::RunStarted {
                run: RunId::parse(&format!("{:032x}", 0x5151)).unwrap(),
                trigger: halvo_coord::types::Trigger::Person,
                slot: None,
            },
        };
        events.push(lost);
    }
    assert_eq!(c.delete(&agent(0)), Err(Cause::State));
    assert!(deletes(&c).is_empty());
    let after = c.snapshot();
    let r = row(&after, 0).unwrap();
    assert_eq!(r.line, mine.line, "the row reads as before");
    assert_eq!(r.status, mine.status);
    assert_eq!(r.controls, mine.controls, "Delete stands on the row again");
}
