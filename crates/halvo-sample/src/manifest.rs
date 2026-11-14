//! The script manifest (contracts.md C10.9) and the tier manifest (C9.3).

use std::collections::BTreeSet;

use halvo_schema::{AppId, Domain, SeedCategory};
use serde::Deserialize;

use crate::form::{is_id, json_opt, json_str, json_strs, text_lines, TITLE_MAX};
use crate::script::{Sites, Step};

/// One line of a script manifest: a step with the masked form of each of its titles.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepLine {
    pub step: String,
    pub app: String,
    pub site: Option<String>,
    pub titles: Vec<String>,
    pub masked_titles: Vec<String>,
    pub paths: Vec<String>,
    pub password_input: bool,
    pub card_title: bool,
    pub seed_category: bool,
}

impl StepLine {
    /// The manifest line of a script step: `masked_titles[i]` is `halvo_redact::mask(titles[i])` (C7.11), and
    /// `seed_category` is true when the script names the step in a range of an episode whose category is not
    /// `other` (C10.9).
    pub fn of(step: &Step, seed_category: bool) -> StepLine {
        StepLine {
            step: step.step.clone(),
            app: step.app.clone(),
            site: step.site.clone(),
            titles: step.titles.clone(),
            masked_titles: step.titles.iter().map(|t| halvo_redact::mask(t)).collect(),
            paths: step.paths.clone(),
            password_input: step.password_input,
            card_title: step.card_title,
            seed_category,
        }
    }

    /// The line without its LF, compact with the keys in the order of C10.9.
    pub fn write(&self) -> String {
        format!(
            "{{\"step\":{},\"app\":{},\"site\":{},\"titles\":{},\"masked_titles\":{},\"paths\":{},\"password_input\":{},\"card_title\":{},\"seed_category\":{}}}",
            json_str(&self.step),
            json_str(&self.app),
            json_opt(self.site.as_deref()),
            json_strs(&self.titles),
            json_strs(&self.masked_titles),
            json_strs(&self.paths),
            self.password_input,
            self.card_title,
            self.seed_category
        )
    }

    /// True when a title event with this app, text and site can come from the step (C10.10): the app is the
    /// step's, the text is a string of `titles` (or, with `either`, of `masked_titles`), and a site that the
    /// record carries is the step's site.
    pub fn produces(
        &self,
        app: Option<&str>,
        text: &str,
        site: Option<&str>,
        either: bool,
    ) -> bool {
        if app != Some(self.app.as_str()) {
            return false;
        }
        if let Some(s) = site {
            if self.site.as_deref() != Some(s) {
                return false;
            }
        }
        self.titles.iter().any(|t| t == text)
            || (either && self.masked_titles.iter().any(|t| t == text))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawStepLine {
    step: String,
    app: String,
    site: Option<String>,
    titles: Vec<String>,
    masked_titles: Vec<String>,
    paths: Vec<String>,
    password_input: bool,
    card_title: bool,
    seed_category: bool,
}

/// Reads a script manifest: one line per step of C10.9, sorted by `step` in byte order with no step twice.
/// None when the bytes are no such file.
pub fn parse_script_manifest(bytes: &[u8]) -> Option<Vec<StepLine>> {
    let mut out: Vec<StepLine> = Vec::new();
    for l in text_lines(bytes)? {
        let r: RawStepLine = serde_json::from_slice(l.as_bytes()).ok()?;
        let line = StepLine {
            step: r.step,
            app: r.app,
            site: r.site,
            titles: r.titles,
            masked_titles: r.masked_titles,
            paths: r.paths,
            password_input: r.password_input,
            card_title: r.card_title,
            seed_category: r.seed_category,
        };
        if line.write() != l || !step_line_ok(&line) {
            return None;
        }
        if out.last().is_some_and(|p| p.step >= line.step) {
            return None;
        }
        out.push(line);
    }
    Some(out)
}

fn step_line_ok(l: &StepLine) -> bool {
    let distinct: BTreeSet<&String> = l.titles.iter().collect();
    is_id(&l.step)
        && AppId::parse(&l.app).is_ok()
        && l.site.as_deref().is_none_or(|s| Domain::parse(s).is_ok())
        && !l.titles.is_empty()
        && distinct.len() == l.titles.len()
        && l.titles.iter().all(|t| t.len() <= TITLE_MAX)
        && l.masked_titles.len() == l.titles.len()
        && l.titles
            .iter()
            .zip(&l.masked_titles)
            .all(|(t, m)| halvo_redact::mask(t) == *m)
        && if l.site.is_some() {
            l.paths.len() == l.titles.len()
        } else {
            l.paths.is_empty()
        }
        && (!l.card_title || l.titles.iter().zip(&l.masked_titles).any(|(t, m)| t != m))
}

/// The tier ids of the tier table (C9.1).
pub const TIERS: [&str; 7] = [
    "gen-dev",
    "gen-unseen",
    "gen-check",
    "gen-heldout",
    "rec-dev",
    "rec-check",
    "rec-heldout",
];

/// The split, the source and the `sites` values of a tier id (C9.1).
pub fn tier_facts(tier: &str) -> Option<(&'static str, &'static str, &'static [Sites])> {
    Some(match tier {
        "gen-dev" => ("dev", "generated", &[Sites::Seen]),
        "gen-unseen" => ("dev", "generated", &[Sites::Unseen]),
        "gen-check" => ("dev", "generated", &[Sites::Check]),
        "gen-heldout" => ("heldout", "generated", &[Sites::Heldout]),
        "rec-dev" => ("dev", "recorded", &[Sites::Seen, Sites::Unseen]),
        "rec-check" => ("dev", "recorded", &[Sites::Check]),
        "rec-heldout" => ("heldout", "recorded", &[Sites::Heldout]),
        _ => return None,
    })
}

/// The counts of a body (C9.3): per seed category, in the order of 0.5, the tasks of each kind.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub streams: u64,
    pub non_seed: u64,
    pub planted: [u64; 10],
    pub one_off: [u64; 10],
    pub twice: [u64; 10],
    pub near_miss: [u64; 10],
    pub marked: [u64; 10],
}

/// A tier manifest (C9.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TierManifest {
    pub tier: String,
    pub split: String,
    pub source: String,
    pub seed: u64,
    pub day1: i64,
    pub canaries: Vec<String>,
    pub streams: Vec<(String, Sites)>,
    pub counts: Counts,
    pub files: Vec<(String, String)>,
}

fn toml_list(list: &[String]) -> String {
    let inner: Vec<String> = list.iter().map(|s| format!("\"{s}\"")).collect();
    format!("[{}]", inner.join(", "))
}

impl TierManifest {
    /// The manifest as the text of C9.3.
    pub fn write(&self) -> String {
        let mut out = String::new();
        out.push_str("version = 1\n");
        out.push_str(&format!("tier = \"{}\"\n", self.tier));
        out.push_str(&format!("split = \"{}\"\n", self.split));
        out.push_str(&format!("source = \"{}\"\n", self.source));
        out.push_str(&format!("seed = {}\n", self.seed));
        out.push_str(&format!("day1 = {}\n", self.day1));
        out.push_str(&format!("canaries = {}\n", toml_list(&self.canaries)));
        for (id, sites) in &self.streams {
            out.push_str(&format!(
                "[[stream]]\nid = \"{id}\"\nsites = \"{}\"\n",
                sites.as_str()
            ));
        }
        out.push_str("[counts]\n");
        out.push_str(&format!("streams = {}\n", self.counts.streams));
        out.push_str(&format!("non_seed = {}\n", self.counts.non_seed));
        let tables: [(&str, &[u64; 10]); 5] = [
            ("planted", &self.counts.planted),
            ("one_off", &self.counts.one_off),
            ("twice", &self.counts.twice),
            ("near_miss", &self.counts.near_miss),
            ("marked", &self.counts.marked),
        ];
        for (name, values) in tables {
            out.push_str(&format!("[counts.{name}]\n"));
            for (c, n) in SeedCategory::ALL.iter().zip(values.iter()) {
                out.push_str(&format!("{} = {n}\n", c.as_str()));
            }
        }
        for (path, hash) in &self.files {
            out.push_str(&format!(
                "[[file]]\npath = \"{path}\"\nsha256 = \"{hash}\"\n"
            ));
        }
        out
    }

    /// Reads a tier manifest. It is taken only when it is byte-equal to the text that `write` gives for what
    /// was read, so every line, its order and its spaces are those of C9.3; the values are checked by their
    /// rules (the tier table, ids, domains sorted by bytes, the file table sorted by path with hashes of 64
    /// lower-case hex digits).
    pub fn parse(bytes: &[u8]) -> Option<TierManifest> {
        let lines = text_lines(bytes)?;
        let mut it = lines.iter().copied().peekable();
        let mut next = || it.next();
        let value = |line: Option<&str>, key: &str| -> Option<String> {
            line?
                .strip_prefix(key)?
                .strip_prefix(" = ")
                .map(str::to_owned)
        };
        let quoted = |v: String| -> Option<String> {
            let inner = v.strip_prefix('"')?.strip_suffix('"')?;
            (!inner.contains('"') && !inner.contains('\\')).then(|| inner.to_owned())
        };
        let number = |v: String| -> Option<String> { Some(v) };
        (next()? == "version = 1").then_some(())?;
        let tier = quoted(value(next(), "tier")?)?;
        let split = quoted(value(next(), "split")?)?;
        let source = quoted(value(next(), "source")?)?;
        let seed: u64 = number(value(next(), "seed")?)?.parse().ok()?;
        let day1: i64 = value(next(), "day1")?.parse().ok()?;
        let canaries_text = value(next(), "canaries")?;
        let canaries = parse_toml_list(&canaries_text)?;
        let mut streams = Vec::new();
        let mut line = next()?;
        while line == "[[stream]]" {
            let id = quoted(value(next(), "id")?)?;
            let sites = Sites::parse(&quoted(value(next(), "sites")?)?)?;
            streams.push((id, sites));
            line = next()?;
        }
        (line == "[counts]").then_some(())?;
        let streams_n: u64 = value(next(), "streams")?.parse().ok()?;
        let non_seed: u64 = value(next(), "non_seed")?.parse().ok()?;
        let mut tables = [[0u64; 10]; 5];
        for (t, name) in ["planted", "one_off", "twice", "near_miss", "marked"]
            .iter()
            .enumerate()
        {
            (next()? == format!("[counts.{name}]")).then_some(())?;
            for (i, c) in SeedCategory::ALL.iter().enumerate() {
                tables[t][i] = value(next(), c.as_str())?.parse().ok()?;
            }
        }
        let mut files = Vec::new();
        while let Some(l) = next() {
            (l == "[[file]]").then_some(())?;
            let path = quoted(value(next(), "path")?)?;
            let hash = quoted(value(next(), "sha256")?)?;
            files.push((path, hash));
        }
        let m = TierManifest {
            tier,
            split,
            source,
            seed,
            day1,
            canaries,
            streams,
            counts: Counts {
                streams: streams_n,
                non_seed,
                planted: tables[0],
                one_off: tables[1],
                twice: tables[2],
                near_miss: tables[3],
                marked: tables[4],
            },
            files,
        };
        (m.write().as_bytes() == bytes && m.values_ok()).then_some(m)
    }

    fn values_ok(&self) -> bool {
        let Some((split, source, sites)) = tier_facts(&self.tier) else {
            return false;
        };
        let hash_ok = |h: &str| {
            h.len() == 64
                && h.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        };
        let path_ok = |p: &str| {
            !p.is_empty()
                && p != "manifest.toml"
                && p.split('/')
                    .all(|seg| !seg.is_empty() && seg != "." && seg != "..")
        };
        self.split == split
            && self.source == source
            && self.seed <= i64::MAX as u64
            && self.canaries.windows(2).all(|w| w[0] < w[1])
            && self.canaries.iter().all(|c| Domain::parse(c).is_ok())
            && self.streams.windows(2).all(|w| w[0].0 < w[1].0)
            && self.streams.iter().all(|(id, s)| {
                is_id(id) && id.starts_with(&format!("{}-", self.tier)) && sites.contains(s)
            })
            && self.counts.streams == self.streams.len() as u64
            && self.files.windows(2).all(|w| w[0].0 < w[1].0)
            && self.files.iter().all(|(p, h)| path_ok(p) && hash_ok(h))
    }
}

fn parse_toml_list(text: &str) -> Option<Vec<String>> {
    let inner = text.strip_prefix('[')?.strip_suffix(']')?;
    if inner.is_empty() {
        return Some(Vec::new());
    }
    inner
        .split(", ")
        .map(|s| {
            let v = s.strip_prefix('"')?.strip_suffix('"')?;
            (!v.contains('"') && !v.contains('\\')).then(|| v.to_owned())
        })
        .collect()
}
