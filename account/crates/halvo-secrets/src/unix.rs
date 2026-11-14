use crate::place::{fault_with, EntryKind, Fs, Last, Meta, Opened, Published, Walk};
use std::collections::VecDeque;
use std::ffi::{CString, OsStr, OsString};
use std::fs::File;
use std::io::{self, Read as _, Write as _};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::raw::{c_char, c_int, c_uint, c_void};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
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
const O_EXEC: c_int = 0x4000_0000;
/// A directory opened for search alone: a step that the person may pass but not list is
/// walked as the system itself would walk it.
const O_SEARCH: c_int = O_EXEC | O_DIRECTORY;
const AT_FDCWD: c_int = -2;
const AT_SYMLINK_NOFOLLOW: c_int = 0x0020;
const ENOENT: i32 = 2;
const EEXIST: i32 = 17;
const ENOTDIR: i32 = 20;
const ELOOP: i32 = 62;
const ENAMETOOLONG: i32 = 63;
const S_IFMT: u16 = 0o170_000;
const S_IFLNK: u16 = 0o120_000;
const RENAME_EXCL: c_uint = 0x0004;
const ACL_TYPE_EXTENDED: c_int = 0x0100;
const ACL_FIRST_ENTRY: c_int = 0;
const ACL_NEXT_ENTRY: c_int = -1;
const ACL_EXTENDED_ALLOW: c_int = 1;
const ACL_ENTRY_ONLY_INHERIT: c_int = 1 << 8;
const ACL_CHANGES_ENTRIES: [c_int; 6] = [1 << 2, 1 << 4, 1 << 5, 1 << 6, 1 << 12, 1 << 13];
const ID_TYPE_UID: c_int = 0;
const STAT_WORDS: usize = 64;
const PATH_MAX: usize = 1024;
/// The links that one walk follows at most: MAXSYMLINKS of <sys/param.h>, the bound of the
/// system's own lookup. The next one answers a fault with ELOOP, so a loop of links ends.
const MAX_LINKS: usize = 32;

const F_GETPATH: c_int = 50;

extern "C" {
    fn geteuid() -> u32;
    fn fcntl(fd: c_int, cmd: c_int, ...) -> c_int;
    fn openat(fd: c_int, path: *const c_char, flags: c_int, ...) -> c_int;
    fn mkdirat(fd: c_int, path: *const c_char, mode: u16) -> c_int;
    fn unlinkat(fd: c_int, path: *const c_char, flags: c_int) -> c_int;
    #[cfg_attr(target_arch = "x86_64", link_name = "fstatat$INODE64")]
    fn fstatat(fd: c_int, path: *const c_char, buf: *mut c_void, flags: c_int) -> c_int;
    fn readlinkat(fd: c_int, path: *const c_char, buf: *mut c_char, size: usize) -> isize;
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
    fn acl_get_flagset_np(entry: *mut c_void, flagset: *mut *mut c_void) -> c_int;
    fn acl_get_flag_np(flagset: *mut c_void, flag: c_int) -> c_int;
    fn acl_get_qualifier(entry: *mut c_void) -> *mut c_void;
    fn acl_free(object: *mut c_void) -> c_int;
    fn mbr_uuid_to_id(uuid: *const u8, id: *mut u32, kind: *mut c_int) -> c_int;
}

pub(crate) struct UnixFs;

struct Acl {
    any: bool,
    changes_entries: bool,
}

/// The head of the 64-bit-inode `struct stat` of <sys/stat.h>, the layout that `fstatat`
/// writes on arm64 and that `fstatat$INODE64` writes on x86_64.
#[repr(C)]
struct StatHead {
    dev: i32,
    mode: u16,
    nlink: u16,
    ino: u64,
    uid: u32,
    gid: u32,
}

fn failed_list() -> io::Error {
    fault_with(io::Error::last_os_error().raw_os_error().unwrap_or(-1))
}

/// Reads the access list of `file`. A list that cannot be read answers a fault, never "no
/// list". `changes_entries` is an allow entry that lets someone other than root and `user`
/// add, remove or rename the entries of this object, or change its list or its owner.
fn acl_of(file: &File, user: u32) -> io::Result<Acl> {
    // SAFETY: the descriptor is open for as long as `file` is borrowed here, and the list
    // that the call allocates is freed by acl_free below, the one place that frees it.
    let acl = unsafe { acl_get_fd_np(file.as_raw_fd(), ACL_TYPE_EXTENDED) };
    if acl.is_null() {
        let error = io::Error::last_os_error();
        return if error.raw_os_error() == Some(ENOENT) {
            Ok(Acl {
                any: false,
                changes_entries: false,
            })
        } else {
            Err(fault_with(error.raw_os_error().unwrap_or(-1)))
        };
    }
    let found = entries_of(acl, user);
    // SAFETY: `acl` came from acl_get_fd_np above, no entry, set or qualifier taken from it
    // outlives entries_of, and it is freed once.
    unsafe { acl_free(acl) };
    found
}

fn entries_of(acl: *mut c_void, user: u32) -> io::Result<Acl> {
    let mut found = Acl {
        any: false,
        changes_entries: false,
    };
    let mut entry = ptr::null_mut();
    let mut which = ACL_FIRST_ENTRY;
    // SAFETY: `acl` is the live list of the caller, which frees it after this returns; the
    // call writes only the local `entry`, which points into that list.
    while unsafe { acl_get_entry(acl, which, &mut entry) } == 0 {
        which = ACL_NEXT_ENTRY;
        found.any = true;
        if lets_another_change(entry, user)? {
            found.changes_entries = true;
        }
    }
    Ok(found)
}

/// Whether `entry` allows a right of `ACL_CHANGES_ENTRIES` on the object itself to someone
/// other than root and `user`. An entry that only children inherit grants nothing on the
/// object, and an entry that names root or the person alone grants no other user. A part of
/// the entry that cannot be read answers a fault.
fn lets_another_change(entry: *mut c_void, user: u32) -> io::Result<bool> {
    let mut tag = 0;
    // SAFETY: `entry` points into the caller's live list; the call writes the local `tag`.
    if unsafe { acl_get_tag_type(entry, &mut tag) } != 0 {
        return Err(failed_list());
    }
    if tag != ACL_EXTENDED_ALLOW {
        return Ok(false);
    }
    let mut permset = ptr::null_mut();
    // SAFETY: the same live entry; the call writes the local `permset`, which points into
    // the entry and is read only inside this call.
    if unsafe { acl_get_permset(entry, &mut permset) } != 0 {
        return Err(failed_list());
    }
    let mut changes = false;
    for perm in ACL_CHANGES_ENTRIES {
        // SAFETY: `permset` belongs to the live entry above, and the call only reads it.
        match unsafe { acl_get_perm_np(permset, perm) } {
            0 => {}
            1 => changes = true,
            _ => return Err(failed_list()),
        }
    }
    if !changes {
        return Ok(false);
    }
    let mut flagset = ptr::null_mut();
    // SAFETY: the same live entry; the call writes the local `flagset`, which points into
    // the entry and is read only inside this call.
    if unsafe { acl_get_flagset_np(entry, &mut flagset) } != 0 {
        return Err(failed_list());
    }
    // SAFETY: `flagset` belongs to the live entry above, and the call only reads it.
    match unsafe { acl_get_flag_np(flagset, ACL_ENTRY_ONLY_INHERIT) } {
        0 => {}
        1 => return Ok(false),
        _ => return Err(failed_list()),
    }
    // SAFETY: the same live entry; the call returns a copy of the entry's 16-byte qualifier
    // that acl_free releases below, or null.
    let qualifier = unsafe { acl_get_qualifier(entry) };
    if qualifier.is_null() {
        return Err(failed_list());
    }
    let mut id = 0u32;
    let mut kind: c_int = -1;
    // SAFETY: `qualifier` points at the 16 bytes of a uuid that stay valid until the
    // acl_free below; the call writes the locals `id` and `kind`.
    let resolved = unsafe { mbr_uuid_to_id(qualifier.cast(), &mut id, &mut kind) };
    // SAFETY: `qualifier` came from acl_get_qualifier, nothing reads it after the call
    // above, and this is its one release.
    unsafe { acl_free(qualifier) };
    if resolved != 0 {
        return Err(fault_with(resolved));
    }
    Ok(!(kind == ID_TYPE_UID && (id == 0 || id == user)))
}

/// The list is read before the metadata: where the person may not read an object's
/// security, macOS refuses its fstat too, and the failure is a failed read of its list.
fn meta_of(file: &File, user: u32) -> io::Result<Meta> {
    let acl = acl_of(file, user)?;
    let metadata = file.metadata()?;
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

fn open_fd(dir: c_int, name: &OsStr, flags: c_int) -> io::Result<File> {
    let name = c_name(name)?;
    // SAFETY: `dir` is AT_FDCWD or the descriptor of a File that the caller borrows for the
    // call, `name` is a NUL-terminated string that lives to the end of this function, and
    // the mode is passed as the promoted int that the variadic openat reads when O_CREAT is
    // set.
    let fd = unsafe { openat(dir, name.as_ptr(), flags | O_CLOEXEC, 0o600 as c_uint) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: openat returned a new descriptor that nothing else owns, so the File takes it
    // and closes it once.
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn open_at(dir: &File, name: &OsStr, flags: c_int) -> io::Result<File> {
    open_fd(dir.as_raw_fd(), name, flags)
}

fn open_root() -> io::Result<File> {
    open_fd(AT_FDCWD, OsStr::new("/"), O_SEARCH)
}

/// The entry `name` of `dir` itself, a link not followed.
fn entry_at(dir: &File, name: &OsStr) -> io::Result<StatHead> {
    let name = c_name(name)?;
    let mut space = [0u64; STAT_WORDS];
    // SAFETY: `space` holds 512 bytes aligned for u64, more than the 144 of the struct stat
    // that the call writes; `dir` and `name` are live for the call.
    let result = unsafe {
        fstatat(
            dir.as_raw_fd(),
            name.as_ptr(),
            space.as_mut_ptr().cast(),
            AT_SYMLINK_NOFOLLOW,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the call succeeded, so the buffer begins with a struct stat whose first 24
    // bytes are the fields of StatHead in this order; the buffer is aligned for its u64.
    Ok(unsafe { ptr::read(space.as_ptr().cast::<StatHead>()) })
}

/// The target of the link `name` in `dir`, read relative to the handle that the walk holds.
fn read_link_at(dir: &File, name: &OsStr) -> io::Result<PathBuf> {
    let name = c_name(name)?;
    let mut target = vec![0u8; PATH_MAX + 1];
    // SAFETY: `target` holds PATH_MAX + 1 bytes and the call writes at most that many; `dir`
    // and `name` are live for the call.
    let len = unsafe {
        readlinkat(
            dir.as_raw_fd(),
            name.as_ptr(),
            target.as_mut_ptr().cast(),
            target.len(),
        )
    };
    let Ok(len) = usize::try_from(len) else {
        return Err(io::Error::last_os_error());
    };
    if len > PATH_MAX {
        return Err(fault_with(ENAMETOOLONG));
    }
    target.truncate(len);
    Ok(PathBuf::from(OsString::from_vec(target)))
}

fn last_error_unless(result: c_int) -> io::Result<()> {
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

enum Step {
    Root,
    Up,
    Name(OsString),
}

fn steps_of(path: &Path) -> Vec<Step> {
    path.components()
        .filter_map(|step| match step {
            Component::RootDir => Some(Step::Root),
            Component::ParentDir => Some(Step::Up),
            Component::Normal(name) => Some(Step::Name(name.to_os_string())),
            Component::CurDir | Component::Prefix(_) => None,
        })
        .collect()
}

fn missing(ancestors: Vec<Meta>, code: i32) -> Walk<File> {
    Walk {
        ancestors,
        last: Last::Missing(code),
    }
}

impl Fs for UnixFs {
    type Dir = File;
    type Item = File;
    type Temp = File;

    fn user(&self) -> u32 {
        // SAFETY: geteuid takes no argument, touches no memory of ours and cannot fail.
        unsafe { geteuid() }
    }

    /// Walks the caller's path as given, from the root, one step relative to the handle of
    /// the step above, each step opened for search alone and never through a link. A step
    /// that is a link is read relative to the handle of the directory that holds it, which
    /// the walk passed and records; the link is recorded with its owner; and its target is
    /// walked from the root or from that directory in the same way. Every directory that the
    /// walk passes and every link that it follows is in `ancestors` for the rule.
    fn walk(&self, path: &Path) -> io::Result<Walk<File>> {
        let invalid = || io::Error::from(io::ErrorKind::InvalidInput);
        let last = path.file_name().ok_or_else(invalid)?;
        let parent = path.parent().ok_or_else(invalid)?;
        let user = self.user();
        let root = open_root()?;
        let mut ancestors = vec![meta_of(&root, user)?];
        let mut held = vec![root];
        let mut pending = VecDeque::from(steps_of(parent));
        let mut links = 0;
        while let Some(step) = pending.pop_front() {
            let name = match step {
                Step::Root => {
                    held.truncate(1);
                    continue;
                }
                Step::Up => {
                    if held.len() > 1 {
                        held.pop();
                    }
                    continue;
                }
                Step::Name(name) => name,
            };
            let here = held.last().ok_or_else(invalid)?;
            let error = match open_at(here, &name, O_SEARCH | O_NOFOLLOW) {
                Ok(child) => {
                    ancestors.push(meta_of(&child, user)?);
                    held.push(child);
                    continue;
                }
                Err(error) => error,
            };
            match error.raw_os_error() {
                Some(ENOENT) => return Ok(missing(ancestors, ENOENT)),
                Some(ENOTDIR | ELOOP) => {}
                _ => return Err(error),
            }
            let entry = match entry_at(here, &name) {
                Ok(entry) => entry,
                Err(error) if error.raw_os_error() == Some(ENOENT) => {
                    return Ok(missing(ancestors, ENOENT));
                }
                Err(error) => return Err(error),
            };
            if entry.mode & S_IFMT != S_IFLNK {
                ancestors.push(Meta::of_kind(EntryKind::Other));
                return Ok(missing(ancestors, ENOTDIR));
            }
            links += 1;
            if links > MAX_LINKS {
                return Err(fault_with(ELOOP));
            }
            ancestors.push(Meta {
                kind: EntryKind::Link,
                owner: entry.uid,
                mode: u32::from(entry.mode) & 0o7777,
                len: 0,
                acl: false,
                acl_write: false,
                links: u64::from(entry.nlink),
            });
            // Only root and the person, or in a sticky directory the link's owner, can put
            // another entry in place of the link that entry_at read: the rule takes `here`
            // only when no one else may change its entries, so a link of another owner
            // read here is refused with the rest.
            let target = read_link_at(here, &name)?;
            if target.as_os_str().is_empty() {
                return Ok(missing(ancestors, ENOENT));
            }
            for step in steps_of(&target).into_iter().rev() {
                pending.push_front(step);
            }
        }
        let parent = held.pop().ok_or_else(invalid)?;
        drop(held);
        let last = match open_at(&parent, last, O_RDONLY | O_NOFOLLOW | O_NONBLOCK) {
            Ok(handle) => {
                let meta = meta_of(&handle, user)?;
                if meta.kind == EntryKind::Directory {
                    Last::Dir(meta, handle)
                } else {
                    Last::NotADirectory
                }
            }
            Err(error) if error.raw_os_error() == Some(ENOENT) => Last::Absent(parent),
            Err(error) if error.raw_os_error() == Some(ELOOP) => Last::NotADirectory,
            Err(error) => return Err(error),
        };
        Ok(Walk { ancestors, last })
    }

    fn make_dir(&self, parent: &File, name: &OsStr) -> io::Result<()> {
        let name = c_name(name)?;
        // SAFETY: `parent` stays open through the call and `name` is a NUL-terminated string
        // that outlives it.
        if unsafe { mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) } == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(EEXIST) {
            Ok(())
        } else {
            Err(error)
        }
    }

    fn is_absent(&self, error: &io::Error) -> bool {
        error.raw_os_error() == Some(ENOENT)
    }

    fn exists(&self, dir: &File, name: &str) -> io::Result<bool> {
        match entry_at(dir, OsStr::new(name)) {
            Ok(_) => Ok(true),
            Err(error) if error.raw_os_error() == Some(ENOENT) => Ok(false),
            Err(error) => Err(error),
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
        meta_of(item, self.user())
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
        // SAFETY: `fd` belongs to the borrowed `dir`, which stays open through the call, and
        // both names are NUL-terminated strings that live to the end of this function.
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
        // SAFETY: `dir` stays open through the call and `name` is a NUL-terminated string
        // that outlives it.
        last_error_unless(unsafe { unlinkat(dir.as_raw_fd(), name.as_ptr(), 0) })
    }

    fn sync_dir(&self, dir: &File) -> io::Result<()> {
        dir.sync_all()
    }

    fn names(&self, dir: &File) -> io::Result<Vec<String>> {
        let mut path = vec![0u8; PATH_MAX];
        // SAFETY: `dir` stays open through the call, and F_GETPATH writes at most MAXPATHLEN
        // bytes, its NUL included, into the buffer, which holds PATH_MAX (MAXPATHLEN) bytes.
        if unsafe { fcntl(dir.as_raw_fd(), F_GETPATH, path.as_mut_ptr()) } < 0 {
            return Err(io::Error::last_os_error());
        }
        let len = path
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(path.len());
        path.truncate(len);
        let mut names = Vec::new();
        for entry in std::fs::read_dir(PathBuf::from(OsString::from_vec(path)))? {
            if let Ok(name) = entry?.file_name().into_string() {
                names.push(name);
            }
        }
        Ok(names)
    }
}
