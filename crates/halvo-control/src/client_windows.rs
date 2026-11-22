//! The control channel on Windows (contracts.md C15): the stream of a named pipe, the token
//! user that each end of the pipe reads of the other, and the connector of a data directory's
//! pipe.

use std::io::{self, Read, Write};
use std::os::windows::io::{AsHandle, AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle};
use std::time::{Duration, Instant};

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    LocalFree, ERROR_BROKEN_PIPE, ERROR_FILE_NOT_FOUND, ERROR_IO_PENDING, ERROR_OPERATION_ABORTED,
    ERROR_PIPE_BUSY, ERROR_PIPE_NOT_CONNECTED, GENERIC_READ, GENERIC_WRITE, HANDLE, HLOCAL,
    WAIT_OBJECT_0,
};
use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows::Win32::Security::{GetTokenInformation, TokenUser, PSID, TOKEN_QUERY, TOKEN_USER};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, WriteFile, FILE_FLAG_OVERLAPPED, FILE_SHARE_NONE, OPEN_EXISTING,
    SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT,
};
use windows::Win32::System::Pipes::{GetNamedPipeServerProcessId, WaitNamedPipeW};
use windows::Win32::System::Threading::{
    CreateEventW, GetCurrentProcess, OpenProcess, OpenProcessToken, WaitForSingleObject, INFINITE,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};

use crate::client::{ClientError, Connector, PeerRejection, Transport};
use crate::paths::DataDir;

fn code(error: &windows::core::Error) -> u32 {
    let hresult = error.code().0 as u32;
    if hresult & 0xFFFF_0000 == 0x8007_0000 {
        hresult & 0xFFFF
    } else {
        hresult
    }
}

pub(crate) fn os_error(error: windows::core::Error) -> io::Error {
    io::Error::from_raw_os_error(code(&error) as i32)
}

fn wait_millis(timeout: Option<Duration>) -> u32 {
    timeout.map_or(INFINITE, |timeout| {
        u32::try_from(timeout.as_micros().div_ceil(1_000))
            .map_or(INFINITE - 1, |ms| ms.min(INFINITE - 1))
    })
}

/// The SID string of the token user of this process.
pub fn current_user() -> io::Result<String> {
    // SAFETY: GetCurrentProcess takes nothing and returns the pseudo handle of this process,
    // which needs no close.
    let process = unsafe { GetCurrentProcess() };
    user_of_token_of(process)
}

/// The SID string of the token user of the process `pid`.
pub fn user_of_process(pid: u32) -> io::Result<String> {
    // SAFETY: OpenProcess takes no pointer; the handle that it returns is owned at once below.
    let process =
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.map_err(os_error)?;
    // SAFETY: `process` is a new handle that nothing else owns or closes.
    let owned = unsafe { OwnedHandle::from_raw_handle(process.0) };
    user_of_token_of(HANDLE(owned.as_raw_handle()))
}

pub(crate) fn user_of_token_of(process: HANDLE) -> io::Result<String> {
    let mut token = HANDLE::default();
    // SAFETY: `process` is open for the call, and `token` is a live local that receives a new
    // handle.
    unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) }.map_err(os_error)?;
    // SAFETY: `token` is a new handle that nothing else owns or closes.
    let owned = unsafe { OwnedHandle::from_raw_handle(token.0) };
    let token = HANDLE(owned.as_raw_handle());
    let mut needed = 0u32;
    // SAFETY: with no buffer the call writes only the length that it needs, into the live local
    // `needed`.
    let _ = unsafe { GetTokenInformation(token, TokenUser, None, 0, &mut needed) };
    let mut buffer = vec![0u64; (needed as usize).div_ceil(8).max(1)];
    let size =
        u32::try_from(buffer.len() * 8).map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
    // SAFETY: `buffer` holds `size` bytes, aligned to 8 as TOKEN_USER asks, and lives across the
    // call; `needed` is a live local.
    unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            Some(buffer.as_mut_ptr().cast()),
            size,
            &mut needed,
        )
    }
    .map_err(os_error)?;
    // SAFETY: the call wrote a TOKEN_USER at the start of `buffer`, whose SID points inside
    // `buffer`, which lives until `sid_text` returns.
    let sid = unsafe { (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    sid_text(sid)
}

fn sid_text(sid: PSID) -> io::Result<String> {
    let mut text = PWSTR::null();
    // SAFETY: `sid` points to a valid SID for the call, and `text` is a live local that receives
    // a string that LocalFree releases below.
    unsafe { ConvertSidToStringSidW(sid, &mut text) }.map_err(os_error)?;
    // SAFETY: `text` is the NUL-terminated wide string that the call above wrote.
    let read = unsafe { text.to_string() };
    // SAFETY: the string came from ConvertSidToStringSidW, whose memory LocalFree releases, once.
    let _ = unsafe { LocalFree(Some(HLOCAL(text.0.cast()))) };
    read.map_err(|_| io::Error::from(io::ErrorKind::InvalidData))
}

/// One end of a connected pipe that was opened for overlapped I/O. Each read and each write
/// waits at most the timeout that was set last, and the close of the other end reads as the end
/// of the stream. The request of a byte-mode pipe ends by its length, since such a pipe has no
/// half-close (contracts.md C15.4).
#[derive(Debug)]
pub struct PipeStream {
    pipe: OwnedHandle,
    event: OwnedHandle,
    timeout: Option<Duration>,
}

impl PipeStream {
    /// Takes `pipe`, a connected end of a pipe that was opened with FILE_FLAG_OVERLAPPED.
    pub fn from_overlapped(pipe: OwnedHandle) -> io::Result<PipeStream> {
        // SAFETY: CreateEventW is given no attributes and no name; the handle that it returns is
        // owned at once below.
        let event = unsafe { CreateEventW(None, true, false, PCWSTR::null()) }.map_err(os_error)?;
        // SAFETY: `event` is a new handle that nothing else owns or closes.
        let event = unsafe { OwnedHandle::from_raw_handle(event.0) };
        Ok(PipeStream {
            pipe,
            event,
            timeout: None,
        })
    }

    fn transfer(
        &mut self,
        start: impl FnOnce(HANDLE, *mut OVERLAPPED) -> windows::core::Result<()>,
    ) -> io::Result<usize> {
        let pipe = HANDLE(self.pipe.as_raw_handle());
        let event = HANDLE(self.event.as_raw_handle());
        let mut overlapped = OVERLAPPED {
            hEvent: event,
            ..Default::default()
        };
        match start(pipe, &mut overlapped) {
            Ok(()) => {}
            Err(error) if code(&error) == ERROR_IO_PENDING.0 => {}
            Err(error) => return Err(os_error(error)),
        }
        // SAFETY: the event is open; the call takes no pointer.
        if unsafe { WaitForSingleObject(event, wait_millis(self.timeout)) } != WAIT_OBJECT_0 {
            // SAFETY: the pipe is open and `overlapped` names the one operation started above.
            let _ = unsafe { CancelIoEx(pipe, Some(&overlapped)) };
        }
        let mut done = 0u32;
        // SAFETY: the call waits until the operation of `overlapped` has ended, which a cancel
        // ends at once; `overlapped`, `done` and the caller's buffer live until it returns.
        match unsafe { GetOverlappedResult(pipe, &overlapped, &mut done, true) } {
            Ok(()) => Ok(done as usize),
            Err(error) if code(&error) == ERROR_OPERATION_ABORTED.0 => {
                Err(io::ErrorKind::TimedOut.into())
            }
            Err(error) => Err(os_error(error)),
        }
    }
}

impl Read for PipeStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let len = buf.len().min(u32::MAX as usize);
        if len == 0 {
            return Ok(0);
        }
        let read = self.transfer(|pipe, overlapped| {
            // SAFETY: the pipe is open, and `buf` and `overlapped` outlive the operation, which
            // `transfer` waits for.
            unsafe { ReadFile(pipe, Some(&mut buf[..len]), None, Some(overlapped)) }
        });
        match read {
            Err(error)
                if error.raw_os_error() == Some(ERROR_BROKEN_PIPE.0 as i32)
                    || error.raw_os_error() == Some(ERROR_PIPE_NOT_CONNECTED.0 as i32) =>
            {
                Ok(0)
            }
            other => other,
        }
    }
}

impl Write for PipeStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let len = buf.len().min(u32::MAX as usize);
        if len == 0 {
            return Ok(0);
        }
        self.transfer(|pipe, overlapped| {
            // SAFETY: the pipe is open, and `buf` and `overlapped` outlive the operation, which
            // `transfer` waits for.
            unsafe { WriteFile(pipe, Some(&buf[..len]), None, Some(overlapped)) }
        })
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Transport for PipeStream {
    fn set_timeout(&mut self, timeout: Duration) -> io::Result<()> {
        self.timeout = Some(timeout);
        Ok(())
    }

    fn end_request(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl AsHandle for PipeStream {
    fn as_handle(&self) -> BorrowedHandle<'_> {
        self.pipe.as_handle()
    }
}

/// The connector of the named pipe of a data directory for the user of this process
/// (contracts.md C15.1, C15.3, C15.5). It opens the pipe with identification-level impersonation
/// alone, waits for a busy pipe inside the request's deadline, and answers
/// [`ClientError::Unreachable`] with the pipe's name when no pipe of that name stands. Before it
/// hands the stream over, it reads the server's process id from the pipe and requires the token
/// user of that process to be its own; otherwise it answers
/// [`PeerRejection::ServerOfAnotherUser`] and no byte of the request is written.
#[derive(Debug, Clone)]
pub struct PipeConnector {
    name: String,
    own: String,
    user_of: fn(u32) -> io::Result<String>,
}

/// Whether a client whose own user has the SID string `own` sends to a server whose token user
/// has the SID string `server`: only when the two are one (contracts.md C15.3).
pub fn check_server(server: &str, own: &str) -> Result<(), PeerRejection> {
    if server == own {
        Ok(())
    } else {
        Err(PeerRejection::ServerOfAnotherUser)
    }
}

impl PipeConnector {
    /// The connector to the pipe that [`crate::paths::pipe_name`] gives `data_dir` with the SID of
    /// this process's user.
    pub fn for_data_dir(data_dir: &DataDir) -> Result<PipeConnector, ClientError> {
        let own = current_user().map_err(ClientError::Io)?;
        Ok(PipeConnector {
            name: crate::paths::pipe_name(&own, data_dir),
            own,
            user_of: user_of_process,
        })
    }

    #[cfg(test)]
    fn reading_users_as(mut self, user_of: fn(u32) -> io::Result<String>) -> Self {
        self.user_of = user_of;
        self
    }

    /// The name of the pipe.
    pub fn name(&self) -> &str {
        &self.name
    }

    fn checked(&self, pipe: OwnedHandle) -> Result<PipeStream, ClientError> {
        let mut pid = 0u32;
        // SAFETY: the pipe is open and `pid` is a live local.
        unsafe { GetNamedPipeServerProcessId(HANDLE(pipe.as_raw_handle()), &mut pid) }
            .map_err(|error| ClientError::Io(os_error(error)))?;
        let server = (self.user_of)(pid).map_err(ClientError::Io)?;
        check_server(&server, &self.own).map_err(ClientError::PeerRejected)?;
        PipeStream::from_overlapped(pipe).map_err(ClientError::Io)
    }
}

impl Connector for PipeConnector {
    type Transport = PipeStream;

    fn endpoint(&self) -> String {
        self.name.clone()
    }

    fn connect(&self, timeout: Duration) -> Result<PipeStream, ClientError> {
        let until = Instant::now() + timeout;
        let name: Vec<u16> = self.name.encode_utf16().chain(std::iter::once(0)).collect();
        loop {
            // SAFETY: `name` ends in a NUL and outlives the call; no other pointer is passed.
            let opened = unsafe {
                CreateFileW(
                    PCWSTR(name.as_ptr()),
                    (GENERIC_READ | GENERIC_WRITE).0,
                    FILE_SHARE_NONE,
                    None,
                    OPEN_EXISTING,
                    FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                    None,
                )
            };
            let error = match opened {
                Ok(pipe) => {
                    // SAFETY: `pipe` is a new handle that nothing else owns or closes.
                    let pipe = unsafe { OwnedHandle::from_raw_handle(pipe.0) };
                    return self.checked(pipe);
                }
                Err(error) => error,
            };
            if code(&error) == ERROR_FILE_NOT_FOUND.0 {
                return Err(ClientError::Unreachable {
                    endpoint: self.endpoint(),
                });
            }
            if code(&error) != ERROR_PIPE_BUSY.0 {
                return Err(ClientError::Io(os_error(error)));
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(ClientError::Deadline {
                    seconds: timeout.as_secs(),
                });
            }
            // SAFETY: `name` ends in a NUL and outlives the call; no other pointer is passed.
            let _ = unsafe { WaitNamedPipeW(PCWSTR(name.as_ptr()), wait_millis(Some(left))) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::Client;
    use windows::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
    use windows::Win32::System::Pipes::{
        CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
    };

    const ANOTHER_USER: &str = "S-1-5-21-1000000001-1000000002-1000000003-1001";

    fn server_end(name: &str) -> PipeStream {
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
        let pipe = unsafe { OwnedHandle::from_raw_handle(pipe.0) };
        PipeStream::from_overlapped(pipe).unwrap()
    }

    #[test]
    fn the_pipe_client_refuses_a_server_of_another_user() {
        let own = current_user().unwrap();
        assert_eq!(check_server(&own, &own), Ok(()));
        assert_eq!(
            check_server(ANOTHER_USER, &own),
            Err(PeerRejection::ServerOfAnotherUser)
        );
        let root = tempfile::tempdir().unwrap();
        let connector = PipeConnector::for_data_dir(&DataDir::from_flag(root.path()).unwrap())
            .unwrap()
            .reading_users_as(|_| Ok(ANOTHER_USER.to_owned()));
        let mut server = server_end(connector.name());
        let refused = Client::new(connector).status();
        assert!(
            matches!(
                refused,
                Err(ClientError::PeerRejected(
                    PeerRejection::ServerOfAnotherUser
                ))
            ),
            "{refused:?}"
        );
        server.set_timeout(Duration::from_secs(5)).unwrap();
        let mut written = Vec::new();
        let read = server.read_to_end(&mut written);
        assert!(
            written.is_empty(),
            "the client wrote {} bytes to a server of another user ({read:?})",
            written.len()
        );
    }
}
