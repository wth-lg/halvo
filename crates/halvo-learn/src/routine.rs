use crate::{MISS_CYCLES, STALE_FLOOR, TOLERANCE};

tokens! {
    /// The mark of a routine (L6.3).
    pub enum Mark {
        None = "none",
        Stale = "stale",
    }
}

/// The figures of a `learn_routine` row, the keys `period` to `since` of L1.4 in their order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoutineState {
    /// The median gap between occurrence days, or `None` until a first estimate.
    pub period: Option<u32>,
    /// The occurrence days in the window.
    pub seen: u32,
    /// The first occurrence day, or `None` before any.
    pub first: Option<u32>,
    /// The last occurrence day, or `None` before any.
    pub last: Option<u32>,
    /// `last` + `period`, or `None` while either is `None`.
    pub next: Option<u32>,
    /// The cycles missed since `last`.
    pub missed: u32,
    /// The mark.
    pub mark: Mark,
    /// The day on which the mark took its value, or `None` while it has always been `none`.
    pub since: Option<u32>,
}

/// The routine figures of `today` from the newest row's figures `prev` and the occurrence
/// days (L6.2, L6.3). It reads the days d of `days` with today - 92 <= d <= today, ascending
/// and each once, whatever order and repeats `days` holds. `period` is, with at least three
/// such days, the median of the gaps between successive days, the lower of the two middle
/// gaps for an even number; `missed` is (today - `last` - 1) / (`period` + [`TOLERANCE`])
/// once today is after `last`; the mark is `stale` when [`MISS_CYCLES`] is above 0, `missed`
/// is at least it and today - `last` is at least [`STALE_FLOOR`]. `first`, `last` and
/// `period` keep `prev`'s values where the days give none.
pub fn routine_step(prev: Option<&RoutineState>, days: &[u32], today: u32) -> RoutineState {
    let from = today.saturating_sub(92);
    let mut window: Vec<u32> = days
        .iter()
        .copied()
        .filter(|day| (from..=today).contains(day))
        .collect();
    window.sort_unstable();
    window.dedup();

    let first = window.first().copied().or(prev.and_then(|p| p.first));
    let last = window.last().copied().or(prev.and_then(|p| p.last));
    let period = if window.len() >= 3 {
        Some(median_gap(&window))
    } else {
        prev.and_then(|p| p.period)
    };
    let next = last
        .zip(period)
        .map(|(last, period)| last.saturating_add(period));
    let missed = match (last, period) {
        (Some(last), Some(period)) if today > last => {
            (today - last - 1) / period.saturating_add(TOLERANCE)
        }
        _ => 0,
    };
    let quiet_long = last.is_some_and(|last| today.saturating_sub(last) >= STALE_FLOOR);
    let mark = if MISS_CYCLES > 0 && missed >= MISS_CYCLES && quiet_long {
        Mark::Stale
    } else {
        Mark::None
    };
    let since = if mark != prev.map_or(Mark::None, |p| p.mark) {
        Some(today)
    } else {
        prev.and_then(|p| p.since)
    };
    RoutineState {
        period,
        seen: window.len() as u32,
        first,
        last,
        next,
        missed,
        mark,
        since,
    }
}

fn median_gap(days: &[u32]) -> u32 {
    let mut gaps: Vec<u32> = days.windows(2).map(|pair| pair[1] - pair[0]).collect();
    gaps.sort_unstable();
    gaps[(gaps.len() - 1) / 2]
}
