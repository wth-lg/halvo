//! The control channel of a service run on Windows: the named pipe of the
//! data directory for the daemon's own user (contracts.md C15.1, C15.2,
//! C15.4), in the form of the Unix socket's listener.

use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use halvo_control::client::Transport;
use halvo_control::client_windows::{current_user, user_of_process, PipeStream};
use halvo_control::paths::{pipe_name, DataDir};
use halvo_control::wire::{REPLY_MAX_BYTES, REQUEST_MAX_BYTES};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{
    LocalFree, ERROR_ACCESS_DENIED, ERROR_IO_PENDING, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED,
    HANDLE, HLOCAL, WAIT_OBJECT_0, WIN32_ERROR,
};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows::Win32::Storage::FileSystem::{
    FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, PIPE_ACCESS_DUPLEX,
};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeClientProcessId,
    PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};

use super::{Admitting, Asked, RequestEnd};

const ACCEPT_POLL: Duration = Duration::from_millis(10);

/// The instances of the pipe: one that accepts and four that serve
/// (contracts.md C15.2).
pub const MAX_INSTANCES: u32 = 5;

const OUT_QUOTA: u32 = REPLY_MAX_BYTES as u32 + 4;

const IN_QUOTA: u32 = REQUEST_MAX_BYTES as u32 + 4;

type UserOf = fn(u32) -> io::Result<String>;

/// Whether the server serves a client whose token user has the SID string
/// `client`, in a daemon whose own user has the SID string `own`: only when
/// the two are one (contracts.md C15.2).
pub fn serves_client(client: &str, own: &str) -> bool {
    client == own
}

/// A listening control pipe: the thread that accepts on it and the requests
/// that its connections hand to the service run's loop. Its drop stops the
/// accepting thread, which closes the instance that it holds.
pub struct Listening {
    path: PathBuf,
    asked: Receiver<Asked>,
    admitting: Arc<Admitting>,
    closed: Arc<AtomicBool>,
    accepter: Option<JoinHandle<()>>,
}

impl Listening {
    /// Creates the first instance of the pipe of `dir` for this process's
    /// user and starts accepting on it. When a pipe of that name stands, as
    /// one of another listener on the same data directory does, it fails with
    /// an error of the kind `AddrInUse` that names the pipe. Each connection
    /// is served only when the token user of the client's process is the
    /// daemon's own user; any other client is dropped with no read of its
    /// request and no reply.
    pub fn bind(dir: &DataDir) -> io::Result<Listening> {
        Self::bind_reading(dir, user_of_process)
    }

    fn bind_reading(dir: &DataDir, user_of: UserOf) -> io::Result<Listening> {
        let own = current_user()?;
        let name = pipe_name(&own, dir);
        let first =
            create_instance(&name, &own, true).map_err(|error| match error.raw_os_error() {
                Some(code)
                    if code == ERROR_ACCESS_DENIED.0 as i32 || code == ERROR_PIPE_BUSY.0 as i32 =>
                {
                    io::Error::new(
                        io::ErrorKind::AddrInUse,
                        format!("the control pipe {name} stands already"),
                    )
                }
                _ => error,
            })?;
        let admitting = Arc::new(Admitting::default());
        let closed = Arc::new(AtomicBool::new(false));
        let (hand, asked) = mpsc::channel();
        let accepter = std::thread::Builder::new()
            .name("halvo-control-server".to_owned())
            .spawn({
                let name = name.clone();
                let admitting = admitting.clone();
                let closed = closed.clone();
                move || accept(first, &name, &own, user_of, &admitting, &closed, &hand)
            })?;
        Ok(Listening {
            path: PathBuf::from(name),
            asked,
            admitting,
            closed,
            accepter: Some(accepter),
        })
    }

    /// The next request that a connection handed over, if one waits.
    pub fn next(&self) -> Option<Asked> {
        self.asked.try_recv().ok()
    }

    /// What the connections share with the loop.
    pub fn admitting(&self) -> &Admitting {
        &self.admitting
    }

    /// The name of the pipe.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Listening {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::SeqCst);
        if let Some(accepter) = self.accepter.take() {
            let _ = accepter.join();
        }
    }
}

fn os_error(error: windows::core::Error) -> io::Error {
    match WIN32_ERROR::from_error(&error) {
        Some(code) => io::Error::from_raw_os_error(code.0 as i32),
        None => io::Error::other(error),
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn create_instance(name: &str, own: &str, first: bool) -> io::Result<OwnedHandle> {
    let name = wide(name);
    let sddl = wide(&format!("D:P(A;;GA;;;{own})"));
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: `sddl` ends in a NUL and outlives the call, and `descriptor` is
    // a live local that receives memory that LocalFree releases below.
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl.as_ptr()),
            SDDL_REVISION_1,
            &mut descriptor,
            None,
        )
    }
    .map_err(os_error)?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: false.into(),
    };
    let mut open = PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED;
    if first {
        open |= FILE_FLAG_FIRST_PIPE_INSTANCE;
    }
    // SAFETY: `name` ends in a NUL, and `attributes` and the descriptor that
    // it points to live across the call.
    let pipe = unsafe {
        CreateNamedPipeW(
            PCWSTR(name.as_ptr()),
            open,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            MAX_INSTANCES,
            OUT_QUOTA,
            IN_QUOTA,
            0,
            Some(&attributes),
        )
    };
    let failed = io::Error::last_os_error();
    // SAFETY: the descriptor came from the conversion above, whose memory
    // LocalFree releases, once.
    let _ = unsafe { LocalFree(Some(HLOCAL(descriptor.0))) };
    if pipe.is_invalid() {
        return Err(failed);
    }
    // SAFETY: `pipe` is a new handle that nothing else owns or closes.
    Ok(unsafe { OwnedHandle::from_raw_handle(pipe.0) })
}

fn new_event() -> io::Result<OwnedHandle> {
    // SAFETY: CreateEventW is given no attributes and no name; the handle that
    // it returns is owned at once below.
    let event = unsafe { CreateEventW(None, true, false, PCWSTR::null()) }.map_err(os_error)?;
    // SAFETY: `event` is a new handle that nothing else owns or closes.
    Ok(unsafe { OwnedHandle::from_raw_handle(event.0) })
}

enum Connect {
    Connected,
    Failed,
    Closed,
}

fn connect(instance: &OwnedHandle, event: &OwnedHandle, closed: &AtomicBool) -> Connect {
    let pipe = HANDLE(instance.as_raw_handle());
    let signal = HANDLE(event.as_raw_handle());
    let mut overlapped = OVERLAPPED {
        hEvent: signal,
        ..Default::default()
    };
    // SAFETY: the instance and the event are open, and `overlapped` lives
    // until the operation has ended, which the wait below makes sure of.
    match unsafe { ConnectNamedPipe(pipe, Some(&mut overlapped)) } {
        Ok(()) => return Connect::Connected,
        Err(error) if error.code() == ERROR_PIPE_CONNECTED.to_hresult() => {
            return Connect::Connected
        }
        Err(error) if error.code() == ERROR_IO_PENDING.to_hresult() => {}
        Err(_) => return Connect::Failed,
    }
    loop {
        // SAFETY: the event is open; the call takes no pointer.
        if unsafe { WaitForSingleObject(signal, ACCEPT_POLL.as_millis() as u32) } == WAIT_OBJECT_0 {
            break;
        }
        if closed.load(Ordering::SeqCst) {
            // SAFETY: the instance is open and `overlapped` names its one
            // pending connect.
            let _ = unsafe { CancelIoEx(pipe, Some(&overlapped)) };
            break;
        }
    }
    let mut done = 0u32;
    // SAFETY: the call waits until the connect of `overlapped` has ended,
    // which a cancel ends at once; `overlapped` and `done` live until then.
    let connected = unsafe { GetOverlappedResult(pipe, &overlapped, &mut done, true) };
    match connected {
        _ if closed.load(Ordering::SeqCst) => Connect::Closed,
        Ok(()) => Connect::Connected,
        Err(_) => Connect::Failed,
    }
}

fn accept(
    first: OwnedHandle,
    name: &str,
    own: &str,
    user_of: UserOf,
    admitting: &Arc<Admitting>,
    closed: &AtomicBool,
    hand: &Sender<Asked>,
) {
    let Ok(event) = new_event() else {
        return;
    };
    let mut listening = first;
    loop {
        match connect(&listening, &event, closed) {
            Connect::Closed => return,
            Connect::Failed => {
                // SAFETY: the instance is open; a disconnect readies it for
                // the next connect.
                let _ = unsafe { DisconnectNamedPipe(HANDLE(listening.as_raw_handle())) };
                std::thread::sleep(ACCEPT_POLL);
            }
            Connect::Connected => {
                let next = loop {
                    if closed.load(Ordering::SeqCst) {
                        return;
                    }
                    match create_instance(name, own, false) {
                        Ok(next) => break next,
                        Err(_) => std::thread::sleep(ACCEPT_POLL),
                    }
                };
                let connected = std::mem::replace(&mut listening, next);
                let own = own.to_owned();
                let admitting = admitting.clone();
                let hand = hand.clone();
                let _ = std::thread::Builder::new()
                    .name("halvo-control-connection".to_owned())
                    .spawn(move || serve_one(connected, &own, user_of, &admitting, &hand));
            }
        }
    }
}

fn client_user(instance: &OwnedHandle, user_of: UserOf) -> Option<String> {
    let mut pid = 0u32;
    // SAFETY: the instance is open and connected, and `pid` is a live local.
    unsafe { GetNamedPipeClientProcessId(HANDLE(instance.as_raw_handle()), &mut pid) }.ok()?;
    user_of(pid).ok()
}

fn serve_one(
    instance: OwnedHandle,
    own: &str,
    user_of: UserOf,
    admitting: &Arc<Admitting>,
    hand: &Sender<Asked>,
) {
    if !client_user(&instance, user_of).is_some_and(|client| serves_client(&client, own)) {
        return;
    }
    let Ok(mut stream) = PipeStream::from_overlapped(instance) else {
        return;
    };
    if stream
        .set_timeout(super::READ_DEADLINE.max(super::REPLY_WRITE_DEADLINE))
        .is_err()
    {
        return;
    }
    let written = super::serve_request(&mut stream, RequestEnd::Length, admitting, hand);
    drop(stream);
    if let Some(written) = written {
        let _ = written.send(());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use halvo_control::proto::{self, Request};
    use halvo_control::wire;
    use std::io::{Read, Write};
    use std::time::Instant;

    const ANOTHER_USER: &str = "S-1-5-21-1000000001-1000000002-1000000003-1001";

    fn data_dir(root: &tempfile::TempDir) -> DataDir {
        DataDir::from_flag(root.path()).unwrap()
    }

    fn request_frame(request: &Request) -> Vec<u8> {
        let mut frame = Vec::new();
        wire::write_frame(
            &mut frame,
            &proto::encode_request(request),
            REQUEST_MAX_BYTES,
        )
        .unwrap();
        frame
    }

    #[test]
    fn the_pipe_server_drops_a_client_of_another_user() {
        let own = current_user().unwrap();
        assert!(
            serves_client(&own, &own),
            "the daemon's own user is not served"
        );
        assert!(!serves_client(ANOTHER_USER, &own), "another user is served");
        let root = tempfile::tempdir().unwrap();
        let listening =
            Listening::bind_reading(&data_dir(&root), |_| Ok(ANOTHER_USER.to_owned())).unwrap();
        let mut client = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(listening.path())
            .unwrap();
        let wrote = client.write_all(&request_frame(&Request::Status { indicator: false }));
        let mut reply = Vec::new();
        let read = client.read_to_end(&mut reply);
        assert!(
            reply.is_empty(),
            "a client of another user got {} bytes (write {wrote:?}, read {read:?})",
            reply.len()
        );
        let until = Instant::now() + Duration::from_millis(500);
        while Instant::now() < until {
            assert!(
                listening.next().is_none(),
                "the request of a client of another user reached the loop"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
