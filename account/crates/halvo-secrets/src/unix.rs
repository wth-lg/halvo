use crate::place::{EntryKind, Fs, Meta, Opened, Published};
use std::ffi::CString;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, Read as _, Write as _};
use std::os::fd::AsRawFd;
use std::os::raw::{c_char, c_int, c_uint, c_void};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::Path;
use std::ptr;

const O_NONBLOCK: c_int = 0x0004;
const O_NOFOLLOW: c_int = 0x0100;
const ELOOP: i32 = 62;
const EEXIST: i32 = 17;
const RENAME_EXCL: c_uint = 0x0004;
const ENOENT: i32 = 2;
const ACL_TYPE_EXTENDED: c_int = 0x0100;
const ACL_FIRST_ENTRY: c_int = 0;

extern "C" {
    fn geteuid() -> u32;
    fn renamex_np(from: *const c_char, to: *const c_char, flags: c_uint) -> c_int;
    fn acl_get_fd_np(fd: c_int, kind: c_int) -> *mut c_void;
    fn acl_get_link_np(path: *const c_char, kind: c_int) -> *mut c_void;
    fn acl_get_entry(acl: *mut c_void, entry_id: c_int, entry: *mut *mut c_void) -> c_int;
    fn acl_free(object: *mut c_void) -> c_int;
}

pub(crate) struct UnixFs;

fn has_entries(acl: *mut c_void) -> io::Result<bool> {
    if acl.is_null() {
        let error = io::Error::last_os_error();
        return if error.raw_os_error() == Some(ENOENT) {
            Ok(false)
        } else {
            Err(error)
        };
    }
    let mut entry = ptr::null_mut();
    let found = unsafe { acl_get_entry(acl, ACL_FIRST_ENTRY, &mut entry) } == 0;
    unsafe { acl_free(acl) };
    Ok(found)
}

fn meta_of(metadata: &fs::Metadata, acl: bool) -> Meta {
    let file_type = metadata.file_type();
    let kind = if file_type.is_symlink() {
        EntryKind::Link
    } else if file_type.is_dir() {
        EntryKind::Directory
    } else if file_type.is_file() {
        EntryKind::File
    } else {
        EntryKind::Other
    };
    Meta {
        kind,
        owner: metadata.uid(),
        mode: metadata.mode() & 0o7777,
        len: metadata.len(),
        acl,
        links: metadata.nlink(),
    }
}

fn c_path(path: &Path) -> io::Result<CString> {
    CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))
}

impl Fs for UnixFs {
    type Item = File;
    type Temp = File;

    fn user(&self) -> u32 {
        unsafe { geteuid() }
    }

    fn lstat(&self, path: &Path) -> io::Result<Option<Meta>> {
        match fs::symlink_metadata(path) {
            Ok(metadata) => {
                let path = c_path(path)?;
                let acl =
                    has_entries(unsafe { acl_get_link_np(path.as_ptr(), ACL_TYPE_EXTENDED) })?;
                Ok(Some(meta_of(&metadata, acl)))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn create_dir(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(parent)?;
        }
        match DirBuilder::new().mode(0o700).create(path) {
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
            other => other,
        }
    }

    fn open_item(&self, path: &Path) -> io::Result<Opened<File>> {
        match self.lstat(path)? {
            None => return Ok(Opened::Absent),
            Some(meta) if meta.kind != EntryKind::File => return Ok(Opened::NotAFile),
            Some(_) => {}
        }
        match OpenOptions::new()
            .read(true)
            .custom_flags(O_NOFOLLOW | O_NONBLOCK)
            .open(path)
        {
            Ok(file) => Ok(Opened::Item(file)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Opened::Absent),
            Err(error) if error.raw_os_error() == Some(ELOOP) => Ok(Opened::NotAFile),
            Err(error) => Err(error),
        }
    }

    fn item_meta(&self, item: &File) -> io::Result<Meta> {
        let acl = has_entries(unsafe { acl_get_fd_np(item.as_raw_fd(), ACL_TYPE_EXTENDED) })?;
        Ok(meta_of(&item.metadata()?, acl))
    }

    fn read_item(&self, item: &mut File, buf: &mut [u8]) -> io::Result<usize> {
        item.read(buf)
    }

    fn create_temp(&self, path: &Path) -> io::Result<Option<File>> {
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(O_NOFOLLOW)
            .open(path)
        {
            Ok(file) => Ok(Some(file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn write_temp(&self, temp: &mut File, bytes: &[u8]) -> io::Result<()> {
        temp.write_all(bytes)
    }

    fn sync_temp(&self, temp: &File) -> io::Result<()> {
        temp.sync_all()
    }

    fn publish(&self, from: &Path, to: &Path) -> io::Result<Published> {
        let from = c_path(from)?;
        let to = c_path(to)?;
        if unsafe { renamex_np(from.as_ptr(), to.as_ptr(), RENAME_EXCL) } == 0 {
            return Ok(Published::Done);
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(EEXIST) {
            Ok(Published::Exists)
        } else {
            Err(error)
        }
    }

    fn remove(&self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }

    fn sync_dir(&self, path: &Path) -> io::Result<()> {
        File::open(path)?.sync_all()
    }
}
