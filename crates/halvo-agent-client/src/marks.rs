//! The mark of a run: the file `<data directory>/agents/marks/<run id>`, written before the runner starts and
//! completed with the runner's line after it, so that a later life of the library finds what may still run. Its
//! lines are read by halvo-control's `marks`, their one reader, which the capture service's delete uses too.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use halvo_control::marks as reader;
pub use halvo_control::marks::Group;
use halvo_schema::executable::text::{CreationTime, RunId};

/// One line `runner ...` or `browser ...` of a mark: the process number, the creation time as the process table
/// records it, and the group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcLine {
    pub pid: NonZeroU32,
    pub created: CreationTime,
    pub group: Group,
}

/// A mark as a later life reads it: the run, and the runner's and the browser's lines where they were written
/// whole. A mark without the runner's line is not complete.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mark {
    pub run: RunId,
    pub runner: Option<ProcLine>,
    pub browser: Option<ProcLine>,
}

impl Mark {
    /// Whether the mark holds the runner's line.
    pub fn is_complete(&self) -> bool {
        self.runner.is_some()
    }
}

/// The directory of the marks under a data directory.
pub fn marks_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("agents").join("marks")
}

/// The text of the first line.
pub fn first_line(run: &RunId) -> String {
    reader::first_line(run.as_str())
}

/// The text of a `runner` or `browser` line.
pub fn proc_line(word: &str, line: &ProcLine) -> String {
    let group = match line.group {
        Group::Leader(g) => g.to_string(),
        Group::Job => "job".to_owned(),
    };
    format!(
        "{word} pid={} created={} group={group}\n",
        line.pid, line.created
    )
}

impl From<reader::Line> for ProcLine {
    fn from(line: reader::Line) -> ProcLine {
        ProcLine {
            pid: line.pid,
            created: CreationTime::from_micros(line.created),
            group: line.group,
        }
    }
}

/// Reads the text of a mark whose file name is `run` by [`halvo_control::marks::read`]: the first line must name
/// that run, and a line that does not end in a newline is not there yet. `None` when the first line is not whole or
/// names another run.
pub fn parse(run: &RunId, text: &str) -> Option<Mark> {
    let lines = reader::read(run.as_str(), text)?;
    Some(Mark {
        run: run.clone(),
        runner: lines.runner.map(ProcLine::from),
        browser: lines.browser.map(ProcLine::from),
    })
}

/// The open mark of a run that this life is starting.
pub(crate) struct MarkFile {
    file: File,
}

/// What a create of a mark found.
pub(crate) enum Created {
    Made(MarkFile),
    Exists,
}

impl MarkFile {
    /// Makes the mark with its first line, refusing one that exists.
    pub(crate) fn create(data_dir: &Path, run: &RunId) -> io::Result<Created> {
        let dir = marks_dir(data_dir);
        fs::create_dir_all(&dir)?;
        let path = dir.join(run.as_str());
        let file = match OpenOptions::new().append(true).create_new(true).open(&path) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => return Ok(Created::Exists),
            Err(e) => return Err(e),
        };
        let mut mark = MarkFile { file };
        if let Err(e) = mark.append(&first_line(run)) {
            let _ = fs::remove_file(&path);
            return Err(e);
        }
        Ok(Created::Made(mark))
    }

    /// Appends one line.
    pub(crate) fn append(&mut self, line: &str) -> io::Result<()> {
        self.file.write_all(line.as_bytes())?;
        self.file.flush()
    }
}

/// Removes the mark of `run`; one that is not there is no error.
pub fn remove(data_dir: &Path, run: &RunId) -> io::Result<()> {
    match fs::remove_file(marks_dir(data_dir).join(run.as_str())) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Every mark under the data directory, by its run: a file whose name is no run id is not a mark and is left. A
/// mark whose first line is not whole reads as a mark with no line but the first.
pub fn read_all(data_dir: &Path) -> io::Result<Vec<Mark>> {
    let entries = match fs::read_dir(marks_dir(data_dir)) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut marks = Vec::new();
    for entry in entries {
        let entry = entry?;
        let Some(run) = entry
            .file_name()
            .to_str()
            .and_then(|n| RunId::parse(n).ok())
        else {
            continue;
        };
        if !entry.file_type()?.is_file() {
            continue;
        }
        let bytes = fs::read(entry.path())?;
        let mark = std::str::from_utf8(&bytes)
            .ok()
            .and_then(|text| parse(&run, text))
            .unwrap_or(Mark {
                run,
                runner: None,
                browser: None,
            });
        marks.push(mark);
    }
    marks.sort_by(|a, b| a.run.cmp(&b.run));
    Ok(marks)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run() -> RunId {
        RunId::parse("0123456789abcdef0123456789abcdef").unwrap()
    }

    fn line(pid: u32, created: &str, group: Group) -> ProcLine {
        ProcLine {
            pid: NonZeroU32::new(pid).unwrap(),
            created: CreationTime::parse(created).unwrap(),
            group,
        }
    }

    fn dir(name: &str) -> PathBuf {
        let exe = std::env::current_exe().unwrap();
        let target = exe.ancestors().nth(3).unwrap().to_path_buf();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let d = target
            .join("tmp")
            .join("agent-client-unit")
            .join(format!("{name}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_lines_have_the_form_of_the_contract() {
        assert_eq!(
            first_line(&run()),
            "mark run=0123456789abcdef0123456789abcdef\n"
        );
        let leader = line(
            812,
            "1762900000123456",
            Group::Leader(NonZeroU32::new(812).unwrap()),
        );
        assert_eq!(
            proc_line("runner", &leader),
            "runner pid=812 created=1762900000123456 group=812\n"
        );
        assert_eq!(
            proc_line("browser", &line(9, "1", Group::Job)),
            "browser pid=9 created=1 group=job\n"
        );
    }

    #[test]
    fn a_mark_reads_back_and_is_complete_only_with_the_runner_line() {
        let r = run();
        let runner = line(
            812,
            "1762900000123456",
            Group::Leader(NonZeroU32::new(812).unwrap()),
        );
        let browser = line(
            813,
            "1762900000999999",
            Group::Leader(NonZeroU32::new(813).unwrap()),
        );
        let first = first_line(&r);
        let only = parse(&r, &first).unwrap();
        assert_eq!(only.runner, None);
        assert!(!only.is_complete());
        let two = format!("{first}{}", proc_line("runner", &runner));
        let m = parse(&r, &two).unwrap();
        assert!(m.is_complete());
        assert_eq!(m.runner, Some(runner));
        assert_eq!(m.browser, None);
        let three = format!("{two}{}", proc_line("browser", &browser));
        assert_eq!(parse(&r, &three).unwrap().browser, Some(browser));
        let job = format!("{first}{}", proc_line("runner", &line(4, "77", Group::Job)));
        assert_eq!(parse(&r, &job).unwrap().runner.unwrap().group, Group::Job);
    }

    #[test]
    fn read_all_reads_a_mark_and_remove_takes_it() {
        let d = dir("read-remove");
        let r = run();
        fs::create_dir_all(marks_dir(&d)).unwrap();
        let text = format!(
            "{}{}",
            first_line(&r),
            proc_line("runner", &line(3, "4", Group::Job))
        );
        fs::write(marks_dir(&d).join(r.as_str()), text).unwrap();
        let all = read_all(&d).unwrap();
        assert_eq!(all.len(), 1);
        assert!(all[0].is_complete());
        remove(&d, &r).unwrap();
        assert!(read_all(&d).unwrap().is_empty());
        remove(&d, &r).unwrap();
    }

    #[test]
    fn read_all_leaves_a_file_whose_name_is_no_run_id_and_reads_no_marks_dir_as_none() {
        let d = dir("read-all");
        assert!(read_all(&d).unwrap().is_empty());
        fs::create_dir_all(marks_dir(&d)).unwrap();
        fs::write(marks_dir(&d).join("notes.txt"), "mark run=x\n").unwrap();
        let r = run();
        fs::write(marks_dir(&d).join(r.as_str()), "mark run=").unwrap();
        let all = read_all(&d).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].run, r);
        assert!(!all[0].is_complete());
        assert!(marks_dir(&d).join("notes.txt").exists());
    }
}
