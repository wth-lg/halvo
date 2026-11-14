use std::collections::BTreeSet;
use std::num::NonZeroU32;

use halvo_learn::{routine_step, Mark as RuleMark, RoutineState};
use halvo_schema::learning::names::Mark;
use halvo_schema::learning::record::{Day, LearnRoutine};
use halvo_schema::{AgentId, RowRead, Summary, SummaryRead, TypedRecord, WorkflowId, WriterStep};

use crate::host::Clock;
use crate::rows;

mod days;

#[cfg(test)]
mod tests;

const REWRITE_DAYS: u32 = 30;

const AHEAD_DAYS_KEPT: u32 = 2;

/// The routine step of the learning run (contracts.md L4.5, L6), a `WriterStep` that a host puts after the
/// recurrence step. After each committed batch it reads the bounded summaries and the rows of the kinds
/// `learn_routine` and `learn_run` of the family `learn`; it reads no `learn_edit` row. For the newest
/// `learn_routine` row of each workflow and agent it applies `routine_step` of halvo-learn to that row's figures,
/// the occurrence days of L6.1 and the day of the host's clock, and returns the new row, with `rev` + 1 and `day`
/// today, where its mark, `period` or `last` differs from the newest row's or the newest row is 30 or more days
/// old. A summary is an occurrence when its category and its whole site value, the source included, equal the
/// row's, or, where both have no site, its category and its app equal the row's. While the day of a pair's newest
/// row lies one or two days after the clock's day, it returns no row for that pair and does not apply
/// `routine_step` to it; two days is the workflow run's store's own number for a row stamped after its clock. It
/// returns `learn_routine` rows alone, and nothing when a `learn_routine` or `learn_run` row cannot be read or the
/// clock gives day 0.
pub struct LearnStep<C> {
    clock: C,
}

impl<C: Clock> LearnStep<C> {
    /// The step that takes today from `clock`, the host's clock (contracts.md L0.6).
    pub fn new(clock: C) -> LearnStep<C> {
        LearnStep { clock }
    }
}

impl<C: Clock> WriterStep for LearnStep<C> {
    fn after_commit(&self, summaries: &dyn SummaryRead, rows: &dyn RowRead) -> Vec<TypedRecord> {
        written(self.clock.today(), summaries, rows).unwrap_or_default()
    }
}

fn written(
    today: u32,
    summaries: &dyn SummaryRead,
    rows: &dyn RowRead,
) -> Option<Vec<TypedRecord>> {
    let day = Day::new(today)?;
    let routines = rows::routines(rows).ok()?;
    let runs = rows::runs(rows).ok()?;
    let pairs: BTreeSet<(&WorkflowId, &AgentId)> =
        routines.iter().map(|r| (&r.workflow, &r.agent)).collect();
    let newest: Vec<&LearnRoutine> = pairs
        .into_iter()
        .filter_map(|(workflow, agent)| rows::newest_routine(&routines, workflow, agent))
        .filter(|row| !rests(row, today))
        .collect();
    let mut theirs: Vec<Summary> = Vec::new();
    summaries.for_each(&mut |s| {
        if newest.iter().any(|row| days::users_doing(row, s)) {
            theirs.push(s.clone());
        }
    });
    let mut out = Vec::new();
    for row in newest {
        let prev = figures(row);
        let next = routine_step(Some(&prev), &days::occurrences(row, &theirs, &runs), today);
        let due = next.mark != prev.mark
            || next.period != prev.period
            || next.last != prev.last
            || today.saturating_sub(row.day.get()) >= REWRITE_DAYS;
        if !due {
            continue;
        }
        if let Some(record) = successor(row, &next, day).and_then(|r| rows::routine_record(&r).ok())
        {
            out.push(record);
        }
    }
    Some(out)
}

fn rests(newest: &LearnRoutine, today: u32) -> bool {
    (1..=AHEAD_DAYS_KEPT).contains(&newest.day.get().saturating_sub(today))
}

fn figures(row: &LearnRoutine) -> RoutineState {
    RoutineState {
        period: row.period.map(NonZeroU32::get),
        seen: row.seen,
        first: row.first.map(Day::get),
        last: row.last.map(Day::get),
        next: row.next.map(Day::get),
        missed: row.missed,
        mark: match row.mark {
            Mark::None => RuleMark::None,
            Mark::Stale => RuleMark::Stale,
        },
        since: row.since.map(Day::get),
    }
}

fn stored_day(day: Option<u32>) -> Option<Option<Day>> {
    day.map(Day::new).map_or(Some(None), |d| d.map(Some))
}

fn successor(row: &LearnRoutine, next: &RoutineState, day: Day) -> Option<LearnRoutine> {
    Some(LearnRoutine {
        workflow: row.workflow.clone(),
        agent: row.agent.clone(),
        rev: row.rev.checked_add(1)?,
        day,
        category: row.category,
        site: row.site.clone(),
        app: row.app.clone(),
        period: next
            .period
            .map(NonZeroU32::new)
            .map_or(Some(None), |p| p.map(Some))?,
        seen: next.seen,
        first: stored_day(next.first)?,
        last: stored_day(next.last)?,
        next: stored_day(next.next)?,
        missed: next.missed,
        mark: match next.mark {
            RuleMark::None => Mark::None,
            RuleMark::Stale => Mark::Stale,
        },
        since: stored_day(next.since)?,
    })
}
