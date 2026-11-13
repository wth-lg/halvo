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

pub use crate::peer::ForeignPeer;

/// A host and a port, the unit of the relay's list and of its record.
pub type HostPort = (String, u16);

/// The relay of A11.1 (rulings CP-24, CP-64, CP-89 (e)): a SOCKS5 listener on a loopback port of
/// the executor's own process, given to the browser as its only route. It starts with an empty
/// list and refuses every tunnel until the runner allows a host and port. It takes one CONNECT per
/// tunnel; a tunnel to a host and port outside the list is refused at the SOCKS5 request, before
/// any byte leaves and before any lookup, and the refusal is recorded by host and port. A listed
/// host is then looked up by `Names`: `Names::Fixture` takes a name under `.test` alone, to
/// 127.0.0.1, and `Names::System` asks the system. A peer that is not at 127.0.0.1, or not one
/// process of the browser's tree (`peer::check_peer`), is refused unread and recorded apart. The
/// bytes of a tunnel pass both ways unread. A host that leaves the list has its open tunnels
/// closed. When the process ends, the system closes the relay's sockets with it.
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
    list: Mutex<List>,
    names: Names,
    refusals: Mutex<Vec<HostPort>>,
    open: Mutex<HashMap<u64, Open>>,
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

/// An open tunnel: its host and port, whether a change of the list closed it, and the task that
/// copies its bytes once that task runs.
struct Open {
    hp: HostPort,
    closed: Arc<std::sync::atomic::AtomicBool>,
    copy: Option<AbortHandle>,
}

/// The relay's listener as the system names it, for a start that places it in the guard.
#[cfg(unix)]
type ListenerRaw = std::os::fd::RawFd;
#[cfg(windows)]
type ListenerRaw = usize;

/// How long the relay waits after a failed accept (a process out of descriptors, a connection
/// that its peer abandoned) before it accepts again; the listener stays open.
const ACCEPT_PAUSE: Duration = Duration::from_millis(50);

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

/// The start flags that make the relay the browser's only route: the relay as its SOCKS5 proxy,
/// also for loopback, QUIC off, and a page's peer connections only through the proxy (A11.1).
pub fn relay_flags(port: u16) -> Vec<String> {
    vec![
        format!("--proxy-server=socks5://127.0.0.1:{port}"),
        "--proxy-bypass-list=<-loopback>".to_string(),
        "--disable-quic".to_string(),
        "--force-webrtc-ip-handling-policy=disable_non_proxied_udp".to_string(),
    ]
}

/// Where the relay sends a tunnel to a host that its list holds, which it looks up only after the
/// list let the host through.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Names {
    /// The system's lookup, the product's.
    #[default]
    System,
    /// The fixture's names of the probe and the tests: a name under `.test` goes to 127.0.0.1 and
    /// every other name nowhere.
    Fixture,
}

/// The relay's list (A11.1): the hosts and ports that a tunnel may reach, or every host and port,
/// as in the person's handback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum List {
    Hosts(BTreeSet<HostPort>),
    Every,
}

impl List {
    fn holds(&self, hp: &HostPort) -> bool {
        match self {
            List::Hosts(set) => set.contains(hp),
            List::Every => true,
        }
    }
}

impl Default for List {
    fn default() -> List {
        List::Hosts(BTreeSet::new())
    }
}

async fn look_up(names: Names, host: &str, port: u16) -> Option<SocketAddr> {
    match names {
        Names::Fixture => host
            .to_ascii_lowercase()
            .ends_with(".test")
            .then_some(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)),
        Names::System => tokio::net::lookup_host((host, port)).await.ok()?.next(),
    }
}

fn key(host: &str, port: u16) -> HostPort {
    (host.to_ascii_lowercase(), port)
}

impl Relay {
    /// Starts the relay on a loopback port that the system picks, with an empty list, so that it
    /// refuses every tunnel until the runner allows a host, and with `names` for the hosts that the
    /// list lets through. Until `trust` names a browser, every connection is refused as foreign.
    pub async fn start(names: Names) -> std::io::Result<Relay> {
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
            names,
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
    /// Starts the guard of the relay's port for the browser `root` (`guard::guard_main`): the
    /// guard's program with `GUARD_FLAG`, the root's and this process's numbers and creation
    /// times, the record's path, the run's profile and the browser's program (the run's mark), the relay's listener placed in it, under the start lock, with
    /// the guard's environment and the system directory (CP-160 (1)). The error names the step
    /// that failed.
    pub fn start_guard(
        &self,
        guard: &crate::guard::GuardProgram,
        root: &Proc,
        record: &std::path::Path,
        profile: &std::path::Path,
        program: &std::path::Path,
    ) -> Result<crate::start::Started, &'static str> {
        let listener = self.shared.listener.lock().map_err(|_| "relay")?;
        let raw = (*listener).ok_or("relay_closed")?;
        let me = procs::own().ok_or("own_row")?;
        let dir = crate::start::system_dir().ok_or("system_dir")?;
        let env = crate::start::guard_env().ok_or("guard_env")?;
        let mut args = guard.prefix.clone();
        args.extend([
            crate::guard::GUARD_FLAG.into(),
            root.pid.to_string().into(),
            root.started_us.to_string().into(),
            me.pid.to_string().into(),
            me.started_us.to_string().into(),
            record.as_os_str().to_owned(),
            profile.as_os_str().to_owned(),
            program.as_os_str().to_owned(),
        ]);
        let spec = crate::start::Spec {
            program: &guard.program,
            args,
            env,
            dir: &dir,
        };
        let held = crate::start::lock();
        #[cfg(unix)]
        let placed = raw;
        #[cfg(windows)]
        let placed = raw as crate::start::RawPlaced;
        crate::start::start_with(&held, &spec, placed).map_err(|_| "guard_start")
    }

    #[cfg(test)]
    pub(crate) fn for_tests(port: u16) -> RelayHandle {
        RelayHandle {
            shared: Arc::new(Shared {
                port,
                ..Shared::default()
            }),
        }
    }

    /// The relay's loopback port.
    pub fn port(&self) -> u16 {
        self.shared.port
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

    /// Sets the list to exactly `list`. Every open tunnel to a host and port that the new list
    /// does not hold is closed.
    pub fn set_list(&self, list: List) {
        if let Ok(mut l) = self.shared.list.lock() {
            *l = list.clone();
        }
        self.close_outside(&list);
    }

    /// Adds one host and port to the list; a list of every host stays one.
    pub fn allow(&self, host: &str, port: u16) {
        if let Ok(mut l) = self.shared.list.lock() {
            if let List::Hosts(set) = &mut *l {
                set.insert(key(host, port));
            }
        }
    }

    /// Takes one host and port out of the list and closes its open tunnels; a list of every host
    /// becomes the list of the hosts of the open tunnels less this one.
    pub fn remove(&self, host: &str, port: u16) {
        let list = match self.shared.list.lock() {
            Ok(mut l) => {
                if *l == List::Every {
                    *l = List::Hosts(self.open_tunnels().into_iter().collect());
                }
                if let List::Hosts(set) = &mut *l {
                    set.remove(&key(host, port));
                }
                l.clone()
            }
            Err(_) => return,
        };
        self.close_outside(&list);
    }

    /// The list as it stands.
    pub fn list(&self) -> List {
        self.shared
            .list
            .lock()
            .map(|l| l.clone())
            .unwrap_or_default()
    }

    fn close_outside(&self, list: &List) {
        let leaving: Vec<Open> = match self.shared.open.lock() {
            Ok(mut open) => {
                let ids: Vec<u64> = open
                    .iter()
                    .filter(|(_, o)| !list.holds(&o.hp))
                    .map(|(id, _)| *id)
                    .collect();
                ids.into_iter().filter_map(|id| open.remove(&id)).collect()
            }
            Err(_) => return,
        };
        for Open { hp, closed, copy } in leaving {
            closed.store(true, Ordering::SeqCst);
            if let Some(abort) = copy {
                abort.abort();
            }
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
            .map(|o| o.values().map(|t| t.hp.clone()).collect())
            .unwrap_or_default()
    }

    pub fn tunnels(&self) -> u64 {
        self.shared.tunnels.load(Ordering::SeqCst)
    }

    fn listed(&self, hp: &HostPort) -> bool {
        match self.shared.list.lock() {
            Ok(l) => l.holds(hp),
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
    let peer = socket.peer_addr()?;
    let peer_port = peer.port();
    let root = shared.browser.lock().ok().and_then(|b| b.clone());
    let relay_port = shared.port;
    let verdict = tokio::task::spawn_blocking(move || {
        if peer.ip() != IpAddr::V4(Ipv4Addr::LOCALHOST) {
            return Err(ForeignPeer {
                peer_port,
                owners: 0,
                in_tree: 0,
                cause: "peer_address",
            });
        }
        crate::peer::check_peer(root.as_ref(), peer_port, relay_port)
    })
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
    let Some(address) = look_up(shared.names, &host, port).await else {
        socket.write_all(&[5, 4, 0, 1, 0, 0, 0, 0, 0, 0]).await?;
        return Ok(());
    };
    let mut upstream = match TcpStream::connect(address).await {
        Ok(s) => s,
        Err(_) => {
            socket.write_all(&[5, 5, 0, 1, 0, 0, 0, 0, 0, 0]).await?;
            return Ok(());
        }
    };
    #[cfg(test)]
    tests::connect_pause(port).await;
    let id = shared.next.fetch_add(1, Ordering::SeqCst);
    let closed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let registered = match shared.list.lock() {
        Ok(list) if list.holds(&hp) => match shared.open.lock() {
            Ok(mut open) => {
                open.insert(
                    id,
                    Open {
                        hp: hp.clone(),
                        closed: closed.clone(),
                        copy: None,
                    },
                );
                true
            }
            Err(_) => false,
        },
        _ => false,
    };
    if !registered {
        refuse(&mut socket, &shared, hp, 2).await;
        return Ok(());
    }
    shared.tunnels.fetch_add(1, Ordering::SeqCst);
    if socket
        .write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0])
        .await
        .is_err()
        || closed.load(Ordering::SeqCst)
    {
        if let Ok(mut open) = shared.open.lock() {
            open.remove(&id);
        }
        return Ok(());
    }
    let copy = tokio::spawn(async move {
        let _ = tokio::io::copy_bidirectional(&mut socket, &mut upstream).await;
    });
    match shared.open.lock() {
        Ok(mut open) => match open.get_mut(&id) {
            Some(o) if !closed.load(Ordering::SeqCst) => o.copy = Some(copy.abort_handle()),
            _ => copy.abort(),
        },
        Err(_) => copy.abort(),
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

    fn hosts(entries: Vec<(&str, u16)>) -> List {
        List::Hosts(entries.into_iter().map(|(h, p)| key(h, p)).collect())
    }

    #[test]
    fn guard_stand_in_entry() {
        let args: Vec<String> = std::env::args().collect();
        let Some(at) = args.iter().position(|a| a == crate::guard::GUARD_FLAG) else {
            return;
        };
        let out = args.iter().skip_while(|a| *a != "--").nth(1).cloned();
        let mut text = format!("args {}\n", args[at + 1..].join(" "));
        let mut names = crate::child::initial_env();
        names.sort();
        text.push_str(&format!("env {}\n", names.join(" ")));
        #[cfg(target_os = "macos")]
        {
            let port = crate::peer::sockets(&[std::process::id()])
                .iter()
                .find(|s| s.listening)
                .map_or(0, |s| s.local.1);
            text.push_str(&format!("listener {port}\n"));
        }
        text.push_str("end\n");
        if let Some(out) = out {
            let _ = std::fs::write(out, text);
        }
        std::process::exit(0);
    }

    /// The guard's start (CP-160 (1), finding (3)'s other half): the guard's program gets
    /// `GUARD_FLAG` with the browser's and this process's numbers and creation times and the
    /// record, the guard's environment alone, and on macOS the relay's listener. A stand-in entry
    /// takes the guard's place, so no guard runs here.
    #[tokio::test]
    async fn the_guard_starts_with_its_arguments_its_list_and_the_listener() {
        let relay = Relay::start(Names::Fixture).await.unwrap();
        let dir = crate::child::scratch("guard-start");
        let out = dir.join("guard.txt");
        let guard = crate::guard::GuardProgram {
            program: std::env::current_exe().unwrap(),
            prefix: vec![
                "relay::tests::guard_stand_in_entry".into(),
                "--exact".into(),
                "--quiet".into(),
                "--test-threads=1".into(),
                "--".into(),
                out.clone().into_os_string(),
            ],
        };
        let root = Proc {
            pid: 4242,
            ppid: 1,
            started_us: 77,
            pgid: 0,
            exe: None,
        };
        let mut started = relay
            .handle()
            .start_guard(
                &guard,
                &root,
                &dir.join("record"),
                std::path::Path::new("/p/profile"),
                std::path::Path::new("/b/chrome"),
            )
            .expect("started");
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        let text = loop {
            if let Ok(t) = std::fs::read_to_string(&out) {
                if t.ends_with("end\n") {
                    break t;
                }
            }
            if std::time::Instant::now() > deadline {
                break String::new();
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        started.kill();
        let own = me();
        let args = format!(
            "args 4242 77 {} {} {} /p/profile /b/chrome",
            own.pid,
            own.started_us,
            dir.join("record").display()
        );
        assert!(text.lines().any(|l| l == args), "{text}");
        let mut names: Vec<String> = crate::start::guard_env()
            .unwrap()
            .into_iter()
            .map(|(k, _)| k.to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert!(
            text.lines()
                .any(|l| l == format!("env {}", names.join(" "))),
            "{text}"
        );
        #[cfg(target_os = "macos")]
        assert!(
            text.lines()
                .any(|l| l == format!("listener {}", relay.port)),
            "{text}"
        );
        relay.stop();
        let closed = relay_closed_start(&guard, &root, &dir);
        assert_eq!(closed, Err("relay_closed"));
    }

    fn relay_closed_start(
        guard: &crate::guard::GuardProgram,
        root: &Proc,
        dir: &std::path::Path,
    ) -> Result<(), &'static str> {
        let shared = Arc::new(Shared::default());
        RelayHandle { shared }
            .start_guard(
                guard,
                root,
                &dir.join("record"),
                std::path::Path::new("/p/profile"),
                std::path::Path::new("/b/chrome"),
            )
            .map(|mut s| s.kill())
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
        let relay = Relay::start(Names::Fixture).await.unwrap();
        let (echo, accepted) = echo_server().await;
        for trusted in [
            None,
            Some(Proc {
                pid: 1,
                ppid: 0,
                started_us: 1,
                pgid: 0,
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
        relay.handle().allow("utility.a.test", echo);
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
            dir: &crate::start::system_dir().unwrap(),
        };
        let mut second =
            crate::start::start_with(&held, &spec, std::os::fd::AsRawFd::as_raw_fd(&c)).unwrap();
        drop(held);
        std::thread::sleep(Duration::from_millis(300));
        let peer = c.local_addr().unwrap().port();
        assert_eq!(
            crate::peer::check_peer(Some(&me()), peer, relay_port).map_err(|f| (f.cause, f.owners)),
            Err(("several_owners", 2))
        );
        second.kill();
        assert_eq!(
            crate::peer::check_peer(Some(&me()), peer, relay_port),
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
            dir: &crate::start::system_dir().unwrap(),
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
            crate::peer::check_peer(Some(&root), peer, relay_port)
                .map_err(|f| (f.cause, f.in_tree)),
            Err(("several_owners", 1))
        );
        outside.kill();
        assert_eq!(
            crate::peer::check_peer(Some(&root), peer, relay_port),
            Ok(inside.pid())
        );
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
            crate::peer::check_peer(Some(&me()), peer, relay_port),
            Ok(std::process::id())
        );
        let mut stale = me();
        stale.started_us += 1;
        assert_eq!(
            crate::peer::check_peer(Some(&stale), peer, relay_port).map_err(|f| f.cause),
            Err("outside_tree")
        );
        assert_eq!(
            crate::peer::check_peer(None, peer, relay_port).map_err(|f| f.cause),
            Err("no_browser")
        );
        assert_eq!(
            crate::peer::check_peer(Some(&me()), peer, relay_port.wrapping_add(1))
                .map_err(|f| f.cause),
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
        let relay = Relay::start(Names::Fixture).await.unwrap();
        relay.handle().trust(Some(me()));
        relay
            .handle()
            .set_list(hosts(vec![("echo.agent.test", echo)]));
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
        let relay = Relay::start(Names::Fixture).await.unwrap();
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
        let relay = Relay::start(Names::Fixture).await.unwrap();
        relay.handle().trust(Some(me()));
        let h = relay.handle();
        h.set_list(hosts(vec![("slow.agent.test", echo)]));
        let pending = tokio::spawn({
            let port = relay.port;
            async move { request_via(port, "slow.agent.test", echo, 1).await.1 }
        });
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        h.set_list(hosts(vec![("other.agent.test", echo)]));
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

    /// Point 58 (d) in the product's form (A11.1, the list with nothing bound): a relay starts
    /// with an empty list and refuses every host, recorded, until the runner allows one; a tunnel
    /// then passes bytes both ways. A list of every host lets every host through the list, and
    /// `allow` leaves it one; the fixture's names then refuse a name outside `.test` at the lookup.
    #[tokio::test]
    async fn a_relay_starts_with_an_empty_list_and_a_list_of_every_host_stays_one() {
        let (echo, accepted) = echo_server().await;
        let relay = Relay::start(Names::Fixture).await.unwrap();
        let h = relay.handle();
        h.trust(Some(me()));
        assert_eq!(h.list(), List::default());
        let (_, code) = connect_via(relay.port, "echo.agent.test", echo).await;
        assert_eq!(code, 2);
        assert_eq!(h.refusals(), vec![("echo.agent.test".to_string(), echo)]);
        assert_eq!(accepted.load(Ordering::SeqCst), 0);
        h.allow("ECHO.agent.test", echo);
        let (mut c, code) = connect_via(relay.port, "echo.agent.test", echo).await;
        assert_eq!(code, 0);
        c.write_all(b"hello").await.unwrap();
        let mut back = [0u8; 5];
        c.read_exact(&mut back).await.unwrap();
        assert_eq!(&back, b"hello");
        assert_eq!(relay.tunnels(), 1);
        h.set_list(List::Every);
        h.allow("x.agent.test", 1);
        assert_eq!(h.list(), List::Every);
        let (_, code) = connect_via(relay.port, "example.com", 80).await;
        assert_eq!(code, 4);
        assert_eq!(h.refusals().len(), 1);
        h.remove("echo.agent.test", echo);
        assert_eq!(h.list(), List::default());
        assert_eq!(h.closed(), vec![("echo.agent.test".to_string(), echo)]);
        relay.stop();
    }

    #[tokio::test]
    async fn a_host_outside_the_list_is_refused_before_any_connection_and_recorded() {
        let (echo, accepted) = echo_server().await;
        let relay = Relay::start(Names::Fixture).await.unwrap();
        relay.handle().trust(Some(me()));
        let h = relay.handle();
        h.set_list(hosts(vec![("allowed.agent.test", echo)]));
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
        let relay = Relay::start(Names::Fixture).await.unwrap();
        relay.handle().trust(Some(me()));
        let h = relay.handle();
        h.set_list(hosts(vec![("a.agent.test", echo), ("b.agent.test", echo)]));
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
        h.set_list(List::default());
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(h.open_tunnels().is_empty());
        relay.stop();
    }
}

/// The review of 85b6d770 (evidence/builder-44/review, finder b, case 2): a remove that lands
/// between the list's second read and the tunnel's registration closes the tunnel.
#[cfg(test)]
mod review_tunnel_race {
    use super::*;

    fn own() -> Proc {
        procs::table()
            .unwrap()
            .into_iter()
            .find(|p| p.pid == std::process::id())
            .unwrap()
    }

    fn client(port: u16) -> TcpStream {
        let c = {
            let _held = crate::start::lock();
            std::net::TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap()
        };
        c.set_nonblocking(true).unwrap();
        TcpStream::from_std(c).unwrap()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_tunnel_registered_while_its_host_leaves_the_list_is_closed() {
        let echo = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let echo_port = echo.local_addr().unwrap().port();
        tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((s, _)) = echo.accept().await {
                held.push(s);
            }
        });
        let relay = Relay::start(Names::Fixture).await.unwrap();
        let port = relay.port;
        let h = relay.handle();
        h.trust(Some(own()));
        let host = "race.agent.test";
        let mut escaped = 0;
        let mut kept = Vec::new();
        for _ in 0..30 {
            h.allow(host, echo_port);
            let before = h.tunnels();
            let watcher = {
                let h = h.clone();
                std::thread::spawn(move || {
                    let until = std::time::Instant::now() + Duration::from_secs(10);
                    while h.tunnels() == before && std::time::Instant::now() < until {
                        std::hint::spin_loop();
                    }
                    h.remove(host, echo_port);
                })
            };
            let mut c = client(port);
            c.write_all(&[5, 1, 0]).await.unwrap();
            let mut r = [0u8; 2];
            c.read_exact(&mut r).await.unwrap();
            let mut req = vec![5, 1, 0, 3, host.len() as u8];
            req.extend_from_slice(host.as_bytes());
            req.extend_from_slice(&echo_port.to_be_bytes());
            c.write_all(&req).await.unwrap();
            let mut reply = [0u8; 10];
            let _ = tokio::time::timeout(Duration::from_secs(10), c.read_exact(&mut reply)).await;
            watcher.join().unwrap();
            tokio::time::sleep(Duration::from_millis(50)).await;
            let k = key(host, echo_port);
            if !h.list().holds(&k) && h.open_tunnels().contains(&k) {
                escaped += 1;
            }
            kept.push(c);
        }
        relay.stop();
        assert_eq!(
            escaped, 0,
            "a tunnel to a host that left the list stayed open in {escaped} of 30 tries"
        );
    }
}

/// The review of 85b6d770 (finder c, case 1): a process outside the tree that connects from
/// another local address with the source port of a tree process's connection gets no tunnel.
#[cfg(all(test, target_os = "macos"))]
mod review_peer_address {
    use super::*;

    /// A second local IPv4 address from which a TCP connection to 127.0.0.1 can be made: 127.0.0.2
    /// where the system has it, else the host's own address. The UDP connect sends no packet.
    #[cfg(target_os = "macos")]
    fn second_address() -> Option<Ipv4Addr> {
        let mut tries = vec![Ipv4Addr::new(127, 0, 0, 2)];
        if let Ok(u) = std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)) {
            if u.connect((Ipv4Addr::new(192, 0, 2, 1), 9)).is_ok() {
                if let Ok(SocketAddr::V4(a)) = u.local_addr() {
                    tries.push(*a.ip());
                }
            }
        }
        tries
            .into_iter()
            .find(|ip| std::net::TcpListener::bind((*ip, 0)).is_ok())
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_peer_from_another_local_address_with_a_tree_port_is_refused() {
        let Some(other) = second_address() else {
            eprintln!("setup: no second local address; nothing shown");
            return;
        };
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let relay = Relay::start(Names::Fixture).await.unwrap();
            let rp = relay.port;
            // A: a connection from 127.0.0.1:P to the relay that a /bin/sleep child alone holds.
            let held = crate::start::lock();
            let a = std::net::TcpStream::connect((Ipv4Addr::LOCALHOST, rp)).unwrap();
            let p = a.local_addr().unwrap().port();
            let spec = crate::start::Spec {
                program: std::path::Path::new("/bin/sleep"),
                args: vec!["20".into()],
                env: Vec::new(),
                dir: &crate::start::system_dir().unwrap(),
            };
            let mut sleeper =
                crate::start::start_with(&held, &spec, std::os::fd::AsRawFd::as_raw_fd(&a))
                    .unwrap();
            drop(a);
            drop(held);
            std::thread::sleep(Duration::from_millis(300));
            let root = procs::table()
                .unwrap()
                .into_iter()
                .find(|q| q.pid == sleeper.pid())
                .unwrap();
            relay.handle().trust(Some(root));
            relay.handle().allow("utility.a.test", 9);
            let before = relay.handle().foreign_count();
            // B: this process, outside the trusted tree, connects from <other>:P.
            let made = {
                let _held = crate::start::lock();
                tokio::net::TcpSocket::new_v4()
                    .and_then(|s| s.bind(SocketAddr::from((other, p))).map(|_| s))
            };
            let connected = match made {
                Ok(s) => s
                    .connect(SocketAddr::from((Ipv4Addr::LOCALHOST, rp)))
                    .await
                    .ok(),
                Err(_) => None,
            };
            let Some(mut b) = connected else {
                sleeper.kill();
                eprintln!("setup: could not connect from {other}:{p}; nothing shown");
                return;
            };
            let _ = b.write_all(&[5, 1, 0]).await;
            let mut got = [0u8; 2];
            let read = tokio::time::timeout(Duration::from_secs(5), b.read(&mut got)).await;
            sleeper.kill();
            let tunnel_offered = matches!(read, Ok(Ok(2))) && got == [5, 0];
            assert!(
                !tunnel_offered,
                "a process outside the tree got the SOCKS5 greeting reply from {other}:{p}"
            );
            assert_eq!(relay.handle().foreign_count(), before + 1);
        });
    }
}
