use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

const HANDSHAKE_WAIT: Duration = Duration::from_secs(10);

/// The relay of ruling CP-24: a SOCKS5 listener on a loopback port that the driver's own process
/// opens, given to the browser as its only route. It takes one CONNECT per tunnel, resolves the
/// name itself (a name under `.test`, `localhost` and a loopback address go to 127.0.0.1; any
/// other is refused), and passes bytes both ways without reading them. When the process ends, the
/// system closes its sockets with it.
pub struct Relay {
    pub port: u16,
    task: JoinHandle<()>,
    counts: Arc<RelayCounts>,
}

#[derive(Default)]
pub struct RelayCounts {
    pub tunnels: AtomicU64,
    pub refused: AtomicU64,
    pub outbound_ports: Mutex<Vec<u16>>,
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

impl Relay {
    pub async fn start() -> std::io::Result<Relay> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let port = listener.local_addr()?.port();
        let counts = Arc::new(RelayCounts::default());
        let shared = counts.clone();
        let task = tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                let shared = shared.clone();
                tokio::spawn(async move {
                    let _ = tunnel(socket, shared).await;
                });
            }
        });
        Ok(Relay { port, task, counts })
    }

    pub fn tunnels(&self) -> u64 {
        self.counts.tunnels.load(Ordering::SeqCst)
    }

    pub fn refused(&self) -> u64 {
        self.counts.refused.load(Ordering::SeqCst)
    }

    /// The local ports of the relay's own connections to their destinations.
    pub fn outbound_ports(&self) -> Vec<u16> {
        self.counts
            .outbound_ports
            .lock()
            .map(|p| p.clone())
            .unwrap_or_default()
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

async fn read_exact_in_time(socket: &mut TcpStream, buf: &mut [u8]) -> std::io::Result<()> {
    match tokio::time::timeout(HANDSHAKE_WAIT, socket.read_exact(buf)).await {
        Ok(r) => r.map(|_| ()),
        Err(_) => Err(std::io::ErrorKind::TimedOut.into()),
    }
}

async fn tunnel(mut socket: TcpStream, counts: Arc<RelayCounts>) -> std::io::Result<()> {
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
    let target = if request[1] == 1 {
        destination(&host)
    } else {
        None
    };
    let Some(ip) = target else {
        counts.refused.fetch_add(1, Ordering::SeqCst);
        socket.write_all(&[5, 2, 0, 1, 0, 0, 0, 0, 0, 0]).await?;
        return Ok(());
    };
    let mut upstream = match TcpStream::connect(SocketAddr::new(ip, port)).await {
        Ok(s) => s,
        Err(_) => {
            socket.write_all(&[5, 5, 0, 1, 0, 0, 0, 0, 0, 0]).await?;
            return Ok(());
        }
    };
    counts.tunnels.fetch_add(1, Ordering::SeqCst);
    if let (Ok(local), Ok(mut ports)) = (upstream.local_addr(), counts.outbound_ports.lock()) {
        ports.push(local.port());
    }
    socket.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).await?;
    let _ = tokio::io::copy_bidirectional(&mut socket, &mut upstream).await;
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

    #[tokio::test]
    async fn a_tunnel_passes_bytes_both_ways_and_a_name_outside_test_is_refused() {
        let echo = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let echo_port = echo.local_addr().unwrap().port();
        tokio::spawn(async move {
            if let Ok((mut s, _)) = echo.accept().await {
                let mut b = [0u8; 5];
                if s.read_exact(&mut b).await.is_ok() {
                    let _ = s.write_all(&b).await;
                }
            }
        });
        let relay = Relay::start().await.unwrap();
        let mut c = TcpStream::connect((Ipv4Addr::LOCALHOST, relay.port))
            .await
            .unwrap();
        c.write_all(&[5, 1, 0]).await.unwrap();
        let mut r = [0u8; 2];
        c.read_exact(&mut r).await.unwrap();
        assert_eq!(r, [5, 0]);
        let name = b"echo.agent.test";
        let mut req = vec![5, 1, 0, 3, name.len() as u8];
        req.extend_from_slice(name);
        req.extend_from_slice(&echo_port.to_be_bytes());
        c.write_all(&req).await.unwrap();
        let mut reply = [0u8; 10];
        c.read_exact(&mut reply).await.unwrap();
        assert_eq!(reply[1], 0);
        c.write_all(b"hello").await.unwrap();
        let mut back = [0u8; 5];
        c.read_exact(&mut back).await.unwrap();
        assert_eq!(&back, b"hello");
        assert_eq!(relay.tunnels(), 1);

        let mut d = TcpStream::connect((Ipv4Addr::LOCALHOST, relay.port))
            .await
            .unwrap();
        d.write_all(&[5, 1, 0]).await.unwrap();
        d.read_exact(&mut r).await.unwrap();
        let other = b"example.com";
        let mut req = vec![5, 1, 0, 3, other.len() as u8];
        req.extend_from_slice(other);
        req.extend_from_slice(&80u16.to_be_bytes());
        d.write_all(&req).await.unwrap();
        d.read_exact(&mut reply).await.unwrap();
        assert_eq!(reply[1], 2);
        assert_eq!(relay.refused(), 1);
        relay.stop();
    }
}
