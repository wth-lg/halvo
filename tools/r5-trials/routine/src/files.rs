use std::fmt;
use std::path::Path;

use crate::rule::Setting;
use crate::train::{grid, Cadence, Cell};
use crate::trial::Lines;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    Usage(String),
    Seed(String),
    Input(String),
    Output(String),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::Usage(detail) => write!(f, "REFUSED usage: {detail}"),
            Refusal::Seed(detail) => write!(f, "REFUSED seed: {detail}"),
            Refusal::Input(detail) => write!(f, "REFUSED input: {detail}"),
            Refusal::Output(detail) => write!(f, "REFUSED output: {detail}"),
        }
    }
}

fn text_lines(text: &str) -> Option<Vec<&str>> {
    let body = text
        .strip_suffix('\n')
        .filter(|body| !body.starts_with('\u{feff}') && !body.contains('\r'))?;
    Some(body.split('\n').collect())
}

fn integer(word: &str) -> Option<u64> {
    let digits = !word.is_empty()
        && word.bytes().all(|b| b.is_ascii_digit())
        && (word == "0" || !word.starts_with('0'));
    digits.then(|| word.parse().ok()).flatten()
}

fn count(word: &str) -> Option<u32> {
    integer(word).and_then(|n| u32::try_from(n).ok())
}

pub fn read_text(path: &Path) -> Option<String> {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
}

fn seed_line(line: &str) -> Option<Cell> {
    let words: Vec<&str> = line.split(' ').collect();
    let [cadence, jitter, miss_pct, seed] = words.as_slice() else {
        return None;
    };
    Some(Cell {
        cadence: Cadence::from_days(count(cadence)?)?,
        jitter: count(jitter)?,
        miss_pct: count(miss_pct)?,
        seed: u64::from(count(seed)?),
    })
}

pub fn parse_seeds(text: Option<&str>) -> Result<Vec<Cell>, Refusal> {
    let refuse = |detail: String| Refusal::Seed(detail);
    let text = text.ok_or_else(|| refuse("the seeds file cannot be read".to_owned()))?;
    let lines = text_lines(text)
        .ok_or_else(|| refuse("the seeds file is not LF text with one LF at its end".to_owned()))?;
    let mut cells = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let cell = seed_line(line)
            .ok_or_else(|| refuse(format!("seeds line {} is not of its form", i + 1)))?;
        cells.push(cell);
    }
    if cells != grid() {
        return Err(refuse(
            "the seeds file is not the grid of the trial".to_owned(),
        ));
    }
    Ok(cells)
}

pub const TARGETS_FORM: [(&str, &[&str]); 4] = [
    (
        "rule",
        &["k_clean", "locators_max", "edits_max", "value_min"],
    ),
    (
        "gate",
        &[
            "dev_benign",
            "dev_fault",
            "dev_critical",
            "dev_cost_budget",
            "heldout_benign",
            "heldout_fault",
            "heldout_critical",
            "heldout_cost_budget",
            "calendar_days_min",
        ],
    ),
    (
        "routine",
        &[
            "miss_cycles",
            "stale_floor",
            "tolerance",
            "false_stale_max",
            "detected_min",
            "detect_days",
        ],
    ),
    ("memory", &["increment_bytes_max", "host_bytes_max"]),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Expected {
    Table(&'static str),
    Key(&'static str, &'static str),
}

fn expected_lines() -> Vec<Expected> {
    TARGETS_FORM
        .iter()
        .flat_map(|&(table, keys)| {
            std::iter::once(Expected::Table(table))
                .chain(keys.iter().map(move |&key| Expected::Key(table, key)))
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Targets {
    entries: Vec<(&'static str, &'static str, u64)>,
}

impl Targets {
    pub fn key(&self, table: &str, key: &str) -> Result<u32, Refusal> {
        let value = self
            .entries
            .iter()
            .find(|(t, k, _)| *t == table && *k == key)
            .map(|(_, _, v)| *v)
            .ok_or_else(|| Refusal::Input(format!("targets key {key} is absent")))?;
        u32::try_from(value)
            .map_err(|_| Refusal::Input(format!("targets key {key} is out of range")))
    }

    pub fn lines(&self) -> Result<Lines, Refusal> {
        Ok(Lines {
            false_stale_max: self.key("routine", "false_stale_max")?,
            detected_min: self.key("routine", "detected_min")?,
            detect_days: self.key("routine", "detect_days")?,
        })
    }

    pub fn setting(&self) -> Result<Setting, Refusal> {
        Ok(Setting::new(
            self.key("routine", "miss_cycles")?,
            self.key("routine", "stale_floor")?,
            self.key("routine", "tolerance")?,
        ))
    }
}

pub fn parse_targets(text: Option<&str>) -> Result<Targets, Refusal> {
    let refuse = |detail: String| Refusal::Input(detail);
    let text = text.ok_or_else(|| refuse("the targets file cannot be read".to_owned()))?;
    let lines = text_lines(text).ok_or_else(|| {
        refuse("the targets file is not LF text with one LF at its end".to_owned())
    })?;
    let expected = expected_lines();
    let mut entries = Vec::new();
    for (i, want) in expected.iter().enumerate() {
        let line = lines.get(i).copied();
        let ok = match (*want, line) {
            (Expected::Table(table), Some(line)) => line == format!("[{table}]"),
            (Expected::Key(table, key), Some(line)) => {
                match line
                    .strip_prefix(key)
                    .and_then(|rest| rest.strip_prefix(" = "))
                    .and_then(integer)
                {
                    Some(value) => {
                        entries.push((table, key, value));
                        true
                    }
                    None => false,
                }
            }
            (_, None) => false,
        };
        if !ok {
            let what = match want {
                Expected::Table(table) => format!("the table header [{table}]"),
                Expected::Key(table, key) => format!("the key {key} of [{table}]"),
            };
            return Err(refuse(format!("targets line {} is not {what}", i + 1)));
        }
    }
    if lines.len() != expected.len() {
        return Err(refuse(format!(
            "targets holds {} lines, not {}",
            lines.len(),
            expected.len()
        )));
    }
    Ok(Targets { entries })
}
