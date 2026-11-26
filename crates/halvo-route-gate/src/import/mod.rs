//! `route-gate import` (contract T5.4): reads the fenced block of the agent-setup run's driver file
//! (AS-A13.6) and cells file (AS-A13.5), and of a hosted round's file where `--hosted` is given,
//! and writes `recorded.tsv`, `models.tsv`, `sources.tsv` and `freeze.txt` (T5.2, T5.3) into the
//! directory that `--out` names, after every input has parsed. Refusal causes: `usage`, `file`, and
//! `source` with the detail `<driver|cells|hosted>:<line number>`, `<driver|cells|hosted>:block`,
//! `driver:verdict` or `hosted:verdict`. The driver file is read before the cells file, and
//! each file in line order; a carriage return, a byte sequence that is no UTF-8, no LF at the end
//! or a byte-order mark at the start, anywhere in a file, is refused with its line before the
//! block is looked for. In the driver block a line that starts with `attempt` or `verdict` is of its form
//! and one that starts with `bound` is refused; in the cells block a line that starts with `bound`
//! is of its form, one that starts with `verdict` starts `verdict candidates=`, and one that starts
//! with `attempt` is refused. A line whose first bytes, after spaces and tabs alone and compared
//! without ASCII letter case, are a known word of its block (`attempt` and `verdict` in the driver
//! block, `bound` and `verdict` in the cells block) and that does not start with that word at its
//! first byte is refused with its line. Every other line of a block is passed over but for the
//! count below, and no line outside the block is read. Within one model, machine, task and seed,
//! `compile`, `recover` and `write` have `i=1` and `replay` has `i` from 1 to 3 with no gap; the
//! verdict's driver and writer have exactly the attempt lines of AS-A13.6, else `driver:verdict`.
//! In the driver block (a line above the opening fence is not read), a line whose words, divided
//! by spaces and tabs, have `limits` first and later `task=bill` or `task=order` while the verdict
//! names no driver, or `task=reply` while it names no writer, each compared without ASCII letter
//! case, is refused with its line; it is checked among the verdict's lines, in file order, after
//! the set (ruling SG-63, over T5.4's literal form until the planner's Revision).
//! The last line of each block starts with `verdict`, and the cells block holds a `bound` line.
//! After the set and the verdict's lines comes the count (U0.2c), for every model: within one
//! model, machine, task and seed, five attempt lines for each counted `drive` line and none
//! without one, and within one model, machine and seed, one `write` line for each counted `reply`
//! line and none without one. A counted line starts at its first byte with `drive model=<token>
//! machine=<m> task=<bill|order> seed=<n> ` or `reply model=<token> machine=<m> seed=<n> `, with
//! values of T5.1 and T5.2; the rest of it is not read, and a `drive` or `reply` line in any other
//! form is not counted. A cell that breaks the count is refused at its first counted line, or at
//! its first attempt line when it has none; of several such cells, the smallest line is named.
//! For the unit that writes the recorded table (U3.1): a refusal of the count means that the
//! trial file lost or gained a line of a cell. The cure is a rebuild of the file by the
//! agent-setup run from its raw files, never a line added or removed by hand.
//! Still passed over (named limits): a line of a known word hidden by a byte other than a space
//! or a tab, such as a no-break space or a form feed, or by a letter outside ASCII; a `limits`
//! line in another form; and a value typed by hand, which only a rebuild from the trial's raw
//! files tells (T5.4). A cell missing whole, its `drive` or `reply` line with its attempt lines,
//! is refused by the set when it is a `drive` cell of the verdict's driver or a `reply` cell of
//! its writer; any other cell missing whole imports with its rows fewer and no refusal. A cell
//! that ends `notrun` after its compile and leaves its attempt lines with no `drive` line is
//! refused, until the agent-setup run answers whether its harness writes such a cell (its pass
//! inbox, 2026-11-15T00:50:19Z, question 3).
//!
//! The hosted block (`--hosted` with `--date s3=`) is read after the cells file, with the text and
//! the fence of the driver file. Every line of it is of one of eight kinds, each in its exact form,
//! and any other line, a `spend`, `egress` or `host` line among them, is refused at its line; no
//! form holds a path. `commit` is its first line and `verdict` its last, each once, and `records`
//! stands once. Each line names `machine=mac` and a model whose token is the model word of exactly
//! one row of the table of hosts, with `_` for `-`; an attempt line makes a row of that row's tier
//! with the level `medium` and the source `s3`. `compile` and `write` have `i` 1 or 2 and `replay`
//! 1 to 6, the second pass's replays 4 to 6; an attempt line of the kind `recover` is refused, and
//! a `drive` line holds `recover=notread`. Within one model, task and seed, four attempt lines
//! stand for each `drive` line, one `write` line for each `reply` line, and one `marks` line for
//! each of either. The verdict's driver has the attempt lines and the twelve passed `drive` lines
//! of both passes, and its writer the six `reply` lines with `bodies=2`. The freeze file then holds
//! `sources=3` and a third line, `frontier driver=<token|none>`, whose token is the verdict's
//! driver where it is the model of the row of `frontier`; no line fills `low_cost_hosted`. Without
//! `--hosted` the four files are those of the two files alone.

mod blob;
mod block;
mod hosted;
mod lines;
mod set;

use std::collections::HashSet;
use std::io::Write;
use std::path::Path;

pub use blob::blob_id;
pub use hosted::HostModels;

use crate::out::{self, Refused};
use crate::table::{Machine, Model, Table};
use block::{block, BlockFault};

/// The four files that `import` writes, in the order in which it writes them.
pub const FILES: [&str; 4] = ["recorded.tsv", "models.tsv", "sources.tsv", "freeze.txt"];

const MODELS_HEADER: &str = "model\tmachine\tbound_bytes\tfits\tgroup\tsource";
const SOURCES_HEADER: &str = "id\tpath\tblob\tdate\tmeasures\tlimits";

/// One of the two trial files, or the file of a hosted round.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Input {
    Driver,
    Cells,
    Hosted,
}

impl Input {
    pub const fn as_str(self) -> &'static str {
        match self {
            Input::Driver => "driver",
            Input::Cells => "cells",
            Input::Hosted => "hosted",
        }
    }
}

/// Why a trial file does not parse: the detail of `REFUSED source`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SourceError {
    /// The line of this number, counted from the first line of the file.
    Line(Input, usize),
    /// The file does not hold exactly two fence lines, an opening and then a closing, or the
    /// cells block holds no `bound` line.
    Block(Input),
    /// The driver block or the hosted block holds no line or more than one line that starts
    /// `verdict`, or the verdict's driver or writer lacks an attempt line of the set or has a
    /// further one, or lacks one of its `drive` or `reply` lines.
    Verdict(Input),
}

impl SourceError {
    /// `<driver|cells|hosted>:<line number>`, `<driver|cells|hosted>:block`, `driver:verdict` or
    /// `hosted:verdict`.
    pub fn detail(&self) -> String {
        match self {
            SourceError::Line(input, line) => format!("{}:{line}", input.as_str()),
            SourceError::Block(input) => format!("{}:block", input.as_str()),
            SourceError::Verdict(input) => format!("{}:verdict", input.as_str()),
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
    pub sources: usize,
}

impl Imported {
    /// `import rows=<n> models=<n> sources=<n>`.
    pub fn line(&self) -> String {
        format!(
            "import rows={} models={} sources={}",
            self.rows, self.models, self.sources
        )
    }
}

/// Parses both trial files and makes the four files, or names the first fault: the driver file
/// before the cells file, each in line order; after the lines of the driver block a gap of the
/// numbering, the count of verdict lines, the last line, the set of the verdict's driver and writer
/// and their `drive` and `reply` lines, and then the count of attempt lines against the counted
/// `drive` and `reply` lines of every model; after the lines of the cells block its last line and
/// then whether it holds a `bound` line (`cells:block`).
pub fn import(driver: Trial<'_>, cells: Trial<'_>) -> Result<Imported, SourceError> {
    imports(driver, cells, None)
}

/// Parses both trial files as `import` does and then the block of a hosted round, `hosted`, whose
/// models take the tiers that `models` gives them, and makes the four files with its rows, its
/// row of sources.tsv and the third line of the freeze file.
pub fn import_hosted(
    driver: Trial<'_>,
    cells: Trial<'_>,
    hosted: Trial<'_>,
    models: &HostModels,
) -> Result<Imported, SourceError> {
    imports(driver, cells, Some((hosted, models)))
}

fn imports(
    driver: Trial<'_>,
    cells: Trial<'_>,
    hosted: Option<(Trial<'_>, &HostModels)>,
) -> Result<Imported, SourceError> {
    let block_of = |input: Input, bytes| {
        block(bytes).map_err(|fault| match fault {
            BlockFault::Text(line) => SourceError::Line(input, line),
            BlockFault::Block => SourceError::Block(input),
        })
    };
    let mut rows = Vec::new();
    let mut attempts = set::Attempts::new();
    let mut keyed = Vec::new();
    let mut verdicts = Vec::new();
    let mut said = Vec::new();
    let mut counted = Vec::new();
    let driver_block = block_of(Input::Driver, driver.bytes)?;
    for &line in &driver_block {
        let fault = SourceError::Line(Input::Driver, line.number);
        if line.bytes.starts_with(lines::ATTEMPT) {
            let row = lines::attempt(line.bytes)
                .filter(lines::numbered)
                .ok_or(fault)?;
            let key = lines::attempt_key(&row);
            if attempts.insert(key.clone(), row.pass).is_some() {
                return Err(fault);
            }
            keyed.push((line.number, key));
            rows.push(row);
        } else if line.bytes.starts_with(lines::VERDICT) {
            verdicts.push(lines::verdict(line.bytes).ok_or(fault)?);
        } else if line.bytes.starts_with(lines::BOUND)
            || lines::written_another_way(line.bytes, &lines::DRIVER_WORDS)
        {
            return Err(fault);
        } else if line.bytes.starts_with(lines::DRIVE)
            || line.bytes.starts_with(lines::REPLY)
            || lines::limits(line.bytes).is_some()
        {
            said.push((line.number, line.bytes));
            if let Some(cell) = lines::counted(line.bytes) {
                counted.push((line.number, cell));
            }
        }
    }
    if let Some(number) = set::gap(&keyed, &attempts) {
        return Err(SourceError::Line(Input::Driver, number));
    }
    let [verdict] = <[lines::Verdict; 1]>::try_from(verdicts)
        .map_err(|_| SourceError::Verdict(Input::Driver))?;
    last_line(Input::Driver, &driver_block)?;
    if !set::holds(&verdict, &attempts) {
        return Err(SourceError::Verdict(Input::Driver));
    }
    set::verdict_lines(&verdict, &said, &attempts).map_err(|unsaid| match unsaid {
        set::Unsaid::Line(number) => SourceError::Line(Input::Driver, number),
        set::Unsaid::Missing => SourceError::Verdict(Input::Driver),
    })?;
    if let Some(number) = set::count(&keyed, &counted) {
        return Err(SourceError::Line(Input::Driver, number));
    }
    let mut bounds = Vec::new();
    let mut pairs: HashSet<(Model, Machine)> = HashSet::new();
    let cells_block = block_of(Input::Cells, cells.bytes)?;
    for &line in &cells_block {
        let fault = SourceError::Line(Input::Cells, line.number);
        if line.bytes.starts_with(lines::BOUND) {
            let bound = lines::bound(line.bytes).ok_or(fault)?;
            if !pairs.insert((bound.model.clone(), bound.machine)) {
                return Err(fault);
            }
            bounds.push(bound);
        } else if line.bytes.starts_with(lines::VERDICT)
            && !line.bytes.starts_with(lines::CANDIDATES)
            || line.bytes.starts_with(lines::ATTEMPT)
            || lines::written_another_way(line.bytes, &lines::CELLS_WORDS)
        {
            return Err(fault);
        }
    }
    last_line(Input::Cells, &cells_block)?;
    if bounds.is_empty() {
        return Err(SourceError::Block(Input::Cells));
    }
    let hosted = match hosted {
        Some((trial, models)) => {
            let read = hosted::read(&block_of(Input::Hosted, trial.bytes)?, models)?;
            Some((trial, read))
        }
        None => None,
    };
    let mut frontier = None;
    if let Some((_, read)) = hosted.as_ref() {
        rows.extend(read.rows.iter().cloned());
        frontier = Some(&read.driver);
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
    let mut sources = format!(
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
    let mut date = driver.date.max(cells.date);
    let mut count = 2;
    if let Some((trial, _)) = hosted.as_ref() {
        sources.push_str(&format!(
            "s3\t{}\t{}\t{}\thosted_driver_attempts\t3_seeds_1_machine_2_passes_team_fixtures\n",
            trial.path,
            blob_id(trial.bytes),
            trial.date.as_str(),
        ));
        date = date.max(trial.date);
        count = 3;
    }
    let name = |model: &Option<Model>| model.as_ref().map_or("none", Model::as_str).to_owned();
    let mut freeze = format!(
        "freeze rows={} models={} sources={count} date={}\non_device driver={} writer={}\n",
        rows.len(),
        bounds.len(),
        date.as_str(),
        name(&verdict.driver),
        name(&verdict.writer),
    );
    if let Some(driver) = frontier {
        freeze.push_str(&format!("frontier driver={}\n", name(driver)));
    }
    Ok(Imported {
        files: [
            (FILES[0], table.to_text()),
            (FILES[1], models),
            (FILES[2], sources),
            (FILES[3], freeze),
        ],
        rows: rows.len(),
        models: bounds.len(),
        sources: count,
    })
}

fn last_line(input: Input, block: &[block::Line<'_>]) -> Result<(), SourceError> {
    match block.last() {
        Some(line) if !line.bytes.starts_with(lines::VERDICT) => {
            Err(SourceError::Line(input, line.number))
        }
        _ => Ok(()),
    }
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
    hosted: Option<(&'a str, Date)>,
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
    let (mut driver, mut cells, mut hosted, mut out) = (None, None, None, None);
    let (mut s1, mut s2, mut s3) = (None, None, None);
    let mut words = args.iter();
    while let Some(name) = words.next() {
        let slot = match name.as_str() {
            "--driver" => &mut driver,
            "--cells" => &mut cells,
            "--hosted" => &mut hosted,
            "--out" => &mut out,
            "--date" => {
                let Some(value) = words.next() else {
                    return refuse("usage", "--date has no value");
                };
                let (id, date) = match value.split_once('=') {
                    Some(("s1", date)) => ("s1", date),
                    Some(("s2", date)) => ("s2", date),
                    Some(("s3", date)) => ("s3", date),
                    _ => {
                        return refuse(
                            "usage",
                            "--date takes s1=<YYYY-MM-DD>, s2=<YYYY-MM-DD> or s3=<YYYY-MM-DD>",
                        )
                    }
                };
                let Some(date) = Date::parse(date) else {
                    return refuse(
                        "usage",
                        format!("--date {id} is not a calendar date YYYY-MM-DD"),
                    );
                };
                let slot = match id {
                    "s1" => &mut s1,
                    "s2" => &mut s2,
                    _ => &mut s3,
                };
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
    let hosted = match (hosted, s3) {
        (Some(path), Some(date)) => Some((path, date)),
        (None, None) => None,
        _ => return refuse("usage", "--hosted and --date s3=<YYYY-MM-DD> come together"),
    };
    let paths = [("--driver", driver), ("--cells", cells)]
        .into_iter()
        .chain(hosted.as_ref().map(|(path, _)| ("--hosted", *path)));
    for (name, path) in paths {
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
        hosted,
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
    let hosted = match &call.hosted {
        Some((path, date)) => {
            let Ok(bytes) = std::fs::read(path) else {
                return refuse("file", "--hosted cannot be read");
            };
            Some((*path, bytes, date))
        }
        None => None,
    };
    if !Path::new(call.out).is_dir() {
        return refuse("file", "--out is not a directory");
    }
    for name in FILES {
        if std::fs::symlink_metadata(Path::new(call.out).join(name)).is_ok() {
            return refuse("file", format!("--out holds {name}"));
        }
    }
    let driver = Trial {
        path: call.driver,
        bytes: &driver,
        date: &call.s1,
    };
    let cells = Trial {
        path: call.cells,
        bytes: &cells,
        date: &call.s2,
    };
    let imported = match &hosted {
        Some((path, bytes, date)) => {
            let hosted = Trial { path, bytes, date };
            let models = HostModels::of_ruling(&halvo_route::release::ruling());
            import_hosted(driver, cells, hosted, &models)
        }
        None => import(driver, cells),
    }
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
