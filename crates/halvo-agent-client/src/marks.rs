//! The mark of a run: the file `<data directory>/agents/marks/<run id>`, written before the runner starts and
//! completed with the runner's line after it, so that a later life of the library finds what may still run.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use halvo_schema::executable::text::{CreationTime, RunId};

/// The process group of a line: on macOS the group whose leader is the line's process, on Windows `job`, the job
/// object of the library's own that holds the runner and every process that it starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Leader(NonZeroU32),
    Job,
}

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
    format!("mark run={run}\n")
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

fn count(s: &str) -> Option<NonZeroU32> {
    if s.is_empty() || s.starts_with('0') || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

fn read_proc_line(word: &str, line: &str) -> Option<ProcLine> {
    let mut parts = line.split(' ');
    if parts.next()? != word {
        return None;
    }
    let pid = count(parts.next()?.strip_prefix("pid=")?)?;
    let created = CreationTime::parse(parts.next()?.strip_prefix("created=")?).ok()?;
    let group = match parts.next()?.strip_prefix("group=")? {
        "job" => Group::Job,
        g => Group::Leader(count(g)?),
    };
    if parts.next().is_some() {
        return None;
    }
    Some(ProcLine {
        pid,
        created,
        group,
    })
}

/// Reads the text of a mark whose file name is `run`: the first line must name that run, and a line that does
/// not end in a newline is not there yet. `None` when the first line is not whole or names another run.
pub fn parse(run: &RunId, text: &str) -> Option<Mark> {
    let mut lines = text
        .split_inclusive('\n')
        .filter_map(|l| l.strip_suffix('\n'));
    if lines.next()? != first_line(run).trim_end_matches('\n') {
        return None;
    }
    let runner = lines.next().and_then(|l| read_proc_line("runner", l));
    let browser = runner
        .and(lines.next())
        .and_then(|l| read_proc_line("browser", l));
    Some(Mark {
        run: run.clone(),
        runner,
        browser,
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
    fn a_line_that_is_not_whole_is_not_there() {
        let r = run();
        let first = first_line(&r);
        let cut = format!("{first}runner pid=812 created=17629");
        assert_eq!(parse(&r, &cut).unwrap().runner, None);
        assert_eq!(parse(&r, &first[..first.len() - 1]), None);
        let other = RunId::parse("ffffffffffffffffffffffffffffffff").unwrap();
        assert_eq!(parse(&other, &first), None);
    }

    #[test]
    fn a_line_out_of_its_form_is_not_read_as_a_process() {
        let r = run();
        let first = first_line(&r);
        for bad in [
            "runner pid=0 created=1 group=1\n",
            "runner pid=812 created=0 group=812\n",
            "runner pid=812 created=01 group=812\n",
            "runner pid=812 created=1 group=0\n",
            "runner pid=812 created=1 group=jobs\n",
            "runner pid=812 created=1 group=812 \n",
            "runner pid=+812 created=1 group=812\n",
            "runner pid=0812 created=1 group=812\n",
            "runner pid=812 created=1 group=0812\n",
            "runner  pid=812 created=1 group=812\n",
            "runner pid=812 group=812 created=1\n",
            "browser pid=812 created=1 group=812\n",
            "runner pid=812 created=123456789012345678901 group=812\n",
        ] {
            let text = format!("{first}{bad}");
            assert_eq!(parse(&r, &text).unwrap().runner, None, "{bad:?}");
        }
        let twice = format!(
            "{first}{}{}",
            proc_line("runner", &line(5, "6", Group::Job)),
            proc_line("runner", &line(5, "6", Group::Job))
        );
        assert_eq!(parse(&r, &twice).unwrap().browser, None);
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
