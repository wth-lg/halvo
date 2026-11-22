//! The reader of a run's mark, the file `agents/marks/<run id>` under a data directory: the agent client writes its
//! first line before it starts a run's runner, then the runner's line and the browser's line once each has started.
//! The agent client reads its marks through this reader, and so does the capture service's delete of everything,
//! which ends the runner and the browser that each mark names; neither holds a reader of its own.

use std::num::{NonZeroU32, NonZeroU64};
use std::str::FromStr;

/// The process group of a line: on macOS the group whose leader is the line's process, on Windows `job`, the job
/// object of the library's own that holds the runner and every process that it starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Leader(NonZeroU32),
    Job,
}

/// One whole line `runner ...` or `browser ...` of a mark: the process number, the creation time in microseconds
/// since the Unix epoch as the process table records it, and the group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Line {
    pub pid: NonZeroU32,
    pub created: NonZeroU64,
    pub group: Group,
}

/// The runner's and the browser's lines of a mark, each where it was written whole; the browser's line is read only
/// after the runner's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lines {
    pub runner: Option<Line>,
    pub browser: Option<Line>,
}

/// The text of the first line of the mark of the run `run`.
pub fn first_line(run: &str) -> String {
    format!("mark run={run}\n")
}

/// A number of at least 1 in decimal digits with no leading zero.
fn number<T: FromStr>(s: &str) -> Option<T> {
    if s.is_empty() || s.starts_with('0') || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

fn read_line(word: &str, line: &str) -> Option<Line> {
    let mut parts = line.split(' ');
    if parts.next()? != word {
        return None;
    }
    let pid = number(parts.next()?.strip_prefix("pid=")?)?;
    let created = number(parts.next()?.strip_prefix("created=")?)?;
    let group = match parts.next()?.strip_prefix("group=")? {
        "job" => Group::Job,
        g => Group::Leader(number(g)?),
    };
    if parts.next().is_some() {
        return None;
    }
    Some(Line {
        pid,
        created,
        group,
    })
}

/// Reads the text of the mark whose file name is `run`: the first line must name that run, and a line that does
/// not end in a newline is not there yet. `None` when the first line is not whole or names another run.
pub fn read(run: &str, text: &str) -> Option<Lines> {
    let mut lines = text
        .split_inclusive('\n')
        .filter_map(|l| l.strip_suffix('\n'));
    if lines.next()? != first_line(run).trim_end_matches('\n') {
        return None;
    }
    let runner = lines.next().and_then(|l| read_line("runner", l));
    let browser = runner
        .and(lines.next())
        .and_then(|l| read_line("browser", l));
    Some(Lines { runner, browser })
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUN: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn a_line_that_is_not_whole_is_not_there() {
        let first = first_line(RUN);
        let cut = format!("{first}runner pid=812 created=17629");
        assert_eq!(read(RUN, &cut).unwrap().runner, None);
        assert_eq!(read(RUN, &first[..first.len() - 1]), None);
        assert_eq!(read("ffffffffffffffffffffffffffffffff", &first), None);
    }

    #[test]
    fn a_line_out_of_its_form_is_not_read_as_a_process() {
        let first = first_line(RUN);
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
            assert_eq!(read(RUN, &text).unwrap().runner, None, "{bad:?}");
        }
        let twice =
            format!("{first}runner pid=5 created=6 group=job\nrunner pid=5 created=6 group=job\n");
        assert_eq!(read(RUN, &twice).unwrap().browser, None);
    }
}
