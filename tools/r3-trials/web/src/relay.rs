use std::collections::{BTreeSet, HashMap};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::{AbortHandle, JoinHandle};

use crate::procs::{self, Proc};

const HANDSHAKE_WAIT: Duration = Duration::from_secs(10);

/// The most connections that the relay holds open at one time; a connection beyond it is closed at
/// its accept and counted (A11.1).
pub const MAX_OPEN: usize = 512;

/// The most entries that each record of the relay keeps (the refusals, the closed tunnels and the
/// foreign peers); an entry beyond it is counted and not kept (A11.1).
pub const MAX_RECORD: usize = 4096;

/// A connection that the relay refused because its other end was not one process of the
/// browser's tree, as the system's table read it while the relay held the socket: the port of the
/// other end and how many owners the table named, and of them how many were in the tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForeignPeer {
    pub peer_port: u16,
    pub owners: usize,
    pub in_tree: usize,
    pub cause: &'static str,
}

/// A host and a port, the unit of the relay's list and of its record.
pub type HostPort = (String, u16);

/// The relay of rulings CP-24 and CP-64: a SOCKS5 listener on a loopback port that the driver's
/// own process opens, given to the browser as its only route. It takes one CONNECT per tunnel. When
/// it holds a list, a tunnel to a host and port outside the list is refused at the SOCKS5 request,
/// before any byte leaves and before the relay looks the name up, and the refusal is recorded by
/// host and port. A name that it may reach goes to 127.0.0.1 when it is under `.test`, `localhost`
/// or a loopback address, and is refused otherwise. It passes the bytes of a tunnel both ways
/// without reading them. When a host leaves the list, its open tunnels are closed. When the process
/// ends, the system closes the relay's sockets with it.
pub struct Relay {
    pub port: u16,
    task: JoinHandle<()>,
    shared: Arc<Shared>,
}

/// A handle on a running relay that the runner keeps to change the list and to read the record.
#[derive(Clone)]
pub struct RelayHandle {
    shared: Arc<Shared>,
}

#[derive(Default)]
struct Shared {
    port: u16,
    listener: Mutex<Option<ListenerRaw>>,
    list: Mutex<Option<BTreeSet<HostPort>>>,
    refusals: Mutex<Vec<HostPort>>,
    open: Mutex<HashMap<u64, (HostPort, AbortHandle)>>,
    closed: Mutex<Vec<HostPort>>,
    next: AtomicU64,
    tunnels: AtomicU64,
    browser: Mutex<Option<Proc>>,
    foreign: Mutex<Vec<ForeignPeer>>,
    foreign_count: AtomicU64,
    connections: AtomicUsize,
    over_bound: AtomicU64,
    unkept: AtomicU64,
    accept_errors: AtomicU64,
}

/// The relay's listener as the system names it, for a start that places it in the guard.
#[cfg(unix)]
type ListenerRaw = std::os::fd::RawFd;
#[cfg(windows)]
type ListenerRaw = usize;

/// How long the relay waits after a failed accept (a process out of descriptors, a connection
/// that its peer abandoned) before it accepts again; the listener stays open.
const ACCEPT_PAUSE: Duration = Duration::from_millis(50);

/// How often the guard reads the process table.
const GUARD_POLL: Duration = Duration::from_millis(20);

/// The flag of the program that the guard runs: `--relay-guard <pid> <creation time in µs>`.
pub const GUARD_FLAG: &str = "--relay-guard";

/// The guard of the relay's port (A11.1: the browser's processes are ended before the relay's
/// listener closes, also when the executor's process is killed): run as `--relay-guard PID US`
/// in a process that holds the relay's listener and nothing else of the executor's process, it
/// reads the process table until no process of the tree of the browser PID with creation time US
/// runs, and returns; the system then closes its copy of the listener. The tree is what its reads
/// found: the first process, the processes that `checks::tree_of` adds while the first process runs
/// (the adopted ones that name the run's profile among them), and each process whose parent was a
/// member that still ran at the read that found it. A process that a member starts after the first
/// process has ended and that loses its parent before the next read, 20 ms later, is not followed,
/// so the guard can free the port while it runs; and when the guard cannot be started, the
/// browser's start goes on without one and no line says so. Both limits are U2.1's to close
/// (CP-161). While the executor lives it holds the listener too, and only the executor accepts.
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
        Some((epid as u32, estart)),
        GuardIo {
            read: &mut || procs::table().ok(),
            now_ms: &mut || start.elapsed().as_millis(),
            pause: &mut |unread| {
                std::thread::sleep(if unread {
                    crate::killwatch::PASS_WAIT
                } else {
                    GUARD_POLL
                })
            },
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

/// How long the guard waits, once its executor is gone, for the browser to end by itself (CP-148).
pub const GRACE: Duration = Duration::from_millis(3_000);

/// How long the guard waits after it asked the run's processes to end before it ends them by force.
pub const TERM_WAIT: Duration = Duration::from_millis(2_000);

/// How long the guard keeps the port after it read the tree gone: longer than one pass of the row's
/// watch on the development Mac under load (the longest kept pass was 476 ms), so that the watch can
/// see the order (CP-148 (3)).
pub const PORT_MARGIN: Duration = Duration::from_millis(1_000);

/// What the guard needs from the system, given for tests: the process table (`None`: it could not
/// be read), the clock, the pause (told whether the last read was unread), the end of one process of
/// the run (`false`: ask it, `true`: by force; each reads the creation time again), and the record
/// of the guard's own act (milliseconds since the executor was read gone, how many it asked, how
/// many it ended by force), called once.
pub struct GuardIo<'a> {
    pub read: &'a mut dyn FnMut() -> Option<Vec<Proc>>,
    pub now_ms: &'a mut dyn FnMut() -> u128,
    pub pause: &'a mut dyn FnMut(bool),
    pub end: &'a mut dyn FnMut(&Proc, bool) -> bool,
    pub record: &'a mut dyn FnMut(&GuardAct),
}

/// What the guard did after its executor was gone, recorded once when it read the tree gone.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GuardAct {
    /// Milliseconds from the read of the executor gone to the read of the tree gone.
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

/// The guard's loop. It keeps the relay's port while a process of the browser's tree runs. A table
/// that could not be read is not an empty one (the coordinator's point on an unread table): the
/// guard keeps the port and reads again in the watch's steps; when the table stays unread for
/// `killwatch::CAP`, it ends with exit 3 and the line `guard_end cause=table_unread`. When the
/// executor (`executor`, by number and start time) is gone and a process of the tree still runs
/// (CP-148), it waits `GRACE`, asks each process of the run that still runs (by number and start
/// time in the table of that moment) to end, ends by force each one that still runs `TERM_WAIT`
/// later, and reads on until none runs. It ends nothing while the executor runs, and no process
/// whose number another process took. Once it read the tree gone it keeps the port `PORT_MARGIN`
/// more, then exits 0.
pub fn guard_loop(pid: u32, started_us: u64, executor: Option<(u32, u64)>, io: GuardIo<'_>) -> i32 {
    let GuardIo {
        read,
        now_ms,
        pause,
        end,
        record,
    } = io;
    let mut known: Vec<Proc> = Vec::new();
    let mut unread_since: Option<u128> = None;
    let mut executor_gone_at: Option<u128> = None;
    let mut asked_at: Option<u128> = None;
    let (mut asked, mut forced) = (0usize, 0usize);
    let (mut acted, mut recorded) = (false, false);
    let mut act = GuardAct::default();
    let mut gone_at: Option<u128> = None;
    loop {
        let Some(table) = read() else {
            let now = now_ms();
            let since = *unread_since.get_or_insert(now);
            if now.saturating_sub(since) >= crate::killwatch::CAP.as_millis() {
                eprintln!("guard_end cause=table_unread");
                return 3;
            }
            pause(true);
            continue;
        };
        unread_since = None;
        let root = table
            .iter()
            .find(|p| p.pid == pid && p.started_us == started_us)
            .cloned();
        let mut living: Vec<Proc> = known
            .iter()
            .filter(|k| table.iter().any(|t| procs::same(t, k)))
            .cloned()
            .collect();
        if let Some(root) = &root {
            living.push(root.clone());
        }
        for p in procs::descendants(&table, &living)
            .into_iter()
            .chain(root.iter().flat_map(|r| crate::checks::tree_of(&table, r)))
        {
            if !known.iter().any(|k| procs::same(k, &p)) {
                known.push(p);
            }
        }
        // the processes of the run that run now, each by number and start time in this table
        let members: Vec<Proc> = known
            .iter()
            .filter(|k| table.iter().any(|t| procs::same(t, k)))
            .cloned()
            .collect();
        let now = now_ms();
        if root.is_none() && members.is_empty() {
            if acted && !recorded {
                act.ms = now.saturating_sub(executor_gone_at.unwrap_or(now));
                act.asked = asked;
                act.forced = forced;
                record(&act);
                recorded = true;
            }
            let since = *gone_at.get_or_insert(now);
            if now.saturating_sub(since) >= PORT_MARGIN.as_millis() {
                return 0;
            }
            pause(false);
            continue;
        }
        gone_at = None;
        let executor_runs = executor.is_none_or(|(epid, estart)| {
            table
                .iter()
                .any(|t| t.pid == epid && t.started_us == estart)
        });
        if !executor_runs {
            let since = *executor_gone_at.get_or_insert(now);
            if now.saturating_sub(since) >= GRACE.as_millis() {
                match asked_at {
                    None => {
                        for p in &members {
                            if end(p, false) {
                                asked += 1;
                            }
                            note(&mut act, p, pid, started_us);
                        }
                        // a process that cannot be asked (Windows) is ended by force after the wait
                        acted = true;
                        asked_at = Some(now);
                    }
                    Some(at) if now.saturating_sub(at) >= TERM_WAIT.as_millis() => {
                        for p in &members {
                            if end(p, true) {
                                forced += 1;
                            }
                            note(&mut act, p, pid, started_us);
                        }
                    }
                    Some(_) => {}
                }
            }
        }
        pause(false);
    }
}

/// Notes in the guard's record a process that it asked or forced: its program's file name, and
/// whether it is the browser's first process.
fn note(act: &mut GuardAct, p: &Proc, pid: u32, started_us: u64) {
    act.first |= p.pid == pid && p.started_us == started_us;
    let name = p
        .exe
        .as_ref()
        .and_then(|e| e.file_name())
        .map_or("-".to_string(), |n| n.to_string_lossy().replace(' ', "_"));
    if !act.programs.contains(&name) {
        act.programs.push(name);
    }
}

/// Counts one more open connection when fewer than `MAX_OPEN` are open, and says whether it did.
fn admit(open: &AtomicUsize) -> bool {
    if open.fetch_add(1, Ordering::SeqCst) >= MAX_OPEN {
        open.fetch_sub(1, Ordering::SeqCst);
        return false;
    }
    true
}

fn keep<T>(record: &Mutex<Vec<T>>, entry: T, unkept: &AtomicU64) {
    if let Ok(mut r) = record.lock() {
        if r.len() < MAX_RECORD {
            r.push(entry);
            return;
        }
    }
    unkept.fetch_add(1, Ordering::SeqCst);
}

/// Whether the other end of the relay's connection from `peer_port` belongs to exactly one process,
/// and that process to the tree of the browser `root`, read from the system's table while the
/// caller holds the accepted socket: on macOS the descriptors of the tree's processes and then of
/// every other process that this user may read, on Windows the table of TCP connections with owner
/// ids. The owner is read twice, before and after the tree, and must be the same process both
/// times; a pid whose creation time is not the one that the tree recorded is not in the tree. The
/// reads hold the start lock, so that no child of this process is between its fork and its exec,
/// when it holds a copy of every descriptor of this process.
pub fn check_peer(
    root: Option<&Proc>,
    peer_port: u16,
    relay_port: u16,
) -> Result<u32, ForeignPeer> {
    let _held = crate::start::lock();
    let foreign = |owners: usize, in_tree: usize, cause: &'static str| ForeignPeer {
        peer_port,
        owners,
        in_tree,
        cause,
    };
    let Some(root) = root else {
        return Err(foreign(0, 0, "no_browser"));
    };
    let first = owners(peer_port, relay_port, root);
    let table = procs::table().map_err(|_| foreign(0, 0, "table"))?;
    let tree: Vec<u32> = procs::descendants(&table, std::slice::from_ref(root))
        .into_iter()
        .filter(|p| table.iter().any(|t| procs::same(t, p)))
        .map(|p| p.pid)
        .collect();
    let in_tree = first.iter().filter(|p| tree.contains(p)).count();
    match first.as_slice() {
        [] => return Err(foreign(0, 0, "no_owner")),
        [one] if tree.contains(one) => {}
        [_] => return Err(foreign(1, 0, "outside_tree")),
        many => return Err(foreign(many.len(), in_tree, "several_owners")),
    }
    let again = owners(peer_port, relay_port, root);
    if again != first {
        return Err(foreign(again.len(), in_tree, "owner_changed"));
    }
    Ok(first[0])
}

#[cfg(target_os = "macos")]
fn owners(peer_port: u16, relay_port: u16, root: &Proc) -> Vec<u32> {
    let table = procs::table().unwrap_or_default();
    let tree: Vec<u32> = procs::descendants(&table, std::slice::from_ref(root))
        .iter()
        .map(|p| p.pid)
        .collect();
    let mut found = crate::peer::owners_of_connection(peer_port, relay_port, &tree);
    if found.len() == 1 {
        let me = std::process::id();
        let others: Vec<u32> = crate::peer::all_pids()
            .into_iter()
            .filter(|p| !tree.contains(p) && *p != me)
            .collect();
        found.extend(crate::peer::owners_of_connection(
            peer_port, relay_port, &others,
        ));
    }
    found.sort_unstable();
    found.dedup();
    found
}

#[cfg(not(target_os = "macos"))]
fn owners(peer_port: u16, relay_port: u16, _root: &Proc) -> Vec<u32> {
    crate::peer::owners_of_connection(peer_port, relay_port, &[])
}

/// Binds the relay's listener on a loopback port that the system picks: on macOS under the start
/// lock, because a socket is made close-on-exec only after it exists, so that no child that this
/// process starts inherits it; on Windows for the relay's exclusive use (`SO_EXCLUSIVEADDRUSE`),
/// so that no other socket binds the port while the listener lives or one of its connections
/// waits to close, and with no inheritance from its creation on.
fn bind_listener() -> std::io::Result<std::net::TcpListener> {
    #[cfg(windows)]
    {
        exclusive::bind_exclusive()
    }
    #[cfg(not(windows))]
    {
        let _held = crate::start::lock();
        std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
    }
}

#[cfg(windows)]
mod exclusive {
    use std::os::windows::io::FromRawSocket;
    use windows_sys::Win32::Networking::WinSock::{
        bind, closesocket, listen, setsockopt, WSASocketW, AF_INET, INADDR_LOOPBACK,
        INVALID_SOCKET, IN_ADDR, IN_ADDR_0, IPPROTO_TCP, SOCKADDR, SOCKADDR_IN, SOCK_STREAM,
        SOL_SOCKET, SO_EXCLUSIVEADDRUSE, WSA_FLAG_NO_HANDLE_INHERIT, WSA_FLAG_OVERLAPPED,
    };

    pub(super) fn bind_exclusive() -> std::io::Result<std::net::TcpListener> {
        // Starting std's socket layer first runs WSAStartup, which WSASocketW needs.
        drop(std::net::UdpSocket::bind("127.0.0.1:0"));
        // SAFETY: WSASocketW makes a new socket or returns INVALID_SOCKET; nothing is borrowed.
        let s = unsafe {
            WSASocketW(
                AF_INET as i32,
                SOCK_STREAM,
                IPPROTO_TCP,
                std::ptr::null(),
                0,
                WSA_FLAG_OVERLAPPED | WSA_FLAG_NO_HANDLE_INHERIT,
            )
        };
        if s == INVALID_SOCKET {
            return Err(std::io::Error::last_os_error());
        }
        let fail = |s| {
            let e = std::io::Error::last_os_error();
            // SAFETY: the socket is owned here and closed once.
            unsafe { closesocket(s) };
            Err(e)
        };
        let on: i32 = 1;
        // SAFETY: the option value is a live i32 of the size given.
        if unsafe {
            setsockopt(
                s,
                SOL_SOCKET,
                SO_EXCLUSIVEADDRUSE,
                &on as *const i32 as *const u8,
                4,
            )
        } != 0
        {
            return fail(s);
        }
        let addr = SOCKADDR_IN {
            sin_family: AF_INET,
            sin_port: 0,
            sin_addr: IN_ADDR {
                S_un: IN_ADDR_0 {
                    S_addr: INADDR_LOOPBACK.to_be(),
                },
            },
            sin_zero: [0; 8],
        };
        // SAFETY: `addr` is a live SOCKADDR_IN of the size given.
        if unsafe {
            bind(
                s,
                &addr as *const SOCKADDR_IN as *const SOCKADDR,
                std::mem::size_of::<SOCKADDR_IN>() as i32,
            )
        } != 0
        {
            return fail(s);
        }
        // SAFETY: the socket is bound above.
        if unsafe { listen(s, 128) } != 0 {
            return fail(s);
        }
        // SAFETY: the socket is open, owned here alone and moved into the listener.
        Ok(unsafe { std::net::TcpListener::from_raw_socket(s as u64) })
    }
}

/// The start flags that make the relay the browser's only route, each with what it closes.
pub fn relay_flags(port: u16) -> Vec<String> {
    vec![
        format!("--proxy-server=socks5://127.0.0.1:{port}"),
        "--proxy-bypass-list=<-loopback>".to_string(),
        "--disable-quic".to_string(),
        "--force-webrtc-ip-handling-policy=disable_non_proxied_udp".to_string(),
        RELAY_RESOLVER_RULE.to_string(),
    ]
}

/// The resolver rule of the trial with the relay's own address left out of `MAP * ~NOTFOUND`, so
/// that the browser can reach its proxy; it comes after the trial's rule, and the browser takes
/// the last copy of a switch.
pub const RELAY_RESOLVER_RULE: &str =
    "--host-resolver-rules=MAP *.test 127.0.0.1,EXCLUDE 127.0.0.1,MAP * ~NOTFOUND";

/// Where a SOCKS5 CONNECT may go: 127.0.0.1 for a name under `.test`, for `localhost` and for a
/// loopback address, and nowhere else.
pub fn destination(host: &str) -> Option<IpAddr> {
    let lower = host.to_ascii_lowercase();
    if lower.ends_with(".test") || lower == "localhost" {
        return Some(IpAddr::V4(Ipv4Addr::LOCALHOST));
    }
    match lower.parse::<IpAddr>() {
        Ok(ip) if ip.is_loopback() => Some(ip),
        _ => None,
    }
}

fn key(host: &str, port: u16) -> HostPort {
    (host.to_ascii_lowercase(), port)
}

impl Relay {
    /// Starts the relay on a loopback port that the system picks. Until `trust` names a browser,
    /// every connection is refused as foreign.
    pub async fn start() -> std::io::Result<Relay> {
        let std_listener = bind_listener()?;
        std_listener.set_nonblocking(true)?;
        #[cfg(unix)]
        let raw = std::os::fd::AsRawFd::as_raw_fd(&std_listener);
        #[cfg(windows)]
        let raw = std::os::windows::io::AsRawSocket::as_raw_socket(&std_listener) as usize;
        let listener = TcpListener::from_std(std_listener)?;
        let port = listener.local_addr()?.port();
        let shared = Arc::new(Shared {
            port,
            listener: Mutex::new(Some(raw)),
            ..Shared::default()
        });
        let serving = shared.clone();
        let task = tokio::spawn(async move {
            loop {
                let socket = match listener.accept().await {
                    Ok((socket, _)) => socket,
                    Err(_) => {
                        serving.accept_errors.fetch_add(1, Ordering::SeqCst);
                        tokio::time::sleep(ACCEPT_PAUSE).await;
                        continue;
                    }
                };
                let shared = serving.clone();
                if !admit(&shared.connections) {
                    shared.over_bound.fetch_add(1, Ordering::SeqCst);
                    drop(socket);
                    continue;
                }
                tokio::spawn(async move {
                    let _ = accept(socket, shared.clone()).await;
                    shared.connections.fetch_sub(1, Ordering::SeqCst);
                });
            }
        });
        Ok(Relay { port, task, shared })
    }

    pub fn handle(&self) -> RelayHandle {
        RelayHandle {
            shared: self.shared.clone(),
        }
    }

    pub fn tunnels(&self) -> u64 {
        self.handle().tunnels()
    }

    pub fn refused(&self) -> u64 {
        self.handle().refusals().len() as u64
    }

    pub fn stop(self) {
        self.close();
    }

    fn close(&self) {
        if let Ok(mut l) = self.shared.listener.lock() {
            *l = None;
        }
        self.task.abort();
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        self.close();
    }
}

impl RelayHandle {
    /// Starts the guard of the relay's port for the browser `root` (`guard_main`): `program` run
    /// with `GUARD_FLAG`, the root's pid and creation time, and the relay's listener placed in it
    /// by `start::start_with`, under the start lock. None when the relay is closed or the start
    /// failed.
    pub fn start_guard(
        &self,
        program: &std::path::Path,
        root: &Proc,
        record: &std::path::Path,
    ) -> Option<crate::start::Started> {
        let listener = self.shared.listener.lock().ok()?;
        let raw = (*listener)?;
        // the executor (this process) by number and start time, so that the guard ends the run when
        // the executor is gone (CP-148)
        let me = procs::own()?;
        let spec = crate::start::Spec {
            program,
            args: vec![
                GUARD_FLAG.into(),
                root.pid.to_string().into(),
                root.started_us.to_string().into(),
                me.pid.to_string().into(),
                me.started_us.to_string().into(),
                record.as_os_str().to_owned(),
            ],
            env: Vec::new(),
            stderr: None,
        };
        let held = crate::start::lock();
        #[cfg(unix)]
        let placed = raw;
        #[cfg(windows)]
        let placed = raw as crate::start::RawPlaced;
        crate::start::start_with(&held, &spec, placed).ok()
    }

    /// Names the first process of the browser whose tree the relay serves, or none: from now on a
    /// connection is accepted only from a process of that tree (`check_peer`).
    pub fn trust(&self, browser: Option<Proc>) {
        if let Ok(mut b) = self.shared.browser.lock() {
            *b = browser;
        }
    }

    /// The connections that the relay refused as foreign, in order, at most `MAX_RECORD`.
    pub fn foreign(&self) -> Vec<ForeignPeer> {
        self.shared
            .foreign
            .lock()
            .map(|f| f.clone())
            .unwrap_or_default()
    }

    /// How many connections the relay refused as foreign, kept or not.
    pub fn foreign_count(&self) -> u64 {
        self.shared.foreign_count.load(Ordering::SeqCst)
    }

    /// How many connections the relay closed at their accept because `MAX_OPEN` were open, and how
    /// many entries its records did not keep because of `MAX_RECORD`.
    pub fn over_bounds(&self) -> (u64, u64) {
        (
            self.shared.over_bound.load(Ordering::SeqCst),
            self.shared.unkept.load(Ordering::SeqCst),
        )
    }

    /// Sets the list to exactly `entries`, or takes the list away with `None` (every name that
    /// `destination` takes passes). Every open tunnel to a host and port that the new list does not
    /// hold is closed.
    pub fn set_list(&self, entries: Option<Vec<HostPort>>) {
        let list: Option<BTreeSet<HostPort>> =
            entries.map(|e| e.into_iter().map(|(h, p)| key(&h, p)).collect());
        if let Ok(mut l) = self.shared.list.lock() {
            *l = list.clone();
        }
        if let Some(list) = list {
            self.close_outside(&list);
        }
    }

    /// Adds one host and port to the list; with no list, it starts one that holds it alone.
    pub fn allow(&self, host: &str, port: u16) {
        if let Ok(mut l) = self.shared.list.lock() {
            l.get_or_insert_with(BTreeSet::new).insert(key(host, port));
        }
    }

    /// Takes one host and port out of the list and closes its open tunnels.
    pub fn remove(&self, host: &str, port: u16) {
        let list = match self.shared.list.lock() {
            Ok(mut l) => {
                let set = l.get_or_insert_with(BTreeSet::new);
                set.remove(&key(host, port));
                set.clone()
            }
            Err(_) => return,
        };
        self.close_outside(&list);
    }

    fn close_outside(&self, list: &BTreeSet<HostPort>) {
        let leaving: Vec<(u64, HostPort, AbortHandle)> = match self.shared.open.lock() {
            Ok(mut open) => {
                let ids: Vec<u64> = open
                    .iter()
                    .filter(|(_, (hp, _))| !list.contains(hp))
                    .map(|(id, _)| *id)
                    .collect();
                ids.into_iter()
                    .filter_map(|id| open.remove(&id).map(|(hp, a)| (id, hp, a)))
                    .collect()
            }
            Err(_) => return,
        };
        for (_, hp, abort) in leaving {
            abort.abort();
            keep(&self.shared.closed, hp, &self.shared.unkept);
        }
    }

    /// Every refusal, in order, by host and port.
    pub fn refusals(&self) -> Vec<HostPort> {
        self.shared
            .refusals
            .lock()
            .map(|r| r.clone())
            .unwrap_or_default()
    }

    /// The refusals of one host and port.
    pub fn refusals_of(&self, host: &str, port: u16) -> usize {
        let k = key(host, port);
        self.refusals().iter().filter(|r| **r == k).count()
    }

    /// The tunnels that the relay closed because their host left the list, in order.
    pub fn closed(&self) -> Vec<HostPort> {
        self.shared
            .closed
            .lock()
            .map(|c| c.clone())
            .unwrap_or_default()
    }

    /// How many connections the relay holds open now (at most `MAX_OPEN`).
    pub fn connections(&self) -> usize {
        self.shared.connections.load(Ordering::SeqCst)
    }

    /// The host and port of every tunnel that is open now.
    pub fn open_tunnels(&self) -> Vec<HostPort> {
        self.shared
            .open
            .lock()
            .map(|o| o.values().map(|(hp, _)| hp.clone()).collect())
            .unwrap_or_default()
    }

    pub fn tunnels(&self) -> u64 {
        self.shared.tunnels.load(Ordering::SeqCst)
    }

    fn listed(&self, hp: &HostPort) -> bool {
        match self.shared.list.lock() {
            Ok(l) => l.as_ref().map(|set| set.contains(hp)).unwrap_or(true),
            Err(_) => false,
        }
    }
}

async fn read_exact_in_time(socket: &mut TcpStream, buf: &mut [u8]) -> std::io::Result<()> {
    match tokio::time::timeout(HANDSHAKE_WAIT, socket.read_exact(buf)).await {
        Ok(r) => r.map(|_| ()),
        Err(_) => Err(std::io::ErrorKind::TimedOut.into()),
    }
}

async fn refuse(socket: &mut TcpStream, shared: &Shared, hp: HostPort, code: u8) {
    keep(&shared.refusals, hp, &shared.unkept);
    let _ = socket.write_all(&[5, code, 0, 1, 0, 0, 0, 0, 0, 0]).await;
}

/// Checks the peer of a new connection while it holds the socket, before it reads a byte: a
/// foreign peer is recorded apart and its socket closed unread.
async fn accept(socket: TcpStream, shared: Arc<Shared>) -> std::io::Result<()> {
    let peer_port = socket.peer_addr()?.port();
    let root = shared.browser.lock().ok().and_then(|b| b.clone());
    let relay_port = shared.port;
    let verdict =
        tokio::task::spawn_blocking(move || check_peer(root.as_ref(), peer_port, relay_port))
            .await
            .unwrap_or(Err(ForeignPeer {
                peer_port,
                owners: 0,
                in_tree: 0,
                cause: "check",
            }));
    match verdict {
        Ok(_) => tunnel(socket, shared).await,
        Err(f) => {
            shared.foreign_count.fetch_add(1, Ordering::SeqCst);
            keep(&shared.foreign, f, &shared.unkept);
            drop(socket);
            Ok(())
        }
    }
}

async fn tunnel(mut socket: TcpStream, shared: Arc<Shared>) -> std::io::Result<()> {
    let mut head = [0u8; 2];
    read_exact_in_time(&mut socket, &mut head).await?;
    if head[0] != 5 {
        return Ok(());
    }
    let mut methods = vec![0u8; head[1] as usize];
    read_exact_in_time(&mut socket, &mut methods).await?;
    if !methods.contains(&0) {
        socket.write_all(&[5, 0xff]).await?;
        return Ok(());
    }
    socket.write_all(&[5, 0]).await?;
    let mut request = [0u8; 4];
    read_exact_in_time(&mut socket, &mut request).await?;
    let host = match request[3] {
        1 => {
            let mut a = [0u8; 4];
            read_exact_in_time(&mut socket, &mut a).await?;
            Ipv4Addr::from(a).to_string()
        }
        3 => {
            let mut len = [0u8; 1];
            read_exact_in_time(&mut socket, &mut len).await?;
            let mut name = vec![0u8; len[0] as usize];
            read_exact_in_time(&mut socket, &mut name).await?;
            String::from_utf8_lossy(&name).into_owned()
        }
        4 => {
            let mut a = [0u8; 16];
            read_exact_in_time(&mut socket, &mut a).await?;
            std::net::Ipv6Addr::from(a).to_string()
        }
        _ => String::new(),
    };
    let mut port = [0u8; 2];
    read_exact_in_time(&mut socket, &mut port).await?;
    let port = u16::from_be_bytes(port);
    let hp = key(&host, port);
    let handle = RelayHandle {
        shared: shared.clone(),
    };
    if request[1] != 1 || !handle.listed(&hp) {
        refuse(&mut socket, &shared, hp, 2).await;
        return Ok(());
    }
    let Some(ip) = destination(&host) else {
        refuse(&mut socket, &shared, hp, 2).await;
        return Ok(());
    };
    let mut upstream = match TcpStream::connect(SocketAddr::new(ip, port)).await {
        Ok(s) => s,
        Err(_) => {
            socket.write_all(&[5, 5, 0, 1, 0, 0, 0, 0, 0, 0]).await?;
            return Ok(());
        }
    };
    #[cfg(test)]
    tests::connect_pause(port).await;
    if !handle.listed(&hp) {
        refuse(&mut socket, &shared, hp, 2).await;
        return Ok(());
    }
    shared.tunnels.fetch_add(1, Ordering::SeqCst);
    socket.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).await?;
    let id = shared.next.fetch_add(1, Ordering::SeqCst);
    let copy = tokio::spawn(async move {
        let _ = tokio::io::copy_bidirectional(&mut socket, &mut upstream).await;
    });
    if let Ok(mut open) = shared.open.lock() {
        open.insert(id, (hp, copy.abort_handle()));
    }
    let _ = copy.await;
    if let Ok(mut open) = shared.open.lock() {
        open.remove(&id);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A guard run on stand-in tables: `table(t)` gives the table at `t` ms (None: unread), the
    /// pause adds `step` ms, and each end of a process calls `ended(p, force, t)`, which may change
    /// what later tables hold. Returns the exit, the ms at the exit, the ends and the record.
    #[allow(clippy::type_complexity)]
    fn run_guard(
        executor: Option<(u32, u64)>,
        step: u128,
        table: &dyn Fn(u128, &[(u32, bool, u128)]) -> Option<Vec<Proc>>,
    ) -> (
        i32,
        u128,
        Vec<(u32, bool, u128)>,
        Option<(u128, usize, usize)>,
    ) {
        let clock = std::cell::Cell::new(0u128);
        let ends = std::cell::RefCell::new(Vec::new());
        let mut record = None;
        let mut reads = 0u64;
        let code = guard_loop(
            999_999,
            7,
            executor,
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
                record: &mut |act| record = Some((act.ms, act.asked, act.forced)),
            },
        );
        (code, clock.get(), ends.into_inner(), record)
    }

    fn row(pid: u32, started_us: u64) -> Proc {
        Proc {
            pid,
            ppid: 1,
            started_us,
            exe: None,
        }
    }

    /// The coordinator's point on an unread table: a guard whose read of the process table fails
    /// once keeps the port, reads again, and ends only when the browser's first process is gone.
    #[test]
    fn a_table_that_could_not_be_read_once_is_no_empty_table_for_the_guard() {
        let (code, _, ends, _) = run_guard(None, 20, &|t, _| match t {
            0 => None,
            1..=100 => Some(vec![row(999_999, 7)]),
            _ => Some(Vec::new()),
        });
        assert_eq!(code, 0);
        assert!(ends.is_empty(), "{ends:?}");
    }

    /// A table that stays unread ends the guard at the watch's cap with its own cause.
    #[test]
    fn a_table_that_stays_unread_ends_the_guard_at_the_cap() {
        let (code, at, _, _) = run_guard(None, 2, &|_, _| None);
        assert_eq!(code, 3);
        assert!(at >= crate::killwatch::CAP.as_millis(), "ended at {at} ms");
    }

    /// CP-148: the executor gone and a browser that never ends by itself: after the grace the guard
    /// asks it to end, after the wait it ends it by force, and it frees the port only after the tree
    /// is gone and its margin has passed.
    #[test]
    fn a_browser_that_never_ends_is_ended_after_the_grace_and_the_port_freed_after_it() {
        // the executor (pid 5) runs until 1,000 ms; the browser goes only when it is forced
        let (code, at, ends, record) = run_guard(Some((5, 50)), 10, &|t, ends| {
            let mut v = Vec::new();
            if !ends.iter().any(|(p, force, _)| *p == 999_999 && *force) {
                v.push(row(999_999, 7));
            }
            if t < 1_000 {
                v.push(row(5, 50));
            }
            Some(v)
        });
        assert_eq!(code, 0);
        let asked = ends
            .iter()
            .find(|e| !e.1)
            .expect("the guard asked the browser to end");
        let forced = ends
            .iter()
            .find(|e| e.1)
            .expect("the guard ended the browser by force");
        assert!(asked.2 >= 1_000 + 3_000, "asked at {} ms", asked.2);
        assert!(forced.2 >= asked.2 + 2_000, "forced at {} ms", forced.2);
        assert!(at >= forced.2 + 1_000, "the port freed at {at} ms");
        let (ms, a, f) = record.expect("the guard recorded its act");
        assert!(ms >= 3_000 && a == 1 && f == 1, "{ms} {a} {f}");
        assert_eq!(
            (GRACE, TERM_WAIT, PORT_MARGIN),
            (
                Duration::from_millis(3_000),
                Duration::from_millis(2_000),
                Duration::from_millis(1_000)
            )
        );
    }

    /// CP-148: a browser that ends by itself inside the grace is left to itself.
    #[test]
    fn a_browser_that_ends_inside_the_grace_is_left_to_itself() {
        let (code, _, ends, record) = run_guard(Some((5, 50)), 10, &|t, _| {
            let mut v = Vec::new();
            if t < 1_000 + 2_500 {
                v.push(row(999_999, 7));
            }
            if t < 1_000 {
                v.push(row(5, 50));
            }
            Some(v)
        });
        assert_eq!(code, 0);
        assert!(
            ends.is_empty(),
            "the guard ended a browser that ended by itself: {ends:?}"
        );
        assert!(record.is_none());
    }

    /// CP-148: while the executor runs the guard ends nothing, however long the browser runs.
    #[test]
    fn the_guard_ends_nothing_while_its_executor_runs() {
        let (code, _, ends, _) = run_guard(Some((5, 50)), 10, &|t, _| {
            let mut v = vec![row(5, 50)];
            if t < 60_000 {
                v.push(row(999_999, 7));
            }
            Some(v)
        });
        assert_eq!(code, 0);
        assert!(ends.is_empty(), "{ends:?}");
    }

    /// CP-148 (2): a number of the tree that another process took is left alone: the browser ends,
    /// and the same pid comes back with another start time while the executor is gone.
    #[test]
    fn a_number_of_the_tree_that_another_process_took_is_left_alone() {
        let (code, _, ends, _) = run_guard(Some((5, 50)), 10, &|t, _| {
            let mut v = Vec::new();
            if t < 2_000 {
                v.push(row(999_999, 7));
            } else {
                // another process with the browser's number
                v.push(row(999_999, 8));
            }
            if t < 1_000 {
                v.push(row(5, 50));
            }
            Some(v)
        });
        assert_eq!(code, 0);
        assert!(
            ends.is_empty(),
            "the guard ended a process that took the tree's number: {ends:?}"
        );
    }

    /// CP-161 F1, the fence: the one way from a test's end closure to an ask or a forced end. `own`
    /// is the test's own stand-in browser, its number from the child's handle and its start time read
    /// once after the start. A call on that process goes on to `ask`; every other call sends nothing,
    /// is recorded in `refused` with its number, start time and force flag, and comes back as an error
    /// that holds the record.
    fn fence(
        own: (u32, u64),
        refused: &std::cell::RefCell<Vec<(u32, u64, bool)>>,
        p: &Proc,
        force: bool,
        ask: &mut dyn FnMut(&Proc, bool) -> bool,
    ) -> Result<bool, String> {
        if p.pid == own.0 && p.started_us == own.1 {
            return Ok(ask(p, force));
        }
        refused.borrow_mut().push((p.pid, p.started_us, force));
        Err(format!(
            "the fence refused an end of pid={} started_us={} force={force}; refused: {:?}",
            p.pid,
            p.started_us,
            refused.borrow()
        ))
    }

    /// CP-161 F1: the fence passes on a call on the own stand-in alone, by number and start time; a
    /// call on another number with another start time (the executor's stand-in), on the stand-in's
    /// number with another start time, or on another number with the stand-in's start time sends
    /// nothing and is recorded.
    #[test]
    fn the_fence_passes_on_the_own_stand_in_alone_and_records_every_other_call() {
        let own = (100_001, 7);
        let refused = std::cell::RefCell::new(Vec::new());
        let mut asked = Vec::new();
        let mut errors = Vec::new();
        {
            let mut ask = |p: &Proc, force: bool| {
                asked.push((p.pid, p.started_us, force));
                true
            };
            assert_eq!(
                fence(own, &refused, &row(100_001, 7), false, &mut ask),
                Ok(true)
            );
            assert_eq!(
                fence(own, &refused, &row(100_001, 7), true, &mut ask),
                Ok(true)
            );
            for (p, force) in [
                (row(100_005, 50), false),
                (row(100_001, 8), true),
                (row(100_002, 7), false),
            ] {
                match fence(own, &refused, &p, force, &mut ask) {
                    Ok(sent) => panic!("the fence passed on {p:?} force={force}: {sent}"),
                    Err(e) => errors.push(e),
                }
            }
        }
        assert_eq!(asked, [(100_001, 7, false), (100_001, 7, true)]);
        assert_eq!(
            refused.into_inner(),
            [
                (100_005, 50, false),
                (100_001, 8, true),
                (100_002, 7, false)
            ]
        );
        assert!(
            errors[1].contains("pid=100001 started_us=8 force=true"),
            "{errors:?}"
        );
    }

    /// CP-161 F1: the guard over a built table that holds the stand-in browser and three processes
    /// of no tie to its tree (one with no tie at all, one whose parent number names the browser but
    /// which started before it, one child of the executor) and the executor until 1,000 ms; its end
    /// goes through the fence to an ask that records. The guard asks the stand-in alone, at 4,000 ms;
    /// the fence refuses nothing, and the guard frees the port once the stand-in is gone.
    #[test]
    fn the_guard_on_a_built_table_ends_the_stand_in_alone_through_the_fence() {
        let own = (100_001u32, 7u64);
        let clock = std::cell::Cell::new(0u128);
        let refused = std::cell::RefCell::new(Vec::new());
        let asked = std::cell::RefCell::new(Vec::new());
        let others = [
            Proc {
                pid: 100_002,
                ppid: 1,
                started_us: 9,
                exe: None,
            },
            Proc {
                pid: 100_003,
                ppid: 100_001,
                started_us: 3,
                exe: None,
            },
            Proc {
                pid: 100_006,
                ppid: 100_005,
                started_us: 60,
                exe: None,
            },
        ];
        let mut reads = 0u64;
        let code = guard_loop(
            own.0,
            own.1,
            Some((100_005, 50)),
            GuardIo {
                read: &mut || {
                    reads += 1;
                    assert!(reads < 100_000, "the guard reads on without end");
                    let mut v = others.to_vec();
                    // the stand-in ends when it is asked
                    if asked.borrow().is_empty() {
                        v.push(row(100_001, 7));
                    }
                    if clock.get() < 1_000 {
                        v.push(row(100_005, 50));
                    }
                    Some(v)
                },
                now_ms: &mut || clock.get(),
                pause: &mut |_| clock.set(clock.get() + 10),
                end: &mut |p, force| {
                    fence(own, &refused, p, force, &mut |p, force| {
                        asked
                            .borrow_mut()
                            .push((p.pid, p.started_us, force, clock.get()));
                        true
                    })
                    .unwrap_or_else(|record| panic!("{record}"))
                },
                record: &mut |_| {},
            },
        );
        assert_eq!(code, 0);
        assert!(refused.borrow().is_empty(), "{:?}", refused.borrow());
        assert_eq!(asked.into_inner(), [(100_001, 7, false, 4_000)]);
        assert_eq!(clock.get(), 5_010);
    }

    /// CP-161 F3 (V2): a member born after the first process ended is followed. The executor is gone
    /// from the start; the first process ends by itself at 1,000 ms; its child ends only by force; a
    /// grandchild born at 4,000 ms, after the ask, ends only by force. The guard ends the grandchild
    /// by force at 5,000 ms and keeps the port until it is gone.
    #[test]
    fn a_grandchild_born_after_the_first_process_ended_is_ended_and_the_port_kept_until_it_is_gone()
    {
        let child = || Proc {
            pid: 100_010,
            ppid: 999_999,
            started_us: 10,
            exe: None,
        };
        let grandchild = || Proc {
            pid: 100_011,
            ppid: 100_010,
            started_us: 4_000_000,
            exe: None,
        };
        let forced = |ends: &[(u32, bool, u128)], pid: u32| ends.iter().any(|e| e.0 == pid && e.1);
        let (code, at, ends, _) = run_guard(Some((5, 50)), 10, &|t, ends| {
            let mut v = Vec::new();
            if t < 1_000 {
                v.push(row(999_999, 7));
            }
            if !forced(ends, 100_010) {
                v.push(child());
            }
            if t >= 4_000 && !forced(ends, 100_011) {
                v.push(grandchild());
            }
            Some(v)
        });
        assert_eq!(code, 0);
        assert!(
            ends.contains(&(100_011, true, 5_000)),
            "the grandchild was not ended by force at 5,000 ms: {ends:?}"
        );
        assert!(ends.contains(&(100_010, true, 5_000)), "{ends:?}");
        assert_eq!(at, 6_010, "the port freed at {at} ms");
    }

    /// CP-161 F3 (V5): the port is kept while a member runs, also when the first process is gone. The
    /// executor runs throughout; the first process ends by itself at 2,000 ms, its child at 8,000 ms.
    /// The guard ends nothing and frees the port at 9,000 ms.
    #[test]
    fn the_port_is_kept_while_a_member_runs_after_the_first_process_is_gone() {
        let (code, at, ends, _) = run_guard(Some((5, 50)), 10, &|t, _| {
            let mut v = vec![row(5, 50)];
            if t < 2_000 {
                v.push(row(999_999, 7));
            }
            if t < 8_000 {
                v.push(Proc {
                    pid: 100_010,
                    ppid: 999_999,
                    started_us: 10,
                    exe: None,
                });
            }
            Some(v)
        });
        assert_eq!(code, 0);
        assert!(ends.is_empty(), "{ends:?}");
        assert_eq!(at, 9_000, "the port freed at {at} ms");
    }

    /// CP-161 F3 (V7, the review's G1): the executor is known by its number and its start time
    /// together. The executor (5, started 50) is gone at 1,000 ms and another process takes its
    /// number (5, started 60); the browser ends when it is asked, or by itself at 60,000 ms. The
    /// guard asks it at 4,000 ms, after its grace.
    #[test]
    fn a_process_that_took_the_executors_number_is_not_the_executor() {
        let (code, at, ends, _) = run_guard(Some((5, 50)), 10, &|t, ends| {
            let mut v = vec![if t < 1_000 { row(5, 50) } else { row(5, 60) }];
            if ends.is_empty() && t < 60_000 {
                v.push(row(999_999, 7));
            }
            Some(v)
        });
        assert_eq!(code, 0);
        assert_eq!(ends, [(999_999, false, 4_000)]);
        assert_eq!(at, 5_010, "the port freed at {at} ms");
    }

    /// CP-161 F3 (the builder's G1): a table that could not be read is no empty table. It stays
    /// unread for 1,500 ms, longer than the port's margin, while the browser runs; the browser ends
    /// by itself at 3,000 ms, and the guard frees the port at 4,000 ms. The verifier's S7: unread to
    /// 1,000 ms, then the tree gone: the port freed at 2,000 ms.
    #[test]
    fn a_table_unread_longer_than_the_margin_keeps_the_port_while_the_browser_runs() {
        let (code, at, ends, _) = run_guard(None, 10, &|t, _| match t {
            0..=1_499 => None,
            1_500..=2_999 => Some(vec![row(999_999, 7)]),
            _ => Some(Vec::new()),
        });
        assert_eq!((code, at), (0, 4_000));
        assert!(ends.is_empty(), "{ends:?}");
        let (code, at, _, _) = run_guard(None, 10, &|t, _| {
            if t < 1_000 {
                None
            } else {
                Some(Vec::new())
            }
        });
        assert_eq!((code, at), (0, 2_000));
    }

    /// CP-161 F1 (d): the test's own stand-ins, each ended through its handle and reaped when this
    /// goes out of scope, on every way out of the test, a failed one too. A handle that was reaped
    /// already sends nothing.
    #[cfg(target_os = "macos")]
    struct OwnStandIns(Vec<Arc<Mutex<std::process::Child>>>);

    #[cfg(target_os = "macos")]
    impl Drop for OwnStandIns {
        fn drop(&mut self) {
            for c in &self.0 {
                let mut c = c.lock().unwrap_or_else(|e| e.into_inner());
                let _ = c.kill();
                let _ = c.wait();
            }
        }
    }

    /// CP-148 on real processes (macOS): a stand-in browser that never ends by itself and a stand-in
    /// executor; the executor is killed; the guard asks the browser to end after its grace, and
    /// returns only after the browser is gone. CP-161 F1: the end reaches the ask and the forced end
    /// through the fence alone, which passes on a call on the stand-in browser alone (its number from
    /// the child's handle, its start time read once after the start) and fails the test at any other;
    /// the test fails inside its read or its pause when the guard has not returned 20 s after the
    /// test's start; both stand-ins are ended on every way out.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_guard_ends_a_real_stand_in_after_its_executor_is_killed() {
        use crate::start::Start;
        let test_start = std::time::Instant::now();
        let bound = Duration::from_secs(20);
        let browser = Arc::new(Mutex::new(
            std::process::Command::new("/bin/sleep")
                .arg("60")
                .start()
                .unwrap(),
        ));
        let mut own = OwnStandIns(vec![browser.clone()]);
        let executor = Arc::new(Mutex::new(
            std::process::Command::new("/bin/sleep")
                .arg("60")
                .start()
                .unwrap(),
        ));
        own.0.push(executor.clone());
        let browser_pid = browser.lock().unwrap().id();
        let executor_pid = executor.lock().unwrap().id();
        std::thread::sleep(Duration::from_millis(300));
        let table = procs::table().unwrap();
        let b = table.iter().find(|p| p.pid == browser_pid).unwrap().clone();
        let e = table
            .iter()
            .find(|p| p.pid == executor_pid)
            .unwrap()
            .clone();
        let start = std::time::Instant::now();
        let killer = {
            let executor = executor.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(500));
                let mut executor = executor.lock().unwrap_or_else(|e| e.into_inner());
                let _ = executor.kill();
                let _ = executor.wait();
            })
        };
        let refused = std::cell::RefCell::new(Vec::new());
        let mut record = None;
        let code = guard_loop(
            b.pid,
            b.started_us,
            Some((e.pid, e.started_us)),
            GuardIo {
                read: &mut || {
                    assert!(
                        test_start.elapsed() < bound,
                        "the guard has not returned {bound:?} after the test's start"
                    );
                    // the stand-in is this test's child: reap it once it ended, as its parent would
                    let _ = browser.lock().unwrap().try_wait();
                    procs::table().ok()
                },
                now_ms: &mut || start.elapsed().as_millis(),
                pause: &mut |_| {
                    assert!(
                        test_start.elapsed() < bound,
                        "the guard has not returned {bound:?} after the test's start"
                    );
                    std::thread::sleep(GUARD_POLL)
                },
                end: &mut |p, force| {
                    fence(
                        (browser_pid, b.started_us),
                        &refused,
                        p,
                        force,
                        &mut |p, force| {
                            if force {
                                procs::kill(p)
                            } else {
                                procs::ask_end(p)
                            }
                        },
                    )
                    .unwrap_or_else(|record| panic!("{record}"))
                },
                record: &mut |act| record = Some(act.clone()),
            },
        );
        let took = start.elapsed();
        killer.join().unwrap();
        let status = browser.lock().unwrap().wait().unwrap();
        drop(own);
        assert_eq!(code, 0);
        assert!(refused.borrow().is_empty(), "{:?}", refused.borrow());
        assert!(
            took >= Duration::from_millis(500) + GRACE,
            "returned after {took:?}"
        );
        assert!(took < Duration::from_secs(20), "returned after {took:?}");
        assert!(!procs::running(&b), "the stand-in browser still runs");
        assert!(!status.success(), "{status:?}");
        let act = record.expect("the guard recorded its act");
        assert!(
            act.first,
            "the guard did not name the stand-in browser: {act:?}"
        );
        assert_eq!(act.programs, ["sleep"]);
    }

    #[test]
    fn the_relay_sends_test_names_and_loopback_to_loopback_and_nothing_else() {
        assert_eq!(
            destination("utility.agent.test"),
            Some(IpAddr::V4(Ipv4Addr::LOCALHOST))
        );
        assert_eq!(
            destination("EVIL.AGENT.TEST"),
            Some(IpAddr::V4(Ipv4Addr::LOCALHOST))
        );
        assert_eq!(
            destination("127.0.0.1"),
            Some(IpAddr::V4(Ipv4Addr::LOCALHOST))
        );
        for other in [
            "example.com",
            "agent.test.example.com",
            "10.0.0.1",
            "",
            "test",
        ] {
            assert_eq!(destination(other), None, "{other}");
        }
    }

    #[test]
    fn the_flags_make_the_relay_the_only_route() {
        let flags = relay_flags(18999);
        assert_eq!(flags[0], "--proxy-server=socks5://127.0.0.1:18999");
        assert!(flags.contains(&"--proxy-bypass-list=<-loopback>".to_string()));
        assert!(flags.contains(&"--disable-quic".to_string()));
    }

    /// A connection to `port` whose socket no child that another test starts can inherit: on macOS
    /// a socket is made close-on-exec only after it exists, so it is made under the start lock.
    fn client(port: u16) -> TcpStream {
        let c = {
            let _held = crate::start::lock();
            std::net::TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap()
        };
        c.set_nonblocking(true).unwrap();
        TcpStream::from_std(c).unwrap()
    }

    fn me() -> Proc {
        procs::table()
            .unwrap()
            .into_iter()
            .find(|p| p.pid == std::process::id())
            .unwrap()
    }

    #[tokio::test]
    async fn a_peer_outside_the_trusted_tree_is_refused_unread_and_recorded_apart() {
        let relay = Relay::start().await.unwrap();
        let (echo, accepted) = echo_server().await;
        for trusted in [
            None,
            Some(Proc {
                pid: 1,
                ppid: 0,
                started_us: 1,
                exe: None,
            }),
        ] {
            relay.handle().trust(trusted);
            let mut c = client(relay.port);
            let mut req = vec![5, 1, 0, 5, 1, 0, 3, 14];
            req.extend_from_slice(b"utility.a.test");
            req.extend_from_slice(&echo.to_be_bytes());
            let _ = c.write_all(&req).await;
            let mut got = Vec::new();
            let read = tokio::time::timeout(Duration::from_secs(5), c.read_to_end(&mut got)).await;
            assert!(matches!(read, Ok(Ok(0)) | Ok(Err(_))), "{read:?}");
            assert!(got.is_empty());
        }
        let foreign = relay.handle().foreign();
        assert_eq!(foreign.len(), 2);
        assert_eq!(foreign[0].cause, "no_browser");
        assert!(
            ["no_owner", "outside_tree"].contains(&foreign[1].cause),
            "{:?}",
            foreign[1]
        );
        assert_eq!(relay.handle().foreign_count(), 2);
        assert!(relay.handle().refusals().is_empty());
        assert_eq!(accepted.load(Ordering::SeqCst), 0);
        relay.handle().trust(Some(me()));
        let (_c, code) = connect_via(relay.port, "utility.a.test", echo).await;
        assert_eq!(code, 0);
        assert_eq!(relay.handle().foreign_count(), 2);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_connection_that_a_second_process_also_holds_is_refused_as_several_owners() {
        let held = crate::start::lock();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let relay_port = listener.local_addr().unwrap().port();
        let c = std::net::TcpStream::connect(("127.0.0.1", relay_port)).unwrap();
        let _accepted = listener.accept().unwrap();
        let spec = crate::start::Spec {
            program: std::path::Path::new("/bin/sleep"),
            args: vec!["5".into()],
            env: Vec::new(),
            stderr: None,
        };
        let mut second =
            crate::start::start_with(&held, &spec, std::os::fd::AsRawFd::as_raw_fd(&c)).unwrap();
        drop(held);
        std::thread::sleep(Duration::from_millis(300));
        let peer = c.local_addr().unwrap().port();
        assert_eq!(
            check_peer(Some(&me()), peer, relay_port).map_err(|f| (f.cause, f.owners)),
            Err(("several_owners", 2))
        );
        second.kill();
        assert_eq!(
            check_peer(Some(&me()), peer, relay_port),
            Ok(std::process::id())
        );
    }

    #[cfg(windows)]
    #[test]
    fn no_other_socket_binds_the_relay_s_port_while_its_listener_lives() {
        let listener = bind_listener().unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(std::net::TcpListener::bind(("127.0.0.1", port)).is_err());
        use windows_sys::Win32::Networking::WinSock::{
            bind, closesocket, setsockopt, socket, AF_INET, INADDR_LOOPBACK, IN_ADDR, IN_ADDR_0,
            IPPROTO_TCP, SOCKADDR, SOCKADDR_IN, SOCK_STREAM, SOL_SOCKET, SO_REUSEADDR,
        };
        let on: i32 = 1;
        let addr = SOCKADDR_IN {
            sin_family: AF_INET,
            sin_port: port.to_be(),
            sin_addr: IN_ADDR {
                S_un: IN_ADDR_0 {
                    S_addr: INADDR_LOOPBACK.to_be(),
                },
            },
            sin_zero: [0; 8],
        };
        // SAFETY: a new socket with SO_REUSEADDR tries the port and is closed once.
        let bound = unsafe {
            let s = socket(AF_INET as i32, SOCK_STREAM, IPPROTO_TCP);
            setsockopt(
                s,
                SOL_SOCKET,
                SO_REUSEADDR,
                &on as *const i32 as *const u8,
                4,
            );
            let r = bind(
                s,
                &addr as *const SOCKADDR_IN as *const SOCKADDR,
                std::mem::size_of::<SOCKADDR_IN>() as i32,
            );
            closesocket(s);
            r == 0
        };
        assert!(!bound, "a socket with SO_REUSEADDR bound the relay's port");
        drop(listener);
    }

    #[test]
    fn the_relay_admits_max_open_connections_and_keeps_max_record_entries() {
        let open = AtomicUsize::new(0);
        for _ in 0..MAX_OPEN {
            assert!(admit(&open));
        }
        assert!(!admit(&open));
        assert_eq!(open.load(Ordering::SeqCst), MAX_OPEN);
        open.fetch_sub(1, Ordering::SeqCst);
        assert!(admit(&open));
        let record = Mutex::new(Vec::new());
        let unkept = AtomicU64::new(0);
        for i in 0..MAX_RECORD + 3 {
            keep(&record, i, &unkept);
        }
        assert_eq!(record.lock().unwrap().len(), MAX_RECORD);
        assert_eq!(unkept.load(Ordering::SeqCst), 3);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_second_holder_outside_the_tree_is_read_and_refused() {
        let held = crate::start::lock();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let relay_port = listener.local_addr().unwrap().port();
        let c = std::net::TcpStream::connect(("127.0.0.1", relay_port)).unwrap();
        let _accepted = listener.accept().unwrap();
        let spec = crate::start::Spec {
            program: std::path::Path::new("/bin/sleep"),
            args: vec!["5".into()],
            env: Vec::new(),
            stderr: None,
        };
        let fd = std::os::fd::AsRawFd::as_raw_fd(&c);
        let mut inside = crate::start::start_with(&held, &spec, fd).unwrap();
        let mut outside = crate::start::start_with(&held, &spec, fd).unwrap();
        drop(held);
        std::thread::sleep(Duration::from_millis(300));
        let root = procs::table()
            .unwrap()
            .into_iter()
            .find(|p| p.pid == inside.pid())
            .unwrap();
        let peer = c.local_addr().unwrap().port();
        assert_eq!(
            check_peer(Some(&root), peer, relay_port).map_err(|f| (f.cause, f.in_tree)),
            Err(("several_owners", 1))
        );
        outside.kill();
        assert_eq!(check_peer(Some(&root), peer, relay_port), Ok(inside.pid()));
        inside.kill();
    }

    #[test]
    fn the_peer_check_takes_this_process_for_its_own_connection_and_no_other_root() {
        let held = crate::start::lock();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let relay_port = listener.local_addr().unwrap().port();
        let c = std::net::TcpStream::connect(("127.0.0.1", relay_port)).unwrap();
        drop(held);
        let _held = listener.accept().unwrap();
        let peer = c.local_addr().unwrap().port();
        assert_eq!(
            check_peer(Some(&me()), peer, relay_port),
            Ok(std::process::id())
        );
        let mut stale = me();
        stale.started_us += 1;
        assert_eq!(
            check_peer(Some(&stale), peer, relay_port).map_err(|f| f.cause),
            Err("outside_tree")
        );
        assert_eq!(
            check_peer(None, peer, relay_port).map_err(|f| f.cause),
            Err("no_browser")
        );
        assert_eq!(
            check_peer(Some(&me()), peer, relay_port.wrapping_add(1)).map_err(|f| f.cause),
            Err("no_owner")
        );
        assert_eq!((MAX_OPEN, MAX_RECORD), (512, 4096));
    }

    async fn connect_via(relay: u16, host: &str, port: u16) -> (TcpStream, u8) {
        let mut c = client(relay);
        c.write_all(&[5, 1, 0]).await.unwrap();
        let mut r = [0u8; 2];
        c.read_exact(&mut r).await.unwrap();
        let mut req = vec![5, 1, 0, 3, host.len() as u8];
        req.extend_from_slice(host.as_bytes());
        req.extend_from_slice(&port.to_be_bytes());
        c.write_all(&req).await.unwrap();
        let mut reply = [0u8; 10];
        c.read_exact(&mut reply).await.unwrap();
        (c, reply[1])
    }

    /// `connect_via` with the SOCKS command given (1 CONNECT, 2 BIND, 3 UDP ASSOCIATE).
    async fn request_via(relay: u16, host: &str, port: u16, command: u8) -> (TcpStream, u8) {
        let mut c = client(relay);
        c.write_all(&[5, 1, 0]).await.unwrap();
        let mut r = [0u8; 2];
        c.read_exact(&mut r).await.unwrap();
        let mut req = vec![5, command, 0, 3, host.len() as u8];
        req.extend_from_slice(host.as_bytes());
        req.extend_from_slice(&port.to_be_bytes());
        c.write_all(&req).await.unwrap();
        let mut reply = [0u8; 10];
        c.read_exact(&mut reply).await.unwrap();
        (c, reply[1])
    }

    /// CP-126 E5 T4: a SOCKS command that is no CONNECT is refused, also for a listed host
    /// (pr-review-19 T4; the verifier's M46).
    #[tokio::test]
    async fn a_socks_command_that_is_no_connect_is_refused() {
        let (echo, accepted) = echo_server().await;
        let relay = Relay::start().await.unwrap();
        relay.handle().trust(Some(me()));
        relay
            .handle()
            .set_list(Some(vec![("echo.agent.test".into(), echo)]));
        for command in [2u8, 3] {
            let (_, code) = request_via(relay.port, "echo.agent.test", echo, command).await;
            assert_ne!(code, 0, "command {command} was answered as a connect");
        }
        let (_, code) = request_via(relay.port, "echo.agent.test", echo, 1).await;
        assert_eq!(code, 0, "the control: a connect to the listed host");
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert_eq!(accepted.load(Ordering::SeqCst), 1);
    }

    /// CP-126 E5 M53: a connection that its peer closes leaves the relay's count of open
    /// connections, through the relay's accept and its end and not through `admit` alone.
    #[tokio::test]
    async fn a_closed_connection_leaves_the_count_of_open_ones() {
        let relay = Relay::start().await.unwrap();
        relay.handle().trust(Some(me()));
        let h = relay.handle();
        let wait_for = |want: usize| {
            let h = h.clone();
            async move {
                let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
                while h.connections() != want && std::time::Instant::now() < until {
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
                h.connections()
            }
        };
        let mut c = client(relay.port);
        c.write_all(&[5, 1, 0]).await.unwrap();
        let mut r = [0u8; 2];
        c.read_exact(&mut r).await.unwrap();
        assert_eq!(wait_for(1).await, 1, "the open connection is not counted");
        drop(c);
        assert_eq!(
            wait_for(0).await,
            0,
            "the closed connection is still counted"
        );
    }

    /// A pause after the relay's upstream connect to `port`, which a test sets: it stands for a
    /// connect that is still pending when the list changes.
    static CONNECT_PAUSE: std::sync::Mutex<Option<(u16, u64)>> = std::sync::Mutex::new(None);

    pub(super) async fn connect_pause(port: u16) {
        let ms = match CONNECT_PAUSE.lock() {
            Ok(p) => p.filter(|(p, _)| *p == port).map(|(_, ms)| ms),
            Err(_) => None,
        };
        if let Some(ms) = ms {
            tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
        }
    }

    /// CP-126 E5 T5: a host that leaves the list while the relay's connect to it is pending gets
    /// no tunnel: the relay reads the list again after the connect (pr-review-19 T5; the
    /// verifier's M47). A pause after the connect to the echo listener's port stands for the
    /// pending connect (this Mac's loopback answers a connect at once, also with a full queue).
    #[tokio::test]
    async fn a_host_that_leaves_the_list_during_its_connect_gets_no_tunnel() {
        let (echo, _) = echo_server().await;
        if let Ok(mut p) = CONNECT_PAUSE.lock() {
            *p = Some((echo, 800));
        }
        let relay = Relay::start().await.unwrap();
        relay.handle().trust(Some(me()));
        let h = relay.handle();
        h.set_list(Some(vec![("slow.agent.test".into(), echo)]));
        let pending = tokio::spawn({
            let port = relay.port;
            async move { request_via(port, "slow.agent.test", echo, 1).await.1 }
        });
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        h.set_list(Some(vec![("other.agent.test".into(), echo)]));
        let code = tokio::time::timeout(std::time::Duration::from_secs(10), pending)
            .await
            .expect("the request did not end")
            .unwrap();
        if let Ok(mut p) = CONNECT_PAUSE.lock() {
            *p = None;
        }
        assert_ne!(
            code, 0,
            "the host left the list during its connect and got a tunnel"
        );
        assert!(h.open_tunnels().is_empty());
    }

    async fn echo_server() -> (u16, Arc<AtomicU64>) {
        let echo = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let port = echo.local_addr().unwrap().port();
        let accepted = Arc::new(AtomicU64::new(0));
        let seen = accepted.clone();
        tokio::spawn(async move {
            while let Ok((mut s, _)) = echo.accept().await {
                seen.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(async move {
                    let mut b = [0u8; 5];
                    while s.read_exact(&mut b).await.is_ok() {
                        if s.write_all(&b).await.is_err() {
                            break;
                        }
                    }
                });
            }
        });
        (port, accepted)
    }

    #[tokio::test]
    async fn a_tunnel_passes_bytes_both_ways_and_a_name_outside_test_is_refused() {
        let (echo, _) = echo_server().await;
        let relay = Relay::start().await.unwrap();
        relay.handle().trust(Some(me()));
        let (mut c, code) = connect_via(relay.port, "echo.agent.test", echo).await;
        assert_eq!(code, 0);
        c.write_all(b"hello").await.unwrap();
        let mut back = [0u8; 5];
        c.read_exact(&mut back).await.unwrap();
        assert_eq!(&back, b"hello");
        assert_eq!(relay.tunnels(), 1);
        let (_, code) = connect_via(relay.port, "example.com", 80).await;
        assert_eq!(code, 2);
        assert_eq!(
            relay.handle().refusals(),
            vec![("example.com".to_string(), 80)]
        );
        relay.stop();
    }

    #[tokio::test]
    async fn a_host_outside_the_list_is_refused_before_any_connection_and_recorded() {
        let (echo, accepted) = echo_server().await;
        let relay = Relay::start().await.unwrap();
        relay.handle().trust(Some(me()));
        let h = relay.handle();
        h.set_list(Some(vec![("allowed.agent.test".into(), echo)]));
        let (_, code) = connect_via(relay.port, "evil.agent.test", echo).await;
        assert_eq!(code, 2);
        assert_eq!(accepted.load(Ordering::SeqCst), 0);
        assert_eq!(h.refusals_of("EVIL.agent.test", echo), 1);
        let (_, code) = connect_via(relay.port, "allowed.agent.test", echo + 1).await;
        assert_eq!(code, 2);
        assert_eq!(accepted.load(Ordering::SeqCst), 0);
        let (_, code) = connect_via(relay.port, "Allowed.Agent.Test", echo).await;
        assert_eq!(code, 0);
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while accepted.load(Ordering::SeqCst) == 0 && std::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(accepted.load(Ordering::SeqCst), 1);
        relay.stop();
    }

    #[tokio::test]
    async fn a_host_that_leaves_the_list_has_its_open_tunnels_closed() {
        let (echo, _) = echo_server().await;
        let relay = Relay::start().await.unwrap();
        relay.handle().trust(Some(me()));
        let h = relay.handle();
        h.set_list(Some(vec![
            ("a.agent.test".into(), echo),
            ("b.agent.test".into(), echo),
        ]));
        let (mut a, code_a) = connect_via(relay.port, "a.agent.test", echo).await;
        let (mut b, code_b) = connect_via(relay.port, "b.agent.test", echo).await;
        assert_eq!((code_a, code_b), (0, 0));
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(h.open_tunnels().len(), 2);
        h.remove("a.agent.test", echo);
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(h.closed(), vec![("a.agent.test".to_string(), echo)]);
        a.write_all(b"hello").await.ok();
        let mut back = [0u8; 5];
        let gone = tokio::time::timeout(Duration::from_secs(2), a.read_exact(&mut back)).await;
        assert!(
            matches!(gone, Ok(Err(_))),
            "the closed tunnel still answered"
        );
        b.write_all(b"hello").await.unwrap();
        b.read_exact(&mut back).await.unwrap();
        assert_eq!(&back, b"hello");
        let (_, code) = connect_via(relay.port, "a.agent.test", echo).await;
        assert_eq!(code, 2);
        h.set_list(Some(Vec::new()));
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(h.open_tunnels().is_empty());
        relay.stop();
    }
}
