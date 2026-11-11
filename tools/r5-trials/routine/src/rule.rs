use crate::train::TRAIN_DAYS;

pub const WINDOW_DAYS: u32 = 93;
pub const DAYS_FOR_PERIOD: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Setting {
    pub miss_cycles: u32,
    pub stale_floor: u32,
    pub tolerance: u32,
}

impl Setting {
    pub const fn new(miss_cycles: u32, stale_floor: u32, tolerance: u32) -> Setting {
        Setting {
            miss_cycles,
            stale_floor,
            tolerance,
        }
    }

    pub fn name(&self) -> String {
        format!(
            "{}-{}-{}",
            self.miss_cycles, self.stale_floor, self.tolerance
        )
    }
}

pub const SETTINGS: [Setting; 8] = [
    Setting::new(2, 0, 5),
    Setting::new(2, 0, 2),
    Setting::new(2, 60, 5),
    Setting::new(2, 60, 2),
    Setting::new(3, 0, 5),
    Setting::new(3, 0, 2),
    Setting::new(3, 60, 5),
    Setting::new(3, 60, 2),
];

pub const NO_MARK: Setting = Setting::new(0, 0, 0);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mark {
    #[default]
    None,
    Stale,
}

impl Mark {
    pub fn name(self) -> &'static str {
        match self {
            Mark::None => "none",
            Mark::Stale => "stale",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RoutineState {
    pub seen: u32,
    pub first: Option<u32>,
    pub last: Option<u32>,
    pub period: Option<u32>,
    pub next: Option<u32>,
    pub missed: u32,
    pub mark: Mark,
    pub since: Option<u32>,
}

pub fn median_gap(days: &[u32]) -> u32 {
    let mut gaps: Vec<u32> = days.windows(2).map(|w| w[1] - w[0]).collect();
    gaps.sort_unstable();
    gaps[(gaps.len() - 1) / 2].max(1)
}

pub fn routine_step(
    setting: Setting,
    prev: Option<&RoutineState>,
    days: &[u32],
    today: u32,
) -> RoutineState {
    let first = days.first().copied().or(prev.and_then(|p| p.first));
    let last = days.last().copied().or(prev.and_then(|p| p.last));
    let period = if days.len() >= DAYS_FOR_PERIOD {
        Some(median_gap(days))
    } else {
        prev.and_then(|p| p.period)
    };
    let next = last.zip(period).map(|(last, period)| last + period);
    let missed = match (last, period) {
        (Some(last), Some(period)) if today > last => {
            (today - last - 1) / (period + setting.tolerance)
        }
        _ => 0,
    };
    let past_floor = last
        .and_then(|last| today.checked_sub(last))
        .is_some_and(|quiet| quiet >= setting.stale_floor);
    let mark = if setting.miss_cycles > 0 && missed >= setting.miss_cycles && past_floor {
        Mark::Stale
    } else {
        Mark::None
    };
    let prev_mark = prev.map_or(Mark::None, |p| p.mark);
    let since = if mark != prev_mark {
        Some(today)
    } else {
        prev.and_then(|p| p.since)
    };
    RoutineState {
        seen: days.len() as u32,
        first,
        last,
        period,
        next,
        missed,
        mark,
        since,
    }
}

pub fn window(days: &[u32], today: u32) -> &[u32] {
    let from = today.saturating_sub(WINDOW_DAYS - 1);
    let start = days.partition_point(|&d| d < from);
    let end = days.partition_point(|&d| d <= today);
    &days[start..end]
}

pub fn walk(setting: Setting, days: &[u32]) -> Vec<RoutineState> {
    let mut states: Vec<RoutineState> = Vec::with_capacity(TRAIN_DAYS as usize);
    for today in 1..=TRAIN_DAYS {
        let state = routine_step(setting, states.last(), window(days, today), today);
        states.push(state);
    }
    states
}

pub fn on_day(states: &[RoutineState], day: u32) -> &RoutineState {
    &states[(day - 1) as usize]
}
