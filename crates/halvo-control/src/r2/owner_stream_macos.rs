//! The owner stream on macOS: an end is one descriptor of a `socketpair(AF_UNIX, SOCK_STREAM, 0)`, with close-on-exec
//! set in every process that holds it, and it travels as one SCM_RIGHTS control message on the `sendmsg` call that
//! writes the first byte of its frame. A sender keeps its own descriptor of an end until the receiver has answered:
//! closed while the message is in flight, it can leave the receiver a socket whose first write fails.

use std::io::{self, ErrorKind, Read, Write};
use std::mem::ManuallyDrop;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use super::owner_stream::{cannot, judge, OwnerStreamEnd, OwnerStreamReady};
use crate::client::Transport;
use crate::proto::ErrorReply;
use crate::wire::{write_frame, FrameError, REPLY_MAX_BYTES};

/// The accepted connection of the control server, borrowed for the length of the handler's call.
pub type Connection<'a> = &'a UnixStream;

/// The client's end of a stream: one descriptor.
pub(super) type End = OwnedFd;

impl From<OwnerStreamEnd> for OwnedFd {
    fn from(end: OwnerStreamEnd) -> OwnedFd {
        end.0
    }
}

/// The transport of a client call that receives an end: the control connection, with its descriptor.
pub trait CarriesEnds: Transport + AsFd {}

impl<T: Transport + AsFd> CarriesEnds for T {}

/// The owner's channel as the handler of `owner_stream` uses it. The caller holds its turn on the channel for the
/// length of the handler's call, so no other exchange is open on it.
pub trait OwnerChannel {
    /// Writes the frame `stream` on the channel with `end` as one SCM_RIGHTS control message on the call that writes
    /// the frame's first byte, as [`send_with_end`] does. The handler keeps its own descriptor of `end` open until
    /// [`OwnerChannel::answer`] has returned.
    fn send_stream(&mut self, frame: &[u8], end: BorrowedFd<'_>) -> io::Result<()>;

    /// The owner's next frame on the channel that is no report, waiting at most `within`: `None` at the end of the
    /// channel, and an error of kind `TimedOut` when no frame came.
    fn answer(&mut self, within: Duration) -> io::Result<Option<Vec<u8>>>;
}

/// How long the handler waits, after the reply that carried the client's end, for the end of the client's request
/// before it closes its own copy of that end.
pub const CLIENT_END_WAIT: Duration = Duration::from_secs(1);

/// The frame `stream` on macOS: the end comes beside it.
const STREAM_BESIDE: &[u8] = br#"{"stream":{"handles":null}}"#;

/// The bytes of the data of a control message that carries `n` descriptors.
const fn descriptor_bytes(n: u32) -> u32 {
    n * size_of::<libc::c_int>() as u32
}

/// One new stream, a socket pair with close-on-exec set on both descriptors: the owner's end, then the client's.
fn new_stream() -> io::Result<(OwnedFd, OwnedFd)> {
    let mut fds = [-1; 2];
    // SAFETY: socketpair writes two descriptors into the array of two that the pointer names.
    let made = unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM, 0, fds.as_mut_ptr()) };
    if made != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: socketpair returned 0, so both are new open descriptors that nothing else in this process owns.
    let ends = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
    set_cloexec(ends.0.as_fd())?;
    set_cloexec(ends.1.as_fd())?;
    Ok(ends)
}

fn set_cloexec(fd: BorrowedFd<'_>) -> io::Result<()> {
    // SAFETY: fcntl reads the descriptor flags of a descriptor that the borrow keeps open.
    let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFD) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fcntl sets the descriptor flags of a descriptor that the borrow keeps open.
    if unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Writes `payload` as one frame, its length as a big-endian u32 and then its bytes, on `socket`, with `end` as one
/// SCM_RIGHTS control message on the `sendmsg` call that writes the frame's first byte; the rest of the frame follows
/// by plain writes. The caller keeps its own descriptor of `end` until the receiver has answered.
pub fn send_with_end(
    socket: BorrowedFd<'_>,
    payload: &[u8],
    end: BorrowedFd<'_>,
) -> io::Result<()> {
    let len = u32::try_from(payload.len()).map_err(|_| io::Error::from(ErrorKind::InvalidInput))?;
    let mut frame = Vec::with_capacity(4 + payload.len());
    frame.extend_from_slice(&len.to_be_bytes());
    frame.extend_from_slice(payload);
    let one = descriptor_bytes(1);
    // SAFETY: the function computes a size from a length and reads no memory.
    let space = unsafe { libc::CMSG_SPACE(one) };
    let mut control = vec![0u64; (space as usize).div_ceil(8)];
    let mut iov = libc::iovec {
        iov_base: frame.as_mut_ptr().cast(),
        iov_len: frame.len(),
    };
    // SAFETY: an all-zero msghdr is a valid value of the type, whose pointers are then null.
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.as_mut_ptr().cast();
    msg.msg_controllen = space;
    // SAFETY: `control` holds `space` bytes, aligned for a header, the room of one header and one descriptor; the
    // functions of the system give the header and the place of its data inside those bytes.
    unsafe {
        let header = libc::CMSG_FIRSTHDR(&msg);
        (*header).cmsg_level = libc::SOL_SOCKET;
        (*header).cmsg_type = libc::SCM_RIGHTS;
        (*header).cmsg_len = libc::CMSG_LEN(one);
        libc::CMSG_DATA(header)
            .cast::<libc::c_int>()
            .write_unaligned(end.as_raw_fd());
    }
    let sent = loop {
        // SAFETY: sendmsg reads the frame and the control message, which both live through the call, and the borrows
        // keep both descriptors open.
        let sent = unsafe { libc::sendmsg(socket.as_raw_fd(), &msg, 0) };
        match usize::try_from(sent) {
            Ok(sent) => break sent,
            Err(_) => {
                let error = io::Error::last_os_error();
                if error.kind() != ErrorKind::Interrupted {
                    return Err(error);
                }
            }
        }
    };
    if sent == 0 {
        return Err(ErrorKind::WriteZero.into());
    }
    // SAFETY: the stream borrows the descriptor of `socket`, which the borrow keeps open, for this call alone, and
    // ManuallyDrop keeps it from closing the descriptor.
    let mut rest = ManuallyDrop::new(unsafe { UnixStream::from_raw_fd(socket.as_raw_fd()) });
    rest.write_all(&frame[sent..])
}

/// Writes one reply frame on the connection.
pub(super) fn write_reply(connection: Connection<'_>, reply: &[u8]) -> Result<(), FrameError> {
    let mut writer = connection;
    write_frame(&mut writer, reply, REPLY_MAX_BYTES)
}

/// Reads the connection until the client has ended its request, or for [`CLIENT_END_WAIT`], and drops what it reads.
fn wait_for_end(connection: Connection<'_>) {
    let until = Instant::now() + CLIENT_END_WAIT;
    let mut reader = connection;
    let mut sink = [0u8; 64];
    loop {
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() || connection.set_read_timeout(Some(left)).is_err() {
            return;
        }
        match reader.read(&mut sink) {
            Ok(0) => return,
            Ok(_) => {}
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(_) => return,
        }
    }
}

/// Steps (c) to (f) of the handler: it returns the refusal to write when it handed no end to the client, with both
/// ends closed.
pub(super) fn serve(
    connection: Connection<'_>,
    owner: &mut dyn OwnerChannel,
    answer_by: Instant,
) -> Result<(), ErrorReply> {
    let (owners, clients) =
        new_stream().map_err(|_| cannot("the stream to the owner cannot be made"))?;
    let answer = owner
        .send_stream(STREAM_BESIDE, owners.as_fd())
        .and_then(|()| owner.answer(answer_by.saturating_duration_since(Instant::now())));
    drop(owners);
    if let Err(refusal) = judge(answer) {
        drop(clients);
        return Err(refusal);
    }
    let ready = OwnerStreamReady { handles: None }.encode();
    // A descriptor sent on a connection that the client never reads goes with that connection.
    if send_with_end(connection.as_fd(), &ready, clients.as_fd()).is_ok() {
        wait_for_end(connection);
    }
    drop(clients);
    Ok(())
}

/// What the client's reads of one reply received beside its bytes.
#[derive(Debug, Default)]
pub(super) struct Received {
    /// Each descriptor that came, with close-on-exec set.
    pub(super) ends: Vec<OwnedFd>,
    /// The control messages of every kind that came.
    pub(super) controls: usize,
    /// Whether a descriptor came that the reader could not take: the system cut a control message for lack of room,
    /// or close-on-exec could not be set.
    pub(super) refused: bool,
}

/// The room for descriptors of each `recvmsg` call of the client: eight, so that more than one is seen.
const ROOM: u32 = descriptor_bytes(8);

/// Reads one reply frame and every descriptor that comes beside its bytes, inside `deadline`.
pub(super) fn read_reply<T: CarriesEnds>(
    transport: &mut T,
    deadline: Instant,
) -> Result<(Vec<u8>, Received), FrameError> {
    let mut received = Received::default();
    let mut prefix = [0u8; 4];
    receive_exactly(transport, deadline, &mut prefix, &mut received)?;
    let len = u32::from_be_bytes(prefix);
    if len == 0 {
        return Err(FrameError::Empty);
    }
    if u64::from(len) > REPLY_MAX_BYTES as u64 {
        return Err(FrameError::TooLong {
            len: u64::from(len),
            max: REPLY_MAX_BYTES,
        });
    }
    let mut body = vec![0u8; len as usize];
    receive_exactly(transport, deadline, &mut body, &mut received)?;
    Ok((body, received))
}

fn receive_exactly<T: CarriesEnds>(
    transport: &mut T,
    deadline: Instant,
    buf: &mut [u8],
    received: &mut Received,
) -> Result<(), FrameError> {
    let mut filled = 0;
    while filled < buf.len() {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(FrameError::Io(ErrorKind::TimedOut.into()));
        }
        transport.set_timeout(left).map_err(FrameError::Io)?;
        match receive(transport.as_fd(), &mut buf[filled..], received) {
            Ok(0) => return Err(FrameError::Truncated),
            Ok(n) => filled += n,
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => return Err(FrameError::Io(e)),
        }
    }
    Ok(())
}

/// One `recvmsg` call into `buf`, with room for eight descriptors beside it. Each descriptor that comes gets
/// close-on-exec before the call returns.
fn receive(socket: BorrowedFd<'_>, buf: &mut [u8], received: &mut Received) -> io::Result<usize> {
    // SAFETY: the function computes a size from a length and reads no memory.
    let space = unsafe { libc::CMSG_SPACE(ROOM) };
    let mut control = vec![0u64; (space as usize).div_ceil(8)];
    let mut iov = libc::iovec {
        iov_base: buf.as_mut_ptr().cast(),
        iov_len: buf.len(),
    };
    // SAFETY: an all-zero msghdr is a valid value of the type, whose pointers are then null.
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.as_mut_ptr().cast();
    msg.msg_controllen = space;
    // SAFETY: recvmsg writes at most the lengths that the message names into `buf` and `control`, which live through
    // the call, and the borrow keeps the descriptor open.
    let got = unsafe { libc::recvmsg(socket.as_raw_fd(), &mut msg, 0) };
    let got = usize::try_from(got).map_err(|_| io::Error::last_os_error())?;
    if msg.msg_flags & libc::MSG_CTRUNC != 0 {
        received.refused = true;
    }
    // SAFETY: after recvmsg, `msg` names the bytes of `control` that hold whole control messages; the functions of
    // the system walk those headers and give the data of each inside them. Each descriptor of an SCM_RIGHTS message is
    // new in this process, and nothing else owns it.
    unsafe {
        let mut header = libc::CMSG_FIRSTHDR(&msg);
        while !header.is_null() {
            received.controls += 1;
            if (*header).cmsg_level == libc::SOL_SOCKET && (*header).cmsg_type == libc::SCM_RIGHTS {
                let data = libc::CMSG_DATA(header).cast::<libc::c_int>();
                let count = ((*header).cmsg_len - libc::CMSG_LEN(0)) / descriptor_bytes(1);
                for at in 0..count as usize {
                    let fd = OwnedFd::from_raw_fd(data.add(at).read_unaligned());
                    match set_cloexec(fd.as_fd()) {
                        Ok(()) => received.ends.push(fd),
                        Err(_) => received.refused = true,
                    }
                }
            }
            header = libc::CMSG_NXTHDR(&msg, header);
        }
    }
    Ok(got)
}

/// The client's end, where the reply names no handle and exactly one descriptor came beside it.
pub(super) fn take_end<T>(_: &T, ready: OwnerStreamReady, received: Received) -> Option<End> {
    let Received {
        mut ends, refused, ..
    } = received;
    if ready.handles.is_some() || refused || ends.len() != 1 {
        return None;
    }
    ends.pop()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::{decode_reply, decode_request, Decoded, ErrorCode, Reply};
    use crate::r2::OwnerStream;
    use crate::wire::{read_frame, REQUEST_MAX_BYTES};

    /// An owner's channel that answers every frame `stream` with one scripted frame, or with the end of the channel.
    struct Scripted(Option<&'static [u8]>);

    impl OwnerChannel for Scripted {
        fn send_stream(&mut self, frame: &[u8], _: BorrowedFd<'_>) -> io::Result<()> {
            assert_eq!(frame, STREAM_BESIDE);
            Ok(())
        }

        fn answer(&mut self, _: Duration) -> io::Result<Option<Vec<u8>>> {
            Ok(self.0.map(<[u8]>::to_vec))
        }
    }

    /// Ruling 5 of the daemon half: `full` answers busy, `refused` internal, the end of the owner's channel
    /// store_unavailable and a body with a key invalid_argument, each an error reply of v1, and the client's recvmsg
    /// gets no control message beside it.
    #[test]
    fn each_refusal_hands_no_end() {
        let request = OwnerStream.encode();
        let keyed = br#"{"v":2,"body":{"op":"owner_stream","x":1}}"#.to_vec();
        let full: &'static [u8] = br#"{"stream":"full"}"#;
        let refused: &'static [u8] = br#"{"stream":"refused"}"#;
        let taken: &'static [u8] = br#"{"stream":"taken"}"#;
        for (request, answer, code) in [
            (request.clone(), Some(full), ErrorCode::Busy),
            (request.clone(), Some(refused), ErrorCode::Internal),
            (request, None, ErrorCode::StoreUnavailable),
            (keyed, Some(taken), ErrorCode::InvalidArgument),
        ] {
            let (mut client, server) = UnixStream::pair().unwrap();
            let served = std::thread::spawn(move || {
                let frame = read_frame(&mut &server, REQUEST_MAX_BYTES).unwrap();
                let Ok(Decoded::Unknown(request)) = decode_request(&frame) else {
                    panic!("the request is no body of version 2");
                };
                crate::r2::dispatch_unknown(&request, &server, &mut Scripted(answer))
            });
            write_frame(&mut client, &request, REQUEST_MAX_BYTES).unwrap();
            let deadline = Instant::now() + Duration::from_secs(30);
            let (frame, received) = read_reply(&mut client, deadline).unwrap();
            assert!(
                served.join().unwrap(),
                "{code:?}: the handler wrote no reply"
            );
            assert_eq!(received.controls, 0, "{code:?}: a control message came");
            assert!(received.ends.is_empty(), "{code:?}");
            match decode_reply(&frame).unwrap() {
                Reply::Error(error) => assert_eq!(error.code(), code),
                other => panic!("{code:?}: the reply is {other:?}"),
            }
        }
    }
}
