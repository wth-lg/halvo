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

    #[cfg(target_os = "macos")]
    fn end_own_group(&self) {
        if let Some(Group::Leader(g)) = self.line.map(|l| l.group) {
            sys::kill_group(g.get());
        }
    }

    /// Ends every process of the run that the library may end and waits for the runner: on macOS the runner's
    /// group only while the runner, running or exited and not reaped, still holds its number, and the browser's
    /// group while its leader has the number, creation time and group of its line; on Windows the job.
    pub(crate) fn end_all(&mut self) {
        #[cfg(target_os = "macos")]
        if !self.reaped {
            match sys::child_state(self.child.id()) {
                sys::ChildState::Running | sys::ChildState::Exited => self.end_own_group(),
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
            end_led_group(&b);
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
            let groups: Vec<u32> = [self.line, self.browser]
                .into_iter()
                .flatten()
                .filter_map(|l| match l.group {
                    Group::Leader(g) => Some(g.get()),
                    Group::Job => None,
                })
                .collect();
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
        let mut left = Vec::new();
        for line in [Some(runner), mark.browser].into_iter().flatten() {
            if !end_led_group(&line) && !lives(&line) {
                left.extend(members(&line));
            }
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
    if g != line.pid {
        return Vec::new();
    }
    sys::table()
        .unwrap_or_default()
        .into_iter()
        .filter(|p| p.pgid == g.get() && !p.zombie)
        .filter_map(|p| NonZeroU32::new(p.pid))
        .collect()
}

/// Signals the group of a line whose process leads it, only while that process still has the line's number,
/// creation time and group, read just before the signal.
#[cfg(target_os = "macos")]
pub(crate) fn end_led_group(line: &ProcLine) -> bool {
    let Group::Leader(g) = line.group else {
        return false;
    };
    if g != line.pid {
        return false;
    }
    match sys::info(line.pid.get()) {
        sys::Read::Found(i) if i.started_us == line.created.micros().get() && i.pgid == g.get() => {
            sys::kill_group(g.get())
        }
        _ => false,
    }
}
