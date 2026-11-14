//! The guard of the relay's port (A11.1, the coordinator's rulings CP-148 (1) and (2), CP-158 (1)):
//! a process that holds a copy of the relay's listener until the browser's tree is gone, members
//! born later among them, and that ends the run when its executor is gone.

use std::time::Duration;

use crate::procs::{self, Proc};

/// The flag of the program that the guard runs:
/// `--relay-guard <pid> <creation time in µs> <executor pid> <executor creation time> <record>
/// <profile> <program>`: the browser's first process and the executor, each by number and creation
/// time, the file of the guard's record, and the run's profile directory and the browser's program,
/// from which the guard builds the run's mark (CP-126 E1 (b), CP-170 E1) before its first read.
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

/// How long the guard keeps the port for members that do not bear the run's mark, counted from
/// the last read that showed a running member with the mark (from the guard's first read with a
/// member when none ever bore it), before it writes its last line with the cause `left_unmarked`
/// and exits with 4 (CP-185 (3), CP-189 G6). The cap of the unread table, as CP-185 names it by
/// default. A read that fails neither starts, moves nor tests the count, and after an unread
/// spell the guard leaves by the cap only when two reads in a row agree.
pub const LEFT_CAP: Duration = UNREAD_CAP;

/// The bound of the executor's end of a start with no first reply (`Trigger::Now`) for a member
/// whose program or arguments stay unread after the forced end: the loop then returns with the
/// cause `now_unread` (CP-189 G5).
pub const NOW_UNREAD_BOUND: Duration = Duration::from_secs(10);

/// The bound of the same end for a member that bears the mark and does not end under the forced
/// end: the loop then returns with the cause `now_marked` (CP-189 G5).
pub const NOW_MARKED_BOUND: Duration = Duration::from_secs(15);

/// How often the guard reads the process table, and how long it waits after a read that failed.
pub const POLL: Duration = Duration::from_millis(20);
pub const UNREAD_PAUSE: Duration = Duration::from_millis(2);

/// When the guard's loop ends the run's processes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    /// The guard of a run: once the executor, known by number and creation time together, no
    /// longer runs and `GRACE` has passed (CP-148 (1)); it keeps the port `PORT_MARGIN` after the
    /// tree is gone. On Windows a row with the executor's number, no creation time and the guard's
    /// own file name counts as the executor (CP-176 A4 (a), `GuardIo::own_name`). The limit of
    /// that rule (A4 (e)): after a hard end of the executor, a process that the guard cannot open,
    /// of the same program name, that takes its number holds the guard, and the browser with it,
    /// for as long as it runs. The browser has no route then, since the relay ended with the
    /// executor. The ruling names the run's job of A6.6 and a reap at the next start as what ends
    /// it; neither is in this crate yet.
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
    /// The arguments of a process by its number (`checks::command_args` in the product), by which
    /// the loop tests a process against the run's mark.
    pub args: &'a dyn Fn(u32) -> Option<Vec<String>>,
    /// The run's mark, which the executor gives the guard at its start; `None` takes no process by
    /// the dead parent's number.
    pub mark: Option<&'a crate::checks::Mark>,
    /// The file name of the guard's own program, which is the executor's (CP-176 A4 (a)): given
    /// on Windows, where a row of a process that the system cannot open has no creation time, so
    /// that a row with the executor's number, no creation time and this file name (or none) is a
    /// read that finds the executor. `None` on macOS, which has no such row, and for
    /// `Trigger::Now`.
    pub own_name: Option<&'a std::ffi::OsStr>,
    pub now_ms: &'a mut dyn FnMut() -> u128,
    pub pause: &'a mut dyn FnMut(bool),
    pub end: &'a mut dyn FnMut(&Proc, bool) -> bool,
    pub record: &'a mut dyn FnMut(&GuardAct),
}

/// The guard's record (CP-189 G4): what it sent, what it left and what runs, written when that
/// changes and at every exit.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GuardAct {
    /// Milliseconds from the read of the trigger to this write; 0 before a trigger.
    pub ms: u128,
    /// The asks that were sent: one call of the end without force for each process of the run at
    /// the moment of the ask, counted when the call sent the signal.
    pub asked: usize,
    /// The forced ends that were sent, counted per call: from `TERM_WAIT` on, every read calls the
    /// forced end for each process of the run that still runs, so one process can count more than
    /// once.
    pub forced: usize,
    /// Whether the browser's first process was among the processes that it asked or forced.
    pub first: bool,
    /// The file names of the programs of the processes that it asked or forced.
    pub programs: Vec<String>,
    /// The reads, up to the act's record, in which the executor's row had its number and no
    /// creation time (CP-176 A4 (c)).
    pub untimed: usize,
    /// The members left so far because they did not bear the run's mark at the read of an ask or
    /// a forced end, each once, less those that the guard later asked or ended (CP-185 (1), (4),
    /// CP-189 G4).
    pub left: usize,
    /// The file names of the programs of those members.
    pub left_programs: Vec<String>,
    /// Why the record was written (CP-189 G4): `act` at the ask; `forced` at the read in which a
    /// member is first ended by force; `left` at the read in which a member is first left at the
    /// forced end; `left_gone` when a left member is gone; `change` when what runs changed;
    /// `tree_gone` once no member runs; `left_unmarked` at `LEFT_CAP`; `table_unread` at exit 3;
    /// `untimed` at the end of a guard that never wrote but counted untimed reads (CP-186 S4);
    /// `now_left`, `now_unread` and `now_marked` at the end of a start with no first reply (G5).
    pub cause: &'static str,
    /// The file names of the programs of the members that run at this write; `None` when the
    /// last read of the table failed (CP-189 G4).
    pub running: Option<Vec<String>>,
    /// How many members run at this write.
    pub running_count: usize,
    /// The guard's clock at this write, in ms (CP-189 G4).
    pub at_ms: u128,
}

impl GuardAct {
    /// The record's line (CP-189 G4): `ended_by=guard|none ms=.. asked=.. forced=..
    /// first=yes|no programs=.. executor_untimed=.. left=.. left_programs=.. running=..|unread
    /// running_programs=.. at_ms=.. cause=..`. Its first word says `ended_by=guard` only when the
    /// guard sent an ask or a forced end, at every write.
    pub fn line(&self) -> String {
        let names = |v: &[String]| {
            if v.is_empty() {
                "-".to_string()
            } else {
                v.join(",")
            }
        };
        format!(
            "ended_by={} ms={} asked={} forced={} first={} programs={} executor_untimed={} \
             left={} left_programs={} running={} running_programs={} at_ms={} cause={}",
            if self.asked + self.forced > 0 {
                "guard"
            } else {
                "none"
            },
            self.ms,
            self.asked,
            self.forced,
            if self.first { "yes" } else { "no" },
            names(&self.programs),
            self.untimed,
            self.left,
            names(&self.left_programs),
            match &self.running {
                Some(_) => self.running_count.to_string(),
                None => "unread".to_string(),
            },
            names(self.running.as_deref().unwrap_or_default()),
            self.at_ms,
            if self.cause.is_empty() {
                "-"
            } else {
                self.cause
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

/// What the guard's program reads from its arguments: the browser's first process and the
/// executor, each by number and creation time, the file of the record, and the run's mark. The
/// mark is no `Option`: a guard cannot run without the run's profile and the browser's program
/// (CP-185 (2), CP-189 G7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GuardArgs {
    pub pid: u32,
    pub started_us: u64,
    pub epid: u32,
    pub estart: u64,
    pub record: Option<std::path::PathBuf>,
    pub mark: crate::checks::Mark,
}

/// The guard's arguments after `GUARD_FLAG`, read with no effect: `None` when a number cannot be
/// read, when the executor's creation time is 0, which names no executor (CP-176 A4 (b)), and
/// when the run's profile (argument 5) or the browser's program (argument 6) is missing or empty
/// (CP-185 (2)). The mark is built from the profile, the program and the first process's
/// creation time.
pub(crate) fn parse_args(argv: &[String]) -> Option<GuardArgs> {
    let num = |i: usize| argv.get(i).and_then(|v| v.parse::<u64>().ok());
    let (pid, started_us, epid, estart) = (num(0)?, num(1)?, num(2)?, num(3)?);
    if estart == 0 {
        return None;
    }
    let given = |i: usize| argv.get(i).filter(|v| !v.is_empty());
    let (profile, program) = (given(5)?, given(6)?);
    Some(GuardArgs {
        pid: u32::try_from(pid).ok()?,
        started_us,
        epid: u32::try_from(epid).ok()?,
        estart,
        record: argv.get(4).map(std::path::PathBuf::from),
        mark: crate::checks::Mark::new(
            std::path::Path::new(profile),
            std::path::Path::new(program),
            started_us,
        ),
    })
}

/// The guard's program: `argv` after `GUARD_FLAG`. It runs `guard_loop` with the system's table,
/// the ask and the forced end of `procs`, and writes the act into the record file. Exit 2 for
/// arguments that it cannot read, for an executor creation time of 0, which names no executor
/// (CP-176 A4 (b)), and for a start without the run's profile or the browser's program, from which
/// it builds the mark that every ask and forced end needs (CP-185 (2)). Exit 4 after `LEFT_CAP`
/// with members that do not bear the mark.
pub fn guard_main(argv: &[String]) -> i32 {
    let Some(GuardArgs {
        pid,
        started_us,
        epid,
        estart,
        record,
        mark,
    }) = parse_args(argv)
    else {
        return 2;
    };
    let own = std::env::current_exe().ok();
    let own_name = if cfg!(windows) {
        own.as_deref().and_then(|p| p.file_name())
    } else {
        None
    };
    let start = std::time::Instant::now();
    guard_loop(
        pid,
        started_us,
        Trigger::ExecutorGone {
            pid: epid,
            started_us: estart,
        },
        GuardIo {
            read: &mut || procs::table().ok(),
            args: &crate::checks::command_args,
            mark: Some(&mark),
            own_name,
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
                    write_record(r, act);
                }
            },
        },
    )
}

/// Writes the record's line whole (CP-189 G4): into a file beside `path`, then renamed over it,
/// so that a reader finds the last line or the one before it, never a part.
pub(crate) fn write_record(path: &std::path::Path, act: &GuardAct) {
    let mut beside = path.as_os_str().to_owned();
    beside.push(".writing");
    let beside = std::path::PathBuf::from(beside);
    if std::fs::write(&beside, act.line() + "\n").is_ok() {
        let _ = std::fs::rename(&beside, path);
    }
}

/// The processes of the run in `table`: `known` grows by every process of the tree of the
/// browser's first process (by creation time, the adopted processes that name its profile among
/// them), every child of a member that runs, every process of the group that the first process
/// leads that began at or after it, while `group_held` says that the last read held a process of
/// that group and no other process has the group's number (so the group never ended in between
/// and its number was not taken again), and every process that bears the run's `mark`
/// (CP-126 E1 (b), `checks::Mark::owns` with `args`) and whose parent is a member that no longer
/// runs, when it began at or after that member and that member's number names no running process
/// that began at or before it (CP-170 E1); with no mark yet that last clause takes nothing. The
/// guard's own number `me` is never a member. Gives the members that run now, each by number and
/// creation time.
pub fn members(
    known: &mut Vec<Proc>,
    table: &[Proc],
    root: &Proc,
    me: u32,
    group_held: bool,
    mark: Option<&crate::checks::Mark>,
    args: &dyn Fn(u32) -> Option<Vec<String>>,
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
        found.extend(crate::checks::tree_of_with(table, root, args));
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
    for t in table
        .iter()
        .filter(|t| mark.is_some_and(|m| m.owns(t, args)))
    {
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
/// runs, known by number and creation time together, and a read that finds the executor clears
/// the time of its going and of the ask (CP-170 E2); once an unbroken run of reads misses the
/// executor and a process of the run still runs it waits `GRACE` from the first of them, asks each process of the run that still runs to end, by
/// number and creation time in the table of that moment, ends by force each one that still runs
/// `TERM_WAIT` later and at every read after, and reads on until none runs; then it keeps the port
/// `PORT_MARGIN` more and exits 0. A number that another process took is no member. With
/// `Trigger::Now` it acts at once and returns 0 when none runs.
pub fn guard_loop(pid: u32, started_us: u64, trigger: Trigger, io: GuardIo<'_>) -> i32 {
    let GuardIo {
        read,
        args,
        mark,
        own_name,
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
    let mut act = GuardAct::default();
    // CP-189 G4: the record, written when what it says changes and at every exit.
    let mut book = Book::default();
    let mut gone_at: Option<u128> = None;
    let mut group_held = false;
    // CP-185 (1): the run's mark, read for a process at the read of each ask and forced end; a
    // process that does not bear it, or whose program or arguments cannot be read, is left.
    let bears = |p: &Proc| mark.is_some_and(|m| m.owns(p, args));
    // CP-189 G6: the cap's count starts at the last read that showed a marked member, or at the
    // first read with a member; `after_unread` holds the cap back at the first read after a spell.
    let mut last_marked_at: Option<u128> = None;
    let mut first_member_at: Option<u128> = None;
    let mut after_unread = false;
    // CP-189 G5: the read of the first forced pass of `Trigger::Now`.
    let mut forced_at: Option<u128> = None;
    let (grace, margin) = match trigger {
        Trigger::ExecutorGone { .. } => (GRACE.as_millis(), PORT_MARGIN.as_millis()),
        Trigger::Now => (0, 0),
    };
    loop {
        let Some(table) = read() else {
            let now = now_ms();
            let since = *unread_since.get_or_insert(now);
            if now.saturating_sub(since) >= UNREAD_CAP.as_millis() {
                if book.written || act.untimed > 0 {
                    book.write(&mut act, record, "table_unread", now, trigger_at, None);
                }
                eprintln!("guard_end cause=table_unread");
                return 3;
            }
            after_unread = true;
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
        let running = members(&mut known, &table, &root, me, group_held, mark, args);
        group_held = root.pgid == root.pid
            && table
                .iter()
                .any(|t| t.pgid == root.pgid && t.started_us >= root.started_us);
        let now = now_ms();
        if running.is_empty() {
            if book.written && book.changed(&act, Some(&running)) {
                book.write(
                    &mut act,
                    record,
                    "tree_gone",
                    now,
                    trigger_at,
                    Some(&running),
                );
            }
            let since = *gone_at.get_or_insert(now);
            if now.saturating_sub(since) >= margin {
                if book.written {
                    book.write(
                        &mut act,
                        record,
                        "tree_gone",
                        now,
                        trigger_at,
                        Some(&running),
                    );
                } else if act.untimed > 0 {
                    book.write(&mut act, record, "untimed", now, trigger_at, Some(&running));
                }
                return 0;
            }
            pause(false);
            continue;
        }
        gone_at = None;
        // CP-185 (3): every member keeps the port while one that bears the mark runs; members
        // without it keep it for `LEFT_CAP` after the last marked one is gone, then the guard
        // writes its last line and exits, which closes its copy of the listener.
        first_member_at.get_or_insert(now);
        let spell_just_ended = std::mem::take(&mut after_unread);
        // CP-189 G5: the executor's end of a start with no first reply returns at the first read
        // at or after its forced end at which no member bears the mark and no member's program or
        // arguments are unread; a member that stays unread, or a marked member that does not end,
        // ends the wait at a bound of its own.
        if let (Trigger::Now, Some(at)) = (trigger, forced_at) {
            let unread = running
                .iter()
                .any(|p| p.exe.is_none() || args(p.pid).is_none());
            let marked = running.iter().any(&bears);
            let cause = if !marked && !unread {
                Some("now_left")
            } else if marked && now.saturating_sub(at) >= NOW_MARKED_BOUND.as_millis() {
                Some("now_marked")
            } else if unread && now.saturating_sub(at) >= NOW_UNREAD_BOUND.as_millis() {
                Some("now_unread")
            } else {
                None
            };
            if let Some(cause) = cause {
                for p in running.iter().filter(|p| !bears(p)) {
                    book.leave(p);
                }
                book.write(&mut act, record, cause, now, trigger_at, Some(&running));
                return if cause == "now_left" { 0 } else { 5 };
            }
        }
        if running.iter().any(&bears) {
            last_marked_at = Some(now);
        } else {
            let since = last_marked_at.or(first_member_at).unwrap_or(now);
            if now.saturating_sub(since) >= LEFT_CAP.as_millis() && !spell_just_ended {
                for p in &running {
                    book.leave(p);
                }
                book.write(
                    &mut act,
                    record,
                    "left_unmarked",
                    now,
                    trigger_at,
                    Some(&running),
                );
                eprintln!(
                    "guard_end cause=left_unmarked running={} programs={}",
                    running.len(),
                    act.running.as_deref().unwrap_or_default().join(",")
                );
                return 4;
            }
        }
        let fire = match trigger {
            Trigger::ExecutorGone {
                pid: epid,
                started_us: estart,
            } => match executor_in(&table, epid, estart, own_name) {
                Executor::Found => false,
                Executor::Untimed => {
                    act.untimed += 1;
                    false
                }
                Executor::Gone => true,
            },
            Trigger::Now => true,
        };
        // What this read did, for the cause of a write at its end.
        let mut cause = "";
        if fire {
            let since = *trigger_at.get_or_insert(now);
            if now.saturating_sub(since) >= grace {
                match asked_at {
                    None => {
                        for p in &running {
                            if !bears(p) {
                                book.leave(p);
                                continue;
                            }
                            book.ended(p);
                            if end(p, false) {
                                act.asked += 1;
                            }
                            note(&mut act, p, &root);
                        }
                        asked_at = Some(now);
                        cause = "act";
                    }
                    Some(at) if now.saturating_sub(at) >= TERM_WAIT.as_millis() => {
                        forced_at.get_or_insert(now);
                        for p in &running {
                            if !bears(p) {
                                if book.leave(p) && cause.is_empty() {
                                    cause = "left";
                                }
                                continue;
                            }
                            book.ended(p);
                            if end(p, true) {
                                act.forced += 1;
                                if !book.forced.iter().any(|f| procs::same(f, p)) {
                                    book.forced.push(p.clone());
                                    cause = "forced";
                                }
                            }
                            note(&mut act, p, &root);
                        }
                    }
                    Some(_) => {}
                }
            }
        } else {
            trigger_at = None;
            asked_at = None;
        }
        if cause == "act" || (book.written && book.changed(&act, Some(&running))) {
            let cause = if cause.is_empty() {
                book.why(&running)
            } else {
                cause
            };
            book.write(&mut act, record, cause, now, trigger_at, Some(&running));
        }
        pause(false);
    }
}

/// The guard's record as it is written (CP-189 G4): the members left so far, each by number and
/// creation time with its program's file name, less those that the guard later asked or ended;
/// the members that it ended by force; whether it wrote; and what the last write said, so that it
/// writes again only when that changes.
#[derive(Default)]
struct Book {
    left: Vec<(Proc, String)>,
    forced: Vec<Proc>,
    written: bool,
    last: Option<Said>,
}

/// What a write says, without its times: the counts sent, the members left, and what runs.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Said {
    asked: usize,
    forced: usize,
    left: Vec<(u32, u64)>,
    running: Option<Vec<(u32, u64)>>,
}

impl Book {
    /// Notes a member that the loop left because it did not bear the run's mark; true when it was
    /// not noted before.
    fn leave(&mut self, p: &Proc) -> bool {
        if self.left.iter().any(|(l, _)| procs::same(l, p)) {
            return false;
        }
        self.left.push((p.clone(), program_name(p)));
        true
    }

    /// Takes out of the left list a member that the guard asks or ends.
    fn ended(&mut self, p: &Proc) {
        self.left.retain(|(l, _)| !procs::same(l, p));
    }

    fn said(&self, act: &GuardAct, running: Option<&[Proc]>) -> Said {
        let ids = |v: &mut dyn Iterator<Item = &Proc>| -> Vec<(u32, u64)> {
            v.map(|p| (p.pid, p.started_us)).collect()
        };
        Said {
            asked: act.asked,
            forced: act.forced,
            left: ids(&mut self.left.iter().map(|(p, _)| p)),
            running: running.map(|r| ids(&mut r.iter())),
        }
    }

    /// Whether what a write would say now differs from the last write.
    fn changed(&self, act: &GuardAct, running: Option<&[Proc]>) -> bool {
        self.last.as_ref() != Some(&self.said(act, running))
    }

    /// The cause of a write that no act of this read made: a left member that is gone, or another
    /// change of what runs.
    fn why(&self, running: &[Proc]) -> &'static str {
        let gone = self
            .left
            .iter()
            .any(|(l, _)| !running.iter().any(|r| procs::same(r, l)));
        if gone {
            "left_gone"
        } else {
            "change"
        }
    }

    /// Writes the record with `cause` at `now`: the counts, the left list and what runs
    /// (`None`: the last read failed).
    #[allow(clippy::too_many_arguments)]
    fn write(
        &mut self,
        act: &mut GuardAct,
        record: &mut dyn FnMut(&GuardAct),
        cause: &'static str,
        now: u128,
        trigger_at: Option<u128>,
        running: Option<&[Proc]>,
    ) {
        act.ms = now.saturating_sub(trigger_at.unwrap_or(now));
        act.at_ms = now;
        act.cause = cause;
        act.left = self.left.len();
        act.left_programs = names_of(self.left.iter().map(|(_, n)| n.clone()));
        act.running = running.map(|r| names_of(r.iter().map(program_name)));
        act.running_count = running.map_or(0, <[Proc]>::len);
        record(act);
        self.last = Some(self.said(act, running));
        self.written = true;
    }
}

/// The file name of a process's program, with a space as `_`; `-` when it cannot be read.
fn program_name(p: &Proc) -> String {
    p.exe
        .as_ref()
        .and_then(|e| e.file_name())
        .map_or("-".to_string(), |n| n.to_string_lossy().replace(' ', "_"))
}

/// The names in their first order, each once.
fn names_of(names: impl Iterator<Item = String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for n in names {
        if !out.contains(&n) {
            out.push(n);
        }
    }
    out
}

/// What one read of the table says of the executor (CP-176 A4 (a)).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Executor {
    /// A row with its number and its creation time.
    Found,
    /// No such row, and a row with its number, no creation time and the guard's own file name
    /// `own_name` (or no file name): a process that the system could not open, which the guard
    /// counts as the executor, as a read that finds it.
    Untimed,
    /// Neither: a row with its number and another known creation time, or with no creation time and
    /// another program's file name, is another process.
    Gone,
}

/// The executor in `table` by its number `epid` and creation time `estart`, and on Windows
/// (`own_name` given) a row with its number and no creation time whose file name is the guard's
/// own or unknown (CP-176 A4 (a)). File names are compared with ASCII case folded, as Windows
/// names them.
fn executor_in(
    table: &[Proc],
    epid: u32,
    estart: u64,
    own_name: Option<&std::ffi::OsStr>,
) -> Executor {
    if table
        .iter()
        .any(|t| t.pid == epid && t.started_us == estart)
    {
        return Executor::Found;
    }
    let Some(own) = own_name else {
        return Executor::Gone;
    };
    let own = own.to_string_lossy().to_ascii_lowercase();
    let untimed = table.iter().any(|t| {
        t.pid == epid
            && t.started_us == 0
            && t.exe
                .as_deref()
                .and_then(|e| e.file_name())
                .is_none_or(|n| n.to_string_lossy().to_ascii_lowercase() == own)
    });
    if untimed {
        Executor::Untimed
    } else {
        Executor::Gone
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
/// ask and forced end and the run's `mark`: the executor's end of a start that gave no first reply
/// (CP-158 (1)). It returns once no member bears the mark and none is unread, or at one of its two
/// bounds (`NOW_UNREAD_BOUND`, `NOW_MARKED_BOUND`), and gives what it did and what it left
/// (CP-189 G5).
pub fn end_tree_now(root: &Proc, mark: &crate::checks::Mark) -> GuardAct {
    let start = std::time::Instant::now();
    let mut done = GuardAct::default();
    guard_loop(
        root.pid,
        root.started_us,
        Trigger::Now,
        GuardIo {
            read: &mut || procs::table().ok(),
            args: &crate::checks::command_args,
            mark: Some(mark),
            own_name: None,
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
        run_with(trigger, step, table, &every_marked)
    }

    /// The arguments of every process: the run's profile `/p` as a whole path, so that a row of a
    /// program inside the bundle bears the run's mark.
    fn every_marked(_pid: u32) -> Option<Vec<String>> {
        Some(vec!["--user-data-dir=/p".to_string()])
    }

    /// `run` with the arguments of a process given, from which the loop takes the run's mark.
    fn run_with(
        trigger: Trigger,
        step: u128,
        table: &dyn Fn(u128, &Ends) -> Option<Vec<Proc>>,
        args: &dyn Fn(u32) -> Option<Vec<String>>,
    ) -> (i32, u128, Ends, Option<GuardAct>) {
        run_full(trigger, step, table, args, None)
    }

    /// The file name of the guard's own program in the Windows tests of CP-176 A4.
    const OWN: &str = "halvo.exe";

    /// `run` as on Windows: the guard's own file name `OWN` given.
    fn run_windows(
        trigger: Trigger,
        step: u128,
        table: &dyn Fn(u128, &Ends) -> Option<Vec<Proc>>,
    ) -> (i32, u128, Ends, Option<GuardAct>) {
        run_full(
            trigger,
            step,
            table,
            &every_marked,
            Some(std::ffi::OsStr::new(OWN)),
        )
    }

    fn run_full(
        trigger: Trigger,
        step: u128,
        table: &dyn Fn(u128, &Ends) -> Option<Vec<Proc>>,
        args: &dyn Fn(u32) -> Option<Vec<String>>,
        own_name: Option<&std::ffi::OsStr>,
    ) -> (i32, u128, Ends, Option<GuardAct>) {
        let (code, at, ends, mut records) = run_records(trigger, step, table, args, own_name);
        (code, at, ends, records.pop())
    }

    /// `run_full` that gives every record that the loop wrote, in order (CP-185 (4)).
    fn run_records(
        trigger: Trigger,
        step: u128,
        table: &dyn Fn(u128, &Ends) -> Option<Vec<Proc>>,
        args: &dyn Fn(u32) -> Option<Vec<String>>,
        own_name: Option<&std::ffi::OsStr>,
    ) -> (i32, u128, Ends, Vec<GuardAct>) {
        let mark = run_mark();
        let clock = Cell::new(0u128);
        let ends = RefCell::new(Vec::new());
        let mut records = Vec::new();
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
                args,
                mark: Some(&mark),
                own_name,
                now_ms: &mut || clock.get(),
                pause: &mut |_| clock.set(clock.get() + step),
                end: &mut |p, force| {
                    ends.borrow_mut().push((p.pid, force, clock.get()));
                    true
                },
                record: &mut |act| records.push(act.clone()),
            },
        );
        (code, clock.get(), ends.into_inner(), records)
    }

    /// A row of a program inside the browser's bundle `/B.app`.
    fn row(pid: u32, ppid: u32, started_us: u64, pgid: u32) -> Proc {
        Proc {
            pid,
            ppid,
            started_us,
            pgid,
            exe: Some(std::path::PathBuf::from(BUNDLE_EXE)),
        }
    }

    /// A row of a program outside the browser's bundle, which never bears the run's mark.
    fn stranger(pid: u32, ppid: u32, started_us: u64, pgid: u32) -> Proc {
        Proc {
            exe: Some(std::path::PathBuf::from(
                "/Applications/Mail.app/Contents/MacOS/Mail",
            )),
            ..row(pid, ppid, started_us, pgid)
        }
    }

    const BUNDLE_EXE: &str = "/B.app/Contents/MacOS/b";

    /// A row of a program inside the browser's bundle; with `marked_args` it bears the run's mark.
    fn in_bundle(pid: u32, ppid: u32, started_us: u64, pgid: u32) -> Proc {
        Proc {
            exe: Some(std::path::PathBuf::from(BUNDLE_EXE)),
            ..row(pid, ppid, started_us, pgid)
        }
    }

    /// The arguments of the processes `marked`: the run's profile `/p` as a whole path.
    fn marked_args(marked: &'static [u32]) -> impl Fn(u32) -> Option<Vec<String>> {
        move |pid| {
            marked.contains(&pid).then(|| {
                vec![
                    "--type=renderer".to_string(),
                    "--user-data-dir=/p".to_string(),
                ]
            })
        }
    }

    /// The mark of the runs of the stand-in tables: the profile `/p`, the bundle `/B.app`, from 7 µs.
    fn run_mark() -> crate::checks::Mark {
        crate::checks::Mark {
            profile: "/p".into(),
            since_us: 7,
            root: std::path::PathBuf::from("/B.app"),
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
            1..=100 => Some(vec![row(ROOT, 1, 7, ROOT), stranger(5, 1, 50, 5)]),
            _ => Some(vec![stranger(5, 1, 50, 5)]),
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
            _ if t < 4_000 => Some(vec![row(ROOT, 1, 7, ROOT), stranger(5, 1, 50, 5)]),
            _ => Some(vec![stranger(5, 1, 50, 5)]),
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
                v.push(stranger(5, 1, 50, 5));
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
                v.push(stranger(5, 1, 50, 5));
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
            let mut v = vec![stranger(5, 1, 50, 5)];
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
                v.push(stranger(5, 1, 50, 5));
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
            let mut v = vec![stranger(5, 1, 51, 5)];
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
            let mut v = vec![stranger(5, 1, 50, 5)];
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
            let (code, at, ends, _) = run_with(
                executor(50),
                10,
                &|t, ends| {
                    let mut v = Vec::new();
                    if t < 500 {
                        v.push(in_bundle(ROOT, 1, 7, ROOT));
                    }
                    if t < 1_000 {
                        v.push(in_bundle(20, ROOT, 8, ROOT));
                    }
                    if t >= 1_000 && forced(ends, 30).is_none() {
                        v.push(in_bundle(30, ppid, 9, pgid));
                    }
                    Some(v)
                },
                &marked_args(&[ROOT, 20, 30]),
            );
            assert_eq!(code, 0);
            let end = forced(&ends, 30).expect("the orphan was ended");
            assert!(at >= end + 1_000, "the port freed at {at} ms ({ppid})");
        }
    }

    /// CP-170 E1 (c): the same orphan by its parent's number without the run's mark is no member:
    /// nothing asks or ends it.
    #[test]
    fn an_orphan_by_its_parent_s_number_without_the_mark_is_no_member() {
        let (code, _, ends, _) = run_with(
            executor(50),
            10,
            &|t, _| {
                let mut v = Vec::new();
                if t < 500 {
                    v.push(in_bundle(ROOT, 1, 7, 0));
                }
                if t < 1_000 {
                    v.push(in_bundle(20, ROOT, 8, 0));
                }
                if (1_000..60_000).contains(&t) {
                    v.push(in_bundle(30, 20, 9, 0));
                }
                Some(v)
            },
            &marked_args(&[ROOT, 20]),
        );
        assert_eq!(code, 0);
        assert!(ends.is_empty(), "{ends:?}");
    }

    /// CP-170 E1 (c), T1: a process that bears the mark and has a dead member's number as its
    /// parent number but began before that member is no member.
    #[test]
    fn a_marked_process_older_than_the_dead_member_is_no_member() {
        let args = marked_args(&[ROOT, 20, 30]);
        let root = in_bundle(ROOT, 1, 7, 0);
        let mark = crate::checks::Mark {
            profile: "/p".into(),
            since_us: 7,
            root: std::path::PathBuf::from("/B.app"),
        };
        let mut known = vec![root.clone(), in_bundle(20, ROOT, 9, 0)];
        let table = vec![in_bundle(30, 20, 8, 0)];
        assert!(members(&mut known, &table, &root, 4, false, Some(&mark), &args).is_empty());
        let mut known = vec![root.clone(), in_bundle(20, ROOT, 9, 0)];
        let table = vec![in_bundle(30, 20, 10, 0)];
        assert_eq!(
            members(&mut known, &table, &root, 4, false, Some(&mark), &args).len(),
            1
        );
    }

    /// The review's case me01 (evidence/pr-review-27/cases/me/01-orphan-dead-taker.case): member K
    /// (200) of the run ends; a foreign process takes number 200, starts F (300) and ends. F's
    /// parent number reads 200, it began after K, and no running process holds 200: F bears no
    /// mark and is no member.
    #[test]
    fn me01_a_child_of_a_foreign_taker_of_a_dead_member_s_number_is_no_member() {
        let args = marked_args(&[100, 200]);
        let root = in_bundle(100, 5, 10, 0);
        let k = in_bundle(200, 100, 20, 0);
        let mut known = Vec::new();
        let mark = Some(crate::checks::Mark {
            since_us: 10,
            ..run_mark()
        });
        let first = members(
            &mut known,
            &[root.clone(), k.clone()],
            &root,
            1,
            false,
            mark.as_ref(),
            &args,
        );
        assert_eq!(first.len(), 2, "{first:?}");
        let f = row(300, 200, 31, 0);
        let later = members(
            &mut known,
            &[root.clone(), f],
            &root,
            1,
            false,
            mark.as_ref(),
            &args,
        );
        assert!(!later.iter().any(|p| p.pid == 300), "{later:?}");
    }

    /// The review's case me01 through the loop: the executor (5) is gone, the root and K end, the
    /// foreign F runs on; the loop ends nothing but the run's tree.
    #[test]
    fn me01_the_loop_ends_no_child_of_a_foreign_taker_that_ended() {
        let (code, _, ends, _) = run_with(
            Trigger::ExecutorGone {
                pid: 5,
                started_us: 1,
            },
            20,
            &|t, _| {
                Some(if t < 100 {
                    vec![in_bundle(ROOT, 5, 7, 0), in_bundle(200, ROOT, 20, 0)]
                } else if t < 200 {
                    vec![in_bundle(ROOT, 5, 7, 0)]
                } else if t < 10_000 {
                    vec![row(300, 200, 31, 0)]
                } else {
                    vec![]
                })
            },
            &marked_args(&[ROOT, 200]),
        );
        assert_eq!(code, 0);
        assert!(!ends.iter().any(|e| e.0 == 300), "{ends:?}");
    }

    /// The verifier's G1 (CP-170 E1): the executor runs until 30,000 ms; the browser and its member
    /// 200 end at 1,000 ms; a foreign process takes number 200 at 1,500 ms, starts 300 and ends;
    /// 300 runs on. Nothing asks or ends 300, and the guard frees the port once the tree is gone.
    #[test]
    fn g1_a_foreign_child_of_a_reused_number_is_never_asked_or_ended() {
        let (code, at, ends, _) = run_with(
            executor(50),
            20,
            &|t, _| {
                let mut v = Vec::new();
                if t < 30_000 {
                    v.push(stranger(5, 1, 50, 5));
                }
                if t < 1_000 {
                    v.push(in_bundle(ROOT, 1, 7, 0));
                    v.push(in_bundle(200, ROOT, 20, 0));
                }
                if (1_500..2_000).contains(&t) {
                    v.push(row(200, 1, 40, 0));
                }
                if (1_800..60_000).contains(&t) {
                    v.push(row(300, 200, 45, 0));
                }
                Some(v)
            },
            &marked_args(&[ROOT, 200]),
        );
        assert_eq!(code, 0);
        assert!(ends.is_empty(), "{ends:?}");
        assert!(at < 5_000, "the port was held until {at} ms");
    }

    /// The verifier's G2 (CP-170 E2): the executor runs throughout, and the reads at 1 s and at
    /// 5 s lack its row; nothing is asked or ended.
    #[test]
    fn g2_two_reads_that_miss_a_running_executor_end_nothing() {
        let (code, _, ends, _) = run(executor(50), 20, &|t, _| {
            let mut v = Vec::new();
            if t != 1_000 && t != 5_000 {
                v.push(stranger(5, 1, 50, 5));
            }
            if t < 10_000 {
                v.push(row(ROOT, 1, 7, ROOT));
            } else if t < 20_000 {
                return Some(vec![stranger(5, 1, 50, 5)]);
            }
            Some(v)
        });
        assert_eq!(code, 0);
        assert!(ends.is_empty(), "{ends:?}");
    }

    /// CP-170 E2: an executor that one early read missed and that ends later gets the whole grace
    /// from its true end.
    #[test]
    fn an_executor_missed_once_early_gets_the_whole_grace_from_its_end() {
        let (code, _, ends, _) = run(executor(50), 20, &|t, ends| {
            let mut v = Vec::new();
            if t != 1_000 && t < 20_000 {
                v.push(stranger(5, 1, 50, 5));
            }
            if forced(ends, ROOT).is_none() {
                v.push(row(ROOT, 1, 7, ROOT));
            }
            Some(v)
        });
        assert_eq!(code, 0);
        let asked = ends.iter().find(|e| !e.1).expect("asked");
        assert!(asked.2 >= 20_000 + 3_000, "asked at {} ms", asked.2);
    }

    /// A row with the executor's number (5) and no creation time, as Windows gives a process
    /// that it cannot open, with the program's file name `name`.
    fn untimed(name: Option<&str>) -> Proc {
        Proc {
            exe: name.map(std::path::PathBuf::from),
            ..stranger(5, 1, 0, 5)
        }
    }

    /// A browser's first process that runs until the guard forces it, beside the executor's row
    /// `executor` until 10,000 ms.
    fn beside_until_10s(executor: Proc) -> impl Fn(u128, &Ends) -> Option<Vec<Proc>> {
        move |t, ends| {
            let mut v = Vec::new();
            if t < 10_000 {
                v.push(executor.clone());
            }
            if forced(ends, ROOT).is_none() {
                v.push(row(ROOT, 1, 7, ROOT));
            }
            Some(v)
        }
    }

    /// CP-176 A4 (a), (c) and (d): on Windows a row with the executor's number, no creation time
    /// and the guard's own file name (in any ASCII case, or with no file name) is a read that finds
    /// the executor: for 10,000 ms, longer than `GRACE` and `TERM_WAIT` together, nothing is asked
    /// or ended; when the row is gone the grace is counted from that read; the record counts the
    /// 500 reads that found the row.
    #[test]
    fn a4_an_untimed_row_with_the_guard_s_own_name_is_the_executor() {
        for name in [Some(OWN), Some("HALVO.EXE"), None] {
            let (code, _, ends, record) =
                run_windows(executor(50), 20, &beside_until_10s(untimed(name)));
            assert_eq!(code, 0, "{name:?}");
            let asked = ends.iter().find(|e| !e.1).expect("asked");
            assert!(
                asked.2 >= 10_000 + 3_000,
                "{name:?}: asked at {} ms",
                asked.2
            );
            assert_eq!(record.expect("recorded").untimed, 500, "{name:?}");
        }
    }

    /// CP-176 A4 (a) and (d): a row with the executor's number, no creation time and another
    /// program's file name is not the executor: the guard asks after its grace from the first read.
    #[test]
    fn a4_an_untimed_row_with_another_program_s_name_is_not_the_executor() {
        let (code, _, ends, record) = run_windows(
            executor(50),
            20,
            &beside_until_10s(untimed(Some("other.exe"))),
        );
        assert_eq!(code, 0);
        let asked = ends.iter().find(|e| !e.1).expect("asked");
        assert!(
            (3_000..10_000).contains(&asked.2),
            "asked at {} ms",
            asked.2
        );
        assert_eq!(record.expect("recorded").untimed, 0);
    }

    /// CP-176 A4 (a) and (d): on Windows a row with the executor's number and another known
    /// creation time is another process, whatever its file name.
    #[test]
    fn a4_a_row_with_another_known_time_is_not_the_executor_on_windows() {
        let other = Proc {
            exe: Some(OWN.into()),
            ..stranger(5, 1, 51, 5)
        };
        let (code, _, ends, _) = run_windows(executor(50), 20, &beside_until_10s(other));
        assert_eq!(code, 0);
        let asked = ends.iter().find(|e| !e.1).expect("asked");
        assert!(
            (3_000..10_000).contains(&asked.2),
            "asked at {} ms",
            asked.2
        );
    }

    /// CP-176 A4 (a): on macOS (no own file name given) nothing changes: a row with no creation
    /// time is not the executor.
    #[test]
    fn a4_on_macos_an_untimed_row_is_not_the_executor() {
        let (code, _, ends, _) = run(executor(50), 20, &beside_until_10s(untimed(Some(OWN))));
        assert_eq!(code, 0);
        let asked = ends.iter().find(|e| !e.1).expect("asked");
        assert!(
            (3_000..10_000).contains(&asked.2),
            "asked at {} ms",
            asked.2
        );
    }

    /// CP-176 A4 (b): the guard refuses an executor creation time of 0 at its start.
    #[test]
    fn a4_the_guard_refuses_an_executor_creation_time_of_0() {
        let argv = |estart: &str| -> Vec<String> {
            ["9", "7", "5", estart, "/r"]
                .iter()
                .map(|s| s.to_string())
                .collect()
        };
        let full = |estart: &str| {
            let mut v = argv(estart);
            v.extend(["/p".to_string(), "/B.app/Contents/MacOS/b".to_string()]);
            v
        };
        assert_eq!(parse_args(&full("0")), None);
        let got = parse_args(&full("50")).expect("read");
        assert_eq!(
            (got.pid, got.started_us, got.epid, got.estart),
            (9, 7, 5, 50)
        );
    }

    /// CP-185 (9), first case: a child of the browser's first process whose program lies outside
    /// the browser's directory (a mail program that the system starts for a link) is neither asked
    /// nor ended. The port is kept while the marked first process runs, and for `LEFT_CAP` after it
    /// is gone. The record is written at the act with the member that was left, and again at the
    /// cap with the cause `left_unmarked`.
    #[test]
    fn cp185_a_child_outside_the_browser_s_directory_is_neither_asked_nor_ended() {
        let (code, at, ends, records) = run_records(
            executor(50),
            100,
            &|t, ends| {
                let mut v = vec![stranger(70, ROOT, 20, 70)];
                if t < 1_000 {
                    v.push(stranger(5, 1, 50, 5));
                }
                if forced(ends, ROOT).is_none() {
                    v.push(row(ROOT, 1, 7, ROOT));
                }
                Some(v)
            },
            &every_marked,
            None,
        );
        assert!(ends.iter().all(|e| e.0 == ROOT), "{ends:?}");
        let gone = forced(&ends, ROOT).expect("the first process was ended");
        assert_eq!(code, 4);
        assert!(at >= gone + LEFT_CAP.as_millis(), "returned at {at} ms");
        let first = records.first().expect("a record at the act");
        assert_eq!(
            (first.cause, first.left, first.left_programs.clone()),
            ("act", 1, vec!["Mail".to_string()])
        );
        let last = records.last().unwrap();
        assert_eq!((last.cause, last.left), ("left_unmarked", 1));
    }

    /// CP-185 (9), second case: a process of the first process's group (macOS) without the mark is
    /// neither asked nor ended, and keeps the port for `LEFT_CAP` once the marked members are gone.
    #[test]
    fn cp185_a_process_of_the_group_without_the_mark_is_neither_asked_nor_ended() {
        let (code, at, ends, last) = run(executor(50), 100, &|t, ends| {
            let mut v = vec![stranger(71, 1, 20, ROOT)];
            if t < 1_000 {
                v.push(stranger(5, 1, 50, 5));
            }
            if forced(ends, ROOT).is_none() {
                v.push(row(ROOT, 1, 7, ROOT));
            }
            Some(v)
        });
        assert!(ends.iter().all(|e| e.0 == ROOT), "{ends:?}");
        let gone = forced(&ends, ROOT).expect("the first process was ended");
        assert_eq!(code, 4);
        assert!(at >= gone + LEFT_CAP.as_millis(), "returned at {at} ms");
        let last = last.expect("recorded");
        assert_eq!((last.cause, last.left), ("left_unmarked", 1));
    }

    /// CP-185 (9), third case: the browser's first process with its mark is asked at the grace and
    /// ended by force `TERM_WAIT` later, as before, and the record names no member that was left.
    #[test]
    fn cp185_the_first_process_with_its_mark_is_asked_and_then_ended_as_today() {
        let (code, _, ends, last) = run(executor(50), 10, &|t, ends| {
            let mut v = Vec::new();
            if t < 1_000 {
                v.push(stranger(5, 1, 50, 5));
            }
            if forced(ends, ROOT).is_none() {
                v.push(row(ROOT, 1, 7, ROOT));
            }
            Some(v)
        });
        assert_eq!(code, 0);
        let asked = ends.iter().find(|e| !e.1).expect("asked");
        assert_eq!(asked.0, ROOT);
        assert!(asked.2 >= 1_000 + 3_000, "asked at {} ms", asked.2);
        let forced_at = forced(&ends, ROOT).expect("forced");
        assert!(forced_at >= asked.2 + 2_000, "forced at {forced_at} ms");
        let last = last.expect("recorded");
        assert!(last.first);
        assert_eq!((last.cause, last.left), ("tree_gone", 0));
    }

    /// CP-185 (9), fourth case: a guard whose one member left does not bear the mark returns 4
    /// after `LEFT_CAP` with its last line, where before it never returned.
    #[test]
    fn cp185_a_guard_with_one_member_without_the_mark_returns_after_the_cap() {
        let (code, at, ends, last) = run(executor(50), 100, &|t, _| {
            let mut v = vec![stranger(72, ROOT, 20, 72)];
            if t < 1_000 {
                v.push(row(ROOT, 1, 7, ROOT));
            }
            Some(v)
        });
        assert!(ends.is_empty(), "{ends:?}");
        assert_eq!(code, 4);
        // CP-189 G6: the cap counts from the last read that showed the marked first process, at
        // 900 ms.
        assert!(at >= 900 + LEFT_CAP.as_millis(), "returned at {at} ms");
        assert!(
            at < 900 + LEFT_CAP.as_millis() + 1_000,
            "returned at {at} ms"
        );
        let last = last.expect("its last line");
        assert_eq!(
            (last.cause, last.left, last.left_programs.clone()),
            ("left_unmarked", 1, vec!["Mail".to_string()])
        );
        let line = last.line();
        assert!(line.starts_with("ended_by=none "), "{line}");
        assert!(
            line.contains(" left=1 left_programs=Mail running=1 running_programs=Mail "),
            "{line}"
        );
        assert!(line.ends_with(" cause=left_unmarked"), "{line}");
    }

    /// CP-186 S4: the count of reads in which the executor's row had no creation time reaches the
    /// guard's record also when the guard never acted, in a line that says nothing was ended.
    #[test]
    fn s4_the_untimed_count_reaches_the_record_when_the_guard_never_acted() {
        let (code, _, ends, record) = run_windows(executor(50), 20, &|t, _| {
            let mut v = Vec::new();
            if t < 1_000 {
                v.push(untimed(Some(OWN)));
            }
            if t < 500 {
                v.push(row(ROOT, 1, 7, ROOT));
            }
            Some(v)
        });
        assert_eq!(code, 0);
        assert!(ends.is_empty(), "{ends:?}");
        let record = record.expect("a record of the untimed reads");
        assert_eq!((record.asked, record.forced), (0, 0));
        assert_eq!(record.untimed, 25);
        let line = record.line();
        assert!(
            line.starts_with(
                "ended_by=none ms=0 asked=0 forced=0 first=no programs=- executor_untimed=25 \
                 left=0 left_programs=- running=0 running_programs=- at_ms="
            ),
            "{line}"
        );
        assert!(line.ends_with(" cause=untimed"), "{line}");
    }

    /// CP-185 (9), fifth case: the guard's entry refuses a start without the run's profile or the
    /// browser's program, and builds the mark from both.
    #[test]
    fn cp185_a_start_without_the_two_arguments_is_refused() {
        let argv = |rest: &[&str]| -> Vec<String> {
            ["9", "7", "5", "50", "/r"]
                .iter()
                .chain(rest.iter())
                .map(|s| s.to_string())
                .collect()
        };
        assert!(parse_args(&argv(&[])).is_none());
        assert!(parse_args(&argv(&["/p"])).is_none());
        assert!(parse_args(&argv(&["", "/B.app/Contents/MacOS/b"])).is_none());
        assert!(parse_args(&argv(&["/p", ""])).is_none());
        let got = parse_args(&argv(&["/p", "/B.app/Contents/MacOS/b"])).expect("a mark");
        assert_eq!((got.mark.profile.as_str(), got.mark.since_us), ("/p", 7));
        assert_eq!(got.record, Some(std::path::PathBuf::from("/r")));
    }

    /// The verifier's m36: the margin begins again when a member is seen again after an empty read.
    #[test]
    fn m36_the_margin_begins_again_after_a_member_is_seen_again() {
        let (code, at, _, _) = run(executor(50), 10, &|t, _| {
            let mut v = vec![stranger(5, 1, 50, 5)];
            if t < 1_000 || (1_010..3_000).contains(&t) {
                v.push(row(ROOT, 1, 7, ROOT));
            }
            Some(v)
        });
        assert_eq!(code, 0);
        assert!(at >= 3_000 + 1_000, "the port freed at {at} ms");
    }

    /// The verifier's m39: the guard never takes its own number as a member, also as a child of a
    /// member.
    #[test]
    fn m39_the_guard_s_own_number_is_never_a_member() {
        let root = row(ROOT, 1, 7, ROOT);
        let mut known = Vec::new();
        let table = vec![root.clone(), row(4, ROOT, 8, ROOT)];
        let got = members(&mut known, &table, &root, 4, false, None, &|_| None);
        assert_eq!(got.iter().map(|p| p.pid).collect::<Vec<_>>(), vec![ROOT]);
    }

    /// The verifier's m42: the unread time is one unbroken spell, so two spells that sum to more
    /// than `UNREAD_CAP` with a read between them do not end the guard.
    #[test]
    fn m42_two_unread_spells_with_a_read_between_do_not_end_the_guard() {
        let (code, at, _, _) = run(executor(50), 1_000, &|t, _| match t {
            0..=199_000 => None,
            200_000 => Some(vec![stranger(5, 1, 50, 5), row(ROOT, 1, 7, ROOT)]),
            201_000..=399_000 => None,
            _ if t < 500_000 => Some(vec![stranger(5, 1, 50, 5), row(ROOT, 1, 7, ROOT)]),
            _ => Some(vec![stranger(5, 1, 50, 5)]),
        });
        assert_eq!(code, 0, "ended at {at} ms");
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
            stranger(5, 1, 50, 5),
        ];
        let none = |_: u32| None;
        assert!(members(&mut known, &table, &root, 4, false, None, &none).is_empty());
        let args = marked_args(&[30]);
        let mark = crate::checks::Mark {
            profile: "/p".into(),
            since_us: 7,
            root: std::path::PathBuf::from("/B.app"),
        };
        let mut known = vec![root.clone(), row(20, ROOT, 8, ROOT)];
        let table = vec![in_bundle(30, 20, 10, 0), stranger(5, 1, 50, 5)];
        assert_eq!(
            members(&mut known, &table, &root, 4, false, Some(&mark), &args).len(),
            1
        );
        let mut known = vec![root.clone()];
        let table = vec![row(32, 1, 10, ROOT), row(33, 1, 6, ROOT)];
        assert_eq!(
            members(&mut known, &table, &root, 4, true, None, &none).len(),
            1
        );
        let mut known = vec![root.clone()];
        assert!(members(&mut known, &table, &root, 4, false, None, &none).is_empty());
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
                    let mut v = vec![
                        stranger(61, 1, 3, 61),
                        stranger(62, 61, 4, 61),
                        stranger(63, 1, 9, 63),
                    ];
                    if !asked.borrow().iter().any(|a| a.1) {
                        v.push(row(ROOT, 1, 7, ROOT));
                    }
                    Some(v)
                },
                args: &every_marked,
                mark: Some(&run_mark()),
                own_name: None,
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
    /// The stand-in browser bears the run's mark (CP-185 (1)): `/bin/sh` waiting on a pipe of the
    /// test, with the argument `--user-data-dir=<profile>` (CP-189 G1), and the mark's program
    /// `/bin/sh`.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_guard_ends_a_real_stand_in_after_its_executor_is_killed() {
        use crate::start::Start;
        let profile =
            std::env::temp_dir().join(format!("halvo-guard-fence-profile-{}", std::process::id()));
        let mut own = Own(Vec::new());
        own.0.push(
            std::process::Command::new("/bin/sh")
                .args(["-c", "read x", "stand-in"])
                .arg(format!("--user-data-dir={}", profile.display()))
                .stdin(std::process::Stdio::piped())
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
        let mark =
            crate::checks::Mark::new(&profile, std::path::Path::new("/bin/sh"), b.started_us);
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
                args: &crate::checks::command_args,
                mark: Some(&mark),
                own_name: None,
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
        let name = b
            .exe
            .as_ref()
            .and_then(|e| e.file_name())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert_eq!(act.programs, [name]);
        assert_eq!(act.left, 0, "{act:?}");
        assert!(fence.refused.borrow().is_empty());
    }

    // CP-189: the tests of the closed list after the step of three readers on the changed guard.

    /// CP-189 G7, fenced, on the real process table of each system: the guard's loop leaves a
    /// process without the mark and writes it into its record. The stand-in browser is this test
    /// binary with the run's profile as an argument; its child is this test binary with no
    /// profile, a member that does not bear the mark. The executor, another such child, is ended
    /// at 500 ms. The guard asks and ends the stand-in browser through the fence, which admits it
    /// alone, and leaves its child; the test then ends that child, its own, and the guard reads the
    /// tree gone. The test has a bound of 30 s inside its read and pause.
    #[test]
    fn g7_the_guard_leaves_a_process_without_the_mark_on_the_real_table() {
        use crate::start::Start;
        struct Own(Vec<std::process::Child>);
        impl Drop for Own {
            fn drop(&mut self) {
                for c in &mut self.0 {
                    let _ = c.kill();
                    let _ = c.wait();
                }
            }
        }
        let me = std::env::current_exe().unwrap();
        let profile =
            std::env::temp_dir().join(format!("halvo-guard-left-profile-{}", std::process::id()));
        let start = |entry: &str, extra: &[String]| {
            std::process::Command::new(&me)
                .args(crate::child::entry_args(entry, extra))
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .start()
                .unwrap()
        };
        let mut own = Own(Vec::new());
        own.0.push(start(
            "child::hold_parent_entry",
            &[
                "--hold-parent".to_string(),
                format!("--user-data-dir={}", profile.display()),
            ],
        ));
        own.0.push(start(
            "child::hold_child_entry",
            &["--hold-child".to_string()],
        ));
        let (bpid, epid) = (own.0[0].id(), own.0[1].id());
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let (b, e, kid) = loop {
            let table = procs::table().unwrap();
            let b = table.iter().find(|p| p.pid == bpid).cloned();
            let e = table.iter().find(|p| p.pid == epid).cloned();
            let kid = b.as_ref().and_then(|b| {
                table
                    .iter()
                    .find(|p| p.ppid == b.pid && p.started_us >= b.started_us)
                    .cloned()
            });
            if let (Some(b), Some(e), Some(kid)) = (b, e, kid) {
                break (b, e, kid);
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the stand-ins did not start"
            );
            std::thread::sleep(Duration::from_millis(100));
        };
        let mark = crate::checks::Mark::new(&profile, &me, b.started_us);
        let fence = Fence {
            admit: (b.pid, b.started_us),
            refused: RefCell::new(Vec::new()),
        };
        let started = std::time::Instant::now();
        let bound = Duration::from_secs(30);
        let mut records: Vec<GuardAct> = Vec::new();
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
                        started.elapsed() < bound,
                        "the guard ran past the test's bound"
                    );
                    let mut own = own.borrow_mut();
                    if started.elapsed() > Duration::from_millis(500) {
                        let _ = own.0[1].kill();
                        let _ = own.0[1].wait();
                    }
                    let _ = own.0[0].try_wait();
                    let table = procs::table().ok()?;
                    if !table.iter().any(|t| procs::same(t, &b)) {
                        procs::kill(&kid);
                    }
                    procs::table().ok()
                },
                args: &crate::checks::command_args,
                mark: Some(&mark),
                own_name: None,
                now_ms: &mut || started.elapsed().as_millis(),
                pause: &mut |_| {
                    assert!(
                        started.elapsed() < bound,
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
                record: &mut |act| records.push(act.clone()),
            },
        );
        assert_eq!(code, 0);
        assert!(fence.refused.borrow().is_empty());
        assert!(!procs::running(&b), "the stand-in browser still runs");
        let name = me.file_name().unwrap().to_string_lossy().replace(' ', "_");
        let last = records.last().expect("the guard wrote its record");
        assert_eq!((last.left, last.left_programs.clone()), (1, vec![name]));
        assert!(last.first, "{last:?}");
    }

    /// A loop on stand-in tables in which the arguments of a process may change with the time:
    /// `table(t, ends)` gives the table at `t` ms and `args(pid, t)` the arguments of `pid` at
    /// `t`. Gives the exit, the ms at the exit, the ends and every record in order.
    fn run_timed(
        trigger: Trigger,
        step: u128,
        table: &dyn Fn(u128, &Ends) -> Option<Vec<Proc>>,
        args: &dyn Fn(u32, u128) -> Option<Vec<String>>,
    ) -> (i32, u128, Ends, Vec<GuardAct>) {
        let mark = run_mark();
        let clock = Cell::new(0u128);
        let ends = RefCell::new(Vec::new());
        let mut records = Vec::new();
        let mut reads = 0u64;
        let read_args = |pid: u32| args(pid, clock.get());
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
                args: &read_args,
                mark: Some(&mark),
                own_name: None,
                now_ms: &mut || clock.get(),
                pause: &mut |_| clock.set(clock.get() + step),
                end: &mut |p, force| {
                    ends.borrow_mut().push((p.pid, force, clock.get()));
                    true
                },
                record: &mut |act| records.push(act.clone()),
            },
        );
        (code, clock.get(), ends.into_inner(), records)
    }

    /// The arguments of every process: the run's profile `/p`, except `pid` from `from_ms` on,
    /// whose arguments are `then` (`None`: unread).
    fn args_but(
        pid: u32,
        from_ms: u128,
        then: Option<&'static str>,
    ) -> impl Fn(u32, u128) -> Option<Vec<String>> {
        move |p, t| {
            if p == pid && t >= from_ms {
                then.map(|a| vec![a.to_string()])
            } else {
                every_marked(p)
            }
        }
    }

    /// The table of the cases of G7: the executor runs until 1,000 ms; the first process runs until
    /// the guard ends it by force, or until 20,000 ms when `root_ends_itself`; `member` runs until
    /// 20,000 ms beside it (`None`: no member beside it).
    fn g7_table(
        root: Proc,
        member: Option<Proc>,
        root_ends_itself: bool,
    ) -> impl Fn(u128, &Ends) -> Option<Vec<Proc>> {
        move |t, ends| {
            let mut v = Vec::new();
            if t < 1_000 {
                v.push(stranger(5, 1, 50, 5));
            }
            let root_runs = if root_ends_itself {
                t < 20_000
            } else {
                forced(ends, ROOT).is_none()
            };
            if root_runs {
                v.push(root.clone());
            }
            if let Some(m) = &member {
                if t < 20_000 && forced(ends, m.pid).is_none() {
                    v.push(m.clone());
                }
            }
            Some(v)
        }
    }

    /// CP-189 G7: a table over the clauses of the run's mark at the ask and at the forced end
    /// (CP-185 (1)). In each row the process named does not bear the mark at the read of that act,
    /// so it is passed to neither the ask nor the forced end, or, for the rows "after the ask",
    /// it is asked while it bears the mark and not ended by force once it does not. Every other
    /// process of the row bears the mark and is asked and ended as before.
    #[test]
    fn g7_the_mark_s_clauses_at_the_ask_and_at_the_forced_end() {
        let gone = executor(50);
        let unread_exe = Proc {
            exe: None,
            ..row(30, ROOT, 20, 0)
        };
        type Table = Box<dyn Fn(u128, &Ends) -> Option<Vec<Proc>>>;
        type Args = Box<dyn Fn(u32, u128) -> Option<Vec<String>>>;
        let cases: Vec<(&str, Trigger, u32, bool, Table, Args)> = vec![
            (
                "the first process without the mark (its program outside the bundle)",
                gone,
                ROOT,
                false,
                Box::new(g7_table(stranger(ROOT, 1, 7, ROOT), None, true)),
                Box::new(args_but(0, 0, None)),
            ),
            (
                "a member whose program cannot be read",
                gone,
                30,
                false,
                Box::new(g7_table(row(ROOT, 1, 7, ROOT), Some(unread_exe), false)),
                Box::new(args_but(0, 0, None)),
            ),
            (
                "a member whose arguments cannot be read",
                gone,
                31,
                false,
                Box::new(g7_table(
                    row(ROOT, 1, 7, ROOT),
                    Some(row(31, ROOT, 20, 0)),
                    false,
                )),
                Box::new(args_but(31, 0, None)),
            ),
            (
                "Trigger::Now and a member without the mark",
                Trigger::Now,
                32,
                false,
                Box::new(g7_table(
                    row(ROOT, 1, 7, ROOT),
                    Some(stranger(32, ROOT, 20, 0)),
                    false,
                )),
                Box::new(args_but(0, 0, None)),
            ),
            (
                "a process of the bundle that names another profile",
                gone,
                33,
                false,
                Box::new(g7_table(
                    row(ROOT, 1, 7, ROOT),
                    Some(row(33, ROOT, 20, 0)),
                    false,
                )),
                Box::new(args_but(33, 0, Some("--user-data-dir=/other"))),
            ),
            (
                "a member whose arguments are unread after the ask",
                gone,
                34,
                true,
                Box::new(g7_table(
                    row(ROOT, 1, 7, ROOT),
                    Some(row(34, ROOT, 20, 0)),
                    false,
                )),
                Box::new(args_but(34, 4_500, None)),
            ),
            (
                "a member that names another profile after the ask",
                gone,
                35,
                true,
                Box::new(g7_table(
                    row(ROOT, 1, 7, ROOT),
                    Some(row(35, ROOT, 20, 0)),
                    false,
                )),
                Box::new(args_but(35, 4_500, Some("--user-data-dir=/other"))),
            ),
            (
                "the first process that names another profile after the ask",
                gone,
                ROOT,
                true,
                Box::new(g7_table(row(ROOT, 1, 7, ROOT), None, true)),
                Box::new(args_but(ROOT, 4_500, Some("--user-data-dir=/other"))),
            ),
        ];
        for (name, trigger, pid, asked_first, table, args) in cases {
            let (_, _, ends, _) = run_timed(trigger, 100, &*table, &*args);
            let on_pid: Vec<&(u32, bool, u128)> = ends.iter().filter(|e| e.0 == pid).collect();
            if asked_first {
                assert!(
                    on_pid.iter().any(|e| !e.1),
                    "{name}: asked while it bore the mark: {ends:?}"
                );
                assert!(!on_pid.iter().any(|e| e.1), "{name}: {ends:?}");
            } else {
                assert!(on_pid.is_empty(), "{name}: {ends:?}");
            }
            if pid != ROOT {
                assert!(
                    ends.iter().any(|e| e.0 == ROOT && e.1),
                    "{name}: the marked first process is ended by force: {ends:?}"
                );
            }
        }
    }

    /// CP-189 G1 at the guard: a member of the bundle whose arguments name the run's profile `/p`
    /// otherwise than by the two exact forms (`/p word`, `/q/p`, a file below `/p` with no profile
    /// argument) is neither asked nor ended; the first process with `--user-data-dir=/p` is.
    #[test]
    fn g1_a_member_that_names_the_profile_otherwise_is_not_asked() {
        // On Windows one command line with the quotes that it holds; on macOS each argument apart.
        let cases: Vec<Vec<String>> = if cfg!(windows) {
            vec![
                vec![r#"b.exe "--user-data-dir=/p word""#.to_string()],
                vec!["b.exe --user-data-dir=/q/p".to_string()],
                vec!["b.exe /p/Downloads/report.pdf".to_string()],
            ]
        } else {
            vec![
                vec!["--user-data-dir=/p word".to_string()],
                vec!["--user-data-dir=/q/p".to_string()],
                vec![
                    "/B.app/Contents/MacOS/b".to_string(),
                    "/p/Downloads/report.pdf".to_string(),
                ],
            ]
        };
        for other in cases {
            let args = |pid: u32, _t: u128| {
                if pid == 40 {
                    Some(other.clone())
                } else {
                    every_marked(pid)
                }
            };
            let (_, _, ends, _) = run_timed(
                executor(50),
                100,
                &g7_table(row(ROOT, 1, 7, ROOT), Some(row(40, ROOT, 20, 0)), false),
                &args,
            );
            assert!(!ends.iter().any(|e| e.0 == 40), "{other:?}: {ends:?}");
            assert!(
                ends.iter().any(|e| e.0 == ROOT && e.1),
                "{other:?}: {ends:?}"
            );
        }
    }

    /// CP-189 G6: `LEFT_CAP` is 300 s, written as a number.
    #[test]
    fn g6_the_cap_is_300_seconds() {
        assert_eq!(LEFT_CAP, Duration::from_secs(300));
    }

    /// CP-189 G6, adversary 3's case: the marked first process is last read at 900 ms, the table
    /// then stays unread for 290 s, and afterwards only a member without the mark runs. The cap
    /// counts from the last read that showed a marked member, so the guard leaves at about
    /// 300 s, not 300 s after the unread spell.
    #[test]
    fn g6_the_cap_counts_from_the_last_read_that_showed_a_marked_member() {
        let (code, at, ends, _) = run_timed(
            executor(50),
            100,
            &|t, _| {
                let mut v = vec![stranger(5, 1, 50, 5), stranger(70, ROOT, 20, 70)];
                if t < 1_000 {
                    v.push(row(ROOT, 1, 7, ROOT));
                    return Some(v);
                }
                if t < 291_000 {
                    return None;
                }
                Some(v)
            },
            &|p, _| every_marked(p),
        );
        assert!(ends.is_empty(), "{ends:?}");
        assert_eq!(code, 4);
        assert!((300_000..302_000).contains(&at), "returned at {at} ms");
    }

    /// CP-189 G6, the review's first shape: a member that bears the mark, then does not, then
    /// does again keeps the port while it bears it; the cap counts from its last marked read.
    #[test]
    fn g6_a_member_that_bears_the_mark_again_keeps_the_port() {
        let (code, at, ends, _) = run_timed(
            executor(50),
            1_000,
            &|t, _| {
                let mut v = vec![stranger(5, 1, 50, 5), row(ROOT, 1, 7, ROOT)];
                if t < 400_000 {
                    v.push(row(40, ROOT, 20, 0));
                }
                Some(v)
            },
            &|p, t| {
                if p == ROOT || (p == 40 && (10_000..20_000).contains(&t)) {
                    Some(vec!["--user-data-dir=/other".to_string()])
                } else {
                    every_marked(p)
                }
            },
        );
        assert!(ends.is_empty(), "{ends:?}");
        assert_eq!(code, 4);
        assert!(at >= 400_000 + 299_000, "returned at {at} ms");
    }

    /// CP-189 G6, the review's second shape: with no marked member at first, a marked member that
    /// is born later and runs from 100 s to 200 s keeps the port, and the cap counts from its last
    /// read, not from the guard's first read.
    #[test]
    fn g6_a_marked_member_born_after_a_span_with_none_keeps_the_port() {
        let (code, at, ends, _) = run_timed(
            executor(50),
            1_000,
            &|t, _| {
                let mut v = vec![stranger(5, 1, 50, 5), stranger(ROOT, 1, 7, ROOT)];
                if (100_000..200_000).contains(&t) {
                    v.push(row(40, ROOT, 100_000, 0));
                }
                Some(v)
            },
            &|p, _| every_marked(p),
        );
        assert!(ends.is_empty(), "{ends:?}");
        assert_eq!(code, 4);
        assert!(at >= 199_000 + 300_000, "returned at {at} ms");
    }

    /// CP-189 G6: after an unread spell the guard does not leave by the cap at the first read; one
    /// further read must agree. The marked first process is last read at 900 ms; members without
    /// the mark are read until 50 s, the table is unread from 50 s to 320 s, and the first read
    /// after it already lies past the cap.
    #[test]
    fn g6_one_further_read_must_agree_after_an_unread_spell() {
        let first_after = std::cell::Cell::new(None::<u128>);
        let (code, at, _, _) = run_timed(
            executor(50),
            100,
            &|t, _| {
                let mut v = vec![stranger(5, 1, 50, 5), stranger(70, ROOT, 20, 70)];
                if t < 1_000 {
                    v.push(row(ROOT, 1, 7, ROOT));
                    return Some(v);
                }
                if (50_000..320_000).contains(&t) {
                    return None;
                }
                if t >= 320_000 && first_after.get().is_none() {
                    first_after.set(Some(t));
                }
                Some(v)
            },
            &|p, _| every_marked(p),
        );
        assert_eq!(code, 4);
        let first = first_after.get().expect("a read after the spell");
        assert!(
            at > first,
            "left at the first read after the spell, {at} ms"
        );
    }

    /// CP-189 G5, adversary 2's case C: the executor's end of a start with no first reply returns
    /// at the first read at or after the forced end at which no member bears the mark and none is
    /// unread. Mail, a child of the first process, is left and named in the record, whose cause is
    /// no `tree_gone` while it runs.
    #[test]
    fn g5_the_end_of_a_failed_start_returns_once_no_member_bears_the_mark() {
        let (code, at, ends, records) = run_timed(
            Trigger::Now,
            100,
            &|_, ends| {
                let mut v = vec![stranger(70, ROOT, 20, 0)];
                if forced(ends, ROOT).is_none() {
                    v.push(row(ROOT, 1, 7, ROOT));
                }
                Some(v)
            },
            &|p, _| every_marked(p),
        );
        assert_eq!(code, 0);
        assert!(!ends.iter().any(|e| e.0 == 70), "{ends:?}");
        let forced_at = forced(&ends, ROOT).expect("forced");
        assert!(
            at <= forced_at + 300,
            "returned at {at} ms, forced at {forced_at} ms"
        );
        let last = records.last().expect("a record");
        assert_eq!(
            (last.cause, last.left, last.left_programs.clone()),
            ("now_left", 1, vec!["Mail".to_string()])
        );
    }

    /// CP-189 G5: a member whose arguments stay unread ends the wait at a bound of its own after
    /// the forced end, with a cause of its own.
    #[test]
    fn g5_a_member_that_stays_unread_ends_the_wait_at_its_bound() {
        let (_, at, ends, records) = run_timed(
            Trigger::Now,
            100,
            &|_, ends| {
                let mut v = vec![row(71, ROOT, 20, 0)];
                if forced(ends, ROOT).is_none() {
                    v.push(row(ROOT, 1, 7, ROOT));
                }
                Some(v)
            },
            &|p, _| if p == 71 { None } else { every_marked(p) },
        );
        assert!(!ends.iter().any(|e| e.0 == 71), "{ends:?}");
        let forced_at = forced(&ends, ROOT).expect("forced");
        let bound = NOW_UNREAD_BOUND.as_millis();
        assert!(
            (forced_at + bound..forced_at + bound + 300).contains(&at),
            "returned at {at} ms"
        );
        assert_eq!(records.last().expect("a record").cause, "now_unread");
    }

    /// CP-189 G5: a marked member that does not end under the forced end ends the wait at a bound
    /// of its own, with a cause of its own.
    #[test]
    fn g5_a_marked_member_that_does_not_end_ends_the_wait_at_its_bound() {
        let (_, at, ends, records) = run_timed(
            Trigger::Now,
            100,
            &|_, ends| {
                let mut v = vec![row(72, ROOT, 20, 0)];
                if forced(ends, ROOT).is_none() {
                    v.push(row(ROOT, 1, 7, ROOT));
                }
                Some(v)
            },
            &|p, _| every_marked(p),
        );
        let first_forced = forced(&ends, 72).expect("forced");
        let bound = NOW_MARKED_BOUND.as_millis();
        assert!(
            (first_forced + bound..first_forced + bound + 300).contains(&at),
            "returned at {at} ms"
        );
        assert_eq!(records.last().expect("a record").cause, "now_marked");
    }

    /// CP-189 G4, adversary 2's case A and the review's guard/05: a member without the mark that
    /// appears after the ask is left at the forced end, and the record written at that read names
    /// it, with the forced end counted.
    #[test]
    fn g4_a_member_left_at_the_forced_end_is_written_at_that_read() {
        let (_, _, ends, records) = run_timed(
            executor(50),
            100,
            &|t, ends| {
                let mut v = Vec::new();
                if t < 1_000 {
                    v.push(stranger(5, 1, 50, 5));
                }
                if t >= 4_500 {
                    v.push(stranger(80, ROOT, 4_500, 0));
                }
                if forced(ends, ROOT).is_none() {
                    v.push(row(ROOT, 1, 7, ROOT));
                }
                Some(v)
            },
            &|p, _| every_marked(p),
        );
        let forced_at = forced(&ends, ROOT).expect("forced");
        let at_force = records
            .iter()
            .find(|r| r.at_ms == forced_at)
            .expect("a record written at the read of the forced end");
        assert_eq!(
            (
                at_force.forced,
                at_force.left,
                at_force.left_programs.clone()
            ),
            (1, 1, vec!["Mail".to_string()]),
            "{at_force:?}"
        );
    }

    /// CP-189 G4, adversary 2's case B: at the cap the last line names what still runs at that
    /// write, apart from what was left so far. Mail and foo are left at the ask; Mail ends at 10 s.
    #[test]
    fn g4_the_last_line_names_what_still_runs() {
        let foo = |pid| Proc {
            exe: Some("/usr/local/bin/foo".into()),
            ..row(pid, ROOT, 20, 0)
        };
        let (code, _, _, records) = run_timed(
            executor(50),
            1_000,
            &move |t, ends| {
                let mut v = vec![foo(71)];
                if t < 1_000 {
                    v.push(stranger(5, 1, 50, 5));
                }
                if t < 10_000 {
                    v.push(stranger(70, ROOT, 20, 0));
                }
                if forced(ends, ROOT).is_none() {
                    v.push(row(ROOT, 1, 7, ROOT));
                }
                Some(v)
            },
            &|p, _| every_marked(p),
        );
        assert_eq!(code, 4);
        let last = records.last().expect("a last line");
        assert_eq!(last.cause, "left_unmarked");
        assert_eq!(last.running, Some(vec!["foo".to_string()]), "{last:?}");
        assert!(
            last.line().contains("running=1 running_programs=foo"),
            "{}",
            last.line()
        );
    }

    /// CP-189 G4, the review's guard/02: a member left at the forced end, then a table that stays
    /// unread until the guard's exit 3: the last record names the member, says that the last read
    /// failed, and has the exit's cause.
    #[test]
    fn g4_the_exit_after_an_unread_table_writes_what_was_left() {
        let (code, _, _, records) = run_timed(
            executor(50),
            1_000,
            &|t, ends| {
                if t > 10_000 {
                    return None;
                }
                let mut v = Vec::new();
                if t < 1_000 {
                    v.push(stranger(5, 1, 50, 5));
                }
                if t >= 4_500 {
                    v.push(stranger(80, ROOT, 4_500, 0));
                }
                if forced(ends, ROOT).is_none() {
                    v.push(row(ROOT, 1, 7, ROOT));
                }
                Some(v)
            },
            &|p, _| every_marked(p),
        );
        assert_eq!(code, 3);
        let last = records.last().expect("a last record");
        assert_eq!(last.cause, "table_unread");
        assert_eq!(last.left_programs, vec!["Mail".to_string()]);
        assert_eq!(last.running, None);
        assert!(last.line().contains("running=unread"), "{}", last.line());
    }

    /// CP-189 G4, the review's guard/03: a cap at which the guard asked and ended nothing says
    /// `ended_by=none`; once it asked or ended, every line says `ended_by=guard`.
    #[test]
    fn g4_the_first_word_says_ended_by_guard_only_when_something_was_sent() {
        let (code, _, ends, records) = run_timed(
            executor(50),
            1_000,
            &|_, _| Some(vec![stranger(5, 1, 50, 5), stranger(ROOT, 1, 7, ROOT)]),
            &|p, _| every_marked(p),
        );
        assert_eq!(code, 4);
        assert!(ends.is_empty());
        let last = records.last().expect("a last line");
        assert!(last.line().starts_with("ended_by=none "), "{}", last.line());
        let (_, _, _, records) = run_timed(
            executor(50),
            100,
            &g7_table(row(ROOT, 1, 7, ROOT), None, false),
            &|p, _| every_marked(p),
        );
        assert!(!records.is_empty());
        for r in &records {
            assert!(r.line().starts_with("ended_by=guard "), "{}", r.line());
        }
    }

    /// CP-189 G4: a member that was left at the ask and later bears the mark and is ended by force
    /// is in no list as left.
    #[test]
    fn g4_a_member_left_and_later_ended_is_not_left() {
        let (_, _, ends, records) = run_timed(
            executor(50),
            100,
            &g7_table(row(ROOT, 1, 7, ROOT), Some(row(36, ROOT, 20, 0)), false),
            &|p, t| {
                if p == 36 && t < 4_500 {
                    Some(vec!["--user-data-dir=/other".to_string()])
                } else {
                    every_marked(p)
                }
            },
        );
        assert!(ends.iter().any(|e| e.0 == 36 && e.1), "{ends:?}");
        let last = records.last().expect("a record");
        assert_eq!(
            (last.left, last.left_programs.clone()),
            (0, Vec::<String>::new()),
            "{last:?}"
        );
    }

    /// CP-189 G4: the record is written beside its place and renamed over it, so the place holds
    /// the last whole line, and a write that cannot finish leaves the line before it as it was.
    #[test]
    fn g4_the_record_is_written_beside_and_renamed_whole() {
        let dir = std::env::temp_dir().join(format!("halvo-guard-record-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("guard.record");
        let first = GuardAct {
            asked: 1,
            cause: "act",
            ..GuardAct::default()
        };
        write_record(&path, &first);
        assert_eq!(
            std::fs::read_to_string(&path).expect("read"),
            first.line() + "\n"
        );
        assert!(!dir.join("guard.record.writing").exists());
        // A write that cannot make the file beside leaves the record as it was.
        std::fs::create_dir(dir.join("guard.record.writing")).expect("blocker");
        let second = GuardAct {
            asked: 1,
            forced: 1,
            cause: "forced",
            ..GuardAct::default()
        };
        write_record(&path, &second);
        let held = std::fs::read_to_string(&path).expect("read");
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(held, first.line() + "\n");
    }
}
