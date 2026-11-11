use std::path::Path;

use crate::Refusal;

pub const TARGETS_PATH: &str = "docs/r5/targets.toml";

pub const LAYOUT: [(&str, &[&str]); 4] = [
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
pub struct Targets {
    pub dev_benign: u64,
    pub dev_fault: u64,
    pub dev_critical: u64,
    pub dev_cost_budget: u64,
    pub calendar_days_min: u64,
}

fn input(detail: String) -> Refusal {
    Refusal::Input(detail)
}

fn integer(text: &str) -> Option<u64> {
    let digits = !text.is_empty()
        && text.bytes().all(|b| b.is_ascii_digit())
        && (text == "0" || !text.starts_with('0'));
    if digits {
        text.parse().ok()
    } else {
        None
    }
}

enum Expected {
    Table(&'static str),
    Key(&'static str, &'static str),
}

pub fn parse(text: &str) -> Result<Targets, Refusal> {
    let body = text
        .strip_suffix('\n')
        .filter(|body| !body.starts_with('\u{feff}') && !body.contains('\r'))
        .ok_or_else(|| {
            input("the targets file is not LF text with one LF at its end".to_owned())
        })?;
    let lines: Vec<&str> = body.split('\n').collect();
    let mut expected = Vec::new();
    for (table, keys) in LAYOUT {
        expected.push(Expected::Table(table));
        expected.extend(keys.iter().map(|key| Expected::Key(table, key)));
    }
    let mut values: Vec<((&str, &str), u64)> = Vec::new();
    for (number, want) in (1usize..).zip(&expected) {
        let line = lines.get(number - 1).copied();
        match want {
            Expected::Table(table) => {
                if line != Some(format!("[{table}]").as_str()) {
                    return Err(input(format!("targets line {number} is not [{table}]")));
                }
            }
            Expected::Key(table, key) => {
                let value = line
                    .and_then(|line| line.strip_prefix(key))
                    .and_then(|rest| rest.strip_prefix(" = "))
                    .and_then(integer)
                    .ok_or_else(|| {
                        input(format!(
                            "targets line {number} is not {key} = <integer> of [{table}]"
                        ))
                    })?;
                values.push(((table, key), value));
            }
        }
    }
    if lines.len() != expected.len() {
        return Err(input(format!(
            "targets line {} is not of the form of L8.1",
            expected.len() + 1
        )));
    }
    let value = |key: &str| {
        values
            .iter()
            .find(|((table, name), _)| *table == "gate" && *name == key)
            .map(|(_, value)| *value)
            .unwrap_or_default()
    };
    Ok(Targets {
        dev_benign: value("dev_benign"),
        dev_fault: value("dev_fault"),
        dev_critical: value("dev_critical"),
        dev_cost_budget: value("dev_cost_budget"),
        calendar_days_min: value("calendar_days_min"),
    })
}

pub fn read(root: &Path) -> Result<Targets, Refusal> {
    let bytes = std::fs::read(root.join(TARGETS_PATH))
        .map_err(|_| input("the targets file cannot be read".to_owned()))?;
    let text =
        String::from_utf8(bytes).map_err(|_| input("the targets file is not UTF-8".to_owned()))?;
    parse(&text)
}
