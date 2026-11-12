use std::num::{NonZeroU32, NonZeroU64};
use std::process::{Child, ChildStdin, ChildStdout, Command};
use std::thread;
use std::time::{Duration, Instant};

use halvo_schema::executable::frame::Browser;
use halvo_schema::executable::text::CreationTime;

use crate::api::Reaped;
use crate::marks::{Group, Mark, ProcLine};
use crate::sys;

const GONE_WAIT: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(20);

/// The runner that this life made: its child, which stays unreaped until the library has done with its group, the
/// facts of its mark's lines, and on Windows the job that holds it.
pub(crate) struct RunProc {
    child: Child,
    reaped: bool,
    line: Option<ProcLine>,
    browser: Option<ProcLine>,
    #[cfg(windows)]
    job: sys::Job,
}

/// A start of a runner that did not give a runner whose line can be written.
pub(crate) enum SpawnFailed {
    /// No process was made.
    NotMade,
    /// A process was made and its line cannot be read; the caller ends it.
    Made(RunProc),
}

fn created(us: u64) -> Option<CreationTime> {
    NonZeroU64::new(us).map(CreationTime::from_micros)
}

/// Starts the runner as the leader of a process group of its own (macOS) or, suspended, in a job of the library's
/// own that it then lets run (Windows), and reads its line.
pub(crate) fn spawn(cmd: &mut Command) -> Result<RunProc, SpawnFailed> {
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
        let child = cmd.spawn().map_err(|_| SpawnFailed::NotMade)?;
        let pid = child.id();
        let line = match sys::info(pid) {
            sys::Read::Found(i) => created(i.started_us).and_then(|c| {
                Some(ProcLine {
                    pid: NonZeroU32::new(pid)?,
                    created: c,
                    group: Group::Leader(NonZeroU32::new(i.pgid).filter(|g| g.get() == pid)?),
                })
            }),
            _ => None,
        };
        let proc = RunProc {
            child,
            reaped: false,
            line,
            browser: None,
        };
        if proc.line.is_none() {
            return Err(SpawnFailed::Made(proc));
        }
        Ok(proc)
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use std::os::windows::process::CommandExt;
        let job = sys::Job::new().map_err(|_| SpawnFailed::NotMade)?;
        cmd.creation_flags(sys::CREATE_SUSPENDED);
        let child = cmd.spawn().map_err(|_| SpawnFailed::NotMade)?;
        let mut proc = RunProc {
            child,
            reaped: false,
            line: None,
            browser: None,
            job,
        };
        if proc.job.assign(proc.child.as_raw_handle()).is_err() {
            return Err(SpawnFailed::Made(proc));
        }
        let line = sys::created_of(proc.child.as_raw_handle())
            .and_then(created)
            .and_then(|c| {
                Some(ProcLine {
                    pid: NonZeroU32::new(proc.child.id())?,
                    created: c,
                    group: Group::Job,
                })
            });
        if line.is_none() || sys::resume(proc.child.id()).is_err() {
            return Err(SpawnFailed::Made(proc));
        }
        proc.line = line;
        Ok(proc)
    }
}

impl RunProc {
    /// The runner's line.
    pub(crate) fn line(&self) -> Option<ProcLine> {
        self.line
    }

    /// The two ends of the link.
    pub(crate) fn take_link(&mut self) -> Option<(ChildStdin, ChildStdout)> {
        Some((self.child.stdin.take()?, self.child.stdout.take()?))
    }

    /// The browser's line of a frame `browser`, when the frame names a process of this run: on macOS a direct child
    /// of the runner with the frame's creation time whose group is the frame's and is led by the browser or by the
    /// runner; on Windows a process of the run's job with the frame's creation time and no group.
    pub(crate) fn browser_line(&self, frame: &Browser) -> Option<ProcLine> {
        let runner = self.line?;
        #[cfg(target_os = "macos")]
        {
            let group = frame.group?;
            let sys::Read::Found(i) = sys::info(frame.pid.get()) else {
                return None;
            };
            let led = group == frame.pid || group == runner.pid;
            let ok = i.ppid == runner.pid.get()
                && i.started_us == frame.created.micros().get()
                && i.pgid == group.get()
                && !i.zombie
                && led;
            ok.then_some(ProcLine {
                pid: frame.pid,
                created: frame.created,
                group: Group::Leader(group),
            })
        }
        #[cfg(windows)]
        {
            let _ = runner;
            let ok = frame.group.is_none()
                && self
                    .job
                    .holds(frame.pid.get(), frame.created.micros().get());
            ok.then_some(ProcLine {
                pid: frame.pid,
                created: frame.created,
                group: Group::Job,
            })
        }
    }

    /// Keeps the browser's line.
    pub(crate) fn set_browser(&mut self, line: ProcLine) {
        self.browser = Some(line);
    }

    /// Whether the mark holds a browser's line.
    pub(crate) fn has_browser(&self) -> bool {
        self.browser.is_some()
    }

    /// Whether the runner has exited; reaps it when it has. On macOS a runner that exited and is not reaped still
    /// holds its number, so its group, with what the runner left in it, is ended before the reap.
    pub(crate) fn exited(&mut self) -> bool {
        if self.reaped {
            return true;
        }
        #[cfg(target_os = "macos")]
        match sys::child_state(self.child.id()) {
            sys::ChildState::Running => return false,
            sys::ChildState::Exited => {
                self.end_own_group();
                let _ = self.child.try_wait();
                self.reaped = true;
            }
            sys::ChildState::Gone => self.reaped = true,
        }
        #[cfg(windows)]
        if matches!(self.child.try_wait(), Ok(Some(_))) {
            self.reaped = true;
        }
        self.reaped
    }

    /// Ends the runner's group, whose number is the runner's own (`process_group(0)`), while the unreaped runner
    /// holds that number.
    #[cfg(target_os = "macos")]
    fn end_own_group(&self) {
        end_group(self.child.id());
    }

    /// Ends every process of the run that the library may end and waits for the runner. On macOS, while the
    /// runner runs, it stops the runner's group, reads the runner's living descendants, ends the runner's group and
    /// then each descendant outside it (its group when it leads one); while the runner has exited unreaped it ends
    /// the runner's group; and it ends the browser's group while a process of a line is in it. On Windows the job.
    pub(crate) fn end_all(&mut self) {
        #[cfg(target_os = "macos")]
        if !self.reaped {
            match sys::child_state(self.child.id()) {
                sys::ChildState::Running => {
                    let pid = self.child.id();
                    sys::stop_group(pid);
                    let created = match sys::info(pid) {
                        sys::Read::Found(i) => i.started_us,
                        _ => self.line.map_or(0, |l| l.created.micros().get()),
                    };
                    let tree = descendants(pid, created);
                    self.end_own_group();
                    for d in &tree {
                        end_descendant(d, pid);
                    }
                }
                sys::ChildState::Exited => self.end_own_group(),
                sys::ChildState::Gone => self.reaped = true,
            }
        }
        self.end_leftovers();
        if !self.reaped {
            let _ = self.child.kill();
            let _ = self.child.wait();
            self.reaped = true;
        }
    }

    /// Ends what is left of the run once the runner has exited: the browser's group on macOS, the job on Windows.
    pub(crate) fn end_leftovers(&mut self) {
        #[cfg(target_os = "macos")]
        if let Some(b) = self.browser {
            end_line_group(&b);
        }
        #[cfg(windows)]
        self.job.terminate();
    }

    /// Whether, after `end_all`, every process of the run that the library can see is gone within a bound: on
    /// macOS no process is left in the runner's group or in the browser's, on Windows the job holds none.
    pub(crate) fn all_gone(&self) -> bool {
        let deadline = Instant::now() + GONE_WAIT;
        loop {
            if self.nothing_left() {
                return true;
            }
            if Instant::now() > deadline {
                return false;
            }
            thread::sleep(POLL);
        }
    }

    fn nothing_left(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            let mut groups: Vec<u32> = self
                .browser
                .into_iter()
                .filter_map(|l| match l.group {
                    Group::Leader(g) => Some(g.get()),
                    Group::Job => None,
                })
                .collect();
            groups.push(self.child.id());
            sys::table().is_ok_and(|t| !t.iter().any(|p| !p.zombie && groups.contains(&p.pgid)))
        }
        #[cfg(windows)]
        {
            self.job.active_processes() == Some(0)
        }
    }
}

/// What a later life does with one mark: a mark with no runner's line is `gone`; else it ends the processes of
/// the mark that it may end and answers `gone` when the runner is gone, `alive` when it lives, with the members of
/// a group whose leader is gone, which it reports and does not end.
pub(crate) fn reap_mark(mark: &Mark) -> (Reaped, Vec<NonZeroU32>) {
    let Some(runner) = mark.runner else {
        return (Reaped::Gone, Vec::new());
    };
    #[cfg(target_os = "macos")]
    {
        let tree = if holds_group(&runner) {
            sys::stop_group(runner.pid.get());
            descendants(runner.pid.get(), runner.created.micros().get())
        } else {
            Vec::new()
        };
        let mut left = Vec::new();
        for line in [Some(runner), mark.browser].into_iter().flatten() {
            if !end_line_group(&line) && !lives(&line) {
                left.extend(members(&line));
            }
        }
        for d in &tree {
            end_descendant(d, runner.pid.get());
        }
        left.sort();
        left.dedup();
        let deadline = Instant::now() + GONE_WAIT;
        while lives(&runner) && Instant::now() < deadline {
            thread::sleep(POLL);
        }
        let answer = if lives(&runner) {
            Reaped::Alive
        } else {
            Reaped::Gone
        };
        (answer, left)
    }
    #[cfg(windows)]
    {
        let wait = u32::try_from(GONE_WAIT.as_millis()).unwrap_or(u32::MAX);
        if let Some(b) = mark.browser {
            sys::end(b.pid.get(), b.created.micros().get(), wait);
        }
        match sys::end(runner.pid.get(), runner.created.micros().get(), wait) {
            sys::End::Stays => (Reaped::Alive, Vec::new()),
            sys::End::Ended | sys::End::Gone => (Reaped::Gone, Vec::new()),
        }
    }
}

#[cfg(target_os = "macos")]
fn lives(line: &ProcLine) -> bool {
    matches!(
        sys::info(line.pid.get()),
        sys::Read::Found(i) if i.started_us == line.created.micros().get() && !i.zombie
    )
}

#[cfg(target_os = "macos")]
fn members(line: &ProcLine) -> Vec<NonZeroU32> {
    let Group::Leader(g) = line.group else {
        return Vec::new();
    };
    sys::table()
        .unwrap_or_default()
        .into_iter()
        .filter(|p| p.pgid == g.get() && !p.zombie)
        .filter_map(|p| NonZeroU32::new(p.pid))
        .collect()
}

/// Whether the process of a line, with the line's number and creation time, is in the line's group, read now.
#[cfg(target_os = "macos")]
fn holds_group(line: &ProcLine) -> bool {
    let Group::Leader(g) = line.group else {
        return false;
    };
    matches!(
        sys::info(line.pid.get()),
        sys::Read::Found(i) if i.started_us == line.created.micros().get() && i.pgid == g.get()
    )
}

/// Ends the group of a line only while the line's process, with the line's number and creation time, is in it,
/// read just before the signal: a group that holds a process of the mark keeps its number, so the signal reaches
/// that group alone, the browser in the runner's group among it.
#[cfg(target_os = "macos")]
pub(crate) fn end_line_group(line: &ProcLine) -> bool {
    match line.group {
        Group::Leader(g) if holds_group(line) => end_group(g.get()),
        _ => false,
    }
}

#[cfg(target_os = "macos")]
const GROUP_WAIT: Duration = Duration::from_secs(2);

/// Signals the group `g`, then reads it again and signals it again while a process that is no zombie is in it, until
/// none is or `GROUP_WAIT` passes: a process forked while the signal goes out joins the group after it. While a
/// member is left the group keeps its number, so each later signal reaches the same group.
#[cfg(target_os = "macos")]
fn end_group(g: u32) -> bool {
    if !sys::kill_group(g) {
        return false;
    }
    let deadline = Instant::now() + GROUP_WAIT;
    while Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
        let left = sys::table().is_ok_and(|t| t.iter().any(|p| p.pgid == g && !p.zombie));
        if !left || !sys::kill_group(g) {
            break;
        }
    }
    true
}

/// The living descendants of the process `root` created at `created_us`, parents before children: a process is a
/// child of a member when its parent's number is the member's and it was created at or after the member, as the
/// trial's `procs.rs` walks a tree.
#[cfg(target_os = "macos")]
fn descendants(root: u32, created_us: u64) -> Vec<sys::Info> {
    sys::table()
        .map(|table| descendants_in(&table, root, created_us))
        .unwrap_or_default()
}

/// The descendants in `table` of the process `root` created at `created_us`, parents before children.
#[cfg(target_os = "macos")]
fn descendants_in(table: &[sys::Info], root: u32, created_us: u64) -> Vec<sys::Info> {
    let mut tree: Vec<(u32, u64)> = vec![(root, created_us)];
    let mut found = Vec::new();
    let mut i = 0;
    while i < tree.len() {
        let (parent, since) = tree[i];
        i += 1;
        for p in table {
            if p.ppid == parent
                && p.pid != parent
                && p.started_us >= since
                && !tree.iter().any(|(t, _)| *t == p.pid)
            {
                tree.push((p.pid, p.started_us));
                found.push(*p);
            }
        }
    }
    found
}

/// Ends one descendant of the runner read before the runner's group was ended, after reading it again: nothing
/// when it is gone, has another creation time or is in the runner's group; its group when it leads one; else the
/// process alone.
#[cfg(target_os = "macos")]
fn end_descendant(d: &sys::Info, runner_group: u32) {
    let sys::Read::Found(now) = sys::info(d.pid) else {
        return;
    };
    if now.started_us != d.started_us || now.pgid == runner_group {
        return;
    }
    if now.pgid == d.pid {
        end_group(d.pid);
    } else {
        sys::kill_pid(d.pid);
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    fn p(pid: u32, ppid: u32, started_us: u64) -> sys::Info {
        sys::Info {
            pid,
            ppid,
            pgid: pid,
            started_us,
            zombie: false,
        }
    }

    fn pids(found: &[sys::Info]) -> Vec<u32> {
        let mut v: Vec<u32> = found.iter().map(|i| i.pid).collect();
        v.sort_unstable();
        v
    }

    #[test]
    fn the_walk_takes_every_generation_below_the_runner_and_nothing_beside_it() {
        let table = vec![
            p(1, 0, 1),
            p(10, 1, 100),
            p(11, 10, 101),
            p(12, 11, 102),
            p(13, 12, 103),
            p(20, 1, 104),
            p(21, 20, 105),
        ];
        assert_eq!(pids(&descendants_in(&table, 10, 100)), vec![11, 12, 13]);
    }

    #[test]
    fn the_walk_takes_no_process_created_before_the_parent_that_its_parent_number_names() {
        let table = vec![
            p(10, 1, 100),
            p(11, 10, 101),
            p(30, 10, 50),
            p(31, 30, 60),
            p(32, 11, 90),
        ];
        assert_eq!(pids(&descendants_in(&table, 10, 100)), vec![11]);
    }
}
