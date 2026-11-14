//! `check-labels` (contracts.md C12.6): reads a body's batches, label rows and script manifest, and nothing that
//! the generator holds in memory, and checks the rows, the ranges, the title events outside them, the marks and
//! the password ranges.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use halvo_schema::Category;
use halvo_seam::record::Edge;

use crate::body::{read_stream, stream_dirs, StreamRecords};
use crate::form::{text_lines, Refusal};
use crate::labels::{parse_row, Row};
use crate::manifest::{parse_script_manifest, StepLine, TierManifest};
use crate::script::TaskKind;

/// The rule under which the ranges are checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rule {
    Generated,
    Recorded,
}

impl Rule {
    /// The written name.
    pub fn as_str(self) -> &'static str {
        match self {
            Rule::Generated => "generated",
            Rule::Recorded => "recorded",
        }
    }

    /// The value of a written name.
    pub fn parse(s: &str) -> Option<Rule> {
        match s {
            "generated" => Some(Rule::Generated),
            "recorded" => Some(Rule::Recorded),
            _ => None,
        }
    }
}

/// What `check-labels` found: the lines it prints and whether every check held.
pub struct Checked {
    pub lines: Vec<String>,
    pub pass: bool,
}

type OpenEpisode<'a> = Option<(usize, &'a Row, Vec<(usize, i64, i64)>)>;

struct Stream {
    id: String,
    rows: Vec<(usize, Row)>,
    records: StreamRecords,
}

fn row_fail(id: &str, line: usize, token: &str) -> String {
    format!("FAIL row {id} {line} {token}")
}

/// The FAIL lines of the six rules of C10.4 for one stream, with the episode ids and the task ids that the
/// earlier streams hold.
fn check_rows(
    s: &Stream,
    steps: &BTreeSet<&str>,
    episodes_seen: &mut BTreeSet<String>,
    tasks_seen: &mut BTreeMap<String, String>,
) -> Vec<String> {
    let mut fails: BTreeMap<usize, u8> = BTreeMap::new();
    let mut mark = |line: usize, rule: u8| {
        let e = fails.entry(line).or_insert(rule);
        *e = (*e).min(rule);
    };
    let mut current: OpenEpisode = None;
    let mut last_episode: Option<(i64, String)> = None;
    let mut task_facts: BTreeMap<&str, (Category, TaskKind)> = BTreeMap::new();
    let mut ranges: Vec<(i64, i64, usize)> = Vec::new();
    let close = |cur: &OpenEpisode, mark: &mut dyn FnMut(usize, u8)| {
        if let Some((line, row, rs)) = cur {
            let (start, end) = row.span();
            match (rs.first(), rs.last()) {
                (Some(first), Some(last)) => {
                    if first.1 != start {
                        mark(first.0, 2);
                    }
                    if last.2 != end {
                        mark(last.0, 2);
                    }
                }
                _ => mark(*line, 2),
            }
        }
    };
    for (line, row) in &s.rows {
        let (start, end) = row.span();
        if start >= end {
            mark(*line, 2);
        }
        match row {
            Row::Episode {
                episode,
                task,
                category,
                kind,
                ..
            } => {
                close(&current, &mut mark);
                if let Some((ps, pe)) = &last_episode {
                    if (start, episode.as_str()) <= (*ps, pe.as_str()) {
                        mark(*line, 1);
                    }
                }
                last_episode = Some((start, episode.clone()));
                if !episodes_seen.insert(episode.clone()) {
                    mark(*line, 5);
                }
                match tasks_seen.get(task) {
                    Some(owner) if *owner != s.id => mark(*line, 5),
                    _ => {
                        tasks_seen.insert(task.clone(), s.id.clone());
                    }
                }
                match task_facts.get(task.as_str()) {
                    Some(f) if *f != (*category, *kind) => mark(*line, 4),
                    Some(_) => {}
                    None => {
                        task_facts.insert(task, (*category, *kind));
                    }
                }
                current = Some((*line, row, Vec::new()));
            }
            Row::Range {
                episode,
                step,
                start,
                end,
            } => {
                if !steps.contains(step.as_str()) {
                    mark(*line, 6);
                }
                match &mut current {
                    Some((_, erow, rs)) if erow.episode() == episode => {
                        if rs.last().is_some_and(|p| p.1 >= *start) {
                            mark(*line, 1);
                        }
                        rs.push((*line, *start, *end));
                    }
                    _ => mark(*line, 1),
                }
                ranges.push((*start, *end, *line));
            }
        }
    }
    close(&current, &mut mark);
    ranges.sort();
    let mut reach: Option<(i64, usize)> = None;
    for (start, end, line) in &ranges {
        if let Some((far, far_line)) = reach {
            if *start < far {
                mark((*line).max(far_line), 3);
            }
        }
        if reach.is_none_or(|(far, _)| *end > far) {
            reach = Some((*end, *line));
        }
    }
    let token = |rule: u8| match rule {
        1 => "order",
        2 => "span",
        3 => "overlap",
        _ => "reference",
    };
    fails
        .into_iter()
        .map(|(line, rule)| row_fail(&s.id, line, token(rule)))
        .collect()
}

fn read_rows(path: &Path) -> Result<Vec<(usize, Row)>, Refusal> {
    let fault = || Refusal::new("labels", path.display().to_string());
    let bytes = fs::read(path).map_err(|_| fault())?;
    let lines = text_lines(&bytes).ok_or_else(fault)?;
    lines
        .iter()
        .enumerate()
        .map(|(i, l)| {
            parse_row(l.as_bytes())
                .map(|r| (i + 1, r))
                .ok_or_else(fault)
        })
        .collect()
}

/// Runs `check-labels` on `dir`.
pub fn check_labels(dir: &Path, rule: Option<Rule>) -> Result<Checked, Refusal> {
    let manifest_path = dir.join("manifest.toml");
    let source = match fs::read(&manifest_path) {
        Ok(bytes) => {
            let m = TierManifest::parse(&bytes).ok_or_else(|| {
                Refusal::new("tier-manifest", manifest_path.display().to_string())
            })?;
            Rule::parse(&m.source)
        }
        Err(_) if fs::symlink_metadata(&manifest_path).is_err() => None,
        Err(_) => {
            return Err(Refusal::new(
                "tier-manifest",
                manifest_path.display().to_string(),
            ))
        }
    };
    let rule = rule.or(source).unwrap_or(Rule::Generated);
    let sm_path = dir.join("script-manifest.jsonl");
    let sm_fault = || Refusal::new("script-manifest", sm_path.display().to_string());
    let sm_bytes = fs::read(&sm_path).map_err(|_| sm_fault())?;
    let steps: Vec<StepLine> = parse_script_manifest(&sm_bytes).ok_or_else(sm_fault)?;
    let dirs = stream_dirs(dir)
        .filter(|d| !d.is_empty())
        .ok_or_else(|| Refusal::new("batches", dir.join("streams").display().to_string()))?;
    let mut streams = Vec::new();
    for d in &dirs {
        if !d.labels.is_file() {
            return Err(Refusal::new("labels", d.labels.display().to_string()));
        }
        let rows = read_rows(&d.labels)?;
        let records = read_stream(&d.days)
            .map_err(|(p, line)| Refusal::new("batches", format!("{}:{line}", p.display())))?;
        streams.push(Stream {
            id: d.id.clone(),
            rows,
            records,
        });
    }
    Ok(check_streams(rule, &steps, &streams))
}

fn check_streams(rule: Rule, steps: &[StepLine], streams: &[Stream]) -> Checked {
    let by_id: BTreeMap<&str, &StepLine> = steps.iter().map(|s| (s.step.as_str(), s)).collect();
    let step_ids: BTreeSet<&str> = by_id.keys().copied().collect();
    let mut episode_category: BTreeMap<&str, (Category, TaskKind)> = BTreeMap::new();
    for s in streams {
        for (_, r) in &s.rows {
            if let Row::Episode {
                episode,
                category,
                kind,
                ..
            } = r
            {
                episode_category
                    .entry(episode.as_str())
                    .or_insert((*category, *kind));
            }
        }
    }
    let mut seed_steps: BTreeSet<&str> = BTreeSet::new();
    for s in streams {
        for (_, r) in &s.rows {
            if let Row::Range { episode, step, .. } = r {
                let seed = episode_category
                    .get(episode.as_str())
                    .is_some_and(|(c, _)| *c != Category::Other);
                if seed {
                    seed_steps.insert(step.as_str());
                }
            }
        }
    }
    let seed_in_order: Vec<&StepLine> = steps
        .iter()
        .filter(|s| seed_steps.contains(s.step.as_str()))
        .collect();
    let mut lines = vec![format!("rule {}", rule.as_str())];
    let mut fails = 0usize;
    let (mut marked_ranges, mut unmarked) = (0u64, 0u64);
    let (mut password_ranges, mut password_keys) = (0u64, 0u64);
    let (mut card_masked, mut card_scripted) = (0u64, 0u64);
    let (mut n_episodes, mut n_ranges, mut n_titles) = (0u64, 0u64, 0u64);
    let mut episodes_seen = BTreeSet::new();
    let mut tasks_seen = BTreeMap::new();
    for s in streams {
        let row_lines = check_rows(s, &step_ids, &mut episodes_seen, &mut tasks_seen);
        fails += row_lines.len();
        lines.extend(row_lines);
        let mut other: Vec<(i64, u8, usize, String)> = Vec::new();
        let titles = &s.records.titles;
        n_titles += titles.len() as u64;
        let mut marks: Vec<(i64, Edge)> = s.records.marks.clone();
        marks.sort_by_key(|(t, e)| (*t, *e == Edge::End));
        let mut in_range = vec![false; titles.len()];
        for (line, row) in &s.rows {
            let Row::Range {
                episode,
                step,
                start,
                end,
            } = row
            else {
                n_episodes += 1;
                continue;
            };
            n_ranges += 1;
            let step_line = by_id.get(step.as_str());
            let mut inside = 0u64;
            let mut own = 0u64;
            for (i, t) in titles.iter().enumerate() {
                if *start <= t.at && t.at < *end {
                    in_range[i] = true;
                    inside += 1;
                    let produces = step_line.is_some_and(|sl| {
                        sl.produces(
                            t.app.as_deref(),
                            &t.text,
                            t.site.as_deref(),
                            rule == Rule::Recorded,
                        )
                    });
                    if produces {
                        own += 1;
                    }
                    if let Some(sl) = step_line.filter(|sl| sl.card_title) {
                        let in_titles = sl.titles.contains(&t.text);
                        let in_masked = sl.masked_titles.contains(&t.text);
                        if in_masked && !in_titles {
                            card_masked += 1;
                        }
                        if in_titles && !in_masked {
                            card_scripted += 1;
                        }
                    }
                }
            }
            let ok = match rule {
                Rule::Generated => inside == 1 && own == 1,
                Rule::Recorded => own >= 1,
            };
            if !ok {
                other.push((
                    *start,
                    0,
                    *line,
                    format!(
                        "FAIL range {} {episode} {step} {start} {end} titles={inside} own={own}",
                        s.id
                    ),
                ));
            }
            let kind = episode_category.get(episode.as_str()).map(|(_, k)| *k);
            if kind == Some(TaskKind::Marked) {
                marked_ranges += 1;
                let opened = marks
                    .iter()
                    .rposition(|(t, e)| *e == Edge::Start && *t <= *start);
                let closed = opened.is_some_and(|i| {
                    marks[i + 1..]
                        .iter()
                        .find(|(_, e)| *e == Edge::End)
                        .is_some_and(|(t, _)| *t >= *end)
                });
                if !closed {
                    unmarked += 1;
                    other.push((
                        *start,
                        1,
                        *line,
                        format!("FAIL unmarked {} {episode} {step} {start} {end}", s.id),
                    ));
                }
            }
            if step_line.is_some_and(|sl| sl.password_input) {
                password_ranges += 1;
                let keys = s
                    .records
                    .keys
                    .iter()
                    .filter(|k| *start <= **k && **k < *end)
                    .count() as u64;
                password_keys += keys;
                if keys > 0 {
                    other.push((
                        *start,
                        2,
                        *line,
                        format!(
                            "FAIL keys {} {episode} {step} {start} {end} key_events={keys}",
                            s.id
                        ),
                    ));
                }
            }
        }
        if rule == Rule::Generated {
            for (i, t) in titles.iter().enumerate() {
                if in_range[i] {
                    continue;
                }
                let first = seed_in_order
                    .iter()
                    .find(|sl| sl.produces(t.app.as_deref(), &t.text, t.site.as_deref(), false));
                if let Some(sl) = first {
                    other.push((
                        t.at,
                        0,
                        usize::MAX,
                        format!("FAIL outside {} {} {}", s.id, t.at, sl.step),
                    ));
                }
            }
        }
        other.sort_by_key(|o| (o.0, o.2, o.1));
        fails += other.len();
        lines.extend(other.into_iter().map(|o| o.3));
    }
    lines.push(format!("marked ranges={marked_ranges} unmarked={unmarked}"));
    lines.push(format!(
        "password ranges={password_ranges} key_events={password_keys}"
    ));
    lines.push(format!(
        "card masked={card_masked} scripted={card_scripted}"
    ));
    lines.push(format!(
        "checked streams={} episodes={n_episodes} ranges={n_ranges} title_events={n_titles} failures={fails}",
        streams.len()
    ));
    let pass = fails == 0;
    lines.push(if pass {
        "check-labels PASS".to_owned()
    } else {
        "check-labels FAIL".to_owned()
    });
    Checked { lines, pass }
}
