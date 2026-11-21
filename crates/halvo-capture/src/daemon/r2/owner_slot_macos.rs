//! The owner slot on macOS: the channel is one socket pair, so that an end of a stream travels beside a frame, and
//! the owner gets its end at the descriptors 3 and 4; the start time of a process is the delete walk's reading
//! alone; an orphan is ended by signals.

use std::fs::{self, DirBuilder, File, Metadata, OpenOptions, Permissions};
use std::io::{self, ErrorKind};
use std::net::Shutdown;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use super::{start_time, Owner, ORPHAN_TERM_WAIT, POLL, REPLY_WAIT};

/// How long an orphan has to end after SIGKILL before the slot gives up on it.
const ORPHAN_KILL_WAIT: Duration = Duration::from_secs(1);

/// The slot's end of the channel on which it writes, and the copy of that end that its reader thread reads.
pub(super) type Writes = UnixStream;
pub(super) type Reads = UnixStream;

/// The slot's ends of a new channel.
pub(super) struct Channel {
    pub(super) writes: Writes,
    pub(super) reads: Reads,
}

/// The highest descriptor number, plus one, that the child's start looks at.
fn descriptor_limit() -> libc::c_int {
    // SAFETY: sysconf reads a limit of the system and touches no memory.
    match unsafe { libc::sysconf(libc::_SC_OPEN_MAX) } {
        limit if limit > 5 => libc::c_int::try_from(limit.min(65_536)).unwrap_or(65_536),
        _ => 65_536,
    }
}

/// Starts `command` with the owner's end of a new socket pair at the descriptors 3 and 4, named by `--channel=3,4`,
/// and close-on-exec on every other descriptor above the standard streams. A write of the slot on the channel blocks
/// at most as long as a call waits for its reply.
pub(super) fn launch(command: &mut Command) -> io::Result<(Child, Channel)> {
    let (owners, writes) = UnixStream::pair()?;
    writes.set_write_timeout(Some(REPLY_WAIT))?;
    let reads = writes.try_clone()?;
    let end = owners.as_raw_fd();
    let limit = descriptor_limit();
    command.arg("--channel=3,4");
    // SAFETY: the closure runs in the child between fork and exec and calls fcntl and dup2 alone, which are
    // async-signal-safe. It first copies the owner's end above 9, so that no dup2 writes over it before it is copied.
    unsafe {
        command.pre_exec(move || {
            let high = libc::fcntl(end, libc::F_DUPFD_CLOEXEC, 10);
            if high < 0 {
                return Err(io::Error::last_os_error());
            }
            for place in [3, 4] {
                if libc::dup2(high, place) < 0 {
                    return Err(io::Error::last_os_error());
                }
            }
            for descriptor in 5..limit {
                let flags = libc::fcntl(descriptor, libc::F_GETFD);
                if flags >= 0 && flags & libc::FD_CLOEXEC == 0 {
                    libc::fcntl(descriptor, libc::F_SETFD, flags | libc::FD_CLOEXEC);
                }
            }
            Ok(())
        });
    }
    let child = command.spawn();
    drop(owners);
    Ok((child?, Channel { writes, reads }))
}

/// Closes the slot's side of the channel: the owner reads its end, and the reader thread's copy reads on.
pub(super) fn close(writes: Writes) {
    let _ = writes.shutdown(Shutdown::Write);
}

fn own_uid() -> libc::uid_t {
    // SAFETY: geteuid takes no argument and cannot fail.
    unsafe { libc::geteuid() }
}

/// Whether the pid file found at the slot's make is this user's, with no mode bit beyond 0600.
pub(super) fn private(meta: &Metadata) -> bool {
    meta.uid() == own_uid() && meta.mode() & 0o7777 & !0o600 == 0
}

/// Makes `derived/` with mode 0700 where it is missing, and narrows a folder of this user to 0700.
pub(super) fn private_dir(dir: &Path) -> io::Result<()> {
    match DirBuilder::new().mode(0o700).create(dir) {
        Err(error) if error.kind() != ErrorKind::AlreadyExists => return Err(error),
        _ => {}
    }
    let meta = fs::symlink_metadata(dir)?;
    if !meta.is_dir() || meta.uid() != own_uid() {
        return Err(ErrorKind::PermissionDenied.into());
    }
    if meta.mode() & 0o7777 != 0o700 {
        fs::set_permissions(dir, Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// A new file of mode 0600; it fails where any entry stands at the name.
pub(super) fn create_private(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

/// No reading beside the delete walk's on macOS, which reads the process table itself.
pub(super) fn creation_time(_pid: u32) -> Option<u64> {
    None
}

/// Whether a process of `pid` exists, of this user or of another.
pub(super) fn exists(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    // SAFETY: kill with the signal 0 sends nothing; it checks the pid and touches no memory.
    let found = unsafe { libc::kill(pid, 0) };
    found == 0 || io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

fn gone_within(owner: Owner, wait: Duration) -> bool {
    let until = Instant::now() + wait;
    while start_time(owner.pid) == Some(owner.started) {
        if Instant::now() >= until {
            return false;
        }
        std::thread::sleep(POLL);
    }
    true
}

/// Ends the orphan `owner`: its start time is read once more right before each signal, SIGTERM and after 5 s
/// SIGKILL. True when no process of that pid and start time runs after the call.
pub(super) fn end(owner: Owner) -> bool {
    let Ok(pid) = libc::pid_t::try_from(owner.pid) else {
        return false;
    };
    if start_time(owner.pid) != Some(owner.started) {
        return true;
    }
    // SAFETY: kill takes a pid and a signal number and touches no memory.
    unsafe { libc::kill(pid, libc::SIGTERM) };
    if gone_within(owner, ORPHAN_TERM_WAIT) {
        return true;
    }
    if start_time(owner.pid) == Some(owner.started) {
        // SAFETY: as above.
        unsafe { libc::kill(pid, libc::SIGKILL) };
    }
    gone_within(owner, ORPHAN_KILL_WAIT)
}
