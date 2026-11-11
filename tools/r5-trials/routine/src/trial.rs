use crate::rule::{on_day, walk, Mark, RoutineState, Setting, SETTINGS};
use crate::train::{Cell, Kind, Train, TRAIN_DAYS};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lines {
    pub false_stale_max: u32,
    pub detected_min: u32,
    pub detect_days: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellCount {
    pub cell: Cell,
    pub false_stale: u32,
    pub detected: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingResult {
    pub setting: Setting,
    pub cells: Vec<CellCount>,
    pub worst_false_stale: u32,
    pub worst_detected: u32,
    pub meets: bool,
}

pub fn false_stale(states: &[RoutineState]) -> u32 {
    let mut before = Mark::None;
    let mut turns = 0;
    for state in states {
        if before == Mark::None && state.mark == Mark::Stale {
            turns += 1;
        }
        before = state.mark;
    }
    turns
}

pub fn detected(states: &[RoutineState], days: &[u32], detect_days: u32) -> bool {
    days.last().is_some_and(|&last| {
        (last..=last.saturating_add(detect_days))
            .take_while(|&day| day <= TRAIN_DAYS)
            .any(|day| on_day(states, day).mark == Mark::Stale)
    })
}

pub fn count_cell(setting: Setting, cell: Cell, trains: &[Train], detect_days: u32) -> CellCount {
    let mut count = CellCount {
        cell,
        false_stale: 0,
        detected: 0,
    };
    for train in trains {
        let states = walk(setting, &train.days);
        match train.kind {
            Kind::Same | Kind::Shift => count.false_stale += false_stale(&states),
            Kind::Stop => count.detected += u32::from(detected(&states, &train.days, detect_days)),
        }
    }
    count
}

pub fn judge(setting: Setting, cells: Vec<CellCount>, lines: Lines) -> SettingResult {
    let worst_false_stale = cells.iter().map(|c| c.false_stale).max().unwrap_or(0);
    let worst_detected = cells.iter().map(|c| c.detected).min().unwrap_or(0);
    SettingResult {
        setting,
        cells,
        worst_false_stale,
        worst_detected,
        meets: worst_false_stale <= lines.false_stale_max && worst_detected >= lines.detected_min,
    }
}

pub fn verdict(results: &[SettingResult]) -> Option<Setting> {
    results.iter().find(|r| r.meets).map(|r| r.setting)
}

pub fn results(cells: &[Cell], trains: &[Vec<Train>], lines: Lines) -> Vec<SettingResult> {
    SETTINGS
        .iter()
        .map(|&setting| {
            let counts = cells
                .iter()
                .zip(trains)
                .map(|(&cell, trains)| count_cell(setting, cell, trains, lines.detect_days))
                .collect();
            judge(setting, counts, lines)
        })
        .collect()
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

pub fn block(results: &[SettingResult]) -> String {
    let mut text = String::new();
    for result in results {
        for count in &result.cells {
            text.push_str(&format!(
                "cell setting={} cadence={} jitter={} miss_pct={} false_stale={} detected={}\n",
                result.setting.name(),
                count.cell.cadence.days(),
                count.cell.jitter,
                count.cell.miss_pct,
                count.false_stale,
                count.detected
            ));
        }
    }
    for result in results {
        text.push_str(&format!(
            "setting={} worst_false_stale={} worst_detected={} meets={}\n",
            result.setting.name(),
            result.worst_false_stale,
            result.worst_detected,
            yes_no(result.meets)
        ));
    }
    text.push_str(&match verdict(results) {
        Some(s) => format!(
            "verdict rule=cadence miss_cycles={} stale_floor={} tolerance={}\n",
            s.miss_cycles, s.stale_floor, s.tolerance
        ),
        None => "verdict rule=none miss_cycles=- stale_floor=- tolerance=-\n".to_owned(),
    });
    text
}
