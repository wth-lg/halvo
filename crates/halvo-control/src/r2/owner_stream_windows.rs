//! The owner stream on Windows: a stream is two anonymous pipes from `CreatePipe`, one for each direction, so an end is
//! two handles, the one that reads and the one that writes. The handler gives each handle to its process with
//! `DuplicateHandle`, not inheritable and with `DUPLICATE_CLOSE_SOURCE`, and names the two values in a frame: to the
//! owner in the frame `stream`, to the client in `handles` of the reply. The client's process is opened once, from the
//! process id of the client of the pipe, and the handler gives its end only into a process whose token user, read on
//! that handle, is the daemon's own. Where the client's hand-over fails, the handler takes back what it gave the
//! client. The handler writes on the pipe with overlapped writes of its own, since the server opens its instance for
//! overlapped I/O, and waits at most 2 s for a reply.

use std::ffi::c_void;
use std::io::{self, Read, Write};
use std::os::windows::io::{
    AsHandle, AsRawHandle, BorrowedHandle, FromRawHandle, IntoRawHandle, OwnedHandle,
};
use std::time::{Duration, Instant};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{
    DuplicateHandle, DUPLICATE_CLOSE_SOURCE, DUPLICATE_SAME_ACCESS, ERROR_IO_PENDING,
    ERROR_OPERATION_ABORTED, HANDLE, WAIT_OBJECT_0,
};
use windows::Win32::Storage::FileSystem::{GetFileType, WriteFile, FILE_TYPE_PIPE};
use windows::Win32::System::Pipes::{
    CreatePipe, GetNamedPipeClientProcessId, GetNamedPipeServerProcessId,
};
use windows::Win32::System::Threading::{
    CreateEventW, GetCurrentProcess, OpenProcess, WaitForSingleObject, PROCESS_DUP_HANDLE,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};

use super::owner_stream::{cannot, judge, stream_frame, OwnerStreamEnd, OwnerStreamReady};
use crate::client::Transport;
use crate::client_windows::{current_user, os_error, user_of_token_of};
use crate::proto::ErrorReply;
use crate::wire::{read_frame, write_frame, FrameError, REPLY_MAX_BYTES};

/// The longest wait of the handler for the write of one reply: the server's deadline of a reply's write.
const REPLY_WRITE_WAIT: Duration = Duration::from_secs(2);

/// The reading of the SID string of the token user of a process from a handle of that process.
type UserOf = fn(HANDLE) -> io::Result<String>;

/// The accepted connection of the control server, a pipe, borrowed for the length of the handler's call.
pub type Connection<'a> = BorrowedHandle<'a>;

/// The client's end of a stream: the handle that reads, then the one that writes.
pub(super) type End = (OwnedHandle, OwnedHandle);

impl From<OwnerStreamEnd> for (OwnedHandle, OwnedHandle) {
    fn from(end: OwnerStreamEnd) -> (OwnedHandle, OwnedHandle) {
        end.0
    }
}

/// The transport of a client call that receives an end: the control connection, a pipe, with its handle.
pub trait CarriesEnds: Transport + AsHandle {}

impl<T: Transport + AsHandle> CarriesEnds for T {}

/// The owner's channel as the handlers of `owner_stream` and `relay` use it. The caller holds its turn on the channel
/// for the length of the handler's call, so no other exchange is open on it.
pub trait OwnerChannel {
    /// The owner's process, held from the start of the owner: the handler duplicates the owner's two handles into it.
    fn owner_process(&self) -> BorrowedHandle<'_>;

    /// Writes the frame `stream` on the channel; on Windows the frame names the owner's two handles by their values.
    fn send_stream(&mut self, frame: &[u8]) -> io::Result<()>;

    /// Writes `frame` on the channel as one frame with nothing beside it: the request that the handler of `relay`
    /// hands to the owner. An error of kind `BrokenPipe` is the end of the channel.
    fn send(&mut self, frame: &[u8]) -> io::Result<()>;

    /// The owner's next frame on the channel that is no report, waiting at most `within`: `None` at the end of the
    /// channel, and an error of kind `TimedOut` when no frame came.
    fn answer(&mut self, within: Duration) -> io::Result<Option<Vec<u8>>>;
}

fn raw(handle: BorrowedHandle<'_>) -> HANDLE {
    HANDLE(handle.as_raw_handle())
}

/// One anonymous pipe, not inheritable: the handle that reads, then the one that writes.
fn pipe() -> io::Result<(OwnedHandle, OwnedHandle)> {
    let (mut reads, mut writes) = (HANDLE::default(), HANDLE::default());
    // SAFETY: CreatePipe writes two new handles into the two places that the pointers name; with no attributes
    // neither handle is inheritable.
    unsafe { CreatePipe(&mut reads, &mut writes, None, 0) }.map_err(io::Error::other)?;
    // SAFETY: both handles are new and open, and nothing else in this process owns them.
    Ok(unsafe {
        (
            OwnedHandle::from_raw_handle(reads.0),
            OwnedHandle::from_raw_handle(writes.0),
        )
    })
}

/// One new stream: the owner's end and the client's end, each the handle that reads first.
fn new_stream() -> io::Result<([OwnedHandle; 2], [OwnedHandle; 2])> {
    let (owner_reads, client_writes) = pipe()?;
    let (client_reads, owner_writes) = pipe()?;
    Ok(([owner_reads, owner_writes], [client_reads, client_writes]))
}

/// Gives `end` to `process`, with the same access and not inheritable, and closes this process's handle in the same
/// call. Returns the value of the handle in `process`.
fn give_one(process: BorrowedHandle<'_>, end: OwnedHandle) -> io::Result<u64> {
    let source = HANDLE(end.into_raw_handle());
    let mut value = HANDLE::default();
    // SAFETY: `source` is a handle of this process that this call owns; DUPLICATE_CLOSE_SOURCE closes it whatever the
    // outcome, so nothing closes it again. The borrow keeps `process` open, and the call writes the one handle that
    // the pointer names.
    unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            source,
            raw(process),
            &mut value,
            0,
            false,
            DUPLICATE_SAME_ACCESS | DUPLICATE_CLOSE_SOURCE,
        )
    }
    .map_err(io::Error::other)?;
    Ok(value.0.addr() as u64)
}

/// The handle that a value of a frame names.
fn named(value: u64) -> Option<HANDLE> {
    let value = usize::try_from(value).ok()?;
    Some(HANDLE(std::ptr::without_provenance_mut::<c_void>(value)))
}

/// Closes the handle of `value` in `process`.
fn take_back(process: BorrowedHandle<'_>, value: u64) {
    let Some(value) = named(value) else {
        return;
    };
    // SAFETY: with DUPLICATE_CLOSE_SOURCE, no target process and no target pointer, the call closes the handle of
    // `value` in `process` and writes nothing; the borrow keeps `process` open.
    let _ = unsafe {
        DuplicateHandle(
            raw(process),
            value,
            HANDLE::default(),
            std::ptr::null_mut(),
            0,
            false,
            DUPLICATE_CLOSE_SOURCE,
        )
    };
}

/// Gives both handles of an end to `process`. Where the second fails, it takes the first back, and the handles that
/// were not given close here.
fn give(process: BorrowedHandle<'_>, end: [OwnedHandle; 2]) -> io::Result<[u64; 2]> {
    let [reads, writes] = end;
    let first = give_one(process, reads)?;
    match give_one(process, writes) {
        Ok(second) => Ok([first, second]),
        Err(error) => {
            take_back(process, first);
            Err(error)
        }
    }
}

/// The client's process, opened once from the process id of the client of the accepted connection, with the rights to
/// duplicate handles into it and to read its token: an error, and the handle closed, unless the token user that
/// `user_of` reads on that handle is the user of this process, the daemon's own. The process id is read once, so the
/// process that is checked is the one that receives the end.
fn client_process(connection: Connection<'_>, user_of: UserOf) -> io::Result<OwnedHandle> {
    let mut pid = 0u32;
    // SAFETY: the borrow keeps the connection open for the call, which writes the one u32 that the pointer names.
    unsafe { GetNamedPipeClientProcessId(raw(connection), &mut pid) }.map_err(os_error)?;
    // SAFETY: OpenProcess returns a new handle, which nothing else owns.
    let process = unsafe {
        OpenProcess(
            PROCESS_DUP_HANDLE | PROCESS_QUERY_LIMITED_INFORMATION,
            false,
            pid,
        )
    }
    .map_err(os_error)?;
    // SAFETY: the handle is new and open, and nothing else in this process owns it.
    let process = unsafe { OwnedHandle::from_raw_handle(process.0) };
    if user_of(raw(process.as_handle()))? != current_user()? {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    Ok(process)
}

/// A new event of manual reset, not signalled.
fn new_event() -> io::Result<OwnedHandle> {
    // SAFETY: CreateEventW is given no attributes and no name; the handle that it returns is owned at once below.
    let event = unsafe { CreateEventW(None, true, false, PCWSTR::null()) }.map_err(os_error)?;
    // SAFETY: `event` is a new handle that nothing else owns or closes.
    Ok(unsafe { OwnedHandle::from_raw_handle(event.0) })
}

/// The writer of one reply on the borrowed connection: each write is a write with an `OVERLAPPED` and an event of its
/// own, which waits at most until `deadline` and is cancelled then. On a handle opened for overlapped I/O the write may
/// pend; on any other it ends inside the call.
struct OverlappedWriter<'a> {
    connection: Connection<'a>,
    event: OwnedHandle,
    deadline: Instant,
}

impl Write for OverlappedWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let len = buf.len().min(u32::MAX as usize);
        if len == 0 {
            return Ok(0);
        }
        let left = self.deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        let pipe = raw(self.connection);
        let event = HANDLE(self.event.as_raw_handle());
        let mut overlapped = OVERLAPPED {
            hEvent: event,
            ..Default::default()
        };
        // SAFETY: the borrow keeps the connection open, and `buf` and `overlapped` outlive the write, which
        // GetOverlappedResult below waits for.
        match unsafe { WriteFile(pipe, Some(&buf[..len]), None, Some(&mut overlapped)) } {
            Ok(()) => {}
            Err(error) if error.code() == ERROR_IO_PENDING.to_hresult() => {
                let millis = u32::try_from(left.as_millis().max(1)).unwrap_or(u32::MAX - 1);
                // SAFETY: the event is open; the call takes no pointer.
                if unsafe { WaitForSingleObject(event, millis) } != WAIT_OBJECT_0 {
                    // SAFETY: the connection is open and `overlapped` names the one write started above.
                    let _ = unsafe { CancelIoEx(pipe, Some(&overlapped)) };
                }
            }
            Err(error) => return Err(os_error(error)),
        }
        let mut done = 0u32;
        // SAFETY: the call waits until the write of `overlapped` has ended, which a cancel ends at once; `overlapped`,
        // `done` and `buf` live until it returns.
        match unsafe { GetOverlappedResult(pipe, &overlapped, &mut done, true) } {
            Ok(()) => Ok(done as usize),
            Err(error) if error.code() == ERROR_OPERATION_ABORTED.to_hresult() => {
                Err(io::ErrorKind::TimedOut.into())
            }
            Err(error) => Err(os_error(error)),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Writes one reply frame on the connection, with overlapped writes of its own that wait at most 2 s in all.
pub(super) fn write_reply(connection: Connection<'_>, reply: &[u8]) -> Result<(), FrameError> {
    let mut writer = OverlappedWriter {
        connection,
        event: new_event().map_err(FrameError::Io)?,
        deadline: Instant::now() + REPLY_WRITE_WAIT,
    };
    write_frame(&mut writer, reply, REPLY_MAX_BYTES)
}

/// Steps (c) to (f) of the handler: it returns the refusal to write when it handed no end to the client, with both
/// ends closed.
pub(super) fn serve(
    connection: Connection<'_>,
    owner: &mut dyn OwnerChannel,
    answer_by: Instant,
) -> Result<(), ErrorReply> {
    serve_reading(connection, owner, answer_by, user_of_token_of)
}

/// [`serve`] with `user_of` as the reading of the token user of the client's process.
fn serve_reading(
    connection: Connection<'_>,
    owner: &mut dyn OwnerChannel,
    answer_by: Instant,
    user_of: UserOf,
) -> Result<(), ErrorReply> {
    let (owners, clients) =
        new_stream().map_err(|_| cannot("the stream to the owner cannot be made"))?;
    let answer = give(owner.owner_process(), owners).and_then(|[reads, writes]| {
        owner.send_stream(&stream_frame(Some([reads, writes])))?;
        owner.answer(answer_by.saturating_duration_since(Instant::now()))
    });
    if let Err(refusal) = judge(answer) {
        drop(clients);
        return Err(refusal);
    }
    let handed = cannot("the end of the stream cannot be handed to the client");
    let process = client_process(connection, user_of).map_err(|_| handed.clone())?;
    let values = give(process.as_handle(), clients).map_err(|_| handed)?;
    let ready = OwnerStreamReady {
        handles: Some(values),
    }
    .encode();
    if write_reply(connection, &ready).is_err() {
        for value in values {
            take_back(process.as_handle(), value);
        }
    }
    drop(process);
    Ok(())
}

/// What the client's read of one reply receives beside its bytes: nothing on Windows, where the reply names the
/// handles.
pub(super) struct Received;

/// A transport whose every read gets the time that is left of the deadline.
struct Timed<'a, T> {
    transport: &'a mut T,
    deadline: Instant,
}

impl<T: Transport> Read for Timed<'_, T> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let left = self.deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        self.transport.set_timeout(left)?;
        self.transport.read(buf)
    }
}

/// Reads one reply frame inside `deadline`.
pub(super) fn read_reply<T: CarriesEnds>(
    transport: &mut T,
    deadline: Instant,
) -> Result<(Vec<u8>, Received), FrameError> {
    let mut timed = Timed {
        transport,
        deadline,
    };
    Ok((read_frame(&mut timed, REPLY_MAX_BYTES)?, Received))
}

/// The process id of the server of the pipe `handle`.
fn server_of(handle: HANDLE) -> Option<u32> {
    let mut pid = 0u32;
    // SAFETY: the call reads the handle's server process id and writes the one u32 that the pointer names; a value
    // that names no pipe makes it fail.
    unsafe { GetNamedPipeServerProcessId(handle, &mut pid) }.ok()?;
    Some(pid)
}

/// The client's end, where the reply names two values that pass the three checks of contracts.md C24.13 item 6 of the
/// workflows run: each names a pipe, the two differ from each other and from the control connection, and the server of
/// each is the server of the control connection, which made both pipes. Where one fails, it touches neither value.
pub(super) fn take_end<T: CarriesEnds>(
    transport: &T,
    ready: OwnerStreamReady,
    _: Received,
) -> Option<End> {
    let [reads, writes] = ready.handles?;
    let (reads, writes) = (named(reads)?, named(writes)?);
    let control = raw(transport.as_handle());
    if reads == writes || reads == control || writes == control {
        return None;
    }
    for handle in [reads, writes] {
        // SAFETY: GetFileType reads the type of the object that a handle value names; a value that names none makes
        // it answer an unknown type.
        if unsafe { GetFileType(handle) } != FILE_TYPE_PIPE {
            return None;
        }
    }
    let server = server_of(control)?;
    if server_of(reads)? != server || server_of(writes)? != server {
        return None;
    }
    // SAFETY: the daemon gave these two handles to this process for this call, and the checks show two pipes that
    // the server of the control connection made, neither of them the control connection: nothing else here owns them.
    Some(unsafe {
        (
            OwnedHandle::from_raw_handle(reads.0),
            OwnedHandle::from_raw_handle(writes.0),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::{decode_reply, encode_reply, ErrorCode, Reply};
    use std::sync::atomic::{AtomicBool, Ordering};
    use windows::Win32::Storage::FileSystem::{FILE_FLAG_OVERLAPPED, PIPE_ACCESS_DUPLEX};
    use windows::Win32::System::Pipes::{
        CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
    };

    const ANOTHER_USER: &str = "S-1-5-21-1000000001-1000000002-1000000003-1001";

    /// Whether the reading of another user ran.
    static READ: AtomicBool = AtomicBool::new(false);

    /// A reading of the token user that answers another user's SID for every process.
    fn another_user(_: HANDLE) -> io::Result<String> {
        READ.store(true, Ordering::SeqCst);
        Ok(ANOTHER_USER.to_owned())
    }

    /// An owner's channel in this process that takes every end and closes the two handles that the frame names.
    struct Taking;

    impl OwnerChannel for Taking {
        fn owner_process(&self) -> BorrowedHandle<'_> {
            // SAFETY: the pseudo handle of this process is open for the life of the process and needs no close.
            unsafe { BorrowedHandle::borrow_raw(GetCurrentProcess().0) }
        }

        fn send_stream(&mut self, frame: &[u8]) -> io::Result<()> {
            let frame: serde_json::Value = serde_json::from_slice(frame).unwrap();
            for value in frame["stream"]["handles"].as_array().unwrap() {
                let handle = named(value.as_u64().unwrap()).unwrap();
                // SAFETY: the handler gave this handle to this process, the owner's, and nothing else owns it.
                drop(unsafe { OwnedHandle::from_raw_handle(handle.0) });
            }
            Ok(())
        }

        fn send(&mut self, _: &[u8]) -> io::Result<()> {
            unreachable!("the handler of owner_stream writes no frame without an end")
        }

        fn answer(&mut self, _: Duration) -> io::Result<Option<Vec<u8>>> {
            Ok(Some(br#"{"stream":"taken"}"#.to_vec()))
        }
    }

    /// The server's end of a new pipe, opened for overlapped I/O as the control server opens its instances.
    fn server_end(name: &str) -> OwnedHandle {
        let name: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: `name` ends in a NUL and outlives the call; no attributes are passed.
        let pipe = unsafe {
            CreateNamedPipeW(
                PCWSTR(name.as_ptr()),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                4_096,
                4_096,
                0,
                None,
            )
        };
        assert!(!pipe.is_invalid(), "{}", io::Error::last_os_error());
        // SAFETY: `pipe` is a new handle that nothing else owns or closes.
        unsafe { OwnedHandle::from_raw_handle(pipe.0) }
    }

    /// Rule 5: with a reading of the client's token user that answers another user's SID, the handler hands the
    /// client no end after the owner took its own: it returns `internal`, the one reply on the pipe is that error
    /// reply of v1, and no reply names a handle.
    #[test]
    fn an_end_goes_to_no_process_of_another_user() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let name = format!(
            r"\\.\pipe\halvo-control-test-{}-{nanos}",
            std::process::id()
        );
        let server = server_end(&name);
        let mut client = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&name)
            .unwrap();
        let answer_by = Instant::now() + Duration::from_secs(4);
        let refusal = serve_reading(server.as_handle(), &mut Taking, answer_by, another_user)
            .expect_err("the handler handed an end into a process of another user");
        assert!(READ.load(Ordering::SeqCst), "the token user was not read");
        assert_eq!(refusal.code(), ErrorCode::Internal);
        write_reply(server.as_handle(), &encode_reply(&Reply::Error(refusal))).unwrap();
        let frame = read_frame(&mut client, REPLY_MAX_BYTES).unwrap();
        drop(server);
        let mut after = [0u8; 1];
        let read = client.read(&mut after);
        assert!(
            !matches!(read, Ok(n) if n > 0),
            "a byte came after the reply: {read:?}"
        );
        assert!(
            OwnerStreamReady::decode(&frame).is_err(),
            "a reply names handles"
        );
        match decode_reply(&frame).unwrap() {
            Reply::Error(error) => assert_eq!(error.code(), ErrorCode::Internal),
            other => panic!("the reply is {other:?}"),
        }
    }
}
