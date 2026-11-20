#[path = "coordinator_support.rs"]
mod coord;

use coord::{agent, Both, MANUAL};
use halvo_coord::mem::Method;
use halvo_coord::ports::{PortError, Reaped};
use halvo_coord::run::recover;
use halvo_coord::types::{Action, Limits, RunEnd, RunEvent, RunId};
use halvo_coord::Coordinator;

fn two_live() -> Limits {
    Limits {
        max_live_runs: 2,
        ..Limits::PRODUCT
    }
}

/// A first life of the app over `owner` in which the person starts a run of each agent, and a
/// Quit whose kill the recorder refuses, so that every run outlives Quit: no real kill runs.
fn outlived_quit(owner: Both) -> (Coordinator<Both>, Vec<RunId>) {
    let n = owner.ports.agents.borrow().len();
    let c = Coordinator::new(owner, two_live());
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

#[test]
fn u46b_recover_keeps_on_the_board_the_run_that_reap_answers_alive() {
    let (c, runs) = outlived_quit(coord::owner(&[MANUAL, MANUAL], two_live()));
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
