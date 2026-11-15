use crate::cli::{Cause, Refusal};

/// The path of the targets file under the repository root, where the commands are run.
pub const TARGETS_PATH: &str = "docs/r5/targets.toml";

/// The keys of `docs/r5/targets.toml` (contracts.md L8.1), each table with its keys in the order of the file.
pub const TABLES: &[(&str, &[&str])] = &[
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

/// The values of the targets file, one per key of `TABLES`, in the same order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Targets {
    values: Vec<u64>,
}

impl Targets {
    /// The value of `table.key`; a key that `TABLES` does not hold is a fault of the caller.
    pub fn get(&self, table: &str, key: &str) -> u64 {
        let at = TABLES
            .iter()
            .flat_map(|(t, keys)| keys.iter().map(move |k| (*t, *k)))
            .position(|(t, k)| t == table && k == key)
            .unwrap_or_else(|| panic!("the targets file has no key {table}.{key}"));
        self.values[at]
    }
}

fn integer(text: &str) -> Option<u64> {
    let digits = !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
    let leading_zero = text.len() > 1 && text.starts_with('0');
    if !digits || leading_zero {
        return None;
    }
    text.parse().ok()
}

/// Reads the targets file of L8.1 from its bytes, or refuses it with `targets`: a text file of L0.4 whose lines
/// are, in the order of `TABLES`, each table header `[name]` followed by one line `<key> = <integer>` per key.
pub fn parse(bytes: &[u8]) -> Result<Targets, Refusal> {
    let refuse = |detail: String| Refusal::new(Cause::Targets, detail);
    let text = std::str::from_utf8(bytes)
        .ok()
        .and_then(|t| t.strip_suffix('\n'))
        .filter(|t| !t.starts_with('\u{feff}') && !t.contains('\r'))
        .ok_or_else(|| refuse("docs/r5/targets.toml is not a text file of L0.4".into()))?;
    let mut lines = text.split('\n');
    let mut values = Vec::new();
    for (table, keys) in TABLES {
        let header = format!("[{table}]");
        if lines.next() != Some(header.as_str()) {
            return Err(refuse(format!("the line {header} is not in its place")));
        }
        for key in *keys {
            let value = lines
                .next()
                .and_then(|line| line.strip_prefix(*key))
                .and_then(|rest| rest.strip_prefix(" = "))
                .and_then(integer)
                .ok_or_else(|| {
                    refuse(format!(
                        "the line of {table}.{key} is not {key} = <integer>"
                    ))
                })?;
            values.push(value);
        }
    }
    if lines.next().is_some() {
        return Err(refuse("a line after the last key".into()));
    }
    Ok(Targets { values })
}

/// Reads `docs/r5/targets.toml` under the current directory, or refuses with `targets`.
pub fn read() -> Result<Targets, Refusal> {
    let bytes = std::fs::read(TARGETS_PATH)
        .map_err(|_| Refusal::new(Cause::Targets, "docs/r5/targets.toml cannot be read"))?;
    parse(&bytes)
}
