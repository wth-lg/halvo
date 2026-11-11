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
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::Usage(detail) => write!(f, "REFUSED usage: {detail}"),
            Refusal::Seed(detail) => write!(f, "REFUSED seed: {detail}"),
            Refusal::Input(detail) => write!(f, "REFUSED input: {detail}"),
        }
    }
}

fn text_lines(text: &str) -> Option<Vec<&str>> {
    let body = text
        .strip_suffix('\n')
        .filter(|body| !body.starts_with('\u{feff}') && !body.contains('\r'))?;
    Some(body.split('\n').collect())
}

fn count(word: &str) -> Option<u32> {
    let digits = !word.is_empty()
        && word.bytes().all(|b| b.is_ascii_digit())
        && (word == "0" || !word.starts_with('0'));
    digits.then(|| word.parse().ok()).flatten()
}

fn is_name(word: &str) -> bool {
    !word.is_empty()
        && word
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Targets {
    entries: Vec<(String, String, u32)>,
}

impl Targets {
    pub fn key(&self, table: &str, key: &str) -> Result<u32, Refusal> {
        self.entries
            .iter()
            .find(|(t, k, _)| t == table && k == key)
            .map(|(_, _, v)| *v)
            .ok_or_else(|| Refusal::Input(format!("targets key {key} is absent")))
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
    let mut tables: Vec<&str> = Vec::new();
    let mut entries: Vec<(String, String, u32)> = Vec::new();
    for (i, line) in lines.into_iter().enumerate() {
        let bad = || refuse(format!("targets line {} is not of its form", i + 1));
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            if !is_name(name) || tables.contains(&name) {
                return Err(bad());
            }
            tables.push(name);
            continue;
        }
        let (key, value) = line.split_once(" = ").ok_or_else(bad)?;
        let table = tables.last().ok_or_else(bad)?;
        let value = count(value).ok_or_else(bad)?;
        if !is_name(key) || entries.iter().any(|(t, k, _)| t == table && k == key) {
            return Err(bad());
        }
        entries.push(((*table).to_owned(), key.to_owned(), value));
    }
    Ok(Targets { entries })
}
