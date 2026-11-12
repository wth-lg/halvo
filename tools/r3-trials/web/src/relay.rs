use std::collections::{BTreeSet, HashMap};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::{AbortHandle, JoinHandle};

const HANDSHAKE_WAIT: Duration = Duration::from_secs(10);

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
    list: Mutex<Option<BTreeSet<HostPort>>>,
    refusals: Mutex<Vec<HostPort>>,
    open: Mutex<HashMap<u64, (HostPort, AbortHandle)>>,
    closed: Mutex<Vec<HostPort>>,
    next: AtomicU64,
    tunnels: AtomicU64,
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
    pub async fn start() -> std::io::Result<Relay> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let port = listener.local_addr()?.port();
        let shared = Arc::new(Shared::default());
        let serving = shared.clone();
        let task = tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                let shared = serving.clone();
                tokio::spawn(async move {
                    let _ = tunnel(socket, shared).await;
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
        self.task.abort();
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl RelayHandle {
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
            if let Ok(mut c) = self.shared.closed.lock() {
                c.push(hp);
            }
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
    if let Ok(mut r) = shared.refusals.lock() {
        r.push(hp);
    }
    let _ = socket.write_all(&[5, code, 0, 1, 0, 0, 0, 0, 0, 0]).await;
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

    async fn connect_via(relay: u16, host: &str, port: u16) -> (TcpStream, u8) {
        let mut c = TcpStream::connect((Ipv4Addr::LOCALHOST, relay))
            .await
            .unwrap();
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
