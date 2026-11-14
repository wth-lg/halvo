//! `leak` (contracts.md C12.8): compares a body with files and development bodies, by site names, canary
//! names, stream digests, episode templates, title tokens and the share of shared site-masked titles.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use halvo_schema::Category;
use sha2::{Digest, Sha256};

use crate::body::{read_stream, stream_dirs, StreamRecords};
use crate::form::{text_lines, Refusal};
use crate::input_copy::read_tier;
use crate::labels::{parse_row, Row};
use crate::manifest::{parse_script_manifest, StepLine, TierManifest};
use crate::text::{site_masked, site_name, template, tokens};

/// What a check of leaks reads of one body: its script manifest, its streams with their rows and records, and
/// its canaries where it has a manifest.
pub struct BodyView {
    pub steps: Vec<StepLine>,
    pub streams: Vec<(String, Vec<Row>, StreamRecords)>,
    pub canaries: Vec<String>,
}

fn read_body(dir: &Path, cause: &'static str) -> Result<BodyView, Refusal> {
    let fault = |p: &Path| Refusal::new(cause, p.display().to_string());
    let sm = dir.join("script-manifest.jsonl");
    let steps = fs::read(&sm)
        .ok()
        .and_then(|b| parse_script_manifest(&b))
        .ok_or_else(|| fault(&sm))?;
    let mut canaries = Vec::new();
    let mp = dir.join("manifest.toml");
    if fs::symlink_metadata(&mp).is_ok() {
        let m = fs::read(&mp)
            .ok()
            .and_then(|b| TierManifest::parse(&b))
            .ok_or_else(|| fault(&mp))?;
        canaries = m.canaries;
    }
    let mut streams = Vec::new();
    let root = dir.join("streams");
    if fs::symlink_metadata(&root).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(fault(&root));
    }
    let listing = stream_dirs(dir).unwrap_or_default();
    if let Some(link) = listing.links.first() {
        return Err(fault(&dir.join("streams").join(link)));
    }
    for d in listing.dirs {
        let bytes = fs::read(&d.labels).map_err(|_| fault(&d.labels))?;
        let rows: Vec<Row> = text_lines(&bytes)
            .ok_or_else(|| fault(&d.labels))?
            .iter()
            .map(|l| parse_row(l.as_bytes()))
            .collect::<Option<_>>()
            .ok_or_else(|| fault(&d.labels))?;
        let records = read_stream(&d.days).map_err(|(p, _)| fault(&p))?;
        streams.push((d.id, rows, records));
    }
    Ok(BodyView {
        steps,
        streams,
        canaries,
    })
}

fn digest(r: &StreamRecords) -> Option<[u8; 32]> {
    if r.titles.is_empty() {
        return None;
    }
    let mut h = Sha256::new();
    for t in &r.titles {
        h.update(t.app.as_deref().unwrap_or("").as_bytes());
        h.update(b"\n");
        h.update(t.text.as_bytes());
        h.update(b"\n");
    }
    Some(h.finalize().into())
}

/// The episodes of a body: for each, its category and its template, read from the title events inside its
/// ranges, each masked with the site of its range's step, in the order of instant.
fn episodes(b: &BodyView) -> Vec<(Category, Vec<String>, Vec<String>)> {
    let site: BTreeMap<&str, Option<&str>> = b
        .steps
        .iter()
        .map(|s| (s.step.as_str(), s.site.as_deref()))
        .collect();
    let mut out = Vec::new();
    for (_, rows, records) in &b.streams {
        let mut current: Option<(Category, Vec<(i64, String)>)> = None;
        let flush = |cur: &mut Option<(Category, Vec<(i64, String)>)>,
                     out: &mut Vec<(Category, Vec<String>, Vec<String>)>| {
            if let Some((c, mut events)) = cur.take() {
                events.sort_by_key(|e| e.0);
                let masked: Vec<String> = events.into_iter().map(|e| e.1).collect();
                out.push((c, template(masked.clone()), masked));
            }
        };
        for row in rows {
            match row {
                Row::Episode { category, .. } => {
                    flush(&mut current, &mut out);
                    current = Some((*category, Vec::new()));
                }
                Row::Range {
                    step, start, end, ..
                } => {
                    let s = site.get(step.as_str()).copied().flatten();
                    if let Some((_, events)) = &mut current {
                        for t in &records.titles {
                            if *start <= t.at && t.at < *end {
                                events.push((t.at, site_masked(&t.text, s)));
                            }
                        }
                    }
                }
            }
        }
        flush(&mut current, &mut out);
    }
    out
}

/// The names, each in lower case, that `hay` holds without regard to the case of ASCII letters, found in one
/// pass over the bytes with the names listed by their first byte.
fn names_in<'a>(hay: &[u8], names: &[&'a str]) -> BTreeSet<&'a str> {
    let mut by_first: Vec<Vec<&'a str>> = vec![Vec::new(); 256];
    for n in names {
        if let Some(b) = n.as_bytes().first() {
            by_first[usize::from(b.to_ascii_lowercase())].push(n);
        }
    }
    let mut found = BTreeSet::new();
    for i in 0..hay.len() {
        for n in &by_first[usize::from(hay[i].to_ascii_lowercase())] {
            let nb = n.as_bytes();
            if i + nb.len() <= hay.len() && hay[i..i + nb.len()].eq_ignore_ascii_case(nb) {
                found.insert(*n);
            }
        }
    }
    found
}

/// True when a PATH argument names a symbolic link, also when it names one with separators or `.` components
/// after its name.
fn names_a_link(arg: &str) -> bool {
    let is_link = |p: &str| fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink());
    let mut named = arg;
    loop {
        if is_link(named) {
            return true;
        }
        let next = match named.strip_suffix(std::path::is_separator) {
            Some(t) => t,
            None => match named
                .strip_suffix('.')
                .filter(|t| t.ends_with(std::path::is_separator))
            {
                Some(t) => t,
                None => return false,
            },
        };
        if next.is_empty() {
            return false;
        }
        named = next;
    }
}

struct Walk {
    files: Vec<(String, PathBuf)>,
    bodies: Vec<PathBuf>,
}

fn walk(arg: &str, path: &Path, shown: String, skip: &Path, out: &mut Walk) -> Result<(), Refusal> {
    let meta = fs::symlink_metadata(path).map_err(|_| Refusal::new("path", arg.to_owned()))?;
    if meta.file_type().is_symlink() {
        return Ok(());
    }
    if meta.is_file() {
        let real = fs::canonicalize(path).map_err(|_| Refusal::new("path", shown.clone()))?;
        if !real.starts_with(skip) {
            out.files.push((shown, path.to_path_buf()));
        }
        return Ok(());
    }
    if !meta.is_dir() {
        return Ok(());
    }
    if let Ok(real) = fs::canonicalize(path) {
        if real.starts_with(skip) {
            return Ok(());
        }
    }
    if path.join("script-manifest.jsonl").is_file() {
        out.bodies.push(path.to_path_buf());
    }
    let mut names: Vec<String> = Vec::new();
    for e in fs::read_dir(path).map_err(|_| Refusal::new("path", shown.clone()))? {
        let e = e.map_err(|_| Refusal::new("path", shown.clone()))?;
        let name = e
            .file_name()
            .to_str()
            .map(str::to_owned)
            .ok_or_else(|| Refusal::new("path", shown.clone()))?;
        names.push(name);
    }
    names.sort();
    for name in names {
        let child = path.join(&name);
        let child_shown = format!("{shown}/{name}");
        let is_dir = fs::symlink_metadata(&child).is_ok_and(|m| m.is_dir());
        if is_dir && (name == ".git" || name == "target") {
            continue;
        }
        walk(arg, &child, child_shown, skip, out)?;
    }
    Ok(())
}

/// Runs `leak` and returns the lines it prints and whether the four shared counts are 0.
pub fn leak(tier: &Path, paths: &[String]) -> Result<(Vec<String>, bool), Refusal> {
    let (manifest, hash) = read_tier(tier)?;
    let body = read_body(tier, "tier-files")?;
    let skip = fs::canonicalize(tier)
        .map_err(|_| Refusal::new("tier-manifest", tier.display().to_string()))?;
    let mut w = Walk {
        files: Vec::new(),
        bodies: Vec::new(),
    };
    for p in paths {
        if names_a_link(p) {
            return Err(Refusal::new("path", p.clone()));
        }
        walk(
            p,
            Path::new(p),
            p.trim_end_matches('/').to_owned(),
            &skip,
            &mut w,
        )?;
    }
    let dev: Vec<(PathBuf, BodyView)> = w
        .bodies
        .iter()
        .map(|d| read_body(d, "body").map(|b| (d.clone(), b)))
        .collect::<Result<_, _>>()?;
    let own: Vec<&PathBuf> = dev
        .iter()
        .filter(|(_, b)| !manifest.canaries.is_empty() && b.canaries == manifest.canaries)
        .map(|(d, _)| d)
        .collect();
    let canary_names: BTreeSet<String> = manifest
        .canaries
        .iter()
        .map(|c| site_name(c).to_owned())
        .collect();
    let site_names: BTreeSet<String> = body
        .steps
        .iter()
        .filter_map(|s| s.site.as_deref().map(|d| site_name(d).to_owned()))
        .collect();
    let title_tokens: BTreeSet<String> = body
        .steps
        .iter()
        .flat_map(|s| {
            s.titles
                .iter()
                .flat_map(|t| tokens(&site_masked(t, s.site.as_deref())))
                .collect::<Vec<_>>()
        })
        .filter(|t| t != "*")
        .collect();
    let mut site_found: BTreeSet<String> = BTreeSet::new();
    let mut canary_found: BTreeSet<String> = BTreeSet::new();
    let mut token_found: BTreeSet<String> = BTreeSet::new();
    let mut site_hits: Vec<(String, usize)> = Vec::new();
    let mut canary_hits: Vec<(String, usize)> = Vec::new();
    for (shown, path) in &w.files {
        let bytes = fs::read(path).map_err(|_| Refusal::new("path", shown.clone()))?;
        let in_own = own.iter().any(|d| path.starts_with(d));
        let wanted: Vec<&str> = site_names
            .iter()
            .filter(|n| !(in_own && canary_names.contains(*n)))
            .map(String::as_str)
            .collect();
        let found = names_in(&bytes, &wanted);
        if !found.is_empty() {
            site_hits.push((shown.clone(), found.len()));
        }
        site_found.extend(found.into_iter().map(str::to_owned));
        if !in_own {
            let wanted: Vec<&str> = canary_names.iter().map(String::as_str).collect();
            let found = names_in(&bytes, &wanted);
            if !found.is_empty() {
                canary_hits.push((shown.clone(), found.len()));
            }
            canary_found.extend(found.into_iter().map(str::to_owned));
        }
        if let Ok(text) = std::str::from_utf8(&bytes) {
            for t in tokens(text) {
                if title_tokens.contains(&t) {
                    token_found.insert(t);
                }
            }
        }
    }
    site_hits.sort();
    canary_hits.sort();
    let dev_digests: BTreeSet<[u8; 32]> = dev
        .iter()
        .flat_map(|(_, b)| b.streams.iter().filter_map(|s| digest(&s.2)))
        .collect();
    let shared_streams = body
        .streams
        .iter()
        .filter(|s| digest(&s.2).is_some_and(|d| dev_digests.contains(&d)))
        .count();
    let dev_templates: BTreeSet<Vec<String>> = dev
        .iter()
        .flat_map(|(_, b)| episodes(b).into_iter().map(|e| e.1))
        .collect();
    let mine = episodes(&body);
    let my_templates: BTreeSet<&Vec<String>> = mine.iter().map(|e| &e.1).collect();
    let shared_templates = my_templates
        .iter()
        .filter(|t| dev_templates.contains(**t))
        .count();
    let dev_masked: BTreeSet<String> = dev
        .iter()
        .flat_map(|(_, b)| {
            b.steps
                .iter()
                .flat_map(|s| {
                    s.titles
                        .iter()
                        .chain(&s.masked_titles)
                        .map(|t| site_masked(t, s.site.as_deref()))
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        })
        .collect();
    let mut lines = vec![format!(
        "leak {hash} bodies={} files={} canaries_listed={}",
        dev.len(),
        w.files.len(),
        manifest.canaries.len()
    )];
    lines.extend(site_hits.iter().map(|(p, n)| format!("HIT site {p} {n}")));
    lines.extend(
        canary_hits
            .iter()
            .map(|(p, n)| format!("HIT canary {p} {n}")),
    );
    lines.push(format!(
        "shared streams={shared_streams} sites={} templates={shared_templates} canaries={}",
        site_found.len(),
        canary_found.len()
    ));
    lines.push(format!("tokens {}", token_found.len()));
    for c in Category::ALL {
        let of: Vec<&(Category, Vec<String>, Vec<String>)> =
            mine.iter().filter(|e| e.0 == *c).collect();
        let shared = of
            .iter()
            .filter(|e| e.2.iter().any(|m| dev_masked.contains(m)))
            .count();
        lines.push(format!("share {} {shared} {}", c.as_str(), of.len()));
    }
    let pass = shared_streams == 0
        && site_found.is_empty()
        && shared_templates == 0
        && canary_found.is_empty();
    lines.push(if pass { "leak PASS" } else { "leak FAIL" }.to_owned());
    Ok((lines, pass))
}
