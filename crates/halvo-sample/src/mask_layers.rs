//! `mask-layers` (contracts.md C12.11): masks the two texts of each raw layer record and cuts each to 78,000
//! bytes, in that order.

use std::fs;
use std::path::PathBuf;

use crate::form::{text_lines, write_new, Refusal};
use crate::layers::{parse_record, LayerRecord};

/// The longest layer text after the cut, in bytes (C11.5).
pub const LAYER_MAX: usize = 78_000;

/// The arguments of `mask-layers`.
#[derive(Clone, Debug)]
pub struct MaskArgs {
    pub out: PathBuf,
    pub raw: PathBuf,
}

/// The longest prefix of whole characters of `s` within `max` bytes.
pub fn cut(s: &str, max: usize) -> &str {
    let mut end = s.len().min(max);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Runs `mask-layers` and returns the line it prints.
pub fn mask_layers(args: &MaskArgs) -> Result<Vec<String>, Refusal> {
    let fault = || Refusal::new("raw", args.raw.display().to_string());
    let bytes = fs::read(&args.raw).map_err(|_| fault())?;
    let lines = text_lines(&bytes).ok_or_else(fault)?;
    let mut out = String::new();
    let (mut changed, mut cut_n) = (0u64, 0u64);
    let mut last: Option<String> = None;
    for l in &lines {
        let r = parse_record(l.as_bytes()).ok_or_else(fault)?;
        if last.as_ref().is_some_and(|p| *p >= r.step) {
            return Err(fault());
        }
        last = Some(r.step.clone());
        let mut one = |t: Option<String>| {
            t.map(|t| {
                let m = halvo_redact::mask(&t);
                if m != t {
                    changed += 1;
                }
                let c = cut(&m, LAYER_MAX).to_owned();
                if c.len() != m.len() {
                    cut_n += 1;
                }
                c
            })
        };
        let rec = LayerRecord {
            step: r.step,
            source: r.source,
            accessibility_text: one(r.accessibility_text),
            extracted_text: one(r.extracted_text),
        };
        out.push_str(&rec.write());
        out.push('\n');
    }
    if fs::symlink_metadata(&args.out).is_ok() {
        return Err(Refusal::new("exists", args.out.display().to_string()));
    }
    write_new(&args.out, out.as_bytes()).map_err(|e| {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            Refusal::new("exists", args.out.display().to_string())
        } else {
            Refusal::new("output", args.out.display().to_string())
        }
    })?;
    Ok(vec![format!(
        "masked steps={} texts_changed={changed} texts_cut={cut_n}",
        lines.len()
    )])
}
