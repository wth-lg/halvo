use halvo_schema::executable::names::Mode;
use halvo_schema::learning::record::{LearnRoutine, LearnRun, RunStatus, Verdict};
use halvo_schema::Summary;

pub(super) fn users_doing(routine: &LearnRoutine, summary: &Summary) -> bool {
    summary.category == routine.category
        && match (&summary.site, &routine.site) {
            (Some(a), Some(b)) => a == b,
            (None, None) => summary.app == routine.app,
            _ => false,
        }
}

fn agents_doing(routine: &LearnRoutine, run: &LearnRun) -> bool {
    run.workflow == routine.workflow
        && run.mode == Mode::Replay
        && matches!(
            run.status,
            RunStatus::Ended {
                verdict: Verdict::Verified,
                ..
            }
        )
}

pub(super) fn occurrences(
    routine: &LearnRoutine,
    summaries: &[Summary],
    runs: &[LearnRun],
) -> Vec<u32> {
    let mut days: Vec<u32> = summaries
        .iter()
        .filter(|s| users_doing(routine, s))
        .map(|s| s.day)
        .chain(
            runs.iter()
                .filter(|r| agents_doing(routine, r))
                .map(|r| r.day.get()),
        )
        .filter(|&day| day > 0)
        .collect();
    days.sort_unstable();
    days.dedup();
    days
}
