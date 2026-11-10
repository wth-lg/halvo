use std::fmt;
use std::path::Path;

use crate::book::{self, Book};
use crate::candidate::Candidate;
use crate::corpus::Spec;
use crate::parts::{self, Kind};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Cached {
    None,
    Fresh,
    Absent,
    Stale,
}

impl Cached {
    pub fn name(self) -> &'static str {
        match self {
            Cached::None => "none",
            Cached::Fresh => "fresh",
            Cached::Absent => "absent",
            Cached::Stale => "stale",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Judgement {
    pub target_written: bool,
    pub other_cells_same: bool,
    pub lost: Vec<Kind>,
    pub cached: Cached,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub candidate: String,
    pub file: String,
    pub judgement: Judgement,
}

impl fmt::Display for Line {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "sheet candidate={} file={} other_cells_same={} lost={} cached_results={}",
            self.candidate,
            self.file,
            if self.judgement.other_cells_same {
                "yes"
            } else {
                "no"
            },
            parts::list(&self.judgement.lost),
            self.judgement.cached.name()
        )
    }
}

pub fn cached_results(saved: &Book, spec: &Spec) -> Cached {
    spec.dependents
        .iter()
        .map(|dependent| {
            let value = book::parse_ref(dependent.cell)
                .and_then(|pos| saved.cell(dependent.sheet, pos))
                .map(|cell| &cell.value);
            match value {
                None => Cached::Absent,
                Some(value) if book::is_empty(value) => Cached::Absent,
                Some(value) if book::number(value) == Some(dependent.after) => Cached::Fresh,
                Some(_) => Cached::Stale,
            }
        })
        .max()
        .unwrap_or(Cached::None)
}

pub fn judge(original: &Path, saved: &Path, spec: &Spec) -> Result<Judgement, String> {
    let target = book::parse_ref(spec.cell).ok_or_else(|| format!("bad cell {}", spec.cell))?;
    let before = book::read(original)?;
    let after = book::read(saved)?;
    let target_written = after
        .cell(spec.sheet, target)
        .is_some_and(|cell| cell.formula.is_none() && book::number(&cell.value) == Some(spec.new));
    let other_cells_same = book::same_except(&before, &after, spec.sheet, target);
    let lost = parts::lost(&parts::inventory(original)?, &parts::inventory(saved)?);
    Ok(Judgement {
        target_written,
        other_cells_same,
        lost,
        cached: cached_results(&after, spec),
    })
}

pub fn run_cell(
    candidate: Candidate,
    spec: &Spec,
    corpus: &Path,
    out: &Path,
) -> Result<Line, String> {
    let original = corpus.join(spec.file);
    let dir = out.join(candidate.name());
    std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let saved = dir.join(spec.file);
    std::fs::copy(&original, &saved).map_err(|e| format!("copy {}: {e}", spec.file))?;
    candidate.change(&saved, spec.sheet, spec.cell, spec.new)?;
    let judgement = judge(&original, &saved, spec)?;
    if !judgement.target_written {
        return Err(format!(
            "the target cell {} does not hold the new value",
            spec.cell
        ));
    }
    Ok(Line {
        candidate: candidate.name().to_string(),
        file: spec.file.to_string(),
        judgement,
    })
}
