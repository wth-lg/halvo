//! What the rows of `web-rows` read about a run's processes (CP-170 E8).

use crate::checks::Mark;
use crate::procs::Proc;

/// One read of what a run has running: its own processes by the run's mark, its guards, and
/// whether the read stays open, with its cause.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Left {
    pub own: usize,
    pub guards: usize,
    pub open: Option<&'static str>,
}

impl Left {
    /// Whether the read shows the run gone: none of its processes, none of its guards, and nothing
    /// open.
    pub fn gone(&self) -> bool {
        self.own == 0 && self.guards == 0 && self.open.is_none()
    }
}

/// Reads what the run of `mark` has running in `table`: its own processes (`Mark::owns` with
/// `args`, a guard not among them), and its guards (`is_guard`). A table that could not be read is open with the cause
/// `table_unread`; a process whose program lies inside the browser's bundle, which began at or
/// after the mark and whose arguments cannot be read, is open with the cause `args_unread`, because
/// it may be the run's own. Whether a program lies inside the bundle is read by the function that
/// `Mark::owns` uses (`checks::inside`), so that both read one path alike (CP-186 S7).
pub fn read_left(
    table: Option<&[Proc]>,
    mark: &Mark,
    args: &dyn Fn(u32) -> Option<Vec<String>>,
    is_guard: &dyn Fn(&Proc) -> bool,
) -> Left {
    let Some(table) = table else {
        return Left {
            own: 0,
            guards: 0,
            open: Some("table_unread"),
        };
    };
    let own = table
        .iter()
        .filter(|p| !is_guard(p) && mark.owns(p, args))
        .count();
    let guards = table.iter().filter(|p| is_guard(p)).count();
    let unread = table.iter().any(|p| {
        p.started_us >= mark.since_us
            && p.pid != std::process::id()
            && p.exe
                .as_deref()
                .is_some_and(|e| crate::checks::inside(e, &mark.root))
            && args(p.pid).is_none()
    });
    Left {
        own,
        guards,
        open: unread.then_some("args_unread"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// CP-186 S7: a process whose program lies inside the bundle by the path test of
    /// `Mark::owns`, with its arguments unread, is open; one whose path only begins like the
    /// bundle's (`/B.apps`) is not.
    #[test]
    fn s7_read_left_reads_inside_as_mark_owns_does() {
        let proc = |exe: &str| Proc {
            pid: 4_000_000_041,
            ppid: 1,
            started_us: 20,
            pgid: 0,
            exe: Some(exe.into()),
        };
        let table = vec![proc("/B.app/Contents/MacOS/b")];
        let left = read_left(Some(&table), &mark(), &|_| None, &|_| false);
        assert_eq!(left.open, Some("args_unread"));
        let table = vec![proc("/B.apps/b")];
        let left = read_left(Some(&table), &mark(), &|_| None, &|_| false);
        assert_eq!(left.open, None);
    }

    /// CP-186 S7, the review's case relay/05 (Windows): a bundle process whose path differs in
    /// letter case and slashes, with its arguments unread, is open, as `Mark::owns` reads it.
    #[cfg(windows)]
    #[test]
    fn s7_a_bundle_process_in_another_letter_case_is_open_on_windows() {
        let mark = Mark {
            profile: r"C:\p".into(),
            since_us: 10,
            root: PathBuf::from(r"C:\Apps\Browser"),
        };
        let table = vec![Proc {
            pid: 4_000_000_040,
            ppid: 1,
            started_us: 20,
            pgid: 0,
            exe: Some(r"c:/apps/browser/b.exe".into()),
        }];
        let left = read_left(Some(&table), &mark, &|_| None, &|_| false);
        assert_eq!(left.open, Some("args_unread"));
    }

    fn mark() -> Mark {
        Mark {
            profile: "/p".into(),
            since_us: 10,
            root: PathBuf::from("/B.app"),
        }
    }

    fn row(pid: u32, started_us: u64, exe: &str) -> Proc {
        Proc {
            pid,
            ppid: 1,
            started_us,
            pgid: 0,
            exe: Some(PathBuf::from(exe)),
        }
    }

    fn args(pid: u32) -> Option<Vec<String>> {
        match pid {
            20 | 40 => Some(vec!["--user-data-dir=/p".into()]),
            21 => Some(vec!["--user-data-dir=/q".into()]),
            _ => None,
        }
    }

    /// CP-170 E8 (b): an unread table is open, never gone.
    #[test]
    fn an_unread_table_is_open() {
        let left = read_left(None, &mark(), &args, &|_| false);
        assert_eq!(left.open, Some("table_unread"));
        assert!(!left.gone());
    }

    /// CP-170 E8 (b): a process inside the bundle, begun after the mark, whose arguments cannot be
    /// read, is open; one of another program or begun before the mark is not.
    #[test]
    fn a_bundle_process_with_unread_arguments_is_open() {
        let table = vec![row(30, 11, "/B.app/Contents/MacOS/helper")];
        let left = read_left(Some(&table), &mark(), &args, &|_| false);
        assert_eq!(left.open, Some("args_unread"));
        let table = vec![
            row(31, 11, "/other/x"),
            row(32, 5, "/B.app/Contents/MacOS/old"),
        ];
        assert!(read_left(Some(&table), &mark(), &args, &|_| false).gone());
    }

    #[test]
    fn the_run_s_own_and_its_guards_are_counted() {
        let table = vec![
            row(20, 11, "/B.app/Contents/MacOS/b"),
            row(21, 11, "/B.app/Contents/MacOS/b"),
            row(40, 11, "/B.app/Contents/MacOS/guard"),
        ];
        let left = read_left(Some(&table), &mark(), &args, &|p| p.pid == 40);
        assert_eq!(
            left,
            Left {
                own: 1,
                guards: 1,
                open: None
            }
        );
    }
}
