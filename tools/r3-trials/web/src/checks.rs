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
/// by creation time, and the processes that the stop reports as adopted (A11.1).
pub fn tree_of(table: &[Proc], root: &Proc) -> Vec<Proc> {
    let mut tree = procs::descendants(table, std::slice::from_ref(root));
    tree.retain(|p| table.iter().any(|t| procs::same(t, p)));
    let roots: Vec<std::path::PathBuf> = root
        .exe
        .as_deref()
        .map(procs::bundle_root)
        .into_iter()
        .collect();
    let adopted = procs::adopted(table, &tree, root.started_us, &roots);
    for a in adopted {
        if !tree.iter().any(|t| procs::same(t, &a)) {
            tree.push(a);
        }
    }
    tree
}

/// Whether a command line names a debugging port.
pub fn names_a_port(command_line: &str) -> bool {
    command_line.contains("--remote-debugging-port")
}

/// Reads every rule once. A rule whose read the system refused counts each unread process as one.
pub fn check(s: &Subject<'_>) -> Vec<Count> {
    let table = procs::table().unwrap_or_default();
    let tree = tree_of(&table, s.root);
    let pids: Vec<u32> = tree.iter().map(|p| p.pid).collect();
    let mut port_flag = 0;
    for p in &tree {
        match command_line(p.pid) {
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
    let executor_listener = peer::sockets(&[me])
        .iter()
        .filter(|x| x.pid == me && x.proto == Proto::Tcp && x.listening)
        .filter(|x| Some(x.local.1) != s.relay_port)
        .count();
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

/// The processes but the browser's first one and this process that hold an end of the pipe,
/// among the processes that started at or after the browser's first process (an older process
/// cannot have inherited an end).
pub fn pipe_holders(table: &[Proc], root: &Proc, ids: &PipeIds) -> usize {
    let me = std::process::id();
    table
        .iter()
        .filter(|p| p.pid != me && !procs::same(p, root))
        .filter(|p| p.started_us == 0 || p.started_us >= root.started_us)
        .filter(|p| holds_an_end(p.pid, ids))
        .count()
}

/// Whether process `pid` holds an end of the pipe `ids`.
pub fn holds_an_end(pid: u32, ids: &PipeIds) -> bool {
    sys::holds_an_end(pid, ids)
}

/// The command line of `pid`, as the system gives it to a process of the same user.
pub fn command_line(pid: u32) -> Option<String> {
    sys::command_line(pid)
}

#[cfg(target_os = "macos")]
mod sys {
    use crate::start::PipeIds;

    pub(super) fn holds_an_end(pid: u32, ids: &PipeIds) -> bool {
        crate::peer::pipe_handles_of(pid)
            .map(|held| held.iter().any(|(_, h)| *h != 0 && ids.handles.contains(h)))
            .unwrap_or(false)
    }

    pub(super) fn command_line(pid: u32) -> Option<String> {
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
        Some(args.join(" "))
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
        crate::peer::pipe_holders(pid, &values, &ids.names)
            .map(|found| !found.is_empty())
            .unwrap_or(false)
    }

    pub(super) fn command_line(pid: u32) -> Option<String> {
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
        Some(String::from_utf16_lossy(text))
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod sys {
    use crate::start::PipeIds;
    pub(super) fn holds_an_end(_pid: u32, _ids: &PipeIds) -> bool {
        false
    }
    pub(super) fn command_line(_pid: u32) -> Option<String> {
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
    fn this_process_s_command_line_is_read() {
        let line = command_line(std::process::id()).unwrap_or_default();
        let me = std::env::current_exe().unwrap();
        let name = me.file_name().unwrap().to_string_lossy().into_owned();
        assert!(line.contains(&name), "{name}");
    }
}
