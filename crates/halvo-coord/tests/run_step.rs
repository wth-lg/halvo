use halvo_coord::run::{run_step, Ending, Next, Phase, RunMove, StopCause};
use halvo_coord::types::{Cause, ItemId, Outcome, RunEnd, RunItem, RunItemKind};

fn item() -> RunItem {
    RunItem {
        item: ItemId::parse(&format!("{:032x}", 7)).unwrap(),
        kind: RunItemKind::Confirm,
    }
}

fn phases() -> Vec<Phase> {
    vec![
        Phase::Starting,
        Phase::Working,
        Phase::Waiting(item()),
        Phase::Stopping(StopCause::Cancelled),
        Phase::Stopping(StopCause::Lapsed),
        Phase::Stopping(StopCause::Deleted),
    ]
}

fn moves() -> Vec<RunMove> {
    let mut out = vec![
        RunMove::Spawn,
        RunMove::Hello,
        RunMove::Wait(item()),
        RunMove::Resume,
        RunMove::Stop(StopCause::Cancelled),
        RunMove::Stop(StopCause::Lapsed),
        RunMove::Stop(StopCause::Deleted),
        RunMove::End(Ending::Frame(None)),
        RunMove::End(Ending::Kill),
        RunMove::End(Ending::Restart),
        RunMove::End(Ending::UnknownRun),
    ];
    for o in Outcome::ALL {
        out.push(RunMove::End(Ending::Frame(Some(*o))));
    }
    out
}

fn expected(from: Option<&Phase>, mv: &RunMove) -> Result<Next, Cause> {
    let live = |p: Phase| Ok(Next::Live(p));
    let ended = |c: RunEnd| Ok(Next::Ended(c));
    let stopped = |p: &Phase| match p {
        Phase::Stopping(c) => Some(*c),
        _ => None,
    };
    match (from, mv) {
        (None, RunMove::Spawn) => live(Phase::Starting),
        (Some(Phase::Starting), RunMove::Hello) => live(Phase::Working),
        (Some(Phase::Working), RunMove::Wait(i)) => live(Phase::Waiting(i.clone())),
        (Some(Phase::Waiting(_)), RunMove::Resume) => live(Phase::Working),
        (Some(p), RunMove::Stop(c)) if stopped(p).is_none() => live(Phase::Stopping(*c)),
        (Some(Phase::Stopping(_)), RunMove::Stop(StopCause::Deleted)) => {
            live(Phase::Stopping(StopCause::Deleted))
        }
        (Some(Phase::Stopping(c)), RunMove::End(Ending::Frame(_) | Ending::Kill)) => ended(c.end()),
        (Some(Phase::Stopping(_)), RunMove::End(Ending::Restart | Ending::UnknownRun)) => {
            ended(RunEnd::Interrupted)
        }
        (Some(_), RunMove::End(Ending::Frame(Some(o)))) => ended(RunEnd::from(*o)),
        (Some(_), RunMove::End(Ending::Frame(None) | Ending::Restart)) => {
            ended(RunEnd::Interrupted)
        }
        _ => Err(Cause::State),
    }
}

#[test]
fn run_step_has_the_moves_of_i5_6_and_refuses_every_other_pair_with_state() {
    let mut froms: Vec<Option<Phase>> = vec![None];
    froms.extend(phases().into_iter().map(Some));
    let mut pairs = 0;
    for from in &froms {
        for mv in moves() {
            assert_eq!(
                run_step(from.as_ref(), mv.clone()),
                expected(from.as_ref(), &mv),
                "{from:?} {mv:?}"
            );
            pairs += 1;
        }
    }
    assert_eq!(pairs, 7 * 20);
}

#[test]
fn a_stopped_run_ends_with_the_cause_of_its_stop_whatever_its_last_frame() {
    for c in [StopCause::Cancelled, StopCause::Lapsed, StopCause::Deleted] {
        let end = run_step(
            Some(&Phase::Stopping(c)),
            RunMove::End(Ending::Frame(Some(Outcome::Completed))),
        );
        assert_eq!(end, Ok(Next::Ended(c.end())));
        assert_ne!(c.end(), RunEnd::Completed);
    }
    assert_eq!(StopCause::Cancelled.end(), RunEnd::Cancelled);
    assert_eq!(StopCause::Lapsed.end(), RunEnd::Lapsed);
    assert_eq!(StopCause::Deleted.end(), RunEnd::Deleted);
}

#[test]
fn a_stopping_run_that_a_restart_finds_or_whose_kill_is_unknown_run_ends_interrupted() {
    for mv in [Ending::Restart, Ending::UnknownRun] {
        assert_eq!(
            run_step(
                Some(&Phase::Stopping(StopCause::Cancelled)),
                RunMove::End(mv)
            ),
            Ok(Next::Ended(RunEnd::Interrupted))
        );
    }
}

#[test]
fn a_kill_ends_only_a_stopped_run() {
    for p in [Phase::Starting, Phase::Working, Phase::Waiting(item())] {
        assert_eq!(
            run_step(Some(&p), RunMove::End(Ending::Kill)),
            Err(Cause::State)
        );
    }
}
