//! `route-gate import` (contract T5.4): reads the fenced block of the agent-setup run's driver file
//! (AS-A13.6) and cells file (AS-A13.5) and writes `recorded.tsv`, `models.tsv`, `sources.tsv` and
//! `freeze.txt` (T5.2, T5.3) into the directory that `--out` names, after both inputs have parsed.
//! Refusal causes: `usage`, `file`, and `source` with the detail `<driver|cells>:<line number>`,
//! `<driver|cells>:block` or `driver:verdict`. The driver file is read before the cells file, and
//! each file in line order; a carriage return anywhere in a file is refused before its block is
//! looked for. Known lines are those of their own file; every other line of a block, a known line
//! of the other file among them, is passed over, and no line outside the block is read.

mod blob;
mod block;
mod lines;

use std::collections::HashSet;
use std::io::Write;
use std::path::Path;

pub use blob::blob_id;

use crate::out::{self, Refused};
use crate::table::{Machine, Model, Table};
use block::{block, BlockFault};

/// The four files that `import` writes, in the order in which it writes them.
pub const FILES: [&str; 4] = ["recorded.tsv", "models.tsv", "sources.tsv", "freeze.txt"];

const MODELS_HEADER: &str = "model\tmachine\tbound_bytes\tfits\tgroup\tsource";
const SOURCES_HEADER: &str = "id\tpath\tblob\tdate\tmeasures\tlimits";

/// One of the two trial files.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Input {
    Driver,
    Cells,
}

impl Input {
    pub const fn as_str(self) -> &'static str {
        match self {
            Input::Driver => "driver",
            Input::Cells => "cells",
        }
    }
}

/// Why a trial file does not parse: the detail of `REFUSED source`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SourceError {
    /// The line of this number, counted from the first line of the file.
    Line(Input, usize),
    /// No block, two blocks, or a block that is not closed.
    Block(Input),
    /// The driver file holds no line or more than one line that starts `verdict driver=`.
    Verdict,
}

impl SourceError {
    /// `<driver|cells>:<line number>`, `<driver|cells>:block` or `driver:verdict`.
    pub fn detail(&self) -> String {
        match self {
            SourceError::Line(input, line) => format!("{}:{line}", input.as_str()),
            SourceError::Block(input) => format!("{}:block", input.as_str()),
            SourceError::Verdict => "driver:verdict".to_owned(),
        }
    }
}

/// A calendar date written `YYYY-MM-DD`.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Date(String);

impl Date {
    pub fn parse(text: &str) -> Option<Date> {
        let b = text.as_bytes();
        let form = b.len() == 10
            && b[4] == b'-'
            && b[7] == b'-'
            && b.iter()
                .enumerate()
                .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit());
        if !form {
            return None;
        }
        let year: u32 = text[..4].parse().ok()?;
        let month: u32 = text[5..7].parse().ok()?;
        let day: u32 = text[8..].parse().ok()?;
        let leap =
            year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
        let last = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap => 29,
            2 => 28,
            _ => return None,
        };
        (1..=last).contains(&day).then(|| Date(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A trial file as the importer takes it: the path as it was given, its bytes and its date.
#[derive(Clone, Copy, Debug)]
pub struct Trial<'a> {
    pub path: &'a str,
    pub bytes: &'a [u8],
    pub date: &'a Date,
}

/// What `import` makes: the text of each of the four files, by the names of `FILES`, and the counts
/// of the printed line.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Imported {
    pub files: [(&'static str, String); 4],
    pub rows: usize,
    pub models: usize,
}

impl Imported {
    /// `import rows=<n> models=<n> sources=2`.
    pub fn line(&self) -> String {
        format!("import rows={} models={} sources=2", self.rows, self.models)
    }
}

/// Parses both trial files and makes the four files, or names the first fault: the driver file
/// before the cells file, each in line order, and the count of verdict lines after the lines of
/// the driver file.
pub fn import(driver: Trial<'_>, cells: Trial<'_>) -> Result<Imported, SourceError> {
    let block_of = |input: Input, bytes| {
        block(bytes).map_err(|fault| match fault {
            BlockFault::CarriageReturn(line) => SourceError::Line(input, line),
            BlockFault::Block => SourceError::Block(input),
        })
    };
    let mut rows = Vec::new();
    let mut keys = HashSet::new();
    let mut verdicts = Vec::new();
    for line in block_of(Input::Driver, driver.bytes)? {
        let fault = SourceError::Line(Input::Driver, line.number);
        if line.bytes.starts_with(lines::ATTEMPT) {
            let row = lines::attempt(line.bytes).ok_or(fault)?;
            if !keys.insert(lines::attempt_key(&row)) {
                return Err(fault);
            }
            rows.push(row);
        } else if line.bytes.starts_with(lines::VERDICT) {
            verdicts.push(lines::verdict(line.bytes).ok_or(fault)?);
        }
    }
    let [verdict] = <[lines::Verdict; 1]>::try_from(verdicts).map_err(|_| SourceError::Verdict)?;
    let mut bounds = Vec::new();
    let mut pairs: HashSet<(Model, Machine)> = HashSet::new();
    for line in block_of(Input::Cells, cells.bytes)? {
        if line.bytes.starts_with(lines::BOUND) {
            let fault = SourceError::Line(Input::Cells, line.number);
            let bound = lines::bound(line.bytes).ok_or(fault)?;
            if !pairs.insert((bound.model.clone(), bound.machine)) {
                return Err(fault);
            }
            bounds.push(bound);
        }
    }

    rows.sort_by_cached_key(|row| row.to_line());
    let table = Table::from_rows(&rows).expect("the rows of attempt lines are rows of T5.1");
    let mut model_lines: Vec<String> = bounds.iter().map(lines::Bound::to_line).collect();
    model_lines.sort();
    let mut models = String::from(MODELS_HEADER);
    models.push('\n');
    for line in &model_lines {
        models.push_str(line);
        models.push('\n');
    }
    let sources = format!(
        "{SOURCES_HEADER}\n\
         s1\t{}\t{}\t{}\tdriver_trial_attempts\t3_seeds_2_machines_team_fixtures\n\
         s2\t{}\t{}\t{}\tfootprint_bounds\t5_runs_a_cell_no_16gb_laptop\n",
        driver.path,
        blob_id(driver.bytes),
        driver.date.as_str(),
        cells.path,
        blob_id(cells.bytes),
        cells.date.as_str(),
    );
    let name = |model: &Option<Model>| model.as_ref().map_or("none", Model::as_str).to_owned();
    let freeze = format!(
        "freeze rows={} models={} sources=2 date={}\non_device driver={} writer={}\n",
        rows.len(),
        bounds.len(),
        driver.date.max(cells.date).as_str(),
        name(&verdict.driver),
        name(&verdict.writer),
    );
    Ok(Imported {
        files: [
            (FILES[0], table.to_text()),
            (FILES[1], models),
            (FILES[2], sources),
            (FILES[3], freeze),
        ],
        rows: rows.len(),
        models: bounds.len(),
    })
}

/// Why `write_files` left none of its own files in the directory.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum WriteFault {
    /// A file stands at the path of this name, before the call or by the time of its link.
    Exists(String),
    /// A file could not be written or put in place.
    Unwritable,
}

/// Writes each file into `dir` through `out::write_new`, in order, calling `before_link` with the
/// name before each link. When one write fails, the call removes the files that it put in place
/// before it, never a file that it did not make, and returns the fault.
pub fn write_files(
    dir: &str,
    files: &[(&str, String)],
    before_link: &mut dyn FnMut(&str),
) -> Result<(), WriteFault> {
    let mut made = Vec::new();
    for (name, text) in files {
        let path = Path::new(dir).join(name).to_string_lossy().into_owned();
        match out::write_new(&path, text, || before_link(name)) {
            Ok(()) => made.push(path),
            Err(fault) => {
                remove(&made);
                return Err(match fault {
                    Refused::Exists => WriteFault::Exists((*name).to_owned()),
                    Refused::Unwritable => WriteFault::Unwritable,
                });
            }
        }
    }
    Ok(())
}

fn remove(paths: &[String]) {
    for path in paths {
        let _ = std::fs::remove_file(path);
    }
}

struct Refusal {
    cause: &'static str,
    detail: String,
}

fn refuse<T>(cause: &'static str, detail: impl Into<String>) -> Result<T, Refusal> {
    Err(Refusal {
        cause,
        detail: detail.into(),
    })
}

struct Call<'a> {
    driver: &'a str,
    cells: &'a str,
    out: &'a str,
    s1: Date,
    s2: Date,
}

fn printable(word: &str) -> String {
    if word.starts_with("--") && word.bytes().all(|b| b.is_ascii_lowercase() || b == b'-') {
        word.to_owned()
    } else {
        "of another form".to_owned()
    }
}

fn path_cell(path: &str) -> bool {
    !path.is_empty()
        && path != "-"
        && path
            .bytes()
            .all(|b| b >= 0x20 && b != 0x7f && b != b'"' && b != b'\'')
}

fn call(args: &[String]) -> Result<Call<'_>, Refusal> {
    let (mut driver, mut cells, mut out, mut s1, mut s2) = (None, None, None, None, None);
    let mut words = args.iter();
    while let Some(name) = words.next() {
        let slot = match name.as_str() {
            "--driver" => &mut driver,
            "--cells" => &mut cells,
            "--out" => &mut out,
            "--date" => {
                let Some(value) = words.next() else {
                    return refuse("usage", "--date has no value");
                };
                let (id, date) = match value.split_once('=') {
                    Some(("s1", date)) => ("s1", date),
                    Some(("s2", date)) => ("s2", date),
                    _ => return refuse("usage", "--date takes s1=<YYYY-MM-DD> or s2=<YYYY-MM-DD>"),
                };
                let Some(date) = Date::parse(date) else {
                    return refuse(
                        "usage",
                        format!("--date {id} is not a calendar date YYYY-MM-DD"),
                    );
                };
                let slot = if id == "s1" { &mut s1 } else { &mut s2 };
                if slot.replace(date).is_some() {
                    return refuse("usage", format!("--date {id} twice"));
                }
                continue;
            }
            _ => return refuse("usage", format!("unknown argument {}", printable(name))),
        };
        let Some(value) = words.next() else {
            return refuse("usage", format!("{name} has no value"));
        };
        if slot.replace(value.as_str()).is_some() {
            return refuse("usage", format!("{name} twice"));
        }
    }
    let (Some(driver), Some(cells), Some(out)) = (driver, cells, out) else {
        return refuse("usage", "--driver, --cells and --out are required");
    };
    let (Some(s1), Some(s2)) = (s1, s2) else {
        return refuse(
            "usage",
            "--date s1=<YYYY-MM-DD> and --date s2=<YYYY-MM-DD> are required",
        );
    };
    for (name, path) in [("--driver", driver), ("--cells", cells)] {
        if !path_cell(path) {
            return refuse(
                "usage",
                format!("{name} is not a path that a cell of sources.tsv can hold"),
            );
        }
    }
    Ok(Call {
        driver,
        cells,
        out,
        s1,
        s2,
    })
}

fn run(args: &[String]) -> Result<(Imported, &str), Refusal> {
    let call = call(args)?;
    let Ok(driver) = std::fs::read(call.driver) else {
        return refuse("file", "--driver cannot be read");
    };
    let Ok(cells) = std::fs::read(call.cells) else {
        return refuse("file", "--cells cannot be read");
    };
    if !Path::new(call.out).is_dir() {
        return refuse("file", "--out is not a directory");
    }
    for name in FILES {
        if std::fs::symlink_metadata(Path::new(call.out).join(name)).is_ok() {
            return refuse("file", format!("--out holds {name}"));
        }
    }
    let imported = import(
        Trial {
            path: call.driver,
            bytes: &driver,
            date: &call.s1,
        },
        Trial {
            path: call.cells,
            bytes: &cells,
            date: &call.s2,
        },
    )
    .or_else(|error| refuse("source", error.detail()))?;
    let files: Vec<(&str, String)> = imported
        .files
        .iter()
        .map(|(name, text)| (*name, text.clone()))
        .collect();
    match write_files(call.out, &files, &mut |_| {}) {
        Ok(()) => Ok((imported, call.out)),
        Err(WriteFault::Exists(name)) => refuse("file", format!("--out holds {name}")),
        Err(WriteFault::Unwritable) => refuse("file", "--out cannot be written"),
    }
}

/// Runs `route-gate import` with the arguments after the command's name and returns its exit
/// status (T0.3): 0 with the line on `stdout`; 2 with one line `REFUSED <cause>: <detail>` on
/// `stderr`, nothing on `stdout` and no file written; 1 when the line cannot be written to `stdout`,
/// after the four files have been removed again, with nothing on `stderr`.
pub fn command(args: &[String], stdout: &mut dyn Write, stderr: &mut dyn Write) -> u8 {
    match run(args) {
        Ok((imported, dir)) => {
            let line = format!("{}\n", imported.line());
            if stdout
                .write_all(line.as_bytes())
                .and_then(|()| stdout.flush())
                .is_ok()
            {
                0
            } else {
                let made: Vec<String> = FILES
                    .iter()
                    .map(|name| Path::new(dir).join(name).to_string_lossy().into_owned())
                    .collect();
                remove(&made);
                1
            }
        }
        Err(refusal) => {
            let _ = writeln!(stderr, "REFUSED {}: {}", refusal.cause, refusal.detail);
            2
        }
    }
}
