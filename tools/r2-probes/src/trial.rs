use crate::text::median;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::Write;
use std::path::Path;

pub const TIME_LINE_MS: f64 = 172.0;
pub const BYTE_LINE: f64 = 78_000.0;
pub const D3_POINTS: f64 = 2.0;
pub const RUNNERS: [&str; 2] = ["macos-15", "windows-2025"];

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum How {
    Median,
    Max,
    Min,
    Sum,
    Text,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Field {
    pub key: String,
    pub how: How,
    pub value: Value,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Record {
    pub cell: String,
    pub run: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_measurable: Option<String>,
    #[serde(default)]
    pub fields: Vec<Field>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub detail: Value,
}

impl Record {
    pub fn new(cell: &str, run: u32) -> Record {
        Record {
            cell: cell.to_string(),
            run,
            not_measurable: None,
            fields: Vec::new(),
            detail: Value::Null,
        }
    }

    pub fn missing(cell: &str, reason: &str) -> Record {
        let mut record = Record::new(cell, 0);
        record.not_measurable = Some(crate::text::fold(reason));
        record
    }

    pub fn number(mut self, key: &str, how: How, value: f64) -> Record {
        self.fields.push(Field {
            key: key.to_string(),
            how,
            value: serde_json::json!(value),
        });
        self
    }

    pub fn text(mut self, key: &str, value: &str) -> Record {
        self.fields.push(Field {
            key: key.to_string(),
            how: How::Text,
            value: Value::String(value.to_string()),
        });
        self
    }

    pub fn with_detail(mut self, detail: Value) -> Record {
        self.detail = detail;
        self
    }

    pub fn renamed(&self, cell: &str, run: u32) -> Record {
        let mut copy = self.clone();
        copy.cell = cell.to_string();
        copy.run = run;
        copy.detail = Value::Null;
        copy
    }
}

pub fn append(path: &Path, record: &Record) -> Result<(), String> {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| format!("open {}: {e}", path.display()))?;
    let line = serde_json::to_string(record).map_err(|e| e.to_string())?;
    writeln!(file, "{line}").map_err(|e| e.to_string())
}

pub fn read(path: &Path) -> Result<Vec<Record>, String> {
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).map_err(|e| format!("{}: {e}", path.display())))
        .collect()
}

#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Measured {
        runs: usize,
        values: Vec<(String, String)>,
    },
    NotMeasurable(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    pub name: String,
    pub outcome: Outcome,
}

impl Cell {
    pub fn line(&self) -> String {
        match &self.outcome {
            Outcome::NotMeasurable(reason) => {
                format!("cell {} not measurable: {reason}", self.name)
            }
            Outcome::Measured { runs, values } => {
                let mut line = format!("cell {} runs={runs}", self.name);
                for (key, value) in values {
                    line.push_str(&format!(" {key}={value}"));
                }
                line
            }
        }
    }

    pub fn measured(&self) -> bool {
        matches!(self.outcome, Outcome::Measured { .. })
    }

    pub fn runs(&self) -> Option<usize> {
        match &self.outcome {
            Outcome::Measured { runs, .. } => Some(*runs),
            Outcome::NotMeasurable(_) => None,
        }
    }

    pub fn text(&self, key: &str) -> Option<&str> {
        match &self.outcome {
            Outcome::Measured { values, .. } => values
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value.as_str()),
            Outcome::NotMeasurable(_) => None,
        }
    }

    pub fn number(&self, key: &str) -> Option<f64> {
        self.text(key)?.parse().ok()
    }
}

fn format_number(key: &str, value: f64) -> String {
    if key.ends_with("_ms") {
        format!("{value:.1}")
    } else if key.ends_with("_pct") {
        format!("{value:.2}")
    } else if value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        format!("{value:.1}")
    }
}

fn text_of(value: &Value) -> String {
    match value {
        Value::String(text) => text.replace(' ', "_"),
        other => other.to_string(),
    }
}

pub fn aggregate(records: &[Record]) -> Vec<Cell> {
    let mut names: Vec<&str> = Vec::new();
    for record in records {
        if !names.contains(&record.cell.as_str()) {
            names.push(&record.cell);
        }
    }
    names
        .into_iter()
        .map(|name| {
            let group: Vec<&Record> = records.iter().filter(|r| r.cell == name).collect();
            if let Some(reason) = group.iter().find_map(|r| r.not_measurable.clone()) {
                return Cell {
                    name: name.to_string(),
                    outcome: Outcome::NotMeasurable(reason),
                };
            }
            let mut keys: Vec<(&str, How)> = Vec::new();
            for record in &group {
                for field in &record.fields {
                    if !keys.iter().any(|(key, _)| *key == field.key) {
                        keys.push((&field.key, field.how));
                    }
                }
            }
            let values = keys
                .into_iter()
                .map(|(key, how)| {
                    let found: Vec<&Value> = group
                        .iter()
                        .flat_map(|r| r.fields.iter())
                        .filter(|f| f.key == key)
                        .map(|f| &f.value)
                        .collect();
                    let numbers: Vec<f64> = found.iter().filter_map(|v| v.as_f64()).collect();
                    let value = match how {
                        How::Text => {
                            let mut seen: Vec<String> = Vec::new();
                            for value in found {
                                let text = text_of(value);
                                if !seen.contains(&text) {
                                    seen.push(text);
                                }
                            }
                            seen.join("+")
                        }
                        How::Median => format_number(key, median(&numbers).unwrap_or(f64::NAN)),
                        How::Max => format_number(
                            key,
                            numbers.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                        ),
                        How::Min => format_number(
                            key,
                            numbers.iter().copied().fold(f64::INFINITY, f64::min),
                        ),
                        How::Sum => format_number(key, numbers.iter().sum()),
                    };
                    (key.to_string(), value)
                })
                .collect();
            Cell {
                name: name.to_string(),
                outcome: Outcome::Measured {
                    runs: group.len(),
                    values,
                },
            }
        })
        .collect()
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct HostedRun {
    pub branch: String,
    pub commit: String,
    pub run: String,
    pub attempt: String,
}

impl HostedRun {
    pub fn line(&self) -> String {
        format!(
            "hosted-run branch={} commit={} run={} attempt={}",
            self.branch, self.commit, self.run, self.attempt
        )
    }
}

pub fn render(
    cells: &[Cell],
    hosted: &[HostedRun],
    other_lines: &[String],
    verdicts: &[String],
) -> String {
    let names: Vec<&str> = cells.iter().map(|cell| cell.name.as_str()).collect();
    let mut lines = vec![format!("cells: {}", names.join(" "))];
    lines.extend(hosted.iter().map(HostedRun::line));
    lines.extend(cells.iter().map(Cell::line));
    lines.extend(other_lines.iter().cloned());
    lines.extend(verdicts.iter().cloned());
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

fn find<'a>(cells: &'a [Cell], name: &str) -> Option<&'a Cell> {
    cells.iter().find(|cell| cell.name == name)
}

struct Engine {
    cell_name: String,
    runner: &'static str,
    value: String,
    holds: bool,
    error_pct: Option<f64>,
}

fn timed(cells: &[Cell], runner: &'static str, cell_name: &str) -> Option<Engine> {
    let hosted_name = format!("engine.{runner}.{cell_name}.x1.0");
    let hosted = find(cells, &hosted_name)?;
    let hosted_ms = hosted.number("call_ms")?;
    let (value, holds) = if hosted_ms <= TIME_LINE_MS {
        ("holds".to_string(), true)
    } else {
        let second =
            find(cells, &format!("{hosted_name}.this-mac")).and_then(|cell| cell.number("call_ms"));
        match second {
            Some(ms) if ms <= TIME_LINE_MS => ("holds re-timed".to_string(), true),
            Some(_) => ("over re-timed".to_string(), false),
            None => ("over not re-timed".to_string(), false),
        }
    };
    Some(Engine {
        cell_name: cell_name.to_string(),
        runner,
        value,
        holds,
        error_pct: hosted.number("cer_pct"),
    })
}

fn first_that_holds(cells: &[Cell], runner: &'static str, cell_names: &[&str]) -> Option<Engine> {
    let mut measured: Vec<Engine> = cell_names
        .iter()
        .filter_map(|name| timed(cells, runner, name))
        .collect();
    match measured.iter().position(|engine| engine.holds) {
        Some(index) => Some(measured.swap_remove(index)),
        None if measured.is_empty() => None,
        None => Some(measured.swap_remove(0)),
    }
}

fn byte_line(cells: &[Cell], prefixes: &[String]) -> &'static str {
    let group: Vec<&Cell> = cells
        .iter()
        .filter(|cell| prefixes.iter().any(|prefix| cell.name.starts_with(prefix)))
        .collect();
    let over = group
        .iter()
        .filter_map(|cell| cell.number("stored_bytes"))
        .any(|bytes| bytes > BYTE_LINE);
    let every_prefix_measured = prefixes.iter().all(|prefix| {
        group
            .iter()
            .any(|cell| cell.name.starts_with(prefix) && cell.measured())
    });
    if over {
        "leaves"
    } else if !every_prefix_measured || group.iter().any(|cell| !cell.measured()) {
        "not measurable"
    } else {
        "stays"
    }
}

pub fn layer_verdicts(cells: &[Cell]) -> Vec<String> {
    let apple = first_that_holds(
        cells,
        "macos-15",
        &["apple-vision-accurate", "apple-vision-fast"],
    );
    let windows = first_that_holds(cells, "windows-2025", &["windows-media-ocr"]);
    let one_mac = first_that_holds(cells, "macos-15", &["pp-ocrv6-tiny"]);
    let one_windows = first_that_holds(cells, "windows-2025", &["pp-ocrv6-tiny"]);
    let mut lines = Vec::new();
    for (name, engine) in [
        ("apple-vision", &apple),
        ("windows-media-ocr", &windows),
        ("pp-ocrv6-tiny", &one_mac),
        ("pp-ocrv6-tiny", &one_windows),
    ] {
        if let Some(engine) = engine {
            lines.push(format!(
                "verdict engine {name} {}: {}",
                engine.runner, engine.value
            ));
        }
    }

    let os_pair = match (&apple, &windows) {
        (Some(a), Some(w)) if a.holds && w.holds => match (a.error_pct, w.error_pct) {
            (Some(x), Some(y)) => (x - y).abs() <= D3_POINTS,
            _ => false,
        },
        _ => false,
    };
    lines.push(format!(
        "verdict D3: {}",
        if os_pair { "os-pair" } else { "pp-ocrv6-tiny" }
    ));

    let address: Vec<&Cell> = cells
        .iter()
        .filter(|cell| cell.name.starts_with("address."))
        .collect();
    let fails = address.iter().any(|cell| {
        cell.measured() && cell.number("readable_runs") != cell.runs().map(|runs| runs as f64)
    });
    let every_runner_measured = RUNNERS.iter().all(|runner| {
        address
            .iter()
            .any(|cell| cell.name.starts_with(&format!("address.{runner}.")) && cell.measured())
    });
    let site = if fails {
        "leaves"
    } else if !every_runner_measured || address.iter().any(|cell| !cell.measured()) {
        "not measurable"
    } else {
        "stays"
    };
    lines.push(format!("verdict layer site: {site}"));

    let prefixes: Vec<String> = RUNNERS
        .iter()
        .map(|runner| format!("accessibility.{runner}."))
        .collect();
    lines.push(format!(
        "verdict layer accessibility_text: {}",
        byte_line(cells, &prefixes)
    ));

    let named = if os_pair {
        [&apple, &windows]
    } else {
        [&one_mac, &one_windows]
    };
    let extracted = if named.iter().any(|engine| engine.is_none()) {
        "not measurable"
    } else if named.iter().flat_map(|e| e.iter()).any(|e| !e.holds) {
        "leaves"
    } else {
        let prefixes: Vec<String> = named
            .iter()
            .flat_map(|e| e.iter())
            .map(|e| format!("extract.{}.{}.", e.runner, e.cell_name))
            .collect();
        byte_line(cells, &prefixes)
    };
    lines.push(format!("verdict layer extracted_text: {extracted}"));
    lines
}

pub fn input_verdicts(cells: &[Cell]) -> Vec<String> {
    let counted = |driver: &str| {
        find(cells, &format!("input.windows-2025.{driver}")).is_some_and(|cell| {
            cell.number("keys") == Some(20.0) && cell.number("clicks") == Some(5.0)
        })
    };
    let driver = if counted("playwright") {
        "playwright"
    } else if counted("enigo") {
        "enigo"
    } else {
        "none"
    };
    vec![format!("verdict driver: {driver}")]
}

pub fn private_verdicts(cells: &[Cell]) -> Vec<String> {
    let mut lines = Vec::new();
    for browser in ["chrome", "edge", "firefox", "safari"] {
        for runner in RUNNERS {
            let Some(cell) = find(cells, &format!("private.{runner}.{browser}")) else {
                continue;
            };
            let value = if !cell.measured() {
                "not measurable"
            } else if matches!(cell.text("signal"), Some("attribute" | "class" | "title")) {
                "readable"
            } else {
                "not-readable"
            };
            lines.push(format!(
                "verdict private-window {browser} {runner}: {value}"
            ));
        }
    }
    lines
}
