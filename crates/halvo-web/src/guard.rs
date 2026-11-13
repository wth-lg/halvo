//! The guard of the relay's port (A11.1, the coordinator's rulings CP-148 (1) and (2), CP-158 (1)):
//! a process that holds a copy of the relay's listener until the browser's tree is gone, members
//! born later among them, and that ends the run when its executor is gone.

use std::time::Duration;

use crate::procs::{self, Proc};

/// The flag of the program that the guard runs:
/// `--relay-guard <pid> <creation time in µs> <executor pid> <executor creation time> <record>`.
pub const GUARD_FLAG: &str = "--relay-guard";

/// `GRACE_MS`: how long the guard waits, once its executor is gone, for the browser to end by
/// itself (CP-148 (1)).
pub const GRACE: Duration = Duration::from_millis(3_000);

/// `TERM_WAIT_MS`: how long the guard waits after it asked the run's processes to end before it
/// ends by force each one that still runs (CP-148 (1)).
pub const TERM_WAIT: Duration = Duration::from_millis(2_000);

/// How long the guard keeps the port after it read the tree gone: longer than one pass of the
/// trial's watch on the development Mac under load (CP-148 (3)).
pub const PORT_MARGIN: Duration = Duration::from_millis(1_000);

/// How long the guard keeps the port while its read of the process table fails before it ends
/// with exit 3 and `guard_end cause=table_unread` (CP-145, its addition of 07:54Z).
pub const UNREAD_CAP: Duration = Duration::from_secs(300);

/// How often the guard reads the process table, and how long it waits after a read that failed.
pub const POLL: Duration = Duration::from_millis(20);
pub const UNREAD_PAUSE: Duration = Duration::from_millis(2);

/// When the guard's loop ends the run's processes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    /// The guard of a run: once the executor, known by number and creation time together, no
    /// longer runs and `GRACE` has passed (CP-148 (1)); it keeps the port `PORT_MARGIN` after the
    /// tree is gone.
    ExecutorGone { pid: u32, started_us: u64 },
    /// The executor's own end of a start that gave no first reply (CP-158 (1)): at once, by the
    /// same ask and forced end, with no margin.
    Now,
}

/// What the loop needs from the system, given for tests: the process table (`None`: it could not
/// be read), the clock, the pause (told whether the last read was unread), the end of one process of
/// the run (`false`: ask it, `true`: by force; each reads the creation time again), and the record
/// of the act, called once.
pub struct GuardIo<'a> {
    pub read: &'a mut dyn FnMut() -> Option<Vec<Proc>>,
    pub now_ms: &'a mut dyn FnMut() -> u128,
    pub pause: &'a mut dyn FnMut(bool),
    pub end: &'a mut dyn FnMut(&Proc, bool) -> bool,
    pub record: &'a mut dyn FnMut(&GuardAct),
}

/// What the loop did once it acted, recorded once when it read the tree gone.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GuardAct {
    /// Milliseconds from the read of the trigger to the read of the tree gone.
    pub ms: u128,
    pub asked: usize,
    pub forced: usize,
    /// Whether the browser's first process was among the processes that it asked or forced.
    pub first: bool,
    /// The file names of the programs of the processes that it asked or forced.
    pub programs: Vec<String>,
}

impl GuardAct {
    /// The record's line: `ended_by=guard ms=.. asked=.. forced=.. first=yes|no programs=..`.
    pub fn line(&self) -> String {
        format!(
            "ended_by=guard ms={} asked={} forced={} first={} programs={}",
            self.ms,
            self.asked,
            self.forced,
            if self.first { "yes" } else { "no" },
            if self.programs.is_empty() {
                "-".to_string()
            } else {
                self.programs.join(",")
            }
        )
    }
}

/// The program that runs the guard: `program` with `prefix` before `GUARD_FLAG` and its
/// arguments. The product's is the executor's own binary with no prefix, whose `main` calls
/// `run_if_guard` first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuardProgram {
    pub program: std::path::PathBuf,
    pub prefix: Vec<std::ffi::OsString>,
}

impl GuardProgram {
    /// This process's own binary, with no prefix.
    pub fn this_program() -> std::io::Result<GuardProgram> {
        Ok(GuardProgram {
            program: std::env::current_exe()?,
            prefix: Vec::new(),
        })
    }
}

/// For a binary's `main`: when the first argument after the program is `GUARD_FLAG`, runs the
/// guard and gives its exit status; otherwise `None`.
pub fn run_if_guard(args: &[String]) -> Option<i32> {
    match args.get(1) {
        Some(flag) if flag == GUARD_FLAG => Some(guard_main(&args[2..])),
        _ => None,
    }
}

/// The guard's program: `argv` after `GUARD_FLAG`. It runs `guard_loop` with the system's table,
/// the ask and the forced end of `procs`, and writes the act into the record file. Exit 2 for
/// arguments that it cannot read.
pub fn guard_main(argv: &[String]) -> i32 {
    let num = |i: usize| argv.get(i).and_then(|v| v.parse::<u64>().ok());
    let (Some(pid), Some(started_us), Some(epid), Some(estart)) = (num(0), num(1), num(2), num(3))
    else {
        return 2;
    };
    let record = argv.get(4).map(std::path::PathBuf::from);
    let start = std::time::Instant::now();
    guard_loop(
        pid as u32,
        started_us,
        Trigger::ExecutorGone {
            pid: epid as u32,
            started_us: estart,
        },
        GuardIo {
            read: &mut || procs::table().ok(),
            now_ms: &mut || start.elapsed().as_millis(),
            pause: &mut |unread| std::thread::sleep(if unread { UNREAD_PAUSE } else { POLL }),
            end: &mut |p, force| {
                if force {
                    procs::kill(p)
                } else {
                    procs::ask_end(p)
                }
            },
            record: &mut |act| {
                if let Some(r) = &record {
                    let _ = std::fs::write(r, act.line() + "\n");
                }
            },
        },
    )
}

/// The processes of the run in `table`: `known` grows by every process of the tree of the
/// browser's first process (by creation time, the adopted processes that name its profile among
/// them), every child of a member that runs, every process of the group that the first process
/// leads that began at or after it, while `group_held` says that the last read held a process of
/// that group and no other process has the group's number (so the group never ended in between
/// and its number was not taken again), and every process whose parent is a member that no longer runs, when that
/// member's number names no running process that began at or before it. Gives the members that
/// run now, each by number and creation time.
pub fn members(
    known: &mut Vec<Proc>,
    table: &[Proc],
    root: &Proc,
    me: u32,
    group_held: bool,
) -> Vec<Proc> {
    let alive = |p: &Proc| table.iter().any(|t| procs::same(t, p));
    let living: Vec<Proc> = known.iter().filter(|k| alive(k)).cloned().collect();
    let group = known
        .iter()
        .find(|k| procs::same(k, root))
        .filter(|r| r.pgid == r.pid)
        .map_or(0, |r| r.pgid);
    let mut found: Vec<Proc> = Vec::new();
    if alive(root) {
        found.extend(crate::checks::tree_of(table, root));
    }
    found.extend(procs::descendants(table, &living));
    let taken = table
        .iter()
        .any(|t| t.pid == group && t.started_us != root.started_us);
    if group != 0 && group_held && !taken {
        found.extend(
            table
                .iter()
                .filter(|t| t.pgid == group && t.started_us >= root.started_us)
                .cloned(),
        );
    }
    for t in table {
        let orphan = known.iter().any(|k| {
            !alive(k)
                && t.ppid == k.pid
                && t.started_us != 0
                && t.started_us >= k.started_us
                && !table
                    .iter()
                    .any(|o| o.pid == k.pid && o.started_us <= t.started_us)
        });
        if orphan {
            found.push(t.clone());
        }
    }
    for p in found {
        if p.pid != me && !known.iter().any(|k| procs::same(k, &p)) {
            known.push(p);
        }
    }
    known.iter().filter(|k| alive(k)).cloned().collect()
}

/// The guard's loop. It keeps the port while a process of the run runs (`members`). A table that
/// could not be read is not an empty one: the loop keeps the port and reads again, and when the
/// table stays unread for `UNREAD_CAP` it ends with exit 3 and the line
/// `guard_end cause=table_unread`. With `Trigger::ExecutorGone` it ends nothing while the executor
/// runs, known by number and creation time together; once the executor is gone and a process of
/// the run still runs it waits `GRACE`, asks each process of the run that still runs to end, by
/// number and creation time in the table of that moment, ends by force each one that still runs
/// `TERM_WAIT` later and at every read after, and reads on until none runs; then it keeps the port
/// `PORT_MARGIN` more and exits 0. A number that another process took is no member. With
/// `Trigger::Now` it acts at once and returns 0 when none runs.
pub fn guard_loop(pid: u32, started_us: u64, trigger: Trigger, io: GuardIo<'_>) -> i32 {
    let GuardIo {
        read,
        now_ms,
        pause,
        end,
        record,
    } = io;
    let me = std::process::id();
    let mut root = Proc {
        pid,
        ppid: 0,
        started_us,
        pgid: 0,
        exe: None,
    };
    let mut known: Vec<Proc> = Vec::new();
    let mut unread_since: Option<u128> = None;
    let mut trigger_at: Option<u128> = None;
    let mut asked_at: Option<u128> = None;
    let (mut asked, mut forced) = (0usize, 0usize);
    let (mut acted, mut recorded) = (false, false);
    let mut act = GuardAct::default();
    let mut gone_at: Option<u128> = None;
    let mut group_held = false;
    let (grace, margin) = match trigger {
        Trigger::ExecutorGone { .. } => (GRACE.as_millis(), PORT_MARGIN.as_millis()),
        Trigger::Now => (0, 0),
    };
    loop {
        let Some(table) = read() else {
            let now = now_ms();
            let since = *unread_since.get_or_insert(now);
            if now.saturating_sub(since) >= UNREAD_CAP.as_millis() {
                eprintln!("guard_end cause=table_unread");
                return 3;
            }
            pause(true);
            continue;
        };
        unread_since = None;
        if let Some(r) = table.iter().find(|t| procs::same(t, &root)) {
            if !known.iter().any(|k| procs::same(k, r)) {
                root = r.clone();
                known.push(r.clone());
            }
        }
        let running = members(&mut known, &table, &root, me, group_held);
        group_held = root.pgid == root.pid
            && table
                .iter()
                .any(|t| t.pgid == root.pgid && t.started_us >= root.started_us);
        let now = now_ms();
        if running.is_empty() {
            if acted && !recorded {
                act.ms = now.saturating_sub(trigger_at.unwrap_or(now));
                act.asked = asked;
                act.forced = forced;
                record(&act);
                recorded = true;
            }
            let since = *gone_at.get_or_insert(now);
            if now.saturating_sub(since) >= margin {
                return 0;
            }
            pause(false);
            continue;
        }
        gone_at = None;
        let fire = match trigger {
            Trigger::ExecutorGone {
                pid: epid,
                started_us: estart,
            } => !table
                .iter()
                .any(|t| t.pid == epid && t.started_us == estart),
            Trigger::Now => true,
        };
        if fire {
            let since = *trigger_at.get_or_insert(now);
            if now.saturating_sub(since) >= grace {
                match asked_at {
                    None => {
                        for p in &running {
                            if end(p, false) {
                                asked += 1;
                            }
                            note(&mut act, p, &root);
                        }
                        acted = true;
                        asked_at = Some(now);
                    }
                    Some(at) if now.saturating_sub(at) >= TERM_WAIT.as_millis() => {
                        for p in &running {
                            if end(p, true) {
                                forced += 1;
                            }
                            note(&mut act, p, &root);
                        }
                    }
                    Some(_) => {}
                }
            }
        }
        pause(false);
    }
}

/// Notes in the record a process that the loop asked or forced: its program's file name, and
/// whether it is the browser's first process.
fn note(act: &mut GuardAct, p: &Proc, root: &Proc) {
    act.first |= procs::same(p, root);
    let name = p
        .exe
        .as_ref()
        .and_then(|e| e.file_name())
        .map_or("-".to_string(), |n| n.to_string_lossy().replace(' ', "_"));
    if !act.programs.contains(&name) {
        act.programs.push(name);
    }
}

/// Ends the tree of `root` by the guard's rule at once (`Trigger::Now`), with the system's table,
/// ask and forced end: the executor's end of a start that gave no first reply (CP-158 (1)).
pub fn end_tree_now(root: &Proc) -> GuardAct {
    let start = std::time::Instant::now();
    let mut done = GuardAct::default();
    guard_loop(
        root.pid,
        root.started_us,
        Trigger::Now,
        GuardIo {
            read: &mut || {
                if start.elapsed() > UNREAD_CAP {
                    return Some(Vec::new());
                }
                procs::table().ok()
            },
            now_ms: &mut || start.elapsed().as_millis(),
            pause: &mut |unread| std::thread::sleep(if unread { UNREAD_PAUSE } else { POLL }),
            end: &mut |p, force| {
                if force {
                    procs::kill(p)
                } else {
                    procs::ask_end(p)
                }
            },
            record: &mut |act| done = act.clone(),
        },
    );
    done
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    const ROOT: u32 = 999_999;

    type Ends = Vec<(u32, bool, u128)>;

    /// A loop run on stand-in tables: `table(t, ends)` gives the table at `t` ms (None: unread),
    /// the pause adds `step` ms, and each end of a process is recorded with its time, which later
    /// tables may read. Gives the exit, the ms at the exit, the ends and the record.
    fn run(
        trigger: Trigger,
        step: u128,
        table: &dyn Fn(u128, &Ends) -> Option<Vec<Proc>>,
    ) -> (i32, u128, Ends, Option<GuardAct>) {
        let clock = Cell::new(0u128);
        let ends = RefCell::new(Vec::new());
        let mut record = None;
        let mut reads = 0u64;
        let code = guard_loop(
            ROOT,
            7,
            trigger,
            GuardIo {
                read: &mut || {
                    reads += 1;
                    assert!(reads < 1_000_000, "the guard reads on without end");
                    table(clock.get(), &ends.borrow())
                },
                now_ms: &mut || clock.get(),
                pause: &mut |_| clock.set(clock.get() + step),
                end: &mut |p, force| {
                    ends.borrow_mut().push((p.pid, force, clock.get()));
                    true
                },
                record: &mut |act| record = Some(act.clone()),
            },
        );
        (code, clock.get(), ends.into_inner(), record)
    }

    fn row(pid: u32, ppid: u32, started_us: u64, pgid: u32) -> Proc {
        Proc {
            pid,
            ppid,
            started_us,
            pgid,
            exe: None,
        }
    }

    fn executor(started_us: u64) -> Trigger {
        Trigger::ExecutorGone { pid: 5, started_us }
    }

    fn forced(ends: &Ends, pid: u32) -> Option<u128> {
        ends.iter().find(|e| e.0 == pid && e.1).map(|e| e.2)
    }

    /// The coordinator's point on an unread table: a read of the table that fails once keeps the
    /// port, and the guard ends only when the browser's first process is gone.
    #[test]
    fn a_table_that_could_not_be_read_once_is_no_empty_table_for_the_guard() {
        let (code, _, ends, _) = run(executor(50), 20, &|t, _| match t {
            0 => None,
            1..=100 => Some(vec![row(ROOT, 1, 7, ROOT), row(5, 1, 50, 5)]),
            _ => Some(vec![row(5, 1, 50, 5)]),
        });
        assert_eq!(code, 0);
        assert!(ends.is_empty(), "{ends:?}");
    }

    /// The verifier's point (2) on G1: a table that stays unread for longer than the margin while
    /// the browser runs keeps the port, and the guard frees it only after the browser is gone and
    /// the margin has passed.
    #[test]
    fn a_table_unread_for_longer_than_the_margin_keeps_the_port_while_the_browser_runs() {
        let (code, at, ends, _) = run(executor(50), 20, &|t, _| match t {
            1_000..=2_500 => None,
            _ if t < 4_000 => Some(vec![row(ROOT, 1, 7, ROOT), row(5, 1, 50, 5)]),
            _ => Some(vec![row(5, 1, 50, 5)]),
        });
        assert_eq!(code, 0);
        assert!(ends.is_empty(), "{ends:?}");
        assert!(at >= 4_000 + 1_000, "the port freed at {at} ms");
    }

    /// A table that stays unread ends the guard at its cap with its own cause.
    #[test]
    fn a_table_that_stays_unread_ends_the_guard_at_the_cap() {
        let (code, at, _, _) = run(executor(50), 2, &|_, _| None);
        assert_eq!(code, 3);
        assert!(at >= UNREAD_CAP.as_millis(), "ended at {at} ms");
        assert_eq!(UNREAD_CAP, Duration::from_secs(300));
    }

    /// CP-148 (4): the executor gone and a browser that never ends by itself: after the grace the
    /// guard asks it to end, after the wait it ends it by force, and it frees the port only after
    /// the tree is gone and its margin has passed.
    #[test]
    fn a_browser_that_never_ends_is_ended_after_the_grace_and_the_port_freed_after_it() {
        let (code, at, ends, record) = run(executor(50), 10, &|t, ends| {
            let mut v = Vec::new();
            if forced(ends, ROOT).is_none() {
                v.push(row(ROOT, 1, 7, ROOT));
            }
            if t < 1_000 {
                v.push(row(5, 1, 50, 5));
            }
            Some(v)
        });
        assert_eq!(code, 0);
        let asked = ends.iter().find(|e| !e.1).expect("asked");
        let force = forced(&ends, ROOT).expect("forced");
        assert!(asked.2 >= 1_000 + 3_000, "asked at {} ms", asked.2);
        assert!(force >= asked.2 + 2_000, "forced at {force} ms");
        assert!(at >= force + 1_000, "the port freed at {at} ms");
        let act = record.expect("the guard recorded its act");
        assert!(
            act.ms >= 3_000 && act.asked == 1 && act.forced == 1,
            "{act:?}"
        );
        assert!(act.first);
        assert_eq!(
            (GRACE, TERM_WAIT, PORT_MARGIN),
            (
                Duration::from_millis(3_000),
                Duration::from_millis(2_000),
                Duration::from_millis(1_000)
            )
        );
    }

    /// CP-148 (4): a browser that ends by itself inside the grace is left to itself.
    #[test]
    fn a_browser_that_ends_inside_the_grace_is_left_to_itself() {
        let (code, _, ends, record) = run(executor(50), 10, &|t, _| {
            let mut v = Vec::new();
            if t < 1_000 + 2_500 {
                v.push(row(ROOT, 1, 7, ROOT));
            }
            if t < 1_000 {
                v.push(row(5, 1, 50, 5));
            }
            Some(v)
        });
        assert_eq!(code, 0);
        assert!(ends.is_empty(), "{ends:?}");
        assert!(record.is_none());
    }

    /// CP-148 (4): while the executor runs the guard ends nothing, however long the browser runs.
    #[test]
    fn the_guard_ends_nothing_while_its_executor_runs() {
        let (code, _, ends, _) = run(executor(50), 10, &|t, _| {
            let mut v = vec![row(5, 1, 50, 5)];
            if t < 60_000 {
                v.push(row(ROOT, 1, 7, ROOT));
            }
            Some(v)
        });
        assert_eq!(code, 0);
        assert!(ends.is_empty(), "{ends:?}");
    }

    /// CP-148 (2) and (4): a number of the tree that another process took is left alone.
    #[test]
    fn a_number_of_the_tree_that_another_process_took_is_left_alone() {
        let (code, _, ends, _) = run(executor(50), 10, &|t, _| {
            let mut v = Vec::new();
            if t < 2_000 {
                v.push(row(ROOT, 1, 7, ROOT));
            } else {
                v.push(row(ROOT, 1, 8, ROOT));
            }
            if t < 1_000 {
                v.push(row(5, 1, 50, 5));
            }
            Some(v)
        });
        assert_eq!(code, 0);
        assert!(ends.is_empty(), "{ends:?}");
    }

    /// The verifier's V7 and S11, the review's G1, finding (2): the executor is known by its number
    /// and its creation time together; a process that took its number is not the executor, and the
    /// guard acts after its grace.
    #[test]
    fn a_process_that_took_the_executor_s_number_is_not_the_executor() {
        let (code, _, ends, _) = run(executor(50), 10, &|_, ends| {
            let mut v = vec![row(5, 1, 51, 5)];
            if forced(ends, ROOT).is_none() {
                v.push(row(ROOT, 1, 7, ROOT));
            }
            Some(v)
        });
        assert_eq!(code, 0);
        let asked = ends.iter().find(|e| !e.1).expect("the guard acted");
        assert!(asked.2 >= 3_000, "asked at {} ms", asked.2);
        assert!(ends.iter().all(|e| e.0 == ROOT), "{ends:?}");
    }

    /// The verifier's V2 and S6, M48's first half: a process that a living member starts after the
    /// browser's first process has ended is followed and ended: a grandchild born at 4,000 ms is
    /// ended by force at 5,000 ms, and the port is kept until it is gone.
    #[test]
    fn a_member_born_after_the_first_process_ended_is_followed_and_ended() {
        for group in [ROOT, 0] {
            let (code, at, ends, _) = run(executor(50), 10, &|t, ends| {
                let mut v = Vec::new();
                if t < 500 {
                    v.push(row(ROOT, 1, 7, group));
                }
                if forced(ends, 20).is_none() {
                    v.push(row(20, ROOT, 8, group));
                }
                if t >= 4_000 && forced(ends, 30).is_none() {
                    v.push(row(30, 20, 9, group));
                }
                Some(v)
            });
            assert_eq!(code, 0);
            let late = forced(&ends, 30).expect("the late grandchild was ended");
            assert!((5_000..5_100).contains(&late), "forced at {late} ms");
            assert_eq!(
                Some(late),
                forced(&ends, 20),
                "not in the pass of its parent"
            );
            assert!(at >= late + 1_000, "the port freed at {at} ms");
        }
    }

    /// The verifier's V5, M48's second half: the port is kept while any member runs, also when the
    /// first process is gone.
    #[test]
    fn the_port_is_kept_while_a_member_runs_after_the_first_process_is_gone() {
        let (code, at, _, _) = run(executor(50), 10, &|t, _| {
            let mut v = vec![row(5, 1, 50, 5)];
            if t < 500 {
                v.push(row(ROOT, 1, 7, ROOT));
            }
            if t < 8_000 {
                v.push(row(20, ROOT, 8, ROOT));
            }
            Some(v)
        });
        assert_eq!(code, 0);
        assert!(at >= 8_000 + 1_000, "the port freed at {at} ms");
    }

    /// The review's point 1, finding (1): a process that is born and loses its parent between two
    /// reads of the table, after the browser's first process has ended, is followed: by the
    /// browser's own process group where the system keeps one (macOS, its parent then 1), and by
    /// its parent's number where the system keeps that (Windows).
    #[test]
    fn an_orphan_born_between_two_reads_is_followed_and_the_port_kept_until_it_is_gone() {
        for (ppid, pgid) in [(1, ROOT), (20, 0)] {
            let (code, at, ends, _) = run(executor(50), 10, &|t, ends| {
                let mut v = Vec::new();
                if t < 500 {
                    v.push(row(ROOT, 1, 7, ROOT));
                }
                if t < 1_000 {
                    v.push(row(20, ROOT, 8, ROOT));
                }
                if t >= 1_000 && forced(ends, 30).is_none() {
                    v.push(row(30, ppid, 9, pgid));
                }
                Some(v)
            });
            assert_eq!(code, 0);
            let end = forced(&ends, 30).expect("the orphan was ended");
            assert!(at >= end + 1_000, "the port freed at {at} ms ({ppid})");
        }
    }

    /// An orphan's rule takes no process of a number that another process took before it began,
    /// and no process of another group.
    #[test]
    fn a_child_of_a_number_that_another_process_took_is_no_member() {
        let root = row(ROOT, 1, 7, ROOT);
        let mut known = vec![root.clone(), row(20, ROOT, 8, ROOT)];
        let table = vec![
            row(20, 1, 9, 20),
            row(30, 20, 10, 20),
            row(31, 1, 10, 77),
            row(5, 1, 50, 5),
        ];
        assert!(members(&mut known, &table, &root, 4, false).is_empty());
        let mut known = vec![root.clone(), row(20, ROOT, 8, ROOT)];
        let table = vec![row(30, 20, 10, 0), row(5, 1, 50, 5)];
        assert_eq!(members(&mut known, &table, &root, 4, false).len(), 1);
        let mut known = vec![root.clone()];
        let table = vec![row(32, 1, 10, ROOT), row(33, 1, 6, ROOT)];
        assert_eq!(members(&mut known, &table, &root, 4, true).len(), 1);
        let mut known = vec![root.clone()];
        assert!(members(&mut known, &table, &root, 4, false).is_empty());
    }

    /// CP-158 (1): `Trigger::Now` asks at once, ends by force after `TERM_WAIT`, and returns when
    /// none runs, with no margin.
    #[test]
    fn the_end_of_a_start_with_no_reply_asks_at_once_and_forces_after_the_wait() {
        let (code, at, ends, record) = run(Trigger::Now, 10, &|_, ends| {
            let mut v = Vec::new();
            if forced(ends, ROOT).is_none() {
                v.push(row(ROOT, 1, 7, ROOT));
            }
            Some(v)
        });
        assert_eq!(code, 0);
        assert_eq!(ends.first().map(|e| (e.1, e.2)), Some((false, 0)));
        let force = forced(&ends, ROOT).expect("forced");
        assert!((2_000..2_100).contains(&force), "{force}");
        assert!(at < force + 100, "{at}");
        assert!(record.is_some());
    }

    /// The fence of the coordinator's ruling CP-161 F1, for the one test below that drives the
    /// loop with the real table and the real ask: a call passes only for the stand-in browser
    /// that the test started, by number and creation time; every other call sends nothing, is
    /// recorded, and fails the test at that call.
    struct Fence {
        admit: (u32, u64),
        refused: RefCell<Vec<(u32, u64, bool)>>,
    }

    impl Fence {
        fn pass(&self, p: &Proc, force: bool, send: &dyn Fn(&Proc, bool) -> bool) -> bool {
            if (p.pid, p.started_us) == self.admit {
                return send(p, force);
            }
            self.refused.borrow_mut().push((p.pid, p.started_us, force));
            panic!(
                "the guard handed the fence a process that the test did not start: {:?}",
                self.refused.borrow()
            );
        }
    }

    #[test]
    fn the_fence_passes_the_stand_in_alone_and_records_every_other_call() {
        let fence = Fence {
            admit: (40, 400),
            refused: RefCell::new(Vec::new()),
        };
        let asked = RefCell::new(Vec::new());
        let send = |p: &Proc, force: bool| {
            asked.borrow_mut().push((p.pid, force));
            true
        };
        assert!(fence.pass(&row(40, 1, 400, 0), false, &send));
        for other in [row(41, 1, 400, 0), row(40, 1, 401, 0), row(41, 1, 401, 0)] {
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                fence.pass(&other, true, &send)
            }));
            assert!(r.is_err());
        }
        assert_eq!(*asked.borrow(), vec![(40, false)]);
        assert_eq!(fence.refused.borrow().len(), 3);
    }

    /// CP-161 F1 (2): the loop on a table that the test builds, with the stand-in's tree and three
    /// processes of no tree, through the fence in front of an ask that records: the ask's record
    /// holds the stand-in alone.
    #[test]
    fn the_loop_hands_the_fence_its_tree_alone_on_a_built_table() {
        let fence = Fence {
            admit: (ROOT, 7),
            refused: RefCell::new(Vec::new()),
        };
        let asked = RefCell::new(Vec::new());
        let send = |p: &Proc, force: bool| {
            asked.borrow_mut().push((p.pid, force));
            true
        };
        let clock = Cell::new(0u128);
        let code = guard_loop(
            ROOT,
            7,
            executor(50),
            GuardIo {
                read: &mut || {
                    let mut v = vec![row(61, 1, 3, 61), row(62, 61, 4, 61), row(63, 1, 9, 63)];
                    if !asked.borrow().iter().any(|a| a.1) {
                        v.push(row(ROOT, 1, 7, ROOT));
                    }
                    Some(v)
                },
                now_ms: &mut || clock.get(),
                pause: &mut |_| clock.set(clock.get() + 10),
                end: &mut |p, force| fence.pass(p, force, &send),
                record: &mut |_| {},
            },
        );
        assert_eq!(code, 0);
        assert_eq!(*asked.borrow(), vec![(ROOT, false), (ROOT, true)]);
    }

    /// Ends the test's own stand-ins on every way out, also when the test fails.
    #[cfg(target_os = "macos")]
    struct Own(Vec<std::process::Child>);

    #[cfg(target_os = "macos")]
    impl Drop for Own {
        fn drop(&mut self) {
            for c in &mut self.0 {
                let _ = c.kill();
                let _ = c.wait();
            }
        }
    }

    /// CP-148 on real processes (macOS), fenced (CP-161 F1, finding (4)): a stand-in browser that
    /// never ends by itself and a stand-in executor; the executor is killed; the guard asks the
    /// browser to end after its grace, and returns only after the browser is gone. Its ask and
    /// forced end pass the fence alone, and the test has a bound of 20 s inside its read and pause.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_guard_ends_a_real_stand_in_after_its_executor_is_killed() {
        use crate::start::Start;
        let mut own = Own(Vec::new());
        own.0.push(
            std::process::Command::new("/bin/sleep")
                .arg("60")
                .start()
                .unwrap(),
        );
        own.0.push(
            std::process::Command::new("/bin/sleep")
                .arg("60")
                .start()
                .unwrap(),
        );
        let (bpid, epid) = (own.0[0].id(), own.0[1].id());
        std::thread::sleep(Duration::from_millis(300));
        let table = procs::table().unwrap();
        let b = table.iter().find(|p| p.pid == bpid).unwrap().clone();
        let e = table.iter().find(|p| p.pid == epid).unwrap().clone();
        let fence = Fence {
            admit: (b.pid, b.started_us),
            refused: RefCell::new(Vec::new()),
        };
        let start = std::time::Instant::now();
        let bound = Duration::from_secs(20);
        let mut record = None;
        let own = RefCell::new(own);
        let code = guard_loop(
            b.pid,
            b.started_us,
            Trigger::ExecutorGone {
                pid: e.pid,
                started_us: e.started_us,
            },
            GuardIo {
                read: &mut || {
                    assert!(
                        start.elapsed() < bound,
                        "the guard ran past the test's bound"
                    );
                    let mut own = own.borrow_mut();
                    if start.elapsed() > Duration::from_millis(500) {
                        let _ = own.0[1].kill();
                        let _ = own.0[1].wait();
                    }
                    let _ = own.0[0].try_wait();
                    procs::table().ok()
                },
                now_ms: &mut || start.elapsed().as_millis(),
                pause: &mut |_| {
                    assert!(
                        start.elapsed() < bound,
                        "the guard ran past the test's bound"
                    );
                    std::thread::sleep(POLL)
                },
                end: &mut |p, force| {
                    fence.pass(p, force, &|p, force| {
                        if force {
                            procs::kill(p)
                        } else {
                            procs::ask_end(p)
                        }
                    })
                },
                record: &mut |act| record = Some(act.clone()),
            },
        );
        let took = start.elapsed();
        let status = own.borrow_mut().0[0].wait().unwrap();
        assert_eq!(code, 0);
        assert!(took >= Duration::from_millis(500) + GRACE, "{took:?}");
        assert!(!procs::running(&b), "the stand-in browser still runs");
        assert!(!status.success(), "{status:?}");
        let act = record.expect("the guard recorded its act");
        assert!(act.first, "{act:?}");
        assert_eq!(act.programs, ["sleep"]);
        assert!(fence.refused.borrow().is_empty());
    }
}
