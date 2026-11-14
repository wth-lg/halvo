//! The script (contracts.md C12.2): its six line forms, their writers, and the reader with the rules S1 to S15.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use halvo_schema::{AppId, Category, Domain};
use serde::Deserialize;

use crate::form::{
    instant, is_id, json_opt, json_str, json_strs, text_lines, whole_seconds_between, Refusal, DAY,
    LAST_DAY, MILLI, SECOND, TITLE_MAX,
};

/// The `sites` value of a stream: which body of its author it belongs to (C9.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Sites {
    Seen,
    Unseen,
    Check,
    Heldout,
}

impl Sites {
    /// The written name.
    pub fn as_str(self) -> &'static str {
        match self {
            Sites::Seen => "seen",
            Sites::Unseen => "unseen",
            Sites::Check => "check",
            Sites::Heldout => "heldout",
        }
    }

    /// The value of a written name.
    pub fn parse(s: &str) -> Option<Sites> {
        match s {
            "seen" => Some(Sites::Seen),
            "unseen" => Some(Sites::Unseen),
            "check" => Some(Sites::Check),
            "heldout" => Some(Sites::Heldout),
            _ => None,
        }
    }
}

/// The kind of a task (C10.5), in the order of C10.3.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TaskKind {
    Planted,
    OneOff,
    Twice,
    NonSeed,
    NearMiss,
    Marked,
}

impl TaskKind {
    /// Every kind, in the order of C10.3.
    pub const ALL: [TaskKind; 6] = [
        TaskKind::Planted,
        TaskKind::OneOff,
        TaskKind::Twice,
        TaskKind::NonSeed,
        TaskKind::NearMiss,
        TaskKind::Marked,
    ];

    /// The written name.
    pub fn as_str(self) -> &'static str {
        match self {
            TaskKind::Planted => "planted",
            TaskKind::OneOff => "one_off",
            TaskKind::Twice => "twice",
            TaskKind::NonSeed => "non_seed",
            TaskKind::NearMiss => "near_miss",
            TaskKind::Marked => "marked",
        }
    }

    /// The value of a written name.
    pub fn parse(s: &str) -> Option<TaskKind> {
        TaskKind::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

/// The edge of a run mark.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Edge {
    Start,
    End,
}

impl Edge {
    /// The written name.
    pub fn as_str(self) -> &'static str {
        match self {
            Edge::Start => "start",
            Edge::End => "end",
        }
    }

    /// The value of a written name.
    pub fn parse(s: &str) -> Option<Edge> {
        match s {
            "start" => Some(Edge::Start),
            "end" => Some(Edge::End),
            _ => None,
        }
    }
}

/// The header line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    pub day1: i64,
    pub canaries: Vec<String>,
}

/// A step line: one page or window that the script shows (C10.9 without `masked_titles`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub step: String,
    pub app: String,
    pub site: Option<String>,
    pub titles: Vec<String>,
    pub paths: Vec<String>,
    pub password_input: bool,
    pub card_title: bool,
}

/// One stretch: an item of an episode or of a noise line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub step: String,
    pub title: usize,
    pub start: i64,
    pub end: i64,
    pub keys: u64,
    pub clicks: u64,
    pub scrolls: u64,
}

/// An episode line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Episode {
    pub stream: String,
    pub episode: String,
    pub task: String,
    pub category: Category,
    pub kind: TaskKind,
    pub day: u32,
    pub ranges: Vec<Item>,
}

/// A noise line: one stretch outside every episode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Noise {
    pub stream: String,
    pub day: u32,
    pub item: Item,
}

/// A mark line: one `run_mark` record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mark {
    pub stream: String,
    pub day: u32,
    pub edge: Edge,
    pub at: i64,
}

/// One line of a script.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Line {
    Header(Header),
    Stream { stream: String, sites: Sites },
    Step(Step),
    Episode(Episode),
    Noise(Noise),
    Mark(Mark),
}

fn write_item(i: &Item) -> String {
    format!(
        "{{\"step\":{},\"title\":{},\"start\":{},\"end\":{},\"keys\":{},\"clicks\":{},\"scrolls\":{}}}",
        json_str(&i.step),
        i.title,
        i.start,
        i.end,
        i.keys,
        i.clicks,
        i.scrolls
    )
}

impl Line {
    /// The line as a script file holds it, without its LF: compact, keys in the order of C12.2.
    pub fn write(&self) -> String {
        match self {
            Line::Header(h) => format!(
                "{{\"line\":\"script\",\"v\":1,\"day1\":{},\"canaries\":{}}}",
                h.day1,
                json_strs(&h.canaries)
            ),
            Line::Stream { stream, sites } => format!(
                "{{\"line\":\"stream\",\"stream\":{},\"sites\":{}}}",
                json_str(stream),
                json_str(sites.as_str())
            ),
            Line::Step(s) => format!(
                "{{\"line\":\"step\",\"step\":{},\"app\":{},\"site\":{},\"titles\":{},\"paths\":{},\"password_input\":{},\"card_title\":{}}}",
                json_str(&s.step),
                json_str(&s.app),
                json_opt(s.site.as_deref()),
                json_strs(&s.titles),
                json_strs(&s.paths),
                s.password_input,
                s.card_title
            ),
            Line::Episode(e) => format!(
                "{{\"line\":\"episode\",\"stream\":{},\"episode\":{},\"task\":{},\"category\":{},\"kind\":{},\"day\":{},\"ranges\":[{}]}}",
                json_str(&e.stream),
                json_str(&e.episode),
                json_str(&e.task),
                json_str(e.category.as_str()),
                json_str(e.kind.as_str()),
                e.day,
                e.ranges.iter().map(write_item).collect::<Vec<_>>().join(",")
            ),
            Line::Noise(n) => format!(
                "{{\"line\":\"noise\",\"stream\":{},\"day\":{},\"item\":{}}}",
                json_str(&n.stream),
                n.day,
                write_item(&n.item)
            ),
            Line::Mark(m) => format!(
                "{{\"line\":\"mark\",\"stream\":{},\"day\":{},\"edge\":{},\"at\":{}}}",
                json_str(&m.stream),
                m.day,
                json_str(m.edge.as_str()),
                m.at
            ),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawHeader {
    line: String,
    v: u32,
    day1: i64,
    canaries: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawStream {
    line: String,
    stream: String,
    sites: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawStep {
    line: String,
    step: String,
    app: String,
    site: Option<String>,
    titles: Vec<String>,
    paths: Vec<String>,
    password_input: bool,
    card_title: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawItem {
    step: String,
    title: u64,
    start: i64,
    end: i64,
    keys: u64,
    clicks: u64,
    scrolls: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEpisode {
    line: String,
    stream: String,
    episode: String,
    task: String,
    category: String,
    kind: String,
    day: u32,
    ranges: Vec<RawItem>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawNoise {
    line: String,
    stream: String,
    day: u32,
    item: RawItem,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMark {
    line: String,
    stream: String,
    day: u32,
    edge: String,
    at: i64,
}

fn item(r: RawItem) -> Option<Item> {
    Some(Item {
        step: r.step,
        title: usize::try_from(r.title).ok()?,
        start: r.start,
        end: r.end,
        keys: r.keys,
        clicks: r.clicks,
        scrolls: r.scrolls,
    })
}

/// Reads one line of a script, without its LF, in the form of C12.2 (rule S15's form part): one of the six
/// forms, compact, with exactly the keys shown in their order, and values of the right type. The value rules
/// are checked by `Script::read`.
pub fn parse_line(bytes: &[u8]) -> Option<Line> {
    let prefix = |p: &str| bytes.starts_with(p.as_bytes());
    let line = if prefix("{\"line\":\"script\",") {
        let h: RawHeader = serde_json::from_slice(bytes).ok()?;
        (h.line == "script" && h.v == 1).then_some(())?;
        Line::Header(Header {
            day1: h.day1,
            canaries: h.canaries,
        })
    } else if prefix("{\"line\":\"stream\",") {
        let s: RawStream = serde_json::from_slice(bytes).ok()?;
        (s.line == "stream").then_some(())?;
        Line::Stream {
            stream: s.stream,
            sites: Sites::parse(&s.sites)?,
        }
    } else if prefix("{\"line\":\"step\",") {
        let s: RawStep = serde_json::from_slice(bytes).ok()?;
        (s.line == "step").then_some(())?;
        Line::Step(Step {
            step: s.step,
            app: s.app,
            site: s.site,
            titles: s.titles,
            paths: s.paths,
            password_input: s.password_input,
            card_title: s.card_title,
        })
    } else if prefix("{\"line\":\"episode\",") {
        let e: RawEpisode = serde_json::from_slice(bytes).ok()?;
        (e.line == "episode").then_some(())?;
        Line::Episode(Episode {
            stream: e.stream,
            episode: e.episode,
            task: e.task,
            category: Category::parse(&e.category).ok()?,
            kind: TaskKind::parse(&e.kind)?,
            day: e.day,
            ranges: e.ranges.into_iter().map(item).collect::<Option<Vec<_>>>()?,
        })
    } else if prefix("{\"line\":\"noise\",") {
        let n: RawNoise = serde_json::from_slice(bytes).ok()?;
        (n.line == "noise").then_some(())?;
        Line::Noise(Noise {
            stream: n.stream,
            day: n.day,
            item: item(n.item)?,
        })
    } else if prefix("{\"line\":\"mark\",") {
        let m: RawMark = serde_json::from_slice(bytes).ok()?;
        (m.line == "mark").then_some(())?;
        Line::Mark(Mark {
            stream: m.stream,
            day: m.day,
            edge: Edge::parse(&m.edge)?,
            at: m.at,
        })
    } else {
        return None;
    };
    (line.write().as_bytes() == bytes).then_some(line)
}

/// Where a line of a script stands: the index of its file among the files given and its line number from 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct At {
    pub file: usize,
    pub line: usize,
}

/// A script read from one or more files, with every rule of C12.2 checked.
#[derive(Clone, Debug)]
pub struct Script {
    pub files: Vec<PathBuf>,
    pub header: Header,
    pub streams: BTreeMap<String, Sites>,
    pub steps: BTreeMap<String, Step>,
    pub episodes: Vec<(At, Episode)>,
    pub noise: Vec<(At, Noise)>,
    pub marks: Vec<(At, Mark)>,
    pub lines: Vec<(At, Line)>,
}

/// A broken rule of C12.2: where, and the rule's token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Broken {
    pub at: Option<At>,
    pub rule: &'static str,
}

fn broken(at: At, rule: &'static str) -> Broken {
    Broken { at: Some(at), rule }
}

impl Script {
    /// Reads the script files and checks the rules S1 to S15. The refusal has the cause `script` and a detail
    /// that names the file by its place among the arguments, `SCRIPT <n>` from 1, with the line number and the
    /// rule, or the file that cannot be read; never by its name, which may be a service's name.
    pub fn read(paths: &[PathBuf]) -> Result<Script, Refusal> {
        let mut texts = Vec::new();
        for (k, p) in paths.iter().enumerate() {
            let bytes =
                fs::read(p).map_err(|_| Refusal::new("script", format!("SCRIPT {}", k + 1)))?;
            texts.push(bytes);
        }
        Script::from_bytes(paths, &texts).map_err(|b| {
            let detail = match b.at {
                Some(at) => format!("SCRIPT {}:{}: {}", at.file + 1, at.line, b.rule),
                None => b.rule.to_owned(),
            };
            Refusal::new("script", detail)
        })
    }

    /// The same as `read`, over bytes that stand for the files.
    pub fn from_bytes(paths: &[PathBuf], texts: &[Vec<u8>]) -> Result<Script, Broken> {
        let mut header_bytes: Option<String> = None;
        let mut header: Option<Header> = None;
        let mut lines = Vec::new();
        for (file, bytes) in texts.iter().enumerate() {
            let at0 = At { file, line: 1 };
            let Some(text_lines) = text_lines(bytes) else {
                return Err(broken(at0, "S15"));
            };
            if text_lines.is_empty() {
                return Err(broken(at0, "S1"));
            }
            for (i, l) in text_lines.iter().enumerate() {
                let at = At { file, line: i + 1 };
                let parsed = parse_line(l.as_bytes()).ok_or_else(|| broken(at, "S15"))?;
                let is_header = matches!(parsed, Line::Header(_));
                if (i == 0) != is_header {
                    return Err(broken(at, "S1"));
                }
                if let Line::Header(h) = &parsed {
                    match &header_bytes {
                        None => {
                            header_bytes = Some((*l).to_owned());
                            header = Some(h.clone());
                        }
                        Some(b) if b == l => {}
                        Some(_) => return Err(broken(at, "S1")),
                    }
                    continue;
                }
                lines.push((at, parsed));
            }
        }
        let header = header.ok_or(Broken {
            at: None,
            rule: "S1",
        })?;
        let mut script = Script {
            files: paths.to_vec(),
            header,
            streams: BTreeMap::new(),
            steps: BTreeMap::new(),
            episodes: Vec::new(),
            noise: Vec::new(),
            marks: Vec::new(),
            lines: lines.clone(),
        };
        script.collect(lines)?;
        script.check()?;
        Ok(script)
    }

    fn collect(&mut self, lines: Vec<(At, Line)>) -> Result<(), Broken> {
        let mut stream_bytes: HashMap<String, String> = HashMap::new();
        let mut step_bytes: HashMap<String, String> = HashMap::new();
        for (at, line) in lines {
            match line {
                Line::Header(_) => return Err(broken(at, "S1")),
                Line::Stream { ref stream, sites } => {
                    let text = line.write();
                    match stream_bytes.get(stream) {
                        Some(b) if *b != text => return Err(broken(at, "S1")),
                        Some(_) => {}
                        None => {
                            stream_bytes.insert(stream.clone(), text);
                            self.streams.insert(stream.clone(), sites);
                        }
                    }
                }
                Line::Step(ref s) => {
                    let text = line.write();
                    match step_bytes.get(&s.step) {
                        Some(b) if *b != text => return Err(broken(at, "S1")),
                        Some(_) => {}
                        None => {
                            step_bytes.insert(s.step.clone(), text);
                            self.steps.insert(s.step.clone(), s.clone());
                        }
                    }
                }
                Line::Episode(e) => self.episodes.push((at, e)),
                Line::Noise(n) => self.noise.push((at, n)),
                Line::Mark(m) => self.marks.push((at, m)),
            }
        }
        Ok(())
    }

    fn line_at(&self, step: &str) -> At {
        self.lines
            .iter()
            .find(|(_, l)| matches!(l, Line::Step(s) if s.step == step))
            .map_or(At { file: 0, line: 1 }, |(at, _)| *at)
    }

    fn check(&self) -> Result<(), Broken> {
        self.check_header()?;
        self.check_ids()?;
        self.check_steps()?;
        self.check_items()?;
        self.check_labels()?;
        self.check_marks()?;
        self.check_files()?;
        Ok(())
    }

    fn check_header(&self) -> Result<(), Broken> {
        let at = At { file: 0, line: 1 };
        if self.header.day1 % SECOND != 0 {
            return Err(broken(at, "S14"));
        }
        let mut seen = BTreeSet::new();
        for c in &self.header.canaries {
            if Domain::parse(c).is_err() || !seen.insert(c) {
                return Err(broken(at, "S15"));
            }
        }
        Ok(())
    }

    fn check_ids(&self) -> Result<(), Broken> {
        for (at, line) in &self.lines {
            let ok = match line {
                Line::Stream { stream, .. } => is_id(stream) && stream.len() <= 36,
                Line::Step(s) => is_id(&s.step),
                Line::Episode(e) => is_id(&e.episode) && is_id(&e.task),
                _ => true,
            };
            if !ok {
                return Err(broken(*at, "S2"));
            }
        }
        let mut episodes = BTreeSet::new();
        for (at, e) in &self.episodes {
            if !episodes.insert(e.episode.as_str()) {
                return Err(broken(*at, "S2"));
            }
        }
        Ok(())
    }

    fn check_steps(&self) -> Result<(), Broken> {
        for s in self.steps.values() {
            let at = self.line_at(&s.step);
            if AppId::parse(&s.app).is_err() {
                return Err(broken(at, "S15"));
            }
            if let Some(site) = &s.site {
                if Domain::parse(site).is_err() {
                    return Err(broken(at, "S15"));
                }
            }
            let distinct: BTreeSet<&String> = s.titles.iter().collect();
            if s.titles.is_empty() || distinct.len() != s.titles.len() {
                return Err(broken(at, "S15"));
            }
            if s.titles.iter().any(|t| t.len() > TITLE_MAX) {
                return Err(broken(at, "S8"));
            }
            let paths_ok = match s.site {
                Some(_) => s.paths.len() == s.titles.len(),
                None => s.paths.is_empty(),
            };
            if !paths_ok {
                return Err(broken(at, "S12"));
            }
            if s.card_title && s.titles.iter().all(|t| halvo_redact::mask(t) == *t) {
                return Err(broken(at, "S15"));
            }
        }
        Ok(())
    }

    fn items(&self) -> Vec<(At, &str, u32, &Item)> {
        let mut out = Vec::new();
        for (at, e) in &self.episodes {
            for i in &e.ranges {
                out.push((*at, e.stream.as_str(), e.day, i));
            }
        }
        for (at, n) in &self.noise {
            out.push((*at, n.stream.as_str(), n.day, &n.item));
        }
        out
    }

    fn check_items(&self) -> Result<(), Broken> {
        let seed_steps = self.seed_steps();
        let canaries: BTreeSet<&str> = self.header.canaries.iter().map(String::as_str).collect();
        let mut named = BTreeSet::new();
        for (at, stream, day, i) in self.items() {
            if !self.streams.contains_key(stream) {
                return Err(broken(at, "S3"));
            }
            let Some(step) = self.steps.get(&i.step) else {
                return Err(broken(at, "S3"));
            };
            if i.title >= step.titles.len() {
                return Err(broken(at, "S3"));
            }
            named.insert(i.step.as_str());
            if !(1..=LAST_DAY).contains(&day) {
                return Err(broken(at, "S4"));
            }
            if i.start % MILLI != 0 || i.end % MILLI != 0 {
                return Err(broken(at, "S14"));
            }
            if !(0 <= i.start && i.start < i.end && i.end <= DAY) {
                return Err(broken(at, "S4"));
            }
            if step.password_input && i.keys != 0 {
                return Err(broken(at, "S7"));
            }
            let (Some(s), Some(e)) = (
                instant(self.header.day1, day, i.start),
                instant(self.header.day1, day, i.end),
            ) else {
                return Err(broken(at, "S4"));
            };
            let room = u64::try_from(whole_seconds_between(s, e)).unwrap_or(0);
            if i.keys.saturating_add(i.clicks) > room || i.scrolls > room {
                return Err(broken(at, "S13"));
            }
        }
        for (at, n) in &self.noise {
            if seed_steps.contains(n.item.step.as_str()) {
                return Err(broken(*at, "S6"));
            }
        }
        for (at, e) in &self.episodes {
            for i in &e.ranges {
                let site = self.steps[&i.step].site.as_deref();
                if site.is_some_and(|s| canaries.contains(s)) {
                    return Err(broken(*at, "S9"));
                }
            }
        }
        for s in self.steps.keys() {
            if !named.contains(s.as_str()) {
                return Err(broken(self.line_at(s), "S10"));
            }
        }
        let mut by_stream: BTreeMap<&str, Vec<(i64, i64, At)>> = BTreeMap::new();
        for (at, stream, day, i) in self.items() {
            let s = instant(self.header.day1, day, i.start).unwrap_or(i64::MIN);
            let e = instant(self.header.day1, day, i.end).unwrap_or(i64::MIN);
            by_stream.entry(stream).or_default().push((s, e, at));
        }
        for list in by_stream.values_mut() {
            list.sort();
            for w in list.windows(2) {
                if w[1].0 < w[0].1 {
                    return Err(broken(w[1].2.max(w[0].2), "S4"));
                }
            }
        }
        Ok(())
    }

    /// The steps that a range of an episode of a seed category names.
    pub fn seed_steps(&self) -> BTreeSet<&str> {
        self.episodes
            .iter()
            .filter(|(_, e)| e.category != Category::Other)
            .flat_map(|(_, e)| e.ranges.iter().map(|i| i.step.as_str()))
            .collect()
    }

    /// The instant of an offset on a day of this script.
    pub fn instant(&self, day: u32, offset: i64) -> i64 {
        instant(self.header.day1, day, offset).unwrap_or(i64::MIN)
    }

    fn check_labels(&self) -> Result<(), Broken> {
        let mut tasks: BTreeMap<&str, Vec<&(At, Episode)>> = BTreeMap::new();
        for pair in &self.episodes {
            let (at, e) = pair;
            if e.ranges.is_empty() {
                return Err(broken(*at, "S5"));
            }
            if e.ranges.windows(2).any(|w| w[1].start <= w[0].start) {
                return Err(broken(*at, "S5"));
            }
            let seed = e.category != Category::Other;
            if seed == (e.kind == TaskKind::NonSeed) {
                return Err(broken(*at, "S5"));
            }
            tasks.entry(e.task.as_str()).or_default().push(pair);
        }
        for list in tasks.values() {
            let (at0, first) = list[0];
            for (at, e) in list {
                if e.stream != first.stream || e.category != first.category || e.kind != first.kind
                {
                    return Err(broken(*at, "S5"));
                }
            }
            let n = list.len();
            let days: BTreeSet<u32> = list.iter().map(|(_, e)| e.day).collect();
            let count_ok = match first.kind {
                TaskKind::Planted | TaskKind::NearMiss => n >= 3,
                TaskKind::OneOff => n == 1,
                TaskKind::Twice => n == 2,
                TaskKind::Marked => n == 5,
                TaskKind::NonSeed => (3..=8).contains(&days.len()),
            };
            if !count_ok {
                return Err(broken(*at0, "S5"));
            }
        }
        for (task, list) in &tasks {
            let (at, first) = list[0];
            if first.kind != TaskKind::Twice {
                continue;
            }
            let (sites, apps) = self.task_sites_apps(list);
            for (other, others) in &tasks {
                let o = &others[0].1;
                if other == task || o.stream != first.stream || o.category != first.category {
                    continue;
                }
                let (osites, oapps) = self.task_sites_apps(others);
                let shares = if sites.is_empty() {
                    !apps.is_disjoint(&oapps)
                } else {
                    !sites.is_disjoint(&osites)
                };
                if shares {
                    return Err(broken(*at, "S5"));
                }
            }
        }
        Ok(())
    }

    fn task_sites_apps(&self, list: &[&(At, Episode)]) -> (BTreeSet<String>, BTreeSet<String>) {
        let mut sites = BTreeSet::new();
        let mut apps = BTreeSet::new();
        for (_, e) in list {
            for i in &e.ranges {
                let step = &self.steps[&i.step];
                if let Some(s) = &step.site {
                    sites.insert(s.clone());
                }
                apps.insert(step.app.clone());
            }
        }
        (sites, apps)
    }

    fn check_marks(&self) -> Result<(), Broken> {
        for (at, m) in &self.marks {
            if !self.streams.contains_key(&m.stream) {
                return Err(broken(*at, "S3"));
            }
            if !(1..=LAST_DAY).contains(&m.day) {
                return Err(broken(*at, "S4"));
            }
            if m.at % MILLI != 0 {
                return Err(broken(*at, "S14"));
            }
            if !(0..DAY).contains(&m.at) {
                return Err(broken(*at, "S4"));
            }
        }
        let mut marked: BTreeMap<&str, Vec<&(At, Episode)>> = BTreeMap::new();
        for pair in &self.episodes {
            if pair.1.kind == TaskKind::Marked {
                marked.entry(pair.1.stream.as_str()).or_default().push(pair);
            }
        }
        for (stream, list) in &marked {
            let at0 = list[0].0;
            let tasks: BTreeSet<&str> = list.iter().map(|(_, e)| e.task.as_str()).collect();
            if tasks.len() > 1 {
                return Err(broken(at0, "S5"));
            }
            let first = list
                .iter()
                .map(|(_, e)| self.instant(e.day, e.ranges[0].start))
                .min()
                .unwrap_or(i64::MIN);
            let last = list
                .iter()
                .map(|(_, e)| self.instant(e.day, e.ranges[e.ranges.len() - 1].end))
                .max()
                .unwrap_or(i64::MIN);
            let mut marks: Vec<(i64, Edge, At)> = self
                .marks
                .iter()
                .filter(|(_, m)| m.stream == *stream)
                .map(|(at, m)| (self.instant(m.day, m.at), m.edge, *at))
                .collect();
            marks.sort();
            let start = marks
                .iter()
                .rposition(|(t, e, _)| *e == Edge::Start && *t <= first);
            let Some(si) = start else {
                return Err(broken(at0, "S5"));
            };
            let Some((end_t, end_e, _)) = marks.get(si + 1) else {
                return Err(broken(at0, "S5"));
            };
            if *end_e != Edge::End || *end_t < last {
                return Err(broken(at0, "S5"));
            }
            let (from, to) = (marks[si].0, *end_t);
            let task = list[0].1.task.as_str();
            for (at, e) in &self.episodes {
                if e.stream != *stream || e.task == task {
                    continue;
                }
                for i in &e.ranges {
                    let (s, en) = (self.instant(e.day, i.start), self.instant(e.day, i.end));
                    if s < to && en > from {
                        return Err(broken(*at, "S5"));
                    }
                }
            }
        }
        Ok(())
    }

    fn check_files(&self) -> Result<(), Broken> {
        let mut marked_files = BTreeSet::new();
        for (at, e) in &self.episodes {
            if e.kind == TaskKind::Marked {
                marked_files.insert(at.file);
            }
        }
        for (at, _) in &self.marks {
            marked_files.insert(at.file);
        }
        for (at, e) in &self.episodes {
            if e.kind != TaskKind::Marked && marked_files.contains(&at.file) {
                return Err(broken(*at, "S11"));
            }
        }
        for (at, _) in &self.noise {
            if marked_files.contains(&at.file) {
                return Err(broken(*at, "S11"));
            }
        }
        Ok(())
    }

    /// The template of an episode of this script (C12.4): the site-masked forms of the titles its items
    /// choose, each masked with the site of its step, with an entry equal to the one before it left out.
    pub fn template(&self, e: &Episode) -> Vec<String> {
        crate::text::template(e.ranges.iter().map(|i| {
            let step = &self.steps[&i.step];
            crate::text::site_masked(&step.titles[i.title], step.site.as_deref())
        }))
    }

    /// The path of a file of this script.
    pub fn file(&self, index: usize) -> &Path {
        &self.files[index]
    }
}
