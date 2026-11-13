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
            if let Some(f) = found.as_ref().filter(|f| !f.is_empty()) {
                // which programs the run left: the file names of their executables
                let mut names: Vec<String> = f
                    .iter()
                    .filter_map(|p| {
                        p.exe
                            .as_ref()?
                            .file_name()
                            .map(|n| n.to_string_lossy().replace(' ', "_"))
                    })
                    .collect();
                names.sort();
                names.dedup();
                eprintln!("left_found n={n} programs={}", names.join(","));
            }
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

/// The four ends of the executor's pipe, by the order of `PipeIds::handles`.
pub const ENDS: [&str; 4] = [
    "browser_read",
    "executor_write",
    "executor_read",
    "browser_write",
];

/// The keys that a read and a cell's check line carry beside the six rules (CP-145 (2) (g) and the
/// coordinator's point on an unread table): the processes of the two rules that passed or are
/// open, and the reads whose process table could not be read.
pub const KEYS: [&str; 5] = [
    "port_flag_passing",
    "port_flag_open",
    "pipe_holder_passing",
    "pipe_holder_open",
    "table_unread",
];

/// How long the follow of a counted process lasts at most, and its step (CP-145 (2) (c): the
/// length of `FIRST_REPLY_WAIT`, inside which a start must answer).
pub const FOLLOW_CAP: std::time::Duration = std::time::Duration::from_secs(30);
pub const FOLLOW_STEP: std::time::Duration = std::time::Duration::from_millis(5);

/// The class of a process that a read counted in `port_flag` or `pipe_holder` (CP-145 (2)).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Counts,
    Passing,
    Open,
}

impl Class {
    pub fn word(self) -> &'static str {
        match self {
            Class::Counts => "counts",
            Class::Passing => "passing",
            Class::Open => "open",
        }
    }
}

/// The parent of a counted process, by number and start time from the table of the same read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Parent {
    First,
    Tree,
    Executor,
    Other,
    Unread,
}

impl Parent {
    pub fn word(self) -> &'static str {
        match self {
            Parent::First => "first",
            Parent::Tree => "tree",
            Parent::Executor => "executor",
            Parent::Other => "other",
            Parent::Unread => "unread",
        }
    }
}

/// One read of a counted process's own state: its arguments, its program and the ends of the
/// executor's pipe that it holds, by index of `ENDS`; `None` where the read failed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProcRead {
    pub args: Option<Vec<String>>,
    pub exe: Option<std::path::PathBuf>,
    pub ends: Option<Vec<usize>>,
}

/// Why a counted process counts at once (CP-145 (2) (b)), or `None` when it may be on its way and
/// the check follows it.
pub fn counts_at_once(
    r: &ProcRead,
    parent: Parent,
    bundle: &Path,
    windows: bool,
) -> Option<&'static str> {
    if windows {
        return Some("windows");
    }
    if let Some(a) = &r.args {
        return Some(if a.iter().any(|x| names_a_port(x)) {
            "flag"
        } else {
            "args_read_holding"
        });
    }
    if r.ends
        .as_ref()
        .is_some_and(|e| e.iter().any(|i| *i == 1 || *i == 2))
    {
        return Some("executor_end");
    }
    if r.exe.as_deref().is_some_and(|e| !inside(e, bundle)) {
        return Some("program_out");
    }
    if !matches!(parent, Parent::First | Parent::Tree) {
        return Some("parent");
    }
    None
}

/// The follow of a counted process whose arguments could not be read (CP-145 (2) (c) to (e)):
/// `read` gives its state by number and start time, or `None` once it no longer runs. Gives the
/// class, what the deciding read found, and the milliseconds to it.
pub fn follow(
    read: &mut dyn FnMut() -> Option<ProcRead>,
    now_ms: &mut dyn FnMut() -> u128,
    pause: &mut dyn FnMut(),
    cap: std::time::Duration,
    bundle: &Path,
) -> (Class, &'static str, u128) {
    let cap = cap.as_millis();
    loop {
        let at = now_ms();
        let Some(r) = read() else {
            return (Class::Open, "holder_gone", at);
        };
        if let Some(a) = &r.args {
            let found = if a.iter().any(|x| names_a_port(x)) {
                "flag"
            } else if r.exe.as_deref().is_none_or(|e| !inside(e, bundle)) {
                "program_out"
            } else if r.ends.as_ref().is_none_or(|e| !e.is_empty()) {
                "holds"
            } else {
                return (Class::Passing, "settled", at);
            };
            return (Class::Counts, found, at);
        }
        if at >= cap {
            return (Class::Counts, "args_unread_at_cap", at);
        }
        pause();
    }
}

/// A process that a read counted, named (CP-145 (2) (g)).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NamedProc {
    pub rule: &'static str,
    pub pid: u32,
    pub started_us: u64,
    pub ppid: u32,
    pub parent_started_us: Option<u64>,
    pub parent: Parent,
    /// The program's file name, never its path.
    pub program: Option<String>,
    pub bundle: &'static str,
    pub args: bool,
    pub flag: bool,
    pub end: String,
    pub follow_ms: Option<u128>,
    pub found: &'static str,
    pub class: Class,
}

impl NamedProc {
    /// The keys of its naming line, after the step, the run and the read.
    pub fn keys(&self) -> String {
        format!(
            "rule={} pid={} started_us={} ppid={} parent_started_us={} parent={} program={} bundle={} args={} flag={} end={} follow_ms={} found={} class={}",
            self.rule,
            self.pid,
            self.started_us,
            self.ppid,
            self.parent_started_us.map_or("-".into(), |v| v.to_string()),
            self.parent.word(),
            self.program.as_deref().unwrap_or("-").replace(' ', "_"),
            self.bundle,
            if self.args { "read" } else { "unread" },
            if self.flag { "yes" } else { "no" },
            self.end,
            self.follow_ms.map_or("-".into(), |v| v.to_string()),
            self.found,
            self.class.word()
        )
    }
}

/// Whether a read holds: each of the two followed rules counts nothing less its passing and open
/// processes and has none open, the table was read, and each other rule counts 0.
pub fn read_holds(counts: &[Count]) -> bool {
    let n = |rule: &str| {
        counts
            .iter()
            .find(|c| c.rule == rule)
            .map_or(0, |c| c.count)
    };
    RULES.iter().all(|r| match *r {
        "port_flag" | "pipe_holder" => {
            let passing = n(&format!("{r}_passing"));
            let open = n(&format!("{r}_open"));
            n(r).saturating_sub(passing + open) == 0 && open == 0
        }
        _ => n(r) == 0,
    }) && n("table_unread") == 0
}

/// Reads every rule once. A rule whose read the system refused counts each unread process as one.
pub fn check(s: &Subject<'_>) -> Vec<Count> {
    check_named(s).0
}

/// `check`, with each process that it counted in `port_flag` or `pipe_holder` named and classed.
pub fn check_named(s: &Subject<'_>) -> (Vec<Count>, Vec<NamedProc>) {
    let table = {
        // the start lock for the check's read (CP-145 (2) (f)): no child of this process is on its
        // way while the table is read
        let _held = crate::start::lock();
        procs::table().ok()
    };
    check_detail(s, table, &command_line, FOLLOW_CAP)
}

/// `check` with the reading of a process's command line given, for tests; it follows no process.
pub fn check_with(s: &Subject<'_>, line_of: &dyn Fn(u32) -> Option<String>) -> Vec<Count> {
    check_detail(s, procs::table().ok(), line_of, std::time::Duration::ZERO).0
}

/// The ends of `ids` that `pid` holds, by index of `ENDS`; `None` when its descriptors could not
/// be read.
fn ends_of(pid: u32, ids: Option<&PipeIds>) -> Option<Vec<usize>> {
    let Some(ids) = ids else {
        return Some(Vec::new());
    };
    #[cfg(target_os = "macos")]
    {
        crate::peer::pipe_handles_of(pid).map(|held| {
            (0..4)
                .filter(|i| ids.handles[*i] != 0 && held.iter().any(|(_, h)| *h == ids.handles[*i]))
                .collect()
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        if holds_an_end(pid, ids) {
            None
        } else {
            Some(Vec::new())
        }
    }
}

/// `check_named` on a table read under the start lock (`None`: it could not be read), with the
/// reading of a command line and the follow's cap given.
pub fn check_detail(
    s: &Subject<'_>,
    table: Option<Vec<Proc>>,
    line_of: &dyn Fn(u32) -> Option<String>,
    follow_cap: std::time::Duration,
) -> (Vec<Count>, Vec<NamedProc>) {
    let table_unread = usize::from(table.is_none());
    let table = table.unwrap_or_default();
    let tree = tree_of(&table, s.root);
    let pids: Vec<u32> = tree.iter().map(|p| p.pid).collect();
    let mut port_flag = 0;
    let mut flagged: Vec<(Proc, bool)> = Vec::new();
    for p in &tree {
        match line_of(p.pid) {
            Some(line) if names_a_port(&line) => {
                port_flag += 1;
                flagged.push((p.clone(), true));
            }
            Some(_) => {}
            None if procs::running(p) => {
                port_flag += 1;
                flagged.push((p.clone(), false));
            }
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
    let holders: Vec<Proc> = s
        .pipe
        .map(|ids| pipe_holder_procs(&table, s.root, ids))
        .unwrap_or_default();
    let pipe_holder = holders.len();
    // CP-145 (2): each counted process classed by its own state
    let bundle = s
        .root
        .exe
        .as_deref()
        .map(procs::bundle_root)
        .unwrap_or_default();
    let parent_of = |p: &Proc| -> (Parent, Option<u64>) {
        let Some(row) = table.iter().find(|t| t.pid == p.ppid) else {
            return (Parent::Unread, None);
        };
        let kind = if procs::same(row, s.root) {
            Parent::First
        } else if row.pid == me {
            Parent::Executor
        } else if tree.iter().any(|t| procs::same(t, row))
            && row.exe.as_deref().is_some_and(|e| inside(e, &bundle))
        {
            Parent::Tree
        } else {
            Parent::Other
        };
        (kind, Some(row.started_us))
    };
    let state = |p: &Proc| ProcRead {
        args: line_of(p.pid).map(|l| vec![l]),
        exe: p.exe.clone(),
        ends: ends_of(p.pid, s.pipe),
    };
    let start = std::time::Instant::now();
    let mut named = Vec::new();
    let mut class_of: Vec<(Proc, Class, &'static str, Option<u128>, ProcRead)> = Vec::new();
    let all: Vec<(&'static str, Proc)> = flagged
        .iter()
        .map(|(p, _)| ("port_flag", p.clone()))
        .chain(holders.iter().map(|p| ("pipe_holder", p.clone())))
        .collect();
    for (rule, p) in &all {
        // the class is the process's, the same on both lines
        let known = class_of.iter().find(|(q, ..)| procs::same(q, p)).cloned();
        let (class, found, follow_ms, first) = match known {
            Some((_, c, f, ms, r)) => (c, f, ms, r),
            None => {
                let first = state(p);
                let (parent, _) = parent_of(p);
                let (c, f, ms) = match counts_at_once(&first, parent, &bundle, cfg!(windows)) {
                    Some(f) => (Class::Counts, f, None),
                    None => {
                        let (c, f, ms) = follow(
                            &mut || {
                                if !procs::running(p) {
                                    return None;
                                }
                                Some(ProcRead {
                                    args: line_of(p.pid).map(|l| vec![l]),
                                    exe: procs::table()
                                        .ok()
                                        .and_then(|t| t.into_iter().find(|t| procs::same(t, p)))
                                        .and_then(|t| t.exe),
                                    ends: ends_of(p.pid, s.pipe),
                                })
                            },
                            &mut || start.elapsed().as_millis(),
                            &mut || std::thread::sleep(FOLLOW_STEP),
                            follow_cap,
                            &bundle,
                        );
                        (c, f, Some(ms))
                    }
                };
                class_of.push((p.clone(), c, f, ms, first.clone()));
                (c, f, ms, first)
            }
        };
        let (parent, parent_started_us) = parent_of(p);
        named.push(NamedProc {
            rule,
            pid: p.pid,
            started_us: p.started_us,
            ppid: p.ppid,
            parent_started_us,
            parent,
            program: p
                .exe
                .as_ref()
                .and_then(|e| e.file_name())
                .map(|n| n.to_string_lossy().into_owned()),
            bundle: match p.exe.as_deref() {
                None => "unread",
                Some(e) if inside(e, &bundle) => "in",
                Some(_) => "out",
            },
            args: first.args.is_some(),
            flag: first
                .args
                .as_ref()
                .is_some_and(|a| a.iter().any(|x| names_a_port(x))),
            end: match &first.ends {
                None => "unread".into(),
                Some(e) if e.is_empty() => "none".into(),
                Some(e) => e.iter().map(|i| ENDS[*i]).collect::<Vec<_>>().join(","),
            },
            follow_ms,
            found,
            class,
        });
    }
    let of = |rule: &str, class: Class| {
        named
            .iter()
            .filter(|n| n.rule == rule && n.class == class)
            .count()
    };
    let mut counts = vec![
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
    ];
    for (rule, count) in [
        ("port_flag_passing", of("port_flag", Class::Passing)),
        ("port_flag_open", of("port_flag", Class::Open)),
        ("pipe_holder_passing", of("pipe_holder", Class::Passing)),
        ("pipe_holder_open", of("pipe_holder", Class::Open)),
        ("table_unread", table_unread),
    ] {
        counts.push(Count { rule, count });
    }
    (counts, named)
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
    pipe_holder_procs(table, root, ids).len()
}

/// The processes that `pipe_holders` counts.
pub fn pipe_holder_procs(table: &[Proc], root: &Proc, ids: &PipeIds) -> Vec<Proc> {
    let me = std::process::id();
    table
        .iter()
        .filter(|p| p.pid != me && !procs::same(p, root))
        .filter(|p| {
            cfg!(target_os = "macos") || p.started_us == 0 || p.started_us >= root.started_us
        })
        .filter(|p| holds_an_end(p.pid, ids))
        .cloned()
        .collect()
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

    /// CP-145 (2) (g): a check that counts a second holder of the pipe names it (number, start
    /// time, parent, program by file name, the ends held, its class) and counts as before; a holder
    /// whose arguments were read while it held an end counts at once.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_counted_holder_is_named_with_its_class() {
        let (mut on, mut root, row) = second_holder();
        let subject = Subject {
            root: &row,
            profile: std::path::Path::new("/nonexistent-profile"),
            pipe: Some(&on.ids),
            relay_port: None,
        };
        let (counts, named) = check_named(&subject);
        let plain = check(&subject);
        let child = on.child.pid();
        on.child.kill();
        let _ = root.kill();
        let _ = root.wait();
        let count = |cs: &[Count], rule: &str| cs.iter().find(|c| c.rule == rule).unwrap().count;
        for rule in ["port_flag", "pipe_holder"] {
            let n = named.iter().filter(|c| c.rule == rule).count();
            assert_eq!(count(&counts, rule), n, "{rule}: {named:?}");
        }
        assert_eq!(count(&counts, "pipe_holder"), count(&plain, "pipe_holder"));
        let holder = named
            .iter()
            .find(|c| c.rule == "pipe_holder" && c.pid == child)
            .unwrap_or_else(|| panic!("the holder {child} is not named: {named:?}"));
        assert_eq!(holder.ppid, std::process::id(), "{holder:?}");
        assert_eq!(holder.parent, Parent::Executor, "{holder:?}");
        assert_eq!(holder.program.as_deref(), Some("sleep"), "{holder:?}");
        assert_eq!(holder.end, "browser_read,browser_write", "{holder:?}");
        assert_eq!(
            (holder.class, holder.found),
            (Class::Counts, "args_read_holding")
        );
        assert!(
            !holder.keys().contains("/bin/"),
            "a path in {}",
            holder.keys()
        );
        assert!(!read_holds(&counts));
    }

    /// CP-145 (2): the classes of a counted process, on stand-in reads. Each case: the read that
    /// counted it, its parent, the later reads of the follow (`None`: it no longer runs), and the
    /// class with what decided it.
    #[test]
    fn a_counted_process_is_classed_by_its_own_state() {
        use std::path::PathBuf;
        let bundle = PathBuf::from("/Applications/B.app");
        let inner = || Some(bundle.join("Contents/Frameworks/H.app/Contents/MacOS/H"));
        let r = |args: Option<&str>, exe: Option<PathBuf>, ends: Option<Vec<usize>>| ProcRead {
            args: args.map(|a| vec![a.to_string()]),
            exe,
            ends,
        };
        let unread = r(None, inner(), Some(vec![0]));
        type Case = (
            &'static str,
            ProcRead,
            Parent,
            Vec<Option<ProcRead>>,
            bool,
            Class,
            &'static str,
        );
        let cases: Vec<Case> = vec![
            (
                "unread, then read with no flag, no end, a parent of the tree",
                unread.clone(),
                Parent::Tree,
                vec![Some(r(Some("H --type=gpu"), inner(), Some(vec![])))],
                false,
                Class::Passing,
                "settled",
            ),
            (
                "unread, then read with the flag",
                unread.clone(),
                Parent::First,
                vec![Some(r(
                    Some("H --remote-debugging-port=0"),
                    inner(),
                    Some(vec![]),
                ))],
                false,
                Class::Counts,
                "flag",
            ),
            (
                "unread, then still holding an end",
                unread.clone(),
                Parent::First,
                vec![Some(r(Some("H"), inner(), Some(vec![3])))],
                false,
                Class::Counts,
                "holds",
            ),
            (
                "arguments read while it held an end, which it then closes",
                r(Some("H"), inner(), Some(vec![0])),
                Parent::First,
                vec![Some(r(Some("H"), inner(), Some(vec![])))],
                false,
                Class::Counts,
                "args_read_holding",
            ),
            (
                "the same, and it then ends",
                r(Some("H"), inner(), Some(vec![0])),
                Parent::First,
                vec![None],
                false,
                Class::Counts,
                "args_read_holding",
            ),
            (
                "unread, then gone",
                unread.clone(),
                Parent::First,
                vec![None],
                false,
                Class::Open,
                "holder_gone",
            ),
            (
                "a parent outside the tree",
                unread.clone(),
                Parent::Other,
                vec![Some(r(Some("H"), inner(), Some(vec![])))],
                false,
                Class::Counts,
                "parent",
            ),
            (
                "a parent that cannot be read",
                unread.clone(),
                Parent::Unread,
                vec![Some(r(Some("H"), inner(), Some(vec![])))],
                false,
                Class::Counts,
                "parent",
            ),
            (
                "the executor as the parent",
                unread.clone(),
                Parent::Executor,
                vec![Some(r(Some("H"), inner(), Some(vec![])))],
                false,
                Class::Counts,
                "parent",
            ),
            (
                "a program outside the bundle",
                r(None, Some(PathBuf::from("/bin/sh")), Some(vec![0])),
                Parent::First,
                vec![Some(r(Some("sh"), inner(), Some(vec![])))],
                false,
                Class::Counts,
                "program_out",
            ),
            (
                "an executor's end held",
                r(None, inner(), Some(vec![2])),
                Parent::First,
                vec![Some(r(Some("H"), inner(), Some(vec![])))],
                false,
                Class::Counts,
                "executor_end",
            ),
            (
                "descriptors that cannot be read at the later read",
                unread.clone(),
                Parent::First,
                vec![Some(r(Some("H"), inner(), None))],
                false,
                Class::Counts,
                "holds",
            ),
            (
                "arguments still unread at the cap",
                unread.clone(),
                Parent::First,
                vec![Some(unread.clone()); 10],
                false,
                Class::Counts,
                "args_unread_at_cap",
            ),
            (
                "on Windows a holder that a later read no longer finds",
                unread.clone(),
                Parent::First,
                vec![Some(r(Some("H"), inner(), Some(vec![])))],
                true,
                Class::Counts,
                "windows",
            ),
        ];
        let mut wrong = Vec::new();
        for (name, first, parent, later, windows, want, why) in cases {
            let (class, found) = match counts_at_once(&first, parent, &bundle, windows) {
                Some(f) => (Class::Counts, f),
                None => {
                    let mut reads = later.into_iter();
                    let clock = std::cell::Cell::new(0u128);
                    let (c, f, _) = follow(
                        &mut || reads.next().unwrap_or(None),
                        &mut || clock.get(),
                        &mut || clock.set(clock.get() + 5),
                        // the cap of the case: nine steps of 5 ms
                        std::time::Duration::from_millis(45),
                        &bundle,
                    );
                    (c, f)
                }
            };
            if (class, found) != (want, why) {
                wrong.push(format!("{name}: {class:?} {found}, want {want:?} {why}"));
            }
        }
        assert!(wrong.is_empty(), "{wrong:#?}");
        assert_eq!(FOLLOW_CAP, std::time::Duration::from_secs(30));
        assert_eq!(FOLLOW_STEP, std::time::Duration::from_millis(5));
    }

    /// CP-145 (2) (f): the check takes the start lock for its read, so it waits while a start holds
    /// it.
    #[cfg(unix)]
    #[test]
    fn a_check_waits_while_the_start_lock_is_held() {
        let held = crate::start::lock();
        let (tx, rx) = std::sync::mpsc::channel();
        let reader = std::thread::spawn(move || {
            let row = Proc {
                pid: std::process::id(),
                ppid: 1,
                started_us: 0,
                exe: None,
            };
            let _ = check_named(&Subject {
                root: &row,
                profile: std::path::Path::new("/nonexistent-profile"),
                pipe: None,
                relay_port: None,
            });
            let _ = tx.send(());
        });
        let early = rx.recv_timeout(std::time::Duration::from_millis(500));
        drop(held);
        let late = rx.recv_timeout(std::time::Duration::from_secs(30));
        let _ = reader.join();
        assert!(
            early.is_err(),
            "the check read while the start lock was held"
        );
        assert!(
            late.is_ok(),
            "the check did not read after the lock was let go"
        );
    }

    /// The coordinator's point on an unread table: a read whose process table could not be read is
    /// open (`table_unread=1`), never a read that found nothing.
    #[test]
    fn a_read_of_an_unread_table_is_open_and_not_empty() {
        let row = Proc {
            pid: std::process::id(),
            ppid: 1,
            started_us: 0,
            exe: None,
        };
        let subject = Subject {
            root: &row,
            profile: std::path::Path::new("/nonexistent-profile"),
            pipe: None,
            relay_port: None,
        };
        let (counts, _) = check_detail(&subject, None, &|_| None, std::time::Duration::ZERO);
        let n = |rule: &str| counts.iter().find(|c| c.rule == rule).unwrap().count;
        assert_eq!(n("table_unread"), 1);
        assert!(
            !read_holds(&counts),
            "an unread table read as a read that holds"
        );
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
        // the holder may hold pipes that this test process inherited from its own parent (a hosted
        // runner passes some on without close-on-exec); of the executor's four ends it holds the
        // third alone
        let ends = &on.ids.handles;
        assert!(
            held.iter().any(|(_, h)| *h == ends[2])
                && held.iter().all(|(_, h)| *h == ends[2] || !ends.contains(h)),
            "the holder holds an end of the executor's other than the third: {held:?}"
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
