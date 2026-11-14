//! `split` (contracts.md C12.5): assigns whole streams, as groups closed under shared sites and templates, to
//! the two sides of one author.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use crate::form::{create_out_dir, write_new, Refusal};
use crate::rng::Draw;
use crate::script::{Line, Script, Sites};

/// The arguments of `split`.
#[derive(Clone, Debug)]
pub struct SplitArgs {
    pub sides: (Sites, Sites),
    pub seed: u64,
    pub out: PathBuf,
    pub scripts: Vec<PathBuf>,
}

/// The groups of a script's streams: sets closed under links, where two streams are linked when they share a
/// site (a `site` of a step that an item of the stream names, null and the canaries of the header left out)
/// or an episode template. Each group is sorted; the groups are listed in the order of their smallest
/// stream id.
pub fn groups(script: &Script) -> Vec<Vec<String>> {
    let ids: Vec<&String> = script.streams.keys().collect();
    let index: BTreeMap<&str, usize> = ids
        .iter()
        .enumerate()
        .map(|(i, s)| (s.as_str(), i))
        .collect();
    let mut parent: Vec<usize> = (0..ids.len()).collect();
    fn find(p: &mut [usize], i: usize) -> usize {
        let mut r = i;
        while p[r] != r {
            r = p[r];
        }
        let mut j = i;
        while p[j] != r {
            let next = p[j];
            p[j] = r;
            j = next;
        }
        r
    }
    let canaries: BTreeSet<&str> = script.header.canaries.iter().map(String::as_str).collect();
    let mut owner: BTreeMap<String, usize> = BTreeMap::new();
    let mut link = |key: String, stream: &str, parent: &mut Vec<usize>| {
        let i = index[stream];
        match owner.get(&key) {
            Some(j) => {
                let (a, b) = (find(parent, i), find(parent, *j));
                if a != b {
                    parent[a.max(b)] = a.min(b);
                }
            }
            None => {
                owner.insert(key, i);
            }
        }
    };
    let mut site_of = |stream: &str, step: &str, parent: &mut Vec<usize>| {
        if let Some(site) = &script.steps[step].site {
            if !canaries.contains(site.as_str()) {
                link(format!("site\n{site}"), stream, parent);
            }
        }
    };
    for (_, e) in &script.episodes {
        for i in &e.ranges {
            site_of(&e.stream, &i.step, &mut parent);
        }
    }
    for (_, n) in &script.noise {
        site_of(&n.stream, &n.item.step, &mut parent);
    }
    let mut owner_t: BTreeMap<Vec<String>, usize> = BTreeMap::new();
    for (_, e) in &script.episodes {
        let t = script.template(e);
        let i = index[e.stream.as_str()];
        match owner_t.get(&t) {
            Some(j) => {
                let (a, b) = (find(&mut parent, i), find(&mut parent, *j));
                if a != b {
                    parent[a.max(b)] = a.min(b);
                }
            }
            None => {
                owner_t.insert(t, i);
            }
        }
    }
    let mut by_root: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    for (i, id) in ids.iter().enumerate() {
        let r = find(&mut parent, i);
        by_root.entry(r).or_default().push((*id).clone());
    }
    let mut out: Vec<Vec<String>> = by_root.into_values().collect();
    out.sort_by(|a, b| a[0].cmp(&b[0]));
    out
}

/// The number of different sites and of different episode templates of a set of streams.
pub fn side_facts(script: &Script, streams: &BTreeSet<String>) -> (usize, usize) {
    let canaries: BTreeSet<&str> = script.header.canaries.iter().map(String::as_str).collect();
    let mut sites = BTreeSet::new();
    let mut templates = BTreeSet::new();
    let mut add = |step: &str| {
        if let Some(s) = &script.steps[step].site {
            if !canaries.contains(s.as_str()) {
                sites.insert(s.clone());
            }
        }
    };
    for (_, e) in &script.episodes {
        if streams.contains(&e.stream) {
            for i in &e.ranges {
                add(&i.step);
            }
            templates.insert(script.template(e));
        }
    }
    for (_, n) in &script.noise {
        if streams.contains(&n.stream) {
            add(&n.item.step);
        }
    }
    (sites.len(), templates.len())
}

fn stream_of(line: &Line) -> Option<&str> {
    match line {
        Line::Stream { stream, .. } => Some(stream),
        Line::Episode(e) => Some(&e.stream),
        Line::Noise(n) => Some(&n.stream),
        Line::Mark(m) => Some(&m.stream),
        Line::Header(_) | Line::Step(_) => None,
    }
}

/// Runs `split` and returns the lines it prints.
pub fn split(args: &SplitArgs) -> Result<Vec<String>, Refusal> {
    let mut names = BTreeSet::new();
    for (k, p) in args.scripts.iter().enumerate() {
        let name = p
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| Refusal::new("script", format!("SCRIPT {}", k + 1)))?;
        if !names.insert(name.to_owned()) {
            return Err(Refusal::new("script", format!("SCRIPT {}", k + 1)));
        }
    }
    let script = Script::read(&args.scripts)?;
    let mut groups = groups(&script);
    let total: usize = groups.iter().map(Vec::len).sum();
    let mut draw = Draw::named(&format!("halvo-sample split {}", args.seed));
    draw.shuffle(&mut groups);
    let mut second: BTreeSet<String> = BTreeSet::new();
    let mut first: BTreeSet<String> = BTreeSet::new();
    for g in &groups {
        if second.len() * 2 < total {
            second.extend(g.iter().cloned());
        } else {
            first.extend(g.iter().cloned());
        }
    }
    if first.is_empty() || second.is_empty() {
        return Err(Refusal::new("no-split", format!("streams={total}")));
    }
    let side_streams = [(args.sides.0, &first), (args.sides.1, &second)];
    let mut side_steps: Vec<BTreeSet<&str>> = Vec::new();
    for (_, streams) in &side_streams {
        let mut steps = BTreeSet::new();
        for (_, e) in &script.episodes {
            if streams.contains(&e.stream) {
                steps.extend(e.ranges.iter().map(|i| i.step.as_str()));
            }
        }
        for (_, n) in &script.noise {
            if streams.contains(&n.stream) {
                steps.insert(n.item.step.as_str());
            }
        }
        side_steps.push(steps);
    }
    let header = Line::Header(script.header.clone()).write();
    let mut outputs: Vec<(&str, usize, PathBuf, String)> = Vec::new();
    for (k, (sites, streams)) in side_streams.iter().enumerate() {
        for (file, path) in args.scripts.iter().enumerate() {
            let mut text = format!("{header}\n");
            for (at, line) in &script.lines {
                if at.file != file {
                    continue;
                }
                let keep = match line {
                    Line::Step(s) => side_steps[k].contains(s.step.as_str()),
                    other => stream_of(other).is_some_and(|s| streams.contains(s)),
                };
                if !keep {
                    continue;
                }
                let written = match line {
                    Line::Stream { stream, .. } => Line::Stream {
                        stream: stream.clone(),
                        sites: *sites,
                    }
                    .write(),
                    other => other.write(),
                };
                text.push_str(&written);
                text.push('\n');
            }
            let name = path.file_name().map(PathBuf::from).unwrap_or_default();
            outputs.push((
                sites.as_str(),
                file,
                PathBuf::from(sites.as_str()).join(name),
                text,
            ));
        }
    }
    create_out_dir(&args.out)?;
    for (side, file, rel, text) in &outputs {
        let path = args.out.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|_| Refusal::new("output", args.out.join(side).display().to_string()))?;
        }
        write_new(&path, text.as_bytes()).map_err(|_| {
            Refusal::new(
                "output",
                format!("{}: SCRIPT {}", args.out.display(), file + 1),
            )
        })?;
    }
    let mut lines = vec![format!("groups {}", groups.len())];
    for (sites, streams) in side_streams {
        let (n_sites, n_templates) = side_facts(&script, streams);
        lines.push(format!(
            "side {} streams={} sites={n_sites} templates={n_templates}",
            sites.as_str(),
            streams.len()
        ));
    }
    Ok(lines)
}
