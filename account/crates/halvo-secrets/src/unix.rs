use crate::place::{EntryKind, Fs, Last, Meta, Opened, Published, Walk};
use std::ffi::{CString, OsStr, OsString};
use std::fs::{self, File};
use std::io::{self, Read as _, Write as _};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::raw::{c_char, c_int, c_uint, c_void};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::ptr;

const O_RDONLY: c_int = 0x0000;
const O_WRONLY: c_int = 0x0001;
const O_NONBLOCK: c_int = 0x0004;
const O_NOFOLLOW: c_int = 0x0100;
const O_CREAT: c_int = 0x0200;
const O_EXCL: c_int = 0x0800;
const O_DIRECTORY: c_int = 0x0010_0000;
const O_CLOEXEC: c_int = 0x0100_0000;
const AT_SYMLINK_NOFOLLOW: c_int = 0x0020;
const ENOENT: i32 = 2;
const EEXIST: i32 = 17;
const ENOTDIR: i32 = 20;
const ELOOP: i32 = 62;
const RENAME_EXCL: c_uint = 0x0004;
const ACL_TYPE_EXTENDED: c_int = 0x0100;
const ACL_FIRST_ENTRY: c_int = 0;
const ACL_NEXT_ENTRY: c_int = -1;
const ACL_EXTENDED_ALLOW: c_int = 1;
const ACL_CHANGES_ENTRIES: [c_int; 6] = [1 << 2, 1 << 4, 1 << 5, 1 << 6, 1 << 12, 1 << 13];
const STAT_SPACE: usize = 512;

extern "C" {
    fn geteuid() -> u32;
    fn openat(fd: c_int, path: *const c_char, flags: c_int, ...) -> c_int;
    fn mkdirat(fd: c_int, path: *const c_char, mode: u16) -> c_int;
    fn unlinkat(fd: c_int, path: *const c_char, flags: c_int) -> c_int;
    fn fstatat(fd: c_int, path: *const c_char, buf: *mut c_void, flags: c_int) -> c_int;
    fn renameatx_np(
        from_fd: c_int,
        from: *const c_char,
        to_fd: c_int,
        to: *const c_char,
        flags: c_uint,
    ) -> c_int;
    fn acl_get_fd_np(fd: c_int, kind: c_int) -> *mut c_void;
    fn acl_get_entry(acl: *mut c_void, entry_id: c_int, entry: *mut *mut c_void) -> c_int;
    fn acl_get_tag_type(entry: *mut c_void, tag: *mut c_int) -> c_int;
    fn acl_get_permset(entry: *mut c_void, permset: *mut *mut c_void) -> c_int;
    fn acl_get_perm_np(permset: *mut c_void, perm: c_int) -> c_int;
    fn acl_free(object: *mut c_void) -> c_int;
}

pub(crate) struct UnixFs;

struct Acl {
    any: bool,
    changes_entries: bool,
}

fn acl_of(file: &File) -> io::Result<Acl> {
    let acl = unsafe { acl_get_fd_np(file.as_raw_fd(), ACL_TYPE_EXTENDED) };
    if acl.is_null() {
        let error = io::Error::last_os_error();
        return if error.raw_os_error() == Some(ENOENT) {
            Ok(Acl {
                any: false,
                changes_entries: false,
            })
        } else {
            Err(error)
        };
    }
    let mut found = Acl {
        any: false,
        changes_entries: false,
    };
    let mut entry = ptr::null_mut();
    let mut which = ACL_FIRST_ENTRY;
    while unsafe { acl_get_entry(acl, which, &mut entry) } == 0 {
        which = ACL_NEXT_ENTRY;
        found.any = true;
        let mut tag = 0;
        let mut permset = ptr::null_mut();
        if unsafe { acl_get_tag_type(entry, &mut tag) } != 0
            || unsafe { acl_get_permset(entry, &mut permset) } != 0
        {
            found.changes_entries = true;
            continue;
        }
        if tag == ACL_EXTENDED_ALLOW
            && ACL_CHANGES_ENTRIES
                .iter()
                .any(|perm| unsafe { acl_get_perm_np(permset, *perm) } == 1)
        {
            found.changes_entries = true;
        }
    }
    unsafe { acl_free(acl) };
    Ok(found)
}

fn meta_of(file: &File) -> io::Result<Meta> {
    let metadata = file.metadata()?;
    let acl = acl_of(file)?;
    let file_type = metadata.file_type();
    let kind = if file_type.is_dir() {
        EntryKind::Directory
    } else if file_type.is_file() {
        EntryKind::File
    } else {
        EntryKind::Other
    };
    Ok(Meta {
        kind,
        owner: metadata.uid(),
        mode: metadata.mode() & 0o7777,
        len: metadata.len(),
        acl: acl.any,
        acl_write: acl.changes_entries,
        links: metadata.nlink(),
    })
}

fn c_name(name: &OsStr) -> io::Result<CString> {
    CString::new(name.as_bytes()).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))
}

fn open_at(dir: &File, name: &OsStr, flags: c_int) -> io::Result<File> {
    let name = c_name(name)?;
    let fd = unsafe {
        openat(
            dir.as_raw_fd(),
            name.as_ptr(),
            flags | O_CLOEXEC,
            0o600 as c_uint,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn open_dir_at(dir: &File, name: &OsStr) -> io::Result<File> {
    open_at(dir, name, O_RDONLY | O_DIRECTORY | O_NOFOLLOW)
}

fn last_error_unless(result: c_int) -> io::Result<()> {
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn split(path: &Path) -> io::Result<(PathBuf, Vec<OsString>, OsString)> {
    let invalid = || io::Error::from(io::ErrorKind::InvalidInput);
    let last = path.file_name().ok_or_else(invalid)?.to_os_string();
    let mut base = path.parent().ok_or_else(invalid)?.to_path_buf();
    let mut missing = Vec::new();
    loop {
        match fs::canonicalize(&base) {
            Ok(real) => {
                missing.reverse();
                return Ok((real, missing, last));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                missing.push(base.file_name().ok_or(error)?.to_os_string());
                base.pop();
            }
            Err(error) => return Err(error),
        }
    }
}

fn steps(real: &Path) -> impl Iterator<Item = &OsStr> {
    real.components().filter_map(|step| match step {
        Component::Normal(name) => Some(name),
        _ => None,
    })
}

impl Fs for UnixFs {
    type Dir = File;
    type Item = File;
    type Temp = File;

    fn user(&self) -> u32 {
        unsafe { geteuid() }
    }

    fn walk(&self, path: &Path) -> io::Result<Walk<File>> {
        let (real, missing, last) = split(path)?;
        let mut dir = File::open("/")?;
        let mut ancestors = vec![meta_of(&dir)?];
        for name in steps(&real).chain(missing.iter().map(OsString::as_os_str)) {
            match open_dir_at(&dir, name) {
                Ok(child) => {
                    ancestors.push(meta_of(&child)?);
                    dir = child;
                }
                Err(error) if error.raw_os_error() == Some(ENOENT) => {
                    return Ok(Walk {
                        ancestors,
                        last: Last::Absent,
                    });
                }
                Err(error) if matches!(error.raw_os_error(), Some(ENOTDIR | ELOOP)) => {
                    ancestors.push(Meta::of_kind(EntryKind::Other));
                    return Ok(Walk {
                        ancestors,
                        last: Last::Absent,
                    });
                }
                Err(error) => return Err(error),
            }
        }
        let last = match open_at(&dir, &last, O_RDONLY | O_NOFOLLOW | O_NONBLOCK) {
            Ok(handle) => {
                let meta = meta_of(&handle)?;
                let handle = (meta.kind == EntryKind::Directory).then_some(handle);
                Last::Found(meta, handle)
            }
            Err(error) if error.raw_os_error() == Some(ENOENT) => Last::Absent,
            Err(error) if error.raw_os_error() == Some(ELOOP) => {
                Last::Found(Meta::of_kind(EntryKind::Link), None)
            }
            Err(error) => return Err(error),
        };
        Ok(Walk { ancestors, last })
    }

    fn make_dir(&self, path: &Path) -> io::Result<()> {
        let (real, missing, last) = split(path)?;
        let mut dir = File::open("/")?;
        for name in steps(&real) {
            dir = open_dir_at(&dir, name)?;
        }
        for name in missing.iter().chain([&last]) {
            let c = c_name(name)?;
            if unsafe { mkdirat(dir.as_raw_fd(), c.as_ptr(), 0o700) } != 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() != Some(EEXIST) {
                    return Err(error);
                }
            }
            dir = open_dir_at(&dir, name)?;
        }
        Ok(())
    }

    fn exists(&self, dir: &File, name: &str) -> io::Result<bool> {
        let name = c_name(OsStr::new(name))?;
        let mut space = [0u8; STAT_SPACE];
        let result = unsafe {
            fstatat(
                dir.as_raw_fd(),
                name.as_ptr(),
                space.as_mut_ptr().cast(),
                AT_SYMLINK_NOFOLLOW,
            )
        };
        if result == 0 {
            return Ok(true);
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(ENOENT) {
            Ok(false)
        } else {
            Err(error)
        }
    }

    fn open_item(&self, dir: &File, name: &str) -> io::Result<Opened<File>> {
        match open_at(dir, OsStr::new(name), O_RDONLY | O_NOFOLLOW | O_NONBLOCK) {
            Ok(file) => Ok(Opened::Item(file)),
            Err(error) if error.raw_os_error() == Some(ENOENT) => Ok(Opened::Absent),
            Err(error) if error.raw_os_error() == Some(ELOOP) => Ok(Opened::NotAFile),
            Err(error) => Err(error),
        }
    }

    fn item_meta(&self, item: &File) -> io::Result<Meta> {
        meta_of(item)
    }

    fn read_item(&self, item: &mut File, buf: &mut [u8]) -> io::Result<usize> {
        item.read(buf)
    }

    fn create_temp(&self, dir: &File, name: &str) -> io::Result<Option<File>> {
        match open_at(
            dir,
            OsStr::new(name),
            O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW,
        ) {
            Ok(file) => Ok(Some(file)),
            Err(error) if error.raw_os_error() == Some(EEXIST) => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn write_temp(&self, temp: &mut File, bytes: &[u8]) -> io::Result<()> {
        temp.write_all(bytes)
    }

    fn sync_temp(&self, temp: &File) -> io::Result<()> {
        temp.sync_all()
    }

    fn publish(&self, dir: &File, from: &str, to: &str) -> io::Result<Published> {
        let from = c_name(OsStr::new(from))?;
        let to = c_name(OsStr::new(to))?;
        let fd = dir.as_raw_fd();
        if unsafe { renameatx_np(fd, from.as_ptr(), fd, to.as_ptr(), RENAME_EXCL) } == 0 {
            return Ok(Published::Done);
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(EEXIST) {
            Ok(Published::Exists)
        } else {
            Err(error)
        }
    }

    fn remove(&self, dir: &File, name: &str) -> io::Result<()> {
        let name = c_name(OsStr::new(name))?;
        last_error_unless(unsafe { unlinkat(dir.as_raw_fd(), name.as_ptr(), 0) })
    }

    fn sync_dir(&self, dir: &File) -> io::Result<()> {
        dir.sync_all()
    }
}
