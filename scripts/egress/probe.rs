//! The probe and helper binary of the egress check (contracts.md C26.6).
//!
//! `probe.sh` builds this file into `target/egress/bin/egress-spawn` and copies it to
//! `egress-probe`. One binary serves four roles, chosen by the first argument:
//!
//! - `spawn NAME`: runs `egress-probe connect NAME` as a child and exits with its status,
//!   so that the probe is a grandchild of the shell of probe.sh under another image name.
//! - `connect NAME`: makes the connect, send or lookup of probe NAME and exits 0 when it
//!   succeeded, 1 when it failed or 5 s passed.
//! - `run STATUS PID -- COMMAND [ARG...]`: starts COMMAND with the caller's standard streams,
//!   environment and directory, writes its own process id to PID before the start, and writes
//!   to STATUS the exit code, `signal-<n>` or `not-started`; it exits 0 once STATUS is written.
//! - `serve PORT STOP READY`: listens on 127.0.0.1:PORT, writes READY once bound, accepts and
//!   closes connections until the file STOP exists or 120 s pass; it exits 3 when the bind fails.

use std::env;
use std::fs;
use std::io::ErrorKind;
use std::net::{
    Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6, TcpListener, TcpStream,
    ToSocketAddrs, UdpSocket,
};
use std::path::{Path, PathBuf};
use std::process::{exit, Command};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

const LIMIT: Duration = Duration::from_secs(5);
const NAMES: [&str; 8] = [
    "ipv4",
    "ipv6",
    "dns",
    "loopback",
    "udp",
    "loopback2",
    "none",
    "listen",
];

fn main() {
    let args: Vec<String> = env::args().collect();
    let role = args.get(1).map(String::as_str).unwrap_or("");
    let code = match role {
        "spawn" if args.len() == 3 && NAMES.contains(&args[2].as_str()) => spawn(&args[2]),
        "connect" if args.len() == 3 && NAMES.contains(&args[2].as_str()) => connect(&args[2]),
        "run" if args.len() >= 6 && args[4] == "--" => run(&args[2], &args[3], &args[5..]),
        "serve" if args.len() == 5 => serve(&args[2], &args[3], &args[4]),
        _ => {
            eprintln!("REFUSED usage: {}", args[1..].join(" "));
            2
        }
    };
    exit(code);
}

fn sibling(name: &str) -> PathBuf {
    let me = env::current_exe().unwrap_or_default();
    let dir = me.parent().map(Path::to_path_buf).unwrap_or_default();
    dir.join(format!("{name}{}", env::consts::EXE_SUFFIX))
}

fn spawn(name: &str) -> i32 {
    match Command::new(sibling("egress-probe"))
        .arg("connect")
        .arg(name)
        .status()
    {
        Ok(status) => match status.code() {
            Some(0) => 0,
            _ => 1,
        },
        Err(_) => 1,
    }
}

fn connect(name: &str) -> i32 {
    let (done, wait) = mpsc::channel();
    let owned = name.to_string();
    thread::spawn(move || {
        let ok = attempt(&owned);
        let _ = done.send(ok);
    });
    match wait.recv_timeout(LIMIT) {
        Ok(true) => 0,
        _ => 1,
    }
}

fn v4(a: u8, b: u8, c: u8, d: u8, port: u16) -> SocketAddr {
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(a, b, c, d), port))
}

fn tcp(addr: SocketAddr) -> bool {
    TcpStream::connect_timeout(&addr, LIMIT).is_ok()
}

fn attempt(name: &str) -> bool {
    match name {
        "ipv4" => tcp(v4(192, 0, 2, 1, 443)),
        "ipv6" => tcp(SocketAddr::V6(SocketAddrV6::new(
            Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1),
            443,
            0,
            0,
        ))),
        "dns" => ("halvo-egress-probe.invalid", 443)
            .to_socket_addrs()
            .map(|mut found| found.next().is_some())
            .unwrap_or(false),
        "loopback" => tcp(v4(127, 0, 0, 1, 47201)),
        "loopback2" => tcp(v4(127, 0, 0, 1, 47202)),
        "udp" => UdpSocket::bind(v4(0, 0, 0, 0, 0))
            .and_then(|socket| socket.send_to(&[0u8], v4(192, 0, 2, 1, 9)))
            .map(|sent| sent == 1)
            .unwrap_or(false),
        "none" => true,
        "listen" => listen(),
        _ => false,
    }
}

fn listen() -> bool {
    let addr = v4(127, 0, 0, 1, 47201);
    let Ok(listener) = TcpListener::bind(addr) else {
        return false;
    };
    let Ok(_client) = TcpStream::connect_timeout(&addr, LIMIT) else {
        return false;
    };
    listener.accept().is_ok()
}

fn run(status_path: &str, pid_path: &str, command: &[String]) -> i32 {
    if fs::write(pid_path, format!("{}\n", std::process::id())).is_err() {
        return 2;
    }
    let status = match Command::new(&command[0]).args(&command[1..]).status() {
        Ok(status) => describe(status),
        Err(_) => "not-started".to_string(),
    };
    match fs::write(status_path, format!("{status}\n")) {
        Ok(()) => 0,
        Err(_) => 2,
    }
}

#[cfg(unix)]
fn describe(status: std::process::ExitStatus) -> String {
    use std::os::unix::process::ExitStatusExt;
    match (status.code(), status.signal()) {
        (Some(code), _) => code.to_string(),
        (None, Some(signal)) => format!("signal-{signal}"),
        (None, None) => "not-started".to_string(),
    }
}

#[cfg(not(unix))]
fn describe(status: std::process::ExitStatus) -> String {
    match status.code() {
        Some(code) => code.to_string(),
        None => "not-started".to_string(),
    }
}

fn serve(port: &str, stop: &str, ready: &str) -> i32 {
    let Ok(port) = port.parse::<u16>() else {
        eprintln!("REFUSED usage: {port}");
        return 2;
    };
    let Ok(listener) = TcpListener::bind(v4(127, 0, 0, 1, port)) else {
        return 3;
    };
    if listener.set_nonblocking(true).is_err() || fs::write(ready, "ready\n").is_err() {
        return 2;
    }
    let started = Instant::now();
    while !Path::new(stop).exists() && started.elapsed() < Duration::from_secs(120) {
        match listener.accept() {
            Ok((stream, _)) => drop(stream),
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(_) => thread::sleep(Duration::from_millis(20)),
        }
    }
    0
}
