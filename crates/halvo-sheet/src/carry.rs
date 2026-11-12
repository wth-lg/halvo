use std::io;
use std::path::Path;

#[cfg(target_os = "macos")]
pub(crate) fn carry(workbook: &Path, made: &Path) -> io::Result<()> {
    use std::ffi::c_void;
    use std::fs::{File, OpenOptions};
    use std::os::fd::AsRawFd;

    extern "C" {
        fn fcopyfile(from: i32, to: i32, state: *mut c_void, flags: u32) -> i32;
    }
    const COPYFILE_ACL: u32 = 1 << 0;
    const COPYFILE_XATTR: u32 = 1 << 2;

    let from = File::open(workbook)?;
    let to = OpenOptions::new().write(true).open(made)?;
    let done = unsafe {
        fcopyfile(
            from.as_raw_fd(),
            to.as_raw_fd(),
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

#[cfg(target_os = "macos")]
pub(crate) fn uncarry(made: &Path) -> io::Result<()> {
    use std::ffi::c_void;
    use std::fs::File;
    use std::os::fd::AsRawFd;

    extern "C" {
        fn acl_init(count: i32) -> *mut c_void;
        fn acl_set_fd(fd: i32, acl: *mut c_void) -> i32;
        fn acl_free(object: *mut c_void) -> i32;
    }

    let file = File::open(made)?;
    let empty = unsafe { acl_init(0) };
    if empty.is_null() {
        return Err(io::Error::last_os_error());
    }
    let done = unsafe { acl_set_fd(file.as_raw_fd(), empty) };
    let error = (done < 0).then(io::Error::last_os_error);
    unsafe { acl_free(empty) };
    error.map_or(Ok(()), Err)
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn uncarry(_: &Path) -> io::Result<()> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

#[cfg(windows)]
pub(crate) fn carry(workbook: &Path, made: &Path) -> io::Result<()> {
    windows::streams(workbook, made)?;
    windows::access_list(workbook, made)
}

#[cfg(not(any(target_os = "macos", windows)))]
pub(crate) fn carry(_: &Path, _: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(windows)]
mod windows {
    use std::ffi::{c_void, OsString};
    use std::fs::{File, OpenOptions};
    use std::io::{self, Read, Write};
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use std::path::Path;

    use crate::parts::FILE_MAX;

    type Handle = *mut c_void;

    #[repr(C)]
    struct FindStreamData {
        size: i64,
        name: [u16; 296],
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn FindFirstStreamW(name: *const u16, level: i32, data: *mut c_void, flags: u32) -> Handle;
        fn FindNextStreamW(find: Handle, data: *mut c_void) -> i32;
        fn FindClose(find: Handle) -> i32;
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

    const ERROR_HANDLE_EOF: i32 = 38;
    const SE_FILE_OBJECT: u32 = 1;
    const DACL_SECURITY_INFORMATION: u32 = 0x0000_0004;
    const PROTECTED_DACL_SECURITY_INFORMATION: u32 = 0x8000_0000;
    const UNPROTECTED_DACL_SECURITY_INFORMATION: u32 = 0x2000_0000;
    const SE_DACL_PROTECTED: u16 = 0x1000;
    const READ_CONTROL: u32 = 0x0002_0000;
    const WRITE_DAC: u32 = 0x0004_0000;

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    fn stream_names(workbook: &Path) -> io::Result<Vec<OsString>> {
        let name = wide(workbook);
        let mut data = FindStreamData {
            size: 0,
            name: [0; 296],
        };
        let find = unsafe {
            FindFirstStreamW(
                name.as_ptr(),
                0,
                (&mut data as *mut FindStreamData).cast(),
                0,
            )
        };
        if find as isize == -1 {
            let error = io::Error::last_os_error();
            return match error.raw_os_error() {
                Some(ERROR_HANDLE_EOF) => Ok(Vec::new()),
                _ => Err(error),
            };
        }
        let mut names = Vec::new();
        loop {
            let len = data.name.iter().position(|&unit| unit == 0).unwrap_or(296);
            let stream = OsString::from_wide(&data.name[..len]);
            if stream != "::$DATA" {
                names.push(stream);
            }
            if unsafe { FindNextStreamW(find, (&mut data as *mut FindStreamData).cast()) } == 0 {
                let error = io::Error::last_os_error();
                unsafe { FindClose(find) };
                return match error.raw_os_error() {
                    Some(ERROR_HANDLE_EOF) => Ok(names),
                    _ => Err(error),
                };
            }
        }
    }

    pub(super) fn streams(workbook: &Path, made: &Path) -> io::Result<()> {
        let mut room = FILE_MAX;
        for stream in stream_names(workbook)? {
            let mut from = workbook.as_os_str().to_os_string();
            from.push(&stream);
            let mut to = made.as_os_str().to_os_string();
            to.push(&stream);
            let mut bytes = Vec::new();
            File::open(&from)?.take(room + 1).read_to_end(&mut bytes)?;
            if bytes.len() as u64 > room {
                return Err(io::Error::from(io::ErrorKind::FileTooLarge));
            }
            room -= bytes.len() as u64;
            let mut out = OpenOptions::new().write(true).create_new(true).open(&to)?;
            out.write_all(&bytes)?;
            out.sync_all()?;
        }
        Ok(())
    }

    pub(super) fn access_list(workbook: &Path, made: &Path) -> io::Result<()> {
        let from = File::open(workbook)?;
        let to = OpenOptions::new()
            .access_mode(READ_CONTROL | WRITE_DAC)
            .open(made)?;
        let mut dacl: *mut c_void = std::ptr::null_mut();
        let mut descriptor: *mut c_void = std::ptr::null_mut();
        let read = unsafe {
            GetSecurityInfo(
                from.as_raw_handle(),
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
            None => unsafe {
                SetSecurityInfo(
                    to.as_raw_handle(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION | protection,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    dacl,
                    std::ptr::null_mut(),
                )
            },
        };
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
