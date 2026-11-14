//! Reading the files of a body or of the golden directory: its streams, day files and records.

use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

use halvo_seam::record::{CaptureRecord, Edge, Payload, LINE_MAX};

/// A day file name, `day-<NNN>.jsonl` with NNN from 001 to 093.
pub fn day_of(name: &str) -> Option<u32> {
    let n = name.strip_prefix("day-")?.strip_suffix(".jsonl")?;
    if n.len() != 3 || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let d: u32 = n.parse().ok()?;
    (1..=93).contains(&d).then_some(d)
}

/// One stream directory of a body: its id, its day files in the order of name and the path of its label file.
#[derive(Clone, Debug)]
pub struct StreamDir {
    pub id: String,
    pub days: Vec<(u32, PathBuf)>,
    pub labels: PathBuf,
}

/// What `dir/streams/` holds: its stream directories, and the names of its entries that are symbolic links,
/// which are not followed (contracts.md C12.6, C12.8). Both in byte order of name.
#[derive(Clone, Debug, Default)]
pub struct Streams {
    pub dirs: Vec<StreamDir>,
    pub links: Vec<String>,
}

/// The directories under `dir/streams/`, in byte order of name, each with its day files, and the links there.
/// None when `dir/streams` is missing, is itself a symbolic link or cannot be read.
pub fn stream_dirs(dir: &Path) -> Option<Streams> {
    let root = dir.join("streams");
    if fs::symlink_metadata(&root).ok()?.file_type().is_symlink() {
        return None;
    }
    let mut names: Vec<String> = Vec::new();
    let mut links: Vec<String> = Vec::new();
    for entry in fs::read_dir(&root).ok()? {
        let entry = entry.ok()?;
        let kind = entry.file_type().ok()?;
        if kind.is_symlink() {
            links.push(entry.file_name().to_str()?.to_owned());
        } else if kind.is_dir() {
            names.push(entry.file_name().to_str()?.to_owned());
        }
    }
    names.sort();
    links.sort();
    let mut out = Vec::new();
    for id in names {
        let sd = root.join(&id);
        let mut days: Vec<(u32, PathBuf)> = Vec::new();
        for entry in fs::read_dir(&sd).ok()? {
            let entry = entry.ok()?;
            if let Some(d) = entry.file_name().to_str().and_then(day_of) {
                days.push((d, entry.path()));
            }
        }
        days.sort();
        out.push(StreamDir {
            labels: sd.join("labels.jsonl"),
            id,
            days,
        });
    }
    Some(Streams { dirs: out, links })
}

/// Calls `each` with every record of a batch file in line order. Err with the line number, counted from 1,
/// of the first line that is no record of C4, or 0 when the file cannot be read.
pub fn each_record(path: &Path, mut each: impl FnMut(CaptureRecord)) -> Result<(), usize> {
    let file = fs::File::open(path).map_err(|_| 0usize)?;
    let mut reader = BufReader::new(file);
    let mut buf = Vec::with_capacity(512);
    let mut n = 0usize;
    loop {
        buf.clear();
        let read = reader
            .by_ref()
            .take(LINE_MAX as u64 + 1)
            .read_until(b'\n', &mut buf)
            .map_err(|_| n + 1)?;
        if read == 0 {
            return Ok(());
        }
        n += 1;
        let record = CaptureRecord::read_line(&buf).map_err(|_| n)?;
        each(record);
    }
}

/// A title event as the files hold it: its instant, app id, text and site value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TitleEvent {
    pub at: i64,
    pub app: Option<String>,
    pub text: String,
    pub site: Option<String>,
}

/// What a check reads of one stream's records: its title events, its keystroke instants and its marks, each in
/// line order across the day files.
#[derive(Clone, Debug, Default)]
pub struct StreamRecords {
    pub titles: Vec<TitleEvent>,
    pub keys: Vec<i64>,
    pub marks: Vec<(i64, Edge)>,
}

/// Reads the day files of a stream. Err with the path and line of the first line that is no record.
pub fn read_stream(days: &[(u32, PathBuf)]) -> Result<StreamRecords, (PathBuf, usize)> {
    let mut out = StreamRecords::default();
    for (_, path) in days {
        each_record(path, |r| match r.payload() {
            Payload::WindowTitle(t) => out.titles.push(TitleEvent {
                at: r.at().0,
                app: r.app().map(|a| a.as_str().to_owned()),
                text: t.text.as_str().to_owned(),
                site: t.site.as_ref().map(|d| d.as_str().to_owned()),
            }),
            Payload::Keystroke(_) => out.keys.push(r.at().0),
            Payload::RunMark(e) => out.marks.push((r.at().0, *e)),
            _ => {}
        })
        .map_err(|line| (path.clone(), line))?;
    }
    Ok(out)
}
