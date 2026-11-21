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
    store_ends(&life, 0, &record(&runs[0], Outcome::Failed));
    tick_at(&life, 1);
    assert_eq!(
        row(&life, 0).line,
        text("line.all_set.stopped_early"),
        "a record Failed of a run that reap answered gone reads as before"
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

/// Pushes into the owner's ledger a run of agent `n` that this life did not start, started and
/// ended `completed`, after the agent's last event, and gives the store its record.
fn another_writers_run(c: &Coordinator<Both>, n: usize, run: &RunId) {
    {
        let mut events = c.ports().ports.events.borrow_mut();
        let last = events
            .iter()
            .filter(|e| e.agent == agent(n))
            .map(|e| e.seq.get())
            .max()
            .unwrap_or(0);
        let event = |seq: u32, what: What| Event {
            workflow: flow(n),
            agent: agent(n),
            seq: NonZeroU32::new(seq).unwrap(),
            day: DAY,
            at: 0,
            what,
        };
        events.push(event(
            last + 1,
            What::RunStarted {
                run: run.clone(),
                trigger: Trigger::Person,
                slot: None,
            },
        ));
        events.push(event(
            last + 2,
            What::RunEnded {
                run: run.clone(),
                cause: RunEnd::Completed,
                defers: None,
            },
        ));
    }
    c.ports()
        .ports
        .store_end(&agent(n), record(run, Outcome::Completed));
}

/// The controls of the row of a task that ended in the ordinary way.
fn finished_controls() -> Vec<Control> {
    let finished = coord::coordinator(&[MANUAL], Limits::PRODUCT);
    assert_eq!(row(&finished, 0).line, text("line.all_set.done"));
    controls(&row(&finished, 0))
}

#[test]
fn u46b_a_gone_workflows_row_survives_a_history_read_that_fails_at_recover() {
    for failure in [
        PortError::Busy,
        PortError::NoService,
        PortError::Refused(Name::parse("store").unwrap()),
    ] {
        let (c, runs) = outlived_quit(coord::owner(&[MANUAL], room()));
        let next = alive_next_life(&c, &runs[0]);
        next.ports.workflows.borrow_mut().clear();
        next.ports
            .refuse_agent(Method::History, &agent(0), failure.clone());
        let life = Coordinator::new(next, Limits::PRODUCT);
        life.tick();
        life.ports().ports.clear_refusals();
        for k in 1..=3 {
            tick_at(&life, k);
        }
        assert_eq!(
            seen(&life, 0),
            Some((Status::Working, kill_stuck(), Vec::new())),
            "the first read of the history answered {failure:?}"
        );
        assert!(!end_runs(&life).contains(&runs[0]), "{failure:?}: end_run");
    }
}

/// Recovery's `run_ended` of the run that reap answered alive never reached the ledger (refused
/// `malformed` while the clock stood 3 days back), so the ledger's last end is an earlier run's.
#[test]
fn u46b_the_row_reads_the_end_of_that_run_where_recoverys_run_ended_never_landed() {
    for (earlier, end, line, card) in [
        (
            Outcome::NothingToDo,
            Outcome::Completed,
            "line.all_set.done",
            "line.run.completed",
        ),
        (
            Outcome::Completed,
            Outcome::Failed,
            "line.problem.failed",
            "line.run.failed",
        ),
        (
            Outcome::Completed,
            Outcome::NothingToDo,
            "line.all_set.nothing",
            "line.run.nothing_to_do",
        ),
    ] {
        let owner = coord::owner(&[MANUAL], room());
        for e in owner.ports.events.borrow_mut().iter_mut() {
            if let What::RunEnded { cause, .. } = &mut e.what {
                *cause = RunEnd::from(earlier);
            }
        }
        if let Some(last) = owner.ports.agents.borrow_mut()[0].last.as_mut() {
            last.outcome = earlier;
        }
        let (c, runs) = outlived_quit(owner);
        let next = alive_next_life(&c, &runs[0]);
        next.ports.refuse_record(
            |e| matches!(e.what, What::RunEnded { .. }),
            PortError::Refused(Name::parse("malformed").unwrap()),
        );
        let life = Coordinator::new(next, Limits::PRODUCT);
        life.tick();
        life.ports().ports.set_clock(T0 - 3 * DAY_MS, DAY - 3);
        life.tick();
        life.ports().ports.clear_refusals();
        store_ends(&life, 0, &record(&runs[0], end));
        life.ports().ports.set_clock(T0 + 1_000, DAY);
        for k in 1..=3 {
            tick_at(&life, k);
        }
        assert_eq!(
            row(&life, 0).line,
            text(line),
            "an earlier {earlier:?}, the record {end:?}"
        );
        assert_eq!(
            life.card(&agent(0)).expect("the card").runs[0].text,
            text(card),
            "an earlier {earlier:?}, the record {end:?}: the card"
        );
    }
}

#[test]
fn u46b_the_line_does_not_come_back_once_a_record_named_the_run() {
    let (c, runs) = outlived_quit(coord::owner(&[MANUAL], room()));
    let life = Coordinator::new(alive_next_life(&c, &runs[0]), Limits::PRODUCT);
    life.tick();
    store_ends(&life, 0, &record(&runs[0], Outcome::Completed));
    tick_at(&life, 1);
    assert_eq!(row(&life, 0).line, text("line.all_set.done"));
    for k in 0..Limits::PRODUCT.history_runs {
        let run = RunId::parse(&format!("{:032x}", 0x333 + k)).unwrap();
        another_writers_run(&life, 0, &run);
        tick_at(&life, 2 + u64::from(k));
        assert_ne!(
            row(&life, 0).line,
            kill_stuck(),
            "after {} more records of the store",
            k + 1
        );
    }
    assert_ne!(
        row(&life, 0).line,
        text("line.all_set.stopped_early"),
        "the run that the record named completed never reads as cut short"
    );
}

#[test]
fn u46b_a_later_life_reads_a_run_that_completed_after_reap_answered_it_alive_as_finished() {
    let finished = finished_controls();
    for third in [vec![Reaped::Gone], Vec::new()] {
        let (c, runs) = outlived_quit(coord::owner(&[MANUAL], room()));
        let life = Coordinator::new(
            next_life(&c, &[(runs[0].clone(), Reaped::Alive)]),
            Limits::PRODUCT,
        );
        life.tick();
        store_ends(&life, 0, &record(&runs[0], Outcome::Completed));
        tick_at(&life, 1);
        assert_eq!(row(&life, 0).line, text("line.all_set.done"));
        let marks: Vec<(RunId, Reaped)> = third.iter().map(|a| (runs[0].clone(), *a)).collect();
        let owner = next_life(&life, &marks);
        owner
            .ports
            .histories
            .borrow_mut()
            .clone_from(&life.ports().ports.histories.borrow());
        let later = Coordinator::new(owner, Limits::PRODUCT);
        later.tick();
        let r = row(&later, 0);
        assert_eq!(
            (r.line.clone(), controls(&r)),
            (text("line.all_set.done"), finished.clone()),
            "the later life's reap answered {third:?}"
        );
        assert_eq!(
            later.card(&agent(0)).expect("the card").runs[0].text,
            text("line.run.completed"),
            "the later life's reap answered {third:?}: the card"
        );
    }

    for (end, line, card) in [
        (
            Outcome::Completed,
            "line.all_set.done",
            "line.run.completed",
        ),
        (
            Outcome::NothingToDo,
            "line.all_set.nothing",
            "line.run.nothing_to_do",
        ),
    ] {
        let (c, runs) = outlived_quit(coord::owner(&[MANUAL], room()));
        let life = Coordinator::new(
            next_life(&c, &[(runs[0].clone(), Reaped::Gone)]),
            Limits::PRODUCT,
        );
        life.tick();
        store_ends(&life, 0, &record(&runs[0], end));
        tick_at(&life, 1);
        let r = row(&life, 0);
        assert_eq!(
            (r.line.clone(), controls(&r)),
            (text(line), finished.clone()),
            "a run that this life's reap answered gone, which the record names {end:?}"
        );
        assert_eq!(
            life.card(&agent(0)).expect("the card").runs[0].text,
            text(card)
        );
    }
}

/// The narrow form of RD-151 (b) for later lives: a record Failed is not the runner's word that
/// the task ended, so it reads as today in the life after.
#[test]
fn u46b_a_later_life_reads_a_run_that_the_record_names_failed_as_before() {
    let (c, runs) = outlived_quit(coord::owner(&[MANUAL], room()));
    let life = Coordinator::new(
        next_life(&c, &[(runs[0].clone(), Reaped::Alive)]),
        Limits::PRODUCT,
    );
    life.tick();
    store_ends(&life, 0, &record(&runs[0], Outcome::Failed));
    tick_at(&life, 1);
    assert_eq!(row(&life, 0).line, text("line.problem.failed"));
    let owner = next_life(&life, &[(runs[0].clone(), Reaped::Gone)]);
    owner
        .ports
        .histories
        .borrow_mut()
        .clone_from(&life.ports().ports.histories.borrow());
    let later = Coordinator::new(owner, Limits::PRODUCT);
    later.tick();
    assert_eq!(
        row(&later, 0).line,
        text("line.all_set.stopped_early"),
        "the life after reads a record Failed as today"
    );
    assert_eq!(
        later.card(&agent(0)).expect("the card").runs[0].text,
        text("line.run.interrupted")
    );
}

#[test]
fn u46b_a_newer_run_of_another_writer_does_not_end_the_line_of_a_run_that_reap_answered_alive() {
    let (c, runs) = outlived_quit(coord::owner(&[MANUAL], room()));
    let life = Coordinator::new(alive_next_life(&c, &runs[0]), Limits::PRODUCT);
    life.tick();
    let r2 = RunId::parse("00000000000000000000000000009999").unwrap();
    another_writers_run(&life, 0, &r2);
    tick_at(&life, 1);
    assert_eq!(life.control(&agent(0), Action::SwitchOff), Ok(()));
    tick_at(&life, 2);
    assert_eq!(life.control(&agent(0), Action::SwitchOn), Ok(()));
    tick_at(&life, 3);
    assert_eq!(
        row(&life, 0).line,
        kill_stuck(),
        "r1 that reap answered alive has no record of the store"
    );
    assert_eq!(life.control(&agent(0), Action::RunNow), Err(Cause::State));
    assert_eq!(coord::started(&life), Vec::<RunId>::new(), "a start");
    assert!(!end_runs(&life).contains(&runs[0]), "end_run");
}

/// The rule of `Board::answered_alive` for every run that reap answered alive, the second of two
/// among them, and where it counts: a refused Start now of the second sends no `end_run` for it.
#[test]
fn u46b_every_run_that_reap_answers_alive_is_kept_and_holds_off_its_end_run() {
    let (c, runs) = outlived_quit(coord::owner(&[MANUAL, MANUAL], room()));
    let alive: Vec<(RunId, Reaped)> = runs.iter().map(|r| (r.clone(), Reaped::Alive)).collect();
    let board = recover(&next_life(&c, &alive), &Limits::PRODUCT)
        .expect("recovery")
        .board;
    for run in &runs {
        assert!(board.answered_alive(run), "{run:?}");
    }
    let next = next_life(&c, &alive);
    for _ in 0..16 {
        let _ = next.ports.bytes16();
    }
    next.ports.refuse_record(
        |e| matches!(e.what, What::RunEnded { .. }),
        PortError::Refused(Name::parse("malformed").unwrap()),
    );
    let life = Coordinator::new(next, Limits::PRODUCT);
    life.tick();
    life.ports().ports.set_clock(T0 - 3 * DAY_MS, DAY - 3);
    life.tick();
    let press = life.control(&agent(1), Action::RunNow);
    life.ports().ports.clear_refusals();
    life.ports().ports.set_clock(T0 + 1_000, DAY);
    life.tick();
    assert!(press.is_err(), "{press:?}");
    assert!(
        !end_runs(&life).contains(&runs[1]),
        "no end_run for the second run"
    );
    assert_eq!(coord::started(&life), Vec::<RunId>::new());
}

#[test]
fn u46b_two_gone_workflows_each_keep_their_row_while_reap_answers_both_alive() {
    let (c, runs) = outlived_quit(coord::owner(&[MANUAL, MANUAL], room()));
    let alive: Vec<(RunId, Reaped)> = runs.iter().map(|r| (r.clone(), Reaped::Alive)).collect();
    let next = next_life(&c, &alive);
    next.ports.workflows.borrow_mut().clear();
    let life = Coordinator::new(next, Limits::PRODUCT);
    life.tick();
    for n in 0..2 {
        assert_eq!(
            seen(&life, n),
            Some((Status::Working, kill_stuck(), Vec::new())),
            "agent {n}"
        );
    }
}

#[test]
fn u46b_the_second_of_two_runs_that_reap_answered_alive_reads_its_record() {
    let (c, runs) = outlived_quit(coord::owner(&[MANUAL, MANUAL], room()));
    let alive: Vec<(RunId, Reaped)> = runs.iter().map(|r| (r.clone(), Reaped::Alive)).collect();
    let life = Coordinator::new(next_life(&c, &alive), Limits::PRODUCT);
    life.tick();
    store_ends(&life, 1, &record(&runs[1], Outcome::Completed));
    tick_at(&life, 1);
    assert_eq!(row(&life, 1).line, text("line.all_set.done"));
    assert_eq!(
        life.card(&agent(1)).expect("the card").runs[0].text,
        text("line.run.completed")
    );
    assert_eq!(
        row(&life, 0).line,
        kill_stuck(),
        "the first run has no record"
    );

    let (c, runs) = outlived_quit(coord::owner(&[MANUAL, MANUAL], room()));
    let alive: Vec<(RunId, Reaped)> = runs.iter().map(|r| (r.clone(), Reaped::Alive)).collect();
    let life = Coordinator::new(next_life(&c, &alive), Limits::PRODUCT);
    life.tick();
    store_ends(&life, 1, &record(&runs[1], Outcome::Failed));
    tick_at(&life, 1);
    assert_eq!(
        row(&life, 1).line,
        text("line.problem.failed"),
        "a record Failed of the second run that reap answered alive"
    );
    assert_eq!(
        life.card(&agent(1)).expect("the card").runs[0].text,
        text("line.run.failed")
    );
}

/// The `ready` items raised for agent 0 that the ledger holds.
fn readies(c: &Coordinator<Both>) -> usize {
    c.ports()
        .ports
        .events
        .borrow()
        .iter()
        .filter(|e| {
            e.agent == agent(0)
                && matches!(
                    e.what,
                    What::ItemRaised {
                        raised: halvo_coord::types::Raised::Ready,
                        ..
                    }
                )
        })
        .count()
}

/// What the person sees of agent 0: the status, the line and the controls of its row, and the
/// kinds of its items.
type Shown = (Status, String, Vec<Control>, Vec<String>);

fn shown(c: &Coordinator<Both>) -> Shown {
    let s = c.snapshot();
    let r = s.rows.iter().find(|r| r.agent == agent(0)).unwrap();
    let items = s
        .items
        .iter()
        .filter(|i| i.agent() == &agent(0))
        .map(|i| format!("{:?}", i.kind()))
        .collect();
    (r.status, r.line.clone(), controls(r), items)
}

/// An owner whose one agent has no clean run: its seeded history cleared, or its one earlier run
/// failed.
fn no_clean_run(cleared: bool, a: A) -> Both {
    let owner = coord::owner(&[a], room());
    if cleared {
        owner.ports.events.borrow_mut().clear();
        owner.ports.agents.borrow_mut()[0].last = None;
    } else {
        for e in owner.ports.events.borrow_mut().iter_mut() {
            if let What::RunEnded { cause, .. } = &mut e.what {
                *cause = RunEnd::Failed;
            }
        }
        if let Some(last) = owner.ports.agents.borrow_mut()[0].last.as_mut() {
            last.outcome = Outcome::Failed;
        }
    }
    owner
}

/// The owner of the life after `c`: the store of `c` with its histories, the marks, and the
/// library's ids moved on.
fn carried(c: &Coordinator<Both>, marks: &[(RunId, Reaped)]) -> Both {
    let next = next_life(c, marks);
    next.ports
        .histories
        .borrow_mut()
        .clone_from(&c.ports().ports.histories.borrow());
    for _ in 0..32 {
        let _ = next.ports.bytes16();
    }
    next
}

/// Refuses every `run_ended` as `malformed` until `drop_lost`, so that recovery's never reaches
/// the ledger.
fn lose_run_ended(next: &Both) {
    next.ports.refuse_record(
        |e| matches!(e.what, What::RunEnded { .. }),
        PortError::Refused(Name::parse("malformed").unwrap()),
    );
}

/// Drops the refused `run_ended` as a debt of a day that stands alone (the clock 3 days back),
/// then sets the clock to the day again.
fn drop_lost(life: &Coordinator<Both>) {
    life.ports().ports.set_clock(T0 - 3 * DAY_MS, DAY - 3);
    life.tick();
    life.ports().ports.clear_refusals();
    life.ports().ports.set_clock(T0 + 1_000, DAY);
}

/// The reference world: the agent of `no_clean_run`, whose run the person starts and whose runner
/// ends it with `end` in the ordinary way, its record first; what the person sees after three
/// ticks, and in the life after a Quit with nothing live.
fn ordinary(cleared: bool, a: A, end: Outcome) -> (Coordinator<Both>, Shown, Shown) {
    let c = Coordinator::new(no_clean_run(cleared, a), room());
    c.tick();
    assert_eq!(c.control(&agent(0), Action::RunNow), Ok(()));
    let run = coord::started(&c).last().cloned().unwrap();
    assert_eq!(c.on_run(&run, RunEvent::Hello), Ok(()));
    store_ends(&c, 0, &record(&run, end));
    assert_eq!(c.on_run(&run, RunEvent::Ended(Some(end))), Ok(()));
    for k in 1..=3 {
        tick_at(&c, k);
    }
    let in_life = shown(&c);
    let later = Coordinator::new(carried(&c, &[]), Limits::PRODUCT);
    later.tick();
    for k in 4..=6 {
        tick_at(&later, k);
    }
    let after = shown(&later);
    (c, in_life, after)
}

/// What R8 asks of a read of a run that the record names `end`: what the person sees equals the
/// reference world; for `completed` and `nothing_to_do` also the row's line and the card's run by
/// key, no item and no `ready` raised since the record, and a row that reads that the task
/// finished before it offers Start now.
fn reads_as(c: &Coordinator<Both>, form: &str, end: Outcome, want: &Shown, at_record: usize) {
    assert_eq!(
        &shown(c),
        want,
        "{form}: the row, its items and its controls, as where the run ended so in the ordinary way"
    );
    let (line, card) = match end {
        Outcome::Completed => ("line.all_set.done", "line.run.completed"),
        Outcome::NothingToDo => ("line.all_set.nothing", "line.run.nothing_to_do"),
        _ => return,
    };
    let r = row(c, 0);
    assert_eq!(r.line, text(line), "{form}: the row's line");
    assert_eq!(
        c.card(&agent(0)).expect("the card").runs[0].text,
        text(card),
        "{form}: the card"
    );
    assert_eq!(shown(c).3, Vec::<String>::new(), "{form}: no item");
    assert_eq!(
        readies(c),
        at_record,
        "{form}: no ready raised since the record"
    );
    let json = serde_json::to_string(&r).unwrap();
    let said = json
        .find(&serde_json::to_string(&text(line)).unwrap())
        .expect("the line");
    let start = json.find("\"run_now\"").expect("Start now");
    assert!(
        said < start,
        "{form}: the row reads that the task finished before it offers Start now"
    );
}

/// R8 (RD-156 (1)): for an agent with no clean run, a first task that outlived Quit and that the
/// record names `completed` or `nothing_to_do` is never offered as ready to start: not in the life
/// where reap answered it alive, not in the life after, not in a first next life whose reap
/// answered it gone, and not where its own life ended it with no outcome; a record `failed` reads
/// as where the run failed in the ordinary way. A scheduled agent that needs no person starts
/// nothing more in the slot that the finished run served; in the life where reap answered its
/// run alive the next slot waits for a start of the person (a named limit), and the life after
/// starts it.
#[test]
fn u46b_a_first_task_that_the_record_names_finished_is_never_offered_as_ready_to_start() {
    for cleared in [true, false] {
        for end in [Outcome::Completed, Outcome::NothingToDo, Outcome::Failed] {
            let (_, in_life, after) = ordinary(cleared, MANUAL, end);
            for lost in [false, true] {
                let form =
                    format!("cleared={cleared} end={end:?} recovery's run_ended lost={lost}");

                let (c, runs) = outlived_quit(no_clean_run(cleared, MANUAL));
                let next = carried(&c, &[(runs[0].clone(), Reaped::Alive)]);
                if lost {
                    lose_run_ended(&next);
                }
                let life = Coordinator::new(next, Limits::PRODUCT);
                life.tick();
                if lost {
                    drop_lost(&life);
                }
                store_ends(&life, 0, &record(&runs[0], end));
                let at_record = readies(&life);
                for k in 1..=3 {
                    tick_at(&life, k);
                }
                let place = format!("{form}, the life where reap answered alive");
                reads_as(&life, &place, end, &in_life, at_record);
                for marks in [vec![(runs[0].clone(), Reaped::Gone)], Vec::new()] {
                    let later = Coordinator::new(carried(&life, &marks), Limits::PRODUCT);
                    later.tick();
                    for k in 4..=6 {
                        tick_at(&later, k);
                    }
                    let place = format!("{form}, the life after with the marks {marks:?}");
                    reads_as(&later, &place, end, &after, at_record);
                }

                let (c, runs) = outlived_quit(no_clean_run(cleared, MANUAL));
                store_ends(&c, 0, &record(&runs[0], end));
                let at_record = readies(&c);
                let next = carried(&c, &[(runs[0].clone(), Reaped::Gone)]);
                if lost {
                    lose_run_ended(&next);
                }
                let first = Coordinator::new(next, Limits::PRODUCT);
                first.tick();
                if lost {
                    drop_lost(&first);
                }
                for k in 1..=3 {
                    tick_at(&first, k);
                }
                let place = format!("{form}, a first next life whose reap answered gone");
                reads_as(&first, &place, end, &after, at_record);
            }

            let c = Coordinator::new(no_clean_run(cleared, MANUAL), room());
            c.tick();
            assert_eq!(c.control(&agent(0), Action::RunNow), Ok(()));
            let run = coord::started(&c).last().cloned().unwrap();
            assert_eq!(c.on_run(&run, RunEvent::Hello), Ok(()));
            store_ends(&c, 0, &record(&run, end));
            assert_eq!(c.on_run(&run, RunEvent::Ended(None)), Ok(()));
            let at_record = readies(&c);
            for k in 1..=3 {
                tick_at(&c, k);
            }
            let place =
                format!("cleared={cleared} end={end:?}, its own life ended it with no outcome");
            reads_as(&c, &place, end, &in_life, at_record);
        }
    }

    let slotted = A {
        person: false,
        slotted: true,
    };
    let next_slot = |c: &Coordinator<Both>| {
        let before = coord::started(c).len();
        c.ports().ports.set_clock(T0 + 2 * DAY_MS, DAY + 2);
        c.tick();
        coord::started(c).len() - before
    };
    let (reference, in_life, _) = ordinary(true, slotted, Outcome::Completed);
    let (c, runs) = outlived_quit(no_clean_run(true, slotted));
    let life = Coordinator::new(
        carried(&c, &[(runs[0].clone(), Reaped::Alive)]),
        Limits::PRODUCT,
    );
    life.tick();
    store_ends(&life, 0, &record(&runs[0], Outcome::Completed));
    let at_record = readies(&life);
    for k in 1..=3 {
        tick_at(&life, k);
    }
    let place = "a scheduled agent that needs no person, the life where reap answered alive";
    reads_as(&life, place, Outcome::Completed, &in_life, at_record);
    assert_eq!(
        coord::started(&life),
        Vec::<RunId>::new(),
        "the slot that the finished run served starts nothing more"
    );
    assert_eq!(
        next_slot(&reference),
        1,
        "the ordinary world starts one run of the scheduler in the next slot"
    );
    assert_eq!(
        next_slot(&life),
        0,
        "a named limit: in the life where reap answered alive, the ready item that the tick \
         raised while the run had no record stands hidden, and the scheduler starts nothing \
         until a start of the person"
    );
    let later = Coordinator::new(carried(&life, &[]), Limits::PRODUCT);
    later.ports().ports.set_clock(T0 + 2 * DAY_MS, DAY + 2);
    later.tick();
    later
        .ports()
        .ports
        .set_clock(T0 + 2 * DAY_MS + 1_000, DAY + 2);
    later.tick();
    assert_eq!(
        coord::started(&later).len(),
        1,
        "the life after starts the run of the scheduler in that slot"
    );
}

/// R9 (RD-156 (2)): the workflow is gone at recovery and the owner reads the history; the older
/// run, which reap answered alive, has no record, and a newer run of another writer has its
/// record. Whatever reap answered for the newer run, and in either order of the answers, the row
/// stands with the stuck-kill line and no control, and nothing is logged, sent or started.
#[test]
fn u46b_a_gone_workflow_keeps_its_row_whatever_newer_run_another_writer_added() {
    let r2 = RunId::parse("00000000000000000000000000009999").unwrap();
    for newer in [Some(Reaped::Alive), Some(Reaped::Gone), None] {
        for older_first in [true, false] {
            let (c, runs) = outlived_quit(coord::owner(&[MANUAL], room()));
            another_writers_run(&c, 0, &r2);
            let mut marks = vec![(runs[0].clone(), Reaped::Alive)];
            marks.extend(newer.map(|answer| (r2.clone(), answer)));
            if !older_first {
                marks.reverse();
            }
            let next = next_life(&c, &marks);
            next.ports.workflows.borrow_mut().clear();
            let life = Coordinator::new(next, Limits::PRODUCT);
            life.tick();
            tick_at(&life, 1);
            assert_eq!(
                seen(&life, 0),
                Some((Status::Working, kill_stuck(), Vec::new())),
                "reap's answers {marks:?}"
            );
            let effects = life
                .ports()
                .ports
                .tried
                .borrow()
                .iter()
                .filter(|t| matches!(t.call, Call::Log(_) | Call::Send(_) | Call::Start(_)))
                .count();
            assert_eq!(effects, 0, "nothing logged, sent or started: {marks:?}");
        }
    }
}

/// R10 (RD-156 (a)): a run that its own life ends with no outcome, whose `run_ended` that life
/// writes `interrupted`, reads the end of a later record `completed` or `nothing_to_do`, the word
/// of the runner itself; a record `failed` leaves the interrupted row standing.
#[test]
fn u46b_a_run_that_its_own_life_ended_with_no_outcome_reads_the_end_that_the_record_names() {
    for (end, line, card) in [
        (
            Outcome::Completed,
            "line.all_set.done",
            "line.run.completed",
        ),
        (
            Outcome::NothingToDo,
            "line.all_set.nothing",
            "line.run.nothing_to_do",
        ),
        (
            Outcome::Failed,
            "line.all_set.stopped_early",
            "line.run.interrupted",
        ),
    ] {
        let c = Coordinator::new(coord::owner(&[MANUAL], room()), room());
        c.tick();
        assert_eq!(c.control(&agent(0), Action::RunNow), Ok(()));
        let run = coord::started(&c).last().cloned().unwrap();
        assert_eq!(c.on_run(&run, RunEvent::Hello), Ok(()));
        assert_eq!(c.on_run(&run, RunEvent::Ended(None)), Ok(()));
        tick_at(&c, 1);
        assert!(
            c.ports().ports.events.borrow().iter().any(|e| matches!(
                &e.what,
                What::RunEnded { run: r, cause: RunEnd::Interrupted, .. } if *r == run
            )),
            "the life wrote its own interrupted"
        );
        store_ends(&c, 0, &record(&run, end));
        tick_at(&c, 2);
        assert_eq!(row(&c, 0).line, text(line), "the record {end:?}");
        assert_eq!(
            c.card(&agent(0)).expect("the card").runs[0].text,
            text(card),
            "the record {end:?}: the card"
        );
    }
}

/// The coordinator's word of 2026-11-21T01:59:47Z on R8: the one `ready` that the tick raises
/// while the run that reap answered alive has no record stays, as Schedule.tla has it; once the
/// record names that first task completed, its item is not shown and is counted and reached
/// nowhere a person meets it: no number of the tray or of the items counts it, no published
/// snapshot or card carries its id, which the page needs to reach its Start, and no rule of the
/// slots starts anything from it.
#[test]
fn u46b_the_hidden_ready_item_of_a_finished_first_task_is_counted_reached_and_started_nowhere() {
    for (a, kind) in [
        (MANUAL, "manual"),
        (
            A {
                person: false,
                slotted: true,
            },
            "scheduled, no person",
        ),
    ] {
        let (reference, _, _) = ordinary(true, a, Outcome::Completed);
        let (c, runs) = outlived_quit(no_clean_run(true, a));
        let life = Coordinator::new(
            carried(&c, &[(runs[0].clone(), Reaped::Alive)]),
            Limits::PRODUCT,
        );
        let before = life.ports().ports.events.borrow().len();
        life.tick();
        let hidden = life.ports().ports.events.borrow()[before..]
            .iter()
            .find_map(|e| match &e.what {
                What::ItemRaised {
                    item,
                    raised: halvo_coord::types::Raised::Ready,
                } if e.agent == agent(0) => Some(item.clone()),
                _ => None,
            })
            .expect("the one raise that Schedule.tla holds");
        store_ends(&life, 0, &record(&runs[0], Outcome::Completed));
        for k in 1..=3 {
            tick_at(&life, k);
        }

        assert_eq!(
            (attention(&life), shown(&life).3.len()),
            (attention(&reference), 0),
            "{kind}: the number of the tray and the items count no hidden item"
        );
        let id = serde_json::to_string(&hidden).unwrap();
        let snapshot = serde_json::to_string(&life.snapshot()).unwrap();
        let card = serde_json::to_string(&life.card(&agent(0)).expect("the card")).unwrap();
        assert!(
            !snapshot.contains(&id) && !card.contains(&id),
            "{kind}: no published snapshot or card carries the id of the hidden item"
        );

        let starts = coord::started(&life).len();
        life.ports().ports.set_clock(T0 + 2 * DAY_MS, DAY + 2);
        life.tick();
        life.ports()
            .ports
            .set_clock(T0 + 2 * DAY_MS + 1_000, DAY + 2);
        life.tick();
        let answered = life
            .ports()
            .ports
            .events
            .borrow()
            .iter()
            .any(|e| matches!(&e.what, What::ItemAnswered { item, .. } if *item == hidden));
        assert_eq!(
            (coord::started(&life).len() - starts, answered),
            (0, false),
            "{kind}: no rule of the slots starts anything from the hidden item"
        );
    }
}

/// What a page is given of agent 0 after a move: the published snapshot and the agent's card,
/// serialized.
fn published(c: &Coordinator<Both>) -> String {
    let snapshot = serde_json::to_string(&c.snapshot()).unwrap();
    let card = serde_json::to_string(&c.card(&agent(0)).expect("the card")).unwrap();
    format!("{snapshot}\n{card}")
}

/// Another writer's `hold_set` with the cause `failures` for agent 0 in the owner's ledger,
/// which the next tick reads.
fn failures_hold(c: &Coordinator<Both>) {
    let mut events = c.ports().ports.events.borrow_mut();
    let last = events
        .iter()
        .filter(|e| e.agent == agent(0))
        .map(|e| e.seq.get())
        .max()
        .unwrap_or(0);
    events.push(Event {
        workflow: flow(0),
        agent: agent(0),
        seq: NonZeroU32::new(last + 1).unwrap(),
        day: DAY,
        at: 0,
        what: What::HoldSet {
            cause: halvo_coord::types::HoldSetCause::Failures,
        },
    });
}

/// The life where reap answered alive the first task of an agent with no clean run, with the
/// record `end` after its first tick and three ticks more; the id of the `ready` that its first
/// tick raised.
fn stored_raise(
    cleared: bool,
    a: A,
    end: Outcome,
) -> (Coordinator<Both>, halvo_coord::types::ItemId) {
    let (c, runs) = outlived_quit(no_clean_run(cleared, a));
    let life = Coordinator::new(
        carried(&c, &[(runs[0].clone(), Reaped::Alive)]),
        Limits::PRODUCT,
    );
    let before = life.ports().ports.events.borrow().len();
    life.tick();
    let raised = life.ports().ports.events.borrow()[before..]
        .iter()
        .find_map(|e| match &e.what {
            What::ItemRaised {
                item,
                raised: halvo_coord::types::Raised::Ready,
            } if e.agent == agent(0) => Some(item.clone()),
            _ => None,
        })
        .expect("the one raise that Schedule.tla holds");
    store_ends(&life, 0, &record(&runs[0], end));
    for k in 1..=3 {
        tick_at(&life, k);
    }
    (life, raised)
}

/// R12 (RD-162 (1), the coordinator's line of 2026-11-21T04:00:53Z): the item of a run that the
/// record names completed or nothing_to_do is not shown, not counted and not reachable, in
/// every state of the agent, until a new slot opens for it. Case 1: one Switch off after the
/// record, then Switch on. Case 2: the hold `failures`. Both read as the ordinary way under the
/// same hold, publish no id of the stored item, and start nothing from it; a record `failed`
/// leaves the ordinary item shown under Switch off. Case 3, the guard: an agent on a schedule
/// whose first task found nothing to do gets the ask at its next open slot, as the ordinary
/// way has it.
#[test]
fn u46b_the_stored_ready_of_a_finished_first_task_stays_hidden_under_a_hold_until_a_new_slot_opens()
{
    let tray = |c: &Coordinator<Both>| (shown(c), attention(c));
    for cleared in [true, false] {
        for (end, line) in [
            (Outcome::Completed, "line.all_set.done"),
            (Outcome::NothingToDo, "line.all_set.nothing"),
            (Outcome::Failed, ""),
        ] {
            let form = format!("cleared={cleared} end={end:?}");
            let (reference, _, _) = ordinary(cleared, MANUAL, end);
            assert_eq!(reference.control(&agent(0), Action::SwitchOff), Ok(()));
            tick_at(&reference, 4);
            let want_off = tray(&reference);
            assert_eq!(reference.control(&agent(0), Action::SwitchOn), Ok(()));
            tick_at(&reference, 5);
            let want_on = tray(&reference);

            let (life, hidden) = stored_raise(cleared, MANUAL, end);
            let id = serde_json::to_string(&hidden).unwrap();
            let mut pages = vec![published(&life)];
            assert_eq!(life.control(&agent(0), Action::SwitchOff), Ok(()));
            tick_at(&life, 4);
            pages.push(published(&life));
            assert_eq!(
                tray(&life),
                want_off,
                "{form}: case 1, switched off: the row, the items, the controls and the tray"
            );
            assert_eq!(life.control(&agent(0), Action::SwitchOn), Ok(()));
            tick_at(&life, 5);
            pages.push(published(&life));
            assert_eq!(tray(&life), want_on, "{form}: case 1, switched on again");
            if end == Outcome::Failed {
                assert_eq!(
                    want_off.0 .3,
                    vec!["Ready".to_string()],
                    "{form}: the ordinary way's own item stays shown under Switch off"
                );
                continue;
            }
            assert_eq!(
                (want_off.0 .1.clone(), want_off.1),
                (text("line.off.person"), Some(0)),
                "{form}: the ordinary way reads off"
            );
            assert_eq!(
                row(&life, 0).line,
                text(line),
                "{form}: the row reads the end"
            );

            let (reference, _, _) = ordinary(cleared, MANUAL, end);
            failures_hold(&reference);
            tick_at(&reference, 4);
            let want_hold = tray(&reference);
            let (held, hidden_2) = stored_raise(cleared, MANUAL, end);
            let id_2 = serde_json::to_string(&hidden_2).unwrap();
            failures_hold(&held);
            tick_at(&held, 4);
            pages.push(published(&held));
            assert_eq!(
                (want_hold.0 .1.clone(), want_hold.1),
                (text("line.problem.failures"), Some(0)),
                "{form}: the ordinary way reads the hold failures"
            );
            assert_eq!(tray(&held), want_hold, "{form}: case 2, the hold failures");
            held.ports().ports.set_clock(T0 + 2 * DAY_MS, DAY + 2);
            held.tick();
            pages.push(published(&held));
            assert_eq!(
                tray(&held).0 .3,
                Vec::<String>::new(),
                "{form}: case 2, two days on"
            );

            for (c, hidden, id) in [(&life, &hidden, &id), (&held, &hidden_2, &id_2)] {
                assert!(
                    !pages.iter().any(|p| p.contains(id.as_str())),
                    "{form}: no published snapshot or card carries the id of the stored item"
                );
                let answered =
                    c.ports().ports.events.borrow().iter().any(
                        |e| matches!(&e.what, What::ItemAnswered { item, .. } if item == hidden),
                    );
                assert_eq!(
                    (coord::started(c).len(), answered),
                    (0, false),
                    "{form}: nothing starts from the stored item"
                );
            }
        }
    }

    let slotted = A {
        person: false,
        slotted: true,
    };
    let next_slot = |c: &Coordinator<Both>| {
        c.ports().ports.set_clock(T0 + 2 * DAY_MS, DAY + 2);
        c.tick();
        c.ports().ports.set_clock(T0 + 2 * DAY_MS + 1_000, DAY + 2);
        c.tick();
    };
    let (reference, _, _) = ordinary(true, slotted, Outcome::NothingToDo);
    let (life, _) = stored_raise(true, slotted, Outcome::NothingToDo);
    assert_eq!(
        (row(&life, 0).line, shown(&life).3),
        (text("line.all_set.nothing"), Vec::<String>::new()),
        "case 3: before the next slot the stored item is hidden"
    );
    next_slot(&reference);
    next_slot(&life);
    assert_eq!(
        tray(&life),
        tray(&reference),
        "case 3: at the next open slot the ask is shown and counted as the ordinary way has it"
    );
    assert_eq!(shown(&life).3, vec!["Ready".to_string()], "case 3: the ask");
    let press = |c: &Coordinator<Both>| {
        let before = coord::started(c).len();
        let item = c
            .snapshot()
            .items
            .into_iter()
            .find(|i| i.agent() == &agent(0))
            .expect("the card of the ask");
        let answer = c.answer(
            item.item(),
            halvo_coord::types::Choice::Start,
            None,
            item.step(),
        );
        (answer, coord::started(c).len() - before)
    };
    assert_eq!(
        press(&life),
        press(&reference),
        "case 3: the ask is reachable, and its Start starts one run as the ordinary way's"
    );
}
