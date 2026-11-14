//! What a file that `prepare` makes takes from the workbook besides its bytes. Every step acts
//! through the handle that read the workbook and the handle that made the file, never through
//! a path that a link could turn to another file.

use std::fs::File;
use std::io;
use std::path::Path;

/// The access that `prepare` asks for when it makes a file on Windows: read and write, and the
/// reading and writing of its access list.
#[cfg(windows)]
pub(crate) const MADE_ACCESS: u32 = 0x8000_0000 | 0x4000_0000 | 0x0002_0000 | 0x0004_0000;

/// Gives the file that `made` has open the extended attributes and the access list of the
/// workbook that `book` has open, through those two descriptors alone.
#[cfg(target_os = "macos")]
pub(crate) fn carry(book: &File, _: &Path, made: &File, _: &Path) -> io::Result<()> {
    use std::ffi::c_void;
    use std::os::fd::AsRawFd;

    extern "C" {
        fn fcopyfile(from: i32, to: i32, state: *mut c_void, flags: u32) -> i32;
    }
    const COPYFILE_ACL: u32 = 1 << 0;
    const COPYFILE_XATTR: u32 = 1 << 2;

    // SAFETY: both descriptors are open for the whole call, as the borrowed files live across
    // it; a null state is the documented way to ask for no state; `made` is open for writing,
    // which fcopyfile needs of its destination.
    let done = unsafe {
        fcopyfile(
            book.as_raw_fd(),
            made.as_raw_fd(),
            std::ptr::null_mut(),
            COPYFILE_ACL | COPYFILE_XATTR,
        )
    };
    if done < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Empties the access list of the file that `made` has open, through that descriptor.
#[cfg(target_os = "macos")]
pub(crate) fn uncarry(made: &File) -> io::Result<()> {
    use std::ffi::c_void;
    use std::os::fd::AsRawFd;

    extern "C" {
        fn acl_init(count: i32) -> *mut c_void;
        fn acl_set_fd(fd: i32, acl: *mut c_void) -> i32;
        fn acl_free(object: *mut c_void) -> i32;
    }

    // SAFETY: acl_init takes a count and returns a new list or null, which is checked below.
    let empty = unsafe { acl_init(0) };
    if empty.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `empty` is the valid list that acl_init returned, and the descriptor is open for
    // the whole call.
    let done = unsafe { acl_set_fd(made.as_raw_fd(), empty) };
    let error = (done < 0).then(io::Error::last_os_error);
    // SAFETY: `empty` came from acl_init, is freed once here and not used after.
    unsafe { acl_free(empty) };
    error.map_or(Ok(()), Err)
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn uncarry(_: &File) -> io::Result<()> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

/// Gives the file that `made` has open the named streams and the access list of the workbook
/// that `book` has open. The streams are listed through `book`; each is opened under the
/// paths with no reparse point followed, and used only when its handle is of the file that
/// `book` or `made` has open.
#[cfg(windows)]
pub(crate) fn carry(book: &File, workbook: &Path, made: &File, path: &Path) -> io::Result<()> {
    windows::streams(book, workbook, made, path)?;
    windows::access_list(book, made)
}

#[cfg(not(any(target_os = "macos", windows)))]
pub(crate) fn carry(_: &File, _: &Path, _: &File, _: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(windows)]
mod windows {
    use std::ffi::{c_void, OsString};
    use std::fs::{File, OpenOptions};
    use std::io::{self, Read, Write};
    use std::os::windows::ffi::OsStringExt;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use std::path::Path;

    use halvo_checkpoint::{same_file, FileId};

    use crate::parts::FILE_MAX;

    type Handle = *mut c_void;

    #[link(name = "kernel32")]
    extern "system" {
        fn GetFileInformationByHandleEx(
            file: Handle,
            class: i32,
            info: *mut c_void,
            size: u32,
        ) -> i32;
        fn LocalFree(memory: *mut c_void) -> *mut c_void;
    }

    #[link(name = "advapi32")]
    extern "system" {
        fn GetSecurityInfo(
            handle: Handle,
            object: u32,
            info: u32,
            owner: *mut *mut c_void,
            group: *mut *mut c_void,
            dacl: *mut *mut c_void,
            sacl: *mut *mut c_void,
            descriptor: *mut *mut c_void,
        ) -> u32;
        fn SetSecurityInfo(
            handle: Handle,
            object: u32,
            info: u32,
            owner: *mut c_void,
            group: *mut c_void,
            dacl: *mut c_void,
            sacl: *mut c_void,
        ) -> u32;
        fn GetSecurityDescriptorControl(
            descriptor: *mut c_void,
            control: *mut u16,
            revision: *mut u32,
        ) -> i32;
    }

    const FILE_STREAM_INFO: i32 = 7;
    const ERROR_HANDLE_EOF: i32 = 38;
    const ERROR_MORE_DATA: i32 = 234;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const SE_FILE_OBJECT: u32 = 1;
    const DACL_SECURITY_INFORMATION: u32 = 0x0000_0004;
    const PROTECTED_DACL_SECURITY_INFORMATION: u32 = 0x8000_0000;
    const UNPROTECTED_DACL_SECURITY_INFORMATION: u32 = 0x2000_0000;
    const SE_DACL_PROTECTED: u16 = 0x1000;

    /// Whether the handles `a` and `b` are open on one file, by the one compare of identity
    /// of the crate halvo-checkpoint.
    fn same(a: &File, b: &File) -> io::Result<()> {
        if same_file(&FileId::of(a)?, &FileId::of(b)?) {
            Ok(())
        } else {
            Err(io::Error::from(io::ErrorKind::NotFound))
        }
    }

    /// The names of the named streams of the file that `book` has open, `:name:$DATA` each,
    /// listed through that handle.
    fn stream_names(book: &File) -> io::Result<Vec<OsString>> {
        let mut room = 16 * 1024;
        let buffer = loop {
            let mut buffer = vec![0u64; room / 8];
            // SAFETY: the handle is open for the whole call, and the buffer holds `room` bytes,
            // aligned to 8 as FILE_STREAM_INFO needs.
            let done = unsafe {
                GetFileInformationByHandleEx(
                    book.as_raw_handle(),
                    FILE_STREAM_INFO,
                    buffer.as_mut_ptr().cast(),
                    room as u32,
                )
            };
            if done != 0 {
                break buffer;
            }
            let error = io::Error::last_os_error();
            match error.raw_os_error() {
                Some(ERROR_HANDLE_EOF) => return Ok(Vec::new()),
                Some(ERROR_MORE_DATA) if room < 1 << 24 => room *= 4,
                _ => return Err(error),
            }
        };
        let bytes: Vec<u8> = buffer.iter().flat_map(|word| word.to_le_bytes()).collect();
        let field = |at: usize| -> usize {
            bytes
                .get(at..at + 4)
                .map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
        };
        let mut names = Vec::new();
        let mut at = 0;
        loop {
            let (next, length) = (field(at), field(at + 4));
            let units: Vec<u16> = bytes
                .get(at + 24..at + 24 + length)
                .unwrap_or_default()
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_le_bytes(*pair))
                .collect();
            let name = OsString::from_wide(&units);
            if !name.is_empty() && name != "::$DATA" {
                names.push(name);
            }
            if next == 0 {
                return Ok(names);
            }
            at += next;
        }
    }

    pub(super) fn streams(
        book: &File,
        workbook: &Path,
        made: &File,
        path: &Path,
    ) -> io::Result<()> {
        let mut room = FILE_MAX;
        for stream in stream_names(book)? {
            let mut from = workbook.as_os_str().to_os_string();
            from.push(&stream);
            let mut to = path.as_os_str().to_os_string();
            to.push(&stream);
            let source = OpenOptions::new()
                .read(true)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
                .open(&from)?;
            same(&source, book)?;
            let mut bytes = Vec::new();
            (&source).take(room + 1).read_to_end(&mut bytes)?;
            if bytes.len() as u64 > room {
                return Err(io::Error::from(io::ErrorKind::FileTooLarge));
            }
            room -= bytes.len() as u64;
            let mut out = OpenOptions::new()
                .write(true)
                .create_new(true)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
                .open(&to)?;
            same(&out, made)?;
            out.write_all(&bytes)?;
            out.sync_all()?;
        }
        Ok(())
    }

    pub(super) fn access_list(book: &File, made: &File) -> io::Result<()> {
        let mut dacl: *mut c_void = std::ptr::null_mut();
        let mut descriptor: *mut c_void = std::ptr::null_mut();
        // SAFETY: the handle is open for the whole call and was opened for reading, which
        // holds READ_CONTROL; the out pointers point at locals that live across the call, and
        // the null ones are fields that the call is asked not to return.
        let read = unsafe {
            GetSecurityInfo(
                book.as_raw_handle(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut dacl,
                std::ptr::null_mut(),
                &mut descriptor,
            )
        };
        if read != 0 {
            return Err(io::Error::from_raw_os_error(read as i32));
        }
        let mut control = 0u16;
        let mut revision = 0u32;
        // SAFETY: `descriptor` is the descriptor that GetSecurityInfo returned, valid until
        // LocalFree below; the out pointers point at locals.
        let known =
            unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) };
        let unknown = (known == 0).then(io::Error::last_os_error);
        let protection = if control & SE_DACL_PROTECTED != 0 {
            PROTECTED_DACL_SECURITY_INFORMATION
        } else {
            UNPROTECTED_DACL_SECURITY_INFORMATION
        };
        let written = match unknown {
            Some(_) => 0,
            // SAFETY: the handle of `made` is open for the whole call with WRITE_DAC
            // ([`super::MADE_ACCESS`]); `dacl` points into `descriptor`, which is still
            // allocated; the null pointers are the parts that the call is asked not to set.
            None => unsafe {
                SetSecurityInfo(
                    made.as_raw_handle(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION | protection,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    dacl,
                    std::ptr::null_mut(),
                )
            },
        };
        // SAFETY: `descriptor` came from GetSecurityInfo, is freed once here, and neither it
        // nor `dacl`, which points into it, is used after.
        unsafe { LocalFree(descriptor) };
        if let Some(error) = unknown {
            return Err(error);
        }
        if written != 0 {
            return Err(io::Error::from_raw_os_error(written as i32));
        }
        Ok(())
    }
}
