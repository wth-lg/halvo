//! The owner stream on Windows: a stream is two anonymous pipes from `CreatePipe`, one for each direction, so an end is
//! two handles, the one that reads and the one that writes. The handler gives each handle to its process with
//! `DuplicateHandle`, not inheritable and with `DUPLICATE_CLOSE_SOURCE`, and names the two values in a frame: to the
//! owner in the frame `stream`, to the client in `handles` of the reply. Where the client's hand-over fails, the
//! handler takes back what it gave the client.

use std::ffi::c_void;
use std::fs::File;
use std::io::{self, Read};
use std::mem::ManuallyDrop;
use std::os::windows::io::{
    AsHandle, AsRawHandle, BorrowedHandle, FromRawHandle, IntoRawHandle, OwnedHandle,
};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{
    DuplicateHandle, DUPLICATE_CLOSE_SOURCE, DUPLICATE_SAME_ACCESS, HANDLE,
};
use windows::Win32::Storage::FileSystem::{GetFileType, FILE_TYPE_PIPE};
use windows::Win32::System::Pipes::{
    CreatePipe, GetNamedPipeClientProcessId, GetNamedPipeServerProcessId,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcess, PROCESS_DUP_HANDLE};

use super::owner_stream::{cannot, judge, OwnerStreamEnd, OwnerStreamReady};
use crate::client::Transport;
use crate::proto::ErrorReply;
use crate::wire::{read_frame, write_frame, FrameError, REPLY_MAX_BYTES};

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

/// The client's process, opened with the right to duplicate handles alone, from the process id of the client of the
/// accepted connection.
fn client_process(connection: Connection<'_>) -> io::Result<OwnedHandle> {
    let mut pid = 0u32;
    // SAFETY: the borrow keeps the connection open for the call, which writes the one u32 that the pointer names.
    unsafe { GetNamedPipeClientProcessId(raw(connection), &mut pid) }.map_err(io::Error::other)?;
    // SAFETY: OpenProcess returns a new handle, which nothing else owns.
    let process =
        unsafe { OpenProcess(PROCESS_DUP_HANDLE, false, pid) }.map_err(io::Error::other)?;
    // SAFETY: the handle is new and open, and nothing else in this process owns it.
    Ok(unsafe { OwnedHandle::from_raw_handle(process.0) })
}

/// Writes one reply frame on the connection.
pub(super) fn write_reply(connection: Connection<'_>, reply: &[u8]) -> Result<(), FrameError> {
    // SAFETY: the file borrows the handle of the connection, which the borrow keeps open, for this call alone, and
    // ManuallyDrop keeps it from closing the handle.
    let mut file = ManuallyDrop::new(unsafe { File::from_raw_handle(connection.as_raw_handle()) });
    write_frame(&mut *file, reply, REPLY_MAX_BYTES)
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
    let answer = give(owner.owner_process(), owners).and_then(|[reads, writes]| {
        let frame = format!(r#"{{"stream":{{"handles":[{reads},{writes}]}}}}"#);
        owner.send_stream(frame.as_bytes())?;
        owner.answer(answer_by.saturating_duration_since(Instant::now()))
    });
    if let Err(refusal) = judge(answer) {
        drop(clients);
        return Err(refusal);
    }
    let handed = cannot("the end of the stream cannot be handed to the client");
    let process = client_process(connection).map_err(|_| handed.clone())?;
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
