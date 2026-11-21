//! The control channel on Windows (contracts.md C15): the stream of a named pipe and the token
//! user that each end of the pipe reads of the other.

use std::io::{self, Read, Write};
use std::os::windows::io::{AsHandle, AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle};
use std::time::Duration;

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    LocalFree, ERROR_BROKEN_PIPE, ERROR_IO_PENDING, ERROR_OPERATION_ABORTED,
    ERROR_PIPE_NOT_CONNECTED, HANDLE, HLOCAL, WAIT_OBJECT_0,
};
use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows::Win32::Security::{GetTokenInformation, TokenUser, PSID, TOKEN_QUERY, TOKEN_USER};
use windows::Win32::Storage::FileSystem::{ReadFile, WriteFile};
use windows::Win32::System::Threading::{
    CreateEventW, GetCurrentProcess, OpenProcess, OpenProcessToken, WaitForSingleObject, INFINITE,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};

use crate::client::Transport;

fn code(error: &windows::core::Error) -> u32 {
    let hresult = error.code().0 as u32;
    if hresult & 0xFFFF_0000 == 0x8007_0000 {
        hresult & 0xFFFF
    } else {
        hresult
    }
}

fn os_error(error: windows::core::Error) -> io::Error {
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

fn user_of_token_of(process: HANDLE) -> io::Result<String> {
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
