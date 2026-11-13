//! `input-copy` (contracts.md C12.7): a tier's batches reduced to an input set by the fixture transport, the
//! layer records of the set's layers keyed by stream and instant, and the tier's manifest hash; nothing else.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use halvo_seam::client::Client;
use halvo_seam::fixture::FixtureTransport;
use halvo_seam::input_set::{InputSet, Layer};
use halvo_seam::message::OpenCursor;
use halvo_seam::record::{CaptureRecord, Payload};

use crate::body::day_of;
use crate::form::{create_out_dir, file_sha256, json_opt, json_str, write_new, Refusal};
use crate::labels::{parse_row, Row};
use crate::layers::{parse_file, LayerRecord};
use crate::manifest::{parse_script_manifest, StepLine, TierManifest};

/// The arguments of `input-copy`.
#[derive(Clone, Debug)]
pub struct InputCopyArgs {
    pub input_set: PathBuf,
    pub layers: Option<PathBuf>,
    pub tier: PathBuf,
    pub out: PathBuf,
}

/// The day files of each stream of a tier, from its manifest's file table, in rising day order.
pub fn stream_days(m: &TierManifest) -> BTreeMap<String, Vec<(u32, String)>> {
    let mut out: BTreeMap<String, Vec<(u32, String)>> = m
        .streams
        .iter()
        .map(|(id, _)| (id.clone(), Vec::new()))
        .collect();
    for (path, _) in &m.files {
        let parts: Vec<&str> = path.split('/').collect();
        if let ["streams", id, name] = parts.as_slice() {
            if let (Some(list), Some(day)) = (out.get_mut(*id), day_of(name)) {
                list.push((day, path.clone()));
            }
        }
    }
    for list in out.values_mut() {
        list.sort();
    }
    out
}

/// Reads and checks a tier's manifest: the file exists, is of C9.3, and every file it lists hashes to its
/// entry. The cause is `tier-manifest`.
pub fn read_tier(tier: &Path) -> Result<(TierManifest, String), Refusal> {
    let path = tier.join("manifest.toml");
    let fault = |p: &Path| Refusal::new("tier-manifest", p.display().to_string());
    let bytes = fs::read(&path).map_err(|_| fault(&path))?;
    let m = TierManifest::parse(&bytes).ok_or_else(|| fault(&path))?;
    for (rel, hash) in &m.files {
        let p = tier.join(rel);
        if file_sha256(&p).ok().as_deref() != Some(hash.as_str()) {
            return Err(fault(&p));
        }
    }
    Ok((m, crate::form::sha256_hex(&bytes)))
}

/// Every record that the fixture transport serves for a stream's day files, with the ordinal of its segment.
fn read_through(
    files: &[PathBuf],
    set: Option<&Path>,
) -> Result<Vec<(u64, CaptureRecord)>, String> {
    let transport = FixtureTransport::open(files, 1, set).map_err(|e| format!("{e:?}"))?;
    let mut client = Client::new(transport);
    let opened = client
        .open(OpenCursor::fresh(None, None))
        .map_err(|e| format!("{e:?}"))?;
    let mut out = Vec::new();
    loop {
        let batch = client.read().map_err(|e| format!("{e:?}"))?;
        for entry in batch.records {
            out.push((entry.pos.seg, entry.record));
        }
        if batch.next >= opened.watermark {
            break;
        }
    }
    Ok(out)
}

fn count_lines(path: &Path) -> u64 {
    fs::read(path)
        .map(|b| b.iter().filter(|c| **c == b'\n').count() as u64)
        .unwrap_or(0)
}

/// Runs `input-copy` and returns the line it prints.
pub fn input_copy(args: &InputCopyArgs) -> Result<Vec<String>, Refusal> {
    let set = InputSet::read(&args.input_set)
        .map_err(|_| Refusal::new("input-set", args.input_set.display().to_string()))?;
    let (manifest, hash) = read_tier(&args.tier)?;
    let streams = stream_days(&manifest);
    let wanted: Vec<Layer> = set.layers().to_vec();
    let mut layer_lines: Vec<(String, i64, String)> = Vec::new();
    if !wanted.is_empty() {
        let layers = read_layers(args, &manifest)?;
        let steps = read_steps(&args.tier, &manifest)?;
        for (id, days) in &streams {
            let files: Vec<PathBuf> = days.iter().map(|(_, p)| args.tier.join(p)).collect();
            let records = read_through(&files, None).map_err(|_| {
                Refusal::new(
                    "tier-files",
                    args.tier.join("streams").join(id).display().to_string(),
                )
            })?;
            let ranges = read_ranges(&args.tier, id)?;
            for (_, r) in records {
                let Payload::WindowTitle(t) = r.payload() else {
                    continue;
                };
                let at = r.at().0;
                let step = ranges
                    .iter()
                    .find(|(s, e, _)| *s <= at && at < *e)
                    .map(|(_, _, step)| step.clone())
                    .or_else(|| {
                        steps
                            .iter()
                            .find(|sl| {
                                sl.produces(
                                    r.app().map(|a| a.as_str()),
                                    t.text.as_str(),
                                    t.site.as_ref().map(|d| d.as_str()),
                                    true,
                                )
                            })
                            .map(|sl| sl.step.clone())
                    });
                let Some(rec) = step.and_then(|s| layers.get(&s)) else {
                    continue;
                };
                layer_lines.push((id.clone(), at, layer_line(id, at, rec, &wanted)));
            }
        }
        layer_lines.sort_by(|a, b| (&a.0, a.1).cmp(&(&b.0, b.1)));
    }
    let mut copies: Vec<(String, Vec<u8>)> = Vec::new();
    let (mut read_n, mut written, mut day_files) = (0u64, 0u64, 0u64);
    for (id, days) in &streams {
        let files: Vec<PathBuf> = days.iter().map(|(_, p)| args.tier.join(p)).collect();
        let records = read_through(&files, Some(&args.input_set)).map_err(|_| {
            Refusal::new(
                "tier-files",
                args.tier.join("streams").join(id).display().to_string(),
            )
        })?;
        let mut per_seg: Vec<Vec<u8>> = vec![Vec::new(); days.len()];
        for (seg, r) in records {
            per_seg[(seg - 1) as usize].extend(r.to_line());
            written += 1;
        }
        for ((_, rel), bytes) in days.iter().zip(per_seg) {
            read_n += count_lines(&args.tier.join(rel));
            day_files += 1;
            copies.push((rel.clone(), bytes));
        }
    }
    create_out_dir(&args.out)?;
    let fault = |p: &Path| Refusal::new("output", p.display().to_string());
    let put = |rel: &str, bytes: &[u8]| -> Result<(), Refusal> {
        let p = args.out.join(rel);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).map_err(|_| fault(&p))?;
        }
        write_new(&p, bytes).map_err(|_| fault(&p))
    };
    put("manifest.sha256", format!("{hash}\n").as_bytes())?;
    for (rel, bytes) in &copies {
        put(rel, bytes)?;
    }
    if !wanted.is_empty() {
        let text: String = layer_lines.iter().map(|l| format!("{}\n", l.2)).collect();
        put("layers/instants.jsonl", text.as_bytes())?;
    }
    Ok(vec![format!(
        "input-copy {hash} streams={} day_files={day_files} records_read={read_n} records_written={written} layer_records={}",
        streams.len(),
        layer_lines.len()
    )])
}

fn layer_line(stream: &str, at: i64, rec: &LayerRecord, wanted: &[Layer]) -> String {
    let mut out = format!(
        "{{\"stream\":{},\"at\":{at},\"source\":{}",
        json_str(stream),
        json_str(&rec.source)
    );
    if wanted.contains(&Layer::AccessibilityText) {
        out.push_str(&format!(
            ",\"accessibility_text\":{}",
            json_opt(rec.accessibility_text.as_deref())
        ));
    }
    if wanted.contains(&Layer::ExtractedText) {
        out.push_str(&format!(
            ",\"extracted_text\":{}",
            json_opt(rec.extracted_text.as_deref())
        ));
    }
    out.push('}');
    out
}

fn checked(tier: &Path, m: &TierManifest, rel: &str) -> Result<Vec<u8>, Refusal> {
    let p = tier.join(rel);
    let listed = m
        .files
        .iter()
        .find(|(r, _)| r == rel)
        .map(|(_, h)| h.clone());
    let bytes = fs::read(&p).map_err(|_| Refusal::new("tier-manifest", p.display().to_string()))?;
    if listed.as_deref() != Some(crate::form::sha256_hex(&bytes).as_str()) {
        return Err(Refusal::new("tier-manifest", p.display().to_string()));
    }
    Ok(bytes)
}

fn read_steps(tier: &Path, m: &TierManifest) -> Result<Vec<StepLine>, Refusal> {
    let bytes = checked(tier, m, "script-manifest.jsonl")?;
    parse_script_manifest(&bytes).ok_or_else(|| {
        Refusal::new(
            "tier-files",
            tier.join("script-manifest.jsonl").display().to_string(),
        )
    })
}

fn read_ranges(tier: &Path, stream: &str) -> Result<Vec<(i64, i64, String)>, Refusal> {
    let rel = format!("streams/{stream}/labels.jsonl");
    let p = tier.join(&rel);
    let fault = || Refusal::new("tier-files", p.display().to_string());
    let bytes = fs::read(&p).map_err(|_| fault())?;
    let lines = crate::form::text_lines(&bytes).ok_or_else(fault)?;
    let mut out = Vec::new();
    for l in lines {
        match parse_row(l.as_bytes()).ok_or_else(fault)? {
            Row::Range {
                step, start, end, ..
            } => out.push((start, end, step)),
            Row::Episode { .. } => {}
        }
    }
    Ok(out)
}

fn read_layers(
    args: &InputCopyArgs,
    m: &TierManifest,
) -> Result<BTreeMap<String, LayerRecord>, Refusal> {
    let (path, bytes) = match &args.layers {
        None => {
            let p = args.tier.join("layers/steps.jsonl");
            let b = fs::read(&p).map_err(|_| Refusal::new("layers", p.display().to_string()))?;
            let listed = m.files.iter().find(|(r, _)| r == "layers/steps.jsonl");
            if listed.map(|(_, h)| h.as_str()) != Some(crate::form::sha256_hex(&b).as_str()) {
                return Err(Refusal::new("tier-manifest", p.display().to_string()));
            }
            (p, b)
        }
        Some(dir) => {
            let p = dir.join("steps.jsonl");
            let fault = || Refusal::new("layers", p.display().to_string());
            let b = fs::read(&p).map_err(|_| fault())?;
            if let Some(parent) = dir.parent() {
                let lm = parent.join("manifest.toml");
                if fs::symlink_metadata(&lm).is_ok() {
                    let name = dir.file_name().and_then(|n| n.to_str()).ok_or_else(fault)?;
                    let text = fs::read(&lm).map_err(|_| fault())?;
                    let entries = parse_layer_manifest(&text).ok_or_else(fault)?;
                    let want = format!("{name}/steps.jsonl");
                    let hash = entries
                        .iter()
                        .find(|(p, _)| *p == want)
                        .map(|(_, h)| h.clone())
                        .ok_or_else(fault)?;
                    if hash != crate::form::sha256_hex(&b) {
                        return Err(fault());
                    }
                }
            }
            (p, b)
        }
    };
    let records =
        parse_file(&bytes).ok_or_else(|| Refusal::new("layers", path.display().to_string()))?;
    Ok(records.into_iter().map(|r| (r.step.clone(), r)).collect())
}

/// Reads a layer manifest (C11.6): `version = 1`, then `[[file]]` tables with a path and a hash, sorted by path.
pub fn parse_layer_manifest(bytes: &[u8]) -> Option<Vec<(String, String)>> {
    let lines = crate::form::text_lines(bytes)?;
    let mut it = lines.into_iter();
    (it.next()? == "version = 1").then_some(())?;
    let mut out: Vec<(String, String)> = Vec::new();
    while let Some(l) = it.next() {
        (l == "[[file]]").then_some(())?;
        let path = it
            .next()?
            .strip_prefix("path = \"")?
            .strip_suffix('"')?
            .to_owned();
        let hash = it
            .next()?
            .strip_prefix("sha256 = \"")?
            .strip_suffix('"')?
            .to_owned();
        let hex_ok = hash.len() == 64
            && hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if !hex_ok || path.contains('"') || out.last().is_some_and(|p| p.0 >= path) {
            return None;
        }
        out.push((path, hash));
    }
    Some(out)
}
