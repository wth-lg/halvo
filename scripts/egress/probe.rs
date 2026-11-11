//! The probe and helper binary of the egress check (contracts.md C26.6).
//!
//! `probe.sh` builds this file into `target/egress/bin/egress-spawn` and copies it to
//! `egress-probe`. One binary serves four roles, chosen by the first argument:
//!
//! - `spawn NAME`: runs `egress-probe connect NAME` as a child and exits with its status,
//!   so that the probe is a grandchild of the shell of probe.sh under another image name.
//! - `connect NAME`: makes the connect, send or lookup of probe NAME and exits 0 when it
//!   succeeded, 1 when it failed or 5 s passed.
//! - `run STATUS -- COMMAND [ARG...]`: starts COMMAND with the caller's standard streams,
//!   environment and directory, and writes to STATUS the exit code, `signal-<n>` or
//!   `not-started`; it exits 0 once STATUS is written. STATUS `fd3` is the descriptor 3 that the
//!   caller passes, which COMMAND does not inherit. On Windows COMMAND runs with a token that
//!   holds no administrator rights.
//! - `serve PORT STOP READY`: listens on 127.0.0.1:PORT, writes READY once bound, accepts and
//!   closes connections until the file STOP exists or 120 s pass; it exits 3 when the bind fails.

use std::env;
use std::fs;
use std::io::{ErrorKind, Write};
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
const NAMES: [&str; 11] = [
    "ipv4",
    "ipv6",
    "dns",
    "loopback",
    "udp",
    "icmp",
    "resolver",
    "session",
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
        "run" if args.len() >= 5 && args[3] == "--" => run(&args[2], &args[4..]),
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
        "icmp" => icmp(),
        "resolver" => tool(
            "/usr/bin/nscurl",
            &["-m", "4", "https://egress-probe-resolver.invalid/"],
        ),
        "session" => tool(
            "/usr/bin/nscurl",
            &[
                "--background",
                "-m",
                "4",
                "http://127.0.0.1:47202/egress-probe-session",
            ],
        ),
        "none" => true,
        "listen" => listen(),
        _ => false,
    }
}

fn tool(path: &str, args: &[&str]) -> bool {
    let Ok(mut child) = Command::new(path).args(args).spawn() else {
        return false;
    };
    let started = Instant::now();
    while started.elapsed() < LIMIT - Duration::from_millis(500) {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(_) => break,
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    false
}

#[cfg(target_os = "macos")]
fn icmp() -> bool {
    extern "C" {
        fn socket(domain: i32, kind: i32, protocol: i32) -> i32;
        fn sendto(
            fd: i32,
            buf: *const u8,
            len: usize,
            flags: i32,
            addr: *const u8,
            len: u32,
        ) -> isize;
        fn close(fd: i32) -> i32;
    }
    let mut packet = [8u8, 0, 0, 0, 0x45, 0x47, 0, 1];
    let mut sum: u32 = 0;
    for pair in packet.chunks(2) {
        sum += u32::from(u16::from_be_bytes([pair[0], pair[1]]));
    }
    while sum > 0xffff {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    let check = !(sum as u16);
    packet[2..4].copy_from_slice(&check.to_be_bytes());
    let addr: [u8; 16] = [16, 2, 0, 0, 192, 0, 2, 1, 0, 0, 0, 0, 0, 0, 0, 0];
    unsafe {
        let fd = socket(2, 2, 1);
        if fd < 0 {
            return false;
        }
        let sent = sendto(fd, packet.as_ptr(), packet.len(), 0, addr.as_ptr(), 16);
        close(fd);
        sent == packet.len() as isize
    }
}

#[cfg(windows)]
fn icmp() -> bool {
    #[link(name = "iphlpapi")]
    extern "system" {
        fn IcmpCreateFile() -> isize;
        fn IcmpSendEcho(
            handle: isize,
            dest: u32,
            data: *const u8,
            size: u16,
            options: *const u8,
            reply: *mut u8,
            reply_size: u32,
            timeout: u32,
        ) -> u32;
        fn IcmpCloseHandle(handle: isize) -> i32;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetLastError() -> u32;
    }
    let data = [0u8; 8];
    let mut reply = [0u8; 256];
    unsafe {
        let handle = IcmpCreateFile();
        if handle == -1 {
            return false;
        }
        let count = IcmpSendEcho(
            handle,
            u32::from_ne_bytes([192, 0, 2, 1]),
            data.as_ptr(),
            data.len() as u16,
            std::ptr::null(),
            reply.as_mut_ptr(),
            reply.len() as u32,
            2000,
        );
        let error = GetLastError();
        IcmpCloseHandle(handle);
        count > 0 || error == 11010
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
fn icmp() -> bool {
    false
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

fn run(status_path: &str, command: &[String]) -> i32 {
    let Some(mut sink) = status_sink(status_path) else {
        return 2;
    };
    let status = start(command);
    match sink.write_all(format!("{status}\n").as_bytes()) {
        Ok(()) => 0,
        Err(_) => 2,
    }
}

#[cfg(unix)]
fn status_sink(path: &str) -> Option<fs::File> {
    use std::os::unix::io::FromRawFd;
    extern "C" {
        fn fcntl(fd: i32, cmd: i32, ...) -> i32;
    }
    if path != "fd3" {
        return fs::File::create(path).ok();
    }
    if unsafe { fcntl(3, 2, 1) } == -1 {
        return None;
    }
    Some(unsafe { fs::File::from_raw_fd(3) })
}

#[cfg(not(unix))]
fn status_sink(path: &str) -> Option<fs::File> {
    fs::File::create(path).ok()
}

#[cfg(not(windows))]
fn start(command: &[String]) -> String {
    match Command::new(&command[0]).args(&command[1..]).status() {
        Ok(status) => describe(status),
        Err(_) => "not-started".to_string(),
    }
}

#[cfg(windows)]
fn start(command: &[String]) -> String {
    lowered::start(command)
}

#[cfg(windows)]
mod lowered {
    use std::ffi::c_void;
    use std::iter::repeat;

    type Handle = *mut c_void;

    #[repr(C)]
    struct SidAndAttributes {
        sid: *mut c_void,
        attributes: u32,
    }

    #[repr(C)]
    struct StartupInfo {
        cb: u32,
        reserved: *mut u16,
        desktop: *mut u16,
        title: *mut u16,
        x: u32,
        y: u32,
        x_size: u32,
        y_size: u32,
        x_chars: u32,
        y_chars: u32,
        fill: u32,
        flags: u32,
        show: u16,
        reserved2_len: u16,
        reserved2: *mut u8,
        input: Handle,
        output: Handle,
        error: Handle,
    }

    #[repr(C)]
    struct ProcessInformation {
        process: Handle,
        thread: Handle,
        process_id: u32,
        thread_id: u32,
    }

    #[link(name = "advapi32")]
    extern "system" {
        fn OpenProcessToken(process: Handle, access: u32, token: *mut Handle) -> i32;
        fn CreateRestrictedToken(
            existing: Handle,
            flags: u32,
            disable_count: u32,
            disable: *const SidAndAttributes,
            delete_count: u32,
            delete: *const c_void,
            restrict_count: u32,
            restrict: *const SidAndAttributes,
            created: *mut Handle,
        ) -> i32;
        fn CreateWellKnownSid(
            kind: i32,
            domain: *const c_void,
            sid: *mut u8,
            size: *mut u32,
        ) -> i32;
        fn CreateProcessAsUserW(
            token: Handle,
            application: *const u16,
            command: *mut u16,
            process_attributes: *const c_void,
            thread_attributes: *const c_void,
            inherit: i32,
            flags: u32,
            environment: *const c_void,
            directory: *const u16,
            startup: *const StartupInfo,
            information: *mut ProcessInformation,
        ) -> i32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> Handle;
        fn GetStdHandle(which: u32) -> Handle;
        fn SetHandleInformation(handle: Handle, mask: u32, flags: u32) -> i32;
        fn WaitForSingleObject(handle: Handle, milliseconds: u32) -> u32;
        fn GetExitCodeProcess(handle: Handle, code: *mut u32) -> i32;
        fn CloseHandle(handle: Handle) -> i32;
    }

    fn quote(arg: &str, line: &mut String) {
        if !line.is_empty() {
            line.push(' ');
        }
        if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
            line.push_str(arg);
            return;
        }
        line.push('"');
        let mut slashes = 0;
        for c in arg.chars() {
            if c == '\\' {
                slashes += 1;
                continue;
            }
            if c == '"' {
                line.extend(repeat('\\').take(slashes * 2 + 1));
            } else {
                line.extend(repeat('\\').take(slashes));
            }
            line.push(c);
            slashes = 0;
        }
        line.extend(repeat('\\').take(slashes * 2));
        line.push('"');
    }

    pub fn start(command: &[String]) -> String {
        let mut line = String::new();
        for arg in command {
            quote(arg, &mut line);
        }
        let mut wide: Vec<u16> = line.encode_utf16().chain(Some(0)).collect();
        let mut admins = [0u8; 68];
        let mut admins_size = admins.len() as u32;
        unsafe {
            let mut own: Handle = std::ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), 0xF01FF, &mut own) == 0 {
                return "not-started".to_string();
            }
            if CreateWellKnownSid(26, std::ptr::null(), admins.as_mut_ptr(), &mut admins_size) == 0
            {
                CloseHandle(own);
                return "not-started".to_string();
            }
            let disable = [SidAndAttributes {
                sid: admins.as_mut_ptr().cast(),
                attributes: 0,
            }];
            let mut token: Handle = std::ptr::null_mut();
            let made = CreateRestrictedToken(
                own,
                0x1 | 0x4,
                1,
                disable.as_ptr(),
                0,
                std::ptr::null(),
                0,
                std::ptr::null(),
                &mut token,
            );
            CloseHandle(own);
            if made == 0 {
                return "not-started".to_string();
            }
            let mut startup: StartupInfo = std::mem::zeroed();
            startup.cb = std::mem::size_of::<StartupInfo>() as u32;
            startup.flags = 0x100;
            startup.input = GetStdHandle(-10i32 as u32);
            startup.output = GetStdHandle(-11i32 as u32);
            startup.error = GetStdHandle(-12i32 as u32);
            for handle in [startup.input, startup.output, startup.error] {
                SetHandleInformation(handle, 1, 1);
            }
            let mut information: ProcessInformation = std::mem::zeroed();
            let created = CreateProcessAsUserW(
                token,
                std::ptr::null(),
                wide.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1,
                0,
                std::ptr::null(),
                std::ptr::null(),
                &startup,
                &mut information,
            );
            CloseHandle(token);
            if created == 0 {
                return "not-started".to_string();
            }
            CloseHandle(information.thread);
            WaitForSingleObject(information.process, 0xFFFF_FFFF);
            let mut code: u32 = 0;
            let read = GetExitCodeProcess(information.process, &mut code);
            CloseHandle(information.process);
            if read == 0 {
                return "not-started".to_string();
            }
            (code as i32).to_string()
        }
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
