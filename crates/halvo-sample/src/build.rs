//! `build` (contracts.md C12.3): renders a script to batch files, label rows and a script manifest, and with a
//! tier to a body with its manifest.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use halvo_schema::{AppId, Category, Domain, Instant, SeedCategory, Text};
use halvo_seam::record::{
    Button, CaptureRecord, Edge as SeamEdge, KeyClass, Motion, Payload, Pointer, RecordId, Seq,
    Title, Why,
};

use crate::form::{create_out_dir, file_sha256, sha256_hex, write_new, Refusal, MILLI, SECOND};
use crate::labels::rows_of;
use crate::layers;
use crate::manifest::{tier_facts, Counts, StepLine, TierManifest};
use crate::rng::Draw;
use crate::script::{Edge, Item, Script, Sites, TaskKind};

/// The arguments of `build`.
#[derive(Clone, Debug)]
pub struct BuildArgs {
    pub tier: Option<String>,
    pub layer_records: Option<PathBuf>,
    pub seed: u64,
    pub out: PathBuf,
    pub scripts: Vec<PathBuf>,
}

/// The largest scroll `count` that `build` draws: the events of one interval of 1,000 ms (C12.3).
pub const SCROLL_COUNT_MAX: u64 = 20;

fn key_class(n: u64) -> KeyClass {
    KeyClass::ALL[n as usize]
}

/// The id of a record: a UUID of version 7 with the millisecond of `at` in its first 48 bits, the top 12 bits
/// of `w1` after the version and the top 62 bits of `w2` after the variant bits `10`.
pub fn record_id(at: i64, w1: u64, w2: u64) -> String {
    let ms = (at.div_euclid(MILLI) as u64) & 0xFFFF_FFFF_FFFF;
    let rand_a = w1 >> 52;
    let rand_b = w2 >> 2;
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        ms >> 16,
        ms & 0xFFFF,
        0x7000 | rand_a,
        0x8000 | ((rand_b >> 48) & 0x3FFF),
        rand_b & 0xFFFF_FFFF_FFFF
    )
}

/// One record before its id and seq are drawn: its instant, its rank among records of one instant, its day,
/// its app and its payload.
struct Pending {
    at: i64,
    rank: u8,
    day: u32,
    app: Option<String>,
    payload: Payload,
}

enum Event<'a> {
    Mark { at: i64, day: u32, edge: Edge },
    Item { at: i64, day: u32, item: &'a Item },
}

/// The records of one stream, by scripted day, in their order, as batch-file lines.
pub fn render_stream(script: &Script, stream: &str, seed: u64) -> BTreeMap<u32, Vec<u8>> {
    let mut draw = Draw::named(&format!("halvo-sample build {seed} {stream}"));
    let mut events: Vec<(i64, u8, Edge, Event)> = Vec::new();
    for (_, m) in &script.marks {
        if m.stream == stream {
            let at = script.instant(m.day, m.at);
            events.push((
                at,
                0,
                m.edge,
                Event::Mark {
                    at,
                    day: m.day,
                    edge: m.edge,
                },
            ));
        }
    }
    for (_, e) in &script.episodes {
        if e.stream == stream {
            for item in &e.ranges {
                let at = script.instant(e.day, item.start);
                events.push((
                    at,
                    1,
                    Edge::Start,
                    Event::Item {
                        at,
                        day: e.day,
                        item,
                    },
                ));
            }
        }
    }
    for (_, n) in &script.noise {
        if n.stream == stream {
            let at = script.instant(n.day, n.item.start);
            events.push((
                at,
                1,
                Edge::Start,
                Event::Item {
                    at,
                    day: n.day,
                    item: &n.item,
                },
            ));
        }
    }
    events.sort_by_key(|e| (e.0, e.1, e.2));
    let mut pending: Vec<Pending> = Vec::new();
    let mut last: Option<(u32, String)> = None;
    for (_, _, _, ev) in &events {
        match ev {
            Event::Mark { at, day, edge } => pending.push(Pending {
                at: *at,
                rank: 0,
                day: *day,
                app: None,
                payload: Payload::RunMark(match edge {
                    Edge::Start => SeamEdge::Start,
                    Edge::End => SeamEdge::End,
                }),
            }),
            Event::Item { at, day, item } => {
                let step = &script.steps[&item.step];
                let change = last
                    .as_ref()
                    .is_none_or(|(d, app)| d != day || *app != step.app);
                if change {
                    pending.push(Pending {
                        at: *at,
                        rank: 1,
                        day: *day,
                        app: Some(step.app.clone()),
                        payload: Payload::ForegroundApp,
                    });
                }
                pending.push(Pending {
                    at: *at,
                    rank: 2,
                    day: *day,
                    app: Some(step.app.clone()),
                    payload: Payload::WindowTitle(Title {
                        text: Text::parse(&step.titles[item.title])
                            .unwrap_or_else(|_| Text::cut(&step.titles[item.title])),
                        cut: Some(false),
                        why: Some(if change {
                            Why::AppChange
                        } else {
                            Why::TitleChange
                        }),
                        site: step.site.as_deref().and_then(|s| Domain::parse(s).ok()),
                    }),
                });
                let end = script.instant(*day, item.end);
                let first = at.div_euclid(SECOND) + 1;
                let last_s = (end - 1).div_euclid(SECOND);
                let seconds: Vec<i64> = (first..=last_s).map(|s| s * SECOND).collect();
                let chosen = draw.pick(&seconds, (item.keys + item.clicks) as usize);
                let mut classes = Vec::new();
                for _ in 0..item.keys {
                    classes.push(key_class(draw.below(KeyClass::ALL.len() as u64)));
                }
                let scroll_at = draw.pick(&seconds, item.scrolls as usize);
                let mut counts = Vec::new();
                for _ in 0..item.scrolls {
                    counts.push(1 + draw.below(SCROLL_COUNT_MAX));
                }
                for (i, s) in chosen.iter().enumerate() {
                    let (rank, payload) = if (i as u64) < item.keys {
                        (3, Payload::Keystroke(Some(classes[i])))
                    } else {
                        (4, Payload::Pointer(Pointer::Click(Some(Button::Primary))))
                    };
                    pending.push(Pending {
                        at: *s,
                        rank,
                        day: *day,
                        app: Some(step.app.clone()),
                        payload,
                    });
                }
                for (s, c) in scroll_at.iter().zip(counts) {
                    pending.push(Pending {
                        at: *s,
                        rank: 5,
                        day: *day,
                        app: Some(step.app.clone()),
                        payload: Payload::Pointer(Pointer::Motion(
                            Motion::Scroll,
                            halvo_seam::record::Count::new(c),
                        )),
                    });
                }
                last = Some((*day, step.app.clone()));
            }
        }
    }
    pending.sort_by_key(|p| (p.at, p.rank));
    lines_of(pending, &mut draw)
}

fn lines_of(pending: Vec<Pending>, draw: &mut Draw) -> BTreeMap<u32, Vec<u8>> {
    let mut days: BTreeMap<u32, Vec<u8>> = BTreeMap::new();
    let mut seq = 0u64;
    for p in pending {
        let (w1, w2) = (draw.word(), draw.word());
        let id = RecordId::parse(&record_id(p.at, w1, w2));
        let app = p.app.as_deref().and_then(|a| AppId::parse(a).ok());
        let record = id.and_then(|id| {
            CaptureRecord::new(id, Instant(p.at), Seq::new(seq)?, app, p.payload).ok()
        });
        if let Some(r) = record {
            days.entry(p.day).or_default().extend(r.to_line());
            seq += 1;
        }
    }
    days
}

/// The counts of the tasks of a script by kind and seed category (C9.3).
pub fn counts_of(script: &Script) -> Counts {
    let mut counts = Counts {
        streams: script.streams.len() as u64,
        ..Counts::default()
    };
    let mut tasks: BTreeMap<&str, (Category, TaskKind)> = BTreeMap::new();
    for (_, e) in &script.episodes {
        tasks.insert(e.task.as_str(), (e.category, e.kind));
    }
    for (category, kind) in tasks.values() {
        let Some(seed) = category.seed() else {
            counts.non_seed += 1;
            continue;
        };
        let i = SeedCategory::ALL
            .iter()
            .position(|c| *c == seed)
            .unwrap_or(0);
        let table = match kind {
            TaskKind::Planted => &mut counts.planted,
            TaskKind::OneOff => &mut counts.one_off,
            TaskKind::Twice => &mut counts.twice,
            TaskKind::NearMiss => &mut counts.near_miss,
            TaskKind::Marked => &mut counts.marked,
            TaskKind::NonSeed => continue,
        };
        table[i] += 1;
    }
    counts
}

fn sites_of_tier(tier: &str) -> Option<Sites> {
    tier_facts(tier).map(|(_, _, s)| s[0])
}

/// Runs `build` and returns the lines it prints.
pub fn build(args: &BuildArgs) -> Result<Vec<String>, Refusal> {
    let script = Script::read(&args.scripts)?;
    if let Some(tier) = &args.tier {
        let want = sites_of_tier(tier).ok_or_else(|| Refusal::new("usage", tier.clone()))?;
        for (id, sites) in &script.streams {
            if *sites != want {
                return Err(Refusal::new("script", format!("stream {id}: sites")));
            }
            let negative = script
                .episodes
                .iter()
                .any(|(_, e)| e.stream == *id && e.kind != TaskKind::Planted);
            if !negative {
                return Err(Refusal::new("script", format!("stream {id}: negative")));
            }
        }
    }
    let layer_bytes = match &args.layer_records {
        None => None,
        Some(path) => {
            let fault = || Refusal::new("layer-records", path.display().to_string());
            let bytes = fs::read(path).map_err(|_| fault())?;
            let records = layers::parse_file(&bytes).ok_or_else(fault)?;
            let steps: Vec<&str> = records.iter().map(|r| r.step.as_str()).collect();
            let want: Vec<&str> = script.steps.keys().map(String::as_str).collect();
            if steps != want {
                return Err(fault());
            }
            Some(bytes)
        }
    };
    create_out_dir(&args.out)?;
    let out = &args.out;
    let fault = |p: &Path| Refusal::new("output", p.display().to_string());
    let mut files: Vec<(String, String)> = Vec::new();
    let mut write = |rel: String, bytes: &[u8]| -> Result<(), Refusal> {
        let path = out.join(&rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|_| fault(&path))?;
        }
        write_new(&path, bytes).map_err(|_| fault(&path))?;
        files.push((rel, sha256_hex(bytes)));
        Ok(())
    };
    let seed_steps: BTreeSet<&str> = script
        .episodes
        .iter()
        .filter(|(_, e)| e.category != Category::Other)
        .flat_map(|(_, e)| e.ranges.iter().map(|i| i.step.as_str()))
        .collect();
    let step_lines: Vec<StepLine> = script
        .steps
        .values()
        .map(|s| StepLine::of(s, seed_steps.contains(s.step.as_str())))
        .collect();
    let manifest_text: String = step_lines
        .iter()
        .map(|l| format!("{}\n", l.write()))
        .collect();
    write("script-manifest.jsonl".to_owned(), manifest_text.as_bytes())?;
    let (mut records, mut title_events, mut clicks, mut active) = (0u64, 0u64, 0u64, 0i64);
    for stream in script.streams.keys() {
        let dir = match &args.tier {
            Some(t) => format!("{t}-{stream}"),
            None => stream.clone(),
        };
        for (day, bytes) in render_stream(&script, stream, args.seed) {
            let (n, t, c, a) = tally(&bytes);
            records += n;
            title_events += t;
            clicks += c;
            active += a;
            write(format!("streams/{dir}/day-{day:03}.jsonl"), &bytes)?;
        }
        let rows: String = rows_of(&script, stream)
            .iter()
            .map(|r| format!("{}\n", r.write()))
            .collect();
        write(format!("streams/{dir}/labels.jsonl"), rows.as_bytes())?;
    }
    if let Some(bytes) = &layer_bytes {
        write("layers/steps.jsonl".to_owned(), bytes)?;
    }
    files.sort();
    let mut manifest_hash = None;
    if let Some(tier) = &args.tier {
        let (split, source, _) = tier_facts(tier).unwrap_or(("dev", "generated", &[]));
        let mut canaries = script.header.canaries.clone();
        canaries.sort();
        let sites = sites_of_tier(tier).unwrap_or(Sites::Seen);
        let manifest = TierManifest {
            tier: tier.clone(),
            split: split.to_owned(),
            source: source.to_owned(),
            seed: args.seed,
            day1: script.header.day1,
            canaries,
            streams: script
                .streams
                .keys()
                .map(|s| (format!("{tier}-{s}"), sites))
                .collect(),
            counts: counts_of(&script),
            files: files.clone(),
        };
        let text = manifest.write();
        let path = out.join("manifest.toml");
        write_new(&path, text.as_bytes()).map_err(|_| fault(&path))?;
        manifest_hash = file_sha256(&path).ok();
    }
    let tasks: BTreeSet<&str> = script
        .episodes
        .iter()
        .map(|(_, e)| e.task.as_str())
        .collect();
    let ranges: usize = script.episodes.iter().map(|(_, e)| e.ranges.len()).sum();
    let per_hour = |n: u64| {
        if active > 0 {
            (i128::from(n) * 3600 / i128::from(active)) as u64
        } else {
            0
        }
    };
    let (mut titles, mut changed, mut false_masks) = (0u64, 0u64, 0u64);
    for l in &step_lines {
        for (t, m) in l.titles.iter().zip(&l.masked_titles) {
            titles += 1;
            if t != m {
                changed += 1;
                if !l.card_title {
                    false_masks += 1;
                }
            }
        }
    }
    let mut lines = vec![
        format!(
            "counts streams={} tasks={} episodes={} ranges={} records={} title_events={} clicks={} active_seconds={}",
            script.streams.len(),
            tasks.len(),
            script.episodes.len(),
            ranges,
            records,
            title_events,
            clicks,
            active
        ),
        format!(
            "rates title_changes_per_hour={} clicks_per_hour={}",
            per_hour(title_events),
            per_hour(clicks)
        ),
        format!("masks titles={titles} changed={changed} false={false_masks}"),
    ];
    if let Some(h) = manifest_hash {
        lines.push(format!("manifest {h}"));
    }
    Ok(lines)
}

/// The records, title events and clicks of one day file, and its whole seconds from the first to the last
/// record.
fn tally(bytes: &[u8]) -> (u64, u64, u64, i64) {
    let (mut n, mut t, mut c) = (0, 0, 0);
    let (mut first, mut last) = (None, 0i64);
    for line in bytes.split_inclusive(|b| *b == b'\n') {
        let Ok(r) = CaptureRecord::read_line(line) else {
            continue;
        };
        n += 1;
        match r.payload() {
            Payload::WindowTitle(_) => t += 1,
            Payload::Pointer(Pointer::Click(_)) => c += 1,
            _ => {}
        }
        first.get_or_insert(r.at().0);
        last = r.at().0;
    }
    let span = first.map_or(0, |f| (last - f) / SECOND);
    (n, t, c, span)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_that_cannot_be_made_takes_no_seq() {
        let pending = |app: Option<&str>, payload: Payload, at: i64| Pending {
            at,
            rank: 0,
            day: 1,
            app: app.map(str::to_owned),
            payload,
        };
        let records = vec![
            pending(None, Payload::RunMark(SeamEdge::Start), SECOND),
            pending(Some("org.chromium.Chromium"), Payload::RunMark(SeamEdge::End), 2 * SECOND),
            pending(None, Payload::RunMark(SeamEdge::End), 3 * SECOND),
        ];
        let days = lines_of(records, &mut Draw::named("seq"));
        let seqs: Vec<u64> = days[&1]
            .split_inclusive(|b| *b == b'\n')
            .map(|l| CaptureRecord::read_line(l).expect("a record").seq().get())
            .collect();
        assert_eq!(seqs, vec![0, 1]);
    }
}
