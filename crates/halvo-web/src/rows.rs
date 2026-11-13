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
/// it may be the run's own.
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
            && p.exe.as_deref().is_some_and(|e| e.starts_with(&mark.root))
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
