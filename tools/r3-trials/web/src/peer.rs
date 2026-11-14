//! Who holds a socket or a pipe end: the system's tables that the relay's peer check (A11.1,
//! CP-89 (e), CP-93 (6)) and the checks of a session (CP-89 (c)) read. On macOS the descriptors of
//! each process of this user through `proc_pidinfo` and `proc_pidfdinfo`, a private interface of
//! the system that reads a process of the same user alone; on Windows the tables of the IP helper
//! with owner process ids.

use std::net::IpAddr;

/// The protocol of a socket that a table names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Proto {
    Tcp,
    Udp,
}

/// One socket of a process: its protocol, its local and remote address, and for TCP whether it
/// accepts connections.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Socket {
    pub pid: u32,
    pub proto: Proto,
    pub local: (IpAddr, u16),
    pub remote: (IpAddr, u16),
    pub listening: bool,
}

/// The processes that hold the TCP connection whose local end is 127.0.0.1:`peer_port` and whose
/// remote end is 127.0.0.1:`relay_port`, the relay's address, as the system's table reads them now, each
/// process once. `candidates` are the processes whose descriptors are read on macOS; Windows
/// reads its one table and takes no candidates.
pub fn owners_of_connection(peer_port: u16, relay_port: u16, candidates: &[u32]) -> Vec<u32> {
    let mut owners: Vec<u32> = sockets(candidates)
        .into_iter()
        .filter(|s| {
            s.proto == Proto::Tcp
                && !s.listening
                && s.local.1 == peer_port
                && s.remote.1 == relay_port
                && s.local.0 == IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
                && s.remote.0 == IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
        })
        .map(|s| s.pid)
        .collect();
    owners.sort_unstable();
    owners.dedup();
    owners
}

/// The sockets of the processes `pids` (macOS) or of every process (Windows) that the system
/// lets this process read.
pub fn sockets(pids: &[u32]) -> Vec<Socket> {
    sys::sockets(pids)
}

/// Every process number of the system.
pub fn all_pids() -> Vec<u32> {
    sys::all_pids()
}

#[cfg(windows)]
pub use sys::pipe_holders;
#[cfg(target_os = "macos")]
pub use sys::{fds_of, pipe_handle_of, pipe_handles_of};

#[cfg(target_os = "macos")]
mod sys {
    use super::{Proto, Socket};
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    const PROC_ALL_PIDS: u32 = 1;
    const PROC_PIDFDSOCKETINFO: i32 = 3;
    const PROC_PIDFDPIPEINFO: i32 = 6;
    const PROX_FDTYPE_SOCKET: u32 = 2;
    const PROX_FDTYPE_PIPE: u32 = 6;
    const SOCKET_FDINFO_SIZE: usize = 792;
    const PIPE_FDINFO_SIZE: usize = 184;
    const SOI_KIND: usize = 256;
    const SOI_PROTOCOL: usize = 180;
    const SOI_PROTO: usize = 264;
    const SOCKINFO_IN: i32 = 1;
    const SOCKINFO_TCP: i32 = 2;
    const INSI_FPORT: usize = 0;
    const INSI_LPORT: usize = 4;
    const INSI_VFLAG: usize = 24;
    const INSI_FADDR: usize = 32;
    const INSI_LADDR: usize = 48;
    const TCPSI_STATE: usize = 80;
    const TSI_S_LISTEN: i32 = 1;
    const INI_IPV4: u8 = 1;
    const PIPE_HANDLE: usize = 160;
    const IPPROTO_UDP: i32 = 17;

    pub fn all_pids() -> Vec<u32> {
        // SAFETY: a null buffer asks for the size in bytes of the list.
        let size = unsafe { libc::proc_listpids(PROC_ALL_PIDS, 0, std::ptr::null_mut(), 0) };
        if size <= 0 {
            return Vec::new();
        }
        let mut pids = vec![0i32; size as usize / 4 + 64];
        // SAFETY: the buffer holds `pids.len() * 4` bytes, which the call is told.
        let got = unsafe {
            libc::proc_listpids(
                PROC_ALL_PIDS,
                0,
                pids.as_mut_ptr() as *mut libc::c_void,
                (pids.len() * 4) as i32,
            )
        };
        if got <= 0 {
            return Vec::new();
        }
        pids.truncate(got as usize / 4);
        pids.into_iter()
            .filter(|p| *p > 0)
            .map(|p| p as u32)
            .collect()
    }

    /// The descriptors of `pid` with their types, or none when the system refuses the read.
    pub fn fds_of(pid: u32) -> Option<Vec<(i32, u32)>> {
        // SAFETY: a null buffer asks for the size in bytes of the descriptor list.
        let size = unsafe {
            libc::proc_pidinfo(
                pid as i32,
                libc::PROC_PIDLISTFDS,
                0,
                std::ptr::null_mut(),
                0,
            )
        };
        if size <= 0 {
            return None;
        }
        let n = size as usize / std::mem::size_of::<libc::proc_fdinfo>() + 16;
        let mut list: Vec<libc::proc_fdinfo> = Vec::with_capacity(n);
        // SAFETY: the buffer has room for `n` entries, which the call is told in bytes; the
        // returned size says how many it wrote, and only those are read.
        let got = unsafe {
            libc::proc_pidinfo(
                pid as i32,
                libc::PROC_PIDLISTFDS,
                0,
                list.as_mut_ptr() as *mut libc::c_void,
                (n * std::mem::size_of::<libc::proc_fdinfo>()) as i32,
            )
        };
        if got <= 0 {
            return None;
        }
        // SAFETY: the call wrote `got` bytes of whole entries at the start of the buffer.
        unsafe { list.set_len(got as usize / std::mem::size_of::<libc::proc_fdinfo>()) };
        Some(list.iter().map(|e| (e.proc_fd, e.proc_fdtype)).collect())
    }

    /// Reads the information of flavour `flavor` of descriptor `fd` of `pid` into a buffer of
    /// `size` bytes, aligned for 8-byte fields.
    fn fd_info(pid: u32, fd: i32, flavor: i32, size: usize) -> Option<Vec<u64>> {
        let mut buf = vec![0u64; size.div_ceil(8)];
        // SAFETY: the buffer holds at least `size` bytes, which the call is told.
        let got = unsafe {
            libc::proc_pidfdinfo(
                pid as i32,
                fd,
                flavor,
                buf.as_mut_ptr() as *mut libc::c_void,
                size as i32,
            )
        };
        (got as usize == size).then_some(buf)
    }

    fn bytes(buf: &[u64]) -> &[u8] {
        // SAFETY: a slice of u64 is readable as bytes of eight times its length.
        unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const u8, buf.len() * 8) }
    }

    fn i32_at(b: &[u8], at: usize) -> i32 {
        i32::from_ne_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
    }

    fn u64_at(b: &[u8], at: usize) -> u64 {
        let mut a = [0u8; 8];
        a.copy_from_slice(&b[at..at + 8]);
        u64::from_ne_bytes(a)
    }

    fn port_at(b: &[u8], at: usize) -> u16 {
        u16::from_be(i32_at(b, at) as u16)
    }

    fn addr_at(b: &[u8], at: usize, v4: bool) -> IpAddr {
        if v4 {
            IpAddr::V4(Ipv4Addr::new(
                b[at + 12],
                b[at + 13],
                b[at + 14],
                b[at + 15],
            ))
        } else {
            let mut a = [0u8; 16];
            a.copy_from_slice(&b[at..at + 16]);
            IpAddr::V6(Ipv6Addr::from(a))
        }
    }

    /// The system's handle of the pipe end at descriptor `fd` of `pid`.
    pub fn pipe_handle_of(pid: u32, fd: i32) -> Option<u64> {
        let buf = fd_info(pid, fd, PROC_PIDFDPIPEINFO, PIPE_FDINFO_SIZE)?;
        Some(u64_at(bytes(&buf), PIPE_HANDLE))
    }

    /// The descriptors of `pid` that are pipe ends, each with the system's handle of its end.
    pub fn pipe_handles_of(pid: u32) -> Option<Vec<(i32, u64)>> {
        let fds = fds_of(pid)?;
        Some(
            fds.into_iter()
                .filter(|(_, t)| *t == PROX_FDTYPE_PIPE)
                .filter_map(|(fd, _)| pipe_handle_of(pid, fd).map(|h| (fd, h)))
                .collect(),
        )
    }

    fn socket_of(pid: u32, fd: i32) -> Option<Socket> {
        let buf = fd_info(pid, fd, PROC_PIDFDSOCKETINFO, SOCKET_FDINFO_SIZE)?;
        let b = bytes(&buf);
        let kind = i32_at(b, SOI_KIND);
        let proto = match (kind, i32_at(b, SOI_PROTOCOL)) {
            (SOCKINFO_TCP, _) => Proto::Tcp,
            (SOCKINFO_IN, IPPROTO_UDP) => Proto::Udp,
            _ => return None,
        };
        let ini = SOI_PROTO;
        let v4 = b[ini + INSI_VFLAG] & INI_IPV4 != 0;
        Some(Socket {
            pid,
            proto,
            local: (
                addr_at(b, ini + INSI_LADDR, v4),
                port_at(b, ini + INSI_LPORT),
            ),
            remote: (
                addr_at(b, ini + INSI_FADDR, v4),
                port_at(b, ini + INSI_FPORT),
            ),
            listening: proto == Proto::Tcp && i32_at(b, ini + TCPSI_STATE) == TSI_S_LISTEN,
        })
    }

    pub fn sockets(pids: &[u32]) -> Vec<Socket> {
        let mut out = Vec::new();
        for pid in pids {
            let Some(fds) = fds_of(*pid) else { continue };
            for (fd, kind) in fds {
                if kind == PROX_FDTYPE_SOCKET {
                    if let Some(s) = socket_of(*pid, fd) {
                        out.push(s);
                    }
                }
            }
        }
        out
    }
}

#[cfg(windows)]
mod sys {
    use super::{Proto, Socket};
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    use windows_sys::Win32::Foundation::{
        CloseHandle, CompareObjectHandles, DuplicateHandle, DUPLICATE_SAME_ACCESS, HANDLE,
    };
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, GetExtendedUdpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID,
        MIB_UDP6ROW_OWNER_PID, MIB_UDPROW_OWNER_PID, TCP_TABLE_OWNER_PID_ALL, UDP_TABLE_OWNER_PID,
    };
    use windows_sys::Win32::Networking::WinSock::{AF_INET, AF_INET6};
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, OpenProcess, PROCESS_DUP_HANDLE,
    };

    const MIB_TCP_STATE_LISTEN: u32 = 2;

    pub fn all_pids() -> Vec<u32> {
        crate::procs::table()
            .map(|t| t.into_iter().map(|p| p.pid).collect())
            .unwrap_or_default()
    }

    /// Reads one table of the IP helper into a buffer aligned for its rows.
    fn table(read: impl Fn(*mut core::ffi::c_void, *mut u32) -> u32) -> Option<Vec<u64>> {
        let mut size = 0u32;
        read(std::ptr::null_mut(), &mut size);
        for _ in 0..4 {
            let mut buf = vec![0u64; (size as usize).div_ceil(8) + 64];
            let mut have = (buf.len() * 8) as u32;
            if read(buf.as_mut_ptr() as *mut core::ffi::c_void, &mut have) == 0 {
                return Some(buf);
            }
            size = have;
        }
        None
    }

    /// The rows of a table that starts with a count of `u32` and an array of `T`.
    fn rows<T: Copy>(buf: &[u64]) -> Vec<T> {
        let base = buf.as_ptr() as *const u8;
        // SAFETY: the table starts with a u32 count followed, at the alignment of `T`, by that
        // many rows, all inside the buffer that the table call filled.
        unsafe {
            let n = *(base as *const u32) as usize;
            let first = base.add(std::mem::align_of::<T>().max(4)) as *const T;
            let room =
                (buf.len() * 8 - std::mem::align_of::<T>().max(4)) / std::mem::size_of::<T>();
            (0..n.min(room)).map(|i| *first.add(i)).collect()
        }
    }

    fn port(p: u32) -> u16 {
        u16::from_be(p as u16)
    }

    pub fn sockets(_pids: &[u32]) -> Vec<Socket> {
        let mut out = Vec::new();
        if let Some(buf) = table(|p, size| {
            // SAFETY: `table` gives a buffer and its size in bytes; the call writes at most `*size`
            // bytes into the buffer and reports a larger need through `size`.
            unsafe { GetExtendedTcpTable(p, size, 0, AF_INET as u32, TCP_TABLE_OWNER_PID_ALL, 0) }
        }) {
            for r in rows::<MIB_TCPROW_OWNER_PID>(&buf) {
                out.push(Socket {
                    pid: r.dwOwningPid,
                    proto: Proto::Tcp,
                    local: (
                        IpAddr::V4(Ipv4Addr::from(u32::from_be(r.dwLocalAddr))),
                        port(r.dwLocalPort),
                    ),
                    remote: (
                        IpAddr::V4(Ipv4Addr::from(u32::from_be(r.dwRemoteAddr))),
                        port(r.dwRemotePort),
                    ),
                    listening: r.dwState == MIB_TCP_STATE_LISTEN,
                });
            }
        }
        if let Some(buf) = table(|p, size| {
            // SAFETY: as for the IPv4 table above: a buffer and its size from `table`.
            unsafe { GetExtendedTcpTable(p, size, 0, AF_INET6 as u32, TCP_TABLE_OWNER_PID_ALL, 0) }
        }) {
            for r in rows::<MIB_TCP6ROW_OWNER_PID>(&buf) {
                out.push(Socket {
                    pid: r.dwOwningPid,
                    proto: Proto::Tcp,
                    local: (
                        IpAddr::V6(Ipv6Addr::from(r.ucLocalAddr)),
                        port(r.dwLocalPort),
                    ),
                    remote: (
                        IpAddr::V6(Ipv6Addr::from(r.ucRemoteAddr)),
                        port(r.dwRemotePort),
                    ),
                    listening: r.dwState == MIB_TCP_STATE_LISTEN,
                });
            }
        }
        if let Some(buf) = table(|p, size| {
            // SAFETY: as for the TCP tables above: a buffer and its size from `table`.
            unsafe { GetExtendedUdpTable(p, size, 0, AF_INET as u32, UDP_TABLE_OWNER_PID, 0) }
        }) {
            for r in rows::<MIB_UDPROW_OWNER_PID>(&buf) {
                out.push(Socket {
                    pid: r.dwOwningPid,
                    proto: Proto::Udp,
                    local: (
                        IpAddr::V4(Ipv4Addr::from(u32::from_be(r.dwLocalAddr))),
                        port(r.dwLocalPort),
                    ),
                    remote: (IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
                    listening: false,
                });
            }
        }
        if let Some(buf) = table(|p, size| {
            // SAFETY: as for the TCP tables above: a buffer and its size from `table`.
            unsafe { GetExtendedUdpTable(p, size, 0, AF_INET6 as u32, UDP_TABLE_OWNER_PID, 0) }
        }) {
            for r in rows::<MIB_UDP6ROW_OWNER_PID>(&buf) {
                out.push(Socket {
                    pid: r.dwOwningPid,
                    proto: Proto::Udp,
                    local: (
                        IpAddr::V6(Ipv6Addr::from(r.ucLocalAddr)),
                        port(r.dwLocalPort),
                    ),
                    remote: (IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0),
                    listening: false,
                });
            }
        }
        out
    }

    /// The handle values among `values` of process `pid` that name the same object as one of
    /// `ours`, handles of this process: each value is duplicated into this process and compared
    /// with `CompareObjectHandles`, which makes no request of the object, so a pipe with a pending
    /// read in `pid` cannot hold the check up. None when the process cannot be opened for
    /// duplication.
    pub fn pipe_holders(pid: u32, values: &[usize], ours: &[usize]) -> Option<Vec<usize>> {
        // SAFETY: OpenProcess returns a new handle or null.
        let process = unsafe { OpenProcess(PROCESS_DUP_HANDLE, 0, pid) };
        if process.is_null() {
            return None;
        }
        let mut found = Vec::new();
        for v in values {
            let mut dup: HANDLE = std::ptr::null_mut();
            // SAFETY: the duplicate is made in this process and closed below.
            let ok = unsafe {
                DuplicateHandle(
                    process,
                    *v as HANDLE,
                    GetCurrentProcess(),
                    &mut dup,
                    0,
                    0,
                    DUPLICATE_SAME_ACCESS,
                )
            };
            if ok == 0 {
                continue;
            }
            if ours.iter().any(|o| {
                // SAFETY: both handles are open handles of this process; the call reads the objects
                // that they name and nothing else.
                unsafe { CompareObjectHandles(dup, *o as HANDLE) != 0 }
            }) {
                found.push(*v);
            }
            // SAFETY: the duplicate is owned here and closed once.
            unsafe { CloseHandle(dup) };
        }
        // SAFETY: the process handle is owned here and closed once.
        unsafe { CloseHandle(process) };
        Some(found)
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod sys {
    use super::Socket;
    pub fn all_pids() -> Vec<u32> {
        Vec::new()
    }
    pub fn sockets(_pids: &[u32]) -> Vec<Socket> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_names_this_process_as_the_one_owner_of_its_own_connection() {
        let held = crate::start::lock();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let relay_port = listener.local_addr().expect("addr").port();
        let client = std::net::TcpStream::connect(("127.0.0.1", relay_port)).expect("connect");
        drop(held);
        let (_accepted, _) = listener.accept().expect("accept");
        let peer_port = client.local_addr().expect("addr").port();
        let me = std::process::id();
        assert_eq!(owners_of_connection(peer_port, relay_port, &[me]), vec![me]);
        assert!(owners_of_connection(peer_port, relay_port.wrapping_add(1), &[me]).is_empty());
        let mine = sockets(&[me]);
        assert!(mine
            .iter()
            .any(|s| s.pid == me && s.listening && s.local.1 == relay_port));
        assert!(all_pids().contains(&me));
    }
}
