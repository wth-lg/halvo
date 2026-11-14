//! Label rows (contracts.md C10.2 to C10.4): their writer, their reader and the rows that a script gives.

use halvo_schema::Category;
use serde::Deserialize;

use crate::form::json_str;
use crate::script::{Script, TaskKind};

/// One label row (C10.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    Episode {
        episode: String,
        task: String,
        category: Category,
        kind: TaskKind,
        start: i64,
        end: i64,
    },
    Range {
        episode: String,
        step: String,
        start: i64,
        end: i64,
    },
}

impl Row {
    /// The row as a line of labels.jsonl, without its LF.
    pub fn write(&self) -> String {
        match self {
            Row::Episode {
                episode,
                task,
                category,
                kind,
                start,
                end,
            } => format!(
                "{{\"row\":\"episode\",\"episode\":{},\"task\":{},\"category\":{},\"kind\":{},\"start\":{},\"end\":{}}}",
                json_str(episode),
                json_str(task),
                json_str(category.as_str()),
                json_str(kind.as_str()),
                start,
                end
            ),
            Row::Range {
                episode,
                step,
                start,
                end,
            } => format!(
                "{{\"row\":\"range\",\"episode\":{},\"step\":{},\"start\":{},\"end\":{}}}",
                json_str(episode),
                json_str(step),
                start,
                end
            ),
        }
    }

    /// The episode id of the row.
    pub fn episode(&self) -> &str {
        match self {
            Row::Episode { episode, .. } | Row::Range { episode, .. } => episode,
        }
    }

    /// The span of the row.
    pub fn span(&self) -> (i64, i64) {
        match self {
            Row::Episode { start, end, .. } | Row::Range { start, end, .. } => (*start, *end),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEpisode {
    row: String,
    episode: String,
    task: String,
    category: String,
    kind: String,
    start: i64,
    end: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRange {
    row: String,
    episode: String,
    step: String,
    start: i64,
    end: i64,
}

/// Reads one line of a label file, without its LF: one of the two rows of C10.3, compact, keys in order, with
/// ids of 0.7 and a known category and kind. The rules of C10.4 are not checked here.
pub fn parse_row(bytes: &[u8]) -> Option<Row> {
    let row = if bytes.starts_with(b"{\"row\":\"episode\",") {
        let r: RawEpisode = serde_json::from_slice(bytes).ok()?;
        (r.row == "episode").then_some(())?;
        Row::Episode {
            episode: r.episode,
            task: r.task,
            category: Category::parse(&r.category).ok()?,
            kind: TaskKind::parse(&r.kind)?,
            start: r.start,
            end: r.end,
        }
    } else if bytes.starts_with(b"{\"row\":\"range\",") {
        let r: RawRange = serde_json::from_slice(bytes).ok()?;
        (r.row == "range").then_some(())?;
        Row::Range {
            episode: r.episode,
            step: r.step,
            start: r.start,
            end: r.end,
        }
    } else {
        return None;
    };
    let ids_ok = match &row {
        Row::Episode { episode, task, .. } => {
            crate::form::is_id(episode) && crate::form::is_id(task)
        }
        Row::Range { episode, step, .. } => crate::form::is_id(episode) && crate::form::is_id(step),
    };
    if !ids_ok {
        return None;
    }
    (row.write().as_bytes() == bytes).then_some(row)
}

/// The label rows that a script gives for one of its streams, in the order of C10.4 rule 1: episodes by
/// `start`, then by episode id in byte order, each followed by its ranges in the order of `start`.
pub fn rows_of(script: &Script, stream: &str) -> Vec<Row> {
    let mut episodes: Vec<(i64, &str, Vec<Row>)> = Vec::new();
    for (_, e) in &script.episodes {
        if e.stream != stream {
            continue;
        }
        let ranges: Vec<Row> = e
            .ranges
            .iter()
            .map(|i| Row::Range {
                episode: e.episode.clone(),
                step: i.step.clone(),
                start: script.instant(e.day, i.start),
                end: script.instant(e.day, i.end),
            })
            .collect();
        let start = ranges[0].span().0;
        let end = ranges[ranges.len() - 1].span().1;
        let mut rows = vec![Row::Episode {
            episode: e.episode.clone(),
            task: e.task.clone(),
            category: e.category,
            kind: e.kind,
            start,
            end,
        }];
        rows.extend(ranges);
        episodes.push((start, e.episode.as_str(), rows));
    }
    episodes.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
    episodes.into_iter().flat_map(|(_, _, rows)| rows).collect()
}
