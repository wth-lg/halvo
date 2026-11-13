use std::ffi::OsString;
use std::num::{NonZeroU32, NonZeroU64};
#[cfg(target_os = "macos")]
use std::process::Child;
use std::process::{ChildStdin, ChildStdout, Command};
use std::thread;
use std::time::{Duration, Instant};

use halvo_schema::executable::frame::Browser;
use halvo_schema::executable::text::CreationTime;

use crate::api::{Reaped, RunnerExit};
use crate::marks::{Group, Mark, ProcLine};
use crate::sys;

const GONE_WAIT: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(20);

/// The runner that this life made: its child, which stays unreaped until the library has done with its group, the
/// facts of its mark's lines, and on Windows the job that holds it.
pub(crate) struct RunProc {
    #[cfg(target_os = "macos")]
    child: Child,
    #[cfg(windows)]
    child: sys::Process,
    #[cfg(windows)]
    link: Option<(ChildStdin, ChildStdout)>,
    reaped: bool,
    exit: Option<RunnerExit>,
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

/// The status that the runner exited with, or the signal that ended it.
#[cfg(target_os = "macos")]
fn runner_exit(status: std::process::ExitStatus) -> Option<RunnerExit> {
    use std::os::unix::process::ExitStatusExt;
    status
        .code()
        .map(RunnerExit::Status)
        .or_else(|| status.signal().map(RunnerExit::Signal))
}

/// The exit code of the runner, as the status of a C runtime reads it.
#[cfg(windows)]
fn runner_exit(code: u32) -> Option<RunnerExit> {
    Some(RunnerExit::Status(i32::from_ne_bytes(code.to_ne_bytes())))
}

fn created(us: u64) -> Option<CreationTime> {
    NonZeroU64::new(us).map(CreationTime::from_micros)
}

/// The whole environment of a runner, from the closed list of its system (contracts.md A6.1 and A6.6, ruling CP-160
/// (1) (b)); nothing else of the library's process reaches the runner. On macOS `HOME`, as the library's process has
/// it, and `TMPDIR`, the per-user temporary directory that the system answers; on Windows `SystemRoot`, the Windows
/// directory that the system answers, `TEMP` and `TMP`, the directory Temp under the account's LocalAppData known
/// folder, and `LOCALAPPDATA`, as the library's process has it. `HOME` and `LOCALAPPDATA` are left out where the
/// library's process has none, and a value of the system that cannot be read leaves its name out.
pub(crate) fn environment() -> Vec<(OsString, OsString)> {
    let mut env = Vec::new();
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            env.push(("HOME".into(), home));
        }
        if let Some(temp) = sys::user_temp_dir() {
            env.push(("TMPDIR".into(), temp));
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        if let Some(root) = sys::windows_directory() {
            env.push(("SystemRoot".into(), OsString::from_wide(&root)));
        }
        if let Some(local) = sys::local_app_data() {
            let mut temp = OsString::from_wide(&local);
            temp.push("\\Temp");
            env.push(("TEMP".into(), temp.clone()));
            env.push(("TMP".into(), temp));
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            env.push(("LOCALAPPDATA".into(), local));
        }
    }
    env
}

/// Starts the runner of `cmd`, its program and arguments, with three handles or descriptors of the library's open in
/// it and no other: the two ends of the link as its standard input and output and the null device as its standard
/// error. It gives the runner the whole environment of `environment` and the system's directory as its working
/// directory: `/` on macOS, the system directory on Windows, never the library's own; a Windows start whose system
/// directory cannot be read makes no process. On macOS the runner is the leader of a process group of its own and
/// every other descriptor closes at its exec; on Windows it starts suspended, with the list of those three as the
/// only handles that it inherits, in a job of the library's own that then lets it run. Reads its line.
pub(crate) fn spawn(cmd: &mut Command) -> Result<RunProc, SpawnFailed> {
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::process::CommandExt;
        use std::process::Stdio;
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .env_clear()
            .envs(environment())
            .current_dir("/")
            .process_group(0);
        sys::close_beyond_three(cmd);
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
            exit: None,
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
        use std::os::windows::ffi::OsStrExt;
        let program: Vec<u16> = cmd.get_program().encode_wide().collect();
        let args: Vec<Vec<u16>> = cmd.get_args().map(|a| a.encode_wide().collect()).collect();
        let line =
            command_line(&program, args.iter().map(Vec::as_slice)).ok_or(SpawnFailed::NotMade)?;
        let application: Vec<u16> = program.into_iter().chain(Some(0)).collect();
        let vars: Vec<(Vec<u16>, Vec<u16>)> = environment()
            .iter()
            .map(|(k, v)| (k.encode_wide().collect(), v.encode_wide().collect()))
            .collect();
        let block = environment_block(&vars).ok_or(SpawnFailed::NotMade)?;
        let directory: Vec<u16> = sys::system_directory()
            .ok_or(SpawnFailed::NotMade)?
            .into_iter()
            .chain(Some(0))
            .collect();
        let job = sys::Job::new().map_err(|_| SpawnFailed::NotMade)?;
        let sys::Started {
            process,
            thread,
            stdin,
            stdout,
        } = sys::start(&application, line, Some(&block), Some(&directory))
            .map_err(|_| SpawnFailed::NotMade)?;
        let mut proc = RunProc {
            child: process,
            link: Some((ChildStdin::from(stdin), ChildStdout::from(stdout))),
            reaped: false,
            exit: None,
            line: None,
            browser: None,
            job,
        };
        if proc.job.assign(proc.child.raw()).is_err() {
            return Err(SpawnFailed::Made(proc));
        }
        let line = sys::created_of(proc.child.raw())
            .and_then(created)
            .and_then(|c| {
                Some(ProcLine {
                    pid: NonZeroU32::new(proc.child.id())?,
                    created: c,
                    group: Group::Job,
                })
            });
        if line.is_none() || sys::resume(&thread).is_err() {
            return Err(SpawnFailed::Made(proc));
        }
        proc.line = line;
        Ok(proc)
    }
}

/// The environment block of a Windows start: each `name=value` ending in a NUL, the names in the order of their
/// upper case, and one NUL more at the end. None for an empty name, a name that holds `=`, a NUL in any part, or
/// two names that are equal but for case.
#[cfg(any(windows, test))]
fn environment_block(vars: &[(Vec<u16>, Vec<u16>)]) -> Option<Vec<u16>> {
    const EQUALS: u16 = b'=' as u16;
    let upper = |name: &[u16]| -> Vec<u16> {
        name.iter()
            .map(|&c| {
                if (u16::from(b'a')..=u16::from(b'z')).contains(&c) {
                    c - 32
                } else {
                    c
                }
            })
            .collect()
    };
    let mut sorted: Vec<&(Vec<u16>, Vec<u16>)> = vars.iter().collect();
    sorted.sort_by_key(|(name, _)| upper(name));
    if sorted.windows(2).any(|w| upper(&w[0].0) == upper(&w[1].0)) {
        return None;
    }
    let mut block = Vec::new();
    for (name, value) in sorted {
        if name.is_empty() || name.contains(&EQUALS) || name.contains(&0) || value.contains(&0) {
            return None;
        }
        block.extend_from_slice(name);
        block.push(EQUALS);
        block.extend_from_slice(value);
        block.push(0);
    }
    if block.is_empty() {
        block.push(0);
    }
    block.push(0);
    Some(block)
}

/// The command line of a Windows start: the program in quotes, then each argument, in quotes when it is empty or
/// holds a space or a tab, with the backslashes before a quote doubled and the quote escaped, as the C runtime
/// reads a command line back into its arguments; ends in a NUL. None for a NUL in any part or a quote in the
/// program.
#[cfg(any(windows, test))]
fn command_line<'a>(program: &[u16], args: impl Iterator<Item = &'a [u16]>) -> Option<Vec<u16>> {
    const QUOTE: u16 = b'"' as u16;
    const BACKSLASH: u16 = b'\\' as u16;
    const SPACE: u16 = b' ' as u16;
    const TAB: u16 = b'\t' as u16;
    if program.contains(&0) || program.contains(&QUOTE) {
        return None;
    }
    let mut line = vec![QUOTE];
    line.extend_from_slice(program);
    line.push(QUOTE);
    for arg in args {
        if arg.contains(&0) {
            return None;
        }
        line.push(SPACE);
        let quote = arg.is_empty() || arg.contains(&SPACE) || arg.contains(&TAB);
        if quote {
            line.push(QUOTE);
        }
        let mut backslashes = 0;
        for &c in arg {
            if c == BACKSLASH {
                backslashes += 1;
            } else {
                if c == QUOTE {
                    line.extend(std::iter::repeat_n(BACKSLASH, backslashes + 1));
                }
                backslashes = 0;
            }
            line.push(c);
        }
        if quote {
            line.extend(std::iter::repeat_n(BACKSLASH, backslashes));
            line.push(QUOTE);
        }
    }
    line.push(0);
    Some(line)
}

impl RunProc {
    /// The runner's line.
    pub(crate) fn line(&self) -> Option<ProcLine> {
        self.line
    }

    /// How the runner ended, once the library has reaped it; none before, and none when the system reaped it.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "RunEnd and the start's answer carry it from the next unit of src/supervisor.rs (U4.9's report)"
        )
    )]
    pub(crate) fn exit(&self) -> Option<RunnerExit> {
        self.exit
    }

    /// The two ends of the link.
    pub(crate) fn take_link(&mut self) -> Option<(ChildStdin, ChildStdout)> {
        #[cfg(target_os = "macos")]
        {
            Some((self.child.stdin.take()?, self.child.stdout.take()?))
        }
        #[cfg(windows)]
        {
            self.link.take()
        }
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
                self.exit = self.child.try_wait().ok().flatten().and_then(runner_exit);
                self.reaped = true;
            }
            sys::ChildState::Gone => self.reaped = true,
        }
        #[cfg(windows)]
        if let Some(code) = self.child.try_wait() {
            self.exit = runner_exit(code);
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
            #[cfg(target_os = "macos")]
            {
                self.exit = self.child.wait().ok().and_then(runner_exit);
            }
            #[cfg(windows)]
            {
                self.exit = self.child.wait().and_then(runner_exit);
            }
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

#[cfg(test)]
mod exit_tests {
    use super::*;

    struct Ends(RunProc);

    impl Drop for Ends {
        fn drop(&mut self) {
            self.0.end_all();
        }
    }

    fn started(mut cmd: Command) -> Ends {
        match spawn(&mut cmd) {
            Ok(p) => Ends(p),
            Err(SpawnFailed::Made(p)) => {
                drop(Ends(p));
                panic!("the runner was made and its line could not be read");
            }
            Err(SpawnFailed::NotMade) => panic!("no runner was made"),
        }
    }

    #[cfg(target_os = "macos")]
    fn exits_with_two() -> Command {
        let mut c = Command::new("/bin/sh");
        c.args(["-c", "exit 2"]);
        c
    }

    #[cfg(target_os = "macos")]
    fn waits() -> Command {
        let mut c = Command::new("/bin/sleep");
        c.arg("60");
        c
    }

    #[cfg(windows)]
    fn shell(args: &[&str]) -> Command {
        use std::os::windows::ffi::OsStringExt;
        let mut path = std::path::PathBuf::from(OsString::from_wide(
            &sys::system_directory().expect("the system directory"),
        ));
        path.push("cmd.exe");
        let mut c = Command::new(path);
        c.args(args);
        c
    }

    #[cfg(windows)]
    fn exits_with_two() -> Command {
        shell(&["/d", "/c", "exit 2"])
    }

    #[cfg(windows)]
    fn waits() -> Command {
        shell(&["/d", "/k"])
    }

    #[test]
    fn a_runner_that_exits_with_the_status_2_gives_a_refusal_and_a_runner_that_the_library_killed_does_not(
    ) {
        let mut refused = started(exits_with_two());
        let deadline = Instant::now() + Duration::from_secs(10);
        while !refused.0.exited() && Instant::now() < deadline {
            thread::sleep(POLL);
        }
        assert_eq!(
            refused.0.exit(),
            Some(RunnerExit::Status(2)),
            "how a runner that exited with the status 2 ended"
        );
        assert!(refused.0.exit().is_some_and(RunnerExit::is_refusal));
        let mut killed = started(waits());
        assert!(!killed.0.exited(), "the waiting runner runs");
        killed.0.end_all();
        let ended = killed.0.exit();
        #[cfg(target_os = "macos")]
        assert_eq!(
            ended,
            Some(RunnerExit::Signal(9)),
            "how a runner that the library killed ended"
        );
        #[cfg(windows)]
        assert_eq!(
            ended,
            Some(RunnerExit::Status(1)),
            "how a runner that the library killed ended"
        );
        assert!(
            !ended.is_some_and(RunnerExit::is_refusal),
            "a runner that the library killed reads as a refusal"
        );
    }
}

#[cfg(test)]
mod environment_block_tests {
    use super::environment_block;

    fn block(vars: &[(&str, &str)]) -> Option<String> {
        let vars: Vec<(Vec<u16>, Vec<u16>)> = vars
            .iter()
            .map(|(k, v)| (k.encode_utf16().collect(), v.encode_utf16().collect()))
            .collect();
        environment_block(&vars).map(|b| String::from_utf16(&b).unwrap())
    }

    #[test]
    fn the_block_holds_each_variable_in_the_order_of_the_upper_case_of_its_name_and_ends_in_two_nuls(
    ) {
        assert_eq!(
            block(&[
                ("TMP", "b"),
                ("SystemRoot", "r"),
                ("TEMP", "a"),
                ("LOCALAPPDATA", "l")
            ])
            .as_deref(),
            Some("LOCALAPPDATA=l\0SystemRoot=r\0TEMP=a\0TMP=b\0\0")
        );
        assert_eq!(
            block(&[("b", "1"), ("A", "2")]).as_deref(),
            Some("A=2\0b=1\0\0")
        );
        assert_eq!(
            block(&[("B", "1"), ("a", "2")]).as_deref(),
            Some("a=2\0B=1\0\0")
        );
    }

    #[test]
    fn an_empty_block_is_two_nuls() {
        assert_eq!(block(&[]).as_deref(), Some("\0\0"));
    }

    #[test]
    fn an_empty_name_a_name_with_an_equals_sign_or_a_nul_gives_no_block() {
        assert_eq!(block(&[("", "x")]), None);
        assert_eq!(block(&[("A=B", "x")]), None);
        assert_eq!(block(&[("A\0", "x")]), None);
        assert_eq!(block(&[("A", "x\0y")]), None);
        assert_eq!(block(&[("Temp", "x"), ("TEMP", "y")]), None);
    }
}

#[cfg(test)]
mod command_line_tests {
    use super::command_line;

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn line(program: &str, args: &[&str]) -> Option<String> {
        let args: Vec<Vec<u16>> = args.iter().map(|a| w(a)).collect();
        let line = command_line(&w(program), args.iter().map(Vec::as_slice))?;
        assert_eq!(line.last(), Some(&0), "the command line ends in a NUL");
        Some(String::from_utf16(&line[..line.len() - 1]).unwrap())
    }

    #[test]
    fn the_program_is_quoted_and_a_plain_argument_is_not() {
        assert_eq!(
            line(r"C:\a b\runner.exe", &["--run", "abc"]).as_deref(),
            Some(r#""C:\a b\runner.exe" --run abc"#)
        );
    }

    #[test]
    fn an_empty_argument_and_one_with_a_space_or_a_tab_are_quoted() {
        assert_eq!(
            line("p", &["", "a b", "a\tb"]).as_deref(),
            Some("\"p\" \"\" \"a b\" \"a\tb\"")
        );
    }

    #[test]
    fn a_quote_is_escaped_and_the_backslashes_before_it_are_doubled() {
        assert_eq!(
            line("p", &[r#"x"y"#, r#"x\"y"#, r"a\\b"]).as_deref(),
            Some(r#""p" x\"y x\\\"y a\\b"#)
        );
    }

    #[test]
    fn the_backslashes_before_the_closing_quote_of_a_quoted_argument_are_doubled() {
        assert_eq!(
            line("p", &[r"C:\a b\"]).as_deref(),
            Some(r#""p" "C:\a b\\""#)
        );
    }

    #[test]
    fn a_nul_in_any_part_or_a_quote_in_the_program_gives_no_command_line() {
        assert_eq!(line("p\0", &[]), None);
        assert_eq!(line("p", &["a\0b"]), None);
        assert_eq!(line("p\"q", &[]), None);
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
