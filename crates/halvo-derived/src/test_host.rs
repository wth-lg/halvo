//! What a test host of the store adds, compiled only with the cargo feature `test-host`, which the owner binary does
//! not enable.

use std::fmt;
use std::io::{ErrorKind, Write};
use std::path::Path;

use halvo_schema_core::category::SeedCategory;
use halvo_schema_core::ids::{RequestId, SummaryId, WorkflowId};
use halvo_schema_core::record::{Workflow, WorkflowState};
use halvo_schema_core::request::{InterfaceRead, ReplyResult};
use serde::Serialize;

use crate::error::Error;
use crate::read::ReadHandle;

/// One stream of an export: its stream id and a handle of the interface-read role on its database.
pub struct Stream<'s> {
    pub id: &'s str,
    pub handle: ReadHandle<'s>,
}

/// Why `export_proposals` wrote nothing, or stopped.
#[derive(Debug)]
pub enum ExportError {
    /// The file exists; nothing is written.
    Exists,
    /// An argument breaks its form: the manifest hash, the build commit or a stream id, named here.
    Usage(&'static str),
    /// A read through a stream's handle failed.
    Read(Error),
    /// The file cannot be created or written.
    Write(ErrorKind),
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExportError::Exists => f.write_str("the proposals file exists"),
            ExportError::Usage(what) => write!(f, "the {what} breaks its form"),
            ExportError::Read(error) => write!(f, "a read failed: {error}"),
            ExportError::Write(kind) => write!(f, "the proposals file cannot be written: {kind:?}"),
        }
    }
}

impl std::error::Error for ExportError {}

fn lower_hex(s: &str, len: usize) -> bool {
    s.len() == len
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn stream_id(s: &str) -> bool {
    (1..=48).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

#[derive(Serialize)]
struct Header<'a> {
    manifest: &'a str,
    build: &'a str,
}

#[derive(Serialize)]
struct Line<'a> {
    stream: &'a str,
    category: SeedCategory,
    summaries: Vec<SummaryId>,
}

fn proposed(stream: &Stream<'_>) -> Result<Vec<(SeedCategory, Vec<SummaryId>)>, ExportError> {
    let mut found = Vec::new();
    let mut after: Option<WorkflowId> = None;
    loop {
        let request =
            InterfaceRead::workflows(stream.handle.role(), RequestId::from_random([0; 16]), after);
        let page = match stream.handle.read(&request).map_err(ExportError::Read)? {
            ReplyResult::Workflows(page) => page,
            _ => return Err(ExportError::Read(Error::Fault(crate::Fault::Row))),
        };
        for item in page.workflows {
            if let Workflow::Full(full) = item.workflow {
                if full.state == WorkflowState::Proposed && !item.summaries.is_empty() {
                    found.push((full.category, item.summaries));
                }
            }
        }
        match page.next {
            Some(next) => after = Some(next),
            None => return Ok(found),
        }
    }
}

/// Writes the proposals file of C13 through the interface read alone: the header `{"manifest":..,"build":..}` from
/// its arguments and one line `{"stream":..,"category":..,"summaries":[..]}` per stored workflow in the state
/// `proposed` with a summary in its support set, sorted by stream id in byte order, then by category in the order
/// of the category list, then by the first summary id and then by the whole list, with `summaries` in ascending
/// order. It reads no statement of its own, no key and no other record, writes no episode log, and returns the
/// number of proposal lines. It writes nothing when the file exists or an argument breaks its form, and returns the
/// first read or write error otherwise.
pub fn export_proposals(
    path: &Path,
    manifest: &str,
    build: &str,
    streams: &[Stream<'_>],
) -> Result<usize, ExportError> {
    if !lower_hex(manifest, 64) {
        return Err(ExportError::Usage("manifest hash"));
    }
    if !lower_hex(build, 40) {
        return Err(ExportError::Usage("build commit"));
    }
    if !streams.iter().all(|s| stream_id(s.id)) {
        return Err(ExportError::Usage("stream id"));
    }
    if path.exists() {
        return Err(ExportError::Exists);
    }
    let mut lines: Vec<(&str, SeedCategory, Vec<SummaryId>)> = Vec::new();
    for stream in streams {
        for (category, summaries) in proposed(stream)? {
            lines.push((stream.id, category, summaries));
        }
    }
    lines.sort_by(|a, b| {
        a.0.as_bytes()
            .cmp(b.0.as_bytes())
            .then(a.1.cmp(&b.1))
            .then_with(|| a.2.first().cmp(&b.2.first()))
            .then_with(|| a.2.cmp(&b.2))
    });
    let mut text = serde_json::to_string(&Header { manifest, build })
        .map_err(|_| ExportError::Write(ErrorKind::InvalidData))?;
    text.push('\n');
    for (stream, category, summaries) in &lines {
        let line = Line {
            stream,
            category: *category,
            summaries: summaries.clone(),
        };
        text.push_str(
            &serde_json::to_string(&line)
                .map_err(|_| ExportError::Write(ErrorKind::InvalidData))?,
        );
        text.push('\n');
    }
    let mut file = std::fs::File::options()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| match e.kind() {
            ErrorKind::AlreadyExists => ExportError::Exists,
            kind => ExportError::Write(kind),
        })?;
    file.write_all(text.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|e| ExportError::Write(e.kind()))?;
    Ok(lines.len())
}
