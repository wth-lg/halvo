//! Layer records by step (contracts.md C11.5).

use serde::Deserialize;

use crate::form::{is_id, json_opt, json_str, text_lines};

/// One layer record by step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerRecord {
    pub step: String,
    pub source: String,
    pub accessibility_text: Option<String>,
    pub extracted_text: Option<String>,
}

impl LayerRecord {
    /// The line without its LF, compact with the keys in the order of C11.5.
    pub fn write(&self) -> String {
        format!(
            "{{\"step\":{},\"source\":{},\"accessibility_text\":{},\"extracted_text\":{}}}",
            json_str(&self.step),
            json_str(&self.source),
            json_opt(self.accessibility_text.as_deref()),
            json_opt(self.extracted_text.as_deref())
        )
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    step: String,
    source: String,
    accessibility_text: Option<String>,
    extracted_text: Option<String>,
}

/// Reads one line, without its LF, of a layer file: compact, keys in order, a step id of 0.7 and the source
/// `render` or `daemon`.
pub fn parse_record(bytes: &[u8]) -> Option<LayerRecord> {
    let r: Raw = serde_json::from_slice(bytes).ok()?;
    let rec = LayerRecord {
        step: r.step,
        source: r.source,
        accessibility_text: r.accessibility_text,
        extracted_text: r.extracted_text,
    };
    let ok = is_id(&rec.step) && (rec.source == "render" || rec.source == "daemon");
    (ok && rec.write().as_bytes() == bytes).then_some(rec)
}

/// Reads a layer file: one record per line, sorted by `step` in byte order with no step twice.
pub fn parse_file(bytes: &[u8]) -> Option<Vec<LayerRecord>> {
    let mut out: Vec<LayerRecord> = Vec::new();
    for l in text_lines(bytes)? {
        let r = parse_record(l.as_bytes())?;
        if out.last().is_some_and(|p| p.step >= r.step) {
            return None;
        }
        out.push(r);
    }
    Some(out)
}
