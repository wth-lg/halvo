//! The code that only unix builds: the start call, the inherited descriptors, the receipt of an end of a stream and
//! the local date.

use std::fs::File;
use std::io::{self, Read};
use std::os::fd::{AsRawFd, FromRawFd};
use std::sync::Once;

/// The start call: the limit of a core file is 0, soft and hard, so no core file of the owner holds the key. A crash
/// report of the system (ReportCrash on macOS) holds no image of the memory and is not turned off.
pub(crate) fn crash_reports_off() -> io::Result<()> {
    let zero = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: setrlimit reads the one rlimit value that the pointer names, which lives through the call.
    if unsafe { libc::setrlimit(libc::RLIMIT_CORE, &zero) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Takes the inherited descriptor `raw` as the owner's own, close-on-exec. It refuses 0, 1 and 2, a number above the
/// range of a descriptor, and a descriptor that is not open.
pub(crate) fn adopt(raw: u64) -> io::Result<File> {
    let fd = i32::try_from(raw)
        .ok()
        .filter(|fd| *fd > 2)
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "not an inherited descriptor")
        })?;
    // SAFETY: fcntl takes a descriptor number; on a number that is not open it fails with EBADF and changes nothing.
    if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the starter placed the descriptor in this process for the owner, `Args::parse` refuses two ends of one
    // number, and nothing else in this process owns it, so it is wrapped once.
    Ok(unsafe { File::from_raw_fd(fd) })
}

/// The read end of the starter's channel, with the descriptors that came beside its bytes since the reader last took
/// them. Where the end is a socket, every read is one `recvmsg`, which takes the descriptors of the control message
/// beside the bytes; where it is a pipe, nothing comes beside its bytes.
pub(crate) struct StarterEnd {
    end: File,
    socket: bool,
    came: Vec<File>,
}

/// The read end of the starter's channel as its reader thread holds it.
pub(crate) fn starter_end(end: File) -> StarterEnd {
    // SAFETY: an all-zero stat is a valid value of the type.
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: fstat reads the descriptor, which `end` keeps open, and writes the one stat value that the pointer
    // names, which lives through the call.
    let read = unsafe { libc::fstat(end.as_raw_fd(), &mut stat) } == 0;
    StarterEnd {
        end,
        socket: read && stat.st_mode & libc::S_IFMT == libc::S_IFSOCK,
        came: Vec::new(),
    }
}

impl Read for StarterEnd {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.socket {
            receive(&self.end, buf, &mut self.came)
        } else {
            self.end.read(buf)
        }
    }
}

impl crate::Inbound for StarterEnd {
    fn came(&mut self) -> Vec<File> {
        std::mem::take(&mut self.came)
    }
}

/// The bytes of the buffer for the control messages of one `recvmsg`. It is larger than the control message of the
/// most descriptors that one call of macOS or of Linux sends; `receive` fails where the system cut one all the same.
const CONTROL_LEN: usize = 4_096;

/// Reads from the socket `end` as a read does, and takes each descriptor that came beside the bytes as the owner's
/// own, close-on-exec, at the end of `came`. macOS has no flag that sets close-on-exec at the receipt; the owner
/// starts no child, so no child gets a copy between the two calls.
fn receive(end: &File, buf: &mut [u8], came: &mut Vec<File>) -> io::Result<usize> {
    let mut control = [0u64; CONTROL_LEN / 8];
    let mut iov = libc::iovec {
        iov_base: buf.as_mut_ptr().cast(),
        iov_len: buf.len(),
    };
    // SAFETY: an all-zero msghdr is a valid value of the type, whose pointers are then null.
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.as_mut_ptr().cast();
    msg.msg_controllen = CONTROL_LEN as _;
    // SAFETY: recvmsg writes at most `buf.len()` bytes into `buf` and at most CONTROL_LEN bytes into `control`,
    // which both live through the call, and sets in `msg` the length of the control messages that it wrote.
    let read = unsafe { libc::recvmsg(end.as_raw_fd(), &mut msg, 0) };
    let read = usize::try_from(read).map_err(|_| io::Error::last_os_error())?;
    let written = control.as_ptr() as usize + msg.msg_controllen as usize;
    // SAFETY: the system wrote `msg_controllen` bytes of control messages into `control`, which is aligned for a
    // header, and its macros walk the headers inside those bytes.
    let mut header = unsafe { libc::CMSG_FIRSTHDR(&msg) };
    while !header.is_null() {
        // SAFETY: `header` is a header inside the bytes that the system wrote, as above.
        let (level, kind, len) = unsafe {
            (
                (*header).cmsg_level,
                (*header).cmsg_type,
                (*header).cmsg_len,
            )
        };
        if (len as usize) < size_of::<libc::cmsghdr>() {
            break;
        }
        if level == libc::SOL_SOCKET && kind == libc::SCM_RIGHTS {
            // SAFETY: the macro adds the size of a header to the pointer, which stays inside `control`.
            let data = unsafe { libc::CMSG_DATA(header) } as usize;
            let after = written.min(header as usize + len as usize);
            let count = after.saturating_sub(data) / size_of::<libc::c_int>();
            for at in 0..count {
                let place = (data + at * size_of::<libc::c_int>()) as *const libc::c_int;
                // SAFETY: `place` is one whole descriptor number inside the bytes that the system wrote; the message
                // gives no alignment for it.
                let fd = unsafe { place.read_unaligned() };
                // SAFETY: fcntl takes a descriptor number; the system has just opened this one for the owner, and on
                // an open descriptor the call sets the flag.
                unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
                // SAFETY: the system opened the descriptor in this process for this message and nothing else owns
                // it, so it is wrapped once.
                came.push(unsafe { File::from_raw_fd(fd) });
            }
        }
        // SAFETY: as for the first header; the macro returns null after the last header inside the bytes.
        header = unsafe { libc::CMSG_NXTHDR(&msg, header) };
    }
    if msg.msg_flags & libc::MSG_CTRUNC != 0 {
        return Err(io::Error::other(
            "a control message beside the bytes was cut",
        ));
    }
    Ok(read)
}

/// The end of a stream that came beside a frame `stream`: exactly one descriptor, on which the owner writes, and a
/// copy of it, close-on-exec as well, on which its reader thread reads. A frame that names handle values, no
/// descriptor and more than one give none, and what came is closed.
pub(crate) fn stream_end(handles: Option<(u64, u64)>, mut came: Vec<File>) -> Option<(File, File)> {
    if handles.is_some() || came.len() != 1 {
        return None;
    }
    let writes = came.pop()?;
    let reads = writes.try_clone().ok()?;
    Some((reads, writes))
}

/// Shuts the socket of a stream down in both directions, so that the reader thread, which holds the other descriptor
/// of the stream, reads its end. On an end that is no socket the call fails and changes nothing.
pub(crate) fn shut(end: &File) {
    // SAFETY: shutdown takes a descriptor number, which `end` keeps open through the call.
    unsafe { libc::shutdown(end.as_raw_fd(), libc::SHUT_RDWR) };
}

extern "C" {
    fn tzset();
}

static TIME_ZONE: Once = Once::new();

/// The local date (year, month 1 to 12, day 1 to 31) of a second since 1970-01-01, by the system's time zone.
pub(crate) fn local_date(secs: i64) -> Option<(i64, u32, u32)> {
    // SAFETY: tzset reads the environment and the time zone files and sets the C library's state, once.
    TIME_ZONE.call_once(|| unsafe { tzset() });
    let at = libc::time_t::try_from(secs).ok()?;
    // SAFETY: an all-zero tm is a valid value of the type, whose one pointer is then null.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: localtime_r reads `at` and writes `tm` alone, both alive through the call.
    if unsafe { libc::localtime_r(&at, &mut tm) }.is_null() {
        return None;
    }
    Some((
        i64::from(tm.tm_year) + 1900,
        u32::try_from(tm.tm_mon + 1).ok()?,
        u32::try_from(tm.tm_mday).ok()?,
    ))
}

#[cfg(test)]
mod tests {
    /// Rule 6: after the start call the limit of a core file reads 0, soft and hard.
    #[test]
    fn core_files_are_off_after_the_start_call() {
        super::crash_reports_off().unwrap();
        let mut limit = libc::rlimit {
            rlim_cur: 1,
            rlim_max: 1,
        };
        // SAFETY: getrlimit writes the one rlimit value that the pointer names, which lives through the call.
        assert_eq!(unsafe { libc::getrlimit(libc::RLIMIT_CORE, &mut limit) }, 0);
        assert_eq!((limit.rlim_cur, limit.rlim_max), (0, 0));
    }
}
