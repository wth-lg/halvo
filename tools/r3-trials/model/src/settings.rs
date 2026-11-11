use crate::wire::Level;

/// The token limits of one level (contracts.md A12.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub action_tokens: u32,
    pub reply_tokens: u32,
    pub recoveries: u32,
}

/// The settings file of A12.5, with exactly its keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub driver: String,
    pub writer: String,
    pub context: u32,
    pub snapshot_tokens: u32,
    pub low: Limits,
    pub medium: Limits,
    pub high: Limits,
}

impl Settings {
    pub fn limits(&self, level: Level) -> Limits {
        match level {
            Level::Low => self.low,
            Level::Medium => self.medium,
            Level::High => self.high,
        }
    }
}

const TOP: [&str; 7] = [
    "driver",
    "writer",
    "context",
    "snapshot_tokens",
    "low",
    "medium",
    "high",
];
const LEVEL: [&str; 3] = ["action_tokens", "reply_tokens", "recoveries"];

fn only_keys(table: &toml::Table, keys: &[&str], prefix: &str) -> Result<(), String> {
    match table.keys().find(|key| !keys.contains(&key.as_str())) {
        Some(key) => Err(format!("{prefix}{key} is not a key of the settings")),
        None => Ok(()),
    }
}

fn integer(table: &toml::Table, key: &str, prefix: &str, positive: bool) -> Result<u32, String> {
    let value = table
        .get(key)
        .ok_or_else(|| format!("{prefix}{key} is missing"))?;
    let n = value
        .as_integer()
        .ok_or_else(|| format!("{prefix}{key} is not an integer"))?;
    let low = i64::from(positive);
    if n < low || n > i64::from(u32::MAX) {
        let kind = if positive { "positive" } else { "non-negative" };
        return Err(format!("{prefix}{key} is not a {kind} integer below 2^32"));
    }
    Ok(n as u32)
}

fn token(table: &toml::Table, key: &str) -> Result<String, String> {
    let text = table
        .get(key)
        .ok_or_else(|| format!("{key} is missing"))?
        .as_str()
        .ok_or_else(|| format!("{key} is not a string"))?;
    let ok = !text.is_empty()
        && text.len() <= 32
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    if ok {
        Ok(text.to_string())
    } else {
        Err(format!("{key} is not a model token or none"))
    }
}

fn level(table: &toml::Table, key: &str) -> Result<Limits, String> {
    let level = table
        .get(key)
        .ok_or_else(|| format!("{key} is missing"))?
        .as_table()
        .ok_or_else(|| format!("{key} is not a table"))?;
    let prefix = format!("{key}.");
    only_keys(level, &LEVEL, &prefix)?;
    Ok(Limits {
        action_tokens: integer(level, "action_tokens", &prefix, true)?,
        reply_tokens: integer(level, "reply_tokens", &prefix, true)?,
        recoveries: integer(level, "recoveries", &prefix, false)?,
    })
}

pub fn parse(text: &str) -> Result<Settings, String> {
    let table: toml::Table = text
        .parse()
        .map_err(|e| format!("settings is no TOML file: {e}"))?;
    only_keys(&table, &TOP, "")?;
    Ok(Settings {
        driver: token(&table, "driver")?,
        writer: token(&table, "writer")?,
        context: integer(&table, "context", "", true)?,
        snapshot_tokens: integer(&table, "snapshot_tokens", "", true)?,
        low: level(&table, "low")?,
        medium: level(&table, "medium")?,
        high: level(&table, "high")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const START: &str = include_str!("../settings.toml");

    #[test]
    fn the_committed_file_holds_the_starting_values() {
        let limits = Limits {
            action_tokens: 256,
            reply_tokens: 512,
            recoveries: 2,
        };
        assert_eq!(
            parse(START),
            Ok(Settings {
                driver: "none".to_string(),
                writer: "none".to_string(),
                context: 8192,
                snapshot_tokens: 4096,
                low: limits,
                medium: limits,
                high: limits,
            })
        );
    }

    #[test]
    fn each_level_has_its_own_limits() {
        let text = START.replacen("reply_tokens = 512", "reply_tokens = 100", 1);
        let settings = parse(&text).unwrap();
        assert_eq!(settings.limits(Level::Low).reply_tokens, 100);
        assert_eq!(settings.limits(Level::Medium).reply_tokens, 512);
        assert_eq!(settings.limits(Level::High).reply_tokens, 512);
    }

    #[test]
    fn a_file_with_another_set_of_keys_is_refused() {
        let cases = [
            (
                START.replace("driver = \"none\"\n", ""),
                "driver is missing",
            ),
            (
                START.replace("context = 8192", "context = 8192\nthreads = 4"),
                "threads is not a key of the settings",
            ),
            (
                START.replace("context = 8192", "context = \"8192\""),
                "context is not an integer",
            ),
            (
                START.replace("context = 8192", "context = 0"),
                "context is not a positive integer below 2^32",
            ),
            (
                START.replace("snapshot_tokens = 4096", "snapshot_tokens = -1"),
                "snapshot_tokens is not a positive integer below 2^32",
            ),
            (
                START.replacen("recoveries = 2", "recoveries = 2\ntemperature = 1", 1),
                "low.temperature is not a key of the settings",
            ),
            (
                START.replacen("action_tokens = 256\n", "", 1),
                "low.action_tokens is missing",
            ),
            (
                START.replace(
                    "[high]\naction_tokens = 256\nreply_tokens = 512\nrecoveries = 2\n",
                    "",
                ),
                "high is missing",
            ),
            (
                START.replace("driver = \"none\"", "driver = \"Big Model\""),
                "driver is not a model token or none",
            ),
            (START.replace("[low]", "[low"), "settings is no TOML file"),
        ];
        for (text, fault) in cases {
            let got = parse(&text).map(|_| ()).map_err(|e| {
                if e.starts_with("settings is no TOML file") {
                    "settings is no TOML file".to_string()
                } else {
                    e
                }
            });
            assert_eq!(got, Err(fault.to_string()), "{fault}");
        }
    }

    #[test]
    fn recoveries_may_be_zero() {
        let text = START.replace("recoveries = 2", "recoveries = 0");
        assert_eq!(parse(&text).unwrap().limits(Level::High).recoveries, 0);
    }
}
