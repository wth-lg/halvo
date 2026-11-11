use crate::rule::{walk, Mark, RoutineState, Setting};
use crate::train::{Cell, Train};

pub const SAMPLE_TRAINS: [u32; 5] = [1, 2, 101, 102, 151];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    pub day: u32,
    pub mark: Mark,
    pub period: Option<u32>,
}

pub fn events(states: &[RoutineState]) -> Vec<Event> {
    let mut before = (Mark::None, None);
    let mut out = Vec::new();
    for (day, state) in (1..).zip(states) {
        let now = (state.mark, state.period);
        if now != before {
            out.push(Event {
                day,
                mark: state.mark,
                period: state.period,
            });
        }
        before = now;
    }
    out
}

fn join(words: impl Iterator<Item = String>) -> String {
    words.collect::<Vec<_>>().join(",")
}

pub fn line(cell: &Cell, train: &Train, events: &[Event]) -> String {
    let days = join(train.days.iter().map(u32::to_string));
    let events = join(events.iter().map(|e| {
        let period = e.period.map_or("null".to_owned(), |p| p.to_string());
        format!(
            "{{\"day\":{},\"mark\":\"{}\",\"period\":{}}}",
            e.day,
            e.mark.name(),
            period
        )
    }));
    format!(
        "{{\"cell\":\"{}\",\"train\":{},\"kind\":\"{}\",\"days\":[{days}],\"events\":[{events}]}}",
        cell.name(),
        train.number,
        train.kind.name()
    )
}

pub fn sample(cells: &[Cell], trains: &[Vec<Train>], setting: Setting) -> String {
    let mut text = String::new();
    for (cell, trains) in cells.iter().zip(trains) {
        for number in SAMPLE_TRAINS {
            let train = &trains[(number - 1) as usize];
            let states = walk(setting, &train.days);
            text.push_str(&line(cell, train, &events(&states)));
            text.push('\n');
        }
    }
    text
}
