#[path = "coordinator_support.rs"]
mod coord;

use coord::{agent, flow, Both, A, DAY, MANUAL, T0};
use halvo_coord::mem::{Call, Method};
use halvo_coord::ports::{PortError, Reaped, Rng};
use halvo_coord::run::recover;
use halvo_coord::types::{
    Action, Body, Category, Cause, Control, DismissedWorkflow, Event, Limits, Name, Outcome,
    PastRun, Row, RunEnd, RunEvent, RunHistory, RunId, Status, Trigger, What, Workflow,
};
use halvo_coord::{words, Coordinator};
use std::num::NonZeroU32;

/// The line of a stuck kill on the system that runs the test, in U2.4r's words.
fn kill_stuck() -> String {
    let key = if cfg!(windows) {
        "line.working.kill_stuck_windows"
    } else {
        "line.working.kill_stuck_mac"
    };
    words::text(key, &[]).unwrap()
}

fn text(key: &str) -> String {
    words::text(key, &[]).unwrap()
}

fn room() -> Limits {
    Limits {
        max_live_runs: 3,
        ..Limits::PRODUCT
    }
}

/// A first life of the app over `owner` in which the person starts a run of each agent, and a
/// Quit whose kill the recorder refuses, so that every run outlives Quit: no real kill runs.
fn outlived_quit(owner: Both) -> (Coordinator<Both>, Vec<RunId>) {
    let n = owner.ports.agents.borrow().len();
    let c = Coordinator::new(owner, room());
    c.tick();
    let runs: Vec<RunId> = (0..n)
        .map(|i| {
            assert_eq!(c.control(&agent(i), Action::RunNow), Ok(()));
            let run = coord::started(&c).last().cloned().unwrap();
            assert_eq!(c.on_run(&run, RunEvent::Hello), Ok(()));
            run
        })
        .collect();
    c.quit_stop();
    c.ports().ports.refuse(Method::Kill, PortError::NoService);
    c.quit_kill();
    assert_eq!(c.live(), n, "the recorder refused every kill");
    (c, runs)
}

/// The owner of the next life: the store of the first life, and the marks that the supervisor
/// library left, each with the answer that `Runs::reap` gives for it.
fn next_life(c: &Coordinator<Both>, marks: &[(RunId, Reaped)]) -> Both {
    let next = coord::owner(&[], Limits::PRODUCT);
    let first = &c.ports().ports;
    next.ports
        .workflows
        .borrow_mut()
        .clone_from(&first.workflows.borrow());
    next.ports
        .agents
        .borrow_mut()
        .clone_from(&first.agents.borrow());
    next.ports
        .events
        .borrow_mut()
        .clone_from(&first.events.borrow());
    next.ports.leave_marks(marks);
    next
}

fn row(c: &Coordinator<Both>, n: usize) -> Row {
    c.snapshot()
        .rows
        .into_iter()
        .find(|r| r.agent == agent(n))
        .unwrap()
}

fn tick_at(c: &Coordinator<Both>, k: u64) {
    c.ports().ports.set_now(T0 + k * 1_000);
    c.tick();
}

#[test]
fn u46b_recover_keeps_on_the_board_the_run_that_reap_answers_alive() {
    let (c, runs) = outlived_quit(coord::owner(&[MANUAL, MANUAL], room()));
    let next = next_life(
        &c,
        &[
            (runs[0].clone(), Reaped::Alive),
            (runs[1].clone(), Reaped::Gone),
        ],
    );
    let life = recover(&next, &Limits::PRODUCT).expect("recovery");
    let ended: Vec<(RunId, RunEnd)> = life
        .ended
        .iter()
        .map(|e| (e.run.clone(), e.cause))
        .collect();
    assert_eq!(
        ended,
        [
            (runs[0].clone(), RunEnd::Interrupted),
            (runs[1].clone(), RunEnd::Interrupted)
        ],
        "recovery ends both runs interrupted, whatever reap answered"
    );
    assert_eq!(
        life.board.alive_at_reap(&agent(0)),
        Some(&runs[0]),
        "the board keeps the run that reap answered alive"
    );
    assert_eq!(
        life.board.alive_at_reap(&agent(1)),
        None,
        "the board keeps no run that reap answered gone"
    );
}

#[test]
fn u46b_the_first_row_of_that_agent_in_the_next_life_reads_that_its_task_may_still_run() {
    let (c, runs) = outlived_quit(coord::owner(&[MANUAL, MANUAL], room()));
    let life = Coordinator::new(
        next_life(
            &c,
            &[
                (runs[0].clone(), Reaped::Alive),
                (runs[1].clone(), Reaped::Gone),
            ],
        ),
        Limits::PRODUCT,
    );
    life.tick();
    let first = row(&life, 0);
    assert_eq!(
        (first.status, first.line),
        (Status::Working, kill_stuck()),
        "the run that reap answered alive may still run"
    );
    let other = row(&life, 1);
    assert_ne!(
        other.line,
        kill_stuck(),
        "a run that reap answered gone does not run"
    );
}

#[test]
fn u46b_a_switch_off_keeps_the_line_and_a_record_of_the_store_that_names_the_run_ends_it() {
    let (c, runs) = outlived_quit(coord::owner(&[MANUAL], room()));
    let life = Coordinator::new(
        next_life(&c, &[(runs[0].clone(), Reaped::Alive)]),
        Limits::PRODUCT,
    );
    life.tick();
    assert_eq!(row(&life, 0).line, kill_stuck());
    assert_eq!(life.control(&agent(0), Action::SwitchOff), Ok(()));
    tick_at(&life, 1);
    assert_eq!(
        row(&life, 0).line,
        kill_stuck(),
        "a hold that stops new starts does not stop the run that may still run"
    );
    life.ports().ports.agents.borrow_mut()[0].last = Some(PastRun {
        run: runs[0].clone(),
        day: coord::DAY,
        outcome: Outcome::Failed,
        unknown: 0,
    });
    tick_at(&life, 2);
    assert_eq!(
        row(&life, 0).line,
        text("line.off.person"),
        "once the store has ended the run, the row reads its own line again"
    );
}

#[test]
fn u46b_an_agent_with_no_clean_run_shows_no_ready_card_while_its_run_may_still_run() {
    let owner = coord::owner(&[MANUAL], room());
    for e in owner.ports.events.borrow_mut().iter_mut() {
        if let What::RunEnded { cause, .. } = &mut e.what {
            *cause = RunEnd::Failed;
        }
    }
    if let Some(last) = owner.ports.agents.borrow_mut()[0].last.as_mut() {
        last.outcome = Outcome::Failed;
    }
    let (c, runs) = outlived_quit(owner);
    let life = Coordinator::new(
        next_life(&c, &[(runs[0].clone(), Reaped::Alive)]),
        Limits::PRODUCT,
    );
    life.tick();
    assert!(
        life.ports().ports.events.borrow().iter().any(|e| matches!(
            e.what,
            What::ItemRaised {
                raised: halvo_coord::types::Raised::Ready,
                ..
            }
        )),
        "the tick raised ready for an agent with no clean run"
    );
    let s = life.snapshot();
    let first = s.rows.iter().find(|r| r.agent == agent(0)).unwrap();
    assert_eq!(
        (first.status, first.line.clone()),
        (Status::Working, kill_stuck())
    );
    assert!(
        s.items.iter().all(|i| i.agent() != &agent(0)),
        "no card invites a start while the run may still run"
    );
}

#[test]
fn u46b_a_stuck_record_keeps_its_line_before_the_line_of_a_run_that_may_still_run() {
    let (c, runs) = outlived_quit(coord::owner(&[MANUAL], room()));
    let next = next_life(&c, &[(runs[0].clone(), Reaped::Alive)]);
    next.ports.refuse_agent(
        Method::Log,
        &agent(0),
        PortError::Refused(Name::parse("store").unwrap()),
    );
    let life = Coordinator::new(next, Limits::PRODUCT);
    life.tick();
    for k in 1..=3 {
        tick_at(&life, k);
    }
    assert!(life.record_stuck(&agent(0)));
    let first = row(&life, 0);
    assert_eq!(
        (first.status, first.line),
        (Status::Problem, text("line.problem.record"))
    );
}

#[test]
fn u46b_an_agent_set_aside_at_recover_reads_so_once_its_history_answers() {
    let (c, runs) = outlived_quit(coord::owner(&[MANUAL], room()));
    let next = next_life(&c, &[(runs[0].clone(), Reaped::Alive)]);
    next.ports.refuse_agent(
        Method::History,
        &agent(0),
        PortError::Refused(Name::parse("store").unwrap()),
    );
    let life = Coordinator::new(next, Limits::PRODUCT);
    life.tick();
    assert_eq!(row(&life, 0).line, text("line.problem.record"));
    life.ports().ports.clear_refusals();
    tick_at(&life, 1);
    let first = row(&life, 0);
    assert_eq!(
        (first.status, first.line),
        (Status::Working, kill_stuck()),
        "the tick that makes the agent's recovery reads the run that reap answered alive"
    );
}

#[test]
fn u46b_the_board_keeps_every_run_that_reap_answers_alive_whatever_its_place_among_the_answers() {
    let (c, runs) = outlived_quit(coord::owner(&[MANUAL, MANUAL, MANUAL], room()));
    let next = next_life(
        &c,
        &[
            (runs[1].clone(), Reaped::Gone),
            (runs[0].clone(), Reaped::Alive),
            (runs[2].clone(), Reaped::Alive),
        ],
    );
    let life = recover(&next, &Limits::PRODUCT).expect("recovery");
    let kept: Vec<Option<&RunId>> = (0..3)
        .map(|n| life.board.alive_at_reap(&agent(n)))
        .collect();
    assert_eq!(kept, [Some(&runs[0]), None, Some(&runs[2])]);
}

#[test]
fn u46b_the_line_stands_while_the_rows_of_recovery_are_owed() {
    let (c, runs) = outlived_quit(coord::owner(&[MANUAL], room()));
    let next = next_life(&c, &[(runs[0].clone(), Reaped::Alive)]);
    next.ports.refuse(Method::Log, PortError::NoService);
    let life = Coordinator::new(next, Limits::PRODUCT);
    life.tick();
    assert!(
        !life.ports().ports.events.borrow().iter().any(|e| matches!(
            &e.what,
            What::RunEnded { run, .. } if *run == runs[0]
        )),
        "the owner took no run_ended of the run"
    );
    let first = row(&life, 0);
    assert_eq!((first.status, first.line), (Status::Working, kill_stuck()));
}

#[test]
fn u46b_a_record_of_the_store_that_names_the_run_ends_the_line_while_mail_replies_go_unread() {
    let (c, runs) = outlived_quit(coord::owner(&[MANUAL], room()));
    let next = next_life(&c, &[(runs[0].clone(), Reaped::Alive)]);
    if let Workflow::Full(w) = &mut next.ports.workflows.borrow_mut()[0] {
        w.category = Category::Email;
    }
    next.ports.agents.borrow_mut()[0].last = Some(PastRun {
        run: runs[0].clone(),
        day: coord::DAY,
        outcome: Outcome::Failed,
        unknown: 0,
    });
    next.ports.refuse(Method::Replies, PortError::NoService);
    let life = Coordinator::new(next, Limits::PRODUCT);
    life.tick();
    assert_eq!(
        row(&life, 0).line,
        text("line.needs_you.setup"),
        "the store has ended the run, so the row reads its own line, unread replies or not"
    );
}

const DAY_MS: u64 = 86_400_000;

fn row_of(c: &Coordinator<Both>, n: usize) -> Option<Row> {
    c.snapshot().rows.into_iter().find(|r| r.agent == agent(n))
}

fn controls(row: &Row) -> Vec<Control> {
    row.controls.iter().map(|c| c.control).collect()
}

fn seen(c: &Coordinator<Both>, n: usize) -> Option<(Status, String, Vec<Control>)> {
    row_of(c, n).map(|r| (r.status, r.line.clone(), controls(&r)))
}

fn end_runs(c: &Coordinator<Both>) -> Vec<RunId> {
    c.ports()
        .ports
        .calls
        .borrow()
        .iter()
        .filter_map(|t| match &t.call {
            Call::Send(Body::EndRun { run }) => Some(run.clone()),
            _ => None,
        })
        .collect()
}

fn attention(c: &Coordinator<Both>) -> Option<u32> {
    c.ports()
        .ports
        .calls
        .borrow()
        .iter()
        .rev()
        .find_map(|t| match t.call {
            Call::Attention(n) => Some(n),
            _ => None,
        })
}

fn record(run: &RunId, outcome: Outcome) -> PastRun {
    PastRun {
        run: run.clone(),
        day: DAY,
        outcome,
        unknown: 0,
    }
}

/// The owner of a next life in which reap answers `alive` for `run`, with the library's ids
/// moved on, so that a run that this life starts has an id of its own.
fn alive_next_life(c: &Coordinator<Both>, run: &RunId) -> Both {
    let next = next_life(c, &[(run.clone(), Reaped::Alive)]);
    for _ in 0..16 {
        let _ = next.ports.bytes16();
    }
    next
}

/// The store takes `done` as the agent's last record, and its history leads with it.
fn store_ends(c: &Coordinator<Both>, n: usize, done: &PastRun) {
    c.ports().ports.store_end(&agent(n), done.clone());
    c.ports().ports.histories.borrow_mut().insert(
        0,
        (
            agent(n),
            RunHistory {
                run: done.clone(),
                effects: Vec::new(),
            },
        ),
    );
}

#[test]
fn u46b_next_life_shows_a_gone_workflows_run_that_reap_answers_alive() {
    let (c, runs) = outlived_quit(coord::owner(&[MANUAL], room()));
    let next = next_life(&c, &[(runs[0].clone(), Reaped::Alive)]);
    next.ports.workflows.borrow_mut().clear();
    let life = Coordinator::new(next, Limits::PRODUCT);
    life.tick();
    assert_eq!(
        seen(&life, 0),
        Some((Status::Working, kill_stuck(), Vec::new())),
        "the workflow left the owner's list between the two lives"
    );
    assert_eq!(
        life.control(&agent(0), Action::RunNow),
        Err(Cause::UnknownAgent),
        "the agent of a gone workflow is no member, as I5.11's gone_outlives_app has it"
    );
    tick_at(&life, 1);
    assert_eq!(seen(&life, 0).map(|s| s.1), Some(kill_stuck()));
    assert!(
        !end_runs(&life).contains(&runs[0]),
        "nothing is sent for it"
    );
    store_ends(&life, 0, &record(&runs[0], Outcome::Failed));
    let later = Coordinator::new(
        next_life(&life, &[(runs[0].clone(), Reaped::Alive)]),
        Limits::PRODUCT,
    );
    later.tick();
    assert_eq!(
        seen(&later, 0),
        None,
        "a record of the store that names the run, read at the next recovery, ends the row"
    );

    let (c, runs) = outlived_quit(coord::owner(&[MANUAL], room()));
    let life = Coordinator::new(
        next_life(&c, &[(runs[0].clone(), Reaped::Alive)]),
        Limits::PRODUCT,
    );
    life.tick();
    assert_eq!(row(&life, 0).line, kill_stuck());
    life.ports().ports.workflows.borrow_mut().clear();
    tick_at(&life, 1);
    assert_eq!(
        seen(&life, 0),
        Some((Status::Working, kill_stuck(), Vec::new())),
        "the workflow left the owner's list after the next life's first tick"
    );

    let (c, runs) = outlived_quit(coord::owner(&[MANUAL, MANUAL], room()));
    let next = next_life(
        &c,
        &[
            (runs[0].clone(), Reaped::Gone),
            (runs[1].clone(), Reaped::Alive),
        ],
    );
    next.ports.workflows.borrow_mut().clear();
    let life = Coordinator::new(next, Limits::PRODUCT);
    life.tick();
    assert_eq!(
        seen(&life, 0),
        None,
        "nothing of a gone workflow whose run reap answered gone may still run"
    );
    assert_eq!(
        seen(&life, 1).map(|s| s.1),
        Some(kill_stuck()),
        "beside it, the gone workflow whose run reap answered alive keeps its row"
    );
}

/// A refused press, the scheduler and a dismissed workflow, each in a state in which nothing
/// owes the run's end: none of them sends `end_run` for the run that reap answered `alive`.
#[test]
fn u46b_no_end_run_for_a_run_that_reap_answered_alive_at_a_refused_start_now() {
    let malformed = || PortError::Refused(Name::parse("malformed").unwrap());

    let (c, runs) = outlived_quit(coord::owner(&[MANUAL], room()));
    let next = alive_next_life(&c, &runs[0]);
    next.ports
        .refuse_record(|e| matches!(e.what, What::RunEnded { .. }), malformed());
    let life = Coordinator::new(next, Limits::PRODUCT);
    life.tick();
    life.ports().ports.set_clock(T0 - 3 * DAY_MS, DAY - 3);
    life.tick();
    let press = life.control(&agent(0), Action::RunNow);
    life.ports().ports.clear_refusals();
    life.ports().ports.set_clock(T0 + 1_000, DAY);
    life.tick();
    assert!(!end_runs(&life).contains(&runs[0]), "a press: end_run");
    assert_eq!(row(&life, 0).line, kill_stuck(), "a press: the row");
    let second = life.control(&agent(0), Action::RunNow);
    assert_eq!(
        (press.is_err(), second.is_err()),
        (true, true),
        "a press: {press:?} {second:?}"
    );
    assert_eq!(
        coord::started(&life),
        Vec::<RunId>::new(),
        "a press: a start"
    );

    let slotted = A {
        person: false,
        slotted: true,
    };
    let c = Coordinator::new(coord::owner(&[slotted], room()), room());
    c.tick();
    let r1 = coord::started(&c)
        .last()
        .cloned()
        .expect("the scheduler's start");
    assert_eq!(c.on_run(&r1, RunEvent::Hello), Ok(()));
    c.quit_stop();
    c.ports().ports.refuse(Method::Kill, PortError::NoService);
    c.quit_kill();
    let next = alive_next_life(&c, &r1);
    next.ports
        .refuse_record(|e| matches!(e.what, What::RunEnded { .. }), malformed());
    let life = Coordinator::new(next, Limits::PRODUCT);
    life.tick();
    life.ports().ports.set_clock(T0 + 94 * DAY_MS, DAY + 94);
    life.tick();
    life.ports().ports.clear_refusals();
    life.ports()
        .ports
        .set_clock(T0 + 94 * DAY_MS + 1_000, DAY + 94);
    life.tick();
    assert!(!end_runs(&life).contains(&r1), "the scheduler: end_run");
    assert_eq!(row(&life, 0).line, kill_stuck(), "the scheduler: the row");
    assert_eq!(
        coord::started(&life),
        Vec::<RunId>::new(),
        "the scheduler: a start"
    );

    let (c, runs) = outlived_quit(coord::owner(&[MANUAL], room()));
    let next = alive_next_life(&c, &runs[0]);
    {
        let mut listed = next.ports.workflows.borrow_mut();
        if let Workflow::Full(f) = listed[0].clone() {
            listed[0] = Workflow::Dismissed(DismissedWorkflow {
                id: f.id,
                category: f.category,
                site: f.site,
            });
        }
    }
    next.ports.refuse_agent(
        Method::Log,
        &agent(0),
        PortError::Refused(Name::parse("binding").unwrap()),
    );
    let life = Coordinator::new(next, Limits::PRODUCT);
    life.tick();
    let press = life.control(&agent(0), Action::RunNow);
    tick_at(&life, 1);
    assert!(press.is_err(), "a dismissed workflow: {press:?}");
    assert!(
        !end_runs(&life).contains(&runs[0]),
        "a dismissed workflow: end_run"
    );
    assert_eq!(
        row(&life, 0).line,
        kill_stuck(),
        "a dismissed workflow: the row"
    );
    assert_eq!(
        coord::started(&life),
        Vec::<RunId>::new(),
        "a dismissed workflow: a start"
    );
}

#[test]
fn u46b_a_run_that_outlived_quit_and_completed_reads_finished() {
    let finished = coord::coordinator(&[MANUAL], Limits::PRODUCT);
    let finished_controls = controls(&row(&finished, 0));
    assert_eq!(row(&finished, 0).line, text("line.all_set.done"));

    let (c, runs) = outlived_quit(coord::owner(&[MANUAL], room()));
    let life = Coordinator::new(
        next_life(&c, &[(runs[0].clone(), Reaped::Alive)]),
        Limits::PRODUCT,
    );
    life.tick();
    assert_eq!(row(&life, 0).line, kill_stuck());
    store_ends(&life, 0, &record(&runs[0], Outcome::Completed));
    tick_at(&life, 1);
    let r = row(&life, 0);
    assert_eq!(
        (r.status, r.line.clone()),
        (Status::AllSet, text("line.all_set.done")),
        "the store's record says that the task was done"
    );
    assert_ne!(r.line, text("line.all_set.stopped_early"));
    assert_eq!(controls(&r), finished_controls);
    let card = life.card(&agent(0)).expect("the card");
    assert_eq!(
        card.runs.first().map(|run| run.text.clone()),
        Some(text("line.run.completed"))
    );
    assert_ne!(card.runs[0].text, text("line.run.interrupted"));

    let (c, runs) = outlived_quit(coord::owner(&[MANUAL], room()));
    let life = Coordinator::new(
        next_life(&c, &[(runs[0].clone(), Reaped::Alive)]),
        Limits::PRODUCT,
    );
    life.tick();
    store_ends(&life, 0, &record(&runs[0], Outcome::Failed));
    tick_at(&life, 1);
    assert_eq!(
        row(&life, 0).line,
        text("line.problem.failed"),
        "the end that the store's record holds"
    );
    assert_eq!(
        life.card(&agent(0)).expect("the card").runs[0].text,
        text("line.run.failed")
    );

    let (c, runs) = outlived_quit(coord::owner(&[MANUAL], room()));
    let life = Coordinator::new(
        next_life(&c, &[(runs[0].clone(), Reaped::Gone)]),
        Limits::PRODUCT,
    );
    life.tick();
    store_ends(&life, 0, &record(&runs[0], Outcome::Completed));
    tick_at(&life, 1);
    assert_eq!(
        row(&life, 0).line,
        text("line.all_set.stopped_early"),
        "a run that reap answered gone reads as before"
    );
    assert_eq!(
        life.card(&agent(0)).expect("the card").runs[0].text,
        text("line.run.interrupted")
    );
}

#[test]
fn u46b_a_mail_agent_with_its_setup_open_shows_the_line_while_its_run_may_still_run() {
    let owner = coord::owner(&[MANUAL], room());
    for e in owner.ports.events.borrow_mut().iter_mut() {
        if let What::RunEnded { cause, .. } = &mut e.what {
            *cause = RunEnd::Failed;
        }
    }
    if let Some(last) = owner.ports.agents.borrow_mut()[0].last.as_mut() {
        last.outcome = Outcome::Failed;
    }
    let (c, runs) = outlived_quit(owner);
    let next = next_life(&c, &[(runs[0].clone(), Reaped::Alive)]);
    if let Workflow::Full(w) = &mut next.ports.workflows.borrow_mut()[0] {
        w.category = Category::Email;
    }
    let life = Coordinator::new(next, Limits::PRODUCT);
    life.tick();
    let s = life.snapshot();
    let first = s.rows.iter().find(|r| r.agent == agent(0)).unwrap();
    assert_eq!(
        (first.status, first.line.clone()),
        (Status::Working, kill_stuck())
    );
    assert!(
        s.items.is_empty(),
        "no setup card while the run may still run"
    );
    assert_eq!(attention(&life), Some(0));
}

#[test]
fn u46b_a_later_record_of_another_writer_does_not_bring_the_line_back() {
    let (c, runs) = outlived_quit(coord::owner(&[MANUAL], room()));
    let life = Coordinator::new(
        next_life(&c, &[(runs[0].clone(), Reaped::Alive)]),
        Limits::PRODUCT,
    );
    life.tick();
    assert_eq!(row(&life, 0).line, kill_stuck());
    store_ends(&life, 0, &record(&runs[0], Outcome::Completed));
    tick_at(&life, 1);
    assert_ne!(
        row(&life, 0).line,
        kill_stuck(),
        "r1's record ends the line"
    );
    let r2 = RunId::parse("00000000000000000000000000009999").unwrap();
    {
        let mut events = life.ports().ports.events.borrow_mut();
        let last = events
            .iter()
            .filter(|e| e.agent == agent(0))
            .map(|e| e.seq.get())
            .max()
            .unwrap_or(0);
        let event = |seq: u32, what: What| Event {
            workflow: flow(0),
            agent: agent(0),
            seq: NonZeroU32::new(seq).unwrap(),
            day: DAY,
            at: 0,
            what,
        };
        events.push(event(
            last + 1,
            What::RunStarted {
                run: r2.clone(),
                trigger: Trigger::Person,
                slot: None,
            },
        ));
        events.push(event(
            last + 2,
            What::RunEnded {
                run: r2.clone(),
                cause: RunEnd::Completed,
                defers: None,
            },
        ));
    }
    life.ports()
        .ports
        .store_end(&agent(0), record(&r2, Outcome::Completed));
    tick_at(&life, 2);
    assert_ne!(
        row(&life, 0).line,
        kill_stuck(),
        "a run that this life did not start, recorded after r1, brings no line back"
    );
}
