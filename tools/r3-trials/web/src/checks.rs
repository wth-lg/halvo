//! The checks that the executor keeps from the start of a browser to its stop (A11.1, the
//! coordinator's rulings CP-89 (c) and CP-93): each reads one rule from the system and gives the
//! number of things that break it, which is 0 when the rule holds.

use std::path::Path;

use crate::peer::{self, Proto};
use crate::procs::{self, Proc};
use crate::start::PipeIds;

/// The rules, in the order in which a check prints them.
pub const RULES: [&str; 6] = [
    "port_flag",
    "tcp_listen",
    "udp_wide",
    "active_port_file",
    "executor_listener",
    "pipe_holder",
];

/// What a check reads: the browser's first process, the profile, the identity of its pipe (none
/// on a port), and the relay's port, which is the one listener that the executor's process may
/// hold.
pub struct Subject<'a> {
    pub root: &'a Proc,
    pub profile: &'a Path,
    pub pipe: Option<&'a PipeIds>,
    pub relay_port: Option<u16>,
}

/// One rule's count.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Count {
    pub rule: &'static str,
    pub count: usize,
}

/// The browser's tree as the process table reads it now: the first process with every descendant
/// by creation time, and the processes that the stop reports as adopted (A11.1) whose arguments
/// name the first process's profile directory, as the crash handler's `--database=` does; an
/// adopted process of another browser of the same app, such as the person's own, is no member.
pub fn tree_of(table: &[Proc], root: &Proc) -> Vec<Proc> {
    tree_of_with(table, root, &command_args)
}

/// `tree_of` with the reading of a process's arguments given, for tests.
pub fn tree_of_with(
    table: &[Proc],
    root: &Proc,
    command_args: &dyn Fn(u32) -> Option<Vec<String>>,
) -> Vec<Proc> {
    let mut tree = procs::descendants(table, std::slice::from_ref(root));
    tree.retain(|p| table.iter().any(|t| procs::same(t, p)));
    let roots: Vec<std::path::PathBuf> = root
        .exe
        .as_deref()
        .map(procs::bundle_root)
        .into_iter()
        .collect();
    let profile = command_args(root.pid).and_then(|args| {
        args.iter()
            .find_map(|a| a.strip_prefix("--user-data-dir=").map(str::to_string))
    });
    let Some(profile) = profile.filter(|p| !p.is_empty()) else {
        return tree;
    };
    let adopted = procs::adopted(table, &tree, root.started_us, &roots);
    for a in adopted {
        let ours =
            command_args(a.pid).is_some_and(|args| args.iter().any(|x| names_profile(x, &profile)));
        if ours && !tree.iter().any(|t| procs::same(t, &a)) {
            tree.push(a);
        }
    }
    tree
}

/// Whether `text` names `profile` as a whole path: an occurrence that the end of the text, a path
/// separator, a quote or a space follows, so a profile whose name begins another's names only its
/// own processes.
pub fn names_profile(text: &str, profile: &str) -> bool {
    !profile.is_empty()
        && text.match_indices(profile).any(|(i, _)| {
            matches!(
                text[i + profile.len()..].chars().next(),
                None | Some('/' | '\\' | '"' | ' ')
            )
        })
}

/// What makes a process a run's own (CP-126 E1 (b)): it began at or after `since_us`, the run's
/// start less the margin of the clock; its program lies inside `root`, the app bundle of the
/// browser that the run started (macOS) or that browser's install directory (Windows); and one of
/// its arguments names `profile`, the run's profile directory, as a whole path (`names_profile`).
/// The egress check's shell, its runner and its `log stream` run other programs, and a browser of
/// another worker or of the person names another profile: none of them is the run's own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mark {
    pub profile: String,
    pub since_us: u64,
    pub root: std::path::PathBuf,
}

impl Mark {
    /// The mark of a run of `program` (the browser itself, not a wrapper that starts it) with
    /// `profile`, begun at `since_us`. On macOS the program's path is read through its links, as the
    /// process table gives an executable's path.
    pub fn new(profile: &Path, program: &Path, since_us: u64) -> Mark {
        let program = if cfg!(target_os = "macos") {
            std::fs::canonicalize(program).unwrap_or_else(|_| program.to_path_buf())
        } else {
            program.to_path_buf()
        };
        Mark {
            profile: profile.to_string_lossy().into_owned(),
            since_us,
            root: procs::bundle_root(&program),
        }
    }

    /// Whether `p` is the run's own, with `args` reading a process's arguments.
    pub fn owns(&self, p: &Proc, args: &dyn Fn(u32) -> Option<Vec<String>>) -> bool {
        p.started_us >= self.since_us
            && p.pid != std::process::id()
            && p.exe.as_deref().is_some_and(|e| inside(e, &self.root))
            && args(p.pid).is_some_and(|a| a.iter().any(|x| names_profile(x, &self.profile)))
    }
}

/// Whether `exe` lies inside `root`: by path components on macOS, and on Windows with `/` and `\`
/// taken as one and the letter case ignored, as the system compares paths there.
fn inside(exe: &Path, root: &Path) -> bool {
    if root.as_os_str().is_empty() {
        return false;
    }
    if cfg!(windows) {
        let norm = |p: &Path| p.to_string_lossy().replace('\\', "/").to_lowercase();
        let root = norm(root);
        norm(exe).starts_with(&format!("{}/", root.trim_end_matches('/')))
    } else {
        exe.starts_with(root)
    }
}

/// What one end of a run's own processes found and left (CP-126 E1 (d)): `before`, what its first
/// pass found before it ended anything; `after`, what its last pass found (0 when it stopped on a
/// pass that found none); `ended`, how many processes it recorded and sent the end to; `passes`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ended {
    pub before: usize,
    pub after: usize,
    pub ended: usize,
    pub passes: usize,
}

impl Ended {
    /// Two ends of one run as one: the first one's `before`, the last one's `after`.
    pub fn then(self, later: Ended) -> Ended {
        Ended {
            before: self.before,
            after: later.after,
            ended: self.ended + later.ended,
            passes: self.passes + later.passes,
        }
    }
}

/// The most passes over the process table that one end of a run's own processes makes.
pub const END_PASSES: usize = 100;
/// The pause after a pass that sent an end, so that the system can take the ended processes out of
/// its table before the next pass reads it.
const END_PAUSE: std::time::Duration = std::time::Duration::from_millis(20);

/// Ends what a run left (CP-126 E1): each pass reads the process table and takes every running
/// process that is the run's own (`Mark::owns`); each one is recorded by number and creation time
/// and ended by `procs::kill`, which reads its creation time again just before the kill and leaves
/// a number that another process now has alone. The routine ends on a pass that finds none: only a
/// process that names the profile can make another one that names it. A table that cannot be read
/// counts as a pass that found one. After `END_PASSES` it stops with what its last pass found.
pub fn end_runs_own(mark: &Mark) -> Ended {
    end_runs_own_with(
        mark,
        &procs::table,
        &command_args,
        &procs::running,
        &procs::kill,
    )
}

/// `end_runs_own` with the table, the arguments, the running test and the kill given, for tests.
pub fn end_runs_own_with(
    mark: &Mark,
    table: &dyn Fn() -> std::io::Result<Vec<Proc>>,
    args: &dyn Fn(u32) -> Option<Vec<String>>,
    running: &dyn Fn(&Proc) -> bool,
    kill: &dyn Fn(&Proc) -> bool,
) -> Ended {
    let mut out = Ended::default();
    let mut recorded: Vec<Proc> = Vec::new();
    for pass in 0..END_PASSES {
        out.passes = pass + 1;
        let found: Option<Vec<Proc>> = table().ok().map(|t| {
            t.into_iter()
                .filter(|p| mark.owns(p, args) && running(p))
                .collect()
        });
        let n = found.as_ref().map_or(1, Vec::len);
        if pass == 0 {
            out.before = n;
        }
        out.after = n;
        if n == 0 {
            break;
        }
        for p in found.unwrap_or_default() {
            if !recorded.iter().any(|r| procs::same(r, &p)) {
                recorded.push(p.clone());
            }
            kill(&p);
        }
        std::thread::sleep(END_PAUSE);
    }
    out.ended = recorded.len();
    out
}

/// Whether a command line names a debugging port.
pub fn names_a_port(command_line: &str) -> bool {
    command_line.contains("--remote-debugging-port")
}

/// Reads every rule once. A rule whose read the system refused counts each unread process as one.
pub fn check(s: &Subject<'_>) -> Vec<Count> {
    check_with(s, &command_line)
}

/// `check` with the reading of a process's command line given, for tests.
pub fn check_with(s: &Subject<'_>, line_of: &dyn Fn(u32) -> Option<String>) -> Vec<Count> {
    let table = procs::table().unwrap_or_default();
    let tree = tree_of(&table, s.root);
    let pids: Vec<u32> = tree.iter().map(|p| p.pid).collect();
    let mut port_flag = 0;
    for p in &tree {
        match line_of(p.pid) {
            Some(line) if names_a_port(&line) => port_flag += 1,
            Some(_) => {}
            None if procs::running(p) => port_flag += 1,
            None => {}
        }
    }
    let sockets = peer::sockets(&pids);
    let mine: Vec<_> = sockets.iter().filter(|x| pids.contains(&x.pid)).collect();
    let tcp_listen = mine
        .iter()
        .filter(|x| x.proto == Proto::Tcp && x.listening)
        .count();
    let udp_wide = mine
        .iter()
        .filter(|x| x.proto == Proto::Udp && !x.local.0.is_loopback())
        .count();
    let active_port_file = usize::from(s.profile.join("DevToolsActivePort").exists());
    let me = std::process::id();
    let executor_listener = listeners_beside(&peer::sockets(&[me]), me, s.relay_port);
    let pipe_holder = s
        .pipe
        .map(|ids| pipe_holders(&table, s.root, ids))
        .unwrap_or(0);
    vec![
        Count {
            rule: "port_flag",
            count: port_flag,
        },
        Count {
            rule: "tcp_listen",
            count: tcp_listen,
        },
        Count {
            rule: "udp_wide",
            count: udp_wide,
        },
        Count {
            rule: "active_port_file",
            count: active_port_file,
        },
        Count {
            rule: "executor_listener",
            count: executor_listener,
        },
        Count {
            rule: "pipe_holder",
            count: pipe_holder,
        },
    ]
}

/// The TCP listeners of process `pid` among `sockets` but the one on `relay_port`.
pub fn listeners_beside(sockets: &[peer::Socket], pid: u32, relay_port: Option<u16>) -> usize {
    sockets
        .iter()
        .filter(|x| x.pid == pid && x.proto == Proto::Tcp && x.listening)
        .filter(|x| Some(x.local.1) != relay_port)
        .count()
}

/// The processes but the browser's first one and this process that hold an end of the pipe: on
/// macOS every process whose descriptors this user may read, so that an end passed over a socket
/// to an older process is found too; on Windows the processes that started at or after the
/// browser's first process, which alone can have inherited an end (a handle that a holder copies
/// into an older process with `DuplicateHandle` is not read there).
pub fn pipe_holders(table: &[Proc], root: &Proc, ids: &PipeIds) -> usize {
    let me = std::process::id();
    table
        .iter()
        .filter(|p| p.pid != me && !procs::same(p, root))
        .filter(|p| {
            cfg!(target_os = "macos") || p.started_us == 0 || p.started_us >= root.started_us
        })
        .filter(|p| holds_an_end(p.pid, ids))
        .count()
}

/// Whether process `pid` holds an end of the pipe `ids`.
pub fn holds_an_end(pid: u32, ids: &PipeIds) -> bool {
    sys::holds_an_end(pid, ids)
}

/// The command line of `pid`, as the system gives it to a process of the same user.
pub fn command_line(pid: u32) -> Option<String> {
    sys::command_args(pid).map(|a| a.join(" "))
}

/// The arguments of `pid`, as the system gives them to a process of the same user; on Windows the
/// command line as one argument.
pub fn command_args(pid: u32) -> Option<Vec<String>> {
    sys::command_args(pid)
}

#[cfg(target_os = "macos")]
mod sys {
    use crate::start::PipeIds;

    pub(super) fn holds_an_end(pid: u32, ids: &PipeIds) -> bool {
        crate::peer::pipe_handles_of(pid)
            .map(|held| held.iter().any(|(_, h)| *h != 0 && ids.handles.contains(h)))
            .unwrap_or(false)
    }

    pub(super) fn command_args(pid: u32) -> Option<Vec<String>> {
        let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid as i32];
        let mut size: libc::size_t = 0;
        // SAFETY: a null buffer asks for the size of the arguments area.
        if unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                3,
                std::ptr::null_mut(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        } != 0
        {
            return None;
        }
        let mut buf = vec![0u8; size];
        // SAFETY: the buffer holds `size` bytes, which the call is told.
        if unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                3,
                buf.as_mut_ptr() as *mut libc::c_void,
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        } != 0
        {
            return None;
        }
        buf.truncate(size);
        if buf.len() < 4 {
            return None;
        }
        let argc = i32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]).max(0) as usize;
        let rest = &buf[4..];
        let exe_end = rest.iter().position(|b| *b == 0)?;
        let mut at = exe_end;
        while at < rest.len() && rest[at] == 0 {
            at += 1;
        }
        let args: Vec<String> = rest[at..]
            .split(|b| *b == 0)
            .take(argc)
            .map(|a| String::from_utf8_lossy(a).into_owned())
            .collect();
        Some(args)
    }
}

#[cfg(windows)]
mod sys {
    use crate::start::PipeIds;
    use windows_sys::Wdk::System::Threading::{
        NtQueryInformationProcess, ProcessCommandLineInformation,
    };
    use windows_sys::Win32::Foundation::{CloseHandle, UNICODE_STRING};
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    /// The handle values that a process of this system may hold: every multiple of 4 below the
    /// bound, the form of a handle value of a process.
    const VALUES: usize = 16384;

    pub(super) fn holds_an_end(pid: u32, ids: &PipeIds) -> bool {
        let values: Vec<usize> = (1..VALUES).map(|i| i * 4).collect();
        crate::peer::pipe_holders(pid, &values, &ids.handles)
            .map(|found| !found.is_empty())
            .unwrap_or(false)
    }

    pub(super) fn command_args(pid: u32) -> Option<Vec<String>> {
        // SAFETY: OpenProcess returns a new handle or null.
        let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if h.is_null() {
            return None;
        }
        let mut buf = vec![0u64; 4096];
        let mut got = 0u32;
        // SAFETY: the buffer holds `buf.len() * 8` bytes, which the call is told; it starts with a
        // UNICODE_STRING whose buffer points into the same allocation.
        let status = unsafe {
            NtQueryInformationProcess(
                h,
                ProcessCommandLineInformation,
                buf.as_mut_ptr() as *mut core::ffi::c_void,
                (buf.len() * 8) as u32,
                &mut got,
            )
        };
        // SAFETY: the handle is owned here and closed once.
        unsafe { CloseHandle(h) };
        if status < 0 {
            return None;
        }
        // SAFETY: on success the buffer starts with a UNICODE_STRING whose units lie inside it.
        let text = unsafe {
            let us = &*(buf.as_ptr() as *const UNICODE_STRING);
            std::slice::from_raw_parts(us.Buffer, us.Length as usize / 2)
        };
        Some(vec![String::from_utf16_lossy(text)])
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod sys {
    use crate::start::PipeIds;
    pub(super) fn holds_an_end(_pid: u32, _ids: &PipeIds) -> bool {
        false
    }
    pub(super) fn command_args(_pid: u32) -> Option<Vec<String>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_line_with_a_debugging_port_is_named_and_the_pipe_is_not() {
        assert!(names_a_port("chrome --remote-debugging-port=0 about:blank"));
        assert!(!names_a_port("chrome --remote-debugging-pipe about:blank"));
        assert_eq!(RULES.len(), 6);
    }

    #[test]
    fn a_profile_is_named_as_a_whole_path() {
        assert!(names_profile("--user-data-dir=/t/p-1", "/t/p-1"));
        assert!(names_profile("--database=/t/p-1/Crashpad", "/t/p-1"));
        assert!(names_profile(
            r#"chrome.exe "--user-data-dir=C:\t\p-1" --type=gpu"#,
            r"C:\t\p-1"
        ));
        assert!(names_profile(
            r"chrome.exe --user-data-dir=C:\t\p-1 x",
            r"C:\t\p-1"
        ));
        assert!(names_profile(
            "--user-data-dir=/t/p-10 --x=/t/p-1",
            "/t/p-1"
        ));
        assert!(!names_profile("--user-data-dir=/t/p-10", "/t/p-1"));
        assert!(!names_profile("--user-data-dir=/t/p-1x", "/t/p-1"));
        assert!(!names_profile("--user-data-dir=/t/p-1", ""));
    }

    /// The pr-review-19 test (join-1), as the review wrote it: a failed start ends each process that
    /// names its profile. Under the egress check the check's own shell
    /// (`sh scripts/egress/check.sh LIST -- BROWSER ARGS`) carries the browser's arguments; a perl
    /// process with the shell's arguments stands in for it.
    #[cfg(target_os = "macos")]
    #[test]
    fn naming_profile_leaves_the_egress_checks_own_shell_alone() {
        use crate::start::Start;
        let profile =
            std::env::temp_dir().join(format!("r3-web-egress-own-{}", std::process::id()));
        let since_us = crate::procs::table()
            .unwrap()
            .into_iter()
            .find(|p| p.pid == std::process::id())
            .map(|p| p.started_us)
            .unwrap();
        let mut check = std::process::Command::new("/usr/bin/perl")
            .args([
                "-e",
                "sleep 30",
                "--",
                "scripts/egress/check.sh",
                "list.txt",
                "--",
            ])
            .arg("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome")
            .arg(format!("--user-data-dir={}", profile.display()))
            .start()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let (mut seen, mut named) = (false, Vec::new());
        while std::time::Instant::now() < deadline {
            let table = crate::procs::table().unwrap();
            seen = table.iter().any(|p| p.pid == check.id());
            let mark = Mark::new(
                &profile,
                std::path::Path::new(
                    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
                ),
                since_us,
            );
            named = table
                .iter()
                .filter(|p| mark.owns(p, &command_args))
                .cloned()
                .collect::<Vec<_>>();
            if seen && named.iter().any(|p| p.pid == check.id()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let _ = check.kill();
        let _ = check.wait();
        assert!(
            seen,
            "the stand-in of the check's shell was not in the table"
        );
        assert!(
            !named.iter().any(|p| p.pid == check.id()),
            "the egress check's own shell is among the processes that a failed start ends"
        );
    }

    /// CP-126 E1 (4): the routine read a process of the run by number and creation time; by the
    /// kill another process has the number. The kill reads the creation time again and leaves the
    /// other process alone. The table stands in for the read, a live `sleep` for the other process.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_number_that_another_process_took_is_left_alone() {
        use crate::start::Start;
        let mut other = std::process::Command::new("/bin/sleep")
            .arg("30")
            .start()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));
        let real = procs::table()
            .unwrap()
            .into_iter()
            .find(|p| p.pid == other.id())
            .unwrap();
        let read = Proc {
            started_us: real.started_us.saturating_sub(5_000_000),
            ..real.clone()
        };
        let mark = Mark {
            profile: "/t/p".into(),
            since_us: 0,
            root: real.exe.clone().unwrap().parent().unwrap().to_path_buf(),
        };
        let ended = end_runs_own_with(
            &mark,
            &|| Ok(vec![read.clone()]),
            &|_| Some(vec!["--user-data-dir=/t/p".to_string()]),
            &|_| true,
            &procs::kill,
        );
        let alive = matches!(other.try_wait(), Ok(None));
        let _ = other.kill();
        let _ = other.wait();
        assert!(
            alive,
            "the process that has the number now was ended ({ended:?})"
        );
        assert_eq!(ended.ended, 1, "the read process was recorded");
    }

    /// A started child on the pipe and the test's own `sleep`, whose row stands for the browser's
    /// first process so that the child on the pipe is a second process with the browser's ends.
    #[cfg(target_os = "macos")]
    fn second_holder() -> (crate::start::OnPipe, std::process::Child, Proc) {
        use crate::start::Start;
        let spec = crate::start::Spec {
            program: std::path::Path::new("/bin/sleep"),
            args: vec!["5".into()],
            env: Vec::new(),
            stderr: None,
        };
        let on = {
            let held = crate::start::lock();
            crate::start::start_on_pipe(&held, &spec).expect("start")
        };
        let root = std::process::Command::new("/bin/sleep")
            .arg("5")
            .start()
            .expect("root");
        std::thread::sleep(std::time::Duration::from_millis(300));
        let row = procs::table()
            .unwrap()
            .into_iter()
            .find(|p| p.pid == root.id())
            .expect("row of the root");
        (on, root, row)
    }

    /// CP-126 E5 T1: `check()` with the pipe's ids counts a second process that holds an end of the
    /// pipe (pr-review-19 T1: the count pinned to 0 passed every test).
    #[cfg(target_os = "macos")]
    #[test]
    fn check_counts_a_second_holder_of_the_pipe() {
        let (mut on, mut root, row) = second_holder();
        let counts = check(&Subject {
            root: &row,
            profile: std::path::Path::new("/nonexistent-profile"),
            pipe: Some(&on.ids),
            relay_port: None,
        });
        on.child.kill();
        let _ = root.kill();
        let _ = root.wait();
        let holders = counts
            .iter()
            .find(|c| c.rule == "pipe_holder")
            .unwrap()
            .count;
        assert!(holders >= 1, "pipe_holder={holders} with a second holder");
    }

    /// CP-126 E5 T2: a process that holds the executor's read end alone (the third of the four
    /// ends) is found (pr-review-19 T2: the search cut to the first two ends passed every test).
    #[cfg(target_os = "macos")]
    #[test]
    fn a_holder_of_the_executors_read_end_alone_is_found() {
        use crate::start::Start;
        let (mut on, mut root, _) = second_holder();
        let read_end = on.from_browser.try_clone().unwrap();
        let mut holder = std::process::Command::new("/bin/sleep")
            .arg("5")
            .stdin(std::process::Stdio::from(read_end))
            .start()
            .expect("holder");
        std::thread::sleep(std::time::Duration::from_millis(300));
        let found = holds_an_end(holder.id(), &on.ids);
        let held = crate::peer::pipe_handles_of(holder.id()).unwrap_or_default();
        on.child.kill();
        for c in [&mut root, &mut holder] {
            let _ = c.kill();
            let _ = c.wait();
        }
        assert!(
            held.iter().all(|(_, h)| *h == on.ids.handles[2]) && !held.is_empty(),
            "the holder holds the third end alone: {held:?}"
        );
        assert!(found, "a holder of the third end alone was not found");
    }

    /// CP-126 E5 T6: a running process of the tree whose command line cannot be read counts in
    /// `port_flag` (pr-review-19 T6).
    #[cfg(target_os = "macos")]
    #[test]
    fn a_tree_process_whose_command_line_cannot_be_read_counts_in_port_flag() {
        use crate::start::Start;
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("5")
            .start()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(200));
        let row = procs::table()
            .unwrap()
            .into_iter()
            .find(|p| p.pid == child.id())
            .unwrap();
        let subject = Subject {
            root: &row,
            profile: std::path::Path::new("/nonexistent-profile"),
            pipe: None,
            relay_port: None,
        };
        let pid = child.id();
        let flag = |c: &[Count]| c.iter().find(|x| x.rule == "port_flag").unwrap().count;
        let unread = flag(&check_with(&subject, &|p| {
            if p == pid {
                None
            } else {
                command_line(p)
            }
        }));
        let read = flag(&check_with(&subject, &command_line));
        let _ = child.kill();
        let _ = child.wait();
        assert_eq!((unread, read), (1, 0));
    }

    /// CP-126 E5 N5: an adopted process (parent 1, in the browser's bundle, begun after the root)
    /// joins the tree only when one of its arguments names the root's profile as a whole path
    /// (c296a1cf's rule; pr-review-19 N5 took it out with every test passing).
    #[test]
    fn tree_of_takes_an_adopted_process_only_when_it_names_the_profile() {
        let row = |pid: u32, ppid: u32, exe: &str| Proc {
            pid,
            ppid,
            started_us: if pid == 100 { 10 } else { 20 },
            exe: Some(std::path::PathBuf::from(exe)),
        };
        let table = vec![
            row(100, 50, "/b/App.app/Contents/MacOS/App"),
            row(200, 1, "/b/App.app/Contents/Frameworks/handler"),
            row(300, 1, "/b/App.app/Contents/Frameworks/handler"),
            row(400, 1, "/b/App.app/Contents/Frameworks/handler"),
        ];
        let args = |pid: u32| -> Option<Vec<String>> {
            Some(vec![match pid {
                100 => "--user-data-dir=/t/p-1".into(),
                200 => "--database=/t/p-1/Crashpad".into(),
                300 => "--user-data-dir=/t/p-10".into(),
                _ => "--other".into(),
            }])
        };
        let tree: Vec<u32> = tree_of_with(&table, &table[0], &args)
            .iter()
            .map(|p| p.pid)
            .collect();
        assert_eq!(tree, vec![100, 200]);
    }

    /// The routine's passes: it stops on a pass that finds none, counts what its first pass found,
    /// and counts a table that cannot be read as one found.
    #[test]
    fn the_routine_ends_on_an_empty_pass_and_counts_an_unread_table() {
        let mark = Mark {
            profile: "/t/p".into(),
            since_us: 0,
            root: std::path::PathBuf::from("/b"),
        };
        let p = |pid: u32| Proc {
            pid,
            ppid: 1,
            started_us: 10,
            exe: Some(std::path::PathBuf::from("/b/x")),
        };
        let reads = std::cell::Cell::new(0);
        let killed = std::cell::RefCell::new(Vec::new());
        let ended = end_runs_own_with(
            &mark,
            &|| {
                reads.set(reads.get() + 1);
                Ok(if reads.get() == 1 {
                    vec![p(7), p(8)]
                } else {
                    Vec::new()
                })
            },
            &|_| Some(vec!["--user-data-dir=/t/p".to_string()]),
            &|_| true,
            &|q| {
                killed.borrow_mut().push(q.pid);
                true
            },
        );
        assert_eq!(
            (ended.before, ended.after, ended.ended, ended.passes),
            (2, 0, 2, 2)
        );
        assert_eq!(*killed.borrow(), vec![7, 8]);
        let unread = end_runs_own_with(
            &mark,
            &|| Err(std::io::Error::other("no table")),
            &|_| None,
            &|_| true,
            &|_| true,
        );
        assert_eq!(
            (unread.before, unread.after, unread.passes),
            (1, 1, END_PASSES)
        );
    }

    /// CP-126 E1 (3): a process whose arguments name the run's profile but whose program is not the
    /// browser's (here perl, beside a browser that would run from an app bundle) is no process of
    /// the run.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_process_whose_program_is_not_the_browsers_is_no_process_of_the_run() {
        use crate::start::Start;
        let dir = std::env::temp_dir().join(format!("r3-web-other-{}", std::process::id()));
        let profile = dir.join("p-1");
        let since_us = crate::procs::table()
            .unwrap()
            .into_iter()
            .find(|p| p.pid == std::process::id())
            .map(|p| p.started_us)
            .unwrap();
        let mut other = std::process::Command::new("/usr/bin/perl")
            .args(["-e", "sleep 30", "--"])
            .arg(format!("--user-data-dir={}", profile.display()))
            .start()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut taken = false;
        while std::time::Instant::now() < deadline {
            let table = crate::procs::table().unwrap();
            let mark = Mark::new(
                &profile,
                std::path::Path::new(
                    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
                ),
                since_us,
            );
            taken = table
                .iter()
                .any(|p| p.pid == other.id() && mark.owns(p, &command_args));
            if taken {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let _ = other.kill();
        let _ = other.wait();
        assert!(
            !taken,
            "a process of another program counts as the run's own"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn each_rule_counts_what_breaks_it() {
        use crate::start::Start;
        let root = |pid: u32| {
            procs::table()
                .unwrap()
                .into_iter()
                .find(|p| p.pid == pid)
                .unwrap()
        };
        let dir = std::env::temp_dir().join(format!("r3-web-checks-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut child = std::process::Command::new("/bin/sh")
            .args(["-c", "sleep 5; true", "sh", "--remote-debugging-port=0"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .start()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(200));
        let counts = |pid: u32, relay: Option<u16>| {
            let r = root(pid);
            check(&Subject {
                root: &r,
                profile: &dir,
                pipe: None,
                relay_port: relay,
            })
        };
        let of = |c: &[Count], rule: &str| c.iter().find(|x| x.rule == rule).unwrap().count;
        let quiet = counts(child.id(), None);
        assert_eq!(of(&quiet, "port_flag"), 1);
        assert_eq!(of(&quiet, "tcp_listen"), 0);
        assert_eq!(of(&quiet, "active_port_file"), 0);
        let _ = child.kill();
        let _ = child.wait();
        let held = crate::start::lock();
        let tcp = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let udp = std::net::UdpSocket::bind("0.0.0.0:0").unwrap();
        drop(held);
        std::fs::write(dir.join("DevToolsActivePort"), b"0\n").unwrap();
        let me = std::process::id();
        let loud = counts(me, None);
        assert_eq!(of(&loud, "port_flag"), 0);
        assert!(of(&loud, "tcp_listen") >= 1);
        assert!(of(&loud, "udp_wide") >= 1);
        assert_eq!(of(&loud, "active_port_file"), 1);
        assert!(of(&loud, "executor_listener") >= 1);
        drop((tcp, udp));
        let _ = std::fs::remove_file(dir.join("DevToolsActivePort"));
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn the_relay_s_listener_is_the_one_listener_that_the_executor_may_hold() {
        use std::net::{IpAddr, Ipv4Addr};
        let at = |pid: u32, port: u16, listening: bool, proto: Proto| peer::Socket {
            pid,
            proto,
            local: (IpAddr::V4(Ipv4Addr::LOCALHOST), port),
            remote: (IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
            listening,
        };
        let socks = vec![
            at(7, 5000, true, Proto::Tcp),
            at(7, 5001, true, Proto::Tcp),
            at(7, 5002, false, Proto::Tcp),
            at(7, 5003, false, Proto::Udp),
            at(8, 5004, true, Proto::Tcp),
        ];
        assert_eq!(listeners_beside(&socks, 7, Some(5000)), 1);
        assert_eq!(listeners_beside(&socks, 7, None), 2);
        assert_eq!(listeners_beside(&socks, 7, Some(5001)), 1);
        assert_eq!(listeners_beside(&socks, 9, None), 0);
    }

    #[test]
    fn this_process_s_command_line_is_read() {
        let line = command_line(std::process::id()).unwrap_or_default();
        let me = std::env::current_exe().unwrap();
        let name = me.file_name().unwrap().to_string_lossy().into_owned();
        assert!(line.contains(&name), "{name}");
    }
}
